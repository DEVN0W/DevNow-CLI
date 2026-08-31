use std::sync::Arc;

use async_trait::async_trait;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::CollabAgentSpawnBeginEvent;
use codex_protocol::protocol::CollabAgentSpawnEndEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::user_input::UserInput;
use futures::future::join_all;
use tokio_util::sync::CancellationToken;

use crate::agent::control::SpawnAgentOptions;
use crate::agent::exceeds_thread_spawn_depth_limit;
use crate::agent::next_thread_spawn_depth;
use crate::agent::status::is_final;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::models_manager::agentic_team::AgenticTeamConfig;
use crate::state::TaskKind;
use crate::tools::handlers::multi_agents_common::apply_mode_agent_role_config;
use crate::tools::handlers::multi_agents_common::apply_mode_roster_overrides;
use crate::tools::handlers::multi_agents_common::apply_spawn_agent_overrides;
use crate::tools::handlers::multi_agents_common::apply_spawn_agent_runtime_overrides;
use crate::tools::handlers::multi_agents_common::build_agent_spawn_config;
use crate::tools::handlers::multi_agents_common::input_preview;
use crate::tools::handlers::multi_agents_common::thread_spawn_source;

use super::SessionTask;
use super::SessionTaskContext;
use super::explicit_agent_history::continuation_input;
use super::explicit_agent_history::record_manual_turn;
use super::workspace_target::apply_requested_workspace;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExplicitAgentDelegation {
    pub(crate) agents: Vec<ExplicitAgentTarget>,
    pub(crate) input: Vec<UserInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExplicitAgentTarget {
    pub(crate) agent_name: String,
    pub(crate) agent_role: String,
}

pub(crate) fn parse_explicit_agent_delegation(
    input: &[UserInput],
    team: &AgenticTeamConfig,
) -> Option<ExplicitAgentDelegation> {
    let UserInput::Text { text, .. } = input.first()? else {
        return None;
    };
    let mut remaining = text.trim_start();
    let mut agents = Vec::new();
    while let Some(mention) = remaining.strip_prefix('@') {
        let agent_end = mention.find(char::is_whitespace).unwrap_or(mention.len());
        let requested_agent = mention[..agent_end].trim_end_matches([':', ',']);
        if requested_agent.is_empty() {
            return None;
        }
        let agent = team.agent_for_role(requested_agent)?;
        if agents
            .iter()
            .any(|target: &ExplicitAgentTarget| target.agent_name == agent.name)
        {
            return None;
        }
        agents.push(ExplicitAgentTarget {
            agent_name: agent.name.clone(),
            agent_role: agent.role.clone(),
        });
        remaining = mention[agent_end..].trim_start();
    }
    let task = remaining.trim_start_matches(['-', ':']).trim_start();
    if agents.is_empty() || task.is_empty() {
        return None;
    }
    let mut delegated_input = input.to_vec();
    delegated_input[0] = UserInput::Text {
        text: task.to_string(),
        text_elements: Vec::new(),
    };
    Some(ExplicitAgentDelegation {
        agents,
        input: delegated_input,
    })
}

struct ExplicitAgentResult {
    agent_name: String,
    message: String,
    failed: bool,
}

pub(crate) struct ExplicitAgentDelegationTask {
    delegation: ExplicitAgentDelegation,
}

impl ExplicitAgentDelegationTask {
    pub(crate) fn new(delegation: ExplicitAgentDelegation) -> Self {
        Self { delegation }
    }

    async fn run_agent(
        session_handle: Arc<Session>,
        ctx: Arc<TurnContext>,
        target: ExplicitAgentTarget,
        mut input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<ExplicitAgentResult> {
        let role_name = target.agent_name.clone();
        let prompt = input_preview(&input);
        let mut config = match build_agent_spawn_config(
            &session_handle.get_base_instructions().await,
            ctx.as_ref(),
        ) {
            Ok(config) => config,
            Err(err) => {
                return Some(ExplicitAgentResult {
                    agent_name: target.agent_name,
                    message: format!("Unable to prepare `{role_name}`: {err}"),
                    failed: true,
                });
            }
        };
        if let Err(err) = apply_mode_agent_role_config(
            session_handle.as_ref(),
            ctx.as_ref(),
            &mut config,
            Some(role_name.as_str()),
        )
        .await
        {
            return Some(ExplicitAgentResult {
                agent_name: target.agent_name,
                message: format!("Unable to prepare `{role_name}`: {err}"),
                failed: true,
            });
        }
        if let Err(err) = apply_mode_roster_overrides(
            session_handle.as_ref(),
            ctx.as_ref(),
            &mut config,
            Some(role_name.as_str()),
            /*requested_model*/ None,
        ) {
            return Some(ExplicitAgentResult {
                agent_name: target.agent_name,
                message: format!("Unable to prepare `{role_name}`: {err}"),
                failed: true,
            });
        }
        if let Err(err) = apply_spawn_agent_runtime_overrides(&mut config, ctx.as_ref()) {
            return Some(ExplicitAgentResult {
                agent_name: target.agent_name,
                message: format!("Unable to prepare `{role_name}`: {err}"),
                failed: true,
            });
        }
        apply_requested_workspace(&mut config, &input);
        let delegated_workspace = config.cwd.display().to_string();
        let workspace_directive = format!(
            "Authoritative delegated workspace: {delegated_workspace}\nStart all file discovery, repository inspection, commands, edits, and validation in this exact directory. Do not inspect or operate on the parent DevNow launch repository unless the user explicitly names it as the target. Before acting, verify the working directory and correct it to this path if necessary."
        );
        config.developer_instructions = Some(match config.developer_instructions.take() {
            Some(instructions) if !instructions.trim().is_empty() => {
                format!("{instructions}\n\n{workspace_directive}")
            }
            Some(_) | None => workspace_directive.clone(),
        });
        input.insert(
            0,
            UserInput::Text {
                text: workspace_directive,
                text_elements: Vec::new(),
            },
        );
        let child_depth = next_thread_spawn_depth(&ctx.session_source);
        apply_spawn_agent_overrides(&mut config, child_depth);

        let call_id = format!("explicit_agent_{}_{}", ctx.sub_id, role_name);
        let model = config.model.clone().unwrap_or_default();
        let reasoning_effort = config.model_reasoning_effort.unwrap_or_default();
        session_handle
            .send_event(
                ctx.as_ref(),
                EventMsg::CollabAgentSpawnBegin(CollabAgentSpawnBeginEvent {
                    call_id: call_id.clone(),
                    sender_thread_id: session_handle.conversation_id,
                    prompt: prompt.clone(),
                    model: model.clone(),
                    reasoning_effort,
                }),
            )
            .await;

        let source = match thread_spawn_source(
            session_handle.conversation_id,
            &ctx.session_source,
            child_depth,
            Some(target.agent_role.as_str()),
            /*task_name*/ None,
            Some(target.agent_name.clone()),
        ) {
            Ok(source) => source,
            Err(err) => {
                let failure_reason = err.to_string();
                session_handle
                    .send_event(
                        ctx.as_ref(),
                        EventMsg::CollabAgentSpawnEnd(CollabAgentSpawnEndEvent {
                            call_id,
                            sender_thread_id: session_handle.conversation_id,
                            new_thread_id: None,
                            new_agent_nickname: Some(target.agent_name.clone()),
                            new_agent_role: Some(target.agent_role.clone()),
                            prompt,
                            model,
                            reasoning_effort,
                            status: AgentStatus::NotFound,
                            failure_reason: Some(failure_reason.clone()),
                        }),
                    )
                    .await;
                return Some(ExplicitAgentResult {
                    agent_name: target.agent_name,
                    message: format!("Unable to start `{role_name}`: {failure_reason}"),
                    failed: true,
                });
            }
        };
        let spawn_result = session_handle
            .services
            .agent_control
            .spawn_agent_with_metadata(config, input, Some(source), SpawnAgentOptions::default())
            .await;
        let (thread_id, nickname, agent_role, status, failure_reason) = match &spawn_result {
            Ok(agent) => (
                Some(agent.thread_id),
                agent.metadata.agent_nickname.clone(),
                agent.metadata.agent_role.clone(),
                agent.status.clone(),
                None,
            ),
            Err(err) => (
                None,
                Some(target.agent_name.clone()),
                Some(target.agent_role.clone()),
                AgentStatus::NotFound,
                Some(err.to_string()),
            ),
        };
        session_handle
            .send_event(
                ctx.as_ref(),
                EventMsg::CollabAgentSpawnEnd(CollabAgentSpawnEndEvent {
                    call_id,
                    sender_thread_id: session_handle.conversation_id,
                    new_thread_id: thread_id,
                    new_agent_nickname: nickname,
                    new_agent_role: agent_role,
                    prompt,
                    model,
                    reasoning_effort,
                    status,
                    failure_reason,
                }),
            )
            .await;

        let agent = match spawn_result {
            Ok(agent) => agent,
            Err(err) => {
                return Some(ExplicitAgentResult {
                    agent_name: target.agent_name,
                    message: format!("Unable to start `{role_name}`: {err}"),
                    failed: true,
                });
            }
        };
        let mut status_rx = match session_handle
            .services
            .agent_control
            .subscribe_status(agent.thread_id)
            .await
        {
            Ok(status_rx) => status_rx,
            Err(err) => {
                return Some(ExplicitAgentResult {
                    agent_name: target.agent_name,
                    message: format!("Unable to monitor `{role_name}`: {err}"),
                    failed: true,
                });
            }
        };
        let mut status = status_rx.borrow().clone();
        while !is_final(&status) {
            tokio::select! {
                _ = cancellation_token.cancelled() => return None,
                changed = status_rx.changed() => {
                    if changed.is_err() {
                        status = session_handle.services.agent_control.get_status(agent.thread_id).await;
                        break;
                    }
                    status = status_rx.borrow().clone();
                }
            }
        }
        let (message, failed) = match status {
            AgentStatus::Completed(Some(summary)) => (summary, false),
            AgentStatus::Completed(None) => {
                (format!("`{role_name}` completed without a summary."), false)
            }
            AgentStatus::Errored(message) => (format!("`{role_name}` failed: {message}"), true),
            status => (
                format!("`{role_name}` did not complete successfully: {status:?}"),
                true,
            ),
        };
        Some(ExplicitAgentResult {
            agent_name: target.agent_name,
            message,
            failed,
        })
    }
}

#[async_trait]
impl SessionTask for ExplicitAgentDelegationTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "session_task.explicit_agent_delegation"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        _input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        let session_handle = session.clone_session();
        session_handle
            .send_event(
                ctx.as_ref(),
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: ctx.sub_id.clone(),
                    model_context_window: ctx.model_context_window(),
                    collaboration_mode_kind: ctx.collaboration_mode.mode,
                }),
            )
            .await;
        session_handle
            .set_server_reasoning_included(/*included*/ false)
            .await;

        let child_depth = next_thread_spawn_depth(&ctx.session_source);
        if exceeds_thread_spawn_depth_limit(child_depth, ctx.config.agent_max_depth) {
            return Some(
                "Agent depth limit reached; the requested agent was not started.".to_string(),
            );
        }

        let is_parallel = self.delegation.agents.len() > 1;
        let continuation = continuation_input(session_handle.as_ref()).await;
        let delegated_agents = self
            .delegation
            .agents
            .iter()
            .map(|target| format!("@{}", target.agent_name))
            .collect::<Vec<_>>()
            .join(" ");
        record_manual_turn(
            session_handle.as_ref(),
            ctx.as_ref(),
            "user",
            format!(
                "Delegated to: {delegated_agents}\nRequest:\n{}",
                input_preview(&self.delegation.input)
            ),
        )
        .await;
        let futures = self.delegation.agents.iter().cloned().map(|target| {
            let mut input = self.delegation.input.clone();
            if let Some(continuation) = continuation.clone() {
                input.insert(0, continuation);
            }
            if is_parallel {
                input.insert(
                    0,
                    UserInput::Text {
                        text: format!(
                            "You are @{}. Multiple agents are running this plan concurrently. Execute only the assignment labeled for your agent name or role, and avoid files assigned to other agents.",
                            target.agent_name
                        ),
                        text_elements: Vec::new(),
                    },
                );
            }
            Self::run_agent(
                Arc::clone(&session_handle),
                Arc::clone(&ctx),
                target,
                input,
                cancellation_token.clone(),
            )
        });
        let results = join_all(futures).await;
        let results = results.into_iter().collect::<Option<Vec<_>>>()?;
        for result in results.iter().filter(|result| result.failed) {
            let item = TurnItem::AgentMessage(AgentMessageItem {
                id: format!(
                    "{}-explicit-agent-failure-{}",
                    ctx.sub_id, result.agent_name
                ),
                content: vec![AgentMessageContent::Text {
                    text: format!("Agent @{} failed:\n{}", result.agent_name, result.message),
                }],
                phase: None,
                memory_citation: None,
            });
            session_handle
                .emit_turn_item_started(ctx.as_ref(), &item)
                .await;
            session_handle
                .emit_turn_item_completed(ctx.as_ref(), item)
                .await;
        }
        let message = if let [result] = results.as_slice() {
            result.message.clone()
        } else {
            results
                .into_iter()
                .map(|result| format!("@{}\n{}", result.agent_name, result.message))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        record_manual_turn(
            session_handle.as_ref(),
            ctx.as_ref(),
            "assistant",
            format!("Agent result:\n{message}"),
        )
        .await;
        Some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models_manager::agentic_team::AgenticAgentConfig;
    use pretty_assertions::assert_eq;

    fn roster() -> AgenticTeamConfig {
        AgenticTeamConfig {
            team_name: None,
            default_agent: Some("developer".to_string()),
            agents: vec![
                AgenticAgentConfig {
                    name: "sr-developer".to_string(),
                    role: "sr-developer".to_string(),
                    model: "gpt-5.6-luna".to_string(),
                    model_provider: Some("openai".to_string()),
                    instructions: "Implement bounded tasks.".to_string(),
                },
                AgenticAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: Some("openai".to_string()),
                    instructions: "Implement scoped tasks.".to_string(),
                },
            ],
        }
    }

    #[test]
    fn parses_configured_agent_mention_and_removes_prefix() {
        let input = vec![UserInput::Text {
            text: "  @sr-developer - fix the calculator".to_string(),
            text_elements: Vec::new(),
        }];

        let delegation = parse_explicit_agent_delegation(&input, &roster());

        assert_eq!(
            delegation,
            Some(ExplicitAgentDelegation {
                agents: vec![ExplicitAgentTarget {
                    agent_name: "sr-developer".to_string(),
                    agent_role: "sr-developer".to_string(),
                }],
                input: vec![UserInput::Text {
                    text: "fix the calculator".to_string(),
                    text_elements: Vec::new(),
                }],
            })
        );
    }

    #[test]
    fn parses_consecutive_agent_mentions_for_parallel_delegation() {
        let input = vec![UserInput::Text {
            text: "@sr-developer @developer implement their assigned plan sections".to_string(),
            text_elements: Vec::new(),
        }];

        let delegation = parse_explicit_agent_delegation(&input, &roster());

        assert_eq!(
            delegation,
            Some(ExplicitAgentDelegation {
                agents: vec![
                    ExplicitAgentTarget {
                        agent_name: "sr-developer".to_string(),
                        agent_role: "sr-developer".to_string(),
                    },
                    ExplicitAgentTarget {
                        agent_name: "developer".to_string(),
                        agent_role: "developer".to_string(),
                    },
                ],
                input: vec![UserInput::Text {
                    text: "implement their assigned plan sections".to_string(),
                    text_elements: Vec::new(),
                }],
            })
        );
    }

    #[test]
    fn rejects_duplicate_agent_mentions() {
        let input = vec![UserInput::Text {
            text: "@developer @developer implement the plan".to_string(),
            text_elements: Vec::new(),
        }];

        assert_eq!(parse_explicit_agent_delegation(&input, &roster()), None);
    }

    #[test]
    fn ignores_unknown_agent_mention() {
        let input = vec![UserInput::Text {
            text: "@unknown fix the calculator".to_string(),
            text_elements: Vec::new(),
        }];

        assert_eq!(parse_explicit_agent_delegation(&input, &roster()), None);
    }
}
