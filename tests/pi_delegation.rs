//! Public Pi tools -> control CLI -> isolated binary Server -> real managed child PTY.
#[path = "support/live.rs"]
mod live;
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
                stream
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                if exchange_preamble(&mut stream).is_err() {
                    continue;
                }
                let message: ClientMessage = read_frame(&mut stream).unwrap();
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
                            let DelegationRequest::SendInitial { identity, .. } = request.as_ref()
                            else {
                                unreachable!()
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

impl Drop for ControlProxy {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::os::unix::net::UnixStream::connect(&self.path);
        self.thread.take().unwrap().join().unwrap();
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
        for group in self.live.session_groups() {
            self.live.own_group(group);
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
