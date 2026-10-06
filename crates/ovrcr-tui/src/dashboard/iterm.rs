//! Explicit terminal-control setup. This never runs from notification enabling
//! or click navigation; the Server supplies the current active-owner context.
use super::desktop::BridgeControl;
use super::{Dashboard, DashboardAction};
use ovrcr_protocol::{
    BridgeActivationTarget, BridgeOperation, BridgeStatus, ClientMessage, Request, Response,
    ServerEvent, ServerMessage,
};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Setup {
    request: Option<u64>,
    target: Option<BridgeActivationTarget>,
    pending: Option<BridgeOperation>,
    in_flight: Option<BridgeControl>,
    poll_until: Option<Instant>,
    next_poll: Option<Instant>,
}

impl Dashboard {
    pub(super) fn fail_iterm_host(&mut self) {
        self.desktop.iterm = Setup::default();
        self.show_notice(ovrcr_protocol::ITermFocusStatus::Unavailable.guidance());
    }

    pub(super) fn iterm_control_pending(&self) -> bool {
        self.desktop.iterm.pending.is_some()
    }
    pub(super) fn begin_iterm_setup(&mut self) -> DashboardAction {
        if !self.settings.iterm_focus {
            self.show_notice(
                "iTerm focus off; enable Exact iTerm focus in Settings, then explicitly set up",
            );
            return DashboardAction::Redraw;
        }
        let setup = &mut self.desktop.iterm;
        if setup.request.is_some() || setup.pending.is_some() || setup.in_flight.is_some() {
            self.show_notice(
                "iTerm setup pending; respond to the separate macOS Automation prompt",
            );
            return DashboardAction::Redraw;
        }
        // When automatic observation expires, the same explicit action checks
        // status once. It cannot initiate a second authorization task.
        if let Some(target) = setup.target.clone()
            && setup
                .poll_until
                .is_some_and(|until| Instant::now() >= until)
        {
            setup.pending = Some(BridgeOperation::ITermStatus { target });
            self.show_notice("Checking iTerm setup; no new authorization request");
            return DashboardAction::Redraw;
        }
        let request_id = self.error_owning_request_id();
        self.desktop.iterm.request = Some(request_id);
        self.show_notice("Preparing current Dashboard iTerm setup");
        DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::PrepareITermFocus,
        })
    }

    pub(super) fn handle_iterm_setup_message(&mut self, message: &ServerMessage) -> bool {
        match message {
            ServerMessage::Response {
                request_id,
                response: Response::ITermFocusPrepared(target),
            } if self.desktop.iterm.request == Some(*request_id) => {
                self.desktop.iterm.request = None;
                self.error_owning_requests.remove(request_id);
                let target = target
                    .as_ref()
                    .filter(|target| {
                        self.settings.iterm_focus
                            && target.iterm_focus
                            && target.owner.as_ref().is_some_and(|owner| {
                                Some(&owner.context) == self.navigation.context.as_ref()
                            })
                            && target.iterm_session_id.is_some()
                    })
                    .cloned();
                if let Some(target) = target {
                    self.desktop.iterm.target = Some(target.clone());
                    self.desktop.iterm.pending = Some(BridgeOperation::ITermSetup { target });
                    self.desktop.iterm.poll_until = Some(Instant::now() + Duration::from_secs(30));
                    self.show_notice(
                        "iTerm setup pending; respond to the separate macOS Automation prompt",
                    );
                } else {
                    self.show_notice("Exact iTerm focus unavailable; use a local Dashboard in iTerm with the opt-in enabled");
                }
                true
            }
            ServerMessage::Event(ServerEvent::ITermFocus(status)) => {
                self.show_notice(status.guidance());
                true
            }
            _ => false,
        }
    }

    pub(super) fn cancel_iterm_setup_if_disabled(&mut self) {
        if self.settings.iterm_focus {
            return;
        }
        let setup = &mut self.desktop.iterm;
        if let Some(request) = setup.request.take() {
            self.ignored_responses.insert(request);
        }
        if let Some(control) = setup.in_flight.take() {
            control.cancel();
        }
        *setup = Setup::default();
    }

    pub(super) fn handle_iterm_bridge_update(
        &mut self,
        operation: &BridgeOperation,
        status: BridgeStatus,
    ) {
        let target = match operation {
            BridgeOperation::ITermSetup { target } | BridgeOperation::ITermStatus { target } => {
                target
            }
            _ => return,
        };
        if !self.settings.iterm_focus || self.desktop.iterm.target.as_ref() != Some(target) {
            return;
        }
        match status {
            BridgeStatus::ITermPending => {
                if self
                    .desktop
                    .iterm
                    .poll_until
                    .is_some_and(|until| Instant::now() < until)
                {
                    self.desktop.iterm.next_poll = Some(Instant::now() + Duration::from_secs(1));
                } else {
                    self.show_notice("iTerm setup unconfirmed; choose Set up iTerm focus to check once without another prompt");
                }
            }
            other => {
                self.desktop.iterm.poll_until = None;
                self.desktop.iterm.next_poll = None;
                self.desktop.iterm.target = None;
                self.show_notice(match other {
                    BridgeStatus::ITermAuthorized => {
                        ovrcr_protocol::ITermFocusStatus::Authorized.guidance()
                    }
                    BridgeStatus::ITermDenied => {
                        ovrcr_protocol::ITermFocusStatus::Denied.guidance()
                    }
                    BridgeStatus::ITermAuthorizationRequired => {
                        ovrcr_protocol::ITermFocusStatus::AuthorizationRequired.guidance()
                    }
                    _ => ovrcr_protocol::ITermFocusStatus::Unavailable.guidance(),
                });
            }
        }
    }

    pub(super) fn emit_iterm_control(&mut self) -> bool {
        self.cancel_iterm_setup_if_disabled();
        if self
            .desktop
            .iterm
            .in_flight
            .as_ref()
            .is_some_and(|control| control.finished())
        {
            self.desktop.iterm.in_flight = None;
        }
        let setup = &mut self.desktop.iterm;
        if setup.pending.is_none() && setup.next_poll.is_some_and(|at| Instant::now() >= at) {
            setup.next_poll = None;
            if setup.poll_until.is_some_and(|until| Instant::now() < until) {
                if let Some(target) = setup.target.clone() {
                    setup.pending = Some(BridgeOperation::ITermStatus { target });
                }
            } else {
                self.show_notice("iTerm setup unconfirmed; choose Set up iTerm focus to check once without another prompt");
                return true;
            }
        }
        if setup.in_flight.is_some() {
            return false;
        }
        let Some(operation) = setup.pending.clone() else {
            return false;
        };
        if let Some(control) = self.submit_iterm_control(operation) {
            self.desktop.iterm.pending = None;
            self.desktop.iterm.in_flight = Some(control);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::TerminalSize;

    #[test]
    fn explicit_setup_without_opt_in_never_sends_a_request_or_changes_alert_settings() {
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        assert_eq!(dashboard.begin_iterm_setup(), DashboardAction::Redraw);
        assert!(dashboard.desktop.iterm.request.is_none());
        assert!(dashboard.desktop.iterm.pending.is_none());
        assert!(!dashboard.settings.desktop_notifications);
        assert!(!dashboard.settings.ready_sound);
        dashboard.settings.iterm_focus = true;
        let action = dashboard.begin_iterm_setup();
        assert!(matches!(
            action,
            DashboardAction::Request(ClientMessage {
                request: Request::PrepareITermFocus,
                ..
            })
        ));
        assert_eq!(
            dashboard.begin_iterm_setup(),
            DashboardAction::Redraw,
            "no duplicate setup request while pending"
        );
    }
}
