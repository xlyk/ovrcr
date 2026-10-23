//! Conservative native Claude resume configuration. This is an adapter for the
//! existing same-row reopen operation, not another process owner.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::ClaudeConversation;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Shared nonsecret recipe data; Reporter still owns the invocation lifecycle.
pub const REPORTER_MARKER: &str = ": ovrcr-managed-claude-v1; ";
pub const REPORTER_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

pub fn reporter_settings(executable: &str) -> serde_json::Value {
    let quoted = format!("'{}'", executable.replace('\'', "'\"'\"'"));
    let mut hooks = serde_json::Map::new();
    for event in REPORTER_EVENTS {
        hooks.insert((*event).into(), serde_json::json!([{"hooks":[{"type":"command",
            "command":format!("{REPORTER_MARKER}{quoted} report claude --stdin-json"),"timeout":2,"async":false}]}]));
    }
    serde_json::json!({"hooks":hooks,"statusLine":{"type":"command",
        "command":format!("{REPORTER_MARKER}{quoted} report claude-statusline --stdin-json"),"refreshInterval":60}})
}

pub fn config_dir() -> Result<PathBuf> {
    let directory = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .context("Claude configuration directory is unavailable")?;
    if !directory.is_absolute() {
        bail!("Claude configuration directory must be absolute for recovery");
    }
    Ok(directory)
}

/// Strip the original task prompt and reject configuration that cannot be saved
/// as non-secret references. Inline settings and system prompts are not persisted.
pub fn launch_options(argv: &[OsString]) -> Result<Vec<String>> {
    let mut options: Vec<String> = Vec::new();
    let mut index = 1;
    let mut prompt = false;
    while index < argv.len() {
        let arg = argv[index].to_str().context("non-UTF8 Claude option")?;
        if arg == "--" {
            if prompt || index + 2 != argv.len() {
                bail!("unsupported Claude prompt arguments");
            }
            break;
        }
        if matches!(arg, "--resume" | "-r" | "--session-id") {
            index += 2;
            continue;
        }
        if !arg.starts_with('-') {
            // Native --allowedTools is variadic: a later bare word is another grant.
            if options.iter().any(|value| value == "--allowedTools") {
                bail!("generated Claude permissions require an explicit -- before a task prompt");
            }
            if prompt {
                bail!("multiple Claude task prompts");
            }
            prompt = true;
            index += 1;
            continue;
        }
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg, None), |(a, b)| (a, Some(b)));
        match name {
            "--model" | "--permission-mode" | "--agent" | "--settings" | "--setting-sources"
            | "--tools" | "--mcp-config" => {
                let value = if let Some(value) = inline {
                    value
                } else {
                    index += 1;
                    argv.get(index)
                        .and_then(|value| value.to_str())
                        .context("missing Claude option value")?
                };
                if name == "--settings" || name == "--mcp-config" {
                    let path = Path::new(value);
                    if !path.is_file() {
                        bail!("Claude policy must reference an existing file for recovery");
                    }
                    options.push(name.into());
                    options.push(
                        path.canonicalize()?
                            .to_str()
                            .context("non-UTF8 Claude settings path")?
                            .into(),
                    );
                } else {
                    options.extend([name.into(), value.into()]);
                }
            }
            "--allowedTools" if inline.is_none() => {
                index += 1;
                let value = argv
                    .get(index)
                    .and_then(|value| value.to_str())
                    .context("missing generated Claude permission recipe")?;
                if value != crate::claude_delegation::SCOPED_MCP_APPROVALS {
                    bail!("foreign Claude permission recipe");
                }
                options.extend([name.into(), value.into()]);
            }
            "--agents" => {
                let value = if let Some(value) = inline {
                    value
                } else {
                    index += 1;
                    argv.get(index)
                        .and_then(|value| value.to_str())
                        .context("missing Claude helper recipe")?
                };
                crate::claude_delegation::helper_model(value)?;
                options.extend([name.into(), value.into()]);
            }
            "--strict-mcp-config"
            | "--verbose"
            | "--dangerously-skip-permissions"
            | "--allow-dangerously-skip-permissions"
                if inline.is_none() =>
            {
                options.push(name.into())
            }
            _ => bail!("Claude launch configuration cannot be retained as non-secret references"),
        }
        index += 1;
    }
    if options.iter().any(|value| value == "--allowedTools") {
        let value = |name: &str| {
            options
                .windows(2)
                .find(|pair| pair[0] == name)
                .map(|pair| pair[1].as_str())
                .context("incomplete generated Claude permission policy")
        };
        let model = value("--model")?;
        let artifacts = Path::new(value("--mcp-config")?)
            .parent()
            .context("generated Claude policy directory unavailable")?;
        let canonical =
            crate::claude_delegation::launch_argv(Path::new("claude"), model, artifacts, None);
        if canonical[1..]
            .iter()
            .map(|value| value.to_str())
            .ne(options.iter().map(|value| Some(value.as_str())))
        {
            bail!("Claude permission policy differs from the generated Research recipe");
        }
    }
    Ok(options)
}

pub fn validate(reference: &ClaudeConversation) -> Result<()> {
    let id = reference.conversation.as_bytes();
    if id.len() != 36
        || id[14] != b'4'
        || !matches!(id[19], b'8' | b'9' | b'a' | b'b')
        || !id.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        })
    {
        bail!("Claude recovery requires an exact canonical UUIDv4");
    }
    if !reference.executable.is_absolute()
        || !reference.history.is_absolute()
        || !reference.config_dir.is_absolute()
    {
        bail!("Claude recovery references must be absolute");
    }
    if reference.options.len() > 32
        || reference
            .options
            .iter()
            .any(|value| value.len() > 4096 || value.chars().any(char::is_control))
    {
        bail!("Claude recovery configuration exceeds limits");
    }
    let mut argv = vec![reference.executable.clone().into_os_string()];
    argv.extend(reference.options.iter().map(OsString::from));
    if launch_options(&argv)? != reference.options {
        bail!("invalid retained Claude configuration");
    }
    Ok(())
}

pub fn resume_argv(reference: &ClaudeConversation) -> Result<Vec<OsString>> {
    validate(reference)?;
    if config_dir()? != reference.config_dir {
        bail!("Claude configuration changed; restore the recorded CLAUDE_CONFIG_DIR before Retry");
    }
    if !reference.config_dir.is_dir() {
        bail!("Claude configuration directory is unavailable; restore it before Retry");
    }
    let executable = std::fs::metadata(&reference.executable)
        .context("Claude executable is unavailable; restore it before Retry")?;
    if !executable.is_file() || executable.permissions().mode() & 0o111 == 0 {
        bail!("Claude executable is not executable; restore it before Retry");
    }
    if !reference.history.is_file() {
        bail!("Claude conversation history is unavailable; restore it before Retry");
    }
    let mut argv = vec![
        std::env::current_exe()?.into_os_string(),
        "agent".into(),
        "run".into(),
        "claude".into(),
        "--".into(),
        reference.executable.clone().into_os_string(),
        "--resume".into(),
        reference.conversation.clone().into(),
    ];
    argv.extend(reference.options.iter().map(OsString::from));
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_launch_options_drop_task_prompts_and_refuse_inline_configuration() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            launch_options(&args(&[
                "claude",
                "--model=sonnet",
                "--permission-mode",
                "default",
                "private task prompt"
            ]))
            .unwrap(),
            vec!["--model", "sonnet", "--permission-mode", "default"]
        );
        assert!(
            launch_options(&args(&["claude", "--", "private task prompt"]))
                .unwrap()
                .is_empty()
        );
        for option in [
            "--system-prompt",
            "--append-system-prompt",
            "--agents",
            "--settings",
            "--mcp-config",
        ] {
            assert!(
                launch_options(&args(&["claude", option, "{private inline configuration}"]))
                    .is_err(),
                "{option}"
            );
        }
    }

    #[test]
    fn research_permissions_retain_only_the_exact_generated_recipe() {
        let root = tempfile::tempdir().unwrap();
        for name in ["claude-mcp.json", "claude-settings.json"] {
            std::fs::write(root.path().join(name), "{}").unwrap();
        }
        let generated = crate::claude_delegation::launch_argv(
            Path::new("/bin/claude"),
            "sonnet",
            &root.path().canonicalize().unwrap(),
            None,
        );
        let retained = launch_options(&generated).unwrap();
        let mut prompted = generated.clone();
        prompted.extend(["--".into(), "private task".into()]);
        assert_eq!(launch_options(&prompted).unwrap(), retained);
        for extra in ["Bash", "mcp__foreign__tool"] {
            let mut changed = generated.clone();
            changed.push(extra.into());
            assert!(launch_options(&changed).is_err(), "{extra}");
        }
        assert_eq!(
            retained,
            generated[1..]
                .iter()
                .map(|value| value.to_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
        let mut old = generated.clone();
        old.truncate(old.len() - 2);
        assert!(
            !launch_options(&old)
                .unwrap()
                .iter()
                .any(|value| value == "--allowedTools")
        );
        for value in [
            "mcp__ovrcr_child__*",
            "mcp__ovrcr_child__ovrcr_read",
            "Bash",
            "mcp__ovrcr_child__ovrcr_read,mcp__ovrcr_child__ovrcr_publish_question,mcp__ovrcr_child__ovrcr_publish",
        ] {
            let mut changed = generated.clone();
            *changed.last_mut().unwrap() = value.into();
            assert!(launch_options(&changed).is_err(), "{value}");
        }
        for extras in [
            vec![
                "--allowedTools",
                crate::claude_delegation::SCOPED_MCP_APPROVALS,
            ],
            vec!["--agent", "other"],
            vec!["--dangerously-skip-permissions"],
        ] {
            let mut changed = generated.clone();
            changed.extend(extras.into_iter().map(OsString::from));
            assert!(launch_options(&changed).is_err());
        }
        let mut changed = generated.clone();
        changed[4] = "bypassPermissions".into();
        assert!(launch_options(&changed).is_err());
        let reference = ClaudeConversation {
            conversation: "00000000-0000-4000-8000-000000000001".into(),
            executable: "/bin/claude".into(),
            history: "/private/history.jsonl".into(),
            config_dir: "/private/profile".into(),
            options: retained,
        };
        validate(&reference).unwrap();
    }

    #[test]
    fn generated_native_helper_recipe_is_retained_but_changed_inline_policy_is_refused() {
        let generated = crate::claude_delegation::helper_agents("claude-exact-1");
        let options = launch_options(&[
            "claude".into(),
            "--agents".into(),
            generated.clone().into(),
            "--agent".into(),
            "ovrcr-assigned-coordinator".into(),
        ])
        .unwrap();
        assert_eq!(
            options,
            [
                "--agents",
                &generated,
                "--agent",
                "ovrcr-assigned-coordinator"
            ]
        );
        let reference = ClaudeConversation {
            conversation: "00000000-0000-4000-8000-000000000001".into(),
            executable: "/bin/claude".into(),
            history: "/private/history.jsonl".into(),
            config_dir: "/private/profile".into(),
            options: options.clone(),
        };
        validate(&reference).unwrap();
        let mut changed = reference.clone();
        changed.options[1] = generated.replace("mcp__ovrcr_child__ovrcr_read", "Bash");
        assert!(validate(&changed).is_err());
        changed.options[1] = "{}".into();
        assert!(validate(&changed).is_err());
        changed.options[1] = "x".repeat(4097);
        assert!(validate(&changed).is_err());
    }

    #[test]
    fn generated_native_helper_resume_reconstructs_exact_owned_recipe() {
        const KEY: &str = "OVRCR_CLAUDE_HELPER_RESUME_TEST";
        let Some(root) = std::env::var_os(KEY) else {
            // Isolate the provider configuration without racing other tests' environment.
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("history.jsonl"), "").unwrap();
            assert!(std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "claude_recovery::tests::generated_native_helper_resume_reconstructs_exact_owned_recipe", "--nocapture"])
                .env(KEY, root.path()).env("CLAUDE_CONFIG_DIR", root.path())
                .status().unwrap().success());
            return;
        };
        let root = PathBuf::from(root);
        for name in ["claude-mcp.json", "claude-settings.json"] {
            std::fs::write(root.join(name), "{}").unwrap();
        }
        let research = crate::claude_delegation::launch_argv(
            Path::new("/bin/sh"),
            "sonnet",
            &root.canonicalize().unwrap(),
            None,
        );
        let mut research_reference = ClaudeConversation {
            conversation: "00000000-0000-4000-8000-000000000001".into(),
            executable: "/bin/sh".into(),
            history: root.join("history.jsonl"),
            config_dir: root.clone(),
            options: launch_options(&research).unwrap(),
        };
        let resumed = resume_argv(&research_reference).unwrap();
        assert_eq!(resumed[8..], research[1..]);
        research_reference
            .options
            .truncate(research_reference.options.len() - 2);
        let legacy = resume_argv(&research_reference).unwrap();
        assert!(!legacy.iter().any(|value| value == "--allowedTools"));
        assert_eq!(legacy[8..], research[1..research.len() - 2]);
        let generated = crate::claude_delegation::helper_agents("claude-exact-1");
        let mut reference = ClaudeConversation {
            conversation: "00000000-0000-4000-8000-000000000001".into(),
            executable: "/bin/sh".into(),
            history: root.join("history.jsonl"),
            config_dir: root.clone(),
            options: vec![
                "--agents".into(),
                generated,
                "--agent".into(),
                "ovrcr-assigned-coordinator".into(),
            ],
        };
        let argv = resume_argv(&reference).unwrap();
        assert_eq!(argv[5], "/bin/sh");
        assert_eq!(argv[6], "--resume");
        assert_eq!(argv[7], reference.conversation.as_str());
        assert_eq!(
            &argv[8..],
            reference
                .options
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
        reference.options[1] = "{}".into();
        assert!(resume_argv(&reference).is_err());
    }

    #[test]
    fn saved_settings_are_exact_file_references_and_missing_files_fail() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings with spaces.json");
        std::fs::write(&settings, "{\"secret\":\"do-not-copy\"}").unwrap();
        let options = launch_options(&[
            "claude".into(),
            "--settings".into(),
            settings.clone().into_os_string(),
        ])
        .unwrap();
        assert_eq!(
            options,
            vec![
                "--settings".to_owned(),
                settings
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
            ]
        );
        std::fs::remove_file(&settings).unwrap();
        assert!(
            launch_options(&[
                "claude".into(),
                "--settings".into(),
                settings.into_os_string()
            ])
            .is_err()
        );
    }
    #[test]
    fn assigned_read_policy_recovery_retains_disabled_builtins_and_one_exact_mcp_file() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("mcp.json");
        std::fs::write(&config, "{\"mcpServers\":{}}").unwrap();
        let options = launch_options(&[
            "claude".into(),
            "--tools".into(),
            "".into(),
            "--strict-mcp-config".into(),
            "--mcp-config".into(),
            config.clone().into_os_string(),
            "--permission-mode".into(),
            "default".into(),
        ])
        .unwrap();
        assert_eq!(
            options,
            vec![
                "--tools".to_owned(),
                "".into(),
                "--strict-mcp-config".into(),
                "--mcp-config".into(),
                config.canonicalize().unwrap().to_str().unwrap().into(),
                "--permission-mode".into(),
                "default".into()
            ]
        );
        std::fs::remove_file(&config).unwrap();
        assert!(
            launch_options(&[
                "claude".into(),
                "--mcp-config".into(),
                config.into_os_string()
            ])
            .is_err()
        );
    }
}
