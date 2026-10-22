//! Atomic workflow closure and its fixed child effects.
use super::super::delegation_recovery::{WorkflowRecord, hash};
use super::cancellation::*;
use super::*;

impl ServerState {
    pub(in crate::server) fn require_dispatch_open(
        &self,
        store: &crate::retained::SessionStore,
        workflow: &str,
    ) -> Result<()> {
        if let Some(row) = store.workflow(workflow)? {
            require_dispatch_open(row.cancellation.as_ref()).map_err(|_| {
                lifecycle_error(
                    ErrorCode::Conflict,
                    "workflow is cancelled; new and resumed dispatch are closed",
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn cancellation_view(
        &self,
        capture: &WorkflowCancellation<WorkflowAuthority>,
    ) -> Result<WorkflowCancellationView> {
        capture.validate()?;
        let store = self.retained.lock();
        let mut targets = Vec::new();
        for target in &capture.targets {
            let record = if target.refusal.is_some() {
                None
            } else {
                match store.assignment(&target.handle) {
                    Ok(record) => record,
                    Err(error) if error.downcast_ref::<rusqlite::Error>().is_some() => {
                        return Err(error);
                    }
                    Err(_) => None,
                }
            };
            let receipt = record.and_then(|record| {
                record.receipts.into_iter().find(|r| {
                    r.request == target.interrupt_request
                        && r.operation == DispatchOperation::Interrupt
                        && r.identity == target.identity
                        && r.result_version == target.result_version
                })
            });
            targets.push(WorkflowCancellationTarget {
                handle: target.handle.clone(),
                identity: target.identity.clone(),
                result_version: target.result_version,
                control_epoch: target.control_epoch,
                interrupt_request: target.interrupt_request.clone(),
                outcome: if target.refusal.is_some() {
                    DispatchOutcome::Rejected
                } else {
                    receipt
                        .as_ref()
                        .map_or(DispatchOutcome::Unknown, |r| r.outcome)
                },
                reason: target
                    .refusal
                    .clone()
                    .or_else(|| receipt.and_then(|r| r.reason)),
            });
        }
        let mut gaps = capture.gaps.clone();
        if capture.unverifiable_members > 0 {
            gaps.push(format!(
                "{} fixed retained members remain unverifiable; no effects or adoption authorized",
                capture.unverifiable_members
            ));
        }
        Ok(WorkflowCancellationView {
            request: capture.receipt.request.clone(),
            captured_authority: capture.captured_authority.clone(),
            dispatch_closure: DispatchOutcome::Applied,
            target_outcome: if capture.unverifiable_members > 0 {
                DispatchOutcome::Unknown
            } else {
                aggregate_cancellation(targets.iter().map(|t| t.outcome))
            },
            targets,
            gaps,
            unverifiable_members: capture.unverifiable_members,
        })
    }

    pub(crate) fn cancel_workflow(
        &self,
        auth: DelegationAuth,
        workflow: String,
        request: String,
    ) -> Result<Response> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let parent = self.delegation_auth(&auth)?;
        let store = self.retained.lock();
        let mut row: WorkflowRecord = store.workflow(&workflow)?.context("workflow missing")?;
        let (binding, producer) = parent
            .authenticated_workflow_source(&auth)
            .context("workflow source retired")?;
        row.require(
            &auth,
            parent.run(),
            &binding,
            &producer,
            &self.server_lifetime,
        )?;
        if let Some(capture) = &row.cancellation {
            capture.validate()?;
            if capture.receipt.request != request {
                bail!("workflow already cancelled by another fixed request");
            }
            // Authenticate today's owner, but return the original capture without rediscovery.
            drop(store);
            return Ok(Response::Workflows(vec![
                self.workflow_snapshot(row, None)?,
            ]));
        }
        let (mut records, gaps) = store.workflow_members(&workflow)?;
        let count: u32 = store.connection.query_row(
            "SELECT count(*) FROM delegations WHERE workflow = ?1",
            [&workflow],
            |r| r.get(0),
        )?;
        let unverifiable = count
            .checked_sub(records.len() as u32)
            .context("workflow membership count changed")?;
        let digest = hash(&serde_json::to_vec(&(&request, &row.authority))?);
        let mut requests: Vec<&str> = row.transfers.iter().map(|r| r.request.as_str()).collect();
        requests.extend(row.deliveries.iter().map(|d| d.reference.request.as_str()));
        requests.extend(records.iter().flat_map(|r| {
            r.archive_receipts
                .iter()
                .map(|r| r.intent.request.request.as_str())
        }));
        requests.extend(
            records
                .iter()
                .flat_map(|r| r.receipts.iter().map(|r| r.request.as_str())),
        );
        let targets = records
            .iter()
            .map(|record| CancellationTarget {
                handle: record.handle.clone(),
                identity: record.identity.clone(),
                result_version: record.result_version,
                control_epoch: record.control.epoch,
                interrupt_request: format!(
                    "cancel-{}",
                    hash(
                        &serde_json::to_vec(&(&request, &record.handle))
                            .expect("identifiers serialize")
                    )
                ),
                refusal: None,
            })
            .collect::<Vec<_>>();
        let admission = admit_cancellation(
            &mut row.cancellation,
            &row.authority,
            CancellationRequest {
                request: &request,
                digest: &digest,
                expected_authority: &row.authority,
            },
            &requests,
            || Ok(targets),
        )?;
        let mut targets = admission.capture().targets.clone();
        drop(requests);
        let capture = row
            .cancellation
            .as_mut()
            .context("cancel capture missing")?;
        capture.gaps = gaps;
        capture.unverifiable_members = unverifiable;
        capture.validate()?;
        // Reject a full row before even preparing child transitions. No truncation
        // or alternate store is permitted, including after many parent transfers.
        if serde_json::to_vec(&row)?.len() > 262_144 {
            bail!("workflow cancellation capture exceeds durable row capacity");
        }
        for (record, target) in records.iter_mut().zip(&mut targets) {
            let original = record.clone();
            if self
                .prepare_interrupt(
                    &store,
                    &auth,
                    record,
                    &target.interrupt_request,
                    (
                        &target.identity,
                        target.result_version,
                        target.control_epoch,
                    ),
                )
                .is_err()
            {
                *record = original;
                target.refusal = Some("interrupt intent cannot be durably admitted; original child journal and ownership are preserved without effects".into());
            }
        }
        row.cancellation
            .as_mut()
            .context("cancel capture missing")?
            .targets = targets.clone();
        // Refusals are part of the complete row-size preflight too.
        if serde_json::to_vec(&row)?.len() > 262_144 {
            bail!("workflow cancellation capture exceeds durable row capacity");
        }
        parent.with_workflow_context(&auth, &row.authority, || {
            let transaction = store.connection.unchecked_transaction()?;
            store.save_workflow(&row)?;
            for (record, target) in records.iter().zip(&targets) {
                if target.refusal.is_none() {
                    store.save_assignment(record)?;
                }
            }
            transaction.commit()?;
            Ok(())
        })?;
        // No replay enters this phase, even if a prior response or effect was lost.
        for (record, target) in records.iter_mut().zip(&targets) {
            if target.refusal.is_some() {
                continue;
            }
            self.deliver_interrupt(&store, &auth, record, &target.interrupt_request)?;
        }
        drop(store);
        for record in &records {
            if let Some(id) = record.identity.session {
                super::super::dispatch::publish_session_changed(self, id);
            }
        }
        Ok(Response::Workflows(vec![
            self.workflow_snapshot(row, None)?,
        ]))
    }
}
