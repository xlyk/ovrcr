//! Ready (CONTEXT.md): the accepted root observation of a completed response cycle from a
//! supported readiness provider. This is the only place the Dashboard decides whether a
//! session is ready and whether that readiness may be delivered as an alert. The sidebar
//! glyph, the metadata line, and desktop delivery all ask here, so they cannot disagree.
use ovrcr_protocol::{
    ActivitySample, AgentActivity, InputRequest, ReporterHealth, SessionPhase, SessionSummary,
};

pub(super) fn activity(session: &SessionSummary) -> AgentActivity {
    session
        .agent
        .as_ref()
        .map_or(session.activity, |agent| agent.effective_activity())
}

/// The open Input request of a running, connected, supported session: the one identity the
/// Dashboard may deliver as an Input needed alert.
pub(super) fn input_live(session: &SessionSummary) -> Option<&InputRequest> {
    if session.phase != SessionPhase::Running {
        return None;
    }
    let agent = session.agent.as_ref()?;
    if !agent.binding.provider.supports_readiness()
        || agent.health.state != ReporterHealth::Connected
    {
        return None;
    }
    agent.input_request.as_ref()
}

pub(super) fn ready(session: &SessionSummary) -> Option<&ActivitySample> {
    session
        .agent
        .as_ref()?
        .activity
        .as_ref()
        .filter(|sample| sample.state == AgentActivity::ResponseReady)
}

pub(super) fn delivery_live(session: &SessionSummary) -> bool {
    session.phase == SessionPhase::Running
        && session.agent.as_ref().is_some_and(|agent| {
            agent.binding.provider.supports_readiness()
                && agent.health.state == ReporterHealth::Connected
        })
        && ready(session)
            .and_then(|sample| sample.turn.as_deref())
            .is_some_and(|turn| !turn.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{
        ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, ReporterHealth,
        SampleQuality, SessionId, SessionPhase,
    };

    fn session(
        phase: SessionPhase,
        provider: AgentProvider,
        health: ReporterHealth,
        state: AgentActivity,
        turn: Option<&str>,
    ) -> SessionSummary {
        SessionSummary {
            id: SessionId(1),
            project: "p".into(),
            workspace: "w".into(),
            name: "s".into(),
            label: "l".into(),
            pid: Some(1),
            started_unix_ms: 0,
            phase,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent_epoch: 1,
            unread: None,
            agent: Some(AgentSnapshot {
                binding: AgentBinding {
                    provider,
                    invocation: "i".into(),
                    conversation: "c".into(),
                    generation: 1,
                },
                activity: Some(ActivitySample {
                    state,
                    quality: SampleQuality::Observed,
                    turn: turn.map(Into::into),
                }),
                metrics: None,
                health: HealthSample {
                    state: health,
                    reason: None,
                },
                activity_revision: 1,
                metrics_revision: 0,
                health_revision: 0,
                input_request: None,
                input_revision: 0,
            }),
        }
    }

    #[test]
    fn readiness_table() {
        use AgentActivity::*;
        use AgentProvider::*;
        use ReporterHealth::*;
        use SessionPhase::*;
        let cases = [
            // phase, provider, health, state, turn, expect ready, expect live
            (
                Running,
                Codex,
                Connected,
                ResponseReady,
                Some("t"),
                true,
                true,
            ),
            (
                Running,
                Codex,
                Unavailable,
                ResponseReady,
                Some("t"),
                true,
                false,
            ),
            (
                Running,
                Codex,
                Connected,
                ResponseReady,
                Some(""),
                true,
                false,
            ),
            (Running, Codex, Connected, ResponseReady, None, true, false),
            (
                Running,
                Claude,
                Connected,
                ResponseReady,
                Some("t"),
                true,
                false,
            ),
            (Running, Pi, Connected, ResponseReady, Some("t"), true, true),
            (
                Running,
                Omp,
                Connected,
                ResponseReady,
                Some("t"),
                true,
                true,
            ),
            (
                Running,
                Grok,
                Connected,
                ResponseReady,
                Some("t"),
                true,
                false,
            ),
            (
                Running,
                Pi,
                Unavailable,
                ResponseReady,
                Some("t"),
                true,
                false,
            ),
            (
                Paused,
                Codex,
                Connected,
                ResponseReady,
                Some("t"),
                true,
                false,
            ),
            (Running, Codex, Connected, Busy, Some("t"), false, false),
        ];
        for (phase, provider, health, state, turn, want_ready, want_live) in cases {
            let s = session(phase.clone(), provider, health, state, turn);
            assert_eq!(
                ready(&s).is_some(),
                want_ready,
                "{phase:?} {provider:?} {health:?} {state:?} {turn:?}"
            );
            assert_eq!(
                delivery_live(&s),
                want_live,
                "{phase:?} {provider:?} {health:?} {state:?} {turn:?}"
            );
            assert_eq!(activity(&s), state);
        }
    }

    #[test]
    fn activity_falls_back_to_the_rollup_without_an_agent() {
        let mut s = session(
            SessionPhase::Running,
            AgentProvider::Codex,
            ReporterHealth::Connected,
            AgentActivity::Busy,
            None,
        );
        s.agent = None;
        s.activity = AgentActivity::WaitingInput;
        assert_eq!(activity(&s), AgentActivity::WaitingInput);
        assert!(ready(&s).is_none());
    }
}
