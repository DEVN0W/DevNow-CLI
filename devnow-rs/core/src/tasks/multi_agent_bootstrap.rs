use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use codex_protocol::config_types::ModeKind;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::CollabAgentRef;
use codex_protocol::protocol::CollabAgentSpawnBeginEvent;
use codex_protocol::protocol::CollabAgentSpawnEndEvent;
use codex_protocol::protocol::CollabAgentStatusEntry;
use codex_protocol::protocol::CollabWaitingBeginEvent;
use codex_protocol::protocol::CollabWaitingEndEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption;
use codex_protocol::request_user_input::RequestUserInputResponse;
use codex_protocol::user_input::UserInput;
use futures::future::join_all;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;

use crate::agent::control::SpawnAgentOptions;
use crate::agent::exceeds_thread_spawn_depth_limit;
use crate::agent::next_thread_spawn_depth;
use crate::agent::status::is_final;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::connectors::sanitize_name;
use crate::models_manager::multi_agent_team::MultiAgentAgentConfig;
use crate::models_manager::multi_agent_team::MultiAgentTeamConfig;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskContext;
use crate::tools::handlers::multi_agents_common::apply_multi_agent_team_overrides;
use crate::tools::handlers::multi_agents_common::build_agent_spawn_config;
use crate::tools::handlers::multi_agents_common::input_preview;
use crate::tools::handlers::multi_agents_common::thread_spawn_source;
use uuid::Uuid;

#[derive(Clone, Copy, Default)]
pub(crate) struct MultiAgentBootstrapTask;

impl MultiAgentBootstrapTask {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SessionTask for MultiAgentBootstrapTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "session_task.multi_agent_bootstrap"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        let session_handle = session.clone_session();
        let event = EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: ctx.sub_id.clone(),
            model_context_window: ctx.model_context_window(),
            collaboration_mode_kind: ctx.collaboration_mode.mode,
        });
        session_handle.send_event(ctx.as_ref(), event).await;
        session_handle
            .set_server_reasoning_included(/*included*/ false)
            .await;

        if ctx.collaboration_mode.mode != ModeKind::MultiAgent {
            warn!("multi-agent bootstrap task invoked outside multi-agent mode");
            return None;
        }

        let Some(team) = session.models_manager().multi_agent_team().cloned() else {
            warn!(
                "multi-agent mode is enabled, but no multi_agent.toml roster was loaded from CODEX_HOME"
            );
            return Arc::new(crate::tasks::RegularTask::new())
                .run(session, ctx, input, cancellation_token)
                .await;
        };
        let mut stage_history = Vec::new();

        let child_depth = next_thread_spawn_depth(&ctx.session_source);
        if exceeds_thread_spawn_depth_limit(child_depth, ctx.config.agent_max_depth) {
            warn!(
                "multi-agent roster cannot spawn at depth {child_depth}; falling back to regular chat"
            );
            return Arc::new(crate::tasks::RegularTask::new())
                .run(session, ctx, input, cancellation_token)
                .await;
        }

        let ordered_agents = ordered_multi_agent_roster(&team);
        if ordered_agents.is_empty() {
            warn!("multi-agent roster does not define any runnable agents");
            return Arc::new(crate::tasks::RegularTask::new())
                .run(session, ctx, input, cancellation_token)
                .await;
        }

        let base_instructions = session_handle.get_base_instructions().await;
        let base_spawn_config = match build_agent_spawn_config(&base_instructions, ctx.as_ref()) {
            Ok(config) => config,
            Err(err) => {
                warn!(
                    "failed to build multi-agent spawn config: {err}; falling back to regular chat"
                );
                return Arc::new(crate::tasks::RegularTask::new())
                    .run(session, ctx, input, cancellation_token)
                    .await;
            }
        };
        let prompt = stage_spawn_preview(&input);
        let locked_workspace_path =
            infer_locked_workspace_path(&input, &ctx.cwd.display().to_string());
        let reasoning_effort = ctx.reasoning_effort.unwrap_or_default();
        let Some(initial_agent) = ordered_agents.first().cloned() else {
            return None;
        };
        if cancellation_token.is_cancelled() {
            return None;
        }

        if initial_agent.role.eq_ignore_ascii_case("architect") {
            let architect_input = stage_prompt_input(
                &initial_agent,
                input.clone(),
                None,
                true,
                &locked_workspace_path,
            );
            let architect_preview = stage_spawn_preview(&architect_input);
            let architect_result = spawn_multi_agent_stage(
                session_handle.as_ref(),
                ctx.as_ref(),
                &base_spawn_config,
                &base_instructions.text,
                &initial_agent,
                architect_input,
                child_depth,
                architect_preview,
                reasoning_effort,
            )
            .await;

            let architect_spawned = match architect_result {
                Ok(spawned_agent) => spawned_agent,
                Err(err) => {
                    warn!(
                        "failed to start initial multi-agent stage `{}`: {err}; falling back to regular chat",
                        initial_agent.name
                    );
                    return Arc::new(crate::tasks::RegularTask::new())
                        .run(session, ctx, input, cancellation_token)
                        .await;
                }
            };

            let architect_wait_call_id =
                format!("multi_agent_wait_{}_{}", ctx.sub_id, initial_agent.name);
            session_handle
                .send_event(
                    ctx.as_ref(),
                    EventMsg::CollabWaitingBegin(CollabWaitingBeginEvent {
                        sender_thread_id: session_handle.conversation_id,
                        receiver_thread_ids: vec![architect_spawned.thread_id],
                        receiver_agents: vec![CollabAgentRef {
                            thread_id: architect_spawned.thread_id,
                            agent_nickname: architect_spawned.metadata.agent_nickname.clone(),
                            agent_role: architect_spawned.metadata.agent_role.clone(),
                        }],
                        call_id: architect_wait_call_id.clone(),
                    }),
                )
                .await;

            let architect_status = wait_for_agent_completion(
                session_handle.as_ref(),
                architect_spawned.thread_id,
                cancellation_token.clone(),
            )
            .await;
            if let Some(status) = architect_status.as_ref() {
                session_handle
                    .send_event(
                        ctx.as_ref(),
                        EventMsg::CollabWaitingEnd(CollabWaitingEndEvent {
                            sender_thread_id: session_handle.conversation_id,
                            call_id: architect_wait_call_id,
                            agent_statuses: vec![CollabAgentStatusEntry {
                                thread_id: architect_spawned.thread_id,
                                agent_nickname: architect_spawned.metadata.agent_nickname.clone(),
                                agent_role: architect_spawned.metadata.agent_role.clone(),
                                status: status.clone(),
                            }],
                            statuses: HashMap::from([(
                                architect_spawned.thread_id,
                                status.clone(),
                            )]),
                        }),
                    )
                    .await;
            }
            if let Some(status) = architect_status.clone() {
                stage_history.push(StageHistoryRecord::new(
                    architect_spawned
                        .metadata
                        .agent_nickname
                        .clone()
                        .unwrap_or_else(|| initial_agent.name.clone()),
                    initial_agent.role.clone(),
                    status,
                    Some("parent".to_string()),
                ));
            }
            let architect_summary = match architect_status.clone() {
                Some(AgentStatus::Completed(Some(message))) => message,
                Some(AgentStatus::Completed(None)) => {
                    return Some(format!(
                        "Architect `{}` finished without a design summary, so bootstrap could not request approval.",
                        initial_agent.name
                    ));
                }
                Some(AgentStatus::Errored(message)) => {
                    return Some(format!(
                        "Architect `{}` failed before approval: {message}",
                        initial_agent.name
                    ));
                }
                Some(status) => {
                    return Some(format!(
                        "Architect `{}` stopped before approval with status `{status:?}`.",
                        initial_agent.name
                    ));
                }
                None => return None,
            };

            let Some(architect_approved) = request_architect_approval(
                session_handle.as_ref(),
                ctx.as_ref(),
                &initial_agent,
                &architect_summary,
            )
            .await
            else {
                return Some(format!(
                    "Architect design approval was cancelled before the multi-agent flow could continue."
                ));
            };

            if !architect_approved {
                info!(
                    "architect {} [{}] was not approved; pausing multi-agent flow",
                    initial_agent.name, initial_agent.role
                );
                return Some(format!(
                    "Architect design was not approved yet. The flow is paused until the architect design is revised and approved."
                ));
            }

            let Some(next_agent) = team
                .developer_agent()
                .cloned()
                .or_else(|| ordered_agents.get(1).cloned())
            else {
                return Some(format!(
                    "Architect design was approved for `{}`. No downstream agent is configured after `{}`.",
                    prompt, initial_agent.name
                ));
            };

            let downstream_input = stage_prompt_input(
                &next_agent,
                input.clone(),
                Some(compact_handoff_context(&format!(
                    "Approved architect design from {} [{}]:\n\n{}",
                    architect_spawned
                        .metadata
                        .agent_nickname
                        .clone()
                        .unwrap_or_else(|| initial_agent.name.clone()),
                    initial_agent.role,
                    architect_summary
                ))),
                false,
                &locked_workspace_path,
            );
            match run_multi_agent_chain(
                session_handle.as_ref(),
                ctx.as_ref(),
                &base_spawn_config,
                &base_instructions.text,
                &team,
                &ordered_agents,
                next_agent,
                downstream_input,
                child_depth,
                reasoning_effort,
                &locked_workspace_path,
                &mut stage_history,
                cancellation_token,
            )
            .await
            {
                Ok(message) => Some(message),
                Err(err) => Some(format!(
                    "Architect design was approved, but the multi-agent chain failed: {err}"
                )),
            }
        } else {
            let initial_input =
                stage_prompt_input(&initial_agent, input, None, true, &locked_workspace_path);
            info!(
                "multi-agent flow starting with {} [{}] and continuing through the configured handoff chain",
                initial_agent.name, initial_agent.role
            );
            match run_multi_agent_chain(
                session_handle.as_ref(),
                ctx.as_ref(),
                &base_spawn_config,
                &base_instructions.text,
                &team,
                &ordered_agents,
                initial_agent,
                initial_input,
                child_depth,
                reasoning_effort,
                &locked_workspace_path,
                &mut stage_history,
                cancellation_token,
            )
            .await
            {
                Ok(message) => Some(message),
                Err(err) => Some(format!("multi-agent chain failed: {err}")),
            }
        }
    }
}

async fn run_multi_agent_chain(
    session_handle: &Session,
    ctx: &TurnContext,
    base_spawn_config: &crate::config::Config,
    base_instructions: &str,
    team: &MultiAgentTeamConfig,
    ordered_agents: &[MultiAgentAgentConfig],
    mut current_agent: MultiAgentAgentConfig,
    root_input: Vec<UserInput>,
    child_depth: i32,
    reasoning_effort: codex_protocol::openai_models::ReasoningEffort,
    locked_workspace_path: &str,
    stage_history: &mut Vec<StageHistoryRecord>,
    cancellation_token: CancellationToken,
) -> anyhow::Result<String> {
    let root_input = root_input;
    let mut handoff_context: Option<String> = None;
    let mut open_findings = OpenFindingsLedger::default();
    let max_rounds = ordered_agents.len().saturating_mul(4).max(12);
    let mut rounds = 0usize;
    loop {
        rounds += 1;
        if rounds > max_rounds {
            return Ok(render_production_summary(
                stage_history.as_slice(),
                &open_findings,
                &format!("stopped after {rounds} rounds to avoid an infinite loop"),
                false,
            ));
        }
        if cancellation_token.is_cancelled() {
            return Ok(render_production_summary(
                stage_history.as_slice(),
                &open_findings,
                &format!(
                    "cancelled while waiting for `{}` [{}]",
                    current_agent.name, current_agent.role
                ),
                false,
            ));
        }

        let current_handoff_context = merge_handoff_context(
            handoff_context.as_deref(),
            open_findings.render_for_agent(&current_agent),
        );
        let current_handoff_context = enrich_handoff_context_for_agent(
            &current_agent,
            stage_history.as_slice(),
            &open_findings,
            current_handoff_context,
        );
        let current_input = stage_prompt_input(
            &current_agent,
            root_input.clone(),
            current_handoff_context,
            handoff_context.is_none(),
            locked_workspace_path,
        );
        let current_preview = stage_spawn_preview(&current_input);
        let spawned_agent = match spawn_multi_agent_stage(
            session_handle,
            ctx,
            base_spawn_config,
            base_instructions,
            &current_agent,
            current_input.clone(),
            child_depth,
            current_preview,
            reasoning_effort,
        )
        .await
        {
            Ok(spawned_agent) => spawned_agent,
            Err(err) => {
                let status = AgentStatus::Errored(err.to_string());
                let next_agent_name =
                    next_agent_name_for_status(ordered_agents, &current_agent, &status);
                let Some(next_agent_name) = next_agent_name else {
                    stage_history.push(StageHistoryRecord::new(
                        current_agent.name.clone(),
                        current_agent.role.clone(),
                        status.clone(),
                        None,
                    ));
                    return Ok(render_production_summary(
                        stage_history.as_slice(),
                        &open_findings,
                        &format!(
                            "failed to spawn {} [{}] and had no fallback route",
                            current_agent.name, current_agent.role
                        ),
                        false,
                    ));
                };
                let Some(next_agent) = team.agent_for_role(&next_agent_name).cloned() else {
                    stage_history.push(StageHistoryRecord::new(
                        current_agent.name.clone(),
                        current_agent.role.clone(),
                        status.clone(),
                        Some(next_agent_name.clone()),
                    ));
                    return Ok(render_production_summary(
                        stage_history.as_slice(),
                        &open_findings,
                        &format!(
                            "failed to spawn {} [{}], and the fallback route `{next_agent_name}` could not be found",
                            current_agent.name, current_agent.role
                        ),
                        false,
                    ));
                };
                if next_agent.name.eq_ignore_ascii_case(&current_agent.name) {
                    stage_history.push(StageHistoryRecord::new(
                        current_agent.name.clone(),
                        current_agent.role.clone(),
                        status.clone(),
                        Some(next_agent.name.clone()),
                    ));
                    return Ok(render_production_summary(
                        stage_history.as_slice(),
                        &open_findings,
                        &format!(
                            "failed to spawn {} [{}] and the fallback route pointed back to the same agent",
                            current_agent.name, current_agent.role
                        ),
                        false,
                    ));
                }
                open_findings.record_status(&current_agent, &status, Some(&err.to_string()));
                handoff_context = Some(compact_handoff_context(&format!(
                    "Failed to spawn {} [{}]: {}\n\nRoute to {} [{}] for recovery.",
                    current_agent.name, current_agent.role, err, next_agent.name, next_agent.role
                )));
                current_agent = next_agent;
                continue;
            }
        };
        info!(
            "multi-agent stage {} [{}] spawned on thread {}",
            current_agent.name, current_agent.role, spawned_agent.thread_id
        );

        let last_spawned_agent_name = spawned_agent
            .metadata
            .agent_nickname
            .clone()
            .unwrap_or_else(|| current_agent.name.clone());

        let wait_call_id = format!("multi_agent_wait_{}_{}", ctx.sub_id, current_agent.name);
        session_handle
            .send_event(
                ctx,
                EventMsg::CollabWaitingBegin(CollabWaitingBeginEvent {
                    sender_thread_id: session_handle.conversation_id,
                    receiver_thread_ids: vec![spawned_agent.thread_id],
                    receiver_agents: vec![CollabAgentRef {
                        thread_id: spawned_agent.thread_id,
                        agent_nickname: spawned_agent.metadata.agent_nickname.clone(),
                        agent_role: spawned_agent.metadata.agent_role.clone(),
                    }],
                    call_id: wait_call_id.clone(),
                }),
            )
            .await;

        let status = wait_for_agent_completion(
            session_handle,
            spawned_agent.thread_id,
            cancellation_token.clone(),
        )
        .await;
        if let Some(status) = status.as_ref() {
            session_handle
                .send_event(
                    ctx,
                    EventMsg::CollabWaitingEnd(CollabWaitingEndEvent {
                        sender_thread_id: session_handle.conversation_id,
                        call_id: wait_call_id,
                        agent_statuses: vec![CollabAgentStatusEntry {
                            thread_id: spawned_agent.thread_id,
                            agent_nickname: spawned_agent.metadata.agent_nickname.clone(),
                            agent_role: spawned_agent.metadata.agent_role.clone(),
                            status: status.clone(),
                        }],
                        statuses: HashMap::from([(spawned_agent.thread_id, status.clone())]),
                    }),
                )
                .await;
        }
        let Some(status) = status else {
            return Ok(format!(
                "Multi-agent flow stopped while waiting for {} [{}] at {}.",
                current_agent.name, current_agent.role, spawned_agent.thread_id
            ));
        };
        let routed_status = status_for_routing(&status);
        open_findings.record_status(&current_agent, &routed_status, None);

        if matches!(routed_status, AgentStatus::Completed(_))
            && should_run_parallel_review_stage(team, &current_agent)
        {
            let review_agents = team.parallel_review_agents();
            if !review_agents.is_empty() {
                let review_result = run_parallel_review_stage(
                    session_handle,
                    ctx,
                    base_spawn_config,
                    base_instructions,
                    team,
                    ordered_agents,
                    &current_agent,
                    &spawned_agent,
                    &status,
                    root_input.as_slice(),
                    review_agents,
                    child_depth,
                    reasoning_effort,
                    locked_workspace_path,
                    &mut open_findings,
                    stage_history,
                    cancellation_token.clone(),
                )
                .await?;

                current_agent = review_result.next_agent;
                handoff_context = Some(review_result.next_handoff);
                continue;
            }
        }

        let next_agent_name =
            next_agent_name_for_status(ordered_agents, &current_agent, &routed_status);
        let Some(next_agent_name) = next_agent_name else {
            stage_history.push(StageHistoryRecord::new(
                last_spawned_agent_name.clone(),
                current_agent.role.clone(),
                routed_status.clone(),
                None,
            ));
            return Ok(render_production_summary(
                stage_history.as_slice(),
                &open_findings,
                &format!(
                    "completed at {} [{}] with status `{routed_status:?}`",
                    last_spawned_agent_name, current_agent.role
                ),
                matches!(routed_status, AgentStatus::Completed(_)),
            ));
        };
        let Some(next_agent) = team.agent_for_role(&next_agent_name).cloned() else {
            stage_history.push(StageHistoryRecord::new(
                last_spawned_agent_name.clone(),
                current_agent.role.clone(),
                routed_status.clone(),
                Some(next_agent_name.clone()),
            ));
            return Ok(render_production_summary(
                stage_history.as_slice(),
                &open_findings,
                &format!(
                    "reached {} [{}], but the configured next agent `{next_agent_name}` could not be found",
                    current_agent.name, current_agent.role
                ),
                matches!(routed_status, AgentStatus::Completed(_)),
            ));
        };
        info!(
            "multi-agent stage {} [{}] completed with {status:?}; forwarding to {} [{}]",
            last_spawned_agent_name, current_agent.role, next_agent.name, next_agent.role
        );

        let next_input_summary = stage_handoff_summary(
            &current_agent,
            &last_spawned_agent_name,
            &routed_status,
            Some(&next_agent.name),
        );
        let stage_context = stage_completion_context(
            session_handle,
            &current_agent,
            &spawned_agent,
            &routed_status,
        )
        .await;
        stage_history.push(StageHistoryRecord::new(
            last_spawned_agent_name.clone(),
            current_agent.role.clone(),
            routed_status.clone(),
            Some(next_agent.name.clone()),
        ));
        let next_handoff_context = merge_handoff_context(
            Some(&next_input_summary),
            merge_handoff_context(
                Some(&stage_context),
                open_findings.render_for_agent(&next_agent),
            ),
        );
        handoff_context = next_handoff_context.map(|text| compact_handoff_context(&text));
        current_agent = next_agent;
    }
}

fn next_agent_name_for_status(
    ordered_agents: &[MultiAgentAgentConfig],
    current_agent: &MultiAgentAgentConfig,
    status: &AgentStatus,
) -> Option<String> {
    let routed_status = status_for_routing(status);
    let explicit_route = match &routed_status {
        AgentStatus::Completed(_) => current_agent.next_on_pass.as_deref(),
        AgentStatus::Errored(_)
        | AgentStatus::Interrupted
        | AgentStatus::NotFound
        | AgentStatus::Shutdown
        | AgentStatus::PendingInit
        | AgentStatus::Running => current_agent.next_on_fail.as_deref(),
    };
    if let Some(route) = explicit_route {
        return Some(route.to_string());
    }

    if let Some(index) = ordered_agents
        .iter()
        .position(|agent| agent.name.eq_ignore_ascii_case(&current_agent.name))
        && let Some(next_agent) = ordered_agents.get(index + 1)
    {
        return Some(next_agent.name.clone());
    }

    None
}

fn status_for_routing(status: &AgentStatus) -> AgentStatus {
    match status {
        AgentStatus::Completed(Some(summary)) if completed_summary_requests_fail(summary) => {
            AgentStatus::Errored(summary.clone())
        }
        _ => status.clone(),
    }
}

fn completed_summary_requests_fail(summary: &str) -> bool {
    summary.lines().any(|line| {
        let line = line.trim_start();
        let Some(value) = line.strip_prefix("status:") else {
            return false;
        };
        value.trim().eq_ignore_ascii_case("fail")
    })
}

fn next_agent_name_after_pass(
    ordered_agents: &[MultiAgentAgentConfig],
    current_agent: &MultiAgentAgentConfig,
) -> Option<String> {
    if let Some(route) = current_agent.next_on_pass.as_deref() {
        return Some(route.to_string());
    }

    if let Some(index) = ordered_agents
        .iter()
        .position(|agent| agent.name.eq_ignore_ascii_case(&current_agent.name))
        && let Some(next_agent) = ordered_agents.get(index + 1)
    {
        return Some(next_agent.name.clone());
    }

    None
}

fn should_run_parallel_review_stage(
    team: &MultiAgentTeamConfig,
    current_agent: &MultiAgentAgentConfig,
) -> bool {
    team.developer_agent().is_some_and(|developer| {
        developer.name.eq_ignore_ascii_case(&current_agent.name)
            || developer.role.eq_ignore_ascii_case(&current_agent.role)
    }) && !team.parallel_review_agents().is_empty()
}

struct ParallelReviewStageResult {
    next_agent: MultiAgentAgentConfig,
    next_handoff: String,
}

#[derive(Clone, Debug)]
struct StageHistoryRecord {
    agent_name: String,
    role: String,
    status: AgentStatus,
    forwarded_to: Option<String>,
}

impl StageHistoryRecord {
    fn new(
        agent_name: String,
        role: String,
        status: AgentStatus,
        forwarded_to: Option<String>,
    ) -> Self {
        Self {
            agent_name,
            role,
            status,
            forwarded_to,
        }
    }

    fn status_label(&self) -> &'static str {
        match status_for_routing(&self.status) {
            AgentStatus::Completed(_) => "pass",
            AgentStatus::Errored(_)
            | AgentStatus::Interrupted
            | AgentStatus::NotFound
            | AgentStatus::Shutdown
            | AgentStatus::PendingInit
            | AgentStatus::Running => "fail",
        }
    }

    fn completed_text(&self) -> String {
        status_detail_text(&self.status)
    }
}

#[derive(Default)]
struct OpenFindingsLedger {
    by_role: BTreeMap<String, String>,
}

impl OpenFindingsLedger {
    fn record_status(
        &mut self,
        agent: &MultiAgentAgentConfig,
        status: &AgentStatus,
        fallback_detail: Option<&str>,
    ) {
        let role_key = agent.role.trim().to_ascii_lowercase();
        match status {
            AgentStatus::Completed(_) => {
                self.by_role.remove(&role_key);
            }
            AgentStatus::Errored(message) => {
                let finding = fallback_detail.unwrap_or(message);
                self.by_role.insert(role_key, normalize_finding(finding));
            }
            AgentStatus::Interrupted
            | AgentStatus::NotFound
            | AgentStatus::Shutdown
            | AgentStatus::PendingInit
            | AgentStatus::Running => {
                let finding = fallback_detail.unwrap_or("stage did not complete");
                self.by_role.insert(role_key, normalize_finding(finding));
            }
        }
    }

    fn render(&self) -> Option<String> {
        if self.by_role.is_empty() {
            return None;
        }

        let mut lines = vec!["Open unresolved findings:".to_string()];
        let mut grouped_findings: Vec<(u8, String, BTreeSet<String>)> = Vec::new();
        for (role, finding) in &self.by_role {
            let priority = role_feedback_priority(role);
            if let Some((current_priority, _, roles)) = grouped_findings
                .iter_mut()
                .find(|(_, grouped_finding, _)| grouped_finding == finding)
            {
                *current_priority = (*current_priority).min(priority);
                roles.insert(role.clone());
                continue;
            }
            let mut roles = BTreeSet::new();
            roles.insert(role.clone());
            grouped_findings.push((priority, finding.clone(), roles));
        }

        grouped_findings.sort_by(
            |(priority_a, finding_a, roles_a), (priority_b, finding_b, roles_b)| {
                priority_a
                    .cmp(priority_b)
                    .then_with(|| roles_a.len().cmp(&roles_b.len()))
                    .then_with(|| finding_a.cmp(finding_b))
            },
        );

        for (_priority, finding, roles) in grouped_findings {
            let mut roles = roles.into_iter().collect::<Vec<_>>();
            roles.sort_by_key(|role| role_feedback_priority(role));
            let roles = roles.join(", ");
            lines.push(format!("- roles: {roles}"));
            lines.push(indent_block(&finding));
        }

        Some(lines.join("\n"))
    }

    fn render_for_agent(&self, agent: &MultiAgentAgentConfig) -> Option<String> {
        self.render().map(|block| {
            format!(
                "Cumulative unresolved findings for {} [{}]:\n{}\n\nTreat these as the prioritized open issue queue. Work the highest-priority blockers first, carry the remaining backlog forward, and close the whole set before marking the developer stage complete.",
                agent.name, agent.role, block
            )
        })
    }
}

fn role_feedback_priority(role: &str) -> u8 {
    match role.trim().to_ascii_lowercase().as_str() {
        "security" => 0,
        "execution tester" => 1,
        "tester" => 2,
        "quality" => 3,
        "critic" => 4,
        "judge" => 5,
        "architect" => 6,
        "planner" => 7,
        "developer" | "coder" | "worker" => 8,
        _ => 9,
    }
}

fn normalize_finding(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(80)
        .map(|line| truncate_line(line, 320))
        .collect::<Vec<_>>()
        .join("\n")
}

fn indent_block(text: &str) -> String {
    text.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn merge_handoff_context(summary: Option<&str>, findings: Option<String>) -> Option<String> {
    match (summary, findings) {
        (None, None) => None,
        (Some(summary), None) => Some(summary.to_string()),
        (None, Some(findings)) => Some(findings),
        (Some(summary), Some(findings)) => Some(format!("{summary}\n\n{findings}")),
    }
}

async fn run_parallel_review_stage(
    session_handle: &Session,
    ctx: &TurnContext,
    base_spawn_config: &crate::config::Config,
    base_instructions: &str,
    team: &MultiAgentTeamConfig,
    ordered_agents: &[MultiAgentAgentConfig],
    developer_agent: &MultiAgentAgentConfig,
    developer_spawned: &crate::agent::control::LiveAgent,
    developer_status: &AgentStatus,
    root_input: &[UserInput],
    review_agents: Vec<&MultiAgentAgentConfig>,
    child_depth: i32,
    reasoning_effort: codex_protocol::openai_models::ReasoningEffort,
    locked_workspace_path: &str,
    open_findings: &mut OpenFindingsLedger,
    stage_history: &mut Vec<StageHistoryRecord>,
    cancellation_token: CancellationToken,
) -> anyhow::Result<ParallelReviewStageResult> {
    let review_context = stage_completion_context(
        session_handle,
        developer_agent,
        developer_spawned,
        developer_status,
    )
    .await;

    let mut spawned_agents = Vec::with_capacity(review_agents.len());
    let mut report_lines = Vec::with_capacity(review_agents.len());
    let total_reviews = review_agents.len();
    let mut passed_reviews = 0usize;
    let threshold_percent = team.pass_threshold_percent();
    info!(
        "multi-agent parallel review starting for {} [{}] with {} reviewer(s) and a {}% pass threshold",
        developer_spawned
            .metadata
            .agent_nickname
            .clone()
            .unwrap_or_else(|| developer_agent.name.clone()),
        developer_agent.role,
        total_reviews,
        threshold_percent
    );
    for &review_agent in &review_agents {
        let review_input = stage_prompt_input(
            review_agent,
            root_input.to_vec(),
            Some(review_context.clone()),
            false,
            locked_workspace_path,
        );
        let review_preview = stage_spawn_preview(&review_input);
        match spawn_multi_agent_stage(
            session_handle,
            ctx,
            base_spawn_config,
            base_instructions,
            review_agent,
            review_input.clone(),
            child_depth,
            review_preview,
            reasoning_effort,
        )
        .await
        {
            Ok(spawned_agent) => spawned_agents.push((review_agent.clone(), spawned_agent)),
            Err(err) => {
                stage_history.push(StageHistoryRecord::new(
                    review_agent.name.clone(),
                    review_agent.role.clone(),
                    AgentStatus::NotFound,
                    Some(developer_agent.name.clone()),
                ));
                report_lines.push(format!(
                    "- {} [{}]: spawn failed\n  {}",
                    review_agent.name.clone(),
                    review_agent.role,
                    err
                ));
            }
        }
    }

    let mut statuses = std::collections::HashMap::with_capacity(spawned_agents.len());
    let mut agent_statuses = Vec::with_capacity(spawned_agents.len());
    let mut highest_priority_failed_review: Option<MultiAgentAgentConfig> = None;
    let mut highest_priority_failed_review_priority = u8::MAX;
    if !spawned_agents.is_empty() {
        let receiver_thread_ids = spawned_agents
            .iter()
            .map(|(_, spawned_agent)| spawned_agent.thread_id)
            .collect::<Vec<_>>();
        let receiver_agents = spawned_agents
            .iter()
            .map(|(_agent, spawned_agent)| CollabAgentRef {
                thread_id: spawned_agent.thread_id,
                agent_nickname: spawned_agent.metadata.agent_nickname.clone(),
                agent_role: spawned_agent.metadata.agent_role.clone(),
            })
            .collect::<Vec<_>>();
        let waiting_call_id = format!(
            "multi_agent_parallel_{}_{}",
            ctx.sub_id, developer_agent.name
        );
        session_handle
            .send_event(
                ctx,
                EventMsg::CollabWaitingBegin(CollabWaitingBeginEvent {
                    sender_thread_id: session_handle.conversation_id,
                    receiver_thread_ids: receiver_thread_ids.clone(),
                    receiver_agents,
                    call_id: waiting_call_id.clone(),
                }),
            )
            .await;

        let review_futures = spawned_agents
            .into_iter()
            .map(|(review_agent, spawned_agent)| {
                let cancellation_token = cancellation_token.clone();
                async move {
                    let status = wait_for_agent_completion(
                        session_handle,
                        spawned_agent.thread_id,
                        cancellation_token.clone(),
                    )
                    .await;
                    (review_agent, spawned_agent, status)
                }
            });
        let outcomes = join_all(review_futures).await;

        for (review_agent, spawned_agent, status) in outcomes {
            let status = status.unwrap_or(AgentStatus::Shutdown);
            let status_summary = match &status {
                AgentStatus::Completed(Some(summary)) => summary.clone(),
                AgentStatus::Completed(None) => "completed without a summary".to_string(),
                AgentStatus::Errored(message) => message.clone(),
                other => format!("{other:?}"),
            };
            let required_grade = team.passing_grade_for(&review_agent);
            let reported_grade = extract_review_grade(&status_summary);
            let review_passed = review_grade_passed(reported_grade, required_grade);
            let normalized_review_summary =
                normalize_review_summary(&status_summary, review_passed);
            let review_status = if review_passed {
                AgentStatus::Completed(Some(normalized_review_summary.clone()))
            } else {
                AgentStatus::Errored(match reported_grade {
                    Some(grade) => format!("review grade {grade} below required {required_grade}"),
                    None => format!(
                        "missing review grade; expected a numeric grade of at least {required_grade}"
                    ),
                })
            };
            let review_detail = match &review_status {
                AgentStatus::Errored(message) => {
                    format!("{normalized_review_summary}\n\n{message}")
                }
                _ => normalized_review_summary.clone(),
            };
            open_findings.record_status(&review_agent, &review_status, Some(&review_detail));
            statuses.insert(spawned_agent.thread_id, review_status.clone());
            agent_statuses.push(CollabAgentStatusEntry {
                thread_id: spawned_agent.thread_id,
                agent_nickname: spawned_agent.metadata.agent_nickname.clone(),
                agent_role: spawned_agent.metadata.agent_role.clone(),
                status: review_status.clone(),
            });
            stage_history.push(StageHistoryRecord::new(
                spawned_agent
                    .metadata
                    .agent_nickname
                    .clone()
                    .unwrap_or_else(|| review_agent.name.clone()),
                review_agent.role.clone(),
                review_status.clone(),
                Some(developer_agent.name.clone()),
            ));
            report_lines.push(format!(
                "- {} [{}]: {review_status:?}\n  {status_summary}\n  grade: {:?}\n  required_grade: {required_grade}",
                spawned_agent
                    .metadata
                    .agent_nickname
                    .clone()
                    .unwrap_or_else(|| review_agent.name.clone()),
                review_agent.role,
                reported_grade
            ));
            if review_passed {
                passed_reviews += 1;
            } else {
                let priority = role_feedback_priority(review_agent.role.as_str());
                if priority <= highest_priority_failed_review_priority {
                    highest_priority_failed_review = Some(review_agent.clone());
                    highest_priority_failed_review_priority = priority;
                }
            }
        }

        session_handle
            .send_event(
                ctx,
                EventMsg::CollabWaitingEnd(CollabWaitingEndEvent {
                    sender_thread_id: session_handle.conversation_id,
                    call_id: waiting_call_id,
                    agent_statuses: agent_statuses.clone(),
                    statuses: statuses.clone(),
                }),
            )
            .await;
    }
    info!(
        "multi-agent parallel review completed for {} [{}] with {passed_reviews}/{total_reviews} reviewer pass(es)",
        developer_spawned
            .metadata
            .agent_nickname
            .clone()
            .unwrap_or_else(|| developer_agent.name.clone()),
        developer_agent.role
    );

    let review_summary = report_lines.join("\n");
    let approval_percent = if total_reviews == 0 {
        0
    } else {
        ((passed_reviews * 100) / total_reviews) as u8
    };
    let approved = approval_threshold_met(passed_reviews, total_reviews, threshold_percent);
    let next_agent = if let Some(ref failing_review_agent) = highest_priority_failed_review {
        resolve_next_agent_for_failed_review(ordered_agents, team, failing_review_agent)?
    } else if approved {
        if team.uses_numeric_routing() {
            let review_route = review_agents
                .first()
                .map(|agent| agent.weight)
                .unwrap_or(developer_agent.weight);
            let Some(next_route) = team
                .agents
                .iter()
                .filter(|agent| agent.weight > review_route)
                .map(|agent| agent.weight)
                .min()
            else {
                return Err(anyhow::anyhow!(
                    "developer stage completed, but no downstream routed stage is configured after `{}`",
                    developer_agent.name
                ));
            };
            team.agents
                .iter()
                .find(|agent| agent.weight == next_route)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "developer stage completed, but the configured next routed stage `{next_route}` could not be found"
                    )
                })?
        } else {
            let Some(next_agent_name) = next_agent_name_after_pass(ordered_agents, developer_agent)
            else {
                return Err(anyhow::anyhow!(
                    "developer stage completed, but no downstream agent is configured after `{}`",
                    developer_agent.name
                ));
            };
            team.agent_for_role(&next_agent_name)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "developer stage completed, but the configured next agent `{next_agent_name}` could not be found"
                    )
                })?
        }
    } else {
        team.developer_agent()
            .cloned()
            .unwrap_or_else(|| developer_agent.clone())
    };

    let next_handoff = if highest_priority_failed_review.is_some() {
        let failing_review_agent = highest_priority_failed_review
            .as_ref()
            .expect("review failure present");
        merge_handoff_context(
            Some(&format!(
                "Parallel review set recorded a failing reviewer result for `{}` [{}] and is routing according to that agent's `next_on_fail` path.\n\n{}",
                failing_review_agent.name,
                failing_review_agent.role,
                review_summary
            )),
            open_findings.render_for_agent(&next_agent),
        )
    } else if approved {
        merge_handoff_context(
            Some(&format!(
                "Parallel review set passed at {approval_percent}% against a required threshold of {threshold_percent}% after developer `{}` [{}].\n\n{}",
                developer_spawned
                    .metadata
                    .agent_nickname
                    .clone()
                    .unwrap_or_else(|| developer_agent.name.clone()),
                developer_agent.role,
                review_summary
            )),
            open_findings.render_for_agent(&next_agent),
        )
    } else {
        merge_handoff_context(
            Some(&format!(
                "Parallel review set reached {approval_percent}% against a required threshold of {threshold_percent}% after developer `{}` [{}]. Return to developer for fixes.\n\n{}",
                developer_spawned
                    .metadata
                    .agent_nickname
                    .clone()
                    .unwrap_or_else(|| developer_agent.name.clone()),
                developer_agent.role,
                review_summary
            )),
            open_findings.render_for_agent(developer_agent),
        )
    }
    .map(|text| compact_handoff_context(&text))
    .unwrap_or_default();

    Ok(ParallelReviewStageResult {
        next_agent,
        next_handoff,
    })
}

fn resolve_next_agent_for_failed_review(
    ordered_agents: &[MultiAgentAgentConfig],
    team: &MultiAgentTeamConfig,
    failing_review_agent: &MultiAgentAgentConfig,
) -> anyhow::Result<MultiAgentAgentConfig> {
    let Some(next_agent_name) = next_agent_name_for_status(
        ordered_agents,
        failing_review_agent,
        &AgentStatus::Errored("review failed".to_string()),
    ) else {
        return Err(anyhow::anyhow!(
            "review stage failed for `{}` [{}], but no downstream failure route is configured",
            failing_review_agent.name,
            failing_review_agent.role
        ));
    };

    team.agent_for_role(&next_agent_name)
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "review stage failed for `{}` [{}], but the configured failure route `{next_agent_name}` could not be found",
                failing_review_agent.name,
                failing_review_agent.role
            )
        })
}

async fn stage_completion_context(
    _session_handle: &Session,
    stage_agent: &MultiAgentAgentConfig,
    stage_spawned: &crate::agent::control::LiveAgent,
    stage_status: &AgentStatus,
) -> String {
    let stage_name = stage_spawned
        .metadata
        .agent_nickname
        .clone()
        .unwrap_or_else(|| stage_agent.name.clone());

    render_completed_stage_context(&stage_name, stage_agent.role.as_str(), stage_status)
}

fn render_completed_stage_context(
    stage_name: &str,
    stage_role: &str,
    stage_status: &AgentStatus,
) -> String {
    format!(
        "Completed stage output from {stage_name} [{stage_role}].\n\nReference evidence only: preserve concrete code, commands, tests, findings, and open issues across handoffs. Do not reinterpret this as a new plan, checklist, or architecture brief.\n\nStage status: {stage_status:?}\nsummary: {}\nnext: hand off the current code snapshot and open findings only.",
        status_detail_text(stage_status)
    )
}

fn stage_prompt_input(
    agent: &MultiAgentAgentConfig,
    mut input: Vec<UserInput>,
    extra_context: Option<String>,
    include_input: bool,
    locked_workspace_path: &str,
) -> Vec<UserInput> {
    let prefix = stage_prompt_prefix(agent);
    let mut stage_input = Vec::with_capacity(input.len() + usize::from(prefix.is_some()) + 4);
    if let Some(prefix) = prefix {
        stage_input.push(UserInput::Text {
            text: prefix,
            text_elements: Vec::new(),
        });
    }
    if !agent.instructions.trim().is_empty() {
        let agent_label = if agent.name.trim().is_empty() {
            agent.role.trim()
        } else {
            agent.name.trim()
        };
        let role_label = if agent.role.trim().is_empty() {
            "agent"
        } else {
            agent.role.trim()
        };
        stage_input.push(UserInput::Text {
            text: format!(
                "Stage-specific instructions for {agent_label} [{role_label}] (reference only; follow the role contract above first, and ignore any conflicts with it):\n\n{}",
                agent.instructions.trim()
            ),
            text_elements: Vec::new(),
        });
    }
    stage_input.push(UserInput::Text {
        text: locked_workspace_context(locked_workspace_path),
        text_elements: Vec::new(),
    });
    stage_input.push(UserInput::Text {
        text: request_contract_context(),
        text_elements: Vec::new(),
    });
    if let Some(extra_context) = extra_context {
        stage_input.push(UserInput::Text {
            text: format!(
                "Prior stage handoff context (reference only; treat this as the current state before acting):\n\n{extra_context}"
            ),
            text_elements: Vec::new(),
        });
    }
    if include_input {
        stage_input.push(UserInput::Text {
            text:
                "Current user task (reference only; keep this as the anchor for the current stage):"
                    .to_string(),
            text_elements: Vec::new(),
        });
    } else {
        stage_input.push(UserInput::Text {
            text: "Original user task (reference only; keep this as the anchor for the current stage):"
                .to_string(),
            text_elements: Vec::new(),
        });
    }
    stage_input.append(&mut input);
    stage_input
}

fn locked_workspace_context(locked_workspace_path: &str) -> String {
    format!(
        "Locked workspace path for this agent chain: {locked_workspace_path}\nThis is the active scope for all downstream stages in this chain.\nUse this exact path for file reads, file writes, validation, and reviews unless a new user request explicitly changes the folder.\nTreat any prior agent path mentions as historical context only."
    )
}

fn request_contract_context() -> String {
    "Original user request contract:\n- Treat the first user prompt as the immutable scope lock for the whole chain.\n- Preserve explicit exclusions and constraints from that prompt.\n- If the user said not to create a file type, dependency, or workflow, do not add it later unless direct failure evidence proves it is required.\n- Do not invent new features, files, or agent responsibilities from reviewer suggestions.\n- Only carry forward findings that are directly evidenced and still unresolved."
        .to_string()
}

fn compact_handoff_context(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(120)
        .map(|line| truncate_line(line, 360))
        .collect::<Vec<_>>()
        .join("\n")
}

fn enrich_handoff_context_for_agent(
    agent: &MultiAgentAgentConfig,
    stage_history: &[StageHistoryRecord],
    open_findings: &OpenFindingsLedger,
    handoff_context: Option<String>,
) -> Option<String> {
    let handoff_context = if agent.role.trim().eq_ignore_ascii_case("project manager") {
        let production_summary = render_production_summary(
            stage_history,
            open_findings,
            "pre-production handoff to project manager",
            false,
        );
        merge_handoff_context(handoff_context.as_deref(), Some(production_summary))
    } else {
        handoff_context
    };

    handoff_context.map(|text| compact_handoff_context(&text))
}

fn stage_spawn_preview(input: &[UserInput]) -> String {
    compact_handoff_context(&input_preview(input))
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    let mut truncated = line.chars().take(max_chars).collect::<String>();
    if line.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

fn stage_handoff_summary(
    current_agent: &MultiAgentAgentConfig,
    last_spawned_agent_name: &str,
    status: &AgentStatus,
    forwarded_to: Option<&str>,
) -> String {
    let status_label = match status_for_routing(status) {
        AgentStatus::Completed(_) => "pass",
        AgentStatus::Errored(_)
        | AgentStatus::Interrupted
        | AgentStatus::NotFound
        | AgentStatus::Shutdown
        | AgentStatus::PendingInit
        | AgentStatus::Running => "fail",
    };
    let summary = status_detail_text(status);
    let progress = match status {
        AgentStatus::Completed(_) => "stage complete",
        AgentStatus::Errored(_) => "stage failed",
        AgentStatus::Interrupted => "stage interrupted",
        AgentStatus::NotFound => "agent not found",
        AgentStatus::Shutdown => "agent shut down",
        AgentStatus::PendingInit => "waiting for init",
        AgentStatus::Running => "agent still running",
    };

    format!(
        "agent {} [{}]\nsummary: {summary}\nstatus: {status_label}\nrole: {}\nprogress: {progress}\nnext: {}\nforwarded_to: {}\n",
        last_spawned_agent_name,
        current_agent.role,
        current_agent.role,
        forwarded_to.unwrap_or("none"),
        forwarded_to.unwrap_or("none"),
    )
}

fn render_production_summary(
    stage_history: &[StageHistoryRecord],
    open_findings: &OpenFindingsLedger,
    terminal_note: &str,
    workflow_passed: bool,
) -> String {
    let mut lines = Vec::new();
    lines.push("Production agent summary".to_string());
    lines.push(format!(
        "overall_status: {}",
        if workflow_passed { "pass" } else { "fail" }
    ));
    lines.push(format!("terminal_note: {terminal_note}"));
    lines.push(String::new());
    lines.push("| Agent | Role | Status | Completed | Next | Forwarded to |".to_string());
    lines.push("| --- | --- | --- | --- | --- | --- |".to_string());

    for record in stage_history {
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            record.agent_name,
            record.role,
            record.status_label(),
            truncate_line(&record.completed_text(), 80),
            match &record.status {
                AgentStatus::Completed(_) => "complete",
                _ => "pending",
            },
            record.forwarded_to.as_deref().unwrap_or("none"),
        ));
    }

    if let Some(findings) = open_findings.render() {
        lines.push(String::new());
        lines.push(findings);
    }

    lines.join("\n")
}

fn status_detail_text(status: &AgentStatus) -> String {
    match status {
        AgentStatus::Completed(Some(summary)) => summary.clone(),
        AgentStatus::Completed(None) => "<no summary provided>".to_string(),
        AgentStatus::Errored(message) => message.clone(),
        AgentStatus::Interrupted => "interrupted before completion".to_string(),
        AgentStatus::NotFound => "agent not found".to_string(),
        AgentStatus::Shutdown => "agent shut down".to_string(),
        AgentStatus::PendingInit => "waiting for init".to_string(),
        AgentStatus::Running => "agent still running".to_string(),
    }
}

fn approval_threshold_met(
    passed_reviews: usize,
    total_reviews: usize,
    threshold_percent: u8,
) -> bool {
    if total_reviews == 0 {
        return false;
    }

    passed_reviews.saturating_mul(100) >= total_reviews.saturating_mul(threshold_percent as usize)
}

fn review_grade_passed(reported_grade: Option<u8>, required_grade: u8) -> bool {
    reported_grade.is_some_and(|grade| grade >= required_grade)
}

fn normalize_review_summary(summary: &str, passed: bool) -> String {
    let desired_status = if passed { "pass" } else { "fail" };
    let mut lines = Vec::new();
    let mut replaced_status = false;

    for line in summary.lines() {
        let trimmed = line.trim_start();
        if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("status:") {
            lines.push(format!("status: {desired_status}"));
            replaced_status = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced_status {
        lines.push(format!("status: {desired_status}"));
    }

    lines.join("\n")
}

fn extract_review_grade(text: &str) -> Option<u8> {
    for line in text.lines() {
        let normalized = line.trim().to_ascii_lowercase();
        if normalized.contains("grade") || normalized.contains("score") {
            if let Some(grade) = extract_first_grade_value(line) {
                return Some(grade);
            }
        }
    }
    None
}

fn extract_first_grade_value(text: &str) -> Option<u8> {
    let mut digits = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }

        if !digits.is_empty() {
            if let Ok(value) = digits.parse::<u16>()
                && value <= 100
            {
                return Some(value as u8);
            }
            digits.clear();
        }
    }

    if !digits.is_empty()
        && let Ok(value) = digits.parse::<u16>()
        && value <= 100
    {
        return Some(value as u8);
    }

    None
}

fn infer_locked_workspace_path(root_input: &[UserInput], fallback: &str) -> String {
    let mut fallback_candidate = fallback.to_string();
    let mut preferred_candidate: Option<String> = None;

    for candidate in root_input.iter().flat_map(user_input_paths) {
        if preferred_candidate.is_none() && looks_like_workspace_target(&candidate) {
            preferred_candidate = Some(candidate.clone());
        }
        if fallback_candidate.is_empty() {
            fallback_candidate = candidate.clone();
        }
    }

    preferred_candidate
        .or_else(|| root_input.iter().flat_map(user_input_paths).next())
        .unwrap_or(fallback_candidate)
}

fn user_input_paths(input: &UserInput) -> Vec<String> {
    match input {
        UserInput::Text { text, .. } => extract_absolute_paths(text),
        UserInput::LocalImage { path } => vec![path.display().to_string()],
        UserInput::Skill { path, .. } => vec![path.display().to_string()],
        UserInput::Mention { path, .. } => vec![path.clone()],
        UserInput::Image { .. } => Vec::new(),
        _ => Vec::new(),
    }
}

fn extract_absolute_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in text.split_whitespace() {
        if let Some(candidate) = normalize_path_candidate(token) {
            paths.push(candidate);
        }
    }
    paths
}

fn normalize_path_candidate(token: &str) -> Option<String> {
    let candidate = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"'
                | '\''
                | ','
                | ';'
                | ':'
                | '.'
                | ')'
                | '('
                | ']'
                | '['
                | '{'
                | '}'
                | '<'
                | '>'
        )
    });
    if candidate.is_empty() {
        return None;
    }

    let is_unix_absolute = candidate.starts_with('/');
    let is_windows_absolute = candidate.len() >= 3
        && candidate.as_bytes()[1] == b':'
        && matches!(candidate.as_bytes()[2], b'\\' | b'/')
        && candidate.as_bytes()[0].is_ascii_alphabetic();

    if is_unix_absolute || is_windows_absolute {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn looks_like_workspace_target(candidate: &str) -> bool {
    let normalized = candidate.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/poc/") || normalized.contains("c:/poc/")
}

fn stage_prompt_prefix(agent: &MultiAgentAgentConfig) -> Option<String> {
    let role = agent.role.trim();
    let name = agent.name.trim();
    let agent_label = if name.is_empty() { role } else { name };
    let role_label = if role.is_empty() { "agent" } else { role };
    let passing_grade = agent.passing_grade.unwrap_or(100).min(100);

    let prefix = match role.to_ascii_lowercase().as_str() {
        "architect" => format!(
            "You are {agent_label} [{role_label}]. Start with a brief `Working:` line, then produce a design only. Do not implement code, scaffold files, run tests, install dependencies, or expand into a task breakdown. Return a concrete architecture summary, risks, boundaries, and the next handoff target."
        ),
        "planner" => format!(
            "You are {agent_label} [{role_label}]. Start with a brief `Working:` line, then produce a plan only. Do not write implementation code, scaffold files, run tests, inspect runtime behavior, or install dependencies. Return the concrete task breakdown, target files, preparation steps, and the next handoff. The developer owns implementation. If a needed dependency is missing, record it as a blocker and hand off rather than trying to work around it. If a planned step would create, copy, move, or write files outside the current writable workspace, mark it as requiring approval instead of executing it yourself."
        ),
        "developer" | "coder" | "worker" => format!(
            "You are {agent_label} [{role_label}]. IMPLEMENTATION ONLY.\n\nStart with a brief `Working:` line.\n\nYour job is to make concrete code changes in the target workspace.\nTreat all previous architect, planner, review, and handoff output as reference evidence only.\nDo not write a plan.\nDo not write phases.\nDo not redesign.\nDo not continue planning from prior stage output.\nDo not narrate a developer task list.\nDo not turn this stage into a checklist-writing exercise.\nYour first substantive action must be a concrete file edit or a concrete command that advances implementation.\nDo not stop after preflight.\nWhen a filesystem step is needed, use the exact command or path instead of describing it abstractly. Prefer concrete actions like `mkdir -p <path>`, `ls <path>`, `find <path>`, `rg <pattern> <path>`, `git diff`, or a direct file edit.\nIf the target folder does not exist, create it and continue.\n\nIf the handoff includes open findings from quality, security, tester, or judge, treat the whole unresolved backlog as the work item and address it together so you do not bounce the same defect between stages.\nCarry forward previously unresolved findings until they are fixed or explicitly superseded by direct evidence.\nDo not introduce new requirements, files, or behaviors unless they are required by the original user request or by direct evidence of a failure.\nIf a reviewer suggests an out-of-scope enhancement, record it as out of scope rather than turning it into a new loop.\n\nYou must modify files, write code, run validation, fix failures, and report files changed and commands run.\nIf no files were created or modified, your task is incomplete.\nIf a filesystem-changing command would create, copy, move, or write outside the current writable workspace, stop and request approval first.\nIf approval is granted, continue with the concrete implementation."
        ),
        "critic" | "security" | "quality" | "tester" | "execution tester" | "judge" => format!(
            "You are {agent_label} [{role_label}]. Start with a brief `Working:` line, then validate the current implementation snapshot and any prior stage handoff context. Do not treat the raw user request as the only input, but do treat the original user request as the immutable scope lock. Do not rewrite the solution or continue planning. Keep feedback concise and non-repetitive. Every finding must cite concrete evidence (file/line, command output, or observed behavior). If you infer beyond direct evidence, label it explicitly as an inference. Keep `status` internally consistent with findings and with the configured grade threshold; if the grade meets or exceeds {passing_grade}%, the final result should be `pass`. Do not introduce new scope, wishlist items, or agent responsibilities that were not part of the original request. If a response proposes something out of scope, mark it as out of scope instead of turning it into a new failure loop. Report concrete findings, status, progress, next, and forwarded_to. End with exactly one final line in the output in the form `grade: <0-100>` and nothing else after it. Do not output `grade: None`, `grade: Some(...)`, or multiple grade lines. This role passes at {passing_grade}% or higher. Score only against the current implementation and evidence, and do not exceed 100. If the response is mostly planning language, missing code evidence, or missing validation evidence, mark it `fail`, set a low grade, and send it back to the developer."
        ),
        "project manager" => format!(
            "You are {agent_label} [{role_label}]. Start with a brief `Working:` line, then produce the final production summary only. Do not implement code, do not continue review, and do not restart the workflow. Summarize the complete agent history, unresolved findings, pass/fail status, and the final close-out table. Treat the original user request plus the latest feedback as the anchor, and close the loop only when the evidence supports it. If you include a grade, it must be a single final line in the form `grade: <0-100>`."
        ),
        _ => format!(
            "You are {agent_label} [{role_label}]. Start with a brief `Working:` line and stay strictly within your assigned role. Do not take over work owned by another agent. Before any filesystem-changing command that creates, copies, moves, or writes outside the current writable workspace, stop and request approval instead of running it directly."
        ),
    };

    Some(prefix)
}

async fn spawn_multi_agent_stage(
    session_handle: &Session,
    ctx: &TurnContext,
    base_spawn_config: &crate::config::Config,
    base_instructions: &str,
    agent: &MultiAgentAgentConfig,
    input: Vec<UserInput>,
    child_depth: i32,
    stage_spawn_preview: String,
    reasoning_effort: codex_protocol::openai_models::ReasoningEffort,
) -> anyhow::Result<crate::agent::control::LiveAgent> {
    let agent_display_name = agent.name.clone();
    let agent_role = agent.role.clone();
    let preferred_agent_nickname = Some(agent_display_name.clone());
    let agent_path_task_name = format!(
        "{}_{}",
        sanitize_name(&agent_display_name),
        sanitize_name(&Uuid::new_v4().to_string())
    );
    let call_id = format!(
        "multi_agent_bootstrap_{}_{}",
        ctx.sub_id,
        agent_display_name.to_lowercase()
    );
    session_handle
        .send_event(
            ctx,
            EventMsg::CollabAgentSpawnBegin(CollabAgentSpawnBeginEvent {
                call_id: call_id.clone(),
                sender_thread_id: session_handle.conversation_id,
                prompt: stage_spawn_preview.clone(),
                model: agent.model.clone(),
                reasoning_effort,
            }),
        )
        .await;

    let mut config = base_spawn_config.clone();
    config.base_instructions = Some(base_instructions.to_string());
    if let Err(err) = apply_multi_agent_team_overrides(
        session_handle,
        &mut config,
        Some(agent_role.as_str()),
        None,
    ) {
        session_handle
            .send_event(
                ctx,
                EventMsg::CollabAgentSpawnEnd(CollabAgentSpawnEndEvent {
                    call_id,
                    sender_thread_id: session_handle.conversation_id,
                    new_thread_id: None,
                    new_agent_nickname: Some(agent_display_name),
                    new_agent_role: Some(agent_role),
                    prompt: stage_spawn_preview,
                    model: agent.model.clone(),
                    reasoning_effort,
                    status: AgentStatus::NotFound,
                    failure_reason: Some(err.to_string()),
                }),
            )
            .await;
        return Err(err.into());
    }

    let session_source = match thread_spawn_source(
        session_handle.conversation_id,
        &ctx.session_source,
        child_depth,
        Some(agent.role.as_str()),
        Some(agent_path_task_name),
        preferred_agent_nickname,
    ) {
        Ok(session_source) => session_source,
        Err(err) => {
            session_handle
                .send_event(
                    ctx,
                    EventMsg::CollabAgentSpawnEnd(CollabAgentSpawnEndEvent {
                        call_id,
                        sender_thread_id: session_handle.conversation_id,
                        new_thread_id: None,
                        new_agent_nickname: Some(agent.name.clone()),
                        new_agent_role: Some(agent.role.clone()),
                        prompt: stage_spawn_preview,
                        model: agent.model.clone(),
                        reasoning_effort,
                        status: AgentStatus::NotFound,
                        failure_reason: Some(err.to_string()),
                    }),
                )
                .await;
            return Err(err.into());
        }
    };

    let result = session_handle
        .services
        .agent_control
        .spawn_agent_with_metadata(
            config,
            input,
            Some(session_source),
            SpawnAgentOptions::default(),
        )
        .await;

    let (new_thread_id, new_agent_nickname, new_agent_role, status) = match &result {
        Ok(spawned_agent) => (
            Some(spawned_agent.thread_id),
            spawned_agent.metadata.agent_nickname.clone(),
            spawned_agent.metadata.agent_role.clone(),
            spawned_agent.status.clone(),
        ),
        Err(_) => (None, None, Some(agent.role.clone()), AgentStatus::NotFound),
    };

    session_handle
        .send_event(
            ctx,
            EventMsg::CollabAgentSpawnEnd(CollabAgentSpawnEndEvent {
                call_id,
                sender_thread_id: session_handle.conversation_id,
                new_thread_id,
                new_agent_nickname,
                new_agent_role,
                prompt: stage_spawn_preview,
                model: agent.model.clone(),
                reasoning_effort,
                status,
                failure_reason: result.as_ref().err().map(|err| err.to_string()),
            }),
        )
        .await;

    result.map_err(Into::into)
}

async fn wait_for_agent_completion(
    session_handle: &Session,
    thread_id: codex_protocol::ThreadId,
    cancellation_token: CancellationToken,
) -> Option<AgentStatus> {
    let Ok(mut status_rx) = session_handle
        .services
        .agent_control
        .subscribe_status(thread_id)
        .await
    else {
        return Some(
            session_handle
                .services
                .agent_control
                .get_status(thread_id)
                .await,
        );
    };
    let mut status = status_rx.borrow().clone();
    while !is_final(&status) {
        tokio::select! {
            _ = cancellation_token.cancelled() => return None,
            changed = status_rx.changed() => {
                if changed.is_err() {
                    return Some(session_handle.services.agent_control.get_status(thread_id).await);
                }
                status = status_rx.borrow().clone();
            }
        }
    }
    Some(status)
}

async fn request_architect_approval(
    session_handle: &Session,
    ctx: &TurnContext,
    agent: &MultiAgentAgentConfig,
    architect_summary: &str,
) -> Option<bool> {
    let response = session_handle
        .request_user_input(
            ctx,
            format!("architect_approval_{}", ctx.sub_id),
            RequestUserInputArgs {
                questions: vec![RequestUserInputQuestion {
                    id: "architect_design_approval".to_string(),
                    header: "Design".to_string(),
                    question: format!(
                        "Approve the {} design before implementation starts?",
                        agent.name
                    ),
                    is_other: false,
                    is_secret: false,
                    options: Some(vec![
                        RequestUserInputQuestionOption {
                            label: "Approve design".to_string(),
                            description:
                                "Continue to the next configured agent with this architecture."
                                    .to_string(),
                        },
                        RequestUserInputQuestionOption {
                            label: "Revise design".to_string(),
                            description:
                                "Stop implementation so the architecture can be revised first."
                                    .to_string(),
                        },
                    ]),
                }],
            },
        )
        .await?;

    architect_design_approved(&response, architect_summary)
}

fn architect_design_approved(
    response: &RequestUserInputResponse,
    architect_summary: &str,
) -> Option<bool> {
    let answers = response
        .answers
        .get("architect_design_approval")?
        .answers
        .iter()
        .map(|answer| answer.trim().to_lowercase())
        .collect::<Vec<_>>();
    if answers.iter().any(|answer| answer == "approve design") {
        return Some(true);
    }
    if answers.iter().any(|answer| answer == "revise design") {
        return Some(false);
    }
    if answers
        .iter()
        .any(|answer| answer == "yes" || answer == "yes approve" || answer == "approve")
    {
        return Some(true);
    }
    if answers
        .iter()
        .any(|answer| answer == "no" || answer == "revise" || answer == "reject")
    {
        return Some(false);
    }
    warn!(
        "architect approval response could not be parsed; answers={answers:?}, summary_preview={}",
        architect_summary.chars().take(120).collect::<String>()
    );
    None
}

fn ordered_multi_agent_roster(team: &MultiAgentTeamConfig) -> Vec<MultiAgentAgentConfig> {
    if team.uses_numeric_routing() {
        let mut ordered = team
            .agents
            .iter()
            .filter(|agent| agent.weight > 0)
            .cloned()
            .enumerate()
            .collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            left.1
                .weight
                .cmp(&right.1.weight)
                .then_with(|| left.0.cmp(&right.0))
        });
        return ordered.into_iter().map(|(_, agent)| agent).collect();
    }

    let mut ordered = Vec::with_capacity(team.agents.len());
    let mut seen_names = BTreeSet::<String>::new();

    if let Some(architect_agent) = team.agent_for_role("architect")
        && seen_names.insert(architect_agent.name.to_lowercase())
    {
        ordered.push(architect_agent.clone());
    }

    if let Some(default_agent) = team.default_agent.as_deref()
        && let Some(agent) = team.agent_for_role(default_agent)
        && seen_names.insert(agent.name.to_lowercase())
    {
        ordered.push(agent.clone());
    }

    for agent in &team.agents {
        if seen_names.insert(agent.name.to_lowercase()) {
            ordered.push(agent.clone());
        }
    }

    ordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use codex_protocol::request_user_input::RequestUserInputAnswer;
    use pretty_assertions::assert_eq;

    #[test]
    fn ordered_multi_agent_roster_places_default_agent_first() {
        let team = MultiAgentTeamConfig {
            team_name: Some("Team".to_string()),
            default_agent: Some("planner".to_string()),
            routing: Default::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "coder".to_string(),
                    role: "coder".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 40,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "coder".to_string(),
                },
                MultiAgentAgentConfig {
                    name: "planner".to_string(),
                    role: "planner".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 60,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "planner".to_string(),
                },
                MultiAgentAgentConfig {
                    name: "critic".to_string(),
                    role: "critic".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "critic".to_string(),
                },
            ],
        };

        let ordered = ordered_multi_agent_roster(&team);

        assert_eq!(
            ordered
                .into_iter()
                .map(|agent| agent.name)
                .collect::<Vec<_>>(),
            vec![
                "planner".to_string(),
                "coder".to_string(),
                "critic".to_string()
            ]
        );
    }

    #[test]
    fn ordered_multi_agent_roster_skips_unrouted_agents_when_numeric_routing_is_active() {
        let team = MultiAgentTeamConfig {
            team_name: Some("Team".to_string()),
            default_agent: Some("developer".to_string()),
            routing: Default::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "architect".to_string(),
                    role: "architect".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 1,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "architect".to_string(),
                },
                MultiAgentAgentConfig {
                    name: "planner".to_string(),
                    role: "planner".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 0,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "planner".to_string(),
                },
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 2,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "developer".to_string(),
                },
                MultiAgentAgentConfig {
                    name: "critic".to_string(),
                    role: "critic".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 3,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "critic".to_string(),
                },
            ],
        };

        let ordered = ordered_multi_agent_roster(&team);

        assert_eq!(
            ordered
                .into_iter()
                .map(|agent| agent.name)
                .collect::<Vec<_>>(),
            vec![
                "architect".to_string(),
                "developer".to_string(),
                "critic".to_string()
            ]
        );
    }

    #[test]
    fn architect_design_approved_accepts_expected_yes_labels() {
        let response = RequestUserInputResponse {
            answers: HashMap::from([(
                "architect_design_approval".to_string(),
                RequestUserInputAnswer {
                    answers: vec!["Approve design".to_string()],
                },
            )]),
        };

        assert_eq!(architect_design_approved(&response, "summary"), Some(true));
    }

    #[test]
    fn architect_design_approved_accepts_expected_no_labels() {
        let response = RequestUserInputResponse {
            answers: HashMap::from([(
                "architect_design_approval".to_string(),
                RequestUserInputAnswer {
                    answers: vec!["Revise design".to_string()],
                },
            )]),
        };

        assert_eq!(architect_design_approved(&response, "summary"), Some(false));
    }

    #[test]
    fn stage_prompt_prefix_keeps_planning_role_out_of_implementation() {
        let agent = MultiAgentAgentConfig {
            name: "planner".to_string(),
            role: "planner".to_string(),
            model: "gpt-5.4".to_string(),
            model_provider: None,
            weight: 50,
            passing_grade: None,
            next_on_pass: None,
            next_on_fail: None,
            instructions: String::new(),
        };

        let prefix = stage_prompt_prefix(&agent).expect("expected a planner prefix");

        assert!(prefix.contains("Working:"));
        assert!(prefix.contains("planning only"));
        assert!(prefix.contains("Do not write implementation code"));
        assert!(prefix.contains("developer owns implementation"));
    }

    #[test]
    fn stage_prompt_prefix_requires_developer_implementation() {
        let agent = MultiAgentAgentConfig {
            name: "developer".to_string(),
            role: "developer".to_string(),
            model: "gpt-5.4".to_string(),
            model_provider: None,
            weight: 50,
            passing_grade: None,
            next_on_pass: None,
            next_on_fail: None,
            instructions: String::new(),
        };

        let prefix = stage_prompt_prefix(&agent).expect("expected a developer prefix");

        assert!(prefix.contains("Working:"));
        assert!(prefix.contains("IMPLEMENTATION ONLY"));
        assert!(prefix.contains(
            "Treat all previous architect, planner, review, and handoff output as reference evidence only"
        ));
        assert!(prefix.contains("Do not write a plan"));
        assert!(prefix.contains("Do not narrate a developer task list"));
        assert!(prefix.contains("Do not continue planning from prior stage output"));
        assert!(prefix.contains("whole unresolved backlog"));
        assert!(prefix.contains("mkdir -p <path>"));
        assert!(prefix.contains(
            "Your first substantive action must be a concrete file edit or a concrete command that advances implementation."
        ));
        assert!(
            prefix.contains("If approval is granted, continue with the concrete implementation")
        );
    }

    #[test]
    fn default_multi_agent_team_instructions_prefer_concrete_commands() {
        let instructions = default_multi_agent_team_instructions();

        assert!(instructions.contains("mkdir -p"));
        assert!(instructions.contains("ls"));
        assert!(instructions.contains("find"));
        assert!(instructions.contains("rg"));
        assert!(instructions.contains("Every agent should prefer concrete filesystem actions"));
    }

    #[test]
    fn stage_prompt_prefix_requires_reviewing_current_snapshot() {
        let agent = MultiAgentAgentConfig {
            name: "critic".to_string(),
            role: "critic".to_string(),
            model: "gpt-5.4".to_string(),
            model_provider: None,
            weight: 50,
            passing_grade: None,
            next_on_pass: None,
            next_on_fail: None,
            instructions: String::new(),
        };

        let prefix = stage_prompt_prefix(&agent).expect("expected a critic prefix");

        assert!(prefix.contains("current implementation snapshot"));
        assert!(prefix.contains("prior stage handoff context"));
        assert!(prefix.contains("Do not treat the raw user request as the only input"));
        assert!(prefix.contains("immutable scope lock"));
        assert!(prefix.contains("out of scope"));
        assert!(prefix.contains("grade: <0-100>"));
    }

    #[test]
    fn render_completed_stage_context_stays_concise() {
        let rendered = render_completed_stage_context(
            "Ampere",
            "developer",
            &AgentStatus::Completed(Some("developer finished".to_string())),
        );

        assert!(rendered.contains("Completed stage output from Ampere [developer]"));
        assert!(rendered.contains("Stage status: Completed(Some(\"developer finished\"))"));
        assert!(rendered.contains("reference evidence only"));
        assert!(rendered.contains("hand off the current code snapshot and open findings only"));
        assert!(!rendered.contains("Stage transcript evidence"));
        assert!(!rendered.contains("user: create a python game"));
    }

    #[test]
    fn stage_prompt_input_keeps_root_input_for_downstream_handoffs() {
        let agent = MultiAgentAgentConfig {
            name: "developer".to_string(),
            role: "developer".to_string(),
            model: "gpt-5.4".to_string(),
            model_provider: None,
            weight: 50,
            passing_grade: None,
            next_on_pass: None,
            next_on_fail: None,
            instructions: String::new(),
        };

        let input = vec![UserInput::Text {
            text: "original user task".to_string(),
            text_elements: Vec::new(),
        }];
        let output = stage_prompt_input(
            &agent,
            input,
            Some("handoff".to_string()),
            false,
            "/mnt/c/POC/test-game-1",
        );

        assert_eq!(output.len(), 6);
        assert!(
            matches!(output[0], UserInput::Text { ref text, .. } if text.contains("IMPLEMENTATION ONLY"))
        );
        assert!(
            matches!(output[1], UserInput::Text { ref text, .. } if text.contains("Locked workspace path for this agent chain: /mnt/c/POC/test-game-1"))
        );
        assert!(
            matches!(output[2], UserInput::Text { ref text, .. } if text.contains("Original user request contract"))
        );
        assert!(
            matches!(output[3], UserInput::Text { ref text, .. } if text.contains("Prior stage handoff context (reference only; treat this as the current state before acting)"))
        );
        assert!(
            matches!(output[4], UserInput::Text { ref text, .. } if text.contains("Original user task"))
        );
        assert!(
            matches!(output[5], UserInput::Text { ref text, .. } if text.contains("original user task"))
        );
    }

    #[test]
    fn stage_prompt_input_keeps_stage_instructions_local_to_the_agent() {
        let agent = MultiAgentAgentConfig {
            name: "developer".to_string(),
            role: "developer".to_string(),
            model: "gpt-5.4".to_string(),
            model_provider: None,
            weight: 50,
            next_on_pass: None,
            next_on_fail: None,
            instructions: "Use Python for implementation.".to_string(),
        };

        let output = stage_prompt_input(&agent, Vec::new(), None, false, "/mnt/c/POC/test-game-1");

        assert_eq!(output.len(), 5);
        assert!(
            matches!(output[0], UserInput::Text { ref text, .. } if text.contains("IMPLEMENTATION ONLY"))
        );
        assert!(
            matches!(output[1], UserInput::Text { ref text, .. } if text.contains("Stage-specific instructions for developer [developer]"))
        );
        assert!(
            matches!(output[2], UserInput::Text { ref text, .. } if text.contains("Locked workspace path for this agent chain: /mnt/c/POC/test-game-1"))
        );
        assert!(
            matches!(output[3], UserInput::Text { ref text, .. } if text.contains("Original user request contract"))
        );
        assert!(
            matches!(output[4], UserInput::Text { ref text, .. } if text.contains("Original user task"))
        );
    }

    #[test]
    fn stage_handoff_summary_uses_role_specific_compact_summary() {
        let agent = MultiAgentAgentConfig {
            name: "developer".to_string(),
            role: "developer".to_string(),
            model: "gpt-5.4".to_string(),
            model_provider: None,
            weight: 50,
            next_on_pass: Some("critic".to_string()),
            next_on_fail: Some("developer".to_string()),
            instructions: String::new(),
        };

        let summary = stage_handoff_summary(
            &agent,
            "developer-1",
            &AgentStatus::Completed(Some(
                "I planned the implementation in detail.\nsummary: verbose planning artifact"
                    .to_string(),
            )),
            Some("critic"),
        );

        assert!(summary.contains("summary: I planned the implementation in detail."));
        assert!(summary.contains("status: pass"));
        assert!(summary.contains("forwarded_to: critic"));
    }

    #[test]
    fn open_findings_ledger_deduplicates_shared_issue_text() {
        let mut ledger = OpenFindingsLedger::default();
        let shared = AgentStatus::Errored("Missing evidence for the implementation.".to_string());

        ledger.record_status(
            &MultiAgentAgentConfig {
                name: "critic".to_string(),
                role: "critic".to_string(),
                model: "gpt-5.4".to_string(),
                model_provider: None,
                weight: 50,
                passing_grade: None,
                next_on_pass: None,
                next_on_fail: None,
                instructions: String::new(),
            },
            &shared,
            None,
        );
        ledger.record_status(
            &MultiAgentAgentConfig {
                name: "security".to_string(),
                role: "security".to_string(),
                model: "gpt-5.4".to_string(),
                model_provider: None,
                weight: 50,
                passing_grade: None,
                next_on_pass: None,
                next_on_fail: None,
                instructions: String::new(),
            },
            &shared,
            None,
        );

        let rendered = ledger.render().expect("expected open findings");

        assert!(rendered.contains("roles: critic, security"));
        assert_eq!(
            rendered
                .matches("Missing evidence for the implementation.")
                .count(),
            1
        );
    }

    #[test]
    fn open_findings_ledger_prioritizes_security_before_quality() {
        let mut ledger = OpenFindingsLedger::default();

        ledger.record_status(
            &MultiAgentAgentConfig {
                name: "quality".to_string(),
                role: "quality".to_string(),
                model: "gpt-5.4".to_string(),
                model_provider: None,
                weight: 50,
                passing_grade: None,
                next_on_pass: None,
                next_on_fail: None,
                instructions: String::new(),
            },
            &AgentStatus::Errored("quality issue".to_string()),
            None,
        );
        ledger.record_status(
            &MultiAgentAgentConfig {
                name: "security".to_string(),
                role: "security".to_string(),
                model: "gpt-5.4".to_string(),
                model_provider: None,
                weight: 50,
                passing_grade: None,
                next_on_pass: None,
                next_on_fail: None,
                instructions: String::new(),
            },
            &AgentStatus::Errored("security issue".to_string()),
            None,
        );

        let rendered = ledger.render().expect("expected open findings");
        let security_index = rendered.find("security issue").expect("security finding");
        let quality_index = rendered.find("quality issue").expect("quality finding");

        assert!(security_index < quality_index);
    }

    #[test]
    fn production_summary_renders_table_and_overall_status() {
        let history = vec![StageHistoryRecord::new(
            "developer-1".to_string(),
            "developer".to_string(),
            AgentStatus::Completed(Some("implemented the fix".to_string())),
            Some("critic".to_string()),
        )];
        let ledger = OpenFindingsLedger::default();

        let rendered = render_production_summary(&history, &ledger, "completed", true);

        assert!(rendered.contains("Production agent summary"));
        assert!(rendered.contains("| Agent | Role | Status | Completed | Next | Forwarded to |"));
        assert!(rendered.contains("overall_status: pass"));
        assert!(rendered.contains("implemented the fix"));
    }

    #[test]
    fn project_manager_handoff_context_includes_full_production_summary() {
        let history = vec![
            StageHistoryRecord::new(
                "developer-1".to_string(),
                "developer".to_string(),
                AgentStatus::Completed(Some("implemented the fix".to_string())),
                Some("critic".to_string()),
            ),
            StageHistoryRecord::new(
                "critic-1".to_string(),
                "critic".to_string(),
                AgentStatus::Completed(Some("reviewed and approved".to_string())),
                Some("project manager".to_string()),
            ),
        ];
        let mut ledger = OpenFindingsLedger::default();
        ledger.record_status(
            &MultiAgentAgentConfig {
                name: "quality".to_string(),
                role: "quality".to_string(),
                model: "gpt-5.4".to_string(),
                model_provider: None,
                weight: 50,
                passing_grade: None,
                next_on_pass: None,
                next_on_fail: None,
                instructions: String::new(),
            },
            &AgentStatus::Errored("missing evidence".to_string()),
            None,
        );
        let project_manager = MultiAgentAgentConfig {
            name: "pm".to_string(),
            role: "project manager".to_string(),
            model: "gpt-5.4".to_string(),
            model_provider: None,
            weight: 90,
            passing_grade: None,
            next_on_pass: None,
            next_on_fail: None,
            instructions: String::new(),
        };

        let rendered = enrich_handoff_context_for_agent(
            &project_manager,
            &history,
            &ledger,
            Some("latest review handoff".to_string()),
        )
        .expect("expected pm handoff context");

        assert!(rendered.contains("Production agent summary"));
        assert!(rendered.contains("developer-1"));
        assert!(rendered.contains("critic-1"));
        assert!(rendered.contains("Open unresolved findings"));
        assert!(rendered.contains("latest review handoff"));
    }

    #[test]
    fn approval_threshold_met_accepts_meeting_or_exceeding_grade() {
        assert!(approval_threshold_met(4, 5, 80));
        assert!(approval_threshold_met(3, 4, 75));
        assert!(!approval_threshold_met(2, 4, 75));
        assert!(!approval_threshold_met(0, 0, 0));
    }

    #[test]
    fn extract_review_grade_prefers_explicit_grade_or_score_lines() {
        assert_eq!(
            extract_review_grade("summary: fine\ngrade: 90\nnext: developer"),
            Some(90)
        );
        assert_eq!(
            extract_review_grade("score: 72/100\nstatus: pass"),
            Some(72)
        );
        assert_eq!(extract_review_grade("no grade provided"), None);
    }

    #[test]
    fn review_grade_passed_uses_grade_over_status_text() {
        assert!(review_grade_passed(Some(90), 80));
        assert!(!review_grade_passed(Some(79), 80));
        assert!(!review_grade_passed(Some(90), 80));
    }

    #[test]
    fn normalize_review_summary_rewrites_status_from_grade_verdict() {
        let passed = normalize_review_summary("summary: good\nstatus: fail\nnext: developer", true);
        assert!(passed.contains("status: pass"));
        assert!(!passed.contains("status: fail"));

        let failed =
            normalize_review_summary("summary: good\nstatus: pass\nnext: developer", false);
        assert!(failed.contains("status: fail"));
        assert!(!failed.contains("status: pass"));
    }

    #[test]
    fn infer_locked_workspace_path_prefers_user_target_folder() {
        let input = vec![UserInput::Text {
            text: "Create the app in `/mnt/c/POC/test-game-1` and keep downstream agents on that path.".to_string(),
            text_elements: Vec::new(),
        }];

        assert_eq!(
            infer_locked_workspace_path(&input, "C:/fallback"),
            "/mnt/c/POC/test-game-1".to_string()
        );
    }

    #[test]
    fn infer_locked_workspace_path_falls_back_to_cwd_when_no_path_is_present() {
        let input = vec![UserInput::Text {
            text: "Hello world, please help".to_string(),
            text_elements: Vec::new(),
        }];

        assert_eq!(
            infer_locked_workspace_path(&input, "C:/fallback"),
            "C:/fallback".to_string()
        );
    }

    #[test]
    fn next_agent_name_for_status_prefers_configured_handoff() {
        let team = MultiAgentTeamConfig {
            team_name: Some("Team".to_string()),
            default_agent: Some("architect".to_string()),
            routing: Default::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "planner".to_string(),
                    role: "planner".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 50,
                    passing_grade: None,
                    next_on_pass: Some("developer".to_string()),
                    next_on_fail: Some("developer".to_string()),
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 50,
                    passing_grade: None,
                    next_on_pass: Some("critic".to_string()),
                    next_on_fail: Some("developer".to_string()),
                    instructions: String::new(),
                },
            ],
        };
        let ordered = ordered_multi_agent_roster(&team);
        let planner = team.agent_for_role("planner").expect("planner");

        assert_eq!(
            next_agent_name_for_status(&ordered, planner, &AgentStatus::Completed(None)),
            Some("developer".to_string())
        );
    }

    #[test]
    fn next_agent_name_for_status_routes_completed_fail_summary_to_failure_path() {
        let team = MultiAgentTeamConfig {
            team_name: Some("Team".to_string()),
            default_agent: Some("architect".to_string()),
            routing: Default::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "planner".to_string(),
                    role: "planner".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 50,
                    passing_grade: None,
                    next_on_pass: Some("developer".to_string()),
                    next_on_fail: Some("developer".to_string()),
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 50,
                    passing_grade: None,
                    next_on_pass: Some("critic".to_string()),
                    next_on_fail: Some("planner".to_string()),
                    instructions: String::new(),
                },
            ],
        };
        let ordered = ordered_multi_agent_roster(&team);
        let planner = team.agent_for_role("planner").expect("planner");

        let status = AgentStatus::Completed(Some(
            "summary: incomplete\nstatus: fail\nforwarded_to: planner".to_string(),
        ));
        assert_eq!(
            next_agent_name_for_status(&ordered, planner, &status),
            Some("developer".to_string())
        );
    }

    #[test]
    fn next_agent_name_for_status_returns_none_for_terminal_last_stage() {
        let team = MultiAgentTeamConfig {
            team_name: Some("Team".to_string()),
            default_agent: Some("developer".to_string()),
            routing: Default::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 70,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: Some("developer".to_string()),
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "judge".to_string(),
                    role: "judge".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 30,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: Some("developer".to_string()),
                    instructions: String::new(),
                },
            ],
        };
        let ordered = ordered_multi_agent_roster(&team);
        let judge = team.agent_for_role("judge").expect("judge");

        assert_eq!(
            next_agent_name_for_status(&ordered, judge, &AgentStatus::Completed(None)),
            None
        );
        assert_eq!(next_agent_name_after_pass(&ordered, judge), None);
    }

    #[test]
    fn failed_parallel_review_routes_using_the_failing_agents_next_on_fail() {
        let team = MultiAgentTeamConfig {
            team_name: Some("Team".to_string()),
            default_agent: Some("developer".to_string()),
            routing: Default::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 50,
                    passing_grade: None,
                    next_on_pass: Some("critic".to_string()),
                    next_on_fail: Some("developer".to_string()),
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "quality".to_string(),
                    role: "quality ai".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 60,
                    passing_grade: Some(40),
                    next_on_pass: Some("tester".to_string()),
                    next_on_fail: Some("developer".to_string()),
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "project manager".to_string(),
                    role: "project manager".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: None,
                    weight: 70,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
            ],
        };
        let ordered = ordered_multi_agent_roster(&team);
        let quality = team.agent_for_role("quality").expect("quality");
        let routed = resolve_next_agent_for_failed_review(&ordered, &team, quality)
            .expect("expected a next agent for failed review");

        assert_eq!(routed.name, "developer");
    }

    #[test]
    fn completed_summary_requests_fail_detects_status_fail_lines() {
        assert!(completed_summary_requests_fail(
            "summary: something went wrong\nstatus: fail\nforwarded_to: developer"
        ));
        assert!(!completed_summary_requests_fail(
            "summary: all good\nstatus: pass\nforwarded_to: critic"
        ));
    }
}
