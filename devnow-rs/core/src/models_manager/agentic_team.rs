use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

const MAX_AGENTS: usize = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Configuration loaded from `CODEX_HOME/agentic.toml`.
pub struct AgenticTeamConfig {
    pub team_name: Option<String>,
    pub default_agent: Option<String>,
    pub agents: Vec<AgenticAgentConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgenticAgentConfig {
    pub name: String,
    pub role: String,
    pub model: String,
    pub model_provider: Option<String>,
    pub instructions: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgenticTeamToml {
    team_name: Option<String>,
    default_agent: Option<String>,
    agents: Vec<AgenticAgentToml>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgenticAgentToml {
    name: String,
    role: String,
    model: String,
    model_provider: Option<String>,
    instructions: String,
}

/// Load and validate the manual agent roster from `codex_home`.
pub fn load_agentic_team(codex_home: &Path) -> std::io::Result<Option<AgenticTeamConfig>> {
    let path = codex_home.join("agentic.toml");
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path)?;
    let parsed: AgenticTeamToml = toml::from_str(&contents).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "failed to parse manual agent roster at {}: {err}",
                path.display()
            ),
        )
    })?;

    validate_agentic_team(parsed, &path).map(Some)
}

fn validate_agentic_team(
    parsed: AgenticTeamToml,
    path: &Path,
) -> std::io::Result<AgenticTeamConfig> {
    let team_name = parsed
        .team_name
        .map(|team_name| normalize_required("team_name", team_name, path))
        .transpose()?;
    let default_agent = parsed
        .default_agent
        .map(|default_agent| normalize_required("default_agent", default_agent, path))
        .transpose()?;

    if parsed.agents.is_empty() {
        return invalid_data(path, "must define at least one agent");
    }
    if parsed.agents.len() > MAX_AGENTS {
        return invalid_data(path, "cannot define more than 100 agents");
    }

    let mut seen_names = BTreeSet::new();
    let mut agents = Vec::with_capacity(parsed.agents.len());
    for agent in parsed.agents {
        let name = normalize_required("agents.name", agent.name, path)?;
        if !seen_names.insert(name.to_ascii_lowercase()) {
            return invalid_data(path, &format!("contains duplicate agent name `{name}`"));
        }
        agents.push(AgenticAgentConfig {
            name,
            role: normalize_required("agents.role", agent.role, path)?,
            model: canonical_model_slug(&normalize_required("agents.model", agent.model, path)?),
            model_provider: normalize_optional(
                "agents.model_provider",
                agent.model_provider,
                path,
            )?,
            instructions: normalize_required("agents.instructions", agent.instructions, path)?,
        });
    }

    if let Some(default_agent) = &default_agent
        && !agents.iter().any(|agent| {
            agent.name.eq_ignore_ascii_case(default_agent)
                || agent.role.eq_ignore_ascii_case(default_agent)
        })
    {
        return invalid_data(
            path,
            &format!("references unknown default_agent `{default_agent}`"),
        );
    }

    Ok(AgenticTeamConfig {
        team_name,
        default_agent,
        agents,
    })
}

impl AgenticTeamConfig {
    pub fn agent_for_role(&self, role_name: &str) -> Option<&AgenticAgentConfig> {
        let role_aliases = role_name_aliases(role_name);
        self.agents.iter().find(|agent| {
            agent.name.eq_ignore_ascii_case(role_name)
                || agent.role.eq_ignore_ascii_case(role_name)
                || role_aliases.iter().any(|alias| {
                    agent.name.eq_ignore_ascii_case(alias) || agent.role.eq_ignore_ascii_case(alias)
                })
        })
    }

    pub fn agent_for_request(&self, role_name: Option<&str>) -> Option<&AgenticAgentConfig> {
        match role_name {
            Some(role_name) => self.agent_for_role(role_name),
            None => self
                .default_agent
                .as_deref()
                .and_then(|default_agent| self.agent_for_role(default_agent)),
        }
    }
}

fn canonical_model_slug(model: &str) -> String {
    match model.to_ascii_lowercase().as_str() {
        "gpt.5-4" => "gpt-5.4".to_string(),
        "claude oppus-4.8" => "claude-opus-4-8".to_string(),
        _ => model.to_string(),
    }
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

fn normalize_required(field: &str, value: String, path: &Path) -> std::io::Result<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return invalid_data(path, &format!("has an empty `{field}` value"));
    }
    Ok(value)
}

fn normalize_optional(
    field: &str,
    value: Option<String>,
    path: &Path,
) -> std::io::Result<Option<String>> {
    value
        .map(|value| normalize_required(field, value, path))
        .transpose()
}

fn invalid_data<T>(path: &Path, message: &str) -> std::io::Result<T> {
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("manual agent roster at {} {message}", path.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    const VALID_ROSTER: &str = r#"
team_name = "DevNow Manual Agent Team"
default_agent = "developer"

[[agents]]
name = "developer"
role = "developer"
model = "gpt.5-4"
model_provider = "openai"
instructions = "Implement assigned tasks."

[[agents]]
name = "technical-architect"
role = "technical-architect"
model = "claude oppus-4.8"
model_provider = "claude"
instructions = "Design the solution."
"#;

    fn write_roster(contents: &str) -> tempfile::TempDir {
        let temp_dir = tempdir().expect("create temp dir");
        std::fs::write(temp_dir.path().join("agentic.toml"), contents).expect("write roster");
        temp_dir
    }

    #[test]
    fn missing_roster_returns_none() {
        let temp_dir = tempdir().expect("create temp dir");
        assert_eq!(
            load_agentic_team(temp_dir.path()).expect("load roster"),
            None
        );
    }

    #[test]
    fn valid_roster_is_normalized() {
        let temp_dir = write_roster(VALID_ROSTER);
        let roster = load_agentic_team(temp_dir.path())
            .expect("load roster")
            .expect("roster should exist");
        assert_eq!(roster.default_agent.as_deref(), Some("developer"));
        assert_eq!(roster.agents[0].model, "gpt-5.4");
        assert_eq!(roster.agents[1].model, "claude-opus-4-8");
        assert_eq!(
            roster
                .agent_for_request(None)
                .map(|agent| agent.name.as_str()),
            Some("developer")
        );
    }

    #[test]
    fn malformed_roster_names_exact_path() {
        let temp_dir = write_roster("not = [valid");
        let path = temp_dir.path().join("agentic.toml");
        let error = load_agentic_team(temp_dir.path()).expect_err("malformed roster should fail");
        assert!(error.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn duplicate_names_are_case_insensitive() {
        let temp_dir = write_roster(
            &VALID_ROSTER.replace("name = \"technical-architect\"", "name = \"Developer\""),
        );
        let error = load_agentic_team(temp_dir.path()).expect_err("duplicate should fail");
        assert!(error.to_string().contains("duplicate agent name"));
    }

    #[test]
    fn empty_required_fields_are_rejected() {
        for field in ["name", "role", "model", "instructions"] {
            let contents = VALID_ROSTER.replacen(
                &format!(
                    "{field} = \"{}\"",
                    match field {
                        "name" | "role" => "developer",
                        "model" => "gpt.5-4",
                        "instructions" => "Implement assigned tasks.",
                        _ => unreachable!(),
                    }
                ),
                &format!("{field} = \" \""),
                1,
            );
            let temp_dir = write_roster(&contents);
            let error = load_agentic_team(temp_dir.path()).expect_err("empty field should fail");
            assert!(error.to_string().contains(field));
        }
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let temp_dir = write_roster(&format!(
            "{VALID_ROSTER}\nbase_url = \"https://example.com\"\n"
        ));
        let error = load_agentic_team(temp_dir.path()).expect_err("unknown field should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn strategic_fields_are_rejected() {
        let temp_dir = write_roster(&format!(
            "{VALID_ROSTER}\n[routing]\ndeveloper = \"developer\"\n"
        ));
        let error = load_agentic_team(temp_dir.path()).expect_err("routing should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn unknown_default_agent_is_rejected() {
        let temp_dir = write_roster(&VALID_ROSTER.replace(
            "default_agent = \"developer\"",
            "default_agent = \"missing\"",
        ));
        let error = load_agentic_team(temp_dir.path()).expect_err("unknown default should fail");
        assert!(
            error
                .to_string()
                .contains("unknown default_agent `missing`")
        );
    }

    #[test]
    fn oversized_roster_is_rejected() {
        let agents = (0..=MAX_AGENTS)
            .map(|index| {
                format!(
                    "[[agents]]\nname = \"agent-{index}\"\nrole = \"role-{index}\"\nmodel = \"gpt-5.4\"\ninstructions = \"Work.\"\n"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let temp_dir = write_roster(&agents);
        let error = load_agentic_team(temp_dir.path()).expect_err("oversized roster should fail");
        assert!(error.to_string().contains("more than 100 agents"));
    }
}
