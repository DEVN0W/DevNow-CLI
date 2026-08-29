use super::*;
use pretty_assertions::assert_eq;

#[test]
fn preset_names_use_mode_display_names() {
    assert_eq!(plan_preset().name, ModeKind::Plan.display_name());
    assert_eq!(
        default_preset(CollaborationModesConfig::default()).name,
        ModeKind::Default.display_name()
    );
    assert_eq!(
        multi_agent_preset(None).name,
        ModeKind::MultiAgent.display_name()
    );
    assert_eq!(
        plan_preset().reasoning_effort,
        Some(Some(ReasoningEffort::Medium))
    );
}

#[test]
fn default_mode_instructions_replace_mode_names_placeholder() {
    let default_instructions = default_preset(CollaborationModesConfig {
        default_mode_request_user_input: true,
        multi_agent_team: None,
    })
    .developer_instructions
    .expect("default preset should include instructions")
    .expect("default instructions should be set");

    assert!(!default_instructions.contains(KNOWN_MODE_NAMES_PLACEHOLDER));
    assert!(!default_instructions.contains(REQUEST_USER_INPUT_AVAILABILITY_PLACEHOLDER));
    assert!(!default_instructions.contains(ASKING_QUESTIONS_GUIDANCE_PLACEHOLDER));

    let known_mode_names = format_mode_names(&TUI_VISIBLE_COLLABORATION_MODES);
    let expected_snippet = format!("Known mode names are {known_mode_names}.");
    assert!(default_instructions.contains(&expected_snippet));

    let expected_availability_message =
        request_user_input_availability_message(ModeKind::Default, true);
    assert!(default_instructions.contains(&expected_availability_message));
    assert!(default_instructions.contains("prefer using the `request_user_input` tool"));
}

#[test]
fn default_mode_instructions_use_plain_text_questions_when_feature_disabled() {
    let default_instructions = default_preset(CollaborationModesConfig::default())
        .developer_instructions
        .expect("default preset should include instructions")
        .expect("default instructions should be set");

    assert!(!default_instructions.contains("prefer using the `request_user_input` tool"));
    assert!(
        default_instructions.contains("ask the user directly with a concise plain-text question")
    );
}

#[test]
fn multi_agent_preset_uses_multi_agent_instructions() {
    let multi_agent_instructions = multi_agent_preset(None)
        .developer_instructions
        .expect("multi-agent preset should include instructions")
        .expect("multi-agent instructions should be set");

    assert!(multi_agent_instructions.contains("Multi Agent mode"));
    assert!(multi_agent_instructions.contains("Role guidance"));
    assert!(multi_agent_instructions.contains("many agents"));
    assert!(multi_agent_instructions.contains("architect -> planner"));
    assert!(multi_agent_instructions.contains("agent <name>: <one-line status summary>"));
    assert!(multi_agent_instructions.contains("status: pass"));
    assert!(multi_agent_instructions.contains("Reporting style"));
    assert!(
        multi_agent_instructions
            .contains("Do not collapse useful context into a single terse line.")
    );
    assert!(
        multi_agent_instructions
            .contains("visible progress update before, during, and after meaningful work")
    );
    assert!(multi_agent_instructions.contains("preparation steps"));
    assert!(multi_agent_instructions.contains("pass/fail result"));
    assert!(multi_agent_instructions.contains("Execution Tester"));
    assert!(multi_agent_instructions.contains("Quality"));
    assert!(multi_agent_instructions.contains("Security"));
    assert!(
        multi_agent_instructions
            .contains("Planner: turn the architect output into a concrete task breakdown")
    );
    assert!(multi_agent_instructions.contains("Planner: begin with a short `Working:` update"));
    assert!(multi_agent_instructions.contains("Architect: begin with a short `Working:` update"));
    assert!(multi_agent_instructions.contains("Developer: begin with a short `Working:` update"));
}

#[test]
fn multi_agent_preset_renders_configured_team() {
    let team = MultiAgentTeamConfig {
        team_name: Some("Builder Team".to_string()),
        default_agent: Some("architect".to_string()),
        routing: MultiAgentRoutingConfig {
            developer: Some("developer".to_string()),
            parallel_reviewers: vec![
                "critic".to_string(),
                "security".to_string(),
                "quality".to_string(),
            ],
        },
        agents: vec![
            MultiAgentAgentConfig {
                name: "architect".to_string(),
                role: "architect".to_string(),
                model: "gpt-5.5".to_string(),
                model_provider: Some("openai".to_string()),
                weight: 15,
                passing_grade: None,
                next_on_pass: Some("planner".to_string()),
                next_on_fail: Some("planner".to_string()),
                instructions: "Create the design.".to_string(),
            },
            MultiAgentAgentConfig {
                name: "planner".to_string(),
                role: "planner".to_string(),
                model: "gpt-5.5".to_string(),
                model_provider: Some("openai".to_string()),
                weight: 20,
                passing_grade: None,
                next_on_pass: Some("developer".to_string()),
                next_on_fail: Some("developer".to_string()),
                instructions: "Break the work into tasks.".to_string(),
            },
            MultiAgentAgentConfig {
                name: "developer".to_string(),
                role: "developer".to_string(),
                model: "gpt-5".to_string(),
                model_provider: Some("openai".to_string()),
                weight: 60,
                passing_grade: None,
                next_on_pass: Some("critic".to_string()),
                next_on_fail: Some("developer".to_string()),
                instructions: "Own the implementation.".to_string(),
            },
            MultiAgentAgentConfig {
                name: "critic".to_string(),
                role: "critic".to_string(),
                model: "gpt-4.1".to_string(),
                model_provider: Some("claude".to_string()),
                weight: 20,
                passing_grade: None,
                next_on_pass: Some("judge".to_string()),
                next_on_fail: Some("developer".to_string()),
                instructions: "Challenge the plan.".to_string(),
            },
        ],
    };

    let multi_agent_instructions = multi_agent_preset(Some(&team))
        .developer_instructions
        .expect("multi-agent preset should include instructions")
        .expect("multi-agent instructions should be set");

    assert!(multi_agent_instructions.contains("Builder Team"));
    assert!(multi_agent_instructions.contains("Default agent: `architect`"));
    assert!(multi_agent_instructions.contains("role: architect"));
    assert!(multi_agent_instructions.contains("role: planner"));
    assert!(multi_agent_instructions.contains("weight: 60%"));
    assert!(multi_agent_instructions.contains("provider: `openai`"));
    assert!(multi_agent_instructions.contains("next on pass: `critic`"));
    assert!(multi_agent_instructions.contains("next on fail: `developer`"));
    assert!(multi_agent_instructions.contains("output prefix: `agent developer:`"));
    assert!(multi_agent_instructions.contains("instructions:"));
    assert!(multi_agent_instructions.contains("Own the implementation."));
    assert!(multi_agent_instructions.contains("Create the design."));
    assert!(multi_agent_instructions.contains("gpt-4.1"));
    assert!(multi_agent_instructions.contains("role: developer"));
    assert!(multi_agent_instructions.contains("progress: <what you changed or checked>"));
    assert!(multi_agent_instructions.contains("Routing developer: `developer`"));
    assert!(
        multi_agent_instructions.contains("Parallel reviewers: `critic`, `security`, `quality`")
    );
    assert!(multi_agent_instructions.contains(
        "fan out the configured review, security, quality, and execution agents together"
    ));
}
