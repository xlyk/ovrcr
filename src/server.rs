use crate::config::Registry;
use crate::protocol::{
    ClientMessage, ClientRole, DispatchMessage, ErrorCode, HierarchySnapshot, ProjectSummary,
    Request, Response, ServerEvent, ServerMessage, WorkspaceSummary, read_frame, write_frame,
};
use crate::session::{Session, SessionEvent, SessionId, SessionPhase, TerminalSize};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
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
    pub dashboard: Mutex<Option<SyncSender<DashboardOutbound>>>,
    pub next_session_id: AtomicU64,
    pub mutation_lock: Mutex<()>,
    pub dispatch: SyncSender<DispatchMessage>,
    pub shutdown: AtomicBool,
    pub events: Mutex<Option<SyncSender<SessionEvent>>>,
    dashboard_slot: Mutex<Option<DashboardSlot>>,
}

pub struct DashboardOutbound {
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
}

struct DashboardSlot {
    sender: SyncSender<DashboardOutbound>,
    identity: Arc<()>,
    stream: UnixStream,
}

struct DashboardSnapshot {
    sender: SyncSender<DashboardOutbound>,
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
                break;
            }
            let identity = Arc::new(());
            let Ok(close_stream) = stream.try_clone() else {
                break;
            };
            let mut slot = state.dashboard_slot.lock().unwrap();
            if slot.is_some() {
                break;
            }
            *slot = Some(DashboardSlot {
                sender: dashboard_tx.clone(),
                identity: identity.clone(),
                stream: close_stream,
            });
            drop(slot);
            *state.dashboard.lock().unwrap() = Some(dashboard_tx.clone());
            dashboard_identity = Some(identity);
            role = ClientRole::Dashboard;
            let mut output = stream.try_clone().ok();
            let writer_receiver = dashboard_rx.take().expect("dashboard writer receiver");
            writer = Some(
                thread::Builder::new()
                    .name("ovrcr-dashboard-writer".into())
                    .spawn(move || {
                        while let Ok(outbound) = writer_receiver.recv() {
                            let result = match output.as_mut() {
                                Some(stream) => write_frame(stream, &outbound.message)
                                    .map_err(|error| error.to_string()),
                                None => Err("dashboard writer stream unavailable".into()),
                            };
                            if let Some(completion) = outbound.completion {
                                let _ = completion.send(result.clone());
                            }
                            if result.is_err() {
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
            let _ = slot.take();
            *state.dashboard.lock().unwrap() = None;
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
        Request::Shutdown { kill } => handle_shutdown(state, *kill, |session| {
            session.terminate(Duration::from_secs(5))
        }),
        Request::DashboardHello => Response::Ok,
        _ => error_response(
            ErrorCode::InvalidRequest,
            "command handling belongs to Task 5".into(),
        ),
    }
}

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
        return error_response(ErrorCode::SessionsRemain, "sessions remain".into());
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

fn dashboard_try_send(state: &Arc<ServerState>, message: ServerMessage) -> bool {
    dashboard_send(state, message, None)
}

fn dashboard_send(
    state: &Arc<ServerState>,
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
) -> bool {
    let Some(snapshot) = dashboard_snapshot(state) else {
        return false;
    };
    if matches!(
        snapshot.sender.try_send(DashboardOutbound {
            message,
            completion,
        }),
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_))
    ) {
        disconnect_dashboard(state, snapshot);
        return false;
    }
    true
}

fn dashboard_snapshot(state: &Arc<ServerState>) -> Option<DashboardSnapshot> {
    let slot = state.dashboard_slot.lock().unwrap();
    let slot = slot.as_ref()?;
    Some(DashboardSnapshot {
        sender: slot.sender.clone(),
        identity: slot.identity.clone(),
        stream: slot.stream.try_clone().ok()?,
    })
}

fn disconnect_dashboard(state: &Arc<ServerState>, snapshot: DashboardSnapshot) {
    let _ = snapshot.stream.shutdown(std::net::Shutdown::Both);
    let mut slot = state.dashboard_slot.lock().unwrap();
    if slot
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.identity, &snapshot.identity))
    {
        let _ = slot.take();
        *state.dashboard.lock().unwrap() = None;
    }
}

fn acquire_startup_lock(parent: &Path) -> Result<File> {
    let path = parent.join(".server.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open server startup lock {}", path.display()))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error()).context("lock server startup");
    }
    Ok(file)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ServerEvent, ServerMessage};
    use std::io::Read;

    fn test_state(
        dashboard: Option<SyncSender<DashboardOutbound>>,
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
                sender: dashboard.as_ref().unwrap().clone(),
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
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::ScreenDirty {
                    session: SessionId(1),
                }),
                completion: None,
            })
            .unwrap();
        let identity = Arc::new(());
        let state = test_state(Some(sender), Some((identity, server_stream)));
        dashboard_try_send(
            &state,
            ServerMessage::Event(ServerEvent::ScreenDirty {
                session: SessionId(2),
            }),
        );
        let mut byte = [0_u8; 1];
        assert_eq!(client_stream.read(&mut byte).unwrap(), 0);
        assert!(state.dashboard.lock().unwrap().is_none());
        drop(receiver);
    }

    #[test]
    fn dashboard_overflow_does_not_clear_replacement_slot() {
        let (old_server, mut old_client) = UnixStream::pair().unwrap();
        let _old_handler_stream = old_server.try_clone().unwrap();
        let _old_writer_stream = old_server.try_clone().unwrap();
        old_client
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let (old_sender, _) = mpsc::sync_channel(1);
        let old_identity = Arc::new(());
        let state = test_state(Some(old_sender), Some((old_identity, old_server)));
        let old_snapshot = dashboard_snapshot(&state).unwrap();

        let (new_server, _new_client) = UnixStream::pair().unwrap();
        let (new_sender, _) = mpsc::sync_channel(1);
        let new_identity = Arc::new(());
        *state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
            sender: new_sender.clone(),
            identity: new_identity,
            stream: new_server,
        });
        *state.dashboard.lock().unwrap() = Some(new_sender);
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
}
