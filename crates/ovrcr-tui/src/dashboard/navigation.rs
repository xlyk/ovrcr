//! A notification changes UI selection through the normal view handshake.
//! It never acknowledges unread, answers an Input request or launches a run.
use super::{
    Dashboard, InputMode,
    state::{PaneChange, find_session},
};
use crate::protocol::{
    BRIDGE_SCHEMA_VERSION, BridgeContext, BridgeNavigationOffer, BridgeNavigationTicket,
    ClientMessage, PROTOCOL_VERSION, Request, Response, ServerEvent, ServerMessage,
};
use crate::session::{SessionId, SessionRunId};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Navigation {
    pub(super) context: Option<BridgeContext>,
    pending: Option<(u64, BridgeNavigationOffer)>,
    pub(super) discard_input: bool,
    pub(super) input_epoch: u64,
    pub(super) detached_palette_requests: HashSet<u64>,
}

impl Dashboard {
    pub(super) fn navigation_ticket(
        &self,
        session: SessionId,
        run: SessionRunId,
    ) -> Option<BridgeNavigationTicket> {
        let context = self.navigation.context.as_ref()?;
        let ticket = BridgeNavigationTicket {
            schema: BRIDGE_SCHEMA_VERSION,
            server_wire: PROTOCOL_VERSION,
            server_socket: context.server_socket.clone(),
            callback_executable: context.callback_executable.clone(),
            callback_executable_sha256: context.callback_executable_sha256.clone(),
            server_lifetime: context.server_lifetime.clone(),
            session,
            run,
        };
        ticket.validate().then_some(ticket)
    }

    fn navigation_offer_valid(&self, offer: &BridgeNavigationOffer) -> bool {
        ovrcr_protocol::bridge::canonical_uuid(&offer.navigation)
            && self
                .navigation_ticket(offer.ticket.session, offer.ticket.run)
                .as_ref()
                == Some(&offer.ticket)
            && find_session(self, offer.ticket.session)
                .is_some_and(|session| !session.archived && session.run == offer.ticket.run)
    }

    pub(super) fn handle_notification_navigation(&mut self, message: &ServerMessage) -> bool {
        match message {
            ServerMessage::Event(ServerEvent::BridgeContext(context)) => {
                self.navigation.context = Some(context.clone());
                let request_id = self.next_request_id();
                self.push_request(ClientMessage {
                    request_id,
                    request: Request::DashboardBridgeIdentity {
                        iterm_session_id: std::env::var("ITERM_SESSION_ID").ok().filter(|value| {
                            value.len() <= 256 && !value.chars().any(char::is_control)
                        }),
                    },
                });
                true
            }
            ServerMessage::Event(ServerEvent::NotificationNavigation(offer)) => {
                if self.navigation_offer_valid(offer) {
                    let request_id = self.next_request_id();
                    self.navigation.pending = Some((request_id, offer.clone()));
                    self.push_request(ClientMessage {
                        request_id,
                        request: Request::ConfirmNotificationNavigation {
                            navigation: offer.navigation.clone(),
                        },
                    });
                }
                true
            }
            ServerMessage::Response {
                request_id,
                response: Response::NotificationNavigationConfirmed(offer),
            } => {
                if self
                    .navigation
                    .pending
                    .as_ref()
                    .is_some_and(|(id, _)| id == request_id)
                {
                    let (_, expected) = self.navigation.pending.take().unwrap();
                    if offer.as_deref() == Some(&expected) && self.navigation_offer_valid(&expected)
                    {
                        self.apply_notification_navigation(&expected);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn apply_notification_navigation(&mut self, offer: &BridgeNavigationOffer) {
        // Select always releases capture, including a click on the same target.
        self.retarget(PaneChange::Select);
        self.cancel_notification_palette();
        self.settings_editor.cancel_draft();
        self.close_events();
        self.details = None;
        self.tasks = None;
        self.deferred_history_at_tail = None;
        self.outbox.discard_input();
        // Displaying a stopped retained run must not trigger automatic recovery.
        self.recovery_requests
            .entry((offer.ticket.session, offer.ticket.run))
            .or_insert(None);
        self.select_session(offer.ticket.session);
        self.mode = InputMode::Browse;
        self.navigation.input_epoch = self.navigation.input_epoch.wrapping_add(1);
        let request_id = self.next_request_id();
        self.push_request(ClientMessage {
            request_id,
            request: Request::NotificationNavigationApplied {
                navigation: offer.navigation.clone(),
            },
        });
    }
}
