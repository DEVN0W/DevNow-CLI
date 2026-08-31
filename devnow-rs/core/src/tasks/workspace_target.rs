use std::path::Path;
use std::path::PathBuf;

use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::config::Config;

pub(super) fn infer_workspace_path(root_input: &[UserInput], fallback: &Path) -> PathBuf {
    root_input
        .iter()
        .flat_map(user_input_paths)
        .last()
        .map(normalize_for_host)
        .unwrap_or_else(|| fallback.to_path_buf())
}

pub(super) fn apply_requested_workspace(config: &mut Config, root_input: &[UserInput]) {
    let Some(requested_path) = root_input
        .iter()
        .flat_map(user_input_paths)
        .last()
        .map(normalize_for_host)
    else {
        return;
    };
    let workspace = existing_workspace_directory(&requested_path);
    let Ok(workspace) = AbsolutePathBuf::from_absolute_path(workspace) else {
        return;
    };
    let file_system_policy_matches_legacy = config.permissions.file_system_sandbox_policy
        == FileSystemSandboxPolicy::from_legacy_sandbox_policy(
            config.permissions.sandbox_policy.get(),
            &config.cwd,
        );

    config.cwd = workspace;
    if file_system_policy_matches_legacy {
        config.permissions.file_system_sandbox_policy =
            FileSystemSandboxPolicy::from_legacy_sandbox_policy(
                config.permissions.sandbox_policy.get(),
                &config.cwd,
            );
    }
    // The parent may have loaded AGENTS.md from a different repository.
    config.user_instructions = None;
}

fn user_input_paths(input: &UserInput) -> Vec<String> {
    match input {
        UserInput::Text { text, .. } => extract_absolute_paths(text),
        _ => Vec::new(),
    }
}

fn extract_absolute_paths(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(normalize_path_candidate)
        .collect()
}

fn normalize_path_candidate(token: &str) -> Option<String> {
    let candidate = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"'
                | '\''
                | ','
                | ';'
                | ':'
                | '.'
                | ')'
                | '('
                | ']'
                | '['
                | '{'
                | '}'
                | '<'
                | '>'
        )
    });
    let is_unix_absolute = candidate.starts_with('/');
    let bytes = candidate.as_bytes();
    let is_windows_absolute = bytes.len() >= 3
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
        && bytes[0].is_ascii_alphabetic();

    (is_unix_absolute || is_windows_absolute).then(|| candidate.to_string())
}

fn normalize_for_host(candidate: String) -> PathBuf {
    #[cfg(windows)]
    {
        let normalized = candidate.replace('\\', "/");
        let bytes = normalized.as_bytes();
        if normalized.len() >= 7
            && normalized[..5].eq_ignore_ascii_case("/mnt/")
            && bytes[5].is_ascii_alphabetic()
            && bytes[6] == b'/'
        {
            return PathBuf::from(format!(
                "{}:\\{}",
                bytes[5] as char,
                normalized[7..].replace('/', "\\")
            ));
        }
        PathBuf::from(candidate)
    }
    #[cfg(not(windows))]
    {
        let bytes = candidate.as_bytes();
        if bytes.len() >= 3
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/')
            && bytes[0].is_ascii_alphabetic()
        {
            let drive = (bytes[0] as char).to_ascii_lowercase();
            let remainder = candidate[3..].replace('\\', "/");
            return PathBuf::from(format!("/mnt/{drive}/{remainder}"));
        }
        PathBuf::from(candidate)
    }
}

fn existing_workspace_directory(requested_path: &Path) -> PathBuf {
    let mut candidate = if requested_path.is_file() {
        requested_path.parent().unwrap_or(requested_path)
    } else {
        requested_path
    };
    while !candidate.is_dir() {
        let Some(parent) = candidate.parent() else {
            return requested_path.to_path_buf();
        };
        candidate = parent;
    }
    candidate.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;

    fn text_input(text: &str) -> Vec<UserInput> {
        vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]
    }

    #[test]
    fn requested_workspace_path_wins_over_fallback() {
        let input = text_input("Create the app in `/mnt/c/POC/test-game-1`.");

        assert_eq!(
            infer_workspace_path(&input, Path::new("/fallback")),
            PathBuf::from("/mnt/c/POC/test-game-1")
        );
    }

    #[test]
    fn workspace_path_falls_back_to_current_directory() {
        let input = text_input("Hello world, please help");

        assert_eq!(
            infer_workspace_path(&input, Path::new("/fallback")),
            PathBuf::from("/fallback")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn windows_workspace_path_is_converted_for_wsl() {
        let input = text_input(r"Create it in C:\agent_works\calculator");

        assert_eq!(
            infer_workspace_path(&input, Path::new("/fallback")),
            PathBuf::from("/mnt/c/agent_works/calculator")
        );
    }

    #[test]
    fn newest_prompt_workspace_overrides_continuation_workspace() {
        let input = vec![
            UserInput::Text {
                text: r"Prior workspace: C:\old_project".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::Text {
                text: r"Continue instead in C:\new_project".to_string(),
                text_elements: Vec::new(),
            },
        ];

        #[cfg(windows)]
        let expected = PathBuf::from(r"C:\new_project");
        #[cfg(not(windows))]
        let expected = PathBuf::from("/mnt/c/new_project");
        assert_eq!(
            infer_workspace_path(&input, Path::new("/fallback")),
            expected
        );
    }

    #[test]
    fn applying_requested_workspace_changes_child_config_cwd() {
        let workspace = tempfile::tempdir().expect("workspace");
        let mut config = crate::config::test_config();
        config.user_instructions = Some("parent repository instructions".to_string());
        let input = text_input(&format!(
            "Test the calculator at {}",
            workspace.path().display()
        ));

        apply_requested_workspace(&mut config, &input);

        assert_eq!(
            config.cwd,
            AbsolutePathBuf::from_absolute_path(workspace.path()).expect("absolute workspace")
        );
        assert_eq!(config.user_instructions, None);
    }
}
