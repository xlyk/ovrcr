//! One status value for a session: the glyph the sidebar row draws and the process, paused,
//! activity, Unread and elapsed clauses both pane headers lay out. Which clause a session
//! gets, and its wording, live here once, so the sidebar and the two metadata lines cannot
//! disagree about the same session; an Unread clause replaces a header line outright, and
//! each surface arranges and clips the rest for its own width. The value carries the glyph's
//! colour, because the colour is part of what the status says; text styles, widths and
//! clipping stay with the renderer. Readiness itself is still decided in [`super::ready`].
use super::render::{GREEN, MUTED, RED, SUBTEXT, TEAL, YELLOW};
use ovrcr_protocol::{
    AgentActivity, AgentSnapshot, InputKind, ReporterHealth, SampleQuality, SessionPhase,
    SessionSummary,
};
use ratatui::style::Color;
use std::time::Duration;

const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const SPINNER_FRAME_MS: u64 = 100;
/// How long one spinner frame stands: the redraw cadence the Dashboard owes a busy session.
pub(super) const SPINNER_INTERVAL: Duration = Duration::from_millis(SPINNER_FRAME_MS);

/// The glyph and colour a session holds still at, or `None` when its glyph is a moving
/// spinner frame — the one state that needs the clock. [`busy`] and [`SessionStatus::of`]
/// both read this, so the redraw cadence and the drawn glyph are a single decision.
fn fixed_glyph(session: &SessionSummary) -> Option<(char, Color)> {
    Some(match (&session.phase, super::ready::activity(session)) {
        (SessionPhase::Exited { .. } | SessionPhase::Stopped, _) => ('·', MUTED()),
        (SessionPhase::Interrupted, _) => ('!', YELLOW()),
        (SessionPhase::Paused, _) => ('P', SUBTEXT()),
        (_, AgentActivity::Unknown) => ('-', MUTED()),
        (_, AgentActivity::Idle) => (' ', MUTED()),
        (_, AgentActivity::WaitingInput) => ('?', YELLOW()),
        (_, AgentActivity::Error) => ('!', RED()),
        // A reviewed Ready response keeps its reported activity. The checkmark is the
        // unreviewed mark, so it leaves when Unread does.
        (_, AgentActivity::ResponseReady) if session.unread.is_some() => ('✓', TEAL()),
        (_, AgentActivity::ResponseReady) => (' ', MUTED()),
        (SessionPhase::Running, AgentActivity::Busy) => return None,
    })
}

/// A session whose glyph is a spinner frame, so the Dashboard owes it the spinner cadence.
/// The clock picks which frame, never whether there is one, so this needs no clock.
pub(super) fn busy(session: &SessionSummary) -> bool {
    fixed_glyph(session).is_none()
}

/// What one session says about itself.
pub(super) struct SessionStatus {
    /// Sidebar glyph and its colour. An unavailable reporter mutes the colour.
    pub glyph: char,
    pub color: Color,
    pub exited: bool,
    pub live: bool,
    pub paused: bool,
    /// A Ready observation is present: the headers give its wording the whole line.
    pub ready: bool,
    /// The managed process: its pid, `closed` after exit, or `—`.
    pub pid: String,
    pub elapsed: String,
    /// The activity clause the headers append, leading space included: ` busy observed`,
    /// ` input needed · confirm`, ` unavailable`, or ` idle` without an agent.
    pub activity: String,
    /// Reporting loss also remains textual when an Input request overrides activity.
    pub reporting_health: Option<String>,
    /// Essential Agent state for a narrow picker row, without explanatory detail.
    pub compact: String,
    /// Recovery explanations remain actionable after the managed process exits.
    pub recovery_diagnostic: bool,
    /// The Unread clause, which replaces a header line outright when present.
    pub unread: Option<String>,
}

impl SessionStatus {
    pub(super) fn of(session: &SessionSummary, now_unix_ms: u64) -> Self {
        let exited = matches!(session.phase, SessionPhase::Exited { .. });
        let (glyph, color) = fixed_glyph(session).unwrap_or((
            SPINNER_FRAMES
                [((now_unix_ms / SPINNER_FRAME_MS) % SPINNER_FRAMES.len() as u64) as usize],
            GREEN(),
        ));
        Self {
            glyph,
            color: if session.agent.as_ref().is_some_and(unavailable) {
                MUTED()
            } else {
                color
            },
            exited,
            live: session.phase.is_live(),
            paused: matches!(session.phase, SessionPhase::Paused),
            ready: super::ready::ready(session).is_some(),
            pid: if exited {
                "closed".to_string()
            } else {
                session
                    .pid
                    .map_or_else(|| "—".to_string(), |pid| pid.to_string())
            },
            elapsed: session
                .started_unix_ms
                .map(|started| format_elapsed_at(started, now_unix_ms))
                .unwrap_or_default(),
            activity: activity_clause(session),
            compact: compact_agent_clause(session),
            reporting_health: session
                .agent
                .as_ref()
                .filter(|agent| unavailable(agent))
                .map(unavailable_clause),
            recovery_diagnostic: session.recovery.as_ref().is_some_and(|recovery| {
                recovery.unavailable.is_some()
                    || recovery.failure.is_some()
                    || recovery.requires_ack
            }),
            unread: session.unread.as_ref().map(|_| unread_clause(session)),
        }
    }
}

fn format_elapsed_at(started_unix_ms: u64, now_unix_ms: u64) -> String {
    let minutes = now_unix_ms.saturating_sub(started_unix_ms) / 60_000;
    let days = minutes / (24 * 60);
    let hours = minutes / 60 % 24;
    let minutes = minutes % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h{minutes:02}")
    } else {
        format!("{minutes}m")
    }
}

/// The reporter is not currently connected. Callers differ on what a session with no agent
/// at all means: the glyph has nothing to mute (`is_some_and`), while an Unread with no
/// reporter behind it has nobody left to confirm it and reads Unavailable (`is_none_or`).
fn unavailable(agent: &AgentSnapshot) -> bool {
    agent.health.state == ReporterHealth::Unavailable
}

fn compact_agent_clause(session: &SessionSummary) -> String {
    if session.unread.is_some() && session.agent.is_none() {
        return unread_clause(session);
    }
    let activity = super::ready::activity(session);
    let lost = session.agent.as_ref().is_some_and(unavailable);
    let state = if lost && activity != AgentActivity::WaitingInput {
        "Unavailable"
    } else {
        match activity {
            AgentActivity::Unknown => "Unknown",
            AgentActivity::Idle => "Idle",
            AgentActivity::Busy => "Busy",
            AgentActivity::WaitingInput => "Input",
            AgentActivity::ResponseReady => "Ready",
            AgentActivity::Error => "Error",
        }
    };
    let mut text = if session.unread.is_some() {
        format!("Unread {state}")
    } else {
        state.to_owned()
    };
    if lost && activity == AgentActivity::WaitingInput {
        text.push_str(if session.unread.is_some() {
            " Unavail"
        } else {
            " Unavailable"
        });
    } else if !lost
        && activity != AgentActivity::WaitingInput
        && let Some(sample) = session.agent.as_ref().and_then(|a| a.activity.as_ref())
    {
        text.push(' ');
        text.push_str(quality(sample.quality));
    }
    text
}

fn unread_clause(session: &SessionSummary) -> String {
    let health = if session.agent.as_ref().is_none_or(unavailable) {
        " Unavailable"
    } else {
        ""
    };
    // Unread names the reporting provider's own state, so a session without one says nothing
    // more; the rollup fallback below belongs to the live activity label alone.
    let activity = session.agent.as_ref().map_or_else(String::new, |agent| {
        let clause = provider_clause(session, agent);
        if unavailable(agent) {
            let marker = format!(" {}", unavailable_clause(agent));
            clause.replacen(&marker, "", 1)
        } else {
            clause
        }
    });
    format!("Unread{health}{activity}")
}

fn activity_clause(session: &SessionSummary) -> String {
    if let Some(recovery) = &session.recovery {
        let mut parts = Vec::new();
        if let Some(unavailable) = &recovery.unavailable {
            parts.push(unavailable.clone());
        }
        if let Some(failure) = &recovery.failure {
            parts.push(failure.clone());
        }
        if session.phase.is_live()
            && recovery.conversation.is_some()
            && !recovery.attached
            && recovery.unavailable.is_none()
        {
            parts.push("Agent launched; awaiting conversation attachment".into());
        }
        if recovery.requires_ack {
            parts.push("confirm previous processes stopped".into());
        }
        if !parts.is_empty() {
            if session.phase.is_live()
                && let Some(agent) = &session.agent
            {
                parts.insert(0, provider_clause(session, agent).trim_start().into());
            }
            return format!(" {}", parts.join(" · "));
        }
    }
    match session.phase {
        SessionPhase::Stopped => return " stopped".into(),
        SessionPhase::Interrupted => return " interrupted".into(),
        _ => {}
    }
    let Some(agent) = &session.agent else {
        return match session.activity {
            AgentActivity::Unknown => " unknown",
            AgentActivity::Idle => " idle",
            AgentActivity::Busy => " busy",
            AgentActivity::WaitingInput => " waiting input",
            AgentActivity::Error => " error",
            AgentActivity::ResponseReady => " response ready",
        }
        .to_owned();
    };
    provider_clause(session, agent)
}

fn provider_clause(session: &SessionSummary, agent: &AgentSnapshot) -> String {
    // An open Input request outranks whatever is underneath it, exactly as
    // AgentSnapshot::effective_activity does.
    if let Some(request) = agent.input_requests.first() {
        return format!(
            " input needed · {}",
            match request.kind {
                InputKind::Select => "select",
                InputKind::Confirm => "confirm",
                InputKind::Input => "input",
                InputKind::Editor => "editor",
                InputKind::Custom => "custom",
                InputKind::Approval => "approval",
            }
        );
    }
    if let Some(activity) = super::ready::ready(session) {
        let quality = quality(activity.quality);
        let health = if unavailable(agent) {
            format!(" {}", unavailable_clause(agent))
        } else {
            String::new()
        };
        return format!("{health} response ready · {quality}");
    }
    if unavailable(agent) {
        return format!(" {}", unavailable_clause(agent));
    }
    let Some(activity) = &agent.activity else {
        return " unknown".into();
    };
    let state = match activity.state {
        AgentActivity::Unknown => "unknown",
        AgentActivity::Idle => "idle",
        AgentActivity::Busy => "busy",
        AgentActivity::WaitingInput => "waiting",
        AgentActivity::Error => "error",
        AgentActivity::ResponseReady => "response ready",
    };
    format!(" {state} {}", quality(activity.quality))
}

/// Observed reporting loss: keep the stable `unavailable` token so Unread and narrow
/// headers stay consistent, and append the most specific reason the reporter published.
/// Silence before evidence never reaches this path. Capability completeness (Ready vs
/// Input-request) is reported by `ovrcr agent doctor`, not inferred from this clause.
fn unavailable_clause(agent: &AgentSnapshot) -> String {
    match agent
        .health
        .reason
        .as_deref()
        .filter(|reason| !reason.is_empty())
    {
        Some(reason) => format!("unavailable · {reason}"),
        None => "unavailable".into(),
    }
}

fn quality(quality: SampleQuality) -> &'static str {
    match quality {
        SampleQuality::Confirmed => "confirmed",
        SampleQuality::Observed => "observed",
        SampleQuality::Estimated => "estimated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{
        ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, InputRequest,
        ReadyObservation, SessionId,
    };

    #[test]
    fn codex_resume_awaits_attachment_before_ready_without_claiming_unavailable() {
        let mut row = session(SessionPhase::Running, None);
        row.kind = ovrcr_protocol::SessionKind::Agent {
            name: "codex".into(),
        };
        row.recovery = Some(ovrcr_protocol::SessionRecovery {
            conversation: Some("exact-native-id".into()),
            attached: false,
            requires_ack: false,
            unavailable: None,
            failure: None,
        });
        let clause = activity_clause(&row);
        assert!(
            clause.contains("awaiting conversation attachment"),
            "{clause}"
        );
        assert!(!clause.contains("reporting unavailable"), "{clause}");
        assert!(!clause.contains("response ready"), "{clause}");
    }

    #[test]
    fn recovery_unavailability_does_not_hide_live_response_ready() {
        let mut row = session(
            SessionPhase::Running,
            Some(agent(
                AgentActivity::ResponseReady,
                SampleQuality::Observed,
                ReporterHealth::Connected,
                vec![],
            )),
        );
        row.recovery = Some(ovrcr_protocol::SessionRecovery {
            conversation: Some("non-resumable".into()),
            attached: true,
            requires_ack: false,
            unavailable: Some("No native history".into()),
            failure: None,
        });
        let clause = activity_clause(&row);
        assert!(clause.contains("response ready"), "{clause}");
        assert!(clause.contains("No native history"), "{clause}");
    }

    #[test]
    fn exited_empty_recovery_does_not_restore_stale_activity() {
        let mut row = session(
            SessionPhase::Exited {
                code: Some(0),
                signal: None,
            },
            None,
        );
        row.activity = AgentActivity::Busy;
        row.recovery = Some(ovrcr_protocol::SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: false,
            unavailable: None,
            failure: None,
        });
        assert!(!SessionStatus::of(&row, 0).recovery_diagnostic);
    }

    fn session(phase: SessionPhase, agent: Option<AgentSnapshot>) -> SessionSummary {
        SessionSummary {
            archived: false,
            cwd: "/work".into(),
            id: SessionId(1),
            run: crate::protocol::SessionRunId(1),
            kind: crate::protocol::SessionKind::Terminal,
            recovery: None,
            project: "p".into(),
            workspace: "w".into(),
            name: "s".into(),
            title: None,
            manual_title: None,
            label: "claude/sonnet".into(),
            pid: Some(42),
            started_unix_ms: Some(0),
            phase,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent_epoch: 1,
            unread: None,
            agent,
        }
    }

    fn agent(
        state: AgentActivity,
        quality: SampleQuality,
        health: ReporterHealth,
        requests: Vec<InputKind>,
    ) -> AgentSnapshot {
        AgentSnapshot {
            binding: AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "i".into(),
                conversation: "c".into(),
                generation: 1,
            },
            activity: Some(ActivitySample {
                state,
                quality,
                turn: Some("t".into()),
            }),
            metrics: None,
            health: HealthSample {
                state: health,
                reason: None,
            },
            activity_revision: 1,
            metrics_revision: 0,
            health_revision: 0,
            input_requests: requests
                .into_iter()
                .enumerate()
                .map(|(index, kind)| InputRequest {
                    id: format!("request-{index}"),
                    kind,
                })
                .collect(),
            input_revision: 1,
        }
    }

    /// Phase x effective activity x reporter health, at the glyph and the activity clause.
    #[test]
    fn status_table() {
        use AgentActivity::*;
        use ReporterHealth::*;
        use SampleQuality::*;
        let running = SessionPhase::Running;
        let exited = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        // Eight columns per row: phase, agent snapshot, then the expected glyph, glyph
        // colour, activity clause, pid text, ready and paused. `exited` is checked against
        // the pid text and `busy` against the glyph, so neither needs a column of its own;
        // the Unread clause has its own test below.
        let cases = [
            (
                running.clone(),
                None,
                '-',
                MUTED(),
                " unknown",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(Unknown, Observed, Connected, vec![])),
                '-',
                MUTED(),
                " unknown observed",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(Idle, Confirmed, Connected, vec![])),
                ' ',
                MUTED(),
                " idle confirmed",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(Busy, Observed, Connected, vec![])),
                '⠋',
                GREEN(),
                " busy observed",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(WaitingInput, Estimated, Connected, vec![])),
                '?',
                YELLOW(),
                " waiting estimated",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(Error, Observed, Connected, vec![])),
                '!',
                RED(),
                " error observed",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(ResponseReady, Confirmed, Connected, vec![])),
                ' ',
                MUTED(),
                " response ready · confirmed",
                "42",
                true,
                false,
            ),
            // An unavailable reporter mutes the glyph and says so in the clause.
            (
                running.clone(),
                Some(agent(Busy, Observed, Unavailable, vec![])),
                '⠋',
                MUTED(),
                " unavailable",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(ResponseReady, Observed, Unavailable, vec![])),
                ' ',
                MUTED(),
                " unavailable response ready · observed",
                "42",
                true,
                false,
            ),
            // An open Input request outranks the activity underneath it, glyph included.
            (
                running.clone(),
                Some(agent(Busy, Observed, Connected, vec![InputKind::Confirm])),
                '?',
                YELLOW(),
                " input needed · confirm",
                "42",
                false,
                false,
            ),
            (
                running.clone(),
                Some(agent(
                    ResponseReady,
                    Observed,
                    Connected,
                    vec![InputKind::Approval, InputKind::Select],
                )),
                '?',
                YELLOW(),
                " input needed · approval",
                "42",
                true,
                false,
            ),
            // Phase outranks every activity.
            (
                SessionPhase::Paused,
                Some(agent(Busy, Observed, Connected, vec![])),
                'P',
                SUBTEXT(),
                " busy observed",
                "42",
                false,
                true,
            ),
            (
                exited.clone(),
                Some(agent(Busy, Observed, Connected, vec![])),
                '·',
                MUTED(),
                " busy observed",
                "closed",
                false,
                false,
            ),
            (
                exited,
                Some(agent(ResponseReady, Observed, Connected, vec![])),
                '·',
                MUTED(),
                " response ready · observed",
                "closed",
                true,
                false,
            ),
        ];
        for (phase, snapshot, glyph, color, activity, pid, ready, paused) in cases {
            let session = session(phase.clone(), snapshot);
            let status = SessionStatus::of(&session, 0);
            let case = format!("{phase:?} {activity}");
            assert_eq!(status.glyph, glyph, "{case}");
            assert_eq!(status.color, color, "{case}");
            assert_eq!(status.activity, activity, "{case}");
            assert_eq!(status.pid, pid, "{case}");
            assert_eq!(status.ready, ready, "{case}");
            assert_eq!(status.paused, paused, "{case}");
            assert_eq!(status.exited, pid == "closed", "{case}");
            assert_eq!(status.unread, None, "{case}");
            // The redraw cadence and the glyph read the same activity.
            assert_eq!(
                busy(&session),
                SPINNER_FRAMES.contains(&status.glyph),
                "{case}"
            );
        }
    }

    #[test]
    fn unread_outranks_the_activity_and_keeps_reporter_health() {
        let mut ready = session(
            SessionPhase::Running,
            Some(agent(
                AgentActivity::ResponseReady,
                SampleQuality::Observed,
                ReporterHealth::Connected,
                Vec::new(),
            )),
        );
        ready.unread = Some(ReadyObservation {
            binding: ready.agent.as_ref().unwrap().binding.clone(),
            turn: Some("t".into()),
            activity_revision: 1,
        });
        assert_eq!(
            SessionStatus::of(&ready, 0).unread.as_deref(),
            Some("Unread response ready · observed")
        );
        assert_eq!(SessionStatus::of(&ready, 0).glyph, '✓');
        ready.agent.as_mut().unwrap().health.state = ReporterHealth::Unavailable;
        assert_eq!(
            SessionStatus::of(&ready, 0).unread.as_deref(),
            Some("Unread Unavailable response ready · observed")
        );
        // Without a reporting provider there is no clause to add, only the health word.
        ready.agent = None;
        assert_eq!(
            SessionStatus::of(&ready, 0).unread.as_deref(),
            Some("Unread Unavailable")
        );

        assert_eq!(SessionStatus::of(&ready, 0).compact, "Unread Unavailable");
        ready.unread = None;
        ready.activity = AgentActivity::Unknown;
        assert_eq!(SessionStatus::of(&ready, 0).compact, "Unknown");

        // An open Input request outranks the Ready sample inside the Unread clause too, and
        // the glyph goes with it while the Unread itself stands.
        let mut waiting = session(
            SessionPhase::Running,
            Some(agent(
                AgentActivity::ResponseReady,
                SampleQuality::Observed,
                ReporterHealth::Connected,
                vec![InputKind::Confirm],
            )),
        );
        waiting.unread = Some(ReadyObservation {
            binding: waiting.agent.as_ref().unwrap().binding.clone(),
            turn: Some("t".into()),
            activity_revision: 1,
        });
        let status = SessionStatus::of(&waiting, 0);
        assert_eq!(
            status.unread.as_deref(),
            Some("Unread input needed · confirm")
        );
        assert_eq!(status.glyph, '?');

        // An unavailable reporter under a sample that is not Ready says ` unavailable` in the
        // activity clause; Unread already carries the health word, so it is not repeated.
        let snapshot = waiting.agent.as_mut().unwrap();
        snapshot.input_requests.clear();
        snapshot.activity = Some(ActivitySample {
            state: AgentActivity::Busy,
            quality: SampleQuality::Observed,
            turn: None,
        });
        snapshot.health.state = ReporterHealth::Unavailable;
        let status = SessionStatus::of(&waiting, 0);
        assert_eq!(status.activity, " unavailable");
        assert_eq!(status.unread.as_deref(), Some("Unread Unavailable"));
    }

    #[test]
    fn elapsed_rolls_up_from_minutes_to_days() {
        for (now, expected) in [
            (0, "0m"),
            (59_000, "0m"),
            (60_000, "1m"),
            (3_600_000, "1h00"),
            (3_660_000, "1h01"),
            (86_400_000, "1d 0h"),
            (90_000_000, "1d 1h"),
        ] {
            let session = session(SessionPhase::Running, None);
            assert_eq!(SessionStatus::of(&session, now).elapsed, expected, "{now}");
        }
    }

    #[test]
    fn unavailable_status_names_the_published_reason_without_claiming_other_capabilities() {
        let mut snapshot = agent(
            AgentActivity::Busy,
            SampleQuality::Observed,
            ReporterHealth::Unavailable,
            Vec::new(),
        );
        snapshot.health.reason = Some("supervisor_disconnected".into());
        let clause = activity_clause(&session(SessionPhase::Running, Some(snapshot)));
        assert!(
            clause.contains("unavailable · supervisor_disconnected"),
            "{clause}"
        );
        assert!(!clause.contains("Ready"), "{clause}");
        assert!(!clause.contains("Input"), "{clause}");
    }

    #[test]
    fn the_spinner_frame_advances_with_the_clock() {
        let session = session(
            SessionPhase::Running,
            Some(agent(
                AgentActivity::Busy,
                SampleQuality::Observed,
                ReporterHealth::Connected,
                Vec::new(),
            )),
        );
        for (now, frame) in [(0, '⠋'), (100, '⠙'), (900, '⠏'), (1000, '⠋')] {
            assert_eq!(SessionStatus::of(&session, now).glyph, frame, "{now}");
        }
    }
}
