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
    pub(super) fn open(path: &Path, create: bool) -> Result<Self> {
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
    pub(super) fn publish(&self, name: &str, bytes: &[u8]) -> Result<()> {
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
            // linkat refuses replacement, unlike rename; discovery sees complete bytes only.
            if unsafe {
                libc::linkat(
                    self.0.as_raw_fd(),
                    temporary.as_ptr(),
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    0,
                )
            } != 0
            {
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
