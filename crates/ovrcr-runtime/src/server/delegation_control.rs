//! Per-assignment authority. Callers persist each transition while holding the
//! retained store through the Session state/writer fence.
use anyhow::{Result, bail};
use ovrcr_protocol::{AssignmentControl, AssignmentControlEvidence, ControlBarrier, ControlOwner};

fn next_epoch(control: &AssignmentControl, expected: u64) -> Result<u64> {
    if control.epoch != expected {
        bail!("assignment control epoch changed");
    }
    control
        .epoch
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("assignment control epoch exhausted"))
}

pub(super) fn require_controller(control: &AssignmentControl, expected: u64) -> Result<()> {
    if control.epoch != expected
        || control.owner != ControlOwner::Controller
        || control.barrier.is_some()
    {
        return Err(lifecycle_error(
            ErrorCode::Conflict,
            "assignment controller input is fenced; reconcile current control ownership",
        ));
    }
    Ok(())
}

pub(super) fn take_control(control: &mut AssignmentControl, expected: u64) -> Result<()> {
    let epoch = next_epoch(control, expected)?;
    if control.owner != ControlOwner::Controller || control.barrier.is_some() {
        bail!("assignment is not available for human takeover");
    }
    control.epoch = epoch;
    control.owner = ControlOwner::Human;
    Ok(())
}

pub(super) fn note_human_input(control: &mut AssignmentControl, expected: u64) -> Result<()> {
    if control.epoch != expected
        || control.owner != ControlOwner::Human
        || control.barrier.is_some()
    {
        bail!("human input requires current explicit takeover");
    }
    control.human_input_revision = control
        .human_input_revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("human input revision exhausted"))?;
    control.attribution_invalidated = true;
    Ok(())
}

pub(super) fn return_control(
    control: &mut AssignmentControl,
    expected: u64,
    request: &str,
    takeover: &AssignmentControlEvidence,
    reconciled: &AssignmentControlEvidence,
    current: &AssignmentControlEvidence,
) -> Result<()> {
    let epoch = next_epoch(control, expected)?;
    if control.owner != ControlOwner::Human || control.barrier.is_some() {
        bail!("only the current human owner can return this assignment");
    }
    if reconciled != current || current.human_input_revision != control.human_input_revision {
        bail!("return reconciliation changed; inspect the assignment again");
    }
    // A binding change cannot be acknowledged as an attributable human edit.
    if takeover.identity != current.identity
        || takeover.binding != current.binding
        || takeover.server_lifetime != current.server_lifetime
    {
        bail!("assignment identity or reporting binding changed");
    }
    let changed = takeover.human_input_revision != current.human_input_revision
        || takeover.input_revision != current.input_revision
        || takeover.result_version != current.result_version
        || takeover.publication != current.publication
        || takeover.publication_issue != current.publication_issue;
    control.epoch = epoch;
    control.owner = ControlOwner::Controller;
    control.barrier = Some(ControlBarrier::HumanReturn {
        request: request.to_owned(),
    });
    control.attribution_invalidated |= changed;
    Ok(())
}

pub(super) fn confirm_return(
    control: &mut AssignmentControl,
    expected: u64,
    returned_request: &str,
    reconciled: &AssignmentControlEvidence,
    current: &AssignmentControlEvidence,
) -> Result<()> {
    let epoch = next_epoch(control, expected)?;
    if control.owner != ControlOwner::Controller
        || !matches!(&control.barrier, Some(ControlBarrier::HumanReturn { request }) if request == returned_request)
    {
        bail!("confirmation does not own the current human-return barrier");
    }
    if reconciled != current || current.human_input_revision != control.human_input_revision {
        bail!("return confirmation changed; reconcile the assignment again");
    }
    control.epoch = epoch;
    control.barrier = None;
    Ok(())
}

pub(super) fn suspend(control: &mut AssignmentControl) -> Result<()> {
    let epoch = next_epoch(control, control.epoch)?;
    control.epoch = epoch;
    control.owner = ControlOwner::Uncertain;
    control.barrier = Some(ControlBarrier::AttributionChanged);
    control.attribution_invalidated = true;
    Ok(())
}

use super::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ControlIntent {
    pub(super) request: String,
    pub(super) operation: DispatchOperation,
    pub(super) digest: String,
}

impl Record {
    pub(super) fn control_replay(
        &self,
        request: &str,
        operation: DispatchOperation,
        payload: &impl Serialize,
    ) -> Result<bool> {
        if request.is_empty()
            || request.len() > 128
            || !request
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            bail!(
                "control request identity must contain 1..128 letters, digits, hyphens or underscores"
            );
        }
        let expected = digest(&serde_json::to_vec(&(operation, payload))?);
        if let Some(intent) = self
            .control_intents
            .iter()
            .find(|intent| intent.request == request)
        {
            if intent.operation != operation || intent.digest != expected {
                bail!("control request already belongs to another intent");
            }
            return Ok(true);
        }
        if self
            .receipts
            .iter()
            .any(|receipt| receipt.request == request)
        {
            bail!("request already identifies another operation");
        }
        if self.control_intents.len() >= 64 {
            bail!(
                "assignment control receipt storage is full (64 operations); existing receipts are retained"
            );
        }
        Ok(false)
    }

    pub(super) fn record_control(
        &mut self,
        request: &str,
        operation: DispatchOperation,
        payload: &impl Serialize,
    ) -> Result<()> {
        self.control_intents.push(ControlIntent {
            request: request.to_owned(),
            operation,
            digest: digest(&serde_json::to_vec(&(operation, payload))?),
        });
        self.receipts.push(DispatchReceipt {
            request: request.to_owned(),
            operation,
            identity: self.identity.clone(),
            result_version: self.result_version,
            outcome: DispatchOutcome::Applied,
            input: None,
            reason: None,
        });
        Ok(())
    }
}

fn hold_before_human_view(handle: &str) -> Result<()> {
    #[cfg(feature = "acceptance-diagnostics")]
    if take_fault("human-before-view", handle).is_some() {
        let release = PathBuf::from(
            std::env::var_os("OVRCR_TEST_DELEGATION_FAULT").context("owned fault path")?,
        )
        .with_extension("release");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !release.exists() {
            if Instant::now() >= deadline {
                bail!("owned human input barrier was not released");
            }
            std::thread::park_timeout(Duration::from_millis(5));
        }
        std::fs::remove_file(release)?;
    }
    #[cfg(not(feature = "acceptance-diagnostics"))]
    let _ = handle;
    Ok(())
}

impl ServerState {
    pub(super) fn control_evidence(
        &self,
        record: &Record,
        agent: &AgentSnapshot,
    ) -> Result<AssignmentControlEvidence> {
        if record.binding.as_ref() != Some(&agent.binding)
            || agent.health.state != ReporterHealth::Connected
        {
            bail!("assignment reporting binding is unavailable or changed");
        }
        let (publication, mut publication_issue) = match self.assignment_publication(record) {
            Ok(publication) => (publication, None),
            Err(PublicationIssue::Unavailable) => (None, Some(ControlPublicationIssue::Missing)),
            Err(PublicationIssue::Invalid) => (None, Some(ControlPublicationIssue::Invalid)),
            Err(PublicationIssue::Uncommitted) => {
                (None, Some(ControlPublicationIssue::Unattributed))
            }
        };
        if publication_issue.is_none() && record.control.attribution_invalidated {
            publication_issue = Some(ControlPublicationIssue::Unattributed);
        }
        Ok(AssignmentControlEvidence {
            server_lifetime: self.server_lifetime.clone(),
            identity: record.identity.clone(),
            binding: agent.binding.clone(),
            result_version: record.result_version,
            activity_revision: agent.activity_revision,
            input_revision: agent.input_revision,
            publication,
            publication_issue,
            human_input_revision: record.control.human_input_revision,
        })
    }

    pub(crate) fn project_assignment_control(&self, summary: &mut SessionSummary) {
        let record = self.retained.lock().assigned_record(summary.id);
        match record {
            Ok(Some(record)) => {
                let evidence = if record.identity.run == Some(summary.run) {
                    summary
                        .agent
                        .as_ref()
                        .and_then(|agent| self.control_evidence(&record, agent).ok())
                } else {
                    None
                };
                summary.assignment_control = Some(AssignmentControlView {
                    handle: record.handle,
                    control: record.control,
                    evidence,
                });
            }
            Ok(None) => {}
            Err(error) => eprintln!("assignment control projection unavailable: {error:#}"),
        }
    }

    /// Called at the existing reporting publication boundary, never by Reconcile.
    pub(crate) fn refresh_assignment_binding(&self, id: SessionId) -> Result<()> {
        let session = self.session_for_control(id)?;
        let store = self.retained.lock();
        let Some(mut record) = store.assigned_record(id)? else {
            return Ok(());
        };
        let Some(binding) = record.binding.clone() else {
            return Ok(());
        };
        if record.control.owner != ControlOwner::Uncertain
            && session
                .with_assignment_attribution(
                    record.identity.run.context("assigned run missing")?,
                    &binding,
                    |_| Ok(()),
                )
                .is_err()
        {
            suspend(&mut record.control)?;
            store.save_assignment(&record)?;
        }
        Ok(())
    }

    pub(crate) fn assignment_control(
        &self,
        owner: &Arc<()>,
        transition: AssignmentControlRequest,
    ) -> Result<()> {
        let (request, id, run, epoch, revision, operation) = match &transition {
            AssignmentControlRequest::Takeover {
                request,
                session,
                run,
                expected_epoch,
                view_revision,
            } => (
                request,
                *session,
                *run,
                *expected_epoch,
                *view_revision,
                DispatchOperation::Takeover,
            ),
            AssignmentControlRequest::Return {
                request,
                session,
                run,
                expected_epoch,
                view_revision,
                ..
            } => (
                request,
                *session,
                *run,
                *expected_epoch,
                *view_revision,
                DispatchOperation::HumanReturn,
            ),
        };
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        let store = self.retained.lock();
        let mut record = store
            .assigned_record(id)?
            .context("selected session is not assigned")?;
        if record.identity.run != Some(run) || session.run() != run {
            bail!("assigned run changed");
        }
        if record.control_replay(request, operation, &transition)? {
            return Ok(());
        }
        let binding = record
            .binding
            .clone()
            .context("assignment has no current reporting binding")?;
        let result = session.with_assignment_context(run, &binding, |agent| {
            self.dashboard
                .with_focused_input(owner, id, run, revision, || {
                    let evidence = self.control_evidence(&record, agent)?;
                    match &transition {
                        AssignmentControlRequest::Takeover { .. } => {
                            take_control(&mut record.control, epoch)?;
                            record.takeover_evidence = Some(evidence);
                        }
                        AssignmentControlRequest::Return {
                            evidence: observed, ..
                        } => {
                            let baseline = record
                                .takeover_evidence
                                .as_ref()
                                .context("takeover evidence is unavailable")?;
                            return_control(
                                &mut record.control,
                                epoch,
                                request,
                                baseline,
                                observed,
                                &evidence,
                            )?;
                        }
                    }
                    record.record_control(request, operation, &transition)?;
                    if take_fault("control-before-save", request).is_some() {
                        bail!("owned control persistence fault before save");
                    }
                    store.save_assignment(&record)
                })
                .context(
                    "control transition requires the owning Dashboard's acknowledged focused view",
                )?
        });
        drop(store);
        result?;
        super::super::dispatch::publish_session_changed(self, id);
        Ok(())
    }

    pub(crate) fn assigned_human_input(
        &self,
        owner: &Arc<()>,
        id: SessionId,
        run: SessionRunId,
        epoch: u64,
        revision: u64,
        bytes: &[u8],
    ) -> Result<()> {
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        let store = self.retained.lock();
        let mut record = store
            .assigned_record(id)?
            .context("human input requires an assigned child")?;
        if record.identity.run != Some(run) {
            bail!("assigned run changed");
        }
        if record.control.epoch != epoch
            || record.control.owner != ControlOwner::Human
            || record.control.barrier.is_some()
        {
            bail!("human input requires current explicit takeover");
        }
        let binding = record
            .binding
            .clone()
            .context("assignment has no current reporting binding")?;
        // Writer is acquired before the slot; a waiting write cannot hold old view ownership.
        let result = session.with_assignment_writer(run, &binding, |writer| {
            hold_before_human_view(&record.handle)?;
            self.dashboard
                .with_focused_input(owner, id, run, revision, || {
                    if bytes.is_empty() {
                        return Ok(());
                    }
                    note_human_input(&mut record.control, epoch)?;
                    if take_fault("human-before-save", &record.handle).is_some() {
                        bail!("owned human input persistence fault before save");
                    }
                    store.save_assignment(&record)?;
                    writer.write_all(bytes)?;
                    writer.flush()?;
                    Ok(())
                })
                .context("human input requires the owning Dashboard's acknowledged focused view")?
        });
        drop(store);
        // Do not build hierarchy or write a socket while any input guard is held.
        if result.is_ok() {
            super::super::dispatch::publish_session_changed(self, id);
        }
        result
    }

    pub(super) fn confirm_human_return(
        &self,
        auth: DelegationAuth,
        handle: String,
        request: String,
        returned_request: String,
        epoch: u64,
        observed: AssignmentControlEvidence,
    ) -> Result<AssignmentSnapshot> {
        let authenticated = self.owned_assignment(&auth, &handle)?;
        let parent = self.session_for_control(auth.session)?;
        let child = self.assignment_session(&authenticated)?;
        let model = read_contract(&authenticated)?.model;
        let store = self.retained.lock();
        let mut record = store
            .assignment(&handle)?
            .context("assignment disappeared")?;
        let payload = (&returned_request, epoch, &observed);
        if record.control_replay(&request, DispatchOperation::ConfirmHumanReturn, &payload)? {
            drop(store);
            return self.reconciliation_snapshot(record);
        }
        let captured = record.control.clone();
        let parent_binding = record.parent_binding.clone();
        let binding = record.binding.clone().context("assignment is not bound")?;
        child
            .with_delegated_context(
                &crate::session::DelegatedInput {
                    request: &request,
                    parent: &parent,
                    parent_binding: &parent_binding,
                    capability: &auth.capability,
                    child_binding: &binding,
                    model: &model,
                    text: "",
                    control: &captured,
                    control_epoch: epoch,
                    authority: crate::session::DelegatedAuthority::ConfirmHumanReturn,
                },
                |agent| {
                    let current = self.control_evidence(&record, agent)?;
                    if let Some(baseline) = &record.takeover_evidence {
                        record.control.attribution_invalidated |= baseline.human_input_revision
                            != current.human_input_revision
                            || baseline.input_revision != current.input_revision
                            || baseline.publication != current.publication
                            || baseline.publication_issue != current.publication_issue
                            || baseline.result_version != current.result_version;
                    }
                    confirm_return(
                        &mut record.control,
                        epoch,
                        &returned_request,
                        &observed,
                        &current,
                    )?;
                    record.record_control(
                        &request,
                        DispatchOperation::ConfirmHumanReturn,
                        &payload,
                    )?;
                    store.save_assignment(&record)
                },
            )?
            .context("parent or child control binding changed before confirmation")?;
        drop(store);
        super::super::dispatch::publish_session_changed(self, child.id());
        self.reconciliation_snapshot(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{
        AgentBinding, AgentProvider, AssignmentIdentity, SessionId, SessionRunId,
    };

    fn evidence() -> AssignmentControlEvidence {
        AssignmentControlEvidence {
            server_lifetime: "server-one".into(),
            identity: AssignmentIdentity {
                instance: "instance".into(),
                workflow: "workflow".into(),
                controller: "parent".into(),
                controller_epoch: 7,
                task: "task".into(),
                attempt: 1,
                contract: 1,
                request: "prepare".into(),
                session: Some(SessionId(2)),
                run: Some(SessionRunId(3)),
            },
            binding: AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "invocation".into(),
                conversation: "child".into(),
                generation: 1,
            },
            result_version: 1,
            activity_revision: 1,
            input_revision: 0,
            publication: None,
            publication_issue: None,
            human_input_revision: 0,
        }
    }

    #[test]
    fn passive_takeover_return_never_restores_old_epochs_or_invalidates_evidence() {
        let mut control = AssignmentControl::default();
        let observed = evidence();
        require_controller(&control, 1).unwrap();
        take_control(&mut control, 1).unwrap();
        assert_eq!(control.owner, ControlOwner::Human);
        assert!(require_controller(&control, 1).is_err());
        assert!(require_controller(&control, 2).is_err());
        return_control(&mut control, 2, "return", &observed, &observed, &observed).unwrap();
        assert!(require_controller(&control, 3).is_err());
        confirm_return(&mut control, 3, "return", &observed, &observed).unwrap();
        require_controller(&control, 4).unwrap();
        assert!(!control.attribution_invalidated);
        for epoch in 1..4 {
            assert!(require_controller(&control, epoch).is_err());
        }
        assert_eq!(observed.identity.controller_epoch, 7);
    }

    #[test]
    fn human_bytes_are_fenced_on_return_and_only_affected_attribution_changes() {
        let mut control = AssignmentControl::default();
        let sibling = AssignmentControl::default();
        let before = evidence();
        take_control(&mut control, 1).unwrap();
        note_human_input(&mut control, 2).unwrap();
        let mut current = before.clone();
        current.human_input_revision = 1;
        assert!(return_control(&mut control, 2, "return", &before, &before, &current).is_err());
        assert_eq!(control.epoch, 2);
        return_control(&mut control, 2, "return", &before, &current, &current).unwrap();
        assert!(note_human_input(&mut control, 2).is_err());
        assert!(control.attribution_invalidated);
        assert_eq!(sibling, AssignmentControl::default());
        let mut later = current.clone();
        later.input_revision += 1;
        assert!(confirm_return(&mut control, 3, "return", &current, &later).is_err());
        assert!(confirm_return(&mut control, 3, "other-return", &later, &later).is_err());
        assert_eq!(control.epoch, 3);
        confirm_return(&mut control, 3, "return", &later, &later).unwrap();
        assert!(
            control.attribution_invalidated,
            "confirmation does not recreate result attribution"
        );
    }

    #[test]
    fn uncertain_authority_cannot_be_cleared_by_return_confirmation() {
        let mut control = AssignmentControl::default();
        let evidence = evidence();
        suspend(&mut control).unwrap();
        assert_eq!(control.owner, ControlOwner::Uncertain);
        assert!(take_control(&mut control, 2).is_err());
        assert!(note_human_input(&mut control, 2).is_err());
        assert!(confirm_return(&mut control, 2, "return", &evidence, &evidence).is_err());
        assert_eq!(control.barrier, Some(ControlBarrier::AttributionChanged));
    }

    #[test]
    fn epoch_exhaustion_does_not_partially_transfer_ownership() {
        let mut control = AssignmentControl {
            epoch: u64::MAX,
            ..AssignmentControl::default()
        };
        let before = control.clone();
        assert!(take_control(&mut control, u64::MAX).is_err());
        assert_eq!(control, before);
    }
    #[test]
    fn control_receipts_are_request_exact_bounded_and_do_not_rewrite_unknown_input() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = super::super::tests::record(
            dir.path().join("assignment"),
            DispatchStatus::InputUnknown,
        );
        record.upgrade_legacy();
        let original = record.receipts.clone();
        let payload = (1, "current-view");
        assert!(
            !record
                .control_replay("take", DispatchOperation::Takeover, &payload)
                .unwrap()
        );
        record
            .record_control("take", DispatchOperation::Takeover, &payload)
            .unwrap();
        assert!(
            record
                .control_replay("take", DispatchOperation::Takeover, &payload)
                .unwrap()
        );
        assert!(
            record
                .control_replay("take", DispatchOperation::Takeover, &(2, "current-view"))
                .is_err()
        );
        assert!(
            record
                .control_replay("take", DispatchOperation::HumanReturn, &payload)
                .is_err()
        );
        assert_eq!(&record.receipts[..original.len()], original.as_slice());
        assert!(record.receipts.iter().any(|receipt| matches!(
            receipt.operation,
            DispatchOperation::InitialInput | DispatchOperation::Followup
        ) && matches!(
            receipt.outcome,
            DispatchOutcome::Unknown | DispatchOutcome::Partial
        )));
        for n in 1..64 {
            record
                .record_control(&format!("take-{n}"), DispatchOperation::Takeover, &payload)
                .unwrap();
        }
        assert!(
            record
                .control_replay("overflow", DispatchOperation::Takeover, &payload)
                .is_err()
        );
        assert!(
            record
                .control_replay("take", DispatchOperation::Takeover, &payload)
                .unwrap()
        );
        assert_eq!(record.control_intents.len(), 64);
    }

    #[test]
    fn legacy_metadata_preserves_receipt_digests_without_inventing_initial_authority() {
        let dir = tempfile::tempdir().unwrap();
        let mut record =
            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Launched);
        record.upgrade_legacy();
        let mut raw = serde_json::to_value(&record).unwrap();
        for field in [
            "control",
            "takeover_evidence",
            "control_intents",
            "initial_control_epoch",
        ] {
            raw.as_object_mut().unwrap().remove(field);
        }
        let old_receipts = record.receipts.clone();
        let old_digests = record.request_digests.clone();
        let restored: Record = serde_json::from_value(raw).unwrap();
        assert_eq!(restored.control, AssignmentControl::default());
        assert!(
            restored.initial_control_epoch.is_none(),
            "a legacy Accepted initial intent has no captured authority"
        );
        assert_eq!(restored.receipts, old_receipts);
        assert_eq!(restored.request_digests, old_digests);
        let intent: super::super::followups::FollowupIntent =
            serde_json::from_value(serde_json::json!({
                "request":"legacy", "expected":record.identity,"expected_result_version":1,
                "target":record.identity,"result_version":2,"digest":"original-payload-digest",
                "contract_digest":"original-contract-digest","brief_digest":"original-brief-digest"
            }))
            .unwrap();
        assert_eq!(intent.expected_control_epoch, 0);
        assert!(require_controller(&restored.control, intent.expected_control_epoch).is_err());
        assert_eq!(intent.digest, "original-payload-digest");
    }
}
