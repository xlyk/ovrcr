//! One bounded owner for native Dashboard catalog subprocesses.
//!
//! Callers serialize access through the Server's `CatalogQueue`. If cleanup
//! cannot prove the spawned process group is gone, this owner keeps the exact
//! child and its bounded nonblocking capture pipes; the next catalog or
//! shutdown attempt must resolve it before another process can be spawned.

use anyhow::{Context, Result, bail};
use std::fmt;
use std::io::{self, ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CAPTURE_CHUNK: usize = 16 * 1024;
const CAPTURE_DRAIN_BUDGET: usize = 64 * 1024;
const CLEANUP_GRACE: Duration = Duration::from_secs(2);
const PROCESS_POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(crate) struct CatalogOwnerError(String);

impl fmt::Display for CatalogOwnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogOwnerError {}

pub(crate) fn owner_error(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CatalogOwnerError(message.into()))
}

pub(crate) fn is_owner_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<CatalogOwnerError>().is_some())
}

#[derive(Default)]
pub(crate) struct CatalogProcessOwner {
    state: OwnerState,
    closed: bool,
}

#[derive(Default)]
enum OwnerState {
    #[default]
    Empty,
    Child(OwnedCatalogProcess),
    ReapedGroup(OwnedCatalogProcess),
}

#[derive(Default)]
struct OwnedCatalogProcess {
    child: Option<Child>,
    pid: libc::pid_t,
    label: &'static str,
    capture: CatalogCapture,
}

#[derive(Default)]
pub(crate) struct CatalogCapture {
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    stdout_buffer: Vec<u8>,
    stderr_buffer: Vec<u8>,
    stdout_limit: usize,
    stderr_limit: usize,
    stdout_total: usize,
    stderr_total: usize,
    stdout_overflow: bool,
    stderr_overflow: bool,
    stdout_nonblocking: bool,
    stderr_nonblocking: bool,
}

pub(crate) struct CatalogOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr_total: usize,
}

impl CatalogCapture {
    pub(crate) fn take_from_child(
        child: &mut Child,
        stdout_limit: usize,
        stderr_limit: usize,
    ) -> Self {
        let mut capture = Self {
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            stdout_limit,
            stderr_limit,
            ..Self::default()
        };
        capture.set_nonblocking();
        capture
    }

    pub(crate) fn stdout_mut(&mut self) -> Option<&mut ChildStdout> {
        self.stdout.as_mut()
    }

    pub(crate) fn stderr_mut(&mut self) -> Option<&mut ChildStderr> {
        self.stderr.as_mut()
    }

    pub(crate) fn record_totals(&mut self, stdout_total: usize, stderr_total: usize) {
        self.stdout_total = self.stdout_total.max(stdout_total);
        self.stderr_total = self.stderr_total.max(stderr_total);
        self.stdout_overflow |= self.stdout_total > self.stdout_limit;
        self.stderr_overflow |= self.stderr_total > self.stderr_limit;
    }

    fn set_nonblocking(&mut self) {
        self.stdout_nonblocking = self
            .stdout
            .as_ref()
            .is_none_or(|pipe| set_nonblocking(pipe.as_raw_fd()).is_ok());
        self.stderr_nonblocking = self
            .stderr
            .as_ref()
            .is_none_or(|pipe| set_nonblocking(pipe.as_raw_fd()).is_ok());
    }

    fn all_nonblocking(&self) -> bool {
        self.stdout_nonblocking && self.stderr_nonblocking
    }

    fn pipes_open(&self) -> bool {
        self.stdout.is_some() || self.stderr.is_some()
    }

    fn drain(&mut self) -> Result<usize> {
        let mut drained = 0;
        if self.stdout_nonblocking {
            drained += drain_pipe(
                &mut self.stdout,
                &mut self.stdout_buffer,
                &mut self.stdout_total,
                &mut self.stdout_overflow,
                self.stdout_limit,
            )?;
        }
        if self.stderr_nonblocking {
            drained += drain_pipe(
                &mut self.stderr,
                &mut self.stderr_buffer,
                &mut self.stderr_total,
                &mut self.stderr_overflow,
                self.stderr_limit,
            )?;
        }
        Ok(drained)
    }

    fn has_overflow(&self) -> bool {
        self.stdout_overflow || self.stderr_overflow
    }

    fn into_output(self) -> CatalogOutput {
        CatalogOutput {
            stdout: self.stdout_buffer,
            stderr_total: self.stderr_total,
        }
    }
}

impl CatalogProcessOwner {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }

    pub(crate) fn close(&mut self) {
        self.closed = true;
    }

    pub(crate) fn run_command(
        &mut self,
        label: &'static str,
        command: &mut Command,
        stdout_limit: usize,
        stderr_limit: usize,
        deadline: Duration,
        cancelled: &AtomicBool,
    ) -> Result<CatalogOutput> {
        if self.closed {
            return Err(owner_error("native catalog process owner is closed"));
        }
        self.resolve_existing()?;

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .with_context(|| format!("native {label} model discovery unavailable"))?;
        let pid = child.id() as libc::pid_t;
        let capture = CatalogCapture::take_from_child(&mut child, stdout_limit, stderr_limit);
        let mut process = OwnedCatalogProcess {
            child: Some(child),
            pid,
            label,
            capture,
        };

        let primary = if !process.capture.all_nonblocking() {
            Err(anyhow::anyhow!(
                "native {label} discovery capture pipe unavailable"
            ))
        } else {
            self.wait_for_catalog_exit(label, &mut process, deadline, cancelled)
        };
        let status = match primary {
            Ok(()) => {
                match self.finish_observed_exit(label, &mut process, Instant::now() + CLEANUP_GRACE)
                {
                    Ok(status) => status,
                    Err(error) => {
                        self.retain_after_error(process);
                        return Err(error);
                    }
                }
            }
            Err(error) => {
                let cleanup = self.resolve_process(label, &mut process, CLEANUP_GRACE);
                if let Err(cleanup_error) = cleanup {
                    self.retain_after_error(process);
                    return Err(owner_error(format!(
                        "native {label} discovery failed ({error:#}); process cleanup remains uncertain ({cleanup_error:#})"
                    )));
                }
                return Err(error);
            }
        };

        if !status.success() {
            bail!("native {label} model inventory failed");
        }
        process.capture.drain()?;
        if process.capture.stdout_overflow {
            bail!("native {label} model discovery output bound");
        }
        if process.capture.stderr_overflow {
            bail!("native {label} model discovery diagnostic bound");
        }
        Ok(process.capture.into_output())
    }

    /// Retain a child whose adapter-specific cleanup could not establish a
    /// safe wait. Existing pipes and bounded byte counters move with it.
    pub(crate) fn retain_unresolved_child(
        &mut self,
        label: &'static str,
        child: Child,
        mut capture: CatalogCapture,
    ) -> Result<()> {
        self.ensure_empty_for_adoption()?;
        let pid = child.id() as libc::pid_t;
        capture.set_nonblocking();
        self.state = OwnerState::Child(OwnedCatalogProcess {
            child: Some(child),
            pid,
            label,
            capture,
        });
        Ok(())
    }

    /// Retain the process group after the exact child has already been waited.
    /// This state permits only a read-only absence probe and never a signal.
    pub(crate) fn retain_reaped_group(
        &mut self,
        label: &'static str,
        pid: libc::pid_t,
        mut capture: CatalogCapture,
    ) -> Result<()> {
        self.ensure_empty_for_adoption()?;
        capture.set_nonblocking();
        self.state = OwnerState::ReapedGroup(OwnedCatalogProcess {
            child: None,
            pid,
            label,
            capture,
        });
        Ok(())
    }

    pub(crate) fn resolve_existing(&mut self) -> Result<()> {
        self.resolve_existing_with_budget(CLEANUP_GRACE)
    }

    fn resolve_existing_with_budget(&mut self, budget: Duration) -> Result<()> {
        let state = std::mem::take(&mut self.state);
        match state {
            OwnerState::Empty => Ok(()),
            OwnerState::Child(mut process) => {
                let label = process.label;
                let result = self.resolve_process(label, &mut process, budget);
                if result.is_err() {
                    self.retain_after_error(process);
                }
                result
            }
            OwnerState::ReapedGroup(mut process) => {
                let label = process.label;
                let result = self.resolve_reaped_group(label, &mut process, budget);
                if result.is_err() {
                    self.state = OwnerState::ReapedGroup(process);
                }
                result
            }
        }
    }

    fn wait_for_catalog_exit(
        &mut self,
        label: &'static str,
        process: &mut OwnedCatalogProcess,
        deadline: Duration,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let deadline = Instant::now() + deadline;
        loop {
            process.capture.drain()?;
            if process.capture.has_overflow() {
                bail!("native {label} model discovery output bound");
            }
            if cancelled.load(Ordering::Acquire) {
                bail!("native {label} model discovery cancelled");
            }
            match observe_process(process.pid) {
                Observation::Exited => return Ok(()),
                Observation::Running => {}
                Observation::Unknown(observation) => {
                    report_uncertain(label, process.pid, observation);
                    return Err(owner_error(format!(
                        "native {label} discovery process-group ownership is uncertain"
                    )));
                }
            }
            if Instant::now() >= deadline {
                bail!("native {label} model discovery deadline");
            }
            std::thread::park_timeout(PROCESS_POLL);
        }
    }

    fn resolve_process(
        &mut self,
        label: &'static str,
        process: &mut OwnedCatalogProcess,
        budget: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + budget;
        let mut reported_uncertainty = false;
        loop {
            let _ = process.capture.drain();
            match observe_process(process.pid) {
                Observation::Exited => {
                    self.finish_observed_exit(label, process, deadline)?;
                    return Ok(());
                }
                Observation::Running => {
                    match signal_group(process.pid) {
                        Ok(true) => {}
                        Ok(false) => {
                            return Err(owner_error(format!(
                                "native {label} cleanup group {} disappeared while its leader remained live",
                                process.pid
                            )));
                        }
                        Err(error) => {
                            return Err(owner_error(format!(
                                "native {label} cleanup signal is uncertain for group {} (errno {:?}): {error:#}",
                                process.pid,
                                error.raw_os_error()
                            )));
                        }
                    }
                    match self.wait_after_owned_signal(label, process, deadline) {
                        Ok(()) => return Ok(()),
                        Err(error) if Instant::now() >= deadline => return Err(error),
                        Err(_) => {}
                    }
                }
                Observation::Unknown(observation) => {
                    if !reported_uncertainty {
                        report_uncertain(label, process.pid, observation);
                        reported_uncertainty = true;
                    }
                    match group_absence(process.pid) {
                        GroupAbsence::Absent => {
                            let child = process
                                .child
                                .as_mut()
                                .context("native catalog owner lost its unreaped child handle")?;
                            match child.try_wait() {
                                Ok(Some(_status)) => {
                                    process.child = None;
                                    self.finish_reaped_group_after_absence(
                                        label, process, deadline,
                                    )?;
                                    return Ok(());
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    return Err(owner_error(format!(
                                        "native {label} uncertain child wait failed (pid {}, errno {:?})",
                                        process.pid,
                                        error.raw_os_error()
                                    )));
                                }
                            }
                        }
                        GroupAbsence::Present(_) => {}
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(owner_error(format!(
                    "native {label} process group {} remains owned but unresolved",
                    process.pid
                )));
            }
            std::thread::park_timeout(PROCESS_POLL);
        }
    }

    fn wait_after_owned_signal(
        &mut self,
        label: &'static str,
        process: &mut OwnedCatalogProcess,
        deadline: Instant,
    ) -> Result<()> {
        let mut reported_uncertainty = false;
        loop {
            let _ = process.capture.drain();
            match observe_process(process.pid) {
                Observation::Exited => {
                    self.finish_observed_exit(label, process, deadline)?;
                    return Ok(());
                }
                Observation::Running => {}
                Observation::Unknown(observation) => {
                    if !reported_uncertainty {
                        report_uncertain(label, process.pid, observation);
                        reported_uncertainty = true;
                    }
                    if group_absence(process.pid) == GroupAbsence::Absent {
                        let child = process
                            .child
                            .as_mut()
                            .context("native catalog owner lost its unreaped child handle")?;
                        if child.try_wait().ok().flatten().is_some() {
                            process.child = None;
                            self.finish_reaped_group_after_absence(label, process, deadline)?;
                            return Ok(());
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(owner_error(format!(
                    "native {label} cleanup did not observe leader {} exit",
                    process.pid
                )));
            }
            std::thread::park_timeout(PROCESS_POLL);
        }
    }

    fn finish_observed_exit(
        &mut self,
        label: &'static str,
        process: &mut OwnedCatalogProcess,
        absence_deadline: Instant,
    ) -> Result<ExitStatus> {
        // WNOWAIT proves the exact child is an unreaped exit. Signal only while
        // that leader still reserves the group id; a failed signal is resolved
        // by waiting and a read-only absence probe below.
        let signal_error = signal_group(process.pid).err();
        let child = process
            .child
            .as_mut()
            .context("native catalog owner lost its unreaped child handle")?;
        let status = child
            .wait()
            .with_context(|| format!("native {label} discovery cleanup wait failed"))?;
        process.child = None;
        self.finish_reaped_group_after_absence(label, process, absence_deadline)
            .map_err(|error| {
                owner_error(format!(
                    "native {label} discovery cleanup remains uncertain after wait (pid {}, signal errno {:?}): {error:#}",
                    process.pid,
                    signal_error.as_ref().and_then(io::Error::raw_os_error)
                ))
            })?;
        Ok(status)
    }

    fn finish_reaped_group_after_absence(
        &mut self,
        label: &'static str,
        process: &mut OwnedCatalogProcess,
        deadline: Instant,
    ) -> Result<()> {
        loop {
            let _ = process.capture.drain();
            match group_absence(process.pid) {
                GroupAbsence::Absent => {
                    self.drain_capture_to_eof(label, process, deadline)?;
                    return Ok(());
                }
                GroupAbsence::Present(errno) if Instant::now() >= deadline => {
                    return Err(owner_error(format!(
                        "native {label} waited process group {} remains present or inaccessible (errno {errno:?})",
                        process.pid
                    )));
                }
                GroupAbsence::Present(_) => {
                    std::thread::park_timeout(PROCESS_POLL);
                }
            }
        }
    }

    fn drain_capture_to_eof(
        &mut self,
        label: &'static str,
        process: &mut OwnedCatalogProcess,
        deadline: Instant,
    ) -> Result<()> {
        while process.capture.pipes_open() {
            process.capture.drain()?;
            if !process.capture.pipes_open() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(owner_error(format!(
                    "native {label} catalog capture pipes remain open after group {} absence",
                    process.pid
                )));
            }
            std::thread::park_timeout(PROCESS_POLL);
        }
        Ok(())
    }

    fn resolve_reaped_group(
        &mut self,
        label: &'static str,
        process: &mut OwnedCatalogProcess,
        budget: Duration,
    ) -> Result<()> {
        self.finish_reaped_group_after_absence(label, process, Instant::now() + budget)
    }

    fn retain_after_error(&mut self, process: OwnedCatalogProcess) {
        self.state = if process.child.is_some() {
            OwnerState::Child(process)
        } else {
            OwnerState::ReapedGroup(process)
        };
    }

    fn ensure_empty_for_adoption(&self) -> Result<()> {
        if self.closed {
            return Err(owner_error("native catalog process owner is closed"));
        }
        if matches!(self.state, OwnerState::Empty) {
            Ok(())
        } else {
            Err(owner_error(
                "native catalog process owner already retains an unresolved process",
            ))
        }
    }
}

enum Observation {
    Running,
    Exited,
    Unknown(OwnershipObservation),
}

#[derive(Clone, Copy)]
struct OwnershipObservation {
    observed_group: libc::pid_t,
    group_errno: Option<i32>,
    waitid_errno: Option<i32>,
    checks: u8,
}

fn observe_process(pid: libc::pid_t) -> Observation {
    for checks in 1..=10 {
        match observed_child_exit(pid) {
            Ok(true) => return Observation::Exited,
            Ok(false) => {}
            Err(waitid_errno) => {
                let (observed_group, group_errno) = observed_group(pid);
                return Observation::Unknown(OwnershipObservation {
                    observed_group,
                    group_errno,
                    waitid_errno,
                    checks,
                });
            }
        }
        let (observed_group, group_errno) = observed_group(pid);
        if observed_group == pid {
            return Observation::Running;
        }
        if observed_group == -1 && group_errno == Some(libc::ESRCH) && checks < 10 {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }
        return Observation::Unknown(OwnershipObservation {
            observed_group,
            group_errno,
            waitid_errno: None,
            checks,
        });
    }
    unreachable!("ownership retry loop returns after its tenth check")
}

fn observed_child_exit(pid: libc::pid_t) -> std::result::Result<bool, Option<i32>> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let observed = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as u32,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if observed == -1 {
        return Err(io::Error::last_os_error().raw_os_error());
    }
    let observed_pid = unsafe { info.si_pid() };
    if observed_pid != 0 && observed_pid != pid {
        return Err(None);
    }
    Ok(observed_pid == pid)
}

fn observed_group(pid: libc::pid_t) -> (libc::pid_t, Option<i32>) {
    let observed_group = unsafe { libc::getpgid(pid) };
    let errno = if observed_group == -1 {
        io::Error::last_os_error().raw_os_error()
    } else {
        None
    };
    (observed_group, errno)
}

fn group_absence(pid: libc::pid_t) -> GroupAbsence {
    let result = unsafe { libc::kill(-pid, 0) };
    if result == -1 {
        let errno = io::Error::last_os_error().raw_os_error();
        if errno == Some(libc::ESRCH) {
            return GroupAbsence::Absent;
        }
        return GroupAbsence::Present(errno);
    }
    GroupAbsence::Present(None)
}

#[derive(PartialEq, Eq)]
enum GroupAbsence {
    Absent,
    Present(Option<i32>),
}

fn signal_group(pid: libc::pid_t) -> io::Result<bool> {
    let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(false);
    }
    Err(error)
}

fn report_uncertain(label: &str, pid: libc::pid_t, observation: OwnershipObservation) {
    eprintln!(
        "ovrcr: native {label} discovery ownership uncertain: pid={pid} observed_group={} getpgid_errno={:?} waitid_errno={:?} checks={}",
        observation.observed_group,
        observation.group_errno,
        observation.waitid_errno,
        observation.checks
    );
}

fn drain_pipe(
    pipe: &mut Option<impl Read>,
    buffer: &mut Vec<u8>,
    total: &mut usize,
    overflow: &mut bool,
    limit: usize,
) -> Result<usize> {
    let Some(reader) = pipe.as_mut() else {
        return Ok(0);
    };
    let mut chunk = [0u8; CAPTURE_CHUNK];
    let mut drained = 0;
    loop {
        if drained >= CAPTURE_DRAIN_BUDGET {
            return Ok(drained);
        }
        match reader.read(&mut chunk) {
            Ok(0) => {
                *pipe = None;
                return Ok(drained);
            }
            Ok(count) => {
                drained += count;
                *total = total.saturating_add(count);
                let room = limit.saturating_sub(buffer.len());
                let retained = room.min(count);
                buffer.extend_from_slice(&chunk[..retained]);
                *overflow |= retained < count;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(drained),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn set_nonblocking(fd: libc::c_int) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::thread;

    fn wait_for_path(path: &std::path::Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        path.exists()
    }

    fn pid_absent(pid: libc::pid_t) -> bool {
        let result = unsafe { libc::kill(pid, 0) };
        result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn group_present(pid: libc::pid_t) -> bool {
        let result = unsafe { libc::kill(-pid, 0) };
        result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[test]
    fn owner_errors_are_classified_without_exposing_process_output() {
        let error = owner_error("native catalog process group remains unresolved");
        assert!(is_owner_error(&error));
        assert!(error.to_string().contains("process group"));
        assert!(!error.to_string().contains("argv"));
    }

    #[test]
    fn bounded_pipe_capture_keeps_complete_output_across_drain_iterations() {
        let mut owner = CatalogProcessOwner::default();
        let mut command = Command::new("/usr/bin/head");
        command.args(["-c", "196608", "/dev/zero"]);
        let output = owner
            .run_command(
                "synthetic",
                &mut command,
                256 * 1024,
                64 * 1024,
                Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(output.stdout.len(), 196_608);
        assert_eq!(output.stderr_total, 0);
        assert!(matches!(owner.state, OwnerState::Empty));
    }

    #[test]
    fn unknown_nonmatching_child_blocks_repeated_spawns_until_it_is_waited() {
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("ready");
        let release = root.path().join("release");
        let unexpected_spawn = root.path().join("unexpected-spawn");
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                &format!(
                    "printf ready > '{}'; while [ ! -e '{}' ]; do /bin/sleep 0.01; done",
                    ready.display(),
                    release.display()
                ),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let capture = CatalogCapture::take_from_child(&mut child, 1024, 1024);
        if !wait_for_path(&ready) || unsafe { libc::getpgid(pid) } == pid {
            let _ = std::fs::write(&release, "release\n");
            let _ = child.wait();
            panic!("synthetic child did not reach the nonmatching process group");
        }

        let mut owner = CatalogProcessOwner::default();
        let adopted = owner.retain_unresolved_child("synthetic", child, capture);
        let first_refused = owner
            .resolve_existing_with_budget(Duration::from_millis(100))
            .is_err();
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("printf spawned > '{}'", unexpected_spawn.display()));
        let second_refused = owner
            .run_command(
                "synthetic",
                &mut command,
                1024,
                1024,
                Duration::from_secs(1),
                &AtomicBool::new(false),
            )
            .is_err();
        let shutdown_refused = owner
            .resolve_existing_with_budget(Duration::from_millis(100))
            .is_err();
        let no_replacement_spawned = !unexpected_spawn.exists();

        std::fs::write(&release, "release\n").unwrap();
        let recovered = owner.resolve_existing().is_ok();
        let waited_child_absent = pid_absent(pid);
        let process_group_absent = !group_present(pid);

        assert!(adopted.is_ok());
        assert!(first_refused, "unknown ownership must remain unresolved");
        assert!(
            second_refused,
            "a repeated request must not spawn another child"
        );
        assert!(
            shutdown_refused,
            "shutdown cleanup attempt stays bounded and refuses"
        );
        assert!(no_replacement_spawned);
        assert!(
            recovered,
            "a later positive exit observation permits recovery"
        );
        assert!(waited_child_absent, "the exact owned child must be waited");
        assert!(process_group_absent, "the exact owned group must be absent");
        assert!(matches!(owner.state, OwnerState::Empty));
    }

    #[test]
    fn reaped_group_uncertainty_is_read_only_until_descendant_exit() {
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("descendant-ready");
        let release = root.path().join("descendant-release");
        let script = "(/bin/sh -c 'printf ready > \"$READY\"; while [ ! -e \"$RELEASE\" ]; do /bin/sleep 0.01; done') & exit 0";
        let mut child = Command::new("/bin/sh")
            .args(["-c", script])
            .env("READY", &ready)
            .env("RELEASE", &release)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let capture = CatalogCapture::take_from_child(&mut child, 1024, 1024);
        let waited = child.wait().is_ok();
        let descendant_started = wait_for_path(&ready);
        let mut owner = CatalogProcessOwner::default();
        let retained = owner.retain_reaped_group("synthetic", pid, capture);
        let unresolved = owner
            .resolve_existing_with_budget(Duration::from_millis(100))
            .is_err();
        let descendant_still_present = group_present(pid);

        std::fs::write(&release, "release\n").unwrap();
        let recovered = owner.resolve_existing().is_ok();
        let group_absent = !group_present(pid);

        assert!(waited, "the exact leader must already be waited");
        assert!(descendant_started, "the held descendant did not start");
        assert!(retained.is_ok());
        assert!(unresolved, "the group-only state must refuse while present");
        assert!(
            descendant_still_present,
            "group-only recovery must not signal after leader reap"
        );
        assert!(
            recovered,
            "read-only absence proof permits eventual recovery"
        );
        assert!(group_absent);
        assert!(pid_absent(pid), "the waited leader must remain absent");
    }
}
