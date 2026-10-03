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

#[test]
fn quota_waiting_providers_are_visible_before_the_first_native_report() {
    let dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    let rows = sidebar_rows(&dashboard, 39, 1_000);
    for expected in [
        "Quota left",
        "Claude — checking",
        "Codex usage off",
        "Grok usage off",
    ] {
        assert!(
            rows.iter().any(|row| row == expected),
            "{expected}: {rows:?}"
        );
    }
}

#[test]
fn quota_narrow_sidebar_preserves_waiting_provider_and_full_state() {
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(
        Box::default(),
    )));
    narrow_sidebar(&mut dashboard);
    let rows = sidebar_rows(&dashboard, 19, 1_000);
    for expected in ["Claude — checking", "Codex usage off", "Grok usage off"] {
        assert!(
            rows.iter().any(|row| row == expected),
            "{expected}: {rows:?}"
        );
    }
}

#[test]
fn quota_narrow_sidebar_preserves_window_identity_percentage_and_full_state() {
    for (used, over_limit, reset, now, expected) in [
        (Some(10_000), false, None, 1_000, &["0% exhausted"][..]),
        (Some(5_000), false, None, 301_001, &["50% left  stale 5m"]),
        (
            Some(10_000),
            false,
            None,
            301_001,
            &["0% exhausted  stale", "5m"],
        ),
        (None, true, None, 1_000, &["over limit"]),
        (None, true, None, 301_001, &["over limit  stale", "5m"]),
        (Some(5_000), false, Some(1_000), 1_000, &["— reset due"]),
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
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
            snapshot,
        ))));
        let rows = sidebar_rows(&dashboard, 19, now);
        let five_hour = rows
            .iter()
            .position(|row| row.contains("Claude") && row.contains("5h"))
            .expect("provider and five-hour window identity");
        assert_eq!(
            rows[five_hour + 1..five_hour + 1 + expected.len()],
            *expected,
            "{rows:?}"
        );
        let seven_day = rows
            .iter()
            .position(|row| row.trim() == "7d")
            .expect("independent seven-day window identity");
        assert_eq!(
            rows[seven_day + 1],
            if now > 301_000 {
                "80% left  stale 5m"
            } else {
                "80%"
            },
            "{rows:?}"
        );
        assert!(rows.iter().any(|row| row == "Codex usage off"), "{rows:?}");
        assert!(rows.iter().any(|row| row == "Grok usage off"), "{rows:?}");
    }
}

#[test]
fn quota_wrapped_rows_preserve_last_tree_selection_and_exclude_quota_mouse_hits() {
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
    let quota_row = current
        .iter()
        .position(|row| row == "Quota left")
        .expect("quota heading");
    assert_eq!(
        expired.iter().position(|row| row == "Quota left"),
        Some(quota_row),
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
    assert_eq!(last_tree_row + 1, quota_row, "{current:?}");
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
    for row in quota_row as u16..39 {
        dashboard.mouse_action(
            mouse_event(MouseEventKind::Moved, 18, row, KeyModifiers::NONE),
            area,
        );
        assert!(
            matches!(
                dashboard.mouse_action(
                    mouse_event(
                        MouseEventKind::Down(MouseButton::Left),
                        18,
                        row,
                        KeyModifiers::NONE
                    ),
                    area
                ),
                DashboardAction::None | DashboardAction::Redraw
            ),
            "quota row {row} must not close or select a session"
        );
        assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    }
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
fn quota_block_shows_off_checking_current_and_failed_with_retry() {
    let mut dashboard = dashboard_fixture();
    let rows = sidebar_rows(&dashboard, 39, NOW);
    for expected in ["Claude — checking", "Codex usage off", "Grok usage off"] {
        assert!(
            rows.iter().any(|row| row == expected),
            "{expected}: {rows:?}"
        );
    }
    publish_quota(&mut dashboard, mixed_snapshot());
    let rows = sidebar_rows(&dashboard, 39, NOW);
    assert!(
        rows.iter().any(|row| row == "Claude — checking"),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row.starts_with("Codex  5h [") && row.ends_with("] 37%")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|row| row == "Grok — unavailable  retry 3m"),
        "{rows:?}"
    );
    // The retry marker counts down from next_check, never resets on redraw.
    let later = sidebar_rows(&dashboard, 39, NOW + 120_000);
    assert!(
        later
            .iter()
            .any(|row| row == "Grok — unavailable  retry 1m"),
        "{later:?}"
    );
    let due = sidebar_rows(&dashboard, 39, NOW + 170_000);
    assert!(
        due.iter().any(|row| row == "Grok — unavailable  retry now"),
        "{due:?}"
    );
}

#[test]
fn quota_block_shows_a_stale_value_with_its_age() {
    let mut dashboard = dashboard_fixture();
    let mut snapshot = mixed_snapshot();
    snapshot.codex.state = QuotaState::Unavailable;
    snapshot.codex.checked_unix_ms = Some(NOW - 12 * 60_000);
    snapshot.codex.observed_unix_ms = Some(NOW - 30 * 60_000);
    publish_quota(&mut dashboard, snapshot);
    let rows = sidebar_rows(&dashboard, 39, NOW);
    assert!(
        rows.iter()
            .any(|row| row.starts_with("Codex  5h") && row.ends_with("37% left  stale 12m")),
        "{rows:?}"
    );
    let hours = sidebar_rows(&dashboard, 39, NOW + 3 * 3_600_000);
    assert!(
        hours.iter().any(|row| row.ends_with("37% left  stale 3h")),
        "{hours:?}"
    );
}

#[test]
fn quota_block_ladder_gives_three_lines_then_one_then_none() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.clear();
    dashboard.install_hierarchy(hierarchy);
    publish_quota(&mut dashboard, mixed_snapshot());
    let full = rows_at(&dashboard, 12, NOW);
    assert!(full.iter().any(|row| row == "Quota left"), "{full:?}");
    let mut seen = Vec::new();
    for height in (3..12).rev() {
        let rows = rows_at(&dashboard, height, NOW);
        let compact = [
            "Claude — checking",
            "Codex 5h 37%",
            "Grok — unavailable  retry 3m",
        ];
        let state = if rows.iter().any(|row| row == "Quota left") {
            "full"
        } else if let Some(at) = rows.iter().position(|row| row == compact[0]) {
            assert_eq!(rows[at..at + 3], compact, "height {height}: {rows:?}");
            "three"
        } else if rows.iter().any(|row| row == "Quota: u") {
            "one"
        } else {
            assert!(
                !rows.iter().any(|row| row.contains("Quota")
                    || row.contains("Claude")
                    || row.contains("Codex")),
                "height {height}: {rows:?}"
            );
            "none"
        };
        if seen.last() != Some(&state) {
            seen.push(state);
        }
    }
    assert_eq!(seen, ["full", "three", "one", "none"]);
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

#[test]
fn quota_details_show_off_sentence_reason_local_times_and_next_check() {
    let local = |stamp: u64| {
        chrono::DateTime::from_timestamp_millis(stamp as i64)
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    };
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('u'));
    let off = details_text(&dashboard, NOW);
    assert!(off.contains("Quota details"), "{off}");
    assert!(
        off.contains("Codex/Grok usage off: set `quota.enabled = true` in dashboard.toml"),
        "{off}"
    );
    assert!(
        off.contains("reason: waiting for a managed Claude session's first response"),
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
    assert!(text.contains("reason: timed out after 20 s"), "{text}");
    assert!(
        text.contains(&format!(
            "last observation: {} (1m ago)",
            local(NOW - 60_000)
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "last account check: {} (1m ago)",
            local(NOW - 60_000)
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("next check: {} (in 3m)", local(NOW + 170_000))),
        "{text}"
    );
    assert!(text.contains("next check: not scheduled"), "{text}");
    let lower = text.to_lowercase();
    for banned in ["token", "spend", "cost"] {
        assert!(!lower.contains(banned), "{banned}: {text}");
    }
}

fn palette_request(dashboard: &mut Dashboard, query: &str) -> ClientMessage {
    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste(query.into()));
    match dashboard.key(KeyCode::Enter) {
        DashboardAction::Request(message) => message,
        other => panic!("{query} sent no request: {other:?}"),
    }
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
    assert!(
        footer.contains("Quota refresh cooling down: 24s left"),
        "{footer}"
    );
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
    dashboard.event_action(Event::Paste("enable codex".into()));
    let listed = sidebar_rows(&dashboard, 120, NOW).join("\n");
    assert!(listed.contains("Enable Codex and Grok usage"), "{listed}");
    dashboard.key(KeyCode::Esc);
    let message = palette_request(&mut dashboard, "enable codex and grok usage");
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
    let rows = sidebar_rows(&dashboard, 39, NOW);
    assert!(rows.iter().any(|row| row == "Codex usage off"), "{rows:?}");
    // Once the Server republishes the setting on, the command is gone.
    publish_quota(&mut dashboard, mixed_snapshot());
    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste("enable codex".into()));
    let rows = sidebar_rows(&dashboard, 120, NOW).join("\n");
    assert!(!rows.contains("Enable Codex and Grok usage"), "{rows}");
}

#[test]
fn quota_details_name_a_probe_source_with_its_time_and_age() {
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
    let when = chrono::DateTime::from_timestamp_millis((NOW - 60_000) as i64)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    assert!(
        text.contains(&format!("source: probe; last probe {when} (1m ago)")),
        "{text}"
    );
}

#[test]
fn quota_rows_keep_reported_windows_and_dash_missing_allowance() {
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
    let joined = rows.join("\n");
    assert!(joined.contains("Quota left"), "{rows:?}");
    assert!(
        rows.iter()
            .any(|row| row.contains("Claude") && row.contains("5h") && row.contains("58%")),
        "{rows:?}"
    );
    let seven = rows
        .iter()
        .find(|row| row.contains("7d"))
        .expect("seven-day row");
    assert!(seven.contains('—') || seven.contains("—"), "{seven}");
    assert!(
        !seven.contains('█') && !seven.contains('░'),
        "missing allowance drew a bar: {seven}"
    );
    assert!(
        rows.iter().any(|row| row.contains("Grok")
            && row.contains("wk")
            && !row.contains("5h")
            && !row.contains("7d")),
        "{rows:?}"
    );
}
