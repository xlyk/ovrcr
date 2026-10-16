//! Immutable cancellation component for the sole durable workflow row.
use anyhow::{Result, bail};
use ovrcr_protocol::{AssignmentIdentity, DispatchOutcome};
#[cfg(test)]
use ovrcr_protocol::{DispatchOperation, DispatchReceipt};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Integration supplies the sole workflow owner's authority type. Validation
/// checks the decoded historical snapshot, not equality with today's owner.
pub(crate) trait CancellationAuthority: Clone + PartialEq {
    fn validate_capture(&self) -> Result<()>;
    fn instance(&self) -> &str;
    fn workflow(&self) -> &str;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancellationTarget {
    pub handle: String,
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub control_epoch: u64,
    pub interrupt_request: String,
    /// Frozen no-effect refusal when the original child journal cannot admit an
    /// interrupt. Never infer it from future mutable membership or receipts.
    #[serde(default)]
    pub refusal: Option<String>,
}

/// Certifies dispatch closure only. Target delivery receipts remain in each
/// assignment's existing journal and never imply settlement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancellationReceipt {
    pub request: String,
    pub digest: String,
    pub dispatch_closed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowCancellation<A> {
    pub captured_authority: A,
    pub receipt: CancellationReceipt,
    pub targets: Vec<CancellationTarget>,
    #[serde(default)]
    pub gaps: Vec<String>,
    #[serde(default)]
    pub unverifiable_members: u32,
}

impl<A: CancellationAuthority> WorkflowCancellation<A> {
    pub(crate) fn validate(&self) -> Result<()> {
        self.captured_authority.validate_capture()?;
        if !request_id(&self.receipt.request)
            || !valid_digest(&self.receipt.digest)
            || !self.receipt.dispatch_closed
            || self.targets.len() > 4096
            || self.unverifiable_members as usize + self.targets.len() > 4096
            || self.gaps.len() > 65
            || self
                .gaps
                .iter()
                .any(|gap| gap.is_empty() || gap.len() > 512)
            || (self.unverifiable_members > 0 && self.gaps.is_empty())
        {
            bail!("invalid bounded workflow cancellation capture");
        }
        let mut handles = HashSet::new();
        let mut requests = HashSet::new();
        for target in &self.targets {
            let identity = &target.identity;
            if !request_id(&target.handle)
                || identity.request != target.handle
                || !request_id(&target.interrupt_request)
                || target.interrupt_request == self.receipt.request
                || target.control_epoch == 0
                || target.result_version == 0
                || identity.attempt == 0
                || identity.contract == 0
                || identity.controller_epoch == 0
                || identity.session.is_some() != identity.run.is_some()
                || identity.session.is_some_and(|session| session.0 == 0)
                || identity.run.is_some_and(|run| run.0 == 0)
                || identity.instance != self.captured_authority.instance()
                || identity.workflow != self.captured_authority.workflow()
                || !handles.insert(&target.handle)
                || !requests.insert(&target.interrupt_request)
                || target
                    .refusal
                    .as_ref()
                    .is_some_and(|reason| reason.is_empty() || reason.len() > 256)
            {
                bail!("workflow cancellation targets are foreign, duplicated or invalid");
            }
        }
        Ok(())
    }
}

pub(crate) struct CancellationRequest<'a, A> {
    pub request: &'a str,
    pub digest: &'a str,
    pub expected_authority: &'a A,
}

/// Replay never rediscovers membership or authorizes another target effect.
pub(crate) enum CancellationAdmission<'a, A> {
    Installed(&'a WorkflowCancellation<A>),
    Replay(&'a WorkflowCancellation<A>),
}

impl<A: CancellationAuthority> CancellationAdmission<'_, A> {
    pub(crate) fn capture(&self) -> &WorkflowCancellation<A> {
        match self {
            Self::Installed(capture) | Self::Replay(capture) => capture,
        }
    }

    /// Caller additionally holds workflow/child authorization fences and saves
    /// Unknown before bytes. This checks receipt admission, not native ownership.
    #[cfg(test)]
    #[allow(dead_code)] // Used by the standalone pure-model integration suite.
    pub(crate) fn target_pending(
        &self,
        current_authority: &A,
        index: usize,
        receipt: &DispatchReceipt,
        operation: DispatchOperation,
    ) -> Result<bool> {
        let capture = self.capture();
        capture.validate()?;
        let target = capture
            .targets
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("cancellation target is outside the fixed capture"))?;
        if receipt.request != target.interrupt_request
            || receipt.identity != target.identity
            || receipt.result_version != target.result_version
            || receipt.operation != operation
        {
            bail!("cancellation target receipt does not match its fixed intent");
        }
        if matches!(self, Self::Replay(_)) {
            return Ok(false);
        }
        current_authority.validate_capture()?;
        if current_authority != &capture.captured_authority {
            bail!("workflow authority changed before cancellation effect");
        }
        Ok(receipt.outcome == DispatchOutcome::Accepted)
    }
}

/// Called on the existing WorkflowRecord's cancellation field under mutation
/// and retained locks. Commit row and target intents atomically before effects.
pub(crate) fn admit_cancellation<'a, A: CancellationAuthority>(
    slot: &'a mut Option<WorkflowCancellation<A>>,
    current_authority: &A,
    request: CancellationRequest<'_, A>,
    other_operation_requests: &[&str],
    discover: impl FnOnce() -> Result<Vec<CancellationTarget>>,
) -> Result<CancellationAdmission<'a, A>> {
    current_authority.validate_capture()?;
    if request.expected_authority != current_authority {
        bail!("workflow controller authority changed");
    }
    if !request_id(request.request) || !valid_digest(request.digest) {
        bail!("invalid cancellation request identity or digest");
    }
    if let Some(saved) = slot {
        saved.validate()?;
        if saved.captured_authority.instance() != current_authority.instance()
            || saved.captured_authority.workflow() != current_authority.workflow()
            || saved.receipt.request != request.request
            || saved.receipt.digest != request.digest
        {
            bail!("workflow cancellation already belongs to another intent");
        }
        return Ok(CancellationAdmission::Replay(saved));
    }
    if other_operation_requests.contains(&request.request) {
        bail!("request already identifies another workflow operation");
    }
    let capture = WorkflowCancellation {
        captured_authority: current_authority.clone(),
        receipt: CancellationReceipt {
            request: request.request.to_owned(),
            digest: request.digest.to_owned(),
            dispatch_closed: true,
        },
        targets: discover()?,
        gaps: Vec::new(),
        unverifiable_members: 0,
    };
    capture.validate()?;
    *slot = Some(capture);
    Ok(CancellationAdmission::Installed(slot.as_ref().unwrap()))
}

impl CancellationAuthority for ovrcr_protocol::WorkflowAuthority {
    fn validate_capture(&self) -> Result<()> {
        self.binding.validate()?;
        if self.epoch == 0
            || self.parent.0 == 0
            || self.parent_run.0 == 0
            || self.binding.provider != ovrcr_protocol::AgentProvider::Pi
            || [
                &self.instance,
                &self.workflow,
                &self.controller,
                &self.producer,
                &self.server_lifetime,
            ]
            .iter()
            .any(|id| !request_id(id))
        {
            bail!("invalid historical cancellation authority");
        }
        Ok(())
    }
    fn instance(&self) -> &str {
        &self.instance
    }
    fn workflow(&self) -> &str {
        &self.workflow
    }
}

/// Run on both new and resumed dispatch; transfer never removes this tombstone.
pub(crate) fn require_dispatch_open<A: CancellationAuthority>(
    cancellation: Option<&WorkflowCancellation<A>>,
) -> Result<()> {
    if let Some(capture) = cancellation {
        capture.validate()?;
        bail!("workflow is cancelled; new and resumed dispatch are closed");
    }
    Ok(())
}

fn request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn aggregate_cancellation(
    outcomes: impl IntoIterator<Item = DispatchOutcome>,
) -> DispatchOutcome {
    let mut delivered = false;
    let mut refused = false;
    for outcome in outcomes {
        match outcome {
            DispatchOutcome::Applied => delivered = true,
            DispatchOutcome::Rejected => refused = true,
            DispatchOutcome::Accepted | DispatchOutcome::Partial | DispatchOutcome::Unknown => {
                return DispatchOutcome::Unknown;
            }
        }
    }
    match (delivered, refused) {
        (true, true) => DispatchOutcome::Partial,
        (false, true) => DispatchOutcome::Rejected,
        _ => DispatchOutcome::Applied,
    }
}
