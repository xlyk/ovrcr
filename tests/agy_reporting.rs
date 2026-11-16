//! Drive two managed agents through their private reporting sockets and real PTYs.
#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT};
use ovrcr::protocol::{
    AgentActivity, AgentProvider, QuotaSnapshot, QuotaSource, QuotaState, Request, Response,
    ServerEvent, ServerMessage, SessionId, SessionSummary, TerminalSize, read_frame,
};
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
fn successful_stop_then_idle_creates_unread_through_the_managed_server_path() {
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

    let native = fixture.root.path().join("agy");
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' '{{"conversation_id":"{FIRST}","agent_state":"working"}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
IFS= read -r event
if [ "$event" = finish ]; then
    printf '%s\n' '{{"conversationId":"{FIRST}","executionNum":0,"terminationReason":"NO_TOOL_CALL","fullyIdle":true,"continuationActive":false,"error":null}}' | "$OVRCR_TEST_AGY_CLI" report agy --stdin > "$HOME/stop-result.json"
    printf '%s\n' '{{"conversation_id":"{FIRST}","agent_state":"idle"}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
    touch "$HOME/agy-ready-finished"
fi
read -r ignored
"#
    );
    std::fs::write(&native, script).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    fixture.start_binary_env(&[("OVRCR_TEST_AGY_CLI", fixture.executable.as_os_str())]);
    fixture.ready("feature/agy");
    let session = create(&fixture, &native, &[]);
    let busy = wait(&fixture, session, |row| {
        row.activity == AgentActivity::Busy
            && row.recovery.as_ref().is_some_and(|recovery| {
                recovery.attached && recovery.conversation.as_deref() == Some(FIRST)
            })
    });

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
            run: busy.run,
            bytes: b"finish\n".to_vec(),
        },
    )
    .unwrap();
    assert!(
        !matches!(input, Response::Error { .. }),
        "input failed: {input:?}"
    );

    let marker = fixture.root.path().join("agy-ready-finished");
    let end = Instant::now() + live::wait_deadline();
    while !marker.is_file() {
        assert!(
            Instant::now() < end,
            "Agy Stop and idle callbacks did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read(fixture.root.path().join("stop-result.json")).unwrap(),
        b"{\"decision\":\"allow\"}\n"
    );

    let ready = wait(&fixture, session, |row| {
        row.activity == AgentActivity::ResponseReady && row.unread.is_some()
    });
    let unread = ready
        .unread
        .expect("successful Agy response becomes unread");
    assert_eq!(unread.binding.provider, AgentProvider::Agy);
    assert_eq!(unread.binding.conversation, FIRST);
    assert_eq!(unread.turn.as_deref(), Some("agy-cycle-1"));
    assert!(unread.activity_revision > 0);
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

fn quota_until(
    dashboard: &mut std::os::unix::net::UnixStream,
    why: &str,
    check: impl Fn(&QuotaSnapshot) -> bool,
) -> QuotaSnapshot {
    dashboard
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let end = Instant::now() + live::wait_deadline();
    loop {
        assert!(Instant::now() < end, "{why}");
        if let Ok(ServerMessage::Event(ServerEvent::QuotaChanged(snap))) =
            read_frame::<ServerMessage>(dashboard)
            && check(&snap)
        {
            return *snap;
        }
    }
}

#[test]
fn statusline_allowance_supplements_held_account_isolates_malformed_quota_respects_consent_and_hands_off_to_usage()
 {
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

    let release_usage = fixture.root.path().join("release-usage");
    let isolated_home = fixture.root.path().join("isolated-agy-home");
    std::fs::create_dir_all(&isolated_home).unwrap();
    let collector = fixture.root.path().join("agy-collector");
    let collector_script = format!(
        r#"#!/bin/sh
if [ "$*" = '-p /usage --output-format json --settings {{}}' ]; then
    while [ ! -f '{}' ]; do
        sleep 0.05
    done
    printf '%s\n' '{{"status":"SUCCESS","command":{{"name":"usage","data":{{"email":"account@example.invalid","description":"ACCOUNT_SECRET_PLAN","groups":[{{"name":"Gemini","buckets":[{{"id":"gemini-weekly","remaining_fraction":0.82,"reset_time":"2099-01-02T00:00:00Z","window":"weekly"}},{{"id":"gemini-5h","remaining_fraction":0.64,"reset_time":"2099-01-01T00:00:00Z","window":"5h"}}]}},{{"name":"Claude + GPT","buckets":[{{"id":"3p-weekly","remaining_fraction":0.9,"reset_time":"2099-01-02T00:00:00Z","window":"weekly"}},{{"id":"3p-5h","remaining_fraction":0.75,"reset_time":"2099-01-01T00:00:00Z","window":"5h"}}]}}]}}}}}}'
    exit 0
fi
exit 2
"#,
        release_usage.display()
    );
    std::fs::write(&collector, collector_script).unwrap();
    std::fs::set_permissions(&collector, std::fs::Permissions::from_mode(0o700)).unwrap();

    let settings_doc = fixture.config.join("dashboard.toml");
    std::fs::create_dir_all(&fixture.config).unwrap();
    std::fs::write(
        &settings_doc,
        format!(
            "[quota]\nenabled = true\n\
             [quota.codex]\ncommand = '/no-codex-fixture'\n\
             [quota.grok]\ncommand = '/no-grok-fixture'\n\
             [quota.agy]\ncommand = '{}'\n",
            collector.display()
        ),
    )
    .unwrap();

    let native = fixture.root.path().join("agy");
    let native_script = format!(
        r#"#!/bin/sh
printf '%s\n' '{{"conversation_id":"{FIRST}","conversation_title":"Supplement Topic","account_email":"statusline@example.invalid","last_user_input":"SECRET_PROMPT_484","model":{{"display_name":"Gemini 2.5 Pro"}},"agent_state":"working","quota":{{"gemini-weekly":{{"remaining_fraction":0.7,"reset_time":"2099-01-02T00:00:00Z","description":"SECRET_PLAN_DESC"}},"3p-weekly":{{"remainingFraction":0.85,"resetTime":"2099-01-02T00:00:00Z","token":"SECRET_TOKEN_484"}},"gemini-5h":{{"remaining_fraction":0.55,"reset_time":"2099-01-01T00:00:00Z"}},"3p-5h":{{"remaining_fraction":0.4,"reset_time":"2099-01-01T00:00:00Z"}},"disabled-bucket":{{"remaining_fraction":0.1,"disabled":true}}}}}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
while IFS= read -r event; do
    if [ "$event" = malformed_finish ]; then
        printf '%s\n' '{{"conversationId":"{FIRST}","executionNum":0,"terminationReason":"NO_TOOL_CALL","fullyIdle":true,"continuationActive":false,"error":null}}' | "$OVRCR_TEST_AGY_CLI" report agy --stdin >/dev/null
        printf '%s\n' '{{"conversation_id":"{FIRST}","conversation_title":"Updated Subject After Malformed","model":{{"display_name":"Gemini 2.5 Flash"}},"agent_state":"idle","quota":{{"gemini-5h":{{"remaining_fraction":1.5,"secret":"MALFORMED_SECRET_484"}}}}}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
        touch "$HOME/malformed-finish-done"
    fi
done
"#
    );
    std::fs::write(&native, native_script).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();

    fixture.start_binary_env(&[("OVRCR_TEST_AGY_CLI", fixture.executable.as_os_str())]);
    fixture.ready("feature/agy");

    let mut dashboard = ovrcr::protocol::connect_server(&fixture.socket).unwrap();
    assert!(matches!(
        ovrcr::protocol::client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));

    let session = create(&fixture, &native, &[]);
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

    // 1. While /usage collector is held in Checking, status-line supplement populates Agy with
    // Session provenance and checked_unix_ms == None.
    let supplemented = quota_until(
        &mut dashboard,
        "status-line supplement did not populate Agy while /usage was held",
        |q| {
            q.agy.state == QuotaState::Current
                && matches!(q.agy.source, Some(QuotaSource::Session { session: id, .. }) if id == session)
        },
    );
    assert!(supplemented.agy.observed_unix_ms.is_some());
    assert_eq!(supplemented.agy.checked_unix_ms, None);
    let win_summary: Vec<(&str, &str, bool)> = supplemented
        .agy
        .windows
        .iter()
        .map(|w| (w.id.as_str(), w.label.as_str(), w.general))
        .collect();
    assert_eq!(
        win_summary,
        vec![
            ("gemini-5h", "5h", true),
            ("gemini-weekly", "7d", true),
            ("3p-5h", "5h", false),
            ("3p-weekly", "7d", false),
        ]
    );

    let bound = wait(&fixture, session, |row| {
        row.activity == AgentActivity::Busy && row.display_name() == "Supplement Topic"
    });

    // 2. Setting quota.agy.home suppresses ambient session supplement; resetting restores it.
    assert_eq!(
        fixture.request(Request::SetSetting {
            path: "quota.agy.home".into(),
            value: Some(format!("'{}'", isolated_home.display())),
        }),
        Response::Ok
    );
    let home_suppressed = quota_until(
        &mut dashboard,
        "explicit quota.agy.home did not suppress ambient session supplement",
        |q| q.agy.state == QuotaState::Checking && q.agy.windows.is_empty(),
    );
    assert!(
        !matches!(
            home_suppressed.agy.source,
            Some(QuotaSource::Session { .. })
        ),
        "{:?}",
        home_suppressed.agy.source
    );

    assert_eq!(
        fixture.request(Request::SetSetting {
            path: "quota.agy.home".into(),
            value: None,
        }),
        Response::Ok
    );
    let home_restored = quota_until(
        &mut dashboard,
        "clearing quota.agy.home did not restore ambient session supplement",
        |q| {
            q.agy.state == QuotaState::Current
                && matches!(q.agy.source, Some(QuotaSource::Session { .. }))
        },
    );
    assert_eq!(home_restored.agy.checked_unix_ms, None);

    // 3. Setting quota.enabled = false disables Agy; re-enabling restores supplement while /usage is held.
    assert_eq!(
        fixture.request(Request::SetSetting {
            path: "quota.enabled".into(),
            value: Some("false".into()),
        }),
        Response::Ok
    );
    let disabled = quota_until(
        &mut dashboard,
        "quota.enabled = false did not disable Agy",
        |q| q.agy.state == QuotaState::Disabled,
    );
    assert!(disabled.agy.windows.is_empty());

    assert_eq!(
        fixture.request(Request::SetSetting {
            path: "quota.enabled".into(),
            value: Some("true".into()),
        }),
        Response::Ok
    );
    let reenabled = quota_until(
        &mut dashboard,
        "re-enabling quota.enabled did not restore ambient session supplement",
        |q| {
            q.agy.state == QuotaState::Current
                && matches!(q.agy.source, Some(QuotaSource::Session { .. }))
        },
    );
    assert_eq!(reenabled.agy.checked_unix_ms, None);

    // 4. Malformed status-line quota isolates Invalid to Agy WITHOUT disrupting title, model, or Ready/Unread!
    ovrcr::protocol::write_frame(
        &mut dashboard,
        &ovrcr::protocol::ClientMessage {
            request_id: 3,
            request: Request::Input {
                session,
                run: bound.run,
                bytes: b"malformed_finish\n".to_vec(),
            },
        },
    )
    .unwrap();
    let invalid_quota = quota_until(
        &mut dashboard,
        "malformed status-line quota did not report Invalid for Agy",
        |q| q.agy.state == QuotaState::Invalid,
    );
    assert_eq!(invalid_quota.agy.checked_unix_ms, None);
    assert!(matches!(
        invalid_quota.agy.source,
        Some(QuotaSource::Session { .. })
    ));

    let ready = wait(&fixture, session, |row| {
        row.activity == AgentActivity::ResponseReady
            && row.unread.is_some()
            && row.display_name() == "Updated Subject After Malformed"
    });
    assert_eq!(
        ready
            .agent
            .as_ref()
            .and_then(|a| a.metrics.as_ref())
            .and_then(|m| m.sample.model.as_deref()),
        Some("Gemini 2.5 Flash")
    );
    assert_eq!(
        ready.unread.as_ref().and_then(|u| u.turn.as_deref()),
        Some("agy-cycle-1")
    );

    // 5. Releasing the held /usage collector hands off cleanly to authoritative NativeProfile windows.
    std::fs::write(&release_usage, b"ok\n").unwrap();
    let authoritative = quota_until(
        &mut dashboard,
        "released /usage collector did not hand off to authoritative NativeProfile windows",
        |q| {
            q.agy.state == QuotaState::Current
                && matches!(q.agy.source, Some(QuotaSource::NativeProfile { .. }))
                && q.agy.checked_unix_ms.is_some()
        },
    );
    assert_eq!(
        authoritative.agy.windows[0].remaining_basis_points(),
        Some(6_400)
    );
    assert_eq!(
        authoritative.agy.windows[1].remaining_basis_points(),
        Some(8_200)
    );
    assert_eq!(
        authoritative.agy.windows[2].remaining_basis_points(),
        Some(7_500)
    );

    // 6. Confirm zero secret/email/prompt leakage across QuotaSnapshot and Server Events.
    let events_out = fixture
        .command()
        .args(["events", "--json"])
        .output()
        .unwrap();
    assert!(events_out.status.success());
    let combined = format!(
        "{:?}\n{:?}\n{}",
        supplemented,
        authoritative,
        String::from_utf8_lossy(&events_out.stdout)
    );
    for forbidden in [
        "statusline@example.invalid",
        "account@example.invalid",
        "SECRET_PROMPT_484",
        "SECRET_PLAN_DESC",
        "SECRET_TOKEN_484",
        "MALFORMED_SECRET_484",
        "ACCOUNT_SECRET_PLAN",
        "@",
    ] {
        assert!(
            !combined.contains(forbidden),
            "leaked forbidden value {forbidden:?}"
        );
    }
}

#[test]
fn focused_invalid_session_yields_to_unfocused_valid_windows_and_never_clobbers_settled_account_failure()
 {
    let fixture = Live::idle().bounded();
    let db_dir = fixture
        .root
        .path()
        .join(".gemini/antigravity-cli/conversations");
    std::fs::create_dir_all(&db_dir).unwrap();
    for id in [FIRST, SECOND] {
        let db = rusqlite::Connection::open(db_dir.join(format!("{id}.db"))).unwrap();
        db.execute("CREATE TABLE trajectory_meta(cascade_id TEXT)", [])
            .unwrap();
        db.execute("INSERT INTO trajectory_meta(cascade_id) VALUES (?1)", [id])
            .unwrap();
    }

    let release_login_failure = fixture.root.path().join("release-login-failure");
    let collector = fixture.root.path().join("agy-collector-fail");
    let collector_script = format!(
        r#"#!/bin/sh
if [ "$*" = '-p /usage --output-format json --settings {{}}' ]; then
    while [ ! -f '{}' ]; do
        sleep 0.05
    done
    printf 'Authentication required. Please log in.\n' >&2
    exit 1
fi
exit 2
"#,
        release_login_failure.display()
    );
    std::fs::write(&collector, collector_script).unwrap();
    std::fs::set_permissions(&collector, std::fs::Permissions::from_mode(0o700)).unwrap();

    let settings_doc = fixture.config.join("dashboard.toml");
    std::fs::create_dir_all(&fixture.config).unwrap();
    std::fs::write(
        &settings_doc,
        format!(
            "[quota]\nenabled = true\n\
             [quota.codex]\ncommand = '/no-codex-fixture'\n\
             [quota.grok]\ncommand = '/no-grok-fixture'\n\
             [quota.agy]\ncommand = '{}'\n",
            collector.display()
        ),
    )
    .unwrap();

    let native = fixture.root.path().join("agy");
    let native_script = format!(
        r#"#!/bin/sh
id="{FIRST}"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --conversation) id="$2"; shift ;;
        --model) [ "$2" != two ] || id="{SECOND}"; shift ;;
    esac
    shift
done
if [ "$id" = "{FIRST}" ]; then
    printf '%s\n' '{{"conversation_id":"{FIRST}","conversation_title":"Unfocused Valid","agent_state":"working","quota":{{"gemini-5h":{{"remaining_fraction":0.62,"reset_time":"2099-01-01T00:00:00Z"}}}}}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
else
    printf '%s\n' '{{"conversation_id":"{SECOND}","conversation_title":"Focused Invalid","agent_state":"working","quota":{{"gemini-5h":{{"remaining_fraction":1.5}}}}}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
fi
while IFS= read -r event; do
    case "$event" in
        go_invalid)
            printf '%s\n' '{{"conversation_id":"'"$id"'","conversation_title":"Now Invalid","agent_state":"idle","quota":{{"gemini-5h":{{"remaining_fraction":1.5}}}}}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
            ;;
        go_valid)
            printf '%s\n' '{{"conversation_id":"'"$id"'","conversation_title":"Recovered Valid","agent_state":"working","quota":{{"gemini-5h":{{"remaining_fraction":0.48,"reset_time":"2099-01-01T00:00:00Z"}}}}}}' | "$OVRCR_TEST_AGY_CLI" report agy-statusline --stdin-json --render-command cat >/dev/null
            ;;
    esac
done
"#
    );
    std::fs::write(&native, native_script).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();

    fixture.start_binary_env(&[("OVRCR_TEST_AGY_CLI", fixture.executable.as_os_str())]);
    fixture.ready("feature/agy");

    let mut dashboard = ovrcr::protocol::connect_server(&fixture.socket).unwrap();
    assert!(matches!(
        ovrcr::protocol::client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));

    let unfocused_valid = create(&fixture, &native, &[]);
    let first_snap = quota_until(
        &mut dashboard,
        "unfocused valid session did not populate Agy",
        |q| {
            q.agy.state == QuotaState::Current
                && matches!(q.agy.source, Some(QuotaSource::Session { session: id, .. }) if id == unfocused_valid)
        },
    );
    assert_eq!(
        first_snap.agy.windows[0].remaining_basis_points(),
        Some(6_200)
    );

    let focused_invalid = create(&fixture, &native, &["--conversation", SECOND]);
    let focused_summary = wait(&fixture, focused_invalid, |row| {
        row.display_name() == "Focused Invalid"
    });
    let selected = ovrcr::protocol::client::request(
        &mut dashboard,
        2,
        Request::Select {
            session: focused_invalid,
            size: TerminalSize { rows: 24, cols: 80 },
        },
    )
    .unwrap();
    assert!(
        !matches!(selected, Response::Error { .. }),
        "select focused_invalid failed: {selected:?}"
    );

    // Trigger settings re-evaluation / refresh check via Inspect + current Hierarchy/Quota state:
    // Focusing the windowless Invalid session must NOT displace the unfocused session's valid windows!
    std::thread::sleep(std::time::Duration::from_millis(150));
    // Verify by sending another invalid update from the focused session and checking Inspector/QuotaChanged:
    ovrcr::protocol::write_frame(
        &mut dashboard,
        &ovrcr::protocol::ClientMessage {
            request_id: 3,
            request: Request::Input {
                session: focused_invalid,
                run: focused_summary.run,
                bytes: b"go_invalid\n".to_vec(),
            },
        },
    )
    .unwrap();
    wait(&fixture, focused_invalid, |row| {
        row.display_name() == "Now Invalid"
    });

    // Toggle quota.enabled false -> true on dashboard to read the freshly recomputed Agy row!
    assert_eq!(
        fixture.request(Request::SetSetting {
            path: "quota.enabled".into(),
            value: Some("false".into()),
        }),
        Response::Ok
    );
    quota_until(&mut dashboard, "disable Agy", |q| {
        q.agy.state == QuotaState::Disabled
    });
    assert_eq!(
        fixture.request(Request::SetSetting {
            path: "quota.enabled".into(),
            value: Some("true".into()),
        }),
        Response::Ok
    );
    let after_focus = quota_until(
        &mut dashboard,
        "focused Invalid session clobbered unfocused valid Agy session windows",
        |q| q.agy.state != QuotaState::Disabled,
    );
    assert_eq!(
        after_focus.agy.state,
        QuotaState::Current,
        "focused Invalid session overrode unfocused session with valid windows: {after_focus:?}"
    );
    assert!(matches!(
        after_focus.agy.source,
        Some(QuotaSource::Session { session: id, .. }) if id == unfocused_valid
    ));
    assert_eq!(
        after_focus.agy.windows[0].remaining_basis_points(),
        Some(6_200)
    );

    // Now stop unfocused_valid so only the windowless Invalid session remains while /usage is Checking:
    // With no valid windows anywhere, focused_invalid surfaces Invalid with empty windows.
    fixture.request(Request::KillSession {
        session: unfocused_valid,
    });
    wait(&fixture, unfocused_valid, |row| !row.phase.is_live());
    let only_invalid = quota_until(
        &mut dashboard,
        "windowless Invalid session while /usage Checking did not surface Invalid",
        |q| q.agy.state == QuotaState::Invalid,
    );
    assert!(only_invalid.agy.windows.is_empty());

    // Release /usage collector so account settles into NotSignedIn ("not signed in") without windows:
    // Windowless Invalid session MUST yield to the concrete account failure state and reason!
    std::fs::write(&release_login_failure, b"ok\n").unwrap();
    let settled_failure = quota_until(
        &mut dashboard,
        "settled NotSignedIn account failure was clobbered by windowless Invalid session",
        |q| q.agy.state == QuotaState::NotSignedIn,
    );
    assert_eq!(settled_failure.agy.reason.as_deref(), Some("not signed in"));
    assert!(matches!(
        settled_failure.agy.source,
        Some(QuotaSource::NativeProfile { .. })
    ));

    // Once focused_invalid reports valid non-empty windows, it DOES supplement the failed account!
    ovrcr::protocol::write_frame(
        &mut dashboard,
        &ovrcr::protocol::ClientMessage {
            request_id: 6,
            request: Request::Input {
                session: focused_invalid,
                run: focused_summary.run,
                bytes: b"go_valid\n".to_vec(),
            },
        },
    )
    .unwrap();
    let supplemented_after_failure = quota_until(
        &mut dashboard,
        "valid session windows did not supplement failed windowless account",
        |q| q.agy.state == QuotaState::Current,
    );
    assert_eq!(
        supplemented_after_failure.agy.windows[0].remaining_basis_points(),
        Some(4_800)
    );
    assert_eq!(supplemented_after_failure.agy.checked_unix_ms, None);
    assert!(supplemented_after_failure.agy.next_check_unix_ms.is_some());
}

#[test]
fn setup_over_empty_disabled_statusline_enables_conversation_capture_and_reopen() {
    let fixture = Live::idle().bounded();
    let antigravity_dir = fixture.root.path().join(".gemini/antigravity-cli");
    let db_dir = antigravity_dir.join("conversations");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db = rusqlite::Connection::open(db_dir.join(format!("{FIRST}.db"))).unwrap();
    db.execute("CREATE TABLE trajectory_meta(cascade_id TEXT)", [])
        .unwrap();
    db.execute(
        "INSERT INTO trajectory_meta(cascade_id) VALUES (?1)",
        [FIRST],
    )
    .unwrap();
    drop(db);

    // Seed the exact unconfigured/disabled statusLine shape Antigravity /config writes.
    let settings_path = antigravity_dir.join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{"statusLine":{"command":"","enabled":false,"type":""},"verbosity":"low"}"#,
    )
    .unwrap();

    let setup = fixture
        .command()
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
        .args(["agent", "setup", "agy", "--print"])
        .output()
        .unwrap();
    assert!(
        setup.status.success(),
        "{}",
        String::from_utf8_lossy(&setup.stderr)
    );
    let payload: serde_json::Value = serde_json::from_slice(&setup.stdout).unwrap();
    std::fs::write(
        &settings_path,
        serde_json::to_vec_pretty(&payload["settings"]["document"]).unwrap(),
    )
    .unwrap();
    let hooks_path = fixture.root.path().join(".gemini/config/hooks.json");
    std::fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
    std::fs::write(
        &hooks_path,
        serde_json::to_vec_pretty(&payload["hooks"]["document"]).unwrap(),
    )
    .unwrap();

    // Stand-in `agy` that honors `settings.json` statusLine.{type,enabled,command} like 1.5.9.
    let native = fixture.root.path().join("agy");
    let script = format!(
        r#"#!/bin/sh
id={FIRST}
while [ "$#" -gt 0 ]; do
    case "$1" in
        --conversation)
            id=$2
            printf '%s\n' "$id" >> "$HOME/agy-reopened"
            shift ;;
    esac
    shift
done
cmd=$(python3 -c '
import json, os, pathlib
d = json.loads((pathlib.Path(os.environ["HOME"]) / ".gemini/antigravity-cli/settings.json").read_text())
sl = d.get("statusLine") or {{}}
if sl.get("type") == "command" and sl.get("enabled", True) is not False and sl.get("command"):
    print(sl["command"])
')
if [ -n "$cmd" ]; then
    printf '%s\n' '{{"conversation_id":"'"$id"'","session_id":"'"$id"'","conversation_title":"Resumable Session","agent_state":"idle","model":{{"id":"fixture","display_name":"Fixture Model"}}}}' | /bin/sh -c "$cmd" >/dev/null
fi
"#
    );
    std::fs::write(&native, script).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();

    fixture.start_binary();
    fixture.ready("feature/agy");
    let session = create(&fixture, &native, &[]);
    let exited = wait(&fixture, session, |s| !s.phase.is_live());
    let recovery = exited.recovery.as_ref().expect("recovery summary");
    assert_eq!(recovery.unavailable, None, "{recovery:?}");
    assert_eq!(recovery.conversation.as_deref(), Some(FIRST));

    let reopen = fixture.request(Request::ReopenSession {
        session,
        expected_run: exited.run,
        acknowledge_stopped: true,
    });
    assert!(
        !matches!(reopen, Response::Error { .. }),
        "reopen failed: {reopen:?}"
    );
    wait(&fixture, session, |s| {
        s.run != exited.run
            && std::fs::read_to_string(fixture.root.path().join("agy-reopened"))
                .is_ok_and(|log| log.lines().any(|line| line == FIRST))
    });
}
