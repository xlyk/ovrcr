#[path = "support/live.rs"]
mod live;

use live::Live;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn server_publishes_private_build_identity_before_accepting_clients() {
    let fixture = serving_fixture();
    let path = fixture.socket.with_extension("sock.build.json");
    let contents =
        fs::read(&path).expect("running server must publish its captured build identity");
    let identity: serde_json::Value = serde_json::from_slice(&contents).unwrap();
    assert_eq!(
        identity["pid"].as_u64(),
        fixture.server_pid().map(u64::from)
    );
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

use ovrcr::client::connect_dashboard;
use ovrcr::protocol::{Request, Response, client};
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static ENVIRONMENT: Mutex<()> = Mutex::new(());

struct Environment {
    _lock: MutexGuard<'static, ()>,
    values: Vec<(&'static str, Option<OsString>)>,
}

impl Environment {
    fn new(fixture: &Live, executable: &Path) -> Self {
        let lock = ENVIRONMENT
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let values = [
            ("HOME", fixture.root.path().as_os_str()),
            ("OVRCR_HOME", fixture.config.as_os_str()),
            ("OVRCR_SOCKET", fixture.socket.as_os_str()),
            ("OVRCR_SERVER_EXECUTABLE", executable.as_os_str()),
            ("SHELL", std::ffi::OsStr::new("/bin/sh")),
        ]
        .into_iter()
        .map(|(key, value)| {
            let previous = std::env::var_os(key);
            // These tests serialize all process environment changes in this binary.
            unsafe { std::env::set_var(key, value) };
            (key, previous)
        })
        .collect();
        Self {
            _lock: lock,
            values,
        }
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            };
        }
    }
}

fn serving_fixture() -> Live {
    let fixture = Live::binary();
    // A successful request establishes that startup reached the accept loop;
    // the shared fixture's socket-existence wait alone does not establish it.
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(_)
    ));
    fixture
}

fn copied_fixture() -> Live {
    let mut fixture = Live::idle();
    let copy = fixture.root.path().join("ovrcr");
    fs::copy(&fixture.executable, &copy).unwrap();
    fixture.executable = copy;
    fixture.start_binary();
    let fixture = fixture.bounded();
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(_)
    ));
    fixture
}

fn replace_binary(path: &Path) {
    let mut contents = fs::read(path).unwrap();
    contents.extend_from_slice(b"OVRCR_SAME_PROTOCOL_DIFFERENT_BUILD");
    let staging = path.with_extension("next");
    fs::write(&staging, contents).unwrap();
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(staging, path).unwrap();
}

#[test]
fn same_build_connects_without_restart_offer() {
    let fixture = serving_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let mut stream =
        connect_dashboard(&fixture.paths(), &mut |_| panic!("same build must attach")).unwrap();
    assert!(client::list(&mut stream, 1).unwrap().projects.is_empty());
}

#[test]
fn unknown_build_decline_preserves_running_session() {
    let fixture = serving_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    fixture.ready("unknown-server");
    let before = fixture.request(Request::Inspect);
    fs::remove_file(fixture.socket.with_extension("sock.build.json")).unwrap();
    let mut prompts = Vec::new();
    let error = connect_dashboard(&fixture.paths(), &mut |prompt| {
        prompts.push(prompt.to_owned());
        Ok(false)
    })
    .expect_err("unknown build must require explicit restart approval");
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts[0].contains("unknown")
            && prompts[0].contains("running sessions")
            && prompts[0].contains(&fixture.socket.display().to_string()),
        "{}",
        prompts[0]
    );
    assert!(format!("{error:#}").contains("declined"));
    assert_eq!(fixture.request(Request::Inspect), before);
}

#[test]
fn captured_build_survives_atomic_executable_replacement_and_decline() {
    let fixture = copied_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let sidecar = fixture.socket.with_extension("sock.build.json");
    let captured = fs::read(&sidecar).unwrap();
    replace_binary(&fixture.executable);
    let mut prompts = 0;
    connect_dashboard(&fixture.paths(), &mut |prompt| {
        assert!(prompt.contains("different build"), "{prompt}");
        prompts += 1;
        Ok(false)
    })
    .expect_err("same-protocol replacement must be detected");
    assert_eq!(prompts, 1);
    assert_eq!(fs::read(sidecar).unwrap(), captured);
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(_)
    ));
}

#[test]
fn accepted_restart_replaces_unknown_build_through_checked_connection() {
    let fixture = serving_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let _cleanup = RestartCleanup(&fixture);
    let before = fixture.server_pid().unwrap();
    fixture.ready("approved-restart");
    let groups = fixture.session_groups();
    for group in &groups {
        fixture.own_group(*group);
    }
    fs::remove_file(fixture.socket.with_extension("sock.build.json")).unwrap();
    let mut offers = 0;
    let mut stream = connect_dashboard(&fixture.paths(), &mut |_| {
        offers += 1;
        Ok(true)
    })
    .unwrap();
    assert_eq!(offers, 1);
    assert!(fixture.join_within(Duration::from_secs(3)).is_finished());
    let identity: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.socket.with_extension("sock.build.json")).unwrap(),
    )
    .unwrap();
    assert_ne!(identity["pid"].as_u64(), Some(u64::from(before)));
    for group in groups {
        assert!(!live::group_exists(group));
        fixture.forget_group(group);
    }
    client::list(&mut stream, 1).unwrap();
    assert_eq!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    wait_stopped(&fixture);
}

#[test]
fn changed_identity_during_consent_does_not_shutdown_server() {
    let fixture = serving_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let sidecar = fixture.socket.with_extension("sock.build.json");
    let captured = fs::read(&sidecar).unwrap();
    fs::remove_file(&sidecar).unwrap();
    let error = connect_dashboard(&fixture.paths(), &mut |_| {
        fs::write(&sidecar, &captured).unwrap();
        fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)).unwrap();
        Ok(true)
    })
    .expect_err("approval must not authorize a changed identity");
    assert!(format!("{error:#}").contains("changed"), "{error:#}");
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(_)
    ));
}

#[test]
fn missing_server_uses_the_explicit_alternate_executable_without_prompt() {
    let fixture = Live::idle();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let _cleanup = RestartCleanup(&fixture);
    let mut stream = connect_dashboard(&fixture.paths(), &mut |_| {
        panic!("missing server needs no destructive approval")
    })
    .unwrap();
    let identity: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.socket.with_extension("sock.build.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(identity["executable"].as_str(), fixture.executable.to_str());
    client::shutdown(&mut stream, 1, true).unwrap();
    wait_stopped(&fixture);
}

#[test]
fn socket_replacement_during_consent_never_receives_shutdown() {
    use std::os::unix::net::UnixListener;
    let fixture = serving_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    fs::remove_file(fixture.socket.with_extension("sock.build.json")).unwrap();
    let original_socket = fixture.socket.with_extension("original");
    let mut replacement = None;
    let error = connect_dashboard(&fixture.paths(), &mut |_| {
        fs::rename(&fixture.socket, &original_socket).unwrap();
        replacement = Some(UnixListener::bind(&fixture.socket).unwrap());
        Ok(true)
    })
    .expect_err("a newly bound socket is outside the approval");
    assert!(format!("{error:#}").contains("changed"), "{error:#}");
    replacement.as_ref().unwrap().set_nonblocking(true).unwrap();
    assert_eq!(
        replacement.as_ref().unwrap().accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let mut original = ovrcr::protocol::connect_server(&original_socket).unwrap();
    client::list(&mut original, 1).unwrap();
    drop(replacement);
    fs::remove_file(&fixture.socket).unwrap();
    fs::rename(original_socket, &fixture.socket).unwrap();
}

#[test]
fn newer_protocol_server_refuses_without_prompt_or_shutdown_frame() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    let fixture = Live::idle();
    let _environment = Environment::new(&fixture, &fixture.executable);
    fs::create_dir_all(fixture.socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&fixture.socket).unwrap();
    let responder = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut preamble = Vec::from(*b"OVRC");
        preamble.extend_from_slice(&(ovrcr::protocol::PROTOCOL_VERSION + 1).to_be_bytes());
        stream.write_all(&preamble).unwrap();
        ovrcr::protocol::read_preamble(&mut stream).unwrap();
        assert_eq!(
            stream.read(&mut [0; 1]).unwrap(),
            0,
            "incompatible peers must receive no old protocol frame"
        );
    });
    let error = connect_dashboard(&fixture.paths(), &mut |_| {
        panic!("a newer server must not be replaced by an older CLI")
    })
    .unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("older than the running server")
            && text.contains("newer CLI")
            && text.contains("shutdown --kill")
            && text.contains(&fixture.socket.display().to_string()),
        "{text}"
    );
    responder.join().unwrap();
}

/// Run `older_protocol_server_child` as a separate process: an accepted
/// restart signals the socket peer, which must never be this test process.
fn spawn_older_server(fixture: &Live) -> std::process::Child {
    fs::create_dir_all(fixture.socket.parent().unwrap()).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "older_protocol_server_child",
            "--ignored",
            "--nocapture",
        ])
        .env("OVRCR_SOCKET", &fixture.socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !fixture.socket.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "older-protocol fixture did not bind"
        );
        std::thread::park_timeout(Duration::from_millis(5));
    }
    child
}

struct OlderServer(std::process::Child);

impl Drop for OlderServer {
    fn drop(&mut self) {
        // This test spawned the child; its PID cannot have been reused before
        // this owner reaps it.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn accepted_restart_stops_older_protocol_server_and_starts_this_build() {
    let fixture = Live::idle();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let _cleanup = RestartCleanup(&fixture);
    let mut older = OlderServer(spawn_older_server(&fixture));
    let mut prompts = Vec::new();
    let mut stream = connect_dashboard(&fixture.paths(), &mut |prompt| {
        prompts.push(prompt.to_owned());
        Ok(true)
    })
    .unwrap();
    assert_eq!(prompts.len(), 1);
    let protocol = ovrcr::protocol::PROTOCOL_VERSION;
    assert!(
        prompts[0].starts_with("Restart OVRCR server\n")
            && prompts[0].contains(&format!(
                "an older protocol (server {}, this CLI {protocol})",
                protocol - 1
            ))
            && prompts[0].contains("All running sessions will stop.")
            && prompts[0].contains("terminal output is not kept")
            && prompts[0].contains("Reopen starts a fresh shell")
            && prompts[0].contains(&fixture.socket.display().to_string()),
        "{}",
        prompts[0]
    );
    assert!(older.0.wait().unwrap().success());
    assert_eq!(
        fs::read_to_string(fixture.socket.with_extension("sigterm")).unwrap(),
        "SIGTERM"
    );
    let identity: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.socket.with_extension("sock.build.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(identity["executable"].as_str(), fixture.executable.to_str());
    client::list(&mut stream, 1).unwrap();
    assert_eq!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    wait_stopped(&fixture);
}

#[test]
fn declined_older_protocol_restart_sends_no_signal() {
    let fixture = Live::idle();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let mut older = OlderServer(spawn_older_server(&fixture));
    let mut prompts = 0;
    let error = connect_dashboard(&fixture.paths(), &mut |prompt| {
        assert!(prompt.contains("an older protocol"), "{prompt}");
        prompts += 1;
        Ok(false)
    })
    .expect_err("declining must leave the older server running");
    assert_eq!(prompts, 1);
    assert!(format!("{error:#}").contains("declined"), "{error:#}");
    std::thread::sleep(Duration::from_millis(100));
    assert!(older.0.try_wait().unwrap().is_none());
    assert!(fixture.socket.exists());
    assert!(!fixture.socket.with_extension("sigterm").exists());
}

#[test]
fn older_protocol_socket_replacement_during_consent_sends_no_signal() {
    use std::os::unix::net::UnixListener;
    let fixture = Live::idle();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let mut older = OlderServer(spawn_older_server(&fixture));
    let original_socket = fixture.socket.with_extension("original");
    let mut replacement = None;
    let error = connect_dashboard(&fixture.paths(), &mut |_| {
        fs::rename(&fixture.socket, &original_socket).unwrap();
        replacement = Some(UnixListener::bind(&fixture.socket).unwrap());
        Ok(true)
    })
    .expect_err("a newly bound socket is outside the approval");
    assert!(format!("{error:#}").contains("changed"), "{error:#}");
    std::thread::sleep(Duration::from_millis(100));
    assert!(older.0.try_wait().unwrap().is_none());
    assert!(!fixture.socket.with_extension("sigterm").exists());
    drop(replacement);
}

#[test]
fn server_sigterm_runs_kill_shutdown_and_stops_sessions() {
    // The older-protocol restart relies on this signal contract.
    let fixture = serving_fixture();
    fixture.ready("sigterm-restart");
    let groups = fixture.session_groups();
    assert!(!groups.is_empty());
    for group in &groups {
        fixture.own_group(*group);
    }
    let pid = fixture.server_pid().unwrap();
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) }, 0);
    assert!(fixture.join_within(Duration::from_secs(10)).is_finished());
    assert!(!fixture.socket.exists());
    for group in groups {
        assert!(!live::group_exists(group));
        fixture.forget_group(group);
    }
}

#[test]
fn controlled_shutdown_refusal_preserves_socket_and_does_not_spawn() {
    use ovrcr::protocol::{ClientMessage, ErrorCode, ServerMessage, read_frame, write_frame};
    use std::os::unix::net::UnixListener;
    let fixture = Live::idle();
    let _environment = Environment::new(&fixture, &fixture.executable);
    fs::create_dir_all(fixture.socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&fixture.socket).unwrap();
    let inode = fs::metadata(&fixture.socket).unwrap().ino();
    let responder = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        ovrcr::protocol::exchange_preamble(&mut stream).unwrap();
        let request: ClientMessage = read_frame(&mut stream).unwrap();
        assert_eq!(request.request, Request::Shutdown { kill: true });
        write_frame(
            &mut stream,
            &ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Error {
                    code: ErrorCode::Conflict,
                    message: "fixture refuses controlled shutdown".into(),
                },
            },
        )
        .unwrap();
    });
    let error = connect_dashboard(&fixture.paths(), &mut |_| Ok(true))
        .expect_err("refused controlled shutdown must stop startup");
    assert!(
        format!("{error:#}").contains("fixture refuses controlled shutdown"),
        "{error:#}"
    );
    assert_eq!(fs::metadata(&fixture.socket).unwrap().ino(), inode);
    responder.join().unwrap();
}

#[test]
fn stale_fixed_service_refuses_before_restart_offer_and_preserves_server() {
    let fixture = serving_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let mut config = ovrcr::service::ServiceConfig::resolve().unwrap();
    config.executable = fixture.root.path().join("old-fixed-service-ovrcr");
    fs::create_dir_all(config.definition_path.parent().unwrap()).unwrap();
    #[cfg(target_os = "macos")]
    let definition = format!(
        "<key>ProgramArguments</key>\n<array>\n<string>{}</string>\n<string>server</string>\n</array>\n<key>OVRCR_SOCKET</key>\n<string>{}</string>\n",
        config.executable.display(),
        fixture.socket.display()
    );
    #[cfg(target_os = "linux")]
    let definition = format!(
        "Environment=\"OVRCR_SOCKET={}\"\nExecStart=\"{}\" server\n",
        fixture.socket.display(),
        config.executable.display()
    );
    fs::write(&config.definition_path, definition).unwrap();
    let error = connect_dashboard(&fixture.paths(), &mut |_| {
        panic!("fixed stale service cannot be safely replaced by this offer")
    })
    .unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("service install --kill-sessions")
            && text.contains(&config.definition_path.display().to_string()),
        "{text}"
    );
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(_)
    ));
}

// The fixture's old child and a client-started replacement are distinct hosts.
// This guard owns the private root's detached replacement on every failure path.
struct RestartCleanup<'a>(&'a Live);

impl Drop for RestartCleanup<'_> {
    fn drop(&mut self) {
        if let Ok(Some(mut stream)) = ovrcr::client::connect_if_running(&self.0.paths()) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let _ = client::shutdown(&mut stream, 999, true);
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while self.0.socket.exists() && std::time::Instant::now() < deadline {
                std::thread::park_timeout(Duration::from_millis(5));
            }
            if self.0.socket.exists() {
                eprintln!(
                    "owned replacement cleanup incomplete: {}",
                    self.0.socket.display()
                );
            }
        }
    }
}

fn wait_stopped(fixture: &Live) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while fixture.socket.exists() && std::time::Instant::now() < deadline {
        std::thread::park_timeout(Duration::from_millis(5));
    }
    assert!(
        !fixture.socket.exists(),
        "owned detached server did not finish cleanup"
    );
    assert!(!fixture.socket.with_extension("sock.build.json").exists());
}

#[test]
fn accepted_restart_runs_changed_binary_with_same_protocol_and_package_version() {
    let fixture = copied_fixture();
    let _environment = Environment::new(&fixture, &fixture.executable);
    let _cleanup = RestartCleanup(&fixture);
    let original_version = fixture.command().arg("--version").output().unwrap();
    assert!(original_version.status.success());
    let original_identity = fs::read(fixture.socket.with_extension("sock.build.json")).unwrap();
    replace_binary(&fixture.executable);
    let changed_version = fixture.command().arg("--version").output().unwrap();
    assert!(changed_version.status.success());
    assert_eq!(changed_version.stdout, original_version.stdout);
    let mut stream = connect_dashboard(&fixture.paths(), &mut |_| Ok(true)).unwrap();
    fixture.join();
    assert_ne!(
        fs::read(fixture.socket.with_extension("sock.build.json")).unwrap(),
        original_identity
    );
    client::list(&mut stream, 1).unwrap();
    assert_eq!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    wait_stopped(&fixture);
}

#[test]
fn server_refuses_unrecognized_existing_identity_without_replacing_user_file() {
    let fixture = Live::idle();
    fs::create_dir_all(fixture.socket.parent().unwrap()).unwrap();
    let path = fixture.socket.with_extension("sock.build.json");
    fs::write(&path, "unrelated user file").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut child = fixture
        .command()
        .arg("server")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
        std::thread::park_timeout(Duration::from_millis(5));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = live::request_with_timeout(
            &fixture.socket,
            1,
            Request::Shutdown { kill: true },
            Duration::from_secs(2),
        );
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    assert!(!status.success(), "must not overwrite an unrecognized file");
    assert_eq!(fs::read_to_string(&path).unwrap(), "unrelated user file");
    assert!(
        !fixture.socket.exists(),
        "failed publication must remove its owned socket"
    );
}

fn startup_peer_executable(fixture: &Live, mode: &str) -> std::path::PathBuf {
    let host = std::env::current_exe().unwrap();
    let quoted_host = host.to_str().unwrap().replace('\'', "'\\''");
    let executable = fixture.root.path().join("startup-peer");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nOVRCR_TEST_STARTUP_PEER_MODE={mode} exec '{quoted_host}' --exact startup_peer_child --ignored --nocapture\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    executable
}

// The client owns this child, and the fixture owns its release file and group.
// Release it on every assertion failure before Live removes its private root.
struct StartupPeerCleanup<'a>(&'a Live);

impl Drop for StartupPeerCleanup<'_> {
    fn drop(&mut self) {
        let directory = self.0.socket.parent().unwrap();
        let _ = fs::write(directory.join("startup-peer.release"), "release");
        let Some(group) = fs::read_to_string(directory.join("startup-peer.pgid"))
            .ok()
            .and_then(|text| text.trim().parse::<libc::pid_t>().ok())
        else {
            return;
        };
        eprintln!(
            "startup peer fixture: root={} owned_pgid={group}",
            self.0.root.path().display()
        );
        self.0.own_group(group);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while live::group_exists(group) && std::time::Instant::now() < deadline {
            let mut status = 0;
            // connect_or_start spawned this fixture child from this test process.
            // Reap it after release; a dropped Child otherwise leaves a zombie.
            unsafe { libc::waitpid(group, &mut status, libc::WNOHANG) };
            std::thread::park_timeout(Duration::from_millis(10));
        }
        if !live::group_exists(group) {
            self.0.forget_group(group);
            eprintln!("startup peer fixture: owned_pgid={group} reaped");
        }
    }
}

#[test]
fn spawned_peer_exit_after_handshake_failure_reports_its_status_and_log() {
    let fixture = Live::idle();
    let executable = startup_peer_executable(&fixture, "exit");
    let _environment = Environment::new(&fixture, &executable);
    let _cleanup = StartupPeerCleanup(&fixture);
    let started = std::time::Instant::now();
    let error = ovrcr::client::connect_or_start(&fixture.paths()).unwrap_err();
    let text = format!("{error:#}");
    assert!(text.contains("exited during startup"), "{text}");
    assert!(text.contains("17"), "{text}");
    assert!(
        text.contains("fixture startup failure after accepting client"),
        "{text}"
    );
    assert!(
        text.contains(
            &fixture
                .socket
                .parent()
                .unwrap()
                .join("server.log")
                .display()
                .to_string()
        ),
        "{text}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!fixture.socket.exists());
}

#[test]
fn spawned_live_peer_preserves_first_handshake_error_without_reconnecting() {
    let fixture = Live::idle();
    let executable = startup_peer_executable(&fixture, "live");
    let _environment = Environment::new(&fixture, &executable);
    let _cleanup = StartupPeerCleanup(&fixture);
    let started = std::time::Instant::now();
    let error = ovrcr::client::connect_or_start(&fixture.paths()).unwrap_err();
    let elapsed = started.elapsed();
    let text = format!("{error:#}");
    assert!(text.contains("protocol version mismatch"), "{text}");
    assert!(!text.contains("exited during startup"), "{text}");
    assert!(
        !text.contains("timed out waiting for server startup"),
        "{text}"
    );
    assert!(elapsed >= Duration::from_secs(5), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(6), "{elapsed:?}");
    assert_eq!(
        fs::read_to_string(
            fixture
                .socket
                .parent()
                .unwrap()
                .join("startup-peer.attempts")
        )
        .unwrap(),
        "accepted\n"
    );
}

#[test]
#[ignore = "subprocess fixture for spawned-child startup tests"]
fn startup_peer_child() {
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    let mode = std::env::var("OVRCR_TEST_STARTUP_PEER_MODE").unwrap();
    let socket = std::path::PathBuf::from(std::env::var_os("OVRCR_SOCKET").unwrap());
    let directory = socket.parent().unwrap();
    let group = unsafe { libc::getpgrp() };
    assert_eq!(group as u32, std::process::id());
    fs::write(directory.join("startup-peer.pgid"), group.to_string()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    ovrcr::protocol::read_preamble(&mut stream).unwrap();
    fs::write(directory.join("startup-peer.attempts"), "accepted\n").unwrap();
    if mode == "exit" {
        eprintln!("fixture startup failure after accepting client");
        drop(stream);
        drop(listener);
        fs::remove_file(socket).unwrap();
        std::process::exit(17);
    }
    assert_eq!(mode, "live");
    let mut preamble = Vec::from(*b"OVRC");
    preamble.extend_from_slice(&(ovrcr::protocol::PROTOCOL_VERSION - 1).to_be_bytes());
    stream.write_all(&preamble).unwrap();
    drop(stream);
    listener.set_nonblocking(true).unwrap();
    let release = directory.join("startup-peer.release");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !release.exists() && std::time::Instant::now() < deadline {
        if let Ok((stream, _)) = listener.accept() {
            fs::OpenOptions::new()
                .append(true)
                .open(directory.join("startup-peer.attempts"))
                .unwrap()
                .write_all(b"accepted\n")
                .unwrap();
            drop(stream);
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        release.exists(),
        "fixture owner did not release startup peer"
    );
    drop(listener);
    fs::remove_file(socket).unwrap();
}

#[test]
#[ignore = "subprocess fixture for older-protocol restart tests"]
fn older_protocol_server_child() {
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let socket = std::path::PathBuf::from(std::env::var_os("OVRCR_SOCKET").unwrap());
    let stopped = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(libc::SIGTERM, Arc::clone(&stopped)).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !stopped.load(Ordering::Acquire) {
        assert!(
            std::time::Instant::now() < deadline,
            "older-protocol fixture was never stopped"
        );
        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            let mut preamble = Vec::from(*b"OVRC");
            preamble.extend_from_slice(&(ovrcr::protocol::PROTOCOL_VERSION - 1).to_be_bytes());
            let _ = stream.write_all(&preamble);
            let _ = ovrcr::protocol::read_preamble(&mut stream);
        }
        std::thread::park_timeout(Duration::from_millis(5));
    }
    // Mirror the server's signal shutdown: remove the socket, then exit cleanly.
    fs::write(socket.with_extension("sigterm"), "SIGTERM").unwrap();
    drop(listener);
    fs::remove_file(&socket).unwrap();
}
