//! Fenced native delivery through the existing Session writer.
use super::interrupt::InterruptBaseline;
use super::*;

/// Original effect provenance survives parent transfer. A new confirmation uses
/// current workflow authority, never this historical authority as permission.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InterruptIntent {
    pub request: String,
    pub authority: ovrcr_protocol::WorkflowAuthority,
    pub control_epoch: u64,
    pub baseline: Option<InterruptBaseline>,
}

impl InterruptIntent {
    pub(super) fn validate(&self, receipt: &DispatchReceipt) -> Result<()> {
        super::cancellation::CancellationAuthority::validate_capture(&self.authority)?;
        if self.request != receipt.request
            || receipt.operation != DispatchOperation::Interrupt
            || self.control_epoch == 0
            || self.authority.instance != receipt.identity.instance
            || self.authority.workflow != receipt.identity.workflow
            || self.authority.controller != receipt.identity.controller
            || self.authority.epoch < receipt.identity.controller_epoch
        {
            bail!("interrupt journal provenance changed");
        }
        if let Some(baseline) = &self.baseline {
            baseline.binding.validate()?;
            validate_agent_id(&baseline.turn)?;
            if baseline.identity != receipt.identity
                || baseline.result_version != receipt.result_version
                || baseline.server_lifetime != self.authority.server_lifetime
                || baseline.binding.provider != AgentProvider::Pi
                || baseline.identity.session.is_none()
                || baseline.identity.run.is_none()
                || baseline.activity_revision == 0
                || baseline.native_control_revision == 0
            {
                bail!("interrupt baseline does not match original receipt");
            }
        } else if receipt.outcome != DispatchOutcome::Rejected {
            bail!("interrupt without native baseline cannot authorize delivery");
        }
        Ok(())
    }
}

use crate::retained::SessionStore;
use crate::session::{DelegatedAuthority, DelegatedInput};

impl ServerState {
    /// No bytes here. Cancellation prepares all records and commits them with its
    /// fixed capture in one transaction before calling the delivery stage.
    pub(super) fn prepare_interrupt(
        &self,
        store: &SessionStore,
        auth: &DelegationAuth,
        record: &mut Record,
        request: &str,
        expected: (&AssignmentIdentity, u32, u64),
    ) -> Result<bool> {
        let (expected, version, epoch) = expected;
        let payload = (expected, version, epoch);
        if record.control_replay(request, DispatchOperation::Interrupt, &payload)? {
            return Ok(false);
        }
        let authority = self.require_workflow(store, auth, record)?;
        let preparation = (|| -> Result<InterruptBaseline> {
            if record.identity != *expected || record.result_version != version {
                bail!("interrupt assignment identity or version changed");
            }
            control::require_controller(&record.control, epoch)?;
            if record.control.attribution_invalidated {
                bail!("assignment attribution is unresolved");
            }
            let parent = self.session_for_control(auth.session)?;
            let child = self.assignment_session(record)?;
            let model = expected_native_model(record)?;
            let binding = record.binding.as_ref().context("assignment is not bound")?;
            child
                .with_delegated_context(
                    &DelegatedInput {
                        request,
                        parent: &parent,
                        parent_binding: &authority.binding,
                        parent_producer: Some(&authority.producer),
                        parent_source: auth.source.as_ref(),
                        capability: &auth.capability,
                        child_binding: binding,
                        model: &model,
                        text: "",
                        control: &record.control,
                        control_epoch: epoch,
                        authority: DelegatedAuthority::Controller,
                    },
                    |agent| {
                        InterruptBaseline::capture(
                            &self.server_lifetime,
                            &record.identity,
                            version,
                            agent,
                        )
                    },
                )?
                .context("interrupt parent or child binding changed")
        })();
        record.record_control(request, DispatchOperation::Interrupt, &payload)?;
        let (baseline, outcome, reason) = match preparation {
            Ok(baseline) => {
                record.control.epoch = record.control.epoch.checked_add(1).context("child control epoch exhausted")?;
                record.control.barrier = Some(ControlBarrier::Interrupt { request: request.to_owned() });
                (Some(baseline), DispatchOutcome::Unknown, None)
            }
            Err(_) => (None, DispatchOutcome::Rejected, Some("native interrupt refused: current ownership, run, binding, busy turn or supported keybinding is unavailable".to_owned())),
        };
        record.interrupt_intents.push(InterruptIntent {
            request: request.to_owned(),
            authority,
            control_epoch: record.control.epoch,
            baseline,
        });
        let receipt = record
            .receipts
            .last_mut()
            .context("interrupt receipt missing")?;
        receipt.outcome = outcome;
        receipt.reason = reason;
        Ok(true)
    }

    /// This is called only in the original request after durable admission. A
    /// repeated request returns its receipt without reaching this method.
    pub(super) fn deliver_interrupt(
        &self,
        store: &SessionStore,
        auth: &DelegationAuth,
        record: &mut Record,
        request: &str,
    ) -> Result<()> {
        let intent = record
            .interrupt_intents
            .iter()
            .find(|i| i.request == request)
            .context("interrupt intent missing")?
            .clone();
        intent.validate(
            record
                .request_receipt(request, DispatchOperation::Interrupt)
                .context("original interrupt receipt missing")?,
        )?;
        let Some(baseline) = intent.baseline else {
            return Ok(());
        };
        let authority = self.require_workflow(store, auth, record)?;
        if authority != intent.authority {
            bail!("interrupt effect authority changed; original delivery remains unknown");
        }
        let barrier = Some(ControlBarrier::Interrupt {
            request: request.to_owned(),
        });
        if record.identity != baseline.identity
            || record.result_version != baseline.result_version
            || record.control.epoch != intent.control_epoch
            || record.control.barrier != barrier
        {
            bail!("interrupt child fence changed; original delivery remains unknown");
        }
        if take_fault("interrupt-before-effect", request).is_some() {
            return Ok(());
        }
        let parent = self.session_for_control(auth.session)?;
        let child = self.assignment_session(record)?;
        let model = expected_native_model(record)?;
        let captured = record.control.clone();
        let delivered = child.send_delegated_interrupt(
            &DelegatedInput {
                request,
                parent: &parent,
                parent_binding: &authority.binding,
                parent_producer: Some(&authority.producer),
                parent_source: auth.source.as_ref(),
                capability: &auth.capability,
                child_binding: &baseline.binding,
                model: &model,
                text: "",
                control: &captured,
                control_epoch: intent.control_epoch,
                authority: DelegatedAuthority::Interrupt,
            },
            |agent| {
                let current = InterruptBaseline::capture(
                    &self.server_lifetime,
                    &record.identity,
                    record.result_version,
                    agent,
                )?;
                if current != baseline {
                    bail!("native interrupt evidence changed before delivery");
                }
                Ok(())
            },
        );
        let receipt = record
            .receipts
            .iter_mut()
            .find(|r| r.request == request && r.operation == DispatchOperation::Interrupt)
            .context("interrupt receipt missing")?;
        match delivered {
            Ok(Some((outcome, input))) => {
                receipt.outcome = outcome;
                receipt.input = Some(input);
            }
            Ok(None) | Err(_) => {
                receipt.outcome = DispatchOutcome::Rejected;
                receipt.reason = Some(
                    "native interrupt fence changed before bytes; reconcile current assignment"
                        .into(),
                );
            }
        }
        store.save_assignment(record)?;
        Ok(())
    }

    pub(super) fn interrupt_assignment(
        &self,
        auth: DelegationAuth,
        handle: String,
        request: String,
        expected: AssignmentIdentity,
        version: u32,
        epoch: u64,
    ) -> Result<AssignmentSnapshot> {
        let mut record = self.owned_assignment(&auth, &handle)?;
        let store = self.retained.lock();
        if self.prepare_interrupt(
            &store,
            &auth,
            &mut record,
            &request,
            (&expected, version, epoch),
        )? {
            store.save_assignment(&record)?;
            self.deliver_interrupt(&store, &auth, &mut record, &request)?;
        }
        drop(store);
        if let Some(id) = record.identity.session {
            super::super::dispatch::publish_session_changed(self, id);
        }
        self.reconciliation_snapshot(record)
    }

    pub(super) fn confirm_interrupt(
        &self,
        auth: DelegationAuth,
        handle: String,
        request: String,
        interrupted_request: String,
        epoch: u64,
        observed: AssignmentControlEvidence,
    ) -> Result<AssignmentSnapshot> {
        let mut record = self.owned_assignment(&auth, &handle)?;
        let store = self.retained.lock();
        let payload = (&interrupted_request, epoch, &observed);
        if record.control_replay(&request, DispatchOperation::ConfirmInterrupt, &payload)? {
            drop(store);
            return self.reconciliation_snapshot(record);
        }
        let authority = self.require_workflow(&store, &auth, &record)?;
        let intent = record
            .interrupt_intents
            .iter()
            .find(|i| i.request == interrupted_request)
            .context("original interrupt provenance missing")?
            .clone();
        intent.validate(
            record
                .request_receipt(&interrupted_request, DispatchOperation::Interrupt)
                .context("original interrupt receipt missing")?,
        )?;
        let workflow = store
            .workflow(&record.identity.workflow)?
            .context("workflow missing")?;
        if workflow.authority != intent.authority
            && !workflow
                .transfers
                .iter()
                .any(|transfer| transfer.previous == intent.authority)
        {
            bail!("original interrupt authority is absent from workflow history");
        }
        let baseline = intent
            .baseline
            .context("original interrupt was refused without a barrier")?;
        if intent.control_epoch != epoch
            || record.control.epoch != epoch
            || record.control.barrier
                != Some(ControlBarrier::Interrupt {
                    request: interrupted_request.clone(),
                })
        {
            bail!("confirmation does not own the exact original interrupt barrier");
        }
        let child = self.assignment_session(&record)?;
        let parent = self.session_for_control(auth.session)?;
        let model = expected_native_model(&record)?;
        let captured = record.control.clone();
        child
            .with_delegated_context(
                &DelegatedInput {
                    request: &request,
                    parent: &parent,
                    parent_binding: &authority.binding,
                    parent_producer: Some(&authority.producer),
                    parent_source: auth.source.as_ref(),
                    capability: &auth.capability,
                    child_binding: &baseline.binding,
                    model: &model,
                    text: "",
                    control: &captured,
                    control_epoch: epoch,
                    authority: DelegatedAuthority::Interrupt,
                },
                |agent| {
                    let current = self.control_evidence(&record, agent)?;
                    if current != observed {
                        bail!("interrupt reconciliation evidence changed");
                    }
                    baseline.confirm_settlement(
                        &self.server_lifetime,
                        &record.identity,
                        record.result_version,
                        agent,
                    )?;
                    record.control.epoch = epoch
                        .checked_add(1)
                        .context("child control epoch exhausted")?;
                    record.control.barrier = None;
                    // Other receipts and attribution flags deliberately survive confirmation.
                    record.record_control(
                        &request,
                        DispatchOperation::ConfirmInterrupt,
                        &payload,
                    )?;
                    store.save_assignment(&record)
                },
            )?
            .context("current controller or original child binding changed before confirmation")?;
        drop(store);
        super::super::dispatch::publish_session_changed(self, child.id());
        self.reconciliation_snapshot(record)
    }
}
