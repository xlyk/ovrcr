//! Provider allowance layout through Dashboard messages, drawing, and input.

use crate::*;
use ovrcr::protocol::{QuotaSnapshot, QuotaState, QuotaWindow};

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
        "Claude — waiting for report",
        "Codex — unavailable",
        "Grok — unavailable",
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
    for expected in [
        "Claude",
        "waiting for report",
        "Codex — unavailable",
        "Grok — unavailable",
    ] {
        assert!(
            rows.iter().any(|row| row == expected),
            "{expected}: {rows:?}"
        );
    }
}

#[test]
fn quota_narrow_sidebar_preserves_window_identity_percentage_and_full_state() {
    for (used, over_limit, reset, now, expected) in [
        (Some(10_000), false, None, 1_000, "0% exhausted"),
        (Some(5_000), false, None, 301_001, "50% stale"),
        (Some(10_000), false, None, 301_001, "0% exhausted stale"),
        (None, true, None, 1_000, "over limit"),
        (None, true, None, 301_001, "over limit stale"),
        (Some(5_000), false, Some(1_000), 1_000, "— reset due"),
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
        assert_eq!(rows[five_hour + 1], expected, "{rows:?}");
        let seven_day = rows
            .iter()
            .position(|row| row.trim() == "7d")
            .expect("independent seven-day window identity");
        assert_eq!(
            rows[seven_day + 1],
            if now > 301_000 { "80% stale" } else { "80%" },
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row == "Codex — unavailable"),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row == "Grok — unavailable"),
            "{rows:?}"
        );
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
