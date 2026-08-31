use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::user_input::UserInput;

use crate::codex::Session;
use crate::codex::TurnContext;

const MANUAL_HISTORY_OPEN: &str = "<devnow_manual_agent_handoff>";
const MANUAL_HISTORY_CLOSE: &str = "</devnow_manual_agent_handoff>";
const MAX_CONTINUATION_ENTRIES: usize = 8;
const MAX_CONTINUATION_CHARS: usize = 24_000;

pub(super) async fn continuation_input(session: &Session) -> Option<UserInput> {
    let history = session.clone_history().await;
    continuation_text(history.raw_items()).map(|text| UserInput::Text {
        text,
        text_elements: Vec::new(),
    })
}

pub(super) async fn record_manual_turn(
    session: &Session,
    ctx: &TurnContext,
    role: &str,
    body: String,
) {
    let content = format!("{MANUAL_HISTORY_OPEN}\n{body}\n{MANUAL_HISTORY_CLOSE}");
    let item = ResponseItem::Message {
        id: None,
        role: role.to_string(),
        content: vec![if role == "assistant" {
            ContentItem::OutputText { text: content }
        } else {
            ContentItem::InputText { text: content }
        }],
        end_turn: None,
        phase: None,
    };
    session
        .record_into_history(std::slice::from_ref(&item), ctx)
        .await;
    session
        .persist_rollout_items(&[RolloutItem::ResponseItem(item)])
        .await;
    session.ensure_rollout_materialized().await;
}

fn continuation_text(items: &[ResponseItem]) -> Option<String> {
    let mut entries = items
        .iter()
        .filter_map(manual_history_text)
        .rev()
        .take(MAX_CONTINUATION_ENTRIES)
        .collect::<Vec<_>>();
    entries.reverse();
    if entries.is_empty() {
        return None;
    }

    let mut text = String::from(
        "Continuation history from earlier manually delegated agents in this DevNow session. Use it as prior work and preserve its target workspace. The newest user instruction remains authoritative.\n\n",
    );
    for entry in entries {
        if text.len().saturating_add(entry.len()) > MAX_CONTINUATION_CHARS {
            break;
        }
        text.push_str(entry);
        text.push_str("\n\n");
    }
    Some(text.trim_end().to_string())
}

fn manual_history_text(item: &ResponseItem) -> Option<&str> {
    let ResponseItem::Message { content, .. } = item else {
        return None;
    };
    content.iter().find_map(|item| match item {
        ContentItem::InputText { text } | ContentItem::OutputText { text }
            if text.starts_with(MANUAL_HISTORY_OPEN) && text.ends_with(MANUAL_HISTORY_CLOSE) =>
        {
            Some(text.as_str())
        }
        ContentItem::InputText { .. }
        | ContentItem::InputImage { .. }
        | ContentItem::OutputText { .. } => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn message(role: &str, text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: role.to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            end_turn: None,
            phase: None,
        }
    }

    #[test]
    fn continuation_prompt_keeps_previous_agent_handoff_and_workspace() {
        let items = vec![
            message("user", "# AGENTS.md instructions for C:\\devnow"),
            message(
                "user",
                &format!(
                    "{MANUAL_HISTORY_OPEN}\nDelegated to: @sr-developer\nRequest: create calculator in C:\\agent_works\\calculator\n{MANUAL_HISTORY_CLOSE}"
                ),
            ),
            message(
                "assistant",
                &format!(
                    "{MANUAL_HISTORY_OPEN}\nAgent result: Created calculator.py and verified arithmetic.\n{MANUAL_HISTORY_CLOSE}"
                ),
            ),
        ];

        let continuation = continuation_text(&items).expect("continuation history");

        assert!(continuation.contains(r"C:\agent_works\calculator"));
        assert!(continuation.contains("Created calculator.py"));
        assert!(!continuation.contains("AGENTS.md instructions for C:\\devnow"));
    }

    #[test]
    fn continuation_prompt_is_absent_without_manual_handoffs() {
        assert_eq!(
            continuation_text(&[message("user", "ordinary history")]),
            None
        );
    }
}
