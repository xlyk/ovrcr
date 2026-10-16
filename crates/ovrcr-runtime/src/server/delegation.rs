//! Server-owned assignment control. Task payloads live only in private publications.
#[path = "delegation_control.rs"]
pub(crate) mod control;
#[path = "delegation_followups.rs"]
mod followups;
#[path = "delegation_policy.rs"]
mod policy;

use super::delegation_files::Directory;
use super::{ServerState, lifecycle_error};
use anyhow::{Context, Result, bail};
use followups::{FollowupIntent, VersionState, initial_result_version, read_version_submission};
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

// Interface shared with the publication/wake slice. A missing intent is not evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PublicationIntent {
    pub identity: PublicationIdentity,
    pub binding: AgentBinding,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub handle: String,
    #[serde(default)]
    pub control: AssignmentControl,
    #[serde(default)]
    pub takeover_evidence: Option<AssignmentControlEvidence>,
    #[serde(default)]
    pub control_intents: Vec<control::ControlIntent>,
    #[serde(default)]
    pub initial_control_epoch: Option<u64>,
    pub identity: AssignmentIdentity,
    pub binding: Option<AgentBinding>,
    pub parent_binding: AgentBinding,
    pub parent: SessionId,
    pub parent_run: SessionRunId,
    pub root: PathBuf,
    pub contract_digest: String,
    pub brief_digest: Option<String>,
    pub result_digest: Option<String>,
    #[serde(default = "initial_result_version")]
    pub result_version: u32,
    #[serde(default)]
    pub publication_intent: Option<PublicationIntent>,
    #[serde(default)]
    pub handled_publication: Option<PublicationIdentity>,
    #[serde(default)]
    pub acceptance: Option<AcceptanceReference>,
    #[serde(default)]
    pub previous_versions: Vec<VersionState>,
    #[serde(default)]
    pub followups: Vec<FollowupIntent>,
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
    #[serde(default)]
    pub previous_request: Option<String>,
    #[serde(default)]
    pub successor_request: Option<String>,
    #[serde(default)]
    pub execution_attempts: Option<u32>,
    #[serde(default)]
    pub retryable_capacity_refusal: bool,
}

// Three initial operation slots plus bounded same-child followups; never evict evidence.
const MAX_DELEGATION_RECORDS: u64 = 4096;

impl Record {
    /// Validate operation evidence before using it to authorize another effect.
    /// Receipt reconstruction is never an independent source of budget proof.
    fn spent_execution_attempts(&self) -> Option<u32> {
        if self.identity.session.is_some() != self.identity.run.is_some()
            || self.handle != self.identity.request
            || self.receipts.len() > 3 + followups::MAX_FOLLOWUPS + 64
            || self.control_intents.len() > 64
            || self.followups.len() > followups::MAX_FOLLOWUPS
            || self.request_digests.len() > 3
        {
            return None;
        }
        let allocated = self.identity.session.is_some();
        let consistent = match self.dispatch {
            DispatchStatus::Prepared => !allocated && self.binding.is_none(),
            DispatchStatus::LaunchUnknown => self.binding.is_none(),
            DispatchStatus::Launched => allocated && self.binding.is_none(),
            DispatchStatus::InputUnknown | DispatchStatus::Applied => {
                allocated && self.binding.is_some()
            }
            DispatchStatus::Refused => self.binding.is_none() || allocated,
        };
        if !consistent {
            return None;
        }
        let first = self.previous_versions.first();
        let initial_identity = first.map_or(&self.identity, |v| &v.identity);
        let initial_digest = first.map_or(&self.contract_digest, |v| &v.contract_digest);
        let versions: Vec<_> = self
            .previous_versions
            .iter()
            .chain(std::iter::once(&self.version()))
            .cloned()
            .collect();
        if versions[0].result_version != 1
            || versions.iter().enumerate().any(|(index, v)| {
                (index > 0 && v.result_version <= versions[index - 1].result_version)
                    || v.identity.session != self.identity.session
                    || v.identity.run != self.identity.run
                    || {
                        let mut identity = v.identity.clone();
                        identity.contract = initial_identity.contract;
                        identity != *initial_identity
                    }
            })
        {
            return None;
        }
        // Reservations are dense; materialized versions need not be. A rejected
        // stale-control intent retains its files and numbers without advancing
        // the current version. Validate both histories rather than guessing gaps.
        let mut reserved_result: u32 = 1;
        let mut reserved_contract = initial_identity.contract;
        let mut materialized = 0;
        for (index, intent) in self.followups.iter().enumerate() {
            let expected = &versions[materialized];
            let receipt = self.request_receipt(&intent.request, DispatchOperation::Followup)?;
            let mut target = intent.expected.clone();
            target.contract = intent.target.contract;
            if intent.expected != expected.identity
                || intent.expected_result_version != expected.result_version
                || intent.result_version != reserved_result.checked_add(1)?
                || !(intent.target.contract == intent.expected.contract
                    || intent.target.contract == reserved_contract.checked_add(1)?)
                || target != intent.target
            {
                return None;
            }
            reserved_result = intent.result_version;
            reserved_contract = reserved_contract.max(intent.target.contract);
            let next = versions
                .get(materialized + 1)
                .filter(|version| version.result_version == intent.result_version);
            if let Some(next) = next {
                if receipt.outcome == DispatchOutcome::Accepted
                    || next.identity != intent.target
                    || next.contract_digest != intent.contract_digest
                    || next.brief_digest.as_deref() != Some(intent.brief_digest.as_str())
                {
                    return None;
                }
                materialized += 1;
            } else if receipt.outcome != DispatchOutcome::Rejected
                && !(receipt.outcome == DispatchOutcome::Accepted
                    && index + 1 == self.followups.len())
            {
                return None;
            }
        }
        if materialized + 1 != versions.len() {
            return None;
        }
        let mut prepare_identity = initial_identity.clone();
        prepare_identity.session = None;
        prepare_identity.run = None;
        for (index, receipt) in self.receipts.iter().enumerate() {
            if matches!(
                receipt.operation,
                DispatchOperation::Takeover
                    | DispatchOperation::HumanReturn
                    | DispatchOperation::ConfirmHumanReturn
            ) {
                let intent = self
                    .control_intents
                    .iter()
                    .find(|intent| intent.request == receipt.request)?;
                let version = versions
                    .iter()
                    .find(|v| v.result_version == receipt.result_version)?;
                if intent.operation != receipt.operation
                    || receipt.identity != version.identity
                    || self.receipts[..index]
                        .iter()
                        .any(|old| old.request == receipt.request)
                {
                    return None;
                }
                continue;
            }
            if receipt.operation == DispatchOperation::Followup {
                let intent = self
                    .followups
                    .iter()
                    .find(|f| f.request == receipt.request)?;
                let expected = versions
                    .iter()
                    .find(|v| v.result_version == intent.expected_result_version)?;
                let mut target = intent.expected.clone();
                target.contract = intent.target.contract;
                if intent.expected != expected.identity
                    || target != intent.target
                    || receipt.identity != intent.target
                    || receipt.result_version != intent.result_version
                    || self.receipts[..index]
                        .iter()
                        .any(|old| old.request == receipt.request)
                {
                    return None;
                }
                continue;
            }
            let expected = if receipt.operation == DispatchOperation::InitialInput {
                initial_identity
            } else {
                &prepare_identity
            };
            if receipt.request != self.identity.request
                || receipt.result_version != 1
                || &receipt.identity != expected
                || self.receipts[..index]
                    .iter()
                    .any(|old| old.operation == receipt.operation)
            {
                return None;
            }
        }
        if self.followups.iter().enumerate().any(|(index, f)| {
            self.request_receipt(&f.request, DispatchOperation::Followup)
                .is_none()
                || self.followups[..index]
                    .iter()
                    .any(|old| old.request == f.request)
        }) {
            return None;
        }
        if self
            .control_intents
            .iter()
            .enumerate()
            .any(|(index, intent)| {
                !matches!(
                    intent.operation,
                    DispatchOperation::Takeover
                        | DispatchOperation::HumanReturn
                        | DispatchOperation::ConfirmHumanReturn
                ) || self
                    .request_receipt(&intent.request, intent.operation)
                    .is_none()
                    || self.control_intents[..index]
                        .iter()
                        .any(|old| old.request == intent.request)
            })
        {
            return None;
        }
        for (index, (operation, saved)) in self.request_digests.iter().enumerate() {
            if !matches!(
                operation,
                DispatchOperation::Prepare
                    | DispatchOperation::Launch
                    | DispatchOperation::InitialInput
            ) {
                return None;
            }
            let receipt = self.receipt(*operation)?;
            let expected =
                digest(&serde_json::to_vec(&(*operation, initial_digest, &receipt.identity)).ok()?);
            if saved != &expected
                || self.request_digests[..index]
                    .iter()
                    .any(|(old, _)| old == operation)
            {
                return None;
            }
        }
        let launch = self.receipt(DispatchOperation::Launch).map(|r| r.outcome);
        let initial = self.receipt(DispatchOperation::InitialInput);
        let crossed = matches!(
            launch,
            Some(DispatchOutcome::Applied | DispatchOutcome::Unknown | DispatchOutcome::Partial)
        ) || initial.is_some()
            || !self.followups.is_empty()
            || !self.control_intents.is_empty()
            || !self.previous_versions.is_empty()
            || self.binding.is_some()
            || matches!(
                self.dispatch,
                DispatchStatus::LaunchUnknown
                    | DispatchStatus::Launched
                    | DispatchStatus::InputUnknown
                    | DispatchStatus::Applied
            );
        if let Some(count) = self.execution_attempts {
            // Allocating a row alone is not execution, and an authoritative
            // pre-process rejection may retain that allocated identity.
            if count == 0
                && (crossed
                    || self.identity.session.is_some() && launch != Some(DispatchOutcome::Rejected))
            {
                return None;
            }
            return Some(count);
        }
        // Old records can establish one first attempt, never an unknown cumulative
        // retry history. Current resource absence is deliberately not consulted.
        if self.identity.attempt != 1 || self.previous_request.is_some() {
            return None;
        }
        let contract = read_contract(self).ok()?;
        if contract.validate().is_err()
            || contract.previous_request.is_some()
            || contract.request != self.identity.request
            || contract.workflow != self.identity.workflow
            || contract.controller != self.identity.controller
            || contract.task != self.identity.task
        {
            return None;
        }
        if matches!(self.dispatch, DispatchStatus::Prepared) {
            if crossed || self.identity.session.is_some() {
                return None;
            }
            let prepare = self.receipt(DispatchOperation::Prepare).map(|r| r.outcome);
            // Raw legacy Prepared was durably replaced with LaunchUnknown before
            // the native boundary. A validated intact record with no operation
            // evidence contradicting it retains that old pre-effect invariant.
            return if self.receipts.is_empty() && self.request_digests.is_empty()
                || prepare == Some(DispatchOutcome::Applied)
                    && matches!(launch, None | Some(DispatchOutcome::Accepted))
            {
                Some(0)
            } else {
                None
            };
        }
        if crossed || self.identity.session.is_some() {
            return Some(1);
        }
        // Refused/Rejected is not no-process proof: it may follow earlier effects.
        None
    }

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
        let initial_digest = self
            .previous_versions
            .first()
            .map_or(&self.contract_digest, |version| &version.contract_digest);
        let expected = digest(&serde_json::to_vec(&(operation, initial_digest, identity))?);
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
            result_version: 1,
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
        // Derive before receipts are synthesized; do not bless their provenance.
        self.execution_attempts = self.spent_execution_attempts();
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
                    (
                        DispatchOperation::Followup
                        | DispatchOperation::Takeover
                        | DispatchOperation::HumanReturn
                        | DispatchOperation::ConfirmHumanReturn,
                        _,
                    ) => {
                        unreachable!("legacy dispatch has no followups")
                    }
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
                    result_version: 1,
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
    pub(super) fn delegation_auth(
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

    pub(super) fn owned_assignment(&self, auth: &DelegationAuth, handle: &str) -> Result<Record> {
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

    pub(crate) fn delegation_capabilities(
        &self,
        auth: &DelegationAuth,
        selection: &DelegationSelection,
    ) -> Result<DelegationCapabilities> {
        self.delegation_auth(auth)?;
        self.reject_assigned_input(auth.session)?;
        let (settings, invalid) = self.policy_snapshot();
        let (inventory, unavailable) = match available_models() {
            Ok(inventory) => (inventory, invalid),
            Err(_) => (
                Vec::new(),
                Some("Pi model inventory is unavailable; no substitute selected".into()),
            ),
        };
        let (live, children) = self.delegation_live_counts()?;
        Ok(policy::resolve(
            &settings,
            selection,
            inventory,
            live,
            children,
            unavailable,
        ))
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
        } else if let DelegationRequest::Prepare { auth, contract } = &request {
            contract
                .validate()
                .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
            self.delegation_auth(auth)?;
            if self
                .retained
                .lock()
                .assignment(&contract.request)?
                .is_none()
            {
                Some(available_model(contract).is_ok())
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
            DelegationRequest::Capabilities { .. } => {
                return Err(lifecycle_error(
                    ErrorCode::InvalidRequest,
                    "capabilities require the discovery response path",
                ));
            }
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
                let previous = self.retained.lock().assignment(&handle)?;
                let bytes = contract_bytes(
                    &contract,
                    previous.as_ref().map(|record| {
                        record
                            .previous_versions
                            .first()
                            .map_or(record.contract_digest.as_str(), |v| {
                                v.contract_digest.as_str()
                            })
                    }),
                )?;
                let contract_digest = digest(&bytes);
                let is_new = previous.is_none();
                let prepared_policy = if is_new {
                    Some(self.contract_policy(&contract, availability == Some(true))?)
                } else {
                    None
                };
                let mut record = if let Some(mut record) = previous {
                    if record.parent != auth.session
                        || record.parent_run != parent.run()
                        || record.parent_binding != parent_binding
                        || record
                            .previous_versions
                            .first()
                            .map_or(&record.contract_digest, |version| &version.contract_digest)
                            != &contract_digest
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
                    let policy = prepared_policy
                        .as_ref()
                        .context("new assignment policy unavailable")?;
                    if let Some(reason) = &policy.reason {
                        return Err(lifecycle_error(ErrorCode::Conflict, reason));
                    }
                    if contract.previous_request.is_none()
                        && contract.limits != policy.effective_limits
                    {
                        return Err(lifecycle_error(
                            ErrorCode::Conflict,
                            "resolve effective delegation limits before Prepare",
                        ));
                    }
                    let previous = if let Some(previous_request) = &contract.previous_request {
                        let mut previous = store
                            .assignment(previous_request)?
                            .context("previous assignment not found")?;
                        previous.upgrade_legacy();
                        if previous.parent != auth.session
                            || previous.parent_run != parent.run()
                            || previous.parent_binding != parent_binding
                        {
                            return Err(lifecycle_error(
                                ErrorCode::Conflict,
                                "retry controller changed",
                            ));
                        }
                        if previous.dispatch != DispatchStatus::Refused
                            || !previous.retryable_capacity_refusal
                            || previous.result_version != 1
                            || !previous.previous_versions.is_empty()
                            || !previous.followups.is_empty()
                            || !previous.control_intents.is_empty()
                            || previous.control != AssignmentControl::default()
                            || previous.publication_intent.is_some()
                            || previous.result_digest.is_some()
                            || previous.unresolved_input()
                            || previous.identity.session.is_some()
                            || previous.identity.run.is_some()
                            || !previous
                                .receipt(DispatchOperation::Launch)
                                .is_some_and(|receipt| receipt.outcome == DispatchOutcome::Rejected)
                        {
                            return Err(lifecycle_error(
                                ErrorCode::Conflict,
                                "only an authoritative zero-effect capacity refusal can be retried",
                            ));
                        }
                        let old_contract = read_contract(&previous)?;
                        let mut comparable = contract.clone();
                        comparable.request = old_contract.request.clone();
                        comparable.previous_request = old_contract.previous_request.clone();
                        if comparable != old_contract
                            || previous.spent_execution_attempts().is_none()
                        {
                            return Err(lifecycle_error(
                                ErrorCode::Conflict,
                                "retry must preserve the exact logical task and effective contract",
                            ));
                        }
                        if previous
                            .successor_request
                            .as_ref()
                            .is_some_and(|request| request != &contract.request)
                            || store
                                .assignment_successor(previous_request)?
                                .is_some_and(|record| record.handle != contract.request)
                        {
                            return Err(lifecycle_error(
                                ErrorCode::Conflict,
                                "capacity refusal already has a successor; reconcile that request",
                            ));
                        }
                        Some(previous)
                    } else {
                        if store
                            .task_assignment(&contract.workflow, &contract.task)?
                            .is_some()
                        {
                            return Err(lifecycle_error(
                                ErrorCode::Conflict,
                                "logical task already exists; reconcile its original request",
                            ));
                        }
                        None
                    };
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
                        control: AssignmentControl::default(),
                        takeover_evidence: None,
                        control_intents: vec![],
                        initial_control_epoch: None,
                        handle: handle.clone(),
                        identity: AssignmentIdentity {
                            instance: store.delegation_instance()?,
                            workflow: contract.workflow.clone(),
                            controller: contract.controller.clone(),
                            controller_epoch: previous
                                .as_ref()
                                .map_or(1, |record| record.identity.controller_epoch),
                            task: contract.task.clone(),
                            attempt: match &previous {
                                Some(record) => record
                                    .identity
                                    .attempt
                                    .checked_add(1)
                                    .context("attempt identity exhausted")?,
                                None => 1,
                            },
                            contract: previous
                                .as_ref()
                                .map_or(1, |record| record.identity.contract),
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
                        result_version: 1,
                        publication_intent: None,
                        handled_publication: None,
                        acceptance: None,
                        previous_versions: Vec::new(),
                        followups: Vec::new(),
                        dispatch: DispatchStatus::Prepared,
                        reason: None,
                        project: contract.project.clone(),
                        workspace: contract.workspace.clone(),
                        receipts: Vec::new(),
                        request_digests: Vec::new(),
                        previous_request: contract.previous_request.clone(),
                        successor_request: None,
                        execution_attempts: Some(
                            previous
                                .as_ref()
                                .and_then(Record::spent_execution_attempts)
                                .unwrap_or(0),
                        ),
                        retryable_capacity_refusal: false,
                    };
                    record.intent(DispatchOperation::Prepare, &record.identity.clone())?;
                    if let Some(previous) = &previous {
                        store.save_assignment_successor(previous, &record)?;
                    } else {
                        store.save_assignment(&record)?;
                    }
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
                    let policy = self.contract_policy(&contract, availability == Some(true))?;
                    if let Some(reason) = policy.reason {
                        record.dispatch = DispatchStatus::Refused;
                        record.outcome(
                            DispatchOperation::Launch,
                            DispatchOutcome::Rejected,
                            None,
                            Some(&reason),
                        );
                        self.retained.lock().save_assignment(&record)?;
                    } else {
                        if self.check_live_capacity().is_err() || policy.available_slots == 0 {
                            record.dispatch = DispatchStatus::Refused;
                            record.reason = Some(if policy.live_sessions >= 50 {
                                "all 50 live process slots are occupied".into()
                            } else {
                                "delegation live-child limit or minimum free slots after launch prevents admission".into()
                            });
                            record.retryable_capacity_refusal =
                                record.spent_execution_attempts().is_some();
                            let reason = record.reason.clone();
                            record.outcome(
                                DispatchOperation::Launch,
                                DispatchOutcome::Rejected,
                                None,
                                reason.as_deref(),
                            );
                            self.retained.lock().save_assignment(&record)?;
                            return self.reconciliation_snapshot(record);
                        }
                        let spent = record.spent_execution_attempts().ok_or_else(|| {
                            lifecycle_error(
                                ErrorCode::Conflict,
                                "execution-attempt history is unknown; reconcile without replay",
                            )
                        })?;
                        if policy
                            .effective_limits
                            .max_execution_attempts
                            .is_some_and(|limit| spent >= limit)
                        {
                            record.dispatch = DispatchStatus::Refused;
                            record.outcome(
                                DispatchOperation::Launch,
                                DispatchOutcome::Rejected,
                                None,
                                Some("logical task execution-attempt limit reached"),
                            );
                            self.retained.lock().save_assignment(&record)?;
                            return self.reconciliation_snapshot(record);
                        }
                        // Resolve the executable before charging: lookup cannot create
                        // a process, and an error here must leave Prepared unspent.
                        let executable = std::env::current_exe()?;
                        record.execution_attempts = Some(
                            spent
                                .checked_add(1)
                                .context("execution-attempt count exhausted")?,
                        );
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
                            executable.into_os_string(),
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
                                if error.is::<crate::session::NoProcessStarted>() {
                                    record.execution_attempts = Some(spent);
                                }
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
                expected_control_epoch,
            } => {
                let mut record = self.owned_assignment(&auth, &handle)?;
                if record.identity.session.is_none() || record.identity.run.is_none() {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "initial input requires the original allocated child run",
                    ));
                }
                let legacy_pending_input = record.initial_control_epoch.is_none()
                    && record
                        .receipt(DispatchOperation::InitialInput)
                        .is_some_and(|receipt| receipt.outcome == DispatchOutcome::Accepted);
                if !record.intent(DispatchOperation::InitialInput, &identity)? {
                    self.validate_allocation(&record)?;
                    return self.reconciliation_snapshot(record);
                }
                if legacy_pending_input {
                    return self.reject_initial(record, "legacy initial input has no captured control epoch; inspect without replay");
                }
                // Accepted initial intents retain their original authority through recovery.
                // Terminalize only a durably stale captured intent, never a caller-only mismatch.
                if record.initial_control_epoch.is_some_and(|captured| {
                    control::require_controller(&record.control, captured).is_err()
                        || record.control.attribution_invalidated
                }) {
                    return self.reject_initial(
                        record,
                        "captured initial input control authority changed; no bytes written",
                    );
                }
                if record
                    .initial_control_epoch
                    .is_some_and(|epoch| epoch != expected_control_epoch)
                {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "initial request control epoch changed",
                    ));
                }
                record.initial_control_epoch = Some(expected_control_epoch);
                if control::require_controller(&record.control, expected_control_epoch).is_err()
                    || record.control.attribution_invalidated
                {
                    return self.reject_initial(
                        record,
                        "initial input control authority changed; no bytes written",
                    );
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
                        control: &record.control,
                        control_epoch: expected_control_epoch,
                        authority: crate::session::DelegatedAuthority::Controller,
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
            DelegationRequest::Followup {
                auth,
                handle,
                request,
                expected_identity,
                expected_result_version,
                expected_control_epoch,
                text,
                revised_contract,
            } => {
                return self.followup(
                    auth,
                    handle,
                    request,
                    (
                        expected_identity,
                        expected_result_version,
                        expected_control_epoch,
                    ),
                    text,
                    revised_contract.map(|contract| *contract),
                );
            }
            DelegationRequest::CollectVersion {
                auth,
                handle,
                version,
            } => {
                return self.collect_version(auth, handle, version);
            }
            DelegationRequest::RecordAcceptance {
                auth,
                handle,
                identity,
                result_version,
                result_digest,
                expected_control_epoch,
            } => {
                return self.record_acceptance(
                    auth,
                    handle,
                    identity,
                    result_version,
                    result_digest,
                    expected_control_epoch,
                );
            }
            DelegationRequest::ConfirmHumanReturn {
                auth,
                handle,
                request,
                returned_request,
                expected_control_epoch,
                evidence,
            } => {
                return self
                    .confirm_human_return(
                        auth,
                        handle,
                        request,
                        returned_request,
                        expected_control_epoch,
                        *evidence,
                    )
                    .map_err(|error| lifecycle_error(ErrorCode::Conflict, format!("{error:#}")));
            }
            DelegationRequest::Inspect { auth, handle } => {
                return self.assignment_snapshot(self.owned_assignment(&auth, &handle)?);
            }
            DelegationRequest::Reconcile { auth, request } => {
                return self.reconciliation_snapshot(self.owned_assignment(&auth, &request)?);
            }
            DelegationRequest::Collect { auth, handle } => {
                let mut record = self.owned_assignment(&auth, &handle)?;
                if let Some(publication) = self
                    .assignment_publication(&record)
                    .map_err(publication_error)?
                    && record.result_digest.is_none()
                {
                    record.result_digest = Some(publication.digest);
                    self.retained.lock().save_assignment(&record)?;
                }
                return self.assignment_snapshot(record);
            }
            DelegationRequest::MarkHandled {
                auth,
                handle,
                publication,
            } => {
                let mut record = self.owned_assignment(&auth, &handle)?;
                let parent = self.delegation_auth(&auth)?;
                let child = self.assignment_session(&record)?;
                // Reporter binding replacements use this same retained -> Session order.
                // Keep the verified controller/child generations stable through the write.
                let store = self.retained.lock();
                if parent.run() != record.parent_run
                    || parent.summary().agent.as_ref().map(|a| &a.binding)
                        != Some(&record.parent_binding)
                    || Some(child.run()) != record.identity.run
                    || child.summary().agent.as_ref().map(|a| &a.binding) != record.binding.as_ref()
                {
                    bail!("assignment controller or child conversation changed");
                }
                let current = self
                    .assignment_publication(&record)
                    .map_err(publication_error)?;
                if current.as_ref() != Some(&publication) {
                    bail!("handled identity does not match current publication");
                }
                record.result_digest = Some(publication.digest.clone());
                record.handled_publication = Some(publication);
                store.save_assignment(&record)?;
                drop(store);
                if let Some(session) = record.identity.session {
                    self.delegation_observations.invalidate(session);
                }
                return self.assignment_snapshot(record);
            }
            DelegationRequest::Observe { .. } => {
                bail!("Observe must use the bounded control observation path")
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
                if submission.version != record.result_version {
                    bail!("submission belongs to a stale result version");
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
                let publication = PublicationIdentity {
                    assignment: record.identity.clone(),
                    kind: PublicationKind::Submission,
                    version: submission.version,
                    digest: hash.clone(),
                };
                if record.publication_intent.as_ref().is_some_and(|intent| {
                    intent.identity != publication || intent.binding != binding
                }) {
                    bail!("submission version has a different authenticated publication intent");
                }
                let store = self.retained.lock();
                session.with_delegation_binding(&auth.capability, &binding, || {
                    record.publication_intent = Some(PublicationIntent {
                        identity: publication,
                        binding: binding.clone(),
                    });
                    store.save_assignment(&record)?;
                    let directory = Directory::open(&record.root, false)?;
                    if let Err(error) = directory.publish(
                        &format!("submission-v{}.json", record.result_version),
                        &bytes,
                    ) {
                        if directory
                            .read(&format!("submission-v{}.json", record.result_version))
                            .ok()
                            .as_ref()
                            != Some(&bytes)
                        {
                            return Err(error);
                        }
                        directory.sync()?;
                    }
                    record.result_digest = Some(hash);
                    store.save_assignment(&record)
                })?;
                drop(store);
                self.delegation_observations.invalidate(auth.session);
                record
            }
        };
        self.reconciliation_snapshot(record)
    }

    /// Authorship comes from durable child authorization, never parseable file presence.
    pub(super) fn assignment_publication(
        &self,
        record: &Record,
    ) -> std::result::Result<Option<PublicationIdentity>, PublicationIssue> {
        let directory =
            Directory::open(&record.root, false).map_err(|_| PublicationIssue::Unavailable)?;
        let expected = if let Some(intent) = &record.publication_intent {
            if intent.identity.assignment != record.identity
                || intent.identity.version != record.result_version
                || intent.identity.kind != PublicationKind::Submission
                || record.binding.as_ref() != Some(&intent.binding)
                || record
                    .result_digest
                    .as_ref()
                    .is_some_and(|d| d != &intent.identity.digest)
            {
                return Err(PublicationIssue::Invalid);
            }
            Some(intent.identity.clone())
        } else {
            record
                .result_digest
                .as_ref()
                .map(|digest| PublicationIdentity {
                    assignment: record.identity.clone(),
                    kind: PublicationKind::Submission,
                    version: record.result_version,
                    digest: digest.clone(),
                })
        };
        let bytes = match directory.read(&format!("submission-v{}.json", record.result_version)) {
            Ok(bytes) => bytes,
            Err(error)
                if expected.is_none()
                    && error.chain().any(|e| {
                        e.downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                    }) =>
            {
                return Ok(None);
            }
            Err(_) => return Err(PublicationIssue::Unavailable),
        };
        let expected = expected.ok_or(PublicationIssue::Uncommitted)?;
        if digest(&bytes) != expected.digest {
            return Err(PublicationIssue::Invalid);
        }
        let mut version = record.version();
        version.result_digest = Some(expected.digest.clone());
        read_version_submission(record, &version).map_err(|_| PublicationIssue::Invalid)?;
        // A previous link may have succeeded while its directory fsync failed.
        if record.result_digest.is_none() {
            directory
                .sync()
                .map_err(|_| PublicationIssue::Unavailable)?;
        }
        Ok(Some(expected))
    }

    fn assignment_session(
        &self,
        record: &Record,
    ) -> Result<std::sync::Arc<crate::session::Session>> {
        let id = record.identity.session.context("assignment has no child")?;
        let session = self.session_for_control(id)?;
        if Some(session.run()) != record.identity.run {
            return Err(lifecycle_error(ErrorCode::Conflict, "assigned run changed"));
        }
        let summary = session.summary();
        if let Some(binding) = &record.binding
            && summary.agent.as_ref().map(|agent| &agent.binding) != Some(binding)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "child conversation binding changed",
            ));
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
            &serde_json::json!({"schema": 1, "handle": handle, "identity": record.identity, "contract": contract, "result_version": 1, "output": record.root.join("submission-v1.json"), "publication": "Call ovrcr_publish. Only this child can publish; it cannot accept the result. Research tools are read-only."}),
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
                let _ = expected;
                read_version_submission(&record, &record.version())?
            }
            None => None,
        };
        followups::verify_acceptance(&record, &record.version())?;
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
        let version = record.version();
        let publication = if submission.is_some() {
            record.version_reference(&version).publication
        } else {
            None
        };
        let history = record
            .previous_versions
            .iter()
            .map(|v| record.version_reference(v))
            .collect();
        let brief = record
            .root
            .join(format!("brief-v{}.json", record.result_version));
        let result_path = record
            .root
            .join(format!("submission-v{}.json", record.result_version));
        let control_evidence = summary
            .as_ref()
            .and_then(|summary| summary.agent.as_ref())
            .and_then(|agent| self.control_evidence(&record, agent).ok());
        Ok(AssignmentSnapshot {
            control: record.control.clone(),
            control_evidence,
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
            brief,
            result_path,
            submission,
            acceptance: if record.control.attribution_invalidated {
                "suspended"
            } else if record.acceptance.is_some() {
                "accepted"
            } else {
                "pending"
            }
            .into(),
            result_version: record.result_version,
            publication,
            acceptance_ref: record.acceptance,
            history,
            selected_version: None,
            historical_submission: None,
            receipts: record.receipts,
            resources,
            retryable_capacity_refusal: record.retryable_capacity_refusal,
            successor_request: record.successor_request,
            execution_attempts: record.execution_attempts,
        })
    }
}

fn publication_error(issue: PublicationIssue) -> anyhow::Error {
    anyhow::anyhow!(match issue {
        PublicationIssue::Invalid => "submission publication changed or is partial",
        PublicationIssue::Unavailable =>
            "submission publication unavailable; authenticated intent is retained",
        PublicationIssue::Uncommitted => "publication has no authenticated child intent",
    })
}

/// Preserve the exact older canonical bytes when the only added fields were
/// absent defaults. A different contract never acquires the old digest.
fn legacy_contract_bytes(contract: &ResearchContract) -> Result<Option<Vec<u8>>> {
    if contract.limits != DelegationLimits::default() || contract.previous_request.is_some() {
        return Ok(None);
    }
    let bytes = serde_json::to_vec(contract)?;
    let suffix = format!(
        ",\"limits\":{},\"previous_request\":null}}",
        serde_json::to_string(&DelegationLimits::default())?
    );
    Ok(bytes.strip_suffix(suffix.as_bytes()).map(|prefix| {
        let mut legacy = prefix.to_vec();
        legacy.push(b'}');
        legacy
    }))
}

fn contract_bytes(contract: &ResearchContract, expected: Option<&str>) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(contract)?;
    if let Some(expected) = expected
        && digest(&bytes) != expected
        && let Some(legacy) = legacy_contract_bytes(contract)?
        && digest(&legacy) == expected
    {
        return Ok(legacy);
    }
    Ok(bytes)
}

fn read_contract(record: &Record) -> Result<ResearchContract> {
    let bytes = Directory::open(&record.root, false)?
        .read(&format!("contract-v{}.json", record.identity.contract))?;
    if digest(&bytes) != record.contract_digest {
        bail!("immutable contract changed");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn available_models() -> Result<Vec<DelegationChoice>> {
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
    let mut choices = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(backend), Some(model)) = (fields.next(), fields.next()) else {
            continue;
        };
        if backend == "provider" && model == "model" {
            continue;
        }
        if !DelegationSettings::valid_model(&format!("{backend}/{model}")) {
            continue;
        }
        choices.push(DelegationChoice {
            provider: DelegationHarness::Pi,
            model_provider: backend.into(),
            model: model.into(),
        });
    }
    if serde_json::to_vec(&choices)?.len() > 512 * 1024 {
        bail!("provider inventory exceeds response bound");
    }
    Ok(choices)
}

fn available_model(contract: &ResearchContract) -> Result<()> {
    if !available_models()?.iter().any(|choice| {
        choice.model_provider == contract.model_provider && choice.model == contract.model
    }) {
        bail!("requested exact model is unavailable");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn record(root: PathBuf, status: DispatchStatus) -> Record {
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
            limits: DelegationLimits::default(),
            previous_request: None,
        };
        let bytes = serde_json::to_vec(&contract).unwrap();
        Directory::open(&root, true)
            .unwrap()
            .publish("contract-v1.json", &bytes)
            .unwrap();
        let allocated = status != DispatchStatus::Prepared;
        Record {
            control: AssignmentControl::default(),
            takeover_evidence: None,
            control_intents: vec![],
            initial_control_epoch: None,
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
            result_version: 1,
            publication_intent: None,
            handled_publication: None,
            acceptance: None,
            previous_versions: Vec::new(),
            followups: Vec::new(),
            dispatch: status,
            reason: None,
            project: String::new(),
            workspace: String::new(),
            receipts: vec![],
            request_digests: vec![],
            previous_request: None,
            successor_request: None,
            execution_attempts: None,
            retryable_capacity_refusal: false,
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

    fn operation(value: &mut Record, operation: DispatchOperation, outcome: DispatchOutcome) {
        let mut identity = value.identity.clone();
        if operation != DispatchOperation::InitialInput {
            identity.session = None;
            identity.run = None;
        }
        // Test fixtures start pending before allocation, then simulate persisted outcomes.
        value.request_digests.retain(|(kind, _)| *kind != operation);
        value.receipts.retain(|r| r.operation != operation);
        value.receipts.push(DispatchReceipt {
            request: value.handle.clone(),
            operation,
            result_version: 1,
            identity: identity.clone(),
            outcome,
            input: None,
            reason: None,
        });
        value.request_digests.push((
            operation,
            digest(&serde_json::to_vec(&(operation, &value.contract_digest, &identity)).unwrap()),
        ));
    }

    #[test]
    fn execution_accounting_uses_receipts_and_never_resource_absence() {
        let cases = [
            ("raw prepared", Some(0)),
            ("accepted launch", Some(0)),
            ("prepare unknown", None),
            ("unknown launch without row", Some(1)),
            ("partial launch without row", Some(1)),
            ("applied launch", Some(1)),
            ("rejected input after launch", Some(1)),
            ("refused without proof", None),
            ("rejected without proof", None),
            ("known preflight refusal", Some(0)),
            ("allocated preflight refusal", Some(0)),
            ("known capacity refusal", Some(0)),
            ("zero contradicts unknown", None),
            ("zero contradicts input", None),
            ("prepared contradicts launch", None),
            ("unpaired allocation", None),
            ("wrong receipt identity", None),
            ("wrong operation digest", None),
            ("duplicate receipt", None),
            ("missing predecessor count", None),
            ("known inherited count", Some(2)),
            ("known count after removal", Some(2)),
            ("invalid legacy contract", None),
            ("contradictory applied binding", None),
        ];
        for (name, expected) in cases {
            let directory = tempfile::tempdir().unwrap();
            let mut value = record(
                directory.path().join("assignment"),
                DispatchStatus::Prepared,
            );
            match name {
                "raw prepared" => {}
                "accepted launch"
                | "prepare unknown"
                | "wrong receipt identity"
                | "wrong operation digest"
                | "duplicate receipt" => {
                    operation(
                        &mut value,
                        DispatchOperation::Prepare,
                        DispatchOutcome::Applied,
                    );
                    operation(
                        &mut value,
                        DispatchOperation::Launch,
                        DispatchOutcome::Accepted,
                    );
                    match name {
                        "prepare unknown" => operation(
                            &mut value,
                            DispatchOperation::Prepare,
                            DispatchOutcome::Unknown,
                        ),
                        "wrong receipt identity" => {
                            value.receipts[0].identity.task = "other".into()
                        }
                        "wrong operation digest" => value.request_digests[0].1 = "changed".into(),
                        "duplicate receipt" => value.receipts.push(value.receipts[0].clone()),
                        _ => {}
                    }
                }
                "unknown launch without row"
                | "partial launch without row"
                | "zero contradicts unknown" => {
                    value.dispatch = DispatchStatus::LaunchUnknown;
                    operation(
                        &mut value,
                        DispatchOperation::Launch,
                        if name == "partial launch without row" {
                            DispatchOutcome::Partial
                        } else {
                            DispatchOutcome::Unknown
                        },
                    );
                    if name == "zero contradicts unknown" {
                        value.execution_attempts = Some(0);
                    }
                }
                "applied launch" | "rejected input after launch" | "zero contradicts input" => {
                    value.dispatch = DispatchStatus::Launched;
                    value.identity.session = Some(SessionId(2));
                    value.identity.run = Some(SessionRunId(1));
                    operation(
                        &mut value,
                        DispatchOperation::Launch,
                        DispatchOutcome::Applied,
                    );
                    if name != "applied launch" {
                        operation(
                            &mut value,
                            DispatchOperation::InitialInput,
                            DispatchOutcome::Rejected,
                        );
                        value.dispatch = DispatchStatus::Refused;
                    }
                    if name == "zero contradicts input" {
                        value.execution_attempts = Some(0);
                    }
                }
                "refused without proof"
                | "rejected without proof"
                | "known preflight refusal"
                | "allocated preflight refusal"
                | "known capacity refusal" => {
                    value.dispatch = DispatchStatus::Refused;
                    if name != "refused without proof" {
                        operation(
                            &mut value,
                            DispatchOperation::Launch,
                            DispatchOutcome::Rejected,
                        );
                    }
                    if name.contains("preflight") || name == "known capacity refusal" {
                        value.execution_attempts = Some(0);
                    }
                    if name == "allocated preflight refusal" {
                        value.identity.session = Some(SessionId(2));
                        value.identity.run = Some(SessionRunId(1));
                    }
                    value.retryable_capacity_refusal = name == "known capacity refusal";
                }
                "prepared contradicts launch" => operation(
                    &mut value,
                    DispatchOperation::Launch,
                    DispatchOutcome::Unknown,
                ),
                "unpaired allocation" => value.identity.run = Some(SessionRunId(1)),
                "missing predecessor count" | "known inherited count" => {
                    value.identity.attempt = 2;
                    value.previous_request = Some("previous".into());
                    if name == "known inherited count" {
                        value.execution_attempts = Some(2);
                    }
                }
                "known count after removal" => {
                    value.dispatch = DispatchStatus::Refused;
                    value.execution_attempts = Some(2);
                }
                "invalid legacy contract" => {
                    std::fs::write(value.root.join("contract-v1.json"), b"{}").unwrap()
                }
                "contradictory applied binding" => {
                    value.dispatch = DispatchStatus::Applied;
                    value.identity.session = Some(SessionId(2));
                    value.identity.run = Some(SessionRunId(1));
                }
                _ => unreachable!(),
            }
            assert_eq!(value.spent_execution_attempts(), expected, "{name}");
            value.upgrade_legacy();
            assert_eq!(
                value.execution_attempts, expected,
                "derive before synthesized receipts: {name}"
            );
        }
    }

    #[test]
    fn same_child_versions_keep_one_execution_and_initial_receipt_anchors() {
        let dir = tempfile::tempdir().unwrap();
        let mut value = record(dir.path().join("assignment"), DispatchStatus::Applied);
        for op in [
            DispatchOperation::Prepare,
            DispatchOperation::Launch,
            DispatchOperation::InitialInput,
        ] {
            operation(&mut value, op, DispatchOutcome::Applied);
        }
        value.execution_attempts = Some(1);
        let initial_receipts = value.receipts.clone();
        let initial_digests = value.request_digests.clone();
        for version in 2..=65 {
            let expected = value.identity.clone();
            let mut target = expected.clone();
            if version % 2 == 0 {
                target.contract += 1;
            }
            let request = format!("followup-{version}");
            value.followups.push(FollowupIntent {
                request: request.clone(),
                expected,
                expected_result_version: version - 1,
                expected_control_epoch: 1,
                target: target.clone(),
                result_version: version,
                digest: "payload".into(),
                contract_digest: format!("contract-{version}"),
                brief_digest: "brief".into(),
            });
            value.receipts.push(DispatchReceipt {
                request,
                operation: DispatchOperation::Followup,
                result_version: version,
                identity: target.clone(),
                outcome: DispatchOutcome::Applied,
                input: None,
                reason: None,
            });
            value.previous_versions.push(value.version());
            value.identity = target;
            value.result_version = version;
            value.contract_digest = format!("contract-{version}");
            value.brief_digest = Some("brief".into());
            assert_eq!(
                value.spent_execution_attempts(),
                Some(1),
                "version {version}"
            );
            value.upgrade_legacy();
            assert_eq!(value.execution_attempts, Some(1));
            assert_eq!(&value.receipts[..3], initial_receipts.as_slice());
            assert_eq!(value.request_digests, initial_digests);
        }
        for index in 0..64 {
            value
                .record_control(
                    &format!("control-{index}"),
                    match index % 3 {
                        0 => DispatchOperation::Takeover,
                        1 => DispatchOperation::HumanReturn,
                        _ => DispatchOperation::ConfirmHumanReturn,
                    },
                    &index,
                )
                .unwrap();
            assert_eq!(
                value.spent_execution_attempts(),
                Some(1),
                "control operation {index}"
            );
        }
        let last_followup = value
            .receipts
            .iter()
            .rposition(|r| r.operation == DispatchOperation::Followup)
            .unwrap();
        for outcome in [
            DispatchOutcome::Unknown,
            DispatchOutcome::Partial,
            DispatchOutcome::Rejected,
        ] {
            value.receipts[last_followup].outcome = outcome;
            value.dispatch = DispatchStatus::Refused;
            assert_eq!(value.spent_execution_attempts(), Some(1));
        }
        let mut zero = value.clone();
        zero.execution_attempts = Some(0);
        assert_eq!(zero.spent_execution_attempts(), None);
        let mut malformed = value.clone();
        malformed.receipts.last_mut().unwrap().identity.contract += 1;
        assert_eq!(malformed.spent_execution_attempts(), None);
        let mut malformed = value.clone();
        malformed.previous_versions[0].result_version = 9;
        assert_eq!(malformed.spent_execution_attempts(), None);
        let mut malformed = value.clone();
        malformed.request_digests[0].1 = value.contract_digest.clone();
        assert_eq!(malformed.spent_execution_attempts(), None);
        value.receipts.push(value.receipts.last().unwrap().clone());
        assert_eq!(value.spent_execution_attempts(), None);
    }

    #[test]
    fn reserved_gaps_require_rejected_intents_and_exact_materialized_lineage() {
        let dir = tempfile::tempdir().unwrap();
        let mut value = record(dir.path().join("assignment"), DispatchStatus::Applied);
        for op in [
            DispatchOperation::Prepare,
            DispatchOperation::Launch,
            DispatchOperation::InitialInput,
        ] {
            operation(&mut value, op, DispatchOutcome::Applied);
        }
        value.execution_attempts = Some(1);
        // Reserve/reject revision2, continue unchanged at result3, then revise
        // to contract3/result4. Only the actual results enter version history.
        for (result, contract, outcome, advance) in [
            (2, 2, DispatchOutcome::Rejected, false),
            (3, 1, DispatchOutcome::Applied, true),
            (4, 3, DispatchOutcome::Applied, true),
        ] {
            let expected = value.identity.clone();
            let mut target = expected.clone();
            target.contract = contract;
            let request = format!("reserved-{result}");
            let contract_digest = if contract == expected.contract {
                value.contract_digest.clone()
            } else {
                format!("contract-{contract}")
            };
            let brief_digest = format!("brief-{result}");
            value.followups.push(FollowupIntent {
                request: request.clone(),
                expected,
                expected_result_version: value.result_version,
                expected_control_epoch: 1,
                target: target.clone(),
                result_version: result,
                digest: "payload".into(),
                contract_digest: contract_digest.clone(),
                brief_digest: brief_digest.clone(),
            });
            value.receipts.push(DispatchReceipt {
                request,
                operation: DispatchOperation::Followup,
                result_version: result,
                identity: target.clone(),
                outcome,
                input: None,
                reason: None,
            });
            if advance {
                value.previous_versions.push(value.version());
                value.identity = target;
                value.result_version = result;
                value.contract_digest = contract_digest;
                value.brief_digest = Some(brief_digest);
            }
            assert_eq!(value.spent_execution_attempts(), Some(1));
        }
        let mut restored: Record =
            serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        restored.upgrade_legacy();
        assert_eq!(
            restored.execution_attempts,
            Some(1),
            "retained metadata reload keeps proof"
        );
        for case in [
            "unbacked result",
            "unbacked contract",
            "duplicate reservation",
            "duplicate request",
            "unknown skipped",
            "partial skipped",
            "accepted skipped",
            "missing materialization",
            "changed digest",
            "changed receipt",
            "zero count",
        ] {
            let mut bad = value.clone();
            match case {
                "unbacked result" => {
                    bad.followups.remove(0);
                    bad.receipts.remove(3);
                }
                "unbacked contract" => {
                    bad.followups[0].target.contract = 1;
                    bad.receipts[3].identity.contract = 1;
                }
                "duplicate reservation" => bad.followups[1].result_version = 2,
                "duplicate request" => bad.followups[1].request = bad.followups[0].request.clone(),
                "unknown skipped" => bad.receipts[3].outcome = DispatchOutcome::Unknown,
                "partial skipped" => bad.receipts[3].outcome = DispatchOutcome::Partial,
                "accepted skipped" => bad.receipts[3].outcome = DispatchOutcome::Accepted,
                "missing materialization" => {
                    bad.previous_versions.remove(1);
                }
                "changed digest" => bad.previous_versions[1].brief_digest = Some("foreign".into()),
                "changed receipt" => bad.receipts[4].identity.contract = 2,
                "zero count" => bad.execution_attempts = Some(0),
                _ => unreachable!(),
            }
            assert_eq!(bad.spent_execution_attempts(), None, "{case}");
            bad.upgrade_legacy();
            assert_eq!(bad.execution_attempts, None, "{case}");
        }
    }

    #[test]
    fn old_contract_bytes_preserve_immutable_digest_without_weakening_comparison() {
        let directory = tempfile::tempdir().unwrap();
        let value = record(
            directory.path().join("assignment"),
            DispatchStatus::Prepared,
        );
        let contract = read_contract(&value).unwrap();
        let modern = serde_json::to_vec(&contract).unwrap();
        let suffix = format!(
            ",\"limits\":{},\"previous_request\":null}}",
            serde_json::to_string(&DelegationLimits::default()).unwrap()
        );
        let mut old = modern.strip_suffix(suffix.as_bytes()).unwrap().to_vec();
        old.push(b'}');
        let old_digest = digest(&old);
        assert_eq!(contract_bytes(&contract, Some(&old_digest)).unwrap(), old);
        let mut changed = contract.clone();
        changed.scope.push_str(" changed");
        assert_ne!(
            digest(&contract_bytes(&changed, Some(&old_digest)).unwrap()),
            old_digest
        );
        changed = contract;
        changed.limits.max_execution_attempts = Some(1);
        assert_ne!(
            digest(&contract_bytes(&changed, Some(&old_digest)).unwrap()),
            old_digest
        );
    }
}
