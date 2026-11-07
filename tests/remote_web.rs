//! The settings toggle really starts and stops the remote gateway.
//!
//! Runs a disposable Server (no PTYs needed) with `[remote] enabled = true`,
//! waits for the child gateway to answer, flips the setting through the same
//! Server request the Dashboard uses, and expects the listener to close.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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
