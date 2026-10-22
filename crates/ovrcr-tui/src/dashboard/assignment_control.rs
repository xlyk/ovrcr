//! Human control is an explicit Server transition. Focus and view changes only
//! consume this projection; they never issue a transition themselves.
use super::state::find_session;
use super::{Dashboard, DashboardAction, InputMode};
use ovrcr_protocol::{
    AssignmentControl, AssignmentControlRequest, ClientMessage, ControlBarrier, ControlOwner,
    Request, SessionPhase, SessionSummary,
};

pub(super) fn input_denial(control: Option<&AssignmentControl>) -> Option<&'static str> {
    let control = control?;
    match control.owner {
        ControlOwner::Human if control.barrier.is_none() => None,
        ControlOwner::Human => Some("assignment control requires reconciliation"),
        ControlOwner::Controller => Some("Pi controls this assignment; choose Take control first"),
        ControlOwner::Uncertain => Some("assignment control is uncertain; automation is suspended"),
    }
}

pub(super) fn session_input_denial(summary: &SessionSummary) -> Option<&'static str> {
    let view = summary.assignment_control.as_ref()?;
    if view.evidence.is_none() {
        return Some("current assignment binding is unavailable");
    }
    input_denial(Some(&view.control))
}

pub(super) fn control_clause(control: Option<&AssignmentControl>) -> Option<&'static str> {
    let control = control?;
    Some(match control.owner {
        ControlOwner::Human => "Human control",
        ControlOwner::Uncertain => "Control uncertain",
        ControlOwner::Controller => match control.barrier {
            Some(ControlBarrier::HumanReturn { .. }) => "Pi return pending reconciliation",
            Some(ControlBarrier::Interrupt { .. }) => "Pi interrupt pending reconciliation",
            Some(_) => "Pi control suspended",
            None if control.attribution_invalidated => "Pi control; fresh result version required",
            None => "Pi control · view only",
        },
    })
}

pub(super) fn transition_denial(
    session: Option<&SessionSummary>,
    take: bool,
) -> Option<&'static str> {
    let Some(session) = session else {
        return Some("no session selected");
    };
    if session.phase != SessionPhase::Running {
        return Some("session is not running");
    }
    let Some(view) = &session.assignment_control else {
        return Some("session is not assigned");
    };
    if view.evidence.is_none() {
        return Some("current assignment binding is unavailable");
    }
    match (&view.control.owner, take) {
        (ControlOwner::Controller, true) if view.control.barrier.is_none() => None,
        (ControlOwner::Human, false) if view.control.barrier.is_none() => None,
        (ControlOwner::Uncertain, _) => Some("assignment control is uncertain"),
        (_, _) if view.control.barrier.is_some() => {
            Some("assignment control requires reconciliation")
        }
        (ControlOwner::Human, true) => Some("you already control this assignment"),
        (ControlOwner::Controller, false) => Some("Pi already controls this assignment"),
        _ => Some("assignment control is unavailable"),
    }
}

pub(super) fn older_control(incoming: &SessionSummary, current: &SessionSummary) -> bool {
    let rank = |summary: &SessionSummary| {
        summary
            .assignment_control
            .as_ref()
            .map(|view| (view.control.epoch, view.control.human_input_revision))
    };
    rank(incoming) < rank(current)
}

impl Dashboard {
    pub(super) fn assignment_control_changed(&mut self, incoming: &SessionSummary) {
        let changed = find_session(self, incoming.id).is_some_and(|current| {
            current.run == incoming.run
                && current
                    .assignment_control
                    .as_ref()
                    .map(|v| (&v.control.owner, v.control.epoch))
                    != incoming
                        .assignment_control
                        .as_ref()
                        .map(|v| (&v.control.owner, v.control.epoch))
        });
        if self.focused_session() == Some(incoming.id) && changed {
            self.mode = InputMode::Browse;
            self.agent_typing = None;
            self.mouse.held = Default::default();
            self.mouse.last_motion = None;
            self.outbox.discard_input();
            self.navigation.input_epoch = self.navigation.input_epoch.wrapping_add(1);
        }
    }

    pub(super) fn assignment_control_request(&mut self, take: bool) -> DashboardAction {
        let selected = self
            .action_session()
            .and_then(|id| find_session(self, id))
            .cloned();
        if let Some(reason) = transition_denial(selected.as_ref(), take) {
            self.set_error(reason);
            return DashboardAction::Redraw;
        }
        let session = selected.expect("validated selected session");
        if !self.focused_pane().is_some_and(|pane| {
            pane.session == Some(session.id)
                && pane.run == Some(session.run)
                && self.pane_ready(pane)
        }) {
            self.set_error("waiting for acknowledged screen");
            return DashboardAction::Redraw;
        }
        let view = session.assignment_control.expect("validated assignment");
        let request_id = self.error_owning_request_id();
        let request = format!(
            "dashboard-{request_id}-{}-{}-{}",
            session.id.0, session.run.0, view.control.epoch
        );
        let transition = if take {
            AssignmentControlRequest::Takeover {
                request,
                session: session.id,
                run: session.run,
                expected_epoch: view.control.epoch,
                view_revision: self.view_revision(),
            }
        } else {
            AssignmentControlRequest::Return {
                request,
                session: session.id,
                run: session.run,
                expected_epoch: view.control.epoch,
                view_revision: self.view_revision(),
                evidence: Box::new(view.evidence.expect("validated current evidence")),
            }
        };
        self.mode = InputMode::Browse;
        self.agent_typing = None;
        self.cancel_mouse_gesture();
        self.outbox.discard_input();
        self.navigation.input_epoch = self.navigation.input_epoch.wrapping_add(1);
        DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::AssignmentControl(transition),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_human_ownership_enables_assigned_input() {
        let mut control = AssignmentControl::default();
        assert!(
            input_denial(Some(&control))
                .unwrap()
                .contains("Take control")
        );
        control.owner = ControlOwner::Human;
        assert_eq!(input_denial(Some(&control)), None);
        control.owner = ControlOwner::Uncertain;
        assert!(input_denial(Some(&control)).is_some());
        assert_eq!(
            input_denial(None),
            None,
            "unassigned sessions keep ordinary input"
        );
    }

    #[test]
    fn pending_return_is_visible_and_never_grants_input() {
        let mut control = AssignmentControl {
            barrier: Some(ControlBarrier::HumanReturn {
                request: "return".into(),
            }),
            ..AssignmentControl::default()
        };
        assert_eq!(
            control_clause(Some(&control)),
            Some("Pi return pending reconciliation")
        );
        assert!(input_denial(Some(&control)).is_some());
        control.barrier = Some(ControlBarrier::Interrupt {
            request: "interrupt".into(),
        });
        assert_eq!(
            control_clause(Some(&control)),
            Some("Pi interrupt pending reconciliation")
        );
        assert!(input_denial(Some(&control)).is_some());
        control.barrier = None;
        control.attribution_invalidated = true;
        assert_eq!(
            control_clause(Some(&control)),
            Some("Pi control; fresh result version required")
        );
        assert!(input_denial(Some(&control)).is_some());
    }
}
