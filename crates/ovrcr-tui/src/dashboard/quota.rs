use super::{
    Dashboard, DashboardAction, InputMode,
    render::{RED, SUBTEXT, SURFACE2, TEXT, YELLOW, compose_row, label_color, sidebar_area},
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
                .bg(super::render::BASE)
                .fg(super::render::TEXT),
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
        for provider in [&quota.claude, &quota.codex, &quota.grok] {
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
            }
            lines.push(String::new());
        }
        lines
    }

    /// Sends a quota request the user asked for from the palette; the Server
    /// validates and republishes, so nothing is applied here.
    pub(super) fn quota_request(&mut self, request: Request) -> DashboardAction {
        self.palette = None;
        DashboardAction::Request(ClientMessage {
            request_id: self.error_owning_request_id(),
            request,
        })
    }

    /// Rendering and every tree interaction share this reduced rectangle.
    pub(super) fn sidebar_rects(&self, area: Rect) -> (Rect, Rect) {
        let sidebar = sidebar_area(area, self.sidebar_preference());
        let desired = self.quotas.as_ref().map_or(0, |quota| {
            if sidebar.width < 8 {
                0
            } else {
                let content = sidebar_quota_rows(quota, sidebar.width, 0)
                    .iter()
                    .map(|(_, height)| height)
                    .sum::<u16>();
                // Trailing blank under the last provider row; dropped first when short.
                content.saturating_add(1)
            }
        });
        let content = desired.saturating_sub(1);
        // The ladder keeps three tree lines: full (blank optional), compact, pointer, then nothing.
        let height = if desired == 0 || sidebar.height < 4 {
            0
        } else if sidebar.height >= desired + 3 {
            desired
        } else if content > 0 && sidebar.height >= content + 3 {
            content
        } else if sidebar.height >= 6 {
            3
        } else {
            1
        };
        (
            Rect::new(sidebar.x, sidebar.y, sidebar.width, sidebar.height - height),
            Rect::new(sidebar.x, sidebar.bottom() - height, sidebar.width, height),
        )
    }

    pub(super) fn draw_quota(&self, frame: &mut Frame<'_>, now: u64) {
        let (_, area) = self.sidebar_rects(frame.area());
        let Some(quota) = &self.quotas else { return };
        if area.is_empty() {
            return;
        }
        match area.height {
            1 => {
                return frame.render_widget(
                    Paragraph::new("Quota: u").style(Style::default().fg(TEXT)),
                    area,
                );
            }
            3 => {
                let lines = [&quota.claude, &quota.codex, &quota.grok]
                    .map(|provider| compact_line(provider, now));
                return frame.render_widget(Paragraph::new(Vec::from(lines)), area);
            }
            _ => {}
        }
        let rows = sidebar_quota_rows(quota, area.width, now);
        let content: u16 = rows.iter().map(|(_, height)| height).sum();
        let mut offset = 0u16;
        for (line, height) in rows {
            frame.render_widget(
                Paragraph::new(line).wrap(Wrap { trim: false }),
                Rect::new(area.x, area.y + offset, area.width, height),
            );
            offset = offset.saturating_add(height);
        }
        if area.height > content {
            frame.render_widget(
                Paragraph::new(Line::from("")),
                Rect::new(area.x, area.y + content, area.width, 1),
            );
        }
    }
}

/// Fixed cells around the bar: inset + name + gaps + label + % + right inset.
const QUOTA_FIXED_COLS: usize = 17;
/// Widest reserved stale suffix so every row stays column-aligned.
const STALE_RESERVE: usize = " stale 59m".len();
const FILL_GLYPH: &str = "━";
const TRACK_GLYPH: &str = "─";

/// Bar width N for Option A, or `None` when the sidebar is too narrow for bars.
fn bar_width(sidebar_width: u16, reserve_stale: bool) -> Option<usize> {
    let mut n = usize::from(sidebar_width).saturating_sub(QUOTA_FIXED_COLS);
    if reserve_stale {
        n = n.saturating_sub(STALE_RESERVE);
    }
    (n >= 6).then_some(n)
}

/// Filled cells for a remaining-allowance bar of width `n`.
fn bar_filled(remaining_bp: u16, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let remaining = f64::from(remaining_bp) / 10_000.0;
    let mut filled = (remaining * n as f64).round() as usize;
    if remaining_bp > 0 && filled == 0 {
        filled = 1;
    }
    if remaining_bp < 10_000 {
        filled = filled.min(n.saturating_sub(1));
    }
    filled.min(n)
}

fn any_provider_stale(quota: &QuotaSnapshot, now: u64) -> bool {
    [&quota.claude, &quota.codex, &quota.grok]
        .into_iter()
        .any(|provider| provider.stale(now) && !provider.windows.is_empty())
}

fn provider_color(provider: &ProviderQuota) -> Color {
    label_color(provider.provider.name())
}

fn name_style(provider: &ProviderQuota) -> Style {
    Style::default()
        .fg(provider_color(provider))
        .add_modifier(Modifier::BOLD)
}

fn subtext() -> Style {
    Style::default().fg(SUBTEXT)
}

fn text_style() -> Style {
    Style::default().fg(TEXT)
}

/// Threshold colors on % text only (WCAG 1.4.1 keeps the words).
fn percent_style(window: &QuotaWindow, now: u64) -> Style {
    if window.resets_unix_ms.is_some_and(|reset| reset <= now) {
        return subtext();
    }
    if window.over_limit {
        return Style::default().fg(RED);
    }
    match window.remaining_basis_points() {
        None => subtext(),
        Some(0) => Style::default().fg(RED),
        Some(left) => {
            let pct = (u32::from(left) + 50) / 100;
            if pct <= 5 {
                Style::default().fg(RED)
            } else if pct <= 20 {
                Style::default().fg(YELLOW)
            } else {
                text_style()
            }
        }
    }
}

fn percent_label(window: &QuotaWindow, now: u64) -> String {
    if window.resets_unix_ms.is_some_and(|reset| reset <= now) {
        return "— reset due".into();
    }
    if window.over_limit {
        return "over limit".into();
    }
    match window.remaining_basis_points() {
        Some(0) => "0% exhausted".into(),
        Some(left) if left < 100 => "<1%".into(),
        Some(left) => format!("{}%", (u32::from(left) + 50) / 100),
        None => "—".into(),
    }
}

fn short_percent_column(label: &str) -> Option<String> {
    match label {
        "—" | "<1%" => Some(format!("{label:>4}")),
        s if s.ends_with('%') && !s.contains(' ') && s.len() <= 4 => Some(format!("{s:>4}")),
        _ => None,
    }
}

fn stale_suffix(provider: &ProviderQuota, now: u64) -> Option<String> {
    if !provider.stale(now) || provider.windows.is_empty() {
        return None;
    }
    match provider
        .checked_unix_ms
        .max(provider.observed_unix_ms)
        .and_then(|stamp| ovrcr_protocol::freshness::age_ms(stamp, now))
    {
        Some(age) => Some(format!(" stale {}", span(age / 60_000))),
        None => Some(" stale".into()),
    }
}

fn header_line(width: u16) -> Line<'static> {
    let title = " QUOTA LEFT ";
    let clipped = if title.len() > usize::from(width) {
        let mut out = title.chars().take(usize::from(width)).collect::<String>();
        while out.len() > usize::from(width) {
            out.pop();
        }
        out
    } else {
        title.to_string()
    };
    let rule = if clipped == title { "─" } else { " " };
    compose_row(
        vec![Span::styled(
            clipped,
            Style::default().fg(SUBTEXT).add_modifier(Modifier::BOLD),
        )],
        Span::styled(rule, Style::default().fg(SURFACE2)),
        Vec::new(),
        usize::from(width),
        false,
    )
}

fn bar_spans(
    provider: &ProviderQuota,
    window: &QuotaWindow,
    n: usize,
    now: u64,
) -> Vec<Span<'static>> {
    let color = provider_color(provider);
    let show_bar = !window.over_limit
        && window.resets_unix_ms.is_none_or(|reset| reset > now)
        && window.remaining_basis_points().is_some();
    if !show_bar {
        return Vec::new();
    }
    match window.remaining_basis_points() {
        Some(left) => {
            let filled = bar_filled(left, n);
            vec![
                Span::styled(FILL_GLYPH.repeat(filled), Style::default().fg(color)),
                Span::styled(
                    TRACK_GLYPH.repeat(n.saturating_sub(filled)),
                    Style::default().fg(SURFACE2),
                ),
            ]
        }
        None => Vec::new(),
    }
}

fn unknown_track(n: usize) -> Vec<Span<'static>> {
    vec![Span::styled(
        TRACK_GLYPH.repeat(n),
        Style::default().fg(SURFACE2),
    )]
}

fn window_row(
    provider: &ProviderQuota,
    window: &QuotaWindow,
    name: &str,
    _width: u16,
    n: Option<usize>,
    reserve_stale: bool,
    now: u64,
) -> Line<'static> {
    let label = percent_label(window, now);
    let style = percent_style(window, now);
    let stale = stale_suffix(provider, now);
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(format!("{name:6}"), name_style(provider)),
        Span::raw(" "),
        Span::styled(format!("{:2}", window.label), subtext()),
        Span::raw(" "),
    ];
    if let Some(n) = n {
        if label == "—" {
            spans.extend(unknown_track(n));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(format!("{:>4}", "—"), subtext()));
        } else if let Some(column) = short_percent_column(&label) {
            spans.extend(bar_spans(provider, window, n, now));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(column, style));
        } else {
            // Long words (0% exhausted / over limit / reset due): occupy bar + % columns.
            spans.push(Span::styled(label.clone(), style));
            let used = label.chars().count();
            let pad = n.saturating_add(1).saturating_add(4).saturating_sub(used);
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
        }
        match (&stale, reserve_stale) {
            (Some(suffix), _) => spans.push(Span::styled(suffix.clone(), subtext())),
            (None, true) => spans.push(Span::raw(" ".repeat(STALE_RESERVE))),
            (None, false) => {}
        }
    } else {
        // Narrow: identity + percentage/state, no bars (state may wrap below).
        spans.push(Span::styled(label, style));
        if let Some(suffix) = stale {
            // Keep the historical " left" cue only beside a positive retained % when stale.
            let left =
                if !window.over_limit && window.remaining_basis_points().is_some_and(|l| l > 0) {
                    " left"
                } else {
                    ""
                };
            if !left.is_empty() {
                spans.push(Span::styled(left, style));
            }
            // suffix already includes a leading space (" stale …").
            spans.push(Span::styled(suffix, subtext()));
        }
    }
    Line::from(spans)
}

fn state_row(provider: &ProviderQuota, width: u16, now: u64) -> Vec<(Line<'static>, u16)> {
    let (state, widest) = state_text(provider, now);
    let name = provider.provider.name();
    let gap = separator(provider);
    let widest_line = format!("{name}{gap}{widest}");
    if Line::raw(widest_line).width() > usize::from(width) {
        vec![
            (
                Line::from(Span::styled(name.to_string(), name_style(provider))),
                1,
            ),
            (Line::from(Span::styled(state, subtext())), 1),
        ]
    } else {
        vec![(
            Line::from(vec![
                Span::styled(format!("{name}"), name_style(provider)),
                Span::styled(format!("{gap}{state}"), subtext()),
            ]),
            1,
        )]
    }
}

/// Reserve the same rows for current, stale, and reset-due states, without a clock-driven resize.
fn sidebar_quota_rows(quota: &QuotaSnapshot, width: u16, now: u64) -> Vec<(Line<'static>, u16)> {
    let reserve_stale = any_provider_stale(quota, now);
    let n = bar_width(width, reserve_stale);
    let mut rows = vec![(header_line(width), 1)];
    for provider in [&quota.claude, &quota.codex, &quota.grok] {
        if !provider.windows.iter().any(|window| window.general) {
            rows.extend(state_row(provider, width, now));
        } else {
            for (index, window) in provider
                .windows
                .iter()
                .filter(|window| window.general)
                .take(2)
                .enumerate()
            {
                let name = if index == 0 {
                    provider.provider.name()
                } else {
                    ""
                };
                if n.is_none() {
                    // Narrow ladder: keep identity readable before decorative bars.
                    let prefix = format!("{name:6} {:2}", window.label);
                    let suffix = window_text(provider, window, now);
                    let longest_state = "0% exhausted  stale 59m";
                    let wrapped = |text: &str| {
                        Paragraph::new(text.to_string())
                            .wrap(Wrap { trim: false })
                            .line_count(width) as u16
                    };
                    if Line::raw(format!("{prefix} {longest_state}")).width() > usize::from(width) {
                        rows.push((
                            Line::from(vec![
                                Span::styled(format!("{name:6}"), name_style(provider)),
                                Span::raw(" "),
                                Span::styled(format!("{:2}", window.label), subtext()),
                            ]),
                            wrapped(&prefix),
                        ));
                        rows.push((
                            Line::from(Span::styled(suffix, percent_style(window, now))),
                            wrapped(longest_state),
                        ));
                        continue;
                    }
                }
                rows.push((
                    window_row(provider, window, name, width, n, reserve_stale, now),
                    1,
                ));
            }
        }
    }
    rows
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

/// The words after a provider's name when it has no general window, and the
/// widest form they take, so the clock never changes the rows reserved.
fn state_text(provider: &ProviderQuota, now: u64) -> (String, String) {
    let label = provider.state.label();
    if provider.state == QuotaState::Disabled {
        return ("usage off".into(), "usage off".into());
    }
    match provider
        .next_check_unix_ms
        .filter(|_| failed(provider.state))
    {
        Some(next) => (
            format!("{label} · retry {}", until_short(next, now)),
            format!("{label} · retry 59m"),
        ),
        None => (label.into(), label.into()),
    }
}

fn separator(_provider: &ProviderQuota) -> &'static str {
    " · "
}

fn failed(state: QuotaState) -> bool {
    !matches!(
        state,
        QuotaState::Current | QuotaState::Checking | QuotaState::Disabled | QuotaState::Waiting
    )
}

/// The compact ladder's one line per provider: its first general window, or its state.
fn compact_line(provider: &ProviderQuota, now: u64) -> Line<'static> {
    let name = provider.provider.name();
    match provider.windows.iter().find(|window| window.general) {
        Some(window) => {
            let label = percent_label(window, now);
            let mut spans = vec![
                Span::styled(name.to_string(), name_style(provider)),
                Span::raw(" "),
                Span::styled(window.label.clone(), subtext()),
                Span::raw(" "),
                Span::styled(label, percent_style(window, now)),
            ];
            if let Some(suffix) = stale_suffix(provider, now) {
                let left = if !window.over_limit
                    && window.remaining_basis_points().is_some_and(|l| l > 0)
                {
                    " left"
                } else {
                    ""
                };
                if !left.is_empty() {
                    spans.push(Span::styled(left, percent_style(window, now)));
                }
                spans.push(Span::styled(suffix, subtext()));
            }
            Line::from(spans)
        }
        None => {
            let (state, _) = state_text(provider, now);
            Line::from(vec![
                Span::styled(name.to_string(), name_style(provider)),
                Span::styled(format!("{}{state}", separator(provider)), subtext()),
            ])
        }
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
mod bar_math_tests {
    use super::{STALE_RESERVE, bar_filled, bar_width};

    #[test]
    fn bar_width_subtracts_fixed_columns_and_floors_at_six() {
        assert_eq!(bar_width(22, false), None);
        assert_eq!(bar_width(23, false), Some(6));
        assert_eq!(bar_width(39, false), Some(22));
        assert_eq!(bar_width(39, true), Some(39 - 17 - STALE_RESERVE));
        assert_eq!(bar_width(27, true), None);
    }

    #[test]
    fn bar_fill_rounds_clamps_full_and_keeps_a_sliver() {
        assert_eq!(bar_filled(10_000, 10), 10);
        assert_eq!(bar_filled(9_800, 10), 9); // 98% never looks full
        assert_eq!(bar_filled(1, 10), 1); // positive remaining keeps one cell
        assert_eq!(bar_filled(0, 10), 0);
        assert_eq!(bar_filled(5_000, 10), 5);
        assert_eq!(bar_filled(3_700, 10), 4); // 37% remaining → round(3.7)=4
    }
}
