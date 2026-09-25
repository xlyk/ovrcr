#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT, WORKSPACE};
use ovrcr::protocol::{
    AgentBinding, AgentCommand, AgentOperationResult, AgentProvider, AgentSecret, BranchRequest,
    ClientMessage, ConversationReference, CreateSessionRequest, DashboardView, ErrorCode,
    ExtensionConversation, Request, ReserveAgent, Response, ServerEvent, ServerMessage, SessionId,
    SessionLaunch, SessionPhase, SessionSummary, SupervisorAuth, SupervisorRequest, client,
    connect_server, read_frame, write_frame,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

fn fixture() -> Live {
    let live = Live::binary();
    assert_eq!(
        live.request(Request::AddProject {
            name: PROJECT.into(),
            repo: live.repo.clone(),
            workspace_root: live.workspace_root.clone(),
        }),
        Response::Ok
    );
    live.clear_root_shell();
    live
}

fn workspace(launch: Option<SessionLaunch>) -> Request {
    Request::CreateWorkspaceWithLaunch {
        project: PROJECT.into(),
        id: WORKSPACE.into(),
        branch: BranchRequest::New {
            branch: "title-work".into(),
            base: "main".into(),
        },
        launch,
    }
}

fn title_program() -> SessionLaunch {
    SessionLaunch { kind: ovrcr_protocol::SessionKind::Terminal, argv: vec!["/bin/sh".into(), "-c".into(),
        "stty -echo; printf 'TITLE-READY\\n'; while IFS= read -r title; do [ \"$title\" = EXIT ] && exit 0; printf '\\033]2;%s\\007applied:%s\\n' \"$title\" \"$title\"; done".into()],
    label: Some("owned title fixture".into()), }
}

fn start_title_fixture(
    pi_script: String,
    settings: Option<&str>,
) -> (Live, std::path::PathBuf, std::path::PathBuf) {
    let live = Live::idle().bounded();
    let fake_pi = live.root.path().join("fake-pi");
    let calls = live.root.path().join("title-calls");
    std::fs::write(
        &fake_pi,
        pi_script.replace("__CALLS__", &calls.to_string_lossy()),
    )
    .unwrap();
    std::fs::set_permissions(&fake_pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    if let Some(settings) = settings {
        std::fs::write(live.root.path().join("dashboard.toml"), settings).unwrap();
    }
    live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", fake_pi.as_os_str())]);
    assert_eq!(
        live.request(Request::AddProject {
            name: PROJECT.into(),
            repo: live.repo.clone(),
            workspace_root: live.workspace_root.clone(),
        }),
        Response::Ok
    );
    live.clear_root_shell();
    assert_eq!(live.request(workspace(None)), Response::Ok);
    (live, fake_pi, calls)
}

fn title_fixture() -> (Live, std::path::PathBuf, std::path::PathBuf) {
    start_title_fixture(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> __CALLS__\nprintf '%s\\n' \"$PWD\" >> __CALLS__.cwd\nwhile IFS= read -r line; do printf '%s\\n' '{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Fixture Topic\"}],\"stopReason\":\"stop\"}}'; printf '%s\\n' '{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_result\",\"text\":\"ignored\"}],\"stopReason\":\"stop\"}}'; exit 0; done\n".to_owned(),
        Some("title_model = 'pi/test'\n"),
    )
}

fn agent_program(token: &std::path::Path) -> Vec<std::ffi::OsString> {
    vec![
        "/bin/sh".into(),
        "-c".into(),
        "stty -echo; printf '%s\\n' \"$OVRCR_HOOK_TOKEN\" > \"$1\"; printf AGENT_READY; while IFS= read -r line; do :; done".into(),
        "agent-title-fixture".into(),
        token.as_os_str().into(),
    ]
}

fn parse_secret(path: &std::path::Path) -> AgentSecret {
    let text = std::fs::read_to_string(path).unwrap();
    let text = text.trim();
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap();
    }
    AgentSecret(bytes)
}

fn retain_reference(
    live: &Live,
    session: &SessionSummary,
    capability: AgentSecret,
    provider: AgentProvider,
    conversation: &str,
    reference: ConversationReference,
) {
    let mut stream = connect_server(&live.socket).unwrap();
    let Response::AgentOperation(AgentOperationResult::Reserved(reservation)) = client::request(
        &mut stream,
        10,
        Request::ReserveAgent(ReserveAgent {
            session: session.id,
            capability,
            operation: format!("reserve-{conversation}"),
            expected_epoch: 0,
            invocation: format!("invocation-{conversation}"),
            provider,
        }),
    )
    .unwrap() else {
        panic!("reserve failed")
    };
    let auth = SupervisorAuth {
        session: session.id,
        lease: reservation.lease,
    };
    let Response::AgentOperation(AgentOperationResult::Bound(binding)) = client::request(
        &mut stream,
        11,
        Request::Supervisor(SupervisorRequest {
            auth: auth.clone(),
            operation: format!("bind-{conversation}"),
            command: AgentCommand::Bind {
                expected_binding: None,
                conversation: conversation.into(),
            },
        }),
    )
    .unwrap() else {
        panic!("bind failed")
    };
    assert_eq!(
        binding,
        AgentBinding {
            provider,
            invocation: format!("invocation-{conversation}"),
            conversation: conversation.into(),
            generation: 1,
        }
    );
    let retained = client::request(
        &mut stream,
        12,
        Request::Supervisor(SupervisorRequest {
            auth,
            operation: format!("retain-{conversation}"),
            command: AgentCommand::RetainConversation {
                binding,
                reference: Box::new(reference),
            },
        }),
    )
    .unwrap();
    assert!(
        matches!(
            retained,
            Response::AgentOperation(AgentOperationResult::ConversationRetained)
        ),
        "retain failed for {conversation}: {retained:?}"
    );
}

fn retain_pi_history(
    live: &Live,
    fake_pi: &std::path::Path,
    session: &SessionSummary,
    token: &std::path::Path,
    conversation: &str,
    history: std::path::PathBuf,
) {
    retain_reference(
        live,
        session,
        parse_secret(token),
        AgentProvider::Pi,
        conversation,
        ConversationReference::Pi(ExtensionConversation {
            conversation: conversation.into(),
            executable: fake_pi.into(),
            history: Some(history),
            config_dir: live.root.path().into(),
            options: vec![],
        }),
    );
}

fn wait_display(live: &Live, id: SessionId, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let name = sessions(live)
            .into_iter()
            .find(|session| session.id == id)
            .unwrap()
            .display_name()
            .to_owned();
        if name == expected {
            return;
        }
        assert!(Instant::now() < deadline, "display stayed {name:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_file(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !path.exists() {
        assert!(Instant::now() < deadline, "missing file {}", path.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn created(live: &Live, request: Request) -> SessionSummary {
    let response = live.request(request);
    let Response::CreatedSession(summary) = response else {
        panic!("session creation failed: {response:?}")
    };
    if let Some(pid) = summary.pid {
        live.own_group(pid as libc::pid_t);
    }
    *summary
}

fn sessions(live: &Live) -> Vec<SessionSummary> {
    let Response::Hierarchy(hierarchy) = live.request(Request::List) else {
        panic!("list failed")
    };
    hierarchy
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .collect()
}

fn terminal(live: &Live, session: SessionId) -> String {
    match live.request(Request::ReadTerminal {
        session,
        max_lines: None,
    }) {
        Response::TerminalText { text, .. } => text,
        other => panic!("unexpected terminal response: {other:?}"),
    }
}

fn wait_for(live: &Live, session: SessionId, marker: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !terminal(live, session).contains(marker) {
        assert!(Instant::now() < deadline, "missing output {marker}");
        std::thread::yield_now();
    }
}

fn emit(live: &Live, session: SessionId, title: &str) {
    assert_eq!(
        live.request(Request::SendTerminal {
            session,
            text: title.into(),
            submit: true
        }),
        Response::Ok
    );
    wait_for(live, session, &format!("applied:{title}"));
}

fn dashboard(live: &Live) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut stream = connect_server(&live.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        match client::request(&mut stream, 1, Request::DashboardHello).unwrap() {
            Response::Hierarchy(_) => return stream,
            Response::Error {
                code: ErrorCode::Conflict,
                message,
            } if message == "another dashboard is already connected" => {
                // Closing a client releases server ownership asynchronously.
                assert!(
                    Instant::now() < deadline,
                    "dashboard ownership was not released"
                );
                std::thread::yield_now();
            }
            other => panic!("dashboard attach failed: {other:?}"),
        }
    }
}

fn changed(stream: &mut UnixStream, id: SessionId, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "missing title event {expected}");
        match read_frame::<ServerMessage>(stream).unwrap() {
            ServerMessage::Event(ServerEvent::SessionChanged(summary))
                if summary.id == id && summary.display_name() == expected =>
            {
                return;
            }
            ServerMessage::Event(ServerEvent::Output { .. }) => {
                panic!("hidden session must not deliver screen output")
            }
            _ => {}
        }
    }
}

// SetView is acknowledged by the same dispatcher that publishes output-driven
// events. After emit() observes parsed output, this fences its publication too.
fn no_title_changes(stream: &mut UnixStream, id: SessionId, revision: u64) {
    write_frame(
        stream,
        &ClientMessage {
            request_id: 2,
            request: Request::SetView {
                view: DashboardView {
                    revision,
                    panes: vec![],
                    focused: None,
                },
            },
        },
    )
    .unwrap();
    loop {
        match read_frame::<ServerMessage>(stream).unwrap() {
            ServerMessage::Response {
                request_id: 2,
                response,
            } => {
                assert_eq!(response, Response::Ok);
                return;
            }
            ServerMessage::Event(ServerEvent::SessionChanged(summary)) if summary.id == id => {
                panic!("application title must not publish a session change: {summary:?}");
            }
            ServerMessage::Event(ServerEvent::Output { .. }) => {
                panic!("hidden session must not deliver screen output");
            }
            _ => {}
        }
    }
}

#[test]
fn conversation_subjects_use_retained_provider_histories_and_survive_restart() {
    let (live, fake_pi, calls) = title_fixture();
    let _dashboard = dashboard(&live);
    let providers = [
        (AgentProvider::Pi, "pi"),
        (AgentProvider::Omp, "omp"),
        (AgentProvider::Codex, "codex"),
        (AgentProvider::Claude, "claude"),
    ];
    let mut made = Vec::new();
    for (provider, name) in providers {
        let token = live.root.path().join(format!("{name}.token"));
        let session = created(
            &live,
            Request::CreateSession(CreateSessionRequest {
                kind: ovrcr_protocol::SessionKind::Agent { name: name.into() },
                project: PROJECT.into(),
                workspace: WORKSPACE.into(),
                name: format!("named-{name}"),
                label: Some(name.into()),
                argv: agent_program(&token),
            }),
        );
        wait_for(&live, session.id, "AGENT_READY");
        assert_eq!(
            sessions(&live)
                .into_iter()
                .find(|row| row.id == session.id)
                .unwrap()
                .display_name(),
            format!("named-{name}")
        );
        let conversation = match provider {
            AgentProvider::Pi => "00000000-0000-4000-8000-000000000001".to_owned(),
            AgentProvider::Omp => "00000000-0000-4000-8000-000000000002".to_owned(),
            AgentProvider::Codex => "00000000-0000-4000-8000-000000000003".to_owned(),
            AgentProvider::Claude => "00000000-0000-4000-8000-000000000004".to_owned(),
            _ => unreachable!(),
        };
        let history = live.root.path().join(format!("{name}.jsonl"));
        let body = match provider {
            AgentProvider::Pi | AgentProvider::Omp => format!(
                "{{\"type\":\"session\",\"id\":\"{conversation}\"}}\n{{\"role\":\"user\",\"content\":\"please name it\"}}\n{{\"role\":\"assistant\",\"content\":\"the answer\"}}\n"
            ),
            AgentProvider::Codex => format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{conversation}\"}}}}\n{{\"role\":\"assistant\",\"content\":\"the answer\"}}\n"
            ),
            AgentProvider::Claude => format!(
                "{{\"type\":\"mode\"}}\n{{\"sessionId\":\"{conversation}\",\"role\":\"user\",\"content\":\"please name it\"}}\n{{\"sessionId\":\"{conversation}\",\"role\":\"assistant\",\"content\":\"the answer\"}}\n"
            ),
            _ => unreachable!(),
        };
        std::fs::write(&history, body).unwrap();
        let reference = match provider {
            AgentProvider::Pi => ConversationReference::Pi(ExtensionConversation {
                conversation: conversation.clone(),
                executable: fake_pi.clone(),
                history: Some(history),
                config_dir: live.root.path().into(),
                options: vec![],
            }),
            AgentProvider::Omp => ConversationReference::Omp(ExtensionConversation {
                conversation: conversation.clone(),
                executable: fake_pi.clone(),
                history: Some(history),
                config_dir: live.root.path().into(),
                options: vec![],
            }),
            AgentProvider::Codex => {
                ConversationReference::Codex(ovrcr::protocol::CodexConversation {
                    conversation: conversation.clone(),
                    executable: fake_pi.clone(),
                    history: Some(history),
                    config_dir: live.root.path().into(),
                    options: vec![],
                })
            }
            AgentProvider::Claude => {
                ConversationReference::Claude(ovrcr::protocol::ClaudeConversation {
                    conversation: conversation.clone(),
                    executable: fake_pi.clone(),
                    history,
                    config_dir: live.root.path().into(),
                    options: vec![],
                })
            }
            _ => unreachable!(),
        };
        retain_reference(
            &live,
            &session,
            parse_secret(&token),
            provider,
            &conversation,
            reference,
        );
        wait_display(&live, session.id, "Fixture Topic");
        made.push(session.id);
    }
    let call_count = std::fs::read_to_string(&calls).unwrap().lines().count();
    assert_eq!(call_count, 4);

    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", fake_pi.as_os_str())]);
    for id in made {
        wait_display(&live, id, "Fixture Topic");
    }
}

#[test]
fn title_model_missing_mismatches_terminal_and_grok_do_not_spawn_pi() {
    let live = Live::idle().bounded();
    let fake_pi = live.root.path().join("fake-pi");
    let calls = live.root.path().join("calls");
    std::fs::write(
        &fake_pi,
        format!("#!/bin/sh\nprintf called >> {}\nexit 0\n", calls.display()),
    )
    .unwrap();
    std::fs::set_permissions(&fake_pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", fake_pi.as_os_str())]);
    assert_eq!(
        live.request(Request::AddProject {
            name: PROJECT.into(),
            repo: live.repo.clone(),
            workspace_root: live.workspace_root.clone(),
        }),
        Response::Ok
    );
    live.clear_root_shell();
    assert_eq!(live.request(workspace(None)), Response::Ok);
    let _dashboard = dashboard(&live);
    let terminal = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "terminal-name".into(),
            label: Some("terminal".into()),
            argv: title_program().argv,
        }),
    );
    let grok_token = live.root.path().join("grok.token");
    let grok = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent {
                name: "grok".into(),
            },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "grok-name".into(),
            label: Some("grok".into()),
            argv: agent_program(&grok_token),
        }),
    );
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        sessions(&live)
            .into_iter()
            .find(|row| row.id == terminal.id)
            .unwrap()
            .display_name(),
        "terminal-name"
    );
    assert_eq!(
        sessions(&live)
            .into_iter()
            .find(|row| row.id == grok.id)
            .unwrap()
            .display_name(),
        "grok-name"
    );
    assert!(
        !calls.exists(),
        "title call spawned without a model or Grok history"
    );
}

#[test]
fn title_failures_mismatches_and_missing_binary_do_not_change_rows_or_retry() {
    let script = "#!/bin/sh\nprintf call >> __CALLS__\nexit 2\n".to_owned();
    let (live, fake_pi, calls) = start_title_fixture(script, Some("title_model = 'pi/test'\n"));
    let _dashboard = dashboard(&live);

    let valid_token = live.root.path().join("valid.token");
    let valid = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "failed-title".into(),
            label: Some("pi".into()),
            argv: agent_program(&valid_token),
        }),
    );
    wait_for(&live, valid.id, "AGENT_READY");
    let valid_history = live.root.path().join("valid.jsonl");
    std::fs::write(
        &valid_history,
        "{\"type\":\"session\",\"id\":\"00000000-0000-4000-8000-000000000101\"}\n{\"role\":\"assistant\",\"content\":\"reply\"}\n",
    )
    .unwrap();
    retain_pi_history(
        &live,
        &fake_pi,
        &valid,
        &valid_token,
        "00000000-0000-4000-8000-000000000101",
        valid_history,
    );
    wait_file(&calls);
    assert_eq!(
        sessions(&live)
            .into_iter()
            .find(|row| row.id == valid.id)
            .unwrap()
            .display_name(),
        "failed-title"
    );
    let db = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    let attempts: i64 = db
        .query_row(
            "SELECT attempt_count FROM conversation_subjects WHERE session = ?1",
            [i64::try_from(valid.id.0).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 1);
    drop(db);

    let mismatch_token = live.root.path().join("mismatch.token");
    let mismatch = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "mismatch-title".into(),
            label: Some("pi".into()),
            argv: agent_program(&mismatch_token),
        }),
    );
    wait_for(&live, mismatch.id, "AGENT_READY");
    let mismatch_history = live.root.path().join("mismatch.jsonl");
    std::fs::write(
        &mismatch_history,
        "{\"type\":\"session\",\"id\":\"wrong\"}\n{\"role\":\"assistant\",\"content\":\"reply\"}\n",
    )
    .unwrap();
    retain_pi_history(
        &live,
        &fake_pi,
        &mismatch,
        &mismatch_token,
        "00000000-0000-4000-8000-000000000102",
        mismatch_history,
    );
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(std::fs::read_to_string(&calls).unwrap(), "call");
    let db = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    let mismatch_rows: i64 = db
        .query_row(
            "SELECT count(*) FROM conversation_subjects WHERE session = ?1",
            [i64::try_from(mismatch.id.0).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        mismatch_rows, 0,
        "header mismatch must not spend an attempt"
    );
    drop(db);

    let missing = Live::idle().bounded();
    std::fs::write(
        missing.root.path().join("dashboard.toml"),
        "title_model = 'pi/test'\n",
    )
    .unwrap();
    let late_pi = missing.root.path().join("late-pi");
    missing.start_binary_env(&[("OVRCR_PI_EXECUTABLE", late_pi.as_os_str())]);
    assert_eq!(
        missing.request(Request::AddProject {
            name: PROJECT.into(),
            repo: missing.repo.clone(),
            workspace_root: missing.workspace_root.clone(),
        }),
        Response::Ok
    );
    missing.clear_root_shell();
    assert_eq!(missing.request(workspace(None)), Response::Ok);
    let _dashboard = dashboard(&missing);
    let token = missing.root.path().join("missing.token");
    let session = created(
        &missing,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "missing-pi".into(),
            label: Some("pi".into()),
            argv: agent_program(&token),
        }),
    );
    wait_for(&missing, session.id, "AGENT_READY");
    let history = missing.root.path().join("missing.jsonl");
    std::fs::write(
        &history,
        "{\"type\":\"session\",\"id\":\"00000000-0000-4000-8000-000000000103\"}\n{\"role\":\"assistant\",\"content\":\"reply\"}\n",
    )
    .unwrap();
    retain_pi_history(
        &missing,
        &late_pi,
        &session,
        &token,
        "00000000-0000-4000-8000-000000000103",
        history.clone(),
    );
    std::thread::sleep(Duration::from_secs(3));
    let late_calls = missing.root.path().join("late-calls");
    std::fs::write(
        &late_pi,
        format!(
            "#!/bin/sh\nprintf late >> {}\nexit 0\n",
            late_calls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&late_pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        &history,
        "{\"type\":\"session\",\"id\":\"00000000-0000-4000-8000-000000000103\"}\n{\"role\":\"assistant\",\"content\":\"changed\"}\n",
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        !late_calls.exists(),
        "missing Pi must not retry in this server process"
    );
    assert_eq!(
        sessions(&missing)
            .into_iter()
            .find(|row| row.id == session.id)
            .unwrap()
            .display_name(),
        "missing-pi",
        "missing Pi must not retry in this server process"
    );
}

#[test]
fn title_subject_storage_archive_and_exclusions_cover_live_paths() {
    let script = "#!/bin/sh\nprintf call >> __CALLS__\nwhile IFS= read -r line; do printf '%s\\n' \"$line\" > __CALLS__.stdin; printf '%s\\n' '{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Fixture Topic\"}],\"stopReason\":\"stop\"}}'; sleep 1; exit 0; done\n".to_owned();
    let (live, fake_pi, calls) = start_title_fixture(script, Some("title_model = 'pi/test'\n"));
    let _dashboard = dashboard(&live);
    let token = live.root.path().join("subject.token");
    let session = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "subject-original".into(),
            label: Some("pi".into()),
            argv: agent_program(&token),
        }),
    );
    wait_for(&live, session.id, "AGENT_READY");
    let history = live.root.path().join("large.jsonl");
    let head_secret = "HEAD_SECRET_SHOULD_NOT_BE_READ";
    std::fs::write(
        &history,
        format!(
            "{{\"type\":\"session\",\"id\":\"00000000-0000-4000-8000-000000000201\"}}\n{}\n{{\"role\":\"user\",\"content\":\"tail prompt\"}}\n{{\"role\":\"assistant\",\"content\":\"tail reply\"}}\n",
            format!("{head_secret}\n").repeat(300 * 1024 / head_secret.len())
        ),
    )
    .unwrap();
    retain_pi_history(
        &live,
        &fake_pi,
        &session,
        &token,
        "00000000-0000-4000-8000-000000000201",
        history,
    );
    wait_file(&calls);
    wait_display(&live, session.id, "Fixture Topic");
    let prompt = std::fs::read_to_string(calls.with_extension("stdin")).unwrap();
    assert!(prompt.contains("tail reply"));
    assert!(!prompt.contains(head_secret));
    let database = std::fs::read(ovrcr::config::database_path(&live.config)).unwrap();
    let database = String::from_utf8_lossy(&database);
    assert!(!database.contains("tail reply"));
    assert!(!database.contains("Name this conversation"));
    assert!(!database.contains(head_secret));

    assert_eq!(
        live.request(Request::CloseTerminal {
            session: session.id,
            expected_run: session.run,
        }),
        Response::Ok
    );
    let archived = std::process::Command::new(&live.executable)
        .args(["--json", "terminal", "list", "--archived"])
        .env("OVRCR_CONFIG", &live.config)
        .env("OVRCR_SOCKET", &live.socket)
        .output()
        .unwrap();
    assert!(archived.status.success(), "{archived:?}");
    let rows: serde_json::Value = serde_json::from_slice(&archived.stdout).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == session.id.0)
        .unwrap();
    assert_eq!(row["display_name"], "Fixture Topic");

    let hermes_token = live.root.path().join("hermes.token");
    let hermes = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent {
                name: "hermes".into(),
            },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "hermes-original".into(),
            label: Some("hermes".into()),
            argv: agent_program(&hermes_token),
        }),
    );
    wait_for(&live, hermes.id, "AGENT_READY");
    let nested = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "nested-agent-terminal".into(),
            label: Some("terminal".into()),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf NESTED_READY; while IFS= read -r line; do :; done".into(),
            ],
        }),
    );
    wait_for(&live, nested.id, "NESTED_READY");
    let before = std::fs::read_to_string(&calls).unwrap();
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(std::fs::read_to_string(&calls).unwrap(), before);
}

#[test]
fn title_startup_and_shutdown_cleanup_titles_directory_and_process_group() {
    let live = Live::idle().bounded();
    let titles = live.socket.parent().unwrap().join("titles");
    std::fs::create_dir_all(&titles).unwrap();
    std::fs::write(titles.join("leftover"), "stale").unwrap();
    std::fs::write(
        live.root.path().join("dashboard.toml"),
        "title_model = 'pi/test'\n",
    )
    .unwrap();
    let fake_pi = live.root.path().join("fake-pi");
    let calls = live.root.path().join("title-calls");
    std::fs::write(
        &fake_pi,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > {}.pid\nprintf started > {}\nwhile :; do sleep 1; done\n",
            calls.display(),
            calls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake_pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", fake_pi.as_os_str())]);
    assert!(
        !titles.exists(),
        "stale titles directory must be removed on startup"
    );
    assert_eq!(
        live.request(Request::AddProject {
            name: PROJECT.into(),
            repo: live.repo.clone(),
            workspace_root: live.workspace_root.clone(),
        }),
        Response::Ok
    );
    live.clear_root_shell();
    assert_eq!(live.request(workspace(None)), Response::Ok);
    let _dashboard = dashboard(&live);
    let token = live.root.path().join("blocking.token");
    let session = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "blocking-title".into(),
            label: Some("pi".into()),
            argv: agent_program(&token),
        }),
    );
    wait_for(&live, session.id, "AGENT_READY");
    let history = live.root.path().join("blocking.jsonl");
    std::fs::write(
        &history,
        "{\"type\":\"session\",\"id\":\"00000000-0000-4000-8000-000000000301\"}\n{\"role\":\"assistant\",\"content\":\"reply\"}\n",
    )
    .unwrap();
    retain_pi_history(
        &live,
        &fake_pi,
        &session,
        &token,
        "00000000-0000-4000-8000-000000000301",
        history,
    );
    wait_file(&calls);
    let pgid: libc::pid_t = std::fs::read_to_string(calls.with_extension("pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(live::group_exists(pgid));
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    assert!(live::wait_group_absent(pgid, Duration::from_secs(2)));
}

#[test]
fn late_title_result_after_archive_is_not_saved_or_shown() {
    let script = "#!/bin/sh\nprintf started > __CALLS__\nsleep 2\nprintf '%s\\n' '{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Late Topic\"}],\"stopReason\":\"stop\"}}'\n".to_owned();
    let (live, fake_pi, calls) = start_title_fixture(script, Some("title_model = 'pi/test'\n"));
    let _dashboard = dashboard(&live);
    let token = live.root.path().join("late.token");
    let session = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "late-original".into(),
            label: Some("pi".into()),
            argv: agent_program(&token),
        }),
    );
    wait_for(&live, session.id, "AGENT_READY");
    let history = live.root.path().join("late.jsonl");
    std::fs::write(
        &history,
        "{\"type\":\"session\",\"id\":\"00000000-0000-4000-8000-000000000401\"}\n{\"role\":\"assistant\",\"content\":\"reply\"}\n",
    )
    .unwrap();
    retain_pi_history(
        &live,
        &fake_pi,
        &session,
        &token,
        "00000000-0000-4000-8000-000000000401",
        history,
    );
    wait_file(&calls);
    assert_eq!(
        live.request(Request::CloseTerminal {
            session: session.id,
            expected_run: session.run,
        }),
        Response::Ok
    );
    std::thread::sleep(Duration::from_secs(3));
    let archived = std::process::Command::new(&live.executable)
        .args(["--json", "terminal", "list", "--archived"])
        .env("OVRCR_CONFIG", &live.config)
        .env("OVRCR_SOCKET", &live.socket)
        .output()
        .unwrap();
    let rows: serde_json::Value = serde_json::from_slice(&archived.stdout).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == session.id.0)
        .unwrap();
    assert_eq!(row["display_name"], "late-original");
}

#[test]
fn stable_workspace_launch_titles_rename_reset_and_reopen_keep_identity_and_reset_output() {
    let live = fixture();
    let original = created(&live, workspace(Some(title_program())));
    assert_eq!(
        sessions(&live).len(),
        1,
        "selected launch replaces default shell"
    );
    wait_for(&live, original.id, "TITLE-READY");
    let mut dashboard = dashboard(&live); // no pane selected: output remains hidden
    emit(&live, original.id, "first title 🦀");
    assert_eq!(sessions(&live)[0].display_name(), "title-work");
    no_title_changes(&mut dashboard, original.id, 1);
    assert_eq!(
        live.request(Request::SetSessionTitle {
            session: original.id,
            title: Some("  pinned  ".into())
        }),
        Response::Ok
    );
    changed(&mut dashboard, original.id, "pinned");
    emit(&live, original.id, "latest application");
    assert_eq!(sessions(&live)[0].display_name(), "pinned");
    no_title_changes(&mut dashboard, original.id, 2);
    assert_eq!(
        live.request(Request::SetSessionTitle {
            session: original.id,
            title: None
        }),
        Response::Ok
    );
    changed(&mut dashboard, original.id, "title-work");
    assert_eq!(sessions(&live)[0].name, original.name);
    let reused = created(
        &live,
        Request::ReopenSession {
            session: original.id,
            expected_run: original.run,
            acknowledge_stopped: false,
        },
    );
    assert_eq!(
        (reused.id, reused.run, reused.pid),
        (original.id, original.run, original.pid)
    );
    assert!(matches!(
        live.request(Request::SetSessionTitle {
            session: original.id,
            title: Some(" \n\t".into())
        }),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert_eq!(
        live.request(Request::SendTerminal {
            session: original.id,
            text: "EXIT".into(),
            submit: true
        }),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(sessions(&live)[0].phase, SessionPhase::Exited { .. }) {
        assert!(Instant::now() < deadline, "session did not exit");
        std::thread::yield_now();
    }
    assert!(terminal(&live, original.id).contains("TITLE-READY"));
    let reopened = created(
        &live,
        Request::ReopenSession {
            session: original.id,
            expected_run: original.run,
            acknowledge_stopped: true,
        },
    );
    assert_eq!(reopened.id, original.id);
    assert_eq!(reopened.run.0, original.run.0 + 1);
    assert_eq!(reopened.name, original.name);
    assert_eq!(reopened.display_name(), "title-work");
    assert!(!terminal(&live, reopened.id).contains("TITLE-READY"));
    assert_eq!(
        live.request(Request::SendTerminal {
            session: reopened.id,
            text: r"printf '\033]2;fresh title\007'; printf 'SHELL_%s\n' TITLE_SET".into(),
            submit: true,
        }),
        Response::Ok
    );
    wait_for(&live, reopened.id, "SHELL_TITLE_SET");
    assert_eq!(sessions(&live)[0].display_name(), "title-work");
    assert_eq!(sessions(&live).len(), 1);
}

#[test]
fn empty_workspace_and_generated_names_preserve_explicit_titles() {
    let live = fixture();
    assert_eq!(live.request(workspace(None)), Response::Ok);
    assert!(sessions(&live).is_empty());
    let registry = ovrcr::config::load_registry(&live.config).unwrap();
    assert!(
        registry
            .workspace(PROJECT, WORKSPACE)
            .unwrap()
            .path
            .join(".git")
            .is_file()
    );
    let new = |name: &str| {
        let launch = title_program();
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: name.into(),
            label: launch.label,
            argv: launch.argv,
        })
    };
    let explicit = created(&live, new("title-work"));
    let automatic = created(&live, new(""));
    assert_ne!(
        automatic.name, explicit.name,
        "automatic naming must avoid the pinned name"
    );
    wait_for(&live, explicit.id, "TITLE-READY");
    emit(&live, explicit.id, "application override");
    assert_eq!(
        sessions(&live)
            .into_iter()
            .find(|session| session.id == explicit.id)
            .unwrap()
            .display_name(),
        "title-work"
    );
    assert!(matches!(
        live.request(new("title-work")),
        Response::Error {
            code: ErrorCode::AlreadyExists,
            ..
        }
    ));
    assert_eq!(
        live.request(Request::SendTerminal {
            session: explicit.id,
            text: "EXIT".into(),
            submit: true
        }),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !sessions(&live).into_iter().any(|session| {
        session.id == explicit.id && matches!(session.phase, SessionPhase::Exited { .. })
    }) {
        assert!(Instant::now() < deadline, "explicit session did not exit");
        std::thread::yield_now();
    }
    let reopened = created(
        &live,
        Request::ReopenSession {
            session: explicit.id,
            expected_run: explicit.run,
            acknowledge_stopped: true,
        },
    );
    assert_eq!(reopened.id, explicit.id);
    assert_eq!(reopened.run.0, explicit.run.0 + 1);
    assert_eq!(reopened.name, explicit.name);
    assert_eq!(
        reopened.display_name(),
        "title-work",
        "reopen keeps pinned title"
    );
    assert_eq!(
        live.request(Request::SendTerminal {
            session: reopened.id,
            text: r"printf '\033]2;still pinned\007'; printf 'PIN_%s\n' KEPT".into(),
            submit: true,
        }),
        Response::Ok
    );
    wait_for(&live, reopened.id, "PIN_KEPT");
    assert_eq!(
        sessions(&live)
            .into_iter()
            .find(|session| session.id == reopened.id)
            .unwrap()
            .display_name(),
        "title-work",
    );
}

#[test]
fn agent_and_terminal_titles_stay_user_controlled_across_dashboard_reconnect() {
    let live = fixture();
    assert_eq!(live.request(workspace(None)), Response::Ok);
    for kind in [
        ovrcr_protocol::SessionKind::Terminal,
        ovrcr_protocol::SessionKind::Agent {
            name: "fixture-agent".into(),
        },
    ] {
        for explicit in [false, true] {
            let launch = title_program();
            let name = if explicit {
                format!("named-{}", sessions(&live).len())
            } else {
                String::new()
            };
            let session = created(
                &live,
                Request::CreateSession(CreateSessionRequest {
                    kind: kind.clone(),
                    project: PROJECT.into(),
                    workspace: WORKSPACE.into(),
                    name,
                    label: launch.label,
                    argv: launch.argv,
                }),
            );
            wait_for(&live, session.id, "TITLE-READY");
            let mut stream = dashboard(&live);
            for (revision, title) in [(1, "Thinking…"), (2, "Done;界")] {
                emit(&live, session.id, title);
                let current = sessions(&live)
                    .into_iter()
                    .find(|s| s.id == session.id)
                    .unwrap();
                assert_eq!(current.display_name(), session.name);
                assert_eq!(current.run, session.run);
                no_title_changes(&mut stream, session.id, revision);
            }
            let id = session.id.0.to_string();
            for args in [
                vec!["terminal", "rename", &id, "User rename"],
                vec!["terminal", "rename", &id, "--reset"],
            ] {
                let expected = if args[3] == "--reset" {
                    &session.name
                } else {
                    "User rename"
                };
                let result = std::process::Command::new(&live.executable)
                    .args(args)
                    .env("OVRCR_CONFIG", &live.config)
                    .env("OVRCR_SOCKET", &live.socket)
                    .output()
                    .unwrap();
                assert!(result.status.success(), "{result:?}");
                changed(&mut stream, session.id, expected);
                drop(stream);
                stream = dashboard(&live);
                emit(&live, session.id, &format!("After reconnect: {expected}"));
                let current = sessions(&live)
                    .into_iter()
                    .find(|s| s.id == session.id)
                    .unwrap();
                assert_eq!(current.display_name(), expected);
                assert_eq!(
                    (current.id, current.run, current.pid),
                    (session.id, session.run, session.pid)
                );
                no_title_changes(&mut stream, session.id, 1);
            }
        }
    }
}

#[test]
fn saved_titles_survive_restart_and_reopen_without_legacy_application_titles() {
    let live = fixture();
    assert_eq!(live.request(workspace(None)), Response::Ok);
    let mut originals = Vec::new();
    for name in ["", "", "explicit"] {
        let launch = title_program();
        let session = created(
            &live,
            Request::CreateSession(CreateSessionRequest {
                kind: launch.kind,
                project: PROJECT.into(),
                workspace: WORKSPACE.into(),
                name: name.into(),
                label: launch.label,
                argv: launch.argv,
            }),
        );
        wait_for(&live, session.id, "TITLE-READY");
        originals.push(session);
    }
    assert_eq!(
        live.request(Request::SetSessionTitle {
            session: originals[2].id,
            title: Some("User title".into()),
        }),
        Response::Ok
    );
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    // Seed old-version storage, then assert only through public server/CLI reads.
    let db = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    for session in [&originals[0], &originals[2]] {
        db.execute(
            "UPDATE retained_sessions SET application_title = 'Legacy app' WHERE id = ?1",
            [i64::try_from(session.id.0).unwrap()],
        )
        .unwrap();
    }
    drop(db);
    let expected = ["title-work", "title-work-2", "User title"];
    let offline = std::process::Command::new(&live.executable)
        .args(["--json", "terminal", "list"])
        .env("OVRCR_CONFIG", &live.config)
        .env("OVRCR_SOCKET", &live.socket)
        .output()
        .unwrap();
    assert!(offline.status.success(), "{offline:?}");
    let rows: serde_json::Value = serde_json::from_slice(&offline.stdout).unwrap();
    for (original, expected) in originals.iter().zip(expected) {
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == original.id.0)
            .unwrap();
        assert_eq!(row["display_name"], expected);
    }
    live.start_binary();
    for (original, expected) in originals.iter().zip(expected) {
        let retained = sessions(&live)
            .into_iter()
            .find(|s| s.id == original.id)
            .unwrap();
        assert_eq!(retained.display_name(), expected);
        assert_eq!(retained.run, original.run);
        let reopened = created(
            &live,
            Request::ReopenSession {
                session: original.id,
                expected_run: original.run,
                acknowledge_stopped: false,
            },
        );
        assert_eq!(reopened.display_name(), expected);
        assert_eq!(reopened.id, original.id);
        assert_eq!(reopened.name, original.name);
        assert_eq!(reopened.run.0, original.run.0 + 1);
        assert_eq!(
            live.request(Request::SendTerminal {
                session: reopened.id,
                text: "printf '\\033]0;new application\\007'; printf 'REOPEN_%s\\n' TITLE_OK"
                    .into(),
                submit: true,
            }),
            Response::Ok
        );
        wait_for(&live, reopened.id, "REOPEN_TITLE_OK");
        assert_eq!(
            sessions(&live)
                .into_iter()
                .find(|s| s.id == reopened.id)
                .unwrap()
                .display_name(),
            expected
        );
    }
    // A user reset is durable too, for both generated and explicit names.
    for original in [&originals[0], &originals[2]] {
        for title in [Some("Temporary rename".into()), None] {
            assert_eq!(
                live.request(Request::SetSessionTitle {
                    session: original.id,
                    title
                }),
                Response::Ok
            );
        }
    }
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    live.start_binary();
    for original in &originals {
        let retained = sessions(&live)
            .into_iter()
            .find(|s| s.id == original.id)
            .unwrap();
        assert_eq!(retained.display_name(), original.name);
        let reopened = created(
            &live,
            Request::ReopenSession {
                session: retained.id,
                expected_run: retained.run,
                acknowledge_stopped: false,
            },
        );
        assert_eq!(reopened.display_name(), original.name);
    }
}

#[test]
fn workspace_launch_failure_retains_worktree_and_retry_uses_existing_workspace() {
    let live = fixture();
    let response = live.request(workspace(Some(SessionLaunch {
        kind: ovrcr_protocol::SessionKind::Terminal,
        argv: vec!["/does-not-exist/ovrcr-owned-test".into()],
        label: None,
    })));
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            }
        ),
        "{response:?}"
    );
    let registry = ovrcr::config::load_registry(&live.config).unwrap();
    assert!(
        registry
            .workspace(PROJECT, WORKSPACE)
            .unwrap()
            .path
            .join(".git")
            .is_file()
    );
    assert!(sessions(&live).is_empty());
    assert!(matches!(
        live.request(workspace(None)),
        Response::Error {
            code: ErrorCode::AlreadyExists,
            ..
        }
    ));
    let launch = title_program();
    let retry = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: String::new(),
            argv: launch.argv,
            label: launch.label,
        }),
    );
    wait_for(&live, retry.id, "TITLE-READY");
    assert!(matches!(
        live.request(Request::ReopenSession {
            session: SessionId(u64::MAX),
            expected_run: ovrcr::protocol::SessionRunId(1),
            acknowledge_stopped: false,
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
}
