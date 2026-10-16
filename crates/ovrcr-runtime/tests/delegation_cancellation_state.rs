//! Pure cancellation component/evidence tests; no WorkflowRecord, transfer,
//! production admission, Server, PTY or native provider is exercised here.
#[path = "../src/server/delegation_cancellation.rs"]
mod cancellation;
#[path = "../src/server/delegation_interrupt.rs"]
mod interrupt;
use cancellation::*;
use interrupt::InterruptBaseline;
use ovrcr_protocol::*;
use serde::{Deserialize, Serialize};
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn identity() -> AssignmentIdentity {
    AssignmentIdentity {
        instance: "instance".into(),
        workflow: "workflow".into(),
        controller: "controller".into(),
        controller_epoch: 7,
        task: "task".into(),
        attempt: 1,
        contract: 1,
        request: "assignment".into(),
        session: Some(SessionId(2)),
        run: Some(SessionRunId(3)),
    }
}
fn agent() -> AgentSnapshot {
    AgentSnapshot {
        binding: AgentBinding {
            provider: AgentProvider::Pi,
            invocation: "child".into(),
            conversation: "conversation".into(),
            generation: 1,
        },
        activity: Some(ActivitySample {
            state: AgentActivity::Busy,
            quality: SampleQuality::Observed,
            turn: Some("turn-1".into()),
        }),
        metrics: None,
        health: HealthSample {
            state: ReporterHealth::Connected,
            reason: None,
        },
        activity_revision: 10,
        metrics_revision: 0,
        health_revision: 1,
        input_requests: vec![],
        input_revision: 1,
        native_control: Some(NativeControlSample {
            turn: "turn-1".into(),
            interrupt: NativeInterruptCapability::Escape,
            settled: false,
        }),
        native_control_revision: 11,
    }
}
// Test data only. Integration implements CancellationAuthority for the actual
// sole workflow authority after #326 is accepted; none of its code is imported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestAuthority {
    instance: String,
    workflow: String,
    controller: String,
    epoch: u64,
    parent: SessionId,
    parent_run: SessionRunId,
    binding: AgentBinding,
    producer: String,
    server_lifetime: String,
}
impl CancellationAuthority for TestAuthority {
    fn validate_capture(&self) -> anyhow::Result<()> {
        if self.epoch == 0 || self.parent.0 == 0 || self.parent_run.0 == 0 {
            anyhow::bail!("invalid captured authority");
        }
        for value in [
            &self.instance,
            &self.workflow,
            &self.controller,
            &self.producer,
            &self.server_lifetime,
        ] {
            validate_agent_id(value)?;
        }
        self.binding.validate()
    }
    fn instance(&self) -> &str {
        &self.instance
    }
    fn workflow(&self) -> &str {
        &self.workflow
    }
}
fn authority() -> TestAuthority {
    TestAuthority {
        instance: "instance".into(),
        workflow: "workflow".into(),
        controller: "controller".into(),
        epoch: 7,
        parent: SessionId(1),
        parent_run: SessionRunId(1),
        binding: AgentBinding {
            invocation: "parent".into(),
            ..agent().binding
        },
        producer: "source-1".into(),
        server_lifetime: "server-1".into(),
    }
}
fn request(a: &TestAuthority) -> CancellationRequest<'_, TestAuthority> {
    CancellationRequest {
        request: "cancel-1",
        digest: DIGEST,
        expected_authority: a,
    }
}
fn targets() -> Vec<CancellationTarget> {
    vec![CancellationTarget {
        handle: "assignment".into(),
        identity: identity(),
        result_version: 1,
        control_epoch: 2,
        interrupt_request: "cancel-1-target-1".into(),
        refusal: None,
    }]
}
fn capture() -> WorkflowCancellation<TestAuthority> {
    let mut slot = None;
    let a = authority();
    admit_cancellation(&mut slot, &a, request(&a), &[], || Ok(targets())).unwrap();
    slot.unwrap()
}
fn receipt(target: &CancellationTarget, outcome: DispatchOutcome) -> DispatchReceipt {
    // Followup is an operation-neutral stand-in on this accepted base, which has
    // no Interrupt variant. Production integration must supply Interrupt exactly.
    DispatchReceipt {
        request: target.interrupt_request.clone(),
        operation: DispatchOperation::Followup,
        result_version: target.result_version,
        identity: target.identity.clone(),
        outcome,
        input: None,
        reason: None,
    }
}
#[test]
fn interruption_requires_current_busy_turn_without_open_native_input() {
    let original = agent();
    assert!(InterruptBaseline::capture("server", &identity(), 1, &original).is_ok());
    for state in [
        AgentActivity::Idle,
        AgentActivity::Error,
        AgentActivity::ResponseReady,
        AgentActivity::Unknown,
        AgentActivity::WaitingInput,
    ] {
        let mut current = original.clone();
        current.activity.as_mut().unwrap().state = state;
        assert!(InterruptBaseline::capture("server", &identity(), 1, &current).is_err());
    }
    let mut mutations = vec![];
    let mut current = original.clone();
    current.input_requests.push(InputRequest {
        id: "prompt:1".into(),
        kind: InputKind::Select,
    });
    mutations.push(current);
    let mut current = original.clone();
    current.health.state = ReporterHealth::Unavailable;
    mutations.push(current);
    let mut current = original.clone();
    current.activity.as_mut().unwrap().turn = None;
    mutations.push(current);
    let mut current = original.clone();
    current.activity.as_mut().unwrap().quality = SampleQuality::Estimated;
    mutations.push(current);
    let mut current = original.clone();
    current.activity_revision = 0;
    mutations.push(current);
    let mut current = original;
    current.binding.provider = AgentProvider::Omp;
    mutations.push(current);
    for current in mutations {
        assert!(InterruptBaseline::capture("server", &identity(), 1, &current).is_err());
    }
    let mut unallocated = identity();
    unallocated.run = None;
    assert!(InterruptBaseline::capture("server", &unallocated, 1, &agent()).is_err());
}

#[test]
fn delivery_or_agent_end_cannot_confirm_settlement() {
    let busy = agent();
    let baseline = InterruptBaseline::capture("server", &identity(), 1, &busy).unwrap();
    assert!(
        baseline
            .confirm_settlement("server", &identity(), 1, &busy)
            .is_err()
    );
    let mut busy_new_revision = busy.clone();
    busy_new_revision.activity_revision += 1;
    assert!(
        baseline
            .confirm_settlement("server", &identity(), 1, &busy_new_revision)
            .is_err()
    );
    for state in [
        AgentActivity::Idle,
        AgentActivity::Error,
        AgentActivity::ResponseReady,
    ] {
        let mut settled = busy.clone();
        settled.activity.as_mut().unwrap().state = state;
        assert!(
            baseline
                .confirm_settlement("server", &identity(), 1, &settled)
                .is_err(),
            "old revision"
        );
        settled.activity_revision += 1;
        settled.native_control.as_mut().unwrap().settled = true;
        settled.native_control_revision += 1;
        baseline
            .confirm_settlement("server", &identity(), 1, &settled)
            .unwrap();
    }
}

#[test]
fn settlement_cannot_cross_turn_binding_run_version_or_server_replacement() {
    let baseline = InterruptBaseline::capture("server", &identity(), 1, &agent()).unwrap();
    let mut settled = agent();
    settled.activity_revision += 1;
    settled.native_control.as_mut().unwrap().settled = true;
    settled.native_control_revision += 1;
    settled.activity.as_mut().unwrap().state = AgentActivity::Idle;
    let mut mutations = vec![];
    let mut current = settled.clone();
    current.activity.as_mut().unwrap().turn = Some("turn-2".into());
    mutations.push(current);
    let mut current = settled.clone();
    current.binding.conversation = "replacement".into();
    mutations.push(current);
    let mut current = settled.clone();
    current.binding.generation += 1;
    mutations.push(current);
    let mut current = settled.clone();
    current.binding.invocation = "replacement".into();
    mutations.push(current);
    let mut current = settled.clone();
    current.health.state = ReporterHealth::Unavailable;
    mutations.push(current);
    let mut current = settled.clone();
    current.activity.as_mut().unwrap().quality = SampleQuality::Estimated;
    mutations.push(current);
    let mut current = settled.clone();
    current.input_requests.push(InputRequest {
        id: "prompt:2".into(),
        kind: InputKind::Confirm,
    });
    mutations.push(current);
    for current in mutations {
        assert!(
            baseline
                .confirm_settlement("server", &identity(), 1, &current)
                .is_err()
        );
    }
    let mut replaced = identity();
    replaced.run = Some(SessionRunId(4));
    assert!(
        baseline
            .confirm_settlement("server", &replaced, 1, &settled)
            .is_err()
    );
    assert!(
        baseline
            .confirm_settlement("server-2", &identity(), 1, &settled)
            .is_err()
    );
    assert!(
        baseline
            .confirm_settlement("server", &identity(), 2, &settled)
            .is_err()
    );
    let restored: InterruptBaseline =
        serde_json::from_slice(&serde_json::to_vec(&baseline).unwrap()).unwrap();
    assert_eq!(restored, baseline);
}

#[test]
fn parent_transfer_preserves_original_capture_receipt_and_targets() {
    let original = capture();
    let before = serde_json::to_vec(&original).unwrap();
    let mut slot = Some(original.clone());
    let mut current = authority();
    current.epoch += 1;
    current.parent = SessionId(9);
    current.parent_run = SessionRunId(9);
    current.binding.invocation = "replacement-parent".into();
    current.binding.generation += 1;
    current.producer = "source-2".into();
    current.server_lifetime = "server-2".into();
    let replay = admit_cancellation(&mut slot, &current, request(&current), &[], || {
        panic!("must not rediscover after transfer")
    })
    .unwrap();
    assert!(matches!(replay, CancellationAdmission::Replay(_)));
    assert_eq!(replay.capture(), &original);
    assert_eq!(serde_json::to_vec(replay.capture()).unwrap(), before);
    replay.capture().validate().unwrap();
    assert!(
        !replay
            .target_pending(
                &current,
                0,
                &receipt(&original.targets[0], DispatchOutcome::Accepted),
                DispatchOperation::Followup
            )
            .unwrap()
    );
    assert!(require_dispatch_open(slot.as_ref()).is_err());
    let old = authority();
    assert!(
        admit_cancellation(&mut slot, &current, request(&old), &[], || panic!(
            "stale owner cannot discover"
        ))
        .is_err()
    );
    assert_eq!(slot.as_ref(), Some(&original));
}

#[test]
fn stable_membership_includes_old_controller_epochs_and_prepared_unallocated_targets() {
    let mut current = authority();
    current.epoch = 10;
    let mut fixed = targets();
    fixed[0].identity.controller_epoch = 1;
    fixed[0].identity.controller = "original-controller".into();
    let mut prepared = fixed[0].clone();
    prepared.handle = "prepared".into();
    prepared.identity.request = prepared.handle.clone();
    prepared.identity.task = "prepared-task".into();
    prepared.identity.session = None;
    prepared.identity.run = None;
    prepared.identity.attempt = 2;
    prepared.interrupt_request = "cancel-prepared".into();
    fixed.push(prepared);
    let mut slot = None;
    let installed = admit_cancellation(&mut slot, &current, request(&current), &[], || {
        Ok(fixed.clone())
    })
    .unwrap();
    assert!(matches!(installed, CancellationAdmission::Installed(_)));
    assert_eq!(installed.capture().targets, fixed);
    let mut later_allocation = receipt(&fixed[1], DispatchOutcome::Accepted);
    later_allocation.identity.session = Some(SessionId(90));
    later_allocation.identity.run = Some(SessionRunId(91));
    assert!(
        installed
            .target_pending(&current, 1, &later_allocation, DispatchOperation::Followup)
            .is_err()
    );
    let frozen = installed.capture().clone();
    let replay = admit_cancellation(&mut slot, &current, request(&current), &[], || {
        panic!("later allocation cannot replace fixed targets")
    })
    .unwrap();
    assert_eq!(replay.capture(), &frozen);
    assert_eq!(replay.capture().targets[1].identity.session, None);
    assert_eq!(replay.capture().targets[1].identity.run, None);
}

#[test]
fn tombstone_blocks_new_and_resumed_accepted_dispatch_after_decode() {
    let slot: Option<WorkflowCancellation<TestAuthority>> = None;
    require_dispatch_open(slot.as_ref()).unwrap();
    let original = capture();
    let raw = serde_json::to_vec(&original).unwrap();
    let restored: WorkflowCancellation<TestAuthority> = serde_json::from_slice(&raw).unwrap();
    assert_eq!(restored, original);
    // This checks the shared guard only. Its production callers still need wiring
    // on both first and resumed Accepted paths for every dispatch operation.
    for operation in [
        DispatchOperation::Prepare,
        DispatchOperation::Launch,
        DispatchOperation::InitialInput,
        DispatchOperation::Followup,
    ] {
        let pending = DispatchReceipt {
            operation,
            ..receipt(&restored.targets[0], DispatchOutcome::Accepted)
        };
        assert_eq!(pending.outcome, DispatchOutcome::Accepted);
        assert!(
            require_dispatch_open(Some(&restored)).is_err(),
            "{operation:?}"
        );
    }
}

#[test]
fn duplicate_and_colliding_cancellation_requests_never_rediscover_or_overwrite() {
    let a = authority();
    let mut slot = None;
    assert!(
        admit_cancellation(&mut slot, &a, request(&a), &["cancel-1"], || panic!(
            "cross-operation collision must precede discovery"
        ))
        .is_err()
    );
    assert!(slot.is_none());
    let mut discoveries = 0;
    let installed = admit_cancellation(&mut slot, &a, request(&a), &["transfer-1"], || {
        discoveries += 1;
        Ok(targets())
    })
    .unwrap();
    let original = installed.capture().clone();
    assert_eq!(discoveries, 1);
    let replay = admit_cancellation(&mut slot, &a, request(&a), &[], || {
        panic!("duplicate request must not enumerate")
    })
    .unwrap();
    assert!(matches!(replay, CancellationAdmission::Replay(_)));
    assert_eq!(replay.capture(), &original);
    for (id, digest) in [
        (
            "cancel-1",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
        ("cancel-2", DIGEST),
        ("../invalid", DIGEST),
        ("cancel-1", "short"),
    ] {
        assert!(
            admit_cancellation(
                &mut slot,
                &a,
                CancellationRequest {
                    request: id,
                    digest,
                    expected_authority: &a
                },
                &[],
                || panic!("conflicting request cannot enumerate")
            )
            .is_err()
        );
        assert_eq!(slot.as_ref(), Some(&original));
    }
}

#[test]
fn only_first_admission_and_exact_pending_target_receipt_can_begin_delivery() {
    let a = authority();
    let mut slot = None;
    let installed = admit_cancellation(&mut slot, &a, request(&a), &[], || Ok(targets())).unwrap();
    let target = &installed.capture().targets[0];
    assert!(
        installed
            .target_pending(
                &a,
                0,
                &receipt(target, DispatchOutcome::Accepted),
                DispatchOperation::Followup
            )
            .unwrap()
    );
    for outcome in [
        DispatchOutcome::Applied,
        DispatchOutcome::Rejected,
        DispatchOutcome::Partial,
        DispatchOutcome::Unknown,
    ] {
        assert!(
            !installed
                .target_pending(
                    &a,
                    0,
                    &receipt(target, outcome),
                    DispatchOperation::Followup
                )
                .unwrap(),
            "{outcome:?} must never authorize another write"
        );
    }
    let mut transferred = a.clone();
    transferred.epoch += 1;
    assert!(
        installed
            .target_pending(
                &transferred,
                0,
                &receipt(target, DispatchOutcome::Accepted),
                DispatchOperation::Followup
            )
            .is_err()
    );
    for field in [
        "request",
        "operation",
        "result_version",
        "run",
        "attempt",
        "contract",
    ] {
        let mut changed = receipt(target, DispatchOutcome::Accepted);
        match field {
            "request" => changed.request = "replacement".into(),
            "operation" => changed.operation = DispatchOperation::InitialInput,
            "result_version" => changed.result_version += 1,
            "run" => changed.identity.run = Some(SessionRunId(100)),
            "attempt" => changed.identity.attempt += 1,
            _ => changed.identity.contract += 1,
        }
        assert!(
            installed
                .target_pending(&a, 0, &changed, DispatchOperation::Followup)
                .is_err(),
            "{field}"
        );
    }
    assert!(
        installed
            .target_pending(
                &a,
                1,
                &receipt(target, DispatchOutcome::Accepted),
                DispatchOperation::Followup
            )
            .is_err()
    );
}

#[test]
fn lost_reply_and_unknown_or_partial_target_outcomes_never_replay() {
    let a = authority();
    let original = capture();
    let mut slot = Some(original.clone());
    let replay = admit_cancellation(&mut slot, &a, request(&a), &[], || {
        panic!("lost reply cannot enumerate")
    })
    .unwrap();
    for outcome in [
        DispatchOutcome::Accepted,
        DispatchOutcome::Applied,
        DispatchOutcome::Rejected,
        DispatchOutcome::Partial,
        DispatchOutcome::Unknown,
    ] {
        assert!(
            !replay
                .target_pending(
                    &a,
                    0,
                    &receipt(&original.targets[0], outcome),
                    DispatchOperation::Followup
                )
                .unwrap(),
            "replay {outcome:?}"
        );
    }
    assert_eq!(replay.capture().receipt, original.receipt);
    assert_eq!(replay.capture().targets, original.targets);
}

#[test]
fn decoded_provenance_and_membership_validation_fail_closed() {
    let original = capture();
    let mut invalid = original.clone();
    invalid.captured_authority.epoch = 0;
    assert!(invalid.validate().is_err());
    let mut invalid = original.clone();
    invalid.captured_authority.binding.generation = 0;
    assert!(invalid.validate().is_err());
    let mut invalid = original.clone();
    invalid.receipt.dispatch_closed = false;
    assert!(invalid.validate().is_err());
    for field in [
        "instance",
        "workflow",
        "handle",
        "duplicate_handle",
        "duplicate_request",
        "half_allocation",
        "control_epoch",
        "result_version",
        "attempt",
        "cancel_request",
    ] {
        let mut invalid = original.clone();
        match field {
            "instance" => invalid.targets[0].identity.instance = "foreign".into(),
            "workflow" => invalid.targets[0].identity.workflow = "foreign".into(),
            "handle" => invalid.targets[0].handle = "alias".into(),
            "duplicate_handle" => invalid.targets.push(invalid.targets[0].clone()),
            "duplicate_request" => {
                let mut second = invalid.targets[0].clone();
                second.handle = "second".into();
                second.identity.request = second.handle.clone();
                invalid.targets.push(second);
            }
            "half_allocation" => invalid.targets[0].identity.run = None,
            "control_epoch" => invalid.targets[0].control_epoch = 0,
            "result_version" => invalid.targets[0].result_version = 0,
            "attempt" => invalid.targets[0].identity.attempt = 0,
            _ => invalid.targets[0].interrupt_request = invalid.receipt.request.clone(),
        }
        assert!(invalid.validate().is_err(), "{field}");
        assert!(
            require_dispatch_open(Some(&invalid)).is_err(),
            "malformed tombstones cannot reopen dispatch"
        );
    }
    let mut raw = serde_json::to_value(&original).unwrap();
    raw["new_unsupported_field"] = true.into();
    assert!(serde_json::from_value::<WorkflowCancellation<TestAuthority>>(raw).is_err());
    let mut oversized = original;
    oversized.targets = vec![oversized.targets[0].clone(); 4097];
    assert!(oversized.validate().is_err());
}

#[test]
fn current_authority_fence_is_distinct_from_historical_capture_validation() {
    let original = authority();
    for field in [
        "instance",
        "workflow",
        "controller",
        "epoch",
        "parent",
        "parent_run",
        "binding",
        "producer",
        "server_lifetime",
    ] {
        let mut changed = original.clone();
        match field {
            "instance" => changed.instance = "foreign".into(),
            "workflow" => changed.workflow = "foreign".into(),
            "controller" => changed.controller = "new-controller".into(),
            "epoch" => changed.epoch += 1,
            "parent" => changed.parent = SessionId(5),
            "parent_run" => changed.parent_run = SessionRunId(6),
            "binding" => changed.binding.generation += 1,
            "producer" => changed.producer = "new-source".into(),
            _ => changed.server_lifetime = "new-server".into(),
        }
        let mut slot = None;
        assert!(
            admit_cancellation(&mut slot, &changed, request(&original), &[], || panic!(
                "stale authority cannot enumerate"
            ))
            .is_err(),
            "{field}"
        );
        assert!(slot.is_none());
    }
    let frozen = capture();
    frozen.validate().unwrap();
    let mut returned = original.clone();
    returned.epoch += 2;
    let mut slot = Some(frozen.clone());
    let replay = admit_cancellation(&mut slot, &returned, request(&returned), &[], || {
        panic!("ABA cannot rediscover")
    })
    .unwrap();
    assert_eq!(replay.capture(), &frozen);
    replay.capture().validate().unwrap();
}

#[test]
fn aggregate_separates_delivery_refusal_and_unknown_effects() {
    use DispatchOutcome::*;
    assert_eq!(aggregate_cancellation([]), Applied);
    assert_eq!(aggregate_cancellation([Applied, Applied]), Applied);
    assert_eq!(aggregate_cancellation([Rejected, Rejected]), Rejected);
    assert_eq!(aggregate_cancellation([Applied, Rejected]), Partial);
    assert_eq!(aggregate_cancellation([Rejected, Applied]), Partial);
    for unresolved in [Accepted, Partial, Unknown] {
        for known in [Applied, Rejected] {
            assert_eq!(aggregate_cancellation([unresolved, known]), Unknown);
            assert_eq!(aggregate_cancellation([known, unresolved]), Unknown);
        }
    }
}
