//! Public Pi tools -> control CLI -> isolated binary Server -> real managed child PTY.
#[path = "pi_delegation/control.rs"]
mod assignment_control;
#[path = "pi_delegation/delivery.rs"]
mod delivery;
#[path = "pi_delegation/followups.rs"]
mod followups;
#[path = "support/live.rs"]
mod live;
#[path = "pi_delegation/recovery.rs"]
mod parent_recovery;
#[path = "pi_delegation/questions.rs"]
mod questions;
use live::{Live, PROJECT};
use ovrcr::protocol::*;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};

fn cli_json(live: &Live, args: &[&str]) -> serde_json::Value {
    let output = live.command().arg("--json").args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

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
            && request_id == id
            && !matches!(response, Response::Screen { .. })
        {
            return Ok(response);
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
    let workspaces = cli_json(&live, &["workspace", "list", "--project", PROJECT]);
    let workspace = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| workspace["branch"] == "research-fixture")
        .unwrap();
    let workspace_id = workspace["id"]
        .as_str()
        .expect("public workspace ID")
        .to_owned();
    assert_ne!(workspace_id, "research-fixture");
    let selected = cli_json(
        &live,
        &[
            "workspace",
            "get",
            "--project",
            PROJECT,
            "--branch",
            "research-fixture",
        ],
    );
    assert_eq!(selected["id"], workspace_id);
    let Response::CreatedSession(parent) =
        live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: workspace_id.clone(),
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
    let capabilities = tool(
        &live,
        parent.id,
        "ovrcr_capabilities",
        serde_json::json!({}),
        &live.root.path().join("capabilities.json"),
    );
    assert_eq!(capabilities["choices"].as_array().unwrap().len(), 2);
    assert!(
        capabilities["resolved_choice"].is_null(),
        "multiple models need a parent selection"
    );
    assert_eq!(capabilities["native_helpers"], false);
    assert_eq!(capabilities["native_descendant_usage"], false);
    let omitted = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","acceptance_evidence":["citation"]}),
        &live.root.path().join("needs-choice.json"),
    );
    assert_eq!(omitted["dispatch"], "Refused");
    assert!(omitted["handle"].is_null());
    let set = live
        .command()
        .args([
            "settings",
            "set",
            "delegation.default_model",
            "test/alternate-model",
        ])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let defaulted = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README using the configured default","acceptance_evidence":["citation"]}),
        &live.root.path().join("default-choice.json"),
    );
    assert_eq!(defaulted["dispatch"], "Applied", "{defaulted}");
    for group in live.session_groups() {
        live.own_group(group);
    }
    let default_brief: serde_json::Value =
        serde_json::from_slice(&fs::read(defaulted["brief"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(default_brief["contract"]["provider"], "pi");
    assert_eq!(default_brief["contract"]["model_provider"], "test");
    assert_eq!(default_brief["contract"]["model"], "alternate-model");
    assert_eq!(default_brief["contract"]["limits"]["max_live_children"], 50);
    let raw = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README. Do not change files.","model_provider":"test","model":"fixture-model","acceptance_evidence":["README citation"]}),
        &live.root.path().join("start.json"),
    );
    let started: AssignmentSnapshot =
        serde_json::from_value(raw.clone()).unwrap_or_else(|_| panic!("{raw}"));
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
        && s.workspace == workspace_id
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
    let contract = serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README", "model_provider":"test", "model":"missing-model", "acceptance_evidence":["README citation"]});
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
    let helper_gap = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"],"mandatory_capabilities":["native_helpers"]}),
        &live.root.path().join("native-helper-gap.json"),
    );
    assert_eq!(helper_gap["dispatch"], "Refused");
    assert!(
        helper_gap["reason"]
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
    // The same public production path enforces typed policy without changing child flags.
    let unsupported_limit = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"],"required_limits":{"max_output_tokens":1}}),
        &live.root.path().join("unsupported-limit.json"),
    );
    assert_eq!(unsupported_limit["dispatch"], "Refused");
    assert!(
        unsupported_limit["reason"]
            .as_str()
            .unwrap()
            .contains("unsupported mandatory")
    );
    for (key, value) in [
        ("delegation.enabled", "false"),
        ("delegation.max_live_children", "1"),
        ("delegation.max_execution_attempts", "1"),
    ] {
        let output = live
            .command()
            .args(["settings", "set", key, value])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let disabled = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}),
        &live.root.path().join("disabled.json"),
    );
    assert_eq!(disabled["dispatch"], "Refused");
    assert!(disabled["reason"].as_str().unwrap().contains("disabled"));
    let output = live
        .command()
        .args(["settings", "set", "delegation.enabled", "true"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let limited = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}),
        &live.root.path().join("child-limit.json"),
    );
    assert_eq!(limited["dispatch"], "Refused", "{limited}");
    assert_eq!(limited["retryable_capacity_refusal"], true);
    assert_eq!(limited["execution_attempts"], 0);
    let output = live
        .command()
        .args(["settings", "set", "delegation.max_live_children", "50"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let limited_retry = tool(
        &live,
        parent.id,
        "ovrcr_retry",
        serde_json::json!({"handle":limited["handle"]}),
        &live.root.path().join("immutable-limit-retry.json"),
    );
    assert_eq!(
        limited_retry["dispatch"], "Refused",
        "retry cannot loosen the original effective child limit"
    );
    assert_eq!(limited_retry["execution_attempts"], 0);
    let minimum_free = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"],"required_limits":{"min_free_slots_after_launch":49}}),
        &live.root.path().join("minimum-free.json"),
    );
    assert_eq!(minimum_free["dispatch"], "Refused");
    assert_eq!(minimum_free["retryable_capacity_refusal"], true);
    // Delegation uses the existing shared fifty-live-session admission rule.
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("inventory");
    };
    let count = sessions
        .iter()
        .filter(|session| session.phase.is_live())
        .count();
    for index in count..50 {
        let Response::CreatedSession(session) =
            live.request(Request::CreateSession(CreateSessionRequest {
                project: PROJECT.into(),
                workspace: workspace_id.clone(),
                name: format!("capacity-{index}"),
                label: None,
                argv: vec!["/bin/sh".into()],
                kind: SessionKind::Terminal,
            }))
        else {
            panic!("fill existing live capacity");
        };
        live.own_group(unsafe { libc::getpgid(session.pid.unwrap() as i32) });
    }
    let capacity = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","model_provider":"test","model":"fixture-model","acceptance_evidence":["README citation"]}),
        &live.root.path().join("capacity.json"),
    );
    assert_eq!(capacity["dispatch"], "Refused");
    assert!(
        capacity["reason"]
            .as_str()
            .unwrap()
            .contains("50 live process slots")
    );
    let refused = snapshot(
        &live,
        parent.id,
        capacity["handle"].as_str().unwrap(),
        &live.root.path().join("capacity-collected.json"),
    );
    assert_eq!(refused.dispatch, DispatchStatus::Refused);
    assert!(refused.identity.session.is_none());
    live.own_group(unsafe { libc::getpgid(parent.pid.unwrap() as i32) });
    for group in live.session_groups() {
        live.own_group(group);
    }
}

#[test]
fn public_pi_preflight_refusal_refunds_execution_but_post_spawn_failure_does_not() {
    let live = Live::idle();
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
    let pi = live.claude.command.parent().unwrap().join("pi");
    let probes = live.root.path().join("model-probes");
    let entered = live.root.path().join("launch-preflight");
    let release = live.root.path().join("release-preflight");
    // Public start discovers capabilities, prepares, then checks availability for
    // Launch. Pause only that third inventory probe: Prepare has committed, but
    // no native child creation has been attempted.
    let gate = format!(
        "#!/bin/sh\ncase \" $* \" in *\" --list-models \"*) n=0; [ ! -f '{0}' ] || n=$(cat '{0}'); n=$((n+1)); echo \"$n\" > '{0}'; if [ \"$n\" = 3 ]; then : > '{1}'; while [ ! -f '{2}' ]; do sleep 0.01; done; fi;; esac\n",
        probes.display(),
        entered.display(),
        release.display(),
    );
    fs::write(
        &pi,
        format!("{gate}{}", include_str!("fixtures/pi/research_pi.sh")),
    )
    .unwrap();
    fs::set_permissions(&pi, fs::Permissions::from_mode(0o700)).unwrap();
    live.start_binary_env(&[
        ("OVRCR_TEST_DELEGATION", "1".as_ref()),
        ("OVRCR_TEST_PI_HOST", host.as_os_str()),
    ]);
    live.ready("preflight-refund");
    let workspaces = cli_json(&live, &["workspace", "list", "--project", PROJECT]);
    let workspace = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| workspace["branch"] == "preflight-refund")
        .unwrap();
    let workspace_id = workspace["id"].as_str().unwrap();
    let workspace_path = std::path::PathBuf::from(workspace["path"].as_str().unwrap());
    let Response::CreatedSession(parent) =
        live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: workspace_id.into(),
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
        panic!("parent creation");
    };
    for group in live.session_groups() {
        live.own_group(group);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
            panic!("inventory")
        };
        if sessions
            .iter()
            .any(|s| s.id == parent.id && s.agent.is_some())
        {
            break;
        }
        assert!(Instant::now() < deadline, "parent binding");
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = live.root.path().join("preflight-refused.json");
    let args = serde_json::json!({"project":PROJECT,"workspace":workspace_id,"scope":"Read README","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"],"required_limits":{"max_execution_attempts":1}});
    assert_eq!(
        live.request(Request::SendTerminal {
            session: parent.id,
            text: format!(
                "tool:{}",
                serde_json::json!({"name":"ovrcr_start","args":args,"output":output})
            ),
            submit: true,
        }),
        Response::Ok
    );
    wait_file(&entered);
    let moved = live.root.path().join("temporarily-moved-workspace");
    fs::rename(&workspace_path, &moved).unwrap();
    fs::write(&release, "release").unwrap();
    wait_file(&output);
    // Restore before any assertion can fail so even RED leaves fixture-owned
    // resource cleanup on its ordinary path.
    fs::rename(&moved, &workspace_path).unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert!(reply.get("error").is_none(), "{reply}");
    let refused: AssignmentSnapshot = serde_json::from_value(reply["details"].clone()).unwrap();
    assert_eq!(refused.dispatch, DispatchStatus::Refused);
    assert!(refused.identity.session.is_none());
    assert_eq!(
        refused.execution_attempts,
        Some(0),
        "pre-allocation workspace refusal has authoritative zero effects"
    );
    assert!(
        !refused.retryable_capacity_refusal,
        "a workspace error is not capacity retry authorization"
    );
    let repeated = tool(
        &live,
        parent.id,
        "ovrcr_start",
        serde_json::json!({"handle":refused.handle}),
        &live.root.path().join("refused-repeat.json"),
    );
    assert_eq!(repeated["dispatch"], "Refused");
    assert_eq!(repeated["execution_attempts"], 0);
    assert_eq!(repeated["identity"], reply["details"]["identity"]);
    let retried = tool(
        &live,
        parent.id,
        "ovrcr_retry",
        serde_json::json!({"handle":refused.handle}),
        &live.root.path().join("refused-retry.json"),
    );
    assert_eq!(retried["dispatch"], "Refused");
    assert!(
        retried["reason"]
            .as_str()
            .unwrap()
            .contains("zero-effect capacity")
    );
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("inventory")
    };
    assert!(
        sessions.iter().all(
            |s| !matches!(&s.kind, SessionKind::Agent { name } if name == "pi")
                || s.id == parent.id
        ),
        "no child allocated on refusal"
    );

    // A real native managed child is now created, but its provider fails before
    // it can bind. A later exited/non-live child does not refund that execution.
    let child_entered = live.root.path().join("provider-started");
    fs::write(
        &pi,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" --no-approve \"*) : > '{}'; exit 23;; esac\n{}",
            child_entered.display(),
            include_str!("fixtures/pi/research_pi.sh")
        ),
    )
    .unwrap();
    let failed = tool(
        &live,
        parent.id,
        "ovrcr_start",
        args,
        &live.root.path().join("post-spawn-failed.json"),
    );
    wait_file(&child_entered);
    assert_eq!(
        failed["execution_attempts"], 1,
        "a provider startup failure follows native creation"
    );
    assert!(!failed["retryable_capacity_refusal"].as_bool().unwrap());
    let child: SessionId = serde_json::from_value(failed["identity"]["session"].clone()).unwrap();
    assert_ne!(child, parent.id);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
            panic!("inventory")
        };
        if sessions
            .iter()
            .find(|s| s.id == child)
            .is_none_or(|s| !s.phase.is_live())
        {
            break;
        }
        assert!(Instant::now() < deadline, "failed provider must exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    let after = tool(
        &live,
        parent.id,
        "ovrcr_inspect",
        serde_json::json!({"handle":failed["handle"]}),
        &live.root.path().join("after-provider-exit.json"),
    );
    assert_eq!(
        after["execution_attempts"], 1,
        "later resource absence is never zero-effect proof"
    );
    let retry = tool(
        &live,
        parent.id,
        "ovrcr_retry",
        serde_json::json!({"handle":failed["handle"]}),
        &live.root.path().join("post-spawn-retry.json"),
    );
    assert_eq!(retry["dispatch"], "Refused");
    for group in live.session_groups() {
        live.own_group(group);
    }
}

#[test]
fn ordinary_input_and_view_continue_while_workspace_lifecycle_is_busy() {
    let live = Live::binary();
    live.ready("input-progress");
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let ordinary = sessions.iter().find(|s| s.name == "local").unwrap();
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("git"))
        .find(|p| p.is_file())
        .unwrap();
    let entered = live.root.path().join("git-entered");
    let release = live.root.path().join("git-release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let _release = Release(release.clone());
    let shim = live.claude.command.parent().unwrap().join("git");
    fs::write(&shim,format!("#!/bin/sh\ncase \"$*\" in *\"worktree add\"*\"blocked-lifecycle\"*) : > '{}'; while [ ! -f '{}' ]; do sleep 0.01; done;; esac\nexec '{}' \"$@\"\n", entered.display(),release.display(),git.display())).unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    let mut dashboard = connect_server(&live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    client::request(&mut dashboard, 1, Request::DashboardHello).unwrap();
    assert_eq!(
        client::request(
            &mut dashboard,
            2,
            Request::CreateWorkspace {
                project: PROJECT.into(),
                id: "blocked-lifecycle".into(),
                branch: BranchRequest::New {
                    branch: "blocked-lifecycle".into(),
                    base: "main".into()
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    wait_file(&entered);
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            3,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: ordinary.id,
                        run: ordinary.run,
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: Some(ordinary.id)
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    let marker = live.root.path().join("input-during-lifecycle");
    assert_eq!(
        client::request(
            &mut dashboard,
            4,
            Request::Input {
                session: ordinary.id,
                run: ordinary.run,
                bytes: format!("printf delivered > '{}'\r", marker.display()).into_bytes()
            }
        )
        .unwrap(),
        Response::Ok
    );
    wait_file(&marker);
    assert_eq!(fs::read_to_string(marker).unwrap(), "delivered");
    assert!(
        !release.exists(),
        "the workspace lifecycle must still be blocked"
    );
    let pasted = live.root.path().join("paste-during-lifecycle");
    assert_eq!(
        live::request_with_timeout(
            &live.socket,
            5,
            Request::SendTerminal {
                session: ordinary.id,
                text: format!("printf pasted > '{}'", pasted.display()),
                submit: true
            },
            Duration::from_secs(1)
        ),
        Some(Response::Ok)
    );
    wait_file(&pasted);
    assert!(matches!(
        live::request_with_timeout(
            &live.socket,
            6,
            Request::Keystroke {
                session: ordinary.id,
                expected_run: ordinary.run,
                key: ":enter:".into()
            },
            Duration::from_secs(1)
        ),
        Some(Response::Error {
            code: ErrorCode::Conflict,
            ..
        })
    ));
    fs::write(&release, "release").unwrap();
    for group in live.session_groups() {
        live.own_group(group);
    }
}

#[derive(Clone)]
struct ControlRule {
    operation: &'static str,
    // 0: drop before forwarding; 1: discard real reply; 2: forward with fault.
    loss: u8,
    fault: Option<(&'static str, usize)>,
}

struct ControlProxy {
    path: std::path::PathBuf,
    rules: std::sync::Arc<std::sync::Mutex<Vec<ControlRule>>>,
    requests: std::sync::Arc<std::sync::Mutex<Vec<DelegationRequest>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn configure_proxy_stream(stream: &std::os::unix::net::UnixStream) -> std::io::Result<bool> {
    match stream.set_read_timeout(Some(Duration::from_secs(30))) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {
            use std::os::fd::AsRawFd;
            let mut byte = 0u8;
            // macOS rejects SO_RCVTIMEO after peer close; skip only proven EOF.
            let peek = unsafe {
                libc::recv(
                    stream.as_raw_fd(),
                    (&mut byte as *mut u8).cast(),
                    1,
                    libc::MSG_PEEK | libc::MSG_DONTWAIT,
                )
            };
            let peek_errno = if peek < 0 {
                std::io::Error::last_os_error().raw_os_error()
            } else {
                None
            };
            let mut status = libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            };
            let poll = unsafe { libc::poll(&mut status, 1, 0) };
            eprintln!(
                "CONTROL_PROXY_TIMEOUT_SETUP error={:?} peek={} peek_errno={:?} poll={} revents={}",
                error.raw_os_error(),
                peek,
                peek_errno,
                poll,
                status.revents
            );
            if peek == 0 {
                Ok(false)
            } else if peek > 0 && poll == 1 && status.revents & libc::POLLHUP != 0 {
                // Closed peer's queued bytes remain finite; preserve normal frame handling.
                Ok(true)
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn control_proxy_closed_peer_does_not_prevent_the_next_valid_frame() {
    let (closed, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(peer);
    assert!(
        !configure_proxy_stream(&closed).unwrap(),
        "positively closed peer has no frame to route"
    );
    let (mut stream, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
    assert!(configure_proxy_stream(&stream).unwrap());
    let expected = ClientMessage {
        request_id: 1007,
        request: Request::Inspect,
    };
    write_frame(&mut peer, &expected).unwrap();
    let actual: ClientMessage = read_frame(&mut stream).unwrap();
    assert_eq!(actual.request_id, expected.request_id);
    assert!(matches!(actual.request, Request::Inspect));
    let (mut queued, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
    write_frame(&mut peer, &expected).unwrap();
    drop(peer);
    assert!(
        configure_proxy_stream(&queued).unwrap(),
        "buffered complete frame must remain processable after peer close"
    );
    let actual: ClientMessage = read_frame(&mut queued).unwrap();
    assert_eq!(actual.request_id, expected.request_id);
    assert!(matches!(actual.request, Request::Inspect));
    assert!(
        read_frame::<ClientMessage>(&mut queued).is_err(),
        "positively closed peer reaches EOF after its original queued frame"
    );
}

impl ControlProxy {
    fn new(
        path: std::path::PathBuf,
        server: std::path::PathBuf,
        fault: std::path::PathBuf,
    ) -> Self {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let rules = Arc::new(Mutex::new(Vec::<ControlRule>::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_rules = rules.clone();
        let worker_requests = requests.clone();
        let worker_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if worker_stop.load(Ordering::Relaxed) {
                    break;
                }
                let mut stream = stream.unwrap();
                if !configure_proxy_stream(&stream).unwrap() {
                    continue;
                }
                if exchange_preamble(&mut stream).is_err() {
                    continue;
                }
                let mut message: ClientMessage = read_frame(&mut stream).unwrap();
                // This serial fault proxy tests mutation ordering. Keep observer reads
                // nonblocking here; dedicated observation tests use the real wait path.
                if let Request::Delegation(request) = &mut message.request
                    && let DelegationRequest::Observe { wait_ms, .. } = request.as_mut()
                {
                    *wait_ms = 0;
                }
                if matches!(&message.request, Request::Workflow(_)) {
                    let declaration = matches!(&message.request, Request::Workflow(request) if matches!(request.as_ref(), WorkflowRequest::DeclareDelivery { .. }));
                    let rule = if declaration {
                        let mut rules = worker_rules.lock().unwrap();
                        rules
                            .iter()
                            .position(|r| r.operation == "DeclareDelivery")
                            .map(|index| rules.remove(index))
                    } else {
                        None
                    };
                    if rule.as_ref().is_some_and(|r| r.loss == 0) {
                        continue;
                    }
                    let mut upstream = connect_server(&server).unwrap();
                    upstream
                        .set_read_timeout(Some(Duration::from_secs(30)))
                        .unwrap();
                    let response =
                        client::request(&mut upstream, message.request_id, message.request)
                            .unwrap();
                    if !rule.as_ref().is_some_and(|r| r.loss == 1) {
                        let _ = write_frame(
                            &mut stream,
                            &ServerMessage::Response {
                                request_id: message.request_id,
                                response,
                            },
                        );
                    }
                    continue;
                }
                let Request::Delegation(request) = &message.request else {
                    panic!("only delegation uses proxy")
                };
                let (operation, handle) = match request.as_ref() {
                    DelegationRequest::Prepare { contract, .. } => {
                        ("Prepare", contract.request.clone())
                    }
                    DelegationRequest::Launch { handle, .. } => ("Launch", handle.clone()),
                    DelegationRequest::SendInitial { handle, .. } => {
                        ("InitialInput", handle.clone())
                    }
                    DelegationRequest::Reconcile { request, .. } => ("Reconcile", request.clone()),
                    DelegationRequest::Inspect { handle, .. } => ("Inspect", handle.clone()),
                    DelegationRequest::Collect { handle, .. } => ("Collect", handle.clone()),
                    DelegationRequest::Publish { handle, .. } => ("Publish", handle.clone()),
                    DelegationRequest::PublishQuestion { handle, .. } => {
                        ("PublishQuestion", handle.clone())
                    }
                    DelegationRequest::CollectQuestion { handle, .. } => {
                        ("CollectQuestion", handle.clone())
                    }
                    DelegationRequest::AnswerQuestion { answer, .. } => {
                        ("AnswerQuestion", answer.request.clone())
                    }
                    DelegationRequest::Followup { request, .. } => ("Followup", request.clone()),
                    DelegationRequest::CollectVersion { handle, .. } => {
                        ("CollectVersion", handle.clone())
                    }
                    DelegationRequest::ConfirmHumanReturn { request, .. } => {
                        ("ConfirmHumanReturn", request.clone())
                    }
                    DelegationRequest::RecordAcceptance { handle, .. } => {
                        ("RecordAcceptance", handle.clone())
                    }
                    DelegationRequest::Observe { .. } => ("Observe", String::new()),
                    DelegationRequest::MarkHandled { handle, .. } => {
                        ("MarkHandled", handle.clone())
                    }
                    DelegationRequest::Capabilities { .. } => ("Capabilities", String::new()),
                    DelegationRequest::Archive { archive, .. } => {
                        ("Archive", archive.request.clone())
                    }
                    DelegationRequest::Interrupt { handle, .. } => ("Interrupt", handle.clone()),
                    DelegationRequest::ConfirmInterrupt { handle, .. } => {
                        ("ConfirmInterrupt", handle.clone())
                    }
                };
                worker_requests
                    .lock()
                    .unwrap()
                    .push(request.as_ref().clone());
                let rule = {
                    let mut rules = worker_rules.lock().unwrap();
                    rules
                        .iter()
                        .position(|rule| rule.operation == operation)
                        .map(|index| rules.remove(index))
                };
                if rule.as_ref().is_some_and(|rule| rule.loss == 0) {
                    continue;
                }
                if let Some((point, value)) = rule.as_ref().and_then(|rule| rule.fault) {
                    if point == "await-rebind" {
                        wait_file(&fault.with_extension("rebind.done"));
                    } else {
                        if point == "input-target-captured" || point == "input-before-write" {
                            let identity = match request.as_ref() {
                                DelegationRequest::SendInitial { identity, .. } => identity,
                                DelegationRequest::Followup {
                                    expected_identity, ..
                                } => expected_identity,
                                _ => unreachable!(),
                            };
                            let deadline = Instant::now() + Duration::from_secs(10);
                            loop {
                                let mut control = connect_server(&server).unwrap();
                                let Response::Inventory { sessions, .. } =
                                    client::request(&mut control, 1, Request::Inspect).unwrap()
                                else {
                                    panic!("inventory")
                                };
                                if sessions
                                    .iter()
                                    .find(|s| Some(s.id) == identity.session)
                                    .and_then(|s| s.agent.as_ref())
                                    .is_some_and(|a| {
                                        a.health.state == ReporterHealth::Connected
                                            && (point != "input-before-write"
                                                || a.metrics
                                                    .as_ref()
                                                    .and_then(|m| m.sample.model.as_deref())
                                                    == Some("fixture-model"))
                                    })
                                {
                                    break;
                                }
                                assert!(
                                    Instant::now() < deadline,
                                    "child binding not ready at fault boundary"
                                );
                                std::thread::sleep(Duration::from_millis(5));
                            }
                        }
                        fs::write(&fault, format!("{point}|{handle}|{value}")).unwrap();
                    }
                }
                let mut upstream = connect_server(&server).unwrap();
                upstream
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                let response =
                    client::request(&mut upstream, message.request_id, message.request).unwrap();
                if rule.as_ref().is_some_and(|rule| rule.loss == 1) {
                    continue;
                }
                let _ = write_frame(
                    &mut stream,
                    &ServerMessage::Response {
                        request_id: message.request_id,
                        response,
                    },
                );
            }
        });
        Self {
            path,
            rules,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn arm(&self, rule: ControlRule) {
        self.rules.lock().unwrap().push(rule);
    }

    fn original(&self, operation: DispatchOperation) -> DelegationRequest {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| {
                matches!(
                    (operation, request),
                    (
                        DispatchOperation::Prepare,
                        DelegationRequest::Prepare { .. }
                    ) | (DispatchOperation::Launch, DelegationRequest::Launch { .. })
                        | (
                            DispatchOperation::InitialInput,
                            DelegationRequest::SendInitial { .. }
                        )
                )
            })
            .unwrap()
            .clone()
    }
}

impl ControlProxy {
    fn retire(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = std::os::unix::net::UnixStream::connect(&self.path);
            thread.join().unwrap();
        }
    }
}
impl Drop for ControlProxy {
    fn drop(&mut self) {
        self.retire();
    }
}

struct RecoveryFixture {
    proxy: ControlProxy,
    live: Live,
    parent: SessionId,
    workspace: String,
    files: std::cell::Cell<u32>,
    bytes: std::path::PathBuf,
}

impl RecoveryFixture {
    fn new() -> Self {
        Self::with_model(None)
    }

    fn with_model(initial_model: Option<&str>) -> Self {
        Self::with_model_gate(initial_model, false)
    }

    fn with_model_gate(initial_model: Option<&str>, gated: bool) -> Self {
        let live = Live::idle().bounded();
        let fault = live.root.path().join("fault");
        let proxy = ControlProxy::new(
            live.root.path().join("proxy.sock"),
            live.socket.clone(),
            fault.clone(),
        );
        let gate = live.root.path().join("allow-result");
        let bytes = live.root.path().join("input-bytes");
        fs::create_dir(&bytes).unwrap();
        let history = live.root.path().join("history");
        fs::create_dir(&history).unwrap();
        fs::create_dir_all(live.root.path().join(".pi/agent")).unwrap();
        let bin = live.claude.command.parent().unwrap();
        let pi = bin.join("pi");
        fs::write(&pi, include_bytes!("fixtures/pi/research_pi.sh")).unwrap();
        fs::set_permissions(&pi, fs::Permissions::from_mode(0o700)).unwrap();
        let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
        let rebind = fault.with_extension("rebind");
        let mut environment = vec![
            ("OVRCR_TEST_DELEGATION", "1".as_ref()),
            ("OVRCR_TEST_PI_HOST", host.as_os_str()),
            (
                "OVRCR_TEST_CONTROL_BINARY",
                std::ffi::OsStr::new(env!("CARGO_BIN_EXE_ovrcr")),
            ),
            ("OVRCR_TEST_DELEGATION_RESULT_GATE", gate.as_os_str()),
            (
                "OVRCR_TEST_DELEGATION_CONTROL_SOCKET",
                proxy.path.as_os_str(),
            ),
            ("OVRCR_TEST_DELEGATION_FAULT", fault.as_os_str()),
            ("OVRCR_TEST_DELEGATION_BYTES", bytes.as_os_str()),
            ("OVRCR_TEST_HISTORY", history.as_os_str()),
        ];
        if let Some(model) = initial_model {
            environment.push(("OVRCR_TEST_DELEGATION_INITIAL_MODEL", model.as_ref()));
            environment.push(("OVRCR_TEST_DELEGATION_REBIND_GATE", rebind.as_os_str()));
        }
        let model_gate = live.root.path().join("allow-initial-model");
        if gated {
            fs::write(&model_gate, "initially ready").unwrap();
            environment.push(("OVRCR_TEST_DELEGATION_MODEL_GATE", model_gate.as_os_str()));
        }
        live.start_binary_env(&environment);
        live.ready("recovery");
        let workspace = cli_json(
            &live,
            &[
                "workspace",
                "get",
                "--project",
                PROJECT,
                "--branch",
                "recovery",
            ],
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let Response::CreatedSession(parent) =
            live.request(Request::CreateSession(CreateSessionRequest {
                project: PROJECT.into(),
                workspace: workspace.clone(),
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
            panic!("create parent")
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
                panic!("inventory")
            };
            if sessions
                .iter()
                .any(|s| s.id == parent.id && s.agent.is_some())
            {
                break;
            }
            assert!(Instant::now() < deadline, "parent binding");
            std::thread::sleep(Duration::from_millis(10));
        }
        Self {
            proxy,
            live,
            parent: parent.id,
            workspace,
            files: std::cell::Cell::new(0),
            bytes,
        }
    }

    fn tool(&self, name: &str, args: serde_json::Value) -> serde_json::Value {
        let id = self.files.get();
        self.files.set(id + 1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tool(
                &self.live,
                self.parent,
                name,
                args,
                &self.live.root.path().join(format!("reply-{id}.json")),
            )
        }));
        match result {
            Ok(result) => result,
            Err(panic) => {
                let db =
                    rusqlite::Connection::open(self.live.config.join("registry.sqlite3")).unwrap();
                let mut query = db.prepare("SELECT metadata FROM delegations").unwrap();
                for raw in query.query_map([], |row| row.get::<_, String>(0)).unwrap() {
                    let value: serde_json::Value = serde_json::from_str(&raw.unwrap()).unwrap();
                    eprintln!(
                        "pending dispatch={} binding={} receipts={}",
                        value["dispatch"], value["binding"], value["receipts"]
                    );
                }
                std::panic::resume_unwind(panic)
            }
        }
    }
    fn start(&self) -> serde_json::Value {
        self.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":self.workspace,"scope":"Read README", "model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}))
    }
    fn recover(&self, handle: &str) -> serde_json::Value {
        self.tool("ovrcr_start", serde_json::json!({"handle":handle}))
    }
    fn control(&self, request: DelegationRequest) -> AssignmentSnapshot {
        match self.live.request(Request::Delegation(Box::new(request))) {
            Response::Delegation(snapshot) => *snapshot,
            response => panic!("unexpected control response: {response:?}"),
        }
    }
    fn reconcile(&self, handle: &str) -> AssignmentSnapshot {
        let DelegationRequest::Prepare { auth, .. } =
            self.proxy.original(DispatchOperation::Prepare)
        else {
            unreachable!()
        };
        self.control(DelegationRequest::Reconcile {
            auth,
            request: handle.into(),
        })
    }
    fn input_count(&self, snapshot: &AssignmentSnapshot) -> usize {
        fs::read_to_string(snapshot.brief.with_extension("json.input-count"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
    fn byte_count(&self, snapshot: &AssignmentSnapshot) -> u64 {
        fs::read_to_string(
            self.bytes
                .join(snapshot.identity.session.unwrap().0.to_string()),
        )
        .map(|s| s.lines().map(|v| v.parse::<u64>().unwrap()).sum())
        .unwrap_or(0)
    }
    fn child_count(&self) -> usize {
        let Response::Inventory { sessions, .. } = self.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        sessions
            .iter()
            .filter(|s| s.kind == SessionKind::Agent { name: "pi".into() } && s.id != self.parent)
            .count()
    }
}

#[test]
fn public_retry_preserves_spent_attempt_while_initial_model_is_withheld() {
    let fixture = RecoveryFixture::with_model_gate(None, true);
    let output = fixture
        .live
        .command()
        .args(["settings", "set", "delegation.max_live_children", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    assert_eq!(first.dispatch, DispatchStatus::Applied);
    fs::remove_file(fixture.live.root.path().join("allow-initial-model")).unwrap();
    let refused: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    assert_eq!(refused.dispatch, DispatchStatus::Refused);
    assert!(refused.retryable_capacity_refusal);
    assert_eq!(refused.execution_attempts, Some(0));
    assert_eq!(
        fixture.live.request(Request::CloseTerminal {
            session: first.identity.session.unwrap(),
            expected_run: first.identity.run.unwrap()
        }),
        Response::Ok
    );

    let waiting: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_retry", serde_json::json!({"handle":refused.handle})),
    )
    .unwrap();
    assert_eq!(waiting.dispatch, DispatchStatus::Launched);
    assert_eq!(waiting.identity.task, refused.identity.task);
    assert_eq!(waiting.identity.attempt, refused.identity.attempt + 1);
    let row = || {
        let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
        let raw: String = db
            .query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [&waiting.handle],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str::<serde_json::Value>(&raw).unwrap()
    };
    let pending = row();
    let binding = pending["binding"].clone();
    assert_eq!(binding["generation"], 1);
    let initial = waiting
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::InitialInput)
        .unwrap();
    assert_eq!(initial.outcome, DispatchOutcome::Accepted);
    let requests = fixture.proxy.requests.lock().unwrap();
    assert!(requests.iter().filter(|request| matches!(request, DelegationRequest::SendInitial { handle, .. } if handle == &waiting.handle)).count() >= 2);
    drop(requests);
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let child = sessions
        .iter()
        .find(|s| Some(s.id) == waiting.identity.session)
        .unwrap();
    assert_eq!(
        child.agent.as_ref().unwrap().binding,
        serde_json::from_value(binding.clone()).unwrap()
    );
    assert!(child.agent.as_ref().unwrap().metrics.is_none());
    assert_eq!(fixture.byte_count(&waiting), 0);
    assert_eq!(fixture.input_count(&waiting), 0);
    eprintln!(
        "MODEL_GATE_TRACE waiting={} durable_count={} binding={} initial={:?}",
        waiting
            .execution_attempts
            .map_or("unknown".into(), |n| n.to_string()),
        pending["execution_attempts"],
        binding,
        initial.outcome
    );
    assert_eq!(
        waiting.execution_attempts,
        Some(1),
        "a captured binding awaiting native model metrics is still exactly one spent attempt"
    );
    assert_eq!(pending["execution_attempts"], 1);
    let reconciled = fixture.reconcile(&waiting.handle);
    assert_eq!(reconciled.execution_attempts, Some(1));
    let DelegationRequest::Prepare { auth, .. } =
        fixture.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    let inspected_control = fixture.control(DelegationRequest::Inspect {
        auth,
        handle: waiting.handle.clone(),
    });
    assert_eq!(inspected_control.execution_attempts, Some(1));
    assert_eq!(inspected_control.receipts, waiting.receipts);
    let inspected: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":waiting.handle}),
    ))
    .unwrap();
    assert_eq!(inspected.execution_attempts, Some(1));
    assert_eq!(inspected.receipts, waiting.receipts);
    assert_eq!(row()["execution_attempts"], 1);
    assert_eq!(
        fixture.child_count(),
        2,
        "one stopped predecessor workload and one retry child"
    );
    let gate = fixture.live.root.path().join("allow-initial-model");
    fs::write(&gate, "publish model").unwrap();
    wait_file(&gate.with_extension("reported"));
    let applied: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_retry", serde_json::json!({"handle":refused.handle})),
    )
    .unwrap();
    assert_eq!(applied.dispatch, DispatchStatus::Applied);
    assert_eq!(applied.identity, waiting.identity);
    assert_eq!(applied.execution_attempts, Some(1));
    assert_eq!(row()["binding"], binding);
    assert_eq!(row()["execution_attempts"], 1);
    wait_file(&applied.brief.with_extension("json.input-count"));
    assert_eq!(fixture.input_count(&applied), 1);
    let initial_applied = applied
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::InitialInput)
        .unwrap();
    assert_eq!(initial_applied.request, initial.request);
    assert_eq!(initial_applied.identity, initial.identity);
    assert_eq!(initial_applied.result_version, initial.result_version);
    assert_eq!(initial_applied.outcome, DispatchOutcome::Applied);
    for operation in [DispatchOperation::Prepare, DispatchOperation::Launch] {
        assert_eq!(
            applied.receipts.iter().find(|r| r.operation == operation),
            waiting.receipts.iter().find(|r| r.operation == operation)
        );
    }
    assert_eq!(row()["request_digests"], pending["request_digests"]);
    let count = fixture.byte_count(&applied);
    let repeated: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_retry", serde_json::json!({"handle":refused.handle})),
    )
    .unwrap();
    assert_eq!(repeated.identity, applied.identity);
    assert_eq!(repeated.receipts, applied.receipts);
    assert_eq!(repeated.execution_attempts, Some(1));
    assert_eq!(fixture.byte_count(&repeated), count);
    assert_eq!(fixture.input_count(&repeated), 1);
    assert_eq!(fixture.child_count(), 2);
    eprintln!(
        "MODEL_GATE_TRACE final=Applied count=1 duplicate_input=false binding_unchanged=true receipts={}",
        serde_json::to_string(&applied.receipts).unwrap()
    );
}

#[test]
fn public_legacy_accounting_migration_preserves_effects_and_original_contract_bytes() {
    let fixture = RecoveryFixture::new();
    let set = fixture
        .live
        .command()
        .args(["settings", "set", "delegation.max_execution_attempts", "1"])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    fixture.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 0,
        fault: None,
    });
    let first = fixture.start();
    assert_eq!(first["dispatch"], "Unknown");
    let handle = first["handle"].as_str().unwrap();
    assert_eq!(fixture.child_count(), 0);
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let metadata: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [handle],
            |row| row.get(0),
        )
        .unwrap();
    let original: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    let contract_path = Path::new(original["root"].as_str().unwrap()).join("contract-v1.json");
    let original_contract = fs::read(&contract_path).unwrap();
    let save = |value: &serde_json::Value| {
        db.execute(
            "UPDATE delegations SET session=NULL,metadata=?1 WHERE handle=?2",
            rusqlite::params![value.to_string(), handle],
        )
        .unwrap();
    };
    let legacy = || {
        let mut value = original.clone();
        for key in [
            "execution_attempts",
            "receipts",
            "request_digests",
            "project",
            "workspace",
            "previous_request",
            "successor_request",
            "retryable_capacity_refusal",
        ] {
            value.as_object_mut().unwrap().remove(key);
        }
        value
    };
    let launch_receipt = |value: &mut serde_json::Value, outcome: &str| {
        let mut identity = value["identity"].clone();
        identity["session"] = serde_json::Value::Null;
        identity["run"] = serde_json::Value::Null;
        value["receipts"] = serde_json::json!([
            {"request":handle,"operation":"Prepare","identity":identity,"outcome":"Applied","input":null,"reason":null},
            {"request":handle,"operation":"Launch","identity":identity,"outcome":outcome,"input":null,"reason":null}
        ]);
    };
    for (name, expected) in [
        ("legacy launch unknown without resource", Some(1)),
        ("legacy refusal without proof", None),
        ("legacy input refusal after launch", Some(1)),
        ("legacy applied without resource", Some(1)),
        ("zero contradicts unknown receipt", None),
        ("prepared contradicts unknown receipt", None),
        ("unknown predecessor history", None),
        ("known count survives missing resource", Some(2)),
    ] {
        let mut value = legacy();
        match name {
            "legacy launch unknown without resource" => value["dispatch"] = "LaunchUnknown".into(),
            "legacy refusal without proof" => value["dispatch"] = "Refused".into(),
            "legacy input refusal after launch" | "legacy applied without resource" => {
                value["dispatch"] = if name.contains("refusal") {
                    "Refused"
                } else {
                    "Applied"
                }
                .into();
                value["identity"]["session"] = 100_000.into();
                value["identity"]["run"] = 1.into();
                value["binding"] = serde_json::json!({"provider":"Pi","invocation":"legacy-child","conversation":"legacy-A","generation":1});
            }
            "zero contradicts unknown receipt" | "prepared contradicts unknown receipt" => {
                launch_receipt(&mut value, "Unknown");
                if name.starts_with("zero") {
                    value["dispatch"] = "LaunchUnknown".into();
                    value["execution_attempts"] = 0.into();
                }
            }
            "unknown predecessor history" => {
                value["dispatch"] = "LaunchUnknown".into();
                value["identity"]["attempt"] = 2.into();
                value["previous_request"] = "missing-predecessor".into();
            }
            "known count survives missing resource" => {
                value["dispatch"] = "Refused".into();
                value["execution_attempts"] = 2.into();
            }
            _ => unreachable!(),
        }
        save(&value);
        let migrated = fixture.reconcile(handle);
        assert_eq!(migrated.execution_attempts, expected, "{name}");
        assert!(migrated.resources.current_session.is_none(), "{name}");
        let recovered = fixture.recover(handle);
        assert_eq!(
            recovered["execution_attempts"],
            serde_json::to_value(expected).unwrap(),
            "{name}: {recovered}"
        );
        let repeated = fixture.reconcile(handle);
        assert_eq!(repeated.execution_attempts, expected, "{name}");
        assert_eq!(repeated.identity, migrated.identity, "{name}");
        assert_eq!(repeated.receipts, migrated.receipts, "{name}");
        let retry = fixture.tool("ovrcr_retry", serde_json::json!({"handle":handle}));
        assert_eq!(retry["dispatch"], "Refused", "{name}");
        assert_eq!(
            fixture.child_count(),
            0,
            "legacy migration must not invent a retry: {name}"
        );
    }
    // Reconstruct the actual pre-#327 canonical document. This tests binary
    // migration of immutable bytes, not merely a new contract with default fields.
    let modern = std::str::from_utf8(&original_contract).unwrap();
    let boundary = modern.rfind(",\"limits\":").unwrap();
    let old_bytes = format!("{}}}", &modern[..boundary]).into_bytes();
    fs::write(&contract_path, &old_bytes).unwrap();
    let hash = std::process::Command::new("node").args(["--input-type=module", "-e", "import {createHash} from 'node:crypto';import {readFileSync} from 'node:fs';console.log(createHash('sha256').update(readFileSync(process.argv[1])).digest('hex'));"]).arg(&contract_path).output().unwrap();
    assert!(hash.status.success());
    let mut value = legacy();
    value["contract_digest"] = String::from_utf8(hash.stdout).unwrap().trim().into();
    save(&value);
    let intent = fixture
        .live
        .config
        .join("parent-intents")
        .join(format!("{handle}.json"));
    fs::write(&intent, &old_bytes).unwrap();
    let prepared = fixture.reconcile(handle);
    assert_eq!(prepared.execution_attempts, Some(0));
    let started: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied);
    assert_eq!(started.execution_attempts, Some(1));
    assert_eq!(
        fs::read(&contract_path).unwrap(),
        old_bytes,
        "migration must retain exact original contract bytes"
    );
    wait_file(&started.brief.with_extension("json.started"));
    assert_eq!(fixture.input_count(&started), 1);
    assert_eq!(fixture.child_count(), 1);
    for changed in ["scope", "nondefault limit"] {
        let mut contract: serde_json::Value = serde_json::from_slice(&old_bytes).unwrap();
        if changed == "scope" {
            contract["scope"] = "Changed task".into();
        } else {
            contract["limits"] = serde_json::json!({"max_execution_attempts":1});
        }
        fs::write(&intent, serde_json::to_vec(&contract).unwrap()).unwrap();
        let refused = fixture.recover(handle);
        assert_eq!(refused["dispatch"], "Refused", "{changed}: {refused}");
        assert_eq!(fixture.reconcile(handle).execution_attempts, Some(1));
        assert_eq!(fixture.input_count(&started), 1);
        assert_eq!(fs::read(&contract_path).unwrap(), old_bytes);
    }
    fs::write(&intent, &old_bytes).unwrap();
    let same: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
    assert_eq!(same.identity, started.identity);
    assert_eq!(same.receipts, started.receipts);
    assert_eq!(same.execution_attempts, Some(1));
    assert_eq!(fixture.input_count(&same), 1);

    // A clarification must reuse the old contract path and bytes. Recovering
    // the initial request after that version advance still uses its first digest.
    let current = followups::complete(&fixture, &same);
    let clarified: AssignmentSnapshot =
        serde_json::from_value(followups::followup(&fixture, &current, None)).unwrap();
    assert_eq!(clarified.result_version, 2);
    assert_eq!(clarified.execution_attempts, Some(1));
    let clarified = followups::complete(&fixture, &clarified);
    let replayed: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
    assert_eq!(replayed.result_version, 2);
    assert_eq!(replayed.execution_attempts, Some(1));
    assert_eq!(fs::read(&contract_path).unwrap(), old_bytes);

    let mut revised: ResearchContract = serde_json::from_slice(&old_bytes).unwrap();
    revised.scope = "Clarify the original README citation.".into();
    fixture.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 2,
        fault: Some(("followup-after-files", 0)),
    });
    let pending: AssignmentSnapshot =
        serde_json::from_value(followups::followup(&fixture, &clarified, Some(revised))).unwrap();
    assert_eq!(pending.result_version, 2);
    let request = pending.receipts.last().unwrap().request.clone();
    let private = fixture
        .live
        .config
        .join("parent-intents")
        .join(format!("followup-{request}.json"));
    let mut saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&private).unwrap()).unwrap();
    saved["revised_contract"]
        .as_object_mut()
        .unwrap()
        .remove("limits");
    saved["revised_contract"]
        .as_object_mut()
        .unwrap()
        .remove("previous_request");
    fs::write(&private, serde_json::to_vec(&saved).unwrap()).unwrap();
    let root = contract_path.parent().unwrap();
    let contract_v2 = root.join("contract-v2.json");
    let modern = fs::read_to_string(&contract_v2).unwrap();
    let boundary = modern.rfind(",\"limits\":").unwrap();
    let legacy_revision = format!("{}}}", &modern[..boundary]).into_bytes();
    fs::write(&contract_v2, &legacy_revision).unwrap();
    let brief_v3 = root.join("brief-v3.json");
    let mut brief: serde_json::Value =
        serde_json::from_slice(&fs::read(&brief_v3).unwrap()).unwrap();
    brief["contract"].as_object_mut().unwrap().remove("limits");
    brief["contract"]
        .as_object_mut()
        .unwrap()
        .remove("previous_request");
    let legacy_brief = serde_json::to_vec_pretty(&brief).unwrap();
    fs::write(&brief_v3, &legacy_brief).unwrap();
    let mut payload = serde_json::to_vec(&(
        &pending.identity,
        pending.result_version,
        saved["expected_control_epoch"].as_u64().unwrap(),
        saved["text"].as_str().unwrap(),
    ))
    .unwrap();
    payload.pop();
    payload.push(b',');
    payload.extend_from_slice(&legacy_revision);
    payload.push(b']');
    let payload_path = root.join(format!("followup-{request}.json"));
    fs::write(&payload_path, &payload).unwrap();
    let hash = |path: &Path| {
        let output = std::process::Command::new("node").args(["--input-type=module", "-e", "import {createHash} from 'node:crypto';import {readFileSync} from 'node:fs';console.log(createHash('sha256').update(readFileSync(process.argv[1])).digest('hex'));"]).arg(path).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let metadata: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [handle],
            |row| row.get(0),
        )
        .unwrap();
    let mut record: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    let last = record["followups"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["digest"] = hash(&payload_path).into();
    last["contract_digest"] = hash(&contract_v2).into();
    last["brief_digest"] = hash(&brief_v3).into();
    db.execute(
        "UPDATE delegations SET metadata=?1 WHERE handle=?2",
        rusqlite::params![record.to_string(), handle],
    )
    .unwrap();
    let recovered: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_followup",
        serde_json::json!({"handle":handle,"request":request}),
    ))
    .unwrap();
    assert_eq!(recovered.result_version, 3);
    assert_eq!(recovered.identity.contract, 2);
    assert_eq!(recovered.execution_attempts, Some(1));
    assert_eq!(fixture.child_count(), 1);
    let completed = followups::complete(&fixture, &recovered);
    assert_eq!(completed.execution_attempts, Some(1));
    assert_eq!(fixture.input_count(&completed), 1);
    assert_eq!(fs::read(&contract_path).unwrap(), old_bytes);
    assert_eq!(fs::read(&contract_v2).unwrap(), legacy_revision);
    assert_eq!(fs::read(&brief_v3).unwrap(), legacy_brief);
    let again: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_followup",
        serde_json::json!({"handle":handle,"request":request}),
    ))
    .unwrap();
    assert_eq!(again.receipts, completed.receipts);
    assert_eq!(again.execution_attempts, Some(1));
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dispatch_freezes_fresh_child_target_before_metrics_and_write() {
    for stage in ["before-first", "after-capture", "before-write"] {
        let fixture = RecoveryFixture::with_model(Some(if stage == "after-capture" {
            "waiting-model"
        } else {
            "fixture-model"
        }));
        let gate = fixture.live.root.path().join("fault.rebind");
        let release = fixture.live.root.path().join("fault.release");
        struct Release(std::path::PathBuf);
        impl Drop for Release {
            fn drop(&mut self) {
                let _ = fs::write(&self.0, "release");
            }
        }
        let _release = Release(release.clone());
        let point = match stage {
            "before-first" => "await-rebind",
            "after-capture" => "input-target-captured",
            _ => "input-before-write",
        };
        fixture.proxy.arm(ControlRule {
            operation: "InitialInput",
            loss: if stage == "after-capture" { 1 } else { 2 },
            fault: Some((point, 0)),
        });
        if stage == "before-first" {
            fs::write(&gate, "rebind").unwrap();
        }
        let output = fixture.live.root.path().join("gated-start.json");
        let first = if stage == "before-write" {
            let args = serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Read README", "model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]});
            assert_eq!(
                fixture.live.request(Request::SendTerminal {
                    session: fixture.parent,
                    text: format!(
                        "tool:{}",
                        serde_json::json!({"name":"ovrcr_start","args":args,"output":output})
                    ),
                    submit: true
                }),
                Response::Ok
            );
            wait_file(&fixture.live.root.path().join("fault.hit"));
            serde_json::Value::Null
        } else {
            fixture.start()
        };
        let DelegationRequest::Prepare { contract, .. } =
            fixture.proxy.original(DispatchOperation::Prepare)
        else {
            unreachable!()
        };
        let handle = contract.request;
        if stage != "before-first" {
            let db =
                rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
            let metadata: String = db
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle=?1",
                    [&handle],
                    |row| row.get(0),
                )
                .unwrap();
            let record: serde_json::Value = serde_json::from_str(&metadata).unwrap();
            assert_eq!(
                record["binding"]["generation"], 1,
                "target must be frozen before waiting"
            );
            if stage == "after-capture" {
                assert_eq!(first["dispatch"], "Unknown");
                let saved = fixture.reconcile(&handle);
                let mut wrong_epoch = fixture.proxy.original(DispatchOperation::InitialInput);
                if let DelegationRequest::SendInitial {
                    expected_control_epoch,
                    ..
                } = &mut wrong_epoch
                {
                    *expected_control_epoch += 1;
                } else {
                    unreachable!()
                }
                assert!(matches!(
                    fixture
                        .live
                        .request(Request::Delegation(Box::new(wrong_epoch))),
                    Response::Error {
                        code: ErrorCode::Conflict,
                        ..
                    }
                ));
                assert_eq!(
                    fixture.reconcile(&handle).receipts,
                    saved.receipts,
                    "caller-only wrong epoch cannot terminalize a still-authorized original intent"
                );
                assert_eq!(
                    saved
                        .receipts
                        .iter()
                        .find(|r| r.operation == DispatchOperation::InitialInput)
                        .unwrap()
                        .outcome,
                    DispatchOutcome::Accepted
                );
            }
            fs::write(&gate, "rebind").unwrap();
        }
        wait_file(&gate.with_extension("rebind.done"));
        let DelegationRequest::SendInitial { identity, .. } =
            fixture.proxy.original(DispatchOperation::InitialInput)
        else {
            unreachable!()
        };
        let child = identity.session.unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect)
            else {
                panic!("inventory")
            };
            let binding = sessions
                .iter()
                .find(|s| s.id == child)
                .unwrap()
                .agent
                .as_ref()
                .unwrap()
                .binding
                .clone();
            if binding.generation == 3 {
                assert!(binding.conversation.starts_with("fixture-"));
                break;
            }
            assert!(Instant::now() < deadline, "child A→B→A not observed");
            std::thread::sleep(Duration::from_millis(5));
        }
        let rejected = if stage == "before-write" {
            fs::write(&release, "release").unwrap();
            wait_file(&output);
            serde_json::from_slice::<serde_json::Value>(&fs::read(&output).unwrap()).unwrap()["details"].clone()
        } else if stage == "after-capture" {
            fixture.recover(&handle)
        } else {
            first
        };
        assert_eq!(rejected["dispatch"], "Refused", "{stage}: {rejected}");
        let current = fixture.reconcile(&handle);
        println!(
            "INITIAL_FENCE stage={stage} epoch={} input={} bytes={}",
            current.control.epoch,
            fixture.input_count(&current),
            fixture.byte_count(&current)
        );
        assert_eq!(fixture.input_count(&current), 0);
        assert_eq!(fixture.byte_count(&current), 0);
        assert_eq!(
            current
                .receipts
                .iter()
                .find(|r| r.operation == DispatchOperation::InitialInput)
                .unwrap()
                .outcome,
            DispatchOutcome::Rejected
        );
        assert_eq!(fixture.input_count(&current), 0);
        assert_eq!(fixture.byte_count(&current), 0);
    }
}

impl Drop for RecoveryFixture {
    fn drop(&mut self) {
        if self.live.hosted() {
            for group in self.live.session_groups() {
                self.live.own_group(group);
            }
        }
    }
}

#[test]
fn public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects() {
    for operation in ["Prepare", "Launch", "InitialInput"] {
        for loss in [0, 1] {
            let fixture = RecoveryFixture::new();
            fixture.proxy.arm(ControlRule {
                operation,
                loss,
                fault: None,
            });
            let first = fixture.start();
            assert_eq!(first["dispatch"], "Unknown", "{operation}/{loss}: {first}");
            assert!(
                first.get("receipts").is_none(),
                "a lost reply must not fabricate Server receipts"
            );
            let handle = first["handle"].as_str().unwrap();
            let recovered = fixture.recover(handle);
            assert_eq!(
                recovered["dispatch"], "Applied",
                "{operation}/{loss}: {recovered}"
            );
            let recovered: AssignmentSnapshot = serde_json::from_value(recovered).unwrap();
            wait_file(&recovered.brief.with_extension("json.started"));
            assert_eq!(fixture.input_count(&recovered), 1);
            assert_eq!(fixture.child_count(), 1);
            assert!(
                recovered
                    .receipts
                    .iter()
                    .all(|r| r.outcome == DispatchOutcome::Applied)
            );
            // Replay the exact original pre-allocation Launch and input directly.
            for kind in [DispatchOperation::Launch, DispatchOperation::InitialInput] {
                let same = fixture.control(fixture.proxy.original(kind));
                assert_eq!(same.identity, recovered.identity);
            }
            assert_eq!(fixture.input_count(&recovered), 1);
            assert_eq!(fixture.child_count(), 1);
            let mut conflicting = fixture.proxy.original(DispatchOperation::Prepare);
            let DelegationRequest::Prepare { contract, .. } = &mut conflicting else {
                unreachable!()
            };
            contract.scope.push_str(" changed");
            assert!(matches!(
                fixture
                    .live
                    .request(Request::Delegation(Box::new(conflicting))),
                Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                }
            ));
        }
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dispatch_partial_and_uncertain_input_is_never_replayed() {
    for (point, limit, expected) in [
        ("input", 0, DispatchOutcome::Rejected),
        ("input", 7, DispatchOutcome::Partial),
        ("input", usize::MAX, DispatchOutcome::Unknown),
        ("input-after-effect", 0, DispatchOutcome::Unknown),
    ] {
        eprintln!("input failure case {point}/{limit}");
        let fixture = RecoveryFixture::new();
        // usize::MAX injects an unconfirmed flush after all real bytes are written.
        fixture.proxy.arm(ControlRule {
            operation: "InitialInput",
            loss: 2,
            fault: Some((point, limit)),
        });
        let first = fixture.start();
        let handle = first["handle"].as_str().unwrap();
        let snapshot = fixture.reconcile(handle);
        let input = snapshot
            .receipts
            .iter()
            .find(|r| r.operation == DispatchOperation::InitialInput)
            .unwrap();
        assert_eq!(input.outcome, expected, "{point}/{limit}: {first}");
        if limit == usize::MAX || point == "input-after-effect" {
            wait_file(&snapshot.brief.with_extension("json.started"));
            assert_eq!(fixture.input_count(&snapshot), 1);
        }
        if limit == 7 {
            assert_eq!(input.input.as_ref().unwrap().written_bytes, 7);
            let deadline = Instant::now() + Duration::from_secs(3);
            while fixture.byte_count(&snapshot) < 7 {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(fixture.byte_count(&snapshot), 7);
            assert_eq!(fixture.input_count(&snapshot), 0);
        }
        let before = fixture.byte_count(&snapshot);
        let _ = fixture.recover(handle);
        let same = fixture.control(fixture.proxy.original(DispatchOperation::InitialInput));
        assert_eq!(same.receipts, snapshot.receipts);
        assert_eq!(fixture.byte_count(&snapshot), before);
        assert_eq!(fixture.child_count(), 1);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dispatch_reconciles_publication_and_launch_failure_boundaries() {
    for point in [
        "prepare-before-file",
        "prepare-after-file",
        "launch-before-effect",
        "launch-after-effect",
    ] {
        let fixture = RecoveryFixture::new();
        let prepare = point.starts_with("prepare");
        fixture.proxy.arm(ControlRule {
            operation: if prepare { "Prepare" } else { "Launch" },
            loss: 1,
            fault: Some((point, 0)),
        });
        let first = fixture.start();
        assert_eq!(first["dispatch"], "Unknown");
        let handle = first["handle"].as_str().unwrap();
        let durable = fixture.reconcile(handle);
        let operation = if prepare {
            DispatchOperation::Prepare
        } else {
            DispatchOperation::Launch
        };
        assert_eq!(
            durable
                .receipts
                .iter()
                .find(|r| r.operation == operation)
                .unwrap()
                .outcome,
            DispatchOutcome::Unknown
        );
        assert_eq!(
            fixture.child_count(),
            usize::from(point == "launch-after-effect")
        );
        let recovered = fixture.recover(handle);
        if prepare {
            assert_eq!(recovered["dispatch"], "Applied", "{point}: {recovered}");
            let recovered: AssignmentSnapshot = serde_json::from_value(recovered).unwrap();
            wait_file(&recovered.brief.with_extension("json.started"));
            assert_eq!(fixture.input_count(&recovered), 1);
        } else {
            assert_eq!(recovered["dispatch"], "LaunchUnknown");
            let same = fixture.control(fixture.proxy.original(DispatchOperation::Launch));
            assert_eq!(same.identity, durable.identity);
            assert_eq!(same.receipts, durable.receipts);
            assert_eq!(fixture.input_count(&same), 0);
            assert_eq!(
                same.resources.current_session.is_some(),
                point == "launch-after-effect"
            );
        }
        assert_eq!(
            fixture.child_count(),
            usize::from(prepare || point == "launch-after-effect")
        );
    }
}

#[test]
fn public_dispatch_recovery_preserves_receipts_when_result_is_corrupt_and_run_replaced() {
    let fixture = RecoveryFixture::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    wait_file(&started.brief.with_extension("json.started"));
    fs::write(fixture.live.root.path().join("allow-result"), "continue").unwrap();
    wait_file(&started.brief.with_extension("json.published"));
    let valid = fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":started.handle}),
    );
    assert!(valid["submission"].is_object());
    fs::write(&started.result_path, b"{malformed").unwrap();
    let recovered: AssignmentSnapshot =
        serde_json::from_value(fixture.recover(&started.handle)).unwrap();
    assert_eq!(recovered.receipts, started.receipts);
    assert_eq!(
        recovered.resources.current_session.as_ref().unwrap().run,
        started.identity.run.unwrap()
    );
    let strict = tool_reply(
        &fixture.live,
        fixture.parent,
        "ovrcr_collect",
        serde_json::json!({"handle":started.handle}),
        &fixture.live.root.path().join("corrupt-collect.json"),
    );
    assert!(
        strict["error"]
            .as_str()
            .unwrap()
            .contains("changed or is partial")
    );
    let child = started.identity.session.unwrap();
    assert_eq!(
        fixture
            .live
            .request(Request::KillSession { session: child }),
        Response::Ok
    );
    let reopened = fixture.live.request(Request::ReopenSession {
        session: child,
        expected_run: started.identity.run.unwrap(),
        acknowledge_stopped: false,
    });
    assert!(
        matches!(reopened, Response::CreatedSession(_)),
        "{reopened:?}"
    );
    let current = fixture.reconcile(&started.handle);
    assert_eq!(current.identity, started.identity);
    assert_ne!(
        current.resources.current_session.as_ref().unwrap().run,
        started.identity.run.unwrap()
    );
    assert_eq!(current.receipts, started.receipts);
    assert!(matches!(
        fixture.live.request(Request::Delegation(Box::new(
            fixture.proxy.original(DispatchOperation::InitialInput)
        ))),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
}

#[test]
fn public_dispatch_preprocess_refusal_preserves_workspace_and_rejects_conflicting_identity() {
    let fixture = RecoveryFixture::new();
    fixture.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 0,
        fault: None,
    });
    let first = fixture.start();
    let handle = first["handle"].as_str().unwrap();
    let row = cli_json(
        &fixture.live,
        &[
            "workspace",
            "get",
            "--project",
            PROJECT,
            "--branch",
            "recovery",
        ],
    );
    let workspace = std::path::PathBuf::from(row["path"].as_str().unwrap());
    let absent = workspace.with_extension("temporarily-unavailable");
    fs::rename(&workspace, &absent).unwrap();
    let refused = fixture.recover(handle);
    fs::rename(&absent, &workspace).unwrap();
    assert_eq!(refused["dispatch"], "Refused", "{refused}");
    let snapshot = fixture.reconcile(handle);
    assert!(snapshot.identity.session.is_none());
    assert!(snapshot.resources.workspace_present);
    assert_eq!(
        snapshot.resources.workspace.as_deref(),
        Some(fixture.workspace.as_str())
    );
    assert_eq!(
        snapshot
            .receipts
            .iter()
            .find(|r| r.operation == DispatchOperation::Launch)
            .unwrap()
            .outcome,
        DispatchOutcome::Rejected
    );
    let mut wrong = fixture.proxy.original(DispatchOperation::Launch);
    let DelegationRequest::Launch { identity, .. } = &mut wrong else {
        unreachable!()
    };
    identity.controller_epoch += 1;
    assert!(matches!(
        fixture.live.request(Request::Delegation(Box::new(wrong))),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    let _ = fixture.control(fixture.proxy.original(DispatchOperation::Launch));
    assert_eq!(fixture.child_count(), 0);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dispatch_storage_exhaustion_preserves_unresolved_obligations() {
    let fixture = RecoveryFixture::new();
    fixture.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 2,
        fault: Some(("launch-before-effect", 0)),
    });
    let original = fixture.start();
    let handle = original["handle"].as_str().unwrap();
    let unresolved = fixture.reconcile(handle);
    let mut db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let metadata: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [handle],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !metadata.contains("Read README"),
        "registry must contain correlation, not scope payload"
    );
    let transaction = db.transaction().unwrap();
    for index in 1..4096 {
        let mut value: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        let id = format!("capacity-{index}");
        value["handle"] = id.clone().into();
        value["identity"]["request"] = id.clone().into();
        transaction
            .execute(
                "INSERT INTO delegations(handle,workflow,session,metadata) VALUES(?1,?2,NULL,?3)",
                rusqlite::params![
                    id,
                    value["identity"]["workflow"].as_str().unwrap(),
                    value.to_string()
                ],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    let refused = fixture.start();
    assert_eq!(refused["dispatch"], "Refused", "{refused}");
    assert!(refused["reason"].as_str().unwrap().contains("4096"));
    assert!(
        !fixture
            .live
            .config
            .join("workflows")
            .join(refused["handle"].as_str().unwrap())
            .exists()
    );
    assert_eq!(fixture.child_count(), 0);
    let recovered = fixture.reconcile(handle);
    assert_eq!(recovered.receipts, unresolved.receipts);
    assert_eq!(recovered.identity, unresolved.identity);
    let count: i64 = db
        .query_row("SELECT count(*) FROM delegations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 4096);
}

#[test]
fn public_dispatch_root_drift_after_allocation_is_rejected_without_replay() {
    let fixture = RecoveryFixture::new();
    let root = cli_json(
        &fixture.live,
        &[
            "workspace",
            "get",
            "--project",
            PROJECT,
            "--path",
            fixture.live.repo.to_str().unwrap(),
        ],
    );
    let root_id = root["id"].as_str().unwrap();
    live::git(
        &fixture.live.repo,
        &["switch", "-c", "feature/dispatch-root-drift"],
    );
    let started = fixture.tool(
        "ovrcr_start",
        serde_json::json!({
            "project": PROJECT, "workspace": root_id, "scope": "Read README",
            "model_provider": "test", "model": "fixture-model", "acceptance_evidence": ["citation"],
        }),
    );
    let snapshot = fixture.reconcile(started["handle"].as_str().unwrap());
    assert!(
        snapshot.identity.session.is_some(),
        "the failure must occur after allocation"
    );
    assert_eq!(snapshot.identity.run, Some(SessionRunId(1)));
    assert!(
        snapshot.resources.current_session.is_none(),
        "the unstarted row is removed"
    );
    assert!(snapshot.resources.workspace_present);
    assert_eq!(snapshot.resources.workspace.as_deref(), Some(root_id));
    assert_eq!(fixture.child_count(), 0);
    assert_eq!(started["dispatch"], "Refused", "{started}");
    let receipt = snapshot
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::Launch)
        .unwrap();
    assert_eq!(receipt.outcome, DispatchOutcome::Rejected);
    let allocated = snapshot.identity.session.unwrap();
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let rows: i64 = db
        .query_row(
            "SELECT count(*) FROM retained_sessions WHERE id=?1",
            [allocated.0 as i64],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 0);

    live::git(&fixture.live.repo, &["switch", "main"]);
    for _ in 0..2 {
        let recovered: AssignmentSnapshot =
            serde_json::from_value(fixture.recover(&snapshot.handle)).unwrap();
        assert_eq!(recovered.dispatch, DispatchStatus::Refused);
        assert_eq!(recovered.identity, snapshot.identity);
        assert_eq!(recovered.receipts, snapshot.receipts);
        assert_eq!(recovered.resources, snapshot.resources);
        let repeated = fixture.control(fixture.proxy.original(DispatchOperation::Launch));
        assert_eq!(repeated.identity, snapshot.identity);
        assert_eq!(repeated.receipts, snapshot.receipts);
        assert_eq!(
            fixture.child_count(),
            0,
            "restoring the root must not replay the rejected operation"
        );
    }
}

#[test]
fn public_dispatch_retired_parent_a_to_b_to_a_cannot_replay_control() {
    let fixture = RecoveryFixture::new();
    let initial: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let inventory = || {
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        sessions
            .into_iter()
            .find(|s| s.id == fixture.parent)
            .unwrap()
            .agent
            .unwrap()
            .binding
    };
    let original = inventory();
    for conversation in ["replacement-B", original.conversation.as_str()] {
        assert_eq!(
            fixture.live.request(Request::SendTerminal {
                session: fixture.parent,
                text: format!("session_start:{conversation}:new"),
                submit: true
            }),
            Response::Ok
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let binding = inventory();
            if binding.conversation == conversation && binding.generation > original.generation {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "replacement binding not observed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    assert_ne!(inventory().generation, original.generation);
    let retired = fixture.recover(&initial.handle);
    assert_eq!(retired["dispatch"], "Refused", "{retired}");
    assert!(
        retired["reason"]
            .as_str()
            .unwrap()
            .contains("controller changed")
    );
    assert_eq!(fixture.input_count(&initial), 1);
    assert_eq!(fixture.child_count(), 1);
}

#[path = "pi_delegation/observation.rs"]
mod observation;

#[path = "pi_delegation/cancellation.rs"]
mod cancellation;
