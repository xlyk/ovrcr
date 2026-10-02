use super::{Dashboard, DashboardAction, InputMode, render::sidebar_area};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use ovrcr_protocol::{ProviderQuota, QuotaSnapshot, QuotaSource, QuotaState, QuotaWindow};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Clear, Paragraph, Wrap},
};

/// A read-only popup over the dashboard. Both share scrolling and closing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Details {
    Quota,
    Settings,
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
        DashboardAction::Redraw
    }

    pub(super) fn details_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
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
            Details::Settings => (
                "Settings · read-only · Esc close · ↑↓ scroll",
                super::settings::report_lines(self.settings_report.as_deref(), now),
            ),
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
        let mut lines = Vec::new();
        let age = |stamp| {
            ovrcr_protocol::freshness::age_ms(stamp, now)
                .map(|age| format!("{}s ago", age / 1_000))
                .unwrap_or_else(|| "unverifiable".into())
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
                None => "source: no native source selected".into(),
            });
            lines.push(format!(
                "last observation: {}",
                provider
                    .observed_unix_ms
                    .map(age)
                    .unwrap_or_else(|| "not reported".into())
            ));
            lines.push(format!(
                "last account check: {}",
                provider.checked_unix_ms.map(age).unwrap_or_else(|| {
                    "not reported (native callback is not a backend check)".into()
                })
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

    /// Rendering and every tree interaction share this reduced rectangle.
    pub(super) fn sidebar_rects(&self, area: Rect) -> (Rect, Rect) {
        let sidebar = sidebar_area(area, self.sidebar_preference());
        let desired = self.quotas.as_ref().map_or(0, |quota| {
            if sidebar.width < 8 {
                0
            } else {
                sidebar_quota_rows(quota, sidebar.width, 0)
                    .iter()
                    .map(|(_, height)| height)
                    .sum()
            }
        });
        let height = if sidebar.width < 8 || sidebar.height <= 3 {
            0
        } else if sidebar.height < desired + 3 {
            u16::from(desired > 0)
        } else {
            desired
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
        if area.height == 1 {
            frame.render_widget(
                Paragraph::new("Quota left: details")
                    .style(Style::default().fg(super::render::TEXT)),
                area,
            );
            return;
        }
        let mut offset = 0;
        for (text, height) in sidebar_quota_rows(quota, area.width, now) {
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(super::render::TEXT)),
                Rect::new(area.x, area.y + offset, area.width, height),
            );
            offset += height;
        }
    }
}

/// Reserve the same rows for current, stale, and reset-due states, without a clock-driven resize.
fn sidebar_quota_rows(quota: &QuotaSnapshot, width: u16, now: u64) -> Vec<(String, u16)> {
    let height = |text: &str| {
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .line_count(width) as u16
    };
    let mut rows = vec![("Quota left".into(), height("Quota left"))];
    for provider in [&quota.claude, &quota.codex, &quota.grok] {
        if !provider.windows.iter().any(|window| window.general) {
            let text = format!("{} — {}", provider.provider.name(), provider.state.label());
            if ratatui::text::Line::raw(&text).width() > usize::from(width) {
                rows.push((provider.provider.name().into(), 1));
                rows.push((
                    provider.state.label().into(),
                    height(provider.state.label()),
                ));
            } else {
                rows.push((text, 1));
            }
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
                let prefix = format!("{name:6} {:2}", window.label);
                let suffix = window_text(provider, window, now);
                let longest_state = "0% exhausted stale";
                if ratatui::text::Line::raw(format!("{prefix} {longest_state}")).width()
                    > usize::from(width)
                {
                    rows.push((prefix.clone(), height(&prefix)));
                    rows.push((suffix, height(longest_state)));
                    continue;
                }
                let reserved = ratatui::text::Line::raw(format!("{prefix} {suffix}")).width();
                let bar_width = usize::from(width).saturating_sub(reserved + 3).min(10);
                let bar = if bar_width >= 3
                    && !window.over_limit
                    && window.resets_unix_ms.is_none_or(|reset| reset > now)
                {
                    window
                        .remaining_basis_points()
                        .map(|left| {
                            let filled = usize::from(left) * bar_width / 10_000;
                            format!(
                                " [{}{}]",
                                "█".repeat(filled),
                                "░".repeat(bar_width - filled)
                            )
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                rows.push((format!("{prefix}{bar} {suffix}"), 1));
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
    if provider.stale(now) && !provider.windows.is_empty() {
        format!("{value} stale")
    } else {
        value
    }
}
