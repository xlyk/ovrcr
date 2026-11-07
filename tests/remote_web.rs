//! The settings toggle really starts and stops the remote gateway.
//!
//! Runs a disposable Server (no PTYs needed) with `[remote] enabled = true`,
//! waits for the child gateway to answer, flips the setting through the same
//! Server request the Dashboard uses, and expects the listener to close.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve a port")
        .local_addr()
        .expect("read the reserved port")
        .port()
}

fn http_get(port: u16, path: &str) -> std::io::Result<Vec<u8>> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(response)
}

fn http_post(port: u16, path: &str, body: &str) -> std::io::Result<Vec<u8>> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(response)
}

fn status_of(response: &[u8]) -> Option<u16> {
    let text = String::from_utf8_lossy(response);
    text.lines().next()?.split_whitespace().nth(1)?.parse().ok()
}

fn body_of(response: &[u8]) -> &[u8] {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map_or(response, |at| &response[at + 4..])
}

fn wait_until(deadline: Instant, mut check: impl FnMut() -> bool) -> bool {
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn remote_gateway_follows_the_settings_toggle() {
    let home = tempfile::tempdir().expect("temporary instance");
    let port = free_port();
    std::fs::write(
        home.path().join("dashboard.toml"),
        format!("[remote]\nenabled = true\nport = {port}\n"),
    )
    .expect("write dashboard.toml");

    let binary = env!("CARGO_BIN_EXE_ovrcr");
    let child = Command::new(binary)
        .arg("server")
        .env("OVRCR_HOME", home.path())
        .env_remove("OVRCR_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the Server");
    let guard = ServerGuard(child);

    let socket = home.path().join("server.sock");
    let deadline = Instant::now() + Duration::from_secs(10);
    assert!(
        wait_until(deadline, || socket.exists()),
        "Server socket did not appear"
    );

    assert!(
        wait_until(deadline, || http_get(port, "/api/state").is_ok()),
        "the remote gateway did not start from the setting"
    );
    let body = http_get(port, "/api/state").expect("request state");
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"sessions\":[]"), "unexpected state: {body}");

    let page = http_get(port, "/").expect("request index");
    assert!(
        String::from_utf8_lossy(&page).contains("OVRCR"),
        "the embedded page was not served"
    );
    let app = http_get(port, "/app.js").expect("request app.js");
    assert!(
        String::from_utf8_lossy(&app).contains("ROW_PX"),
        "the embedded script was not served"
    );
    let font = http_get(port, "/fonts/jetbrains-mono-nerd-regular.woff2").expect("request font");
    assert!(
        font.windows(b"font/woff2".len())
            .any(|window| window == b"font/woff2"),
        "the embedded font was not served as a font"
    );

    let status = Command::new(binary)
        .args(["--json", "settings", "set", "remote.enabled", "false"])
        .env("OVRCR_HOME", home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("write the setting through the Server");
    assert!(status.success(), "setting write failed");

    let deadline = Instant::now() + Duration::from_secs(5);
    assert!(
        wait_until(deadline, || http_get(port, "/api/state").is_err()),
        "gateway still listening after remote.enabled = false"
    );

    let _ = Command::new(binary)
        .args(["--json", "shutdown"])
        .env("OVRCR_HOME", home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    drop(guard);
}

#[test]
fn remote_gateway_creates_an_agent_session_from_the_phone_payload() {
    let home = tempfile::tempdir().expect("temporary instance");
    let repo = home.path().join("repo");
    fs::create_dir_all(&repo).expect("create the repo");
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["init", "-q"])
            .status()
            .expect("git init")
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "commit",
                "--allow-empty",
                "-qm",
                "init",
            ])
            .status()
            .expect("git commit")
            .success()
    );

    let bin_dir = home.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("create the stub bin dir");
    let args_path = home.path().join("agent-args.txt");
    let stub = bin_dir.join("omp");
    fs::write(
        &stub,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$OVRCR_TEST_ARGS\"\nsleep 5\n",
    )
    .expect("write the agent stub");
    let mut permissions = fs::metadata(&stub).expect("stat the stub").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(&stub, permissions).expect("make the stub executable");

    let port = free_port();
    fs::write(
        home.path().join("dashboard.toml"),
        format!("[remote]\nenabled = true\nport = {port}\n"),
    )
    .expect("write dashboard.toml");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let binary = env!("CARGO_BIN_EXE_ovrcr");
    let child = Command::new(binary)
        .arg("server")
        .env("OVRCR_HOME", home.path())
        .env_remove("OVRCR_SOCKET")
        .env("PATH", &path)
        .env("OVRCR_TEST_ARGS", &args_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the Server");
    let guard = ServerGuard(child);

    let socket = home.path().join("server.sock");
    let deadline = Instant::now() + Duration::from_secs(10);
    assert!(
        wait_until(deadline, || socket.exists()),
        "Server socket did not appear"
    );
    assert!(
        wait_until(deadline, || http_get(port, "/api/state").is_ok()),
        "the remote gateway did not start"
    );

    let added = Command::new(binary)
        .args(["project", "add", "--workspace-root"])
        .arg(&repo)
        .arg("gui")
        .arg(&repo)
        .env("OVRCR_HOME", home.path())
        .env_remove("OVRCR_SOCKET")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("add the project");
    assert!(added.success(), "project add failed");

    let deadline = Instant::now() + Duration::from_secs(10);
    let state = loop {
        let raw = http_get(port, "/api/state").expect("state");
        let value: serde_json::Value = serde_json::from_slice(body_of(&raw)).expect("state JSON");
        if value["projects"]
            .as_array()
            .is_some_and(|projects| !projects.is_empty())
        {
            break value;
        }
        assert!(
            Instant::now() < deadline,
            "project missing from state: {}",
            String::from_utf8_lossy(&raw)
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let project = &state["projects"][0];
    assert_eq!(project["name"], "gui");
    let workspace = project["workspaces"][0]["id"]
        .as_str()
        .expect("workspace id");
    let agents = state["agents"].as_array().expect("agents");
    assert!(
        agents.iter().any(|agent| {
            agent["name"] == "omp" && agent["takes_task"] == serde_json::Value::Bool(true)
        }),
        "stub omp was not detected: {agents:?}"
    );

    let payload = serde_json::json!({
        "project": "gui",
        "workspace": workspace,
        "agent": "omp",
        "prompt": "remote prompt",
    })
    .to_string();
    let raw = http_post(port, "/api/session", &payload).expect("create request");
    assert_eq!(
        status_of(&raw),
        Some(200),
        "create failed: {}",
        String::from_utf8_lossy(&raw)
    );
    let created: serde_json::Value = serde_json::from_slice(body_of(&raw)).expect("create JSON");
    let id = created["session"]["id"].as_u64().expect("created id");
    assert_eq!(created["session"]["kind"]["Agent"]["name"], "omp");
    assert_eq!(created["session"]["label"], "Oh My Pi");

    let deadline = Instant::now() + Duration::from_secs(10);
    let args = loop {
        if let Ok(text) = fs::read_to_string(&args_path)
            && !text.trim().is_empty()
        {
            break text;
        }
        assert!(Instant::now() < deadline, "the agent stub never ran");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        args.lines().any(|line| line == "remote prompt"),
        "prompt was not passed to the agent: {args:?}"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    assert!(
        wait_until(deadline, || {
            http_get(port, "/api/state")
                .ok()
                .and_then(|raw| serde_json::from_slice::<serde_json::Value>(body_of(&raw)).ok())
                .is_some_and(|value| {
                    value["sessions"]
                        .as_array()
                        .is_some_and(|sessions| sessions.iter().any(|session| session["id"] == id))
                })
        }),
        "created session did not appear in state"
    );

    let _ = Command::new(binary)
        .args(["--json", "shutdown"])
        .env("OVRCR_HOME", home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    drop(guard);
}
