//! Helpers for rendering and navigating multi-agent state in the TUI.
//!
//! This module owns the shared presentation contracts for multi-agent history rows, `/agent` picker
//! entries, and the fast-switch keyboard shortcuts. Higher-level coordination, such as deciding
//! which thread becomes active or when a thread closes, stays in [`crate::app::App`].

use crate::history_cell::PlainHistoryCell;
use crate::render::line_utils::prefix_lines;
use crate::text_formatting::truncate_text;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::CollabAgentInteractionBeginEvent;
use codex_protocol::protocol::CollabAgentInteractionEndEvent;
use codex_protocol::protocol::CollabAgentRef;
use codex_protocol::protocol::CollabAgentSpawnBeginEvent;
use codex_protocol::protocol::CollabAgentSpawnEndEvent;
use codex_protocol::protocol::CollabAgentStatusEntry;
use codex_protocol::protocol::CollabCloseEndEvent;
use codex_protocol::protocol::CollabResumeBeginEvent;
use codex_protocol::protocol::CollabResumeEndEvent;
use codex_protocol::protocol::CollabWaitingBeginEvent;
use codex_protocol::protocol::CollabWaitingEndEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
#[cfg(target_os = "macos")]
use crossterm::event::KeyEventKind;
#[cfg(target_os = "macos")]
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::collections::HashMap;
use std::collections::HashSet;

const COLLAB_PROMPT_PREVIEW_GRAPHEMES: usize = 160;
const COLLAB_AGENT_ERROR_PREVIEW_GRAPHEMES: usize = 160;
const COLLAB_AGENT_RESPONSE_PREVIEW_GRAPHEMES: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentPickerThreadEntry {
    /// Human-friendly nickname shown in picker rows and footer labels.
    pub(crate) agent_nickname: Option<String>,
    /// Agent type shown in brackets when present, for example `worker`.
    pub(crate) agent_role: Option<String>,
    /// Whether the thread has emitted a close event and should render dimmed.
    pub(crate) is_closed: bool,
}

#[derive(Clone, Copy)]
struct AgentLabel<'a> {
    nickname: Option<&'a str>,
    role: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnRequestSummary {
    pub(crate) model: String,
    pub(crate) reasoning_effort: ReasoningEffortConfig,
}

pub(crate) fn agent_picker_status_dot_spans(is_closed: bool) -> Vec<Span<'static>> {
    let dot = if is_closed {
        "•".into()
    } else {
        "•".green()
    };
    vec![dot, " ".into()]
}

pub(crate) fn format_agent_picker_item_name(
    agent_nickname: Option<&str>,
    agent_role: Option<&str>,
    is_primary: bool,
) -> String {
    if is_primary {
        return "Main [default]".to_string();
    }

    let agent_nickname = agent_nickname
        .map(str::trim)
        .filter(|nickname| !nickname.is_empty());
    let agent_role = agent_role.map(str::trim).filter(|role| !role.is_empty());
    match (agent_nickname, agent_role) {
        (Some(agent_nickname), Some(agent_role)) => format!("{agent_nickname} [{agent_role}]"),
        (Some(agent_nickname), None) => agent_nickname.to_string(),
        (None, Some(agent_role)) => format!("[{agent_role}]"),
        (None, None) => "Agent".to_string(),
    }
}

pub(crate) fn previous_agent_shortcut() -> crate::key_hint::KeyBinding {
    crate::key_hint::alt(KeyCode::Left)
}

pub(crate) fn next_agent_shortcut() -> crate::key_hint::KeyBinding {
    crate::key_hint::alt(KeyCode::Right)
}

/// Matches the canonical "previous agent" binding plus platform-specific fallbacks that keep agent
/// navigation working when enhanced key reporting is unavailable.
pub(crate) fn previous_agent_shortcut_matches(
    key_event: KeyEvent,
    allow_word_motion_fallback: bool,
) -> bool {
    previous_agent_shortcut().is_press(key_event)
        || previous_agent_word_motion_fallback(key_event, allow_word_motion_fallback)
}

/// Matches the canonical "next agent" binding plus platform-specific fallbacks that keep agent
/// navigation working when enhanced key reporting is unavailable.
pub(crate) fn next_agent_shortcut_matches(
    key_event: KeyEvent,
    allow_word_motion_fallback: bool,
) -> bool {
    next_agent_shortcut().is_press(key_event)
        || next_agent_word_motion_fallback(key_event, allow_word_motion_fallback)
}

#[cfg(target_os = "macos")]
fn previous_agent_word_motion_fallback(
    key_event: KeyEvent,
    allow_word_motion_fallback: bool,
) -> bool {
    // Some terminals, especially on macOS, send Option+b/f as word-motion keys instead of
    // Option+arrow events unless enhanced keyboard reporting is enabled. Callers should only
    // enable this fallback when the composer is empty so draft editing retains the expected
    // word-wise motion behavior.
    allow_word_motion_fallback
        && matches!(
            key_event,
            KeyEvent {
                code: KeyCode::Char('b'),
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }
        )
}

#[cfg(not(target_os = "macos"))]
fn previous_agent_word_motion_fallback(
    _key_event: KeyEvent,
    _allow_word_motion_fallback: bool,
) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn next_agent_word_motion_fallback(key_event: KeyEvent, allow_word_motion_fallback: bool) -> bool {
    // Some terminals, especially on macOS, send Option+b/f as word-motion keys instead of
    // Option+arrow events unless enhanced keyboard reporting is enabled. Callers should only
    // enable this fallback when the composer is empty so draft editing retains the expected
    // word-wise motion behavior.
    allow_word_motion_fallback
        && matches!(
            key_event,
            KeyEvent {
                code: KeyCode::Char('f'),
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }
        )
}

#[cfg(not(target_os = "macos"))]
fn next_agent_word_motion_fallback(
    _key_event: KeyEvent,
    _allow_word_motion_fallback: bool,
) -> bool {
    false
}

pub(crate) fn spawn_end(
    ev: CollabAgentSpawnEndEvent,
    spawn_request: Option<&SpawnRequestSummary>,
) -> PlainHistoryCell {
    let CollabAgentSpawnEndEvent {
        call_id: _,
        sender_thread_id: _,
        new_thread_id,
        new_agent_nickname,
        new_agent_role,
        prompt,
        failure_reason,
        status: _,
        ..
    } = ev;
    let pending_spawn = failure_reason.as_deref().is_some_and(|reason| {
        reason == "No spawned thread id was returned"
            || reason.starts_with("Spawned thread ")
                && reason.contains("was not reported in agents_states")
    });

    let title = match new_thread_id {
        Some(_) => title_with_agent(
            "Spawned",
            AgentLabel {
                nickname: new_agent_nickname.as_deref(),
                role: new_agent_role.as_deref(),
            },
            spawn_request,
        ),
        None if pending_spawn => title_with_agent(
            "Spawn pending",
            AgentLabel {
                nickname: new_agent_nickname.as_deref(),
                role: new_agent_role.as_deref(),
            },
            spawn_request,
        ),
        None => title_with_agent(
            "Spawn failed",
            AgentLabel {
                nickname: new_agent_nickname.as_deref(),
                role: new_agent_role.as_deref(),
            },
            spawn_request,
        ),
    };

    let mut details = Vec::new();
    if new_thread_id.is_some() {
        details.push(working_line("Spawn complete; agent is now running"));
        details.push(forwarded_to_line(
            new_agent_nickname.as_deref(),
            new_agent_role.as_deref(),
        ));
        if let Some(line) = prompt_line(&prompt) {
            details.push(line);
        }
    } else if pending_spawn {
        details.push(working_line(
            "Spawn still pending; waiting for the spawned thread id",
        ));
        details.push(summary_line(
            "spawn completed but the thread id has not been surfaced yet",
        ));
        details.push(progress_line(
            "progress",
            "waiting for the backend spawn event to complete",
        ));
        details.push(next_line(
            "keep waiting for the spawned agent to come online",
        ));
        if let Some(line) = prompt_line(&prompt) {
            details.push(line);
        }
    } else if let Some(reason) = failure_reason {
        details.push(working_line(
            "Spawn failed; returning to developer for fixes",
        ));
        details.push(Line::from(Span::from(format!("Reason: {reason}"))));
    }
    collab_event(title, details)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn spawn_begin(
    ev: CollabAgentSpawnBeginEvent,
    spawn_request: Option<&SpawnRequestSummary>,
) -> PlainHistoryCell {
    let CollabAgentSpawnBeginEvent {
        call_id: _,
        sender_thread_id: _,
        prompt,
        model: _,
        reasoning_effort: _,
    } = ev;

    let mut spans = vec![Span::from("Spawning").bold()];
    spans.extend(spawn_request_spans(spawn_request));
    let title = title_spans_line(spans);

    let mut details = Vec::new();
    details.push(working_line("Spawning agent"));
    details.push(summary_line("creating the requested child agent"));
    details.push(progress_line(
        "progress",
        "spawn request queued and waiting for a live thread",
    ));
    details.push(next_line("wait for the spawned agent to come online"));
    if let Some(line) = prompt_line(&prompt) {
        details.push(line);
    }
    collab_event(title, details)
}

pub(crate) fn interaction_end(ev: CollabAgentInteractionEndEvent) -> PlainHistoryCell {
    let CollabAgentInteractionEndEvent {
        call_id: _,
        sender_thread_id: _,
        receiver_thread_id: _,
        receiver_agent_nickname,
        receiver_agent_role,
        prompt,
        status,
    } = ev;

    let title = title_with_agent(
        "Forwarded to",
        AgentLabel {
            nickname: receiver_agent_nickname.as_deref(),
            role: receiver_agent_role.as_deref(),
        },
        /*spawn_request*/ None,
    );

    let mut details = Vec::new();
    details.push(working_line("Handoff delivered"));
    details.push(summary_line("task routed to the child agent"));
    details.push(progress_line("progress", "input sent to agent"));
    details.push(next_line("wait for the child agent to respond"));
    if matches!(status, AgentStatus::Running | AgentStatus::PendingInit) {
        details.push(blocked_line("waiting on agent response"));
    }
    if let Some(line) = prompt_line(&prompt) {
        details.push(line);
    }
    collab_event(title, details)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn interaction_begin(ev: CollabAgentInteractionBeginEvent) -> PlainHistoryCell {
    let CollabAgentInteractionBeginEvent {
        call_id: _,
        sender_thread_id: _,
        receiver_thread_id: _,
        prompt,
    } = ev;

    let title = title_text("Routing to agent");

    let mut details = Vec::new();
    details.push(working_line("Preparing handoff"));
    details.push(summary_line("routing the task to the target agent"));
    details.push(progress_line(
        "progress",
        "handoff request queued for delivery",
    ));
    details.push(next_line("child agent should receive the assignment"));
    details.push(forwarded_to_line(None, Some("in-flight routing target")));
    if let Some(line) = prompt_line(&prompt) {
        details.push(line);
    }
    collab_event(title, details)
}

pub(crate) fn waiting_begin(ev: CollabWaitingBeginEvent) -> PlainHistoryCell {
    let CollabWaitingBeginEvent {
        sender_thread_id: _,
        receiver_thread_ids,
        receiver_agents,
        call_id: _,
    } = ev;
    let receiver_agents = merge_wait_receivers(&receiver_thread_ids, receiver_agents);

    let title = match receiver_agents.as_slice() {
        [receiver] => title_with_agent(
            "Blocked on",
            agent_label_from_ref(receiver),
            /*spawn_request*/ None,
        ),
        [] => title_text("Waiting for agents"),
        _ => title_text(format!("Waiting for {} agents", receiver_agents.len())),
    };

    let details = if receiver_agents.len() > 1 {
        std::iter::once(working_line("Waiting for all routed agents to respond"))
            .chain(std::iter::once(summary_line(
                "multiple child agents are still in flight",
            )))
            .chain(std::iter::once(progress_line(
                "progress",
                "no child agent has returned a final answer yet",
            )))
            .chain(std::iter::once(next_line("wait for all routed agents")))
            .chain(receiver_agents.iter().map(|receiver| {
                forwarded_to_line(
                    receiver.agent_nickname.as_deref(),
                    receiver.agent_role.as_deref(),
                )
            }))
            .collect()
    } else {
        let mut details = vec![working_line("Waiting for agent response")];
        details.push(summary_line("a single child agent is still running"));
        details.push(progress_line(
            "progress",
            "no final status has been received yet",
        ));
        details.push(next_line("parent should wait or retry if needed"));
        if let Some(receiver) = receiver_agents.first() {
            details.push(blocked_line(format!(
                "waiting on {}",
                agent_label_text(agent_label_from_ref(receiver))
            )));
        }
        details
    };

    collab_event(title, details)
}

pub(crate) fn waiting_end(ev: CollabWaitingEndEvent) -> PlainHistoryCell {
    let CollabWaitingEndEvent {
        call_id: _,
        sender_thread_id: _,
        agent_statuses,
        statuses,
    } = ev;
    let details = wait_complete_lines(&statuses, &agent_statuses);
    collab_event(title_text("Finished waiting"), details)
}

pub(crate) fn close_end(ev: CollabCloseEndEvent) -> PlainHistoryCell {
    let CollabCloseEndEvent {
        call_id: _,
        sender_thread_id: _,
        receiver_thread_id: _,
        receiver_agent_nickname,
        receiver_agent_role,
        status: _,
    } = ev;

    collab_event(
        title_with_agent(
            "Closed",
            AgentLabel {
                nickname: receiver_agent_nickname.as_deref(),
                role: receiver_agent_role.as_deref(),
            },
            /*spawn_request*/ None,
        ),
        Vec::new(),
    )
}

pub(crate) fn resume_begin(ev: CollabResumeBeginEvent) -> PlainHistoryCell {
    let CollabResumeBeginEvent {
        call_id: _,
        sender_thread_id: _,
        receiver_thread_id: _,
        receiver_agent_nickname,
        receiver_agent_role,
    } = ev;

    collab_event(
        title_with_agent(
            "Resuming",
            AgentLabel {
                nickname: receiver_agent_nickname.as_deref(),
                role: receiver_agent_role.as_deref(),
            },
            /*spawn_request*/ None,
        ),
        Vec::new(),
    )
}

pub(crate) fn resume_end(ev: CollabResumeEndEvent) -> PlainHistoryCell {
    let CollabResumeEndEvent {
        call_id: _,
        sender_thread_id: _,
        receiver_thread_id: _,
        receiver_agent_nickname,
        receiver_agent_role,
        status,
    } = ev;

    collab_event(
        title_with_agent(
            "Resumed",
            AgentLabel {
                nickname: receiver_agent_nickname.as_deref(),
                role: receiver_agent_role.as_deref(),
            },
            /*spawn_request*/ None,
        ),
        vec![status_summary_line(&status)],
    )
}

fn collab_event(title: Line<'static>, details: Vec<Line<'static>>) -> PlainHistoryCell {
    let mut lines: Vec<Line<'static>> = vec![title];
    if !details.is_empty() {
        lines.extend(prefix_lines(details, "  └ ".dim(), "    ".into()));
    }
    PlainHistoryCell::new(lines)
}

fn title_text(title: impl Into<String>) -> Line<'static> {
    title_spans_line(vec![Span::from(title.into()).bold()])
}

fn title_with_agent(
    prefix: &str,
    agent: AgentLabel<'_>,
    spawn_request: Option<&SpawnRequestSummary>,
) -> Line<'static> {
    let mut spans = vec![Span::from(format!("{prefix} ")).bold()];
    spans.extend(agent_label_spans(agent));
    spans.extend(spawn_request_spans(spawn_request));
    title_spans_line(spans)
}

fn title_spans_line(mut spans: Vec<Span<'static>>) -> Line<'static> {
    let mut title = Vec::with_capacity(spans.len() + 1);
    title.push(Span::from("• ").dim());
    title.append(&mut spans);
    title.into()
}

fn agent_label_from_ref(agent: &CollabAgentRef) -> AgentLabel<'_> {
    AgentLabel {
        nickname: agent.agent_nickname.as_deref(),
        role: agent.agent_role.as_deref(),
    }
}

fn forwarded_to_line(nickname: Option<&str>, role: Option<&str>) -> Line<'static> {
    let mut spans = vec![Span::from("Forwarded to: ").bold()];
    spans.extend(agent_label_spans(AgentLabel { nickname, role }));
    spans.into()
}

fn working_line(message: impl Into<String>) -> Line<'static> {
    vec![
        Span::from("Working: ").cyan().bold(),
        Span::from(message.into()),
    ]
    .into()
}

fn summary_line(message: impl Into<String>) -> Line<'static> {
    vec![Span::from("Summary: ").dim(), Span::from(message.into())].into()
}

fn blocked_line(message: impl Into<String>) -> Line<'static> {
    vec![
        Span::from("Blocked: ").yellow().bold(),
        Span::from(message.into()),
    ]
    .into()
}

fn next_line(message: impl Into<String>) -> Line<'static> {
    vec![Span::from("Next: ").dim(), Span::from(message.into())].into()
}

fn progress_line(label: &str, message: impl Into<String>) -> Line<'static> {
    vec![
        Span::from(format!("{label}: ")).dim(),
        Span::from(message.into()),
    ]
    .into()
}

fn agent_label_text(agent: AgentLabel<'_>) -> String {
    let nickname = agent
        .nickname
        .map(str::trim)
        .filter(|nickname| !nickname.is_empty());
    let role = agent.role.map(str::trim).filter(|role| !role.is_empty());
    match (nickname, role) {
        (Some(nickname), Some(role)) => format!("{nickname} [{role}]"),
        (Some(nickname), None) => nickname.to_string(),
        (None, Some(role)) => role.to_string(),
        (None, None) => "agent".to_string(),
    }
}

fn agent_label_spans(agent: AgentLabel<'_>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let nickname = agent
        .nickname
        .map(str::trim)
        .filter(|nickname| !nickname.is_empty());
    let role = agent.role.map(str::trim).filter(|role| !role.is_empty());

    if let Some(nickname) = nickname {
        spans.push(Span::from(nickname.to_string()).cyan().bold());
        if let Some(role) = role {
            spans.push(Span::from(" ").dim());
            spans.push(Span::from(format!("[{role}]")));
        }
    } else if let Some(role) = role {
        spans.push(Span::from(role.to_string()).cyan().bold());
    } else {
        spans.push(Span::from("agent").cyan());
    }

    spans
}

fn spawn_request_spans(spawn_request: Option<&SpawnRequestSummary>) -> Vec<Span<'static>> {
    let Some(spawn_request) = spawn_request else {
        return Vec::new();
    };

    let model = spawn_request.model.trim();
    if model.is_empty() && spawn_request.reasoning_effort == ReasoningEffortConfig::default() {
        return Vec::new();
    }

    let details = if model.is_empty() {
        format!("({})", spawn_request.reasoning_effort)
    } else {
        format!("({model} {})", spawn_request.reasoning_effort)
    };

    vec![Span::from(" ").dim(), Span::from(details).magenta()]
}

fn prompt_line(prompt: &str) -> Option<Line<'static>> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(Line::from(Span::from(truncate_text(
            trimmed,
            COLLAB_PROMPT_PREVIEW_GRAPHEMES,
        ))))
    }
}

fn merge_wait_receivers(
    receiver_thread_ids: &[ThreadId],
    mut receiver_agents: Vec<CollabAgentRef>,
) -> Vec<CollabAgentRef> {
    if receiver_agents.is_empty() {
        return receiver_thread_ids
            .iter()
            .map(|thread_id| CollabAgentRef {
                thread_id: *thread_id,
                agent_nickname: None,
                agent_role: None,
            })
            .collect();
    }

    let mut seen = receiver_agents
        .iter()
        .map(|agent| agent.thread_id)
        .collect::<HashSet<_>>();
    for thread_id in receiver_thread_ids {
        if seen.insert(*thread_id) {
            receiver_agents.push(CollabAgentRef {
                thread_id: *thread_id,
                agent_nickname: None,
                agent_role: None,
            });
        }
    }
    receiver_agents
}

fn wait_complete_lines(
    statuses: &HashMap<ThreadId, AgentStatus>,
    agent_statuses: &[CollabAgentStatusEntry],
) -> Vec<Line<'static>> {
    if statuses.is_empty() && agent_statuses.is_empty() {
        return vec![Line::from(Span::from("No agents completed yet"))];
    }

    let entries = if agent_statuses.is_empty() {
        let mut entries = statuses
            .iter()
            .map(|(thread_id, status)| CollabAgentStatusEntry {
                thread_id: *thread_id,
                agent_nickname: None,
                agent_role: None,
                status: status.clone(),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.thread_id.to_string().cmp(&right.thread_id.to_string()));
        entries
    } else {
        let mut entries = agent_statuses.to_vec();
        let seen = entries
            .iter()
            .map(|entry| entry.thread_id)
            .collect::<HashSet<_>>();
        let mut extras = statuses
            .iter()
            .filter(|(thread_id, _)| !seen.contains(thread_id))
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
    };

    let mut lines: Vec<Line<'static>> = entries
        .into_iter()
        .map(|entry| {
            let CollabAgentStatusEntry {
                thread_id: _,
                agent_nickname,
                agent_role,
                status,
            } = entry;
            let mut spans = agent_label_spans(AgentLabel {
                nickname: agent_nickname.as_deref(),
                role: agent_role.as_deref(),
            });
            spans.push(Span::from(": ").dim());
            spans.extend(status_summary_spans(&status));
            spans.into()
        })
        .collect();

    let approved = lines
        .iter()
        .all(|line| matches!(line_to_status(line), Some("Pass") | Some("Completed")));
    let approved_count = lines
        .iter()
        .filter(|line| matches!(line_to_status(line), Some("Pass") | Some("Completed")))
        .count();
    let total_count = lines.len();
    let summary = if approved && total_count > 0 {
        format!("Summary: all {total_count} agents approved")
    } else {
        format!("Summary: {approved_count}/{total_count} agents approved")
    };
    lines.push(Line::from(Span::from(summary).bold()));
    lines
}

fn line_to_status(line: &Line<'static>) -> Option<&'static str> {
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    if text.contains(": Pass") {
        Some("Pass")
    } else if text.contains(": Fail") {
        Some("Fail")
    } else if text.contains(": Completed") {
        Some("Completed")
    } else if text.contains(": Error") {
        Some("Error")
    } else {
        None
    }
}

fn status_summary_line(status: &AgentStatus) -> Line<'static> {
    status_summary_spans(status).into()
}

fn status_summary_spans(status: &AgentStatus) -> Vec<Span<'static>> {
    match status {
        AgentStatus::PendingInit => vec![Span::from("Pending init").cyan()],
        AgentStatus::Running => vec![Span::from("Running").cyan().bold()],
        // Allow `.yellow()`
        #[allow(clippy::disallowed_methods)]
        AgentStatus::Interrupted => vec![Span::from("Interrupted").yellow()],
        AgentStatus::Completed(message) => {
            let mut spans = match message_status(message.as_ref()) {
                Some("Pass") => vec![Span::from("Pass").green()],
                Some("Fail") => vec![Span::from("Fail").red()],
                _ => vec![Span::from("Completed").green()],
            };
            if let Some(message) = message.as_ref() {
                let message_preview = truncate_text(
                    &message.split_whitespace().collect::<Vec<_>>().join(" "),
                    COLLAB_AGENT_RESPONSE_PREVIEW_GRAPHEMES,
                );
                if !message_preview.is_empty() {
                    spans.push(Span::from(" - ").dim());
                    spans.push(Span::from(message_preview));
                }
            }
            spans
        }
        AgentStatus::Errored(error) => {
            let mut spans = vec![Span::from("Error").red()];
            let error_preview = truncate_text(
                &error.split_whitespace().collect::<Vec<_>>().join(" "),
                COLLAB_AGENT_ERROR_PREVIEW_GRAPHEMES,
            );
            if !error_preview.is_empty() {
                spans.push(Span::from(" - ").dim());
                spans.push(Span::from(error_preview));
            }
            spans
        }
        AgentStatus::Shutdown => vec![Span::from("Shutdown")],
        AgentStatus::NotFound => vec![Span::from("Not found").red()],
    }
}

fn message_status(message: Option<&String>) -> Option<&'static str> {
    let message = message?;
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("status: fail")
        || normalized.contains("status: failed")
        || normalized.contains("status: error")
    {
        Some("Fail")
    } else if normalized.contains("status: pass")
        || normalized.contains("status: approved")
        || normalized.contains("status: success")
    {
        Some("Pass")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_cell::HistoryCell;
    #[cfg(target_os = "macos")]
    use crossterm::event::KeyEvent;
    #[cfg(target_os = "macos")]
    use crossterm::event::KeyModifiers;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use ratatui::style::Color;
    use ratatui::style::Modifier;

    #[test]
    fn collab_events_snapshot() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");
        let robie_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
            .expect("valid robie thread id");
        let bob_id = ThreadId::from_string("00000000-0000-0000-0000-000000000003")
            .expect("valid bob thread id");

        let spawn = spawn_end(
            CollabAgentSpawnEndEvent {
                call_id: "call-spawn".to_string(),
                sender_thread_id,
                new_thread_id: Some(robie_id),
                new_agent_nickname: Some("Robie".to_string()),
                new_agent_role: Some("explorer".to_string()),
                prompt: "Compute 11! and reply with just the integer result.".to_string(),
                model: "gpt-5".to_string(),
                reasoning_effort: ReasoningEffortConfig::High,
                status: AgentStatus::PendingInit,
            },
            Some(&SpawnRequestSummary {
                model: "gpt-5".to_string(),
                reasoning_effort: ReasoningEffortConfig::High,
            }),
        );

        let route = interaction_begin(CollabAgentInteractionBeginEvent {
            call_id: "call-route".to_string(),
            sender_thread_id,
            receiver_thread_id: robie_id,
            prompt: "Please review the design and start the build.".to_string(),
        });

        let send = interaction_end(CollabAgentInteractionEndEvent {
            call_id: "call-send".to_string(),
            sender_thread_id,
            receiver_thread_id: robie_id,
            receiver_agent_nickname: Some("Robie".to_string()),
            receiver_agent_role: Some("explorer".to_string()),
            prompt: "Please continue and return the answer only.".to_string(),
            status: AgentStatus::Running,
        });

        let waiting = waiting_begin(CollabWaitingBeginEvent {
            sender_thread_id,
            receiver_thread_ids: vec![robie_id],
            receiver_agents: vec![CollabAgentRef {
                thread_id: robie_id,
                agent_nickname: Some("Robie".to_string()),
                agent_role: Some("explorer".to_string()),
            }],
            call_id: "call-wait".to_string(),
        });

        let mut statuses = HashMap::new();
        statuses.insert(
            robie_id,
            AgentStatus::Completed(Some("39916800".to_string())),
        );
        statuses.insert(bob_id, AgentStatus::Errored("tool timeout".to_string()));
        let finished = waiting_end(CollabWaitingEndEvent {
            sender_thread_id,
            call_id: "call-wait".to_string(),
            agent_statuses: vec![
                CollabAgentStatusEntry {
                    thread_id: robie_id,
                    agent_nickname: Some("Robie".to_string()),
                    agent_role: Some("explorer".to_string()),
                    status: AgentStatus::Completed(Some("39916800".to_string())),
                },
                CollabAgentStatusEntry {
                    thread_id: bob_id,
                    agent_nickname: Some("Bob".to_string()),
                    agent_role: Some("worker".to_string()),
                    status: AgentStatus::Errored("tool timeout".to_string()),
                },
            ],
            statuses,
        });

        let close = close_end(CollabCloseEndEvent {
            call_id: "call-close".to_string(),
            sender_thread_id,
            receiver_thread_id: robie_id,
            receiver_agent_nickname: Some("Robie".to_string()),
            receiver_agent_role: Some("explorer".to_string()),
            status: AgentStatus::Completed(Some("39916800".to_string())),
        });

        let snapshot = [spawn, route, send, waiting, finished, close]
            .iter()
            .map(cell_to_text)
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_snapshot!("collab_agent_transcript", snapshot);
    }

    #[test]
    fn collab_spawn_failure_snapshot() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");

        let cell = spawn_end(
            CollabAgentSpawnEndEvent {
                call_id: "call-spawn-failed".to_string(),
                sender_thread_id,
                new_thread_id: None,
                new_agent_nickname: Some("Herschel".to_string()),
                new_agent_role: Some("developer".to_string()),
                prompt: "Implement the approved plan.".to_string(),
                model: "gpt-5.4".to_string(),
                reasoning_effort: ReasoningEffortConfig::Medium,
                status: AgentStatus::Errored("No spawned thread id was returned".to_string()),
                failure_reason: Some("No spawned thread id was returned".to_string()),
            },
            Some(&SpawnRequestSummary {
                model: "gpt-5.4".to_string(),
                reasoning_effort: ReasoningEffortConfig::Medium,
            }),
        );

        assert_snapshot!("collab_spawn_failure", cell_to_text(&cell));
    }

    #[test]
    fn collab_spawn_begin_snapshot() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");
        let cell = spawn_begin(
            CollabAgentSpawnBeginEvent {
                call_id: "call-spawn".to_string(),
                sender_thread_id,
                prompt: "Create a Python web-based mini game in `/mnt/c/POC/test-game-1`."
                    .to_string(),
                model: "gpt-5.5".to_string(),
                reasoning_effort: ReasoningEffortConfig::Medium,
            },
            Some(&SpawnRequestSummary {
                model: "gpt-5.5".to_string(),
                reasoning_effort: ReasoningEffortConfig::Medium,
            }),
        );

        assert_snapshot!("collab_spawn_begin", cell_to_text(&cell));
    }

    #[test]
    fn collab_interaction_begin_snapshot() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");
        let receiver_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
            .expect("valid receiver thread id");

        let cell = interaction_begin(CollabAgentInteractionBeginEvent {
            call_id: "call-route".to_string(),
            sender_thread_id,
            receiver_thread_id,
            prompt: "Please review the design and start the build.".to_string(),
        });

        assert_snapshot!("collab_interaction_begin", cell_to_text(&cell));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn agent_shortcut_matches_option_arrow_word_motion_fallbacks_only_when_allowed() {
        assert!(previous_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
            false,
        ));
        assert!(next_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Right, KeyModifiers::ALT),
            false,
        ));
        assert!(previous_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            true,
        ));
        assert!(next_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
            true,
        ));
        assert!(!previous_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            false,
        ));
        assert!(!next_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
            false,
        ));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn agent_shortcut_matches_option_arrows_only() {
        assert!(previous_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Left, crossterm::event::KeyModifiers::ALT,),
            false
        ));
        assert!(next_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Right, crossterm::event::KeyModifiers::ALT,),
            false
        ));
        assert!(!previous_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Char('b'), crossterm::event::KeyModifiers::ALT,),
            false
        ));
        assert!(!next_agent_shortcut_matches(
            KeyEvent::new(KeyCode::Char('f'), crossterm::event::KeyModifiers::ALT,),
            false
        ));
    }

    #[test]
    fn title_styles_nickname_and_role() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");
        let robie_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
            .expect("valid robie thread id");
        let cell = spawn_end(
            CollabAgentSpawnEndEvent {
                call_id: "call-spawn".to_string(),
                sender_thread_id,
                new_thread_id: Some(robie_id),
                new_agent_nickname: Some("Robie".to_string()),
                new_agent_role: Some("explorer".to_string()),
                prompt: String::new(),
                model: "gpt-5".to_string(),
                reasoning_effort: ReasoningEffortConfig::High,
                status: AgentStatus::PendingInit,
            },
            Some(&SpawnRequestSummary {
                model: "gpt-5".to_string(),
                reasoning_effort: ReasoningEffortConfig::High,
            }),
        );

        let lines = cell.display_lines(200);
        let title = &lines[0];
        assert_eq!(title.spans[2].content.as_ref(), "Robie");
        assert_eq!(title.spans[2].style.fg, Some(Color::Cyan));
        assert!(title.spans[2].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(title.spans[4].content.as_ref(), "[explorer]");
        assert_eq!(title.spans[4].style.fg, None);
        assert!(!title.spans[4].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(title.spans[6].content.as_ref(), "(gpt-5 high)");
        assert_eq!(title.spans[6].style.fg, Some(Color::Magenta));
    }

    #[test]
    fn title_prefers_role_over_thread_id_when_nickname_is_missing() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");
        let robie_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
            .expect("valid robie thread id");
        let cell = spawn_end(
            CollabAgentSpawnEndEvent {
                call_id: "call-spawn".to_string(),
                sender_thread_id,
                new_thread_id: Some(robie_id),
                new_agent_nickname: None,
                new_agent_role: Some("planner".to_string()),
                prompt: String::new(),
                model: "gpt-5".to_string(),
                reasoning_effort: ReasoningEffortConfig::Medium,
                status: AgentStatus::PendingInit,
            },
            None,
        );

        let lines = cell.display_lines(200);
        let title = &lines[0];
        assert_eq!(title.spans[2].content.as_ref(), "planner");
        assert_eq!(title.spans.len(), 3);
    }

    #[test]
    fn failed_spawn_uses_role_label() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");
        let cell = spawn_end(
            CollabAgentSpawnEndEvent {
                call_id: "call-spawn".to_string(),
                sender_thread_id,
                new_thread_id: None,
                new_agent_nickname: None,
                new_agent_role: Some("critic".to_string()),
                prompt: String::new(),
                model: "gpt-5".to_string(),
                reasoning_effort: ReasoningEffortConfig::Medium,
                status: AgentStatus::NotFound,
            },
            None,
        );

        let lines = cell.display_lines(200);
        let title = &lines[0];
        assert_eq!(title.spans[2].content.as_ref(), "critic");
        assert_eq!(title.spans.len(), 3);
    }

    #[test]
    fn collab_resume_interrupted_snapshot() {
        let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
            .expect("valid sender thread id");
        let robie_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
            .expect("valid robie thread id");

        let cell = resume_end(CollabResumeEndEvent {
            call_id: "call-resume".to_string(),
            sender_thread_id,
            receiver_thread_id: robie_id,
            receiver_agent_nickname: Some("Robie".to_string()),
            receiver_agent_role: Some("explorer".to_string()),
            status: AgentStatus::Interrupted,
        });

        assert_snapshot!("collab_resume_interrupted", cell_to_text(&cell));
    }

    fn cell_to_text(cell: &PlainHistoryCell) -> String {
        cell.display_lines(200)
            .iter()
            .map(line_to_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn line_to_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }
}
