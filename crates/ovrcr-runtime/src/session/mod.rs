use anyhow::{Context, Result, bail};
use ovrcr_protocol::context::{ContextUsageSnapshot, validate_context};
use ovrcr_protocol::{AgentReport, AgentUpdate, HISTORY_ROWS, HistorySnapshotId};
use ovrcr_terminal::{encode_paste, history::FrozenHistory, vt100};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use ovrcr_protocol::{
    AgentActivity, SessionId, SessionKind, SessionPhase, SessionRunId, SessionSummary, TerminalSize,
};

/// The native spawn primitive failed before the requested program could execute.
#[derive(Debug)]
pub(crate) struct NoProcessStarted;

impl std::fmt::Display for NoProcessStarted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("no process was started")
    }
}

impl std::error::Error for NoProcessStarted {}

/// The session's owned process had already exited before terminate ran.
#[derive(Debug)]
pub(crate) struct AlreadyExited;

impl std::fmt::Display for AlreadyExited {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("session already exited")
    }
}

impl std::error::Error for AlreadyExited {}

/// Attached-group discovery (`ps -t` / listing) failed, so ownership is unknown.
#[derive(Debug)]
pub(crate) struct DiscoveryFailed;

impl std::fmt::Display for DiscoveryFailed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("process ownership is uncertain")
    }
}

impl std::error::Error for DiscoveryFailed {}

fn discovery_error(error: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(DiscoveryFailed).context(error)
}

mod io;
mod process;
mod reporting;
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

/// Operator override for [`GROUP_LEADER_TIMEOUT`], in whole milliseconds.
const GROUP_LEADER_TIMEOUT_ENV: &str = "OVRCR_GROUP_LEADER_TIMEOUT_MS";

/// How long this spawn may wait for the child's process group.
///
/// Read once per spawn from [`GROUP_LEADER_TIMEOUT_ENV`]. An absent, empty,
/// or unparsable value keeps [`GROUP_LEADER_TIMEOUT`], so the default bound
/// is unchanged unless an operator sets the variable deliberately.
fn group_leader_timeout(raw: Option<&str>) -> Duration {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(GROUP_LEADER_TIMEOUT, Duration::from_millis)
}

/// Kill and reap a child that never became a usable session.
///
/// `kill` only delivers a signal, so without the wait the failed spawn would
/// leave a zombie this process never collects. `wait` also covers the child
/// that already died from the signal `kill` sent.
fn kill_and_reap(child: &mut (dyn Child + Send + Sync)) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The process group the child leads, once it has created it.
///
/// `spawn_command` returns as soon as the fork completes, while the child
/// calls `setsid` and claims the PTY with `TIOCSCTTY` afterwards on its own
/// schedule. Until then the terminal reports no foreground group, and a
/// child that already exited reports none again, so a single read races
/// both ways. Poll the terminal, and accept the child's own group id once
/// `setsid` has run: that group persists while the child is a zombie, so a
/// command that exits immediately is still attributed correctly.
///
/// `probe` is a test seam. `None` in every production spawn; a test passes a
/// closure that reports whether this poll may consult the child's group, so
/// the timeout branch can be exercised. `setsid` runs in the child's
/// `pre_exec`, before any command of ours gets to run, so no real command can
/// withhold its process group for long enough to reach the deadline.
#[cfg(unix)]
fn wait_for_group_leader(
    master: &dyn MasterPty,
    pid: u32,
    timeout: Duration,
    probe: Option<&(dyn Fn(libc::pid_t) -> bool + Send + Sync)>,
) -> Result<libc::pid_t> {
    let pid = libc::pid_t::try_from(pid).context("PTY child PID does not fit a pid_t")?;
    let deadline = Instant::now() + timeout;
    loop {
        if probe.is_none_or(|probe| probe(pid)) {
            if let Some(pgid) = master.process_group_leader() {
                return Ok(pgid);
            }
            if unsafe { libc::getpgid(pid) } == pid {
                return Ok(pid);
            }
        }
        if Instant::now() >= deadline {
            bail!("PTY did not provide a process-group leader within {timeout:?}")
        }
        thread::park_timeout(Duration::from_micros(200));
    }
}

mod shell_prompt;

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
    pub run: SessionRunId,
    pub kind: SessionKind,
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
    Output {
        id: SessionId,
        run: SessionRunId,
        bytes: Vec<u8>,
    },
    Exited {
        id: SessionId,
        run: SessionRunId,
        phase: SessionPhase,
    },
}

struct SessionState {
    reporting: reporting::ReportingState,
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

/// Keep application and manual titles with the parser. No callback acquires
/// session-state or server locks.
#[derive(Clone, Debug, Default)]
pub(crate) struct SessionTitles {
    pub(crate) revision: u64,
    pub(crate) application: Option<String>,
    pub(crate) pinned: Option<String>,
}

const MAX_TITLE_CHARS: usize = 128;

pub(crate) fn sanitize_title(title: &str) -> Option<String> {
    let title: String = title.chars().filter(|c| {
        !c.is_control() && !matches!(*c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }).take(MAX_TITLE_CHARS).collect();
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

impl vt100::Callbacks for SessionTitles {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        let next = sanitize_title(&String::from_utf8_lossy(title));
        if self.application != next {
            self.application = next;
            self.revision = self.revision.saturating_add(1);
        }
    }

    fn unhandled_osc(&mut self, screen: &mut vt100::Screen, params: &[&[u8]]) {
        // vt100 routes title text containing semicolons here because vte
        // splits OSC parameters. Reassemble a bounded prefix of that text.
        if matches!(params.first(), Some(&b"0" | &b"2")) && params.len() > 2 {
            let title: Vec<u8> = params[1..]
                .iter()
                .enumerate()
                .flat_map(|(index, part)| {
                    (index > 0)
                        .then_some(b';')
                        .into_iter()
                        .chain(part.iter().copied())
                })
                .take(MAX_TITLE_CHARS * 4)
                .collect();
            self.set_window_title(screen, &title);
        }
    }
}

struct TerminalState {
    parser: vt100::Parser<SessionTitles>,
    revision: u64,
}

#[cfg(test)]
type TermResultHook = Arc<dyn Fn() -> Option<bool> + Send + Sync>;

#[cfg(test)]
type ListingErrorHook = Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>;

pub struct Session {
    // Keep startup files alive until the shell has finished using them.
    _shell_startup: Option<tempfile::TempDir>,
    summary: SessionSummary,
    #[cfg(test)]
    pub(crate) initial_cwd: PathBuf,
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
    #[cfg(test)]
    listing_error_hook: Mutex<Option<ListingErrorHook>>,
    #[cfg(test)]
    term_result_hook: Mutex<Option<TermResultHook>>,
    terminate_lock: Mutex<()>,
    reader_done: Mutex<bool>,
    reader_changed: Condvar,
    handles: Mutex<JoinHandles>,
}

/// Publishes a spawning session where its own events can be dispatched.
pub(crate) type SessionRegister<'a> = &'a dyn Fn(&Arc<Session>);

/// The register for a spawn whose test does not watch publication. It still
/// runs at the publication point, so the hook constructors below reach the
/// PTY reader and child waiter the same way production does.
#[cfg(test)]
pub(crate) const NO_REGISTER: SessionRegister<'static> = &|_| {};

/// Test-only control over the group-leader wait in `spawn_internal`.
///
/// `None` in every production spawn, which reads its bound from the
/// environment and consults the real PTY on every poll.
pub(crate) struct LeaderWaitOverride<'a> {
    pub(crate) timeout: Duration,
    pub(crate) probe: &'a (dyn Fn(libc::pid_t) -> bool + Send + Sync),
}

impl Session {
    /// Spawn a session and publish it through `register` before its PTY
    /// reader and child waiter start.
    ///
    /// The threads that turn PTY bytes and the child's exit into
    /// `SessionEvent`s start inside the spawn, so a caller that registers the
    /// session afterwards races its own dispatcher: an event for a session
    /// the dispatcher cannot find yet is dropped, losing the first output of
    /// every session. `register` runs on this thread, at the point where the
    /// session exists but nothing can emit an event for it yet.
    pub(crate) fn spawn_registered(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: crate::server::ReportingSender<SessionEvent>,
        register: SessionRegister<'_>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(id, spec, size, events, register, None, None, None, None)
    }

    #[cfg(test)]
    pub(crate) fn spawn_with_test_hooks(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: crate::server::ReportingSender<SessionEvent>,
        reap_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_result_hook: Option<Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(
            id,
            spec,
            size,
            events,
            NO_REGISTER,
            reap_hook,
            signal_hook,
            signal_result_hook,
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn spawn_with_leader_wait(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: crate::server::ReportingSender<SessionEvent>,
        leader_wait: LeaderWaitOverride<'_>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(
            id,
            spec,
            size,
            events,
            NO_REGISTER,
            None,
            None,
            None,
            Some(leader_wait),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_internal(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: crate::server::ReportingSender<SessionEvent>,
        register: SessionRegister<'_>,
        reap_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_hook: Option<Arc<dyn Fn() + Send + Sync>>,
        signal_result_hook: Option<Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>>,
        leader_wait: Option<LeaderWaitOverride<'_>>,
    ) -> Result<Arc<Self>> {
        #[cfg(not(test))]
        let _ = &signal_hook;
        #[cfg(not(test))]
        let _ = &signal_result_hook;

        let argv0 = spec
            .argv
            .first()
            .context("session command cannot be empty")
            .context(NoProcessStarted)?
            .clone();
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context(NoProcessStarted)?;
        let mut command = CommandBuilder::new(argv0);
        command.args(spec.argv.iter().skip(1));
        #[cfg(test)]
        let initial_cwd = spec.cwd.clone();
        command.cwd(&spec.cwd);
        let shell_startup =
            shell_prompt::configure(&mut command, &spec.argv).context(NoProcessStarted)?;
        command.env_remove("OVRCR_AGENT_SOCKET");
        command.env_remove("OVRCR_AGENT_TOKEN");
        command.env_remove("OVRCR_HOOK_SOCKET");
        command.env_remove("OVRCR_SESSION_ID");
        command.env_remove("OVRCR_HOOK_TOKEN");
        if let Some(hook_env) = &spec.hook_env {
            command.env("OVRCR_HOOK_SOCKET", &hook_env.socket);
            command.env("OVRCR_SESSION_ID", hook_env.session.0.to_string());
            command.env("OVRCR_HOOK_TOKEN", capability_hex(&hook_env.capability));
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .context(NoProcessStarted)?;
        let pid = child.process_id().context("PTY child has no process ID")?;

        let (leader_timeout, leader_probe) = match &leader_wait {
            Some(override_) => (override_.timeout, Some(override_.probe)),
            None => (
                group_leader_timeout(
                    std::env::var_os(GROUP_LEADER_TIMEOUT_ENV)
                        .as_deref()
                        .and_then(|value| value.to_str()),
                ),
                None,
            ),
        };
        #[cfg(unix)]
        let pgid = wait_for_group_leader(pair.master.as_ref(), pid, leader_timeout, leader_probe)
            .inspect_err(|_| kill_and_reap(&mut *child))?;
        #[cfg(not(unix))]
        let pgid = {
            let _ = pid;
            bail!("OVRCR sessions require Unix process groups")
        };

        if pgid != pid as libc::pid_t {
            kill_and_reap(&mut *child);
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
            _shell_startup: shell_startup,
            #[cfg(test)]
            initial_cwd,
            summary: SessionSummary {
                cwd: spec.cwd.clone(),
                archived: false,
                title: None,
                id,
                run: spec.run,
                kind: spec.kind.clone(),
                project: spec.project,
                workspace: spec.workspace,
                name: spec.name.clone(),
                label: spec.label,
                pid: Some(pid),
                started_unix_ms: Some(started_unix_ms),
                phase: SessionPhase::Running,
                activity: AgentActivity::Unknown,
                context_usage: None,
                agent: None,
                agent_epoch: 0,
                unread: None,
                recovery: None,
            },
            state: Mutex::new(SessionState {
                reporting: reporting::ReportingState::default(),
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
                parser: vt100::Parser::new_with_callbacks(
                    size.rows,
                    size.cols,
                    HISTORY_ROWS,
                    SessionTitles {
                        revision: 0,
                        application: None,
                        pinned: sanitize_title(&spec.name),
                    },
                ),
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
            #[cfg(test)]
            listing_error_hook: Mutex::new(None),
            #[cfg(test)]
            term_result_hook: Mutex::new(None),
        });

        // Publish the session before anything can emit an event for it.
        register(&session);

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
        Ok(session)
    }

    pub fn run(&self) -> SessionRunId {
        self.summary.run
    }

    pub fn id(&self) -> SessionId {
        self.summary.id
    }

    pub fn is_live(&self) -> bool {
        matches!(
            self.state.lock().unwrap().phase,
            SessionPhase::Running | SessionPhase::Paused
        )
    }

    pub(crate) fn title_revision(&self) -> u64 {
        self.terminal.lock().unwrap().parser.callbacks().revision
    }

    pub(crate) fn title_snapshot(&self) -> SessionTitles {
        self.terminal.lock().unwrap().parser.callbacks().clone()
    }

    pub(crate) fn restore_titles(&self, pinned: Option<String>, application: Option<String>) {
        let mut terminal = self.terminal.lock().unwrap();
        let titles = terminal.parser.callbacks_mut();
        titles.pinned = pinned.and_then(|title| sanitize_title(&title));
        titles.application = application.and_then(|title| sanitize_title(&title));
    }

    pub(crate) fn set_title(&self, title: Option<String>) -> Result<()> {
        let title = title
            .map(|value| sanitize_title(&value).context("title must contain visible text"))
            .transpose()?;
        let mut terminal = self.terminal.lock().unwrap();
        let titles = terminal.parser.callbacks_mut();
        if titles.pinned != title {
            titles.pinned = title;
            titles.revision = titles.revision.saturating_add(1);
        }
        Ok(())
    }

    pub(crate) fn effective_title(&self) -> Option<String> {
        let terminal = self.terminal.lock().unwrap();
        let titles = terminal.parser.callbacks();
        titles.pinned.clone().or_else(|| titles.application.clone())
    }

    pub fn summary(&self) -> SessionSummary {
        let mut summary = self.summary.clone();
        summary.title = self.effective_title();
        summary.recovery = None;
        let state = self.state.lock().unwrap();
        summary.phase = state.phase.clone();
        summary.pid = state.pid;
        summary.activity = state.activity;
        summary.context_usage = state.context_usage.clone();
        summary.agent_epoch = state.reporting.epoch;
        summary.unread = state.reporting.unread();
        summary.agent = state.reporting.snapshot.clone();
        if let Some(agent) = &summary.agent {
            summary.activity = agent.effective_activity();
            summary.context_usage = agent.metrics.as_ref().map(|metrics| ContextUsageSnapshot {
                report: ovrcr_protocol::context::ContextUsageReport {
                    source: if agent.binding.provider == ovrcr_protocol::AgentProvider::Claude {
                        ovrcr_protocol::context::ContextSource::ClaudeCodeStatusline
                    } else {
                        ovrcr_protocol::context::ContextSource::Generic
                    },
                    model: metrics.sample.model.clone(),
                    conversation: Some(agent.binding.conversation.clone()),
                    used_tokens: metrics.sample.context.value.used_tokens,
                    capacity_tokens: metrics.sample.context.value.capacity_tokens,
                },
                received_unix_ms: metrics.context_received_unix_ms,
            });
        }
        summary
    }

    /// Process groups attached to this session's terminal other than the
    /// leader's own group: jobs an interactive shell started with `&`.
    fn attached_subgroups(&self) -> Result<std::collections::BTreeSet<libc::pid_t>> {
        #[cfg(test)]
        if let Some(hook) = self.listing_error_hook.lock().unwrap().clone()
            && let Some(error) = hook()
        {
            return Err(discovery_error(error));
        }
        let Some(tty) = self.tty.as_deref() else {
            return Ok(std::collections::BTreeSet::new());
        };
        attached_groups_checked(tty, self.pgid).map_err(discovery_error)
    }

    pub fn set_paused(&self, paused: bool) -> Result<bool> {
        let _control = self.terminate_lock.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        if !matches!(state.phase, SessionPhase::Running | SessionPhase::Paused) {
            bail!("session has exited");
        }
        verify_owned_group(self)?;

        let subgroups = self.attached_subgroups()?;
        let signal = if paused { libc::SIGSTOP } else { libc::SIGCONT };
        if !signal_group(self.pgid, signal)? {
            bail!("PTY process group no longer exists");
        }
        signal_attached_groups(&subgroups, signal);

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
        match self.state.lock().unwrap().phase {
            SessionPhase::Running => Ok(()),
            SessionPhase::Paused => Err(InputAdmissionError::Paused),
            SessionPhase::Exited { .. } | SessionPhase::Stopped | SessionPhase::Interrupted => {
                Err(InputAdmissionError::Exited)
            }
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
            SessionEvent::Output { id, run, bytes }
                if id == self.summary.id && run == self.run() =>
            {
                let mut terminal = self.terminal.lock().unwrap();
                terminal.parser.process(&bytes);
                terminal.revision = terminal.revision.saturating_add(1);
                self.parser_changed.notify_all();
            }
            SessionEvent::Exited { id, run, phase }
                if id == self.summary.id && run == self.run() =>
            {
                let mut state = self.state.lock().unwrap();
                state.phase = phase;
                state.pid = None;
                state.activity = AgentActivity::Unknown;
                state.reporting.lost("pty_exited_before_finalization");
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
        if state.reporting.active()
            && state.reporting.snapshot.is_some()
            && !matches!(report.update, AgentUpdate::Provider(_))
        {
            bail!("legacy reports rejected while supervisor is active");
        }
        match &report.update {
            AgentUpdate::Provider(provider) => state.reporting.apply(provider, false),
            AgentUpdate::Activity(activity) => {
                let mut next_order = state.activity_order;
                next_order.accept(report.sequence)?;
                let changed = state.reporting.snapshot.is_some() || state.activity != *activity;
                state.reporting.snapshot = None;
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
                let changed = state.reporting.snapshot.is_some()
                    || state.context_usage.as_ref() != Some(&snapshot);
                state.reporting.snapshot = None;
                state.context_order = next_order;
                state.context_usage = Some(snapshot);
                Ok(changed)
            }
        }
    }

    pub fn revoke_hook_capability(&self) {
        let mut state = self.state.lock().unwrap();
        state.reporting.lost("capability_revoked");
        state.hook_capability = None;
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

    #[cfg(test)]
    pub(crate) fn set_listing_error_hook(&self, hook: Option<ListingErrorHook>) {
        *self.listing_error_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub(crate) fn set_term_result_hook(&self, hook: Option<TermResultHook>) {
        *self.term_result_hook.lock().unwrap() = hook;
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
            return Err(anyhow::Error::new(AlreadyExited));
        }

        // Ownership is the controlling terminal, so every attached group is
        // signalled, including job-control subgroups an interactive shell
        // created. SIGTERM goes first so handlers can run; SIGCONT lets
        // stopped groups receive it. Groups still present after
        // `HANGUP_DELAY` get SIGHUP, which is what a closed terminal delivers
        // and the only signal interactive shells honour, so `local` shells
        // exit without waiting out the grace period.
        let mut signal_error = None;
        let initially_owned = should_signal_group(self)?;
        let mut listing_error = None;
        let subgroups = match self.attached_subgroups() {
            Ok(groups) => groups,
            Err(error) => {
                listing_error = Some(error);
                std::collections::BTreeSet::new()
            }
        };
        let mut leader_signal_delivered = false;
        if initially_owned {
            let term_result = {
                #[cfg(test)]
                {
                    let override_result = self
                        .term_result_hook
                        .lock()
                        .unwrap()
                        .as_ref()
                        .and_then(|hook| hook());
                    if let Some(delivered) = override_result {
                        Ok(delivered)
                    } else {
                        signal_group(self.pgid, libc::SIGTERM)
                    }
                }
                #[cfg(not(test))]
                signal_group(self.pgid, libc::SIGTERM)
            };
            leader_signal_delivered = term_result?;
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
        if let Some(error) = listing_error {
            return Err(signal_error.unwrap_or(error));
        }
        if !leader_signal_delivered {
            return Err(if initially_owned {
                signal_error
                    .unwrap_or_else(|| anyhow::anyhow!("controlled stop did not deliver SIGTERM"))
            } else {
                anyhow::Error::new(AlreadyExited)
            });
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
