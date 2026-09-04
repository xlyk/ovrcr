use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    pub projects: Vec<ProjectRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub name: String,
    pub repo: PathBuf,
    pub workspace_root: PathBuf,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
}

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

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("read registry {}", path.display()));
            }
        };

        let registry: Self = toml::from_str(&contents).context("parse registry")?;
        registry.validate().context("validate registry")?;
        Ok(registry)
    }

    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        self.validate().context("validate registry")?;
        let contents = toml::to_string_pretty(self).context("serialize registry")?;
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
                    return Err(error).with_context(|| {
                        format!("create temporary registry in {}", parent.display())
                    });
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

    pub fn project(&self, name: &str) -> Result<&ProjectRecord> {
        self.projects
            .iter()
            .find(|project| project.name == name)
            .with_context(|| format!("project not found: {name}"))
    }

    pub fn project_mut(&mut self, name: &str) -> Result<&mut ProjectRecord> {
        self.projects
            .iter_mut()
            .find(|project| project.name == name)
            .with_context(|| format!("project not found: {name}"))
    }

    pub fn workspace(&self, project: &str, workspace: &str) -> Result<&WorkspaceRecord> {
        self.project(project)?
            .workspaces
            .iter()
            .find(|record| record.name == workspace)
            .with_context(|| format!("workspace not found: {project}/{workspace}"))
    }

    pub fn add_project(&mut self, project: ProjectRecord) -> Result<()> {
        validate_name(&project.name, "project")?;
        validate_project(&project)?;
        if self
            .projects
            .iter()
            .any(|record| record.name == project.name)
        {
            bail!("duplicate project: {}", project.name);
        }
        self.projects.push(project);
        Ok(())
    }

    pub fn remove_project(&mut self, name: &str) -> Result<ProjectRecord> {
        let index = self
            .projects
            .iter()
            .position(|project| project.name == name)
            .with_context(|| format!("project not found: {name}"))?;
        if !self.projects[index].workspaces.is_empty() {
            bail!("cannot remove project {name}: workspaces remain");
        }
        Ok(self.projects.remove(index))
    }

    pub fn add_workspace(&mut self, project: &str, workspace: WorkspaceRecord) -> Result<()> {
        validate_workspace(&workspace)?;
        let project_record = self.project_mut(project)?;
        if project_record
            .workspaces
            .iter()
            .any(|record| record.name == workspace.name)
        {
            bail!("duplicate workspace: {project}/{}", workspace.name);
        }
        project_record.workspaces.push(workspace);
        Ok(())
    }

    pub fn remove_workspace(&mut self, project: &str, workspace: &str) -> Result<WorkspaceRecord> {
        let project_record = self.project_mut(project)?;
        let index = project_record
            .workspaces
            .iter()
            .position(|record| record.name == workspace)
            .with_context(|| format!("workspace not found: {project}/{workspace}"))?;
        Ok(project_record.workspaces.remove(index))
    }

    fn validate(&self) -> Result<()> {
        for (index, project) in self.projects.iter().enumerate() {
            validate_project(project)?;
            if self.projects[..index]
                .iter()
                .any(|previous| previous.name == project.name)
            {
                bail!("duplicate project: {}", project.name);
            }
        }
        Ok(())
    }
}

fn validate_project(project: &ProjectRecord) -> Result<()> {
    validate_name(&project.name, "project")?;
    for (index, workspace) in project.workspaces.iter().enumerate() {
        validate_workspace(workspace)?;
        if project.workspaces[..index]
            .iter()
            .any(|previous| previous.name == workspace.name)
        {
            bail!("duplicate workspace: {}/{}", project.name, workspace.name);
        }
    }
    Ok(())
}

fn validate_workspace(workspace: &WorkspaceRecord) -> Result<()> {
    validate_name(&workspace.name, "workspace")
}

fn validate_name(name: &str, kind: &str) -> Result<()> {
    if name.is_empty()
        || name.contains("..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("invalid {kind} name: {name:?}");
    }
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
        registry.save_atomic(&path).unwrap();
        assert_eq!(Registry::load(&path).unwrap(), registry);

        let empty_path = dir.path().join("empty-config.toml");
        Registry::default().save_atomic(&empty_path).unwrap();
        assert_eq!(Registry::load(&empty_path).unwrap(), Registry::default());
    }

    #[test]
    fn corrupt_registry_is_reported_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[[projects]\n").unwrap();
        assert!(
            Registry::load(&path)
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
            Registry::load(&path)
                .unwrap_err()
                .to_string()
                .contains("parse registry")
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "");
    }

    #[test]
    fn registry_rejects_duplicate_projects_and_invalid_names() {
        let project = ProjectRecord {
            name: "consigint".into(),
            repo: PathBuf::from("/repo/consigint"),
            workspace_root: PathBuf::from("/worktrees/consigint"),
            workspaces: Vec::new(),
        };
        let mut registry = Registry::default();
        registry.add_project(project.clone()).unwrap();
        assert!(
            registry
                .add_project(project)
                .unwrap_err()
                .to_string()
                .contains("duplicate project")
        );
        assert!(
            registry
                .add_project(ProjectRecord {
                    name: "bad/name".into(),
                    repo: PathBuf::new(),
                    workspace_root: PathBuf::new(),
                    workspaces: Vec::new(),
                })
                .unwrap_err()
                .to_string()
                .contains("invalid project name")
        );
    }

    #[test]
    fn project_removal_rejects_recorded_workspaces() {
        let mut registry = Registry::default();
        registry
            .add_project(ProjectRecord {
                name: "consigint".into(),
                repo: PathBuf::from("/repo/consigint"),
                workspace_root: PathBuf::from("/worktrees/consigint"),
                workspaces: Vec::new(),
            })
            .unwrap();
        registry
            .add_workspace(
                "consigint",
                WorkspaceRecord {
                    name: "cleanup".into(),
                    path: PathBuf::from("/worktrees/consigint/cleanup"),
                    branch: "feature/cleanup".into(),
                },
            )
            .unwrap();
        assert!(
            registry
                .remove_project("consigint")
                .unwrap_err()
                .to_string()
                .contains("workspaces remain")
        );
    }
}
