//! Conservative native Claude resume configuration. This is an adapter for the
//! existing same-row reopen operation, not another process owner.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::ClaudeConversation;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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
    let mut options = Vec::new();
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
            "--model" | "--permission-mode" | "--agent" | "--settings" | "--setting-sources" => {
                let value = if let Some(value) = inline {
                    value
                } else {
                    index += 1;
                    argv.get(index)
                        .and_then(|value| value.to_str())
                        .context("missing Claude option value")?
                };
                if name == "--settings" {
                    let path = Path::new(value);
                    if !path.is_file() {
                        bail!("Claude settings must reference an existing file for recovery");
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
        ] {
            assert!(
                launch_options(&args(&["claude", option, "{private inline configuration}"]))
                    .is_err(),
                "{option}"
            );
        }
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
}
