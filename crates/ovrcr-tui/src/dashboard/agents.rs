use super::settings::AgentOverride;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const KNOWN_AGENTS: [&str; 12] = [
    "claude",
    "codex",
    "grok",
    "gemini",
    "aider",
    "opencode",
    "pi",
    "omp",
    "goose",
    "amp",
    "cursor-agent",
    "hermes",
];
/// Detected agents OVRCR launches through `agent run <name> --` so managed reporting
/// (Grok retains history for titles; Hermes has process supervision only). Claude, Codex,
/// Pi, and Oh My Pi defaults also carry that provider's auto-trust flag. Explicit custom
/// overrides keep their stored argv unchanged; a raw shell command is never adopted silently.
pub const MANAGED_AGENTS: [&str; 7] = [
    "claude",
    "codex",
    "pi",
    "omp",
    "grok",
    "hermes",
    "cursor-agent",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSource {
    Detected,
    Shell,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEntry {
    pub name: String,
    pub argv: Vec<OsString>,
    pub source: AgentSource,
}

pub fn detect_agents(
    path: &OsStr,
    shell: Option<&OsStr>,
    launcher: Option<&Path>,
) -> Vec<AgentEntry> {
    let mut entries = Vec::new();
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    for name in KNOWN_AGENTS {
        if let Some(found) = dirs.iter().find_map(|dir| executable_in(dir, name)) {
            let argv = match launcher {
                Some(launcher) if MANAGED_AGENTS.contains(&name) => {
                    let mut native = vec![OsString::from(found)];
                    // Native argv only: the wrapper's `--` is not a prompt separator.
                    ovrcr_protocol::apply_default_auto_trust(name, &mut native);
                    let mut argv = vec![
                        launcher.as_os_str().to_owned(),
                        "agent".into(),
                        "run".into(),
                        name.into(),
                        "--".into(),
                    ];
                    argv.extend(native);
                    argv
                }
                _ => vec![found.into()],
            };
            entries.push(AgentEntry {
                name: name.into(),
                argv,
                source: AgentSource::Detected,
            });
        }
    }
    let shell = shell
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("/bin/sh"));
    entries.push(AgentEntry {
        name: "shell".into(),
        argv: vec![shell],
        source: AgentSource::Shell,
    });
    entries.push(AgentEntry {
        name: "Custom".into(),
        argv: Vec::new(),
        source: AgentSource::Custom,
    });
    entries
}

pub fn apply_overrides(
    mut entries: Vec<AgentEntry>,
    overrides: &[AgentOverride],
) -> Vec<AgentEntry> {
    let mut custom = entries.pop();
    let mut shell = entries.pop();
    for override_agent in overrides {
        let argv = override_agent
            .argv
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        if let Some(existing) = entries
            .iter_mut()
            .find(|entry| entry.name == override_agent.name)
        {
            existing.argv = argv;
        } else if shell
            .as_ref()
            .is_some_and(|entry| entry.name == override_agent.name)
        {
            if let Some(entry) = shell.as_mut() {
                entry.argv = argv;
            }
        } else if custom
            .as_ref()
            .is_some_and(|entry| entry.name == override_agent.name)
        {
            if let Some(entry) = custom.as_mut() {
                entry.argv = argv;
            }
        } else {
            entries.push(AgentEntry {
                name: override_agent.name.clone(),
                argv,
                source: AgentSource::Detected,
            });
        }
    }
    if let Some(shell) = shell {
        entries.push(shell);
    }
    if let Some(custom) = custom {
        entries.push(custom);
    }
    entries
}

fn executable_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = dir.join(name);
    let metadata = std::fs::metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return None;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn write_stub(dir: &Path, name: &str, mode: u32) {
        let path = dir.join(name);
        std::fs::write(&path, []).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(mode);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    #[test]
    fn detects_executables_then_shell_then_custom() {
        let dir = tempfile::tempdir().unwrap();
        write_stub(dir.path(), "claude", 0o755);
        write_stub(dir.path(), "codex", 0o755);
        write_stub(dir.path(), "gemini", 0o644);
        let shell = dir.path().join("zsh");
        write_stub(dir.path(), "zsh", 0o755);

        let agents = detect_agents(dir.path().as_os_str(), Some(shell.as_os_str()), None);
        assert_eq!(
            agents
                .iter()
                .map(|entry| (entry.name.as_str(), entry.source, entry.argv.clone()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "claude",
                    AgentSource::Detected,
                    vec![dir.path().join("claude").into()],
                ),
                (
                    "codex",
                    AgentSource::Detected,
                    vec![dir.path().join("codex").into()],
                ),
                ("shell", AgentSource::Shell, vec![shell.into()]),
                ("Custom", AgentSource::Custom, Vec::new()),
            ]
        );

        let overridden = apply_overrides(
            agents,
            &[AgentOverride {
                name: "claude".into(),
                argv: vec!["claude".into(), "--verbose".into()],
            }],
        );
        assert_eq!(
            overridden[0].argv,
            vec![OsString::from("claude"), OsString::from("--verbose")]
        );
        assert_eq!(overridden[1].name, "codex");
        assert_eq!(overridden[2].name, "shell");
        assert_eq!(overridden[3].name, "Custom");
    }

    #[test]
    fn custom_override_replaces_the_custom_entry() {
        let dir = tempfile::tempdir().unwrap();
        let agents = detect_agents(dir.path().as_os_str(), None, None);

        let overridden = apply_overrides(
            agents,
            &[AgentOverride {
                name: "Custom".into(),
                argv: vec!["my-tool".into(), "--flag".into()],
            }],
        );

        let customs: Vec<_> = overridden
            .iter()
            .filter(|entry| entry.name == "Custom")
            .collect();
        assert_eq!(customs.len(), 1);
        assert_eq!(customs[0].source, AgentSource::Custom);
        assert_eq!(
            customs[0].argv,
            vec![OsString::from("my-tool"), OsString::from("--flag")]
        );
        assert!(
            !overridden
                .iter()
                .any(|entry| entry.name == "Custom" && entry.source == AgentSource::Detected)
        );
    }

    #[test]
    fn cursor_detection_uses_managed_launch() {
        let dir = tempfile::tempdir().unwrap();
        write_stub(dir.path(), "cursor-agent", 0o755);
        let launcher = Path::new("/opt/ovrcr/bin/ovrcr");
        let entries = detect_agents(dir.path().as_os_str(), None, Some(launcher));
        let cursor = entries
            .iter()
            .find(|entry| entry.name == "cursor-agent")
            .unwrap();
        assert_eq!(
            cursor.argv,
            vec![
                OsString::from(launcher),
                "agent".into(),
                "run".into(),
                "cursor-agent".into(),
                "--".into(),
                dir.path().join("cursor-agent").into_os_string(),
            ]
        );
        let custom = apply_overrides(
            entries,
            &[AgentOverride {
                name: "cursor-agent".into(),
                argv: vec!["/custom/cursor-agent".into(), "--resume".into()],
            }],
        );
        assert_eq!(
            custom
                .iter()
                .find(|entry| entry.name == "cursor-agent")
                .unwrap()
                .argv,
            vec![OsString::from("/custom/cursor-agent"), "--resume".into()]
        );
    }

    #[test]
    fn managed_agents_launch_through_the_agent_run_route() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "claude",
            "codex",
            "pi",
            "omp",
            "grok",
            "hermes",
            "cursor-agent",
            "gemini",
        ] {
            write_stub(dir.path(), name, 0o755);
        }
        let launcher = Path::new("/opt/ovrcr/bin/ovrcr");
        let entries = detect_agents(dir.path().as_os_str(), None, Some(launcher));
        let argv = |name: &str| {
            entries
                .iter()
                .find(|e| e.name == name)
                .unwrap()
                .argv
                .clone()
        };
        let trust = |name: &str| match name {
            "claude" => Some("--dangerously-skip-permissions"),
            "codex" => Some("--full-auto"),
            "pi" => Some("--approve"),
            "omp" => Some("--auto-approve"),
            _ => None,
        };
        for name in MANAGED_AGENTS {
            let mut expected = vec![
                OsString::from(launcher),
                "agent".into(),
                "run".into(),
                name.into(),
                "--".into(),
                dir.path().join(name).into_os_string(),
            ];
            if let Some(flag) = trust(name) {
                expected.push(flag.into());
            }
            assert_eq!(argv(name), expected, "{name}");
        }
        // Unmanaged detected agents stay bare even when a launcher is present.
        assert_eq!(
            argv("gemini"),
            vec![dir.path().join("gemini").into_os_string()]
        );
        let bare = detect_agents(dir.path().as_os_str(), None, None);
        assert_eq!(
            bare.iter().find(|e| e.name == "claude").unwrap().argv,
            vec![dir.path().join("claude").into_os_string()]
        );
        assert_eq!(
            bare.iter().find(|e| e.name == "codex").unwrap().argv,
            vec![dir.path().join("codex").into_os_string()]
        );
        // Explicit overrides keep argv unchanged for every managed agent.
        let overridden = apply_overrides(
            entries,
            &[
                AgentOverride {
                    name: "claude".into(),
                    argv: vec!["/custom/claude".into(), "--verbose".into()],
                },
                AgentOverride {
                    name: "codex".into(),
                    argv: vec!["/custom/codex".into()],
                },
                AgentOverride {
                    name: "pi".into(),
                    argv: vec!["/custom/pi".into()],
                },
            ],
        );
        assert_eq!(
            overridden.iter().find(|e| e.name == "claude").unwrap().argv,
            vec![
                OsString::from("/custom/claude"),
                OsString::from("--verbose")
            ]
        );
        assert_eq!(
            overridden.iter().find(|e| e.name == "codex").unwrap().argv,
            vec![OsString::from("/custom/codex")]
        );
        assert_eq!(
            overridden.iter().find(|e| e.name == "pi").unwrap().argv,
            vec![OsString::from("/custom/pi")]
        );
    }

    #[test]
    fn hermes_detection_uses_managed_launch_and_preserves_explicit_override() {
        let dir = tempfile::tempdir().unwrap();
        write_stub(dir.path(), "hermes", 0o755);
        let launcher = Path::new("/opt/ovrcr/bin/ovrcr");
        let entries = detect_agents(dir.path().as_os_str(), None, Some(launcher));
        let hermes = entries.iter().find(|entry| entry.name == "hermes").unwrap();
        assert_eq!(hermes.source, AgentSource::Detected);
        assert_eq!(
            hermes.argv,
            vec![
                launcher.as_os_str().to_owned(),
                "agent".into(),
                "run".into(),
                "hermes".into(),
                "--".into(),
                dir.path().join("hermes").into_os_string(),
            ]
        );
        let overridden = apply_overrides(
            entries,
            &[AgentOverride {
                name: "hermes".into(),
                argv: vec!["/custom/hermes".into(), "--profile".into(), "work".into()],
            }],
        );
        assert_eq!(
            overridden
                .iter()
                .find(|entry| entry.name == "hermes")
                .unwrap()
                .argv,
            vec![
                OsString::from("/custom/hermes"),
                "--profile".into(),
                "work".into()
            ]
        );
        std::fs::set_permissions(
            dir.path().join("hermes"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(
            !detect_agents(dir.path().as_os_str(), None, Some(launcher))
                .iter()
                .any(|entry| entry.name == "hermes")
        );
    }
}
