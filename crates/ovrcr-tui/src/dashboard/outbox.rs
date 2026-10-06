//! Every request the Dashboard wants sent, in the order it decided to send it. The event
//! loop drains it once per pass; nothing else holds a request in a private slot.
use ovrcr_protocol::ClientMessage;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Outbox {
    queue: VecDeque<ClientMessage>,
}

impl Outbox {
    pub(super) fn discard_input(&mut self) {
        self.queue
            .retain(|message| !matches!(message.request, ovrcr_protocol::Request::Input { .. }));
    }
    pub(super) fn push(&mut self, message: ClientMessage) {
        self.queue.push_back(message);
    }

    pub(super) fn drain(&mut self) -> Vec<ClientMessage> {
        self.queue.drain(..).collect()
    }
}
