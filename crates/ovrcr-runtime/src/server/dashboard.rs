//! The server's one active dashboard: who holds it, what may be sent to it,
//! and the view it has acknowledged. Every owner check and every teardown is here.
use super::*;

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

struct Geometry {
    owner: Arc<()>,
    size: TerminalSize,
}

#[derive(Default)]
pub struct ActiveDashboard {
    slot: Mutex<Option<DashboardSlot>>,
    view: Mutex<Option<DashboardView>>,
    geometry: Mutex<Option<Geometry>>,
}

impl ActiveDashboard {
    pub(super) fn claim(&self, sink: Arc<DashboardSink>, stream: UnixStream) -> Option<Arc<()>> {
        let identity = Arc::new(());
        let mut slot = self.slot.lock().unwrap();
        if slot.is_some() {
            return None;
        }
        *slot = Some(DashboardSlot {
            sink,
            identity: Arc::clone(&identity),
            stream,
            history: None,
            next_history_id: 1,
        });
        Some(identity)
    }

    #[cfg(test)]
    pub(super) fn is_claimed(&self) -> bool {
        self.slot.lock().unwrap().is_some()
    }

    /// The owner still holds the slot and its queue is not draining a terminal
    /// frame. Publishing a view or resizing for a draining dashboard would
    /// admit input for a connection that is already going away.
    pub(super) fn owns(&self, owner: &Arc<()>) -> bool {
        self.slot.lock().unwrap().as_ref().is_some_and(|current| {
            Arc::ptr_eq(&current.identity, owner) && !current.sink.is_closing()
        })
    }

    pub(super) fn snapshot(&self) -> Option<DashboardSnapshot> {
        let slot = self.slot.lock().unwrap();
        let slot = slot.as_ref()?;
        Some(DashboardSnapshot {
            sink: Arc::clone(&slot.sink),
            identity: Arc::clone(&slot.identity),
            stream: slot.stream.try_clone().ok()?,
        })
    }

    pub(super) fn snapshot_owned(&self, owner: &Arc<()>) -> Option<DashboardSnapshot> {
        self.snapshot()
            .filter(|snapshot| Arc::ptr_eq(&snapshot.identity, owner))
    }

    /// Queue on a resolved dashboard, disconnecting only when the queue is
    /// closed. A draining queue still owes its client a terminal frame, so the
    /// socket stays open.
    fn queue(
        &self,
        snapshot: DashboardSnapshot,
        outbound: DashboardOutbound,
        terminal: bool,
    ) -> bool {
        let result = if terminal {
            snapshot.sink.enqueue_terminal(outbound)
        } else {
            snapshot.sink.enqueue(outbound)
        };
        match result {
            Enqueue::Queued => true,
            Enqueue::Draining => false,
            Enqueue::Closed => {
                self.disconnect(snapshot);
                false
            }
        }
    }

    pub(super) fn try_send(&self, message: ServerMessage) -> bool {
        self.send(message, None)
    }

    pub(super) fn send(
        &self,
        message: ServerMessage,
        completion: Option<SyncSender<Result<(), String>>>,
    ) -> bool {
        let Some(snapshot) = self.snapshot() else {
            return false;
        };
        self.queue(
            snapshot,
            DashboardOutbound {
                message,
                completion,
            },
            false,
        )
    }

    pub(super) fn send_owner(&self, owner: &Arc<()>, message: ServerMessage) -> bool {
        self.send_owner_with_completion(owner, message, None)
    }

    /// A response belongs to the dashboard that asked. A replacement owner must
    /// never receive an evicted connection's late answer under its own request
    /// id.
    pub(super) fn send_owner_with_completion(
        &self,
        owner: &Arc<()>,
        message: ServerMessage,
        completion: Option<SyncSender<Result<(), String>>>,
    ) -> bool {
        let Some(snapshot) = self.snapshot_owned(owner) else {
            return false;
        };
        self.queue(
            snapshot,
            DashboardOutbound {
                message,
                completion,
            },
            false,
        )
    }

    pub(super) fn send_owner_terminal(
        &self,
        owner: &Arc<()>,
        message: ServerMessage,
        completion: Option<SyncSender<Result<(), String>>>,
    ) -> bool {
        let Some(snapshot) = self.snapshot_owned(owner) else {
            return false;
        };
        self.queue(
            snapshot,
            DashboardOutbound {
                message,
                completion,
            },
            true,
        )
    }

    /// Locked access to the owned slot for history bookkeeping. `None` when
    /// `owner` no longer holds the dashboard. Callers that already hold
    /// `sessions` keep the sessions → slot lock order this preserves.
    pub(super) fn with_owned<R>(
        &self,
        owner: &Arc<()>,
        f: impl FnOnce(&mut DashboardSlot) -> R,
    ) -> Option<R> {
        let mut slot = self.slot.lock().unwrap();
        let current = slot
            .as_mut()
            .filter(|current| Arc::ptr_eq(&current.identity, owner))?;
        Some(f(current))
    }

    pub(super) fn view(&self) -> Option<DashboardView> {
        self.view.lock().unwrap().clone()
    }

    pub(super) fn next_revision(&self) -> u64 {
        self.view
            .lock()
            .unwrap()
            .as_ref()
            .map_or(1, |view| view.revision.saturating_add(1))
    }

    /// Publish `published` for `owner` and record the focused pane's geometry.
    /// False when the owner lost the slot or its queue is draining; nothing is
    /// published then.
    pub(super) fn publish_view(
        &self,
        owner: &Arc<()>,
        published: DashboardView,
        focus_changed: bool,
        before_publish: impl FnOnce(),
    ) -> bool {
        let mut slot = self.slot.lock().unwrap();
        let Some(current) = slot
            .as_mut()
            .filter(|current| Arc::ptr_eq(&current.identity, owner) && !current.sink.is_closing())
        else {
            return false;
        };
        before_publish();
        if focus_changed {
            current.history.take();
        }
        let focused_size = published
            .focused
            .and_then(|focused| published.panes.iter().find(|pane| pane.session == focused))
            .map(|pane| pane.size);
        *self.view.lock().unwrap() = Some(published);
        if let Some(size) = focused_size {
            self.set_geometry(owner, size);
        }
        true
    }

    /// Record the geometry `owner` reported. The caller proves ownership; a
    /// dashboard that reports its pane size has not published a view yet.
    pub(super) fn set_geometry(&self, owner: &Arc<()>, size: TerminalSize) {
        *self.geometry.lock().unwrap() = Some(Geometry {
            owner: Arc::clone(owner),
            size,
        });
    }

    pub(super) fn clear_view(&self, fallback_revision: Option<u64>) {
        let mut view = self.view.lock().unwrap();
        match view.as_mut() {
            Some(current) => {
                current.panes.clear();
                current.focused = None;
            }
            None => {
                if let Some(revision) = fallback_revision {
                    *view = Some(DashboardView {
                        revision,
                        panes: Vec::new(),
                        focused: None,
                    });
                }
            }
        }
    }

    pub(super) fn clear_view_for(&self, owner: &Arc<()>, fallback_revision: Option<u64>) {
        let slot = self.slot.lock().unwrap();
        if slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.identity, owner))
        {
            self.clear_view(fallback_revision);
        }
    }

    pub(super) fn geometry(&self) -> Option<TerminalSize> {
        self.geometry
            .lock()
            .unwrap()
            .as_ref()
            .map(|geometry| geometry.size)
    }

    /// A removed session leaves no frozen history and no pane behind; if it was
    /// the focused pane the view is cleared so nothing admits input for it.
    pub(super) fn forget_session(&self, id: SessionId) {
        if let Some(slot) = self.slot.lock().unwrap().as_mut()
            && slot
                .history
                .as_ref()
                .is_some_and(|history| history.opened().session == id)
        {
            slot.history.take();
        }
        let mut view = self.view.lock().unwrap();
        if view
            .as_ref()
            .is_some_and(|current| current.focused == Some(id))
        {
            drop(view);
            self.clear_view(None);
        } else if let Some(current) = view.as_mut() {
            current.panes.retain(|pane| pane.session != id);
        }
    }

    /// The one teardown. Removes `identity`'s slot if it still holds the
    /// dashboard and clears the view and geometry it owned. A stale identity
    /// changes nothing.
    fn vacate(&self, identity: &Arc<()>) {
        let mut slot = self.slot.lock().unwrap();
        if !slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.identity, identity))
        {
            return;
        }
        if let Some(current) = slot.take() {
            current.sink.close();
        }
        *self.view.lock().unwrap() = None;
        let mut geometry = self.geometry.lock().unwrap();
        if geometry
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.owner, identity))
        {
            *geometry = None;
        }
    }

    pub(super) fn disconnect(&self, snapshot: DashboardSnapshot) {
        let _ = snapshot.stream.shutdown(std::net::Shutdown::Both);
        snapshot.sink.close();
        self.vacate(&snapshot.identity);
    }

    pub(super) fn release(&self, owner: &Arc<()>) {
        self.vacate(owner);
    }

    #[cfg(test)]
    pub(super) fn claim_with_identity(
        &self,
        sink: Arc<DashboardSink>,
        identity: Arc<()>,
        stream: UnixStream,
    ) {
        *self.slot.lock().unwrap() = Some(DashboardSlot {
            sink,
            identity,
            stream,
            history: None,
            next_history_id: 1,
        });
    }

    #[cfg(test)]
    pub(super) fn install_view_for_test(&self, view: Option<DashboardView>) {
        *self.view.lock().unwrap() = view;
    }

    #[cfg(test)]
    pub(super) fn slot_for_test(&self) -> std::sync::MutexGuard<'_, Option<DashboardSlot>> {
        self.slot.lock().unwrap()
    }

    /// Whether another thread currently holds the slot lock, so a test can
    /// prove publication is atomic against a concurrent teardown.
    #[cfg(test)]
    pub(super) fn slot_is_locked_for_test(&self) -> bool {
        matches!(
            self.slot.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        )
    }

    #[cfg(test)]
    pub(super) fn geometry_owner_for_test(&self) -> Option<Arc<()>> {
        self.geometry
            .lock()
            .unwrap()
            .as_ref()
            .map(|geometry| Arc::clone(&geometry.owner))
    }
}

/// The dashboard writer thread: pops deliveries, writes frames, and disconnects
/// on the first failed or terminal write.
pub(super) fn spawn_writer(
    state: Arc<ServerState>,
    sink: Arc<DashboardSink>,
    identity: Arc<()>,
    stream: UnixStream,
) -> io::Result<JoinHandle<()>> {
    let mut output = stream.try_clone().ok();
    let close_stream = stream;
    thread::Builder::new()
        .name("ovrcr-dashboard-writer".into())
        .spawn(move || {
            while let Some(delivery) = sink.next() {
                let (message, completion, dirty, terminal) = match delivery {
                    DashboardDelivery::Message(outbound) => {
                        (outbound.message, outbound.completion, None, false)
                    }
                    DashboardDelivery::Terminal(outbound) => {
                        (outbound.message, outbound.completion, None, true)
                    }
                    DashboardDelivery::Dirty { revision, session } => (
                        ServerMessage::Event(ServerEvent::ScreenDirty { session, revision }),
                        None,
                        Some((revision, session)),
                        false,
                    ),
                };
                #[cfg(test)]
                if let Some(hook) = state.before_dashboard_write_hook.lock().unwrap().clone() {
                    hook();
                }
                let result = match output.as_mut() {
                    Some(stream) => {
                        write_frame(stream, &message).map_err(|error| error.to_string())
                    }
                    None => Err("dashboard writer stream unavailable".into()),
                };
                if result.is_ok()
                    && let Some((revision, session)) = dirty
                {
                    sink.dirty_sent(revision, session);
                }
                if let Some(completion) = completion {
                    let _ = completion.send(result.clone());
                }
                if result.is_err() || terminal {
                    sink.close();
                    let _ = close_stream.shutdown(std::net::Shutdown::Both);
                    state.dashboard.disconnect(DashboardSnapshot {
                        sink,
                        identity,
                        stream: close_stream,
                    });
                    break;
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::PaneTarget;

    fn pair() -> (UnixStream, UnixStream) {
        UnixStream::pair().unwrap()
    }

    #[test]
    fn claim_is_exclusive_until_release() {
        let dashboard = ActiveDashboard::default();
        let (_a, stream_a) = pair();
        let owner = dashboard
            .claim(DashboardSink::new(), stream_a)
            .expect("first claim");
        let (_b, stream_b) = pair();
        assert!(dashboard.claim(DashboardSink::new(), stream_b).is_none());
        assert!(dashboard.owns(&owner));
        assert!(!dashboard.owns(&Arc::new(())));
        dashboard.release(&owner);
        assert!(!dashboard.is_claimed());
        assert!(!dashboard.owns(&owner));
    }

    #[test]
    fn release_by_a_stale_owner_leaves_the_replacement_untouched() {
        let dashboard = ActiveDashboard::default();
        let (_a, stream_a) = pair();
        let old = dashboard.claim(DashboardSink::new(), stream_a).unwrap();
        dashboard.release(&old);
        let (_b, stream_b) = pair();
        let new = dashboard.claim(DashboardSink::new(), stream_b).unwrap();
        dashboard.install_view_for_test(Some(DashboardView {
            revision: 3,
            panes: Vec::new(),
            focused: None,
        }));
        dashboard.release(&old);
        assert!(dashboard.owns(&new));
        assert_eq!(dashboard.view().map(|view| view.revision), Some(3));
    }

    #[test]
    fn disconnect_and_release_clear_the_same_state() {
        for use_disconnect in [true, false] {
            let dashboard = ActiveDashboard::default();
            let (_client, server) = pair();
            let owner = dashboard.claim(DashboardSink::new(), server).unwrap();
            assert!(dashboard.publish_view(
                &owner,
                DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: SessionId(1),
                        size: TerminalSize { rows: 5, cols: 7 },
                    }],
                    focused: Some(SessionId(1)),
                },
                false,
                || {},
            ));
            assert_eq!(
                dashboard.geometry(),
                Some(TerminalSize { rows: 5, cols: 7 })
            );
            if use_disconnect {
                let snapshot = dashboard.snapshot_owned(&owner).unwrap();
                dashboard.disconnect(snapshot);
            } else {
                dashboard.release(&owner);
            }
            assert!(!dashboard.is_claimed());
            assert!(dashboard.view().is_none());
            assert!(dashboard.geometry().is_none());
        }
    }

    #[test]
    fn owns_is_false_once_the_sink_is_closing() {
        let dashboard = ActiveDashboard::default();
        let (_client, server) = pair();
        let sink = DashboardSink::new();
        let owner = dashboard.claim(Arc::clone(&sink), server).unwrap();
        assert!(dashboard.send_owner_terminal(
            &owner,
            ServerMessage::Response {
                request_id: 1,
                response: Response::Ok,
            },
            None,
        ));
        assert!(!dashboard.owns(&owner));
        assert!(!dashboard.publish_view(
            &owner,
            DashboardView {
                revision: 1,
                panes: Vec::new(),
                focused: None,
            },
            false,
            || {},
        ));
        assert!(
            dashboard.with_owned(&owner, |_| ()).is_some(),
            "history requests still see the draining slot"
        );
    }

    #[test]
    fn forget_session_drops_history_and_focused_view() {
        let dashboard = ActiveDashboard::default();
        let (_client, server) = pair();
        let owner = dashboard.claim(DashboardSink::new(), server).unwrap();
        dashboard.install_view_for_test(Some(DashboardView {
            revision: 2,
            panes: vec![
                PaneTarget {
                    session: SessionId(1),
                    size: TerminalSize { rows: 1, cols: 1 },
                },
                PaneTarget {
                    session: SessionId(2),
                    size: TerminalSize { rows: 1, cols: 1 },
                },
            ],
            focused: Some(SessionId(2)),
        }));
        dashboard.forget_session(SessionId(1));
        assert_eq!(dashboard.view().unwrap().panes.len(), 1);
        dashboard.forget_session(SessionId(2));
        let view = dashboard.view().unwrap();
        assert!(view.panes.is_empty() && view.focused.is_none() && view.revision == 2);
        assert!(dashboard.owns(&owner));
    }
}
