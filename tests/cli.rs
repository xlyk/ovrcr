use ovrcr::protocol::{ClientMessage, Request, Response, ServerMessage, read_frame, write_frame};
use ovrcr::session::{SessionId, SessionPhase};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Child, Command, ExitStatus, Output, Stdio};

use std::time::{Duration, Instant};

fn isolated_command(root: &tempfile::TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
    command
        .env("OVRCR_CONFIG", root.path().join("config.toml"))
        .env("OVRCR_SOCKET", root.path().join("server.sock"));
    command
}

struct CapturedChild {
    child: Child,
    stdout: File,
    stderr: File,
}

fn spawn_captured(mut command: Command) -> io::Result<CapturedChild> {
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    command
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    Ok(CapturedChild {
        child: command.spawn()?,
        stdout,
        stderr,
    })
}

fn captured_bytes(file: &File) -> io::Result<Vec<u8>> {
    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(65_536).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn wait_captured(captured: &mut CapturedChild, deadline: Instant) -> io::Result<Output> {
    let status = match wait_child_bounded(&mut captured.child, deadline) {
        Ok(status) => status,
        Err(error) => {
            let stdout = captured_bytes(&captured.stdout).unwrap_or_default();
            let stderr = captured_bytes(&captured.stderr).unwrap_or_default();
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; stdout={:?}; stderr={:?}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                ),
            ));
        }
    };
    Ok(Output {
        status,
        stdout: captured_bytes(&captured.stdout)?,
        stderr: captured_bytes(&captured.stderr)?,
    })
}

fn run_cli_bounded(command: Command) -> io::Result<Output> {
    let mut captured = spawn_captured(command)?;
    wait_captured(&mut captured, Instant::now() + Duration::from_secs(5))
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
    let repo = root.path().join("repo");
    let workspaces = root.path().join("workspaces");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&workspaces).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "OVRCR Tests"],
        vec!["config", "user.email", "tests@example.invalid"],
    ] {
        let mut command = Command::new("git");
        command.args(args).current_dir(&repo);
        let output = run_cli_bounded(command).unwrap();
        assert!(
            output.status.success(),
            "git setup: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::write(repo.join("README"), "fixture\n").unwrap();
    let mut command = Command::new("git");
    command.args(["add", "README"]).current_dir(&repo);
    let output = run_cli_bounded(command).unwrap();
    assert!(
        output.status.success(),
        "git add: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut command = Command::new("git");
    command.args(["commit", "-m", "initial"]).current_dir(&repo);
    let output = run_cli_bounded(command).unwrap();
    assert!(
        output.status.success(),
        "git commit: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let config = root.path().join("config.toml");
    let socket = root.path().join("server.sock");
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let mut cleanup = CleanupGuard::new(bin, &config, &socket);
    let run = |args: &[&str]| {
        let mut command = isolated_command(&root);
        command.args(args);
        let output = run_cli_bounded(command).unwrap();
        assert!(
            output.status.success(),
            "{args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        output
    };
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
        "hooks",
        "--new-branch",
        "feature/hooks",
        "--base",
        "main",
    ]);
    cleanup.capture_live_process_groups(&socket);

    let marker = root.path().join("hook-child.marker");
    let report_stdout = root.path().join("hook-report.stdout");
    let script = r#"printf CHILD_READY > "$2"; "$1" --json report activity --state busy --sequence 1 > "$3" && printf HOOK_DONE >> "$2"; while IFS= read -r line; do :; done"#;
    let created = run(&[
        "new",
        "--project",
        "fixture",
        "--workspace",
        "hooks",
        "--name",
        "agent-hook",
        "--",
        "sh",
        "-c",
        script,
        "hook-child",
        bin,
        marker.to_str().unwrap(),
        report_stdout.to_str().unwrap(),
    ]);
    let id: u64 = String::from_utf8_lossy(&created.stdout)
        .trim()
        .parse()
        .unwrap();
    cleanup.capture_live_process_groups(&socket);

    let ready_deadline = Instant::now() + Duration::from_secs(3);
    while !marker.exists() && Instant::now() < ready_deadline {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        marker.exists(),
        "managed hook child did not invoke reporter"
    );
    let activity_deadline = Instant::now() + Duration::from_secs(3);
    let mut observed_busy = false;
    while Instant::now() < activity_deadline {
        if let Ok(Response::Hierarchy(snapshot)) = cli_request(&socket, Request::List) {
            observed_busy = snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|session| {
                    session.id == SessionId(id)
                        && matches!(session.phase, SessionPhase::Running)
                        && session.activity == ovrcr::session::AgentActivity::Busy
                });
            if observed_busy {
                break;
            }
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(observed_busy, "managed server never observed Busy activity");
    let completion_deadline = Instant::now() + Duration::from_secs(3);
    while std::fs::read_to_string(&marker).unwrap_or_default() != "CHILD_READYHOOK_DONE"
        && Instant::now() < completion_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap(),
        "CHILD_READYHOOK_DONE"
    );
    assert!(std::fs::read(&report_stdout).unwrap().is_empty());
    cleanup.confirm();
}

#[test]
fn agent_hook_cli_timeout_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("hook.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = accept_with_deadline(&listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let _ = read_frame::<ClientMessage>(&mut stream).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            std::thread::park_timeout(Duration::from_millis(5));
        }
    });
    let started = Instant::now();
    let mut generic = isolated_command(&root);
    generic
        .args(["--json", "report", "activity", "--state", "busy"])
        .env("OVRCR_HOOK_SOCKET", &socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32));
    let output = run_cli_bounded(generic).unwrap();
    let status = output.status;
    let elapsed = started.elapsed();
    server.join().unwrap();
    assert!(!status.success());
    assert!(output.stdout.is_empty());
    assert!(elapsed < Duration::from_secs(2));

    let mut command = isolated_command(&root);
    command
        .args(["report", "claude", "--stdin-json"])
        .env("OVRCR_HOOK_SOCKET", &socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32))
        .stdin(Stdio::piped());
    let started = Instant::now();
    let mut captured = spawn_captured(command).unwrap();
    let held_open = captured.child.stdin.take().unwrap();
    let output = wait_captured(&mut captured, Instant::now() + Duration::from_secs(2))
        .expect("adapter did not exit within watchdog");
    drop(held_open);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn agent_hook_cli_deadline_spans_stdin_and_dripped_response() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("drip.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = accept_with_deadline(&listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let _ = read_frame::<ClientMessage>(&mut stream).unwrap();
        let mut frame = Vec::new();
        write_frame(
            &mut frame,
            &ServerMessage::Response {
                request_id: 1,
                response: Response::Error {
                    code: ovrcr::protocol::ErrorCode::Conflict,
                    message: "bounded fixture response".repeat(12),
                },
            },
        )
        .unwrap();
        for byte in frame {
            if stream.write_all(&[byte]).is_err() {
                break;
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    });

    let mut command = isolated_command(&root);
    command
        .args(["report", "claude", "--stdin-json"])
        .env("OVRCR_HOOK_SOCKET", &socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32))
        .stdin(Stdio::piped());
    let started = Instant::now();
    let mut captured = spawn_captured(command).unwrap();
    let mut stdin = captured.child.stdin.take().unwrap();
    stdin
        .write_all(br#"{"hook_event_name":"Stop","session_id":"root-1"}"#)
        .unwrap();
    let input_deadline = Instant::now() + Duration::from_millis(900);
    while Instant::now() < input_deadline {
        std::thread::park_timeout(Duration::from_millis(5));
    }
    drop(stdin);
    let output = wait_captured(&mut captured, Instant::now() + Duration::from_secs(2))
        .expect("dripped response helper did not exit within watchdog");
    let elapsed = started.elapsed();
    server.join().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        elapsed >= Duration::from_millis(900) && elapsed < Duration::from_millis(1750),
        "combined deadline took {elapsed:?}"
    );
}

#[test]
fn context_helper_reports_from_managed_pty() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let workspaces = root.path().join("workspaces");
    setup_git_fixture(&repo, &workspaces);
    let config = root.path().join("config.toml");
    let socket = root.path().join("server.sock");
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let mut cleanup = CleanupGuard::new(bin, &config, &socket);
    let run = |args: &[&str]| {
        let mut command = isolated_command(&root);
        command.args(args);
        let output = run_cli_bounded(command).unwrap();
        assert!(
            output.status.success(),
            "{args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        output
    };
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
        "hooks",
        "--new-branch",
        "feature/hooks",
        "--base",
        "main",
    ]);
    cleanup.capture_live_process_groups(&socket);

    let generic_stdout = root.path().join("context.stdout");
    let claude_stdout = root.path().join("claude-context.stdout");
    let result_log = root.path().join("context-results");
    let script = r#"
count=0
while IFS= read -r line; do
    if [ "$count" -eq 0 ]; then
        mode=context
        output="$2"
    else
        mode=claude-context
        output="$3"
    fi
    printf '%s' "$line" | "$1" report "$mode" --stdin-json > "$output"
    status=$?
    printf '%s:%s\n' "$mode" "$status" >> "$4"
    count=$((count + 1))
done
"#;
    let mut created = isolated_command(&root);
    created.args([
        "new",
        "--project",
        "fixture",
        "--workspace",
        "hooks",
        "--name",
        "context-helper",
        "--",
    ]);
    created.args([
        "sh",
        "-c",
        script,
        "context-helper",
        bin,
        generic_stdout.to_str().unwrap(),
        claude_stdout.to_str().unwrap(),
        result_log.to_str().unwrap(),
    ]);
    let created = run_cli_bounded(created).unwrap();
    assert!(
        created.status.success(),
        "create helper shell: stdout={} stderr={}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let id: u64 = String::from_utf8_lossy(&created.stdout)
        .trim()
        .parse()
        .unwrap();
    cleanup.capture_live_process_groups(&socket);
    let mut dashboard = select_session(&socket, SessionId(id));

    let generic = br#"{"source":"generic","model":"generic-model","conversation":"generic-conversation","used_tokens":12,"capacity_tokens":100}"#;
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            Request::Input {
                session: SessionId(id),
                bytes: [generic.as_slice(), b"\n"].concat(),
            },
        )
        .unwrap(),
        Response::Ok
    );
    let generic_ack_deadline = Instant::now() + Duration::from_secs(3);
    while std::fs::read_to_string(&result_log).unwrap_or_default() != "context:0\n"
        && Instant::now() < generic_ack_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&result_log).unwrap(), "context:0\n");
    let generic_state_deadline = Instant::now() + Duration::from_secs(3);
    let mut generic_state = None;
    while Instant::now() < generic_state_deadline {
        if let Ok(Response::Hierarchy(snapshot)) = cli_request(&socket, Request::List)
            && let Some(session) = snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .find(|session| session.id == SessionId(id))
            && session.context_usage.is_some()
        {
            generic_state = session.context_usage.clone();
            break;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let generic_state = generic_state.expect("server never observed generic context");
    assert_eq!(
        generic_state.report.source,
        ovrcr::context::ContextSource::Generic
    );
    assert_eq!(generic_state.report.model.as_deref(), Some("generic-model"));
    assert_eq!(
        generic_state.report.conversation.as_deref(),
        Some("generic-conversation")
    );
    assert_eq!(generic_state.report.used_tokens, Some(12));
    assert_eq!(generic_state.report.capacity_tokens, Some(100));

    let claude = br#"{"session_id":"fixture-conversation","model":{"id":"fixture-model"},"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":8500,"output_tokens":1200,"cache_creation_input_tokens":5000,"cache_read_input_tokens":2000}},"irrelevant":{"secret":"ignored"}}"#;
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            Request::Input {
                session: SessionId(id),
                bytes: [claude.as_slice(), b"\n"].concat(),
            },
        )
        .unwrap(),
        Response::Ok
    );
    let claude_ack_deadline = Instant::now() + Duration::from_secs(3);
    while std::fs::read_to_string(&result_log).unwrap_or_default()
        != "context:0\nclaude-context:0\n"
        && Instant::now() < claude_ack_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(&result_log).unwrap(),
        "context:0\nclaude-context:0\n"
    );
    assert!(std::fs::read(&generic_stdout).unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(&claude_stdout).unwrap(), "ctx 7%\n");
    let claude_state_deadline = Instant::now() + Duration::from_secs(3);
    let mut claude_state = None;
    while Instant::now() < claude_state_deadline {
        if let Ok(Response::Hierarchy(snapshot)) = cli_request(&socket, Request::List)
            && let Some(session) = snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .find(|session| session.id == SessionId(id))
            && session.context_usage.as_ref().is_some_and(|sample| {
                sample.report.source == ovrcr::context::ContextSource::ClaudeCodeStatusline
                    && sample.report.used_tokens == Some(15_500)
            })
        {
            claude_state = session.context_usage.clone();
            break;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let claude_state = claude_state.expect("server never observed Claude context replacement");
    assert_eq!(claude_state.report.model.as_deref(), Some("fixture-model"));
    assert_eq!(
        claude_state.report.conversation.as_deref(),
        Some("fixture-conversation")
    );
    assert_eq!(claude_state.report.used_tokens, Some(15_500));
    assert_eq!(claude_state.report.capacity_tokens, Some(200_000));
    cleanup.confirm();
}

#[test]
fn context_helper_invalid_input_has_no_effect() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let workspaces = root.path().join("workspaces");
    setup_git_fixture(&repo, &workspaces);
    let config = root.path().join("config.toml");
    let socket = root.path().join("server.sock");
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let mut cleanup = CleanupGuard::new(bin, &config, &socket);
    let run = |args: &[&str]| {
        let mut command = isolated_command(&root);
        command.args(args);
        let output = run_cli_bounded(command).unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
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
        "hooks",
        "--new-branch",
        "feature/hooks",
        "--base",
        "main",
    ]);
    cleanup.capture_live_process_groups(&socket);
    let result_log = root.path().join("context-results");
    let diagnostic_log = root.path().join("context-diagnostics");
    let script = r#"
count=0
while IFS= read -r line; do
    if [ "$count" -eq 0 ]; then mode=context; else mode=context; fi
    printf '%s' "$line" | "$1" report "$mode" --stdin-json > /dev/null 2> "$3"
    printf '%s:%s\n' "$mode" "$?" >> "$2"
    count=$((count + 1))
done
"#;
    let mut created = isolated_command(&root);
    created.args([
        "new",
        "--project",
        "fixture",
        "--workspace",
        "hooks",
        "--name",
        "context-invalid",
        "--",
    ]);
    created.args([
        "sh",
        "-c",
        script,
        "context-invalid",
        bin,
        result_log.to_str().unwrap(),
        diagnostic_log.to_str().unwrap(),
    ]);
    let created = run_cli_bounded(created).unwrap();
    assert!(created.status.success());
    let id: u64 = String::from_utf8_lossy(&created.stdout)
        .trim()
        .parse()
        .unwrap();
    cleanup.capture_live_process_groups(&socket);
    let mut dashboard = select_session(&socket, SessionId(id));
    let valid = br#"{"source":"generic","model":"stable-model","conversation":"stable-conversation","used_tokens":25,"capacity_tokens":100}"#;
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            Request::Input {
                session: SessionId(id),
                bytes: [valid.as_slice(), b"\n"].concat()
            }
        )
        .unwrap(),
        Response::Ok
    );
    let valid_deadline = Instant::now() + Duration::from_secs(3);
    while std::fs::read_to_string(&result_log).unwrap_or_default() != "context:0\n"
        && Instant::now() < valid_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let before = loop {
        if let Ok(Response::Hierarchy(snapshot)) = cli_request(&socket, Request::List)
            && let Some(session) = snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .find(|session| session.id == SessionId(id))
            && let Some(sample) = session.context_usage.clone()
        {
            break sample;
        }
        assert!(
            Instant::now() < valid_deadline,
            "valid context not observed"
        );
        std::thread::park_timeout(Duration::from_millis(10));
    };
    let invalid = br#"{"source":"generic","used_tokens":"secret-payload","capacity_tokens":100}"#;
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            Request::Input {
                session: SessionId(id),
                bytes: [invalid.as_slice(), b"\n"].concat()
            }
        )
        .unwrap(),
        Response::Ok
    );
    let invalid_deadline = Instant::now() + Duration::from_secs(3);
    while std::fs::read_to_string(&result_log).unwrap_or_default() != "context:0\ncontext:1\n"
        && Instant::now() < invalid_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(&result_log).unwrap(),
        "context:0\ncontext:1\n"
    );
    assert_eq!(
        std::fs::read_to_string(&diagnostic_log).unwrap(),
        "InvalidRequest: hook input invalid\n"
    );
    let after = match cli_request(&socket, Request::List).unwrap() {
        Response::Hierarchy(snapshot) => snapshot
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .find(|session| session.id == SessionId(id))
            .and_then(|session| session.context_usage.clone())
            .unwrap(),
        response => panic!("unexpected response: {response:?}"),
    };
    assert_eq!(after, before);
    cleanup.confirm();
}

#[test]
fn context_helper_missing_server_does_not_start_one() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("missing.sock");
    let config = root.path().join("missing.toml");
    let valid = br#"{"source":"generic","model":"missing-server-model","conversation":"missing-server-conversation","used_tokens":1,"capacity_tokens":100}"#;
    let mut command = isolated_command(&root);
    command
        .args(["report", "context", "--stdin-json"])
        .env("OVRCR_CONFIG", &config)
        .env("OVRCR_SOCKET", &socket)
        .env("OVRCR_HOOK_SOCKET", &socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32))
        .stdin(Stdio::piped());
    let started = Instant::now();
    let mut captured = spawn_captured(command).unwrap();
    let mut stdin = captured.child.stdin.take().unwrap();
    stdin.write_all(valid).unwrap();
    drop(stdin);
    let output = wait_captured(&mut captured, Instant::now() + Duration::from_secs(2)).unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"NotFound: hook server is unavailable\n");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!socket.exists());
    assert!(!config.exists());

    let incomplete_root = tempfile::tempdir().unwrap();
    let incomplete_socket = incomplete_root.path().join("incomplete.sock");
    let incomplete_config = incomplete_root.path().join("incomplete.toml");
    let mut incomplete = isolated_command(&incomplete_root);
    incomplete
        .args(["report", "context", "--stdin-json"])
        .env("OVRCR_CONFIG", &incomplete_config)
        .env("OVRCR_SOCKET", &incomplete_socket)
        .env("OVRCR_HOOK_SOCKET", &incomplete_socket)
        .env("OVRCR_SESSION_ID", "7")
        .env("OVRCR_HOOK_TOKEN", "ab".repeat(32))
        .stdin(Stdio::piped());
    let incomplete_started = Instant::now();
    let mut incomplete = spawn_captured(incomplete).unwrap();
    let held_open = incomplete.child.stdin.take().unwrap();
    let output = wait_captured(&mut incomplete, Instant::now() + Duration::from_secs(2)).unwrap();
    drop(held_open);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(incomplete_started.elapsed() < Duration::from_secs(2));
    assert!(!incomplete_socket.exists());
    assert!(!incomplete_config.exists());
}

fn setup_git_fixture(repo: &std::path::Path, workspaces: &std::path::Path) {
    std::fs::create_dir(repo).unwrap();
    std::fs::create_dir(workspaces).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "OVRCR Tests"],
        vec!["config", "user.email", "tests@example.invalid"],
    ] {
        let output = run_cli_bounded({
            let mut command = Command::new("git");
            command.args(args).current_dir(repo);
            command
        })
        .unwrap();
        assert!(
            output.status.success(),
            "git setup: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::write(repo.join("README"), "fixture\n").unwrap();
    for args in [vec!["add", "README"], vec!["commit", "-m", "initial"]] {
        let output = run_cli_bounded({
            let mut command = Command::new("git");
            command.args(args).current_dir(repo);
            command
        })
        .unwrap();
        assert!(
            output.status.success(),
            "git fixture: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn accept_with_deadline(
    listener: &std::os::unix::net::UnixListener,
) -> (UnixStream, std::os::unix::net::SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok((stream, address)) => {
                stream
                    .set_nonblocking(false)
                    .expect("make accepted fixture stream blocking");
                return (stream, address);
            }
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

fn cli_request(socket: &std::path::Path, request: Request) -> Result<Response, String> {
    let mut stream = UnixStream::connect(socket).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(250)))
        .map_err(|error| error.to_string())?;
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 1,
            request,
        },
    )
    .map_err(|error| error.to_string())?;
    match read_frame::<ServerMessage>(&mut stream).map_err(|error| error.to_string())? {
        ServerMessage::Response { response, .. } => Ok(response),
        ServerMessage::Event(_) => Err("server sent an event before response".into()),
    }
}

fn select_session(socket: &std::path::Path, session: SessionId) -> UnixStream {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    match read_frame::<ServerMessage>(&mut stream).unwrap() {
        ServerMessage::Response {
            request_id: 1,
            response: Response::Hierarchy(_),
        } => {}
        response => panic!("unexpected dashboard hello response: {response:?}"),
    }
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
    match read_frame::<ServerMessage>(&mut stream).unwrap() {
        ServerMessage::Response {
            request_id: 2,
            response: Response::Screen {
                session: selected, ..
            },
        } => assert_eq!(selected, session),
        response => panic!("unexpected select response: {response:?}"),
    }
    stream
}

fn dashboard_request(stream: &mut UnixStream, request: Request) -> Result<Response, String> {
    write_frame(
        stream,
        &ClientMessage {
            request_id: 3,
            request,
        },
    )
    .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if Instant::now() >= deadline {
            return Err("dashboard response deadline exceeded".into());
        }
        match read_frame::<ServerMessage>(stream).map_err(|error| error.to_string())? {
            ServerMessage::Response {
                request_id: 3,
                response,
            } => return Ok(response),
            ServerMessage::Event(_) | ServerMessage::Response { .. } => {}
        }
    }
}

fn wait_child_bounded(child: &mut Child, deadline: Instant) -> std::io::Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "child exceeded test watchdog",
            ));
        }
        std::thread::park_timeout(Duration::from_millis(5));
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
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
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
                if pgid > 1 && !self.pgids.contains(&pgid) {
                    self.pgids.push(pgid);
                }
            }
        }
    }

    fn cleanup(&mut self) -> Result<(), String> {
        let mut shutdown = Command::new(self.bin)
            .args(["shutdown", "--kill"])
            .env("OVRCR_CONFIG", self.config)
            .env("OVRCR_SOCKET", self.socket)
            .env("OVRCR_KILL_GRACE_MS", "200")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("spawn cleanup shutdown: {error}"))?;
        let mut failures = Vec::new();
        let shutdown_deadline = Instant::now() + Duration::from_secs(3);
        let mut shutdown_status = None;
        while Instant::now() < shutdown_deadline {
            match shutdown.try_wait() {
                Ok(Some(status)) => {
                    shutdown_status = Some(status);
                    break;
                }
                Ok(None) => std::thread::park_timeout(Duration::from_millis(10)),
                Err(error) => {
                    failures.push(format!("poll cleanup shutdown: {error}"));
                    break;
                }
            }
        }
        if shutdown_status.is_none() {
            let _ = shutdown.kill();
            let _ = shutdown.wait();
        } else if !shutdown_status.is_some_and(|status| status.success()) {
            failures.push("cleanup shutdown failed".into());
        }
        for pgid in &self.pgids {
            if unsafe { libc::kill(-*pgid, 0) } != -1 {
                unsafe {
                    libc::kill(-*pgid, libc::SIGKILL);
                }
                let group_deadline = Instant::now() + Duration::from_secs(2);
                while unsafe { libc::kill(-*pgid, 0) } != -1 && Instant::now() < group_deadline {
                    std::thread::park_timeout(Duration::from_millis(10));
                }
                if unsafe { libc::kill(-*pgid, 0) } != -1 {
                    failures.push(format!("managed process group {pgid} remained"));
                }
            }
        }
        let socket_deadline = Instant::now() + Duration::from_secs(3);
        while self.socket.exists() && Instant::now() < socket_deadline {
            std::thread::park_timeout(Duration::from_millis(10));
        }
        if self.socket.exists() {
            failures.push("server socket remained after cleanup".into());
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
