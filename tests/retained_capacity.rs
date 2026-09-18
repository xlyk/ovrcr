#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT, WORKSPACE, wait_deadline, wait_group_absent};
use ovrcr::protocol::{
    ClientMessage, CreateSessionRequest, DashboardView, ErrorCode, PaneTarget, Request, Response,
    ServerMessage, SessionId, SessionKind, SessionPhase, SessionRunId, SessionSummary,
    TerminalSize, client, connect_server, read_frame, write_frame,
};
use serde_json::Value;
use std::os::unix::net::UnixStream;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn cli(live: &Live, args: &[&str]) -> Output {
    Command::new(&live.executable)
        .args(args)
        .env("OVRCR_CONFIG", &live.config)
        .env("OVRCR_SOCKET", &live.socket)
        .env("SHELL", "/bin/sh")
        .output()
        .unwrap()
}

fn json(live: &Live, args: &[&str]) -> Value {
    let output = cli(live, &[&["--json"], args].concat());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn track_groups(live: &Live) {
    for group in live.session_groups() {
        live.own_group(group);
    }
}

fn summaries(live: &Live) -> Vec<SessionSummary> {
    match live.request(Request::List) {
        Response::Hierarchy(snapshot) => snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .collect(),
        other => panic!("list did not return a hierarchy: {other:?}"),
    }
}

fn find(live: &Live, id: SessionId) -> SessionSummary {
    summaries(live)
        .into_iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("missing session {}", id.0))
}

fn live_count(live: &Live) -> usize {
    summaries(live)
        .iter()
        .filter(|row| row.phase.is_live())
        .count()
}

fn wait_until(
    live: &Live,
    id: SessionId,
    timeout: Duration,
    ready: impl Fn(&SessionSummary) -> bool,
) -> SessionSummary {
    let deadline = Instant::now() + timeout;
    loop {
        let row = find(live, id);
        if ready(&row) {
            return row;
        }
        assert!(
            Instant::now() < deadline,
            "session {} did not reach the expected state: {row:?}",
            id.0
        );
        std::thread::yield_now();
    }
}

fn waiter_argv() -> Vec<std::ffi::OsString> {
    vec!["/bin/sh".into(), "-c".into(), "IFS= read -r value".into()]
}

fn create_named(live: &Live, name: &str, argv: Vec<std::ffi::OsString>) -> SessionSummary {
    match live.request(Request::CreateSession(CreateSessionRequest {
        project: PROJECT.into(),
        workspace: WORKSPACE.into(),
        name: name.into(),
        label: None,
        argv,
        kind: SessionKind::Terminal,
    })) {
        Response::CreatedSession(summary) => {
            track_groups(live);
            *summary
        }
        other => panic!("create {name} failed: {other:?}"),
    }
}

fn conflict(response: Response) -> String {
    match response {
        Response::Error {
            code: ErrorCode::Conflict,
            message,
        } => message,
        other => panic!("expected Conflict, got {other:?}"),
    }
}

fn pgid_of(pid: u32) -> libc::pid_t {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    assert!(pgid > 1, "live pid {pid} has no process group");
    pgid
}

fn kill_release(live: &Live, id: SessionId) {
    let row = find(live, id);
    let pid = row.pid.expect("cannot release a session with no process");
    let pgid = pgid_of(pid);
    live.own_group(pgid);
    let killed = cli(live, &["terminal", "kill", &id.0.to_string()]);
    assert!(
        killed.status.success(),
        "{}",
        String::from_utf8_lossy(&killed.stderr)
    );
    assert!(
        wait_group_absent(pgid, Duration::from_secs(10)),
        "killed session {} group {pgid} is still present",
        id.0
    );
    live.forget_group(pgid);
    wait_until(live, id, Duration::from_secs(10), |row| {
        !row.phase.is_live()
    });
}

fn snapshot_ids(rows: &[SessionSummary]) -> Vec<u64> {
    let mut ids = rows.iter().map(|row| row.id.0).collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

#[test]
fn exited_row_does_not_reduce_live_capacity_and_overflow_publishes_nothing() {
    let live = Live::binary();
    live.ready("feature/retained-capacity");
    track_groups(&live);
    assert_eq!(
        live_count(&live),
        1,
        "default workspace local shell must occupy one live slot"
    );

    let exited = create_named(
        &live,
        "exited-row",
        vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
    );
    wait_until(&live, exited.id, wait_deadline(), |row| {
        matches!(row.phase, SessionPhase::Exited { .. })
    });
    assert!(find(&live, exited.id).pid.is_none());
    assert_eq!(live_count(&live), 1);
    assert_eq!(summaries(&live).len(), 2);

    let mut waiters = Vec::new();
    for index in 0..48 {
        let row = create_named(&live, &format!("live-{index:02}"), waiter_argv());
        assert!(row.phase.is_live(), "{}", row.id.0);
        waiters.push(row.id);
    }
    assert_eq!(live_count(&live), 49);
    assert_eq!(summaries(&live).len(), 50);

    let gate = std::sync::Barrier::new(3);
    let admit = |name: &str| {
        Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: name.into(),
            label: None,
            argv: waiter_argv(),
            kind: SessionKind::Terminal,
        })
    };
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            gate.wait();
            live.request(admit("admit-a"))
        });
        let second = scope.spawn(|| {
            gate.wait();
            live.request(admit("admit-b"))
        });
        gate.wait();
        (first.join().unwrap(), second.join().unwrap())
    });
    let mut created = 0usize;
    let mut conflicts = 0usize;
    for response in [first, second] {
        match response {
            Response::CreatedSession(_) => created += 1,
            Response::Error {
                code: ErrorCode::Conflict,
                message,
            } => {
                conflicts += 1;
                assert!(message.contains("50 live process slots"), "{message}");
            }
            other => panic!("admission race produced {other:?}"),
        }
    }
    assert_eq!(
        created, 1,
        "the last live slot must be reserved for one admission"
    );
    assert_eq!(
        conflicts, 1,
        "the losing admission must not take a 51st live slot"
    );
    track_groups(&live);

    let after_fill = summaries(&live);
    assert_eq!(after_fill.len(), 51, "one exited row plus 50 live rows");
    assert_eq!(live_count(&live), 50);
    assert_eq!(
        after_fill.iter().filter(|row| row.pid.is_some()).count(),
        50
    );
    let filled_ids = snapshot_ids(&after_fill);
    let filled_groups = live.session_groups().len();
    assert_eq!(filled_groups, 50);
    let admit_names = after_fill
        .iter()
        .map(|row| row.name.as_str())
        .filter(|name| *name == "admit-a" || *name == "admit-b")
        .count();
    assert_eq!(
        admit_names, 1,
        "the refused concurrent create published a row"
    );

    let ghost = live.root.path().join("overflow-must-not-run");
    let ghost_arg = ghost.to_str().unwrap();
    let overflow_script = "printf ran > \"$1\"; IFS= read -r value";
    let overflow = cli(
        &live,
        &[
            "--json",
            "terminal",
            "create",
            "--project",
            PROJECT,
            "--workspace",
            "feature/retained-capacity",
            "--name",
            "overflow-ghost",
            "--",
            "/bin/sh",
            "-c",
            overflow_script,
            "overflow-guard",
            ghost_arg,
        ],
    );
    assert!(
        !overflow.status.success(),
        "overflow create published a session: {}",
        String::from_utf8_lossy(&overflow.stdout)
    );
    let error = String::from_utf8_lossy(&overflow.stderr);
    assert!(
        error.contains("Conflict") && error.contains("50 live process slots"),
        "{error}"
    );
    assert!(!ghost.exists(), "overflow create started a process");

    let overflow_reopen = live.request(Request::ReopenSession {
        session: exited.id,
        expected_run: exited.run,
        acknowledge_stopped: true,
    });
    let message = conflict(overflow_reopen);
    assert!(message.contains("50 live process slots"), "{message}");

    let after_overflow = summaries(&live);
    assert_eq!(snapshot_ids(&after_overflow), filled_ids);
    assert_eq!(live_count(&live), 50);
    assert_eq!(live.session_groups().len(), filled_groups);
    assert!(
        after_overflow
            .iter()
            .all(|row| row.name != "overflow-ghost")
    );
    let exited_now = find(&live, exited.id);
    assert_eq!(exited_now.run, exited.run);
    assert!(exited_now.pid.is_none());
    assert!(!exited_now.phase.is_live());

    kill_release(&live, waiters[0]);
    kill_release(&live, waiters[1]);
    assert_eq!(live_count(&live), 48);

    let created = json(
        &live,
        &[
            "terminal",
            "create",
            "--project",
            PROJECT,
            "--workspace",
            "feature/retained-capacity",
            "--name",
            "released-create",
            "--",
            "/bin/sh",
            "-c",
            "IFS= read -r value",
        ],
    );
    track_groups(&live);
    assert_eq!(created["name"], "released-create");
    assert_eq!(created["phase"], "running");
    assert!(created["pid"].as_u64().is_some());
    assert_eq!(live_count(&live), 49);

    let reopened = json(
        &live,
        &[
            "terminal",
            "reopen",
            &exited.id.0.to_string(),
            "--ack-stopped",
        ],
    );
    track_groups(&live);
    assert_eq!(reopened["id"], exited.id.0);
    assert_eq!(reopened["run"], exited.run.0 + 1);
    assert_eq!(reopened["phase"], "running");
    assert!(reopened["pid"].as_u64().is_some());
    assert_eq!(live_count(&live), 50);
    assert_eq!(summaries(&live).len(), 52);
}

#[test]
fn closed_session_id_is_not_reused_after_restart() {
    let live = Live::binary();
    live.ready("feature/retained-id-reuse");
    track_groups(&live);
    let created = create_named(&live, "delete-me", waiter_argv());
    let deleted = created.id;
    let pid = created.pid.expect("created session has no pid");
    let pgid = pgid_of(pid);
    live.own_group(pgid);
    let closed = cli(&live, &["terminal", "close", &deleted.0.to_string()]);
    assert!(
        closed.status.success(),
        "{}",
        String::from_utf8_lossy(&closed.stderr)
    );
    assert!(
        wait_group_absent(pgid, Duration::from_secs(10)),
        "closed session group {pgid} is still present"
    );
    live.forget_group(pgid);
    assert!(
        summaries(&live).iter().all(|row| row.id != deleted),
        "closed session {} remained in inventory",
        deleted.0
    );

    let stopped = cli(&live, &["shutdown", "--kill"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    live.join();

    live.start_binary();
    assert!(
        summaries(&live).iter().all(|row| row.id != deleted),
        "restart restored deleted session {}",
        deleted.0
    );
    let replacement = json(
        &live,
        &[
            "terminal",
            "create",
            "--project",
            PROJECT,
            "--workspace",
            "feature/retained-id-reuse",
            "--name",
            "after-delete",
            "--",
            "/bin/sh",
            "-c",
            "IFS= read -r value",
        ],
    );
    track_groups(&live);
    let replacement_id = replacement["id"].as_u64().unwrap();
    assert_ne!(
        replacement_id, deleted.0,
        "deleted session id was reused after restart"
    );
    assert!(
        replacement_id > deleted.0,
        "session ids must keep AUTOINCREMENT after delete, got {replacement_id} after {}",
        deleted.0
    );
}

fn dashboard(live: &Live) -> UnixStream {
    let mut stream = connect_server(&live.socket).unwrap();
    stream.set_read_timeout(Some(wait_deadline())).unwrap();
    stream.set_write_timeout(Some(wait_deadline())).unwrap();
    let response = client::request(&mut stream, 1, Request::DashboardHello).unwrap();
    assert!(matches!(response, Response::Hierarchy(_)), "{response:?}");
    stream
}

fn write_request(stream: &mut UnixStream, request_id: u64, request: Request) {
    write_frame(
        stream,
        &ClientMessage {
            request_id,
            request,
        },
    )
    .unwrap();
}

fn read_dashboard(stream: &mut UnixStream, deadline: Instant, label: &str) -> ServerMessage {
    loop {
        assert!(Instant::now() < deadline, "{label} timed out");
        match read_frame::<ServerMessage>(stream) {
            Ok(message) => return message,
            Err(error) if dashboard_timeout(&*error) => continue,
            Err(error) => panic!("{label}: {error:#}"),
        }
    }
}

fn dashboard_timeout(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(cause) = current {
        if let Some(io) = cause.downcast_ref::<std::io::Error>()
            && matches!(
                io.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )
        {
            return true;
        }
        current = cause.source();
    }
    false
}

fn set_view_bound_to_run(
    stream: &mut UnixStream,
    request_id: u64,
    view: &DashboardView,
) -> Vec<u8> {
    write_request(stream, request_id, Request::SetView { view: view.clone() });
    let deadline = Instant::now() + wait_deadline();
    let pane = &view.panes[0];
    let mut screen = None;
    let mut acknowledged = false;
    while !acknowledged {
        match read_dashboard(stream, deadline, "SetView") {
            ServerMessage::Response {
                request_id: id,
                response,
            } if id == request_id => match response {
                Response::Screen {
                    session,
                    run,
                    revision,
                    bytes,
                    ..
                } => {
                    assert_eq!(session, pane.session);
                    assert_eq!(run, pane.run, "SetView snapshot used a different run");
                    assert_eq!(revision, view.revision);
                    assert!(
                        screen.replace(bytes).is_none(),
                        "SetView returned a duplicate Screen"
                    );
                }
                Response::Ok => {
                    assert!(
                        screen.is_some(),
                        "SetView Ok arrived before the Screen frame; reusing this request id would look like success"
                    );
                    acknowledged = true;
                }
                Response::Error { code, message } => {
                    panic!("SetView failed: {code:?}: {message}")
                }
                other => panic!("SetView returned {other:?}"),
            },
            _ => {}
        }
    }
    screen.expect("SetView Ok without Screen")
}

fn wait_matching_response(
    stream: &mut UnixStream,
    request_id: u64,
    deadline: Instant,
    label: &str,
) -> Response {
    loop {
        match read_dashboard(stream, deadline, label) {
            ServerMessage::Response {
                request_id: id,
                response,
            } if id == request_id => return response,
            _ => {}
        }
    }
}

#[test]
fn dashboard_rejects_old_run_input_after_same_row_reopen() {
    let live = Live::binary();
    live.ready("feature/retained-run-input");
    track_groups(&live);
    let created = create_named(&live, "run-owner", vec!["/bin/sh".into()]);
    let id = created.id;
    let old_run = created.run;
    let size = TerminalSize { rows: 24, cols: 80 };
    let mut dashboard = dashboard(&live);
    let first_view = DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            session: id,
            run: old_run,
            size,
        }],
        focused: Some(id),
    };
    set_view_bound_to_run(&mut dashboard, 2, &first_view);

    assert_eq!(
        live.request(Request::SendTerminal {
            session: id,
            text: "exit".into(),
            submit: true,
        }),
        Response::Ok
    );
    wait_until(&live, id, wait_deadline(), |row| !row.phase.is_live());

    let reopened = json(
        &live,
        &["terminal", "reopen", &id.0.to_string(), "--ack-stopped"],
    );
    track_groups(&live);
    let new_run = SessionRunId(reopened["run"].as_u64().unwrap());
    assert_eq!(reopened["id"], id.0);
    assert_ne!(new_run, old_run);

    let stale_future = live.request(Request::ReopenSession {
        session: id,
        expected_run: SessionRunId(new_run.0 + 5),
        acknowledge_stopped: false,
    });
    let message = conflict(stale_future);
    assert!(
        message.contains("run changed"),
        "future expected-run must not coalesce or spawn: {message}"
    );
    assert_eq!(find(&live, id).run, new_run);

    let second_view = DashboardView {
        revision: 2,
        panes: vec![PaneTarget {
            session: id,
            run: new_run,
            size,
        }],
        focused: Some(id),
    };
    set_view_bound_to_run(&mut dashboard, 10, &second_view);

    write_request(
        &mut dashboard,
        11,
        Request::Input {
            session: id,
            run: old_run,
            bytes: b"printf 'STALE_%s\\n' CAP_114\n".to_vec(),
        },
    );
    match wait_matching_response(
        &mut dashboard,
        11,
        Instant::now() + wait_deadline(),
        "stale Input",
    ) {
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        } => {}
        Response::Ok => panic!("old-run Input was acknowledged after reopen"),
        other => panic!("old-run Input produced {other:?}"),
    }

    write_request(
        &mut dashboard,
        12,
        Request::Input {
            session: id,
            run: new_run,
            bytes: b"printf 'FRESH_%s\\n' CAP_114\n".to_vec(),
        },
    );
    match wait_matching_response(
        &mut dashboard,
        12,
        Instant::now() + wait_deadline(),
        "fresh Input",
    ) {
        Response::Ok => {}
        other => panic!("current-run Input failed: {other:?}"),
    }

    let deadline = Instant::now() + wait_deadline();
    loop {
        let output = cli(&live, &["terminal", "read", &id.0.to_string()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            !text.contains("STALE_CAP_114"),
            "stale input executed after reopen: {text}"
        );
        if text.contains("FRESH_CAP_114") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fresh input did not execute: {text}"
        );
        std::thread::yield_now();
    }
}
