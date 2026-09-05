use crate::config::{ProjectRecord, Registry};
use crate::git::{self, BranchSpec};
use crate::protocol::{
    BranchRequest, ClientMessage, ClientRole, DispatchMessage, ErrorCode, HierarchySnapshot,
    ProjectSummary, Request, Response, ServerEvent, ServerMessage, WorkspaceSummary, read_frame,
    write_frame,
};
use crate::session::{
    Session, SessionEvent, SessionId, SessionPhase, SessionSpec, SessionSummary, TerminalSize,
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

const EVENT_QUEUE: usize = 64;
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
    pub events: Mutex<Option<SyncSender<SessionEvent>>>,
    dashboard_slot: Mutex<Option<DashboardSlot>>,
}

impl ServerState {
    pub fn hierarchy(&self) -> HierarchySnapshot {
        snapshot_from_state(self)
    }

    pub fn handle_request(&self, role: &mut ClientRole, request: Request) -> Response {
        handle_request_with_id(self, role, request, 0)
    }

    pub fn create_session(
        &self,
        request: crate::protocol::CreateSessionRequest,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.create_session_locked(request, None)
    }

    #[cfg(test)]
    pub(crate) fn create_session_with_ready(
        &self,
        request: crate::protocol::CreateSessionRequest,
        ready: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
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
        let size = TerminalSize {
            rows: 40,
            cols: 120,
        };
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
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
        session.terminate(grace)
    }

    pub fn remove_session(&self, id: SessionId) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
        if matches!(session.summary().phase, SessionPhase::Running) {
            return Err(lifecycle_error(
                ErrorCode::SessionRunning,
                "session is still running",
            ));
        }
        self.sessions.lock().unwrap().remove(&id);
        if self.selected.lock().unwrap().as_ref() == Some(&id) {
            *self.selected.lock().unwrap() = None;
        }
        Ok(())
    }

    pub fn request_shutdown(&self, kill: bool) -> Response {
        let _mutation = self.mutation_lock.lock().unwrap();
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
                if let Err(error) = session.terminate(Duration::from_secs(5)) {
                    failures.push(format!("session {}: {error}", session.summary().id.0));
                }
            }
            if !failures.is_empty() {
                return error_response(ErrorCode::PartialFailure, failures.join("; "));
            }
        }
        Response::Ok
    }

    pub fn add_project(&self, name: String, repo: PathBuf, workspace_root: PathBuf) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
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
                        "registry write failed: worktree remains at {}: {error}",
                        workspace.path.display()
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
                    "worktree for workspace {} remains at {}: {error}",
                    name,
                    workspace.path.display()
                ),
                true,
            ));
        }
        Ok(())
    }

    pub fn remove_workspace(&self, project: &str, name: &str) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
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
                    "worktree was removed at {} but registry update failed; live state reflects removal: {error}",
                    workspace.path.display()
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
    let (events, event_receiver) = mpsc::sync_channel(EVENT_QUEUE);
    let (dispatch, dispatch_receiver) = mpsc::sync_channel(EVENT_QUEUE);
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
            DispatchMessage::Select {
                request_id,
                session,
                size,
            } => dispatch_select(&state, request_id, session, size),
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

fn dispatch_select(state: &Arc<ServerState>, request_id: u64, id: SessionId, size: TerminalSize) {
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
        return;
    }
    let previous = *state.selected.lock().unwrap();
    let Some(snapshot) = dashboard_snapshot(state) else {
        return;
    };
    if !snapshot
        .sink
        .replace_selection(previous, id, request_id, size, session.current_screen())
    {
        disconnect_dashboard(state, snapshot);
        return;
    }
    *state.selected.lock().unwrap() = Some(id);
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
        Request::Select { session, size } => {
            if !dashboard {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "Select requires a dashboard connection",
                );
            }
            let _ = state.dispatch.send(DispatchMessage::Select {
                request_id,
                session,
                size,
            });
            Response::Ok
        }
        Request::Input { session, bytes } => {
            if !dashboard || state.selected.lock().unwrap().as_ref() != Some(&session) {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "input is only accepted for the selected dashboard session",
                );
            }
            match state.sessions.lock().unwrap().get(&session).cloned() {
                Some(session) => session.write(&bytes).map_or_else(
                    |error| error_response(ErrorCode::Internal, error.to_string()),
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
            match state.sessions.lock().unwrap().get(&session).cloned() {
                Some(session) => session.resize(size).map_or_else(
                    |error| error_response(ErrorCode::Internal, error.to_string()),
                    |_| Response::Ok,
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
            .kill_session(session, Duration::from_secs(5))
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
    }
}

fn error_for_lifecycle(error: anyhow::Error) -> Response {
    let message = error.to_string();
    let code = error
        .downcast_ref::<LifecycleFailure>()
        .map(|failure| failure.code.clone())
        .unwrap_or(ErrorCode::Conflict);
    error_response(code, message)
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
        && sessions
            .iter()
            .any(|session| matches!(session.summary().phase, SessionPhase::Running))
    {
        return error_response(ErrorCode::SessionsRemain, "sessions remain");
    }
    if kill {
        let mut failures = Vec::new();
        for session in sessions {
            if let Err(error) = terminate(&session) {
                failures.push(format!("session {}: {error}", session.summary().id.0));
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

    fn test_state(
        dashboard: Option<Arc<DashboardSink>>,
        stream: Option<(Arc<()>, UnixStream)>,
    ) -> Arc<ServerState> {
        let (events, _) = mpsc::sync_channel(EVENT_QUEUE);
        let (dispatch, _) = mpsc::sync_channel(EVENT_QUEUE);
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
            events: Mutex::new(Some(events)),
            dashboard_slot: Mutex::new(stream.map(|(identity, stream)| DashboardSlot {
                sink: dashboard.as_ref().unwrap().clone(),
                identity,
                stream,
            })),
        })
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
        let (events, receiver) = mpsc::sync_channel(EVENT_QUEUE);
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
    fn registration_holds_sessions_guard_until_spawn_returns() {
        let root = tempfile::tempdir().unwrap();
        let (events, event_receiver) = mpsc::sync_channel(EVENT_QUEUE);
        let (dispatch, dispatch_receiver) = mpsc::sync_channel(EVENT_QUEUE);
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
