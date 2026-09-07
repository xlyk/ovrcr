use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

impl Registry {
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

    pub fn validate(&self) -> Result<()> {
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
