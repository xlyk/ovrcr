//! Compiled Operator worker against a deterministic fake Pi RPC peer.
//!
//! Every test owns a private instance, server, Pi profile and fake process. The
//! fake speaks pi's LF-framed RPC vocabulary and records each command and
//! admission in an append-only receipt, so assertions are about what the worker
//! actually asked Pi to do, not about a simulated Rust interface.

#[path = "support/fake_pi.rs"]
mod fake_pi;
#[path = "support/operator_harness.rs"]
mod harness;
#[path = "support/live.rs"]
mod live;

use fake_pi::{FakePi, Scenario, Step};
use harness::{OperatorClient, Registry};
use live::Live;
use ovrcr::operator::rpc::{FRAME_LIMIT, RpcChild, RpcMessage};
use ovrcr::protocol::{
    OperatorMessageContent, OperatorToolStatus, OperatorTurnState, OperatorWorkerEvent, Request,
    Response,
};
use serde_json::{Value, json};
use std::{os::unix::fs::MetadataExt, path::Path, process::Command, time::Duration};

const SENTINEL: &str = "sk-FIXTURE-CREDENTIAL-SENTINEL-7f3a";

fn worker_fixture(scenario: &Scenario) -> (Live, FakePi, OperatorClient) {
    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    fake.script(scenario);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let client = OperatorClient::connect(&live);
    (live, fake, client)
}

fn deltas_for(history: &ovrcr::protocol::OperatorHistory, turn: u64) -> String {
    history
        .messages
        .iter()
        .filter(|message| message.turn.0 == turn)
        .filter_map(|message| match &message.content {
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

fn selected_models_in_transcript(history: &ovrcr::protocol::OperatorHistory) -> usize {
    history
        .messages
        .iter()
        .filter(|message| {
            matches!(
                message.content,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ModelSelected(_))
            )
        })
        .count()
}

fn failed_text(history: &ovrcr::protocol::OperatorHistory) -> Option<String> {
    history
        .messages
        .iter()
        .find_map(|message| match &message.content {
            OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(text)) => Some(text.clone()),
            _ => None,
        })
}

#[test]
fn operator_cli_exposes_setup_and_models_and_hides_worker() {
    let live = Live::idle();
    let output = live
        .command()
        .args(["operator", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "operator CLI missing: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    for command in ["setup", "models"] {
        assert!(help.contains(command), "missing {command}: {help}");
    }
    assert!(!help.contains("worker"), "worker must stay hidden: {help}");
    for removed in ["login", "logout", "status"] {
        assert!(
            !help.contains(removed),
            "obsolete auth command {removed} remains: {help}"
        );
    }
}

#[test]
fn setup_requires_a_terminal_and_refuses_json_before_starting_pi() {
    let live = Live::idle();
    let json = live
        .command()
        .args(["--json", "operator", "setup"])
        .output()
        .unwrap();
    assert!(!json.status.success());
    assert!(
        String::from_utf8_lossy(&json.stderr).contains("drop --json"),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let plain = live.command().args(["operator", "setup"]).output().unwrap();
    assert!(!plain.status.success());
    assert!(
        String::from_utf8_lossy(&plain.stderr).contains("interactive terminal"),
        "{}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert!(
        !live.config.join("operator-pi").exists(),
        "refused setup must not prepare a profile or launch pi"
    );
}

#[test]
fn models_lists_the_dedicated_profiles_catalog_without_launching_a_server() {
    let live = Live::idle();
    let fake = FakePi::install(&live.config, live.root.path());
    let mut scenario = Scenario::new();
    scenario.models(vec![
        ("fixture", "default", "Fixture Default"),
        ("fixture", "default", "Duplicate Name"),
        ("vendor", "model/slash", "Vendor Model"),
    ]);
    fake.script(&scenario);

    let text = live
        .command()
        .args(["operator", "models"])
        .env("OVRCR_PI_EXECUTABLE", &fake.wrapper)
        .output()
        .unwrap();
    assert!(
        text.status.success(),
        "{}",
        String::from_utf8_lossy(&text.stderr)
    );
    let expected = "fixture/default\tFixture Default\nvendor/model/slash\tVendor Model\n";
    assert_eq!(String::from_utf8_lossy(&text.stdout), expected);
    assert!(
        !live.socket.exists(),
        "model listing must not start a Server"
    );

    let json_output = live
        .command()
        .args(["--json", "operator", "models"])
        .env("OVRCR_PI_EXECUTABLE", &fake.wrapper)
        .output()
        .unwrap();
    assert!(json_output.status.success());
    let decoded: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    assert_eq!(
        decoded,
        json!({"models": [
            {"provider": "fixture", "id": "default", "name": "Fixture Default"},
            {"provider": "vendor", "id": "model/slash", "name": "Vendor Model"}
        ]})
    );
    let log = fake.log();
    let catalog_commands = log
        .iter()
        .filter(|line| line["event"] == "command" && line["command"] == "get_available_models")
        .count();
    assert_nonzero(catalog_commands, "model catalog commands");
    assert!(
        log.iter().filter(|line| line["event"] == "start").count() >= 2,
        "each listing launches its own Pi catalog process: {log:#?}"
    );
}

fn assert_nonzero(count: usize, what: &str) {
    assert!(count > 0, "{what} must be nonzero");
}

// ------------------------------------------------------------ RPC framing

/// Spawn the fake with a standalone scenario and no instance profile.
fn direct_child(scenario: &Scenario, dir: &Path) -> RpcChild {
    let fake = FakePi::install(dir, dir);
    let scenario_path = dir.join("standalone-scenario.json");
    fake.standalone_script(scenario, &scenario_path);
    let mut command = Command::new(&fake.wrapper);
    command
        .args(["--mode", "rpc", "--session-dir"])
        .arg(dir)
        .arg("--session-id")
        .arg("fixture")
        .env("FAKE_PI_SCENARIO", &scenario_path);
    RpcChild::spawn(&mut command, true).unwrap()
}

#[test]
fn rpc_splits_on_lf_only_and_separators_stay_inside_strings() {
    let dir = tempfile::tempdir().unwrap();
    let mut scenario = Scenario::new();
    scenario.contract("none");
    scenario.startup_records(vec![
        "{\"type\":\"note\",\"text\":\"a\u{2028}b\u{2029}c\"}\n".into(),
        "{\"type\":\"message_update\",\"assistantMessageEvent\":{\"type\":\"text_delta\",\"delta\":\"x\"}}\r\n"
            .into(),
    ]);
    let mut child = direct_child(&scenario, dir.path());
    let first = match child.next_message(Some(Duration::from_secs(10))).unwrap() {
        RpcMessage::Event(event) => event,
        other => panic!("expected event: {other:?}"),
    };
    assert_eq!(first["text"], "a\u{2028}b\u{2029}c");
    let second = match child.next_message(Some(Duration::from_secs(10))).unwrap() {
        RpcMessage::Event(event) => event,
        other => panic!("expected event: {other:?}"),
    };
    assert_eq!(second["assistantMessageEvent"]["delta"], "x");
    child.terminate(Duration::from_millis(200));
}

#[test]
fn rpc_accepts_an_exact_limit_record_and_refuses_one_byte_more() {
    let prefix = "{\"type\":\"note\",\"pad\":\"";
    let padding = FRAME_LIMIT - prefix.len() - 2;
    let dir = tempfile::tempdir().unwrap();
    let mut exact = Scenario::new();
    exact.contract("none");
    exact.startup_records(vec![format!("{prefix}{}\"}}\n", "a".repeat(padding))]);
    let mut child = direct_child(&exact, dir.path());
    let message = child.next_message(Some(Duration::from_secs(60))).unwrap();
    match message {
        RpcMessage::Event(event) => assert_eq!(event["type"], "note"),
        other => panic!("expected exact-limit record: {other:?}"),
    }
    child.terminate(Duration::from_millis(200));

    let dir = tempfile::tempdir().unwrap();
    let mut over = Scenario::new();
    over.contract("none");
    over.startup_records(vec![format!("{prefix}{}\"}}\n", "a".repeat(padding + 1))]);
    let mut child = direct_child(&over, dir.path());
    let error = child
        .next_message(Some(Duration::from_secs(60)))
        .expect_err("16 MiB + 1 complete record must be refused");
    assert_eq!(error.kind(), ovrcr::operator::pi::PiErrorKind::Request);
    child.terminate(Duration::from_millis(200));
}

#[test]
fn rpc_refuses_an_incomplete_record_past_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut scenario = Scenario::new();
    scenario.contract("none");
    scenario.startup_records(vec!["a".repeat(FRAME_LIMIT + 1)]);
    let mut child = direct_child(&scenario, dir.path());
    let error = child
        .next_message(Some(Duration::from_secs(60)))
        .expect_err("an unterminated >16 MiB record must be refused");
    assert_eq!(error.kind(), ovrcr::operator::pi::PiErrorKind::Request);
    child.terminate(Duration::from_millis(200));
}

#[test]
fn rpc_refuses_an_oversize_command_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let mut scenario = Scenario::new();
    scenario.contract("none");
    let mut child = direct_child(&scenario, dir.path());
    let error = child
        .send(&json!({"type": "prompt", "pad": "a".repeat(FRAME_LIMIT)}))
        .expect_err("commands above the frame limit must not be written");
    assert_eq!(error.kind(), ovrcr::operator::pi::PiErrorKind::Request);
    child.terminate(Duration::from_millis(200));
}

// ------------------------------------------------------------ worker turns

#[test]
fn a_completed_turn_selects_the_profile_default_and_promotes_its_checkpoint() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::assistant("first answer"), Step::Settle]);
    let (live, fake, mut client) = worker_fixture(&scenario);

    let opened = client.open();
    assert!(
        opened.model.is_none(),
        "no model before the first selection"
    );
    assert_eq!(opened.title, "Operator");
    let turned = client.submit_turn(&opened, "do the fixture task");
    let history = client.wait_terminal(opened.id);
    let turn = history.conversation.current_turn.as_ref().unwrap();
    assert_eq!(
        turn.state,
        OperatorTurnState::Complete,
        "history: {history:#?}\nfake pi: {:#?}",
        fake.log()
    );

    assert!(deltas_for(&history, turned.identity.turn.0).contains("first answer"));
    assert_eq!(
        selected_models_in_transcript(&history),
        0,
        "the model selection is retained metadata, not a visible journal entry"
    );
    assert_eq!(
        history
            .conversation
            .model
            .as_ref()
            .map(|m| format!("{}/{}", m.provider, m.id)),
        Some("fixture/default".to_string())
    );

    let registry = Registry::of(&live);
    assert_eq!(
        registry.checkpoint(opened.id.0),
        Some(turned.identity.turn.0 as i64)
    );
    assert_eq!(
        registry.turn_state(turned.identity.turn.0),
        Some("Complete".into())
    );
    let digests = registry.batch_digests(turned.identity.turn.0);
    assert_nonzero(digests.len(), "committed batch receipts");

    // Receipts prove what the worker asked: exact ids, exact model, one prompt.
    let log = fake.log();
    let commands: Vec<&Value> = log
        .iter()
        .filter(|line| line["event"] == "command")
        .collect();
    assert_nonzero(commands.len(), "RPC commands");
    assert!(
        commands.iter().all(|line| line["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("ovcr-") || id.starts_with("ovrcr-"))),
        "every command carries a request id: {commands:#?}"
    );
    let set_model = log
        .iter()
        .find(|line| line["event"] == "command" && line["command"] == "set_model")
        .expect("set_model receipt");
    assert_eq!(set_model["provider"], "fixture");
    assert_eq!(set_model["modelId"], "default");
    assert_eq!(set_model["found"], true);
    let prompts: Vec<&Value> = log
        .iter()
        .filter(|line| line["event"] == "prompt")
        .collect();
    assert_eq!(prompts.len(), 1, "exactly one prompt per turn");
    let prompt = prompts[0]["message"].as_str().unwrap();
    assert!(prompt.starts_with("Captured OVRCR context"));
    assert!(prompt.contains("do the fixture task"));
    assert!(log.iter().any(|line| line["event"] == "stdin_closed"));

    // The promoted native session file is private, correctly identified and synced.
    let attempt = live
        .config
        .join("operator-pi/turns")
        .join(turned.identity.turn.0.to_string());
    let session = attempt.join("session.jsonl");
    let metadata = std::fs::symlink_metadata(&session).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.mode() & 0o777, 0o600);
    let header: Value = serde_json::from_str(
        std::fs::read_to_string(&session)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(header["type"], "session");
    assert_eq!(header["id"], format!("operator-{}", opened.id.0));
    assert_eq!(
        header["cwd"].as_str().unwrap(),
        live.config
            .join("operator-workdir")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );

    client.wait_stopped(opened.id);
    assert!(
        !client.session(opened.session).phase.is_live(),
        "controlled stop must retire the worker process"
    );
}

#[test]
fn an_interrupted_turn_keeps_its_partial_journal_and_never_promotes_a_checkpoint() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::Start,
        Step::Delta("partial answer".into()),
        Step::Hold,
    ]);
    let (live, _fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "start something long");
    let partial = client.wait_history(opened.id, |history| {
        harness::has_assistant(history, "partial answer")
    });
    assert_eq!(
        partial.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Running
    );
    assert!(
        !partial.conversation.worker_stopped,
        "a Running turn is never stop proof"
    );
    let pgid = harness::own_worker_group(&live, &mut client, opened.session);

    let stopped = client.stop_turn(turned.identity);
    assert_eq!(stopped.state, OperatorTurnState::Interrupted);
    let history = client.wait_stopped(opened.id);
    assert!(harness::has_assistant(&history, "partial answer"));
    assert!(harness::interrupted(&history));
    harness::assert_group_absent(&live, pgid);

    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    let attempt = live
        .config
        .join("operator-pi/turns")
        .join(turned.identity.turn.0.to_string());
    assert!(
        attempt.is_dir(),
        "uncertain attempt evidence must be preserved"
    );
    assert!(
        !attempt.join("session.jsonl").exists(),
        "an interrupted attempt's session must not become a checkpoint"
    );
}

#[test]
fn a_native_failure_commits_one_fixed_message_without_a_checkpoint() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::Event(
        json!({"type": "extension_error", "message": SENTINEL}),
    )]);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "fail");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error
    );
    assert_eq!(
        failed_text(&history).as_deref(),
        Some("Pi request failed; inspect the partial journal before retrying.")
    );
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    assert!(deltas_for(&history, turned.identity.turn.0).is_empty());
    assert!(
        !harness::history_text(&history).contains("sk-"),
        "native/provider bodies never enter the journal"
    );
    client.wait_stopped(opened.id);
    let _ = fake;
}

#[test]
fn a_startup_failure_refuses_the_turn_before_any_prompt() {
    let mut scenario = Scenario::new();
    scenario.startup("exit");
    scenario.startup_stderr(SENTINEL);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "never prompted");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error
    );
    assert_eq!(
        failed_text(&history).as_deref(),
        Some("Pi request failed; inspect the partial journal before retrying.")
    );
    assert!(
        fake.log().iter().all(|line| line["event"] != "prompt"),
        "no prompt may follow a failed start"
    );
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    assert!(!harness::history_text(&history).contains(SENTINEL));
    assert!(!client.serialized_messages().contains(SENTINEL));
    assert!(!harness::settings_text(&live).contains(SENTINEL));
    let _ = turned;
}

#[test]
fn a_wrong_extension_contract_refuses_the_turn_before_any_prompt() {
    for mode in ["wrong_session", "wrong_tools", "invalid"] {
        let mut scenario = Scenario::new();
        scenario.contract(mode);
        scenario.steps(vec![Step::assistant("must not run")]);
        let (live, fake, mut client) = worker_fixture(&scenario);
        let opened = client.open();
        client.submit_turn(&opened, "contract");
        let history = client.wait_terminal(opened.id);
        assert_eq!(
            history.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Error,
            "contract mode {mode}"
        );
        assert_eq!(
            failed_text(&history).as_deref(),
            Some("Pi Operator integration is unavailable; no prompt was sent."),
            "contract mode {mode}"
        );
        assert!(
            fake.log().iter().all(|line| line["event"] != "prompt"),
            "no prompt may follow a refused contract ({mode})"
        );
        assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    }
}

#[test]
fn a_resumed_turn_uses_the_fresh_profile_default_and_the_last_completed_checkpoint() {
    let mut first = Scenario::new();
    first.steps(vec![Step::assistant("turn one answer"), Step::Settle]);
    let (live, fake, mut client) = worker_fixture(&first);
    let opened = client.open();
    let turn_one = client.submit_turn(&opened, "first");
    client.wait_history(opened.id, |history| {
        history.conversation.current_turn.as_ref().unwrap().state == OperatorTurnState::Complete
    });
    client.wait_stopped(opened.id);
    let checkpoint = live
        .config
        .join("operator-pi/turns")
        .join(turn_one.identity.turn.0.to_string())
        .join("session.jsonl");
    let checkpoint_bytes = std::fs::read_to_string(&checkpoint).unwrap();
    assert!(checkpoint_bytes.contains("turn one answer"));

    // The dedicated profile's default changes between turns. The checkpoint
    // restored by switch_session still says thinking "medium" (turn one), so a
    // worker that keeps the resumed level instead of the fresh default fails.
    let mut second = Scenario::new();
    second.default_model("fixture", "alternate");
    second.set("thinking_level", json!("high"));
    second.steps(vec![Step::assistant("turn two answer"), Step::Settle]);
    fake.script(&second);
    fake.clear_log();

    let summary = client.current().unwrap();
    assert_eq!(
        summary.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Complete
    );
    let turn_two = client.submit_turn(&summary, "second");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Complete
    );
    assert_eq!(
        history.conversation.model.as_ref().map(|m| m.id.clone()),
        Some("alternate".into())
    );

    let log = fake.log();
    let switch = log
        .iter()
        .find(|line| line["event"] == "switch")
        .expect("checkpoint switch receipt");
    assert_eq!(switch["exists"], true);
    let switch_path = Path::new(switch["path"].as_str().unwrap());
    assert_eq!(
        switch_path.parent().unwrap(),
        live.config
            .join("operator-pi/turns")
            .join(turn_two.identity.turn.0.to_string())
            .as_path()
    );
    let copied = std::fs::read_to_string(switch_path).unwrap();
    assert!(
        copied.contains("turn one answer"),
        "the resumed session must contain prior successful context: {copied}"
    );
    let set_model = log
        .iter()
        .find(|line| line["event"] == "command" && line["command"] == "set_model")
        .unwrap();
    assert_eq!(set_model["modelId"], "alternate");
    let set_thinking = log
        .iter()
        .find(|line| line["event"] == "command" && line["command"] == "set_thinking_level")
        .expect("thinking-level receipt");
    assert_eq!(
        set_thinking["level"], "high",
        "the fresh profile thinking level, not the checkpoint's restored medium"
    );

    // Only the new checkpoint survives; the superseded copy is removed.
    assert!(
        !checkpoint.exists(),
        "the superseded checkpoint copy must be removed after acknowledgement"
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turn_two.identity.turn.0 as i64)
    );
}

#[test]
fn submit_snapshots_the_model_override_for_that_turn_only() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::assistant("ok"), Step::Settle]);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();

    assert_eq!(
        client.exchange(Request::SetSetting {
            path: "operator.model".into(),
            value: Some("\"fixture/alternate\"".into())
        }),
        Response::Ok
    );
    let turned = client.submit_turn(&opened, "use the snapshot");
    // A settings edit after acceptance cannot change this turn.
    assert_eq!(
        client.exchange(Request::SetSetting {
            path: "operator.model".into(),
            value: Some("\"fixture/default\"".into())
        }),
        Response::Ok
    );
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.model.as_ref().map(|m| m.id.clone()),
        Some("alternate".into())
    );
    let registry = Registry::of(&live);
    assert_eq!(
        registry.model_override(turned.identity.turn.0).as_deref(),
        Some("fixture/alternate")
    );
    let log = fake.log();
    let set_model = log
        .iter()
        .find(|line| line["event"] == "command" && line["command"] == "set_model")
        .unwrap();
    assert_eq!(set_model["modelId"], "alternate");

    // The next accepted turn reads the edited settings instead.
    // A follow-up requires the previous turn's controlled-stop proof.
    client.wait_stopped(opened.id);
    let summary = client.current().unwrap();
    let second = client.submit_turn(&summary, "next turn");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.model.as_ref().map(|m| m.id.clone()),
        Some("default".into())
    );
    assert_eq!(
        Registry::of(&live)
            .model_override(second.identity.turn.0)
            .as_deref(),
        Some("fixture/default")
    );
}

#[test]
fn assistant_text_is_prefix_deduplicated_and_a_contradiction_fails() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::Partial {
            streamed: "Hello ".into(),
            text: "Hello world".into(),
            stop: "stop".into(),
        },
        Step::Settle,
    ]);
    let (_live, _fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "deduplicate");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Complete
    );
    assert_eq!(deltas_for(&history, turned.identity.turn.0), "Hello world");

    let mut contradiction = Scenario::new();
    contradiction.steps(vec![
        Step::Contradiction {
            streamed: "Hello".into(),
            text: "Goodbye".into(),
        },
        Step::Settle,
    ]);
    let (live, _fake, mut client) = worker_fixture(&contradiction);
    let opened = client.open();
    client.submit_turn(&opened, "contradict");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error
    );
    assert_eq!(
        failed_text(&history).as_deref(),
        Some("Pi request failed; inspect the partial journal before retrying.")
    );
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
}

#[test]
fn a_failed_tool_result_is_recorded_and_still_completes() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::Tool {
            id: "call-failed".into(),
            name: "bash".into(),
            input: json!({"command": "exit 3"}),
            output: "command failed".into(),
            status: "failed",
            truncated: false,
            marker: None,
        },
        Step::assistant("tool done"),
        Step::Settle,
    ]);
    let (_live, _fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    client.submit_turn(&opened, "run a failing command");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Complete
    );
    let calls = harness::tool_calls(&history);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "call-failed");
    let outputs = harness::tool_outputs(&history);
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].1, "command failed");
    assert!(history.messages.iter().any(|message| matches!(
        &message.content,
        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
            status: OperatorToolStatus::Failed { exit_code: None },
            ..
        })
    )));
}

#[test]
fn admissions_are_sequential_and_no_command_runs_after_a_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let second_marker = dir.path().join("second-tool-ran");
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::SecondAdmission {
            id: "call-one".into(),
            name: "bash".into(),
            input: json!({"command": "printf one"}),
            output: "one".into(),
            second: "call-two".into(),
            second_name: "bash".into(),
            second_input: json!({"command": "printf two"}),
            second_marker: Some(second_marker.clone()),
        },
        Step::Settle,
    ]);
    let (_live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    client.submit_turn(&opened, "two tools");
    let history = client.wait_terminal(opened.id);
    // A peer that offers a second tool before the first result is durable is an
    // integration violation: the worker refuses it and stops the turn.
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error
    );
    assert_eq!(
        failed_text(&history).as_deref(),
        Some("Pi request failed; inspect the partial journal before retrying.")
    );
    let calls = harness::tool_calls(&history);
    assert_eq!(
        calls.len(),
        1,
        "only the admitted tool enters the journal: {calls:?}"
    );
    assert_eq!(calls[0].0, "call-one");
    assert!(
        harness::tool_outputs(&history).is_empty(),
        "the refused turn produces no result receipt"
    );
    assert!(
        !second_marker.exists(),
        "a refused second admission must not execute its command"
    );

    let admissions: Vec<Value> = fake.events("admission");
    assert_eq!(admissions.len(), 2);
    assert_eq!(admissions[0]["id"], "call-one");
    assert_eq!(admissions[0]["confirmed"], true);
    assert_eq!(admissions[1]["id"], "call-two");
    assert_eq!(
        admissions[1]["confirmed"], false,
        "the worker must serialize admissions behind the durable result"
    );
    let executed: Vec<Value> = fake.events("executed");
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0]["id"], "call-one");
}

#[test]
fn a_result_for_an_unadmitted_call_is_refused_without_effects() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("unknown-call-ran");
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::UnknownResult {
        id: "never-admitted".into(),
        output: "must not land".into(),
        marker: Some(marker.clone()),
    }]);
    let (_live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    client.submit_turn(&opened, "late acknowledgement");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error
    );
    assert!(
        !marker.exists(),
        "no command may execute after a refused result acknowledgement"
    );
    let acknowledgements: Vec<Value> = fake.events("result_ack");
    assert!(
        acknowledgements
            .iter()
            .all(|line| line["confirmed"] != true),
        "an unadmitted result is never acknowledged: {acknowledgements:#?}"
    );
    assert!(harness::tool_calls(&history).is_empty());
    assert!(harness::tool_outputs(&history).is_empty());
    assert!(!harness::history_text(&history).contains("must not land"));
}

#[test]
fn provider_and_stderr_sentinels_never_reach_the_journal_settings_or_events() {
    let mut scenario = Scenario::new();
    scenario.startup_stderr(&format!("provider stderr {SENTINEL}"));
    scenario.steps(vec![
        Step::Stderr(format!("mid-turn {SENTINEL}")),
        Step::Event(json!({"type": "notify", "message": SENTINEL})),
        Step::assistant("clean answer"),
        Step::Settle,
    ]);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    client.submit_turn(&opened, "keep secrets out");
    let history = client.wait_terminal(opened.id);
    assert!(harness::has_assistant(&history, "clean answer"));

    let registry = Registry::of(&live);
    let attempt = live
        .config
        .join("operator-pi/turns")
        .join(
            history
                .conversation
                .current_turn
                .as_ref()
                .unwrap()
                .identity
                .turn
                .0
                .to_string(),
        )
        .join("session.jsonl");
    let surfaces = [
        ("journal", harness::history_text(&history)),
        ("registry", registry.operator_payload()),
        ("settings", harness::settings_text(&live)),
        ("server events", client.serialized_messages()),
        (
            "native session",
            std::fs::read_to_string(&attempt).unwrap_or_default(),
        ),
        ("fake receipt", serde_json::to_string(&fake.log()).unwrap()),
    ];
    for (surface, text) in surfaces {
        assert!(
            !text.contains(SENTINEL),
            "credential sentinel leaked into {surface}"
        );
    }
    assert!(
        client.messages.len() > 2,
        "the sentinel scan must cover real ServerEvents"
    );
}

#[test]
fn direct_worker_without_a_server_lease_creates_no_profile_side_effects() {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .env_clear()
        .env("HOME", home.path())
        .env("OVRCR_HOME", home.path())
        .env("SHELL", "/bin/sh")
        .args([
            "operator",
            "worker",
            "--conversation",
            "1",
            "--turn",
            "1",
            "--session",
            "1",
            "--run",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "an unmanaged worker must refuse");
    assert!(!home.path().join("operator-pi").exists());
}

#[test]
fn rpc_child_writes_are_bounded_and_termination_reaps_the_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let scenario = Scenario::new();
    let fake = FakePi::install(dir.path(), dir.path());
    let scenario_path = dir.path().join("scenario.json");
    fake.standalone_script(&scenario, &scenario_path);
    let mut command = Command::new(&fake.wrapper);
    command
        .args(["--mode", "rpc"])
        .env("FAKE_PI_SCENARIO", &scenario_path);
    let mut child = RpcChild::spawn(&mut command, true).unwrap();
    child
        .send(&json!({"id": "one", "type": "get_state"}))
        .unwrap();
    // The response is addressed by the request id, not by arrival order.
    loop {
        match child.next_message(Some(Duration::from_secs(10))).unwrap() {
            RpcMessage::Response(response) => {
                assert_eq!(response.id, "one");
                assert!(response.command == "get_state");
                break;
            }
            RpcMessage::Event(_) => continue,
        }
    }
    let status = child.terminate(Duration::from_millis(500));
    assert!(status.is_some(), "owned fixture group must be reaped");
}

// ------------------------------------------------------------ review findings

/// Finding 1: Pi's parallel executor prepares every admission before executing
/// any tool, which the serial durable-admission handshake cannot satisfy. The
/// extension must register all four native tools with `executionMode:
/// "sequential"`; the probe also proves the timeout/cancel/timeout-result rules.
#[test]
fn the_extension_forces_sequential_tools_and_bounds_its_bash_timeout() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/operator-pi/extension_probe.mjs");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pi-operator-extension.mjs");
    assert!(fixture.is_file(), "probe missing: {}", fixture.display());
    let output = Command::new(fake_pi::find_node())
        .arg(&fixture)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "extension probe failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("EXTENSION_PROBE PASS"),
        "probe evidence missing: {stdout}"
    );
}

/// A single assistant message with two tool calls is prepared and executed one
/// at a time; both calls and both durable results are journaled in order.
#[test]
fn one_assistant_message_with_two_tool_calls_journals_both_in_order() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::TwoCallBatch {
            id: "call-read".into(),
            name: "read".into(),
            input: json!({"path": "/tmp/fixture-a"}),
            output: "alpha".into(),
            second: "call-bash".into(),
            second_name: "bash".into(),
            second_input: json!({"command": "printf beta"}),
            second_output: "beta".into(),
        },
        Step::assistant("two calls done"),
        Step::Settle,
    ]);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "two tool calls");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Complete,
        "{history:#?}\n{fake:#?}",
        fake = fake.log()
    );
    let calls = harness::tool_calls(&history);
    assert_eq!(
        calls
            .iter()
            .map(|(id, name, _)| (id.as_str(), name.as_str()))
            .collect::<Vec<_>>(),
        vec![("call-read", "read"), ("call-bash", "bash")]
    );
    let outputs = harness::tool_outputs(&history);
    assert_eq!(
        outputs
            .iter()
            .map(|(id, output, _)| (id.as_str(), output.as_str()))
            .collect::<Vec<_>>(),
        vec![("call-read", "alpha"), ("call-bash", "beta")]
    );
    let admissions: Vec<Value> = fake.events("admission");
    assert_eq!(admissions.len(), 2, "{admissions:#?}");
    assert!(admissions.iter().all(|line| line["confirmed"] == true));
    assert_eq!(fake.events("result_ack").len(), 2);
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turned.identity.turn.0 as i64)
    );
}

/// Finding 2: the successful path is not best-effort. A refused control reply,
/// a malformed record after abort, or a nonzero exit must fail the turn with no
/// promotion and no superseded-checkpoint deletion.
#[test]
fn an_unclean_shutdown_fails_the_turn_without_promoting_a_checkpoint() {
    for mode in [
        "clear_queue_refused",
        "abort_refused",
        "abort_malformed",
        "exit_nonzero",
    ] {
        let mut scenario = Scenario::new();
        scenario.steps(vec![Step::assistant("shutdown answer"), Step::Settle]);
        match mode {
            "clear_queue_refused" => {
                scenario.set("clear_queue_result", json!("error"));
            }
            "abort_refused" => {
                scenario.set("abort_result", json!("error"));
            }
            "abort_malformed" => {
                scenario.set("abort_records", json!(["not-json\n"]));
            }
            "exit_nonzero" => {
                scenario.set("close_exit_code", json!(3));
            }
            _ => unreachable!(),
        }
        let (live, fake, mut client) = worker_fixture(&scenario);
        let opened = client.open();
        let turned = client.submit_turn(&opened, "shut down cleanly");
        let history = client.wait_terminal(opened.id);
        assert_eq!(
            history.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Error,
            "{mode}: {history:#?}\n{}",
            serde_json::to_string(&fake.log()).unwrap()
        );
        assert_eq!(
            failed_text(&history).as_deref(),
            Some("Pi request failed; inspect the partial journal before retrying."),
            "{mode}"
        );
        assert!(
            !harness::history_text(&history).contains("Completed"),
            "{mode}: an unclean shutdown must not commit Completed"
        );
        assert_eq!(
            Registry::of(&live).checkpoint(opened.id.0),
            None,
            "{mode}: no checkpoint promotion"
        );
        let attempt = live
            .config
            .join("operator-pi/turns")
            .join(turned.identity.turn.0.to_string());
        assert!(
            !attempt.join("session.jsonl").exists(),
            "{mode}: the native file must not be promoted"
        );
        assert!(
            harness::has_assistant(&history, "shutdown answer"),
            "{mode}: committed visible text stays"
        );
        client.wait_stopped(opened.id);
    }
}

/// Finding 3: Pi's session manager skips malformed JSON lines, so a checkpoint
/// that is valid only in its header must not be resumed. Every record is
/// validated during the copy, and the source bytes are preserved.
#[test]
fn a_checkpoint_with_a_malformed_record_fails_closed_and_preserves_bytes() {
    use std::io::Write;

    let mut first = Scenario::new();
    first.steps(vec![Step::assistant("checkpoint base"), Step::Settle]);
    let (live, fake, mut client) = worker_fixture(&first);
    let opened = client.open();
    let turn_one = client.submit_turn(&opened, "base");
    client.wait_history(opened.id, |history| {
        history.conversation.current_turn.as_ref().unwrap().state == OperatorTurnState::Complete
    });
    client.wait_stopped(opened.id);
    let checkpoint = live
        .config
        .join("operator-pi/turns")
        .join(turn_one.identity.turn.0.to_string())
        .join("session.jsonl");
    assert!(checkpoint.is_file());
    std::fs::OpenOptions::new()
        .append(true)
        .open(&checkpoint)
        .unwrap()
        .write_all(b"not-json\n")
        .unwrap();
    let corrupted = std::fs::read(&checkpoint).unwrap();

    let mut second = Scenario::new();
    second.steps(vec![Step::assistant("must not resume")]);
    fake.script(&second);
    fake.clear_log();
    let summary = client.current().unwrap();
    let turn_two = client.submit_turn(&summary, "resume the corrupted checkpoint");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error
    );
    assert_eq!(
        failed_text(&history).as_deref(),
        Some("Operator Pi context is unavailable; saved data was preserved.")
    );
    assert!(
        fake.log().iter().all(|line| line["event"] != "prompt"),
        "no prompt may follow a refused checkpoint"
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turn_one.identity.turn.0 as i64),
        "the checkpoint reference is unchanged"
    );
    assert_eq!(
        std::fs::read(&checkpoint).unwrap(),
        corrupted,
        "the original checkpoint bytes are preserved"
    );
    let attempt = live
        .config
        .join("operator-pi/turns")
        .join(turn_two.identity.turn.0.to_string());
    assert!(attempt.is_dir(), "the attempt evidence is preserved");
    assert!(
        !attempt.join("session.jsonl").exists(),
        "a refused copy leaves no partial checkpoint behind"
    );
    client.wait_stopped(opened.id);
}

/// Finding 4: Pi validates the tool and its arguments before the extension hook,
/// so an unknown tool or invalid arguments reaches an execution end with no
/// admission and no durable receipt. That must fail the turn closed.
#[test]
fn an_execution_without_the_admission_dialog_fails_the_turn() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::UnadmittedExecution {
            id: "call-unadmitted".into(),
            name: "grep".into(),
            input: json!({"pattern": "x"}),
            output: "Tool grep not found".into(),
        },
        Step::ModelRequest,
        Step::assistant("must not reach the model"),
        Step::Settle,
    ]);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "bypass admission");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error,
        "{history:#?}\n{}",
        serde_json::to_string(&fake.log()).unwrap()
    );
    assert_eq!(
        failed_text(&history).as_deref(),
        Some("Pi request failed; inspect the partial journal before retrying.")
    );
    assert!(
        harness::tool_calls(&history).is_empty(),
        "an unadmitted execution never gets a tool call receipt"
    );
    assert!(
        harness::tool_outputs(&history).is_empty(),
        "an unadmitted execution never gets a tool output receipt"
    );
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    assert!(
        !harness::history_text(&history).contains("Completed"),
        "no Completed after an unadmitted execution"
    );
    let attempt = live
        .config
        .join("operator-pi/turns")
        .join(turned.identity.turn.0.to_string());
    assert!(!attempt.join("session.jsonl").exists());
    client.wait_stopped(opened.id);
}

/// Finding 5: a timed-out tool result is never acknowledged. Pi must not start
/// another model request, and the turn stops with no promoted checkpoint.
#[test]
fn a_timed_out_tool_result_is_not_acknowledged_and_stops_the_turn() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::Tool {
            id: "call-timeout".into(),
            name: "bash".into(),
            input: json!({"command": "sleep 30", "timeout": 0.1}),
            output: "Command timed out after 0.1 seconds".into(),
            status: "timedout",
            truncated: false,
            marker: None,
        },
        Step::ModelRequest,
        Step::assistant("must not reach the model"),
        Step::Settle,
    ]);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "time out a tool");
    let history = client.wait_terminal(opened.id);
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Interrupted,
        "{history:#?}"
    );
    let acknowledgements: Vec<Value> = fake.events("result_ack");
    assert_eq!(acknowledgements.len(), 1, "{acknowledgements:#?}");
    assert_eq!(acknowledgements[0]["id"], "call-timeout");
    assert_eq!(
        acknowledgements[0]["confirmed"], false,
        "a timed-out result must never be acknowledged"
    );
    assert!(
        fake.events("model_request").is_empty(),
        "no further model request may follow a timeout: {:#?}",
        fake.log()
    );
    assert!(
        !deltas_for(&history, turned.identity.turn.0).contains("must not reach the model"),
        "the agent loop was released after the timeout"
    );
    assert!(history.messages.iter().any(|message| matches!(
        &message.content,
        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
            id,
            status: OperatorToolStatus::TimedOut,
            ..
        }) if id == "call-timeout"
    )));
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    assert!(!harness::history_text(&history).contains("Completed"));
    client.wait_stopped(opened.id);
}

/// Finding 6: a peer that streams harmless notifications while withholding the
/// required control reply cannot defer the deadline; it is checked on every
/// iteration, not only on an idle poll.
#[test]
fn a_streaming_peer_cannot_defer_a_control_deadline() {
    let mut scenario = Scenario::new();
    scenario.set("hang_command", json!("clear_queue"));
    scenario.set("hang_flood", json!(true));
    scenario.steps(vec![Step::assistant("deadline answer"), Step::Settle]);
    let (live, fake, mut client) = worker_fixture(&scenario);
    let opened = client.open();
    let started = std::time::Instant::now();
    client.submit_turn(&opened, "hold the control reply");
    let history = client.wait_terminal(opened.id);
    let elapsed = started.elapsed();
    assert_eq!(
        history.conversation.current_turn.as_ref().unwrap().state,
        OperatorTurnState::Error,
        "{history:#?}"
    );
    assert_eq!(
        failed_text(&history).as_deref(),
        Some("Pi request failed; inspect the partial journal before retrying.")
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "the 5s control bound held despite streamed events: {elapsed:?}"
    );
    let hung = fake
        .log()
        .iter()
        .filter(|line| line["event"] == "command" && line["hung"] == true)
        .count();
    assert_nonzero(hung, "hung control command receipts");
    assert_nonzero(
        fake.events("hang_flood").len(),
        "peer flood receipts while withholding the control reply",
    );
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    client.wait_stopped(opened.id);
}
