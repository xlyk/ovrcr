//! Server-owned assignment control. Task payloads live only in private publications.
use super::delegation_files::Directory;
use super::{ServerState, lifecycle_error};
use anyhow::{Context, Result, bail};
use ovrcr_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub handle: String,
    pub identity: AssignmentIdentity,
    pub binding: Option<AgentBinding>,
    pub parent_binding: AgentBinding,
    pub parent: SessionId,
    pub parent_run: SessionRunId,
    pub root: PathBuf,
    pub contract_digest: String,
    pub brief_digest: Option<String>,
    pub result_digest: Option<String>,
    pub dispatch: DispatchStatus,
    pub reason: Option<String>,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub receipts: Vec<DispatchReceipt>,
    #[serde(default)]
    pub request_digests: Vec<(DispatchOperation, String)>,
}

// Three fixed receipt slots per assignment; never evict unresolved obligations.
const MAX_DELEGATION_RECORDS: u64 = 4096;

impl Record {
    fn receipt(&self, operation: DispatchOperation) -> Option<&DispatchReceipt> {
        self.receipts
            .iter()
            .find(|receipt| receipt.operation == operation)
    }

    fn intent(
        &mut self,
        operation: DispatchOperation,
        identity: &AssignmentIdentity,
    ) -> Result<bool> {
        let expected = digest(&serde_json::to_vec(&(
            operation,
            &self.contract_digest,
            identity,
        ))?);
        if let Some((_, saved)) = self
            .request_digests
            .iter()
            .find(|(kind, _)| *kind == operation)
        {
            if saved != &expected {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "operation identity or contract changed",
                ));
            }
            return Ok(self
                .receipt(operation)
                .is_some_and(|r| r.outcome == DispatchOutcome::Accepted));
        }
        if let Some(receipt) = self.receipt(operation) {
            // A legacy receipt without a provable original request is never unsent.
            if &receipt.identity != identity {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "legacy operation identity changed",
                ));
            }
            return Ok(false);
        }
        if &self.identity != identity {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "assignment binding changed",
            ));
        }
        self.request_digests.push((operation, expected));
        self.receipts.push(DispatchReceipt {
            request: self.identity.request.clone(),
            operation,
            identity: identity.clone(),
            outcome: DispatchOutcome::Accepted,
            input: None,
            reason: None,
        });
        Ok(true)
    }

    fn outcome(
        &mut self,
        operation: DispatchOperation,
        outcome: DispatchOutcome,
        input: Option<InputDelivery>,
        reason: Option<&str>,
    ) {
        let receipt = self
            .receipts
            .iter_mut()
            .find(|r| r.operation == operation)
            .expect("durable operation intent");
        receipt.outcome = outcome;
        receipt.input = input;
        receipt.reason = reason.map(str::to_owned);
        self.reason = reason.map(str::to_owned);
    }

    fn upgrade_legacy(&mut self) {
        if !self.receipts.is_empty() {
            return;
        }
        let valid = read_contract(self).ok();
        if let Some(contract) = &valid {
            self.project = contract.project.clone();
            self.workspace = contract.workspace.clone();
        }
        let status = self.dispatch.clone();
        let allocated = self.identity.session.is_some() && self.identity.run.is_some();
        let consistent = match status {
            DispatchStatus::Prepared => {
                self.identity.session.is_none()
                    && self.identity.run.is_none()
                    && self.binding.is_none()
            }
            DispatchStatus::LaunchUnknown => {
                self.identity.session.is_some() == self.identity.run.is_some()
                    && self.binding.is_none()
            }
            DispatchStatus::Launched => allocated && self.binding.is_none(),
            DispatchStatus::InputUnknown | DispatchStatus::Applied => {
                allocated && self.binding.is_some()
            }
            DispatchStatus::Refused => {
                self.identity.session.is_some() == self.identity.run.is_some()
                    && (self.binding.is_none() || allocated)
            }
        };
        let mut prepare_identity = self.identity.clone();
        prepare_identity.session = None;
        prepare_identity.run = None;
        for operation in [
            DispatchOperation::Prepare,
            DispatchOperation::Launch,
            DispatchOperation::InitialInput,
        ] {
            let outcome = if valid.is_none() || !consistent {
                DispatchOutcome::Unknown
            } else {
                match (operation, &status) {
                    (DispatchOperation::Prepare, _) => DispatchOutcome::Applied,
                    (DispatchOperation::Launch, DispatchStatus::Prepared) => {
                        DispatchOutcome::Accepted
                    }
                    (DispatchOperation::Launch, DispatchStatus::LaunchUnknown) => {
                        DispatchOutcome::Unknown
                    }
                    (DispatchOperation::Launch, DispatchStatus::Refused)
                        if self.binding.is_none() =>
                    {
                        DispatchOutcome::Rejected
                    }
                    (DispatchOperation::Launch, _) => DispatchOutcome::Applied,
                    (
                        DispatchOperation::InitialInput,
                        DispatchStatus::Prepared | DispatchStatus::LaunchUnknown,
                    ) => continue,
                    (DispatchOperation::InitialInput, DispatchStatus::Launched)
                        if self.binding.is_none() =>
                    {
                        DispatchOutcome::Accepted
                    }
                    (DispatchOperation::InitialInput, DispatchStatus::Applied) => {
                        DispatchOutcome::Applied
                    }
                    (DispatchOperation::InitialInput, DispatchStatus::Refused)
                        if self.binding.is_some() =>
                    {
                        DispatchOutcome::Rejected
                    }
                    (DispatchOperation::InitialInput, DispatchStatus::Refused) => continue,
                    (DispatchOperation::InitialInput, _) => DispatchOutcome::Unknown,
                }
            };
            let identity = if operation == DispatchOperation::InitialInput {
                self.identity.clone()
            } else {
                prepare_identity.clone()
            };
            let _ = self.intent(operation, &identity);
            // Legacy allocations can differ from their original pre-launch request.
            if self.receipt(operation).is_none() {
                self.receipts.push(DispatchReceipt {
                    request: self.identity.request.clone(),
                    operation,
                    identity,
                    outcome,
                    input: None,
                    reason: Some(if valid.is_some() && consistent {
                        "reconstructed from legacy durable status and validated contract"
                    } else {
                        "legacy dispatch is unclassifiable; preserve the original intent and do not replay"
                    }.into()),
                });
            } else {
                self.outcome(operation, outcome, None, None);
            }
        }
    }
}

/// Only acceptance builds expose deterministic faults, and each rule is consumed once.
pub(crate) fn take_fault(point: &str, handle: &str) -> Option<usize> {
    #[cfg(feature = "acceptance-diagnostics")]
    {
        let path = std::env::var_os("OVRCR_TEST_DELEGATION_FAULT")?;
        let bytes = std::fs::read(&path).ok()?;
        if bytes.len() > 512 {
            return None;
        }
        let value = std::str::from_utf8(&bytes).ok()?;
        let mut parts = value.trim().split('|');
        if parts.next()? != point || parts.next()? != handle {
            return None;
        }
        let count = parts.next()?.parse().ok()?;
        std::fs::remove_file(&path).ok()?;
        let _ = std::fs::write(PathBuf::from(path).with_extension("hit"), point);
        Some(count)
    }
    #[cfg(not(feature = "acceptance-diagnostics"))]
    {
        let _ = (point, handle);
        None
    }
}

fn hold_before_input(handle: &str) -> Result<()> {
    #[cfg(feature = "acceptance-diagnostics")]
    if take_fault("input-before-write", handle).is_some() {
        let release = PathBuf::from(
            std::env::var_os("OVRCR_TEST_DELEGATION_FAULT").context("missing owned fault path")?,
        )
        .with_extension("release");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !release.exists() {
            if Instant::now() >= deadline {
                bail!("owned input barrier was not released");
            }
            std::thread::park_timeout(Duration::from_millis(5));
        }
        std::fs::remove_file(release)?;
    }
    #[cfg(not(feature = "acceptance-diagnostics"))]
    let _ = handle;
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl ServerState {
    fn delegation_auth(
        &self,
        auth: &DelegationAuth,
    ) -> Result<std::sync::Arc<crate::session::Session>> {
        let session = self.session_for_control(auth.session)?;
        if !matches!(session.summary().kind, SessionKind::Agent { ref name } if name == "pi")
            || !session.authorizes_delegation(&auth.capability)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "delegation requires the current managed Pi capability",
            ));
        }
        Ok(session)
    }

    fn owned_assignment(&self, auth: &DelegationAuth, handle: &str) -> Result<Record> {
        let parent = self.delegation_auth(auth)?;
        let mut record = self
            .retained
            .lock()
            .assignment(handle)?
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "assignment not found"))?;
        if record.parent != auth.session
            || record.parent_run != parent.run()
            || parent.summary().agent.as_ref().map(|agent| &agent.binding)
                != Some(&record.parent_binding)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "assignment controller changed",
            ));
        }
        record.upgrade_legacy();
        Ok(record)
    }

    pub(crate) fn reject_assigned_input(&self, id: SessionId) -> Result<()> {
        if self.session_for_control(id)?.has_assigned_input() {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "Pi owns this assignment; generic terminal input is disabled",
            ));
        }
        Ok(())
    }

    pub(crate) fn delegation(&self, request: DelegationRequest) -> Result<AssignmentSnapshot> {
        // Capability discovery is outside lifecycle serialization. A concurrent launch
        // still rechecks its durable status under the lock before creating anything.
        let availability = if let DelegationRequest::Launch { auth, handle, .. } = &request {
            let record = self.owned_assignment(auth, handle)?;
            if record.dispatch == DispatchStatus::Prepared {
                Some(available_model(&read_contract(&record)?).is_ok())
            } else {
                None
            }
        } else {
            None
        };
        // Same serialization as session admission: validation, receipts and effects cannot
        // race another lifecycle mutation or a competing assignment input.
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let record = match request {
            DelegationRequest::Prepare { auth, contract } => {
                contract
                    .validate()
                    .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
                let parent = self.delegation_auth(&auth)?;
                self.reject_assigned_input(auth.session)?;
                let parent_binding = parent
                    .summary()
                    .agent
                    .filter(|agent| agent.health.state == ReporterHealth::Connected)
                    .map(|agent| agent.binding)
                    .ok_or_else(|| {
                        lifecycle_error(ErrorCode::Conflict, "parent Pi conversation is not bound")
                    })?;
                let handle = contract.request.clone();
                let bytes = serde_json::to_vec(&contract)?;
                let contract_digest = digest(&bytes);
                let previous = self.retained.lock().assignment(&handle)?;
                let mut record = if let Some(mut record) = previous {
                    if record.parent != auth.session
                        || record.parent_run != parent.run()
                        || record.parent_binding != parent_binding
                        || record.contract_digest != contract_digest
                    {
                        return Err(lifecycle_error(
                            ErrorCode::Conflict,
                            "request identity already belongs to another contract or controller",
                        ));
                    }
                    record.upgrade_legacy();
                    record
                } else {
                    let store = self.retained.lock();
                    if store.assignment_count()? >= MAX_DELEGATION_RECORDS {
                        return Err(lifecycle_error(
                            ErrorCode::Conflict,
                            "delegation receipt storage is full (4096 assignments); existing assignments remain recoverable; new admission requires supported retention maintenance",
                        ));
                    }
                    if let Some(previous) = store.workflow_controller(&contract.workflow)?
                        && (previous.identity.controller != contract.controller
                            || previous.parent != auth.session
                            || previous.parent_run != parent.run()
                            || previous.parent_binding != parent_binding)
                    {
                        return Err(lifecycle_error(
                            ErrorCode::Conflict,
                            "workflow already has a different controller",
                        ));
                    }
                    let root = std::fs::canonicalize(&self.registry_path)?
                        .join("workflows")
                        .join(&handle);
                    let mut record = Record {
                        handle: handle.clone(),
                        identity: AssignmentIdentity {
                            instance: store.delegation_instance()?,
                            workflow: contract.workflow.clone(),
                            controller: contract.controller.clone(),
                            controller_epoch: 1,
                            task: contract.task.clone(),
                            attempt: 1,
                            contract: 1,
                            request: contract.request.clone(),
                            session: None,
                            run: None,
                        },
                        binding: None,
                        parent_binding,
                        parent: auth.session,
                        parent_run: parent.run(),
                        root,
                        contract_digest,
                        brief_digest: None,
                        result_digest: None,
                        dispatch: DispatchStatus::Prepared,
                        reason: None,
                        project: contract.project.clone(),
                        workspace: contract.workspace.clone(),
                        receipts: Vec::new(),
                        request_digests: Vec::new(),
                    };
                    record.intent(DispatchOperation::Prepare, &record.identity.clone())?;
                    store.save_assignment(&record)?;
                    record
                };
                if record
                    .receipt(DispatchOperation::Prepare)
                    .is_some_and(|r| r.outcome != DispatchOutcome::Applied)
                {
                    record.outcome(DispatchOperation::Prepare, DispatchOutcome::Unknown, None, Some("contract publication is unresolved; recover with the exact original intent"));
                    self.retained.lock().save_assignment(&record)?;
                    if take_fault("prepare-before-file", &handle).is_some() {
                        return self.reconciliation_snapshot(record);
                    }
                    let directory = Directory::open(&record.root, true)?;
                    if let Err(error) = directory.publish("contract-v1.json", &bytes)
                        && directory.read("contract-v1.json").ok().as_ref() != Some(&bytes)
                    {
                        return Err(error);
                    }
                    if take_fault("prepare-after-file", &handle).is_some() {
                        return self.reconciliation_snapshot(record);
                    }
                    directory.sync()?;
                    record.outcome(
                        DispatchOperation::Prepare,
                        DispatchOutcome::Applied,
                        None,
                        None,
                    );
                    self.retained.lock().save_assignment(&record)?;
                }
                record
            }
            DelegationRequest::Launch {
                auth,
                handle,
                identity,
            } => {
                let mut record = self.owned_assignment(&auth, &handle)?;
                if !record.intent(DispatchOperation::Launch, &identity)? {
                    self.validate_allocation(&record)?;
                    return self.reconciliation_snapshot(record);
                }
                self.retained.lock().save_assignment(&record)?;
                if !record
                    .receipt(DispatchOperation::Prepare)
                    .is_some_and(|r| r.outcome == DispatchOutcome::Applied)
                {
                    record.outcome(
                        DispatchOperation::Launch,
                        DispatchOutcome::Rejected,
                        None,
                        Some("contract publication is not committed; recover the original Prepare"),
                    );
                    self.retained.lock().save_assignment(&record)?;
                    return self.reconciliation_snapshot(record);
                }
                if record.dispatch == DispatchStatus::Prepared {
                    let contract = read_contract(&record)?;
                    // The real provider inventory is checked before creating a session.
                    if availability != Some(true) {
                        record.dispatch = DispatchStatus::Refused;
                        record.reason =
                            Some("requested Pi/model unavailable; no substitute launched".into());
                        record.outcome(
                            DispatchOperation::Launch,
                            DispatchOutcome::Rejected,
                            None,
                            Some("requested Pi/model unavailable; no substitute launched"),
                        );
                        self.retained.lock().save_assignment(&record)?;
                    } else {
                        if self.check_live_capacity().is_err() {
                            record.dispatch = DispatchStatus::Refused;
                            record.reason = Some("all 50 live process slots are occupied".into());
                            record.outcome(
                                DispatchOperation::Launch,
                                DispatchOutcome::Rejected,
                                None,
                                Some("all 50 live process slots are occupied"),
                            );
                            self.retained.lock().save_assignment(&record)?;
                            return self.assignment_snapshot(record);
                        }
                        record.dispatch = DispatchStatus::LaunchUnknown;
                        record.outcome(
                            DispatchOperation::Launch,
                            DispatchOutcome::Unknown,
                            None,
                            Some("launch effect is unresolved; do not replay"),
                        );
                        self.retained.lock().save_assignment(&record)?;
                        if take_fault("launch-before-effect", &handle).is_some() {
                            return self.reconciliation_snapshot(record);
                        }
                        let argv = vec![
                            std::env::current_exe()?.into_os_string(),
                            "agent".into(),
                            "run".into(),
                            "pi".into(),
                            "--".into(),
                            "pi".into(),
                            "--no-approve".into(),
                            "--no-extensions".into(),
                            "--no-skills".into(),
                            "--tools".into(),
                            "read,grep,find,ls".into(),
                            "--provider".into(),
                            contract.model_provider.into(),
                            "--model".into(),
                            contract.model.into(),
                        ];
                        let result = self.create_session_locked(
                            CreateSessionRequest {
                                project: contract.project,
                                workspace: contract.workspace,
                                name: String::new(),
                                label: Some("Pi research".into()),
                                argv,
                                kind: SessionKind::Agent { name: "pi".into() },
                            },
                            None,
                            Some(&handle),
                        );
                        record = self
                            .retained
                            .lock()
                            .assignment(&handle)?
                            .context("assignment disappeared")?;
                        if take_fault("launch-after-effect", &handle).is_some() {
                            return self.reconciliation_snapshot(record);
                        }
                        match result {
                            Ok(_) => {
                                record.dispatch = DispatchStatus::Launched;
                                record.outcome(
                                    DispatchOperation::Launch,
                                    DispatchOutcome::Applied,
                                    None,
                                    None,
                                );
                            }
                            Err(error) => {
                                record.reason = Some("launch did not complete; inspect retained session before taking further action".into());
                                if error.is::<crate::session::NoProcessStarted>()
                                    || record.identity.session.is_none()
                                {
                                    record.dispatch = DispatchStatus::Refused;
                                    record.outcome(DispatchOperation::Launch, DispatchOutcome::Rejected, None, Some("launch refused before a process started; reconcile retained resources"));
                                }
                            }
                        }
                        self.retained.lock().save_assignment(&record)?;
                    }
                }
                record
            }
            DelegationRequest::SendInitial {
                auth,
                handle,
                identity,
            } => {
                let mut record = self.owned_assignment(&auth, &handle)?;
                if record.identity.session.is_none() || record.identity.run.is_none() {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "initial input requires the original allocated child run",
                    ));
                }
                if !record.intent(DispatchOperation::InitialInput, &identity)? {
                    self.validate_allocation(&record)?;
                    return self.reconciliation_snapshot(record);
                }
                self.retained.lock().save_assignment(&record)?;
                if record.dispatch == DispatchStatus::Launched {
                    let session = match self.assignment_session(&record) {
                        Ok(session) => session,
                        Err(_) => {
                            return self.reject_initial(
                                record,
                                "child run or conversation changed before initial input",
                            );
                        }
                    };
                    let summary = session.summary();
                    let Some(agent) = summary.agent.as_ref().filter(|agent| {
                        agent.binding.provider == AgentProvider::Pi
                            && agent.health.state == ReporterHealth::Connected
                    }) else {
                        return self.assignment_snapshot(record);
                    };
                    if record.binding.is_none() {
                        // A newly spawned Session starts at generation zero; its first
                        // Bind is generation one. Never adopt a replacement as the target.
                        if agent.binding.generation != 1 {
                            return self.reject_initial(record, "the fresh child's initial conversation was replaced; do not replay");
                        }
                        record.binding = Some(agent.binding.clone());
                        self.retained.lock().save_assignment(&record)?;
                        if take_fault("input-target-captured", &handle).is_some() {
                            return self.reconciliation_snapshot(record);
                        }
                    }
                    let contract = read_contract(&record)?;
                    if agent
                        .metrics
                        .as_ref()
                        .and_then(|m| m.sample.model.as_deref())
                        != Some(contract.model.as_str())
                    {
                        return self.assignment_snapshot(record);
                    }
                    let bytes = Directory::open(&record.root, false)?.read("brief-v1.json")?;
                    if Some(digest(&bytes)) != record.brief_digest {
                        bail!("immutable brief changed");
                    }
                    hold_before_input(&handle)?;
                    record.dispatch = DispatchStatus::InputUnknown;
                    record.outcome(
                        DispatchOperation::InitialInput,
                        DispatchOutcome::Unknown,
                        None,
                        Some("initial input effect is unknown; do not replay"),
                    );
                    let parent = self.session_for_control(auth.session)?;
                    let store = self.retained.lock();
                    store.save_assignment(&record)?;
                    let text = format!(
                        "Perform the read-only research assignment in {}. Read that immutable brief with your read tool; publish the complete result with ovrcr_publish using handle {} and the exact identity in the brief. Do not modify task or parent records. A terminal success line is not a submission.",
                        record.root.join("brief-v1.json").display(),
                        record.handle
                    );
                    match session.send_delegated_text(crate::session::DelegatedInput {
                        request: &handle,
                        parent: &parent,
                        parent_binding: &record.parent_binding,
                        capability: &auth.capability,
                        child_binding: record.binding.as_ref().unwrap(),
                        model: &contract.model,
                        text: &text,
                    }) {
                        Ok(crate::session::DelegatedDelivery::Applied(input)) => {
                            if take_fault("input-after-effect", &handle).is_some() {
                                drop(store);
                                return self.reconciliation_snapshot(record);
                            }
                            record.dispatch = DispatchStatus::Applied;
                            record.outcome(DispatchOperation::InitialInput, DispatchOutcome::Applied, Some(input), None);
                        }
                        Ok(crate::session::DelegatedDelivery::Rejected(input)) => {
                            record.dispatch = DispatchStatus::Refused;
                            record.outcome(DispatchOperation::InitialInput, DispatchOutcome::Rejected, Some(input), Some("initial input was rejected without writing bytes"));
                        }
                        Ok(crate::session::DelegatedDelivery::Partial(input)) => record.outcome(DispatchOperation::InitialInput, DispatchOutcome::Partial, Some(input), Some("only a prefix reached the PTY; do not replay")),
                        Ok(crate::session::DelegatedDelivery::Unknown(input)) => record.outcome(DispatchOperation::InitialInput, DispatchOutcome::Unknown, Some(input), Some("input bytes reached the PTY but completion is uncertain; do not replay")),
                        Err(_) => {
                            record.reason =
                                Some("initial input effect is unknown; do not replay".into())
                        }
                    }
                    store.save_assignment(&record)?;
                }
                record
            }
            DelegationRequest::Inspect { auth, handle }
            | DelegationRequest::Collect { auth, handle } => {
                return self.assignment_snapshot(self.owned_assignment(&auth, &handle)?);
            }
            DelegationRequest::Reconcile { auth, request } => {
                return self.reconciliation_snapshot(self.owned_assignment(&auth, &request)?);
            }
            DelegationRequest::Publish {
                auth,
                handle,
                submission,
            } => {
                let mut record = self
                    .retained
                    .lock()
                    .assignment(&handle)?
                    .context("assignment not found")?;
                let session = self.delegation_auth(&auth)?;
                if Some(auth.session) != record.identity.session
                    || Some(session.run()) != record.identity.run
                {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "only the assigned child may publish a submission",
                    ));
                }
                self.assignment_session(&record)?;
                submission
                    .validate(&record.identity)
                    .map_err(anyhow::Error::msg)?;
                if record.dispatch != DispatchStatus::Applied
                    && record.dispatch != DispatchStatus::InputUnknown
                {
                    bail!("initial assignment has not been delivered");
                }
                let bytes = serde_json::to_vec(&submission)?;
                let hash = digest(&bytes);
                if record
                    .result_digest
                    .as_ref()
                    .is_some_and(|old| old != &hash)
                {
                    bail!("submission version is already published");
                }
                let binding = record
                    .binding
                    .clone()
                    .context("child assignment is not bound")?;
                let store = self.retained.lock();
                session.with_delegation_binding(&auth.capability, &binding, || {
                    let directory = Directory::open(&record.root, false)?;
                    if let Err(error) = directory.publish("submission-v1.json", &bytes)
                        && directory.read("submission-v1.json").ok().as_ref() != Some(&bytes)
                    {
                        return Err(error);
                    }
                    record.result_digest = Some(hash);
                    store.save_assignment(&record)
                })?;
                drop(store);
                record
            }
        };
        self.reconciliation_snapshot(record)
    }

    fn assignment_session(
        &self,
        record: &Record,
    ) -> Result<std::sync::Arc<crate::session::Session>> {
        let id = record.identity.session.context("assignment has no child")?;
        let session = self.session_for_control(id)?;
        if Some(session.run()) != record.identity.run {
            bail!("assigned run changed");
        }
        let summary = session.summary();
        if let Some(binding) = &record.binding
            && summary.agent.as_ref().map(|agent| &agent.binding) != Some(binding)
        {
            bail!("child conversation binding changed");
        }
        Ok(session)
    }

    fn validate_allocation(&self, record: &Record) -> Result<()> {
        if let Some(summary) = record
            .identity
            .session
            .and_then(|id| self.session_summary(id))
        {
            if Some(summary.run) != record.identity.run {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "assigned run changed; reconcile the original receipt",
                ));
            }
            if summary.phase.is_live()
                && record.binding.as_ref().is_some_and(|binding| {
                    summary.agent.as_ref().map(|a| &a.binding) != Some(binding)
                })
            {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "child conversation binding changed; reconcile the original receipt",
                ));
            }
        }
        Ok(())
    }

    fn reject_initial(&self, mut record: Record, reason: &str) -> Result<AssignmentSnapshot> {
        record.dispatch = DispatchStatus::Refused;
        record.outcome(
            DispatchOperation::InitialInput,
            DispatchOutcome::Rejected,
            None,
            Some(reason),
        );
        self.retained.lock().save_assignment(&record)?;
        self.reconciliation_snapshot(record)
    }

    // Called after row allocation and before spawn. The durable run identity here is
    // exactly the next begin_run identity; the shared admission lock fences both.
    pub(super) fn bind_assignment(
        &self,
        handle: &str,
        row: &crate::retained::RetainedSession,
    ) -> Result<()> {
        let mut record = self
            .retained
            .lock()
            .assignment(handle)?
            .context("assignment not found")?;
        record.identity.session = Some(row.id);
        record.identity.run = Some(SessionRunId(
            row.run.0.checked_add(1).context("run exhausted")?,
        ));
        self.retained.lock().save_assignment(&record)?;
        let contract = read_contract(&record)?;
        let brief = serde_json::to_vec_pretty(
            &serde_json::json!({"schema": 1, "handle": handle, "identity": record.identity, "contract": contract, "output": record.root.join("submission-v1.json"), "publication": "Call ovrcr_publish. Only this child can publish; it cannot accept the result. Research tools are read-only."}),
        )?;
        Directory::open(&record.root, false)?.publish("brief-v1.json", &brief)?;
        record.brief_digest = Some(digest(&brief));
        self.retained.lock().save_assignment(&record)?;
        Ok(())
    }

    fn assignment_snapshot(&self, record: Record) -> Result<AssignmentSnapshot> {
        let summary = record
            .identity
            .session
            .and_then(|id| self.session_summary(id));
        let submission = match &record.result_digest {
            Some(expected) => {
                if summary
                    .as_ref()
                    .is_none_or(|current| Some(current.run) != record.identity.run)
                {
                    bail!("submission belongs to a stale run");
                }
                if let Some(current) = summary
                    .as_ref()
                    .filter(|s| s.phase.is_live())
                    .and_then(|s| s.agent.as_ref())
                    && record.binding.as_ref() != Some(&current.binding)
                {
                    bail!("submission belongs to a changed conversation");
                }
                let bytes = Directory::open(&record.root, false)?.read("submission-v1.json")?;
                if &digest(&bytes) != expected {
                    bail!("submission publication changed or is partial");
                }
                let result: ResearchSubmission = serde_json::from_slice(&bytes)?;
                result
                    .validate(&record.identity)
                    .map_err(anyhow::Error::msg)?;
                Some(result)
            }
            None => None,
        };
        self.snapshot_with_submission(record, submission)
    }

    fn reconciliation_snapshot(&self, record: Record) -> Result<AssignmentSnapshot> {
        self.snapshot_with_submission(record, None)
    }

    fn snapshot_with_submission(
        &self,
        record: Record,
        submission: Option<ResearchSubmission>,
    ) -> Result<AssignmentSnapshot> {
        let summary = record
            .identity
            .session
            .and_then(|id| self.session_summary(id));
        let project = if record.project.is_empty() {
            summary.as_ref().map(|s| s.project.clone())
        } else {
            Some(record.project.clone())
        };
        let workspace = if record.workspace.is_empty() {
            summary.as_ref().map(|s| s.workspace.clone())
        } else {
            Some(record.workspace.clone())
        };
        let workspace_present =
            project
                .as_ref()
                .zip(workspace.as_ref())
                .is_some_and(|(project, workspace)| {
                    self.registry
                        .lock()
                        .unwrap()
                        .workspace(project, workspace)
                        .is_ok()
                });
        let resources = AssignmentResources {
            project,
            workspace,
            workspace_present,
            current_session: summary.as_ref().map(|s| CurrentAssignmentSession {
                session: s.id,
                run: s.run,
                phase: s.phase.clone(),
            }),
        };
        Ok(AssignmentSnapshot {
            handle: record.handle,
            identity: record.identity,
            dispatch: record.dispatch,
            reason: record.reason,
            activity: summary
                .as_ref()
                .map(|s| s.activity)
                .unwrap_or(AgentActivity::Unknown),
            conversation: summary
                .as_ref()
                .and_then(|s| s.agent.as_ref())
                .map(|a| a.binding.conversation.clone()),
            brief: record.root.join("brief-v1.json"),
            result_path: record.root.join("submission-v1.json"),
            submission,
            acceptance: "pending".into(),
            receipts: record.receipts,
            resources,
        })
    }
}

fn read_contract(record: &Record) -> Result<ResearchContract> {
    let bytes = Directory::open(&record.root, false)?.read("contract-v1.json")?;
    if digest(&bytes) != record.contract_digest {
        bail!("immutable contract changed");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn available_model(contract: &ResearchContract) -> Result<()> {
    // Provider inventory is local to the same inherited profile as the eventual child.
    // File-backed bounded output avoids a full pipe blocking the probe at exit.
    let mut output = tempfile::tempfile()?;
    let mut child = Command::new("pi")
        .args(["--no-extensions", "--no-skills", "--list-models"])
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("provider inventory failed");
            }
            break;
        }
        if Instant::now() >= deadline || output.metadata()?.len() > 512 * 1024 {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!("provider inventory unavailable");
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    output.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    output.take(512 * 1024).read_to_string(&mut text)?;
    if !text.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some(contract.model_provider.as_str())
            && fields.next() == Some(contract.model.as_str())
    }) {
        bail!("requested exact model is unavailable");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(root: PathBuf, status: DispatchStatus) -> Record {
        let root = std::fs::canonicalize(root.parent().unwrap())
            .unwrap()
            .join(root.file_name().unwrap());
        let contract = ResearchContract {
            schema: 1,
            workflow: "workflow".into(),
            controller: "controller".into(),
            task: "task".into(),
            request: "request".into(),
            project: "project".into(),
            workspace: "workspace".into(),
            scope: "Read README".into(),
            permitted_actions: vec!["read".into()],
            provider: "pi".into(),
            model_provider: "test".into(),
            model: "fixture-model".into(),
            acceptance_evidence: vec!["citation".into()],
            mandatory_capabilities: vec!["read".into()],
        };
        let bytes = serde_json::to_vec(&contract).unwrap();
        Directory::open(&root, true)
            .unwrap()
            .publish("contract-v1.json", &bytes)
            .unwrap();
        let allocated = status != DispatchStatus::Prepared;
        Record {
            handle: "request".into(),
            identity: AssignmentIdentity {
                instance: "instance".into(),
                workflow: "workflow".into(),
                controller: "controller".into(),
                controller_epoch: 1,
                task: "task".into(),
                attempt: 1,
                contract: 1,
                request: "request".into(),
                session: allocated.then_some(SessionId(2)),
                run: allocated.then_some(SessionRunId(1)),
            },
            binding: matches!(
                status,
                DispatchStatus::Applied | DispatchStatus::InputUnknown
            )
            .then(|| AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "child".into(),
                conversation: "child-A".into(),
                generation: 1,
            }),
            parent_binding: AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "parent".into(),
                conversation: "A".into(),
                generation: 1,
            },
            parent: SessionId(1),
            parent_run: SessionRunId(1),
            root,
            contract_digest: digest(&bytes),
            brief_digest: None,
            result_digest: None,
            dispatch: status,
            reason: None,
            project: String::new(),
            workspace: String::new(),
            receipts: vec![],
            request_digests: vec![],
        }
    }

    #[test]
    fn legacy_dispatch_never_infers_unsent_from_missing_receipts() {
        for (status, operation, expected) in [
            (
                DispatchStatus::Prepared,
                DispatchOperation::Launch,
                DispatchOutcome::Accepted,
            ),
            (
                DispatchStatus::LaunchUnknown,
                DispatchOperation::Launch,
                DispatchOutcome::Unknown,
            ),
            (
                DispatchStatus::InputUnknown,
                DispatchOperation::InitialInput,
                DispatchOutcome::Unknown,
            ),
            (
                DispatchStatus::Applied,
                DispatchOperation::InitialInput,
                DispatchOutcome::Applied,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut value = record(directory.path().join("assignment"), status);
            value.upgrade_legacy();
            assert_eq!(value.receipt(operation).unwrap().outcome, expected);
            assert_eq!(value.project, "project");
            assert!(value.receipts.len() <= 3);
        }
        let directory = tempfile::tempdir().unwrap();
        let mut value = record(
            directory.path().join("assignment"),
            DispatchStatus::Prepared,
        );
        std::fs::write(value.root.join("contract-v1.json"), b"{}").unwrap();
        value.upgrade_legacy();
        assert!(
            value
                .receipts
                .iter()
                .all(|r| r.outcome == DispatchOutcome::Unknown)
        );
        assert!(
            !value
                .intent(DispatchOperation::Launch, &value.identity.clone())
                .unwrap()
        );
        let directory = tempfile::tempdir().unwrap();
        let mut contradictory =
            record(directory.path().join("assignment"), DispatchStatus::Applied);
        contradictory.binding = None;
        contradictory.upgrade_legacy();
        assert!(
            contradictory
                .receipts
                .iter()
                .all(|r| r.outcome == DispatchOutcome::Unknown)
        );
    }

    #[test]
    fn duplicate_launch_matches_original_identity_after_allocation() {
        let directory = tempfile::tempdir().unwrap();
        let mut value = record(
            directory.path().join("assignment"),
            DispatchStatus::Prepared,
        );
        let original = value.identity.clone();
        assert!(value.intent(DispatchOperation::Launch, &original).unwrap());
        value.outcome(
            DispatchOperation::Launch,
            DispatchOutcome::Applied,
            None,
            None,
        );
        value.identity.session = Some(SessionId(2));
        value.identity.run = Some(SessionRunId(1));
        assert!(!value.intent(DispatchOperation::Launch, &original).unwrap());
        let mut conflicting = original;
        conflicting.contract += 1;
        assert!(
            value
                .intent(DispatchOperation::Launch, &conflicting)
                .is_err()
        );
        assert_eq!(value.receipts.len(), 1);
    }
}
