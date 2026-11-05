//! Provider allowance layout through Dashboard messages, drawing, and input.

use crate::*;
use ovrcr::protocol::{
    ProviderQuota, QuotaProvider, QuotaSnapshot, QuotaSource, QuotaState, QuotaWindow,
};

fn sidebar_rows(dashboard: &Dashboard, width: u16, now: u64) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, now))
        .unwrap();
    (0..40)
        .map(|row| {
            (0..width)
                .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn narrow_sidebar(dashboard: &mut Dashboard) {
    let area = Rect::new(0, 0, 120, 40);
    dashboard.install_area(area);
    for (kind, column) in [
        (MouseEventKind::Down(MouseButton::Left), 39),
        (MouseEventKind::Drag(MouseButton::Left), 19),
        (MouseEventKind::Up(MouseButton::Left), 19),
    ] {
        assert_eq!(
            dashboard.mouse_action(mouse_event(kind, column, 10, KeyModifiers::NONE), area),
            DashboardAction::Redraw
        );
    }
    assert_eq!(dashboard.pane_rects(area)[0].terminal.x, 20);
}

/// The sidebar's last row, where the one-line quota summary sits.
fn quota_row(dashboard: &Dashboard, width: u16, now: u64) -> String {
    sidebar_rows(dashboard, width, now)[38].clone()
}

/// The quota row's cells with their colours.
fn quota_cells(dashboard: &Dashboard, now: u64) -> Vec<(String, Color)> {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, now))
        .unwrap();
    (0..39)
        .map(|x| {
            let cell = &terminal.backend().buffer()[(x, 38)];
            (cell.symbol().to_owned(), cell.fg)
        })
        .collect()
}

/// The colours of `token`'s cells on the quota row.
fn token_colors(cells: &[(String, Color)], token: &str) -> Vec<Color> {
    let len = token.chars().count();
    let at = (0..cells.len() - len)
        .find(|&x| {
            cells[x..x + len]
                .iter()
                .map(|(symbol, _)| symbol.as_str())
                .collect::<String>()
                == token
        })
        .unwrap_or_else(|| panic!("{token} missing: {cells:?}"));
    cells[at..at + len].iter().map(|(_, fg)| *fg).collect()
}

const MUTED: Color = Color::Rgb(108, 112, 134);
const RED: Color = Color::Rgb(243, 139, 168);
const TEXT: Color = Color::Rgb(205, 214, 244);

#[test]
fn quota_waiting_providers_are_visible_before_the_first_native_report() {
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    assert_eq!(quota_row(&dashboard, 39, 1_000), " Claude ?");
    let mut snapshot = QuotaSnapshot::default();
    for provider in [&mut snapshot.codex, &mut snapshot.grok] {
        provider.state = QuotaState::Checking;
        provider.reason = None;
    }
    publish_quota(&mut dashboard, snapshot);
    assert_eq!(
        quota_row(&dashboard, 39, 1_000),
        " Claude ? · Codex ? · Grok ?",
        "a provider still checking is named; one switched off is omitted"
    );
}

#[test]
fn quota_narrow_sidebar_keeps_whole_providers_and_points_to_the_rest() {
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(
        Box::default(),
    )));
    narrow_sidebar(&mut dashboard);
    assert_eq!(quota_row(&dashboard, 19, 1_000), " Claude ?");
    publish_quota(&mut dashboard, mixed_snapshot());
    assert_eq!(
        quota_row(&dashboard, 19, NOW),
        " Codex 37% …",
        "providers without a value give way first; never a clipped number"
    );
}

#[test]
fn quota_line_shows_the_least_allowance_left_and_mutes_stale_values() {
    for (used, over_limit, reset, now, expected, color) in [
        (Some(10_000), false, None, 1_000, " Claude 0%", RED),
        (Some(5_000), false, None, 301_001, " Claude 50%", MUTED),
        (Some(10_000), false, None, 301_001, " Claude 0%", MUTED),
        (None, true, None, 1_000, " Claude 0%", RED),
        (None, true, None, 301_001, " Claude 0%", MUTED),
        // A passed reset leaves the seven-day window's value.
        (Some(5_000), false, Some(1_000), 1_000, " Claude 80%", TEXT),
    ] {
        let mut dashboard = dashboard_fixture();
        narrow_sidebar(&mut dashboard);
        let mut snapshot = QuotaSnapshot::default();
        snapshot.claude.state = QuotaState::Current;
        snapshot.claude.observed_unix_ms = Some(1_000);
        snapshot.claude.windows = vec![
            QuotaWindow {
                id: "five-hour".into(),
                label: "5h".into(),
                general: true,
                used_basis_points: used,
                over_limit,
                resets_unix_ms: reset,
            },
            QuotaWindow {
                id: "seven-day".into(),
                label: "7d".into(),
                general: true,
                used_basis_points: Some(2_000),
                over_limit: false,
                resets_unix_ms: None,
            },
        ];
        publish_quota(&mut dashboard, snapshot);
        let row = quota_row(&dashboard, 19, now);
        assert_eq!(row, expected, "{used:?} {over_limit} {reset:?} {now}");
        let cells = quota_cells(&dashboard, now);
        let value = expected.trim_start().trim_start_matches("Claude ");
        assert!(
            token_colors(&cells, value).iter().all(|fg| *fg == color),
            "{row}: {cells:?}"
        );
    }
}

#[test]
fn quota_line_sits_under_the_last_tree_row_and_a_click_opens_details() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    hierarchy.projects[0].workspaces[0].sessions = (1..=50)
        .map(|id| {
            session_summary(
                id,
                "spacelift-agent",
                "progress",
                &format!("session-{id}"),
                "sh",
                Some(id as u32),
                0,
            )
        })
        .collect();
    dashboard.install_hierarchy(hierarchy);
    narrow_sidebar(&mut dashboard);
    dashboard.install_focus(SessionId(50));
    let mut snapshot = QuotaSnapshot::default();
    snapshot.claude.state = QuotaState::Current;
    snapshot.claude.observed_unix_ms = Some(1_000);
    snapshot.claude.windows = ["5h", "7d"]
        .into_iter()
        .map(|label| QuotaWindow {
            id: label.into(),
            label: label.into(),
            general: true,
            used_basis_points: Some(10_000),
            over_limit: false,
            resets_unix_ms: Some(2_000),
        })
        .collect();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
        snapshot,
    ))));
    let current = sidebar_rows(&dashboard, 19, 1_000);
    let expired = sidebar_rows(&dashboard, 19, 301_001);
    assert_eq!(current[38], " Claude 0%");
    assert_eq!(
        expired[38], " Claude ?",
        "a passed reset leaves no value, and the line keeps its row"
    );
    assert_eq!(
        current[..38],
        expired[..38],
        "expiry must preserve tree geometry"
    );
    let last_tree_row = current
        .iter()
        .position(|row| row.contains("session-50"))
        .expect("last session visible")
        + 1;
    assert_eq!(
        current[last_tree_row], "▌      └ sh",
        "the stacked label is the last selectable tree line"
    );
    assert_eq!(last_tree_row + 1, 38, "{current:?}");
    dashboard.install_focus(SessionId(49));
    let action = dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            6,
            last_tree_row as u16,
            KeyModifiers::NONE,
        ),
        area,
    );
    assert!(
        !matches!(action, DashboardAction::None | DashboardAction::PtyBytes(_)),
        "last tree row must select: {action:?}"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    dashboard.drain_outbox();
    dashboard.mouse_action(
        mouse_event(MouseEventKind::Moved, 18, 38, KeyModifiers::NONE),
        area,
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                18,
                38,
                KeyModifiers::NONE
            ),
            area
        ),
        DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(50)),
        "the quota line must not close or select a session"
    );
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        details_text(&dashboard, 1_000).contains("Quota details"),
        "the same details as u"
    );
}

const NOW: u64 = 1_800_000_000_000;

fn window(label: &str, used: u16) -> QuotaWindow {
    QuotaWindow {
        id: label.into(),
        label: label.into(),
        general: true,
        used_basis_points: Some(used),
        over_limit: false,
        resets_unix_ms: None,
    }
}

/// Claude Checking, Codex Current, Grok failed with a retry three minutes out.
fn mixed_snapshot() -> QuotaSnapshot {
    let mut snapshot = QuotaSnapshot::default();
    snapshot.codex.state = QuotaState::Current;
    snapshot.codex.reason = None;
    snapshot.codex.checked_unix_ms = Some(NOW - 60_000);
    snapshot.codex.observed_unix_ms = Some(NOW - 60_000);
    snapshot.codex.next_check_unix_ms = Some(NOW + 240_000);
    snapshot.codex.windows = vec![window("5h", 6_300)];
    snapshot.grok.state = QuotaState::Unavailable;
    snapshot.grok.reason = Some("timed out after 20 s".into());
    snapshot.grok.next_check_unix_ms = Some(NOW + 170_000);
    snapshot
}

fn publish_quota(dashboard: &mut Dashboard, snapshot: QuotaSnapshot) {
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
        snapshot,
    ))));
}

fn rows_at(dashboard: &Dashboard, height: u16, now: u64) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(120, height)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, now))
        .unwrap();
    (0..height)
        .map(|row| {
            (0..39)
                .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[test]
fn quota_line_shows_checking_current_and_failed_providers() {
    let mut dashboard = dashboard_fixture();
    assert_eq!(quota_row(&dashboard, 39, NOW), " Claude ?");
    publish_quota(&mut dashboard, mixed_snapshot());
    // Checking and failed providers have no value to show; when the failed one retries is
    // in the details.
    assert_eq!(
        quota_row(&dashboard, 39, NOW),
        " Claude ? · Codex 37% · Grok ?"
    );
    let cells = quota_cells(&dashboard, NOW);
    assert!(
        token_colors(&cells, "Claude ?")
            .iter()
            .all(|fg| *fg == MUTED)
    );
    assert!(token_colors(&cells, "Grok ?").iter().all(|fg| *fg == MUTED));
    assert!(token_colors(&cells, " 37%").iter().all(|fg| *fg == TEXT));
}

#[test]
fn quota_line_mutes_a_stale_value() {
    let mut dashboard = dashboard_fixture();
    let mut snapshot = mixed_snapshot();
    snapshot.codex.state = QuotaState::Unavailable;
    snapshot.codex.checked_unix_ms = Some(NOW - 12 * 60_000);
    snapshot.codex.observed_unix_ms = Some(NOW - 30 * 60_000);
    publish_quota(&mut dashboard, snapshot);
    assert_eq!(
        quota_row(&dashboard, 39, NOW),
        " Claude ? · Codex 37% · Grok ?",
        "the last good value is kept; its age is in the details"
    );
    let cells = quota_cells(&dashboard, NOW);
    assert!(
        token_colors(&cells, "Codex 37%")
            .iter()
            .all(|fg| *fg == MUTED),
        "{cells:?}"
    );
}

#[test]
fn quota_line_hides_below_the_operator_heading_and_three_tree_lines() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.clear();
    dashboard.install_hierarchy(hierarchy);
    publish_quota(&mut dashboard, mixed_snapshot());
    for height in (3..12).rev() {
        let rows = rows_at(&dashboard, height, NOW);
        // Title and footer rows sit outside the sidebar.
        let shown = height - 2 >= 5;
        assert_eq!(
            rows.iter().position(|row| row.starts_with(" Claude ?")),
            shown.then_some(usize::from(height) - 2),
            "height {height}: {rows:?}"
        );
    }
}

fn details_text(dashboard: &Dashboard, now: u64) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, now))
        .unwrap();
    (0..40)
        .map(|row| {
            (0..120)
                .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A popup clock time as `when` writes it in the local zone: the weekday joins it only when
/// the moment falls on another local day than `NOW`.
fn clock(stamp: u64) -> String {
    let local = |ms: u64| {
        chrono::DateTime::from_timestamp_millis(ms as i64)
            .unwrap()
            .with_timezone(&chrono::Local)
    };
    let pattern = if local(stamp).date_naive() == local(NOW).date_naive() {
        "%-I:%M %p"
    } else {
        "%a %-I:%M %p"
    };
    local(stamp).format(pattern).to_string()
}

#[test]
fn quota_details_show_off_sentence_reason_local_times_and_next_check() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('u'));
    let off = details_text(&dashboard, NOW);
    assert!(off.contains("Quota details"), "{off}");
    assert!(off.contains("Codex · off"), "{off}");
    assert!(
        off.contains("Codex/Grok usage off: set `quota.enabled = true` in dashboard.toml"),
        "{off}"
    );
    assert!(off.contains("Claude · checking…"), "{off}");
    assert!(
        off.contains("  waiting for a managed Claude session's first response"),
        "{off}"
    );
    dashboard.key(KeyCode::Esc);
    publish_quota(&mut dashboard, mixed_snapshot());
    dashboard.key(KeyCode::Char('u'));
    let text = details_text(&dashboard, NOW);
    assert_eq!(
        text.matches("Subscription allowance only").count(),
        1,
        "{text}"
    );
    // A current reading: its window by length, how old it is and when the next check runs.
    assert!(text.contains("Codex · up to date"), "{text}");
    assert!(text.contains("  5-hour  37% left"), "{text}");
    assert!(
        text.contains(&format!(
            "  as of {} (1m ago) · next check {} (in 4m)",
            clock(NOW - 60_000),
            clock(NOW + 240_000)
        )),
        "{text}"
    );
    // A failed read: its reason in words and the retry, with no reading to date.
    assert!(text.contains("Grok · unavailable"), "{text}");
    assert!(text.contains("  timed out after 20 s"), "{text}");
    assert!(
        text.contains(&format!("  next check {} (in 3m)", clock(NOW + 170_000))),
        "{text}"
    );
    let lower = text.to_lowercase();
    for banned in [
        "token",
        "spend",
        "cost",
        "reason:",
        "source:",
        "generation",
        "[5h]",
        "last observation",
        "last account check",
        "native callback",
        "not scheduled",
    ] {
        assert!(!lower.contains(banned), "{banned}: {text}");
    }
}

fn palette_request(dashboard: &mut Dashboard, query: &str) -> ClientMessage {
    palette_search(dashboard, query);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    only_request(dashboard.drain_outbox())
}

#[test]
fn palette_refresh_quota_sends_refresh_and_shows_cooldown_in_footer() {
    let mut dashboard = dashboard_fixture();
    publish_quota(&mut dashboard, mixed_snapshot());
    let message = palette_request(&mut dashboard, "refresh quota");
    assert_eq!(
        message.request,
        ovrcr::protocol::Request::RefreshQuota { provider: None }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::QuotaCooldown {
            remaining_ms: 23_400,
        },
    });
    let footer = rendered_footer(&dashboard, 120);
    // A cooldown is an expected limit, not an error.
    assert_eq!(footer, "Quota refresh cooling down: 24s left");
    // An accepted refresh clears it; the Server's next snapshot is the result.
    let message = palette_request(&mut dashboard, "refresh quota");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    assert!(!rendered_footer(&dashboard, 120).contains("cooling down"));
}

#[test]
fn palette_enable_usage_asks_the_server_and_applies_nothing() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(':'));
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    dashboard.event_action(Event::Paste("enable native".into()));
    let listed = sidebar_rows(&dashboard, 120, NOW).join("\n");
    assert!(listed.contains("Enable native account usage"), "{listed}");
    dashboard.key(KeyCode::Esc);
    let message = palette_request(&mut dashboard, "enable native account usage");
    assert_eq!(
        message.request,
        ovrcr::protocol::Request::SetSetting {
            path: "quota.enabled".into(),
            value: Some("true".into()),
        }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        quota_row(&dashboard, 39, NOW),
        " Claude ?",
        "Codex stays off until the Server republishes"
    );
    // Once the Server republishes the setting on, the command is gone.
    publish_quota(&mut dashboard, mixed_snapshot());
    dashboard.key(KeyCode::Char(':'));
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    dashboard.event_action(Event::Paste("enable native".into()));
    let rows = sidebar_rows(&dashboard, 120, NOW).join("\n");
    assert!(!rows.contains("Enable native account usage"), "{rows}");
}

#[test]
fn quota_details_show_a_probe_reading_in_words_without_its_provenance() {
    let mut dashboard = dashboard_fixture();
    let snapshot = QuotaSnapshot {
        claude: ProviderQuota {
            source: Some(QuotaSource::Probe {
                probed_unix_ms: NOW - 60_000,
            }),
            observed_unix_ms: Some(NOW - 60_000),
            state: QuotaState::Current,
            windows: vec![QuotaWindow {
                id: "five_hour".into(),
                label: "5h".into(),
                general: true,
                used_basis_points: Some(1_200),
                over_limit: false,
                resets_unix_ms: Some(NOW + 3_600_000),
            }],
            ..ProviderQuota::unknown(QuotaProvider::Claude, QuotaState::Current)
        },
        ..QuotaSnapshot::default()
    };
    publish_quota(&mut dashboard, snapshot);
    dashboard.key(KeyCode::Char('u'));
    let text = details_text(&dashboard, NOW);
    assert!(text.contains("Claude · up to date"), "{text}");
    assert!(
        text.contains(&format!(
            "  5-hour  88% left · resets {} (in 1h)",
            clock(NOW + 3_600_000)
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("  as of {} (1m ago)", clock(NOW - 60_000))),
        "{text}"
    );
    for internal in ["source", "probe", "five_hour", "generation", "reset:"] {
        assert!(!text.contains(internal), "{internal}: {text}");
    }
}

#[test]
fn quota_line_skips_unknown_windows_and_names_each_provider_once() {
    let mut dashboard = dashboard_fixture();
    let mut snapshot = QuotaSnapshot::default();
    snapshot.claude.state = QuotaState::Current;
    snapshot.claude.observed_unix_ms = Some(NOW);
    snapshot.claude.windows = vec![
        QuotaWindow {
            id: "five_hour".into(),
            label: "5h".into(),
            general: true,
            used_basis_points: Some(4_200),
            over_limit: false,
            resets_unix_ms: Some(NOW + 3_600_000),
        },
        QuotaWindow {
            id: "seven_day".into(),
            label: "7d".into(),
            general: true,
            used_basis_points: None,
            over_limit: false,
            resets_unix_ms: Some(NOW + 3_600_000),
        },
    ];
    snapshot.grok.state = QuotaState::Current;
    snapshot.grok.windows = vec![QuotaWindow {
        id: "grok/week".into(),
        label: "wk".into(),
        general: true,
        used_basis_points: Some(2_400),
        over_limit: false,
        resets_unix_ms: Some(NOW + 3_600_000),
    }];
    publish_quota(&mut dashboard, snapshot);
    let rows = sidebar_rows(&dashboard, 39, NOW);
    assert_eq!(rows[38], " Claude 58% · Grok 76%", "{rows:?}");
    let joined = rows.join("\n");
    for gone in ["QUOTA LEFT", "━", "5h", "7d", "wk"] {
        assert!(!joined.contains(gone), "{gone}: {rows:?}");
    }
    let cells = quota_cells(&dashboard, NOW);
    assert!(
        token_colors(&cells, "Grok 76%")
            .iter()
            .all(|fg| *fg == MUTED),
        "a reading without a time cannot vouch for itself"
    );
}

#[test]
fn cursor_details_display_used_remaining_and_unknown_without_fabricated_zero() {
    let mut dashboard = dashboard_fixture();
    let mut quota = mixed_snapshot();
    quota.cursor = ProviderQuota {
        windows: vec![
            QuotaWindow {
                id: "cursor/plan".into(),
                label: "plan".into(),
                general: true,
                used_basis_points: Some(3000),
                over_limit: false,
                resets_unix_ms: Some(NOW + 3_600_000),
            },
            QuotaWindow {
                id: "cursor/onDemand".into(),
                label: "on-demand".into(),
                general: false,
                used_basis_points: None,
                over_limit: false,
                resets_unix_ms: None,
            },
        ],
        observed_unix_ms: Some(NOW),
        checked_unix_ms: Some(NOW),
        ..ProviderQuota::unknown(QuotaProvider::Cursor, QuotaState::Current)
    };
    publish_quota(&mut dashboard, quota);
    dashboard.key(KeyCode::Char('u'));
    for _ in 0..50 {
        dashboard.key(KeyCode::Down);
    }
    let details = details_text(&dashboard, NOW);
    assert!(
        details.contains(&format!(
            "  plan  70% left · resets {} (in 1h)",
            clock(NOW + 3_600_000)
        )),
        "{details}"
    );
    assert!(
        details.contains("    30.00% used · 70.00% left"),
        "{details}"
    );
    // Unknown allowance stays unknown: never a fabricated zero.
    assert!(details.contains("  on-demand  unknown"), "{details}");
    assert!(details.contains("    — used · — left"), "{details}");
    assert!(!details.contains("cursor/"), "{details}");
}
