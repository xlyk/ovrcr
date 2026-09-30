//! Native quota clients driven through the production Server/socket/Dashboard path.
#[path = "support/live.rs"]
mod live;

use ovrcr::protocol::*;
use ovrcr::session::TerminalSize;
use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[test]
#[ignore = "task-owned native RPC fixture process"]
fn native_quota_rpc_fixture() {
    let log = std::env::var_os("OVRCR_QUOTA_FIXTURE_LOG").unwrap();
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
    std::fs::write(&fixture.config, format!("projects = []\n[quota]\nenabled = true\n[quota.codex]\ncommand = {config}\n[quota.grok]\ncommand = '/not-a-native-fixture'\n")).unwrap();
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
    std::fs::write(&fixture.config, format!("projects = []\n[quota]\nenabled = true\n[quota.codex]\ncommand = '/not-a-native-fixture'\n[quota.grok]\ncommand = {config}\n")).unwrap();
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

fn native_fixture(mode: &str) -> live::Live {
    let fixture = live::Live::idle().bounded();
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .replace('\'', "'\\''");
    let mut config = "projects = []\n[quota]\nenabled = true\n".to_string();
    for (name, version) in [
        ("codex", "codex-cli 0.155.1"),
        ("grok", "1.0.40 (fixture) [stable]"),
    ] {
        let native = fixture.root.path().join(name);
        std::fs::write(&native, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{version}'; exit 0; fi\nexec '{executable}' --ignored --exact native_quota_rpc_fixture --nocapture --quiet\n")).unwrap();
        std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
        config.push_str(&format!(
            "[quota.{name}]\ncommand = {}\n",
            serde_json::to_string(&native.to_string_lossy()).unwrap()
        ));
    }
    std::fs::write(&fixture.config, config).unwrap();
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
        "Claude — waiting for report",
        "Codex — unavailable",
        "Grok — unavailable",
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
