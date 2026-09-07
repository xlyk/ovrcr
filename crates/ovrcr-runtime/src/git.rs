use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::config::{ProjectRecord, WorkspaceRecord};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchSpec {
    New { branch: String, base: String },
    Existing { branch: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeInspection {
    pub canonical_path: PathBuf,
    pub branch: String,
    pub dirty: bool,
}

pub fn validate_project(repo: &Path, workspace_root: &Path) -> Result<(PathBuf, PathBuf)> {
    let repo = fs::canonicalize(repo)
        .with_context(|| format!("canonicalize Git repository {}", repo.display()))?;
    if !repo.is_dir() {
        bail!("Git repository is not a directory: {}", repo.display());
    }

    let output = run_git(
        &repo,
        &[
            OsString::from("rev-parse"),
            OsString::from("--show-toplevel"),
        ],
    )?;
    let git_root = String::from_utf8(output.stdout)
        .context("decode Git repository root")?
        .trim()
        .to_owned();
    let git_root = fs::canonicalize(&git_root)
        .with_context(|| format!("canonicalize Git repository root {git_root}"))?;
    if git_root != repo {
        bail!(
            "repository path is not its Git worktree root: {}",
            repo.display()
        );
    }

    let workspace_root = fs::canonicalize(workspace_root)
        .with_context(|| format!("canonicalize workspace root {}", workspace_root.display()))?;
    if !workspace_root.is_dir() {
        bail!(
            "workspace root is not a directory: {}",
            workspace_root.display()
        );
    }
    Ok((repo, workspace_root))
}

pub fn create_worktree(
    project: &ProjectRecord,
    name: &str,
    branch: BranchSpec,
) -> Result<WorkspaceRecord> {
    let (repo, workspace_root) = validate_project(&project.repo, &project.workspace_root)?;
    validate_workspace_name(name)?;
    let destination = workspace_root.join(name);
    if destination.exists() {
        bail!(
            "worktree destination already exists: {}",
            destination.display()
        );
    }
    let parent = destination
        .parent()
        .context("worktree destination has no parent")?;
    if fs::canonicalize(parent)
        .with_context(|| format!("canonicalize worktree parent {}", parent.display()))?
        != workspace_root
    {
        bail!("worktree destination is outside workspace root");
    }

    let (branch_name, args) = match branch {
        BranchSpec::New { branch, base } => {
            validate_branch(&repo, &branch)?;
            if base.is_empty() {
                bail!("worktree base cannot be empty");
            }
            validate_commitish(&repo, &base)?;
            let args = vec![
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("-b"),
                OsString::from(&branch),
                OsString::from("--"),
                destination.as_os_str().to_owned(),
                OsString::from(base),
            ];
            (branch, args)
        }
        BranchSpec::Existing { branch } => {
            validate_branch(&repo, &branch)?;
            validate_local_branch(&repo, &branch)?;
            let args = vec![
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("--"),
                destination.as_os_str().to_owned(),
                OsString::from(&branch),
            ];
            (branch, args)
        }
    };

    run_git(&repo, &args)?;
    let canonical_path = fs::canonicalize(&destination)
        .with_context(|| format!("canonicalize created worktree {}", destination.display()))?;
    Ok(WorkspaceRecord {
        name: name.to_owned(),
        path: canonical_path,
        branch: branch_name,
    })
}

pub fn inspect_worktree(
    project: &ProjectRecord,
    workspace: &WorkspaceRecord,
) -> Result<WorktreeInspection> {
    let (repo, workspace_root) = validate_project(&project.repo, &project.workspace_root)?;
    let canonical_path = canonical_workspace_path(&workspace_root, workspace)?;
    let entries = worktree_entries(&repo)?;
    let entry = matching_entry(&entries, &canonical_path, &workspace.branch)?;
    let branch = entry
        .1
        .clone()
        .context("Git worktree has no registered branch")?;
    let status = run_git(
        &canonical_path,
        &[
            OsString::from("status"),
            OsString::from("--porcelain=v1"),
            OsString::from("--untracked-files=all"),
        ],
    )?;
    Ok(WorktreeInspection {
        canonical_path,
        branch,
        dirty: !status.stdout.is_empty(),
    })
}

pub fn remove_worktree(project: &ProjectRecord, workspace: &WorkspaceRecord) -> Result<()> {
    let (repo, workspace_root) = validate_project(&project.repo, &project.workspace_root)?;
    if !project.workspaces.iter().any(|record| record == workspace) {
        bail!("workspace is not registered");
    }
    if !workspace.path.starts_with(&workspace_root) {
        bail!("worktree is outside workspace root");
    }
    let expected_path = workspace_root.join(&workspace.name);
    if workspace.path != expected_path {
        bail!("registry/Git path disagreement");
    }
    let canonical_path = canonical_workspace_path(&workspace_root, workspace)?;
    if canonical_path != expected_path {
        bail!("unexpected canonical path");
    }
    let entries = worktree_entries(&repo)?;
    let entry = matching_entry(&entries, &canonical_path, &workspace.branch)?;
    let status = run_git(
        &canonical_path,
        &[
            OsString::from("status"),
            OsString::from("--porcelain=v1"),
            OsString::from("--untracked-files=all"),
        ],
    )?;
    if !status.stdout.is_empty() {
        bail!("worktree has changes");
    }
    let _ = entry;
    run_git(
        &repo,
        &[
            OsString::from("worktree"),
            OsString::from("remove"),
            canonical_path.as_os_str().to_owned(),
        ],
    )?;
    Ok(())
}

fn validate_workspace_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains("..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("invalid workspace name: {name:?}");
    }
    Ok(())
}

fn validate_branch(repo: &Path, branch: &str) -> Result<()> {
    if branch.is_empty() {
        bail!("branch cannot be empty");
    }
    run_git(
        repo,
        &[
            OsString::from("check-ref-format"),
            OsString::from("--branch"),
            OsString::from(branch),
        ],
    )?;
    Ok(())
}

fn validate_commitish(repo: &Path, base: &str) -> Result<()> {
    run_git(
        repo,
        &[
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("--end-of-options"),
            OsString::from(format!("{base}^{{commit}}")),
        ],
    )
    .with_context(|| format!("validate worktree base {base:?}"))?;
    Ok(())
}

fn validate_local_branch(repo: &Path, branch: &str) -> Result<()> {
    let ref_name = format!("refs/heads/{branch}");
    let output = Command::new("git")
        .current_dir(repo)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(&ref_name)
        .output()
        .with_context(|| format!("check local branch {branch:?}"))?;
    if !output.status.success() {
        bail!("local branch does not exist: {branch}");
    }
    Ok(())
}

fn canonical_workspace_path(workspace_root: &Path, workspace: &WorkspaceRecord) -> Result<PathBuf> {
    let path = fs::canonicalize(&workspace.path)
        .with_context(|| format!("canonicalize worktree {}", workspace.path.display()))?;
    if !path.starts_with(workspace_root) {
        bail!("worktree is outside workspace root");
    }
    Ok(path)
}

fn worktree_entries(repo: &Path) -> Result<Vec<(PathBuf, Option<String>)>> {
    let output = run_git(
        repo,
        &[
            OsString::from("worktree"),
            OsString::from("list"),
            OsString::from("--porcelain"),
        ],
    )?;
    parse_worktree_porcelain(&output.stdout)
}

fn matching_entry<'a>(
    entries: &'a [(PathBuf, Option<String>)],
    expected_path: &Path,
    expected_branch: &str,
) -> Result<&'a (PathBuf, Option<String>)> {
    if entries
        .iter()
        .any(|(path, branch)| branch.as_deref() == Some(expected_branch) && path != expected_path)
    {
        bail!("branch checked out elsewhere");
    }
    for entry in entries {
        let (path, branch) = entry;
        if path == expected_path {
            match branch.as_deref() {
                Some(branch) if branch == expected_branch => return Ok(entry),
                Some(branch) => bail!(
                    "registry/Git path disagreement: registered branch {expected_branch:?}, Git branch {branch:?}"
                ),
                None => bail!("registry/Git path disagreement: Git worktree has no branch"),
            }
        }
    }
    bail!("registry/Git path disagreement")
}

fn parse_worktree_porcelain(bytes: &[u8]) -> Result<Vec<(PathBuf, Option<String>)>> {
    let text = String::from_utf8(bytes.to_vec()).context("decode git worktree list")?;
    let mut entries = Vec::new();
    let mut path = None;
    let mut branch = None;
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(path) = path.take() {
                entries.push((path, branch.take()));
            } else if branch.is_some() {
                bail!("malformed git worktree list: branch without worktree");
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("worktree ") {
            if path.is_some() {
                bail!("malformed git worktree list: missing record separator");
            }
            if value.is_empty() {
                bail!("malformed git worktree list: empty path");
            }
            path = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("branch ") {
            if path.is_none() || branch.is_some() {
                bail!("malformed git worktree list: invalid branch record");
            }
            branch = Some(
                value
                    .strip_prefix("refs/heads/")
                    .unwrap_or(value)
                    .to_owned(),
            );
        }
    }
    Ok(entries)
}

fn run_git(repo: &Path, args: &[OsString]) -> Result<Output> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .with_context(|| format!("run git in {}", repo.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git command failed ({}): {}", output.status, stderr.trim());
    }
    Ok(output)
}
