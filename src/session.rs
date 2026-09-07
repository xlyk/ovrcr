use crate::context::{ContextUsageSnapshot, validate_context};
use crate::protocol::{AgentReport, AgentUpdate};
use anyhow::{Context, Result, bail};
use ovrcr_terminal::{encode_paste, vt100};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, mpsc::SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use ovrcr_protocol::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};

#[derive(Clone, Debug)]
pub struct SessionSpec {
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: String,
    pub cwd: PathBuf,
    pub argv: Vec<OsString>,
    pub hook_env: Option<HookEnvironment>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct HookEnvironment {
    pub socket: PathBuf,
    pub session: SessionId,
    pub capability: [u8; 32],
}

impl std::fmt::Debug for HookEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HookEnvironment { redacted: [redacted] }")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputAdmissionError {
    Paused,
    Exited,
}

impl std::fmt::Display for InputAdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Paused => "session is paused; resume it before sending input",
            Self::Exited => "session has exited",
        })
    }
}

impl std::error::Error for InputAdmissionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReportOrder {
    #[default]
    Unset,
    Receipt,
    Sequenced(u64),
}

impl ReportOrder {
    pub fn accept(&mut self, incoming: Option<u64>) -> Result<()> {
        let next = match (*self, incoming) {
            (Self::Unset, None) => Self::Receipt,
            (Self::Unset, Some(sequence)) if sequence > 0 => Self::Sequenced(sequence),
            (Self::Receipt, None) => Self::Receipt,
            (Self::Sequenced(previous), Some(sequence)) if sequence > previous => {
                Self::Sequenced(sequence)
            }
            _ => bail!("agent report sequence is invalid for the current ordering mode"),
        };
        *self = next;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Output { id: SessionId, bytes: Vec<u8> },
    Exited { id: SessionId, phase: SessionPhase },
}

struct SessionState {
    phase: SessionPhase,
    pid: Option<u32>,
    activity: AgentActivity,
    hook_capability: Option<[u8; 32]>,
    activity_order: ReportOrder,
    context_usage: Option<ContextUsageSnapshot>,
    context_order: ReportOrder,
}

struct JoinHandles {
    reader: Option<JoinHandle<()>>,
    waiter: Option<JoinHandle<()>>,
}

pub struct Session {
    summary: SessionSummary,
    state: Mutex<SessionState>,
    state_changed: Condvar,
    parser: Mutex<vt100::Parser>,
    parser_revision: Mutex<u64>,
    parser_changed: Condvar,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    pgid: libc::pid_t,
    #[cfg(test)]
    signal_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    signal_result_hook: Option<Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>>,
    terminate_lock: Mutex<()>,
    reader_done: Mutex<bool>,
    reader_changed: Condvar,
    handles: Mutex<JoinHandles>,
}

impl Session {
    pub fn spawn(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: SyncSender<SessionEvent>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(id, spec, size, events, None, None, None, None)
    }

    pub(crate) fn spawn_with_ready(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: SyncSender<SessionEvent>,
        ready: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(id, spec, size, events, Some(ready), None, None, None)
    }

    #[cfg(test)]
    pub(crate) fn spawn_with_test_hooks(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: SyncSender<SessionEvent>,
        reap_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_result_hook: Option<Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(
            id,
            spec,
            size,
            events,
            None,
            reap_hook,
            signal_hook,
            signal_result_hook,
        )
    }

    fn spawn_internal(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: SyncSender<SessionEvent>,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
        reap_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_result_hook: Option<Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>>,
    ) -> Result<Arc<Self>> {
        #[cfg(not(test))]
        let _ = &signal_hook;
        #[cfg(not(test))]
        let _ = &signal_result_hook;

        let argv0 = spec
            .argv
            .first()
            .context("session command cannot be empty")?
            .clone();
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(argv0);
        command.args(spec.argv.iter().skip(1));
        command.cwd(spec.cwd);
        command.env_remove("OVRCR_HOOK_SOCKET");
        command.env_remove("OVRCR_SESSION_ID");
        command.env_remove("OVRCR_HOOK_TOKEN");
        if let Some(hook_env) = &spec.hook_env {
            command.env("OVRCR_HOOK_SOCKET", &hook_env.socket);
            command.env("OVRCR_SESSION_ID", hook_env.session.0.to_string());
            command.env("OVRCR_HOOK_TOKEN", capability_hex(&hook_env.capability));
        }
        let mut child = pair.slave.spawn_command(command)?;
        let pid = child.process_id().context("PTY child has no process ID")?;

        #[cfg(unix)]
        let pgid = pair
            .master
            .process_group_leader()
            .context("PTY did not provide a process-group leader")?;
        #[cfg(not(unix))]
        let pgid = {
            let _ = pid;
            bail!("OVRCR sessions require Unix process groups")
        };

        if pgid != pid as libc::pid_t {
            let _ = child.kill();
            bail!("PTY process-group leader does not own the child process");
        }
        verify_group_identity(pgid, false)?;

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let started_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let session = Arc::new(Self {
            summary: SessionSummary {
                id,
                project: spec.project,
                workspace: spec.workspace,
                name: spec.name,
                label: spec.label,
                pid: Some(pid),
                started_unix_ms,
                phase: SessionPhase::Running,
                activity: AgentActivity::Unknown,
                context_usage: None,
            },
            state: Mutex::new(SessionState {
                phase: SessionPhase::Running,
                pid: Some(pid),
                activity: AgentActivity::Unknown,
                hook_capability: spec.hook_env.as_ref().map(|hook| hook.capability),
                activity_order: ReportOrder::default(),
                context_usage: None,
                context_order: ReportOrder::default(),
            }),
            state_changed: Condvar::new(),
            parser: Mutex::new(vt100::Parser::new(size.rows, size.cols, 0)),
            parser_revision: Mutex::new(0),
            parser_changed: Condvar::new(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            pgid,
            terminate_lock: Mutex::new(()),
            reader_done: Mutex::new(false),
            reader_changed: Condvar::new(),
            handles: Mutex::new(JoinHandles {
                reader: None,
                waiter: None,
            }),
            #[cfg(test)]
            signal_hook,
            #[cfg(test)]
            signal_result_hook,
        });

        let reader_session = Arc::clone(&session);
        let reader_events = events.clone();
        let reader_handle = thread::Builder::new()
            .name(format!("ovrcr-session-reader-{id:?}"))
            .spawn(move || {
                read_pty(reader, reader_session, reader_events);
            })?;

        let waiter_session = Arc::clone(&session);
        let waiter_events = events;
        let waiter_reap_hook = reap_hook;
        let waiter_handle = thread::Builder::new()
            .name(format!("ovrcr-session-waiter-{id:?}"))
            .spawn(move || {
                if let Some(reap_hook) = waiter_reap_hook {
                    reap_hook();
                }
                wait_for_child(&mut *child, waiter_session, waiter_events);
            })?;
        {
            let mut handles = session.handles.lock().unwrap();
            handles.reader = Some(reader_handle);
            handles.waiter = Some(waiter_handle);
        }
        if let Some(ready) = ready {
            ready();
        }
        Ok(session)
    }

    pub fn summary(&self) -> SessionSummary {
        let state = self.state.lock().unwrap();
        let mut summary = self.summary.clone();
        summary.phase = state.phase.clone();
        summary.pid = state.pid;
        summary.activity = state.activity;
        summary.context_usage = state.context_usage.clone();
        summary
    }

    pub fn set_paused(&self, paused: bool) -> Result<bool> {
        let _control = self.terminate_lock.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        if matches!(state.phase, SessionPhase::Exited { .. }) {
            bail!("session has exited");
        }
        verify_owned_group(self)?;
        let signal = if paused { libc::SIGSTOP } else { libc::SIGCONT };
        if !signal_group(self.pgid, signal)? {
            bail!("PTY process group no longer exists");
        }
        let next = if paused {
            SessionPhase::Paused
        } else {
            SessionPhase::Running
        };
        let changed = state.phase != next;
        state.phase = next;
        self.state_changed.notify_all();
        Ok(changed)
    }

    fn admit_input(&self) -> std::result::Result<(), InputAdmissionError> {
        let phase = self.state.lock().unwrap().phase.clone();
        match phase {
            SessionPhase::Running => Ok(()),
            SessionPhase::Paused => Err(InputAdmissionError::Paused),
            SessionPhase::Exited { .. } => Err(InputAdmissionError::Exited),
        }
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.admit_input().map_err(anyhow::Error::new)?;
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes).context("write to PTY")?;
        writer.flush().context("flush PTY")?;
        Ok(())
    }

    pub fn resize(&self, size: TerminalSize) -> Result<()> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize PTY")?;
        self.parser
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(size.rows, size.cols);
        Ok(())
    }

    pub fn apply_event(&self, event: SessionEvent) {
        match event {
            SessionEvent::Output { id, bytes } if id == self.summary.id => {
                self.parser.lock().unwrap().process(&bytes);
                let mut revision = self.parser_revision.lock().unwrap();
                *revision = revision.saturating_add(1);
                self.parser_changed.notify_all();
            }
            SessionEvent::Exited { id, phase } if id == self.summary.id => {
                let mut state = self.state.lock().unwrap();
                state.phase = phase;
                state.pid = None;
                state.activity = AgentActivity::Unknown;
                state.hook_capability = None;
                self.state_changed.notify_all();
            }
            _ => {}
        }
    }

    pub fn apply_agent_report(&self, report: &AgentReport) -> Result<bool> {
        let mut state = self.state.lock().unwrap();
        if report.session != self.summary.id
            || state.hook_capability.as_ref() != Some(&report.capability)
            || matches!(state.phase, SessionPhase::Exited { .. })
        {
            bail!("agent report rejected");
        }
        match &report.update {
            AgentUpdate::Activity(activity) => {
                let mut next_order = state.activity_order;
                next_order.accept(report.sequence)?;
                let changed = state.activity != *activity;
                state.activity_order = next_order;
                state.activity = *activity;
                Ok(changed)
            }
            AgentUpdate::Context(context) => {
                validate_context(context)?;
                let mut next_order = state.context_order.clone();
                next_order.accept(report.sequence)?;
                let snapshot = ContextUsageSnapshot {
                    report: context.clone(),
                    received_unix_ms: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                };
                let changed = state.context_usage.as_ref() != Some(&snapshot);
                state.context_order = next_order;
                state.context_usage = Some(snapshot);
                Ok(changed)
            }
        }
    }

    pub fn revoke_hook_capability(&self) {
        self.state.lock().unwrap().hook_capability = None;
    }

    pub fn current_screen(&self) -> Vec<u8> {
        self.parser.lock().unwrap().screen().state_formatted()
    }

    pub fn terminal_text(&self) -> (TerminalSize, String) {
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        (TerminalSize { rows, cols }, screen.contents())
    }

    pub fn send_text(&self, text: &str, submit: bool) -> Result<()> {
        self.admit_input().map_err(anyhow::Error::new)?;
        let bytes = {
            let parser = self.parser.lock().unwrap();
            encode_paste(text, parser.screen().bracketed_paste())
        };
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(&bytes).context("write text to PTY")?;
        if submit {
            writer.write_all(b"\r").context("submit text to PTY")?;
        }
        writer.flush().context("flush PTY")?;
        Ok(())
    }

    pub fn wait_for_output(&self, timeout: Duration) {
        let revision = self.parser_revision.lock().unwrap();
        if *revision == 0 {
            let _ = self.parser_changed.wait_timeout(revision, timeout);
        }
    }

    pub fn terminate(&self, grace: Duration) -> Result<()> {
        let _termination = self.terminate_lock.lock().unwrap();
        if matches!(
            self.state.lock().unwrap().phase,
            SessionPhase::Exited { .. }
        ) {
            self.join_threads()?;
            return Ok(());
        }

        let mut signal_error = None;
        if should_signal_group(self)? {
            signal_group(self.pgid, libc::SIGTERM)?;
            if should_signal_group(self)? {
                #[cfg(test)]
                if let Some(signal_hook) = &self.signal_hook {
                    signal_hook();
                }
                let signal_result = signal_group(self.pgid, libc::SIGCONT);
                #[cfg(test)]
                let signal_result = if let Some(signal_result_hook) = &self.signal_result_hook {
                    match signal_result_hook() {
                        Some(error) => Err(error),
                        None => signal_result,
                    }
                } else {
                    signal_result
                };
                match signal_result {
                    Ok(true) => {
                        let mut state = self.state.lock().unwrap();
                        if matches!(state.phase, SessionPhase::Paused) {
                            state.phase = SessionPhase::Running;
                            self.state_changed.notify_all();
                        }
                    }
                    Ok(false) => {}
                    Err(error) => signal_error = Some(error),
                }
            }
        }
        let deadline = Instant::now() + grace;
        let group_exited = match wait_for_group_exit(self.pgid, deadline) {
            Ok(exited) => exited,
            Err(error) => return Err(signal_error.unwrap_or(error)),
        };
        let group_present = match group_exists(self.pgid) {
            Ok(present) => present,
            Err(error) => return Err(signal_error.unwrap_or(error)),
        };
        if !group_exited && group_present {
            if let Some(error) = signal_error {
                return Err(error);
            }
            let should_kill = match should_signal_group(self) {
                Ok(should_kill) => should_kill,
                Err(error) => return Err(signal_error.unwrap_or(error)),
            };
            if should_kill {
                if let Err(error) = signal_group(self.pgid, libc::SIGKILL) {
                    return Err(signal_error.unwrap_or(error));
                }
            }
            let kill_deadline = Instant::now() + grace.max(Duration::from_secs(2));
            let kill_exited = match wait_for_group_exit(self.pgid, kill_deadline) {
                Ok(exited) => exited,
                Err(error) => return Err(signal_error.unwrap_or(error)),
            };
            if !kill_exited {
                return Err(signal_error.unwrap_or_else(|| {
                    anyhow::anyhow!("PTY process group did not exit after SIGKILL")
                }));
            }
        }

        let phase = match self.wait_until_exited(grace.max(Duration::from_secs(2))) {
            Ok(phase) => phase,
            Err(error) => return Err(signal_error.unwrap_or(error)),
        };
        if !matches!(phase, SessionPhase::Exited { .. }) {
            return Err(signal_error
                .unwrap_or_else(|| anyhow::anyhow!("session did not publish its exit event")));
        }
        if let Err(error) = self.join_threads() {
            return Err(signal_error.unwrap_or(error));
        }
        match group_exists(self.pgid) {
            Ok(false) => {}
            Ok(true) => {
                return Err(signal_error.unwrap_or_else(|| {
                    anyhow::anyhow!("PTY process group still exists after termination")
                }));
            }
            Err(error) => return Err(signal_error.unwrap_or(error)),
        }
        Ok(())
    }

    pub fn wait_until_exited(&self, timeout: Duration) -> Result<SessionPhase> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        loop {
            if let SessionPhase::Exited { .. } = state.phase {
                return Ok(state.phase.clone());
            }
            let now = Instant::now();
            if now >= deadline {
                bail!("timed out waiting for session exit")
            }
            let (next, result) = self
                .state_changed
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .unwrap();
            state = next;
            if result.timed_out() {
                bail!("timed out waiting for session exit")
            }
        }
    }

    fn join_threads(&self) -> Result<()> {
        let (reader, waiter) = {
            let mut handles = self.handles.lock().unwrap();
            (handles.reader.take(), handles.waiter.take())
        };
        if let Some(handle) = reader {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("PTY reader panicked"))?;
        }
        if let Some(handle) = waiter {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("child waiter panicked"))?;
        }
        Ok(())
    }
}

fn capability_hex(capability: &[u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in capability {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn read_pty(
    mut reader: Box<dyn Read + Send>,
    session: Arc<Session>,
    events: SyncSender<SessionEvent>,
) {
    let mut buffer = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        if events
            .send(SessionEvent::Output {
                id: session.summary.id,
                bytes: buffer[..read].to_vec(),
            })
            .is_err()
        {
            break;
        }
    }
    let mut done = session.reader_done.lock().unwrap();
    *done = true;
    session.reader_changed.notify_all();
}

fn wait_for_child(
    child: &mut (dyn portable_pty::Child + Send + Sync),
    session: Arc<Session>,
    events: SyncSender<SessionEvent>,
) {
    let phase = match child.wait() {
        Ok(status) => SessionPhase::Exited {
            code: Some(status.exit_code()),
            signal: status.signal().map(str::to_owned),
        },
        Err(error) => SessionPhase::Exited {
            code: None,
            signal: Some(error.to_string()),
        },
    };
    let mut reader_done = session.reader_done.lock().unwrap();
    while !*reader_done {
        reader_done = session.reader_changed.wait(reader_done).unwrap();
    }
    drop(reader_done);
    while group_exists(session.pgid).unwrap_or(true) {
        thread::park_timeout(Duration::from_millis(5));
    }
    let _ = events.send(SessionEvent::Exited {
        id: session.summary.id,
        phase,
    });
}

#[cfg(unix)]
fn verify_group_identity(pgid: libc::pid_t, allow_reaped_leader: bool) -> Result<()> {
    if pgid <= 1 || pgid == unsafe { libc::getpgrp() } {
        bail!("refusing unsafe process group")
    }
    let actual = unsafe { libc::getpgid(pgid) };
    if actual == pgid {
        return Ok(());
    }
    if allow_reaped_leader
        && actual == -1
        && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        && group_exists(pgid)?
    {
        return Ok(());
    }
    bail!("PTY process group is not owned")
}

#[cfg(not(unix))]
fn verify_group_identity(_: libc::pid_t, _: bool) -> Result<()> {
    bail!("OVRCR sessions require Unix process groups")
}

fn verify_owned_group(session: &Session) -> Result<()> {
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

fn should_signal_group(session: &Session) -> Result<bool> {
    if !group_exists(session.pgid)? {
        return Ok(false);
    }
    match verify_owned_group(session) {
        Ok(()) => Ok(true),
        Err(_error) if !group_exists(session.pgid)? => Ok(false),
        Err(error) => Err(error),
    }
}

fn signal_group(pgid: libc::pid_t, signal: libc::c_int) -> Result<bool> {
    if pgid <= 1 || signal <= 0 {
        bail!("refusing unsafe process-group signal")
    }
    let result = unsafe { libc::kill(-pgid, signal) };
    if result == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(error).context("signal PTY process group");
    }
    Ok(true)
}

fn group_exists(pgid: libc::pid_t) -> Result<bool> {
    if pgid <= 1 {
        bail!("invalid process group")
    }
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(error).context("check PTY process group"),
    }
}

fn wait_for_group_exit(pgid: libc::pid_t, deadline: Instant) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use std::process::Command;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver};

    struct TerminationGuard(Arc<Session>);

    impl TerminationGuard {
        fn force_cleanup(&self) -> Result<()> {
            let pgid = self.0.pgid;
            if !group_exists(pgid)? {
                return Ok(());
            }
            match verify_owned_group(&self.0) {
                Ok(()) => {}
                Err(_error) if !group_exists(pgid)? => return Ok(()),
                Err(error) => return Err(error),
            }
            let _ = signal_group(pgid, libc::SIGTERM)?;
            let _ = signal_group(pgid, libc::SIGCONT)?;
            if wait_for_group_exit(pgid, Instant::now() + Duration::from_secs(2))? {
                return Ok(());
            }
            if !group_exists(pgid)? {
                return Ok(());
            }
            verify_owned_group(&self.0)?;
            let _ = signal_group(pgid, libc::SIGKILL)?;
            if !wait_for_group_exit(pgid, Instant::now() + Duration::from_secs(2))? {
                bail!("test cleanup could not remove PTY process group {pgid}");
            }
            Ok(())
        }
    }

    impl Drop for TerminationGuard {
        fn drop(&mut self) {
            let result = if thread::panicking() {
                self.force_cleanup()
            } else {
                self.0.terminate(Duration::from_millis(200))
            };
            if let Err(error) = result {
                eprintln!(
                    "session test cleanup failed for process group {}: {error}",
                    self.0.pgid
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    struct ReapGate {
        entered: mpsc::SyncSender<()>,
        release: Mutex<Receiver<()>>,
        signal_entered: mpsc::SyncSender<()>,
        signal_release: Mutex<Receiver<()>>,
        signal_result_entered: mpsc::SyncSender<()>,
    }

    #[cfg(target_os = "macos")]
    impl ReapGate {
        fn new() -> (
            Arc<Self>,
            Receiver<()>,
            mpsc::SyncSender<()>,
            Receiver<()>,
            mpsc::SyncSender<()>,
            Receiver<()>,
        ) {
            let (entered_sender, entered) = mpsc::sync_channel(1);
            let (release, release_receiver) = mpsc::sync_channel(1);
            let (signal_entered_sender, signal_entered) = mpsc::sync_channel(1);
            let (signal_release, signal_release_receiver) = mpsc::sync_channel(1);
            let (signal_result_entered_sender, signal_result_entered) = mpsc::sync_channel(1);
            (
                Arc::new(Self {
                    entered: entered_sender,
                    release: Mutex::new(release_receiver),
                    signal_entered: signal_entered_sender,
                    signal_release: Mutex::new(signal_release_receiver),
                    signal_result_entered: signal_result_entered_sender,
                }),
                entered,
                release,
                signal_entered,
                signal_release,
                signal_result_entered,
            )
        }

        fn wait(&self) {
            let _ = self.entered.send(());
            let _ = self.release.lock().unwrap().recv();
        }

        fn before_signal(&self) {
            let _ = self.signal_entered.send(());
            let _ = self.signal_release.lock().unwrap().recv();
        }

        fn after_signal(&self) -> Option<anyhow::Error> {
            let _ = self.signal_result_entered.send(());
            None
        }
    }

    #[cfg(target_os = "macos")]
    struct ReapGateCleanup {
        release: mpsc::SyncSender<()>,
        signal_release: mpsc::SyncSender<()>,
    }

    #[cfg(target_os = "macos")]
    impl Drop for ReapGateCleanup {
        fn drop(&mut self) {
            let _ = self.signal_release.try_send(());
            let _ = self.release.try_send(());
        }
    }

    struct CancellableBarrier {
        state: Mutex<CancellableBarrierState>,
        changed: Condvar,
    }

    struct CancellableBarrierState {
        waiting: usize,
        generation: u64,
        cancelled: bool,
    }

    impl CancellableBarrier {
        fn new() -> Self {
            Self {
                state: Mutex::new(CancellableBarrierState {
                    waiting: 0,
                    generation: 0,
                    cancelled: false,
                }),
                changed: Condvar::new(),
            }
        }

        fn wait(&self) -> bool {
            let mut state = self.state.lock().unwrap();
            if state.cancelled {
                return false;
            }
            let generation = state.generation;
            state.waiting += 1;
            if state.waiting == 2 {
                state.waiting = 0;
                state.generation = state.generation.wrapping_add(1);
                self.changed.notify_all();
                return true;
            }
            while !state.cancelled && state.generation == generation {
                state = self.changed.wait(state).unwrap();
            }
            !state.cancelled
        }

        fn cancel(&self) {
            let mut state = self.state.lock().unwrap();
            state.cancelled = true;
            self.changed.notify_all();
        }
    }

    struct BarrierGuard(Arc<CancellableBarrier>);

    impl Drop for BarrierGuard {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }

    fn spawn_test_shell() -> Arc<Session> {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(1),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "local".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![OsString::from("sh")],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let _ = Box::leak(Box::new(dir));
        let _dispatcher = dispatch_test_events(session.clone(), rx);
        session
    }

    fn dispatch_test_events(session: Arc<Session>, rx: Receiver<SessionEvent>) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let exited = matches!(event, SessionEvent::Exited { .. });
                session.apply_event(event);
                if exited {
                    break;
                }
            }
        })
    }

    fn wait_for_screen(session: &Session, marker: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&session.current_screen()).contains(marker) {
                return true;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        false
    }

    #[cfg(target_os = "macos")]
    fn wait_for_process_state(pid: u32, expected: char, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let pid = pid.to_string();
        while Instant::now() < deadline {
            if let Ok(output) = Command::new("ps")
                .args(["-o", "stat=", "-p", &pid])
                .output()
            {
                let state = String::from_utf8_lossy(&output.stdout);
                if state.trim_start().starts_with(expected) {
                    return true;
                }
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        false
    }

    #[cfg(target_os = "macos")]
    fn spawn_term_race_session(reap_gate: Arc<ReapGate>) -> (Arc<Session>, JoinHandle<()>) {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let signal_gate = Arc::clone(&reap_gate);
        let signal_result_gate = Arc::clone(&reap_gate);
        let session = Session::spawn_with_test_hooks(
            SessionId(50),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "term-race".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "trap 'exit 0' TERM; printf READY; while :; do read line; done".into(),
                ],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
            Some(Arc::new(move || reap_gate.wait())),
            Some(Arc::new(move || signal_gate.before_signal())),
            Some(Arc::new(move || signal_result_gate.after_signal())),
        )
        .unwrap();
        let _ = Box::leak(Box::new(dir));
        let dispatcher = dispatch_test_events(session.clone(), rx);
        (session, dispatcher)
    }

    fn spawn_live_refusal_session(
        signal_result_hook: Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>,
    ) -> (Arc<Session>, JoinHandle<()>) {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn_with_test_hooks(
            SessionId(51),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "live-refusal".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "trap '' TERM; printf READY; while :; do read line; done".into(),
                ],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
            None,
            None,
            Some(signal_result_hook),
        )
        .unwrap();
        let _ = Box::leak(Box::new(dir));
        let dispatcher = dispatch_test_events(session.clone(), rx);
        (session, dispatcher)
    }

    #[test]
    fn pause_resume_phase_and_input_admission() {
        let session = spawn_test_shell();
        let _cleanup = TerminationGuard(Arc::clone(&session));
        session.write(b"printf 'REA%s\\n' DY\r").unwrap();
        assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));

        assert!(session.set_paused(true).unwrap());
        assert_eq!(session.summary().phase, SessionPhase::Paused);
        assert!(!session.set_paused(true).unwrap());
        assert!(session.write(b"SHOULD_NOT_BE_ACCEPTED\r").is_err());
        assert!(session.send_text("SHOULD_NOT_BE_ACCEPTED", true).is_err());
        assert!(session.set_paused(false).unwrap());
        assert!(!session.set_paused(false).unwrap());
        session.write(b"printf 'RESUME_%s\\n' ACK\r").unwrap();
        assert!(wait_for_screen(
            &session,
            "RESUME_ACK",
            Duration::from_secs(2)
        ));
        session.terminate(Duration::from_millis(200)).unwrap();
        assert!(session.set_paused(true).is_err());
        assert!(session.set_paused(false).is_err());
    }

    #[test]
    fn agent_report_order_rejects_zero_sequence_without_mutating_unset() {
        let mut order = ReportOrder::default();
        assert!(order.accept(Some(0)).is_err());
        assert_eq!(order, ReportOrder::Unset);
    }

    #[test]
    fn agent_report_requires_capability_and_preserves_order_on_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let (events, receiver) = mpsc::sync_channel(64);
        let capability = [0x37; 32];
        let session = Session::spawn(
            SessionId(700),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "agent".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "while IFS= read -r line; do :; done".into(),
                ],
                hook_env: Some(HookEnvironment {
                    socket: PathBuf::from("/private/test/ovrcr.sock"),
                    session: SessionId(700),
                    capability,
                }),
            },
            TerminalSize { rows: 24, cols: 80 },
            events,
        )
        .unwrap();
        let _cleanup = TerminationGuard(Arc::clone(&session));
        let _dispatcher = dispatch_test_events(session.clone(), receiver);

        let report = |capability, sequence, activity| crate::protocol::AgentReport {
            session: SessionId(700),
            capability,
            sequence,
            update: crate::protocol::AgentUpdate::Activity(activity),
        };
        assert!(
            session
                .apply_agent_report(&report([0x38; 32], Some(1), AgentActivity::Busy))
                .is_err()
        );
        assert_eq!(session.summary().activity, AgentActivity::Unknown);
        assert!(
            session
                .apply_agent_report(&report(capability, Some(0), AgentActivity::Busy))
                .is_err()
        );
        assert_eq!(session.summary().activity, AgentActivity::Unknown);
        assert!(
            session
                .apply_agent_report(&report(capability, Some(1), AgentActivity::Busy))
                .unwrap()
        );
        assert_eq!(session.summary().activity, AgentActivity::Busy);
        assert!(
            session
                .apply_agent_report(&report(capability, Some(1), AgentActivity::Idle))
                .is_err()
        );
        assert_eq!(session.summary().activity, AgentActivity::Busy);
        assert!(
            !session
                .apply_agent_report(&report(capability, Some(2), AgentActivity::Busy))
                .unwrap()
        );
        assert!(
            session
                .apply_agent_report(&report(capability, Some(2), AgentActivity::Idle))
                .is_err()
        );
        assert_eq!(session.summary().activity, AgentActivity::Busy);
        session.set_paused(true).unwrap();
        assert!(
            session
                .apply_agent_report(&report(capability, Some(3), AgentActivity::Idle))
                .unwrap()
        );
        assert_eq!(session.summary().activity, AgentActivity::Idle);
        session.terminate(Duration::from_millis(200)).unwrap();
        assert!(
            session
                .apply_agent_report(&report(capability, Some(4), AgentActivity::Busy))
                .is_err()
        );
        assert_eq!(session.summary().activity, AgentActivity::Unknown);
    }

    #[test]
    fn pause_resume_terminate_runs_term_handler() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(5),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "term-handler".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "trap 'printf TERM_HANDLED; exit 0' TERM; printf READY; while :; do read line; done"
                        .into(),
                ],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let _cleanup = TerminationGuard(Arc::clone(&session));
        let dispatcher = dispatch_test_events(session.clone(), rx);

        assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));
        assert!(session.set_paused(true).unwrap());
        session.terminate(Duration::from_millis(200)).unwrap();
        dispatcher.join().unwrap();

        assert!(String::from_utf8_lossy(&session.current_screen()).contains("TERM_HANDLED"));
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
        assert!(!group_exists(session.pgid).unwrap());
    }

    #[test]
    fn pause_resume_terminate_serializes_competing_controls() {
        let session = spawn_test_shell();
        let _cleanup = TerminationGuard(Arc::clone(&session));
        let gate = Arc::new(Barrier::new(3));
        let (results, received) = mpsc::sync_channel(3);

        let pause_gate = Arc::clone(&gate);
        let pause_session = Arc::clone(&session);
        let pause_results = results.clone();
        let pause = thread::spawn(move || {
            pause_gate.wait();
            let result = pause_session
                .set_paused(true)
                .map(|_| ())
                .map_err(|error| error.to_string());
            pause_results.send(("pause", result)).unwrap();
        });

        let resume_gate = Arc::clone(&gate);
        let resume_session = Arc::clone(&session);
        let resume_results = results.clone();
        let resume = thread::spawn(move || {
            resume_gate.wait();
            let result = resume_session
                .set_paused(false)
                .map(|_| ())
                .map_err(|error| error.to_string());
            resume_results.send(("resume", result)).unwrap();
        });

        let terminate_gate = Arc::clone(&gate);
        let terminate_session = Arc::clone(&session);
        let terminate = thread::spawn(move || {
            terminate_gate.wait();
            let result = terminate_session
                .terminate(Duration::from_millis(200))
                .map_err(|error| error.to_string());
            results.send(("terminate", result)).unwrap();
        });

        let mut outcomes = Vec::new();
        for _ in 0..3 {
            outcomes.push(
                received
                    .recv_timeout(Duration::from_secs(2))
                    .expect("competing controls must finish promptly"),
            );
        }
        pause.join().unwrap();
        resume.join().unwrap();
        terminate.join().unwrap();

        assert!(
            outcomes
                .iter()
                .any(|(name, result)| { *name == "terminate" && result.is_ok() })
        );
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
        assert!(!group_exists(session.pgid).unwrap());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminate_reaps_group_after_transient_sigcont_permission() {
        let (reap_gate, entered, release, signal_entered, signal_release, signal_result_entered) =
            ReapGate::new();
        let (session, dispatcher) = spawn_term_race_session(reap_gate);
        let _cleanup = TerminationGuard(Arc::clone(&session));
        let _gate_cleanup = ReapGateCleanup {
            release: release.clone(),
            signal_release: signal_release.clone(),
        };
        assert!(entered.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));

        let leader = session.summary().pid.unwrap();
        let (termination_sender, termination_receiver) = mpsc::sync_channel(1);
        let termination_session = Arc::clone(&session);
        let termination = thread::spawn(move || {
            let _ = termination_sender.send(
                termination_session
                    .terminate(Duration::from_millis(200))
                    .map_err(|error| format!("{error:#}")),
            );
        });
        assert!(signal_entered.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(wait_for_process_state(leader, 'Z', Duration::from_secs(2)));
        assert!(group_exists(session.pgid).unwrap());
        signal_release.send(()).unwrap();
        assert!(
            signal_result_entered
                .recv_timeout(Duration::from_secs(2))
                .is_ok()
        );
        release.send(()).unwrap();

        assert!(
            termination_receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_ok()
        );
        termination.join().unwrap();
        dispatcher.join().unwrap();
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
        assert!(!group_exists(session.pgid).unwrap());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminate_retains_sigcont_permission_error_when_group_remains() {
        let (reap_gate, entered, release, signal_entered, signal_release, signal_result_entered) =
            ReapGate::new();
        let (session, dispatcher) = spawn_term_race_session(reap_gate);
        let _cleanup = TerminationGuard(Arc::clone(&session));
        let _gate_cleanup = ReapGateCleanup {
            release: release.clone(),
            signal_release: signal_release.clone(),
        };
        assert!(entered.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));

        let leader = session.summary().pid.unwrap();
        let (termination_sender, termination_receiver) = mpsc::sync_channel(1);
        let termination_session = Arc::clone(&session);
        let termination = thread::spawn(move || {
            let result = termination_session
                .terminate(Duration::from_millis(50))
                .map_err(|error| format!("{error:#}"));
            let _ = termination_sender.send(result);
        });
        assert!(signal_entered.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(wait_for_process_state(leader, 'Z', Duration::from_secs(2)));
        assert!(group_exists(session.pgid).unwrap());
        signal_release.send(()).unwrap();
        assert!(
            signal_result_entered
                .recv_timeout(Duration::from_secs(2))
                .is_ok()
        );

        let error = termination_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .expect_err("a still-present refused group must retain SIGCONT error");
        assert!(error.contains("signal PTY process group"));
        assert!(error.contains("Operation not permitted"));
        release.send(()).unwrap();
        termination.join().unwrap();
        session.wait_until_exited(Duration::from_secs(2)).unwrap();
        dispatcher.join().unwrap();
        assert!(!group_exists(session.pgid).unwrap());
    }

    #[test]
    fn terminate_retains_sigcont_permission_error_for_live_group() {
        let refuse_sigcont = Arc::new(AtomicBool::new(true));
        let refusal = Arc::clone(&refuse_sigcont);
        let signal_result_hook = Arc::new(move || {
            if refusal.load(Ordering::Acquire) {
                Some(anyhow::anyhow!(
                    "signal PTY process group: Operation not permitted (os error 1)"
                ))
            } else {
                None
            }
        });
        let (session, dispatcher) = spawn_live_refusal_session(signal_result_hook);
        let _cleanup = TerminationGuard(Arc::clone(&session));
        assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));
        let leader = session.summary().pid.unwrap();

        let error = session
            .terminate(Duration::from_millis(50))
            .expect_err("a still-present refused group must retain SIGCONT error");
        assert!(error.to_string().contains("signal PTY process group"));
        assert!(error.to_string().contains("Operation not permitted"));
        assert!(pid_exists(leader), "SIGKILL must not reach the live leader");
        assert!(group_exists(session.pgid).unwrap());

        refuse_sigcont.store(false, Ordering::Release);
        session.terminate(Duration::from_millis(200)).unwrap();
        dispatcher.join().unwrap();
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
        assert!(!group_exists(session.pgid).unwrap());
    }

    #[test]
    fn pause_resume_exit_event_cannot_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(4),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "exit-race".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "printf 'EXIT_%s' READY; IFS= read -r _".into(),
                ],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let _cleanup = TerminationGuard(Arc::clone(&session));
        let gate = Arc::new(CancellableBarrier::new());
        let _gate_cleanup = BarrierGuard(Arc::clone(&gate));
        let (exit_ready, exit_ready_receiver) = mpsc::sync_channel(1);
        let (dispatch_done, dispatch_done_receiver) = mpsc::sync_channel(1);
        let (control_result, control_result_receiver) = mpsc::sync_channel(1);
        let dispatcher = dispatch_test_events_with_exit_gate(
            Arc::clone(&session),
            rx,
            Arc::clone(&gate),
            exit_ready,
            dispatch_done,
        );
        let control_gate = Arc::clone(&gate);
        let control_session = Arc::clone(&session);
        let control = thread::spawn(move || {
            if !control_gate.wait() {
                let _ = control_result.send(None);
                return;
            }
            let _ = control_result.send(Some(control_session.set_paused(true)));
        });

        assert!(wait_for_screen(
            &session,
            "EXIT_READY",
            Duration::from_secs(2)
        ));
        session.write(b"\n").unwrap();
        assert!(
            exit_ready_receiver
                .recv_timeout(Duration::from_secs(2))
                .is_ok()
        );
        assert!(matches!(
            control_result_receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Some(Err(_))
        ));
        assert!(
            dispatch_done_receiver
                .recv_timeout(Duration::from_secs(2))
                .is_ok()
        );
        control.join().unwrap();
        dispatcher.join().unwrap();
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
    }

    fn dispatch_test_events_with_exit_gate(
        session: Arc<Session>,
        rx: Receiver<SessionEvent>,
        gate: Arc<CancellableBarrier>,
        exit_ready: mpsc::SyncSender<()>,
        dispatch_done: mpsc::SyncSender<()>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let exited = matches!(event, SessionEvent::Exited { .. });
                if exited {
                    let _ = exit_ready.send(());
                    gate.wait();
                }
                session.apply_event(event);
                if exited {
                    let _ = dispatch_done.send(());
                    break;
                }
            }
        })
    }

    #[test]
    fn pause_resume_rejects_unsafe_group() {
        assert!(verify_group_identity(0, false).is_err());
        assert!(verify_group_identity(1, false).is_err());
        assert!(verify_group_identity(unsafe { libc::getpgrp() }, false).is_err());
    }

    #[test]
    fn shell_round_trip_updates_the_current_screen() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(1),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "local".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![OsString::from("sh")],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let dispatcher = dispatch_test_events(session.clone(), rx);
        session.write(b"printf 'OVRCR_%s' READY\r").unwrap();
        assert!(wait_for_screen(
            &session,
            "OVRCR_READY",
            Duration::from_secs(2)
        ));
        session.terminate(Duration::from_millis(200)).unwrap();
        dispatcher.join().unwrap();
    }

    #[test]
    fn final_output_is_parsed_before_session_becomes_removable() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(2),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "final".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec!["sh".into(), "-c".into(), "printf FINAL_MARKER".into()],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let first_event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(session.summary().phase, SessionPhase::Running));
        let mut next_event = Some(first_event);
        let mut saw_output = false;
        loop {
            let event = match next_event.take() {
                Some(event) => event,
                None => rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            };
            let exited = matches!(event, SessionEvent::Exited { .. });
            if !exited {
                saw_output = saw_output || matches!(event, SessionEvent::Output { .. });
            }
            session.apply_event(event);
            if exited {
                break;
            }
        }
        assert!(saw_output);
        assert!(String::from_utf8_lossy(&session.current_screen()).contains("FINAL_MARKER"));
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
    }

    #[test]
    fn resize_reaches_the_child_tty() {
        let session = spawn_test_shell();
        session
            .resize(TerminalSize {
                rows: 37,
                cols: 111,
            })
            .unwrap();
        session.write(b"stty size\r").unwrap();
        assert!(wait_for_screen(&session, "37 111", Duration::from_secs(2)));
        session.terminate(Duration::from_millis(200)).unwrap();
    }

    #[test]
    fn terminate_removes_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(3),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "group".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "trap '' HUP; sleep 30 & printf 'OVRCR_DESC:%s\\n' \"$!\"; exit".into(),
                ],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let dispatcher = dispatch_test_events(session.clone(), rx);
        let leader = session.summary().pid.unwrap();
        let screen_deadline = Instant::now() + Duration::from_secs(2);
        let descendant = loop {
            if let Some(pid) = extract_tagged_pid(&session.current_screen(), b"OVRCR_DESC:")
                && pid_exists(pid)
            {
                break pid;
            }
            assert!(
                Instant::now() < screen_deadline,
                "descendant PID was not observed: {}",
                String::from_utf8_lossy(&session.current_screen())
            );
            thread::park_timeout(Duration::from_millis(5));
        };
        assert!(pid_exists(descendant));
        let termination = session.terminate(Duration::from_millis(200));
        if termination.is_err() {
            let owned_group = unsafe { libc::getpgid(descendant as libc::pid_t) } == session.pgid;
            assert!(owned_group, "negative-control cleanup lost PTY ownership");
            signal_group(session.pgid, libc::SIGKILL).unwrap();
            assert!(
                wait_for_group_exit(session.pgid, Instant::now() + Duration::from_secs(2)).unwrap()
            );
            let _ = session.wait_until_exited(Duration::from_secs(2));
        }
        termination.unwrap();
        dispatcher.join().unwrap();
        assert_pid_is_gone(leader);
        assert_pid_is_gone(descendant);
        assert!(!group_exists(session.pgid).unwrap());
    }

    #[test]
    fn agent_report_order_rejects_replay_and_mode_switch() {
        let mut order = ReportOrder::default();
        assert!(order.accept(Some(2)).is_ok());
        assert!(order.accept(Some(1)).is_err());
        assert!(order.accept(Some(2)).is_err());
        assert!(order.accept(None).is_err());
        assert_eq!(order, ReportOrder::Sequenced(2));
        assert!(order.accept(Some(3)).is_ok());
    }

    #[test]
    fn agent_report_order_receipt_accepts_repeated_values() {
        let mut order = ReportOrder::default();
        assert!(order.accept(None).is_ok());
        assert!(order.accept(None).is_ok());
        assert_eq!(order, ReportOrder::Receipt);
        assert!(order.accept(Some(1)).is_err());
    }

    fn extract_tagged_pid(screen: &[u8], tag: &[u8]) -> Option<u32> {
        let start = screen.windows(tag.len()).position(|window| window == tag)? + tag.len();
        let digits = screen[start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .copied()
            .collect::<Vec<_>>();
        if digits.is_empty() {
            return None;
        }
        std::str::from_utf8(&digits).ok()?.parse().ok()
    }

    fn pid_exists(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    fn assert_pid_is_gone(pid: u32) {
        assert_eq!(unsafe { libc::kill(pid as libc::pid_t, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }
}
