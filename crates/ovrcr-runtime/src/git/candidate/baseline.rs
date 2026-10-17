//! Complete inherited inputs and successor-only write authority.
use super::*;
use ovrcr_protocol::CodingTargetReference;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputBaseline {
    pub schema: u32,
    pub project: String,
    pub repository: std::path::PathBuf,
    pub anchor: String,
    pub selected: Vec<CodingTargetReference>,
    pub files: Vec<CandidateFile>,
    /// Complete selected content, even when already present in the commit anchor.
    pub inputs: Vec<CandidateFile>,
}

impl InputBaseline {
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 || self.selected.is_empty() || self.selected.len() > 256 {
            bail!("invalid complete input lineage");
        }
        for (index, selected) in self.selected.iter().enumerate() {
            selected.validate().map_err(anyhow::Error::msg)?;
            if self.selected[..index].contains(selected) {
                bail!("duplicate selected input lineage");
            }
        }
        for files in [&self.files, &self.inputs] {
            CandidateSnapshot {
                input_baseline: None,
                schema: 1,
                project: self.project.clone(),
                repository: self.repository.clone(),
                workspace: "input".into(),
                git_identity: "input-baseline".into(),
                base: self.anchor.clone(),
                head: self.anchor.clone(),
                files: files.clone(),
            }
            .validate(&[".".into()])?;
        }
        Ok(())
    }
    pub fn validate_candidate(
        &self,
        candidate: &CandidateSnapshot,
        scope: &[String],
    ) -> Result<()> {
        self.validate()?;
        candidate.validate(&[".".into()])?;
        if candidate.project != self.project
            || candidate.repository != self.repository
            || candidate.base != self.anchor
        {
            bail!("candidate input baseline provenance changed");
        }
        let inherited: BTreeMap<_, _> = self
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect();
        let actual: BTreeMap<_, _> = candidate
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect();
        let paths: BTreeSet<_> = inherited.keys().chain(actual.keys()).copied().collect();
        for path in paths {
            let same = match (inherited.get(path), actual.get(path)) {
                (Some(a), Some(b)) => {
                    if a.base_mode != b.base_mode
                        || a.base_digest != b.base_digest
                        || a.base_content != b.base_content
                    {
                        bail!("candidate anchor preimage changed");
                    }
                    a.mode == b.mode && a.digest == b.digest && a.content == b.content
                }
                (None, None) => true,
                _ => false,
            };
            if !same && !allowed(path, scope) {
                bail!("successor changed inherited input outside authorized paths: {path}");
            }
        }
        Ok(())
    }
}

pub fn same_postimage(a: &CandidateFile, b: &CandidateFile) -> bool {
    a.mode == b.mode && a.content == b.content && a.digest == b.digest
}

/// Resolve input preimages against the exact successor commit before any effects.
pub fn plan(
    project: &ProjectRecord,
    anchor: &str,
    selected: Vec<CodingTargetReference>,
    required: Vec<CandidateFile>,
    permitted: bool,
) -> Result<InputBaseline> {
    let inputs = required.clone();
    let mut files = Vec::new();
    for mut file in required {
        let (mode, content, hash, _) = preimage(&project.repo, anchor, &file.path)?;
        let already = mode == file.mode && content == file.content && hash == file.digest;
        if !already
            && (!permitted
                || mode != file.base_mode
                || content != file.base_content
                || hash != file.base_digest)
        {
            bail!(
                "accepted predecessor is absent or conflicts with the selected successor commit: {}",
                file.path
            );
        }
        if !already {
            file.base_mode = mode;
            file.base_content = content;
            file.base_digest = hash;
            files.push(file);
        }
    }
    let baseline = InputBaseline {
        schema: 1,
        project: project.name.clone(),
        repository: std::fs::canonicalize(&project.repo)?,
        anchor: anchor.into(),
        selected,
        files,
        inputs,
    };
    baseline.validate()?;
    Ok(baseline)
}

/// The lifecycle owner calls this only for an authenticated parent-selected input.
/// No refs/index/commits change. Conflicts are checked before the first write.
pub fn materialize(
    project: &ProjectRecord,
    workspace: &WorkspaceRecord,
    baseline: &InputBaseline,
) -> Result<()> {
    baseline.validate()?;
    inspect_worktree(project, workspace)?;
    let generation = worktree_identity(&workspace.path)?;
    let head = git_stdout(
        &workspace.path,
        &args(&["rev-parse", "--verify", "HEAD^{commit}"]),
    )?;
    if head != baseline.anchor {
        bail!("successor commit anchor changed before input integration");
    }
    let mut changes = Vec::new();
    for file in &baseline.files {
        let current = read_entry(&workspace.path, Path::new(&file.path))?;
        let post = file.bytes()?.map(|bytes| (file.mode, bytes));
        if current != post {
            if current != file.base_bytes()?.map(|bytes| (file.base_mode, bytes)) {
                bail!(
                    "selected input conflicts with actual successor base: {}",
                    file.path
                );
            }
            changes.push(file);
        }
    }
    for file in changes {
        write_entry(&workspace.path, file)?;
    }
    verify_workspace(project, workspace, baseline)?;
    if worktree_identity(&workspace.path)? != generation {
        bail!("successor replaced during input integration");
    }
    Ok(())
}

pub fn verify_workspace(
    project: &ProjectRecord,
    workspace: &WorkspaceRecord,
    baseline: &InputBaseline,
) -> Result<()> {
    baseline.validate()?;
    let current: CandidateSnapshot = serde_json::from_slice(&capture(
        project,
        workspace,
        &baseline.anchor,
        &[".".into()],
    )?)?;
    if current.files != baseline.files {
        bail!("actual successor input baseline changed before dispatch");
    }
    for file in &baseline.inputs {
        if read_entry(&workspace.path, Path::new(&file.path))?
            != file.bytes()?.map(|bytes| (file.mode, bytes))
        {
            bail!("accepted input content changed before dispatch");
        }
    }
    Ok(())
}

fn write_entry(root: &Path, file: &CandidateFile) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut parent = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(root)?;
    let relative = Path::new(&file.path);
    for part in relative.parent().into_iter().flat_map(Path::components) {
        if open_child(
            &parent,
            part.as_os_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?
        .is_none()
        {
            let name = CString::new(part.as_os_str().as_encoded_bytes())?;
            if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755) } != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("create inherited input parent");
            }
        }
        parent = open_child(
            &parent,
            part.as_os_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?
        .context("input parent disappeared")?;
    }
    let name = CString::new(
        relative
            .file_name()
            .context("input filename missing")?
            .as_encoded_bytes(),
    )?;
    if file.mode == 0 {
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(std::io::Error::last_os_error()).context("delete inherited input");
        }
        return Ok(());
    }
    let temporary = CString::new(format!(".ovrcr-input-{}", super::super::new_generation()?))?;
    let bytes = file.bytes()?.context("input bytes missing")?;
    let result = (|| -> Result<()> {
        if file.mode == 0o120000 {
            let target = CString::new(bytes)?;
            if unsafe { libc::symlinkat(target.as_ptr(), parent.as_raw_fd(), temporary.as_ptr()) }
                != 0
            {
                return Err(std::io::Error::last_os_error()).context("create inherited symlink");
            }
        } else {
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    temporary.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error()).context("create inherited file");
            }
            let mut output = unsafe { File::from_raw_fd(fd) };
            output.write_all(&bytes)?;
            if unsafe {
                libc::fchmod(
                    output.as_raw_fd(),
                    if file.mode == 0o100755 { 0o755 } else { 0o644 },
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error()).context("set inherited mode");
            }
            output.sync_all()?;
        }
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temporary.as_ptr(),
                parent.as_raw_fd(),
                name.as_ptr(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error()).context("publish inherited input");
        }
        parent.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        unsafe { libc::unlinkat(parent.as_raw_fd(), temporary.as_ptr(), 0) };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(path: &str, content: &str) -> CandidateFile {
        CandidateFile {
            path: path.into(),
            base_mode: 0,
            base_content: None,
            base_digest: None,
            mode: 0o100644,
            content: Some(STANDARD.encode(content)),
            digest: Some(format!("{:x}", Sha256::digest(content.as_bytes()))),
        }
    }
    fn target() -> CodingTargetReference {
        serde_json::from_value(serde_json::json!({"assignment":{"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"a","attempt":1,"contract":1,"request":"a","session":1,"run":1},"publication":{"assignment":{"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"a","attempt":1,"contract":1,"request":"a","session":1,"run":1},"kind":"Submission","version":1,"digest":"1".repeat(64)},"candidate":{"path":"/private/candidate.json","digest":"2".repeat(64),"workspace":"a","base":"3".repeat(40),"git_identity":"generation","changed_artifacts":["inherited"]},"source_contract_digest":"4".repeat(64)})).unwrap()
    }
    #[test]
    fn complete_inherited_untracked_bytes_do_not_expand_successor_write_authority() {
        let inherited = file("inherited", "A");
        let baseline = InputBaseline {
            schema: 1,
            project: "project".into(),
            repository: "/repo".into(),
            anchor: "3".repeat(40),
            selected: vec![target()],
            files: vec![inherited.clone()],
            inputs: vec![inherited.clone()],
        };
        let mut candidate = CandidateSnapshot {
            input_baseline: None,
            schema: 1,
            project: "project".into(),
            repository: "/repo".into(),
            workspace: "b".into(),
            git_identity: "generation-b".into(),
            base: baseline.anchor.clone(),
            head: baseline.anchor.clone(),
            files: vec![inherited, file("own", "B")],
        };
        assert!(
            baseline
                .validate_candidate(&candidate, &["own".into()])
                .is_ok()
        );
        candidate.files[0] = file("inherited", "changed");
        assert!(
            baseline
                .validate_candidate(&candidate, &["own".into()])
                .is_err()
        );
        candidate.files.remove(0);
        assert!(
            baseline
                .validate_candidate(&candidate, &["own".into()])
                .is_err()
        );
        assert!(
            baseline
                .validate_candidate(&candidate, &["own".into(), "inherited".into()])
                .is_ok()
        );
    }
}
