//! Native quota clients driven through the production Server/socket/Dashboard path.
#[path = "support/live.rs"]
mod live;

use ovrcr::protocol::*;
use ovrcr::session::TerminalSize;
use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

/// Distinctive native body text; no Quota reason may contain it.
const FIXTURE_BODY: &str = "upstream said: fixture@example.invalid overloaded";

#[test]
#[ignore = "task-owned native RPC fixture process"]
fn native_quota_rpc_fixture() {
    let log = std::env::var_os("OVRCR_QUOTA_FIXTURE_LOG").unwrap();
    if let Some(pids) = std::env::var_os("OVRCR_QUOTA_FIXTURE_PIDS") {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pids)
            .unwrap()
            .write_all(format!("{}\n", std::process::id()).as_bytes())
            .unwrap();
    }
    for line in std::io::stdin().lock().lines() {
        let request: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let method = request["method"].as_str().unwrap();
        // Codex and Grok share this append log. Keep each method and its
        // newline in one write so concurrent native processes cannot splice rows.
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(format!("{method}\n").as_bytes())
            .unwrap();
        let Some(id) = request.get("id") else {
            continue;
        };
        let mode = std::env::var("OVRCR_QUOTA_FIXTURE_MODE").unwrap_or_default();
        if method == "account/rateLimits/read" && matches!(mode.as_str(), "http503" | "retry") {
            // A native error whose body a Quota reason must never echo.
            let mut data = serde_json::json!({"status": 503, "body": FIXTURE_BODY});
            if mode == "retry" {
                data["retryAfterSeconds"] = 900.into();
            }
            println!(
                "{}",
                serde_json::json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32000, "message":FIXTURE_BODY, "data":data}})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        let result = match method {
            "initialize" => serde_json::json!({"capabilities": {}}),
            "account/read" => {
                serde_json::json!({"account": {"type":"chatgpt", "email":"fixture@example.invalid", "planType":"plus"}})
            }
            "account/rateLimits/read" => {
                if std::env::var("OVRCR_QUOTA_FIXTURE_MODE").as_deref() == Ok("flood") {
                    loop {
                        println!("{}", serde_json::json!({"method":"fixture/progress"}));
                    }
                }
                if std::env::var("OVRCR_QUOTA_FIXTURE_MODE").as_deref() == Ok("switch") {
                    println!(
                        "{}",
                        serde_json::json!({"method":"account/updated","params":{"account":"B"}})
                    );
                    println!(
                        "{}",
                        serde_json::json!({"method":"account/updated","params":{"account":"A"}})
                    );
                }
                let reset = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 3_600;
                if std::env::var("OVRCR_QUOTA_FIXTURE_MODE").as_deref() == Ok("extra") {
                    serde_json::json!({"rateLimits": null, "rateLimitsByLimitId":{"special-model":{"primary":{"usedPercent":42,"windowDurationMins":60,"resetsAt":reset}}}})
                } else {
                    serde_json::json!({"rateLimits": {"limitId":"codex", "primary": {"usedPercent":67, "windowDurationMins":300, "resetsAt":reset},
                    "secondary": {"usedPercent":31, "windowDurationMins":10080, "resetsAt":reset + 86_400}}, "rateLimitsByLimitId":{}})
                }
            }
            "_x.ai/auth/info" => {
                serde_json::json!({"principalType":"USER", "principalId":"grok-fixture", "email":"fixture@example.invalid"})
            }
            "_x.ai/billing" => {
                let reset = chrono::Utc::now() + chrono::Duration::days(30);
                serde_json::json!({"config":{"creditUsagePercent":24, "currentPeriod":{
                    "type":"USAGE_PERIOD_TYPE_MONTHLY", "start":chrono::Utc::now().to_rfc3339(), "end":reset.to_rfc3339()},
                    "prepaidBalance":{"val":9900}, "onDemandUsed":{"val":5000}}})
            }
            _ => panic!("unexpected native method"),
        };
        println!(
            "{}",
            serde_json::json!({"jsonrpc":"2.0", "id":id, "result":result})
        );
        std::io::stdout().flush().unwrap();
    }
}

#[test]
fn codex_native_read_reaches_remaining_bars_through_real_server_and_socket() {
    let fixture = live::Live::idle().bounded();
    let native = fixture.root.path().join("codex");
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .replace('\'', "'\\''");
    std::fs::write(&native, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'codex-cli 0.155.1'; exit 0; fi\nexec '{executable}' --ignored --exact native_quota_rpc_fixture --nocapture --quiet\n")).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = serde_json::to_string(&native.to_string_lossy()).unwrap();
    std::fs::write(settings_document(&fixture), format!("[quota]\nenabled = true\n[quota.codex]\ncommand = {config}\n[quota.grok]\ncommand = '/not-a-native-fixture'\n")).unwrap();
    let log = fixture.root.path().join("native-methods");
    fixture.start_binary_env(&[("OVRCR_QUOTA_FIXTURE_LOG", log.as_os_str())]);
    let mut socket = connect_server(&fixture.socket).unwrap();
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
    let mut dashboard = ovrcr::tui::Dashboard::new(TerminalSize {
        rows: 32,
        cols: 100,
    });
    dashboard.install_area(ratatui::layout::Rect::new(0, 0, 100, 32));
    let mut quota = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "native Codex quota was not published"
        );
        let message: ServerMessage = read_frame(&mut socket).unwrap();
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) = &message
            && snapshot.codex.state == QuotaState::Current
        {
            quota = Some(snapshot.codex.clone());
        }
        dashboard.handle_server_message(message);
        if quota.is_some() {
            break;
        }
    }
    let quota = quota.unwrap();
    assert!(
        quota.checked_unix_ms.is_some(),
        "native account reads must carry check provenance"
    );
    assert!(matches!(
        quota.source,
        Some(QuotaSource::NativeProfile { .. })
    ));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 32)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let screen = (0..32)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        screen
            .lines()
            .any(|line| line.contains("Codex") && line.contains("5h") && line.contains("33%")),
        "native five-hour quota missing: {screen}"
    );
    assert!(
        screen
            .lines()
            .any(|line| line.contains("7d") && line.contains("69%")),
        "native seven-day quota missing: {screen}"
    );
    let methods = std::fs::read_to_string(log).unwrap();
    assert_eq!(
        methods
            .lines()
            .filter(|line| *line == "account/rateLimits/read")
            .count(),
        1
    );
    assert!(
        !methods.contains("thread/start") && !methods.contains("turn/start"),
        "quota must not create conversations/prompts"
    );
}

#[test]
fn grok_native_billing_reaches_actual_monthly_remaining_bar() {
    let fixture = live::Live::idle().bounded();
    let native = fixture.root.path().join("grok");
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .replace('\'', "'\\''");
    std::fs::write(&native, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo '1.0.40 (fixture) [stable]'; exit 0; fi\nexec '{executable}' --ignored --exact native_quota_rpc_fixture --nocapture --quiet\n")).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = serde_json::to_string(&native.to_string_lossy()).unwrap();
    std::fs::write(settings_document(&fixture), format!("[quota]\nenabled = true\n[quota.codex]\ncommand = '/not-a-native-fixture'\n[quota.grok]\ncommand = {config}\n")).unwrap();
    let log = fixture.root.path().join("native-methods");
    fixture.start_binary_env(&[("OVRCR_QUOTA_FIXTURE_LOG", log.as_os_str())]);
    let mut socket = connect_server(&fixture.socket).unwrap();
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
    let mut dashboard = ovrcr::tui::Dashboard::new(TerminalSize {
        rows: 32,
        cols: 100,
    });
    dashboard.install_area(ratatui::layout::Rect::new(0, 0, 100, 32));
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let quota = loop {
        assert!(
            std::time::Instant::now() < deadline,
            "native Grok quota was not published"
        );
        let message: ServerMessage = read_frame(&mut socket).unwrap();
        let quota = match &message {
            ServerMessage::Event(ServerEvent::QuotaChanged(snapshot))
                if snapshot.grok.state == QuotaState::Current =>
            {
                Some(snapshot.grok.clone())
            }
            _ => None,
        };
        dashboard.handle_server_message(message);
        if let Some(quota) = quota {
            break quota;
        }
    };
    assert_eq!(
        quota.windows.len(),
        1,
        "extra credits/spend are not general allowance"
    );
    assert_eq!(quota.windows[0].label, "mo");
    assert_eq!(quota.windows[0].remaining_basis_points(), Some(7600));
    assert!(quota.checked_unix_ms.is_some());
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 32)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let screen = (0..32)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        screen
            .lines()
            .any(|line| line.contains("Grok") && line.contains("mo") && line.contains("76%")),
        "monthly quota missing: {screen}"
    );
    let methods = std::fs::read_to_string(log).unwrap();
    assert_eq!(
        methods
            .lines()
            .filter(|line| *line == "_x.ai/billing")
            .count(),
        1
    );
    assert!(
        methods
            .lines()
            .all(|method| matches!(method, "initialize" | "_x.ai/auth/info" | "_x.ai/billing")),
        "no prompts, credential exports or mutating billing requests"
    );
}

/// The settings document beside the fixture's instance identity.
fn settings_document(fixture: &live::Live) -> std::path::PathBuf {
    fixture.config.with_file_name("dashboard.toml")
}

fn native_executable(fixture: &live::Live, name: &str, version: &str) -> std::path::PathBuf {
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .replace('\'', "'\\''");
    let native = fixture.root.path().join(name);
    std::fs::write(&native, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{version}'; exit 0; fi\nexec '{executable}' --ignored --exact native_quota_rpc_fixture --nocapture --quiet\n")).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    native
}

#[test]
fn quota_table_in_instance_identity_is_a_finding_and_starts_no_worker() {
    let fixture = live::Live::idle().bounded();
    let native = native_executable(&fixture, "codex", "codex-cli 0.155.1");
    let command = serde_json::to_string(&native.to_string_lossy()).unwrap();
    // Before the shared settings document this enabled collection.
    std::fs::write(
        &fixture.config,
        format!("projects = []\n[quota]\nenabled = true\n[quota.codex]\ncommand = {command}\n"),
    )
    .unwrap();
    let methods = fixture.root.path().join("native-methods");
    let server_log = fixture.root.path().join("server.log");
    fixture.start_binary_logged(
        &[("OVRCR_QUOTA_FIXTURE_LOG", methods.as_os_str())],
        &server_log,
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let log = std::fs::read_to_string(&server_log).unwrap_or_default();
        // Wait for the whole line; eprintln can land in more than one write.
        if log
            .split_once("settings finding")
            .is_some_and(|(_, rest)| rest.contains('\n'))
        {
            assert!(
                log.contains("quota quota settings belong in dashboard.toml"),
                "{log}"
            );
            assert!(log.contains("(line 2)"), "{log}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no settings finding logged: {log}"
        );
        std::thread::park_timeout(Duration::from_millis(20));
    }
    // Collection would start as soon as a Dashboard attaches.
    let _socket = attach(&fixture);
    // This wait asserts an absence: the enabled run above logs within it.
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !methods.exists(),
        "a [quota] table in config.toml started a native client"
    );
}

fn native_fixture(mode: &str) -> live::Live {
    let fixture = live::Live::idle().bounded();
    let mut config = "[quota]\nenabled = true\n".to_string();
    for (name, version) in [
        ("codex", "codex-cli 0.155.1"),
        ("grok", "1.0.40 (fixture) [stable]"),
    ] {
        let native = native_executable(&fixture, name, version);
        config.push_str(&format!(
            "[quota.{name}]\ncommand = {}\n",
            serde_json::to_string(&native.to_string_lossy()).unwrap()
        ));
    }
    std::fs::write(settings_document(&fixture), config).unwrap();
    let log = fixture.root.path().join("native-methods");
    fixture.start_binary_env(&[
        ("OVRCR_QUOTA_FIXTURE_LOG", log.as_os_str()),
        ("OVRCR_QUOTA_FIXTURE_MODE", std::ffi::OsStr::new(mode)),
    ]);
    fixture
}

fn attach(fixture: &live::Live) -> std::os::unix::net::UnixStream {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let mut socket = connect_server(&fixture.socket).unwrap();
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
                response:
                    Response::Error {
                        code: ErrorCode::Conflict,
                        ..
                    },
                ..
            } => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "dashboard slot was not released"
                );
                std::thread::park_timeout(Duration::from_millis(10));
            }
            ServerMessage::Response {
                response: Response::Hierarchy(_),
                ..
            } => return socket,
            other => panic!("unexpected greeting: {other:?}"),
        }
    }
}

#[test]
fn providers_remain_visible_before_any_native_quota_source_reports() {
    let fixture = live::Live::binary();
    fixture.ready("feature/quota-waiting");
    let mut socket = connect_server(&fixture.socket).unwrap();
    write_frame(
        &mut socket,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let mut dashboard = ovrcr::tui::Dashboard::new(TerminalSize {
        rows: 32,
        cols: 120,
    });
    dashboard.install_area(ratatui::layout::Rect::new(0, 0, 120, 32));
    dashboard.handle_server_message(read_frame::<ServerMessage>(&mut socket).unwrap());
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 32)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    for expected in [
        "Quota left",
        "Claude — checking",
        "Codex — off",
        "Grok — off",
    ] {
        assert!(screen.contains(expected), "missing {expected}: {screen}");
    }
}

#[test]
fn unrelated_native_messages_cannot_extend_the_account_request_deadline() {
    let fixture = native_fixture("flood");
    let mut socket = attach(&fixture);
    socket
        .set_read_timeout(Some(Duration::from_secs(25)))
        .unwrap();
    let started = std::time::Instant::now();
    loop {
        let message = read_frame::<ServerMessage>(&mut socket)
            .expect("notification flood kept the native request alive beyond its deadline");
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) = message
            && snapshot.codex.state == QuotaState::Unavailable
            && matches!(
                snapshot.codex.source,
                Some(QuotaSource::NativeProfile { .. })
            )
        {
            assert!(started.elapsed() < Duration::from_secs(25));
            assert!(snapshot.codex.windows.is_empty());
            assert_eq!(
                snapshot.codex.reason.as_deref(),
                Some("timed out after 20 s")
            );
            assert_next_check(&snapshot.codex, 60);
            break;
        }
    }
    assert!(
        std::fs::read_to_string(fixture.root.path().join("native-methods"))
            .unwrap()
            .lines()
            .any(|method| method == "account/rateLimits/read")
    );
    assert!(
        matches!(fixture.request(Request::List), Response::Hierarchy(_)),
        "native flood blocked the server's control path"
    );
}

#[test]
fn native_collection_pauses_without_dashboard_and_deduplicates_fifty_sessions() {
    let fixture = native_fixture("normal");
    let log = fixture.root.path().join("native-methods");
    // This wait asserts an absence; positive synchronization below uses the actual socket.
    std::thread::sleep(Duration::from_millis(250));
    assert!(!log.exists(), "collection ran before dashboard attachment");
    fixture.ready("feature/quota-fifty");
    // ready creates one shell; preserve it and fill the other 49 slots.
    for index in 0..49 {
        assert!(matches!(
            fixture.request(Request::CreateSession(CreateSessionRequest {
                project: live::PROJECT.into(),
                workspace: live::WORKSPACE.into(),
                name: format!("quota-{index}"),
                label: None,
                argv: vec!["sh".into(), "-c".into(), "while :; do sleep 1; done".into()],
                kind: SessionKind::Terminal,
            })),
            Response::CreatedSession(_)
        ));
    }
    let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
        panic!("no hierarchy")
    };
    assert_eq!(
        hierarchy
            .projects
            .iter()
            .flat_map(|p| &p.workspaces)
            .flat_map(|w| &w.sessions)
            .count(),
        50
    );
    let mut socket = attach(&fixture);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let latest = loop {
        assert!(
            std::time::Instant::now() < deadline,
            "native quotas never became current"
        );
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) =
            read_frame::<ServerMessage>(&mut socket).unwrap()
            && snapshot.codex.state == QuotaState::Current
            && snapshot.grok.state == QuotaState::Current
        {
            break *snapshot;
        }
    };
    let methods = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        methods
            .lines()
            .filter(|line| *line == "account/rateLimits/read")
            .count(),
        1
    );
    assert_eq!(
        methods
            .lines()
            .filter(|line| *line == "_x.ai/billing")
            .count(),
        1
    );
    drop(socket);
    let mut reattached = attach(&fixture);
    loop {
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) =
            read_frame::<ServerMessage>(&mut reattached).unwrap()
        {
            assert_eq!(snapshot.codex.windows, latest.codex.windows);
            assert_eq!(snapshot.grok.windows, latest.grok.windows);
            break;
        }
    }
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        methods,
        "reattachment duplicated account reads"
    );
}

#[test]
fn model_specific_bucket_is_not_a_general_sidebar_allowance() {
    let fixture = native_fixture("extra");
    let mut socket = attach(&fixture);
    let mut dashboard = ovrcr::tui::Dashboard::new(TerminalSize {
        rows: 32,
        cols: 100,
    });
    dashboard.install_area(ratatui::layout::Rect::new(0, 0, 100, 32));
    loop {
        let message = read_frame::<ServerMessage>(&mut socket).unwrap();
        let done = matches!(&message, ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) if snapshot.codex.state == QuotaState::Current);
        dashboard.handle_server_message(message);
        if done {
            break;
        }
    }
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 32)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("Codex"));
    assert!(
        !screen.contains("58%"),
        "special-model bucket mislabeled as general allowance: {screen}"
    );
    assert!(screen.contains("not reported"));
    dashboard.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('u'),
        crossterm::event::KeyModifiers::NONE,
    ));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        screen.contains("special-model/primary"),
        "native bucket identity lost: {screen}"
    );
    assert!(screen.contains("58%"));
}

#[test]
fn account_a_to_b_to_a_during_native_read_cannot_publish_old_allowance() {
    let fixture = native_fixture("switch");
    let mut socket = attach(&fixture);
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "account change was not fenced"
        );
        if let ServerMessage::Event(ServerEvent::QuotaChanged(quota)) =
            read_frame::<ServerMessage>(&mut socket).unwrap()
        {
            assert_ne!(
                quota.codex.state,
                QuotaState::Current,
                "cross-generation result admitted"
            );
            if quota.codex.state == QuotaState::SourceConflict {
                assert!(quota.codex.windows.is_empty());
                assert!(quota.codex.checked_unix_ms.is_none());
                break;
            }
        }
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[test]
fn quota_enabled_flips_live_without_restart() {
    let fixture = live::Live::idle().bounded();
    let native = fixture.root.path().join("codex");
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .replace('\'', "'\\''");
    std::fs::write(&native, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'codex-cli 0.155.1'; exit 0; fi\nexec '{executable}' --ignored --exact native_quota_rpc_fixture --nocapture --quiet\n")).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let settings = fixture.root.path().join("dashboard.toml");
    let quota = |enabled: bool| {
        format!(
            "[quota]\nenabled = {enabled}\n[quota.codex]\ncommand = {}\n[quota.grok]\ncommand = '/not-a-native-fixture'\n",
            serde_json::to_string(&native.to_string_lossy()).unwrap()
        )
    };
    std::fs::write(&settings, quota(false)).unwrap();
    let log = fixture.root.path().join("native-methods");
    let pids = fixture.root.path().join("native-pids");
    fixture.start_binary_env(&[
        ("OVRCR_QUOTA_FIXTURE_LOG", log.as_os_str()),
        ("OVRCR_QUOTA_FIXTURE_PIDS", pids.as_os_str()),
    ]);
    let _socket = attach(&fixture);
    // Disabled at startup: the worker waits instead of exiting, and spawns nothing.
    std::thread::sleep(Duration::from_secs(3));
    assert!(!log.exists(), "disabled quota spawned a native client");

    std::fs::write(&settings, quota(true)).unwrap();
    let rate_reads = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter(|line| *line == "account/rateLimits/read")
            .count()
    };
    wait_until("enabling quota did not start collection", || {
        rate_reads() == 1
    });
    let first: i32 = std::fs::read_to_string(&pids)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(first));

    std::fs::write(&settings, quota(false)).unwrap();
    wait_until("disabling quota did not stop the native client", || {
        !alive(first)
    });

    // The worker survived being disabled: enabling again collects with a new client.
    std::fs::write(&settings, quota(true)).unwrap();
    wait_until("re-enabling quota did not collect again", || {
        rate_reads() == 2
    });
    assert_eq!(std::fs::read_to_string(&pids).unwrap().lines().count(), 2);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The row's Next check is `seconds` from now, give or take the read itself.
fn assert_next_check(quota: &ProviderQuota, seconds: u64) {
    let next = quota.next_check_unix_ms.expect("no next check");
    let expected = now_ms() + seconds * 1000;
    assert!(
        next <= expected + 1_000 && next + 5_000 >= expected,
        "{:?} next check {}s out, expected {seconds}s",
        quota.provider,
        (next as i64 - now_ms() as i64) / 1000
    );
}

/// Read Dashboard frames until a quota snapshot satisfies `done`.
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
        {
            for row in [&snapshot.claude, &snapshot.codex, &snapshot.grok] {
                let reason = row.reason.as_deref().unwrap_or_default();
                assert!(
                    !reason.contains("fixture") && !reason.contains("overloaded"),
                    "a Quota reason echoed native bytes: {reason}"
                );
            }
            if done(&snapshot) {
                return *snapshot;
            }
        }
    }
}

fn reads(fixture: &live::Live, method: &str) -> usize {
    std::fs::read_to_string(fixture.root.path().join("native-methods"))
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == method)
        .count()
}

#[test]
fn enabled_attach_shows_checking_then_current_within_one_read() {
    let fixture = native_fixture("normal");
    let mut socket = attach(&fixture);
    let first = quota_until(&mut socket, "no quota snapshot on attach", |_| true);
    assert_eq!(first.codex.state, QuotaState::Checking);
    assert_eq!(first.grok.state, QuotaState::Checking);
    assert_eq!(first.claude.state, QuotaState::Checking);
    assert_eq!(first.claude.reason.as_deref(), Some(CLAUDE_WAITING));
    let current = quota_until(&mut socket, "native quotas never became current", |q| {
        q.codex.state == QuotaState::Current && q.grok.state == QuotaState::Current
    });
    assert_eq!(reads(&fixture, "account/rateLimits/read"), 1);
    assert_eq!(reads(&fixture, "_x.ai/billing"), 1);
    assert_eq!(current.codex.reason, None);
    assert_next_check(&current.codex, 300);
    // Claude has no native reader: it keeps waiting for a managed session.
    assert_eq!(current.claude.state, QuotaState::Checking);
    assert!(matches!(
        fixture.request(Request::RefreshQuota {
            provider: Some(QuotaProvider::Claude)
        }),
        Response::Error { code: ErrorCode::InvalidRequest, message } if message.contains(CLAUDE_WAITING)
    ));
}

#[test]
fn disabled_quota_snapshot_carries_the_off_state_reason() {
    // Off at hello is the protocol default the Dashboard already holds.
    let off = QuotaSnapshot::default();
    for row in [&off.codex, &off.grok] {
        assert_eq!(row.state, QuotaState::Disabled);
        assert_eq!(row.reason.as_deref(), Some(ovrcr::settings::QUOTA_OFF));
    }
    let fixture = native_fixture("normal");
    let mut socket = attach(&fixture);
    quota_until(&mut socket, "native quotas never became current", |q| {
        q.codex.state == QuotaState::Current
    });
    std::fs::write(settings_document(&fixture), "[quota]\nenabled = false\n").unwrap();
    let snapshot = quota_until(&mut socket, "turning quota off was not published", |q| {
        q.codex.state == QuotaState::Disabled && q.grok.state == QuotaState::Disabled
    });
    for row in [&snapshot.codex, &snapshot.grok] {
        assert_eq!(row.reason.as_deref(), Some(ovrcr::settings::QUOTA_OFF));
        assert_eq!(row.next_check_unix_ms, None);
        assert!(row.windows.is_empty());
    }
    assert!(matches!(
        fixture.request(Request::RefreshQuota { provider: None }),
        Response::Error { message, .. } if message == ovrcr::settings::QUOTA_OFF
    ));
}

#[test]
fn missing_executable_is_not_found_with_next_check_at_the_cap() {
    let fixture = live::Live::idle().bounded();
    std::fs::write(
        settings_document(&fixture),
        "[quota]\nenabled = true\n[quota.codex]\ncommand = 'ovrcr-no-such-native-client'\n[quota.grok]\ncommand = '/not-a-native-fixture'\n",
    )
    .unwrap();
    fixture.start_binary_env(&[]);
    let mut socket = attach(&fixture);
    let snapshot = quota_until(&mut socket, "missing executables were not reported", |q| {
        q.codex.state == QuotaState::Unavailable && q.grok.state == QuotaState::Unavailable
    });
    assert_eq!(
        snapshot.codex.reason.as_deref(),
        Some("codex not found on PATH")
    );
    assert_eq!(
        snapshot.grok.reason.as_deref(),
        Some("grok not found at the configured path")
    );
    assert_next_check(&snapshot.codex, 600);
    assert_next_check(&snapshot.grok, 600);
}

#[test]
fn retry_after_overrides_the_ladder_and_a_manual_refresh() {
    let fixture = native_fixture("retry");
    let mut socket = attach(&fixture);
    let failed = quota_until(&mut socket, "HTTP 503 was not reported", |q| {
        q.codex.state == QuotaState::Unavailable
    });
    assert_eq!(failed.codex.reason.as_deref(), Some("HTTP 503"));
    // The ladder says one minute; the provider said fifteen.
    assert_next_check(&failed.codex, 900);
    assert_eq!(
        fixture.request(Request::RefreshQuota {
            provider: Some(QuotaProvider::Codex)
        }),
        Response::Ok
    );
    // This wait asserts an absence: an accepted refresh reads within 100 ms.
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(reads(&fixture, "account/rateLimits/read"), 1);
}

#[test]
fn manual_refresh_skips_failure_backoff_once_per_cooldown() {
    let fixture = native_fixture("http503");
    let mut socket = attach(&fixture);
    let failed = quota_until(&mut socket, "HTTP 503 was not reported", |q| {
        q.codex.state == QuotaState::Unavailable
    });
    assert_eq!(failed.codex.reason.as_deref(), Some("HTTP 503"));
    assert_next_check(&failed.codex, 60);
    assert_eq!(reads(&fixture, "account/rateLimits/read"), 1);
    assert_eq!(
        fixture.request(Request::RefreshQuota { provider: None }),
        Response::Ok
    );
    // Read at once, not a minute later; the second failure climbs the ladder.
    let again = quota_until(&mut socket, "manual refresh did not read", |q| {
        q.codex
            .next_check_unix_ms
            .is_some_and(|next| next > now_ms() + 90_000)
    });
    assert_next_check(&again.codex, 120);
    assert_eq!(reads(&fixture, "account/rateLimits/read"), 2);
    match fixture.request(Request::RefreshQuota { provider: None }) {
        Response::QuotaCooldown { remaining_ms } => {
            assert!((1..=30_000).contains(&remaining_ms), "{remaining_ms}")
        }
        other => panic!("second refresh within 30 s was not refused: {other:?}"),
    }
}

#[test]
fn set_setting_quota_enabled_off_publishes_disabled_rows() {
    let fixture = native_fixture("normal");
    let mut socket = attach(&fixture);
    quota_until(&mut socket, "native quotas never became current", |q| {
        q.codex.state == QuotaState::Current
    });
    assert_eq!(
        fixture.request(Request::SetSetting {
            path: "quota.enabled".into(),
            value: Some("false".into()),
        }),
        Response::Ok
    );
    let snapshot = quota_until(&mut socket, "SetSetting off was not published", |q| {
        q.codex.state == QuotaState::Disabled && q.grok.state == QuotaState::Disabled
    });
    assert_eq!(
        snapshot.codex.reason.as_deref(),
        Some(ovrcr::settings::QUOTA_OFF)
    );
}

/// `claude auth status --json` sets the Claude row until a managed session
/// reports. The Server asks at start and again on each `quota.*` or Claude
/// executable change, so the row follows the fixture without a restart.
#[test]
fn claude_auth_status_sets_the_claude_row_and_keeps_no_identifier() {
    let fixture = live::Live::idle().bounded();
    fixture.claude.set(false, "none", None);
    let server_log = fixture.root.path().join("server.log");
    fixture.start_binary_logged(&[], &server_log);
    let mut socket = attach(&fixture);
    let mut seen = Vec::new();
    let mut claude_until = |what: &str, state: QuotaState, reason: &str| {
        let snapshot = quota_until(&mut socket, what, |q| {
            seen.push(format!("{q:?}"));
            q.claude.state == state
        });
        assert_eq!(snapshot.claude.reason.as_deref(), Some(reason), "{what}");
        assert_eq!(snapshot.claude.source, None);
    };
    let mut change = 0;
    let set = |path: &str, value: String| {
        assert_eq!(
            fixture.request(Request::SetSetting {
                path: path.into(),
                value: Some(value),
            }),
            Response::Ok
        );
    };
    let mut bump = || {
        change += 1;
        set("quota.codex.command", format!("\"codex-{change}\""));
    };

    claude_until(
        "not signed in at start",
        QuotaState::NotSignedIn,
        "run claude auth login",
    );
    fixture.claude.set(true, "claude.ai", Some("max"));
    bump();
    claude_until("signed in", QuotaState::Checking, CLAUDE_WAITING);
    fixture.claude.set(true, "api_key", None);
    bump();
    claude_until(
        "API-key login",
        QuotaState::Unsupported,
        "API-key logins have no subscription allowance",
    );
    fixture.claude.set(false, "none", None);
    bump();
    claude_until(
        "signed out again",
        QuotaState::NotSignedIn,
        "run claude auth login",
    );

    // The executable the settings name wins over PATH; a missing or failing
    // one is Unavailable with OVRCR's own reason.
    let agents = |argv0: &str| {
        format!(
            "[{{ name = \"claude\", argv = [{}] }}]",
            toml::Value::String(argv0.into())
        )
    };
    let missing = fixture.root.path().join("no-such-claude");
    set("agents", agents(&missing.display().to_string()));
    claude_until(
        "missing claude",
        QuotaState::Unavailable,
        "claude not found at the configured path",
    );
    set("agents", agents("/usr/bin/false"));
    claude_until(
        "failing claude",
        QuotaState::Unavailable,
        "claude auth status unreadable",
    );

    let log = std::fs::read_to_string(&server_log).unwrap();
    for text in seen.iter().chain([&log]) {
        assert!(
            !text.contains("example.invalid") && !text.contains("firstParty"),
            "the auth status output leaked: {text}"
        );
    }
}

/// With no `claude` anywhere, the Claude row says so from the first snapshot
/// a Dashboard gets: the frame at hello is expected, not an accident.
#[test]
fn no_claude_on_path_is_unavailable_with_its_reason() {
    let fixture = live::Live::idle().bounded();
    fixture.start_binary_env(&[("PATH", std::ffi::OsStr::new("/usr/bin:/bin"))]);
    let mut socket = attach(&fixture);
    let snapshot = quota_until(&mut socket, "no Claude row at hello", |_| true);
    assert_eq!(snapshot.claude.state, QuotaState::Unavailable);
    assert_eq!(
        snapshot.claude.reason.as_deref(),
        Some("claude not found on PATH")
    );
}

/// A check still running when the Server stops is no answer: the Dashboard
/// never sees "read interrupted" as the Claude row.
#[test]
fn claude_auth_cut_short_by_shutdown_publishes_nothing() {
    let fixture = live::Live::idle().bounded();
    let bin = fixture.root.path().join("hanging-claude");
    std::fs::create_dir(&bin).unwrap();
    let claude = bin.join("claude");
    std::fs::write(&claude, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    fixture.start_binary_env(&[("PATH", std::ffi::OsStr::new(&path))]);
    let mut socket = attach(&fixture);
    write_frame(
        &mut socket,
        &ClientMessage {
            request_id: 2,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    let mut acknowledged = false;
    while let Ok(message) = read_frame::<ServerMessage>(&mut socket) {
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) = &message {
            panic!("a stopping Server published the Claude row: {snapshot:?}");
        }
        acknowledged |= message
            == ServerMessage::Response {
                request_id: 2,
                response: Response::Ok,
            };
    }
    assert!(acknowledged, "shutdown was not acknowledged");
}
