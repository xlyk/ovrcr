use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::config::{ProjectRecord, Registry, WorkspaceRecord};

pub const UNAVAILABLE_CHECKOUT: &str = "checkout unavailable";

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

/// Current checkout label: the full local branch, or `detached @ <short>`.
pub fn checkout_name(path: &Path) -> Result<String> {
    let abbrev = git_stdout(
        path,
        &[
            OsString::from("rev-parse"),
            OsString::from("--abbrev-ref"),
            OsString::from("HEAD"),
        ],
    )?;
    if abbrev == "HEAD" {
        let short = git_stdout(
            path,
            &[
                OsString::from("rev-parse"),
                OsString::from("--short"),
                OsString::from("HEAD"),
            ],
        )?;
        Ok(format!("detached @ {short}"))
    } else if abbrev.is_empty() {
        bail!("Git checkout name is unavailable");
    } else {
        Ok(abbrev)
    }
}

/// Repository default branch using the TUI resolver: sole remote HEAD, else
/// `origin`, then local `main`/`master`, then the current branch. Never invents
/// a branch for a detached checkout.
pub fn default_branch(repo: &Path) -> Result<String> {
    match detected_default(repo)? {
        DefaultSource::Established(name) | DefaultSource::HeadFallback(name) => Ok(name),
    }
}

enum DefaultSource {
    Established(String),
    HeadFallback(String),
}

fn detected_default(repo: &Path) -> Result<DefaultSource> {
    let branches = local_branches(repo).unwrap_or_default();
    let listed = try_git_stdout(repo, &[OsString::from("remote")]).unwrap_or_default();
    let mut names = listed
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let remote_name = match (names.next(), names.next()) {
        (Some(sole), None) => sole.to_owned(),
        _ => "origin".to_owned(),
    };
    let prefix = format!("refs/remotes/{remote_name}/");
    if let Some(remote) = try_git_stdout(
        repo,
        &[
            OsString::from("symbolic-ref"),
            OsString::from("--quiet"),
            OsString::from(format!("{prefix}HEAD")),
        ],
    ) && let Some(branch) = remote.strip_prefix(&prefix)
    {
        return Ok(DefaultSource::Established(
            if branches.iter().any(|local| local == branch) {
                branch.to_owned()
            } else {
                remote
            },
        ));
    }
    for branch in ["main", "master"] {
        if branches.iter().any(|candidate| candidate == branch) {
            return Ok(DefaultSource::Established(branch.to_owned()));
        }
    }
    let head = try_git_stdout(
        repo,
        &[
            OsString::from("rev-parse"),
            OsString::from("--abbrev-ref"),
            OsString::from("HEAD"),
        ],
    )
    .unwrap_or_default();
    if head.is_empty() || head == "HEAD" {
        bail!("No default branch found");
    }
    Ok(DefaultSource::HeadFallback(head))
}

fn stored_root_default(project: &ProjectRecord) -> Option<String> {
    project.workspaces.iter().find_map(|workspace| {
        if !is_repository_checkout(&project.repo, &workspace.path) {
            return None;
        }
        let branch = workspace.branch.trim();
        if branch.is_empty() || branch == UNAVAILABLE_CHECKOUT || branch.starts_with("detached @ ")
        {
            None
        } else {
            Some(workspace.branch.clone())
        }
    })
}

/// Warning when the repository checkout is not on the detected default branch.
/// Inspection failure is not treated as compliance. A HEAD-only fallback must
/// not bless later drift: expected is the stored root branch from setup.
pub fn root_warning(project: &ProjectRecord) -> Option<String> {
    let expected = match detected_default(&project.repo) {
        Ok(DefaultSource::Established(name)) => name,
        Ok(DefaultSource::HeadFallback(head)) => stored_root_default(project).unwrap_or(head),
        Err(error) => match stored_root_default(project) {
            Some(expected) => expected,
            None => {
                return Some(format!(
                    "could not determine the repository default branch: {error:#}"
                ));
            }
        },
    };
    let actual = match checkout_name(&project.repo) {
        Ok(name) => name,
        Err(error) => {
            return Some(format!(
                "could not inspect the repository root checkout (expected {expected}): {error:#}"
            ));
        }
    };
    if actual == expected {
        None
    } else {
        Some(format!(
            "repository root is on {actual}; expected default branch {expected}. Restore the default branch before launching new sessions here."
        ))
    }
}

/// Overlay live checkout labels, including the repository root.
///
/// Use on a returned clone for inventory or offline inspection. Do not persist
/// the result: the root's stored branch remains the setup default.
pub fn observe_registry(registry: &mut Registry) {
    for project in &mut registry.projects {
        for workspace in &mut project.workspaces {
            workspace.branch =
                checkout_name(&workspace.path).unwrap_or_else(|_| UNAVAILABLE_CHECKOUT.to_owned());
        }
    }
}

const IDENTITY_MARKER: &str = "ovrcr-identity";

/// Read the captured generation marker. Never writes; a missing marker is not ours.
pub fn worktree_identity(path: &Path) -> Result<String> {
    read_generation(&absolute_git_dir(path)?)
}

/// Write a private generation marker in the Git admin directory if absent.
///
/// Capture on create/migrate/register only. Verification must use
/// [`worktree_identity`].
pub fn capture_worktree_identity(path: &Path) -> Result<String> {
    capture_generation(&absolute_git_dir(path)?)
}

/// Capture identity for a missing worktree from surviving, uniquely matching
/// admin metadata. Writable migration only: stamps a generation marker if the
/// admin has none. Verification must never call this.
pub(crate) fn recover_worktree_identity(repo: &Path, path: &Path) -> Result<Option<String>> {
    fs::symlink_metadata(repo).with_context(|| {
        format!(
            "repository {} is unavailable; restore access and retry workspace migration",
            repo.display()
        )
    })?;
    let Some(admin) = find_worktree_admin(repo, path)? else {
        return Ok(None);
    };
    Ok(Some(capture_generation(&admin)?))
}

fn absolute_git_dir(path: &Path) -> Result<PathBuf> {
    let git_dir = git_stdout(
        path,
        &[
            OsString::from("rev-parse"),
            OsString::from("--absolute-git-dir"),
        ],
    )?;
    Ok(PathBuf::from(git_dir))
}

fn generation_file(admin: &Path) -> PathBuf {
    admin.join(IDENTITY_MARKER)
}

fn read_generation(admin: &Path) -> Result<String> {
    match read_generation_if_present(admin)? {
        Some(token) => Ok(token),
        None => bail!("Git identity is missing; refuse replacement worktree"),
    }
}

fn read_generation_if_present(admin: &Path) -> Result<Option<String>> {
    let file = generation_file(admin);
    match fs::read_to_string(&file) {
        Ok(text) => {
            let token = text.trim();
            if token.is_empty() {
                bail!("Git identity marker is empty");
            }
            Ok(Some(token.to_owned()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("read Git identity marker {}", file.display()))
        }
    }
}

fn capture_generation(admin: &Path) -> Result<String> {
    if let Some(token) = read_generation_if_present(admin)? {
        return Ok(token);
    }
    let token = new_generation()?;
    let file = generation_file(admin);
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)
    {
        Ok(mut out) => {
            out.write_all(token.as_bytes())
                .and_then(|_| out.sync_all())
                .with_context(|| format!("write Git identity marker {}", file.display()))?;
            Ok(token)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_generation(admin),
        Err(error) => {
            Err(error).with_context(|| format!("create Git identity marker {}", file.display()))
        }
    }
}

fn new_generation() -> Result<String> {
    use std::fmt::Write;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .context("read identity generation source")?;
    let mut token = String::with_capacity(32);
    for byte in bytes {
        write!(&mut token, "{byte:02x}")?;
    }
    Ok(token)
}

/// The repository half of [`validate_project`]: the canonical path of `repo`, confirmed to be the
/// root of its own Git worktree. Separate so a caller that has to create the workspace root can
/// check the repository first and leave no directory behind when this half fails.
pub fn validate_repo(repo: &Path) -> Result<PathBuf> {
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
    Ok(repo)
}

/// The workspace-root half of [`validate_project`]: the canonical path of `workspace_root`,
/// confirmed to be an existing directory.
pub fn validate_workspace_root(workspace_root: &Path) -> Result<PathBuf> {
    let workspace_root = fs::canonicalize(workspace_root)
        .with_context(|| format!("canonicalize workspace root {}", workspace_root.display()))?;
    if !workspace_root.is_dir() {
        bail!(
            "workspace root is not a directory: {}",
            workspace_root.display()
        );
    }
    Ok(workspace_root)
}

pub fn validate_project(repo: &Path, workspace_root: &Path) -> Result<(PathBuf, PathBuf)> {
    Ok((
        validate_repo(repo)?,
        validate_workspace_root(workspace_root)?,
    ))
}

pub fn create_worktree(
    project: &ProjectRecord,
    id: &str,
    branch: BranchSpec,
) -> Result<WorkspaceRecord> {
    let (repo, workspace_root) = validate_project(&project.repo, &project.workspace_root)?;
    ovrcr_protocol::validate_name(id, "workspace")?;
    let destination = workspace_root.join(id);
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
    let git_identity = Some(capture_worktree_identity(&canonical_path)?);
    let branch = checkout_name(&canonical_path).unwrap_or(branch_name);
    Ok(WorkspaceRecord {
        id: id.to_owned(),
        path: canonical_path,
        branch,
        git_identity,
        setup_pending: false,
    })
}

pub fn inspect_worktree(
    project: &ProjectRecord,
    workspace: &WorkspaceRecord,
) -> Result<WorktreeInspection> {
    let (repo, workspace_root) = validate_project(&project.repo, &project.workspace_root)?;
    if is_repository_checkout(&repo, &workspace.path) {
        bail!("refusing to inspect the repository checkout as a worktree");
    }
    let canonical_path = canonical_workspace_path(&workspace_root, workspace)?;
    listed_path(&worktree_entries(&repo)?, &canonical_path)?;
    verify_git_identity(workspace, &canonical_path)?;
    let branch = checkout_name(&canonical_path)?;
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
    if !project.workspaces.iter().any(|record| {
        record.id == workspace.id
            && record.path == workspace.path
            && record.git_identity == workspace.git_identity
    }) {
        bail!("workspace is not registered");
    }
    if is_repository_checkout(&repo, &workspace.path) {
        bail!("refusing to remove the repository checkout");
    }
    if !workspace.path.starts_with(&workspace_root) {
        bail!("worktree is outside workspace root");
    }
    if fs::symlink_metadata(&workspace.path)
        .map_err(|error| error.kind() == std::io::ErrorKind::NotFound)
        .is_err_and(|missing| missing)
    {
        return prune_missing_worktree(&repo, workspace, &workspace.path);
    }
    let canonical_path = canonical_workspace_path(&workspace_root, workspace)?;
    if canonical_path != workspace.path {
        bail!("unexpected canonical path");
    }
    listed_path(&worktree_entries(&repo)?, &canonical_path)?;
    verify_git_identity(workspace, &canonical_path)?;
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

/// Remove the registration of a worktree whose directory was deleted
/// outside OVRCR.
///
/// Allowed only when Git still lists that path as prunable and the surviving
/// admin directory still holds the captured generation marker. Mutation is
/// `git worktree remove` of that path so unrelated prunable admins stay.
fn prune_missing_worktree(
    repo: &Path,
    workspace: &WorkspaceRecord,
    expected_path: &Path,
) -> Result<()> {
    let entries = worktree_entries(repo)?;
    let listed = listed_path(&entries, expected_path)?.clone();
    if !prunable_worktrees(repo)?.iter().any(|path| {
        path == expected_path
            || path == &listed
            || fs::canonicalize(path).is_ok_and(|canonical| canonical == expected_path)
    }) {
        bail!(
            "worktree directory {} is missing but Git does not report it prunable",
            expected_path.display()
        );
    }
    let Some(expected_identity) = workspace.git_identity.as_deref() else {
        bail!("Git identity is missing; refuse replacement worktree");
    };
    let admin = if let Some(admin) = find_worktree_admin(repo, expected_path)? {
        admin
    } else if let Some(admin) = find_worktree_admin(repo, &listed)? {
        admin
    } else {
        bail!(
            "Git admin directory for {} is unavailable; refuse prune",
            expected_path.display()
        );
    };
    match read_generation_if_present(&admin)? {
        Some(actual) if actual == expected_identity => {}
        _ => bail!("unrelated or replacement Git worktree"),
    }
    run_git(
        repo,
        &[
            OsString::from("worktree"),
            OsString::from("remove"),
            listed.as_os_str().to_owned(),
        ],
    )?;
    if listed_path(&worktree_entries(repo)?, expected_path).is_ok() {
        bail!(
            "git worktree remove left {} registered",
            expected_path.display()
        );
    }
    Ok(())
}

fn git_common_dir(repo: &Path) -> Result<PathBuf> {
    let common = git_stdout(
        repo,
        &[
            OsString::from("rev-parse"),
            OsString::from("--git-common-dir"),
        ],
    )?;
    let path = Path::new(&common);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo.join(path)
    };
    fs::canonicalize(&path)
        .with_context(|| format!("canonicalize Git common directory {}", path.display()))
}

fn worktree_path_matches(candidate: &Path, expected: &Path) -> bool {
    candidate == expected
        || fs::canonicalize(candidate).is_ok_and(|canonical| canonical == expected)
        || fs::canonicalize(expected).is_ok_and(|canonical| canonical == candidate)
}

fn find_worktree_admin(repo: &Path, expected: &Path) -> Result<Option<PathBuf>> {
    let mut matched = None;
    for (admin, worktree) in worktree_admin_records(repo)? {
        if !worktree_path_matches(&worktree, expected) {
            continue;
        }
        if let Some(previous) = &matched
            && previous != &admin
        {
            bail!("ambiguous Git admin directory for {}", expected.display());
        }
        matched = Some(admin);
    }
    Ok(matched)
}

fn worktree_admin_records(repo: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let worktrees = git_common_dir(repo)?.join("worktrees");
    let entries = match fs::read_dir(&worktrees) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("read Git worktree admin directory {}", worktrees.display())
            });
        }
    };
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| {
            format!("read Git worktree admin directory {}", worktrees.display())
        })?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat Git worktree admin {}", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }
        let admin = entry.path();
        let gitdir_file = admin.join("gitdir");
        let gitdir = match fs::read_to_string(&gitdir_file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("read Git worktree gitdir {}", gitdir_file.display())
                });
            }
        };
        let gitdir_path = Path::new(gitdir.trim());
        let gitdir_path = if gitdir_path.is_absolute() {
            gitdir_path.to_path_buf()
        } else {
            admin.join(gitdir_path)
        };
        let Some(worktree) = gitdir_path.parent() else {
            bail!(
                "Git worktree gitdir {} has no parent",
                gitdir_file.display()
            );
        };
        records.push((admin, worktree.to_path_buf()));
    }
    Ok(records)
}

fn prunable_worktrees(repo: &Path) -> Result<Vec<PathBuf>> {
    let output = run_git(
        repo,
        &[
            OsString::from("worktree"),
            OsString::from("list"),
            OsString::from("--porcelain"),
        ],
    )?;
    let text = String::from_utf8(output.stdout).context("decode git worktree list")?;
    let mut prunable = Vec::new();
    let mut current = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("worktree ") {
            current = Some(PathBuf::from(value));
        } else if line.starts_with("prunable")
            && let Some(path) = current.take()
        {
            prunable.push(path);
        } else if line.is_empty() {
            current = None;
        }
    }
    Ok(prunable)
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
    match run_git(
        repo,
        &[
            OsString::from("show-ref"),
            OsString::from("--verify"),
            OsString::from("--quiet"),
            OsString::from(ref_name),
        ],
    ) {
        Ok(_) => Ok(()),
        Err(_) => bail!("local branch does not exist: {branch}"),
    }
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

fn listed_path<'a>(
    entries: &'a [(PathBuf, Option<String>)],
    expected_path: &Path,
) -> Result<&'a PathBuf> {
    for (path, _) in entries {
        if path == expected_path {
            return Ok(path);
        }
        if fs::canonicalize(path).is_ok_and(|canonical| canonical == expected_path) {
            return Ok(path);
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

fn is_repository_checkout(repo: &Path, path: &Path) -> bool {
    match (fs::canonicalize(repo), fs::canonicalize(path)) {
        (Ok(repo), Ok(path)) => repo == path,
        _ => path == repo,
    }
}

fn verify_git_identity(workspace: &WorkspaceRecord, path: &Path) -> Result<()> {
    let Some(expected) = workspace.git_identity.as_deref() else {
        bail!("Git identity is missing; refuse replacement worktree");
    };
    let actual = worktree_identity(path)?;
    if actual != expected {
        bail!("unrelated or replacement Git worktree");
    }
    Ok(())
}

fn local_branches(repo: &Path) -> Result<Vec<String>> {
    let output = run_git(
        repo,
        &[
            OsString::from("for-each-ref"),
            OsString::from("refs/heads"),
            OsString::from("--format=%(refname:short)"),
        ],
    )?;
    Ok(String::from_utf8(output.stdout)
        .context("decode local branches")?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn git_stdout(repo: &Path, args: &[OsString]) -> Result<String> {
    let output = run_git(repo, args)?;
    Ok(String::from_utf8(output.stdout)
        .context("decode git output")?
        .trim()
        .to_owned())
}

fn try_git_stdout(repo: &Path, args: &[OsString]) -> Option<String> {
    let text = git_stdout(repo, args).ok()?;
    (!text.is_empty()).then_some(text)
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
