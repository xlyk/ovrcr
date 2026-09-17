//! Native Pi and Oh My Pi recovery. Identity comes from SessionManager, never
//! filenames or most-recent lookup. Launching remains the shared server's job.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{AgentProvider, ExtensionConversation, validate_agent_id};
use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn config_dir(provider: AgentProvider) -> Result<PathBuf> {
    let default = match provider {
        AgentProvider::Pi => ".pi/agent",
        AgentProvider::Omp => {
            for variable in [
                "OMP_PROFILE",
                "PI_PROFILE",
                "PI_CONFIG_DIR",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "XDG_CACHE_HOME",
            ] {
                if std::env::var_os(variable).is_some_and(|v| !v.is_empty()) {
                    bail!("OMP configuration override {variable} is not supported for recovery");
                }
            }
            ".omp/agent"
        }
        _ => bail!("not an extension provider"),
    };
    let path = std::env::var_os("PI_CODING_AGENT_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(default)))
        .context("provider configuration directory is unavailable")?;
    if !path.is_absolute() {
        bail!("provider configuration directory must be absolute");
    }
    Ok(path)
}

/// Only explicit, non-secret options can survive restart. Unknown configuration
/// makes recovery unavailable rather than silently changing native behavior.
pub fn launch_options(provider: AgentProvider, argv: &[OsString]) -> Result<Vec<String>> {
    let mut options = Vec::new();
    let mut index = 1;
    while index < argv.len() {
        let arg = argv[index].to_str().context("non-UTF8 provider option")?;
        if arg == "--" {
            break;
        }
        if !arg.starts_with('-') {
            index += 1;
            continue;
        } // task prompts are never retained
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg, None), |(a, b)| (a, Some(b)));
        if inline.is_none()
            && (matches!(name, "--continue" | "-c" | "--no-session")
                || (provider == AgentProvider::Pi && matches!(name, "--resume" | "-r")))
        {
            index += 1;
            continue;
        }
        let identity = (provider == AgentProvider::Pi && name == "--session")
            || (provider == AgentProvider::Omp && matches!(name, "--resume" | "-r"));
        let value_option = matches!(
            name,
            "--model" | "--provider" | "--thinking" | "--tools" | "--models" | "--session-dir"
        ) || (provider == AgentProvider::Omp && name == "--approval-mode");
        if identity || value_option {
            let value = if let Some(value) = inline {
                value
            } else {
                index += 1;
                argv.get(index)
                    .and_then(|v| v.to_str())
                    .context("missing provider option value")?
            };
            if !identity {
                if name == "--session-dir" && !Path::new(value).is_absolute() {
                    bail!("session directory must be absolute for recovery");
                }
                options.extend([name.into(), value.into()]);
            }
        } else if inline.is_none()
            && (matches!(name, "--no-extensions" | "--no-skills" | "--no-tools")
                || (provider == AgentProvider::Pi
                    && matches!(
                        name,
                        "--approve"
                            | "--no-approve"
                            | "--no-context-files"
                            | "--no-prompt-templates"
                            | "--no-themes"
                    ))
                || (provider == AgentProvider::Omp
                    && matches!(
                        name,
                        "--no-lsp" | "--no-pty" | "--no-rules" | "--no-title" | "--auto-approve"
                    )))
        {
            options.push(name.into());
        } else {
            bail!("provider launch configuration cannot be retained as non-secret references");
        }
        index += 1;
    }
    Ok(options)
}

pub fn validate(provider: AgentProvider, reference: &ExtensionConversation) -> Result<()> {
    validate_agent_id(&reference.conversation)?;
    if !reference.executable.is_absolute()
        || !reference.config_dir.is_absolute()
        || reference.history.as_ref().is_some_and(|p| !p.is_absolute())
    {
        bail!("provider recovery references must be absolute");
    }
    if reference.options.len() > 32
        || reference
            .options
            .iter()
            .any(|s| s.len() > 4096 || s.chars().any(char::is_control))
    {
        bail!("provider recovery configuration exceeds limits");
    }
    let mut argv = vec![reference.executable.clone().into_os_string()];
    argv.extend(reference.options.iter().map(OsString::from));
    if launch_options(provider, &argv)? != reference.options {
        bail!("invalid retained provider configuration");
    }
    Ok(())
}

pub fn resume_argv(
    provider: AgentProvider,
    reference: &ExtensionConversation,
) -> Result<Vec<OsString>> {
    validate(provider, reference)?;
    if config_dir(provider)? != reference.config_dir || !reference.config_dir.is_dir() {
        bail!(
            "provider configuration changed or is unavailable; restore the recorded PI_CODING_AGENT_DIR before Retry"
        );
    }
    let executable = std::fs::metadata(&reference.executable)
        .context("provider executable is unavailable; restore it before Retry")?;
    if !executable.is_file() || executable.permissions().mode() & 0o111 == 0 {
        bail!("provider executable is not executable");
    }
    let history = reference
        .history
        .as_ref()
        .context("provider conversation has no native history file")?;
    let file = std::fs::File::open(history)
        .context("provider conversation history is unavailable; restore it before Retry")?;
    if !file.metadata()?.is_file() {
        bail!("provider history is not a regular file");
    }
    // Check only bounded header metadata; never copy transcript content into OVRCR.
    let mut header = String::new();
    BufReader::new(file.take(16 * 1024)).read_line(&mut header)?;
    let header: serde_json::Value =
        serde_json::from_str(&header).context("provider history header is invalid")?;
    if header["type"] != "session" || header["id"] != reference.conversation {
        bail!("provider history identity does not match the recorded conversation");
    }
    let flag = match provider {
        AgentProvider::Pi => "--session",
        AgentProvider::Omp => "--resume",
        _ => bail!("not an extension provider"),
    };
    let mut argv = vec![
        std::env::current_exe()?.into_os_string(),
        "agent".into(),
        "run".into(),
        provider.name().into(),
        "--".into(),
        reference.executable.clone().into_os_string(),
        flag.into(),
        history.clone().into_os_string(),
    ];
    argv.extend(reference.options.iter().map(OsString::from));
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_preserves_explicit_controls_but_never_prompts_or_selectors() {
        for provider in [AgentProvider::Pi, AgentProvider::Omp] {
            let selector = if provider == AgentProvider::Pi {
                "--session"
            } else {
                "--resume"
            };
            let control = if provider == AgentProvider::Pi {
                "--no-approve"
            } else {
                "--approval-mode=always-ask"
            };
            let argv: Vec<OsString> = [
                provider.name(),
                selector,
                "/exact/history.jsonl",
                "--model=some-model",
                control,
                "PRIVATE_PROMPT",
            ]
            .into_iter()
            .map(Into::into)
            .collect();
            let options = launch_options(provider, &argv).unwrap();
            assert_eq!(
                options,
                if provider == AgentProvider::Pi {
                    vec!["--model", "some-model", "--no-approve"]
                } else {
                    vec!["--model", "some-model", "--approval-mode", "always-ask"]
                }
            );
            for argument in [
                "--api-key=PRIVATE",
                "--system-prompt=PRIVATE",
                "--profile=work",
                "--config=relative.yml",
                "--extension=custom.js",
            ] {
                assert!(
                    launch_options(provider, &[provider.name().into(), argument.into()]).is_err(),
                    "{argument}"
                );
            }
        }
    }

    #[test]
    fn references_are_bounded_and_ephemeral_identity_never_advertises_resume() {
        for provider in [AgentProvider::Pi, AgentProvider::Omp] {
            let mut reference = ExtensionConversation {
                conversation: "native-a".into(),
                executable: "/bin/provider".into(),
                history: None,
                config_dir: "/config".into(),
                options: vec![],
            };
            validate(provider, &reference).unwrap();
            let tagged = if provider == AgentProvider::Pi {
                ovrcr_protocol::ConversationReference::Pi(reference.clone())
            } else {
                ovrcr_protocol::ConversationReference::Omp(reference.clone())
            };
            assert!(
                crate::recovery::unavailable(provider.name(), Some(&tagged), false)
                    .unwrap()
                    .contains("no native history")
            );
            reference.history = Some("relative.jsonl".into());
            assert!(validate(provider, &reference).is_err());
            reference.history = Some("/history.jsonl".into());
            reference.conversation.clear();
            assert!(validate(provider, &reference).is_err());
        }
    }
}
