use codex_core::models_manager::collaboration_mode_presets::CollaborationModesConfig;
use codex_core::models_manager::multi_agent_team::MultiAgentTeamConfig;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::TUI_VISIBLE_COLLABORATION_MODES;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;
use std::convert::Infallible;

const COLLABORATION_MODE_PLAN: &str =
    include_str!("../../core/templates/collaboration_mode/plan.md");
const COLLABORATION_MODE_DEFAULT: &str =
    include_str!("../../core/templates/collaboration_mode/default.md");
const COLLABORATION_MODE_MULTI_AGENT: &str =
    include_str!("../../core/templates/collaboration_mode/multi_agent.md");
const KNOWN_MODE_NAMES_PLACEHOLDER: &str = "{{KNOWN_MODE_NAMES}}";
const REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER: &str = "{{REQUEST_USER_INPUT_AVAILABILITY}}";
const ASKING_QUESTIONS_GUIDANCE_PLACEHOLDER: &str = "{{ASKING_QUESTIONS_GUIDANCE}}";
const MULTI_AGENT_TEAM_PLACEHOLDER: &str = "{{MULTI_AGENT_TEAM}}";

#[derive(Debug, Clone)]
pub(crate) struct ModelCatalog {
    models: Vec<ModelPreset>,
    collaboration_modes_config: CollaborationModesConfig,
}

impl ModelCatalog {
    pub(crate) fn new(
        models: Vec<ModelPreset>,
        collaboration_modes_config: CollaborationModesConfig,
    ) -> Self {
        Self {
            models,
            collaboration_modes_config,
        }
    }

    pub(crate) fn try_list_models(&self) -> Result<Vec<ModelPreset>, Infallible> {
        Ok(self.models.clone())
    }

    pub(crate) fn list_collaboration_modes(&self) -> Vec<CollaborationModeMask> {
        builtin_collaboration_mode_presets(self.collaboration_modes_config.clone())
    }

    pub(crate) fn multi_agent_team(&self) -> Option<&MultiAgentTeamConfig> {
        self.collaboration_modes_config.multi_agent_team.as_ref()
    }
}

fn builtin_collaboration_mode_presets(
    collaboration_modes_config: CollaborationModesConfig,
) -> Vec<CollaborationModeMask> {
    vec![
        plan_preset(),
        default_preset(collaboration_modes_config.clone()),
        multi_agent_preset(collaboration_modes_config.multi_agent_team.as_ref()),
    ]
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
            "Parallel reviewers: `{}`\n\n",
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
        output.push_str(&format!("  - output prefix: `agent {}:`\n", agent.name));
        output.push_str("  - instructions (reference only; role contract still applies):\n");
        for instruction_line in agent.instructions.lines() {
            output.push_str(&format!("    {instruction_line}\n"));
        }
    }
    if !team.routing.parallel_reviewers.is_empty() {
        output.push_str("\n## Routing\n\n");
        output.push_str(
            "After the developer finishes, spawn the configured parallel reviewers together on the same code snapshot. If any reviewer fails, route the work back to the developer and rerun the full review set after the fix. Once all required reviewers pass, continue to the next configured stage.\n",
        );
    }
    output
}

fn default_multi_agent_team_instructions() -> String {
    r#"## Role guidance

- Architect: create the high-level design, system boundaries, dependencies, and risks before anything is implemented.
- Architect: begin with a short `Working:` update, then provide design only. Do not write the implementation, run tests, or install dependencies.
- Planner: turn the architect output into a concrete task breakdown, target files, risks, and preparation steps only.
- Planner: begin with a short `Working:` update, then provide planning only. Do not write implementation code, run tests, inspect runtime behavior, install dependencies, scaffold files, or try to finish the build yourself.
- Planner: if a needed dependency such as Python, Node, Git, or a package manager is missing, record it as a blocker and hand off rather than trying to implement around it.
- Developer: own the implementation and turn the approved handoff into code.
- Developer: begin with a short `Working:` update, then make the actual code changes and bug fixes only.
- Developer: do not replan the work, restate the architecture, or turn the stage into another planning pass.
- Developer: do not produce task breakdowns, checklists, architecture recaps, or future-work summaries.
- Developer: your first concrete action must be code changes, file edits, or a command that advances implementation.
- Developer: if the roster config defines parallel reviewers, finish the implementation first, then let the orchestrator fan out the review, security, quality, and execution agents in parallel.
- Critic: challenge the solution, looking for weak logic, edge cases, and hidden assumptions.
- Critic: begin with a short `Working:` update, then do not rewrite the whole solution; report concrete defects and hand them back to the developer.
- Judge: act as the final technical approver and require agreement between design, implementation, tests, and review.
- Tester: begin with a short `Working:` update, then run the strongest available validation, not just a shallow smoke test.
- Tester: if the app has keyboard, mouse, terminal, or form-driven controls, verify the primary interaction path actually works and fail if input is ignored or broken.
- Execution Tester: begin with a short `Working:` update, then run the actual entrypoint when possible; if the target is Python, execute `python3 <file>` in addition to tests.
- Execution Tester: if the app is interactive, simulate the primary keyboard or UI action path and fail if controls do not respond.
- Quality: begin with a short `Working:` update, then review code quality, architecture clarity, maintainability, spaghetti code, duplicated logic, and control-flow clarity.
- Quality: require input handling, interactive flows, and UI affordances to be explicit and testable instead of implicit or magical.
- Security: look for secrets, auth flaws, injection risks, unsafe dependencies, and missing validation.
- Security: begin with a short `Working:` update, then do not implement features; only report security findings and required fixes.
- Documentation: begin with a short `Working:` update, then update docs, comments, and usage notes.
- Every agent should emit a visible progress update before, during, and after meaningful work so the CLI shows what is happening.
- That progress update must stay role-appropriate: architect shows design work, planner shows task breakdown and preparation only, developer shows code changes, and review/test agents show checks and results.
- When the developer is done and parallel reviewers are configured, run those reviewers concurrently on the same code snapshot. If one reviewer fails, queue the developer for a fix and rerun the whole review set on the updated code before the final judge.

## Reporting style

Every agent should read like a clear operational update, not a terse model reply.

- For design and build stages, describe what you changed, what you checked, and what remains.
- For review and test stages, describe the concrete findings, whether they are pass or fail, and what needs to change next.
- If the agent is blocked, say exactly why and who receives the handoff.
- If the agent forwards work, say so explicitly in both `next` and `forwarded_to`.
- If code was changed, name the files or area touched.
- If validation was run, name the command or check that was used.
- Do not collapse into a single vague summary line when more detail is available.

## Routing

If the configured roster defines `next_on_pass` or `next_on_fail` for that agent, treat those fields as the default handoff targets for that agent. If a route is missing, fall back to the developer.

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
Use explicit handoff routing when you finish a stage: architect -> planner -> developer -> critic -> security -> quality -> execution -> judge -> developer.
When a stage passes, forward it to the next agent using the available multi-agent tools.
When a stage fails, route it back to the developer and include the fix needed.

Keep the labels stable and concise so the orchestrator can merge agent output reliably.
"#.to_string()
}
