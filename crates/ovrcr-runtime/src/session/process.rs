use super::*;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::io as std_io;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
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

/// Normalize a terminal device path to the short form `ps` prints in its
/// `tty` column (`ttys003` on macOS, `pts/3` on Linux).
pub(super) fn short_tty_name(path: &std::path::Path) -> Option<String> {
    let text = path.to_str()?;
    Some(text.strip_prefix("/dev/").unwrap_or(text).to_owned())
}

/// Process groups other than `leader` whose controlling terminal is `tty`.
///
/// Interactive shells put each job in its own process group, so signalling
/// only the leader's group leaves background jobs behind. Ownership is
/// defined by the controlling terminal: anything still attached to the
/// session's PTY belongs to the session; anything that called `setsid` does
/// not. `ps -t` filters in the kernel, so this costs well under a
/// millisecond. A failure to run `ps` yields an empty set so termination can
/// still proceed against the leader's group.
pub(super) fn attached_groups(tty: &str, leader: libc::pid_t) -> BTreeSet<libc::pid_t> {
    attached_groups_checked(tty, leader).unwrap_or_default()
}

pub(super) fn attached_groups_checked(
    tty: &str,
    leader: libc::pid_t,
) -> Result<BTreeSet<libc::pid_t>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,tty="])
        .output()
        .context("list processes attached to session PTY")?;
    if !output.status.success() {
        bail!("ps could not list processes attached to session PTY");
    }
    let mut processes = Vec::new();
    let process_lines = String::from_utf8_lossy(&output.stdout);
    for line in process_lines.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(pgid), Some(process_tty)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(pid), Ok(ppid), Ok(pgid)) = (
            pid.parse::<libc::pid_t>(),
            ppid.parse::<libc::pid_t>(),
            pgid.parse::<libc::pid_t>(),
        ) else {
            continue;
        };
        processes.push((pid, ppid, pgid, process_tty));
    }
    let mut descendants = BTreeSet::from([leader]);
    loop {
        let before = descendants.len();
        for (pid, ppid, _, _) in &processes {
            if descendants.contains(ppid) {
                descendants.insert(*pid);
            }
        }
        if descendants.len() == before {
            break;
        }
    }
    let own_group = unsafe { libc::getpgrp() };
    let mut groups = BTreeSet::new();
    for (pid, _, pgid, process_tty) in processes {
        if process_tty != tty && !descendants.contains(&pid) {
            continue;
        }
        if pgid <= 1 || pgid == leader || pgid == own_group {
            continue;
        }
        groups.insert(pgid);
    }
    Ok(groups)
}

/// Deliver `signal` to every group in `groups`, tolerating groups that have
/// already disappeared and refusing groups that are no longer owned.
pub(super) fn signal_attached_groups(groups: &BTreeSet<libc::pid_t>, signal: libc::c_int) {
    for pgid in groups {
        if verify_group_identity(*pgid, true).is_ok() {
            let _ = signal_group(*pgid, signal);
        }
    }
}
