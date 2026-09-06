use ovrcr::config::Registry;
use ovrcr::protocol::{
    BranchRequest, ClientMessage, CreateSessionRequest, ErrorCode, Request, Response,
    ServerMessage, read_frame, write_frame,
};
use ovrcr::server::{ServerPaths, connect_if_running, connect_or_start, run_server};
use ovrcr::session::{SessionId, SessionPhase};
use std::ffi::OsString;
use std::io::Write;
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
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
fn backpressured_input_and_send_do_not_block_inspect_or_kill() {
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
                branch: "feature/backpressure".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session(
        "blocked",
        vec![
            "sh".into(),
            "-c".into(),
            "stty raw -echo; printf READY; exec sleep 30".into(),
        ],
    );
    let mut dashboard = UnixStream::connect(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    let mut ready = false;
    dashboard
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let readiness_deadline = Instant::now() + Duration::from_secs(2);
    let mut request_id = 2;
    while !ready && Instant::now() < readiness_deadline {
        write_frame(
            &mut dashboard,
            &ClientMessage {
                request_id,
                request: Request::Select {
                    session,
                    size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
                },
            },
        )
        .unwrap();
        request_id += 1;
        let Ok(message) = read_frame::<ServerMessage>(&mut dashboard) else {
            continue;
        };
        match message {
            ServerMessage::Response {
                response: Response::Screen { bytes, .. },
                ..
            }
            | ServerMessage::Event(ovrcr::protocol::ServerEvent::Output { bytes, session: _ })
                if String::from_utf8_lossy(&bytes).contains("READY") =>
            {
                ready = true;
                break;
            }
            _ => {}
        }
    }
    assert!(ready, "blocked-session readiness marker was not rendered");
    dashboard
        .set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();
    while read_frame::<ServerMessage>(&mut dashboard).is_ok() {}
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 20,
            request: Request::Input {
                session,
                bytes: vec![b'x'; 512 * 1024],
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    assert!(
        read_frame::<ServerMessage>(&mut dashboard).is_err(),
        "input response unexpectedly completed while PTY stdin was backpressured"
    );

    let mut blocked_send = UnixStream::connect(&fixture.socket).unwrap();
    blocked_send
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    write_frame(
        &mut blocked_send,
        &ClientMessage {
            request_id: 20,
            request: Request::SendTerminal {
                session,
                text: "queued behind blocked input".into(),
                submit: false,
            },
        },
    )
    .unwrap();
    assert!(
        read_frame::<ServerMessage>(&mut blocked_send).is_err(),
        "SendTerminal unexpectedly completed while PTY stdin was backpressured"
    );

    let mut inspect = UnixStream::connect(&fixture.socket).unwrap();
    inspect
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut inspect,
        &ClientMessage {
            request_id: 21,
            request: Request::Inspect,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut inspect).unwrap(),
        ServerMessage::Response {
            request_id: 21,
            response: Response::Inventory { .. },
        }
    ));
    drop(inspect);

    let mut control = UnixStream::connect(&fixture.socket).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut control,
        &ClientMessage {
            request_id: 21,
            request: Request::List,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut control).unwrap(),
        ServerMessage::Response {
            request_id: 21,
            response: Response::Hierarchy(_),
        }
    ));
    drop(control);
    let mut control = UnixStream::connect(&fixture.socket).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut control,
        &ClientMessage {
            request_id: 22,
            request: Request::KillSession { session },
        },
    )
    .unwrap();
    assert_eq!(
        read_frame::<ServerMessage>(&mut control).unwrap(),
        ServerMessage::Response {
            request_id: 22,
            response: Response::Ok,
        }
    );
    drop(blocked_send);
    drop(dashboard);
    fixture.request(Request::Shutdown { kill: true });
    fixture.join();
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

#[test]
fn pause_resume_server_refuses_removal_and_late_mutation() {
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
                branch: "feature/pause-resume".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session("pause-resume", vec!["sh".into()]);
    let mut dashboard = UnixStream::connect(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 9,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();

    assert_eq!(
        fixture.request(Request::PauseSession { session }),
        Response::Ok
    );
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Event(ovrcr::protocol::ServerEvent::SessionChanged(summary))
                if summary.id == session && matches!(summary.phase, SessionPhase::Paused)
        ) {
            break;
        }
    }
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(ref snapshot)
            if snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|summary| summary.id == session && matches!(summary.phase, SessionPhase::Paused))
    ));
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 10,
            request: Request::Select {
                session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 10,
                response: Response::Screen { .. },
            }
        ) {
            break;
        }
    }
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 11,
            request: Request::Input {
                session,
                bytes: b"paused".to_vec(),
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 11,
                response: Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                },
            }
        ) {
            break;
        }
    }
    assert!(matches!(
        fixture.request(Request::RemoveSession { session }),
        Response::Error {
            code: ErrorCode::SessionRunning,
            ..
        }
    ));
    assert!(matches!(
        fixture.request(Request::SendTerminal {
            session,
            text: "paused".into(),
            submit: true,
        }),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::ResumeSession { session }),
        Response::Ok
    );
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Event(ovrcr::protocol::ServerEvent::SessionChanged(summary))
                if summary.id == session && matches!(summary.phase, SessionPhase::Running)
        ) {
            break;
        }
    }
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(ref snapshot)
            if snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|summary| summary.id == session && matches!(summary.phase, SessionPhase::Running))
    ));

    assert_eq!(
        fixture.request(Request::KillSession { session }),
        Response::Ok
    );
    fixture.wait_exited(session);
    assert_eq!(
        fixture.request(Request::RemoveSession { session }),
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
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    drop(dashboard);
    fixture.join();
}

struct PausePeer {
    pid: libc::pid_t,
    pgid: libc::pid_t,
    address: PathBuf,
}

struct PauseHarness {
    fixture: ControlFixture,
    dir: PathBuf,
    control_path: PathBuf,
    control: UnixDatagram,
    pgids: Vec<libc::pid_t>,
    next_endpoint: usize,
}

impl PauseHarness {
    fn new() -> Self {
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
                    branch: "feature/task4-pause-resume".into(),
                    base: "main".into(),
                },
            }),
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

        let dir = fixture._root.path().join("pause-resume");
        std::fs::create_dir(&dir).unwrap();
        let control_path = dir.join("parent.sock");
        let control = UnixDatagram::bind(&control_path).unwrap();
        control
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        Self {
            fixture,
            dir,
            control_path,
            control,
            pgids: Vec::new(),
            next_endpoint: 0,
        }
    }

    fn create_session(&mut self, name: &str) -> (SessionId, PausePeer, PausePeer) {
        self.create_session_with_options(name, false)
    }

    fn create_session_with_options(
        &mut self,
        name: &str,
        ignore_sighup: bool,
    ) -> (SessionId, PausePeer, PausePeer) {
        let endpoint_dir = self.dir.join(format!("session-{}", self.next_endpoint));
        self.next_endpoint += 1;
        std::fs::create_dir(&endpoint_dir).unwrap();
        let summary = self.fixture.create_session_summary(
            name,
            pause_session_argv(&endpoint_dir, &self.control_path, ignore_sighup),
        );
        self.record_created_pgid(&summary);
        let session = summary.id;
        let first = recv_pause_ready(&self.control);
        let second = recv_pause_ready(&self.control);
        assert_eq!(
            first.pgid, second.pgid,
            "leader and descendant lost PGID ownership"
        );
        assert_eq!(
            unsafe { libc::getpgid(first.pid) },
            first.pgid,
            "leader READY reported a stale PGID"
        );
        assert_eq!(
            unsafe { libc::getpgid(second.pid) },
            second.pgid,
            "descendant READY reported a stale PGID"
        );
        let (leader, descendant) = if first
            .address
            .file_name()
            .is_some_and(|name| name == "leader.sock")
        {
            (first, second)
        } else {
            (second, first)
        };
        (session, leader, descendant)
    }

    fn record_created_pgid(&mut self, summary: &ovrcr::session::SessionSummary) {
        let pid = summary
            .pid
            .expect("successful session creation must report a process ID");
        // Session::spawn requires the PTY leader to own its process group, so
        // the creation response gives us the group ID before any handshake
        // assertions can fail.
        self.pgids.push(pid as libc::pid_t);
    }

    fn finish(&self) {
        if self.fixture.thread.lock().unwrap().is_some() {
            assert_eq!(
                request_with_timeout(
                    &self.fixture.socket,
                    901,
                    Request::Shutdown { kill: true },
                    Duration::from_secs(5),
                ),
                Some(Response::Ok)
            );
            assert!(
                self.join_bounded(Duration::from_secs(2)),
                "control server did not finish within cleanup deadline"
            );
        }
    }

    fn join_bounded(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let finished = self
                .fixture
                .thread
                .lock()
                .unwrap()
                .as_ref()
                .map(|handle| handle.is_finished())
                .unwrap_or(true);
            if finished {
                let handle = self.fixture.thread.lock().unwrap().take();
                return handle.map(|handle| handle.join().is_ok()).unwrap_or(true);
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
    }
}

impl Drop for PauseHarness {
    fn drop(&mut self) {
        let mut cleanup_failed = false;
        if self.fixture.thread.lock().unwrap().is_some() {
            if request_with_timeout(
                &self.fixture.socket,
                999,
                Request::Shutdown { kill: true },
                Duration::from_secs(2),
            ) != Some(Response::Ok)
            {
                cleanup_failed = true;
            }
            for pgid in &self.pgids {
                unsafe {
                    libc::kill(-*pgid, libc::SIGKILL);
                }
            }
            if !self.join_bounded(Duration::from_secs(2)) {
                cleanup_failed = true;
            }
        }
        for pgid in &self.pgids {
            if !wait_group_absent(*pgid, Duration::from_secs(2)) {
                cleanup_failed = true;
            }
        }
        if cleanup_failed {
            let kept =
                std::mem::replace(&mut self.fixture._root, tempfile::tempdir().unwrap()).keep();
            eprintln!(
                "pause/resume fixture cleanup failed; preserved {}",
                kept.display().to_string()
            );
        }
    }
}

fn pause_session_argv(dir: &Path, parent: &Path, ignore_sighup: bool) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("env"),
        OsString::from("OVRCR_PAUSE_ROLE=leader"),
        OsString::from(format!("OVRCR_PAUSE_DIR={}", dir.display())),
        OsString::from(format!("OVRCR_PAUSE_PARENT={}", parent.display())),
    ];
    if ignore_sighup {
        argv.push(OsString::from("OVRCR_PAUSE_IGNORE_SIGHUP=1"));
    }
    argv.extend([
        std::env::current_exe().unwrap().into_os_string(),
        "--ignored".into(),
        "--exact".into(),
        "pause_resume_child_fixture".into(),
        "--nocapture".into(),
    ]);
    argv
}

fn request_with_timeout(
    socket: &Path,
    request_id: u64,
    request: Request,
    timeout: Duration,
) -> Option<Response> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id,
            request,
        },
    )
    .ok()?;
    match read_frame::<ServerMessage>(&mut stream).ok()? {
        ServerMessage::Response { response, .. } => Some(response),
        ServerMessage::Event(_) => None,
    }
}

fn dashboard_for_session(socket: &Path, session: SessionId) -> (UnixStream, Vec<u8>) {
    let mut dashboard = UnixStream::connect(socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    let screen = dashboard_select(&mut dashboard, 2, session);
    (dashboard, screen)
}

fn dashboard_select(stream: &mut UnixStream, request_id: u64, session: SessionId) -> Vec<u8> {
    write_frame(
        stream,
        &ClientMessage {
            request_id,
            request: Request::Select {
                session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    loop {
        match read_frame::<ServerMessage>(stream).unwrap() {
            ServerMessage::Response {
                request_id: id,
                response: Response::Screen { bytes, .. },
            } if id == request_id => return bytes,
            _ => {}
        }
    }
}

fn dashboard_request(stream: &mut UnixStream, request_id: u64, request: Request) -> Response {
    write_frame(
        stream,
        &ClientMessage {
            request_id,
            request,
        },
    )
    .unwrap();
    loop {
        match read_frame::<ServerMessage>(stream).unwrap() {
            ServerMessage::Response {
                request_id: id,
                response,
            } if id == request_id => return response,
            _ => {}
        }
    }
}

fn recv_pause_ready(socket: &UnixDatagram) -> PausePeer {
    let mut bytes = [0_u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(5);
    let size = loop {
        match socket.recv(&mut bytes) {
            Ok(size) => break size,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && Instant::now() < deadline =>
            {
                thread::yield_now();
            }
            Err(error) => panic!("read helper READY datagram: {error}"),
        }
    };
    let message = std::str::from_utf8(&bytes[..size]).unwrap();
    let mut fields = message.splitn(5, ':');
    assert_eq!(
        fields.next(),
        Some("READY"),
        "unexpected helper datagram: {message}"
    );
    let _role = fields.next().unwrap();
    let pid = fields.next().unwrap().parse().unwrap();
    let pgid = fields.next().unwrap().parse().unwrap();
    let address = PathBuf::from(fields.next().unwrap());
    PausePeer { pid, pgid, address }
}

fn wait_peer_stopped(peer: &PausePeer, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        assert_eq!(
            unsafe { libc::getpgid(peer.pid) },
            peer.pgid,
            "peer moved out of its owned process group"
        );
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &peer.pid.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        if output.status.success() && state.trim_start().starts_with('T') {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "peer {} did not stop: {state:?}",
            peer.pid
        );
        thread::yield_now();
    }
}

fn send_peer_command(socket: &UnixDatagram, peer: &PausePeer, token: &str) {
    socket.send_to(token.as_bytes(), &peer.address).unwrap();
}

fn expect_peer_reply(socket: &UnixDatagram, token: &str) {
    assert_eq!(recv_pause_datagram(socket), token.as_bytes());
}

fn expect_peer_replies(socket: &UnixDatagram, tokens: &[&str]) {
    let mut remaining = tokens.to_vec();
    while !remaining.is_empty() {
        let bytes = recv_pause_datagram(socket);
        let Some(index) = remaining
            .iter()
            .position(|token| bytes.as_slice() == token.as_bytes())
        else {
            panic!(
                "unexpected helper reply: {:?}",
                String::from_utf8_lossy(&bytes)
            );
        };
        remaining.swap_remove(index);
    }
}

fn recv_pause_datagram(socket: &UnixDatagram) -> Vec<u8> {
    let mut bytes = [0_u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match socket.recv(&mut bytes) {
            Ok(size) => return bytes[..size].to_vec(),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                thread::yield_now()
            }
            Err(error) => panic!("read helper datagram: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for helper datagram"
        );
    }
}

fn expect_datagram_prefix(socket: &UnixDatagram, prefix: &str) -> Vec<u8> {
    let bytes = recv_pause_datagram(socket);
    assert!(
        bytes.starts_with(prefix.as_bytes()),
        "expected datagram prefix {prefix:?}, got {:?}",
        String::from_utf8_lossy(&bytes)
    );
    bytes
}

fn expect_pty_input(socket: &UnixDatagram, token: &str) {
    let bytes = expect_datagram_prefix(socket, "PTY:");
    assert_eq!(&bytes[4..], token.as_bytes());
}

fn expect_term_acks_and_descendant_final(
    socket: &UnixDatagram,
    peers: &[&PausePeer],
    descendants: &[&PausePeer],
) -> bool {
    let mut seen = Vec::new();
    let mut final_seen = Vec::new();
    let mut drain_ok = true;
    let mut drain_seen = Vec::new();
    while seen.len() < peers.len()
        || final_seen.len() < descendants.len()
        || drain_seen.len() < descendants.len()
    {
        let bytes = recv_pause_datagram(socket);
        if bytes.starts_with(b"TERM_ACK:") {
            let pid = std::str::from_utf8(&bytes[9..])
                .unwrap()
                .parse::<libc::pid_t>()
                .unwrap();
            assert!(
                peers.iter().any(|peer| peer.pid == pid),
                "unknown TERM_ACK pid {pid}"
            );
            assert!(!seen.contains(&pid), "duplicate TERM_ACK pid {pid}");
            seen.push(pid);
        } else if bytes.starts_with(b"FINAL_DESCENDANT_AFTER_TERM:") {
            let pid = std::str::from_utf8(&bytes[b"FINAL_DESCENDANT_AFTER_TERM:".len()..])
                .unwrap()
                .parse::<libc::pid_t>()
                .unwrap();
            assert!(
                descendants.iter().any(|peer| peer.pid == pid),
                "final marker came from an unexpected peer {pid}"
            );
            assert!(
                !final_seen.contains(&pid),
                "duplicate descendant final marker"
            );
            final_seen.push(pid);
        } else if bytes.starts_with(b"DESCENDANT_STDOUT_RESULT:") {
            let prefix = b"DESCENDANT_STDOUT_RESULT:";
            let evidence = std::str::from_utf8(&bytes[prefix.len()..]).unwrap();
            let (pid, result) = evidence.split_once(":drain=").unwrap();
            let pid = pid.parse::<libc::pid_t>().unwrap();
            assert!(
                descendants.iter().any(|peer| peer.pid == pid),
                "drainage evidence came from an unexpected peer {pid}"
            );
            assert!(
                !drain_seen.contains(&pid),
                "duplicate descendant drainage evidence"
            );
            drain_seen.push(pid);
            drain_ok &= result == "ok";
        } else {
            panic!(
                "unexpected termination evidence: {:?}",
                String::from_utf8_lossy(&bytes)
            );
        }
    }
    drain_ok
}

fn wait_pid_absent(pid: libc::pid_t, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let result = unsafe { libc::kill(pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return;
        }
        assert!(Instant::now() < deadline, "PID {pid} did not disappear");
        thread::yield_now();
    }
}

fn wait_group_absent(pgid: libc::pid_t, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let result = unsafe { libc::kill(-pgid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::yield_now();
    }
}

fn session_summary(fixture: &ControlFixture, session: SessionId) -> ovrcr::session::SessionSummary {
    match fixture.request(Request::List) {
        Response::Hierarchy(snapshot) => snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .find(|summary| summary.id == session)
            .unwrap(),
        response => panic!("unexpected list response: {response:?}"),
    }
}

fn wait_exited_and_assert_terminal_contains(
    fixture: &ControlFixture,
    session: SessionId,
    markers: &[&str],
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let response = request_with_timeout(
            &fixture.socket,
            401,
            Request::List,
            Duration::from_millis(250),
        );
        if let Some(Response::Hierarchy(snapshot)) = response {
            let exited = snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|summary| {
                    summary.id == session && matches!(summary.phase, SessionPhase::Exited { .. })
                });
            if exited {
                let terminal = request_with_timeout(
                    &fixture.socket,
                    402,
                    Request::ReadTerminal {
                        session,
                        max_lines: None,
                    },
                    Duration::from_secs(1),
                )
                .unwrap_or_else(|| panic!("terminal read failed at first Exited observation"));
                let Response::TerminalText { text, .. } = terminal else {
                    panic!(
                        "unexpected terminal response at first Exited observation: {terminal:?}"
                    );
                };
                for marker in markers {
                    assert!(
                        text.contains(marker),
                        "terminal omitted final marker {marker:?} at first Exited observation: {text:?}"
                    );
                }
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "session {session:?} did not exit before terminal assertion deadline"
        );
        thread::yield_now();
    }
}

#[test]
fn pause_resume_stops_group_and_rejects_input() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let mut harness = PauseHarness::new();
    let (session, leader, descendant) = harness.create_session("pause-input");
    let (mut dashboard, before_screen) = dashboard_for_session(&harness.fixture.socket, session);
    let before = session_summary(&harness.fixture, session);

    assert_eq!(
        harness.fixture.request(Request::PauseSession { session }),
        Response::Ok
    );
    wait_peer_stopped(&leader, Duration::from_secs(2));
    wait_peer_stopped(&descendant, Duration::from_secs(2));
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            3,
            Request::Input {
                session,
                bytes: b"REJECTED_WHILE_PAUSED".to_vec(),
            },
        ),
        Response::Error {
            code: ErrorCode::Conflict,
            message: "session is paused; resume it before sending input".into(),
        }
    );
    assert!(matches!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Error {
            code: ErrorCode::SessionRunning,
            ..
        }
    ));
    assert!(matches!(
        harness.fixture.request(Request::Shutdown { kill: false }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));

    send_peer_command(&harness.control, &leader, "RESUME_LEADER_1");
    send_peer_command(&harness.control, &descendant, "RESUME_DESCENDANT_1");
    assert_eq!(
        harness.fixture.request(Request::ResumeSession { session }),
        Response::Ok
    );
    expect_peer_replies(
        &harness.control,
        &["RESUME_LEADER_1", "RESUME_DESCENDANT_1"],
    );
    assert_eq!(dashboard_select(&mut dashboard, 4, session), before_screen);
    let after = session_summary(&harness.fixture, session);
    assert_eq!(after.id, before.id);
    assert_eq!(after.pid, before.pid);
    assert!(matches!(after.phase, SessionPhase::Running));
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            5,
            Request::Input {
                session,
                bytes: b"PTY_AFTER_RESUME_1".to_vec(),
            },
        ),
        Response::Ok
    );
    expect_pty_input(&harness.control, "PTY_AFTER_RESUME_1");

    assert_eq!(
        harness.fixture.request(Request::KillSession { session }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&leader, &descendant],
        &[&descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        session,
        &["FINAL_AFTER_TERM", "FINAL_DESCENDANT_AFTER_TERM"],
    );
    assert_eq!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Ok
    );
    drop(dashboard);
    harness.finish();
}

#[test]
fn pause_resume_kill_runs_group_handlers() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let mut harness = PauseHarness::new();
    let (session, leader, descendant) = harness.create_session("kill-paused");
    let (mut dashboard, _) = dashboard_for_session(&harness.fixture.socket, session);
    assert_eq!(
        harness.fixture.request(Request::PauseSession { session }),
        Response::Ok
    );
    wait_peer_stopped(&leader, Duration::from_secs(2));
    wait_peer_stopped(&descendant, Duration::from_secs(2));
    assert_eq!(
        harness.fixture.request(Request::KillSession { session }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&leader, &descendant],
        &[&descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        session,
        &["FINAL_AFTER_TERM", "FINAL_DESCENDANT_AFTER_TERM"],
    );
    assert!(wait_group_absent(leader.pgid, Duration::from_secs(2)));
    assert_eq!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Ok
    );

    let (external, external_leader, external_descendant) =
        harness.create_session("kill-external-stop");
    let _ = dashboard_select(&mut dashboard, 3, external);
    unsafe {
        assert_eq!(libc::kill(-external_leader.pgid, libc::SIGSTOP), 0);
    }
    wait_peer_stopped(&external_leader, Duration::from_secs(2));
    wait_peer_stopped(&external_descendant, Duration::from_secs(2));
    assert!(matches!(
        session_summary(&harness.fixture, external).phase,
        SessionPhase::Running
    ));
    assert_eq!(
        harness
            .fixture
            .request(Request::KillSession { session: external }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&external_leader, &external_descendant],
        &[&external_descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        external,
        &["FINAL_AFTER_TERM", "FINAL_DESCENDANT_AFTER_TERM"],
    );
    assert!(wait_group_absent(
        external_leader.pgid,
        Duration::from_secs(2)
    ));
    assert_eq!(
        harness
            .fixture
            .request(Request::RemoveSession { session: external }),
        Response::Ok
    );
    drop(dashboard);
    harness.finish();
}

#[test]
fn pause_resume_shutdown_cleans_stopped_groups() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let mut harness = PauseHarness::new();
    let (first, first_leader, first_descendant) = harness.create_session("shutdown-one");
    let (second, second_leader, second_descendant) = harness.create_session("shutdown-two");
    for session in [first, second] {
        assert_eq!(
            harness.fixture.request(Request::PauseSession { session }),
            Response::Ok
        );
    }
    for peer in [
        &first_leader,
        &first_descendant,
        &second_leader,
        &second_descendant,
    ] {
        wait_peer_stopped(peer, Duration::from_secs(2));
    }
    assert_eq!(
        harness.fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[
            &first_leader,
            &first_descendant,
            &second_leader,
            &second_descendant,
        ],
        &[&first_descendant, &second_descendant],
    ));
    assert!(wait_group_absent(first_leader.pgid, Duration::from_secs(2)));
    assert!(wait_group_absent(
        second_leader.pgid,
        Duration::from_secs(2)
    ));
    assert!(
        harness.join_bounded(Duration::from_secs(2)),
        "control server did not finish after shutdown"
    );
    assert!(!harness.fixture.socket.exists());
}

#[test]
fn pause_resume_control_races_converge() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    pause_resume_control_races_body();
}

fn pause_resume_control_races_body() {
    let mut harness = PauseHarness::new();
    let (session, leader, descendant) = harness.create_session("control-race");
    let barrier = Arc::new(Barrier::new(4));
    let operations = [
        Request::PauseSession { session },
        Request::ResumeSession { session },
        Request::KillSession { session },
    ];
    let workers = operations
        .into_iter()
        .map(|request| {
            let barrier = Arc::clone(&barrier);
            let socket = harness.fixture.socket.clone();
            thread::spawn(move || {
                let stream = UnixStream::connect(socket).unwrap();
                let mut stream = stream;
                barrier.wait();
                request_on_stream(&mut stream, 1, request, Duration::from_secs(4))
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let responses = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(responses.iter().all(|response| {
        matches!(
            response,
            Response::Ok
                | Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                }
        )
    }));
    if !responses.iter().any(|response| response == &Response::Ok) {
        assert_eq!(
            harness.fixture.request(Request::KillSession { session }),
            Response::Ok
        );
    }
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&leader, &descendant],
        &[&descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        session,
        &["FINAL_AFTER_TERM", "FINAL_DESCENDANT_AFTER_TERM"],
    );
    assert!(wait_group_absent(leader.pgid, Duration::from_secs(2)));
    assert_eq!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Ok
    );

    let (reaped, reaped_leader, reaped_descendant) =
        harness.create_session_with_options("reaped-leader", true);
    send_peer_command(&harness.control, &reaped_leader, "EXIT_LEADER");
    expect_peer_reply(&harness.control, "EXIT_LEADER_ACK");
    wait_pid_absent(reaped_leader.pid, Duration::from_secs(3));
    assert_eq!(
        unsafe { libc::getpgid(reaped_descendant.pid) },
        reaped_descendant.pgid
    );
    assert_eq!(
        harness
            .fixture
            .request(Request::PauseSession { session: reaped }),
        Response::Ok
    );
    wait_peer_stopped(&reaped_descendant, Duration::from_secs(2));
    send_peer_command(&harness.control, &reaped_descendant, "RESUME_SURVIVOR_1");
    assert_eq!(
        harness
            .fixture
            .request(Request::ResumeSession { session: reaped }),
        Response::Ok
    );
    expect_peer_reply(&harness.control, "RESUME_SURVIVOR_1");
    assert_eq!(
        harness
            .fixture
            .request(Request::KillSession { session: reaped }),
        Response::Ok
    );
    let reaped_drain_ok = expect_term_acks_and_descendant_final(
        &harness.control,
        &[&reaped_descendant],
        &[&reaped_descendant],
    );
    if cfg!(target_os = "macos") {
        assert!(
            !reaped_drain_ok,
            "macOS reaped survivor unexpectedly wrote to its detached PTY"
        );
    } else {
        assert!(
            reaped_drain_ok,
            "reaped survivor did not drain final PTY bytes"
        );
    }
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        reaped,
        if cfg!(target_os = "macos") {
            &[]
        } else {
            &["FINAL_DESCENDANT_AFTER_TERM"]
        },
    );
    assert!(wait_group_absent(
        reaped_descendant.pgid,
        Duration::from_secs(2)
    ));
    assert_eq!(
        harness
            .fixture
            .request(Request::RemoveSession { session: reaped }),
        Response::Ok
    );
    harness.finish();
}

fn request_on_stream(
    stream: &mut UnixStream,
    request_id: u64,
    request: Request,
    timeout: Duration,
) -> Response {
    stream.set_read_timeout(Some(timeout)).unwrap();
    write_frame(
        stream,
        &ClientMessage {
            request_id,
            request,
        },
    )
    .unwrap();
    match read_frame::<ServerMessage>(stream).unwrap() {
        ServerMessage::Response { response, .. } => response,
        ServerMessage::Event(event) => panic!("unexpected race event: {event:?}"),
    }
}

#[test]
fn pause_resume_backpressured_input_keeps_controls_available() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let mut harness = PauseHarness::new();
    let summary = harness.fixture.create_session_summary(
        "blocked-pause",
        vec![
            "sh".into(),
            "-c".into(),
            "stty raw -echo; printf READY; exec sleep 30".into(),
        ],
    );
    harness.record_created_pgid(&summary);
    let session = summary.id;
    let mut dashboard = UnixStream::connect(&harness.fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    let _ = dashboard_select(&mut dashboard, 2, session);
    dashboard
        .set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();
    while read_frame::<ServerMessage>(&mut dashboard).is_ok() {}
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 20,
            request: Request::Input {
                session,
                bytes: vec![b'x'; 512 * 1024],
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    assert!(read_frame::<ServerMessage>(&mut dashboard).is_err());

    let mut blocked_send = UnixStream::connect(&harness.fixture.socket).unwrap();
    blocked_send
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    write_frame(
        &mut blocked_send,
        &ClientMessage {
            request_id: 21,
            request: Request::SendTerminal {
                session,
                text: "queued behind blocked input".into(),
                submit: false,
            },
        },
    )
    .unwrap();
    assert!(read_frame::<ServerMessage>(&mut blocked_send).is_err());

    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            22,
            Request::PauseSession { session },
            Duration::from_secs(3),
        ),
        Some(Response::Ok)
    );
    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            23,
            Request::List,
            Duration::from_secs(3),
        )
        .map(|response| matches!(response, Response::Hierarchy(_))),
        Some(true)
    );
    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            24,
            Request::ResumeSession { session },
            Duration::from_secs(3),
        ),
        Some(Response::Ok)
    );
    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            25,
            Request::KillSession { session },
            Duration::from_secs(5),
        ),
        Some(Response::Ok)
    );
    harness.fixture.wait_exited(session);
    drop(blocked_send);
    drop(dashboard);
    harness.finish();
}

#[test]
#[ignore]
fn pause_resume_child_fixture() {
    let role = std::env::var("OVRCR_PAUSE_ROLE").unwrap();
    let dir = PathBuf::from(std::env::var("OVRCR_PAUSE_DIR").unwrap());
    let parent = std::env::var_os("OVRCR_PAUSE_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("parent.sock"));
    let address = dir.join(format!("{role}.sock"));
    let _ = std::fs::remove_file(&address);
    let socket = UnixDatagram::bind(&address).unwrap();
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(libc::SIGTERM, Arc::clone(&term)).unwrap();
    let ignore_sighup = std::env::var("OVRCR_PAUSE_IGNORE_SIGHUP")
        .map(|value| value == "1")
        .unwrap_or(false);
    if role == "descendant" && ignore_sighup {
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
    }
    let mut child = if role == "leader" {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "pause_resume_child_fixture",
                "--nocapture",
            ])
            .env("OVRCR_PAUSE_ROLE", "descendant")
            .env("OVRCR_PAUSE_DIR", &dir)
            .env("OVRCR_PAUSE_PARENT", &parent);
        if ignore_sighup {
            command.env("OVRCR_PAUSE_IGNORE_SIGHUP", "1");
        }
        Some(command.spawn().unwrap())
    } else {
        None
    };
    let pid = unsafe { libc::getpid() };
    let pgid = unsafe { libc::getpgid(pid) };
    let original_termios = set_pause_raw_terminal();
    let stdin_fd = libc::STDIN_FILENO;
    socket
        .send_to(
            format!("READY:{role}:{pid}:{pgid}:{}", address.display()).as_bytes(),
            &parent,
        )
        .unwrap();
    let mut stdin_bytes = [0_u8; 4096];
    let mut datagram_bytes = [0_u8; 4096];
    loop {
        if term.load(Ordering::Acquire) {
            socket
                .send_to(format!("TERM_ACK:{pid}").as_bytes(), &parent)
                .unwrap();
            if let Some(child) = child.as_mut() {
                let _ = child.wait();
                std::io::stdout().write_all(b"FINAL_AFTER_TERM\n").unwrap();
            } else {
                let write_result = std::io::stdout().write_all(b"FINAL_DESCENDANT_AFTER_TERM\n");
                let flush_result = std::io::stdout().flush();
                socket
                    .send_to(
                        format!(
                            "DESCENDANT_STDOUT_RESULT:{pid}:drain={}",
                            if write_result.is_ok() && flush_result.is_ok() {
                                "ok"
                            } else {
                                "error"
                            },
                        )
                        .as_bytes(),
                        &parent,
                    )
                    .unwrap();
            }
            if role == "descendant" {
                socket
                    .send_to(
                        format!("FINAL_DESCENDANT_AFTER_TERM:{pid}").as_bytes(),
                        &parent,
                    )
                    .unwrap();
            }
            std::io::stdout().flush().unwrap();
            restore_pause_terminal(original_termios);
            let _ = std::fs::remove_file(&address);
            return;
        }
        let mut fds = [
            libc::pollfd {
                fd: socket.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if role == "leader" { stdin_fd } else { -1 },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 50) };
        if result == -1 {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR)
            );
            continue;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            let (size, sender) = socket.recv_from(&mut datagram_bytes).unwrap();
            if &datagram_bytes[..size] == b"EXIT_LEADER" && role == "leader" {
                socket
                    .send_to(b"EXIT_LEADER_ACK", sender.as_pathname().unwrap())
                    .unwrap();
                restore_pause_terminal(original_termios);
                let _ = std::fs::remove_file(&address);
                return;
            }
            socket
                .send_to(&datagram_bytes[..size], sender.as_pathname().unwrap())
                .unwrap();
        }
        if role == "leader" && fds[1].revents & libc::POLLIN != 0 {
            let size =
                unsafe { libc::read(stdin_fd, stdin_bytes.as_mut_ptr().cast(), stdin_bytes.len()) };
            if size > 0 {
                let mut message = b"PTY:".to_vec();
                message.extend_from_slice(&stdin_bytes[..size as usize]);
                socket.send_to(&message, &parent).unwrap();
            }
        }
    }
}

fn set_pause_raw_terminal() -> Option<libc::termios> {
    let fd = libc::STDIN_FILENO;
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
        return None;
    }
    let original = unsafe { original.assume_init() };
    let mut raw = original;
    raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ECHONL);
    raw.c_cc[libc::VMIN] = 0;
    raw.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
        return None;
    }
    Some(original)
}

fn restore_pause_terminal(original: Option<libc::termios>) {
    if let Some(original) = original {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
        }
    }
}

#[test]
fn slow_dashboard_recovers_after_output_burst() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    assert_eq!(ovrcr::server::RAW_EVENT_QUEUE_CAPACITY, 64);
    assert_eq!(ovrcr::server::RAW_DISPATCH_QUEUE_CAPACITY, 64);
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
                branch: "feature/slow-dashboard".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let burst = fixture.create_session(
        "burst",
        vec![
            "sh".into(),
            "-c".into(),
            "i=0; while [ $i -lt 200000 ]; do printf 'BURST_%06d\\n' \"$i\"; i=$((i+1)); done; printf FINAL_MARKER".into(),
        ],
    );

    let mut dashboard = UnixStream::connect(&fixture.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 2,
            request: Request::Select {
                session: burst,
                size: ovrcr::session::TerminalSize {
                    rows: 40,
                    cols: 120,
                },
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(
            fixture.request(Request::List),
            Response::Hierarchy(ref snapshot)
                if snapshot
                    .projects
                    .iter()
                    .flat_map(|project| project.workspaces.iter())
                    .flat_map(|workspace| workspace.sessions.iter())
                    .any(|session| session.id == burst && matches!(session.phase, SessionPhase::Exited { .. }))
        ) {
            break;
        }
        assert!(Instant::now() < deadline, "burst process did not finish");
        thread::park_timeout(Duration::from_millis(5));
    }

    let mut saw_dirty = false;
    while !saw_dirty {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ovrcr::protocol::ServerEvent::ScreenDirty { session })
                if session == burst =>
            {
                saw_dirty = true
            }
            _ => {}
        }
    }
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 3,
            request: Request::Select {
                session: burst,
                size: ovrcr::session::TerminalSize {
                    rows: 40,
                    cols: 120,
                },
            },
        },
    )
    .unwrap();
    let snapshot = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    assert!(matches!(
        snapshot,
        ServerMessage::Response {
            response: Response::Screen { bytes, .. },
            ..
        } if String::from_utf8_lossy(&bytes).contains("FINAL_MARKER")
    ));
    let mut dirty_count = 1;
    dashboard
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let quiet_deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < quiet_deadline {
        match read_frame::<ServerMessage>(&mut dashboard) {
            Ok(ServerMessage::Event(ovrcr::protocol::ServerEvent::ScreenDirty { session }))
                if session == burst =>
            {
                dirty_count += 1
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.root_cause().downcast_ref::<std::io::Error>(),
                    Some(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        )
                ) =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    assert_eq!(
        dirty_count, 1,
        "quiet burst emitted more than one ScreenDirty"
    );
    drop(dashboard);
    fixture.request(Request::RemoveSession { session: burst });
    let local = fixture.only_session_id();
    fixture.request(Request::KillSession { session: local });
    fixture.wait_exited(local);
    fixture.request(Request::RemoveSession { session: local });
    fixture.request(Request::RemoveWorkspace {
        project: "fixture".into(),
        name: "work".into(),
    });
    fixture.request(Request::RemoveProject {
        name: "fixture".into(),
    });
    fixture.request(Request::Shutdown { kill: false });
    fixture.join();
}

#[test]
fn concurrent_terminal_sends_are_serialized_as_complete_pastes() {
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
                branch: "feature/concurrent-send".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session(
        "concurrent",
        vec![
            "sh".into(),
            "-c".into(),
            r#"stty raw -echo; printf '\033[?2004hREADY\r\n'; dd bs=1 count=34 2>/dev/null | od -An -tx1; printf '\r\nTAIL\r\n'"#.into(),
        ],
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match fixture.request(Request::ReadTerminal {
            session,
            max_lines: None,
        }) {
            Response::TerminalText { text, .. } if text.contains("READY") => break,
            _ if Instant::now() < deadline => thread::park_timeout(Duration::from_millis(10)),
            response => panic!("concurrent terminal was not ready: {response:?}"),
        }
    }

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers = ["AA\nA", "BB\nB"].map(|text| {
        let socket = fixture.socket.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        thread::spawn(move || {
            let mut stream = UnixStream::connect(socket).unwrap();
            barrier.wait();
            write_frame(
                &mut stream,
                &ClientMessage {
                    request_id: 1,
                    request: Request::SendTerminal {
                        session,
                        text: text.into(),
                        submit: true,
                    },
                },
            )
            .unwrap();
            match read_frame::<ServerMessage>(&mut stream).unwrap() {
                ServerMessage::Response {
                    response: Response::Ok,
                    ..
                } => {}
                response => panic!("unexpected send response: {response:?}"),
            }
        })
    });
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }

    fixture.wait_exited(session);
    let text = match fixture.request(Request::ReadTerminal {
        session,
        max_lines: None,
    }) {
        Response::TerminalText { text, .. } => text,
        response => panic!("unexpected terminal read: {response:?}"),
    };
    let hex = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let a = "1b 5b 32 30 30 7e 41 41 0a 41 1b 5b 32 30 31 7e 0d";
    let b = "1b 5b 32 30 30 7e 42 42 0a 42 1b 5b 32 30 31 7e 0d";
    assert!(
        hex.contains(&format!("{a} {b}")) || hex.contains(&format!("{b} {a}")),
        "concurrent paste bytes interleaved: {text:?}"
    );

    assert_eq!(
        fixture.request(Request::CloseTerminal { session }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::CloseTerminal { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
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
fn resource_terminal_requests_preserve_background_state_and_close_cleanly() {
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
                branch: "feature/resource-terminal".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    let background = fixture.create_session(
        "background",
        vec![
            "sh".into(),
            "-c".into(),
            r#"printf '\033[?2004hREADY\r\n'; IFS= read -r line; printf '\r\nACK\r\n'; printf '%s' "$line" | od -An -tx1; printf 'TAIL\r\n'"#.into(),
        ],
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    let (background_size, initial_text) = loop {
        match fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: None,
        }) {
            Response::TerminalText { size, text, .. } if text.contains("READY") => {
                break (size, text);
            }
            _ if Instant::now() < deadline => thread::park_timeout(Duration::from_millis(10)),
            response => panic!("background terminal was not ready: {response:?}"),
        }
    };
    assert_eq!(
        background_size,
        ovrcr::session::TerminalSize {
            rows: 40,
            cols: 120
        }
    );
    assert!(!initial_text.contains("ACK"));
    assert!(matches!(
        fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: Some(0),
        }),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        fixture.request(Request::ReadTerminal {
            session: SessionId(u64::MAX),
            max_lines: None,
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));

    let mut dashboard = UnixStream::connect(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 2,
            request: Request::Select {
                session: local,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 2,
                response: Response::Screen { .. },
            }
        ) {
            break;
        }
    }

    let background_pid = match fixture.request(Request::Inspect) {
        Response::Inventory { registry, sessions } => {
            assert_eq!(registry.projects.len(), 1);
            assert_eq!(sessions.len(), 2);
            sessions
                .into_iter()
                .find(|session| session.id == background)
                .and_then(|session| session.pid)
                .expect("running background PID")
        }
        response => panic!("unexpected inventory response: {response:?}"),
    };
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: background,
            text: "alpha".into(),
            submit: false,
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: background,
            text: " beta".into(),
            submit: true,
        }),
        Response::Ok
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    let final_text = loop {
        match fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: None,
        }) {
            Response::TerminalText { size, text, .. } if text.contains("TAIL") => {
                assert_eq!(size, background_size, "background send resized the PTY");
                break text;
            }
            _ if Instant::now() < deadline => thread::park_timeout(Duration::from_millis(10)),
            response => panic!("background terminal did not acknowledge input: {response:?}"),
        }
    };
    let hex = final_text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        hex.contains("1b 5b 32 30 30 7e 61 6c 70 68 61 1b 5b 32 30 31 7e"),
        "first send was not bracketed: {final_text:?}"
    );
    assert!(
        hex.contains("1b 5b 32 30 30 7e 20 62 65 74 61 1b 5b 32 30 31 7e"),
        "submitted send was not bracketed: {final_text:?}"
    );
    assert!(matches!(
        fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: Some(1),
        }),
        Response::TerminalText { text, .. } if text == "TAIL"
    ));

    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 3,
            request: Request::Input {
                session: local,
                bytes: b"printf SELECTED_OK\r".to_vec(),
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 3,
                response: Response::Ok,
            }
        ) {
            break;
        }
    }

    fixture.wait_exited(background);
    assert_eq!(
        fixture.request(Request::CloseTerminal {
            session: background,
        }),
        Response::Ok
    );
    wait_for_group_absent(background_pid as libc::pid_t, Duration::from_secs(2));
    assert_eq!(
        fixture.request(Request::CloseTerminal { session: local }),
        Response::Ok
    );
    assert!(matches!(
        fixture.request(Request::CloseTerminal {
            session: background,
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    drop(dashboard);
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
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
fn fifty_sessions_survive_detach_and_leave_no_process_groups() {
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
                branch: "feature/fifty-sessions".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let sessions = (0..50)
        .map(|index| {
            fixture.create_session(
                &format!("waiting-{index}"),
                vec![
                    "sh".into(),
                    "-c".into(),
                    format!("printf 'SESSION_MARKER_{index}'; while :; do sleep 1; done").into(),
                ],
            )
        })
        .collect::<Vec<_>>();

    let mut dashboard = UnixStream::connect(&fixture.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    for (index, session) in sessions.iter().copied().enumerate() {
        select_screen_containing(
            &mut dashboard,
            index as u64 + 2,
            session,
            &format!("SESSION_MARKER_{index}"),
        );
    }
    drop(dashboard);

    let mut summaries = Vec::new();
    if let Response::Hierarchy(snapshot) = fixture.request(Request::List) {
        summaries = snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .filter(|summary| sessions.contains(&summary.id))
            .collect();
    }
    assert_eq!(summaries.len(), sessions.len());
    let pgids = summaries
        .iter()
        .filter_map(|summary| summary.pid)
        .map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) })
        .collect::<Vec<_>>();
    assert_eq!(
        pgids.len(),
        50,
        "all 50 managed process groups must be saved"
    );
    assert!(pgids.iter().all(|pgid| *pgid > 1 && group_exists(*pgid)));

    let mut reattached = UnixStream::connect(&fixture.socket).unwrap();
    write_frame(
        &mut reattached,
        &ClientMessage {
            request_id: 100,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut reattached).unwrap();
    reattached
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    for (index, session) in sessions.iter().copied().enumerate() {
        select_screen_containing(
            &mut reattached,
            index as u64 + 101,
            session,
            &format!("SESSION_MARKER_{index}"),
        );
    }
    drop(reattached);

    let previous_grace = std::env::var_os("OVRCR_KILL_GRACE_MS");
    unsafe { std::env::set_var("OVRCR_KILL_GRACE_MS", "500") };
    for session in sessions {
        assert_eq!(
            fixture.request(Request::KillSession { session }),
            Response::Ok
        );
    }
    match previous_grace {
        Some(value) => unsafe { std::env::set_var("OVRCR_KILL_GRACE_MS", value) },
        None => unsafe { std::env::remove_var("OVRCR_KILL_GRACE_MS") },
    }
    for pgid in pgids {
        wait_for_group_absent(pgid, Duration::from_secs(3));
    }
    if let Response::Hierarchy(snapshot) = fixture.request(Request::List) {
        assert!(
            snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .filter(|summary| summary.name.starts_with("waiting-"))
                .all(|summary| summary.pid.is_none())
        );
    }
    let local = fixture.only_session_id();
    fixture.request(Request::KillSession { session: local });
    fixture.wait_exited(local);
    for session in sessions_for_cleanup(&fixture) {
        fixture.request(Request::RemoveSession { session });
    }
    fixture.request(Request::RemoveSession { session: local });
    fixture.request(Request::RemoveWorkspace {
        project: "fixture".into(),
        name: "work".into(),
    });
    fixture.request(Request::RemoveProject {
        name: "fixture".into(),
    });
    fixture.request(Request::Shutdown { kill: false });
    fixture.join();
}

fn sessions_for_cleanup(fixture: &ControlFixture) -> Vec<SessionId> {
    match fixture.request(Request::List) {
        Response::Hierarchy(snapshot) => snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .filter(|session| session.name.starts_with("waiting-"))
            .map(|session| session.id)
            .collect(),
        _ => Vec::new(),
    }
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
    fn create_session_summary(
        &self,
        name: &str,
        argv: Vec<OsString>,
    ) -> ovrcr::session::SessionSummary {
        match self.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(),
            workspace: "work".into(),
            name: name.into(),
            label: None,
            argv,
        })) {
            Response::CreatedSession(summary) => summary,
            response => panic!("unexpected response: {response:?}"),
        }
    }
    fn create_session(&self, name: &str, argv: Vec<OsString>) -> SessionId {
        self.create_session_summary(name, argv).id
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
fn dashboard_geometry_sizes_connected_empty_and_detached_sessions() {
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
                branch: "feature/dashboard-geometry".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let mut dashboard = UnixStream::connect(&fixture.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 2,
            request: Request::DashboardGeometry {
                size: ovrcr::session::TerminalSize { rows: 17, cols: 61 },
            },
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 2,
            response: Response::Ok,
        }
    ));
    let connected_path = fixture._root.path().join("connected-size");
    let connected = fixture.create_session(
        "connected",
        vec![
            "sh".into(),
            "-c".into(),
            format!("stty size > {}; sleep 30", connected_path.display()).into(),
        ],
    );
    wait_for_file_contents(&connected_path, "17 61");
    drop(dashboard);
    let detached_path = fixture._root.path().join("detached-size");
    let detached = fixture.create_session(
        "detached",
        vec![
            "sh".into(),
            "-c".into(),
            format!("stty size > {}; sleep 30", detached_path.display()).into(),
        ],
    );
    wait_for_file_contents(&detached_path, "40 120");
    assert!(matches!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    ));
    fixture.join();
    let _ = (connected, detached);
}

#[test]
fn dashboard_receives_concrete_ordinary_request_errors() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = UnixStream::connect(&fixture.paths.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 2,
            request: Request::Select {
                session: SessionId(99),
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 2,
            response: Response::Error {
                code: ErrorCode::NotFound,
                message,
            },
        } if message.contains("session 99 not found")
    ));
    drop(dashboard);
    fixture.stop();
}

#[test]
fn selection_snapshot_precedes_later_quiet_tail_output() {
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
                branch: "feature/select-order".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session(
        "quiet-tail",
        vec![
            "sh".into(),
            "-c".into(),
            "printf READY; read line; printf QUIET_TAIL; exec sleep 30".into(),
        ],
    );
    let mut dashboard = UnixStream::connect(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 2,
            request: Request::Select {
                session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    let mut saw_snapshot = false;
    while !saw_snapshot {
        if let ServerMessage::Response {
            request_id: 2,
            response: Response::Screen { .. },
        } = read_frame::<ServerMessage>(&mut dashboard).unwrap()
        {
            saw_snapshot = true;
        }
    }
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 3,
            request: Request::Input {
                session,
                bytes: b"\n".to_vec(),
            },
        },
    )
    .unwrap();
    let mut saw_tail = false;
    while !saw_tail {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ovrcr::protocol::ServerEvent::Output { bytes, .. })
                if String::from_utf8_lossy(&bytes).contains("QUIET_TAIL") =>
            {
                saw_tail = true;
            }
            _ => {}
        }
    }
    assert!(saw_snapshot);
    assert!(saw_tail);
    drop(dashboard);
    assert!(matches!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    ));
    fixture.join();
}

fn wait_for_file_contents(path: &Path, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if std::fs::read_to_string(path)
            .map(|contents| contents.trim() == expected)
            .unwrap_or(false)
        {
            return;
        }
        thread::yield_now();
    }
    panic!("{} did not contain {expected:?}", path.display());
}

fn select_screen_containing(
    stream: &mut UnixStream,
    request_id: u64,
    session: SessionId,
    marker: &str,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        write_frame(
            stream,
            &ClientMessage {
                request_id,
                request: Request::Select {
                    session,
                    size: ovrcr::session::TerminalSize {
                        rows: 40,
                        cols: 120,
                    },
                },
            },
        )
        .unwrap();
        while Instant::now() < deadline {
            let Ok(message) = read_frame::<ServerMessage>(stream) else {
                break;
            };
            if let ServerMessage::Response {
                response: Response::Screen { bytes, .. },
                ..
            } = message
                && String::from_utf8_lossy(&bytes).contains(marker)
            {
                return;
            }
        }
    }
    panic!("session {session:?} did not render {marker:?}");
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
fn cli_resolves_relative_project_paths_against_invocation_cwd_with_existing_server() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let fixture = ControlFixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args([
            "project",
            "add",
            "relative",
            ".",
            "--workspace-root",
            "../workspaces",
        ])
        .current_dir(&fixture.repo)
        .env("OVRCR_CONFIG", fixture._root.path().join("config.toml"))
        .env("OVRCR_SOCKET", &fixture.socket)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "relative project registration failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let registry = Registry::load(&fixture._root.path().join("config.toml")).unwrap();
    let project = registry
        .projects
        .iter()
        .find(|project| project.name == "relative")
        .unwrap();
    assert_eq!(project.repo, fixture.repo);
    assert_eq!(project.workspace_root, fixture.workspace_root);
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
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

fn wait_for_group_absent(pgid: libc::pid_t, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !group_exists(pgid) {
            return;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    panic!("PTY process group {pgid} did not disappear");
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
