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

/// Test seam: make the next accepted connection's thread spawn fail.
#[cfg(test)]
pub(super) static FAIL_NEXT_ACCEPT_SPAWN: AtomicBool = AtomicBool::new(false);

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
    run_server_inner(paths, registry_path, None, None, None)
}

#[cfg(feature = "acceptance-diagnostics")]
pub fn run_server_with_diagnostics(
    paths: ServerPaths,
    registry_path: PathBuf,
    diagnostics: ServerQueueDiagnostics,
) -> Result<()> {
    run_server_inner(
        paths,
        registry_path,
        Some(diagnostics.raw_events),
        Some(diagnostics.dispatcher),
        Some(diagnostics.dashboard),
    )
}

fn run_server_inner(
    paths: ServerPaths,
    registry_path: PathBuf,
    event_monitor: Option<reporting_queue::ReportingQueueMonitor>,
    dispatch_monitor: Option<reporting_queue::ReportingQueueMonitor>,
    #[cfg(feature = "acceptance-diagnostics")] dashboard_monitor: Option<DashboardQueueMonitor>,
    #[cfg(not(feature = "acceptance-diagnostics"))] _dashboard_monitor: Option<()>,
) -> Result<()> {
    let parent = paths
        .socket
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    secure_socket_directory(parent)?;
    let bound_socket = resolve_bound_socket(&paths.socket)?;
    eprintln!("ovrcr server starting for {}", paths.socket.display());
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
    let titles_dir = bound_socket
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("titles");
    let _ = fs::remove_dir_all(&titles_dir);
    let listener = UnixListener::bind(&paths.socket)
        .with_context(|| format!("bind server socket {}", paths.socket.display()))?;
    fs::set_permissions(&paths.socket, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("secure server socket {}", paths.socket.display()))?;
    drop(startup_lock);
    eprintln!("ovrcr server listening on {}", paths.socket.display());
    // The socket is bound so concurrent starters see a live server, but a
    // failure from here on must not leave a dead socket file behind.
    let loaded = (|| -> Result<_> {
        let registry = initialize_registry(&registry_path)
            .with_context(|| format!("load server registry {}", registry_path.display()))?;
        let task_manager = crate::task_manager::TaskManager::open(&registry_path)?;
        let retained = SessionStore::open(&registry_path)?;
        Ok((registry, task_manager, retained))
    })();
    let (registry, task_manager, retained) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            drop(listener);
            let _ = fs::remove_file(&paths.socket);
            return Err(error);
        }
    };
    let settings = watch::Watched::load(&registry_path);
    for finding in &settings.report.findings {
        eprintln!(
            "settings finding in {}: {} {}{}",
            settings.report.path.display(),
            finding.key.as_deref().unwrap_or("(document)"),
            finding.message,
            finding
                .line
                .map(|line| format!(" (line {line})"))
                .unwrap_or_default()
        );
    }
    let (events, event_receiver) = event_channel(event_monitor.as_ref());
    let (dispatch, dispatch_receiver) = dispatch_channel(dispatch_monitor.as_ref());
    let state = Arc::new(ServerState {
        tasks: Some(Arc::clone(&task_manager)),
        socket: bound_socket,
        registry_path,
        registry: Mutex::new(registry),
        sessions: Mutex::new(HashMap::new()),
        dashboard: ActiveDashboard::default(),
        quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
        settings: Mutex::new(settings),
        retained: parking_lot::Mutex::new(retained),
        observations: parking_lot::Mutex::new(HashMap::new()),
        mutation_lock: Mutex::new(()),
        dispatch: dispatch.clone(),
        shutdown: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        events: Mutex::new(Some(events)),
        #[cfg(test)]
        resize_hook: Mutex::new(None),
        #[cfg(test)]
        before_view_publish_hook: Mutex::new(None),
        #[cfg(test)]
        before_dashboard_write_hook: Mutex::new(None),
        #[cfg(feature = "acceptance-diagnostics")]
        dashboard_monitor,
    });
    task_manager.start(Arc::downgrade(&state));
    let bridge_dispatch = dispatch.clone();
    let bridge = thread::Builder::new()
        .name("ovrcr-event-bridge".into())
        .spawn(move || bridge_events(event_receiver, bridge_dispatch))?;
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::Builder::new()
        .name("ovrcr-dispatcher".into())
        .spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver))?;
    let quota_state = Arc::clone(&state);
    let quota_thread = thread::Builder::new()
        .name("ovrcr-codex-quota".into())
        .spawn(move || quota::run(quota_state, ovrcr_protocol::QuotaProvider::Codex))?;
    let grok_state = Arc::clone(&state);
    let grok_quota_thread = thread::Builder::new()
        .name("ovrcr-grok-quota".into())
        .spawn(move || quota::run(grok_state, ovrcr_protocol::QuotaProvider::Grok))?;
    state.ensure_protected_roots();
    let refresh_state = Arc::clone(&state);
    let refresh = thread::Builder::new()
        .name("ovrcr-workspace-refresh".into())
        .spawn(move || {
            while !refresh_state.shutdown.load(Ordering::Acquire) {
                thread::park_timeout(Duration::from_millis(200));
                if refresh_state.shutdown.load(Ordering::Acquire) {
                    break;
                }
                if refresh_state.dashboard.is_claimed() {
                    refresh_state.observe_checkouts();
                    match refresh_state
                        .dispatch
                        .try_send(DispatchMessage::RefreshHierarchy)
                    {
                        Ok(()) => {}
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                    }
                }
                for _ in 0..9 {
                    if refresh_state.shutdown.load(Ordering::Acquire) {
                        return;
                    }
                    thread::park_timeout(Duration::from_millis(200));
                }
            }
        })?;
    let title_state = Arc::clone(&state);
    let title_thread = thread::Builder::new()
        .name("ovrcr-title-worker".into())
        .spawn(move || title::TitleWorker::new(titles_dir).run(title_state))?;
    let mut signals = signal_hook::iterator::Signals::new([libc::SIGTERM, libc::SIGINT])?;
    let signal_handle = signals.handle();
    let signal_state = Arc::downgrade(&state);
    let signal_thread = thread::spawn(move || {
        for _signal in signals.forever() {
            let Some(state) = signal_state.upgrade() else {
                break;
            };
            let response = state.request_shutdown(true);
            if let Response::Error { message, .. } = &response {
                eprintln!("signal shutdown: {message}");
            }
            if state.stopping.load(Ordering::Acquire) {
                state.shutdown.store(true, Ordering::Release);
                wake_accept(&state);
                break;
            }
        }
    });
    for incoming in listener.incoming() {
        if state.shutdown.load(Ordering::Acquire) {
            break;
        }
        match incoming {
            Ok(stream) => {
                let connection_state = Arc::clone(&state);
                #[cfg(test)]
                let inject_failure = FAIL_NEXT_ACCEPT_SPAWN.swap(false, Ordering::AcqRel);
                #[cfg(not(test))]
                let inject_failure = false;
                let spawned = if inject_failure {
                    drop(stream);
                    Err(io::Error::other("injected thread spawn failure"))
                } else {
                    thread::Builder::new()
                        .name("ovrcr-client".into())
                        .spawn(move || handle_connection(connection_state, stream))
                };
                if let Err(error) = spawned {
                    // One EAGAIN under load must not take every PTY down
                    // with the server; drop this client and keep serving.
                    eprintln!("ovrcr server: cannot spawn client thread: {error}");
                    thread::sleep(Duration::from_millis(50));
                }
            }
            Err(_) if state.shutdown.load(Ordering::Acquire) => break,
            Err(error) => return Err(error).context("accept server client"),
        }
    }
    // Wake parked workers before joining them; shutdown must not wait for the next poll.
    refresh.thread().unpark();
    title_thread.thread().unpark();
    quota_thread.thread().unpark();
    grok_quota_thread.thread().unpark();
    signal_handle.close();
    let _ = signal_thread.join();
    let task_shutdown = task_manager.stop();
    if let Some(snapshot) = state.dashboard.snapshot() {
        state.dashboard.disconnect(snapshot);
    }
    let _ = quota_thread.join();
    let _ = grok_quota_thread.join();
    let _ = dispatch.send(DispatchMessage::Stop);
    dispatcher
        .join()
        .map_err(|_| anyhow::anyhow!("server dispatcher panicked"))?;
    let _ = refresh.join();
    let _ = title_thread.join();
    state.events.lock().unwrap().take();
    drop(state);
    drop(dispatch);
    bridge
        .join()
        .map_err(|_| anyhow::anyhow!("server event bridge panicked"))?;
    let _ = fs::remove_file(&paths.socket);
    match &task_shutdown {
        Ok(()) => eprintln!("ovrcr server stopped"),
        Err(error) => eprintln!("ovrcr server stopped with task shutdown error: {error:#}"),
    }
    task_shutdown
}

/// Prepare the socket's parent directory for a server that a client is
/// about to start, applying the same rules the server applies itself, and
/// return the log file path the detached server should write to.
pub fn prepare_socket_directory(socket: &Path) -> Result<PathBuf> {
    let parent = socket
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    secure_socket_directory(parent)?;
    Ok(parent.join("server.log"))
}
/// Prepare the socket's parent directory without modifying a directory OVRCR
/// did not create.
///
/// A missing directory is created with mode 0700. An existing directory must
/// be a real directory owned by the current user; startup refuses otherwise
/// with an error naming it. Its mode is left alone, because `OVRCR_SOCKET`
/// may point into a shared location such as the user's home directory; the
/// bound socket file is made mode 0700 instead, which is what gates
/// `connect`, so other users cannot reach the server even from a shared
/// directory.
fn secure_socket_directory(parent: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    match fs::symlink_metadata(parent) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                bail!(
                    "server socket directory {} is a symlink; point OVRCR_SOCKET at a real private directory",
                    parent.display()
                );
            }
            if !metadata.is_dir() {
                bail!(
                    "server socket directory {} is not a directory",
                    parent.display()
                );
            }
            if metadata.uid() != unsafe { libc::getuid() } {
                bail!(
                    "server socket directory {} is not owned by the current user",
                    parent.display()
                );
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .with_context(|| format!("create server socket directory {}", parent.display()))?;
            // The umask can only remove bits from 0700, but a racing creator
            // could have made the directory with a wider mode; verify it.
            let mode = fs::metadata(parent)
                .with_context(|| format!("inspect server socket directory {}", parent.display()))?
                .permissions()
                .mode()
                & 0o777;
            if mode & 0o077 != 0 {
                bail!(
                    "server socket directory {} was created with shared mode {mode:03o}",
                    parent.display()
                );
            }
            Ok(())
        }
        Err(error) => Err(error)
            .with_context(|| format!("inspect server socket directory {}", parent.display())),
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

pub(super) fn wake_accept(state: &Arc<ServerState>) {
    let _ = UnixStream::connect(&state.socket);
}
