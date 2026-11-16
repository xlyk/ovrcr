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
        skill: None,
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

#[test]
fn dashboard_slash_commands_menu_copy_and_thinking_work_end_to_end() {
    use crossterm::event::{Event, KeyCode};
    use ovrcr::protocol::{
        self, ClientMessage, OperatorConversationId, OperatorHistory, OperatorResponse,
        ServerMessage, Settings, TerminalSize,
    };
    use ovrcr::tui::{Dashboard, DashboardAction, write_clipboard};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use std::collections::VecDeque;

    struct LiveDashboard {
        stream: std::os::unix::net::UnixStream,
        dashboard: Dashboard,
        pending: VecDeque<ClientMessage>,
        next_probe_id: u64,
    }

    impl LiveDashboard {
        fn connect(live: &Live) -> Self {
            let mut stream = protocol::connect_server(&live.socket).unwrap();
            let budget = Duration::from_secs(5);
            stream.set_read_timeout(Some(budget)).unwrap();
            stream.set_write_timeout(Some(budget)).unwrap();
            let hierarchy =
                match protocol::client::request(&mut stream, 900_000, Request::DashboardHello)
                    .unwrap()
                {
                    Response::Hierarchy(hierarchy) => hierarchy,
                    other => panic!("Dashboard handshake: {other:?}"),
                };
            let mut dashboard = Dashboard::new(TerminalSize {
                rows: 40,
                cols: 160,
            });
            dashboard.install_hierarchy(hierarchy);
            dashboard.install_area(Rect::new(0, 0, 160, 40));
            dashboard.install_settings(Settings::default());
            Self {
                stream,
                dashboard,
                pending: VecDeque::new(),
                next_probe_id: 900_001,
            }
        }

        fn exchange(&mut self, request: Request) -> Response {
            self.next_probe_id += 1;
            let id = self.next_probe_id;
            protocol::write_frame(
                &mut self.stream,
                &ClientMessage {
                    request_id: id,
                    request,
                },
            )
            .unwrap();
            loop {
                let incoming: ServerMessage = protocol::read_frame(&mut self.stream).unwrap();
                let matched = match &incoming {
                    ServerMessage::Response {
                        request_id,
                        response,
                    } if *request_id == id => Some(response.clone()),
                    _ => None,
                };
                self.pending
                    .extend(self.dashboard.handle_server_message(incoming));
                self.pending.extend(self.dashboard.drain_outbox());
                if let Some(response) = matched {
                    self.flush();
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

        fn pump(&mut self) {
            self.stream
                .set_read_timeout(Some(Duration::from_millis(10)))
                .unwrap();
            while let Ok(message) = protocol::read_frame::<ServerMessage>(&mut self.stream) {
                self.pending
                    .extend(self.dashboard.handle_server_message(message));
            }
            self.stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            self.pending.extend(self.dashboard.drain_outbox());
            if !self.pending.is_empty() {
                self.flush();
            }
        }

        fn current_id(&mut self) -> OperatorConversationId {
            match self.exchange(Request::Operator(OperatorRequest::Current)) {
                Response::Operator(OperatorResponse::Current(Some(summary))) => summary.id,
                other => panic!("expected current conversation, got {other:?}"),
            }
        }

        fn wait_turn_settled(&mut self, conversation: OperatorConversationId) -> OperatorHistory {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                self.pump();
                let history = match self.exchange(Request::Operator(OperatorRequest::Read {
                    conversation,
                    after: 0,
                    limit: 100,
                })) {
                    Response::Operator(OperatorResponse::History(history)) => history,
                    other => panic!("expected history, got {other:?}"),
                };
                if history
                    .conversation
                    .current_turn
                    .as_ref()
                    .is_some_and(|t| t.state == OperatorTurnState::Complete)
                    && history.conversation.worker_stopped
                {
                    self.pump();
                    return history;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for settled turn: {history:#?}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn render(&mut self) -> String {
            let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
            terminal
                .draw(|frame| ovrcr::tui::draw_dashboard(frame, &self.dashboard))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..40)
                .map(|y| {
                    (0..160)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    let raw_markdown = "# Deployment Plan\n\n```sh\ncargo test\n```";
    let mut scenario = Scenario::new();
    scenario.set(
        "available_thinking_levels",
        json!(["minimal", "low", "medium", "high"]),
    );
    scenario.prompt_steps(vec![
        vec![Step::assistant(raw_markdown), Step::Settle],
        vec![
            Step::assistant("Second reply with high thinking."),
            Step::Settle,
        ],
    ]);
    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    fake.script(&scenario);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let mut ui = LiveDashboard::connect(&live);

    // 1. Cold Quick message (`Ctrl-o`) rejects `/compcat` and empty `/copy` locally,
    //    then transitions into full Operator with ReasoningPicker open on `/thinking`.
    ui.dashboard.ctrl('o');
    ui.dashboard.event_action(Event::Paste("/compcat".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let quick_err = ui.render();
    assert!(
        quick_err.contains("Unknown slash command \"/compcat\"") && quick_err.contains("/compcat"),
        "{quick_err}"
    );
    assert!(
        fake.log().is_empty(),
        "must not start Pi on unknown slash command"
    );

    ui.dashboard.ctrl('c');
    ui.dashboard.event_action(Event::Paste("/copy".into()));
    assert_eq!(ui.dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let quick_no_reply = ui.render();
    assert!(
        quick_no_reply.contains("No assistant reply to copy yet."),
        "{quick_no_reply}"
    );

    ui.dashboard.ctrl('c');
    ui.dashboard.event_action(Event::Paste("/thinking".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let opened_picker = ui.render();
    assert!(
        !opened_picker.contains("Quick message · Ctrl-o")
            && opened_picker.contains("default (Pi's choice)")
            && opened_picker.contains("Enter pick"),
        "Quick `/thinking` must open full Operator with ReasoningPicker:\n{opened_picker}"
    );
    assert!(
        fake.log().is_empty(),
        "`/thinking` picker must not start Pi"
    );
    ui.dashboard.key(KeyCode::Esc);

    // 2. Full Operator `/` menu shows all 7 registered commands (`/copy`, `/thinking`, `/model`, `/new`, `/session`, `/compact`, `/tree`) and
    //    dismisses cleanly on `Esc` without leaving Operator.
    ui.dashboard.event_action(Event::Paste("/".into()));
    let menu_screen = ui.render();
    assert!(
        menu_screen.contains("/copy")
            && menu_screen.contains("/thinking [level]")
            && menu_screen.contains("/model [provider/id | default]")
            && menu_screen.contains("/new")
            && menu_screen.contains("/session")
            && menu_screen.contains("/compact [instructions]")
            && menu_screen.contains("/tree"),
        "{menu_screen}"
    );
    assert_eq!(ui.dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    let menu_dismissed = ui.render();
    assert!(
        !menu_dismissed.contains("Copy latest assistant reply Markdown")
            && menu_dismissed.contains("> /▏"),
        "{menu_dismissed}"
    );
    ui.dashboard.ctrl('c');

    // 3. Multi-segment path `/etc/hosts` submits as a real prompt to `FakePi`.
    ui.dashboard
        .event_action(Event::Paste("/etc/hosts audit".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let conversation_id = ui.current_id();
    ui.wait_turn_settled(conversation_id);
    let after_turn_one = ui.render();
    assert!(
        after_turn_one.contains("Deployment Plan") && after_turn_one.contains("cargo test"),
        "{after_turn_one}"
    );

    // 4. `/copy` returns raw Markdown (`DashboardAction::CopyText`) from both the `/` menu
    //    and Quick message (`Ctrl-o`), and writes valid OSC 52 bytes via `write_clipboard`.
    ui.dashboard.event_action(Event::Paste("/co".into()));
    assert_eq!(
        ui.dashboard.key(KeyCode::Enter),
        DashboardAction::CopyText(raw_markdown.into())
    );
    let mut osc52 = Vec::new();
    write_clipboard(&mut osc52, raw_markdown).unwrap();
    let osc52_str = String::from_utf8(osc52).unwrap();
    assert_eq!(
        osc52_str,
        "\x1b]52;c;IyBEZXBsb3ltZW50IFBsYW4KCmBgYHNoCmNhcmdvIHRlc3QKYGBg\x1b\\"
    );

    ui.dashboard.key(KeyCode::Esc);
    ui.dashboard.ctrl('o');
    ui.dashboard.event_action(Event::Paste("/copy".into()));
    assert_eq!(
        ui.dashboard.key(KeyCode::Enter),
        DashboardAction::CopyText(raw_markdown.into())
    );

    // 5. Reopen Operator: model capabilities (`minimal, low, medium, high`) reject `/thinking max`
    //    locally while `/thinking HIGH` persists via real Server `SetSetting` and applies to the
    //    next turn, and leading-space ` /copy` submits as a normal prompt.
    ui.dashboard.key(KeyCode::Char('o'));
    ui.flush();
    ui.pump();

    ui.dashboard
        .event_action(Event::Paste("/thinking max".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let unsupported = ui.render();
    assert!(
        unsupported.contains("Unsupported thinking level \"max\"")
            && unsupported.contains("default, minimal, low, medium, high")
            && unsupported.contains("> /thinking max▏"),
        "{unsupported}"
    );

    ui.dashboard.ctrl('c');
    ui.dashboard
        .event_action(Event::Paste("/thinking HIGH".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let after_thinking = ui.render();
    assert!(
        after_thinking.contains("Set thinking level to high.")
            || after_thinking.contains("set: high"),
        "{after_thinking}"
    );

    let dashboard_toml = std::fs::read_to_string(live.config.join("dashboard.toml")).unwrap();
    assert!(
        dashboard_toml.contains("thinking_level = \"high\""),
        "{dashboard_toml}"
    );

    ui.dashboard
        .event_action(Event::Paste(" /copy literal prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let second_history = ui.wait_turn_settled(conversation_id);
    assert_eq!(
        second_history
            .conversation
            .reasoning
            .as_ref()
            .unwrap()
            .level,
        ovrcr::protocol::OperatorThinkingLevel::High
    );
    let prompts: Vec<_> = fake
        .log()
        .into_iter()
        .filter(|entry| entry["event"] == "prompt")
        .map(|entry| {
            let msg = entry["message"].as_str().unwrap_or_default();
            msg.rsplit_once("User request:\n")
                .map_or(msg, |(_, body)| body)
                .to_owned()
        })
        .collect();
    assert_eq!(
        prompts,
        vec!["/etc/hosts audit", " /copy literal prompt"],
        "multi-segment path and leading-space draft must both submit as prompts"
    );
}

#[test]
fn dashboard_slash_model_picker_and_switching_work_end_to_end() {
    use crossterm::event::{Event, KeyCode};
    use ovrcr::protocol::{
        self, ClientMessage, OperatorConversationId, OperatorHistory, OperatorModel,
        OperatorResponse, ServerMessage, Settings, TerminalSize,
    };
    use ovrcr::tui::Dashboard;
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use std::collections::VecDeque;

    struct LiveDashboard {
        stream: std::os::unix::net::UnixStream,
        dashboard: Dashboard,
        pending: VecDeque<ClientMessage>,
        next_probe_id: u64,
    }

    impl LiveDashboard {
        fn connect(live: &Live) -> Self {
            let mut stream = protocol::connect_server(&live.socket).unwrap();
            let budget = Duration::from_secs(5);
            stream.set_read_timeout(Some(budget)).unwrap();
            stream.set_write_timeout(Some(budget)).unwrap();
            let hierarchy =
                match protocol::client::request(&mut stream, 900_000, Request::DashboardHello)
                    .unwrap()
                {
                    Response::Hierarchy(hierarchy) => hierarchy,
                    other => panic!("Dashboard handshake: {other:?}"),
                };
            let mut dashboard = Dashboard::new(TerminalSize {
                rows: 40,
                cols: 160,
            });
            dashboard.install_hierarchy(hierarchy);
            dashboard.install_area(Rect::new(0, 0, 160, 40));
            dashboard.install_settings(Settings::default());
            Self {
                stream,
                dashboard,
                pending: VecDeque::new(),
                next_probe_id: 900_001,
            }
        }

        fn exchange(&mut self, request: Request) -> Response {
            self.next_probe_id += 1;
            let id = self.next_probe_id;
            protocol::write_frame(
                &mut self.stream,
                &ClientMessage {
                    request_id: id,
                    request,
                },
            )
            .unwrap();
            loop {
                let incoming: ServerMessage = protocol::read_frame(&mut self.stream).unwrap();
                let matched = match &incoming {
                    ServerMessage::Response {
                        request_id,
                        response,
                    } if *request_id == id => Some(response.clone()),
                    _ => None,
                };
                self.pending
                    .extend(self.dashboard.handle_server_message(incoming));
                self.pending.extend(self.dashboard.drain_outbox());
                if let Some(response) = matched {
                    self.flush();
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

        fn pump(&mut self) {
            self.stream
                .set_read_timeout(Some(Duration::from_millis(10)))
                .unwrap();
            while let Ok(message) = protocol::read_frame::<ServerMessage>(&mut self.stream) {
                self.pending
                    .extend(self.dashboard.handle_server_message(message));
            }
            self.stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            self.pending.extend(self.dashboard.drain_outbox());
            if !self.pending.is_empty() {
                self.flush();
            }
        }

        fn current_id(&mut self) -> OperatorConversationId {
            match self.exchange(Request::Operator(OperatorRequest::Current)) {
                Response::Operator(OperatorResponse::Current(Some(summary))) => summary.id,
                other => panic!("expected current conversation, got {other:?}"),
            }
        }

        fn wait_turn_running(&mut self, conversation: OperatorConversationId, expected_turn: u64) {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                self.pump();
                let history = match self.exchange(Request::Operator(OperatorRequest::Read {
                    conversation,
                    after: 0,
                    limit: 100,
                })) {
                    Response::Operator(OperatorResponse::History(history)) => history,
                    other => panic!("expected history, got {other:?}"),
                };
                if history.conversation.current_turn.as_ref().is_some_and(|t| {
                    t.identity.turn.0 == expected_turn && t.state == OperatorTurnState::Running
                }) {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for running turn {expected_turn}: {history:#?}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn wait_turn_settled(&mut self, conversation: OperatorConversationId) -> OperatorHistory {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                self.pump();
                let history = match self.exchange(Request::Operator(OperatorRequest::Read {
                    conversation,
                    after: 0,
                    limit: 100,
                })) {
                    Response::Operator(OperatorResponse::History(history)) => history,
                    other => panic!("expected history, got {other:?}"),
                };
                if history
                    .conversation
                    .current_turn
                    .as_ref()
                    .is_some_and(|t| t.state == OperatorTurnState::Complete)
                    && history.conversation.worker_stopped
                {
                    self.pump();
                    return history;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for settled turn: {history:#?}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn render(&mut self) -> String {
            let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
            terminal
                .draw(|frame| ovrcr::tui::draw_dashboard(frame, &self.dashboard))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..40)
                .map(|y| {
                    (0..160)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    let hold_gate = tempfile::tempdir().unwrap();
    let release_turn_two = hold_gate.path().join("release-turn-two");

    let mut scenario = Scenario::new();
    scenario
        .default_model("fixture", "default")
        .models(vec![
            ("fixture", "default", "Fixture Default"),
            ("anthropic", "claude-sonnet-4-6", "Claude Sonnet 4.6"),
            ("openrouter", "vendor/sub/id", "Nested Router Model"),
        ])
        .prompt_steps(vec![
            vec![Step::assistant("Reply 1 on sonnet"), Step::Settle],
            vec![
                Step::Start,
                Step::Delta("Streaming turn 2...".into()),
                Step::WaitFile(release_turn_two.clone()),
                Step::assistant("Reply 2 finished cleanly"),
                Step::Settle,
            ],
            vec![Step::assistant("Reply 3 on nested router"), Step::Settle],
            vec![Step::assistant("Reply 4 on harness default"), Step::Settle],
        ]);

    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    fake.script(&scenario);

    // Populate a conflicting ambient HOME/.pi catalog to prove the live Server + TUI
    // only ever inspect `<OVRCR_HOME>/operator-pi`.
    let ambient_home = live.root.path().join("ambient-home");
    std::fs::create_dir_all(ambient_home.join(".pi")).unwrap();
    std::fs::write(
        ambient_home.join(".pi/fake-pi-scenario.json"),
        serde_json::to_vec(&json!({
            "models": [
                {"provider": "ambient-leak", "id": "forbidden", "name": "Ambient Leak"}
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    live.start_binary_env(&[
        fake_pi::executable_env(&fake),
        ("HOME", ambient_home.as_os_str()),
    ]);
    let mut ui = LiveDashboard::connect(&live);

    // 1. Cold `Ctrl-o` `/model sonnet` discovers models from the dedicated `operator-pi` profile
    //    without starting an RPC turn, ignores `ambient-leak/forbidden`, transitions into full
    //    Operator, and opens `ModelPicker` pre-filtered by `sonnet`.
    ui.dashboard.ctrl('o');
    ui.dashboard
        .event_action(Event::Paste("/model sonnet".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();

    let prefiltered = ui.render();
    assert!(
        !prefiltered.contains("Quick message · Ctrl-o")
            && prefiltered.contains("Filter: sonnet▏")
            && prefiltered.contains("anthropic/claude-sonnet-4-6 · Claude Sonnet 4.6")
            && !prefiltered.contains("ambient-leak"),
        "expected dedicated profile model picker pre-filtered by sonnet:\n{prefiltered}"
    );
    assert!(
        !fake.log().iter().any(|entry| entry["event"] == "prompt"),
        "model discovery must not submit an Operator turn: {:?}",
        fake.log()
    );

    // Pick `anthropic/claude-sonnet-4-6` via Enter -> persists `operator.model` in dashboard.toml
    // and applies to Turn 1.
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let toml_after_pick = std::fs::read_to_string(live.config.join("dashboard.toml")).unwrap();
    assert!(
        toml_after_pick.contains("model = \"anthropic/claude-sonnet-4-6\""),
        "{toml_after_pick}"
    );

    ui.dashboard
        .event_action(Event::Paste("first turn prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let conversation_id = ui.current_id();
    let turn_one = ui.wait_turn_settled(conversation_id);
    assert_eq!(
        turn_one.conversation.model,
        Some(OperatorModel {
            provider: "anthropic".into(),
            id: "claude-sonnet-4-6".into(),
        })
    );

    // 2. Start Turn 2 (held mid-flight) and run exact `/model openrouter/vendor/sub/id` DURING
    //    the running turn: persists immediately without disrupting Turn 2, and takes effect on Turn 3.
    ui.dashboard
        .event_action(Event::Paste("second turn held".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.wait_turn_running(conversation_id, 2);

    ui.dashboard
        .event_action(Event::Paste("/model openrouter/vendor/sub/id".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let mid_turn_screen = ui.render();
    assert!(
        !mid_turn_screen.contains("Filter:")
            && mid_turn_screen.contains("Set Operator model to openrouter/vendor/sub/id."),
        "exact `/model <provider/id>` writes setting immediately without opening picker:\n{mid_turn_screen}"
    );

    std::fs::write(&release_turn_two, "go").unwrap();
    let turn_two = ui.wait_turn_settled(conversation_id);
    assert_eq!(
        turn_two.conversation.model,
        Some(OperatorModel {
            provider: "anthropic".into(),
            id: "claude-sonnet-4-6".into(),
        }),
        "in-flight Turn 2 finishes on its already-selected model without disruption"
    );

    ui.dashboard
        .event_action(Event::Paste("third turn prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let turn_three = ui.wait_turn_settled(conversation_id);
    assert_eq!(
        turn_three.conversation.model,
        Some(OperatorModel {
            provider: "openrouter".into(),
            id: "vendor/sub/id".into(),
        }),
        "Turn 3 picks up the model switched during Turn 2"
    );

    // 3. `/model default` from Quick message (`Ctrl-o`) resets `operator.model` to `None`
    //    without opening a picker, and Turn 4 uses the dedicated Pi profile's default (`fixture/default`).
    ui.dashboard.key(KeyCode::Esc);
    ui.dashboard.ctrl('o');
    ui.dashboard
        .event_action(Event::Paste("/model default".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let after_quick_default = ui.render();
    assert!(
        !after_quick_default.contains("Quick message · Ctrl-o"),
        "Quick message closes on `/model default`:\n{after_quick_default}"
    );
    let toml_after_default = std::fs::read_to_string(live.config.join("dashboard.toml")).unwrap();
    assert!(
        !toml_after_default.contains("openrouter/vendor/sub/id"),
        "`/model default` clears explicit operator.model from dashboard.toml:\n{toml_after_default}"
    );

    ui.dashboard.key(KeyCode::Char('o'));
    ui.flush();
    ui.dashboard
        .event_action(Event::Paste("fourth turn prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let turn_four = ui.wait_turn_settled(conversation_id);
    assert_eq!(
        turn_four.conversation.model,
        Some(OperatorModel {
            provider: "fixture".into(),
            id: "default".into(),
        }),
        "Turn 4 delegates to dedicated Pi profile default (`Harness default`)"
    );

    // 4. Empty dedicated catalog surfaces clear `ovrcr operator setup` guidance while offering Harness default.
    let mut empty_catalog = Scenario::new();
    empty_catalog.models(Vec::new());
    fake.script(&empty_catalog);
    ui.dashboard.event_action(Event::Paste("/model".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let empty_screen = ui.render();
    assert!(
        empty_screen.contains("✓ Harness default") && empty_screen.contains("ovrcr operator setup"),
        "empty catalog offers Harness default and setup guidance:\n{empty_screen}"
    );
}

struct LiveDashboard {
    stream: std::os::unix::net::UnixStream,
    dashboard: ovrcr::tui::Dashboard,
    pending: std::collections::VecDeque<ovrcr::protocol::ClientMessage>,
    next_probe_id: u64,
}

impl LiveDashboard {
    fn connect(live: &Live) -> Self {
        use ovrcr::protocol::{self, Settings, TerminalSize};
        use ovrcr::tui::Dashboard;
        use ratatui::layout::Rect;
        use std::collections::VecDeque;

        let mut stream = protocol::connect_server(&live.socket).unwrap();
        let budget = Duration::from_secs(5);
        stream.set_read_timeout(Some(budget)).unwrap();
        stream.set_write_timeout(Some(budget)).unwrap();
        let hierarchy =
            match protocol::client::request(&mut stream, 910_000, Request::DashboardHello).unwrap()
            {
                Response::Hierarchy(hierarchy) => hierarchy,
                other => panic!("Dashboard handshake: {other:?}"),
            };
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 40,
            cols: 160,
        });
        dashboard.install_hierarchy(hierarchy);
        dashboard.install_area(Rect::new(0, 0, 160, 40));
        dashboard.install_settings(Settings::default());
        Self {
            stream,
            dashboard,
            pending: VecDeque::new(),
            next_probe_id: 910_001,
        }
    }

    fn exchange(&mut self, request: Request) -> Response {
        use ovrcr::protocol::{self, ClientMessage, ServerMessage};

        self.next_probe_id += 1;
        let id = self.next_probe_id;
        protocol::write_frame(
            &mut self.stream,
            &ClientMessage {
                request_id: id,
                request,
            },
        )
        .unwrap();
        loop {
            let incoming: ServerMessage = protocol::read_frame(&mut self.stream).unwrap();
            let matched = match &incoming {
                ServerMessage::Response {
                    request_id,
                    response,
                } if *request_id == id => Some(response.clone()),
                _ => None,
            };
            self.pending
                .extend(self.dashboard.handle_server_message(incoming));
            self.pending.extend(self.dashboard.drain_outbox());
            if let Some(response) = matched {
                self.flush();
                return response;
            }
        }
    }

    fn flush(&mut self) {
        use ovrcr::protocol::{self, ServerMessage};

        for _ in 0..200 {
            self.pending.extend(self.dashboard.drain_outbox());
            if self.pending.is_empty() {
                return;
            }
            while let Some(message) = self.pending.pop_front() {
                let id = message.request_id;
                protocol::write_frame(&mut self.stream, &message).unwrap();
                loop {
                    let incoming: ServerMessage = protocol::read_frame(&mut self.stream).unwrap();
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

    fn pump(&mut self) {
        use ovrcr::protocol::{self, ServerMessage};

        self.stream
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        while let Ok(message) = protocol::read_frame::<ServerMessage>(&mut self.stream) {
            self.pending
                .extend(self.dashboard.handle_server_message(message));
        }
        self.stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        self.pending.extend(self.dashboard.drain_outbox());
        if !self.pending.is_empty() {
            self.flush();
        }
    }

    fn wait_history(
        &mut self,
        conversation: ovrcr::protocol::OperatorConversationId,
        predicate: impl Fn(&ovrcr::protocol::OperatorHistory) -> bool,
    ) -> ovrcr::protocol::OperatorHistory {
        use ovrcr::protocol::OperatorResponse;

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            self.pump();
            let history = match self.exchange(Request::Operator(OperatorRequest::Read {
                conversation,
                after: 0,
                limit: 100,
            })) {
                Response::Operator(OperatorResponse::History(history)) => history,
                other => panic!("expected history, got {other:?}"),
            };
            if predicate(&history) {
                self.pump();
                return history;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for history predicate: {history:#?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn render(&mut self) -> String {
        use ratatui::{Terminal, backend::TestBackend};

        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard(frame, &self.dashboard))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..40)
            .map(|y| {
                (0..160)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn dashboard_slash_new_and_zero_spawn_session_work_end_to_end() {
    use crossterm::event::{Event, KeyCode};
    use ovrcr::protocol::{ErrorCode, OperatorResponse};
    use ovrcr::tui::DashboardAction;
    use std::os::unix::fs::PermissionsExt;

    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    let release_turn_one = live.root.path().join("release-turn-one");

    let models_json = fake.profile.join("models.json");
    std::fs::write(
        &models_json,
        serde_json::to_vec_pretty(&json!({
            "providers": {
                "fixture": {
                    "apiKey": "SECRET_DO_NOT_LEAK_487",
                    "models": [
                        {
                            "id": "default",
                            "name": "Fixture Default",
                            "contextWindow": 100_000
                        }
                    ]
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&models_json, std::fs::Permissions::from_mode(0o600)).unwrap();

    let mut scenario = Scenario::new();
    scenario.set("record_tool_history", json!(true));
    scenario.set(
        "assistant_usage",
        json!({
            "input": 800,
            "output": 150,
            "cacheRead": 200,
            "cacheWrite": 100,
            "totalTokens": 1250,
            "cost": { "total": 0.0125 }
        }),
    );
    scenario.prompt_steps(vec![
        vec![
            Step::WaitFile(release_turn_one.clone()),
            Step::WatchStatus(1),
            Step::tool("call-1", "read", json!({"path": "README.md"}), "ok"),
            Step::assistant("Epoch zero reply."),
            Step::Settle,
        ],
        vec![
            Step::WatchStatus(0),
            Step::assistant("Watch complete."),
            Step::Settle,
        ],
        vec![Step::assistant("Epoch one fresh reply."), Step::Settle],
    ]);
    fake.script(&scenario);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let mut ui = LiveDashboard::connect(&live);

    // 1. Cold `/session` from `Ctrl-o` Quick message transitions into full Operator,
    //    renders empty epoch-0 stats, never leaks `models.json` credentials, and spawns zero Pi processes.
    ui.dashboard.ctrl('o');
    ui.dashboard.event_action(Event::Paste("/session".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let cold_modal = ui.render();
    assert!(
        !cold_modal.contains("Quick message · Ctrl-o")
            && cold_modal.contains("Operator session · /session")
            && cold_modal.contains("epoch 0 · pi none")
            && cold_modal.contains("0 total (in 0 · out 0)")
            && cold_modal.contains("$0.0000")
            && cold_modal.contains("none yet")
            && !cold_modal.contains("SECRET_DO_NOT_LEAK_487"),
        "cold `/session` modal unexpected:\n{cold_modal}"
    );
    assert!(fake.log().is_empty(), "cold `/session` must never spawn Pi");
    assert_eq!(
        ui.dashboard.key(KeyCode::Char('q')),
        DashboardAction::Redraw
    );
    assert!(!ui.render().contains("Operator session · /session"));

    let conversation_id = match ui.exchange(Request::Operator(OperatorRequest::Current)) {
        Response::Operator(OperatorResponse::Current(Some(summary))) => summary.id,
        other => panic!("expected current conversation, got {other:?}"),
    };

    // 2. Start turn 1 and verify `/new` refuses while the turn is `Running`.
    ui.dashboard
        .event_action(Event::Paste("Inspect workspace".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let running_history = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Running)
    });
    ui.dashboard.event_action(Event::Paste("/new".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let busy_screen = ui.render();
    assert!(
        busy_screen.contains("Cannot start a new Operator session while a turn is running."),
        "{busy_screen}"
    );
    ui.dashboard.ctrl('c');
    match ui.exchange(Request::Operator(OperatorRequest::NewSession {
        conversation: conversation_id,
        expected_run: running_history.conversation.run,
    })) {
        Response::Error {
            code: ErrorCode::Conflict,
            message,
        } => assert!(
            message.contains("running") || message.contains("active"),
            "{message}"
        ),
        other => panic!("expected Conflict while turn is running, got {other:?}"),
    }

    // 3. Complete turn 1 under an active `watch_sessions` warm-worker lease and verify `/new` refuses.
    std::fs::write(&release_turn_one, b"go\n").unwrap();
    let leased_history = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Complete)
            && !history.conversation.worker_stopped
    });
    ui.dashboard.event_action(Event::Paste("/new".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let leased_screen = ui.render();
    assert!(
        leased_screen.contains(
            "Cannot start a new Operator session while a watched-session lease or worker is active."
        ),
        "{leased_screen}"
    );
    ui.dashboard.ctrl('c');
    match ui.exchange(Request::Operator(OperatorRequest::NewSession {
        conversation: conversation_id,
        expected_run: leased_history.conversation.run,
    })) {
        Response::Error {
            code: ErrorCode::Conflict,
            message,
        } => assert!(
            message.contains("lease") || message.contains("active"),
            "{message}"
        ),
        other => panic!("expected Conflict while watch lease is active, got {other:?}"),
    }

    // 4. Finish the watched turn on the warm worker (`WatchStatus(0)`) so it retires cleanly,
    //    and verify zero-spawn `/session` reads the promoted `session.jsonl` + `models.json`
    //    contextWindow without spawning Pi.
    let turn_one_id = leased_history
        .conversation
        .current_turn
        .as_ref()
        .expect("leased turn")
        .identity
        .turn
        .0;
    let turn_one_checkpoint = fake
        .profile
        .join("turns")
        .join(turn_one_id.to_string())
        .join("session.jsonl");
    assert!(
        turn_one_checkpoint.is_file(),
        "leased turn checkpoint missing at {}",
        turn_one_checkpoint.display()
    );
    ui.dashboard
        .event_action(Event::Paste("Finish watch".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let settled_history = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity.turn.0 != turn_one_id && turn.state == OperatorTurnState::Complete
            })
            && history.conversation.worker_stopped
    });
    let turn_two_id = settled_history
        .conversation
        .current_turn
        .as_ref()
        .expect("completed turn")
        .identity
        .turn
        .0;
    let checkpoint_path = fake
        .profile
        .join("turns")
        .join(turn_two_id.to_string())
        .join("session.jsonl");
    assert!(
        !turn_one_checkpoint.exists() && checkpoint_path.is_file(),
        "expected turn 2 checkpoint promoted and turn 1 superseded: {} / {}",
        turn_one_checkpoint.display(),
        checkpoint_path.display()
    );

    let starts_before_session = fake
        .log()
        .iter()
        .filter(|entry| entry["event"] == "start")
        .count();
    assert_eq!(starts_before_session, 1);

    ui.dashboard.event_action(Event::Paste("/session".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let populated_modal = ui.render();
    assert!(
        populated_modal.contains("Operator session · /session")
            && populated_modal.contains(&format!("epoch 0 · pi operator-{}", conversation_id.0))
            && populated_modal.contains("fixture/default")
            && populated_modal.contains("medium")
            && populated_modal.contains("2 total (2 completed)")
            && populated_modal.contains("1 calls · 1 results")
            && populated_modal.contains("2,500 total (in 1,600 · out 300)")
            && populated_modal.contains("read 400 · write 200")
            && populated_modal.contains("$0.0250")
            && populated_modal.contains("1,250 / 100,000 tokens (1.2%)")
            && !populated_modal.contains("SECRET_DO_NOT_LEAK_487"),
        "populated `/session` modal unexpected:\n{populated_modal}"
    );
    let starts_after_session = fake
        .log()
        .iter()
        .filter(|entry| entry["event"] == "start")
        .count();
    assert_eq!(
        starts_after_session, starts_before_session,
        "`/session` must never spawn Pi"
    );
    assert_eq!(ui.dashboard.key(KeyCode::Esc), DashboardAction::Redraw);

    // 5. `/new` advances `session_epoch` to 1, clears visible feed + `/copy`, deletes all
    //    superseded prior-epoch checkpoint files on disk, and preserves prior-epoch SQLite audit rows.
    ui.dashboard.event_action(Event::Paste("/new".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let reset_screen = ui.render();
    assert!(
        !reset_screen.contains("Epoch zero reply.")
            && !reset_screen.contains("Watch complete.")
            && reset_screen.contains("Started new Operator session (epoch 1)."),
        "{reset_screen}"
    );
    assert!(
        !turn_one_checkpoint.exists() && !checkpoint_path.exists(),
        "superseded prior-epoch checkpoints should be removed: {} / {}",
        turn_one_checkpoint.display(),
        checkpoint_path.display()
    );

    ui.dashboard.event_action(Event::Paste("/copy".into()));
    assert_eq!(ui.dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(
        ui.render().contains("No assistant reply to copy yet."),
        "prior-epoch reply must not remain copyable after `/new`"
    );

    let registry = Registry::of(&live);
    assert_eq!(registry.checkpoint(conversation_id.0), None);
    let db = rusqlite::Connection::open(&registry.path).unwrap();
    let conv_epoch: i64 = db
        .query_row(
            "SELECT session_epoch FROM operator_conversations WHERE id = ?1",
            [conversation_id.0 as i64],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(conv_epoch, 1);
    let (prior_turn_epoch, prior_turn_state, prior_msg_count): (i64, String, i64) = db
        .query_row(
            "SELECT t.session_epoch, t.state,
                    (SELECT COUNT(*) FROM operator_messages m WHERE m.turn = t.id)
               FROM operator_turns t WHERE t.id = ?1",
            [turn_one_id as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(prior_turn_epoch, 0);
    assert_eq!(prior_turn_state, "Complete");
    assert!(
        prior_msg_count >= 2,
        "epoch-0 messages must remain preserved"
    );
    drop(db);

    // `/session` on the reset epoch reports zeroed stats and `pi none` without spawning Pi.
    ui.dashboard.event_action(Event::Paste("/session".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let epoch_one_modal = ui.render();
    assert!(
        epoch_one_modal.contains("epoch 1 · pi none")
            && epoch_one_modal.contains("0 total (in 0 · out 0)")
            && epoch_one_modal.contains("$0.0000")
            && epoch_one_modal.contains("none yet"),
        "epoch-1 `/session` modal unexpected:\n{epoch_one_modal}"
    );
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["event"] == "start")
            .count(),
        starts_before_session,
        "`/session` after `/new` must never spawn Pi"
    );
    assert_eq!(ui.dashboard.key(KeyCode::Enter), DashboardAction::Redraw);

    // 6. Submitting a prompt in epoch 1 starts cold with fresh model context (zero `switch_session`).
    let mut epoch_one_scenario = Scenario::new();
    epoch_one_scenario.steps(vec![
        Step::assistant("Epoch one fresh reply."),
        Step::Settle,
    ]);
    fake.script(&epoch_one_scenario);
    ui.dashboard
        .event_action(Event::Paste("Fresh epoch one prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let epoch_one_history = ui.wait_history(conversation_id, |history| {
        history.conversation.session_epoch == 1
            && history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.state == OperatorTurnState::Complete)
            && history.conversation.worker_stopped
    });
    assert!(has_assistant(&epoch_one_history, "Epoch one fresh reply."));
    assert!(
        !has_assistant(&epoch_one_history, "Epoch zero reply."),
        "epoch-1 history must not surface epoch-0 messages"
    );
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["event"] == "switch")
            .count(),
        0,
        "epoch-1 cold turn must not switch_session to epoch-0 checkpoint"
    );
}

#[test]
fn dashboard_slash_compact_and_stop_rollback_work_end_to_end() {
    use crossterm::event::{Event, KeyCode};
    use ovrcr::protocol::OperatorResponse;
    use ovrcr::tui::DashboardAction;
    use std::os::unix::fs::PermissionsExt;

    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    let models_json = fake.profile.join("models.json");
    std::fs::write(
        &models_json,
        serde_json::to_vec_pretty(&json!({
            "providers": {
                "fixture": {
                    "apiKey": "SECRET_DO_NOT_LEAK_488",
                    "models": [
                        {
                            "id": "default",
                            "name": "Fixture Default",
                            "contextWindow": 100_000
                        }
                    ]
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&models_json, std::fs::Permissions::from_mode(0o600)).unwrap();

    let mut scenario = Scenario::new();
    scenario.set("compact_modes", json!(["hold", "ok"]));
    scenario.set("compact_tokens_before", json!(4_200));
    scenario.set("compact_summary", json!("Compacted context summary."));
    scenario.set(
        "assistant_usage",
        json!({
            "input": 800,
            "output": 150,
            "cacheRead": 200,
            "cacheWrite": 100,
            "totalTokens": 1_250,
            "cost": { "total": 0.0125 }
        }),
    );
    scenario.prompt_steps(vec![
        vec![
            Step::WatchStatus(1),
            Step::assistant("Pre-compact warm reply."),
            Step::Settle,
        ],
        vec![
            Step::WatchStatus(0),
            Step::assistant("Post-compact follow-up reply."),
            Step::Settle,
        ],
    ]);
    fake.script(&scenario);
    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let mut ui = LiveDashboard::connect(&live);

    // 1. Cold `/compact` before any completed turn refuses without spawning Pi.
    ui.dashboard.key(KeyCode::Char('o'));
    ui.flush();
    ui.dashboard.event_action(Event::Paste("/compact".into()));
    assert_eq!(ui.dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let cold_refused = ui.render();
    assert!(
        cold_refused.contains(
            "Cannot compact Operator context before the first completed turn in this session."
        ),
        "{cold_refused}"
    );
    assert!(
        fake.log().is_empty(),
        "empty-epoch `/compact` must never spawn Pi"
    );
    ui.dashboard.ctrl('c');

    let conversation_id = match ui.exchange(Request::Operator(OperatorRequest::Current)) {
        Response::Operator(OperatorResponse::Current(Some(summary))) => summary.id,
        other => panic!("expected current conversation, got {other:?}"),
    };

    // 2. Seed turn 1 under an active `watch_sessions` warm-worker lease (`WatchStatus(1)`).
    ui.dashboard
        .event_action(Event::Paste("Seed warm session".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let seeded_history = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Complete)
            && history.conversation.worker_live
            && !history.conversation.worker_stopped
    });
    let warm_run = seeded_history.conversation.run;
    let registry = Registry::of(&live);
    let seed_turn_id = registry
        .checkpoint(conversation_id.0)
        .expect("seed checkpoint");

    // 3. Dispatch `/compact cancel me` from `Ctrl-o` Quick message (`compact: "hold"`),
    //    observe live `"Compacting context…"`, then press `Ctrl-x` (`Stop`) and verify rollback.
    ui.dashboard.key(KeyCode::Esc);
    ui.flush();
    ui.dashboard.ctrl('o');
    ui.dashboard
        .event_action(Event::Paste("/compact cancel me".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| {
                turn.identity.turn.0 != seed_turn_id as u64
                    && turn.state == OperatorTurnState::Running
            })
            && history.messages.iter().any(|message| {
                matches!(
                    message.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted)
                )
            })
    });
    let running_screen = ui.render();
    assert!(
        !running_screen.contains("Quick message · Ctrl-o")
            && running_screen.contains("Compact context · cancel me")
            && running_screen.contains("Compacting context…"),
        "running `/compact` screen unexpected:\n{running_screen}"
    );

    ui.dashboard.ctrl('x');
    ui.flush();
    let rolled_back_history = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Interrupted)
            && history.conversation.worker_live
            && !history.conversation.worker_stopped
    });
    assert_eq!(rolled_back_history.conversation.run, warm_run);
    assert_eq!(
        registry.checkpoint(conversation_id.0),
        Some(seed_turn_id),
        "Stop during `/compact` must preserve pre-compaction checkpoint"
    );
    let cancelled_screen = ui.render();
    assert!(
        cancelled_screen
            .contains("Interrupted · compaction cancelled; previous context preserved."),
        "{cancelled_screen}"
    );

    // 4. Run `/compact focus on rust tests` (`compact_modes[1] == "ok"`) on the same warm worker.
    ui.dashboard
        .event_action(Event::Paste("/compact focus on rust tests".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let compacted_history = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Complete)
            && history.messages.iter().any(|message| {
                matches!(
                    message.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionSummary { .. })
                )
            })
    });
    assert_eq!(compacted_history.conversation.run, warm_run);
    assert!(
        compacted_history.conversation.worker_live
            && !compacted_history.conversation.worker_stopped,
        "warm watch lease must survive completed `/compact`"
    );
    let compacted_turn_id = registry
        .checkpoint(conversation_id.0)
        .expect("promoted compacted checkpoint");
    assert_ne!(compacted_turn_id, seed_turn_id);
    let receipt_screen = ui.render();
    assert!(
        receipt_screen.contains("Compact context · focus on rust tests")
            && receipt_screen.contains("Compacted from 4,200 tokens")
            && receipt_screen.contains("Compacted context summary."),
        "completed `/compact` receipt unexpected:\n{receipt_screen}"
    );

    // `/copy` still copies the prior assistant reply, never the compaction summary.
    ui.dashboard.event_action(Event::Paste("/copy".into()));
    match ui.dashboard.key(KeyCode::Enter) {
        DashboardAction::CopyText(text) => assert_eq!(text, "Pre-compact warm reply."),
        other => panic!("expected CopyText of assistant reply after `/compact`, got {other:?}"),
    }

    // Zero-spawn `/session` reads the compacted checkpoint cleanly without spawning another Pi.
    let starts_before_session = fake
        .log()
        .iter()
        .filter(|entry| entry["event"] == "start")
        .count();
    assert_eq!(starts_before_session, 1);
    ui.dashboard.event_action(Event::Paste("/session".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let session_modal = ui.render();
    assert!(
        session_modal.contains("Operator session · /session")
            && session_modal.contains("checkpoint turn #3")
            && session_modal.contains("3 total (2 completed)")
            && session_modal.contains("1,630 total (in 1,100 · out 230)")
            && session_modal.contains("0 / 100,000 tokens (0.0%)")
            && !session_modal.contains("SECRET_DO_NOT_LEAK_488"),
        "compacted `/session` modal unexpected:\n{session_modal}"
    );
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["event"] == "start")
            .count(),
        1,
        "`/session` must never spawn Pi"
    );
    assert_eq!(ui.dashboard.key(KeyCode::Esc), DashboardAction::Redraw);

    // 5. Follow-up prompt on the warm worker succeeds on the compacted session and releases the lease.
    ui.dashboard
        .event_action(Event::Paste("Finish watch after compact".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let settled_history = ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Post-compact follow-up reply.")
            && history.conversation.worker_stopped
    });
    assert_eq!(settled_history.conversation.run, warm_run);

    // 6. `/new` advances `session_epoch`, after which `/compact` refuses again without spawning Pi.
    ui.dashboard.event_action(Event::Paste("/new".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    ui.dashboard.event_action(Event::Paste("/compact".into()));
    assert_eq!(ui.dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(
        ui.render().contains(
            "Cannot compact Operator context before the first completed turn in this session."
        ),
        "post-`/new` `/compact` must refuse cleanly"
    );
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["event"] == "start")
            .count(),
        1,
        "post-`/new` refused `/compact` must not spawn Pi"
    );
}

#[test]
fn dashboard_slash_tree_rewinds_branches_switches_and_restores_cold_leaf_end_to_end() {
    use crossterm::event::{Event, KeyCode};
    use ovrcr::protocol::{ErrorCode, OperatorResponse};
    use ovrcr::tui::DashboardAction;

    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    let release_turn_four = live.root.path().join("release-turn-four");

    let mut s1 = Scenario::new();
    s1.steps(vec![Step::assistant("Root reply one."), Step::Settle]);
    fake.script(&s1);

    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let mut ui = LiveDashboard::connect(&live);

    // 1. Cold `Ctrl-o` Quick message `/tree` transitions into full Operator and reports empty state
    //    without spawning `pi`.
    ui.dashboard.ctrl('o');
    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let cold_tree = ui.render();
    assert!(
        !cold_tree.contains("Quick message · Ctrl-o")
            && cold_tree.contains("Operator tree · /tree")
            && cold_tree.contains("No turns in this Operator session yet."),
        "cold `/tree` unexpected:\n{cold_tree}"
    );
    assert!(fake.log().is_empty(), "cold `/tree` must never spawn Pi");
    assert_eq!(ui.dashboard.key(KeyCode::Esc), DashboardAction::Redraw);

    let conversation_id = match ui.exchange(Request::Operator(OperatorRequest::Current)) {
        Response::Operator(OperatorResponse::Current(Some(summary))) => summary.id,
        other => panic!("expected current conversation, got {other:?}"),
    };

    // 2. Run Turn 1 (`Seed root turn`) and Turn 2 (`Branch alpha prompt`).
    ui.dashboard
        .event_action(Event::Paste("Seed root turn".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Root reply one.") && history.conversation.worker_stopped
    });

    let mut s2 = Scenario::new();
    s2.steps(vec![Step::assistant("Branch alpha reply."), Step::Settle]);
    fake.script(&s2);

    ui.dashboard
        .event_action(Event::Paste("Branch alpha prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let alpha_history = ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Branch alpha reply.") && history.conversation.worker_stopped
    });
    assert!(has_assistant(&alpha_history, "Root reply one."));
    assert!(has_assistant(&alpha_history, "Branch alpha reply."));

    let starts_after_alpha = fake
        .log()
        .iter()
        .filter(|entry| entry["event"] == "start")
        .count();
    assert_eq!(starts_after_alpha, 2);

    // 3. Open `/tree` (zero Pi spawns!), verify unwrapped user prompts, select `Branch alpha prompt`
    //    (`Up` from active leaf `Branch alpha reply.`), and press `Enter` to rewind!
    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let linear_tree = ui.render();
    assert!(
        linear_tree.contains("● user: Seed root turn")
            && linear_tree.contains("● assistant: Root reply one.")
            && linear_tree.contains("● user: Branch alpha prompt")
            && linear_tree.contains("● assistant: Branch alpha reply. ← active")
            && !linear_tree.contains("Captured OVRCR context"),
        "expected clean unwrapped prompts in `/tree`:\n{linear_tree}"
    );
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["event"] == "start")
            .count(),
        starts_after_alpha,
        "`/tree` inspection must never spawn Pi"
    );

    ui.dashboard.key(KeyCode::Up);
    ui.dashboard.key(KeyCode::Enter);
    assert!(
        ui.render().contains("Summarize branch? · /tree"),
        "leaving Branch A tip must offer Summarize branch? modal"
    );
    // Confirm default `No summary` choice.
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["event"] == "start")
            .count(),
        starts_after_alpha,
        "zero-token `/tree` rewind must never spawn Pi"
    );

    let rewound_screen = ui.render();
    assert!(
        !rewound_screen.contains("Operator tree · /tree")
            && rewound_screen.contains("Root reply one.")
            && !rewound_screen.contains("Branch alpha reply.")
            && rewound_screen.contains("> Branch alpha prompt▏"),
        "user rewind must project Turn 1 feed and restore unwrapped prompt to composer:\n{rewound_screen}"
    );

    // `/copy` follows the rewound active branch (`Root reply one.`), not `Branch alpha reply.`.
    ui.dashboard.ctrl('c');
    ui.dashboard.event_action(Event::Paste("/copy".into()));
    match ui.dashboard.key(KeyCode::Enter) {
        DashboardAction::CopyText(text) => assert_eq!(text, "Root reply one."),
        other => panic!("expected CopyText of rewound branch tip, got {other:?}"),
    }

    // 4. Submit Turn 3 (`Branch beta prompt`) on the rewound branch: cold worker restores exact leaf
    //    via `/ovrcr_tree_control` (`summarize: false`) before prompting, creating sibling Branch B!
    let mut s3 = Scenario::new();
    s3.steps(vec![Step::assistant("Branch beta reply."), Step::Settle]);
    fake.script(&s3);

    ui.dashboard
        .event_action(Event::Paste("Branch beta prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let beta_history = ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Branch beta reply.") && history.conversation.worker_stopped
    });
    assert!(has_assistant(&beta_history, "Root reply one."));
    assert!(has_assistant(&beta_history, "Branch beta reply."));
    assert!(
        !has_assistant(&beta_history, "Branch alpha reply."),
        "active Branch B feed must exclude sibling Branch A"
    );
    assert!(
        fake.log()
            .iter()
            .any(|entry| { entry["event"] == "tree_control" && entry["summarize"] == false }),
        "expected cold worker to execute authorized tree_control leaf restoration: {:?}",
        fake.log()
    );

    // 5. Open `/tree` and verify both diverging branches (`├─` / `└─` / `│`) are preserved;
    //    switch to Branch A (`Branch alpha reply.`), inspect `/session` branch vs epoch counts,
    //    then switch back to Branch B (`Branch beta reply.`) — all with zero Pi spawns!
    let starts_after_beta = fake
        .log()
        .iter()
        .filter(|entry| entry["event"] == "start")
        .count();
    assert_eq!(starts_after_beta, 3);

    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let branched_tree = ui.render();
    for expected in [
        "● user: Seed root turn",
        "● assistant: Root reply one.",
        "├─   user: Branch alpha prompt",
        "│    assistant: Branch alpha reply.",
        "└─ ● user: Branch beta prompt",
        "● assistant: Branch beta reply. ← active",
    ] {
        assert!(
            branched_tree.contains(expected),
            "expected `{expected}` in branched `/tree`:\n{branched_tree}"
        );
    }

    ui.dashboard
        .event_action(Event::Paste("alpha assistant".into()));
    ui.dashboard.key(KeyCode::Enter);
    assert!(ui.render().contains("Summarize branch? · /tree"));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let on_alpha = ui.render();
    assert!(
        on_alpha.contains("Branch alpha reply.")
            && !on_alpha.contains("Branch beta reply.")
            && on_alpha.contains("> ▏"),
        "switching to assistant leaf `Branch alpha reply.` must update feed with empty composer:\n{on_alpha}"
    );

    ui.dashboard.event_action(Event::Paste("/session".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let session_on_alpha = ui.render();
    assert!(
        session_on_alpha.contains("2 on branch · 3 total (3 completed)"),
        "expected active-branch vs epoch turn counts in `/session`:\n{session_on_alpha}"
    );
    assert_eq!(ui.dashboard.key(KeyCode::Esc), DashboardAction::Redraw);

    // Switch back to Branch B (`Branch beta reply.`).
    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    ui.dashboard
        .event_action(Event::Paste("beta assistant".into()));
    ui.dashboard.key(KeyCode::Enter);
    assert!(ui.render().contains("Summarize branch? · /tree"));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let back_on_beta = ui.render();
    assert!(
        back_on_beta.contains("Branch beta reply.")
            && !back_on_beta.contains("Branch alpha reply."),
        "switching back to Branch B must restore Branch B feed:\n{back_on_beta}"
    );
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["event"] == "start")
            .count(),
        starts_after_beta,
        "switching back and forth across `/tree` branches must spawn zero Pi processes"
    );

    // 6. Start Turn 4 (held mid-flight) -> read-only `/tree` works while `Running`, whereas
    //    branch navigation refuses cleanly while `Running` AND while `watch_sessions` lease is active!
    let mut s4 = Scenario::new();
    s4.prompt_steps(vec![
        vec![
            Step::WaitFile(release_turn_four.clone()),
            Step::WatchStatus(1),
            Step::assistant("Watched leaf reply."),
            Step::Settle,
        ],
        vec![
            Step::WatchStatus(0),
            Step::assistant("Watch released."),
            Step::Settle,
        ],
    ]);
    fake.script(&s4);

    ui.dashboard
        .event_action(Event::Paste("Start watched turn".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    let running = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Running)
    });

    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    assert!(ui.render().contains("Branch alpha reply."));
    ui.dashboard
        .event_action(Event::Paste("alpha assistant".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    assert!(
        ui.render()
            .contains("Cannot switch Operator tree branch while a turn is running."),
    );
    ui.dashboard.key(KeyCode::Esc);

    let tree_resp = match ui.exchange(Request::Operator(OperatorRequest::Tree {
        conversation: conversation_id,
    })) {
        Response::Operator(OperatorResponse::Tree(tree)) => tree,
        other => panic!("expected Tree response while running, got {other:?}"),
    };
    let alpha_id = tree_resp
        .entries
        .iter()
        .find(|e| e.preview.contains("Branch alpha reply."))
        .expect("alpha entry")
        .id
        .clone();
    match ui.exchange(Request::Operator(OperatorRequest::NavigateTree {
        conversation: conversation_id,
        expected_run: running.conversation.run,
        target_entry_id: alpha_id.clone(),
        summarize: None,
    })) {
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        } => {}
        other => panic!("expected Conflict navigating tree while Running, got {other:?}"),
    }

    // Release Turn 4 into active `watch_sessions` warm lease -> navigation still refuses!
    std::fs::write(&release_turn_four, b"go\n").unwrap();
    let leased = ui.wait_history(conversation_id, |history| {
        history
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Complete)
            && !history.conversation.worker_stopped
    });
    match ui.exchange(Request::Operator(OperatorRequest::NavigateTree {
        conversation: conversation_id,
        expected_run: leased.conversation.run,
        target_entry_id: alpha_id,
        summarize: None,
    })) {
        Response::Error {
            code: ErrorCode::Conflict,
            message,
        } => assert!(
            message.contains("lease") || message.contains("active"),
            "{message}"
        ),
        other => panic!("expected Conflict navigating tree while leased, got {other:?}"),
    }

    // Finish the watch lease cleanly.
    ui.dashboard
        .event_action(Event::Paste("Release watch".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Watch released.") && history.conversation.worker_stopped
    });
}

#[test]
fn dashboard_slash_tree_summarize_branch_supports_custom_prompt_user_rewind_and_stop_rollback() {
    use crossterm::event::{Event, KeyCode};
    use ovrcr::protocol::OperatorResponse;

    let live = Live::idle().bounded();
    let fake = FakePi::install(&live.config, live.root.path());
    let summary_ready = live.root.path().join("summary-ready");
    let summary_release = live.root.path().join("summary-release");

    let mut s1 = Scenario::new();
    s1.steps(vec![Step::assistant("Root answer."), Step::Settle]);
    fake.script(&s1);

    live.start_binary_env(&[fake_pi::executable_env(&fake)]);
    let mut ui = LiveDashboard::connect(&live);
    ui.dashboard.key(KeyCode::Char('o'));
    ui.flush();
    let conversation_id = match ui.exchange(Request::Operator(OperatorRequest::Current)) {
        Response::Operator(OperatorResponse::Current(Some(summary))) => summary.id,
        other => panic!("expected current conversation, got {other:?}"),
    };

    // 1. Seed Turn 1 (`Root prompt`) and Turn 2 (`Branch alpha prompt`).
    ui.dashboard
        .event_action(Event::Paste("Root prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Root answer.") && history.conversation.worker_stopped
    });

    let mut s2 = Scenario::new();
    s2.steps(vec![Step::assistant("Alpha exploration."), Step::Settle]);
    fake.script(&s2);
    ui.dashboard
        .event_action(Event::Paste("Branch alpha prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Alpha exploration.") && history.conversation.worker_stopped
    });

    // 2. Open `/tree`: selecting the current leaf (`Alpha exploration.`) does NOT prompt
    //    `"Summarize branch?"`.
    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    assert!(
        !ui.render().contains("Summarize branch? · /tree")
            && !ui.render().contains("Operator tree · /tree"),
        "selecting current leaf must close `/tree` without prompting Summarize branch?"
    );

    // 3. Reopen `/tree`, filter + search for `Branch alpha prompt`, verify `Esc` back-navigation
    //    from Custom prompt -> Summarize branch? -> `/tree` preserves filter/search/selection,
    //    then submit `Summarize with custom prompt` onto user entry `Branch alpha prompt`!
    let mut s_sum = Scenario::new();
    s_sum.set("tree_summary_mode", json!("ok"));
    s_sum.set(
        "tree_summary_text",
        json!("## Goal\nCaptured alpha exploration findings."),
    );
    fake.script(&s_sum);

    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    ui.dashboard.key(KeyCode::Tab);
    ui.dashboard.key(KeyCode::Tab); // Filter: user-only
    ui.dashboard
        .event_action(Event::Paste("alpha prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    let confirm_screen = ui.render();
    assert!(
        confirm_screen.contains("Summarize branch? · /tree")
            && confirm_screen.contains("No summary")
            && confirm_screen.contains("Summarize")
            && confirm_screen.contains("Summarize with custom prompt"),
        "expected Summarize branch? choices:\n{confirm_screen}"
    );
    // Navigate to `Summarize with custom prompt`, open it, type draft, then Esc twice back to `/tree`.
    ui.dashboard.key(KeyCode::Down);
    ui.dashboard.key(KeyCode::Down);
    ui.dashboard.key(KeyCode::Enter);
    assert!(ui.render().contains("Summarize branch · Custom prompt"));
    ui.dashboard
        .event_action(Event::Paste("temporary draft".into()));
    ui.dashboard.key(KeyCode::Esc);
    assert!(
        ui.render().contains("Summarize branch? · /tree"),
        "Esc in custom prompt returns to Summarize branch?"
    );
    ui.dashboard.key(KeyCode::Esc);
    let back_to_tree = ui.render();
    assert!(
        back_to_tree.contains("Operator tree · /tree")
            && back_to_tree.contains("filter: user-only")
            && back_to_tree.contains("alpha prompt")
            && back_to_tree.contains("Branch alpha prompt"),
        "Esc from Summarize branch? must preserve `/tree` filter, query, and selection:\n{back_to_tree}"
    );

    // Now confirm `Summarize with custom prompt` (`preserve alpha hypothesis`) onto `Branch alpha prompt`.
    ui.dashboard.key(KeyCode::Enter);
    ui.dashboard.key(KeyCode::Down);
    ui.dashboard.key(KeyCode::Down);
    ui.dashboard.key(KeyCode::Enter);
    ui.dashboard
        .event_action(Event::Paste("preserve alpha hypothesis".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();

    let summarized_history = ui.wait_history(conversation_id, |history| {
        history.conversation.worker_stopped
            && history.messages.iter().any(|msg| {
                matches!(
                    &msg.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::BranchSummary { summary })
                        if summary.contains("Captured alpha exploration findings.")
                )
            })
    });
    assert!(
        fake.log().iter().any(|entry| {
            entry["event"] == "tree_control"
                && entry["summarize"] == true
                && entry["customInstructions"] == "preserve alpha hypothesis"
        }),
        "expected worker to pass customInstructions to authorized tree_control: {:?}",
        fake.log()
    );
    // Drain server notifications + automatic history reload so TUI restores user prompt to composer.
    for _ in 0..4 {
        ui.pump();
    }
    let after_summary = ui.render();
    assert!(
        after_summary.contains("Captured alpha exploration findings.")
            && after_summary.contains("Branch summary recorded")
            && after_summary.contains("▌ ✓ Root prompt")
            && !after_summary.contains("▌ ✓ Branch alpha prompt")
            && !after_summary.contains("Alpha exploration.")
            && after_summary.contains("> Branch alpha prompt▏"),
        "branch summary onto user entry must show summary receipt, project target branch, and restore unwrapped prompt to composer:\n{after_summary}"
    );

    // 4. Submit the restored composer prompt (edited to `Branch beta prompt`) to advance past the
    //    branch_summary entry, then start a held `Summarize` switch back to `Alpha exploration.`
    //    and cancel it via foreground `Stop` (`Esc`) -> active Branch B and checkpoint survive!
    let mut s3 = Scenario::new();
    s3.steps(vec![Step::assistant("Beta branch reply."), Step::Settle]);
    fake.script(&s3);
    ui.dashboard.ctrl('c');
    ui.dashboard
        .event_action(Event::Paste("Branch beta prompt".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.wait_history(conversation_id, |history| {
        has_assistant(history, "Beta branch reply.") && history.conversation.worker_stopped
    });
    let checkpoint_before_stop =
        operator_harness::Registry::of(&live).checkpoint(conversation_id.0);

    let mut s_hold = Scenario::new();
    s_hold.set("tree_summary_mode", json!("hold"));
    s_hold.set(
        "tree_summary_ready_file",
        json!(summary_ready.display().to_string()),
    );
    s_hold.set(
        "tree_summary_release_file",
        json!(summary_release.display().to_string()),
    );
    fake.script(&s_hold);

    ui.dashboard.event_action(Event::Paste("/tree".into()));
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();
    ui.pump();
    let tree_with_summary = ui.render();
    assert!(
        tree_with_summary.contains("[branch summary]:"),
        "expected persisted branch_summary node in `/tree`:\n{tree_with_summary}"
    );
    ui.dashboard
        .event_action(Event::Paste("Alpha exploration".into()));
    ui.dashboard.key(KeyCode::Enter);
    assert!(ui.render().contains("Summarize branch? · /tree"));
    ui.dashboard.key(KeyCode::Down); // `Summarize`
    ui.dashboard.key(KeyCode::Enter);
    ui.flush();

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !summary_ready.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "held branch summary never started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    ui.pump();
    let running_summary = ui.render();
    assert!(
        running_summary.contains("Summarizing branch"),
        "expected live Summarizing branch activity label:\n{running_summary}"
    );

    // Foreground `Ctrl-x` stops the in-flight branch summarization and preserves Branch B!
    ui.dashboard.ctrl('x');
    ui.flush();
    let aborted = ui.wait_history(conversation_id, |history| {
        history.conversation.worker_stopped
            && history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.state == OperatorTurnState::Interrupted)
    });
    assert!(has_assistant(&aborted, "Beta branch reply."));
    assert!(!has_assistant(&aborted, "Alpha exploration."));
    assert_eq!(
        operator_harness::Registry::of(&live).checkpoint(conversation_id.0),
        checkpoint_before_stop,
        "aborted branch summarization must preserve pre-switch session.jsonl checkpoint"
    );
    let _ = summarized_history;
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
        OperatorRequest, OperatorToolStatus, OperatorTurnState, OperatorWorkerEvent, Request,
        Response, ServerMessage, SessionPhase, TerminalSize,
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

    mod cleanup {
        include!("fixtures/operator-pi/cleanup.rs");
    }

    const BUDGET: Duration = Duration::from_secs(30);
    /// The same source file the compiled Operator bundles into its system prompt.
    const BUNDLED_SKILL: &str = include_str!("../.agents/skills/ovrcr/SKILL.md");
    /// A personal skill that `--no-skills` must keep out of every model request,
    /// even though the fixture HOME contains it.
    const PERSONAL_SKILL_SENTINEL: &str = "PERSONAL_SKILL_MUST_NOT_LOAD_6f4a1";
    const COMPACTION_CONTEXT_WINDOW: u64 = 12_288;
    const STATUS_WATCH_REQUEST: &str = "Watch status-ready without sending it work.";
    const STATUS_UNWATCH_REQUEST: &str = "Stop watching status-ready; leave it running.";
    const CARE_REQUEST: &str = "Diagnose session ";
    const STATUS_REQUEST: &str = "What needs attention across OVRCR?";
    const STATUS_PROJECT_REQUEST: &str = "Show status only for Project fixture.";
    const STATUS_WORKSPACE_REQUEST: &str =
        "Show status only for Workspace status-work in Project fixture.";
    const STATUS_SESSION_REQUEST: &str =
        "Show status only for the status-ready session in Project fixture.";

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
            "Status overviews default to all registered Projects in this instance",
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
        assert!(
            system.contains("<name>ovrcr-status</name>"),
            "{label}: status was not advertised"
        );
        assert!(system.contains(&format!(
            "<location>{}</location>",
            operator_profile.join("ovrcr-status/SKILL.md").display()
        )));
        assert!(system.contains("<name>ovrcr-workspaces</name>"));
        assert!(system.contains("<description>Inspect or register Projects, create or reuse Git Workspaces, and launch requested Terminals or managed Agents with verified readiness.</description>"));
        assert!(system.contains(&format!(
            "<location>{}</location>",
            operator_profile.join("ovrcr-workspaces/SKILL.md").display()
        )));
        assert_eq!(
            system.matches("<name>ovrcr-").count(),
            5,
            "only implemented skills are advertised"
        );
        assert!(system.contains("<name>ovrcr-session-care</name>"));
        assert!(system.contains(&format!(
            "<location>{}</location>",
            operator_profile.join("ovrcr-session-care/SKILL.md").display()
        )));
        assert!(system.contains("<name>ovrcr-cleanup</name>"));
        assert!(system.contains(&format!(
            "<location>{}</location>",
            operator_profile.join("ovrcr-cleanup/SKILL.md").display()
        )));
        assert!(!system.contains("unregistered-sentinel"));
        assert!(!system.contains("project-sentinel"));
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
        /// Provision only in fixtures that explicitly own their HOME/repo.
        workspaces: bool,
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
            if operator_provider::is_branch_summary_request(body) {
                let user = operator_provider::last_user_text(body);
                let focus = user
                    .lines()
                    .find_map(|line| line.strip_prefix("Additional focus: "))
                    .map(str::trim)
                    .filter(|text| !text.is_empty());
                let summary = match focus {
                    Some(focus) => format!(
                        "## Goal\nSummarized abandoned branch ({focus}).\n\n## Key Decisions\n- Preserved branch context before switching."
                    ),
                    None => String::from(
                        "## Goal\nSummarized abandoned branch before switching.\n\n## Key Decisions\n- Preserved branch context on the target branch.",
                    ),
                };
                return Plan::text(&summary);
            }
            if operator_provider::is_compaction_request(body) {
                let plan = Plan::text("FIXTURE_COMPACTION_SUMMARY");
                return if mode.hold_compaction {
                    plan.holding()
                } else {
                    plan
                };
            }
            let user = operator_provider::last_user_text(body);
            if user == "CLEANUP_TASK_HOLD" {
                return Plan::text("Task remains active during preview").holding();
            }
            if user
                .split("User request:\n")
                .last()
                .is_some_and(|request| request.starts_with("Preview cleanup"))
            {
                let mut counters = counters.lock().unwrap();
                let step = counters.entry("cleanup-preview".into()).or_default();
                let plan = cleanup::plan(body, *step);
                *step += 1;
                return plan;
            }
            if user.starts_with("WORKSPACES_AGENT_PROBE:") {
                return Plan::text(
                    "WORKSPACES_AGENT_RESPONSE: managed Pi received the requested probe.",
                );
            }
            if mode.workspaces
                && let Some(request) = user.split("User request:\n").last()
                && let Some(action) = request
                    .strip_prefix("WORKSPACES_")
                    .and_then(|text| text.split(':').next())
            {
                let mut counters = counters.lock().unwrap();
                let step = counters.entry(format!("workspaces-{action}")).or_default();
                let plan = workspaces_plan(body, action, *step % 3);
                *step += 1;
                return plan;
            }
            let key = [
                "Control exact session",
                "CARE_NATIVE_SEED",
                CARE_REQUEST,
                STATUS_REQUEST,
                STATUS_WATCH_REQUEST,
                STATUS_UNWATCH_REQUEST,
                STATUS_PROJECT_REQUEST,
                STATUS_WORKSPACE_REQUEST,
                STATUS_SESSION_REQUEST,
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
                "GUI_SLASH_SEED",
                "GUI_SKILLS_REQUEST",
                "GUI_BRANCH_ALPHA",
                "GUI_BRANCH_BETA",
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
                ("Control exact session", _) => session_control_plan(body, step),
                ("CARE_NATIVE_SEED", _) => Plan::text("CARE_NATIVE_HISTORY_PRESERVED"),
                (CARE_REQUEST, _) => session_care_plan(body, step),
                (STATUS_REQUEST | STATUS_PROJECT_REQUEST | STATUS_WORKSPACE_REQUEST | STATUS_SESSION_REQUEST, _) => status_plan(body, &key, step),
                (STATUS_WATCH_REQUEST, 0) => {
                    let system = operator_provider::system_text(body);
                    let path = system.split("<name>ovrcr-coordinator</name>").nth(1).unwrap().split("<location>").nth(1).unwrap().split("</location>").next().unwrap();
                    Plan::tool("watch-instructions", "read", json!({"path": path}))
                }
                (STATUS_WATCH_REQUEST, 1) => {
                    assert!(operator_provider::tool_result_texts(body).join("\n").contains("Removing a watch does not"));
                    Plan::tool("watch-baseline", "bash", json!({"command": "printf 'WATCH_ROWS='; \"$OVRCR_EXECUTABLE\" --json terminal list"}))
                }
                (STATUS_WATCH_REQUEST, 2) => {
                    let rows = marked_tool_json(body, "WATCH_ROWS=");
                    let row = rows.as_array().unwrap().iter().find(|row| row["name"] == "status-ready").unwrap();
                    Plan::tool("watch-add", "watch_sessions", json!({"action": "add", "sessions": [{"session_id": row["id"], "project": row["project"], "label": row["name"], "baseline": {"run": row["run"], "activity_revision": row["agent"]["activity_revision"], "unread_turn": row["unread"]["turn"]}}]}))
                }
                (STATUS_WATCH_REQUEST, _) => Plan::text("Watching status-ready; no input sent."),
                (STATUS_UNWATCH_REQUEST, 0) => Plan::tool("watch-remove", "watch_sessions", json!({"action": "remove", "all": true})),
                (STATUS_UNWATCH_REQUEST, _) => Plan::text("Watch removed; session left running."),
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
                ("GUI_SKILLS_REQUEST", _) => {
                    assert!(user.contains("Selected Operator skill: ovrcr-status"));
                    assert!(user.contains(include_str!("../pi/skills/ovrcr-status/SKILL.md")));
                    Plan::text("SKILLS_PICKER_DONE: selected status instructions and captured target received.")
                }
                ("GUI_SLASH_SEED", _) => Plan::text(
                    "Seeded root turn for Operator slash commands.\n\n- Protocol version: `88`\n- Registry schema: `35`",
                ),
                ("GUI_BRANCH_ALPHA", _) => Plan::text(
                    "Alpha branch exploration: verified `/model`, `/thinking`, `/session`, and `/compact` checkpoints.",
                ),
                ("GUI_BRANCH_BETA", _) => Plan::text(
                    "Beta branch exploration: verified `/tree` active-branch switching and branch summarization.",
                ),
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

    fn workspaces_plan(body: &Value, action: &str, step: usize) -> Plan {
        let system = operator_provider::system_text(body);
        let location = system
            .split("<name>ovrcr-workspaces</name>")
            .nth(1)
            .and_then(|tail| tail.split("<location>").nth(1))
            .and_then(|tail| tail.split("</location>").next())
            .expect("Pi must advertise the shipped Workspace procedure");
        match step {
            0 => {
                assert!(
                    !system.contains("Read the repository's default branch and pass that name."),
                    "the shared guide must not override the explicit-base requirement"
                );
                assert!(!system.contains("Registration creates the protected root workspace on the detected default branch."), "registration must not promise a Root branch switch");
                Plan::tool("workspaces-instructions", "read", json!({"path": location}))
            }
            1 => {
                let instructions = operator_provider::tool_result_texts(body).join("\n");
                for required in [
                    "Inspection alone neither registers",
                    "user's explicit base",
                    "ask which path",
                    "Leave it unanswered",
                    "Process creation",
                ] {
                    assert!(
                        instructions.contains(required),
                        "missing Workspace procedure: {required}"
                    );
                }
                assert!(
                    [
                        "inspect",
                        "missing-base",
                        "provision",
                        "reuse",
                        "terminal",
                        "agent",
                        "ambiguous",
                        "detached",
                        "root",
                        "trust"
                    ]
                    .contains(&action)
                );
                Plan::tool(
                    "workspaces-cli",
                    "bash",
                    json!({"command": format!("python3 - {action} <<'PY'\n{}\nPY", include_str!("fixtures/operator-pi/workspaces.py"))}),
                )
            }
            _ => {
                let result = marked_tool_json(body, "WORKSPACES_RESULT=");
                let detail = match action {
                    "inspect" => format!(
                        "Inspection only: repository {} on {}. No registration or launch.",
                        result["repo"], result["branch"]
                    ),
                    "missing-base" => format!(
                        "Which explicit base branch or commit should the new branch use? CLI refused: {}",
                        result["refusal"]
                    ),
                    "ambiguous" => format!(
                        "Which exact Workspace path should I use? Candidates: {}. Refused: {}",
                        result["candidates"], result["refusal"]
                    ),
                    "detached" => format!(
                        "Confirm whether detached checkout {} at HEAD {} is intended; no launch. {}",
                        result["workspace"]["path"], result["head"], result["refusal"]
                    ),
                    "root" => format!(
                        "Root launch refused on {}; Root left unchanged. {}",
                        result["branch"], result["refusal"]
                    ),
                    "trust" => format!(
                        "Running but not a verified Agent: session {} run {} requires a human decision. Left unanswered: {}",
                        result["session"]["id"], result["session"]["run"], result["screen"]
                    ),
                    "agent" | "terminal" => format!(
                        "Verified requested {action}: session {} run {} in {}. Observed: {}",
                        result["session"]["id"],
                        result["session"]["run"],
                        result["session"]["cwd"],
                        result["screen"]
                    ),
                    _ => format!(
                        "{} Workspace: {}",
                        if action == "reuse" {
                            "Reused"
                        } else {
                            "Created"
                        },
                        result["workspace"]
                    ),
                };
                Plan::text(&format!(
                    "Scope: requested repository / Project fixture. {detail}"
                ))
            }
        }
    }

    // Controlled model responses exercise native reads and the matching-instance
    // CLI. Reports are derived from its real tool result, never a canned success.
    fn status_plan(body: &Value, request: &str, step: usize) -> Plan {
        let system = operator_provider::system_text(body);
        let location = system
            .split("<name>ovrcr-status</name>")
            .nth(1)
            .and_then(|tail| tail.split("<location>").nth(1))
            .and_then(|tail| tail.split("</location>").next())
            .expect("Pi must advertise status before the first model request");
        let document = Path::new(location);
        match step % 4 {
            0 => Plan::tool("status-instructions", "read", json!({"path": document})),
            1 => {
                let results = operator_provider::tool_result_texts(body).join("\n");
                assert!(results.contains("Default to all registered Projects"));
                assert!(results.contains("references/inventory.md"));
                Plan::tool(
                    "status-reference",
                    "read",
                    json!({"path": document.parent().unwrap().join("references/inventory.md")}),
                )
            }
            2 => {
                assert!(
                    operator_provider::tool_result_texts(body)
                        .join("\n")
                        .contains("used/50")
                );
                let project = if request == STATUS_REQUEST {
                    ""
                } else {
                    ", '--project', 'fixture'"
                };
                Plan::tool(
                    "status-inventory",
                    "bash",
                    json!({"command": format!(
                        "python3 - <<'PY'\nimport json, os, subprocess\ncli = os.environ['OVRCR_EXECUTABLE']\ndef get(*args):\n    return json.loads(subprocess.check_output([cli, '--json', *args], text=True))\nprint('STATUS_INVENTORY=' + json.dumps(dict(executable=cli, instance=os.environ['OVRCR_HOME'], projects=get('project', 'list'), workspaces=get('workspace', 'list'{project}), sessions=get('terminal', 'list'))))\nPY"
                    )}),
                )
            }
            _ => {
                let inventory = marked_tool_json(body, "STATUS_INVENTORY=");
                let all = inventory["sessions"].as_array().unwrap();
                let live = all
                    .iter()
                    .filter(|row| {
                        row["scope"] == "Workspace"
                            && matches!(row["phase"].as_str(), Some("running" | "paused"))
                    })
                    .count();
                let rows: Vec<_> = all
                    .iter()
                    .filter(|row| row["scope"] == "Workspace")
                    .filter(|row| request == STATUS_REQUEST || row["project"] == "fixture")
                    .filter(|row| {
                        request != STATUS_WORKSPACE_REQUEST || row["workspace"] == "status-work"
                    })
                    .filter(|row| {
                        request != STATUS_SESSION_REQUEST || row["name"] == "status-ready"
                    })
                    .collect();
                let scope = match request {
                    STATUS_PROJECT_REQUEST => "Project fixture",
                    STATUS_WORKSPACE_REQUEST => "Workspace status-work in Project fixture",
                    STATUS_SESSION_REQUEST => "session status-ready in Project fixture",
                    _ => "all registered Projects in this instance",
                };
                let projects = inventory["projects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| request == STATUS_REQUEST || row["name"] == "fixture")
                    .map(|row| row["name"].as_str().unwrap())
                    .collect::<Vec<_>>();
                let workspaces = inventory["workspaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| {
                        request != STATUS_WORKSPACE_REQUEST || row["branch"] == "status-work"
                    })
                    .filter(|row| {
                        request != STATUS_SESSION_REQUEST
                            || rows.iter().any(|session| session["cwd"] == row["path"])
                    })
                    .count();
                let mut text = format!(
                    "Scope: {scope}. Projects: {}. Workspaces: {workspaces}. Sessions: {}.\nInstance live Workspace capacity: {live}/50; {} remaining. Operator: separate slot.\n",
                    projects.join(", "),
                    rows.len(),
                    50 - live
                );
                for row in rows {
                    let activity = if row["kind"] == "terminal" {
                        "Terminal; no Agent reporting"
                    } else if row["reporting_unavailable"] == true || row["agent"].is_null() {
                        "unavailable reporting; activity unknown"
                    } else {
                        row["activity"].as_str().unwrap()
                    };
                    text.push_str(&format!(
                        "Session {} ({}): {activity}{}\n",
                        row["id"],
                        row["name"].as_str().unwrap(),
                        if row["unread"].is_object() {
                            "; Unread result needs review"
                        } else {
                            ""
                        }
                    ));
                }
                text.push_str("Inspection only; resources, Unread and Input requests unchanged.");
                Plan::text(&text)
            }
        }
    }

    // Also usable by the native GUI helper: "Control exact session ID RUN ACTION".
    fn session_control_plan(body: &Value, step: usize) -> Plan {
        let user = operator_provider::last_user_text(body);
        let mut words = user
            .split("Control exact session ")
            .nth(1)
            .unwrap()
            .split_whitespace();
        let id: u64 = words.next().unwrap().parse().unwrap();
        let run: u64 = words.next().unwrap().parse().unwrap();
        let action = words.next().unwrap();
        assert!(["pause", "resume", "stop", "reopen", "close"].contains(&action));
        let call = format!("control-{step}");
        match step % 5 {
            0 => {
                let system = operator_provider::system_text(body);
                let path = system
                    .split("<name>ovrcr-session-care</name>")
                    .nth(1)
                    .unwrap()
                    .split("<location>")
                    .nth(1)
                    .unwrap()
                    .split("</location>")
                    .next()
                    .unwrap();
                Plan::tool(&call, "read", json!({"path": path}))
            }
            1 => {
                let instructions = operator_provider::tool_result_texts(body).join("\n");
                assert!(instructions.contains("--expected-run RUN"));
                assert!(instructions.contains("controlled-stop"));
                Plan::tool(
                    &call,
                    "bash",
                    json!({"command": "printf 'CONTROL_BEFORE='; \"$OVRCR_EXECUTABLE\" --json terminal list"}),
                )
            }
            2 => {
                let rows = marked_tool_json(body, "CONTROL_BEFORE=");
                let row = rows
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["id"] == id)
                    .unwrap();
                assert_eq!(row["scope"], "Workspace");
                // Keep the requested run even if inventory has changed; Server is the fence.
                let command = match action {
                    "stop" => "'kill'",
                    "reopen" => "'terminal', 'reopen'",
                    "close" => "'terminal', 'close'",
                    "pause" => "'pause'",
                    _ => "'resume'",
                };
                Plan::tool(
                    &call,
                    "bash",
                    json!({"command": format!(
                        "python3 - <<'PY'\nimport json, os, subprocess\np = subprocess.run([os.environ['OVRCR_EXECUTABLE'], '--json', {command}, '{id}', '--expected-run', '{run}'], text=True, capture_output=True)\nprint('CONTROL_RESULT=' + json.dumps(dict(code=p.returncode, output=p.stdout, error=p.stderr, instance=os.environ['OVRCR_HOME'], executable=os.environ['OVRCR_EXECUTABLE'])))\nPY"
                    )}),
                )
            }
            3 => Plan::tool(
                &call,
                "bash",
                json!({"command": format!(
                    "printf 'CONTROL_AFTER='; \"$OVRCR_EXECUTABLE\" --json terminal list{}",
                    if action == "close" { " --archived" } else { "" }
                )}),
            ),
            _ => {
                let result = marked_tool_json(body, "CONTROL_RESULT=");
                let rows = marked_tool_json(body, "CONTROL_AFTER=");
                let row = rows.as_array().unwrap().iter().find(|r| r["id"] == id);
                Plan::text(&format!(
                    "Session {id} requested run {run}: {action}; CLI exit {}. Observed: {}. {}",
                    result["code"],
                    row.unwrap_or(&Value::Null),
                    result["error"]
                ))
            }
        }
    }

    // Controlled provider: drive the shipped procedure with actual CLI results,
    // not a canned diagnosis. This does not test unconstrained model inference.
    fn session_care_plan(body: &Value, step: usize) -> Plan {
        let user = operator_provider::last_user_text(body);
        let id: u64 = user
            .split(CARE_REQUEST)
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let call = format!("care-{step}");
        match step % 5 {
            0 => {
                let system = operator_provider::system_text(body);
                let path = system
                    .split("<name>ovrcr-session-care</name>")
                    .nth(1)
                    .expect("session care must be advertised")
                    .split("<location>")
                    .nth(1)
                    .unwrap()
                    .split("</location>")
                    .next()
                    .unwrap();
                Plan::tool(&call, "read", json!({"path": path}))
            }
            1 => {
                let instructions = operator_provider::tool_result_texts(body).join("\n");
                for required in [
                    "Known",
                    "Unknown",
                    "Next supported action",
                    "recovery.requires_ack",
                    "Read-only boundary",
                    "terminal transcript ID",
                    "terminal screen ID",
                ] {
                    assert!(
                        instructions.contains(required),
                        "missing diagnosis instruction: {required}"
                    );
                }
                Plan::tool(
                    &call,
                    "bash",
                    json!({"command": format!(
                        "python3 - <<'PY'\nimport json, os, subprocess\ncli = os.environ['OVRCR_EXECUTABLE']\ndef get(*args):\n    return json.loads(subprocess.check_output([cli, '--json', *args], text=True))\nrow = next(r for r in get('terminal', 'list') if r['id'] == {id})\nprojects = get('project', 'list')\nworkspaces = get('workspace', 'list', '--project', row['project'])\nworkspace = next(w for w in workspaces if w['path'] == row['cwd'])\nassert any(p['name'] == row['project'] for p in projects)\nprint('CARE_INVENTORY=' + json.dumps(dict(instance=os.environ['OVRCR_HOME'], executable=cli, workspace=workspace, session=row)))\nPY"
                    )}),
                )
            }
            2 => {
                let row = marked_tool_json(body, "CARE_INVENTORY=")["session"].clone();
                let provider = row["agent"]["binding"]["provider"].as_str().unwrap_or("");
                eprintln!(
                    "CARE_READ_SELECTION id={id} observed={} provider={provider}",
                    row["id"]
                );
                let command = if matches!(provider, "Codex" | "Claude") {
                    "transcript"
                } else if provider == "Omp" {
                    "screen"
                } else {
                    "read"
                };
                let bounds = match command {
                    "screen" => ", '--rows', '2', '--cols', '80'",
                    "read" => ", '--max-lines', '40'",
                    _ => "",
                };
                Plan::tool(
                    &call,
                    "bash",
                    json!({"command": format!(
                        "python3 - <<'PY'\nimport json, os, subprocess\np = subprocess.run([os.environ['OVRCR_EXECUTABLE'], '--json', 'terminal', '{command}', '{id}'{bounds}], capture_output=True, text=True)\nprint('CARE_CONTENT=' + json.dumps(dict(session={id}, command='{command}', code=p.returncode, output=p.stdout, error=p.stderr)))\nPY"
                    )}),
                )
            }
            3 => {
                let content = marked_tool_json(body, "CARE_CONTENT=");
                assert_eq!(
                    content["session"], id,
                    "only this diagnosis's observation is evidence"
                );
                let fallback = content["command"] == "transcript"
                    && (content["code"] != 0 || content["output"].as_str().unwrap().contains('…'));
                Plan::tool(
                    &call,
                    "bash",
                    json!({"command": format!(
                        "python3 - <<'PY'\nimport json, os, subprocess\ncli = os.environ['OVRCR_EXECUTABLE']\nfallback = None\nif {fallback}:\n    p = subprocess.run([cli, '--json', 'terminal', 'read', '{id}', '--max-lines', '40'], capture_output=True, text=True)\n    fallback = dict(code=p.returncode, output=p.stdout, error=p.stderr)\nrows = json.loads(subprocess.check_output([cli, '--json', 'terminal', 'list'], text=True))\nprint('CARE_REFRESH=' + json.dumps(dict(session=next(r for r in rows if r['id'] == {id}), fallback=fallback)))\nPY",
                        fallback = if fallback { "True" } else { "False" }
                    )}),
                )
            }
            _ => {
                let inventory = marked_tool_json(body, "CARE_INVENTORY=");
                let refreshed = marked_tool_json(body, "CARE_REFRESH=");
                let row = &refreshed["session"];
                // The native GUI helper can inspect real reporting sessions,
                // whose metric age advances even without a state change.
                for key in ["id", "run", "project", "cwd"] {
                    assert_eq!(
                        inventory["session"][key], row[key],
                        "diagnosis target changed"
                    );
                }
                assert_eq!(
                    inventory["session"]["agent"]["binding"],
                    row["agent"]["binding"]
                );
                assert_eq!(
                    inventory["session"]["recovery"]["conversation"],
                    row["recovery"]["conversation"]
                );
                let content = marked_tool_json(body, "CARE_CONTENT=");
                let unavailable = row["reporting_unavailable"] == true || row["agent"].is_null();
                let condition = if matches!(
                    row["phase"].as_str(),
                    Some("interrupted" | "exited" | "stopped" | "paused")
                ) {
                    row["phase"].as_str().unwrap()
                } else if unavailable {
                    "unavailable reporting; activity unknown"
                } else {
                    row["activity"].as_str().unwrap()
                };
                let evidence = if content["code"] == 0 {
                    let parsed: Value =
                        serde_json::from_str(content["output"].as_str().unwrap()).unwrap();
                    let text = match content["command"].as_str().unwrap() {
                        "transcript" => parsed["entries"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|entry| entry["Assistant"].as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                        "read" => parsed["text"].as_str().unwrap().to_owned(),
                        "screen" => parsed["rows"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .flat_map(|row| row["cells"].as_array().unwrap())
                            .filter_map(|cell| cell["text"].as_str())
                            .collect::<String>(),
                        _ => unreachable!(),
                    };
                    text.trim().chars().take(180).collect::<String>()
                } else {
                    "unavailable".into()
                };
                let unknown = if content["code"] != 0 {
                    "content unavailable"
                } else if content["output"].as_str().unwrap().contains('…') {
                    "transcript truncated; terminal tail is partial evidence"
                } else {
                    "task success not established"
                };
                let next = if row["recovery"]["requires_ack"] == true
                    && !matches!(row["phase"].as_str(), Some("running" | "paused"))
                {
                    "human ownership verification required; no acknowledgement performed"
                } else if condition == "waiting_input" {
                    "human answer required; Input request unchanged"
                } else if condition == "response_ready" {
                    "review response; Unread unchanged"
                } else if unavailable {
                    "reporting/recovery needs explicit investigation"
                } else {
                    "wait or request a supported action explicitly"
                };
                Plan::text(&format!(
                    "Scope: instance {}, Project {}, Workspace {} ({}), session {id}, run {}.\nKnown: {condition}. Exit code: {}. Recovery blocker: {}.\nObserved content: {evidence}.\nUnknown: {unknown}.\nNext supported action: {next}. Inspection only; no control or recovery performed.",
                    inventory["instance"].as_str().unwrap(),
                    row["project"].as_str().unwrap(),
                    inventory["workspace"]["branch"].as_str().unwrap(),
                    inventory["workspace"]["id"],
                    row["run"],
                    row["exit_code"],
                    row["recovery"]["unavailable"],
                ))
            }
        }
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
                workspaces: false,
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

    // A real PTY publishes only its own hook capability to a private fixture
    // file, as in server_lifecycle's hook child. Keep the supervisor connection
    // alive so inventory has stable Ready/Input/unavailable observations.
    fn status_agent(live: &live::Live, name: &str) -> (ovrcr::protocol::SessionId, UnixStream) {
        reporter_agent(live, name, ovrcr::protocol::AgentProvider::Codex)
    }

    fn reporter_agent(
        live: &live::Live,
        name: &str,
        provider: ovrcr::protocol::AgentProvider,
    ) -> (ovrcr::protocol::SessionId, UnixStream) {
        use ovrcr::protocol::*;
        let identity = live.root.path().join(format!("{name}.identity"));
        let Response::CreatedSession(session) = live.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(), workspace: "work".into(), name: name.into(), label: None,
            kind: SessionKind::Agent { name: provider.name().into() },
            argv: vec!["/bin/sh".into(), "-c".into(), "umask 077; printf '%s' \"$OVRCR_HOOK_TOKEN\" > \"$1\"; printf 'Observed Agent screen: %s\\n' \"$2\"; while IFS= read -r line; do printf '%s\\n' \"$line\" >> \"$1.input\"; [ \"$line\" != fixture-exit ] || exit 7; done".into(), "status-fixture".into(), identity.clone().into_os_string(), name.into()],
        })) else { panic!("status Agent PTY creation failed") };
        live.own_group(session.pid.unwrap() as libc::pid_t);
        let deadline = Instant::now() + BUDGET;
        let capability = loop {
            if let Ok(token) = std::fs::read_to_string(&identity)
                && token.len() == 64
            {
                break std::array::from_fn(|index| {
                    u8::from_str_radix(&token[index * 2..index * 2 + 2], 16).unwrap()
                });
            }
            assert!(
                Instant::now() < deadline,
                "fixture hook identity unavailable"
            );
            thread::sleep(Duration::from_millis(10));
        };
        let mut stream = protocol::connect_server(&live.socket).unwrap();
        stream.set_read_timeout(Some(BUDGET)).unwrap();
        let response = protocol::client::request(
            &mut stream,
            1,
            Request::ReserveAgent(ReserveAgent {
                session: session.id,
                capability: AgentSecret(capability),
                operation: format!("reserve-{name}"),
                expected_epoch: 0,
                pi_resume: None,
                invocation: format!("invocation-{name}"),
                provider,
            }),
        )
        .unwrap();
        let Response::AgentOperation(AgentOperationResult::Reserved(reservation)) = response else {
            panic!("reserve status reporter: {response:?}")
        };
        let response = protocol::client::request(
            &mut stream,
            2,
            Request::Supervisor(SupervisorRequest {
                auth: SupervisorAuth {
                    session: session.id,
                    lease: reservation.lease.clone(),
                },
                operation: format!("bind-{name}"),
                command: AgentCommand::Bind {
                    expected_binding: None,
                    conversation: format!("conversation-{name}"),
                },
            }),
        )
        .unwrap();
        let Response::AgentOperation(AgentOperationResult::Bound(binding)) = response else {
            panic!("bind status reporter: {response:?}")
        };
        let observations = vec![
            AgentObservation::Activity(ActivitySample {
                state: AgentActivity::Busy,
                quality: SampleQuality::Confirmed,
                turn: Some("status-cycle".into()),
            }),
            match name {
                "status-ready" | "care-transcript" => AgentObservation::Activity(ActivitySample {
                    state: AgentActivity::ResponseReady,
                    quality: SampleQuality::Confirmed,
                    turn: Some("status-cycle".into()),
                }),
                "status-input" | "care-truncated" => AgentObservation::Input(vec![InputRequest {
                    id: "question:status-fixture".into(),
                    kind: InputKind::Input,
                }]),
                "status-unavailable" => AgentObservation::Health(HealthSample {
                    state: ReporterHealth::Unavailable,
                    reason: Some("fixture reporting disconnected".into()),
                }),
                "care-idle" | "care-omp" => AgentObservation::Activity(ActivitySample {
                    state: AgentActivity::Idle,
                    quality: SampleQuality::Confirmed,
                    turn: Some("status-cycle".into()),
                }),
                "care-busy" | "care-interrupted" | "care-exited" | "care-missing" => {
                    AgentObservation::Activity(ActivitySample {
                        state: AgentActivity::Busy,
                        quality: SampleQuality::Confirmed,
                        turn: Some("status-cycle".into()),
                    })
                }
                _ => panic!("unknown status fixture"),
            },
        ];
        for (index, observation) in observations.into_iter().enumerate() {
            let report = ProviderReport {
                binding: binding.clone(),
                revision: index as u64 + 1,
                observation,
            };
            if matches!(report.observation, AgentObservation::Health(_)) {
                let response = protocol::client::request(
                    &mut stream,
                    3,
                    Request::Supervisor(SupervisorRequest {
                        auth: SupervisorAuth {
                            session: session.id,
                            lease: reservation.lease.clone(),
                        },
                        operation: format!("health-{name}"),
                        command: AgentCommand::Health(report),
                    }),
                )
                .unwrap();
                assert_eq!(
                    response,
                    Response::AgentOperation(AgentOperationResult::HealthUpdated)
                );
            } else {
                assert_eq!(
                    live.request(Request::AgentReport(AgentReport {
                        session: session.id,
                        capability,
                        sequence: None,
                        update: AgentUpdate::Provider(report),
                    })),
                    Response::Ok
                );
            }
        }
        if matches!(name, "care-transcript" | "care-truncated") {
            let conversation = binding.conversation.clone();
            let history = live.root.path().join(format!("{name}.jsonl"));
            let text = if name == "care-truncated" {
                format!(
                    "Which deployment target? {}PRIVATE_CLIPPED_TAIL",
                    "x".repeat(17_000)
                )
            } else {
                "Diagnosis fixture reply: checks finished; review required.".into()
            };
            std::fs::write(&history, format!("{}\n{}\n",
                json!({"type": "session_meta", "payload": {"id": conversation}}),
                json!({"type": "event_msg", "payload": {"type": "agent_message", "message": text}}),
            )).unwrap();
            let retained = protocol::client::request(
                &mut stream,
                4,
                Request::Supervisor(SupervisorRequest {
                    auth: SupervisorAuth {
                        session: session.id,
                        lease: reservation.lease,
                    },
                    operation: format!("retain-{name}"),
                    command: AgentCommand::RetainConversation {
                        binding,
                        reference: Box::new(ConversationReference::Codex(CodexConversation {
                            conversation,
                            executable: "/bin/sh".into(),
                            history: Some(history),
                            config_dir: live.root.path().to_owned(),
                            options: vec![],
                        })),
                    },
                }),
            )
            .unwrap();
            assert_eq!(
                retained,
                Response::AgentOperation(AgentOperationResult::ConversationRetained)
            );
        }
        (session.id, stream)
    }

    fn status_cli(live: &live::Live, args: &[&str]) -> Value {
        let output = live.command().arg("--json").args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn workspaces_turn(
        driver: &mut DashboardDriver,
        live: &live::Live,
        provider: &Provider,
        action: &str,
        request: &str,
    ) -> (Value, String) {
        driver.reopen();
        driver.submit(&format!("WORKSPACES_{action}: {request}"));
        let conversation = driver.conversation().id;
        let history = wait_terminal(driver, conversation, provider, live);
        wait_stopped(driver, conversation);
        let turn = history
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn;
        let outputs = tool_outputs_with_status(&history);
        assert!(
            outputs
                .iter()
                .all(|(_, _, status)| *status == OperatorToolStatus::Succeeded),
            "{outputs:?}"
        );
        let answer: String = history
            .messages
            .iter()
            .filter(|message| message.turn == turn)
            .filter_map(|message| match &message.content {
                OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(
            answer.contains("Scope: requested repository / Project fixture"),
            "{answer}"
        );
        assert!(driver.screen().contains("Scope: requested repository"));
        let result = marked_tool_json(
            &provider.requests().last().unwrap().body,
            "WORKSPACES_RESULT=",
        );
        assert_eq!(
            result["executable"],
            live.executable.to_string_lossy().as_ref()
        );
        assert_eq!(result["instance"], live.config.to_string_lossy().as_ref());
        (result, answer)
    }

    fn workspace_session_identities(rows: &Value) -> Vec<Value> {
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| {
                json!([
                    row["id"],
                    row["run"],
                    row["cwd"],
                    row["phase"],
                    row["kind"],
                    row["unread"]
                ])
            })
            .collect()
    }

    #[test]
    #[ignore = "requires installed Pi; explicit Workspace inspection through full/Quick"]
    fn installed_pi_operator_workspaces_explicit_selection() {
        let pi = pi_executable();
        for quick in [false, true] {
            let live = live::Live::idle().bounded();
            let provider = Provider::start(scenario_handler_mode(
                live.repo.clone(),
                live.root.path().join("marker"),
                live.root.path().join("background"),
                ScenarioMode {
                    workspaces: true,
                    ..ScenarioMode::default()
                },
            ));
            provider.seed_profile(&live.config);
            live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
            let mut driver = DashboardDriver::connect(&live);
            let request = "WORKSPACES_inspect: Inspect $HOME/repo only; do not register or launch.";
            let input = format!("/skill:ovrcr-workspaces {request}");
            if quick {
                driver.quick(&input);
            } else {
                driver.open();
                driver.submit(&input);
            }
            let conversation = driver.conversation().id;
            let history = wait_terminal(&mut driver, conversation, &provider, &live);
            wait_stopped(&mut driver, conversation);
            assert!(delta_text(&history).contains("Inspection only:"));
            assert!(driver.screen().contains("Scope: requested repository"));
            let requests = provider.requests();
            assert_eq!(requests.len(), 3, "skill read, real CLI inspection, reply");
            let user = operator_provider::last_user_text(&requests[0].body);
            let document = include_str!("../pi/skills/ovrcr-workspaces/SKILL.md");
            assert!(user.starts_with("Captured OVRCR context"));
            let selected = user
                .find("Selected Operator skill: ovrcr-workspaces")
                .unwrap();
            let instructions = user.find(document).expect("complete selected instructions");
            let request_start = user.find(&format!("User request:\n{request}")).unwrap();
            assert!(selected < instructions && instructions < request_start);
            assert!(!operator_harness::history_text(&history).contains(document));
            let result = marked_tool_json(&requests.last().unwrap().body, "WORKSPACES_RESULT=");
            assert_eq!(result["before"], result["after"]);
            assert_eq!(result["after"]["projects"], json!([]));
            assert_eq!(result["after"]["sessions"], json!([]));
            assert_eq!(
                result["executable"],
                live.executable.to_string_lossy().as_ref()
            );
            assert_eq!(result["instance"], live.config.to_string_lossy().as_ref());
            assert_eq!(status_cli(&live, &["project", "list"]), json!([]));
            assert_eq!(status_cli(&live, &["workspace", "list"]), json!([]));
            drop(driver);
            assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
            live.join();
            eprintln!(
                "INSTALLED_PI_WORKSPACES_EXPLICIT_PASS quick={quick} requests=3 shutdown=verified"
            );
        }
    }

    #[test]
    #[ignore = "requires installed Pi; real Operator CLI/Git/PTY provisioning"]
    fn installed_pi_operator_workspaces_provision_reuse_and_readiness() {
        let pi = pi_executable();
        for policy in ["off", "default_branch_only", "on"] {
            let mut live = live::Live::idle().bounded();
            // The matching binary and fixed working directory have no source checkout.
            let relocated = live.root.path().join("installed/ovrcr");
            std::fs::create_dir(relocated.parent().unwrap()).unwrap();
            std::fs::copy(&live.executable, &relocated).unwrap();
            live.executable = relocated;
            std::fs::write(
                live.config.join("dashboard.toml"),
                format!("automatic_local_terminals = \"{policy}\"\n"),
            )
            .unwrap();
            let provider = Provider::start(scenario_handler_mode(
                live.repo.clone(),
                live.root.path().join("marker"),
                live.root.path().join("background"),
                ScenarioMode {
                    workspaces: true,
                    ..ScenarioMode::default()
                },
            ));
            provider.seed_profile(&live.config);
            provider.seed_workspace_profile(live.root.path());
            install_personal_skill(live.root.path());
            live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
            let mut driver = DashboardDriver::connect(&live);
            driver.open();
            let (inspect, _) = workspaces_turn(
                &mut driver,
                &live,
                &provider,
                "inspect",
                "Inspect the repository at $HOME/repo only; do not register it or launch anything.",
            );
            assert_eq!(inspect["before"], inspect["after"]);
            assert_eq!(inspect["after"]["projects"], json!([]));
            assert_eq!(status_cli(&live, &["project", "list"]), json!([]));
            assert_skill_only_in_system(
                "workspaces",
                &provider.requests()[0].body,
                &live.config.join("operator-pi"),
            );
            assert_eq!(
                std::fs::read_to_string(live.config.join("operator-pi/ovrcr-workspaces/SKILL.md"))
                    .unwrap(),
                include_str!("../pi/skills/ovrcr-workspaces/SKILL.md")
            );

            let (missing, question) = workspaces_turn(
                &mut driver,
                &live,
                &provider,
                "missing-base",
                "Prepare a new branch, but I have not chosen its base. Show the missing-base CLI refusal and ask me; do not guess.",
            );
            assert_eq!(missing["before"], missing["after"]);
            assert!(question.contains("Which explicit base"));
            assert!(
                missing["refusal"]["stderr"]
                    .as_str()
                    .unwrap()
                    .contains("--base")
            );

            let (created, _) = workspaces_turn(
                &mut driver,
                &live,
                &provider,
                "provision",
                "Register $HOME/repo as fixture with $HOME/fixture-workspaces; create feature/setup from main. No explicit session launch; preserve automatic-local-terminal policy.",
            );
            assert_eq!(created["head"], created["base"]);
            assert_eq!(created["workspace"]["branch"], "feature/setup");
            assert_eq!(created["after"]["workspaces"].as_array().unwrap().len(), 2);
            let expected_shells = match policy {
                "off" => 0,
                "on" => 2,
                _ => 1,
            };
            assert_eq!(
                created["after"]["sessions"].as_array().unwrap().len(),
                expected_shells
            );
            let (reused, reply) = workspaces_turn(
                &mut driver,
                &live,
                &provider,
                "reuse",
                "Reuse fixture's existing feature/setup Workspace; do not register or create a duplicate or launch anything.",
            );
            assert!(reply.contains("Reused Workspace"));
            assert_eq!(created["workspace"], reused["workspace"]);
            assert_eq!(
                reused["before"]["workspaces"],
                reused["after"]["workspaces"]
            );
            assert_eq!(
                workspace_session_identities(&reused["before"]["sessions"]),
                workspace_session_identities(&reused["after"]["sessions"])
            );

            let (terminal, _) = workspaces_turn(
                &mut driver,
                &live,
                &provider,
                "terminal",
                "Open a Terminal named requested-terminal in feature/setup that prints its cwd then opens /bin/sh. Reuse that named Terminal if already running.",
            );
            let (again, _) = workspaces_turn(
                &mut driver,
                &live,
                &provider,
                "terminal",
                "Reuse the requested-terminal Terminal in feature/setup.",
            );
            assert_eq!(terminal["session"]["id"], again["session"]["id"]);
            assert_eq!(terminal["session"]["run"], again["session"]["run"]);
            assert_eq!(
                again["after"]["sessions"].as_array().unwrap().len(),
                expected_shells + 1
            );
            assert_eq!(again["session"]["cwd"], created["workspace"]["path"]);
            if policy == "off" {
                let (agent, reply) = workspaces_turn(
                    &mut driver,
                    &live,
                    &provider,
                    "agent",
                    "Start a managed Pi Agent named requested-agent in feature/setup using the supplied Pi executable. Once its composer is verified, send the readiness probe and verify the response.",
                );
                assert_eq!(agent["session"]["kind"], json!({"agent": "pi"}));
                assert_eq!(agent["session"]["cwd"], created["workspace"]["path"]);
                assert!(agent["session"]["unread"].is_object());
                assert!(reply.contains("WORKSPACES_AGENT_RESPONSE"));
                eprintln!(
                    "WORKSPACES_AGENT_STARTUP_SCREEN {}",
                    agent["startup_screen"]
                );
                let id = agent["session"]["id"].as_u64().unwrap().to_string();
                assert!(
                    status_cli(&live, &["terminal", "read", &id])["text"]
                        .as_str()
                        .unwrap()
                        .contains("WORKSPACES_AGENT_RESPONSE")
                );
                assert!(provider.requests().iter().any(|request| {
                    operator_provider::last_user_text(&request.body)
                        .starts_with("WORKSPACES_AGENT_PROBE:")
                }));
            }
            let root_branch = Command::new("git")
                .env("HOME", live.root.path())
                .arg("-C")
                .arg(&live.repo)
                .args(["branch", "--show-current"])
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8(root_branch.stdout).unwrap().trim(),
                "main"
            );
            assert!(
                Command::new("git")
                    .env("HOME", live.root.path())
                    .arg("-C")
                    .arg(&live.repo)
                    .args(["diff", "--exit-code"])
                    .status()
                    .unwrap()
                    .success()
            );
            assert_eq!(
                status_cli(&live, &["workspace", "list"])
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            drop(driver);
            assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
            live.join();
            eprintln!(
                "INSTALLED_PI_WORKSPACES_PASS policy={policy} provider_requests={} real_git=true real_pty=true shutdown=verified",
                provider.requests().len()
            );
        }
    }

    #[test]
    #[ignore = "requires installed Pi; real ambiguous/detached/Root and permission refusals"]
    fn installed_pi_operator_workspaces_refusals_preserve_resources() {
        let pi = pi_executable();
        let live = live::Live::idle().bounded();
        let provider = Provider::start(scenario_handler_mode(
            live.repo.clone(),
            live.root.path().join("marker"),
            live.root.path().join("background"),
            ScenarioMode {
                workspaces: true,
                ..ScenarioMode::default()
            },
        ));
        provider.seed_profile(&live.config);
        live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
        live.ready("feature/setup");
        status_cli(
            &live,
            &[
                "workspace",
                "create",
                "--project",
                "fixture",
                "--new-branch",
                "other",
                "--base",
                "main",
            ],
        );
        let other = status_cli(
            &live,
            &[
                "workspace",
                "get",
                "--project",
                "fixture",
                "--branch",
                "other",
            ],
        );
        let other_path = other["path"].as_str().unwrap();
        assert!(
            Command::new("git")
                .env("HOME", live.root.path())
                .args([
                    "-C",
                    other_path,
                    "checkout",
                    "--ignore-other-worktrees",
                    "feature/setup"
                ])
                .status()
                .unwrap()
                .success()
        );
        let mut driver = DashboardDriver::connect(&live);
        driver.open();
        let (ambiguous, question) = workspaces_turn(
            &mut driver,
            &live,
            &provider,
            "ambiguous",
            "Inspect feature/setup and show any CLI ambiguity; ask me to choose a path before launching.",
        );
        assert!(question.contains("Which exact Workspace path"));
        assert!(
            ambiguous["refusal"]
                .to_string()
                .contains("ambiguous workspace")
        );
        assert_eq!(ambiguous["candidates"].as_array().unwrap().len(), 2);
        assert_eq!(
            ambiguous["before"]["workspaces"],
            ambiguous["after"]["workspaces"]
        );
        assert_eq!(
            workspace_session_identities(&ambiguous["before"]["sessions"]),
            workspace_session_identities(&ambiguous["after"]["sessions"])
        );
        for candidate in ambiguous["candidates"].as_array().unwrap() {
            assert!(question.contains(candidate["path"].as_str().unwrap()));
        }

        assert!(
            Command::new("git")
                .env("HOME", live.root.path())
                .args(["-C", other_path, "checkout", "--detach"])
                .status()
                .unwrap()
                .success()
        );
        let (detached, question) = workspaces_turn(
            &mut driver,
            &live,
            &provider,
            "detached",
            "Inspect the detached Workspace; show why branch targeting refuses and ask me to resolve its path. Do not launch.",
        );
        assert!(
            question.contains("Confirm whether detached checkout") && question.contains(other_path)
        );
        assert!(detached["refusal"].to_string().contains("by --path"));
        assert_eq!(
            detached["before"]["workspaces"],
            detached["after"]["workspaces"]
        );
        assert_eq!(
            workspace_session_identities(&detached["before"]["sessions"]),
            workspace_session_identities(&detached["after"]["sessions"])
        );

        assert!(
            Command::new("git")
                .env("HOME", live.root.path())
                .arg("-C")
                .arg(&live.repo)
                .args(["checkout", "-b", "root-off-default"])
                .status()
                .unwrap()
                .success()
        );
        let (root, reply) = workspaces_turn(
            &mut driver,
            &live,
            &provider,
            "root",
            "Try opening a Terminal in Root; if its branch protection refuses, surface that refusal without switching branches or editing files.",
        );
        assert!(reply.contains("Root launch refused"));
        assert_eq!(root["branch"], "root-off-default");
        assert!(root["refusal"].to_string().contains("default branch"));
        assert_eq!(root["before"]["workspaces"], root["after"]["workspaces"]);
        assert_eq!(
            workspace_session_identities(&root["before"]["sessions"]),
            workspace_session_identities(&root["after"]["sessions"])
        );

        let input_marker = live.root.path().join("permission-answered");
        let command = format!(
            "printf 'Trust this workspace? [y/N]\\n'; IFS= read -r answer; printf '%s' \"$answer\" > '{}'",
            input_marker.display()
        );
        let trust = status_cli(
            &live,
            &[
                "terminal",
                "create",
                "--project",
                "fixture",
                "--workspace",
                "feature/setup",
                "--name",
                "trust-screen",
                "--label",
                "pi",
                "--",
                "/bin/sh",
                "-c",
                &command,
            ],
        );
        let id = trust["id"].as_u64().unwrap().to_string();
        let deadline = Instant::now() + BUDGET;
        loop {
            if status_cli(&live, &["terminal", "read", &id])["text"]
                .as_str()
                .unwrap()
                .contains("Trust this workspace?")
            {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        let (trust, reply) = workspaces_turn(
            &mut driver,
            &live,
            &provider,
            "trust",
            "Inspect trust-screen. Is the Agent ready? Surface unexpected permission questions; do not answer them.",
        );
        assert_eq!(trust["session"]["kind"], "terminal");
        assert!(reply.contains("not a verified Agent") && reply.contains("Trust this workspace?"));
        assert!(
            !input_marker.exists(),
            "inspection must not answer a permission question"
        );
        assert_eq!(
            workspace_session_identities(&trust["before"]["sessions"]),
            workspace_session_identities(&trust["after"]["sessions"])
        );
        assert_eq!(
            status_cli(&live, &["workspace", "list"])
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert!(
            Command::new("git")
                .env("HOME", live.root.path())
                .arg("-C")
                .arg(&live.repo)
                .args(["diff", "--exit-code"])
                .status()
                .unwrap()
                .success()
        );
        drop(driver);
        assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
        live.join();
        eprintln!(
            "INSTALLED_PI_WORKSPACES_REFUSALS_PASS provider_requests={} cases=4 permission_input=false shutdown=verified",
            provider.requests().len()
        );
    }

    fn control_turn(
        driver: &mut DashboardDriver,
        live: &live::Live,
        provider: &Provider,
        id: u64,
        run: u64,
        action: &str,
        quick: bool,
    ) -> Value {
        driver.reopen();
        let conversation = driver.conversation().id;
        let start = provider.requests().len();
        let request = format!("Control exact session {id} {run} {action}");
        if quick {
            driver.quick(&request);
        } else {
            driver.submit(&request);
        }
        let history = wait_terminal(driver, conversation, provider, live);
        wait_stopped(driver, conversation);
        let requests = provider.requests();
        assert_eq!(
            requests.len() - start,
            5,
            "one skill read, inventory, action, refresh, answer"
        );
        let body = &requests.last().unwrap().body;
        let result = marked_tool_json(body, "CONTROL_RESULT=");
        assert_eq!(result["instance"], live.config.to_string_lossy().as_ref());
        assert_eq!(
            result["executable"],
            live.executable.to_string_lossy().as_ref()
        );
        assert!(
            delta_text(&history).contains(&format!("Session {id} requested run {run}: {action}"))
        );
        result
    }

    fn wait_care_row(live: &live::Live, id: u64, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + BUDGET;
        loop {
            let rows = status_cli(live, &["terminal", "list"]);
            let row = rows
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == id)
                .unwrap();
            if predicate(row) {
                return row.clone();
            }
            assert!(
                Instant::now() < deadline,
                "session care observation unavailable: {row}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_session_care_control_and_native_resume() {
        let pi = pi_executable();
        let live = live::Live::idle().bounded();
        let provider = Provider::start(scenario_handler(
            live.repo.clone(),
            live.root.path().join("marker"),
            live.root.path().join("background"),
        ));
        provider.seed_profile(&live.config);
        provider.seed_workspace_profile(live.root.path());
        live.start_binary_env(&[
            ("OVRCR_PI_EXECUTABLE", pi.as_os_str()),
            ("PI_OFFLINE", OsStr::new("1")),
        ]);
        live.ready("care-control");
        let terminal = status_cli(
            &live,
            &[
                "terminal",
                "create",
                "--project",
                "fixture",
                "--workspace",
                "care-control",
                "--name",
                "care-terminal",
                "--",
                "/bin/sh",
            ],
        );
        let id = terminal["id"].as_u64().unwrap();
        let run = terminal["run"].as_u64().unwrap();
        live.own_group(terminal["pid"].as_u64().unwrap() as libc::pid_t);
        let mut driver = DashboardDriver::connect(&live);
        driver.select_fixture_workspace();
        driver.open();
        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run, "pause", false)["code"],
            0
        );
        assert_eq!(wait_care_row(&live, id, |_| true)["phase"], "paused");
        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run, "resume", true)["code"],
            0
        );
        assert_eq!(wait_care_row(&live, id, |_| true)["phase"], "running");
        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run, "stop", false)["code"],
            0
        );
        let stopped = wait_care_row(&live, id, |r| r["phase"] == "exited");
        assert_eq!(stopped["recovery"]["requires_ack"], false);
        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run, "reopen", true)["code"],
            0
        );
        let replacement = wait_care_row(&live, id, |r| r["run"] == run + 1);
        live.own_group(replacement["pid"].as_u64().unwrap() as libc::pid_t);
        let refused = control_turn(&mut driver, &live, &provider, id, run, "stop", false);
        assert_ne!(refused["code"], 0);
        assert!(refused["error"].as_str().unwrap().contains("Conflict"));
        assert_eq!(wait_care_row(&live, id, |_| true)["phase"], "running");
        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run + 1, "close", true)["code"],
            0
        );
        assert!(
            status_cli(&live, &["terminal", "list", "--archived"])
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == id)
        );

        // Real managed Pi creates its own history and certified Reporter reference.
        let native_history = live.root.path().join("native-history");
        let native = status_cli(
            &live,
            &[
                "terminal",
                "create",
                "--project",
                "fixture",
                "--workspace",
                "care-control",
                "--name",
                "care-native",
                "--",
                live.executable.to_str().unwrap(),
                "agent",
                "run",
                "pi",
                "--",
                pi.to_str().unwrap(),
                "--no-extensions",
                "--no-skills",
                "--no-context-files",
                "--no-prompt-templates",
                "--no-themes",
                "--no-approve",
                "--session-dir",
                native_history.to_str().unwrap(),
                "--",
                "CARE_NATIVE_SEED",
            ],
        );
        let id = native["id"].as_u64().unwrap();
        let run = native["run"].as_u64().unwrap();
        live.own_group(native["pid"].as_u64().unwrap() as libc::pid_t);
        wait_care_row(&live, id, |r| r["activity"] == "response_ready");
        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run, "stop", false)["code"],
            0
        );
        let stopped = wait_care_row(&live, id, |r| r["phase"] == "exited");
        assert_eq!(stopped["recovery"]["requires_ack"], false);
        let reference = stopped["recovery"]["conversation"].clone();
        assert!(!reference.is_null(), "native reference: {stopped}");
        let requests_before_resume = provider.requests().len();
        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run, "reopen", true)["code"],
            0
        );
        let resumed = wait_care_row(&live, id, |r| r["recovery"]["attached"] == true);
        assert_eq!(resumed["run"], run + 1);
        assert_eq!(resumed["recovery"]["conversation"], reference);
        live.own_group(resumed["pid"].as_u64().unwrap() as libc::pid_t);
        assert_eq!(
            provider.requests().len() - requests_before_resume,
            5,
            "resume must not replay a prompt"
        );
        let screen = driver.terminal_text(ovrcr::protocol::SessionId(id));
        assert!(
            screen.contains("CARE_NATIVE_HISTORY_PRESERVED"),
            "native history was not restored: {screen}"
        );

        assert_eq!(
            control_turn(&mut driver, &live, &provider, id, run + 1, "stop", false)["code"],
            0
        );
        let stopped = wait_care_row(&live, id, |r| r["phase"] == "exited");
        // The fixture alone makes its history unavailable, preserving its bytes.
        let files = std::fs::read_dir(&native_history)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(
            files.len(),
            1,
            "only the original native conversation exists"
        );
        let history_path = &files[0];
        assert_eq!(session_header(history_path)["id"], reference);
        let history_bytes = std::fs::read(history_path).unwrap();
        let saved = history_path.with_extension("saved");
        std::fs::rename(history_path, &saved).unwrap();
        let failed = control_turn(&mut driver, &live, &provider, id, run + 1, "reopen", false);
        assert_ne!(failed["code"], 0);
        let retained = wait_care_row(&live, id, |_| true);
        assert_eq!(retained["run"], stopped["run"]);
        assert_eq!(retained["phase"], "exited");
        assert_eq!(retained["recovery"]["conversation"], reference);
        assert!(!retained["recovery"]["failure"].is_null());
        assert_eq!(std::fs::read(saved).unwrap(), history_bytes);
        eprintln!(
            "INSTALLED_PI_SESSION_CONTROL_PASS terminal_actions=6 stale_refusals=1 native_resumes=1 recovery_failures=1 provider_requests={}",
            provider.requests().len()
        );
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_session_care_diagnosis() {
        use ovrcr::protocol::AgentProvider;
        let pi = pi_executable();
        let live = live::Live::idle().bounded();
        let provider = Provider::start(scenario_handler_mode(
            live.repo.clone(),
            live.root.path().join("marker"),
            live.root.path().join("background"),
            ScenarioMode {
                workspaces: true,
                ..ScenarioMode::default()
            },
        ));
        provider.seed_profile(&live.config);
        install_personal_skill(live.root.path());
        live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
        live.ready("care-work");
        let (interrupted, owner) = status_agent(&live, "care-interrupted");
        status_cli(&live, &["shutdown", "--kill"]);
        live.join();
        drop(owner);
        // As in retained_sessions::prepare_interrupted_claude: only after all
        // fixture processes stop, seed a same-boot interrupted retained run.
        // Same boot deliberately leaves ownership uncertain, preventing display recovery.
        let database =
            rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
        assert_eq!(
            database
                .execute(
                    "UPDATE retained_sessions SET stopped = 0, failure = NULL WHERE id = ?1",
                    [interrupted.0 as i64],
                )
                .unwrap(),
            1
        );
        drop(database);
        live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
        let mut owners = Vec::new();
        let mut cases = vec![(interrupted, "care-interrupted", "interrupted", "read")];
        for (name, native, condition, read) in [
            ("care-busy", AgentProvider::Pi, "busy", "read"),
            ("care-idle", AgentProvider::Pi, "idle", "read"),
            ("care-omp", AgentProvider::Omp, "idle", "screen"),
            (
                "care-transcript",
                AgentProvider::Codex,
                "response_ready",
                "transcript",
            ),
            (
                "care-truncated",
                AgentProvider::Codex,
                "waiting_input",
                "transcript",
            ),
            (
                "status-unavailable",
                AgentProvider::Codex,
                "unavailable reporting; activity unknown",
                "transcript",
            ),
            ("care-missing", AgentProvider::Codex, "busy", "transcript"),
            ("care-exited", AgentProvider::Pi, "exited", "read"),
        ] {
            let (id, owner) = reporter_agent(&live, name, native);
            owners.push(owner);
            cases.push((id, name, condition, read));
            // Identical misleading display titles cannot retarget the numeric request.
            status_cli(
                &live,
                &["terminal", "rename", &id.0.to_string(), "finished"],
            );
            if name == "care-exited" {
                status_cli(
                    &live,
                    &[
                        "terminal",
                        "send",
                        &id.0.to_string(),
                        "--text",
                        "fixture-exit",
                    ],
                );
                let deadline = Instant::now() + BUDGET;
                loop {
                    let rows = status_cli(&live, &["terminal", "list"]);
                    if rows
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|r| r["id"] == id.0 && r["phase"] == "exited")
                    {
                        break;
                    }
                    assert!(Instant::now() < deadline, "owned fixture did not exit");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        let before = status_cli(&live, &["terminal", "list"]);
        let projects = status_cli(&live, &["project", "list"]);
        let workspaces = status_cli(&live, &["workspace", "list"]);
        let histories: Vec<_> = ["care-transcript", "care-truncated"]
            .into_iter()
            .map(|name| {
                let path = live.root.path().join(format!("{name}.jsonl"));
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        let mut driver = DashboardDriver::connect(&live);
        driver.select_fixture_workspace();
        driver.open();
        let conversation = driver.conversation().id;
        for (index, (id, name, condition, read)) in cases.iter().enumerate() {
            driver.reopen();
            let request = format!(
                "{CARE_REQUEST}{} without changing it. Why does it appear stuck?",
                id.0
            );
            let start = provider.requests().len();
            if index % 2 == 0 {
                driver.submit(&request);
            } else {
                driver.quick(&request);
            }
            let history = wait_terminal(&mut driver, conversation, &provider, &live);
            wait_stopped(&mut driver, conversation);
            let requests = provider.requests();
            let turn_requests = &requests[start..];
            assert_eq!(
                turn_requests.len(),
                5,
                "{name}: instructions, inventory, content, refresh, answer"
            );
            if index == 0 {
                assert_normal_first_prompt(name, &turn_requests[0].body, &request);
            } else {
                assert!(
                    operator_provider::last_user_text(&turn_requests[0].body).ends_with(&request)
                );
                assert!(
                    operator_provider::non_system_text(&turn_requests[0].body)
                        .contains("Inspection only; no control or recovery performed."),
                    "resumed diagnosis retains the prior conversation"
                );
            }
            assert_skill_only_in_system(
                name,
                &turn_requests[0].body,
                &live.config.join("operator-pi"),
            );
            let body = &turn_requests.last().unwrap().body;
            let observed = marked_tool_json(body, "CARE_INVENTORY=");
            let expected = before
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id.0)
                .unwrap();
            assert_eq!(&observed["session"], expected);
            assert_eq!(observed["instance"], live.config.to_string_lossy().as_ref());
            assert_eq!(
                observed["executable"],
                live.executable.to_string_lossy().as_ref()
            );
            assert_eq!(observed["workspace"]["id"], "work");
            let content = marked_tool_json(body, "CARE_CONTENT=");
            assert_eq!(content["session"], id.0);
            assert_eq!(content["command"], *read);
            let refreshed = marked_tool_json(body, "CARE_REFRESH=");
            assert_eq!(&refreshed["session"], expected);
            let turn = history
                .conversation
                .current_turn
                .as_ref()
                .unwrap()
                .identity
                .turn;
            let answer: String = history
                .messages
                .iter()
                .filter(|message| message.turn == turn)
                .filter_map(|message| match &message.content {
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect();
            assert!(
                answer.contains(&format!("Known: {condition}")),
                "{name}: {answer}"
            );
            assert!(
                answer.contains(&format!("session {}, run {}", id.0, expected["run"])),
                "{answer}"
            );
            assert!(answer.contains("Unknown:") && answer.contains("Next supported action:"));
            match *name {
                "care-interrupted" | "care-exited" => {
                    assert_eq!(expected["recovery"]["requires_ack"], true);
                    assert!(
                        answer.contains("human ownership verification required"),
                        "{answer}"
                    );
                }
                "care-transcript" => {
                    assert!(
                        content["output"]
                            .as_str()
                            .unwrap()
                            .contains("Diagnosis fixture reply")
                    );
                    assert!(expected["unread"].is_object());
                    assert!(answer.contains("Unread unchanged"));
                    assert!(
                        answer
                            .contains("Diagnosis fixture reply: checks finished; review required.")
                    );
                }
                "care-truncated" => {
                    assert!(content["output"].as_str().unwrap().contains('…'));
                    assert!(
                        !content["output"]
                            .as_str()
                            .unwrap()
                            .contains("PRIVATE_CLIPPED_TAIL")
                    );
                    assert!(
                        refreshed["fallback"]["output"]
                            .as_str()
                            .unwrap()
                            .contains("Observed Agent screen: care-truncated")
                    );
                    assert!(
                        answer.contains("transcript truncated")
                            && answer.contains("human answer required")
                            && answer.contains("Which deployment target?")
                    );
                    assert!(
                        !expected["agent"]["input_requests"]
                            .as_array()
                            .unwrap()
                            .is_empty()
                    );
                }
                "care-missing" | "status-unavailable" => {
                    assert_ne!(
                        content["code"], 0,
                        "a nonexistent transcript must be refused"
                    );
                    assert!(answer.contains("content unavailable"));
                    assert_eq!(refreshed["fallback"]["code"], 0);
                }
                _ => assert!(
                    answer.contains(&format!("Observed Agent screen: {name}")),
                    "{answer}"
                ),
            }
            let outputs = tool_outputs_with_status(&history);
            assert!(
                outputs
                    .iter()
                    .all(|(_, _, status)| *status == OperatorToolStatus::Succeeded)
            );
            assert!(!operator_provider::non_system_text(body).contains(PERSONAL_SKILL_SENTINEL));
            driver.reopen();
            assert!(
                driver.screen().contains(&format!("Known: {condition}")),
                "{name}: diagnosis must render"
            );
            let after = status_cli(&live, &["terminal", "list"]);
            let rows: Vec<_> = after
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["scope"] == "Workspace")
                .cloned()
                .collect();
            assert_eq!(
                Value::Array(rows),
                before,
                "{name}: process/run, attention and ownership unchanged"
            );
            eprintln!(
                "INSTALLED_PI_SESSION_CARE_CASE name={name} session={} run={} quick={} condition={condition} read={read} requests=5 unchanged=true",
                id.0,
                expected["run"],
                index % 2 != 0
            );
        }
        assert_eq!(status_cli(&live, &["project", "list"]), projects);
        assert_eq!(status_cli(&live, &["workspace", "list"]), workspaces);
        for (path, bytes) in histories {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        for (_, name, _, _) in &cases {
            let input = live.root.path().join(format!("{name}.identity.input"));
            if *name == "care-exited" {
                assert_eq!(std::fs::read_to_string(input).unwrap(), "fixture-exit\n");
            } else {
                assert!(!input.exists(), "diagnosis sent input to {name}");
            }
        }
        assert!(!driver.conversation().worker_live);
        eprintln!(
            "INSTALLED_PI_SESSION_CARE_PASS cases={} requests={} full_and_quick=true unchanged=true",
            cases.len(),
            provider.requests().len()
        );
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_status_invalid_bundle_prevents_prompting() {
        check_invalid_bundle(false);
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_explicit_skill_invalid_bundle() {
        check_invalid_bundle(true);
    }

    fn check_invalid_bundle(explicit: bool) {
        let pi = pi_executable();
        for invalid in [
            "document-directory",
            "reference-symlink",
            "writable-directory",
            "blocked-reference-parent",
        ] {
            for quick in [false, true] {
                let live = live::Live::idle().bounded();
                let provider =
                    Provider::start(Box::new(|_, _| Plan::text("UNEXPECTED_MODEL_WORK")));
                provider.seed_profile(&live.config);
                let directory = live.config.join("operator-pi/ovrcr-status");
                std::fs::create_dir(&directory).unwrap();
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
                match invalid {
                    "document-directory" => {
                        std::fs::create_dir(directory.join("SKILL.md")).unwrap()
                    }
                    "reference-symlink" => {
                        let outside = live.root.path().join("outside");
                        std::fs::create_dir(&outside).unwrap();
                        std::fs::write(outside.join("inventory.md"), "must survive").unwrap();
                        std::os::unix::fs::symlink(&outside, directory.join("references")).unwrap();
                    }
                    "writable-directory" => {
                        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o777))
                            .unwrap()
                    }
                    "blocked-reference-parent" => {
                        std::fs::write(directory.join("references"), "not a directory").unwrap()
                    }
                    _ => unreachable!(),
                }
                live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
                let mut driver = DashboardDriver::connect(&live);
                let request = if explicit {
                    format!("/skill:ovrcr-status {STATUS_REQUEST}")
                } else {
                    STATUS_REQUEST.into()
                };
                if quick {
                    driver.quick(&request);
                } else {
                    driver.open();
                    driver.submit(&request);
                }
                let opened = driver.conversation();
                let failed = driver.wait_history(opened.id, |history| {
                    history
                        .conversation
                        .current_turn
                        .as_ref()
                        .is_some_and(|turn| turn.state == OperatorTurnState::Error)
                });
                wait_stopped(&mut driver, opened.id);
                assert!(
                    provider.requests().is_empty(),
                    "{invalid}: no model request may precede complete asset delivery"
                );
                assert!(tool_outputs_with_status(&failed).is_empty());
                let screen = driver.screen();
                let error = if invalid == "document-directory" {
                    "Pi Operator integration is unavailable; no prompt was sent."
                } else {
                    "Operator Pi context is unavailable; saved data was preserved."
                };
                assert!(screen.contains(error), "{screen}");
                assert!(
                    screen.contains(STATUS_REQUEST),
                    "the failed request remains recoverable in the journal: {screen}"
                );
                if invalid == "reference-symlink" {
                    assert_eq!(
                        std::fs::read_to_string(live.root.path().join("outside/inventory.md"))
                            .unwrap(),
                        "must survive"
                    );
                }
                eprintln!(
                    "INSTALLED_PI_INVALID_BUNDLE_PASS case={invalid} provider_requests=0 quick={quick} explicit={explicit}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_status_overview() {
        check_status_overview(false);
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_explicit_skill_lifetime() {
        check_status_overview(true);
    }

    fn check_status_overview(explicit: bool) {
        let selected = |request: &str| {
            if explicit {
                format!("/skill:ovrcr-status {request}")
            } else {
                request.to_owned()
            }
        };
        let pi = pi_executable();
        for quick in [false, true] {
            let live = live::Live::idle().bounded();
            let provider = Provider::start(scenario_handler(
                live.repo.clone(),
                live.root.path().join("marker"),
                live.root.path().join("background"),
            ));
            provider.seed_profile_with(
                &live.config,
                200_000,
                Some(json!({"enabled": false, "reserveTokens": 1024, "keepRecentTokens": 1})),
            );
            install_personal_skill(live.root.path());
            for (path, name) in [
                (
                    live.config.join("operator-pi/unregistered-sentinel"),
                    "unregistered-sentinel",
                ),
                (
                    live.config.join("operator-pi/skills/unregistered-sentinel"),
                    "unregistered-sentinel",
                ),
                (
                    live.config
                        .join("operator-workdir/.agents/skills/project-sentinel"),
                    "project-sentinel",
                ),
            ] {
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(path.join("SKILL.md"), format!("---\nname: {name}\ndescription: forbidden ambient discovery\n---\nNot registered.\n")).unwrap();
            }
            live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
            live.ready("status-work");
            let other = live.root.path().join("other-repo");
            let clone = live.command().get_program().to_os_string();
            // A second registered Project outside the captured target proves
            // instance-wide default scope rather than an empty metadata demo.
            assert!(
                Command::new("git")
                    .args(["clone", "--quiet"])
                    .arg(&live.repo)
                    .arg(&other)
                    .status()
                    .unwrap()
                    .success()
            );
            let output = live
                .command()
                .args(["project", "add", "other"])
                .arg(&other)
                .arg("--workspace-root")
                .arg(live.root.path().join("other-workspaces"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let (ready, _ready_owner) = status_agent(&live, "status-ready");
            let (input, _input_owner) = status_agent(&live, "status-input");
            let (unavailable, _unavailable_owner) = status_agent(&live, "status-unavailable");
            let before = status_cli(&live, &["terminal", "list"]);
            assert!(
                before
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["id"] == ready.0 && row["unread"].is_object())
            );
            assert!(
                before
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["id"] == input.0 && row["activity"] == "waiting_input")
            );
            assert!(
                before
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["id"] == unavailable.0 && row["reporting_unavailable"] == true)
            );
            let projects_before = status_cli(&live, &["project", "list"]);
            let workspaces_before = status_cli(&live, &["workspace", "list"]);
            let mut driver = DashboardDriver::connect(&live);
            driver.select_fixture_workspace();
            if quick {
                driver.quick(&selected(STATUS_REQUEST));
            } else {
                driver.open();
                driver.submit(&selected(STATUS_REQUEST));
            }
            let opened = driver.conversation();
            let history = wait_terminal(&mut driver, opened.id, &provider, &live);
            wait_stopped(&mut driver, opened.id);
            let answer = delta_text(&history);
            for expected in [
                "Scope: all registered Projects in this instance",
                "Projects: fixture, other",
                "5/50; 45 remaining",
                "Operator: separate slot",
                "Unread result needs review",
                "waiting_input",
                "unavailable reporting; activity unknown",
            ] {
                assert!(answer.contains(expected), "missing {expected}: {answer}");
            }
            let first = &provider.requests()[0].body;
            if explicit {
                assert!(
                    operator_provider::last_user_text(first)
                        .contains(include_str!("../pi/skills/ovrcr-status/SKILL.md"))
                );
                assert!(matches!(
                    history.messages[0].content,
                    OperatorMessageContent::User { skill: Some(_), .. }
                ));
            }
            assert_normal_first_prompt("status", first, STATUS_REQUEST);
            assert!(
                operator_provider::last_user_text(first).contains("\"workspace\":\"status-work\""),
                "captured Workspace must not narrow the default overview"
            );
            assert_skill_only_in_system("status", first, &live.config.join("operator-pi"));
            let outputs = tool_outputs_with_status(&history);
            assert_eq!(
                outputs.len(),
                3,
                "only instructions, supporting reference and metadata were read"
            );
            assert!(
                outputs
                    .iter()
                    .all(|(_, _, status)| *status == OperatorToolStatus::Succeeded)
            );
            let inventory = marked_tool_json(
                &provider.requests().last().unwrap().body,
                "STATUS_INVENTORY=",
            );
            assert_eq!(inventory["executable"], clone.to_string_lossy().as_ref());
            assert_eq!(
                inventory["instance"],
                live.config.to_string_lossy().as_ref()
            );
            assert_eq!(inventory["projects"], projects_before);
            // A narrower resumed request reads the same registered instructions.
            driver.reopen();
            assert!(
                driver.screen().contains("Scope: all registered Projects"),
                "full Operator renders the overview from either entry point"
            );
            for (request, scope) in [
                (STATUS_PROJECT_REQUEST, "Scope: Project fixture"),
                (STATUS_WORKSPACE_REQUEST, "Scope: Workspace status-work"),
                (STATUS_SESSION_REQUEST, "Scope: session status-ready"),
            ] {
                let previous = driver.conversation().current_turn.unwrap().identity.turn.0;
                let narrowed = submit_next(
                    &mut driver,
                    opened.id,
                    &selected(request),
                    previous,
                    &provider,
                    &live,
                );
                wait_stopped(&mut driver, opened.id);
                let turn = narrowed
                    .conversation
                    .current_turn
                    .as_ref()
                    .unwrap()
                    .identity
                    .turn;
                let reply: String = narrowed
                    .messages
                    .iter()
                    .filter(|message| message.turn == turn)
                    .filter_map(|message| match &message.content {
                        OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                            text,
                        )) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(
                    reply.contains(scope) && !reply.contains("Projects: fixture, other"),
                    "{reply}"
                );
                if request == STATUS_SESSION_REQUEST {
                    assert!(
                        reply.contains("Sessions: 1.") && !reply.contains("status-input"),
                        "{reply}"
                    );
                }
            }
            // An explicitly requested existing coordinator watch keeps Pi warm;
            // status itself must neither create nor remove a lease.
            driver.submit(STATUS_WATCH_REQUEST);
            let watched = wait_terminal(&mut driver, opened.id, &provider, &live);
            assert!(watched.conversation.worker_live);
            let warm_run = watched.conversation.run;
            if explicit {
                assert_explicit_selection_refused(&mut driver, &provider);
            }
            let document = live.config.join("operator-pi/ovrcr-status/SKILL.md");
            let inode = std::fs::metadata(&document).unwrap().ino();
            let previous = watched.conversation.current_turn.unwrap().identity.turn.0;
            let warm = submit_next(
                &mut driver,
                opened.id,
                &selected(STATUS_REQUEST),
                previous,
                &provider,
                &live,
            );
            assert_eq!(warm.conversation.run, warm_run);
            assert!(warm.conversation.worker_live);
            assert_eq!(
                std::fs::metadata(&document).unwrap().ino(),
                inode,
                "warm turns do not rematerialize instructions"
            );
            // Native compaction within the same leased worker keeps metadata and
            // actual on-demand instruction reads consistent.
            driver.submit("/compact");
            let compacted = wait_terminal(&mut driver, opened.id, &provider, &live);
            assert_eq!(compacted.conversation.run, warm_run);
            assert!(compacted.conversation.worker_live);
            let previous = driver.conversation().current_turn.unwrap().identity.turn.0;
            let after_compact = submit_next(
                &mut driver,
                opened.id,
                &selected(STATUS_REQUEST),
                previous,
                &provider,
                &live,
            );
            assert_eq!(after_compact.conversation.run, warm_run);
            assert_eq!(std::fs::metadata(&document).unwrap().ino(), inode);
            driver.submit(STATUS_UNWATCH_REQUEST);
            wait_terminal(&mut driver, opened.id, &provider, &live);
            wait_stopped(&mut driver, opened.id);
            assert_eq!(
                driver.session(ready).phase,
                SessionPhase::Running,
                "removing a watch never stops the Agent"
            );
            // Only a later cold start refreshes stale copies, including required
            // references; it does not depend on the source checkout.
            std::fs::write(&document, "stale skill from a previous binary").unwrap();
            let reference = document.parent().unwrap().join("references/inventory.md");
            std::fs::write(&reference, "stale reference").unwrap();
            let previous = driver.conversation().current_turn.unwrap().identity.turn.0;
            submit_next(
                &mut driver,
                opened.id,
                &selected(STATUS_REQUEST),
                previous,
                &provider,
                &live,
            );
            wait_stopped(&mut driver, opened.id);
            assert_eq!(
                std::fs::read_to_string(document).unwrap(),
                include_str!("../pi/skills/ovrcr-status/SKILL.md")
            );
            assert_eq!(
                std::fs::read_to_string(reference).unwrap(),
                include_str!("../pi/skills/ovrcr-status/references/inventory.md")
            );
            assert!(
                provider
                    .requests()
                    .iter()
                    .any(|request| operator_provider::is_compaction_request(&request.body))
            );
            for request in provider
                .requests()
                .iter()
                .filter(|request| !operator_provider::is_compaction_request(&request.body))
            {
                if explicit {
                    let user = operator_provider::last_user_text(&request.body);
                    if user.contains("Selected Operator skill: ovrcr-status") {
                        assert!(user.contains(include_str!("../pi/skills/ovrcr-status/SKILL.md")));
                        assert!(
                            user.find("Captured OVRCR context").unwrap()
                                < user.find("Selected Operator skill:").unwrap()
                        );
                        assert!(
                            user.find("Selected Operator skill:").unwrap()
                                < user.find("User request:").unwrap()
                        );
                    }
                }
                assert_skill_only_in_system(
                    "status lifetime",
                    &request.body,
                    &live.config.join("operator-pi"),
                );
            }
            let after = status_cli(&live, &["terminal", "list"]);
            let workspace_rows: Vec<_> = after
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["scope"] == "Workspace")
                .cloned()
                .collect();
            assert_eq!(
                Value::Array(workspace_rows),
                before,
                "status preserves runs, activity, Unread, Input requests and ownership"
            );
            assert_eq!(status_cli(&live, &["project", "list"]), projects_before);
            assert_eq!(status_cli(&live, &["workspace", "list"]), workspaces_before);
            assert!(
                !driver.conversation().worker_live,
                "status creates no periodic checks or watch lease"
            );
            eprintln!(
                "INSTALLED_PI_STATUS_PASS quick={quick} requests={} sessions=5 scope_cases=4 compacted=true",
                provider.requests().len()
            );
        }
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_explicit_skill() {
        let pi = pi_executable();
        for quick in [false, true] {
            let live = live::Live::idle().bounded();
            let provider = Provider::start(Box::new(|body, _| {
                let user = operator_provider::last_user_text(body);
                if user.contains("Selected Operator skill: ovrcr-coordinator")
                    && user.ends_with("User request:\n")
                {
                    Plan::text("Which tasks and Workspaces should I coordinate?")
                } else {
                    Plan::text("EXPLICIT_SKILL_DONE")
                }
            }));
            provider.seed_profile(&live.config);
            live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
            let mut driver = DashboardDriver::connect(&live);
            if quick {
                driver.quick("/skill:missing UNSENT_REQUEST");
            } else {
                driver.open();
                driver.submit("/skill:missing UNSENT_REQUEST");
            }
            assert!(provider.requests().is_empty());
            let screen = driver.screen();
            assert!(
                screen.contains("unavailable Operator skill") && screen.contains("UNSENT_REQUEST"),
                "{screen}"
            );
            driver.dashboard.ctrl('c');
            driver.submit("/reload UNSENT_NATIVE_COMMAND");
            assert!(provider.requests().is_empty());
            assert!(driver.screen().contains("Unknown slash command"));
            driver.dashboard.ctrl('c');
            driver.submit("/skill:ovrcr-status EXPLICIT_REQUEST");
            let opened = driver.conversation();
            let history = wait_terminal(&mut driver, opened.id, &provider, &live);
            assert!(delta_text(&history).contains("EXPLICIT_SKILL_DONE"));
            wait_stopped(&mut driver, opened.id);
            let requests = provider.requests();
            assert_eq!(requests.len(), 1);
            let user = operator_provider::last_user_text(&requests[0].body);
            let document = include_str!("../pi/skills/ovrcr-status/SKILL.md");
            assert!(user.starts_with("Captured OVRCR context"), "{user}");
            let instructions = user
                .find(document)
                .expect("full selected document before model work");
            let request = user.find("User request:\nEXPLICIT_REQUEST").unwrap();
            assert!(
                instructions < request,
                "instructions precede distinct request"
            );
            assert!(
                user.contains(
                    &live
                        .config
                        .join("operator-pi/ovrcr-status/SKILL.md")
                        .display()
                        .to_string()
                )
            );
            assert!(!operator_harness::history_text(&history).contains(document));
            assert_explicit_selection_refused(&mut driver, &provider);
            // A fresh Dashboard reconnects to the completed checkpoint. A bare
            // invocation supplies instructions, but no invented task or authority.
            drop(driver);
            let mut driver = DashboardDriver::connect(&live);
            driver.open();
            driver.submit("/skill:ovrcr-coordinator");
            let bare = wait_terminal(&mut driver, opened.id, &provider, &live);
            wait_stopped(&mut driver, opened.id);
            assert!(delta_text(&bare).contains("Which tasks and Workspaces"));
            assert!(driver.screen().contains("/skill:ovrcr-coordinator"));
            let requests = provider.requests();
            let user = operator_provider::last_user_text(&requests.last().unwrap().body);
            assert!(user.contains(include_str!("../pi/skills/ovrcr-coordinator/SKILL.md")));
            assert!(
                user.ends_with("User request:\n"),
                "bare selection invents no user task"
            );
            assert_eq!(
                operator_provider::non_system_text(&requests.last().unwrap().body)
                    .matches(document)
                    .count(),
                1,
                "resume preserves the original instruction message without replay"
            );
            for text in ["/etc/hosts", " /new", "ordinary /skill:missing data"] {
                driver.submit(text);
                wait_terminal(&mut driver, opened.id, &provider, &live);
                wait_stopped(&mut driver, opened.id);
                let requests = provider.requests();
                let user = operator_provider::last_user_text(&requests.last().unwrap().body);
                assert!(user.starts_with("Captured OVRCR context"));
                assert!(user.ends_with(&format!("User request:\n{text}")), "{user}");
                assert!(!user.contains("Selected Operator skill:"));
            }
        }
    }

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_skills_picker() {
        let pi = pi_executable();
        for quick in [false, true] {
            let live = live::Live::idle().bounded();
            let provider = Provider::start(scenario_handler(
                live.repo.clone(),
                live.root.path().join("marker"),
                live.root.path().join("background"),
            ));
            provider.seed_profile(&live.config);
            install_personal_skill(live.root.path());
            live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
            live.ready("skill-picker");
            let (target, _owner) = status_agent(&live, "status-unavailable");
            let before = status_cli(&live, &["terminal", "list"]);
            let mut driver = DashboardDriver::connect(&live);
            driver.dashboard.install_focus(target);
            if quick {
                driver.quick("/skills GUI_SKILLS_REQUEST");
            } else {
                driver.open();
                driver.submit("/skills GUI_SKILLS_REQUEST");
            }
            let conversation = driver.conversation();
            let screen = driver.screen();
            assert!(screen.contains("Operator skills"), "{screen}");
            for skill in ovrcr_protocol::operator::skills::BUNDLED_SKILLS {
                assert!(
                    screen.contains(skill.name),
                    "{} missing: {screen}",
                    skill.name
                );
            }
            assert!(!screen.contains("personal-sentinel") && !screen.contains("project-sentinel"));
            assert!(provider.requests().is_empty());
            assert!(conversation.current_turn.is_none());
            driver
                .dashboard
                .event_action(Event::Paste("INVENTORY".into()));
            let filtered = driver.screen();
            assert!(
                filtered.contains("ovrcr-status") && !filtered.contains("ovrcr-coordinator"),
                "{filtered}"
            );
            driver.dashboard.key(KeyCode::Esc);
            assert!(driver.screen().contains("/skills GUI_SKILLS_REQUEST"));
            driver.dashboard.key(KeyCode::Enter);
            driver
                .dashboard
                .event_action(Event::Paste("ovrcr-status".into()));
            driver.dashboard.key(KeyCode::Enter);
            driver.flush();
            assert!(
                driver
                    .screen()
                    .contains("/skill:ovrcr-status GUI_SKILLS_REQUEST")
            );
            assert!(provider.requests().is_empty(), "choosing must not prompt");
            assert!(driver.conversation().current_turn.is_none());
            assert!(
                !live
                    .root
                    .path()
                    .join("status-unavailable.identity.input")
                    .exists()
            );
            driver.dashboard.key(KeyCode::Enter);
            driver.flush();
            let history = wait_terminal(&mut driver, conversation.id, &provider, &live);
            wait_stopped(&mut driver, conversation.id);
            assert!(delta_text(&history).contains("SKILLS_PICKER_DONE"));
            let requests = provider.requests();
            assert_eq!(requests.len(), 1);
            let user = operator_provider::last_user_text(&requests[0].body);
            let document = include_str!("../pi/skills/ovrcr-status/SKILL.md");
            assert!(user.starts_with("Captured OVRCR context"));
            assert!(
                user.find(document).unwrap()
                    < user.find("User request:\nGUI_SKILLS_REQUEST").unwrap()
            );
            let (context, selection) = history
                .messages
                .iter()
                .find_map(|message| match &message.content {
                    OperatorMessageContent::User {
                        text,
                        context,
                        skill,
                    } if text == "GUI_SKILLS_REQUEST" => Some((context, skill)),
                    _ => None,
                })
                .unwrap();
            assert_eq!(context.session, Some(target));
            assert_eq!(context.workspace_id.as_deref(), Some("work"));
            assert_eq!(
                context.cwd.as_deref(),
                Some(live.workspace_path("work").as_path())
            );
            assert_eq!(
                selection.as_ref().unwrap(),
                &ovrcr_protocol::OperatorSkillSelection::named("ovrcr-status").unwrap()
            );
            assert!(user.contains(&serde_json::to_string(context).unwrap()));
            assert_explicit_selection_refused(&mut driver, &provider);
            driver.submit("/skill:unavailable GUI_SKILLS_UNSENT");
            let screen = driver.screen();
            assert!(
                screen.contains("unavailable Operator skill")
                    && screen.contains("GUI_SKILLS_UNSENT"),
                "{screen}"
            );
            assert_eq!(provider.requests().len(), 1);
            assert!(
                !live
                    .root
                    .path()
                    .join("status-unavailable.identity.input")
                    .exists()
            );
            let after = status_cli(&live, &["terminal", "list"]);
            for row in before.as_array().unwrap() {
                assert_eq!(
                    after
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|after| after["id"] == row["id"])
                        .unwrap(),
                    row
                );
            }
            eprintln!(
                "SKILLS_PICKER_PASS quick={quick} model_requests=1 selection_and_cancel_requests=0 target={}",
                target.0
            );
        }
    }

    fn assert_explicit_selection_refused(driver: &mut DashboardDriver, provider: &Provider) {
        let before = driver.conversation();
        let count = provider.requests().len();
        for (name, revision) in [
            ("ovrcr-status", 0),
            ("ovrcr-status", 999),
            ("not-shipped", 1),
        ] {
            let response = driver.exchange(Request::Operator(OperatorRequest::Submit {
                conversation: before.id,
                expected_run: before.run,
                text: "DO_NOT_RUN".into(),
                context: Default::default(),
                skill: Some(ovrcr::protocol::OperatorSkillSelection {
                    name: name.into(),
                    revision,
                }),
            }));
            assert!(matches!(response, Response::Error { .. }), "{response:?}");
            assert_eq!(driver.conversation().current_turn, before.current_turn);
            assert_eq!(
                provider.requests().len(),
                count,
                "refusal before model work"
            );
        }
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

    #[test]
    #[ignore = "requires OVRCR_TEST_PI_EXECUTABLE and a loopback provider; opt in explicitly"]
    fn installed_pi_operator_tree_branch_summarization_and_slash_commands() {
        let pi = pi_executable();
        let live = live::Live::idle().bounded();
        let workspace = live.root.path().join("operator-target");
        std::fs::create_dir(&workspace).unwrap();
        let marker = workspace.join("slash-marker.txt");
        let background_pid = workspace.join("slash-background.pid");
        let provider = Provider::start(scenario_handler(workspace.clone(), marker, background_pid));
        provider.seed_profile_with(
            &live.config,
            200_000,
            Some(json!({"enabled": false, "reserveTokens": 1024, "keepRecentTokens": 1})),
        );
        live.start_binary_env(&launcher_environment(&pi));

        let mut driver = DashboardDriver::connect(&live);
        driver.open();
        let opened = driver.conversation();

        // 1. `/model` discovers both synthetic loopback models (`operator-fixture` + `operator-fixture-fast`).
        driver.submit("/model");
        let model_screen = driver.screen();
        assert!(
            model_screen.contains("Harness default")
                && model_screen.contains("fixture/operator-fixture")
                && model_screen.contains("fixture/operator-fixture-fast"),
            "installed pi `/model` picker must discover loopback catalog:\n{model_screen}"
        );
        driver.dashboard.key(KeyCode::Esc);

        // 2. Seed Turn 1 (`GUI_SLASH_SEED`) and Turn 2 (`GUI_BRANCH_ALPHA`).
        driver.submit("GUI_SLASH_SEED");
        let seed_history = wait_terminal(&mut driver, opened.id, &provider, &live);
        assert!(delta_text(&seed_history).contains("Seeded root turn"));
        wait_stopped(&mut driver, opened.id);

        driver.submit("GUI_BRANCH_ALPHA");
        let alpha_history = wait_terminal(&mut driver, opened.id, &provider, &live);
        assert!(delta_text(&alpha_history).contains("Alpha branch exploration"));
        wait_stopped(&mut driver, opened.id);

        // 3. `/compact` runs real manual compaction against installed `pi 0.87.1`.
        driver.submit("/compact focus on protocol 88");
        let compacted = driver.wait_history(opened.id, |history| {
            history.conversation.worker_stopped
                && history.messages.iter().any(|msg| {
                    matches!(
                        &msg.content,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionSummary { summary, .. })
                            if summary.contains("FIXTURE_COMPACTION_SUMMARY")
                    )
                })
        });
        for _ in 0..4 {
            driver.pump();
        }
        let compacted_screen = driver.screen();
        assert!(
            compacted_screen.contains("Compacted from")
                && compacted_screen.contains("FIXTURE_COMPACTION_SUMMARY"),
            "expected `/compact` receipt in installed-pi Dashboard:\n{compacted_screen}"
        );
        let _ = compacted;

        // 4. Open `/tree`, select user node `GUI_BRANCH_ALPHA`, choose `Summarize with custom prompt`,
        //    and verify real `pi 0.87.1` `ctx.navigateTree(..., { summarize: true, customInstructions })`
        //    appends a `branch_summary` entry, updates the feed, and restores `GUI_BRANCH_ALPHA` into the composer!
        driver.submit("/tree");
        let tree_screen = driver.screen();
        assert!(
            tree_screen.contains("GUI_SLASH_SEED")
                && tree_screen.contains("GUI_BRANCH_ALPHA")
                && tree_screen.contains("[compaction]:"),
            "expected real session.jsonl entries in `/tree`:\n{tree_screen}"
        );
        driver.dashboard.key(KeyCode::Tab);
        driver.dashboard.key(KeyCode::Tab);
        driver
            .dashboard
            .event_action(Event::Paste("GUI_BRANCH_ALPHA".into()));
        driver.dashboard.key(KeyCode::Enter);
        assert!(
            driver.screen().contains("Summarize branch? · /tree"),
            "expected Summarize branch? modal on installed pi"
        );
        driver.dashboard.key(KeyCode::Down);
        driver.dashboard.key(KeyCode::Down);
        driver.dashboard.key(KeyCode::Enter);
        driver
            .dashboard
            .event_action(Event::Paste("keep protocol notes".into()));
        driver.dashboard.key(KeyCode::Enter);
        driver.flush();

        let summarized = driver.wait_history(opened.id, |history| {
            history.conversation.worker_stopped && history.messages.iter().any(|msg| {
                matches!(
                    &msg.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::BranchSummary { summary })
                        if summary.contains("Summarized abandoned branch (keep protocol notes).")
                )
            })
        });
        for _ in 0..4 {
            driver.pump();
        }
        let after_summary = driver.screen();
        assert!(
            after_summary.contains("Branch summary recorded")
                && after_summary.contains("Summarized abandoned branch (keep protocol notes).")
                && after_summary.contains("> GUI_BRANCH_ALPHA▏"),
            "installed pi branch summary must render receipt and restore user prompt to composer:\n{after_summary}"
        );

        // 5. Submit Turn 4 (`GUI_BRANCH_BETA`) on the new summarized branch and verify the provider
        //    receives `pi`'s native `<branch_summary>` message in the next model request!
        driver.dashboard.ctrl('c');
        driver.submit("GUI_BRANCH_BETA");
        let beta_history = wait_terminal(&mut driver, opened.id, &provider, &live);
        assert!(delta_text(&beta_history).contains("Beta branch exploration"));
        wait_stopped(&mut driver, opened.id);

        let last_request = provider.requests().pop().expect("beta request");
        let payload = operator_provider::non_system_text(&last_request.body);
        assert!(
            payload.contains("Summarized abandoned branch (keep protocol notes)."),
            "installed pi must include the branch_summary in subsequent branch context:\n{payload}"
        );
        let _ = summarized;
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
        let slash =
            std::env::var_os("OVRCR_OPERATOR_GUI_SLASH").is_some_and(|value| !value.is_empty());
        let workspaces = std::env::var_os("OVRCR_OPERATOR_GUI_WORKSPACES")
            .is_some_and(|value| !value.is_empty());
        assert!(
            usize::from(compaction)
                + usize::from(coordinator)
                + usize::from(slash)
                + usize::from(workspaces)
                <= 1,
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
                workspaces,
            },
        ));
        if compaction {
            provider.seed_profile_with(
                &root,
                COMPACTION_CONTEXT_WINDOW,
                Some(json!({"enabled": true, "reserveTokens": 0, "keepRecentTokens": 0})),
            );
        } else if slash || workspaces {
            provider.seed_profile_with(
                &root,
                200_000,
                Some(json!({"enabled": false, "reserveTokens": 1024, "keepRecentTokens": 1})),
            );
        } else {
            provider.seed_profile(&root);
        }
        if coordinator || workspaces {
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
            if coordinator {
                let sessions = create_gui_coordinator_sessions(&root, &pi_executable());
                eprintln!("OPERATOR_PI_GUI_COORDINATOR_SESSIONS {sessions}");
            }
        }
        let _ = std::fs::remove_file(&release_marker);
        let _ = std::fs::remove_file(&stop_marker);
        println!(
            "OPERATOR_PI_GUI_READY base={} root={} compaction={compaction} coordinator={coordinator} workspaces={workspaces}",
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
