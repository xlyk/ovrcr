//! Explicit unchanged review context from the original repository's exact base.
use super::*;

pub fn context(repository: &Path, base: &str, paths: &[String]) -> Result<Vec<CandidateFile>> {
    ovrcr_protocol::validate_context_paths(paths).map_err(anyhow::Error::msg)?;
    if !is_oid(base) || super::super::resolve_commit(repository, base)? != base {
        bail!("review context requires its exact original base object");
    }
    let mut total = 0usize;
    let mut files = Vec::new();
    for path in paths {
        if !normal_path(path) || private_path(path) {
            bail!("review context refuses unsafe or private paths");
        }
        let (mode, content, hash, size) = preimage(repository, base, path)?;
        if !matches!(mode, 0o100644 | 0o100755 | 0o120000) {
            bail!("necessary review context is missing or has an unsupported mode: {path}");
        }
        total = total
            .checked_add(size)
            .context("review context size overflow")?;
        if total > MAX_BYTES {
            bail!("complete review context exceeds 256 KiB; no content may be omitted");
        }
        let file = CandidateFile {
            path: path.clone(),
            base_mode: mode,
            base_content: content.clone(),
            base_digest: hash.clone(),
            mode,
            content,
            digest: hash,
        };
        readable(
            file.bytes()?
                .as_deref()
                .context("review context bytes missing")?,
        )?;
        if mode == 0o120000 {
            validate_symlink(Path::new(path), &file.bytes()?.unwrap())?;
        }
        files.push(file);
    }
    Ok(files)
}

pub fn readable(bytes: &[u8]) -> Result<&str> {
    let text = std::str::from_utf8(bytes).context("binary review content is not readable")?;
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        bail!("binary review content cannot be presented completely as readable text");
    }
    Ok(text)
}

pub fn readable_image(path: &str, mode: u32, bytes: &[u8]) -> Result<()> {
    readable(bytes)?;
    if mode == 0o120000 {
        validate_symlink(Path::new(path), bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readable_review_images_preserve_committed_staged_unstaged_untracked_deletion_modes_and_link()
    {
        use std::os::unix::fs::PermissionsExt;
        let (_root, project, workspace, _) = super::super::tests::capture_fixture();
        let git = |values: &[&str]| run_git(&workspace.path, &args(values)).unwrap();
        std::fs::write(workspace.path.join("removed"), "deleted preimage\n").unwrap();
        std::fs::write(workspace.path.join("mode"), "same bytes\n").unwrap();
        std::fs::set_permissions(
            workspace.path.join("mode"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base images"]);
        let base = crate::git::resolve_commit(&workspace.path, "HEAD").unwrap();
        std::fs::write(workspace.path.join("committed"), "committed image\n").unwrap();
        git(&["add", "committed"]);
        git(&["commit", "-m", "committed candidate"]);
        std::fs::write(workspace.path.join("staged"), "staged image\n").unwrap();
        git(&["add", "staged"]);
        std::fs::write(workspace.path.join("README"), "unstaged image\n").unwrap();
        std::fs::write(workspace.path.join("untracked"), "untracked image\n").unwrap();
        std::fs::remove_file(workspace.path.join("removed")).unwrap();
        std::fs::set_permissions(
            workspace.path.join("mode"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::os::unix::fs::symlink("untracked", workspace.path.join("safe-link")).unwrap();
        let bytes = capture(&project, &workspace, &base, &[".".into()]).unwrap();
        let snapshot: CandidateSnapshot = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(snapshot.files.len(), 7);
        for (path, expected) in [
            ("committed", "committed image\n"),
            ("staged", "staged image\n"),
            ("README", "unstaged image\n"),
            ("untracked", "untracked image\n"),
            ("safe-link", "untracked"),
        ] {
            let file = snapshot
                .files
                .iter()
                .find(|file| file.path == path)
                .unwrap();
            assert_eq!(file.bytes().unwrap().unwrap(), expected.as_bytes());
            readable_image(&file.path, file.mode, &file.bytes().unwrap().unwrap()).unwrap();
        }
        let deleted = snapshot
            .files
            .iter()
            .find(|file| file.path == "removed")
            .unwrap();
        assert_eq!(
            deleted.base_bytes().unwrap().unwrap(),
            b"deleted preimage\n"
        );
        assert_eq!(deleted.mode, 0);
        assert!(deleted.bytes().unwrap().is_none());
        let mode = snapshot
            .files
            .iter()
            .find(|file| file.path == "mode")
            .unwrap();
        assert_eq!((mode.base_mode, mode.mode), (0o100644, 0o100755));
        assert_eq!(mode.base_bytes().unwrap(), mode.bytes().unwrap());
        std::fs::write(workspace.path.join("staged"), "later checkout\n").unwrap();
        assert_eq!(
            serde_json::from_slice::<CandidateSnapshot>(&bytes).unwrap(),
            snapshot
        );
    }

    #[test]
    fn exact_context_is_explicit_complete_bounded_and_never_reads_mutable_checkout() {
        let root = tempfile::tempdir().unwrap();
        let git = |values: &[&str]| run_git(root.path(), &args(values)).unwrap();
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "OVRCR Tests"]);
        git(&["config", "user.email", "tests@example.invalid"]);
        std::fs::write(root.path().join("caller"), b"original caller\n").unwrap();
        std::fs::write(root.path().join("large-a"), vec![b'a'; MAX_BYTES / 2 + 1]).unwrap();
        std::fs::write(root.path().join("large-b"), vec![b'b'; MAX_BYTES / 2]).unwrap();
        std::fs::write(root.path().join("binary"), [0, 255]).unwrap();
        std::os::unix::fs::symlink("caller", root.path().join("safe-link")).unwrap();
        std::os::unix::fs::symlink("../escape", root.path().join("unsafe-link")).unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "exact original context"]);
        let base = super::super::super::resolve_commit(root.path(), "HEAD").unwrap();
        std::fs::write(root.path().join("caller"), b"staged caller\n").unwrap();
        git(&["add", "caller"]);
        git(&["commit", "-m", "later HEAD"]);
        std::fs::write(root.path().join("caller"), b"unstaged caller\n").unwrap();
        let files = context(root.path(), &base, &["caller".into(), "safe-link".into()]).unwrap();
        assert_eq!(files[0].bytes().unwrap().unwrap(), b"original caller\n");
        assert_eq!(files[1].mode, 0o120000);
        assert_eq!(files[1].bytes().unwrap().unwrap(), b"caller");
        for paths in [
            vec!["missing".into()],
            vec!["binary".into()],
            vec!["unsafe-link".into()],
            vec![".git/config".into()],
            vec!["large-a".into(), "large-b".into()],
            (0..33).map(|i| format!("file-{i}")).collect(),
        ] {
            assert!(context(root.path(), &base, &paths).is_err(), "{paths:?}");
        }
        assert!(context(root.path(), "HEAD", &["caller".into()]).is_err());
        assert_eq!(
            std::fs::read(root.path().join("caller")).unwrap(),
            b"unstaged caller\n"
        );
    }
}
