use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use ovrcr_protocol::{ProjectRecord, Registry, WorkspaceRecord};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryPath(pub PathBuf);

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl RegistryPath {
    pub fn resolve() -> Result<Self> {
        if let Some(path) = std::env::var_os("OVRCR_CONFIG") {
            return Ok(Self(PathBuf::from(path)));
        }

        let dirs =
            ProjectDirs::from("", "", "ovrcr").context("resolve OVRCR configuration directory")?;
        Ok(Self(dirs.config_dir().join("config.toml")))
    }
}

pub fn load_registry(path: &Path) -> Result<Registry> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("read registry {}", path.display()));
        }
    };

    let registry: Registry = toml::from_str(&contents).context("parse registry")?;
    registry.validate().context("validate registry")?;
    Ok(registry)
}

pub fn save_registry_atomic(registry: &Registry, path: &Path) -> Result<()> {
    registry.validate().context("validate registry")?;
    let contents = toml::to_string_pretty(registry).context("serialize registry")?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create registry directory {}", parent.display()))?;

    let mut temp_path = None;
    let mut temp_file = None;
    for _ in 0..100 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".ovrcr-config-{}-{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("create temporary registry in {}", parent.display()));
            }
        }
    }

    let temp_path = temp_path.context("choose temporary registry path")?;
    let write_result = (|| -> Result<()> {
        let file = temp_file.as_mut().context("open temporary registry")?;
        file.write_all(contents.as_bytes())
            .context("write temporary registry")?;
        file.flush().context("flush temporary registry")?;
        file.sync_all().context("sync temporary registry")?;
        Ok(())
    })();
    drop(temp_file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("replace registry {}", path.display()));
    }

    #[cfg(unix)]
    File::open(parent)
        .with_context(|| format!("open registry directory {}", parent.display()))?
        .sync_all()
        .with_context(|| format!("sync registry directory {}", parent.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn registry_round_trip_preserves_projects_and_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let registry = Registry {
            projects: vec![ProjectRecord {
                name: "consigint".into(),
                repo: PathBuf::from("/repo/consigint"),
                workspace_root: PathBuf::from("/worktrees/consigint"),
                workspaces: vec![WorkspaceRecord {
                    name: "cleanup".into(),
                    path: PathBuf::from("/worktrees/consigint/cleanup"),
                    branch: "feature/cleanup".into(),
                }],
            }],
        };
        save_registry_atomic(&registry, &path).unwrap();
        assert_eq!(load_registry(&path).unwrap(), registry);

        let empty_path = dir.path().join("empty-config.toml");
        save_registry_atomic(&Registry::default(), &empty_path).unwrap();
        assert_eq!(load_registry(&empty_path).unwrap(), Registry::default());
    }

    #[test]
    fn corrupt_registry_is_reported_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[[projects]\n").unwrap();
        assert!(
            load_registry(&path)
                .unwrap_err()
                .to_string()
                .contains("parse registry")
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "[[projects]\n");
    }

    #[test]
    fn present_empty_registry_is_reported_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        assert!(
            load_registry(&path)
                .unwrap_err()
                .to_string()
                .contains("parse registry")
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "");
    }
}
