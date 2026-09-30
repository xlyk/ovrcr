use super::{Dashboard, DashboardAction, InputMode, render::sidebar_area};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use ovrcr_protocol::{ProviderQuota, QuotaSource, QuotaState, QuotaWindow};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Clear, Paragraph, Wrap},
};

impl Dashboard {
    pub(super) fn open_quota_details(&mut self) -> DashboardAction {
        self.cancel_mouse_gesture();
        self.mode = InputMode::Browse;
        self.palette = None;
        self.whichkey = None;
        self.quota_details = Some(0);
        DashboardAction::Redraw
    }

    pub(super) fn quota_details_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) || super::input::is_browse_key(key) {
            self.quota_details = None;
        } else if let Some(scroll) = &mut self.quota_details {
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

    pub(super) fn quota_details_mouse(&mut self, mouse: MouseEvent) -> DashboardAction {
        if let Some(scroll) = &mut self.quota_details {
            match mouse.kind {
                MouseEventKind::ScrollDown => *scroll = scroll.saturating_add(1),
                MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(1),
                _ => {}
            }
        }
        DashboardAction::Redraw
    }

    pub(super) fn draw_quota_details(&self, frame: &mut Frame<'_>, now: u64) {
        let Some(scroll) = self.quota_details else {
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
        let block = Block::bordered()
            .title("Quota details · Esc close · ↑↓ scroll")
            .style(
                Style::default()
                    .bg(super::render::BASE)
                    .fg(super::render::TEXT),
            );
        let inner = block.inner(area);
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

    /// Rendering and every tree interaction share this reduced rectangle.
    pub(super) fn sidebar_rects(&self, area: Rect) -> (Rect, Rect) {
        let sidebar = sidebar_area(area, self.sidebar_preference());
        let desired = self.quotas.as_ref().map_or(0, |quota| {
            1 + [&quota.claude, &quota.codex, &quota.grok]
                .iter()
                .map(|provider| {
                    provider
                        .windows
                        .iter()
                        .filter(|window| window.general)
                        .count()
                        .clamp(1, 2) as u16
                })
                .sum::<u16>()
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
        let mut lines = vec![if area.height == 1 {
            "Quota left: details".into()
        } else {
            "Quota left".into()
        }];
        for provider in [&quota.claude, &quota.codex, &quota.grok] {
            if !provider.windows.iter().any(|window| window.general) {
                lines.push(format!(
                    "{} — {}",
                    provider.provider.name(),
                    provider.state.label()
                ));
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
                    let reserved = ratatui::text::Line::raw(format!("{prefix} {suffix}")).width();
                    let bar_width = usize::from(area.width).saturating_sub(reserved + 3).min(10);
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
                    lines.push(format!("{prefix}{bar} {suffix}"));
                }
            }
        }
        for (offset, text) in lines.into_iter().take(usize::from(area.height)).enumerate() {
            frame.render_widget(
                Paragraph::new(super::render::clip_text(&text, usize::from(area.width)))
                    .style(Style::default().fg(super::render::TEXT)),
                Rect::new(area.x, area.y + offset as u16, area.width, 1),
            );
        }
    }
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
