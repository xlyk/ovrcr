//! Sidebar, agent-hook metadata, and whole-dashboard layout rendering.

use crate::*;

fn review_session(hierarchy: &HierarchySnapshot) -> &ovrcr::session::SessionSummary {
    &hierarchy.projects[1].workspaces[1].sessions[0]
}

fn review_session_mut(hierarchy: &mut HierarchySnapshot) -> &mut ovrcr::session::SessionSummary {
    &mut hierarchy.projects[1].workspaces[1].sessions[0]
}

fn implement_session_mut(hierarchy: &mut HierarchySnapshot) -> &mut ovrcr::session::SessionSummary {
    &mut hierarchy.projects[1].workspaces[0].sessions[0]
}

fn sidebar_row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    (0..39)
        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
        .collect()
}

fn enter_terminal(dashboard: &mut Dashboard) {
    let _ = dashboard.key(KeyCode::Enter);
}

fn hover(dashboard: &mut Dashboard, column: u16, row: u16, area: Rect) {
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
}

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
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(buffer[(0, 4)].symbol(), "▌");
    assert!(rendered[0].contains("OVRCR  agent runtime"));
    assert!(
        rendered
            .iter()
            .any(|row| row.contains("pid: 111  elapsed: 0m"))
    );
    assert!(!rendered.iter().any(|row| row.contains("ctx ")));
    assert!(rendered.iter().any(|row| row.contains("Space Menu")));
    assert!(rendered.iter().any(|row| row.contains("? Help")));
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(39, 10)].symbol(), "│");
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    assert!(rendered[1].contains("pid: 111  elapsed: 0m"));
    assert!(
        rendered[2].contains("review"),
        "the pane title replaces the decorative rule"
    );
    assert!(buffer[(8, 7)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(0, 4)].symbol(), "▌");
    assert_eq!(buffer[(1, 4)].bg, Color::Rgb(49, 50, 68));

    dashboard.install_focus(SessionId(2));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let selected_exited = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 1)].symbol())
        .collect::<String>();
    assert!(selected_exited.contains("pid: closed"));
    assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
}

#[test]
fn sidebar_glyphs_and_columns_match_the_reference_tree() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(5));
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
    // Session: status glyph at column 5, regular-weight name, agent right-aligned in
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
    dashboard.install_focus(SessionId(1));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 4)].symbol(), "▌");
    assert_eq!(buffer[(20, 4)].bg, Color::Rgb(49, 50, 68));
    assert_eq!(buffer[(0, 3)].symbol(), " ");
    assert_eq!(buffer[(20, 3)].bg, Color::Rgb(30, 30, 46));
    assert_eq!(buffer[(32, 4)].fg, Color::Rgb(250, 179, 135));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));

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
    let mut review = review_session(&fixture_hierarchy()).clone();
    review.activity = AgentActivity::ResponseReady;
    review.unread = Some(ovrcr::protocol::ReadyObservation {
        binding: ovrcr::protocol::AgentBinding {
            provider: ovrcr::protocol::AgentProvider::Claude,
            invocation: "invocation".into(),
            conversation: "conversation".into(),
            generation: 1,
        },
        turn: Some("turn".into()),
        activity_revision: 1,
    });
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
        input_requests: Vec::new(),
        input_revision: 0,
    });
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    for (health, color) in [
        (ReporterHealth::Unavailable, Color::Rgb(108, 112, 134)),
        (ReporterHealth::Connected, Color::Rgb(148, 226, 213)),
    ] {
        review.agent.as_mut().unwrap().health.state = health;
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
            Box::new(review.clone()),
        )));
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
    dashboard.install_focus(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut review = review_session(&fixture_hierarchy()).clone();
    review.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        review.clone(),
    ))));
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
    review.activity = AgentActivity::Idle;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        review,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 4)].symbol(), " ");
    // An exited process cannot remain busy, even if an old signal is retained.
    let mut implement = fixture_hierarchy().projects[1].workspaces[0].sessions[0].clone();
    implement.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        implement,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1200))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 7)].symbol(), "·");
}

#[test]
fn agent_hook_selected_metadata_reports_activity_and_lifecycle() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(1));
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
        let mut review = review_session(&fixture_hierarchy()).clone();
        review.activity = activity;
        review.phase = SessionPhase::Running;
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
            Box::new(review),
        )));
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        assert_eq!(metadata(&terminal), format!("{expected}  review"));
    }

    let mut review = review_session(&fixture_hierarchy()).clone();
    review.activity = AgentActivity::Error;
    review.phase = SessionPhase::Paused;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        review.clone(),
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(
        metadata(&terminal),
        "pid: 111  elapsed: 0m  agent error  paused  review"
    );

    review.phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    review.pid = None;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        review,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(metadata(&terminal), "pid: closed  elapsed: 0m  review");
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
            " ",
            muted,
        ),
        (
            AgentActivity::Busy,
            SessionPhase::Paused,
            "P",
            Color::Rgb(166, 173, 200),
        ),
    ] {
        let mut review = review_session(&fixture_hierarchy()).clone();
        review.activity = activity;
        review.phase = phase;
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
            Box::new(review),
        )));
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let cell = &terminal.backend().buffer()[(5, 4)];
        assert_eq!(cell.symbol(), glyph, "{activity:?}");
        assert_eq!(cell.fg, color, "{activity:?}");
    }
    // Exited sessions show a muted dot whatever their last activity was.
    let mut implement = fixture_hierarchy().projects[1].workspaces[0].sessions[0].clone();
    implement.activity = AgentActivity::Error;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        implement,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 7)].symbol(), "·");
    assert_eq!(terminal.backend().buffer()[(5, 7)].fg, muted);
    // A local shell stays quiet until a hook reports real activity inside it.
    let mut progress = fixture_hierarchy().projects[0].workspaces[0].sessions[0].clone();
    progress.activity = AgentActivity::Error;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        progress,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 11)].symbol(), "!");
    assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "$");
}

#[test]
fn reviewed_ready_hides_the_checkmark_and_the_sidebar_draws_no_unread_circle() {
    use ovrcr::protocol::{
        ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, ReadyObservation,
        ReporterHealth, SampleQuality,
    };
    let binding = AgentBinding {
        provider: AgentProvider::Pi,
        invocation: "invocation".into(),
        conversation: "conversation".into(),
        generation: 1,
    };
    let snapshot = |turn: &str| AgentSnapshot {
        binding: binding.clone(),
        activity: Some(ActivitySample {
            state: AgentActivity::ResponseReady,
            quality: SampleQuality::Confirmed,
            turn: Some(turn.into()),
        }),
        metrics: None,
        health: HealthSample {
            state: ReporterHealth::Connected,
            reason: None,
        },
        activity_revision: 2,
        metrics_revision: 0,
        health_revision: 0,
        input_requests: Vec::new(),
        input_revision: 0,
    };
    let observation = |turn: &str| ReadyObservation {
        binding: binding.clone(),
        turn: Some(turn.into()),
        activity_revision: 2,
    };
    let publish = |dashboard: &mut Dashboard, session: SessionSummary| {
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
            Box::new(session),
        )));
    };
    let mut dashboard = dashboard_fixture();
    let mut review = review_session(&fixture_hierarchy()).clone();
    review.activity = AgentActivity::ResponseReady;
    review.agent = Some(snapshot("one"));
    review.unread = Some(observation("one"));
    publish(&mut dashboard, review.clone());

    let mut shell = fixture_hierarchy().projects[0].workspaces[0].sessions[0].clone();
    shell.unread = Some(observation("shell"));
    publish(&mut dashboard, shell);

    let mut other = fixture_hierarchy().projects[1].workspaces[1].sessions[1].clone();
    other.name = "other".into();
    other.label = "codex".into();
    other.activity = AgentActivity::ResponseReady;
    other.agent = Some(snapshot("two"));
    other.unread = Some(observation("two"));
    publish(&mut dashboard, other);

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let draw = |dashboard: &Dashboard, terminal: &mut Terminal<TestBackend>| {
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, dashboard, 0))
            .unwrap();
    };
    let sidebar = |terminal: &Terminal<TestBackend>| {
        (0..20)
            .map(|row| sidebar_row(terminal, row))
            .collect::<Vec<_>>()
    };
    let glyph = |lines: &[String], name: &str| {
        lines
            .iter()
            .find(|row| row.contains(name))
            .unwrap_or_else(|| panic!("missing {name}: {lines:?}"))
            .chars()
            .nth(5)
            .unwrap()
    };
    let header = |terminal: &Terminal<TestBackend>| {
        (40..120)
            .map(|column| terminal.backend().buffer()[(column, 1)].symbol())
            .collect::<String>()
    };

    draw(&dashboard, &mut terminal);
    let lines = sidebar(&terminal);
    assert!(
        lines.iter().all(|row| !row.contains('\u{25cf}')),
        "unread circle is still drawn: {lines:?}"
    );
    assert_eq!(glyph(&lines, "review"), '\u{2713}');
    assert_eq!(glyph(&lines, "other"), '\u{2713}');
    let header_text = header(&terminal);
    assert!(
        header_text.contains("Unread"),
        "header should say Unread: {header_text}"
    );

    dashboard.install_focus(SessionId(1));
    draw(&dashboard, &mut terminal);
    assert_eq!(glyph(&sidebar(&terminal), "review"), '\u{2713}');
    assert!(header(&terminal).contains("Unread"));

    let mut split_dashboard = dashboard_fixture();
    publish(&mut split_dashboard, review.clone());
    assert!(split_dashboard.split_pane());
    draw(&split_dashboard, &mut terminal);
    let split: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        split.contains("Unread"),
        "split header should say Unread: {split}"
    );
    assert!(
        sidebar(&terminal)
            .iter()
            .all(|row| !row.contains('\u{25cf}')),
        "split drawing still has an unread circle"
    );

    review.unread = None;
    publish(&mut dashboard, review);
    draw(&dashboard, &mut terminal);
    let lines = sidebar(&terminal);
    assert_ne!(glyph(&lines, "review"), '\u{2713}');
    assert_eq!(glyph(&lines, "other"), '\u{2713}');
    let reviewed = header(&terminal);
    assert!(
        !reviewed.contains("Unread"),
        "review should clear the Unread header: {reviewed}"
    );
    assert!(
        reviewed.contains("response ready"),
        "reviewed activity stays ready: {reviewed}"
    );
}

#[test]
fn agent_hook_summary_updates_drive_animation() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(5));
    dashboard.install_screen(SessionId(5), &[]);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "$");

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b"PTY output\x1b]52;c;V0FJVElORw==\x1b\\".to_vec(),
    }));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let pane = (0..40)
        .map(|row| {
            (40..120)
                .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("");
    assert!(pane.contains("PTY output"));
    assert!(!pane.contains("V0FJVElORw=="));
    enter_terminal(&mut dashboard);
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );

    let mut summary = fixture_hierarchy().projects[1].workspaces[1].sessions[1].clone();
    summary.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(5, 3)].symbol(), "⠙");

    let mut summary = fixture_hierarchy().projects[1].workspaces[1].sessions[1].clone();
    summary.activity = AgentActivity::Idle;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
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
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
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
        let mut summary = review_session(&fixture_hierarchy()).clone();
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
        summary.context_usage = Some(context);
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
            Box::new(summary),
        )));
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
        // Anywhere on the row selects, including the right-aligned agent.
        let mut dashboard = dashboard_fixture();
        dashboard.mouse_action(click(36, row), area);
        assert_eq!(dashboard.focused_session(), Some(SessionId(id)));
    }
    let mut workspace = dashboard_fixture();
    assert_eq!(
        workspace.mouse_action(click(2, 5), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &workspace, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 5).contains("▸"),
        "workspace click on the disclosure column should fold the group"
    );
    let mut project = dashboard_fixture();
    assert_eq!(
        project.mouse_action(click(0, 9), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &project, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 9).contains("▸"),
        "project disclosure should fold the section"
    );
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
    dashboard.install_focus(SessionId(5));
    let mut hierarchy = fixture_hierarchy();
    review_session_mut(&mut hierarchy).label = "claude / sonnet-4".into();
    implement_session_mut(&mut hierarchy).label = "unknown / model".into();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();

    assert!(buffer[(4, 2)].modifier.contains(Modifier::BOLD));
    assert!(buffer[(4, 5)].modifier.contains(Modifier::BOLD));
    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    assert_eq!(row(4), "     - review          claude:sonnet-4 ");
    assert_eq!(buffer[(23, 4)].fg, Color::Rgb(250, 179, 135));
    assert_eq!(row(7), "     · implement         unknown:model ");
    assert_eq!(buffer[(25, 7)].fg, Color::Rgb(144, 150, 175));
}

#[test]
fn sidebar_shows_the_agent_and_current_model_on_one_row() {
    use ovrcr::protocol::{
        ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, ContextSample, HealthSample,
        Measurement, MetricsSample, MetricsSnapshot, ReporterHealth, SampleQuality, UsageCoverage,
        UsageScope, UsageTotals,
    };
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    review_session_mut(&mut hierarchy).label = "pi / grok-4.7".into();
    review_session_mut(&mut hierarchy).phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    implement_session_mut(&mut hierarchy).label = "grok / grok-4.6".into();
    implement_session_mut(&mut hierarchy).phase = SessionPhase::Running;
    implement_session_mut(&mut hierarchy).agent = Some(AgentSnapshot {
        binding: AgentBinding {
            provider: AgentProvider::Grok,
            invocation: "invocation".into(),
            conversation: "conversation".into(),
            generation: 1,
        },
        activity: Some(ActivitySample {
            state: AgentActivity::Idle,
            quality: SampleQuality::Observed,
            turn: None,
        }),
        metrics: Some(MetricsSnapshot {
            sample: MetricsSample {
                model: Some("grok-4.7".into()),
                context: Measurement {
                    value: ContextSample {
                        used_tokens: None,
                        capacity_tokens: None,
                        quality: SampleQuality::Observed,
                    },
                    source: "fixture".into(),
                },
                usage: Measurement {
                    value: UsageTotals {
                        scope: UsageScope::Conversation,
                        coverage: UsageCoverage::Partial,
                        input_tokens: None,
                        output_tokens: None,
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                        reasoning_output_tokens: None,
                    },
                    source: "fixture".into(),
                },
                cost: Measurement {
                    value: None,
                    source: "fixture".into(),
                },
            },
            received_unix_ms: 0,
            context_received_unix_ms: 0,
            usage_received_unix_ms: 0,
            cost_received_unix_ms: 0,
        }),
        health: HealthSample {
            state: ReporterHealth::Connected,
            reason: None,
        },
        activity_revision: 1,
        metrics_revision: 1,
        health_revision: 1,
        input_requests: Vec::new(),
        input_revision: 0,
    });
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let row = |name: &str| {
        (0..20)
            .map(|y| sidebar_row(&terminal, y))
            .find(|row| row.contains(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    let review = row("review");
    let implement = row("implement");
    assert!(
        review.trim_end().ends_with("pi:grok-4.7"),
        "stored suffix should show: {review:?}"
    );
    assert!(
        implement.trim_end().ends_with("grok:4.7"),
        "live model should beat the stored suffix and drop the repeat: {implement:?}"
    );
    let buffer = terminal.backend().buffer();
    let review_y = (0..20)
        .find(|y| sidebar_row(&terminal, *y).contains("review"))
        .unwrap();
    let label_x = u16::try_from(review.find("pi:grok-4.7").unwrap()).unwrap();
    assert_eq!(
        buffer[(label_x, review_y)].fg,
        Color::Rgb(142, 118, 177),
        "an exited agent name fades"
    );
    assert_eq!(
        buffer[(label_x + 3, review_y)].fg,
        Color::Rgb(108, 112, 134),
        "model stays quiet"
    );
    let implement_y = (0..20)
        .find(|y| sidebar_row(&terminal, *y).contains("implement"))
        .unwrap();
    let implement_x = u16::try_from(implement.find("grok:4.7").unwrap()).unwrap();
    assert_eq!(
        buffer[(implement_x, implement_y)].fg,
        Color::Rgb(137, 180, 250)
    );

    let mut narrow = Terminal::new(TestBackend::new(60, 24)).unwrap();
    let mut hierarchy = fixture_hierarchy();
    review_session_mut(&mut hierarchy).name = "review-a-very-long-session-name".into();
    review_session_mut(&mut hierarchy).label = "claude / opus-4.5-with-a-long-suffix".into();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    narrow
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let narrow_row = |y| {
        (0..29)
            .map(|column| narrow.backend().buffer()[(column, y)].symbol())
            .collect::<String>()
    };
    assert_eq!(
        narrow_row(4),
        "▌    - review-a-very-long-se…",
        "a label that cannot share the line leaves the whole line to the name"
    );
    assert_eq!(
        narrow_row(5),
        "▌      └ claude:opus-4.5-wi… ",
        "the agent and model take the next line, clipped like any other text"
    );
}

#[test]
fn narrow_session_row_keeps_its_model_on_a_second_line() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    review_session_mut(&mut hierarchy).label = "pi / grok-4.7".into();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    let area = Rect::new(0, 0, 60, 24);
    dashboard.install_area(area);
    let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let row = |terminal: &Terminal<TestBackend>, y| {
        (0..29)
            .map(|column| terminal.backend().buffer()[(column, y)].symbol())
            .collect::<String>()
    };
    // Twenty-nine cells cannot hold a twelve-cell name beside `pi:grok-4.7`, so the
    // model is not dropped: the label moves under the name. The shell above stays one line.
    assert_eq!(row(&terminal, 3), "     $ local                 ");
    assert_eq!(row(&terminal, 4), "▌    - review                ");
    assert_eq!(row(&terminal, 5), "▌      └ pi:grok-4.7         ");
    assert_eq!(row(&terminal, 6).trim_end(), "  󰘬 lifecycle");
    // Stacking is all or nothing: `claude` alone would fit, but it stacks with the column.
    assert_eq!(row(&terminal, 7), "     $ local                 ");
    assert_eq!(row(&terminal, 8), "     · implement             ");
    assert_eq!(row(&terminal, 9), "       └ claude              ");
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(9, 5)].fg,
        Color::Rgb(203, 166, 247),
        "provider colour"
    );
    assert_eq!(
        buffer[(11, 5)].fg,
        Color::Rgb(108, 112, 134),
        "model stays quiet"
    );
    for y in [4, 5] {
        assert_eq!(buffer[(0, y)].fg, Color::Rgb(203, 166, 247));
        for x in 0..29 {
            assert_eq!(buffer[(x, y)].bg, Color::Rgb(49, 50, 68), "({x}, {y})");
        }
    }
    assert_eq!(buffer[(0, 6)].bg, Color::Rgb(30, 30, 46));

    // Keys treat both lines as one session: `j` lands on the next row, `k` returns.
    assert_eq!(dashboard.key(KeyCode::Char('j')), DashboardAction::Redraw);
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(6), "lifecycle workspace");
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));

    // The mouse treats the label line as the same session, not a new one.
    dashboard.install_focus(SessionId(5));
    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    dashboard.mouse_action(click(9, 5), area);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    hover(&mut dashboard, 27, 5, area);
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(
        row(&terminal, 4),
        "▌    - review             [x]",
        "hovering the label line marks the session's first line"
    );
    assert_eq!(row(&terminal, 5), "▌      └ pi:grok-4.7         ");
    // The mark itself is only on the first line; its column on the label line selects.
    dashboard.install_focus(SessionId(5));
    dashboard.mouse_action(click(27, 5), area);
    assert!(!palette_text(&dashboard).contains("Close terminal?"));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(matches!(
        dashboard.mouse_action(click(27, 4), area),
        DashboardAction::Redraw
    ));
    assert!(palette_text(&dashboard).contains("Close terminal?"));
}

#[test]
fn sidebar_stacks_every_agent_label_or_none() {
    // At 29 cells `claude` fits beside a twelve-cell name and `pi:grok-4.7` does not.
    let draw = |review_label: &str| {
        let mut dashboard = dashboard_fixture();
        let mut hierarchy = fixture_hierarchy();
        review_session_mut(&mut hierarchy).label = review_label.into();
        implement_session_mut(&mut hierarchy).label = "claude".into();
        dashboard.install_hierarchy(hierarchy);
        dashboard.install_focus(SessionId(5));
        let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        (0..12)
            .map(|y| {
                (0..29)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    let inline = draw("claude");
    assert_eq!(inline[4], "     - review         claude");
    assert_eq!(inline[5], "  󰘬 lifecycle");
    assert_eq!(inline[7], "     · implement      claude");
    assert_eq!(inline[8], "");
    let stacked = draw("pi / grok-4.7");
    assert_eq!(stacked[4], "     - review");
    assert_eq!(stacked[5], "       └ pi:grok-4.7");
    assert_eq!(stacked[6], "  󰘬 lifecycle");
    assert_eq!(stacked[7], "     $ local", "shells never stack");
    assert_eq!(stacked[8], "     · implement");
    assert_eq!(
        stacked[9], "       └ claude",
        "a label that fits still stacks"
    );
    assert_eq!(stacked[10], "");
}

#[test]
fn wide_session_row_keeps_the_agent_and_model_on_one_line() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    review_session_mut(&mut hierarchy).label = "pi / grok-4.7".into();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(
        sidebar_row(&terminal, 4),
        format!("     - review{}pi:grok-4.7 ", " ".repeat(14))
    );
    assert_eq!(sidebar_row(&terminal, 5).trim_end(), "  󰘬 lifecycle");
    let area = Rect::new(0, 0, 120, 40);
    let mut clicked = dashboard_fixture();
    clicked.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 7,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert!(
        clicked.focused_session().is_none(),
        "row 5 is the workspace"
    );
}

#[test]
fn sidebar_shows_each_agent_with_or_without_its_model() {
    for agent in ["claude", "codex", "grok", "pi", "omp", "cursor"] {
        for (label, with_model) in [
            (agent.to_string(), false),
            (format!(" {agent} / shared-model "), true),
        ] {
            let mut dashboard = dashboard_fixture();
            let mut hierarchy = fixture_hierarchy();
            review_session_mut(&mut hierarchy).label = label;
            dashboard.install_hierarchy(hierarchy);
            dashboard.install_focus(SessionId(5));
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal
                .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
                .unwrap();
            let row = sidebar_row(&terminal, 4);
            let under = sidebar_row(&terminal, 5);
            if with_model {
                // `agent:shared-model` fits beside a twelve-cell name only for short
                // agent names; the others keep the whole model on the line under the name.
                let label = format!("{agent}:shared-model");
                let inline = row.trim_end().ends_with(&label);
                assert!(
                    inline || under.trim_end() == format!("       └ {label}"),
                    "{agent} should keep its whole model: {row:?} / {under:?}"
                );
                assert_eq!(
                    under.contains("lifecycle"),
                    inline,
                    "the extra line exists only when the label did not fit: {under:?}"
                );
            } else {
                assert!(
                    row.trim_end().ends_with(agent) && !row.contains(':'),
                    "{agent} without a model stays bare: {row:?}"
                );
                assert!(under.contains("lifecycle"), "one line: {under:?}");
            }
        }
    }
}

#[test]
fn long_sidebar_names_clip_to_one_screen_line() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].name = "consigint-界界界界界界界界界界界界界界".into();
    review_session_mut(&mut hierarchy).name =
        "review-a-very-long-session-name-that-must-not-wrap".into();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
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

    // Too narrow for a twelve-cell name beside the agent: the agent moves under the name.
    let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y| {
        (0..24)
            .map(|column| buffer[(column, y)].symbol())
            .collect::<String>()
    };
    assert_eq!(row(4), "▌    - review-a-very-lo…");
    assert_eq!(row(5), "▌      └ claude         ");
    assert_eq!(buffer[(24, 4)].symbol(), "│");
    assert_eq!(buffer[(24, 5)].symbol(), "│");
}

#[test]
fn dashboard_geometry_remains_drawable_at_target_and_tiny_sizes() {
    for (width, height, terminal_width, terminal_height) in
        [(180, 72, 140, 68), (120, 40, 80, 36), (80, 24, 40, 20)]
    {
        let mut dashboard = dashboard_fixture();
        let area = Rect::new(0, 0, width, height);
        dashboard.install_area(area);
        let terminal_rect = dashboard.pane_rects(area)[0].terminal;
        assert_eq!(
            terminal_rect,
            Rect::new(40, 3, terminal_width, terminal_height)
        );
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
    dashboard.install_focus(SessionId(5));
    let mouse = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let area = Rect::new(0, 0, 120, 40);
    let action = dashboard.mouse_action(mouse(2, 2), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    // A folded workspace shows how many sessions it hides.
    assert_eq!(
        sidebar_row(&terminal, 2),
        format!("  󰘬 auth{}▸ 2 ", " ".repeat(27))
    );
    let action = dashboard.mouse_action(mouse(2, 2), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let action = dashboard.mouse_action(mouse(4, 4), area);
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let action = dashboard.mouse_action(mouse(0, 1), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    // A folded project shows its workspace count and keeps its rule.
    assert_eq!(
        sidebar_row(&terminal, 1),
        format!(" CONSIGINT {} ▸ 2 ws ", "─".repeat(20))
    );
    assert_eq!(sidebar_row(&terminal, 2).trim_end(), "");
    assert_eq!(
        sidebar_row(&terminal, 3),
        format!(" SPACELIFT-AGENT {}", "─".repeat(22))
    );
    let action = dashboard.mouse_action(
        MouseEvent {
            column: 60,
            ..mouse(4, 6)
        },
        area,
    );
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    enter_terminal(&mut dashboard);
    assert_eq!(
        dashboard.mouse_action(mouse(4, 5), area),
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
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    let mut project = dashboard_fixture();
    enter_terminal(&mut project);
    assert_eq!(
        project.mouse_action(click(5, 1), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(project.focused_session().is_none());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &project, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, 1).contains("▸"),
        "label click must not fold the project"
    );

    let mut project_disclosure = dashboard_fixture();
    enter_terminal(&mut project_disclosure);
    assert_eq!(
        project_disclosure.mouse_action(click(0, 1), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &project_disclosure, 0))
        .unwrap();
    assert!(sidebar_row(&terminal, 1).contains("▸"));
    assert_eq!(project_disclosure.focused_session(), Some(SessionId(1)));

    let mut workspace = dashboard_fixture();
    enter_terminal(&mut workspace);
    assert_eq!(
        workspace.mouse_action(click(5, 2), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(workspace.focused_session().is_none());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &workspace, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, 2).contains("▸"),
        "workspace label click must not fold"
    );

    let mut session = dashboard_fixture();
    enter_terminal(&mut session);
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
    dashboard.install_area(area);
    let action = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, action);
    let retained = dashboard.focused_session().expect("split session");
    dashboard.install_screen(SessionId(1), &[]);
    dashboard.install_screen(retained, &[]);
    let left = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == 0)
        .expect("left pane")
        .terminal;
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: left.x + 2,
            row: left.y + 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.install_screen(SessionId(1), &[]);
    dashboard.install_screen(retained, &[]);

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

    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );

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
    dashboard.install_area(area);
    let action = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, action);
    let split_view = dashboard
        .request_view_at(area)
        .expect("split should request a view");
    dashboard.drain_outbox();
    acknowledge_all_view_targets(&mut dashboard, split_view);
    let split = dashboard.focused_session().expect("split session");
    dashboard.install_screen(SessionId(1), &[]);
    dashboard.install_screen(split, &[]);
    let left = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == 0)
        .expect("left pane")
        .terminal;
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: left.x + 2,
            row: left.y + 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.install_screen(SessionId(1), &[]);
    dashboard.install_screen(split, &[]);

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
        .request_view_at(area)
        .expect("container selection should update the view");
    dashboard.drain_outbox();
    acknowledge_all_view_targets(&mut dashboard, selected);
    let retained = dashboard.focused_session().expect("retained wire focus");
    let rects = dashboard.pane_rects(area);
    assert_eq!(rects.len(), 2);
    let focused_rect = rects
        .iter()
        .find(|pane| pane.pane_index == 1)
        .expect("assigned pane")
        .terminal;
    let empty = rects
        .iter()
        .find(|pane| pane.pane_index == 0)
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
    assert_eq!(dashboard.focused_session(), Some(retained));

    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: focused_rect.x + 2,
                row: focused_rect.y + 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
}

#[test]
fn empty_workspace_selection_does_not_cover_a_retained_terminal() {
    let area = Rect::new(0, 0, 120, 40);
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces.push(WorkspaceSummary {
        project: "consigint".into(),
        name: "empty".into(),
        id: "empty".into(),
        root: false,
        warning: None,
        path: PathBuf::from("/tmp/empty"),
        sessions: Vec::new(),
    });
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(area);
    let action = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, action);
    let retained = dashboard.focused_session().expect("split session");
    dashboard.install_screen(SessionId(1), &[]);
    dashboard.install_screen(retained, b"RETAINED_TERMINAL");

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
    let retained = dashboard.focused_session().expect("retained session");
    dashboard.install_screen(retained, b"RETAINED_TERMINAL");
    let retained_rect = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == 0)
        .expect("retained pane")
        .terminal;

    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let shown = (retained_rect.x..retained_rect.right())
        .map(|column| terminal.backend().buffer()[(column, retained_rect.y)].symbol())
        .collect::<String>();
    assert!(shown.contains("RETAINED_TERMINAL"), "{shown}");
}

#[test]
fn fifty_session_selection_scrolls_tree_and_mouse_hits_viewport() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 34, cols: 88 });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(
        Box::default(),
    )));
    dashboard.install_hierarchy(HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "project".into(),
            workspaces: vec![WorkspaceSummary {
                project: "project".into(),
                name: "workspace".into(),
                id: "workspace".into(),
                root: false,
                warning: None,
                path: PathBuf::from("/tmp/workspace"),
                sessions: (1..=50)
                    .map(|id| {
                        session_summary(
                            id,
                            "project",
                            "workspace",
                            &format!("session-{id}"),
                            "sh",
                            Some(id as u32),
                            0,
                        )
                    })
                    .collect(),
            }],
        }],
    });
    dashboard.install_focus(SessionId(50));
    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let rendered = (0..40)
        .map(|row| {
            (0..39)
                .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rendered[33],
        format!("▌    - session-50{}sh ", " ".repeat(19))
    );
    assert!(rendered[34].contains("QUOTA LEFT"), "{:?}", rendered[34]);
    // The last tree row remains selectable; quota cells cannot select/close a session.
    for (index, row) in (33..=34).enumerate() {
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

    dashboard.install_focus(SessionId(1));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    enter_terminal(&mut dashboard);
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
    let mut hierarchy = fixture_hierarchy();
    let workspace = &mut hierarchy.projects[0].workspaces[0];
    for id in 6..=50 {
        workspace.sessions.push(session_summary(
            id,
            "consigint",
            "auth",
            &format!("session-{id}"),
            "sh",
            Some(id as u32),
            0,
        ));
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(50));
    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    dashboard.install_area(Rect::new(0, 0, 80, 24));
    dashboard.install_screen(SessionId(50), &[]);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
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

fn selection_bar_row(terminal: &Terminal<TestBackend>) -> Option<u16> {
    (0..40).find(|&y| terminal.backend().buffer()[(0, y)].symbol() == "▌")
}

#[test]
fn browse_jk_moves_across_project_workspace_and_session_rows() {
    let mut dashboard = dashboard_fixture();
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(4), "review session");

    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(3), "local under auth");

    assert_eq!(
        dashboard.key(KeyCode::Char('k')),
        DashboardAction::Redraw,
        "landing on a workspace must not request a view"
    );
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(2), "auth workspace");
    assert!(sidebar_row(&terminal, 2).contains("auth"));

    dashboard.key(KeyCode::Char('k'));
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(1), "consigint project");
    assert!(sidebar_row(&terminal, 1).contains("CONSIGINT"));

    dashboard.key(KeyCode::Char('j'));
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(2), "back to auth");

    dashboard.key(KeyCode::Down);
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(3));

    dashboard.key(KeyCode::Up);
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(2));
}

#[test]
fn enter_toggles_selected_container_fold_and_still_focuses_sessions() {
    let mut dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    // Climb to the auth workspace row, then fold and expand with Enter.
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    assert!(dashboard.focused_session().is_none());
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 2).contains("▸"),
        "Enter on a workspace should fold it"
    );
    assert_eq!(
        sidebar_row(&terminal, 2),
        format!("▌ 󰘬 auth{}▸ 2 ", " ".repeat(27))
    );

    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, 2).contains("▸"),
        "second Enter should expand the workspace"
    );
    assert_eq!(selection_bar_row(&terminal), Some(2));

    // Climb to the project header and fold it.
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 1).contains("▸"),
        "Enter on a project should fold it"
    );
    assert_eq!(
        sidebar_row(&terminal, 1),
        format!("▌CONSIGINT {} ▸ 2 ws ", "─".repeat(20))
    );

    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, 1).contains("▸"),
        "second Enter should expand the project"
    );

    // Select a session and Enter still focuses the terminal.
    dashboard.key(KeyCode::Char('j'));
    dashboard.key(KeyCode::Char('j'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    dashboard.install_screen(SessionId(5), &[]);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec()),
        "Enter on a session must enter Terminal mode"
    );
}

#[test]
fn browse_jk_requests_a_view_only_for_session_rows() {
    let mut dashboard = dashboard_fixture();
    let to_session = dashboard.key(KeyCode::Char('k'));
    assert!(matches!(to_session, DashboardAction::Request(_)));
    pump_view(&mut dashboard, to_session);
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    assert_eq!(dashboard.key(KeyCode::Char('k')), DashboardAction::Redraw);
    assert!(dashboard.focused_session().is_none());
    assert!(matches!(
        dashboard.key(KeyCode::Char('j')),
        DashboardAction::Request(_)
    ));
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
}

#[test]
fn enter_on_an_unready_session_refuses_and_stays_in_browse() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_unready(SessionId(1));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let footer = rendered_footer(&dashboard, 120);
    assert!(footer.contains("ERROR:"), "{footer}");
    assert_ne!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
}

#[test]
fn j_from_a_folded_workspace_skips_hidden_sessions() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    dashboard.key(KeyCode::Char('j'));
    assert!(dashboard.focused_session().is_none());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 3).contains("lifecycle"),
        "j from folded auth should land on the next visible workspace"
    );
    assert_eq!(selection_bar_row(&terminal), Some(3));
}

#[test]
fn jk_from_a_hidden_session_steps_off_the_folded_header() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    dashboard.key(KeyCode::Char('j'));
    assert!(dashboard.focused_session().is_none());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(3));
    assert!(
        sidebar_row(&terminal, 3).contains("lifecycle"),
        "j after mouse-folding auth should leave the header, not jump to the first project"
    );
}

#[test]
fn empty_tree_jk_does_not_panic() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    dashboard.install_area(Rect::new(0, 0, 88, 38));
    assert_eq!(dashboard.key(KeyCode::Char('j')), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Char('k')), DashboardAction::Redraw);
    assert!(dashboard.focused_session().is_none());
}

#[test]
fn keyboard_selecting_a_project_scrolls_it_into_view() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 34, cols: 88 });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "project".into(),
            workspaces: vec![WorkspaceSummary {
                project: "project".into(),
                name: "workspace".into(),
                id: "workspace".into(),
                root: false,
                warning: None,
                path: PathBuf::from("/tmp/workspace"),
                sessions: (1..=50)
                    .map(|id| {
                        session_summary(
                            id,
                            "project",
                            "workspace",
                            &format!("session-{id}"),
                            "sh",
                            Some(id as u32),
                            0,
                        )
                    })
                    .collect(),
            }],
        }],
    });
    dashboard.install_focus(SessionId(50));
    for _ in 0..52 {
        dashboard.key(KeyCode::Char('k'));
        if dashboard.focused_session().is_none() {
            break;
        }
    }
    dashboard.key(KeyCode::Char('k'));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(selection_bar_row(&terminal), Some(1));
    assert!(sidebar_row(&terminal, 1).contains("PROJECT"));
}

#[test]
fn container_selection_does_not_close_or_switch_a_split() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    let action = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, action);
    let retained = dashboard.focused_session().expect("split session");
    dashboard.install_screen(SessionId(1), &[]);
    dashboard.install_screen(retained, &[]);
    let left = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == 0)
        .expect("left pane")
        .terminal;
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: left.x + 2,
            row: left.y + 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.install_screen(SessionId(1), &[]);
    dashboard.install_screen(retained, &[]);
    assert_eq!(dashboard.pane_rects(area).len(), 2);
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
        DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(retained));
    dashboard.key(KeyCode::Char('x'));
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    dashboard.key(KeyCode::Tab);
    assert_eq!(dashboard.focused_session(), Some(retained));
}

#[test]
fn removing_a_project_drops_its_container_selection() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    let mut hierarchy = fixture_hierarchy();
    hierarchy
        .projects
        .retain(|project| project.name != "consigint");
    dashboard.install_hierarchy(hierarchy);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    dashboard.install_hierarchy(fixture_hierarchy());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, 1).contains("▸"),
        "Enter must not fold a project that was removed"
    );
}

#[test]
fn container_click_in_terminal_mode_returns_to_browse() {
    let mut dashboard = dashboard_fixture();
    enter_terminal(&mut dashboard);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert_ne!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    let footer = rendered_footer(&dashboard, 120);
    assert!(footer.contains("BROWSE"), "{footer}");
}

#[test]
fn highlighted_session_row_shows_close_mark() {
    let mut dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let review = sidebar_row(&terminal, 4);
    assert!(
        !review.contains("[x]") && review.contains("claude"),
        "a highlighted row hides the mark until the pointer is over it, got {review:?}"
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let review = sidebar_row(&terminal, 4);
    assert!(
        review.trim_end().ends_with("[x]") && !review.contains("claude"),
        "hover replaces the agent label with the close mark, got {review:?}"
    );
    let mark_x = review.trim_end().len() - 3;
    assert_eq!(
        terminal.backend().buffer()[(mark_x as u16, 4)].fg,
        Color::Rgb(203, 166, 247),
        "close mark uses the title-bar action colour"
    );
    let local = sidebar_row(&terminal, 3);
    assert!(
        !local.contains("[x]"),
        "unselected session must not show the close mark, got {local:?}"
    );
    let project = sidebar_row(&terminal, 1);
    assert!(
        !project.contains("[x]"),
        "project row must not show the close mark, got {project:?}"
    );
}

#[test]
fn clicking_the_close_mark_confirms_archive_of_the_highlighted_session() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    hover(&mut dashboard, 10, 4, area);
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert_eq!(action, DashboardAction::Redraw);
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Close terminal?") && text.contains("review (#1)"),
        "close mark must open the existing confirm, got {text}"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.pane_rects(area).len(), 1);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("confirming the close mark must archive");
    };
    assert_eq!(
        message.request,
        Request::CloseTerminal {
            session: SessionId(1),
            expected_run: ovrcr::protocol::SessionRunId(1),
        }
    );
}

#[test]
fn dismissing_the_close_mark_confirm_archives_nothing() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    hover(&mut dashboard, 10, 4, area);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    assert!(!palette_text(&dashboard).contains("Close terminal?"));
    assert!(sidebar_contains_close_mark(&dashboard));
}

#[test]
fn close_mark_from_terminal_mode_returns_to_browse() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enter_terminal(&mut dashboard);
    hover(&mut dashboard, 10, 4, area);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let text = palette_text(&dashboard);
    assert!(text.contains("review (#1)"), "{text}");
    assert!(
        rendered_footer(&dashboard, 120).contains("BROWSE"),
        "{text}"
    );
}

#[test]
fn pointer_over_another_session_shows_its_close_mark() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let moved = |column, row| MouseEvent {
        kind: MouseEventKind::Moved,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    dashboard.mouse_action(moved(10, 7), area);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 7).contains("[x]"),
        "hovered session must show the close mark, got {:?}",
        sidebar_row(&terminal, 7)
    );
    assert!(
        !sidebar_row(&terminal, 4).contains("[x]"),
        "only the hovered row shows the close mark"
    );
    dashboard.mouse_action(moved(80, 7), area);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, 7).contains("[x]"));
    assert!(!sidebar_row(&terminal, 4).contains("[x]"));
}

#[test]
fn clicking_a_hovered_close_mark_archives_that_row_without_switching_the_pane() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert_eq!(action, DashboardAction::Redraw);
    let text = palette_text(&dashboard);
    assert!(
        text.contains("(#5)"),
        "hovered close mark must name that session, got {text}"
    );
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(1)),
        "archive must not switch the pane"
    );
    assert_eq!(dashboard.pane_rects(area).len(), 1);
}

#[test]
fn close_confirm_highlights_only_the_clicked_row_until_dismissed() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 3).starts_with('▌'),
        "confirm must highlight the clicked row, got {:?}",
        sidebar_row(&terminal, 3)
    );
    assert!(
        !sidebar_row(&terminal, 4).starts_with('▌'),
        "focused pane session must not stay highlighted during confirm, got {:?}",
        sidebar_row(&terminal, 4)
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, 3).starts_with('▌'));
    assert!(sidebar_row(&terminal, 4).starts_with('▌'));
    dashboard.key(KeyCode::Char('X'));
    let text = palette_text(&dashboard);
    assert!(text.contains("review (#1)"), "{text}");
    assert!(!text.contains("(#5)"), "{text}");
}

#[test]
fn hovered_close_mark_does_not_focus_the_pane_already_showing_that_session() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let split = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, split);
    dashboard.key(KeyCode::Tab);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 6,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 6,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let text = palette_text(&dashboard);
    assert!(text.contains("(#4)"), "{text}");
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.pane_rects(area).len(), 2);
}

#[test]
fn pointer_on_a_workspace_row_clears_the_hover_mark() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, 7).contains("[x]"));
    assert!(!sidebar_row(&terminal, 4).contains("[x]"));
    assert!(
        sidebar_row(&terminal, 5).contains("[x]"),
        "{:?}",
        sidebar_row(&terminal, 5)
    );
}

#[test]
fn palette_keeps_the_mouse_from_a_second_close_mark() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    hover(&mut dashboard, 10, 4, area);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let text = palette_text(&dashboard);
    assert!(text.contains("review (#1)"), "{text}");
    assert!(!text.contains("(#2)"), "{text}");
}

#[test]
fn losing_mouse_capture_clears_a_hover_mark() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.event_action(Event::FocusLost);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, 7).contains("[x]"));
    assert!(!sidebar_row(&terminal, 4).contains("[x]"));
}

fn sidebar_contains_close_mark(dashboard: &Dashboard) -> bool {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..40).any(|y| sidebar_row(&terminal, y).contains("[x]"))
}

#[test]
fn history_and_copy_hide_the_session_close_mark() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('['));
    assert!(
        !sidebar_contains_close_mark(&dashboard),
        "Copy must not show the session close mark"
    );

    let mut dashboard = dashboard_fixture();
    let DashboardAction::Request(begin) = dashboard.key(KeyCode::PageUp) else {
        panic!("history must request a capture");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(ovrcr::protocol::HistoryOpened {
            session: SessionId(1),
            snapshot: ovrcr::protocol::HistorySnapshotId(7),
            revision: 1,
            size: TerminalSize { rows: 10, cols: 20 },
            history_rows: 0,
            total_rows: 10,
        }),
    });
    assert!(
        !sidebar_contains_close_mark(&dashboard),
        "History must not show the session close mark"
    );
}

#[test]
fn clicking_the_close_mark_confirms_a_paused_session() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    review_session_mut(&mut hierarchy).phase = SessionPhase::Paused;
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    hover(&mut dashboard, 10, 4, Rect::new(0, 0, 120, 40));
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 36,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        ),
        DashboardAction::Redraw
    );
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Close terminal?") && text.contains("review (#1)"),
        "{text}"
    );
}

#[test]
fn clicking_the_close_mark_archives_an_exited_session_immediately() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(2));
    hover(&mut dashboard, 10, 7, Rect::new(0, 0, 120, 40));
    let DashboardAction::Request(message) = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    ) else {
        panic!("exited close mark must archive immediately");
    };
    assert_eq!(
        message.request,
        Request::CloseTerminal {
            session: SessionId(2),
            expected_run: ovrcr::protocol::SessionRunId(1),
        }
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
}

#[test]
fn clicking_beside_the_close_mark_still_opens_the_session() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(5));
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert!(
        matches!(
            action,
            DashboardAction::Request(ClientMessage {
                request: Request::SetView { .. },
                ..
            })
        ),
        "title click must open the session, got {action:?}"
    );
    assert!(
        !palette_text(&dashboard).contains("Close terminal?"),
        "title click must not archive"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
}

#[test]
fn close_mark_is_absent_until_hover_and_inert_without_mouse_capture() {
    let mut dashboard = dashboard_fixture();
    assert!(!sidebar_contains_close_mark(&dashboard));
    dashboard.event_action(Event::FocusLost);
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 36,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        ),
        DashboardAction::None
    );
    assert!(!palette_text(&dashboard).contains("Close terminal?"));
}

#[test]
fn close_mark_is_omitted_when_the_row_is_narrower_than_three_cells() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(6, 12)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let text: String = (0..12)
        .map(|y| {
            (0..6)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    assert!(!text.contains("[x]"), "{text}");
}

#[test]
fn selected_project_row_hides_the_session_close_mark() {
    let mut dashboard = dashboard_fixture();
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert!(
        !sidebar_contains_close_mark(&dashboard),
        "a selected project or workspace leaves no highlighted session mark"
    );
}

#[test]
fn a_selected_workspace_still_shows_the_close_mark_on_the_hovered_session() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, 3).contains("[x]"),
        "{:?}",
        sidebar_row(&terminal, 3)
    );
    assert!(!sidebar_row(&terminal, 4).contains("[x]"));
}

#[test]
fn hovering_a_workspace_shows_close_mark_and_click_opens_remove() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    hover(&mut dashboard, 10, 2, area);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let row = sidebar_row(&terminal, 2);
    assert!(row.contains("[x]") && row.contains("auth"), "{row:?}");
    assert!(!sidebar_row(&terminal, 1).contains("[x]"));
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let text = palette_text(&dashboard);
    assert!(text.contains("Remove workspace"), "{text}");
    assert!(text.contains("consigint / auth"), "{text}");
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.pane_rects(area).len(), 1);
}
