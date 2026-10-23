//! #328 ordinary managed Claude production-path regressions; installed gates remain separate.
//! These tests replace native model work, not installed-provider acceptance.
use super::followups::{OwnedFollowup, complete, followup};
use super::*;

struct ClaudeFixture {
    owner: OwnedFollowup,
    profile: std::path::PathBuf,
}

impl std::ops::Deref for ClaudeFixture {
    type Target = RecoveryFixture;
    fn deref(&self) -> &Self::Target {
        &self.owner
    }
}

impl Drop for ClaudeFixture {
    fn drop(&mut self) {
        if let Ok(receipts) =
            fs::read_to_string(self.profile.join("delegation-discovery-owners.jsonl"))
        {
            for line in receipts.lines().take(64) {
                if let Ok(receipt) = serde_json::from_str::<serde_json::Value>(line)
                    && let (Some(pid), Some(pgid)) =
                        (receipt["pid"].as_u64(), receipt["pgid"].as_u64())
                {
                    eprintln!(
                        "CLAUDE_PREFLIGHT_OWNER pid={pid} pgid={pgid} root={}",
                        self.live.root.path().display()
                    );
                }
            }
        }
    }
}

impl ClaudeFixture {
    fn new(mode: &str) -> Self {
        let owner = OwnedFollowup::new();
        let profile = owner.live.root.path().join(".claude");
        fs::create_dir_all(&profile).unwrap();
        let settings = owner
            .live
            .command()
            .args(["agent", "setup", "claude", "--print"])
            .output()
            .unwrap();
        assert!(settings.status.success(), "private Claude setup failed");
        fs::write(profile.join("settings.json"), settings.stdout).unwrap();
        fs::write(profile.join("delegation-mode"), mode).unwrap();
        let host = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pi/claude_delegation_host.mjs");
        // The existing binary Server already owns this private PATH. Replacement
        // affects only the deterministic native executable in this fixture root.
        let quoted = host.to_str().unwrap().replace('\'', "'\"'\"'");
        fs::write(
            &owner.live.claude.command,
            format!("#!/bin/sh\nstty -icanon\nexec node '{quoted}' \"$@\"\n"),
        )
        .unwrap();
        fs::set_permissions(
            &owner.live.claude.command,
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let configured = owner
            .live
            .command()
            .args([
                "settings",
                "set",
                "delegation.allowed_harnesses",
                "[\"pi\",\"claude\"]",
            ])
            .output()
            .unwrap();
        assert!(
            configured.status.success(),
            "{}",
            String::from_utf8_lossy(&configured.stderr)
        );
        Self { owner, profile }
    }

    fn start_claude(&self) -> serde_json::Value {
        self.tool(
            "ovrcr_start",
            serde_json::json!({
                "project": PROJECT, "workspace": self.workspace,
                "provider": "claude", "model_provider": "native", "model": "fixture-model",
                "scope": "Read README using only existing read authority.",
                "acceptance_evidence": ["README citation"]
            }),
        )
    }

    fn started(&self) -> AssignmentSnapshot {
        self.started_policy(false)
    }

    fn started_policy(&self, helpers: bool) -> AssignmentSnapshot {
        let reply = if helpers {
            self.tool("ovrcr_start", serde_json::json!({
                "project":PROJECT,"workspace":self.workspace,"provider":"claude","model_provider":"native","model":"fixture-model",
                "scope":"Read README using the assigned native research helper.","acceptance_evidence":["README citation"],
                "mandatory_capabilities":["read","result_file","native_helpers"]
            }))
        } else {
            self.start_claude()
        };
        assert_eq!(reply["dispatch"], "Applied", "{reply}");
        let snapshot: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
        wait_file(&snapshot.brief.with_extension("json.started"));
        let launch: serde_json::Value =
            serde_json::from_slice(&fs::read(self.profile.join("delegation-launch.json")).unwrap())
                .unwrap();
        let args = launch["argv"].as_array().unwrap();
        assert!(args.iter().any(|arg| arg == "--permission-mode"));
        assert_eq!(
            args[args.iter().position(|arg| arg == "--tools").unwrap() + 1],
            if helpers { "Agent" } else { "" }
        );
        let grants: Vec<_> = args
            .windows(2)
            .filter(|pair| pair[0] == "--allowedTools")
            .collect();
        if helpers {
            assert!(
                grants.is_empty(),
                "helper-v1 must not gain native approvals"
            );
        } else {
            assert_eq!(
                grants.len(),
                1,
                "default Research requires the exact v2 grant recipe"
            );
            assert_eq!(
                grants[0][1],
                "mcp__ovrcr_child__ovrcr_read,mcp__ovrcr_child__ovrcr_publish,mcp__ovrcr_child__ovrcr_publish_question"
            );
        }
        assert!(args.iter().any(|arg| arg == "--strict-mcp-config"));
        let mcp: serde_json::Value = serde_json::from_slice(
            &fs::read(
                args[args.iter().position(|arg| arg == "--mcp-config").unwrap() + 1]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(mcp["mcpServers"].as_object().unwrap().len(), 1);
        assert!(mcp["mcpServers"].get("ovrcr_child").is_some());
        assert!(!args.iter().any(|arg| {
            [
                "--dangerously-skip-permissions",
                "--allow-dangerously-skip-permissions",
                "--bare",
            ]
            .iter()
            .any(|bad| arg == bad)
        }));
        assert_eq!(
            args[args.iter().position(|arg| arg == "--model").unwrap() + 1],
            "fixture-model"
        );
        let Response::Inventory { sessions, .. } = self.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let child = sessions
            .iter()
            .find(|s| Some(s.id) == snapshot.identity.session)
            .unwrap();
        assert_eq!(
            child.kind,
            SessionKind::Agent {
                name: "claude".into()
            }
        );
        assert_eq!(child.run, snapshot.identity.run.unwrap());
        assert_eq!(
            child.agent.as_ref().unwrap().binding.provider,
            AgentProvider::Claude
        );
        snapshot
    }

    fn observe(&self, handle: &str) -> DelegationObservationBatch {
        serde_json::from_value(self.tool(
            "ovrcr_wait",
            serde_json::json!({"handles": [handle], "wait_ms": 0}),
        ))
        .unwrap()
    }

    fn child_count(&self) -> usize {
        let Response::Inventory { sessions, .. } = self.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        sessions
            .iter()
            .filter(|s| {
                s.kind
                    == SessionKind::Agent {
                        name: "claude".into(),
                    }
            })
            .count()
    }
}

#[test]
fn public_claude_native_helper_opt_in_seals_read_only_recipe_and_preserves_recovery() {
    let f = ClaudeFixture::new("no-stop");
    let capabilities = f.tool(
        "ovrcr_capabilities",
        serde_json::json!({"provider":"claude","model_provider":"native","model":"fixture-model"}),
    );
    assert_eq!(capabilities["native_helpers"], true);
    assert_eq!(capabilities["native_descendant_usage"], false);
    let first = f.started_policy(true);
    let launch: serde_json::Value =
        serde_json::from_slice(&fs::read(f.profile.join("delegation-launch.json")).unwrap())
            .unwrap();
    let args: Vec<std::ffi::OsString> = launch["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().into())
        .collect();
    let agents = args.windows(2).find(|a| a[0] == "--agents").unwrap()[1]
        .to_str()
        .unwrap();
    assert_eq!(
        ovrcr_runtime::claude_delegation::helper_model(agents).unwrap(),
        "fixture-model-resolved"
    );
    let definitions: serde_json::Value = serde_json::from_str(agents).unwrap();
    assert_eq!(
        definitions["ovrcr-assigned-research"]["tools"],
        serde_json::json!(["mcp__ovrcr_child__ovrcr_read"])
    );
    let root = first.brief.parent().unwrap();
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("claude-settings.json")).unwrap()).unwrap();
    assert_eq!(
        settings["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
        "fixture-model-resolved"
    );
    assert_eq!(
        settings["permissions"]["deny"],
        serde_json::json!(["Agent(fork)"])
    );
    let options = ovrcr_runtime::claude_recovery::launch_options(
        &[
            vec![f.live.claude.command.clone().into_os_string()],
            args.clone(),
        ]
        .concat(),
    )
    .unwrap();
    assert!(
        options
            .windows(2)
            .any(|a| a[0] == "--agents" && a[1] == agents)
    );
    // Actual Reporter admission must retain the same recipe, not silently drop recovery.
    let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
    let metadata: String = db
        .query_row(
            "SELECT reference FROM agent_conversations WHERE session=?1",
            [i64::try_from(first.identity.session.unwrap().0).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let recovery: ovrcr_protocol::ConversationReference = serde_json::from_str(&metadata).unwrap();
    let ovrcr_protocol::ConversationReference::Claude(recovery) = recovery else {
        panic!("wrong recovery provider")
    };
    assert_eq!(
        recovery.options, options,
        "native admission lost generated recovery"
    );
    ovrcr_runtime::claude_recovery::validate(&recovery).unwrap();
    let result = complete(&f, &first);
    assert_eq!(result.acceptance, "pending");
    assert_eq!(
        f.child_count(),
        1,
        "native helper policy adds no OVRCR Session"
    );
    // All later production effects recheck the sealed force-model/fork policy.
    let mut changed = settings.clone();
    changed["env"]["CLAUDE_CODE_SUBAGENT_MODEL"] = "foreign-model".into();
    fs::write(
        root.join("claude-settings.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let before = f.input_count(&result);
    let refused = followup(&f, &result, None);
    assert!(
        refused.get("error").is_some() || refused["dispatch"] != "Applied",
        "{refused}"
    );
    assert_eq!(f.input_count(&result), before);
}

#[test]
fn public_claude_default_research_preapproves_only_scoped_mcp_tools() {
    let f = ClaudeFixture::new("no-stop");
    let started = f.started();
    let result = complete(&f, &started);
    assert_eq!(result.identity, started.identity);
    assert_eq!(result.conversation, started.conversation);
    assert_eq!(result.submission.as_ref().unwrap().version, 1);
    assert_eq!(result.acceptance, "pending");
    assert_eq!(f.input_count(&result), 1);
}

#[test]
fn public_claude_result_and_followup_keep_native_conversation_without_stop() {
    let f = ClaudeFixture::new("no-stop");
    let first = f.started();
    let result = complete(&f, &first);
    assert_eq!(result.submission.as_ref().unwrap().version, 1);
    assert_eq!(result.acceptance, "pending");
    let receipt = fs::read(first.brief.with_extension("json.started")).unwrap();
    let next: AssignmentSnapshot = serde_json::from_value(followup(&f, &result, None)).unwrap();
    assert_eq!(next.dispatch, DispatchStatus::Applied);
    assert_eq!(next.identity, first.identity);
    assert_eq!(next.conversation, first.conversation);
    assert_eq!(next.result_version, 2);
    let result = complete(&f, &next);
    assert_eq!(result.submission.as_ref().unwrap().version, 2);
    assert_eq!(result.acceptance, "pending");
    let consumed: serde_json::Value =
        serde_json::from_slice(&fs::read(next.brief.with_extension("json.started")).unwrap())
            .unwrap();
    let original: serde_json::Value = serde_json::from_slice(&receipt).unwrap();
    assert_eq!(consumed["pid"], original["pid"]);
    assert_eq!(consumed["conversation"], original["conversation"]);
    assert_eq!(consumed["turn"], 2);
    assert_eq!(f.input_count(&result), 1);
}

#[test]
fn public_claude_observed_stop_without_file_remains_missing_and_unaccepted() {
    let f = ClaudeFixture::new("missing-result");
    let started = f.started();
    wait_file(&started.brief.with_extension("json.stopped"));
    let collected = snapshot(
        &f.live,
        f.parent,
        &started.handle,
        &f.live.root.path().join("claude-missing.json"),
    );
    assert!(collected.submission.is_none());
    assert!(collected.publication.is_none());
    assert_eq!(collected.acceptance, "pending");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let observed = f.observe(&started.handle);
        if let Some(activity) = &observed.observations[0].activity
            && activity.state == AgentActivity::ResponseReady
        {
            assert_eq!(activity.quality, SampleQuality::Observed);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Claude Stop was not observed: {observed:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn public_claude_question_and_scoped_answer_use_current_shared_receipt_once() {
    let f = ClaudeFixture::new("question");
    let started = f.started();
    wait_file(&started.brief.with_extension("json.question-published"));
    let published: serde_json::Value = serde_json::from_slice(
        &fs::read(started.brief.with_extension("json.question-published")).unwrap(),
    )
    .unwrap();
    assert!(published.get("error").is_none(), "{published}");
    assert_eq!(published["details"]["status"], "published");
    let publication: PublicationIdentity =
        serde_json::from_value(published["details"]["publication"].clone()).unwrap();
    let reference = QuestionReference::from_publication(&publication).unwrap();
    assert_eq!(reference.assignment, started.identity);
    assert_eq!(reference.result_version, started.result_version);
    let current = f.reconcile(&started.handle);
    assert!(
        current
            .questions
            .iter()
            .any(|q| q.current && q.reference == reference)
    );
    let question = serde_json::to_value(reference).unwrap();
    let exact = f.tool(
        "ovrcr_collect_question",
        serde_json::json!({"handle": started.handle, "reference": question}),
    );
    assert_eq!(
        exact["collected_question"]["question"],
        "CLAUDE_QUESTION_PRIVATE: Which README section?"
    );
    let answer = f.tool("ovrcr_answer_question", serde_json::json!({"handle": started.handle, "question": question, "expected_control_epoch": started.control.epoch, "basis": "ExistingAuthority", "scope": "Read the assigned README only.", "answer": "CLAUDE_ANSWER_PRIVATE: Introduction."}));
    wait_file(&started.brief.with_extension("json.answer-received"));
    let receipt = answer["receipts"].as_array().unwrap().last().unwrap();
    assert_eq!(receipt["outcome"], "Applied");
    let replay = f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": started.handle, "request": receipt["request"]}),
    );
    assert_eq!(replay["receipts"], answer["receipts"]);
    assert_eq!(
        fs::read_to_string(started.brief.with_extension("json.answer-count")).unwrap(),
        "1\n"
    );
    let events = serde_json::to_string(&f.live.request(Request::Events { follow: false })).unwrap();
    for private in [
        "CLAUDE_QUESTION_PRIVATE",
        "CLAUDE_ANSWER_PRIVATE",
        "OVRCR_DELEGATION_TOKEN",
    ] {
        assert!(!events.contains(private));
    }
}

#[test]
fn public_claude_missing_or_wrong_native_model_never_receives_initial_input() {
    for mode in ["missing-model", "wrong-model"] {
        let f = ClaudeFixture::new(mode);
        let reply = f.start_claude();
        assert_eq!(reply["dispatch"], "Launched", "{mode}: {reply}");
        let pending: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
        let initial = pending
            .receipts
            .iter()
            .find(|receipt| receipt.operation == DispatchOperation::InitialInput)
            .unwrap();
        assert_eq!(initial.outcome, DispatchOutcome::Accepted);
        let current = f.reconcile(&pending.handle);
        assert_eq!(current.dispatch, DispatchStatus::Launched);
        assert_eq!(current.identity, pending.identity);
        assert!(current.identity.session.is_some());
        assert!(current.submission.is_none());
        assert!(current.publication.is_none());
        assert_eq!(current.acceptance, "pending");
        assert_eq!(f.input_count(&current), 0);
        assert_eq!(f.byte_count(&current), 0);
    }
}

#[test]
fn public_claude_startup_model_bootstraps_source_created_by_first_input() {
    let f = ClaudeFixture::new("startup-bootstrap");
    let reply = f.start_claude();
    let pending: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let current = loop {
        let current = f.reconcile(&pending.handle);
        let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let source_observed = sessions
            .iter()
            .find(|child| Some(child.id) == current.identity.session)
            .and_then(|child| child.agent.as_ref())
            .is_some_and(|agent| {
                agent.health.state == ovrcr_protocol::ReporterHealth::Connected
                    && agent.metrics.as_ref().is_some_and(|metrics| {
                        metrics.sample.usage.value.input_tokens == Some(7)
                            && metrics.sample.usage.value.coverage
                                == ovrcr_protocol::UsageCoverage::Partial
                    })
            });
        if f.input_count(&current) == 1
            && current.brief.with_extension("json.source-created").exists()
            && source_observed
        {
            break current;
        }
        assert!(
            Instant::now() < deadline,
            "initial input remained queued without a startup source: {current:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(current.identity, pending.identity);
    assert_eq!(current.dispatch, DispatchStatus::Applied);
    assert_eq!(f.input_count(&current), 1);
    assert!(f.byte_count(&current) > 0);
    assert!(current.submission.is_none());
    assert_eq!(current.acceptance, "pending");
    let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let child = sessions
        .iter()
        .find(|s| Some(s.id) == current.identity.session)
        .unwrap();
    let agent = child.agent.as_ref().unwrap();
    assert_eq!(agent.binding.provider, AgentProvider::Claude);
    assert_eq!(child.run, current.identity.run.unwrap());
    assert_eq!(
        agent.metrics.as_ref().unwrap().sample.model.as_deref(),
        Some("fixture-model-resolved")
    );
    assert!(
        agent.native_control.is_none(),
        "Claude does not certify settled completion"
    );
}

#[test]
fn public_claude_source_created_only_after_prompt_keeps_attribution_fenced() {
    let f = ClaudeFixture::new("startup-bootstrap-after-prompt");
    let pending: AssignmentSnapshot = serde_json::from_value(f.start_claude()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let suspended = loop {
        let current = f.reconcile(&pending.handle);
        if current.control.attribution_invalidated {
            break current;
        }
        assert!(
            Instant::now() < deadline,
            "fresh missing source did not fence attribution"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(suspended.acceptance, "suspended");
    assert_eq!(f.input_count(&suspended), 1);
    assert_eq!(suspended.identity, pending.identity);
    let prefix = format!(
        "Assignment attribution suspended: session={} expected_run=1 current_run=1 control_epoch=2 cause=HealthUnavailable reason=SourceNotCreated health_revision=",
        suspended.identity.session.unwrap().0
    );
    loop {
        let log = fs::read_to_string(f.live.root.path().join("server.log")).unwrap();
        if let Some(line) = log.lines().find(|line| line.starts_with(&prefix)) {
            assert!(line[prefix.len()..].parse::<u64>().unwrap() > 0);
            assert!(!line.contains(f.profile.to_str().unwrap()));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual suspension diagnostic missing"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(f.profile.join("bootstrap-create-source"), "create now").unwrap();
    wait_file(&suspended.brief.with_extension("json.source-created"));
    loop {
        let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let recovered = sessions
            .iter()
            .find(|child| Some(child.id) == pending.identity.session)
            .and_then(|child| child.agent.as_ref())
            .is_some_and(|agent| {
                agent.health.state == ovrcr_protocol::ReporterHealth::Connected
                    && agent
                        .metrics
                        .as_ref()
                        .is_some_and(|metrics| metrics.sample.usage.value.input_tokens == Some(7))
            });
        if recovered {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "created source was not actually ingested"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let current = f.reconcile(&pending.handle);
    assert!(current.control.attribution_invalidated);
    assert_eq!(current.acceptance, "suspended");
    assert!(current.submission.is_none());
    let log = fs::read_to_string(f.live.root.path().join("server.log")).unwrap();
    assert_eq!(
        log.lines().filter(|line| line.starts_with(&prefix)).count(),
        1
    );
}

#[test]
fn public_claude_wrong_startup_model_cannot_bootstrap_initial_input() {
    let f = ClaudeFixture::new("startup-bootstrap-wrong-model");
    let reply = f.start_claude();
    let pending: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let wrong_observed = sessions
            .iter()
            .find(|child| Some(child.id) == pending.identity.session)
            .and_then(|child| child.agent.as_ref())
            .is_some_and(|agent| {
                agent.metrics.as_ref().is_some_and(|metrics| {
                    metrics.sample.model.as_deref() == Some("different-model")
                })
            });
        if wrong_observed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual wrong startup model was not reported"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let current = f.reconcile(&pending.handle);
    assert_eq!(current.identity, pending.identity);
    assert_eq!(current.dispatch, DispatchStatus::Launched);
    assert_eq!(f.input_count(&current), 0);
    assert_eq!(f.byte_count(&current), 0);
    assert!(current.submission.is_none());
}

#[test]
fn public_claude_missing_startup_source_reports_cause_without_releasing_input() {
    let f = ClaudeFixture::new("source-not-created");
    let reply = f.start_claude();
    assert_eq!(reply["dispatch"], "Launched", "{reply}");
    let pending: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
    let session = pending.identity.session.unwrap();
    let retained_binding = || {
        let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
        let raw: String = db
            .query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [&pending.handle],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str::<serde_json::Value>(&raw).unwrap()["binding"].clone()
    };
    // No SessionStart can arrive during the first Start's bounded SendInitial loop.
    // Once Unavailable is observed below, later SendInitial cannot capture a binding.
    assert!(retained_binding().is_null());
    fs::write(f.profile.join("startup-source-release"), "release").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::TerminalText { text, .. } = f.live.request(Request::ReadTerminal {
            session,
            max_lines: Some(100),
        }) else {
            panic!("native startup output");
        };
        let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
            panic!("inventory");
        };
        let source_unavailable = sessions
            .iter()
            .find(|child| child.id == session)
            .and_then(|child| child.agent.as_ref())
            .is_some_and(|agent| {
                agent.health.reason.as_deref() == Some("source_not_created")
                    && agent.health.state == ovrcr_protocol::ReporterHealth::Unavailable
            });
        if source_unavailable
            && text.contains("Claude transcript source: stage=metadata kind=NotFound")
        {
            assert!(text.contains("previously_opened=false"), "{text}");
            assert!(
                !text.contains("projects/fixture"),
                "diagnostic exposed source path"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "source diagnostic missing: {text}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
        panic!("inventory");
    };
    let child = sessions.iter().find(|child| child.id == session).unwrap();
    let agent = child.agent.as_ref().unwrap();
    assert!(
        retained_binding().is_null(),
        "unavailable source never grants capture authority"
    );
    assert_eq!(agent.health.reason.as_deref(), Some("source_not_created"));
    assert_eq!(
        agent.health.state,
        ovrcr_protocol::ReporterHealth::Unavailable
    );
    let current = f.reconcile(&pending.handle);
    assert_eq!(current.identity, pending.identity);
    assert_eq!(current.dispatch, DispatchStatus::Launched);
    let initial = current
        .receipts
        .iter()
        .find(|receipt| receipt.operation == DispatchOperation::InitialInput)
        .unwrap();
    assert_eq!(initial.outcome, DispatchOutcome::Accepted);
    assert_eq!(f.input_count(&current), 0);
    assert_eq!(f.byte_count(&current), 0);
    assert!(current.submission.is_none());
    assert_eq!(current.acceptance, "pending");
}

#[test]
fn public_claude_native_permission_prompt_has_gap_and_no_question_or_result() {
    let f = ClaudeFixture::new("native-prompt");
    let started = f.started();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let batch = f.observe(&started.handle);
        let observation = &batch.observations[0];
        if observation.native_question_gap == Some(NativeQuestionGap::IncompletePromptObservation)
            && !observation.input_requests.is_empty()
        {
            assert!(
                !serde_json::to_string(&batch)
                    .unwrap()
                    .contains("CLAUDE_FIXTURE_NATIVE_PERMISSION_PROMPT"),
                "observation contains private native text"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native input gap missing: {batch:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let current = f.reconcile(&started.handle);
    assert!(current.questions.is_empty());
    assert!(current.submission.is_none());
    assert_eq!(current.acceptance, "pending");
}

#[test]
fn public_claude_unavailable_exact_model_has_no_child_effect_or_substitution() {
    let f = ClaudeFixture::new("result");
    let before = f.child_count();
    let refused = f.tool(
        "ovrcr_start",
        serde_json::json!({
            "project": PROJECT, "workspace": f.workspace,
            "provider": "claude", "model_provider": "native", "model": "unavailable-model",
            "scope": "Read README", "acceptance_evidence": ["citation"]
        }),
    );
    assert_eq!(refused["dispatch"], "Refused", "{refused}");
    assert_eq!(f.child_count(), before);
    assert!(!f.profile.join("delegation-launch.json").exists());
    assert!(refused["reason"].as_str().unwrap().contains("unavailable"));
}

#[test]
fn public_claude_blocked_submission_does_not_become_parent_acceptance() {
    let f = ClaudeFixture::new("incomplete");
    let started = f.started();
    let collected = complete(&f, &started);
    assert_eq!(collected.submission.as_ref().unwrap().outcome, "blocked");
    assert!(!collected.submission.as_ref().unwrap().unresolved.is_empty());
    assert_eq!(collected.acceptance, "pending");
    assert!(collected.acceptance_ref.is_none());
}

#[test]
fn public_claude_stale_publication_and_tampered_file_remain_uncollectable() {
    let f = ClaudeFixture::new("stale-result");
    let started = f.started();
    fs::write(f.live.root.path().join("allow-result"), "continue").unwrap();
    wait_file(&started.brief.with_extension("json.published"));
    let reply: serde_json::Value =
        serde_json::from_slice(&fs::read(started.brief.with_extension("json.published")).unwrap())
            .unwrap();
    assert_eq!(reply["error"], "refused", "{reply}");
    assert_eq!(reply["details"]["status"], "refused");
    assert_eq!(reply["details"]["code"], "Conflict");
    assert!(reply["details"].get("sent").is_none());
    let attempted: AssignmentIdentity =
        serde_json::from_value(reply["attempted_identity"].clone()).unwrap();
    let mut stale = started.identity.clone();
    stale.contract += 1;
    assert_eq!(attempted, stale);
    let current = f.reconcile(&started.handle);
    assert_eq!(current.identity, started.identity);
    assert!(current.submission.is_none());
    assert!(current.publication.is_none());
    assert_eq!(current.acceptance, "pending");
    assert!(current.acceptance_ref.is_none());
    assert!(!started.result_path.exists());
    let uncollectable = tool(
        &f.live,
        f.parent,
        "ovrcr_collect",
        serde_json::json!({"handle": started.handle}),
        &f.live.root.path().join("stale-uncollectable.json"),
    );
    let collected: AssignmentSnapshot = serde_json::from_value(uncollectable).unwrap();
    assert_eq!(collected.identity, started.identity);
    assert_eq!(collected.result_version, started.result_version);
    assert!(collected.submission.is_none());
    assert!(collected.publication.is_none());
    assert_eq!(collected.acceptance, "pending");
    assert!(collected.acceptance_ref.is_none());
    assert!(!started.result_path.exists());
    let uncommitted = serde_json::to_vec(&serde_json::json!({"schema": 1, "identity": started.identity, "version": 1, "outcome": "success", "summary": "Parseable but not authenticated", "evidence": ["README:1"], "unresolved": []})).unwrap();
    for (index, bytes) in [b"{partial".as_slice(), b"{}".as_slice(), &uncommitted]
        .into_iter()
        .enumerate()
    {
        fs::write(&started.result_path, bytes).unwrap();
        fs::set_permissions(&started.result_path, fs::Permissions::from_mode(0o600)).unwrap();
        let reply = tool_reply(
            &f.live,
            f.parent,
            "ovrcr_collect",
            serde_json::json!({"handle": started.handle}),
            &f.live.root.path().join(format!("invalid-{index}.json")),
        );
        assert!(
            reply.get("error").is_some(),
            "invalid/uncommitted bytes became child evidence: {reply}"
        );
    }
}

#[test]
fn public_claude_lost_followup_reply_reconciles_without_second_native_input() {
    let f = ClaudeFixture::new("no-stop");
    let started = f.started();
    let first = complete(&f, &started);
    f.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 1,
        fault: None,
    });
    let uncertain = followup(&f, &first, None);
    assert_eq!(uncertain["dispatch"], "Unknown");
    let original = f.proxy.original(DispatchOperation::Followup);
    let current = f.control(original.clone());
    assert_eq!(current.result_version, 2);
    let completed = complete(&f, &current);
    let same = f.control(original);
    assert_eq!(same.identity, completed.identity);
    assert_eq!(same.receipts, completed.receipts);
    assert_eq!(f.input_count(&same), 1);
    assert_eq!(f.child_count(), 1);
}

#[test]
fn public_claude_native_conversation_replacement_cannot_receive_followup() {
    let f = ClaudeFixture::new("rebind");
    let started = f.started();
    wait_file(&started.brief.with_extension("json.rebound"));
    let rejected = followup(&f, &started, None);
    assert_eq!(rejected["dispatch"], "Refused", "{rejected}");
    assert_eq!(f.input_count(&started), 1);
    let current = f.reconcile(&started.handle);
    assert!(current.submission.is_none());
    assert!(current.acceptance_ref.is_none());
}

#[test]
fn public_claude_mcp_escape_heavy_read_pages_preserve_bytes_and_offsets() {
    let f = ClaudeFixture::new("read-pages");
    let Response::Inventory { registry, .. } = f.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let workspace = &registry.workspace(PROJECT, &f.workspace).unwrap().path;
    let bytes = [
        vec![0; 8191],
        "é".as_bytes().to_vec(),
        vec![0; 4096],
        "終".as_bytes().to_vec(),
    ]
    .concat();
    fs::write(workspace.join("escape-heavy"), &bytes).unwrap();
    let reply = f.start_claude();
    assert_eq!(reply["dispatch"], "Applied", "{reply}");
    let started: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
    let mut offset = 0;
    let mut reconstructed = Vec::new();
    let mut pages = 0;
    while offset < bytes.len() {
        let path = f
            .profile
            .join(format!("delegation-read-page-{offset}.json"));
        wait_file(&path);
        let frame: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let reply = &frame["reply"];
        assert_eq!(reply["id"], "\0".repeat(128));
        assert!(frame["encoded_bytes"].as_u64().unwrap() <= 65_536);
        let value = &reply["result"]["structuredContent"];
        assert_eq!(value["status"], "read", "{value}");
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                reply["result"]["content"][0]["text"].as_str().unwrap()
            )
            .unwrap(),
            *value
        );
        let read = &value["read"];
        assert_eq!(
            read["identity"],
            serde_json::to_value(&started.identity).unwrap()
        );
        assert_eq!(read["result_version"], started.result_version);
        assert_eq!(read["control_epoch"], started.control.epoch);
        assert_eq!(read["offset"].as_u64().unwrap() as usize, offset);
        assert_eq!(read["total_bytes"].as_u64().unwrap() as usize, bytes.len());
        let text = read["text"].as_str().unwrap();
        assert!(!text.is_empty() && text.len() <= 4096);
        let next = read["next_offset"].as_u64().unwrap() as usize;
        assert_eq!(next, offset + text.len());
        assert!(next > offset && next <= bytes.len());
        eprintln!(
            "ASSIGNED_READ_PAGE offset={offset} next={next} encoded_bytes={}",
            frame["encoded_bytes"]
        );
        reconstructed.extend_from_slice(text.as_bytes());
        offset = next;
        pages += 1;
        assert!(pages <= 4);
    }
    assert_eq!(pages, 4);
    assert_eq!(reconstructed, bytes);
    let completed = started.brief.with_extension("json.paged-read");
    wait_file(&completed);
    let completed: serde_json::Value =
        serde_json::from_slice(&fs::read(completed).unwrap()).unwrap();
    assert_eq!(completed["text"].as_str().unwrap().as_bytes(), bytes);
    assert!(f.reconcile(&started.handle).submission.is_none());
}

#[test]
fn public_claude_mcp_read_confines_actual_workspace_and_authority() {
    use std::os::unix::fs::symlink;
    let f = ClaudeFixture::new("no-stop");
    let Response::Inventory { registry, .. } = f.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let workspace = registry
        .workspace(PROJECT, &f.workspace)
        .unwrap()
        .path
        .clone();
    fs::write(workspace.join("README.md"), "CLAUDE_SCOPED_READ_ONLY").unwrap();
    let outside = f.live.root.path().join("outside-secret-fixture");
    fs::write(&outside, "NEVER_RETURN_THIS_OUTSIDE_VALUE").unwrap();
    symlink(&outside, workspace.join("escape")).unwrap();
    symlink(f.live.root.path(), workspace.join("escape-dir")).unwrap();
    fs::hard_link(&outside, workspace.join("hardlink")).unwrap();
    fs::write(workspace.join("oversized"), vec![b'a'; 512 * 1024 + 1]).unwrap();
    fs::write(workspace.join("binary"), [255]).unwrap();
    fs::create_dir(workspace.join("directory")).unwrap();
    let fifo =
        std::ffi::CString::new(workspace.join("fifo").as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let mut cases: Vec<_> = [
        "../outside-secret-fixture",
        "escape",
        "escape-dir/outside-secret-fixture",
        "hardlink",
        "oversized",
        "binary",
        "directory",
        "fifo",
        "missing",
        "./README.md",
        "a/../README.md",
        "nul\0path",
    ]
    .into_iter()
    .map(|path| serde_json::json!({"target":{"Workspace":{"path":path}}}))
    .collect();
    cases.push(serde_json::json!({"target":{"Workspace":{"path":outside}}}));
    cases.extend([
        serde_json::json!({"handle":"foreign"}),
        serde_json::json!({"result_version":999}),
        serde_json::json!({"control_epoch":999}),
        serde_json::json!({"identity":{"instance":"foreign"}}),
        serde_json::json!({"target":{"Answer":{"request":"foreign"}}}),
        serde_json::json!({"offset":524289}),
    ]);
    fs::write(
        f.profile.join("delegation-read-cases.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
    let started = f.started();
    let positive: serde_json::Value =
        serde_json::from_slice(&fs::read(started.brief.with_extension("json.read")).unwrap())
            .unwrap();
    assert_eq!(positive["status"], "read", "{positive}");
    assert_eq!(positive["read"]["text"], "CLAUDE_SCOPED_READ_ONLY");
    assert_eq!(
        positive["read"]["identity"],
        serde_json::to_value(&started.identity).unwrap()
    );
    let denied: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(started.brief.with_extension("json.read-cases")).unwrap())
            .unwrap();
    assert_eq!(denied.len(), cases.len());
    for (request, reply) in cases.iter().zip(&denied) {
        assert_eq!(reply["status"], "refused", "{request}: {reply}");
        assert!(
            !reply
                .to_string()
                .contains("NEVER_RETURN_THIS_OUTSIDE_VALUE")
        );
    }
    assert!(f.reconcile(&started.handle).submission.is_none());
}

#[test]
fn public_claude_mcp_read_supports_registered_repository_root_without_expanding_scope() {
    use std::os::unix::fs::symlink;
    let f = ClaudeFixture::new("no-stop");
    let root = cli_json(
        &f.live,
        &["workspace", "get", "--project", PROJECT, "--branch", "main"],
    );
    assert_eq!(root["path"], f.live.repo.to_str().unwrap());
    fs::write(f.live.repo.join("README.md"), "CLAUDE_REGISTERED_ROOT_READ").unwrap();
    let outside = f.live.root.path().join("root-read-outside");
    fs::write(&outside, "NEVER_RETURN_ROOT_READ_OUTSIDE").unwrap();
    symlink(&outside, f.live.repo.join("root-escape")).unwrap();
    let cases = serde_json::json!([
        {"target":{"Workspace":{"path":"../root-read-outside"}}},
        {"target":{"Workspace":{"path":"root-escape"}}},
        {"target":{"Workspace":{"path":outside}}},
        {"result_version":999}, {"control_epoch":999},
        {"identity":{"instance":"foreign"}}
    ]);
    fs::write(
        f.profile.join("delegation-read-cases.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
    let reply = f.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":root["id"],"provider":"claude","model_provider":"native","model":"fixture-model",
        "scope":"Read README in the registered repository root without edits.","acceptance_evidence":["README citation"]
    }));
    assert_eq!(reply["dispatch"], "Applied", "{reply}");
    let started: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
    wait_file(&started.brief.with_extension("json.started"));
    let positive: serde_json::Value =
        serde_json::from_slice(&fs::read(started.brief.with_extension("json.read")).unwrap())
            .unwrap();
    assert_eq!(positive["status"], "read", "{positive}");
    assert_eq!(positive["read"]["text"], "CLAUDE_REGISTERED_ROOT_READ");
    assert_eq!(
        positive["read"]["identity"],
        serde_json::to_value(&started.identity).unwrap()
    );
    let denied: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(started.brief.with_extension("json.read-cases")).unwrap())
            .unwrap();
    assert_eq!(denied.len(), cases.as_array().unwrap().len());
    for reply in denied {
        assert_eq!(reply["status"], "refused", "{reply}");
        assert!(!reply.to_string().contains("NEVER_RETURN_ROOT_READ_OUTSIDE"));
    }
    let completed = complete(&f, &started);
    assert!(!completed.control.attribution_invalidated);
    assert_eq!(completed.control.epoch, 1);
}

#[test]
fn public_claude_explicit_foreign_backend_refuses_before_native_discovery_or_allocation() {
    let f = ClaudeFixture::new("result");
    for backend in ["anthropic", "custom"] {
        let reply = f.tool(
            "ovrcr_start",
            serde_json::json!({"project":PROJECT,"workspace":f.workspace,
            "provider":"claude","model_provider":backend,"model":"fixture-model",
            "scope":"Read README","acceptance_evidence":["citation"]}),
        );
        assert_eq!(reply["dispatch"], "Refused", "{reply}");
        assert!(reply["handle"].is_null());
        assert!(
            reply["reason"]
                .as_str()
                .unwrap()
                .contains("explicit native backend")
        );
    }
    assert_eq!(f.child_count(), 0);
    assert!(!f.profile.join("delegation-discovery-count").exists());
    assert!(!f.profile.join("delegation-launch.json").exists());
    assert!(
        !f.proxy
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| matches!(request, DelegationRequest::Prepare { .. }))
    );
}

#[test]
fn public_claude_changed_alias_resolution_never_launches_a_substitute_child() {
    let f = ClaudeFixture::new("resolution-mismatch");
    let reply = f.start_claude();
    assert_eq!(reply["dispatch"], "Refused", "{reply}");
    assert!(
        reply["reason"]
            .as_str()
            .unwrap()
            .contains("changed since Prepare")
    );
    assert_eq!(f.child_count(), 0);
    assert!(!f.profile.join("delegation-launch.json").exists());
    let DelegationRequest::Prepare { contract, .. } = f.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    assert_eq!(contract.model_provider, "native");
    assert_eq!(contract.model, "fixture-model");
}

#[test]
fn public_claude_policy_replacement_before_binding_never_receives_initial_input() {
    let f = ClaudeFixture::new("policy-before-bind");
    let reply = f.start_claude();
    assert_eq!(reply["dispatch"], "Refused", "{reply}");
    assert!(reply["reason"].as_str().unwrap().contains("trusted recipe"));
    let current = f.reconcile(reply["handle"].as_str().unwrap());
    assert_eq!(f.input_count(&current), 0);
    assert_eq!(f.byte_count(&current), 0);
    assert!(current.submission.is_none());
    assert_eq!(current.acceptance, "pending");
}

#[test]
fn public_claude_sealed_policy_replacement_fences_followup_without_new_input() {
    let f = ClaudeFixture::new("no-stop");
    let started = f.started();
    let first = complete(&f, &started);
    let before = f.byte_count(&first);
    let policy = first.brief.parent().unwrap().join("claude-mcp.json");
    let original = fs::read(&policy).unwrap();
    fs::write(&policy, b"{\"mcpServers\":{}}").unwrap();
    let reply = followup(&f, &first, None);
    let current = f.reconcile(&first.handle);
    assert_eq!(f.byte_count(&current), before, "{reply}");
    assert!(
        !current
            .history
            .iter()
            .any(|version| version.acceptance.is_some())
    );
    assert_eq!(fs::read(&policy).unwrap(), b"{\"mcpServers\":{}}");
    assert_ne!(fs::read(&policy).unwrap(), original);
}

#[test]
fn public_claude_known_native_prompt_never_consumes_assignment_text() {
    let f = ClaudeFixture::new("pre-input-prompt");
    let reply = f.start_claude();
    let handle = reply["handle"].as_str().unwrap();
    let current = f.reconcile(handle);
    assert_eq!(f.input_count(&current), 0);
    assert_eq!(f.byte_count(&current), 0);
    assert!(current.submission.is_none());
    assert_eq!(current.acceptance, "pending");
}

#[test]
fn public_claude_coding_and_valid_review_refuse_without_discovery_or_intent() {
    use sha2::{Digest, Sha256};
    let f = ClaudeFixture::new("result");
    let identity = serde_json::json!({"instance":"instance","workflow":"workflow","controller":"parent",
        "controller_epoch":1,"task":"implementation","attempt":1,"contract":1,
        "request":"implementation","session":1,"run":1});
    let content = "Read this exact fixture specification.";
    let source = serde_json::json!({"kind":"Spec","reference":"SPEC.md",
        "digest":format!("{:x}",Sha256::digest(content.as_bytes()))});
    let review = serde_json::json!({"implementation":"implementation","axis":"Spec",
        "target":{"coding":{"assignment":identity,
            "publication":{"assignment":identity,"kind":"Submission","version":1,"digest":"a".repeat(64)},
            "candidate":{"path":"/assignment/candidate-v1.json","digest":"b".repeat(64),"workspace":"coding","base":"c".repeat(40),"git_identity":"generation","changed_artifacts":["README.md"]},
            "source_contract_digest":"d".repeat(64)},"sources":[source]},
        "source_documents":[{"source":source,"content":content}]});
    let typed: ReviewAssignment = serde_json::from_value(review.clone()).unwrap();
    assert!(
        typed.validate().is_ok(),
        "valid review must reach the unsupported-provider refusal"
    );
    let coding = serde_json::json!({"requested_base":"HEAD","branch":"unsupported-claude","authorized_paths":["README.md"],"required_checks":["cargo test --offline"]});
    let typed: CodingContract = serde_json::from_value(coding.clone()).unwrap();
    assert!(typed.validate().is_ok());
    for (extension, payload, reason) in [
        (
            "coding",
            coding,
            "Claude coding delegation is not supported",
        ),
        (
            "review",
            review,
            "Claude independent review delegation is not supported",
        ),
    ] {
        for mandatory in [false, true] {
            let mut args = serde_json::json!({"project":PROJECT,"workspace":f.workspace,"provider":"claude",
                "model_provider":"native","model":"fixture-model","scope":"Unsupported mode must not allocate",
                "acceptance_evidence":["independent evidence"]});
            args[extension] = payload.clone();
            if mandatory {
                args["mandatory_capabilities"] = serde_json::json!(["read", "result_file"]);
            }
            let reply = f.tool("ovrcr_start", args);
            assert_eq!(reply["dispatch"], "Refused", "{reply}");
            assert!(reply["handle"].is_null());
            assert_eq!(reply["reason"], reason);
        }
    }
    assert_eq!(f.child_count(), 0);
    assert!(!f.profile.join("delegation-discovery-count").exists());
    assert!(!f.profile.join("delegation-launch.json").exists());
    assert!(
        !f.proxy
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| matches!(request, DelegationRequest::Prepare { .. }))
    );
}

#[test]
fn public_claude_sealed_policy_replacement_refuses_real_read_and_publication() {
    for helpers in [false, true] {
        let f = ClaudeFixture::new("policy-before-publish");
        let started = f.started_policy(helpers);
        fs::write(f.live.root.path().join("allow-result"), b"go").unwrap();
        wait_file(&started.brief.with_extension("json.published"));
        let read: serde_json::Value = serde_json::from_slice(
            &fs::read(started.brief.with_extension("json.replaced-read")).unwrap(),
        )
        .unwrap();
        let published: serde_json::Value = serde_json::from_slice(
            &fs::read(started.brief.with_extension("json.published")).unwrap(),
        )
        .unwrap();
        assert_eq!(read["status"], "refused", "{read}");
        assert_eq!(published["error"], "refused", "{published}");
        assert_eq!(published["details"]["status"], "refused");
        assert_eq!(published["details"]["code"], "Conflict");
        assert!(published["details"].get("sent").is_none());
        let current = f.reconcile(&started.handle);
        assert_eq!(current.identity, started.identity);
        assert!(current.submission.is_none());
        assert!(current.publication.is_none());
        assert!(current.acceptance_ref.is_none());
        assert!(!started.result_path.exists());
        assert_eq!(current.acceptance, "pending");
    }
}

#[test]
fn public_claude_discovery_running_child_is_cleaned_before_launch() {
    let f = ClaudeFixture::new("discovery-running-after-eof");
    let started = f.started();
    assert!(
        !f.profile
            .join("delegation-discovery-natural-expiry.json")
            .exists()
    );
    let eof: serde_json::Value =
        serde_json::from_slice(&fs::read(f.profile.join("delegation-discovery-eof.json")).unwrap())
            .unwrap();
    let pid = eof["pid"].as_u64().unwrap();
    assert!(pid > 0 && pid <= i32::MAX as u64);
    // Closed fixture PID marker only; group identity/absence is separately probed
    // by the existing guarded ownership receipts, never inferred from this file.
    let pid_probe = unsafe { libc::kill(pid as i32, 0) };
    let pid_errno = std::io::Error::last_os_error().raw_os_error();
    assert_eq!((pid_probe, pid_errno), (-1, Some(libc::ESRCH)));
    let group_probe = unsafe { libc::kill(-(pid as i32), 0) };
    let group_errno = std::io::Error::last_os_error().raw_os_error();
    assert_eq!((group_probe, group_errno), (-1, Some(libc::ESRCH)));
    assert_eq!(started.dispatch, DispatchStatus::Applied);
    assert!(started.identity.session.is_some());
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_claude_discovery_unknown_postwait_refuses_before_allocation() {
    let f = ClaudeFixture::new("result");
    let fault = f.live.root.path().join("fault");
    fs::write(&fault, "claude-discovery-postwait-unknown|preallocation|0").unwrap();
    let before = f.child_count();
    let reply = f.start_claude();
    assert_eq!(reply["dispatch"], "Refused", "{reply}");
    assert!(reply["handle"].is_null());
    assert_eq!(f.child_count(), before);
    assert_eq!(
        fs::read_to_string(fault.with_extension("hit")).unwrap(),
        "claude-discovery-postwait-unknown"
    );
    assert!(!fault.exists());
    // Fault simulates unknown qualification only after the real wait/group probe;
    // emitted preflight owners still require actual final PS/kernel absence.
}
