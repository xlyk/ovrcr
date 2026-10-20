//! Native-shaped Codex fixtures through production catalog/launch/reporting/MCP paths.
//! Installed policy/model/provider acceptance remains separate.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MODEL: &str = "fixture-codex-model";

struct CodexFixture {
    proxy: ControlProxy,
    live: Option<Live>,
    parent: SessionId,
    workspace: String,
    sequence: std::cell::Cell<u32>,
}

impl std::ops::Deref for CodexFixture {
    type Target = Live;
    fn deref(&self) -> &Live {
        self.live.as_ref().unwrap()
    }
}

impl CodexFixture {
    fn new(reported_model: &str) -> Self {
        let live = Live::idle().bounded();
        let fault = live.root.path().join("fault");
        let proxy = ControlProxy::new(
            live.root.path().join("proxy.sock"),
            live.socket.clone(),
            fault.clone(),
        );
        let fixture_root = live.root.path().join("codex-fixture");
        fs::create_dir(&fixture_root).unwrap();
        let profile = live.root.path().join(".codex");
        fs::create_dir(&profile).unwrap();
        fs::write(profile.join("config.toml"), "").unwrap();
        let profile = profile.canonicalize().unwrap();
        fs::create_dir_all(live.root.path().join(".pi/agent")).unwrap();
        let bin = live.claude.command.parent().unwrap();
        let pi = bin.join("pi");
        fs::write(
            &pi,
            include_str!("../fixtures/pi/research_pi.sh")
                .replace("exec node ", "stty -icanon || exit\nexec node "),
        )
        .unwrap();
        fs::set_permissions(&pi, fs::Permissions::from_mode(0o700)).unwrap();
        let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
        let codex_host = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pi/codex_delegation_host.mjs");
        let codex = bin.join("codex");
        fs::write(
            &codex,
            format!("#!/bin/sh\nexec node '{}' \"$@\"\n", codex_host.display()),
        )
        .unwrap();
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).unwrap();
        live.start_binary_env(&[
            ("OVRCR_TEST_DELEGATION", "1".as_ref()),
            ("OVRCR_TEST_PI_HOST", host.as_os_str()),
            (
                "OVRCR_TEST_CONTROL_BINARY",
                std::ffi::OsStr::new(env!("CARGO_BIN_EXE_ovrcr")),
            ),
            (
                "OVRCR_TEST_DELEGATION_CONTROL_SOCKET",
                proxy.path.as_os_str(),
            ),
            ("OVRCR_TEST_DELEGATION_FAULT", fault.as_os_str()),
            ("OVRCR_TEST_CODEX_ROOT", fixture_root.as_os_str()),
            ("OVRCR_TEST_CODEX_REPORTED_MODEL", reported_model.as_ref()),
            ("CODEX_HOME", profile.as_os_str()),
        ]);
        live.ready("codex-delegation");
        let workspace = cli_json(
            &live,
            &[
                "workspace",
                "get",
                "--project",
                PROJECT,
                "--branch",
                "codex-delegation",
            ],
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let allowed = live
            .command()
            .args([
                "settings",
                "set",
                "delegation.allowed_harnesses",
                "[\"pi\",\"codex\"]",
            ])
            .output()
            .unwrap();
        assert!(allowed.status.success(), "explicit fixture harness policy");
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
            panic!("create managed Pi parent")
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
                panic!()
            };
            if sessions
                .iter()
                .any(|s| s.id == parent.id && s.agent.is_some())
            {
                break;
            }
            assert!(Instant::now() < deadline, "parent did not bind");
            std::thread::sleep(Duration::from_millis(10));
        }
        Self {
            proxy,
            live: Some(live),
            parent: parent.id,
            workspace,
            sequence: std::cell::Cell::new(0),
        }
    }

    fn next(&self) -> u32 {
        let value = self.sequence.get();
        self.sequence.set(value + 1);
        value
    }
    fn call(&self, name: &str, args: Value) -> Value {
        let output = self.root.path().join(format!("tool-{}.json", self.next()));
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tool(self, self.parent, name, args, &output)
        })) {
            Ok(value) => value,
            Err(payload) => {
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr(),
                    "CODEX_TOOL_PARENT_DIAGNOSTIC tool={} parent={} call={} reply_present={}",
                    match name {
                        "ovrcr_start" => "start",
                        "ovrcr_capabilities" => "capabilities",
                        "ovrcr_collect" => "collect",
                        _ => "other",
                    },
                    self.parent.0,
                    self.sequence.get(),
                    output.is_file(),
                );
                std::panic::resume_unwind(payload)
            }
        }
    }
    fn reply(&self, name: &str, args: Value) -> Value {
        tool_reply(
            self,
            self.parent,
            name,
            args,
            &self.root.path().join(format!("tool-{}.json", self.next())),
        )
    }
    fn request_args(&self, scope: &str) -> Value {
        json!({"project":PROJECT,"workspace":self.workspace,"scope":scope,
            "provider":"codex","model_provider":"openai","model":MODEL,
            "acceptance_evidence":["README citation"]})
    }
    fn start(&self, scope: &str) -> AssignmentSnapshot {
        let value = self.call("ovrcr_start", self.request_args(scope));
        let snapshot: AssignmentSnapshot = serde_json::from_value(value.clone())
            .unwrap_or_else(|_| panic!("Codex discovery/launch not implemented: {value}"));
        assert_eq!(snapshot.dispatch, DispatchStatus::Applied, "{snapshot:?}");
        wait_file(&self.child_root(&snapshot).join("brief-v1.json"));
        snapshot
    }
    fn collect(&self, snapshot: &AssignmentSnapshot) -> AssignmentSnapshot {
        serde_json::from_value(self.call("ovrcr_collect", json!({"handle":snapshot.handle})))
            .unwrap()
    }
    fn child_root(&self, snapshot: &AssignmentSnapshot) -> std::path::PathBuf {
        self.root
            .path()
            .join("codex-fixture")
            .join(snapshot.identity.session.unwrap().0.to_string())
    }
    fn child(&self, snapshot: &AssignmentSnapshot) -> SessionSummary {
        let Response::Inventory { sessions, .. } = self.request(Request::Inspect) else {
            panic!()
        };
        sessions
            .into_iter()
            .find(|s| Some(s.id) == snapshot.identity.session)
            .unwrap()
    }
    fn action(&self, snapshot: &AssignmentSnapshot, mut value: Value) -> Value {
        let sequence = self.next();
        value["sequence"] = sequence.into();
        let root = self.child_root(snapshot);
        let temporary = root.join("command.tmp");
        fs::write(&temporary, serde_json::to_vec(&value).unwrap()).unwrap();
        fs::rename(temporary, root.join("command.json")).unwrap();
        let reply = root.join(format!("reply-{sequence}.json"));
        wait_file(&reply);
        serde_json::from_slice(&fs::read(reply).unwrap()).unwrap()
    }
    fn inputs(&self, snapshot: &AssignmentSnapshot) -> usize {
        fs::read_to_string(self.child_root(snapshot).join("brief-inputs.jsonl"))
            .unwrap()
            .lines()
            .count()
    }
    fn publish(&self, snapshot: &AssignmentSnapshot) -> AssignmentSnapshot {
        let reply = self.action(snapshot, json!({"operation":"publish"}));
        assert!(reply.get("error").is_none(), "{reply}");
        self.collect(snapshot)
    }
}

impl Drop for CodexFixture {
    fn drop(&mut self) {
        let live = self.live.take().unwrap();
        let root = live.root.path().to_owned();
        let server = live.server_pid().unwrap();
        let groups = live.session_groups();
        for group in &groups {
            live.own_group(*group);
        }
        self.proxy.retire();
        drop(live);
        let absent = |pid| unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        let clean =
            absent(server as i32) && groups.iter().all(|group| absent(-*group)) && !root.exists();
        eprintln!(
            "CODEX_DRAFT_CLEANUP server={server} groups={groups:?} root={} clean={clean}",
            root.display()
        );
        if !std::thread::panicking() {
            assert!(clean, "owned fixture cleanup must be verified");
        }
    }
}

#[test]
fn codex_requested_model_and_explicit_policy_reach_native_launch_without_substitution() {
    let f = CodexFixture::new(MODEL);
    let capabilities = f.call(
        "ovrcr_capabilities",
        json!({"provider":"codex","model_provider":"openai","model":MODEL}),
    );
    assert_eq!(capabilities["resolved_choice"]["provider"], "codex");
    let started = f.start("Read README without changing files.");
    assert!(started.submission.is_none());
    assert_eq!(started.acceptance, "pending");
    let child = f.child(&started);
    assert_eq!(child.run, started.identity.run.unwrap());
    assert_eq!(
        child.kind,
        SessionKind::Agent {
            name: "codex".into()
        }
    );
    assert_eq!(child.agent.as_ref().unwrap().binding.generation, 1);
    assert_eq!(
        child
            .agent
            .as_ref()
            .unwrap()
            .metrics
            .as_ref()
            .unwrap()
            .sample
            .model
            .as_deref(),
        Some(MODEL)
    );
    let startup: Value =
        serde_json::from_slice(&fs::read(f.child_root(&started).join("startup.json")).unwrap())
            .unwrap();
    let argv: Vec<String> = serde_json::from_value(startup["argv"].clone()).unwrap();
    for pair in [
        ["--model", MODEL],
        ["--profile", "ovrcr-research"],
        ["--ask-for-approval", "never"],
    ] {
        assert!(
            argv.windows(2)
                .any(|values| values[0] == pair[0] && values[1] == pair[1]),
            "missing explicit {pair:?}: {argv:?}"
        );
    }
    assert!(argv.iter().any(|arg| arg == "--no-daemon"));
    assert!(argv.iter().any(|arg| arg == "--strict-config"));
    assert!(!argv.iter().any(|arg| arg == "--sandbox"));
    let profile: toml::Value = toml::from_str(startup["profile"].as_str().unwrap()).unwrap();
    let policy = &profile["permissions"]["ovrcr-research"];
    let files = policy["filesystem"].as_table().unwrap();
    assert_eq!(files[":root"].as_str(), Some("deny"));
    assert_eq!(
        files[startup["codex_home"].as_str().unwrap()].as_str(),
        Some("deny")
    );
    assert_eq!(
        files[startup["artifact_root"].as_str().unwrap()].as_str(),
        Some("read")
    );
    assert!(files.values().all(|v| v.as_str() != Some("write")));
    assert_eq!(policy["network"]["enabled"].as_bool(), Some(false));
    assert_eq!(
        profile["shell_environment_policy"]["inherit"].as_str(),
        Some("none")
    );
    let publisher = &profile["mcp_servers"]["ovrcr_child"];
    assert_eq!(
        publisher["args"].as_array().unwrap()[0].as_str(),
        Some("delegation-publisher")
    );
    assert_eq!(publisher["enabled_tools"].as_array().unwrap().len(), 2);
    assert_eq!(publisher["env_vars"].as_array().unwrap().len(), 4);
    assert_eq!(
        startup["delegation_session"],
        started.identity.session.unwrap().0
    );
    assert_eq!(startup["raw_hook_token_present"], false);
    assert_eq!(startup["delegation_token_present"], true);
    let reporter: toml::Value =
        toml::from_str(startup["reporter_config"].as_str().unwrap()).unwrap();
    assert!(reporter["hooks"]["SessionStart"].is_array());
    assert!(!argv.iter().any(|arg| {
        [
            "--approve-for-me",
            "--full-auto",
            "--dangerously-bypass-hook-trust",
        ]
        .contains(&arg.as_str())
    }));
    let mut missing = f.request_args("Unavailable exact model must never launch");
    missing["model"] = "definitely-unavailable-codex-model".into();
    let refused = f.call("ovrcr_start", missing);
    assert_eq!(refused["dispatch"], "Refused");
    assert!(refused["identity"]["session"].is_null());
}

#[test]
fn codex_symlinked_executable_preserves_reporting_binding_and_initial_input() {
    let f = CodexFixture::new(MODEL);
    let entry = f.claude.command.parent().unwrap().join("codex");
    let target = entry.with_file_name("codex-native-fixture");
    fs::rename(&entry, &target).unwrap();
    std::os::unix::fs::symlink(&target, &entry).unwrap();
    assert_ne!(entry.file_name(), target.file_name());
    assert_eq!(fs::read_link(&entry).unwrap(), target);

    let started = f.start("Read README through the symlinked Codex entry.");
    let child = f.child(&started);
    assert_eq!(child.run, started.identity.run.unwrap());
    let agent = child.agent.as_ref().expect("Codex Reporter must bind");
    assert_eq!(agent.binding.provider, AgentProvider::Codex);
    assert_eq!(agent.binding.generation, 1);
    assert_eq!(
        agent.metrics.as_ref().unwrap().sample.model.as_deref(),
        Some(MODEL)
    );
    let startup: Value =
        serde_json::from_slice(&fs::read(f.child_root(&started).join("startup.json")).unwrap())
            .unwrap();
    assert_eq!(startup["conversation"], agent.binding.conversation);
    assert_eq!(f.inputs(&started), 1);
    let input: Value = serde_json::from_str(
        fs::read_to_string(f.child_root(&started).join("brief-inputs.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(
        input["identity"],
        serde_json::to_value(&started.identity).unwrap()
    );
    assert_eq!(input["version"], 1);
    assert_eq!(input["conversation"], agent.binding.conversation);
}

#[test]
fn codex_same_child_followup_versions_files_and_late_result_is_rejected() {
    let f = CodexFixture::new(MODEL);
    let first = f.start("Read README.");
    let complete = f.publish(&first);
    assert!(complete.submission.is_some());
    assert_eq!(complete.acceptance, "pending");
    let original = fs::read(&complete.result_path).unwrap();
    f.action(&first, json!({"operation":"stop"}));
    let followed: AssignmentSnapshot = serde_json::from_value(f.call(
        "ovrcr_followup",
        json!({
            "handle":complete.handle,"expected_identity":complete.identity,
            "expected_result_version":1,"expected_control_epoch":complete.control.epoch,
            "text":"Clarify the README conclusion.","revised_contract":null
        }),
    ))
    .unwrap();
    assert_eq!(followed.identity, complete.identity);
    assert_eq!(followed.conversation, complete.conversation);
    assert_eq!(followed.result_version, 2);
    assert!(followed.submission.is_none());
    wait_file(&f.child_root(&first).join("brief-v2.json"));
    assert!(
        f.action(&followed, json!({"operation":"publish","variant":"stale"}))["error"].is_string()
    );
    let second = f.publish(&followed);
    assert_eq!(second.submission.unwrap().version, 2);
    assert_eq!(fs::read(&complete.result_path).unwrap(), original);
    assert_eq!(f.inputs(&first), 2);
    assert_eq!(f.child(&first).agent.unwrap().binding.generation, 1);
}

#[test]
fn codex_question_exact_answer_and_replay_use_registered_tools_and_real_child_input() {
    let f = CodexFixture::new(MODEL);
    let started = f.start("QUESTION read only the authorized README section.");
    wait_file(&f.child_root(&started).join("question-v1.json"));
    let current = f.collect(&started);
    let question = current
        .questions
        .iter()
        .find(|q| q.current)
        .unwrap()
        .reference
        .clone();
    let collected = f.call(
        "ovrcr_collect_question",
        json!({"handle":current.handle,"reference":question}),
    );
    assert_eq!(
        collected["collected_question"]["question"],
        "Which README section is authorized?"
    );
    let args = json!({"handle":current.handle,"question":question,
        "expected_control_epoch":current.control.epoch,"basis":"ExistingAuthority",
        "scope":"Read README only.","answer":"Introduction only."});
    f.action(&started, json!({"operation":"approval"}));
    let blocked = f.call("ovrcr_answer_question", args.clone());
    assert_eq!(blocked["dispatch"], "Refused");
    assert!(!f.child_root(&started).join("answer-received.json").exists());
    f.action(&started, json!({"operation":"stop"}));
    let answered = f.call("ovrcr_answer_question", args.clone());
    let answer_request = answered["request"].as_str().unwrap();
    assert!(!answer_request.is_empty());
    assert_ne!(answered["request"], blocked["request"]);
    let delivered: AssignmentSnapshot = serde_json::from_value(answered.clone()).unwrap();
    let receipt = delivered
        .receipts
        .iter()
        .find(|receipt| receipt.request == answer_request)
        .expect("saved receipt for the delivered answer");
    assert_eq!(receipt.operation, DispatchOperation::AnswerQuestion);
    assert_eq!(receipt.outcome, DispatchOutcome::Applied);
    let input = receipt.input.as_ref().unwrap();
    assert!(input.submit_written);
    assert_eq!(input.written_bytes, input.total_bytes);
    assert_eq!(delivered.control, current.control);
    wait_file(&f.child_root(&started).join("answer-received.json"));
    let received: Value = serde_json::from_slice(
        &fs::read(f.child_root(&started).join("answer-received.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        received["question"],
        serde_json::to_value(&question).unwrap()
    );
    assert_eq!(received["answer"], "Introduction only.");
    assert_eq!(received["request"], answer_request);
    // Recover the delivered request, not the earlier refused native-input attempt.
    let request = f
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|request| {
            matches!(request, DelegationRequest::AnswerQuestion { answer, .. }
                if answer.request == answer_request)
        })
        .unwrap()
        .clone();
    assert!(matches!(
        f.request(Request::Delegation(Box::new(request))),
        Response::Delegation(_)
    ));
    assert_eq!(
        fs::read_to_string(f.child_root(&started).join("answers.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(f.inputs(&started), 1);
    let mut stale = args;
    stale["question"]["question_version"] = 2.into();
    assert_eq!(
        f.call("ovrcr_answer_question", stale)["dispatch"],
        "Refused"
    );
    let events = serde_json::to_string(&f.request(Request::Events { follow: false })).unwrap();
    assert!(!events.contains("Which README section is authorized?"));
    assert!(!events.contains("Introduction only."));
}

#[test]
fn codex_native_approval_refuses_controller_followup_and_exact_replay_without_pty_bytes() {
    let f = CodexFixture::new(MODEL);
    let started = f.start("Read README.");
    assert_eq!(f.inputs(&started), 1);
    f.action(&started, json!({"operation":"approval"}));
    let native = f.child(&started).agent.unwrap();
    assert_eq!(native.input_requests.len(), 1);
    assert_eq!(native.input_requests[0].kind, InputKind::Approval);
    let observed = f.collect(&started);
    assert!(observed.questions.is_empty());
    assert!(observed.submission.is_none());
    let reply = f.call(
        "ovrcr_followup",
        json!({
            "handle":started.handle,"expected_identity":started.identity,
            "expected_result_version":started.result_version,
            "expected_control_epoch":started.control.epoch,
            "text":"Continue reading the README without changing authority.",
            "revised_contract":null
        }),
    );
    let request = reply["request"].as_str().unwrap().to_owned();
    let refused: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
    let receipt = refused
        .receipts
        .iter()
        .find(|receipt| {
            receipt.operation == DispatchOperation::Followup && receipt.request == request
        })
        .unwrap()
        .clone();
    assert_eq!(
        receipt.outcome,
        DispatchOutcome::Rejected,
        "native approval must reject ordinary Controller followup before PTY input"
    );
    assert_eq!(refused.dispatch, DispatchStatus::Refused);
    let input = receipt.input.as_ref().unwrap();
    assert_eq!(input.written_bytes, 0);
    assert!(!input.submit_written);
    assert_eq!(f.inputs(&started), 1);
    assert!(!f.child_root(&started).join("brief-v2.json").exists());
    assert_eq!(
        f.child(&started).agent.unwrap().input_requests,
        native.input_requests
    );

    f.action(&started, json!({"operation":"stop"}));
    assert!(f.child(&started).agent.unwrap().input_requests.is_empty());
    let replayed: AssignmentSnapshot = serde_json::from_value(f.call(
        "ovrcr_followup",
        json!({"handle":started.handle,"request":request}),
    ))
    .unwrap();
    let replay_receipt = replayed
        .receipts
        .iter()
        .find(|item| {
            item.operation == DispatchOperation::Followup && item.request == receipt.request
        })
        .unwrap();
    assert_eq!(replay_receipt, &receipt);
    assert_eq!(replayed.dispatch, DispatchStatus::Refused);
    assert_eq!(f.inputs(&started), 1);
    assert!(!f.child_root(&started).join("brief-v2.json").exists());
    let after = f.child(&started).agent.unwrap();
    assert_eq!(after.binding, native.binding);
    assert!(after.input_requests.is_empty());
}

#[test]
fn codex_native_approval_is_attributed_but_login_and_stop_do_not_invent_question_or_result() {
    let f = CodexFixture::new(MODEL);
    let started = f.start("Read README.");
    f.action(&started, json!({"operation":"approval"}));
    let child = f.child(&started);
    let input = &child.agent.unwrap().input_requests;
    assert_eq!(input.len(), 1);
    assert_eq!(input[0].kind, InputKind::Approval);
    let collected = f.collect(&started);
    assert!(
        collected.questions.is_empty(),
        "native approval text must not be fabricated"
    );
    assert!(collected.submission.is_none());
    f.action(&started, json!({"operation":"stop"}));
    f.action(&started, json!({"operation":"login"}));
    f.action(&started, json!({"operation":"foreign-root"}));
    let missing = f.collect(&started);
    assert!(missing.questions.is_empty());
    assert!(
        missing.submission.is_none(),
        "Observed Stop/login output is not completion"
    );
    assert_eq!(missing.acceptance, "pending");
}

#[test]
fn codex_missing_malformed_incomplete_and_changed_result_never_become_success() {
    let f = CodexFixture::new(MODEL);
    let started = f.start("Read README.");
    assert!(f.collect(&started).submission.is_none());
    for variant in ["malformed", "incomplete", "foreign-run"] {
        assert!(
            f.action(&started, json!({"operation":"publish","variant":variant}))["error"]
                .is_string(),
            "{variant} must fail through production publication"
        );
        assert!(f.collect(&started).submission.is_none());
    }
    let completed = f.publish(&started);
    fs::write(&completed.result_path, b"{partial").unwrap();
    assert!(f.reply("ovrcr_collect", json!({"handle":started.handle}))["error"].is_string());
}

#[test]
fn codex_lost_initial_reply_reconciles_without_second_native_input() {
    let f = CodexFixture::new(MODEL);
    f.proxy.arm(ControlRule {
        operation: "InitialInput",
        loss: 1,
        fault: None,
    });
    let unknown = f.call("ovrcr_start", f.request_args("Read README once."));
    let handle = unknown["handle"].as_str().unwrap();
    let recovered: AssignmentSnapshot =
        serde_json::from_value(f.call("ovrcr_start", json!({"handle":handle}))).unwrap();
    assert_eq!(recovered.dispatch, DispatchStatus::Applied);
    wait_file(&f.child_root(&recovered).join("brief-v1.json"));
    assert_eq!(f.inputs(&recovered), 1);
    let completed = f.publish(&recovered);
    f.call("ovrcr_start", json!({"handle":handle}));
    assert_eq!(f.inputs(&completed), 1);
}

#[test]
fn codex_child_publication_cannot_gain_parent_control_or_use_replaced_conversation() {
    let f = CodexFixture::new(MODEL);
    let started = f.start("Read README.");
    assert!(
        f.action(&started, json!({"operation":"publish","unauthorized":true}))["error"].is_string()
    );
    assert!(f.action(&started, json!({"operation":"parent-control"}))["error"].is_string());
    f.action(
        &started,
        json!({"operation":"replace","return_original":true}),
    );
    assert!(f.child(&started).agent.unwrap().binding.generation > 1);
    assert!(f.action(&started, json!({"operation":"publish"}))["error"].is_string());
    let followup = f.call("ovrcr_followup", json!({"handle":started.handle,
        "expected_identity":started.identity,"expected_result_version":1,
        "expected_control_epoch":started.control.epoch,"text":"Do not reach replacement.","revised_contract":null}));
    assert_eq!(followup["dispatch"], "Refused");
    assert_eq!(f.inputs(&started), 1);
}

#[test]
fn codex_initial_input_waits_for_exact_native_model_and_never_adopts_a_later_generation() {
    for reported in ["omit", "wrong-native-model"] {
        let f = CodexFixture::new(reported);
        let value = f.call(
            "ovrcr_start",
            f.request_args("Wait for real native model evidence."),
        );
        let waiting: AssignmentSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(waiting.dispatch, DispatchStatus::Launched);
        let root = f.child_root(&waiting);
        wait_file(&root.join("startup.json"));
        assert!(!root.join("brief-inputs.jsonl").exists());
        f.action(
            &waiting,
            json!({"operation":"replace","return_original":true}),
        );
        let refused = f.call("ovrcr_start", json!({"handle":waiting.handle}));
        assert_eq!(refused["dispatch"], "Refused");
        assert!(
            !root.join("brief-inputs.jsonl").exists(),
            "A-B-A cannot gain input authority"
        );
    }
}

#[test]
fn codex_retired_run_cannot_publish_and_stale_initial_receipt_cannot_write_to_new_run() {
    let f = CodexFixture::new(MODEL);
    let started = f.start("Read README once.");
    f.publish(&started);
    let publication = f
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|request| matches!(request, DelegationRequest::Publish { .. }))
        .unwrap()
        .clone();
    let initial = f.proxy.original(DispatchOperation::InitialInput);
    let child = f.child(&started);
    for group in f.session_groups() {
        f.own_group(group);
    }
    assert_eq!(
        f.request(Request::CloseTerminal {
            session: child.id,
            expected_run: child.run
        }),
        Response::Ok
    );
    let archived = f.child(&started);
    assert!(archived.archived);
    assert_ne!(archived.run, child.run);
    assert_eq!(
        f.request(Request::UnarchiveSession {
            session: child.id,
            expected_run: archived.run
        }),
        Response::Ok
    );
    let returned = f.child(&started);
    assert!(!returned.archived);
    assert_ne!(returned.run, child.run);
    assert!(
        !returned.phase.is_live(),
        "unarchive never resumes the retired child"
    );
    assert!(matches!(
        f.request(Request::Delegation(Box::new(publication))),
        Response::Error { .. }
    ));
    let receipt = f.request(Request::Delegation(Box::new(initial)));
    assert!(matches!(
        receipt,
        Response::Error { .. } | Response::Delegation(_)
    ));
    assert_eq!(f.child(&started).run, returned.run);
    assert!(
        !f.child(&started).phase.is_live(),
        "stale reconciliation must never restart a run"
    );
}

#[test]
fn codex_lost_publication_reply_is_reconciled_from_files_without_stop_or_reexecution() {
    let f = CodexFixture::new(MODEL);
    let started = f.start("Read README and publish without a native Stop.");
    f.proxy.arm(ControlRule {
        operation: "Publish",
        loss: 1,
        fault: None,
    });
    let unknown = f.action(&started, json!({"operation":"publish"}));
    assert!(unknown["error"].is_string());
    let found = f.collect(&started);
    assert!(
        found.submission.is_some(),
        "lost helper reply cannot hide a committed file"
    );
    assert_eq!(found.acceptance, "pending");
    assert_eq!(f.inputs(&started), 1);
    let bytes = fs::read(&found.result_path).unwrap();
    let digest = found.publication.as_ref().unwrap().identity.digest.clone();
    let repeated = f.publish(&started);
    assert_eq!(repeated.publication.unwrap().identity.digest, digest);
    assert_eq!(fs::read(&found.result_path).unwrap(), bytes);
    assert_eq!(f.inputs(&started), 1);
}

#[test]
fn codex_custom_backend_and_unenforceable_limits_refuse_without_a_child() {
    let f = CodexFixture::new(MODEL);
    let before = f.session_groups();
    let custom = f.reply(
        "ovrcr_capabilities",
        json!({"provider":"codex","model_provider":"custom","model":MODEL}),
    );
    assert!(custom["error"].is_string() || custom["details"]["resolved_choice"].is_null());
    let mut limited = f.request_args("Unsupported native descendant cap must not start work");
    limited["required_limits"] = json!({"max_native_children":1});
    let refused = f.reply("ovrcr_start", limited);
    assert!(refused["error"].is_string() || refused["details"]["identity"]["session"].is_null());
    assert_eq!(f.session_groups(), before);
}

#[test]
fn codex_coding_refuses_before_intent_workspace_profile_or_process_effects() {
    let f = CodexFixture::new(MODEL);
    let report = f.call(
        "ovrcr_capabilities",
        json!({"provider":"codex","model_provider":"openai","model":MODEL}),
    );
    assert_eq!(report["capabilities"], json!(["read", "result_file"]));
    let before_groups = f.session_groups();
    let before_workspaces = cli_json(&f, &["workspace", "list", "--project", PROJECT]);
    let before_branches = live::git(
        &f.repo,
        &["for-each-ref", "--format=%(refname)", "refs/heads/"],
    );
    let before_worktrees = live::git(&f.repo, &["worktree", "list", "--porcelain"]);
    let allocations = || {
        rusqlite::Connection::open(f.config.join("registry.sqlite3"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM delegations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let before_allocations = allocations();
    let coding = json!({"requested_base":"main","branch":"coding/codex-refused",
        "authorized_paths":["README"],"required_checks":["unit"]});
    for explicit_mandatory in [false, true] {
        let mut args = f.request_args("Unsupported coding must not allocate anything.");
        args["workspace"] = "codex-coding-refused".into();
        args["permitted_actions"] = json!(["read", "write", "edit", "check"]);
        args["coding"] = coding.clone();
        if explicit_mandatory {
            args["mandatory_capabilities"] = json!([
                "isolated_workspace",
                "immutable_candidate",
                "write",
                "edit",
                "check"
            ]);
        }
        let refused = f.call("ovrcr_start", args);
        assert_eq!(refused["dispatch"], "Refused", "{refused}");
        assert!(
            refused["handle"].is_null(),
            "preflight must not allocate an intent"
        );
        assert!(
            !f.proxy
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request, DelegationRequest::Prepare { .. }))
        );
    }
    let auth = f
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find_map(|request| {
            if let DelegationRequest::Capabilities { auth, .. } = request {
                Some(auth.clone())
            } else {
                None
            }
        })
        .unwrap();
    let workflow = auth.controller.as_ref().unwrap().workflow.clone();
    let contract: ResearchContract = serde_json::from_value(json!({
        "schema":1,"workflow":workflow,"controller":"parent","task":"task","request":"codex-coding-refused",
        "project":PROJECT,"workspace":"codex-coding-refused","scope":"Unsupported coding.",
        "provider":"codex","model_provider":"openai","model":MODEL,
        "permitted_actions":["read","write","edit","check"],"acceptance_evidence":["candidate"],
        "mandatory_capabilities":["read","result_file"],"coding":coding
    })).unwrap();
    assert!(matches!(
        f.request(Request::Delegation(Box::new(DelegationRequest::Prepare {
            auth: auth.clone(),
            contract: contract.clone(),
        }))),
        Response::Error { code: ErrorCode::InvalidRequest, message }
            if message == "Codex delegation supports read-only research; coding permissions are unavailable"
    ));
    let target_identity = json!({"instance":"instance","workflow":"workflow","controller":"parent",
        "controller_epoch":1,"task":"implementation","attempt":1,"contract":1,
        "request":"implementation","session":1,"run":1});
    let content = "Read this exact fixture specification.";
    let source = json!({"kind":"Spec","reference":"SPEC.md",
        "digest":format!("{:x}", Sha256::digest(content.as_bytes()))});
    let review = json!({"implementation":"implementation","axis":"Spec",
        "target":{"coding":{"assignment":target_identity,
            "publication":{"assignment":target_identity,"kind":"Submission","version":1,"digest":"a".repeat(64)},
            "candidate":{"path":"/assignment/candidate-v1.json","digest":"b".repeat(64),"workspace":"coding","base":"c".repeat(40),"git_identity":"generation","changed_artifacts":["change.txt"]},
            "source_contract_digest":"d".repeat(64)},"sources":[source]},
        "source_documents":[{"source":source,"content":content}]});
    let typed: ReviewAssignment = serde_json::from_value(review.clone()).unwrap();
    assert!(
        typed.validate().is_ok(),
        "valid review shape must reach harness refusal"
    );
    for explicit_mandatory in [false, true] {
        let mut args = f.request_args("Unsupported independent review must not allocate anything.");
        args["review"] = review.clone();
        if explicit_mandatory {
            args["mandatory_capabilities"] = json!(["read", "result_file"]);
        }
        let refused = f.call("ovrcr_start", args);
        assert_eq!(refused["dispatch"], "Refused", "{refused}");
        assert!(refused["handle"].is_null());
        assert_eq!(refused["transport"], "NotSent");
        assert_eq!(
            refused["reason"],
            "requested provider lacks authenticated typed independent-review publication; read/result_file alone is insufficient"
        );
    }
    let mut review_contract = contract;
    review_contract.coding = None;
    review_contract.review = Some(typed);
    review_contract.permitted_actions = vec!["read".into()];
    review_contract.provider = "pi".into();
    assert!(
        review_contract.validate().is_ok(),
        "Pi review remains supported"
    );
    review_contract.provider = "codex".into();
    assert!(matches!(
        f.request(Request::Delegation(Box::new(DelegationRequest::Prepare {
            auth,
            contract: review_contract,
        }))),
        Response::Error { code: ErrorCode::InvalidRequest, message }
            if message == "requested provider lacks authenticated typed independent-review publication; read/result_file alone is insufficient"
    ));
    assert!(
        !f.proxy
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| matches!(request, DelegationRequest::Prepare { .. }))
    );
    assert_eq!(
        fs::read_dir(f.root.path().join("codex-fixture"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(allocations(), before_allocations);
    assert_eq!(f.session_groups(), before_groups);
    assert_eq!(
        cli_json(&f, &["workspace", "list", "--project", PROJECT]),
        before_workspaces
    );
    assert_eq!(
        live::git(
            &f.repo,
            &["for-each-ref", "--format=%(refname)", "refs/heads/"]
        ),
        before_branches
    );
    assert_eq!(
        live::git(&f.repo, &["worktree", "list", "--porcelain"]),
        before_worktrees
    );
    assert!(!f.config.join("codex-children").exists());
}

#[test]
fn codex_prepare_only_reserves_exact_home_and_preserves_native_auth_on_explicit_resume() {
    use std::os::unix::fs::DirBuilderExt;
    let f = CodexFixture::new(MODEL);
    let Response::Inventory {
        sessions: before, ..
    } = f.request(Request::Inspect)
    else {
        panic!("inventory")
    };
    let mut args = f.request_args("Reserve native login home only; no task input.");
    args["prepare_only"] = true.into();
    let prepared: AssignmentSnapshot = serde_json::from_value(f.call("ovrcr_start", args)).unwrap();
    {
        let requests = f.proxy.requests.lock().unwrap().clone();
        assert!(
            !requests.iter().any(|r| matches!(
                r,
                DelegationRequest::Launch { .. } | DelegationRequest::SendInitial { .. }
            )),
            "prepare_only must never call Launch or SendInitial"
        );
    }
    assert_eq!(prepared.dispatch, DispatchStatus::Prepared);
    assert!(prepared.identity.session.is_none() && prepared.identity.run.is_none());
    assert!(prepared.receipts.iter().all(
        |r| r.operation == DispatchOperation::Prepare && r.outcome == DispatchOutcome::Applied
    ));
    let Response::Inventory {
        sessions: after, ..
    } = f.request(Request::Inspect)
    else {
        panic!("inventory")
    };
    assert_eq!(
        after.len(),
        before.len(),
        "reservation must create zero child sessions"
    );
    let instance = f.config.canonicalize().unwrap();
    let artifacts = instance.join("workflows").join(&prepared.handle);
    assert_eq!(
        prepared.brief.parent(),
        Some(artifacts.as_path()),
        "home derives from the Server's canonical artifact namespace"
    );
    let contract = fs::read(artifacts.join("contract-v1.json")).unwrap();
    let home = instance.join("codex-children").join(&prepared.handle);
    assert!(
        !home.exists(),
        "Prepare must not invoke the native constructor"
    );
    let duplicate: AssignmentSnapshot = serde_json::from_value(f.call(
        "ovrcr_start",
        json!({"handle":prepared.handle,"prepare_only":true}),
    ))
    .unwrap();
    assert_eq!(duplicate.identity, prepared.identity);
    assert_eq!(duplicate.result_version, prepared.result_version);
    assert_eq!(
        fs::read(artifacts.join("contract-v1.json")).unwrap(),
        contract
    );
    assert!(!home.exists());
    // Synthetic native-owned file, not a credential or actual login proof.
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let auth = home.join("auth.json");
    let sentinel = b"{\"fixture\":\"reserved-home-sentinel\"}\n";
    fs::write(&auth, sentinel).unwrap();
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!home.join("config.toml").exists());
    let resumed: AssignmentSnapshot =
        serde_json::from_value(f.call("ovrcr_start", json!({"handle":prepared.handle}))).unwrap();
    assert_eq!(resumed.dispatch, DispatchStatus::Applied);
    assert_eq!(resumed.handle, prepared.handle);
    let mut allocated = prepared.identity.clone();
    allocated.session = resumed.identity.session;
    allocated.run = resumed.identity.run;
    assert_eq!(
        allocated, resumed.identity,
        "same canonical assignment must acquire the child"
    );
    wait_file(&f.child_root(&resumed).join("startup.json"));
    let startup: Value =
        serde_json::from_slice(&fs::read(f.child_root(&resumed).join("startup.json")).unwrap())
            .unwrap();
    assert_eq!(startup["codex_home"].as_str(), home.to_str());
    assert_eq!(startup["artifact_root"].as_str(), artifacts.to_str());
    assert_eq!(
        fs::read(&auth).unwrap(),
        sentinel,
        "production constructor must preserve native auth in place"
    );
    assert_eq!(
        fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(home.join("config.toml").exists());
    assert_eq!(
        fs::read(artifacts.join("contract-v1.json")).unwrap(),
        contract
    );
    wait_file(&f.child_root(&resumed).join("brief-v1.json"));
    assert_eq!(
        f.inputs(&resumed),
        1,
        "only the explicit later resume may deliver the task"
    );
}

#[test]
fn codex_prepare_only_lost_prepare_request_and_reply_recover_without_launch_or_input() {
    for loss in [0, 1] {
        let f = CodexFixture::new(MODEL);
        f.proxy.arm(ControlRule {
            operation: "Prepare",
            loss,
            fault: None,
        });
        let mut args = f.request_args("Reserve native login home only; no task input.");
        args["prepare_only"] = true.into();
        let unknown = f.call("ovrcr_start", args);
        assert_eq!(unknown["dispatch"], "Unknown");
        let handle = unknown["handle"].as_str().unwrap();
        assert_eq!(
            unknown["recovery_arguments"],
            json!({"handle":handle,"prepare_only":true}),
            "prepare-only unknown must preserve non-advancing recovery arguments"
        );
        assert!(unknown.get("receipts").is_none());
        let prepared: AssignmentSnapshot =
            serde_json::from_value(f.call("ovrcr_start", unknown["recovery_arguments"].clone()))
                .unwrap();
        assert_eq!(prepared.dispatch, DispatchStatus::Prepared);
        assert!(prepared.identity.session.is_none() && prepared.identity.run.is_none());
        let duplicate: AssignmentSnapshot = serde_json::from_value(
            f.call("ovrcr_start", json!({"handle":handle,"prepare_only":true})),
        )
        .unwrap();
        assert_eq!(duplicate.identity, prepared.identity);
        let requests = f.proxy.requests.lock().unwrap().clone();
        assert!(
            !requests.iter().any(|r| matches!(
                r,
                DelegationRequest::Launch { .. } | DelegationRequest::SendInitial { .. }
            )),
            "lost Prepare recovery must never launch or send input"
        );
        let contracts: Vec<_> = requests
            .iter()
            .filter_map(|r| {
                if let DelegationRequest::Prepare { contract, .. } = r {
                    Some(contract)
                } else {
                    None
                }
            })
            .collect();
        assert!(
            contracts.iter().all(|value| **value == *contracts[0]),
            "retry must preserve the original canonical contract"
        );
    }
}
