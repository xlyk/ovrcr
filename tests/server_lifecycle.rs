#[path = "support/deadline.rs"]
mod deadline;

use deadline::wait_deadline;
use ovrcr::config::{Registry, load_registry, save_registry_atomic};
use ovrcr::context::{ContextSource, ContextUsageReport, context_is_stale};
use ovrcr::protocol::{
    AgentReport, AgentUpdate, BranchRequest, ClientMessage, CreateSessionRequest, DashboardView,
    ErrorCode, HISTORY_ROWS, HistoryOpened, HistoryRow, HistorySnapshotId, MAX_FRAME_BYTES,
    PAGE_BYTES, PAGE_COLS, PAGE_ROWS, PaneTarget, Request, Response, ServerEvent, ServerMessage,
    client, connect_server, read_frame, write_frame,
};
use ovrcr::server::{ServerPaths, connect_if_running, connect_or_start, run_server};
use ovrcr::session::{SessionId, SessionPhase};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{Cursor, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serialize tests that share process-wide state. A test that panics while
/// holding the lock poisons it; later tests still run rather than failing
/// on the poison, so a CI log shows the one real failure.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn context_report(
    session: SessionId,
    capability: [u8; 32],
    sequence: Option<u64>,
    model: Option<&str>,
    conversation: Option<&str>,
    used_tokens: Option<u64>,
    capacity_tokens: Option<u64>,
) -> Request {
    Request::AgentReport(AgentReport {
        session,
        capability,
        sequence,
        update: AgentUpdate::Context(ContextUsageReport {
            source: ContextSource::Generic,
            model: model.map(str::to_owned),
            conversation: conversation.map(str::to_owned),
            used_tokens,
            capacity_tokens,
        }),
    })
}

#[test]
#[ignore]
fn hook_child_report_helper() {
    let identity_path =
        std::env::var_os("OVRCR_AGENT_HOOK_IDENTITY_PATH").expect("hook child identity path");
    let socket = std::env::var_os("OVRCR_HOOK_SOCKET").expect("hook socket");
    let session = std::env::var("OVRCR_SESSION_ID")
        .expect("hook session")
        .parse::<u64>()
        .expect("hook session is decimal");
    let token = std::env::var("OVRCR_HOOK_TOKEN").expect("hook token");
    let capability = parse_hook_capability(&token).expect("hook token is 64 lowercase hex");
    std::fs::write(
        identity_path,
        format!("{}\n{}\n{}\n", socket.to_string_lossy(), session, token),
    )
    .expect("write private hook identity");

    let mut stream = connect_server(&socket).expect("connect hook socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set hook read deadline");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("set hook write deadline");
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 1,
            request: Request::AgentReport(AgentReport {
                session: SessionId(session),
                capability,
                sequence: None,
                update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy),
            }),
        },
    )
    .expect("write hook report");
    let response = read_frame::<ServerMessage>(&mut stream).expect("read hook response");
    assert_eq!(
        response,
        ServerMessage::Response {
            request_id: 1,
            response: Response::Ok,
        }
    );
    let identity_path =
        std::env::var_os("OVRCR_AGENT_HOOK_IDENTITY_PATH").expect("hook child identity path");
    std::fs::OpenOptions::new()
        .append(true)
        .open(identity_path)
        .and_then(|mut file| file.write_all(b"REPORTED\n"))
        .expect("record private hook report acknowledgement");
    let mut stdout = std::io::stdout();
    stdout.write_all(b"HOOK_READY\n").expect("write hook ready");
    stdout.flush().expect("flush hook ready");

    let mut stdin = std::io::stdin();
    let mut byte = [0_u8; 1];
    if stdin.read(&mut byte).is_ok() && byte[0] == b'w' {
        let mut stream = connect_server(&socket).expect("reconnect hook socket");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set reconnect hook read deadline");
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("set reconnect hook write deadline");
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 2,
                request: Request::AgentReport(AgentReport {
                    session: SessionId(session),
                    capability,
                    sequence: None,
                    update: AgentUpdate::Activity(ovrcr::session::AgentActivity::WaitingInput),
                }),
            },
        )
        .expect("write reconnect hook report");
        let response =
            read_frame::<ServerMessage>(&mut stream).expect("read reconnect hook response");
        assert_eq!(
            response,
            ServerMessage::Response {
                request_id: 2,
                response: Response::Ok,
            }
        );
        let identity_path =
            std::env::var_os("OVRCR_AGENT_HOOK_IDENTITY_PATH").expect("hook child identity path");
        std::fs::OpenOptions::new()
            .append(true)
            .open(identity_path)
            .and_then(|mut file| file.write_all(b"WAITING_REPORTED\n"))
            .expect("record reconnect hook report acknowledgement");
    }
    loop {
        thread::park_timeout(Duration::from_secs(1));
    }
}

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
        save_registry_atomic(&Registry::default(), &registry).unwrap();
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
        let mut stream = connect_server(&self.paths.socket).unwrap();
        let response = client::request(&mut stream, 1, request).unwrap();
        ServerMessage::Response {
            request_id: 1,
            response,
        }
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
fn agent_hook_sequence_does_not_regress_state() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let identity = fixture.create_hook_child("ordered", "agent-hook-order");
    let pgid = fixture.original_pgid(identity.session);
    fixture.wait_terminal_contains(identity.session, "HOOK_READY");
    let busy = AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy);
    assert_eq!(
        fixture.request(Request::AgentReport(AgentReport {
            session: identity.session,
            capability: identity.capability,
            sequence: Some(1),
            update: busy.clone(),
        })),
        Response::Ok
    );
    assert_eq!(
        fixture.session_activity(identity.session),
        ovrcr::session::AgentActivity::Busy
    );
    assert!(matches!(
        fixture.request(Request::AgentReport(AgentReport {
            session: identity.session,
            capability: identity.capability,
            sequence: None,
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Idle),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.session_activity(identity.session),
        ovrcr::session::AgentActivity::Busy
    );
    assert!(matches!(
        fixture.request(Request::AgentReport(AgentReport {
            session: identity.session,
            capability: identity.capability,
            sequence: Some(1),
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Idle),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.session_activity(identity.session),
        ovrcr::session::AgentActivity::Busy
    );
    assert!(matches!(
        fixture.request(Request::AgentReport(AgentReport {
            session: identity.session,
            capability: identity.capability,
            sequence: Some(0),
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Idle),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.session_activity(identity.session),
        ovrcr::session::AgentActivity::Busy
    );
    assert_eq!(
        fixture.request(Request::AgentReport(AgentReport {
            session: identity.session,
            capability: identity.capability,
            sequence: Some(2),
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Idle),
        })),
        Response::Ok
    );
    assert_eq!(
        fixture.session_activity(identity.session),
        ovrcr::session::AgentActivity::Idle
    );
    assert_eq!(
        fixture.request(Request::KillSession {
            session: identity.session
        }),
        Response::Ok
    );
    wait_exited_and_assert_terminal_contains(&fixture, identity.session, &["HOOK_READY"]);
    assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    assert!(matches!(
        fixture.request(Request::AgentReport(AgentReport {
            session: identity.session,
            capability: identity.capability,
            sequence: Some(3),
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    fixture.shutdown_kill();
}

#[test]
fn agent_hook_capability_and_exit_are_enforced() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let first = fixture.create_hook_child("agent-a", "agent-hook-auth-a");
    let second = fixture.create_hook_child("agent-b", "agent-hook-auth-b");
    let first_pgid = fixture.original_pgid(first.session);
    let second_pgid = fixture.original_pgid(second.session);
    fixture.wait_terminal_contains(first.session, "HOOK_READY");
    fixture.wait_terminal_contains(second.session, "HOOK_READY");
    assert!(matches!(
        fixture.request(Request::AgentReport(AgentReport {
            session: second.session,
            capability: first.capability,
            sequence: Some(1),
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.session_activity(first.session),
        ovrcr::session::AgentActivity::Unknown
    );
    assert_eq!(
        fixture.session_activity(second.session),
        ovrcr::session::AgentActivity::Unknown
    );
    assert_eq!(
        fixture.request(Request::AgentReport(AgentReport {
            session: second.session,
            capability: second.capability,
            sequence: None,
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy),
        })),
        Response::Ok
    );
    assert_eq!(
        fixture.session_activity(second.session),
        ovrcr::session::AgentActivity::Busy
    );
    assert_eq!(fixture.session_phase(first.session), SessionPhase::Running);
    assert_eq!(fixture.session_phase(second.session), SessionPhase::Running);
    assert!(matches!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::AgentReport(AgentReport {
            session: second.session,
            capability: second.capability,
            sequence: None,
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Idle),
        })),
        Response::Ok
    );
    assert_eq!(fixture.session_phase(second.session), SessionPhase::Running);
    assert_eq!(
        fixture.request(Request::PauseSession {
            session: second.session
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::AgentReport(AgentReport {
            session: second.session,
            capability: second.capability,
            sequence: None,
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::WaitingInput),
        })),
        Response::Ok
    );
    assert_eq!(fixture.session_phase(second.session), SessionPhase::Paused);
    assert_eq!(
        fixture.session_activity(second.session),
        ovrcr::session::AgentActivity::WaitingInput
    );
    assert_eq!(
        fixture.request(Request::CloseTerminal {
            session: first.session
        }),
        Response::Ok
    );
    assert!(wait_group_absent(first_pgid, Duration::from_secs(2)));
    assert!(matches!(
        fixture.request(Request::AgentReport(AgentReport {
            session: first.session,
            capability: first.capability,
            sequence: None,
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy),
        })),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::KillSession {
            session: second.session,
        }),
        Response::Ok
    );
    wait_exited_and_assert_terminal_contains(&fixture, second.session, &["HOOK_READY"]);
    assert!(wait_group_absent(second_pgid, Duration::from_secs(2)));
    assert!(matches!(
        fixture.request(Request::AgentReport(AgentReport {
            session: second.session,
            capability: second.capability,
            sequence: None,
            update: AgentUpdate::Activity(ovrcr::session::AgentActivity::Busy),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    fixture.shutdown_kill();
}

#[test]
fn agent_hook_startup_registration_is_visible() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let identity = fixture.create_hook_child_with_report("startup", "agent-hook-startup");
    assert_eq!(
        fixture.session_activity(identity.session),
        ovrcr::session::AgentActivity::Busy
    );
    let pgid = fixture.original_pgid(identity.session);
    fixture.wait_terminal_contains(identity.session, "HOOK_READY");
    assert_eq!(
        fixture.request(Request::KillSession {
            session: identity.session
        }),
        Response::Ok
    );
    wait_exited_and_assert_terminal_contains(&fixture, identity.session, &["HOOK_READY"]);
    assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    fixture.shutdown_kill();
}

#[test]
fn agent_hook_round_trip_survives_dashboard_reconnect() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let identity = fixture.create_hook_child_with_report("reconnect", "agent-hook-reconnect");
    let pgid = fixture.original_pgid(identity.session);
    assert_eq!(
        fixture.session_activity(identity.session),
        ovrcr::session::AgentActivity::Busy
    );

    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 10,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let initial = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    let ServerMessage::Response {
        request_id: 10,
        response: Response::Hierarchy(snapshot),
    } = initial
    else {
        panic!("dashboard hello did not return a hierarchy: {initial:?}");
    };
    assert!(
        snapshot
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .any(|summary| summary.id == identity.session
                && summary.activity == ovrcr::session::AgentActivity::Busy)
    );
    drop(dashboard);

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: identity.session,
            text: "w".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_identity_marker("agent-hook-reconnect", "WAITING_REPORTED");

    let mut reconnect = connect_server(&fixture.socket).unwrap();
    reconnect
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut reconnect,
        &ClientMessage {
            request_id: 20,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let initial = read_frame::<ServerMessage>(&mut reconnect).unwrap();
    let ServerMessage::Response {
        request_id: 20,
        response: Response::Hierarchy(snapshot),
    } = initial
    else {
        panic!("dashboard reconnect did not return a hierarchy: {initial:?}");
    };
    assert!(
        snapshot
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .any(|summary| summary.id == identity.session
                && summary.activity == ovrcr::session::AgentActivity::WaitingInput),
        "reconnected snapshot: {snapshot:?}"
    );

    write_frame(
        &mut reconnect,
        &ClientMessage {
            request_id: 21,
            request: Request::Select {
                session: identity.session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    let screen = loop {
        if let ServerMessage::Response {
            request_id: 21,
            response: Response::Screen { bytes, .. },
        } = read_frame::<ServerMessage>(&mut reconnect).unwrap()
        {
            break bytes;
        }
    };
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(&screen);
    let text = parser.screen().contents();
    assert!(
        text.contains("HOOK_READY"),
        "helper output was not retained: {text:?}"
    );
    for forbidden in [
        "AgentReport",
        "ClientMessage",
        "ServerMessage",
        "Response",
        "OVRCR_HOOK_SOCKET",
        "WAITING_REPORTED",
    ] {
        assert!(
            !text.contains(forbidden),
            "hook protocol leaked into terminal: {text:?}"
        );
    }
    drop(reconnect);

    assert_eq!(
        fixture.request(Request::KillSession {
            session: identity.session,
        }),
        Response::Ok
    );
    wait_exited_and_assert_terminal_contains(&fixture, identity.session, &["HOOK_READY"]);
    assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    fixture.shutdown_kill();
}

#[test]
fn context_report_replaces_snapshot_and_preserves_activity() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let identity = fixture.create_hook_child("context-replace", "context-replace");
    let pgid = fixture.original_pgid(identity.session);

    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(1),
            Some("model-a"),
            Some("conversation-a"),
            Some(80),
            Some(100),
        )),
        Response::Ok
    );
    let first = fixture.session_summary(identity.session);
    assert_eq!(first.activity, ovrcr::session::AgentActivity::Unknown);
    let first_context = first.context_usage.clone().expect("first context snapshot");
    assert_eq!(
        first_context.report,
        ContextUsageReport {
            source: ContextSource::Generic,
            model: Some("model-a".into()),
            conversation: Some("conversation-a".into()),
            used_tokens: Some(80),
            capacity_tokens: Some(100),
        }
    );

    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(2),
            Some("model-b"),
            Some("conversation-b"),
            Some(5),
            Some(200),
        )),
        Response::Ok
    );
    let second = fixture.session_summary(identity.session);
    assert_eq!(second.activity, ovrcr::session::AgentActivity::Unknown);
    let second_context = second
        .context_usage
        .clone()
        .expect("second context snapshot");
    assert_eq!(
        second_context.report,
        ContextUsageReport {
            source: ContextSource::Generic,
            model: Some("model-b".into()),
            conversation: Some("conversation-b".into()),
            used_tokens: Some(5),
            capacity_tokens: Some(200),
        }
    );
    assert!(second_context.received_unix_ms >= first_context.received_unix_ms);

    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(3),
            Some("model-b"),
            Some("conversation-b"),
            None,
            None,
        )),
        Response::Ok
    );
    let third = fixture.session_summary(identity.session);
    assert_eq!(third.activity, ovrcr::session::AgentActivity::Unknown);
    let third_context = third.context_usage.expect("third context snapshot");
    assert_eq!(
        third_context.report,
        ContextUsageReport {
            source: ContextSource::Generic,
            model: Some("model-b".into()),
            conversation: Some("conversation-b".into()),
            used_tokens: None,
            capacity_tokens: None,
        }
    );
    assert!(third_context.received_unix_ms >= second_context.received_unix_ms);

    assert_eq!(
        fixture.request(Request::KillSession {
            session: identity.session,
        }),
        Response::Ok
    );
    fixture.wait_exited(identity.session);
    assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    fixture.shutdown_kill();
}

#[test]
fn context_report_rejects_old_and_invalid_samples() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let identity = fixture.create_hook_child("context-order", "context-order");
    let pgid = fixture.original_pgid(identity.session);

    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(10),
            Some("model"),
            None,
            Some(10),
            Some(100),
        )),
        Response::Ok
    );
    let initial = fixture.session_summary(identity.session);

    for rejected in [
        context_report(
            identity.session,
            identity.capability,
            Some(9),
            Some("old"),
            None,
            Some(9),
            Some(100),
        ),
        context_report(
            identity.session,
            identity.capability,
            Some(10),
            Some("replay"),
            None,
            Some(10),
            Some(100),
        ),
    ] {
        assert!(matches!(
            fixture.request(rejected),
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ));
        assert_eq!(fixture.session_summary(identity.session), initial);
    }

    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(11),
            Some("model"),
            None,
            Some(5),
            Some(100),
        )),
        Response::Ok
    );
    let lower = fixture.session_summary(identity.session);
    assert_eq!(
        lower.context_usage.as_ref().unwrap().report.used_tokens,
        Some(5)
    );
    assert!(
        lower.context_usage.as_ref().unwrap().received_unix_ms
            >= initial.context_usage.as_ref().unwrap().received_unix_ms
    );

    let before_invalid_capacity = lower.clone();
    assert!(matches!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(12),
            Some("invalid"),
            None,
            Some(12),
            Some(0),
        )),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.session_summary(identity.session),
        before_invalid_capacity
    );

    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(12),
            Some("model"),
            None,
            Some(4),
            Some(100),
        )),
        Response::Ok
    );
    let accepted = fixture.session_summary(identity.session);

    for rejected in [
        context_report(
            identity.session,
            identity.capability,
            None,
            Some("receipt-mode"),
            None,
            Some(3),
            Some(100),
        ),
        context_report(
            identity.session,
            [0xA5; 32],
            Some(13),
            Some("wrong-capability"),
            None,
            Some(3),
            Some(100),
        ),
    ] {
        assert!(matches!(
            fixture.request(rejected),
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ));
        assert_eq!(fixture.session_summary(identity.session), accepted);
    }
    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(13),
            Some("model"),
            None,
            Some(3),
            Some(100),
        )),
        Response::Ok
    );

    assert_eq!(
        fixture.request(Request::KillSession {
            session: identity.session,
        }),
        Response::Ok
    );
    fixture.wait_exited(identity.session);
    let exited = fixture.session_summary(identity.session);
    for rejected in [
        context_report(
            identity.session,
            identity.capability,
            Some(14),
            Some("exited"),
            None,
            Some(2),
            Some(100),
        ),
        context_report(
            identity.session,
            [0xA5; 32],
            Some(15),
            Some("exited-wrong-capability"),
            None,
            Some(2),
            Some(100),
        ),
    ] {
        assert!(matches!(
            fixture.request(rejected),
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ));
        assert_eq!(fixture.session_summary(identity.session), exited);
    }
    assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    fixture.shutdown_kill();
}

#[test]
fn context_snapshot_survives_dashboard_reattach() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let identity = fixture.create_hook_child("context-reconnect", "context-reconnect");
    let pgid = fixture.original_pgid(identity.session);
    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();

    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(1),
            Some("model-reconnect"),
            Some("conversation-reconnect"),
            Some(80),
            Some(100),
        )),
        Response::Ok
    );
    let changed = loop {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ServerEvent::SessionChanged(summary))
                if summary.id == identity.session =>
            {
                break summary;
            }
            _ => {}
        }
    };
    let first_retained = changed
        .context_usage
        .clone()
        .expect("context event snapshot");
    assert_eq!(
        first_retained.report.model.as_deref(),
        Some("model-reconnect")
    );
    assert_eq!(first_retained.report.used_tokens, Some(80));
    thread::sleep(Duration::from_millis(2));
    assert_eq!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(2),
            Some("model-reconnect"),
            Some("conversation-reconnect"),
            Some(80),
            Some(100),
        )),
        Response::Ok
    );
    let changed_again = loop {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ServerEvent::SessionChanged(summary))
                if summary.id == identity.session =>
            {
                break summary;
            }
            _ => {}
        }
    };
    let retained = changed_again
        .context_usage
        .clone()
        .expect("equal-count context event snapshot");
    assert_eq!(retained.report, first_retained.report);
    assert!(retained.received_unix_ms > first_retained.received_unix_ms);
    // Wait for server-side ownership cleanup before registering the replacement.
    // Dropping the local socket alone races the old connection's reader.
    dashboard.shutdown(std::net::Shutdown::Write).unwrap();
    std::io::copy(&mut dashboard, &mut std::io::sink()).unwrap();
    drop(dashboard);

    let mut reconnect = connect_server(&fixture.socket).unwrap();
    reconnect
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame(
        &mut reconnect,
        &ClientMessage {
            request_id: 2,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let ServerMessage::Response {
        request_id: 2,
        response: Response::Hierarchy(snapshot),
    } = read_frame::<ServerMessage>(&mut reconnect).unwrap()
    else {
        panic!("dashboard reconnect did not return a hierarchy");
    };
    let reattached = snapshot
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|summary| summary.id == identity.session)
        .expect("reattached session");
    assert_eq!(reattached.context_usage, Some(retained.clone()));

    assert_eq!(
        fixture.request(Request::KillSession {
            session: identity.session,
        }),
        Response::Ok
    );
    let exited = loop {
        match read_frame::<ServerMessage>(&mut reconnect).unwrap() {
            ServerMessage::Event(ServerEvent::SessionChanged(summary))
                if summary.id == identity.session
                    && matches!(summary.phase, SessionPhase::Exited { .. }) =>
            {
                break summary;
            }
            _ => {}
        }
    };
    assert_eq!(exited.context_usage, Some(retained.clone()));
    assert!(context_is_stale(&retained, retained.received_unix_ms, true));
    assert!(matches!(
        fixture.request(context_report(
            identity.session,
            identity.capability,
            Some(3),
            Some("late"),
            None,
            Some(1),
            Some(100),
        )),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.session_summary(identity.session).context_usage,
        Some(retained)
    );
    drop(reconnect);
    fixture.wait_exited(identity.session);
    assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    fixture.shutdown_kill();
}

#[test]
fn control_fixture_failure_cleanup_reaps_owned_child_and_server() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    let socket = fixture.socket.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = fixture.create_hook_child("failed-ack", "agent-hook-failure");
        assert_eq!("HOOK_READY", "missing acknowledgement");
    }));
    assert!(result.is_err());
    let pgid = fixture
        .process_groups
        .lock()
        .unwrap()
        .last()
        .copied()
        .expect("failed handshake child ownership was recorded");
    drop(fixture);
    assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    assert!(!socket.exists());
}

fn create_private_dir(path: &Path) {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .unwrap();
}

#[test]
fn startup_keeps_shared_existing_socket_directory_and_secures_the_socket() {
    use std::os::unix::fs::DirBuilderExt;
    let mut fixture = ServerFixture::new();
    let parent = fixture.paths.socket.parent().unwrap().to_path_buf();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(&parent)
        .unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    fixture.start();
    let dir_mode = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        dir_mode, 0o755,
        "startup must not modify a directory it did not create"
    );
    let socket_mode = std::fs::metadata(&fixture.paths.socket)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(socket_mode, 0o700, "the socket file itself must be private");
    fixture.stop();
}

#[test]
fn startup_refuses_symlinked_socket_directory() {
    let fixture = ServerFixture::new();
    let target = fixture.root.path().join("real");
    create_private_dir(&target);
    std::os::unix::fs::symlink(&target, fixture.paths.socket.parent().unwrap()).unwrap();
    let registry = fixture.root.path().join("config.toml");
    save_registry_atomic(&Registry::default(), &registry).unwrap();
    let error = run_server(fixture.paths.clone(), registry).unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("server socket directory") && message.contains("symlink"),
        "unexpected error: {message}"
    );
    assert!(!fixture.paths.socket.exists());
}

#[test]
fn startup_stale_socket_is_recovered() {
    let mut fixture = ServerFixture::new();
    create_private_dir(fixture.paths.socket.parent().unwrap());
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
    // Held until the variables are removed again: every sibling that points the
    // CLI at its own socket takes this lock, and a parallel test that spawns a
    // child would otherwise inherit this fixture's socket.
    let env_guard = env_lock();
    let fixture = ServerFixture::new();
    let registry = fixture.root.path().join("config.toml");
    save_registry_atomic(&Registry::default(), &registry).unwrap();
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
    drop(env_guard);
    let deadline = Instant::now() + Duration::from_secs(2);
    while fixture.paths.socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    assert!(!fixture.paths.socket.exists());
}

#[test]
fn startup_failure_reports_server_log() {
    let _env_lock = env_lock();
    let fixture = ServerFixture::new();
    let registry = fixture.root.path().join("config.toml");
    std::fs::write(&registry, "[[projects]\n").unwrap();
    unsafe {
        std::env::set_var("OVRCR_SERVER_EXECUTABLE", env!("CARGO_BIN_EXE_ovrcr"));
        std::env::set_var("OVRCR_SOCKET", &fixture.paths.socket);
        std::env::set_var("OVRCR_CONFIG", &registry);
    }
    let started = Instant::now();
    let result = connect_or_start(&fixture.paths);
    let elapsed = started.elapsed();
    unsafe {
        std::env::remove_var("OVRCR_SERVER_EXECUTABLE");
        std::env::remove_var("OVRCR_SOCKET");
        std::env::remove_var("OVRCR_CONFIG");
    }
    let error = format!("{:#}", result.expect_err("startup must fail"));
    let log = fixture.paths.socket.parent().unwrap().join("server.log");
    assert!(
        error.contains("parse registry"),
        "error must carry the server's failure: {error}"
    );
    assert!(
        error.contains(&log.display().to_string()),
        "error must name the log: {error}"
    );
    assert!(
        error.contains("exited during startup"),
        "a dead server must be reported immediately: {error}"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "startup failure must not wait for the timeout, took {elapsed:?}"
    );
    let mode = std::fs::metadata(&log).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("parse registry")
    );
    assert!(!fixture.paths.socket.exists());
}

#[test]
fn client_with_wrong_protocol_version_is_refused() {
    use ovrcr::protocol::{PROTOCOL_VERSION, read_preamble};
    let mut fixture = ServerFixture::new();
    fixture.start();

    // A newer client: the server answers with its own preamble, then closes
    // the connection without serving a frame.
    let mut stream = UnixStream::connect(&fixture.paths.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut preamble = Vec::from(*b"OVRC");
    preamble.extend_from_slice(&(PROTOCOL_VERSION + 1).to_be_bytes());
    stream.write_all(&preamble).unwrap();
    assert_eq!(read_preamble(&mut stream).unwrap(), PROTOCOL_VERSION);
    // The server closes the connection, so either the frame write or the
    // response read fails; neither may succeed.
    let served = write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 1,
            request: Request::List,
        },
    )
    .and_then(|()| read_frame::<ServerMessage>(&mut stream));
    assert!(
        served.is_err(),
        "server must not serve a client speaking another protocol version"
    );
    drop(stream);

    // A newer server: the client reports both versions and advises restarting.
    let peer = fixture.root.path().join("newer.sock");
    let listener = UnixListener::bind(&peer).unwrap();
    let fake_server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut answer = Vec::from(*b"OVRC");
        answer.extend_from_slice(&(PROTOCOL_VERSION + 1).to_be_bytes());
        stream.write_all(&answer).unwrap();
        let _ = read_preamble(&mut stream);
    });
    let error = connect_if_running(&ServerPaths { socket: peer })
        .expect_err("version mismatch must be an error");
    let message = format!("{error:#}");
    assert!(message.contains("protocol version mismatch"), "{message}");
    assert!(
        message.contains(&format!("version {}", PROTOCOL_VERSION + 1))
            && message.contains("shutdown --kill"),
        "{message}"
    );
    fake_server.join().unwrap();

    // An older server that never sends a preamble closes on the client's.
    let peer = fixture.root.path().join("older.sock");
    let listener = UnixListener::bind(&peer).unwrap();
    let old_server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(stream);
    });
    let error = connect_if_running(&ServerPaths { socket: peer })
        .expect_err("closed handshake must be an error");
    assert!(
        format!("{error:#}").contains("older OVRCR build"),
        "{error:#}"
    );
    old_server.join().unwrap();

    fixture.stop();
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

#[test]
fn startup_stale_concurrent_attempts_leave_one_surviving_server() {
    let fixture = ServerFixture::new();
    let registry = fixture.root.path().join("config.toml");
    save_registry_atomic(&Registry::default(), &registry).unwrap();
    create_private_dir(fixture.paths.socket.parent().unwrap());
    let stale = UnixListener::bind(&fixture.paths.socket).unwrap();
    drop(stale);
    let executable = env!("CARGO_BIN_EXE_ovrcr");
    let mut workers: Vec<Child> = Vec::new();
    for _ in 0..2 {
        workers.push(
            Command::new(executable)
                .arg("server")
                .env("OVRCR_SOCKET", &fixture.paths.socket)
                .env("OVRCR_CONFIG", &registry)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    assert_ne!(workers[0].id(), workers[1].id());
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut exited = 0;
    while Instant::now() < deadline {
        exited = 0;
        for worker in &mut workers {
            if worker.try_wait().unwrap().is_some() {
                exited += 1;
            }
        }
        if exited == 1 {
            break;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    assert_eq!(
        exited, 1,
        "exactly one detached server owner must survive startup"
    );
    let mut first = connect_server(&fixture.paths.socket).unwrap();
    write_frame(
        &mut first,
        &ClientMessage {
            request_id: 3,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut first).unwrap(),
        ServerMessage::Response {
            response: Response::Ok,
            ..
        }
    ));
    drop(first);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if workers
            .iter_mut()
            .all(|worker| worker.try_wait().unwrap().is_some())
        {
            break;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    for worker in &mut workers {
        if worker.try_wait().unwrap().is_none() {
            worker.kill().unwrap();
            worker.wait().unwrap();
        }
    }
    assert!(
        workers
            .iter_mut()
            .all(|worker| worker.try_wait().unwrap().is_some())
    );
    assert!(
        !fixture.paths.socket.exists(),
        "shutdown must stop the sole server"
    );
}

#[test]
fn shutdown_disconnected_requester_still_wakes_accept() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut stream = connect_server(&fixture.paths.socket).unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 9,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    let deadline = Instant::now() + Duration::from_millis(250);
    while fixture.paths.socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    let completed_without_interference = !fixture.paths.socket.exists();
    if !completed_without_interference {
        let _ = UnixStream::connect(&fixture.paths.socket);
    }
    fixture.thread.take().unwrap().join().unwrap();
    assert!(
        completed_without_interference,
        "disconnected shutdown left accept blocked"
    );
}

fn with_kill_grace_ms<T>(value: &str, body: impl FnOnce() -> T) -> T {
    let previous = std::env::var_os("OVRCR_KILL_GRACE_MS");
    unsafe { std::env::set_var("OVRCR_KILL_GRACE_MS", value) };
    let result = body();
    match previous {
        Some(value) => unsafe { std::env::set_var("OVRCR_KILL_GRACE_MS", value) },
        None => unsafe { std::env::remove_var("OVRCR_KILL_GRACE_MS") },
    }
    result
}

fn register_fixture_workspace(fixture: &ControlFixture, branch: &str) {
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: branch.into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
}

fn stubborn_session_argv() -> Vec<OsString> {
    vec![
        "sh".into(),
        "-c".into(),
        "trap '' HUP TERM; printf READY; while :; do sleep 1; done".into(),
    ]
}

#[test]
fn workspace_remove_succeeds_after_directory_deleted() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    register_fixture_workspace(&fixture, "feature/vanished-dir");
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::CloseTerminal { session: local }),
        Response::Ok
    );
    let workspace_dir = fixture.workspace_root.join("work");
    std::fs::remove_dir_all(&workspace_dir).unwrap();

    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
        }),
        Response::Ok
    );
    let listed = String::from_utf8(
        fixture
            .git_output(&["worktree", "list", "--porcelain"])
            .stdout,
    )
    .unwrap();
    assert!(
        !listed
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .any(|path| path.ends_with("/work")),
        "Git must no longer list the worktree: {listed}"
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn shutdown_without_kill_rejects_exited_record() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    register_fixture_workspace(&fixture, "feature/exited-record");
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::CloseTerminal { session: local }),
        Response::Ok
    );
    let exited = fixture.create_session("exits", vec!["sh".into(), "-c".into(), "exit 0".into()]);
    fixture.wait_exited(exited);
    // An exited record still awaiting removal keeps the final screen; a
    // non-kill shutdown must refuse just as it does for a live session.
    assert!(matches!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::RemoveSession { session: exited }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn shutdown_kill_terminates_sessions_concurrently() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    register_fixture_workspace(&fixture, "feature/concurrent-shutdown");
    // Sessions that ignore SIGHUP and SIGTERM force the full grace period
    // before SIGKILL, so a serial shutdown would cost sessions x grace.
    let mut pgids = Vec::new();
    for index in 0..6 {
        let summary =
            fixture.create_session_summary(&format!("stubborn-{index}"), stubborn_session_argv());
        pgids.push(summary.pid.unwrap() as libc::pid_t);
    }
    let started = Instant::now();
    let response = with_kill_grace_ms("500", || fixture.request(Request::Shutdown { kill: true }));
    let elapsed = started.elapsed();
    assert_eq!(response, Response::Ok);
    assert!(
        elapsed < Duration::from_millis(2_000),
        "shutdown --kill should terminate sessions concurrently, took {elapsed:?}"
    );
    for pgid in pgids {
        wait_for_group_absent(pgid, Duration::from_secs(3));
    }
    fixture.join();
}

#[test]
fn kill_does_not_block_dashboard_geometry() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    register_fixture_workspace(&fixture, "feature/unlocked-kill");
    let stubborn = fixture.create_session("stubborn", stubborn_session_argv());

    // A dashboard registers before the kill starts.
    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            response: Response::Hierarchy(_),
            ..
        }
    ));

    // The kill waits out a two-second grace period on another connection.
    let socket = fixture.socket.clone();
    let killer = thread::spawn(move || {
        with_kill_grace_ms("2000", || {
            let mut stream = connect_server(&socket).unwrap();
            write_frame(
                &mut stream,
                &ClientMessage {
                    request_id: 7,
                    request: Request::KillSession { session: stubborn },
                },
            )
            .unwrap();
            read_frame::<ServerMessage>(&mut stream).unwrap()
        })
    });
    thread::sleep(Duration::from_millis(300));

    let started = Instant::now();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 2,
            request: Request::DashboardGeometry {
                size: ovrcr::session::TerminalSize {
                    rows: 30,
                    cols: 100,
                },
            },
        },
    )
    .unwrap();
    let reply = loop {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Response {
                request_id: 2,
                response,
            } => break response,
            ServerMessage::Event(_) => continue,
            other => panic!("unexpected dashboard message: {other:?}"),
        }
    };
    let elapsed = started.elapsed();
    assert_eq!(reply, Response::Ok);
    assert!(
        elapsed < Duration::from_millis(500),
        "dashboard geometry must not wait for an in-flight kill, took {elapsed:?}"
    );
    assert!(matches!(
        killer.join().unwrap(),
        ServerMessage::Response {
            request_id: 7,
            response: Response::Ok
        }
    ));
    drop(dashboard);
    fixture.wait_exited(stubborn);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: stubborn }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::KillSession { session: local }),
        Response::Ok
    );
    fixture.wait_exited(local);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn backpressured_input_and_send_do_not_block_inspect_or_kill() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/backpressure".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session(
        "blocked",
        vec![
            "sh".into(),
            "-c".into(),
            "stty raw -echo; printf READY; exec sleep 30".into(),
        ],
    );
    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    let mut ready = false;
    dashboard
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let readiness_deadline = Instant::now() + Duration::from_secs(2);
    let mut request_id = 2;
    while !ready && Instant::now() < readiness_deadline {
        write_frame(
            &mut dashboard,
            &ClientMessage {
                request_id,
                request: Request::Select {
                    session,
                    size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
                },
            },
        )
        .unwrap();
        request_id += 1;
        let Ok(message) = read_frame::<ServerMessage>(&mut dashboard) else {
            continue;
        };
        match message {
            ServerMessage::Response {
                response: Response::Screen { bytes, .. },
                ..
            }
            | ServerMessage::Event(ovrcr::protocol::ServerEvent::Output {
                bytes,
                session: _,
                revision: _,
            }) if String::from_utf8_lossy(&bytes).contains("READY") => {
                ready = true;
                break;
            }
            _ => {}
        }
    }
    assert!(ready, "blocked-session readiness marker was not rendered");
    dashboard
        .set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();
    while read_frame::<ServerMessage>(&mut dashboard).is_ok() {}
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 20,
            request: Request::Input {
                session,
                bytes: vec![b'x'; 512 * 1024],
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    assert!(
        read_frame::<ServerMessage>(&mut dashboard).is_err(),
        "input response unexpectedly completed while PTY stdin was backpressured"
    );

    let mut blocked_send = connect_server(&fixture.socket).unwrap();
    blocked_send
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    write_frame(
        &mut blocked_send,
        &ClientMessage {
            request_id: 20,
            request: Request::SendTerminal {
                session,
                text: "queued behind blocked input".into(),
                submit: false,
            },
        },
    )
    .unwrap();
    assert!(
        read_frame::<ServerMessage>(&mut blocked_send).is_err(),
        "SendTerminal unexpectedly completed while PTY stdin was backpressured"
    );

    let mut inspect = connect_server(&fixture.socket).unwrap();
    inspect
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut inspect,
        &ClientMessage {
            request_id: 21,
            request: Request::Inspect,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut inspect).unwrap(),
        ServerMessage::Response {
            request_id: 21,
            response: Response::Inventory { .. },
        }
    ));
    drop(inspect);

    let mut control = connect_server(&fixture.socket).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut control,
        &ClientMessage {
            request_id: 21,
            request: Request::List,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut control).unwrap(),
        ServerMessage::Response {
            request_id: 21,
            response: Response::Hierarchy(_),
        }
    ));
    drop(control);
    let mut control = connect_server(&fixture.socket).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut control,
        &ClientMessage {
            request_id: 22,
            request: Request::KillSession { session },
        },
    )
    .unwrap();
    assert_eq!(
        read_frame::<ServerMessage>(&mut control).unwrap(),
        ServerMessage::Response {
            request_id: 22,
            response: Response::Ok,
        }
    );
    drop(blocked_send);
    drop(dashboard);
    fixture.request(Request::Shutdown { kill: true });
    fixture.join();
}

#[test]
fn control_lifecycle_enforces_every_removal_gate() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert!(matches!(
        fixture.request(Request::RemoveProject {
            name: "duplicate".into()
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    fixture.request(Request::AddProject {
        name: "fixture".into(),
        repo: fixture.repo.clone(),
        workspace_root: fixture.workspace_root.clone(),
    });
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/test".into(),
                base: "main".into()
            }
        }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    let review = fixture.create_session("review", vec!["sh".into(), "-c".into(), "exit 0".into()]);
    assert!(matches!(
        fixture.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(),
            workspace: "work".into(),
            name: "review".into(),
            label: None,
            argv: vec!["sh".into()]
        })),
        Response::Error {
            code: ErrorCode::AlreadyExists,
            ..
        }
    ));
    assert!(matches!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::KillSession { session: local }),
        Response::Ok
    );
    fixture.wait_exited(local);
    fixture.wait_exited(review);
    assert!(matches!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::RemoveSession { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveSession { session: review }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Ok
    );
    assert!(
        fixture
            .git_output(&["show-ref", "--verify", "refs/heads/feature/test"])
            .status
            .success()
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn workspace_shell_failure_retains_worktree_and_registry() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    fixture.request(Request::AddProject {
        name: "fixture".into(),
        repo: fixture.repo.clone(),
        workspace_root: fixture.workspace_root.clone(),
    });
    let old_shell = std::env::var_os("SHELL");
    unsafe {
        std::env::set_var("SHELL", "/ovrcr/no-such-shell");
    }
    let response = fixture.request(Request::CreateWorkspace {
        project: "fixture".into(),
        name: "failed".into(),
        branch: BranchRequest::New {
            branch: "feature/failed".into(),
            base: "main".into(),
        },
    });
    match old_shell {
        Some(value) => unsafe { std::env::set_var("SHELL", value) },
        None => unsafe { std::env::remove_var("SHELL") },
    }
    assert!(
        matches!(response, Response::Error { code: ErrorCode::PartialFailure, message } if message.contains("failed") && message.contains("worktree"))
    );
    let listed = fixture.request(Request::List);
    assert!(
        matches!(listed, Response::Hierarchy(ref snapshot) if snapshot.projects[0].workspaces.iter().any(|workspace| workspace.name == "failed" && workspace.sessions.is_empty()))
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "failed".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn fast_exit_session_is_retained_as_exited() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/fast-exit".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let fast = fixture.create_session(
        "fast",
        vec!["sh".into(), "-c".into(), "printf retained".into()],
    );
    fixture.wait_exited(fast);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: fast }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::KillSession { session: local }),
        Response::Ok
    );
    fixture.wait_exited(local);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into()
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn pause_resume_server_refuses_removal_and_late_mutation() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/pause-resume".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session("pause-resume", vec!["sh".into()]);
    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 9,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();

    assert_eq!(
        fixture.request(Request::PauseSession { session }),
        Response::Ok
    );
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Event(ovrcr::protocol::ServerEvent::SessionChanged(summary))
                if summary.id == session && matches!(summary.phase, SessionPhase::Paused)
        ) {
            break;
        }
    }
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(ref snapshot)
            if snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|summary| summary.id == session && matches!(summary.phase, SessionPhase::Paused))
    ));
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 10,
            request: Request::Select {
                session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 10,
                response: Response::Screen { .. },
            }
        ) {
            break;
        }
    }
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 11,
            request: Request::Input {
                session,
                bytes: b"paused".to_vec(),
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 11,
                response: Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                },
            }
        ) {
            break;
        }
    }
    assert!(matches!(
        fixture.request(Request::RemoveSession { session }),
        Response::Error {
            code: ErrorCode::SessionRunning,
            ..
        }
    ));
    assert!(matches!(
        fixture.request(Request::SendTerminal {
            session,
            text: "paused".into(),
            submit: true,
        }),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        fixture.request(Request::ResumeSession { session }),
        Response::Ok
    );
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Event(ovrcr::protocol::ServerEvent::SessionChanged(summary))
                if summary.id == session && matches!(summary.phase, SessionPhase::Running)
        ) {
            break;
        }
    }
    assert!(matches!(
        fixture.request(Request::List),
        Response::Hierarchy(ref snapshot)
            if snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|summary| summary.id == session && matches!(summary.phase, SessionPhase::Running))
    ));

    assert_eq!(
        fixture.request(Request::KillSession { session }),
        Response::Ok
    );
    fixture.wait_exited(session);
    assert_eq!(
        fixture.request(Request::RemoveSession { session }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::KillSession { session: local }),
        Response::Ok
    );
    fixture.wait_exited(local);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    drop(dashboard);
    fixture.join();
}

struct PausePeer {
    pid: libc::pid_t,
    pgid: libc::pid_t,
    address: PathBuf,
    preexit_marker: String,
}

struct PauseHarness {
    fixture: ControlFixture,
    dir: PathBuf,
    control_path: PathBuf,
    control: UnixDatagram,
    pgids: Vec<libc::pid_t>,
    next_endpoint: usize,
}

impl PauseHarness {
    fn new() -> Self {
        let fixture = ControlFixture::new_bounded();
        assert_eq!(
            fixture.request(Request::AddProject {
                name: "fixture".into(),
                repo: fixture.repo.clone(),
                workspace_root: fixture.workspace_root.clone(),
            }),
            Response::Ok
        );
        assert_eq!(
            fixture.request(Request::CreateWorkspace {
                project: "fixture".into(),
                name: "work".into(),
                branch: BranchRequest::New {
                    branch: "feature/task4-pause-resume".into(),
                    base: "main".into(),
                },
            }),
            Response::Ok
        );
        let local = fixture.only_session_id();
        assert_eq!(
            fixture.request(Request::KillSession { session: local }),
            Response::Ok
        );
        fixture.wait_exited(local);
        assert_eq!(
            fixture.request(Request::RemoveSession { session: local }),
            Response::Ok
        );

        let dir = fixture._root.path().join("pause-resume");
        std::fs::create_dir(&dir).unwrap();
        let control_path = dir.join("parent.sock");
        let control = UnixDatagram::bind(&control_path).unwrap();
        control
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        Self {
            fixture,
            dir,
            control_path,
            control,
            pgids: Vec::new(),
            next_endpoint: 0,
        }
    }

    fn create_session(&mut self, name: &str) -> (SessionId, PausePeer, PausePeer) {
        self.create_session_with_options(name, false)
    }

    fn create_session_with_options(
        &mut self,
        name: &str,
        ignore_sighup: bool,
    ) -> (SessionId, PausePeer, PausePeer) {
        let endpoint_dir = self.dir.join(format!("session-{}", self.next_endpoint));
        let preexit_marker = format!("DESCENDANT_PREEXIT_MARKER_{}", self.next_endpoint);
        self.next_endpoint += 1;
        std::fs::create_dir(&endpoint_dir).unwrap();
        let summary = self.fixture.create_session_summary(
            name,
            pause_session_argv(
                &endpoint_dir,
                &self.control_path,
                &preexit_marker,
                ignore_sighup,
            ),
        );
        self.record_created_pgid(&summary);
        let session = summary.id;
        let first = recv_pause_ready(&self.control);
        let second = recv_pause_ready(&self.control);
        assert_eq!(
            first.pgid, second.pgid,
            "leader and descendant lost PGID ownership"
        );
        assert_eq!(
            first.preexit_marker, second.preexit_marker,
            "leader and descendant do not share the pre-exit marker"
        );
        assert_eq!(
            unsafe { libc::getpgid(first.pid) },
            first.pgid,
            "leader READY reported a stale PGID"
        );
        assert_eq!(
            unsafe { libc::getpgid(second.pid) },
            second.pgid,
            "descendant READY reported a stale PGID"
        );
        let (leader, descendant) = if first
            .address
            .file_name()
            .is_some_and(|name| name == "leader.sock")
        {
            (first, second)
        } else {
            (second, first)
        };
        (session, leader, descendant)
    }

    fn record_created_pgid(&mut self, summary: &ovrcr::session::SessionSummary) {
        let pid = summary
            .pid
            .expect("successful session creation must report a process ID");
        // Session::spawn requires the PTY leader to own its process group, so
        // the creation response gives us the group ID before any handshake
        // assertions can fail.
        self.pgids.push(pid as libc::pid_t);
    }

    fn finish(&self) {
        if self.fixture.thread.lock().unwrap().is_some() {
            assert_eq!(
                request_with_timeout(
                    &self.fixture.socket,
                    901,
                    Request::Shutdown { kill: true },
                    Duration::from_secs(5),
                ),
                Some(Response::Ok)
            );
            assert!(
                self.join_bounded(Duration::from_secs(2)),
                "control server did not finish within cleanup deadline"
            );
        }
    }

    fn join_bounded(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let finished = self
                .fixture
                .thread
                .lock()
                .unwrap()
                .as_ref()
                .map(|handle| handle.is_finished())
                .unwrap_or(true);
            if finished {
                let handle = self.fixture.thread.lock().unwrap().take();
                return handle.map(|handle| handle.join().is_ok()).unwrap_or(true);
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
    }
}

impl Drop for PauseHarness {
    fn drop(&mut self) {
        let mut cleanup_failed = false;
        if self.fixture.thread.lock().unwrap().is_some() {
            if request_with_timeout(
                &self.fixture.socket,
                999,
                Request::Shutdown { kill: true },
                Duration::from_secs(2),
            ) != Some(Response::Ok)
            {
                cleanup_failed = true;
            }
            for pgid in &self.pgids {
                unsafe {
                    libc::kill(-*pgid, libc::SIGKILL);
                }
            }
            if !self.join_bounded(Duration::from_secs(2)) {
                cleanup_failed = true;
            }
        }
        for pgid in &self.pgids {
            if !wait_group_absent(*pgid, Duration::from_secs(2)) {
                cleanup_failed = true;
            }
        }
        if cleanup_failed {
            let kept =
                std::mem::replace(&mut self.fixture._root, tempfile::tempdir().unwrap()).keep();
            eprintln!(
                "pause/resume fixture cleanup failed; preserved {}",
                kept.display()
            );
        }
    }
}

fn pause_session_argv(
    dir: &Path,
    parent: &Path,
    preexit_marker: &str,
    ignore_sighup: bool,
) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("env"),
        OsString::from("OVRCR_PAUSE_ROLE=leader"),
        OsString::from(format!("OVRCR_PAUSE_DIR={}", dir.display())),
        OsString::from(format!("OVRCR_PAUSE_PARENT={}", parent.display())),
        OsString::from(format!("OVRCR_PAUSE_PREEXIT_MARKER={preexit_marker}")),
    ];
    if ignore_sighup {
        argv.push(OsString::from("OVRCR_PAUSE_IGNORE_SIGHUP=1"));
    }
    argv.extend([
        std::env::current_exe().unwrap().into_os_string(),
        "--ignored".into(),
        "--exact".into(),
        "pause_resume_child_fixture".into(),
        "--nocapture".into(),
    ]);
    argv
}

fn request_with_timeout(
    socket: &Path,
    request_id: u64,
    request: Request,
    timeout: Duration,
) -> Option<Response> {
    let mut stream = connect_server(socket).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    client::request(&mut stream, request_id, request).ok()
}

fn dashboard_for_session(socket: &Path, session: SessionId) -> (UnixStream, Vec<u8>) {
    let mut dashboard = connect_server(socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    dashboard
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    let screen = dashboard_select(&mut dashboard, 2, session);
    (dashboard, screen)
}

fn dashboard_select(stream: &mut UnixStream, request_id: u64, session: SessionId) -> Vec<u8> {
    write_frame(
        stream,
        &ClientMessage {
            request_id,
            request: Request::Select {
                session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    let mut screen = None;
    loop {
        match read_frame::<ServerMessage>(stream).unwrap() {
            ServerMessage::Response {
                request_id: id,
                response: Response::Screen { bytes, .. },
            } if id == request_id => screen = Some(bytes),
            ServerMessage::Response {
                request_id: id,
                response: Response::Ok,
            } if id == request_id => return screen.expect("select screen before acknowledgement"),
            ServerMessage::Response {
                request_id: id,
                response: Response::Error { message, .. },
            } if id == request_id => panic!("select failed: {message}"),
            _ => {}
        }
    }
}

fn dashboard_request(stream: &mut UnixStream, request_id: u64, request: Request) -> Response {
    client::request(stream, request_id, request).unwrap()
}

struct HistoryDashboardParser {
    screens: std::collections::HashMap<SessionId, vt100::Parser>,
}

struct HistoryFrameReader {
    header: [u8; 4],
    header_len: usize,
    body: Vec<u8>,
    body_len: usize,
}

impl HistoryFrameReader {
    fn new() -> Self {
        Self {
            header: [0; 4],
            header_len: 0,
            body: Vec::new(),
            body_len: 0,
        }
    }

    fn read_some(
        stream: &mut UnixStream,
        bytes: &mut [u8],
        deadline: Instant,
    ) -> std::io::Result<Option<usize>> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            stream.set_read_timeout(Some(remaining))?;
            match stream.read(bytes) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "dashboard closed while reading a history frame",
                    ));
                }
                Ok(read) => return Ok(Some(read)),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) && Instant::now() < deadline => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn next(
        &mut self,
        stream: &mut UnixStream,
        deadline: Instant,
    ) -> std::io::Result<Option<ServerMessage>> {
        while self.header_len < self.header.len() {
            let read = match Self::read_some(stream, &mut self.header[self.header_len..], deadline)?
            {
                Some(read) => read,
                None => return Ok(None),
            };
            self.header_len += read;
        }

        let frame_len = u32::from_be_bytes(self.header) as usize;
        if frame_len > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("history frame too large: {frame_len} bytes"),
            ));
        }
        if self.body.len() != frame_len {
            self.body.resize(frame_len, 0);
        }
        while self.body_len < frame_len {
            let read = match Self::read_some(stream, &mut self.body[self.body_len..], deadline)? {
                Some(read) => read,
                None => return Ok(None),
            };
            self.body_len += read;
        }

        let mut frame = Vec::with_capacity(4 + frame_len);
        frame.extend_from_slice(&self.header);
        frame.extend_from_slice(&self.body);
        let message = read_frame::<ServerMessage>(&mut Cursor::new(frame)).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        self.header_len = 0;
        self.body.clear();
        self.body_len = 0;
        Ok(Some(message))
    }
}

struct HistoryConnection {
    stream: UnixStream,
    frames: HistoryFrameReader,
    parser: HistoryDashboardParser,
}

impl HistoryConnection {
    fn connect(socket: &Path) -> Self {
        Self {
            stream: connect_server(socket).unwrap(),
            frames: HistoryFrameReader::new(),
            parser: HistoryDashboardParser::new(),
        }
    }

    fn next(&mut self, deadline: Instant) -> std::io::Result<Option<ServerMessage>> {
        let message = self.frames.next(&mut self.stream, deadline)?;
        if let Some(message) = &message {
            self.parser.forward(message);
        }
        Ok(message)
    }
}

impl HistoryDashboardParser {
    fn new() -> Self {
        Self {
            screens: std::collections::HashMap::new(),
        }
    }

    fn forward(&mut self, message: &ServerMessage) {
        if let ServerMessage::Event(ServerEvent::Output {
            session,
            bytes,
            revision: _,
        }) = message
        {
            self.screens
                .entry(*session)
                .or_insert_with(|| vt100::Parser::new(24, 80, HISTORY_ROWS))
                .process(bytes);
        }
    }
}

fn history_request(connection: &mut HistoryConnection, id: u64, request: Request) -> Response {
    let waits_for_screen = matches!(&request, Request::Select { .. });
    write_frame(
        &mut connection.stream,
        &ClientMessage {
            request_id: id,
            request,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut screen = None;
    loop {
        assert!(
            Instant::now() < deadline,
            "history response exceeded two-second deadline"
        );
        match connection.next(deadline).unwrap_or_else(|error| {
            panic!("history request {id} failed: {error}");
        }) {
            None => panic!("history response {id} exceeded two-second deadline"),
            Some(ServerMessage::Response {
                request_id,
                response,
            }) if request_id == id => {
                if waits_for_screen {
                    match response {
                        response @ Response::Screen { .. } => screen = Some(response),
                        Response::Ok => {
                            return screen.expect("select screen before acknowledgement");
                        }
                        response @ Response::Error { .. } => return response,
                        response => panic!("select returned {response:?} before screen"),
                    }
                } else {
                    return response;
                }
            }
            Some(_) => {}
        }
    }
}

fn history_response_size(id: u64, response: &Response) -> usize {
    let mut frame = Vec::new();
    write_frame(
        &mut frame,
        &ServerMessage::Response {
            request_id: id,
            response: response.clone(),
        },
    )
    .unwrap();
    u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize
}

fn history_page_all(
    connection: &mut HistoryConnection,
    session: SessionId,
    snapshot: HistorySnapshotId,
    opened: &HistoryOpened,
    next_id: &mut u64,
) -> Vec<HistoryRow> {
    let mut rows = Vec::new();
    let mut start_row = 0;
    while start_row < opened.total_rows {
        let count =
            u16::try_from((opened.total_rows - start_row).min(u32::from(PAGE_ROWS))).unwrap();
        let id = *next_id;
        *next_id += 1;
        let response = history_request(
            connection,
            id,
            Request::HistoryPage {
                session,
                snapshot,
                start_row,
                rows: count,
                start_col: 0,
                cols: PAGE_COLS,
            },
        );
        assert!(
            history_response_size(id, &response) <= PAGE_BYTES,
            "history page response exceeded {PAGE_BYTES} bytes"
        );
        let Response::HistoryRows(page) = response else {
            panic!("history page returned {response:?}");
        };
        assert_eq!(page.start_row, start_row);
        assert!(!page.rows.is_empty() || start_row == opened.total_rows);
        start_row += u32::try_from(page.rows.len()).unwrap();
        rows.extend(page.rows);
    }
    rows
}

fn history_rows_text(rows: &[HistoryRow]) -> String {
    rows.iter()
        .flat_map(|row| row.cells.iter())
        .map(|cell| cell.text.as_str())
        .collect::<String>()
}

fn history_workspace(fixture: &ControlFixture, branch: &str) {
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: branch.into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
}

fn history_ready_shell(fixture: &ControlFixture) -> SessionId {
    let summary = fixture.create_session_summary(
        "history-ready",
        vec![
            "sh".into(),
            "-c".into(),
            "printf 'OLD_VISIBLE\nHISTORY_READY\n'; read gate; i=0; while [ \"$i\" -lt 600 ]; do printf 'NEW_%04d\n' \"$i\"; i=$((i+1)); done; printf 'DETACHED_OFFSCREEN_000\n'; i=0; while [ \"$i\" -lt 40 ]; do printf 'DETACHED_TAIL_%03d\n' \"$i\"; i=$((i+1)); done; printf 'FINAL_HISTORY_MARKER\n'".into(),
        ],
    );
    fixture.record_process_group(&summary);
    summary.id
}

fn history_memory_shell(index: usize, cols: u16) -> Vec<OsString> {
    vec![
        "sh".into(),
        "-c".into(),
        r#"
cols=$1
index=$2
printf 'HISTORY_MEMORY_READY_%s\n' "$index"
IFS= read -r gate
fill=X
while [ "${#fill}" -lt "$cols" ]; do fill="${fill}X"; done
i=0
while [ "$i" -lt 700 ]; do
  printf '%s\n' "$fill"
  i=$((i+1))
done
printf 'HISTORY_MEMORY_DONE_1_%s\n' "$index"
IFS= read -r gate
i=0
while [ "$i" -lt 700 ]; do
  printf '%s\n' "$fill"
  i=$((i+1))
done
printf 'HISTORY_MEMORY_DONE_2_%s\n' "$index"
while :; do sleep 1; done
"#
        .into(),
        "ovrcr-history-memory".into(),
        cols.to_string().into(),
        index.to_string().into(),
    ]
}

fn current_rss_kib() -> u64 {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status")
        && let Some(value) = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:")?.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
    {
        return value;
    }
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("current RSS ps query");
    assert!(output.status.success(), "current RSS ps query failed");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("current RSS ps output is KiB")
}

fn drain_dashboard_events(connection: &mut HistoryConnection) {
    let deadline = Instant::now() + Duration::from_millis(2);
    loop {
        match connection.next(deadline) {
            Ok(Some(ServerMessage::Event(_))) => {}
            Ok(Some(message)) => {
                panic!("unexpected dashboard response while draining: {message:?}")
            }
            Ok(None) => break,
            Err(error) => panic!("dashboard event drain failed: {error}"),
        }
    }
}

fn wait_memory_marker(
    fixture: &ControlFixture,
    dashboard: &mut HistoryConnection,
    session: SessionId,
    marker: &str,
    deadline: Instant,
) {
    while Instant::now() < deadline {
        drain_dashboard_events(dashboard);
        if let Some(Response::TerminalText { text, .. }) = request_with_timeout(
            &fixture.socket,
            700,
            Request::ReadTerminal {
                session,
                max_lines: None,
            },
            Duration::from_millis(250),
        ) && text.contains(marker)
        {
            return;
        }
        thread::yield_now();
    }
    panic!("session {session:?} did not produce memory marker {marker:?}");
}

fn history_memory_probe_all(
    dashboard: &mut HistoryConnection,
    sessions: &[SessionId],
    cols: u16,
    fill_stage: usize,
    next_request_id: &mut u64,
) -> Vec<u32> {
    let mut probed_rows = Vec::with_capacity(sessions.len());
    for (index, session) in sessions.iter().copied().enumerate() {
        let begin_id = *next_request_id;
        *next_request_id += 1;
        let opened = history_request(dashboard, begin_id, Request::HistoryBegin { session });
        let Response::HistoryOpened(opened) = opened else {
            panic!("history begin returned {opened:?} for session {index}");
        };
        assert_eq!(opened.size.cols, cols);
        assert_eq!(opened.history_rows, u32::try_from(HISTORY_ROWS).unwrap());
        assert_eq!(
            opened.total_rows,
            opened.history_rows + u32::from(opened.size.rows)
        );
        probed_rows.push(opened.history_rows);
        println!(
            "MEMORY_PROBE geometry_cols={cols} fill_stage={fill_stage} session_index={index} session={} history_rows={} total_rows={} snapshot={}",
            session.0, opened.history_rows, opened.total_rows, opened.snapshot.0
        );
        assert_eq!(
            history_request(
                dashboard,
                *next_request_id,
                Request::HistoryEnd {
                    session,
                    snapshot: opened.snapshot,
                },
            ),
            Response::Ok
        );
        *next_request_id += 1;
    }
    assert!(probed_rows.iter().all(|rows| *rows <= HISTORY_ROWS as u32));
    assert!(probed_rows.iter().all(|rows| *rows == HISTORY_ROWS as u32));
    probed_rows
}

fn recv_pause_ready(socket: &UnixDatagram) -> PausePeer {
    let mut bytes = [0_u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(5);
    let size = loop {
        match socket.recv(&mut bytes) {
            Ok(size) => break size,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && Instant::now() < deadline =>
            {
                thread::yield_now();
            }
            Err(error) => panic!("read helper READY datagram: {error}"),
        }
    };
    let message = std::str::from_utf8(&bytes[..size]).unwrap();
    let mut fields = message.splitn(6, ':');
    assert_eq!(
        fields.next(),
        Some("READY"),
        "unexpected helper datagram: {message}"
    );
    let _role = fields.next().unwrap();
    let pid = fields.next().unwrap().parse().unwrap();
    let pgid = fields.next().unwrap().parse().unwrap();
    let address = PathBuf::from(fields.next().unwrap());
    let preexit_marker = fields.next().unwrap().to_owned();
    assert!(!preexit_marker.is_empty(), "READY omitted pre-exit marker");
    PausePeer {
        pid,
        pgid,
        address,
        preexit_marker,
    }
}

fn wait_peer_stopped(peer: &PausePeer, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        assert_eq!(
            unsafe { libc::getpgid(peer.pid) },
            peer.pgid,
            "peer moved out of its owned process group"
        );
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &peer.pid.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        if output.status.success() && state.trim_start().starts_with('T') {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "peer {} did not stop: {state:?}",
            peer.pid
        );
        thread::yield_now();
    }
}

fn send_peer_command(socket: &UnixDatagram, peer: &PausePeer, token: &str) {
    socket.send_to(token.as_bytes(), &peer.address).unwrap();
}

fn expect_peer_reply(socket: &UnixDatagram, token: &str) {
    assert_eq!(recv_pause_datagram(socket), token.as_bytes());
}

fn expect_peer_replies(socket: &UnixDatagram, tokens: &[&str]) {
    let mut remaining = tokens.to_vec();
    while !remaining.is_empty() {
        let bytes = recv_pause_datagram(socket);
        let Some(index) = remaining
            .iter()
            .position(|token| bytes.as_slice() == token.as_bytes())
        else {
            panic!(
                "unexpected helper reply: {:?}",
                String::from_utf8_lossy(&bytes)
            );
        };
        remaining.swap_remove(index);
    }
}

fn recv_pause_datagram(socket: &UnixDatagram) -> Vec<u8> {
    let mut bytes = [0_u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match socket.recv(&mut bytes) {
            Ok(size) => return bytes[..size].to_vec(),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                thread::yield_now()
            }
            Err(error) => panic!("read helper datagram: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for helper datagram"
        );
    }
}

fn expect_datagram_prefix(socket: &UnixDatagram, prefix: &str) -> Vec<u8> {
    let bytes = recv_pause_datagram(socket);
    assert!(
        bytes.starts_with(prefix.as_bytes()),
        "expected datagram prefix {prefix:?}, got {:?}",
        String::from_utf8_lossy(&bytes)
    );
    bytes
}

fn expect_pty_input(socket: &UnixDatagram, token: &str) {
    let bytes = expect_datagram_prefix(socket, "PTY:");
    assert_eq!(&bytes[4..], token.as_bytes());
}

fn expect_term_acks_and_descendant_final(
    socket: &UnixDatagram,
    peers: &[&PausePeer],
    descendants: &[&PausePeer],
) -> bool {
    let mut seen = Vec::new();
    let mut final_seen = Vec::new();
    let mut drain_ok = true;
    let mut drain_seen = Vec::new();
    while seen.len() < peers.len()
        || final_seen.len() < descendants.len()
        || drain_seen.len() < descendants.len()
    {
        let bytes = recv_pause_datagram(socket);
        if bytes.starts_with(b"TERM_ACK:") {
            let pid = std::str::from_utf8(&bytes[9..])
                .unwrap()
                .parse::<libc::pid_t>()
                .unwrap();
            assert!(
                peers.iter().any(|peer| peer.pid == pid),
                "unknown TERM_ACK pid {pid}"
            );
            assert!(!seen.contains(&pid), "duplicate TERM_ACK pid {pid}");
            seen.push(pid);
        } else if bytes.starts_with(b"FINAL_DESCENDANT_AFTER_TERM:") {
            let pid = std::str::from_utf8(&bytes[b"FINAL_DESCENDANT_AFTER_TERM:".len()..])
                .unwrap()
                .parse::<libc::pid_t>()
                .unwrap();
            assert!(
                descendants.iter().any(|peer| peer.pid == pid),
                "final marker came from an unexpected peer {pid}"
            );
            assert!(
                !final_seen.contains(&pid),
                "duplicate descendant final marker"
            );
            final_seen.push(pid);
        } else if bytes.starts_with(b"DESCENDANT_STDOUT_RESULT:") {
            let prefix = b"DESCENDANT_STDOUT_RESULT:";
            let evidence = std::str::from_utf8(&bytes[prefix.len()..]).unwrap();
            let (pid, statuses) = evidence.split_once(":write=").unwrap();
            let pid = pid.parse::<libc::pid_t>().unwrap();
            assert!(
                descendants.iter().any(|peer| peer.pid == pid),
                "drainage evidence came from an unexpected peer {pid}"
            );
            assert!(
                !drain_seen.contains(&pid),
                "duplicate descendant drainage evidence"
            );
            drain_seen.push(pid);
            let (write_status, flush_status) = statuses.split_once(":flush=").unwrap();
            for status in [write_status, flush_status] {
                if status == "ok" {
                    continue;
                }
                let errno = status
                    .strip_prefix("errno:")
                    .and_then(|value| value.parse::<libc::c_int>().ok());
                assert_eq!(
                    errno,
                    Some(libc::EIO),
                    "unexpected descendant PTY error (raw status {status:?})"
                );
                if !cfg!(target_os = "macos") {
                    panic!(
                        "EIO after leader hangup is only accepted on macOS (raw status {status:?})"
                    );
                }
            }
            // A successful write must be retained in the terminal; a flush
            // failure alone does not excuse the final-marker assertion.
            drain_ok &= write_status == "ok";
        } else {
            panic!(
                "unexpected termination evidence: {:?}",
                String::from_utf8_lossy(&bytes)
            );
        }
    }
    drain_ok
}

fn wait_pid_absent(pid: libc::pid_t, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let result = unsafe { libc::kill(pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return;
        }
        assert!(Instant::now() < deadline, "PID {pid} did not disappear");
        thread::yield_now();
    }
}

fn wait_group_absent(pgid: libc::pid_t, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let result = unsafe { libc::kill(-pgid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::yield_now();
    }
}

fn session_summary(fixture: &ControlFixture, session: SessionId) -> ovrcr::session::SessionSummary {
    match fixture.request(Request::List) {
        Response::Hierarchy(snapshot) => snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .find(|summary| summary.id == session)
            .unwrap(),
        response => panic!("unexpected list response: {response:?}"),
    }
}

fn wait_exited_and_assert_terminal_contains(
    fixture: &ControlFixture,
    session: SessionId,
    markers: &[&str],
) {
    let deadline = Instant::now() + wait_deadline();
    loop {
        let response = request_with_timeout(
            &fixture.socket,
            401,
            Request::List,
            Duration::from_millis(250),
        );
        if let Some(Response::Hierarchy(snapshot)) = response {
            let exited = snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .any(|summary| {
                    summary.id == session && matches!(summary.phase, SessionPhase::Exited { .. })
                });
            if exited {
                let terminal = request_with_timeout(
                    &fixture.socket,
                    402,
                    Request::ReadTerminal {
                        session,
                        max_lines: None,
                    },
                    Duration::from_secs(1),
                )
                .unwrap_or_else(|| panic!("terminal read failed at first Exited observation"));
                let Response::TerminalText { text, .. } = terminal else {
                    panic!(
                        "unexpected terminal response at first Exited observation: {terminal:?}"
                    );
                };
                for marker in markers {
                    assert!(
                        text.contains(marker),
                        "terminal omitted final marker {marker:?} at first Exited observation: {text:?}"
                    );
                }
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "session {session:?} did not exit before terminal assertion deadline"
        );
        thread::yield_now();
    }
}

#[test]
fn pause_resume_stops_group_and_rejects_input() {
    let _env_lock = env_lock();
    let mut harness = PauseHarness::new();
    let (session, leader, descendant) = harness.create_session("pause-input");
    let (mut dashboard, before_screen) = dashboard_for_session(&harness.fixture.socket, session);
    let before = session_summary(&harness.fixture, session);

    assert_eq!(
        harness.fixture.request(Request::PauseSession { session }),
        Response::Ok
    );
    wait_peer_stopped(&leader, Duration::from_secs(2));
    wait_peer_stopped(&descendant, Duration::from_secs(2));
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            3,
            Request::Input {
                session,
                bytes: b"REJECTED_WHILE_PAUSED".to_vec(),
            },
        ),
        Response::Error {
            code: ErrorCode::Conflict,
            message: "session is paused; resume it before sending input".into(),
        }
    );
    assert!(matches!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Error {
            code: ErrorCode::SessionRunning,
            ..
        }
    ));
    assert!(matches!(
        harness.fixture.request(Request::Shutdown { kill: false }),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));

    send_peer_command(&harness.control, &leader, "RESUME_LEADER_1");
    send_peer_command(&harness.control, &descendant, "RESUME_DESCENDANT_1");
    assert_eq!(
        harness.fixture.request(Request::ResumeSession { session }),
        Response::Ok
    );
    expect_peer_replies(
        &harness.control,
        &["RESUME_LEADER_1", "RESUME_DESCENDANT_1"],
    );
    assert_eq!(dashboard_select(&mut dashboard, 4, session), before_screen);
    let after = session_summary(&harness.fixture, session);
    assert_eq!(after.id, before.id);
    assert_eq!(after.pid, before.pid);
    assert!(matches!(after.phase, SessionPhase::Running));
    assert_eq!(
        dashboard_request(
            &mut dashboard,
            5,
            Request::Input {
                session,
                bytes: b"PTY_AFTER_RESUME_1".to_vec(),
            },
        ),
        Response::Ok
    );
    expect_pty_input(&harness.control, "PTY_AFTER_RESUME_1");

    assert_eq!(
        harness.fixture.request(Request::KillSession { session }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&leader, &descendant],
        &[&descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        session,
        &[
            descendant.preexit_marker.as_str(),
            "FINAL_AFTER_TERM",
            "FINAL_DESCENDANT_AFTER_TERM",
        ],
    );
    assert_eq!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Ok
    );
    drop(dashboard);
    harness.finish();
}

#[test]
fn pause_resume_kill_runs_group_handlers() {
    let _env_lock = env_lock();
    let mut harness = PauseHarness::new();
    let (session, leader, descendant) = harness.create_session("kill-paused");
    let (mut dashboard, _) = dashboard_for_session(&harness.fixture.socket, session);
    assert_eq!(
        harness.fixture.request(Request::PauseSession { session }),
        Response::Ok
    );
    wait_peer_stopped(&leader, Duration::from_secs(2));
    wait_peer_stopped(&descendant, Duration::from_secs(2));
    assert_eq!(
        harness.fixture.request(Request::KillSession { session }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&leader, &descendant],
        &[&descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        session,
        &[
            descendant.preexit_marker.as_str(),
            "FINAL_AFTER_TERM",
            "FINAL_DESCENDANT_AFTER_TERM",
        ],
    );
    assert!(wait_group_absent(leader.pgid, Duration::from_secs(2)));
    assert_eq!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Ok
    );

    let (external, external_leader, external_descendant) =
        harness.create_session("kill-external-stop");
    let _ = dashboard_select(&mut dashboard, 3, external);
    unsafe {
        assert_eq!(libc::kill(-external_leader.pgid, libc::SIGSTOP), 0);
    }
    wait_peer_stopped(&external_leader, Duration::from_secs(2));
    wait_peer_stopped(&external_descendant, Duration::from_secs(2));
    assert!(matches!(
        session_summary(&harness.fixture, external).phase,
        SessionPhase::Running
    ));
    assert_eq!(
        harness
            .fixture
            .request(Request::KillSession { session: external }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&external_leader, &external_descendant],
        &[&external_descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        external,
        &[
            external_descendant.preexit_marker.as_str(),
            "FINAL_AFTER_TERM",
            "FINAL_DESCENDANT_AFTER_TERM",
        ],
    );
    assert!(wait_group_absent(
        external_leader.pgid,
        Duration::from_secs(2)
    ));
    assert_eq!(
        harness
            .fixture
            .request(Request::RemoveSession { session: external }),
        Response::Ok
    );
    drop(dashboard);
    harness.finish();
}

#[test]
fn pause_resume_shutdown_cleans_stopped_groups() {
    let _env_lock = env_lock();
    let mut harness = PauseHarness::new();
    let (first, first_leader, first_descendant) = harness.create_session("shutdown-one");
    let (second, second_leader, second_descendant) = harness.create_session("shutdown-two");
    for session in [first, second] {
        assert_eq!(
            harness.fixture.request(Request::PauseSession { session }),
            Response::Ok
        );
    }
    for peer in [
        &first_leader,
        &first_descendant,
        &second_leader,
        &second_descendant,
    ] {
        wait_peer_stopped(peer, Duration::from_secs(2));
    }
    assert_eq!(
        harness.fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[
            &first_leader,
            &first_descendant,
            &second_leader,
            &second_descendant,
        ],
        &[&first_descendant, &second_descendant],
    ));
    assert!(wait_group_absent(first_leader.pgid, Duration::from_secs(2)));
    assert!(wait_group_absent(
        second_leader.pgid,
        Duration::from_secs(2)
    ));
    assert!(
        harness.join_bounded(Duration::from_secs(2)),
        "control server did not finish after shutdown"
    );
    assert!(!harness.fixture.socket.exists());
}

#[test]
fn pause_resume_control_races_converge() {
    let _env_lock = env_lock();
    pause_resume_control_races_body();
}

fn pause_resume_control_races_body() {
    let mut harness = PauseHarness::new();
    let (session, leader, descendant) = harness.create_session("control-race");
    let barrier = Arc::new(Barrier::new(4));
    let operations = [
        Request::PauseSession { session },
        Request::ResumeSession { session },
        Request::KillSession { session },
    ];
    let streams = operations
        .iter()
        .map(|_| connect_server(&harness.fixture.socket).unwrap())
        .collect::<Vec<_>>();
    let workers = operations
        .into_iter()
        .zip(streams)
        .map(|(request, mut stream)| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                request_on_stream(&mut stream, 1, request, Duration::from_secs(4))
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let responses = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(responses.iter().all(|response| {
        matches!(
            response,
            Response::Ok
                | Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                }
        )
    }));
    if !responses.iter().any(|response| response == &Response::Ok) {
        assert_eq!(
            harness.fixture.request(Request::KillSession { session }),
            Response::Ok
        );
    }
    assert!(expect_term_acks_and_descendant_final(
        &harness.control,
        &[&leader, &descendant],
        &[&descendant],
    ));
    wait_exited_and_assert_terminal_contains(
        &harness.fixture,
        session,
        &[
            descendant.preexit_marker.as_str(),
            "FINAL_AFTER_TERM",
            "FINAL_DESCENDANT_AFTER_TERM",
        ],
    );
    assert!(wait_group_absent(leader.pgid, Duration::from_secs(2)));
    assert_eq!(
        harness.fixture.request(Request::RemoveSession { session }),
        Response::Ok
    );

    let (reaped, reaped_leader, reaped_descendant) =
        harness.create_session_with_options("reaped-leader", true);
    send_peer_command(&harness.control, &reaped_leader, "EXIT_LEADER");
    expect_peer_reply(&harness.control, "EXIT_LEADER_ACK");
    wait_pid_absent(reaped_leader.pid, Duration::from_secs(3));
    assert_eq!(
        unsafe { libc::getpgid(reaped_descendant.pid) },
        reaped_descendant.pgid
    );
    assert_eq!(
        harness
            .fixture
            .request(Request::PauseSession { session: reaped }),
        Response::Ok
    );
    wait_peer_stopped(&reaped_descendant, Duration::from_secs(2));
    send_peer_command(&harness.control, &reaped_descendant, "RESUME_SURVIVOR_1");
    assert_eq!(
        harness
            .fixture
            .request(Request::ResumeSession { session: reaped }),
        Response::Ok
    );
    expect_peer_reply(&harness.control, "RESUME_SURVIVOR_1");
    assert_eq!(
        harness
            .fixture
            .request(Request::KillSession { session: reaped }),
        Response::Ok
    );
    let reaped_drain_ok = expect_term_acks_and_descendant_final(
        &harness.control,
        &[&reaped_descendant],
        &[&reaped_descendant],
    );
    if !cfg!(target_os = "macos") {
        assert!(
            reaped_drain_ok,
            "reaped survivor did not drain final PTY bytes"
        );
    }
    let mut reaped_terminal_markers = vec![reaped_descendant.preexit_marker.as_str()];
    if reaped_drain_ok {
        reaped_terminal_markers.push("FINAL_DESCENDANT_AFTER_TERM");
    }
    wait_exited_and_assert_terminal_contains(&harness.fixture, reaped, &reaped_terminal_markers);
    assert!(wait_group_absent(
        reaped_descendant.pgid,
        Duration::from_secs(2)
    ));
    assert_eq!(
        harness
            .fixture
            .request(Request::RemoveSession { session: reaped }),
        Response::Ok
    );
    harness.finish();
}

fn request_on_stream(
    stream: &mut UnixStream,
    request_id: u64,
    request: Request,
    timeout: Duration,
) -> Response {
    stream.set_read_timeout(Some(timeout)).unwrap();
    stream.set_write_timeout(Some(timeout)).unwrap();
    client::request(stream, request_id, request).unwrap()
}

#[test]
fn pause_resume_backpressured_input_keeps_controls_available() {
    let _env_lock = env_lock();
    let mut harness = PauseHarness::new();
    let summary = harness.fixture.create_session_summary(
        "blocked-pause",
        vec![
            "sh".into(),
            "-c".into(),
            "stty raw -echo; printf READY; exec sleep 30".into(),
        ],
    );
    harness.record_created_pgid(&summary);
    let session = summary.id;
    // The input below can only block once `stty raw` has run, and the
    // dashboard drain must not race the READY output frame; wait for the
    // marker server-side so both hold before selecting.
    harness.fixture.wait_terminal_contains(session, "READY");
    let mut dashboard = connect_server(&harness.fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    dashboard
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    let _ = dashboard_select(&mut dashboard, 2, session);
    dashboard
        .set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();
    while read_frame::<ServerMessage>(&mut dashboard).is_ok() {}
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 20,
            request: Request::Input {
                session,
                bytes: vec![b'x'; 512 * 1024],
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    assert!(read_frame::<ServerMessage>(&mut dashboard).is_err());

    let mut blocked_send = connect_server(&harness.fixture.socket).unwrap();
    blocked_send
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    blocked_send
        .set_write_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    write_frame(
        &mut blocked_send,
        &ClientMessage {
            request_id: 21,
            request: Request::SendTerminal {
                session,
                text: "queued behind blocked input".into(),
                submit: false,
            },
        },
    )
    .unwrap();
    assert!(read_frame::<ServerMessage>(&mut blocked_send).is_err());

    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            22,
            Request::PauseSession { session },
            Duration::from_secs(3),
        ),
        Some(Response::Ok)
    );
    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            23,
            Request::List,
            Duration::from_secs(3),
        )
        .map(|response| matches!(response, Response::Hierarchy(_))),
        Some(true)
    );
    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            24,
            Request::ResumeSession { session },
            Duration::from_secs(3),
        ),
        Some(Response::Ok)
    );
    assert_eq!(
        request_with_timeout(
            &harness.fixture.socket,
            25,
            Request::KillSession { session },
            Duration::from_secs(5),
        ),
        Some(Response::Ok)
    );
    harness.fixture.wait_exited(session);
    drop(blocked_send);
    drop(dashboard);
    harness.finish();
}

#[test]
#[ignore]
fn pause_resume_child_fixture() {
    let role = std::env::var("OVRCR_PAUSE_ROLE").unwrap();
    let dir = PathBuf::from(std::env::var("OVRCR_PAUSE_DIR").unwrap());
    let parent = std::env::var_os("OVRCR_PAUSE_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("parent.sock"));
    let preexit_marker = std::env::var("OVRCR_PAUSE_PREEXIT_MARKER").unwrap();
    assert!(
        !preexit_marker.is_empty(),
        "pre-exit marker must be nonempty"
    );
    let address = dir.join(format!("{role}.sock"));
    let _ = std::fs::remove_file(&address);
    let socket = UnixDatagram::bind(&address).unwrap();
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(libc::SIGTERM, Arc::clone(&term)).unwrap();
    let ignore_sighup = std::env::var("OVRCR_PAUSE_IGNORE_SIGHUP")
        .map(|value| value == "1")
        .unwrap_or(false);
    if role == "descendant" && ignore_sighup {
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
    }
    let mut child = if role == "leader" {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "pause_resume_child_fixture",
                "--nocapture",
            ])
            .env("OVRCR_PAUSE_ROLE", "descendant")
            .env("OVRCR_PAUSE_DIR", &dir)
            .env("OVRCR_PAUSE_PARENT", &parent)
            .env("OVRCR_PAUSE_PREEXIT_MARKER", &preexit_marker);
        if ignore_sighup {
            command.env("OVRCR_PAUSE_IGNORE_SIGHUP", "1");
        }
        Some(command.spawn().unwrap())
    } else {
        None
    };
    let pid = unsafe { libc::getpid() };
    let pgid = unsafe { libc::getpgid(pid) };
    let original_termios = set_pause_raw_terminal();
    let stdin_fd = libc::STDIN_FILENO;
    if role == "descendant" {
        std::io::stdout()
            .write_all(format!("{preexit_marker}\n").as_bytes())
            .unwrap();
        std::io::stdout().flush().unwrap();
    }
    socket
        .send_to(
            format!(
                "READY:{role}:{pid}:{pgid}:{}:{preexit_marker}",
                address.display()
            )
            .as_bytes(),
            &parent,
        )
        .unwrap();
    let mut stdin_bytes = [0_u8; 4096];
    let mut datagram_bytes = [0_u8; 4096];
    loop {
        if term.load(Ordering::Acquire) {
            socket
                .send_to(format!("TERM_ACK:{pid}").as_bytes(), &parent)
                .unwrap();
            if let Some(child) = child.as_mut() {
                let _ = child.wait();
                std::io::stdout().write_all(b"FINAL_AFTER_TERM\n").unwrap();
            } else {
                let write_result = std::io::stdout().write_all(b"FINAL_DESCENDANT_AFTER_TERM\n");
                let flush_result = std::io::stdout().flush();
                let write_status = match &write_result {
                    Ok(()) => "ok".to_owned(),
                    Err(error) => format!("errno:{}", error.raw_os_error().unwrap_or(-1)),
                };
                let flush_status = match &flush_result {
                    Ok(()) => "ok".to_owned(),
                    Err(error) => format!("errno:{}", error.raw_os_error().unwrap_or(-1)),
                };
                socket
                    .send_to(
                        format!(
                            "DESCENDANT_STDOUT_RESULT:{pid}:write={write_status}:flush={flush_status}"
                        )
                        .as_bytes(),
                        &parent,
                    )
                    .unwrap();
            }
            if role == "descendant" {
                socket
                    .send_to(
                        format!("FINAL_DESCENDANT_AFTER_TERM:{pid}").as_bytes(),
                        &parent,
                    )
                    .unwrap();
            }
            std::io::stdout().flush().unwrap();
            restore_pause_terminal(original_termios);
            let _ = std::fs::remove_file(&address);
            return;
        }
        let mut fds = [
            libc::pollfd {
                fd: socket.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if role == "leader" { stdin_fd } else { -1 },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 50) };
        if result == -1 {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR)
            );
            continue;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            let (size, sender) = socket.recv_from(&mut datagram_bytes).unwrap();
            if &datagram_bytes[..size] == b"EXIT_LEADER" && role == "leader" {
                socket
                    .send_to(b"EXIT_LEADER_ACK", sender.as_pathname().unwrap())
                    .unwrap();
                restore_pause_terminal(original_termios);
                let _ = std::fs::remove_file(&address);
                return;
            }
            socket
                .send_to(&datagram_bytes[..size], sender.as_pathname().unwrap())
                .unwrap();
        }
        if role == "leader" && fds[1].revents & libc::POLLIN != 0 {
            let size =
                unsafe { libc::read(stdin_fd, stdin_bytes.as_mut_ptr().cast(), stdin_bytes.len()) };
            if size > 0 {
                let mut message = b"PTY:".to_vec();
                message.extend_from_slice(&stdin_bytes[..size as usize]);
                socket.send_to(&message, &parent).unwrap();
            }
        }
    }
}

fn set_pause_raw_terminal() -> Option<libc::termios> {
    let fd = libc::STDIN_FILENO;
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
        return None;
    }
    let original = unsafe { original.assume_init() };
    let mut raw = original;
    raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ECHONL);
    raw.c_cc[libc::VMIN] = 0;
    raw.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
        return None;
    }
    Some(original)
}

fn restore_pause_terminal(original: Option<libc::termios>) {
    if let Some(original) = original {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
        }
    }
}

#[test]
#[ignore = "bounded historical scrollback RSS measurement"]
fn history_memory_measurements() {
    let _env_lock = env_lock();
    let cols = match std::env::var("OVRCR_HISTORY_MEMORY_GEOMETRY").as_deref() {
        Ok("80") => 80,
        Ok("512") => 512,
        Ok(value) => {
            panic!("OVRCR_HISTORY_MEMORY_GEOMETRY must be exactly 80 or 512, got {value:?}")
        }
        Err(error) => {
            panic!("OVRCR_HISTORY_MEMORY_GEOMETRY is required (allowed values: 80, 512): {error}")
        }
    };
    let fixture = ControlFixture::new_bounded();
    history_workspace(&fixture, &format!("feature/history-memory-{cols}"));
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::KillSession { session: local }),
        Response::Ok
    );
    fixture.wait_exited(local);
    assert_eq!(
        fixture.request(Request::RemoveSession { session: local }),
        Response::Ok
    );

    let mut dashboard = HistoryConnection::connect(&fixture.socket);
    assert!(matches!(
        history_request(&mut dashboard, 1, Request::DashboardHello),
        Response::Hierarchy(_)
    ));
    let mut sessions = Vec::with_capacity(50);
    for index in 0..50 {
        let summary = fixture.create_session_summary(
            &format!("history-memory-{index}"),
            history_memory_shell(index, cols),
        );
        fixture.record_process_group(&summary);
        sessions.push(summary.id);
        let ready = format!("HISTORY_MEMORY_READY_{index}");
        fixture.wait_terminal_contains(summary.id, &ready);

        let selected = history_request(
            &mut dashboard,
            index as u64 + 2,
            Request::Select {
                session: summary.id,
                size: ovrcr::session::TerminalSize { rows: 24, cols },
            },
        );
        let Response::Screen { size, .. } = selected else {
            panic!("select returned {selected:?}");
        };
        assert_eq!(
            size,
            ovrcr::session::TerminalSize { rows: 24, cols },
            "geometry was not applied before fill for session {index}"
        );
        let Response::TerminalText { size, .. } = fixture.request(Request::ReadTerminal {
            session: summary.id,
            max_lines: Some(1),
        }) else {
            panic!("ReadTerminal did not return terminal text for session {index}");
        };
        assert_eq!(
            size,
            ovrcr::session::TerminalSize { rows: 24, cols },
            "ReadTerminal reported the wrong geometry for session {index}"
        );
    }
    assert_eq!(
        sessions.len(),
        50,
        "memory run must measure exactly 50 sessions"
    );
    let process_id = std::process::id();
    let before_fill = current_rss_kib();
    println!(
        "MEMORY_MEASUREMENT geometry_cols={cols} pid={process_id} rss_units=KiB rss_before_fill_after_ready={before_fill} session_count={}",
        sessions.len()
    );

    for (index, session) in sessions.iter().copied().enumerate() {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session,
                text: "go".into(),
                submit: true,
            }),
            Response::Ok
        );
        let done = format!("HISTORY_MEMORY_DONE_1_{index}");
        wait_memory_marker(
            &fixture,
            &mut dashboard,
            session,
            &done,
            Instant::now() + Duration::from_secs(30),
        );
        println!("MEMORY_FILL_DONE geometry_cols={cols} session_index={index}");
    }

    let after_first_fill = current_rss_kib();
    println!(
        "MEMORY_MEASUREMENT geometry_cols={cols} pid={process_id} rss_units=KiB rss_after_first_fill_before_probes={after_first_fill} first_fill_retention_delta_kib={} session_count={}",
        after_first_fill.saturating_sub(before_fill),
        sessions.len()
    );

    let mut next_request_id = 1000_u64;
    let first_rows =
        history_memory_probe_all(&mut dashboard, &sessions, cols, 1, &mut next_request_id);
    assert_eq!(first_rows.len(), sessions.len());
    let after_first_probes = current_rss_kib();
    println!(
        "MEMORY_MEASUREMENT geometry_cols={cols} pid={process_id} rss_units=KiB rss_after_first_probes={after_first_probes} first_probe_allocator_delta_kib={} retained_sessions={}",
        after_first_probes.saturating_sub(after_first_fill),
        sessions.len()
    );

    for (index, session) in sessions.iter().copied().enumerate() {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session,
                text: "again".into(),
                submit: true,
            }),
            Response::Ok
        );
        let done = format!("HISTORY_MEMORY_DONE_2_{index}");
        wait_memory_marker(
            &fixture,
            &mut dashboard,
            session,
            &done,
            Instant::now() + Duration::from_secs(30),
        );
        println!("MEMORY_FILL_DONE geometry_cols={cols} fill_stage=2 session_index={index}");
    }
    let after_second_fill = current_rss_kib();
    println!(
        "MEMORY_MEASUREMENT geometry_cols={cols} pid={process_id} rss_units=KiB rss_after_second_fill_before_probes={after_second_fill} second_fill_growth_after_first_probes_kib={} session_count={}",
        after_second_fill.saturating_sub(after_first_probes),
        sessions.len()
    );
    let second_rows =
        history_memory_probe_all(&mut dashboard, &sessions, cols, 2, &mut next_request_id);
    assert_eq!(second_rows.len(), sessions.len());
    let after_second_probes = current_rss_kib();
    println!(
        "MEMORY_MEASUREMENT geometry_cols={cols} pid={process_id} rss_units=KiB rss_after_second_probes={after_second_probes} second_probe_allocator_delta_kib={} retained_sessions={}",
        after_second_probes.saturating_sub(after_second_fill),
        sessions.len()
    );

    let final_session = sessions[0];
    let final_begin_id = next_request_id;
    next_request_id += 1;
    let final_opened = history_request(
        &mut dashboard,
        final_begin_id,
        Request::HistoryBegin {
            session: final_session,
        },
    );
    let Response::HistoryOpened(final_opened) = final_opened else {
        panic!("final held history begin returned {final_opened:?}");
    };
    assert_eq!(final_opened.history_rows, HISTORY_ROWS as u32);
    let held_snapshot = final_opened.snapshot;
    let held_rss = current_rss_kib();
    println!(
        "MEMORY_MEASUREMENT geometry_cols={cols} pid={process_id} rss_units=KiB rss_with_one_held_snapshot={held_rss} held_session={} held_snapshot={} retained_rows={} retained_sessions={}",
        final_session.0,
        held_snapshot.0,
        final_opened.history_rows,
        sessions.len()
    );
    let final_end_id = next_request_id;
    assert_eq!(
        history_request(
            &mut dashboard,
            final_end_id,
            Request::HistoryEnd {
                session: final_session,
                snapshot: held_snapshot,
            },
        ),
        Response::Ok
    );
    drop(dashboard);

    let groups = fixture.process_groups.lock().unwrap().clone();
    fixture.shutdown_kill();
    assert_eq!(
        groups.len(),
        50,
        "memory run must own exactly 50 process groups"
    );
    assert!(
        groups
            .iter()
            .all(|pgid| wait_group_absent(*pgid, Duration::from_secs(2))),
        "memory run left an owned process group"
    );
    println!(
        "MEMORY_CLEANUP geometry_cols={cols} process_groups_clean=true server_thread_joined=true"
    );
}

#[test]
fn history_reattach_reads_retained_output() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    history_workspace(&fixture, "feature/history-reattach");
    let session = history_ready_shell(&fixture);
    fixture.wait_terminal_contains(session, "HISTORY_READY");

    let mut first = HistoryConnection::connect(&fixture.socket);
    let hello = history_request(&mut first, 1, Request::DashboardHello);
    assert!(matches!(hello, Response::Hierarchy(_)));
    let screen = history_request(
        &mut first,
        2,
        Request::Select {
            session,
            size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
        },
    );
    let Response::Screen { bytes, .. } = screen else {
        panic!("select did not return a screen");
    };
    let mut parser = vt100::Parser::new(24, 80, HISTORY_ROWS);
    parser.process(&bytes);
    assert!(parser.screen().contents().contains("HISTORY_READY"));
    drop(first);

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session,
            text: "go".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_exited(session);

    let mut reattached = HistoryConnection::connect(&fixture.socket);
    assert!(matches!(
        history_request(&mut reattached, 10, Request::DashboardHello),
        Response::Hierarchy(_)
    ));
    let screen = history_request(
        &mut reattached,
        11,
        Request::Select {
            session,
            size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
        },
    );
    let Response::Screen { bytes, .. } = screen else {
        panic!("reattached select did not return a screen");
    };
    let live_text = String::from_utf8_lossy(&bytes);
    assert!(live_text.contains("FINAL_HISTORY_MARKER"));
    assert!(!live_text.contains("DETACHED_OFFSCREEN_000"));

    let opened_id = 12;
    let opened = history_request(
        &mut reattached,
        opened_id,
        Request::HistoryBegin { session },
    );
    let Response::HistoryOpened(opened) = opened else {
        panic!("history begin returned {opened:?}");
    };
    assert!(
        history_response_size(opened_id, &Response::HistoryOpened(opened.clone())) <= PAGE_BYTES
    );
    assert!(opened.history_rows > 0);
    let rows = history_page_all(&mut reattached, session, opened.snapshot, &opened, &mut 13);
    let retained_rows = &rows[..opened.history_rows as usize];
    assert!(history_rows_text(retained_rows).contains("DETACHED_OFFSCREEN_000"));
    assert!(history_rows_text(&rows).contains("FINAL_HISTORY_MARKER"));
    assert_eq!(
        history_request(
            &mut reattached,
            1000,
            Request::HistoryEnd {
                session,
                snapshot: opened.snapshot,
            },
        ),
        Response::Ok
    );
    drop(reattached);

    fixture.shutdown_kill();
    for pgid in fixture.process_groups.lock().unwrap().iter().copied() {
        assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    }
}

#[test]
fn history_frozen_page_survives_eviction_and_exit() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    history_workspace(&fixture, "feature/history-eviction");
    let session = history_ready_shell(&fixture);
    fixture.wait_terminal_contains(session, "HISTORY_READY");
    let mut dashboard = HistoryConnection::connect(&fixture.socket);
    assert!(matches!(
        history_request(&mut dashboard, 1, Request::DashboardHello),
        Response::Hierarchy(_)
    ));
    let screen = history_request(
        &mut dashboard,
        2,
        Request::Select {
            session,
            size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
        },
    );
    let Response::Screen { bytes, .. } = screen else {
        panic!("select did not return a screen");
    };
    let mut parser = vt100::Parser::new(24, 80, HISTORY_ROWS);
    parser.process(&bytes);
    assert!(parser.screen().contents().contains("HISTORY_READY"));

    let begin_id = 3;
    let begin = history_request(&mut dashboard, begin_id, Request::HistoryBegin { session });
    let Response::HistoryOpened(opened) = begin else {
        panic!("history begin returned {begin:?}");
    };
    let frozen_rows = history_page_all(&mut dashboard, session, opened.snapshot, &opened, &mut 4);
    let frozen_text = history_rows_text(&frozen_rows);
    assert!(frozen_text.contains("OLD_VISIBLE"));
    assert!(!frozen_text.contains("FINAL_HISTORY_MARKER"));

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session,
            text: "go".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_exited(session);

    let frozen_again =
        history_page_all(&mut dashboard, session, opened.snapshot, &opened, &mut 100);
    assert_eq!(frozen_again, frozen_rows);
    assert!(history_rows_text(&frozen_again).contains("OLD_VISIBLE"));
    assert!(!history_rows_text(&frozen_again).contains("FINAL_HISTORY_MARKER"));
    assert_eq!(
        history_request(
            &mut dashboard,
            200,
            Request::HistoryEnd {
                session,
                snapshot: opened.snapshot,
            },
        ),
        Response::Ok
    );

    let new_begin_id = 201;
    let new_begin = history_request(
        &mut dashboard,
        new_begin_id,
        Request::HistoryBegin { session },
    );
    let Response::HistoryOpened(new_opened) = new_begin else {
        panic!("new history begin returned {new_begin:?}");
    };
    assert_eq!(
        new_opened.history_rows,
        u32::try_from(HISTORY_ROWS).unwrap()
    );
    let new_rows = history_page_all(
        &mut dashboard,
        session,
        new_opened.snapshot,
        &new_opened,
        &mut 202,
    );
    let new_text = history_rows_text(&new_rows);
    assert!(new_text.contains("FINAL_HISTORY_MARKER"));
    assert!(!new_text.contains("OLD_VISIBLE"));
    assert_eq!(
        history_request(
            &mut dashboard,
            300,
            Request::HistoryEnd {
                session,
                snapshot: new_opened.snapshot,
            },
        ),
        Response::Ok
    );
    drop(dashboard);

    fixture.shutdown_kill();
    for pgid in fixture.process_groups.lock().unwrap().iter().copied() {
        assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    }
}

#[test]
fn history_slow_dashboard_recovers_after_finite_burst() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new_bounded();
    history_workspace(&fixture, "feature/history-slow-dashboard");
    let summary = fixture.create_session_summary(
        "history-burst",
        vec![
            "awk".into(),
            "BEGIN { for (i = 0; i < 200000; i++) printf \"BURST_%06d\\n\", i; printf \"FINAL_HISTORY_MARKER\\n\" }".into(),
        ],
    );
    fixture.record_process_group(&summary);
    let session = summary.id;

    let mut dashboard = HistoryConnection::connect(&fixture.socket);
    assert!(matches!(
        history_request(&mut dashboard, 1, Request::DashboardHello),
        Response::Hierarchy(_)
    ));
    let selected = history_request(
        &mut dashboard,
        2,
        Request::Select {
            session,
            size: ovrcr::session::TerminalSize {
                rows: 40,
                cols: 120,
            },
        },
    );
    assert!(matches!(selected, Response::Screen { .. }));
    fixture.wait_exited(session);

    let drain_deadline = Instant::now() + Duration::from_secs(2);
    let mut dirty = false;
    while Instant::now() < drain_deadline {
        match dashboard.next(Instant::now() + Duration::from_millis(50)) {
            Ok(Some(ServerMessage::Event(ServerEvent::ScreenDirty {
                session: dirty_session,
                revision: _,
            }))) if dirty_session == session => dirty = true,
            Ok(Some(_)) => {}
            Ok(None) => {
                break;
            }
            Err(error) => panic!("draining slow dashboard failed: {error}"),
        }
    }
    assert!(dirty, "finite burst did not emit dirty recovery");

    let recovered = history_request(
        &mut dashboard,
        3,
        Request::Select {
            session,
            size: ovrcr::session::TerminalSize {
                rows: 40,
                cols: 120,
            },
        },
    );
    assert!(matches!(recovered, Response::Screen { .. }));
    let begin_id = 4;
    let begin = history_request(&mut dashboard, begin_id, Request::HistoryBegin { session });
    let Response::HistoryOpened(opened) = begin else {
        panic!("history begin after dirty recovery returned {begin:?}");
    };
    let rows = history_page_all(&mut dashboard, session, opened.snapshot, &opened, &mut 5);
    assert!(history_rows_text(&rows).contains("FINAL_HISTORY_MARKER"));
    assert_eq!(
        history_request(
            &mut dashboard,
            1000,
            Request::HistoryEnd {
                session,
                snapshot: opened.snapshot,
            },
        ),
        Response::Ok
    );
    drop(dashboard);
    fixture.shutdown_kill();
    for pgid in fixture.process_groups.lock().unwrap().iter().copied() {
        assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    }
}

#[test]
fn slow_dashboard_recovers_after_output_burst() {
    let _env_lock = env_lock();
    assert_eq!(ovrcr::server::RAW_EVENT_QUEUE_CAPACITY, 64);
    assert_eq!(ovrcr::server::RAW_DISPATCH_QUEUE_CAPACITY, 64);
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/slow-dashboard".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    // awk emits the burst in well under a second on any runner; a shell
    // loop needs several seconds on a slow CI machine.
    let burst = fixture.create_session(
        "burst",
        vec![
            "awk".into(),
            "BEGIN { for (i = 0; i < 200000; i++) printf \"BURST_%06d\\n\", i; printf \"FINAL_MARKER\" }".into(),
        ],
    );

    let mut dashboard = connect_server(&fixture.socket).unwrap();
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
                session: burst,
                size: ovrcr::session::TerminalSize {
                    rows: 40,
                    cols: 120,
                },
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();

    fixture.wait_exited(burst);

    let mut saw_dirty = false;
    while !saw_dirty {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ovrcr::protocol::ServerEvent::ScreenDirty {
                session,
                revision: _,
            }) if session == burst => saw_dirty = true,
            _ => {}
        }
    }
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 3,
            request: Request::Select {
                session: burst,
                size: ovrcr::session::TerminalSize {
                    rows: 40,
                    cols: 120,
                },
            },
        },
    )
    .unwrap();
    // The session's exit events may still be queued behind the dirty
    // marker; skip them and judge the select response itself.
    let screen = loop {
        if let ServerMessage::Response {
            request_id: 3,
            response,
        } = read_frame::<ServerMessage>(&mut dashboard).unwrap()
        {
            break response;
        }
    };
    assert!(
        matches!(
            &screen,
            Response::Screen { bytes, .. } if String::from_utf8_lossy(bytes).contains("FINAL_MARKER")
        ),
        "select after burst returned {screen:?}"
    );
    let mut dirty_count = 1;
    dashboard
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let quiet_deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < quiet_deadline {
        match read_frame::<ServerMessage>(&mut dashboard) {
            Ok(ServerMessage::Event(ovrcr::protocol::ServerEvent::ScreenDirty {
                session,
                revision: _,
            })) if session == burst => dirty_count += 1,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.root_cause().downcast_ref::<std::io::Error>(),
                    Some(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        )
                ) =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    assert_eq!(
        dirty_count, 1,
        "quiet burst emitted more than one ScreenDirty"
    );
    drop(dashboard);
    fixture.request(Request::RemoveSession { session: burst });
    let local = fixture.only_session_id();
    fixture.request(Request::KillSession { session: local });
    fixture.wait_exited(local);
    fixture.request(Request::RemoveSession { session: local });
    fixture.request(Request::RemoveWorkspace {
        project: "fixture".into(),
        name: "work".into(),
    });
    fixture.request(Request::RemoveProject {
        name: "fixture".into(),
    });
    fixture.request(Request::Shutdown { kill: false });
    fixture.join();
}

#[test]
fn concurrent_terminal_sends_are_serialized_as_complete_pastes() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/concurrent-send".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session(
        "concurrent",
        vec![
            "sh".into(),
            "-c".into(),
            r#"stty raw -echo; printf '\033[?2004hREADY\r\n'; dd bs=1 count=34 2>/dev/null | od -An -tx1; printf '\r\nTAIL\r\n'"#.into(),
        ],
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match fixture.request(Request::ReadTerminal {
            session,
            max_lines: None,
        }) {
            Response::TerminalText { text, .. } if text.contains("READY") => break,
            _ if Instant::now() < deadline => thread::park_timeout(Duration::from_millis(10)),
            response => panic!("concurrent terminal was not ready: {response:?}"),
        }
    }

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers = ["AA\nA", "BB\nB"].map(|text| {
        let socket = fixture.socket.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        thread::spawn(move || {
            let mut stream = connect_server(socket).unwrap();
            barrier.wait();
            write_frame(
                &mut stream,
                &ClientMessage {
                    request_id: 1,
                    request: Request::SendTerminal {
                        session,
                        text: text.into(),
                        submit: true,
                    },
                },
            )
            .unwrap();
            match read_frame::<ServerMessage>(&mut stream).unwrap() {
                ServerMessage::Response {
                    response: Response::Ok,
                    ..
                } => {}
                response => panic!("unexpected send response: {response:?}"),
            }
        })
    });
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }

    fixture.wait_exited(session);
    let text = match fixture.request(Request::ReadTerminal {
        session,
        max_lines: None,
    }) {
        Response::TerminalText { text, .. } => text,
        response => panic!("unexpected terminal read: {response:?}"),
    };
    let hex = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let a = "1b 5b 32 30 30 7e 41 41 0a 41 1b 5b 32 30 31 7e 0d";
    let b = "1b 5b 32 30 30 7e 42 42 0a 42 1b 5b 32 30 31 7e 0d";
    assert!(
        hex.contains(&format!("{a} {b}")) || hex.contains(&format!("{b} {a}")),
        "concurrent paste bytes interleaved: {text:?}"
    );

    assert_eq!(
        fixture.request(Request::CloseTerminal { session }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::CloseTerminal { session: local }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn resource_terminal_requests_preserve_background_state_and_close_cleanly() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/resource-terminal".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let local = fixture.only_session_id();
    let background = fixture.create_session(
        "background",
        vec![
            "sh".into(),
            "-c".into(),
            r#"printf '\033[?2004hREADY\r\n'; IFS= read -r line; printf '\r\nACK\r\n'; printf '%s' "$line" | od -An -tx1; printf 'TAIL\r\n'"#.into(),
        ],
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    let (background_size, initial_text) = loop {
        match fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: None,
        }) {
            Response::TerminalText { size, text, .. } if text.contains("READY") => {
                break (size, text);
            }
            _ if Instant::now() < deadline => thread::park_timeout(Duration::from_millis(10)),
            response => panic!("background terminal was not ready: {response:?}"),
        }
    };
    assert_eq!(
        background_size,
        ovrcr::session::TerminalSize {
            rows: 40,
            cols: 120
        }
    );
    assert!(!initial_text.contains("ACK"));
    assert!(matches!(
        fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: Some(0),
        }),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        fixture.request(Request::ReadTerminal {
            session: SessionId(u64::MAX),
            max_lines: None,
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));

    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
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
                session: local,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 2,
                response: Response::Screen { .. },
            }
        ) {
            break;
        }
    }

    let background_pid = match fixture.request(Request::Inspect) {
        Response::Inventory { registry, sessions } => {
            assert_eq!(registry.projects.len(), 1);
            assert_eq!(sessions.len(), 2);
            sessions
                .into_iter()
                .find(|session| session.id == background)
                .and_then(|session| session.pid)
                .expect("running background PID")
        }
        response => panic!("unexpected inventory response: {response:?}"),
    };
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: background,
            text: "alpha".into(),
            submit: false,
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: background,
            text: " beta".into(),
            submit: true,
        }),
        Response::Ok
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    let final_text = loop {
        match fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: None,
        }) {
            Response::TerminalText { size, text, .. } if text.contains("TAIL") => {
                assert_eq!(size, background_size, "background send resized the PTY");
                break text;
            }
            _ if Instant::now() < deadline => thread::park_timeout(Duration::from_millis(10)),
            response => panic!("background terminal did not acknowledge input: {response:?}"),
        }
    };
    let hex = final_text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        hex.contains("1b 5b 32 30 30 7e 61 6c 70 68 61 1b 5b 32 30 31 7e"),
        "first send was not bracketed: {final_text:?}"
    );
    assert!(
        hex.contains("1b 5b 32 30 30 7e 20 62 65 74 61 1b 5b 32 30 31 7e"),
        "submitted send was not bracketed: {final_text:?}"
    );
    assert!(matches!(
        fixture.request(Request::ReadTerminal {
            session: background,
            max_lines: Some(1),
        }),
        Response::TerminalText { text, .. } if text == "TAIL"
    ));

    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 3,
            request: Request::Input {
                session: local,
                bytes: b"printf SELECTED_OK\r".to_vec(),
            },
        },
    )
    .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 3,
                response: Response::Ok,
            }
        ) {
            break;
        }
    }

    fixture.wait_exited(background);
    assert_eq!(
        fixture.request(Request::CloseTerminal {
            session: background,
        }),
        Response::Ok
    );
    wait_for_group_absent(background_pid as libc::pid_t, Duration::from_secs(2));
    assert_eq!(
        fixture.request(Request::CloseTerminal { session: local }),
        Response::Ok
    );
    assert!(matches!(
        fixture.request(Request::CloseTerminal {
            session: background,
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    drop(dashboard);
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn fifty_sessions_survive_detach_and_leave_no_process_groups() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/fifty-sessions".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let sessions = (0..50)
        .map(|index| {
            fixture.create_session(
                &format!("waiting-{index}"),
                vec![
                    "sh".into(),
                    "-c".into(),
                    format!("printf 'SESSION_MARKER_{index}'; while :; do sleep 1; done").into(),
                ],
            )
        })
        .collect::<Vec<_>>();

    let mut dashboard = connect_server(&fixture.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    for (index, session) in sessions.iter().copied().enumerate() {
        select_screen_containing(
            &mut dashboard,
            index as u64 + 2,
            session,
            &format!("SESSION_MARKER_{index}"),
        );
    }
    drop(dashboard);

    let mut summaries = Vec::new();
    if let Response::Hierarchy(snapshot) = fixture.request(Request::List) {
        summaries = snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .filter(|summary| sessions.contains(&summary.id))
            .collect();
    }
    assert_eq!(summaries.len(), sessions.len());
    let pgids = summaries
        .iter()
        .filter_map(|summary| summary.pid)
        .map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) })
        .collect::<Vec<_>>();
    assert_eq!(
        pgids.len(),
        50,
        "all 50 managed process groups must be saved"
    );
    assert!(pgids.iter().all(|pgid| *pgid > 1 && group_exists(*pgid)));

    let mut reattached = connect_dashboard(&fixture.socket, 100);
    reattached
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    for (index, session) in sessions.iter().copied().enumerate() {
        select_screen_containing(
            &mut reattached,
            index as u64 + 101,
            session,
            &format!("SESSION_MARKER_{index}"),
        );
    }
    drop(reattached);

    let previous_grace = std::env::var_os("OVRCR_KILL_GRACE_MS");
    unsafe { std::env::set_var("OVRCR_KILL_GRACE_MS", "500") };
    for session in sessions {
        assert_eq!(
            fixture.request(Request::KillSession { session }),
            Response::Ok
        );
    }
    match previous_grace {
        Some(value) => unsafe { std::env::set_var("OVRCR_KILL_GRACE_MS", value) },
        None => unsafe { std::env::remove_var("OVRCR_KILL_GRACE_MS") },
    }
    for pgid in pgids {
        wait_for_group_absent(pgid, Duration::from_secs(3));
    }
    if let Response::Hierarchy(snapshot) = fixture.request(Request::List) {
        assert!(
            snapshot
                .projects
                .iter()
                .flat_map(|project| project.workspaces.iter())
                .flat_map(|workspace| workspace.sessions.iter())
                .filter(|summary| summary.name.starts_with("waiting-"))
                .all(|summary| summary.pid.is_none())
        );
    }
    let local = fixture.only_session_id();
    fixture.request(Request::KillSession { session: local });
    fixture.wait_exited(local);
    for session in sessions_for_cleanup(&fixture) {
        fixture.request(Request::RemoveSession { session });
    }
    fixture.request(Request::RemoveSession { session: local });
    fixture.request(Request::RemoveWorkspace {
        project: "fixture".into(),
        name: "work".into(),
    });
    fixture.request(Request::RemoveProject {
        name: "fixture".into(),
    });
    fixture.request(Request::Shutdown { kill: false });
    fixture.join();
}

fn sessions_for_cleanup(fixture: &ControlFixture) -> Vec<SessionId> {
    match fixture.request(Request::List) {
        Response::Hierarchy(snapshot) => snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .filter(|session| session.name.starts_with("waiting-"))
            .map(|session| session.id)
            .collect(),
        _ => Vec::new(),
    }
}

struct ControlFixture {
    _root: tempfile::TempDir,
    repo: std::path::PathBuf,
    workspace_root: std::path::PathBuf,
    socket: std::path::PathBuf,
    thread: std::sync::Mutex<Option<thread::JoinHandle<()>>>,
    process_groups: std::sync::Mutex<Vec<libc::pid_t>>,
    request_timeout: Option<Duration>,
    workspace_ready: std::sync::Mutex<bool>,
}

#[derive(Clone, Copy)]
struct HookIdentity {
    session: SessionId,
    capability: [u8; 32],
}

impl ControlFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let workspace_root = root.path().join("workspaces");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&workspace_root).unwrap();
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
        let socket = root.path().join("server.sock");
        let registry = root.path().join("config.toml");
        save_registry_atomic(&ovrcr::config::Registry::default(), &registry).unwrap();
        let paths = ServerPaths {
            socket: socket.clone(),
        };
        let thread = thread::spawn(move || run_server(paths, registry).unwrap());
        let fixture = Self {
            _root: root,
            repo: repo.canonicalize().unwrap(),
            workspace_root: workspace_root.canonicalize().unwrap(),
            socket,
            thread: std::sync::Mutex::new(Some(thread)),
            process_groups: std::sync::Mutex::new(Vec::new()),
            request_timeout: None,
            workspace_ready: std::sync::Mutex::new(false),
        };
        fixture.wait_socket();
        fixture
    }
    fn new_bounded() -> Self {
        let mut fixture = Self::new();
        fixture.request_timeout = Some(Duration::from_secs(5));
        fixture
    }
    fn wait_socket(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if UnixStream::connect(&self.socket).is_ok() {
                return;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        panic!("control server did not start");
    }
    fn request(&self, request: Request) -> Response {
        let mut stream = connect_server(&self.socket).unwrap();
        if let Some(timeout) = self.request_timeout {
            stream.set_read_timeout(Some(timeout)).unwrap();
            stream.set_write_timeout(Some(timeout)).unwrap();
        }
        client::request(&mut stream, 1, request).unwrap()
    }
    fn create_session_summary(
        &self,
        name: &str,
        argv: Vec<OsString>,
    ) -> ovrcr::session::SessionSummary {
        match self.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(),
            workspace: "work".into(),
            name: name.into(),
            label: None,
            argv,
        })) {
            Response::CreatedSession(summary) => *summary,
            response => panic!("unexpected response: {response:?}"),
        }
    }
    fn create_session(&self, name: &str, argv: Vec<OsString>) -> SessionId {
        self.create_session_summary(name, argv).id
    }

    fn create_hook_child(&self, name: &str, marker: &str) -> HookIdentity {
        self.create_hook_child_inner(name, marker, false)
    }

    fn create_hook_child_with_report(&self, name: &str, marker: &str) -> HookIdentity {
        self.create_hook_child_inner(name, marker, true)
    }

    fn create_hook_child_inner(&self, name: &str, marker: &str, report: bool) -> HookIdentity {
        let mut workspace_ready = self.workspace_ready.lock().unwrap();
        if !*workspace_ready {
            assert_eq!(
                self.request(Request::AddProject {
                    name: "fixture".into(),
                    repo: self.repo.clone(),
                    workspace_root: self.workspace_root.clone(),
                }),
                Response::Ok
            );
            assert_eq!(
                self.request(Request::CreateWorkspace {
                    project: "fixture".into(),
                    name: "work".into(),
                    branch: BranchRequest::New {
                        branch: "agent-hooks".into(),
                        base: "main".into(),
                    },
                }),
                Response::Ok
            );
            *workspace_ready = true;
        }
        drop(workspace_ready);
        let identity_path = self._root.path().join(format!("{marker}.identity"));
        let child = std::env::current_exe().unwrap();
        let script = if report {
            r#"stty -echo; OVRCR_AGENT_HOOK_IDENTITY_PATH="$1" exec "$2" --ignored --exact hook_child_report_helper --nocapture"#
        } else {
            r#"printf '%s\n%s\n%s\n' "$OVRCR_HOOK_SOCKET" "$OVRCR_SESSION_ID" "$OVRCR_HOOK_TOKEN" > "$1"; printf HOOK_READY; while IFS= read -r line; do :; done"#
        };
        let mut argv = vec![
            "sh".into(),
            "-c".into(),
            script.into(),
            "ovrcr-hook-child".into(),
            identity_path.clone().into_os_string(),
        ];
        if report {
            argv.push(child.into_os_string());
        }
        let summary = self.create_session_summary(name, argv);
        self.record_process_group(&summary);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(contents) = std::fs::read_to_string(&identity_path) {
                let mut lines = contents.lines();
                let socket = lines.next().unwrap_or_default();
                let session = lines.next().and_then(|value| value.parse::<u64>().ok());
                let capability = lines.next().and_then(parse_hook_capability);
                if let (Some(session), Some(capability)) = (session, capability) {
                    let expected_socket = self
                        .socket
                        .parent()
                        .unwrap()
                        .canonicalize()
                        .unwrap()
                        .join(self.socket.file_name().unwrap());
                    assert_eq!(socket, expected_socket.to_string_lossy());
                    assert_eq!(session, summary.id.0);
                    if report && !contents.lines().any(|line| line == "REPORTED") {
                        thread::park_timeout(Duration::from_millis(5));
                        continue;
                    }
                    return HookIdentity {
                        session: summary.id,
                        capability,
                    };
                }
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        panic!("managed hook child did not publish its private identity");
    }

    fn record_process_group(&self, summary: &ovrcr::session::SessionSummary) {
        if let Some(pid) = summary.pid {
            self.process_groups.lock().unwrap().push(pid as libc::pid_t);
        }
    }

    fn original_pgid(&self, id: SessionId) -> libc::pid_t {
        let pid = self.session_summary(id).pid.expect("live session PID");
        let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
        assert_eq!(pgid, pid as libc::pid_t);
        pgid
    }

    fn session_summary(&self, id: SessionId) -> ovrcr::session::SessionSummary {
        match self.request(Request::List) {
            Response::Hierarchy(snapshot) => snapshot
                .projects
                .into_iter()
                .flat_map(|project| project.workspaces)
                .flat_map(|workspace| workspace.sessions)
                .find(|session| session.id == id)
                .unwrap_or_else(|| panic!("session record disappeared")),
            response => panic!("unexpected response: {response:?}"),
        }
    }

    fn session_activity(&self, id: SessionId) -> ovrcr::session::AgentActivity {
        self.session_summary(id).activity
    }

    fn session_phase(&self, id: SessionId) -> SessionPhase {
        self.session_summary(id).phase
    }

    fn wait_terminal_contains(&self, id: SessionId, marker: &str) {
        self.wait_terminal_contains_until(id, marker, Instant::now() + Duration::from_secs(2));
    }

    fn wait_terminal_contains_until(&self, id: SessionId, marker: &str, deadline: Instant) {
        let mut last_response = None;
        while Instant::now() < deadline {
            last_response = request_with_timeout(
                &self.socket,
                401,
                Request::ReadTerminal {
                    session: id,
                    max_lines: None,
                },
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(250)),
            );
            if matches!(&last_response, Some(Response::TerminalText { text, .. }) if text.contains(marker))
            {
                return;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        panic!(
            "session {id:?} did not produce terminal marker {marker:?}; last response: {last_response:?}"
        );
    }

    fn wait_identity_marker(&self, marker: &str, expected: &str) {
        let identity_path = self._root.path().join(format!("{marker}.identity"));
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if std::fs::read_to_string(&identity_path)
                .map(|contents| contents.lines().any(|line| line == expected))
                .unwrap_or(false)
            {
                return;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        panic!("managed hook child did not publish marker {expected:?}");
    }

    fn shutdown_kill(&self) {
        let sessions = match self.request(Request::List) {
            Response::Hierarchy(snapshot) => snapshot
                .projects
                .into_iter()
                .flat_map(|project| project.workspaces)
                .flat_map(|workspace| workspace.sessions)
                .collect::<Vec<_>>(),
            response => panic!("unexpected response: {response:?}"),
        };
        for session in sessions {
            if session.name == "local" {
                let _ = self.request(Request::SendTerminal {
                    session: session.id,
                    text: "exit".into(),
                    submit: true,
                });
                self.wait_exited(session.id);
            } else {
                assert_eq!(
                    self.request(Request::KillSession {
                        session: session.id
                    }),
                    Response::Ok
                );
                self.wait_exited(session.id);
            }
        }
        assert_eq!(self.request(Request::Shutdown { kill: true }), Response::Ok);
        assert!(
            self.join_bounded(Duration::from_secs(2)),
            "bounded fixture server did not terminate after shutdown"
        );
    }
    fn only_session_id(&self) -> SessionId {
        match self.request(Request::List) {
            Response::Hierarchy(snapshot) => snapshot.projects[0].workspaces[0].sessions[0].id,
            response => panic!("unexpected response: {response:?}"),
        }
    }
    fn wait_exited(&self, id: SessionId) {
        let deadline = Instant::now() + wait_deadline();
        while Instant::now() < deadline {
            let response = if self.request_timeout.is_some() {
                request_with_timeout(&self.socket, 1, Request::List, Duration::from_millis(250))
            } else {
                Some(self.request(Request::List))
            };
            if let Some(Response::Hierarchy(snapshot)) = response
                && snapshot
                    .projects
                    .iter()
                    .flat_map(|project| project.workspaces.iter())
                    .flat_map(|workspace| workspace.sessions.iter())
                    .any(|session| {
                        session.id == id && matches!(session.phase, SessionPhase::Exited { .. })
                    })
            {
                return;
            }
            thread::park_timeout(Duration::from_millis(10));
        }
        panic!("session {id:?} did not exit");
    }
    fn git_output(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .output()
            .unwrap()
    }
    fn join(&self) {
        self.thread.lock().unwrap().take().unwrap().join().unwrap();
    }

    fn join_bounded(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let finished = self
                .thread
                .lock()
                .unwrap()
                .as_ref()
                .map(|handle| handle.is_finished())
                .unwrap_or(true);
            if finished {
                let handle = self.thread.lock().unwrap().take();
                return handle.map(|handle| handle.join().is_ok()).unwrap_or(true);
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
    }

    fn kill_owned_groups(&self) -> bool {
        let groups = self.process_groups.lock().unwrap().clone();
        let mut cleaned = true;
        for pgid in groups {
            if !group_exists(pgid) {
                continue;
            }
            if unsafe { libc::kill(-pgid, libc::SIGKILL) } != 0 && group_exists(pgid) {
                cleaned = false;
                continue;
            }
            if !wait_group_absent(pgid, Duration::from_secs(2)) {
                cleaned = false;
            }
        }
        cleaned
    }
}

impl Drop for ControlFixture {
    fn drop(&mut self) {
        let mut cleanup_failed = false;
        if self.thread.get_mut().unwrap().is_some() {
            let shutdown = request_with_timeout(
                &self.socket,
                999,
                Request::Shutdown { kill: true },
                self.request_timeout.unwrap_or(Duration::from_secs(2)),
            );
            if shutdown != Some(Response::Ok) {
                eprintln!("control fixture shutdown response: {shutdown:?}");
                cleanup_failed = true;
            }
        }
        if !self.kill_owned_groups() {
            cleanup_failed = true;
        }
        if self.thread.get_mut().unwrap().is_some() && !self.join_bounded(Duration::from_secs(2)) {
            cleanup_failed = true;
        }
        if !self
            .process_groups
            .get_mut()
            .unwrap()
            .iter()
            .copied()
            .all(|pgid| !group_exists(pgid))
        {
            cleanup_failed = true;
        }
        if cleanup_failed {
            let kept = std::mem::replace(&mut self._root, tempfile::tempdir().unwrap()).keep();
            eprintln!(
                "control fixture cleanup incomplete; preserved {}",
                kept.display()
            );
        }
    }
}

fn parse_hook_capability(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut capability = [0_u8; 32];
    for (index, byte) in capability.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(capability)
}

#[test]
fn dashboard_duplicate_hello_does_not_write_from_reader_thread() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut first = connect_server(&fixture.paths.socket).unwrap();
    write_frame(
        &mut first,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut first).unwrap();
    let mut second = connect_server(&fixture.paths.socket).unwrap();
    write_frame(
        &mut second,
        &ClientMessage {
            request_id: 2,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    second
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut second).unwrap(),
        ServerMessage::Response {
            response: Response::Error {
                code: ErrorCode::Conflict,
                ..
            },
            ..
        }
    ));
    first.shutdown(Shutdown::Both).unwrap();
    fixture.stop();
}

#[test]
fn duplicate_dashboard_hello_uses_the_sole_writer() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = connect_server(&fixture.paths.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 20,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 20,
            response: Response::Hierarchy(_),
            ..
        }
    ));
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 21,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 21,
            response: Response::Error {
                code: ErrorCode::Conflict,
                ..
            },
            ..
        }
    ));
    drop(dashboard);
    fixture.stop();
}

#[test]
fn dashboard_request_id_zero_does_not_block_followup_response() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = connect_server(&fixture.paths.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 0,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::List,
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let response = read_frame::<ServerMessage>(&mut dashboard);
    assert!(
        response.is_ok(),
        "request id zero must not block later dashboard responses: {response:?}"
    );
    drop(dashboard);
    fixture.stop();
}

#[test]
fn dashboard_geometry_sizes_connected_empty_and_detached_sessions() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/dashboard-geometry".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let mut dashboard = connect_server(&fixture.socket).unwrap();
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
            request: Request::DashboardGeometry {
                size: ovrcr::session::TerminalSize { rows: 17, cols: 61 },
            },
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 2,
            response: Response::Ok,
        }
    ));
    let connected_path = fixture._root.path().join("connected-size");
    let connected = fixture.create_session(
        "connected",
        vec![
            "sh".into(),
            "-c".into(),
            format!("stty size > {}; sleep 30", connected_path.display()).into(),
        ],
    );
    wait_for_file_contents(&connected_path, "17 61");
    // A local drop is not a server-side detach acknowledgement. Half-close and
    // drain to EOF: this handler drops its ownership/geometry before its socket.
    dashboard.shutdown(std::net::Shutdown::Write).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    std::io::copy(&mut dashboard, &mut std::io::sink()).unwrap();
    drop(dashboard);
    let detached_path = fixture._root.path().join("detached-size");
    let detached = fixture.create_session(
        "detached",
        vec![
            "sh".into(),
            "-c".into(),
            format!("stty size > {}; sleep 30", detached_path.display()).into(),
        ],
    );
    wait_for_file_contents(&detached_path, "40 120");
    assert!(matches!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    ));
    fixture.join();
    let _ = (connected, detached);
}

#[test]
fn dashboard_receives_concrete_ordinary_request_errors() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = connect_server(&fixture.paths.socket).unwrap();
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
                session: SessionId(99),
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 2,
            response: Response::Error {
                code: ErrorCode::NotFound,
                message,
            },
        } if message.contains("session 99 not found")
    ));
    drop(dashboard);
    fixture.stop();
}

#[test]
fn selection_snapshot_precedes_later_quiet_tail_output() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/select-order".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );
    let session = fixture.create_session(
        "quiet-tail",
        vec![
            "sh".into(),
            "-c".into(),
            "printf READY; read line; printf QUIET_TAIL; exec sleep 30".into(),
        ],
    );
    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
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
                session,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    let mut saw_snapshot = false;
    while !saw_snapshot {
        if let ServerMessage::Response {
            request_id: 2,
            response: Response::Screen { .. },
        } = read_frame::<ServerMessage>(&mut dashboard).unwrap()
        {
            saw_snapshot = true;
        }
    }
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 3,
            request: Request::Input {
                session,
                bytes: b"\n".to_vec(),
            },
        },
    )
    .unwrap();
    let mut saw_tail = false;
    while !saw_tail {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ovrcr::protocol::ServerEvent::Output { bytes, .. })
                if String::from_utf8_lossy(&bytes).contains("QUIET_TAIL") =>
            {
                saw_tail = true;
            }
            _ => {}
        }
    }
    assert!(saw_snapshot);
    assert!(saw_tail);
    drop(dashboard);
    assert!(matches!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    ));
    fixture.join();
}

fn split_next_message(
    stream: &mut UnixStream,
    frames: &mut HistoryFrameReader,
    deadline: Instant,
    label: &str,
) -> ServerMessage {
    assert!(
        Instant::now() < deadline,
        "{label} exceeded its absolute deadline"
    );
    let two_second_deadline = Instant::now() + Duration::from_secs(2);
    let read_deadline = deadline.min(two_second_deadline);
    let timeout_label = if deadline <= two_second_deadline {
        "outer"
    } else {
        "two-second read"
    };
    frames
        .next(stream, read_deadline)
        .unwrap_or_else(|error| panic!("{label} read failed before deadline: {error}"))
        .unwrap_or_else(|| panic!("{label} exceeded its {timeout_label} deadline"))
}

fn split_view_messages(
    stream: &mut UnixStream,
    request_id: u64,
    view: &DashboardView,
) -> Vec<ServerMessage> {
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        stream,
        &ClientMessage {
            request_id,
            request: Request::SetView { view: view.clone() },
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut frames = HistoryFrameReader::new();
    let mut screens = HashSet::new();
    let mut acknowledged = false;
    let mut messages = Vec::new();
    while !acknowledged || screens.len() < view.panes.len() {
        let message = split_next_message(stream, &mut frames, deadline, "split view request");
        if let ServerMessage::Response {
            request_id: response_id,
            response,
        } = &message
            && *response_id == request_id
        {
            match response {
                Response::Screen { session, .. } => {
                    assert!(
                        view.panes.iter().any(|pane| pane.session == *session),
                        "split view returned snapshot for unexpected session {session:?}"
                    );
                    assert!(
                        screens.insert(*session),
                        "split view returned duplicate snapshot for {session:?}"
                    );
                }
                Response::Ok => {
                    assert_eq!(
                        screens,
                        view.panes.iter().map(|pane| pane.session).collect(),
                        "split view acknowledged before all expected snapshots"
                    );
                    acknowledged = true;
                }
                Response::Error { message, .. } => {
                    panic!("split view request failed: {message}")
                }
                response => panic!("split view returned unexpected response: {response:?}"),
            }
        }
        messages.push(message);
    }
    assert_eq!(
        screens,
        view.panes.iter().map(|pane| pane.session).collect(),
        "split view must return exactly one snapshot per pane"
    );
    assert!(
        acknowledged,
        "split view did not acknowledge after snapshots"
    );
    messages
}

fn split_view_parsers(
    messages: &[ServerMessage],
    view: &DashboardView,
) -> HashMap<SessionId, vt100::Parser> {
    let mut parsers = view
        .panes
        .iter()
        .map(|pane| {
            (
                pane.session,
                vt100::Parser::new(pane.size.rows, pane.size.cols, 0),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut screens = HashSet::new();
    for message in messages {
        match message {
            ServerMessage::Response {
                response:
                    Response::Screen {
                        session,
                        revision,
                        size,
                        bytes,
                    },
                ..
            } if view.panes.iter().any(|pane| pane.session == *session) => {
                let expected = view
                    .panes
                    .iter()
                    .find(|pane| pane.session == *session)
                    .unwrap();
                assert_eq!(*revision, view.revision);
                assert_eq!(*size, expected.size);
                parsers.get_mut(session).unwrap().process(bytes);
                screens.insert(*session);
            }
            ServerMessage::Event(ServerEvent::Output {
                session,
                revision,
                bytes,
            }) if view.panes.iter().any(|pane| pane.session == *session) => {
                if *revision == view.revision {
                    parsers.get_mut(session).unwrap().process(bytes);
                }
            }
            ServerMessage::Event(ServerEvent::ScreenDirty { session, revision })
                if view.panes.iter().any(|pane| pane.session == *session)
                    && screens.contains(session) =>
            {
                assert_eq!(
                    *revision, view.revision,
                    "old-revision dirty notification arrived after replacement"
                );
            }
            _ => {}
        }
    }
    assert_eq!(screens.len(), view.panes.len());
    parsers
}

fn split_input_marker(
    stream: &mut UnixStream,
    request_id: u64,
    session: SessionId,
    revision: u64,
    parser: &mut vt100::Parser,
    marker: &str,
) {
    write_frame(
        stream,
        &ClientMessage {
            request_id,
            request: Request::Input {
                session,
                bytes: b"SIZE\n".to_vec(),
            },
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut frames = HistoryFrameReader::new();
    let mut acknowledged = false;
    let mut raw = Vec::new();
    while !acknowledged
        || !raw
            .windows(marker.len())
            .any(|window| window == marker.as_bytes())
    {
        let message = split_next_message(stream, &mut frames, deadline, "split input marker");
        match message {
            ServerMessage::Response {
                request_id: response_id,
                response,
            } if response_id == request_id => match response {
                Response::Ok => acknowledged = true,
                Response::Error { message, .. } => {
                    panic!("split input request failed: {message}")
                }
                response => panic!("split input returned unexpected response: {response:?}"),
            },
            ServerMessage::Event(ServerEvent::Output {
                session: output_session,
                revision: output_revision,
                bytes,
            }) if output_session == session => {
                assert_eq!(
                    output_revision, revision,
                    "old-revision output arrived after a pane replacement"
                );
                raw.extend_from_slice(&bytes);
                parser.process(&bytes);
            }
            ServerMessage::Event(ServerEvent::ScreenDirty {
                session: dirty_session,
                revision: dirty_revision,
            }) if dirty_session == session => {
                assert_eq!(
                    dirty_revision, revision,
                    "old-revision dirty notification arrived after a pane replacement"
                );
            }
            _ => {}
        }
    }
    assert!(
        parser.screen().contents().contains(marker),
        "child emitted {marker:?}, but parser screen did not reconstruct it"
    );
}

fn split_terminal_marker(
    fixture: &ControlFixture,
    session: SessionId,
    marker: &str,
) -> (ovrcr::session::TerminalSize, String) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(2));
        if let Some(Response::TerminalText { size, text, .. }) = request_with_timeout(
            &fixture.socket,
            400,
            Request::ReadTerminal {
                session,
                max_lines: None,
            },
            timeout,
        ) && text.contains(marker)
        {
            return (size, text);
        }
        assert!(
            Instant::now() < deadline,
            "session {session:?} did not produce terminal marker {marker:?}"
        );
    }
}

#[test]
fn split_server_two_streams_resize_resync_and_detach() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let fixture = ControlFixture::new_bounded();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::CreateWorkspace {
            project: "fixture".into(),
            name: "work".into(),
            branch: BranchRequest::New {
                branch: "feature/split-server-acceptance".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );

    let command = |side: &str| {
        vec![
            "sh".into(),
            "-c".into(),
            format!(
                r#"SIDE={side}; printf '%s_READY\n' "$SIDE"; while IFS= read -r line; do case "$line" in SIZE) printf '%s_SIZE_%s\n' "$SIDE" "$(stty size)";; BURST) awk -v side="$SIDE" 'BEGIN {{ for (i = 0; i < 200000; i++) printf "%s_%06d\n", side, i; printf "%s_FINAL\n", side }}';; *) printf '%s_ACK_%s\n' "$SIDE" "$line";; esac; done"#,
                side = side,
            )
            .into(),
        ]
    };
    let left = fixture.create_session_summary("left", command("LEFT"));
    fixture.record_process_group(&left);
    let right = fixture.create_session_summary("right", command("RIGHT"));
    fixture.record_process_group(&right);
    let hidden = fixture.create_session_summary(
        "hidden",
        vec![
            "sh".into(),
            "-c".into(),
            r#"printf HIDDEN_READY\n; while IFS= read -r line; do case "$line" in SIZE_BEFORE) printf 'HIDDEN_SIZE_BEFORE_%s\n' "$(stty size)";; SIZE_AFTER) printf 'HIDDEN_SIZE_AFTER_%s\n' "$(stty size)";; *) printf 'HIDDEN_ACK_%s\n' "$line";; esac; done"#.into(),
        ],
    );
    fixture.record_process_group(&hidden);
    let left_pgid = fixture.original_pgid(left.id);
    let right_pgid = fixture.original_pgid(right.id);
    let hidden_pgid = fixture.original_pgid(hidden.id);
    fixture.wait_terminal_contains(left.id, "LEFT_READY");
    fixture.wait_terminal_contains(right.id, "RIGHT_READY");
    fixture.wait_terminal_contains(hidden.id, "HIDDEN_READY");

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: hidden.id,
            text: "SIZE_BEFORE".into(),
            submit: true,
        }),
        Response::Ok
    );
    let (hidden_size_before, hidden_text_before) =
        split_terminal_marker(&fixture, hidden.id, "HIDDEN_SIZE_BEFORE_40 120");
    assert_eq!(
        hidden_size_before,
        ovrcr::session::TerminalSize {
            rows: 40,
            cols: 120,
        }
    );
    assert!(hidden_text_before.contains("HIDDEN_SIZE_BEFORE_40 120"));

    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    dashboard
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(matches!(
        dashboard_request(&mut dashboard, 1, Request::DashboardHello),
        Response::Hierarchy(_)
    ));

    let first_view = DashboardView {
        revision: 1,
        panes: vec![
            PaneTarget {
                session: left.id,
                size: ovrcr::session::TerminalSize { rows: 36, cols: 39 },
            },
            PaneTarget {
                session: right.id,
                size: ovrcr::session::TerminalSize { rows: 36, cols: 40 },
            },
        ],
        focused: Some(left.id),
    };
    let first_messages = split_view_messages(&mut dashboard, 2, &first_view);
    let mut parsers = split_view_parsers(&first_messages, &first_view);
    assert!(parsers[&left.id].screen().contents().contains("LEFT_READY"));
    assert!(
        parsers[&right.id]
            .screen()
            .contents()
            .contains("RIGHT_READY")
    );
    split_input_marker(
        &mut dashboard,
        3,
        left.id,
        first_view.revision,
        parsers.get_mut(&left.id).unwrap(),
        "LEFT_SIZE_36 39",
    );

    let focus_right_view = DashboardView {
        revision: 2,
        focused: Some(right.id),
        ..first_view.clone()
    };
    let focus_right_messages = split_view_messages(&mut dashboard, 4, &focus_right_view);
    let mut parsers = split_view_parsers(&focus_right_messages, &focus_right_view);
    split_input_marker(
        &mut dashboard,
        5,
        right.id,
        focus_right_view.revision,
        parsers.get_mut(&right.id).unwrap(),
        "RIGHT_SIZE_36 40",
    );

    let resized_view = DashboardView {
        revision: 3,
        panes: vec![
            PaneTarget {
                session: left.id,
                size: ovrcr::session::TerminalSize { rows: 26, cols: 29 },
            },
            PaneTarget {
                session: right.id,
                size: ovrcr::session::TerminalSize { rows: 26, cols: 30 },
            },
        ],
        focused: Some(left.id),
    };
    let resized_messages = split_view_messages(&mut dashboard, 6, &resized_view);
    let mut parsers = split_view_parsers(&resized_messages, &resized_view);
    split_input_marker(
        &mut dashboard,
        7,
        left.id,
        resized_view.revision,
        parsers.get_mut(&left.id).unwrap(),
        "LEFT_SIZE_26 29",
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: hidden.id,
            text: "SIZE_AFTER".into(),
            submit: true,
        }),
        Response::Ok
    );
    let (hidden_size_after, hidden_text_after) =
        split_terminal_marker(&fixture, hidden.id, "HIDDEN_SIZE_AFTER_40 120");
    assert_eq!(hidden_size_after, hidden_size_before);
    assert!(hidden_text_after.contains("HIDDEN_SIZE_BEFORE_40 120"));
    assert!(hidden_text_after.contains("HIDDEN_SIZE_AFTER_40 120"));

    let resized_focus_right_view = DashboardView {
        revision: 4,
        focused: Some(right.id),
        ..resized_view.clone()
    };
    let resized_focus_right_messages =
        split_view_messages(&mut dashboard, 8, &resized_focus_right_view);
    let mut parsers = split_view_parsers(&resized_focus_right_messages, &resized_focus_right_view);
    split_input_marker(
        &mut dashboard,
        9,
        right.id,
        resized_focus_right_view.revision,
        parsers.get_mut(&right.id).unwrap(),
        "RIGHT_SIZE_26 30",
    );

    let receive_buffer: libc::c_int = 1024;
    let set_buffer = unsafe {
        libc::setsockopt(
            dashboard.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            (&receive_buffer as *const libc::c_int).cast(),
            std::mem::size_of_val(&receive_buffer) as libc::socklen_t,
        )
    };
    assert_eq!(set_buffer, 0, "failed to bound dashboard receive buffer");

    // Bulk production and recovery share one deadline. The short control
    // deadlines below still prove that PTY output does not block requests.
    let burst_deadline = Instant::now() + Duration::from_secs(30);
    assert_eq!(
        request_with_timeout(
            &fixture.socket,
            10,
            Request::SendTerminal {
                session: left.id,
                text: "BURST".into(),
                submit: true,
            },
            Duration::from_secs(2),
        ),
        Some(Response::Ok)
    );
    assert_eq!(
        request_with_timeout(
            &fixture.socket,
            11,
            Request::SendTerminal {
                session: right.id,
                text: "BURST".into(),
                submit: true,
            },
            Duration::from_secs(2),
        ),
        Some(Response::Ok)
    );
    let control_deadline = Instant::now() + Duration::from_secs(2);
    let mut control_polls = 0;
    while control_polls < 3 {
        assert!(
            Instant::now() < control_deadline,
            "control hierarchy polling exceeded its absolute deadline"
        );
        assert!(matches!(
            request_with_timeout(
                &fixture.socket,
                20 + control_polls,
                Request::List,
                control_deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(2)),
            ),
            Some(Response::Hierarchy(_))
        ));
        control_polls += 1;
    }
    fixture.wait_terminal_contains_until(left.id, "LEFT_FINAL", burst_deadline);
    fixture.wait_terminal_contains_until(right.id, "RIGHT_FINAL", burst_deadline);

    let mut dirty = HashSet::new();
    let mut burst_frames = HistoryFrameReader::new();
    while dirty.len() < 2 {
        let message = split_next_message(
            &mut dashboard,
            &mut burst_frames,
            burst_deadline,
            "split burst recovery",
        );
        match message {
            ServerMessage::Event(ServerEvent::Output {
                session,
                revision,
                bytes,
            }) if session == left.id || session == right.id => {
                assert_eq!(
                    revision, resized_focus_right_view.revision,
                    "old-revision burst output arrived after replacement"
                );
                let _ = bytes;
            }
            ServerMessage::Event(ServerEvent::ScreenDirty { session, revision })
                if session == left.id || session == right.id =>
            {
                assert_eq!(
                    revision, resized_focus_right_view.revision,
                    "old-revision burst dirty notification arrived after replacement"
                );
                dirty.insert(session);
            }
            _ => {}
        }
    }
    let recovered_view = DashboardView {
        revision: 5,
        ..resized_focus_right_view.clone()
    };
    let recovered_messages = split_view_messages(&mut dashboard, 12, &recovered_view);
    let recovered_parsers = split_view_parsers(&recovered_messages, &recovered_view);
    assert!(
        recovered_parsers[&left.id]
            .screen()
            .contents()
            .contains("LEFT_FINAL")
    );
    assert!(
        recovered_parsers[&right.id]
            .screen()
            .contents()
            .contains("RIGHT_FINAL")
    );

    drop(dashboard);
    assert!(group_exists(left_pgid), "left process group died on detach");
    assert!(
        group_exists(right_pgid),
        "right process group died on detach"
    );
    assert!(
        group_exists(hidden_pgid),
        "hidden process group died on detach"
    );

    let mut reattached = connect_server(&fixture.socket).unwrap();
    reattached
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    reattached
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(matches!(
        dashboard_request(&mut reattached, 1, Request::DashboardHello),
        Response::Hierarchy(_)
    ));
    let singleton_view = DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            session: left.id,
            size: ovrcr::session::TerminalSize { rows: 26, cols: 29 },
        }],
        focused: Some(left.id),
    };
    let singleton_messages = split_view_messages(&mut reattached, 2, &singleton_view);
    let singleton_parsers = split_view_parsers(&singleton_messages, &singleton_view);
    assert!(
        singleton_parsers[&left.id]
            .screen()
            .contents()
            .contains("LEFT_FINAL")
    );

    let rebuilt_view = DashboardView {
        revision: 2,
        panes: resized_view.panes.clone(),
        focused: Some(right.id),
    };
    let rebuilt_messages = split_view_messages(&mut reattached, 3, &rebuilt_view);
    let rebuilt_parsers = split_view_parsers(&rebuilt_messages, &rebuilt_view);
    assert!(
        rebuilt_parsers[&left.id]
            .screen()
            .contents()
            .contains("LEFT_FINAL")
    );
    assert!(
        rebuilt_parsers[&right.id]
            .screen()
            .contents()
            .contains("RIGHT_FINAL")
    );
    drop(reattached);

    fixture.shutdown_kill();
    for pgid in [left_pgid, right_pgid, hidden_pgid] {
        assert!(wait_group_absent(pgid, Duration::from_secs(2)));
    }
}

fn wait_for_file_contents(path: &Path, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if std::fs::read_to_string(path)
            .map(|contents| contents.trim() == expected)
            .unwrap_or(false)
        {
            return;
        }
        thread::yield_now();
    }
    panic!("{} did not contain {expected:?}", path.display());
}

fn select_screen_containing(
    stream: &mut UnixStream,
    request_id: u64,
    session: SessionId,
    marker: &str,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        write_frame(
            stream,
            &ClientMessage {
                request_id,
                request: Request::Select {
                    session,
                    size: ovrcr::session::TerminalSize {
                        rows: 40,
                        cols: 120,
                    },
                },
            },
        )
        .unwrap();
        while Instant::now() < deadline {
            let Ok(message) = read_frame::<ServerMessage>(stream) else {
                break;
            };
            if let ServerMessage::Response {
                response: Response::Screen { bytes, .. },
                ..
            } = message
                && String::from_utf8_lossy(&bytes).contains(marker)
            {
                return;
            }
        }
    }
    panic!("session {session:?} did not render {marker:?}");
}

#[test]
fn dashboard_shutdown_acknowledges_through_writer_before_teardown() {
    let mut fixture = ServerFixture::new();
    fixture.start();
    let mut dashboard = connect_server(&fixture.paths.socket).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 10,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 11,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    assert_eq!(
        read_frame::<ServerMessage>(&mut dashboard).unwrap(),
        ServerMessage::Response {
            request_id: 11,
            response: Response::Ok,
        }
    );
    drop(dashboard);
    let deadline = Instant::now() + Duration::from_secs(2);
    while fixture.paths.socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    fixture.thread.take().unwrap().join().unwrap();
    assert!(!fixture.paths.socket.exists());
}

#[test]
fn cli_resolves_relative_project_paths_against_invocation_cwd_with_existing_server() {
    let _env_lock = env_lock();
    let fixture = ControlFixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args([
            "project",
            "add",
            "relative",
            ".",
            "--workspace-root",
            "../workspaces",
        ])
        .current_dir(&fixture.repo)
        .env("OVRCR_CONFIG", fixture._root.path().join("config.toml"))
        .env("OVRCR_SOCKET", &fixture.socket)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "relative project registration failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let registry = load_registry(&fixture._root.path().join("config.toml")).unwrap();
    let project = registry
        .projects
        .iter()
        .find(|project| project.name == "relative")
        .unwrap();
    assert_eq!(project.repo, fixture.repo);
    assert_eq!(project.workspace_root, fixture.workspace_root);
    assert_eq!(
        fixture.request(Request::Shutdown { kill: false }),
        Response::Ok
    );
    fixture.join();
}

#[test]
fn cli_exit_preserves_session_and_shutdown_kill_cleans_up() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let workspaces = root.path().join("workspaces");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&workspaces).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "OVRCR Tests"]);
    git(&repo, &["config", "user.email", "tests@example.invalid"]);
    std::fs::write(repo.join("README"), "fixture\n").unwrap();
    git(&repo, &["add", "README"]);
    git(&repo, &["commit", "-m", "initial"]);

    let config = root.path().join("config.toml");
    let socket = root.path().join("server.sock");
    save_registry_atomic(&Registry::default(), &config).unwrap();
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let mut cleanup = CliLifecycleGuard::new(bin, &config, &socket);
    let server = Command::new(bin)
        .arg("server")
        .env("OVRCR_CONFIG", &config)
        .env("OVRCR_SOCKET", &socket)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let server_pid = server.id();
    cleanup.server = Some(server);
    wait_for_socket(&socket);

    cli(
        bin,
        &config,
        &socket,
        &[
            "project",
            "add",
            "demo",
            repo.to_str().unwrap(),
            "--workspace-root",
            workspaces.to_str().unwrap(),
        ],
    );
    cli(
        bin,
        &config,
        &socket,
        &[
            "workspace",
            "create",
            "--project",
            "demo",
            "--name",
            "one",
            "--new-branch",
            "feature/one",
            "--base",
            "main",
        ],
    );
    let output = cli_with_output(
        bin,
        &config,
        &socket,
        &[
            "new",
            "--project",
            "demo",
            "--workspace",
            "one",
            "--name",
            "agent",
            "--",
            "sh",
            "-c",
            "printf '%s\\n' DETACHED_READY; while :; do sleep 1; done",
        ],
    );
    let agent: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap();
    let agent = SessionId(agent);

    let marker_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < marker_deadline {
        if dashboard_screen(&socket, agent).contains("DETACHED_READY") {
            break;
        }
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(dashboard_screen(&socket, agent).contains("DETACHED_READY"));
    assert!(dashboard_screen(&socket, agent).contains("DETACHED_READY"));
    let groups = session_process_groups(&socket);
    assert!(
        !groups.is_empty(),
        "live session process groups must be observable"
    );

    cli(bin, &config, &socket, &["shutdown", "--kill"]);
    let mut server = cleanup.server.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && server.try_wait().unwrap().is_none() {
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        server.try_wait().unwrap().is_some(),
        "server process did not exit"
    );
    assert!(!pid_exists(server_pid));
    assert!(!socket.exists());
    assert!(groups.iter().all(|pgid| !group_exists(*pgid)));
    cleanup.completed = true;
}

fn git(repo: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
    );
}

fn cli(bin: &str, config: &Path, socket: &Path, args: &[&str]) {
    let output = cli_with_output(bin, config, socket, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn cli_with_output(bin: &str, config: &Path, socket: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin)
        .args(args)
        .env("OVRCR_CONFIG", config)
        .env("OVRCR_SOCKET", socket)
        .output()
        .unwrap()
}

fn wait_for_socket(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !socket.exists() && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(socket.exists(), "server socket did not appear");
}

/// Connect as the dashboard, replacing one that was just dropped.
///
/// The server releases the dashboard slot when it notices the previous
/// connection closed, which can trail the drop by a few milliseconds; a
/// refused hello is retried rather than treated as a failure.
fn connect_dashboard(socket: &Path, request_id: u64) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut stream = connect_server(socket).unwrap();
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id,
                request: Request::DashboardHello,
            },
        )
        .unwrap();
        match read_frame::<ServerMessage>(&mut stream).unwrap() {
            ServerMessage::Response {
                response: Response::Error { .. },
                ..
            } => {
                drop(stream);
                assert!(Instant::now() < deadline, "dashboard slot was not released");
                thread::park_timeout(Duration::from_millis(10));
            }
            _ => return stream,
        }
    }
}

fn dashboard_screen(socket: &Path, session: SessionId) -> String {
    let mut stream = connect_dashboard(socket, 1);
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
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    loop {
        if let ServerMessage::Response {
            request_id: 2,
            response: Response::Screen { bytes, .. },
        } = read_frame::<ServerMessage>(&mut stream).unwrap()
        {
            let mut parser = vt100::Parser::new(24, 80, 0);
            parser.process(&bytes);
            return parser.screen().contents();
        }
    }
}

fn session_process_groups(socket: &Path) -> Vec<libc::pid_t> {
    let mut stream = connect_server(socket).unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 3,
            request: Request::List,
        },
    )
    .unwrap();
    let ServerMessage::Response {
        response: Response::Hierarchy(snapshot),
        ..
    } = read_frame::<ServerMessage>(&mut stream).unwrap()
    else {
        panic!("unexpected list response");
    };
    snapshot
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .filter_map(|session| session.pid)
        .map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) })
        .filter(|pgid| *pgid > 1)
        .collect()
}

fn pid_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn group_exists(pgid: libc::pid_t) -> bool {
    unsafe { libc::kill(-pgid, 0) == 0 }
}

fn wait_for_group_absent(pgid: libc::pid_t, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !group_exists(pgid) {
            return;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    panic!("PTY process group {pgid} did not disappear");
}

struct CliLifecycleGuard {
    bin: String,
    config: PathBuf,
    socket: PathBuf,
    server: Option<Child>,
    completed: bool,
}

impl CliLifecycleGuard {
    fn new(bin: &str, config: &Path, socket: &Path) -> Self {
        Self {
            bin: bin.into(),
            config: config.into(),
            socket: socket.into(),
            server: None,
            completed: false,
        }
    }
}

impl Drop for CliLifecycleGuard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let _ = cli_with_output(
            &self.bin,
            &self.config,
            &self.socket,
            &["shutdown", "--kill"],
        );
        if let Some(server) = self.server.as_mut() {
            if server.try_wait().ok().flatten().is_none() {
                let _ = server.kill();
            }
            let _ = server.wait();
        }
    }
}

fn write_claude_exec_fixture(path: &Path) {
    std::fs::write(
        path,
        r#"#!/bin/sh
if [ "$1" = --version ]; then
  printf '2.1.267 (Claude Code)\n'
  exit 0
fi
OVRCR_TEST_UUID="$2" exec "$OVRCR_TEST_EXECUTABLE" --ignored --exact agent_run_native_exec_helper --nocapture
"#,
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

fn unix_socket_path(fd: libc::c_int, peer: bool) -> Option<Vec<u8>> {
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    let result = unsafe {
        if peer {
            libc::getpeername(
                fd,
                (&mut address as *mut libc::sockaddr_un).cast(),
                &mut length,
            )
        } else {
            libc::getsockname(
                fd,
                (&mut address as *mut libc::sockaddr_un).cast(),
                &mut length,
            )
        }
    };
    if result != 0 || address.sun_family != libc::AF_UNIX as libc::sa_family_t {
        return None;
    }
    let address_start = (&address as *const libc::sockaddr_un).cast::<u8>();
    let path_start = address.sun_path.as_ptr().cast::<u8>();
    let path_offset = unsafe { path_start.offset_from(address_start) as usize };
    let path_length = (length as usize)
        .saturating_sub(path_offset)
        .min(address.sun_path.len());
    let bytes = unsafe { std::slice::from_raw_parts(path_start, path_length) };
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    (end > 0).then(|| bytes[..end].to_vec())
}

#[test]
#[ignore = "native exec descriptor fixture launched through agent run"]
fn agent_run_native_exec_helper() {
    use std::fmt::Write as _;
    use std::io::{BufRead as _, Write as _};
    use std::os::unix::ffi::OsStrExt as _;

    let probe = PathBuf::from(std::env::var_os("OVRCR_TEST_PROBE").unwrap());
    let stay_alive = std::env::var("OVRCR_NATIVE_STAY_ALIVE").as_deref() == Ok("1");
    if stay_alive {
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
    }
    let expected_watch = std::env::var_os("OVRCR_EXPECT_WATCH_SOCKET").unwrap();
    let private_listener = std::env::var_os("OVRCR_AGENT_SOCKET").unwrap();
    std::fs::write(
        probe.with_extension("socket"),
        Path::new(&private_listener).as_os_str().as_bytes(),
    )
    .unwrap();
    let expected_watch = Path::new(&expected_watch).as_os_str().as_bytes();
    let private_listener = Path::new(&private_listener).as_os_str().as_bytes();
    let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    let descriptor_limit = if limit.rlim_cur == libc::RLIM_INFINITY {
        65_536
    } else {
        limit.rlim_cur.min(65_536) as libc::c_int
    };
    let mut metadata = String::new();
    let mut watch_inherited = false;
    let mut listener_inherited = false;
    for fd in 0..descriptor_limit {
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
            continue;
        }
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(fd, &mut stat) }, 0);
        let kind = stat.st_mode & libc::S_IFMT;
        if kind == libc::S_IFSOCK {
            let local = unix_socket_path(fd, false);
            let peer = unix_socket_path(fd, true);
            watch_inherited |=
                local.as_deref() == Some(expected_watch) || peer.as_deref() == Some(expected_watch);
            listener_inherited |= local.as_deref() == Some(private_listener)
                || peer.as_deref() == Some(private_listener);
            writeln!(
                metadata,
                "fd={fd} type=socket local={} peer={}",
                local
                    .as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_else(|| "-".into()),
                peer.as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_else(|| "-".into())
            )
            .unwrap();
        } else {
            let kind = if kind == libc::S_IFCHR {
                "character"
            } else if kind == libc::S_IFREG {
                "regular"
            } else if kind == libc::S_IFIFO {
                "fifo"
            } else {
                "other"
            };
            writeln!(metadata, "fd={fd} type={kind}").unwrap();
        }
    }
    for fd in 0..=2 {
        assert!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0,
            "stdio fd {fd} closed"
        );
        if std::env::var("OVRCR_EXPECT_TTY").as_deref() == Ok("1") {
            assert_eq!(unsafe { libc::isatty(fd) }, 1, "stdio fd {fd} lost its PTY");
        }
    }
    writeln!(metadata, "supervisor_watch_inherited={watch_inherited}").unwrap();
    writeln!(metadata, "private_listener_inherited={listener_inherited}").unwrap();
    std::fs::write(probe.with_extension("fds"), metadata).unwrap();
    assert!(
        !watch_inherited,
        "native exec inherited the supervisor watch socket"
    );
    assert!(
        !listener_inherited,
        "native exec inherited the private listener socket"
    );

    let conversation = std::env::var("OVRCR_TEST_UUID").unwrap();
    if let Some(transcript) = std::env::var_os("OVRCR_TEST_TRANSCRIPT") {
        let row = serde_json::json!({"type":"assistant","sessionId":conversation,"isSidechain":false,"requestId":"lifecycle","message":{"id":"lifecycle","usage":{"input_tokens":7,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":3}}});
        std::fs::write(&transcript, format!("{row}\n")).unwrap();
    }
    std::fs::write(
        &probe,
        format!(
            "{} {} {}\n",
            std::process::id(),
            unsafe { libc::getpgrp() },
            unsafe { libc::getppid() }
        ),
    )
    .unwrap();
    std::fs::write(probe.with_extension("uuid"), &conversation).unwrap();
    let payload = serde_json::to_vec(&serde_json::json!({
        "hook_event_name": "SessionStart",
        "source": "startup",
        "session_id": conversation,
        "transcript_path": std::env::var_os("OVRCR_TEST_TRANSCRIPT")
            .map(|path| path.to_string_lossy().into_owned()),
    }))
    .unwrap();
    ovrcr::report::send_claude_hook(&payload, Instant::now() + Duration::from_secs(1)).unwrap();
    println!("NATIVE_EXEC_STDOUT_READY");
    eprintln!("NATIVE_EXEC_STDERR_READY");
    std::io::stdout().flush().unwrap();
    std::io::stderr().flush().unwrap();
    if stay_alive {
        loop {
            thread::park_timeout(Duration::from_secs(1));
        }
    }
    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input).unwrap();
    println!("NATIVE_EXEC_STDIN={}", input.trim_end());
}

fn owned_collector_group(supervisor_pid: libc::pid_t) -> libc::pid_t {
    let processes = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,command="])
        .output()
        .unwrap();
    assert!(processes.status.success());
    let children: Vec<_> = String::from_utf8(processes.stdout)
        .unwrap()
        .lines()
        .filter_map(|row| {
            let mut fields = row.split_whitespace();
            let pid = fields.next()?.parse::<libc::pid_t>().ok()?;
            let parent = fields.next()?.parse::<libc::pid_t>().ok()?;
            let group = fields.next()?.parse::<libc::pid_t>().ok()?;
            (parent == supervisor_pid && fields.any(|arg| arg == "__agent-collector"))
                .then_some((pid, group))
        })
        .collect();
    assert_eq!(children.len(), 1, "expected one task-owned collector");
    assert_eq!(children[0].0, children[0].1, "collector must own its group");
    children[0].1
}

#[test]
fn agent_run_supervisor_sigkill_releases_reporting_watch() {
    use ovrcr::protocol::{
        ActivitySample, AgentActivity, AgentCommand, AgentObservation, AgentProvider, AgentReport,
        AgentUpdate, ProviderReport, ReporterHealth, Response, SampleQuality, SupervisorRequest,
        UsageCoverage,
    };
    let fixture = ControlFixture::new_bounded();
    let setup = fixture.create_hook_child("setup", "agent-supervisor-crash-setup");
    let setup_pgid = fixture.original_pgid(setup.session);
    let proxy = AdmissionProxy::new(
        fixture._root.path().join("supervisor-watch.sock"),
        fixture.socket.clone(),
        AdmissionFault::Passthrough,
    );
    let native = fixture._root.path().join("claude");
    write_claude_exec_fixture(&native);
    let probe = fixture._root.path().join("supervisor-crash-probe");
    let transcript = fixture._root.path().join("supervisor-crash.jsonl");
    let summary = fixture.create_session_summary(
        "agent-supervisor-crash",
        vec![
            "sh".into(),
            "-c".into(),
            r#"stty -echo; printf '%s\n%s\n%s\n' "$OVRCR_HOOK_SOCKET" "$OVRCR_SESSION_ID" "$OVRCR_HOOK_TOKEN" > "$4.identity"; export OVRCR_HOOK_SOCKET="$5" OVRCR_TEST_EXECUTABLE="$3" OVRCR_TEST_PROBE="$4" OVRCR_EXPECT_WATCH_SOCKET="$5" OVRCR_EXPECT_TTY=1 OVRCR_TEST_TRANSCRIPT="$6" OVRCR_NATIVE_STAY_ALIVE=1; "$1" agent run --provider claude -- "$2" --agent fixture-root --setting-sources "" --settings "path with spaces" --strict-mcp-config; code=$?; printf 'KILLED_SUPERVISOR_EXIT=%s\nOUTER_SURVIVED\n' "$code"; while :; do sleep 1; done"#.into(),
            "agent-supervisor-crash-fixture".into(),
            env!("CARGO_BIN_EXE_ovrcr").into(),
            native.clone().into_os_string(),
            std::env::current_exe().unwrap().into_os_string(),
            probe.clone().into_os_string(),
            proxy.path.clone().into_os_string(),
            transcript.into_os_string(),
        ],
    );
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "NATIVE_EXEC_STDOUT_READY");
    let old_processes: Vec<libc::pid_t> = std::fs::read_to_string(&probe)
        .unwrap()
        .split_whitespace()
        .map(|value| value.parse().unwrap())
        .collect();
    let (native_pid, native_pgid, supervisor_pid) =
        (old_processes[0], old_processes[1], old_processes[2]);
    assert_eq!(
        native_pid, native_pgid,
        "native fixture must own its process group"
    );
    assert_ne!(supervisor_pid, summary.pid.unwrap() as libc::pid_t);
    fixture.process_groups.lock().unwrap().push(native_pgid);
    let deadline = Instant::now() + Duration::from_secs(3);
    let old_snapshot = loop {
        if let Some(snapshot) = fixture.session_summary(summary.id).agent
            && snapshot.metrics.is_some()
        {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "root binding and retained metrics were not ready"
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    assert_eq!(old_snapshot.binding.provider, AgentProvider::Claude);
    assert_eq!(old_snapshot.health.state, ReporterHealth::Connected);
    let old_auth = proxy
        .supervisor
        .lock()
        .unwrap()
        .clone()
        .expect("proxy did not observe the supervisor reservation");
    let old_binding = proxy
        .binding
        .lock()
        .unwrap()
        .clone()
        .expect("proxy did not observe the root binding");
    assert_eq!(old_binding, old_snapshot.binding);
    let collector_pgid = owned_collector_group(supervisor_pid);
    fixture.process_groups.lock().unwrap().push(collector_pgid);

    assert_eq!(unsafe { libc::kill(supervisor_pid, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(2);
    let disconnected = loop {
        let current = fixture
            .session_summary(summary.id)
            .agent
            .expect("bound snapshot disappeared after supervisor crash");
        if current.health.reason.as_deref() == Some("supervisor_disconnected") {
            break current;
        }
        assert!(
            Instant::now() < deadline,
            "server retained ownership after supervisor SIGKILL"
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    assert!(
        wait_group_absent(collector_pgid, Duration::from_secs(2)),
        "collector group survived supervisor death"
    );
    assert_eq!(disconnected.health.state, ReporterHealth::Unavailable);
    assert_ne!(
        disconnected.metrics.unwrap().sample.usage.value.coverage,
        UsageCoverage::Complete
    );
    assert_eq!(
        unsafe { libc::kill(native_pid, 0) },
        0,
        "native did not survive supervisor"
    );
    fixture.wait_terminal_contains(summary.id, "OUTER_SURVIVED");
    assert_eq!(
        unsafe { libc::kill(summary.pid.unwrap() as libc::pid_t, 0) },
        0,
        "outer fixture shell did not survive supervisor"
    );

    let identity = std::fs::read_to_string(probe.with_extension("identity")).unwrap();
    let identity: Vec<_> = identity.lines().map(str::to_owned).collect();
    let capability = parse_hook_capability(&identity[2]).unwrap();
    let replacement_probe = fixture._root.path().join("replacement-probe");
    let replacement_transcript = fixture._root.path().join("replacement.jsonl");
    let replacement = fixture.create_session_summary(
        "agent-supervisor-replacement",
        vec![
            "sh".into(),
            "-c".into(),
            r#"stty -echo; export OVRCR_HOOK_SOCKET="$5" OVRCR_SESSION_ID="$6" OVRCR_HOOK_TOKEN="$7" OVRCR_TEST_EXECUTABLE="$3" OVRCR_TEST_PROBE="$4" OVRCR_EXPECT_WATCH_SOCKET="$5" OVRCR_EXPECT_TTY=1 OVRCR_TEST_TRANSCRIPT="$8"; "$1" agent run --provider claude -- "$2" --agent fixture-root --setting-sources "" --settings "path with spaces" --strict-mcp-config; code=$?; printf 'REPLACEMENT_FINISHED=%s\n' "$code"; IFS= read -r done; printf REPLACEMENT_OUTER_FINISHED; exit "$code""#.into(),
            "agent-supervisor-replacement-fixture".into(),
            env!("CARGO_BIN_EXE_ovrcr").into(),
            native.into_os_string(),
            std::env::current_exe().unwrap().into_os_string(),
            replacement_probe.clone().into_os_string(),
            identity[0].clone().into(),
            identity[1].clone().into(),
            identity[2].clone().into(),
            replacement_transcript.into_os_string(),
        ],
    );
    fixture.record_process_group(&replacement);
    fixture.wait_terminal_contains(replacement.id, "NATIVE_EXEC_STDOUT_READY");
    let replacement_processes: Vec<libc::pid_t> = std::fs::read_to_string(&replacement_probe)
        .unwrap()
        .split_whitespace()
        .map(|value| value.parse().unwrap())
        .collect();
    let replacement_pgid = replacement_processes[1];
    let replacement_supervisor_pid = replacement_processes[2];
    fixture
        .process_groups
        .lock()
        .unwrap()
        .push(replacement_pgid);
    let replacement_uuid =
        std::fs::read_to_string(replacement_probe.with_extension("uuid")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let replacement_binding = loop {
        if let Some(snapshot) = fixture.session_summary(summary.id).agent
            && snapshot.binding.conversation == replacement_uuid
        {
            break snapshot.binding;
        }
        assert!(
            Instant::now() < deadline,
            "replacement invocation did not bind"
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    assert_ne!(replacement_binding, old_binding);

    let late_report = fixture.request(Request::AgentReport(AgentReport {
        session: summary.id,
        capability,
        sequence: None,
        update: AgentUpdate::Provider(ProviderReport {
            binding: old_binding.clone(),
            revision: 99,
            observation: AgentObservation::Activity(ActivitySample {
                state: AgentActivity::Busy,
                quality: SampleQuality::Observed,
                turn: None,
            }),
        }),
    }));
    assert!(matches!(late_report, Response::Error { .. }));
    let late_release = fixture.request(Request::Supervisor(SupervisorRequest {
        auth: old_auth,
        operation: "late-old-release".into(),
        command: AgentCommand::Release {
            expected_binding: Some(old_binding),
        },
    }));
    assert!(matches!(late_release, Response::Error { .. }));
    let after_late = fixture.session_summary(summary.id).agent.unwrap();
    assert_eq!(after_late.binding, replacement_binding);
    assert_eq!(after_late.health.state, ReporterHealth::Connected);

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: replacement.id,
            text: "finish".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(replacement.id, "NATIVE_EXEC_STDIN=finish");
    fixture.wait_terminal_contains(replacement.id, "REPLACEMENT_FINISHED=0");
    assert!(wait_group_absent(replacement_pgid, Duration::from_secs(2)));
    wait_pid_absent(replacement_supervisor_pid, Duration::from_secs(2));
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: replacement.id,
            text: "done".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(replacement.id, "REPLACEMENT_OUTER_FINISHED");
    fixture.wait_exited(replacement.id);
    assert!(wait_group_absent(
        replacement.pid.unwrap() as libc::pid_t,
        Duration::from_secs(2)
    ));

    assert_eq!(unsafe { libc::kill(-native_pgid, libc::SIGKILL) }, 0);
    assert!(wait_group_absent(native_pgid, Duration::from_secs(2)));
    let callback_socket =
        PathBuf::from(std::fs::read_to_string(probe.with_extension("socket")).unwrap());
    let callback_directory = callback_socket.parent().unwrap();
    assert!(
        callback_directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ovrcr-a-")
    );
    std::fs::remove_dir_all(callback_directory).unwrap();
    assert!(!callback_directory.exists());
    assert_eq!(
        fixture.request(Request::KillSession {
            session: summary.id,
        }),
        Response::Ok
    );
    fixture.wait_exited(summary.id);
    assert!(wait_group_absent(
        summary.pid.unwrap() as libc::pid_t,
        Duration::from_secs(2)
    ));
    assert_eq!(
        fixture.request(Request::KillSession {
            session: setup.session,
        }),
        Response::Ok
    );
    fixture.wait_exited(setup.session);
    assert!(wait_group_absent(setup_pgid, Duration::from_secs(2)));
    assert!(
        fixture
            .process_groups
            .lock()
            .unwrap()
            .iter()
            .copied()
            .all(|pgid| !group_exists(pgid))
    );
}

#[test]
fn agent_run_native_exec_does_not_inherit_supervisor_lease() {
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "agent-exec-descriptor-setup");
    let native = fixture._root.path().join("claude");
    write_claude_exec_fixture(&native);
    let probe = fixture._root.path().join("exec-descriptor-probe");
    let transcript = fixture._root.path().join("exec-descriptor.jsonl");
    let summary = fixture.create_session_summary(
        "agent-exec-descriptor",
        vec![
            "sh".into(),
            "-c".into(),
            r#"stty -echo; export OVRCR_TEST_EXECUTABLE="$3" OVRCR_TEST_PROBE="$4" OVRCR_EXPECT_WATCH_SOCKET="$5" OVRCR_EXPECT_TTY=1 OVRCR_TEST_TRANSCRIPT="$6"; "$1" agent run --provider claude -- "$2" --agent fixture-root --setting-sources "" --settings "path with spaces" --strict-mcp-config; code=$?; printf 'NATIVE_EXEC_FINISHED=%s\n' "$code"; IFS= read -r done; printf OUTER_EXEC_FINISHED; exit "$code""#.into(),
            "agent-exec-descriptor-fixture".into(),
            env!("CARGO_BIN_EXE_ovrcr").into(),
            native.into_os_string(),
            std::env::current_exe().unwrap().into_os_string(),
            probe.clone().into_os_string(),
            fixture.socket.clone().into_os_string(),
            transcript.into_os_string(),
        ],
    );
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "NATIVE_EXEC_STDOUT_READY");
    fixture.wait_terminal_contains(summary.id, "NATIVE_EXEC_STDERR_READY");
    let descriptors = std::fs::read_to_string(probe.with_extension("fds")).unwrap();
    assert!(
        descriptors
            .lines()
            .any(|line| line == "fd=0 type=character")
    );
    assert!(
        descriptors
            .lines()
            .any(|line| line == "fd=1 type=character")
    );
    assert!(
        descriptors
            .lines()
            .any(|line| line == "fd=2 type=character")
    );
    assert!(
        descriptors
            .lines()
            .any(|line| line == "supervisor_watch_inherited=false")
    );
    assert!(
        descriptors
            .lines()
            .any(|line| line == "private_listener_inherited=false")
    );
    let bound = fixture
        .session_summary(summary.id)
        .agent
        .expect("native callback did not bind the invocation");
    assert_eq!(
        bound.binding.conversation,
        std::fs::read_to_string(probe.with_extension("uuid")).unwrap()
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "descriptor-input".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "NATIVE_EXEC_STDIN=descriptor-input");
    fixture.wait_terminal_contains(summary.id, "NATIVE_EXEC_FINISHED=0");
    let native_pgid = std::fs::read_to_string(&probe)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<libc::pid_t>()
        .unwrap();
    assert!(
        wait_group_absent(native_pgid, Duration::from_secs(2)),
        "native process group remained after completion"
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "done".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "OUTER_EXEC_FINISHED");
    fixture.wait_exited(summary.id);
    assert!(wait_group_absent(
        summary.pid.unwrap() as libc::pid_t,
        Duration::from_secs(2)
    ));
}

#[test]
#[ignore = "native child fixture launched through agent run"]
fn agent_run_native_helper() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    let path = std::path::PathBuf::from(std::env::var_os("OVRCR_NATIVE_PROBE").unwrap());
    let interrupts = Arc::new(AtomicUsize::new(0));
    let terminated = Arc::new(AtomicBool::new(false));
    let count = interrupts.clone();
    unsafe {
        signal_hook::low_level::register(libc::SIGINT, move || {
            count.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    }
    signal_hook::flag::register(libc::SIGTERM, terminated.clone()).unwrap();
    std::fs::write(
        &path,
        format!(
            "{} {} {} {}\n",
            std::process::id(),
            unsafe { libc::getpgrp() },
            unsafe { libc::getppid() },
            unsafe { libc::tcgetpgrp(0) }
        ),
    )
    .unwrap();
    assert!(std::env::var_os("OVRCR_HOOK_SOCKET").is_none());
    assert!(std::env::var_os("OVRCR_SESSION_ID").is_none());
    assert!(std::env::var_os("OVRCR_HOOK_TOKEN").is_none());
    let endpoint = std::env::var_os("OVRCR_AGENT_SOCKET").expect("private invocation endpoint");
    let token = std::env::var("OVRCR_AGENT_TOKEN").expect("private invocation token");
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(
        std::fs::metadata(&endpoint).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(std::path::Path::new(&endpoint).parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    std::fs::write(path.with_extension("endpoint"), endpoint.as_encoded_bytes()).unwrap();
    let mut wrong = UnixStream::connect(&endpoint).unwrap();
    wrong
        .write_all(format!("{}\n", "0".repeat(64)).as_bytes())
        .unwrap();
    let mut rejected = String::new();
    wrong.read_to_string(&mut rejected).unwrap();
    assert!(rejected.is_empty(), "incorrect token authenticated");
    let mut callback = UnixStream::connect(endpoint).unwrap();
    use std::io::{Read as _, Write as _};
    callback.write_all(format!("{token}\n").as_bytes()).unwrap();
    let mut response = String::new();
    callback.read_to_string(&mut response).unwrap();
    assert_eq!(response, "admission-unavailable\n");
    let report = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args(["report", "activity", "--state", "busy"])
        .output()
        .unwrap();
    assert!(!report.status.success());
    assert!(
        String::from_utf8_lossy(&report.stderr).contains("admission is unavailable"),
        "{report:?}"
    );
    let mut modes: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(0, &mut modes) }, 0);
    modes.c_lflag |= libc::ECHO;
    assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &modes) }, 0);
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::ioctl(0, libc::TIOCGWINSZ, &mut size) }, 0);
    assert_eq!((size.ws_row, size.ws_col), (40, 120));
    println!("NATIVE_READY");
    let mut last = 0;
    loop {
        if terminated.load(Ordering::SeqCst) {
            println!("NATIVE_TERM");
            std::process::exit(23);
        }
        let next = interrupts.load(Ordering::SeqCst);
        if next != last {
            std::fs::write(path.with_extension("interrupts"), next.to_string()).unwrap();
            println!("NATIVE_INTERRUPTS={next}");
            last = next;
        }
        let mut poll = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 20) } > 0 {
            let mut bytes = [0u8; 128];
            let n = unsafe { libc::read(0, bytes.as_mut_ptr().cast(), bytes.len()) };
            if n > 0 {
                assert_eq!(unsafe { libc::tcgetattr(0, &mut modes) }, 0);
                assert_ne!(
                    modes.c_lflag & libc::ECHO,
                    0,
                    "native modes must survive job-control stop"
                );
                println!(
                    "NATIVE_INPUT={}",
                    String::from_utf8_lossy(&bytes[..n as usize]).trim()
                );
            }
        }
    }
}

#[test]
fn agent_run_owns_native_group_and_restores_terminal_after_forwarded_term() {
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "agent-run-setup");
    let probe = fixture._root.path().join("native-probe");
    let summary = fixture.create_session_summary("agent-run", vec![
        "sh".into(), "-c".into(),
        r#"stty -echo; printf '%s\n%s\n%s\n' "$OVRCR_HOOK_SOCKET" "$OVRCR_SESSION_ID" "$OVRCR_HOOK_TOKEN" > "$3.identity"; stty -g > "$3.before"; export OVRCR_NATIVE_PROBE="$3"; "$1" agent run --provider claude -- "$2" --ignored --exact agent_run_native_helper --nocapture; code=$?; stty -g > "$3.after"; printf 'WRAPPER_FINISHED=%s\n' "$code"; IFS= read -r done; printf SHELL_RESTORED; exit "$code""#.into(),
        "agent-run-fixture".into(), env!("CARGO_BIN_EXE_ovrcr").into(), std::env::current_exe().unwrap().into_os_string(), probe.clone().into_os_string(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "NATIVE_READY");
    assert_eq!(
        fixture.session_summary(summary.id).agent_epoch,
        1,
        "reserve before native spawn"
    );
    assert!(
        fixture.session_summary(summary.id).agent.is_none(),
        "admission remains unavailable"
    );
    let values: Vec<i32> = std::fs::read_to_string(&probe)
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    let (pid, pgid, launcher, foreground) = (values[0], values[1], values[2], values[3]);
    assert_eq!(pgid, pid, "native must own its process group");
    assert_ne!(pgid, summary.pid.unwrap() as i32);
    assert_eq!(
        foreground, pgid,
        "native owns inherited terminal foreground"
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "\u{3}".into(),
            submit: false
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "NATIVE_INTERRUPTS=1");
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "after-interrupt".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "NATIVE_INPUT=after-interrupt");
    assert_eq!(
        std::fs::read_to_string(probe.with_extension("interrupts")).unwrap(),
        "1"
    );
    let peer = PausePeer {
        pid,
        pgid,
        address: probe.clone(),
        preexit_marker: String::new(),
    };
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "\u{1a}".into(),
            submit: false
        }),
        Response::Ok
    );
    wait_peer_stopped(&peer, Duration::from_secs(2));
    wait_peer_stopped(
        &PausePeer {
            pid: launcher,
            pgid: summary.pid.unwrap() as i32,
            address: probe.clone(),
            preexit_marker: String::new(),
        },
        Duration::from_secs(2),
    );
    assert_eq!(
        fixture.request(Request::ResumeSession {
            session: summary.id
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "after-continue".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "NATIVE_INPUT=after-continue");
    assert_eq!(
        fixture.request(Request::PauseSession {
            session: summary.id
        }),
        Response::Ok
    );
    wait_peer_stopped(&peer, Duration::from_secs(2));
    assert_eq!(
        fixture.request(Request::ResumeSession {
            session: summary.id
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "after-runtime-resume".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "NATIVE_INPUT=after-runtime-resume");
    let identity = std::fs::read_to_string(probe.with_extension("identity")).unwrap();
    let identity: Vec<_> = identity.lines().collect();
    let invoke = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
        command
            .args([
                "agent",
                "run",
                "--provider",
                "claude",
                "--",
                "/bin/sh",
                "-c",
                "printf SECOND_NATIVE",
            ])
            .env("OVRCR_HOOK_SOCKET", identity[0])
            .env("OVRCR_SESSION_ID", identity[1])
            .env("OVRCR_HOOK_TOKEN", identity[2]);
        command
    };
    let conflict = invoke().output().unwrap();
    assert!(
        !conflict.status.success(),
        "active reservation must refuse native spawn"
    );
    assert!(
        conflict.stdout.is_empty(),
        "conflicting native command spawned"
    );
    assert_eq!(fixture.session_summary(summary.id).agent_epoch, 1);
    assert_eq!(unsafe { libc::kill(launcher, libc::SIGTERM) }, 0);
    fixture.wait_terminal_contains(summary.id, "WRAPPER_FINISHED=23");
    assert_eq!(
        std::fs::read_to_string(probe.with_extension("before")).unwrap(),
        std::fs::read_to_string(probe.with_extension("after")).unwrap(),
        "restore shell termios"
    );
    let failed = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args([
            "agent",
            "run",
            "--provider",
            "claude",
            "--",
            "/definitely-missing-ovrcr-native",
        ])
        .env("OVRCR_HOOK_SOCKET", identity[0])
        .env("OVRCR_SESSION_ID", identity[1])
        .env("OVRCR_HOOK_TOKEN", identity[2])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let reused = invoke().output().unwrap();
    assert!(
        reused.status.success(),
        "released lease must permit next invocation: {:?}",
        reused
    );
    assert_eq!(reused.stdout, b"SECOND_NATIVE");
    assert_eq!(fixture.session_summary(summary.id).agent_epoch, 3);
    assert!(fixture.session_summary(summary.id).agent.is_none());
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "finish".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "SHELL_RESTORED");
    let endpoint = std::fs::read_to_string(probe.with_extension("endpoint")).unwrap();
    assert!(
        !std::path::Path::new(&endpoint).exists(),
        "private invocation socket remains"
    );
    println!(
        "AGENT_OWNERSHIP native_pid={pid} native_pgid={pgid} launcher_pid={launcher} session_pgid={} private_endpoint_removed=true",
        summary.pid.unwrap()
    );
    assert!(
        wait_group_absent(pgid, Duration::from_secs(2)),
        "native group remains"
    );
}

#[test]
fn agent_run_runtime_kill_reaches_owned_native_group() {
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "agent-kill-setup");
    let probe = fixture._root.path().join("kill-probe");
    let summary=fixture.create_session_summary("agent-kill",vec![
        "sh".into(),"-c".into(),r#"export OVRCR_NATIVE_PROBE="$3"; exec "$1" agent run --provider claude -- "$2" --ignored --exact agent_run_native_helper --nocapture"#.into(),
        "agent-kill-fixture".into(),env!("CARGO_BIN_EXE_ovrcr").into(),std::env::current_exe().unwrap().into_os_string(),probe.clone().into_os_string(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "NATIVE_READY");
    let values: Vec<i32> = std::fs::read_to_string(&probe)
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(values[0], values[1]);
    assert_eq!(
        fixture.request(Request::KillSession {
            session: summary.id
        }),
        Response::Ok
    );
    fixture.wait_exited(summary.id);
    assert!(wait_group_absent(values[1], Duration::from_secs(2)));
    assert!(wait_group_absent(
        summary.pid.unwrap() as i32,
        Duration::from_secs(2)
    ));
}

#[test]
fn agent_run_channel_failure_releases_reservation_before_native_fallback() {
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "agent-channel-failure-setup");
    let blocked = fixture._root.path().join("not-a-directory");
    std::fs::write(&blocked, "blocked").unwrap();
    let identity_path = fixture._root.path().join("fallback.identity");
    let summary=fixture.create_session_summary("agent-channel-failure",vec![
        "sh".into(),"-c".into(),
        r#"stty -echo; printf '%s\n%s\n%s\n' "$OVRCR_HOOK_SOCKET" "$OVRCR_SESSION_ID" "$OVRCR_HOOK_TOKEN" > "$3"; TMPDIR="$2" OVRCR_AGENT_SOCKET=outer-socket OVRCR_AGENT_TOKEN=outer-secret "$1" agent run --provider claude -- /bin/sh -c 'test -z "${OVRCR_AGENT_SOCKET+x}${OVRCR_AGENT_TOKEN+x}${OVRCR_HOOK_SOCKET+x}${OVRCR_SESSION_ID+x}${OVRCR_HOOK_TOKEN+x}" || exit 99; printf FALLBACK_READY; IFS= read -r line; exit 17'; code=$?; printf 'FALLBACK_EXIT=%s\n' "$code"; exit "$code""#.into(),
        "agent-channel-failure".into(),env!("CARGO_BIN_EXE_ovrcr").into(),blocked.into_os_string(),identity_path.clone().into_os_string(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "FALLBACK_READY");
    assert_eq!(
        fixture.session_summary(summary.id).agent_epoch,
        1,
        "the reservation was allocated before channel setup"
    );
    let identity = std::fs::read_to_string(identity_path).unwrap();
    let identity: Vec<_> = identity.lines().collect();
    let next = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args([
            "agent",
            "run",
            "--provider",
            "claude",
            "--",
            "/bin/sh",
            "-c",
            "printf NEXT_RESERVED",
        ])
        .env("OVRCR_HOOK_SOCKET", identity[0])
        .env("OVRCR_SESSION_ID", identity[1])
        .env("OVRCR_HOOK_TOKEN", identity[2])
        .output()
        .unwrap();
    assert!(
        next.status.success(),
        "reservation retained during untracked native fallback: {next:?}"
    );
    assert_eq!(next.stdout, b"NEXT_RESERVED");
    assert_eq!(fixture.session_summary(summary.id).agent_epoch, 2);
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "finish".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "FALLBACK_EXIT=17");
    fixture.wait_exited(summary.id);
}

#[test]
#[ignore = "native admission fixture launched through agent run"]
fn agent_admission_native_helper() {
    use std::io::{BufRead, Write};
    let expected = std::env::var("OVRCR_TEST_UUID").unwrap();
    let probe = std::path::PathBuf::from(std::env::var_os("OVRCR_TEST_PROBE").unwrap());
    std::fs::write(&probe, &expected).unwrap();
    std::fs::write(
        probe.with_extension("channel"),
        format!(
            "{}\n{}\n",
            std::env::var("OVRCR_AGENT_SOCKET").unwrap(),
            std::env::var("OVRCR_AGENT_TOKEN").unwrap()
        ),
    )
    .unwrap();
    let write_transcript = || {
        if let Some(path) = std::env::var_os("OVRCR_TEST_TRANSCRIPT") {
            let row = serde_json::json!({"type":"assistant","sessionId":expected,"isSidechain":false,"requestId":"r","message":{"id":"m","usage":{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":1}}});
            std::fs::write(path, format!("{row}\n")).unwrap();
        }
    };
    if std::env::var("OVRCR_TEST_DELAY_TRANSCRIPT").as_deref() != Ok("1") {
        write_transcript();
    }
    println!("ADMISSION_READY");
    for (index, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.unwrap();
        if line == "exit" {
            break;
        }
        let mut payload = serde_json::json!({"hook_event_name":"SessionStart","source":"startup","session_id":expected,"agent_type":"fixture-root"});
        if let Some(path) = std::env::var_os("OVRCR_TEST_TRANSCRIPT") {
            payload["transcript_path"] = path.to_string_lossy().as_ref().into();
        }
        match line.as_str() {
            "kill-collector" => {
                let parent = unsafe { libc::getppid() };
                let processes = Command::new("ps")
                    .args(["-axo", "pid=,ppid=,command="])
                    .output()
                    .unwrap();
                assert!(processes.status.success());
                let collector = String::from_utf8(processes.stdout)
                    .unwrap()
                    .lines()
                    .find_map(|row| {
                        let mut fields = row.split_whitespace();
                        let pid = fields.next()?.parse::<i32>().ok()?;
                        let ppid = fields.next()?.parse::<i32>().ok()?;
                        let command = fields.collect::<Vec<_>>().join(" ");
                        (ppid == parent && command.contains("__agent-collector")).then_some(pid)
                    })
                    .expect("task-owned collector child");
                assert_eq!(unsafe { libc::kill(-collector, libc::SIGKILL) }, 0);
                println!("ADMISSION_CALLBACK={index}");
                continue;
            }
            "create-transcript" => write_transcript(),
            "grow-transcript" => {
                use std::io::Write;
                let path = std::env::var_os("OVRCR_TEST_TRANSCRIPT").unwrap();
                let row = serde_json::json!({"type":"assistant","sessionId":expected,"isSidechain":false,"requestId":"r2","message":{"id":"m2","usage":{"input_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":5}}});
                writeln!(
                    std::fs::OpenOptions::new().append(true).open(path).unwrap(),
                    "{row}"
                )
                .unwrap();
            }
            "statusline"
            | "statusline-replay"
            | "statusline-wrong"
            | "statusline-unknown"
            | "statusline-lower"
            | "statusline-context-unknown" => {
                payload = serde_json::json!({"session_id":expected,"cost":{"total_cost_usd":0.25},"context_window":{"context_window_size":100,"current_usage":{"input_tokens":20,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}});
                if line == "statusline-wrong" {
                    payload["session_id"] = "foreign".into();
                }
                if line == "statusline-unknown" {
                    payload["cost"] = serde_json::Value::Null;
                }
                if line == "statusline-lower" {
                    payload["cost"]["total_cost_usd"] = 0.1.into();
                }
                if line == "statusline-context-unknown" {
                    payload["context_window"]["current_usage"] = serde_json::Value::Null;
                    payload["context_window"]["used_percentage"] = serde_json::Value::Null;
                    payload["context_window"]["remaining_percentage"] = serde_json::Value::Null;
                }
            }
            "wrong" => payload["session_id"] = "wrong-conversation".into(),
            "resume-root" => payload["source"] = "resume".into(),
            "missing-source" => {
                payload.as_object_mut().unwrap().remove("source");
            }
            "child" => payload["agent_id"] = "child-1".into(),
            "malformed-child" => payload["agent_id"] = serde_json::Value::Null,
            "empty-child" => payload["agent_id"] = "".into(),
            "numeric-child" => payload["agent_id"] = 17.into(),
            "child-event" => payload["hook_event_name"] = "SubagentStart".into(),
            "clear" => {
                payload["hook_event_name"] = "SessionEnd".into();
                payload["reason"] = "clear".into();
            }
            "branch" => {
                payload["source"] = "fork".into();
                payload["session_id"] = "uncertified-other-root".into();
            }
            "resume-transition" => payload["source"] = "resume".into(),
            "compact" => {
                payload["hook_event_name"] = "SessionStart".into();
                payload["source"] = "compact".into();
            }
            "end-other" => {
                payload["hook_event_name"] = "SessionEnd".into();
                payload["reason"] = "logout".into();
            }
            "root" | "foreign" | "generic" => {}
            value if value.starts_with("activity:") => {
                let parts: Vec<_> = value.split(':').collect();
                payload["hook_event_name"] = parts[1].into();
                if parts[2] != "missing" {
                    payload["prompt_id"] = parts[2].into();
                }
                if parts.get(3) == Some(&"child") {
                    payload["agent_id"] = "child".into();
                }
                if parts.get(3) == Some(&"wrong") {
                    payload["session_id"] = "foreign".into();
                }
                if parts.get(3) == Some(&"malformed") {
                    payload["prompt_id"] = serde_json::Value::Null;
                }
                if parts.get(3) == Some(&"control") {
                    payload["prompt_id"] = "bad\nprompt".into();
                }
                if parts.get(3) == Some(&"oversized") {
                    payload["prompt_id"] = "p".repeat(257).into();
                }
                if parts[1] == "StopFailure" {
                    payload["error"] = "rate_limit".into();
                }
                payload["notification_type"] = if parts.get(3) == Some(&"generic") {
                    "idle_prompt"
                } else {
                    "permission_prompt"
                }
                .into();
                payload["stop_hook_active"] = true.into();
            }
            _ => panic!("unknown admission fixture instruction"),
        }
        if line == "foreign" {
            let request = serde_json::to_vec(
                &serde_json::json!({"provider":"pi","origin":"claude-hook","payload":payload}),
            )
            .unwrap();
            let mut stream =
                UnixStream::connect(std::env::var_os("OVRCR_AGENT_SOCKET").unwrap()).unwrap();
            stream
                .write_all(format!("{}\n", std::env::var("OVRCR_AGENT_TOKEN").unwrap()).as_bytes())
                .unwrap();
            stream
                .write_all(&(request.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&request).unwrap();
            let mut response = String::new();
            std::io::Read::read_to_string(&mut stream, &mut response).unwrap();
            assert_eq!(response, "admission-ignored\n");
            println!("ADMISSION_CALLBACK={index}");
            continue;
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
        if line.starts_with("statusline") {
            command.args(["report", "claude-statusline", "--stdin-json"]);
        } else if line == "generic" {
            command.args(["report", "activity", "--state", "busy"]);
        } else {
            command.args(["report", "claude", "--stdin-json"]);
        }
        let mut callback = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        callback
            .stdin
            .take()
            .unwrap()
            .write_all(serde_json::to_string(&payload).unwrap().as_bytes())
            .unwrap();
        let output = callback.wait_with_output().unwrap();
        if line.starts_with("statusline") {
            assert_eq!(
                output.stdout,
                if line == "statusline-context-unknown" {
                    "ctx —\n".as_bytes()
                } else {
                    b"ctx 20%\n".as_slice()
                }
            );
        } else {
            assert!(output.stdout.is_empty(), "hooks must be stdout silent");
        }
        println!("ADMISSION_CALLBACK={index}");
    }
}

#[derive(Clone, Copy)]
enum AdmissionFault {
    Passthrough,
    LostBind,
    RejectHealth,
    RejectStatus,
    RejectActivity,
    LostFinalize,
}
struct AdmissionProxy {
    final_status_verified: std::sync::Arc<std::sync::atomic::AtomicBool>,
    supervisor: std::sync::Arc<std::sync::Mutex<Option<ovrcr::protocol::SupervisorAuth>>>,
    binding: std::sync::Arc<std::sync::Mutex<Option<ovrcr::protocol::AgentBinding>>>,
    path: std::path::PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    streams: std::sync::Arc<std::sync::Mutex<Vec<UnixStream>>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl AdmissionProxy {
    fn new(path: std::path::PathBuf, target: std::path::PathBuf, fault: AdmissionFault) -> Self {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let streams = Arc::new(Mutex::new(Vec::new()));
        let thread_stop = stop.clone();
        let thread_streams = streams.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let final_operation = Arc::new(Mutex::new(None::<String>));
        let final_status_verified = Arc::new(AtomicBool::new(false));
        let thread_final_status = final_status_verified.clone();
        let supervisor = Arc::new(Mutex::new(None));
        let thread_supervisor = supervisor.clone();
        let binding = Arc::new(Mutex::new(None));
        let thread_binding = binding.clone();
        let thread = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut front, _)) => {
                        front.set_nonblocking(false).unwrap();
                        thread_streams
                            .lock()
                            .unwrap()
                            .push(front.try_clone().unwrap());
                        let target = target.clone();
                        let streams = thread_streams.clone();
                        let dropped = dropped.clone();
                        let final_operation = final_operation.clone();
                        let final_status_verified = thread_final_status.clone();
                        let supervisor = thread_supervisor.clone();
                        let binding = thread_binding.clone();
                        handlers.push(thread::spawn(move || {
                            if ovrcr::protocol::exchange_preamble(&mut front).is_err() {
                                return;
                            }
                            let Ok(mut upstream) = connect_server(&target) else {
                                return;
                            };
                            streams.lock().unwrap().push(upstream.try_clone().unwrap());
                            while let Ok(message) = read_frame::<ClientMessage>(&mut front) {
                                eprintln!(
                                    "ADMISSION_PROXY request={}",
                                    match &message.request {
                                        Request::Inspect => "inspect",
                                        Request::ReserveAgent(_) => "reserve",
                                        Request::SupervisorHello(_) => "hello",
                                        Request::AgentStatus { .. } => "status",
                                        Request::Supervisor(_) => "supervisor",
                                        _ => "other",
                                    }
                                );
                                let reject = matches!(
                                    (&message.request, fault),
                                    (
                                        Request::Supervisor(ovrcr::protocol::SupervisorRequest {
                                            command: ovrcr::protocol::AgentCommand::Health(_),
                                            ..
                                        }),
                                        AdmissionFault::RejectHealth
                                    ) | (Request::AgentStatus { .. }, AdmissionFault::RejectStatus)
                                        | (Request::AgentReport(_), AdmissionFault::RejectActivity)
                                );
                                if reject {
                                    if matches!(fault, AdmissionFault::RejectHealth) {
                                        continue;
                                    }
                                    if write_frame(
                                        &mut front,
                                        &ServerMessage::Response {
                                            request_id: message.request_id,
                                            response: Response::Error {
                                                code: ovrcr::protocol::ErrorCode::Conflict,
                                                message: "fixture rejected publication".into(),
                                            },
                                        },
                                    )
                                    .is_err()
                                    {
                                        break;
                                    }
                                    continue;
                                }
                                if write_frame(&mut upstream, &message).is_err() {
                                    break;
                                }
                                let Ok(response) = read_frame::<ServerMessage>(&mut upstream)
                                else {
                                    break;
                                };
                                if let (
                                    Request::ReserveAgent(reserve),
                                    ServerMessage::Response {
                                        response:
                                            Response::AgentOperation(
                                                ovrcr::protocol::AgentOperationResult::Reserved(
                                                    reservation,
                                                ),
                                            ),
                                        ..
                                    },
                                ) = (&message.request, &response)
                                {
                                    *supervisor.lock().unwrap() =
                                        Some(ovrcr::protocol::SupervisorAuth {
                                            session: reserve.session,
                                            lease: reservation.lease.clone(),
                                        });
                                }
                                if let ServerMessage::Response {
                                    response:
                                        Response::AgentOperation(
                                            ovrcr::protocol::AgentOperationResult::Bound(current),
                                        ),
                                    ..
                                } = &response
                                {
                                    *binding.lock().unwrap() = Some(current.clone());
                                }
                                let is_bind = matches!(
                                    &message.request,
                                    Request::Supervisor(ovrcr::protocol::SupervisorRequest {
                                        command: ovrcr::protocol::AgentCommand::Bind { .. },
                                        ..
                                    })
                                );
                                if let Request::Supervisor(ovrcr::protocol::SupervisorRequest {
                                    command: ovrcr::protocol::AgentCommand::Finalize { .. },
                                    operation,
                                    ..
                                }) = &message.request
                                {
                                    *final_operation.lock().unwrap() = Some(operation.clone());
                                }
                                if let Request::AgentStatus { operation, .. } = &message.request
                                    && final_operation.lock().unwrap().as_ref() == Some(operation)
                                    && matches!(
                                        &response,
                                        ServerMessage::Response {
                                            response: Response::AgentOperation(
                                                ovrcr::protocol::AgentOperationResult::Released
                                            ),
                                            ..
                                        }
                                    )
                                {
                                    final_status_verified.store(true, Ordering::SeqCst);
                                }
                                let is_finalize = matches!(
                                    &message.request,
                                    Request::Supervisor(ovrcr::protocol::SupervisorRequest {
                                        command: ovrcr::protocol::AgentCommand::Finalize { .. },
                                        ..
                                    })
                                );
                                if is_finalize
                                    && matches!(fault, AdmissionFault::LostFinalize)
                                    && !dropped.swap(true, Ordering::SeqCst)
                                {
                                    continue;
                                }
                                if is_bind
                                    && matches!(
                                        fault,
                                        AdmissionFault::LostBind | AdmissionFault::RejectStatus
                                    )
                                    && !dropped.swap(true, Ordering::SeqCst)
                                {
                                    continue;
                                }
                                if write_frame(&mut front, &response).is_err() {
                                    break;
                                }
                            }
                            let _ = upstream.shutdown(std::net::Shutdown::Both);
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::park_timeout(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        Self {
            path,
            stop,
            streams,
            thread: Some(thread),
            final_status_verified,
            supervisor,
            binding,
        }
    }
}
impl Drop for AdmissionProxy {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        for stream in self.streams.lock().unwrap().iter() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
#[test]
fn agent_admission_lost_bind_then_clear_resolves_without_another_startup() {
    assert_initial_admission("clear", Some(AdmissionFault::LostBind));
}
#[test]
fn agent_admission_failed_unavailable_publication_disconnects_watch() {
    assert_initial_admission("clear", Some(AdmissionFault::RejectHealth));
}
#[test]
fn agent_admission_failed_bind_status_disconnects_watch() {
    assert_initial_admission("clear", Some(AdmissionFault::RejectStatus));
}

#[test]
fn agent_admission_private_claude_route_binds_once_and_clear_retains_lease() {
    assert_initial_admission("clear", None);
}
#[test]
fn agent_admission_branch_freezes_without_replacement_or_reopening() {
    assert_initial_admission("branch", None);
}

#[test]
fn agent_admission_explicit_resume_preserves_argv_and_collects_conversation_usage() {
    assert_agent_admission_resume("--resume", "2.1.267");
}

#[test]
fn agent_admission_short_resume_on_2_1_268_preserves_argv_and_lifecycle() {
    assert_agent_admission_resume("-r", "2.1.268");
}

fn assert_agent_admission_resume(resume_flag: &str, version: &str) {
    use std::os::unix::fs::PermissionsExt;
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "resume-admission-setup");
    let native = fixture._root.path().join("claude");
    std::fs::write(
        &native,
        format!(r#"#!/bin/sh
if [ "$1" = --version ]; then printf '{version} (Claude Code)\n'; exit 0; fi
printf '%s\n' "$@" > "$OVRCR_TEST_PROBE.argv"
OVRCR_TEST_UUID="$2" exec "$OVRCR_TEST_EXECUTABLE" --ignored --exact agent_admission_native_helper --nocapture
"#),
    )
    .unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let expected = "5ebc5f9b-54b5-4928-9955-dc81c23743dd";
    let make_session = |name: &str, probe: &std::path::Path, transcript: &std::path::Path| {
        fixture.create_session_summary(name, vec![
            "sh".into(),
            "-c".into(),
            r#"stty -echo; export OVRCR_HOOK_SOCKET="$7" OVRCR_TEST_PROBE="$4" OVRCR_TEST_EXECUTABLE="$5" OVRCR_TEST_TRANSCRIPT="$6"; "$1" agent run --provider claude -- "$2" "$8" "$3" --agent fixture-root --setting-sources "" --settings "path with spaces" --strict-mcp-config; printf RESUME_ADMISSION_FINISHED; IFS= read -r done"#.into(),
            "resume-admission-fixture".into(),
            env!("CARGO_BIN_EXE_ovrcr").into(),
            native.clone().into_os_string(),
            expected.into(),
            probe.as_os_str().into(),
            std::env::current_exe().unwrap().into_os_string(),
            transcript.as_os_str().into(),
            fixture.socket.clone().into_os_string(),
            resume_flag.into(),
        ])
    };

    let rejected_probe = fixture._root.path().join("wrong-source-resume-probe");
    let rejected_transcript = fixture._root.path().join("wrong-source-resume.jsonl");
    let rejected = make_session("wrong-source-resume", &rejected_probe, &rejected_transcript);
    fixture.record_process_group(&rejected);
    fixture.wait_terminal_contains_until(
        rejected.id,
        "ADMISSION_READY",
        Instant::now() + Duration::from_secs(5),
    );
    for (index, command) in ["root", "resume-root"].into_iter().enumerate() {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: rejected.id,
                text: command.into(),
                submit: true,
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(rejected.id, &format!("ADMISSION_CALLBACK={index}"));
        assert!(
            fixture.session_summary(rejected.id).agent.is_none(),
            "{command} reopened admission after contradictory startup source"
        );
    }
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: rejected.id,
            text: "exit".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(rejected.id, "RESUME_ADMISSION_FINISHED");
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: rejected.id,
            text: "done".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_exited(rejected.id);

    let probe = fixture._root.path().join("resume-admission-probe");
    let transcript = fixture._root.path().join("resumed-root.jsonl");
    let summary = make_session("resume-admission", &probe, &transcript);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains_until(
        summary.id,
        "ADMISSION_READY",
        Instant::now() + Duration::from_secs(5),
    );
    assert_eq!(
        std::fs::read_to_string(probe.with_extension("argv")).unwrap(),
        format!(
            "{resume_flag}\n{expected}\n--agent\nfixture-root\n--setting-sources\n\n--settings\npath with spaces\n--strict-mcp-config\n"
        )
    );
    assert_eq!(std::fs::read_to_string(&probe).unwrap(), expected);
    assert!(fixture.session_summary(summary.id).agent.is_none());

    for (index, command) in ["wrong", "child", "missing-source", "resume-root"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true,
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={index}"));
        if command != "resume-root" {
            assert!(
                fixture.session_summary(summary.id).agent.is_none(),
                "{command} admitted the resumed conversation"
            );
        }
    }
    let bound = fixture
        .session_summary(summary.id)
        .agent
        .expect("matching resume root did not bind");
    assert_eq!(bound.binding.conversation, expected);

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let current = fixture.session_summary(summary.id).agent.unwrap();
        if current.metrics.as_ref().is_some_and(|metrics| {
            metrics.sample.usage.value.input_tokens == Some(10)
                && metrics.sample.usage.value.output_tokens == Some(1)
        }) {
            assert_eq!(
                current.metrics.unwrap().sample.usage.value.scope,
                ovrcr::protocol::UsageScope::Conversation
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "pre-invocation transcript usage was not collected"
        );
        thread::park_timeout(Duration::from_millis(5));
    }

    for (index, command) in [
        "activity:UserPromptSubmit:A",
        "activity:Stop:A",
        "grow-transcript",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true,
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={}", index + 4));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let current = fixture.session_summary(summary.id).agent.unwrap();
        if current.metrics.as_ref().is_some_and(|metrics| {
            metrics.sample.usage.value.input_tokens == Some(60)
                && metrics.sample.usage.value.output_tokens == Some(6)
        }) {
            assert_eq!(
                current.activity.unwrap().state,
                ovrcr::session::AgentActivity::Idle
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "new resumed-turn transcript record was not counted once"
        );
        thread::park_timeout(Duration::from_millis(5));
    }

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "compact".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "ADMISSION_CALLBACK=7");
    let compact = fixture.session_summary(summary.id).agent.unwrap();
    assert_eq!(compact.binding, bound.binding);
    assert_eq!(
        compact.health.state,
        ovrcr::protocol::ReporterHealth::Connected
    );

    for (index, command) in ["resume-transition", "clear", "branch", "resume-root"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true,
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={}", index + 8));
        let current = fixture.session_summary(summary.id).agent.unwrap();
        assert_eq!(current.binding, bound.binding);
        assert_eq!(
            current.health.state,
            ovrcr::protocol::ReporterHealth::Unavailable
        );
    }

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "RESUME_ADMISSION_FINISHED");
}

fn assert_initial_admission(closing_command: &str, fault: Option<AdmissionFault>) {
    // Lifecycle gate; concurrent fresh-executable startup can exhaust the fixed probe budget.
    let _guard = env_lock();
    use std::os::unix::fs::PermissionsExt;
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "admission-setup");
    let proxy = fault.map(|fault| {
        AdmissionProxy::new(
            fixture._root.path().join("proxy.sock"),
            fixture.socket.clone(),
            fault,
        )
    });
    let reporting_socket = proxy.as_ref().map_or(&fixture.socket, |proxy| &proxy.path);

    let native = fixture._root.path().join("claude");
    std::fs::write(&native,r#"#!/bin/sh
if [ "$1" = --version ]; then
  test -z "${OVRCR_AGENT_SOCKET+x}${OVRCR_AGENT_TOKEN+x}${OVRCR_HOOK_SOCKET+x}${OVRCR_SESSION_ID+x}${OVRCR_HOOK_TOKEN+x}" || exit 99
  printf '2.1.267 (Claude Code)\n'; exit 0
fi
printf '%s\n' "$@" > "$OVRCR_TEST_PROBE.argv"
OVRCR_TEST_UUID="$2" exec "$OVRCR_TEST_EXECUTABLE" --ignored --exact agent_admission_native_helper --nocapture
"#).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let probe = fixture._root.path().join("admission-probe");
    let make_session = |name: &str, probe: &std::path::Path| {
        fixture.create_session_summary(name,vec![
        "sh".into(),"-c".into(),
        r#"stty -echo; export OVRCR_HOOK_SOCKET="$5"; printf '%s\n%s\n%s\n' "$OVRCR_HOOK_SOCKET" "$OVRCR_SESSION_ID" "$OVRCR_HOOK_TOKEN" > "$4.identity"; export OVRCR_TEST_PROBE="$4" OVRCR_TEST_EXECUTABLE="$3"; "$1" agent run --provider claude -- "$2" --agent fixture-root --setting-sources "" --settings "path with spaces" --strict-mcp-config; printf ADMISSION_FINISHED; IFS= read -r done"#.into(),
        "admission-fixture".into(),env!("CARGO_BIN_EXE_ovrcr").into(),native.clone().into_os_string(),std::env::current_exe().unwrap().into_os_string(),probe.to_path_buf().into_os_string(),reporting_socket.clone().into_os_string(),
    ])
    };
    let summary = make_session("admission", &probe);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "ADMISSION_READY");
    let expected = std::fs::read_to_string(&probe).unwrap();
    assert_eq!(expected.len(), 36, "supervisor must select a fresh UUID");
    let argv = std::fs::read_to_string(probe.with_extension("argv")).unwrap();
    assert_eq!(
        argv,
        format!(
            "--session-id\n{expected}\n--agent\nfixture-root\n--setting-sources\n\n--settings\npath with spaces\n--strict-mcp-config\n"
        )
    );
    assert_eq!(fixture.session_summary(summary.id).agent_epoch, 1);
    for (index, command) in [
        "wrong",
        "child",
        "malformed-child",
        "empty-child",
        "numeric-child",
        "child-event",
        "foreign",
        "generic",
        "root",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={index}"));
        if command != "root" {
            assert!(
                fixture.session_summary(summary.id).agent.is_none(),
                "{command} admitted"
            );
        }
    }
    let bound = fixture
        .session_summary(summary.id)
        .agent
        .expect("eligible root must bind");
    assert_eq!(bound.binding.conversation, expected);
    assert!(bound.activity.is_none());
    let identity = std::fs::read_to_string(probe.with_extension("identity")).unwrap();
    let identity: Vec<_> = identity.lines().collect();
    let capability = parse_hook_capability(identity[2]).unwrap();
    assert_eq!(
        fixture.request(Request::AgentReport(ovrcr::protocol::AgentReport {
            session: summary.id,
            capability,
            sequence: None,
            update: ovrcr::protocol::AgentUpdate::Provider(ovrcr::protocol::ProviderReport {
                binding: bound.binding.clone(),
                revision: 9,
                observation: ovrcr::protocol::AgentObservation::Activity(
                    ovrcr::protocol::ActivitySample {
                        state: ovrcr::session::AgentActivity::Busy,
                        quality: ovrcr::protocol::SampleQuality::Observed,
                        turn: None,
                    }
                ),
            }),
        })),
        Response::Ok
    );
    let commands = if fault.is_some() {
        vec![closing_command]
    } else {
        vec!["root", "end-other", closing_command, "root"]
    };
    for (index, command) in commands.into_iter().enumerate() {
        let closure_started = Instant::now();
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={}", index + 9));
        if command == closing_command {
            let deadline = Instant::now() + Duration::from_secs(2);
            while fixture
                .session_summary(summary.id)
                .agent
                .as_ref()
                .unwrap()
                .health
                .state
                == ovrcr::protocol::ReporterHealth::Connected
                && Instant::now() < deadline
            {
                thread::park_timeout(Duration::from_millis(5));
            }
        }
        if command == closing_command && fault.is_some() {
            assert!(
                closure_started.elapsed() < Duration::from_millis(1400),
                "closure exceeded its callback deadline plus fixture observation allowance"
            );
        }
        let current = fixture.session_summary(summary.id).agent.unwrap();
        assert_eq!(current.binding, bound.binding);
        assert_eq!(current.activity_revision, 9);
        if command == closing_command {
            assert_eq!(
                current.health.state,
                ovrcr::protocol::ReporterHealth::Unavailable
            );
        }
    }
    if matches!(
        fault,
        Some(AdmissionFault::RejectHealth | AdmissionFault::RejectStatus)
    ) {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: "root".into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, "ADMISSION_CALLBACK=10");
        let current = fixture.session_summary(summary.id).agent.unwrap();
        assert_eq!(current.binding, bound.binding);
        assert_eq!(
            current.health.state,
            ovrcr::protocol::ReporterHealth::Unavailable
        );
    }
    let conflict = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args([
            "agent",
            "run",
            "--provider",
            "claude",
            "--",
            "/bin/sh",
            "-c",
            "printf SHOULD_NOT_SPAWN",
        ])
        .env("OVRCR_HOOK_SOCKET", identity[0])
        .env("OVRCR_SESSION_ID", identity[1])
        .env("OVRCR_HOOK_TOKEN", identity[2])
        .output()
        .unwrap();
    if matches!(
        fault,
        Some(AdmissionFault::RejectHealth | AdmissionFault::RejectStatus)
    ) {
        assert!(
            conflict.status.success(),
            "failed reporting ownership must be relinquished"
        );
    } else {
        assert!(!conflict.status.success());
        assert!(conflict.stdout.is_empty());
    }
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "ADMISSION_FINISHED");
    if closing_command == "clear" && fault.is_none() {
        use std::io::{Read, Write};
        let old = std::fs::read_to_string(probe.with_extension("channel")).unwrap();
        let old: Vec<_> = old.lines().collect();
        assert!(
            UnixStream::connect(old[0]).is_err(),
            "completed invocation endpoint remains"
        );
        let next_probe = fixture._root.path().join("next-admission-probe");
        let next = make_session("next-admission", &next_probe);
        fixture.record_process_group(&next);
        fixture.wait_terminal_contains(next.id, "ADMISSION_READY");
        let next_uuid = std::fs::read_to_string(&next_probe).unwrap();
        assert_ne!(next_uuid, expected);
        let channel = std::fs::read_to_string(next_probe.with_extension("channel")).unwrap();
        let channel: Vec<_> = channel.lines().collect();
        let mut stale = UnixStream::connect(channel[0]).unwrap();
        stale.write_all(format!("{}\n", old[1]).as_bytes()).unwrap();
        let mut response = Vec::new();
        stale.read_to_end(&mut response).unwrap();
        assert!(response.is_empty());
        assert!(
            fixture.session_summary(next.id).agent.is_none(),
            "old token admitted new invocation"
        );
        let mut old_payload = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(["report", "claude", "--stdin-json"])
            .env("OVRCR_AGENT_SOCKET", channel[0])
            .env("OVRCR_AGENT_TOKEN", channel[1])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        old_payload.stdin.take().unwrap().write_all(serde_json::to_string(&serde_json::json!({"hook_event_name":"SessionStart","source":"startup","session_id":expected})).unwrap().as_bytes()).unwrap();
        let old_output = old_payload.wait_with_output().unwrap();
        assert!(old_output.status.success());
        assert!(old_output.stdout.is_empty());
        assert!(
            fixture.session_summary(next.id).agent.is_none(),
            "old UUID admitted new invocation"
        );

        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: next.id,
                text: "root".into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(next.id, "ADMISSION_CALLBACK=0");
        assert_eq!(
            fixture
                .session_summary(next.id)
                .agent
                .unwrap()
                .binding
                .conversation,
            next_uuid
        );
        assert_eq!(
            fixture.session_summary(summary.id).agent.unwrap().binding,
            bound.binding
        );
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: next.id,
                text: "exit".into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(next.id, "ADMISSION_FINISHED");
    }
}

#[test]
fn agent_admission_ineligible_argv_and_probe_failures_preserve_native_arguments() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "admission-argv-setup");
    let native = fixture._root.path().join("claude");
    std::fs::write(&native,r#"#!/bin/sh
if [ "$1" = --version ]; then
  test -z "${OVRCR_AGENT_SOCKET+x}${OVRCR_AGENT_TOKEN+x}${OVRCR_HOOK_SOCKET+x}${OVRCR_SESSION_ID+x}${OVRCR_HOOK_TOKEN+x}" || exit 99
  case "$OVRCR_TEST_VERSION" in
    fail) exit 2;;
    timeout) printf '%s' "$$" > "$OVRCR_TEST_PROBE.pid"; while :; do sleep 1; done;;
    *) printf '%s (Claude Code)\n' "$OVRCR_TEST_VERSION";;
  esac
  exit 0
fi
printf '%s\n' "$@" > "$OVRCR_TEST_PROBE.argv"
printf NATIVE_UNCHANGED
IFS= read -r line
exit 19
"#).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let blocked = fixture._root.path().join("blocked-tempdir");
    std::fs::write(&blocked, "file").unwrap();
    for (index, (argument, version, mode)) in [
        ("--resume=foreign", "2.1.267", "normal"),
        ("--resume", "2.1.268", "normal"),
        ("-r", "2.1.268", "normal"),
        ("--unknown-mode", "2.1.267", "normal"),
        ("doctor", "2.1.267", "normal"),
        ("--model=sonnet", "2.1.266", "normal"),
        ("--model=sonnet", "2.1.269", "normal"),
        ("--model=sonnet", "fail", "normal"),
        ("--model=sonnet", "timeout", "normal"),
        ("--model=sonnet", "2.1.267", "blocked"),
        ("--model=sonnet", "2.1.267", "missing"),
    ]
    .into_iter()
    .enumerate()
    {
        let probe = fixture._root.path().join(format!("argv-{index}"));
        let summary=fixture.create_session_summary(&format!("argv-{index}"),vec![
            "sh".into(),"-c".into(),
            r#"stty -echo; export OVRCR_TEST_PROBE="$3" OVRCR_TEST_VERSION="$4" OVRCR_AGENT_SOCKET=outer-socket OVRCR_AGENT_TOKEN=outer-token; case "$6" in blocked) export TMPDIR="$7";; missing) unset OVRCR_HOOK_SOCKET OVRCR_SESSION_ID OVRCR_HOOK_TOKEN;; esac; "$1" agent run --provider claude -- "$2" "$5"; code=$?; printf 'UNCHANGED_EXIT=%s\n' "$code"; exit "$code""#.into(),
            "argv-fixture".into(),env!("CARGO_BIN_EXE_ovrcr").into(),native.clone().into_os_string(),probe.clone().into_os_string(),version.into(),argument.into(),mode.into(),blocked.clone().into_os_string(),
        ]);
        fixture.record_process_group(&summary);
        fixture.wait_terminal_contains_until(
            summary.id,
            "NATIVE_UNCHANGED",
            Instant::now() + Duration::from_secs(4),
        );
        assert_eq!(
            std::fs::read_to_string(probe.with_extension("argv")).unwrap(),
            format!("{argument}\n"),
            "case {index}"
        );
        assert!(fixture.session_summary(summary.id).agent.is_none());
        if version == "timeout" {
            let pid = std::fs::read_to_string(probe.with_extension("pid"))
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                wait_group_absent(pid, Duration::from_secs(2)),
                "version probe group remained"
            );
        }
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: "finish".into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, "UNCHANGED_EXIT=19");
        fixture.wait_exited(summary.id);
    }
}

#[test]
fn private_claude_activity_tracks_only_current_root_prompt_observations() {
    assert_claude_activity(false);
}
#[test]
fn private_claude_activity_delivery_failure_disconnects_without_stopping_native() {
    assert_claude_activity(true);
}
fn assert_claude_activity(fail_delivery: bool) {
    use ovrcr::protocol::{AgentActivity, SampleQuality};
    use std::os::unix::fs::PermissionsExt;
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "activity-setup");
    let proxy = fail_delivery.then(|| {
        AdmissionProxy::new(
            fixture._root.path().join("activity-proxy.sock"),
            fixture.socket.clone(),
            AdmissionFault::RejectActivity,
        )
    });
    let reporting_socket = proxy.as_ref().map_or(&fixture.socket, |proxy| &proxy.path);
    let native = fixture._root.path().join("claude");
    std::fs::write(&native, r#"#!/bin/sh
if [ "$1" = --version ]; then printf '2.1.267 (Claude Code)\n'; exit; fi
OVRCR_TEST_UUID="$2" exec "$OVRCR_TEST_EXECUTABLE" --ignored --exact agent_admission_native_helper --nocapture
"#).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let probe = fixture._root.path().join("activity-probe");
    let summary = fixture.create_session_summary("activity", vec![
        "/bin/sh".into(), "-c".into(),
        r#"stty -echo; export OVRCR_HOOK_SOCKET="$5"; export OVRCR_TEST_PROBE="$3" OVRCR_TEST_EXECUTABLE="$4"; "$1" agent run --provider claude -- "$2"; printf ACTIVITY_FINISHED; IFS= read -r done"#.into(),
        "activity-fixture".into(), env!("CARGO_BIN_EXE_ovrcr").into(), native.into_os_string(), probe.into_os_string(), std::env::current_exe().unwrap().into_os_string(), reporting_socket.clone().into_os_string(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "ADMISSION_READY");
    let cases = [
        ("root", None, 0, None),
        (
            "activity:UserPromptSubmit:A",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        (
            "activity:UserPromptSubmit:A:control",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        (
            "activity:UserPromptSubmit:A:oversized",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        (
            "activity:Stop:A:child",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        (
            "activity:Stop:A:wrong",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        (
            "activity:Stop:A:malformed",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        ("root", Some(AgentActivity::Busy), 1, Some("A")),
        (
            "activity:PermissionRequest:A",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        (
            "activity:Notification:A:generic",
            Some(AgentActivity::Busy),
            1,
            Some("A"),
        ),
        (
            "activity:Notification:A",
            Some(AgentActivity::WaitingInput),
            2,
            Some("A"),
        ),
        ("activity:Stop:A", Some(AgentActivity::Idle), 3, Some("A")),
        (
            "activity:PreToolUse:A",
            Some(AgentActivity::Busy),
            4,
            Some("A"),
        ),
        (
            "activity:UserPromptSubmit:B",
            Some(AgentActivity::Busy),
            5,
            Some("B"),
        ),
        ("activity:Stop:A", Some(AgentActivity::Busy), 5, Some("B")),
        (
            "activity:Notification:A",
            Some(AgentActivity::Busy),
            5,
            Some("B"),
        ),
        (
            "activity:StopFailure:A",
            Some(AgentActivity::Busy),
            5,
            Some("B"),
        ),
        (
            "activity:Stop:missing",
            Some(AgentActivity::Busy),
            5,
            Some("B"),
        ),
        (
            "activity:PostToolUse:A",
            Some(AgentActivity::Busy),
            5,
            Some("B"),
        ),
        (
            "activity:StopFailure:B",
            Some(AgentActivity::Error),
            6,
            Some("B"),
        ),
        (
            "activity:PostToolUseFailure:B",
            Some(AgentActivity::Busy),
            7,
            Some("B"),
        ),
        ("clear", Some(AgentActivity::Busy), 7, Some("B")),
        ("activity:Stop:B", Some(AgentActivity::Busy), 7, Some("B")),
        (
            "activity:UserPromptSubmit:C",
            Some(AgentActivity::Busy),
            7,
            Some("B"),
        ),
    ];
    for (index, (command, state, revision, turn)) in cases.into_iter().enumerate() {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={index}"));
        let snapshot = fixture.session_summary(summary.id).agent.unwrap();
        if fail_delivery && index == 1 {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let snapshot = fixture.session_summary(summary.id).agent.unwrap();
                if snapshot.health.state == ovrcr::protocol::ReporterHealth::Unavailable {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "failed delivery left reporter connected"
                );
                thread::park_timeout(Duration::from_millis(5));
            }
            assert_eq!(snapshot.activity_revision, 0);
            assert_eq!(
                fixture.request(Request::SendTerminal {
                    session: summary.id,
                    text: "activity:UserPromptSubmit:B".into(),
                    submit: true
                }),
                Response::Ok
            );
            fixture.wait_terminal_contains(summary.id, "ADMISSION_CALLBACK=2");
            let closed = fixture.session_summary(summary.id).agent.unwrap();
            assert_eq!(closed.binding, snapshot.binding);
            assert_eq!(closed.activity_revision, 0);
            assert_eq!(
                closed.health.state,
                ovrcr::protocol::ReporterHealth::Unavailable
            );
            break;
        }
        if command.ends_with(":control") || command.ends_with(":oversized") {
            assert_eq!(
                snapshot.health.state,
                ovrcr::protocol::ReporterHealth::Connected
            );
        }
        assert_eq!(snapshot.activity_revision, revision, "{command}");
        assert_eq!(
            snapshot.activity.as_ref().map(|a| a.state),
            state,
            "{command}"
        );
        if let Some(activity) = snapshot.activity {
            assert_eq!(activity.quality, SampleQuality::Observed);
            assert_eq!(activity.turn.as_deref(), turn);
        }
        assert_eq!(snapshot.binding.generation, 1);
    }
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "ACTIVITY_FINISHED");
}

#[test]
fn fresh_pty_does_not_inherit_outer_managed_reporting_channel() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "fresh_pty_outer_channel_helper",
            "--nocapture",
        ])
        .env("OVRCR_AGENT_SOCKET", root.path().join("stale-outer.sock"))
        .env("OVRCR_AGENT_TOKEN", "7".repeat(64))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "isolated inherited environment fixture subprocess"]
fn fresh_pty_outer_channel_helper() {
    assert!(std::env::var_os("OVRCR_AGENT_SOCKET").is_some());
    assert!(std::env::var_os("OVRCR_AGENT_TOKEN").is_some());
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "fresh-channel-setup");
    let evidence = fixture._root.path().join("fresh-env");
    let summary = fixture.create_session_summary("fresh-channel", vec![
        "/bin/sh".into(), "-c".into(),
        r#"printf '%s%s' "${OVRCR_AGENT_SOCKET+x}" "${OVRCR_AGENT_TOKEN+x}" > "$2"; "$1" report activity --state busy; printf FRESH_REPORT_FINISHED; IFS= read -r done"#.into(),
        "fresh-report".into(), env!("CARGO_BIN_EXE_ovrcr").into(), evidence.clone().into_os_string(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains(summary.id, "FRESH_REPORT_FINISHED");
    assert_eq!(
        fixture.session_summary(summary.id).activity,
        ovrcr::session::AgentActivity::Busy,
        "fresh capability must reach its own runtime instead of the outer private endpoint"
    );
    assert_eq!(
        std::fs::read_to_string(evidence).unwrap(),
        "",
        "private outer channel must not reach a fresh PTY"
    );
}

#[test]
fn claude_metrics_route_collects_partial_usage_and_finalizes_native_exit() {
    assert_claude_metrics_completion(false, false, false, false);
}
#[test]
fn claude_metrics_finalization_recovers_original_receipt_after_lost_ack() {
    assert_claude_metrics_completion(true, false, false, false);
}
#[test]
fn claude_metrics_missing_initial_file_preserves_activity_and_recovers_same_path() {
    assert_claude_metrics_completion(false, true, false, false);
}
#[test]
fn claude_metrics_clear_freezes_components_and_prevents_reader_reopening() {
    assert_claude_metrics_completion(false, false, true, false);
}
#[test]
fn claude_metrics_collector_loss_propagates_while_native_stays_usable() {
    assert_claude_metrics_completion(false, false, false, true);
}
fn assert_claude_metrics_completion(
    lose_ack: bool,
    missing: bool,
    clear: bool,
    kill_collector: bool,
) {
    use std::os::unix::fs::PermissionsExt;
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "metrics-setup");
    let proxy = lose_ack.then(|| {
        AdmissionProxy::new(
            fixture._root.path().join("metrics-proxy.sock"),
            fixture.socket.clone(),
            AdmissionFault::LostFinalize,
        )
    });
    let socket = proxy.as_ref().map_or(&fixture.socket, |proxy| &proxy.path);
    let native = fixture._root.path().join("claude");
    std::fs::write(&native, r#"#!/bin/sh
if [ "$1" = --version ]; then printf '2.1.267 (Claude Code)\n'; exit; fi
OVRCR_TEST_UUID="$2" exec "$OVRCR_TEST_EXECUTABLE" --ignored --exact agent_admission_native_helper --nocapture
"#).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let probe = fixture._root.path().join("metrics-probe");
    let transcript = fixture._root.path().join("root-transcript.jsonl");
    let summary = fixture.create_session_summary("metrics", vec![
        "/bin/sh".into(), "-c".into(),
        r#"stty -echo; export OVRCR_HOOK_SOCKET="$5" OVRCR_TEST_PROBE="$3" OVRCR_TEST_EXECUTABLE="$4" OVRCR_TEST_TRANSCRIPT="$6" OVRCR_TEST_DELAY_TRANSCRIPT="$7"; "$1" agent run --provider claude -- "$2"; printf METRICS_FINISHED; IFS= read -r done"#.into(),
        "metrics-fixture".into(), env!("CARGO_BIN_EXE_ovrcr").into(), native.into_os_string(), probe.into_os_string(), std::env::current_exe().unwrap().into_os_string(), socket.clone().into_os_string(), transcript.into_os_string(), if missing { "1" } else { "0" }.into(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains_until(
        summary.id,
        "ADMISSION_READY",
        Instant::now() + Duration::from_secs(5),
    );
    let commands = if missing {
        vec![
            "root",
            "activity:UserPromptSubmit:A",
            "statusline",
            "create-transcript",
        ]
    } else {
        vec!["root", "statusline"]
    };
    for (index, command) in commands.into_iter().enumerate() {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={index}"));
        if missing && index == 2 {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let snapshot = fixture.session_summary(summary.id).agent.unwrap();
                if snapshot.health.state == ovrcr::protocol::ReporterHealth::Unavailable {
                    assert_eq!(
                        snapshot.activity.unwrap().state,
                        ovrcr::protocol::AgentActivity::Busy
                    );
                    assert_eq!(
                        snapshot.metrics.unwrap().sample.context.value.used_tokens,
                        Some(20)
                    );
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "missing source health must be visible"
                );
                thread::park_timeout(Duration::from_millis(5));
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let before = loop {
        let snapshot = fixture.session_summary(summary.id).agent.unwrap();
        if snapshot.metrics.as_ref().is_some_and(|m| {
            m.sample.usage.value.input_tokens == Some(10)
                && m.sample.context.value.used_tokens == Some(20)
        }) && snapshot.health.state == ovrcr::protocol::ReporterHealth::Connected
        {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "actual statusline/collector metrics not published"
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    if kill_collector {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: "kill-collector".into(),
                submit: true,
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, "ADMISSION_CALLBACK=2");
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let current = fixture.session_summary(summary.id);
            let agent = current.agent.unwrap();
            if agent.health.reason.as_deref() == Some("collector_unavailable") {
                assert!(matches!(
                    current.phase,
                    ovrcr::session::SessionPhase::Running
                ));
                assert_eq!(agent.binding, before.binding);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "collector loss was not propagated"
            );
            thread::park_timeout(Duration::from_millis(5));
        }
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: "statusline".into(),
                submit: true,
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, "ADMISSION_CALLBACK=3");
    }
    if !missing {
        for (index, command) in ["statusline-replay", "statusline-wrong"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                fixture.request(Request::SendTerminal {
                    session: summary.id,
                    text: command.into(),
                    submit: true
                }),
                Response::Ok
            );
            fixture
                .wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={}", index + 2));
            let snapshot = fixture.session_summary(summary.id).agent.unwrap();
            assert_eq!(snapshot.metrics_revision, before.metrics_revision);
            assert_eq!(snapshot.metrics, before.metrics);
        }
    }
    if !missing && !clear && !kill_collector {
        for (index, command) in ["statusline-unknown", "statusline-lower", "statusline"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                fixture.request(Request::SendTerminal {
                    session: summary.id,
                    text: command.into(),
                    submit: true
                }),
                Response::Ok
            );
            fixture
                .wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={}", index + 4));
            let snapshot = fixture.session_summary(summary.id).agent.unwrap();
            assert_eq!(
                snapshot.health.state,
                ovrcr::protocol::ReporterHealth::Connected
            );
            assert_eq!(
                snapshot
                    .metrics
                    .unwrap()
                    .sample
                    .cost
                    .value
                    .map(|cost| cost.usd_ticks),
                if index == 2 {
                    Some(2_500_000_000)
                } else {
                    None
                }
            );
        }
    }
    if !lose_ack && !missing && !clear && !kill_collector {
        let compact_before = fixture.session_summary(summary.id).agent.unwrap();
        for (index, command) in ["compact", "statusline-context-unknown", "statusline"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                fixture.request(Request::SendTerminal {
                    session: summary.id,
                    text: command.into(),
                    submit: true
                }),
                Response::Ok
            );
            fixture
                .wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={}", index + 7));
            let snapshot = fixture.session_summary(summary.id).agent.unwrap();
            assert_eq!(snapshot.binding, before.binding);
            let metrics = snapshot.metrics.unwrap();
            assert_eq!(
                metrics.sample.usage.value,
                compact_before.metrics.as_ref().unwrap().sample.usage.value
            );
            assert_eq!(
                metrics
                    .sample
                    .cost
                    .value
                    .as_ref()
                    .map(|cost| cost.usd_ticks),
                Some(2_500_000_000)
            );
            match index {
                0 => assert_eq!(metrics, compact_before.metrics.as_ref().unwrap().clone()),
                1 => {
                    assert_eq!(metrics.sample.context.value.used_tokens, None);
                    assert_eq!(metrics.sample.context.value.capacity_tokens, Some(100));
                }
                2 => {
                    assert_eq!(metrics.sample.context.value.used_tokens, Some(20));
                    assert_eq!(metrics.sample.context.value.capacity_tokens, Some(100));
                }
                _ => unreachable!(),
            }
        }
    }
    if clear {
        for (index, command) in ["clear", "grow-transcript", "statusline-unknown", "root"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                fixture.request(Request::SendTerminal {
                    session: summary.id,
                    text: command.into(),
                    submit: true
                }),
                Response::Ok
            );
            fixture
                .wait_terminal_contains(summary.id, &format!("ADMISSION_CALLBACK={}", index + 4));
            let snapshot = fixture.session_summary(summary.id).agent.unwrap();
            assert_eq!(snapshot.metrics, before.metrics);
            assert_eq!(snapshot.binding, before.binding);
            assert_eq!(
                snapshot.health.state,
                ovrcr::protocol::ReporterHealth::Unavailable
            );
        }
    }
    let started = Instant::now();
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains_until(
        summary.id,
        "METRICS_FINISHED",
        Instant::now() + Duration::from_secs(3),
    );
    assert!(started.elapsed() < Duration::from_millis(2500));
    let after = fixture.session_summary(summary.id).agent.unwrap();
    assert_eq!(after.binding, before.binding);
    assert_eq!(
        after.health.reason.as_deref(),
        Some(if clear {
            "unfinalized_release"
        } else {
            "incomplete_final_accounting"
        })
    );
    let metrics = after.metrics.unwrap();
    assert_eq!(
        metrics.sample.usage.value.coverage,
        ovrcr::protocol::UsageCoverage::Partial
    );
    assert_eq!(metrics.sample.usage.value.input_tokens, Some(10));
    assert_eq!(metrics.sample.cost.value.unwrap().usd_ticks, 2_500_000_000);
    assert_eq!(
        metrics.context_received_unix_ms,
        before.metrics.unwrap().context_received_unix_ms
    );
    if let Some(proxy) = &proxy {
        assert!(
            proxy
                .final_status_verified
                .load(std::sync::atomic::Ordering::SeqCst),
            "must recover original committed finalization receipt without reattaching"
        );
    }
}

#[test]
fn native_exit_is_bounded_when_reporting_callback_stalls() {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "native_exit_stalled_reporting_helper",
            "--nocapture",
        ])
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.wait();
            panic!("owned reporting fixture exceeded exit bound");
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("NATIVE_EXIT_17"));
}
#[test]
#[ignore = "isolated signal handler and native supervision fixture"]
fn native_exit_stalled_reporting_helper() {
    use ovrcr_runtime::agent_runner::{HookEvent, run_native};
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("callback-entered");
    let (release, blocked) = std::sync::mpsc::channel();
    let (finished, finish) = std::sync::mpsc::channel();
    let child_marker = marker.clone();
    let started = Instant::now();
    let status = run_native(
        &[
            "/bin/sh".into(),
            "-c".into(),
            r#"while test ! -f "$1"; do sleep 0.01; done; printf NATIVE_EXIT_17; exit 17"#.into(),
            "native-exit".into(),
            child_marker.into_os_string(),
        ],
        move |available, _| {
            assert!(available);
            Some(Box::new(move |event| {
                if matches!(event, HookEvent::Poll { .. }) {
                    std::fs::write(&marker, "entered").unwrap();
                    let _ = blocked.recv();
                    let _ = finished.send(());
                }
                Vec::new()
            }))
        },
    )
    .unwrap();
    assert_eq!(status.code(), Some(17));
    assert!(started.elapsed() < Duration::from_millis(2500));
    release.send(()).unwrap();
    finish.recv_timeout(Duration::from_secs(1)).unwrap();
}

#[test]
#[ignore = "Codex synchronous native child fixture"]
fn codex_hook_native_helper() {
    use std::io::{BufRead, Write};
    if let Some(probe) = std::env::var_os("OVRCR_TEST_PROBE") {
        std::fs::write(
            probe,
            format!(
                "{}\n{}\n{}\n",
                std::env::var("OVRCR_AGENT_SOCKET").unwrap(),
                std::env::var("OVRCR_AGENT_TOKEN").unwrap(),
                std::process::id()
            ),
        )
        .unwrap();
    }
    println!("CODEX_NATIVE_READY");
    for (index, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.unwrap();
        if line == "exit" {
            std::process::exit(17);
        }
        let parts: Vec<_> = line.split(':').collect();
        let mut payload = serde_json::json!({"hook_event_name":parts[0],"session_id":parts[1],"turn_id":parts[2],"transcript_path":"/ignored/root.jsonl"});
        if parts.get(3) == Some(&"child") {
            payload["agent_id"] = "child".into();
        }
        if parts.get(3) == Some(&"missing") {
            payload.as_object_mut().unwrap().remove("turn_id");
        }
        let bytes = if parts.get(3) == Some(&"malformed") {
            b"not-json".to_vec()
        } else if parts.get(3) == Some(&"oversize") {
            vec![b'x'; 65_537]
        } else {
            payload.to_string().into_bytes()
        };
        let mut command = if parts.get(3) == Some(&"grandchild") {
            let mut command = Command::new("/bin/sh");
            command.args([
                "-c",
                "\"$1\" report codex --stdin; exit 0",
                "nested",
                env!("CARGO_BIN_EXE_ovrcr"),
            ]);
            command
        } else {
            let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
            command.args(["report", "codex", "--stdin"]);
            command
        };
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        println!("CODEX_CALLBACK={index}");
    }
}

#[test]
fn codex_managed_hooks_ready_interrupt_duplicates_and_rebind() {
    // Lifecycle/correlation gate; concurrent executable startup can exhaust the fixed probe budget.
    let _guard = env_lock();
    use ovrcr::protocol::AgentActivity;
    let fixture = ControlFixture::new_bounded();
    let (summary, probe) = codex_session(&fixture, &fixture.socket);
    let channel = std::fs::read_to_string(&probe).unwrap();
    let channel: Vec<_> = channel.lines().collect();
    // A reporter with the correct private token but a foreign OS parent cannot bind first.
    {
        use std::io::Write;
        let mut foreign = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(["report", "codex", "--stdin"])
            .env("OVRCR_AGENT_SOCKET", channel[0])
            .env("OVRCR_AGENT_TOKEN", channel[1])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        foreign.stdin.take().unwrap().write_all(br#"{"hook_event_name":"UserPromptSubmit","session_id":"foreign","turn_id":"first"}"#).unwrap();
        let output = foreign.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(fixture.session_summary(summary.id).agent.is_none());
    }
    let mut before_duplicate = None;
    for (index, (command, expected)) in [
        ("SessionStart:root:a", None),
        ("Stop:root:a:child", None),
        ("Stop:root:a", None),
        ("UserPromptSubmit:root:a", Some(AgentActivity::Busy)),
        ("Stop:root:a", Some(AgentActivity::ResponseReady)),
        (
            "UserPromptSubmit:root:a",
            Some(AgentActivity::ResponseReady),
        ),
        ("Stop:root:a", Some(AgentActivity::ResponseReady)),
        ("UserPromptSubmit:root:b", Some(AgentActivity::Busy)),
        ("Stop:root:a", Some(AgentActivity::Busy)),
        ("Interrupt:root:b", Some(AgentActivity::Idle)),
        ("Stop:root:b", Some(AgentActivity::Idle)),
        ("UserPromptSubmit:other:c", Some(AgentActivity::Busy)),
        ("Stop:root:b", Some(AgentActivity::Busy)),
        ("Stop:other:c", Some(AgentActivity::ResponseReady)),
        (
            "UserPromptSubmit:other:d:grandchild",
            Some(AgentActivity::ResponseReady),
        ),
        (
            "UserPromptSubmit:other:d:missing",
            Some(AgentActivity::ResponseReady),
        ),
        (
            "UserPromptSubmit:other:d:malformed",
            Some(AgentActivity::ResponseReady),
        ),
        (
            "UserPromptSubmit:other:d:oversize",
            Some(AgentActivity::ResponseReady),
        ),
        ("Unknown:other:d", Some(AgentActivity::ResponseReady)),
        ("UserPromptSubmit:root:e", Some(AgentActivity::Busy)),
        ("Stop:root:e", Some(AgentActivity::ResponseReady)),
        ("SessionEnd:root:e", Some(AgentActivity::ResponseReady)),
        (
            "UserPromptSubmit:root:f",
            Some(AgentActivity::ResponseReady),
        ),
        ("Stop:root:f", Some(AgentActivity::ResponseReady)),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("CODEX_CALLBACK={index}"));
        let current = fixture.session_summary(summary.id).agent;
        if let Some(expected) = expected {
            let current = current.unwrap();
            assert_eq!(
                current.activity.as_ref().unwrap().state,
                expected,
                "{command}"
            );
            if index == 4 {
                before_duplicate = Some(current.clone());
                let token = std::fs::read_to_string(probe.with_extension("capability")).unwrap();
                let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
                    .args([
                        "agent",
                        "run",
                        "codex",
                        "--",
                        "/bin/sh",
                        "-c",
                        "printf CODEX_CONFLICT_NATIVE; exit 23",
                    ])
                    .env("OVRCR_HOOK_SOCKET", &fixture.socket)
                    .env("OVRCR_SESSION_ID", summary.id.0.to_string())
                    .env("OVRCR_HOOK_TOKEN", token.trim())
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(23),
                    "reporting startup conflict prevented native launch"
                );
                assert_eq!(output.stdout, b"CODEX_CONFLICT_NATIVE");
            }
            if index == 5 || index == 6 {
                assert_eq!(
                    Some(current.clone()),
                    before_duplicate,
                    "duplicate refreshed sample"
                );
            }
            if (11..19).contains(&index) {
                assert_eq!(current.binding.conversation, "other");
                assert_eq!(current.binding.generation, 2);
            }
            if index >= 19 {
                assert_eq!(current.binding.conversation, "root");
                assert_eq!(current.binding.generation, 3);
            }
            if index == 11 {
                let token = std::fs::read_to_string(probe.with_extension("capability")).unwrap();
                let capability = parse_hook_capability(token.trim()).unwrap();
                let stale = before_duplicate.as_ref().unwrap();
                assert!(matches!(
                    fixture.request(Request::AgentReport(ovrcr::protocol::AgentReport {
                        session: summary.id,
                        capability,
                        sequence: None,
                        update: ovrcr::protocol::AgentUpdate::Provider(
                            ovrcr::protocol::ProviderReport {
                                binding: stale.binding.clone(),
                                revision: 1000,
                                observation: ovrcr::protocol::AgentObservation::Activity(
                                    ovrcr::protocol::ActivitySample {
                                        state: AgentActivity::ResponseReady,
                                        quality: ovrcr::protocol::SampleQuality::Observed,
                                        turn: Some("a".into()),
                                    }
                                ),
                            }
                        ),
                    })),
                    Response::Error { .. }
                ));
                assert_eq!(fixture.session_summary(summary.id).agent.unwrap(), current);
            }
        } else {
            assert!(current.is_none(), "non-prompt established binding");
        }
    }
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "CODEX_NATIVE_EXIT=17");
}

fn codex_session(
    fixture: &ControlFixture,
    socket: &Path,
) -> (ovrcr::session::SessionSummary, std::path::PathBuf) {
    fixture.create_hook_child("setup", "codex-setup");
    codex_session_named(fixture, socket, "codex-hooks")
}

fn codex_session_named(
    fixture: &ControlFixture,
    socket: &Path,
    name: &str,
) -> (ovrcr::session::SessionSummary, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let native = fixture._root.path().join("codex");
    std::fs::write(&native, "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'codex-cli 0.153.0\\n'; exit; fi\nexec \"$OVRCR_TEST_EXECUTABLE\" --ignored --exact codex_hook_native_helper --nocapture\n").unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let probe = fixture._root.path().join(format!("{name}-channel"));
    let summary = fixture.create_session_summary(name, vec![
        "/bin/sh".into(), "-c".into(),
        r#"stty -echo; printf '%s\n' "$OVRCR_HOOK_TOKEN" > "$4.capability"; export OVRCR_TEST_EXECUTABLE="$3" OVRCR_TEST_PROBE="$4" OVRCR_HOOK_SOCKET="$5"; "$1" agent run codex -- "$2"; printf 'CODEX_NATIVE_EXIT=%s\n' "$?"; IFS= read -r done"#.into(),
        "codex-fixture".into(), env!("CARGO_BIN_EXE_ovrcr").into(), native.into_os_string(), std::env::current_exe().unwrap().into_os_string(), probe.clone().into_os_string(), socket.as_os_str().into(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains_until(
        summary.id,
        "CODEX_NATIVE_READY",
        Instant::now() + Duration::from_secs(5),
    );
    let terminal = fixture.request(Request::ReadTerminal {
        session: summary.id,
        max_lines: None,
    });
    assert!(
        !matches!(&terminal, Response::TerminalText {text,..} if text.contains("reporting unavailable")),
        "unexpected initial admission gate: {terminal:?}"
    );
    (summary, probe)
}

#[test]
fn pi_managed_launch_preserves_argv_exit_and_environment_and_plain_launch_is_untracked() {
    let _guard = env_lock();
    use std::os::unix::fs::PermissionsExt;
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "pi-setup");
    let native = fixture._root.path().join("pi");
    std::fs::write(
        &native,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '0.85.1\\n'; exit; fi\nprintf 'PI_ARGS=%s\\n' \"$*\"\nprintf 'PI_HOOK=[%s%s%s]\\n' \"${OVRCR_HOOK_SOCKET:-}\" \"${OVRCR_HOOK_TOKEN:-}\" \"${OVRCR_SESSION_ID:-}\"\nprintf 'PI_CHANNEL=[%s]\\n' \"${OVRCR_AGENT_SOCKET:+set}\"\nexit 17\n",
    )
    .unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    // Managed: the picker's argv shape.
    let managed = fixture.create_session_summary(
        "pi-managed",
        vec![
            env!("CARGO_BIN_EXE_ovrcr").into(),
            "agent".into(),
            "run".into(),
            "pi".into(),
            "--".into(),
            native.clone().into_os_string(),
            "--model".into(),
            "x/y".into(),
            "hello world".into(),
        ],
    );
    fixture.record_process_group(&managed);
    fixture.wait_terminal_contains(managed.id, "PI_ARGS=--model x/y hello world");
    fixture.wait_terminal_contains(managed.id, "PI_HOOK=[]");
    fixture.wait_terminal_contains(managed.id, "PI_CHANNEL=[set]");
    // Reporting is not implemented yet in this ticket: the supervisor says so once and runs native.
    fixture.wait_terminal_contains(managed.id, "reporting unavailable");
    assert!(fixture.session_summary(managed.id).agent.is_none());
    // Ineligible mode: RPC mode runs native with reporting unavailable, no binding.
    let rpc = fixture.create_session_summary(
        "pi-rpc",
        vec![
            env!("CARGO_BIN_EXE_ovrcr").into(),
            "agent".into(),
            "run".into(),
            "pi".into(),
            "--".into(),
            native.clone().into_os_string(),
            "--mode".into(),
            "rpc".into(),
        ],
    );
    fixture.record_process_group(&rpc);
    fixture.wait_terminal_contains(rpc.id, "PI_ARGS=--mode rpc");
    fixture.wait_terminal_contains(rpc.id, "reporting unavailable");
    assert!(fixture.session_summary(rpc.id).agent.is_none());
    // Plain launch: no supervision, no channel, no binding.
    let plain =
        fixture.create_session_summary("pi-plain", vec![native.into_os_string(), "hi".into()]);
    fixture.record_process_group(&plain);
    fixture.wait_terminal_contains(plain.id, "PI_ARGS=hi");
    fixture.wait_terminal_contains(plain.id, "PI_CHANNEL=[]");
    assert!(fixture.session_summary(plain.id).agent.is_none());
}

#[test]
fn codex_managed_lost_bind_receipt_recovers_before_busy() {
    // Lifecycle/correlation gate; concurrent executable startup can exhaust the fixed probe budget.
    let _guard = env_lock();
    use ovrcr::protocol::{AgentActivity, ReporterHealth};
    let fixture = ControlFixture::new_bounded();
    let proxy = AdmissionProxy::new(
        fixture._root.path().join("codex-proxy.sock"),
        fixture.socket.clone(),
        AdmissionFault::LostBind,
    );
    let (summary, probe) = codex_session(&fixture, &proxy.path);
    for (index, command) in [
        "UserPromptSubmit:root:a",
        "Stop:root:a",
        "UserPromptSubmit:other:b",
        "Stop:other:b",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("CODEX_CALLBACK={index}"));
        let snapshot = fixture
            .session_summary(summary.id)
            .agent
            .unwrap_or_else(|| {
                panic!(
                    "no Codex snapshot after {index}: {:?}",
                    fixture.request(Request::ReadTerminal {
                        session: summary.id,
                        max_lines: None
                    })
                )
            });
        assert_eq!(
            snapshot.activity.unwrap().state,
            if index % 2 == 0 {
                AgentActivity::Busy
            } else {
                AgentActivity::ResponseReady
            }
        );
        assert_eq!(snapshot.binding.generation, if index < 2 { 1 } else { 2 });
    }
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "CODEX_NATIVE_EXIT=17");
    let channel = std::fs::read_to_string(probe).unwrap();
    let channel: Vec<_> = channel.lines().collect();
    assert!(
        UnixStream::connect(channel[0]).is_err(),
        "native exit retained endpoint"
    );
    wait_pid_absent(channel[2].parse().unwrap(), Duration::from_secs(2));
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if fixture
            .session_summary(summary.id)
            .agent
            .unwrap()
            .health
            .state
            != ReporterHealth::Connected
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "dead native reporter stayed connected"
        );
        thread::yield_now();
    }
}

#[test]
fn codex_managed_missing_end_disables_overlap_without_native_failure() {
    // Lifecycle/correlation gate; concurrent executable startup can exhaust the fixed probe budget.
    let _guard = env_lock();
    use ovrcr::protocol::{AgentActivity, ReporterHealth};
    let fixture = ControlFixture::new_bounded();
    let (summary, _) = codex_session(&fixture, &fixture.socket);
    for (index, command) in [
        "UserPromptSubmit:root:a",
        "UserPromptSubmit:root:b",
        "Stop:root:a",
        "Stop:root:b",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            fixture.request(Request::SendTerminal {
                session: summary.id,
                text: command.into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.wait_terminal_contains(summary.id, &format!("CODEX_CALLBACK={index}"));
        let snapshot = fixture
            .session_summary(summary.id)
            .agent
            .unwrap_or_else(|| panic!("no Codex snapshot after callback {index}: {command}"));
        assert_eq!(snapshot.activity.unwrap().state, AgentActivity::Busy);
        if index > 0 {
            let deadline = Instant::now() + Duration::from_secs(2);
            while fixture
                .session_summary(summary.id)
                .agent
                .unwrap()
                .health
                .state
                == ReporterHealth::Connected
            {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
        }
    }
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "CODEX_NATIVE_EXIT=17");
}

#[test]
fn codex_managed_native_death_never_synthesizes_ready_and_removes_socket() {
    // Lifecycle/correlation gate; concurrent executable startup can exhaust the fixed probe budget.
    let _guard = env_lock();
    use ovrcr::protocol::{AgentActivity, ReporterHealth};
    let fixture = ControlFixture::new_bounded();
    let (summary, probe) = codex_session(&fixture, &fixture.socket);
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "UserPromptSubmit:root:a".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "CODEX_CALLBACK=0");
    let channel = std::fs::read_to_string(probe).unwrap();
    let channel: Vec<_> = channel.lines().collect();
    let native = channel[2].parse::<libc::pid_t>().unwrap();
    assert_eq!(unsafe { libc::kill(native, libc::SIGKILL) }, 0);
    fixture.wait_terminal_contains(summary.id, "CODEX_NATIVE_EXIT=137");
    wait_pid_absent(native, Duration::from_secs(2));
    assert!(UnixStream::connect(channel[0]).is_err());
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let snapshot = fixture.session_summary(summary.id).agent.unwrap();
        assert_eq!(snapshot.activity.unwrap().state, AgentActivity::Busy);
        if snapshot.health.state != ReporterHealth::Connected {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}

/// Runs the shipped dashboard and substitutes only the final OS notification
/// executable. Provider callbacks, admission, broadcasts, visibility and input
/// all still cross their real process/PTY/socket boundaries.
struct DesktopAlertDashboard {
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Box<dyn Write + Send>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    process_group: libc::pid_t,
    host_process_groups: Vec<libc::pid_t>,
    received: std::sync::mpsc::Receiver<Vec<u8>>,
    reader: Option<thread::JoinHandle<()>>,
    parser: vt100::Parser,
    record: PathBuf,
}

impl DesktopAlertDashboard {
    fn start(fixture: &ControlFixture, settings: Option<&str>) -> Self {
        let directory = fixture._root.path().join("desktop-host");
        std::fs::create_dir_all(&directory).unwrap();
        let record = directory.join("calls");
        let player = directory.join(if cfg!(target_os = "macos") {
            "afplay"
        } else {
            "paplay"
        });
        std::fs::write(
            &player,
            "#!/bin/sh\nif [ -f \"$OVRCR_TEST_DESKTOP_RECORD.sound.mode\" ]; then\n  IFS= read -r mode < \"$OVRCR_TEST_DESKTOP_RECORD.sound.mode\"\n  case \"$mode\" in\n    fail) printf 'PRIVATE_SOUND_ERROR' >&2; exit 17 ;;\n    block) printf '%s\\n' \"$$\" > \"$OVRCR_TEST_DESKTOP_RECORD.sound.pid\"; exec /bin/sleep 30 ;;\n  esac\nfi\n{ printf 'BEGIN\\n'; printf '%s\\n' \"$@\"; printf 'END\\n'; } >> \"$OVRCR_TEST_DESKTOP_RECORD.sound\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = directory.join(if cfg!(target_os = "macos") {
            "osascript"
        } else {
            "notify-send"
        });
        std::fs::write(
            &executable,
            "#!/bin/sh\nif [ -f \"$OVRCR_TEST_DESKTOP_RECORD.mode\" ]; then\n  IFS= read -r mode < \"$OVRCR_TEST_DESKTOP_RECORD.mode\"\n  case \"$mode\" in\n    fail) printf 'PRIVATE_HOST_ERROR' >&2; exit 17 ;;\n    block) printf '%s\\n' \"$$\" > \"$OVRCR_TEST_DESKTOP_RECORD.pid\"; exec /bin/sleep 30 ;;\n  esac\nfi\n{ printf 'BEGIN\\n'; printf '%s\\n' \"$@\"; printf 'END\\n'; } >> \"$OVRCR_TEST_DESKTOP_RECORD\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let settings_path = fixture._root.path().join("dashboard.toml");
        if let Some(settings) = settings {
            std::fs::write(&settings_path, settings).unwrap();
        }
        let size = portable_pty::PtySize {
            rows: 32,
            cols: 180,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = portable_pty::native_pty_system().openpty(size).unwrap();
        let mut command = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_ovrcr"));
        command.env("OVRCR_SOCKET", &fixture.socket);
        command.env("OVRCR_CONFIG", fixture._root.path().join("config.toml"));
        command.env("OVRCR_DASHBOARD_CONFIG", settings_path);
        command.env("OVRCR_TEST_DESKTOP_RECORD", &record);
        // Never fall back to the user's real desktop tool if this fixture's
        // executable is deliberately removed for the unavailable-host test.
        command.env("PATH", directory);
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command).unwrap();
        let process_group = child.process_id().unwrap() as libc::pid_t;
        assert_eq!(unsafe { libc::getpgid(process_group) }, process_group);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (send, received) = std::sync::mpsc::channel();
        let reader = thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 || send.send(bytes[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        drop(pair.slave);
        let mut dashboard = Self {
            master: Some(pair.master),
            writer,
            child: Some(child),
            process_group,
            host_process_groups: Vec::new(),
            received,
            reader: Some(reader),
            parser: vt100::Parser::new(size.rows, size.cols, 0),
            record,
        };
        dashboard.wait_screen(|screen| screen.contains("codex-hooks"));
        dashboard
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn wait_screen(&mut self, predicate: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if predicate(&self.parser.screen().contents()) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "dashboard screen deadline: {}",
                self.parser.screen().contents()
            );
            if let Ok(bytes) = self.received.recv_timeout(Duration::from_millis(20)) {
                self.parser.process(&bytes);
            }
        }
    }

    fn select(&mut self, name: &str, terminal_marker: &str) {
        let screen = self.parser.screen().contents();
        let row = screen
            .lines()
            .position(|line| line.chars().take(32).collect::<String>().contains(name))
            .unwrap_or_else(|| panic!("session {name} missing from sidebar: {screen}"));
        self.send(format!("\x1b[<0;12;{}M\x1b[<0;12;{}m", row + 1, row + 1).as_bytes());
        self.wait_screen(|screen| screen.contains(terminal_marker));
    }

    fn resize(&mut self, cols: u16) {
        self.master
            .as_ref()
            .unwrap()
            .resize(portable_pty::PtySize {
                rows: 32,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        self.parser = vt100::Parser::new(32, cols, 0);
    }

    fn wait_record(&mut self, record: &std::path::Path, expected: usize, check: impl Fn(&str)) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let contents = std::fs::read_to_string(record).unwrap_or_default();
            let count = contents.lines().filter(|line| *line == "END").count();
            assert!(count <= expected, "unexpected host call: {contents}");
            if count == expected {
                for call in contents.split("END\n").filter(|call| !call.is_empty()) {
                    check(call);
                }
                return;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected} host calls, received {count}: {contents}; screen: {}",
                self.parser.screen().contents()
            );
            if let Ok(bytes) = self.received.recv_timeout(Duration::from_millis(10)) {
                self.parser.process(&bytes);
            }
        }
    }

    fn wait_calls(&mut self, expected: usize, session: SessionId) {
        let body = format!("fixture / work / codex-hooks (#{})", session.0);
        let record = self.record.clone();
        self.wait_record(&record, expected, |call| {
            assert!(call.contains(&body), "identity missing: {call}");
            assert!(
                call.contains("OVRCR · response ready"),
                "title missing: {call}"
            );
            assert!(!call.contains("CODEX_CALLBACK"));
            assert!(!call.contains("OVRCR_HOOK_TOKEN"));
            assert!(!call.contains("root.jsonl"));
        });
    }

    fn wait_sound_calls(&mut self, expected: usize) {
        let file = if cfg!(target_os = "macos") {
            "/System/Library/Sounds/Glass.aiff"
        } else {
            "/usr/share/sounds/freedesktop/stereo/complete.oga"
        };
        let record = self.record.with_extension("sound");
        self.wait_record(&record, expected, |call| {
            assert_eq!(
                call,
                format!("BEGIN\n{file}\n"),
                "the player receives only the fixed sound file"
            );
        });
    }

    fn detach(&mut self) {
        self.send(b"\x07q");
        let deadline = Instant::now() + Duration::from_secs(3);
        let child = self.child.as_mut().unwrap();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "dashboard detach failed: {status:?}");
                self.child.take();
                return;
            }
            assert!(Instant::now() < deadline, "dashboard did not detach");
            thread::park_timeout(Duration::from_millis(5));
        }
    }
}

impl Drop for DesktopAlertDashboard {
    fn drop(&mut self) {
        for &group in &self.host_process_groups {
            if group_exists(group) {
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
            if !wait_group_absent(group, Duration::from_secs(2)) {
                eprintln!("desktop host fixture process group {group} remains");
                assert!(
                    std::thread::panicking(),
                    "desktop host fixture cleanup failed"
                );
            }
        }
        if group_exists(self.process_group) {
            unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
        }
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::park_timeout(Duration::from_millis(5));
            }
        }
        self.writer = Box::new(std::io::sink());
        self.master.take();
        if let Some(reader) = self.reader.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !reader.is_finished() && Instant::now() < deadline {
                thread::park_timeout(Duration::from_millis(5));
            }
            if reader.is_finished() {
                let _ = reader.join();
            } else {
                eprintln!("desktop fixture reader did not terminate");
            }
        }
        if !wait_group_absent(self.process_group, Duration::from_secs(2)) {
            eprintln!(
                "desktop fixture process group {} remains",
                self.process_group
            );
            assert!(std::thread::panicking(), "desktop fixture cleanup failed");
        }
    }
}

fn desktop_codex_callback(
    fixture: &ControlFixture,
    session: SessionId,
    index: &mut usize,
    command: &str,
) {
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session,
            text: command.into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(session, &format!("CODEX_CALLBACK={index}"));
    *index += 1;
}

#[test]
fn codex_unread_real_cli_and_dashboard_review_preserve_native_reporting() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (summary, _) = codex_session(&fixture, &fixture.socket);
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let config = fixture._root.path().join("config.toml");
    let id = summary.id.0.to_string();
    let mut index = 0;
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    let first = fixture.session_summary(summary.id);
    let first_unread = first.unread.clone().expect("native Ready is unread");
    let expected_json = serde_json::to_string(&first_unread).unwrap();
    let listed = cli_with_output(
        bin,
        &config,
        &fixture.socket,
        &["terminal", "list", "--json"],
    );
    assert!(listed.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let listed = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == summary.id.0)
        .unwrap();
    assert_eq!(
        listed["unread"],
        serde_json::to_value(&first_unread).unwrap()
    );
    let text = cli_with_output(bin, &config, &fixture.socket, &["terminal", "list"]);
    assert!(text.status.success());
    assert!(
        String::from_utf8_lossy(&text.stdout)
            .lines()
            .any(|line| line.contains("codex-hooks") && line.ends_with("unread"))
    );

    let mut dashboard = DesktopAlertDashboard::start(&fixture, None);
    dashboard.select("codex-hooks", "CODEX_CALLBACK=1");
    dashboard.wait_screen(|screen| screen.contains("Unread"));
    assert_eq!(
        fixture.session_summary(summary.id),
        first,
        "opening the response does not acknowledge it"
    );
    dashboard.send(b"\x07R");
    dashboard.wait_screen(|screen| !screen.contains("Unread") && screen.contains("response ready"));
    let mut reviewed = first.clone();
    reviewed.unread = None;
    assert_eq!(
        fixture.session_summary(summary.id),
        reviewed,
        "actual dashboard input changes unread only"
    );
    desktop_codex_callback(&fixture, summary.id, &mut index, "Stop:root:a");
    assert_eq!(
        fixture.session_summary(summary.id),
        reviewed,
        "native duplicate does not reopen reviewed response"
    );
    for command in ["UserPromptSubmit:root:b", "Stop:root:b"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    let second = fixture.session_summary(summary.id);
    let stale = cli_with_output(
        bin,
        &config,
        &fixture.socket,
        &[
            "terminal",
            "mark-reviewed",
            &id,
            "--expected",
            &expected_json,
            "--json",
        ],
    );
    assert!(!stale.status.success(), "stale CLI action must fail");
    assert!(String::from_utf8_lossy(&stale.stderr).contains("Ready observation changed"));
    assert_eq!(fixture.session_summary(summary.id), second);
    let expected_json = serde_json::to_string(second.unread.as_ref().unwrap()).unwrap();
    for _ in 0..2 {
        let ack = cli_with_output(
            bin,
            &config,
            &fixture.socket,
            &[
                "terminal",
                "mark-reviewed",
                &id,
                "--expected",
                &expected_json,
                "--json",
            ],
        );
        assert!(
            ack.status.success(),
            "{}",
            String::from_utf8_lossy(&ack.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&ack.stdout).unwrap(),
            serde_json::json!({"ok": true})
        );
    }
    let mut expected = second;
    expected.unread = None;
    assert_eq!(
        fixture.session_summary(summary.id),
        expected,
        "actual CLI requests change unread only"
    );
    for command in [
        "UserPromptSubmit:root:c",
        "Stop:root:c",
        "UserPromptSubmit:root:d",
    ] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    let busy = fixture.session_summary(summary.id);
    assert_eq!(busy.activity, ovrcr::protocol::AgentActivity::Busy);
    assert_eq!(busy.unread.as_ref().unwrap().turn.as_deref(), Some("c"));
    dashboard.detach();
    drop(dashboard);
    assert_eq!(fixture.session_summary(summary.id), busy);
    let mut dashboard = DesktopAlertDashboard::start(&fixture, None);
    dashboard.select("codex-hooks", "CODEX_CALLBACK=7");
    dashboard.wait_screen(|screen| screen.contains("Unread") && screen.contains("busy observed"));
    assert_eq!(
        fixture.session_summary(summary.id),
        busy,
        "reattaching retains unread while server lives"
    );
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: summary.id,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "CODEX_NATIVE_EXIT=17");
    dashboard.resize(80);
    dashboard.wait_screen(|screen| screen.contains("Unread Unavailable"));
    let lost = fixture.session_summary(summary.id);
    assert_eq!(lost.unread, busy.unread);
    assert_eq!(
        lost.agent.as_ref().unwrap().health.state,
        ovrcr::protocol::ReporterHealth::Unavailable
    );
    dashboard.send(b"\x07R");
    dashboard.wait_screen(|screen| {
        !screen.contains("Unread") && screen.contains("CODEX_NATIVE_EXIT=17")
    });
    dashboard.resize(180);
    dashboard.wait_screen(|screen| screen.contains("unavailable"));
    let mut expected = lost;
    expected.unread = None;
    assert_eq!(
        fixture.session_summary(summary.id),
        expected,
        "review cannot revive a lost reporter or complete native work"
    );
    dashboard.detach();
}

#[test]
fn codex_unread_is_discarded_on_terminal_removal_and_server_restart() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (first, _) = codex_session(&fixture, &fixture.socket);
    let mut index = 0;
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, first.id, &mut index, command);
    }
    assert!(fixture.session_summary(first.id).unread.is_some());
    assert_eq!(
        fixture.request(Request::CloseTerminal { session: first.id }),
        Response::Ok
    );
    let Response::Inventory { sessions, .. } = fixture.request(Request::Inspect) else {
        panic!("expected inventory");
    };
    assert!(
        sessions
            .iter()
            .all(|s| s.id != first.id && s.unread.is_none()),
        "removing a terminal removes its unread result"
    );

    let (second, _) = codex_session_named(&fixture, &fixture.socket, "codex-restart");
    let mut index = 0;
    for command in ["UserPromptSubmit:root:b", "Stop:root:b"] {
        desktop_codex_callback(&fixture, second.id, &mut index, command);
    }
    assert!(fixture.session_summary(second.id).unread.is_some());
    assert_eq!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    fixture
        .thread
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join()
        .unwrap();
    let socket = fixture.socket.clone();
    let registry = fixture._root.path().join("config.toml");
    *fixture.thread.lock().unwrap() = Some(thread::spawn(move || {
        run_server(ServerPaths { socket }, registry).unwrap();
    }));
    fixture.wait_socket();
    let Response::Inventory { sessions, registry } = fixture.request(Request::Inspect) else {
        panic!("expected inventory");
    };
    assert!(
        !registry.projects.is_empty(),
        "restart uses the same saved configuration"
    );
    assert!(
        sessions.is_empty(),
        "new server cannot resurrect live terminals or unread results from saved configuration"
    );
}

#[test]
fn desktop_notifications_managed_completion_reaches_host_once_and_respects_visibility() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (summary, _) = codex_session(&fixture, &fixture.socket);
    let mut dashboard =
        DesktopAlertDashboard::start(&fixture, Some("desktop_notifications = true\n"));
    dashboard.select("setup", "HOOK_READY");
    let mut index = 0;
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(1, summary.id);

    // A subsequent real completion is the delivery barrier for duplicate,
    // stale, child and interrupted events; all preceding host calls are counted.
    for command in [
        "Stop:root:a",
        "UserPromptSubmit:root:b",
        "Stop:root:a",
        "Stop:root:b:child",
        "Interrupt:root:b",
        "Stop:root:b",
        "UserPromptSubmit:root:c",
        "Stop:root:c",
    ] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(2, summary.id);

    dashboard.select("codex-hooks", "CODEX_CALLBACK=9");
    for command in ["UserPromptSubmit:root:d", "Stop:root:d"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_screen(|screen| screen.contains("CODEX_CALLBACK=11"));
    // Reassign the focused pane to the fixture's other real managed session.
    dashboard.select("setup", "HOOK_READY");
    for command in ["UserPromptSubmit:root:e", "Stop:root:e"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(3, summary.id);

    dashboard.send(b"v");
    dashboard.wait_screen(|screen| screen.contains("CODEX_CALLBACK=13"));
    dashboard.send(b"\t");
    for command in ["UserPromptSubmit:root:f", "Stop:root:f"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_screen(|screen| screen.contains("CODEX_CALLBACK=15"));
    dashboard.resize(55);
    dashboard.wait_screen(|screen| screen.contains("split hidden"));
    assert!(
        !dashboard
            .parser
            .screen()
            .contents()
            .contains("CODEX_CALLBACK=15")
    );
    for command in ["UserPromptSubmit:root:g", "Stop:root:g"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(4, summary.id);

    dashboard.resize(180);
    dashboard.wait_screen(|screen| screen.contains("CODEX_CALLBACK=17"));
    dashboard.detach();
    for command in ["UserPromptSubmit:root:h", "Stop:root:h"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    drop(dashboard);
    let mut dashboard =
        DesktopAlertDashboard::start(&fixture, Some("desktop_notifications = true\n"));
    dashboard.select("setup", "HOOK_READY");
    for command in ["UserPromptSubmit:root:i", "Stop:root:i"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(5, summary.id);
    dashboard.detach();
}

#[test]
fn desktop_notifications_default_off_and_disabled_events_do_not_replay_on_enable() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (summary, _) = codex_session(&fixture, &fixture.socket);
    let mut dashboard = DesktopAlertDashboard::start(&fixture, None);
    dashboard.select("setup", "HOOK_READY");
    let mut index = 0;
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    // Showing the actual completed output proves this dashboard consumed the
    // completion before enabling, without a timing-only negative assertion.
    dashboard.select("codex-hooks", "CODEX_CALLBACK=1");
    dashboard.select("setup", "HOOK_READY");
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: on"));
    for command in ["UserPromptSubmit:root:b", "Stop:root:b"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(1, summary.id);
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: off"));
    for command in ["UserPromptSubmit:root:c", "Stop:root:c"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.select("codex-hooks", "CODEX_CALLBACK=5");
    dashboard.select("setup", "HOOK_READY");
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: on"));
    for command in ["UserPromptSubmit:root:d", "Stop:root:d"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(2, summary.id);
    dashboard.detach();
}

#[test]
fn desktop_notifications_host_failure_and_blocking_never_block_input_or_detach() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (summary, _) = codex_session(&fixture, &fixture.socket);
    let mut dashboard =
        DesktopAlertDashboard::start(&fixture, Some("desktop_notifications = true\n"));
    let local = fixture.only_session_id();
    assert_eq!(fixture.session_summary(local).name, "local");
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: local,
            text: "printf 'DESKTOP_%s\\n' SHELL_READY".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(local, "DESKTOP_SHELL_READY");
    dashboard.select("local", "DESKTOP_SHELL_READY");
    let mut index = 0;
    let mode = dashboard.record.with_extension("mode");
    let host_pid = dashboard.record.with_extension("pid");
    std::fs::write(&mode, "fail\n").unwrap();
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications unavailable"));
    assert!(
        !dashboard
            .parser
            .screen()
            .contents()
            .contains("PRIVATE_HOST_ERROR")
    );
    assert_eq!(
        fixture
            .session_summary(summary.id)
            .agent
            .unwrap()
            .activity
            .unwrap()
            .state,
        ovrcr::protocol::AgentActivity::ResponseReady
    );
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: off"));
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: on"));

    std::fs::write(&mode, "block\n").unwrap();
    for command in ["UserPromptSubmit:root:b", "Stop:root:b"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    let wait_host_pid = || {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(text) = std::fs::read_to_string(&host_pid)
                && let Ok(pid) = text.trim().parse::<libc::pid_t>()
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "host did not enter blocking command"
            );
            thread::park_timeout(Duration::from_millis(5));
        }
    };
    let blocked_pid = wait_host_pid();
    dashboard.host_process_groups.push(blocked_pid);
    assert_eq!(unsafe { libc::getpgid(blocked_pid) }, blocked_pid);
    // Observe input and rendering while the OS host command is still running.
    dashboard.send(b"\rprintf 'DESKTOP_INPUT_%s\\n' READY\r");
    dashboard.wait_screen(|screen| screen.contains("DESKTOP_INPUT_READY"));
    assert!(
        group_exists(blocked_pid),
        "host exited before the input check"
    );
    dashboard.send(b"\x07");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications unavailable"));
    assert!(wait_group_absent(blocked_pid, Duration::from_secs(2)));

    std::fs::remove_file(&host_pid).unwrap();
    for command in ["UserPromptSubmit:root:c", "Stop:root:c"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    let blocked_pid = wait_host_pid();
    dashboard.host_process_groups.push(blocked_pid);
    assert_eq!(unsafe { libc::getpgid(blocked_pid) }, blocked_pid);
    dashboard.detach();
    assert!(wait_group_absent(blocked_pid, Duration::from_secs(2)));
    assert_eq!(fixture.session_phase(summary.id), SessionPhase::Running);
}

#[test]
fn desktop_notifications_queued_completion_is_cancelled_when_its_pane_becomes_visible() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (first, _) = codex_session(&fixture, &fixture.socket);
    let (queued, _) = codex_session_named(&fixture, &fixture.socket, "codex-queued");
    let mut dashboard =
        DesktopAlertDashboard::start(&fixture, Some("desktop_notifications = true\n"));
    dashboard.select("setup", "HOOK_READY");
    let mode = dashboard.record.with_extension("mode");
    let host_pid = dashboard.record.with_extension("pid");
    std::fs::write(&mode, "block\n").unwrap();
    let mut first_index = 0;
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, first.id, &mut first_index, command);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let blocked_pid = loop {
        if let Ok(text) = std::fs::read_to_string(&host_pid)
            && let Ok(pid) = text.trim().parse::<libc::pid_t>()
        {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "first host did not enter blocker"
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    dashboard.host_process_groups.push(blocked_pid);
    assert_eq!(unsafe { libc::getpgid(blocked_pid) }, blocked_pid);
    // Only the first host invocation blocks. The second completion must queue
    // behind it before we make that session visible through real mouse input.
    std::fs::remove_file(mode).unwrap();
    let mut queued_index = 0;
    for command in ["UserPromptSubmit:root:b", "Stop:root:b"] {
        desktop_codex_callback(&fixture, queued.id, &mut queued_index, command);
    }
    dashboard.wait_screen(|screen| screen.contains("✓ ● codex-queued"));
    assert!(
        group_exists(blocked_pid),
        "first host no longer blocks the queue"
    );
    dashboard.select("codex-queued", "CODEX_CALLBACK=1");
    // Seeing this response consumes its candidate permanently, even if the
    // pane becomes hidden again before the busy host finishes.
    dashboard.select("setup", "HOOK_READY");
    assert!(
        group_exists(blocked_pid),
        "queue released before the visible-to-hidden transition"
    );
    assert_eq!(unsafe { libc::kill(-blocked_pid, libc::SIGTERM) }, 0);
    assert!(wait_group_absent(blocked_pid, Duration::from_secs(2)));

    // A later eligible completion on the other session is the FIFO delivery
    // barrier. Any obsolete queued invocation has a different identity and
    // fails the host payload assertion, even if it precedes this fresh one.
    for command in ["UserPromptSubmit:root:c", "Stop:root:c"] {
        desktop_codex_callback(&fixture, first.id, &mut first_index, command);
    }
    dashboard.wait_calls(1, first.id);
    dashboard.detach();
}

#[test]
fn desktop_notifications_missing_host_tool_preserves_response_and_dashboard_controls() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (summary, _) = codex_session(&fixture, &fixture.socket);
    let mut dashboard =
        DesktopAlertDashboard::start(&fixture, Some("desktop_notifications = true\n"));
    dashboard.select("setup", "HOOK_READY");
    std::fs::remove_file(
        dashboard
            .record
            .parent()
            .unwrap()
            .join(if cfg!(target_os = "macos") {
                "osascript"
            } else {
                "notify-send"
            }),
    )
    .unwrap();
    let mut index = 0;
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications unavailable"));
    assert!(
        !dashboard.record.exists(),
        "missing host cannot record delivery"
    );
    assert_eq!(
        fixture
            .session_summary(summary.id)
            .agent
            .unwrap()
            .activity
            .unwrap()
            .state,
        ovrcr::protocol::AgentActivity::ResponseReady
    );
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: off"));
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: on"));
    dashboard.detach();
}

#[test]
fn ready_sound_is_independent_of_desktop_notifications_on_the_managed_path() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    let (summary, _) = codex_session(&fixture, &fixture.socket);
    let sound_only = Some("ready_sound = true\n");
    let mut dashboard = DesktopAlertDashboard::start(&fixture, sound_only);
    dashboard.select("setup", "HOOK_READY");
    let mut index = 0;
    for command in ["UserPromptSubmit:root:a", "Stop:root:a"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_sound_calls(1);
    // The next accepted completion is the barrier for duplicate, child and
    // interrupted events; every preceding player call is counted.
    for command in [
        "Stop:root:a",
        "UserPromptSubmit:root:b",
        "Stop:root:b:child",
        "Interrupt:root:b",
        "UserPromptSubmit:root:c",
        "Stop:root:c",
    ] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_sound_calls(2);
    assert!(
        !dashboard.record.exists(),
        "sound only never calls the desktop host"
    );

    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: on"));
    for command in ["UserPromptSubmit:root:d", "Stop:root:d"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(1, summary.id);
    dashboard.wait_sound_calls(3);

    dashboard.send(b"S");
    dashboard.wait_screen(|screen| screen.contains("Ready sound: off"));
    for command in ["UserPromptSubmit:root:e", "Stop:root:e"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_calls(2, summary.id);
    dashboard.wait_sound_calls(3);

    // Neither channel: showing the completed output proves this dashboard
    // consumed the completion before sound is enabled again.
    dashboard.send(b"N");
    dashboard.wait_screen(|screen| screen.contains("Desktop notifications: off"));
    for command in ["UserPromptSubmit:root:f", "Stop:root:f"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.select("codex-hooks", "CODEX_CALLBACK=13");
    dashboard.select("setup", "HOOK_READY");
    dashboard.send(b"S");
    dashboard.wait_screen(|screen| screen.contains("Ready sound: on"));

    // A visible response stays silent; the next hidden one is the barrier.
    dashboard.select("codex-hooks", "CODEX_CALLBACK=13");
    for command in ["UserPromptSubmit:root:g", "Stop:root:g"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_screen(|screen| screen.contains("CODEX_CALLBACK=15"));
    dashboard.select("setup", "HOOK_READY");
    for command in ["UserPromptSubmit:root:h", "Stop:root:h"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_sound_calls(4);
    dashboard.wait_calls(2, summary.id);

    // A failing player is reported without its output and preserves Ready.
    let mode = dashboard.record.with_extension("sound.mode");
    std::fs::write(&mode, "fail\n").unwrap();
    for command in ["UserPromptSubmit:root:i", "Stop:root:i"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_screen(|screen| screen.contains("Ready sound unavailable"));
    assert!(
        !dashboard
            .parser
            .screen()
            .contents()
            .contains("PRIVATE_SOUND_ERROR")
    );
    assert_eq!(
        fixture
            .session_summary(summary.id)
            .agent
            .unwrap()
            .activity
            .unwrap()
            .state,
        ovrcr::protocol::AgentActivity::ResponseReady
    );
    std::fs::remove_file(&mode).unwrap();

    // Completions while disconnected never replay after reconnect.
    dashboard.detach();
    for command in ["UserPromptSubmit:root:j", "Stop:root:j"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    drop(dashboard);
    let mut dashboard = DesktopAlertDashboard::start(&fixture, sound_only);
    dashboard.select("setup", "HOOK_READY");
    for command in ["UserPromptSubmit:root:k", "Stop:root:k"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    dashboard.wait_sound_calls(5);
    dashboard.wait_calls(2, summary.id);

    // A hung player never blocks terminal input or detach; detach ends its
    // process group without waiting for the playback deadline.
    let local = fixture.only_session_id();
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: local,
            text: "printf 'SOUND_%s\\n' SHELL_READY".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_terminal_contains(local, "SOUND_SHELL_READY");
    dashboard.select("local", "SOUND_SHELL_READY");
    std::fs::write(&mode, "block\n").unwrap();
    for command in ["UserPromptSubmit:root:l", "Stop:root:l"] {
        desktop_codex_callback(&fixture, summary.id, &mut index, command);
    }
    let player_pid = dashboard.record.with_extension("sound.pid");
    let deadline = Instant::now() + Duration::from_secs(3);
    let blocked_pid = loop {
        if let Ok(text) = std::fs::read_to_string(&player_pid)
            && let Ok(pid) = text.trim().parse::<libc::pid_t>()
        {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "player did not enter blocking mode"
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    dashboard.host_process_groups.push(blocked_pid);
    assert_eq!(unsafe { libc::getpgid(blocked_pid) }, blocked_pid);
    dashboard.send(b"\rprintf 'SOUND_INPUT_%s\\n' READY\r");
    dashboard.wait_screen(|screen| screen.contains("SOUND_INPUT_READY"));
    assert!(
        group_exists(blocked_pid),
        "player exited before the input check"
    );
    dashboard.send(b"\x07");
    dashboard.detach();
    assert!(wait_group_absent(blocked_pid, Duration::from_secs(2)));
    assert_eq!(fixture.session_phase(summary.id), SessionPhase::Running);
}
