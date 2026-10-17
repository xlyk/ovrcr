//! Private atomic publications. Every path component is opened without following links;
//! I/O thereafter is relative to the held directory, including the final rename/link.
use anyhow::{Context, Result, bail};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Component, Path},
};

pub(super) struct Directory(File);
impl Directory {
    pub(super) fn sync(&self) -> Result<()> {
        self.0.sync_all().map_err(Into::into)
    }

    pub(super) fn open(path: &Path, create: bool) -> Result<Self> {
        let directory = open_directory(path, create)?;
        let metadata = directory.metadata()?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            bail!("assignment root must be private (0700)");
        }
        Ok(Self(directory))
    }
    pub(super) fn read(&self, name: &str) -> Result<Vec<u8>> {
        let name = CString::new(name)?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("publication unavailable");
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.len() > 512 * 1024
            || metadata.mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            bail!("invalid or oversized private publication");
        }
        let mut bytes = Vec::new();
        file.take(512 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 512 * 1024 {
            bail!("publication exceeds read bound");
        }
        Ok(bytes)
    }
    /// Parent observation checkpoints have one authenticated Server-side writer.
    pub(super) fn replace(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.write(name, bytes, true)
    }
    pub(super) fn publish(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.write(name, bytes, false)
    }
    fn write(&self, name: &str, bytes: &[u8], replace: bool) -> Result<()> {
        if bytes.len() > 512 * 1024 {
            bail!("publication exceeds write bound");
        }
        let temporary = CString::new(format!(".{}", crate::agent_runner::private_identifier()?))?;
        let name = CString::new(name)?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let result = (|| -> Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            // Both operations expose complete bytes; child publications refuse replacement.
            let result = if replace {
                unsafe {
                    libc::renameat(
                        self.0.as_raw_fd(),
                        temporary.as_ptr(),
                        self.0.as_raw_fd(),
                        name.as_ptr(),
                    )
                }
            } else {
                unsafe {
                    libc::linkat(
                        self.0.as_raw_fd(),
                        temporary.as_ptr(),
                        self.0.as_raw_fd(),
                        name.as_ptr(),
                        0,
                    )
                }
            };
            if result != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            self.0.sync_all()?;
            Ok(())
        })();
        unsafe {
            libc::unlinkat(self.0.as_raw_fd(), temporary.as_ptr(), 0);
        }
        result
    }
}

// Publication privacy is checked by Directory::open; ordinary checkout reads use
// this same no-follow walk without changing the private publication contract.
fn open_directory(path: &Path, create: bool) -> Result<File> {
    if !path.is_absolute() {
        bail!("assignment root must be absolute");
    }
    let mut directory = File::open("/")?;
    for part in path.components() {
        let part = match part {
            Component::Normal(part) => part,
            Component::RootDir => continue,
            _ => bail!("invalid assignment path component"),
        };
        let name = CString::new(part.as_encoded_bytes())?;
        if create {
            let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
            if result == 0 {
                directory.sync_all()?;
            }
            if result != 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("assignment directory is unavailable or a symlink");
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

pub(super) struct ReadTree(File);
impl ReadTree {
    pub(super) fn open(path: &Path) -> Result<Self> {
        Ok(Self(open_directory(path, false)?))
    }

    pub(super) fn still_at(&self, path: &Path) -> Result<bool> {
        let current = open_directory(path, false)?.metadata()?;
        let captured = self.0.metadata()?;
        Ok(current.dev() == captured.dev() && current.ino() == captured.ino())
    }

    pub(super) fn read(&self, path: &Path) -> Result<Vec<u8>> {
        if path.as_os_str().is_empty() || path.as_os_str().len() > 4096 {
            bail!("read path must be a bounded relative file");
        }
        // Components normalizes interior dots, so reject them before walking.
        if path
            .as_os_str()
            .as_encoded_bytes()
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
        {
            bail!("read path must contain only normal relative components");
        }
        let mut parent = self.0.try_clone()?;
        let parts: Vec<_> = path.components().collect();
        for (index, part) in parts.iter().enumerate() {
            let Component::Normal(part) = part else {
                bail!("read path must contain only normal relative components");
            };
            let name = CString::new(part.as_encoded_bytes())?;
            let last = index + 1 == parts.len();
            let flags = libc::O_RDONLY
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC
                | if last { 0 } else { libc::O_DIRECTORY };
            let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error())
                    .context("confined file unavailable or a symlink");
            }
            parent = unsafe { File::from_raw_fd(fd) };
        }
        let metadata = parent.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 512 * 1024 {
            bail!("confined read requires a single-link regular file of at most 512 KiB");
        }
        let mut bytes = Vec::new();
        parent.take(512 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 512 * 1024 {
            bail!("confined file grew beyond the read bound");
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod read_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn checkout_read_is_bounded_relative_and_never_follows_links() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let checkout = root.join("checkout");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::set_permissions(&checkout, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir(checkout.join("nested")).unwrap();
        std::fs::write(checkout.join("nested/read.txt"), "bounded read").unwrap();
        std::fs::write(root.join("outside.txt"), "outside").unwrap();
        let tree = ReadTree::open(&checkout).unwrap();
        assert_eq!(
            tree.read(Path::new("nested/read.txt")).unwrap(),
            b"bounded read"
        );
        assert!(Directory::open(&checkout, false).is_err());
        symlink(root.join("outside.txt"), checkout.join("escape")).unwrap();
        symlink(root.clone(), checkout.join("escape-dir")).unwrap();
        std::fs::hard_link(root.join("outside.txt"), checkout.join("hardlink")).unwrap();
        std::fs::write(checkout.join("large"), vec![0; 512 * 1024 + 1]).unwrap();
        for name in [
            "",
            "../outside.txt",
            "/outside.txt",
            "nested/../nested/read.txt",
            "./nested/read.txt",
            "nested//read.txt",
            "missing",
            "nested",
            "escape",
            "escape-dir/outside.txt",
            "hardlink",
            "large",
            "nul\0path",
        ] {
            assert!(
                tree.read(Path::new(name)).is_err(),
                "unexpected read: {name:?}"
            );
        }
        assert!(tree.still_at(&checkout).unwrap());
        std::fs::rename(&checkout, root.join("old")).unwrap();
        std::fs::create_dir(&checkout).unwrap();
        assert!(!tree.still_at(&checkout).unwrap());
    }
}
