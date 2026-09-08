use super::settings::AgentOverride;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const KNOWN_AGENTS: [&str; 9] = [
    "claude",
    "codex",
    "gemini",
    "aider",
    "opencode",
    "pi",
    "goose",
    "amp",
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

pub fn detect_agents(path: &OsStr, shell: Option<&OsStr>) -> Vec<AgentEntry> {
    let mut entries = Vec::new();
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    for name in KNOWN_AGENTS {
        if let Some(found) = dirs.iter().find_map(|dir| executable_in(dir, name)) {
            entries.push(AgentEntry {
                name: name.into(),
                argv: vec![found.into()],
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
    let custom = entries.pop();
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

        let agents = detect_agents(dir.path().as_os_str(), Some(shell.as_os_str()));
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
}
