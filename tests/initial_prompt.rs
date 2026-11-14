//! Live, production-path Initial prompt intent regressions.
//!
//! The owned `claude`, `pi`, `omp`, and `agy` executables below are native CLI stand-ins.
//! Their captures prove what OVRCR passed to those processes; they are not
//! provider acceptance evidence.
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
const PI_CAPTURE_ENV: &str = "OVRCR_TEST_FAKE_PI_CAPTURE";
const PI_CATALOG_ENV: &str = "OVRCR_TEST_FAKE_PI_CATALOG";
const FAKE_PI_LAUNCH_MARKER: &str = "__OVRCR_FAKE_PI_LAUNCH__";
const PI_CALLS_ENV: &str = "OVRCR_TEST_FAKE_PI_CALLS";
const PI_PIDS_ENV: &str = "OVRCR_TEST_FAKE_PI_PIDS";
const PI_BLOCK_ENV: &str = "OVRCR_TEST_FAKE_PI_BLOCK";
const PI_STARTED_ENV: &str = "OVRCR_TEST_FAKE_PI_STARTED";
const PI_RELEASE_ENV: &str = "OVRCR_TEST_FAKE_PI_RELEASE";
const OMP_CAPTURE_ENV: &str = "OVRCR_TEST_FAKE_OMP_CAPTURE";
const OMP_CATALOG_ENV: &str = "OVRCR_TEST_FAKE_OMP_CATALOG";
const FAKE_OMP_LAUNCH_MARKER: &str = "__OVRCR_FAKE_OMP_LAUNCH__";
const AGY_CAPTURE_ENV: &str = "OVRCR_TEST_FAKE_AGY_CAPTURE";
const AGY_CATALOG_ENV: &str = "OVRCR_TEST_FAKE_AGY_CATALOG";
const FAKE_AGY_LAUNCH_MARKER: &str = "__OVRCR_FAKE_AGY_LAUNCH__";
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

struct GatedPi {
    executable: PathBuf,
    capture: PathBuf,
    catalog: PathBuf,
    calls: PathBuf,
    pids: PathBuf,
    block: PathBuf,
    started: PathBuf,
    release: PathBuf,
    server_log: PathBuf,
}

fn install_gated_pi(live: &Live) -> GatedPi {
    let bin = live.root.path().join("gated-pi-bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("pi");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nfor argument in \"$@\"; do\n  if [ \"$argument\" = \"--list-models\" ]; then\n    printf 'catalog\\n' >> \"${PI_CALLS_ENV}\"\n    printf '%s\\n' \"$$\" >> \"${PI_PIDS_ENV}\"\n    if [ -e \"${PI_BLOCK_ENV}\" ]; then\n      printf '%s' \"$$\" > \"${PI_STARTED_ENV}\"\n      while [ ! -e \"${PI_RELEASE_ENV}\" ]; do /bin/sleep 0.01; done\n    fi\n    cat \"${PI_CATALOG_ENV}\"\n    exit 0\n  fi\ndone\nprintf '%s\\n' '{FAKE_PI_LAUNCH_MARKER}' >> \"${PI_CAPTURE_ENV}\"\nexit 0\n"
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    let catalog = live.root.path().join("gated-pi-models.txt");
    std::fs::write(
        &catalog,
        "provider  model                     context  max-out  thinking  images\n\
         openai    gpt-5.4                   256K     64K      yes       yes\n",
    )
    .unwrap();
    GatedPi {
        executable,
        capture: live.root.path().join("gated-pi-argv.txt"),
        catalog,
        calls: live.root.path().join("gated-pi-calls.txt"),
        pids: live.root.path().join("gated-pi-pids.txt"),
        block: live.root.path().join("gated-pi-block"),
        started: live.root.path().join("gated-pi-started"),
        release: live.root.path().join("gated-pi-release"),
        server_log: live.root.path().join("gated-pi-server.stderr.log"),
    }
}

fn start_server_with_gated_pi(live: &Live, fake: &GatedPi) {
    let mut entries = vec![fake.executable.parent().unwrap().to_path_buf()];
    entries.extend(std::env::split_paths(&live.path()));
    let path = std::env::join_paths(entries).unwrap();
    live.start_binary_logged(
        &[
            ("PATH", path.as_os_str()),
            (PI_CAPTURE_ENV, fake.capture.as_os_str()),
            (PI_CATALOG_ENV, fake.catalog.as_os_str()),
            (PI_CALLS_ENV, fake.calls.as_os_str()),
            (PI_PIDS_ENV, fake.pids.as_os_str()),
            (PI_BLOCK_ENV, fake.block.as_os_str()),
            (PI_STARTED_ENV, fake.started.as_os_str()),
            (PI_RELEASE_ENV, fake.release.as_os_str()),
        ],
        &fake.server_log,
    );
}

fn wait_for_path(path: &Path, description: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(Instant::now() < deadline, "{description} did not appear");
        thread::sleep(Duration::from_millis(5));
    }
}

fn pi_catalog_call_count(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .count()
}

fn owned_pi_groups_absent(pids: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(pids) else {
        return false;
    };
    contents.lines().all(|line| {
        let Ok(pid) = line.parse::<libc::pid_t>() else {
            return false;
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = unsafe { libc::kill(-pid, 0) };
            if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(5));
        }
    })
}

fn owned_pi_processes_absent(pids: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(pids) else {
        return false;
    };
    !contents.trim().is_empty()
        && contents.lines().all(|line| {
            let Ok(pid) = line.parse::<libc::pid_t>() else {
                return false;
            };
            let result = unsafe { libc::kill(pid, 0) };
            result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        })
}

struct ReleaseGateOnDrop(PathBuf);

impl Drop for ReleaseGateOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, "release\n");
    }
}

fn install_fake_omp(live: &Live) -> (PathBuf, PathBuf, PathBuf) {
    let bin = live.root.path().join("fake-omp-bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("omp");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nfor argument in \"$@\"; do\n  if [ \"$argument\" = \"models\" ]; then\n    printf '%s\\n' '__OVRCR_FAKE_OMP_CATALOG__' >> \"${OMP_CAPTURE_ENV}\"\n    for catalog_argument in \"$@\"; do printf 'CATALOG_ARG:%s\\n' \"$catalog_argument\" >> \"${OMP_CAPTURE_ENV}\"; done\n    cat \"${OMP_CATALOG_ENV}\"\n    exit 0\n  fi\ndone\nprintf '%s\\n' '{FAKE_OMP_LAUNCH_MARKER}' >> \"${OMP_CAPTURE_ENV}\"\nfor argument in \"$@\"; do printf 'ARG:%s\\n' \"$argument\" >> \"${OMP_CAPTURE_ENV}\"; done\nexit 0\n"
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    let capture = live.root.path().join("fake-omp-argv.txt");
    let catalog = live.root.path().join("fake-omp-models.json");
    write_omp_catalog(&catalog, true);
    (executable, capture, catalog)
}

fn write_omp_catalog(catalog: &Path, include_openai: bool) {
    let mut models = String::new();
    if include_openai {
        models.push_str(
            r#"{"provider":"openai","kind":"chat","id":"gpt-5.4","selector":"openai/gpt-5.4","name":"GPT-5.4"},"#,
        );
    }
    models.push_str(
        r#"{"provider":"anthropic","kind":"chat","id":"claude-sonnet-4-5","selector":"anthropic/claude-sonnet-4-5","name":"Claude Sonnet 4.5"}"#,
    );
    std::fs::write(catalog, format!(r#"{{"models":[{models}]}}"#)).unwrap();
}

fn start_server_with_omp(
    live: &Live,
    fake_omp: &Path,
    capture: &Path,
    catalog: &Path,
    log: Option<&Path>,
) {
    let mut entries = vec![fake_omp.parent().unwrap().to_path_buf()];
    entries.extend(std::env::split_paths(&live.path()));
    let path = std::env::join_paths(entries).unwrap();
    let environment = [
        ("PATH", path.as_os_str()),
        (OMP_CAPTURE_ENV, capture.as_os_str()),
        (OMP_CATALOG_ENV, catalog.as_os_str()),
    ];
    if let Some(log) = log {
        live.start_binary_logged(&environment, log);
    } else {
        live.start_binary_env(&environment);
    }
}

fn install_fake_agy(live: &Live) -> (PathBuf, PathBuf, PathBuf) {
    let bin = live.root.path().join("fake-agy-bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("agy");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nfor argument in \"$@\"; do\n  if [ \"$argument\" = \"models\" ]; then\n    printf '%s\\n' '__OVRCR_FAKE_AGY_CATALOG__' >> \"${AGY_CAPTURE_ENV}\"\n    for catalog_argument in \"$@\"; do printf 'CATALOG_ARG:%s\\n' \"$catalog_argument\" >> \"${AGY_CAPTURE_ENV}\"; done\n    printf 'Fetching available models...\\n' >&2\n    cat \"${AGY_CATALOG_ENV}\"\n    exit 0\n  fi\ndone\nprintf '%s\\n' '{FAKE_AGY_LAUNCH_MARKER}' >> \"${AGY_CAPTURE_ENV}\"\nfor argument in \"$@\"; do printf 'ARG:%s\\n' \"$argument\" >> \"${AGY_CAPTURE_ENV}\"; done\nexit 0\n"
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    let capture = live.root.path().join("fake-agy-argv.txt");
    let catalog = live.root.path().join("fake-agy-models.tsv");
    std::fs::write(
        &catalog,
        "gemini-4-argon-high\tGemini 4 Argon (High)\n\
         gemini-3.8-flash-low\tGemini 3.8 Flash (Low)\n",
    )
    .unwrap();
    (executable, capture, catalog)
}

fn start_server_with_agy(live: &Live, fake_agy: &Path, capture: &Path, catalog: &Path) {
    let mut entries = vec![fake_agy.parent().unwrap().to_path_buf()];
    entries.extend(std::env::split_paths(&live.path()));
    let path = std::env::join_paths(entries).unwrap();
    live.start_binary_env(&[
        ("PATH", path.as_os_str()),
        (AGY_CAPTURE_ENV, capture.as_os_str()),
        (AGY_CATALOG_ENV, catalog.as_os_str()),
    ]);
}

fn detected_agy_launch(
    live: &Live,
    fake_agy: &Path,
    name: &str,
    model_policy: AgentModelPolicy,
) -> CreateAgentSessionRequest {
    let mut native = vec![fake_agy.canonicalize().unwrap().into_os_string()];
    ovrcr::protocol::apply_default_auto_trust("agy", &mut native);
    let mut argv = vec![
        live.executable.canonicalize().unwrap().into_os_string(),
        "agent".into(),
        "run".into(),
        "agy".into(),
        "--".into(),
    ];
    argv.extend(native);
    CreateAgentSessionRequest {
        session: CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: name.into(),
            label: Some("agy".into()),
            argv,
            kind: SessionKind::Agent { name: "agy".into() },
        },
        model_policy,
        model_binding: None,
        expected_model: None,
    }
}

fn detected_omp_launch(
    live: &Live,
    fake_omp: &Path,
    name: &str,
    model_policy: AgentModelPolicy,
) -> CreateAgentSessionRequest {
    let mut native = vec![fake_omp.canonicalize().unwrap().into_os_string()];
    ovrcr::protocol::apply_default_auto_trust("omp", &mut native);
    let mut argv = vec![
        live.executable.canonicalize().unwrap().into_os_string(),
        "agent".into(),
        "run".into(),
        "omp".into(),
        "--".into(),
    ];
    argv.extend(native);
    CreateAgentSessionRequest {
        session: CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: name.into(),
            label: Some("omp".into()),
            argv,
            kind: SessionKind::Agent { name: "omp".into() },
        },
        model_policy,
        model_binding: None,
        expected_model: None,
    }
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

    // The new-agent form starts with Project focused. Keep the registered
    // fixture project and move to Branch before filtering for its checkout.
    dashboard.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
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

fn query_omp_models(
    connection: &mut UnixStream,
    request_id: u64,
    launch: &CreateAgentSessionRequest,
) -> ovrcr::protocol::AgentModelCapabilities {
    assert_eq!(
        client::request(
            connection,
            request_id,
            Request::QueryAgentModels {
                preset: "omp".into(),
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
        match read_frame::<ServerMessage>(connection).expect("read OMP model capability event") {
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

fn query_agy_models(
    connection: &mut UnixStream,
    request_id: u64,
    launch: &CreateAgentSessionRequest,
) -> ovrcr::protocol::AgentModelCapabilities {
    assert_eq!(
        client::request(
            connection,
            request_id,
            Request::QueryAgentModels {
                preset: "agy".into(),
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
        match read_frame::<ServerMessage>(connection).expect("read Antigravity model event") {
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

#[test]
fn agy_model_catalog_and_initial_prompt_use_detected_interactive_grammar() {
    let live = Live::idle().bounded();
    let (fake_agy, capture, catalog) = install_fake_agy(&live);
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server_with_agy(&live, &fake_agy, &capture, &catalog);
    live.ready(BRANCH);

    let mut dashboard = dashboard(&live);
    let mut launch = detected_agy_launch(
        &live,
        &fake_agy,
        "agy-explicit-prompt-session",
        AgentModelPolicy::HarnessDefault,
    );
    let capabilities = query_agy_models(&mut dashboard, 3, &launch);
    assert_eq!(capabilities.preset, "agy");
    assert!(capabilities.reason.is_none(), "{capabilities:?}");
    let binding = capabilities
        .binding
        .expect("detected Antigravity choices carry a launch binding");
    assert_eq!(
        capabilities.models,
        vec![
            ovrcr::protocol::AgentModelOption {
                alias: "gemini-3.8-flash-low".into(),
                resolved: "Gemini 3.8 Flash (Low)".into(),
            },
            ovrcr::protocol::AgentModelOption {
                alias: "gemini-4-argon-high".into(),
                resolved: "Gemini 4 Argon (High)".into(),
            },
        ],
        "the model picker exposes catalog IDs while retaining native display labels"
    );

    let intent = allocate_intent(&mut dashboard);
    launch.model_policy = AgentModelPolicy::Explicit("gemini-3.8-flash-low".into());
    launch.model_binding = Some(binding);
    launch.expected_model = Some("Gemini 3.8 Flash (Low)".into());
    let prompt = "-fix the parser first\nthen treat --mode=plan as literal prompt text";
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
    assert_eq!(accepted.name, "agy-explicit-prompt-session");
    assert_eq!(accepted.recoverable_prompt, None);
    let session = accepted.session.expect("intent names the started session");
    assert!(accepted.run.is_some(), "intent names the exact started run");

    let output = wait_for_capture(&capture, "literal prompt text");
    assert_eq!(
        output.matches(FAKE_AGY_LAUNCH_MARKER).count(),
        1,
        "one detected native process received the initial prompt"
    );
    let launch_output = output
        .split_once(FAKE_AGY_LAUNCH_MARKER)
        .expect("the detected Antigravity CLI process started")
        .1;
    assert!(
        launch_output.contains(
            "ARG:--model\nARG:gemini-3.8-flash-low\nARG:--dangerously-skip-permissions\n"
        ),
        "the exact selected model and managed interactive permission default reach agy: {launch_output:?}"
    );
    assert!(
        launch_output.contains(&format!("ARG:--prompt-interactive={prompt}\n")),
        "the complete leading-hyphen multiline prompt is one --prompt-interactive argument"
    );
    assert!(
        !launch_output.contains("ARG:--\n"),
        "agy's prompt is not sent with the generic separator grammar"
    );
    assert_eq!(
        output.matches("__OVRCR_FAKE_AGY_CATALOG__").count(),
        2,
        "model capabilities and launch each revalidate the current catalog"
    );
    assert_eq!(
        output.matches("CATALOG_ARG:models\n").count(),
        2,
        "the native catalog command uses exactly the models subcommand"
    );
    assert!(
        !output.contains("CATALOG_ARG:--dangerously-skip-permissions"),
        "the interactive trust flag is not passed to catalog discovery"
    );

    let duplicate = client::request(
        &mut dashboard,
        request_id,
        Request::CreateAgentSessionWithPrompt(request),
    )
    .unwrap();
    assert_eq!(duplicate, Response::InitialPromptIntents(vec![accepted]));
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("expected retained session inventory")
    };
    assert_eq!(
        sessions.iter().filter(|entry| entry.id == session).count(),
        1,
        "the accepted intent identifies exactly one retained session"
    );
    assert_public_stores_exclude(&live, "literal prompt text");

    drop(dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
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
fn pi_catalog_gate_refuses_fresh_start_and_shutdown_until_cleanup_recovers() {
    let live = Live::idle().bounded();
    let fake = install_gated_pi(&live);
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server_with_gated_pi(&live, &fake);
    live.ready(BRANCH);

    let mut dashboard = dashboard(&live);
    let launch = detected_pi_launch(
        &live,
        &fake.executable,
        "pi-catalog-gate-session",
        AgentModelPolicy::HarnessDefault,
    );
    let initial = query_pi_models(&mut dashboard, 3, &launch);
    let binding = initial.binding.expect("initial catalog binds Pi launcher");
    assert_eq!(pi_catalog_call_count(&fake.calls), 1);

    let _release_gate = ReleaseGateOnDrop(fake.release.clone());
    std::fs::write(&fake.block, "block\n").unwrap();
    assert_eq!(
        client::request(
            &mut dashboard,
            4,
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
    wait_for_path(&fake.started, "blocked Pi catalog child");
    let pid: libc::pid_t = std::fs::read_to_string(&fake.started)
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::getpgid(pid) }, pid);
    assert_eq!(pi_catalog_call_count(&fake.calls), 2);

    let intent = allocate_intent(&mut dashboard);
    let intent_id = intent.clone();
    let mut explicit = launch;
    explicit.model_policy = AgentModelPolicy::Explicit("openai/gpt-5.4".into());
    explicit.model_binding = Some(binding);
    explicit.expected_model = Some("openai/gpt-5.4".into());
    let start = InitialPromptLaunchRequest {
        intent,
        launch: explicit,
        prompt: String::new(),
    };
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 6,
            request: Request::CreateAgentSessionWithPrompt(start),
        },
    )
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while pi_catalog_call_count(&fake.calls) < 3 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }

    let shutdown_while_owned = live.request(Request::Shutdown { kill: true });
    let calls_while_owned = pi_catalog_call_count(&fake.calls);
    std::fs::write(&fake.release, "release\n").unwrap();

    if shutdown_while_owned == Response::Ok {
        live.join();
        assert!(
            owned_pi_groups_absent(&fake.pids),
            "every fake Pi process group must be absent after accepted shutdown"
        );
        assert_eq!(
            calls_while_owned, 2,
            "fresh Start must not bypass the active catalog owner"
        );
        assert_ne!(
            shutdown_while_owned,
            Response::Ok,
            "shutdown must be refused while the catalog worker owns a native child"
        );
    }

    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut start_response = None;
    let mut refused_intent = None;
    let mut catalog_result = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while start_response.is_none() || refused_intent.is_none() || catalog_result.is_none() {
        assert!(Instant::now() < deadline, "gate responses did not finish");
        match read_frame::<ServerMessage>(&mut dashboard).expect("read catalog gate response") {
            ServerMessage::Response {
                request_id: 6,
                response,
            } => start_response = Some(response),
            ServerMessage::Event(ServerEvent::AgentModels {
                request_id: 4,
                capabilities,
            }) => catalog_result = Some(capabilities),
            ServerMessage::Event(ServerEvent::InitialPromptIntentChanged(snapshot))
                if snapshot.id == intent_id
                    && snapshot.state == InitialPromptIntentState::Refused =>
            {
                refused_intent = Some(snapshot)
            }
            ServerMessage::Event(_) | ServerMessage::Response { .. } => {}
        }
    }
    let start_response = start_response.unwrap();
    let refused_intent = refused_intent.unwrap();
    let catalog_result = catalog_result.unwrap();
    let calls_after_recovery = pi_catalog_call_count(&fake.calls);
    let recorded_pids = std::fs::read_to_string(&fake.pids)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let groups_absent = owned_pi_groups_absent(&fake.pids);
    let waited_processes_absent = owned_pi_processes_absent(&fake.pids);
    let server_responded_after_recovery = if matches!(shutdown_while_owned, Response::Ok) {
        false
    } else {
        matches!(live.request(Request::List), Response::Hierarchy(_))
    };

    assert!(matches!(
        shutdown_while_owned,
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        calls_while_owned, 2,
        "fresh Start did not bypass the active catalog owner"
    );
    assert_eq!(
        calls_after_recovery, 2,
        "refused fresh Start must not spawn after catalog recovery"
    );
    assert_eq!(
        recorded_pids.len(),
        2,
        "only initial and held catalog requests should own native children"
    );
    assert_eq!(start_response, Response::Ok);
    assert_eq!(refused_intent.state, InitialPromptIntentState::Refused);
    assert!(
        refused_intent
            .message
            .as_deref()
            .is_some_and(|message| { message.contains("native model catalog process is active") })
    );
    assert!(catalog_result.reason.is_none(), "{catalog_result:?}");
    assert_eq!(catalog_result.models, initial.models);
    assert!(
        groups_absent,
        "a fake Pi process group remained after event delivery"
    );
    assert!(
        waited_processes_absent,
        "a fake Pi leader remained unreaped"
    );
    assert!(
        server_responded_after_recovery,
        "the same Server must answer after recovery and before final Shutdown"
    );
    eprintln!(
        "CATALOG_CLEANUP_RECEIPT calls_while_owned={calls_while_owned} calls_after_recovery={calls_after_recovery} recorded_pids={recorded_pids:?} groups_absent={groups_absent} waited_pids_absent={waited_processes_absent} same_server_responded={server_responded_after_recovery}"
    );
    assert_eq!(
        live.request(Request::Shutdown { kill: true }),
        Response::Ok,
        "cleanup recovery permits a later bounded shutdown"
    );
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

#[test]
fn omp_model_catalog_and_initial_prompt_keep_one_detected_native_launch() {
    let live = Live::idle().bounded();
    let (fake_omp, capture, catalog) = install_fake_omp(&live);
    let log = live.root.path().join("server.stderr.log");
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server_with_omp(&live, &fake_omp, &capture, &catalog, Some(&log));
    live.ready(BRANCH);

    let mut dashboard = dashboard(&live);
    let mut launch = detected_omp_launch(
        &live,
        &fake_omp,
        "omp-explicit-prompt-session",
        AgentModelPolicy::HarnessDefault,
    );
    let capabilities = query_omp_models(&mut dashboard, 3, &launch);
    assert_eq!(capabilities.preset, "omp");
    assert!(capabilities.reason.is_none(), "{capabilities:?}");
    let binding = capabilities
        .binding
        .expect("OMP choices carry a launch binding");
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
        "the picker exposes exact selectors from OMP's native JSON catalog"
    );

    let intent = allocate_intent(&mut dashboard);
    launch.model_policy = AgentModelPolicy::Explicit("openai/gpt-5.4".into());
    launch.model_binding = Some(binding);
    launch.expected_model = Some("openai/gpt-5.4".into());
    let prompt = "  T395_OMP_PROMPT_MARKER_73ab9df1\nsecond line remains one argument  ";
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
    assert_eq!(accepted.name, "omp-explicit-prompt-session");

    let output = wait_for_capture(&capture, prompt);
    let launch_output = output
        .split_once(FAKE_OMP_LAUNCH_MARKER)
        .expect("the detected OMP process started")
        .1;
    assert!(
        launch_output.contains("ARG:--model\nARG:openai/gpt-5.4\n"),
        "explicit selection stays in OMP native argv: {launch_output:?}"
    );
    assert!(
        launch_output.contains("ARG:--auto-approve\n"),
        "the detected OMP trust option remains in the launch: {launch_output:?}"
    );
    assert!(
        launch_output.contains(&format!("ARG:--\nARG:{prompt}\n")),
        "the exact multiline prompt is one native positional message: {launch_output:?}"
    );
    assert_eq!(
        output.matches("__OVRCR_FAKE_OMP_CATALOG__").count(),
        2,
        "capability lookup and Start each consult OMP's current native catalog"
    );
    assert_eq!(
        output
            .matches("CATALOG_ARG:models\nCATALOG_ARG:--json\n")
            .count(),
        2,
        "model discovery uses OMP's structured JSON command"
    );
    assert!(!output.contains("CATALOG_ARG:--no-extensions"));

    drop(dashboard);
    let mut dashboard = self::dashboard(&live);
    let reconciled = reconcile(&mut dashboard, 2, &intent);
    assert_eq!(reconciled, accepted);
    let duplicate = client::request(
        &mut dashboard,
        3,
        Request::CreateAgentSessionWithPrompt(request.clone()),
    )
    .unwrap();
    assert_eq!(
        duplicate,
        Response::InitialPromptIntents(vec![accepted.clone()]),
        "replay after reconnect returns the exact accepted OMP session/run"
    );
    assert_eq!(
        std::fs::read_to_string(&capture)
            .unwrap()
            .matches(FAKE_OMP_LAUNCH_MARKER)
            .count(),
        1,
        "duplicate accepted OMP intent does not start another process"
    );
    drop(dashboard);
    let mut settings_dashboard = self::dashboard(&live);
    let remembered = next_settings_report(&mut settings_dashboard);
    assert_eq!(
        remembered.settings.launch_choices.get(PROJECT),
        Some(&LaunchChoice::Agent {
            preset: "omp".into(),
            model_policy: AgentModelPolicy::Explicit("openai/gpt-5.4".into()),
        }),
        "the exact OMP model policy is remembered per project"
    );
    drop(settings_dashboard);
    let mut dashboard = self::dashboard(&live);
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("expected retained session inventory")
    };
    let session = accepted.session.unwrap();
    let run = accepted.run.unwrap();
    let matching: Vec<_> = sessions
        .iter()
        .filter(|entry| entry.id == session && entry.run == run)
        .collect();
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].kind, SessionKind::Agent { name: "omp".into() });
    assert_eq!(matching[0].label, "omp");

    let blank_intent = allocate_intent(&mut dashboard);
    let blank_launch = detected_omp_launch(
        &live,
        &fake_omp,
        "omp-blank-interactive-session",
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
    assert_eq!(blank_accepted.name, "omp-blank-interactive-session");
    let deadline = Instant::now() + Duration::from_secs(10);
    let output = loop {
        let contents = std::fs::read_to_string(&capture).unwrap_or_default();
        if contents.matches(FAKE_OMP_LAUNCH_MARKER).count() >= 2
            && contents
                .rsplit_once(FAKE_OMP_LAUNCH_MARKER)
                .is_some_and(|(_, launch)| launch.contains("ARG:--auto-approve\n"))
        {
            break contents;
        }
        assert!(
            Instant::now() < deadline,
            "second fake OMP process did not start; last output: {contents:?}"
        );
        thread::park_timeout(Duration::from_millis(10));
    };
    let blank_output = output
        .rsplit_once(FAKE_OMP_LAUNCH_MARKER)
        .expect("the second OMP process started")
        .1;
    assert!(blank_output.contains("ARG:--auto-approve\n"));
    assert!(!blank_output.contains("ARG:--model\n"));
    assert!(!blank_output.contains("ARG:--\n"));

    let event_ring_empty = assert_public_stores_exclude(&live, "T395_OMP_PROMPT_MARKER_73ab9df1");
    drop(dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    assert_logs_exclude(
        &live,
        &log,
        "T395_OMP_PROMPT_MARKER_73ab9df1",
        event_ring_empty,
    );
}

#[test]
fn omp_start_refuses_a_model_removed_from_the_fresh_native_catalog() {
    let live = Live::idle().bounded();
    let (fake_omp, capture, catalog) = install_fake_omp(&live);
    write_settings(&live, "agents = []\n[quota]\nenabled = false\n");
    start_server_with_omp(&live, &fake_omp, &capture, &catalog, None);
    live.ready(BRANCH);

    let mut dashboard = dashboard(&live);
    let mut launch = detected_omp_launch(
        &live,
        &fake_omp,
        "omp-stale-model-session",
        AgentModelPolicy::HarnessDefault,
    );
    let capabilities = query_omp_models(&mut dashboard, 3, &launch);
    let binding = capabilities
        .binding
        .expect("OMP choices carry a launch binding");
    launch.model_policy = AgentModelPolicy::Explicit("openai/gpt-5.4".into());
    launch.model_binding = Some(binding);
    launch.expected_model = Some("openai/gpt-5.4".into());
    write_omp_catalog(&catalog, false);

    let intent = allocate_intent(&mut dashboard);
    let prompt = "T395_OMP_STALE_PROMPT_MARKER_b9d73e41\nkeep this draft recoverable";
    assert_eq!(
        client::request(
            &mut dashboard,
            4,
            Request::CreateAgentSessionWithPrompt(InitialPromptLaunchRequest {
                intent: intent.clone(),
                launch,
                prompt: prompt.into(),
            }),
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
            .is_some_and(|message| message.contains("selected omp model resolution changed")),
        "the removed exact model is refused: {refused:?}"
    );
    let output = std::fs::read_to_string(&capture).unwrap_or_default();
    assert_eq!(output.matches("__OVRCR_FAKE_OMP_CATALOG__").count(), 2);
    assert!(!output.contains(FAKE_OMP_LAUNCH_MARKER));
    assert!(!format!("{refused:?}").contains("T395_OMP_STALE_PROMPT_MARKER_b9d73e41"));

    drop(dashboard);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
}
