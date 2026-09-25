use super::settings::AgentOverride;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const KNOWN_AGENTS: [&str; 11] = [
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
];
/// Detected agents OVRCR launches through `agent run <name> --` so the owned reporting
/// extension loads beside the user's own, or, for Grok, so the launch retains the session
/// history file for titles. Codex and Claude keep their explicit routes.
pub const MANAGED_AGENTS: [&str; 3] = ["pi", "omp", "grok"];

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
                Some(launcher) if MANAGED_AGENTS.contains(&name) => vec![
                    launcher.as_os_str().to_owned(),
                    "agent".into(),
                    "run".into(),
                    name.into(),
                    "--".into(),
                    found.into(),
                ],
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
    fn managed_agents_launch_through_the_agent_run_route() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["pi", "omp", "grok", "codex"] {
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
        for name in MANAGED_AGENTS {
            assert_eq!(
                argv(name),
                vec![
                    OsString::from(launcher),
                    "agent".into(),
                    "run".into(),
                    name.into(),
                    "--".into(),
                    dir.path().join(name).into_os_string(),
                ],
                "{name}"
            );
        }
        assert_eq!(
            argv("codex"),
            vec![dir.path().join("codex").into_os_string()]
        );
        let bare = detect_agents(dir.path().as_os_str(), None, None);
        assert_eq!(
            bare.iter().find(|e| e.name == "pi").unwrap().argv,
            vec![dir.path().join("pi").into_os_string()]
        );
        let overridden = apply_overrides(
            entries,
            &[AgentOverride {
                name: "pi".into(),
                argv: vec!["/custom/pi".into()],
            }],
        );
        assert_eq!(
            overridden.iter().find(|e| e.name == "pi").unwrap().argv,
            vec![OsString::from("/custom/pi")]
        );
    }
}
