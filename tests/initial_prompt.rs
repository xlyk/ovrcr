//! Live, production-path Initial prompt intent regressions.
//!
//! The owned `claude` and `pi` executables below are native CLI stand-ins.
//! Their captures prove what OVRCR passed to those processes; they are not
//! provider acceptance evidence.
#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT, WORKSPACE};
use ovrcr::protocol::{
    AgentModelPolicy, ClientMessage, CreateAgentSessionRequest, CreateSessionRequest, ErrorCode,
    InitialPromptIntentId, InitialPromptIntentSnapshot, InitialPromptIntentState,
    InitialPromptLaunchRequest, LaunchChoice, Request, Response, ServerEvent, ServerMessage,
    SessionKind, SettingsReport, client, connect_server, read_frame, write_frame,
};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const CAPTURE_ENV: &str = "OVRCR_TEST_FAKE_CLAUDE_CAPTURE";
const INTERACTIVE_ENV: &str = "OVRCR_TEST_FAKE_CLAUDE_INTERACTIVE";
const FAKE_LAUNCH_MARKER: &str = "__OVRCR_FAKE_CLAUDE_LAUNCH__";
const PI_CAPTURE_ENV: &str = "OVRCR_TEST_FAKE_PI_CAPTURE";
const PI_CATALOG_ENV: &str = "OVRCR_TEST_FAKE_PI_CATALOG";
const FAKE_PI_LAUNCH_MARKER: &str = "__OVRCR_FAKE_PI_LAUNCH__";
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

fn install_fake_pi(live: &Live) -> (PathBuf, PathBuf, PathBuf) {
    let bin = live.root.path().join("fake-pi-bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("pi");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\n\nfor argument in \"$@\"; do\n  if [ \"$argument\" = \"--list-models\" ]; then\n    printf '%s\\n' '__OVRCR_FAKE_PI_CATALOG__' >> \"${PI_CAPTURE_ENV}\"\n    for catalog_argument in \"$@\"; do printf 'CATALOG_ARG:%s\\n' \"$catalog_argument\" >> \"${PI_CAPTURE_ENV}\"; done\n    cat \"${PI_CATALOG_ENV}\"\n    exit 0\n  fi\ndone\nprintf '%s\\n' '{FAKE_PI_LAUNCH_MARKER}' >> \"${PI_CAPTURE_ENV}\"\nfor argument in \"$@\"; do printf 'ARG:%s\\n' \"$argument\" >> \"${PI_CAPTURE_ENV}\"; done\nexit 0\n"
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    let capture = live.root.path().join("fake-pi-argv.txt");
    let catalog = live.root.path().join("fake-pi-models.txt");
    std::fs::write(
        &catalog,
        "provider  model                     context  max-out  thinking  images\n\
         openai    gpt-5.4                   256K     64K      yes       yes\n\
         anthropic claude-sonnet-4-5         200K     16K      yes       yes\n",
    )
    .unwrap();
    (executable, capture, catalog)
}

fn start_server_with_pi(live: &Live, fake_pi: &Path, capture: &Path, catalog: &Path) {
    let mut entries = vec![fake_pi.parent().unwrap().to_path_buf()];
    entries.extend(std::env::split_paths(&live.path()));
    let path = std::env::join_paths(entries).unwrap();
    live.start_binary_env(&[
        ("PATH", path.as_os_str()),
        (PI_CAPTURE_ENV, capture.as_os_str()),
        (PI_CATALOG_ENV, catalog.as_os_str()),
    ]);
}

fn detected_pi_launch(
    live: &Live,
    fake_pi: &Path,
    name: &str,
    model_policy: AgentModelPolicy,
) -> CreateAgentSessionRequest {
    let mut native = vec![fake_pi.canonicalize().unwrap().into_os_string()];
    ovrcr::protocol::apply_default_auto_trust("pi", &mut native);
    let mut argv = vec![
        live.executable.canonicalize().unwrap().into_os_string(),
        "agent".into(),
        "run".into(),
        "pi".into(),
        "--".into(),
    ];
    argv.extend(native);
    CreateAgentSessionRequest {
        session: CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: name.into(),
            label: Some("pi".into()),
            argv,
            kind: SessionKind::Agent { name: "pi".into() },
        },
        model_policy,
        model_binding: None,
        expected_model: None,
    }
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

fn query_pi_models(
    connection: &mut UnixStream,
    request_id: u64,
    launch: &CreateAgentSessionRequest,
) -> ovrcr::protocol::AgentModelCapabilities {
    assert_eq!(
        client::request(
            connection,
            request_id,
            Request::QueryAgentModels {
                preset: "pi".into(),
                origin: ovrcr::protocol::AgentLaunchOrigin::Detected,
                argv: launch.session.argv.clone(),
                project: launch.session.project.clone(),
                workspace: launch.session.workspace.clone(),
            },
        )
        .unwrap(),
        Response::Ok
    );
    loop {
        match read_frame::<ServerMessage>(connection).expect("read Pi model capability event") {
            ServerMessage::Event(ServerEvent::AgentModels {
                request_id: event_id,
                capabilities,
            }) if event_id == request_id => return capabilities,
            ServerMessage::Event(_) => {}
            ServerMessage::Response {
                request_id: event_id,
                response,
            } => panic!("unexpected Dashboard response {event_id}: {response:?}"),
        }
    }
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

#[test]
fn pi_model_catalog_and_initial_prompt_use_the_detected_native_launcher() {
    let live = Live::idle().bounded();
    let (fake_pi, capture, catalog) = install_fake_pi(&live);
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server_with_pi(&live, &fake_pi, &capture, &catalog);
    live.ready(BRANCH);

    let mut dashboard = dashboard(&live);
    let mut launch = detected_pi_launch(
        &live,
        &fake_pi,
        "pi-explicit-prompt-session",
        AgentModelPolicy::HarnessDefault,
    );
    let capabilities = query_pi_models(&mut dashboard, 3, &launch);
    assert_eq!(capabilities.preset, "pi");
    assert!(capabilities.reason.is_none(), "{capabilities:?}");
    let binding = capabilities
        .binding
        .expect("Pi choices carry a launch binding");
    assert_eq!(
        capabilities.models,
        vec![
            ovrcr::protocol::AgentModelOption {
                alias: "anthropic/claude-sonnet-4-5".into(),
                resolved: "anthropic/claude-sonnet-4-5".into(),
            },
            ovrcr::protocol::AgentModelOption {
                alias: "openai/gpt-5.4".into(),
                resolved: "openai/gpt-5.4".into(),
            },
        ],
        "the picker exposes exact provider/model IDs from the effective Pi catalog"
    );

    let intent = allocate_intent(&mut dashboard);
    launch.model_policy = AgentModelPolicy::Explicit("openai/gpt-5.4".into());
    launch.model_binding = Some(binding);
    launch.expected_model = Some("openai/gpt-5.4".into());
    let prompt = "T394_PI_PROMPT_MARKER_731b9a44\nsecond line remains one argument";
    let request = InitialPromptLaunchRequest {
        intent: intent.clone(),
        launch,
        prompt: prompt.into(),
    };
    assert_eq!(
        client::request(
            &mut dashboard,
            4,
            Request::CreateAgentSessionWithPrompt(request.clone()),
        )
        .unwrap(),
        Response::Ok
    );
    let mut request_id = 5;
    let accepted = wait_for_intent(
        &mut dashboard,
        &intent,
        InitialPromptIntentState::LaunchAccepted,
        &mut request_id,
    );
    assert_eq!(accepted.name, "pi-explicit-prompt-session");

    let output = wait_for_capture(&capture, prompt);
    let launch_output = output
        .split_once(FAKE_PI_LAUNCH_MARKER)
        .expect("the detected Pi process started")
        .1;
    assert!(
        launch_output.contains("ARG:--model\nARG:openai/gpt-5.4\nARG:--approve\nARG:--\n"),
        "explicit selection stays inside Pi's native argv and before its prompt delimiter: {launch_output:?}"
    );
    assert!(
        launch_output.contains(&format!("ARG:{prompt}\n")),
        "the complete multiline prompt reaches the Pi process as one argv item"
    );
    assert!(
        output.matches("__OVRCR_FAKE_PI_CATALOG__").count() >= 2,
        "capability lookup and Start each consult the current model catalog"
    );
    assert!(
        !output.contains("CATALOG_ARG:--no-extensions")
            && !output.contains("CATALOG_ARG:--no-skills"),
        "model discovery uses the same effective extension/trust boundary as the interactive launcher"
    );

    drop(dashboard);
    let mut dashboard = self::dashboard(&live);
    let reconciled = reconcile(&mut dashboard, 2, &intent);
    assert_eq!(reconciled, accepted);
    assert_eq!(reconciled.session, accepted.session);
    assert_eq!(reconciled.run, accepted.run);
    assert_eq!(
        client::request(
            &mut dashboard,
            3,
            Request::CreateAgentSessionWithPrompt(request),
        )
        .unwrap(),
        Response::InitialPromptIntents(vec![accepted.clone()]),
        "replaying a Pi intent after reconnect returns the accepted launch"
    );
    assert_eq!(
        std::fs::read_to_string(&capture)
            .unwrap_or_default()
            .matches(FAKE_PI_LAUNCH_MARKER)
            .count(),
        1,
        "duplicate accepted Pi intent does not spawn again"
    );

    let blank_intent =
        match client::request(&mut dashboard, 4, Request::AllocateInitialPromptIntent).unwrap() {
            Response::InitialPromptIntent(intent) => intent,
            other => panic!("expected allocated blank Pi intent, got {other:?}"),
        };
    let blank_launch = detected_pi_launch(
        &live,
        &fake_pi,
        "pi-blank-interactive-session",
        AgentModelPolicy::HarnessDefault,
    );
    assert_eq!(
        client::request(
            &mut dashboard,
            5,
            Request::CreateAgentSessionWithPrompt(InitialPromptLaunchRequest {
                intent: blank_intent.clone(),
                launch: blank_launch,
                prompt: String::new(),
            }),
        )
        .unwrap(),
        Response::Ok
    );
    let mut request_id = 6;
    let blank_accepted = wait_for_intent(
        &mut dashboard,
        &blank_intent,
        InitialPromptIntentState::LaunchAccepted,
        &mut request_id,
    );
    assert_eq!(blank_accepted.name, "pi-blank-interactive-session");
    let deadline = Instant::now() + Duration::from_secs(10);
    let output = loop {
        let contents = std::fs::read_to_string(&capture).unwrap_or_default();
        if contents.matches(FAKE_PI_LAUNCH_MARKER).count() >= 2
            && contents
                .rsplit_once(FAKE_PI_LAUNCH_MARKER)
                .is_some_and(|(_, last_launch)| last_launch.contains("ARG:--approve\n"))
        {
            break contents;
        }
        assert!(
            Instant::now() < deadline,
            "second fake Pi launch did not start; last output: {contents:?}"
        );
        thread::park_timeout(Duration::from_millis(10));
    };
    let blank_launch_output = output
        .rsplit_once(FAKE_PI_LAUNCH_MARKER)
        .expect("the second detected Pi process started")
        .1;
    assert!(
        blank_launch_output.contains("ARG:--approve\n"),
        "blank prompt keeps the detected trust option: {blank_launch_output:?}"
    );
    assert!(
        !blank_launch_output.contains("ARG:--model\n") && !blank_launch_output.contains("ARG:--\n"),
        "blank prompt leaves Pi interactive with Harness default"
    );

    drop(dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
}

#[test]
fn pi_start_refuses_a_model_removed_from_the_fresh_native_catalog() {
    let live = Live::idle().bounded();
    let (fake_pi, capture, catalog) = install_fake_pi(&live);
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server_with_pi(&live, &fake_pi, &capture, &catalog);
    live.ready(BRANCH);

    let mut dashboard = dashboard(&live);
    let mut launch = detected_pi_launch(
        &live,
        &fake_pi,
        "pi-stale-model-session",
        AgentModelPolicy::HarnessDefault,
    );
    let capabilities = query_pi_models(&mut dashboard, 3, &launch);
    let binding = capabilities
        .binding
        .expect("Pi choices carry a launch binding");
    launch.model_policy = AgentModelPolicy::Explicit("openai/gpt-5.4".into());
    launch.model_binding = Some(binding);
    launch.expected_model = Some("openai/gpt-5.4".into());
    std::fs::write(
        &catalog,
        "provider  model                     context  max-out  thinking  images\n\
         anthropic claude-sonnet-4-5         200K     16K      yes       yes\n",
    )
    .unwrap();

    let intent = allocate_intent(&mut dashboard);
    let prompt = "T394_PI_STALE_MODEL_PROMPT_41c0e27a\nkeep this draft recoverable";
    let request = InitialPromptLaunchRequest {
        intent: intent.clone(),
        launch,
        prompt: prompt.into(),
    };
    assert_eq!(
        client::request(
            &mut dashboard,
            4,
            Request::CreateAgentSessionWithPrompt(request),
        )
        .unwrap(),
        Response::Ok
    );
    let mut request_id = 5;
    let refused = wait_for_intent(
        &mut dashboard,
        &intent,
        InitialPromptIntentState::Refused,
        &mut request_id,
    );
    assert_eq!(refused.recoverable_prompt.as_deref(), Some(prompt));
    assert!(
        refused
            .message
            .as_deref()
            .is_some_and(|message| message.contains("selected pi model resolution changed")),
        "the exact missing model is refused with an actionable reason: {refused:?}"
    );
    let output = std::fs::read_to_string(&capture).unwrap_or_default();
    assert!(
        output.contains("__OVRCR_FAKE_PI_CATALOG__"),
        "the initial lookup ran"
    );
    assert_eq!(
        output.matches("__OVRCR_FAKE_PI_CATALOG__").count(),
        2,
        "Start consults the current native catalog again"
    );
    assert!(
        !output.contains(FAKE_PI_LAUNCH_MARKER),
        "the removed model never starts a Pi session"
    );
    assert!(!format!("{refused:?}").contains("T394_PI_STALE_MODEL_PROMPT_41c0e27a"));

    drop(dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
}
