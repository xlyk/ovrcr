//! Fully formed steady-state Dashboard requests have one queue and leave it in order.
//! Existing event/message cutpoints drain it; fresh PTY input is authorized after that
//! drain, and startup retains its reserved handshake requests outside this queue.
use ovrcr_protocol::ClientMessage;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Outbox {
    queue: VecDeque<ClientMessage>,
}

impl Outbox {
    pub(super) fn discard_input(&mut self) {
        self.queue.retain(|message| {
            !matches!(
                message.request,
                ovrcr_protocol::Request::Input { .. }
                    | ovrcr_protocol::Request::AssignedHumanInput { .. }
            )
        });
    }
    pub(super) fn push(&mut self, message: ClientMessage) {
        self.queue.push_back(message);
    }

    pub(super) fn drain(&mut self) -> Vec<ClientMessage> {
        self.queue.drain(..).collect()
    }
}
