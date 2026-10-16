//! Immutable parent assertions on the existing durable workflow owner.
use super::super::delegation_recovery::WorkflowRecord;
use super::*;
use crate::retained::SessionStore;

impl WorkflowRecord {
    pub(crate) fn delivery_head(&self, handle: &str) -> Option<&RecordedDelivery> {
        self.deliveries.iter().rev().find(|delivery| {
            delivery
                .declaration
                .resources
                .iter()
                .any(|resource| resource.handle == handle)
        })
    }

    pub(crate) fn validate_deliveries(&self) -> Result<()> {
        if self.deliveries.len() > MAX_DELIVERIES {
            bail!("delivery history exceeds bounds");
        }
        let mut heads = std::collections::HashMap::new();
        let mut requests = std::collections::HashSet::new();
        for delivery in &self.deliveries {
            delivery
                .declaration
                .validate()
                .map_err(anyhow::Error::msg)?;
            super::cancellation::CancellationAuthority::validate_capture(&delivery.authority)?;
            if !requests.insert(&delivery.reference.request)
                || delivery.reference.request != delivery.declaration.request
                || delivery.reference.digest != digest(&serde_json::to_vec(&delivery.declaration)?)
                || delivery.declaration.workflow != self.authority.workflow
                || delivery.authority.instance != self.authority.instance
                || delivery.authority.workflow != self.authority.workflow
                || delivery.authority.controller != self.authority.controller
                || (delivery.authority != self.authority
                    && !self
                        .transfers
                        .iter()
                        .any(|t| t.previous == delivery.authority))
                || self
                    .transfers
                    .iter()
                    .any(|t| t.request == delivery.reference.request)
                || self
                    .cancellation
                    .as_ref()
                    .is_some_and(|c| c.receipt.request == delivery.reference.request)
            {
                bail!("delivery provenance or request history changed");
            }
            for resource in &delivery.declaration.resources {
                if resource.identity.instance != self.authority.instance
                    || resource.identity.controller != self.authority.controller
                    || resource.previous.as_ref() != heads.get(resource.handle.as_str()).copied()
                {
                    bail!("delivery resource head or membership changed");
                }
                heads.insert(resource.handle.as_str(), &delivery.reference);
            }
        }
        Ok(())
    }

    pub(crate) fn validate_archive_provenance(&self, record: &Record) -> Result<()> {
        for receipt in &record.archive_receipts {
            let authority = &receipt.intent.authority;
            if *authority != self.authority
                && !self.transfers.iter().any(|t| t.previous == *authority)
            {
                bail!("archive receipt lacks its original workflow authority provenance");
            }
        }
        Ok(())
    }

    /// Latest head is scoped per resource; Keep never expires implicitly.
    pub(crate) fn archive_eligibility(
        &self,
        record: &Record,
        request: &ArchiveRequest,
    ) -> std::result::Result<(), ArchiveRefusal> {
        let delivery = self
            .delivery_head(&record.handle)
            .ok_or(ArchiveRefusal::DeliverySuperseded)?;
        if delivery.reference != request.delivery {
            return Err(ArchiveRefusal::DeliverySuperseded);
        }
        let resource = delivery
            .declaration
            .resources
            .iter()
            .find(|r| r.handle == record.handle)
            .ok_or(ArchiveRefusal::DeliverySuperseded)?;
        if resource.identity != request.expected_identity
            || record.identity != request.expected_identity
            || resource.result_version != request.expected_result_version
            || record.result_version != request.expected_result_version
        {
            return Err(ArchiveRefusal::StaleTarget);
        }
        if resource.obligation_revision != request.expected_obligation_revision
            || record.obligation_revision != request.expected_obligation_revision
        {
            return Err(ArchiveRefusal::ObligationChanged);
        }
        if !matches!(resource.retention, RetentionChoice::ArchiveEligible) {
            return Err(ArchiveRefusal::RequiredRetention);
        }
        Ok(())
    }
}

impl Record {
    /// Use at every new request admission, before allocating operation capacity.
    pub(crate) fn delivery_request_used(&self, request: &str) -> bool {
        self.handle == request
            || self.receipts.iter().any(|r| r.request == request)
            || self.followups.iter().any(|r| r.request == request)
            || self.control_intents.iter().any(|r| r.request == request)
            || self
                .archive_receipts
                .iter()
                .any(|r| r.intent.request.request == request)
    }
}

impl ServerState {
    /// Dedicated workflow handler: acquire mutation here, outside workflow's broad guard.
    pub(crate) fn declare_delivery(
        &self,
        auth: DelegationAuth,
        declaration: DeliveryDeclaration,
    ) -> Result<WorkflowSnapshot> {
        declaration.validate().map_err(anyhow::Error::msg)?;
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let store = self.retained.lock();
        let parent = self.delegation_auth(&auth)?;
        let (binding, producer) = parent
            .authenticated_workflow_source(&auth)
            .context("delivery requires current workflow source")?;
        let mut workflow = store
            .workflow(&declaration.workflow)?
            .context("workflow missing")?;
        workflow.require(
            &auth,
            parent.run(),
            &binding,
            &producer,
            &self.server_lifetime,
        )?;
        let body_digest = digest(&serde_json::to_vec(&declaration)?);
        if let Some(prior) = workflow
            .deliveries
            .iter()
            .find(|d| d.reference.request == declaration.request)
        {
            if prior.reference.digest != body_digest || prior.declaration != declaration {
                bail!("delivery request already identifies a different declaration");
            }
            drop(store);
            return self.workflow_snapshot(workflow, None);
        }
        if workflow.deliveries.len() >= MAX_DELIVERIES
            || workflow
                .transfers
                .iter()
                .any(|r| r.request == declaration.request)
            || workflow
                .cancellation
                .as_ref()
                .is_some_and(|c| c.receipt.request == declaration.request)
        {
            bail!(
                "delivery request collides or delivery storage is full; original evidence retained"
            );
        }
        let (members, _) = store.workflow_members(&declaration.workflow)?;
        if members
            .iter()
            .any(|r| r.delivery_request_used(&declaration.request))
        {
            bail!("delivery request already identifies an assignment operation");
        }
        for resource in &declaration.resources {
            let record = members
                .iter()
                .find(|r| r.handle == resource.handle)
                .context("delivery resource is not a verified workflow member")?;
            self.validate_delivery_resource(&store, &workflow, record, resource)?;
        }
        workflow.deliveries.push(RecordedDelivery {
            reference: DeliveryRef {
                request: declaration.request.clone(),
                digest: body_digest,
            },
            authority: workflow.authority.clone(),
            declaration,
        });
        // Parent PTY exit can bypass retained; keep its state pinned through commit.
        parent.with_workflow_context(&auth, &workflow.authority, || {
            let transaction = store.connection.unchecked_transaction()?;
            store.save_workflow(&workflow)?;
            transaction.commit()?;
            Ok(())
        })?;
        drop(store);
        self.workflow_snapshot(workflow, None)
    }

    fn validate_delivery_resource(
        &self,
        store: &SessionStore,
        workflow: &WorkflowRecord,
        record: &Record,
        resource: &DeliveryResource,
    ) -> Result<()> {
        if record.identity != resource.identity
            || record.result_version != resource.result_version
            || record.obligation_revision != resource.obligation_revision
        {
            bail!("delivery assignment identity, version or obligations changed");
        }
        if workflow.delivery_head(&record.handle).map(|d| &d.reference)
            != resource.previous.as_ref()
        {
            bail!("delivery resource head changed; declare against the latest scoped head");
        }
        if record.archive_pending().is_some() {
            bail!("resource archive is in flight; its retention declaration cannot change");
        }
        if let Some(id) = record.identity.session {
            let row = store
                .get(id)
                .context("delivery resource retained row missing")?;
            if Some(row.run) != record.identity.run {
                bail!("delivery resource run changed");
            }
        }
        // These existing typed readers verify known Server-owned artifacts. Opaque
        // evidence indices above never become filesystem paths or content proof.
        if let Some(publication) = &resource.publication
            && self
                .assignment_publication(record)
                .map_err(|_| anyhow::anyhow!("delivery publication unavailable"))?
                .as_ref()
                != Some(publication)
        {
            bail!("delivery publication reference changed");
        }
        if let Some(acceptance) = &resource.acceptance {
            if record.acceptance.as_ref() != Some(acceptance) {
                bail!("delivery acceptance reference changed");
            }
            let version = record.version();
            read_version_submission(record, &version)?;
            followups::verify_acceptance(record, &version)?;
            // Acceptance was verified and privately published by RecordAcceptance;
            // compare its stored reference rather than reading a parent path.
        }
        Ok(())
    }
}
