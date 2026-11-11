use super::*;
use ovrcr::protocol::{SessionKind, SessionRunId};
use ratatui::backend::TestBackend;

fn jump_dashboard(agent_count: u64, rows: u16) -> Dashboard {
    jump_dashboard_at(agent_count, 120, rows)
}

fn jump_dashboard_at(agent_count: u64, cols: u16, rows: u16) -> Dashboard {
    let mut dashboard = Dashboard::new(TerminalSize { rows, cols });
    dashboard.install_area(Rect::new(0, 0, cols, rows));
    dashboard.install_hierarchy(jump_hierarchy(agent_count));
    dashboard.install_focus(SessionId(1));
    dashboard.install_screen(SessionId(1), &[]);
    dashboard
}

fn key(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, modifiers, kind))
}

fn ctrl_g(dashboard: &mut Dashboard) {
    dashboard.event_action(key(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    ));
}

fn digit(dashboard: &mut Dashboard, ch: char) {
    dashboard.event_action(key(
        KeyCode::Char(ch),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
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

fn sidebar(row: &str) -> &str {
    row.split('│').next().unwrap_or(row)
}

#[test]
fn visible_count_uses_only_single_digit_labels_from_zero_to_fifty() {
    for count in [0, 1, 9, 10, 50] {
        let height = if count == 50 { 120 } else { 40 };
        let mut dashboard = jump_dashboard(count, height);
        ctrl_g(&mut dashboard);
        let rows = rendered(&dashboard, 120, height);
        let screen = rows
            .iter()
            .map(|row| sidebar(row))
            .collect::<Vec<_>>()
            .join("\n");

        match count {
            0 => {
                assert!(screen.find("No visible Agents").is_some());
                assert!(!screen.contains("[1]"));
            }
            _ => {
                assert!(screen.contains("[1]"), "count {count}: {screen}");
                assert!(rendered_footer(&dashboard, 120).contains("1–9 jump"));
                for id in 1..=count {
                    let row = rows
                        .iter()
                        .map(|row| sidebar(row))
                        .find(|row| row.contains(&format!("jump-title-{id:02}")))
                        .unwrap_or_else(|| panic!("Agent {id} is visible in count {count}"));
                    assert_eq!(
                        row.contains(&format!("[{id}]")),
                        id <= 9,
                        "count {count}, Agent {id}: {row}"
                    );
                }
                assert!(!screen.contains("[01]"), "labels always use one digit");
                if count > 9 {
                    digit(&mut dashboard, '0');
                    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
                    assert!(
                        rendered_footer(&dashboard, 120)
                            .contains("No visible Agent for that number")
                    );
                    assert!(dashboard.drain_outbox().is_empty());
                    digit(&mut dashboard, '9');
                    assert_eq!(dashboard.focused_session(), Some(SessionId(9)));
                }
            }
        }
    }
}

#[test]
fn cmd_backspace_cancels_without_navigation_or_child_input() {
    let mut dashboard = jump_dashboard(10, 40);
    let before = rendered(&dashboard, 120, 40);
    let current_row = before
        .iter()
        .map(|row| sidebar(row))
        .find(|row| row.contains("jump-title-01"))
        .expect("first Agent row is visible")
        .chars()
        .skip(5)
        .collect::<String>();
    ctrl_g(&mut dashboard);
    let held = rendered(&dashboard, 120, 40);
    let current_labeled = held
        .iter()
        .map(|row| sidebar(row))
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
            .map(|row| sidebar(row))
            .filter(|row| row.contains("jump-title-"))
            .count(),
        before
            .iter()
            .map(|row| sidebar(row))
            .filter(|row| row.contains("jump-title-"))
            .count(),
        "labels do not add rows"
    );

    dashboard.event_action(key(
        KeyCode::Backspace,
        KeyModifiers::SUPER,
        KeyEventKind::Press,
    ));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(rendered_footer(&dashboard, 120).starts_with("BROWSE"));
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        !rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]")),
        "an unrelated Command shortcut cancels and hides the labels"
    );
    digit(&mut dashboard, '2');
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn control_digit_with_extra_modifier_does_not_select_a_jump_target() {
    let mut dashboard = jump_dashboard(10, 40);
    let focused = dashboard.focused_session();
    ctrl_g(&mut dashboard);

    assert_eq!(
        dashboard.event_action(key(
            KeyCode::Char('9'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyEventKind::Press,
        )),
        DashboardAction::None,
        "Ctrl+Shift+9 keeps its ordinary Browse dispatch"
    );
    assert_eq!(dashboard.focused_session(), focused);
    assert!(rendered_footer(&dashboard, 120).starts_with("BROWSE"));
    assert!(
        !rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| sidebar(row).contains("[1]")),
        "an extra modifier prevents navigation"
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn first_nine_labels_follow_drawn_title_rows_in_wide_and_narrow_frames() {
    for (cols, rows) in [(120, 40), (80, 24), (60, 24)] {
        let mut dashboard = jump_dashboard_at(10, cols, rows);
        let before = rendered(&dashboard, cols, rows);
        let visible = (1..=10)
            .filter(|id| {
                before
                    .iter()
                    .any(|row| sidebar(row).contains(&format!(" - jump-title-{id:02}")))
            })
            .collect::<Vec<_>>();
        assert!(
            !visible.is_empty(),
            "frame {cols}x{rows} shows at least one Agent"
        );

        ctrl_g(&mut dashboard);
        let after = rendered(&dashboard, cols, rows);
        for (ordinal, id) in visible.iter().enumerate() {
            let title = format!("jump-title-{id:02}");
            let title_row = format!(" - {title}");
            let before_row = before
                .iter()
                .map(|row| sidebar(row))
                .find(|row| row.contains(&title_row))
                .unwrap();
            let after_row = after
                .iter()
                .map(|row| sidebar(row))
                .find(|row| row.contains(&title))
                .unwrap();
            if ordinal < 9 {
                let label = format!("[{}]", ordinal + 1);
                assert!(
                    after_row.contains(&label),
                    "{cols}x{rows} {title} receives {label}"
                );
            } else {
                assert!(
                    !after_row.contains("["),
                    "{cols}x{rows} later visible Agent {title} receives no label"
                );
            }
            assert_eq!(
                before_row.chars().skip(5).collect::<String>(),
                after_row.chars().skip(5).collect::<String>(),
                "{cols}x{rows} badge leaves status and clipped title columns fixed"
            );
        }
        assert_eq!(
            before
                .iter()
                .map(|row| sidebar(row))
                .filter(|row| row.contains("jump-title-"))
                .count(),
            after
                .iter()
                .map(|row| sidebar(row))
                .filter(|row| row.contains("jump-title-"))
                .count(),
            "{cols}x{rows} labels do not alter one/two-line row geometry"
        );
    }
}

#[test]
fn recent_activity_jump_labels_follow_visible_activity_order() {
    let mut hierarchy = jump_hierarchy(10);
    for session in &mut hierarchy.projects[0].workspaces[0].sessions {
        session.last_activity_unix_ms = Some(match session.id.0 {
            1 => 10,
            2 => 100,
            3 => 90,
            id => 80 - id,
        });
    }
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    let mut settings = ovrcr::tui::Settings::default();
    settings.sidebar_view = ovrcr::protocol::SidebarView::Recent;
    dashboard.install_settings(settings);

    ctrl_g(&mut dashboard);
    let rows = rendered(&dashboard, 120, 40);
    let newest = rows
        .iter()
        .map(|row| sidebar(row))
        .find(|row| row.contains("jump-title-02"))
        .expect("newest session row");
    let next = rows
        .iter()
        .map(|row| sidebar(row))
        .find(|row| row.contains("jump-title-03"))
        .expect("second-newest session row");
    assert!(
        newest.contains("[1]"),
        "activity leader gets first label: {newest}"
    );
    assert!(
        next.contains("[2]"),
        "next activity gets second label: {next}"
    );
    digit(&mut dashboard, '1');
    assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
}

#[test]
fn fifty_agents_label_first_nine_and_scroll_to_new_first_nine() {
    let mut dashboard = jump_dashboard_at(50, 60, 24);
    let before = rendered(&dashboard, 60, 24);
    let visible = (1..=50)
        .filter(|id| {
            before
                .iter()
                .any(|row| sidebar(row).contains(&format!(" - jump-title-{id:02}")))
        })
        .collect::<Vec<_>>();
    assert!(
        visible.len() < 10,
        "60×24 draws only part of fifty rows: {}",
        visible.len()
    );
    assert!(!visible.is_empty());

    ctrl_g(&mut dashboard);
    let after = rendered(&dashboard, 60, 24);
    for (ordinal, id) in visible.iter().enumerate() {
        let title = format!("jump-title-{id:02}");
        let row = after
            .iter()
            .map(|row| sidebar(row))
            .find(|row| row.contains(&title))
            .unwrap();
        if ordinal < 9 {
            let badge = format!("[{}]", ordinal + 1);
            assert!(row.contains(&badge), "visible Agent {id} receives {badge}");
        } else {
            assert!(!row.contains("["), "visible Agent {id} stays unlabelled");
        }
    }
    assert!(!after.iter().any(|row| row.contains("[10]")));

    for _ in 0..20 {
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 4,
                row: 11,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 60, 24),
        );
    }
    let scrolled = rendered(&dashboard, 60, 24);
    let later_visible = (1..=50)
        .filter(|id| {
            scrolled
                .iter()
                .any(|row| sidebar(row).contains(&format!("jump-title-{id:02}")))
        })
        .collect::<Vec<_>>();
    assert!(later_visible.first().is_some_and(|id| *id > 9));
    assert!(!scrolled.iter().any(|row| sidebar(row).contains("[1]")));

    ctrl_g(&mut dashboard);
    let held_again = rendered(&dashboard, 60, 24);
    for (ordinal, id) in later_visible.iter().enumerate() {
        let title = format!("jump-title-{id:02}");
        let row = held_again
            .iter()
            .map(|row| sidebar(row))
            .find(|row| row.contains(&title))
            .unwrap();
        if ordinal < 9 {
            assert!(row.contains(&format!("[{}]", ordinal + 1)), "{row}");
        } else {
            assert!(!row.contains("["), "{row}");
        }
    }
    digit(&mut dashboard, '1');
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(later_visible[0]))
    );
}

#[test]
fn ctrl_g_again_closes_without_selecting() {
    let mut dashboard = jump_dashboard(10, 40);
    ctrl_g(&mut dashboard);
    assert!(
        rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]")),
        "the picker labels the first Agent"
    );
    ctrl_g(&mut dashboard);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(
        !rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]")),
        "closing the picker without a choice hides every label"
    );
    assert!(dashboard.drain_outbox().is_empty());

    // Legacy hosts report Ctrl-g as BEL; the toggle and labels still work.
    dashboard.event_action(key(
        KeyCode::Char('\u{7}'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    assert!(
        rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]")),
        "BEL spelling opens the picker"
    );
    dashboard.event_action(key(
        KeyCode::Char('\u{7}'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    assert!(
        !rendered(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]")),
        "BEL spelling closes the picker"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
}

#[test]
fn captured_targets_stay_fixed_when_candidate_count_crosses_nine() {
    let mut dashboard = jump_dashboard(9, 40);
    ctrl_g(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        jump_hierarchy(10),
    )));
    let rows = rendered(&dashboard, 120, 40);
    let tenth = rows
        .iter()
        .map(|row| sidebar(row))
        .find(|row| row.contains("jump-title-10"))
        .expect("new candidate is visible");
    assert!(!tenth.contains("[10]") && !tenth.contains("[1]"));
    digit(&mut dashboard, '9');
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
fn invalidated_ninth_target_keeps_its_label_without_selecting_a_neighbor() {
    let mut dashboard = jump_dashboard(10, 40);
    ctrl_g(&mut dashboard);
    let mut changed = jump_hierarchy(10);
    let replacement = &mut changed.projects[0].workspaces[0].sessions[8];
    replacement.run = SessionRunId(99);
    replacement.title = Some("replacement-run".into());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(changed)));

    let rows = rendered(&dashboard, 120, 40);
    let replacement_row = rows
        .iter()
        .find(|row| row.contains("replacement-run"))
        .expect("replacement title remains in the list");
    assert!(!replacement_row.contains("[9]"));
    digit(&mut dashboard, '9');
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(dashboard.drain_outbox().is_empty());
    assert!(rendered_footer(&dashboard, 120).contains("Agent unavailable"));
}

#[test]
fn single_digit_badges_preserve_stacked_unicode_rows_and_clipped_metadata() {
    let mut hierarchy = jump_hierarchy(10);
    let first = &mut hierarchy.projects[0].workspaces[0].sessions[0];
    first.title = Some("界e\u{301}🙂-clipped-title-that-is-long".into());
    first.kind = SessionKind::Agent {
        name: "agent-label-that-does-not-fit-in-sidebar".into(),
    };
    first.label = "agent-label-that-does-not-fit-in-sidebar / model-that-clips".into();
    let mut dashboard = Dashboard::new(TerminalSize { rows: 40, cols: 60 });
    dashboard.install_area(Rect::new(0, 0, 60, 40));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    dashboard.install_screen(SessionId(1), &[]);

    let before = rendered(&dashboard, 60, 40);
    let before_title = before
        .iter()
        .map(|row| sidebar(row))
        .find(|row| row.contains(" - ") && row.contains("clipped-title"))
        .unwrap_or_else(|| panic!("Unicode title row is not visible:\n{}", before.join("\n")));
    assert!(
        before_title.contains("界")
            && before_title.contains("e\u{301}")
            && before_title.contains("🙂")
    );
    let before_metadata = before
        .iter()
        .map(|row| sidebar(row))
        .find(|row| row.contains("└ agent-label-that-d"))
        .expect("long Agent metadata wraps to its own row");

    ctrl_g(&mut dashboard);
    let after = rendered(&dashboard, 60, 40);
    let after_title = after
        .iter()
        .map(|row| sidebar(row))
        .find(|row| row.contains("[1]") && row.contains("clipped-title"))
        .expect("single-digit badge fits before the title");
    let after_metadata = after
        .iter()
        .map(|row| sidebar(row))
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
    assert!(!after.iter().any(|row| row.contains("[10]")));
}
