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
    Response, SessionId,
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
    // The resumed attempt holds only the copied base context plus this attempt's
    // own writes; it is never adopted as the checkpoint.
    assert!(
        std::fs::read_to_string(attempt_two.join("session.jsonl"))
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
        std::fs::read_to_string(checkpoint_file(&live, turn_three.identity.turn.0))
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
        vec!["high".to_owned()],
        "the follow-up restores the fresh profile thinking level, not the resumed one"
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
    use super::{live, operator_harness, operator_provider};
    use crossterm::event::{Event, KeyCode};
    use operator_provider::{Chunk, Plan, Provider};
    use ovrcr::protocol::{
        self, ClientMessage, OperatorConversationId, OperatorHistory, OperatorMessageContent,
        OperatorToolStatus, OperatorTurnState, OperatorWorkerEvent, Request, Response,
        ServerMessage, SessionPhase, TerminalSize,
    };
    use ovrcr::tui::{Dashboard, Settings};
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
                OperatorMessageContent::Worker(OperatorWorkerEvent::Completed)
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

    /// Scripted operator/tool scenario shared by the installed-pi test and the
    /// GUI provider. Every turn is identified by its user request marker.
    pub(super) fn scenario_handler(
        workspace: PathBuf,
        marker: PathBuf,
        background_pid: PathBuf,
    ) -> operator_provider::Handler {
        let counters: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
        Box::new(move |body: &Value, _index: usize| {
            if operator_provider::is_compaction_request(body) {
                return Plan::text("FIXTURE_COMPACTION_SUMMARY");
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
                ("COMPACT_ONE", 0) => Plan::text("COMPACT_ONE_DONE"),
                ("COMPACT_TWO", 0) => Plan::text("COMPACT_TWO_DONE"),
                _ => Plan::text("FIXTURE_FOLLOWUP_DONE"),
            }
        })
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
        // so this fails the turn unless the extension registered all four.
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
        let timeout_turn = pending_timeout
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
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
            timeout_attempt.join("session.jsonl").is_file(),
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

    /// A tiny context window forces Pi's own automatic compaction; the fixture
    /// provider recognizes the summarization call. The conversation and native
    /// session must survive it unchanged.
    fn check_compaction(pi: &Path) {
        let live = live::Live::idle().bounded();
        let workspace = live.root.path().join("operator-target");
        std::fs::create_dir(&workspace).unwrap();
        let provider = Provider::start(scenario_handler(
            workspace.clone(),
            workspace.join("compaction-marker"),
            workspace.join("compaction-background.pid"),
        ));
        provider.seed_profile_with(
            &live.config,
            1024,
            Some(json!({"enabled": true, "reserveTokens": 0, "keepRecentTokens": 0})),
        );
        live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
        let mut driver = DashboardDriver::connect(&live);
        driver.open();
        let opened = driver.conversation();

        let filler = "x".repeat(6000);
        driver.submit(&format!("COMPACT_ONE {filler}"));
        let compaction_first = wait_terminal(&mut driver, opened.id, &provider, &live);
        let first_turn = compaction_first
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn
            .0;
        wait_stopped(&mut driver, opened.id);
        let history = submit_next(
            &mut driver,
            opened.id,
            "COMPACT_TWO",
            first_turn,
            &provider,
            &live,
        );
        assert!(delta_text(&history).contains("COMPACT_TWO_DONE"));
        let requests = provider.requests();
        assert!(
            requests
                .iter()
                .any(|request| operator_provider::is_compaction_request(&request.body)),
            "Pi's automatic compaction must call the same provider: {requests:#?}"
        );
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
            "INSTALLED_PI_COMPACTION PASS conversation={} compaction_requests={} provider_requests={}",
            opened.id.0,
            requests
                .iter()
                .filter(|request| operator_provider::is_compaction_request(&request.body))
                .count(),
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
    /// stream.
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
        let provider = Provider::start(scenario_handler(
            workspace.clone(),
            marker.clone(),
            background_pid.clone(),
        ));
        provider.seed_profile(&root);
        let _ = std::fs::remove_file(&release_marker);
        let _ = std::fs::remove_file(&stop_marker);
        println!(
            "OPERATOR_PI_GUI_READY base={} root={}",
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
                    "authorization": request.authorization,
                    "body": request.body,
                }))
                .unwrap(),
            );
            body.push('\n');
        }
        let _ = std::fs::write(&evidence, body);
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
