//! Dashboard Unread identity: Presented, and the last Ready identity per session.
use ovrcr_protocol::{ReadyObservation, SessionId, SessionSummary};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Unread {
    presented: Option<(SessionId, ReadyObservation)>,
    observed: HashMap<SessionId, ReadyObservation>,
}

impl Unread {
    /// Records this session's Unread identity. True when binding or turn is new.
    pub(super) fn observe(&mut self, session: &SessionSummary) -> bool {
        let Some(unread) = &session.unread else {
            return false;
        };
        let new = self.observed.get(&session.id).is_none_or(|previous| {
            previous.binding != unread.binding || previous.turn != unread.turn
        });
        self.observed.insert(session.id, unread.clone());
        new
    }

    pub(super) fn retain(&mut self, existing: &HashSet<SessionId>) {
        self.observed.retain(|id, _| existing.contains(id));
    }

    pub(super) fn commit_presented(&mut self, presented: Option<(SessionId, ReadyObservation)>) {
        self.presented = presented;
    }

    pub(super) fn review_target(&self, session: SessionId) -> Option<ReadyObservation> {
        self.presented
            .as_ref()
            .filter(|(id, _)| *id == session)
            .map(|(_, ready)| ready.clone())
    }
}
