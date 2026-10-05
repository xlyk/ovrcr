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
            ("OVRCR_CONFIG", fixture.config.as_os_str()),
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
fn incompatible_protocol_refuses_without_prompt_or_shutdown_frame() {
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
        preamble.extend_from_slice(&(ovrcr::protocol::PROTOCOL_VERSION - 1).to_be_bytes());
        stream.write_all(&preamble).unwrap();
        ovrcr::protocol::read_preamble(&mut stream).unwrap();
        assert_eq!(
            stream.read(&mut [0; 1]).unwrap(),
            0,
            "incompatible peers must receive no old protocol frame"
        );
    });
    let error = connect_dashboard(&fixture.paths(), &mut |_| {
        panic!("incompatible protocol cannot offer automatic restart")
    })
    .unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("matching old CLI")
            && text.contains("shutdown --kill")
            && text.contains(&fixture.socket.display().to_string()),
        "{text}"
    );
    responder.join().unwrap();
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
