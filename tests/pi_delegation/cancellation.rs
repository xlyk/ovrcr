//! Registered tools -> production control CLI -> binary Server -> real PTYs.
//! Pi's public keybinding API and model work are explicit fixtures; installed
//! loader and native InteractiveMode acceptance are separate evidence.
use super::observation::Fixture;
use super::*;
use std::os::unix::net::UnixStream;

struct Owned(Option<Fixture>);
impl Owned {
    fn new() -> Self {
        Self(Some(Fixture::new(10000)))
    }
}
impl std::ops::Deref for Owned {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        self.0.as_ref().unwrap()
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        let fixture = self.0.take().unwrap();
        let root = fixture.live.root.path().to_owned();
        let server = fixture.live.server_pid().unwrap();
        let mut groups = fixture.live.owned_groups();
        groups.extend(fixture.live.session_groups());
        groups.sort_unstable();
        groups.dedup();
        for group in &groups {
            fixture.live.own_group(*group);
        }
        drop(fixture);
        let absent = |pid| unsafe { libc::kill(pid, 0) } == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        let clean =
            absent(server as i32) && groups.iter().all(|group| absent(-*group)) && !root.exists();
        eprintln!(
            "CANCELLATION_CLEANUP root={} server={} groups={groups:?} clean={clean}",
            root.display(),
            server
        );
        if !std::thread::panicking() {
            assert!(clean, "owned process and resource cleanup");
        }
    }
}
fn inspect(f: &Fixture, child: &AssignmentSnapshot) -> AssignmentSnapshot {
    serde_json::from_value(f.tool("ovrcr_inspect", serde_json::json!({"handle":child.handle})))
        .unwrap()
}
fn bytes(f: &Fixture, child: &AssignmentSnapshot) -> u64 {
    fs::read_to_string(
        f.live
            .root
            .path()
            .join("input-bytes")
            .join(child.identity.session.unwrap().0.to_string()),
    )
    .unwrap_or_default()
    .lines()
    .map(|line| line.parse::<u64>().unwrap())
    .sum()
}
fn supported(f: &Fixture, child: &AssignmentSnapshot) {
    f.until(|| {
        f.summary(child.identity.session.unwrap())
            .agent
            .as_ref()
            .is_some_and(|agent| {
                agent.native_control.as_ref().is_some_and(|sample| {
                    sample.interrupt == NativeInterruptCapability::Escape && !sample.settled
                })
            })
    });
}
fn interrupt_args(child: &AssignmentSnapshot, request: &str) -> serde_json::Value {
    serde_json::json!({"handle":child.handle,"request":request,"expected_identity":child.identity,"expected_result_version":child.result_version,"expected_control_epoch":child.control.epoch})
}
fn confirm_args(child: &AssignmentSnapshot, request: &str, interrupted: &str) -> serde_json::Value {
    serde_json::json!({"handle":child.handle,"request":request,"interrupted_request":interrupted,"expected_control_epoch":child.control.epoch,"evidence":child.control_evidence})
}
fn refusal(f: &Fixture, name: &str, args: serde_json::Value, label: &str) -> serde_json::Value {
    let file = f.live.root.path().join(format!("refusal-{label}.json"));
    let reply = tool_reply(&f.live, f.parent, name, args, &file);
    assert!(
        reply.get("error").is_some() || reply["details"]["dispatch"] == "Refused",
        "{reply}"
    );
    reply
}
fn interrupt_receipt<'a>(child: &'a AssignmentSnapshot, request: &str) -> &'a DispatchReceipt {
    child
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::Interrupt && r.request == request)
        .unwrap()
}
fn terminal_contains(f: &Fixture, child: &AssignmentSnapshot, marker: &str) {
    let Response::TerminalText { text, .. } = f.live.request(Request::ReadTerminal {
        session: child.identity.session.unwrap(),
        max_lines: Some(200),
    }) else {
        panic!("terminal");
    };
    assert!(text.contains(marker), "{text}");
}

#[test]
fn production_question_answer_respects_interrupt_barrier_until_explicit_settlement_confirmation() {
    let f = Owned::new();
    let child = f.start("QUESTION_FILE_ONLY INTERRUPT_SETTLEMENT_GATE");
    wait_file(&child.brief.with_extension("json.question-published"));
    supported(&f, &child);
    let current = inspect(&f, &child);
    let question = current
        .questions
        .iter()
        .find(|q| q.current)
        .unwrap()
        .reference
        .clone();
    let interrupted: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_interrupt",
        interrupt_args(&current, "question-interrupt"),
    ))
    .unwrap();
    assert_eq!(
        interrupt_receipt(&interrupted, "question-interrupt").outcome,
        DispatchOutcome::Applied
    );
    wait_file(&child.brief.with_extension("json.interrupt-consumed"));
    let before = bytes(&f, &child);
    let args = |snapshot: &AssignmentSnapshot| serde_json::json!({"handle": snapshot.handle, "question": question, "expected_control_epoch": snapshot.control.epoch, "basis": "ExistingAuthority", "scope": "Read only the assigned README", "answer": "Use the Introduction section"});
    let blocked = f.tool("ovrcr_answer_question", args(&interrupted));
    assert_eq!(blocked["dispatch"], "Refused");
    assert_eq!(bytes(&f, &child), before);
    let collected: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_collect_question",
        serde_json::json!({"handle": child.handle, "reference": question}),
    ))
    .unwrap();
    assert!(
        collected.collected_question.is_some(),
        "interrupt barrier leaves read-only question evidence available"
    );
    fs::write(
        child.brief.with_extension("json.interrupt-settle"),
        "fresh same-turn settlement",
    )
    .unwrap();
    wait_file(&child.brief.with_extension("json.interrupt-settled"));
    let settled = inspect(&f, &child);
    assert!(matches!(
        settled.control.barrier,
        Some(ControlBarrier::Interrupt { .. })
    ));
    assert_eq!(
        f.tool("ovrcr_answer_question", args(&settled))["dispatch"],
        "Refused",
        "native settlement alone grants no authority"
    );
    assert_eq!(bytes(&f, &child), before);
    let confirmed: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_confirm_interrupt",
        confirm_args(&settled, "question-interrupt-confirm", "question-interrupt"),
    ))
    .unwrap();
    assert!(confirmed.control.barrier.is_none());
    let delivered: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_answer_question", args(&confirmed))).unwrap();
    wait_file(&child.brief.with_extension("json.answer-received"));
    assert_eq!(
        delivered
            .receipts
            .iter()
            .filter(|r| r.operation == DispatchOperation::AnswerQuestion
                && r.outcome == DispatchOutcome::Applied)
            .count(),
        1
    );
    assert_eq!(delivered.result_version, child.result_version);
    assert_eq!(delivered.execution_attempts, Some(1));
}

#[test]
fn production_interrupt_delivery_requires_fresh_settlement_and_keeps_same_child_followup() {
    interrupt_then_confirm(false);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn production_unknown_interrupt_never_replays_and_confirmation_preserves_original_receipt() {
    interrupt_then_confirm(true);
}

fn interrupt_then_confirm(unknown: bool) {
    let f = Owned::new();
    let first = f.start("NO_READY INTERRUPT_SETTLEMENT_GATE");
    supported(&f, &first);
    let before = bytes(&f, &first);
    if unknown {
        fs::write(
            f.live.root.path().join("fault"),
            "interrupt-after-write|interrupt-one|0",
        )
        .unwrap();
    }
    let original_args = interrupt_args(&first, "interrupt-one");
    let interrupted: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_interrupt", original_args.clone())).unwrap();
    wait_file(&first.brief.with_extension("json.interrupt-consumed"));
    assert_eq!(
        bytes(&f, &first),
        before + 1,
        "exactly one Escape without submit"
    );
    terminal_contains(&f, &first, "CHILD_NATIVE_ESCAPE_CONSUMED");
    let outcome = if unknown {
        DispatchOutcome::Unknown
    } else {
        DispatchOutcome::Applied
    };
    let receipt = interrupt_receipt(&interrupted, "interrupt-one").clone();
    assert_eq!(receipt.outcome, outcome);
    assert_eq!(receipt.input.as_ref().unwrap().written_bytes, 1);
    assert!(!receipt.input.as_ref().unwrap().submit_written);
    assert_eq!(
        interrupted.control.barrier,
        Some(ControlBarrier::Interrupt {
            request: "interrupt-one".into()
        })
    );
    let again: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_interrupt", original_args.clone())).unwrap();
    assert_eq!(*interrupt_receipt(&again, "interrupt-one"), receipt);
    assert_eq!(bytes(&f, &first), before + 1);
    refusal(
        &f,
        "ovrcr_confirm_interrupt",
        confirm_args(&inspect(&f, &first), "too-early", "interrupt-one"),
        "agent-end",
    );
    refusal(
        &f,
        "ovrcr_followup",
        serde_json::json!({"handle":first.handle,"expected_identity":first.identity,"expected_result_version":1,"expected_control_epoch":interrupted.control.epoch,"text":"Must remain fenced"}),
        "barrier",
    );
    assert!(
        !first
            .brief
            .with_extension("json.interrupt-settled")
            .exists()
    );
    fs::write(
        first.brief.with_extension("json.interrupt-settle"),
        "settle",
    )
    .unwrap();
    wait_file(&first.brief.with_extension("json.interrupt-settled"));
    let reconciled = inspect(&f, &first);
    refusal(
        &f,
        "ovrcr_confirm_interrupt",
        confirm_args(&reconciled, "wrong-interrupt", "another-interrupt"),
        "wrong-request",
    );
    let confirmed: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_confirm_interrupt",
        confirm_args(&reconciled, "confirm-one", "interrupt-one"),
    ))
    .unwrap();
    assert!(confirmed.control.barrier.is_none());
    assert_eq!(confirmed.control.epoch, interrupted.control.epoch + 1);
    assert_eq!(
        *interrupt_receipt(&confirmed, "interrupt-one"),
        receipt,
        "settlement never rewrites delivery"
    );
    assert_eq!(confirmed.execution_attempts, Some(1));
    let current = f.summary(first.identity.session.unwrap());
    assert_eq!(Some(current.run), first.identity.run);
    assert!(current.phase.is_live());
    fs::write(&f.gate, "late original result").unwrap();
    wait_file(&first.brief.with_extension("json.published"));
    let collected: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", serde_json::json!({"handle":first.handle})))
            .unwrap();
    assert!(collected.submission.is_some());
    assert_eq!(collected.acceptance, "pending");
    let followed: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_followup",serde_json::json!({"handle":first.handle,"expected_identity":first.identity,"expected_result_version":1,"expected_control_epoch":confirmed.control.epoch,"text":"Deliberate fresh same-child followup"}))).unwrap();
    assert_eq!(followed.identity.session, first.identity.session);
    assert_eq!(followed.identity.run, first.identity.run);
    assert_eq!(followed.result_version, 2);
    wait_file(&followed.brief.with_extension("json.started"));
    assert_eq!(*interrupt_receipt(&followed, "interrupt-one"), receipt);
    let before_replay = bytes(&f, &followed);
    let replay: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_interrupt", original_args)).unwrap();
    assert_eq!(*interrupt_receipt(&replay, "interrupt-one"), receipt);
    assert_eq!(
        bytes(&f, &followed),
        before_replay,
        "old interrupt never signals the next turn"
    );
}

fn takeover(f: &Fixture, child: &AssignmentSnapshot) -> UnixStream {
    let mut dashboard = f.dashboard();
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            2,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: child.identity.session.unwrap(),
                        run: child.identity.run.unwrap(),
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: child.identity.session
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            3,
            Request::AssignmentControl(AssignmentControlRequest::Takeover {
                request: "human-takeover".into(),
                session: child.identity.session.unwrap(),
                run: child.identity.run.unwrap(),
                expected_epoch: child.control.epoch,
                view_revision: 1
            })
        )
        .unwrap(),
        Response::Ok
    );
    dashboard
}

#[test]
fn production_cancel_freezes_membership_closes_dispatch_and_preserves_human_late_evidence() {
    let f = Owned::new();
    let active = f.start("NO_READY INTERRUPT_SETTLEMENT_GATE owned");
    let human = f.start("NO_READY protected Human");
    let full = f.start("NO_READY full control journal");
    supported(&f, &active);
    supported(&f, &human);
    let _dashboard = takeover(&f, &human);
    let human_control = inspect(&f, &human).control;
    let before_human = bytes(&f, &human);
    let before_active = bytes(&f, &active);
    let before_full = bytes(&f, &full);
    let refused_human: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_interrupt",
        interrupt_args(&human, "explicit-human-refusal"),
    ))
    .unwrap();
    assert_eq!(
        interrupt_receipt(&refused_human, "explicit-human-refusal").outcome,
        DispatchOutcome::Rejected
    );
    assert_eq!(refused_human.control, human_control);
    let mut stale = interrupt_args(&active, "stale-run-refusal");
    stale["expected_identity"]["run"] = serde_json::json!(active.identity.run.unwrap().0 + 1);
    let stale: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_interrupt", stale)).unwrap();
    assert_eq!(
        interrupt_receipt(&stale, "stale-run-refusal").outcome,
        DispatchOutcome::Rejected
    );
    assert_eq!(bytes(&f, &active), before_active);
    let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO delegations(handle, workflow, session, metadata) VALUES (?1,?2,NULL,?3)",
        rusqlite::params!["damaged-member", active.identity.workflow, "{malformed"],
    )
    .unwrap();
    let raw: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&full.handle],
            |row| row.get(0),
        )
        .unwrap();
    let mut full_record: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for index in 0..64 {
        let request = format!("retained-control-{index}");
        full_record["control_intents"].as_array_mut().unwrap().push(
            serde_json::json!({"request":request,"operation":"Takeover","digest":"a".repeat(64)}),
        );
        full_record["receipts"].as_array_mut().unwrap().push(serde_json::json!({"request":request,"operation":"Takeover","identity":full.identity,"result_version":1,"outcome":"Applied","input":null,"reason":null}));
    }
    let full_bytes = serde_json::to_string(&full_record).unwrap();
    db.execute(
        "UPDATE delegations SET metadata=?1 WHERE handle=?2",
        rusqlite::params![full_bytes, full.handle],
    )
    .unwrap();
    let mut invalid = full_record.clone();
    invalid["handle"] = serde_json::json!("invalid-version");
    invalid["identity"]["request"] = serde_json::json!("invalid-version");
    invalid["result_version"] = serde_json::json!(0);
    let invalid_bytes = serde_json::to_string(&invalid).unwrap();
    db.execute("INSERT INTO delegations(handle,workflow,session,metadata) VALUES ('invalid-version',?1,NULL,?2)",rusqlite::params![active.identity.workflow,invalid_bytes]).unwrap();
    invalid["handle"] = serde_json::json!("unpaired-run");
    invalid["identity"]["request"] = serde_json::json!("unpaired-run");
    invalid["result_version"] = serde_json::json!(1);
    invalid["identity"]["run"] = serde_json::Value::Null;
    let unpaired_bytes = serde_json::to_string(&invalid).unwrap();
    db.execute("INSERT INTO delegations(handle,workflow,session,metadata) VALUES ('unpaired-run',?1,NULL,?2)",rusqlite::params![active.identity.workflow,unpaired_bytes]).unwrap();
    let args = serde_json::json!({"workflow":active.identity.workflow,"request":"cancel-one"});
    let cancelled: WorkflowSnapshot =
        serde_json::from_value(f.tool("ovrcr_cancel_workflow", args.clone())).unwrap();
    let capture = cancelled.cancellation.clone().unwrap();
    assert_eq!(capture.dispatch_closure, DispatchOutcome::Applied);
    assert_eq!(
        capture.target_outcome,
        DispatchOutcome::Unknown,
        "malformed member cannot disappear from accounting"
    );
    assert!(!capture.gaps.is_empty());
    assert_eq!(capture.targets.len(), 3);
    let full_target = capture
        .targets
        .iter()
        .find(|target| target.handle == full.handle)
        .unwrap();
    assert_eq!(full_target.outcome, DispatchOutcome::Rejected);
    assert!(
        full_target
            .reason
            .as_deref()
            .unwrap()
            .contains("durably admitted")
    );
    assert_eq!(bytes(&f, &full), before_full);
    for (handle, expected) in [
        (full.handle.as_str(), full_bytes.as_str()),
        ("invalid-version", invalid_bytes.as_str()),
        ("unpaired-run", unpaired_bytes.as_str()),
    ] {
        let after: String = db
            .query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [handle],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            after, expected,
            "original protected journal/member is unchanged"
        );
    }
    assert_eq!(
        capture
            .targets
            .iter()
            .find(|t| t.handle == active.handle)
            .unwrap()
            .outcome,
        DispatchOutcome::Applied
    );
    assert_eq!(
        capture
            .targets
            .iter()
            .find(|t| t.handle == human.handle)
            .unwrap()
            .outcome,
        DispatchOutcome::Rejected
    );
    wait_file(&active.brief.with_extension("json.interrupt-consumed"));
    assert_eq!(bytes(&f, &active), before_active + 1);
    assert_eq!(bytes(&f, &human), before_human);
    assert_eq!(inspect(&f, &human).control, human_control);
    let again: WorkflowSnapshot =
        serde_json::from_value(f.tool("ovrcr_cancel_workflow", args)).unwrap();
    assert_eq!(again.cancellation, Some(capture.clone()));
    assert_eq!(bytes(&f, &active), before_active + 1);
    assert_eq!(bytes(&f, &human), before_human);
    refusal(
        &f,
        "ovrcr_cancel_workflow",
        serde_json::json!({"workflow":active.identity.workflow,"request":"different-cancel"}),
        "second-cancel",
    );
    let closed=f.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":f.workspace,"scope":"No dispatch after cancellation","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}));
    assert_eq!(closed["dispatch"], "Refused");
    let resumed = f.tool("ovrcr_start", serde_json::json!({"handle":active.handle}));
    assert_eq!(resumed["dispatch"], "Refused");
    fs::write(&f.gate, "late results survive closure").unwrap();
    wait_file(&active.brief.with_extension("json.published"));
    wait_file(&human.brief.with_extension("json.published"));
    for child in [&active, &human] {
        let collected: AssignmentSnapshot = serde_json::from_value(
            f.tool("ovrcr_collect", serde_json::json!({"handle":child.handle})),
        )
        .unwrap();
        assert!(collected.submission.is_some());
        assert_eq!(collected.acceptance, "pending");
        assert!(collected.resources.workspace_present);
        assert!(collected.brief.exists());
        let summary = f.summary(child.identity.session.unwrap());
        assert!(summary.phase.is_live());
        assert_eq!(Some(summary.run), child.identity.run);
    }
    assert_eq!(
        bytes(&f, &human),
        before_human,
        "Human child receives no cancellation bytes"
    );
}

#[test]
fn production_current_owner_confirms_original_interrupt_and_transfer_preserves_tombstone() {
    let f = Owned::new();
    let child = f.start("NO_READY INTERRUPT_SETTLEMENT_GATE transfer");
    supported(&f, &child);
    let interrupted: AssignmentSnapshot = serde_json::from_value(
        f.tool("ovrcr_interrupt", interrupt_args(&child, "before-transfer")),
    )
    .unwrap();
    wait_file(&child.brief.with_extension("json.interrupt-consumed"));
    fs::write(
        child.brief.with_extension("json.interrupt-settle"),
        "settle",
    )
    .unwrap();
    wait_file(&child.brief.with_extension("json.interrupt-settled"));
    let Response::CreatedSession(parent) =
        f.live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: f.workspace.clone(),
            name: "successor".into(),
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
        panic!("successor parent");
    };
    f.until(|| f.summary(parent.id).agent.is_some());
    for group in f.live.session_groups() {
        f.live.own_group(group);
    }
    let parents: Vec<WorkflowParent> =
        serde_json::from_value(f.tool("ovrcr_parents", serde_json::json!({}))).unwrap();
    let target = parents.iter().find(|p| p.session == parent.id).unwrap();
    let transferred:WorkflowSnapshot=serde_json::from_value(f.tool("ovrcr_transfer",serde_json::json!({"request":"transfer-to-successor","parent":target.session,"parent_run":target.run,"binding":target.binding,"producer":target.producer}))).unwrap();
    assert_eq!(transferred.authority.parent, parent.id);
    refusal(
        &f,
        "ovrcr_confirm_interrupt",
        confirm_args(&interrupted, "retired-confirm", "before-transfer"),
        "retired-owner",
    );
    let invoke = |name: &str, args: serde_json::Value, label: &str| {
        tool(
            &f.live,
            parent.id,
            name,
            args,
            &f.live.root.path().join(format!("successor-{label}.json")),
        )
    };
    invoke(
        "ovrcr_recover",
        serde_json::json!({"workflow":child.identity.workflow}),
        "recover",
    );
    let current: AssignmentSnapshot = serde_json::from_value(invoke(
        "ovrcr_inspect",
        serde_json::json!({"handle":child.handle}),
        "inspect",
    ))
    .unwrap();
    assert_eq!(current.control, interrupted.control);
    let confirmed: AssignmentSnapshot = serde_json::from_value(invoke(
        "ovrcr_confirm_interrupt",
        confirm_args(&current, "successor-confirm", "before-transfer"),
        "confirm",
    ))
    .unwrap();
    assert!(confirmed.control.barrier.is_none());
    assert_eq!(confirmed.identity, child.identity);
    assert_eq!(
        interrupt_receipt(&confirmed, "before-transfer"),
        interrupt_receipt(&interrupted, "before-transfer")
    );
    let cancel_args =
        serde_json::json!({"workflow":child.identity.workflow,"request":"cancel-after-transfer"});
    let cancelled: WorkflowSnapshot = serde_json::from_value(invoke(
        "ovrcr_cancel_workflow",
        cancel_args.clone(),
        "cancel",
    ))
    .unwrap();
    let capture = cancelled.cancellation.clone().unwrap();
    assert_eq!(capture.captured_authority.epoch, 2);
    assert_eq!(capture.targets[0].identity.controller_epoch, 1);
    let original = parents.iter().find(|p| p.session == f.parent).unwrap();
    invoke(
        "ovrcr_transfer",
        serde_json::json!({"request":"transfer-back-cancelled","parent":original.session,"parent_run":original.run,"binding":original.binding,"producer":original.producer}),
        "transfer-back",
    );
    f.tool(
        "ovrcr_recover",
        serde_json::json!({"workflow":child.identity.workflow}),
    );
    let repeated: WorkflowSnapshot =
        serde_json::from_value(f.tool("ovrcr_cancel_workflow", cancel_args)).unwrap();
    assert_eq!(repeated.authority.epoch, 3);
    assert_eq!(repeated.cancellation, Some(capture));
    let refused = f.tool("ovrcr_start", serde_json::json!({"handle":child.handle}));
    assert_eq!(refused["dispatch"], "Refused");
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn production_cancel_wins_queued_launch_and_blocks_original_accepted_launch_resume() {
    for accepted in [false, true] {
        let f = Owned::new();
        let pi = f.live.claude.command.parent().unwrap().join("pi");
        let count = f.live.root.path().join("probes");
        let entered = f.live.root.path().join("launch-probe-entered");
        let release = f.live.root.path().join("launch-probe-release");
        let script = format!(
            "#!/bin/sh\ncase \" $* \" in *\" --list-models \"*) n=0; [ ! -f '{0}' ] || n=$(cat '{0}'); n=$((n+1)); echo \"$n\" > '{0}'; if [ \"$n\" = 3 ]; then echo $$ > '{1}'; while [ ! -f '{2}' ]; do sleep 0.01; done; fi;; esac\n",
            count.display(),
            entered.display(),
            release.display()
        );
        let body = fs::read_to_string(&pi).unwrap();
        fs::write(
            &pi,
            format!("{script}\n{}", body.strip_prefix("#!/bin/sh\n").unwrap()),
        )
        .unwrap();
        let output = f.live.root.path().join("queued-start.json");
        f.send(&format!("tool:{}",serde_json::json!({"name":"ovrcr_start","args":{"project":PROJECT,"workspace":f.workspace,"scope":"queued launch","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]},"output":output,"background":true})));
        wait_file(&entered);
        let probe: i32 = fs::read_to_string(&entered)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        f.live.own_group(probe);
        let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
        let raw: String = db
            .query_row("SELECT metadata FROM delegations", [], |row| row.get(0))
            .unwrap();
        let record: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let handle = record["handle"].as_str().unwrap();
        let workflow = record["identity"]["workflow"].as_str().unwrap();
        if accepted {
            fs::write(
                f.live.root.path().join("fault"),
                format!("launch-after-intent|{handle}|0"),
            )
            .unwrap();
            fs::write(&release, "finish Accepted only").unwrap();
            wait_file(&output);
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
            assert_eq!(value["details"]["dispatch"], "Prepared");
            assert!(
                value["details"]["receipts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["operation"] == "Launch" && r["outcome"] == "Accepted")
            );
        }
        let cancelled: WorkflowSnapshot = serde_json::from_value(f.tool(
            "ovrcr_cancel_workflow",
            serde_json::json!({"workflow":workflow,"request":"close-queued-launch"}),
        ))
        .unwrap();
        let target = &cancelled.cancellation.unwrap().targets[0];
        assert_eq!(target.handle, handle);
        assert!(target.identity.session.is_none() && target.identity.run.is_none());
        if !accepted {
            fs::write(&release, "release queued discovery after closure").unwrap();
            wait_file(&output);
        }
        let resumed = f.tool("ovrcr_start", serde_json::json!({"handle":handle}));
        assert_eq!(resumed["dispatch"], "Refused");
        let final_raw: String = db
            .query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [handle],
                |row| row.get(0),
            )
            .unwrap();
        let final_record: serde_json::Value = serde_json::from_str(&final_raw).unwrap();
        assert!(final_record["identity"]["session"].is_null());
        assert!(final_record["identity"]["run"].is_null());
        assert!(
            f.live.session_groups().len() <= 2,
            "cancellation never launches an unaccounted child"
        );
        if accepted {
            assert!(
                final_record["receipts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["operation"] == "Launch" && r["outcome"] == "Accepted")
            );
        }
    }
}
