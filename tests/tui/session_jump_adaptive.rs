use super::*;
use crossterm::event::ModifierKeyCode;
use ovrcr::protocol::{SessionKind, SessionRunId};
use ratatui::backend::TestBackend;

fn jump_dashboard(agent_count: u64, rows: u16) -> Dashboard {
    jump_dashboard_at(agent_count, 120, rows)
}

fn jump_dashboard_at(agent_count: u64, cols: u16, rows: u16) -> Dashboard {
    let mut dashboard = Dashboard::with_input_profile(
        TerminalSize { rows, cols },
        super::DashboardInputProfile::VerifiedOvrcrGui,
    );
    dashboard.install_area(Rect::new(0, 0, cols, rows));
    dashboard.install_hierarchy(jump_hierarchy(agent_count));
    dashboard.install_focus(SessionId(1));
    dashboard.install_screen(SessionId(1), &[]);
    dashboard
}

fn key(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, modifiers, kind))
}

fn command(dashboard: &mut Dashboard, kind: KeyEventKind) {
    dashboard.event_action(key(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        kind,
    ));
}

fn digit(dashboard: &mut Dashboard, ch: char, kind: KeyEventKind) {
    dashboard.event_action(key(KeyCode::Char(ch), KeyModifiers::SUPER, kind));
}

fn rendered(dashboard: &Dashboard, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn badge_modifier(dashboard: &Dashboard, width: u16, height: u16, badge: &str) -> Option<Modifier> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    let chars = badge.chars().collect::<Vec<_>>();
    let buffer = terminal.backend().buffer();
    for y in 0..height {
        for x in 0..width.saturating_sub(chars.len() as u16) {
            if chars.iter().enumerate().all(|(offset, expected)| {
                buffer[(x + u16::try_from(offset).unwrap(), y)]
                    .symbol()
                    .starts_with(*expected)
            }) {
                return Some(buffer[(x + 1, y)].modifier);
            }
        }
    }
    None
}

#[test]
fn visible_count_selects_one_or_two_digit_labels_from_zero_to_fifty() {
    for count in [0, 1, 9, 10, 50] {
        let height = if count == 50 { 120 } else { 40 };
        let mut dashboard = jump_dashboard(count, height);
        command(&mut dashboard, KeyEventKind::Press);
        let screen = rendered(&dashboard, 120, height).join("\n");

        match count {
            0 => {
                assert!(screen.find("No visible Agents").is_some());
                assert!(!screen.contains("[1]"));
            }
            1 | 9 => {
                assert!(screen.contains("[1]"), "count {count}: {screen}");
                assert!(
                    screen.contains(&format!("[{count}]")),
                    "count {count}: {screen}"
                );
                assert!(!screen.contains("[01]"), "short lists use one digit");
                assert!(rendered_footer(&dashboard, 120).contains("Cmd+digit"));
            }
            10 | 50 => {
                assert!(screen.contains("[01]"), "count {count}: {screen}");
                assert!(
                    screen.contains(&format!("[{count}]")),
                    "count {count}: {screen}"
                );
                assert!(rendered_footer(&dashboard, 120).contains("two digits"));
                if count == 50 {
                    digit(&mut dashboard, '5', KeyEventKind::Press);
                    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
                    digit(&mut dashboard, '0', KeyEventKind::Press);
                    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
                }
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn two_digit_prefix_can_be_cleared_retried_and_selects_immediately() {
    let mut dashboard = jump_dashboard(10, 40);
    let before = rendered(&dashboard, 120, 40);
    let current_row = before
        .iter()
        .find(|row| row.contains("jump-title-01"))
        .expect("first Agent row is visible")
        .chars()
        .skip(5)
        .collect::<String>();
    command(&mut dashboard, KeyEventKind::Press);
    let held = rendered(&dashboard, 120, 40);
    let current_labeled = held
        .iter()
        .find(|row| row.contains("jump-title-01"))
        .expect("first Agent remains visible while choosing")
        .chars()
        .skip(5)
        .collect::<String>();
    assert_eq!(
        current_labeled, current_row,
        "badge preserves the status/title column"
    );
    assert_eq!(
        held.iter()
            .filter(|row| row.contains("jump-title-"))
            .count(),
        before
            .iter()
            .filter(|row| row.contains("jump-title-"))
            .count(),
        "labels do not add rows"
    );

    digit(&mut dashboard, '1', KeyEventKind::Press);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(rendered_footer(&dashboard, 120).contains("Cmd+1… next digit"));
    assert!(
        badge_modifier(&dashboard, 120, 40, "[10]")
            .is_some_and(|modifier| modifier.contains(Modifier::BOLD)),
        "badges matching the prefix are emphasized"
    );
    assert!(
        badge_modifier(&dashboard, 120, 40, "[01]")
            .is_some_and(|modifier| modifier.contains(Modifier::DIM)),
        "nonmatching badges are visibly de-emphasized"
    );
    digit(&mut dashboard, '9', KeyEventKind::Press);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(rendered_footer(&dashboard, 120).contains("No visible Agent for that code"));

    digit(&mut dashboard, '1', KeyEventKind::Press);
    dashboard.event_action(key(
        KeyCode::Backspace,
        KeyModifiers::SUPER,
        KeyEventKind::Press,
    ));
    assert!(rendered_footer(&dashboard, 120).contains("two digits"));
    assert!(
        rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[10]"))
    );
    digit(&mut dashboard, '1', KeyEventKind::Press);
    digit(&mut dashboard, '0', KeyEventKind::Press);
    assert_eq!(dashboard.focused_session(), Some(SessionId(10)));
    assert!(rendered_footer(&dashboard, 120).contains("Session selected"));
    assert!(
        !rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[10]")),
        "labels disappear after selection"
    );
    digit(&mut dashboard, '2', KeyEventKind::Press);
    assert_eq!(dashboard.focused_session(), Some(SessionId(10)));
}

#[test]
fn codes_follow_drawn_title_rows_in_wide_and_narrow_frames() {
    for (cols, rows) in [(120, 40), (80, 24), (60, 24)] {
        let mut dashboard = jump_dashboard_at(10, cols, rows);
        let before = rendered(&dashboard, cols, rows);
        let visible = (1..=10)
            .filter(|id| {
                before
                    .iter()
                    .any(|row| row.contains(&format!(" - jump-title-{id:02}")))
            })
            .collect::<Vec<_>>();
        assert!(
            !visible.is_empty(),
            "frame {cols}x{rows} shows at least one Agent"
        );

        command(&mut dashboard, KeyEventKind::Press);
        let after = rendered(&dashboard, cols, rows);
        let code_width = if visible.len() >= 10 { 2 } else { 1 };
        for (ordinal, id) in visible.iter().enumerate() {
            let title = format!("jump-title-{id:02}");
            let title_row = format!(" - {title}");
            let before_row = before.iter().find(|row| row.contains(&title_row)).unwrap();
            let label = format!("[{value:0width$}]", value = ordinal + 1, width = code_width);
            let after_row = after
                .iter()
                .find(|row| row.contains(&title) && row.contains(&label))
                .unwrap();
            assert!(
                after_row.contains(&label),
                "{cols}x{rows} {title} receives {label}"
            );
            assert_eq!(
                before_row.chars().skip(5).collect::<String>(),
                after_row.chars().skip(5).collect::<String>(),
                "{cols}x{rows} badge leaves status and clipped title columns fixed"
            );
        }
        assert_eq!(
            before
                .iter()
                .filter(|row| row.contains("jump-title-"))
                .count(),
            after
                .iter()
                .filter(|row| row.contains("jump-title-"))
                .count(),
            "{cols}x{rows} labels do not alter one/two-line row geometry"
        );
    }
}

#[test]
fn fifty_total_agents_use_one_digit_codes_when_only_a_few_title_rows_are_drawn() {
    let mut dashboard = jump_dashboard_at(50, 60, 24);
    let before = rendered(&dashboard, 60, 24);
    let visible = (1..=50)
        .filter(|id| {
            before
                .iter()
                .any(|row| row.contains(&format!(" - jump-title-{id:02}")))
        })
        .collect::<Vec<_>>();
    assert!(
        visible.len() < 10,
        "60×24 draws only part of fifty rows: {}",
        visible.len()
    );
    assert!(!visible.is_empty());

    command(&mut dashboard, KeyEventKind::Press);
    let after = rendered(&dashboard, 60, 24);
    for (ordinal, id) in visible.iter().enumerate() {
        let title = format!("jump-title-{id:02}");
        let badge = format!("[{}]", ordinal + 1);
        assert!(
            after
                .iter()
                .any(|row| row.contains(&title) && row.contains(&badge)),
            "visible Agent {id} receives one-digit label {badge}"
        );
    }
    assert!(!after.iter().any(|row| row.contains("[01]")));
}

#[test]
fn releasing_with_a_partial_code_cancels_without_selecting() {
    let mut dashboard = jump_dashboard(10, 40);
    command(&mut dashboard, KeyEventKind::Press);
    digit(&mut dashboard, '0', KeyEventKind::Press);
    assert!(rendered_footer(&dashboard, 120).contains("Cmd+0… next digit"));
    command(&mut dashboard, KeyEventKind::Release);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(
        !rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[01]")),
        "partial release cancels and hides every code"
    );
}

#[test]
fn one_digit_width_and_targets_stay_fixed_when_candidate_count_crosses_ten() {
    let mut dashboard = jump_dashboard(9, 40);
    command(&mut dashboard, KeyEventKind::Press);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        jump_hierarchy(10),
    )));
    let rows = rendered(&dashboard, 120, 40);
    let tenth = rows
        .iter()
        .find(|row| row.contains("jump-title-10"))
        .expect("new candidate is visible");
    assert!(!tenth.contains("[10]") && !tenth.contains("[1]"));
    digit(&mut dashboard, '9', KeyEventKind::Press);
    assert_eq!(dashboard.focused_session(), Some(SessionId(9)));
}

fn jump_hierarchy(agent_count: u64) -> HierarchySnapshot {
    let mut hierarchy = fixture_hierarchy();
    let project_name = "jump-fixture".to_owned();
    let workspace_id = "visible".to_owned();
    hierarchy.projects.truncate(1);
    hierarchy.projects[0].name = project_name.clone();
    hierarchy.projects[0].workspaces.truncate(1);
    let workspace = &mut hierarchy.projects[0].workspaces[0];
    workspace.project = project_name.clone();
    workspace.name = workspace_id.clone();
    workspace.id = workspace_id.clone();
    workspace.root = false;
    workspace.sessions = (1..=agent_count)
        .map(|id| {
            let mut session = session_summary(
                id,
                &project_name,
                &workspace_id,
                &format!("agent-{id:02}"),
                "fixture-agent",
                Some(u32::try_from(id).unwrap()),
                0,
            );
            session.kind = SessionKind::Agent {
                name: "fixture-agent".into(),
            };
            session.phase = SessionPhase::Running;
            session.run = SessionRunId(id);
            session.title = Some(format!("jump-title-{id:02}"));
            session
        })
        .collect();
    hierarchy
}

#[test]
fn badges_keep_the_frozen_code_width_when_a_target_run_is_replaced() {
    let mut dashboard = jump_dashboard(10, 40);
    command(&mut dashboard, KeyEventKind::Press);
    let mut changed = jump_hierarchy(10);
    let replacement = &mut changed.projects[0].workspaces[0].sessions[9];
    replacement.run = SessionRunId(99);
    replacement.title = Some("replacement-run".into());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(changed)));

    let rows = rendered(&dashboard, 120, 40);
    let replacement_row = rows
        .iter()
        .find(|row| row.contains("replacement-run"))
        .expect("replacement title remains in the list");
    assert!(!replacement_row.contains("[10]"));
    digit(&mut dashboard, '1', KeyEventKind::Press);
    digit(&mut dashboard, '0', KeyEventKind::Press);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(rendered_footer(&dashboard, 120).contains("Agent unavailable"));
}

#[test]
fn two_digit_badges_preserve_stacked_unicode_rows_and_clipped_metadata() {
    let mut hierarchy = jump_hierarchy(10);
    let first = &mut hierarchy.projects[0].workspaces[0].sessions[0];
    first.title = Some("界e\u{301}🙂-clipped-title-that-is-long".into());
    first.kind = SessionKind::Agent {
        name: "agent-label-that-does-not-fit-in-sidebar".into(),
    };
    first.label = "agent-label-that-does-not-fit-in-sidebar / model-that-clips".into();
    let mut dashboard = Dashboard::with_input_profile(
        TerminalSize { rows: 40, cols: 60 },
        super::DashboardInputProfile::VerifiedOvrcrGui,
    );
    dashboard.install_area(Rect::new(0, 0, 60, 40));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    dashboard.install_screen(SessionId(1), &[]);

    let before = rendered(&dashboard, 60, 40);
    let before_title = before
        .iter()
        .find(|row| row.contains(" - ") && row.contains("clipped-title"))
        .unwrap_or_else(|| panic!("Unicode title row is not visible:\n{}", before.join("\n")));
    assert!(
        before_title.contains("界")
            && before_title.contains("e\u{301}")
            && before_title.contains("🙂")
    );
    let before_metadata = before
        .iter()
        .find(|row| row.contains("└ agent-label-that-d"))
        .expect("long Agent metadata wraps to its own row");

    command(&mut dashboard, KeyEventKind::Press);
    let after = rendered(&dashboard, 60, 40);
    let after_title = after
        .iter()
        .find(|row| row.contains("[01]") && row.contains("clipped-title"))
        .expect("two-digit badge fits before the title");
    let after_metadata = after
        .iter()
        .find(|row| row.contains("└ agent-label-that-d"))
        .expect("metadata remains on its own row");
    assert_eq!(
        before_title.chars().skip(5).collect::<String>(),
        after_title.chars().skip(5).collect::<String>(),
        "the badge leaves the clipped Unicode title and status columns unchanged"
    );
    assert_eq!(
        before_metadata, after_metadata,
        "metadata clipping is unchanged"
    );
    assert!(after.iter().any(|row| row.contains("[10]")));
}
