//! The view handshake with the server: one `SetView` in flight, snapshots matched by
//! request, revision, and session, readiness granted only by a complete final `Ok`.
//! Readiness is derived here, never written by a caller.
use anyhow::anyhow;
use ovrcr_protocol::{ClientMessage, DashboardView, PaneTarget, Request, SessionId, TerminalSize};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How long a refused view waits before the same view is sent again.
pub(super) const VIEW_RETRY_BACKOFF: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub(super) struct RequestedView {
    pub revision: u64,
    pub targets: Vec<(SessionId, TerminalSize)>,
    pub focused: Option<SessionId>,
}

impl RequestedView {
    fn same(&self, other: &Self) -> bool {
        self.targets == other.targets && self.focused == other.focused
    }

    fn contains(&self, session: SessionId) -> bool {
        self.targets.iter().any(|(id, _)| *id == session)
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
                        .map(|(session, size)| PaneTarget {
                            session: *session,
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
    ) -> bool {
        revision == self.revision
            && self.pending.as_ref().is_some_and(|pending| {
                pending.request_id == request_id
                    && pending.view.revision == revision
                    && pending.view.contains(session)
            })
    }

    pub(super) fn record_snapshot(&mut self, session: SessionId) {
        self.snapshots.insert(session);
    }

    /// The final `Ok` for `request_id`. `None` when it is not the in-flight request.
    pub(super) fn acknowledge(
        &mut self,
        request_id: u64,
        desired_now: &RequestedView,
        now: Instant,
    ) -> Option<Acknowledged> {
        if self.pending.as_ref()?.request_id != request_id {
            return None;
        }
        let pending = self.pending.take().expect("checked above");
        let complete = pending
            .view
            .targets
            .iter()
            .all(|(session, _)| self.snapshots.contains(session));
        self.snapshots.clear();
        if !complete {
            // `Ok` is final and every snapshot precedes it, so a missing one is a failed view
            // rather than one still arriving. The caller clears this record for its immediate
            // retry and records it again, so a server that keeps answering without snapshots
            // is throttled.
            self.force_refresh = true;
            self.failed = Some((pending.view.clone(), now));
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

    /// Lets the immediate retry after an `Incomplete` through the backoff.
    pub(super) fn clear_failed(&mut self) {
        self.failed = None;
    }

    /// Throttles the retry after the immediate one.
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
                .map(|id| (SessionId(*id), TerminalSize { rows: 10, cols: 20 }))
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
        assert!(h.snapshot_matches(10, revision, SessionId(1)));
        assert!(
            !h.snapshot_matches(10, revision + 1, SessionId(1)),
            "wrong revision"
        );
        assert!(
            !h.snapshot_matches(99, revision, SessionId(1)),
            "wrong request"
        );
        assert!(
            !h.snapshot_matches(10, revision, SessionId(3)),
            "wrong session"
        );
        h.record_snapshot(SessionId(1));
        let Some(Acknowledged::Incomplete { view: incomplete }) =
            h.acknowledge(10, &v, Instant::now())
        else {
            panic!("a missing snapshot must not complete the view");
        };
        assert!(!h.is_ready(SessionId(1)));
        // The caller's immediate retry, then the throttle for the one after it.
        h.clear_failed();
        let revision = send(&mut h, &v, 12);
        h.record_failure(incomplete);
        h.record_snapshot(SessionId(1));
        h.record_snapshot(SessionId(2));
        assert!(matches!(
            h.acknowledge(12, &v, Instant::now()),
            Some(Acknowledged::Granted)
        ));
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
        assert!(matches!(
            h.acknowledge(1, &v, Instant::now()),
            Some(Acknowledged::Granted)
        ));
        h.invalidate();
        assert!(!h.is_ready(SessionId(1)));
        let other = view(&[2]);
        send(&mut h, &other, 2);
        assert!(
            h.acknowledge(1, &other, Instant::now()).is_none(),
            "late Ok for a retired request"
        );
        assert!(h.refuse(1, Instant::now()).is_none());
        h.record_snapshot(SessionId(2));
        assert!(matches!(
            h.acknowledge(2, &other, Instant::now()),
            Some(Acknowledged::Granted)
        ));
        h.mark_stale(SessionId(2));
        assert!(!h.is_ready(SessionId(2)));
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
        h.acknowledge(1, &v, Instant::now());
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
