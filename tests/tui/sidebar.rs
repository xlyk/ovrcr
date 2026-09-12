//! Sidebar, agent-hook metadata, and whole-dashboard layout rendering.

use crate::*;

#[test]
fn dashboard_layout() {
    let mut dashboard = dashboard_fixture();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = (0..40)
        .map(|row| {
            (0..120)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        dashboard.visible_rows(),
        vec![
            ovrcr::tui::TreeRow::Project {
                name: "consigint".into()
            },
            ovrcr::tui::TreeRow::Workspace {
                project: "consigint".into(),
                name: "auth".into()
            },
            ovrcr::tui::TreeRow::Session { id: SessionId(5) },
            ovrcr::tui::TreeRow::Session { id: SessionId(1) },
            ovrcr::tui::TreeRow::Workspace {
                project: "consigint".into(),
                name: "lifecycle".into()
            },
            ovrcr::tui::TreeRow::Session { id: SessionId(4) },
            ovrcr::tui::TreeRow::Session { id: SessionId(2) },
            ovrcr::tui::TreeRow::Project {
                name: "spacelift-agent".into()
            },
            ovrcr::tui::TreeRow::Workspace {
                project: "spacelift-agent".into(),
                name: "progress".into()
            },
            ovrcr::tui::TreeRow::Session { id: SessionId(3) },
        ]
    );
    assert!(rendered[0].contains("OVRCR  agent runtime"));
    assert!(
        rendered
            .iter()
            .any(|row| row.contains("pid: 111  elapsed: 0m"))
    );
    assert!(!rendered.iter().any(|row| row.contains("ctx ")));
    assert!(rendered.iter().any(|row| row.contains("q Detach")));
    assert!(rendered.iter().any(|row| row.contains("? Help")));
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(39, 10)].symbol(), "│");
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    assert!(rendered[1].contains("pid: 111  elapsed: 0m"));
    assert!(rendered[2].contains("─"));
    assert!(buffer[(8, 7)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(0, 4)].symbol(), "▌");
    assert_eq!(buffer[(1, 4)].bg, Color::Rgb(49, 50, 68));

    dashboard.select_session(SessionId(2));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let selected_exited = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 1)].symbol())
        .collect::<String>();
    assert!(selected_exited.contains("pid: closed"));
}

#[test]
fn sidebar_glyphs_and_columns_match_the_reference_tree() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    let rule = |used: usize| "─".repeat(39 - used);
    // The title band is unchanged; mauve there is the one place it is not selection.
    assert_eq!(row(0).trim_end(), "󰚩 OVRCR  agent runtime");
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
    // Project section header: upper case, blue bold, rule to the edge, no chevron.
    assert_eq!(row(1), format!(" CONSIGINT {}", rule(11)));
    assert_eq!(buffer[(1, 1)].fg, Color::Rgb(137, 180, 250));
    assert!(buffer[(1, 1)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(20, 1)].fg, Color::Rgb(88, 91, 112));
    // Workspace: branch glyph, bold name.
    assert_eq!(row(2).trim_end(), "  󰘬 auth");
    assert_eq!(buffer[(2, 2)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(buffer[(4, 2)].fg, Color::Rgb(205, 214, 244));
    assert!(buffer[(4, 2)].modifier.contains(Modifier::BOLD));
    // Selected local shell: mauve bar, surface background, subtext glyph and name.
    assert_eq!(row(3).trim_end(), "▌    $ local");
    assert_eq!(buffer[(0, 3)].fg, Color::Rgb(203, 166, 247));
    for x in 0..39 {
        assert_eq!(buffer[(x, 3)].bg, Color::Rgb(49, 50, 68), "column {x}");
    }
    assert_eq!(buffer[(5, 3)].fg, Color::Rgb(166, 173, 200));
    assert_eq!(buffer[(7, 3)].fg, Color::Rgb(166, 173, 200));
    // Session: status glyph at column 5, regular-weight name, model right-aligned in
    // the provider colour, one trailing cell.
    assert_eq!(row(4), format!("     - review{}claude ", " ".repeat(19)));
    assert_eq!(buffer[(5, 4)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(buffer[(7, 4)].fg, Color::Rgb(205, 214, 244));
    assert!(!buffer[(7, 4)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(32, 4)].fg, Color::Rgb(250, 179, 135));
    assert_eq!(buffer[(4, 4)].bg, Color::Rgb(30, 30, 46));
    assert_eq!(row(5).trim_end(), "  󰘬 lifecycle");
    assert_eq!(row(6).trim_end(), "     $ local");
    assert_eq!(row(7), format!("     · implement{}claude ", " ".repeat(16)));
    assert!(buffer[(7, 7)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(33, 7)].fg, Color::Rgb(173, 127, 104));
    // Blank gap line before the next project, then its header.
    assert_eq!(row(8).trim_end(), "");
    assert_eq!(row(9), format!(" SPACELIFT-AGENT {}", rule(17)));
    assert_eq!(buffer[(1, 9)].fg, Color::Rgb(137, 180, 250));
    assert_eq!(row(10).trim_end(), "  󰘬 progress");
    assert_eq!(row(11).trim_end(), "     $ local");
    assert_eq!(row(12).trim_end(), "");
    assert_eq!(buffer[(39, 3)].symbol(), "│");

    // Selecting another session moves the bar and background with it.
    dashboard.select_session(SessionId(1));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 4)].symbol(), "▌");
    assert_eq!(buffer[(20, 4)].bg, Color::Rgb(49, 50, 68));
    assert_eq!(buffer[(0, 3)].symbol(), " ");
    assert_eq!(buffer[(20, 3)].bg, Color::Rgb(30, 30, 46));
    assert_eq!(buffer[(32, 4)].fg, Color::Rgb(250, 179, 135));

    // Selecting a workspace container by its name uses the same bar and background.
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 2)].symbol(), "▌");
    assert_eq!(buffer[(0, 2)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(20, 2)].bg, Color::Rgb(49, 50, 68));
    assert_eq!(buffer[(4, 2)].fg, Color::Rgb(205, 214, 244));
    assert_eq!(buffer[(0, 4)].symbol(), " ");
}

#[test]
fn unavailable_reporter_mutes_the_status_glyph() {
    use ovrcr::protocol::{
        ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, ReporterHealth,
        SampleQuality,
    };
    let mut dashboard = dashboard_fixture();
    let review = &mut dashboard.hierarchy.projects[1].workspaces[1].sessions[0];
    review.activity = AgentActivity::ResponseReady;
    review.agent = Some(AgentSnapshot {
        binding: AgentBinding {
            provider: AgentProvider::Claude,
            invocation: "invocation".into(),
            conversation: "conversation".into(),
            generation: 1,
        },
        activity: Some(ActivitySample {
            state: AgentActivity::ResponseReady,
            quality: SampleQuality::Observed,
            turn: None,
        }),
        metrics: None,
        health: HealthSample {
            state: ReporterHealth::Unavailable,
            reason: None,
        },
        activity_revision: 1,
        metrics_revision: 1,
        health_revision: 1,
    });
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    for (health, color) in [
        (ReporterHealth::Unavailable, Color::Rgb(108, 112, 134)),
        (ReporterHealth::Connected, Color::Rgb(148, 226, 213)),
    ] {
        dashboard.hierarchy.projects[1].workspaces[1].sessions[0]
            .agent
            .as_mut()
            .unwrap()
            .health
            .state = health;
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let cell = &terminal.backend().buffer()[(5, 4)];
        assert_eq!(cell.symbol(), "✓", "{health:?}");
        assert_eq!(cell.fg, color, "{health:?}");
    }
}

#[test]
fn sidebar_animates_only_explicitly_busy_sessions() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].activity = AgentActivity::Busy;
    for (time, marker) in [(0, "⠋"), (100, "⠙"), (900, "⠏"), (1000, "⠋")] {
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, time))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(5, 4)].symbol(), marker);
        assert_eq!(
            terminal.backend().buffer()[(5, 4)].fg,
            Color::Rgb(166, 227, 161)
        );
        assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "$");
    }
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].activity = AgentActivity::Idle;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 4)].symbol(), " ");
    // An exited process cannot remain busy, even if an old signal is retained.
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].activity = AgentActivity::Busy;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1200))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 7)].symbol(), "·");
}

#[test]
fn agent_hook_selected_metadata_reports_activity_and_lifecycle() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(1));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let metadata = |terminal: &Terminal<TestBackend>| {
        (40..120)
            .map(|column| terminal.backend().buffer()[(column, 1)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    };

    for (activity, expected) in [
        (
            AgentActivity::Unknown,
            "pid: 111  elapsed: 0m  agent unknown",
        ),
        (AgentActivity::Idle, "pid: 111  elapsed: 0m  agent idle"),
        (AgentActivity::Busy, "pid: 111  elapsed: 0m  agent busy"),
        (
            AgentActivity::WaitingInput,
            "pid: 111  elapsed: 0m  agent waiting input",
        ),
        (AgentActivity::Error, "pid: 111  elapsed: 0m  agent error"),
    ] {
        dashboard.hierarchy.projects[1].workspaces[1].sessions[0].activity = activity;
        dashboard.hierarchy.projects[1].workspaces[1].sessions[0].phase = SessionPhase::Running;
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        assert_eq!(metadata(&terminal), expected);
    }

    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].phase = SessionPhase::Paused;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(
        metadata(&terminal),
        "pid: 111  elapsed: 0m  agent error  paused"
    );

    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].pid = None;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(metadata(&terminal), "pid: closed  elapsed: 0m");
}

#[test]
fn agent_hook_sidebar_states_are_literal() {
    let mut dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let muted = Color::Rgb(108, 112, 134);
    for (activity, phase, glyph, color) in [
        (AgentActivity::Unknown, SessionPhase::Running, "-", muted),
        (AgentActivity::Idle, SessionPhase::Running, " ", muted),
        (
            AgentActivity::WaitingInput,
            SessionPhase::Running,
            "?",
            Color::Rgb(249, 226, 175),
        ),
        (
            AgentActivity::Error,
            SessionPhase::Running,
            "!",
            Color::Rgb(243, 139, 168),
        ),
        (
            AgentActivity::ResponseReady,
            SessionPhase::Running,
            "✓",
            Color::Rgb(148, 226, 213),
        ),
        (
            AgentActivity::Busy,
            SessionPhase::Paused,
            "P",
            Color::Rgb(166, 173, 200),
        ),
    ] {
        let review = &mut dashboard.hierarchy.projects[1].workspaces[1].sessions[0];
        review.activity = activity;
        review.phase = phase;
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let cell = &terminal.backend().buffer()[(5, 4)];
        assert_eq!(cell.symbol(), glyph, "{activity:?}");
        assert_eq!(cell.fg, color, "{activity:?}");
    }
    // Exited sessions show a muted dot whatever their last activity was.
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].activity = AgentActivity::Error;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 7)].symbol(), "·");
    assert_eq!(terminal.backend().buffer()[(5, 7)].fg, muted);
    // A local shell stays quiet until a hook reports real activity inside it.
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0].activity = AgentActivity::Error;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 11)].symbol(), "!");
    assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "$");
}

#[test]
fn agent_hook_summary_updates_drive_animation() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 900);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "$");

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        revision: dashboard.view_revision,
        bytes: b"PTY output\x1b]52;c;V0FJVElORw==\x1b\\".to_vec(),
    }));
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("PTY output")
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("V0FJVElORw==")
    );
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions[1].activity,
        AgentActivity::Unknown
    );
    dashboard.mode = ovrcr::tui::InputMode::Browse;

    let mut summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[1].clone();
    summary.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "⠙");

    summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[1].clone();
    summary.activity = AgentActivity::Idle;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions[1].activity,
        AgentActivity::Idle
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 200))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "$");
}

#[test]
fn selected_session_uses_a_soft_bar_on_one_line() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();

    for col in 0..39 {
        assert_eq!(buffer[(col, 4)].bg, Color::Rgb(49, 50, 68), "column {col}");
    }
    for row in [3, 5] {
        assert_eq!(buffer[(0, row)].bg, Color::Rgb(30, 30, 46));
    }
    let rendered = (3..=5)
        .map(|row| {
            (0..39)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rendered,
        [
            "     $ local",
            &format!("▌    - review{}claude", " ".repeat(19)),
            "  󰘬 lifecycle"
        ]
    );
    // The selected row keeps its own status and provider colours.
    assert_eq!(buffer[(0, 4)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(5, 4)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(buffer[(7, 4)].fg, Color::Rgb(205, 214, 244));
    assert_eq!(buffer[(32, 4)].fg, Color::Rgb(250, 179, 135));
}

#[test]
fn context_updates_leave_the_compact_sidebar_unchanged() {
    let mut dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let initial = terminal.backend().buffer().clone();

    // Fresh, stale, unknown and over-capacity reports still enter dashboard state,
    // but none restores the removed runtime/context row or moves the selection.
    for (used_tokens, capacity_tokens, now) in [
        (Some(25), Some(100), 1_000),
        (Some(25), Some(100), 301_000),
        (None, None, 1_000),
        (Some(101), Some(100), 1_000),
    ] {
        let mut summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[0].clone();
        let context = ContextUsageSnapshot {
            report: ContextUsageReport {
                source: ContextSource::Generic,
                model: None,
                conversation: None,
                used_tokens,
                capacity_tokens,
            },
            received_unix_ms: 1_000,
        };
        summary.context_usage = Some(context.clone());
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
            Box::new(summary),
        )));
        assert_eq!(
            dashboard.hierarchy.projects[1].workspaces[1].sessions[0].context_usage,
            Some(context)
        );
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, now))
            .unwrap();
        let buffer = terminal.backend().buffer();
        for row in 1..=16 {
            for col in 0..39 {
                assert_eq!(
                    buffer[(col, row)],
                    initial[(col, row)],
                    "cell ({col}, {row})"
                );
            }
            let text = (0..39)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>();
            assert!(!text.contains("run ") && !text.contains("ctx "), "{text}");
        }
        assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    }
}

#[test]
fn sidebar_clicks_map_one_line_per_session_and_skip_gap_lines() {
    let area = Rect::new(0, 0, 120, 40);
    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    for (row, id) in [(3, 5), (4, 1), (6, 4), (7, 2), (11, 3)] {
        let mut dashboard = dashboard_fixture();
        dashboard.mouse_action(click(7, row), area);
        assert_eq!(
            dashboard.focused_session(),
            Some(SessionId(id)),
            "row {row}"
        );
        // Anywhere on the row selects, including the right-aligned model.
        let mut dashboard = dashboard_fixture();
        dashboard.mouse_action(click(36, row), area);
        assert_eq!(dashboard.focused_session(), Some(SessionId(id)));
    }
    let mut workspace = dashboard_fixture();
    assert_eq!(
        workspace.mouse_action(click(2, 5), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        workspace
            .collapsed_workspaces
            .contains(&("consigint".into(), "lifecycle".into()))
    );
    let mut project = dashboard_fixture();
    assert_eq!(
        project.mouse_action(click(0, 9), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(project.collapsed_projects.contains("spacelift-agent"));
    for row in [8, 13] {
        let mut blank = dashboard_fixture();
        assert_eq!(
            blank.mouse_action(click(7, row), area),
            ovrcr::tui::DashboardAction::None,
            "row {row}"
        );
        assert_eq!(blank.focused_session(), Some(SessionId(1)));
    }
}

#[test]
fn sidebar_uses_agent_label_prefixes_and_emphasizes_tree_names() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].label = "claude / sonnet-4".into();
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].label = "unknown / model".into();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();

    assert!(buffer[(4, 2)].modifier.contains(Modifier::BOLD));
    assert!(buffer[(4, 5)].modifier.contains(Modifier::BOLD));
    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    assert_eq!(row(4), format!("     - review{}sonnet-4 ", " ".repeat(17)));
    assert_eq!(buffer[(30, 4)].fg, Color::Rgb(250, 179, 135));
    assert_eq!(row(7), format!("     · implement{}model ", " ".repeat(17)));
    assert_eq!(buffer[(33, 7)].fg, Color::Rgb(144, 150, 175));
}

#[test]
fn long_sidebar_names_clip_to_one_screen_line() {
    let mut dashboard = dashboard_fixture();
    dashboard.hierarchy.projects[1].name = "consigint-界界界界界界界界界界界界界界".into();
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].name =
        "review-a-very-long-session-name-that-must-not-wrap".into();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let project = (0..39)
        .map(|column| buffer[(column, 1)].symbol())
        .collect::<String>();
    assert!(project.trim_end().ends_with('…'));
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    let review = (0..39)
        .map(|column| buffer[(column, 4)].symbol())
        .collect::<String>();
    assert_eq!(review, "▌    - review-a-very-long-ses…  claude ");

    // Too narrow for a twelve-cell name beside the model: the model goes first.
    let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let review = (0..24)
        .map(|column| buffer[(column, 4)].symbol())
        .collect::<String>();
    assert_eq!(review, "▌    - review-a-very-lo…");
    assert_eq!(buffer[(24, 4)].symbol(), "│");
}

#[test]
fn dashboard_geometry_remains_drawable_at_target_and_tiny_sizes() {
    for (width, height, terminal_width, terminal_height) in
        [(180, 72, 140, 68), (120, 40, 80, 36), (80, 24, 40, 20)]
    {
        assert_eq!(
            ovrcr::tui::actual_drawn_inner_rect(Rect::new(0, 0, width, height)),
            Rect::new(40, 3, terminal_width, terminal_height)
        );
        let dashboard = dashboard_fixture();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
    }

    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
}

#[test]
fn collapse_and_mouse_hits_use_current_visible_tree() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.toggle_selected_group();
    assert!(
        !dashboard
            .visible_rows()
            .contains(&ovrcr::tui::TreeRow::Session { id: SessionId(5) })
    );
    dashboard.toggle_selected_group();
    let mouse = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let action = dashboard.mouse_action(mouse(2, 2), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert!(
        !dashboard
            .visible_rows()
            .contains(&ovrcr::tui::TreeRow::Session { id: SessionId(5) })
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let row = |terminal: &Terminal<TestBackend>, y| {
        (0..39)
            .map(|column| terminal.backend().buffer()[(column, y)].symbol())
            .collect::<String>()
    };
    // A folded workspace shows how many sessions it hides.
    assert_eq!(row(&terminal, 2), format!("  󰘬 auth{}▸ 2 ", " ".repeat(27)));
    let action = dashboard.mouse_action(mouse(2, 2), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let action = dashboard.mouse_action(mouse(4, 4), Rect::new(0, 0, 120, 40));
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let action = dashboard.mouse_action(mouse(0, 1), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert!(dashboard.collapsed_projects.contains("consigint"));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    // A folded project shows its workspace count and keeps its rule.
    assert_eq!(
        row(&terminal, 1),
        format!(" CONSIGINT {} ▸ 2 ws ", "─".repeat(20))
    );
    assert_eq!(row(&terminal, 2).trim_end(), "");
    assert_eq!(
        row(&terminal, 3),
        format!(" SPACELIFT-AGENT {}", "─".repeat(22))
    );
    let action = dashboard.mouse_action(
        MouseEvent {
            column: 60,
            ..mouse(4, 6)
        },
        Rect::new(0, 0, 120, 40),
    );
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.mouse_action(mouse(4, 5), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
}

#[test]
fn sidebar_labels_select_without_toggling_and_work_in_terminal_mode() {
    let area = Rect::new(0, 0, 120, 40);
    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };

    let mut project = dashboard_fixture();
    project.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        project.mouse_action(click(5, 1), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(project.focused_session().is_none());
    assert!(!project.collapsed_projects.contains("consigint"));

    let mut project_disclosure = dashboard_fixture();
    project_disclosure.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        project_disclosure.mouse_action(click(0, 1), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(project_disclosure.collapsed_projects.contains("consigint"));
    assert_eq!(project_disclosure.focused_session(), Some(SessionId(1)));

    let mut workspace = dashboard_fixture();
    workspace.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        workspace.mouse_action(click(5, 2), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(workspace.focused_session().is_none());
    assert!(workspace.collapsed_workspaces.is_empty());

    let mut session = dashboard_fixture();
    session.mode = ovrcr::tui::InputMode::Terminal;
    assert!(matches!(
        session.mouse_action(click(6, 3), area),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert_eq!(session.focused_session(), Some(SessionId(5)));
}

#[test]
fn sidebar_container_click_keeps_focus_on_another_assigned_pane() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let split = dashboard
        .view_request(area, 60)
        .unwrap()
        .expect("split should request both panes");
    acknowledge_all_view_targets(&mut dashboard, split);
    assert!(dashboard.focus_pane(0));
    let focused = dashboard
        .view_request(area, 61)
        .unwrap()
        .expect("focus should request both panes");
    acknowledge_all_view_targets(&mut dashboard, focused);
    let retained = dashboard.panes[1].session.expect("second pane session");

    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_pane, 1);
    assert_eq!(dashboard.focused_session(), Some(retained));

    let request = dashboard
        .view_request(area, 62)
        .unwrap()
        .expect("container selection should retain the other pane view");
    let Request::SetView { view } = request.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.panes.len(), 1);
    assert_eq!(view.focused, Some(retained));
    deliver_all_view_screens(&mut dashboard, request.request_id, &view);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[dashboard.focused_pane].ready);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);

    dashboard.key(KeyCode::Char(' '));
    let menu = palette_text(&dashboard);
    assert!(menu.contains("w  Workspace"), "{menu}");
    assert!(!menu.contains("t  Terminal"), "{menu}");
    dashboard.key(KeyCode::Esc);
    assert_eq!(
        dashboard.key(KeyCode::Char('p')),
        ovrcr::tui::DashboardAction::Redraw
    );

    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(retained));
    assert!(dashboard.view_request(area, 63).unwrap().is_none());

    assert_eq!(
        dashboard.key(KeyCode::Char('n')),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Tab);
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("container-target".into()));
    dashboard.key(KeyCode::Tab);
    let ovrcr::tui::DashboardAction::Request(create) = dashboard.key(KeyCode::Enter) else {
        panic!("selected workspace should keep the create-terminal action");
    };
    let Request::CreateSession(create) = create.request else {
        panic!("expected CreateSession");
    };
    assert_eq!(create.project, "consigint");
    assert_eq!(create.workspace, "auth");
}

#[test]
fn empty_pane_click_preserves_the_assigned_wire_focus() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let split = dashboard
        .view_request(area, 70)
        .unwrap()
        .expect("split should request both panes");
    acknowledge_all_view_targets(&mut dashboard, split);
    assert!(dashboard.focus_pane(0));
    let focused = dashboard
        .view_request(area, 71)
        .unwrap()
        .expect("focus should request both panes");
    acknowledge_all_view_targets(&mut dashboard, focused);

    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    let selected = dashboard
        .view_request(area, 72)
        .unwrap()
        .expect("container selection should retain one assigned pane");
    acknowledge_all_view_targets(&mut dashboard, selected);
    let retained_pane = dashboard.focused_pane;
    let retained = dashboard.focused_session().expect("retained wire focus");
    let empty = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index != retained_pane)
        .expect("empty pane")
        .terminal;

    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: empty.x + 2,
                row: empty.y + 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_pane, retained_pane);
    assert_eq!(dashboard.focused_session(), Some(retained));
    assert!(dashboard.view_request(area, 73).unwrap().is_none());

    let assigned = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == retained_pane)
        .expect("assigned pane")
        .terminal;
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: assigned.x + 2,
                row: assigned.y + 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
}

#[test]
fn empty_workspace_selection_does_not_cover_a_retained_terminal() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.hierarchy.projects[1]
        .workspaces
        .push(WorkspaceSummary {
            project: "consigint".into(),
            name: "empty".into(),
            path: PathBuf::from("/tmp/empty"),
            sessions: Vec::new(),
        });
    assert!(dashboard.split_pane());
    let split = dashboard
        .view_request(area, 80)
        .unwrap()
        .expect("split should request both panes");
    acknowledge_all_view_targets(&mut dashboard, split);

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let empty_row = (0..40)
        .find(|row| {
            (0..39)
                .map(|column| terminal.backend().buffer()[(column, *row)].symbol())
                .collect::<String>()
                .contains("empty")
        })
        .expect("empty workspace row");
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: empty_row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    let selected = dashboard
        .view_request(area, 81)
        .unwrap()
        .expect("empty workspace selection should retain one pane");
    acknowledge_all_view_targets(&mut dashboard, selected);
    let retained_rect = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == dashboard.focused_pane)
        .expect("retained pane")
        .terminal;
    dashboard.panes[dashboard.focused_pane]
        .parser
        .process(b"RETAINED_TERMINAL");

    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let retained = (retained_rect.x..retained_rect.right())
        .map(|column| terminal.backend().buffer()[(column, retained_rect.y)].symbol())
        .collect::<String>();
    assert!(retained.contains("RETAINED_TERMINAL"), "{retained}");
}

#[test]
fn fifty_session_selection_scrolls_tree_and_mouse_hits_viewport() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 34, cols: 88 });
    dashboard.outer_area = Rect::new(0, 0, 120, 40);
    dashboard.hierarchy = HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "project".into(),
            workspaces: vec![WorkspaceSummary {
                project: "project".into(),
                name: "workspace".into(),
                path: PathBuf::from("/tmp/workspace"),
                sessions: (1..=50)
                    .map(|id| SessionSummary {
                        id: SessionId(id),
                        project: "project".into(),
                        workspace: "workspace".into(),
                        name: format!("session-{id}"),
                        label: "sh".into(),
                        pid: Some(id as u32),
                        started_unix_ms: 0,
                        phase: SessionPhase::Running,
                        activity: AgentActivity::Unknown,
                        context_usage: None,
                        agent: None,
                        agent_epoch: 0,
                        unread: None,
                    })
                    .collect(),
            }],
        }],
    };
    for _ in 0..50 {
        dashboard.move_selection(1);
    }
    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let rendered = (0..40)
        .map(|row| {
            (0..39)
                .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rendered[36],
        format!("▌    - session-50{}sh ", " ".repeat(19))
    );
    assert_eq!(rendered[37].trim_end(), "");
    for (index, row) in (36..=37).enumerate() {
        let action = dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        );
        if index == 0 {
            assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
        } else {
            assert!(matches!(
                action,
                ovrcr::tui::DashboardAction::Redraw | ovrcr::tui::DashboardAction::None
            ));
        }
        assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    }

    for _ in 0..50 {
        dashboard.move_selection(-1);
    }
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    for _ in 0..3 {
        assert_eq!(
            dashboard.mouse_action(
                MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 4,
                    row: 10,
                    modifiers: KeyModifiers::NONE,
                },
                Rect::new(0, 0, 120, 40),
            ),
            ovrcr::tui::DashboardAction::Redraw
        );
    }
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    for (row, expected) in [(1, "session-4"), (2, "session-5"), (3, "session-6")] {
        let text = (0..39)
            .map(|col| terminal.backend().buffer()[(col, row)].symbol())
            .collect::<String>();
        assert_eq!(text[..17].trim_end(), format!("     - {expected}"));
    }
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert!(matches!(
        action,
        ovrcr::tui::DashboardAction::Request(_) | ovrcr::tui::DashboardAction::Redraw
    ));
    assert_eq!(dashboard.focused_session(), Some(SessionId(6)));
}

#[test]
fn dashboard_inner_rect_keeps_last_pty_row_and_cursor_visible() {
    let inner = ovrcr::tui::actual_drawn_inner_rect(Rect::new(0, 0, 120, 40));
    assert_eq!(inner.height, 36);
    assert_eq!(inner.width, 80);
    let mut parser = vt100::Parser::new(inner.height, inner.width, 0);
    parser.process(b"\x1b[36;1HBOTTOM_MARKER");
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| {
            ovrcr::tui::render_terminal(frame, inner, parser.screen(), true);
        })
        .unwrap();
    let bottom = (inner.x..inner.right())
        .map(|column| terminal.backend().buffer()[(column, inner.bottom() - 1)].symbol())
        .collect::<String>();
    assert!(bottom.contains("BOTTOM_MARKER"));
    assert_eq!(terminal.backend().cursor_position().y, inner.bottom() - 1);
}

#[test]
fn shrinking_dashboard_keeps_selected_tree_row_visible() {
    let mut dashboard = dashboard_fixture();
    let workspace = &mut dashboard.hierarchy.projects[0].workspaces[0];
    for id in 6..=50 {
        workspace.sessions.push(SessionSummary {
            id: SessionId(id),
            project: "consigint".into(),
            workspace: "auth".into(),
            name: format!("session-{id}"),
            label: "sh".into(),
            pid: Some(id as u32),
            started_unix_ms: 0,
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent: None,
            agent_epoch: 0,
            unread: None,
        });
    }
    dashboard.select_session(SessionId(50));
    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    let resize = dashboard
        .view_request(outer_area_for_pane(TerminalSize { rows: 20, cols: 40 }), 99)
        .unwrap()
        .expect("resize should request a view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Screen {
            session: SessionId(50),
            revision: dashboard.view_revision,
            size: TerminalSize { rows: 20, cols: 40 },
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Ok,
    });
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let rendered = (0..24)
        .map(|row| {
            (0..39)
                .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("session-50"));
}

#[test]
fn ux_live_metadata_is_readable_and_preserves_selection() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    // The first local shell is unselected on row 3; its name must stay readable.
    let color = buffer[(8, 3)].fg;
    let Color::Rgb(r, g, b) = color else {
        panic!("expected RGB metadata: {color:?}")
    };
    assert!(
        r >= 160 && g >= 160 && b >= 160,
        "faint metadata: {color:?}"
    );
    // The selected review uses the soft selection background.
    assert_eq!(buffer[(8, 4)].bg, Color::Rgb(49, 50, 68));
}
