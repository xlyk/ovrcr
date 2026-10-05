//! Startup-captured executable equality, bound to the owning socket and process.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, OpenOptions};
use std::hash::Hasher;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const MAX_IDENTITY_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableBuild {
    bytes: u64,
    fingerprint: u64,
}

/// This fingerprint detects changed builds; it is not an authenticity proof.
pub fn executable_build(path: &Path) -> Result<ExecutableBuild> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open server executable {}", path.display()))?;
    let before = file.metadata().context("inspect server executable")?;
    if !before.is_file() {
        bail!("server executable is not a regular file");
    }
    let mut hash = DefaultHasher::new();
    let mut bytes = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .context("read server executable build")?;
        if count == 0 {
            break;
        }
        hash.write(&buffer[..count]);
        bytes += count as u64;
    }
    let after = file.metadata().context("recheck server executable")?;
    if bytes != before.len()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        bail!("server executable changed while reading its build");
    }
    Ok(ExecutableBuild {
        bytes,
        fingerprint: hash.finish(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerBuild {
    schema: u32,
    pub pid: u32,
    socket_device: u64,
    socket_inode: u64,
    pub executable: PathBuf,
    pub build: ExecutableBuild,
}

pub fn identity_path(socket: &Path) -> PathBuf {
    let mut path = socket.as_os_str().to_os_string();
    path.push(".build.json");
    PathBuf::from(path)
}

pub fn socket_identity(socket: &Path) -> Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(socket).context("inspect server socket identity")?;
    if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::getuid() } {
        bail!("server socket is not an owned Unix socket");
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn file_stamp(metadata: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u32) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
        metadata.mode(),
    )
}

fn read_record(socket: &Path) -> Result<(ServerBuild, fs::Metadata)> {
    let path = identity_path(socket);
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .context("read captured server build")?;
    let metadata = file.metadata().context("inspect captured server build")?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::getuid() }
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() > MAX_IDENTITY_BYTES
    {
        bail!("captured server build is not a private bounded regular file");
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_IDENTITY_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("read captured server build")?;
    if bytes.len() as u64 > MAX_IDENTITY_BYTES {
        bail!("captured server build is oversized");
    }
    let record: ServerBuild =
        serde_json::from_slice(&bytes).context("decode captured server build")?;
    if record.schema != 1
        || record.pid == 0
        || record.build.bytes == 0
        || !record.executable.is_absolute()
    {
        bail!("unrecognized captured server build");
    }
    let current = fs::symlink_metadata(path).context("recheck captured server build")?;
    if file_stamp(&current) != file_stamp(&metadata) {
        bail!("captured server build changed while reading");
    }
    Ok((record, metadata))
}

/// Unknown, legacy or invalid records fail closed to the caller's restart offer.
pub fn read_server_build(socket: &Path, peer: u32) -> Result<ServerBuild> {
    let (record, _) = read_record(socket)?;
    let (device, inode) = socket_identity(socket)?;
    if record.pid != peer || record.socket_device != device || record.socket_inode != inode {
        bail!("captured server build does not identify the connected socket peer");
    }
    Ok(record)
}

pub(super) struct Publication {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl Publication {
    pub(super) fn publish(
        socket: &Path,
        executable: PathBuf,
        build: ExecutableBuild,
    ) -> Result<Self> {
        let (socket_device, socket_inode) = socket_identity(socket)?;
        let record = ServerBuild {
            schema: 1,
            pid: std::process::id(),
            socket_device,
            socket_inode,
            executable,
            build,
        };
        let path = identity_path(socket);
        let previous = match fs::symlink_metadata(&path) {
            Ok(_) => Some(file_stamp(
                &read_record(socket)
                    .context("refusing to replace an unrecognized existing server build file")?
                    .1,
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("inspect existing server build file"),
        };
        let mut staging =
            tempfile::NamedTempFile::new_in(socket.parent().unwrap_or_else(|| Path::new(".")))
                .context("stage captured server build")?;
        staging
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        staging.write_all(&serde_json::to_vec(&record)?)?;
        staging.as_file().sync_all()?;
        let current = match fs::symlink_metadata(&path) {
            Ok(metadata) => Some(file_stamp(&metadata)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("recheck existing server build file"),
        };
        if current != previous {
            bail!("existing server build file changed during publication");
        }
        let file = staging
            .persist(&path)
            .context("publish captured server build")?;
        let metadata = file.metadata()?;
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.path)
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn fingerprint_detects_byte_changes_with_same_length_and_preserves_open_build() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("executable");
        fs::write(&path, vec![7; 130_000]).unwrap();
        let first = executable_build(&path).unwrap();
        let mut changed = vec![7; 130_000];
        changed[70_000] = 8;
        fs::write(&path, changed).unwrap();
        assert_ne!(executable_build(&path).unwrap(), first);
    }

    #[test]
    fn captured_record_requires_private_file_current_socket_and_peer() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("server.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let executable = std::env::current_exe().unwrap();
        let build = executable_build(&executable).unwrap();
        let publication = Publication::publish(&socket, executable, build.clone()).unwrap();
        assert_eq!(
            read_server_build(&socket, std::process::id())
                .unwrap()
                .build,
            build
        );
        assert!(read_server_build(&socket, std::process::id() + 1).is_err());
        fs::set_permissions(identity_path(&socket), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_server_build(&socket, std::process::id()).is_err());
        fs::set_permissions(identity_path(&socket), fs::Permissions::from_mode(0o600)).unwrap();
        fs::remove_file(&socket).unwrap();
        let _replacement = UnixListener::bind(&socket).unwrap();
        assert!(read_server_build(&socket, std::process::id()).is_err());
        drop(publication);
    }

    #[test]
    fn publication_preserves_unrecognized_entries_and_replaces_private_stale_records() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("server.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let executable = std::env::current_exe().unwrap();
        let build = executable_build(&executable).unwrap();
        let path = identity_path(&socket);
        let publish = || Publication::publish(&socket, executable.clone(), build.clone());
        fs::write(&path, "user notes").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(publish().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "user notes");
        let target = root.path().join("user-file");
        fs::rename(&path, &target).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(publish().is_err());
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "user notes");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(publish().is_err());
        assert!(path.is_dir());
        fs::remove_dir(&path).unwrap();
        let previous = publish().unwrap();
        let replacement = publish().unwrap();
        drop(previous);
        assert!(read_server_build(&socket, std::process::id()).is_ok());
        drop(replacement);
        assert!(!path.exists());
    }

    #[test]
    fn discovery_refuses_symlinks_oversized_and_malformed_records() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("server.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let path = identity_path(&socket);
        for contents in [
            vec![b'x'; MAX_IDENTITY_BYTES as usize + 1],
            b"{broken".to_vec(),
        ] {
            fs::write(&path, contents).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(read_server_build(&socket, std::process::id()).is_err());
        }
        let target = root.path().join("identity");
        fs::rename(&path, &target).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(read_server_build(&socket, std::process::id()).is_err());
    }
}
