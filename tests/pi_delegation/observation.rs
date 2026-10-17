//! Registered production Pi tools, materialized extension, binary Server and real PTYs.
use super::*;
use std::os::unix::net::UnixStream;
static TOOL_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) struct Fixture {
    pub(super) live: Live,
    pub(super) gate: std::path::PathBuf,
    pub(super) wakes: std::path::PathBuf,
    pub(super) parent: SessionId,
    pub(super) workspace: String,
}
impl Fixture {
    pub(super) fn new(deadline: u64) -> Self {
        let live = Live::idle();
        fs::write(
            live.config.join("dashboard.toml"),
            format!("[delegation]\nprogress_deadline_ms = {deadline}\n"),
        )
        .unwrap();
        let gate = live.root.path().join("allow-result");
        let wakes = live.root.path().join("wakes.jsonl");
        let bytes = live.root.path().join("input-bytes");
        let fault = live.root.path().join("fault");
        fs::create_dir(&bytes).unwrap();
        let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
        let pi = live.claude.command.parent().unwrap().join("pi");
        fs::write(&pi, include_bytes!("../fixtures/pi/research_pi.sh")).unwrap();
        fs::set_permissions(&pi, fs::Permissions::from_mode(0o700)).unwrap();
        live.start_binary_env(&[
            ("OVRCR_TEST_DELEGATION", "1".as_ref()),
            ("OVRCR_TEST_PI_INTERRUPT_BINDING", "Escape".as_ref()),
            ("OVRCR_TEST_DELEGATION_FAULT", fault.as_os_str()),
            ("OVRCR_TEST_PI_HOST", host.as_os_str()),
            ("OVRCR_TEST_DELEGATION_RESULT_GATE", gate.as_os_str()),
            ("OVRCR_TEST_PI_WAKE_LOG", wakes.as_os_str()),
            // Match interactive Pi raw input; canonical PTYs truncate full contract JSON.
            ("OVRCR_TEST_DELEGATION_BYTES", bytes.as_os_str()),
        ]);
        live.ready("observe-fixture");
        let workspaces = cli_json(&live, &["workspace", "list", "--project", PROJECT]);
        let workspace = workspaces
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["branch"] == "observe-fixture")
            .unwrap()["id"]
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
            panic!("parent launch");
        };
        let fixture = Self {
            live,
            gate,
            wakes,
            parent: parent.id,
            workspace,
        };
        fixture.until(|| fixture.summary(fixture.parent).agent.is_some());
        for group in fixture.live.session_groups() {
            println!("OBSERVATION_OWNED_GROUP={group}");
            fixture.live.own_group(group);
        }
        fixture
    }
    pub(super) fn until(&self, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !condition() {
            assert!(Instant::now() < deadline, "observation fixture deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub(super) fn summary(&self, id: SessionId) -> SessionSummary {
        let Response::Inventory { sessions, .. } = self.live.request(Request::Inspect) else {
            panic!("inventory");
        };
        sessions.into_iter().find(|s| s.id == id).unwrap()
    }
    pub(super) fn send(&self, text: &str) {
        assert_eq!(
            self.live.request(Request::SendTerminal {
                session: self.parent,
                text: text.into(),
                submit: true
            }),
            Response::Ok
        );
    }
    pub(super) fn tool(&self, name: &str, args: serde_json::Value) -> serde_json::Value {
        let path = self.live.root.path().join(format!(
            "tool-{}.json",
            TOOL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        tool(&self.live, self.parent, name, args, &path)
    }
    pub(super) fn start(&self, scope: &str) -> AssignmentSnapshot {
        let started: AssignmentSnapshot = serde_json::from_value(self.tool("ovrcr_start", serde_json::json!({"project": PROJECT, "workspace": self.workspace, "scope": scope, "model_provider": "test", "model": "fixture-model", "acceptance_evidence": ["README citation"]}))).unwrap();
        assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
        wait_file(&started.brief.with_extension("json.started"));
        for group in self.live.session_groups() {
            if !self.live.owned_groups().contains(&group) {
                println!("OBSERVATION_OWNED_GROUP={group}");
            }
            self.live.own_group(group);
        }
        started
    }
    pub(super) fn observe(
        &self,
        handles: &[&str],
        after: Option<&ObservationCursor>,
        wait_ms: u64,
    ) -> DelegationObservationBatch {
        serde_json::from_value(self.tool(
            "ovrcr_wait",
            serde_json::json!({"handles": handles, "after": after, "wait_ms": wait_ms}),
        ))
        .unwrap()
    }
    pub(super) fn wake_rows(&self) -> Vec<serde_json::Value> {
        fs::read_to_string(&self.wakes)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
    pub(super) fn dashboard(&self) -> UnixStream {
        let mut stream = connect_server(&self.live.socket).unwrap();
        assert!(matches!(
            dashboard_view(&mut stream, 1, Request::DashboardHello).unwrap(),
            Response::Hierarchy(_)
        ));
        stream
    }
}

#[test]
fn production_observer_wakes_for_simultaneous_question_files_without_ready_or_input_events() {
    let f = Fixture::new(10000);
    let mut dashboard = f.dashboard();
    f.send("idle:false");
    f.send("agent_start");
    let a = f.start("QUESTION_FILE_ONLY first QUESTION_SCOPE_SECRET");
    let b = f.start("QUESTION_FILE_ONLY second QUESTION_SCOPE_SECRET");
    for child in [&a, &b] {
        wait_file(&child.brief.with_extension("json.question-published"));
    }
    let mut found = f.observe(&[&a.handle, &b.handle], None, 0);
    f.until(|| {
        found = f.observe(&[&a.handle, &b.handle], None, 0);
        found
            .observations
            .iter()
            .all(|o| o.questions.len() == 1 && o.questions[0].issue.is_none())
    });
    f.until(|| {
        f.wake_rows()
            .iter()
            .any(|row| row["delivery"] == "followUp")
    });
    for (child, observed) in [(&a, &found.observations[0]), (&b, &found.observations[1])] {
        assert!(observed.input_requests.is_empty());
        assert_eq!(
            observed.activity.as_ref().unwrap().state,
            AgentActivity::Busy,
            "question files must not depend on Ready"
        );
        assert_eq!(
            f.summary(child.identity.session.unwrap()).activity,
            AgentActivity::Busy
        );
        assert!(f.summary(child.identity.session.unwrap()).unread.is_none());
        let collected: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_collect_question", serde_json::json!({"handle": child.handle, "reference": observed.questions[0].reference}))).unwrap();
        assert!(
            collected
                .collected_question
                .unwrap()
                .question
                .contains("QUESTION_BODY_SECRET")
        );
        assert_eq!(collected.acceptance, "pending");
    }
    let publication = found.observations[0].questions[0].reference.publication();
    f.tool(
        "ovrcr_mark_handled",
        serde_json::json!({"handle": a.handle, "publication": publication}),
    );
    f.send("run_command:ovrcr-observe-reconnect");
    let reconnect = f.observe(&[&a.handle, &b.handle], None, 0);
    assert!(reconnect.observations[0].questions[0].handled);
    assert!(!reconnect.observations[1].questions[0].handled);
    assert!(
        reconnect
            .observations
            .iter()
            .all(|o| o.questions[0].answer.is_none()),
        "handling and reconnect grant no answer authority"
    );
    let metadata = serde_json::to_string(&reconnect).unwrap();
    let wakes = fs::read_to_string(&f.wakes).unwrap();
    for secret in [
        "QUESTION_BODY_SECRET",
        "QUESTION_CONTEXT_SECRET",
        "QUESTION_SCOPE_SECRET",
        "Introduction",
        "Installation",
    ] {
        assert!(
            !metadata.contains(secret) && !wakes.contains(secret),
            "only bounded references belong in observation/wake payloads"
        );
    }
    assert!(matches!(
        dashboard_view(&mut dashboard, 2, Request::List).unwrap(),
        Response::Hierarchy(_)
    ));
}

#[test]
fn production_stopped_child_question_is_historical_and_observation_leaves_control_responsive() {
    let f = Fixture::new(10000);
    let child = f.start("QUESTION_FILE_ONLY stopped question evidence");
    wait_file(&child.brief.with_extension("json.question-published"));
    let before = f.observe(&[&child.handle], None, 0);
    let exact = before.observations[0].questions[0].reference.clone();
    assert!(before.observations[0].questions[0].current);
    assert_eq!(
        f.live.request(Request::KillSession {
            session: child.identity.session.unwrap()
        }),
        Response::Ok
    );
    f.until(|| !f.summary(child.identity.session.unwrap()).phase.is_live());
    let started = Instant::now();
    let stopped = f.observe(&[&child.handle], None, 0);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "stopped-child observation must remain bounded"
    );
    let question = &stopped.observations[0].questions[0];
    assert_eq!(question.reference, exact);
    assert!(
        !question.current,
        "stopped child evidence grants no current answer authority"
    );
    assert!(question.answer.is_none());
    let mut control = connect_server(&f.live.socket).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(
        matches!(
            client::request(&mut control, 1, Request::Inspect).unwrap(),
            Response::Inventory { .. }
        ),
        "independent control request after observation"
    );
    let historical: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_collect_question",
        serde_json::json!({"handle": child.handle, "reference": exact}),
    ))
    .unwrap();
    assert!(
        historical
            .collected_question
            .unwrap()
            .question
            .contains("QUESTION_BODY_SECRET")
    );
    assert_eq!(historical.result_version, child.result_version);
}

#[test]
fn production_native_prompt_gap_distinguishes_reported_prompt_from_unexplained_deadline() {
    for (scope, expected) in [
        (
            "QUESTION_NATIVE_GAP",
            NativeQuestionGap::IncompletePromptObservation,
        ),
        (
            "QUESTION_NO_EVENT_GAP",
            NativeQuestionGap::NoPromptObservation,
        ),
    ] {
        let f = Fixture::new(150);
        let child = f.start(scope);
        let mut observed = f.observe(&[&child.handle], None, 0);
        f.until(|| {
            observed = f.observe(&[&child.handle], None, 0);
            observed.observations[0].native_question_gap == Some(expected)
        });
        assert!(observed.observations[0].questions.is_empty());
        assert!(
            !child
                .brief
                .with_extension("json.question-published")
                .exists()
        );
        assert_eq!(
            !observed.observations[0].input_requests.is_empty(),
            expected == NativeQuestionGap::IncompletePromptObservation
        );
        assert!(f.summary(child.identity.session.unwrap()).phase.is_live());
        let json = serde_json::to_string(&observed).unwrap();
        assert!(
            !json.contains("QUESTION_BODY_SECRET"),
            "gap classification must not persist fabricated prompt bodies"
        );
    }
}

#[test]
fn production_observer_collects_simultaneous_file_only_results_and_preserves_dashboard() {
    let f = Fixture::new(10000);
    let mut dashboard = f.dashboard();
    let a = f.start("NO_READY first TASK_PAYLOAD_SECRET");
    let b = f.start("NO_READY REBIND_AFTER_RESULT second TASK_PAYLOAD_SECRET");
    f.send("idle:false");
    f.send("agent_start");
    f.until(|| f.summary(f.parent).activity == AgentActivity::Busy);
    let initial = f.observe(&[&a.handle, &b.handle], None, 0);
    fs::write(&f.gate, "publish").unwrap();
    wait_file(&a.brief.with_extension("json.published"));
    wait_file(&b.brief.with_extension("json.published"));
    f.until(|| {
        f.wake_rows()
            .iter()
            .any(|row| row["delivery"] == "followUp")
    });
    let found = f.observe(&[&a.handle, &b.handle], Some(&initial.cursor), 0);
    assert!(
        found
            .observations
            .iter()
            .all(|o| o.publication.is_some() && !o.publication.as_ref().unwrap().handled)
    );
    for assignment in [&a, &b] {
        let collected: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_collect",
            serde_json::json!({"handle": assignment.handle}),
        ))
        .unwrap();
        assert!(collected.submission.is_some());
        assert_eq!(collected.acceptance, "pending");
        let child = f.summary(assignment.identity.session.unwrap());
        assert_eq!(
            child.activity,
            AgentActivity::Busy,
            "file result does not require settlement"
        );
        assert!(
            child.unread.is_none(),
            "file observation must not create Unread"
        );
    }
    let publication = found.observations[0]
        .publication
        .as_ref()
        .unwrap()
        .identity
        .clone();
    for _ in 0..2 {
        f.tool(
            "ovrcr_mark_handled",
            serde_json::json!({"handle": a.handle, "publication": publication}),
        );
    }
    let mut wrong = publication.clone();
    wrong.digest.push('0');
    let error_path = f.live.root.path().join("wrong-handled.json");
    let refused = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_mark_handled",
        serde_json::json!({"handle": a.handle, "publication": wrong}),
        &error_path,
    );
    assert!(
        refused.get("error").is_some(),
        "changed digest cannot inherit handling"
    );
    f.send("run_command:ovrcr-observe-reconnect");
    let reconnected = f.observe(&[&a.handle, &b.handle], None, 0);
    assert!(
        reconnected.observations[0]
            .publication
            .as_ref()
            .unwrap()
            .handled
    );
    assert!(
        !reconnected.observations[1]
            .publication
            .as_ref()
            .unwrap()
            .handled
    );
    let pending_publication = reconnected.observations[1]
        .publication
        .as_ref()
        .unwrap()
        .identity
        .clone();
    fs::write(
        b.brief.with_extension("json.rebind"),
        "replace native conversation",
    )
    .unwrap();
    wait_file(&b.brief.with_extension("json.rebound"));
    let replaced = f.observe(&[&b.handle], None, 0);
    assert!(!replaced.observations[0].current);
    assert_eq!(
        replaced.observations[0]
            .publication
            .as_ref()
            .unwrap()
            .identity,
        pending_publication
    );
    let stale_output = f.live.root.path().join("stale-handled.json");
    let refused = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_mark_handled",
        serde_json::json!({"handle":b.handle,"publication":pending_publication}),
        &stale_output,
    );
    assert!(
        refused["error"].is_string(),
        "replaced native binding cannot acknowledge the old publication: {refused}"
    );
    let pending = f.observe(&[&b.handle], None, 0);
    assert!(
        !pending.observations[0]
            .publication
            .as_ref()
            .unwrap()
            .handled
    );
    assert!(
        b.result_path.exists(),
        "refusal preserves original submitted evidence"
    );
    assert!(
        matches!(
            f.live.request(Request::DashboardHello),
            Response::Error { .. }
        ),
        "observer must leave the existing seat occupied"
    );
    assert!(matches!(
        dashboard_view(&mut dashboard, 3, Request::List).unwrap(),
        Response::Hierarchy(_)
    ));
    let log = fs::read_to_string(&f.wakes).unwrap();
    assert!(!log.contains("TASK_PAYLOAD_SECRET") && !log.contains("Read the assigned repository"));
    assert!(
        f.wake_rows()
            .iter()
            .all(|r| r["options"]["deliverAs"] == "followUp" && r["message"]["display"] == false)
    );
}

#[test]
fn production_observer_deadline_reconnect_overflow_and_wait_abort_leave_child_running() {
    let f = Fixture::new(150);
    let _dashboard = f.dashboard();
    let child = f.start("NO_READY silent until explicitly released");
    let initial = f.observe(&[&child.handle], None, 0);
    let mut last = initial.clone();
    let stalled_deadline = Instant::now() + Duration::from_secs(10);
    while !last.observations[0].stalled {
        assert!(
            Instant::now() < stalled_deadline,
            "silent child did not reach observation deadline"
        );
        last = f.observe(&[&child.handle], Some(&last.cursor), 300);
    }
    let progress = last.observations[0].last_progress_unix_ms;
    f.send("run_command:ovrcr-observe-reconnect");
    let reconnect = f.observe(&[&child.handle], None, 0);
    assert!(reconnect.observations[0].stalled);
    assert_eq!(reconnect.observations[0].last_progress_unix_ms, progress);
    f.until(|| f.wake_rows().iter().any(|row| row["delivery"] == "idle"));
    let checkpoint_path = child.brief.parent().unwrap().join("observation-v1.json");
    let mut checkpoint: serde_json::Value =
        serde_json::from_slice(&fs::read(&checkpoint_path).unwrap()).unwrap();
    checkpoint["progress_unix_ms"] = serde_json::json!(u64::MAX);
    fs::write(&checkpoint_path, serde_json::to_vec(&checkpoint).unwrap()).unwrap();
    let future = f.observe(&[&child.handle], None, 0);
    assert!(
        future.observations[0].stalled && future.observations[0].progress_unresolved,
        "future persisted clocks cannot postpone observation deadlines"
    );
    f.send(&"ui_prompt_start:input\nui_prompt_end:input\n".repeat(140));
    let overflow = f.observe(&[&child.handle], Some(&initial.cursor), 0);
    assert_eq!(overflow.status, ObservationStatus::ReconcileRequired);
    let output = f.live.root.path().join("cancelled-wait.json");
    let stable = f.observe(&[&child.handle], Some(&overflow.cursor), 0);
    f.send(&format!("tool:{}", serde_json::json!({"name":"ovrcr_wait","args":{"handles":[child.handle],"after":stable.cursor,"wait_ms":20000},"output":output,"background":true})));
    f.send(&format!("abort_tool:{}", output.display()));
    wait_file(&output);
    let cancelled: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(
        cancelled["error"]
            .as_str()
            .unwrap()
            .contains("observation wait cancelled"),
        "{cancelled}"
    );
    let current = f.summary(child.identity.session.unwrap());
    assert_eq!(Some(current.run), child.identity.run);
    assert!(
        current.phase.is_live(),
        "wait abort must not interrupt child"
    );
    fs::write(&f.gate, "publish").unwrap();
    wait_file(&child.brief.with_extension("json.published"));
    let collected: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", serde_json::json!({"handle":child.handle})))
            .unwrap();
    assert!(
        collected.submission.is_some(),
        "child remains collectable after abort"
    );
}

// Match the original authored research document in pi_host.mjs. New optional
// wire fields do not belong to that model report's authenticated file bytes.
fn authored_research_bytes(submission: &ResearchSubmission) -> Vec<u8> {
    assert!(submission.coding.is_none());
    #[derive(serde::Serialize)]
    struct Authored<'a> {
        schema: u32,
        identity: &'a AssignmentIdentity,
        version: u32,
        outcome: &'a str,
        summary: &'a str,
        evidence: &'a [String],
        unresolved: &'a [String],
    }
    serde_json::to_vec(&Authored {
        schema: submission.schema,
        identity: &submission.identity,
        version: submission.version,
        outcome: &submission.outcome,
        summary: &submission.summary,
        evidence: &submission.evidence,
        unresolved: &submission.unresolved,
    })
    .unwrap()
}

fn assert_authored_intent(fixture: &Fixture, child: &AssignmentSnapshot, bytes: &[u8]) {
    let database = rusqlite::Connection::open_with_flags(
        fixture.live.config.join("registry.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    database.busy_timeout(Duration::from_secs(5)).unwrap();
    let metadata: String = database
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [child.handle.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    let expected = metadata["publication_intent"]["identity"]["digest"]
        .as_str()
        .expect("real child authenticated publication intent");
    let path = fixture
        .live
        .root
        .path()
        .join(format!("authored-research-v{}.json", child.result_version));
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    // Reuse the suite's existing Node crypto hashing path; no production
    // canonicalizer or new dependency can manufacture the expected bytes.
    let output = std::process::Command::new("node")
        .args(["--input-type=module", "-e", "import {createHash} from 'node:crypto';import {readFileSync} from 'node:fs';console.log(createHash('sha256').update(readFileSync(process.argv[1])).digest('hex'));"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        actual.trim(),
        expected,
        "authored fixture bytes must match the actual child-authenticated intent before restoration"
    );
    eprintln!(
        "AUTHORED_RESEARCH_INTENT version={} bytes={} sha256={} matched=true",
        child.result_version,
        bytes.len(),
        actual.trim()
    );
}

#[test]
fn production_publication_reconciliation_requires_authenticated_intent_and_exact_bytes() {
    let f = Fixture::new(10000);
    let child = f.start("NO_READY authenticated file recovery");
    let submission = ResearchSubmission {
        review: None,
        coding: None,
        schema: 1,
        identity: child.identity.clone(),
        version: 1,
        outcome: "success".into(),
        summary: "Read the assigned repository README.".into(),
        evidence: vec!["README:1 fixture".into()],
        unresolved: vec![],
    };
    let bytes = authored_research_bytes(&submission);
    fs::write(&child.result_path, &bytes).unwrap();
    fs::set_permissions(&child.result_path, fs::Permissions::from_mode(0o600)).unwrap();
    let unauthenticated = f.observe(&[&child.handle], None, 0);
    assert_eq!(
        unauthenticated.observations[0].publication_issue,
        Some(PublicationIssue::Uncommitted)
    );
    assert!(unauthenticated.observations[0].publication.is_none());
    // The child's authenticated Publish records its exact intent before discovering
    // this conflicting file. No parsed file can manufacture the authorization.
    fs::write(&child.result_path, "corrupt").unwrap();
    fs::write(&f.gate, "attempt publication").unwrap();
    wait_file(&child.brief.with_extension("json.published"));
    let invalid = f.observe(&[&child.handle], Some(&unauthenticated.cursor), 0);
    assert_eq!(
        invalid.observations[0].publication_issue,
        Some(PublicationIssue::Invalid)
    );
    assert_authored_intent(&f, &child, &bytes);
    let temporary = child.result_path.with_extension("tmp");
    fs::write(&temporary, &bytes).unwrap();
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(&temporary, &child.result_path).unwrap();
    let recovered = f.observe(&[&child.handle], Some(&invalid.cursor), 1000);
    assert!(
        recovered.observations[0].publication.is_some(),
        "exact authenticated file is discoverable without Ready or a second Publish"
    );
    let collected: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", serde_json::json!({"handle":child.handle})))
            .unwrap();
    assert_eq!(collected.submission, Some(submission));
    assert_eq!(collected.acceptance, "pending");
    fs::write(&child.result_path, "{}").unwrap();
    let modified = f.observe(&[&child.handle], Some(&recovered.cursor), 0);
    assert_eq!(
        modified.observations[0].publication_issue,
        Some(PublicationIssue::Invalid)
    );
    assert!(modified.observations[0].publication.is_none());
}

#[test]
fn production_observe_remains_bounded_while_workspace_lifecycle_is_blocked() {
    let f = Fixture::new(10000);
    let child = f.start("NO_READY lifecycle observation");
    let initial = f.observe(&[&child.handle], None, 0);
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("git"))
        .find(|p| p.is_file())
        .unwrap();
    let entered = f.live.root.path().join("git-entered");
    let release = f.live.root.path().join("git-release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let _release = Release(release.clone());
    let shim = f.live.claude.command.parent().unwrap().join("git");
    fs::write(&shim, format!("#!/bin/sh\ncase \"$*\" in *\"worktree add\"*\"blocked-observe\"*) : > '{}'; while [ ! -f '{}' ]; do sleep 0.01; done;; esac\nexec '{}' \"$@\"\n", entered.display(), release.display(), git.display())).unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    let mut dashboard = f.dashboard();
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            2,
            Request::CreateWorkspace {
                project: PROJECT.into(),
                id: "blocked-observe".into(),
                branch: BranchRequest::New {
                    branch: "blocked-observe".into(),
                    base: "main".into()
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    wait_file(&entered);
    let start = Instant::now();
    let observed = f.observe(&[&child.handle], Some(&initial.cursor), 100);
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(observed.observations[0].identity, child.identity);
    assert!(
        !release.exists(),
        "Observe must finish while lifecycle still holds its mutation lock"
    );
    fs::write(&release, "release").unwrap();
}

#[test]
fn production_ready_and_input_observations_wake_without_submission_or_acceptance() {
    let f = Fixture::new(10000);
    let _dashboard = f.dashboard();
    let ready = f.start("NO_READY READY_WAKE");
    f.until(|| f.summary(ready.identity.session.unwrap()).activity == AgentActivity::ResponseReady);
    let unread = f.summary(ready.identity.session.unwrap()).unread;
    assert!(unread.is_some());
    let ready_batch = f.observe(&[&ready.handle], None, 0);
    let activity = ready_batch.observations[0].activity.as_ref().unwrap();
    assert_eq!(activity.state, AgentActivity::ResponseReady);
    assert_eq!(activity.quality, SampleQuality::Confirmed);
    assert!(ready_batch.observations[0].publication.is_none());
    let collected: AssignmentSnapshot = serde_json::from_value(
        f.tool("ovrcr_collect", serde_json::json!({"handle": ready.handle})),
    )
    .unwrap();
    assert!(collected.submission.is_none());
    assert_eq!(collected.acceptance, "pending");
    assert_eq!(f.summary(ready.identity.session.unwrap()).unread, unread);
    let input = f.start("NO_READY INPUT_WAKE");
    f.until(|| f.summary(input.identity.session.unwrap()).activity == AgentActivity::WaitingInput);
    let input_batch = f.observe(&[&input.handle], None, 0);
    assert_eq!(input_batch.observations[0].input_requests.len(), 1);
    assert_eq!(
        input_batch.observations[0].input_requests[0].kind,
        InputKind::Input
    );
    assert!(input_batch.observations[0].publication.is_none());
    f.until(|| {
        f.wake_rows().iter().any(|r| {
            r["message"]["details"]["handles"]
                .as_array()
                .is_some_and(|handles| handles.contains(&serde_json::json!(input.handle)))
        })
    });
    assert!(!ready.result_path.exists() && !input.result_path.exists());
}

#[test]
fn production_versioned_publications_keep_authorship_handling_and_observation_current() {
    let f = Fixture::new(10000);
    let first = f.start("NO_READY versioned authenticated recovery");
    fs::write(&f.gate, "publish1").unwrap();
    wait_file(&first.brief.with_extension("json.published"));
    let first: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", serde_json::json!({"handle":first.handle})))
            .unwrap();
    let publication1 = first.publication.as_ref().unwrap().identity.clone();
    let handled: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_mark_handled",
        serde_json::json!({"handle":first.handle,"publication":publication1}),
    ))
    .unwrap();
    assert!(handled.publication.as_ref().unwrap().handled);
    let initial = f.observe(&[&first.handle], None, 0);
    assert!(
        initial.observations[0]
            .publication
            .as_ref()
            .unwrap()
            .handled
    );
    let first: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_accept", serde_json::json!({"handle":first.handle,"identity":first.identity,"result_version":1,"result_digest":publication1.digest,"expected_control_epoch":first.control.epoch}))).unwrap();
    fs::remove_file(&f.gate).unwrap();
    let second: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_followup", serde_json::json!({"handle":first.handle,"expected_identity":first.identity,"expected_result_version":1,"expected_control_epoch":first.control.epoch,"text":"Clarify the same scoped result."}))).unwrap();
    assert_eq!(second.identity, first.identity);
    assert_eq!(second.result_version, 2);
    wait_file(&second.brief.with_extension("json.started"));
    assert!(second.history[0].publication.as_ref().unwrap().handled);
    assert_eq!(second.history[0].acceptance, first.acceptance_ref);
    let pending = f.observe(&[&second.handle], Some(&initial.cursor), 0);
    assert!(pending.observations[0].publication.is_none());
    let bytes = authored_research_bytes(&ResearchSubmission {
        review: None,
        coding: None,
        schema: 1,
        identity: second.identity.clone(),
        version: 2,
        outcome: "success".into(),
        summary: "Read the assigned repository README.".into(),
        evidence: vec!["README:1 fixture".into()],
        unresolved: vec![],
    });
    fs::write(&second.result_path, &bytes).unwrap();
    fs::set_permissions(&second.result_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        f.observe(&[&second.handle], None, 0).observations[0].publication_issue,
        Some(PublicationIssue::Uncommitted)
    );
    fs::write(&second.result_path, "corrupt version2").unwrap();
    fs::write(&f.gate, "attempt2").unwrap();
    wait_file(&second.brief.with_extension("json.published"));
    assert_eq!(
        f.observe(&[&second.handle], None, 0).observations[0].publication_issue,
        Some(PublicationIssue::Invalid)
    );
    let late: serde_json::Value =
        serde_json::from_slice(&fs::read(second.brief.with_extension("json.late-result")).unwrap())
            .unwrap();
    assert!(
        late["error"]
            .as_str()
            .unwrap()
            .contains("stale result version"),
        "{late}"
    );
    let blocked = f.tool("ovrcr_followup", serde_json::json!({"handle":second.handle,"expected_identity":second.identity,"expected_result_version":2,"expected_control_epoch":second.control.epoch,"text":"Cannot supersede unresolved authenticated intent."}));
    assert_eq!(blocked["dispatch"], "Refused", "{blocked}");
    assert!(
        !second
            .brief
            .parent()
            .unwrap()
            .join("brief-v3.json")
            .exists()
    );
    assert_authored_intent(&f, &second, &bytes);
    fs::remove_file(&second.result_path).unwrap();
    assert_eq!(
        f.observe(&[&second.handle], None, 0).observations[0].publication_issue,
        Some(PublicationIssue::Unavailable)
    );
    fs::write(&second.result_path, &bytes).unwrap();
    fs::set_permissions(&second.result_path, fs::Permissions::from_mode(0o600)).unwrap();
    let recovered = f.observe(&[&second.handle], None, 0);
    assert_eq!(
        recovered.observations[0]
            .publication
            .as_ref()
            .unwrap()
            .identity
            .version,
        2
    );
    assert!(
        !recovered.observations[0]
            .publication
            .as_ref()
            .unwrap()
            .handled
    );
    let second: AssignmentSnapshot = serde_json::from_value(
        f.tool("ovrcr_collect", serde_json::json!({"handle":second.handle})),
    )
    .unwrap();
    let publication2 = second.publication.as_ref().unwrap().identity.clone();
    assert_eq!(publication2.version, 2);
    assert_eq!(second.submission.as_ref().unwrap().version, 2);
    assert_eq!(second.acceptance, "pending");
    let stale = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_mark_handled",
        serde_json::json!({"handle":second.handle,"publication":publication1}),
        &f.live.root.path().join("old-handling.json"),
    );
    assert!(
        stale["error"]
            .as_str()
            .unwrap()
            .contains("handled identity does not match current publication"),
        "{stale}"
    );
    assert!(
        !f.observe(&[&second.handle], None, 0).observations[0]
            .publication
            .as_ref()
            .unwrap()
            .handled
    );
    let handled2: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_mark_handled",
        serde_json::json!({"handle":second.handle,"publication":publication2}),
    ))
    .unwrap();
    assert!(handled2.publication.as_ref().unwrap().handled);
    let second: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_accept", serde_json::json!({"handle":second.handle,"identity":second.identity,"result_version":2,"result_digest":publication2.digest,"expected_control_epoch":second.control.epoch}))).unwrap();
    let brief: serde_json::Value =
        serde_json::from_slice(&fs::read(&second.brief).unwrap()).unwrap();
    let mut contract: ResearchContract = serde_json::from_value(brief["contract"].clone()).unwrap();
    contract.scope = "NO_READY explain the README prerequisites under revised requirements".into();
    fs::remove_file(&f.gate).unwrap();
    let third: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_followup", serde_json::json!({"handle":second.handle,"expected_identity":second.identity,"expected_result_version":2,"expected_control_epoch":second.control.epoch,"text":"Use the explicitly revised scoped requirements.","revised_contract":contract}))).unwrap();
    assert_eq!(third.result_version, 3);
    assert_eq!(third.identity.contract, 2);
    assert_eq!(third.identity.session, first.identity.session);
    assert_eq!(third.identity.run, first.identity.run);
    wait_file(&third.brief.with_extension("json.started"));
    let revised = f.observe(&[&third.handle], None, 0);
    assert_eq!(revised.observations[0].identity, third.identity);
    assert!(revised.observations[0].publication.is_none());
    fs::write(&f.gate, "publish3").unwrap();
    wait_file(&third.brief.with_extension("json.published"));
    let third: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", serde_json::json!({"handle":third.handle})))
            .unwrap();
    let publication3 = third.publication.as_ref().unwrap().identity.clone();
    assert_eq!(publication3.version, 3);
    assert!(!third.publication.as_ref().unwrap().handled);
    let third: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_mark_handled",
        serde_json::json!({"handle":third.handle,"publication":publication3}),
    ))
    .unwrap();
    assert!(third.publication.as_ref().unwrap().handled);
    for version in [1, 2] {
        let history: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_collect",
            serde_json::json!({"handle":third.handle,"version":version}),
        ))
        .unwrap();
        assert_eq!(
            history.historical_submission.as_ref().unwrap().version,
            version
        );
        assert!(history.submission.is_none());
        assert!(
            history.history[(version - 1) as usize]
                .publication
                .as_ref()
                .unwrap()
                .handled
        );
        assert!(history.history[(version - 1) as usize].acceptance.is_some());
    }
    for brief in [&first.brief, &second.brief, &third.brief] {
        assert_eq!(
            fs::read_to_string(brief.with_extension("json.input-count"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}
