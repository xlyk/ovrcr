use ovrcr::config::Registry;
use ovrcr::protocol::{
    BranchRequest, ClientMessage, CreateSessionRequest, ErrorCode, Request, Response,
    ServerMessage, read_frame, write_frame,
};
use ovrcr::server::{ServerPaths, connect_if_running, connect_or_start, run_server};
use ovrcr::session::{SessionId, SessionPhase};
use std::ffi::OsString;
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct ServerFixture {
    root: tempfile::TempDir,
    paths: ServerPaths,
    thread: Option<thread::JoinHandle<()>>,
}

impl ServerFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = ServerPaths {
            socket: root.path().join("private").join("server.sock"),
        };
        Self {
            root,
            paths,
            thread: None,
        }
    }

    fn start(&mut self) {
        let registry = self.root.path().join("config.toml");
        Registry::default().save_atomic(&registry).unwrap();
        let paths = self.paths.clone();
        self.thread = Some(thread::spawn(move || run_server(paths, registry).unwrap()));
        self.wait_for_socket();
    }

    fn wait_for_socket(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if UnixStream::connect(&self.paths.socket).is_ok() {
                return;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        panic!(
            "server socket did not appear: {}",
            self.paths.socket.display()
        );
    }

    fn request(&self, request: Request) -> ServerMessage {
        let mut stream = UnixStream::connect(&self.paths.socket).unwrap();
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 1,
                request,
            },
        )
        .unwrap();
        read_frame::<ServerMessage>(&mut stream).unwrap()
    }

    fn stop(&mut self) {
        let response = self.request(Request::Shutdown { kill: false });
        assert_eq!(
            response,
            ServerMessage::Response {
                request_id: 1,
                response: Response::Ok
            }
        );
        let thread = self.thread.take().unwrap();
        thread.join().unwrap();
        assert!(!self.paths.socket.exists());
    }
}

#[test]
fn startup_socket_directory_is_private() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mode = std::fs::metadata(fixture.paths.socket.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    fixture.stop();
}

#[test]
fn startup_stale_socket_is_recovered() {
    let mut fixture = ServerFixture::new();
    std::fs::create_dir_all(fixture.paths.socket.parent().unwrap()).unwrap();
    let stale = UnixListener::bind(&fixture.paths.socket).unwrap();
    drop(stale);
    fixture.start();
    assert!(matches!(
        fixture.request(Request::List),
        ServerMessage::Response {
            response: Response::Hierarchy(_),
            ..
        }
    ));
    fixture.stop();
}

#[test]
fn startup_concurrent_attempts_leave_one_server() {
    let fixture = ServerFixture::new();
    let registry = fixture.root.path().join("config.toml");
    Registry::default().save_atomic(&registry).unwrap();
    let executable = env!("CARGO_BIN_EXE_ovrcr");
    unsafe {
        std::env::set_var("OVRCR_SERVER_EXECUTABLE", executable);
        std::env::set_var("OVRCR_SOCKET", &fixture.paths.socket);
        std::env::set_var("OVRCR_CONFIG", &registry);
    }
    let mut workers = Vec::new();
    for _ in 0..2 {
        let paths = fixture.paths.clone();
        workers.push(thread::spawn(move || connect_or_start(&paths)));
    }
    let mut streams = workers
        .into_iter()
        .map(|worker| worker.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(streams.len(), 2);
    let response = {
        let stream = streams.pop().unwrap();
        let mut stream = stream;
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 1,
                request: Request::Shutdown { kill: false },
            },
        )
        .unwrap();
        read_frame::<ServerMessage>(&mut stream).unwrap()
    };
    assert_eq!(
        response,
        ServerMessage::Response {
            request_id: 1,
            response: Response::Ok
        }
    );
    drop(streams);
    unsafe {
        std::env::remove_var("OVRCR_SERVER_EXECUTABLE");
        std::env::remove_var("OVRCR_SOCKET");
        std::env::remove_var("OVRCR_CONFIG");
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while fixture.paths.socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    assert!(!fixture.paths.socket.exists());
}

#[test]
fn startup_read_only_commands_do_not_start_a_missing_server() {
    let fixture = ServerFixture::new();
    assert!(connect_if_running(&fixture.paths).unwrap().is_none());
    assert!(!fixture.paths.socket.exists());
    let executable = env!("CARGO_BIN_EXE_ovrcr");
    let list = Command::new(executable)
        .arg("list")
        .env("OVRCR_SOCKET", &fixture.paths.socket)
        .env("OVRCR_CONFIG", fixture.root.path().join("config.toml"))
        .output()
        .unwrap();
    assert!(list.status.success());
    let shutdown = Command::new(executable)
        .arg("shutdown")
        .env("OVRCR_SOCKET", &fixture.paths.socket)
        .env("OVRCR_CONFIG", fixture.root.path().join("config.toml"))
        .status()
        .unwrap();
    assert!(shutdown.success());
    assert!(!fixture.paths.socket.exists());
}

#[test]
fn startup_stale_concurrent_attempts_leave_one_surviving_server() {
    let fixture = ServerFixture::new();
    let registry = fixture.root.path().join("config.toml");
    Registry::default().save_atomic(&registry).unwrap();
    std::fs::create_dir_all(fixture.paths.socket.parent().unwrap()).unwrap();
    let stale = UnixListener::bind(&fixture.paths.socket).unwrap();
    drop(stale);
    let executable = env!("CARGO_BIN_EXE_ovrcr");
    let mut workers: Vec<Child> = Vec::new();
    for _ in 0..2 {
        workers.push(
            Command::new(executable)
                .arg("server")
                .env("OVRCR_SOCKET", &fixture.paths.socket)
                .env("OVRCR_CONFIG", &registry)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    assert_ne!(workers[0].id(), workers[1].id());
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut exited = 0;
    while Instant::now() < deadline {
        exited = 0;
        for worker in &mut workers {
            if worker.try_wait().unwrap().is_some() {
                exited += 1;
            }
        }
        if exited == 1 {
            break;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    assert_eq!(
        exited, 1,
        "exactly one detached server owner must survive startup"
    );
    let mut first = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut first,
        &ClientMessage {
            request_id: 3,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut first).unwrap(),
        ServerMessage::Response {
            response: Response::Ok,
            ..
        }
    ));
    drop(first);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if workers
            .iter_mut()
            .all(|worker| worker.try_wait().unwrap().is_some())
        {
            break;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    for worker in &mut workers {
        if worker.try_wait().unwrap().is_none() {
            worker.kill().unwrap();
            worker.wait().unwrap();
        }
    }
    assert!(
        workers
            .iter_mut()
            .all(|worker| worker.try_wait().unwrap().is_some())
    );
    assert!(
        !fixture.paths.socket.exists(),
        "shutdown must stop the sole server"
    );
}

#[test]
fn shutdown_disconnected_requester_still_wakes_accept() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut stream = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 9,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    let deadline = Instant::now() + Duration::from_millis(250);
    while fixture.paths.socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    let completed_without_interference = !fixture.paths.socket.exists();
    if !completed_without_interference {
        let _ = UnixStream::connect(&fixture.paths.socket);
    }
    fixture.thread.take().unwrap().join().unwrap();
    assert!(
        completed_without_interference,
        "disconnected shutdown left accept blocked"
    );
}

#[test]
fn control_lifecycle_enforces_every_removal_gate() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let fixture = ControlFixture::new();
    assert!(matches!(
        fixture.request(Request::RemoveProject {
            name: "duplicate".into()
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    fixture.request(Request::AddProject {
        name: "fixture".into(),
        repo: fixture.repo.clone(),
        workspace_root: fixture.workspace_root.clone(),
    });
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/test".into(),
                base: "main".into()
            }
        }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    let review = fixture.create_session("review", vec!["sh".into(), "-c".into(), "exit 0".into()]);
    assert!(matches!(
        fixture.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(),
            workspace: "work".into(),
            name: "review".into(),
            label: None,
            argv: vec!["sh".into()]
        })),
        Response::Error {
            code: ErrorCode::AlreadyExists,
            ..
        }
    ));
    assert!(matches!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::KillSession { session: local }),
        Response::Ok
    );
    fixture.wait_exited(local);
    fixture.wait_exited(review);
    assert!(matches!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::RemoveSession { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveSession { session: review }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Ok
    );
    assert!(
        fixture
            .git_output(&["show-ref", "--verify", "refs/heads/feature/test"])
            .status
            .success()
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn workspace_shell_failure_retains_worktree_and_registry() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let fixture = ControlFixture::new();
    fixture.request(Request::AddProject {
        name: "fixture".into(),
        repo: fixture.repo.clone(),
        workspace_root: fixture.workspace_root.clone(),
    });
    let old_shell = std::env::var_os("SHELL");
    unsafe {
        std::env::set_var("SHELL", "/ovrcr/no-such-shell");
    }
    let response = fixture.request(Request::CreateWorkspace {
        project: "fixture".into(),
        name: "failed".into(),
        branch: BranchRequest::New {
            branch: "feature/failed".into(),
            base: "main".into(),
        },
    });
    match old_shell {
        Some(value) => unsafe { std::env::set_var("SHELL", value) },
        None => unsafe { std::env::remove_var("SHELL") },
    }
    assert!(
        matches!(response, Response::Error { code: ErrorCode::PartialFailure, message } if message.contains("failed") && message.contains("worktree"))
    );
    let listed = fixture.request(Request::List);
    assert!(
        matches!(listed, Response::Hierarchy(ref snapshot) if snapshot.projects[0].workspaces.iter().any(|workspace| workspace.name == "failed" && workspace.sessions.is_empty()))
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "failed".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn fast_exit_session_is_retained_as_exited() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/fast-exit".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let fast = fixture.create_session(
        "fast",
        vec!["sh".into(), "-c".into(), "printf retained".into()],
    );
    fixture.wait_exited(fast);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: fast }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::KillSession { session: local }),
        Response::Ok
    );
    fixture.wait_exited(local);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

struct ControlFixture {
    _root: tempfile::TempDir,
    repo: std::path::PathBuf,
    workspace_root: std::path::PathBuf,
    socket: std::path::PathBuf,
    thread: std::sync::Mutex<Option<thread::JoinHandle<()>>>,
}

impl ControlFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let workspace_root = root.path().join("workspaces");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&workspace_root).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "OVRCR Tests"],
            vec!["config", "user.email", "tests@example.invalid"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(repo.join("README"), "fixture\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "README"])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "initial"])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        let socket = root.path().join("server.sock");
        let registry = root.path().join("config.toml");
        ovrcr::config::Registry::default()
            .save_atomic(&registry)
            .unwrap();
        let paths = ServerPaths {
            socket: socket.clone(),
        };
        let thread = thread::spawn(move || run_server(paths, registry).unwrap());
        let fixture = Self {
            _root: root,
            repo: repo.canonicalize().unwrap(),
            workspace_root: workspace_root.canonicalize().unwrap(),
            socket,
            thread: std::sync::Mutex::new(Some(thread)),
        };
        fixture.wait_socket();
        fixture
    }
    fn wait_socket(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if UnixStream::connect(&self.socket).is_ok() {
                return;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        panic!("control server did not start");
    }
    fn request(&self, request: Request) -> Response {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 1,
                request,
            },
        )
        .unwrap();
        match read_frame::<ServerMessage>(&mut stream).unwrap() {
            ServerMessage::Response { response, .. } => response,
            ServerMessage::Event(_) => panic!("unexpected event"),
        }
    }
    fn create_session(&self, name: &str, argv: Vec<OsString>) -> SessionId {
        match self.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(),
            workspace: "work".into(),
            name: name.into(),
            label: None,
            argv,
        })) {
            Response::CreatedSession(summary) => summary.id,
            response => panic!("unexpected response: {response:?}"),
        }
    }
    fn only_session_id(&self) -> SessionId {
        match self.request(Request::List) {
            Response::Hierarchy(snapshot) => snapshot.projects[0].workspaces[0].sessions[0].id,
            response => panic!("unexpected response: {response:?}"),
        }
    }
    fn wait_exited(&self, id: SessionId) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Response::Hierarchy(snapshot) = self.request(Request::List)
                && snapshot
                    .projects
                    .iter()
                    .flat_map(|project| project.workspaces.iter())
                    .flat_map(|workspace| workspace.sessions.iter())
                    .any(|session| {
                        session.id == id && matches!(session.phase, SessionPhase::Exited { .. })
                    })
            {
                return;
            }
            thread::park_timeout(Duration::from_millis(10));
        }
        panic!("session {id:?} did not exit");
    }
    fn git_output(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .output()
            .unwrap()
    }
    fn join(&self) {
        self.thread.lock().unwrap().take().unwrap().join().unwrap();
    }
}

#[test]
fn dashboard_duplicate_hello_does_not_write_from_reader_thread() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut first = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut first,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut first).unwrap();
    let mut second = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut second,
        &ClientMessage {
            request_id: 2,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    second
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut second).unwrap(),
        ServerMessage::Response {
            response: Response::Error {
                code: ErrorCode::Conflict,
                ..
            },
            ..
        }
    ));
    first.shutdown(Shutdown::Both).unwrap();
    fixture.stop();
}

#[test]
fn duplicate_dashboard_hello_uses_the_sole_writer() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 20,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 20,
            response: Response::Hierarchy(_),
            ..
        }
    ));
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 21,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 21,
            response: Response::Error {
                code: ErrorCode::Conflict,
                ..
            },
            ..
        }
    ));
    drop(dashboard);
    fixture.stop();
}

#[test]
fn dashboard_request_id_zero_does_not_block_followup_response() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 0,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::List,
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let response = read_frame::<ServerMessage>(&mut dashboard);
    assert!(
        response.is_ok(),
        "request id zero must not block later dashboard responses: {response:?}"
    );
    drop(dashboard);
    fixture.stop();
}

#[test]
fn dashboard_shutdown_acknowledges_through_writer_before_teardown() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 10,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 11,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    assert_eq!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 11,
            response: Response::Ok,
        }
    );
    drop(dashboard);
    let deadline = Instant::now() + Duration::from_secs(2);
    while fixture.paths.socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    fixture.thread.take().unwrap().join().unwrap();
    assert!(!fixture.paths.socket.exists());
}

#[test]
fn cli_exit_preserves_session_and_shutdown_kill_cleans_up() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let workspaces = root.path().join("workspaces");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&workspaces).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "OVRCR Tests"]);
    git(&repo, &["config", "user.email", "tests@example.invalid"]);
    std::fs::write(repo.join("README"), "fixture\n").unwrap();
    git(&repo, &["add", "README"]);
    git(&repo, &["commit", "-m", "initial"]);

    let config = root.path().join("config.toml");
    let socket = root.path().join("server.sock");
    Registry::default().save_atomic(&config).unwrap();
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let mut cleanup = CliLifecycleGuard::new(bin, &config, &socket);
    let server = Command::new(bin)
        .arg("server")
        .env("OVRCR_CONFIG", &config)
        .env("OVRCR_SOCKET", &socket)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let server_pid = server.id();
    cleanup.server = Some(server);
    wait_for_socket(&socket);

    cli(
        bin,
        &config,
        &socket,
        &[
            "project",
            "add",
            "demo",
            repo.to_str().unwrap(),
            "--workspace-root",
            workspaces.to_str().unwrap(),
        ],
    );
    cli(
        bin,
        &config,
        &socket,
        &[
            "workspace",
            "create",
            "--project",
            "demo",
            "--name",
            "one",
            "--new-branch",
            "feature/one",
            "--base",
            "main",
        ],
    );
    let output = cli_with_output(
        bin,
        &config,
        &socket,
        &[
            "new",
            "--project",
            "demo",
            "--workspace",
            "one",
            "--name",
            "agent",
            "--",
            "sh",
            "-c",
            "printf '%s\\n' DETACHED_READY; while :; do sleep 1; done",
        ],
    );
    let agent: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap();
    let agent = SessionId(agent);

    let marker_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < marker_deadline {
        if dashboard_screen(&socket, agent).contains("DETACHED_READY") {
            break;
        }
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(dashboard_screen(&socket, agent).contains("DETACHED_READY"));
    assert!(dashboard_screen(&socket, agent).contains("DETACHED_READY"));
    let groups = session_process_groups(&socket);
    assert!(
        !groups.is_empty(),
        "live session process groups must be observable"
    );

    cli(bin, &config, &socket, &["shutdown", "--kill"]);
    let mut server = cleanup.server.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && server.try_wait().unwrap().is_none() {
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        server.try_wait().unwrap().is_some(),
        "server process did not exit"
    );
    assert!(!pid_exists(server_pid));
    assert!(!socket.exists());
    assert!(groups.iter().all(|pgid| !group_exists(*pgid)));
    cleanup.completed = true;
}

fn git(repo: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
    );
}

fn cli(bin: &str, config: &Path, socket: &Path, args: &[&str]) {
    let output = cli_with_output(bin, config, socket, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn cli_with_output(bin: &str, config: &Path, socket: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin)
        .args(args)
        .env("OVRCR_CONFIG", config)
        .env("OVRCR_SOCKET", socket)
        .output()
        .unwrap()
}

fn wait_for_socket(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(socket.exists(), "server socket did not appear");
}

fn dashboard_screen(socket: &Path, session: SessionId) -> String {
    let mut stream = UnixStream::connect(socket).unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut stream).unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 2,
            request: Request::Select {
                session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    loop {
        if let ServerMessage::Response {
            request_id: 2,
            response: Response::Screen { bytes, .. },
        } = read_frame::<ServerMessage>(&mut stream).unwrap()
        {
            let mut parser = vt100::Parser::new(24, 80, 0);
            parser.process(&bytes);
            return parser.screen().contents();
        }
    }
}

fn session_process_groups(socket: &Path) -> Vec<libc::pid_t> {
    let mut stream = UnixStream::connect(socket).unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 3,
            request: Request::List,
        },
    )
    .unwrap();
    let ServerMessage::Response {
        response: Response::Hierarchy(snapshot),
        ..
    } = read_frame::<ServerMessage>(&mut stream).unwrap()
    else {
        panic!("unexpected list response");
    };
    snapshot
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .filter_map(|session| session.pid)
        .map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) })
        .filter(|pgid| *pgid > 1)
        .collect()
}

fn pid_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn group_exists(pgid: libc::pid_t) -> bool {
    unsafe { libc::kill(-pgid, 0) == 0 }
}

struct CliLifecycleGuard {
    bin: String,
    config: PathBuf,
    socket: PathBuf,
    server: Option<Child>,
    completed: bool,
}

impl CliLifecycleGuard {
    fn new(bin: &str, config: &Path, socket: &Path) -> Self {
        Self {
            bin: bin.into(),
            config: config.into(),
            socket: socket.into(),
            server: None,
            completed: false,
        }
    }
}

impl Drop for CliLifecycleGuard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let _ = cli_with_output(
            &self.bin,
            &self.config,
            &self.socket,
            &["shutdown", "--kill"],
        );
        if let Some(server) = self.server.as_mut() {
            if server.try_wait().ok().flatten().is_none() {
                let _ = server.kill();
            }
            let _ = server.wait();
        }
    }
}
