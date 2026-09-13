//! Dashboard Unread identity: Presented, the last Ready identity per session, and the
//! second, independent lane of the last open Input request per session. An Input request is
//! never Unread and never a review target; the lane exists only so a Ready and a request for
//! the same cycle cannot suppress each other's alert.
use ovrcr_protocol::{AgentBinding, InputRequest, ReadyObservation, SessionId, SessionSummary};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Unread {
    presented: Option<(SessionId, ReadyObservation)>,
    observed: HashMap<SessionId, ReadyObservation>,
    requests: HashMap<SessionId, (AgentBinding, InputRequest)>,
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

    /// Records this session's open Input request. True when a request opened or was replaced.
    pub(super) fn observe_request(&mut self, session: &SessionSummary) -> bool {
        let Some(request) = super::ready::input_live(session) else {
            return false;
        };
        let binding = &session
            .agent
            .as_ref()
            .expect("input_live checked it")
            .binding;
        let new = self
            .requests
            .get(&session.id)
            .is_none_or(|(previous, open)| previous != binding || open.id != request.id);
        self.requests
            .insert(session.id, (binding.clone(), request.clone()));
        new
    }

    pub(super) fn retain(&mut self, existing: &HashSet<SessionId>) {
        self.observed.retain(|id, _| existing.contains(id));
        self.requests.retain(|id, _| existing.contains(id));
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

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{
        ActivitySample, AgentActivity, AgentBinding, AgentProvider, AgentSnapshot, HealthSample,
        InputKind, InputRequest, ReporterHealth, SampleQuality, SessionPhase,
    };

    fn binding(provider: AgentProvider) -> AgentBinding {
        AgentBinding {
            provider,
            invocation: "i".into(),
            conversation: "c".into(),
            generation: 1,
        }
    }

    fn session(id: u64, open: Option<(&str, InputKind)>) -> SessionSummary {
        SessionSummary {
            id: SessionId(id),
            project: "p".into(),
            workspace: "w".into(),
            name: "s".into(),
            label: "l".into(),
            pid: Some(1),
            started_unix_ms: 0,
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent_epoch: 1,
            unread: None,
            agent: Some(AgentSnapshot {
                binding: binding(AgentProvider::Pi),
                activity: Some(ActivitySample {
                    state: AgentActivity::Busy,
                    quality: SampleQuality::Observed,
                    turn: Some("t".into()),
                }),
                metrics: None,
                health: HealthSample {
                    state: ReporterHealth::Connected,
                    reason: None,
                },
                activity_revision: 1,
                metrics_revision: 0,
                health_revision: 0,
                input_request: open.map(|(id, kind)| InputRequest {
                    id: id.into(),
                    kind,
                }),
                input_revision: 2,
            }),
        }
    }

    #[test]
    fn observe_request_is_true_only_for_a_newly_opened_or_replaced_identity() {
        let mut unread = Unread::default();
        assert!(!unread.observe_request(&session(1, None)), "no request");
        assert!(unread.observe_request(&session(1, Some(("p1", InputKind::Select)))));
        assert!(
            !unread.observe_request(&session(1, Some(("p1", InputKind::Select)))),
            "the same identity is not new"
        );
        assert!(
            unread.observe_request(&session(1, Some(("p2", InputKind::Confirm)))),
            "a replacement is new"
        );
        assert!(
            !unread.observe_request(&session(1, None)),
            "a close is not an opening"
        );
        let mut rebound = session(1, Some(("p2", InputKind::Confirm)));
        rebound.agent.as_mut().unwrap().binding.generation = 2;
        assert!(
            unread.observe_request(&rebound),
            "the same id under a new binding is a different request"
        );
    }

    #[test]
    fn observe_request_requires_a_live_supported_session_and_retain_prunes_both_lanes() {
        let mut unread = Unread::default();
        let mut claude = session(1, Some(("p1", InputKind::Select)));
        claude.agent.as_mut().unwrap().binding.provider = AgentProvider::Claude;
        assert!(
            !unread.observe_request(&claude),
            "Claude's waits carry no Input request lane"
        );
        let mut exited = session(1, Some(("p1", InputKind::Select)));
        exited.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        assert!(!unread.observe_request(&exited));
        let mut lost = session(1, Some(("p1", InputKind::Select)));
        lost.agent.as_mut().unwrap().health.state = ReporterHealth::Unavailable;
        assert!(!unread.observe_request(&lost));

        assert!(unread.observe_request(&session(1, Some(("p1", InputKind::Select)))));
        let mut ready = session(1, None);
        ready.unread = Some(ovrcr_protocol::ReadyObservation {
            binding: binding(AgentProvider::Pi),
            turn: Some("t".into()),
            activity_revision: 2,
        });
        assert!(unread.observe(&ready));
        assert_eq!(unread.requests.len(), 1);
        assert_eq!(unread.observed.len(), 1);
        unread.retain(&HashSet::new());
        assert!(unread.requests.is_empty() && unread.observed.is_empty());
    }
}
