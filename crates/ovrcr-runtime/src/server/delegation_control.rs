//! Per-assignment authority. Callers persist each transition while holding the
//! retained store through the Session state/writer fence.
use anyhow::{Result, bail};
use ovrcr_protocol::{
    AssignmentControl, AssignmentControlEvidence, ControlBarrier, ControlOwner,
};

fn next_epoch(control: &AssignmentControl, expected: u64) -> Result<u64> {
    if control.epoch != expected {
        bail!("assignment control epoch changed");
    }
    control.epoch.checked_add(1).ok_or_else(|| anyhow::anyhow!("assignment control epoch exhausted"))
}

pub(super) fn require_controller(control: &AssignmentControl, expected: u64) -> Result<()> {
    if control.epoch != expected
        || control.owner != ControlOwner::Controller
        || control.barrier.is_some()
    {
        bail!("assignment controller input is fenced; reconcile current control ownership");
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
    if control.epoch != expected || control.owner != ControlOwner::Human || control.barrier.is_some() {
        bail!("human input requires current explicit takeover");
    }
    control.human_input_revision = control.human_input_revision.checked_add(1)
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
    if takeover.identity != current.identity || takeover.binding != current.binding
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
    control.barrier = Some(ControlBarrier::HumanReturn { request: request.to_owned() });
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

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{AgentBinding, AgentProvider, AssignmentIdentity, SessionId, SessionRunId};

    fn evidence() -> AssignmentControlEvidence {
        AssignmentControlEvidence {
            server_lifetime: "server-one".into(),
            identity: AssignmentIdentity {
                instance: "instance".into(), workflow: "workflow".into(),
                controller: "parent".into(), controller_epoch: 7, task: "task".into(),
                attempt: 1, contract: 1, request: "prepare".into(),
                session: Some(SessionId(2)), run: Some(SessionRunId(3)),
            },
            binding: AgentBinding {
                provider: AgentProvider::Pi, invocation: "invocation".into(), conversation: "child".into(), generation: 1,
            },
            result_version: 1, activity_revision: 1, input_revision: 0,
            publication: None, publication_issue: None, human_input_revision: 0,
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
        for epoch in 1..4 { assert!(require_controller(&control, epoch).is_err()); }
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
        assert!(control.attribution_invalidated, "confirmation does not recreate result attribution");
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
        let mut control = AssignmentControl { epoch: u64::MAX, ..AssignmentControl::default() };
        let before = control.clone();
        assert!(take_control(&mut control, u64::MAX).is_err());
        assert_eq!(control, before);
    }
}
