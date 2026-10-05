//! Hidden Claude quota probe. Fixtures only; no Claude account.
#[path = "support/live.rs"]
mod live;

use ovrcr::protocol::*;
use ovrcr::session::TerminalSize;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

fn settings_document(fixture: &live::Live) -> std::path::PathBuf {
    fixture.config.with_file_name("dashboard.toml")
}

fn attach(fixture: &live::Live) -> std::os::unix::net::UnixStream {
    let mut socket = ovrcr::protocol::connect_server(&fixture.socket).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame(
        &mut socket,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    match read_frame::<ServerMessage>(&mut socket).unwrap() {
        ServerMessage::Response {
            response: Response::Hierarchy(_),
            ..
        } => socket,
        other => panic!("unexpected greeting: {other:?}"),
    }
}

fn quota_until(
    socket: &mut std::os::unix::net::UnixStream,
    what: &str,
    mut done: impl FnMut(&QuotaSnapshot) -> bool,
) -> QuotaSnapshot {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(std::time::Instant::now() < deadline, "{what}");
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) =
            read_frame::<ServerMessage>(socket).unwrap()
            && done(&snapshot)
        {
            return *snapshot;
        }
    }
}

fn install_probe_claude(fixture: &live::Live) {
    let state = fixture
        .claude
        .command
        .parent()
        .unwrap()
        .join("claude-auth-status.json");
    let script =
        include_str!("support/claude_probe.sh").replace("@STATE@", &state.display().to_string());
    std::fs::write(&fixture.claude.command, script).unwrap();
    std::fs::set_permissions(
        &fixture.claude.command,
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
}

struct ProbeRun {
    fixture: live::Live,
    log: std::path::PathBuf,
}

fn start_probe(mode: &str, extra: &[(&str, &std::ffi::OsStr)]) -> ProbeRun {
    let fixture = live::Live::idle().bounded();
    install_probe_claude(&fixture);
    std::fs::write(
        settings_document(&fixture),
        "[quota.claude]\nprobe = true\n",
    )
    .unwrap();
    let log = fixture.root.path().join("probe.log");
    let trust = fixture.root.path().join("claude-trust");
    let pid = fixture.root.path().join("probe.pid");
    let mode_owned = mode.to_string();
    let mut env: Vec<(&str, &std::ffi::OsStr)> = vec![
        ("OVRCR_CLAUDE_FIXTURE_LOG", log.as_os_str()),
        (
            "OVRCR_CLAUDE_FIXTURE_MODE",
            std::ffi::OsStr::new(mode_owned.as_str()),
        ),
        ("OVRCR_CLAUDE_FIXTURE_TRUST", trust.as_os_str()),
        ("OVRCR_CLAUDE_FIXTURE_PID", pid.as_os_str()),
    ];
    env.extend_from_slice(extra);
    let server_log = fixture.root.path().join("probe-server.log");
    fixture.start_binary_logged(&env, &server_log);
    ProbeRun { fixture, log }
}

fn probe_lines(log: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn probe_count(log: &std::path::Path) -> usize {
    probe_lines(log)
        .iter()
        .filter(|line| line.starts_with("PROBE "))
        .count()
}

fn last_probe_pid(log: &std::path::Path) -> u32 {
    probe_lines(log)
        .iter()
        .filter_map(|line| line.strip_prefix("pid="))
        .next_back()
        .unwrap()
        .parse()
        .unwrap()
}

fn pid_alive(pid: u32) -> bool {
    // `/proc` is Linux-only. `kill(pid, 0)` is the same check on the macOS CI runner.
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

fn wait_probes(log: &std::path::Path, count: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while probe_count(log) < count || pid_alive(last_probe_pid(log)) {
        assert!(
            std::time::Instant::now() < deadline,
            "wanted {count} finished probes, log:\n{}",
            probe_lines(log).join("\n")
        );
        std::thread::park_timeout(Duration::from_millis(20));
    }
    // The runner treats a refresh that arrives while a probe is exiting as
    // already answered. Wait until that clear has happened.
    std::thread::park_timeout(Duration::from_millis(100));
}

#[test]
fn claude_probe_off_spawns_nothing_and_waits_for_a_managed_session() {
    let waiting = QuotaSnapshot::default().claude;
    assert_eq!(waiting.state, QuotaState::Checking);
    assert_eq!(waiting.reason.as_deref(), Some(CLAUDE_WAITING));
    let fixture = live::Live::idle().bounded();
    install_probe_claude(&fixture);
    let log = fixture.root.path().join("probe.log");
    // No credentials in the private HOME: the independent account reader has
    // nothing to read, so the signed-in auth fixture keeps the waiting row.
    assert!(
        !fixture
            .root
            .path()
            .join(".claude/.credentials.json")
            .exists()
    );
    fixture.start_binary_env(&[("OVRCR_CLAUDE_FIXTURE_LOG", log.as_os_str())]);
    let mut socket = attach(&fixture);
    socket
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    while let Ok(message) = read_frame::<ServerMessage>(&mut socket) {
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) = message {
            assert_eq!(snapshot.claude.state, QuotaState::Checking);
            assert_eq!(snapshot.claude.reason.as_deref(), Some(CLAUDE_WAITING));
        }
    }
    assert!(matches!(
        fixture.request(Request::RefreshQuota {
            provider: Some(QuotaProvider::Claude),
        }),
        Response::Error { code: ErrorCode::InvalidRequest, message } if message.contains(CLAUDE_WAITING)
    ));
    assert_eq!(probe_count(&log), 0, "probe ran without consent");
    assert!(
        !fixture
            .socket
            .parent()
            .unwrap()
            .join("claude-quota-probe")
            .exists()
    );
}

#[test]
fn claude_probe_does_not_run_before_a_dashboard_attaches() {
    let run = start_probe("callback", &[]);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        probe_count(&run.log),
        0,
        "{}",
        probe_lines(&run.log).join("\n")
    );
    let mut socket = attach(&run.fixture);
    let snapshot = quota_until(&mut socket, "probe did not publish", |quota| {
        quota.claude.state == QuotaState::Current
    });
    assert!(matches!(
        snapshot.claude.source,
        Some(QuotaSource::Probe { .. })
    ));
    assert_eq!(
        probe_count(&run.log),
        1,
        "{}",
        probe_lines(&run.log).join("\n")
    );
    let text = probe_lines(&run.log).join("\n");
    assert!(text.contains("arg=haiku"), "{text}");
    assert!(text.contains("arg=dontAsk"), "{text}");
    assert!(text.contains("arg=Reply with OK."), "{text}");
    assert!(text.contains("arg=--setting-sources"), "{text}");
    assert!(text.contains("arg=--tools"), "{text}");
    assert!(text.contains("\"statusLine\""), "{text}");
    assert!(!text.contains("hooks"), "{text}");
    assert!(text.contains("entries=0"), "{text}");
    assert!(!pid_alive(last_probe_pid(&run.log)));
    assert!(snapshot.claude.checked_unix_ms.is_none());
    assert!(snapshot.claude.observed_unix_ms.is_some());
}

#[test]
fn claude_probe_trust_dialog_is_answered_once() {
    let run = start_probe("trust", &[]);
    let mut socket = attach(&run.fixture);
    quota_until(&mut socket, "first probe did not report", |quota| {
        quota.claude.state == QuotaState::Current
    });
    wait_probes(&run.log, 1);
    assert_eq!(
        run.fixture.request(Request::RefreshQuota {
            provider: Some(QuotaProvider::Claude),
        }),
        Response::Ok
    );
    wait_probes(&run.log, 2);
    let lines = probe_lines(&run.log);
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "DIALOG")
            .count(),
        1,
        "{lines:?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "NODIALOG")
            .count(),
        1,
        "{lines:?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "ANSWERED")
            .count(),
        1,
        "{lines:?}"
    );
}

#[test]
fn claude_probe_waits_for_usage_after_the_startup_callback() {
    let run = start_probe("cold-start", &[]);
    let mut socket = attach(&run.fixture);
    let snapshot = quota_until(
        &mut socket,
        "cold startup never reached current usage",
        |quota| {
            assert_ne!(
                quota.claude.state,
                QuotaState::Unsupported,
                "a startup callback cannot classify a subscription"
            );
            quota.claude.state == QuotaState::Current
        },
    );
    assert!(
        probe_lines(&run.log)
            .iter()
            .any(|line| line == "COLD_CALLBACK_ACKNOWLEDGED")
    );
    assert_eq!(snapshot.claude.windows.len(), 2);
    assert_eq!(
        snapshot.claude.windows[0].remaining_basis_points(),
        Some(8800)
    );
    assert!(matches!(
        snapshot.claude.source,
        Some(QuotaSource::Probe { .. })
    ));
    wait_probes(&run.log, 1);
    assert_eq!(
        probe_count(&run.log),
        1,
        "startup callbacks restarted the probe"
    );
    assert!(!pid_alive(last_probe_pid(&run.log)));
}

#[test]
fn claude_probe_without_rate_limits_is_unavailable() {
    let run = start_probe("no-limits", &[]);
    let mut socket = attach(&run.fixture);
    let snapshot = quota_until(
        &mut socket,
        "missing rate_limits was not classified",
        |quota| {
            quota.claude.state == QuotaState::Unavailable
                && quota.claude.reason.as_deref() == Some(PROBE_MISSING_USAGE)
        },
    );
    assert_eq!(snapshot.claude.reason.as_deref(), Some(PROBE_MISSING_USAGE));
    assert!(matches!(
        snapshot.claude.source,
        Some(QuotaSource::Probe { .. })
    ));
}

#[test]
fn claude_probe_that_exits_is_unavailable() {
    let run = start_probe("exit", &[]);
    let mut socket = attach(&run.fixture);
    let snapshot = quota_until(&mut socket, "early exit was not classified", |quota| {
        quota.claude.reason.as_deref() == Some(PROBE_EXITED)
    });
    assert_eq!(snapshot.claude.state, QuotaState::Unavailable);
    assert!(matches!(
        snapshot.claude.source,
        Some(QuotaSource::Probe { .. })
    ));
}

#[test]
fn claude_probe_silent_for_sixty_seconds_times_out_and_the_process_is_gone() {
    assert_probe_deadline("silent", PROBE_TIMED_OUT);
}

#[test]
fn claude_probe_startup_without_usage_reaches_the_deadline_and_the_process_is_gone() {
    assert_probe_deadline("no-limits-hang", PROBE_MISSING_USAGE);
}

fn assert_probe_deadline(mode: &str, reason: &str) {
    let run = start_probe(mode, &[]);
    let started = std::time::Instant::now();
    let mut socket = attach(&run.fixture);
    socket
        .set_read_timeout(Some(Duration::from_secs(75)))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(75);
    let mut snapshot = None;
    while std::time::Instant::now() < deadline {
        if let Ok(ServerMessage::Event(ServerEvent::QuotaChanged(next))) =
            read_frame::<ServerMessage>(&mut socket)
            && next.claude.reason.as_deref() == Some(reason)
        {
            snapshot = Some(next.claude.clone());
            break;
        }
    }
    let snapshot = snapshot.expect("probe did not reach the deadline");
    assert!(
        started.elapsed() >= Duration::from_secs(60),
        "probe ended before its deadline"
    );
    assert_eq!(snapshot.state, QuotaState::Unavailable);
    let pid = std::fs::read_to_string(run.fixture.root.path().join("probe.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(!pid_alive(pid), "probe process {pid} still exists");
}

#[test]
fn claude_probe_cadence_and_refresh_cooldown_follow_the_fake_clock() {
    let fixture = live::Live::idle().bounded();
    install_probe_claude(&fixture);
    std::fs::write(
        settings_document(&fixture),
        "[quota.claude]\nprobe = true\n",
    )
    .unwrap();
    let log = fixture.root.path().join("probe.log");
    let clock = fixture.root.path().join("quota-clock");
    let start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    std::fs::write(&clock, format!("{start}\n")).unwrap();
    let mode = std::ffi::OsString::from("callback");
    fixture.start_binary_env(&[
        ("OVRCR_CLAUDE_FIXTURE_LOG", log.as_os_str()),
        ("OVRCR_CLAUDE_FIXTURE_MODE", mode.as_os_str()),
        ("OVRCR_QUOTA_CLOCK", clock.as_os_str()),
    ]);
    let mut socket = attach(&fixture);
    quota_until(&mut socket, "attach probe missing", |quota| {
        quota.claude.state == QuotaState::Current
    });
    wait_probes(&log, 1);
    assert_eq!(
        fixture.request(Request::RefreshQuota {
            provider: Some(QuotaProvider::Claude),
        }),
        Response::Ok
    );
    wait_probes(&log, 2);
    match fixture.request(Request::RefreshQuota {
        provider: Some(QuotaProvider::Claude),
    }) {
        Response::QuotaCooldown { remaining_ms } => {
            assert!((1..=30_000).contains(&remaining_ms), "{remaining_ms}");
        }
        other => panic!("cooldown did not hold: {other:?}"),
    }
    assert_eq!(probe_count(&log), 2);
    std::fs::write(&clock, format!("{}\n", start + 31_000)).unwrap();
    assert_eq!(
        fixture.request(Request::RefreshQuota {
            provider: Some(QuotaProvider::Claude),
        }),
        Response::Ok
    );
    wait_probes(&log, 3);
    std::fs::write(&clock, format!("{}\n", start + 31_000 + 29 * 60 * 1000)).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(probe_count(&log), 3, "cadence fired inside 30 minutes");
    std::fs::write(&clock, format!("{}\n", start + 31_000 + 31 * 60 * 1000)).unwrap();
    wait_probes(&log, 4);
}

const CREDENTIAL: &str = "ovrcr-fixture-credential-9f3a7c";

/// One bounded, task-owned HTTP request. Only the fake credential reaches it;
/// the fixture neither logs the bearer nor opens a real provider connection.
fn claude_usage_fixture(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api/oauth/usage", listener.local_addr().unwrap());
    let request = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "no Claude account read"
                    );
                    std::thread::park_timeout(Duration::from_millis(10));
                }
                Err(error) => panic!("loopback accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut reader = std::io::BufReader::new(&stream);
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            assert!(
                reader.read_line(&mut line).unwrap() > 0,
                "incomplete HTTP headers"
            );
            headers.push_str(&line);
            assert!(headers.len() <= 8192, "HTTP headers too large");
            if line == "\r\n" {
                break;
            }
        }
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("get /api/oauth/usage http/1.1\r\n"));
        assert!(
            headers
                .lines()
                .any(|line| { line == format!("authorization: bearer {CREDENTIAL}") }),
            "fixture bearer header absent"
        );
        assert!(
            headers
                .lines()
                .any(|line| line == "anthropic-beta: oauth-2025-04-20")
        );
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(reply.as_bytes()).unwrap();
    });
    (url, request)
}

#[test]
fn claude_probe_off_allows_an_account_read_without_spawning_a_probe() {
    let fixture = live::Live::idle().bounded();
    install_probe_claude(&fixture);
    std::fs::write(settings_document(&fixture), "[quota]\nenabled = false\n").unwrap();
    let credentials = fixture.root.path().join("claude-config");
    std::fs::create_dir(&credentials).unwrap();
    let path = credentials.join(".credentials.json");
    let bytes = format!("{{\"claudeAiOauth\":{{\"accessToken\":\"{CREDENTIAL}\"}}}}\n");
    std::fs::write(&path, &bytes).unwrap();
    let (usage, request) = claude_usage_fixture(
        r#"{"five_hour":{"utilization":42,"resets_at":"2099-01-01T00:00:00Z"},"seven_day":{"utilization":18,"resets_at":"2099-01-02T00:00:00Z"}}"#,
    );
    let log = fixture.root.path().join("probe.log");
    fixture.start_binary_env(&[
        ("CLAUDE_CONFIG_DIR", credentials.as_os_str()),
        ("OVRCR_QUOTA_CLAUDE_USAGE_URL", std::ffi::OsStr::new(&usage)),
        ("OVRCR_CLAUDE_FIXTURE_LOG", log.as_os_str()),
    ]);
    let mut socket = attach(&fixture);
    let snapshot = quota_until(&mut socket, "account allowance did not publish", |quota| {
        quota.claude.state == QuotaState::Current
    });
    request.join().expect("loopback account fixture completed");
    assert!(matches!(
        snapshot.claude.source,
        Some(QuotaSource::NativeProfile { .. })
    ));
    assert_eq!(snapshot.claude.reason, None);
    assert!(snapshot.claude.checked_unix_ms.is_some());
    assert_eq!(snapshot.claude.windows.len(), 2);
    for (label, used) in [("5h", 4200), ("7d", 1800)] {
        assert!(
            snapshot
                .claude
                .windows
                .iter()
                .any(|window| { window.label == label && window.used_basis_points == Some(used) })
        );
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), bytes);
    assert!(!format!("{snapshot:?}").contains(CREDENTIAL));
    assert_eq!(probe_count(&log), 0, "account read started a hidden probe");
    assert!(
        !fixture
            .socket
            .parent()
            .unwrap()
            .join("claude-quota-probe")
            .exists()
    );
    assert_eq!(snapshot.codex.state, QuotaState::Disabled);
    assert_eq!(snapshot.grok.state, QuotaState::Disabled);
}

#[test]
fn claude_probe_output_does_not_contain_a_fixture_credential() {
    let fixture = live::Live::isolated_home().bounded();
    install_probe_claude(&fixture);
    let document = settings_document(&fixture);
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(&document, "[quota.claude]\nprobe = true\n").unwrap();
    let home = fixture.home.clone().unwrap();
    let credentials = home.join(".claude");
    std::fs::create_dir_all(&credentials).unwrap();
    std::fs::write(
        credentials.join(".credentials.json"),
        format!("{{\"claudeAiOauth\":{{\"accessToken\":\"{CREDENTIAL}\"}}}}\n"),
    )
    .unwrap();
    let log = fixture.root.path().join("probe.log");
    let server_log = fixture.root.path().join("server.log");
    let (usage, request) = claude_usage_fixture("{}");
    fixture.start_binary_logged(
        &[
            ("OVRCR_CLAUDE_FIXTURE_LOG", log.as_os_str()),
            (
                "OVRCR_CLAUDE_FIXTURE_MODE",
                std::ffi::OsStr::new("callback"),
            ),
            ("OVRCR_QUOTA_CLAUDE_USAGE_URL", std::ffi::OsStr::new(&usage)),
        ],
        &server_log,
    );
    let mut socket = attach(&fixture);
    let mut seen = String::new();
    let snapshot = quota_until(&mut socket, "probe did not publish", |quota| {
        seen.push_str(&format!("{quota:?}\n"));
        quota.claude.state == QuotaState::Current
            && matches!(quota.claude.source, Some(QuotaSource::Probe { .. }))
    });
    request.join().expect("loopback account fixture completed");
    let server = std::fs::read_to_string(&server_log).unwrap_or_default();
    assert!(
        !format!("{seen}{server}{snapshot:?}").contains(CREDENTIAL),
        "credential reached Server output"
    );
}

#[test]
#[ignore = "task-owned managed Claude helper for the probe suppression test"]
fn claude_probe_managed_helper() {
    use std::io::{BufRead, Write};
    let expected = std::env::var("OVRCR_TEST_UUID").unwrap();
    println!("ADMISSION_READY");
    let _ = std::io::stdout().flush();
    for (index, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.unwrap();
        let payload = if line == "statusline-quota" {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            serde_json::json!({
                "session_id": expected,
                "cost": {"total_cost_usd": 0.25},
                "context_window": {"context_window_size": 100, "current_usage": {"input_tokens": 20, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}},
                "rate_limits": {
                    "five_hour": {"used_percentage": 42, "resets_at": now + 18_000},
                    "seven_day": {"used_percentage": 18, "resets_at": now + 604_800}
                }
            })
        } else if line == "root" {
            serde_json::json!({"hook_event_name":"SessionStart","source":"startup","session_id":expected,"agent_type":"fixture-root"})
        } else if line == "exit" {
            break;
        } else {
            panic!("unknown instruction {line}");
        };
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ovrcr"));
        if line == "statusline-quota" {
            command.args(["report", "claude-statusline", "--stdin-json"]);
        } else {
            command.args(["report", "claude", "--stdin-json"]);
        }
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&payload).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        println!("ADMISSION_CALLBACK={index}");
        let _ = std::io::stdout().flush();
    }
}

#[test]
fn a_live_managed_claude_session_suppresses_the_probe() {
    let run = start_probe("callback", &[]);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(probe_count(&run.log), 0);
    run.fixture.ready("feature/claude-probe");
    let native_dir = run.fixture.root.path().join("managed");
    std::fs::create_dir(&native_dir).unwrap();
    let native = native_dir.join("claude");
    std::fs::write(
        &native,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '2.1.285 (Claude Code)\\n'; exit 0; fi\nOVRCR_TEST_UUID=\"$2\" exec \"$OVRCR_TEST_EXECUTABLE\" --ignored --exact claude_probe_managed_helper --nocapture --quiet\n",
    )
    .unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let script = r#"stty -echo; export OVRCR_TEST_EXECUTABLE="$3"; "$1" agent run --provider claude -- "$2""#;
    let Response::CreatedSession(summary) =
        run.fixture
            .request(Request::CreateSession(CreateSessionRequest {
                kind: SessionKind::Agent {
                    name: "claude".into(),
                },
                project: live::PROJECT.into(),
                workspace: live::WORKSPACE.into(),
                name: "probe-suppress".into(),
                label: None,
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    script.into(),
                    "quota-fixture".into(),
                    run.fixture.executable.clone().into_os_string(),
                    native.into_os_string(),
                    std::env::current_exe().unwrap().into_os_string(),
                ],
            }))
    else {
        panic!("managed session was not created");
    };
    if let Some(pid) = summary.pid {
        run.fixture.own_group(pid as libc::pid_t);
    }
    let session = summary.id;
    wait_terminal(&run.fixture, session, "ADMISSION_READY");
    assert_eq!(
        run.fixture.request(Request::SendTerminal {
            session,
            text: "root".into(),
            submit: true,
        }),
        Response::Ok
    );
    wait_terminal(&run.fixture, session, "ADMISSION_CALLBACK=0");
    assert_eq!(
        run.fixture.request(Request::SendTerminal {
            session,
            text: "statusline-quota".into(),
            submit: true,
        }),
        Response::Ok
    );
    wait_terminal(&run.fixture, session, "ADMISSION_CALLBACK=1");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(probe_count(&run.log), 0, "probe ran before attach");
    let mut socket = attach(&run.fixture);
    write_frame(
        &mut socket,
        &ClientMessage {
            request_id: 2,
            request: Request::Select {
                session,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let mut snapshot = None;
    let mut notes = Vec::new();
    while std::time::Instant::now() < deadline && snapshot.is_none() {
        let message = read_frame::<ServerMessage>(&mut socket).unwrap();
        if let ServerMessage::Response {
            request_id: 2,
            response: Response::Error { message, .. },
        } = &message
        {
            panic!("select failed: {message}");
        }
        if let ServerMessage::Event(ServerEvent::QuotaChanged(quota)) = &message {
            notes.push(format!(
                "{:?} {:?}",
                quota.claude.state, quota.claude.source
            ));
            if matches!(quota.claude.source, Some(QuotaSource::Session { .. }))
                && quota.claude.state == QuotaState::Current
            {
                snapshot = Some(quota.clone());
            }
        }
    }
    let snapshot = snapshot.unwrap_or_else(|| {
        panic!(
            "no session quota ({notes:?})\n{}",
            probe_lines(&run.log).join("\n")
        )
    });
    assert!(
        snapshot
            .claude
            .windows
            .iter()
            .any(|window| window.id == "five_hour")
    );
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        probe_count(&run.log),
        0,
        "{}",
        probe_lines(&run.log).join("\n")
    );
}

fn wait_terminal(fixture: &live::Live, session: SessionId, needle: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "terminal never showed {needle}"
        );
        if let Response::TerminalText { text, .. } = fixture.request(Request::ReadTerminal {
            session,
            max_lines: Some(400),
        }) && text.contains(needle)
        {
            return;
        }
        std::thread::park_timeout(Duration::from_millis(30));
    }
}
