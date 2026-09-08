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
    dirty: HashMap<SessionId, DirtyState>,
    closed: bool,
}

impl DashboardQueue {
    /// Replace every queued output frame with a pending dirty marker for
    /// its session, so the dashboard re-reads the screen instead.
    fn coalesce_output(&mut self) {
        let mut sessions = Vec::new();
        self.messages.retain(|queued| match queued.message {
            ServerMessage::Event(ServerEvent::Output { session, .. }) => {
                sessions.push(session);
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
    Dirty(SessionId),
}

impl DashboardSink {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(DashboardQueue {
                messages: VecDeque::with_capacity(DASHBOARD_QUEUE),
                dirty: HashMap::new(),
                closed: false,
            }),
            wake: Condvar::new(),
        })
    }

    pub(super) fn enqueue(&self, outbound: DashboardOutbound) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            return false;
        }
        if let ServerMessage::Event(ServerEvent::Output { session, .. }) = &outbound.message {
            if queue.dirty.contains_key(session) {
                return true;
            }
            if queue.messages.len() == DASHBOARD_QUEUE {
                queue.messages.retain(|queued| {
                    !matches!(
                        queued.message,
                        ServerMessage::Event(ServerEvent::Output {
                            session: queued_session,
                            ..
                        }) if queued_session == *session
                    )
                });
                queue.dirty.insert(*session, DirtyState::Pending);
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

    pub(super) fn replace_selection(
        &self,
        previous: Option<SessionId>,
        session: SessionId,
        request_id: u64,
        size: TerminalSize,
        bytes: Vec<u8>,
    ) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.closed {
            return false;
        }
        let discard = previous
            .into_iter()
            .chain(std::iter::once(session))
            .collect::<HashSet<_>>();
        queue.messages.retain(|queued| {
            !matches!(
                queued.message,
                ServerMessage::Event(ServerEvent::Output { session, .. }) if discard.contains(&session)
            )
        });
        for id in discard {
            queue.dirty.remove(&id);
        }
        if queue.messages.len() == DASHBOARD_QUEUE {
            queue.closed = true;
            self.wake.notify_all();
            return false;
        }
        queue.messages.push_back(DashboardOutbound {
            message: ServerMessage::Response {
                request_id,
                response: Response::Screen {
                    session,
                    size,
                    bytes,
                },
            },
            completion: None,
        });
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
            if let Some((session, state)) = queue
                .dirty
                .iter_mut()
                .find(|(_, state)| **state == DirtyState::Pending)
            {
                *state = DirtyState::Sending;
                return Some(DashboardDelivery::Dirty(*session));
            }
            queue = self.wake.wait(queue).unwrap();
        }
    }

    pub(super) fn dirty_sent(&self, session: SessionId) {
        let mut queue = self.queue.lock().unwrap();
        if queue.dirty.get(&session) == Some(&DirtyState::Sending) {
            queue.dirty.insert(session, DirtyState::Sent);
        }
    }

    pub(super) fn close(&self) {
        let mut queue = self.queue.lock().unwrap();
        queue.closed = true;
        self.wake.notify_all();
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
        .is_some_and(|current| Arc::ptr_eq(&current.identity, owner))
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
        *state.selected.lock().unwrap() = None;
        clear_dashboard_geometry(state, &snapshot.identity);
    }
}
