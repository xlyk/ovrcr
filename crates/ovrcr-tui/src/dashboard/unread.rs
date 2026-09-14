//! Dashboard Unread identity: Presented, the last Ready identity per session, and the
//! second, independent lane of the open Input requests per session. An Input request is
//! never Unread and never a review target; the lane exists only so a Ready and a request for
//! the same cycle cannot suppress each other's alert. The lane charges each request id
//! once per binding, so several open at once alert once each and none of them twice.
use ovrcr_protocol::{AgentBinding, InputRequest, ReadyObservation, SessionId, SessionSummary};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Unread {
    presented: Option<(SessionId, ReadyObservation)>,
    observed: HashMap<SessionId, ReadyObservation>,
    /// Per session, the binding and the request ids already delivered under it. A
    /// rebinding resets the set: the same id under a new generation is a new request.
    requests: HashMap<SessionId, (AgentBinding, HashSet<String>)>,
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

    /// Records this session's open Input requests and returns the ones seen for the first
    /// time under the current binding. A closed and reopened id is new again; a
    /// republication of the same set returns nothing.
    pub(super) fn observe_request(&mut self, session: &SessionSummary) -> Vec<InputRequest> {
        let live = super::ready::input_live(session);
        if live.is_empty() {
            // Nothing open: forget the delivered ids so a later reopening alerts again,
            // and leave the entry only while a binding is worth remembering.
            self.requests.remove(&session.id);
            return Vec::new();
        }
        let binding = &session
            .agent
            .as_ref()
            .expect("input_live returns nothing without an agent")
            .binding;
        let (known, seen) = self
            .requests
            .entry(session.id)
            .or_insert_with(|| (binding.clone(), HashSet::new()));
        if known != binding {
            *known = binding.clone();
            seen.clear();
        }
        // Only the ids still open stay charged: an id that closed is forgotten here.
        seen.retain(|id| live.iter().any(|request| &request.id == id));
        live.iter()
            .filter(|request| seen.insert(request.id.clone()))
            .cloned()
            .collect()
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

    fn session(id: u64, open: &[(&str, InputKind)]) -> SessionSummary {
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
                input_requests: open
                    .iter()
                    .map(|(id, kind)| InputRequest {
                        id: (*id).into(),
                        kind: *kind,
                    })
                    .collect(),
                input_revision: 2,
            }),
        }
    }

    #[test]
    fn observe_request_returns_exactly_the_ids_seen_for_the_first_time() {
        fn seen(unread: &mut Unread, open: &[(&str, InputKind)]) -> Vec<String> {
            unread
                .observe_request(&session(1, open))
                .into_iter()
                .map(|request| request.id)
                .collect()
        }
        let a = ("approval:c1", InputKind::Approval);
        let b = ("question:q1", InputKind::Select);
        let mut unread = Unread::default();
        assert!(seen(&mut unread, &[]).is_empty(), "nothing open");
        assert_eq!(seen(&mut unread, &[a]), ["approval:c1"]);
        assert_eq!(
            seen(&mut unread, &[a]),
            Vec::<String>::new(),
            "a republication of the same set is not new"
        );
        assert_eq!(
            seen(&mut unread, &[a, b]),
            ["question:q1"],
            "only the request that joined the set is new"
        );
        assert_eq!(
            seen(&mut unread, &[b]),
            Vec::<String>::new(),
            "closing one member does not re-announce the other"
        );
        assert!(
            seen(&mut unread, &[]).is_empty(),
            "a close is not an opening"
        );
        assert_eq!(
            seen(&mut unread, &[b]),
            ["question:q1"],
            "the same id after a close is a new request"
        );
        // The same mechanism makes a paused and recovered reporter unable to replay an
        // alert: every recovery is a forced rebind, so the ids of the generation that
        // paused are not the ids of the one that came back.
        let mut rebound = session(1, &[b]);
        rebound.agent.as_mut().unwrap().binding.generation = 2;
        assert_eq!(
            unread
                .observe_request(&rebound)
                .into_iter()
                .map(|request| request.id)
                .collect::<Vec<_>>(),
            ["question:q1"],
            "the same id under a new binding is a different request"
        );
    }

    #[test]
    fn observe_request_requires_a_live_supported_session_and_retain_prunes_both_lanes() {
        let open = [("approval:c1", InputKind::Approval)];
        let mut unread = Unread::default();
        let mut claude = session(1, &open);
        claude.agent.as_mut().unwrap().binding.provider = AgentProvider::Claude;
        assert!(
            unread.observe_request(&claude).is_empty(),
            "Claude's waits carry no Input request lane"
        );
        let mut exited = session(1, &open);
        exited.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        assert!(unread.observe_request(&exited).is_empty());
        let mut lost = session(1, &open);
        lost.agent.as_mut().unwrap().health.state = ReporterHealth::Unavailable;
        assert!(unread.observe_request(&lost).is_empty());

        assert_eq!(unread.observe_request(&session(1, &open)).len(), 1);
        let mut ready = session(1, &[]);
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
