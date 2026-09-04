use crate::config::Registry;
use crate::protocol::{
    ClientMessage, ClientRole, DispatchMessage, ErrorCode, HierarchySnapshot, ProjectSummary,
    Request, Response, ServerEvent, ServerMessage, WorkspaceSummary, read_frame, write_frame,
};
use crate::session::{Session, SessionEvent, SessionId, SessionPhase, TerminalSize};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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
    pub dashboard: Mutex<Option<SyncSender<ServerMessage>>>,
    pub next_session_id: AtomicU64,
    pub mutation_lock: Mutex<()>,
    pub dispatch: SyncSender<DispatchMessage>,
    pub shutdown: AtomicBool,
    pub events: Mutex<Option<SyncSender<SessionEvent>>>,
    dashboard_identity: Mutex<Option<Arc<()>>>,
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
        dashboard_identity: Mutex::new(None),
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
    *state.dashboard.lock().unwrap() = None;
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
    *state.selected.lock().unwrap() = Some(id);
    dashboard_try_send(
        state,
        response_message(
            request_id,
            Response::Screen {
                session: id,
                size,
                bytes: session.current_screen(),
            },
        ),
    );
}

fn handle_connection(state: Arc<ServerState>, mut stream: UnixStream) {
    let mut role = ClientRole::Control;
    let mut dashboard_identity = None;
    let (dashboard_tx, dashboard_rx) = mpsc::sync_channel(DASHBOARD_QUEUE);
    let mut dashboard_rx = Some(dashboard_rx);
    let mut writer: Option<JoinHandle<()>> = None;
    while !state.shutdown.load(Ordering::Acquire) {
        let message = match read_frame::<ClientMessage>(&mut stream) {
            Ok(message) => message,
            Err(_) => break,
        };
        if matches!(message.request, Request::DashboardHello) {
            if writer.is_some() {
                let _ = send_direct(
                    &mut stream,
                    message.request_id,
                    Response::Error {
                        code: ErrorCode::InvalidRequest,
                        message: "dashboard already registered".into(),
                    },
                );
                continue;
            }
            let mut dashboard = state.dashboard.lock().unwrap();
            if dashboard.is_some() {
                let _ = send_direct(
                    &mut stream,
                    message.request_id,
                    Response::Error {
                        code: ErrorCode::Conflict,
                        message: "dashboard already connected".into(),
                    },
                );
                continue;
            }
            *dashboard = Some(dashboard_tx.clone());
            drop(dashboard);
            let identity = Arc::new(());
            *state.dashboard_identity.lock().unwrap() = Some(identity.clone());
            dashboard_identity = Some(identity);
            role = ClientRole::Dashboard;
            let mut output = stream.try_clone().ok();
            let writer_receiver = dashboard_rx.take().expect("dashboard writer receiver");
            writer = Some(
                thread::Builder::new()
                    .name("ovrcr-dashboard-writer".into())
                    .spawn(move || {
                        while let Ok(message) = writer_receiver.recv() {
                            let Some(ref mut stream) = output else { break };
                            if write_frame(stream, &message).is_err() {
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
        let response = handle_request(&state, &message.request, message.request_id);
        match role {
            ClientRole::Control => {
                if send_direct(&mut stream, message.request_id, response).is_err() {
                    break;
                }
            }
            ClientRole::Dashboard => {
                if !select {
                    dashboard_try_send(&state, response_message(message.request_id, response));
                }
            }
        }
        if shutdown && state.shutdown.load(Ordering::Acquire) {
            wake_accept(&state);
        }
        if matches!(role, ClientRole::Control) {
            break;
        }
    }
    {
        let mut dashboard = state.dashboard.lock().unwrap();
        if dashboard_identity.as_ref().is_some_and(|identity| {
            state
                .dashboard_identity
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, identity))
        }) {
            *dashboard = None;
            *state.dashboard_identity.lock().unwrap() = None;
        }
    }
    drop(dashboard_tx);
    if let Some(writer) = writer {
        let _ = writer.join();
    }
}

fn handle_request(state: &Arc<ServerState>, request: &Request, request_id: u64) -> Response {
    match request {
        Request::List => Response::Hierarchy(snapshot(state)),
        Request::Select { session, size } => {
            let _ = state.dispatch.send(DispatchMessage::Select {
                request_id,
                session: *session,
                size: *size,
            });
            Response::Ok
        }
        Request::Input { session, bytes } => {
            match state.sessions.lock().unwrap().get(session).cloned() {
                Some(session) => session.write(bytes).map_or_else(
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
            match state.sessions.lock().unwrap().get(session).cloned() {
                Some(session) => session.resize(*size).map_or_else(
                    |error| error_response(ErrorCode::Internal, error.to_string()),
                    |_| Response::Ok,
                ),
                None => error_response(
                    ErrorCode::NotFound,
                    format!("session {} not found", session.0),
                ),
            }
        }
        Request::Shutdown { kill } => {
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
                return error_response(ErrorCode::SessionsRemain, "sessions remain".into());
            }
            if *kill {
                for session in sessions {
                    let _ = session.terminate(Duration::from_secs(5));
                }
            }
            state.shutdown.store(true, Ordering::Release);
            Response::Ok
        }
        Request::DashboardHello => Response::Ok,
        _ => error_response(
            ErrorCode::InvalidRequest,
            "command handling belongs to Task 5".into(),
        ),
    }
}

fn error_response(code: ErrorCode, message: String) -> Response {
    Response::Error { code, message }
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

fn dashboard_try_send(state: &Arc<ServerState>, message: ServerMessage) {
    let sender = state.dashboard.lock().unwrap().clone();
    let Some(sender) = sender else { return };
    if matches!(
        sender.try_send(message),
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_))
    ) {
        *state.dashboard.lock().unwrap() = None;
        *state.dashboard_identity.lock().unwrap() = None;
    }
}

fn snapshot(state: &Arc<ServerState>) -> HierarchySnapshot {
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
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            let _ = fs::remove_file(&paths.socket);
            Ok(None)
        }
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
