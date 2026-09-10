//! Copy mode, frozen history, history copy, and pause/resume.

use crate::*;

#[test]
fn resize_matching_screen_restores_snapshot_and_ignores_stale_screen() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    let size = TerminalSize { rows: 20, cols: 40 };
    let resize = dashboard
        .view_request(outer_area_for_pane(size), 901)
        .unwrap()
        .expect("selected dashboard should request resize");
    assert!(matches!(
        resize.request,
        Request::SetView { view }
            if view.focused == Some(session)
                && view.panes.iter().any(|pane| pane.session == session && pane.size == size)
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session,
            revision: dashboard.view_revision,
            size,
            bytes: b"STALE_SCREEN".to_vec(),
        },
    });
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("STALE_SCREEN")
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session,
            revision: dashboard.view_revision,
            size,
            bytes: b"RESTORED_SCREEN".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .size(),
        (size.rows, size.cols)
    );
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("RESTORED_SCREEN")
    );
}

#[test]
fn browse_and_terminal_modes_keep_input_ownership_clear() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 900);
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].phase = SessionPhase::Running;
    let action = dashboard.key_action(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let ovrcr::tui::DashboardAction::Request(request) = action else {
        panic!("selection did not request a view")
    };
    acknowledge_view_request(&mut dashboard, request);
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'q'])
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Detach
    );
}

#[test]
fn copy_mode_routes_keys_and_freezes_output() {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard
        .view_request(Rect::new(0, 0, 89, 38), 900)
        .unwrap()
        .expect("copy test should request a refreshed view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session: id,
            revision: dashboard.view_revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"abc".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Ok,
    });
    dashboard.key(KeyCode::Char('['));
    dashboard.copy_notice = Some("stale movement notice".into());
    assert_eq!(
        dashboard.key(KeyCode::Home),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.copy_notice.is_none());
    dashboard.copy_notice = Some("stale anchor notice".into());
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.copy_notice.is_none());
    dashboard.copy_notice = Some("stale movement notice".into());
    assert_eq!(
        dashboard.key(KeyCode::Right),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.copy_notice.is_none());
    assert_eq!(
        dashboard.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::CopyText("ab".into())
    );
    let anchor = dashboard.copy.as_ref().and_then(|copy| copy.anchor);
    dashboard.finish_copy(Err(io::Error::other("denied")));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert_eq!(dashboard.copy.as_ref().and_then(|copy| copy.anchor), anchor);
    assert!(
        dashboard
            .copy_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("denied"))
    );
    dashboard.finish_copy(Ok(()));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Clipboard request sent; paste to verify")
    );
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        revision: dashboard.view_revision,
        bytes: b"\rNEW".to_vec(),
    }));
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("NEW")
    );
    assert_eq!(
        dashboard.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("ab")
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("secret".into())),
        ovrcr::tui::DashboardAction::None
    );

    let mut pending_screen = dashboard_fixture();
    pending_screen
        .view_request(Rect::new(0, 0, 89, 38), 901)
        .unwrap()
        .expect("pending copy fixture should have an in-flight view");
    assert_eq!(
        pending_screen.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(pending_screen.mode, ovrcr::tui::InputMode::Browse);
    assert_eq!(
        pending_screen.error.as_deref(),
        Some("Waiting for terminal screen")
    );

    let mut failed_selection = copy_ready_dashboard();
    failed_selection.select_request(SessionId(5), 902);
    failed_selection.handle_server_message(ServerMessage::Response {
        request_id: 902,
        response: Response::Error {
            code: ErrorCode::NotFound,
            message: "selection failed".into(),
        },
    });
    assert_eq!(
        failed_selection.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(failed_selection.mode, ovrcr::tui::InputMode::Browse);
    assert_eq!(
        failed_selection.error.as_deref(),
        Some("Waiting for terminal screen")
    );

    let mut release = copy_ready_dashboard();
    assert_eq!(
        release.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(release.mode, ovrcr::tui::InputMode::Browse);
    assert!(release.copy.is_none());

    let mut repeat_motion = copy_ready_dashboard();
    assert_eq!(
        repeat_motion.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let before = repeat_motion.copy.as_ref().unwrap().cursor;
    assert_eq!(
        repeat_motion.key_action(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_ne!(repeat_motion.copy.as_ref().unwrap().cursor, before);

    let mut press_only = copy_ready_dashboard();
    press_only.key(KeyCode::Char('['));
    press_only.key(KeyCode::Home);
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert!(press_only.copy.as_ref().unwrap().anchor.is_none());
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(press_only.copy.as_ref().unwrap().anchor.is_some());
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::CopyText("ab".into())
    );

    let mut press_exit = copy_ready_dashboard();
    press_exit.key(KeyCode::Char('['));
    assert_eq!(
        press_exit.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(press_exit.mode, ovrcr::tui::InputMode::Copy);
    assert_eq!(
        press_exit.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(press_exit.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn matching_history_open_clears_resize_notice_but_stale_response_does_not() {
    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    let resize = dashboard
        .view_request(
            outer_area_for_pane(TerminalSize { rows: 20, cols: 40 }),
            901,
        )
        .unwrap()
        .expect("the new geometry should request a view");
    acknowledge_all_view_targets(&mut dashboard, resize);
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Copy cancelled: terminal resized")
    );

    let begin = match dashboard.key(KeyCode::PageUp) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected history begin request, got {action:?}"),
    };
    let stale = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id + 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(stale.is_empty());
    assert!(dashboard.history.is_none());
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Copy cancelled: terminal resized")
    );

    let opened = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert!(dashboard.history.is_some());
    assert!(dashboard.copy_notice.is_none());
    assert!(!opened.is_empty());

    dashboard.copy_notice = Some("stale replacement notice".into());
    dashboard.history_begin_request = Some(ovrcr::tui::PendingHistoryBegin {
        request_id: 77,
        session: SessionId(1),
        cancelled: false,
        at_tail: false,
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 77,
        response: Response::HistoryOpened(HistoryOpened {
            snapshot: HistorySnapshotId(8),
            ..history_opened(100)
        }),
    });
    assert!(dashboard.copy_notice.is_none());
    assert_eq!(
        dashboard.history.as_ref().unwrap().opened.snapshot,
        HistorySnapshotId(8)
    );
}

#[test]
fn copy_render_highlights_unicode_and_preserves_layout() {
    let mut parser = vt100::Parser::new(2, 6, 0);
    parser.process("\x1b[?25lA界B".as_bytes());
    let mut selection = CopySelection::capture(SessionId(1), parser.screen());
    selection.cursor = CopyPoint { row: 0, col: 1 };
    selection.anchor = Some(selection.cursor);
    assert!(selection.screen.hide_cursor());
    assert!(selection.contains(CopyPoint { row: 0, col: 1 }));
    assert!(selection.contains(CopyPoint { row: 0, col: 2 }));

    let base = Color::Rgb(30, 30, 46);
    let teal = Color::Rgb(148, 226, 213);
    let text = Color::Rgb(205, 214, 244);
    let mut terminal = Terminal::new(TestBackend::new(6, 2)).unwrap();
    terminal
        .draw(|frame| {
            render_copy(frame, Rect::new(0, 0, 6, 2), &selection);
            let buffer = frame.buffer_mut();
            assert_eq!(buffer[(1, 0)].fg, base);
            assert_eq!(buffer[(1, 0)].bg, teal);
            assert_eq!(buffer[(2, 0)].fg, base);
            assert_eq!(buffer[(2, 0)].bg, teal);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "A");
    assert_eq!(buffer[(0, 0)].fg, text);
    assert_eq!(buffer[(0, 0)].bg, base);
    assert_eq!(buffer[(1, 0)].symbol(), "界");
    assert_eq!(buffer[(3, 0)].symbol(), "B");
    assert_eq!(buffer[(3, 0)].fg, text);
    assert_eq!(buffer[(3, 0)].bg, base);
    assert_eq!(
        terminal.backend_mut().get_cursor_position().unwrap(),
        Position::new(1, 0)
    );

    let mut output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| render_copy(frame, Rect::new(0, 0, 6, 2), &selection))
            .unwrap();
    }
    let mut rendered = vt100::Parser::new(2, 6, 0);
    rendered.process(&output);
    assert_eq!(rendered.screen().cell(0, 1).unwrap().contents(), "界");
    assert_eq!(rendered.screen().cell(0, 3).unwrap().contents(), "B");
    assert_eq!(rendered.screen().cell(0, 4).unwrap().contents(), " ");
}

#[test]
fn copy_render_tiny_pane_and_footer() {
    let mut tiny = copy_ready_dashboard();
    tiny.key(KeyCode::Char('['));
    let mut tiny_terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    tiny_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &tiny))
        .unwrap();

    let mut dashboard = copy_ready_dashboard();
    let sidebar_and_metadata = |buffer: &ratatui::buffer::Buffer| {
        (1..3)
            .map(|row| {
                (0..120)
                    .map(|col| buffer[(col, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let initial_layout = sidebar_and_metadata(terminal.backend().buffer());
    dashboard.key(KeyCode::Char('['));
    let id = dashboard.focused_session().unwrap();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        revision: dashboard.view_revision,
        bytes: b"\rLIVE".to_vec(),
    }));
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .cell(0, 0)
            .unwrap()
            .contents(),
        "L"
    );
    assert_eq!(
        dashboard
            .copy
            .as_ref()
            .unwrap()
            .screen
            .cell(0, 0)
            .unwrap()
            .contents(),
        "a"
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let footer = (0..120)
        .map(|col| buffer[(col, 39)].symbol())
        .collect::<String>();
    assert!(footer.starts_with("COPY  ? Help  Esc Back"));
    assert!(
        ["h Move left", "j Move down", "k Move up", "l Move right"]
            .iter()
            .all(|hint| footer.contains(hint))
    );
    assert!(footer.contains("v Select  y Copy"));
    let rendered_layout = sidebar_and_metadata(buffer);
    assert_eq!(rendered_layout, initial_layout);
    assert!(initial_layout[0].contains("pid: 111  elapsed: 0m"));
    assert!(initial_layout[1].contains("─"));
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    assert_eq!(buffer[(39, 10)].symbol(), "│");
    assert_eq!(buffer[(40, 1)].symbol(), "p");
    assert_eq!(buffer[(40, 2)].symbol(), "─");
    assert_eq!(buffer[(40, 3)].symbol(), "a");
    assert_eq!(buffer[(41, 3)].symbol(), "b");
    assert_eq!(buffer[(42, 3)].symbol(), "c");

    dashboard.copy_notice = Some("Copy cancelled: terminal resized".into());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let notice_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(notice_footer.trim_end(), "Copy cancelled: terminal resized");

    dashboard.error = Some("server failed".into());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let error_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(error_footer.trim_end(), "ERROR: server failed");

    dashboard.error = None;
    dashboard.cancel_copy(Some("Copy cancelled: terminal resized"));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let browse_notice_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(
        browse_notice_footer.trim_end(),
        "Copy cancelled: terminal resized"
    );

    let mut entry_notice = dashboard_fixture();
    entry_notice.panes[entry_notice.focused_pane].ready = false;
    assert_eq!(
        entry_notice.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        entry_notice.error.as_deref(),
        Some("Waiting for terminal screen")
    );
    entry_notice.error = None;
    entry_notice.copy_notice = Some("Waiting for terminal screen".into());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &entry_notice, 0))
        .unwrap();
    let entry_notice_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(
        entry_notice_footer.trim_end(),
        "Waiting for terminal screen"
    );
}

#[test]
fn copy_mode_cancels_at_identity_boundaries() {
    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Esc);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());

    let mut dashboard = copy_ready_dashboard();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert!(
        dashboard
            .history_begin_request
            .as_ref()
            .is_some_and(|pending| !pending.cancelled)
    );
    dashboard.key(KeyCode::Char('['));
    assert!(
        dashboard
            .history_begin_request
            .as_ref()
            .is_some_and(|pending| pending.cancelled)
    );
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(matches!(
        release.as_slice(),
        [ClientMessage {
            request: Request::HistoryEnd { .. },
            ..
        }]
    ));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert!(dashboard.copy.is_some());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::PageUp);
    dashboard.key(KeyCode::Char('['));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "cancelled".into(),
        },
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert!(dashboard.copy.is_some());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    dashboard.select_session(SessionId(5));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    dashboard
        .view_request(
            outer_area_for_pane(TerminalSize { rows: 20, cols: 40 }),
            901,
        )
        .unwrap()
        .expect("the new geometry should request a view");
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Copy cancelled: terminal resized")
    );

    for key in [KeyCode::Char('q'), KeyCode::Char('\u{7}')] {
        let mut dashboard = copy_ready_dashboard();
        dashboard.key(KeyCode::Char('['));
        assert_eq!(dashboard.key(key), ovrcr::tui::DashboardAction::Redraw);
        assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
        assert!(dashboard.copy.is_none());
    }
    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    assert_eq!(dashboard.ctrl('g'), ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    let hierarchy = dashboard.hierarchy.clone();
    let mut removed = hierarchy;
    removed.projects[1].workspaces[1]
        .sessions
        .retain(|session| session.id != SessionId(1));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(removed)));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());
}

#[test]
fn copy_mode_waits_for_matching_screen() {
    let mut dashboard = dashboard_fixture();
    let first = dashboard.focused_session().unwrap();
    dashboard
        .view_request(Rect::new(0, 0, 89, 38), 901)
        .unwrap()
        .expect("copy readiness test should request a refreshed view");
    dashboard.select_session(SessionId(5));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session: first,
            revision: dashboard.view_revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"late-first".to_vec(),
        },
    });
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("late-first")
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session: SessionId(5),
            revision: dashboard.view_revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"old-request".to_vec(),
        },
    });
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("old-request")
    );
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Ok,
    });
    assert_eq!(requests.len(), 1);
    let request_id = requests[0].request_id;
    let revision = dashboard.view_revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Screen {
            session: SessionId(5),
            revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"matching".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Ok,
    });
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("matching")
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
}

#[test]
fn copy_mode_keeps_alternate_and_resync_snapshots() {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard
        .view_request(Rect::new(0, 0, 89, 38), 903)
        .unwrap()
        .expect("alternate screen test should request a refreshed view");
    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 903,
        response: Response::Screen {
            session: id,
            revision: dashboard.view_revision,
            size,
            bytes: b"\x1b[?1049hALT".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 903,
        response: Response::Ok,
    });
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Right);
    let requests =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session: id,
            revision: dashboard.view_revision,
        }));
    let view_revision = dashboard.view_revision;
    assert!(matches!(
        requests.as_slice(),
        [ClientMessage {
            request: Request::SetView { view },
            ..
        }] if view.revision == view_revision
            && view.focused == Some(id)
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: requests[0].request_id,
        response: Response::Screen {
            session: id,
            revision: view_revision,
            size,
            bytes: b"\x1b[?1049lPRIMARY".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: requests[0].request_id,
        response: Response::Ok,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        revision: view_revision,
        bytes: b"-LIVE".to_vec(),
    }));
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("PRIMARY-LIVE")
    );
    assert_eq!(
        dashboard.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("AL")
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
}

#[test]
fn pause_resume_browse_keys_send_explicit_requests() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 900);

    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 1,
            request: Request::PauseSession {
                session: SessionId(5),
            },
        })
    );
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase,
        SessionPhase::Running
    );

    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 2,
            request: Request::PauseSession {
                session: SessionId(5),
            },
        })
    );

    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Paused;
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 3,
            request: Request::ResumeSession {
                session: SessionId(5),
            },
        })
    );

    dashboard.panes[dashboard.focused_pane].session = None;
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("selected"))
    );

    dashboard.select_session(SessionId(2));
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("exited"))
    );

    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 901);
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Running;
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'p'])
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'r'])
    );
}

#[test]
fn pause_resume_input_and_paste_stay_guarded() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Paused;

    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert_eq!(
        dashboard.error.as_deref(),
        Some("Session paused; press r to resume")
    );

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.event_action(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        event_to_request(
            &mut dashboard,
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            100,
        )
        .is_none()
    );
    assert!(event_to_request(&mut dashboard, Event::Paste("blocked".into()), 101).is_none());
    assert!(dashboard.input_request(b"blocked".to_vec(), 99).is_none());
    assert!(
        dashboard.event_action(Event::Key(KeyEvent::new(
            KeyCode::Char('g'),
            KeyModifiers::CONTROL,
        ))) == ovrcr::tui::DashboardAction::EnterBrowse
    );

    let paused = dashboard.hierarchy.projects[1].workspaces[1].sessions[1].clone();
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        dashboard.hierarchy.clone(),
    )));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn history_navigation_never_writes_to_pty() {
    let mut dashboard = dashboard_fixture();
    let begin = dashboard.key(KeyCode::PageUp);
    assert_eq!(
        begin,
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 1,
            request: Request::HistoryBegin {
                session: SessionId(1),
            },
        })
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[5~".to_vec())
    );

    dashboard.mode = ovrcr::tui::InputMode::Browse;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(HistoryOpened {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            revision: 1,
            size: TerminalSize { rows: 10, cols: 20 },
            history_rows: 90,
            total_rows: 100,
        }),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);

    for key in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Char('k'),
        KeyCode::Char('j'),
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Char('h'),
        KeyCode::Char('l'),
        KeyCode::Enter,
    ] {
        assert!(!matches!(
            dashboard.key(key),
            ovrcr::tui::DashboardAction::PtyBytes(_)
        ));
    }
    assert!(!matches!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::PtyBytes(_)
    ));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert!(!matches!(
        dashboard.key(KeyCode::End),
        ovrcr::tui::DashboardAction::PtyBytes(_)
    ));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert_eq!(
        dashboard.key(KeyCode::Char('q')),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);

    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste("spacelift-agent progress local".into()));
    let ovrcr::tui::DashboardAction::RequestBatch(requests) = dashboard.key(KeyCode::Enter) else {
        panic!("palette switch did not preserve pending history release");
    };
    assert!(matches!(
        requests.as_slice(),
        [
            ClientMessage {
                request: Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                },
                ..
            },
            ClientMessage {
                request: Request::SetView { view },
                ..
            }
        ] if view.focused == Some(SessionId(3))
            && view.panes.iter().any(|pane| pane.session == SessionId(3))
    ));
}

#[test]
fn history_begin_is_cancelled_and_captured_by_active_overlays() {
    for overlay in ["palette", "tasks"] {
        let mut dashboard = dashboard_fixture();
        assert!(matches!(
            dashboard.key(KeyCode::PageUp),
            ovrcr::tui::DashboardAction::Request(ClientMessage {
                request: Request::HistoryBegin {
                    session: SessionId(1)
                },
                ..
            })
        ));
        if overlay == "palette" {
            assert!(matches!(
                dashboard.key(KeyCode::Char(':')),
                ovrcr::tui::DashboardAction::Request(ClientMessage {
                    request: Request::Inspect,
                    ..
                })
            ));
        } else {
            assert_eq!(dashboard.ctrl('t'), ovrcr::tui::DashboardAction::Redraw);
        }
        assert!(
            dashboard
                .history_begin_request
                .as_ref()
                .is_some_and(|pending| pending.cancelled)
        );
        assert_eq!(
            dashboard.key(KeyCode::PageUp),
            ovrcr::tui::DashboardAction::Redraw
        );
        assert_eq!(
            dashboard.key(KeyCode::Esc),
            ovrcr::tui::DashboardAction::Redraw
        );
        let requests = dashboard.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::HistoryOpened(history_opened(100)),
        });
        assert!(dashboard.history.is_none());
        assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
        assert!(matches!(
            requests.as_slice(),
            [ClientMessage {
                request: Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                },
                ..
            }]
        ));
    }
}

#[test]
fn history_release_precedes_hierarchy_fallback_selection() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let mut hierarchy = dashboard.hierarchy.clone();
    for workspace in hierarchy
        .projects
        .iter_mut()
        .flat_map(|project| &mut project.workspaces)
    {
        workspace
            .sessions
            .retain(|session| session.id != SessionId(1));
    }
    let requests = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    assert!(dashboard.history.is_none());
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    assert!(matches!(
        requests.as_slice(),
        [
            ClientMessage {
                request: Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                },
                ..
            },
            ClientMessage {
                request: Request::SetView { view },
                ..
            }
        ] if view.focused == Some(SessionId(5))
            && view.panes.iter().any(|pane| pane.session == SessionId(5))
    ));
}

#[test]
fn history_live_output_preserves_anchor() {
    let mut dashboard = screen_ready_dashboard(b"");
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    let (start_row, start_col, rows, request_id) = match page_request.request {
        Request::HistoryPage {
            start_row,
            start_col,
            rows,
            ..
        } => (start_row, start_col, rows, page_request.request_id),
        request => panic!("unexpected history request: {request:?}"),
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::HistoryRows(history_complete_page(
            start_row,
            start_col,
            rows,
            20,
            vec![history_cell("frozen", 1)],
        )),
    });
    let before = dashboard.history.clone().expect("history opened");
    let current_revision = dashboard.view_revision;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        revision: current_revision,
        bytes: b"LIVE_MARKER".to_vec(),
    }));
    let after = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after.opened, before.opened);
    assert_eq!(after.top, before.top);
    assert_eq!(after.left, before.left);
    assert_eq!(after.pages, before.pages);
    assert!(after.new_output);
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("LIVE_MARKER")
    );
    let select = dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: SessionId(1),
        revision: current_revision,
    }));
    assert!(matches!(
        select.as_slice(),
        [ClientMessage {
            request: Request::SetView { view },
            ..
        }] if view.focused == Some(SessionId(1))
            && view.panes.iter().any(|pane| pane.session == SessionId(1))
    ));
    let before_screen = dashboard.history.clone().expect("history remains open");
    let view_revision = dashboard.view_revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: select[0].request_id,
        response: Response::Screen {
            session: SessionId(1),
            revision: view_revision,
            size: TerminalSize { rows: 12, cols: 30 },
            bytes: b"SCREEN_MARKER".to_vec(),
        },
    });
    let after_screen = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after_screen.opened, before_screen.opened);
    assert_eq!(after_screen.top, before_screen.top);
    assert_eq!(after_screen.left, before_screen.left);
    assert_eq!(after_screen.pages, before_screen.pages);
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .size(),
        (12, 30)
    );
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("SCREEN_MARKER")
    );
}

#[test]
fn history_resize_preserves_capture() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    let (start_row, start_col, rows) = match page_request.request {
        Request::HistoryPage {
            start_row,
            start_col,
            rows,
            ..
        } => (start_row, start_col, rows),
        request => panic!("unexpected history request: {request:?}"),
    };
    let mut follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_complete_page(
            start_row,
            start_col,
            rows,
            20,
            vec![history_cell("old", 1)],
        )),
    });
    let mut captured_anchor = false;
    while let Some(request) = follow_up.pop() {
        let (start_row, start_col, rows, cols) = match request.request {
            Request::HistoryPage {
                start_row,
                start_col,
                rows,
                cols,
                ..
            } => (start_row, start_col, rows, cols),
            request => panic!("unexpected history request: {request:?}"),
        };
        captured_anchor |= start_row == 48;
        follow_up = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(history_tile_page(start_row, start_col, rows, cols)),
        });
    }
    assert!(captured_anchor);
    dashboard.history.as_mut().unwrap().top = 60;
    dashboard.history.as_mut().unwrap().left = 4;
    let before = dashboard.history.clone().expect("history opened");
    assert!(before.pages.iter().any(|page| page.start_row == 48));
    let resize = dashboard
        .view_request(Rect::new(0, 0, 70, 54), 99)
        .unwrap()
        .expect("live resize request");
    assert!(matches!(resize.request, Request::SetView { .. }));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Screen {
            session: SessionId(1),
            revision: dashboard.view_revision,
            size: TerminalSize { rows: 50, cols: 30 },
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Ok,
    });
    let after = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after.opened, before.opened);
    assert_eq!(after.pages, before.pages);
    assert_eq!(after.top, before.top);
    assert_eq!(after.left, before.left);
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .size(),
        (50, 30)
    );
}

#[test]
fn history_stale_response_is_ignored() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    let pending = dashboard
        .history
        .as_ref()
        .and_then(|view| view.pending.clone())
        .expect("history page pending");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id.wrapping_add(1),
        response: Response::HistoryRows(history_complete_page(
            pending.start_row,
            pending.start_col,
            pending.rows,
            20,
            vec![],
        )),
    });
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&pending)
    );
    assert!(dashboard.history.as_ref().unwrap().pages.is_empty());
    for page in [
        {
            let mut page = history_page(pending.start_row, pending.start_col, 20, vec![]);
            page.session = SessionId(5);
            page
        },
        {
            let mut page = history_page(pending.start_row, pending.start_col, 20, vec![]);
            page.snapshot = HistorySnapshotId(8);
            page
        },
        history_page(
            pending.start_row.saturating_add(16),
            pending.start_col,
            20,
            vec![],
        ),
    ] {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::HistoryRows(page),
        });
        let view = dashboard.history.as_ref().expect("history remains pending");
        assert_eq!(view.pending.as_ref(), Some(&pending));
        assert!(view.pages.is_empty());
    }
    let accepted = dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::HistoryRows(history_complete_page(
            pending.start_row,
            pending.start_col,
            pending.rows,
            pending.start_col,
            vec![],
        )),
    });
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 1);
    assert!(accepted.len() <= 1);
    dashboard.select_session(SessionId(5));
    assert!(dashboard.history.is_none());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_page(0, 0, 20, vec![history_cell("old", 1)])),
    });
    assert!(dashboard.history.is_none());
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn history_empty_rows_only_anchor_after_explicit_anchor() {
    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut fresh = HistoryView::new(history_opened(1), 0);
    fresh.pages.push_back(history_page(0, 0, 0, vec![]));
    dashboard.history = Some(fresh);
    assert!(dashboard.history_request_if_needed().is_none());
    assert!(dashboard.history.as_ref().unwrap().cursor.is_none());
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        None,
        "fresh empty row remains unanchorable"
    );
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Nothing to select on this row")
    );

    let mut anchored = HistoryView::new(history_opened(1), 0);
    anchored.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    anchored.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 0,
        col: 0,
    }));
    anchored.pages.push_back(history_page(0, 0, 0, vec![]));
    dashboard.history = Some(anchored);
    assert!(dashboard.history_request_if_needed().is_none());
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 0 }
    );
    assert!(dashboard.history.as_ref().unwrap().cursor_target.is_none());

    let mut traversing = HistoryView::new(history_opened(3), 0);
    traversing.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    traversing.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 4,
        cell_width: 1,
    });
    traversing.cursor_target = None;
    traversing.pages.push_back(HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        start_col: 0,
        rows: vec![
            HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("A", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            },
            HistoryRow {
                width: 0,
                cells: Vec::new(),
                wrapped: false,
            },
            HistoryRow {
                width: 2,
                cells: vec![history_cell("B", 1), history_cell(" ", 1)],
                wrapped: false,
            },
        ],
    });
    dashboard.history = Some(traversing);
    assert!(matches!(
        dashboard.key(KeyCode::Down),
        ovrcr::tui::DashboardAction::Redraw
    ));
    let empty_cursor = dashboard.history.as_ref().unwrap().cursor.unwrap();
    assert_eq!(empty_cursor.point, HistoryCopyPoint { row: 1, col: 0 });
    assert_eq!(empty_cursor.row_width, 0);
    dashboard.history.as_mut().unwrap().top = 1;
    let mut empty_terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    empty_terminal
        .draw(|frame| {
            ovrcr::tui::render_history(
                frame,
                Rect::new(0, 0, 1, 1),
                dashboard.history.as_ref().unwrap(),
            )
        })
        .unwrap();
    assert_eq!(empty_terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(
        empty_terminal.backend().buffer()[(0, 0)].bg,
        Color::Rgb(30, 30, 46)
    );
    assert!(!empty_terminal.backend().cursor_visible());
    assert!(matches!(
        dashboard.key(KeyCode::Down),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 2, col: 0 }
    );
    assert!(matches!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        Some(HistoryCopyRange {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 1, col: 0 },
        })
    );
}

#[test]
fn history_cursor_stays_visible_while_predecessor_tile_loads() {
    let mut view = HistoryView::new(history_opened(40), 0);
    view.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 4,
        cell_width: 1,
    });
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 17,
        col: 2,
    }));
    view.pages.push_back(history_page(
        0,
        0,
        4,
        vec![
            history_cell("old", 1),
            history_cell("", 1),
            history_cell("", 1),
            history_cell("", 1),
        ],
    ));
    assert!(
        !view
            .resolve_cursor(TerminalSize { rows: 4, cols: 8 })
            .unwrap()
    );
    assert_eq!(
        view.cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 0 }
    );
    assert!(view.cursor_target.is_some());
}

#[test]
fn history_cursor_resolution_reveals_anchored_viewport() {
    let mut view = HistoryView::new(history_opened(40), 0);
    view.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 17,
        col: 3,
    }));
    view.pages.push_back(history_complete_page(
        16,
        0,
        16,
        8,
        vec![history_cell("", 1); 8],
    ));
    assert!(
        view.resolve_cursor(TerminalSize { rows: 4, cols: 4 })
            .unwrap()
    );
    assert_eq!(view.top, 14);
    assert_eq!(view.left, 0);
    assert_eq!(
        view.cursor.unwrap().point,
        HistoryCopyPoint { row: 17, col: 3 }
    );

    let mut unanchored = HistoryView::new(history_opened(2), 0);
    unanchored.left = 128;
    unanchored.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 0,
        col: 128,
    }));
    unanchored.pages.push_back(history_page(
        0,
        127,
        130,
        vec![
            history_cell("界", 2),
            history_cell("", 0),
            history_cell("x", 1),
        ],
    ));
    assert!(
        unanchored
            .resolve_cursor(TerminalSize { rows: 2, cols: 2 })
            .unwrap()
    );
    assert_eq!(unanchored.left, 127);
    assert_eq!(
        unanchored.cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 127 }
    );
}

#[test]
fn history_cursor_async_continuation_resolution_reveals_predecessor_tile() {
    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut view = HistoryView::new(history_opened(1), 0);
    view.left = 128;
    view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 130,
        cell_width: 1,
    });
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 0,
        col: 128,
    }));
    dashboard.history = Some(view);
    let first = dashboard
        .history_request_if_needed()
        .expect("continuation tile request");
    assert!(matches!(
        first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 128,
            ..
        }
    ));
    let predecessor = dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: 0,
                start_col: 128,
                rows: vec![HistoryRow {
                    width: 130,
                    cells: vec![history_cell("", 0), history_cell("tail", 1)],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("predecessor tile request");
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 0 }
    );
    assert!(matches!(
        predecessor.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 0,
            ..
        }
    ));
    let mut leader_cells = vec![history_cell("", 1); 128];
    leader_cells[127] = history_cell("界", 2);
    let follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: predecessor.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 130,
                cells: leader_cells,
                wrapped: false,
            }],
        }),
    });
    assert!(follow_up.is_empty());
    let view = dashboard.history.as_ref().unwrap();
    assert_eq!(view.left, 127);
    assert_eq!(
        view.cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 127 }
    );
    assert_eq!(view.cursor.unwrap().cell_width, 2);
    assert!(view.cursor_target.is_none());
}

#[test]
fn history_key_kinds_gate_actions() {
    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let top_before = dashboard.history.as_ref().unwrap().top;

    let repeat_enter =
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat);
    assert!(matches!(
        dashboard.key_action(repeat_enter),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(dashboard.history.as_ref().unwrap().top, top_before);

    let release_down =
        KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Release);
    assert!(matches!(
        dashboard.key_action(release_down),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(dashboard.history.as_ref().unwrap().top, top_before);

    let repeat_escape =
        KeyEvent::new_with_kind(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Repeat);
    assert!(matches!(
        dashboard.key_action(repeat_escape),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);

    let press_escape =
        KeyEvent::new_with_kind(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Press);
    assert!(matches!(
        dashboard.key_action(press_escape),
        ovrcr::tui::DashboardAction::EnterBrowse
    ));
}

#[test]
fn history_pending_keys_coalesce() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(400)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    assert!(matches!(
        dashboard.event_action(Event::Paste("paste while browsing history".into())),
        ovrcr::tui::DashboardAction::None
    ));
    let pending_before = dashboard
        .history
        .as_ref()
        .and_then(|view| view.pending.clone())
        .expect("one page pending");
    for _ in 0..100 {
        assert!(!matches!(
            dashboard.key(KeyCode::PageUp),
            ovrcr::tui::DashboardAction::Request(_)
        ));
    }
    let pending_after = dashboard
        .history
        .as_ref()
        .and_then(|view| view.pending.clone())
        .expect("one page remains pending");
    assert_eq!(pending_after.request_id, pending_before.request_id);
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 0);
    let (start_row, start_col) = match page_request.request {
        Request::HistoryPage {
            start_row,
            start_col,
            ..
        } => (start_row, start_col),
        request => panic!("unexpected history request: {request:?}"),
    };
    let follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_complete_page(
            start_row,
            start_col,
            pending_before.rows,
            20,
            vec![history_cell("tile", 1)],
        )),
    });
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 1);
    assert_eq!(follow_up.len(), 1);
    assert!(dashboard.history.as_ref().unwrap().pending.is_some());
    assert!(dashboard.history.as_ref().unwrap().pages.len() <= 16);

    let retry_id = dashboard
        .history
        .as_ref()
        .unwrap()
        .pending
        .as_ref()
        .unwrap()
        .request_id;
    let blocked = dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "history tile missing".into(),
        },
    });
    assert!(blocked.is_empty());
    assert!(dashboard.history.as_ref().unwrap().pending.is_none());
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("missing"))
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_id,
        response: Response::Ok,
    });
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("missing"))
    );
    let retry = match dashboard.key(KeyCode::Down) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected explicit retry request, got {action:?}"),
    };
    assert!(dashboard.error.is_none());

    let latest = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    let latest_top = dashboard.history.as_ref().unwrap().top;
    assert_eq!(latest.start_row, (latest_top / 16) * 16);
    assert_eq!(latest.start_col, 0);

    let initial_page = dashboard
        .history
        .as_ref()
        .unwrap()
        .pages
        .front()
        .unwrap()
        .clone();
    dashboard.panes[dashboard.focused_pane].size = TerminalSize {
        rows: 64,
        cols: 256,
    };
    dashboard.history.as_mut().unwrap().top = 1;
    dashboard.history.as_mut().unwrap().left = 1;
    let mut next = ovrcr::tui::DashboardAction::Request(retry);
    let mut starts = Vec::new();
    while let ovrcr::tui::DashboardAction::Request(request) = next {
        let (start_row, start_col, rows, cols) = match request.request {
            Request::HistoryPage {
                start_row,
                start_col,
                rows,
                cols,
                ..
            } => (start_row, start_col, rows, cols),
            request => panic!("unexpected history request: {request:?}"),
        };
        starts.push((start_row, start_col));
        next = dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: request.request_id,
                response: Response::HistoryRows(history_tile_page(
                    start_row, start_col, rows, cols,
                )),
            })
            .into_iter()
            .next()
            .map(ovrcr::tui::DashboardAction::Request)
            .unwrap_or(ovrcr::tui::DashboardAction::Redraw);
    }
    let expected_tiles = (0..5)
        .flat_map(|row| (0..3).map(move |col| (row * 16, col * 128)))
        .collect::<std::collections::BTreeSet<_>>();
    let cached_tiles = dashboard
        .history
        .as_ref()
        .unwrap()
        .pages
        .iter()
        .map(|page| (page.start_row, page.start_col))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(expected_tiles.is_subset(&cached_tiles));
    assert!(starts.iter().any(|(row, col)| *row == 0 && *col == 0));
    assert!(starts.iter().any(|(row, col)| *row == 64 && *col == 256));
    dashboard.history.as_mut().unwrap().top = 200;
    dashboard.history.as_mut().unwrap().left = 0;
    let eviction = match dashboard.key(KeyCode::Down) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected eviction request, got {action:?}"),
    };
    let follow_eviction = dashboard.handle_server_message(ServerMessage::Response {
        request_id: eviction.request_id,
        response: Response::HistoryRows(history_tile_page(192, 0, 16, 128)),
    });
    let eviction_two = follow_eviction
        .first()
        .cloned()
        .expect("next eviction tile");
    let (start_row, start_col, rows, cols) = match eviction_two.request {
        Request::HistoryPage {
            start_row,
            start_col,
            rows,
            cols,
            ..
        } => (start_row, start_col, rows, cols),
        request => panic!("unexpected eviction request: {request:?}"),
    };
    let second_page = history_tile_page(start_row, start_col, rows, cols);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: eviction_two.request_id,
        response: Response::HistoryRows(second_page.clone()),
    });
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 16);
    assert!(
        dashboard
            .history
            .as_ref()
            .unwrap()
            .pages
            .contains(&second_page)
    );
    assert!(
        !dashboard
            .history
            .as_ref()
            .unwrap()
            .pages
            .contains(&initial_page)
    );
}

#[test]
fn history_unanchored_retry_keys_reissue_failed_cursor_request() {
    for malformed in [false, true] {
        for retry_key in [KeyCode::Char('v'), KeyCode::Char('y')] {
            let mut dashboard = dashboard_fixture();
            assert!(matches!(
                dashboard.key(KeyCode::PageUp),
                ovrcr::tui::DashboardAction::Request(_)
            ));
            let requests = dashboard.handle_server_message(ServerMessage::Response {
                request_id: 1,
                response: Response::HistoryOpened(history_opened(40)),
            });
            let failed = requests.first().cloned().expect("cursor page request");
            let failed_bounds = match failed.request {
                Request::HistoryPage {
                    start_row,
                    start_col,
                    rows,
                    cols,
                    ..
                } => (start_row, start_col, rows, cols),
                request => panic!("unexpected cursor request: {request:?}"),
            };
            let failure_response = if malformed {
                Response::HistoryRows(HistoryRows {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                    start_row: failed_bounds.0,
                    start_col: failed_bounds.1,
                    rows: vec![HistoryRow {
                        width: 20,
                        cells: Vec::new(),
                        wrapped: false,
                    }],
                })
            } else {
                Response::Error {
                    code: ErrorCode::Internal,
                    message: "cursor page unavailable".into(),
                }
            };
            assert!(
                dashboard
                    .handle_server_message(ServerMessage::Response {
                        request_id: failed.request_id,
                        response: failure_response,
                    })
                    .is_empty()
            );

            let retry = match dashboard.key_action(KeyEvent::new_with_kind(
                retry_key,
                KeyModifiers::NONE,
                KeyEventKind::Press,
            )) {
                ovrcr::tui::DashboardAction::Request(request) => request,
                action => {
                    panic!("expected explicit cursor retry for {retry_key:?}, got {action:?}")
                }
            };
            assert!(matches!(retry.request, Request::HistoryPage { .. }));
            let follow_up = dashboard.handle_server_message(ServerMessage::Response {
                request_id: retry.request_id,
                response: Response::HistoryRows(history_complete_page(
                    failed_bounds.0,
                    failed_bounds.1,
                    failed_bounds.2,
                    20,
                    vec![history_cell("resolved", 1)],
                )),
            });
            assert!(
                follow_up
                    .iter()
                    .all(|request| matches!(request.request, Request::HistoryPage { .. }))
            );
            let view = dashboard.history.as_ref().unwrap();
            assert!(view.cursor_target.is_none());
            assert_eq!(view.cursor.unwrap().point.row, failed_bounds.0);
        }
    }
}

#[test]
fn copy_history_range_survives_live_eviction() {
    let (mut live_parser, mut frozen, old_lines) = numbered_history();
    let opened = frozen.opened().clone();
    let mut dashboard = screen_ready_dashboard(b"");
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(opened.clone()),
    });
    pump_frozen_history(&mut dashboard, &mut frozen, requests);

    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Home);
    for _ in 0..15 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Down);
    }
    for _ in 0..126 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Right);
    }
    let anchor_action = dashboard.key(KeyCode::Char('v'));
    assert_eq!(anchor_action, ovrcr::tui::DashboardAction::Redraw);
    let anchored = dashboard.history.as_ref().unwrap().anchor;
    assert_eq!(anchored, Some(HistoryCopyPoint { row: 15, col: 126 }));

    for _ in 0..18 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Down);
    }
    for _ in 0..3 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Right);
    }
    let range = dashboard.history.as_ref().unwrap().copy_range().unwrap();
    assert_eq!(range.anchor, HistoryCopyPoint { row: 15, col: 126 });
    assert_eq!(range.cursor, HistoryCopyPoint { row: 33, col: 129 });
    let view = dashboard.history.as_ref().unwrap();
    let cursor = view.cursor.unwrap();
    assert!(cursor.point.row >= view.top);
    assert!(
        cursor.point.row
            < view
                .top
                .saturating_add(dashboard.panes[dashboard.focused_pane].size.rows as u32)
    );
    assert!(cursor.point.col >= view.left);
    assert!(
        u32::from(cursor.point.col) + u32::from(cursor.cell_width)
            <= u32::from(view.left) + u32::from(dashboard.panes[dashboard.focused_pane].size.cols)
    );

    let before_resize = dashboard.history.as_ref().unwrap().copy_range();
    let resize = dashboard
        .view_request(
            outer_area_for_pane(TerminalSize { rows: 30, cols: 70 }),
            900,
        )
        .unwrap()
        .expect("resize should request a view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Screen {
            session: SessionId(1),
            revision: dashboard.view_revision,
            size: TerminalSize { rows: 30, cols: 70 },
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        before_resize
    );
    assert_eq!(
        dashboard.history.as_ref().unwrap().opened.snapshot,
        opened.snapshot
    );

    let live_bytes = (0..600)
        .map(|row| format!("LIVE_{row:03}\r\n"))
        .collect::<String>();
    live_parser.process(live_bytes.as_bytes());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        revision: 0,
        bytes: live_bytes.into_bytes(),
    }));
    assert!(dashboard.history.as_ref().unwrap().new_output);
    let refreshed = FrozenHistory::capture(
        SessionId(1),
        HistorySnapshotId(8),
        2,
        live_parser.screen().clone(),
    )
    .unwrap();
    let refreshed_page = {
        let mut refreshed = refreshed;
        refreshed.page(15, 1, 0, 128).unwrap()
    };
    let refreshed_row = refreshed_page.rows[0]
        .cells
        .iter()
        .filter(|cell| cell.width != 0)
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(!refreshed_row.contains("OLD_015"));

    for start_row in (0..opened.total_rows).step_by(16) {
        if let Some(view) = dashboard.history.as_mut() {
            view.top = start_row;
            view.cursor_target = None;
        }
        if let Some(request) = dashboard.history_request_if_needed() {
            pump_frozen_history(&mut dashboard, &mut frozen, vec![request]);
        }
    }
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 16);
    assert!(
        !dashboard
            .history
            .as_ref()
            .unwrap()
            .pages
            .iter()
            .any(|page| page.start_row == 0)
    );
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        Some(range)
    );

    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Char('y'));
    let expected = (range.anchor.row..=range.cursor.row)
        .map(|row| {
            let line = &old_lines[row as usize];
            let start = if row == range.anchor.row {
                usize::from(range.anchor.col)
            } else {
                0
            };
            let end = if row == range.cursor.row {
                usize::from(range.cursor.col) + 1
            } else {
                line.len()
            };
            line[start..end].to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        dashboard.take_pending_history_copy().as_deref(),
        Some(expected.as_str())
    );

    let reverse_anchor = dashboard.key(KeyCode::Char('v'));
    assert_eq!(reverse_anchor, ovrcr::tui::DashboardAction::Redraw);
    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Char('g'));
    for _ in 0..15 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Down);
    }
    for _ in 0..126 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Right);
    }
    let reverse_range = dashboard.history.as_ref().unwrap().copy_range().unwrap();
    assert_eq!(reverse_range.anchor, HistoryCopyPoint { row: 33, col: 129 });
    assert_eq!(reverse_range.cursor, HistoryCopyPoint { row: 15, col: 126 });
    let view = dashboard.history.as_ref().unwrap();
    let cursor = view.cursor.unwrap();
    assert!(cursor.point.row >= view.top);
    assert!(
        cursor.point.row
            < view
                .top
                .saturating_add(dashboard.panes[dashboard.focused_pane].size.rows as u32)
    );
    assert!(cursor.point.col >= view.left);
    assert!(
        u32::from(cursor.point.col) + u32::from(cursor.cell_width)
            <= u32::from(view.left) + u32::from(dashboard.panes[dashboard.focused_pane].size.cols)
    );
    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Char('y'));
    assert_eq!(
        dashboard.take_pending_history_copy().as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn copy_history_extraction_joins_wrap_and_skips_continuations() {
    let row15 = special_history_row(15);
    let mut direct = String::new();
    let mut pending_blanks = 0;
    let row15_slice = HistoryRow {
        width: row15.width,
        cells: row15.cells[125..].to_vec(),
        wrapped: row15.wrapped,
    };
    append_history_selection(
        &mut direct,
        &row15_slice,
        125,
        126,
        132,
        &mut pending_blanks,
    )
    .unwrap();
    assert_eq!(direct, "e\u{301}界  R");
    let mut omitted = String::new();
    let mut omitted_pending = 0;
    append_history_selection(
        &mut omitted,
        &HistoryRow {
            width: 4,
            cells: vec![
                history_cell("x", 1),
                history_cell("", 1),
                history_cell("", 1),
                history_cell("", 1),
            ],
            wrapped: false,
        },
        0,
        0,
        4,
        &mut omitted_pending,
    )
    .unwrap();
    assert_eq!(omitted, "x");
    assert_eq!(omitted_pending, 3);
    append_history_selection(
        &mut omitted,
        &HistoryRow {
            width: 1,
            cells: vec![history_cell("R", 1)],
            wrapped: false,
        },
        0,
        0,
        1,
        &mut omitted_pending,
    )
    .unwrap();
    assert_eq!(omitted, "x   R");
    assert_eq!(omitted_pending, 0);

    let opened = history_opened(18);
    let range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 15, col: 126 },
        cursor: HistoryCopyPoint { row: 17, col: 3 },
    };
    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut view = HistoryView::new(opened.clone(), 0);
    view.anchor = Some(range.anchor);
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 15,
        col: 128,
    }));
    dashboard.history = Some(view);
    let request = dashboard.history_request_if_needed().expect("cursor page");
    pump_special_history(&mut dashboard, vec![request]);
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 15, col: 127 }
    );
    dashboard.history.as_mut().unwrap().cursor_target =
        Some(ovrcr::tui::HistoryCursorTarget::At(range.cursor));
    if let Some(request) = dashboard.history_request_if_needed() {
        pump_special_history(&mut dashboard, vec![request]);
    }
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        range.cursor
    );

    dashboard.history.as_mut().unwrap().copy_job = Some(HistoryCopyJob::new(77, range));
    let request = dashboard.history_request_if_needed().expect("copy page");
    pump_special_history(&mut dashboard, vec![request]);
    let copied = dashboard
        .take_pending_history_copy()
        .expect("copy completion");
    assert_eq!(copied, "e\u{301}界  RC   D\nR ST");
    assert_eq!(copied.matches("界").count(), 1);
    assert_eq!(copied.matches('\n').count(), 1);
    assert!(copied.contains("e\u{301}"));

    let edge_range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 0, col: 127 },
        cursor: HistoryCopyPoint { row: 0, col: 255 },
    };
    let mut edge_dashboard = dashboard_with_history_copy(edge_range);
    let edge_first = edge_dashboard
        .history_request_if_needed()
        .expect("edge copy first tile");
    assert!(matches!(
        edge_first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 126,
            cols: 128,
            ..
        }
    ));
    let mut edge_first_cells = vec![history_cell("", 1); 128];
    edge_first_cells[1] = history_cell("界", 2);
    edge_first_cells[2] = history_cell("", 0);
    let edge_second = edge_dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: edge_first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 0,
                start_col: 126,
                rows: vec![HistoryRow {
                    width: 256,
                    cells: edge_first_cells,
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("edge copy second tile");
    assert!(matches!(
        edge_second.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 253,
            cols: 128,
            ..
        }
    ));
    let mut edge_second_cells = vec![history_cell("", 1); 3];
    edge_second_cells[2] = history_cell("R", 1);
    edge_dashboard.handle_server_message(ServerMessage::Response {
        request_id: edge_second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: 0,
            start_col: 253,
            rows: vec![HistoryRow {
                width: 256,
                cells: edge_second_cells,
                wrapped: false,
            }],
        }),
    });
    let expected_edge_text = format!("界{}R", " ".repeat(126));
    assert_eq!(
        edge_dashboard
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some(expected_edge_text.as_str())
    );
    let mut edge_output = Vec::new();
    let mut expected_edge_output = Vec::new();
    write_clipboard(&mut expected_edge_output, &expected_edge_text).unwrap();
    assert!(write_completed_history_copy(
        &mut edge_dashboard,
        &mut edge_output
    ));
    assert_eq!(edge_output, expected_edge_output);

    let deferred_edge_range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 0, col: 127 },
        cursor: HistoryCopyPoint { row: 0, col: 255 },
    };
    let mut deferred_edge = dashboard_with_history_copy(deferred_edge_range);
    let deferred_first = deferred_edge
        .history_request_if_needed()
        .expect("requested-edge first tile");
    assert!(matches!(
        deferred_first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 126,
            cols: 128,
            ..
        }
    ));
    let mut deferred_first_cells = vec![history_cell("", 1); 128];
    deferred_first_cells[127] = history_cell("界", 2);
    let deferred_second = deferred_edge
        .handle_server_message(ServerMessage::Response {
            request_id: deferred_first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 0,
                start_col: 126,
                rows: vec![HistoryRow {
                    width: 256,
                    cells: deferred_first_cells,
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("requested-edge overlapping tile");
    assert!(matches!(
        deferred_second.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 252,
            cols: 128,
            ..
        }
    ));
    assert!(
        deferred_edge
            .history
            .as_ref()
            .unwrap()
            .copy_completion
            .is_none()
    );
    let mut deferred_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut deferred_edge,
        &mut deferred_output
    ));
    assert!(deferred_output.is_empty());
    let mut deferred_second_cells = vec![history_cell("", 1); 4];
    deferred_second_cells[1] = history_cell("界", 2);
    deferred_second_cells[2] = history_cell("", 0);
    deferred_second_cells[3] = history_cell("R", 1);
    deferred_edge.handle_server_message(ServerMessage::Response {
        request_id: deferred_second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: 0,
            start_col: 252,
            rows: vec![HistoryRow {
                width: 256,
                cells: deferred_second_cells,
                wrapped: false,
            }],
        }),
    });
    let expected_deferred_edge = format!("{}界R", " ".repeat(126));
    assert_eq!(
        deferred_edge
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some(expected_deferred_edge.as_str())
    );
    let mut expected_deferred_output = Vec::new();
    write_clipboard(&mut expected_deferred_output, &expected_deferred_edge).unwrap();
    assert!(write_completed_history_copy(
        &mut deferred_edge,
        &mut deferred_output
    ));
    assert_eq!(deferred_output, expected_deferred_output);

    let boundary_range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 2, col: 1 },
    };
    let mut boundary_dashboard = dashboard_with_history_copy(boundary_range);
    let boundary_first = boundary_dashboard
        .history_request_if_needed()
        .expect("boundary first row");
    let boundary_second = boundary_dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: boundary_first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 0,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 1,
                    cells: vec![history_cell("A", 1)],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("boundary empty row");
    let boundary_third = boundary_dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: boundary_second.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 1,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 0,
                    cells: Vec::new(),
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("boundary final row");
    boundary_dashboard.handle_server_message(ServerMessage::Response {
        request_id: boundary_third.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: 2,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 2,
                cells: vec![history_cell("B", 1), history_cell(" ", 1)],
                wrapped: false,
            }],
        }),
    });
    assert_eq!(
        boundary_dashboard.take_pending_history_copy().as_deref(),
        Some("A\n\nB ")
    );
    let mut boundary_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut boundary_dashboard,
        &mut boundary_output
    ));
    assert!(boundary_output.is_empty());
    let mut boundary_dashboard = dashboard_with_history_copy(boundary_range);
    let mut boundary_requests = vec![
        boundary_dashboard
            .history_request_if_needed()
            .expect("boundary writer retry first row"),
    ];
    while let Some(request) = boundary_requests.pop() {
        let page = match request.request {
            Request::HistoryPage { start_row, .. } => match start_row {
                0 => HistoryRows {
                    session: opened.session,
                    snapshot: opened.snapshot,
                    start_row: 0,
                    start_col: 0,
                    rows: vec![HistoryRow {
                        width: 1,
                        cells: vec![history_cell("A", 1)],
                        wrapped: false,
                    }],
                },
                1 => HistoryRows {
                    session: opened.session,
                    snapshot: opened.snapshot,
                    start_row: 1,
                    start_col: 0,
                    rows: vec![HistoryRow {
                        width: 0,
                        cells: Vec::new(),
                        wrapped: false,
                    }],
                },
                2 => HistoryRows {
                    session: opened.session,
                    snapshot: opened.snapshot,
                    start_row: 2,
                    start_col: 0,
                    rows: vec![HistoryRow {
                        width: 2,
                        cells: vec![history_cell("B", 1), history_cell(" ", 1)],
                        wrapped: false,
                    }],
                },
                row => panic!("unexpected boundary row {row}"),
            },
            request => panic!("unexpected boundary request {request:?}"),
        };
        boundary_requests.extend(boundary_dashboard.handle_server_message(
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::HistoryRows(page),
            },
        ));
    }
    let mut expected_boundary_output = Vec::new();
    write_clipboard(&mut expected_boundary_output, "A\n\nB ").unwrap();
    assert!(write_completed_history_copy(
        &mut boundary_dashboard,
        &mut boundary_output
    ));
    assert_eq!(boundary_output, expected_boundary_output);

    let mut endpoint_dashboard = dashboard_fixture();
    endpoint_dashboard.mode = ovrcr::tui::InputMode::History;
    let mut endpoint_view = HistoryView::new(history_opened(3), 0);
    endpoint_view.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    endpoint_view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 2,
        col: 0,
    }));
    endpoint_view.pages.push_back(HistoryRows {
        session: opened.session,
        snapshot: opened.snapshot,
        start_row: 0,
        start_col: 0,
        rows: vec![
            HistoryRow {
                width: 1,
                cells: vec![history_cell("A", 1)],
                wrapped: false,
            },
            HistoryRow {
                width: 1,
                cells: vec![history_cell("B", 1)],
                wrapped: false,
            },
            HistoryRow {
                width: 0,
                cells: Vec::new(),
                wrapped: false,
            },
        ],
    });
    endpoint_dashboard.history = Some(endpoint_view);
    assert!(endpoint_dashboard.history_request_if_needed().is_none());
    assert_eq!(
        endpoint_dashboard.history.as_ref().unwrap().cursor,
        Some(HistoryCursor {
            point: HistoryCopyPoint { row: 2, col: 0 },
            row_width: 0,
            cell_width: 1,
        })
    );
    assert!(
        endpoint_dashboard
            .history
            .as_ref()
            .unwrap()
            .cursor_target
            .is_none()
    );
    let endpoint_request = match endpoint_dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected empty endpoint copy request, got {action:?}"),
    };
    let mut endpoint_requests = vec![endpoint_request];
    while let Some(request) = endpoint_requests.pop() {
        let page = match request.request {
            Request::HistoryPage { start_row, .. } => match start_row {
                0 => history_page(0, 0, 1, vec![history_cell("A", 1)]),
                1 => history_page(1, 0, 1, vec![history_cell("B", 1)]),
                2 => history_page(2, 0, 0, vec![]),
                row => panic!("unexpected empty-endpoint row {row}"),
            },
            request => panic!("unexpected empty-endpoint request {request:?}"),
        };
        endpoint_requests.extend(endpoint_dashboard.handle_server_message(
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::HistoryRows(page),
            },
        ));
    }
    assert_eq!(
        endpoint_dashboard
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some("A\nB\n")
    );
    let mut endpoint_output = Vec::new();
    let mut expected_endpoint_output = Vec::new();
    write_clipboard(&mut expected_endpoint_output, "A\nB\n").unwrap();
    assert!(write_completed_history_copy(
        &mut endpoint_dashboard,
        &mut endpoint_output
    ));
    assert_eq!(endpoint_output, expected_endpoint_output);

    let mut exact = HistoryCopyJob::new(
        88,
        HistoryCopyRange {
            session: opened.session,
            snapshot: opened.snapshot,
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 511, col: 127 },
        },
    );
    for row in 0..512 {
        let done = exact
            .consume_page(&HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: row,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 128,
                    cells: vec![history_cell("x", 1); 128],
                    wrapped: true,
                }],
            })
            .unwrap();
        assert_eq!(done, row == 511);
    }
    assert_eq!(exact.into_completion().text.len(), 65_536);

    let mut utf8_overflow = HistoryCopyJob::new(
        89,
        HistoryCopyRange {
            session: opened.session,
            snapshot: opened.snapshot,
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 511, col: 127 },
        },
    );
    for row in 0..512 {
        let mut cells = vec![history_cell("x", 1); 128];
        if row == 511 {
            cells[127] = history_cell("é", 1);
        }
        let result = utf8_overflow.consume_page(&HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: row,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 128,
                cells,
                wrapped: true,
            }],
        });
        if row < 511 {
            assert!(!result.unwrap());
        } else {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        }
    }
}

#[test]
fn copy_history_invalid_snapshot_never_emits_partial_text() {
    let range = HistoryCopyRange {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 1, col: 0 },
    };
    for code in [ErrorCode::Conflict, ErrorCode::NotFound] {
        let (mut dashboard, pending) = dashboard_copy_pending_after_first_tile(range);
        let mut output = Vec::new();
        assert!(!write_completed_history_copy(&mut dashboard, &mut output));
        assert!(output.is_empty());
        let release = dashboard.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::Error {
                code,
                message: "snapshot unavailable".into(),
            },
        });
        assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
        assert!(dashboard.history.is_none());
        assert!(dashboard.take_pending_history_copy().is_none());
        assert!(matches!(
            release.as_slice(),
            [ClientMessage {
                request: Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                },
                ..
            }]
        ));
        assert!(!write_completed_history_copy(&mut dashboard, &mut output));
        assert!(output.is_empty());
    }

    let (mut internal, pending) = dashboard_copy_pending_after_first_tile(range);
    let mut internal_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut internal,
        &mut internal_output
    ));
    assert!(internal_output.is_empty());
    let internal_release = internal.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "history backend unavailable".into(),
        },
    });
    assert!(internal_release.is_empty());
    assert_eq!(internal.mode, ovrcr::tui::InputMode::History);
    assert!(internal.history.as_ref().unwrap().copy_job.is_none());
    assert!(internal.history.as_ref().unwrap().pending.is_none());
    assert!(internal.take_pending_history_copy().is_none());
    assert!(internal.history_request_if_needed().is_none());
    let expected_retry = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "retry\nretry").unwrap();
        expected
    };
    assert_eq!(
        retry_simple_history_copy(&mut internal, "retry", &mut internal_output),
        expected_retry
    );
    assert!(!write_completed_history_copy(
        &mut internal,
        &mut internal_output
    ));
    assert_eq!(internal_output, expected_retry);

    let malformed_pages = vec![
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: Vec::new(),
        },
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![history_cell("short", 1)],
                wrapped: false,
            }],
        },
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("", 3),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            }],
        },
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("", 0),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            }],
        },
    ];
    for malformed_page in malformed_pages {
        let (mut malformed, pending) = dashboard_copy_pending_after_first_tile(range);
        let mut output = Vec::new();
        assert!(!write_completed_history_copy(&mut malformed, &mut output));
        assert!(output.is_empty());
        let _ = malformed.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::HistoryRows(malformed_page),
        });
        assert!(malformed.take_pending_history_copy().is_none());
        assert!(malformed.history.as_ref().unwrap().copy_job.is_none());
        assert!(malformed.history_request_if_needed().is_none());
        assert_eq!(
            retry_simple_history_copy(&mut malformed, "retry", &mut output),
            expected_retry
        );
        assert!(!write_completed_history_copy(&mut malformed, &mut output));
        assert_eq!(output, expected_retry);
    }

    for wrong in 0..5 {
        let (mut stale, pending) = dashboard_copy_pending_after_first_tile(range);
        let mut output = Vec::new();
        assert!(!write_completed_history_copy(&mut stale, &mut output));
        assert!(output.is_empty());
        let mut page = simple_copy_page(1, "stale");
        let request_id = match wrong {
            0 => pending.request_id.wrapping_add(1),
            _ => pending.request_id,
        };
        match wrong {
            1 => page.session = SessionId(99),
            2 => page.snapshot = HistorySnapshotId(99),
            3 => page.start_row = 2,
            4 => page.start_col = 1,
            _ => {}
        }
        let _ = stale.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::HistoryRows(page),
        });
        assert_eq!(
            stale
                .history
                .as_ref()
                .unwrap()
                .pending
                .as_ref()
                .unwrap()
                .request_id,
            pending.request_id
        );
        assert!(stale.history.as_ref().unwrap().copy_job.is_some());
        assert!(stale.take_pending_history_copy().is_none());
        assert!(!write_completed_history_copy(&mut stale, &mut output));
        assert!(output.is_empty());
        stale.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::HistoryRows(simple_copy_page(1, "ok")),
        });
        let expected = {
            let mut expected = Vec::new();
            write_clipboard(&mut expected, "partial\nok").unwrap();
            expected
        };
        assert!(write_completed_history_copy(&mut stale, &mut output));
        assert_eq!(output, expected);
        assert!(!write_completed_history_copy(&mut stale, &mut output));
    }

    let conflicting_range = HistoryCopyRange {
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 0, col: 259 },
        ..range
    };
    let mut conflicting = dashboard_with_history_copy(conflicting_range);
    let mut conflicting_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut conflicting,
        &mut conflicting_output
    ));
    assert!(conflicting_output.is_empty());
    let first = conflicting
        .history_request_if_needed()
        .expect("first wide copy page");
    let second = conflicting
        .handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: 0,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 260,
                    cells: vec![history_cell("x", 1); 128],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("second wide copy page");
    let _ = conflicting.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 127,
            rows: vec![HistoryRow {
                width: 260,
                cells: vec![history_cell("x", 1); 128],
                wrapped: true,
            }],
        }),
    });
    assert!(conflicting.history.as_ref().unwrap().copy_job.is_none());
    assert!(conflicting.take_pending_history_copy().is_none());
    assert!(conflicting.history_request_if_needed().is_none());
    assert!(conflicting_output.is_empty());
    assert!(!write_completed_history_copy(
        &mut conflicting,
        &mut conflicting_output
    ));
    assert!(conflicting_output.is_empty());
    let expected_wrapped_retry = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, &"x".repeat(260)).unwrap();
        expected
    };
    assert_eq!(
        retry_wide_history_copy(&mut conflicting, &mut conflicting_output),
        expected_wrapped_retry
    );
    assert!(!write_completed_history_copy(
        &mut conflicting,
        &mut conflicting_output
    ));

    let mut changed_width = dashboard_with_history_copy(conflicting_range);
    let mut changed_width_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut changed_width,
        &mut changed_width_output
    ));
    let first = changed_width
        .history_request_if_needed()
        .expect("changed-width first copy page");
    let second = changed_width
        .handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: 0,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 260,
                    cells: vec![history_cell("x", 1); 128],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("changed-width second copy page");
    let _ = changed_width.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 127,
            rows: vec![HistoryRow {
                width: 261,
                cells: vec![history_cell("x", 1); 128],
                wrapped: false,
            }],
        }),
    });
    assert!(changed_width.history.as_ref().unwrap().copy_job.is_none());
    assert!(changed_width.take_pending_history_copy().is_none());
    assert!(changed_width_output.is_empty());
    let expected_changed_width = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, &"x".repeat(260)).unwrap();
        expected
    };
    assert_eq!(
        retry_wide_history_copy(&mut changed_width, &mut changed_width_output),
        expected_changed_width
    );
    assert!(!write_completed_history_copy(
        &mut changed_width,
        &mut changed_width_output
    ));

    let mut collision = dashboard_fixture();
    collision.mode = ovrcr::tui::InputMode::History;
    let opened = history_opened(2);
    let mut cached = HistoryView::new(opened.clone(), 0);
    cached.cursor_target = None;
    cached.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    cached.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 4,
        cell_width: 1,
    });
    cached.pages.push_back(history_complete_page(
        0,
        0,
        2,
        4,
        vec![history_cell("canonical", 1)],
    ));
    cached.copy_job = Some(HistoryCopyJob::new(
        91,
        HistoryCopyRange {
            session: opened.session,
            snapshot: opened.snapshot,
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 0, col: 0 },
        },
    ));
    collision.history = Some(cached);
    let collision_request = collision
        .history_request_if_needed()
        .expect("copy collision request");
    let _ = collision.handle_server_message(ServerMessage::Response {
        request_id: collision_request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "copy")),
    });
    assert_eq!(
        collision.history.as_ref().unwrap().pages[0].rows[0].cells[0].text,
        "canonical"
    );
    assert_eq!(
        collision.take_pending_history_copy().as_deref(),
        Some("copy")
    );

    let mut incomplete = dashboard_fixture();
    incomplete.mode = ovrcr::tui::InputMode::History;
    let mut incomplete_view = HistoryView::new(history_opened(2), 0);
    incomplete_view.cursor_target = None;
    incomplete_view.pages.push_back(HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        start_col: 0,
        rows: vec![
            HistoryRow {
                width: 4,
                cells: vec![history_cell("a", 1); 4],
                wrapped: false,
            },
            HistoryRow {
                width: 4,
                cells: vec![history_cell("short", 1)],
                wrapped: false,
            },
        ],
    });
    incomplete.history = Some(incomplete_view);
    let mut incomplete_terminal = Terminal::new(TestBackend::new(180, 40)).unwrap();
    incomplete_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &incomplete, 0))
        .unwrap();
    let incomplete_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| incomplete_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(incomplete_text.contains("HISTORY · frozen · loading"));
    let incomplete_request = incomplete
        .history_request_if_needed()
        .expect("canonical viewport request");
    let incomplete_start = match incomplete_request.request {
        Request::HistoryPage { start_row, .. } => start_row,
        request => panic!("unexpected incomplete-cache request: {request:?}"),
    };
    let _ = incomplete.handle_server_message(ServerMessage::Response {
        request_id: incomplete_request.request_id,
        response: Response::HistoryRows(history_page(
            incomplete_start,
            0,
            4,
            vec![history_cell("short", 1)],
        )),
    });
    assert_eq!(incomplete.history.as_ref().unwrap().pages.len(), 1);
    assert!(incomplete.history_request_if_needed().is_none());

    let (mut switched, _) = dashboard_copy_pending_after_first_tile(range);
    let mut switched_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut switched,
        &mut switched_output
    ));
    switched.select_session(SessionId(2));
    assert_eq!(switched.mode, ovrcr::tui::InputMode::Browse);
    assert!(switched.history.is_none());
    assert!(switched_output.is_empty());

    let (mut removed, _) = dashboard_copy_pending_after_first_tile(range);
    let mut removed_output = Vec::new();
    let mut removed_hierarchy = removed.hierarchy.clone();
    removed_hierarchy.projects[1].workspaces[1]
        .sessions
        .retain(|session| session.id != SessionId(1));
    let _ = removed.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        removed_hierarchy,
    )));
    assert_eq!(removed.mode, ovrcr::tui::InputMode::Browse);
    assert!(removed.history.is_none());
    assert!(!write_completed_history_copy(
        &mut removed,
        &mut removed_output
    ));
    assert!(removed_output.is_empty());

    let (mut staged, pending) = dashboard_copy_pending_after_first_tile(range);
    let _ = staged.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "done")),
    });
    assert!(
        staged
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .is_some()
    );
    let mut staged_output = Vec::new();
    staged.select_session(SessionId(2));
    assert!(staged.history.is_none());
    assert!(staged.take_pending_history_copy().is_none());
    assert!(!write_completed_history_copy(
        &mut staged,
        &mut staged_output
    ));
    assert!(staged_output.is_empty());

    let mut dashboard = dashboard_with_history_copy(range);
    let request = dashboard.history_request_if_needed().expect("copy page");
    let next = dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "partial")),
    });
    let old_pending = next.first().cloned().expect("next row remains pending");
    let old_pending_page = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    let old_job_id = dashboard
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .id;
    let original_range = dashboard
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .range;
    let mut cancellation_output = Vec::new();
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_some());
    assert!(dashboard.take_pending_history_copy().is_none());
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut cancellation_output
    ));
    assert!(cancellation_output.is_empty());
    let cancelled = dashboard.key(KeyCode::Esc);
    assert_eq!(cancelled, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert_eq!(dashboard.copy_notice.as_deref(), Some("Copy cancelled"));
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&old_pending_page)
    );
    assert!(matches!(
        dashboard.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::Redraw
    ));
    let new_job = dashboard
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap();
    assert_ne!(new_job.id, old_job_id);
    assert_eq!(new_job.range, original_range);
    let new_job_id = new_job.id;
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&old_pending_page)
    );
    let late_requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: old_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "late")),
    });
    assert!(dashboard.take_pending_history_copy().is_none());
    assert_eq!(
        dashboard
            .history
            .as_ref()
            .unwrap()
            .copy_job
            .as_ref()
            .unwrap()
            .id,
        new_job_id
    );
    let new_request = late_requests
        .first()
        .cloned()
        .expect("late response retires old slot and starts replacement");
    assert!(matches!(
        new_request.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 0,
            ..
        }
    ));
    let replacement_pending = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: old_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "duplicate-late")),
    });
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&replacement_pending)
    );
    let next = dashboard.handle_server_message(ServerMessage::Response {
        request_id: new_request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "new")),
    });
    let final_request = next.first().cloned().expect("replacement second page");
    let _ = dashboard.handle_server_message(ServerMessage::Response {
        request_id: final_request.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "new")),
    });
    assert_eq!(
        dashboard
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some("new\nnew")
    );
    let expected_cancellation = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "new\nnew").unwrap();
        expected
    };
    assert!(write_completed_history_copy(
        &mut dashboard,
        &mut cancellation_output
    ));
    assert_eq!(cancellation_output, expected_cancellation);
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    ));
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_none());
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut cancellation_output
    ));
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert!(dashboard.take_pending_history_copy().is_none());

    let (mut changed_range, old_changed_pending) = dashboard_copy_pending_after_first_tile(range);
    let changed_old_pending_page = changed_range
        .history
        .as_ref()
        .unwrap()
        .pending
        .clone()
        .unwrap();
    let changed_old_job_id = changed_range
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .id;
    let changed_old_range = changed_range
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .range;
    let mut changed_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert_eq!(
        changed_range.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(!write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert!(changed_output.is_empty());
    assert!(matches!(
        changed_range.key(KeyCode::Down),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert!(matches!(
        changed_range.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert!(changed_range.history.as_ref().unwrap().copy_job.is_none());
    assert_eq!(
        changed_range.history.as_ref().unwrap().pending.as_ref(),
        Some(&changed_old_pending_page)
    );
    let changed_requests = changed_range.handle_server_message(ServerMessage::Response {
        request_id: old_changed_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "late")),
    });
    let changed_first = changed_requests
        .first()
        .cloned()
        .expect("changed-range cursor request");
    assert!(matches!(
        changed_first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 0,
            ..
        }
    ));
    let changed_next = changed_range.handle_server_message(ServerMessage::Response {
        request_id: changed_first.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 0,
            rows: vec![
                HistoryRow {
                    width: 4,
                    cells: vec![history_cell("old", 1); 4],
                    wrapped: false,
                },
                HistoryRow {
                    width: 4,
                    cells: vec![history_cell("late", 1); 4],
                    wrapped: false,
                },
            ],
        }),
    });
    assert!(changed_next.is_empty());
    assert!(
        changed_range
            .history
            .as_ref()
            .unwrap()
            .cursor_target
            .is_none()
    );
    assert_eq!(
        changed_range.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let changed_copy_request = match changed_range.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected changed-range copy request, got {action:?}"),
    };
    let changed_job = changed_range
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap();
    assert_ne!(changed_job.id, changed_old_job_id);
    assert_ne!(changed_job.range, changed_old_range);
    assert!(matches!(
        changed_copy_request.request,
        Request::HistoryPage {
            start_row: 1,
            start_col: 0,
            ..
        }
    ));
    let changed_copy_pending = changed_range.history.as_ref().unwrap().pending.clone();
    assert_eq!(
        changed_copy_pending.as_ref().unwrap().request_id,
        changed_copy_request.request_id
    );
    changed_range.handle_server_message(ServerMessage::Response {
        request_id: old_changed_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "duplicate-old")),
    });
    assert_eq!(
        changed_range.history.as_ref().unwrap().pending.as_ref(),
        changed_copy_pending.as_ref()
    );
    assert!(!write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert!(changed_output.is_empty());
    changed_range.handle_server_message(ServerMessage::Response {
        request_id: changed_copy_request.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "changed")),
    });
    let expected_changed = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "changed").unwrap();
        expected
    };
    assert!(write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert_eq!(changed_output, expected_changed);

    let (mut replacement, _) = dashboard_copy_pending_after_first_tile(range);
    let mut replacement_output = Vec::new();
    assert!(replacement.history.as_ref().unwrap().copy_job.is_some());
    assert!(replacement.history.as_ref().unwrap().pending.is_some());
    replacement.history_begin_request = Some(ovrcr::tui::PendingHistoryBegin {
        request_id: 90,
        session: SessionId(1),
        cancelled: false,
        at_tail: false,
    });
    let replacement_opened = HistoryOpened {
        snapshot: HistorySnapshotId(8),
        ..history_opened(2)
    };
    let replacement_requests = replacement.handle_server_message(ServerMessage::Response {
        request_id: 90,
        response: Response::HistoryOpened(replacement_opened.clone()),
    });
    assert_eq!(
        replacement.history.as_ref().unwrap().opened.snapshot,
        HistorySnapshotId(8)
    );
    assert!(replacement.history.as_ref().unwrap().copy_job.is_none());
    assert!(replacement.take_pending_history_copy().is_none());
    assert!(!write_completed_history_copy(
        &mut replacement,
        &mut replacement_output
    ));
    assert!(replacement_output.is_empty());
    assert!(replacement_requests.iter().any(|request| matches!(
        request.request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    )));

    let oversize_range = HistoryCopyRange {
        cursor: HistoryCopyPoint { row: 511, col: 127 },
        ..range
    };
    let (mut dashboard, mut request) = {
        let mut dashboard = dashboard_with_history_copy(oversize_range);
        let request = dashboard.history_request_if_needed().expect("copy page");
        (dashboard, request)
    };
    for row in 0..512 {
        let mut cells = vec![history_cell("x", 1); 128];
        if row == 511 {
            cells[127] = history_cell("é", 1);
        }
        let follow_up = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: row,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 128,
                    cells,
                    wrapped: true,
                }],
            }),
        });
        if row < 511 {
            request = follow_up.into_iter().next().expect("next valid copy page");
        } else {
            assert!(follow_up.iter().any(|request| matches!(
                request.request,
                Request::HistoryPage {
                    start_row: 0,
                    start_col: 0,
                    ..
                }
            )));
        }
    }
    assert!(dashboard.take_pending_history_copy().is_none());
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_none());
    assert!(
        dashboard
            .copy_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("65536"))
    );
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        Some(oversize_range)
    );
    let oversize_pending = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: oversize_pending.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "oversize retry slot retired".into(),
        },
    });
    assert!(dashboard.history.as_ref().unwrap().pending.is_none());
    let mut oversize_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut oversize_output
    ));
    assert!(oversize_output.is_empty());
    dashboard.history.as_mut().unwrap().cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 3 },
        row_width: 4,
        cell_width: 1,
    });
    let retry_request = match dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected smaller valid retry, got {action:?}"),
    };
    let _ = dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "ok")),
    });
    let expected_oversize_retry = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "ok").unwrap();
        expected
    };
    assert!(write_completed_history_copy(
        &mut dashboard,
        &mut oversize_output
    ));
    assert_eq!(oversize_output, expected_oversize_retry);
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut oversize_output
    ));
}

#[test]
fn history_render_preserves_cells_and_clips() {
    let mut view = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    let mut wide = history_cell("界", 2);
    wide.fg = HistoryColor::Rgb(1, 2, 3);
    view.pages.push_back(history_page(
        0,
        0,
        4,
        vec![
            wide,
            history_cell("", 0),
            history_cell("e\u{301}", 1),
            history_cell("X", 1),
        ],
    ));
    let mut terminal = Terminal::new(TestBackend::new(3, 1)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 3, 1), &view))
        .unwrap();
    terminal.show_cursor().unwrap();
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 3, 1), &view))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "界");
    assert_eq!(terminal.backend().buffer()[(1, 0)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(2, 0)].symbol(), "e\u{301}");
    assert_eq!(terminal.backend().buffer()[(0, 0)].fg, Color::Rgb(1, 2, 3));
    assert!(!terminal.backend().cursor_visible());

    view.left = 1;
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 3, 1), &view))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(1, 0)].symbol(), "e\u{301}");
    assert_eq!(terminal.backend().buffer()[(2, 0)].symbol(), "X");

    view.left = 0;
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 1, 1), &view))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), " ");

    let mut tiled_view = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    let mut left_cells = (0..128).map(|_| history_cell("", 1)).collect::<Vec<_>>();
    left_cells[127] = history_cell("界", 2);
    tiled_view
        .pages
        .push_back(history_page(0, 0, 256, left_cells));
    let mut right_cells = vec![history_cell("", 1); 128];
    right_cells[0] = history_cell("", 0);
    right_cells[1] = history_cell("R", 1);
    tiled_view
        .pages
        .push_back(history_page(0, 128, 256, right_cells));
    let mut tiled_terminal = Terminal::new(TestBackend::new(130, 1)).unwrap();
    tiled_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &tiled_view))
        .unwrap();
    assert_eq!(tiled_terminal.backend().buffer()[(127, 0)].symbol(), "界");
    assert_eq!(tiled_terminal.backend().buffer()[(128, 0)].symbol(), " ");
    assert_eq!(tiled_terminal.backend().buffer()[(129, 0)].symbol(), "R");
    tiled_view.left = 128;
    tiled_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 2, 1), &tiled_view))
        .unwrap();
    assert_eq!(tiled_terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(tiled_terminal.backend().buffer()[(1, 0)].symbol(), "R");

    let mut selected_view = tiled_view.clone();
    selected_view.left = 0;
    selected_view.anchor = Some(HistoryCopyPoint { row: 0, col: 127 });
    selected_view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 127 },
        row_width: 256,
        cell_width: 2,
    });
    selected_view.cursor_target = None;
    tiled_terminal
        .draw(|frame| {
            ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &selected_view);
            let buffer = frame.buffer_mut();
            assert_eq!(buffer[(127, 0)].fg, Color::Rgb(30, 30, 46));
            assert_eq!(buffer[(128, 0)].fg, Color::Rgb(30, 30, 46));
            assert_eq!(buffer[(127, 0)].bg, Color::Rgb(148, 226, 213));
            assert_eq!(buffer[(128, 0)].bg, Color::Rgb(148, 226, 213));
            assert_eq!(buffer[(129, 0)].symbol(), "R");
            assert_eq!(buffer[(129, 0)].bg, Color::Rgb(30, 30, 46));
        })
        .unwrap();
    assert_eq!(tiled_terminal.backend().buffer()[(127, 0)].symbol(), "界");
    assert_eq!(tiled_terminal.backend().buffer()[(128, 0)].symbol(), " ");

    let mut selected_output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut selected_output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 130, 1)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| {
                ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &selected_view)
            })
            .unwrap();
    }
    let mut selected_rendered = vt100::Parser::new(1, 130, 0);
    selected_rendered.process(&selected_output);
    assert_eq!(
        selected_rendered.screen().cell(0, 127).unwrap().contents(),
        "界"
    );
    assert_eq!(
        selected_rendered.screen().cell(0, 128).unwrap().contents(),
        ""
    );
    assert_eq!(
        selected_rendered.screen().cell(0, 129).unwrap().contents(),
        "R"
    );

    selected_view.left = 128;
    let mut left_edge_terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    left_edge_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 1, 1), &selected_view))
        .unwrap();
    assert_eq!(left_edge_terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(
        left_edge_terminal.backend().buffer()[(0, 0)].bg,
        Color::Rgb(30, 30, 46)
    );
    assert!(!left_edge_terminal.backend().cursor_visible());
    selected_view.left = 129;
    left_edge_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 1, 1), &selected_view))
        .unwrap();
    assert_eq!(left_edge_terminal.backend().buffer()[(0, 0)].symbol(), "R");
    assert_eq!(
        left_edge_terminal.backend().buffer()[(0, 0)].bg,
        Color::Rgb(30, 30, 46)
    );

    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut loading = ovrcr::tui::HistoryView::new(history_opened(100), 0);
    loading.pending = Some(ovrcr::tui::PendingHistoryPage {
        request_id: 2,
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        rows: 16,
        start_col: 0,
        cols: 128,
        purpose: ovrcr::tui::HistoryPagePurpose::Viewport,
    });
    dashboard.history = Some(loading);
    let mut dashboard_terminal = Terminal::new(TestBackend::new(180, 40)).unwrap();
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("loading"));
    assert!(dashboard_text.contains("row 1"));
    assert!(dashboard_text.contains("col 1"));
    let mut loaded_history = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    loaded_history.cursor_target = None;
    dashboard.history = Some(loaded_history);
    dashboard
        .history
        .as_mut()
        .unwrap()
        .pages
        .push_back(history_complete_page(
            0,
            0,
            1,
            4,
            vec![history_cell("payload", 1)],
        ));
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("HISTORY · frozen · loaded"));
    dashboard.history.as_mut().unwrap().copy_job = Some(HistoryCopyJob::new(
        4,
        HistoryCopyRange {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 0, col: 0 },
        },
    ));
    dashboard.history.as_mut().unwrap().pending = Some(ovrcr::tui::PendingHistoryPage {
        request_id: 5,
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        rows: 1,
        start_col: 0,
        cols: 128,
        purpose: ovrcr::tui::HistoryPagePurpose::Copy(4),
    });
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("HISTORY · frozen · loaded · copying"));
    dashboard.history.as_mut().unwrap().copy_job = None;
    dashboard.history.as_mut().unwrap().cursor_target =
        Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
            row: 0,
            col: 0,
        }));
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("HISTORY · frozen · loaded · cursor loading"));

    selected_view.left = 0;
    let mut roundtrip_view = tiled_view.clone();
    roundtrip_view.left = 0;
    roundtrip_view.anchor = Some(HistoryCopyPoint { row: 0, col: 127 });
    roundtrip_view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 127 },
        row_width: 256,
        cell_width: 2,
    });
    roundtrip_view.cursor_target = None;
    let mut crossterm_bytes = Vec::new();
    {
        crossterm::style::force_color_output(true);
        let backend = CrosstermBackend::new(&mut crossterm_bytes);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 132, 1)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| {
                ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &roundtrip_view)
            })
            .unwrap();
    }
    let mut round_trip = vt100::Parser::new(1, 130, 0);
    round_trip.process(&crossterm_bytes);
    let leader = round_trip.screen().cell(0, 127).unwrap();
    assert_eq!(leader.contents(), "界");
    assert_eq!(leader.fgcolor(), vt100::Color::Rgb(30, 30, 46));
    assert_eq!(leader.bgcolor(), vt100::Color::Rgb(148, 226, 213));
    assert!(
        round_trip
            .screen()
            .cell(0, 128)
            .unwrap()
            .is_wide_continuation()
    );
    assert_eq!(round_trip.screen().cell(0, 129).unwrap().contents(), "R");
    crossterm::style::force_color_output(false);

    let mut clipped_terminal = Terminal::new(TestBackend::new(128, 1)).unwrap();
    clipped_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 128, 1), &selected_view))
        .unwrap();
    assert_eq!(clipped_terminal.backend().buffer()[(127, 0)].symbol(), " ");
    assert_eq!(
        clipped_terminal.backend().buffer()[(127, 0)].bg,
        Color::Rgb(30, 30, 46)
    );
    assert!(!clipped_terminal.backend().cursor_visible());
    let mut empty_history = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    empty_history.cursor_target = None;
    dashboard.history = Some(empty_history);
    dashboard
        .history
        .as_mut()
        .unwrap()
        .pages
        .push_back(history_page(0, 0, 0, vec![]));
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("empty"));
}

#[test]
fn history_cancelled_begin_releases_late_snapshot() {
    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert_eq!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert!(
        dashboard
            .history_begin_request
            .as_ref()
            .is_some_and(|pending| pending.cancelled)
    );
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.history.is_none());
    assert_eq!(release.len(), 1);
    assert_eq!(release[0].request_id, 2);
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
    assert!(dashboard.history_begin_request.is_none());
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));

    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert_eq!(
        dashboard.ctrl('g'),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.history.is_none());
    assert_eq!(release.len(), 1);
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
    assert!(dashboard.history_begin_request.is_none());
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));

    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(
        dashboard.ctrl('g'),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.history.is_none());
    assert_eq!(release.len(), 1);
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
    assert!(dashboard.history_begin_request.is_none());
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));
}

#[test]
fn pause_resume_dense_status_has_priority() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Paused;
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].activity = AgentActivity::Busy;

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    assert_eq!(row(3).trim_end(), "  P local");
    let rendered = (0..40)
        .map(|y| {
            (0..120)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("pid: 555  elapsed: 0m  agent busy  paused"));
    assert!(rendered.lines().last().unwrap().contains(" r "));
    assert!(
        !rendered
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .any(|key| key == "p")
    );
    assert!(
        rendered
            .lines()
            .last()
            .is_some_and(|footer| footer.split_whitespace().any(|key| key == "q"))
    );
    assert!(
        rendered
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .any(|key| key == "t")
    );

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let terminal_footer = (0..120)
        .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
        .collect::<String>();
    assert!(terminal_footer.contains("Terminal mode"));
    assert!(terminal_footer.contains("Ctrl-g"));
    assert!(!terminal_footer.contains("q detach"));

    dashboard.mode = ovrcr::tui::InputMode::Browse;
    let mut narrow = Terminal::new(TestBackend::new(40, 20)).unwrap();
    narrow
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let footer = (0..40)
        .map(|x| narrow.backend().buffer()[(x, 19)].symbol())
        .collect::<String>();
    assert!(
        footer.split_whitespace().any(|key| key == "r"),
        "narrow footer was {footer:?}"
    );
}

#[test]
fn history_waits_for_coalesced_focus_view_before_capture() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let split = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 901)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, split);
    let captured_session = dashboard.focused_session().unwrap();
    assert!(dashboard.focus_pane(0));
    let pending = dashboard
        .view_request(dashboard.outer_area, 902)
        .unwrap()
        .unwrap();
    assert!(dashboard.focus_pane(1));
    assert!(
        dashboard
            .view_request(dashboard.outer_area, 903)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.history_begin_request.is_none());
    assert_eq!(
        dashboard.error.as_deref(),
        Some("Pane is loading; retry history")
    );

    let Request::SetView { view } = pending.request else {
        panic!("expected view")
    };
    deliver_all_view_screens(&mut dashboard, pending.request_id, &view);
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::Ok,
    });
    let [replacement] = outgoing.as_slice() else {
        panic!("expected coalesced view: {outgoing:?}")
    };
    assert!(matches!(replacement.request, Request::SetView { .. }));
    assert_eq!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.history_begin_request.is_none());
    acknowledge_all_view_targets(&mut dashboard, replacement.clone());
    let ovrcr::tui::DashboardAction::Request(begin) = dashboard.key(KeyCode::PageUp) else {
        panic!("history must open after the committed focus is acknowledged");
    };
    assert_eq!(
        begin.request,
        Request::HistoryBegin {
            session: captured_session
        }
    );
    assert_eq!(
        dashboard.history_begin_request.as_ref().unwrap().session,
        captured_session
    );
}

#[test]
fn ux_history_footer_names_the_actual_escape_action() {
    let range = HistoryCopyRange {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 1, col: 1 },
    };
    let mut dashboard = dashboard_with_history_copy(range);
    for width in [40, 80, 120] {
        let text = rendered_footer(&dashboard, width);
        assert!(text.contains("Esc Cancel copy"), "{text}");
        assert!(!text.contains("y Copy"), "{text}");
    }
    dashboard.key(KeyCode::Esc);
    assert!(dashboard.history.as_ref().unwrap().anchor.is_some());
    dashboard.copy_notice = None;
    for width in [40, 80, 120] {
        let text = rendered_footer(&dashboard, width);
        assert!(text.contains("Esc Back"), "{text}");
    }
    assert!(rendered_footer(&dashboard, 120).contains("y Copy"));
    dashboard.history.as_mut().unwrap().anchor = None;
    assert!(rendered_footer(&dashboard, 80).contains("v Select"));
    dashboard.key(KeyCode::Esc);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    let mut copy = copy_ready_dashboard();
    copy.key(KeyCode::Char('['));
    let text = rendered_footer(&copy, 120);
    for label in ["COPY", "Esc Back", "v Select", "y Copy"] {
        assert!(text.contains(label), "{text}");
    }
}

#[test]
fn copy_mouse_drag_normalizes_wide_cells_and_visible_copy_close_capture_input() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut d = screen_ready_dashboard("a界z".as_bytes());
    d.key(KeyCode::Char('['));
    let area = d.outer_area;
    let pane = d.pane_rects(area)[0].terminal;
    let mouse = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    assert!(d.mouse_capture_required());
    d.mouse_action(
        mouse(MouseEventKind::Down(MouseButton::Left), pane.x + 2, pane.y),
        area,
    );
    d.mouse_action(
        mouse(MouseEventKind::Drag(MouseButton::Left), pane.x + 3, pane.y),
        area,
    );
    d.mouse_action(
        mouse(MouseEventKind::Up(MouseButton::Left), pane.x + 3, pane.y),
        area,
    );
    assert_eq!(
        d.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("界z")
    );
    let action = d.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 16,
            area.y,
        ),
        area,
    );
    assert_eq!(action, ovrcr::tui::DashboardAction::CopyText("界z".into()));
    assert_eq!(d.focused_session(), Some(SessionId(1)));
    d.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 6,
            area.y,
        ),
        area,
    );
    assert!(d.copy.is_none());
    assert_eq!(d.mode, ovrcr::tui::InputMode::Browse);
}
#[test]
fn history_mouse_drag_uses_frozen_offsets_and_cancels_stale_completion() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_, mut frozen, _) = numbered_history();
    let mut d = screen_ready_dashboard(b"");
    let begin = d.key(KeyCode::PageUp);
    let ovrcr::tui::DashboardAction::Request(begin) = begin else {
        panic!("begin")
    };
    let requests = d.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(frozen.opened().clone()),
    });
    pump_frozen_history(&mut d, &mut frozen, requests);
    let area = d.outer_area;
    let pane = d.pane_rects(area)[0].terminal;
    let mouse = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    let top = d.history.as_ref().unwrap().top;
    d.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            pane.x + 1,
            pane.y + 1,
        ),
        area,
    );
    d.mouse_action(
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            pane.x + 4,
            pane.y + 2,
        ),
        area,
    );
    d.mouse_action(
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            pane.x + 4,
            pane.y + 2,
        ),
        area,
    );
    let range = d.history.as_ref().unwrap().copy_range().unwrap();
    assert_eq!(
        range.anchor,
        HistoryCopyPoint {
            row: top + 1,
            col: 1
        }
    );
    assert_eq!(
        range.cursor,
        HistoryCopyPoint {
            row: top + 2,
            col: 4
        }
    );
    let action = d.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 16,
            area.y,
        ),
        area,
    );
    if let ovrcr::tui::DashboardAction::Request(request) = action {
        pump_frozen_history(&mut d, &mut frozen, vec![request]);
    }
    assert!(d.history.as_ref().unwrap().copy_completion.is_some());
    // New selection invalidates a queued clipboard completion before it can emit.
    d.mouse_action(
        mouse(MouseEventKind::Down(MouseButton::Left), pane.x + 3, pane.y),
        area,
    );
    assert!(!write_completed_history_copy(&mut d, &mut Vec::new()));
    d.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 6,
            area.y,
        ),
        area,
    );
    assert!(d.history.is_none());
}

#[test]
fn copy_mouse_release_position_and_focus_loss_bound_the_gesture() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut d = screen_ready_dashboard(b"abcdef");
    d.key(KeyCode::Char('['));
    let area = d.outer_area;
    let pane = d.pane_rects(area)[0].terminal;
    let mouse = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    d.mouse_action(
        mouse(MouseEventKind::Down(MouseButton::Left), pane.x, pane.y),
        area,
    );
    d.mouse_action(
        mouse(MouseEventKind::Up(MouseButton::Left), pane.x + 2, pane.y),
        area,
    );
    assert_eq!(
        d.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("abc")
    );
    d.mouse_action(
        mouse(MouseEventKind::Drag(MouseButton::Left), pane.x + 5, pane.y),
        area,
    );
    assert_eq!(
        d.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("abc")
    );
    d.mouse_action(
        mouse(MouseEventKind::Down(MouseButton::Left), pane.x + 1, pane.y),
        area,
    );
    d.event_action(Event::FocusLost);
    d.event_action(Event::FocusGained);
    d.mouse_action(
        mouse(MouseEventKind::Drag(MouseButton::Left), pane.x + 5, pane.y),
        area,
    );
    assert_eq!(
        d.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("b")
    );
    // This is the view synchronization called by the shared client resize path.
    d.view_request(Rect::new(area.x, area.y, area.width + 1, area.height), 900)
        .unwrap();
    assert!(d.copy.is_none());
}

#[test]
fn whichkey_acquisition_cancels_copy_and_history_drags() {
    use ovrcr::tui::DashboardAction;

    let mouse = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };

    let mut copy = screen_ready_dashboard(b"abcdef");
    copy.key(KeyCode::Char('['));
    let copy_area = copy.outer_area;
    let copy_pane = copy.pane_rects(copy_area)[0].terminal;
    copy.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            copy_pane.x,
            copy_pane.y,
        ),
        copy_area,
    );
    copy.event_action(Event::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    copy.event_action(Event::Mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        copy_pane.x + 5,
        copy_pane.y,
    )));
    copy.key(KeyCode::Esc);
    copy.mouse_action(
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            copy_pane.x + 5,
            copy_pane.y,
        ),
        copy_area,
    );
    assert_eq!(
        copy.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("a")
    );

    let (_, mut frozen, _) = numbered_history();
    let mut history = screen_ready_dashboard(b"");
    let DashboardAction::Request(begin) = history.key(KeyCode::PageUp) else {
        panic!("history begin");
    };
    let requests = history.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(frozen.opened().clone()),
    });
    pump_frozen_history(&mut history, &mut frozen, requests);
    let history_area = history.outer_area;
    let history_pane = history.pane_rects(history_area)[0].terminal;
    history.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            history_pane.x + 1,
            history_pane.y + 1,
        ),
        history_area,
    );
    let before = history.history.as_ref().unwrap().copy_range().unwrap();
    history.event_action(Event::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    history.event_action(Event::Mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        history_pane.x + 4,
        history_pane.y + 2,
    )));
    history.key(KeyCode::Esc);
    history.mouse_action(
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            history_pane.x + 4,
            history_pane.y + 2,
        ),
        history_area,
    );
    assert_eq!(history.history.as_ref().unwrap().copy_range(), Some(before));
}

#[test]
fn history_mouse_bounds_match_offset_painted_cells_and_resize_keeps_capture() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut d = screen_ready_dashboard(b"");
    let area = Rect::new(7, 5, 340, 90);
    d.view_request(area, 900).unwrap();
    let mut view = ovrcr::tui::HistoryView::new(history_opened(100), 5);
    view.left = 7;
    let mut cells = vec![history_cell("x", 1); 300];
    cells[8] = history_cell("界", 2);
    cells[9] = history_cell("", 0);
    cells[262] = history_cell("界", 2);
    cells[263] = history_cell("", 0);
    view.pages.push_back(HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        start_col: 0,
        rows: (0..100)
            .map(|_| HistoryRow {
                width: 300,
                cells: cells.clone(),
                wrapped: false,
            })
            .collect(),
    });
    d.history = Some(view);
    d.mode = ovrcr::tui::InputMode::History;
    let pane = d.pane_rects(area)[0].terminal;
    let mouse = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    d.mouse_action(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            pane.x + 2,
            pane.y + 1,
        ),
        area,
    );
    d.mouse_action(
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            pane.x + 3,
            pane.y + 2,
        ),
        area,
    );
    let range = d.history.as_ref().unwrap().copy_range().unwrap();
    assert_eq!(range.anchor, HistoryCopyPoint { row: 6, col: 8 });
    assert_eq!(range.cursor, HistoryCopyPoint { row: 7, col: 10 });
    // Metadata, clipped wide glyph, columns beyond 256, and rows beyond 64 are not painted text.
    for (x, y) in [
        (pane.x, pane.y - 1),
        (pane.x + 255, pane.y),
        (pane.x + 256, pane.y),
        (pane.x, pane.y + 64),
    ] {
        assert_eq!(
            d.mouse_action(mouse(MouseEventKind::Down(MouseButton::Left), x, y), area),
            ovrcr::tui::DashboardAction::None
        );
        assert_eq!(d.history.as_ref().unwrap().copy_range(), Some(range));
    }
    let session = d.history.as_ref().unwrap().opened.session;
    d.view_request(Rect::new(7, 5, 100, 20), 901).unwrap();
    assert_eq!(d.history.as_ref().unwrap().copy_range(), Some(range));
    assert_eq!(d.history.as_ref().unwrap().opened.session, session);
    d.select_session(SessionId(5));
    assert!(d.history.is_none());
}
