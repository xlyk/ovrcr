use super::*;

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

pub(super) fn resolve_bound_socket(socket: &Path) -> Result<PathBuf> {
    let leaf = socket
        .file_name()
        .filter(|name| !name.is_empty())
        .context("server socket path has no leaf")?;
    match fs::symlink_metadata(socket) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                bail!("server socket path cannot be a symlink")
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect server socket path"),
    }
    let parent = socket
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(fs::canonicalize(parent)
        .with_context(|| format!("resolve server socket parent {}", parent.display()))?
        .join(leaf))
}

pub(super) fn validate_bound_socket(socket: &Path) -> Result<PathBuf> {
    let resolved = resolve_bound_socket(socket)?;
    let metadata = fs::symlink_metadata(socket)
        .with_context(|| format!("inspect bound server socket {}", socket.display()))?;
    if !metadata.file_type().is_socket() {
        bail!("bound server socket is not a Unix socket")
    }
    Ok(resolved)
}

pub(super) fn generate_hook_capability() -> Result<[u8; 32]> {
    let mut capability = [0_u8; 32];
    let mut random = File::open("/dev/urandom").context("open hook capability source")?;
    random
        .read_exact(&mut capability)
        .context("read hook capability")?;
    Ok(capability)
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
    let bound_socket = resolve_bound_socket(&paths.socket)?;
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
    let registry = load_registry(&registry_path)
        .with_context(|| format!("load server registry {}", registry_path.display()))?;
    let (events, event_receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let (dispatch, dispatch_receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
    let state = Arc::new(ServerState {
        socket: bound_socket,
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

pub(super) fn wake_accept(state: &Arc<ServerState>) {
    let _ = UnixStream::connect(&state.socket);
}
