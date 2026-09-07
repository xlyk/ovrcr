use super::*;
use anyhow::{Context, Result, bail};
use std::io as std_io;
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn verify_group_identity(pgid: libc::pid_t, allow_reaped_leader: bool) -> Result<()> {
    if pgid <= 1 || pgid == unsafe { libc::getpgrp() } {
        bail!("refusing unsafe process group")
    }
    let actual = unsafe { libc::getpgid(pgid) };
    if actual == pgid {
        return Ok(());
    }
    if allow_reaped_leader
        && actual == -1
        && std_io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        && group_exists(pgid)?
    {
        return Ok(());
    }
    bail!("PTY process group is not owned")
}

#[cfg(not(unix))]
pub(super) fn verify_group_identity(_: libc::pid_t, _: bool) -> Result<()> {
    bail!("OVRCR sessions require Unix process groups")
}

pub(super) fn verify_owned_group(session: &Session) -> Result<()> {
    #[cfg(unix)]
    {
        if !group_exists(session.pgid)? {
            bail!("PTY process group no longer exists")
        }
        verify_group_identity(session.pgid, true)
    }
    #[cfg(not(unix))]
    {
        let _ = session;
        bail!("OVRCR sessions require Unix process groups")
    }
}

pub(super) fn should_signal_group(session: &Session) -> Result<bool> {
    if !group_exists(session.pgid)? {
        return Ok(false);
    }
    match verify_owned_group(session) {
        Ok(()) => Ok(true),
        Err(_error) if !group_exists(session.pgid)? => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn signal_group(pgid: libc::pid_t, signal: libc::c_int) -> Result<bool> {
    if pgid <= 1 || signal <= 0 {
        bail!("refusing unsafe process-group signal")
    }
    let result = unsafe { libc::kill(-pgid, signal) };
    if result == -1 {
        let error = std_io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(error).context("signal PTY process group");
    }
    Ok(true)
}

pub(super) fn group_exists(pgid: libc::pid_t) -> Result<bool> {
    if pgid <= 1 {
        bail!("invalid process group")
    }
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = std_io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(error).context("check PTY process group"),
    }
}

pub(super) fn wait_for_group_exit(pgid: libc::pid_t, deadline: Instant) -> Result<bool> {
    loop {
        if !group_exists(pgid)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}
