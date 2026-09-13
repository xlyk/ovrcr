use super::*;

pub struct DashboardOutbound {
    pub(super) message: ServerMessage,
    pub(super) completion: Option<SyncSender<Result<(), String>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirtyState {
    Pending,
    Sending,
    Sent,
}

pub(super) struct DashboardQueue {
    pub(super) messages: VecDeque<DashboardOutbound>,
    terminal: Option<DashboardOutbound>,
    dirty: HashMap<(u64, SessionId), DirtyState>,
    closed: bool,
    closing: bool,
    #[cfg(any(test, feature = "acceptance-diagnostics"))]
    peak_items: usize,
    #[cfg(any(test, feature = "acceptance-diagnostics"))]
    peak_bytes: usize,
    #[cfg(any(test, feature = "acceptance-diagnostics"))]
    rejected: u64,
}

#[cfg(any(test, feature = "acceptance-diagnostics"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DashboardQueueSnapshot {
    pub pending_items: usize,
    pub pending_bytes: usize,
    pub peak_items: usize,
    pub peak_bytes: usize,
    pub rejected: u64,
    pub message_items: usize,
    pub terminal_items: usize,
    pub dirty_items: usize,
}

#[cfg(feature = "acceptance-diagnostics")]
#[derive(Clone, Default)]
pub struct DashboardQueueMonitor {
    sink: Arc<Mutex<Option<std::sync::Weak<DashboardSink>>>>,
}

#[cfg(feature = "acceptance-diagnostics")]
impl DashboardQueueMonitor {
    pub fn snapshot(&self) -> Option<DashboardQueueSnapshot> {
        self.sink
            .lock()
            .unwrap()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .map(|sink| sink.reporting_snapshot())
    }

    pub(super) fn register(&self, sink: &Arc<DashboardSink>) {
        *self.sink.lock().unwrap() = Some(Arc::downgrade(sink));
    }
}

impl DashboardQueue {
    #[cfg(any(test, feature = "acceptance-diagnostics"))]
    fn snapshot(&self) -> DashboardQueueSnapshot {
        let message_bytes = self
            .messages
            .iter()
            .map(|outbound| encoded_len(&outbound.message))
            .sum::<usize>();
        let terminal_bytes = self
            .terminal
            .as_ref()
            .map_or(0, |outbound| encoded_len(&outbound.message));
        let dirty_bytes = self
            .dirty
            .keys()
            .map(|(revision, session)| {
                encoded_len(&ServerMessage::Event(ServerEvent::ScreenDirty {
                    session: *session,
                    revision: *revision,
                }))
            })
            .sum::<usize>();
        DashboardQueueSnapshot {
            pending_items: self.messages.len()
                + usize::from(self.terminal.is_some())
                + self.dirty.len(),
            pending_bytes: message_bytes + terminal_bytes + dirty_bytes,
            peak_items: self.peak_items,
            peak_bytes: self.peak_bytes,
            rejected: self.rejected,
            message_items: self.messages.len(),
            terminal_items: usize::from(self.terminal.is_some()),
            dirty_items: self.dirty.len(),
        }
    }

    fn record_peak(&mut self) {
        #[cfg(any(test, feature = "acceptance-diagnostics"))]
        {
            let snapshot = self.snapshot();
            self.peak_items = self.peak_items.max(snapshot.pending_items);
            self.peak_bytes = self.peak_bytes.max(snapshot.pending_bytes);
        }
    }

    fn record_rejected(&mut self) {
        #[cfg(any(test, feature = "acceptance-diagnostics"))]
        {
            self.rejected += 1;
        }
    }

    /// Replace every queued output frame with a pending dirty marker for
    /// its revision and session, so the dashboard re-reads the screen instead.
    fn coalesce_output(&mut self) {
        let mut sessions = Vec::new();
        self.messages.retain(|queued| match queued.message {
            ServerMessage::Event(ServerEvent::Output {
                session, revision, ..
            }) => {
                sessions.push((revision, session));
                false
            }
            _ => true,
        });
        for session in sessions {
            self.dirty.entry(session).or_insert(DirtyState::Pending);
        }
    }
}

pub struct DashboardSink {
    pub(super) queue: Mutex<DashboardQueue>,
    wake: Condvar,
}

pub(super) enum DashboardDelivery {
    Message(DashboardOutbound),
    Terminal(DashboardOutbound),
    Dirty { revision: u64, session: SessionId },
}

/// Outcome of offering one message to a dashboard queue.
///
/// `Draining` is not a disconnect. The terminal frame path marks the queue
/// closing so the writer can still flush that final frame; anything offered in
/// the meantime is dropped with the socket left open. Only `Closed` means the
/// connection can no longer deliver and must be torn down.
pub(super) enum Enqueue {
    Queued,
    Draining,
    Closed,
}

impl DashboardSink {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(DashboardQueue {
                messages: VecDeque::with_capacity(DASHBOARD_QUEUE),
                terminal: None,
                dirty: HashMap::new(),
                closed: false,
                closing: false,
                #[cfg(any(test, feature = "acceptance-diagnostics"))]
                peak_items: 0,
                #[cfg(any(test, feature = "acceptance-diagnostics"))]
                peak_bytes: 0,
                #[cfg(any(test, feature = "acceptance-diagnostics"))]
                rejected: 0,
            }),
            wake: Condvar::new(),
        })
    }

    pub(super) fn enqueue(&self, outbound: DashboardOutbound) -> Enqueue {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            queue.record_rejected();
            return Enqueue::Closed;
        }
        if queue.closing {
            queue.record_rejected();
            return Enqueue::Draining;
        }
        if let ServerMessage::Event(ServerEvent::Output {
            session, revision, ..
        }) = &outbound.message
        {
            let key = (*revision, *session);
            if queue.dirty.contains_key(&key) {
                return Enqueue::Queued;
            }
            if queue.messages.len() == DASHBOARD_QUEUE {
                queue.messages.retain(|queued| {
                    !matches!(
                        queued.message,
                        ServerMessage::Event(ServerEvent::Output {
                            session: queued_session,
                            revision: queued_revision,
                            ..
                        }) if queued_session == *session && queued_revision == *revision
                    )
                });
                queue.dirty.insert(key, DirtyState::Pending);
                queue.record_peak();
                self.wake.notify_one();
                return Enqueue::Queued;
            }
        } else if queue.messages.len() == DASHBOARD_QUEUE {
            // Queued output is not evidence of a stalled dashboard: the
            // writer may simply not have run since a burst. Fold it into
            // dirty markers, as the output path does, and evict only when
            // the queue is still full of messages that cannot be coalesced.
            queue.coalesce_output();
            if queue.messages.len() == DASHBOARD_QUEUE {
                queue.closed = true;
                queue.record_rejected();
                self.wake.notify_all();
                return Enqueue::Closed;
            }
        }
        queue.messages.push_back(outbound);
        queue.record_peak();
        self.wake.notify_one();
        Enqueue::Queued
    }

    /// Boolean view of [`Self::enqueue`] for fixtures that only assert whether
    /// a message was queued. It delegates rather than repeating the rules.
    #[cfg(test)]
    pub(super) fn enqueue_queued(&self, outbound: DashboardOutbound) -> bool {
        matches!(self.enqueue(outbound), Enqueue::Queued)
    }

    pub(super) fn replace_view(
        &self,
        view: &DashboardView,
        request_id: u64,
        screens: Vec<Vec<u8>>,
    ) -> bool {
        if screens.len() != view.panes.len() {
            return false;
        }
        let mut messages = Vec::with_capacity(screens.len() + 1);
        for (pane, bytes) in view.panes.iter().zip(screens) {
            messages.push(DashboardOutbound {
                message: ServerMessage::Response {
                    request_id,
                    response: Response::Screen {
                        session: pane.session,
                        revision: view.revision,
                        size: pane.size,
                        bytes,
                    },
                },
                completion: None,
            });
        }
        messages.push(DashboardOutbound {
            message: ServerMessage::Response {
                request_id,
                response: Response::Ok,
            },
            completion: None,
        });
        if messages.iter().any(|message| {
            bincode::serde::encode_to_vec(&message.message, bincode::config::standard())
                .map_or(true, |bytes| bytes.len() > ovrcr_protocol::MAX_FRAME_BYTES)
        }) {
            let mut queue = self.queue.lock().unwrap();
            queue.closed = true;
            self.wake.notify_all();
            return false;
        }
        let mut queue = self.queue.lock().unwrap();
        if queue.closed || queue.closing {
            return false;
        }
        queue.messages.retain(|queued| {
            !matches!(
                queued.message,
                ServerMessage::Event(ServerEvent::Output { .. })
            )
        });
        queue.dirty.clear();
        if queue.messages.len() + messages.len() > DASHBOARD_QUEUE {
            queue.closed = true;
            self.wake.notify_all();
            return false;
        }
        queue.messages.extend(messages);
        queue.record_peak();
        self.wake.notify_one();
        true
    }

    pub(super) fn enqueue_terminal(&self, outbound: DashboardOutbound) -> Enqueue {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            queue.record_rejected();
            return Enqueue::Closed;
        }
        if queue.closing {
            queue.record_rejected();
            // A terminal frame is already queued; the writer closes this
            // connection once it has flushed that one.
            return Enqueue::Draining;
        }
        queue.closing = true;
        let too_large =
            bincode::serde::encode_to_vec(&outbound.message, bincode::config::standard())
                .map_or(true, |bytes| bytes.len() > ovrcr_protocol::MAX_FRAME_BYTES);
        queue.messages.retain(|queued| {
            !matches!(
                queued.message,
                ServerMessage::Event(ServerEvent::Output { .. })
            )
        });
        queue.dirty.clear();
        if too_large || queue.messages.len() == DASHBOARD_QUEUE {
            queue.closed = true;
            queue.record_rejected();
            queue.terminal = None;
            self.wake.notify_all();
            return Enqueue::Closed;
        }
        queue.terminal = Some(outbound);
        queue.record_peak();
        self.wake.notify_one();
        Enqueue::Queued
    }

    pub(super) fn next(&self) -> Option<DashboardDelivery> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if queue.closed {
                return None;
            }
            if let Some(message) = queue.messages.pop_front() {
                return Some(DashboardDelivery::Message(message));
            }
            if let Some(message) = queue.terminal.take() {
                return Some(DashboardDelivery::Terminal(message));
            }
            if let Some(((revision, session), state)) = queue
                .dirty
                .iter_mut()
                .find(|(_, state)| **state == DirtyState::Pending)
            {
                *state = DirtyState::Sending;
                return Some(DashboardDelivery::Dirty {
                    revision: *revision,
                    session: *session,
                });
            }
            queue = self.wake.wait(queue).unwrap();
        }
    }

    pub(super) fn dirty_sent(&self, revision: u64, session: SessionId) {
        let mut queue = self.queue.lock().unwrap();
        if queue.dirty.get(&(revision, session)) == Some(&DirtyState::Sending) {
            queue.dirty.insert((revision, session), DirtyState::Sent);
        }
    }

    pub(super) fn close(&self) {
        let mut queue = self.queue.lock().unwrap();
        queue.closed = true;
        self.wake.notify_all();
    }

    pub(super) fn is_closing(&self) -> bool {
        let queue = self.queue.lock().unwrap();
        queue.closed || queue.closing
    }

    #[cfg(any(test, feature = "acceptance-diagnostics"))]
    pub fn reporting_snapshot(&self) -> DashboardQueueSnapshot {
        let mut snapshot = self.queue.lock().unwrap().snapshot();
        snapshot.peak_items = snapshot.peak_items.max(snapshot.pending_items);
        snapshot.peak_bytes = snapshot.peak_bytes.max(snapshot.pending_bytes);
        snapshot
    }

    #[cfg(test)]
    pub(super) fn dirty_keys(&self) -> Vec<(u64, SessionId)> {
        self.queue.lock().unwrap().dirty.keys().copied().collect()
    }
}

#[cfg(any(test, feature = "acceptance-diagnostics"))]
fn encoded_len(message: &ServerMessage) -> usize {
    bincode::serde::encode_to_vec(message, bincode::config::standard())
        .map_or(0, |bytes| bytes.len())
}

/// Thin wrappers over [`ActiveDashboard`] kept so existing callers compile.
/// The next ticket routes each caller at the module directly and deletes these.
pub(super) fn dashboard_try_send(state: &ServerState, message: ServerMessage) -> bool {
    state.dashboard.try_send(message)
}

pub(super) fn dashboard_send(
    state: &ServerState,
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
) -> bool {
    state.dashboard.send(message, completion)
}

pub(super) fn dashboard_snapshot(state: &ServerState) -> Option<DashboardSnapshot> {
    state.dashboard.snapshot()
}

pub(super) fn dashboard_owner_matches(state: &ServerState, owner: &Arc<()>) -> bool {
    state.dashboard.owns(owner)
}

pub(super) fn dashboard_send_owner(
    state: &ServerState,
    owner: &Arc<()>,
    message: ServerMessage,
) -> bool {
    state.dashboard.send_owner(owner, message)
}

pub(super) fn dashboard_send_owner_with_completion(
    state: &ServerState,
    owner: &Arc<()>,
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
) -> bool {
    state
        .dashboard
        .send_owner_with_completion(owner, message, completion)
}

pub(super) fn dashboard_send_owner_terminal(
    state: &ServerState,
    owner: &Arc<()>,
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
) -> bool {
    state
        .dashboard
        .send_owner_terminal(owner, message, completion)
}

pub(super) fn disconnect_dashboard(state: &ServerState, snapshot: DashboardSnapshot) {
    state.dashboard.disconnect(snapshot);
}
