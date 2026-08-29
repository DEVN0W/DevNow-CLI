use codex_protocol::protocol::AgentStatus;

/// Helpers for model-visible session state markers that are stored in user-role
/// messages but are not user intent.
use crate::contextual_user_message::SUBAGENT_NOTIFICATION_FRAGMENT;

// TODO(jif) unify with structured schema
pub(crate) fn format_subagent_notification_message(
    agent_reference: &str,
    status: &AgentStatus,
) -> String {
    let payload_json = serde_json::json!({
        "agent_path": agent_reference,
        "status": status,
    })
    .to_string();
    SUBAGENT_NOTIFICATION_FRAGMENT.wrap(payload_json)
}

pub(crate) fn format_subagent_visible_status_message(
    agent_nickname: Option<&str>,
    agent_role: Option<&str>,
    status: &AgentStatus,
) -> String {
    let agent_nickname = agent_nickname
        .map(str::trim)
        .filter(|agent_nickname| !agent_nickname.is_empty());
    let agent_role = agent_role
        .map(str::trim)
        .filter(|agent_role| !agent_role.is_empty());
    let agent_name = agent_nickname.or(agent_role).unwrap_or("agent");
    let role_label = agent_role.unwrap_or("agent");

    match status {
        AgentStatus::Completed(Some(message)) if !message.trim().is_empty() => {
            let message = message.trim();
            if message.starts_with("agent ")
                || message
                    .lines()
                    .any(|line| line.trim_start().starts_with("status:"))
            {
                message.to_string()
            } else {
                format!(
                    "agent {agent_name}: completed assigned stage\nsummary: returned a final response to the parent thread\nstatus: pass\nrole: {role_label}\nprogress: completed the assigned work\nnext: parent should review the child output below\nforwarded_to: parent\n\n{message}"
                )
            }
        }
        AgentStatus::Completed(_) => format!(
            "agent {agent_name}: completed assigned stage\nsummary: completed work and returned control to the parent thread\nstatus: pass\nrole: {role_label}\nprogress: finished the assigned stage\nnext: parent should continue the workflow\nforwarded_to: parent"
        ),
        AgentStatus::Errored(error) if error.contains("stream disconnected before completion") => {
            format!(
                "agent {agent_name}: remote response stream disconnected\nsummary: the child agent/API connection ended before completion; the parent session remains available\nstatus: fail\nrole: {role_label}\nprogress: {error}\nnext: parent should retry or reassign only after confirming no partial child output needs recovery\nforwarded_to: parent"
            )
        }
        AgentStatus::Errored(error) => format!(
            "agent {agent_name}: stage failed\nsummary: the child agent hit an error\nstatus: fail\nrole: {role_label}\nprogress: {error}\nnext: parent should decide whether to retry or revise the task\nforwarded_to: parent"
        ),
        AgentStatus::Interrupted => format!(
            "agent {agent_name}: stage interrupted\nsummary: the child agent was interrupted before finishing\nstatus: fail\nrole: {role_label}\nprogress: interrupted before a final response was produced\nnext: parent should decide whether to resume or retry\nforwarded_to: parent"
        ),
        AgentStatus::NotFound => format!(
            "agent {agent_name}: stage unavailable\nsummary: the child agent could not be found or started\nstatus: fail\nrole: {role_label}\nprogress: child agent was unavailable\nnext: parent should retry or reconfigure the roster\nforwarded_to: parent"
        ),
        AgentStatus::Shutdown => format!(
            "agent {agent_name}: stage shut down\nsummary: the child agent shut down before returning a final result\nstatus: fail\nrole: {role_label}\nprogress: child agent shut down before completion\nnext: parent should retry or continue manually\nforwarded_to: parent"
        ),
        AgentStatus::PendingInit | AgentStatus::Running => format!(
            "agent {agent_name}: working\nsummary: the child agent is still thinking and running\nstatus: running\nrole: {role_label}\nprogress: child agent has not reached a final state yet\nnext: parent should wait for completion\nforwarded_to: parent"
        ),
    }
}

pub(crate) fn format_subagent_context_line(
    agent_reference: &str,
    agent_nickname: Option<&str>,
) -> String {
    match agent_nickname.filter(|nickname| !nickname.is_empty()) {
        Some(agent_nickname) => format!("- {agent_reference}: {agent_nickname}"),
        None => format!("- {agent_reference}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn visible_status_message_passes_through_structured_agent_output() {
        let message = "agent Fermat: design ready\nsummary: produced the architecture\nstatus: pass\nrole: architect\nprogress: designed the system\nnext: planner should create tasks\nforwarded_to: planner";

        assert_eq!(
            format_subagent_visible_status_message(
                Some("Fermat"),
                Some("architect"),
                &AgentStatus::Completed(Some(message.to_string())),
            ),
            message
        );
    }

    #[test]
    fn visible_status_message_classifies_remote_stream_failure() {
        let message = format_subagent_visible_status_message(
            Some("Huygens"),
            Some("explorer"),
            &AgentStatus::Errored(
                "stream disconnected before completion: error sending request".to_string(),
            ),
        );

        assert!(message.contains("remote response stream disconnected"));
        assert!(message.contains("parent session remains available"));
        assert!(message.contains("retry or reassign"));
    }
}
