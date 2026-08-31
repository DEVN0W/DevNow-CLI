use crate::agent::AgentStatus;
use crate::agent::role::apply_role_to_config;
use crate::agent::role::resolve_role_config;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::config::Config;
use crate::error::CodexErr;
use crate::function_tool::FunctionCallError;
use crate::model_provider_info::DEVNOW_GATEWAY_PROVIDER_ID;
use crate::model_provider_info::OPENAI_PROVIDER_ID;
use crate::models_manager::manager::RefreshStrategy;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use codex_features::Feature;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ModeKind;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::protocol::CollabAgentRef;
use codex_protocol::protocol::CollabAgentStatusEntry;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Minimum wait timeout to prevent tight polling loops from burning CPU.
pub(crate) const MIN_WAIT_TIMEOUT_MS: i64 = 10_000;
pub(crate) const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;
pub(crate) const MAX_WAIT_TIMEOUT_MS: i64 = 3600 * 1000;

pub(crate) fn function_arguments(payload: ToolPayload) -> Result<String, FunctionCallError> {
    match payload {
        ToolPayload::Function { arguments } => Ok(arguments),
        _ => Err(FunctionCallError::RespondToModel(
            "collab handler received unsupported payload".to_string(),
        )),
    }
}

pub(crate) fn tool_output_json_text<T>(value: &T, tool_name: &str) -> String
where
    T: Serialize,
{
    serde_json::to_string(value).unwrap_or_else(|err| {
        JsonValue::String(format!("failed to serialize {tool_name} result: {err}")).to_string()
    })
}

pub(crate) fn tool_output_response_item<T>(
    call_id: &str,
    payload: &ToolPayload,
    value: &T,
    success: Option<bool>,
    tool_name: &str,
) -> ResponseInputItem
where
    T: Serialize,
{
    FunctionToolOutput::from_text(tool_output_json_text(value, tool_name), success)
        .to_response_item(call_id, payload)
}

pub(crate) fn tool_output_code_mode_result<T>(value: &T, tool_name: &str) -> JsonValue
where
    T: Serialize,
{
    serde_json::to_value(value).unwrap_or_else(|err| {
        JsonValue::String(format!("failed to serialize {tool_name} result: {err}"))
    })
}

pub(crate) fn build_wait_agent_statuses(
    statuses: &HashMap<ThreadId, AgentStatus>,
    receiver_agents: &[CollabAgentRef],
) -> Vec<CollabAgentStatusEntry> {
    if statuses.is_empty() {
        return Vec::new();
    }

    let mut entries = Vec::with_capacity(statuses.len());
    let mut seen = HashMap::with_capacity(receiver_agents.len());
    for receiver_agent in receiver_agents {
        seen.insert(receiver_agent.thread_id, ());
        if let Some(status) = statuses.get(&receiver_agent.thread_id) {
            entries.push(CollabAgentStatusEntry {
                thread_id: receiver_agent.thread_id,
                agent_nickname: receiver_agent.agent_nickname.clone(),
                agent_role: receiver_agent.agent_role.clone(),
                status: status.clone(),
            });
        }
    }

    let mut extras = statuses
        .iter()
        .filter(|(thread_id, _)| !seen.contains_key(thread_id))
        .map(|(thread_id, status)| CollabAgentStatusEntry {
            thread_id: *thread_id,
            agent_nickname: None,
            agent_role: None,
            status: status.clone(),
        })
        .collect::<Vec<_>>();
    extras.sort_by(|left, right| left.thread_id.to_string().cmp(&right.thread_id.to_string()));
    entries.extend(extras);
    entries
}

pub(crate) fn collab_spawn_error(err: CodexErr) -> FunctionCallError {
    match err {
        CodexErr::UnsupportedOperation(message) if message == "thread manager dropped" => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        CodexErr::UnsupportedOperation(message) => FunctionCallError::RespondToModel(message),
        err => FunctionCallError::RespondToModel(format!("collab spawn failed: {err}")),
    }
}

pub(crate) fn collab_agent_error(agent_id: ThreadId, err: CodexErr) -> FunctionCallError {
    match err {
        CodexErr::ThreadNotFound(id) => {
            FunctionCallError::RespondToModel(format!("agent with id {id} not found"))
        }
        CodexErr::InternalAgentDied => {
            FunctionCallError::RespondToModel(format!("agent with id {agent_id} is closed"))
        }
        CodexErr::UnsupportedOperation(_) => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        err => FunctionCallError::RespondToModel(format!("collab tool failed: {err}")),
    }
}

pub(crate) fn thread_spawn_source(
    parent_thread_id: ThreadId,
    parent_session_source: &SessionSource,
    depth: i32,
    agent_role: Option<&str>,
    task_name: Option<String>,
    preferred_agent_nickname: Option<String>,
) -> Result<SessionSource, FunctionCallError> {
    let agent_path = task_name
        .as_deref()
        .map(|task_name| {
            parent_session_source
                .get_agent_path()
                .unwrap_or_else(AgentPath::root)
                .join(task_name)
                .map_err(FunctionCallError::RespondToModel)
        })
        .transpose()?;
    Ok(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id,
        depth,
        agent_path,
        agent_nickname: preferred_agent_nickname,
        agent_role: agent_role.map(str::to_string),
    }))
}

pub(crate) fn parse_collab_input(
    message: Option<String>,
    items: Option<Vec<UserInput>>,
) -> Result<Vec<UserInput>, FunctionCallError> {
    match (message, items) {
        (Some(_), Some(_)) => Err(FunctionCallError::RespondToModel(
            "Provide either message or items, but not both".to_string(),
        )),
        (None, None) => Err(FunctionCallError::RespondToModel(
            "Provide one of: message or items".to_string(),
        )),
        (Some(message), None) => {
            if message.trim().is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "Empty message can't be sent to an agent".to_string(),
                ));
            }
            Ok(vec![UserInput::Text {
                text: message,
                text_elements: Vec::new(),
            }])
        }
        (None, Some(items)) => {
            if items.is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "Items can't be empty".to_string(),
                ));
            }
            Ok(items)
        }
    }
}

pub(crate) fn input_preview(items: &[UserInput]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|item| match item {
            UserInput::Text { text, .. } => text.clone(),
            UserInput::Image { .. } => "[image]".to_string(),
            UserInput::LocalImage { path } => format!("[local_image:{}]", path.display()),
            UserInput::Skill { name, path } => {
                format!("[skill:${name}]({})", path.display())
            }
            UserInput::Mention { name, path } => format!("[mention:${name}]({path})"),
            _ => "[input]".to_string(),
        })
        .collect();

    parts.join("\n")
}

/// Builds the base config snapshot for a newly spawned sub-agent.
///
/// The returned config starts from the parent's effective config and then refreshes the
/// runtime-owned fields carried on `turn`, including model selection, reasoning settings,
/// approval policy, sandbox, and cwd. Role-specific overrides are layered after this step;
/// skipping this helper and cloning stale config state directly can send the child agent out with
/// the wrong provider or runtime policy.
pub(crate) fn build_agent_spawn_config(
    base_instructions: &BaseInstructions,
    turn: &TurnContext,
) -> Result<Config, FunctionCallError> {
    let mut config = build_agent_shared_config(turn)?;
    config.base_instructions = Some(base_instructions.text.clone());
    Ok(config)
}

pub(crate) fn apply_mode_roster_overrides(
    session: &Session,
    turn: &TurnContext,
    config: &mut Config,
    role_name: Option<&str>,
    requested_model: Option<&str>,
) -> Result<(), FunctionCallError> {
    let (agent_name, agent_role, agent_instructions, model, model_provider_id, roster_name) =
        match turn.collaboration_mode.mode {
            ModeKind::Agentic => {
                let Some(team) = session.services.models_manager.agentic_team() else {
                    return Err(missing_roster_error(
                        turn,
                        "agentic.toml",
                        "manual agent roster",
                    ));
                };
                let Some(agent) = team.agent_for_request(role_name) else {
                    let missing_role = role_name.unwrap_or("default");
                    return Err(FunctionCallError::RespondToModel(format!(
                        "manual agent roster in agentic.toml has no configured agent for `{missing_role}`; set `default_agent` or provide an explicit `agent_type`"
                    )));
                };
                (
                    agent.name.as_str(),
                    agent.role.as_str(),
                    agent.instructions.as_str(),
                    agent.model.as_str(),
                    agent.model_provider.as_deref(),
                    "manual agent roster in agentic.toml",
                )
            }
            ModeKind::MultiAgent => {
                let Some(team) = session.services.models_manager.multi_agent_team() else {
                    return Err(missing_roster_error(
                        turn,
                        "multi_agent.toml",
                        "strategic multi-agent roster",
                    ));
                };
                let Some(agent) = team.agent_for_request(role_name) else {
                    let missing_role = role_name.unwrap_or("default");
                    return Err(FunctionCallError::RespondToModel(format!(
                        "strategic multi-agent roster in multi_agent.toml has no configured agent for `{missing_role}`; set `default_agent` or provide an explicit `agent_type`"
                    )));
                };
                (
                    agent.name.as_str(),
                    agent.role.as_str(),
                    agent.instructions.as_str(),
                    agent.model.as_str(),
                    agent.model_provider.as_deref(),
                    "strategic multi-agent roster in multi_agent.toml",
                )
            }
            ModeKind::Default | ModeKind::Plan | ModeKind::PairProgramming | ModeKind::Execute => {
                return Ok(());
            }
        };

    config.developer_instructions = Some(format!(
        "You are `{agent_name}`, the configured `{agent_role}` agent. Stay within this role.\n\n{agent_instructions}"
    ));

    if requested_model.is_none() {
        config.model = Some(model.to_string());
    }

    let Some(model_provider_id) = model_provider_id.filter(|_| requested_model.is_none()) else {
        return Ok(());
    };
    let Some(model_provider) = config.model_providers.get(model_provider_id).cloned() else {
        return Err(FunctionCallError::RespondToModel(format!(
            "{roster_name} references unavailable model provider `{model_provider_id}` for `{agent_name}`; configure that provider for this thread"
        )));
    };
    config.model_provider_id = model_provider_id.to_string();
    config.model_provider = model_provider;
    Ok(())
}

pub(crate) async fn apply_mode_agent_role_config(
    session: &Session,
    turn: &TurnContext,
    config: &mut Config,
    role_name: Option<&str>,
) -> Result<(), FunctionCallError> {
    let role_name = resolve_mode_agent_role_name(session, turn, role_name)?;
    let role_name = role_name.as_deref();
    if role_name.is_some_and(|role_name| resolve_role_config(config, role_name).is_none())
        && mode_roster_has_agent(session, turn, role_name)
    {
        return Ok(());
    }
    apply_role_to_config(config, role_name)
        .await
        .map_err(FunctionCallError::RespondToModel)
}

fn mode_roster_has_agent(session: &Session, turn: &TurnContext, role_name: Option<&str>) -> bool {
    match turn.collaboration_mode.mode {
        ModeKind::Agentic => session
            .services
            .models_manager
            .agentic_team()
            .and_then(|team| team.agent_for_request(role_name))
            .is_some(),
        ModeKind::MultiAgent => session
            .services
            .models_manager
            .multi_agent_team()
            .and_then(|team| team.agent_for_request(role_name))
            .is_some(),
        ModeKind::Default | ModeKind::Plan | ModeKind::PairProgramming | ModeKind::Execute => false,
    }
}

pub(crate) fn apply_multi_agent_team_overrides(
    session: &Session,
    config: &mut Config,
    role_name: Option<&str>,
    requested_model: Option<&str>,
) -> Result<(), FunctionCallError> {
    let Some(team) = session.services.models_manager.multi_agent_team() else {
        return Ok(());
    };
    let Some(agent) = team.agent_for_request(role_name) else {
        let missing_role = role_name.unwrap_or("default");
        return Err(FunctionCallError::RespondToModel(format!(
            "strategic multi-agent roster in multi_agent.toml has no configured agent for `{missing_role}`; set `default_agent` or provide an explicit `agent_type`"
        )));
    };

    if requested_model.is_none() {
        config.model = Some(agent.model.clone());
    }
    if let Some(model_provider_id) = agent.model_provider.as_ref() {
        let Some(model_provider) = config.model_providers.get(model_provider_id).cloned() else {
            return Err(FunctionCallError::RespondToModel(format!(
                "strategic multi-agent roster in multi_agent.toml references unavailable model provider `{model_provider_id}` for `{}`; configure that provider for this thread",
                agent.name
            )));
        };
        config.model_provider_id = model_provider_id.clone();
        config.model_provider = model_provider;
    }
    Ok(())
}

pub(crate) fn resolve_mode_agent_role_name(
    session: &Session,
    turn: &TurnContext,
    role_name: Option<&str>,
) -> Result<Option<String>, FunctionCallError> {
    if let Some(role_name) = role_name {
        return Ok(Some(role_name.to_string()));
    }

    let default_agent = match turn.collaboration_mode.mode {
        ModeKind::Agentic => session
            .services
            .models_manager
            .agentic_team()
            .ok_or_else(|| missing_roster_error(turn, "agentic.toml", "manual agent roster"))?
            .default_agent
            .clone(),
        ModeKind::MultiAgent => session
            .services
            .models_manager
            .multi_agent_team()
            .ok_or_else(|| {
                missing_roster_error(turn, "multi_agent.toml", "strategic multi-agent roster")
            })?
            .default_agent
            .clone(),
        ModeKind::Default | ModeKind::Plan | ModeKind::PairProgramming | ModeKind::Execute => None,
    };
    if default_agent.is_some() {
        return Ok(default_agent);
    }
    if turn.collaboration_mode.mode.uses_manual_roster()
        || turn.collaboration_mode.mode.uses_strategic_roster()
    {
        return Err(FunctionCallError::RespondToModel(format!(
            "{} needs `default_agent` or an explicit `agent_type`",
            turn.collaboration_mode.mode.display_name()
        )));
    }
    Ok(None)
}

fn missing_roster_error(
    turn: &TurnContext,
    file_name: &str,
    roster_description: &str,
) -> FunctionCallError {
    FunctionCallError::RespondToModel(format!(
        "{roster_description} is unavailable; expected a valid configuration at {}",
        turn.config.codex_home.join(file_name).display()
    ))
}

pub(crate) fn build_agent_resume_config(
    turn: &TurnContext,
    child_depth: i32,
) -> Result<Config, FunctionCallError> {
    let mut config = build_agent_shared_config(turn)?;
    apply_spawn_agent_overrides(&mut config, child_depth);
    // For resume, keep base instructions sourced from rollout/session metadata.
    config.base_instructions = None;
    Ok(config)
}

fn build_agent_shared_config(turn: &TurnContext) -> Result<Config, FunctionCallError> {
    let base_config = turn.config.clone();
    let mut config = (*base_config).clone();
    config.model = Some(turn.model_info.slug.clone());
    config.model_provider = turn.provider.clone();
    config.model_reasoning_effort = turn.reasoning_effort;
    config.model_reasoning_summary = Some(turn.reasoning_summary);
    config.developer_instructions = turn.developer_instructions.clone();
    config.compact_prompt = turn.compact_prompt.clone();
    apply_spawn_agent_runtime_overrides(&mut config, turn)?;

    Ok(config)
}

/// Copies runtime-only turn state onto a child config before it is handed to `AgentControl`.
///
/// These values are chosen by the live turn rather than persisted config, so leaving them stale
/// can make a child agent disagree with its parent about approval policy, cwd, or sandboxing.
pub(crate) fn apply_spawn_agent_runtime_overrides(
    config: &mut Config,
    turn: &TurnContext,
) -> Result<(), FunctionCallError> {
    config
        .permissions
        .approval_policy
        .set(turn.approval_policy.value())
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("approval_policy is invalid: {err}"))
        })?;
    config.permissions.shell_environment_policy = turn.shell_environment_policy.clone();
    config.codex_linux_sandbox_exe = turn.codex_linux_sandbox_exe.clone();
    config.cwd = turn.cwd.clone();
    config
        .permissions
        .sandbox_policy
        .set(turn.sandbox_policy.get().clone())
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("sandbox_policy is invalid: {err}"))
        })?;
    config.permissions.file_system_sandbox_policy = turn.file_system_sandbox_policy.clone();
    config.permissions.network_sandbox_policy = turn.network_sandbox_policy;
    Ok(())
}

pub(crate) fn apply_spawn_agent_overrides(config: &mut Config, child_depth: i32) {
    if child_depth >= config.agent_max_depth && !config.features.enabled(Feature::MultiAgentV2) {
        let _ = config.features.disable(Feature::SpawnCsv);
        let _ = config.features.disable(Feature::Collab);
    }
}

pub(crate) async fn apply_requested_spawn_agent_model_overrides(
    session: &Session,
    turn: &TurnContext,
    config: &mut Config,
    requested_model: Option<&str>,
    requested_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), FunctionCallError> {
    if requested_model.is_none() && requested_reasoning_effort.is_none() {
        return Ok(());
    }

    if let Some(requested_model) = requested_model {
        let available_models = session
            .services
            .models_manager
            .list_models(RefreshStrategy::Offline)
            .await;
        let selected_model_name = find_spawn_agent_model_name(&available_models, requested_model)?;
        let selected_model_info = session
            .services
            .models_manager
            .get_model_info(&selected_model_name, config)
            .await;

        config.model = Some(selected_model_name.clone());
        apply_requested_model_provider(config, &selected_model_name)?;
        if let Some(reasoning_effort) = requested_reasoning_effort {
            validate_spawn_agent_reasoning_effort(
                &selected_model_name,
                &selected_model_info.supported_reasoning_levels,
                reasoning_effort,
            )?;
            config.model_reasoning_effort = Some(reasoning_effort);
        } else {
            config.model_reasoning_effort = selected_model_info.default_reasoning_level;
        }

        return Ok(());
    }

    if let Some(reasoning_effort) = requested_reasoning_effort {
        validate_spawn_agent_reasoning_effort(
            &turn.model_info.slug,
            &turn.model_info.supported_reasoning_levels,
            reasoning_effort,
        )?;
        config.model_reasoning_effort = Some(reasoning_effort);
    }

    Ok(())
}

fn apply_requested_model_provider(
    config: &mut Config,
    requested_model: &str,
) -> Result<(), FunctionCallError> {
    let provider_id = if requested_model.starts_with("claude-") {
        Some(DEVNOW_GATEWAY_PROVIDER_ID)
    } else if config.model_provider_id == DEVNOW_GATEWAY_PROVIDER_ID {
        Some(OPENAI_PROVIDER_ID)
    } else {
        None
    };
    let Some(provider_id) = provider_id else {
        return Ok(());
    };
    let Some(provider) = config.model_providers.get(provider_id).cloned() else {
        return Err(FunctionCallError::RespondToModel(format!(
            "requested model `{requested_model}` requires unavailable model provider `{provider_id}`"
        )));
    };
    config.model_provider_id = provider_id.to_string();
    config.model_provider = provider;
    Ok(())
}

fn find_spawn_agent_model_name(
    available_models: &[codex_protocol::openai_models::ModelPreset],
    requested_model: &str,
) -> Result<String, FunctionCallError> {
    available_models
        .iter()
        .find(|model| model.model == requested_model)
        .map(|model| model.model.clone())
        .ok_or_else(|| {
            let available = available_models
                .iter()
                .map(|model| model.model.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            FunctionCallError::RespondToModel(format!(
                "Unknown model `{requested_model}` for spawn_agent. Available models: {available}"
            ))
        })
}

fn validate_spawn_agent_reasoning_effort(
    model: &str,
    supported_reasoning_levels: &[ReasoningEffortPreset],
    requested_reasoning_effort: ReasoningEffort,
) -> Result<(), FunctionCallError> {
    if supported_reasoning_levels
        .iter()
        .any(|preset| preset.effort == requested_reasoning_effort)
    {
        return Ok(());
    }

    let supported = supported_reasoning_levels
        .iter()
        .map(|preset| preset.effort.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(FunctionCallError::RespondToModel(format!(
        "Reasoning effort `{requested_reasoning_effort}` is not supported for model `{model}`. Supported reasoning efforts: {supported}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigBuilder;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    async fn test_config() -> Config {
        let home = tempdir().expect("create temp dir");
        ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .build()
            .await
            .expect("build config")
    }

    #[tokio::test]
    async fn requested_claude_model_reuses_managed_claude_provider() {
        let mut config = test_config().await;

        apply_requested_model_provider(&mut config, "claude-fable-5")
            .expect("select Claude provider");

        assert_eq!(config.model_provider_id, DEVNOW_GATEWAY_PROVIDER_ID);
        assert_eq!(
            config.model_provider,
            config.model_providers[DEVNOW_GATEWAY_PROVIDER_ID]
        );
    }

    #[tokio::test]
    async fn requested_gpt_model_resets_managed_claude_provider_to_openai() {
        let mut config = test_config().await;
        config.model_provider_id = DEVNOW_GATEWAY_PROVIDER_ID.to_string();
        config.model_provider = config.model_providers[DEVNOW_GATEWAY_PROVIDER_ID].clone();

        apply_requested_model_provider(&mut config, "gpt-5.4").expect("select OpenAI provider");

        assert_eq!(config.model_provider_id, OPENAI_PROVIDER_ID);
        assert_eq!(
            config.model_provider,
            config.model_providers[OPENAI_PROVIDER_ID]
        );
    }
}
