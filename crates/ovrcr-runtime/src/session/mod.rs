use anyhow::{Context, Result, bail};
use ovrcr_protocol::context::{ContextUsageSnapshot, validate_context};
use ovrcr_protocol::{AgentReport, AgentUpdate, HISTORY_ROWS, HistorySnapshotId};
use ovrcr_terminal::{encode_paste, history::FrozenHistory, vt100};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, mpsc::SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use ovrcr_protocol::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};

mod io;
mod process;
use io::{read_pty, wait_for_child};
use process::*;

/// How long `terminate` waits after SIGTERM before hanging up a process
/// group that is still present. Interactive shells ignore SIGTERM, so this
/// bounds the cost of closing a `local` shell without denying programs that
/// handle SIGTERM a chance to exit cleanly first.
const HANGUP_DELAY: Duration = Duration::from_millis(500);

/// How long `spawn` waits for the child to claim the PTY as its controlling
/// terminal. The child does that between `fork` and `exec`, so the parent
/// normally observes it within microseconds; the bound only limits how long
/// a wedged child can stall session creation.
const GROUP_LEADER_TIMEOUT: Duration = Duration::from_secs(5);

/// The process group the child leads, once it has created it.
///
/// `spawn_command` returns as soon as the fork completes, while the child
/// calls `setsid` and claims the PTY with `TIOCSCTTY` afterwards on its own
/// schedule. Until then the terminal reports no foreground group, and a
/// child that already exited reports none again, so a single read races
/// both ways. Poll the terminal, and accept the child's own group id once
/// `setsid` has run: that group persists while the child is a zombie, so a
/// command that exits immediately is still attributed correctly.
#[cfg(unix)]
fn wait_for_group_leader(master: &dyn MasterPty, pid: u32) -> Result<libc::pid_t> {
    let pid = libc::pid_t::try_from(pid).context("PTY child PID does not fit a pid_t")?;
    let deadline = Instant::now() + GROUP_LEADER_TIMEOUT;
    loop {
        if let Some(pgid) = master.process_group_leader() {
            return Ok(pgid);
        }
        if unsafe { libc::getpgid(pid) } == pid {
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            bail!("PTY did not provide a process-group leader")
        }
        thread::park_timeout(Duration::from_micros(200));
    }
}

#[cfg(test)]
mod tests;

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

struct TerminalState {
    parser: vt100::Parser,
    revision: u64,
}

pub struct Session {
    summary: SessionSummary,
    state: Mutex<SessionState>,
    state_changed: Condvar,
    terminal: Mutex<TerminalState>,
    parser_changed: Condvar,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    pgid: libc::pid_t,
    /// Short controlling-terminal name (`ttys003`, `pts/3`) used to find
    /// job-control subgroups that share the session's PTY.
    tty: Option<String>,
    #[cfg(test)]
    signal_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    signal_result_hook: Option<Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>>,
    #[cfg(test)]
    history_capture_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
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

    #[allow(clippy::too_many_arguments)]
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
        let pgid = wait_for_group_leader(pair.master.as_ref(), pid)?;
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
        let tty = pair
            .master
            .tty_name()
            .and_then(|path| short_tty_name(&path));

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
            terminal: Mutex::new(TerminalState {
                parser: vt100::Parser::new(size.rows, size.cols, HISTORY_ROWS),
                revision: 0,
            }),
            parser_changed: Condvar::new(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            pgid,
            tty,
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
            #[cfg(test)]
            history_capture_hook: Mutex::new(None),
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

    /// Process groups attached to this session's terminal other than the
    /// leader's own group: jobs an interactive shell started with `&`.
    fn attached_subgroups(&self) -> std::collections::BTreeSet<libc::pid_t> {
        self.tty
            .as_deref()
            .map(|tty| attached_groups(tty, self.pgid))
            .unwrap_or_default()
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
        signal_attached_groups(&self.attached_subgroups(), signal);
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
        let mut terminal = self.terminal.lock().unwrap();
        terminal.parser.screen_mut().set_size(size.rows, size.cols);
        terminal.revision = terminal.revision.saturating_add(1);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn master_size(&self) -> Result<TerminalSize> {
        let size = self.master.lock().unwrap().get_size()?;
        Ok(TerminalSize {
            rows: size.rows,
            cols: size.cols,
        })
    }

    pub fn apply_event(&self, event: SessionEvent) {
        match event {
            SessionEvent::Output { id, bytes } if id == self.summary.id => {
                let mut terminal = self.terminal.lock().unwrap();
                terminal.parser.process(&bytes);
                terminal.revision = terminal.revision.saturating_add(1);
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
                let mut next_order = state.context_order;
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
        let mut terminal = self.terminal.lock().unwrap();
        terminal.parser.screen_mut().set_scrollback(0);
        terminal.parser.screen().state_formatted()
    }

    pub fn terminal_text(&self) -> (TerminalSize, String) {
        let mut terminal = self.terminal.lock().unwrap();
        terminal.parser.screen_mut().set_scrollback(0);
        let screen = terminal.parser.screen();
        let (rows, cols) = screen.size();
        (TerminalSize { rows, cols }, screen.contents())
    }

    pub fn capture_history(&self, snapshot: HistorySnapshotId) -> Result<FrozenHistory> {
        let (revision, screen) = {
            let terminal = self.terminal.lock().unwrap();
            (terminal.revision, terminal.parser.screen().clone())
        };
        #[cfg(test)]
        if let Some(hook) = self.history_capture_hook.lock().unwrap().clone() {
            hook();
        }
        FrozenHistory::capture(self.summary.id, snapshot, revision, screen)
    }

    #[cfg(test)]
    pub(crate) fn with_terminal_lock_for_test(&self, operation: impl FnOnce()) {
        let _terminal = self.terminal.lock().unwrap();
        operation();
    }

    #[cfg(test)]
    pub(crate) fn set_history_capture_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.history_capture_hook.lock().unwrap() = hook;
    }

    pub fn send_text(&self, text: &str, submit: bool) -> Result<()> {
        let bracketed_paste = {
            let terminal = self.terminal.lock().unwrap();
            terminal.parser.screen().bracketed_paste()
        };
        let bytes = encode_paste(text, bracketed_paste);
        self.admit_input().map_err(anyhow::Error::new)?;
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(&bytes).context("write text to PTY")?;
        if submit {
            writer.write_all(b"\r").context("submit text to PTY")?;
        }
        writer.flush().context("flush PTY")?;
        Ok(())
    }

    pub fn wait_for_output(&self, timeout: Duration) {
        let terminal = self.terminal.lock().unwrap();
        if terminal.revision == 0 {
            let _ = self.parser_changed.wait_timeout(terminal, timeout);
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

        // Ownership is the controlling terminal, so every attached group is
        // signalled, including job-control subgroups an interactive shell
        // created. SIGTERM goes first so handlers can run; SIGCONT lets
        // stopped groups receive it. Groups still present after
        // `HANGUP_DELAY` get SIGHUP, which is what a closed terminal delivers
        // and the only signal interactive shells honour, so `local` shells
        // exit without waiting out the grace period.
        let subgroups = self.attached_subgroups();
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
        for signal in [libc::SIGTERM, libc::SIGCONT] {
            signal_attached_groups(&subgroups, signal);
        }
        let deadline = Instant::now() + grace;
        let hangup_at = deadline.min(Instant::now() + HANGUP_DELAY);
        match wait_for_group_exit(self.pgid, hangup_at) {
            Ok(true) => {}
            Ok(false) => match should_signal_group(self) {
                Ok(true) => {
                    if let Err(error) = signal_group(self.pgid, libc::SIGHUP) {
                        return Err(signal_error.unwrap_or(error));
                    }
                }
                Ok(false) => {}
                Err(error) => return Err(signal_error.unwrap_or(error)),
            },
            Err(error) => return Err(signal_error.unwrap_or(error)),
        }
        for pgid in &subgroups {
            if !wait_for_group_exit(*pgid, hangup_at)? && verify_group_identity(*pgid, true).is_ok()
            {
                let _ = signal_group(*pgid, libc::SIGHUP);
            }
        }
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
            if should_kill && let Err(error) = signal_group(self.pgid, libc::SIGKILL) {
                return Err(signal_error.unwrap_or(error));
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

        for pgid in &subgroups {
            if wait_for_group_exit(*pgid, deadline)? {
                continue;
            }
            if verify_group_identity(*pgid, true).is_ok() {
                let _ = signal_group(*pgid, libc::SIGKILL);
            }
            let kill_deadline = Instant::now() + grace.max(Duration::from_secs(2));
            if !wait_for_group_exit(*pgid, kill_deadline)? {
                return Err(signal_error.unwrap_or_else(|| {
                    anyhow::anyhow!("attached process group {pgid} did not exit after SIGKILL")
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
