#[path = "support/live.rs"]
mod live;

use ovrcr::protocol::{
    OperatorProvider, OperatorRequest, OperatorResponse, Request, Response, client, connect_server,
};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn compiled_terminal_list_exposes_idle_operator_scope_and_identity() {
    let live = live::Live::binary();
    let mut dashboard = connect_server(&live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    dashboard
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    let Response::Operator(OperatorResponse::Created(conversation)) = client::request(
        &mut dashboard,
        2,
        Request::Operator(OperatorRequest::Create {
            provider: OperatorProvider::ChatGpt,
            model: "offline-fixture".into(),
        }),
    )
    .unwrap() else {
        panic!("idle operator conversation was not created");
    };
    assert!(conversation.current_turn.is_none());

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
    let rows: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        row["scope"],
        json!({"OperatorConversation": {"conversation": conversation.id.0}})
    );
    assert_eq!(row["id"], conversation.session.0);
    assert_eq!(row["run"], 0);
    assert_eq!(row["project"], "");
    assert_eq!(row["workspace"], "");
    assert!(row["pid"].is_null());
    assert_eq!(row["phase"], "stopped");
    drop(dashboard);
}
