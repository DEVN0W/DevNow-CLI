use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Configuration loaded from `CODEX_HOME/multi_agent.toml`.
pub struct MultiAgentTeamConfig {
    pub team_name: Option<String>,
    pub default_agent: Option<String>,
    pub routing: MultiAgentRoutingConfig,
    pub agents: Vec<MultiAgentAgentConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MultiAgentRoutingConfig {
    pub developer: Option<String>,
    pub parallel_reviewers: Vec<String>,
    pub pass_threshold_percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiAgentAgentConfig {
    pub name: String,
    pub role: String,
    pub model: String,
    pub model_provider: Option<String>,
    pub weight: u8,
    pub passing_grade: Option<u8>,
    pub next_on_pass: Option<String>,
    pub next_on_fail: Option<String>,
    pub instructions: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MultiAgentTeamToml {
    team_name: Option<String>,
    default_agent: Option<String>,
    routing: Option<MultiAgentRoutingToml>,
    agents: Vec<MultiAgentAgentToml>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct MultiAgentRoutingToml {
    developer: Option<String>,
    #[serde(default)]
    parallel_reviewers: Vec<String>,
    pass_threshold_percent: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MultiAgentAgentToml {
    name: String,
    role: String,
    model: String,
    model_provider: Option<String>,
    #[serde(alias = "routing")]
    weight: Option<u8>,
    passing_grade: Option<u8>,
    next_on_pass: Option<String>,
    next_on_fail: Option<String>,
    instructions: String,
}

pub(crate) fn load_multi_agent_team(
    codex_home: &Path,
) -> std::io::Result<Option<MultiAgentTeamConfig>> {
    let path = codex_home.join("multi_agent.toml");
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path)?;
    let parsed: MultiAgentTeamToml = toml::from_str(&contents).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "failed to parse multi-agent team file at {}: {err}",
                path.display()
            ),
        )
    })?;

    validate_multi_agent_team(parsed, &path).map(Some)
}

fn validate_multi_agent_team(
    parsed: MultiAgentTeamToml,
    path: &Path,
) -> std::io::Result<MultiAgentTeamConfig> {
    let team_name = parsed
        .team_name
        .map(|team_name| normalize_required_label("team_name", team_name))
        .transpose()?;
    let default_agent = parsed
        .default_agent
        .map(|default_agent| normalize_required_label("default_agent", default_agent))
        .transpose()?;
    let routing = parsed.routing.unwrap_or_default();
    let routing_developer = routing
        .developer
        .map(|developer| normalize_required_label("routing.developer", developer))
        .transpose()?;
    if parsed.agents.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "multi-agent team file at {} must define at least one agent",
                path.display()
            ),
        ));
    }
    if parsed.agents.len() > 100 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "multi-agent team file at {} cannot define more than 100 agents",
                path.display()
            ),
        ));
    }

    let mut seen_names = BTreeSet::new();
    let mut agents = Vec::with_capacity(parsed.agents.len());
    for agent in &parsed.agents {
        let name = normalize_required_label("agents.name", agent.name.clone())?;
        if !seen_names.insert(name.clone()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "multi-agent team file at {} contains duplicate agent name `{name}`",
                    path.display()
                ),
            ));
        }
    }

    for agent in parsed.agents {
        let name = normalize_required_label("agents.name", agent.name)?;
        let role = normalize_required_label("agents.role", agent.role)?;
        let model = normalize_required_label("agents.model", agent.model)?;
        let model_provider =
            normalize_optional_string("agents.model_provider", agent.model_provider)?;
        let instructions = normalize_required_label("agents.instructions", agent.instructions)?;
        let weight = agent.weight.unwrap_or(0);
        let passing_grade = agent.passing_grade;
        if passing_grade.is_some_and(|passing_grade| passing_grade > 100) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "multi-agent team file at {} has an invalid passing grade for `{name}`; passing_grade must be between 0 and 100",
                    path.display()
                ),
            ));
        }
        if weight > 100 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "multi-agent team file at {} has an invalid routing weight for `{name}`; routing must be between 0 and 100",
                    path.display()
                ),
            ));
        }
        agents.push(MultiAgentAgentConfig {
            name,
            role,
            model,
            model_provider,
            weight,
            passing_grade,
            next_on_pass: agent.next_on_pass,
            next_on_fail: agent.next_on_fail,
            instructions,
        });
    }

    let agents_snapshot = agents.clone();
    for agent in &mut agents {
        agent.next_on_pass = normalize_optional_agent_label(
            &agents_snapshot,
            path,
            "agents.next_on_pass",
            agent.next_on_pass.take(),
        )?;
        agent.next_on_fail = normalize_optional_agent_label(
            &agents_snapshot,
            path,
            "agents.next_on_fail",
            agent.next_on_fail.take(),
        )?;
    }

    let routing_developer = routing_developer
        .map(|routing_developer| {
            resolve_agent_label(&agents, path, "routing.developer", &routing_developer)
        })
        .transpose()?;
    let parallel_reviewers = normalize_agent_label_list(
        &agents,
        path,
        "routing.parallel_reviewers",
        routing.parallel_reviewers,
    )?;
    let pass_threshold_percent = routing.pass_threshold_percent;
    if let Some(pass_threshold_percent) = pass_threshold_percent
        && pass_threshold_percent > 100
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "multi-agent team file at {} has an invalid `routing.pass_threshold_percent`; the value must be between 0 and 100",
                path.display()
            ),
        ));
    }
    if let Some(routing_developer) = &routing_developer
        && parallel_reviewers
            .iter()
            .any(|reviewer| reviewer.eq_ignore_ascii_case(routing_developer))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "multi-agent team file at {} cannot include `{routing_developer}` in both `routing.developer` and `routing.parallel_reviewers`",
                path.display()
            ),
        ));
    }

    if let Some(default_agent) = &default_agent
        && !agents.iter().any(|agent| {
            agent.name.eq_ignore_ascii_case(default_agent)
                || agent.role.eq_ignore_ascii_case(default_agent)
        })
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "multi-agent team file at {} references unknown default_agent `{default_agent}`",
                path.display()
            ),
        ));
    }

    Ok(MultiAgentTeamConfig {
        team_name,
        default_agent,
        routing: MultiAgentRoutingConfig {
            developer: routing_developer,
            parallel_reviewers,
            pass_threshold_percent,
        },
        agents,
    })
}

impl MultiAgentTeamConfig {
    pub fn agent_for_role(&self, role_name: &str) -> Option<&MultiAgentAgentConfig> {
        self.agents
            .iter()
            .find(|agent| {
                agent.name.eq_ignore_ascii_case(role_name)
                    || agent.role.eq_ignore_ascii_case(role_name)
            })
            .or_else(|| {
                let role_aliases = role_name_aliases(role_name);
                self.agents.iter().find(|agent| {
                    role_aliases.iter().any(|alias| {
                        agent.name.eq_ignore_ascii_case(alias)
                            || agent.role.eq_ignore_ascii_case(alias)
                    })
                })
            })
    }

    pub fn agent_for_request(&self, role_name: Option<&str>) -> Option<&MultiAgentAgentConfig> {
        match role_name {
            Some(role_name) => self.agent_for_role(role_name),
            None => self
                .default_agent
                .as_deref()
                .and_then(|default_agent| self.agent_for_role(default_agent)),
        }
    }

    pub fn developer_agent(&self) -> Option<&MultiAgentAgentConfig> {
        self.routing
            .developer
            .as_deref()
            .and_then(|developer| self.agent_for_role(developer))
            .or_else(|| self.agent_for_role("developer"))
            .or_else(|| self.agent_for_role("coder"))
            .or_else(|| self.agent_for_role("worker"))
    }

    pub fn passing_grade_for(&self, agent: &MultiAgentAgentConfig) -> u8 {
        agent.passing_grade.unwrap_or(100).min(100)
    }

    pub fn pass_threshold_percent(&self) -> u8 {
        self.routing.pass_threshold_percent.unwrap_or(100).min(100)
    }

    pub fn uses_numeric_routing(&self) -> bool {
        if !self.routing.parallel_reviewers.is_empty() {
            return false;
        }

        let mut seen = BTreeSet::new();
        self.agents
            .iter()
            .any(|agent| agent.weight == 0 || agent.weight <= 10 || !seen.insert(agent.weight))
    }

    pub fn parallel_review_agents(&self) -> Vec<&MultiAgentAgentConfig> {
        let explicit_reviewers: Vec<&MultiAgentAgentConfig> = self
            .routing
            .parallel_reviewers
            .iter()
            .filter_map(|reviewer| self.agent_for_role(reviewer))
            .collect();

        if !explicit_reviewers.is_empty() {
            return explicit_reviewers;
        }

        if self.uses_numeric_routing() {
            let numeric_reviewers = self.numeric_parallel_review_agents();
            if !numeric_reviewers.is_empty() {
                return numeric_reviewers;
            }
        }

        let Some(developer) = self.developer_agent() else {
            return Vec::new();
        };

        self.agents
            .iter()
            .filter(|agent| {
                !agent.name.eq_ignore_ascii_case(&developer.name)
                    && !agent.role.eq_ignore_ascii_case(&developer.role)
                    && !is_parallel_review_fallback_excluded(agent)
            })
            .collect()
    }

    fn numeric_parallel_review_agents(&self) -> Vec<&MultiAgentAgentConfig> {
        let Some(developer) = self.developer_agent() else {
            return Vec::new();
        };
        let developer_route = developer.weight;
        if developer_route == 0 {
            return Vec::new();
        }
        let Some(next_route) = self
            .agents
            .iter()
            .filter(|agent| agent.weight > developer_route)
            .map(|agent| agent.weight)
            .min()
        else {
            return Vec::new();
        };

        self.agents
            .iter()
            .filter(|agent| {
                agent.weight == next_route
                    && agent.weight > 0
                    && !is_parallel_review_fallback_excluded(agent)
            })
            .collect()
    }
}

fn is_parallel_review_fallback_excluded(agent: &MultiAgentAgentConfig) -> bool {
    let name = agent.name.trim().to_ascii_lowercase();
    let role = agent.role.trim().to_ascii_lowercase();
    matches!(name.as_str(), "architect" | "planner" | "project manager")
        || matches!(role.as_str(), "architect" | "planner" | "project manager")
}

fn role_name_aliases(role_name: &str) -> &'static [&'static str] {
    match role_name.to_ascii_lowercase().as_str() {
        "developer" | "coder" | "worker" => &["developer", "coder", "worker"],
        "architect" | "lead" | "tech lead" => &["architect", "lead", "tech lead"],
        "tester" | "execution tester" => &["tester", "execution tester"],
        "quality ai" | "quality" => &["quality ai", "quality"],
        "security ai" | "security" => &["security ai", "security"],
        _ => &[],
    }
}

fn resolve_agent_label(
    agents: &[MultiAgentAgentConfig],
    path: &Path,
    field_name: &str,
    label: &str,
) -> std::io::Result<String> {
    agents
        .iter()
        .find(|agent| {
            agent.name.eq_ignore_ascii_case(label) || agent.role.eq_ignore_ascii_case(label)
        })
        .map(|agent| agent.name.clone())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "multi-agent team file at {} references unknown agent `{label}` from `{field_name}`",
                    path.display()
                ),
            )
        })
}

fn normalize_agent_label_list(
    agents: &[MultiAgentAgentConfig],
    path: &Path,
    field_name: &str,
    labels: Vec<String>,
) -> std::io::Result<Vec<String>> {
    let mut resolved = Vec::with_capacity(labels.len());
    let mut seen = BTreeSet::new();
    for label in labels {
        let normalized = normalize_required_label(field_name, label)?;
        let canonical = resolve_agent_label(agents, path, field_name, &normalized)?;
        if !seen.insert(canonical.to_lowercase()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "multi-agent team file at {} contains duplicate routing target `{canonical}` in `{field_name}`",
                    path.display()
                ),
            ));
        }
        resolved.push(canonical);
    }
    Ok(resolved)
}

fn normalize_required_label(field_name: &str, value: String) -> std::io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("multi-agent team field `{field_name}` cannot be blank"),
        ))
    } else {
        Ok(trimmed.to_string())
    }
}

#[cfg(test)]
mod role_resolution_tests {
    use super::*;

    #[test]
    fn agent_for_role_accepts_developer_aliases() {
        let team = MultiAgentTeamConfig {
            team_name: Some("Builder Team".to_string()),
            default_agent: Some("architect".to_string()),
            routing: MultiAgentRoutingConfig::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "architect".to_string(),
                    role: "architect".to_string(),
                    model: "gpt-5.4".to_string(),
                    model_provider: Some("azure-openai".to_string()),
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "design".to_string(),
                },
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.3-codex".to_string(),
                    model_provider: Some("azure-openai".to_string()),
                    weight: 40,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "build".to_string(),
                },
            ],
        };

        let developer = team
            .agent_for_role("worker")
            .expect("worker alias should resolve");
        assert_eq!(developer.name, "developer");
        assert_eq!(
            team.agent_for_request(None).expect("default agent").name,
            "architect"
        );
        assert_eq!(
            team.agent_for_role("coder")
                .expect("coder alias should resolve")
                .name,
            "developer"
        );
    }
}

fn normalize_optional_string(
    field_name: &str,
    value: Option<String>,
) -> std::io::Result<Option<String>> {
    value
        .map(|value| normalize_required_label(field_name, value))
        .transpose()
}

fn normalize_optional_agent_label(
    agents: &[MultiAgentAgentConfig],
    path: &Path,
    field_name: &str,
    value: Option<String>,
) -> std::io::Result<Option<String>> {
    match value {
        Some(value) => {
            let normalized = normalize_required_label(field_name, value)?;
            resolve_agent_label(agents, path, field_name, &normalized).map(Some)
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    #[test]
    fn load_multi_agent_team_returns_none_when_missing() {
        let temp_dir = TempDir::new().expect("create temp dir");

        assert_eq!(
            load_multi_agent_team(temp_dir.path()).expect("load team"),
            None
        );
    }

    #[test]
    fn load_multi_agent_team_parses_valid_roster() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
team_name = "Builder Team"
default_agent = "planner"

[[agents]]
name = "planner"
role = "planner"
model = "gpt-5.5"
model_provider = "openai"
weight = 20
instructions = "Break the work into tasks."

[[agents]]
name = "lead"
role = "tech lead"
model = "gpt-5"
model_provider = "openai"
weight = 60
instructions = "Own the architecture."

[[agents]]
name = "critic"
role = "critic"
model = "gpt-4.1"
model_provider = "claude"
weight = 20
instructions = "Challenge the plan."
"#,
        )
        .expect("write team file");

        let team = load_multi_agent_team(temp_dir.path())
            .expect("load team")
            .expect("team should be present");

        assert_eq!(
            team,
            MultiAgentTeamConfig {
                team_name: Some("Builder Team".to_string()),
                default_agent: Some("planner".to_string()),
                routing: MultiAgentRoutingConfig::default(),
                agents: vec![
                    MultiAgentAgentConfig {
                        name: "planner".to_string(),
                        role: "planner".to_string(),
                        model: "gpt-5.5".to_string(),
                        model_provider: Some("openai".to_string()),
                        weight: 20,
                        passing_grade: None,
                        next_on_pass: None,
                        next_on_fail: None,
                        instructions: "Break the work into tasks.".to_string(),
                    },
                    MultiAgentAgentConfig {
                        name: "lead".to_string(),
                        role: "tech lead".to_string(),
                        model: "gpt-5".to_string(),
                        model_provider: Some("openai".to_string()),
                        weight: 60,
                        passing_grade: None,
                        next_on_pass: None,
                        next_on_fail: None,
                        instructions: "Own the architecture.".to_string(),
                    },
                    MultiAgentAgentConfig {
                        name: "critic".to_string(),
                        role: "critic".to_string(),
                        model: "gpt-4.1".to_string(),
                        model_provider: Some("claude".to_string()),
                        weight: 20,
                        passing_grade: None,
                        next_on_pass: None,
                        next_on_fail: None,
                        instructions: "Challenge the plan.".to_string(),
                    },
                ],
            }
        );
    }

    #[test]
    fn load_multi_agent_team_parses_parallel_routing() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
team_name = "Builder Team"
default_agent = "architect"

[routing]
developer = "lead"
parallel_reviewers = ["critic", "tester"]
pass_threshold_percent = 80

[[agents]]
name = "architect"
role = "architect"
model = "gpt-5.5"
weight = 15
instructions = "Create the design."

[[agents]]
name = "lead"
role = "developer"
model = "gpt-5.5"
weight = 60
passing_grade = 85
next_on_pass = "project manager"
instructions = "Own the implementation."

[[agents]]
name = "critic"
role = "quality ai"
model = "gpt-4.1"
weight = 15
passing_grade = 70
instructions = "Challenge the implementation."

[[agents]]
name = "tester"
role = "execution tester"
model = "gpt-5.4-mini"
weight = 10
instructions = "Run the app."

[[agents]]
name = "project manager"
role = "project manager"
model = "gpt-5.4-mini"
weight = 10
instructions = "Summarize the final workflow."
"#,
        )
        .expect("write team file");

        let team = load_multi_agent_team(temp_dir.path())
            .expect("load team")
            .expect("team should be present");

        assert_eq!(
            team.routing,
            MultiAgentRoutingConfig {
                developer: Some("lead".to_string()),
                parallel_reviewers: vec!["critic".to_string(), "tester".to_string()],
                pass_threshold_percent: Some(80),
            }
        );
        assert_eq!(
            team.developer_agent().map(|agent| agent.name.as_str()),
            Some("lead")
        );
        assert_eq!(
            team.parallel_review_agents()
                .into_iter()
                .map(|agent| agent.name.as_str())
                .collect::<Vec<_>>(),
            vec!["critic", "tester"]
        );
        assert_eq!(
            team.agent_for_role("project manager")
                .map(|agent| agent.name.as_str()),
            Some("project manager")
        );
        assert_eq!(
            team.agent_for_role("lead")
                .and_then(|agent| agent.next_on_pass.as_deref()),
            Some("project manager")
        );
        assert_eq!(
            team.agent_for_role("lead")
                .and_then(|agent| agent.passing_grade),
            Some(85)
        );
        assert_eq!(
            team.agent_for_role("critic")
                .and_then(|agent| agent.passing_grade),
            Some(70)
        );
    }

    #[test]
    fn parallel_review_agents_falls_back_to_review_agents_only() {
        let team = MultiAgentTeamConfig {
            team_name: None,
            default_agent: Some("developer".to_string()),
            routing: MultiAgentRoutingConfig::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "architect".to_string(),
                    role: "architect".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "planner".to_string(),
                    role: "planner".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 60,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "critic".to_string(),
                    role: "critic".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "tester".to_string(),
                    role: "tester".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "project manager".to_string(),
                    role: "project manager".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
            ],
        };

        assert_eq!(
            team.parallel_review_agents()
                .into_iter()
                .map(|agent| agent.name.as_str())
                .collect::<Vec<_>>(),
            vec!["critic", "tester"]
        );
    }

    #[test]
    fn parallel_review_agents_uses_next_numeric_routing_group() {
        let team = MultiAgentTeamConfig {
            team_name: None,
            default_agent: Some("developer".to_string()),
            routing: MultiAgentRoutingConfig::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "developer".to_string(),
                    role: "developer".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 2,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "critic".to_string(),
                    role: "critic".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 3,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "security".to_string(),
                    role: "security ai".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 3,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "quality".to_string(),
                    role: "quality ai".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 3,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
                MultiAgentAgentConfig {
                    name: "project manager".to_string(),
                    role: "project manager".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 4,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: String::new(),
                },
            ],
        };

        assert!(team.uses_numeric_routing());
        assert_eq!(
            team.parallel_review_agents()
                .into_iter()
                .map(|agent| agent.name.as_str())
                .collect::<Vec<_>>(),
            vec!["critic", "security", "quality"]
        );
    }

    #[test]
    fn agent_for_request_uses_default_agent_when_role_is_omitted() {
        let team = MultiAgentTeamConfig {
            team_name: None,
            default_agent: Some("planner".to_string()),
            routing: MultiAgentRoutingConfig::default(),
            agents: vec![
                MultiAgentAgentConfig {
                    name: "planner".to_string(),
                    role: "planner".to_string(),
                    model: "gpt-5.5".to_string(),
                    model_provider: None,
                    weight: 20,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "Plan it.".to_string(),
                },
                MultiAgentAgentConfig {
                    name: "lead".to_string(),
                    role: "tech lead".to_string(),
                    model: "gpt-5".to_string(),
                    model_provider: None,
                    weight: 80,
                    passing_grade: None,
                    next_on_pass: None,
                    next_on_fail: None,
                    instructions: "Lead it.".to_string(),
                },
            ],
        };

        assert_eq!(
            team.agent_for_request(None)
                .map(|agent| agent.name.as_str()),
            Some("planner")
        );
    }

    #[test]
    fn agent_for_request_does_not_fall_back_for_unknown_role() {
        let team = MultiAgentTeamConfig {
            team_name: None,
            default_agent: Some("planner".to_string()),
            routing: MultiAgentRoutingConfig::default(),
            agents: vec![MultiAgentAgentConfig {
                name: "planner".to_string(),
                role: "planner".to_string(),
                model: "gpt-5.5".to_string(),
                model_provider: None,
                weight: 20,
                passing_grade: None,
                next_on_pass: None,
                next_on_fail: None,
                instructions: "Plan it.".to_string(),
            }],
        };

        assert_eq!(team.agent_for_request(Some("missing")), None);
    }

    #[test]
    fn load_multi_agent_team_rejects_unknown_default_agent() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
default_agent = "missing"

[[agents]]
name = "lead"
role = "tech lead"
model = "gpt-5"
weight = 100
instructions = "Own the architecture."
"#,
        )
        .expect("write team file");

        let err = load_multi_agent_team(temp_dir.path()).expect_err("unknown default should fail");
        assert!(
            err.to_string()
                .contains("references unknown default_agent `missing`")
        );
    }

    #[test]
    fn load_multi_agent_team_rejects_invalid_weight() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
[[agents]]
name = "lead"
role = "tech lead"
model = "gpt-5"
weight = 101
instructions = "Own the architecture."
"#,
        )
        .expect("write team file");

        let err = load_multi_agent_team(temp_dir.path()).expect_err("invalid weight should fail");
        assert!(
            err.to_string()
                .contains("routing must be between 0 and 100")
        );
    }

    #[test]
    fn load_multi_agent_team_treats_missing_routing_as_disabled() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
[[agents]]
name = "developer"
role = "developer"
model = "gpt-5"
weight = 3
instructions = "Implement the task."

[[agents]]
name = "planner"
role = "planner"
model = "gpt-5"
instructions = "Plan the task."
"#,
        )
        .expect("write team file");

        let team = load_multi_agent_team(temp_dir.path())
            .expect("load team")
            .expect("team should be present");

        assert_eq!(
            team.agent_for_role("planner")
                .expect("planner should exist")
                .weight,
            0
        );
        assert!(team.uses_numeric_routing());
    }

    #[test]
    fn load_multi_agent_team_rejects_unknown_route_target() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
[[agents]]
name = "lead"
role = "tech lead"
model = "gpt-5"
weight = 50
next_on_pass = "coder"
instructions = "Own the architecture."
"#,
        )
        .expect("write team file");

        let err =
            load_multi_agent_team(temp_dir.path()).expect_err("unknown route target should fail");
        assert!(err.to_string().contains("references unknown agent `coder`"));
    }

    #[test]
    fn load_multi_agent_team_rejects_unknown_parallel_route_target() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
[routing]
parallel_reviewers = ["critic"]

[[agents]]
name = "lead"
role = "developer"
model = "gpt-5"
weight = 100
instructions = "Own the architecture."
"#,
        )
        .expect("write team file");

        let err = load_multi_agent_team(temp_dir.path())
            .expect_err("unknown parallel reviewer target should fail");
        assert!(
            err.to_string()
                .contains("references unknown agent `critic`")
        );
    }

    #[test]
    fn load_multi_agent_team_resolves_role_aliases_in_routes() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("multi_agent.toml");
        std::fs::write(
            &path,
            r#"
[[agents]]
name = "lead"
role = "developer"
model = "gpt-5"
weight = 100
next_on_pass = "developer"
instructions = "Own the implementation."
"#,
        )
        .expect("write team file");

        let team = load_multi_agent_team(temp_dir.path())
            .expect("load team")
            .expect("team should be present");

        assert_eq!(
            team.agents[0].next_on_pass.as_deref(),
            Some("lead"),
            "role aliases should normalize to the configured agent name"
        );
    }

    #[test]
    fn agent_for_role_matches_name_or_role() {
        let team = MultiAgentTeamConfig {
            team_name: None,
            default_agent: None,
            routing: MultiAgentRoutingConfig::default(),
            agents: vec![MultiAgentAgentConfig {
                name: "critic".to_string(),
                role: "quality ai".to_string(),
                model: "gpt-5".to_string(),
                model_provider: Some("claude".to_string()),
                weight: 20,
                passing_grade: None,
                next_on_pass: None,
                next_on_fail: None,
                instructions: "Review the plan.".to_string(),
            }],
        };

        assert_eq!(
            team.agent_for_role("critic")
                .map(|agent| agent.model_provider.as_deref()),
            Some(Some("claude"))
        );
        assert_eq!(
            team.agent_for_role("quality ai")
                .map(|agent| agent.model_provider.as_deref()),
            Some(Some("claude"))
        );
    }
}
