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
use operator_harness::{OperatorClient, Registry};
use ovrcr::protocol::{
    ErrorCode, OperatorRequest, Request, Response, SessionScope, client, connect_server,
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

fn assert_all_seen(rows: &[Value]) {
    assert_eq!(rows.len(), 2, "operator and terminal rows: {rows:?}");
    assert!(rows.iter().all(|row| row.get("id").is_some()));
}
