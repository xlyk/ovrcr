use crate::config::{ProjectRecord, Registry};
use crate::git::{self, BranchSpec};
use crate::protocol::{
    BranchRequest, ClientMessage, ClientRole, DispatchMessage, ErrorCode, HierarchySnapshot,
    ProjectSummary, Request, Response, ServerEvent, ServerMessage, WorkspaceSummary, read_frame,
    write_frame,
};
use crate::session::{
    InputAdmissionError, Session, SessionEvent, SessionId, SessionPhase, SessionSpec,
    SessionSummary, TerminalSize,
};
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerPaths {
    pub socket: PathBuf,
}

impl ServerPaths {
    pub fn resolve() -> Result<Self> {
        if let Some(socket) = std::env::var_os("OVRCR_SOCKET") {
            return Ok(Self {
                socket: PathBuf::from(socket),
            });
        }
        #[cfg(target_os = "linux")]
        let root = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_temp_dir("ovrcr"));
        #[cfg(target_os = "macos")]
        let root = user_temp_dir("ovrcr");
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let root = user_temp_dir("ovrcr");
        Ok(Self {
            socket: root.join("ovrcr").join("server.sock"),
        })
    }
}

fn user_temp_dir(name: &str) -> PathBuf {
    let temp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let user = unsafe { libc::getuid() };
    temp.join(format!("{name}-{user}"))
}

pub struct ServerState {
    socket: PathBuf,
    pub registry_path: PathBuf,
    pub registry: Mutex<Registry>,
    pub sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    pub selected: Mutex<Option<SessionId>>,
    pub dashboard: Mutex<Option<Arc<DashboardSink>>>,
    pub next_session_id: AtomicU64,
    pub mutation_lock: Mutex<()>,
    pub dispatch: SyncSender<DispatchMessage>,
    pub shutdown: AtomicBool,
    pub stopping: AtomicBool,
    pub dashboard_size: Mutex<Option<DashboardGeometry>>,
    pub events: Mutex<Option<SyncSender<SessionEvent>>>,
    dashboard_slot: Mutex<Option<DashboardSlot>>,
}

pub struct DashboardGeometry {
    owner: Arc<()>,
    size: TerminalSize,
}

impl ServerState {
    pub fn hierarchy(&self) -> HierarchySnapshot {
        snapshot_from_state(self)
    }

    pub fn inventory(&self) -> (Registry, Vec<SessionSummary>) {
        let _mutation = self.mutation_lock.lock().unwrap();
        let registry = self.registry.lock().unwrap().clone();
        let sessions = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .map(|session| session.summary())
            .collect();
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
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
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
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
        if matches!(session.summary().phase, SessionPhase::Exited { .. }) {
            return Err(lifecycle_error(ErrorCode::Conflict, "session has exited"));
        }
        session
            .send_text(text, submit)
            .map_err(|error| lifecycle_error(input_error_code(&error), error_chain_string(&error)))
    }

    pub fn close_terminal(&self, id: SessionId, grace: Duration) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        let termination = session.terminate(grace).map(|_| ());
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, termination, refresh)?;
        self.remove_session_locked(id)
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
        handle_request_with_id(self, role, request, 0)
    }

    pub fn create_session(
        &self,
        request: crate::protocol::CreateSessionRequest,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, None)
    }

    #[cfg(test)]
    pub(crate) fn create_session_with_ready(
        &self,
        request: crate::protocol::CreateSessionRequest,
        ready: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, Some(ready))
    }

    fn create_session_locked(
        &self,
        request: crate::protocol::CreateSessionRequest,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<SessionSummary> {
        let (cwd, label) = {
            let registry = self.registry.lock().unwrap();
            let workspace = registry
                .workspace(&request.project, &request.workspace)
                .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?;
            let label = request.label.clone().unwrap_or_else(|| {
                request
                    .argv
                    .first()
                    .and_then(|arg| Path::new(arg).file_name())
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            (workspace.path.clone(), label)
        };
        let id = SessionId(self.next_session_id.fetch_add(1, Ordering::Relaxed));
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.values().any(|session| {
            let summary = session.summary();
            summary.project == request.project
                && summary.workspace == request.workspace
                && summary.name == request.name
        }) {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                format!(
                    "duplicate session: {}/{}:{}",
                    request.project, request.workspace, request.name
                ),
            ));
        }
        let spec = SessionSpec {
            project: request.project,
            workspace: request.workspace,
            name: request.name,
            label,
            cwd,
            argv: request.argv,
        };
        let size = self.dashboard_size.lock().unwrap().as_ref().map_or(
            TerminalSize {
                rows: 40,
                cols: 120,
            },
            |geometry| geometry.size,
        );
        let events = self
            .events
            .lock()
            .unwrap()
            .as_ref()
            .context("server event channel closed")?
            .clone();
        let session = match ready {
            Some(ready) => Session::spawn_with_ready(id, spec, size, events, ready),
            None => Session::spawn(id, spec, size, events),
        }
        .context("spawn session")?;
        let summary = session.summary();
        sessions.insert(id, session);
        Ok(summary)
    }

    pub fn kill_session(&self, id: SessionId, grace: Duration) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        let termination = session.terminate(grace).map(|_| ());
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, termination, refresh)
    }

    pub fn remove_session(&self, id: SessionId) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.remove_session_locked(id)
    }

    fn remove_session_locked(&self, id: SessionId) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
        if !matches!(session.summary().phase, SessionPhase::Exited { .. }) {
            return Err(lifecycle_error(
                ErrorCode::SessionRunning,
                "session is still live; kill it before removal",
            ));
        }
        self.sessions.lock().unwrap().remove(&id);
        let mut selected = self.selected.lock().unwrap();
        if selected.as_ref() == Some(&id) {
            *selected = None;
        }
        Ok(())
    }

    pub fn request_shutdown(&self, kill: bool) -> Response {
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
        if !kill && !sessions.is_empty() {
            return error_response(ErrorCode::SessionsRemain, "sessions remain");
        }
        if kill {
            let mut failures = Vec::new();
            for session in sessions {
                let id = session.summary().id;
                let termination = session.terminate(Duration::from_secs(5)).map(|_| ());
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
        Response::Ok
    }

    fn reject_if_stopping(&self) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            bail!("server is stopping")
        }
        Ok(())
    }

    fn session_for_control(&self, id: SessionId) -> Result<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })
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
        if let Some(snapshot) = dashboard_snapshot(self) {
            disconnect_dashboard(self, snapshot);
        }
        bail!("{error}")
    }

    pub fn add_project(&self, name: String, repo: PathBuf, workspace_root: PathBuf) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let (repo, workspace_root) = git::validate_project(&repo, &workspace_root)?;
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        if next.projects.iter().any(|project| project.name == name) {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                format!("duplicate project: {name}"),
            ));
        }
        next.add_project(ProjectRecord {
            name,
            repo,
            workspace_root,
            workspaces: Vec::new(),
        })?;
        next.save_atomic(&self.registry_path)?;
        *registry = next;
        Ok(())
    }

    pub fn remove_project(&self, name: &str) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        let project = next
            .projects
            .iter()
            .find(|project| project.name == name)
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("project not found: {name}"))
            })?;
        if !project.workspaces.is_empty() {
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!("cannot remove project {name}: workspaces remain"),
            ));
        }
        next.remove_project(name)?;
        next.save_atomic(&self.registry_path)?;
        *registry = next;
        Ok(())
    }

    pub fn create_workspace(
        &self,
        project: String,
        name: String,
        branch: BranchRequest,
    ) -> Result<()> {
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
            .any(|workspace| workspace.name == name)
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
            if let Err(error) = next.save_atomic(&self.registry_path) {
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
        if let Err(error) = self.create_session_locked(
            crate::protocol::CreateSessionRequest {
                project,
                workspace: name.clone(),
                name: "local".into(),
                label: None,
                argv: vec![shell],
            },
            None,
        ) {
            return Err(lifecycle_error_with_hierarchy(
                ErrorCode::PartialFailure,
                format!(
                    "worktree for workspace {} remains at {}: {}",
                    name,
                    workspace.path.display(),
                    error_chain_string(&error)
                ),
                true,
            ));
        }
        Ok(())
    }

    pub fn remove_workspace(&self, project: &str, name: &str) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let occupied = self.sessions.lock().unwrap().values().any(|session| {
            let summary = session.summary();
            summary.project == project && summary.workspace == name
        });
        if occupied {
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!("sessions remain for workspace {project}/{name}"),
            ));
        }
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
        if git::inspect_worktree(&project_record, &workspace)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?
            .dirty
        {
            return Err(lifecycle_error(
                ErrorCode::DirtyWorktree,
                "worktree has changes",
            ));
        }
        git::remove_worktree(&project_record, &workspace)?;
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        next.remove_workspace(project, name)?;
        if let Err(error) = next.save_atomic(&self.registry_path) {
            *registry = next;
            return Err(lifecycle_error_with_hierarchy(
                ErrorCode::PartialFailure,
                format!(
                    "worktree was removed at {} but registry update failed; live state reflects removal: {}",
                    workspace.path.display(),
                    error_chain_string(&error)
                ),
                true,
            ));
        }
        *registry = next;
        Ok(())
    }
}

pub struct DashboardOutbound {
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirtyState {
    Pending,
    Sending,
    Sent,
}

struct DashboardQueue {
    messages: VecDeque<DashboardOutbound>,
    dirty: HashMap<SessionId, DirtyState>,
    closed: bool,
}

pub struct DashboardSink {
    queue: Mutex<DashboardQueue>,
    wake: Condvar,
}

enum DashboardDelivery {
    Message(DashboardOutbound),
    Dirty(SessionId),
}

impl DashboardSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(DashboardQueue {
                messages: VecDeque::with_capacity(DASHBOARD_QUEUE),
                dirty: HashMap::new(),
                closed: false,
            }),
            wake: Condvar::new(),
        })
    }

    fn enqueue(&self, outbound: DashboardOutbound) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            return false;
        }
        if let ServerMessage::Event(ServerEvent::Output { session, .. }) = &outbound.message {
            if queue.dirty.contains_key(session) {
                return true;
            }
            if queue.messages.len() == DASHBOARD_QUEUE {
                queue.messages.retain(|queued| {
                    !matches!(
                        queued.message,
                        ServerMessage::Event(ServerEvent::Output {
                            session: queued_session,
                            ..
                        }) if queued_session == *session
                    )
                });
                queue.dirty.insert(*session, DirtyState::Pending);
                self.wake.notify_one();
                return true;
            }
        } else if queue.messages.len() == DASHBOARD_QUEUE {
            queue.closed = true;
            self.wake.notify_all();
            return false;
        }
        queue.messages.push_back(outbound);
        self.wake.notify_one();
        true
    }

    fn replace_selection(
        &self,
        previous: Option<SessionId>,
        session: SessionId,
        request_id: u64,
        size: TerminalSize,
        bytes: Vec<u8>,
    ) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            return false;
        }
        let discard = previous
            .into_iter()
            .chain(std::iter::once(session))
            .collect::<HashSet<_>>();
        queue.messages.retain(|queued| {
            !matches!(
                queued.message,
                ServerMessage::Event(ServerEvent::Output { session, .. }) if discard.contains(&session)
            )
        });
        for id in discard {
            queue.dirty.remove(&id);
        }
        if queue.messages.len() == DASHBOARD_QUEUE {
            queue.closed = true;
            self.wake.notify_all();
            return false;
        }
        queue.messages.push_back(DashboardOutbound {
            message: ServerMessage::Response {
                request_id,
                response: Response::Screen {
                    session,
                    size,
                    bytes,
                },
            },
            completion: None,
        });
        self.wake.notify_one();
        true
    }

    fn next(&self) -> Option<DashboardDelivery> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if queue.closed {
                return None;
            }
            if let Some(message) = queue.messages.pop_front() {
                return Some(DashboardDelivery::Message(message));
            }
            if let Some((session, state)) = queue
                .dirty
                .iter_mut()
                .find(|(_, state)| **state == DirtyState::Pending)
            {
                *state = DirtyState::Sending;
                return Some(DashboardDelivery::Dirty(*session));
            }
            queue = self.wake.wait(queue).unwrap();
        }
    }

    fn dirty_sent(&self, session: SessionId) {
        let mut queue = self.queue.lock().unwrap();
        if queue.dirty.get(&session) == Some(&DirtyState::Sending) {
            queue.dirty.insert(session, DirtyState::Sent);
        }
    }

    fn close(&self) {
        let mut queue = self.queue.lock().unwrap();
        queue.closed = true;
        self.wake.notify_all();
    }
}

struct DashboardSlot {
    sink: Arc<DashboardSink>,
    identity: Arc<()>,
    stream: UnixStream,
}

struct DashboardSnapshot {
    sink: Arc<DashboardSink>,
    identity: Arc<()>,
    stream: UnixStream,
}

pub fn run_server(paths: ServerPaths, registry_path: PathBuf) -> Result<()> {
    let parent = paths
        .socket
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create server socket directory {}", parent.display()))?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("secure server socket directory {}", parent.display()))?;
    let startup_lock = acquire_startup_lock(parent)?;
    if paths.socket.exists() {
        match UnixStream::connect(&paths.socket) {
            Ok(_) => bail!("server is already running"),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                match fs::remove_file(&paths.socket) {
                    Ok(()) => {}
                    Err(remove_error) if remove_error.kind() == io::ErrorKind::NotFound => {}
                    Err(remove_error) => {
                        return Err(remove_error).with_context(|| {
                            format!("remove stale server socket {}", paths.socket.display())
                        });
                    }
                }
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("probe server socket {}", paths.socket.display()));
            }
        }
    }
    let listener = UnixListener::bind(&paths.socket)
        .with_context(|| format!("bind server socket {}", paths.socket.display()))?;
    drop(startup_lock);
    let registry = Registry::load(&registry_path)
        .with_context(|| format!("load server registry {}", registry_path.display()))?;
    let (events, event_receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let (dispatch, dispatch_receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
    let state = Arc::new(ServerState {
        socket: paths.socket.clone(),
        registry_path,
        registry: Mutex::new(registry),
        sessions: Mutex::new(HashMap::new()),
        selected: Mutex::new(None),
        dashboard: Mutex::new(None),
        next_session_id: AtomicU64::new(1),
        mutation_lock: Mutex::new(()),
        dispatch: dispatch.clone(),
        shutdown: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        dashboard_size: Mutex::new(None),
        events: Mutex::new(Some(events)),
        dashboard_slot: Mutex::new(None),
    });
    let bridge_dispatch = dispatch.clone();
    let bridge = thread::Builder::new()
        .name("ovrcr-event-bridge".into())
        .spawn(move || bridge_events(event_receiver, bridge_dispatch))?;
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::Builder::new()
        .name("ovrcr-dispatcher".into())
        .spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver))?;
    for incoming in listener.incoming() {
        if state.shutdown.load(Ordering::Acquire) {
            break;
        }
        match incoming {
            Ok(stream) => {
                let connection_state = Arc::clone(&state);
                thread::Builder::new()
                    .name("ovrcr-client".into())
                    .spawn(move || handle_connection(connection_state, stream))?;
            }
            Err(_) if state.shutdown.load(Ordering::Acquire) => break,
            Err(error) => return Err(error).context("accept server client"),
        }
    }
    if let Some(snapshot) = dashboard_snapshot(&state) {
        disconnect_dashboard(&state, snapshot);
    }
    let _ = dispatch.send(DispatchMessage::Stop);
    dispatcher
        .join()
        .map_err(|_| anyhow::anyhow!("server dispatcher panicked"))?;
    state.events.lock().unwrap().take();
    drop(state);
    drop(dispatch);
    bridge
        .join()
        .map_err(|_| anyhow::anyhow!("server event bridge panicked"))?;
    let _ = fs::remove_file(&paths.socket);
    Ok(())
}

fn bridge_events(events: Receiver<SessionEvent>, dispatch: SyncSender<DispatchMessage>) {
    while let Ok(event) = events.recv() {
        if dispatch.send(DispatchMessage::Session(event)).is_err() {
            break;
        }
    }
}

pub fn run_dispatcher(state: Arc<ServerState>, commands: Receiver<DispatchMessage>) {
    while let Ok(command) = commands.recv() {
        match command {
            DispatchMessage::Session(event) => dispatch_session_event(&state, event),
            DispatchMessage::RefreshSession { session } => {
                dispatch_refresh_session(&state, session)
            }
            DispatchMessage::Select {
                request_id,
                session,
                size,
                completion,
            } => dispatch_select(&state, request_id, session, size, completion),
            DispatchMessage::Stop => break,
        }
    }
}

fn dispatch_session_event(state: &Arc<ServerState>, event: SessionEvent) {
    let id = match &event {
        SessionEvent::Output { id, .. } | SessionEvent::Exited { id, .. } => *id,
    };
    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return };
    let output = match &event {
        SessionEvent::Output { bytes, .. } => Some(bytes.clone()),
        SessionEvent::Exited { .. } => None,
    };
    session.apply_event(event);
    if let Some(bytes) = output {
        if state.selected.lock().unwrap().as_ref() == Some(&id) {
            dashboard_try_send(
                state,
                ServerMessage::Event(ServerEvent::Output { session: id, bytes }),
            );
        }
    } else {
        dashboard_try_send(
            state,
            ServerMessage::Event(ServerEvent::SessionChanged(session.summary())),
        );
        dashboard_try_send(
            state,
            ServerMessage::Event(ServerEvent::HierarchyChanged(snapshot(state))),
        );
    }
}

fn dispatch_refresh_session(state: &Arc<ServerState>, id: SessionId) {
    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return };
    dashboard_try_send(
        state,
        ServerMessage::Event(ServerEvent::SessionChanged(session.summary())),
    );
}

fn dispatch_select(
    state: &ServerState,
    request_id: u64,
    id: SessionId,
    size: TerminalSize,
    completion: SyncSender<()>,
) {
    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else {
        dashboard_try_send(
            state,
            response_message(
                request_id,
                Response::Error {
                    code: ErrorCode::NotFound,
                    message: format!("session {} not found", id.0),
                },
            ),
        );
        let _ = completion.send(());
        return;
    };
    if let Err(error) = session.resize(size) {
        dashboard_try_send(
            state,
            response_message(
                request_id,
                Response::Error {
                    code: ErrorCode::Internal,
                    message: error.to_string(),
                },
            ),
        );
        let _ = completion.send(());
        return;
    }
    let previous = *state.selected.lock().unwrap();
    let Some(snapshot) = dashboard_snapshot(state) else {
        let _ = completion.send(());
        return;
    };
    set_dashboard_geometry(state, &snapshot.identity, size);
    if !snapshot
        .sink
        .replace_selection(previous, id, request_id, size, session.current_screen())
    {
        disconnect_dashboard(state, snapshot);
        let _ = completion.send(());
        return;
    }
    *state.selected.lock().unwrap() = Some(id);
    let _ = completion.send(());
}

fn set_dashboard_geometry(state: &ServerState, owner: &Arc<()>, size: TerminalSize) {
    *state.dashboard_size.lock().unwrap() = Some(DashboardGeometry {
        owner: Arc::clone(owner),
        size,
    });
}

fn clear_dashboard_geometry(state: &ServerState, owner: &Arc<()>) {
    let mut geometry = state.dashboard_size.lock().unwrap();
    if geometry
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.owner, owner))
    {
        *geometry = None;
    }
}

fn handle_connection(state: Arc<ServerState>, mut stream: UnixStream) {
    let mut role = ClientRole::Control;
    let mut dashboard_identity = None;
    let dashboard_sink = DashboardSink::new();
    let mut writer: Option<JoinHandle<()>> = None;
    while !state.shutdown.load(Ordering::Acquire) {
        let message = match read_frame::<ClientMessage>(&mut stream) {
            Ok(message) => message,
            Err(_) => break,
        };
        if matches!(message.request, Request::DashboardHello) {
            if writer.is_some() {
                let (completion, result) = mpsc::sync_channel(1);
                if dashboard_send(
                    &state,
                    response_message(
                        message.request_id,
                        error_response(
                            ErrorCode::Conflict,
                            "dashboard is already registered on this connection",
                        ),
                    ),
                    Some(completion),
                ) {
                    let _ = result.recv();
                }
                break;
            }
            let identity = Arc::new(());
            let Ok(close_stream) = stream.try_clone() else {
                break;
            };
            let mut slot = state.dashboard_slot.lock().unwrap();
            if slot.is_some() {
                drop(slot);
                let _ = send_direct(
                    &mut stream,
                    message.request_id,
                    error_response(
                        ErrorCode::Conflict,
                        "another dashboard is already connected",
                    ),
                );
                break;
            }
            *slot = Some(DashboardSlot {
                sink: Arc::clone(&dashboard_sink),
                identity: identity.clone(),
                stream: close_stream,
            });
            drop(slot);
            *state.dashboard.lock().unwrap() = Some(Arc::clone(&dashboard_sink));
            dashboard_identity = Some(Arc::clone(&identity));
            role = ClientRole::Dashboard;
            let mut output = stream.try_clone().ok();
            let writer_sink = Arc::clone(&dashboard_sink);
            let writer_state = Arc::clone(&state);
            let writer_identity = Arc::clone(&identity);
            let writer_close_stream = stream.try_clone().expect("clone dashboard close stream");
            writer = Some(
                thread::Builder::new()
                    .name("ovrcr-dashboard-writer".into())
                    .spawn(move || {
                        while let Some(delivery) = writer_sink.next() {
                            let (message, completion, dirty) = match delivery {
                                DashboardDelivery::Message(outbound) => {
                                    (outbound.message, outbound.completion, None)
                                }
                                DashboardDelivery::Dirty(session) => (
                                    ServerMessage::Event(ServerEvent::ScreenDirty { session }),
                                    None,
                                    Some(session),
                                ),
                            };
                            let result = match output.as_mut() {
                                Some(stream) => {
                                    write_frame(stream, &message).map_err(|error| error.to_string())
                                }
                                None => Err("dashboard writer stream unavailable".into()),
                            };
                            if result.is_ok()
                                && let Some(session) = dirty
                            {
                                writer_sink.dirty_sent(session);
                            }
                            if let Some(completion) = completion {
                                let _ = completion.send(result.clone());
                            }
                            if result.is_err() {
                                writer_sink.close();
                                let _ = writer_close_stream.shutdown(std::net::Shutdown::Both);
                                disconnect_dashboard(
                                    &writer_state,
                                    DashboardSnapshot {
                                        sink: writer_sink,
                                        identity: writer_identity,
                                        stream: writer_close_stream,
                                    },
                                );
                                break;
                            }
                        }
                    })
                    .expect("dashboard writer thread"),
            );
            dashboard_try_send(
                &state,
                response_message(message.request_id, Response::Hierarchy(snapshot(&state))),
            );
            continue;
        }
        let shutdown = matches!(message.request, Request::Shutdown { .. });
        let select = matches!(message.request, Request::Select { .. });
        let response = handle_request_with_id(
            &state,
            &mut role,
            message.request.clone(),
            message.request_id,
        );
        let successful_shutdown = shutdown && matches!(&response, Response::Ok);
        let (delivered, dashboard_shutdown_attempt) = match role {
            ClientRole::Control => (
                send_direct(&mut stream, message.request_id, response).is_ok(),
                false,
            ),
            ClientRole::Dashboard => {
                if !select {
                    if successful_shutdown {
                        let (completion, result) = mpsc::sync_channel(1);
                        let queued = dashboard_send(
                            &state,
                            response_message(message.request_id, response),
                            Some(completion),
                        );
                        let delivered =
                            queued && result.recv().is_ok_and(|write_result| write_result.is_ok());
                        (delivered, true)
                    } else {
                        (
                            dashboard_try_send(
                                &state,
                                response_message(message.request_id, response),
                            ),
                            false,
                        )
                    }
                } else {
                    (true, false)
                }
            }
        };
        if successful_shutdown {
            debug_assert!(dashboard_shutdown_attempt || matches!(role, ClientRole::Control));
            state.shutdown.store(true, Ordering::Release);
            wake_accept(&state);
        }
        if !delivered {
            break;
        }
        if matches!(role, ClientRole::Control) {
            break;
        }
    }
    {
        let mut slot = state.dashboard_slot.lock().unwrap();
        let owns_dashboard = dashboard_identity.as_ref().is_some_and(|identity| {
            slot.as_ref()
                .is_some_and(|current| Arc::ptr_eq(&current.identity, identity))
        });
        if owns_dashboard {
            if let Some(current) = slot.take() {
                current.sink.close();
            }
            *state.dashboard.lock().unwrap() = None;
            *state.selected.lock().unwrap() = None;
            clear_dashboard_geometry(&state, dashboard_identity.as_ref().unwrap());
        }
    }
    dashboard_sink.close();
    if let Some(writer) = writer {
        let _ = writer.join();
    }
}

fn handle_request_with_id(
    state: &ServerState,
    role: &mut ClientRole,
    request: Request,
    request_id: u64,
) -> Response {
    let dashboard = matches!(role, ClientRole::Dashboard);
    match request {
        Request::List => Response::Hierarchy(state.hierarchy()),
        Request::Inspect => {
            let (registry, sessions) = state.inventory();
            Response::Inventory { registry, sessions }
        }
        Request::ReadTerminal { session, max_lines } => state
            .read_terminal(session, max_lines)
            .map_or_else(error_for_lifecycle, |(size, text)| Response::TerminalText {
                session,
                size,
                text,
            }),
        Request::SendTerminal {
            session,
            text,
            submit,
        } => state
            .send_terminal(session, &text, submit)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::CloseTerminal { session } => state
            .close_terminal(session, requested_kill_grace())
            .map_or_else(error_for_lifecycle, |_| {
                dashboard_try_send_arc(
                    state,
                    ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                );
                Response::Ok
            }),
        Request::PauseSession { session } => state
            .set_session_paused(session, true)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::ResumeSession { session } => state
            .set_session_paused(session, false)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::Select { session, size } => {
            if !dashboard {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "Select requires a dashboard connection",
                );
            }
            let (sender, receiver) = mpsc::sync_channel(1);
            if state
                .dispatch
                .send(DispatchMessage::Select {
                    request_id,
                    session,
                    size,
                    completion: sender,
                })
                .is_err()
            {
                return error_response(ErrorCode::Internal, "dispatcher is unavailable");
            }
            let _ = receiver.recv();
            Response::Ok
        }
        Request::Input { session, bytes } => {
            if !dashboard || state.selected.lock().unwrap().as_ref() != Some(&session) {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "input is only accepted for the selected dashboard session",
                );
            }
            let selected_session = state.sessions.lock().unwrap().get(&session).cloned();
            match selected_session {
                Some(session) => session.write(&bytes).map_or_else(
                    |error| error_response(input_error_code(&error), error_chain_string(&error)),
                    |_| Response::Ok,
                ),
                None => error_response(
                    ErrorCode::NotFound,
                    format!("session {} not found", session.0),
                ),
            }
        }
        Request::Resize { session, size } => {
            if !dashboard || state.selected.lock().unwrap().as_ref() != Some(&session) {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "resize is only accepted for the selected dashboard session",
                );
            }
            let geometry_owner = dashboard_snapshot(state).map(|snapshot| snapshot.identity);
            let selected_session = state.sessions.lock().unwrap().get(&session).cloned();
            match selected_session {
                Some(session) => session.resize(size).map_or_else(
                    |error| error_response(ErrorCode::Internal, error.to_string()),
                    |_| {
                        if let Some(owner) = geometry_owner.as_ref() {
                            set_dashboard_geometry(state, owner, size);
                        }
                        Response::Ok
                    },
                ),
                None => error_response(
                    ErrorCode::NotFound,
                    format!("session {} not found", session.0),
                ),
            }
        }
        Request::Shutdown { kill } => state.request_shutdown(kill),
        Request::AddProject {
            name,
            repo,
            workspace_root,
        } => state
            .add_project(name, repo, workspace_root)
            .map_or_else(error_for_lifecycle, |_| {
                dashboard_try_send_arc(
                    state,
                    ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                );
                Response::Ok
            }),
        Request::RemoveProject { name } => {
            state
                .remove_project(&name)
                .map_or_else(error_for_lifecycle, |_| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
                    Response::Ok
                })
        }
        Request::CreateWorkspace {
            project,
            name,
            branch,
        } => state.create_workspace(project, name, branch).map_or_else(
            |error| lifecycle_response_with_partial_hierarchy(state, error),
            |_| {
                dashboard_try_send_arc(
                    state,
                    ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                );
                Response::Ok
            },
        ),
        Request::RemoveWorkspace { project, name } => {
            state.remove_workspace(&project, &name).map_or_else(
                |error| lifecycle_response_with_partial_hierarchy(state, error),
                |_| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
                    Response::Ok
                },
            )
        }
        Request::CreateSession(request) => {
            state
                .create_session(request)
                .map_or_else(error_for_lifecycle, |summary| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
                    Response::CreatedSession(summary)
                })
        }
        Request::KillSession { session } => state
            .kill_session(session, requested_kill_grace())
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::RemoveSession { session } => {
            state
                .remove_session(session)
                .map_or_else(error_for_lifecycle, |_| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
                    Response::Ok
                })
        }
        Request::DashboardHello => {
            *role = ClientRole::Dashboard;
            Response::Ok
        }
        Request::DashboardGeometry { size } => {
            if !dashboard {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "DashboardGeometry requires a dashboard connection",
                );
            }
            let _mutation = state.mutation_lock.lock().unwrap();
            if let Some(snapshot) = dashboard_snapshot(state) {
                set_dashboard_geometry(state, &snapshot.identity, size);
                Response::Ok
            } else {
                error_response(ErrorCode::Conflict, "dashboard is disconnected")
            }
        }
    }
}

fn error_for_lifecycle(error: anyhow::Error) -> Response {
    let message = error_chain_string(&error);
    let code = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<LifecycleFailure>())
        .map(|failure| failure.code.clone())
        .unwrap_or(ErrorCode::Conflict);
    error_response(code, message)
}

fn input_error_code(error: &anyhow::Error) -> ErrorCode {
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<InputAdmissionError>().is_some())
    {
        ErrorCode::Conflict
    } else {
        ErrorCode::Internal
    }
}

fn lifecycle_code(error: &anyhow::Error) -> ErrorCode {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<LifecycleFailure>())
        .map_or(ErrorCode::Conflict, |failure| failure.code.clone())
}

fn combine_control_and_refresh(
    id: SessionId,
    control: Result<()>,
    refresh: Result<()>,
) -> Result<()> {
    match (control, refresh) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(lifecycle_error(
            ErrorCode::PartialFailure,
            format!(
                "session {} refresh failed: {}",
                id.0,
                error_chain_string(&error)
            ),
        )),
        (Err(control), Err(refresh)) => Err(lifecycle_error(
            lifecycle_code(&control),
            format!(
                "{}; session {} refresh failed: {}",
                error_chain_string(&control),
                id.0,
                error_chain_string(&refresh)
            ),
        )),
    }
}

fn error_chain_string(error: &anyhow::Error) -> String {
    let mut chain = error.chain().map(ToString::to_string);
    chain.next().map_or_else(String::new, |first| {
        chain.fold(first, |mut message, cause| {
            if !message.contains(&cause) {
                message.push_str(": ");
                message.push_str(&cause);
            }
            message
        })
    })
}

fn lifecycle_response_with_partial_hierarchy(
    state: &ServerState,
    error: anyhow::Error,
) -> Response {
    let publish_hierarchy = error
        .downcast_ref::<LifecycleFailure>()
        .is_some_and(|failure| failure.publish_hierarchy);
    let response = error_for_lifecycle(error);
    if publish_hierarchy {
        dashboard_try_send_arc(
            state,
            ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
        );
    }
    response
}

fn dashboard_try_send_arc(state: &ServerState, message: ServerMessage) -> bool {
    dashboard_send(state, message, None)
}

#[cfg(test)]
fn handle_shutdown<F>(state: &Arc<ServerState>, kill: bool, mut terminate: F) -> Response
where
    F: FnMut(&Arc<Session>) -> Result<()>,
{
    let _mutation = state.mutation_lock.lock().unwrap();
    let sessions = state
        .sessions
        .lock()
        .unwrap()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    if !kill
        && sessions.iter().any(|session| {
            matches!(
                session.summary().phase,
                SessionPhase::Running | SessionPhase::Paused
            )
        })
    {
        return error_response(ErrorCode::SessionsRemain, "sessions remain");
    }
    if kill {
        let mut failures = Vec::new();
        for session in sessions {
            if let Err(error) = terminate(&session) {
                failures.push(format!(
                    "session {}: {}",
                    session.summary().id.0,
                    error_chain_string(&error)
                ));
            }
        }
        if !failures.is_empty() {
            return error_response(ErrorCode::PartialFailure, failures.join("; "));
        }
    }
    Response::Ok
}

fn error_response(code: ErrorCode, message: impl std::fmt::Display) -> Response {
    Response::Error {
        code,
        message: message.to_string(),
    }
}

fn requested_kill_grace() -> Duration {
    // Integration tests use this per-server-process seam to exercise the short
    // escalation path without changing the normal five-second CLI behavior.
    std::env::var("OVRCR_KILL_GRACE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(5))
}
fn response_message(request_id: u64, response: Response) -> ServerMessage {
    ServerMessage::Response {
        request_id,
        response,
    }
}
fn send_direct(stream: &mut UnixStream, request_id: u64, response: Response) -> Result<()> {
    write_frame(stream, &response_message(request_id, response))
}

fn dashboard_try_send(state: &ServerState, message: ServerMessage) -> bool {
    dashboard_send(state, message, None)
}

fn dashboard_send(
    state: &ServerState,
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
) -> bool {
    let Some(snapshot) = dashboard_snapshot(state) else {
        return false;
    };
    if !snapshot.sink.enqueue(DashboardOutbound {
        message,
        completion,
    }) {
        disconnect_dashboard(state, snapshot);
        return false;
    }
    true
}

fn dashboard_snapshot(state: &ServerState) -> Option<DashboardSnapshot> {
    let slot = state.dashboard_slot.lock().unwrap();
    let slot = slot.as_ref()?;
    Some(DashboardSnapshot {
        sink: slot.sink.clone(),
        identity: slot.identity.clone(),
        stream: slot.stream.try_clone().ok()?,
    })
}

fn disconnect_dashboard(state: &ServerState, snapshot: DashboardSnapshot) {
    let _ = snapshot.stream.shutdown(std::net::Shutdown::Both);
    snapshot.sink.close();
    let mut slot = state.dashboard_slot.lock().unwrap();
    if slot
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.identity, &snapshot.identity))
    {
        let _ = slot.take();
        *state.dashboard.lock().unwrap() = None;
        *state.selected.lock().unwrap() = None;
        clear_dashboard_geometry(state, &snapshot.identity);
    }
}

fn acquire_startup_lock(parent: &Path) -> Result<File> {
    let path = parent.join(".server.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open server startup lock {}", path.display()))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error()).context("lock server startup");
    }
    Ok(file)
}

fn snapshot(state: &Arc<ServerState>) -> HierarchySnapshot {
    snapshot_from_state(state)
}

fn snapshot_from_state(state: &ServerState) -> HierarchySnapshot {
    let registry = state.registry.lock().unwrap().clone();
    let sessions = state.sessions.lock().unwrap();
    let mut projects = registry
        .projects
        .into_iter()
        .map(|project| {
            let mut workspaces = project
                .workspaces
                .into_iter()
                .map(|workspace| WorkspaceSummary {
                    project: project.name.clone(),
                    name: workspace.name,
                    path: workspace.path,
                    sessions: Vec::new(),
                })
                .collect::<Vec<_>>();
            for workspace in &mut workspaces {
                workspace.sessions = sessions
                    .values()
                    .filter_map(|session| {
                        let summary = session.summary();
                        (summary.project == project.name && summary.workspace == workspace.name)
                            .then_some(summary)
                    })
                    .collect();
                workspace.sessions.sort_by(|left, right| {
                    (left.name != "local", left.id.0).cmp(&(right.name != "local", right.id.0))
                });
            }
            workspaces.sort_by(|left, right| left.name.cmp(&right.name));
            ProjectSummary {
                name: project.name,
                workspaces,
            }
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    HierarchySnapshot { projects }
}

fn wake_accept(state: &Arc<ServerState>) {
    let _ = UnixStream::connect(&state.socket);
}

pub fn connect_if_running(paths: &ServerPaths) -> Result<Option<UnixStream>> {
    match UnixStream::connect(&paths.socket) {
        Ok(stream) => Ok(Some(stream)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("connect server {}", paths.socket.display()))
        }
    }
}

pub fn connect_or_start(paths: &ServerPaths) -> Result<UnixStream> {
    if let Some(stream) = connect_if_running(paths)? {
        return Ok(stream);
    }
    let executable = std::env::var_os("OVRCR_SERVER_EXECUTABLE")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| std::env::current_exe().context("resolve OVRCR executable"))?;
    let mut command = Command::new(executable);
    command
        .arg("server")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().context("start detached OVRCR server")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(stream) = connect_if_running(paths)? {
            return Ok(stream);
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for server startup")
        }
        thread::park_timeout(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ServerEvent, ServerMessage};
    use std::io::Read;
    use std::net::Shutdown;

    #[test]
    fn raw_event_and_dispatch_queues_reject_the_65th_item() {
        let (event_sender, _event_receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        for _ in 0..RAW_EVENT_QUEUE_CAPACITY {
            event_sender
                .try_send(SessionEvent::Output {
                    id: SessionId(1),
                    bytes: Vec::new(),
                })
                .unwrap();
        }
        assert!(matches!(
            event_sender.try_send(SessionEvent::Output {
                id: SessionId(1),
                bytes: Vec::new(),
            }),
            Err(mpsc::TrySendError::Full(_))
        ));

        let (dispatch_sender, _dispatch_receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
        for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
            dispatch_sender.try_send(DispatchMessage::Stop).unwrap();
        }
        assert!(matches!(
            dispatch_sender.try_send(DispatchMessage::Stop),
            Err(mpsc::TrySendError::Full(_))
        ));
    }

    #[test]
    fn lifecycle_response_preserves_error_chain_and_code() {
        let error =
            lifecycle_error(ErrorCode::NotFound, "missing executable").context("spawn session");
        let response = error_for_lifecycle(error);
        assert_eq!(
            response,
            Response::Error {
                code: ErrorCode::NotFound,
                message: "spawn session: missing executable".into(),
            }
        );
    }

    fn test_state(
        dashboard: Option<Arc<DashboardSink>>,
        stream: Option<(Arc<()>, UnixStream)>,
    ) -> Arc<ServerState> {
        test_state_with_dispatch(dashboard, stream).0
    }

    fn test_state_with_dispatch(
        dashboard: Option<Arc<DashboardSink>>,
        stream: Option<(Arc<()>, UnixStream)>,
    ) -> (Arc<ServerState>, Receiver<DispatchMessage>) {
        let (events, _) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let (dispatch, receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
        (
            Arc::new(ServerState {
                socket: PathBuf::from("/tmp/ovrcr-test.sock"),
                registry_path: PathBuf::from("config.toml"),
                registry: Mutex::new(Registry::default()),
                sessions: Mutex::new(HashMap::new()),
                selected: Mutex::new(None),
                dashboard: Mutex::new(dashboard.clone()),
                next_session_id: AtomicU64::new(1),
                mutation_lock: Mutex::new(()),
                dispatch,
                shutdown: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
                dashboard_size: Mutex::new(None),
                events: Mutex::new(Some(events)),
                dashboard_slot: Mutex::new(stream.map(|(identity, stream)| DashboardSlot {
                    sink: dashboard.as_ref().unwrap().clone(),
                    identity,
                    stream,
                })),
            }),
            receiver,
        )
    }

    fn spawn_live_test_session(
        id: SessionId,
    ) -> (tempfile::TempDir, Arc<Session>, Receiver<SessionEvent>) {
        let cwd = tempfile::tempdir().unwrap();
        let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let session = Session::spawn(
            id,
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "live".into(),
                label: "sh".into(),
                cwd: cwd.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "trap '' TERM; while :; do sleep 1; done".into(),
                ],
            },
            TerminalSize { rows: 24, cols: 80 },
            events,
        )
        .unwrap();
        (cwd, session, receiver)
    }

    fn apply_test_session_events(
        session: Arc<Session>,
        receiver: Receiver<SessionEvent>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                let exited = matches!(event, SessionEvent::Exited { .. });
                session.apply_event(event);
                if exited {
                    break;
                }
            }
        })
    }

    fn saturated_control_state(
        session: &Arc<Session>,
        id: SessionId,
    ) -> (Arc<ServerState>, Receiver<DispatchMessage>, UnixStream) {
        let (server_stream, client_stream) = UnixStream::pair().unwrap();
        let sink = DashboardSink::new();
        let identity = Arc::new(());
        let (state, dispatch_receiver) =
            test_state_with_dispatch(Some(sink), Some((identity, server_stream)));
        state
            .sessions
            .lock()
            .unwrap()
            .insert(id, Arc::clone(session));
        for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
            state.dispatch.try_send(DispatchMessage::Stop).unwrap();
        }
        (state, dispatch_receiver, client_stream)
    }

    fn cleanup_test_session(session: &Session, events: JoinHandle<()>) {
        let _ = session.set_paused(false);
        let _ = session.terminate(Duration::from_secs(2));
        let _ = events.join();
    }

    #[test]
    fn dashboard_overflow_closes_affected_connection() {
        let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
        client_stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let sink = DashboardSink::new();
        for _ in 0..DASHBOARD_QUEUE {
            assert!(sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                    projects: Vec::new()
                },)),
                completion: None,
            }));
        }
        let identity = Arc::new(());
        let state = test_state(Some(sink), Some((identity, server_stream)));
        dashboard_try_send(
            &state,
            ServerMessage::Event(ServerEvent::ScreenDirty {
                session: SessionId(2),
            }),
        );
        let mut byte = [0_u8; 1];
        assert_eq!(client_stream.read(&mut byte).unwrap(), 0);
        assert!(state.dashboard.lock().unwrap().is_none());
    }

    #[test]
    fn dashboard_overflow_does_not_clear_replacement_slot() {
        let (old_server, mut old_client) = UnixStream::pair().unwrap();
        let _old_handler_stream = old_server.try_clone().unwrap();
        let _old_writer_stream = old_server.try_clone().unwrap();
        old_client
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let old_sink = DashboardSink::new();
        let old_identity = Arc::new(());
        let state = test_state(Some(old_sink), Some((old_identity, old_server)));
        let old_snapshot = dashboard_snapshot(&state).unwrap();

        let (new_server, _new_client) = UnixStream::pair().unwrap();
        let new_sink = DashboardSink::new();
        let new_identity = Arc::new(());
        *state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
            sink: new_sink.clone(),
            identity: new_identity,
            stream: new_server,
        });
        *state.dashboard.lock().unwrap() = Some(new_sink);
        disconnect_dashboard(&state, old_snapshot);

        assert!(state.dashboard.lock().unwrap().is_some());
        assert!(matches!(old_client.read(&mut [0_u8; 1]), Ok(0)));
    }

    #[test]
    fn session_refresh_dispatch_failures_disconnect_without_blocking() {
        let id = SessionId(3);
        let (_cwd, session, receiver) = spawn_live_test_session(id);
        let events = apply_test_session_events(Arc::clone(&session), receiver);
        let (state, dispatch_receiver, mut client_stream) = saturated_control_state(&session, id);
        let mut dispatch_receiver = Some(dispatch_receiver);
        client_stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let (completion, result) = mpsc::sync_channel(1);
        let control_state = Arc::clone(&state);
        let worker = thread::spawn(move || {
            let outcome = control_state.set_session_paused(id, true);
            let _ = completion.send(outcome);
        });
        let outcome = match result.recv_timeout(Duration::from_millis(250)) {
            Ok(outcome) => outcome,
            Err(error) => {
                drop(dispatch_receiver.take());
                let _ = result.recv_timeout(Duration::from_secs(1));
                let _ = worker.join();
                cleanup_test_session(&session, events);
                panic!("pause control must not block on a full dispatch queue: {error}");
            }
        };
        worker.join().unwrap();
        let error = outcome.unwrap_err();
        let partial_failure = error
            .downcast_ref::<LifecycleFailure>()
            .is_some_and(|failure| failure.code == ErrorCode::PartialFailure);
        let error_message = error_chain_string(&error);
        let pause_preserved = matches!(session.summary().phase, SessionPhase::Paused);
        let dashboard_closed = matches!(client_stream.read(&mut [0_u8; 1]), Ok(0));
        drop(dispatch_receiver.take());
        cleanup_test_session(&session, events);
        assert!(partial_failure);
        assert!(error_message.contains("dispatcher queue is full"));
        assert!(pause_preserved);
        assert!(dashboard_closed);

        let id = SessionId(4);
        let (_cwd, session, receiver) = spawn_live_test_session(id);
        let events = apply_test_session_events(Arc::clone(&session), receiver);
        let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
        client_stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let sink = DashboardSink::new();
        let identity = Arc::new(());
        let (state, dispatch_receiver) =
            test_state_with_dispatch(Some(sink), Some((identity, server_stream)));
        state
            .sessions
            .lock()
            .unwrap()
            .insert(id, Arc::clone(&session));
        drop(dispatch_receiver);
        let error = state.set_session_paused(id, true).unwrap_err();
        let pause_preserved = matches!(session.summary().phase, SessionPhase::Paused);
        let dashboard_closed = matches!(client_stream.read(&mut [0_u8; 1]), Ok(0));
        cleanup_test_session(&session, events);
        assert!(error.to_string().contains("dispatcher is unavailable"));
        assert!(pause_preserved);
        assert!(dashboard_closed);
    }

    #[test]
    fn kill_and_close_refresh_failures_keep_exited_records() {
        for (id, close) in [(SessionId(5), false), (SessionId(6), true)] {
            let (_cwd, session, receiver) = spawn_live_test_session(id);
            let events = apply_test_session_events(Arc::clone(&session), receiver);
            let (state, dispatch_receiver, mut client_stream) =
                saturated_control_state(&session, id);
            client_stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let error = if close {
                state.close_terminal(id, Duration::from_millis(250))
            } else {
                state.kill_session(id, Duration::from_millis(250))
            }
            .unwrap_err();
            let lifecycle = error.downcast_ref::<LifecycleFailure>().unwrap();
            assert_eq!(lifecycle.code, ErrorCode::PartialFailure);
            assert!(error.to_string().contains("dispatcher queue is full"));
            assert!(matches!(
                session.summary().phase,
                SessionPhase::Exited { .. }
            ));
            assert!(state.sessions.lock().unwrap().contains_key(&id));
            assert_eq!(client_stream.read(&mut [0_u8; 1]).unwrap(), 0);
            drop(dispatch_receiver);
            state.remove_session(id).unwrap();
            events.join().unwrap();
            assert!(!state.sessions.lock().unwrap().contains_key(&id));
        }
    }

    #[test]
    fn shutdown_refresh_failure_keeps_exited_record_and_server_available() {
        let id = SessionId(7);
        let (_cwd, session, receiver) = spawn_live_test_session(id);
        let events = apply_test_session_events(Arc::clone(&session), receiver);
        let (state, dispatch_receiver, mut client_stream) = saturated_control_state(&session, id);
        client_stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let response = state.request_shutdown(true);
        assert!(matches!(
            response,
            Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            }
        ));
        assert!(!state.stopping.load(Ordering::Acquire));
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
        assert!(state.sessions.lock().unwrap().contains_key(&id));
        assert_eq!(client_stream.read(&mut [0_u8; 1]).unwrap(), 0);
        drop(dispatch_receiver);
        state.remove_session(id).unwrap();
        events.join().unwrap();
        assert_eq!(state.request_shutdown(false), Response::Ok);
    }

    #[test]
    fn control_and_refresh_failures_preserve_both_causes() {
        let id = SessionId(8);
        let (_cwd, session, receiver) = {
            let cwd = tempfile::tempdir().unwrap();
            let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
            let session = Session::spawn(
                id,
                SessionSpec {
                    project: "p".into(),
                    workspace: "w".into(),
                    name: "exited".into(),
                    label: "sh".into(),
                    cwd: cwd.path().to_path_buf(),
                    argv: vec!["sh".into(), "-c".into(), "exit 0".into()],
                },
                TerminalSize { rows: 24, cols: 80 },
                events,
            )
            .unwrap();
            (cwd, session, receiver)
        };
        let events = apply_test_session_events(Arc::clone(&session), receiver);
        session.wait_until_exited(Duration::from_secs(2)).unwrap();
        events.join().unwrap();
        let state = test_state(None, None);
        state
            .sessions
            .lock()
            .unwrap()
            .insert(id, Arc::clone(&session));
        let error = state.set_session_paused(id, true).unwrap_err();
        let lifecycle = error.downcast_ref::<LifecycleFailure>().unwrap();
        assert_eq!(lifecycle.code, ErrorCode::Conflict);
        let message = error_chain_string(&error);
        assert!(message.contains("session has exited"));
        assert!(message.contains("dispatcher is unavailable"));
        session.terminate(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn dashboard_shutdown_waits_for_stalled_writer_completion() {
        let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
        let send_buffer = 1_i32;
        let result = unsafe {
            libc::setsockopt(
                server_stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&send_buffer as *const i32).cast(),
                std::mem::size_of_val(&send_buffer) as libc::socklen_t,
            )
        };
        assert_eq!(result, 0);
        let state = test_state(None, None);
        state.registry.lock().unwrap().projects = (0..256)
            .map(|index| crate::config::ProjectRecord {
                name: format!("project-{index}-{}", "x".repeat(2_000)),
                repo: PathBuf::from(format!("/repo/{index}")),
                workspace_root: PathBuf::from(format!("/workspace/{index}")),
                workspaces: Vec::new(),
            })
            .collect();
        let handler_state = Arc::clone(&state);
        let handler = thread::spawn(move || handle_connection(handler_state, server_stream));
        write_frame(
            &mut client_stream,
            &ClientMessage {
                request_id: 0,
                request: Request::DashboardHello,
            },
        )
        .unwrap();
        write_frame(
            &mut client_stream,
            &ClientMessage {
                request_id: 1,
                request: Request::Shutdown { kill: false },
            },
        )
        .unwrap();
        thread::park_timeout(Duration::from_millis(100));
        let shutdown_before_drain = state.shutdown.load(Ordering::Acquire);

        client_stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let _ = read_frame::<ServerMessage>(&mut client_stream).unwrap();
        assert!(matches!(
            read_frame::<ServerMessage>(&mut client_stream).unwrap(),
            ServerMessage::Response {
                request_id: 1,
                response: Response::Ok,
            }
        ));
        handler.join().unwrap();
        assert!(
            !shutdown_before_drain,
            "shutdown must wait for the writer's actual response attempt"
        );
        assert!(state.shutdown.load(Ordering::Acquire));
    }

    #[test]
    fn accepted_shutdown_rejects_late_mutation_while_ack_writer_is_blocked() {
        let (dashboard_server, mut dashboard_client) = UnixStream::pair().unwrap();
        let send_buffer = 1_i32;
        let result = unsafe {
            libc::setsockopt(
                dashboard_server.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&send_buffer as *const i32).cast(),
                std::mem::size_of_val(&send_buffer) as libc::socklen_t,
            )
        };
        assert_eq!(result, 0);
        let state = test_state(None, None);
        state.registry.lock().unwrap().projects = (0..256)
            .map(|index| crate::config::ProjectRecord {
                name: format!("project-{index}-{}", "x".repeat(2_000)),
                repo: PathBuf::from(format!("/repo/{index}")),
                workspace_root: PathBuf::from(format!("/workspace/{index}")),
                workspaces: Vec::new(),
            })
            .collect();
        state
            .registry
            .lock()
            .unwrap()
            .projects
            .push(crate::config::ProjectRecord {
                name: "late".into(),
                repo: PathBuf::from("/repo/late"),
                workspace_root: PathBuf::from("/workspace/late"),
                workspaces: Vec::new(),
            });
        let dashboard_state = Arc::clone(&state);
        let dashboard_handler =
            thread::spawn(move || handle_connection(dashboard_state, dashboard_server));
        let (control_server, mut control_client) = UnixStream::pair().unwrap();
        let control_state = Arc::clone(&state);
        let control_handler =
            thread::spawn(move || handle_connection(control_state, control_server));
        write_frame(
            &mut dashboard_client,
            &ClientMessage {
                request_id: 1,
                request: Request::DashboardHello,
            },
        )
        .unwrap();
        write_frame(
            &mut dashboard_client,
            &ClientMessage {
                request_id: 2,
                request: Request::Shutdown { kill: false },
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !state.stopping.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "shutdown did not enter stopping state"
            );
            thread::yield_now();
        }
        control_client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write_frame(
            &mut control_client,
            &ClientMessage {
                request_id: 3,
                request: Request::RemoveProject {
                    name: "late".into(),
                },
            },
        )
        .unwrap();
        assert_eq!(
            read_frame::<ServerMessage>(&mut control_client).unwrap(),
            ServerMessage::Response {
                request_id: 3,
                response: Response::Error {
                    code: ErrorCode::Conflict,
                    message: "server is stopping".into(),
                },
            }
        );
        let _ = control_client.shutdown(Shutdown::Both);
        control_handler.join().unwrap();
        for (request_id, request) in [
            (
                4,
                Request::PauseSession {
                    session: SessionId(99),
                },
            ),
            (
                5,
                Request::ResumeSession {
                    session: SessionId(99),
                },
            ),
        ] {
            let (control_server, mut control_client) = UnixStream::pair().unwrap();
            control_client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let control_state = Arc::clone(&state);
            let control_handler =
                thread::spawn(move || handle_connection(control_state, control_server));
            write_frame(
                &mut control_client,
                &ClientMessage {
                    request_id,
                    request,
                },
            )
            .unwrap();
            assert_eq!(
                read_frame::<ServerMessage>(&mut control_client).unwrap(),
                ServerMessage::Response {
                    request_id,
                    response: Response::Error {
                        code: ErrorCode::Conflict,
                        message: "server is stopping".into(),
                    },
                }
            );
            let _ = control_client.shutdown(Shutdown::Both);
            control_handler.join().unwrap();
        }
        dashboard_client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        loop {
            if matches!(
                read_frame::<ServerMessage>(&mut dashboard_client).unwrap(),
                ServerMessage::Response {
                    request_id: 2,
                    response: Response::Ok,
                }
            ) {
                break;
            }
        }
        dashboard_handler.join().unwrap();
        assert!(state.shutdown.load(Ordering::Acquire));
    }

    #[test]
    fn uncommitted_partial_failure_does_not_publish_hierarchy() {
        let sink = DashboardSink::new();
        let (server_stream, _client_stream) = UnixStream::pair().unwrap();
        let state = test_state(Some(sink.clone()), Some((Arc::new(()), server_stream)));
        let response = lifecycle_response_with_partial_hierarchy(
            &state,
            lifecycle_error(
                ErrorCode::PartialFailure,
                "registry write failed: worktree remains",
            ),
        );
        assert!(matches!(
            response,
            Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            }
        ));
        assert!(
            sink.queue.lock().unwrap().messages.is_empty(),
            "uncommitted partial failure must not publish unchanged hierarchy"
        );
    }

    #[test]
    fn shutdown_termination_failure_is_partial_and_server_remains_available() {
        let cwd = tempfile::tempdir().unwrap();
        let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let session = Session::spawn(
            SessionId(7),
            crate::session::SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "fault".into(),
                label: "sh".into(),
                cwd: cwd.path().to_path_buf(),
                argv: vec!["sh".into()],
            },
            TerminalSize { rows: 24, cols: 80 },
            events,
        )
        .unwrap();
        let dispatch_session = Arc::clone(&session);
        let waiter = thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                let exited = matches!(event, SessionEvent::Exited { .. });
                dispatch_session.apply_event(event);
                if exited {
                    break;
                }
            }
        });
        let state = test_state(None, None);
        state
            .sessions
            .lock()
            .unwrap()
            .insert(SessionId(7), Arc::clone(&session));
        let response = handle_shutdown(&state, true, |_| {
            Err(anyhow::anyhow!("controlled ownership failure"))
        });
        assert!(matches!(
            response,
            Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            }
        ));
        assert!(!state.shutdown.load(Ordering::Acquire));
        assert!(matches!(session.summary().phase, SessionPhase::Running));
        session.terminate(Duration::from_secs(2)).unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn shutdown_without_kill_rejects_paused_session() {
        let cwd = tempfile::tempdir().unwrap();
        let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let session = Session::spawn(
            SessionId(8),
            crate::session::SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "paused".into(),
                label: "sh".into(),
                cwd: cwd.path().to_path_buf(),
                argv: vec!["sh".into()],
            },
            TerminalSize { rows: 24, cols: 80 },
            events,
        )
        .unwrap();
        let dispatch_session = Arc::clone(&session);
        let waiter = thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                let exited = matches!(event, SessionEvent::Exited { .. });
                dispatch_session.apply_event(event);
                if exited {
                    break;
                }
            }
        });
        session.set_paused(true).unwrap();
        let state = test_state(None, None);
        state
            .sessions
            .lock()
            .unwrap()
            .insert(SessionId(8), Arc::clone(&session));
        assert!(matches!(
            handle_shutdown(&state, false, |_| panic!("paused session was not guarded")),
            Response::Error {
                code: ErrorCode::SessionsRemain,
                ..
            }
        ));
        session.terminate(Duration::from_secs(2)).unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn close_failure_retains_record_until_cleanup_can_finish() {
        let cwd = tempfile::tempdir().unwrap();
        let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let id = SessionId(17);
        let session = Session::spawn(
            id,
            crate::session::SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "retained".into(),
                label: "sh".into(),
                cwd: cwd.path().to_path_buf(),
                argv: vec!["sh".into()],
            },
            TerminalSize { rows: 24, cols: 80 },
            events,
        )
        .unwrap();
        let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
        state
            .sessions
            .lock()
            .unwrap()
            .insert(id, Arc::clone(&session));
        *state.selected.lock().unwrap() = Some(id);

        let error = state
            .close_terminal(id, Duration::from_millis(20))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("timed out waiting for session exit"),
            "unexpected close failure: {error:#}"
        );
        assert!(state.sessions.lock().unwrap().contains_key(&id));
        assert_eq!(*state.selected.lock().unwrap(), Some(id));

        loop {
            let event = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            let exited = matches!(event, SessionEvent::Exited { .. });
            session.apply_event(event);
            if exited {
                break;
            }
        }
        state.close_terminal(id, Duration::from_millis(20)).unwrap();
        assert!(!state.sessions.lock().unwrap().contains_key(&id));
        assert_eq!(*state.selected.lock().unwrap(), None);
    }

    #[test]
    fn registration_holds_sessions_guard_until_spawn_returns() {
        let root = tempfile::tempdir().unwrap();
        let (events, event_receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let (dispatch, dispatch_receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let registry = Registry {
            projects: vec![crate::config::ProjectRecord {
                name: "project".into(),
                repo: root.path().to_path_buf(),
                workspace_root: root.path().to_path_buf(),
                workspaces: vec![crate::config::WorkspaceRecord {
                    name: "workspace".into(),
                    path: workspace.clone(),
                    branch: "main".into(),
                }],
            }],
        };
        let state = Arc::new(ServerState {
            socket: root.path().join("socket"),
            registry_path: root.path().join("config.toml"),
            registry: Mutex::new(registry),
            sessions: Mutex::new(HashMap::new()),
            selected: Mutex::new(None),
            dashboard: Mutex::new(None),
            next_session_id: AtomicU64::new(1),
            mutation_lock: Mutex::new(()),
            dispatch: dispatch.clone(),
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            dashboard_size: Mutex::new(None),
            events: Mutex::new(Some(events)),
            dashboard_slot: Mutex::new(None),
        });
        let dispatcher_state = Arc::clone(&state);
        let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
        let bridge_dispatch = dispatch.clone();
        let bridge = thread::spawn(move || bridge_events(event_receiver, bridge_dispatch));
        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let ready: Arc<dyn Fn() + Send + Sync> = {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Arc::new(move || {
                entered.wait();
                release.wait();
            })
        };
        let creator_state = Arc::clone(&state);
        let creator_ready = Arc::clone(&ready);
        let creator = thread::spawn(move || {
            creator_state.create_session_with_ready(
                crate::protocol::CreateSessionRequest {
                    project: "project".into(),
                    workspace: "workspace".into(),
                    name: "fast".into(),
                    label: None,
                    argv: vec!["sh".into(), "-c".into(), "printf retained".into()],
                },
                creator_ready,
            )
        });
        entered.wait();
        assert!(
            state.sessions.try_lock().is_err(),
            "registration must hold sessions guard while Session::spawn is paused"
        );
        release.wait();
        let summary = creator.join().unwrap().unwrap();
        assert_eq!(summary.name, "fast");
        assert!(state.sessions.lock().unwrap().contains_key(&summary.id));
        let session = state
            .sessions
            .lock()
            .unwrap()
            .get(&summary.id)
            .cloned()
            .unwrap();
        session.wait_until_exited(Duration::from_secs(2)).unwrap();
        let _ = dispatch.send(DispatchMessage::Stop);
        dispatcher.join().unwrap();
        state.events.lock().unwrap().take();
        drop(dispatch);
        bridge.join().unwrap();
    }
}
