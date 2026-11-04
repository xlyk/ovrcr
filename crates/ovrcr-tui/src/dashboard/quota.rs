use super::{
    Dashboard, DashboardAction, InputMode,
    render::{MUTED, RED, TEXT, YELLOW, label_color, sidebar_area},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use ovrcr_protocol::{
    ClientMessage, ProviderQuota, QuotaSnapshot, QuotaSource, QuotaState, QuotaWindow, Request,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

/// A popup over the dashboard. Settings is the editable one; see `settings_editor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Details {
    Quota,
    Settings,
    Events,
}

impl Dashboard {
    pub(super) fn open_quota_details(&mut self) -> DashboardAction {
        self.open_details(Details::Quota)
    }

    pub(super) fn open_details(&mut self, details: Details) -> DashboardAction {
        self.cancel_mouse_gesture();
        self.mode = InputMode::Browse;
        self.palette = None;
        self.whichkey = None;
        self.details = Some((details, 0));
        self.settings_editor = Default::default();
        DashboardAction::Redraw
    }

    pub(super) fn details_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        if self
            .details
            .is_some_and(|(details, _)| details == Details::Settings)
        {
            return self.settings_editor_key(key);
        }
        if self
            .details
            .is_some_and(|(details, _)| details == Details::Events)
        {
            return self.events_key(key);
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) || super::input::is_browse_key(key) {
            self.details = None;
        } else if let Some((_, scroll)) = &mut self.details {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                KeyCode::Home => *scroll = 0,
                _ => {}
            }
        }
        DashboardAction::Redraw
    }

    pub(super) fn details_mouse(&mut self, mouse: MouseEvent) -> DashboardAction {
        if self
            .details
            .is_some_and(|(details, _)| details == Details::Settings)
        {
            let code = match mouse.kind {
                MouseEventKind::ScrollDown => KeyCode::Down,
                MouseEventKind::ScrollUp => KeyCode::Up,
                _ => return DashboardAction::None,
            };
            return self.settings_editor_key(KeyEvent::from(code));
        }
        if self
            .details
            .is_some_and(|(details, _)| details == Details::Events)
        {
            return self.events_mouse(mouse);
        }
        if let Some((_, scroll)) = &mut self.details {
            match mouse.kind {
                MouseEventKind::ScrollDown => *scroll = scroll.saturating_add(1),
                MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(1),
                _ => {}
            }
        }
        DashboardAction::Redraw
    }

    pub(super) fn draw_details(&self, frame: &mut Frame<'_>, now: u64) {
        let Some((details, scroll)) = self.details else {
            return;
        };
        if details == Details::Settings {
            return self.draw_settings_editor(frame, now);
        }
        if details == Details::Events {
            return self.draw_events(frame);
        }
        let outer = frame.area();
        let width = outer.width.saturating_sub(4).min(82);
        let height = outer.height.saturating_sub(2).min(30);
        let area = Rect::new(
            outer.x + (outer.width - width) / 2,
            outer.y + (outer.height - height) / 2,
            width,
            height,
        );
        let (title, lines) = match details {
            Details::Quota => (
                "Quota details · Esc close · ↑↓ scroll",
                self.quota_lines(now),
            ),
            Details::Settings => unreachable!("drawn by settings_editor"),
            Details::Events => unreachable!("drawn by events"),
        };
        let block = Block::bordered().title(title).style(
            Style::default()
                .bg(super::render::BASE())
                .fg(super::render::TEXT()),
        );
        let inner = block.inner(area);
        let paragraph = Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false });
        let max_scroll = paragraph
            .line_count(inner.width)
            .saturating_sub(usize::from(inner.height));
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            paragraph.scroll((scroll.min(max_scroll.min(u16::MAX as usize) as u16), 0)),
            inner,
        );
    }

    fn quota_lines(&self, now: u64) -> Vec<String> {
        let quota = self.quotas.clone().unwrap_or_default();
        let mut lines = vec!["Subscription allowance only.".into(), String::new()];
        let seen = |stamp| match ovrcr_protocol::freshness::age_ms(stamp, now) {
            Some(age) => format!("{} ({} ago)", local_time(stamp), span(age / 60_000)),
            None => format!("{} (unverifiable age)", local_time(stamp)),
        };
        for provider in [&quota.claude, &quota.codex, &quota.grok, &quota.cursor] {
            lines.push(format!(
                "{} — {}",
                provider.provider.name(),
                if provider.state == QuotaState::Current && !provider.windows.is_empty() {
                    "reported"
                } else {
                    provider.state.label()
                }
            ));
            match (&provider.reason, provider.state) {
                (Some(sentence), QuotaState::Disabled) => lines.push(sentence.clone()),
                (Some(reason), _) => lines.push(format!("reason: {reason}")),
                (None, _) => {}
            }
            lines.push(match &provider.source {
                Some(QuotaSource::Session {
                    session,
                    run,
                    binding,
                }) => format!(
                    "source: session #{} run #{}; Reporting generation {}",
                    session.0, run.0, binding.generation
                ),
                Some(QuotaSource::NativeProfile {
                    profile,
                    generation,
                }) => format!("source: {profile}; generation {generation}"),
                Some(QuotaSource::Probe { probed_unix_ms }) => {
                    format!("source: probe; last probe {}", seen(*probed_unix_ms))
                }
                None => "source: no native source selected".into(),
            });
            lines.push(format!(
                "last observation: {}",
                provider
                    .observed_unix_ms
                    .map(seen)
                    .unwrap_or_else(|| "not reported".into())
            ));
            lines.push(format!(
                "last account check: {}",
                provider.checked_unix_ms.map(seen).unwrap_or_else(|| {
                    "not reported (native callback is not a backend check)".into()
                })
            ));
            lines.push(format!(
                "next check: {}",
                provider
                    .next_check_unix_ms
                    .map(|stamp| format!("{} ({})", local_time(stamp), until(stamp, now)))
                    .unwrap_or_else(|| "not scheduled".into())
            ));
            for window in &provider.windows {
                let reset = window
                    .resets_unix_ms
                    .and_then(|stamp| i64::try_from(stamp).ok())
                    .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
                    .map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                    .unwrap_or_else(|| "not reported or unverifiable".into());
                lines.push(format!(
                    "{} [{}]: {} · reset: {reset}",
                    window.label,
                    window.id,
                    window_text(provider, window, now)
                ));
                if provider.provider == crate::protocol::QuotaProvider::Cursor {
                    let used = window
                        .used_basis_points
                        .map(|points| format!("{:.2}%", f64::from(points) / 100.0))
                        .unwrap_or_else(|| {
                            if window.over_limit {
                                "over limit".into()
                            } else {
                                "—".into()
                            }
                        });
                    let remaining = window
                        .remaining_basis_points()
                        .map(|points| format!("{:.2}%", f64::from(points) / 100.0))
                        .unwrap_or_else(|| "—".into());
                    lines.push(format!("  used: {used} · remaining: {remaining}"));
                }
            }
            lines.push(String::new());
        }
        lines
    }

    /// Sends a quota request the user asked for from the palette; the Server
    /// validates and republishes, so nothing is applied here.
    pub(super) fn quota_request(&mut self, request: Request) -> DashboardAction {
        self.palette = None;
        let request_id = self.error_owning_request_id();
        self.enqueue_request(ClientMessage {
            request_id,
            request,
        });
        DashboardAction::Redraw
    }

    /// Rendering and every tree interaction share these rectangles: Operator and the tree
    /// from the top, the attention strip and the quota line at the bottom. The Operator
    /// heading and three tree lines come first; the strip, then the quota line, take a line
    /// each only while that reservation still fits. Neither depends on the clock.
    pub(super) fn sidebar_rects(&self, area: Rect) -> super::SidebarRects {
        const RESERVED: u16 = 4;
        let sidebar = sidebar_area(area, self.sidebar_preference());
        let attention = u16::from(
            sidebar.width > 0 && sidebar.height > RESERVED && self.attention_counts() != (0, 0),
        );
        let quota = u16::from(
            sidebar.width >= 8
                && sidebar.height > RESERVED + attention
                && self
                    .quotas
                    .as_ref()
                    .is_some_and(|quota| listed(quota).next().is_some()),
        );
        let available = sidebar.height - attention - quota;
        let heading = u16::from(available > 0);
        let rows = if self.operator.collapsed {
            0
        } else {
            self.operator_ids().len().min(5) as u16
        };
        let operator_height = heading + rows.min(available.saturating_sub(heading + 3));
        // The strip sits at the bottom, above the quota line: when it comes or goes, only
        // the tree's last line does, and no row moves under the pointer.
        super::SidebarRects {
            attention: Rect::new(
                sidebar.x,
                sidebar.bottom() - quota - attention,
                sidebar.width,
                attention,
            ),
            operator: Rect::new(sidebar.x, sidebar.y, sidebar.width, operator_height),
            tree: Rect::new(
                sidebar.x,
                sidebar.y + operator_height,
                sidebar.width,
                available - operator_height,
            ),
            quota: Rect::new(sidebar.x, sidebar.bottom() - quota, sidebar.width, quota),
        }
    }

    pub(super) fn draw_quota(&self, frame: &mut Frame<'_>, now: u64) {
        let area = self.sidebar_rects(frame.area()).quota;
        let Some(quota) = &self.quotas else { return };
        if area.is_empty() {
            return;
        }
        frame.render_widget(
            Paragraph::new(quota_line(quota, usize::from(area.width), now)),
            area,
        );
    }
}

fn provider_color(provider: &ProviderQuota) -> Color {
    label_color(provider.provider.name())
}

fn name_style(provider: &ProviderQuota) -> Style {
    Style::default()
        .fg(provider_color(provider))
        .add_modifier(Modifier::BOLD)
}

/// The providers the quota line names: every one not switched off, in snapshot order.
fn listed(quota: &QuotaSnapshot) -> impl Iterator<Item = &ProviderQuota> {
    [&quota.claude, &quota.codex, &quota.grok, &quota.cursor]
        .into_iter()
        .filter(|provider| provider.state != QuotaState::Disabled)
}

/// One provider on the quota line, and whether it has a value: its name and the least
/// allowance left in any general window that has not passed its reset, threshold-coloured on
/// the number (WCAG 1.4.1: the number is always printed). A stale value is muted; no value at
/// all is a muted `?`.
fn quota_token(provider: &ProviderQuota, now: u64) -> (bool, Vec<Span<'static>>) {
    let name = provider.provider.name();
    let muted = Style::default().fg(MUTED());
    let left = provider
        .windows
        .iter()
        .filter(|window| window.general && window.resets_unix_ms.is_none_or(|reset| reset > now))
        .filter_map(|window| {
            if window.over_limit {
                Some(0)
            } else {
                window.remaining_basis_points()
            }
        })
        .min();
    let Some(left) = left else {
        return (false, vec![Span::styled(format!("{name} ?"), muted)]);
    };
    let percent = match left {
        0 => "0%".into(),
        1..100 => "<1%".into(),
        left => format!("{}%", (u32::from(left) + 50) / 100),
    };
    if provider.stale(now) {
        return (true, vec![Span::styled(format!("{name} {percent}"), muted)]);
    }
    let color = match (u32::from(left) + 50) / 100 {
        0..=5 => RED(),
        6..=20 => YELLOW(),
        _ => TEXT(),
    };
    (
        true,
        vec![
            Span::styled(name, name_style(provider)),
            Span::styled(format!(" {percent}"), Style::default().fg(color)),
        ],
    )
}

/// The sidebar's quota line, ` Claude 98% · Codex 38% · Grok 95%`. When it does not fit,
/// providers without a value give way first, then the rest from the end, to a muted `…`;
/// the details popup lists them all.
fn quota_line(quota: &QuotaSnapshot, width: usize, now: u64) -> Line<'static> {
    let muted = Style::default().fg(MUTED());
    let tokens: Vec<_> = listed(quota)
        .map(|provider| quota_token(provider, now))
        .collect();
    let cells = |shown: &[bool]| {
        let kept = || tokens.iter().zip(shown).filter(|(_, keep)| **keep);
        1 + kept()
            .flat_map(|((_, spans), _)| spans)
            .map(Span::width)
            .sum::<usize>()
            + 3 * kept().count().saturating_sub(1)
            + if shown.contains(&false) { 2 } else { 0 }
    };
    let mut shown = vec![true; tokens.len()];
    let mut give_way = (0..tokens.len())
        .rev()
        .filter(|&index| !tokens[index].0)
        .chain((0..tokens.len()).rev().filter(|&index| tokens[index].0));
    while cells(&shown) > width
        && let Some(index) = give_way.next()
    {
        shown[index] = false;
    }
    let mut spans = vec![Span::raw(" ")];
    for ((_, token), _) in tokens.iter().zip(&shown).filter(|(_, keep)| **keep) {
        if spans.len() > 1 {
            spans.push(Span::styled(" · ", muted));
        }
        spans.extend(token.iter().cloned());
    }
    if shown.contains(&false) {
        spans.push(Span::styled(
            if spans.len() > 1 { " …" } else { "…" },
            muted,
        ));
    }
    Line::from(spans)
}

pub(super) fn window_text(provider: &ProviderQuota, window: &QuotaWindow, now: u64) -> String {
    if window.resets_unix_ms.is_some_and(|reset| reset <= now) {
        return "— reset due".into();
    }
    let value = if window.over_limit {
        "over limit".into()
    } else {
        match window.remaining_basis_points() {
            Some(0) => "0% exhausted".into(),
            Some(left) if left < 100 => "<1%".into(),
            Some(left) => format!("{}%", (u32::from(left) + 50) / 100),
            None => "—".into(),
        }
    };
    if !provider.stale(now) || provider.windows.is_empty() {
        return value;
    }
    let left = if !window.over_limit && window.remaining_basis_points().is_some_and(|l| l > 0) {
        " left"
    } else {
        ""
    };
    match provider
        .checked_unix_ms
        .max(provider.observed_unix_ms)
        .and_then(|stamp| ovrcr_protocol::freshness::age_ms(stamp, now))
    {
        Some(age) => format!("{value}{left}  stale {}", span(age / 60_000)),
        None => format!("{value}{left}  stale"),
    }
}

/// Whole minutes as the largest unit that keeps them short: 12m, 5h, 3d.
fn span(minutes: u64) -> String {
    match minutes {
        0 => "<1m".into(),
        1..60 => format!("{minutes}m"),
        60..2_880 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1_440),
    }
}

fn until_short(stamp: u64, now: u64) -> String {
    match stamp.saturating_sub(now) {
        0 => "now".into(),
        left => span(left.div_ceil(60_000)),
    }
}

fn until(stamp: u64, now: u64) -> String {
    match stamp.saturating_sub(now) {
        0 => "due now".into(),
        _ => format!("in {}", until_short(stamp, now)),
    }
}

pub(super) fn local_time(stamp: u64) -> String {
    i64::try_from(stamp)
        .ok()
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|stamp| {
            stamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "unverifiable".into())
}

#[cfg(test)]
mod sidebar_line_tests {
    use super::*;
    use crate::protocol::{QuotaProvider, TerminalSize};

    const NOW: u64 = 1_800_000_000_000;

    fn window(used: Option<u16>, over_limit: bool, resets: Option<u64>) -> QuotaWindow {
        QuotaWindow {
            id: "w".into(),
            label: "w".into(),
            general: true,
            used_basis_points: used,
            over_limit,
            resets_unix_ms: resets,
        }
    }

    fn current(provider: QuotaProvider, windows: Vec<QuotaWindow>) -> ProviderQuota {
        ProviderQuota {
            windows,
            observed_unix_ms: Some(NOW),
            checked_unix_ms: Some(NOW),
            ..ProviderQuota::unknown(provider, QuotaState::Current)
        }
    }

    fn snapshot() -> QuotaSnapshot {
        QuotaSnapshot {
            claude: current(
                QuotaProvider::Claude,
                vec![
                    window(Some(200), false, None),
                    window(Some(5_800), false, None),
                ],
            ),
            codex: current(
                QuotaProvider::Codex,
                vec![
                    window(Some(6_200), false, None),
                    QuotaWindow {
                        general: false,
                        ..window(Some(10_000), false, None)
                    },
                ],
            ),
            grok: current(
                QuotaProvider::Grok,
                vec![window(Some(500), false, Some(NOW + 1))],
            ),
            cursor: QuotaSnapshot::default().cursor,
        }
    }

    #[test]
    fn each_provider_shows_its_most_constrained_general_window_and_off_ones_are_omitted() {
        assert_eq!(
            quota_line(&snapshot(), 39, NOW).to_string(),
            " Claude 42% · Codex 38% · Grok 95%"
        );
        let mut all_off = QuotaSnapshot::default();
        all_off.claude.state = QuotaState::Disabled;
        assert_eq!(listed(&all_off).count(), 0);
        assert_eq!(
            listed(&QuotaSnapshot::default())
                .map(|provider| provider.provider)
                .collect::<Vec<_>>(),
            [QuotaProvider::Claude],
            "Codex, Grok and Cursor are off by default"
        );
    }

    #[test]
    fn providers_that_do_not_fit_give_way_from_the_end() {
        let quota = snapshot();
        assert_eq!(
            quota_line(&quota, 30, NOW).to_string(),
            " Claude 42% · Codex 38% …",
            "never a clipped number"
        );
        assert_eq!(quota_line(&quota, 14, NOW).to_string(), " Claude 42% …");
        assert_eq!(quota_line(&quota, 5, NOW).to_string(), " …");
        let mut unknown = QuotaSnapshot {
            cursor: current(
                QuotaProvider::Cursor,
                vec![window(Some(3_000), false, None)],
            ),
            ..QuotaSnapshot::default()
        };
        for provider in [&mut unknown.codex, &mut unknown.grok] {
            provider.state = QuotaState::Unavailable;
        }
        assert_eq!(
            quota_line(&unknown, 39, NOW).to_string(),
            " Claude ? · Codex ? · Cursor 70% …",
            "a provider without a value gives way before a number"
        );
        assert_eq!(quota_line(&unknown, 14, NOW).to_string(), " Cursor 70% …");
    }

    #[test]
    fn numbers_take_threshold_colours_and_stale_or_missing_values_are_muted() {
        let token = |provider: ProviderQuota| quota_token(&provider, NOW).1;
        let value = |used, over| {
            token(current(
                QuotaProvider::Codex,
                vec![window(used, over, None)],
            ))
        };
        for (used, over, text, color) in [
            (Some(200), false, " 98%", TEXT()),
            (Some(8_500), false, " 15%", YELLOW()),
            (Some(9_700), false, " 3%", RED()),
            (Some(9_960), false, " <1%", RED()),
            (Some(10_000), false, " 0%", RED()),
            (None, true, " 0%", RED()),
        ] {
            let spans = value(used, over);
            assert_eq!(spans[0].content, "Codex");
            assert_eq!(spans[0].style.fg, Some(label_color("Codex")));
            assert_eq!(spans[1].content, text);
            assert_eq!(spans[1].style.fg, Some(color), "{text}");
        }
        let mut stale = current(
            QuotaProvider::Claude,
            vec![window(Some(4_200), false, None)],
        );
        stale.checked_unix_ms = Some(NOW - 12 * 60_000);
        stale.observed_unix_ms = Some(NOW - 30 * 60_000);
        let mut retained = stale.clone();
        retained.state = QuotaState::Unavailable;
        for provider in [stale, retained] {
            let spans = token(provider);
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].content, "Claude 58%");
            assert_eq!(spans[0].style.fg, Some(MUTED()));
        }
        let missing = [
            QuotaSnapshot::default().claude,
            current(QuotaProvider::Claude, vec![window(None, false, None)]),
            current(
                QuotaProvider::Claude,
                vec![window(Some(0), false, Some(NOW))],
            ),
            current(
                QuotaProvider::Claude,
                vec![QuotaWindow {
                    general: false,
                    ..window(Some(0), false, None)
                }],
            ),
        ];
        for provider in missing {
            let spans = token(provider);
            assert_eq!(spans.len(), 1);
            assert_eq!(
                spans[0].content, "Claude ?",
                "checking, unknown, reset due, not general"
            );
            assert_eq!(spans[0].style.fg, Some(MUTED()));
        }
    }

    #[test]
    fn a_click_on_the_quota_line_opens_the_details_that_u_opens() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent};
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 30,
            cols: 120,
        });
        let area = Rect::new(0, 0, 120, 30);
        dashboard.install_area(area);
        let line = dashboard.sidebar_rects(area).quota;
        assert_eq!((line.y, line.height), (28, 1));
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: line.x + 2,
            row: line.y,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(dashboard.mouse_action(click, area), DashboardAction::Redraw);
        assert_eq!(dashboard.details, Some((Details::Quota, 0)));
        dashboard.details = None;
        assert_eq!(dashboard.key(KeyCode::Char('u')), DashboardAction::Redraw);
        assert_eq!(dashboard.details, Some((Details::Quota, 0)));
    }

    #[test]
    fn the_strip_then_the_quota_line_yield_to_the_operator_heading_and_three_tree_lines() {
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 30,
            cols: 120,
        });
        // Title and footer rows sit outside the sidebar.
        let rects = |dashboard: &Dashboard, height: u16| {
            let rects = dashboard.sidebar_rects(Rect::new(0, 0, 120, height + 2));
            (
                rects.attention.height,
                rects.quota.height,
                rects.tree.height,
            )
        };
        assert_eq!(rects(&dashboard, 5), (0, 1, 3));
        assert_eq!(rects(&dashboard, 4), (0, 0, 3));
        dashboard.quotas = None;
        assert_eq!(rects(&dashboard, 5), (0, 0, 4), "no snapshot, no line");
        dashboard.quotas = Some(QuotaSnapshot::default());
        let session = crate::protocol::SessionSummary {
            scope: ovrcr_protocol::SessionScope::Workspace,
            archived: false,
            cwd: "/work".into(),
            id: crate::protocol::SessionId(1),
            run: crate::protocol::SessionRunId(1),
            kind: crate::protocol::SessionKind::Terminal,
            recovery: None,
            project: "p".into(),
            workspace: "w".into(),
            name: "s".into(),
            title: None,
            manual_title: None,
            assignment_control: None,
            label: "l".into(),
            pid: Some(1),
            started_unix_ms: Some(0),
            phase: crate::protocol::SessionPhase::Running,
            activity: crate::protocol::AgentActivity::WaitingInput,
            context_usage: None,
            agent_epoch: 1,
            unread: None,
            agent: None,
        };
        dashboard.install_hierarchy(crate::protocol::HierarchySnapshot {
            projects: vec![crate::protocol::ProjectSummary {
                name: "p".into(),
                workspaces: vec![crate::protocol::WorkspaceSummary {
                    project: "p".into(),
                    name: "w".into(),
                    id: "w".into(),
                    root: false,
                    warning: None,
                    path: "/w".into(),
                    sessions: vec![session],
                }],
            }],
        });
        assert_eq!(rects(&dashboard, 6), (1, 1, 3));
        assert_eq!(
            rects(&dashboard, 5),
            (1, 0, 3),
            "the strip outranks the quota line"
        );
        assert_eq!(rects(&dashboard, 4), (0, 0, 3));
        let full = dashboard.sidebar_rects(Rect::new(0, 0, 120, 30));
        assert_eq!(
            full.operator.y, 1,
            "the strip never moves Operator or the tree"
        );
        assert_eq!(full.tree.bottom(), full.attention.y);
        assert_eq!(
            full.attention.bottom(),
            full.quota.y,
            "directly above the quota line"
        );
        assert_eq!(full.quota.bottom(), 29);
        dashboard.toggle_sidebar();
        assert!(
            dashboard
                .sidebar_rects(Rect::new(0, 0, 120, 30))
                .attention
                .is_empty()
        );
    }
}
