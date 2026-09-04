use ovrcr::config::Registry;
use ovrcr::protocol::{ClientMessage, Request, Response, ServerMessage, read_frame, write_frame};
use ovrcr::server::{ServerPaths, connect_if_running, connect_or_start, run_server};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
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
