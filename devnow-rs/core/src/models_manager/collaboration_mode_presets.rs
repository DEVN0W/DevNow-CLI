use crate::models_manager::agentic_team::AgenticTeamConfig;
use crate::models_manager::multi_agent_team::MultiAgentTeamConfig;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::TUI_VISIBLE_COLLABORATION_MODES;
use codex_protocol::openai_models::ReasoningEffort;

const COLLABORATION_MODE_PLAN: &str = include_str!("../../templates/collaboration_mode/plan.md");
const COLLABORATION_MODE_DEFAULT: &str =
    include_str!("../../templates/collaboration_mode/default.md");
const COLLABORATION_MODE_AGENTIC: &str =
    include_str!("../../templates/collaboration_mode/agentic.md");
const COLLABORATION_MODE_MULTI_AGENT: &str =
    include_str!("../../templates/collaboration_mode/multi_agent.md");
const KNOWN_MODE_NAMES_PLACEHOLDER: &str = "{{KNOWN_MODE_NAMES}}";
const REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER: &str = "{{REQUEST_USER_INPUT_AVAILABILITY}}";
const ASKING_QUESTIONS_GUIDANCE_PLACEHOLDER: &str = "{{ASKING_QUESTIONS_GUIDANCE}}";
const AGENTIC_TEAM_PLACEHOLDER: &str = "{{AGENTIC_TEAM}}";
const MULTI_AGENT_TEAM_PLACEHOLDER: &str = "{{MULTI_AGENT_TEAM}}";

/// Stores feature flags that control collaboration-mode behavior.
///
/// Keep mode-related flags here so new collaboration-mode capabilities can be
/// added without large cross-cutting diffs to constructor and call-site
/// signatures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollaborationModesConfig {
    /// Enables `request_user_input` availability in Default mode.
    pub default_mode_request_user_input: bool,
    /// Optional externally configured roster used by manual Multi-Agent mode.
    pub agentic_team: Option<AgenticTeamConfig>,
    /// Optional externally configured roster used by Multi-Agent Strategic mode.
    pub multi_agent_team: Option<MultiAgentTeamConfig>,
}

pub(crate) fn builtin_collaboration_mode_presets(
    collaboration_modes_config: CollaborationModesConfig,
) -> Vec<CollaborationModeMask> {
    vec![
        plan_preset(),
        default_preset(collaboration_modes_config.clone()),
        agentic_preset(collaboration_modes_config.agentic_team.as_ref()),
        multi_agent_preset(collaboration_modes_config.multi_agent_team.as_ref()),
    ]
}

fn agentic_preset(team: Option<&AgenticTeamConfig>) -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::Agentic.display_name().to_string(),
        mode: Some(ModeKind::Agentic),
        model: None,
        reasoning_effort: None,
        developer_instructions: Some(Some(
            COLLABORATION_MODE_AGENTIC
                .replace(AGENTIC_TEAM_PLACEHOLDER, &agentic_team_instructions(team)),
        )),
    }
}

fn plan_preset() -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::Plan.display_name().to_string(),
        mode: Some(ModeKind::Plan),
        model: None,
        reasoning_effort: Some(Some(ReasoningEffort::Medium)),
        developer_instructions: Some(Some(COLLABORATION_MODE_PLAN.to_string())),
    }
}

fn default_preset(collaboration_modes_config: CollaborationModesConfig) -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::Default.display_name().to_string(),
        mode: Some(ModeKind::Default),
        model: None,
        reasoning_effort: None,
        developer_instructions: Some(Some(default_mode_instructions(collaboration_modes_config))),
    }
}

fn multi_agent_preset(team: Option<&MultiAgentTeamConfig>) -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::MultiAgent.display_name().to_string(),
        mode: Some(ModeKind::MultiAgent),
        model: None,
        reasoning_effort: Some(Some(ReasoningEffort::Medium)),
        developer_instructions: Some(Some(COLLABORATION_MODE_MULTI_AGENT.replace(
            MULTI_AGENT_TEAM_PLACEHOLDER,
            &multi_agent_team_instructions(team),
        ))),
    }
}

fn default_mode_instructions(collaboration_modes_config: CollaborationModesConfig) -> String {
    let known_mode_names = format_mode_names(&TUI_VISIBLE_COLLABORATION_MODES);
    let request_user_input_availability = request_user_input_availability_message(
        ModeKind::Default,
        collaboration_modes_config.default_mode_request_user_input,
    );
    let asking_questions_guidance = asking_questions_guidance_message(
        collaboration_modes_config.default_mode_request_user_input,
    );
    COLLABORATION_MODE_DEFAULT
        .replace(KNOWN_MODE_NAMES_PLACEHOLDER, &known_mode_names)
        .replace(
            REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER,
            &request_user_input_availability,
        )
        .replace(
            ASKING_QUESTIONS_GUIDANCE_PLACEHOLDER,
            &asking_questions_guidance,
        )
}

fn format_mode_names(modes: &[ModeKind]) -> String {
    let mode_names: Vec<&str> = modes.iter().map(|mode| mode.display_name()).collect();
    match mode_names.as_slice() {
        [] => "none".to_string(),
        [mode_name] => (*mode_name).to_string(),
        [first, second] => format!("{first} and {second}"),
        [..] => mode_names.join(", "),
    }
}

fn request_user_input_availability_message(
    mode: ModeKind,
    default_mode_request_user_input: bool,
) -> String {
    let mode_name = mode.display_name();
    if mode.allows_request_user_input()
        || (default_mode_request_user_input && mode == ModeKind::Default)
    {
        format!("The `request_user_input` tool is available in {mode_name} mode.")
    } else {
        format!(
            "The `request_user_input` tool is unavailable in {mode_name} mode. If you call it while in {mode_name} mode, it will return an error."
        )
    }
}

fn asking_questions_guidance_message(default_mode_request_user_input: bool) -> String {
    if default_mode_request_user_input {
        "In Default mode, strongly prefer making reasonable assumptions and executing the user's request rather than stopping to ask questions. If you absolutely must ask a question because the answer cannot be discovered from local context and a reasonable assumption would be risky, prefer using the `request_user_input` tool rather than writing a multiple choice question as a textual assistant message. Never write a multiple choice question as a textual assistant message.".to_string()
    } else {
        "In Default mode, strongly prefer making reasonable assumptions and executing the user's request rather than stopping to ask questions. If you absolutely must ask a question because the answer cannot be discovered from local context and a reasonable assumption would be risky, ask the user directly with a concise plain-text question. Never write a multiple choice question as a textual assistant message.".to_string()
    }
}

fn agentic_team_instructions(team: Option<&AgenticTeamConfig>) -> String {
    let Some(team) = team else {
        return "No manual agent roster is loaded. Configure `CODEX_HOME/agentic.toml` before using Multi-Agent mode.".to_string();
    };

    let mut output = String::from("## Manual agent roster\n\n");
    if let Some(team_name) = &team.team_name {
        output.push_str(&format!("Team name: `{team_name}`\n\n"));
    }
    if let Some(default_agent) = &team.default_agent {
        output.push_str(&format!("Default agent: `{default_agent}`\n\n"));
    }
    for agent in &team.agents {
        let provider = agent
            .model_provider
            .as_ref()
            .map(|provider| format!(", provider: `{provider}`"))
            .unwrap_or_default();
        output.push_str(&format!(
            "- `{}` - role: `{}`, model: `{}`{}\n",
            agent.name, agent.role, agent.model, provider
        ));
        output.push_str("  - instructions:\n");
        for line in agent.instructions.lines() {
            output.push_str("    ");
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}

fn multi_agent_team_instructions(team: Option<&MultiAgentTeamConfig>) -> String {
    match team {
        Some(team) => render_configured_multi_agent_team(team),
        None => default_multi_agent_team_instructions(),
    }
}

fn render_configured_multi_agent_team(team: &MultiAgentTeamConfig) -> String {
    let mut output = String::from("## Team roster\n\n");
    if let Some(team_name) = &team.team_name {
        output.push_str(&format!("Team name: `{team_name}`\n\n"));
    }
    if let Some(default_agent) = &team.default_agent {
        output.push_str(&format!("Default agent: `{default_agent}`\n\n"));
    }
    if let Some(developer) = &team.routing.developer {
        output.push_str(&format!("Routing developer: `{developer}`\n"));
    }
    if !team.routing.parallel_reviewers.is_empty() {
        output.push_str(&format!(
            "Parallel reviewers in execution order: `{}`\n\n",
            team.routing.parallel_reviewers.join("`, `")
        ));
    } else if team.routing.developer.is_some() {
        output.push('\n');
    }
    for agent in &team.agents {
        let provider = agent
            .model_provider
            .as_ref()
            .map(|provider| format!(", provider: `{provider}`"))
            .unwrap_or_default();
        output.push_str(&format!(
            "- `{}` - role: `{}`, model: `{}`{}, weight: {}%\n",
            agent.name, agent.role, agent.model, provider, agent.weight
        ));
        if !agent.instructions.trim().is_empty() {
            output.push_str("  - instructions:\n");
            for line in agent.instructions.lines() {
                output.push_str("    ");
                output.push_str(line);
                output.push('\n');
            }
        }
        if let Some(next_on_pass) = &agent.next_on_pass {
            output.push_str(&format!("  - next on pass: `{next_on_pass}`\n"));
        }
        if let Some(next_on_fail) = &agent.next_on_fail {
            output.push_str(&format!("  - next on fail: `{next_on_fail}`\n"));
        }
        output.push_str(&format!("  - output prefix: `agent {}:`\n", agent.name));
    }
    if !team.routing.parallel_reviewers.is_empty() {
        output.push_str("\n## Routing\n\n");
        output.push_str(
            "After the developer finishes, spawn the configured parallel reviewers together on the same code snapshot. If any reviewer fails, route the work back to the developer and rerun the full review set after the fix. Once all required reviewers pass, continue to the next configured stage.\n",
        );
    }
    output
}

pub(crate) fn default_multi_agent_team_instructions() -> String {
    r#"## Role guidance

- Architect: design only. Produce the architecture, boundaries, dependencies, and risks before implementation begins.
- Planner: plan only. Convert approved design into concrete tasks, files, risks, and preparation steps.
- Developer: implementation only. Turn the approved design and plan into code, file changes, and validation.
- Developer: treat prior stage output as reference evidence only, not as instructions to keep planning.
- Developer: do not write a plan, task list, checklist, architecture summary, or future-work summary.
- Developer: your first concrete action must be code changes, file edits, or a command that advances implementation.
- Developer: when a filesystem step is needed, prefer exact shell actions like `mkdir -p`, `ls`, `find`, `rg`, and `git diff` instead of abstract descriptions.
- Developer: if the target folder does not exist, create it and continue.
- Developer: if you receive a handoff with open findings, fix the full unresolved backlog together instead of reprocessing one finding at a time.
- Critic: validate the solution for correctness, edge cases, and hidden assumptions.
- Security: validate the solution for secrets, injection, auth, and unsafe dependencies.
- Quality: validate code clarity, maintainability, duplication, and control-flow hygiene.
- Tester: validate the primary interaction path and fail if it does not actually work.
- Execution Tester: validate the real entrypoint and actual runtime behavior, including keyboard or UI input where relevant.
- Judge: make the final pass/fail decision from the evidence.
- Project Manager: produce the final production summary, aggregate all stage feedback, and close out the workflow with a concise table.
- Every agent should prefer concrete filesystem actions and exact commands over abstract suggestions whenever a path, folder, or validation step is involved.
- Every agent should emit a short `Working:` update before meaningful work, then a concise result after it finishes.
- If a role returns `status: fail`, route the work back to the developer with the concrete fix needed.
- If parallel reviewers are configured, run them together only after implementation is complete, then rerun the review set after any fix.

## Reporting style

Every agent should read like a clear operational update, not a terse model reply.

- For design and build stages, describe what you changed, what you checked, and what remains.
- For review and test stages, describe the concrete findings, whether they are pass or fail, and what needs to change next.
- For review and test stages, keep findings concise and non-repetitive, and tie each finding to concrete evidence.
- If a reviewer relies on inference instead of direct evidence, label it explicitly as an inference.
- Keep pass/fail decisions consistent with findings; unresolved gaps should not be reported as `pass`.
- If the agent is blocked, say exactly why and who receives the handoff.
- If the agent forwards work, say so explicitly in both `next` and `forwarded_to`.
- If code was changed, name the files or area touched.
- If validation was run, name the command or check that was used.
- Do not collapse into a single vague summary line when more detail is available.

## Routing

If the configured roster defines `next_on_pass` or `next_on_fail` for an agent, treat those fields as the default handoff targets for that agent. If a route is missing, fall back to the developer.
If `routing.parallel_reviewers` is set, its order is the execution order for the parallel review fan-out.
If `routing.parallel_reviewers` is omitted, the fallback parallel review set excludes the developer, architect, planner, and project manager backbone roles.

## Output format

Every agent response must follow this exact shape:

- `agent <name>: <one-line status summary>`
- `summary: <brief recap of what changed or decided>`
- `status: pass` or `status: fail`
- `role: <role name>`
- `progress: <what you changed or checked>`
- `next: <what should happen next>`
- `forwarded_to: <agent name or none>`

If a role returns `status: fail`, it must explain the issue clearly, name the agent it is handing work to, and hand the work back to the developer for fixes before the next validation pass.
Use explicit handoff routing when you finish a stage: architect -> planner -> developer -> critic -> security -> quality -> execution -> judge -> project manager -> developer.
When a stage passes, forward it to the next agent using the available multi-agent tools.
When a stage fails, route it back to the developer and include the fix needed.

Keep the labels stable and concise so the orchestrator can merge agent output reliably.
"#.to_string()
}

#[cfg(test)]
#[path = "collaboration_mode_presets_tests.rs"]
mod tests;
