//! Production-path Codex adapter checks with an owned native CLI stand-in.
//! A captured argv proves what OVRCR passed to the fixture, not provider acceptance.
#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT, WORKSPACE};
use ovrcr::protocol::{
    AgentLaunchOrigin, AgentModelPolicy, ClientMessage, CreateAgentSessionRequest,
    CreateSessionRequest, InitialPromptIntentId, InitialPromptIntentSnapshot,
    InitialPromptIntentState, InitialPromptLaunchRequest, Request, Response, ServerEvent,
    ServerMessage, SessionKind, client, connect_server, read_frame, write_frame,
};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const CODEX_CAPTURE_ENV: &str = "OVRCR_TEST_FAKE_CODEX_CAPTURE";
const CODEX_LAUNCH_MARKER: &str = "__OVRCR_FAKE_CODEX_LAUNCH_BEGIN__";
const CODEX_LAUNCH_END: &str = "__OVRCR_FAKE_CODEX_LAUNCH_END__";

fn install_fake_codex(live: &Live) -> PathBuf {
    let bin = live.root.path().join("fake-codex-bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("codex");
    let args_path = live
        .root
        .path()
        .join("app-server-args.txt")
        .display()
        .to_string();
    let startup_args_path = live
        .root
        .path()
        .join("startup-config-args.txt")
        .display()
        .to_string();
    let cwd_path = live
        .root
        .path()
        .join("app-server-cwd.txt")
        .display()
        .to_string();
    let request_path = live
        .root
        .path()
        .join("app-server-requests.jsonl")
        .display()
        .to_string();
    let capture_path = live.root.path().join("codex-launch-capture.txt");
    let template = r##"#!/bin/sh
if [ "$1" = "-c" ] && [ "$2" = "features.plugins=false" ] && [ "$4" = "--print-startup-config" ]; then
printf '%s\n' "$@" > '@STARTUP_ARGS@'
printf '%s\n' '{"appServerVersion":"0.164.0","configRequirements":{"requirements":null}}'
exit 0
fi
if [ "$1" = "-c" ] && [ "$2" = "features.plugins=false" ]; then
printf '%s\n' "$@" > '@APP_ARGS@'
printf '%s\n' "$PWD" > '@CWD@'
while IFS= read -r line; do
printf '%s\n' "$line" >> '@REQUESTS@'
case "$line" in
*'"method":"initialize"'*) printf '%s\n' '{"id":1,"result":{"serverInfo":{"name":"Codex","version":"0.164.0"}}}' ;;
*'"method":"account/gatewayOAuth/read"'*) printf '%s\n' '{"id":2,"result":{"required":false,"status":null}}' ;;
*'"method":"model/list"'*) printf '%s\n' '{"id":3,"result":{"data":[{"id":"preset-gpt-5.4","model":"gpt-5.4","hidden":false,"isDefault":true}],"nextCursor":null}}' ;;
esac
done
exit 0
fi
case "$1" in --approve-for-me|--model) ;; *) exit 0 ;; esac
printf '%s\n' '__OVRCR_FAKE_CODEX_LAUNCH_BEGIN__' >> "${OVRCR_TEST_FAKE_CODEX_CAPTURE}"
for argument in "$@"; do printf 'ARG:%s\n' "$argument" >> "${OVRCR_TEST_FAKE_CODEX_CAPTURE}"; done
printf '%s\n' '__OVRCR_FAKE_CODEX_LAUNCH_END__' >> "${OVRCR_TEST_FAKE_CODEX_CAPTURE}"
exit 0
"##;
    let script = template
        .replace("@STARTUP_ARGS@", &startup_args_path)
        .replace("@APP_ARGS@", &args_path)
        .replace("@CWD@", &cwd_path)
        .replace("@REQUESTS@", &request_path);
    std::fs::write(&capture_path, "").unwrap();
    std::fs::write(&executable, script).unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    executable
}

fn start_server(live: &Live, codex: &Path, codex_home: &Path) {
    std::fs::create_dir_all(codex_home).unwrap();
    let mut path = vec![codex.parent().unwrap().to_path_buf()];
    path.extend(std::env::split_paths(&live.path()));
    let path = std::env::join_paths(path).unwrap();
    let capture_path = live.root.path().join("codex-launch-capture.txt");
    let environment = [
        ("PATH", path.as_os_str()),
        ("CODEX_HOME", codex_home.as_os_str()),
        (CODEX_CAPTURE_ENV, capture_path.as_os_str()),
    ];
    live.start_binary_logged(&environment, &live.root.path().join("server-stderr.log"));
}

fn open_dashboard(live: &Live) -> UnixStream {
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
                code: ovrcr::protocol::ErrorCode::Conflict,
                ..
            }) if Instant::now() < deadline => drop(stream),
            Ok(response) => panic!("unexpected DashboardHello response: {response:?}"),
            Err(error) => panic!("DashboardHello did not complete: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "current Dashboard did not attach"
        );
    }
}

fn codex_argv(live: &Live, codex: &Path) -> Vec<OsString> {
    let mut native = vec![codex.canonicalize().unwrap().into_os_string()];
    ovrcr::protocol::apply_default_auto_trust("codex", &mut native);
    let mut argv = vec![
        live.executable.canonicalize().unwrap().into_os_string(),
        "agent".into(),
        "run".into(),
        "codex".into(),
        "--".into(),
    ];
    argv.extend(native);
    argv
}

fn query_models(
    dashboard: &mut UnixStream,
    request_id: u64,
    live: &Live,
    codex: &Path,
) -> ovrcr::protocol::AgentModelCapabilities {
    assert_eq!(
        client::request(
            dashboard,
            request_id,
            Request::QueryAgentModels {
                preset: "codex".into(),
                origin: AgentLaunchOrigin::Detected,
                argv: codex_argv(live, codex),
                project: PROJECT.into(),
                workspace: WORKSPACE.into(),
            },
        )
        .unwrap(),
        Response::Ok
    );
    loop {
        match read_frame::<ServerMessage>(dashboard).unwrap() {
            ServerMessage::Event(ServerEvent::AgentModels {
                request_id: response_id,
                capabilities,
            }) if response_id == request_id => return capabilities,
            ServerMessage::Event(_) => {}
            ServerMessage::Response {
                request_id,
                response,
            } => panic!("unexpected Dashboard response {request_id}: {response:?}"),
        }
    }
}

fn create_prompt_session(
    dashboard: &mut UnixStream,
    request_id: u64,
    intent: &InitialPromptIntentId,
    request: InitialPromptLaunchRequest,
) -> InitialPromptIntentSnapshot {
    write_frame(
        dashboard,
        &ClientMessage {
            request_id,
            request: Request::CreateAgentSessionWithPrompt(request),
        },
    )
    .unwrap();
    let mut acknowledged = false;
    loop {
        match read_frame::<ServerMessage>(dashboard).unwrap() {
            ServerMessage::Response {
                request_id: response_id,
                response,
            } if response_id == request_id => {
                assert_eq!(response, Response::Ok);
                acknowledged = true;
            }
            ServerMessage::Event(ServerEvent::InitialPromptIntentChanged(snapshot))
                if snapshot.id == *intent
                    && snapshot.state == InitialPromptIntentState::LaunchAccepted =>
            {
                assert!(
                    acknowledged,
                    "intent snapshot follows its request acknowledgment"
                );
                return *snapshot;
            }
            ServerMessage::Event(_) | ServerMessage::Response { .. } => {}
        }
    }
}

fn wait_for_capture(capture: &PathBuf, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let contents = std::fs::read_to_string(capture).unwrap_or_default();
        if contents.contains(needle) {
            return contents;
        }
        assert!(
            Instant::now() < deadline,
            "fake Codex did not capture {needle:?}; last output: {contents:?}"
        );
        thread::park_timeout(Duration::from_millis(10));
    }
}

#[test]
fn codex_models_come_from_the_current_dashboard_workspace_app_server() {
    let live = Live::idle().bounded();
    let codex = install_fake_codex(&live);
    let codex_home = live.root.path().join("private-codex-home");
    start_server(&live, &codex, &codex_home);
    live.ready("feature/codex-model-catalog");

    let mut dashboard = open_dashboard(&live);
    let request_id = 2;
    assert_eq!(
        client::request(
            &mut dashboard,
            request_id,
            Request::QueryAgentModels {
                preset: "codex".into(),
                origin: AgentLaunchOrigin::Detected,
                argv: codex_argv(&live, &codex),
                project: PROJECT.into(),
                workspace: WORKSPACE.into(),
            },
        )
        .unwrap(),
        Response::Ok
    );
    let capabilities = loop {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ServerEvent::AgentModels {
                request_id: response_id,
                capabilities,
            }) if response_id == request_id => break capabilities,
            ServerMessage::Event(_) => {}
            ServerMessage::Response {
                request_id,
                response,
            } => panic!("unexpected Dashboard response {request_id}: {response:?}"),
        }
    };

    assert_eq!(capabilities.preset, "codex");
    assert_eq!(capabilities.models.len(), 1, "{capabilities:#?}");
    assert_eq!(capabilities.models[0].alias, "gpt-5.4");
    assert_eq!(capabilities.models[0].resolved, "gpt-5.4");
    assert!(capabilities.binding.is_some());

    let args = std::fs::read_to_string(live.root.path().join("app-server-args.txt")).unwrap();
    assert_eq!(args, "-c\nfeatures.plugins=false\napp-server\n--stdio\n");
    let requests =
        std::fs::read_to_string(live.root.path().join("app-server-requests.jsonl")).unwrap();
    let methods = requests
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["method"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "account/gatewayOAuth/read",
            "model/list"
        ]
    );
    let initialize: serde_json::Value =
        serde_json::from_str(requests.lines().next().unwrap()).unwrap();
    assert_eq!(
        initialize["params"]["capabilities"]["explicitGatewayOauth"],
        true
    );
    let startup_args =
        std::fs::read_to_string(live.root.path().join("startup-config-args.txt")).unwrap();
    assert_eq!(
        startup_args,
        "-c\nfeatures.plugins=false\napp-server\n--print-startup-config\n"
    );
    assert!(!requests.contains("thread/start"));
    assert!(!requests.contains("turn/start"));

    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
}

#[test]
fn codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path() {
    let live = Live::idle().bounded();
    let codex = install_fake_codex(&live);
    let codex_home = live.root.path().join("private-codex-home");
    start_server(&live, &codex, &codex_home);
    live.ready("feature/codex-initial-prompt");
    let mut dashboard = open_dashboard(&live);
    let capture = live.root.path().join("codex-launch-capture.txt");

    let default_intent =
        match client::request(&mut dashboard, 2, Request::AllocateInitialPromptIntent).unwrap() {
            Response::InitialPromptIntent(id) => id,
            other => panic!("expected allocated blank Codex prompt intent: {other:?}"),
        };
    let default = CreateAgentSessionRequest {
        session: CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "codex-default".into(),
            label: Some("codex".into()),
            argv: codex_argv(&live, &codex),
            kind: SessionKind::Agent {
                name: "codex".into(),
            },
        },
        model_policy: AgentModelPolicy::HarnessDefault,
        model_binding: None,
        expected_model: None,
    };
    let default_intent_request = InitialPromptLaunchRequest {
        intent: default_intent.clone(),
        launch: default,
        prompt: String::new(),
    };
    let blank = create_prompt_session(&mut dashboard, 3, &default_intent, default_intent_request);
    assert_eq!(blank.project, PROJECT);
    assert_eq!(blank.workspace, WORKSPACE);
    assert_eq!(blank.name, "codex-default");
    assert_eq!(blank.state, InitialPromptIntentState::LaunchAccepted);
    assert_eq!(blank.recoverable_prompt, None);
    let default_capture = wait_for_capture(&capture, CODEX_LAUNCH_END);
    let default_block =
        format!("{CODEX_LAUNCH_MARKER}\nARG:--approve-for-me\n{CODEX_LAUNCH_END}\n");
    assert_eq!(default_capture.matches(&default_block).count(), 1);
    assert!(!default_capture.contains("ARG:--model\n"));

    let capabilities = query_models(&mut dashboard, 4, &live, &codex);
    assert_eq!(capabilities.models.len(), 1, "{capabilities:#?}");
    let binding = capabilities
        .binding
        .clone()
        .expect("verified Codex binding");
    let model = capabilities.models[0].clone();
    let launch = CreateAgentSessionRequest {
        session: CreateSessionRequest {
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: "codex-explicit-prompt".into(),
            label: Some("codex".into()),
            argv: codex_argv(&live, &codex),
            kind: SessionKind::Agent {
                name: "codex".into(),
            },
        },
        model_policy: AgentModelPolicy::Explicit(model.alias.clone()),
        model_binding: Some(binding),
        expected_model: Some(model.resolved.clone()),
    };
    let intent =
        match client::request(&mut dashboard, 5, Request::AllocateInitialPromptIntent).unwrap() {
            Response::InitialPromptIntent(id) => id,
            other => panic!("expected allocated Codex prompt intent: {other:?}"),
        };
    let prompt = "  CODEX_INITIAL_PROMPT_PRIVACY_2f14411d\nsecond line of prompt\n trailing  ";
    let prompt_request = InitialPromptLaunchRequest {
        intent: intent.clone(),
        launch,
        prompt: prompt.into(),
    };
    let accepted = create_prompt_session(&mut dashboard, 6, &intent, prompt_request.clone());
    assert_eq!(accepted.state, InitialPromptIntentState::LaunchAccepted);
    assert_eq!(accepted.project, PROJECT);
    assert_eq!(accepted.workspace, WORKSPACE);
    assert_eq!(accepted.name, "codex-explicit-prompt");
    let session = accepted.session.expect("accepted intent names its session");
    let run = accepted.run.expect("accepted intent names its run");
    assert_eq!(
        accepted.delivery,
        Some(ovrcr::protocol::InitialPromptDelivery::Unknown)
    );
    assert_eq!(accepted.recoverable_prompt, None);

    let captured = wait_for_capture(&capture, &format!("{prompt}\n{CODEX_LAUNCH_END}"));
    let explicit_block = format!(
        "{CODEX_LAUNCH_MARKER}\nARG:--model\nARG:gpt-5.4\nARG:--approve-for-me\nARG:--\nARG:{prompt}\n{CODEX_LAUNCH_END}\n"
    );
    assert_eq!(captured.matches(&explicit_block).count(), 1);
    assert_eq!(captured.matches(CODEX_LAUNCH_MARKER).count(), 2);
    assert_eq!(captured.matches(CODEX_LAUNCH_END).count(), 2);
    assert!(
        !captured.contains("INPUT:"),
        "prompt is one native argv item"
    );

    drop(dashboard);
    let mut reconnected_dashboard = open_dashboard(&live);
    let Response::InitialPromptIntents(reconciled) = client::request(
        &mut reconnected_dashboard,
        2,
        Request::ReconcileInitialPromptIntents,
    )
    .unwrap() else {
        panic!("expected Codex Initial prompt reconciliation")
    };
    let replayed = reconciled
        .iter()
        .find(|snapshot| snapshot.id == intent)
        .expect("accepted Codex intent remains reconcilable");
    assert_eq!(replayed, &accepted);
    assert_eq!(replayed.session, Some(session));
    assert_eq!(replayed.run, Some(run));
    assert_eq!(
        client::request(
            &mut reconnected_dashboard,
            3,
            Request::CreateAgentSessionWithPrompt(prompt_request),
        )
        .unwrap(),
        Response::InitialPromptIntents(vec![accepted.clone()]),
        "replaying the accepted intent returns its exact session/run"
    );
    assert_eq!(
        std::fs::read_to_string(&capture)
            .unwrap()
            .matches(CODEX_LAUNCH_MARKER)
            .count(),
        2,
        "accepted-intent replay starts no third native process"
    );

    drop(reconnected_dashboard);
    let settings = std::fs::read_to_string(live.config.join("dashboard.toml")).unwrap();
    assert!(
        settings.contains("codex"),
        "accepted harness choice is remembered"
    );
    assert!(
        settings.contains("gpt-5.4"),
        "explicit model choice is remembered"
    );
    assert!(
        !settings.contains(prompt),
        "Initial prompt is not saved to settings"
    );
    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
        panic!("expected retained session inventory")
    };
    assert!(
        !serde_json::to_string(&sessions).unwrap().contains(prompt),
        "Initial prompt is not saved in session metadata"
    );
    let Response::Events(events) = live.request(Request::Events { follow: false }) else {
        panic!("expected OVRCR EventLog snapshot")
    };
    assert!(
        !serde_json::to_string(&events).unwrap().contains(prompt),
        "Initial prompt is not saved in OVRCR Events"
    );
    let database = ovrcr::config::database_path(&live.config);
    if let Ok(bytes) = std::fs::read(&database) {
        assert!(
            !bytes
                .windows(prompt.len())
                .any(|window| window == prompt.as_bytes()),
            "Initial prompt is not saved in retained storage"
        );
    }
    let request_log =
        std::fs::read_to_string(live.root.path().join("app-server-requests.jsonl")).unwrap();
    assert!(!request_log.contains("thread/start"));
    assert!(!request_log.contains("turn/start"));

    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
}
