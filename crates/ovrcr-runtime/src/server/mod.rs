use crate::config::{
    ProjectRecord, Registry, WorkspaceRecord, initialize_registry, save_registry_atomic,
};
use crate::git::{self, BranchSpec};
use crate::retained::{RetainedSession, SessionMetadata, SessionStore};
use crate::session::{
    AlreadyExited, HookEnvironment, InputAdmissionError, Session, SessionEvent, SessionId,
    SessionPhase, SessionSpec, SessionSummary, TerminalSize,
};
use crate::task_manager::TaskManager;
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{
    BranchRequest, ClientMessage, ClientRole, DashboardView, ErrorCode, HierarchySnapshot,
    PROTOCOL_VERSION, ProjectSummary, Request, Response, ServerEvent, ServerMessage, SessionKind,
    SessionRunId, WorkspaceSummary, read_frame, read_preamble, write_frame, write_preamble,
};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub mod build_identity;
mod claude_allowance;
mod connections;
mod cursor_quota;
mod dashboard;
mod dispatch;
mod event_log;
mod lifecycle;
mod navigation;
pub use navigation::bridge_executable_sha256;
mod outbound;
mod quota;
mod quota_probe;
mod reporting_queue;
mod startup;
mod title;
mod watch;

use connections::{
    combine_control_and_refresh, error_chain_string, error_for_lifecycle, error_response,
    handle_connection, handle_request_with_id, input_error_code, requested_kill_grace,
    response_message,
};
pub(crate) use dashboard::ActiveDashboard;
use dashboard::spawn_writer;
use dispatch::bridge_events;
pub use dispatch::{DispatchCompletion, DispatchMessage, HistoryRequest, run_dispatcher};
use outbound::{DashboardDelivery, Enqueue};
pub use outbound::{DashboardOutbound, DashboardSink};
#[cfg(feature = "acceptance-diagnostics")]
pub use outbound::{DashboardQueueMonitor, DashboardQueueSnapshot};
pub use ovrcr_protocol::AutomaticLocalTerminals;
pub use quota::{NativeQuotaUpdate, normalize_native_quota};
#[cfg(feature = "acceptance-diagnostics")]
pub use startup::run_server_with_diagnostics;
pub use startup::{ServerPaths, prepare_socket_directory, run_server};
use startup::{generate_hook_capability, validate_bound_socket, wake_accept};

#[cfg(test)]
use connections::handle_shutdown;

#[derive(Debug)]
struct LifecycleFailure {
    code: ErrorCode,
    message: String,
    publish_hierarchy: bool,
}

impl std::fmt::Display for LifecycleFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LifecycleFailure {}

fn lifecycle_error(code: ErrorCode, message: impl Into<String>) -> anyhow::Error {
    lifecycle_error_with_hierarchy(code, message, false)
}

fn lifecycle_error_with_hierarchy(
    code: ErrorCode,
    message: impl Into<String>,
    publish_hierarchy: bool,
) -> anyhow::Error {
    anyhow::Error::new(LifecycleFailure {
        code,
        message: message.into(),
        publish_hierarchy,
    })
}

pub const RAW_EVENT_QUEUE_CAPACITY: usize = 64;
pub const RAW_DISPATCH_QUEUE_CAPACITY: usize = 64;
const DASHBOARD_QUEUE: usize = 64;

#[cfg(not(feature = "acceptance-diagnostics"))]
use reporting_queue::ReportingQueueMonitor;
use reporting_queue::reporting_channel;
#[cfg(feature = "acceptance-diagnostics")]
pub use reporting_queue::{ReportingQueueMonitor, ReportingQueueSnapshot};
pub(crate) use reporting_queue::{ReportingReceiver, ReportingSender};

fn event_weight(event: &SessionEvent) -> usize {
    match event {
        SessionEvent::Output { bytes, .. } => bytes.len(),
        SessionEvent::RestoreInputFailed { message, .. } => message.len(),
        SessionEvent::Exited { phase, .. } => {
            bincode::serde::encode_to_vec(phase, bincode::config::standard())
                .map_or(0, |bytes| bytes.len())
        }
    }
}

fn dispatch_weight(message: &DispatchMessage) -> usize {
    match message {
        DispatchMessage::AgentReport { report, .. } => {
            bincode::serde::encode_to_vec(report, bincode::config::standard())
                .map_or(0, |v| v.len())
        }
        DispatchMessage::AgentCommand { request, .. } => {
            bincode::serde::encode_to_vec(request, bincode::config::standard())
                .map_or(0, |bytes| bytes.len())
        }
        DispatchMessage::SetView { view, .. } => {
            bincode::serde::encode_to_vec(view, bincode::config::standard())
                .map_or(0, |bytes| bytes.len())
        }
        DispatchMessage::NativeQuota(update) => {
            update.report.windows.as_ref().map_or(0, |windows| {
                windows
                    .iter()
                    .map(|window| {
                        std::mem::size_of_val(window) + window.id.len() + window.label.len()
                    })
                    .sum()
            })
        }
        DispatchMessage::CursorQuota(update) => {
            update.native.report.windows.as_ref().map_or(0, |windows| {
                windows
                    .iter()
                    .map(|window| {
                        std::mem::size_of_val(window) + window.id.len() + window.label.len()
                    })
                    .sum()
            })
        }
        DispatchMessage::Session(event) => event_weight(event),
        _ => 0,
    }
}

pub(crate) fn event_channel(
    monitor: Option<&ReportingQueueMonitor>,
) -> (
    ReportingSender<SessionEvent>,
    ReportingReceiver<SessionEvent>,
) {
    reporting_channel(RAW_EVENT_QUEUE_CAPACITY, event_weight, monitor)
}

pub(crate) fn dispatch_channel(
    monitor: Option<&ReportingQueueMonitor>,
) -> (
    ReportingSender<DispatchMessage>,
    ReportingReceiver<DispatchMessage>,
) {
    reporting_channel(RAW_DISPATCH_QUEUE_CAPACITY, dispatch_weight, monitor)
}

#[cfg(feature = "acceptance-diagnostics")]
#[derive(Clone, Default)]
pub struct ServerQueueDiagnostics {
    pub raw_events: ReportingQueueMonitor,
    pub dispatcher: ReportingQueueMonitor,
    pub dashboard: DashboardQueueMonitor,
}

#[cfg(test)]
type ResizeHook = Arc<dyn Fn(&Session, TerminalSize) -> Result<()> + Send + Sync>;

enum WorkspaceLaunch {
    None,
    Shell,
    Session(ovrcr_protocol::SessionLaunch),
}

const MAX_LIVE_SESSIONS: usize = 50;

struct SessionControlTarget {
    run: SessionRunId,
    session: Option<Arc<Session>>,
    already_exited: bool,
}

#[derive(Clone, Debug)]
struct CheckoutObservation {
    path: PathBuf,
    git_identity: Option<String>,
    name: String,
    root: bool,
    warning: Option<String>,
}

#[derive(Default)]
struct WipWait {
    pending: Mutex<WipPending>,
    cv: Condvar,
}

#[derive(Default)]
struct WipPending {
    ask: Option<WipAsk>,
}

struct WipAsk {
    left: Vec<(String, String)>,
    save: Vec<(String, String)>,
}

struct WipTarget {
    project: String,
    id: String,
    branch: String,
    path: PathBuf,
}

pub struct ServerState {
    server_lifetime: String,
    callback_executable_sha256: Option<String>,
    pub tasks: Option<Arc<TaskManager>>,
    socket: PathBuf,
    pub registry_path: PathBuf,
    pub registry: Mutex<Registry>,
    pub sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    pub(super) dashboard: ActiveDashboard,
    quotas: Mutex<ovrcr_protocol::QuotaSnapshot>,
    quota_refresh: Mutex<quota::Refresh>,
    settings: Mutex<watch::Watched>,
    pub(crate) retained: parking_lot::Mutex<SessionStore>,
    observations: parking_lot::Mutex<HashMap<(String, String), CheckoutObservation>>,
    pub mutation_lock: Mutex<()>,
    pub(crate) lifecycle: lifecycle::Lifecycle,
    pub dispatch: ReportingSender<DispatchMessage>,
    pub shutdown: AtomicBool,
    pub stopping: AtomicBool,
    pub events: Mutex<Option<ReportingSender<SessionEvent>>>,
    event_log: Mutex<event_log::Log>,
    wip: WipWait,
    #[cfg(test)]
    pub(super) resize_hook: Mutex<Option<ResizeHook>>,
    #[cfg(test)]
    pub(super) before_view_publish_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Called by the dashboard writer thread just before each socket write, so
    /// a test can wait for the writer to reach a write it expects to block in
    /// instead of guessing how long that takes.
    #[cfg(test)]
    pub(super) before_dashboard_write_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(feature = "acceptance-diagnostics")]
    dashboard_monitor: Option<DashboardQueueMonitor>,
}

impl ServerState {
    /// Only the selected native Claude invocation can replace the retained source.
    fn refresh_claude_quota(&self) {
        let old = self.quotas.lock().unwrap().claude.clone();
        let session = self.chosen_claude_session(&old);
        let probe_enabled = self.quota_settings().claude_probe;
        let now = quota::clock_ms();
        let fresh_managed = quota_probe::fresh_managed_claude(self, now);
        let (auth, account, probe) = {
            let mut refresh = self.quota_refresh.lock().unwrap();
            if !probe_enabled {
                refresh.claude_probe = None;
            }
            let probe = refresh
                .claude_probe
                .clone()
                .filter(|_| probe_enabled && !fresh_managed);
            (
                refresh.claude_auth.clone(),
                refresh.claude_account.clone(),
                probe,
            )
        };
        let next = claude_allowance::merge(
            &old,
            session.as_ref(),
            probe.as_ref(),
            auth.as_ref(),
            account.as_ref(),
            probe_enabled,
            now,
        );
        if next != old {
            let before_state = old.state;
            let before_reason = old.reason.clone();
            let after_state = next.state;
            let after_reason = next.reason.clone();
            let mut quotas = self.quotas.lock().unwrap();
            quotas.claude = next;
            // Sent even when it is the default: leaving a not-signed-in
            // state returns the row to it.
            let snapshot = quotas.clone();
            drop(quotas);
            self.send_quotas(snapshot);
            quota::note_transition(
                self,
                ovrcr_protocol::QuotaProvider::Claude,
                before_state,
                before_reason.as_deref(),
                after_state,
                after_reason.as_deref(),
            );
        }
    }

    /// The focused Claude session, else the session named on the previous row.
    fn chosen_claude_session(
        &self,
        previous: &ovrcr_protocol::ProviderQuota,
    ) -> Option<claude_allowance::ChosenSession> {
        use ovrcr_protocol::{AgentProvider, QuotaSource, ReporterHealth};
        let focused = self.dashboard.view().and_then(|view| view.focused);
        let selected = focused
            .and_then(|id| self.sessions.lock().unwrap().get(&id).cloned())
            .filter(|session| {
                session
                    .summary()
                    .agent
                    .as_ref()
                    .is_some_and(|agent| agent.binding.provider == AgentProvider::Claude)
            })
            .or_else(|| match &previous.source {
                Some(QuotaSource::Session { session, .. }) => {
                    self.sessions.lock().unwrap().get(session).cloned()
                }
                _ => None,
            })?;
        let summary = selected.summary();
        if let Some(quota) = selected.quota_snapshot() {
            return Some(claude_allowance::ChosenSession::Reported(quota));
        }
        if let Some(agent) = summary
            .agent
            .filter(|agent| agent.binding.provider == AgentProvider::Claude)
        {
            return Some(claude_allowance::ChosenSession::Waiting {
                source: QuotaSource::Session {
                    session: summary.id,
                    run: summary.run,
                    binding: agent.binding,
                },
                live_connected: summary.phase.is_live()
                    && agent.health.state == ReporterHealth::Connected,
            });
        }
        Some(claude_allowance::ChosenSession::Other)
    }

    /// The default (quota off, nothing reported) is what a new Dashboard
    /// already holds, so it is not sent; a settings change sends it anyway.
    fn publish_quotas(&self) {
        let snapshot = self.quotas.lock().unwrap().clone();
        if snapshot != ovrcr_protocol::QuotaSnapshot::default() {
            self.send_quotas(snapshot);
        }
    }

    fn send_quotas(&self, snapshot: ovrcr_protocol::QuotaSnapshot) {
        self.dashboard
            .try_send(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
                snapshot,
            ))));
    }

    /// Send the Server's stored reading of the settings document, the one its
    /// workers use. The watcher (`watch.rs`) keeps it within 2 s of the file.
    pub(super) fn publish_settings(&self) {
        let report = self.settings.lock().unwrap().report.clone();
        self.dashboard
            .try_send(ServerMessage::Event(ServerEvent::SettingsChanged(
                Box::new(report),
            )));
    }

    pub fn hierarchy(&self) -> HierarchySnapshot {
        self.observe_checkouts();
        snapshot_from_state(self)
    }

    fn summary_for_record(
        record: &RetainedSession,
        session: Option<&Arc<Session>>,
        boot_id: Option<&str>,
    ) -> SessionSummary {
        // The caller has dropped `retained`. Reading the session takes its
        // terminal lock; doing that while holding the store stalls every
        // retained_sessions update behind the parser.
        if let Some(session) = session.filter(|session| session.run() == record.run) {
            let mut summary = session.summary();
            summary.title = record.effective_title();
            summary.manual_title = record.metadata.pinned_title.clone();
            if !summary.phase.is_live() || record.conversation.is_some() || record.identity_invalid
            {
                let mut recovery = record.recovery(boot_id);
                if summary.phase.is_live() {
                    recovery.requires_ack = false;
                    recovery.attached = !record.identity_invalid
                        && summary.agent.as_ref().is_some_and(|agent| {
                            record
                                .conversation
                                .as_ref()
                                .is_some_and(|reference| reference.matches_binding(&agent.binding))
                        });
                }
                summary.recovery = Some(recovery);
            }
            return summary;
        }
        record.summary(boot_id)
    }

    pub(crate) fn session_summary(&self, id: SessionId) -> Option<SessionSummary> {
        let (record, session, boot_id) = {
            let retained = self.retained.lock();
            let record = retained.get(id)?.clone();
            let session = self.sessions.lock().unwrap().get(&id).cloned();
            let boot_id = retained.boot_id().map(str::to_owned);
            (record, session, boot_id)
        };
        Some(Self::summary_for_record(
            &record,
            session.as_ref(),
            boot_id.as_deref(),
        ))
    }

    pub(crate) fn session_summaries(&self) -> Vec<SessionSummary> {
        let (rows, boot_id) = {
            let retained = self.retained.lock();
            let sessions = self.sessions.lock().unwrap();
            let rows = retained
                .records()
                .map(|record| (record.clone(), sessions.get(&record.id).cloned()))
                .collect::<Vec<_>>();
            let boot_id = retained.boot_id().map(str::to_owned);
            (rows, boot_id)
        };
        rows.into_iter()
            .map(|(record, session)| {
                Self::summary_for_record(&record, session.as_ref(), boot_id.as_deref())
            })
            .collect()
    }

    pub(crate) fn persist_session_titles(&self, session: &Session) -> Result<()> {
        let titles = session.title_snapshot();
        self.retained.lock().update_titles(
            session.id(),
            session.run(),
            titles.revision,
            titles.pinned,
            titles.application,
        )?;
        Ok(())
    }

    pub(crate) fn persist_session_exit(&self, session: &Session) -> Result<()> {
        self.persist_session_titles(session)?;
        let summary = session.summary();
        let mut retained = self.retained.lock();
        if matches!(summary.phase, SessionPhase::Exited { .. })
            && retained.get(session.id()).is_some_and(|record| {
                record.conversation.is_some()
                    && !record.identity_invalid
                    && !record.stopped
                    && record.failure.is_none()
            })
        {
            // Exit is not interruption or proof that descendants stopped. Persist
            // the explicit-action diagnostic without changing the ownership evidence.
            let message = if summary.agent.is_none() {
                "Agent exited before conversation attachment was confirmed; check native reporting and history, then Retry with Enter"
            } else {
                "Agent exited; press Enter to resume the conversation"
            };
            retained.record_failure(session.id(), session.run(), message.into())?;
        }
        Ok(())
    }

    pub fn inventory(&self) -> (Registry, Vec<SessionSummary>) {
        let mut registry = self.registry.lock().unwrap().clone();
        git::observe_registry(&mut registry);
        let sessions = self.session_summaries();
        (registry, sessions)
    }

    pub fn read_terminal(
        &self,
        id: SessionId,
        max_lines: Option<usize>,
    ) -> Result<(TerminalSize, String)> {
        if max_lines == Some(0) {
            return Err(lifecycle_error(
                ErrorCode::InvalidRequest,
                "max_lines must be greater than zero",
            ));
        }
        let session = self.session_for_control(id)?;
        let (size, text) = session.terminal_text();
        let text = match max_lines {
            Some(max_lines) => {
                let lines = text.lines().collect::<Vec<_>>();
                lines[lines.len().saturating_sub(max_lines)..].join("\n")
            }
            None => text,
        };
        Ok((size, text))
    }

    pub fn send_terminal(&self, id: SessionId, text: &str, submit: bool) -> Result<()> {
        let session = self.session_for_control(id)?;
        if matches!(session.summary().phase, SessionPhase::Exited { .. }) {
            return Err(lifecycle_error(ErrorCode::Conflict, "session has exited"));
        }
        session
            .send_text(text, submit)
            .map_err(|error| lifecycle_error(input_error_code(&error), error_chain_string(&error)))
    }

    /// Write one named key to `expected_run`. Focus is read here, at write
    /// time: the Dashboard owns the keyboard of the session it is focused on.
    pub fn keystroke(&self, id: SessionId, expected_run: SessionRunId, key: &str) -> Result<()> {
        let (key, modifiers) = ovrcr_terminal::key::parse_keystroke(key)
            .map_err(|message| lifecycle_error(ErrorCode::InvalidRequest, message))?;
        let session = self.session_for_control(id)?;
        if session.run() != expected_run {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session changed; refresh inventory",
            ));
        }
        if self.dashboard.view().and_then(|view| view.focused) == Some(id) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "the Dashboard is focused on this session; focus another session or detach first",
            ));
        }
        session
            .send_key(key, modifiers)
            .map_err(|error| lifecycle_error(input_error_code(&error), error_chain_string(&error)))
    }

    pub fn close_terminal(
        &self,
        id: SessionId,
        expected_run: SessionRunId,
        grace: Duration,
    ) -> Result<()> {
        // Exited rows can be filed immediately. This does not acknowledge
        // ownership uncertainty or signal a remembered process.
        {
            let _mutation = self.mutation_lock.lock().unwrap();
            self.reject_if_stopping()?;
            let summary = self
                .session_summary(id)
                .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
            if summary.run != expected_run || summary.archived {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "session changed; refresh inventory",
                ));
            }
            if !summary.phase.is_live() {
                return self.archive_session_locked(id, expected_run);
            }
        }
        let target = self.control_target(id, Some(expected_run))?;
        let termination = match &target.session {
            Some(session) => session.terminate(grace),
            None => Err(anyhow::Error::new(AlreadyExited)),
        };
        let _mutation = self.mutation_lock.lock().unwrap();
        self.ensure_control_target(id, &target)?;
        let termination = self.finish_control_stop(id, &target, termination);
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, termination, refresh)?;
        self.archive_session_locked(id, expected_run)
    }

    fn archive_session_locked(&self, id: SessionId, run: SessionRunId) -> Result<()> {
        self.retained.lock().set_archived(id, run, true)?;
        if let Some(session) = self.sessions.lock().unwrap().remove(&id) {
            session.revoke_hook_capability();
        }
        self.dashboard.forget_session(id);
        Ok(())
    }

    pub fn delete_archived_session(&self, id: SessionId, expected: SessionRunId) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let summary = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
        if summary.run != expected || !summary.archived {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session archive state changed; refresh inventory",
            ));
        }
        self.retained.lock().remove(id, expected)?;
        Ok(())
    }

    pub fn unarchive_session(&self, id: SessionId, expected: SessionRunId) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let summary = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
        if summary.run != expected || !summary.archived {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session archive state changed; refresh inventory",
            ));
        }
        self.retained.lock().set_archived(id, expected, false)?;
        Ok(())
    }

    /// Capture the owned run before waiting without the mutation lock.
    fn control_target(
        &self,
        id: SessionId,
        expected_run: Option<SessionRunId>,
    ) -> Result<SessionControlTarget> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let summary = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
        if expected_run.is_some_and(|expected| summary.run != expected) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session run changed; refresh inventory",
            ));
        }
        if summary
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack)
        {
            return Err(lifecycle_error(
                ErrorCode::OwnershipUncertain,
                "previous processes may still be running; explicitly acknowledge they stopped",
            ));
        }
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .filter(|session| session.run() == summary.run)
            .cloned();
        if let Some(session) = &session {
            session.revoke_hook_capability();
        }
        Ok(SessionControlTarget {
            run: summary.run,
            session,
            already_exited: !summary.phase.is_live(),
        })
    }

    fn persist_control_stop(&self, id: SessionId, target: &SessionControlTarget) -> Result<()> {
        if let Some(session) = &target.session {
            self.persist_session_titles(session)?;
        }
        self.retained.lock().mark_stopped(id, target.run)?;
        Ok(())
    }

    fn finish_control_stop(
        &self,
        id: SessionId,
        target: &SessionControlTarget,
        termination: Result<()>,
    ) -> Result<()> {
        match termination {
            Ok(()) if target.session.is_some() => self.persist_control_stop(id, target),
            Ok(()) => Err(anyhow::Error::new(AlreadyExited)),
            Err(error) if error.is::<AlreadyExited>() && target.already_exited => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn ensure_control_target(&self, id: SessionId, target: &SessionControlTarget) -> Result<()> {
        let current = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session no longer exists"))?;
        let sessions = self.sessions.lock().unwrap();
        let same_process = match (target.session.as_ref(), sessions.get(&id)) {
            (Some(expected), Some(current)) => Arc::ptr_eq(expected, current),
            (None, None) => true,
            _ => false,
        };
        if current.run != target.run || !same_process {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session was reopened; the replacement run was not controlled",
            ));
        }
        Ok(())
    }

    pub fn set_session_paused(&self, id: SessionId, paused: bool) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        let control = session.set_paused(paused).map(|_| ());
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, control, refresh)
    }

    pub fn handle_request(&self, role: &mut ClientRole, request: Request) -> Response {
        handle_request_with_id(self, role, request, 0, None)
    }

    pub fn create_session(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, None)
    }

    #[cfg(test)]
    pub(crate) fn create_session_with_ready(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
        ready: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, Some(ready))
    }

    /// Admission stays locked from the live-slot check through durable intent and publication.
    fn create_session_locked(
        &self,
        mut request: ovrcr_protocol::CreateSessionRequest,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<SessionSummary> {
        if request.argv.is_empty() {
            return Err(lifecycle_error(
                ErrorCode::InvalidRequest,
                "session command cannot be empty",
            ));
        }
        let workspace_record = self
            .registry
            .lock()
            .unwrap()
            .workspace(&request.project, &request.workspace)
            .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
            .clone();
        let cwd = workspace_record.path.clone();
        if !cwd.is_dir() {
            return Err(lifecycle_error(
                ErrorCode::NotFound,
                "working directory is unavailable",
            ));
        }
        let display =
            git::checkout_name(&cwd).unwrap_or_else(|_| git::UNAVAILABLE_CHECKOUT.to_owned());
        let automatic = request.name.is_empty();
        {
            let retained = self.retained.lock();
            let used: std::collections::HashSet<_> = retained
                .records()
                .filter(|record| {
                    record.metadata.project == request.project
                        && record.metadata.workspace == request.workspace
                })
                .map(|record| record.metadata.name.as_str())
                .collect();
            if automatic {
                request.name = display.clone();
                let mut suffix = 2u64;
                while used.contains(request.name.as_str()) {
                    request.name = format!("{display}-{suffix}");
                    suffix += 1;
                }
            } else if used.contains(request.name.as_str()) {
                return Err(lifecycle_error(
                    ErrorCode::AlreadyExists,
                    "duplicate session name",
                ));
            }
        }
        let pinned_title = if automatic || matches!(request.kind, SessionKind::Agent { .. }) {
            None
        } else {
            Some(
                crate::session::sanitize_title(&request.name).ok_or_else(|| {
                    lifecycle_error(ErrorCode::InvalidRequest, "title must contain visible text")
                })?,
            )
        };
        self.check_live_capacity()?;
        let label = default_session_label(&request.kind, request.label, &request.argv);
        let record = self.retained.lock().create(SessionMetadata {
            project: request.project,
            workspace: request.workspace,
            name: request.name,
            label,
            cwd,
            kind: request.kind,
            pinned_title,
            application_title: None,
        })?;
        let result = self.spawn_record_locked(record.id, record.run, request.argv, ready, None);
        if let Err(error) = &result {
            let removed = {
                let mut retained = self.retained.lock();
                let current = retained.get(record.id).map(|record| record.run);
                match current {
                    Some(run) if run.0 == 0 || error.is::<crate::session::NoProcessStarted>() => {
                        retained.remove(record.id, run)?
                    }
                    _ => false,
                }
            };
            if removed {
                self.dashboard.forget_session(record.id);
            }
        }
        result
    }

    fn check_live_capacity(&self) -> Result<()> {
        if self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.is_live())
            .count()
            >= MAX_LIVE_SESSIONS
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "all 50 live process slots are occupied",
            ));
        }
        Ok(())
    }

    /// The mutation lock is the slot reservation: no other admission can pass while spawning.
    fn spawn_record_locked(
        &self,
        id: SessionId,
        expected_run: SessionRunId,
        argv: Vec<std::ffi::OsString>,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
        restore_command: Option<String>,
    ) -> Result<SessionSummary> {
        self.check_live_capacity()?;
        let cwd = self
            .retained
            .lock()
            .get(id)
            .map(|record| record.metadata.cwd.clone());
        if let Some(cwd) = cwd {
            self.refuse_root_launch(&cwd)?;
        }
        let socket = validate_bound_socket(&self.socket)?;
        let capability = generate_hook_capability()?;
        let events = self
            .events
            .lock()
            .unwrap()
            .as_ref()
            .context("server event channel closed")?
            .clone();
        let record = self.retained.lock().begin_run(id, expected_run)?;
        if let Some(old) = self.sessions.lock().unwrap().remove(&id) {
            old.revoke_hook_capability();
        }
        self.dashboard.forget_session(id);
        let metadata = &record.metadata;
        let spec = SessionSpec {
            restore_command,
            run: record.run,
            kind: metadata.kind.clone(),
            project: metadata.project.clone(),
            workspace: metadata.workspace.clone(),
            name: metadata.name.clone(),
            label: metadata.label.clone(),
            cwd: metadata.cwd.clone(),
            argv,
            hook_env: Some(HookEnvironment {
                socket,
                session: id,
                capability,
            }),
        };
        let size = self.dashboard.geometry().unwrap_or(TerminalSize {
            rows: 40,
            cols: 120,
        });
        let register = |session: &Arc<Session>| {
            session.restore_titles(
                metadata.pinned_title.clone(),
                metadata.application_title.clone(),
            );
            self.sessions
                .lock()
                .unwrap()
                .insert(id, Arc::clone(session));
            if let Some(ready) = ready.as_ref() {
                ready();
            }
        };
        if let Err(error) = Session::spawn_registered(id, spec, size, events, &register) {
            if let Some(failed) = self.sessions.lock().unwrap().remove(&id) {
                failed.revoke_hook_capability();
            }
            let no_process = error.is::<crate::session::NoProcessStarted>();
            let message = if no_process {
                "Launch failed before a process started; check the executable and retry"
            } else {
                "Launch did not complete; confirm previous processes stopped before retrying"
            };
            let persisted = {
                let mut retained = self.retained.lock();
                if no_process {
                    retained
                        .mark_stopped(id, record.run)
                        .and_then(|_| retained.record_failure(id, record.run, message.into()))
                } else {
                    retained.record_failure(id, record.run, message.into())
                }
            };
            if let Err(error) = persisted {
                eprintln!("retain session {} launch failure: {error:#}", id.0);
            }
            let failure = lifecycle_error_with_hierarchy(
                if no_process {
                    ErrorCode::PartialFailure
                } else {
                    ErrorCode::OwnershipUncertain
                },
                format!("session {} retained: {message}", id.0),
                true,
            );
            return Err(if no_process {
                failure.context(crate::session::NoProcessStarted)
            } else {
                failure
            });
        }
        self.session_summary(id)
            .context("published session disappeared")
    }

    pub fn set_session_title(&self, id: SessionId, title: Option<String>) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let clearing = title.is_none();
        let session = self.sessions.lock().unwrap().get(&id).cloned();
        if let Some(session) = session {
            session
                .set_title(title)
                .map_err(|error| lifecycle_error(ErrorCode::InvalidRequest, error.to_string()))?;
            let titles = session.title_snapshot();
            let mut retained = self.retained.lock();
            retained
                .update_titles(
                    session.id(),
                    session.run(),
                    titles.revision,
                    titles.pinned,
                    titles.application,
                )
                .map_err(|error| {
                    lifecycle_error_with_hierarchy(
                        ErrorCode::PartialFailure,
                        format!("live title changed but could not be retained: {error}"),
                        true,
                    )
                })?;
            if clearing {
                Self::dismiss_recorded_subject(&mut retained, id, session.run())?;
            }
        } else {
            let mut retained = self.retained.lock();
            let record = retained
                .get(id)
                .cloned()
                .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
            if record.disposition == crate::retained::Disposition::Archived {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "session is archived; unarchive before renaming",
                ));
            }
            let title = title
                .map(|title| {
                    crate::session::sanitize_title(&title).ok_or_else(|| {
                        lifecycle_error(
                            ErrorCode::InvalidRequest,
                            "title must contain visible text",
                        )
                    })
                })
                .transpose()?;
            let revision = record
                .title_revision
                .checked_add(1)
                .context("title revision exhausted")?;
            retained.update_titles(
                id,
                record.run,
                revision,
                title,
                record.metadata.application_title,
            )?;
            if clearing {
                Self::dismiss_recorded_subject(&mut retained, id, record.run)?;
            }
        }
        self.refresh_session_locked(id)
    }

    fn dismiss_recorded_subject(
        retained: &mut crate::retained::SessionStore,
        id: SessionId,
        run: SessionRunId,
    ) -> Result<()> {
        let Some(conversation) = retained
            .get(id)
            .and_then(|record| record.conversation.as_ref())
            .map(|reference| reference.identity().to_owned())
        else {
            return Ok(());
        };
        retained.dismiss_conversation_subject(id, run, &conversation)?;
        Ok(())
    }

    pub fn acknowledge_session_stopped(
        &self,
        id: SessionId,
        expected_run: SessionRunId,
    ) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.acknowledge_stopped_locked(id, expected_run)?;
        self.refresh_session_locked(id)
    }

    fn acknowledge_stopped_locked(&self, id: SessionId, expected_run: SessionRunId) -> Result<()> {
        let summary = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
        if summary.run != expected_run {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session run changed; refresh inventory",
            ));
        }
        if summary.phase.is_live() {
            return Err(lifecycle_error(
                ErrorCode::SessionRunning,
                "the currently owned process is still live",
            ));
        }
        self.retained.lock().mark_stopped(id, expected_run)?;
        Ok(())
    }

    pub fn reopen_session(
        &self,
        id: SessionId,
        expected_run: SessionRunId,
        acknowledge_stopped: bool,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reopen_session_locked(id, expected_run, acknowledge_stopped)
    }

    pub fn recover_session(
        &self,
        id: SessionId,
        expected_run: SessionRunId,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let summary = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
        // A duplicate may observe its successor, but must never launch that successor again.
        if summary.run != expected_run || summary.phase.is_live() {
            return self.reopen_session_locked(id, expected_run, false);
        }
        if !summary.can_auto_recover() {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session requires an explicit recovery action",
            ));
        }
        let result = self.reopen_session_locked(id, expected_run, false);
        if let Err(error) = &result {
            // Failures before begin_run (including capacity) must also suppress retries
            // across Dashboard reconnects. Spawn failures already record the new run.
            if self
                .retained
                .lock()
                .get(id)
                .is_some_and(|row| row.run == expected_run)
            {
                self.retained
                    .lock()
                    .record_failure(id, expected_run, error.to_string())?;
            }
        }
        result
    }

    fn reopen_session_locked(
        &self,
        id: SessionId,
        expected_run: SessionRunId,
        acknowledge_stopped: bool,
    ) -> Result<SessionSummary> {
        self.reject_if_stopping()?;
        let summary = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
        if summary.archived {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session is archived; unarchive before reopening",
            ));
        }
        if summary.run != expected_run {
            if expected_run.0.checked_add(1) == Some(summary.run.0)
                && self
                    .retained
                    .lock()
                    .get(id)
                    .is_some_and(|r| r.disposition == crate::retained::Disposition::Active)
            {
                if let Some(failure) = summary
                    .recovery
                    .as_ref()
                    .and_then(|recovery| recovery.failure.as_ref())
                {
                    return Err(lifecycle_error(ErrorCode::PartialFailure, failure.clone()));
                }
                return Ok(summary);
            }
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session run changed; refresh inventory",
            ));
        }
        if summary.phase.is_live() {
            return Ok(summary);
        }
        if let Some(reason) = summary
            .recovery
            .as_ref()
            .and_then(|recovery| recovery.unavailable.as_ref())
        {
            return Err(lifecycle_error(ErrorCode::InvalidRequest, reason.clone()));
        }
        if summary
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack)
        {
            if !acknowledge_stopped {
                return Err(lifecycle_error(
                    ErrorCode::OwnershipUncertain,
                    "confirm that the previous agent and background processes stopped; use --ack-stopped",
                ));
            }
            self.acknowledge_stopped_locked(id, expected_run)?;
        }
        let record = self
            .retained
            .lock()
            .get(id)
            .cloned()
            .context("retained session disappeared")?;
        if !record.metadata.cwd.is_dir() {
            let message = format!(
                "Recorded working directory is unavailable: {}",
                record.metadata.cwd.display()
            );
            self.retained
                .lock()
                .record_failure(id, expected_run, message.clone())?;
            return Err(lifecycle_error(ErrorCode::NotFound, message));
        }
        let shell = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .ok_or_else(|| {
                lifecycle_error(
                    ErrorCode::InvalidRequest,
                    "SHELL is unset; configure a shell before reopening",
                )
            })?;
        let command = match &record.metadata.kind {
            SessionKind::Agent { name } => {
                let command = match record.conversation.as_ref() {
                    Some(reference) => crate::recovery::resume_argv(name, reference),
                    None => crate::recovery::picker_argv(name),
                }
                .and_then(|argv| crate::recovery::terminal_command(&argv));
                match command {
                    Ok(command) => Some(command),
                    Err(error) => {
                        let message = error.to_string();
                        self.retained
                            .lock()
                            .record_failure(id, expected_run, message.clone())?;
                        return Err(lifecycle_error(ErrorCode::InvalidRequest, message));
                    }
                }
            }
            SessionKind::Terminal => None,
        };
        let old = self.sessions.lock().unwrap().get(&id).cloned();
        if let Some(old) = old {
            match old.terminate(Duration::from_secs(2)) {
                Ok(()) => {}
                Err(error) if error.is::<AlreadyExited>() => {}
                Err(error) => return Err(error),
            }

            self.persist_session_exit(&old)?;
        }
        // Canonical PTY buffers can drop a long burst even when split over lines.
        // Stage only long commands in the new shell's transient environment and
        // submit a short eval; the native argv never travels through that buffer.
        let staged = command
            .as_ref()
            .filter(|command| command.len() > 512)
            .map(|command| command.trim_end_matches('\n').to_owned());
        let input = command.map(|command| {
            if staged.is_some() {
                "eval \"$OVRCR_RESTORE_COMMAND\"\n".to_owned()
            } else {
                command
            }
        });
        let summary = self.spawn_record_locked(id, expected_run, vec![shell], None, staged)?;
        if let Some(command) = input {
            let session = self
                .sessions
                .lock()
                .unwrap()
                .get(&id)
                .cloned()
                .context("published terminal disappeared")?;
            let events = self
                .events
                .lock()
                .unwrap()
                .as_ref()
                .context("server event channel closed")?
                .clone();
            let run = summary.run;
            // A shell can still be loading its rc files. Never hold the server's
            // mutation boundary while the PTY applies input backpressure.
            if let Err(error) = std::thread::Builder::new()
                .name("restore-input".into())
                .spawn(move || {
                    if let Err(error) = session.write(command.as_bytes()) {
                        let _ = events.send(SessionEvent::RestoreInputFailed {
                            id,
                            run,
                            message: format!("Resume command could not be sent: {error}"),
                        });
                    }
                })
            {
                self.retained
                    .lock()
                    .record_failure(id, run, error.to_string())?;
                return Err(error.into());
            }
        }
        Ok(summary)
    }

    pub fn kill_session(&self, id: SessionId, grace: Duration) -> Result<()> {
        let target = self.control_target(id, None)?;
        let termination = match &target.session {
            Some(session) => session.terminate(grace),
            None => Err(anyhow::Error::new(AlreadyExited)),
        };
        let _mutation = self.mutation_lock.lock().unwrap();
        self.ensure_control_target(id, &target)?;
        let termination = self.finish_control_stop(id, &target, termination);
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, termination, refresh)
    }

    pub fn remove_session(&self, id: SessionId) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.remove_session_locked(id)
    }

    fn remove_session_locked(&self, id: SessionId) -> Result<()> {
        let summary = self
            .session_summary(id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "session not found"))?;
        if summary.phase.is_live() {
            return Err(lifecycle_error(
                ErrorCode::SessionRunning,
                "session is still live; kill it before removal",
            ));
        }
        if summary.archived {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "archived record deletion requires its observed run; use terminal remove",
            ));
        }
        if summary
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack)
        {
            return Err(lifecycle_error(
                ErrorCode::OwnershipUncertain,
                "use terminal acknowledge-stopped before removing an ownership-uncertain record",
            ));
        }
        self.retained.lock().remove(id, summary.run)?;
        if let Some(session) = self.sessions.lock().unwrap().remove(&id) {
            session.revoke_hook_capability();
        }
        self.dashboard.forget_session(id);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_tasks_for_test(
        tasks: Arc<TaskManager>,
        registry_path: PathBuf,
    ) -> Arc<Self> {
        let (events, _) = event_channel(None);
        let (dispatch, _) = dispatch_channel(None);
        let retained = SessionStore::open(&registry_path).unwrap();
        Arc::new(Self {
            server_lifetime: navigation::new_identity().unwrap(),
            callback_executable_sha256: Some("0".repeat(64)),
            tasks: Some(tasks),
            quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
            quota_refresh: Mutex::default(),
            settings: Mutex::new(watch::Watched::empty()),
            socket: crate::config::default_socket_path(&registry_path),
            registry_path,
            registry: Mutex::new(Registry::default()),
            sessions: Mutex::new(HashMap::new()),
            dashboard: ActiveDashboard::default(),
            retained: parking_lot::Mutex::new(retained),
            observations: parking_lot::Mutex::new(HashMap::new()),
            mutation_lock: Mutex::new(()),
            lifecycle: lifecycle::Lifecycle::default(),
            dispatch,
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            events: Mutex::new(Some(events)),
            event_log: Mutex::new(event_log::Log::memory()),
            wip: WipWait::default(),
            resize_hook: Mutex::new(None),
            before_view_publish_hook: Mutex::new(None),
            before_dashboard_write_hook: Mutex::new(None),
            #[cfg(feature = "acceptance-diagnostics")]
            dashboard_monitor: None,
        })
    }
    fn save_uncommitted_work(&self) -> bool {
        self.settings
            .lock()
            .unwrap()
            .report
            .settings
            .save_uncommitted_work
    }

    /// Dirty feature worktrees that can be saved as `wip/<branch>`. The root
    /// checkout and a detached checkout are left alone.
    fn wip_targets(&self) -> Vec<WipTarget> {
        let registry = self.registry.lock().unwrap().clone();
        let mut targets = Vec::new();
        for project in &registry.projects {
            for workspace in &project.workspaces {
                if is_root_workspace(project, workspace) || !workspace.path.exists() {
                    continue;
                }
                let Ok(inspection) = git::inspect_worktree(project, workspace) else {
                    continue;
                };
                if !inspection.dirty || git::wip_refname(&inspection.branch).is_err() {
                    continue;
                }
                targets.push(WipTarget {
                    project: project.name.clone(),
                    id: workspace.id.clone(),
                    branch: inspection.branch,
                    path: inspection.canonical_path,
                });
            }
        }
        targets.sort_by(|left, right| {
            (&left.project, &left.branch).cmp(&(&right.project, &right.branch))
        });
        targets
    }

    pub(crate) fn save_workspace_wip(&self, project: &str, name: &str) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let (project_record, workspace) = {
            let registry = self.registry.lock().unwrap();
            (
                registry
                    .project(project)
                    .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
                    .clone(),
                registry
                    .workspace(project, name)
                    .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
                    .clone(),
            )
        };
        if is_root_workspace(&project_record, &workspace) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "cannot save the repository-root workspace",
            ));
        }
        let inspection = git::inspect_worktree(&project_record, &workspace)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?;
        if !inspection.dirty {
            return Ok(());
        }
        git::publish_wip(&inspection.canonical_path, &inspection.branch)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))
    }

    pub(crate) fn answer_wip_save(&self, project: &str, workspace: &str, save: bool) -> bool {
        let mut pending = self.wip.pending.lock().unwrap();
        let Some(ask) = pending.ask.as_mut() else {
            return false;
        };
        let Some(index) = ask
            .left
            .iter()
            .position(|(candidate_project, candidate_workspace)| {
                candidate_project == project && candidate_workspace == workspace
            })
        else {
            return true;
        };
        ask.left.remove(index);
        if save {
            ask.save.push((project.to_owned(), workspace.to_owned()));
        }
        self.wip.cv.notify_all();
        true
    }

    /// Save dirty feature worktrees before shutdown. With the consent setting
    /// on, every one is pushed. Otherwise an attached Dashboard is asked once
    /// per worktree; a declined worktree is left unsaved. No Dashboard and the
    /// setting off continues without saving.
    fn resolve_shutdown_wip(&self) -> Result<(), String> {
        let targets = self.wip_targets();
        if targets.is_empty() {
            return Ok(());
        }
        let save = if self.save_uncommitted_work() {
            targets
                .iter()
                .map(|target| (target.project.clone(), target.id.clone()))
                .collect()
        } else if self.dashboard.is_claimed() {
            self.ask_shutdown_wip(&targets)?
        } else {
            return Ok(());
        };
        for (project, id) in save {
            let Some(target) = targets
                .iter()
                .find(|target| target.project == project && target.id == id)
            else {
                continue;
            };
            let _mutation = self.mutation_lock.lock().unwrap();
            if self.stopping.load(Ordering::Acquire) {
                return Err("server is stopping".into());
            }
            git::publish_wip(&target.path, &target.branch).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn ask_shutdown_wip(&self, targets: &[WipTarget]) -> Result<Vec<(String, String)>, String> {
        {
            let mut pending = self.wip.pending.lock().unwrap();
            if pending.ask.is_some() {
                return Err("already confirming uncommitted work".into());
            }
            pending.ask = Some(WipAsk {
                left: targets
                    .iter()
                    .map(|target| (target.project.clone(), target.id.clone()))
                    .collect(),
                save: Vec::new(),
            });
        }
        for target in targets {
            self.dashboard
                .try_send(ServerMessage::Event(ServerEvent::WipSavePrompt {
                    project: target.project.clone(),
                    workspace: target.id.clone(),
                    branch: target.branch.clone(),
                }));
        }
        loop {
            let pending = self.wip.pending.lock().unwrap();
            let done = pending.ask.as_ref().is_some_and(|ask| ask.left.is_empty());
            let claimed = self.dashboard.is_claimed();
            if done || !claimed {
                break;
            }
            let (_pending, _) = self
                .wip
                .cv
                .wait_timeout(pending, Duration::from_millis(250))
                .unwrap_or_else(|error| error.into_inner());
        }
        let mut pending = self.wip.pending.lock().unwrap();
        Ok(pending.ask.take().map(|ask| ask.save).unwrap_or_default())
    }

    pub fn request_shutdown(&self, kill: bool) -> Response {
        let admission = self.tasks.as_ref().map(|tasks| tasks.admission_guard());
        if let Some(tasks) = &self.tasks
            && !kill
            && tasks.has_active()
        {
            return error_response(ErrorCode::SessionsRemain, "task runs remain");
        }
        {
            let _mutation = self.mutation_lock.lock().unwrap();
            if self.stopping.load(Ordering::Acquire) {
                return error_response(ErrorCode::Conflict, "server is stopping");
            }
            if !kill
                && self
                    .sessions
                    .lock()
                    .unwrap()
                    .values()
                    .any(|session| session.is_live())
            {
                return error_response(ErrorCode::SessionsRemain, "live sessions remain");
            }
        }
        if let Err(error) = self.resolve_shutdown_wip() {
            return error_response(ErrorCode::Conflict, error);
        }
        let _mutation = self.mutation_lock.lock().unwrap();
        if self.stopping.load(Ordering::Acquire) {
            return error_response(ErrorCode::Conflict, "server is stopping");
        }
        let sessions = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if !kill && sessions.iter().any(|session| session.is_live()) {
            return error_response(ErrorCode::SessionsRemain, "live sessions remain");
        }
        if kill {
            // Terminate concurrently: each session may wait out the whole
            // grace period, so a serial loop costs sessions x grace.
            let grace = requested_kill_grace();
            let workers = sessions
                .into_iter()
                .map(|session| {
                    let id = session.id();
                    session.revoke_hook_capability();
                    let worker = thread::Builder::new()
                        .name(format!("ovrcr-shutdown-kill-{}", id.0))
                        .spawn(move || session.terminate(grace).map(|_| ()));
                    (id, worker)
                })
                .collect::<Vec<_>>();
            let mut failures = Vec::new();
            for (id, worker) in workers {
                let termination = match worker {
                    Ok(handle) => handle
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("termination worker panicked"))),
                    Err(error) => Err(error).context("spawn termination worker"),
                };
                let verified = termination.is_ok();
                let termination = match termination {
                    Ok(()) => Ok(()),
                    Err(error) if error.is::<AlreadyExited>() => Ok(()),
                    Err(error) => Err(error),
                };
                let termination = termination.and_then(|()| {
                    let session = self.sessions.lock().unwrap().get(&id).cloned();
                    let Some(session) = session else {
                        return Ok(());
                    };
                    if verified {
                        self.persist_control_stop(
                            id,
                            &SessionControlTarget {
                                run: session.run(),
                                session: Some(session),
                                already_exited: false,
                            },
                        )
                    } else {
                        self.persist_session_exit(&session)
                    }
                });

                let refresh = self.refresh_session_locked(id);
                if let Err(error) = combine_control_and_refresh(id, termination, refresh) {
                    failures.push(format!("session {}: {}", id.0, error_chain_string(&error)));
                }
            }
            if !failures.is_empty() {
                return error_response(ErrorCode::PartialFailure, failures.join("; "));
            }
        }
        self.stopping.store(true, Ordering::Release);
        if let Some(tasks) = &self.tasks {
            tasks.quiesce();
        }
        drop(_mutation);
        drop(admission);
        if let Some(tasks) = &self.tasks
            && let Err(error) = tasks.stop()
        {
            return error_response(
                ErrorCode::PartialFailure,
                format!("task shutdown: {error:#}"),
            );
        }
        Response::Ok
    }

    fn reject_if_stopping(&self) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            bail!("server is stopping")
        }
        Ok(())
    }

    fn session_for_control(&self, id: SessionId) -> Result<Arc<Session>> {
        let session = self.sessions.lock().unwrap().get(&id).cloned();
        if let Some(session) = session {
            return Ok(session);
        }
        if self.retained.lock().get(id).is_some() {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session has no current terminal; explicitly reopen it",
            ));
        }
        Err(lifecycle_error(
            ErrorCode::NotFound,
            format!("session {} not found", id.0),
        ))
    }

    fn refresh_session_locked(&self, id: SessionId) -> Result<()> {
        let error = match self
            .dispatch
            .try_send(DispatchMessage::RefreshSession { session: id })
        {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(_)) => "dispatcher queue is full",
            Err(mpsc::TrySendError::Disconnected(_)) => "dispatcher is unavailable",
        };
        if let Some(snapshot) = self.dashboard.snapshot() {
            self.dashboard.disconnect(snapshot);
        }
        bail!("{error}")
    }

    pub fn add_project(&self, name: String, repo: PathBuf, workspace_root: PathBuf) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        // A relative path resolves against the server's working directory, not the caller's, so
        // `workspaces/demo` would be created and registered somewhere the caller never named.
        for path in [&repo, &workspace_root] {
            if !path.is_absolute() {
                return Err(lifecycle_error(
                    ErrorCode::InvalidRequest,
                    format!("path must be absolute: {}", path.display()),
                ));
            }
        }
        // The repository is validated before the workspace root is created, so a repository that
        // is missing or not its own worktree root leaves no empty directory behind.
        let repo = git::validate_repo(&repo)?;
        let existing = {
            let registry = self.registry.lock().unwrap();
            registry
                .projects
                .iter()
                .find(|project| project.name == name)
                .cloned()
        };
        if let Some(existing) = existing {
            if existing.repo != repo {
                return Err(lifecycle_error(
                    ErrorCode::AlreadyExists,
                    format!("duplicate project: {name}"),
                ));
            }
            let has_root = existing
                .workspaces
                .iter()
                .any(|workspace| is_root_workspace(&existing, workspace));
            if !has_root {
                return self.ensure_one_root_locked(&name);
            }
            let pending = existing.workspaces.iter().any(|workspace| {
                is_root_workspace(&existing, workspace) && workspace.setup_pending
            });
            if !pending {
                return Err(lifecycle_error(
                    ErrorCode::AlreadyExists,
                    format!("duplicate project: {name}"),
                ));
            }
            return self.complete_root_setup_locked(&name);
        }
        self.require_root_default(&repo)?;
        if !workspace_root.exists() {
            fs::create_dir_all(&workspace_root)
                .with_context(|| format!("create workspace root {}", workspace_root.display()))?;
        }
        let workspace_root = git::validate_workspace_root(&workspace_root)?;
        let root = self.root_record_for(&repo)?;
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        next.add_project(ProjectRecord {
            name: name.clone(),
            repo,
            workspace_root,
            workspaces: Vec::new(),
        })?;
        next.add_workspace(&name, root)?;
        save_registry_atomic(&next, &self.registry_path)?;
        *registry = next;
        drop(registry);
        self.complete_root_setup_locked(&name)
    }

    pub(crate) fn task_mutation_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.mutation_lock.lock().unwrap()
    }
    pub fn remove_project(&self, name: &str) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        if self
            .tasks
            .as_ref()
            .is_some_and(|tasks| tasks.references_project(name))
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "project is referenced by a scheduled task",
            ));
        }
        let project = self
            .registry
            .lock()
            .unwrap()
            .projects
            .iter()
            .find(|project| project.name == name)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("project not found: {name}"))
            })?;
        if project
            .workspaces
            .iter()
            .any(|workspace| !is_root_workspace(&project, workspace))
        {
            return Err(lifecycle_error(
                ErrorCode::WorkspacesRemain,
                format!("cannot remove project {name}: workspaces remain"),
            ));
        }
        let occupied = self.session_summaries().iter().any(|session| {
            session.project == name
                && (session.phase.is_live()
                    || session
                        .recovery
                        .as_ref()
                        .is_some_and(|recovery| recovery.requires_ack))
        });
        if occupied {
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!(
                    "live or ownership-uncertain sessions remain for project {name}; stop live sessions or acknowledge stopped processes before removal"
                ),
            ));
        }
        for workspace in &project.workspaces {
            if let Some(tasks) = &self.tasks
                && tasks.occupies_workspace(name, &workspace.id)?
            {
                return Err(lifecycle_error(
                    ErrorCode::SessionsRemain,
                    format!("task run remains for workspace {name}/{}", workspace.id),
                ));
            }
        }
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        let root_ids: Vec<String> = project
            .workspaces
            .iter()
            .map(|workspace| workspace.id.clone())
            .collect();
        for id in &root_ids {
            next.remove_workspace(name, id)?;
        }
        next.remove_project(name)?;
        let archived_all = if let Some(root_id) = root_ids.first() {
            self.retained
                .lock()
                .archive_workspace(&next, name, root_id, || Ok(()))?
        } else {
            save_registry_atomic(&next, &self.registry_path)?;
            Vec::new()
        };
        *registry = next;
        drop(registry);
        for id in archived_all {
            if let Some(session) = self.sessions.lock().unwrap().remove(&id) {
                session.revoke_hook_capability();
            }
            self.dashboard.forget_session(id);
        }
        Ok(())
    }

    pub fn create_workspace(
        &self,
        project: String,
        name: String,
        branch: BranchRequest,
    ) -> Result<()> {
        // Feature worktrees are never the default-branch root workspace.
        let launch = if crate::settings::load(&self.registry_path)
            .settings
            .automatic_local_terminals
            .should_create(false)
        {
            WorkspaceLaunch::Shell
        } else {
            WorkspaceLaunch::None
        };
        self.create_workspace_inner(project, name, branch, launch)
            .map(|_| ())
    }

    pub fn create_task_workspace(
        &self,
        project: String,
        name: String,
        branch: String,
        base: String,
    ) -> Result<()> {
        self.create_workspace_inner(
            project,
            name,
            BranchRequest::New { branch, base },
            WorkspaceLaunch::None,
        )
        .map(|_| ())
    }

    pub fn create_workspace_with_launch(
        &self,
        project: String,
        name: String,
        branch: BranchRequest,
        launch: Option<ovrcr_protocol::SessionLaunch>,
    ) -> Result<Option<SessionSummary>> {
        self.create_workspace_inner(
            project,
            name,
            branch,
            launch.map_or(WorkspaceLaunch::None, WorkspaceLaunch::Session),
        )
    }

    fn create_workspace_inner(
        &self,
        project: String,
        name: String,
        branch: BranchRequest,
        launch: WorkspaceLaunch,
    ) -> Result<Option<SessionSummary>> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let project_record = self
            .registry
            .lock()
            .unwrap()
            .project(&project)
            .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
            .clone();
        if project_record
            .workspaces
            .iter()
            .any(|workspace| workspace.id == name)
        {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                format!("duplicate workspace: {project}/{name}"),
            ));
        }
        let branch = match branch {
            BranchRequest::New { branch, base } => BranchSpec::New { branch, base },
            BranchRequest::Existing { branch } => BranchSpec::Existing { branch },
        };
        let workspace = git::create_worktree(&project_record, &name, branch)
            .with_context(|| format!("create workspace {project}/{name}"))?;
        {
            let mut registry = self.registry.lock().unwrap();
            let mut next = registry.clone();
            next.add_workspace(&project, workspace.clone())?;
            if let Err(error) = save_registry_atomic(&next, &self.registry_path) {
                return Err(lifecycle_error(
                    ErrorCode::PartialFailure,
                    format!(
                        "registry write failed: worktree remains at {}: {}",
                        workspace.path.display(),
                        error_chain_string(&error)
                    ),
                ));
            }
            *registry = next;
        }
        let (launch, session_name) = match launch {
            WorkspaceLaunch::None => return Ok(None),
            WorkspaceLaunch::Session(launch) => (launch, String::new()),
            WorkspaceLaunch::Shell => {
                let shell = std::env::var_os("SHELL").ok_or_else(|| {
                    lifecycle_error_with_hierarchy(
                        ErrorCode::PartialFailure,
                        format!(
                            "worktree for workspace {} exists at {} but SHELL is unset",
                            name,
                            workspace.path.display()
                        ),
                        true,
                    )
                })?;
                (
                    ovrcr_protocol::SessionLaunch {
                        argv: vec![shell],
                        label: None,
                        kind: SessionKind::Terminal,
                    },
                    "local".into(),
                )
            }
        };
        self.create_session_locked(
            ovrcr_protocol::CreateSessionRequest {
                project,
                workspace: name.clone(),
                name: session_name,
                label: launch.label,
                argv: launch.argv,
                kind: launch.kind,
            },
            None,
        )
        .map(Some)
        .map_err(|error| {
            if error
                .downcast_ref::<LifecycleFailure>()
                .is_some_and(|failure| failure.code == ErrorCode::OwnershipUncertain)
            {
                return error;
            }
            lifecycle_error_with_hierarchy(
                ErrorCode::PartialFailure,
                format!(
                    "worktree for workspace {} remains at {}: {}",
                    name,
                    workspace.path.display(),
                    error_chain_string(&error)
                ),
                true,
            )
        })
    }

    pub fn remove_workspace(&self, project: &str, name: &str, force: bool) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.remove_workspace_locked(project, name, None, force)
    }

    pub(crate) fn remove_task_workspace(
        &self,
        snapshot: &crate::tasks::Run,
        run_dir: &Path,
    ) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        // Another cleanup may have consumed ownership while this request waited for the lock.
        let run = if run_dir.join("run.json").exists() {
            crate::tasks::read_run(run_dir)?
        } else {
            snapshot.clone()
        };
        if !run.status.is_terminal() {
            bail!("run is still active");
        }
        let Some(cwd) = &run.directory else {
            return Ok(());
        };
        let crate::tasks::TaskTarget::Git { project, .. } = &run.spec.target else {
            bail!("run does not own a Git workspace");
        };
        if !cwd.exists() {
            if run.workspace.is_some() {
                let mut cleaned = run;
                cleaned.workspace = None;
                crate::tasks::write_run(run_dir, &cleaned)?;
            }
            return Ok(());
        }
        let name = run
            .workspace
            .as_deref()
            .context("retained worktree is not owned by this run; inspect it before cleanup")?;
        self.registry
            .lock()
            .unwrap()
            .workspace(project, name)
            .context("retained worktree is not registered; inspect it before cleanup")?;
        self.remove_workspace_locked(project, name, Some((&run, run_dir)), false)
    }

    fn remove_workspace_locked(
        &self,
        project: &str,
        name: &str,
        cleanup: Option<(&crate::tasks::Run, &Path)>,
        mut force: bool,
    ) -> Result<()> {
        let (project_record, workspace, repository_roots) = {
            let registry = self.registry.lock().unwrap();
            (
                registry
                    .project(project)
                    .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
                    .clone(),
                registry
                    .workspace(project, name)
                    .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
                    .clone(),
                registry
                    .projects
                    .iter()
                    .map(|project| project.repo.clone())
                    .collect::<Vec<_>>(),
            )
        };
        // Another project may register this checkout as its protected root.
        if repository_roots
            .iter()
            .any(|root| same_path(root, &workspace.path))
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "cannot remove the repository-root workspace",
            ));
        }
        if let Some(tasks) = &self.tasks
            && tasks.occupies_workspace(project, name)?
        {
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!("task run remains for workspace {project}/{name}"),
            ));
        }
        // Force stands in for the per-row acknowledgement; it never kills a live process.
        let (live, uncertain): (Vec<_>, Vec<_>) = self
            .session_summaries()
            .into_iter()
            .filter(|session| {
                session.project == project
                    && session.workspace == name
                    && (session.phase.is_live()
                        || session
                            .recovery
                            .as_ref()
                            .is_some_and(|recovery| recovery.requires_ack))
            })
            .partition(|session| session.phase.is_live());
        if !live.is_empty() || (!uncertain.is_empty() && !force) {
            // Archived rows are hidden from the default listing, so the refusal
            // must name every blocker and the command that clears it.
            let describe = |session: &SessionSummary| {
                format!(
                    "#{}{}",
                    session.id.0,
                    if session.archived { " (archived)" } else { "" }
                )
            };
            let mut hints = Vec::new();
            if !live.is_empty() {
                hints.push(format!(
                    "live: {}; kill or close them",
                    live.iter().map(describe).collect::<Vec<_>>().join(", ")
                ));
            }
            if !uncertain.is_empty() && !force {
                hints.push(format!(
                    "unacknowledged stopped: {}; run `ovrcr terminal acknowledge-stopped <id>` or `ovrcr terminal remove <id>`, or remove with --force",
                    uncertain.iter().map(describe).collect::<Vec<_>>().join(", ")
                ));
            }
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!(
                    "sessions block removal of workspace {project}/{name}: {}",
                    hints.join("; ")
                ),
            ));
        }
        // Git also deletes ignored files. A clean worktree is not permission to
        // delete a provider's recorded history, even for an archived session.
        let history_holders = self
            .retained
            .lock()
            .records()
            .filter(|record| {
                let history = match record.conversation.as_ref() {
                    Some(ovrcr_protocol::ConversationReference::Claude(reference)) => {
                        Some(&reference.history)
                    }
                    Some(
                        ovrcr_protocol::ConversationReference::Pi(reference)
                        | ovrcr_protocol::ConversationReference::Omp(reference),
                    ) => reference.history.as_ref(),
                    Some(ovrcr_protocol::ConversationReference::Codex(reference)) => {
                        reference.history.as_ref()
                    }
                    Some(ovrcr_protocol::ConversationReference::Grok(reference)) => {
                        Some(&reference.history)
                    }
                    Some(ovrcr_protocol::ConversationReference::Hermes(reference)) => {
                        Some(&reference.state_db)
                    }
                    None => None,
                };
                history.is_some_and(|path| {
                    path.starts_with(&workspace.path)
                        || path
                            .canonicalize()
                            .is_ok_and(|path| path.starts_with(&workspace.path))
                })
            })
            .map(|record| {
                format!(
                    "#{}{}",
                    record.id.0,
                    if record.disposition == crate::retained::Disposition::Archived {
                        " (archived)"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>();
        if !history_holders.is_empty() {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                format!(
                    "sessions {} record provider history inside this worktree; preserve it outside the worktree and update the provider reference, or `ovrcr terminal remove <id>` to drop the record before removal",
                    history_holders.join(", ")
                ),
            ));
        }
        // A directory deleted outside OVRCR has nothing left to protect;
        // removal then prunes Git's stale registration (see git.rs).
        let directory_present = fs::symlink_metadata(&workspace.path).is_ok();
        if directory_present {
            let inspection = git::inspect_worktree(&project_record, &workspace)
                .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?;
            if inspection.dirty && self.save_uncommitted_work() {
                git::publish_wip(&inspection.canonical_path, &inspection.branch)
                    .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?;
                force = true;
            } else if inspection.dirty && !force {
                return Err(lifecycle_error(
                    ErrorCode::DirtyWorktree,
                    "worktree has changes",
                ));
            }
        }
        if let Some((run, _)) = cleanup
            && (run.workspace.as_deref() != Some(workspace.id.as_str())
                || run.directory.as_ref() != Some(&workspace.path))
        {
            bail!("registered worktree does not match this run's ownership");
        }
        if let Some((run, run_dir)) = cleanup {
            // Release ownership durably before deletion, so a crash or a reused name cannot
            // let the old run authorize removal of a later workspace at the same path.
            let mut cleaned = run.clone();
            cleaned.workspace = None;
            crate::tasks::write_run(run_dir, &cleaned)?;
        } else if let Some(tasks) = &self.tasks {
            // Ordinary workspace removal also consumes any historical run's ownership.
            tasks.release_workspace_ownership(project, &workspace)?;
        }
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        next.remove_workspace(project, name)?;
        let mut removed = false;
        let archived = self.retained.lock().archive_workspace(&next, project, name, || {
            git::remove_worktree(&project_record, &workspace, force).with_context(|| {
                if cleanup.is_some() {
                    format!("cleanup ownership released; inspect retained worktree {} before manual cleanup", workspace.path.display())
                } else {
                    format!("remove workspace {project}/{name}")
                }
            })?;
            removed = true;
            Ok(())
        }).map_err(|error| {
            if removed || !workspace.path.exists() {
                lifecycle_error_with_hierarchy(
                    ErrorCode::PartialFailure,
                    format!("worktree is unavailable at {} but removal metadata was not committed; workspace and session records are retained for recovery: {}", workspace.path.display(), error_chain_string(&error)),
                    true,
                )
            } else {
                error
            }
        })?;
        *registry = next;
        drop(registry);
        // The worktree is gone; only now persist the forced acknowledgement, by
        // each row's current run (archiving just bumped the unarchived ones), so
        // a Git refusal above leaves no acknowledgement behind.
        {
            let mut retained = self.retained.lock();
            for session in &uncertain {
                if let Some(run) = retained.get(session.id).map(|record| record.run) {
                    retained.mark_stopped(session.id, run)?;
                }
            }
        }
        for id in archived {
            if let Some(session) = self.sessions.lock().unwrap().remove(&id) {
                session.revoke_hook_capability();
            }
            self.dashboard.forget_session(id);
        }
        Ok(())
    }

    pub(crate) fn observe_checkouts(&self) {
        if self.stopping.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return;
        }
        let captured = self.registry.lock().unwrap().clone();
        let mut computed = HashMap::new();
        for project in &captured.projects {
            for workspace in &project.workspaces {
                let root = is_root_workspace(project, workspace);
                let name = git::checkout_name(&workspace.path)
                    .unwrap_or_else(|_| git::UNAVAILABLE_CHECKOUT.to_owned());
                computed.insert(
                    (project.name.clone(), workspace.id.clone()),
                    CheckoutObservation {
                        path: workspace.path.clone(),
                        git_identity: workspace.git_identity.clone(),
                        name,
                        root,
                        warning: root.then(|| git::root_warning(project)).flatten(),
                    },
                );
            }
        }
        if self.stopping.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return;
        }
        let current = self.registry.lock().unwrap().clone();
        let mut cache = self.observations.lock();
        cache.retain(|(project, id), obs| {
            current.projects.iter().any(|candidate| {
                candidate.name == *project
                    && candidate.workspaces.iter().any(|workspace| {
                        workspace.id == *id
                            && workspace.path == obs.path
                            && workspace.git_identity == obs.git_identity
                    })
            })
        });
        for (key, obs) in computed {
            let Some(project) = current
                .projects
                .iter()
                .find(|project| project.name == key.0)
            else {
                continue;
            };
            let Some(workspace) = project
                .workspaces
                .iter()
                .find(|workspace| workspace.id == key.1)
            else {
                continue;
            };
            if workspace.path != obs.path || workspace.git_identity != obs.git_identity {
                continue;
            }
            cache.insert(key, obs);
        }
    }

    fn refuse_root_launch(&self, cwd: &Path) -> Result<()> {
        let projects = self.registry.lock().unwrap().projects.clone();
        for project in &projects {
            if !same_path(cwd, &project.repo) {
                continue;
            }
            if let Some(warning) = git::root_warning(project) {
                return Err(lifecycle_error(ErrorCode::Conflict, warning));
            }
        }
        Ok(())
    }

    fn require_root_default(&self, repo: &Path) -> Result<()> {
        let expected = git::default_branch(repo)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?;
        let actual = git::checkout_name(repo).map_err(|error| {
            lifecycle_error(
                ErrorCode::Conflict,
                format!("could not verify repository checkout: {error}"),
            )
        })?;
        if actual != expected {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                format!(
                    "repository root must be on {expected} (currently {actual}); check out {expected} manually and retry"
                ),
            ));
        }
        Ok(())
    }

    fn root_record_for(&self, repo: &Path) -> Result<WorkspaceRecord> {
        let branch =
            git::checkout_name(repo).unwrap_or_else(|_| git::UNAVAILABLE_CHECKOUT.to_owned());
        let git_identity = git::capture_worktree_identity(repo)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?;
        let id = ovrcr_protocol::new_workspace_id()
            .map_err(|error| lifecycle_error(ErrorCode::Internal, error.to_string()))?;
        Ok(WorkspaceRecord {
            id,
            path: repo.to_path_buf(),
            branch,
            git_identity: Some(git_identity),
            setup_pending: true,
        })
    }

    fn set_setup_pending(&self, project: &str, workspace_id: &str, pending: bool) -> Result<()> {
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        let workspace = next
            .project_mut(project)
            .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "workspace not found"))?;
        workspace.setup_pending = pending;
        save_registry_atomic(&next, &self.registry_path)?;
        *registry = next;
        Ok(())
    }

    fn has_admitted_root_shell(&self, project: &str, workspace_id: &str) -> bool {
        self.session_summaries().iter().any(|session| {
            session.project == project
                && session.workspace == workspace_id
                && !session.archived
                && matches!(session.kind, SessionKind::Terminal)
                && session.phase.is_live()
        })
    }

    fn unresolved_root_shell(&self, project: &str, workspace_id: &str) -> Option<SessionSummary> {
        self.session_summaries().into_iter().find(|session| {
            session.project == project
                && session.workspace == workspace_id
                && !session.archived
                && matches!(session.kind, SessionKind::Terminal)
                && !session.phase.is_live()
        })
    }

    fn launch_root_shell(&self, project: &str, workspace_id: &str) -> Result<SessionSummary> {
        let shell = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .ok_or_else(|| lifecycle_error(ErrorCode::PartialFailure, "SHELL is unset"))?;
        self.create_session_locked(
            ovrcr_protocol::CreateSessionRequest {
                project: project.to_owned(),
                workspace: workspace_id.to_owned(),
                name: "local".into(),
                label: None,
                argv: vec![shell],
                kind: SessionKind::Terminal,
            },
            None,
        )
    }

    fn complete_root_setup_locked(&self, project: &str) -> Result<()> {
        let project_record = self
            .registry
            .lock()
            .unwrap()
            .project(project)
            .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
            .clone();
        if let Some(warning) = git::root_warning(&project_record) {
            return Err(lifecycle_error(ErrorCode::Conflict, warning));
        }
        let Some(root) = project_record
            .workspaces
            .iter()
            .find(|workspace| is_root_workspace(&project_record, workspace))
        else {
            return Err(lifecycle_error(
                ErrorCode::Internal,
                "protected root workspace is missing",
            ));
        };
        if !root.setup_pending {
            return Ok(());
        }
        let root_id = root.id.clone();
        if self.has_admitted_root_shell(project, &root_id) {
            self.set_setup_pending(project, &root_id, false)?;
            return Ok(());
        }
        // Root workspace is the detected default-branch checkout.
        let policy = crate::settings::load(&self.registry_path)
            .settings
            .automatic_local_terminals;
        if !policy.should_create(true) {
            // Preference skips automatic local creation; existing rows stay untouched.
            self.set_setup_pending(project, &root_id, false)?;
            return Ok(());
        }
        if let Some(existing) = self.unresolved_root_shell(project, &root_id) {
            let requires_ack = existing
                .recovery
                .as_ref()
                .is_some_and(|recovery| recovery.requires_ack);
            let detail = existing
                .recovery
                .as_ref()
                .and_then(|recovery| recovery.failure.clone())
                .unwrap_or_else(|| {
                    "confirm that the previous agent and background processes stopped; use --ack-stopped"
                        .to_owned()
                });
            return Err(lifecycle_error_with_hierarchy(
                if requires_ack {
                    ErrorCode::OwnershipUncertain
                } else {
                    ErrorCode::PartialFailure
                },
                format!(
                    "protected root workspace registered at {} but initial shell failed: {detail}",
                    project_record.repo.display()
                ),
                true,
            ));
        }
        match self.launch_root_shell(project, &root_id) {
            Ok(_) => {
                self.set_setup_pending(project, &root_id, false)?;
                Ok(())
            }
            Err(error) => {
                if error
                    .downcast_ref::<LifecycleFailure>()
                    .is_some_and(|failure| failure.code == ErrorCode::OwnershipUncertain)
                {
                    return Err(error);
                }
                Err(lifecycle_error_with_hierarchy(
                    ErrorCode::PartialFailure,
                    format!(
                        "protected root workspace registered at {} but initial shell failed: {}",
                        project_record.repo.display(),
                        error_chain_string(&error)
                    ),
                    true,
                ))
            }
        }
    }

    pub(crate) fn ensure_protected_roots(&self) {
        let _mutation = self.mutation_lock.lock().unwrap();
        if self.stopping.load(Ordering::Acquire) {
            return;
        }
        let names: Vec<String> = self
            .registry
            .lock()
            .unwrap()
            .projects
            .iter()
            .map(|project| project.name.clone())
            .collect();
        for name in names {
            if let Err(error) = self.ensure_one_root_locked(&name) {
                eprintln!("ovrcr server: root workspace for {name}: {error:#}");
            }
        }
    }

    fn ensure_one_root_locked(&self, name: &str) -> Result<()> {
        let project = self
            .registry
            .lock()
            .unwrap()
            .project(name)
            .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
            .clone();
        let has_root = project
            .workspaces
            .iter()
            .any(|workspace| is_root_workspace(&project, workspace));
        if !has_root {
            self.require_root_default(&project.repo)?;
            let root = self.root_record_for(&project.repo)?;
            let mut registry = self.registry.lock().unwrap();
            let mut next = registry.clone();
            next.add_workspace(name, root)?;
            save_registry_atomic(&next, &self.registry_path)?;
            *registry = next;
        }
        let project = self
            .registry
            .lock()
            .unwrap()
            .project(name)
            .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
            .clone();
        let Some(root) = project
            .workspaces
            .iter()
            .find(|workspace| is_root_workspace(&project, workspace))
        else {
            return Ok(());
        };
        if root.setup_pending {
            self.complete_root_setup_locked(name)?;
        }
        Ok(())
    }
}

/// Harness identity for a new session when the client omits `label`.
/// Agent sessions take the provider name from `kind` so a managed launcher
/// whose argv0 is `ovrcr` does not publish OVRCR as the harness. Terminal
/// sessions keep the executable basename. An explicit label always wins.
fn default_session_label(
    kind: &SessionKind,
    explicit: Option<String>,
    argv: &[std::ffi::OsString],
) -> String {
    if let Some(label) = explicit {
        return label;
    }
    match kind {
        SessionKind::Agent { name } => name.clone(),
        SessionKind::Terminal => argv
            .first()
            .and_then(|arg| Path::new(arg).file_name())
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod default_session_label_tests {
    use super::default_session_label;
    use ovrcr_protocol::SessionKind;
    use std::ffi::OsString;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn managed_pi_launcher_uses_agent_kind_not_ovrcr_argv0() {
        assert_eq!(
            default_session_label(
                &SessionKind::Agent { name: "pi".into() },
                None,
                &argv(&["/usr/local/bin/ovrcr", "agent", "run", "pi", "--", "pi"]),
            ),
            "pi"
        );
    }

    #[test]
    fn managed_claude_launcher_uses_agent_kind_not_ovrcr_argv0() {
        assert_eq!(
            default_session_label(
                &SessionKind::Agent {
                    name: "claude".into()
                },
                None,
                &argv(&[
                    "ovrcr",
                    "agent",
                    "run",
                    "--provider",
                    "claude",
                    "--",
                    "claude"
                ]),
            ),
            "claude"
        );
    }

    #[test]
    fn shell_session_keeps_executable_basename() {
        assert_eq!(
            default_session_label(&SessionKind::Terminal, None, &argv(&["/bin/zsh", "-l"]),),
            "zsh"
        );
    }

    #[test]
    fn explicit_label_wins_over_agent_kind() {
        assert_eq!(
            default_session_label(
                &SessionKind::Agent { name: "pi".into() },
                Some("custom-harness".into()),
                &argv(&["ovrcr", "agent", "run", "pi", "--", "pi"]),
            ),
            "custom-harness"
        );
    }

    #[test]
    fn direct_agent_executable_matches_kind_when_label_omitted() {
        assert_eq!(
            default_session_label(
                &SessionKind::Agent {
                    name: "codex".into()
                },
                None,
                &argv(&["codex"]),
            ),
            "codex"
        );
    }
}

fn snapshot(state: &Arc<ServerState>) -> HierarchySnapshot {
    snapshot_from_state(state)
}

fn snapshot_from_state(state: &ServerState) -> HierarchySnapshot {
    let registry = state.registry.lock().unwrap().clone();
    let observations = state.observations.lock().clone();
    let mut sessions = state.session_summaries();
    sessions.retain(|session| !session.archived);
    let mut projects = registry
        .projects
        .iter()
        .map(|project| {
            let workspaces = project
                .workspaces
                .iter()
                .map(|workspace| {
                    let observed = observations
                        .get(&(project.name.clone(), workspace.id.clone()))
                        .filter(|obs| {
                            obs.path == workspace.path && obs.git_identity == workspace.git_identity
                        });
                    let root = observed
                        .map(|obs| obs.root)
                        .unwrap_or_else(|| workspace.path == project.repo);
                    WorkspaceSummary {
                        project: project.name.clone(),
                        id: workspace.id.clone(),
                        name: observed
                            .map(|obs| obs.name.clone())
                            .unwrap_or_else(|| git::UNAVAILABLE_CHECKOUT.to_owned()),
                        path: workspace.path.clone(),
                        root,
                        warning: observed
                            .filter(|_| root)
                            .and_then(|obs| obs.warning.clone()),
                        sessions: Vec::new(),
                    }
                })
                .collect::<Vec<_>>();
            ProjectSummary {
                name: project.name.clone(),
                workspaces,
            }
        })
        .collect::<Vec<_>>();
    // Returned records outlive registry entries. Keep them selectable without
    // registering a new workspace or substituting a different working directory.
    for session in sessions {
        let project_index = projects
            .iter()
            .position(|p| p.name == session.project)
            .unwrap_or_else(|| {
                projects.push(ProjectSummary {
                    name: session.project.clone(),
                    workspaces: Vec::new(),
                });
                projects.len() - 1
            });
        let project = &mut projects[project_index];
        if let Some(workspace) = project
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == session.workspace)
        {
            workspace.sessions.push(session);
        } else {
            project.workspaces.push(WorkspaceSummary {
                project: session.project.clone(),
                id: session.workspace.clone(),
                name: git::UNAVAILABLE_CHECKOUT.to_owned(),
                path: session.cwd.clone(),
                root: false,
                warning: None,
                sessions: vec![session],
            });
        }
    }
    for project in &mut projects {
        project
            .workspaces
            .sort_by(|a, b| (!a.root, &a.name).cmp(&(!b.root, &b.name)));
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .sort_by_key(|row| (row.name != "local", row.id.0));
        }
    }
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    HierarchySnapshot { projects }
}

fn is_root_workspace(project: &ProjectRecord, workspace: &WorkspaceRecord) -> bool {
    same_path(&workspace.path, &project.repo)
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
