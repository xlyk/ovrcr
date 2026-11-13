//! Operator acceptance on the real application path.
//!
//! The singleton lifecycle, turn/checkpoint authority and process-stop proof are
//! driven through the compiled Server, a managed worker and a deterministic
//! fake Pi RPC peer. `installed_pi_operator_single_conversation` is the
//! separate opt-in proof against the installed `pi`, and
//! `operator_pi_gui_provider` scripts the same scenarios for the GUI smoke.
//!
//! Every fixture owns a temporary HOME/OVRCR_HOME/socket/Git workspace, its
//! private Pi profile and its own processes. No personal instance or real
//! credentials are ever used.

#[path = "support/fake_pi.rs"]
mod fake_pi;
#[path = "support/live.rs"]
mod live;
#[path = "support/operator_harness.rs"]
mod operator_harness;
#[cfg(feature = "acceptance-diagnostics")]
#[path = "support/operator_provider.rs"]
mod operator_provider;

use fake_pi::{FakePi, Scenario, Step};
use live::Live;
use operator_harness::{OperatorClient, Registry, has_assistant, tool_calls, tool_outputs};
use ovrcr::protocol::{
    OperatorMessageContent, OperatorRequest, OperatorTurnState, OperatorWorkerEvent, Request,
    Response, SessionId, SessionRunId,
};
use serde_json::json;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::Duration,
};

fn fixture(scenario: &Scenario) -> (Live, FakePi, OperatorClient) {
    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    fake.script(scenario);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let client = OperatorClient::connect(&live);
    (live, fake, client)
}

fn turn_dir(live: &Live, turn: u64) -> PathBuf {
    live.config.join("operator-pi/turns").join(turn.to_string())
}

fn checkpoint_file(live: &Live, turn: u64) -> PathBuf {
    turn_dir(live, turn).join("session.jsonl")
}

fn state(history: &ovrcr::protocol::OperatorHistory) -> OperatorTurnState {
    history.conversation.current_turn.as_ref().unwrap().state
}

fn fail_message(history: &ovrcr::protocol::OperatorHistory) -> Option<String> {
    history
        .messages
        .iter()
        .find_map(|message| match &message.content {
            OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(text)) => Some(text.clone()),
            _ => None,
        })
}

fn complete(
    client: &mut OperatorClient,
    conversation: ovrcr::protocol::OperatorConversationId,
) -> ovrcr::protocol::OperatorHistory {
    let history = client.wait_terminal(conversation);
    assert_eq!(state(&history), OperatorTurnState::Complete, "{history:#?}");
    client.wait_stopped(conversation)
}

#[test]
fn reasoning_changes_during_a_turn_apply_only_to_the_next_turn() {
    use ovrcr::protocol::OperatorThinkingLevel as Level;
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::Delta("held turn".into()), Step::Hold]);
    let (live, fake, mut client) = fixture(&scenario);
    let output = live
        .command()
        .args(["settings", "set", "operator.thinking_level", "low"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let opened = client.open();
    let first = client.submit_turn(&opened, "first");
    let history = client.wait_history(opened.id, |history| has_assistant(history, "held turn"));
    assert_eq!(
        history.conversation.reasoning.as_ref().unwrap().level,
        Level::Low
    );
    assert_eq!(
        client.exchange(Request::SetSetting {
            path: "operator.thinking_level".into(),
            value: Some("'high'".into()),
        }),
        Response::Ok
    );
    let history = client.read(opened.id.0, 0, 100);
    assert_eq!(
        history.conversation.reasoning.as_ref().unwrap().level,
        Level::Low
    );
    let levels: Vec<_> = fake
        .log()
        .into_iter()
        .filter(|line| line["command"] == "set_thinking_level")
        .collect();
    assert_eq!(levels.len(), 1, "no mid-turn reconfiguration");
    assert_eq!(levels[0]["level"], "low");
    client.stop_turn(first.identity);
    client.wait_stopped(opened.id);

    let mut second = Scenario::new();
    second.steps(vec![Step::assistant("next turn"), Step::Settle]);
    fake.script(&second);
    let current = client.current().unwrap();
    client.submit_turn(&current, "second");
    let history = complete(&mut client, opened.id);
    assert_eq!(
        history.conversation.reasoning.as_ref().unwrap().requested,
        Level::High
    );
    assert_eq!(
        history.conversation.reasoning.as_ref().unwrap().level,
        Level::High
    );
    let output = live
        .command()
        .args(["settings", "set", "operator.thinking_level", "bogus"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "unknown levels must be refused by the CLI writer"
    );
}

#[test]
fn operator_cli_is_the_native_profile_entry_point() {
    let live = Live::idle();
    let output = live
        .command()
        .args(["operator", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("setup"), "{help}");
    assert!(help.contains("models"), "{help}");
    assert!(!help.contains("worker"), "worker is hidden: {help}");
}

#[test]
fn an_idle_async_wakeup_is_journaled_once_on_the_leased_run_and_retires_cleanly() {
    let event_log = PathBuf::from("/private/tmp/ovrcr-468-async-wakeup-fake-pi-retry.log");
    let _ = std::fs::remove_file(&event_log);
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::WatchStatus(1),
        Step::assistant("initial turn"),
        Step::Settle,
        Step::async_wakeup(
            "A watched Workspace Agent finished.",
            "I reviewed the completed session.",
        ),
    ]);
    scenario.set("log", event_log.display().to_string());
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let first = client.submit_turn(&opened, "start watcher");
    assert_eq!(first.identity.run, SessionRunId(1));

    let final_history = client.wait_history(opened.id, |history| {
        history.messages.iter().any(|message| {
            matches!(
                &message.content,
                OperatorMessageContent::AsyncWakeup { text }
                    if text == "A watched Workspace Agent finished."
            )
        }) && history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity.turn.0 != first.identity.turn.0
                    && turn.state == OperatorTurnState::Complete
            })
    });
    client.wait_stopped(opened.id);

    let wakeups: Vec<_> = final_history
        .messages
        .iter()
        .filter_map(|message| match &message.content {
            OperatorMessageContent::AsyncWakeup { text } => Some((message.turn, text)),
            _ => None,
        })
        .collect();
    assert_eq!(
        wakeups.len(),
        1,
        "the settled batch creates one journal row"
    );
    let turn = final_history.conversation.current_turn.as_ref().unwrap();
    assert_eq!(wakeups[0].0, turn.identity.turn);
    assert_eq!(turn.identity.session, first.identity.session);
    assert_eq!(turn.identity.run, first.identity.run);
    assert!(has_assistant(
        &final_history,
        "I reviewed the completed session."
    ));
    assert_eq!(
        fake.log()
            .iter()
            .filter(|line| line["event"] == "prompt")
            .count(),
        1,
        "the wakeup is the already-started native Pi request, not a second RPC prompt"
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turn.identity.turn.0 as i64),
        "the accepted wakeup becomes the next durable native-session checkpoint"
    );
    let admissions: Vec<_> = fake
        .log()
        .into_iter()
        .filter(|line| line["event"] == "async_wakeup_admission")
        .collect();
    assert!(
        admissions.len() >= 2,
        "the pre-settle offer is retried at idle: {admissions:?}"
    );
    assert_eq!(admissions[0]["confirmed"], false);
    for retry in &admissions[1..admissions.len() - 1] {
        assert_eq!(retry["confirmed"], false);
        assert_eq!(retry["retry"], true);
    }
    assert_eq!(admissions.last().unwrap()["confirmed"], true);
    assert_eq!(admissions.last().unwrap()["retry"], true);
}

#[test]
fn current_never_creates_and_open_yields_one_durable_conversation() {
    let scenario = Scenario::new();
    let (live, _fake, mut client) = fixture(&scenario);

    assert!(
        client.current().is_none(),
        "Current must not create a record"
    );
    let registry = Registry::of(&live);
    assert!(
        registry.conversation_ids().is_empty(),
        "Current must not insert a conversation"
    );
    assert!(
        !live.config.join("operator-pi/turns").exists(),
        "Current must not prepare attempt directories"
    );

    let opened = client.open();
    assert_eq!(opened.title, "Operator");
    assert_eq!(opened.run.0, 0);
    assert!(opened.model.is_none());
    assert!(opened.current_turn.is_none());
    assert!(opened.worker_stopped);
    assert_eq!(client.current().as_ref(), Some(&opened));
    assert_eq!(client.open(), opened, "repeated Open keeps the same IDs");
    let ids = Registry::of(&live).conversation_ids();
    assert_eq!(ids, vec![opened.id.0], "exactly one durable conversation");
    assert!(!ids.is_empty());

    // Open never starts Pi: no worker process and no attempt directory.
    let session = client.session(opened.session);
    assert!(
        session.pid.is_none(),
        "Open must not start a worker: {session:#?}"
    );
    assert!(
        !live
            .config
            .join("operator-pi/turns")
            .read_dir()
            .is_ok_and(|mut entries| entries.next().is_some()),
        "Open must not create attempt directories"
    );

    // A reconnect reads the same singleton and still starts nothing.
    drop(client);
    let mut reconnected = OperatorClient::connect(&live);
    assert_eq!(reconnected.current(), Some(opened.clone()));
    assert!(reconnected.session(opened.session).pid.is_none());
    assert_eq!(Registry::of(&live).conversation_ids(), vec![opened.id.0]);
}

#[test]
fn leased_worker_reuses_one_live_run_for_warm_user_turns_then_retires() {
    let mut scenario = Scenario::new();
    scenario.set(
        "model_default_thinking",
        json!({"fixture/default": "medium", "fixture/alternate": "low"}),
    );
    scenario.prompt_steps(vec![
        vec![
            Step::assistant("first leased answer"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::assistant("second leased answer"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::assistant("final unleased answer"),
            Step::WatchStatus(0),
            Step::Settle,
        ],
    ]);
    let (live, fake, mut client) = fixture(&scenario);
    for (key, value) in [
        ("operator.model", "fixture/alternate"),
        ("operator.thinking_level", "high"),
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
    let opened = client.open();
    let first = client.submit_turn(&opened, "launch watched work");
    let leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete && history.conversation.worker_live
    });
    assert_eq!(first.identity.run, SessionRunId(1));
    assert_eq!(
        leased.conversation.current_turn.as_ref().unwrap().identity,
        first.identity
    );
    assert!(!leased.conversation.worker_stopped);
    assert!(client.session(opened.session).pid.is_some());
    let first_checkpoint = checkpoint_file(&live, first.identity.turn.0);
    assert!(
        first_checkpoint.is_file(),
        "leased turns publish a separate checkpoint copy"
    );
    assert_eq!(
        std::fs::symlink_metadata(&first_checkpoint).unwrap().mode() & 0o777,
        0o600
    );
    assert!(
        std::fs::read_to_string(&first_checkpoint)
            .unwrap()
            .starts_with("{\"type\":\"session\""),
        "the copied checkpoint starts with the validated native header"
    );
    assert!(matches!(
        live.request(Request::Shutdown { kill: false }),
        Response::Error {
            code: ovrcr::protocol::ErrorCode::SessionsRemain,
            ..
        }
    ));

    assert_eq!(
        client.exchange(Request::SetSetting {
            path: "operator.model".into(),
            value: None,
        }),
        Response::Ok
    );
    assert_eq!(
        client.exchange(Request::SetSetting {
            path: "operator.thinking_level".into(),
            value: Some("'high'".into()),
        }),
        Response::Ok
    );
    let second = client.submit_turn(&leased.conversation, "continue while watched");
    assert_eq!(
        second.identity.run, first.identity.run,
        "warm Submit keeps the worker run"
    );
    assert!(second.identity.turn.0 > first.identity.turn.0);

    let second_leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete
            && history.conversation.worker_live
            && history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.identity == second.identity)
    });
    assert_eq!(
        second_leased.conversation.model,
        Some(ovrcr::protocol::OperatorModel {
            provider: "fixture".into(),
            id: "default".into(),
        }),
        "clearing an explicit model applies the Pi profile default on a warm turn"
    );
    assert_eq!(
        client.exchange(Request::SetSetting {
            path: "operator.thinking_level".into(),
            value: Some("'default'".into()),
        }),
        Response::Ok
    );
    let third = client.submit_turn(&second_leased.conversation, "restore profile reasoning");
    assert_eq!(third.identity.run, first.identity.run);
    assert!(third.identity.turn.0 > second.identity.turn.0);

    let final_history = client.wait_stopped(opened.id);
    assert_eq!(state(&final_history), OperatorTurnState::Complete);
    assert_eq!(
        final_history
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity,
        third.identity
    );
    assert!(!final_history.conversation.worker_live);
    assert!(final_history.conversation.worker_stopped);
    assert!(client.session(opened.session).pid.is_none());
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(third.identity.turn.0 as i64)
    );
    assert!(
        !first_checkpoint.exists(),
        "the new checkpoint replaces the old copy"
    );

    let starts = fake.events("start");
    let prompts = fake.events("prompt");
    assert_eq!(
        starts.len(),
        1,
        "the leased follow-up reused one Pi process"
    );
    assert_eq!(prompts.len(), 3);
    assert!(
        prompts[0]["message"]
            .as_str()
            .unwrap()
            .contains("launch watched work")
    );
    assert!(
        prompts[1]["message"]
            .as_str()
            .unwrap()
            .contains("continue while watched")
    );
    assert!(
        prompts[2]["message"]
            .as_str()
            .unwrap()
            .contains("restore profile reasoning")
    );
    let selected_models: Vec<_> = fake
        .events("command")
        .into_iter()
        .filter(|event| event["command"] == "set_model")
        .map(|event| event["modelId"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(selected_models, ["alternate", "default", "default"]);
    let selected_levels: Vec<_> = fake
        .events("command")
        .into_iter()
        .filter(|event| event["command"] == "set_thinking_level")
        .map(|event| event["level"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        selected_levels,
        ["high", "high", "medium"],
        "Default restores the captured profile level after an explicit warm level"
    );
    assert_eq!(
        final_history.conversation.reasoning.as_ref().unwrap().level,
        ovrcr::protocol::OperatorThinkingLevel::Medium
    );
    assert_eq!(
        live.request(Request::Shutdown { kill: false }),
        Response::Ok,
        "clean unleased retirement leaves non-kill shutdown available"
    );
    assert!(matches!(
        live.join_within(Duration::from_secs(10)),
        live::Stop::Finished
    ));
}

#[test]
fn leased_stop_rolls_back_on_the_live_worker_without_advancing_checkpoint() {
    let marker_dir = tempfile::tempdir().unwrap();
    let release_abort_settlement = marker_dir.path().join("release-abort-settlement");
    let release_wakeup = marker_dir.path().join("release-wakeup");
    let mut scenario = Scenario::new();
    scenario.prompt_steps(vec![
        vec![
            Step::assistant("completed checkpoint"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::Start,
            Step::Delta("partial stopped answer".into()),
            Step::WatchStatus(1),
            Step::Hold,
        ],
    ]);
    scenario.set(
        "abort_settle_release_file",
        release_abort_settlement.display().to_string(),
    );
    scenario.set(
        "wakeup_after_switch",
        json!({
            "text": "watch report queued during Stop rollback",
            "answer": "the live worker handled the queued watch report",
            "release_file": release_wakeup.display().to_string(),
        }),
    );
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let first = client.submit_turn(&opened, "create a checkpoint");
    let leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete && history.conversation.worker_live
    });
    let worker_pid = client.session(opened.session).pid.unwrap();
    let second = client.submit_turn(&leased.conversation, "start a cancellable turn");
    let partial = client.wait_history(opened.id, |history| {
        has_assistant(history, "partial stopped answer")
            && state(history) == OperatorTurnState::Running
    });
    assert_eq!(second.identity.run, first.identity.run);

    // Send Stop without reading its reply, then reconnect while the fake Pi is
    // deliberately holding agent_settled. The durable turn must remain busy.
    drop(client);
    let mut stop_stream = ovrcr::protocol::connect_server(&live.socket).unwrap();
    let hello =
        ovrcr::protocol::client::request(&mut stop_stream, 900_000, Request::DashboardHello)
            .unwrap();
    assert!(
        matches!(hello, Response::Hierarchy(_)),
        "Dashboard handshake: {hello:?}"
    );
    ovrcr::protocol::write_frame(
        &mut stop_stream,
        &ovrcr::protocol::ClientMessage {
            request_id: 900_001,
            request: Request::Operator(OperatorRequest::Stop {
                identity: second.identity,
            }),
        },
    )
    .unwrap();
    fake.wait_log(Duration::from_secs(3), |log| {
        log.iter()
            .any(|line| line["event"] == "command" && line["command"] == "abort")
    });
    drop(stop_stream);
    let mut reconnect = OperatorClient::reconnect_after_disconnect(&live);
    let current = reconnect.current().unwrap();
    let busy = reconnect.read(opened.id.0, 0, 100);
    assert_eq!(state(&busy), OperatorTurnState::Running);
    assert!(busy.conversation.worker_live);
    assert!(matches!(
        reconnect.submit(&current, "cannot enter during native rollback"),
        Response::Error { .. }
    ));

    std::fs::write(&release_abort_settlement, "settle").unwrap();
    let stopped = reconnect.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Interrupted
    });
    assert_eq!(state(&stopped), OperatorTurnState::Interrupted);
    let history = reconnect.read(opened.id.0, 0, 100);
    assert!(has_assistant(&history, "partial stopped answer"));
    assert!(
        history.conversation.worker_live,
        "the leased Pi worker remains live"
    );
    assert!(!history.conversation.worker_stopped);
    assert_eq!(
        reconnect.session(opened.session).pid,
        Some(worker_pid),
        "the exact worker process remains attached to the leased run"
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(first.identity.turn.0 as i64),
        "Stop never promotes its partial session"
    );
    assert_eq!(
        Registry::of(&live)
            .turn_state(second.identity.turn.0)
            .as_deref(),
        Some("Interrupted")
    );
    std::fs::write(&release_wakeup, "offer").unwrap();
    let final_history = reconnect.wait_history(opened.id, |history| {
        history.messages.iter().any(|message| {
            matches!(
                &message.content,
                OperatorMessageContent::AsyncWakeup { text }
                    if text == "watch report queued during Stop rollback"
            )
        }) && history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity.turn.0 > second.identity.turn.0
                    && turn.state == OperatorTurnState::Complete
            })
    });
    reconnect.wait_stopped(opened.id);
    assert!(has_assistant(
        &final_history,
        "the live worker handled the queued watch report"
    ));
    assert_eq!(
        final_history
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .run,
        first.identity.run
    );
    assert_eq!(
        fake.events("start").len(),
        1,
        "Stop rollback and wakeup reuse the original Pi process"
    );
    assert!(
        partial.messages.iter().any(|message| {
            matches!(
                &message.content,
                OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text))
                    if text.contains("partial stopped answer")
            )
        }),
        "the visible partial journal remains available"
    );
    let switches: Vec<_> = fake
        .log()
        .into_iter()
        .filter(|line| line["event"] == "switch")
        .collect();
    assert_eq!(
        switches.len(),
        1,
        "rollback switches to the completed checkpoint"
    );
    assert!(switches[0]["exists"].as_bool().unwrap());
    assert!(
        std::fs::read_to_string(switches[0]["path"].as_str().unwrap())
            .unwrap()
            .contains("completed checkpoint"),
        "the replacement Pi session is based on the completed checkpoint"
    );
}

#[test]
fn leased_stop_uses_the_five_second_owned_group_fallback_when_pi_does_not_settle() {
    let marker_dir = tempfile::tempdir().unwrap();
    let never_release = marker_dir.path().join("abort-settlement-never-released");
    let mut scenario = Scenario::new();
    scenario.prompt_steps(vec![
        vec![
            Step::assistant("fallback checkpoint"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::Start,
            Step::Delta("partial fallback answer".into()),
            Step::WatchStatus(1),
            Step::Hold,
        ],
    ]);
    scenario.set(
        "abort_settle_release_file",
        never_release.display().to_string(),
    );
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let first = client.submit_turn(&opened, "create the fallback checkpoint");
    let leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete && history.conversation.worker_live
    });
    let second = client.submit_turn(&leased.conversation, "stop a Pi peer that never settles");
    client.wait_history(opened.id, |history| {
        has_assistant(history, "partial fallback answer")
            && state(history) == OperatorTurnState::Running
    });
    let pgid = operator_harness::own_worker_group(&live, &mut client, opened.session);
    let started = std::time::Instant::now();

    let stopped = client.stop_turn(second.identity);
    let elapsed = started.elapsed();
    assert_eq!(stopped.state, OperatorTurnState::Interrupted);
    assert!(
        elapsed >= Duration::from_millis(4500) && elapsed < Duration::from_secs(8),
        "the Server waits about five seconds before hard fallback; elapsed {elapsed:?}"
    );
    let final_history = client.read(opened.id.0, 0, 100);
    assert_eq!(state(&final_history), OperatorTurnState::Interrupted);
    assert!(has_assistant(&final_history, "partial fallback answer"));
    assert!(!final_history.conversation.worker_live);
    assert!(final_history.conversation.worker_stopped);
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(first.identity.turn.0 as i64),
        "hard fallback does not promote the stopped attempt"
    );
    assert!(
        fake.log()
            .iter()
            .any(|line| line["event"] == "command" && line["command"] == "abort"),
        "the worker attempts scoped native abort before escalation"
    );
    operator_harness::assert_group_absent(&live, pgid);
}

#[test]
fn leased_first_turn_stop_restores_an_empty_native_session() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::Start,
        Step::Delta("partial first-turn answer".into()),
        Step::WatchStatus(1),
        Step::Hold,
    ]);
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let turn = client.submit_turn(&opened, "cancel before the first checkpoint");
    let running = client.wait_history(opened.id, |history| {
        has_assistant(history, "partial first-turn answer")
            && state(history) == OperatorTurnState::Running
    });
    let worker_pid = client.session(opened.session).pid.unwrap();

    let stopped = client.stop_turn(turn.identity);
    assert_eq!(stopped.state, OperatorTurnState::Interrupted);
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), None);
    assert!(running.conversation.worker_live);
    assert!(client.session(opened.session).pid == Some(worker_pid));
    let history = client.read(opened.id.0, 0, 100);
    assert!(has_assistant(&history, "partial first-turn answer"));

    let switches: Vec<_> = fake
        .log()
        .into_iter()
        .filter(|line| line["event"] == "switch")
        .collect();
    assert_eq!(
        switches.len(),
        1,
        "a changed first turn switches to an empty session"
    );
    let target = switches[0]["path"].as_str().unwrap();
    assert!(switches[0]["exists"].as_bool().unwrap());
    let restored = std::fs::read_to_string(target).unwrap();
    assert_eq!(
        restored.lines().count(),
        1,
        "the first-turn rollback has only a session header"
    );
    let header: serde_json::Value = serde_json::from_str(restored.lines().next().unwrap()).unwrap();
    assert_eq!(header["type"], "session");
    assert_eq!(header["id"], format!("operator-{}", opened.id.0));

    let partial = turn_dir(&live, turn.identity.turn.0).join("session.partial.jsonl");
    assert!(
        partial.is_file(),
        "the failed native attempt remains inspectable"
    );
    assert!(
        std::fs::read_to_string(partial)
            .unwrap()
            .contains("cancel before the first checkpoint"),
        "the partial native journal retains the user's prompt"
    );
}

#[test]
fn leased_zero_record_failure_keeps_the_worker_without_switching_sessions() {
    let mut scenario = Scenario::new();
    scenario.prompt_steps(vec![
        vec![
            Step::assistant("completed zero-change checkpoint"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::assistant("follow-up after zero-record failure"),
            Step::WatchStatus(0),
            Step::Settle,
        ],
    ]);
    scenario.set("prompt_responses", json!(["ok", "error", "ok"]));
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let first = client.submit_turn(&opened, "create the checkpoint");
    let leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete && history.conversation.worker_live
    });
    let worker_pid = client.session(opened.session).pid.unwrap();

    let failed = client.submit_turn(
        &leased.conversation,
        "fail before native records are written",
    );
    let failure = client.wait_history(opened.id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity == failed.identity && turn.state == OperatorTurnState::Error
            })
    });
    assert_eq!(failed.identity.run, first.identity.run);
    assert!(failure.conversation.worker_live);
    assert_eq!(client.session(opened.session).pid, Some(worker_pid));
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(first.identity.turn.0 as i64)
    );
    assert!(has_assistant(&failure, "completed zero-change checkpoint"));

    let log = fake.log();
    assert_eq!(
        log.iter().filter(|line| line["event"] == "switch").count(),
        0,
        "a failure with no native-record changes skips switch_session"
    );
    for command in ["clear_queue", "abort"] {
        assert!(
            log.iter()
                .any(|line| line["event"] == "command" && line["command"] == command),
            "zero-record rollback still issues {command}"
        );
    }
    let partial = turn_dir(&live, failed.identity.turn.0).join("session.partial.jsonl");
    assert!(
        partial.is_file(),
        "the unchanged native journal is retained as an attempt artifact"
    );

    client.submit_turn(
        &failure.conversation,
        "continue from the completed checkpoint",
    );
    let completed = client.wait_stopped(opened.id);
    assert!(has_assistant(
        &completed,
        "follow-up after zero-record failure"
    ));
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), Some(3));
}

#[test]
fn leased_failure_with_native_changes_rolls_back_and_keeps_the_partial_journal() {
    let mut scenario = Scenario::new();
    scenario.prompt_steps(vec![
        vec![
            Step::assistant("failure rollback checkpoint"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::Start,
            Step::Delta("partial answer before failure".into()),
            Step::Event(json!({"type": "extension_error", "message": "injected provider failure"})),
            Step::Hold,
        ],
        vec![
            Step::assistant("follow-up after recovered failure"),
            Step::WatchStatus(0),
            Step::Settle,
        ],
    ]);
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let first = client.submit_turn(&opened, "create the failure checkpoint");
    let leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete && history.conversation.worker_live
    });
    let failed = client.submit_turn(&leased.conversation, "fail after appending native records");
    let failure = client.wait_history(opened.id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity == failed.identity && turn.state == OperatorTurnState::Error
            })
    });
    assert_eq!(failed.identity.run, first.identity.run);
    assert!(has_assistant(&failure, "partial answer before failure"));
    assert!(failure.conversation.worker_live);
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(first.identity.turn.0 as i64)
    );
    let attempt = turn_dir(&live, failed.identity.turn.0);
    assert!(attempt.join("session.partial.jsonl").is_file());
    assert!(!attempt.join("session.jsonl").exists());
    let switches: Vec<_> = fake
        .log()
        .into_iter()
        .filter(|line| line["event"] == "switch")
        .collect();
    assert_eq!(
        switches.len(),
        1,
        "a changed native session switches back to the checkpoint"
    );
    let restored = std::fs::read_to_string(switches[0]["path"].as_str().unwrap()).unwrap();
    assert!(restored.contains("failure rollback checkpoint"));
    assert!(!restored.contains("fail after appending native records"));
    assert!(!restored.contains("partial answer before failure"));

    client.submit_turn(
        &failure.conversation,
        "continue from the recovered checkpoint",
    );
    let completed = client.wait_stopped(opened.id);
    assert!(has_assistant(
        &completed,
        "follow-up after recovered failure"
    ));
    assert_eq!(Registry::of(&live).checkpoint(opened.id.0), Some(3));
    assert_eq!(
        fake.events("start").len(),
        1,
        "a failed leased turn reuses the original Pi process"
    );
}

#[test]
fn leased_tool_timeout_rolls_back_before_accepting_an_async_wakeup() {
    let marker_dir = tempfile::tempdir().unwrap();
    let release_wakeup = marker_dir.path().join("release-wakeup");
    let fake_log = PathBuf::from("/private/tmp/ovrcr-469-tool-timeout-fake-pi.jsonl");
    let _ = std::fs::remove_file(&fake_log);
    let mut scenario = Scenario::new();
    scenario.prompt_steps(vec![
        vec![
            Step::assistant("timeout rollback checkpoint"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::Start,
            Step::Tool {
                id: "timeout-after-lease".into(),
                name: "bash".into(),
                input: json!({"command": "sleep 30", "timeout": 0.1}),
                output: "Command timed out after 0.1 seconds".into(),
                status: "timedout",
                truncated: false,
                marker: None,
            },
            Step::ModelRequest,
            Step::assistant("must not reach the model after timeout"),
            Step::Settle,
        ],
    ]);
    scenario.set(
        "wakeup_after_switch",
        json!({
            "text": "the watched session settled during rollback",
            "answer": "I resumed the queued watch report after rollback",
            "release_file": release_wakeup.display().to_string(),
        }),
    );
    scenario.set("log", fake_log.display().to_string());
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let first = client.submit_turn(&opened, "create timeout rollback checkpoint");
    let leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete && history.conversation.worker_live
    });
    let timed_out = client.submit_turn(&leased.conversation, "run a timed-out tool");

    let failed_history = client.wait_history(opened.id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity == timed_out.identity && turn.state == OperatorTurnState::Error
            })
    });
    assert_eq!(
        Registry::of(&live)
            .turn_state(timed_out.identity.turn.0)
            .as_deref(),
        Some("Error")
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(first.identity.turn.0 as i64)
    );
    assert!(failed_history.conversation.worker_live);
    std::fs::write(&release_wakeup, "continue").unwrap();

    let final_history = client.wait_history(opened.id, |history| {
        history.messages.iter().any(|message| {
            matches!(
                &message.content,
                OperatorMessageContent::AsyncWakeup { text }
                    if text == "the watched session settled during rollback"
            )
        }) && history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity.turn.0 > timed_out.identity.turn.0
                    && turn.state == OperatorTurnState::Complete
            })
    });
    client.wait_stopped(opened.id);

    assert_eq!(timed_out.identity.run, first.identity.run);
    assert_eq!(
        Registry::of(&live)
            .turn_state(timed_out.identity.turn.0)
            .as_deref(),
        Some("Error")
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        final_history
            .conversation
            .current_turn
            .as_ref()
            .map(|turn| turn.identity.turn.0 as i64),
        "the accepted async wakeup is the only post-checkpoint promotion"
    );
    assert_ne!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(timed_out.identity.turn.0 as i64),
        "the timed-out turn never becomes a checkpoint"
    );
    assert!(has_assistant(
        &final_history,
        "I resumed the queued watch report after rollback"
    ));
    assert!(final_history.messages.iter().any(|message| {
        message.turn == timed_out.identity.turn
            && matches!(
                &message.content,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                    id,
                    status: ovrcr::protocol::OperatorToolStatus::TimedOut,
                    ..
                }) if id == "timeout-after-lease"
            )
    }));
    assert!(
        !final_history.messages.iter().any(|message| {
            message.turn == timed_out.identity.turn
                && matches!(
                    &message.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text))
                        if text.contains("must not reach the model after timeout")
                )
        }),
        "Pi never receives another model request after timeout"
    );
    let wakeup = final_history.conversation.current_turn.as_ref().unwrap();
    assert_eq!(wakeup.identity.run, first.identity.run);
    assert_eq!(
        fake.events("start").len(),
        1,
        "rollback and wakeup reuse the original Pi process"
    );
    let attempt = turn_dir(&live, timed_out.identity.turn.0);
    assert!(attempt.join("session.partial.jsonl").is_file());
    assert!(!attempt.join("session.jsonl").exists());
    let switches: Vec<_> = fake
        .log()
        .into_iter()
        .filter(|line| line["event"] == "switch")
        .collect();
    assert_eq!(
        switches.len(),
        1,
        "timeout rollback uses one in-place native session switch"
    );
    let restored = std::fs::read_to_string(switches[0]["path"].as_str().unwrap()).unwrap();
    assert!(restored.contains("timeout rollback checkpoint"));
    assert!(!restored.contains("run a timed-out tool"));
    assert!(
        fake.events("async_wakeup_admission")
            .iter()
            .any(|event| event["confirmed"] == true)
    );
}

#[test]
fn cold_resumed_leased_worker_keeps_its_active_session_across_warm_turns() {
    let mut cold = Scenario::new();
    cold.steps(vec![Step::assistant("cold checkpoint base"), Step::Settle]);
    let (live, fake, mut client) = fixture(&cold);
    let opened = client.open();
    let first = client.submit_turn(&opened, "make a cold checkpoint");
    complete(&mut client, opened.id);

    let mut resumed = Scenario::new();
    resumed.prompt_steps(vec![
        vec![
            Step::assistant("cold resumed answer"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::assistant("first warm answer"),
            Step::WatchStatus(1),
            Step::Settle,
        ],
        vec![
            Step::assistant("second warm answer"),
            Step::WatchStatus(0),
            Step::Settle,
        ],
    ]);
    fake.script(&resumed);
    let current = client.current().unwrap();
    let second = client.submit_turn(&current, "resume and lease");
    assert_ne!(second.identity.run, first.identity.run);
    let second_history = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete
            && history.conversation.worker_live
            && history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.identity == second.identity)
    });
    let active_session = live
        .config
        .join("operator-pi/runs")
        .join(second.identity.run.0.to_string())
        .join("session.jsonl");
    let second_checkpoint = checkpoint_file(&live, second.identity.turn.0);
    assert!(
        active_session.is_file(),
        "cold resume creates the session file Pi keeps active for this worker"
    );
    assert!(
        second_checkpoint.is_file(),
        "leased completion promotes a canonical per-turn session.jsonl copy"
    );

    let third = client.submit_turn(&second_history.conversation, "first warm turn");
    let third_history = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete
            && history.conversation.worker_live
            && history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.identity == third.identity)
    });
    assert!(
        active_session.is_file(),
        "warm cleanup must preserve Pi's live file"
    );

    let fourth = client.submit_turn(&third_history.conversation, "second warm turn");
    let final_history = client.wait_stopped(opened.id);
    assert_eq!(
        final_history
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity,
        fourth.identity
    );
    assert!(
        active_session.is_file(),
        "retirement keeps the worker's source file"
    );
    let promoted = checkpoint_file(&live, fourth.identity.turn.0);
    let contents = std::fs::read_to_string(promoted).unwrap();
    for expected in [
        "cold checkpoint base",
        "cold resumed answer",
        "first warm answer",
        "second warm answer",
    ] {
        assert!(
            contents.contains(expected),
            "checkpoint lost {expected}: {contents}"
        );
    }
    assert!(has_assistant(&final_history, "second warm answer"));
}

#[test]
fn kill_shutdown_terminates_a_leased_worker_and_its_pi_group() {
    let mut scenario = Scenario::new();
    scenario.prompt_steps(vec![vec![
        Step::assistant("leased before kill"),
        Step::WatchStatus(1),
        Step::Settle,
    ]]);
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    client.submit_turn(&opened, "start leased worker");
    let leased = client.wait_history(opened.id, |history| {
        state(history) == OperatorTurnState::Complete && history.conversation.worker_live
    });
    assert!(leased.conversation.worker_live);
    assert_eq!(fake.events("start").len(), 1);

    let pgid = operator_harness::own_worker_group(&live, &mut client, opened.session);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    operator_harness::assert_group_absent(&live, pgid);
    assert!(matches!(
        live.join_within(Duration::from_secs(10)),
        live::Stop::Finished
    ));
}

#[test]
fn only_completed_turns_advance_the_checkpoint_across_failure_and_interruption() {
    let marker_dir = tempfile::tempdir().unwrap();
    let marker = marker_dir.path().join("fixture-marker");
    let mut first = Scenario::new();
    first.steps(vec![
        Step::Tool {
            id: "call-write".into(),
            name: "write".into(),
            input: json!({"path": marker.display().to_string(), "content": "first-answer"}),
            output: "wrote fixture-marker".into(),
            status: "succeeded",
            truncated: false,
            marker: Some(marker.clone()),
        },
        Step::assistant("first answer"),
        Step::Settle,
    ]);
    let (live, fake, mut client) = fixture(&first);
    let opened = client.open();
    let turn_one = client.submit_turn(&opened, "first");
    let history = complete(&mut client, opened.id);
    assert!(has_assistant(&history, "first answer"));
    let calls = tool_calls(&history);
    assert_eq!(calls.len(), 1, "one durable tool receipt: {calls:?}");
    assert_eq!(calls[0].1, "write");
    assert_eq!(tool_outputs(&history).len(), 1);
    assert!(marker.exists(), "the admitted write executed");

    let registry = Registry::of(&live);
    let turn_one_id = turn_one.identity.turn.0;
    assert_eq!(registry.checkpoint(opened.id.0), Some(turn_one_id as i64));
    assert!(
        !registry.batch_digests(turn_one_id).is_empty(),
        "a completed turn has committed batch receipts"
    );
    let checkpoint = checkpoint_file(&live, turn_one_id);
    assert!(checkpoint.is_file());
    assert_eq!(
        std::fs::symlink_metadata(&checkpoint).unwrap().mode() & 0o777,
        0o600
    );

    // A failing turn never promotes its checkpoint.
    let mut failing = Scenario::new();
    failing.steps(vec![Step::Event(
        json!({"type": "extension_error", "message": "provider"}),
    )]);
    fake.script(&failing);
    fake.clear_log();
    let summary = client.current().unwrap();
    let turn_two = client.submit_turn(&summary, "second");
    let failed = client.wait_terminal(opened.id);
    assert_eq!(state(&failed), OperatorTurnState::Error);
    assert_eq!(
        fail_message(&failed).as_deref(),
        Some("Pi request failed; inspect the partial journal before retrying.")
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turn_one_id as i64),
        "a non-Completed turn leaves the checkpoint untouched"
    );
    let attempt_two = turn_dir(&live, turn_two.identity.turn.0);
    assert!(
        attempt_two.is_dir(),
        "failed attempt evidence must be preserved"
    );
    // The run-owned active session holds only the copied base context plus this
    // attempt's own writes; it is never adopted as a checkpoint on failure.
    assert!(
        std::fs::read_to_string(
            live.config
                .join("operator-pi/runs")
                .join(turn_two.identity.run.0.to_string())
                .join("session.jsonl")
        )
        .unwrap()
        .contains("first answer"),
        "a resumed attempt starts from the completed checkpoint copy"
    );
    let registry = Registry::of(&live);
    assert!(
        !registry.batch_digests(turn_two.identity.turn.0).is_empty(),
        "the failing turn's receipt is durable"
    );
    assert!(
        !registry
            .batch_digests(turn_two.identity.turn.0)
            .iter()
            .any(|(_, digest)| {
                registry
                    .batch_digests(turn_one_id)
                    .iter()
                    .any(|(_, first)| first == digest)
            }),
        "each turn has its own receipt digest"
    );
    client.wait_stopped(opened.id);

    // An interrupted turn keeps its partial journal and no checkpoint.
    let mut interrupting = Scenario::new();
    interrupting.steps(vec![
        Step::Start,
        Step::Delta("partial interruption".into()),
        Step::Hold,
    ]);
    fake.script(&interrupting);
    let summary = client.current().unwrap();
    let turn_three = client.submit_turn(&summary, "third");
    client.wait_history(opened.id, |history| {
        has_assistant(history, "partial interruption")
    });
    let pgid = operator_harness::own_worker_group(&live, &mut client, opened.session);
    let stopped = client.stop_turn(turn_three.identity);
    assert_eq!(stopped.state, OperatorTurnState::Interrupted);
    let interrupted = client.wait_stopped(opened.id);
    assert_eq!(state(&interrupted), OperatorTurnState::Interrupted);
    assert!(has_assistant(&interrupted, "partial interruption"));
    assert!(operator_harness::interrupted(&interrupted));
    operator_harness::assert_group_absent(&live, pgid);
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turn_one_id as i64),
        "an interrupted turn is not a context checkpoint"
    );
    assert!(
        std::fs::read_to_string(
            live.config
                .join("operator-pi/runs")
                .join(turn_three.identity.run.0.to_string())
                .join("session.jsonl")
        )
        .unwrap()
        .contains("first answer"),
        "the interrupted attempt kept only its checkpoint copy"
    );

    // The follow-up resumes only the last completed checkpoint.
    let mut follow_up = Scenario::new();
    follow_up.default_model("fixture", "alternate");
    // A fresh profile thinking level that differs from the checkpoint's, so the
    // two possible resolutions are distinguishable in the fake's command log.
    follow_up.set("thinking_level", json!("high"));
    follow_up.steps(vec![Step::assistant("follow up answer"), Step::Settle]);
    fake.script(&follow_up);
    fake.clear_log();
    let summary = client.current().unwrap();
    let turn_four = client.submit_turn(&summary, "fourth");
    let history = complete(&mut client, opened.id);
    assert!(has_assistant(&history, "follow up answer"));
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turn_four.identity.turn.0 as i64)
    );
    assert!(
        !checkpoint.exists(),
        "the superseded checkpoint copy is removed only after the new receipt"
    );
    let log = fake.log();
    let switch = log
        .iter()
        .find(|line| line["event"] == "switch")
        .expect("the follow-up resumes a checkpoint");
    assert_eq!(switch["exists"], true);
    let copied = std::fs::read_to_string(switch["path"].as_str().unwrap()).unwrap();
    assert!(
        copied.contains("first answer"),
        "the resumed context is the last completed turn, not the failed/interrupted ones: {copied}"
    );
    assert!(!copied.contains("partial interruption"));
    // The fake restores the resumed session's own model/thinking from the
    // checkpoint, so a worker that reads the post-switch state instead of the
    // fresh profile defaults is detectable rather than accidentally correct.
    assert!(
        copied.contains("\"modelId\":\"default\""),
        "the checkpoint records the previous turn's model: {copied}"
    );
    assert!(
        copied.contains("\"thinkingLevel\":\"medium\""),
        "the checkpoint records the previous turn's thinking level: {copied}"
    );
    let selected: Vec<(String, String)> = log
        .iter()
        .filter(|line| line["event"] == "command" && line["command"] == "set_model")
        .map(|line| {
            (
                line["provider"].as_str().unwrap_or_default().to_owned(),
                line["modelId"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        selected,
        vec![("fixture".to_owned(), "alternate".to_owned())],
        "the follow-up applies the fresh profile default, not the resumed model"
    );
    let levels: Vec<String> = log
        .iter()
        .filter(|line| line["event"] == "command" && line["command"] == "set_thinking_level")
        .filter_map(|line| line["level"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        levels,
        Vec::<String>::new(),
        "default leaves Pi's model-specific thinking choice alone"
    );
    let resumed_session = checkpoint_file(&live, turn_four.identity.turn.0);
    assert!(
        std::fs::read_to_string(&resumed_session)
            .unwrap()
            .contains("first answer"),
        "the promoted native session keeps the completed history"
    );
    assert_eq!(
        history
            .conversation
            .model
            .as_ref()
            .map(|model| model.id.clone()),
        Some("alternate".into()),
        "the fresh profile default applies to the follow-up"
    );
}

#[test]
fn a_natural_worker_exit_is_not_stop_proof_and_blocks_the_next_turn() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::Start,
        Step::Delta("killed evidence".into()),
        Step::Hold,
    ]);
    let (live, fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "crash me");
    client.wait_history(opened.id, |history| {
        has_assistant(history, "killed evidence")
    });
    let pgid = operator_harness::own_worker_group(&live, &mut client, opened.session);

    // The test kills the owned group directly: this is a crash, not a Server stop.
    assert_eq!(unsafe { libc::kill(-pgid, libc::SIGKILL) }, 0);
    assert!(live::wait_group_absent(pgid, Duration::from_secs(10)));
    live.forget_group(pgid);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let session = client.session(opened.session);
        if session.pid.is_none() && !session.phase.is_live() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "killed worker stayed live"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // A Running turn with a dead process is still not a certified stop.
    let running = client.read(opened.id.0, 0, 100);
    assert_eq!(state(&running), OperatorTurnState::Running);
    assert!(!running.conversation.worker_stopped);
    // Stop cannot certify a process the Server no longer owns; it interrupts the
    // turn but leaves `worker_stopped` false.
    assert!(matches!(
        client.stop(turned.identity),
        Response::Error { .. }
    ));
    let history = client.read(opened.id.0, 0, 100);
    assert_eq!(state(&history), OperatorTurnState::Interrupted);
    assert!(
        !history.conversation.worker_stopped,
        "a terminal turn state is never inferred as controlled process-stop proof"
    );
    assert!(matches!(
        client.submit(&history.conversation, "do not replay"),
        Response::Error { .. }
    ));

    // Exact-run acknowledgement is the independent recovery path.
    assert_eq!(
        client.exchange(Request::AcknowledgeSessionStopped {
            session: opened.session,
            expected_run: turned.identity.run,
        }),
        Response::Ok
    );
    let acknowledged = client.wait_stopped(opened.id);
    assert_eq!(state(&acknowledged), OperatorTurnState::Interrupted);
    assert!(has_assistant(&acknowledged, "killed evidence"));

    let mut next = Scenario::new();
    next.steps(vec![Step::assistant("after crash"), Step::Settle]);
    fake.script(&next);
    fake.clear_log();
    let summary = client.current().unwrap();
    let follow_up = client.submit_turn(&summary, "after the crash");
    let history = complete(&mut client, opened.id);
    assert!(has_assistant(&history, "after crash"));
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(follow_up.identity.turn.0 as i64)
    );
    assert!(
        fake.log().iter().all(|line| line["event"] != "switch"),
        "with no completed checkpoint the next attempt starts empty"
    );
    let killed_attempt = turn_dir(&live, turned.identity.turn.0);
    assert!(
        killed_attempt.is_dir(),
        "the killed attempt keeps its evidence"
    );
}

#[test]
fn a_worker_that_exits_after_settling_never_promotes_a_checkpoint() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![
        Step::assistant("streamed then gone"),
        Step::Settle,
        Step::Exit(0),
    ]);
    let (live, _fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "exit early");
    let history = client.wait_terminal(opened.id);
    assert_eq!(state(&history), OperatorTurnState::Error);
    assert!(
        has_assistant(&history, "streamed then gone"),
        "already-committed visible text is preserved"
    );
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        None,
        "an exit before the native file is verified cannot promote a checkpoint"
    );
    assert!(!checkpoint_file(&live, turned.identity.turn.0).exists());
    client.wait_stopped(opened.id);
}

#[test]
fn a_damaged_checkpoint_fails_closed_and_preserves_its_bytes() {
    for damage in ["missing", "corrupt", "body", "nonprivate", "symlink"] {
        let mut first = Scenario::new();
        first.steps(vec![Step::assistant("checkpoint base"), Step::Settle]);
        let (live, fake, mut client) = fixture(&first);
        let opened = client.open();
        let turn_one = client.submit_turn(&opened, "base");
        complete(&mut client, opened.id);
        let checkpoint = checkpoint_file(&live, turn_one.identity.turn.0);
        let original = std::fs::read(&checkpoint).unwrap();
        let preserved = match damage {
            "missing" => {
                std::fs::remove_file(&checkpoint).unwrap();
                Vec::new()
            }
            "corrupt" => {
                std::fs::write(&checkpoint, b"{\"type\":\"session\",\"version\":9}\n").unwrap();
                std::fs::read(&checkpoint).unwrap()
            }
            "body" => {
                // The header stays valid; a later record is malformed. Pi's
                // session manager would skip it and resume a shortened history.
                let mut damaged = original.clone();
                if !damaged.ends_with(b"\n") {
                    damaged.push(b'\n');
                }
                damaged.extend_from_slice(b"not-json\n");
                std::fs::write(&checkpoint, damaged).unwrap();
                std::fs::read(&checkpoint).unwrap()
            }
            "nonprivate" => {
                std::fs::set_permissions(&checkpoint, std::fs::Permissions::from_mode(0o644))
                    .unwrap();
                original.clone()
            }
            "symlink" => {
                let other = live.root.path().join("checkpoint-target");
                std::fs::write(&other, &original).unwrap();
                std::fs::remove_file(&checkpoint).unwrap();
                std::os::unix::fs::symlink(&other, &checkpoint).unwrap();
                original.clone()
            }
            _ => unreachable!(),
        };

        let mut second = Scenario::new();
        second.steps(vec![Step::assistant("must not resume")]);
        fake.script(&second);
        fake.clear_log();
        let summary = client.current().unwrap();
        let turn_two = client.submit_turn(&summary, "resume");
        let history = client.wait_terminal(opened.id);
        assert_eq!(state(&history), OperatorTurnState::Error, "{damage}");
        assert_eq!(
            fail_message(&history).as_deref(),
            Some("Operator Pi context is unavailable; saved data was preserved."),
            "{damage}"
        );
        assert!(!has_assistant(&history, "must not resume"), "{damage}");
        assert_eq!(
            Registry::of(&live).checkpoint(opened.id.0),
            Some(turn_one.identity.turn.0 as i64),
            "{damage}: the checkpoint reference is unchanged"
        );
        let attempt = turn_dir(&live, turn_two.identity.turn.0);
        assert!(!attempt.join("session.jsonl").exists(), "{damage}");
        match damage {
            "missing" => assert!(!checkpoint.exists()),
            "symlink" => assert!(
                std::fs::symlink_metadata(&checkpoint)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            ),
            _ => assert_eq!(std::fs::read(&checkpoint).unwrap(), preserved, "{damage}"),
        }
        assert!(
            fake.log().iter().all(|line| line["event"] != "prompt"),
            "{damage}: no prompt may follow a failed resume"
        );
        client.wait_stopped(opened.id);
    }
}

#[test]
fn an_existing_attempt_directory_is_never_replayed() {
    let mut first = Scenario::new();
    first.steps(vec![Step::assistant("base"), Step::Settle]);
    let (live, fake, mut client) = fixture(&first);
    let opened = client.open();
    let turn_one = client.submit_turn(&opened, "base");
    complete(&mut client, opened.id);

    // SQLite assigns the next turn id sequentially; pre-create its attempt.
    let next_turn = turn_one.identity.turn.0 + 1;
    let attempt = turn_dir(&live, next_turn);
    std::fs::create_dir_all(&attempt).unwrap();
    std::fs::set_permissions(&attempt, std::fs::Permissions::from_mode(0o700)).unwrap();
    let marker = attempt.join("prior-attempt-marker");
    std::fs::write(&marker, b"uncertain prior attempt").unwrap();

    let mut second = Scenario::new();
    second.steps(vec![Step::assistant("must not replay")]);
    fake.script(&second);
    let summary = client.current().unwrap();
    let submitted = client.submit_turn(&summary, "resume into an existing attempt");
    assert_eq!(submitted.identity.turn.0, next_turn);
    let history = client.wait_terminal(opened.id);
    assert_eq!(state(&history), OperatorTurnState::Error);
    assert_eq!(
        fail_message(&history).as_deref(),
        Some("Operator Pi context is unavailable; saved data was preserved.")
    );
    assert_eq!(std::fs::read(&marker).unwrap(), b"uncertain prior attempt");
    assert!(!has_assistant(&history, "must not replay"));
    assert_eq!(
        Registry::of(&live).checkpoint(opened.id.0),
        Some(turn_one.identity.turn.0 as i64)
    );
    client.wait_stopped(opened.id);
}

#[test]
fn stale_run_submits_and_terminal_lifecycle_routes_are_refused() {
    let mut scenario = Scenario::new();
    scenario.steps(vec![Step::assistant("done"), Step::Settle]);
    let (live, _fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let turned = client.submit_turn(&opened, "one");
    complete(&mut client, opened.id);

    // A stale observed run cannot submit again.
    let response = client.exchange(Request::Operator(OperatorRequest::Submit {
        conversation: opened.id,
        expected_run: opened.run,
        text: "stale".into(),
        context: ovrcr::protocol::OperatorContext::default(),
    }));
    assert!(matches!(response, Response::Error { .. }), "{response:?}");

    // Operator history is not a terminal lifecycle.
    for request in [
        Request::CloseTerminal {
            session: opened.session,
            expected_run: turned.identity.run,
        },
        Request::UnarchiveSession {
            session: opened.session,
            expected_run: turned.identity.run,
        },
        Request::DeleteArchivedSession {
            session: opened.session,
            expected_run: turned.identity.run,
        },
        Request::RemoveSession {
            session: opened.session,
        },
    ] {
        // The public terminal lifecycle path is a control request (no Dashboard
        // ownership), so its refusal is synchronous rather than deferred.
        let response = live.request(request.clone());
        assert!(
            matches!(response, Response::Error { .. }),
            "{request:?} must be refused, got {response:?}"
        );
    }
    assert_eq!(
        Registry::of(&live).conversation_ids(),
        vec![opened.id.0],
        "refused lifecycle routes preserve the conversation row"
    );
    assert!(client.session(opened.session).id == opened.session);
}

/// The structured restore payload must not be an alias for another session.
#[test]
fn operator_session_identity_is_the_conversation_session() {
    let scenario = Scenario::new();
    let (live, _fake, mut client) = fixture(&scenario);
    let opened = client.open();
    let session: SessionId = opened.session;
    assert_eq!(session, client.session(session).id);
    let summary = ovrcr::retained::load_session_summaries(&live.config).unwrap();
    let row = summary
        .into_iter()
        .find(|summary| summary.id == session)
        .expect("retained operator session");
    assert!(matches!(
        row.scope,
        ovrcr::protocol::SessionScope::OperatorConversation { conversation } if conversation == opened.id
    ));
    assert!(!row.archived);
}

#[cfg(feature = "acceptance-diagnostics")]
mod installed_pi {
    use super::{
        fake_pi::{Scenario, Step},
        fixture, live, operator_harness, operator_provider,
    };
    use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use operator_provider::{Chunk, Plan, Provider};
    use ovrcr::protocol::{
        self, ClientMessage, OperatorConversationId, OperatorHistory, OperatorMessageContent,
        OperatorToolStatus, OperatorTurnState, OperatorWorkerEvent, Request, Response,
        ServerMessage, SessionPhase, TerminalSize,
    };
    use ovrcr::tui::{Dashboard, Settings};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
    use serde_json::{Value, json};
    use std::{
        collections::{HashMap, VecDeque},
        ffi::{OsStr, OsString},
        os::unix::{
            fs::{MetadataExt, PermissionsExt},
            net::UnixStream,
        },
        path::{Path, PathBuf},
        process::Command,
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    const BUDGET: Duration = Duration::from_secs(30);
    /// The same source file the compiled Operator bundles into its system prompt.
    const BUNDLED_SKILL: &str = include_str!("../.agents/skills/ovrcr/SKILL.md");
    /// A personal skill that `--no-skills` must keep out of every model request,
    /// even though the fixture HOME contains it.
    const PERSONAL_SKILL_SENTINEL: &str = "PERSONAL_SKILL_MUST_NOT_LOAD_6f4a1";
    const COMPACTION_CONTEXT_WINDOW: u64 = 12_288;

    /// Inherited provider/session credentials the launcher must strip before
    /// Pi — and therefore before the admitted Bash child — sees them. Every
    /// value is a unique fixture sentinel, never a real credential.
    const DENIED_ENV: &[(&str, &str)] = &[
        (
            "ANTHROPIC_API_KEY",
            "SENTINEL_ANTHROPIC_9f3a2c_fixture_only",
        ),
        ("OPENAI_API_KEY", "SENTINEL_OPENAI_5c17bd_fixture_only"),
        (
            "AWS_CONTAINER_AUTHORIZATION_TOKEN",
            "SENTINEL_AWS_a41e0_fixture_only",
        ),
        ("PI_SESSION_ID", "SENTINEL_PI_SESSION_7be2_fixture_only"),
        (
            "PI_CODING_AGENT_SESSION_DIR",
            "SENTINEL_PI_DIR_2d8f_fixture_only",
        ),
    ];

    /// An ordinary tool credential, not a provider credential: it must survive
    /// into the admitted Bash child as the positive control for the denylist.
    const ORDINARY_ENV: (&str, &str) = ("GITHUB_TOKEN", "SENTINEL_GITHUB_64c9_fixture_only");

    /// Every fixture sentinel, denied first and the ordinary control last.
    fn fixture_sentinels() -> impl Iterator<Item = (&'static str, &'static str)> {
        DENIED_ENV.iter().copied().chain([ORDINARY_ENV])
    }

    /// The launcher environment for the compiled Server: the installed Pi plus
    /// the inherited credential sentinels the dedicated profile must drop.
    fn launcher_environment(pi: &Path) -> Vec<(&'static str, &OsStr)> {
        let mut environment = vec![("OVRCR_PI_EXECUTABLE", pi.as_os_str())];
        for (key, value) in fixture_sentinels() {
            environment.push((key, OsStr::new(value)));
        }
        environment
    }

    fn pi_executable() -> PathBuf {
        let value = std::env::var_os("OVRCR_TEST_PI_EXECUTABLE")
            .filter(|value| !value.is_empty())
            .expect("set OVRCR_TEST_PI_EXECUTABLE to the installed pi binary");
        let path = PathBuf::from(value);
        assert!(path.is_file(), "installed pi not found: {}", path.display());
        path
    }

    /// The real Dashboard model on a real socket: open, composer edits and
    /// submissions come from its outbox, never from hand-built Submit requests.
    struct DashboardDriver {
        stream: UnixStream,
        dashboard: Dashboard,
        pending: VecDeque<ClientMessage>,
        next_id: u64,
    }

    impl DashboardDriver {
        fn connect(live: &live::Live) -> Self {
            let mut stream = protocol::connect_server(&live.socket).unwrap();
            stream.set_read_timeout(Some(BUDGET)).unwrap();
            stream.set_write_timeout(Some(BUDGET)).unwrap();
            let hierarchy =
                match protocol::client::request(&mut stream, 900_000, Request::DashboardHello)
                    .unwrap()
                {
                    Response::Hierarchy(hierarchy) => hierarchy,
                    other => panic!("Dashboard handshake: {other:?}"),
                };
            let mut dashboard = Dashboard::new(TerminalSize {
                rows: 50,
                cols: 160,
            });
            dashboard.install_hierarchy(hierarchy);
            dashboard.install_area(ratatui::layout::Rect::new(0, 0, 160, 50));
            let mut settings = Settings::default();
            settings.operator.model = None;
            dashboard.install_settings(settings);
            Self {
                stream,
                dashboard,
                pending: VecDeque::new(),
                next_id: 900_001,
            }
        }

        fn exchange(&mut self, request: Request) -> Response {
            self.next_id += 1;
            let id = self.next_id;
            protocol::write_frame(
                &mut self.stream,
                &ClientMessage {
                    request_id: id,
                    request,
                },
            )
            .unwrap();
            loop {
                let message: ServerMessage = protocol::read_frame(&mut self.stream).unwrap();
                let response = match &message {
                    ServerMessage::Response {
                        request_id,
                        response,
                    } if *request_id == id => Some(response.clone()),
                    _ => None,
                };
                self.pending
                    .extend(self.dashboard.handle_server_message(message));
                self.pending.extend(self.dashboard.drain_outbox());
                if let Some(response) = response {
                    return response;
                }
            }
        }

        fn flush(&mut self) {
            for _ in 0..200 {
                self.pending.extend(self.dashboard.drain_outbox());
                if self.pending.is_empty() {
                    return;
                }
                while let Some(message) = self.pending.pop_front() {
                    let id = message.request_id;
                    protocol::write_frame(&mut self.stream, &message).unwrap();
                    loop {
                        let incoming: ServerMessage =
                            protocol::read_frame(&mut self.stream).unwrap();
                        let done = matches!(
                            &incoming,
                            ServerMessage::Response { request_id, .. } if *request_id == id
                        );
                        self.pending
                            .extend(self.dashboard.handle_server_message(incoming));
                        self.pending.extend(self.dashboard.drain_outbox());
                        if done {
                            break;
                        }
                    }
                }
            }
            panic!("Dashboard outbox did not converge");
        }

        fn open(&mut self) {
            self.dashboard.key(KeyCode::Char('o'));
            self.flush();
        }

        /// Drain unsolicited server events so the Dashboard sees completion and
        /// stop proof before the next composer action.
        fn pump(&mut self) {
            self.stream
                .set_read_timeout(Some(Duration::from_millis(2)))
                .unwrap();
            while let Ok(message) = protocol::read_frame::<ServerMessage>(&mut self.stream) {
                self.pending
                    .extend(self.dashboard.handle_server_message(message));
            }
            self.stream.set_read_timeout(Some(BUDGET)).unwrap();
            self.pending.extend(self.dashboard.drain_outbox());
            if !self.pending.is_empty() {
                self.flush();
            }
        }

        fn submit(&mut self, text: &str) {
            self.pump();
            assert!(
                !matches!(
                    self.dashboard.event_action(Event::Paste(text.into())),
                    ovrcr::tui::DashboardAction::PtyBytes(_)
                ),
                "paste must only edit the native draft"
            );
            self.dashboard.key(KeyCode::Enter);
            self.flush();
        }

        /// The first explicit Quick message: capture the target, type into the
        /// overlay, then Send. The first Send opens the singleton conversation.
        fn quick(&mut self, text: &str) {
            self.pump();
            self.dashboard.ctrl('o');
            self.flush();
            self.dashboard.event_action(Event::Paste(text.into()));
            self.dashboard.key(KeyCode::Enter);
            self.flush();
        }

        /// Drive the real sidebar selection from no selection to the fixture's
        /// feature workspace: project -> root workspace -> `skill-cli` workspace.
        fn select_fixture_workspace(&mut self) {
            self.pump();
            for _ in 0..3 {
                self.dashboard.key(KeyCode::Down);
            }
            self.flush();
        }

        /// Leave any Operator/Quick view and open the same conversation again.
        fn reopen(&mut self) {
            self.pump();
            self.dashboard.key(KeyCode::Esc);
            self.flush();
            self.dashboard.key(KeyCode::Esc);
            self.flush();
            self.open();
        }

        /// The actual public Dashboard render, including ephemeral activity.
        fn screen_buffer(&mut self) -> Buffer {
            self.pump();
            let backend = TestBackend::new(160, 50);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| ovrcr::tui::draw_dashboard(frame, &self.dashboard))
                .unwrap();
            terminal.backend().buffer().clone()
        }

        /// The flattened render text existing callers already assert on.
        fn screen(&mut self) -> String {
            self.screen_buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect()
        }

        fn history(&mut self, conversation: OperatorConversationId) -> OperatorHistory {
            match self.exchange(Request::Operator(ovrcr::protocol::OperatorRequest::Read {
                conversation,
                after: 0,
                limit: 100,
            })) {
                Response::Operator(ovrcr::protocol::OperatorResponse::History(history)) => history,
                other => panic!("operator history: {other:?}"),
            }
        }

        fn wait_history(
            &mut self,
            conversation: OperatorConversationId,
            predicate: impl Fn(&OperatorHistory) -> bool,
        ) -> OperatorHistory {
            let deadline = Instant::now() + BUDGET;
            loop {
                let history = self.history(conversation);
                if predicate(&history) {
                    return history;
                }
                assert!(
                    Instant::now() < deadline,
                    "operator history deadline: {history:#?}"
                );
                thread::sleep(Duration::from_millis(20));
            }
        }

        fn terminal_text(&mut self, session: ovrcr::protocol::SessionId) -> String {
            match self.exchange(Request::ReadTerminal {
                session,
                max_lines: Some(400),
            }) {
                Response::TerminalText { text, size, .. } => format!("{size:?}\n{text}"),
                other => format!("{other:?}"),
            }
        }

        fn stop(&mut self, identity: ovrcr::protocol::OperatorTurnIdentity) -> Response {
            self.exchange(Request::Operator(ovrcr::protocol::OperatorRequest::Stop {
                identity,
            }))
        }

        fn conversation(&mut self) -> ovrcr::protocol::OperatorConversationSummary {
            match self.exchange(Request::Operator(ovrcr::protocol::OperatorRequest::Current)) {
                Response::Operator(ovrcr::protocol::OperatorResponse::Current(Some(summary))) => {
                    summary
                }
                other => panic!("operator current: {other:?}"),
            }
        }

        fn session(&mut self, id: ovrcr::protocol::SessionId) -> ovrcr::protocol::SessionSummary {
            self.exchange(Request::Inspect)
                .pipe_inspect()
                .into_iter()
                .find(|session| session.id == id)
                .expect("inspect reports the operator session")
        }
    }

    fn session_header(path: &Path) -> Value {
        let text = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(text.lines().next().unwrap()).unwrap()
    }

    fn buffer_row(buffer: &Buffer, row: u16) -> String {
        (0..buffer.area.width)
            .map(|column| buffer[(column, row)].symbol())
            .collect()
    }

    fn buffer_text(buffer: &Buffer) -> String {
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn find_row(buffer: &Buffer, needle: &str) -> u16 {
        (0..buffer.area.height)
            .find(|row| buffer_row(buffer, *row).contains(needle))
            .unwrap_or_else(|| {
                panic!(
                    "the Dashboard never rendered {needle:?}; rows:\n{}",
                    (0..buffer.area.height)
                        .map(|row| buffer_row(buffer, row))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            })
    }

    fn cell_column(buffer: &Buffer, row: u16, needle: &str) -> u16 {
        let mut text = String::new();
        let mut cells = Vec::new();
        for column in 0..buffer.area.width {
            cells.push((text.len(), column));
            text.push_str(buffer[(column, row)].symbol());
        }
        let byte = text.find(needle).unwrap_or_else(|| {
            panic!("row {row} never rendered {needle:?}: {text}");
        });
        cells
            .iter()
            .rev()
            .find(|(start, _)| *start <= byte)
            .map(|(_, column)| *column)
            .expect("the row has at least one cell")
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn thinking_for_turn(history: &OperatorHistory, turn: u64) -> String {
        history
            .messages
            .iter()
            .filter(|message| message.turn.0 == turn)
            .filter_map(|message| match &message.content {
                OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(text)) => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// The worker suite's held-stream recipe with forty numbered lines split
    /// across multiple thinking deltas, then a held final message.
    fn held_thinking_scenario(thinking: &str, release: &Path, answer: &str) -> Scenario {
        let lines: Vec<&str> = thinking.split('\n').collect();
        let groups = lines.len().div_ceil(10);
        let mut steps = vec![Step::Start, Step::ThinkingStart { index: 0 }];
        for (group, chunk) in lines.chunks(10).enumerate() {
            let mut text = chunk.join("\n");
            if group + 1 < groups {
                text.push('\n');
            }
            steps.push(Step::thinking_delta(0, &text));
        }
        steps.push(Step::WaitFile(release.to_path_buf()));
        steps.push(Step::ThinkingMessageEnd {
            thinking: thinking.to_owned(),
            text: answer.to_owned(),
            stop: "stop".into(),
        });
        steps.push(Step::Settle);
        let mut scenario = Scenario::new();
        scenario.steps(steps);
        scenario
    }

    /// The real Server/worker/Dashboard path with fake Pi: a running request
    /// shows one live thought, completion hides thinking, keyboard and mouse
    /// collapse and reopen the request, and a collapse chosen while a second
    /// held turn runs survives its completion.
    #[test]
    fn request_feed_through_live_server() {
        const FIRST: &str = "first held thinking turn";
        const SECOND: &str = "second held thinking turn";
        let release_dir = tempfile::tempdir().unwrap();
        let first_release = release_dir.path().join("release-first");
        let second_release = release_dir.path().join("release-second");
        let lines = (1..=40)
            .map(|line| format!("THINKING_LINE_{line:02}"))
            .collect::<Vec<_>>();
        let thinking = lines.join("\n");
        let answer = "THINKING_ANSWER_AFTER_RELEASE";
        let reply = format!("▌   {answer}");
        let area = Rect::new(0, 0, 160, 50);

        let (live, fake, client) =
            fixture(&held_thinking_scenario(&thinking, &first_release, answer));
        // The DashboardDriver must own the one dashboard slot; `fixture`'s
        // harness client claims it only long enough to prove the Server speaks.
        drop(client);
        let mut driver = DashboardDriver::connect(&live);
        driver.open();
        let opened = driver.conversation();
        driver.submit(FIRST);

        // The held turn's thinking is a durable history fact, not a delay.
        let running = driver.wait_history(opened.id, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| {
                    turn.state == OperatorTurnState::Running
                        && thinking_for_turn(history, turn.identity.turn.0) == thinking
                })
        });
        let first_turn = running
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        assert_eq!(
            delta_text(&running).matches(answer).count(),
            0,
            "the answer is still held: {running:#?}"
        );

        let preview = driver.screen_buffer();
        let preview_text = buffer_text(&preview);
        assert!(
            find_row(&preview, &format!("▌ ● {FIRST}")) < find_row(&preview, "THINKING_LINE_40"),
            "the live thought sits under the running request: {preview_text}"
        );
        assert_eq!(
            preview_text.matches("THINKING_LINE_").count(),
            1,
            "a running request shows only its latest thought: {preview_text}"
        );

        std::fs::write(&first_release, "go").unwrap();
        let completed = driver.wait_history(opened.id, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| {
                    turn.identity.turn.0 == first_turn && turn.state == OperatorTurnState::Complete
                })
                && history.conversation.worker_stopped
                && delta_text(history).matches(answer).count() == 1
        });
        assert_eq!(
            completed.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Complete
        );
        assert!(
            completed.conversation.worker_stopped,
            "completion waits for controlled-stop proof"
        );

        let finished = driver.screen_buffer();
        let finished_text = buffer_text(&finished);
        assert!(
            !finished_text.contains("THINKING_LINE_"),
            "a finished request hides its thinking: {finished_text}"
        );
        assert!(
            find_row(&finished, &format!("▌ ✓ {FIRST}")) < find_row(&finished, &reply),
            "the reply sits under the finished request: {finished_text}"
        );

        // Keyboard: Tab focuses the transcript, Home selects the request and
        // Enter collapses it to its one row; Enter again reopens it.
        driver.dashboard.key(KeyCode::Tab);
        driver.dashboard.key(KeyCode::Home);
        driver.dashboard.key(KeyCode::Enter);
        let collapsed = driver.screen_buffer();
        let collapsed_text = buffer_text(&collapsed);
        find_row(&collapsed, &format!("▌ ✓ {FIRST}"));
        assert!(!collapsed_text.contains(&reply), "{collapsed_text}");
        driver.dashboard.key(KeyCode::Enter);
        let reopened = driver.screen_buffer();
        find_row(&reopened, &reply);

        // Mouse: the request row toggles through the public mouse path.
        let row = find_row(&reopened, &format!("▌ ✓ {FIRST}"));
        let column = cell_column(&reopened, row, FIRST);
        driver.dashboard.mouse_action(click(column, row), area);
        let mouse_collapsed = driver.screen_buffer();
        assert!(
            !buffer_text(&mouse_collapsed).contains(&reply),
            "a click on the request collapses it: {}",
            buffer_text(&mouse_collapsed)
        );
        // The short feed sits against the composer, so the row has moved.
        let row = find_row(&mouse_collapsed, &format!("▌ ✓ {FIRST}"));
        driver.dashboard.mouse_action(click(column, row), area);
        let mouse_opened = driver.screen_buffer();
        find_row(&mouse_opened, &reply);
        let row = find_row(&mouse_opened, &format!("▌ ✓ {FIRST}"));
        driver.dashboard.mouse_action(click(column, row), area);

        // Second held turn: the first request stays collapsed while it runs.
        driver.dashboard.key(KeyCode::Tab);
        fake.script(&held_thinking_scenario(&thinking, &second_release, answer));
        driver.submit(SECOND);
        let second_running = driver.wait_history(opened.id, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| {
                    turn.identity.turn.0 != first_turn
                        && turn.state == OperatorTurnState::Running
                        && thinking_for_turn(history, turn.identity.turn.0) == thinking
                })
        });
        let second_turn = second_running
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        assert_eq!(
            delta_text(&second_running).matches(answer).count(),
            1,
            "the second answer is still held after the first turn's: {second_running:#?}"
        );
        let second_preview = driver.screen_buffer();
        let second_preview_text = buffer_text(&second_preview);
        find_row(&second_preview, &format!("▌ ● {SECOND}"));
        assert_eq!(
            second_preview_text.matches("THINKING_LINE_").count(),
            1,
            "only the running request shows a thought: {second_preview_text}"
        );
        assert!(
            !second_preview_text.contains(&reply),
            "the first request stays collapsed: {second_preview_text}"
        );

        // An explicit collapse of the running request hides its thought.
        driver.dashboard.key(KeyCode::Tab);
        driver.dashboard.key(KeyCode::End);
        driver.dashboard.key(KeyCode::Enter);
        let second_collapsed = driver.screen_buffer();
        let second_collapsed_text = buffer_text(&second_collapsed);
        let row = find_row(&second_collapsed, &format!("▌ ● {SECOND}"));
        assert!(
            buffer_row(&second_collapsed, row).contains("working"),
            "{second_collapsed_text}"
        );
        assert!(
            !second_collapsed_text.contains("THINKING_LINE_"),
            "{second_collapsed_text}"
        );

        std::fs::write(&second_release, "go").unwrap();
        let second_completed = driver.wait_history(opened.id, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| {
                    turn.identity.turn.0 == second_turn && turn.state == OperatorTurnState::Complete
                })
                && history.conversation.worker_stopped
                && delta_text(history).matches(answer).count() == 2
        });
        assert_eq!(
            second_completed
                .conversation
                .current_turn
                .as_ref()
                .unwrap()
                .state,
            OperatorTurnState::Complete
        );
        let survived = driver.screen_buffer();
        let survived_text = buffer_text(&survived);
        find_row(&survived, &format!("▌ ✓ {SECOND}"));
        assert!(
            !survived_text.contains(&reply),
            "an explicit collapse survives completion: {survived_text}"
        );
        // Enter reopens the still-selected request with its reply.
        driver.dashboard.key(KeyCode::Enter);
        let second_opened = driver.screen_buffer();
        assert!(
            find_row(&second_opened, &format!("▌ ✓ {SECOND}")) < find_row(&second_opened, &reply),
            "{}",
            buffer_text(&second_opened)
        );
    }

    fn delta_text(history: &OperatorHistory) -> String {
        history
            .messages
            .iter()
            .filter_map(|message| match &message.content {
                ovrcr::protocol::OperatorMessageContent::Worker(
                    ovrcr::protocol::OperatorWorkerEvent::AssistantDelta(text),
                ) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Synthetic request shape: role and content length, never a provider body.
    fn request_shape(body: &Value) -> Vec<String> {
        body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|message| {
                format!(
                    "{}:{}",
                    message["role"].as_str().unwrap_or("?"),
                    operator_provider::content_text(&message["content"]).len()
                )
            })
            .collect()
    }

    /// The CLI guide stays in instruction text; Pi advertises the private
    /// coordinator file by metadata. Its body is read on demand. Pi's OpenAI
    /// adapter may use `developer` instead of `system`.
    fn assert_skill_only_in_system(label: &str, body: &Value, operator_profile: &Path) {
        let system = operator_provider::system_text(body);
        for instruction in [
            "Use the read tool to load a skill's file when the task matches its description.",
            "Use the bundled coordinator skill for multi-session work.",
            "watch_sessions only to add, list, or remove bounded watches",
            "opaque unread.turn (string or null)",
            "Claude Code and Codex via terminal transcript --json",
            "Leave finished Retained sessions open unless asked to close or archive them.",
        ] {
            assert!(
                system.contains(instruction),
                "{label}: Operator policy omitted {instruction:?}"
            );
        }
        assert!(
            system.contains("<name>ovrcr-coordinator</name>"),
            "{label}: Pi did not advertise the explicitly loaded coordinator skill"
        );
        assert!(
            system.contains(
                "<description>Coordinate parallel coding or research through visible OVRCR Workspace Agent sessions, register watches, and verify asynchronous results.</description>"
            ),
            "{label}: Pi omitted the coordinator skill description"
        );
        let coordinator_skill = operator_profile.join("ovrcr-coordinator/SKILL.md");
        assert!(
            system.contains(&format!(
                "<location>{}</location>",
                coordinator_skill.display()
            )),
            "{label}: Pi did not advertise the exact private coordinator skill path {}",
            coordinator_skill.display()
        );
        assert_eq!(
            system.matches(BUNDLED_SKILL).count(),
            1,
            "{label}: the bundled CLI skill must appear exactly once in the system prompt; shape={:?}",
            request_shape(body)
        );
        let conversation = operator_provider::non_system_text(body);
        assert!(
            !system.contains(PERSONAL_SKILL_SENTINEL),
            "{label}: a personal skill was discovered despite --no-skills"
        );
        assert!(
            !system.contains("<name>personal-operator-sentinel</name>")
                && !system.contains("<description>fixture personal skill</description>"),
            "{label}: personal skill metadata was discovered despite --no-skills"
        );
        assert!(
            !conversation.contains(BUNDLED_SKILL),
            "{label}: the bundled CLI skill leaked into the conversation payload"
        );
        assert!(
            !conversation.contains(PERSONAL_SKILL_SENTINEL),
            "{label}: a personal skill was discovered despite --no-skills"
        );
    }

    /// The first model request carries the normal Operator context and user
    /// input, not skill text or a personal skill.
    fn assert_normal_first_prompt(label: &str, body: &Value, marker: &str) {
        let messages = body["messages"].as_array().cloned().unwrap_or_default();
        for message in &messages {
            let role = message["role"].as_str().unwrap_or_default();
            assert!(
                role == "user" || operator_provider::is_instruction_role(role),
                "{label}: first request has an unexpected {role} message; shape={:?}",
                request_shape(body)
            );
        }
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1,
            "{label}: the first request has exactly one user message"
        );
        let user = operator_provider::last_user_text(body);
        assert!(
            user.contains(marker),
            "{label}: user input is missing: {user}"
        );
        assert!(
            user.starts_with("Captured OVRCR context (identities are fixed for this turn):\n"),
            "{label}: the captured context precedes the user input: {user}"
        );
        assert!(
            user.contains("\n\nUser request:\n"),
            "{label}: the user request is delimited: {user}"
        );
        assert!(
            !user.contains(BUNDLED_SKILL) && !user.contains(PERSONAL_SKILL_SENTINEL),
            "{label}: guidance was injected into the user message"
        );
    }

    fn install_personal_skill(home: &Path) {
        let directory = home.join(".agents/skills/personal-operator-sentinel");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("SKILL.md"),
            format!(
                "---\nname: personal-operator-sentinel\ndescription: fixture personal skill\n---\n\n# Personal sentinel\n\n{PERSONAL_SKILL_SENTINEL}\n"
            ),
        )
        .unwrap();
    }

    fn assert_no_guidance(label: &str, text: &str) {
        assert!(
            !text.contains(BUNDLED_SKILL),
            "{label}: bundled CLI skill appeared outside the system prompt"
        );
        assert!(
            !text.contains(PERSONAL_SKILL_SENTINEL),
            "{label}: a personal skill was discovered"
        );
    }

    /// Hold pi's compaction request and wait for the public Dashboard render to
    /// show the live status before the test releases the provider.
    fn wait_compaction_status(
        driver: &mut DashboardDriver,
        provider: &Provider,
        conversation: OperatorConversationId,
    ) -> usize {
        let deadline = Instant::now() + BUDGET;
        let compaction_index = loop {
            driver.pump();
            if let Some(index) = provider
                .requests()
                .iter()
                .position(|request| operator_provider::is_compaction_request(&request.body))
            {
                break index;
            }
            let history = driver.history(conversation);
            assert!(
                history
                    .conversation
                    .current_turn
                    .as_ref()
                    .is_none_or(|turn| turn.state == OperatorTurnState::Running),
                "the turn finished before pi made its compaction request: {history:#?}"
            );
            assert!(
                Instant::now() < deadline,
                "pi never made a compaction request: {:#?}",
                provider.requests()
            );
            thread::sleep(Duration::from_millis(10));
        };
        let deadline = Instant::now() + BUDGET;
        loop {
            let screen = driver.screen();
            if screen.contains("Compacting context") {
                return compaction_index;
            }
            assert!(
                Instant::now() < deadline,
                "the Dashboard never rendered the Compacting status: {screen}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn tool_names(history: &OperatorHistory) -> Vec<String> {
        history
            .messages
            .iter()
            .filter_map(|message| match &message.content {
                ovrcr::protocol::OperatorMessageContent::Worker(
                    ovrcr::protocol::OperatorWorkerEvent::ToolCall { name, .. },
                ) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn tool_names_for(history: &OperatorHistory, turn: u64) -> Vec<String> {
        history
            .messages
            .iter()
            .filter(|message| message.turn.0 == turn)
            .filter_map(|message| match &message.content {
                ovrcr::protocol::OperatorMessageContent::Worker(
                    ovrcr::protocol::OperatorWorkerEvent::ToolCall { name, .. },
                ) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn tool_outputs_for(history: &OperatorHistory, turn: u64) -> Vec<(String, String)> {
        history
            .messages
            .iter()
            .filter(|message| message.turn.0 == turn)
            .filter_map(|message| match &message.content {
                ovrcr::protocol::OperatorMessageContent::Worker(
                    ovrcr::protocol::OperatorWorkerEvent::ToolOutput { id, output, .. },
                ) => Some((id.clone(), output.clone())),
                _ => None,
            })
            .collect()
    }

    /// Durable tool outputs with their recorded status.
    fn tool_outputs_with_status(
        history: &OperatorHistory,
    ) -> Vec<(String, String, OperatorToolStatus)> {
        history
            .messages
            .iter()
            .filter_map(|message| match &message.content {
                OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                    id,
                    output,
                    status,
                    ..
                }) => Some((id.clone(), output.clone(), status.clone())),
                _ => None,
            })
            .collect()
    }

    fn has_completed(history: &OperatorHistory) -> bool {
        history.messages.iter().any(|message| {
            matches!(
                message.content,
                OperatorMessageContent::Worker(OperatorWorkerEvent::Completed { .. })
            )
        })
    }

    /// The provider body as structured messages: text content, tool-call names
    /// and arguments, and tool results. A content-only extraction would hide a
    /// leaked `tool_calls[].function.arguments` from the context assertions.
    fn structured_request_text(body: &Value) -> String {
        let mut parts = Vec::new();
        for message in body["messages"].as_array().into_iter().flatten() {
            parts.push(operator_provider::content_text(&message["content"]));
            for call in message["tool_calls"].as_array().into_iter().flatten() {
                if let Some(name) = call["function"]["name"].as_str() {
                    parts.push(name.to_owned());
                }
                if let Some(arguments) = call["function"]["arguments"].as_str() {
                    parts.push(arguments.to_owned());
                }
            }
        }
        parts.join("\n")
    }

    /// Only the arguments strings of assistant tool calls, the part of the
    /// structured request a content-only extraction cannot see.
    fn tool_call_arguments_text(body: &Value) -> String {
        body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
            .filter_map(|call| call["function"]["arguments"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn assert_no_fixture_sentinel(label: &str, text: &str) {
        for (key, sentinel) in fixture_sentinels() {
            assert!(
                !text.contains(sentinel),
                "{label} contains the {key} fixture sentinel"
            );
        }
    }

    /// Every regular file under `root` (the private Pi profile and its attempt
    /// sessions) is inspected for a fixture sentinel.
    fn scan_profile_for_sentinels(root: &Path, label: &str) {
        let mut stack = vec![root.to_path_buf()];
        let mut inspected = 0usize;
        while let Some(directory) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.file_type().is_symlink() {
                    continue;
                }
                if metadata.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
                    continue;
                }
                inspected += 1;
                let bytes = std::fs::read(&path).unwrap();
                assert_no_fixture_sentinel(
                    &format!("{label} {}", path.display()),
                    &String::from_utf8_lossy(&bytes),
                );
            }
        }
        assert!(inspected > 0, "{label} had no files to inspect");
    }

    fn attempt_evidence(live: &live::Live, turn: u64) -> String {
        let root = live.config.join("operator-pi");
        let attempt = root.join("turns").join(turn.to_string());
        let mut evidence = format!("attempt={}\n", attempt.display());
        if let Ok(entries) = std::fs::read_dir(&attempt) {
            for entry in entries.flatten() {
                let path = entry.path();
                let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                evidence.push_str(&format!("  {} ({size} bytes)\n", path.display()));
                if let Ok(text) = std::fs::read_to_string(&path)
                    && text.len() < 200_000
                {
                    evidence.push_str(&text.replace('\n', "\n  "));
                    evidence.push('\n');
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                evidence.push_str(&format!("profile: {}\n", entry.path().display()));
            }
        }
        for name in [
            "models.json",
            "settings.json",
            "models-store.json",
            "auth.json",
        ] {
            if let Ok(text) = std::fs::read_to_string(root.join(name)) {
                evidence.push_str(&format!("{name}: {text}\n"));
            }
        }
        evidence
    }

    fn wait_terminal(
        driver: &mut DashboardDriver,
        conversation: OperatorConversationId,
        provider: &Provider,
        live: &live::Live,
    ) -> OperatorHistory {
        let history = driver.wait_history(conversation, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.state != OperatorTurnState::Running)
        });
        assert_eq!(
            history.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Complete,
            "{history:#?}\nterminal: {}\nevidence:\n{}\nprovider diagnostics: {:?}\nprovider requests: {:#?}",
            driver.terminal_text(history.conversation.session),
            attempt_evidence(
                live,
                history
                    .conversation
                    .current_turn
                    .as_ref()
                    .unwrap()
                    .identity
                    .turn
                    .0
            ),
            provider.diagnostics(),
            provider.requests()
        );
        history
    }

    fn wait_stopped(driver: &mut DashboardDriver, conversation: OperatorConversationId) {
        driver.wait_history(conversation, |history| history.conversation.worker_stopped);
    }

    /// Submit through the Dashboard composer and prove a *new* turn was accepted.
    fn submit_next(
        driver: &mut DashboardDriver,
        conversation: OperatorConversationId,
        text: &str,
        previous_turn: u64,
        provider: &Provider,
        live: &live::Live,
    ) -> OperatorHistory {
        driver.submit(text);
        driver.wait_history(conversation, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.identity.turn.0 != previous_turn)
        });
        wait_terminal(driver, conversation, provider, live)
    }

    fn process_alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// Optional fixture behavior on top of [`scenario_handler`].
    #[derive(Clone, Copy, Default)]
    struct ScenarioMode {
        /// Hold pi's own compaction summary request until the test releases it,
        /// so the Dashboard's live Compacting status can be observed.
        hold_compaction: bool,
        /// Make the first COMPACT turn's answer large enough that the next
        /// prompt crosses the seeded threshold in a manual GUI run.
        inflate_compaction_turn: bool,
        /// Script the private local-provider Operator coordinator walkthrough.
        coordinator: bool,
    }

    /// Scripted operator/tool scenario shared by the installed-pi test and the
    /// GUI provider. Every turn is identified by its user request marker.
    pub(super) fn scenario_handler(
        workspace: PathBuf,
        marker: PathBuf,
        background_pid: PathBuf,
    ) -> operator_provider::Handler {
        scenario_handler_mode(workspace, marker, background_pid, ScenarioMode::default())
    }

    fn scenario_handler_mode(
        workspace: PathBuf,
        marker: PathBuf,
        background_pid: PathBuf,
        mode: ScenarioMode,
    ) -> operator_provider::Handler {
        let counters: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
        Box::new(move |body: &Value, _index: usize| {
            if operator_provider::is_compaction_request(body) {
                let plan = Plan::text("FIXTURE_COMPACTION_SUMMARY");
                return if mode.hold_compaction {
                    plan.holding()
                } else {
                    plan
                };
            }
            let user = operator_provider::last_user_text(body);
            let key = [
                "REQUEST_ONE_REAL_TOOLS",
                "REQUEST_TWO_RESUME",
                "REQUEST_THREE_HELD",
                "REQUEST_FOUR_BACKGROUND",
                "REQUEST_FIVE_AFTER_INTERRUPT",
                "REQUEST_SIX_TWO_TOOLS",
                "REQUEST_SEVEN_TIMEOUT",
                "REQUEST_MARKDOWN",
                "REQUEST_SKILL_SYSTEM",
                "REQUEST_SKILL_QUICK",
                "REQUEST_SKILL_CLI",
                "REQUEST_SKILL_RESUME",
                "REQUEST_COORDINATOR_DISPATCH",
                "REQUEST_COORDINATOR_WARM_CHAT",
                "OVRCR_GUI_COORDINATOR_ALPHA",
                "OVRCR_GUI_COORDINATOR_BETA",
                "These watched OVRCR Workspace Agent sessions settled:",
                "REQUEST_THINKING_HELD",
                "REQUEST_THINKING_LONG_HELD",
                "COMPACT_ONE",
                "COMPACT_TWO",
            ]
            .into_iter()
            .find(|marker| user.contains(marker))
            .unwrap_or("OTHER")
            .to_owned();
            let step = {
                let mut counters = counters.lock().unwrap();
                let step = *counters.get(&key).unwrap_or(&0);
                counters.insert(key.clone(), step + 1);
                step
            };
            let marker_text = marker.display().to_string();
            let workspace_text = workspace.display().to_string();
            match (key.as_str(), step) {
                ("REQUEST_ONE_REAL_TOOLS", 0) => Plan::tool(
                    "call-write",
                    "write",
                    json!({"path": marker_text, "content": "OFFLINE_MARKER_v1\n"}),
                ),
                ("REQUEST_ONE_REAL_TOOLS", 1) => {
                    Plan::tool("call-read", "read", json!({"path": marker_text}))
                }
                ("REQUEST_ONE_REAL_TOOLS", 2) => Plan::tool(
                    "call-edit",
                    "edit",
                    json!({
                        "path": marker_text,
                        "edits": [{"oldText": "OFFLINE_MARKER_v1", "newText": "OFFLINE_MARKER_v2"}]
                    }),
                ),
                // The admitted Bash child reports which inherited credentials it
                // sees. `key=SET` stays durable in the journal without copying a
                // value; the exact values go to a file outside the instance.
                ("REQUEST_ONE_REAL_TOOLS", 3) => Plan::tool(
                    "call-bash",
                    "bash",
                    json!({
                        "command": format!(
                            "cd '{workspace_text}' && printf 'CREDENTIAL_PROBE anthropic=%s openai=%s aws=%s pi_session=%s pi_session_dir=%s github=%s\\n' \
                             \"${{ANTHROPIC_API_KEY:+SET}}\" \"${{OPENAI_API_KEY:+SET}}\" \
                             \"${{AWS_CONTAINER_AUTHORIZATION_TOKEN:+SET}}\" \"${{PI_SESSION_ID:+SET}}\" \
                             \"${{PI_CODING_AGENT_SESSION_DIR:+SET}}\" \"${{GITHUB_TOKEN:+SET}}\" && \
                             printf 'ANTHROPIC_API_KEY=%s\\nOPENAI_API_KEY=%s\\nAWS_CONTAINER_AUTHORIZATION_TOKEN=%s\\nPI_SESSION_ID=%s\\nPI_CODING_AGENT_SESSION_DIR=%s\\nGITHUB_TOKEN=%s\\n' \
                             \"$ANTHROPIC_API_KEY\" \"$OPENAI_API_KEY\" \"$AWS_CONTAINER_AUTHORIZATION_TOKEN\" \
                             \"$PI_SESSION_ID\" \"$PI_CODING_AGENT_SESSION_DIR\" \"$GITHUB_TOKEN\" > credential-probe.env && \
                             printf 'BASH_' && \"$OVRCR_EXECUTABLE\" --json project list"
                        )
                    }),
                ),
                ("REQUEST_ONE_REAL_TOOLS", 4) => Plan::text("TURN_ONE_DONE"),
                ("REQUEST_TWO_RESUME", 0) => Plan::text("TURN_TWO_DONE"),
                ("REQUEST_THREE_HELD", 0) => Plan::text("HELD_BEFORE_RELEASE").holding(),
                ("REQUEST_COORDINATOR_DISPATCH", 0) if mode.coordinator => Plan::tool(
                    "coordinator-skill-read",
                    "read",
                    json!({
                        "path": workspace
                            .parent()
                            .expect("GUI workspace has an instance root")
                            .join("operator-pi/ovrcr-coordinator/SKILL.md")
                    }),
                ),
                ("REQUEST_COORDINATOR_DISPATCH", 1) if mode.coordinator => {
                    let readback = operator_provider::tool_result_texts(body).join("\n");
                    assert!(
                        readback.contains(
                            "Use this workflow for work that needs multiple Workspace Agent sessions."
                        ),
                        "Pi's read tool returned the bundled coordinator skill"
                    );
                    Plan::tool(
                        "coordinator-dispatch",
                        "bash",
                        json!({"command": coordinator_dispatch_script()}),
                    )
                }
                ("REQUEST_COORDINATOR_DISPATCH", 2) if mode.coordinator => Plan::tool(
                    "coordinator-watch",
                    "watch_sessions",
                    coordinator_watch_input(body),
                ),
                ("REQUEST_COORDINATOR_DISPATCH", 3) if mode.coordinator => {
                    Plan::text(&coordinator_dispatch_result(body))
                }
                ("REQUEST_COORDINATOR_WARM_CHAT", 0) if mode.coordinator => {
                    assert!(
                        !operator_provider::non_system_text(body).contains("REQUEST_THREE_HELD")
                            && !operator_provider::non_system_text(body)
                                .contains("HELD_BEFORE_RELEASE"),
                        "the interrupted Operator turn was rolled back before warm chat"
                    );
                    Plan::text("COORDINATOR_WARM_CHAT_DONE while both Workspace replies remain held")
                }
                ("OVRCR_GUI_COORDINATOR_ALPHA", 0) if mode.coordinator => Plan::tool(
                    "coordinator-alpha-edit",
                    "edit",
                    json!({
                        "path": coordinator_workspace(&workspace, GUI_COORD_ALPHA_WORKSPACE).join("README"),
                        "edits": [{
                            "oldText": "Disposable OVRCR GUI fixture for consigint\n",
                            "newText": "Disposable OVRCR GUI fixture for consigint\n\nCoordinator alpha native evidence.\n"
                        }]
                    }),
                ),
                ("OVRCR_GUI_COORDINATOR_ALPHA", 1) if mode.coordinator => Plan::tool(
                    "coordinator-alpha-check",
                    "bash",
                    json!({"command": "git diff --check"}),
                ),
                ("OVRCR_GUI_COORDINATOR_ALPHA", 2) if mode.coordinator => Plan::new(vec![
                    Chunk::Text("OVRCR_ALPHA_PARTIAL".into()),
                ])
                .holding()
                .then_text_after_release(
                    "\nOVRCR_WORKER_DONE\nsummary: appended the alpha fixture note\nfiles: README\nchecks: git diff --check exit 0",
                ),
                ("OVRCR_GUI_COORDINATOR_BETA", 0) if mode.coordinator => Plan::tool(
                    "coordinator-beta-edit",
                    "edit",
                    json!({
                        "path": coordinator_workspace(&workspace, GUI_COORD_BETA_WORKSPACE).join("README"),
                        "edits": [{
                            "oldText": "Disposable OVRCR GUI fixture for consigint\n",
                            "newText": "Disposable OVRCR GUI fixture for consigint\n\nCoordinator beta native evidence.\n"
                        }]
                    }),
                ),
                ("OVRCR_GUI_COORDINATOR_BETA", 1) if mode.coordinator => Plan::tool(
                    "coordinator-beta-check",
                    "bash",
                    json!({"command": "git diff --check"}),
                ),
                ("OVRCR_GUI_COORDINATOR_BETA", 2) if mode.coordinator => Plan::new(vec![
                    Chunk::Text("OVRCR_BETA_PARTIAL".into()),
                ])
                .holding()
                .then_text_after_release(
                    "\nOVRCR_WORKER_DONE\nsummary: appended the beta fixture note\nfiles: README\nchecks: git diff --check exit 0",
                ),
                ("These watched OVRCR Workspace Agent sessions settled:", 0)
                    if mode.coordinator => Plan::tool(
                        "coordinator-read-verify",
                        "bash",
                        json!({"command": coordinator_wakeup_script(&workspace, &user)}),
                    ),
                ("These watched OVRCR Workspace Agent sessions settled:", 1)
                    if mode.coordinator => {
                        let verification = operator_provider::tool_result_texts(body).join("\n");
                        for evidence in [
                            "GIT_STATUS_ALPHA",
                            "DIFF_STAT_ALPHA",
                            "CHECK_ALPHA exit=0",
                            "GIT_STATUS_BETA",
                            "DIFF_STAT_BETA",
                            "CHECK_BETA exit=0",
                        ] {
                            assert!(
                                verification.contains(evidence),
                                "Workspace verification returned {evidence:?}"
                            );
                        }
                        Plan::text(&coordinator_wakeup_result(&user))
                    }
                ("REQUEST_FOUR_BACKGROUND", 0) => Plan::tool(
                    "call-background",
                    "bash",
                    json!({
                        "command": format!(
                            "cd '{workspace_text}' && sleep 30 >/dev/null 2>&1 & echo $! > '{}'; wait",
                            background_pid.display()
                        )
                    }),
                ),
                ("REQUEST_FIVE_AFTER_INTERRUPT", 0) => Plan::text("TURN_FIVE_DONE"),
                // One assistant message carrying two tool calls. Pi must prepare
                // and execute them one at a time so both durable admissions hold.
                ("REQUEST_SIX_TWO_TOOLS", 0) => Plan::new(vec![
                    Chunk::ToolCall {
                        id: "call-two-read".into(),
                        name: "read".into(),
                        arguments: json!({ "path": marker_text }),
                    },
                    Chunk::ToolCall {
                        id: "call-two-bash".into(),
                        name: "bash".into(),
                        arguments: json!({
                            "command": format!(
                                "cd '{workspace_text}' && printf TWO_TOOL_BASH"
                            )
                        }),
                    },
                ]),
                ("REQUEST_SIX_TWO_TOOLS", 1) => Plan::text("TWO_TOOL_TURN_DONE"),
                // A real attached Bash call with a short native timeout. The
                // background descendant keeps writing, so the shell never
                // finishes on its own; the tool must time out at the deadline.
                ("REQUEST_SEVEN_TIMEOUT", 0) => Plan::tool(
                    "call-timeout",
                    "bash",
                    json!({
                        "command": format!(
                            "cd '{workspace_text}' && {{ {{ while :; do printf x >> timeout-writer.out; sleep 0.02; done; }} & \
                             echo $! > timeout-writer.pid; wait; }}"
                        ),
                        "timeout": 2
                    }),
                ),
                // A second model request for this turn is the exact failure the
                // test guards against; its text is unique and asserted absent.
                ("REQUEST_SEVEN_TIMEOUT", 1) => Plan::text("TIMEOUT_CONTINUATION_MUST_NOT_APPEAR"),
                // Native Markdown presentation: headings, emphasis, inline code,
                // a table, a list and a fenced block all lose their source markers.
                ("REQUEST_MARKDOWN", 0) => Plan::text(
                    "## Operator report\n\n**Ready** with `cargo test`. *All checks pass.*\n\n| Area | Result |\n|---|---|\n| tests | **pass** |\n| docs | updated |\n\n- first item\n- second item\n\n```text\ncargo test -p ovrcr-tui\n```\n",
                ),
                ("REQUEST_SKILL_SYSTEM", 0) => Plan::text("SKILL_SYSTEM_ANSWER"),
                ("REQUEST_SKILL_QUICK", 0) => Plan::text("SKILL_QUICK_ANSWER"),
                ("REQUEST_SKILL_CLI", 0) => Plan::tool(
                    "call-skill-cli",
                    "bash",
                    json!({
                        "command": format!(
                            "cd '{}' && printf 'CLI_PWD=%s\\n' \"$PWD\" && \
                             printf 'CLI_ENV home=%s config=%s socket=%s exe=%s profile=%s home_dir=%s\\n' \
                             \"${{OVRCR_HOME-}}\" \"${{OVRCR_CONFIG-}}\" \"${{OVRCR_SOCKET-}}\" \
                             \"${{OVRCR_EXECUTABLE-}}\" \"${{PI_CODING_AGENT_DIR-}}\" \"$HOME\" && \
                             printf 'CLI_VERSION ' && \"${{OVRCR_EXECUTABLE}}\" --version && \
                             printf 'CLI_PROJECT ' && \"${{OVRCR_EXECUTABLE}}\" --json project get fixture && \
                             printf 'CLI_WORKSPACES ' && \"${{OVRCR_EXECUTABLE}}\" --json workspace list --project fixture",
                            workspace
                                .parent()
                                .map(|parent| parent.join("operator-relocated"))
                                .unwrap_or_else(|| workspace.join("operator-relocated"))
                                .display()
                        )
                    }),
                ),
                ("REQUEST_SKILL_CLI", 1) => Plan::text("SKILL_CLI_ANSWER"),
                ("REQUEST_SKILL_RESUME", 0) => Plan::text("SKILL_RESUME_ANSWER"),
                // Two displayable thinking deltas arrive before the hold; the
                // answer only exists in the stream after release.
                ("REQUEST_THINKING_HELD", 0) => Plan::new(vec![
                    Chunk::Thinking("THINKING_DELTA_ONE".into()),
                    Chunk::Thinking("THINKING_DELTA_TWO".into()),
                ])
                .holding()
                .then_text_after_release("THINKING_ANSWER_AFTER_RELEASE"),
                // Forty lines arrive as forty thinking chunks between single
                // newlines with no final terminator, then the answer is held.
                ("REQUEST_THINKING_LONG_HELD", 0) => Plan::new(
                    (1..=40)
                        .map(|line| {
                            Chunk::Thinking(format!(
                                "THINKING_LINE_{line:02}{}",
                                if line < 40 { "\n" } else { "" }
                            ))
                        })
                        .collect(),
                )
                .holding()
                .then_text_after_release("THINKING_LONG_ANSWER"),
                ("COMPACT_ONE", 0) if mode.inflate_compaction_turn => {
                    Plan::text(&format!("COMPACT_ONE_DONE {}", "C".repeat(60_000)))
                }
                ("COMPACT_ONE", 0) => Plan::text("COMPACT_ONE_DONE"),
                ("COMPACT_TWO", 0) => Plan::text("COMPACT_TWO_DONE"),
                _ => Plan::text("FIXTURE_FOLLOWUP_DONE"),
            }
        })
    }

    const GUI_COORD_ALPHA_WORKSPACE: &str = "gui-consigint-worktree-lifecycle";
    const GUI_COORD_BETA_WORKSPACE: &str = "gui-consigint-auth-handoff";
    const GUI_COORD_ALPHA_SESSION: &str = "coordinator-alpha";
    const GUI_COORD_BETA_SESSION: &str = "coordinator-beta";

    fn coordinator_workspace(operator_workspace: &Path, branch: &str) -> PathBuf {
        operator_workspace
            .parent()
            .expect("GUI operator workspace has a fixture root")
            .join("workspaces/consigint")
            .join(branch)
    }

    fn coordinator_dispatch_script() -> &'static str {
        r#"python3 - <<'PY'
import json
import os
import subprocess

cli = os.environ["OVRCR_EXECUTABLE"]
rows = json.loads(subprocess.check_output([cli, "terminal", "list", "--json"], text=True))
wanted = [
    ("coordinator-alpha", "gui-consigint-worktree-lifecycle", "OVRCR_GUI_COORDINATOR_ALPHA"),
    ("coordinator-beta", "gui-consigint-auth-handoff", "OVRCR_GUI_COORDINATOR_BETA"),
]
sessions = []
for name, branch, marker in wanted:
    matches = [row for row in rows if row.get("name") == name]
    if len(matches) != 1:
        raise SystemExit(f"expected exactly one {name} session, found {len(matches)}")
    row = matches[0]
    agent = row.get("agent") or {}
    kind = row.get("kind")
    if row.get("scope") != "Workspace" or not isinstance(kind, dict) or kind.get("agent") != "pi" or row.get("phase") != "running":
        raise SystemExit(f"{name} is not a running Workspace Pi Agent")
    if row.get("workspace") != branch:
        raise SystemExit(f"{name} is in unexpected Workspace {row.get('workspace')!r}")
    revision = agent.get("activity_revision")
    if not isinstance(revision, int) or isinstance(revision, bool):
        raise SystemExit(f"{name} has no numeric Reporter activity_revision")
    run = row.get("run")
    if not isinstance(run, int) or isinstance(run, bool):
        raise SystemExit(f"{name} has no numeric run identity")
    unread = row.get("unread")
    unread_turn = unread.get("turn") if isinstance(unread, dict) else None
    if unread_turn is not None and not isinstance(unread_turn, str):
        raise SystemExit(f"{name} unread.turn is not an opaque string or null")
    sessions.append((row, {
        "run": run,
        "activity_revision": revision,
        "unread_turn": unread_turn,
    }, marker))

for row, baseline, marker in sessions:
    task = (
        f"{marker}\nAppend exactly one sentence to the existing README in this Workspace: "
        f"Coordinator {'alpha' if marker.endswith('ALPHA') else 'beta'} native evidence. "
        "Change no other files. Run git diff --check and report its actual exit code. "
        "Finish with OVRCR_WORKER_DONE (summary, files, checks) or "
        "OVRCR_WORKER_BLOCKED (reason, need)."
    )
    subprocess.run([cli, "terminal", "send", str(row["id"]), "--text", task], check=True)

watch = {
    "action": "add",
    "sessions": [
        {
            "session_id": row["id"],
            "project": row["project"],
            "label": row["name"],
            "baseline": baseline,
        }
        for row, baseline, _ in sessions
    ],
}
table = [
    {"task": marker.rsplit("_", 1)[-1].title(), "workspace": row["workspace"], "session_id": row["id"], "name": row["name"]}
    for row, _, marker in sessions
]
print("OVRCR_WATCH_INPUT=" + json.dumps(watch, separators=(",", ":")))
print("OVRCR_SESSION_TABLE=" + json.dumps(table, separators=(",", ":")))
PY"#
    }

    fn marked_tool_json(body: &Value, prefix: &str) -> Value {
        operator_provider::tool_result_texts(body)
            .iter()
            .rev()
            .flat_map(|text| text.lines())
            .find_map(|line| {
                line.strip_prefix(prefix)
                    .and_then(|value| serde_json::from_str(value).ok())
            })
            .unwrap_or_else(|| panic!("fixture tool result did not include {prefix}"))
    }

    fn coordinator_watch_input(body: &Value) -> Value {
        marked_tool_json(body, "OVRCR_WATCH_INPUT=")
    }

    fn coordinator_table(body: &Value) -> Vec<Value> {
        marked_tool_json(body, "OVRCR_SESSION_TABLE=")
            .as_array()
            .cloned()
            .expect("fixture dispatch table is an array")
    }

    fn coordinator_dispatch_result(body: &Value) -> String {
        let rows = coordinator_table(body);
        assert_eq!(rows.len(), 2, "both coding tasks are dispatched");
        let mut result = String::from(
            "Dispatched both tasks and registered their session watches.\n\n| Task | Workspace | Session |\n|---|---|---|\n",
        );
        for row in rows {
            result.push_str(&format!(
                "| {} | {} | {} ({}) |\n",
                row["task"].as_str().unwrap_or("task"),
                row["workspace"].as_str().unwrap_or("unknown"),
                row["name"].as_str().unwrap_or("session"),
                row["session_id"].as_u64().unwrap_or_default(),
            ));
        }
        result.push_str("Operator is ready for another message while both Agents work.");
        result
    }

    fn coordinator_wakeup_rows(user: &str) -> Vec<(String, u64)> {
        user.lines()
            .filter_map(|line| {
                let tail = line.strip_prefix("- session ")?;
                let (id, label) = tail.split_once(' ')?;
                let label = label.strip_prefix('(')?.split_once(')')?.0;
                Some((label.to_owned(), id.parse().ok()?))
            })
            .collect()
    }

    fn coordinator_wakeup_script(workspace: &Path, user: &str) -> String {
        let rows = coordinator_wakeup_rows(user);
        assert_eq!(rows.len(), 2, "one wakeup batches both settled sessions");
        let alpha = rows
            .iter()
            .find(|(name, _)| name == GUI_COORD_ALPHA_SESSION)
            .unwrap()
            .1;
        let beta = rows
            .iter()
            .find(|(name, _)| name == GUI_COORD_BETA_SESSION)
            .unwrap()
            .1;
        let alpha_path = coordinator_workspace(workspace, GUI_COORD_ALPHA_WORKSPACE);
        let beta_path = coordinator_workspace(workspace, GUI_COORD_BETA_WORKSPACE);
        format!(
            "set +e\nprintf 'PI_SESSION_ALPHA %s\\n' '{alpha}'\n\"$OVRCR_EXECUTABLE\" terminal read '{alpha}' --max-lines 30 || exit $?\nprintf 'PI_SESSION_BETA %s\\n' '{beta}'\n\"$OVRCR_EXECUTABLE\" terminal read '{beta}' --max-lines 30 || exit $?\n( cd '{}' || exit 90; printf 'GIT_STATUS_ALPHA\\n'; git status --short --branch || exit $?; printf 'DIFF_STAT_ALPHA\\n'; git diff --stat || exit $?; git diff --check; code=$?; printf 'CHECK_ALPHA exit=%s\\n' \"$code\"; exit \"$code\" ) || exit $?\n( cd '{}' || exit 90; printf 'GIT_STATUS_BETA\\n'; git status --short --branch || exit $?; printf 'DIFF_STAT_BETA\\n'; git diff --stat || exit $?; git diff --check; code=$?; printf 'CHECK_BETA exit=%s\\n' \"$code\"; exit \"$code\" )",
            alpha_path.display(),
            beta_path.display()
        )
    }

    fn coordinator_wakeup_result(user: &str) -> String {
        let rows = coordinator_wakeup_rows(user);
        assert_eq!(rows.len(), 2, "one wakeup contains both sessions");
        let alpha = rows
            .iter()
            .find(|(name, _)| name == GUI_COORD_ALPHA_SESSION)
            .unwrap()
            .1;
        let beta = rows
            .iter()
            .find(|(name, _)| name == GUI_COORD_BETA_SESSION)
            .unwrap()
            .1;
        format!(
            "Both Pi sessions completed their requested README change.\n\n| Task | Workspace | Session |\n|---|---|---|\n| Alpha | {GUI_COORD_ALPHA_WORKSPACE} | coordinator-alpha ({alpha}) |\n| Beta | {GUI_COORD_BETA_WORKSPACE} | coordinator-beta ({beta}) |\n\nVerified `git status`, `git diff --stat`, and `git diff --check` (exit 0) in both Workspaces. Both Retained sessions remain open for review."
        )
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_single_conversation() {
        let pi = pi_executable();
        let live = live::Live::idle().bounded();
        let workspace = live.root.path().join("operator-target");
        std::fs::create_dir(&workspace).unwrap();
        let marker = workspace.join("fixture-marker.txt");
        let background_pid = workspace.join("background.pid");
        let provider = Provider::start(scenario_handler(
            workspace.clone(),
            marker.clone(),
            background_pid.clone(),
        ));
        provider.seed_profile(&live.config);
        // The Server inherits these; the worker inherits them from the Server;
        // the dedicated Pi profile must strip them before Pi or Bash sees them.
        live.start_binary_env(&launcher_environment(&pi));

        let mut driver = DashboardDriver::connect(&live);
        driver.open();
        let opened = driver.conversation();
        assert_eq!(opened.title, "Operator");
        assert_eq!(
            operator_harness::Registry::of(&live).conversation_ids(),
            vec![opened.id.0]
        );

        // Turn one: real read/write/edit/Bash operations and CLI output.
        driver.submit("REQUEST_ONE_REAL_TOOLS");
        let first = wait_terminal(&mut driver, opened.id, &provider, &live);
        assert_eq!(
            tool_names(&first),
            vec!["write", "read", "edit", "bash"],
            "the four native tools ran in order: {first:#?}"
        );
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "OFFLINE_MARKER_v2\n"
        );
        let outputs = operator_harness::tool_outputs(&first);
        assert_eq!(outputs.len(), 4, "{outputs:#?}");
        assert!(outputs[0].1.contains("fixture-marker.txt"), "{outputs:#?}");
        assert!(outputs[1].1.contains("OFFLINE_MARKER_v1"), "{outputs:#?}");
        assert!(
            outputs[3].1.contains("BASH_") && outputs[3].1.contains("[]"),
            "Bash observed the instance CLI: {outputs:#?}"
        );
        // The inherited provider/session credentials are stripped before Pi, so
        // the admitted Bash child cannot see them; an ordinary tool credential
        // still arrives. The durable journal proves both without copying values.
        assert!(
            outputs[3].1.contains(
                "CREDENTIAL_PROBE anthropic= openai= aws= pi_session= pi_session_dir= github=SET"
            ),
            "Bash inherited a denied credential or lost the ordinary one: {outputs:#?}"
        );
        let probe = std::fs::read_to_string(workspace.join("credential-probe.env"))
            .expect("the admitted Bash wrote its credential probe");
        let probed: HashMap<String, String> = probe
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        for (key, _) in DENIED_ENV {
            assert_eq!(
                probed.get(*key).map(String::as_str),
                Some(""),
                "the {key} fixture sentinel reached the admitted Bash child: {probe}"
            );
        }
        assert_eq!(
            probed.get(ORDINARY_ENV.0).map(String::as_str),
            Some(ORDINARY_ENV.1),
            "an ordinary tool credential must reach the admitted Bash child: {probe}"
        );
        assert!(delta_text(&first).contains("TURN_ONE_DONE"));

        let requests = provider.wait_requests(5, BUDGET);
        assert!(requests.len() >= 5, "one provider request per model step");
        assert!(
            requests
                .iter()
                .all(|request| request.authorization.as_deref()
                    == Some("Bearer fixture-synthetic-key")),
            "the dedicated profile's synthetic auth is the only credential"
        );
        assert!(
            requests
                .iter()
                .map(|request| operator_provider::request_text(&request.body))
                .any(|text| text.contains("BASH_"))
                || requests
                    .iter()
                    .flat_map(|request| operator_provider::tool_result_texts(&request.body))
                    .any(|text| text.contains("BASH_")),
            "the provider observed the CLI result: {requests:#?}"
        );

        let registry = operator_harness::Registry::of(&live);
        let turn_one = first
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        assert_eq!(registry.checkpoint(opened.id.0), Some(turn_one as i64));
        let attempt = live
            .config
            .join("operator-pi/turns")
            .join(turn_one.to_string());
        let header = session_header(&attempt.join("session.jsonl"));
        let session_header_id = format!("operator-{}", opened.id.0);
        assert_eq!(header["id"], session_header_id);
        assert_eq!(header["type"], "session");
        wait_stopped(&mut driver, opened.id);

        // Turn two: the same Pi session and prior successful messages.
        let second = submit_next(
            &mut driver,
            opened.id,
            "REQUEST_TWO_RESUME",
            turn_one,
            &provider,
            &live,
        );
        let turn_two = second
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        assert!(delta_text(&second).contains("TURN_TWO_DONE"));
        assert!(delta_text(&second).contains("TURN_ONE_DONE"));
        let resumed = provider.requests();
        let last_body = &resumed.last().unwrap().body;
        let resumed_text = structured_request_text(last_body);
        assert!(resumed_text.contains("TURN_ONE_DONE"), "{resumed_text}");
        assert!(
            resumed_text.contains("OFFLINE_MARKER_v1"),
            "the read result from turn one is prior context: {resumed_text}"
        );
        // The structured extraction must include `tool_calls[].function.arguments`,
        // not only message content: turn one's write arguments are prior context.
        let resumed_arguments = tool_call_arguments_text(last_body);
        assert!(
            resumed_arguments
                .lines()
                .any(|line| line.contains("\"content\"") && line.contains("OFFLINE_MARKER_v1")),
            "turn one's write arguments are prior context: {resumed_arguments}"
        );
        assert_eq!(
            operator_harness::Registry::of(&live).conversation_ids(),
            vec![opened.id.0],
            "one conversation for the whole lifecycle"
        );
        let header = session_header(
            &live
                .config
                .join("operator-pi/turns")
                .join(turn_two.to_string())
                .join("session.jsonl"),
        );
        assert_eq!(header["id"], session_header_id, "same native Pi session id");
        wait_stopped(&mut driver, opened.id);

        // Turn three: streamed output is visible before the provider finishes.
        driver.submit("REQUEST_THREE_HELD");
        let held = driver.wait_history(opened.id, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.identity.turn.0 != turn_two)
                && delta_text(history).contains("HELD_BEFORE_RELEASE")
        });
        assert_eq!(
            held.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Running,
            "the held stream is still running"
        );
        assert!(
            !held.conversation.worker_stopped,
            "a held stream is not a stop"
        );
        let before_release = provider.requests().len();
        provider.release();
        let third = wait_terminal(&mut driver, opened.id, &provider, &live);
        assert!(provider.requests().len() >= before_release);
        assert!(delta_text(&third).contains("HELD_BEFORE_RELEASE"));
        let turn_three = third
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        wait_stopped(&mut driver, opened.id);
        assert_eq!(
            operator_harness::Registry::of(&live).checkpoint(opened.id.0),
            Some(turn_three as i64)
        );

        // Turn four: an interrupted attached Bash command with a background child.
        driver.submit("REQUEST_FOUR_BACKGROUND");
        let pending = driver.wait_history(opened.id, |history| {
            !operator_harness::tool_calls(history).is_empty()
                && history
                    .conversation
                    .current_turn
                    .as_ref()
                    .is_some_and(|turn| turn.state == OperatorTurnState::Running)
        });
        let identity = pending.conversation.current_turn.as_ref().unwrap().identity;
        let deadline = Instant::now() + BUDGET;
        while !background_pid.exists() {
            assert!(
                Instant::now() < deadline,
                "bash never spawned its background child"
            );
            thread::sleep(Duration::from_millis(20));
        }
        let child: u32 = std::fs::read_to_string(&background_pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            process_alive(child),
            "background child {child} must be alive"
        );

        let stopped = match driver.stop(identity) {
            Response::Operator(ovrcr::protocol::OperatorResponse::Stopped(turn)) => turn,
            other => panic!("operator stop: {other:?}"),
        };
        assert_eq!(stopped.state, OperatorTurnState::Interrupted);
        let interrupted =
            driver.wait_history(opened.id, |history| history.conversation.worker_stopped);
        assert_eq!(
            interrupted
                .conversation
                .current_turn
                .as_ref()
                .unwrap()
                .state,
            OperatorTurnState::Interrupted
        );
        assert!(operator_harness::has_assistant(&interrupted, ""));
        let deadline = Instant::now() + BUDGET;
        while process_alive(child) {
            assert!(
                Instant::now() < deadline,
                "the owned background child {child} survived the stop"
            );
            thread::sleep(Duration::from_millis(20));
        }
        let session = driver
            .exchange(Request::Inspect)
            .pipe_inspect()
            .into_iter()
            .find(|session| session.id == opened.session)
            .unwrap();
        assert!(
            !matches!(session.phase, SessionPhase::Running),
            "the controlled stop retired the worker: {session:#?}"
        );

        // Turn five: only the last successful checkpoint is provider context.
        let fifth = submit_next(
            &mut driver,
            opened.id,
            "REQUEST_FIVE_AFTER_INTERRUPT",
            turn_three,
            &provider,
            &live,
        );
        assert!(delta_text(&fifth).contains("TURN_FIVE_DONE"));
        let after = provider.requests();
        let context = structured_request_text(&after.last().unwrap().body);
        assert!(
            context.contains("TURN_THREE_DONE") || context.contains("HELD_BEFORE_RELEASE"),
            "the last successful checkpoint is prior context: {context}"
        );
        assert!(
            !context.contains("REQUEST_FOUR_BACKGROUND"),
            "the interrupted request is not model context: {context}"
        );
        assert!(
            !context.contains("sleep 30"),
            "the interrupted tool call is not model context: {context}"
        );
        // `sleep 30` appears only inside a tool call's arguments in the
        // interrupted session, so this is the assertion that a content-only
        // extraction (or a resumed interrupted attempt) would weaken.
        let interrupted_arguments = tool_call_arguments_text(&after.last().unwrap().body);
        assert!(
            !interrupted_arguments.contains("sleep 30"),
            "the interrupted tool-call arguments are not model context: {interrupted_arguments}"
        );
        let interrupted_journal = operator_harness::history_text(&interrupted);
        assert!(
            interrupted_journal.contains("REQUEST_FOUR_BACKGROUND")
                && interrupted_journal.contains("sleep 30"),
            "the visible interrupted journal remains: {interrupted:#?}"
        );
        assert!(!operator_harness::tool_calls(&interrupted).is_empty());
        wait_stopped(&mut driver, opened.id);

        // Turn six: one assistant message with two tool calls. Pi prepares every
        // admission before executing anything unless the tools are sequential,
        // so this fails the turn unless the extension registered every tool sequentially.
        let fifth_turn = fifth
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        let requests_before = provider.requests().len();
        let sixth = submit_next(
            &mut driver,
            opened.id,
            "REQUEST_SIX_TWO_TOOLS",
            fifth_turn,
            &provider,
            &live,
        );
        let sixth_turn = sixth
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        assert!(delta_text(&sixth).contains("TWO_TOOL_TURN_DONE"));
        assert_eq!(
            tool_names_for(&sixth, sixth_turn),
            vec!["read", "bash"],
            "both calls of one assistant message journal in order: {sixth:#?}"
        );
        let two_tool_outputs = tool_outputs_for(&sixth, sixth_turn);
        let read_output = two_tool_outputs
            .iter()
            .find(|(id, _)| id == "call-two-read")
            .map(|(_, output)| output.clone())
            .expect("read receipt for the batch");
        let bash_output = two_tool_outputs
            .iter()
            .find(|(id, _)| id == "call-two-bash")
            .map(|(_, output)| output.clone())
            .expect("bash receipt for the batch");
        assert!(
            read_output.contains("OFFLINE_MARKER_v2"),
            "the read executed after the write: {read_output}"
        );
        assert!(
            bash_output.contains("TWO_TOOL_BASH"),
            "the bash executed after the read: {bash_output}"
        );
        let after_six = provider.requests();
        assert!(
            after_six.len() >= requests_before + 2,
            "one request per model step of the batch: {:#?}",
            after_six.len()
        );
        let batch_results = operator_provider::tool_result_texts(&after_six.last().unwrap().body);
        assert!(
            batch_results
                .iter()
                .any(|text| text.contains("OFFLINE_MARKER_v2"))
                && batch_results
                    .iter()
                    .any(|text| text.contains("TWO_TOOL_BASH")),
            "both tool results reached the follow-up model request: {batch_results:#?}"
        );
        eprintln!(
            "INSTALLED_PI_TWO_TOOL_CALLS PASS turn={} tools={:?} read={} bash={} provider_requests={}",
            sixth_turn,
            tool_names_for(&sixth, sixth_turn),
            read_output.lines().next().unwrap_or_default(),
            bash_output.lines().next().unwrap_or_default(),
            provider.requests().len()
        );
        wait_stopped(&mut driver, opened.id);

        // Turn seven: a real attached Bash call with a short native timeout. It
        // spawns a descendant that keeps writing, so only the timeout ends the
        // tool; the durable TimedOut receipt must stop the turn without another
        // provider request and without promoting this attempt as a checkpoint.
        let timeout_output = workspace.join("timeout-writer.out");
        let timeout_pid = workspace.join("timeout-writer.pid");
        let requests_before_timeout = provider.requests().len();
        // The timeout turn must leave whatever checkpoint the previous completed
        // turn promoted untouched, without assuming which turn that was here.
        let checkpoint_before_timeout =
            operator_harness::Registry::of(&live).checkpoint(opened.id.0);
        driver.submit("REQUEST_SEVEN_TIMEOUT");
        let pending_timeout = driver.wait_history(opened.id, |history| {
            let Some(turn) = history.conversation.current_turn.as_ref() else {
                return false;
            };
            turn.state == OperatorTurnState::Running
                && turn.identity.turn.0 > sixth_turn
                && tool_names_for(history, turn.identity.turn.0)
                    .iter()
                    .any(|name| name == "bash")
        });
        let timeout_identity = pending_timeout
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity;
        let timeout_turn = timeout_identity.turn.0;
        let pid = driver
            .session(opened.session)
            .pid
            .expect("the timed-out turn owns a worker");
        let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
        assert!(pgid > 1, "the worker has no process group: {pgid}");
        live.own_group(pgid);
        let deadline = Instant::now() + BUDGET;
        while !timeout_pid.exists() {
            assert!(
                Instant::now() < deadline,
                "the timed bash command never spawned its descendant"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let descendant: u32 = std::fs::read_to_string(&timeout_pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_ne!(descendant, pid, "the descendant is a real child");
        assert!(
            process_alive(descendant),
            "the background descendant must be alive while the command is attached"
        );
        let written = std::fs::metadata(&timeout_output)
            .map(|meta| meta.len())
            .unwrap_or(0);
        thread::sleep(Duration::from_millis(150));
        let grown = std::fs::metadata(&timeout_output)
            .map(|meta| meta.len())
            .unwrap_or(0);
        assert!(
            grown > written,
            "the background descendant must keep writing: {written} -> {grown}"
        );

        let timed_out = driver.wait_history(opened.id, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.state != OperatorTurnState::Running)
        });
        assert_eq!(
            timed_out.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Interrupted,
            "a timed-out tool stops the turn through controlled stop: {timed_out:#?}"
        );
        assert!(
            operator_harness::interrupted(&timed_out),
            "the timed-out turn is interrupted: {timed_out:#?}"
        );
        assert!(
            !has_completed(&timed_out),
            "a timed-out tool never completes the turn"
        );
        let timed_outputs = tool_outputs_with_status(&timed_out);
        let receipt = timed_outputs
            .iter()
            .find(|(id, _, _)| id == "call-timeout")
            .unwrap_or_else(|| panic!("durable TimedOut receipt missing: {timed_outputs:#?}"));
        assert_eq!(
            receipt.2,
            OperatorToolStatus::TimedOut,
            "the durable receipt records the timeout: {receipt:#?}"
        );
        assert!(
            receipt.1.contains("timed out"),
            "Pi's native timeout text is the durable output: {receipt:#?}"
        );
        let timeout_calls: Vec<(String, String, String)> = operator_harness::tool_calls(&timed_out)
            .into_iter()
            .filter(|(id, _, _)| id == "call-timeout")
            .collect();
        assert_eq!(
            timeout_calls.len(),
            1,
            "one durable admission: {timeout_calls:?}"
        );
        assert!(
            timeout_calls[0].2.contains("\"timeout\":2"),
            "the admitted arguments carry the native timeout: {timeout_calls:?}"
        );
        assert!(
            !delta_text(&timed_out).contains("TIMEOUT_CONTINUATION_MUST_NOT_APPEAR"),
            "the agent loop was released after the timeout: {timed_out:#?}"
        );
        assert_eq!(
            provider.requests().len(),
            requests_before_timeout + 1,
            "a timed-out tool must not make Pi ask the provider again"
        );
        wait_stopped(&mut driver, opened.id);
        let session = driver.session(opened.session);
        assert!(
            !matches!(session.phase, SessionPhase::Running),
            "the controlled stop retired the worker: {session:#?}"
        );
        let deadline = Instant::now() + BUDGET;
        while process_alive(descendant) {
            assert!(
                Instant::now() < deadline,
                "the owned descendant {descendant} survived the stop"
            );
            thread::sleep(Duration::from_millis(20));
        }
        let settled = std::fs::metadata(&timeout_output)
            .map(|meta| meta.len())
            .unwrap_or(0);
        thread::sleep(Duration::from_millis(250));
        assert_eq!(
            std::fs::metadata(&timeout_output)
                .map(|meta| meta.len())
                .unwrap_or(0),
            settled,
            "the descendant stopped writing after the owned group terminated"
        );
        operator_harness::assert_group_absent(&live, pgid);
        assert_eq!(
            operator_harness::Registry::of(&live).checkpoint(opened.id.0),
            checkpoint_before_timeout,
            "a timed-out attempt is never a context checkpoint"
        );
        assert_eq!(
            checkpoint_before_timeout,
            Some(sixth_turn as i64),
            "the timeout turn follows the completed sixth turn"
        );
        let timeout_attempt = live
            .config
            .join("operator-pi/turns")
            .join(timeout_turn.to_string());
        assert!(
            timeout_attempt.is_dir(),
            "the attempt evidence is preserved"
        );
        assert!(
            live.config
                .join("operator-pi/runs")
                .join(timeout_identity.run.0.to_string())
                .join("session.jsonl")
                .is_file(),
            "the timed-out native session is preserved: {timeout_attempt:?}"
        );
        eprintln!(
            "INSTALLED_PI_TIMEOUT PASS turn={} descendant={} pgid={} provider_requests={} checkpoint={:?}",
            timeout_turn,
            descendant,
            pgid,
            provider.requests().len(),
            operator_harness::Registry::of(&live).checkpoint(opened.id.0)
        );

        // A settings/model edit is the only way a turn changes model; no fixture
        // credential sentinel crosses into the journal or settings.
        let mut journal = String::new();
        for history in [&first, &second, &third, &interrupted, &fifth, &sixth] {
            journal.push_str(&operator_harness::history_text(history));
        }
        journal.push_str(&operator_harness::history_text(&timed_out));
        assert!(!journal.contains(operator_provider::API_KEY));
        assert!(!operator_harness::settings_text(&live).contains(operator_provider::API_KEY));
        assert_no_fixture_sentinel("journal", &journal);
        assert_no_fixture_sentinel("settings", &operator_harness::settings_text(&live));
        assert_no_fixture_sentinel(
            "registry",
            &operator_harness::Registry::of(&live).operator_payload(),
        );
        let provider_bodies = serde_json::to_string(&provider.request_bodies()).unwrap();
        assert_no_fixture_sentinel("provider requests", &provider_bodies);
        scan_profile_for_sentinels(&live.config.join("operator-pi"), "native Pi profile");

        // Automatic compaction keeps one conversation and one native session.
        check_compaction(&pi);

        eprintln!(
            "INSTALLED_PI_OPERATOR PASS pi={} conversation={} session={} checkpoints={:?} provider_requests={}",
            pi.display(),
            opened.id.0,
            opened.session.0,
            operator_harness::Registry::of(&live).checkpoint(opened.id.0),
            provider.requests().len()
        );
    }

    trait InspectExt {
        fn pipe_inspect(self) -> Vec<ovrcr::protocol::SessionSummary>;
    }
    impl InspectExt for Response {
        fn pipe_inspect(self) -> Vec<ovrcr::protocol::SessionSummary> {
            match self {
                Response::Inventory { sessions, .. } => sessions,
                other => panic!("inspect: {other:?}"),
            }
        }
    }

    /// Pi's own automatic compaction is forced by COMPACT_ONE's oversized answer,
    /// which crosses the context budget before that same turn settles. The fixture
    /// holds the summarization request so the public Dashboard render shows the
    /// live Compacting status; release resumes and completes the first turn.
    /// COMPACT_TWO then proves a normal post-compaction follow-up. The
    /// conversation, native session and system-only guidance must survive it
    /// unchanged.
    fn check_compaction(pi: &Path) {
        let live = live::Live::idle().bounded();
        let workspace = live.root.path().join("operator-target");
        std::fs::create_dir(&workspace).unwrap();
        install_personal_skill(live.root.path());
        let provider = Provider::start(scenario_handler_mode(
            workspace.clone(),
            workspace.join("compaction-marker"),
            workspace.join("compaction-background.pid"),
            ScenarioMode {
                hold_compaction: true,
                inflate_compaction_turn: true,
                coordinator: false,
            },
        ));
        provider.seed_profile_with(
            &live.config,
            COMPACTION_CONTEXT_WINDOW,
            Some(json!({"enabled": true, "reserveTokens": 0, "keepRecentTokens": 0})),
        );
        live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
        let mut driver = DashboardDriver::connect(&live);
        driver.open();
        let opened = driver.conversation();

        // COMPACT_ONE's oversized answer crosses the budget at the end of the
        // same first turn. Its summarization request is held, so the Dashboard
        // must render the live Compacting status before agent_settled.
        driver.submit("COMPACT_ONE");
        let compaction_index = wait_compaction_status(&mut driver, &provider, opened.id);
        let held = driver.screen();
        assert!(
            held.contains("Compacting context"),
            "the held compaction status must be the public Dashboard render: {held}"
        );
        provider.release();
        let compaction_first = wait_terminal(&mut driver, opened.id, &provider, &live);
        assert!(
            delta_text(&compaction_first).contains("COMPACT_ONE_DONE"),
            "the first-turn answer streamed before compaction: {compaction_first:#?}"
        );
        let cleared = driver.screen();
        assert!(
            !cleared.contains("Compacting context"),
            "the Compacting status must clear after compaction: {cleared}"
        );
        assert_skill_only_in_system(
            "pre-compaction",
            &provider.requests()[0].body,
            &live.config.join("operator-pi"),
        );
        wait_stopped(&mut driver, opened.id);

        // The next turn is a normal request over the compacted context.
        driver.submit("COMPACT_TWO");
        let history = wait_terminal(&mut driver, opened.id, &provider, &live);
        let streamed = delta_text(&history);
        assert!(
            streamed.contains("COMPACT_TWO_DONE") || streamed.contains("FIXTURE_FOLLOWUP_DONE"),
            "the post-compaction assistant answer streamed: {history:#?}"
        );
        assert!(
            !streamed.contains("FIXTURE_COMPACTION_SUMMARY"),
            "the summarization stream is not assistant text: {history:#?}"
        );
        wait_stopped(&mut driver, opened.id);

        let requests = provider.requests();
        eprintln!(
            "INSTALLED_PI_COMPACTION_REQUEST_SHAPES {:?}",
            requests
                .iter()
                .map(|request| (
                    operator_provider::is_compaction_request(&request.body),
                    request_shape(&request.body)
                ))
                .collect::<Vec<_>>()
        );
        let normal = requests
            .iter()
            .enumerate()
            .filter(|(_, request)| !operator_provider::is_compaction_request(&request.body));
        for (index, request) in normal {
            assert_skill_only_in_system(
                &format!("normal provider request {index}"),
                &request.body,
                &live.config.join("operator-pi"),
            );
        }
        for request in requests
            .iter()
            .filter(|request| operator_provider::is_compaction_request(&request.body))
        {
            assert!(
                !operator_provider::non_system_text(&request.body).contains(BUNDLED_SKILL),
                "the compaction conversation payload contains the bundled CLI skill"
            );
            assert!(
                !operator_provider::non_system_text(&request.body)
                    .contains(PERSONAL_SKILL_SENTINEL),
                "the compaction conversation payload contains a personal skill"
            );
        }
        assert!(
            requests
                .iter()
                .skip(compaction_index + 1)
                .any(|request| !operator_provider::is_compaction_request(&request.body)),
            "the turn continues with a normal request after compaction: {requests:#?}"
        );
        for request in requests.iter().skip(compaction_index + 1) {
            if !operator_provider::is_compaction_request(&request.body) {
                assert_skill_only_in_system(
                    "post-compaction",
                    &request.body,
                    &live.config.join("operator-pi"),
                );
            }
        }
        let journal = format!(
            "{}{}",
            operator_harness::history_text(&compaction_first),
            operator_harness::history_text(&history)
        );
        assert_no_guidance("compaction journal", &journal);
        assert_eq!(
            operator_harness::Registry::of(&live).conversation_ids(),
            vec![opened.id.0],
            "compaction keeps one conversation"
        );
        let checkpoint = operator_harness::Registry::of(&live)
            .checkpoint(opened.id.0)
            .unwrap();
        let header = session_header(
            &live
                .config
                .join("operator-pi/turns")
                .join(checkpoint.to_string())
                .join("session.jsonl"),
        );
        assert_eq!(header["id"], format!("operator-{}", opened.id.0));
        eprintln!(
            "INSTALLED_PI_COMPACTION PASS conversation={} compaction_requests={} provider_requests={} status_rendered=true status_cleared=true",
            opened.id.0,
            requests
                .iter()
                .filter(|request| operator_provider::is_compaction_request(&request.body))
                .count(),
            requests.len()
        );
    }

    /// The bundled CLI skill is system-only in the first request of both entry
    /// points, on independent clean profiles, and remains so when the completed
    /// conversation is reopened and resumed. No personal skill is discovered.
    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_bundled_skill_system_only() {
        let pi = pi_executable();
        for (entry, marker, answer, quick) in [
            ("full", "REQUEST_SKILL_SYSTEM", "SKILL_SYSTEM_ANSWER", false),
            ("quick", "REQUEST_SKILL_QUICK", "SKILL_QUICK_ANSWER", true),
        ] {
            let live = live::Live::idle().bounded();
            let workspace = live.root.path().join("operator-target");
            std::fs::create_dir(&workspace).unwrap();
            install_personal_skill(live.root.path());
            let provider = Provider::start(scenario_handler(
                workspace.clone(),
                workspace.join("skill-marker.txt"),
                workspace.join("skill-background.pid"),
            ));
            provider.seed_profile(&live.config);
            live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
            let mut driver = DashboardDriver::connect(&live);
            if quick {
                driver.quick(marker);
            } else {
                driver.open();
                driver.submit(marker);
            }
            let opened = driver.conversation();
            let history = wait_terminal(&mut driver, opened.id, &provider, &live);
            assert!(
                delta_text(&history).contains(answer),
                "{entry}: the first answer streamed: {history:#?}"
            );
            let first = provider.wait_requests(1, BUDGET);
            eprintln!(
                "INSTALLED_PI_SKILL_{entry}_FIRST_SHAPE {:?}",
                request_shape(&first[0].body)
            );
            assert_normal_first_prompt(&format!("{entry} first"), &first[0].body, marker);
            assert_skill_only_in_system(
                &format!("{entry} first"),
                &first[0].body,
                &live.config.join("operator-pi"),
            );
            assert_no_guidance(
                &format!("{entry} journal"),
                &operator_harness::history_text(&history),
            );
            for body in provider.request_bodies() {
                assert_no_guidance(
                    &format!("{entry} conversation payload"),
                    &operator_provider::non_system_text(&body),
                );
            }
            wait_stopped(&mut driver, opened.id);

            driver.reopen();
            driver.submit("REQUEST_SKILL_RESUME");
            let resumed = wait_terminal(&mut driver, opened.id, &provider, &live);
            assert!(
                delta_text(&resumed).contains("SKILL_RESUME_ANSWER"),
                "{entry}: the resumed answer streamed: {resumed:#?}"
            );
            wait_stopped(&mut driver, opened.id);
            let resumed_body = provider
                .requests()
                .last()
                .expect("the resumed turn made a provider request")
                .body
                .clone();
            assert_skill_only_in_system(
                &format!("{entry} resumed"),
                &resumed_body,
                &live.config.join("operator-pi"),
            );
            assert!(
                operator_provider::non_system_text(&resumed_body).contains(answer),
                "{entry}: the previous answer is normal conversation context"
            );
            assert_no_guidance(
                &format!("{entry} resumed journal"),
                &operator_harness::history_text(&resumed),
            );
            assert_eq!(
                operator_harness::Registry::of(&live).conversation_ids(),
                vec![opened.id.0],
                "{entry}: one durable conversation"
            );
            eprintln!(
                "INSTALLED_PI_SKILL_{entry}_PASS conversation={} provider_requests={}",
                opened.id.0,
                provider.requests().len()
            );
        }
    }

    /// A first-turn CLI task runs the matching inherited binary with the
    /// isolated instance and socket, from a relocated directory, and uses the
    /// captured target. The skill arrives only in the system prompt.
    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_bundled_skill_first_turn_cli() {
        let pi = pi_executable();
        let live = live::Live::idle().bounded();
        let inherited_config = live.root.path().join("inherited-config.toml");
        // A PATH-selected decoy must never stand in for the inherited binary.
        let decoy_dir = live.root.path().join("decoy-bin");
        std::fs::create_dir(&decoy_dir).unwrap();
        let decoy_marker = decoy_dir.join("executed");
        let decoy = decoy_dir.join("ovrcr");
        std::fs::write(
            &decoy,
            format!(
                "#!/bin/sh\nprintf 'DECOY_OVRCR_USED\\n' >> '{}'\nexit 42\n",
                decoy_marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(
            std::iter::once(decoy_dir).chain(std::env::split_paths(&live.path())),
        )
        .unwrap();
        live.start_binary_env(&[
            ("OVRCR_PI_EXECUTABLE", pi.as_os_str()),
            ("OVRCR_CONFIG", inherited_config.as_os_str()),
            ("PATH", path.as_os_str()),
        ]);
        live.ready("skill-cli");
        let workspace = live.workspace_path("work");
        let relocated = workspace.parent().unwrap().join("operator-relocated");
        std::fs::create_dir_all(&relocated).unwrap();
        assert!(
            !relocated.starts_with(env!("CARGO_MANIFEST_DIR")),
            "the CLI task runs outside the repository checkout: {}",
            relocated.display()
        );
        let provider = Provider::start(scenario_handler(
            workspace.clone(),
            workspace.join("skill-marker.txt"),
            workspace.join("skill-background.pid"),
        ));
        provider.seed_profile(&live.config);
        assert!(
            !live.root.path().join(".agents/skills").exists(),
            "the CLI fixture HOME must not contain a personal skills directory"
        );
        assert!(
            !live.config.join("operator-pi/skills").exists(),
            "the dedicated Pi profile must not contain a user skills directory"
        );
        assert!(
            !relocated.join(".pi/skills").exists() && !relocated.join(".agents/skills").exists(),
            "the relocated CLI directory must not contain project skills"
        );
        let mut driver = DashboardDriver::connect(&live);
        driver.select_fixture_workspace();
        driver.open();
        let opened = driver.conversation();
        driver.submit("REQUEST_SKILL_CLI");
        let history = wait_terminal(&mut driver, opened.id, &provider, &live);
        wait_stopped(&mut driver, opened.id);
        assert!(
            delta_text(&history).contains("SKILL_CLI_ANSWER"),
            "the CLI turn completed: {history:#?}"
        );

        let first = provider.wait_requests(1, BUDGET);
        eprintln!(
            "INSTALLED_PI_SKILL_CLI_FIRST_SHAPE {:?}",
            request_shape(&first[0].body)
        );
        assert_normal_first_prompt("cli first", &first[0].body, "REQUEST_SKILL_CLI");
        assert_skill_only_in_system(
            "cli first",
            &first[0].body,
            &live.config.join("operator-pi"),
        );
        let user = operator_provider::last_user_text(&first[0].body);
        assert!(user.contains("\"project\":\"fixture\""), "{user}");
        assert!(user.contains("\"workspace\":\"skill-cli\""), "{user}");
        assert!(user.contains("\"workspace_id\":\"work\""), "{user}");
        assert!(
            user.contains(&format!("\"cwd\":\"{}\"", workspace.display())),
            "the captured cwd reaches the first model request: {user}"
        );

        let outputs = operator_harness::tool_outputs(&history);
        let output = &outputs
            .iter()
            .find(|(id, _, _)| id == "call-skill-cli")
            .unwrap_or_else(|| panic!("the real CLI tool call has a durable result: {outputs:#?}"))
            .1;
        let pwd = output
            .lines()
            .find_map(|line| line.strip_prefix("CLI_PWD="))
            .expect("the CLI task prints its working directory");
        assert_eq!(pwd.to_owned(), relocated.display().to_string());
        let environment = output
            .lines()
            .find(|line| line.starts_with("CLI_ENV "))
            .expect("the CLI task prints its inherited environment");
        let environment: HashMap<&str, &str> = environment["CLI_ENV ".len()..]
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .collect();
        let expected_home = live.config.to_string_lossy().into_owned();
        let expected_config = inherited_config.to_string_lossy().into_owned();
        let expected_socket = live.socket.to_string_lossy().into_owned();
        let expected_profile = live
            .config
            .join("operator-pi")
            .to_string_lossy()
            .into_owned();
        let expected_home_dir = live.root.path().to_string_lossy().into_owned();
        assert_eq!(
            environment.get("home").copied(),
            Some(expected_home.as_str())
        );
        assert_eq!(
            environment.get("config").copied(),
            Some(expected_config.as_str())
        );
        assert_eq!(
            environment.get("socket").copied(),
            Some(expected_socket.as_str())
        );
        assert_eq!(
            environment.get("profile").copied(),
            Some(expected_profile.as_str())
        );
        assert_eq!(
            environment.get("home_dir").copied(),
            Some(expected_home_dir.as_str())
        );
        let executable = environment
            .get("exe")
            .copied()
            .expect("the CLI task prints OVRCR_EXECUTABLE");
        assert_eq!(
            std::path::Path::new(executable).canonicalize().unwrap(),
            live.executable.canonicalize().unwrap(),
            "the shell used the matching OVRCR binary"
        );
        let version = output
            .lines()
            .find_map(|line| line.strip_prefix("CLI_VERSION "))
            .expect("the real binary reports its version");
        assert!(version.contains(env!("CARGO_PKG_VERSION")), "{version}");
        assert!(version.contains("protocol"), "{version}");
        let project = output
            .lines()
            .find_map(|line| line.strip_prefix("CLI_PROJECT "))
            .expect("project get returns JSON");
        assert!(project.contains("\"name\":\"fixture\""), "{project}");
        assert!(
            project.contains(&format!("\"repo\":\"{}\"", live.repo.display())),
            "{project}"
        );
        let workspaces = output
            .lines()
            .find_map(|line| line.strip_prefix("CLI_WORKSPACES "))
            .expect("workspace list returns JSON");
        assert!(workspaces.contains("\"id\":\"work\""), "{workspaces}");
        assert!(
            workspaces.contains(&format!("\"path\":\"{}\"", workspace.display())),
            "{workspaces}"
        );
        assert!(
            workspaces.contains("\"branch\":\"skill-cli\""),
            "{workspaces}"
        );

        let requests = provider.requests();
        assert!(requests.len() >= 2, "the tool result feeds another request");
        for (index, request) in requests.iter().enumerate() {
            assert_skill_only_in_system(
                &format!("cli provider request {index}"),
                &request.body,
                &live.config.join("operator-pi"),
            );
            for result in operator_provider::tool_result_texts(&request.body) {
                assert_no_guidance("CLI tool result", &result);
            }
        }
        assert_no_guidance("CLI tool output", output);
        assert!(
            !decoy_marker.exists(),
            "a PATH-selected ovrcr ran instead of the inherited binary"
        );
        let journal = operator_harness::history_text(&history);
        assert_no_guidance("CLI journal", &journal);
        assert!(journal.contains("CLI_ENV"), "the CLI receipt is journaled");
        eprintln!(
            "INSTALLED_PI_SKILL_CLI_PASS conversation={} workspace={} relocated={} provider_requests={}",
            opened.id.0,
            workspace.display(),
            relocated.display(),
            requests.len()
        );
    }

    // ------------------------------------------------------------ GUI provider

    /// The GUI smoke's scripted provider. It refuses anything but a
    /// launcher-owned GUI fixture root: `OVRCR_OPERATOR_GUI_HOME` must name a
    /// private `ovrcr-gui-*` root whose launcher created `operator-pi-fixture-owner`
    /// (mode 0600) containing this exact root and the matching
    /// `OVRCR_OPERATOR_GUI_TOKEN`. It seeds only that root's `operator-pi`,
    /// prints `OPERATOR_PI_GUI_READY`, and serves until the root disappears or a
    /// private stop marker appears. A private release marker releases a held
    /// stream. With `OVRCR_OPERATOR_GUI_COMPACTION` set, the profile uses a small
    /// context window and COMPACT_ONE produces an oversized answer, which triggers
    /// automatic compaction in that same turn. Observe `Compacting context…`, then
    /// create the release marker to finish the turn; send COMPACT_TWO afterwards
    /// to prove the normal post-compaction follow-up. With
    /// `OVRCR_OPERATOR_GUI_COORDINATOR=1`, it checks the private HOME's matching
    /// ownership marker, seeds its normal Pi profile to the loopback provider,
    /// and starts two managed Pi sessions on separate fixture worktrees.
    #[test]
    #[ignore = "manual GUI acceptance helper; requires OVRCR_OPERATOR_GUI_HOME and OVRCR_OPERATOR_GUI_TOKEN"]
    fn operator_pi_gui_provider() {
        let root = gui_root();
        let stop_marker = root.join("operator-pi/fixture-stop");
        let release_marker = root.join("operator-pi/fixture-release");
        let workspace = gui_workspace(&root);
        std::fs::create_dir_all(&workspace).unwrap();
        let marker = workspace.join("gui-fixture-marker.txt");
        let background_pid = workspace.join("gui-background.pid");
        // Manual compaction mode: COMPACT_ONE returns an oversized assistant
        // answer, so the next COMPACT_TWO prompt crosses the seeded threshold.
        // The summarization request is held until the release marker exists, so
        // the GUI can show the live Compacting status before releasing it.
        let compaction = std::env::var_os("OVRCR_OPERATOR_GUI_COMPACTION")
            .is_some_and(|value| !value.is_empty());
        let coordinator = std::env::var_os("OVRCR_OPERATOR_GUI_COORDINATOR")
            .is_some_and(|value| !value.is_empty());
        assert!(
            !(compaction && coordinator),
            "run one GUI provider mode at a time"
        );
        let provider = Provider::start(scenario_handler_mode(
            workspace.clone(),
            marker.clone(),
            background_pid.clone(),
            ScenarioMode {
                hold_compaction: compaction,
                inflate_compaction_turn: compaction,
                coordinator,
            },
        ));
        if compaction {
            provider.seed_profile_with(
                &root,
                COMPACTION_CONTEXT_WINDOW,
                Some(json!({"enabled": true, "reserveTokens": 0, "keepRecentTokens": 0})),
            );
        } else {
            provider.seed_profile(&root);
        }
        if coordinator {
            let token = std::env::var("OVRCR_OPERATOR_GUI_TOKEN")
                .expect("gui_root validated OVRCR_OPERATOR_GUI_TOKEN");
            let home = gui_private_workspace_home(std::env::var_os("HOME"), &token)
                .unwrap_or_else(|error| panic!("{error}"));
            assert!(
                home.starts_with("/private/tmp"),
                "coordinator GUI HOME must be task-private under /private/tmp"
            );
            assert!(
                std::env::var_os("PI_CODING_AGENT_DIR").is_none(),
                "clear inherited PI_CODING_AGENT_DIR before starting the GUI"
            );
            provider.seed_workspace_profile(&home);
            let sessions = create_gui_coordinator_sessions(&root, &pi_executable());
            eprintln!("OPERATOR_PI_GUI_COORDINATOR_SESSIONS {sessions}");
        }
        let _ = std::fs::remove_file(&release_marker);
        let _ = std::fs::remove_file(&stop_marker);
        println!(
            "OPERATOR_PI_GUI_READY base={} root={} compaction={compaction} coordinator={coordinator}",
            provider.base,
            root.display()
        );
        use std::io::Write as _;
        std::io::stdout().flush().unwrap();
        eprintln!(
            "OPERATOR_PI_GUI_PROVIDER root={} workspace={} marker={}",
            root.display(),
            workspace.display(),
            marker.display()
        );
        let deadline = Instant::now() + Duration::from_secs(3600);
        while root.is_dir() && !stop_marker.exists() {
            if release_marker.exists() {
                provider.release();
            }
            if Instant::now() >= deadline {
                eprintln!("OPERATOR_PI_GUI_PROVIDER timeout");
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        // Capture the request evidence beside the fixture before exiting.
        let evidence = root.join("operator-pi/fixture-requests.jsonl");
        let mut body = String::new();
        for request in provider.requests() {
            body.push_str(
                &serde_json::to_string(&json!({
                    "path": request.path,
                    "authorization": request.authorization.as_ref().map(|_| "<redacted>"),
                    "body": request.body,
                }))
                .unwrap(),
            );
            body.push('\n');
        }
        let _ = std::fs::write(&evidence, body);
        let _ = std::fs::set_permissions(&evidence, std::fs::Permissions::from_mode(0o600));
        eprintln!(
            "OPERATOR_PI_GUI_PROVIDER exit root_exists={} stop_marker={} requests={} evidence={}",
            root.is_dir(),
            stop_marker.exists(),
            provider.requests().len(),
            evidence.display()
        );
    }

    /// The GUI launcher's ownership marker. The provider helper writes a private
    /// `operator-pi` profile into the root it is given, so it must only ever seed
    /// a root a launcher explicitly claimed for this task; a sibling task's
    /// fixture must be refused, not clobbered.
    const GUI_OWNER_MARKER: &str = "operator-pi-fixture-owner";
    const GUI_OWNER_VERSION: &str = "ovrcr-operator-gui-fixture v1";

    fn gui_owner_text(root: &Path, token: &str) -> String {
        format!(
            "{GUI_OWNER_VERSION}\nroot={}\ntoken={token}\n",
            root.display()
        )
    }

    fn gui_private_workspace_home(home: Option<OsString>, token: &str) -> Result<PathBuf, String> {
        let Some(home) = home.filter(|value| !value.is_empty()) else {
            return Err("coordinator GUI needs a private HOME".into());
        };
        let home = PathBuf::from(home)
            .canonicalize()
            .map_err(|error| format!("coordinator GUI HOME unavailable: {error}"))?;
        let metadata = std::fs::symlink_metadata(&home)
            .map_err(|error| format!("coordinator GUI HOME unavailable: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("coordinator GUI HOME must be a real directory".into());
        }
        if metadata.mode() & 0o777 != 0o700 {
            return Err("coordinator GUI HOME must be private (mode 0700)".into());
        }
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("coordinator GUI HOME is owned by another user".into());
        }

        let marker = home.join(GUI_OWNER_MARKER);
        let marker_metadata = std::fs::symlink_metadata(&marker).map_err(|_| {
            format!(
                "coordinator GUI HOME needs {GUI_OWNER_MARKER} (mode 0600) bound to this HOME and fixture token"
            )
        })?;
        if !marker_metadata.is_file() || marker_metadata.file_type().is_symlink() {
            return Err(format!(
                "coordinator GUI HOME {GUI_OWNER_MARKER} must be a regular file, not a symlink"
            ));
        }
        if marker_metadata.mode() & 0o777 != 0o600
            || marker_metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err(format!(
                "coordinator GUI HOME {GUI_OWNER_MARKER} must be private (mode 0600) and owned by this user"
            ));
        }
        if std::fs::read_to_string(&marker)
            .map_err(|error| format!("coordinator GUI HOME marker: {error}"))?
            != gui_owner_text(&home, token)
        {
            return Err(format!(
                "coordinator GUI HOME {GUI_OWNER_MARKER} does not bind this HOME and fixture token"
            ));
        }

        for directory in [home.join(".pi"), home.join(".pi/agent")] {
            match std::fs::symlink_metadata(&directory) {
                Ok(metadata)
                    if metadata.is_dir()
                        && !metadata.file_type().is_symlink()
                        && metadata.uid() == unsafe { libc::geteuid() }
                        && metadata.mode() & 0o777 == 0o700 => {}
                Ok(_) => {
                    return Err(format!(
                        "coordinator GUI Pi profile directory must be private and cannot be a symlink: {}",
                        directory.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "cannot inspect coordinator GUI Pi profile directory {}: {error}",
                        directory.display()
                    ));
                }
            }
        }
        let profile = home.join(".pi/agent");
        for filename in ["models.json", "settings.json"] {
            let path = profile.join(filename);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(format!(
                        "coordinator GUI Pi profile file must be regular, not a symlink: {}",
                        path.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "cannot inspect coordinator GUI Pi profile file {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        let auth = profile.join("auth.json");
        match std::fs::symlink_metadata(&auth) {
            Ok(_) => return Err("coordinator GUI Pi profile contains auth.json".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect coordinator GUI Pi profile auth file: {error}"
                ));
            }
        }
        Ok(home)
    }

    fn gui_root() -> PathBuf {
        gui_root_from(
            std::env::var_os("OVRCR_OPERATOR_GUI_HOME"),
            std::env::var_os("OVRCR_OPERATOR_GUI_TOKEN"),
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Validate a launcher-owned GUI fixture root. `home` is
    /// `OVRCR_OPERATOR_GUI_HOME`; `token` must match the private ownership
    /// marker the launcher wrote into that root.
    fn gui_root_from(home: Option<OsString>, token: Option<OsString>) -> Result<PathBuf, String> {
        let Some(home) = home.filter(|value| !value.is_empty()) else {
            return Err(
                "set OVRCR_OPERATOR_GUI_HOME to the launcher-created GUI fixture root".into(),
            );
        };
        let token = token
            .and_then(|value| value.into_string().ok())
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 128
                    && value
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
            })
            .ok_or_else(|| {
                format!("set OVRCR_OPERATOR_GUI_TOKEN to the token recorded in {GUI_OWNER_MARKER}")
            })?;
        let root = PathBuf::from(home)
            .canonicalize()
            .map_err(|error| format!("GUI fixture root unavailable: {error}"))?;
        if !root.is_dir() {
            return Err(format!(
                "GUI fixture root is not a directory: {}",
                root.display()
            ));
        }
        if !root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ovrcr-gui-"))
        {
            return Err(format!(
                "refusing {}: a launcher-owned GUI fixture root is named ovrcr-gui-*",
                root.display()
            ));
        }
        let registry = std::fs::symlink_metadata(root.join("registry.sqlite3")).map_err(|_| {
            format!(
                "GUI fixture root needs registry.sqlite3: {}",
                root.display()
            )
        })?;
        if !registry.is_file() || registry.file_type().is_symlink() {
            return Err(format!(
                "GUI fixture root needs a regular registry.sqlite3: {}",
                root.display()
            ));
        }
        if std::fs::symlink_metadata(root.join("server.sock")).is_err() {
            return Err(format!(
                "GUI fixture root needs server.sock: {}",
                root.display()
            ));
        }
        let marker = root.join(GUI_OWNER_MARKER);
        let expected = gui_owner_text(&root, &token);
        let metadata = std::fs::symlink_metadata(&marker).map_err(|_| {
            format!(
                "refusing {}: the GUI launcher must create {GUI_OWNER_MARKER} (mode 0600) containing:\n{expected}",
                root.display()
            )
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing {}: {GUI_OWNER_MARKER} must be a regular file, not a symlink",
                root.display()
            ));
        }
        if metadata.mode() & 0o077 != 0 {
            return Err(format!(
                "refusing {}: {GUI_OWNER_MARKER} must be private (mode 0600)",
                root.display()
            ));
        }
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(format!(
                "refusing {}: {GUI_OWNER_MARKER} is owned by another user",
                root.display()
            ));
        }
        let recorded = std::fs::read_to_string(&marker)
            .map_err(|error| format!("refusing {}: {GUI_OWNER_MARKER}: {error}", root.display()))?;
        if recorded != expected {
            return Err(format!(
                "refusing {}: {GUI_OWNER_MARKER} does not bind this root and token; expected exactly:\n{expected}",
                root.display()
            ));
        }
        Ok(root)
    }

    /// Proves the ownership guard: another task's GUI fixture is refused and left
    /// untouched, and only a private marker binding this root and token is
    /// accepted.
    #[test]
    fn operator_gui_fixture_requires_a_launcher_ownership_marker() {
        let token = "task-owner-token-7f3a";
        let base = tempfile::tempdir().unwrap();

        // A sibling task's fixture root: right shape, no ownership marker.
        let foreign = base.path().join("ovrcr-gui-other-task");
        std::fs::create_dir(&foreign).unwrap();
        std::fs::write(foreign.join("registry.sqlite3"), b"").unwrap();
        std::fs::write(foreign.join("server.sock"), b"").unwrap();
        let foreign_profile = foreign.join("operator-pi");
        std::fs::create_dir(&foreign_profile).unwrap();
        std::fs::write(foreign_profile.join("models.json"), b"ANOTHER_TASK").unwrap();

        let error = gui_root_from(None, Some(token.into())).unwrap_err();
        assert!(error.contains("OVRCR_OPERATOR_GUI_HOME"), "{error}");

        let error = gui_root_from(
            Some(foreign.clone().into_os_string()),
            Some("bad token".into()),
        )
        .unwrap_err();
        assert!(error.contains("OVRCR_OPERATOR_GUI_TOKEN"), "{error}");

        let wrong_root = base.path().join("not-a-gui-fixture");
        std::fs::create_dir(&wrong_root).unwrap();
        let error =
            gui_root_from(Some(wrong_root.into_os_string()), Some(token.into())).unwrap_err();
        assert!(
            error.contains("a launcher-owned GUI fixture root is named ovrcr-gui-*"),
            "{error}"
        );

        let error =
            gui_root_from(Some(foreign.clone().into_os_string()), Some(token.into())).unwrap_err();
        assert!(error.contains(GUI_OWNER_MARKER), "{error}");

        // A marker that does not bind this exact root and token is refused, and
        // a non-private or symlinked marker is refused too.
        let marker = foreign.join(GUI_OWNER_MARKER);
        std::fs::write(
            &marker,
            gui_owner_text(Path::new("/somewhere/else"), "other-task-token"),
        )
        .unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();
        let error =
            gui_root_from(Some(foreign.clone().into_os_string()), Some(token.into())).unwrap_err();
        assert!(
            error.contains("does not bind this root and token"),
            "{error}"
        );
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error =
            gui_root_from(Some(foreign.clone().into_os_string()), Some(token.into())).unwrap_err();
        assert!(error.contains("must be private"), "{error}");
        std::fs::remove_file(&marker).unwrap();
        let target = base.path().join("foreign-owner-target");
        std::fs::write(&target, gui_owner_text(&foreign, token)).unwrap();
        std::os::unix::fs::symlink(&target, &marker).unwrap();
        let error =
            gui_root_from(Some(foreign.clone().into_os_string()), Some(token.into())).unwrap_err();
        assert!(
            error.contains("must be a regular file, not a symlink"),
            "{error}"
        );

        // Refusal never seeded anything: the other task's profile is intact.
        assert_eq!(
            std::fs::read(foreign_profile.join("models.json")).unwrap(),
            b"ANOTHER_TASK"
        );
        assert!(!foreign_profile.join("settings.json").exists());

        // A private launcher marker binds the canonical root and matching token.
        let accepted = base
            .path()
            .join(format!("ovrcr-gui-fixture-{}", std::process::id()));
        std::fs::create_dir(&accepted).unwrap();
        std::fs::write(accepted.join("registry.sqlite3"), b"").unwrap();
        std::fs::write(accepted.join("server.sock"), b"").unwrap();
        let accepted = accepted.canonicalize().unwrap();
        let owner = accepted.join(GUI_OWNER_MARKER);
        std::fs::write(&owner, gui_owner_text(&accepted, token)).unwrap();
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            gui_root_from(Some(accepted.clone().into_os_string()), Some(token.into())).unwrap(),
            accepted
        );

        let home = base.path().join("ovrcr-gui-workspace-home");
        std::fs::create_dir(&home).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error =
            gui_private_workspace_home(Some(home.clone().into_os_string()), token).unwrap_err();
        assert!(error.contains(GUI_OWNER_MARKER), "{error}");

        let home = home.canonicalize().unwrap();
        let marker = home.join(GUI_OWNER_MARKER);
        std::fs::write(&marker, gui_owner_text(&home, token)).unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            gui_private_workspace_home(Some(home.clone().into_os_string()), token).unwrap(),
            home
        );

        let pi = base.path().join("foreign-pi-profile");
        std::fs::create_dir(&pi).unwrap();
        std::os::unix::fs::symlink(&pi, home.join(".pi")).unwrap();
        let error = gui_private_workspace_home(Some(home.into_os_string()), token).unwrap_err();
        assert!(error.contains("cannot be a symlink"), "{error}");
    }

    fn create_gui_coordinator_sessions(root: &Path, pi: &Path) -> String {
        let cli = Path::new(env!("CARGO_BIN_EXE_ovrcr"));
        let home = std::env::var_os("HOME").expect("coordinator GUI needs a private HOME");
        for (name, branch, label) in [
            (
                GUI_COORD_ALPHA_SESSION,
                GUI_COORD_ALPHA_WORKSPACE,
                "pi / coordinator alpha",
            ),
            (
                GUI_COORD_BETA_SESSION,
                GUI_COORD_BETA_WORKSPACE,
                "pi / coordinator beta",
            ),
        ] {
            let output = Command::new(cli)
                .args([
                    "terminal",
                    "create",
                    "--json",
                    "--project",
                    "consigint",
                    "--workspace",
                    branch,
                    "--name",
                    name,
                    "--label",
                    label,
                    "--",
                ])
                .arg(cli)
                .args(["agent", "run", "pi", "--"])
                .arg(pi)
                .env("HOME", &home)
                .env("OVRCR_HOME", root)
                .env("OVRCR_SOCKET", root.join("server.sock"))
                .output()
                .unwrap_or_else(|error| panic!("start {name} fixture session: {error}"));
            assert!(
                output.status.success(),
                "create {name} fixture session failed with {}",
                output.status
            );
        }

        let deadline = Instant::now() + Duration::from_secs(60);
        let rows = loop {
            let output = Command::new(cli)
                .args(["terminal", "list", "--json"])
                .env("HOME", &home)
                .env("OVRCR_HOME", root)
                .env("OVRCR_SOCKET", root.join("server.sock"))
                .output()
                .expect("inspect coordinator fixture sessions");
            if output.status.success() {
                let sessions: Vec<Value> = serde_json::from_slice(&output.stdout)
                    .expect("terminal list --json returns session rows");
                let rows: Vec<Value> = [GUI_COORD_ALPHA_SESSION, GUI_COORD_BETA_SESSION]
                    .into_iter()
                    .map(|name| {
                        sessions
                            .iter()
                            .find(|row| row["name"] == name)
                            .cloned()
                            .unwrap_or(Value::Null)
                    })
                    .collect();
                if rows.iter().all(|row| {
                    row["scope"] == "Workspace"
                        && row["phase"] == "running"
                        && row["kind"]["agent"] == "pi"
                        && row["agent"]["activity_revision"].as_u64().is_some()
                }) {
                    break rows;
                }
            }
            assert!(
                Instant::now() < deadline,
                "managed Pi coordinator fixture sessions did not reach Reporter readiness"
            );
            thread::sleep(Duration::from_millis(100));
        };
        serde_json::to_string(
            &rows
                .iter()
                .map(|row| {
                    json!({
                        "name": row["name"],
                        "id": row["id"],
                        "run": row["run"],
                        "workspace": row["workspace"],
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    fn gui_workspace(root: &Path) -> PathBuf {
        // The GUI owns a disposable fixture root; its operator workdir is private.
        let candidates = ["operator-target", "workspace", "project"];
        for candidate in candidates {
            let path = root.join(candidate);
            if path.is_dir() {
                return path;
            }
        }
        let target = root.join("operator-target");
        let _ = Command::new("mkdir").arg("-p").arg(&target).status();
        target
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn provider_smoke_streams_thinking_before_released_answer() {
    use std::io::{Read, Write};
    let provider = operator_provider::Provider::start(Box::new(|_body, _index| {
        operator_provider::Plan::new(vec![
            operator_provider::Chunk::Thinking("THINKING_ONE".into()),
            operator_provider::Chunk::Thinking("THINKING_TWO".into()),
        ])
        .holding()
        .then_text_after_release("ANSWER_AFTER_RELEASE")
    }));
    let mut stream =
        std::net::TcpStream::connect(provider.base.trim_start_matches("http://")).unwrap();
    let body =
        r#"{"model":"operator-fixture","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();

    let mut received = String::new();
    let mut buffer = [0_u8; 4096];
    while !received.contains("THINKING_TWO") {
        let read = stream.read(&mut buffer).unwrap();
        assert!(
            read > 0,
            "provider closed before the held thinking: {received}"
        );
        received.push_str(&String::from_utf8_lossy(&buffer[..read]));
    }
    assert!(received.contains("reasoning_content"), "{received}");
    assert!(!received.contains("ANSWER_AFTER_RELEASE"), "{received}");

    provider.release();
    let mut rest = String::new();
    stream.read_to_string(&mut rest).unwrap();
    received.push_str(&rest);
    assert!(received.contains("ANSWER_AFTER_RELEASE"), "{received}");
    assert!(received.contains("[DONE]"), "{received}");
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn provider_smoke_streams_openai_chunks() {
    use std::io::{Read, Write};
    let provider = operator_provider::Provider::start(Box::new(|_body, _index| {
        operator_provider::Plan::text("HELLO_PROVIDER")
    }));
    let mut stream =
        std::net::TcpStream::connect(provider.base.trim_start_matches("http://")).unwrap();
    let body =
        r#"{"model":"operator-fixture","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    eprintln!("PROVIDER_SMOKE {response}");
    assert!(response.contains("HELLO_PROVIDER"), "{response}");
    assert!(response.contains("[DONE]"), "{response}");
    assert_eq!(provider.requests().len(), 1);
}
