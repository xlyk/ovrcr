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
}

impl DashboardQueue {
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

impl DashboardSink {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(DashboardQueue {
                messages: VecDeque::with_capacity(DASHBOARD_QUEUE),
                terminal: None,
                dirty: HashMap::new(),
                closed: false,
                closing: false,
            }),
            wake: Condvar::new(),
        })
    }

    pub(super) fn enqueue(&self, outbound: DashboardOutbound) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed || queue.closing {
            return false;
        }
        if let ServerMessage::Event(ServerEvent::Output {
            session, revision, ..
        }) = &outbound.message
        {
            let key = (*revision, *session);
            if queue.dirty.contains_key(&key) {
                return true;
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
                self.wake.notify_one();
                return true;
            }
        } else if queue.messages.len() == DASHBOARD_QUEUE {
            // Queued output is not evidence of a stalled dashboard: the
            // writer may simply not have run since a burst. Fold it into
            // dirty markers, as the output path does, and evict only when
            // the queue is still full of messages that cannot be coalesced.
            queue.coalesce_output();
            if queue.messages.len() == DASHBOARD_QUEUE {
                queue.closed = true;
                self.wake.notify_all();
                return false;
            }
        }
        queue.messages.push_back(outbound);
        self.wake.notify_one();
        true
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
        self.wake.notify_one();
        true
    }

    pub(super) fn enqueue_terminal(&self, outbound: DashboardOutbound) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed || queue.closing {
            return false;
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
            queue.terminal = None;
            self.wake.notify_all();
            return false;
        }
        queue.terminal = Some(outbound);
        self.wake.notify_one();
        true
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

    #[cfg(test)]
    pub(super) fn dirty_keys(&self) -> Vec<(u64, SessionId)> {
        self.queue.lock().unwrap().dirty.keys().copied().collect()
    }
}

pub(super) struct DashboardSlot {
    pub(super) sink: Arc<DashboardSink>,
    pub(super) identity: Arc<()>,
    pub(super) stream: UnixStream,
    pub(super) history: Option<ovrcr_terminal::history::FrozenHistory>,
    pub(super) next_history_id: u64,
}

pub(super) struct DashboardSnapshot {
    pub(super) sink: Arc<DashboardSink>,
    pub(super) identity: Arc<()>,
    pub(super) stream: UnixStream,
}

pub(super) fn dashboard_try_send(state: &ServerState, message: ServerMessage) -> bool {
    dashboard_send(state, message, None)
}

pub(super) fn dashboard_send(
    state: &ServerState,
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
) -> bool {
    let Some(snapshot) = dashboard_snapshot(state) else {
        return false;
    };
    if !snapshot.sink.enqueue(DashboardOutbound {
        message,
        completion,
    }) {
        disconnect_dashboard(state, snapshot);
        return false;
    }
    true
}

pub(super) fn dashboard_snapshot(state: &ServerState) -> Option<DashboardSnapshot> {
    let slot = state.dashboard_slot.lock().unwrap();
    let slot = slot.as_ref()?;
    Some(DashboardSnapshot {
        sink: slot.sink.clone(),
        identity: slot.identity.clone(),
        stream: slot.stream.try_clone().ok()?,
    })
}

pub(super) fn dashboard_owner_matches(state: &ServerState, owner: &Arc<()>) -> bool {
    state
        .dashboard_slot
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.identity, owner) && !current.sink.is_closing())
}

pub(super) fn dashboard_send_owner(
    state: &ServerState,
    owner: &Arc<()>,
    message: ServerMessage,
) -> bool {
    let Some(snapshot) =
        dashboard_snapshot(state).filter(|snapshot| Arc::ptr_eq(&snapshot.identity, owner))
    else {
        return false;
    };
    if snapshot.sink.enqueue(DashboardOutbound {
        message,
        completion: None,
    }) {
        true
    } else {
        disconnect_dashboard(state, snapshot);
        false
    }
}

pub(super) fn dashboard_send_owner_terminal(
    state: &ServerState,
    owner: &Arc<()>,
    message: ServerMessage,
    completion: Option<SyncSender<Result<(), String>>>,
) -> bool {
    let Some(snapshot) =
        dashboard_snapshot(state).filter(|snapshot| Arc::ptr_eq(&snapshot.identity, owner))
    else {
        return false;
    };
    if snapshot.sink.enqueue_terminal(DashboardOutbound {
        message,
        completion,
    }) {
        true
    } else {
        disconnect_dashboard(state, snapshot);
        false
    }
}

pub(super) fn disconnect_dashboard(state: &ServerState, snapshot: DashboardSnapshot) {
    let _ = snapshot.stream.shutdown(std::net::Shutdown::Both);
    snapshot.sink.close();
    let mut slot = state.dashboard_slot.lock().unwrap();
    if slot
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.identity, &snapshot.identity))
    {
        let _ = slot.take();
        *state.dashboard.lock().unwrap() = None;
        *state.view.lock().unwrap() = None;
        clear_dashboard_geometry(state, &snapshot.identity);
    }
}
