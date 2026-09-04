use ovrcr::config::Registry;
use ovrcr::protocol::{ClientMessage, Request, Response, ServerMessage, read_frame, write_frame};
use ovrcr::server::{ServerPaths, connect_if_running, connect_or_start, run_server};
use std::io::Read;
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

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
    let mut byte = [0_u8; 1];
    assert_eq!(
        second.read(&mut byte).unwrap(),
        0,
        "duplicate dashboard must be closed without a second writer"
    );
    first.shutdown(Shutdown::Both).unwrap();
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
