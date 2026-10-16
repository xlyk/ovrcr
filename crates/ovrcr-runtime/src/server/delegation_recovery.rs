//! One durable workflow row. Assignment identity and child control never change here.
use super::{ServerState, delegation_files::Directory};
use crate::retained::SessionStore;
use anyhow::{Context, Result, bail};
use ovrcr_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const MAX_WORKFLOWS: usize = 4096;
const MAX_TRANSFERS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum Departure {
    Reload { producer: String },
    Native { reference: ExtensionConversation },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResumeAdmission {
    pub run: SessionRunId,
    pub invocation: String,
    pub reference: ExtensionConversation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowRecord {
    pub schema: u32,
    pub authority: WorkflowAuthority,
    pub departure: Option<Departure>,
    pub admission: Option<ResumeAdmission>,
    pub reopen_run: Option<SessionRunId>,
    pub pointer: Option<ContinuationPointer>,
    pub transfers: Vec<WorkflowTransitionReceipt>,
    #[serde(default)]
    pub cancellation:
        Option<super::delegation::cancellation::WorkflowCancellation<WorkflowAuthority>>,
}

pub(crate) fn hash(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
pub(crate) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl WorkflowRecord {
    pub(crate) fn validate(&self) -> Result<()> {
        let valid_authority = |a: &WorkflowAuthority| {
            a.epoch > 0
                && a.parent.0 > 0
                && a.parent_run.0 > 0
                && a.binding.provider == AgentProvider::Pi
                && a.binding.generation > 0
                && [
                    &a.instance,
                    &a.workflow,
                    &a.controller,
                    &a.producer,
                    &a.server_lifetime,
                    &a.binding.invocation,
                    &a.binding.conversation,
                ]
                .iter()
                .all(|v| identifier(v))
        };
        if let Some(capture) = &self.cancellation {
            capture.validate()?;
            if capture.captured_authority.instance != self.authority.instance
                || capture.captured_authority.workflow != self.authority.workflow
                || capture.captured_authority.controller != self.authority.controller
                || capture.captured_authority.epoch > self.authority.epoch
            {
                bail!("cancel capture provenance changed");
            }
        }
        let a = &self.authority;
        if self.schema != 1 || !valid_authority(a) || self.transfers.len() > MAX_TRANSFERS {
            bail!("unsupported or malformed workflow authority; original records remain retained");
        }
        let mut previous: Option<&WorkflowAuthority> = None;
        let mut requests = std::collections::HashSet::new();
        for receipt in &self.transfers {
            let before = &receipt.previous;
            let after = &receipt.current;
            if !valid_authority(before)
                || !valid_authority(after)
                || !identifier(&receipt.request)
                || !requests.insert(&receipt.request)
                || after.epoch
                    != before
                        .epoch
                        .checked_add(1)
                        .context("saved epoch exhausted")?
                || [&before.instance, &after.instance]
                    .iter()
                    .any(|v| *v != &a.instance)
                || [&before.workflow, &after.workflow]
                    .iter()
                    .any(|v| *v != &a.workflow)
                || [&before.controller, &after.controller]
                    .iter()
                    .any(|v| *v != &a.controller)
                || previous.is_some_and(|prior| prior != before)
                || (previous.is_none() && before.epoch != 1)
            {
                bail!("workflow transition history is conflicting or incomplete");
            }
            let expected = match receipt.reason.as_str() {
                "explicit transfer" => hash(&serde_json::to_vec(&WorkflowTransfer {
                    request: receipt.request.clone(),
                    expected: before.clone(),
                    parent: after.parent,
                    parent_run: after.parent_run,
                    binding: after.binding.clone(),
                    producer: after.producer.clone(),
                })?),
                "orderly reload" | "owned native resume" => {
                    if before.server_lifetime != after.server_lifetime
                        || before.parent != after.parent
                        || before.producer == after.producer
                    {
                        bail!("automatic transition changed parent/lifetime or reused its source");
                    }
                    if receipt.reason == "orderly reload" {
                        if before.parent_run != after.parent_run || before.binding != after.binding
                        {
                            bail!("reload changed its exact managed invocation");
                        }
                    } else if (before.parent_run != after.parent_run
                        && before.parent_run.0.checked_add(1) != Some(after.parent_run.0))
                        || before.binding.invocation == after.binding.invocation
                        || before.binding.conversation != after.binding.conversation
                    {
                        bail!("native resume changed its exact retained parent reference");
                    }
                    hash(&serde_json::to_vec(after)?)
                }
                _ => bail!("unsupported workflow transition evidence"),
            };
            if expected != receipt.digest {
                bail!("workflow transition digest changed");
            }
            previous = Some(after);
        }
        if previous.is_some_and(|last| last != a) || (previous.is_none() && a.epoch != 1) {
            bail!("current workflow authority has no complete transition history");
        }
        if let Some(capture) = &self.cancellation {
            let captured = &capture.captured_authority;
            if (captured != a && !self.transfers.iter().any(|r| &r.previous == captured))
                || self
                    .transfers
                    .iter()
                    .any(|r| r.request == capture.receipt.request)
                || capture.receipt.digest
                    != hash(&serde_json::to_vec(&(&capture.receipt.request, captured))?)
            {
                bail!("workflow cancellation lacks original authenticated capture provenance");
            }
        }
        match &self.departure {
            Some(Departure::Reload { producer }) if producer != &a.producer => {
                bail!("reload departure names another source")
            }
            Some(Departure::Native { reference }) => {
                crate::extension_recovery::validate(AgentProvider::Pi, reference)?;
                if reference.conversation != a.binding.conversation || reference.history.is_none() {
                    bail!("native departure reference changed");
                }
            }
            _ => {}
        }
        if self
            .reopen_run
            .is_some_and(|run| Some(run.0) != a.parent_run.0.checked_add(1))
            || (self.reopen_run.is_some()
                && !matches!(self.departure, Some(Departure::Native { .. })))
        {
            bail!("retained resume successor changed");
        }
        if let Some(admission) = &self.admission
            && (!identifier(&admission.invocation)
                || admission.invocation == a.binding.invocation
                || !(admission.run == a.parent_run || self.reopen_run == Some(admission.run))
                || !matches!(&self.departure, Some(Departure::Native { reference }) if reference == &admission.reference))
        {
            bail!("native continuation admission is conflicting or unproven");
        }
        if let Some(pointer) = &self.pointer
            && (pointer.schema != 1
                || pointer.instance != a.instance
                || pointer.workflow != a.workflow
                || pointer.version == 0
                || pointer.version > 256
                || pointer.digest.len() != 64
                || !pointer
                    .digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            bail!("invalid continuation pointer");
        }
        Ok(())
    }

    pub(crate) fn require(
        &self,
        auth: &DelegationAuth,
        run: SessionRunId,
        binding: &AgentBinding,
        producer: &str,
        lifetime: &str,
    ) -> Result<()> {
        self.validate()?;
        let a = &self.authority;
        let fence_matches = match &auth.controller {
            Some(fence) => fence == &a.fence(),
            // Old callers have no captured controller epoch. Never infer one after transfer.
            None => a.epoch == 1 && self.transfers.is_empty(),
        };
        if !fence_matches
            || a.parent != auth.session
            || a.parent_run != run
            || &a.binding != binding
            || a.producer != producer
            || a.server_lifetime != lifetime
            || self.departure.is_some()
        {
            return Err(super::lifecycle_error(
                ErrorCode::Conflict,
                "workflow controller changed or departed; reconcile the current controller before effects",
            ));
        }
        Ok(())
    }

    /// The caller persists the whole updated row before publishing this decision.
    pub(crate) fn transfer(
        &mut self,
        request: &str,
        digest: &str,
        expected: &WorkflowAuthority,
        next: WorkflowAuthority,
        reason: &str,
    ) -> Result<WorkflowTransitionReceipt> {
        self.validate()?;
        if !identifier(request) {
            bail!("invalid transfer request");
        }
        if self
            .cancellation
            .as_ref()
            .is_some_and(|c| c.receipt.request == request)
        {
            bail!("request already identifies workflow cancellation");
        }
        if let Some(receipt) = self.transfers.iter().find(|r| r.request == request) {
            if receipt.digest != digest {
                bail!("transfer request reused with different arguments");
            }
            return Ok(receipt.clone());
        }
        if &self.authority != expected
            || next.instance != expected.instance
            || next.workflow != expected.workflow
            || next.controller != expected.controller
            || (reason != "explicit transfer" && next.server_lifetime != expected.server_lifetime)
            || next.epoch
                != expected
                    .epoch
                    .checked_add(1)
                    .context("controller epoch exhausted")?
        {
            bail!("workflow transfer expected authority changed");
        }
        if self.transfers.len() == MAX_TRANSFERS {
            bail!("workflow transfer storage is full; original receipts remain retained");
        }
        let receipt = WorkflowTransitionReceipt {
            request: request.into(),
            digest: digest.into(),
            previous: expected.clone(),
            current: next.clone(),
            reason: reason.into(),
        };
        self.authority = next;
        self.departure = None;
        self.admission = None;
        self.reopen_run = None;
        self.transfers.push(receipt.clone());
        Ok(receipt)
    }
}

/// A bounded inventory, not another owner. Damaged rows still consume capacity.
pub(crate) struct WorkflowInventory {
    pub valid: Vec<WorkflowRecord>,
    pub malformed: Vec<String>,
    pub total: usize,
}

impl SessionStore {
    pub(crate) fn workflow(&self, workflow: &str) -> Result<Option<WorkflowRecord>> {
        use rusqlite::OptionalExtension;
        let raw: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT substr(CAST(COALESCE(metadata, '') AS BLOB), 1, 262145) FROM delegation_workflows WHERE workflow = ?1",
                [workflow],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|raw| {
            if raw.len() > 262_144 {
                bail!(
                    "workflow metadata exceeds supported bound; original evidence remains retained"
                );
            }
            let row: WorkflowRecord = serde_json::from_slice(&raw).map_err(|_| {
                anyhow::anyhow!(
                    "workflow authority metadata is malformed; original evidence remains retained"
                )
            })?;
            row.validate()?;
            if row.authority.workflow != workflow {
                bail!("workflow key changed");
            }
            Ok(row)
        })
        .transpose()
    }
    pub(crate) fn workflows(&self) -> Result<WorkflowInventory> {
        let mut stmt = self
            .connection
            .prepare("SELECT workflow FROM delegation_workflows ORDER BY workflow LIMIT 4097")?;
        let keys: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        if keys.len() > MAX_WORKFLOWS {
            bail!("workflow storage exceeds supported capacity");
        }
        let mut inventory = WorkflowInventory {
            valid: Vec::new(),
            malformed: Vec::new(),
            total: keys.len(),
        };
        for key in keys {
            match self.workflow(&key) {
                Ok(Some(row)) => inventory.valid.push(row),
                Ok(None) => bail!("workflow disappeared during bounded discovery"),
                Err(error) if error.downcast_ref::<rusqlite::Error>().is_some() => {
                    return Err(error);
                }
                Err(_) => inventory.malformed.push(key),
            }
        }
        Ok(inventory)
    }
    pub(crate) fn save_workflow(&self, record: &WorkflowRecord) -> Result<()> {
        record.validate()?;
        let bytes = serde_json::to_string(record)?;
        if bytes.len() > 262_144 {
            bail!("workflow metadata storage is full; original evidence remains retained");
        }
        self.connection.execute("INSERT INTO delegation_workflows (workflow, metadata) VALUES (?1,?2) ON CONFLICT(workflow) DO UPDATE SET metadata=excluded.metadata", rusqlite::params![record.authority.workflow, bytes])?;
        Ok(())
    }
    pub(crate) fn workflow_members(
        &self,
        workflow: &str,
    ) -> Result<(Vec<super::delegation::Record>, Vec<String>)> {
        let mut stmt = self.connection.prepare(
            "SELECT handle FROM delegations WHERE workflow = ?1 ORDER BY handle LIMIT 4097",
        )?;
        let handles: Vec<String> = stmt
            .query_map([workflow], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        if handles.len() > 4096 {
            bail!("workflow membership exceeds supported capacity");
        }
        let instance = self.delegation_instance()?;
        let mut members = Vec::new();
        let mut gaps = Vec::new();
        let mut extra = 0;
        for handle in handles {
            let row = match self.assignment(&handle) {
                Ok(row) => row,
                Err(error) if error.downcast_ref::<rusqlite::Error>().is_some() => {
                    return Err(error);
                }
                Err(_) => None,
            };
            if let Some(row) = row.filter(|row| {
                identifier(&handle)
                    && row.handle == handle
                    && row.identity.request == handle
                    && row.identity.instance == instance
                    && row.identity.workflow == workflow
                    && [&row.identity.controller, &row.identity.task]
                        .iter()
                        .all(|id| identifier(id))
                    && row.identity.controller_epoch > 0
                    && row.identity.attempt > 0
                    && row.identity.contract > 0
                    && row.result_version > 0
                    && row.control.epoch > 0
                    && row.identity.session.is_some() == row.identity.run.is_some()
                    && row.identity.session.is_none_or(|id| id.0 > 0)
                    && row.identity.run.is_none_or(|run| run.0 > 0)
            }) {
                members.push(row);
            } else if gaps.len() < 64 {
                let label = if identifier(&handle) {
                    handle.as_str()
                } else {
                    "retained member"
                };
                gaps.push(format!("{label}: assignment metadata is missing or unverifiable; original evidence remains read-only and effects require direct validation"));
            } else {
                extra += 1;
            }
        }
        if extra > 0 {
            gaps.push(format!("{extra} additional retained members have unverifiable metadata; original evidence remains read-only"));
        }
        Ok((members, gaps))
    }

    fn workflow_recovery_gaps(
        &self,
        parent: Option<(SessionId, SessionRunId, &AgentBinding)>,
        workflow: Option<&str>,
        inventory: &WorkflowInventory,
    ) -> Result<Vec<WorkflowRecoveryGap>> {
        // CASE prevents json_extract from evaluating invalid JSON. Scope before
        // decoding: an unrelated malformed assignment is never a caller's gap.
        let mut query = self.connection.prepare("SELECT handle, workflow FROM delegations WHERE (?1 IS NULL OR workflow = ?1) AND (?2 IS NULL OR CASE WHEN json_valid(metadata) THEN json_extract(metadata, '$.parent') = ?2 AND json_extract(metadata, '$.parent_run') = ?3 AND json_extract(metadata, '$.parent_binding.provider') = 'Pi' AND json_extract(metadata, '$.parent_binding.invocation') = ?4 AND json_extract(metadata, '$.parent_binding.conversation') = ?5 AND json_extract(metadata, '$.parent_binding.generation') = ?6 ELSE 0 END) ORDER BY handle LIMIT 4097")?;
        let rows: Vec<(String, String)> = query
            .query_map(
                rusqlite::params![
                    workflow,
                    parent.map(|p| i64::try_from(p.0.0)).transpose()?,
                    parent.map(|p| i64::try_from(p.1.0)).transpose()?,
                    parent.map(|p| p.2.invocation.as_str()),
                    parent.map(|p| p.2.conversation.as_str()),
                    parent.map(|p| i64::try_from(p.2.generation)).transpose()?
                ],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?
            .collect::<std::result::Result<_, _>>()?;
        if rows.len() > 4096 {
            bail!("retained obligations exceed supported record capacity");
        }
        let associated: std::collections::HashSet<_> =
            rows.iter().map(|(_, key)| key.clone()).collect();
        let instance = self.delegation_instance()?;
        let mut gaps = Vec::new();
        let mut extra = 0;
        for (handle, key) in rows {
            if inventory
                .valid
                .iter()
                .any(|row| row.authority.workflow == key)
            {
                continue;
            }
            if gaps.len() == 64 {
                extra += 1;
                continue;
            }
            let decoded = match self.assignment(&handle) {
                Ok(row) => row,
                Err(error) if error.downcast_ref::<rusqlite::Error>().is_some() => {
                    return Err(error);
                }
                Err(_) => None,
            };
            let row = decoded.filter(|row| {
                row.handle == handle
                    && row.identity.request == handle
                    && row.identity.workflow == key
                    && row.identity.instance == instance
                    && [
                        &row.identity.instance,
                        &row.identity.workflow,
                        &row.identity.controller,
                        &row.identity.task,
                        &row.identity.request,
                    ]
                    .into_iter()
                    .all(|id| identifier(id))
                    && row.identity.controller_epoch > 0
                    && row.identity.attempt > 0
                    && row.identity.contract > 0
                    && parent.is_none_or(|(id, run, binding)| {
                        row.parent == id && row.parent_run == run && &row.parent_binding == binding
                    })
            });
            let damaged = inventory.malformed.contains(&key);
            let reason = if row.is_none() {
                "Retained record has unverifiable metadata. Original evidence remains read-only; automatic recovery and transfer are unavailable."
            } else if damaged {
                "Retained workflow authority is malformed or unsupported. Original obligations and receipts remain read-only; automatic recovery and transfer are unavailable."
            } else {
                "Retained workflow lacks certified recovery authority. Its original obligations and receipts remain read-only; automatic recovery and transfer are unavailable for this older record."
            };
            let trusted_keys = identifier(&key) && identifier(&handle);
            gaps.push(WorkflowRecoveryGap {
                workflow: trusted_keys.then_some(key),
                handle: trusted_keys.then_some(handle),
                identity: row.as_ref().map(|row| row.identity.clone()),
                brief: row.as_ref().map(|row| row.root.join("brief-v1.json")),
                result_path: row.as_ref().map(|row| {
                    row.root
                        .join(format!("result-v{}.json", row.result_version))
                }),
                reason: reason.into(),
            });
        }
        if parent.is_none() {
            for key in &inventory.malformed {
                if associated.contains(key) || workflow.is_some_and(|selected| selected != key) {
                    continue;
                }
                if gaps.len() == 64 {
                    extra += 1;
                    continue;
                }
                gaps.push(WorkflowRecoveryGap { workflow: identifier(key).then(||key.clone()), handle: None, identity: None, brief: None, result_path: None, reason: "Retained authority record has unverifiable metadata and no trustworthy assignment attribution. Original evidence remains read-only; automatic recovery and transfer are unavailable.".into() });
            }
        }
        if extra > 0 {
            gaps.push(WorkflowRecoveryGap { workflow: None, handle: None, identity: None, brief: None, result_path: None, reason: format!("{extra} additional retained records have recovery gaps; original evidence remains read-only. Select an explicit workflow for scoped inspection.") });
        }
        Ok(gaps)
    }

    /// Called inside authenticated reservation admission, before publishing its lease.
    pub(crate) fn admit_workflow_resume(
        &self,
        session: SessionId,
        run: SessionRunId,
        reserve: &ReserveAgent,
        lifetime: &str,
    ) -> Result<()> {
        if reserve.provider != AgentProvider::Pi {
            return Ok(());
        }
        let Some(reference) = reserve.pi_resume.as_deref() else {
            return Ok(());
        };
        crate::extension_recovery::validate(AgentProvider::Pi, reference)?;
        let transaction = self.connection.unchecked_transaction()?;
        for mut row in self.workflows()?.valid {
            let a = &row.authority;
            if a.parent != session || a.server_lifetime != lifetime {
                continue;
            }
            let Some(Departure::Native {
                reference: original,
            }) = &row.departure
            else {
                continue;
            };
            if original != reference || !(a.parent_run == run || row.reopen_run == Some(run)) {
                continue;
            }
            let admission = ResumeAdmission {
                run,
                invocation: reserve.invocation.clone(),
                reference: reference.clone(),
            };
            if row.admission.as_ref().is_some_and(|old| old != &admission) {
                bail!(
                    "native continuation already admitted to another invocation; explicit transfer required"
                );
            }
            row.admission = Some(admission);
            self.save_workflow(&row)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// This only records an existing caller's exact retained resume intent; it never launches.
    pub(crate) fn record_workflow_reopen(
        &self,
        id: SessionId,
        old: SessionRunId,
        reference: &ConversationReference,
        lifetime: &str,
    ) -> Result<()> {
        let ConversationReference::Pi(reference) = reference else {
            return Ok(());
        };
        let reference = crate::extension_recovery::canonical_pi_reference(reference)?;
        let transaction = self.connection.unchecked_transaction()?;
        for mut row in self.workflows()?.valid {
            if row.authority.parent == id
                && row.authority.parent_run == old
                && row.authority.server_lifetime == lifetime
                && matches!(&row.departure, Some(Departure::Native { reference: original }) if original == &reference)
            {
                row.reopen_run = Some(SessionRunId(
                    old.0.checked_add(1).context("run identity exhausted")?,
                ));
                self.save_workflow(&row)?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Session validates the lease, binding and Producer while this retained lock stays held.
    pub(crate) fn workflow_source(
        &self,
        id: SessionId,
        run: SessionRunId,
        binding: &AgentBinding,
        producer: &str,
        reason: &str,
        lifetime: &str,
    ) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        for mut row in self.workflows()?.valid {
            let old = row.authority.clone();
            if old.parent != id || old.server_lifetime != lifetime {
                continue;
            }
            if old.parent_run == run && old.binding == *binding && old.producer == producer {
                continue;
            }
            let eligible = match &row.departure {
                Some(Departure::Reload { producer: previous }) => {
                    reason == "reload"
                        && previous == &old.producer
                        && old.parent_run == run
                        && old.binding == *binding
                        && previous != producer
                }
                Some(Departure::Native { reference }) => {
                    row.admission.as_ref().is_some_and(|admission| {
                        admission.run == run
                            && admission.invocation == binding.invocation
                            && &admission.reference == reference
                            && reference.conversation == binding.conversation
                            && (old.parent_run == run || row.reopen_run == Some(run))
                    })
                }
                None => false,
            };
            if !eligible {
                continue;
            } // New/forked sources may report, but acquire no workflow.
            let next = WorkflowAuthority {
                parent_run: run,
                binding: binding.clone(),
                producer: producer.into(),
                epoch: old
                    .epoch
                    .checked_add(1)
                    .context("controller epoch exhausted")?,
                ..old.clone()
            };
            let request = format!("source-{producer}");
            let digest = hash(&serde_json::to_vec(&next)?);
            row.transfer(
                &request,
                &digest,
                &old,
                next,
                if reason == "reload" {
                    "orderly reload"
                } else {
                    "owned native resume"
                },
            )?;
            self.save_workflow(&row)?;
        }
        transaction.commit()?;
        Ok(())
    }
    pub(crate) fn workflow_departure(
        &self,
        id: SessionId,
        run: SessionRunId,
        binding: &AgentBinding,
        producer: &str,
        reaped: bool,
        lifetime: &str,
    ) -> Result<()> {
        let reference = self.get(id).and_then(|r| r.conversation.clone());
        let transaction = self.connection.unchecked_transaction()?;
        for mut row in self.workflows()?.valid {
            if row.authority.parent != id
                || row.authority.parent_run != run
                || row.authority.binding != *binding
                || row.authority.producer != producer
                || row.authority.server_lifetime != lifetime
            {
                continue;
            }
            let departure = if reaped {
                let Some(ConversationReference::Pi(reference)) = reference.clone() else {
                    continue;
                };
                if reference.conversation != binding.conversation || reference.history.is_none() {
                    continue;
                }
                let reference = crate::extension_recovery::canonical_pi_reference(&reference)?;
                Departure::Native { reference }
            } else {
                Departure::Reload {
                    producer: producer.into(),
                }
            };
            // Native completion supersedes a pending reload only when the same owner reaped.
            row.departure = Some(departure);
            self.save_workflow(&row)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

impl ServerState {
    pub(crate) fn current_workflow_authority(
        &self,
        auth: &DelegationAuth,
        workflow: &str,
    ) -> Result<WorkflowAuthority> {
        let parent = self.delegation_auth(auth)?;
        let store = self.retained.lock();
        let (binding, producer) = parent
            .authenticated_workflow_source(auth)
            .context("workflow source capability is missing or retired")?;
        let record = store.workflow(workflow)?.context("workflow lacks certified recovery authority; original obligations remain read-only and transfer is unavailable")?;
        record.require(
            auth,
            parent.run(),
            &binding,
            &producer,
            &self.server_lifetime,
        )?;
        Ok(record.authority)
    }
    pub(super) fn require_workflow(
        &self,
        store: &SessionStore,
        auth: &DelegationAuth,
        record: &super::delegation::Record,
    ) -> Result<WorkflowAuthority> {
        let parent = self.delegation_auth(auth)?;
        let (binding, producer) = parent
            .authenticated_workflow_source(auth)
            .context("workflow source capability is missing or retired")?;
        let row = store
            .workflow(&record.identity.workflow)?
            .context("workflow authority is missing")?;
        row.require(
            auth,
            parent.run(),
            &binding,
            &producer,
            &self.server_lifetime,
        )?;
        Ok(row.authority)
    }
    pub(crate) fn workflow_snapshot(
        &self,
        row: WorkflowRecord,
        receipt: Option<WorkflowTransitionReceipt>,
    ) -> Result<WorkflowSnapshot> {
        let (members, mut gaps) = self
            .retained
            .lock()
            .workflow_members(&row.authority.workflow)?;
        let handles = members.iter().map(|record| record.handle.clone()).collect();
        let active =
            row.authority.server_lifetime == self.server_lifetime && row.departure.is_none();
        if !active {
            gaps.push("workflow owner departed or Server lifetime changed; explicit transfer may be required".into());
        }
        for record in &members {
            let handle = &record.handle;
            if let (Some(id), Some(run)) = (record.identity.session, record.identity.run)
                && !self
                    .session_summary(id)
                    .is_some_and(|s| s.run == run && s.phase.is_live())
            {
                gaps.push(format!(
                    "{handle}: child run is stopped or missing; left stopped"
                ));
            }
        }
        let continuation = row.pointer.as_ref().and_then(|pointer| {
            let read = (|| -> Result<ContinuationIndex> {
                let directory = Directory::open(&std::fs::canonicalize(&self.registry_path)?.join("workflow-continuations").join(&row.authority.workflow), false)?;
                let bytes = directory.read(&format!("index-v{}.json", pointer.version))?;
                if bytes.len() > 65536 || hash(&bytes) != pointer.digest { bail!("continuation bytes changed"); }
                let index: ContinuationIndex = serde_json::from_slice(&bytes)?;
                index.validate().map_err(anyhow::Error::msg)?;
                Ok(index)
            })();
            match read { Ok(index) => Some(index), Err(_) => { gaps.push("continuation publication is missing, malformed or conflicting; original obligations remain retained".into()); None } }
        });
        let cancellation = row
            .cancellation
            .as_ref()
            .map(|capture| self.cancellation_view(capture))
            .transpose()?;
        Ok(WorkflowSnapshot {
            authority: row.authority,
            active,
            handles,
            pointer: row.pointer,
            continuation,
            gaps,
            receipt,
            cancellation,
        })
    }
    pub(crate) fn workflow_request(&self, request: WorkflowRequest) -> Result<Response> {
        match request {
            WorkflowRequest::Cancel {
                auth,
                workflow,
                request,
            } => self.cancel_workflow(auth, workflow, request),
            WorkflowRequest::Parents { auth } => {
                let parent = self.delegation_auth(&auth)?;
                let sessions: Vec<_> = self.sessions.lock().unwrap().values().cloned().collect();
                let _store = self.retained.lock();
                parent
                    .authenticated_workflow_source(&auth)
                    .context("workflow source capability is missing or retired")?;
                let mut targets = Vec::new();
                for session in sessions {
                    if let Some((binding, producer)) = session.workflow_source()
                        && binding.provider == AgentProvider::Pi
                    {
                        targets.push(WorkflowParent {
                            session: session.id(),
                            run: session.run(),
                            binding,
                            producer,
                        });
                    }
                }
                targets.sort_by_key(|target| target.session.0);
                Ok(Response::WorkflowParents(targets))
            }
            WorkflowRequest::Recover { auth, workflow } => {
                let parent = self.delegation_auth(&auth)?;
                let store = self.retained.lock();
                let (binding, producer) = parent
                    .authenticated_workflow_source(&auth)
                    .context("workflow source capability is missing or retired")?;
                let inventory = store.workflows()?;
                let gaps = store.workflow_recovery_gaps(
                    Some((auth.session, parent.run(), &binding)),
                    workflow.as_deref(),
                    &inventory,
                )?;
                let rows = inventory.valid;
                let mut selected = Vec::new();
                for row in rows {
                    if workflow
                        .as_ref()
                        .is_some_and(|id| id != &row.authority.workflow)
                    {
                        continue;
                    }
                    let mut lookup = auth.clone();
                    lookup.controller = Some(row.authority.fence());
                    if row
                        .require(
                            &lookup,
                            parent.run(),
                            &binding,
                            &producer,
                            &self.server_lifetime,
                        )
                        .is_ok()
                    {
                        selected.push(row);
                    } else if workflow.is_some() {
                        bail!(
                            "workflow pointer has no current authority; use explicit owner or Dashboard transfer"
                        );
                    }
                }
                if workflow.is_some() && selected.is_empty() && gaps.is_empty() {
                    bail!("workflow identity is missing; no work was launched or replayed");
                }
                drop(store);
                let output = selected
                    .into_iter()
                    .map(|row| self.workflow_snapshot(row, None))
                    .collect::<Result<Vec<_>>>()?;
                let recovery = WorkflowRecovery {
                    workflows: output,
                    gaps,
                };
                if serde_json::to_vec(&recovery)?.len() > ovrcr_protocol::MAX_FRAME_BYTES - 8192 {
                    bail!(
                        "workflow discovery exceeds the response bound; recover one explicit workflow identity"
                    );
                }
                Ok(Response::WorkflowRecovery(Box::new(recovery)))
            }
            WorkflowRequest::Save {
                auth,
                workflow,
                expected_version,
                index,
            } => {
                index.validate().map_err(anyhow::Error::msg)?;
                let _mutation = self.mutation_lock.lock().unwrap();
                let authority = self.current_workflow_authority(&auth, &workflow)?;
                super::delegation::hold_before_effect("workflow-before-save", &workflow)?;
                let store = self.retained.lock();
                let mut row = store.workflow(&workflow)?.context("workflow disappeared")?;
                let parent = self.delegation_auth(&auth)?;
                let (binding, producer) = parent
                    .authenticated_workflow_source(&auth)
                    .context("continuation source capability is retired")?;
                row.require(
                    &auth,
                    parent.run(),
                    &binding,
                    &producer,
                    &self.server_lifetime,
                )?;
                if row.authority != authority {
                    bail!("workflow controller changed");
                }
                let bytes = serde_json::to_vec(&index)?;
                let digest = hash(&bytes);
                if row.pointer.as_ref().is_some_and(|p| {
                    Some(p.version) == expected_version.checked_add(1) && p.digest == digest
                }) {
                    drop(store);
                    return Ok(Response::Workflows(vec![
                        self.workflow_snapshot(row, None)?,
                    ]));
                }
                if row.pointer.as_ref().map_or(0, |p| p.version) != expected_version {
                    bail!("continuation version changed; reconcile the original publication");
                }
                let version = expected_version
                    .checked_add(1)
                    .context("continuation version exhausted")?;
                if version > 256 {
                    bail!(
                        "continuation storage is full; existing records and obligations remain retained"
                    );
                }
                let directory = Directory::open(
                    &std::fs::canonicalize(&self.registry_path)?
                        .join("workflow-continuations")
                        .join(&workflow),
                    true,
                )?;
                let name = format!("index-v{version}.json");
                if let Err(error) = directory.publish(&name, &bytes)
                    && directory.read(&name).ok().as_deref() != Some(bytes.as_slice())
                {
                    return Err(error);
                }
                directory.sync()?;
                row.pointer = Some(ContinuationPointer {
                    schema: 1,
                    instance: authority.instance,
                    workflow,
                    version,
                    digest,
                });
                store.save_workflow(&row)?;
                drop(store);
                Ok(Response::Workflows(vec![
                    self.workflow_snapshot(row, None)?,
                ]))
            }
            WorkflowRequest::Transfer { auth, transfer } => self
                .transfer_workflow(transfer, None, Some(&auth))
                .map(|s| Response::Workflows(vec![s])),
        }
    }
    pub(crate) fn workflow_candidates(
        &self,
        owner: &std::sync::Arc<()>,
        parent: SessionId,
        run: SessionRunId,
    ) -> Result<WorkflowCandidates> {
        if !self.dashboard.owns(owner) {
            bail!("workflow recovery requires the current Dashboard");
        }
        let session = self.session_for_control(parent)?;
        if session.run() != run {
            bail!("proposed parent run changed");
        }
        let (binding, producer) = session
            .workflow_source()
            .context("proposed Pi source is not certified")?;
        let store = self.retained.lock();
        let inventory = store.workflows()?;
        let legacy = store.workflow_recovery_gaps(None, None, &inventory)?;
        let mut candidates = Vec::new();
        for row in inventory.valid {
            if row.authority.parent == parent
                && row.authority.parent_run == run
                && row.authority.binding == binding
                && row.authority.producer == producer
                && row.departure.is_none()
                && row.authority.server_lifetime == self.server_lifetime
            {
                continue;
            }
            if candidates.len() == 64 {
                bail!("more than 64 recoverable workflows; narrow recovery is required");
            }
            candidates.push(WorkflowTransfer {
                request: crate::agent_runner::private_identifier()?,
                expected: row.authority,
                parent,
                parent_run: run,
                binding: binding.clone(),
                producer: producer.clone(),
            });
        }
        let output = WorkflowCandidates {
            candidates,
            gaps: legacy,
        };
        if serde_json::to_vec(&output)?.len() > MAX_FRAME_BYTES - 8192 {
            bail!(
                "workflow recovery choices exceed response bound; original records remain retained"
            );
        }
        Ok(output)
    }
    pub(crate) fn transfer_workflow(
        &self,
        transfer: WorkflowTransfer,
        owner: Option<&std::sync::Arc<()>>,
        auth: Option<&DelegationAuth>,
    ) -> Result<WorkflowSnapshot> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let store = self.retained.lock();
        let mut row = store
            .workflow(&transfer.expected.workflow)?
            .context("workflow missing")?;
        let digest = hash(&serde_json::to_vec(&transfer)?);
        if let Some(receipt) = row
            .transfers
            .iter()
            .find(|receipt| receipt.request == transfer.request)
            .cloned()
        {
            if receipt.digest != digest || receipt.previous != transfer.expected {
                bail!("transfer request reused with different arguments");
            }
            if let Some(auth) = auth {
                let session = self.delegation_auth(auth)?;
                if !session.authorizes_workflow_source(auth.source.as_ref()) {
                    bail!("transfer receipt source capability is retired");
                }
                let (binding, producer) = session
                    .workflow_source()
                    .context("receipt reader source is not current")?;
                let matches = |authority: &WorkflowAuthority| {
                    authority.parent == auth.session
                        && authority.parent_run == session.run()
                        && authority.binding == binding
                        && authority.producer == producer
                        && authority.server_lifetime == self.server_lifetime
                };
                if !matches(&receipt.previous) && !matches(&row.authority) {
                    bail!("transfer receipt belongs to another principal");
                }
            } else if let Some(owner) = owner {
                self.dashboard
                    .with_workflow_owner(owner, || ())
                    .context("Dashboard owner changed")?;
            } else {
                bail!("transfer receipt requires source or Dashboard authentication");
            }
            drop(store);
            return self.workflow_snapshot(row, Some(receipt));
        }
        let parent = self.session_for_control(transfer.parent)?;
        let (binding, producer) = parent
            .workflow_source()
            .context("proposed Pi source is not certified")?;
        if parent.run() != transfer.parent_run
            || binding != transfer.binding
            || producer != transfer.producer
        {
            bail!("proposed managed parent changed");
        }
        if let Some(auth) = auth {
            let current = self.delegation_auth(auth)?;
            if !current.authorizes_workflow_source(auth.source.as_ref()) {
                bail!("workflow source capability is missing or retired");
            }
            let (binding, producer) = current
                .workflow_source()
                .context("current owner source changed")?;
            row.require(
                auth,
                current.run(),
                &binding,
                &producer,
                &self.server_lifetime,
            )?;
        } else if owner.is_none() {
            bail!("workflow transfer requires current-owner or Dashboard authority");
        }
        let next = WorkflowAuthority {
            parent: transfer.parent,
            parent_run: transfer.parent_run,
            binding,
            producer,
            epoch: transfer
                .expected
                .epoch
                .checked_add(1)
                .context("controller epoch exhausted")?,
            server_lifetime: self.server_lifetime.clone(),
            ..transfer.expected.clone()
        };
        let apply = || -> Result<WorkflowTransitionReceipt> {
            let receipt = row.transfer(
                &transfer.request,
                &digest,
                &transfer.expected,
                next,
                "explicit transfer",
            )?;
            store.save_workflow(&row)?;
            Ok(receipt)
        };
        let receipt = if let Some(owner) = owner {
            self.dashboard
                .with_workflow_owner(owner, apply)
                .context("Dashboard owner changed")??
        } else {
            apply()?
        };
        drop(store);
        self.workflow_snapshot(row, Some(receipt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_capture_survives_transfer_and_rejects_fabricated_historical_authority() {
        use super::super::delegation::cancellation::{CancellationReceipt, WorkflowCancellation};
        let mut row = original();
        let authority = row.authority.clone();
        row.cancellation = Some(WorkflowCancellation {
            receipt: CancellationReceipt {
                request: "cancel".into(),
                digest: hash(&serde_json::to_vec(&("cancel", &authority)).unwrap()),
                dispatch_closed: true,
            },
            captured_authority: authority,
            targets: vec![],
            gaps: vec![],
            unverifiable_members: 0,
        });
        let captured = serde_json::to_vec(&row.cancellation).unwrap();
        transfer(&mut row, "transfer-cancelled", "source-2");
        row.validate().unwrap();
        assert_eq!(serde_json::to_vec(&row.cancellation).unwrap(), captured);
        let store = SessionStore::in_memory();
        store.save_workflow(&row).unwrap();
        let decoded = store.workflow("workflow").unwrap().unwrap();
        assert_eq!(serde_json::to_vec(&decoded.cancellation).unwrap(), captured);
        row.cancellation.as_mut().unwrap().captured_authority.parent = SessionId(999);
        assert!(
            row.validate().is_err(),
            "decoded provenance must be an actual authority in the sole row's history"
        );
    }

    #[test]
    fn complete_cancellation_capture_cannot_overflow_durable_workflow_row_or_replace_evidence() {
        use super::super::delegation::cancellation::{
            CancellationReceipt, CancellationTarget, WorkflowCancellation,
        };
        let mut row = original();
        let store = SessionStore::in_memory();
        store.save_workflow(&row).unwrap();
        let authority = row.authority.clone();
        let targets = (0..1024)
            .map(|index| {
                let handle = format!("{}-{index}", "h".repeat(100));
                CancellationTarget {
                    handle: handle.clone(),
                    identity: AssignmentIdentity {
                        instance: authority.instance.clone(),
                        workflow: authority.workflow.clone(),
                        controller: authority.controller.clone(),
                        controller_epoch: 1,
                        task: "t".repeat(100),
                        attempt: 1,
                        contract: 1,
                        request: handle,
                        session: Some(SessionId(index + 100)),
                        run: Some(SessionRunId(1)),
                    },
                    result_version: 1,
                    control_epoch: 1,
                    interrupt_request: format!("cancel-target-{index}"),
                    refusal: None,
                }
            })
            .collect();
        row.cancellation = Some(WorkflowCancellation {
            receipt: CancellationReceipt {
                request: "cancel".into(),
                digest: hash(&serde_json::to_vec(&("cancel", &authority)).unwrap()),
                dispatch_closed: true,
            },
            captured_authority: authority,
            targets,
            gaps: vec![],
            unverifiable_members: 0,
        });
        row.validate().unwrap();
        assert!(serde_json::to_vec(&row).unwrap().len() > 262_144);
        assert!(store.save_workflow(&row).is_err());
        assert!(
            store
                .workflow("workflow")
                .unwrap()
                .unwrap()
                .cancellation
                .is_none(),
            "oversize preflight leaves original ownership/evidence untouched"
        );
    }
    fn original() -> WorkflowRecord {
        WorkflowRecord {
            schema: 1,
            authority: WorkflowAuthority {
                instance: "instance".into(),
                workflow: "workflow".into(),
                controller: "controller".into(),
                epoch: 1,
                parent: SessionId(7),
                parent_run: SessionRunId(1),
                binding: AgentBinding {
                    provider: AgentProvider::Pi,
                    invocation: "native-1".into(),
                    conversation: "conversation".into(),
                    generation: 1,
                },
                producer: "source-1".into(),
                server_lifetime: "server-1".into(),
            },
            departure: None,
            admission: None,
            reopen_run: None,
            pointer: None,
            transfers: Vec::new(),
            cancellation: None,
        }
    }
    fn transfer(
        row: &mut WorkflowRecord,
        request: &str,
        producer: &str,
    ) -> WorkflowTransitionReceipt {
        let old = row.authority.clone();
        let next = WorkflowAuthority {
            epoch: old.epoch + 1,
            producer: producer.into(),
            ..old.clone()
        };
        let intent = WorkflowTransfer {
            request: request.into(),
            expected: old.clone(),
            parent: next.parent,
            parent_run: next.parent_run,
            binding: next.binding.clone(),
            producer: next.producer.clone(),
        };
        row.transfer(
            request,
            &hash(&serde_json::to_vec(&intent).unwrap()),
            &old,
            next,
            "explicit transfer",
        )
        .unwrap()
    }
    #[test]
    fn complete_history_is_required_and_exact_replay_never_reverts_authority() {
        let mut row = original();
        let first = transfer(&mut row, "transfer-1", "source-2");
        transfer(&mut row, "transfer-2", "source-3");
        let current = row.authority.clone();
        let replay = row
            .transfer(
                &first.request,
                &first.digest,
                &first.previous,
                first.current.clone(),
                "explicit transfer",
            )
            .unwrap();
        assert_eq!(replay, first);
        assert_eq!(row.authority, current);
        row.validate().unwrap();
        let mut missing = row.clone();
        missing.transfers.remove(0);
        assert!(missing.validate().is_err());
        let mut changed = row.clone();
        changed.transfers[0].current.binding.generation += 1;
        assert!(changed.validate().is_err());
        let mut changed = row.clone();
        changed.transfers[1].digest = "0".repeat(64);
        assert!(changed.validate().is_err());
        let mut changed = row;
        changed.authority.epoch += 1;
        assert!(changed.validate().is_err());
    }
    #[test]
    fn invalid_departure_and_admission_never_become_authority() {
        let mut row = original();
        row.departure = Some(Departure::Reload {
            producer: "foreign".into(),
        });
        assert!(row.validate().is_err());
        row.departure = None;
        row.admission = Some(ResumeAdmission {
            run: SessionRunId(1),
            invocation: "native-2".into(),
            reference: ExtensionConversation {
                conversation: "conversation".into(),
                executable: "/fixture/pi".into(),
                history: Some("/fixture/history.jsonl".into()),
                config_dir: "/fixture/profile".into(),
                options: Vec::new(),
            },
        });
        assert!(row.validate().is_err());
        row.admission = None;
        row.reopen_run = Some(SessionRunId(2));
        assert!(row.validate().is_err());
    }
    #[test]
    fn storage_failure_preserves_old_authority_and_malformed_rows_are_not_skipped() {
        let store = SessionStore::in_memory();
        let original = original();
        store.save_workflow(&original).unwrap();
        store.connection.execute_batch("CREATE TRIGGER fail_workflow BEFORE UPDATE ON delegation_workflows BEGIN SELECT RAISE(FAIL, 'fixture storage full'); END;").unwrap();
        let mut next = original.clone();
        transfer(&mut next, "transfer", "source-2");
        assert!(store.save_workflow(&next).is_err());
        assert_eq!(
            store.workflow("workflow").unwrap().unwrap().authority,
            original.authority
        );
        store
            .connection
            .execute_batch(
                "DROP TRIGGER fail_workflow; UPDATE delegation_workflows SET metadata = '{';",
            )
            .unwrap();
        assert!(store.workflow("workflow").is_err());
        let inventory = store.workflows().unwrap();
        assert!(inventory.valid.is_empty());
        assert_eq!(inventory.malformed, ["workflow"]);
        assert_eq!(
            inventory.total, 1,
            "invalid row still occupies retained capacity"
        );
    }
    #[test]
    fn malformed_workflow_inventory_keeps_conservative_full_capacity() {
        let store = SessionStore::in_memory();
        store.connection.execute_batch("WITH RECURSIVE count(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM count WHERE x<4096) INSERT INTO delegation_workflows(workflow,metadata) SELECT 'damaged-'||x,'{' FROM count;").unwrap();
        let inventory = store.workflows().unwrap();
        assert_eq!(inventory.total, MAX_WORKFLOWS);
        assert_eq!(inventory.malformed.len(), MAX_WORKFLOWS);
        assert!(inventory.valid.is_empty());
        let raw: String = store
            .connection
            .query_row(
                "SELECT metadata FROM delegation_workflows WHERE workflow='damaged-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw, "{");
    }

    #[test]
    fn original_missing_epoch_is_never_inherited_after_transfer() {
        let mut row = original();
        let auth = DelegationAuth {
            session: row.authority.parent,
            capability: "fixture".into(),
            controller: None,
            source: None,
        };
        let a = &row.authority;
        row.require(
            &auth,
            a.parent_run,
            &a.binding,
            &a.producer,
            &a.server_lifetime,
        )
        .unwrap();
        transfer(&mut row, "transfer", "source-2");
        let a = &row.authority;
        assert!(
            row.require(
                &auth,
                a.parent_run,
                &a.binding,
                &a.producer,
                &a.server_lifetime
            )
            .is_err()
        );
    }
}
