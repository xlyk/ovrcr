//! Drive two managed agents through their private reporting sockets and real PTYs.
#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT};
use ovrcr::protocol::{AgentActivity, Request, Response, SessionId, SessionSummary, TerminalSize};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Instant;

const FIRST: &str = "629ef478-dcdd-43c3-9716-b9036fb3ace4";
const SECOND: &str = "609e6b77-ee3c-428d-8215-76166159ade5";

fn create(fixture: &Live, native: &Path, args: &[&str]) -> SessionId {
    let result = fixture
        .command()
        .args([
            "terminal",
            "create",
            "--json",
            "--project",
            PROJECT,
            "--workspace",
            "feature/agy",
            "--",
        ])
        .arg(&fixture.executable)
        .args(["agent", "run", "agy", "--"])
        .arg(native)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    SessionId(
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()["id"]
            .as_u64()
            .unwrap(),
    )
}

fn wait(fixture: &Live, id: SessionId, check: impl Fn(&SessionSummary) -> bool) -> SessionSummary {
    let end = Instant::now() + live::wait_deadline();
    loop {
        let Response::Inventory { sessions, .. } = fixture.request(Request::Inspect) else {
            panic!("inventory unavailable")
        };
        let row = sessions.into_iter().find(|s| s.id == id).unwrap();
        if check(&row) {
            return row;
        }
        assert!(
            Instant::now() < end,
            "session {id:?} did not reach expected state: {row:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn two_managed_sessions_retain_and_reopen_only_their_foreground_conversation() {
    let fixture = Live::idle().bounded();
    let db_dir = fixture
        .root
        .path()
        .join(".gemini/antigravity-cli/conversations");
    std::fs::create_dir_all(&db_dir).unwrap();
    for uuid in [FIRST, SECOND] {
        let db = rusqlite::Connection::open(db_dir.join(format!("{uuid}.db"))).unwrap();
        db.execute("CREATE TABLE trajectory_meta(cascade_id TEXT)", [])
            .unwrap();
        db.execute(
            "INSERT INTO trajectory_meta(cascade_id) VALUES (?1)",
            [uuid],
        )
        .unwrap();
    }
    let native = fixture.root.path().join("agy");
    let script = format!(
        r#"#!/bin/sh
id={FIRST}
title=One
while [ "$#" -gt 0 ]; do
    case "$1" in
        --conversation)
            id=$2
            printf '%s\n' "$id" >> "$HOME/agy-reopened"
            shift ;;
        --model) [ "$2" != two ] || id={SECOND}; shift ;;
    esac
    shift
done
[ "$id" != {SECOND} ] || title=Two
printf '%s\n' '{{"conversation_id":"'"$id"'","session_id":"'"$id"'","conversation_title":"'"$title"'","agent_state":"working","model":{{"id":"fixture","display_name":"Fixture Model"}}}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json
printf 'BOUND %s\n' "$id"
read -r ignored
"#
    );
    std::fs::write(&native, script).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    fixture.start_binary_env(&[("OVRCR_TEST_AGY_CLI", fixture.executable.as_os_str())]);
    fixture.ready("feature/agy");
    let a = create(&fixture, &native, &[]);
    let b = create(&fixture, &native, &["--model", "two"]);
    let bound = |row: &SessionSummary, uuid: &str, title: &str| {
        row.activity == AgentActivity::Busy
            && row
                .recovery
                .as_ref()
                .is_some_and(|r| r.attached && r.conversation.as_deref() == Some(uuid))
            && row
                .agent
                .as_ref()
                .and_then(|a| a.metrics.as_ref())
                .and_then(|m| m.sample.model.as_deref())
                == Some("Fixture Model")
            && row.display_name() == title
            && row.unread.is_none()
    };
    let first = wait(&fixture, a, |s| bound(s, FIRST, "One"));
    let peer = wait(&fixture, b, |s| bound(s, SECOND, "Two"));
    let response = fixture.request(Request::SetSessionTitle {
        session: b,
        title: Some("My pinned session".into()),
    });
    assert!(
        !matches!(response, Response::Error { .. }),
        "pin failed: {response:?}"
    );
    fixture.request(Request::KillSession { session: a });
    wait(&fixture, a, |s| !s.phase.is_live());
    let result = fixture.request(Request::ReopenSession {
        session: a,
        expected_run: first.run,
        acknowledge_stopped: true,
    });
    assert!(
        !matches!(result, Response::Error { .. }),
        "reopen: {result:?}"
    );
    assert_ne!(wait(&fixture, a, |s| bound(s, FIRST, "One")).run, first.run);
    assert_eq!(
        wait(&fixture, b, |s| bound(s, SECOND, "My pinned session")).run,
        peer.run
    );
    let replacement = wait(&fixture, a, |s| s.phase.is_live());
    fixture.request(Request::KillSession { session: a });
    wait(&fixture, a, |s| !s.phase.is_live());
    std::fs::remove_file(db_dir.join(format!("{FIRST}.db"))).unwrap();
    let refused = fixture.request(Request::ReopenSession {
        session: a,
        expected_run: replacement.run,
        acknowledge_stopped: true,
    });
    assert!(
        matches!(refused, Response::Error { .. }),
        "missing history reopened: {refused:?}"
    );
    let surviving = wait(&fixture, b, |s| bound(s, SECOND, "My pinned session"));
    assert_eq!(surviving.run, peer.run);
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("agy-reopened")).unwrap(),
        format!("{FIRST}\n")
    );
}

#[test]
fn a_lost_statusline_delivery_does_not_replace_the_last_observed_state() {
    let fixture = Live::idle().bounded();
    let db_dir = fixture
        .root
        .path()
        .join(".gemini/antigravity-cli/conversations");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db = rusqlite::Connection::open(db_dir.join(format!("{FIRST}.db"))).unwrap();
    db.execute("CREATE TABLE trajectory_meta(cascade_id TEXT)", [])
        .unwrap();
    db.execute(
        "INSERT INTO trajectory_meta(cascade_id) VALUES (?1)",
        [FIRST],
    )
    .unwrap();

    // The managed Agy adapter admits only the native `agy` executable name.
    let native = fixture.root.path().join("agy");
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' '{{"conversation_id":"{FIRST}","conversation_title":"Retained","model":{{"display_name":"Fixture Model"}},"agent_state":"working"}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat
IFS= read -r event
if [ "$event" = lose ]; then
    printf '%s\n' '{{"conversation_id":"{FIRST}","agent_state":"idle"}}' | OVRCR_AGENT_SOCKET="$HOME/missing-invocation-socket" "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat
    printf '%s\n' '{{"conversationId":"{FIRST}","executionNum":1,"terminationReason":"NO_TOOL_CALL","fullyIdle":true,"continuationActive":false,"error":null}}' | OVRCR_AGENT_SOCKET="$HOME/missing-invocation-socket" "$OVRCR_TEST_AGY_CLI" report agy --stdin > "$HOME/stop-result.json"
    touch "$HOME/lost-delivery-finished"
fi
read -r ignored
"#
    );
    std::fs::write(&native, script).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    fixture.start_binary_env(&[("OVRCR_TEST_AGY_CLI", fixture.executable.as_os_str())]);
    fixture.ready("feature/agy");
    let session = create(&fixture, &native, &[]);
    let bound = wait(&fixture, session, |row| {
        row.activity == AgentActivity::Busy
            && row
                .recovery
                .as_ref()
                .is_some_and(|r| r.attached && r.conversation.as_deref() == Some(FIRST))
    });
    assert_eq!(
        bound
            .agent
            .as_ref()
            .and_then(|agent| agent.metrics.as_ref())
            .and_then(|metrics| metrics.sample.model.as_deref()),
        Some("Fixture Model")
    );

    let mut dashboard = ovrcr::protocol::connect_server(&fixture.socket).unwrap();
    assert!(matches!(
        ovrcr::protocol::client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    let selected = ovrcr::protocol::client::request(
        &mut dashboard,
        2,
        Request::Select {
            session,
            size: TerminalSize { rows: 24, cols: 80 },
        },
    )
    .unwrap();
    assert!(
        !matches!(selected, Response::Error { .. }),
        "select failed: {selected:?}"
    );

    let input = ovrcr::protocol::client::request(
        &mut dashboard,
        3,
        Request::Input {
            session,
            run: bound.run,
            bytes: b"lose\n".to_vec(),
        },
    )
    .unwrap();
    assert!(
        !matches!(input, Response::Error { .. }),
        "input failed: {input:?}"
    );
    let marker = fixture.root.path().join("lost-delivery-finished");
    let end = Instant::now() + live::wait_deadline();
    while !marker.is_file() {
        assert!(Instant::now() < end, "lost callback helpers did not finish");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read(fixture.root.path().join("stop-result.json")).unwrap(),
        b"{\"decision\":\"allow\"}\n"
    );

    let after_loss = wait(&fixture, session, |row| {
        row.phase.is_live() && row.activity == AgentActivity::Busy
    });
    assert_eq!(after_loss.run, bound.run);
    assert_eq!(
        after_loss
            .agent
            .as_ref()
            .and_then(|agent| agent.metrics.as_ref())
            .and_then(|metrics| metrics.sample.model.as_deref()),
        Some("Fixture Model"),
        "a callback sent to a dead private channel cannot erase the retained row"
    );
}

#[test]
fn unmanaged_callbacks_preserve_native_rendering_and_stop_decisions() {
    use std::io::Write;
    use std::process::Stdio;

    let fixture = Live::idle();
    let payload = br#"{"lastUserInput":"PRIVATE PROMPT","email":"private@example.invalid","unrelated":[1,2]}"#;
    for (args, expected) in [
        (
            vec!["report", "agy", "--stdin"],
            b"{\"decision\":\"allow\"}".as_slice(),
        ),
        (
            vec![
                "report",
                "agy-statusline",
                "--stdin-json",
                "--render-command",
                "cat",
            ],
            payload.as_slice(),
        ),
    ] {
        let mut child = fixture
            .command()
            .env_remove("OVRCR_AGENT_SOCKET")
            .env_remove("OVRCR_AGENT_TOKEN")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stderr.is_empty());
        assert_eq!(
            output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout),
            expected
        );
    }
    assert!(!fixture.socket.exists());
}
