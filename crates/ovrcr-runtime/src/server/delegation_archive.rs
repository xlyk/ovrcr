//! Archive admission, owned stop and filing reuse the existing Server/Session owners.
use super::super::delegation_recovery::WorkflowRecord;
use super::*;
use crate::retained::{Disposition, SessionStore};
use crate::session::{DelegatedAuthority, DelegatedInput, Session};
use std::sync::Arc;

const MAX_ARCHIVE_RECORD_BYTES: usize = 262_144;
const MAX_CONTROL_OPERATIONS: usize = 64;
// All receipt identifiers are bounded ASCII (128), digests (64), two bindings,
// and fixed-size numbers/enums. Validate the actual serialized receipt too.
const CONTINUATION_BYTES: usize = 16_384;
const OUTCOME_GROWTH_BYTES: usize = 512;

impl Record {
    fn archive_continued(&self, request: &str) -> bool {
        self.archive_receipts.iter().any(|receipt| {
            receipt.intent.request.continuation_of.as_deref() == Some(request)
                && matches!(receipt.outcome, ArchiveOutcome::Archived { .. })
        })
    }

    pub(crate) fn archive_pending(&self) -> Option<&ArchiveReceipt> {
        self.archive_receipts.iter().find(|receipt| {
            receipt.intent.request.continuation_of.is_none()
                && matches!(
                    receipt.outcome,
                    ArchiveOutcome::StopUnknown | ArchiveOutcome::StoppedArchivePending
                )
                && !self.archive_continued(&receipt.intent.request.request)
        })
    }

    /// The pending journal itself durably reserves a future owner one attempt.
    /// Failed continuation transactions add no receipt and cannot spend this slot.
    pub(crate) fn validate_archive_storage(&self) -> Result<()> {
        let mut requests = std::collections::HashSet::new();
        let mut continuations = std::collections::HashSet::new();
        let mut reservations = 0usize;
        let mut reserved_bytes = 0usize;
        for receipt in &self.archive_receipts {
            let intent = &receipt.intent;
            let request = &intent.request;
            request.validate().map_err(anyhow::Error::msg)?;
            super::cancellation::CancellationAuthority::validate_capture(&intent.authority)?;
            if !requests.insert(&request.request)
                || request.handle != self.handle
                || intent.authority.instance != self.identity.instance
                || intent.authority.workflow != self.identity.workflow
                || intent.authority.controller != self.identity.controller
                || intent.digest != digest(&serde_json::to_vec(request)?)
                || self.receipts.iter().any(|r| r.request == request.request)
                || self.followups.iter().any(|r| r.request == request.request)
                || self
                    .control_intents
                    .iter()
                    .any(|r| r.request == request.request)
                || request.request == self.handle
                || serde_json::to_vec(receipt)?.len() > CONTINUATION_BYTES
            {
                bail!("archive journal identity, provenance or bounded size changed");
            }
            if let Some(binding) = &intent.child_binding {
                binding.validate()?;
            }
            match &receipt.outcome {
                ArchiveOutcome::Refused { .. } => {
                    if intent.control_epoch_admitted.is_some() || request.continuation_of.is_some()
                    {
                        bail!("archive refusal cannot own a stop or consume continuation capacity");
                    }
                }
                ArchiveOutcome::StopUnknown | ArchiveOutcome::StoppedArchivePending => {
                    if request.continuation_of.is_some() {
                        bail!("continuation must commit only with final filing");
                    }
                    if !self.archive_continued(&request.request) {
                        reservations += 1;
                        // Outcome growth spends its own reserved headroom. Keep
                        // future continuation bytes protected during certification.
                        reserved_bytes += CONTINUATION_BYTES
                            + 1
                            + OUTCOME_GROWTH_BYTES
                                .saturating_sub(serde_json::to_vec(&receipt.outcome)?.len());
                    }
                }
                ArchiveOutcome::Archived {
                    previous_run,
                    archived_run,
                } => {
                    if Some(*previous_run) != request.expected_identity.run
                        || previous_run.0.checked_add(1) != Some(archived_run.0)
                    {
                        bail!("archived run transition changed");
                    }
                }
            }
            if !matches!(receipt.outcome, ArchiveOutcome::Refused { .. }) {
                if request.expected_identity.instance != self.identity.instance
                    || request.expected_identity.workflow != self.identity.workflow
                    || request.expected_identity.session != self.identity.session
                    || request.expected_identity.run != self.identity.run
                    || intent.authority.epoch < request.expected_identity.controller_epoch
                {
                    bail!("effectful archive target provenance changed");
                }
                let admitted = intent
                    .control_epoch_admitted
                    .context("archive barrier epoch missing")?;
                if request.continuation_of.is_none()
                    && request.expected_control_epoch.checked_add(1) != Some(admitted)
                {
                    bail!("archive admission epoch changed");
                }
            }
            if let Some(predecessor) = &request.continuation_of {
                let original = self
                    .archive_receipts
                    .iter()
                    .find(|r| r.intent.request.request == *predecessor)
                    .context("continuation predecessor missing")?;
                if !continuations.insert(predecessor)
                    || !matches!(receipt.outcome, ArchiveOutcome::Archived { .. })
                    || !matches!(original.outcome, ArchiveOutcome::StoppedArchivePending)
                    || original.intent.request.continuation_of.is_some()
                    || request.expected_identity != original.intent.request.expected_identity
                    || request.expected_result_version
                        != original.intent.request.expected_result_version
                    || request.expected_obligation_revision
                        != original.intent.request.expected_obligation_revision
                    || Some(request.expected_control_epoch)
                        != original.intent.control_epoch_admitted
                    || intent.control_epoch_admitted != original.intent.control_epoch_admitted
                {
                    bail!("continuation does not preserve its exact stopped predecessor");
                }
            }
        }
        if reservations > 1 {
            bail!("multiple unresolved archive effects on one resource");
        }
        let operations = self.control_intents.len() + self.archive_receipts.len();
        if operations + reservations > MAX_CONTROL_OPERATIONS {
            bail!("archive operation storage is full; protected continuation retained");
        }
        // Every writer preserves independent answer outcome and archive capacity.
        // In particular, an archive refusal cannot spend an Accepted answer's margin.
        let answer_bytes = self.answer_outcome_reserved_bytes();
        if (!self.archive_receipts.is_empty() || answer_bytes > 0)
            && serde_json::to_vec(self)?.len() + reserved_bytes + answer_bytes
                > MAX_ARCHIVE_RECORD_BYTES
        {
            bail!("archive byte storage is full; protected continuation retained");
        }
        if let Some(pending) = self.archive_pending() {
            if self.control.epoch
                != pending
                    .intent
                    .control_epoch_admitted
                    .context("missing archive epoch")?
                || !matches!(&self.control.barrier, Some(ControlBarrier::Archive { request }) if request == &pending.intent.request.request)
            {
                bail!("unresolved archive lost its exact barrier");
            }
        } else if matches!(self.control.barrier, Some(ControlBarrier::Archive { .. })) {
            bail!("archive barrier lacks an unresolved original receipt");
        }
        Ok(())
    }

    fn archive_replay(&self, request: &ArchiveRequest) -> Result<bool> {
        if let Some(receipt) = self
            .archive_receipts
            .iter()
            .find(|r| r.intent.request.request == request.request)
        {
            if receipt.intent.request != *request
                || receipt.intent.digest != digest(&serde_json::to_vec(request)?)
            {
                bail!("archive request already belongs to another immutable intent");
            }
            return Ok(true);
        }
        if self.delivery_request_used(&request.request) {
            bail!("archive request already identifies another operation");
        }
        Ok(false)
    }

    fn admit_archive(
        &mut self,
        request: &ArchiveRequest,
        authority: &WorkflowAuthority,
        stopped: bool,
    ) -> Result<()> {
        let next = self
            .control
            .epoch
            .checked_add(1)
            .context("child control epoch exhausted")?;
        self.control.epoch = next;
        self.control.barrier = Some(ControlBarrier::Archive {
            request: request.request.clone(),
        });
        self.archive_receipts.push(ArchiveReceipt {
            intent: ArchiveIntent {
                request: request.clone(),
                digest: digest(&serde_json::to_vec(request)?),
                authority: authority.clone(),
                control_epoch_admitted: Some(next),
                child_binding: self.binding.clone(),
            },
            outcome: if stopped {
                ArchiveOutcome::StoppedArchivePending
            } else {
                ArchiveOutcome::StopUnknown
            },
        });
        self.validate_archive_storage()
    }

    fn require_archive_barrier(&self, original: &ArchiveReceipt) -> Result<()> {
        if self.identity != original.intent.request.expected_identity
            || self.result_version != original.intent.request.expected_result_version
            || self.obligation_revision != original.intent.request.expected_obligation_revision
            || self.control.owner != ControlOwner::Controller
            || Some(self.control.epoch) != original.intent.control_epoch_admitted
            || !matches!(&self.control.barrier, Some(ControlBarrier::Archive { request }) if request == &original.intent.request.request)
        {
            bail!("archive target or exact predecessor barrier changed");
        }
        Ok(())
    }
}

impl SessionStore {
    /// SQL staging only. Caller owns the transaction and publishes cache after commit.
    fn stage_archive_stop(&self, id: SessionId, run: SessionRunId) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE retained_sessions SET stopped=1 WHERE id=?1 AND run=?2 AND disposition!=1",
            rusqlite::params![i64::try_from(id.0)?, i64::try_from(run.0)?],
        )?;
        if changed != 1 {
            bail!("stopped archive target changed");
        }
        Ok(())
    }

    fn stage_assignment_archive(&self, id: SessionId, run: SessionRunId) -> Result<SessionRunId> {
        let next = SessionRunId(run.0.checked_add(1).context("archive run exhausted")?);
        let changed = self.connection.execute(
            "UPDATE retained_sessions SET disposition=1, run=?1 WHERE id=?2 AND run=?3 AND stopped=1 AND disposition!=1",
            rusqlite::params![i64::try_from(next.0)?, i64::try_from(id.0)?, i64::try_from(run.0)?],
        )?;
        if changed != 1 {
            bail!("archive requires the exact durably stopped unchanged run");
        }
        Ok(next)
    }
}

impl ServerState {
    fn refuse_archive(
        &self,
        store: &SessionStore,
        auth: &DelegationAuth,
        mut record: Record,
        request: &ArchiveRequest,
        authority: &WorkflowAuthority,
        reason: ArchiveRefusal,
    ) -> Result<Record> {
        // Refusals also preserve every pending operation's protected capacity.
        record.archive_receipts.push(ArchiveReceipt {
            intent: ArchiveIntent {
                request: request.clone(),
                digest: digest(&serde_json::to_vec(request)?),
                authority: authority.clone(),
                control_epoch_admitted: None,
                child_binding: record.binding.clone(),
            },
            outcome: ArchiveOutcome::Refused { reason },
        });
        record.validate_archive_storage()?;
        let parent = self.delegation_auth(auth)?;
        parent.with_workflow_context(auth, authority, || store.save_assignment(&record))?;
        Ok(record)
    }

    fn archive_admission_refusal(
        &self,
        record: &Record,
        workflow: &WorkflowRecord,
        request: &ArchiveRequest,
    ) -> Option<ArchiveRefusal> {
        if let Err(reason) = workflow.archive_eligibility(record, request) {
            return Some(reason);
        }
        if record.control.owner == ControlOwner::Human {
            return Some(ArchiveRefusal::HumanOwned);
        }
        if record.control.owner == ControlOwner::Uncertain {
            return Some(ArchiveRefusal::OwnershipUncertain);
        }
        if record.control.epoch != request.expected_control_epoch
            || record.control.barrier.is_some()
            || record.archive_pending().is_some()
            || record.unresolved_input()
            || record.pending_answer()
            || record.receipts.iter().any(|r| {
                r.operation == DispatchOperation::Followup && r.outcome == DispatchOutcome::Accepted
            })
        {
            return Some(ArchiveRefusal::UnresolvedControl);
        }
        if record.control.attribution_invalidated {
            return Some(ArchiveRefusal::AttributionChanged);
        }
        None
    }

    /// Routed before delegation() acquires its match-wide mutation guard.
    pub(crate) fn archive_assignment(
        &self,
        auth: DelegationAuth,
        request: ArchiveRequest,
        grace: Duration,
    ) -> Result<AssignmentSnapshot> {
        request.validate().map_err(anyhow::Error::msg)?;
        let mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let mut store = self.retained.lock();
        let mut record = store
            .assignment(&request.handle)?
            .context("archive assignment missing")?;
        record.validate_archive_storage()?;
        let authority = self.require_workflow(&store, &auth, &record)?;
        let workflow = store
            .workflow(&record.identity.workflow)?
            .context("workflow missing")?;
        workflow.validate_archive_provenance(&record)?;
        // Current-owner authentication precedes replay; run validation follows it.
        if record.archive_replay(&request)? {
            drop(store);
            drop(mutation);
            return self.reconciliation_snapshot(record);
        }
        if workflow
            .deliveries
            .iter()
            .any(|d| d.reference.request == request.request)
            || workflow
                .transfers
                .iter()
                .any(|r| r.request == request.request)
            || workflow
                .cancellation
                .as_ref()
                .is_some_and(|c| c.receipt.request == request.request)
        {
            bail!("archive request already identifies a workflow operation");
        }
        let (members, _) = store.workflow_members(&record.identity.workflow)?;
        if members.iter().any(|member| {
            member.handle != record.handle && member.delivery_request_used(&request.request)
        }) {
            bail!("archive request already identifies another workflow member's operation");
        }
        let id = request
            .expected_identity
            .session
            .context("archive child missing")?;
        let run = request
            .expected_identity
            .run
            .context("archive run missing")?;
        let child = self.sessions.lock().unwrap().get(&id).cloned();
        let current = store.get(id).context("retained archive target missing")?;
        if request.continuation_of.is_some() {
            let original =
                self.require_archive_continuation(&record, &workflow, &request, current.stopped)?;
            if current.run != run
                || current.disposition == Disposition::Archived
                || child.as_ref().is_some_and(|s| s.run() != run)
            {
                bail!("stopped continuation target changed");
            }
            // No separate admission, capability check, termination guard or signal.
            if !self.file_archive(&mut store, &auth, &mut record, &original, Some(&request))? {
                bail!(
                    "current parent source departed before continuation commit; no receipt was consumed"
                );
            }
            drop(store);
            self.finish_archive_cache(id, run, child.as_ref());
            drop(mutation);
            return self.reconciliation_snapshot(record);
        }
        let refusal = self
            .archive_admission_refusal(&record, &workflow, &request)
            .or_else(|| {
                if current.run != run
                    || current.disposition == Disposition::Archived
                    || child.as_ref().is_some_and(|s| s.run() != run)
                {
                    Some(ArchiveRefusal::StaleTarget)
                } else if !current.stopped && child.is_none() {
                    Some(ArchiveRefusal::OwnershipUncertain)
                } else {
                    None
                }
            });
        if let Some(reason) = refusal {
            let record =
                self.refuse_archive(&store, &auth, record, &request, &authority, reason)?;
            drop(store);
            drop(mutation);
            return self.reconciliation_snapshot(record);
        }
        if current.stopped {
            record.admit_archive(&request, &authority, true)?;
            let parent = self.delegation_auth(&auth)?;
            parent.with_workflow_context(&auth, &authority, || store.save_assignment(&record))?;
            let original = record
                .archive_pending()
                .context("stopped archive admission missing")?
                .clone();
            let filed = self.file_archive(&mut store, &auth, &mut record, &original, None)?;
            drop(store);
            if filed {
                self.finish_archive_cache(id, run, child.as_ref());
            }
            drop(mutation);
            return self.reconciliation_snapshot(record);
        }
        let child = child.context("archive has no current owned Session")?;
        let Some(termination) = child.try_termination()? else {
            let record = self.refuse_archive(
                &store,
                &auth,
                record,
                &request,
                &authority,
                ArchiveRefusal::Busy,
            )?;
            drop(store);
            drop(mutation);
            return self.reconciliation_snapshot(record);
        };
        let parent = self.delegation_auth(&auth)?;
        let binding = record
            .binding
            .clone()
            .context("archive child has no reporting binding")?;
        let model = read_contract(&record)?.model;
        let control = record.control.clone();
        hold_before_effect("archive-before-admission", &request.request)?;
        // Shared Session seam uses the existing ordered paired-state checks and
        // revokes with already-locked child state ONLY after this closure commits.
        child
            .with_delegated_archive(
                &DelegatedInput {
                    request: &request.request,
                    parent: &parent,
                    parent_binding: &authority.binding,
                    parent_producer: Some(&authority.producer),
                    parent_source: auth.source.as_ref(),
                    capability: &auth.capability,
                    child_binding: &binding,
                    model: &model,
                    text: "",
                    control: &control,
                    control_epoch: request.expected_control_epoch,
                    authority: DelegatedAuthority::Controller,
                },
                || {
                    record.admit_archive(&request, &authority, false)?;
                    if take_fault("archive-before-save", &request.request).is_some() {
                        bail!("owned archive admission fault");
                    }
                    let transaction = store.connection.unchecked_transaction()?;
                    store.save_assignment(&record)?;
                    transaction.commit()?;
                    Ok(())
                },
            )?
            .context("archive parent or child source changed before admission")?;
        let original = record
            .archive_pending()
            .context("archive admission missing")?
            .clone();
        drop(store);
        drop(mutation);
        if take_fault("archive-abandon-admission", &request.request).is_some() {
            bail!("owned archive abandonment after durable admission; original stop unknown");
        }
        hold_before_effect("archive-after-admission", &request.request)?;
        // The existing termination owner operates only on the captured Arc/run.
        // A later parent transfer never retargets this already admitted chain.
        let stopped = termination.terminate(grace).is_ok();
        drop(termination);
        hold_before_effect("archive-after-stop", &request.request)?;
        let mutation = self.mutation_lock.lock().unwrap();
        if stopped {
            self.persist_session_titles(&child)?;
        }
        let mut store = self.retained.lock();
        let mut record = store
            .assignment(&request.handle)?
            .context("archive journal missing after stop")?;
        record.require_archive_barrier(&original)?;
        self.ensure_archive_target(&store, id, run, Some(&child))?;
        if !stopped {
            drop(store);
            drop(mutation);
            return self.reconciliation_snapshot(record);
        }
        // Certify stop independently of current permission for a new filing.
        let mut certified = record.clone();
        certified
            .archive_receipts
            .iter_mut()
            .find(|r| r.intent == original.intent)
            .context("original archive receipt changed")?
            .outcome = ArchiveOutcome::StoppedArchivePending;
        let transaction = store.connection.unchecked_transaction()?;
        store.stage_archive_stop(id, run)?;
        store.save_assignment(&certified)?;
        transaction.commit()?;
        store.publish_stopped(id, run);
        record = certified;
        let original = record
            .archive_pending()
            .context("certified archive missing")?
            .clone();
        hold_before_effect("archive-before-filing", &request.request)?;
        // PTY exit or transfer leaves the durable stopped fact for a current owner.
        let current = self.require_workflow(&store, &auth, &record);
        if current
            .as_ref()
            .is_ok_and(|a| *a == original.intent.authority)
        {
            self.file_archive(&mut store, &auth, &mut record, &original, None)?;
        }
        let archived = record.archive_receipts.iter().any(|r| {
            r.intent.request.request == request.request
                && matches!(r.outcome, ArchiveOutcome::Archived { .. })
        });
        drop(store);
        if archived {
            self.finish_archive_cache(id, run, Some(&child));
        }
        drop(mutation);
        self.reconciliation_snapshot(record)
    }

    fn require_archive_continuation(
        &self,
        record: &Record,
        workflow: &WorkflowRecord,
        request: &ArchiveRequest,
        stopped: bool,
    ) -> Result<ArchiveReceipt> {
        let predecessor = request
            .continuation_of
            .as_ref()
            .context("continuation predecessor missing")?;
        let original = record
            .archive_pending()
            .context("no unresolved original archive")?;
        if original.intent.request.request != *predecessor
            || !matches!(original.outcome, ArchiveOutcome::StoppedArchivePending)
            || !stopped
            || request.expected_identity != original.intent.request.expected_identity
            || request.expected_result_version != original.intent.request.expected_result_version
            || request.expected_obligation_revision
                != original.intent.request.expected_obligation_revision
            || Some(request.expected_control_epoch) != original.intent.control_epoch_admitted
        {
            bail!(
                "continuation requires its exact certified stopped predecessor; no stop is replayed"
            );
        }
        record.require_archive_barrier(original)?;
        workflow
            .archive_eligibility(record, request)
            .map_err(|r| anyhow::anyhow!("continuation delivery refused: {r:?}"))?;
        Ok(original.clone())
    }

    /// Caller owns mutation/retained. Parent state stays pinned through commit.
    /// False means the final source pin failed; SQL failures remain errors.
    fn file_archive(
        &self,
        store: &mut SessionStore,
        auth: &DelegationAuth,
        record: &mut Record,
        original: &ArchiveReceipt,
        continuation: Option<&ArchiveRequest>,
    ) -> Result<bool> {
        record.require_archive_barrier(original)?;
        let authority = self.require_workflow(store, auth, record)?;
        if continuation.is_none() && authority != original.intent.authority {
            bail!("original archive owner changed; explicit continuation required");
        }
        let request = continuation.unwrap_or(&original.intent.request);
        let workflow = store
            .workflow(&record.identity.workflow)?
            .context("workflow missing")?;
        workflow
            .archive_eligibility(record, request)
            .map_err(|r| anyhow::anyhow!("archive filing delivery refused: {r:?}"))?;
        let id = original
            .intent
            .request
            .expected_identity
            .session
            .context("child missing")?;
        let run = original
            .intent
            .request
            .expected_identity
            .run
            .context("run missing")?;
        let next = SessionRunId(run.0.checked_add(1).context("archive run exhausted")?);
        let mut filed = record.clone();
        if let Some(request) = continuation {
            filed.archive_receipts.push(ArchiveReceipt {
                intent: ArchiveIntent {
                    request: request.clone(),
                    digest: digest(&serde_json::to_vec(request)?),
                    authority: authority.clone(),
                    control_epoch_admitted: original.intent.control_epoch_admitted,
                    child_binding: original.intent.child_binding.clone(),
                },
                outcome: ArchiveOutcome::Archived {
                    previous_run: run,
                    archived_run: next,
                },
            });
        } else {
            filed
                .archive_receipts
                .iter_mut()
                .find(|r| r.intent == original.intent)
                .context("original archive receipt missing")?
                .outcome = ArchiveOutcome::Archived {
                previous_run: run,
                archived_run: next,
            };
        }
        filed.control.barrier = None;
        filed.validate_archive_storage()?;
        let parent = self.delegation_auth(auth)?;
        // This diagnostic gate precedes the final state pin so a PTY exit can win.
        hold_before_effect("archive-before-parent-pin", &request.request)?;
        // Nest the SQL result so source loss is distinguishable from a storage
        // error without retrying a failed transaction or hiding its cause.
        let pinned = parent.with_workflow_context(auth, &authority, || {
            Ok((|| -> Result<()> {
                let transaction = store.connection.unchecked_transaction()?;
                store.stage_assignment_archive(id, run)?;
                if take_fault("archive-after-stage", &request.request).is_some() {
                    bail!("owned archive transaction fault after row stage");
                }
                store.save_assignment(&filed)?;
                if take_fault("archive-before-commit", &request.request).is_some() {
                    bail!("owned archive final commit fault");
                }
                transaction.commit()?;
                Ok(())
            })())
        });
        let Ok(committed) = pinned else {
            return Ok(false);
        };
        committed?;
        // Exact cache publication point: SQL and receipt have both committed,
        // and current parent state has been released. No cache mutates on rollback.
        store.publish_archived(id, run, next);
        *record = filed;
        Ok(true)
    }

    fn ensure_archive_target(
        &self,
        store: &SessionStore,
        id: SessionId,
        run: SessionRunId,
        captured: Option<&Arc<Session>>,
    ) -> Result<()> {
        let row = store.get(id).context("archive target missing")?;
        let sessions = self.sessions.lock().unwrap();
        let same = match (captured, sessions.get(&id)) {
            (Some(expected), Some(current)) => Arc::ptr_eq(expected, current),
            (None, None) => true,
            _ => false,
        };
        if row.run != run || row.disposition == Disposition::Archived || !same {
            bail!("archive original target replaced");
        }
        Ok(())
    }

    fn finish_archive_cache(
        &self,
        id: SessionId,
        run: SessionRunId,
        captured: Option<&Arc<Session>>,
    ) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(captured) = captured
            && sessions
                .get(&id)
                .is_some_and(|current| current.run() == run && Arc::ptr_eq(current, captured))
        {
            sessions.remove(&id);
        }
        drop(sessions);
        if let Some(captured) = captured {
            captured.revoke_hook_capability();
        }
        self.dashboard.forget_session(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Record, WorkflowAuthority, ArchiveRequest) {
        let binding = AgentBinding {
            provider: AgentProvider::Pi,
            invocation: "parent-invocation".into(),
            conversation: "parent-conversation".into(),
            generation: 1,
        };
        let identity = AssignmentIdentity {
            instance: "instance".into(),
            workflow: "workflow".into(),
            controller: "controller".into(),
            controller_epoch: 1,
            task: "task".into(),
            attempt: 1,
            contract: 1,
            request: "assignment".into(),
            session: Some(SessionId(2)),
            run: Some(SessionRunId(1)),
        };
        let record: Record = serde_json::from_value(serde_json::json!({
            "handle":"assignment","identity":identity,"binding":binding,"parent_binding":binding,
            "parent":1,"parent_run":1,"root":"/private/task-owned","contract_digest":"a".repeat(64),
            "brief_digest":null,"result_digest":null,"dispatch":"Applied","reason":null,
            "obligation_revision":1,"archive_receipts":[],"execution_attempts":1
        }))
        .unwrap();
        let authority = WorkflowAuthority {
            instance: "instance".into(),
            workflow: "workflow".into(),
            controller: "controller".into(),
            epoch: 1,
            parent: SessionId(1),
            parent_run: SessionRunId(1),
            binding,
            producer: "producer".into(),
            server_lifetime: "lifetime".into(),
        };
        let request = ArchiveRequest {
            request: "archive".into(),
            handle: "assignment".into(),
            expected_identity: identity,
            expected_result_version: 1,
            expected_obligation_revision: 1,
            expected_control_epoch: 1,
            delivery: DeliveryRef {
                request: "delivery".into(),
                digest: "a".repeat(64),
            },
            continuation_of: None,
        };
        (record, authority, request)
    }

    #[test]
    fn archive_reserves_last_slot_until_atomic_successful_continuation() {
        let (mut record, authority, request) = fixture();
        for index in 0..62 {
            record.control_intents.push(control::ControlIntent {
                request: format!("control-{index}"),
                operation: DispatchOperation::Takeover,
                digest: "a".repeat(64),
            });
        }
        record.admit_archive(&request, &authority, true).unwrap();
        let original = record.archive_pending().unwrap().clone();
        let mut exhausted = record.clone();
        exhausted.control_intents.push(control::ControlIntent {
            request: "steal-reserved-slot".into(),
            operation: DispatchOperation::Takeover,
            digest: "a".repeat(64),
        });
        assert!(exhausted.validate_archive_storage().is_err());
        let mut continuation = request.clone();
        continuation.request = "continuation".into();
        continuation.expected_control_epoch = record.control.epoch;
        continuation.continuation_of = Some(request.request.clone());
        record.archive_receipts.push(ArchiveReceipt {
            intent: ArchiveIntent {
                request: continuation.clone(),
                digest: digest(&serde_json::to_vec(&continuation).unwrap()),
                authority,
                control_epoch_admitted: original.intent.control_epoch_admitted,
                child_binding: original.intent.child_binding.clone(),
            },
            outcome: ArchiveOutcome::Archived {
                previous_run: SessionRunId(1),
                archived_run: SessionRunId(2),
            },
        });
        record.control.barrier = None;
        record.validate_archive_storage().unwrap();
        assert!(record.archive_pending().is_none());
        assert_eq!(record.archive_receipts[0], original);
    }

    #[test]
    fn stop_certification_spends_original_outcome_headroom_without_spending_continuation_bytes() {
        let (mut record, authority, request) = fixture();
        record.admit_archive(&request, &authority, false).unwrap();
        record.reason = Some(String::new());
        let reserved = CONTINUATION_BYTES + 1 + OUTCOME_GROWTH_BYTES
            - serde_json::to_vec(&record.archive_receipts[0].outcome)
                .unwrap()
                .len();
        let available =
            MAX_ARCHIVE_RECORD_BYTES - reserved - serde_json::to_vec(&record).unwrap().len();
        record
            .reason
            .as_mut()
            .unwrap()
            .push_str(&"x".repeat(available));
        record.validate_archive_storage().unwrap();
        record.archive_receipts[0].outcome = ArchiveOutcome::StoppedArchivePending;
        record.validate_archive_storage().unwrap();
        record.reason.as_mut().unwrap().push('x');
        assert!(record.validate_archive_storage().is_err());
    }

    #[test]
    fn archive_reserves_bytes_against_later_record_writes_and_future_owner() {
        let (mut record, mut authority, request) = fixture();
        record.admit_archive(&request, &authority, true).unwrap();
        let remaining = MAX_ARCHIVE_RECORD_BYTES
            - CONTINUATION_BYTES
            - OUTCOME_GROWTH_BYTES
            - serde_json::to_vec(&record).unwrap().len()
            - 32;
        record.reason = Some("x".repeat(remaining));
        record.validate_archive_storage().unwrap();
        record.reason.as_mut().unwrap().push_str(&"x".repeat(64));
        assert!(record.validate_archive_storage().is_err());
        authority.instance = "i".repeat(128);
        authority.workflow = "w".repeat(128);
        authority.controller = "c".repeat(128);
        authority.producer = "p".repeat(128);
        authority.server_lifetime = "l".repeat(128);
        authority.binding.invocation = "\"".repeat(256);
        authority.binding.conversation = "\\".repeat(256);
        authority.epoch = u64::MAX;
        authority.parent = SessionId(u64::MAX);
        authority.parent_run = SessionRunId(u64::MAX);
        let mut maximal = record.archive_receipts[0].clone();
        maximal.intent.authority = authority.clone();
        maximal.intent.child_binding = Some(authority.binding);
        maximal.intent.request.request = "r".repeat(128);
        maximal.intent.request.handle = "h".repeat(128);
        maximal.intent.request.expected_identity.instance = "i".repeat(128);
        maximal.intent.request.expected_identity.workflow = "w".repeat(128);
        maximal.intent.request.expected_identity.controller = "c".repeat(128);
        maximal.intent.request.expected_identity.task = "t".repeat(128);
        maximal.intent.request.expected_identity.request = "h".repeat(128);
        maximal.intent.request.delivery.request = "d".repeat(128);
        maximal.intent.request.continuation_of = Some("a".repeat(128));
        maximal.outcome = ArchiveOutcome::Archived {
            previous_run: SessionRunId(u64::MAX - 1),
            archived_run: SessionRunId(u64::MAX),
        };
        assert!(serde_json::to_vec(&maximal).unwrap().len() < CONTINUATION_BYTES);
    }

    fn serialized_boundary_record(handle: &str, session: SessionId) -> (Record, WorkflowAuthority) {
        let (mut record, authority, mut request) = fixture();
        record.handle = handle.into();
        record.identity.request = handle.into();
        record.identity.session = Some(session);
        request.handle = handle.into();
        request.expected_identity = record.identity.clone();
        for index in 0..62 {
            record.control_intents.push(control::ControlIntent {
                request: format!("prior-control-{index}"),
                operation: DispatchOperation::Takeover,
                digest: "a".repeat(64),
            });
        }
        record.admit_archive(&request, &authority, true).unwrap();
        // Fixed serialized fixture size leaves little space for both the future
        // owner's receipt and the original outcome; no reservation formula here.
        record.reason = Some(String::new());
        let padding = 245_000 - serde_json::to_vec(&record).unwrap().len();
        record.reason = Some("x".repeat(padding));
        let bytes = serde_json::to_vec(&record).unwrap();
        assert_eq!(bytes.len(), 245_000);
        let decoded: Record = serde_json::from_slice(&bytes).unwrap();
        decoded.validate_archive_storage().unwrap();
        (decoded, authority)
    }

    fn future_owner_continuation(
        record: &Record,
        mut authority: WorkflowAuthority,
    ) -> ArchiveReceipt {
        let original = record.archive_pending().unwrap();
        let mut request = original.intent.request.clone();
        request.request = "continuation-for-current-owner-".to_owned() + &"r".repeat(97);
        assert_eq!(request.request.len(), 128);
        request.expected_control_epoch = original.intent.control_epoch_admitted.unwrap();
        request.continuation_of = Some(original.intent.request.request.clone());
        // Exercise an independently bounded, much larger successor principal.
        authority.epoch = u64::MAX;
        authority.parent = SessionId(u64::MAX);
        authority.parent_run = SessionRunId(u64::MAX);
        authority.producer = "p".repeat(128);
        authority.server_lifetime = "l".repeat(128);
        authority.binding.invocation = "\"".repeat(256);
        authority.binding.conversation = "\\".repeat(256);
        authority.binding.generation = u64::MAX;
        let run = request.expected_identity.run.unwrap();
        ArchiveReceipt {
            intent: ArchiveIntent {
                digest: digest(&serde_json::to_vec(&request).unwrap()),
                request,
                authority,
                control_epoch_admitted: original.intent.control_epoch_admitted,
                child_binding: original.intent.child_binding.clone(),
            },
            outcome: ArchiveOutcome::Archived {
                previous_run: run,
                archived_run: SessionRunId(run.0 + 1),
            },
        }
    }

    fn decoded_candidate(record: &Record, receipt: ArchiveReceipt, retire_barrier: bool) -> Record {
        let mut candidate = record.clone();
        candidate.archive_receipts.push(receipt);
        if retire_barrier {
            candidate.control.barrier = None;
        }
        serde_json::from_slice(&serde_json::to_vec(&candidate).unwrap()).unwrap()
    }

    #[test]
    fn serialized_near_full_records_protect_slots_and_bytes_until_their_own_continuation() {
        let (record, authority) = serialized_boundary_record("first-resource", SessionId(2));
        let (other, other_authority) = serialized_boundary_record("second-resource", SessionId(3));
        let original = serde_json::to_vec(&record).unwrap();
        let other_original = serde_json::to_vec(&other).unwrap();
        let extra_control = control::ControlIntent {
            request: "cannot-spend-protected-slot".into(),
            operation: DispatchOperation::Takeover,
            digest: "a".repeat(64),
        };
        // Plenty of actual row bytes alone cannot release the protected slot.
        let mut slot_limited: Record = serde_json::from_slice(&original).unwrap();
        slot_limited.reason = None;
        slot_limited.control_intents.push(extra_control.clone());
        assert!(serde_json::to_vec(&slot_limited).unwrap().len() < 32_000);
        assert!(
            slot_limited
                .validate_archive_storage()
                .unwrap_err()
                .to_string()
                .contains("operation storage")
        );

        // Conversely, a spare operation slot cannot spend protected bytes. This
        // refusal is otherwise a valid, bounded public receipt for a future owner.
        let mut refusal = future_owner_continuation(&record, authority.clone());
        refusal.intent.request.continuation_of = None;
        refusal.intent.control_epoch_admitted = None;
        refusal.intent.digest = digest(&serde_json::to_vec(&refusal.intent.request).unwrap());
        refusal.outcome = ArchiveOutcome::Refused {
            reason: ArchiveRefusal::Busy,
        };
        let mut byte_limited: Record = serde_json::from_slice(&original).unwrap();
        byte_limited.control_intents.pop();
        byte_limited.archive_receipts.push(refusal);
        assert_eq!(
            byte_limited.control_intents.len() + byte_limited.archive_receipts.len(),
            63
        );
        assert!(
            serde_json::to_vec(&byte_limited).unwrap().len() < 262_144,
            "plain serialized size alone must not authorize spending reserved bytes"
        );
        assert!(
            byte_limited
                .validate_archive_storage()
                .unwrap_err()
                .to_string()
                .contains("byte storage")
        );

        let continuation = future_owner_continuation(&record, authority);
        assert!(serde_json::to_vec(&continuation).unwrap().len() > 2_000);
        let completed = decoded_candidate(&record, continuation.clone(), true);
        completed.validate_archive_storage().unwrap();
        assert_eq!(
            completed.control_intents.len() + completed.archive_receipts.len(),
            64
        );
        assert!(serde_json::to_vec(&completed).unwrap().len() <= 262_144);
        assert!(completed.archive_pending().is_none());
        assert_eq!(completed.archive_receipts[0], record.archive_receipts[0]);
        assert!(
            completed
                .archive_replay(&continuation.intent.request)
                .unwrap()
        );
        let mut after_completion = completed.clone();
        after_completion.control_intents.push(extra_control.clone());
        assert!(
            after_completion.validate_archive_storage().is_err(),
            "successful filing consumes its reserved slot, never grants another operation"
        );

        // A different resource still needs its own reserved outcome and slot.
        let mut other_attempt: Record = serde_json::from_slice(&other_original).unwrap();
        other_attempt.control_intents.push(extra_control);
        assert!(other_attempt.validate_archive_storage().is_err());
        assert!(other.archive_pending().is_some());
        let other_completed = decoded_candidate(
            &other,
            future_owner_continuation(&other, other_authority),
            true,
        );
        other_completed.validate_archive_storage().unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), original);
        assert_eq!(serde_json::to_vec(&other).unwrap(), other_original);
    }

    #[test]
    fn rejected_continuation_candidates_preserve_serialized_original_for_later_owner() {
        let (record, authority) = serialized_boundary_record("preserved-resource", SessionId(2));
        let original_bytes = serde_json::to_vec(&record).unwrap();
        let original_receipt = serde_json::to_vec(&record.archive_receipts[0]).unwrap();
        let mut candidates = Vec::new();
        let valid = future_owner_continuation(&record, authority.clone());
        let mut provisional = valid.clone();
        provisional.outcome = ArchiveOutcome::StopUnknown;
        candidates.push(("separately persisted admission", provisional, false));
        let mut wrong_predecessor = valid.clone();
        wrong_predecessor.intent.request.continuation_of = Some("some-other-original".into());
        wrong_predecessor.intent.digest =
            digest(&serde_json::to_vec(&wrong_predecessor.intent.request).unwrap());
        candidates.push(("foreign predecessor", wrong_predecessor, true));
        let mut wrong_epoch = valid.clone();
        wrong_epoch.intent.request.expected_control_epoch += 1;
        wrong_epoch.intent.digest =
            digest(&serde_json::to_vec(&wrong_epoch.intent.request).unwrap());
        candidates.push(("changed predecessor epoch", wrong_epoch, true));
        let mut wrong_run = valid.clone();
        wrong_run.intent.request.expected_identity.run = Some(SessionRunId(2));
        wrong_run.intent.digest = digest(&serde_json::to_vec(&wrong_run.intent.request).unwrap());
        wrong_run.outcome = ArchiveOutcome::Archived {
            previous_run: SessionRunId(2),
            archived_run: SessionRunId(3),
        };
        candidates.push(("replacement run", wrong_run, true));
        let mut oversized_owner = valid.clone();
        oversized_owner.intent.authority.producer.push('p');
        candidates.push(("unbounded future owner", oversized_owner, true));
        candidates.push(("unretired predecessor barrier", valid, false));

        for (index, (label, mut proposal, retire_barrier)) in candidates.into_iter().enumerate() {
            // Each attempt starts from the retained bytes, as a later request
            // would. Rejected metadata is never substituted for that original.
            let retained: Record = serde_json::from_slice(&original_bytes).unwrap();
            proposal.intent.authority.parent = SessionId(100 + index as u64);
            proposal.intent.authority.epoch = 2 + index as u64;
            proposal.intent.request.request = format!("unsuccessful-owner-{index}");
            proposal.intent.digest = digest(&serde_json::to_vec(&proposal.intent.request).unwrap());
            assert!(!retained.archive_replay(&proposal.intent.request).unwrap());
            let rejected = decoded_candidate(&retained, proposal, retire_barrier);
            assert!(rejected.validate_archive_storage().is_err(), "{label}");
            retained.validate_archive_storage().unwrap();
            assert_eq!(
                serde_json::to_vec(&retained.archive_receipts[0]).unwrap(),
                original_receipt,
                "{label}"
            );
            assert_eq!(
                serde_json::to_vec(&retained).unwrap(),
                original_bytes,
                "{label}"
            );
        }
        let retained: Record = serde_json::from_slice(&original_bytes).unwrap();
        let later = future_owner_continuation(&retained, authority);
        let completed = decoded_candidate(&retained, later, true);
        completed.validate_archive_storage().unwrap();
        assert_eq!(
            completed.archive_receipts.len(),
            2,
            "all failed proposals leave the original protected slot for the successful owner"
        );
        assert_eq!(
            serde_json::to_vec(&completed.archive_receipts[0]).unwrap(),
            original_receipt
        );
    }
}
