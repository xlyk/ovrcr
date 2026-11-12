//! Operator scope on the compiled Server: the singleton's ownership, its
//! refusal of terminal lifecycle routes, preserved legacy rows, and terminal
//! control that stays independent of it.

#[path = "support/fake_pi.rs"]
mod fake_pi;
#[path = "support/live.rs"]
mod live;
#[path = "support/operator_harness.rs"]
mod operator_harness;

use fake_pi::{FakePi, Scenario, Step};
use live::Live;
use operator_harness::{OperatorClient, Registry, has_assistant};
use ovrcr::protocol::{
    CreateSessionRequest, ErrorCode, OperatorRequest, OperatorTurnState, Request, Response,
    SessionKind, SessionScope, SessionSummary, client, connect_server,
};
use serde_json::{Value, json};
use std::time::Duration;

fn fixture() -> (Live, FakePi, OperatorClient) {
    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::assistant("scoped answer"), Step::Settle]);
    fake.script(&scenario);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let client = OperatorClient::connect(&live);
    (live, fake, client)
}

fn terminal_rows(live: &Live) -> Vec<Value> {
    let output = live
        .command()
        .args(["--json", "terminal", "list"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn create_waiter(live: &Live, name: &str) -> SessionSummary {
    match live.request(Request::CreateSession(CreateSessionRequest {
        project: live::PROJECT.into(),
        workspace: live::WORKSPACE.into(),
        name: name.into(),
        label: None,
        argv: vec!["/bin/sh".into(), "-c".into(), "IFS= read -r value".into()],
        kind: SessionKind::Terminal,
    })) {
        Response::CreatedSession(session) => *session,
        other => panic!("create {name} failed: {other:?}"),
    }
}

fn track_workspace_groups(live: &Live) {
    for group in live.session_groups() {
        live.own_group(group);
    }
}

fn inspect_sessions(live: &Live) -> Vec<SessionSummary> {
    match live.request(Request::Inspect) {
        Response::Inventory { sessions, .. } => sessions,
        other => panic!("inspect did not return inventory: {other:?}"),
    }
}

#[test]
fn compiled_terminal_list_exposes_the_idle_operator_scope_and_identity() {
    let (live, _fake, mut client) = fixture();
    let opened = client.open();
    assert!(opened.current_turn.is_none());

    let rows = terminal_rows(&live);
    assert_eq!(rows.len(), 1, "exactly one operator row: {rows:?}");
    let row = &rows[0];
    assert_eq!(
        row["scope"],
        json!({"OperatorConversation": {"conversation": opened.id.0}})
    );
    assert_eq!(row["id"], opened.session.0);
    assert_eq!(row["run"], 0);
    assert_eq!(row["project"], "");
    assert_eq!(row["workspace"], "");
    assert!(row["pid"].is_null());
    assert_eq!(row["phase"], "stopped");
}

#[test]
fn operator_requests_require_the_current_dashboard() {
    let live = Live::idle().bounded();
    live.start_binary();
    for request in [
        Request::Operator(OperatorRequest::Current),
        Request::Operator(OperatorRequest::Open),
    ] {
        let mut control = connect_server(&live.socket).unwrap();
        control
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let response = client::request(&mut control, 1, request).unwrap();
        assert!(
            matches!(
                response,
                Response::Error {
                    code: ErrorCode::InvalidRequest,
                    ..
                }
            ),
            "operator requests need Dashboard ownership: {response:?}"
        );
    }
    assert!(
        Registry::of(&live).conversation_ids().is_empty(),
        "a control connection cannot create the singleton"
    );

    let mut dashboard = OperatorClient::connect(&live);
    let opened = dashboard.open();
    // A second dashboard cannot share the ownership slot.
    let mut second = connect_server(&live.socket).unwrap();
    second
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let response = client::request(&mut second, 1, Request::DashboardHello).unwrap();
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ),
        "{response:?}"
    );
    assert_eq!(dashboard.current(), Some(opened));
}

#[test]
fn legacy_operator_rows_refuse_lifecycle_but_keep_exact_run_acknowledgement() {
    let (live, _fake, mut client) = fixture();
    let opened = client.open();
    let turned = client.submit_turn(&opened, "legacy carrier");
    let history = client.wait_terminal(opened.id);
    client.wait_stopped(opened.id);

    // The pre-singleton shape is a row without the marker.
    operator_harness::sqlite_exec(&live, "UPDATE operator_conversations SET singleton=0");
    let registry = Registry::of(&live);
    assert!(client.current().is_none(), "legacy rows are not Current");
    assert!(matches!(
        client.exchange(Request::Operator(OperatorRequest::Read {
            conversation: opened.id,
            after: 0,
            limit: 100,
        })),
        Response::Error { .. }
    ));
    assert!(matches!(
        client.submit(&history.conversation, "revive"),
        Response::Error { .. }
    ));

    for request in [
        Request::UnarchiveSession {
            session: opened.session,
            expected_run: turned.identity.run,
        },
        Request::DeleteArchivedSession {
            session: opened.session,
            expected_run: turned.identity.run,
        },
        Request::CloseTerminal {
            session: opened.session,
            expected_run: turned.identity.run,
        },
        Request::ReopenSession {
            session: opened.session,
            expected_run: turned.identity.run,
            acknowledge_stopped: false,
        },
        Request::RemoveSession {
            session: opened.session,
        },
    ] {
        let response = live.request(request.clone());
        assert!(
            matches!(
                response,
                Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                }
            ),
            "{request:?} must be refused for a legacy Operator row, got {response:?}"
        );
    }
    assert!(
        registry.conversation_ids().contains(&opened.id.0),
        "refused routes preserve the legacy row"
    );

    // Exact-run acknowledgement is the independent recovery path and does not
    // require singleton membership.
    assert_eq!(
        live.request(Request::AcknowledgeSessionStopped {
            session: opened.session,
            expected_run: turned.identity.run,
        }),
        Response::Ok
    );
    assert_eq!(
        operator_harness::query_value(
            &live,
            &format!(
                "SELECT stopped FROM retained_sessions WHERE id={}",
                opened.session.0
            )
        ),
        Some(1)
    );

    // The next Open creates a fresh singleton, leaving the legacy row untouched.
    let fresh = client.open();
    assert_ne!(fresh.id, opened.id);
    assert_eq!(fresh.title, "Operator");
    let mut ids = Registry::of(&live).conversation_ids();
    ids.sort_unstable();
    let mut expected = vec![opened.id.0, fresh.id.0];
    expected.sort_unstable();
    assert_eq!(ids, expected);
}

#[test]
fn ordinary_terminal_control_stays_independent_of_the_operator_singleton() {
    let (live, _fake, mut client) = fixture();
    live.ready("scope");
    let opened = client.open();

    let rows = terminal_rows(&live);
    assert_all_seen(&rows);
    let operator_row = rows
        .iter()
        .find(|row| row["id"] == opened.session.0)
        .expect("operator row");
    assert_eq!(
        operator_row["scope"],
        json!({"OperatorConversation": {"conversation": opened.id.0}})
    );
    let terminal = rows
        .iter()
        .find(|row| {
            !matches!(
                &row["scope"],
                Value::Object(map) if map.contains_key("OperatorConversation")
            )
        })
        .expect("ordinary terminal row")
        .clone();
    assert!(
        !terminal["pid"].is_null(),
        "fixture shell is live: {terminal:?}"
    );

    let session = terminal["id"].as_u64().unwrap();
    let run = terminal["run"].as_u64().unwrap();
    assert_eq!(
        live.request(Request::CloseTerminal {
            session: ovrcr::protocol::SessionId(session),
            expected_run: ovrcr::protocol::SessionRunId(run),
        }),
        Response::Ok
    );
    assert_eq!(
        client.current(),
        Some(opened.clone()),
        "terminal control never changes the Operator singleton"
    );
    assert_eq!(client.session(opened.session).id, opened.session);
    assert_eq!(
        client.session(opened.session).scope,
        SessionScope::OperatorConversation {
            conversation: opened.id
        }
    );
}

#[test]
fn generic_terminal_controls_refuse_operator_scope() {
    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::Delta("held operator turn".into()), Step::Hold]);
    fake.script(&scenario);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let mut client = OperatorClient::connect(&live);
    let opened = client.open();
    for request in [
        Request::SendTerminal {
            session: opened.session,
            text: "must not be pasted".into(),
            submit: false,
        },
        Request::Keystroke {
            session: opened.session,
            expected_run: opened.run,
            key: ":enter:".into(),
        },
        Request::PauseSession {
            session: opened.session,
        },
        Request::ResumeSession {
            session: opened.session,
        },
    ] {
        assert!(
            matches!(
                live.request(request.clone()),
                Response::Error {
                    code: ErrorCode::Conflict,
                    message,
                } if message.contains("Operator")
            ),
            "idle Operator scope must be explicitly refused by {request:?}"
        );
    }
    let turn = client.submit_turn(&opened, "keep the worker live");
    let history = client.wait_history(opened.id, |history| {
        has_assistant(history, "held operator turn")
    });
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Running
    );

    let responses = [
        Request::SendTerminal {
            session: opened.session,
            text: "must not be pasted".into(),
            submit: false,
        },
        Request::Keystroke {
            session: opened.session,
            expected_run: turn.identity.run,
            key: ":enter:".into(),
        },
        Request::PauseSession {
            session: opened.session,
        },
        Request::ResumeSession {
            session: opened.session,
        },
    ];
    for request in responses {
        assert!(
            matches!(
                live.request(request.clone()),
                Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                }
            ),
            "Server route must refuse {request:?}"
        );
    }

    let id = opened.session.0.to_string();
    for args in [
        vec![
            "terminal",
            "send",
            id.as_str(),
            "--text",
            "must not be pasted",
        ],
        vec!["terminal", "keystroke", id.as_str(), ":enter:"],
        vec!["pause", id.as_str()],
        vec!["resume", id.as_str()],
    ] {
        let output = live.command().args(&args).output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success(), "CLI route accepted {args:?}");
        assert!(text.contains("Operator"), "unexpected refusal: {text}");
    }

    client.stop_turn(turn.identity);
    client.wait_stopped(opened.id);
}

#[test]
fn fifty_workspace_sessions_coexist_with_the_operator_dedicated_slot() {
    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    let mut held = Scenario::new();
    held.steps(vec![Step::Delta("first turn".into()), Step::Hold]);
    fake.script(&held);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    live.ready("feature/operator-capacity");
    for index in 0..48 {
        create_waiter(&live, &format!("waiter-{index:02}"));
    }
    track_workspace_groups(&live);

    let mut client = OperatorClient::connect(&live);
    let opened = client.open();
    let first = client.submit_turn(&opened, "hold the dedicated worker");
    client.wait_history(opened.id, |history| has_assistant(history, "first turn"));

    // The 50th Workspace session must be admitted while the Operator worker is live.
    let fiftieth = create_waiter(&live, "fiftieth");
    track_workspace_groups(&live);
    assert!(fiftieth.phase.is_live());

    let live_sessions = inspect_sessions(&live);
    let workspaces = live_sessions
        .iter()
        .filter(|session| session.scope == SessionScope::Workspace && session.phase.is_live())
        .count();
    assert_eq!(workspaces, 50);
    assert_eq!(
        live_sessions
            .iter()
            .filter(|session| {
                matches!(session.scope, SessionScope::OperatorConversation { .. })
                    && session.phase.is_live()
            })
            .count(),
        1,
        "the Operator worker remains live beside all fifty Workspace sessions"
    );
    let overflow = live.request(Request::CreateSession(CreateSessionRequest {
        project: live::PROJECT.into(),
        workspace: live::WORKSPACE.into(),
        name: "fifty-first".into(),
        label: None,
        argv: vec!["/bin/sh".into()],
        kind: SessionKind::Terminal,
    }));
    assert!(
        matches!(
            overflow,
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ),
        "51st Workspace session must remain refused: {overflow:?}"
    );

    client.stop_turn(first.identity);
    client.wait_stopped(opened.id);
    let mut second = Scenario::new();
    second.steps(vec![
        Step::assistant("turn at full Workspace capacity"),
        Step::Settle,
    ]);
    fake.script(&second);
    let current = client
        .current()
        .expect("Operator conversation remains current");
    client.submit_turn(&current, "start while all fifty slots are occupied");
    let history = client.wait_history(opened.id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state != OperatorTurnState::Running)
    });
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Complete,
        "Operator turn must start with all fifty Workspace slots occupied: {history:#?}"
    );
    client.wait_stopped(opened.id);

    let sessions = inspect_sessions(&live);
    assert_eq!(
        sessions
            .iter()
            .filter(|session| session.scope == SessionScope::Workspace && session.phase.is_live())
            .count(),
        50
    );
}

fn assert_all_seen(rows: &[Value]) {
    assert_eq!(rows.len(), 2, "operator and terminal rows: {rows:?}");
    assert!(rows.iter().all(|row| row.get("id").is_some()));
}
