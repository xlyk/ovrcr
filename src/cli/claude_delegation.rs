//! Compose the positively assigned Claude policy with the existing Reporter.
use super::{AppResult, RuntimeError};
use anyhow::{Context, Result, ensure};
use ovrcr_runtime::claude_delegation::{self, ARTIFACT_ENV};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

fn immutable(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("policy parent unavailable")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(path) {
        Ok(_) => {
            fs::File::open(parent)?.sync_all()?;
        }
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)?;
            let meta = file.metadata()?;
            ensure!(
                meta.is_file()
                    && meta.uid() == unsafe { libc::geteuid() }
                    && meta.mode() & 0o077 == 0
                    && meta.len() <= 512 * 1024,
                "private Claude policy unavailable or replaced"
            );
            let mut existing = Vec::new();
            file.take(512 * 1024 + 1).read_to_end(&mut existing)?;
            ensure!(
                existing == bytes,
                "private Claude policy changed; no launch permitted"
            );
        }
        Err(error) => return Err(error.error.into()),
    }
    Ok(())
}

pub(super) fn prepare(argv: &[OsString]) -> AppResult<bool> {
    if std::env::var_os(ARTIFACT_ENV).is_none() {
        return Ok(false);
    }
    prepare_assigned(argv)
        .map(|()| true)
        .map_err(RuntimeError::internal)
}

fn prepare_assigned(argv: &[OsString]) -> Result<()> {
    let raw = std::env::var_os(ARTIFACT_ENV).context("Claude assignment policy context missing")?;
    let artifacts = Path::new(&raw).canonicalize()?;
    let instance = ovrcr::config::RegistryPath::resolve()?.0.canonicalize()?;
    let meta = fs::metadata(&artifacts)?;
    ensure!(
        artifacts.parent() == Some(instance.join("workflows").as_path())
            && meta.is_dir()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0,
        "foreign or nonprivate Claude assignment policy context"
    );
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(artifacts.join("contract-v1.json"))?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0
            && meta.len() <= 512 * 1024,
        "invalid Claude research contract"
    );
    let mut bytes = Vec::new();
    file.take(512 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 512 * 1024, "Claude contract exceeds bounds");
    let contract: ovrcr::protocol::ResearchContract = serde_json::from_slice(&bytes)?;
    contract.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        contract.provider == "claude"
            && contract.model_provider == "native"
            && artifacts.file_name().and_then(|name| name.to_str())
                == Some(contract.request.as_str()),
        "unsupported or foreign Claude research policy"
    );
    let helper_model = if contract.native_helpers() {
        let value = argv
            .windows(2)
            .find(|args| args[0] == "--agents")
            .and_then(|args| args[1].to_str())
            .context("native helper recipe missing")?;
        Some(claude_delegation::helper_model(value)?)
    } else {
        None
    };
    let executable = claude_delegation::executable()?;
    ensure!(
        argv == claude_delegation::launch_argv(
            &executable,
            &contract.model,
            &artifacts,
            helper_model.as_deref()
        ),
        "native Claude argv differs from assigned read-only policy"
    );
    ensure!(
        std::env::var_os("OVRCR_SESSION_ID").is_some()
            && std::env::var_os("OVRCR_HOOK_TOKEN").is_some(),
        "managed Claude child authority unavailable"
    );
    let binary = std::env::current_exe()?.canonicalize()?;
    let reporter = super::agent_setup::compose(
        &claude_delegation::helper_settings(helper_model.as_deref()).to_string(),
        binary.to_str().context("non-UTF8 Reporter path")?,
    )?;
    immutable(&artifacts.join("claude-settings.json"), reporter.as_bytes())?;
    let files =
        claude_delegation::policy_files(&binary, &contract.request, helper_model.as_deref())?;
    ensure!(
        files.settings == reporter.as_bytes(),
        "assigned settings differ from the trusted Reporter recipe"
    );
    immutable(&artifacts.join("claude-mcp.json"), &files.mcp)?;
    immutable(&artifacts.join("claude-policy.json"), &files.manifest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assigned_policy_replay_refuses_changed_or_symlinked_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let policy = temp.path().join("settings");
        immutable(&policy, b"policy").unwrap();
        immutable(&policy, b"policy").unwrap();
        assert!(immutable(&policy, b"changed").is_err());
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&policy, &link).unwrap();
        assert!(immutable(&link, b"policy").is_err());
    }
}
