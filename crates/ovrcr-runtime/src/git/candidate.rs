//! Immutable, scope-confined deltas. Capture never commits, changes a ref or pushes.
pub mod baseline;
pub mod review;
use super::{git_stdout, inspect_worktree, run_git, worktree_identity};
use crate::config::{ProjectRecord, WorkspaceRecord};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::{CString, OsString},
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Component, Path},
};

const MAX_FILES: usize = 1024;
const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateFile {
    pub path: String,
    pub base_mode: u32,
    pub base_content: Option<String>,
    pub base_digest: Option<String>,
    /// Git mode, or zero for a deletion.
    pub mode: u32,
    /// Base64 file bytes or symlink target; absent only for a deletion.
    pub content: Option<String>,
    pub digest: Option<String>,
}

impl CandidateFile {
    pub fn base_bytes(&self) -> Result<Option<Vec<u8>>> {
        self.base_content
            .as_deref()
            .map(|bytes| {
                STANDARD
                    .decode(bytes)
                    .context("invalid candidate base bytes")
            })
            .transpose()
    }
    pub fn bytes(&self) -> Result<Option<Vec<u8>>> {
        self.content
            .as_deref()
            .map(|bytes| STANDARD.decode(bytes).context("invalid candidate bytes"))
            .transpose()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_baseline: Option<ovrcr_protocol::InputBaselineReference>,
    pub schema: u32,
    pub project: String,
    pub repository: std::path::PathBuf,
    pub workspace: String,
    pub git_identity: String,
    pub base: String,
    pub head: String,
    pub files: Vec<CandidateFile>,
}

impl CandidateSnapshot {
    /// Validate stored bytes without consulting or reopening the disposable checkout.
    pub fn validate(&self, scope: &[String]) -> Result<()> {
        validate_scope(scope)?;
        if !matches!(
            (self.schema, self.input_baseline.is_some()),
            (1, false) | (2, true)
        ) || !self.repository.is_absolute()
            || !is_oid(&self.base)
            || !is_oid(&self.head)
            || self.git_identity.is_empty()
            || self.git_identity.len() > 256
            || self.files.len() > MAX_FILES
        {
            bail!("invalid immutable candidate manifest");
        }
        ovrcr_protocol::validate_name(&self.project, "project")?;
        ovrcr_protocol::validate_name(&self.workspace, "workspace")?;
        let mut paths = BTreeSet::new();
        let mut total = 0usize;
        for file in &self.files {
            if !normal_path(&file.path)
                || !allowed(&file.path, scope)
                || private_path(&file.path)
                || !paths.insert(&file.path)
            {
                bail!("unsafe, duplicate or unauthorized immutable candidate path");
            }
            for (postimage, mode, content, hash) in [
                (false, file.base_mode, &file.base_content, &file.base_digest),
                (true, file.mode, &file.content, &file.digest),
            ] {
                match (mode, content, hash) {
                    (0, None, None) => continue,
                    (0o100644 | 0o100755 | 0o120000, Some(content), Some(hash)) => {
                        if content.len() > MAX_BYTES.div_ceil(3) * 4 {
                            bail!("candidate exceeds byte bound");
                        }
                        let bytes = STANDARD
                            .decode(content)
                            .context("invalid immutable candidate bytes")?;
                        if format!("{:x}", Sha256::digest(&bytes)) != *hash {
                            bail!("immutable candidate content digest changed");
                        }
                        total = total
                            .checked_add(bytes.len())
                            .context("candidate size overflow")?;
                        if total > MAX_BYTES {
                            bail!("candidate exceeds 256 KiB");
                        }
                        if postimage && mode == 0o120000 {
                            validate_symlink(Path::new(&file.path), &bytes)?;
                        }
                    }
                    _ => bail!("immutable candidate mode and content disagree"),
                }
            }
        }
        Ok(())
    }
}

/// Paths are normalized repository-relative prefixes; `.` explicitly grants the repository.
pub fn validate_scope(paths: &[String]) -> Result<()> {
    if paths.is_empty() || paths.len() > 64 {
        bail!("coding requires 1..64 authorized repository paths");
    }
    for path in paths {
        if path != "." && !normal_path(path) {
            bail!("authorized paths must be normalized repository-relative paths");
        }
    }
    Ok(())
}

fn normal_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

fn allowed(path: &str, scope: &[String]) -> bool {
    scope.iter().any(|prefix| {
        prefix == "."
            || path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn private_path(path: &str) -> bool {
    path.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        matches!(
            part.as_str(),
            ".git"
                | ".secrets"
                | ".ssh"
                | ".aws"
                | ".gnupg"
                | ".npmrc"
                | ".netrc"
                | "credentials"
                | "credentials.json"
                | "id_rsa"
                | "id_ed25519"
        ) || part == ".env"
            || (part.starts_with(".env.")
                && !part.ends_with(".example")
                && !part.ends_with(".sample"))
            || part.ends_with(".pem")
            || part.ends_with(".key")
            || part.ends_with(".p12")
    })
}

fn paths(output: &[u8]) -> Result<BTreeSet<String>> {
    output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            let path = std::str::from_utf8(path).context("candidate paths must be UTF-8")?;
            if !normal_path(path) {
                bail!("unsafe candidate path");
            }
            Ok(path.to_owned())
        })
        .collect()
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn preimage(
    worktree: &Path,
    base: &str,
    path: &str,
) -> Result<(u32, Option<String>, Option<String>, usize)> {
    let output = run_git(
        worktree,
        &args(&["--literal-pathspecs", "ls-tree", "-z", base, "--", path]),
    )?;
    if output.stdout.is_empty() {
        return Ok((0, None, None, 0));
    }
    let entry = std::str::from_utf8(&output.stdout)?;
    let header = entry.split_once('\t').context("invalid base tree entry")?.0;
    let mut fields = header.split_whitespace();
    let mode = u32::from_str_radix(fields.next().context("missing base mode")?, 8)?;
    if fields.next() != Some("blob") {
        bail!("candidate capture refuses base submodules and directories: {path}");
    }
    let oid = fields.next().context("missing base object")?;
    let size = git_stdout(worktree, &args(&["cat-file", "-s", oid]))?.parse::<usize>()?;
    if size > MAX_BYTES {
        bail!("candidate base preimage exceeds 256 KiB");
    }
    let bytes = run_git(worktree, &args(&["cat-file", "blob", oid]))?.stdout;
    Ok((
        mode,
        Some(STANDARD.encode(&bytes)),
        Some(format!("{:x}", Sha256::digest(&bytes))),
        bytes.len(),
    ))
}

fn changed_paths(worktree: &Path, base: &str) -> Result<BTreeSet<String>> {
    // Index flags can hide tracked edits even when content and modes changed.
    let flags = run_git(worktree, &args(&["ls-files", "-v", "-z"]))?.stdout;
    if flags
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.first())
        .any(|flag| flag.is_ascii_lowercase() || *flag == b'S')
    {
        bail!("candidate capture refuses assume-unchanged or skip-worktree index entries");
    }
    if !run_git(worktree, &args(&["ls-files", "--unmerged", "-z"]))?
        .stdout
        .is_empty()
    {
        bail!("candidate capture refuses unresolved merges");
    }
    let mut changed = paths(
        &run_git(
            worktree,
            &args(&[
                "-c",
                "core.filemode=true",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--name-only",
                "--no-renames",
                "--ignore-submodules=none",
                "-z",
                base,
                "--",
            ]),
        )?
        .stdout,
    )?;
    changed.extend(paths(
        &run_git(
            worktree,
            &args(&["ls-files", "--others", "--exclude-standard", "-z"]),
        )?
        .stdout,
    )?);
    if changed.len() > MAX_FILES {
        bail!("candidate exceeds 1024 changed files");
    }
    Ok(changed)
}

fn open_child(parent: &File, name: &std::ffi::OsStr, flags: i32) -> Result<Option<File>> {
    let name = CString::new(name.as_encoded_bytes())?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error).context("candidate path unavailable, replaced or a symlink");
    }
    Ok(Some(unsafe { File::from_raw_fd(fd) }))
}

fn validate_symlink(relative: &Path, bytes: &[u8]) -> Result<()> {
    let target = Path::new(std::str::from_utf8(bytes).context("symlink target must be UTF-8")?);
    let mut depth = relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for part in target.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::CurDir => {}
            _ => bail!("candidate symlink escapes the repository"),
        }
    }
    Ok(())
}

fn read_entry(worktree: &Path, relative: &Path) -> Result<Option<(u32, Vec<u8>)>> {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
    let mut parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(worktree)?;
    for part in relative.parent().into_iter().flat_map(Path::components) {
        let Some(next) = open_child(
            &parent,
            part.as_os_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?
        else {
            return Ok(None);
        };
        parent = next;
    }
    let name = relative.file_name().context("candidate file has no name")?;
    let c_name = CString::new(name.as_encoded_bytes())?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            c_name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error).context("inspect candidate file");
    }
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT == libc::S_IFLNK {
        let mut bytes = vec![0; 4097];
        let len = unsafe {
            libc::readlinkat(
                parent.as_raw_fd(),
                c_name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if len < 0 {
            return Err(std::io::Error::last_os_error()).context("read candidate symlink");
        }
        if len as usize == bytes.len() {
            bail!("candidate symlink target exceeds bound");
        }
        bytes.truncate(len as usize);
        validate_symlink(relative, &bytes)?;
        return Ok(Some((0o120000, bytes)));
    }
    let Some(file) = open_child(&parent, name, libc::O_RDONLY | libc::O_NONBLOCK)? else {
        bail!("candidate changed during capture");
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("candidate capture does not support directories, submodules or special files");
    }
    if metadata.len() > MAX_BYTES as u64 {
        bail!("candidate exceeds 256 KiB");
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    Ok(Some((
        if metadata.mode() & 0o111 != 0 {
            0o100755
        } else {
            0o100644
        },
        bytes,
    )))
}

fn scan(worktree: &Path, base: &str, scope: &[String]) -> Result<Vec<CandidateFile>> {
    let changed = changed_paths(worktree, base)?;
    let mut files = Vec::new();
    let mut total = 0usize;
    for path in changed {
        if !allowed(&path, scope) {
            bail!("candidate contains changes outside authorized paths: {path}");
        }
        if private_path(&path) {
            bail!("candidate capture refuses a private credential path: {path}");
        }
        let (base_mode, base_content, base_digest, base_size) = preimage(worktree, base, &path)?;
        total = total
            .checked_add(base_size)
            .context("candidate size overflow")?;
        if total > MAX_BYTES {
            bail!("candidate exceeds 256 KiB");
        }
        let (mode, bytes) = match read_entry(worktree, Path::new(&path))? {
            Some(entry) => entry,
            None => {
                files.push(CandidateFile {
                    path,
                    base_mode,
                    base_content,
                    base_digest,
                    mode: 0,
                    content: None,
                    digest: None,
                });
                continue;
            }
        };
        total = total
            .checked_add(bytes.len())
            .context("candidate size overflow")?;
        if total > MAX_BYTES {
            bail!("candidate exceeds 256 KiB");
        }
        files.push(CandidateFile {
            path,
            base_mode,
            base_content,
            base_digest,
            mode,
            digest: Some(format!("{:x}", Sha256::digest(&bytes))),
            content: Some(STANDARD.encode(bytes)),
        });
    }
    Ok(files)
}

/// Return complete immutable delta bytes for publication outside the checkout.
/// A consumer uses these bytes and the exact base, never mutable checkout paths.
pub fn capture(
    project: &ProjectRecord,
    workspace: &WorkspaceRecord,
    base: &str,
    scope: &[String],
) -> Result<Vec<u8>> {
    validate_scope(scope)?;
    if !is_oid(base) {
        bail!("candidate base must be an exact Git object ID");
    }
    if !project.workspaces.contains(workspace) {
        bail!("candidate workspace is not the registered generation");
    }
    let inspected = inspect_worktree(project, workspace)?;
    let path = &inspected.canonical_path;
    let generation = worktree_identity(path)?;
    let head = git_stdout(path, &args(&["rev-parse", "--verify", "HEAD^{commit}"]))?;
    run_git(path, &args(&["merge-base", "--is-ancestor", base, &head]))
        .context("candidate HEAD does not descend from the recorded base")?;
    let files = scan(path, base, scope)?;
    if files != scan(path, base, scope)?
        || git_stdout(path, &args(&["rev-parse", "--verify", "HEAD^{commit}"]))? != head
        || worktree_identity(path)? != generation
        || inspect_worktree(project, workspace)?.canonical_path != *path
    {
        bail!("candidate changed during capture; no candidate published");
    }
    let snapshot = CandidateSnapshot {
        input_baseline: None,
        schema: 1,
        project: project.name.clone(),
        repository: std::fs::canonicalize(&project.repo)?,
        workspace: workspace.id.clone(),
        git_identity: generation,
        base: base.into(),
        head,
        files,
    };
    snapshot.validate(scope)?;
    let bytes = serde_json::to_vec(&snapshot)?;
    if bytes.len() > 512 * 1024 {
        bail!("candidate manifest exceeds publication bound");
    }
    Ok(bytes)
}

fn is_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_rejects_traversal_and_prefix_confusion() {
        for path in [
            "",
            "../src",
            "/src",
            "src/../secret",
            "src//file",
            "src/./file",
            "src\nfile",
        ] {
            assert!(validate_scope(&[path.into()]).is_err(), "{path:?}");
        }
        let scope = vec!["src".into()];
        assert!(allowed("src/main.rs", &scope));
        assert!(!allowed("src-private/key", &scope));
        assert!(validate_scope(&[".".into()]).is_ok());
    }
    #[test]
    fn private_paths_and_unsafe_git_paths_are_refused() {
        for path in [
            ".env",
            "nested/.env.production",
            ".aws/credentials",
            "private.pem",
            "nested/.git/config",
        ] {
            assert!(private_path(path), "{path}");
        }
        assert!(!private_path(".env.example"));
        assert!(paths(b"src/a\0../escape\0").is_err());
        assert!(paths(b"src/a\0src/b\0").unwrap().len() == 2);
        assert!(!is_oid("main"));
    }
    #[test]
    fn immutable_manifest_rejects_tampering_duplicates_modes_and_scope() {
        let bytes = b"captured bytes";
        let file = CandidateFile {
            path: "src/file".into(),
            base_mode: 0,
            base_content: None,
            base_digest: None,
            mode: 0o100644,
            content: Some(STANDARD.encode(bytes)),
            digest: Some(format!("{:x}", Sha256::digest(bytes))),
        };
        let snapshot = CandidateSnapshot {
            input_baseline: None,
            schema: 1,
            project: "project".into(),
            repository: "/repo".into(),
            workspace: "workspace".into(),
            git_identity: "generation".into(),
            base: "1".repeat(40),
            head: "2".repeat(40),
            files: vec![file],
        };
        let scope = vec!["src".into()];
        assert!(snapshot.validate(&scope).is_ok());
        let mut changed = snapshot.clone();
        changed.files[0].content = Some(STANDARD.encode(b"later mutable bytes"));
        assert!(changed.validate(&scope).is_err());
        changed = snapshot.clone();
        changed.files.push(changed.files[0].clone());
        assert!(changed.validate(&scope).is_err());
        changed = snapshot.clone();
        changed.files[0].mode = 0;
        assert!(changed.validate(&scope).is_err());
        assert!(snapshot.validate(&["other".into()]).is_err());
        changed = snapshot;
        changed.base = "main".into();
        assert!(changed.validate(&scope).is_err());
    }
    #[test]
    fn submodule_changes_cannot_be_hidden_by_repository_diff_settings() {
        let root = tempfile::tempdir().unwrap();
        let git = |path: &Path, values: &[&str]| run_git(path, &args(values)).unwrap();
        git(root.path(), &["init", "-b", "main"]);
        git(root.path(), &["config", "user.name", "OVRCR Tests"]);
        git(
            root.path(),
            &["config", "user.email", "tests@example.invalid"],
        );
        std::fs::write(root.path().join("README"), b"base bytes\n").unwrap();
        git(root.path(), &["add", "README"]);
        git(root.path(), &["commit", "-m", "base"]);
        git(root.path(), &["clone", "--no-hardlinks", ".", "nested"]);
        git(root.path(), &["add", "nested"]);
        git(root.path(), &["commit", "-m", "gitlink base"]);
        let base = git_stdout(root.path(), &args(&["rev-parse", "HEAD"])).unwrap();
        git(root.path(), &["config", "diff.ignoreSubmodules", "all"]);
        std::fs::write(root.path().join("nested/README"), b"child edit\n").unwrap();
        assert!(
            git(root.path(), &["diff", "--name-only", &base])
                .stdout
                .is_empty(),
            "fixture must demonstrate the repository setting hiding the edit"
        );
        assert_eq!(
            changed_paths(root.path(), &base).unwrap(),
            BTreeSet::from(["nested".into()])
        );
        let error = scan(root.path(), &base, &[".".into()]).unwrap_err();
        assert!(
            error.to_string().contains("refuses base submodules"),
            "{error:#}"
        );
    }

    pub(super) fn capture_fixture() -> (tempfile::TempDir, ProjectRecord, WorkspaceRecord, String) {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        for values in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "OVRCR Tests"],
            vec!["config", "user.email", "tests@example.invalid"],
        ] {
            run_git(&repo, &args(&values)).unwrap();
        }
        std::fs::write(repo.join("README"), b"base bytes\n").unwrap();
        std::fs::set_permissions(repo.join("README"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        run_git(&repo, &args(&["add", "README"])).unwrap();
        run_git(&repo, &args(&["commit", "-m", "base"])).unwrap();
        let mut project = ProjectRecord {
            name: "project".into(),
            repo,
            workspace_root: root.path().join("workspaces"),
            workspaces: vec![],
        };
        std::fs::create_dir(&project.workspace_root).unwrap();
        let base = super::super::resolve_commit(&project.repo, "HEAD").unwrap();
        let workspace = super::super::create_worktree(
            &project,
            "coding",
            super::super::BranchSpec::New {
                branch: "coding".into(),
                base: base.clone(),
            },
        )
        .unwrap();
        project.workspaces.push(workspace.clone());
        (root, project, workspace, base)
    }

    #[test]
    fn capture_includes_mode_only_change_under_core_filemode_false_without_config_mutation() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, project, workspace, base) = capture_fixture();
        run_git(
            &workspace.path,
            &args(&["config", "core.filemode", "false"]),
        )
        .unwrap();
        std::fs::set_permissions(
            workspace.path.join("README"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(
            run_git(&workspace.path, &args(&["diff", "--name-only", &base]))
                .unwrap()
                .stdout
                .is_empty(),
            "fixture must demonstrate Git hiding the mode-only change"
        );
        let captured = capture(&project, &workspace, &base, &["README".into()]).unwrap();
        let snapshot: CandidateSnapshot = serde_json::from_slice(&captured).unwrap();
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].path, "README");
        assert_eq!(snapshot.files[0].base_mode, 0o100644);
        assert_eq!(snapshot.files[0].mode, 0o100755);
        assert_eq!(
            snapshot.files[0].base_bytes().unwrap(),
            snapshot.files[0].bytes().unwrap()
        );
        assert_eq!(
            git_stdout(
                &workspace.path,
                &args(&["config", "--bool", "--get", "core.filemode"])
            )
            .unwrap(),
            "false"
        );
    }

    #[test]
    fn capture_refuses_index_flags_that_hide_tracked_edits() {
        for flag in ["--assume-unchanged", "--skip-worktree"] {
            let (_root, project, workspace, base) = capture_fixture();
            run_git(&workspace.path, &args(&["update-index", flag, "README"])).unwrap();
            std::fs::write(workspace.path.join("README"), b"hidden tracked edit\n").unwrap();
            assert!(
                run_git(&workspace.path, &args(&["diff", "--name-only", &base]))
                    .unwrap()
                    .stdout
                    .is_empty(),
                "fixture must demonstrate hidden edits under {flag}"
            );
            let error = capture(&project, &workspace, &base, &["README".into()]).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("assume-unchanged or skip-worktree"),
                "{flag}: {error:#}"
            );
        }
    }

    #[test]
    fn descriptor_reads_preserve_modes_links_deletion_and_refuse_parent_escape() {
        use std::{
            fs,
            os::unix::fs::{PermissionsExt, symlink},
        };
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("run"), b"old bytes").unwrap();
        fs::set_permissions(root.path().join("run"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            read_entry(root.path(), Path::new("run")).unwrap(),
            Some((0o100755, b"old bytes".to_vec()))
        );
        symlink("run", root.path().join("link")).unwrap();
        assert_eq!(
            read_entry(root.path(), Path::new("link")).unwrap(),
            Some((0o120000, b"run".to_vec()))
        );
        symlink("../escape", root.path().join("bad-link")).unwrap();
        assert!(read_entry(root.path(), Path::new("bad-link")).is_err());
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("private"), b"do not read").unwrap();
        symlink(outside.path(), root.path().join("parent")).unwrap();
        assert!(read_entry(root.path(), Path::new("parent/private")).is_err());
        assert_eq!(
            read_entry(root.path(), Path::new("missing/deleted")).unwrap(),
            None
        );
    }
}
