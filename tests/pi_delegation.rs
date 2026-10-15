//! Public Pi tools -> control CLI -> isolated binary Server -> real managed child PTY.
#[path = "support/live.rs"]
mod live;
use live::{Live, PROJECT, WORKSPACE};
use ovrcr::protocol::*;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};

fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(Instant::now() < deadline, "missing {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn tool_reply(
    live: &Live,
    parent: SessionId,
    name: &str,
    args: serde_json::Value,
    file: &Path,
) -> serde_json::Value {
    let command = format!(
        "tool:{}",
        serde_json::json!({"name":name,"args":args,"output":file})
    );
    assert_eq!(
        live.request(Request::SendTerminal {
            session: parent,
            text: command,
            submit: true
        }),
        Response::Ok
    );
    wait_file(file);
    let value: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    value
}
fn tool(
    live: &Live,
    parent: SessionId,
    name: &str,
    args: serde_json::Value,
    file: &Path,
) -> serde_json::Value {
    let value = tool_reply(live, parent, name, args, file);
    assert!(value.get("error").is_none(), "{value}");
    value["details"].clone()
}
fn snapshot(live: &Live, parent: SessionId, handle: &str, file: &Path) -> AssignmentSnapshot {
    serde_json::from_value(tool(
        live,
        parent,
        "ovrcr_collect",
        serde_json::json!({"handle":handle}),
        file,
    ))
    .unwrap()
}
fn dashboard_view(
    stream: &mut std::os::unix::net::UnixStream,
    id: u64,
    request: Request,
) -> anyhow::Result<Response> {
    write_frame(
        stream,
        &ClientMessage {
            request_id: id,
            request,
        },
    )?;
    loop {
        if let ServerMessage::Response {
            request_id,
            response,
        } = read_frame(stream)?
        {
            if request_id == id && !matches!(response, Response::Screen { .. }) {
                return Ok(response);
            }
        }
    }
}

#[test]
fn public_pi_research_assignment_launches_collects_and_fences_real_child() {
    let live = Live::idle();
    let gate = live.root.path().join("allow-result");
    let bin = live.claude.command.parent().unwrap();
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
    let pi = bin.join("pi");
    fs::write(&pi, include_bytes!("fixtures/pi/research_pi.sh")).unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o700)).unwrap();
    live.start_binary_env(&[
        ("OVRCR_TEST_DELEGATION", "1".as_ref()),
        ("OVRCR_TEST_PI_HOST", host.as_os_str()),
        ("OVRCR_TEST_DELEGATION_RESULT_GATE", gate.as_os_str()),
    ]);
    live.ready("research-fixture");
    let Response::CreatedSession(parent) =
        live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "parent".into(),
            label: None,
            argv: vec![
                env!("CARGO_BIN_EXE_ovrcr").into(),
                "agent".into(),
                "run".into(),
                "pi".into(),
                "--".into(),
                "pi".into(),
                "--model".into(),
                "fixture-model".into(),
            ],
            kind: SessionKind::Agent { name: "pi".into() },
        }))
    else {
        panic!("create parent");
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
            panic!()
        };
        if sessions
            .iter()
            .find(|s| s.id == parent.id)
            .unwrap()
            .agent
            .is_some()
        {
            break;
        }
        assert!(Instant::now() < deadline, "parent binding missing");
        std::thread::sleep(Duration::from_millis(10));
    }
    let raw = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":WORKSPACE,"scope":"Read README. Do not change files.","model_provider":"test","model":"fixture-model","acceptance_evidence":["README citation"]}),
        &live.root.path().join("start.json"),
    );
    let started: AssignmentSnapshot = serde_json::from_value(raw.clone()).expect(&raw.to_string());
    assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
    assert!(
        started.submission.is_none(),
        "launch must not wait for completion"
    );
    assert_eq!(started.acceptance, "pending");
    let child = started.identity.session.unwrap();
    let run = started.identity.run.unwrap();
    wait_file(&started.brief.with_extension("json.started"));
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!()
    };
    assert!(sessions.iter().any(|s| s.id == child
        && s.run == run
        && s.kind == SessionKind::Agent { name: "pi".into() }));
    for request in [
        Request::SendTerminal {
            session: child,
            text: "UNOWNED".into(),
            submit: true,
        },
        Request::Keystroke {
            session: child,
            expected_run: run,
            key: ":enter:".into(),
        },
    ] {
        assert!(matches!(
            live.request(request),
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }
    // The real Dashboard socket can inspect this session but its Input request
    // crosses the same ownership fence. Ordinary focused input still works.
    let mut dashboard = connect_server(&live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    let size = TerminalSize {
        rows: 24,
        cols: 120,
    };
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            2,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: child,
                        run,
                        size
                    }],
                    focused: Some(child)
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    assert!(matches!(
        client::request(
            &mut dashboard,
            3,
            Request::Input {
                session: child,
                run,
                bytes: b"UNOWNED\r".to_vec()
            }
        )
        .unwrap(),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    let ordinary = sessions.iter().find(|s| s.name == "local").unwrap();
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            4,
            Request::SetView {
                view: DashboardView {
                    revision: 2,
                    panes: vec![PaneTarget {
                        session: ordinary.id,
                        run: ordinary.run,
                        size
                    }],
                    focused: Some(ordinary.id)
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    let marker = live.root.path().join("ordinary-input");
    assert_eq!(
        client::request(
            &mut dashboard,
            5,
            Request::Input {
                session: ordinary.id,
                run: ordinary.run,
                bytes: format!("printf ordinary > '{}'\r", marker.display()).into_bytes()
            }
        )
        .unwrap(),
        Response::Ok
    );
    wait_file(&marker);
    assert_eq!(fs::read_to_string(marker).unwrap(), "ordinary");
    assert!(
        matches!(
            live.request(Request::Keystroke {
                session: ordinary.id,
                expected_run: ordinary.run,
                key: ":enter:".into()
            }),
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ),
        "ordinary Keystroke focus guard must stay intact"
    );
    drop(dashboard);
    let missing = snapshot(
        &live,
        parent.id,
        &started.handle,
        &live.root.path().join("missing.json"),
    );
    assert!(missing.submission.is_none());
    fs::write(&gate, "continue").unwrap();
    wait_file(&started.brief.with_extension("json.published"));
    let completed = snapshot(
        &live,
        parent.id,
        &started.handle,
        &live.root.path().join("complete.json"),
    );
    assert_eq!(
        completed.submission.as_ref().unwrap().summary,
        "Read the assigned repository README."
    );
    assert_eq!(completed.acceptance, "pending");
    assert_eq!(
        fs::metadata(&completed.result_path).unwrap().mode() & 0o077,
        0
    );
    // Reusing the initial operation is a read of its receipt, not another input.
    let same = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"handle":started.handle}),
        &live.root.path().join("same.json"),
    );
    assert_eq!(
        same["identity"],
        serde_json::to_value(&completed.identity).unwrap()
    );
    assert_eq!(
        fs::read_to_string(started.brief.with_extension("json.input-count")).unwrap(),
        "1\n",
        "repeated start must not replay input"
    );
    let events = serde_json::to_string(&live.request(Request::Events { follow: false })).unwrap();
    assert!(!events.contains("Read README. Do not change files."));
    assert!(!events.contains("Read the assigned repository README."));
    // Known refusal stays separate from an uncertain transport outcome.
    let contract = serde_json::json!({"project":PROJECT,"workspace":WORKSPACE,"scope":"Read README", "model_provider":"test", "model":"missing-model", "acceptance_evidence":["README citation"]});
    let refused = tool(
        &live,
        parent.id,
        "ovrcr_start",
        contract.clone(),
        &live.root.path().join("refused.json"),
    );
    assert_eq!(refused["dispatch"], "Refused");
    assert!(refused["identity"]["session"].is_null());
    let mut unsupported = contract.clone();
    unsupported["mandatory_capabilities"] = serde_json::json!(["unlimited_network"]);
    let unsupported = tool(
        &live,
        parent.id,
        "ovrcr_start",
        unsupported,
        &live.root.path().join("unsupported.json"),
    );
    assert_eq!(unsupported["dispatch"], "Refused");
    assert!(
        unsupported["reason"]
            .as_str()
            .unwrap()
            .contains("unsupported mandatory capability")
    );
    // The parent can collect but cannot impersonate the child publisher.
    let forged = tool_reply(
        &live,
        parent.id,
        "ovrcr_publish",
        serde_json::json!({"handle":started.handle,"submission":completed.submission}),
        &live.root.path().join("forged.json"),
    );
    assert!(
        forged["error"]
            .as_str()
            .unwrap()
            .contains("only the assigned child")
    );
    // A real child submitting a stale contract cannot produce a collectable result.
    let mut stale_contract = contract;
    stale_contract["scope"] = "STALE_RESULT: Read README".into();
    stale_contract["model"] = "fixture-model".into();
    let stale: AssignmentSnapshot = serde_json::from_value(tool(
        &live,
        parent.id,
        "ovrcr_start",
        stale_contract,
        &live.root.path().join("stale-start.json"),
    ))
    .unwrap();
    wait_file(&stale.brief.with_extension("json.published"));
    let rejection = fs::read_to_string(stale.brief.with_extension("json.published")).unwrap();
    assert!(
        rejection.contains("foreign or stale submission identity"),
        "{rejection}"
    );
    let stale = snapshot(
        &live,
        parent.id,
        &stale.handle,
        &live.root.path().join("stale-collect.json"),
    );
    assert!(stale.submission.is_none());
    // Even identical contents at an escaping symlink do not establish a publication.
    let evidence = fs::read(&completed.result_path).unwrap();
    let escaped = live.root.path().join("outside.json");
    fs::write(&escaped, &evidence).unwrap();
    fs::set_permissions(&escaped, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&completed.result_path).unwrap();
    std::os::unix::fs::symlink(&escaped, &completed.result_path).unwrap();
    let escape = tool_reply(
        &live,
        parent.id,
        "ovrcr_collect",
        serde_json::json!({"handle":started.handle}),
        &live.root.path().join("escape.json"),
    );
    assert!(
        escape["error"].is_string(),
        "symlink publication accepted: {escape}"
    );
    fs::remove_file(&completed.result_path).unwrap();
    fs::write(&completed.result_path, &evidence).unwrap();
    fs::set_permissions(&completed.result_path, fs::Permissions::from_mode(0o600)).unwrap();
    // A published success cannot authorize a changed file.
    fs::write(&completed.result_path, b"{partial").unwrap();
    let command = format!(
        "tool:{}",
        serde_json::json!({"name":"ovrcr_collect","args":{"handle":started.handle},"output":live.root.path().join("partial.json")})
    );
    assert_eq!(
        live.request(Request::SendTerminal {
            session: parent.id,
            text: command,
            submit: true
        }),
        Response::Ok
    );
    wait_file(&live.root.path().join("partial.json"));
    let partial = fs::read_to_string(live.root.path().join("partial.json")).unwrap();
    assert!(partial.contains("changed or is partial"), "{partial}");
    live.own_group(unsafe { libc::getpgid(parent.pid.unwrap() as i32) });
    for group in live.session_groups() {
        live.own_group(group);
    }
}
