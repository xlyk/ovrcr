//! Only the Server-selected private Codex child profile is composed here.
use super::{AppResult, RuntimeError};
use anyhow::{Context, Result, ensure};
use ovrcr_runtime::codex_delegation::{self, ARTIFACT_ENV, PROFILE};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

fn directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => ensure!(
            meta.file_type().is_dir(),
            "private Codex path is not a directory"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(0o700).create(path)?;
        }
        Err(e) => return Err(e.into()),
    }
    let meta = fs::metadata(path)?;
    ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
        "Codex home must remain private"
    );
    Ok(())
}
fn immutable(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::symlink_metadata(path)?.file_type().is_file(),
                "private Codex policy is not a regular file"
            );
            let mut existing = Vec::new();
            fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)?
                .take(1_048_577)
                .read_to_end(&mut existing)?;
            ensure!(
                existing == bytes,
                "private Codex policy changed; launch refused"
            );
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub(super) fn prepare(argv: &[OsString]) -> AppResult<()> {
    let selected = argv
        .windows(2)
        .any(|p| p[0] == "--profile" && p[1] == PROFILE);
    if !selected || std::env::var_os(ARTIFACT_ENV).is_none() {
        return Ok(());
    }
    prepare_private(argv).map_err(RuntimeError::internal)
}
fn prepare_private(argv: &[OsString]) -> Result<()> {
    let artifacts = PathBuf::from(
        std::env::var_os(ARTIFACT_ENV).context("Codex child policy context missing")?,
    )
    .canonicalize()?;
    let instance = ovrcr::config::RegistryPath::resolve()?.0.canonicalize()?;
    let home = ovrcr_runtime::codex_recovery::config_dir()?;
    let expected = instance
        .join("codex-children")
        .join(artifacts.file_name().context("missing assignment handle")?);
    ensure!(
        home == expected && artifacts.parent() == Some(instance.join("workflows").as_path()),
        "foreign Codex policy context"
    );
    let contract_file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(artifacts.join("contract-v1.json"))?;
    let meta = contract_file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0
            && meta.len() <= 512 * 1024,
        "invalid private Codex contract"
    );
    let mut bytes = Vec::new();
    contract_file.take(512 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 512 * 1024, "contract exceeds bounds");
    let contract: ovrcr::protocol::ResearchContract = serde_json::from_slice(&bytes)?;
    ensure!(
        contract.provider == "codex" && contract.model_provider == "openai",
        "unsupported Codex policy contract"
    );
    contract.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        artifacts.file_name().and_then(|s| s.to_str()) == Some(&contract.request),
        "foreign Codex task root"
    );
    let native = ovrcr_runtime::codex_delegation::executable()?;
    let expected_argv: Vec<OsString> = vec![
        native.into_os_string(),
        "--model".into(),
        contract.model.into(),
        "--profile".into(),
        PROFILE.into(),
        "--ask-for-approval".into(),
        "never".into(),
        "--no-daemon".into(),
        "--strict-config".into(),
    ];
    ensure!(
        argv == expected_argv,
        "Codex child argv does not match strict policy"
    );
    ensure!(
        std::env::var_os("OVRCR_HOOK_TOKEN").is_some()
            && std::env::var_os("OVRCR_SESSION_ID").is_some(),
        "managed Codex child authority missing"
    );
    directory(&instance.join("codex-children"))?;
    directory(&home)?;
    let binary = std::env::current_exe()?.canonicalize()?;
    let profile = codex_delegation::profile(
        &std::env::current_dir()?.canonicalize()?,
        &artifacts,
        &instance,
        &home,
        &binary,
    )?;
    let base = super::codex_setup::compose(
        "",
        binary.to_str().context("non-UTF8 reporting executable")?,
    )?;
    immutable(&home.join("config.toml"), base.as_bytes())?;
    immutable(
        &home.join(format!("{PROFILE}.config.toml")),
        profile.as_bytes(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_policy_replay_refuses_changes_and_symlink_targets() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("profile");
        immutable(&file, b"owned policy").unwrap();
        immutable(&file, b"owned policy").unwrap();
        assert!(immutable(&file, b"different policy").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"owned policy");
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(immutable(&link, b"owned policy").is_err());
    }
}
