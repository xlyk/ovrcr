//! Live, production-path Initial prompt intent regressions.
//!
//! The owned `claude` executable below is only a native CLI stand-in. Its
//! capture proves what OVRCR passed to that process; it is not provider
//! acceptance evidence.
#[path = "support/live.rs"]
mod live;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use live::{Live, PROJECT, WORKSPACE};
use ovrcr::protocol::{
    AgentModelPolicy, BranchRequest, ClientMessage, CreateAgentSessionRequest,
    CreateSessionRequest, ErrorCode, InitialPromptIntentId, InitialPromptIntentSnapshot,
    InitialPromptIntentState, InitialPromptLaunchRequest, LaunchChoice, Request, Response,
    ServerEvent, ServerMessage, SessionKind, SessionPhase, SettingsReport, client, connect_server,
    read_frame, write_frame,
};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const CAPTURE_ENV: &str = "OVRCR_TEST_FAKE_CLAUDE_CAPTURE";
const INTERACTIVE_ENV: &str = "OVRCR_TEST_FAKE_CLAUDE_INTERACTIVE";
const FAKE_LAUNCH_MARKER: &str = "__OVRCR_FAKE_CLAUDE_LAUNCH__";
const BRANCH: &str = "feature/initial-prompt-intent";

fn install_fake_claude(live: &Live) -> (PathBuf, PathBuf) {
    let bin = live.root.path().join("fake-native-bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("claude");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\ncase \"$1\" in auth|--version|--output-format) exit 0 ;; esac\nprintf '%s\\n' '{FAKE_LAUNCH_MARKER}' >> \"${CAPTURE_ENV}\"\nfor argument in \"$@\"; do printf 'ARG:%s\\n' \"$argument\" >> \"${CAPTURE_ENV}\"; done\nif [ \"$OVRCR_TEST_FAKE_CLAUDE_INTERACTIVE\" = \"1\" ]; then IFS= read -r input || exit 0; printf 'INPUT:%s\\n' \"$input\" >> \"${CAPTURE_ENV}\"; fi\nexit 0\n"
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    let capture = live.root.path().join("fake-claude-argv.txt");
    (executable, capture)
}

fn fixture_path(live: &Live, fake_claude: &Path) -> OsString {
    let mut entries = vec![fake_claude.parent().unwrap().to_path_buf()];
    entries.extend(std::env::split_paths(&live.path()));
    std::env::join_paths(entries).unwrap()
}

fn start_server(
    live: &Live,
    fake_claude: &Path,
    capture: &Path,
    log: Option<&Path>,
    interactive: bool,
) {
    let path = fixture_path(live, fake_claude);
    let interactive = OsString::from(if interactive { "1" } else { "0" });
    let environment = [
        ("PATH", path.as_os_str()),
        (CAPTURE_ENV, capture.as_os_str()),
        (INTERACTIVE_ENV, interactive.as_os_str()),
    ];
    if let Some(log) = log {
        live.start_binary_logged(&environment, log);
    } else {
        live.start_binary_env(&environment);
    }
}

fn write_settings(live: &Live, content: &str) {
    std::fs::write(live.config.join("dashboard.toml"), content).unwrap();
}

fn detected_claude_launch(
    live: &Live,
    fake_claude: &Path,
    name: &str,
    model_policy: AgentModelPolicy,
) -> CreateAgentSessionRequest {
    let mut native = vec![fake_claude.canonicalize().unwrap().into_os_string()];
    ovrcr::protocol::apply_default_auto_trust("claude", &mut native);
    let mut argv = vec![
        live.executable.canonicalize().unwrap().into_os_string(),
        "agent".into(),
        "run".into(),
        "claude".into(),
        "--".into(),
    ];
    argv.extend(native);
    CreateAgentSessionRequest {
        session: CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: name.into(),
            label: Some("claude".into()),
            argv,
            kind: SessionKind::Agent {
                name: "claude".into(),
            },
        },
        model_policy,
        model_binding: None,
        expected_model: None,
    }
}

fn dashboard(live: &Live) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut stream = connect_server(&live.socket).expect("connect to fixture Server");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        match client::request(&mut stream, 1, Request::DashboardHello) {
            Ok(Response::Hierarchy(_)) => return stream,
            Ok(Response::Error {
                code: ErrorCode::Conflict,
                ..
            }) if Instant::now() < deadline => {
                drop(stream);
                thread::park_timeout(Duration::from_millis(10));
            }
            Ok(response) => panic!("unexpected DashboardHello response: {response:?}"),
            Err(error) => panic!("DashboardHello did not complete: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "current Dashboard did not attach"
        );
    }
}

fn drive_dashboard_requests(
    connection: &mut UnixStream,
    dashboard: &mut ovrcr::tui::Dashboard,
    initial: Vec<ClientMessage>,
) -> Vec<ServerMessage> {
    let mut awaiting = BTreeSet::new();
    for request in initial {
        assert!(awaiting.insert(request.request_id));
        write_frame(connection, &request).expect("send Dashboard request");
    }
    let mut observed = Vec::new();
    while !awaiting.is_empty() {
        let message = read_frame::<ServerMessage>(connection).expect("read Dashboard response");
        let response_id = match &message {
            ServerMessage::Response { request_id, .. } => Some(*request_id),
            ServerMessage::Event(_) => None,
        };
        for followup in dashboard.handle_server_message(message.clone()) {
            assert!(awaiting.insert(followup.request_id));
            write_frame(connection, &followup).expect("send Dashboard follow-up");
        }
        if let Some(request_id) = response_id {
            awaiting.remove(&request_id);
        }
        observed.push(message);
    }
    observed
}

fn dashboard_round_trip(
    connection: &mut UnixStream,
    dashboard: &mut ovrcr::tui::Dashboard,
    request_id: u64,
    request: Request,
) -> Vec<ServerMessage> {
    drive_dashboard_requests(
        connection,
        dashboard,
        vec![ClientMessage {
            request_id,
            request,
        }],
    )
}

fn wait_for_dashboard_settings(connection: &mut UnixStream, dashboard: &mut ovrcr::tui::Dashboard) {
    loop {
        let message = read_frame::<ServerMessage>(connection).expect("read initial settings event");
        let settings = matches!(
            &message,
            ServerMessage::Event(ServerEvent::SettingsChanged(_))
        );
        let followups = dashboard.handle_server_message(message);
        if !followups.is_empty() {
            drive_dashboard_requests(connection, dashboard, followups);
        }
        if settings {
            return;
        }
    }
}

#[test]
fn dashboard_new_agent_session_starts_on_the_selected_registered_checkout() {
    let live = Live::idle();
    write_settings(
        &live,
        "agents = [{ name = \"fixture-agent\", argv = [\"/bin/sh\", \"-c\", \"printf FIXTURE_AGENT_READY; exec sleep 60\"] }]\nlaunch_choices.fixture = { kind = \"Agent\", preset = \"fixture-agent\", model_policy = \"HarnessDefault\" }\n[quota]\nenabled = false\n",
    );
    live.start();
    assert_eq!(
        live.request(Request::AddProject {
            name: PROJECT.into(),
            repo: live.repo.clone(),
            workspace_root: live.workspace_root.clone(),
        }),
        Response::Ok
    );
    live.clear_root_shell();
    assert_eq!(
        live.request(Request::CreateWorkspace {
            project: PROJECT.into(),
            id: WORKSPACE.into(),
            branch: BranchRequest::New {
                branch: "feature/dashboard-agent".into(),
                base: "main".into(),
            },
        }),
        Response::Ok
    );

    let mut dashboard =
        ovrcr::tui::Dashboard::new(ovrcr::session::TerminalSize { rows: 24, cols: 80 });
    dashboard.install_config_dir(live.config.clone());
    let mut connection = connect_server(&live.socket).expect("connect Dashboard socket");
    connection
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    connection
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();

    let mut startup = dashboard_round_trip(
        &mut connection,
        &mut dashboard,
        90_001,
        Request::DashboardHello,
    );
    assert!(startup.iter().any(|message| matches!(
        message,
        ServerMessage::Response {
            response: Response::Hierarchy(_),
            ..
        }
    )));
    startup.extend(dashboard_round_trip(
        &mut connection,
        &mut dashboard,
        90_002,
        Request::DashboardGeometry {
            size: ovrcr::protocol::TerminalSize { rows: 24, cols: 80 },
        },
    ));
    startup.extend(dashboard_round_trip(
        &mut connection,
        &mut dashboard,
        90_003,
        Request::ReconcileInitialPromptIntents,
    ));
    if !startup.iter().any(|message| {
        matches!(
            message,
            ServerMessage::Event(ServerEvent::SettingsChanged(_))
        )
    }) {
        wait_for_dashboard_settings(&mut connection, &mut dashboard);
    }

    dashboard.key(KeyCode::Char('n'));
    let opening = dashboard.drain_outbox();
    assert_eq!(
        opening.len(),
        2,
        "new agent form refreshes registry before labels"
    );
    assert_eq!(opening[0].request, Request::Inspect);
    assert_eq!(opening[1].request, Request::List);
    drive_dashboard_requests(&mut connection, &mut dashboard, opening);

    for character in "feature/dashboard-agent".chars() {
        dashboard.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    dashboard.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    dashboard.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for character in "fixture-agent".chars() {
        dashboard.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    dashboard.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    let launch = dashboard.drain_outbox();
    assert!(
        launch
            .iter()
            .any(|message| message.request == Request::AllocateInitialPromptIntent),
        "expected the launch intent allocation: {launch:?}"
    );
    let launch_responses = drive_dashboard_requests(&mut connection, &mut dashboard, launch);

    let deadline = Instant::now() + Duration::from_secs(8);
    let (hierarchy, session) = loop {
        let Response::Hierarchy(hierarchy) = live.request(Request::List) else {
            panic!("server list must return current hierarchy");
        };
        let session = hierarchy
            .projects
            .iter()
            .find(|project| project.name == PROJECT)
            .and_then(|project| {
                project
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == WORKSPACE)
            })
            .and_then(|workspace| {
                workspace.sessions.iter().find(|session| {
                    matches!(&session.kind, SessionKind::Agent { name } if name == "fixture-agent")
                        && session.phase == SessionPhase::Running
                })
            })
            .cloned();
        if let Some(session) = session {
            break (hierarchy, session);
        }
        assert!(
            Instant::now() < deadline,
            "Dashboard did not start fixture agent on {WORKSPACE}; responses={launch_responses:?}, hierarchy={hierarchy:?}"
        );
        thread::park_timeout(Duration::from_millis(10));
    };
    let workspace = hierarchy
        .projects
        .iter()
        .find(|project| project.name == PROJECT)
        .and_then(|project| {
            project
                .workspaces
                .iter()
                .find(|workspace| workspace.id == WORKSPACE)
        })
        .expect("the selected registered workspace remains present");
    assert_eq!(workspace.name, "feature/dashboard-agent");
    assert_eq!(session.workspace, WORKSPACE);
    assert_eq!(session.cwd, live.workspace_path(WORKSPACE));
    for group in live.session_groups() {
        live.own_group(group);
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    let terminal = loop {
        if let Response::TerminalText { text, .. } = live.request(Request::ReadTerminal {
            session: session.id,
            max_lines: Some(20),
        }) && text.contains("FIXTURE_AGENT_READY")
        {
            break text;
        }
        assert!(
            Instant::now() < deadline,
            "agent PTY did not show its startup output"
        );
        thread::park_timeout(Duration::from_millis(10));
    };
    assert!(terminal.contains("FIXTURE_AGENT_READY"));
}

fn next_settings_report(connection: &mut UnixStream) -> SettingsReport {
    connection
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    loop {
        match read_frame::<ServerMessage>(connection).expect("read Dashboard settings event") {
            ServerMessage::Event(ServerEvent::SettingsChanged(report)) => return *report,
            ServerMessage::Event(_) => {}
            ServerMessage::Response {
                request_id,
                response,
            } => {
                panic!("unexpected Dashboard response {request_id}: {response:?}")
            }
        }
    }
}

fn allocate_intent(connection: &mut UnixStream) -> InitialPromptIntentId {
    match client::request(connection, 2, Request::AllocateInitialPromptIntent).unwrap() {
        Response::InitialPromptIntent(id) => id,
        other => panic!("expected allocated Initial prompt intent, got {other:?}"),
    }
}

fn submit_without_reading_response(
    connection: &mut UnixStream,
    request_id: u64,
    request: InitialPromptLaunchRequest,
) {
    write_frame(
        connection,
        &ClientMessage {
            request_id,
            request: Request::CreateAgentSessionWithPrompt(request),
        },
    )
    .unwrap();
}

fn discard_unread_response(connection: &mut UnixStream) {
    // The caller waits for the native-process effect before closing, proving
    // admission succeeded while leaving the response unobserved.
    let _ = connection.shutdown(std::net::Shutdown::Both);
}

fn reconcile(
    connection: &mut UnixStream,
    request_id: u64,
    id: &InitialPromptIntentId,
) -> InitialPromptIntentSnapshot {
    let Response::InitialPromptIntents(snapshots) = client::request(
        connection,
        request_id,
        Request::ReconcileInitialPromptIntents,
    )
    .unwrap() else {
        panic!("expected Initial prompt reconciliation response")
    };
    snapshots
        .into_iter()
        .find(|snapshot| snapshot.id == *id)
        .unwrap_or_else(|| panic!("intent {id:?} is missing from reconciliation"))
}

fn wait_for_intent(
    connection: &mut UnixStream,
    id: &InitialPromptIntentId,
    expected: InitialPromptIntentState,
    request_id: &mut u64,
) -> InitialPromptIntentSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = reconcile(connection, *request_id, id);
        *request_id += 1;
        if snapshot.state == expected {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "intent did not reach {expected:?}; last state was {:?}",
            snapshot.state
        );
        thread::park_timeout(Duration::from_millis(10));
    }
}

fn wait_for_capture(capture: &Path, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let contents = std::fs::read_to_string(capture).unwrap_or_default();
        if contents.contains(needle) {
            return contents;
        }
        assert!(
            Instant::now() < deadline,
            "fake Claude did not capture {needle:?}; last output: {contents:?}"
        );
        thread::park_timeout(Duration::from_millis(10));
    }
}

fn fake_launch_count(capture: &str) -> usize {
    capture.matches(FAKE_LAUNCH_MARKER).count()
}

fn assert_retained_storage_excludes(live: &Live, marker: &str) {
    let database = ovrcr::config::database_path(&live.config);
    assert!(
        database.is_file(),
        "retained metadata database was not created"
    );
    let mut files = vec![database.clone()];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut path = database.as_os_str().to_os_string();
        path.push(suffix);
        files.push(PathBuf::from(path));
    }
    for path in files {
        match std::fs::read(&path) {
            Ok(bytes) => assert!(
                !bytes
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes()),
                "retained storage {} contains prompt bytes",
                path.display()
            ),
            Err(error) if path != database && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("read retained storage {}: {error}", path.display()),
        }
    }
}

fn assert_public_stores_exclude(live: &Live, marker: &str) -> bool {
    let settings = std::fs::read_to_string(live.config.join("dashboard.toml")).unwrap();
    let Response::Inventory { registry, sessions } = live.request(Request::Inspect) else {
        panic!("expected retained metadata inventory")
    };
    let inventory = serde_json::to_string(&(registry, sessions)).unwrap();
    let Response::Events(events) = live.request(Request::Events { follow: false }) else {
        panic!("expected OVRCR EventLog snapshot")
    };
    let event_ring_empty = events.is_empty();
    let events = serde_json::to_string(&events).unwrap();
    assert!(!settings.contains(marker), "settings contain prompt text");
    assert!(
        !inventory.contains(marker),
        "retained metadata contains prompt text"
    );
    assert!(!events.contains(marker), "OVRCR Events contain prompt text");
    assert_retained_storage_excludes(live, marker);
    event_ring_empty
}

fn assert_logs_exclude(live: &Live, log: &Path, marker: &str, event_ring_empty: bool) {
    let stderr = std::fs::read_to_string(log).expect("Server stderr capture exists");
    assert!(
        !stderr.contains(marker),
        "Server stderr contains prompt text"
    );
    assert_retained_storage_excludes(live, marker);

    let event_path = ovrcr::config::events_log_path(&live.config);
    assert_eq!(event_path, live.config.join("events.jsonl"));
    match std::fs::read(&event_path) {
        Ok(event_log) => assert!(
            !event_log
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()),
            "{} contains prompt bytes",
            event_path.display()
        ),
        Err(error) if event_ring_empty && error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("read EventLog {}: {error}", event_path.display()),
    }
}

#[test]
fn lost_response_reconnect_reconciles_one_exact_launch_and_duplicate_does_not_replay() {
    let live = Live::idle().bounded();
    let (fake_claude, capture) = install_fake_claude(&live);
    let log = live.root.path().join("server.stderr.log");
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server(&live, &fake_claude, &capture, Some(&log), false);
    live.ready(BRANCH);

    let prompt = "  T392_PROMPT_MARKER_6da4981f\nsecond line belongs to the same prompt  ";
    let launch = detected_claude_launch(
        &live,
        &fake_claude,
        "initial-prompt-session",
        AgentModelPolicy::HarnessDefault,
    );
    let mut first_dashboard = dashboard(&live);
    let intent = allocate_intent(&mut first_dashboard);
    let first_request = InitialPromptLaunchRequest {
        intent: intent.clone(),
        launch,
        prompt: prompt.into(),
    };

    // Leave the response unread, then wait for the real PTY child to reach
    // our owned stand-in executable before reconnecting.
    submit_without_reading_response(&mut first_dashboard, 3, first_request.clone());
    let output = wait_for_capture(&capture, "T392_PROMPT_MARKER_6da4981f");
    assert_eq!(
        fake_launch_count(&output),
        1,
        "one native process was launched"
    );
    let exact_payload = format!("ARG:--\nARG:{prompt}\n");
    assert_eq!(
        output.matches(&exact_payload).count(),
        1,
        "the fake CLI receives the exact raw multiline prompt once"
    );
    discard_unread_response(&mut first_dashboard);

    let mut reconnected_dashboard = dashboard(&live);
    let mut request_id = 2;
    let reconciled = wait_for_intent(
        &mut reconnected_dashboard,
        &intent,
        InitialPromptIntentState::LaunchAccepted,
        &mut request_id,
    );
    let session = reconciled
        .session
        .expect("accepted intent has a session id");
    let run = reconciled.run.expect("accepted intent has a run id");
    assert_eq!(reconciled.project, PROJECT);
    assert_eq!(reconciled.workspace, WORKSPACE);
    assert_eq!(reconciled.name, "initial-prompt-session");
    assert_eq!(reconciled.recoverable_prompt, None);

    let duplicate = client::request(
        &mut reconnected_dashboard,
        request_id,
        Request::CreateAgentSessionWithPrompt(first_request.clone()),
    )
    .unwrap();
    let Response::InitialPromptIntents(duplicate) = duplicate else {
        panic!("repeated intent must reconcile to its accepted state: {duplicate:?}");
    };
    assert_eq!(duplicate, vec![reconciled.clone()]);

    drop(reconnected_dashboard);
    let mut settings_dashboard = dashboard(&live);
    let remembered = next_settings_report(&mut settings_dashboard);
    assert_eq!(
        remembered.settings.launch_choices.get(PROJECT),
        Some(&LaunchChoice::Agent {
            preset: "claude".into(),
            model_policy: AgentModelPolicy::HarnessDefault,
        }),
        "accepted prompt launch remembers its per-project choice after the response is lost"
    );

    assert_eq!(
        live.request(Request::SetSetting {
            path: format!("launch_choices.{}", toml_edit::Key::new(PROJECT)),
            value: Some("{ kind = \"Terminal\" }".into()),
        }),
        Response::Ok
    );
    let updated = next_settings_report(&mut settings_dashboard);
    assert_eq!(
        updated.settings.launch_choices.get(PROJECT),
        Some(&LaunchChoice::Terminal),
        "the newer project choice is visible through Dashboard settings"
    );

    assert_eq!(
        client::request(
            &mut settings_dashboard,
            2,
            Request::CreateAgentSessionWithPrompt(first_request),
        )
        .unwrap(),
        Response::InitialPromptIntents(vec![reconciled.clone()]),
        "repeating the accepted intent reconciles without replaying its older choice"
    );
    drop(settings_dashboard);
    let mut readback_dashboard = dashboard(&live);
    let readback = next_settings_report(&mut readback_dashboard);
    assert_eq!(
        readback.settings.launch_choices.get(PROJECT),
        Some(&LaunchChoice::Terminal),
        "reconciling an older accepted intent preserves the newer project choice"
    );

    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("expected retained session inventory")
    };
    let matching: Vec<_> = sessions
        .iter()
        .filter(|entry| entry.id == session && entry.run == run)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "reconciliation names one exact session/run"
    );
    assert_eq!(
        fake_launch_count(&wait_for_capture(&capture, FAKE_LAUNCH_MARKER)),
        1
    );

    let event_ring_empty = assert_public_stores_exclude(&live, "T392_PROMPT_MARKER_6da4981f");

    drop(readback_dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    assert_logs_exclude(&live, &log, "T392_PROMPT_MARKER_6da4981f", event_ring_empty);
}

#[test]
fn old_server_lifetime_intent_is_unknown_after_restart_and_never_replays() {
    let live = Live::idle().bounded();
    let (fake_claude, capture) = install_fake_claude(&live);
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server(&live, &fake_claude, &capture, None, false);
    live.ready(BRANCH);

    let prompt = "T392_RESTART_MARKER_5128ea06";
    let launch = detected_claude_launch(
        &live,
        &fake_claude,
        "restart-bound-prompt",
        AgentModelPolicy::HarnessDefault,
    );
    let mut first_dashboard = dashboard(&live);
    let old_intent = allocate_intent(&mut first_dashboard);
    let request = InitialPromptLaunchRequest {
        intent: old_intent.clone(),
        launch,
        prompt: prompt.into(),
    };
    assert_eq!(
        client::request(
            &mut first_dashboard,
            3,
            Request::CreateAgentSessionWithPrompt(request.clone()),
        )
        .unwrap(),
        Response::Ok
    );
    let mut request_id = 4;
    let accepted = wait_for_intent(
        &mut first_dashboard,
        &old_intent,
        InitialPromptIntentState::LaunchAccepted,
        &mut request_id,
    );
    let old_session = accepted.session.unwrap();
    let old_run = accepted.run.unwrap();
    let capture_before_restart = wait_for_capture(&capture, prompt);
    assert_eq!(fake_launch_count(&capture_before_restart), 1);

    drop(first_dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    start_server(&live, &fake_claude, &capture, None, false);

    let mut current_dashboard = dashboard(&live);
    let new_intent = allocate_intent(&mut current_dashboard);
    assert_ne!(new_intent.server_lifetime, old_intent.server_lifetime);
    let replay_response = client::request(
        &mut current_dashboard,
        3,
        Request::CreateAgentSessionWithPrompt(request),
    )
    .unwrap();
    let Response::InitialPromptIntents(replay) = replay_response else {
        panic!("old lifetime identity must reconcile as unknown: {replay_response:?}");
    };
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].id, old_intent);
    assert_eq!(replay[0].state, InitialPromptIntentState::Unknown);
    assert_eq!(replay[0].session, None);
    assert_eq!(replay[0].recoverable_prompt, None);

    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("expected retained session inventory")
    };
    let matching: Vec<_> = sessions
        .iter()
        .filter(|entry| entry.id == old_session && entry.run == old_run)
        .collect();
    assert_eq!(matching.len(), 1, "the old run remains the only launch");
    let output = std::fs::read_to_string(&capture).unwrap();
    assert_eq!(
        fake_launch_count(&output),
        1,
        "replay did not spawn or send again"
    );

    drop(current_dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
}

#[test]
fn known_preprocess_refusal_recovers_frozen_launch_without_persisting_prompt() {
    let live = Live::idle().bounded();
    let (fake_claude, capture) = install_fake_claude(&live);
    let log = live.root.path().join("server.stderr.log");
    let configured = toml::Value::String(fake_claude.display().to_string());
    write_settings(
        &live,
        &format!(
            "agents = [{{ name = \"claude\", argv = [{configured}] }}]\n[quota]\nenabled = false\n"
        ),
    );
    start_server(&live, &fake_claude, &capture, Some(&log), false);
    live.ready(BRANCH);

    let prompt = "T392_REFUSAL_MARKER_92fcaa70\nrecovery keeps this exact draft";
    let launch = detected_claude_launch(
        &live,
        &fake_claude,
        "refused-prompt-session",
        AgentModelPolicy::Explicit("fixture-model-v3".into()),
    );
    let mut dashboard = dashboard(&live);
    let intent = allocate_intent(&mut dashboard);
    let request = InitialPromptLaunchRequest {
        intent: intent.clone(),
        launch: launch.clone(),
        prompt: prompt.into(),
    };
    assert_eq!(
        client::request(
            &mut dashboard,
            3,
            Request::CreateAgentSessionWithPrompt(request),
        )
        .unwrap(),
        Response::Ok
    );
    let mut request_id = 4;
    let refused = wait_for_intent(
        &mut dashboard,
        &intent,
        InitialPromptIntentState::Refused,
        &mut request_id,
    );
    assert_eq!(refused.session, None);
    assert_eq!(refused.run, None);
    assert_eq!(refused.recoverable_prompt.as_deref(), Some(prompt));
    let recovered_launch = refused
        .recoverable_launch
        .as_ref()
        .expect("known no-spawn refusal retains the launch choices");
    assert_eq!(recovered_launch.session.name, "refused-prompt-session");
    assert_eq!(recovered_launch.session.label.as_deref(), Some("claude"));
    assert_eq!(
        recovered_launch.session.kind,
        SessionKind::Agent {
            name: "claude".into()
        }
    );
    assert_eq!(recovered_launch.session.argv, launch.session.argv);
    assert_eq!(
        recovered_launch.model_policy,
        AgentModelPolicy::Explicit("fixture-model-v3".into())
    );
    assert!(
        !refused
            .message
            .as_deref()
            .unwrap_or_default()
            .contains(prompt)
    );
    assert!(!format!("{refused:?}").contains(prompt));
    let event_ring_empty = assert_public_stores_exclude(&live, "T392_REFUSAL_MARKER_92fcaa70");
    assert!(
        std::fs::read_to_string(&capture)
            .unwrap_or_default()
            .is_empty(),
        "a known pre-process refusal must not start the fake CLI"
    );

    drop(dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    assert_logs_exclude(
        &live,
        &log,
        "T392_REFUSAL_MARKER_92fcaa70",
        event_ring_empty,
    );
}

#[test]
fn blank_prompt_uses_the_same_deduplicated_interactive_intent() {
    let live = Live::idle().bounded();
    let (fake_claude, capture) = install_fake_claude(&live);
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server(&live, &fake_claude, &capture, None, true);
    live.ready(BRANCH);

    let launch = detected_claude_launch(
        &live,
        &fake_claude,
        "blank-prompt-interactive-session",
        AgentModelPolicy::HarnessDefault,
    );
    let mut dashboard = dashboard(&live);
    let intent = allocate_intent(&mut dashboard);
    let request = InitialPromptLaunchRequest {
        intent: intent.clone(),
        launch,
        prompt: String::new(),
    };
    assert_eq!(
        client::request(
            &mut dashboard,
            3,
            Request::CreateAgentSessionWithPrompt(request.clone()),
        )
        .unwrap(),
        Response::Ok
    );
    let mut request_id = 4;
    let accepted = wait_for_intent(
        &mut dashboard,
        &intent,
        InitialPromptIntentState::LaunchAccepted,
        &mut request_id,
    );
    let session = accepted
        .session
        .expect("interactive launch has a session id");
    let run = accepted.run.expect("interactive launch has a run id");
    let first_capture = wait_for_capture(&capture, FAKE_LAUNCH_MARKER);
    assert_eq!(fake_launch_count(&first_capture), 1);
    let launch_args = first_capture
        .split_once(FAKE_LAUNCH_MARKER)
        .unwrap()
        .1
        .split_once("INPUT:")
        .map_or(first_capture.as_str(), |(args, _)| args);
    assert!(
        !launch_args.contains("\nARG:\n"),
        "blank text is not sent as an argument"
    );
    assert!(
        !launch_args.contains("\nARG:--\n"),
        "no prompt separator is sent"
    );

    let duplicate = client::request(
        &mut dashboard,
        request_id,
        Request::CreateAgentSessionWithPrompt(request),
    )
    .unwrap();
    let Response::InitialPromptIntents(duplicate) = duplicate else {
        panic!("repeated blank intent must reconcile to its accepted state: {duplicate:?}");
    };
    assert_eq!(duplicate, vec![accepted]);

    let input = "T392_INTERACTIVE_INPUT_5ddf28a2";
    assert_eq!(
        live.request(Request::SendTerminal {
            session,
            text: input.into(),
            submit: true,
        }),
        Response::Ok
    );
    let output = wait_for_capture(&capture, &format!("INPUT:{input}"));
    assert_eq!(
        fake_launch_count(&output),
        1,
        "duplicate Start did not spawn twice"
    );
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("expected retained session inventory")
    };
    let matching: Vec<_> = sessions
        .iter()
        .filter(|entry| entry.id == session && entry.run == run)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "the interactive input reached the exact run"
    );

    drop(dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
}
