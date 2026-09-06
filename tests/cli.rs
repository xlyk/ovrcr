use ovrcr::protocol::{
    AgentReport, AgentUpdate, ClientMessage, Request, Response, ServerMessage, read_frame,
    write_frame,
};
use ovrcr::session::{SessionId, SessionPhase};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

use std::time::{Duration, Instant};

fn isolated_command(root: &tempfile::TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
    command
        .env("OVRCR_CONFIG", root.path().join("config.toml"))
        .env("OVRCR_SOCKET", root.path().join("server.sock"));
    command
}

#[test]
fn agent_hook_cli_requires_identity_without_starting_server() {
    let root = tempfile::tempdir().unwrap();
    let output = isolated_command(&root)
        .args([
            "--json",
            "report",
            "activity",
            "--state",
            "busy",
            "--sequence",
            "1",
        ])
        .env_remove("OVRCR_HOOK_SOCKET")
        .env_remove("OVRCR_SESSION_ID")
        .env_remove("OVRCR_HOOK_TOKEN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("hook identity"));
    assert!(!root.path().join("server.sock").exists());
    assert!(!root.path().join("config.toml").exists());
}

#[test]
fn agent_hook_cli_reaches_managed_session() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("hook.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = accept_with_deadline(&listener);
        let message = read_frame::<ClientMessage>(&mut stream).unwrap();
        assert_eq!(message.request_id, 1);
        assert_eq!(
            message.request,
            Request::AgentReport(AgentReport {
                session: SessionId(7),
                capability: [0xab; 32],
                sequence: Some(1),
                update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy),
            })
        );
        write_frame(
            &mut stream,
            &ServerMessage::Response {
                request_id: 1,
                response: Response::Ok,
            },
        )
        .unwrap();
    });
    let output = isolated_command(&root)
        .args([
            "--json",
            "report",
            "activity",
            "--state",
            "busy",
            "--sequence",
            "1",
        ])
        .env("OVRCR_HOOK_SOCKET", &socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32))
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn agent_hook_cli_timeout_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("hook.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = accept_with_deadline(&listener);
        let _ = read_frame::<ClientMessage>(&mut stream).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            std::thread::park_timeout(Duration::from_millis(5));
        }
    });
    let started = Instant::now();
    let output = isolated_command(&root)
        .args(["report", "activity", "--state", "busy"])
        .env("OVRCR_HOOK_SOCKET", &socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32))
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    server.join().unwrap();
    assert!(!output.status.success());
    assert!(elapsed < Duration::from_secs(2));
    assert!(output.stdout.is_empty());

    let mut command = isolated_command(&root);
    command
        .args(["report", "claude", "--stdin-json"])
        .env("OVRCR_HOOK_SOCKET", &socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn().unwrap();
    let held_open = child.stdin.take().unwrap();
    let output = child.wait_with_output().unwrap();
    drop(held_open);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

fn accept_with_deadline(
    listener: &std::os::unix::net::UnixListener,
) -> (UnixStream, std::os::unix::net::SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok(pair) => return pair,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "listener did not receive hook report"
                );
                std::thread::park_timeout(Duration::from_millis(5));
            }
            Err(error) => panic!("accept hook report: {error}"),
        }
    }
}

#[test]
fn resource_aliases_are_visible_and_parse_as_commands() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        &["projects", "--help"][..],
        &["project", "create", "--help"],
        &["project", "delete", "--help"],
        &["workspaces", "delete", "--help"],
        &["terminals", "create", "--help"],
    ] {
        let output = isolated_command(&root).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn terminal_workspace_filter_requires_project() {
    let root = tempfile::tempdir().unwrap();
    let output = isolated_command(&root)
        .args(["terminal", "list", "--workspace", "work"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--project"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pause_resume_cli_requires_id() {
    let root = tempfile::tempdir().unwrap();
    for command in ["pause", "resume"] {
        let missing = isolated_command(&root).args([command]).output().unwrap();
        assert_eq!(missing.status.code(), Some(2), "{command} without ID");
        assert!(
            String::from_utf8_lossy(&missing.stderr)
                .contains("required arguments were not provided"),
            "{command} missing-ID diagnostic: {}",
            String::from_utf8_lossy(&missing.stderr)
        );
        assert!(!root.path().join("server.sock").exists());

        let nonnumeric = isolated_command(&root)
            .args([command, "not-a-number"])
            .output()
            .unwrap();
        assert_eq!(nonnumeric.status.code(), Some(2), "{command} nonnumeric ID");
        assert!(
            String::from_utf8_lossy(&nonnumeric.stderr).contains("invalid value"),
            "{command} nonnumeric diagnostic: {}",
            String::from_utf8_lossy(&nonnumeric.stderr)
        );
        assert!(!root.path().join("server.sock").exists());
    }
}

#[test]
fn offline_project_inspection_reads_and_sorts_the_registry_without_starting_server() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("config.toml"),
        r#"
[[projects]]
name = "zeta"
repo = "/repos/zeta"
workspace_root = "/workspaces/zeta"

[[projects]]
name = "alpha"
repo = "/repos/alpha"
workspace_root = "/workspaces/alpha"
"#,
    )
    .unwrap();

    let output = isolated_command(&root)
        .args(["--json", "project", "list"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([
            {
                "name": "alpha",
                "repo": "/repos/alpha",
                "workspace_root": "/workspaces/alpha",
                "workspace_count": 0
            },
            {
                "name": "zeta",
                "repo": "/repos/zeta",
                "workspace_root": "/workspaces/zeta",
                "workspace_count": 0
            }
        ])
    );
    assert!(!root.path().join("server.sock").exists());
}

#[test]
fn offline_json_commands_keep_errors_structured_and_do_not_start_server() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
[[projects]]
name = "alpha"
repo = "/repos/alpha"
workspace_root = "/workspaces/alpha"

[[projects.workspaces]]
name = "one"
path = "/workspaces/alpha/one"
branch = "feature/one"
"#,
    )
    .unwrap();

    for args in [
        &["project", "get", "missing", "--json"][..],
        &["workspace", "list", "--project", "missing", "--json"],
        &[
            "workspace",
            "get",
            "--project",
            "alpha",
            "--name",
            "missing",
            "--json",
        ],
        &[
            "terminal",
            "list",
            "--project",
            "alpha",
            "--workspace",
            "missing",
            "--json",
        ],
        &["terminal", "read", "99", "--json"],
        &["terminal", "send", "99", "--text", "hello", "--json"],
        &["terminal", "close", "99", "--json"],
        &["pause", "99", "--json"],
        &["resume", "99", "--json"],
    ] {
        let output = isolated_command(&root).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {:?}", output.stdout);
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "NotFound", "{args:?}: {error}");
        assert!(!root.path().join("server.sock").exists(), "{args:?}");
    }
}

#[test]
fn malformed_offline_registry_is_reported_without_replacing_it() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    let malformed = "this is not = valid TOML [[[";
    std::fs::write(&config, malformed).unwrap();

    let output = isolated_command(&root)
        .args(["project", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "Internal");
    assert_eq!(std::fs::read_to_string(config).unwrap(), malformed);
    assert!(!root.path().join("server.sock").exists());
}

#[test]
fn max_lines_must_be_positive_and_legacy_offline_json_is_valid() {
    let root = tempfile::tempdir().unwrap();
    for value in ["0", "-1"] {
        let output = isolated_command(&root)
            .args(["terminal", "read", "1", "--max-lines", value])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{value}");
        assert!(!root.path().join("server.sock").exists());
    }

    let listed = isolated_command(&root)
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&listed.stdout).unwrap(),
        serde_json::json!([])
    );

    let shutdown = isolated_command(&root)
        .args(["shutdown", "--json"])
        .output()
        .unwrap();
    assert!(shutdown.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&shutdown.stdout).unwrap(),
        serde_json::json!({ "ok": true })
    );
    assert!(!root.path().join("server.sock").exists());
}

#[test]
fn new_and_existing_branch_flags_are_exclusive() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args([
            "workspace",
            "create",
            "--project",
            "fixture",
            "--name",
            "work",
            "--new-branch",
            "feature/new",
            "--base",
            "main",
            "--branch",
            "feature/existing",
        ])
        .env("OVRCR_CONFIG", root.path().join("config.toml"))
        .env("OVRCR_SOCKET", root.path().join("server.sock"))
        .output()
        .expect("run compiled ovrcr binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("cannot be used together"),
        "expected an argument conflict diagnostic, got: {stderr}"
    );
}

#[test]
fn session_command_keeps_arguments_after_separator() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let workspaces = root.path().join("workspaces");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&workspaces).unwrap();
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
    let config = root.path().join("config.toml");
    let socket = root.path().join("server.sock");
    let envs = [
        ("OVRCR_CONFIG", config.as_os_str()),
        ("OVRCR_SOCKET", socket.as_os_str()),
    ];
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let run = |args: &[&str]| {
        let mut command = Command::new(bin);
        command.args(args);
        for &(key, value) in &envs {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    let mut cleanup = CleanupGuard::new(bin, &config, &socket);
    run(&[
        "project",
        "add",
        "fixture",
        repo.to_str().unwrap(),
        "--workspace-root",
        workspaces.to_str().unwrap(),
    ]);
    run(&[
        "workspace",
        "create",
        "--project",
        "fixture",
        "--name",
        "work",
        "--new-branch",
        "feature/test",
        "--base",
        "main",
    ]);
    cleanup.capture_live_process_groups(&socket);
    let created = run(&[
        "new",
        "--project",
        "fixture",
        "--workspace",
        "work",
        "--name",
        "args",
        "--",
        "sh",
        "-c",
        "printf '%s\\n' \"$0\" \"$@\"",
        "agent",
        "--json",
    ]);
    let id: u64 = String::from_utf8_lossy(&created.stdout)
        .trim()
        .parse()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !socket.exists() && Instant::now() < deadline {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut exited = false;
    while Instant::now() < deadline {
        let mut control = UnixStream::connect(&socket).unwrap();
        write_frame(
            &mut control,
            &ClientMessage {
                request_id: 3,
                request: Request::List,
            },
        )
        .unwrap();
        if let ServerMessage::Response {
            response: Response::Hierarchy(snapshot),
            ..
        } = read_frame::<ServerMessage>(&mut control).unwrap()
        {
            let is_exited = snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|session| {
                    session.id == SessionId(id)
                        && matches!(session.phase, SessionPhase::Exited { .. })
                });
            if is_exited {
                exited = true;
                break;
            }
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(exited, "session {id} did not reach Exited before deadline");
    cleanup.capture_live_process_groups(&socket);
    let mut dashboard = UnixStream::connect(&socket).unwrap();
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
                session: SessionId(id),
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let screen = loop {
        if let ServerMessage::Response {
            request_id: 2,
            response: Response::Screen { bytes, .. },
            ..
        } = read_frame::<ServerMessage>(&mut dashboard).unwrap()
        {
            break bytes;
        }
    };
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(&screen);
    let contents = parser.screen().contents();
    let received = contents
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let text = String::from_utf8_lossy(&screen);
    assert_eq!(
        received,
        ["agent", "--json"],
        "retained screen did not contain exact argv: {text:?}"
    );
    cleanup.confirm();
}

struct CleanupGuard<'a> {
    bin: &'a str,
    config: &'a std::path::Path,
    socket: &'a std::path::Path,
    pgids: Vec<libc::pid_t>,
    cleaned: bool,
}

impl<'a> CleanupGuard<'a> {
    fn new(bin: &'a str, config: &'a std::path::Path, socket: &'a std::path::Path) -> Self {
        Self {
            bin,
            config,
            socket,
            pgids: Vec::new(),
            cleaned: false,
        }
    }

    fn capture_live_process_groups(&mut self, socket: &std::path::Path) {
        let mut stream = UnixStream::connect(socket).unwrap();
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 4,
                request: Request::List,
            },
        )
        .unwrap();
        let ServerMessage::Response {
            response: Response::Hierarchy(snapshot),
            ..
        } = read_frame::<ServerMessage>(&mut stream).unwrap()
        else {
            panic!("unexpected list response")
        };
        for session in snapshot
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
        {
            if let Some(pid) = session.pid {
                let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
                if pgid > 1 {
                    self.pgids.push(pgid);
                }
            }
        }
    }

    fn cleanup(&mut self) -> Result<(), String> {
        let output = Command::new(self.bin)
            .args(["shutdown", "--kill"])
            .env("OVRCR_CONFIG", self.config)
            .env("OVRCR_SOCKET", self.socket)
            .output()
            .map_err(|error| format!("spawn cleanup shutdown: {error}"))?;
        let mut failures = Vec::new();
        if !output.status.success() {
            failures.push(format!(
                "cleanup shutdown failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.socket.exists() && Instant::now() < deadline {
            std::thread::park_timeout(Duration::from_millis(10));
        }
        if self.socket.exists() {
            failures.push("server socket remained after cleanup".into());
        }
        for pgid in &self.pgids {
            if unsafe { libc::kill(-*pgid, 0) } != -1 {
                failures.push(format!("managed process group {pgid} remained"));
            }
        }
        if failures.is_empty() {
            self.cleaned = true;
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn confirm(&mut self) {
        if let Err(error) = self.cleanup() {
            panic!("cleanup confirmation failed: {error}");
        }
    }
}

impl Drop for CleanupGuard<'_> {
    fn drop(&mut self) {
        if !self.cleaned
            && let Err(error) = self.cleanup()
        {
            eprintln!("cleanup confirmation failed during unwind: {error}");
        }
    }
}
