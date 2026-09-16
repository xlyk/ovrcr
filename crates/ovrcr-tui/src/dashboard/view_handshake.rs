//! The view handshake with the server: one `SetView` in flight, snapshots matched by
//! request, revision, and session, readiness granted only by a complete final `Ok`.
//! Readiness is derived here, never written by a caller.
use anyhow::anyhow;
use ovrcr_protocol::{
    ClientMessage, DashboardView, PaneTarget, Request, SessionId, SessionRunId, TerminalSize,
};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How long a refused view waits before the same view is sent again.
pub(super) const VIEW_RETRY_BACKOFF: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub(super) struct RequestedView {
    pub revision: u64,
    pub targets: Vec<(SessionId, SessionRunId, TerminalSize)>,
    pub focused: Option<SessionId>,
}

impl RequestedView {
    fn same(&self, other: &Self) -> bool {
        self.targets == other.targets && self.focused == other.focused
    }

    fn contains(&self, session: SessionId) -> bool {
        self.targets.iter().any(|(id, _, _)| *id == session)
    }

    fn run_of(&self, session: SessionId) -> Option<SessionRunId> {
        self.targets
            .iter()
            .find(|(id, _, _)| *id == session)
            .map(|(_, run, _)| *run)
    }
}

struct Pending {
    request_id: u64,
    view: RequestedView,
    parser_discarded: bool,
}

#[derive(Debug)]
pub(super) enum Desire {
    Send {
        request: ClientMessage,
        owns_error: bool,
    },
    Coalesced,
    Unchanged,
    Waiting,
}

#[derive(Debug)]
pub(super) enum Acknowledged {
    Granted,
    Incomplete { view: RequestedView },
    Refresh,
}

/// Every field of the dashboard's view state machine: the revision, the one in-flight
/// request, the acknowledged view, the refused view and its backoff, and the snapshots
/// and stale marks readiness is derived from.
#[derive(Default)]
pub(super) struct ViewHandshake {
    revision: u64,
    pending: Option<Pending>,
    acknowledged: Option<RequestedView>,
    failed: Option<(RequestedView, Instant)>,
    force_refresh: bool,
    /// Ids of `SetView` requests still waiting for their final `Ok` or `Error`.
    request_ids: HashSet<u64>,
    snapshots: HashSet<SessionId>,
    stale: HashSet<SessionId>,
    user_change: bool,
}

impl ViewHandshake {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// The user changed selection, splits, focus, or closed a pane: the next request those
    /// changes produce owns the error banner, because a banner written before the change
    /// describes the state the user just left.
    pub(super) fn note_user_change(&mut self) {
        self.user_change = true;
    }

    pub(super) fn acknowledged(&self) -> Option<&RequestedView> {
        self.acknowledged.as_ref()
    }

    /// Whether a `SetView` is waiting for its final response. An acknowledged empty view
    /// has no targets, so pane counts cannot answer this.
    pub(super) fn in_flight(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn pending_targets(&self) -> usize {
        self.pending.as_ref().map_or(0, |p| p.view.targets.len())
    }

    pub(super) fn was_view_request(&self, request_id: u64) -> bool {
        self.request_ids.contains(&request_id)
    }

    pub(super) fn retire(&mut self, request_id: u64) {
        self.request_ids.remove(&request_id);
    }

    pub(super) fn mark_parser_discarded(&mut self) {
        if let Some(pending) = self.pending.as_mut() {
            pending.parser_discarded = true;
        }
    }

    /// `ScreenDirty` for one session: only that session loses readiness.
    pub(super) fn mark_stale(&mut self, session: SessionId) {
        self.stale.insert(session);
        self.force_refresh = true;
    }

    /// The instant a refused view may be sent again, so the event loop can wake for the retry.
    pub(super) fn retry_deadline(&self) -> Option<Instant> {
        self.failed
            .as_ref()
            .map(|(_, refused_at)| *refused_at + VIEW_RETRY_BACKOFF)
    }

    pub(super) fn is_ready(&self, session: SessionId) -> bool {
        self.pending.is_none()
            && !self.stale.contains(&session)
            && self
                .acknowledged
                .as_ref()
                .is_some_and(|view| view.contains(session))
    }

    /// Selection, assignment, or geometry changed: nothing is acknowledged any more.
    pub(super) fn invalidate(&mut self) {
        self.acknowledged = None;
    }

    /// Whether the desired view is the one the server just refused and is still inside its
    /// backoff. A repeating refusal would otherwise re-send `SetView` on every loop pass.
    fn retry_is_waiting(&mut self, desired: &RequestedView, now: Instant) -> bool {
        let Some((refused, refused_at)) = self.failed.as_ref() else {
            return false;
        };
        if refused.same(desired) && now.duration_since(*refused_at) < VIEW_RETRY_BACKOFF {
            return true;
        }
        self.failed = None;
        false
    }

    pub(super) fn desire(
        &mut self,
        desired: RequestedView,
        request_id: u64,
        now: Instant,
    ) -> anyhow::Result<Desire> {
        if self.retry_is_waiting(&desired, now) {
            // A true no-op for the waiting view: readiness, pane errors, and the recorded
            // failure all survive until the backoff deadline the event loop wakes for.
            return Ok(Desire::Waiting);
        }
        if self.pending.is_some() {
            // The change the user asked for is coalesced into the request this pending view's
            // completion sends, so its banner ownership waits here rather than being dropped.
            return Ok(Desire::Coalesced);
        }
        if !self.force_refresh
            && self
                .acknowledged
                .as_ref()
                .is_some_and(|acknowledged| acknowledged.same(&desired))
        {
            // A true no-op completes nothing, so it must not hand a pending user change's
            // banner ownership to whichever request the server asks for next.
            self.user_change = false;
            return Ok(Desire::Unchanged);
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("dashboard view revision exhausted"))?;
        self.revision = revision;
        let mut view = desired;
        view.revision = revision;
        self.force_refresh = false;
        self.request_ids.insert(request_id);
        let owns_error = std::mem::take(&mut self.user_change);
        self.snapshots.clear();
        let request = ClientMessage {
            request_id,
            request: Request::SetView {
                view: DashboardView {
                    revision,
                    panes: view
                        .targets
                        .iter()
                        .map(|(session, run, size)| PaneTarget {
                            session: *session,
                            run: *run,
                            size: *size,
                        })
                        .collect(),
                    focused: view.focused,
                },
            },
        };
        self.pending = Some(Pending {
            request_id,
            view,
            parser_discarded: false,
        });
        Ok(Desire::Send {
            request,
            owns_error,
        })
    }

    pub(super) fn snapshot_matches(
        &self,
        request_id: u64,
        revision: u64,
        session: SessionId,
        run: SessionRunId,
    ) -> bool {
        revision == self.revision
            && self.pending.as_ref().is_some_and(|pending| {
                pending.request_id == request_id
                    && pending.view.revision == revision
                    && pending.view.run_of(session) == Some(run)
            })
    }

    pub(super) fn current_run(&self, session: SessionId) -> Option<SessionRunId> {
        self.pending
            .as_ref()
            .map(|pending| &pending.view)
            .or(self.acknowledged.as_ref())
            .and_then(|view| view.run_of(session))
    }

    pub(super) fn record_snapshot(&mut self, session: SessionId) {
        self.snapshots.insert(session);
    }

    /// The final `Ok` for `request_id`. `None` when it is not the in-flight request.
    pub(super) fn acknowledge(
        &mut self,
        request_id: u64,
        desired_now: &RequestedView,
    ) -> Option<Acknowledged> {
        if self.pending.as_ref()?.request_id != request_id {
            return None;
        }
        let pending = self.pending.take().expect("checked above");
        let complete = pending
            .view
            .targets
            .iter()
            .all(|(session, _, _)| self.snapshots.contains(session));
        self.snapshots.clear();
        if !complete {
            // `Ok` is final and every snapshot precedes it, so a missing one is a failed view
            // rather than one still arriving. The failure is the caller's to record, after its
            // immediate re-request: `failed` is clear whenever a request is in flight, so
            // recording it here would let that re-request through and spin one `SetView` per
            // snapshot-less `Ok`.
            //
            // The previous grant must go with it: the panes still hold the screens of the view
            // this `Ok` failed to replace, and leaving `acknowledged` intact would keep
            // `is_ready` true — and input allowed — against them until the next grant.
            self.acknowledged = None;
            self.force_refresh = true;
            return Some(Acknowledged::Incomplete { view: pending.view });
        }
        if !self.force_refresh && pending.view.same(desired_now) && !pending.parser_discarded {
            self.stale.clear();
            self.failed = None;
            self.acknowledged = Some(pending.view);
            return Some(Acknowledged::Granted);
        }
        // Desired changed while in flight, or the parser was discarded: nothing is ready and
        // the next `desire` mints a fresh request.
        self.acknowledged = None;
        self.force_refresh |= pending.parser_discarded;
        Some(Acknowledged::Refresh)
    }

    /// A refused in-flight view. Input stays revoked and the view waits out one backoff.
    pub(super) fn refuse(&mut self, request_id: u64, now: Instant) -> Option<RequestedView> {
        if self.pending.as_ref()?.request_id != request_id {
            return None;
        }
        let pending = self.pending.take().expect("checked above");
        self.snapshots.clear();
        self.acknowledged = None;
        self.failed = Some((pending.view.clone(), now));
        Some(pending.view)
    }

    /// Records a view whose `Ok` arrived without every snapshot, after the caller has
    /// re-requested it. The re-request is immediate because nothing was recorded yet; the one
    /// after it finds this record and waits out the backoff.
    pub(super) fn record_failure(&mut self, view: RequestedView) {
        self.failed = Some((view, Instant::now()));
    }

    /// Seam for `Dashboard::install_screen`: the desired view counts as acknowledged.
    pub(super) fn install_acknowledged(&mut self, view: RequestedView) {
        self.pending = None;
        self.stale.clear();
        self.user_change = false;
        self.acknowledged = Some(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{Request, SessionId, TerminalSize};
    use std::time::Instant;

    fn view(sessions: &[u64]) -> RequestedView {
        RequestedView {
            revision: 0,
            targets: sessions
                .iter()
                .map(|id| {
                    (
                        SessionId(*id),
                        SessionRunId(1),
                        TerminalSize { rows: 10, cols: 20 },
                    )
                })
                .collect(),
            focused: sessions.first().map(|id| SessionId(*id)),
        }
    }

    fn send(h: &mut ViewHandshake, v: &RequestedView, id: u64) -> u64 {
        match h.desire(v.clone(), id, Instant::now()).unwrap() {
            Desire::Send { request, .. } => match request.request {
                Request::SetView { view } => view.revision,
                _ => panic!(),
            },
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn one_request_in_flight_and_readiness_only_after_full_acknowledgement() {
        let mut h = ViewHandshake::default();
        let v = view(&[1, 2]);
        let revision = send(&mut h, &v, 10);
        assert!(matches!(
            h.desire(v.clone(), 11, Instant::now()).unwrap(),
            Desire::Coalesced
        ));
        assert!(!h.is_ready(SessionId(1)));
        assert!(h.snapshot_matches(10, revision, SessionId(1), SessionRunId(1)));
        assert!(
            !h.snapshot_matches(10, revision + 1, SessionId(1), SessionRunId(1)),
            "wrong revision"
        );
        assert!(
            !h.snapshot_matches(99, revision, SessionId(1), SessionRunId(1)),
            "wrong request"
        );
        assert!(
            !h.snapshot_matches(10, revision, SessionId(3), SessionRunId(1)),
            "wrong session"
        );
        assert!(
            !h.snapshot_matches(10, revision, SessionId(1), SessionRunId(2)),
            "wrong run"
        );
        h.record_snapshot(SessionId(1));
        let Some(Acknowledged::Incomplete { view: incomplete }) = h.acknowledge(10, &v) else {
            panic!("a missing snapshot must not complete the view");
        };
        assert!(!h.is_ready(SessionId(1)));
        // The caller's immediate retry, then the throttle for the one after it.
        let revision = send(&mut h, &v, 12);
        h.record_failure(incomplete);
        h.record_snapshot(SessionId(1));
        h.record_snapshot(SessionId(2));
        assert!(matches!(h.acknowledge(12, &v), Some(Acknowledged::Granted)));
        assert!(h.is_ready(SessionId(1)) && h.is_ready(SessionId(2)));
        assert_eq!(h.revision(), revision);
        assert!(matches!(
            h.desire(v.clone(), 13, Instant::now()).unwrap(),
            Desire::Unchanged
        ));
        // `ScreenDirty` for one session revokes only that session's readiness.
        h.mark_stale(SessionId(2));
        assert!(h.is_ready(SessionId(1)));
        assert!(!h.is_ready(SessionId(2)));
    }

    #[test]
    fn change_revokes_readiness_and_stale_responses_are_ignored() {
        let mut h = ViewHandshake::default();
        let v = view(&[1]);
        send(&mut h, &v, 1);
        h.record_snapshot(SessionId(1));
        assert!(matches!(h.acknowledge(1, &v), Some(Acknowledged::Granted)));
        h.invalidate();
        assert!(!h.is_ready(SessionId(1)));
        let other = view(&[2]);
        send(&mut h, &other, 2);
        assert!(
            h.acknowledge(1, &other).is_none(),
            "late Ok for a retired request"
        );
        assert!(h.refuse(1, Instant::now()).is_none());
        h.record_snapshot(SessionId(2));
        assert!(matches!(
            h.acknowledge(2, &other),
            Some(Acknowledged::Granted)
        ));
        h.mark_stale(SessionId(2));
        assert!(!h.is_ready(SessionId(2)));
    }

    #[test]
    fn repeated_snapshotless_acknowledgements_wait_out_the_backoff() {
        let mut h = ViewHandshake::default();
        let v = view(&[1]);
        send(&mut h, &v, 1);
        // No snapshot: the `Ok` is incomplete and the caller re-requests at once.
        let Some(Acknowledged::Incomplete { view: first }) = h.acknowledge(1, &v) else {
            panic!("a missing snapshot must not complete the view");
        };
        let retry = h.desire(v.clone(), 2, Instant::now()).unwrap();
        assert!(
            matches!(retry, Desire::Send { .. }),
            "first retry is immediate"
        );
        h.record_failure(first);
        // The server answers the retry without a snapshot too: this one has to wait.
        let Some(Acknowledged::Incomplete { view: second }) = h.acknowledge(2, &v) else {
            panic!("a missing snapshot must not complete the view");
        };
        assert!(
            matches!(
                h.desire(v.clone(), 3, Instant::now()).unwrap(),
                Desire::Waiting
            ),
            "a repeating snapshot-less Ok must not spin one SetView per acknowledgement"
        );
        h.record_failure(second);
        let later = Instant::now() + VIEW_RETRY_BACKOFF + std::time::Duration::from_millis(1);
        assert!(matches!(
            h.desire(v.clone(), 4, later).unwrap(),
            Desire::Send { .. }
        ));
    }

    #[test]
    fn an_incomplete_acknowledgement_revokes_the_previous_grant() {
        // A resize mints a new view without `invalidate`, so the grant for the previous one is
        // all that stands between a snapshot-less `Ok` and input reaching a pane whose parser
        // still holds the screen the failed view was meant to replace.
        let mut h = ViewHandshake::default();
        let granted = view(&[1]);
        send(&mut h, &granted, 1);
        h.record_snapshot(SessionId(1));
        assert!(matches!(
            h.acknowledge(1, &granted),
            Some(Acknowledged::Granted)
        ));
        assert!(h.is_ready(SessionId(1)));

        let mut resized = granted.clone();
        resized.targets[0].2 = TerminalSize { rows: 20, cols: 40 };
        send(&mut h, &resized, 2);
        let Some(Acknowledged::Incomplete { view: first }) = h.acknowledge(2, &resized) else {
            panic!("a missing snapshot must not complete the view");
        };
        assert!(
            !h.is_ready(SessionId(1)),
            "the superseded grant must not outlive an incomplete acknowledgement"
        );

        // The caller's immediate re-request, answered the same way: the pass that then waits out
        // the backoff must not find readiness restored either.
        send(&mut h, &resized, 3);
        h.record_failure(first);
        assert!(matches!(
            h.acknowledge(3, &resized),
            Some(Acknowledged::Incomplete { .. })
        ));
        assert!(matches!(
            h.desire(resized.clone(), 4, Instant::now()).unwrap(),
            Desire::Waiting
        ));
        assert!(
            !h.is_ready(SessionId(1)),
            "input stays revoked for the whole backoff"
        );
    }

    #[test]
    fn refusal_waits_out_the_backoff_then_resends() {
        let mut h = ViewHandshake::default();
        let v = view(&[1]);
        send(&mut h, &v, 1);
        assert!(h.refuse(1, Instant::now()).is_some());
        assert!(matches!(
            h.desire(v.clone(), 2, Instant::now()).unwrap(),
            Desire::Waiting
        ));
        let later = Instant::now() + VIEW_RETRY_BACKOFF + std::time::Duration::from_millis(1);
        assert!(matches!(
            h.desire(v.clone(), 3, later).unwrap(),
            Desire::Send { .. }
        ));
    }

    #[test]
    fn user_change_owns_the_error_banner_once() {
        let mut h = ViewHandshake::default();
        let v = view(&[1]);
        h.note_user_change();
        assert!(matches!(
            h.desire(v.clone(), 1, Instant::now()).unwrap(),
            Desire::Send {
                owns_error: true,
                ..
            }
        ));
        h.record_snapshot(SessionId(1));
        h.acknowledge(1, &v);
        h.mark_stale(SessionId(1));
        assert!(matches!(
            h.desire(v.clone(), 2, Instant::now()).unwrap(),
            Desire::Send {
                owns_error: false,
                ..
            }
        ));
    }
}
