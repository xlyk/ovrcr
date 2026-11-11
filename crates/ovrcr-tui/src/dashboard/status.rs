//! One status value for a session: the glyph the sidebar row draws, the plain-words state,
//! reporting and elapsed clauses both pane headers lay out, and the activity, Unread and
//! health clauses the Agents picker shows. Which clause a session gets, and its wording,
//! live here once, so the sidebar, the two pane headers and the picker cannot disagree
//! about the same session; each surface arranges and clips the clauses for its own width.
//! The value carries the glyph's colour, because the colour is part of what the status
//! says; text styles, widths and clipping stay with the renderer. Readiness itself is still
//! decided in [`super::ready`].
use super::render::{GREEN, MUTED, RED, SUBTEXT, TEAL, YELLOW};
use ovrcr_protocol::{
    AgentActivity, AgentSnapshot, InputKind, ReporterHealth, SampleQuality, SessionKind,
    SessionPhase, SessionSummary,
};
use ratatui::style::Color;
use std::time::Duration;

const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const SPINNER_FRAME_MS: u64 = 100;
/// How long one spinner frame stands: the redraw cadence the Dashboard owes a busy session.
pub(super) const SPINNER_INTERVAL: Duration = Duration::from_millis(SPINNER_FRAME_MS);
/// How long after its start an agent that has reported nothing still reads `starting…`.
/// Managed agents bind within seconds of launch; one that never binds by then has no
/// reporting (hooks not set up, or a provider without it) and reads `status off`.
const STARTING_GRACE_MS: u64 = 60_000;

/// The glyph and colour a session holds still at, or `None` when its glyph is a moving
/// spinner frame — the one state that needs the clock. [`busy`] and [`SessionStatus::of`]
/// both read this, so the redraw cadence and the drawn glyph are a single decision.
pub(super) fn sidebar_activity_glyph(
    activity: AgentActivity,
    unread: bool,
    now_unix_ms: u64,
) -> (char, Color) {
    match activity {
        AgentActivity::Unknown => ('-', MUTED()),
        AgentActivity::Idle => (' ', MUTED()),
        AgentActivity::WaitingInput => ('?', YELLOW()),
        AgentActivity::Error => ('!', RED()),
        AgentActivity::ResponseReady if unread => ('✓', TEAL()),
        AgentActivity::ResponseReady => (' ', MUTED()),
        AgentActivity::Busy => (
            SPINNER_FRAMES
                [((now_unix_ms / SPINNER_FRAME_MS) % SPINNER_FRAMES.len() as u64) as usize],
            GREEN(),
        ),
    }
}
fn fixed_glyph(session: &SessionSummary) -> Option<(char, Color)> {
    Some(match session.phase {
        SessionPhase::Exited { .. } | SessionPhase::Stopped => ('·', MUTED()),
        SessionPhase::Interrupted => ('!', YELLOW()),
        SessionPhase::Paused => ('P', SUBTEXT()),
        SessionPhase::Running if super::ready::activity(session) == AgentActivity::Busy => {
            return None;
        }
        _ => sidebar_activity_glyph(super::ready::activity(session), session.unread.is_some(), 0),
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
    /// The managed process id while it runs; `None` once it has exited or when unknown.
    pub pid: Option<u32>,
    pub elapsed: String,
    /// The headers' state in plain words — `Working`, `Ready · unread reply`, `Waiting for
    /// input`, `Paused`, `Exited` — then any recovery or control explanation. Empty when
    /// nothing is known, as for a plain shell. Quality words and internal reasons stay out.
    pub state: String,
    /// `state` for a header too narrow to hold it: a bare `Unread` when there is one, since
    /// the sidebar glyph still shows the activity and the explanations need the full width;
    /// otherwise the same as `state`.
    pub narrow_state: String,
    /// The headers' reporting health, only when it matters: `starting…` before an agent's
    /// first report, `status off` when its reporting is off or lost.
    pub reporting: Option<&'static str>,
    /// An Input request is open, even when the session cannot currently answer it.
    pub has_open_input_request: bool,
    /// The Agents picker's activity clause, leading space included: ` busy observed`,
    /// ` input needed · confirm`, ` unavailable`, or ` idle` without an agent.
    pub activity: String,
    /// Reporting loss also remains textual when an Input request overrides activity.
    pub reporting_health: Option<String>,
    /// Essential Agent state for a narrow picker row, without explanatory detail.
    pub compact: String,
    /// The Agents picker's Unread clause, which replaces its activity when present.
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
        let (state, narrow_state) = header_states(session);
        Self {
            glyph,
            color: if session.agent.as_ref().is_some_and(unavailable) {
                MUTED()
            } else {
                color
            },
            exited,
            pid: session.pid.filter(|_| !exited),
            elapsed: session
                .started_unix_ms
                .map(|started| format_elapsed_at(started, now_unix_ms))
                .unwrap_or_default(),
            state,
            narrow_state,
            reporting: reporting_words(session, now_unix_ms),
            has_open_input_request: session
                .agent
                .as_ref()
                .is_some_and(|agent| !agent.input_requests.is_empty()),
            activity: {
                let mut clause = activity_clause(session);
                if let Some(control) = super::assignment_control::control_clause(
                    session
                        .assignment_control
                        .as_ref()
                        .map(|view| &view.control),
                ) {
                    clause.push_str(" · ");
                    clause.push_str(control);
                }
                clause
            },
            compact: {
                let mut clause = compact_agent_clause(session);
                if let Some(control) = super::assignment_control::control_clause(
                    session
                        .assignment_control
                        .as_ref()
                        .map(|view| &view.control),
                ) {
                    clause.push_str(" · ");
                    clause.push_str(control);
                }
                clause
            },
            reporting_health: session
                .agent
                .as_ref()
                .filter(|agent| unavailable(agent))
                .map(unavailable_clause),
            unread: session.unread.as_ref().map(|_| {
                let unread = unread_clause(session);
                match super::assignment_control::control_clause(
                    session
                        .assignment_control
                        .as_ref()
                        .map(|view| &view.control),
                ) {
                    Some(control) => format!("{control} · {unread}"),
                    None => unread,
                }
            }),
        }
    }

    /// The shared sidebar attention classification used by the strip, its jump, and folded
    /// counts.
    pub(super) fn attention(session: &SessionSummary) -> Option<super::attention::Attention> {
        let activity = super::ready::activity(session);
        if session.phase == SessionPhase::Running && activity == AgentActivity::WaitingInput {
            Some(super::attention::Attention::Input)
        } else {
            (session.unread.is_some() && activity != AgentActivity::Busy)
                .then_some(super::attention::Attention::Ready)
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

/// The headers' state words: the process phase, or a running session's effective activity,
/// then the Unread, recovery and control clauses, joined with ` · `; and the narrow form,
/// a bare `Unread` when there is one.
fn header_states(session: &SessionSummary) -> (String, String) {
    let base = match session.phase {
        SessionPhase::Running => activity_words(session),
        SessionPhase::Paused => "Paused",
        SessionPhase::Exited { .. } => "Exited",
        SessionPhase::Stopped => "Stopped",
        SessionPhase::Interrupted => "Interrupted",
    };
    let mut clauses = recovery_parts(session);
    if let Some(control) = super::assignment_control::control_clause(
        session
            .assignment_control
            .as_ref()
            .map(|view| &view.control),
    ) {
        clauses.push(control.into());
    }
    // A clause that leads because nothing came before it still starts a sentence.
    let sentence = |lead: &[&str]| {
        let mut parts: Vec<&str> = lead
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect();
        parts.extend(clauses.iter().map(String::as_str));
        let joined = parts.join(" · ");
        let mut chars = joined.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect::<String>()
        })
    };
    if session.unread.is_some() {
        (sentence(&[base, "unread reply"]), "Unread".into())
    } else {
        let state = sentence(&[base]);
        (state.clone(), state)
    }
}

/// A running session's effective activity in words; nothing for Unknown, which is what a
/// plain shell, or an agent before its first report, says.
fn activity_words(session: &SessionSummary) -> &'static str {
    match super::ready::activity(session) {
        AgentActivity::Busy => "Working",
        AgentActivity::WaitingInput
            if session
                .agent
                .as_ref()
                .and_then(|agent| agent.input_requests.first())
                .is_some_and(|request| request.kind == InputKind::Approval) =>
        {
            "Waiting for approval"
        }
        AgentActivity::WaitingInput => "Waiting for input",
        AgentActivity::ResponseReady => "Ready",
        AgentActivity::Idle => "Idle",
        AgentActivity::Error => "Error",
        AgentActivity::Unknown => "",
    }
}

/// The reasons `Reporting::lost` writes when the Server loses a session's reporter. Unlike a
/// provider's own Unavailable reasons before its first report, these are never a startup.
const SERVER_LOSSES: [&str; 4] = [
    "supervisor_disconnected",
    "capability_revoked",
    "unfinalized_release",
    "pty_exited_before_finalization",
];

/// Reporting health in words, only for a running agent and only when it matters. Before the
/// first report a young session is `starting…`, whatever the provider says meanwhile (Claude
/// reads `source_unavailable` until its transcript exists), but a Server-side loss
/// ([`SERVER_LOSSES`]) reads `status off` at once. Past the grace an agent that never bound
/// has no reporting at all; one that bound but has not been asked anything says nothing.
/// After the first report, a lost reporter is `status off`, and so is an Unread with no
/// reporter left behind it.
fn reporting_words(session: &SessionSummary, now_unix_ms: u64) -> Option<&'static str> {
    if session.phase != SessionPhase::Running {
        return None;
    }
    if session.unread.is_some() && session.agent.as_ref().is_none_or(unavailable) {
        return Some("status off");
    }
    if !matches!(session.kind, SessionKind::Agent { .. }) && session.agent.is_none() {
        return None;
    }
    let reported = session
        .agent
        .as_ref()
        .map_or(session.activity != AgentActivity::Unknown, |agent| {
            agent.activity.is_some() || !agent.input_requests.is_empty()
        });
    if reported {
        return session
            .agent
            .as_ref()
            .filter(|agent| unavailable(agent))
            .map(|_| "status off");
    }
    if session.agent.as_ref().is_some_and(|agent| {
        unavailable(agent)
            && agent
                .health
                .reason
                .as_deref()
                .is_some_and(|reason| SERVER_LOSSES.contains(&reason))
    }) {
        return Some("status off");
    }
    if session
        .started_unix_ms
        .is_some_and(|started| now_unix_ms.saturating_sub(started) < STARTING_GRACE_MS)
    {
        Some("starting…")
    } else if session.agent.is_none() {
        Some("status off")
    } else {
        None
    }
}

/// Recovery explanations, which stay actionable after the managed process exits.
fn recovery_parts(session: &SessionSummary) -> Vec<String> {
    let Some(recovery) = &session.recovery else {
        return Vec::new();
    };
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
    parts
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
    let mut parts = recovery_parts(session);
    if !parts.is_empty() {
        if session.phase.is_live()
            && let Some(agent) = &session.agent
        {
            parts.insert(0, provider_clause(session, agent).trim_start().into());
        }
        return format!(" {}", parts.join(" · "));
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
        assert!(recovery_parts(&row).is_empty());
        assert_eq!(SessionStatus::of(&row, 0).state, "Exited");
        // A narrow header keeps only a bare Unread: explanations need the full width.
        row.recovery.as_mut().unwrap().unavailable = Some("No native history".into());
        row.unread = Some(ReadyObservation {
            binding: agent(
                AgentActivity::ResponseReady,
                SampleQuality::Observed,
                ReporterHealth::Connected,
                Vec::new(),
            )
            .binding,
            turn: Some("t".into()),
            activity_revision: 1,
        });
        let status = SessionStatus::of(&row, 0);
        assert_eq!(status.state, "Exited · unread reply · No native history");
        assert_eq!(status.narrow_state, "Unread");
    }

    fn session(phase: SessionPhase, agent: Option<AgentSnapshot>) -> SessionSummary {
        SessionSummary {
            scope: ovrcr_protocol::SessionScope::Workspace,
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
            assignment_control: None,
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
            native_control: None,
            native_control_revision: 0,
        }
    }

    /// Phase x effective activity x reporter health, at the glyph, the picker's activity
    /// clause and the headers' state and reporting words.
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
        // Columns: phase, agent snapshot, then the expected glyph, glyph colour, picker
        // activity clause, header state, header reporting words and pid. `exited` is checked
        // against the phase and `busy` against the glyph, so neither needs a column of its
        // own; the Unread clauses have their own test below.
        let cases = [
            // A plain shell says nothing about activity it was never told.
            (
                running.clone(),
                None,
                '-',
                MUTED(),
                " unknown",
                "",
                None,
                Some(42),
            ),
            (
                running.clone(),
                Some(agent(Unknown, Observed, Connected, vec![])),
                '-',
                MUTED(),
                " unknown observed",
                "",
                None,
                Some(42),
            ),
            (
                running.clone(),
                Some(agent(Idle, Confirmed, Connected, vec![])),
                ' ',
                MUTED(),
                " idle confirmed",
                "Idle",
                None,
                Some(42),
            ),
            (
                running.clone(),
                Some(agent(Busy, Observed, Connected, vec![])),
                '⠋',
                GREEN(),
                " busy observed",
                "Working",
                None,
                Some(42),
            ),
            (
                running.clone(),
                Some(agent(WaitingInput, Estimated, Connected, vec![])),
                '?',
                YELLOW(),
                " waiting estimated",
                "Waiting for input",
                None,
                Some(42),
            ),
            (
                running.clone(),
                Some(agent(Error, Observed, Connected, vec![])),
                '!',
                RED(),
                " error observed",
                "Error",
                None,
                Some(42),
            ),
            (
                running.clone(),
                Some(agent(ResponseReady, Confirmed, Connected, vec![])),
                ' ',
                MUTED(),
                " response ready · confirmed",
                "Ready",
                None,
                Some(42),
            ),
            // An unavailable reporter mutes the glyph; the picker names it, the headers keep
            // the state and say `status off` instead of the reason.
            (
                running.clone(),
                Some(agent(Busy, Observed, Unavailable, vec![])),
                '⠋',
                MUTED(),
                " unavailable",
                "Working",
                Some("status off"),
                Some(42),
            ),
            (
                running.clone(),
                Some(agent(ResponseReady, Observed, Unavailable, vec![])),
                ' ',
                MUTED(),
                " unavailable response ready · observed",
                "Ready",
                Some("status off"),
                Some(42),
            ),
            // An open Input request outranks the activity underneath it, glyph included.
            (
                running.clone(),
                Some(agent(Busy, Observed, Connected, vec![InputKind::Confirm])),
                '?',
                YELLOW(),
                " input needed · confirm",
                "Waiting for input",
                None,
                Some(42),
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
                "Waiting for approval",
                None,
                Some(42),
            ),
            // Phase outranks every activity, and only a running agent reports its health.
            (
                SessionPhase::Paused,
                Some(agent(Busy, Observed, Connected, vec![])),
                'P',
                SUBTEXT(),
                " busy observed",
                "Paused",
                None,
                Some(42),
            ),
            (
                SessionPhase::Paused,
                Some(agent(Busy, Observed, Unavailable, vec![])),
                'P',
                MUTED(),
                " unavailable",
                "Paused",
                None,
                Some(42),
            ),
            (
                exited.clone(),
                Some(agent(Busy, Observed, Connected, vec![])),
                '·',
                MUTED(),
                " busy observed",
                "Exited",
                None,
                None,
            ),
            (
                exited,
                Some(agent(ResponseReady, Observed, Connected, vec![])),
                '·',
                MUTED(),
                " response ready · observed",
                "Exited",
                None,
                None,
            ),
        ];
        for (phase, snapshot, glyph, color, activity, state, reporting, pid) in cases {
            let session = session(phase.clone(), snapshot);
            let status = SessionStatus::of(&session, 0);
            let case = format!("{phase:?} {activity}");
            assert_eq!(status.glyph, glyph, "{case}");
            assert_eq!(status.color, color, "{case}");
            assert_eq!(status.activity, activity, "{case}");
            assert_eq!(status.state, state, "{case}");
            assert_eq!(status.narrow_state, status.state, "{case}");
            assert_eq!(status.reporting, reporting, "{case}");
            assert_eq!(status.pid, pid, "{case}");
            assert_eq!(
                status.exited,
                matches!(phase, SessionPhase::Exited { .. }),
                "{case}"
            );
            assert_eq!(status.unread, None, "{case}");
            // The headers never carry quality words or internal reasons.
            for internal in [
                "observed",
                "confirmed",
                "estimated",
                "unavailable",
                "unknown",
            ] {
                assert!(!status.state.contains(internal), "{case}: {}", status.state);
            }
            // The redraw cadence and the glyph read the same activity.
            assert_eq!(
                busy(&session),
                SPINNER_FRAMES.contains(&status.glyph),
                "{case}"
            );
        }
    }

    /// Before an agent's first report the headers say `starting…`; once the grace has passed,
    /// one that never bound has no reporting at all, and one that bound but has not been asked
    /// anything says nothing. Terminals never get reporting words.
    #[test]
    fn reporting_words_start_then_settle_without_false_alarms() {
        let grace = STARTING_GRACE_MS;
        let mut launched = session(SessionPhase::Running, None);
        launched.kind = SessionKind::Agent {
            name: "claude".into(),
        };
        let words = |session: &SessionSummary, now| SessionStatus::of(session, now).reporting;
        assert_eq!(words(&launched, 0), Some("starting…"));
        assert_eq!(words(&launched, grace - 1), Some("starting…"));
        assert_eq!(words(&launched, grace), Some("status off"), "never bound");

        // Bound, but Claude has no transcript yet: Unavailable is not an alarm before the
        // first report, and past the grace an unasked agent says nothing.
        let mut bound = agent(
            AgentActivity::Unknown,
            SampleQuality::Observed,
            ReporterHealth::Unavailable,
            Vec::new(),
        );
        bound.activity = None;
        bound.health.reason = Some("source_unavailable".into());
        launched.agent = Some(bound);
        assert_eq!(words(&launched, 0), Some("starting…"));
        assert_eq!(words(&launched, grace), None);
        let status = SessionStatus::of(&launched, 0);
        assert_eq!(status.state, "");
        assert!(!status.state.contains("source_unavailable"));

        // A real loss before the first report is not a startup: the supervisor is gone.
        launched.agent.as_mut().unwrap().health.reason = Some("supervisor_disconnected".into());
        assert_eq!(words(&launched, 0), Some("status off"));
        assert_eq!(words(&launched, grace), Some("status off"));
        for reason in SERVER_LOSSES {
            launched.agent.as_mut().unwrap().health.reason = Some(reason.into());
            assert_eq!(words(&launched, 0), Some("status off"), "{reason}");
        }
        // A provider's own reason before its first report is still a startup.
        launched.agent.as_mut().unwrap().health.reason = Some("transcript_pending".into());
        assert_eq!(words(&launched, 0), Some("starting…"));
        launched.agent.as_mut().unwrap().health.reason = Some("source_unavailable".into());

        // The first report ends `starting…`; a reporter lost after it is `status off`.
        launched.agent.as_mut().unwrap().activity = Some(ActivitySample {
            state: AgentActivity::Busy,
            quality: SampleQuality::Observed,
            turn: Some("t".into()),
        });
        assert_eq!(words(&launched, 0), Some("status off"));
        launched.agent.as_mut().unwrap().health.state = ReporterHealth::Connected;
        assert_eq!(words(&launched, 0), None);
        assert_eq!(SessionStatus::of(&launched, 0).state, "Working");

        // A legacy hook's rollup is a report too.
        let mut rollup = session(SessionPhase::Running, None);
        rollup.kind = SessionKind::Agent {
            name: "claude".into(),
        };
        rollup.activity = AgentActivity::Busy;
        assert_eq!(words(&rollup, grace), None);
        assert_eq!(SessionStatus::of(&rollup, grace).state, "Working");

        // A terminal is not an agent: no `starting…`, no `status off`, no `Unknown`.
        let shell = session(SessionPhase::Running, None);
        assert_eq!(words(&shell, 0), None);
        assert_eq!(words(&shell, grace * 10), None);
        assert_eq!(SessionStatus::of(&shell, 0).state, "");

        // A start the clock cannot place never claims `starting…`.
        launched.agent = None;
        launched.started_unix_ms = None;
        assert_eq!(words(&launched, 0), Some("status off"));
        // Only a running agent reports; a paused or exited one says its phase.
        launched.started_unix_ms = Some(0);
        launched.phase = SessionPhase::Paused;
        assert_eq!(words(&launched, 0), None);
    }

    #[test]
    fn header_state_names_recovery_and_control_after_the_phase() {
        let mut row = session(SessionPhase::Interrupted, None);
        row.recovery = Some(ovrcr_protocol::SessionRecovery {
            conversation: Some("c".into()),
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: Some("resume failed".into()),
        });
        assert_eq!(
            SessionStatus::of(&row, 0).state,
            "Interrupted · resume failed · confirm previous processes stopped"
        );
        // A clause that leads still starts with a capital.
        row.phase = SessionPhase::Running;
        row.recovery.as_mut().unwrap().requires_ack = false;
        row.recovery.as_mut().unwrap().failure = None;
        assert_eq!(
            SessionStatus::of(&row, 0).state,
            "Agent launched; awaiting conversation attachment"
        );
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
        // The headers keep the state and add the Unread as a clause.
        assert_eq!(SessionStatus::of(&ready, 0).state, "Ready · unread reply");
        assert_eq!(SessionStatus::of(&ready, 0).reporting, None);
        ready.agent.as_mut().unwrap().health.state = ReporterHealth::Unavailable;
        assert_eq!(
            SessionStatus::of(&ready, 0).unread.as_deref(),
            Some("Unread Unavailable response ready · observed")
        );
        assert_eq!(SessionStatus::of(&ready, 0).state, "Ready · unread reply");
        assert_eq!(SessionStatus::of(&ready, 0).reporting, Some("status off"));
        // Without a reporting provider there is no clause to add, only the health word.
        ready.agent = None;
        assert_eq!(
            SessionStatus::of(&ready, 0).unread.as_deref(),
            Some("Unread Unavailable")
        );
        assert_eq!(SessionStatus::of(&ready, 0).state, "Unread reply");
        assert_eq!(SessionStatus::of(&ready, 0).reporting, Some("status off"));
        // A narrow header keeps the Unread ahead of the activity word.
        ready.activity = AgentActivity::Busy;
        let status = SessionStatus::of(&ready, 0);
        assert_eq!(status.state, "Working · unread reply");
        assert_eq!(status.narrow_state, "Unread");
        ready.activity = AgentActivity::Unknown;

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
        assert_eq!(status.state, "Waiting for input · unread reply");

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
