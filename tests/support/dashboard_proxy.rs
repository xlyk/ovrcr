//! Startup discovery for Dashboard sockets hosted by this test process.
use anyhow::Result;
use ovrcr::server::build_identity::{executable_build, identity_path, socket_identity};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn publish_build(socket: &Path) -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let (device, inode) = socket_identity(socket)?;
    let record = serde_json::json!({
        "schema": 1,
        "pid": std::process::id(),
        "socket_device": device,
        "socket_inode": inode,
        "executable": executable,
        "build": executable_build(&executable)?,
    });
    let mut staging = tempfile::NamedTempFile::new_in(socket.parent().unwrap())?;
    staging
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer(staging.as_file_mut(), &record)?;
    staging.as_file().sync_all()?;
    staging.persist(identity_path(socket))?;
    Ok(executable)
}
