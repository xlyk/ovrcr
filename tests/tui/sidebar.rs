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

fn rendered_rows_at(
    dashboard: &Dashboard,
    width: u16,
    height: u16,
    now_unix_ms: u64,
) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, dashboard, now_unix_ms))
        .unwrap();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn sidebar_text_width(terminal: &Terminal<TestBackend>) -> u16 {
    (0..=39)
        .find(|column| terminal.backend().buffer()[(*column, 1)].symbol() == "│")
        .unwrap_or_else(|| 39.min(terminal.backend().buffer().area.width))
}

fn sidebar_row_at_width(terminal: &Terminal<TestBackend>, y: u16, width: u16) -> String {
    (0..width)
        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
        .collect()
}

fn sidebar_row_with(terminal: &Terminal<TestBackend>, text: &str) -> u16 {
    (0..terminal.backend().buffer().area.height)
        .find(|row| {
            sidebar_row_at_width(terminal, *row, sidebar_text_width(terminal)).contains(text)
        })
        .unwrap_or_else(|| panic!("missing sidebar row containing {text:?}"))
}

fn sidebar_rows_with(terminal: &Terminal<TestBackend>, text: &str) -> Vec<u16> {
    (0..terminal.backend().buffer().area.height)
        .filter(|row| {
            sidebar_row_at_width(terminal, *row, sidebar_text_width(terminal)).contains(text)
        })
        .collect()
}

fn dashboard_sidebar_row(dashboard: &Dashboard, text: &str, width: u16, height: u16) -> u16 {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    sidebar_row_with(&terminal, text)
}

fn dashboard_sidebar_rows(dashboard: &Dashboard, text: &str, width: u16, height: u16) -> Vec<u16> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    sidebar_rows_with(&terminal, text)
}

fn assert_selected_sidebar_row(terminal: &Terminal<TestBackend>, text: &str) {
    let row = selection_bar_row(terminal).expect("a sidebar row is selected");
    assert!(
        sidebar_row(terminal, row).contains(text),
        "selected row should contain {text:?}, got {:?}",
        sidebar_row(terminal, row)
    );
}
#[test]
fn usage_section_renders_eight_canonical_session_counts_and_server_total() {
    let mut hierarchy = HierarchySnapshot {
        usage: ovrcr_protocol::UsageSnapshot {
            total_tokens: 4_600_000,
            server_started_unix_ms: 0,
        },
        projects: vec![ProjectSummary {
            name: "usage-fixture".into(),
            workspaces: vec![WorkspaceSummary {
                project: "usage-fixture".into(),
                id: "work".into(),
                name: "work".into(),
                root: true,
                warning: None,
                path: PathBuf::from("/tmp/usage-fixture"),
                sessions: Vec::new(),
            }],
        }],
    };
    let states = [
        ("working", SessionPhase::Running, AgentActivity::Busy),
        ("input", SessionPhase::Running, AgentActivity::WaitingInput),
        ("ready", SessionPhase::Running, AgentActivity::ResponseReady),
        ("idle", SessionPhase::Running, AgentActivity::Idle),
        ("error", SessionPhase::Running, AgentActivity::Error),
        ("paused", SessionPhase::Paused, AgentActivity::Busy),
        (
            "exited",
            SessionPhase::Exited {
                code: Some(0),
                signal: None,
            },
            AgentActivity::Unknown,
        ),
        ("stopped", SessionPhase::Stopped, AgentActivity::Unknown),
        (
            "interrupted",
            SessionPhase::Interrupted,
            AgentActivity::Unknown,
        ),
        ("unknown", SessionPhase::Running, AgentActivity::Unknown),
    ];
    hierarchy.projects[0].workspaces[0].sessions = states
        .into_iter()
        .enumerate()
        .map(|(index, (name, phase, activity))| {
            let mut session = session_summary(
                index as u64 + 1,
                "usage-fixture",
                "work",
                name,
                "sh",
                Some(index as u32 + 1),
                0,
            );
            session.phase = phase;
            session.activity = activity;
            session
        })
        .collect();
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 11_520_000))
        .unwrap();
    let sidebar = (0..40)
        .map(|row| sidebar_row_at_width(&terminal, row, sidebar_text_width(&terminal)))
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "USAGE",
        "4.6M tokens",
        "3h12m",
        "1 working",
        "1 input",
        "1 ready",
        "1 idle",
        "2 error",
        "1 paused",
        "2 exited",
        "1 unknown",
    ] {
        assert!(
            sidebar.contains(expected),
            "missing {expected:?}: {sidebar}"
        );
    }
}

#[test]
fn usage_compacts_to_one_line_and_then_hides_when_the_sidebar_is_short() {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.usage = ovrcr_protocol::UsageSnapshot {
        total_tokens: 4_600_000,
        server_started_unix_ms: 0,
    };
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_hierarchy(hierarchy);
    let mut saw_compact = false;
    let mut saw_hidden = false;

    for height in 8..=40 {
        dashboard.install_area(Rect::new(0, 0, 120, height));
        let mut terminal = Terminal::new(TestBackend::new(120, height)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let screen = (0..height)
            .map(|row| sidebar_row_at_width(&terminal, row, sidebar_text_width(&terminal)))
            .collect::<Vec<_>>()
            .join("\n");
        if screen.contains("Usage · 5 sessions · 4.6M tokens") {
            saw_compact = true;
            assert!(
                !screen.contains("USAGE"),
                "compact form should replace full rows"
            );
        }
        if height < 20 && !screen.contains("Usage") && !screen.contains("USAGE") {
            saw_hidden = true;
        }
    }
    assert!(saw_compact, "Usage did not reach its compact form");
    assert!(saw_hidden, "Usage did not hide on a short sidebar");
}

#[test]
fn usage_zero_counters_are_dimmed_and_the_setting_can_hide_the_section() {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.usage = ovrcr_protocol::UsageSnapshot {
        total_tokens: 0,
        server_started_unix_ms: 0,
    };
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let (zero_x, zero_y) = rendered_cell_sequence(&terminal, "⠹ 0 working", 120, 40).unwrap();
    let (positive_x, positive_y) =
        rendered_cell_sequence(&terminal, "- 4 unknown", 120, 40).unwrap();
    assert_ne!(
        terminal.backend().buffer()[(zero_x, zero_y)].fg,
        terminal.backend().buffer()[(positive_x, positive_y)].fg,
        "a zero counter should use the dim color"
    );

    dashboard.install_settings(ovrcr::protocol::Settings {
        usage: ovrcr::protocol::UsageSettings { sidebar: false },
        ..Default::default()
    });
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let screen = (0..40)
        .map(|row| sidebar_row_at_width(&terminal, row, sidebar_text_width(&terminal)))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!screen.contains("USAGE"));
    assert!(!screen.contains("Usage ·"));
}

fn rendered_cell_sequence(
    terminal: &Terminal<TestBackend>,
    needle: &str,
    width: u16,
    height: u16,
) -> Option<(u16, u16)> {
    let symbols: Vec<_> = needle.chars().map(|ch| ch.to_string()).collect();
    (0..height).find_map(|row| {
        (0..=width.saturating_sub(symbols.len() as u16)).find_map(|column| {
            symbols
                .iter()
                .enumerate()
                .all(|(offset, symbol)| {
                    terminal.backend().buffer()[(column + offset as u16, row)].symbol() == symbol
                })
                .then_some((column, row))
        })
    })
}

#[test]
fn row_create_buttons_render_only_on_hover_for_projects_and_workspaces() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    let rows = rendered_rows(&dashboard, area.width, area.height);
    let project_row = rows
        .iter()
        .position(|line| line.contains("CONSIGINT"))
        .expect("project row");
    let workspace_row = rows
        .iter()
        .position(|line| line.contains("auth"))
        .expect("workspace row");
    assert!(!rows[project_row].contains("[+]"));
    assert!(!rows[workspace_row].contains("[+]"));

    hover(&mut dashboard, 5, project_row as u16, area);
    let rows = rendered_rows(&dashboard, area.width, area.height);
    assert!(
        rows[project_row].contains("[+]"),
        "project row should show its create button on hover: {}",
        rows[project_row]
    );
    assert!(!rows[workspace_row].contains("[+]"));

    hover(&mut dashboard, 5, workspace_row as u16, area);
    let rows = rendered_rows(&dashboard, area.width, area.height);
    let plus = rows[workspace_row]
        .find("[+]")
        .expect("hovered workspace create button");
    let close = rows[workspace_row]
        .find("[x]")
        .expect("hovered workspace close button");
    assert!(
        plus < close,
        "workspace buttons must be [+][x]: {}",
        rows[workspace_row]
    );
}

#[test]
fn m_switches_to_recent_activity_with_newest_agents_grouped_by_project() {
    use ovrcr::protocol::{Request, SessionKind, Settings, SidebarView};

    let now = 20_000_000;
    let mut hierarchy = fixture_hierarchy();
    for session in hierarchy
        .projects
        .iter_mut()
        .flat_map(|project| project.workspaces.iter_mut())
        .flat_map(|workspace| workspace.sessions.iter_mut())
    {
        match session.id.0 {
            1 => {
                session.kind = SessionKind::Agent {
                    name: "codex".into(),
                };
                session.label = "codex".into();
                session.title = Some("review newest".into());
                session.last_activity_unix_ms = Some(now - 60_000);
            }
            2 => {
                session.kind = SessionKind::Agent {
                    name: "claude".into(),
                };
                session.label = "claude".into();
                session.title = Some("implement older".into());
                session.last_activity_unix_ms = Some(now - 3 * 60 * 60_000);
            }
            _ => {}
        }
    }
    let mut alpha_workspace = hierarchy.projects[1].workspaces[1].clone();
    alpha_workspace.project = "alpha".into();
    alpha_workspace.name = "feature/alpha".into();
    alpha_workspace.id = "alpha-workspace".into();
    alpha_workspace.sessions.truncate(1);
    alpha_workspace.sessions[0].project = "alpha".into();
    alpha_workspace.sessions[0].workspace = "alpha-workspace".into();
    alpha_workspace.sessions[0].id = SessionId(6);
    alpha_workspace.sessions[0].run = ovrcr::session::SessionRunId(6);
    alpha_workspace.sessions[0].name = "alpha-agent".into();
    alpha_workspace.sessions[0].title = Some("alpha agent".into());
    alpha_workspace.sessions[0].kind = SessionKind::Agent {
        name: "codex".into(),
    };
    alpha_workspace.sessions[0].label = "codex".into();
    alpha_workspace.sessions[0].last_activity_unix_ms = Some(now - 10_000);
    hierarchy.projects.push(ProjectSummary {
        name: "alpha".into(),
        workspaces: vec![alpha_workspace],
    });
    hierarchy.projects.push(ProjectSummary {
        name: "empty-project".into(),
        workspaces: vec![WorkspaceSummary {
            project: "empty-project".into(),
            name: "empty".into(),
            id: "empty".into(),
            root: false,
            warning: None,
            path: PathBuf::from("/tmp/empty"),
            sessions: Vec::new(),
        }],
    });
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    dashboard.install_screen(SessionId(1), &[]);

    let tree = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(
        tree.contains("SPACELIFT-AGENT"),
        "default view is the tree:\n{tree}"
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('m')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    assert!(matches!(
        request.request,
        Request::SetSetting { ref path, ref value }
            if path == "sidebar_view" && value.as_deref() == Some("\"recent\"")
    ));
    let settings = Settings {
        sidebar_view: SidebarView::Recent,
        ..Settings::default()
    };
    dashboard.install_settings(settings);

    let rows = rendered_rows_at(&dashboard, 120, 40, now)
        .into_iter()
        .map(|row| row.chars().take(39).collect::<String>())
        .collect::<Vec<_>>();
    let text = rows.join("\n");
    assert!(
        !text.contains("SPACELIFT-AGENT"),
        "terminal-only project is omitted:\n{text}"
    );
    assert!(
        !text.contains("EMPTY-PROJECT"),
        "empty project is omitted:\n{text}"
    );
    let alpha = rows
        .iter()
        .position(|line| line.contains("ALPHA"))
        .expect("alpha project header");
    let consigint = rows
        .iter()
        .position(|line| line.contains("CONSIGINT"))
        .expect("consigint project header");
    assert!(alpha < consigint, "projects remain alphabetic:\n{text}");
    let newest = rows
        .iter()
        .position(|line| line.contains("review newest"))
        .expect("newest Agent row");
    let older = rows
        .iter()
        .position(|line| line.contains("implement older"))
        .expect("older Agent row");
    assert!(newest < older, "newest Agent sorts first:\n{text}");
    assert!(
        rows[newest].contains("1m"),
        "age appears on the title line: {}",
        rows[newest]
    );
    assert!(rows[newest + 1].contains("└") && rows[newest + 1].contains("auth · codex"));
    assert!(rows[older + 1].contains("└") && rows[older + 1].contains("lifecycle · claude"));

    assert_eq!(
        dashboard.key(KeyCode::Char('m')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(matches!(
        only_request(dashboard.drain_outbox()).request,
        Request::SetSetting { ref path, ref value }
            if path == "sidebar_view" && value.as_deref() == Some("\"tree\"")
    ));
    dashboard.install_settings(Settings::default());
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .join("\n")
            .contains("SPACELIFT-AGENT"),
        "returning to tree restores terminal rows"
    );
}

#[test]
fn recent_rows_render_two_lines_and_clip_long_titles_and_branches() {
    use ovrcr::protocol::{SessionKind, Settings, SidebarView};

    let mut hierarchy = fixture_hierarchy();
    let session = review_session_mut(&mut hierarchy);
    session.kind = SessionKind::Agent {
        name: "codex".into(),
    };
    session.label = "codex".into();
    session.title = Some("a very long review title that cannot fit in the sidebar".into());
    session.last_activity_unix_ms = Some(0);
    hierarchy.projects[1].workspaces[1].name =
        "feature/a-branch-name-that-is-also-too-long-to-fit".into();

    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    let settings = Settings {
        sidebar_view: SidebarView::Recent,
        ..Settings::default()
    };
    dashboard.install_settings(settings);

    let rows = rendered_sidebar_rows_at(&dashboard, 120, 40, 2 * 24 * 60 * 60_000);
    let title = rows
        .iter()
        .position(|row| row.contains("a very long"))
        .expect("recent title line");
    assert!(
        rows[title].contains("2d"),
        "age on first line: {}",
        rows[title]
    );
    assert!(
        rows[title].contains('…'),
        "title clips with ellipsis: {}",
        rows[title]
    );
    assert!(
        rows[title + 1].contains("└") && rows[title + 1].contains('…'),
        "long branch clips on the second line: {}",
        rows[title + 1]
    );
}

#[test]
fn recent_view_preserves_unread_input_highlights_and_folded_counts() {
    use ovrcr::protocol::{Settings, SidebarView};

    let mut hierarchy = fixture_hierarchy();
    let project = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == "consigint")
        .expect("fixture project");
    let auth = project
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == "auth")
        .expect("auth workspace");
    let input = auth
        .sessions
        .iter_mut()
        .find(|session| session.id == SessionId(1))
        .expect("Input Agent");
    set_attention(input, AgentActivity::Busy, true, false);
    input.title = Some("input-urgent".into());
    input.last_activity_unix_ms = Some(20);

    let lifecycle = project
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == "lifecycle")
        .expect("lifecycle workspace");
    let unread = lifecycle
        .sessions
        .iter_mut()
        .find(|session| session.id == SessionId(2))
        .expect("Unread Agent");
    set_attention(unread, AgentActivity::ResponseReady, false, true);
    unread.title = Some("ready-unread".into());
    unread.last_activity_unix_ms = Some(10);

    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    dashboard.install_settings(Settings {
        sidebar_view: SidebarView::Recent,
        ..Settings::default()
    });

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 20))
        .unwrap();
    for title in ["input-urgent", "ready-unread"] {
        let row = (0..40)
            .find(|row| sidebar_row(&terminal, *row).contains(title))
            .unwrap_or_else(|| panic!("recent Agent sidebar title was not rendered: {title}"));
        let column = sidebar_row(&terminal, row).find(title).unwrap() as u16;
        let cell = &terminal.backend().buffer()[(column, row)];
        assert_eq!(cell.fg, Color::Rgb(205, 214, 244), "{title}");
        assert!(
            cell.modifier.contains(Modifier::BOLD),
            "{title} keeps the shared Unread/Input title emphasis"
        );
    }

    let project_row = rendered_rows(&dashboard, 120, 40)
        .iter()
        .position(|line| line.contains("CONSIGINT"))
        .expect("recent project row") as u16;
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: project_row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    let folded = rendered_rows(&dashboard, 120, 40);
    let folded_project = &folded[project_row as usize];
    assert!(
        folded_project.contains("?1 ✓1 ▸ 2 ws"),
        "recent project fold counts its Agent Input and Unread rows, excluding Terminals: {folded_project}"
    );
}

#[test]
fn recent_activity_reorders_on_session_update_without_changing_selection() {
    use ovrcr::protocol::{SessionKind, Settings, SidebarView};

    let mut hierarchy = fixture_hierarchy();
    for session in hierarchy
        .projects
        .iter_mut()
        .flat_map(|project| project.workspaces.iter_mut())
        .flat_map(|workspace| workspace.sessions.iter_mut())
    {
        if matches!(session.id.0, 1 | 2) {
            session.kind = SessionKind::Agent {
                name: "codex".into(),
            };
            session.label = "codex".into();
            session.title = Some(if session.id == SessionId(1) {
                "selected review".into()
            } else {
                "other implement".into()
            });
            session.last_activity_unix_ms = Some(if session.id == SessionId(1) { 10 } else { 20 });
        }
    }
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy.clone());
    dashboard.install_focus(SessionId(1));
    let settings = Settings {
        sidebar_view: SidebarView::Recent,
        ..Settings::default()
    };
    dashboard.install_settings(settings);
    let ordered = rendered_sidebar_rows_at(&dashboard, 120, 40, 0);
    assert!(
        ordered
            .iter()
            .position(|row| row.contains("other implement"))
            < ordered
                .iter()
                .position(|row| row.contains("selected review")),
        "newer session starts first"
    );

    let mut updated = hierarchy.projects[1].workspaces[1].sessions[0].clone();
    updated.last_activity_unix_ms = Some(30);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        updated,
    ))));
    let reordered = rendered_sidebar_rows_at(&dashboard, 120, 40, 0);
    assert!(
        reordered
            .iter()
            .position(|row| row.contains("selected review"))
            < reordered
                .iter()
                .position(|row| row.contains("other implement")),
        "updated session moves to the top"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
}

#[test]
fn live_server_dashboard_saves_and_restores_recent_sidebar_view() {
    use crate::live::Live;
    use ovrcr::protocol as p;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    fn dashboard_request(
        stream: &mut UnixStream,
        dashboard: &mut Dashboard,
        request_id: u64,
        request: p::Request,
        wait_for_settings: bool,
    ) -> p::Response {
        p::write_frame(
            stream,
            &p::ClientMessage {
                request_id,
                request,
            },
        )
        .unwrap();
        let mut response = None;
        let mut got_settings = !wait_for_settings;
        while response.is_none() || !got_settings {
            match p::read_frame::<p::ServerMessage>(stream).unwrap() {
                p::ServerMessage::Response {
                    request_id: received,
                    response: value,
                } if received == request_id => {
                    if let p::Response::Hierarchy(hierarchy) = &value {
                        dashboard.install_hierarchy(hierarchy.clone());
                    } else {
                        dashboard.handle_server_message(p::ServerMessage::Response {
                            request_id: received,
                            response: value.clone(),
                        });
                    }
                    response = Some(value);
                }
                p::ServerMessage::Event(p::ServerEvent::SettingsChanged(report)) => {
                    got_settings = true;
                    dashboard.handle_server_message(p::ServerMessage::Event(
                        p::ServerEvent::SettingsChanged(report),
                    ));
                }
                message => {
                    dashboard.handle_server_message(message);
                }
            }
        }
        response.unwrap()
    }

    let live = Live::idle().bounded();
    std::fs::write(
        live.config.join("dashboard.toml"),
        "quota.enabled = false\n",
    )
    .unwrap();
    live.start_binary();
    live.ready("feature/sidebar-recent");
    let p::Response::CreatedSession(agent) =
        live.request(p::Request::CreateSession(p::CreateSessionRequest {
            project: crate::live::PROJECT.into(),
            workspace: crate::live::WORKSPACE.into(),
            name: "recent-live".into(),
            label: Some("codex".into()),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "while :; do sleep 1; done".into(),
            ],
            kind: p::SessionKind::Agent {
                name: "codex".into(),
            },
        }))
    else {
        panic!("live Agent session was not created");
    };

    let area = Rect::new(0, 0, 120, 40);
    let mut stream = p::connect_server(&live.socket).unwrap();
    stream
        .set_read_timeout(Some(live.timeout().unwrap_or(Duration::from_secs(5))))
        .unwrap();
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: area.height,
        cols: area.width,
    });
    dashboard.install_area(area);
    assert!(matches!(
        dashboard_request(
            &mut stream,
            &mut dashboard,
            900_000,
            p::Request::DashboardHello,
            true,
        ),
        p::Response::Hierarchy(_)
    ));
    dashboard.install_focus(agent.id);
    let tree = rendered_rows(&dashboard, area.width, area.height).join("\n");
    assert!(tree.contains("recent-live"), "real Dashboard tree:\n{tree}");
    assert!(
        tree.contains("local"),
        "Terminal session remains in tree:\n{tree}"
    );

    assert_eq!(dashboard.key(KeyCode::Char('m')), DashboardAction::Redraw);
    let setting = only_request(dashboard.drain_outbox());
    assert!(matches!(
        &setting.request,
        p::Request::SetSetting { path, value }
            if path == "sidebar_view" && value.as_deref() == Some("\"recent\"")
    ));
    let saved = dashboard_request(
        &mut stream,
        &mut dashboard,
        setting.request_id,
        setting.request,
        true,
    );
    assert_eq!(saved, p::Response::Ok);
    let recent = rendered_rows(&dashboard, area.width, area.height).join("\n");
    assert!(
        recent.contains("recent-live"),
        "Agent appears in recent view:\n{recent}"
    );
    assert!(
        !recent.contains("local"),
        "Terminal is excluded from recent view:\n{recent}"
    );
    drop(stream);

    let mut next_stream = p::connect_server(&live.socket).unwrap();
    next_stream
        .set_read_timeout(Some(live.timeout().unwrap_or(Duration::from_secs(5))))
        .unwrap();
    let mut attached = Dashboard::new(TerminalSize {
        rows: area.height,
        cols: area.width,
    });
    attached.install_area(area);
    assert!(matches!(
        dashboard_request(
            &mut next_stream,
            &mut attached,
            900_001,
            p::Request::DashboardHello,
            true,
        ),
        p::Response::Hierarchy(_)
    ));
    assert!(
        rendered_rows(&attached, area.width, area.height)
            .join("\n")
            .contains("recent-live"),
        "next Dashboard attachment restored the saved view"
    );
}

#[test]
fn live_dashboard_input_publishes_activity_without_pty_output() {
    use crate::live::Live;
    use ovrcr::protocol as p;
    use std::time::{Duration, Instant};

    let live = Live::binary();
    live.ready("feature/sidebar-input-activity");
    let agent_ready = live.root.path().join("non-echoing-agent.ready");
    let p::Response::CreatedSession(agent) =
        live.request(p::Request::CreateSession(p::CreateSessionRequest {
            project: crate::live::PROJECT.into(),
            workspace: crate::live::WORKSPACE.into(),
            name: "non-echoing-agent".into(),
            label: Some("codex".into()),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                format!(
                    "stty -echo; : > '{}'; exec cat >/dev/null",
                    agent_ready.display()
                )
                .into(),
            ],
            kind: p::SessionKind::Agent {
                name: "codex".into(),
            },
        }))
    else {
        panic!("live Agent session was not created");
    };
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while !agent_ready.exists() {
        assert!(
            Instant::now() < ready_deadline,
            "non-echoing Agent did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut stream = p::connect_server(&live.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let p::Response::Hierarchy(hierarchy) =
        p::client::request(&mut stream, 910_000, p::Request::DashboardHello).unwrap()
    else {
        panic!("live Dashboard handshake did not return hierarchy");
    };
    let initial = hierarchy
        .projects
        .iter()
        .flat_map(|project| &project.workspaces)
        .flat_map(|workspace| &workspace.sessions)
        .find(|session| session.id == agent.id)
        .expect("live Agent in Dashboard hierarchy")
        .last_activity_unix_ms;
    p::write_frame(
        &mut stream,
        &p::ClientMessage {
            request_id: 910_001,
            request: p::Request::SetView {
                view: p::DashboardView {
                    revision: 1,
                    panes: vec![p::PaneTarget {
                        run: agent.run,
                        session: agent.id,
                        size: p::TerminalSize { rows: 24, cols: 80 },
                    }],
                    focused: Some(agent.id),
                },
            },
        },
    )
    .unwrap();
    loop {
        match p::read_frame::<p::ServerMessage>(&mut stream).unwrap() {
            p::ServerMessage::Response {
                request_id: 910_001,
                response: p::Response::Ok,
            } => break,
            p::ServerMessage::Response {
                request_id: 910_001,
                response: p::Response::Error { code, message },
            } => panic!("Dashboard view rejected: {code:?}: {message}"),
            _ => {}
        }
    }

    p::write_frame(
        &mut stream,
        &p::ClientMessage {
            request_id: 910_002,
            request: p::Request::Input {
                session: agent.id,
                run: agent.run,
                bytes: b"no-echo\n".to_vec(),
            },
        },
    )
    .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut accepted = false;
    let mut published_activity = None;
    let drain_deadline = Instant::now() + Duration::from_millis(750);
    while Instant::now() < drain_deadline && (!accepted || published_activity.is_none()) {
        let Ok(message) = p::read_frame::<p::ServerMessage>(&mut stream) else {
            break;
        };
        match message {
            p::ServerMessage::Response {
                request_id: 910_002,
                response,
            } => {
                assert_eq!(response, p::Response::Ok, "Dashboard input was accepted");
                accepted = true;
            }
            p::ServerMessage::Event(p::ServerEvent::SessionChanged(summary))
                if summary.id == agent.id
                    && summary
                        .last_activity_unix_ms
                        .is_some_and(|activity| initial.is_none_or(|before| activity > before)) =>
            {
                published_activity = summary.last_activity_unix_ms;
            }
            p::ServerMessage::Event(p::ServerEvent::Output { session, .. })
                if session == agent.id =>
            {
                panic!("non-echoing Agent unexpectedly emitted PTY output");
            }
            _ => {}
        }
    }
    assert!(
        accepted,
        "Dashboard input response arrived within the bounded drain"
    );
    assert!(
        published_activity.is_some(),
        "accepted Dashboard input publishes its new activity within the bounded event drain, without relying on PTY output"
    );
}

fn set_attention(session: &mut SessionSummary, state: AgentActivity, input: bool, unread: bool) {
    use ovrcr::protocol::{
        ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, InputKind,
        InputRequest, ReadyObservation, ReporterHealth, SampleQuality,
    };

    let binding = AgentBinding {
        provider: AgentProvider::Codex,
        invocation: format!("fixture-{}", session.id.0),
        conversation: format!("conversation-{}", session.id.0),
        generation: 1,
    };
    session.phase = SessionPhase::Running;
    session.kind = ovrcr::protocol::SessionKind::Agent {
        name: "codex".into(),
    };
    session.agent = Some(AgentSnapshot {
        binding: binding.clone(),
        activity: Some(ActivitySample {
            state,
            quality: SampleQuality::Observed,
            turn: Some("fixture-turn".into()),
        }),
        metrics: None,
        health: HealthSample {
            state: ReporterHealth::Connected,
            reason: None,
        },
        activity_revision: 1,
        metrics_revision: 0,
        health_revision: 1,
        input_requests: if input {
            vec![InputRequest {
                id: "question:fixture".into(),
                kind: InputKind::Confirm,
            }]
        } else {
            Vec::new()
        },
        input_revision: u64::from(input),
        native_control: None,
        native_control_revision: 0,
    });
    session.unread = unread.then_some(ReadyObservation {
        binding,
        turn: Some("fixture-turn".into()),
        activity_revision: 1,
    });
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
    let review_row = sidebar_row_with(&terminal, "review");
    let implement_row = sidebar_row_with(&terminal, "implement");
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(buffer[(0, review_row)].symbol(), "▌");
    assert!(rendered[0].starts_with("󰚩 OVRCR") && !rendered[0].contains("Operator"));
    assert!(!rendered.iter().any(|row| row.contains("ctx ")));
    assert!(rendered.iter().any(|row| row.contains("Space Menu")));
    assert!(rendered.iter().any(|row| row.contains("? Help")));
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(39, 9)].symbol(), "│");
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    // The header leads with the title; the pid sits muted at the far right.
    let header = rendered[1].chars().skip(40).collect::<String>();
    assert!(header.starts_with("review  claude · 0m"), "{header}");
    assert!(header.trim_end().ends_with("pid 111"), "{header}");
    assert_eq!(buffer[(119 - 6, 1)].fg, Color::Rgb(108, 112, 134));
    assert!(
        !header.contains("pid:") && !header.contains("agent"),
        "{header}"
    );
    // Without metrics the second line is the plain rule, not the title again.
    let second = rendered[2].chars().skip(40).collect::<String>();
    assert_eq!(
        second,
        "─".repeat(80),
        "the title is not repeated on line 2"
    );
    assert!(buffer[(8, implement_row)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(1, review_row)].bg, Color::Rgb(49, 50, 68));

    dashboard.install_focus(SessionId(2));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let selected_exited = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 1)].symbol())
        .collect::<String>();
    assert!(
        selected_exited.contains("implement  claude  Exited · 0m"),
        "{selected_exited}"
    );
    assert!(!selected_exited.contains("pid"), "{selected_exited}");
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
    let project_row = sidebar_row_with(&terminal, "CONSIGINT");
    let auth_row = sidebar_row_with(&terminal, "auth");
    let local_row = sidebar_row_with(&terminal, "$ local");
    let review_row = sidebar_row_with(&terminal, "review");
    let lifecycle_row = sidebar_row_with(&terminal, "lifecycle");
    let implement_row = sidebar_row_with(&terminal, "implement");
    let spacelift_row = sidebar_row_with(&terminal, "SPACELIFT-AGENT");
    let progress_row = sidebar_row_with(&terminal, "progress");
    // The title band is the brand alone; the first project leads the sidebar.
    let title = (0..40).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
    assert_eq!(title.trim_end(), "󰚩 OVRCR");
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
    // Project section header: upper case, blue bold, rule to the edge, no chevron.
    assert_eq!(row(project_row), format!(" CONSIGINT {}", rule(11)));
    assert_eq!(buffer[(1, project_row)].fg, Color::Rgb(137, 180, 250));
    assert!(buffer[(1, project_row)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(20, project_row)].fg, Color::Rgb(88, 91, 112));
    // Workspace: branch glyph, bold name.
    assert_eq!(row(auth_row).trim_end(), "  󰘬 auth");
    assert_eq!(buffer[(2, auth_row)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(buffer[(4, auth_row)].fg, Color::Rgb(205, 214, 244));
    assert!(buffer[(4, auth_row)].modifier.contains(Modifier::BOLD));
    // Selected local shell: mauve bar, surface background, subtext glyph and name.
    assert_eq!(row(local_row).trim_end(), "▌    $ local");
    assert_eq!(buffer[(0, local_row)].fg, Color::Rgb(203, 166, 247));
    for x in 0..39 {
        assert_eq!(
            buffer[(x, local_row)].bg,
            Color::Rgb(49, 50, 68),
            "column {x}"
        );
    }
    assert_eq!(buffer[(5, local_row)].fg, Color::Rgb(166, 173, 200));
    assert_eq!(buffer[(7, local_row)].fg, Color::Rgb(166, 173, 200));
    // Session: status glyph at column 5, regular-weight name, agent right-aligned in
    // the provider colour, one trailing cell.
    assert_eq!(
        row(review_row),
        format!("     - review{}claude ", " ".repeat(19))
    );
    assert_eq!(buffer[(5, review_row)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(buffer[(7, review_row)].fg, Color::Rgb(205, 214, 244));
    assert!(!buffer[(7, review_row)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(32, review_row)].fg, Color::Rgb(250, 179, 135));
    assert_eq!(buffer[(4, review_row)].bg, Color::Rgb(30, 30, 46));
    assert_eq!(row(lifecycle_row).trim_end(), "  󰘬 lifecycle");
    assert_eq!(row(implement_row - 1).trim_end(), "     $ local");
    assert_eq!(
        row(implement_row),
        format!("     · implement{}claude ", " ".repeat(16))
    );
    assert!(buffer[(7, implement_row)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(33, implement_row)].fg, Color::Rgb(173, 127, 104));
    // Blank gap line before the next project, then its header.
    assert_eq!(row(spacelift_row - 1).trim_end(), "");
    assert_eq!(row(spacelift_row), format!(" SPACELIFT-AGENT {}", rule(17)));
    assert_eq!(buffer[(1, spacelift_row)].fg, Color::Rgb(137, 180, 250));
    assert_eq!(row(progress_row).trim_end(), "  󰘬 progress");
    assert_eq!(row(progress_row + 1).trim_end(), "     $ local");
    assert_eq!(row(progress_row + 2).trim_end(), "");
    assert_eq!(buffer[(39, auth_row)].symbol(), "│");

    // Selecting another session moves the bar and background with it.
    dashboard.install_focus(SessionId(1));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, review_row)].symbol(), "▌");
    assert_eq!(buffer[(20, review_row)].bg, Color::Rgb(49, 50, 68));
    assert_eq!(buffer[(0, local_row)].symbol(), " ");
    assert_eq!(buffer[(20, local_row)].bg, Color::Rgb(30, 30, 46));
    assert_eq!(buffer[(32, review_row)].fg, Color::Rgb(250, 179, 135));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));

    // Selecting a workspace container by its name uses the same bar and background.
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: auth_row,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, auth_row)].symbol(), "▌");
    assert_eq!(buffer[(0, auth_row)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(20, auth_row)].bg, Color::Rgb(49, 50, 68));
    assert_eq!(buffer[(4, auth_row)].fg, Color::Rgb(205, 214, 244));
    assert_eq!(buffer[(0, review_row)].symbol(), " ");
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
        native_control: None,
        native_control_revision: 0,
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
        let review_row = sidebar_row_with(&terminal, "review");
        let cell = &terminal.backend().buffer()[(5, review_row)];
        assert_eq!(cell.symbol(), "✓", "{health:?}");
        assert_eq!(cell.fg, color, "{health:?}");
    }
}

fn rendered_sidebar_rows_at(
    dashboard: &Dashboard,
    width: u16,
    height: u16,
    now_unix_ms: u64,
) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, dashboard, now_unix_ms))
        .unwrap();
    (0..height)
        .map(|y| {
            (0..width.min(39))
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect()
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
        let review_row = sidebar_row_with(&terminal, "review");
        assert_eq!(
            terminal.backend().buffer()[(5, review_row)].symbol(),
            marker
        );
        assert_eq!(
            terminal.backend().buffer()[(5, review_row)].fg,
            Color::Rgb(166, 227, 161)
        );
        assert_eq!(
            terminal.backend().buffer()[(5, review_row - 1)].symbol(),
            "$"
        );
    }
    review.activity = AgentActivity::Idle;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        review,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1100))
        .unwrap();
    let review_row = sidebar_row_with(&terminal, "review");
    assert_eq!(terminal.backend().buffer()[(5, review_row)].symbol(), " ");
    // An exited process cannot remain busy, even if an old signal is retained.
    let mut implement = fixture_hierarchy().projects[1].workspaces[0].sessions[0].clone();
    implement.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        implement,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1200))
        .unwrap();
    let implement_row = sidebar_row_with(&terminal, "implement");
    assert_eq!(
        terminal.backend().buffer()[(5, implement_row)].symbol(),
        "·"
    );
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

    // The pane is 80 columns: the words lead, the pid closes the line at the far right.
    let with_pid = |words: &str| {
        format!(
            "{words}{}pid 111",
            " ".repeat(80 - 7 - words.chars().count())
        )
    };
    for (activity, expected) in [
        // A hook that has reported nothing says nothing: no `agent unknown`.
        (AgentActivity::Unknown, "review  claude · 0m"),
        (AgentActivity::Idle, "review  claude  Idle · 0m"),
        (AgentActivity::Busy, "review  claude  Working · 0m"),
        (
            AgentActivity::WaitingInput,
            "review  claude  Waiting for input · 0m",
        ),
        (AgentActivity::Error, "review  claude  Error · 0m"),
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
        assert_eq!(metadata(&terminal), with_pid(expected));
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
    assert_eq!(metadata(&terminal), with_pid("review  claude  Paused · 0m"));

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
    assert_eq!(metadata(&terminal), "review  claude  Exited · 0m");
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
        let review_row = sidebar_row_with(&terminal, "review");
        let cell = &terminal.backend().buffer()[(5, review_row)];
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
    let implement_row = sidebar_row_with(&terminal, "implement");
    assert_eq!(
        terminal.backend().buffer()[(5, implement_row)].symbol(),
        "·"
    );
    assert_eq!(terminal.backend().buffer()[(5, implement_row)].fg, muted);
    // A local shell stays quiet until a hook reports real activity inside it.
    let mut progress = fixture_hierarchy().projects[0].workspaces[0].sessions[0].clone();
    progress.activity = AgentActivity::Error;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        progress,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let progress_row = sidebar_row_with(&terminal, "progress") + 1;
    assert_eq!(terminal.backend().buffer()[(5, progress_row)].symbol(), "!");
    let shell_row = sidebar_rows_with(&terminal, "$ local")[0];
    assert_eq!(terminal.backend().buffer()[(5, shell_row)].symbol(), "$");
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
        native_control: None,
        native_control_revision: 0,
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
        header_text.contains("Ready · unread reply"),
        "header should say unread reply: {header_text}"
    );

    dashboard.install_focus(SessionId(1));
    draw(&dashboard, &mut terminal);
    assert_eq!(glyph(&sidebar(&terminal), "review"), '\u{2713}');
    assert!(header(&terminal).contains("unread reply"));

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
        split.contains("unread reply"),
        "split header should say unread reply: {split}"
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
        !reviewed.to_lowercase().contains("unread"),
        "review should clear the Unread header: {reviewed}"
    );
    assert!(
        reviewed.contains("Ready"),
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
    let shell_row = sidebar_rows_with(&terminal, "$ local")[0];
    assert_eq!(terminal.backend().buffer()[(5, shell_row)].symbol(), "$");

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
    let busy_row = sidebar_row_with(&terminal, "local");
    assert_eq!(terminal.backend().buffer()[(5, busy_row)].symbol(), "⠙");

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
    let idle_row = sidebar_rows_with(&terminal, "$ local")[0];
    assert_eq!(terminal.backend().buffer()[(5, idle_row)].symbol(), "$");
}

#[test]
fn selected_session_uses_a_soft_bar_on_one_line() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();

    let local_row = sidebar_rows_with(&terminal, "$ local")[0];
    let review_row = sidebar_row_with(&terminal, "review");
    let lifecycle_row = sidebar_row_with(&terminal, "lifecycle");
    for col in 0..39 {
        assert_eq!(
            buffer[(col, review_row)].bg,
            Color::Rgb(49, 50, 68),
            "column {col}"
        );
    }
    for row in [local_row, lifecycle_row] {
        assert_eq!(buffer[(0, row)].bg, Color::Rgb(30, 30, 46));
    }
    let rendered = [local_row, review_row, lifecycle_row]
        .into_iter()
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
    assert_eq!(buffer[(0, review_row)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(5, review_row)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(buffer[(7, review_row)].fg, Color::Rgb(205, 214, 244));
    assert_eq!(buffer[(32, review_row)].fg, Color::Rgb(250, 179, 135));
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
    let dashboard = dashboard_fixture();
    let local_rows = dashboard_sidebar_rows(&dashboard, "$ local", 120, 40);
    assert_eq!(local_rows.len(), 3, "fixture has three local shell rows");
    let session_rows = [
        (local_rows[0], 5),
        (dashboard_sidebar_row(&dashboard, "review", 120, 40), 1),
        (local_rows[1], 4),
        (dashboard_sidebar_row(&dashboard, "implement", 120, 40), 2),
        (local_rows[2], 3),
    ];
    for (row, id) in session_rows {
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
    let workspace_row = dashboard_sidebar_row(&workspace, "auth", 120, 40);
    assert_eq!(
        workspace.mouse_action(click(2, workspace_row), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &workspace, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, workspace_row).contains("▸"),
        "workspace click on the disclosure column should fold the group"
    );
    let mut project = dashboard_fixture();
    let project_row = dashboard_sidebar_row(&project, "CONSIGINT", 120, 40);
    assert_eq!(
        project.mouse_action(click(0, project_row), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &project, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, project_row).contains("▸"),
        "project disclosure should fold the section"
    );
    let gaps_dashboard = dashboard_fixture();
    let implement_row = dashboard_sidebar_row(&gaps_dashboard, "implement", 120, 40);
    let progress_row = dashboard_sidebar_row(&gaps_dashboard, "progress", 120, 40);
    for row in [implement_row + 1, progress_row + 2] {
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

    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    let review_row = sidebar_row_with(&terminal, "review");
    let implement_row = sidebar_row_with(&terminal, "implement");
    let auth_row = sidebar_row_with(&terminal, "auth");
    let lifecycle_row = sidebar_row_with(&terminal, "lifecycle");
    assert!(buffer[(4, auth_row)].modifier.contains(Modifier::BOLD));
    assert!(buffer[(4, lifecycle_row)].modifier.contains(Modifier::BOLD));
    assert_eq!(row(review_row), "     - review          claude:sonnet-4 ");
    assert_eq!(buffer[(23, review_row)].fg, Color::Rgb(250, 179, 135));
    assert_eq!(
        row(implement_row),
        "     · implement         unknown:model "
    );
    assert_eq!(buffer[(25, implement_row)].fg, Color::Rgb(144, 150, 175));
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
        native_control: None,
        native_control_revision: 0,
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
    let review_row = dashboard_sidebar_row(&dashboard, "review-a-very", 60, 24);
    let under_row = review_row + 1;
    assert_eq!(
        narrow_row(review_row),
        "▌    - review-a-very-long-se…",
        "a label that cannot share the line leaves the whole line to the name"
    );
    assert_eq!(
        narrow_row(under_row),
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
    // Leave enough vertical room for the production-default Usage and Quota
    // sections without clipping the tree row whose two-line layout is tested.
    let area = Rect::new(0, 0, 60, 27);
    dashboard.install_area(area);
    let mut terminal = Terminal::new(TestBackend::new(60, 27)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let row = |terminal: &Terminal<TestBackend>, y| {
        (0..29)
            .map(|column| terminal.backend().buffer()[(column, y)].symbol())
            .collect::<String>()
    };
    let review_row = sidebar_row_with(&terminal, "review");
    let lifecycle_row = sidebar_row_with(&terminal, "lifecycle");
    let local_rows = sidebar_rows_with(&terminal, "$ local");
    let implement_row = sidebar_row_with(&terminal, "implement");
    assert_eq!(local_rows.len(), 2);
    // Twenty-nine cells cannot hold a twelve-cell name beside `pi:grok-4.7`, so the
    // model is not dropped: the label moves under the name. The shell above stays one line.
    assert_eq!(
        row(&terminal, local_rows[0]),
        "     $ local                 "
    );
    assert_eq!(row(&terminal, review_row), "▌    - review                ");
    assert_eq!(
        row(&terminal, review_row + 1),
        "▌      └ pi:grok-4.7         "
    );
    assert_eq!(row(&terminal, lifecycle_row).trim_end(), "  󰘬 lifecycle");
    // Stacking is all or nothing: `claude` alone would fit, but it stacks with the column.
    assert_eq!(
        row(&terminal, local_rows[1]),
        "     $ local                 "
    );
    assert_eq!(
        row(&terminal, implement_row),
        "     · implement             "
    );
    assert_eq!(
        row(&terminal, implement_row + 1),
        "       └ claude              "
    );
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(9, review_row + 1)].fg,
        Color::Rgb(203, 166, 247),
        "provider colour"
    );
    assert_eq!(
        buffer[(11, review_row + 1)].fg,
        Color::Rgb(108, 112, 134),
        "model stays quiet"
    );
    for y in [review_row, review_row + 1] {
        assert_eq!(buffer[(0, y)].fg, Color::Rgb(203, 166, 247));
        for x in 0..29 {
            assert_eq!(buffer[(x, y)].bg, Color::Rgb(49, 50, 68), "({x}, {y})");
        }
    }
    assert_eq!(buffer[(0, lifecycle_row)].bg, Color::Rgb(30, 30, 46));

    // Keys treat both lines as one session: `j` lands on the next row, `k` returns.
    assert_eq!(dashboard.key(KeyCode::Char('j')), DashboardAction::Redraw);
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_selected_sidebar_row(&terminal, "lifecycle");
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
    dashboard.mouse_action(click(9, review_row + 1), area);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    hover(&mut dashboard, 27, review_row + 1, area);
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(
        row(&terminal, review_row),
        "▌    - review             [x]",
        "hovering the label line marks the session's first line"
    );
    assert_eq!(
        row(&terminal, review_row + 1),
        "▌      └ pi:grok-4.7         "
    );
    // The mark itself is only on the first line; its column on the label line selects.
    dashboard.install_focus(SessionId(5));
    dashboard.mouse_action(click(27, review_row + 1), area);
    assert!(!palette_text(&dashboard).contains("Close terminal?"));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(matches!(
        dashboard.mouse_action(click(27, review_row), area),
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
        let mut terminal = Terminal::new(TestBackend::new(60, 27)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let rows = (0..27)
            .map(|y| {
                (0..29)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        (rows, dashboard)
    };
    let (inline, inline_dashboard) = draw("claude");
    let inline_review = dashboard_sidebar_row(&inline_dashboard, "review", 60, 27) as usize;
    let inline_lifecycle = dashboard_sidebar_row(&inline_dashboard, "lifecycle", 60, 27) as usize;
    let inline_implement = dashboard_sidebar_row(&inline_dashboard, "implement", 60, 27) as usize;
    assert_eq!(inline[inline_review], "     - review         claude");
    assert_eq!(inline[inline_lifecycle], "  󰘬 lifecycle");
    assert_eq!(inline[inline_implement], "     · implement      claude");
    assert_eq!(inline[inline_implement + 1], "");
    let (stacked, stacked_dashboard) = draw("pi / grok-4.7");
    let stacked_review = dashboard_sidebar_row(&stacked_dashboard, "review", 60, 27) as usize;
    let stacked_lifecycle = dashboard_sidebar_row(&stacked_dashboard, "lifecycle", 60, 27) as usize;
    let stacked_local_rows = dashboard_sidebar_rows(&stacked_dashboard, "$ local", 60, 27);
    let stacked_implement = dashboard_sidebar_row(&stacked_dashboard, "implement", 60, 27) as usize;
    assert_eq!(stacked[stacked_review], "     - review");
    assert_eq!(stacked[stacked_review + 1], "       └ pi:grok-4.7");
    assert_eq!(stacked[stacked_lifecycle], "  󰘬 lifecycle");
    assert_eq!(
        stacked[stacked_local_rows[1] as usize], "     $ local",
        "shells never stack"
    );
    assert_eq!(stacked[stacked_implement], "     · implement");
    assert_eq!(
        stacked[stacked_implement + 1],
        "       └ claude",
        "a label that fits still stacks"
    );
    assert_eq!(stacked[stacked_implement + 2], "");
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
    let review_row = sidebar_row_with(&terminal, "review");
    let lifecycle_row = sidebar_row_with(&terminal, "lifecycle");
    assert_eq!(
        sidebar_row(&terminal, review_row),
        format!("     - review{}pi:grok-4.7 ", " ".repeat(14))
    );
    assert_eq!(
        sidebar_row(&terminal, lifecycle_row).trim_end(),
        "  󰘬 lifecycle"
    );
    let area = Rect::new(0, 0, 120, 40);
    let mut clicked = dashboard_fixture();
    let lifecycle_row = dashboard_sidebar_row(&clicked, "lifecycle", 120, 40);
    clicked.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 7,
            row: lifecycle_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert!(
        clicked.focused_session().is_none(),
        "the lifecycle workspace row does not select a session"
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
            let review_y = sidebar_row_with(&terminal, "review");
            let row = sidebar_row(&terminal, review_y);
            let under = sidebar_row(&terminal, review_y + 1);
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
    let project_row = dashboard_sidebar_row(&dashboard, "CONSIGINT-", 80, 24);
    let review_row = dashboard_sidebar_row(&dashboard, "review-a-very", 80, 24);
    let project = (0..39)
        .map(|column| buffer[(column, project_row)].symbol())
        .collect::<String>();
    assert!(project.trim_end().ends_with('…'));
    assert_eq!(buffer[(39, project_row)].symbol(), "│");
    let review = (0..39)
        .map(|column| buffer[(column, review_row)].symbol())
        .collect::<String>();
    // The title keeps its room first: the agent label goes before the title clips.
    assert_eq!(review, "▌    - review-a-very-long-session-name…");

    // Too narrow for a twelve-cell name beside the agent: the agent moves under the name.
    let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let narrow_review_row = dashboard_sidebar_row(&dashboard, "review-a-very", 50, 24);
    let row = |y| {
        (0..24)
            .map(|column| buffer[(column, y)].symbol())
            .collect::<String>()
    };
    assert_eq!(row(narrow_review_row), "▌    - review-a-very-lo…");
    assert_eq!(row(narrow_review_row + 1), "▌      └ claude         ");
    assert_eq!(buffer[(24, narrow_review_row)].symbol(), "│");
    assert_eq!(buffer[(24, narrow_review_row + 1)].symbol(), "│");
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
    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    let action = dashboard.mouse_action(mouse(2, auth_row), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    // A folded workspace shows how many sessions it hides.
    assert_eq!(
        sidebar_row(&terminal, auth_row),
        format!("  󰘬 auth{}▸ 2 ", " ".repeat(27))
    );
    let action = dashboard.mouse_action(mouse(2, auth_row), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    let action = dashboard.mouse_action(mouse(4, review_row), area);
    assert!({
        let _ = action;
        matches!(dashboard.drain_outbox().as_slice(), [_])
    });
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let project_row = dashboard_sidebar_row(&dashboard, "CONSIGINT", 120, 40);
    let action = dashboard.mouse_action(mouse(0, project_row), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    // A folded project shows its workspace count and keeps its rule.
    assert_eq!(
        sidebar_row(&terminal, project_row),
        format!(" CONSIGINT {} ▸ 2 ws ", "─".repeat(20))
    );
    assert_eq!(sidebar_row(&terminal, project_row + 1).trim_end(), "");
    assert_eq!(
        sidebar_row(&terminal, project_row + 2),
        format!(" SPACELIFT-AGENT {}", "─".repeat(22))
    );
    let action = dashboard.mouse_action(
        MouseEvent {
            column: 60,
            ..mouse(4, dashboard.pane_rects(area)[0].terminal.y + 3)
        },
        area,
    );
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    enter_terminal(&mut dashboard);
    let progress_session_row = dashboard_sidebar_rows(&dashboard, "$ local", 120, 40)[0];
    assert_eq!(
        dashboard.mouse_action(mouse(4, progress_session_row), area),
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
    let project_row = dashboard_sidebar_row(&project, "CONSIGINT", 120, 40);
    enter_terminal(&mut project);
    assert_eq!(
        project.mouse_action(click(5, project_row), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(project.focused_session().is_none());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &project, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, project_row).contains("▸"),
        "label click must not fold the project"
    );

    let mut project_disclosure = dashboard_fixture();
    let project_row = dashboard_sidebar_row(&project_disclosure, "CONSIGINT", 120, 40);
    enter_terminal(&mut project_disclosure);
    assert_eq!(
        project_disclosure.mouse_action(click(0, project_row), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &project_disclosure, 0))
        .unwrap();
    assert!(sidebar_row(&terminal, project_row).contains("▸"));
    assert_eq!(project_disclosure.focused_session(), Some(SessionId(1)));

    let mut workspace = dashboard_fixture();
    let workspace_row = dashboard_sidebar_row(&workspace, "auth", 120, 40);
    enter_terminal(&mut workspace);
    assert_eq!(
        workspace.mouse_action(click(5, workspace_row), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(workspace.focused_session().is_none());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &workspace, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, workspace_row).contains("▸"),
        "workspace label click must not fold"
    );

    let mut session = dashboard_fixture();
    let session_row = dashboard_sidebar_rows(&session, "$ local", 120, 40)[0];
    enter_terminal(&mut session);
    assert!({
        let _ = session.mouse_action(click(6, session_row), area);
        matches!(session.drain_outbox().as_slice(), [_])
    });
    assert_eq!(session.focused_session(), Some(SessionId(5)));
}

#[test]
fn sidebar_container_click_keeps_focus_on_another_assigned_pane() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    let action = dashboard.key(KeyCode::Char('v'));
    // `v` offers a session for the new pane; Esc keeps the suggestion.
    dashboard.key(KeyCode::Esc);
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

    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: auth_row,
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
                row: auth_row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(retained));

    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('w'));
    assert_eq!(
        dashboard.key(KeyCode::Char('t')),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.key(KeyCode::Tab); // Name
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("container-target".into()));
    assert_eq!(
        dashboard.event_action(Event::Key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT,)
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let create = only_request(dashboard.drain_outbox());
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
    // `v` offers a session for the new pane; Esc keeps the suggestion.
    dashboard.key(KeyCode::Esc);
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

    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: auth_row,
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
    // `v` offers a session for the new pane; Esc keeps the suggestion.
    dashboard.key(KeyCode::Esc);
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
        usage: ovrcr_protocol::UsageSnapshot::default(),
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
    let session_50_row = u16::try_from(
        rendered
            .iter()
            .position(|row| row.contains("session-50"))
            .expect("last session is rendered in the tree viewport"),
    )
    .unwrap();
    let quota_row = u16::try_from(
        rendered
            .iter()
            .position(|row| row.contains("QUOTA LEFT"))
            .expect("quota section row"),
    )
    .unwrap();
    assert_eq!(
        rendered[usize::from(session_50_row)],
        format!("▌    - session-50{}sh ", " ".repeat(19))
    );
    assert_eq!(
        rendered[usize::from(quota_row)].trim_end(),
        " QUOTA LEFT ───────────────────────────",
        "the quota section"
    );
    // The last tree row remains selectable; the quota section cannot select/close a session.
    for (index, row) in [session_50_row, quota_row].into_iter().enumerate() {
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
            assert!({
                let _ = action;
                matches!(dashboard.drain_outbox().as_slice(), [_])
            });
        } else {
            assert!(matches!(
                action,
                ovrcr::tui::DashboardAction::Redraw | ovrcr::tui::DashboardAction::None
            ));
        }
        assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    }
    // The quota section opened its details; close them.
    dashboard.key(KeyCode::Esc);

    dashboard.install_focus(SessionId(1));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let scroll_row = dashboard_sidebar_row(&dashboard, "session-1", 120, 40);
    enter_terminal(&mut dashboard);
    for _ in 0..3 {
        assert_eq!(
            dashboard.mouse_action(
                MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 4,
                    row: scroll_row,
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
    let session_rows: Vec<_> = ["session-4", "session-5", "session-6"]
        .into_iter()
        .map(|name| {
            (0..40)
                .find(|row| sidebar_row(&terminal, *row).contains(name))
                .unwrap_or_else(|| panic!("missing visible {name} row"))
        })
        .collect();
    assert!(
        session_rows.windows(2).all(|rows| rows[1] == rows[0] + 1),
        "scrolled tree rows stay consecutive: {session_rows:?}"
    );
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: session_rows[2],
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert_eq!(action, DashboardAction::Redraw);
    assert!(
        dashboard
            .drain_outbox()
            .iter()
            .all(|message| matches!(message.request, Request::SetView { .. }))
    );
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
    let local_row = sidebar_rows_with(&terminal, "$ local")[0];
    let review_row = sidebar_row_with(&terminal, "review");
    // The first local shell remains readable in the production layout.
    let color = buffer[(8, local_row)].fg;
    let Color::Rgb(r, g, b) = color else {
        panic!("expected RGB metadata: {color:?}")
    };
    assert!(
        r >= 160 && g >= 160 && b >= 160,
        "faint metadata: {color:?}"
    );
    // The selected review uses the soft selection background.
    assert_eq!(buffer[(8, review_row)].bg, Color::Rgb(49, 50, 68));
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
    assert_selected_sidebar_row(&terminal, "review");

    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_selected_sidebar_row(&terminal, "$ local");

    assert_eq!(
        dashboard.key(KeyCode::Char('k')),
        DashboardAction::Redraw,
        "landing on a workspace must not request a view"
    );
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_selected_sidebar_row(&terminal, "auth");

    dashboard.key(KeyCode::Char('k'));
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_selected_sidebar_row(&terminal, "CONSIGINT");

    dashboard.key(KeyCode::Char('j'));
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_selected_sidebar_row(&terminal, "auth");

    dashboard.key(KeyCode::Down);
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_selected_sidebar_row(&terminal, "$ local");

    dashboard.key(KeyCode::Up);
    assert!(dashboard.focused_session().is_none());
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_selected_sidebar_row(&terminal, "auth");
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
    let auth_row = sidebar_row_with(&terminal, "auth");
    assert!(
        sidebar_row(&terminal, auth_row).contains("▸"),
        "Enter on a workspace should fold it"
    );
    assert_eq!(
        sidebar_row(&terminal, auth_row),
        format!("▌ 󰘬 auth{}▸ 2 ", " ".repeat(27))
    );

    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, auth_row).contains("▸"),
        "second Enter should expand the workspace"
    );
    assert_selected_sidebar_row(&terminal, "auth");

    // Climb to the project header and fold it.
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let project_row = sidebar_row_with(&terminal, "CONSIGINT");
    assert!(
        sidebar_row(&terminal, project_row).contains("▸"),
        "Enter on a project should fold it"
    );
    assert_eq!(
        sidebar_row(&terminal, project_row),
        format!("▌CONSIGINT {} ▸ 2 ws ", "─".repeat(20))
    );

    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        !sidebar_row(&terminal, project_row).contains("▸"),
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
    assert_eq!(to_session, DashboardAction::Redraw);
    let request = only_request(dashboard.drain_outbox());
    acknowledge_view_request(&mut dashboard, request);
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    assert_eq!(dashboard.key(KeyCode::Char('k')), DashboardAction::Redraw);
    assert!(dashboard.focused_session().is_none());
    assert!({
        let _ = dashboard.key(KeyCode::Char('j'));
        matches!(dashboard.drain_outbox().as_slice(), [_])
    });
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
}

#[test]
fn enter_on_an_unready_session_refuses_and_stays_in_browse() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_unready(SessionId(1));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    // A loading pane is an expected limit: it says so plainly, without an error marker.
    let footer = rendered_footer(&dashboard, 120);
    assert_eq!(footer, "Pane is loading; retry input");
    assert_ne!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
}

#[test]
fn folded_workspace_and_project_show_hidden_attention_and_drop_reviewed_counts() {
    let mut hierarchy = fixture_hierarchy();
    let project = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == "consigint")
        .unwrap();
    let auth = project
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == "auth")
        .unwrap();
    set_attention(
        &mut auth.sessions[0],
        AgentActivity::ResponseReady,
        false,
        true,
    );
    set_attention(&mut auth.sessions[1], AgentActivity::Busy, true, false);
    let mut ready = auth.sessions[0].clone();
    let mut input = auth.sessions[1].clone();
    let lifecycle = project
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == "lifecycle")
        .unwrap();
    set_attention(&mut lifecycle.sessions[0], AgentActivity::Busy, false, true);

    let mut workspace_dashboard = dashboard_fixture();
    workspace_dashboard.install_hierarchy(hierarchy.clone());
    let area = Rect::new(0, 0, 120, 40);
    let click = |dashboard: &mut Dashboard, column, row| {
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        )
    };
    let sidebar_position = |rows: &[String], needle: &str| {
        rows.iter()
            .position(|row| row.split('│').next().unwrap_or(row).contains(needle))
            .unwrap_or_else(|| panic!("missing sidebar row containing {needle:?}: {rows:?}"))
    };
    let auth_row = sidebar_position(&rendered_rows(&workspace_dashboard, 120, 40), " auth");
    assert_eq!(
        click(&mut workspace_dashboard, 2, auth_row as u16),
        ovrcr::tui::DashboardAction::Redraw
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &workspace_dashboard, 0))
        .unwrap();
    let workspace_row =
        sidebar_position(&rendered_rows(&workspace_dashboard, 120, 40), " auth") as u16;
    assert!(
        sidebar_row(&terminal, workspace_row).contains("?1 ✓1 ▸ 2"),
        "folded workspace shows each active kind once: {}",
        sidebar_row(&terminal, workspace_row)
    );
    let buffer = terminal.backend().buffer();
    let input_glyph = (0..39)
        .find(|x| buffer[(*x, workspace_row)].symbol() == "?")
        .expect("Input count glyph");
    let ready_glyph = (0..39)
        .find(|x| buffer[(*x, workspace_row)].symbol() == "✓")
        .expect("Ready count glyph");
    assert_eq!(
        buffer[(input_glyph, workspace_row)].fg,
        Color::Rgb(249, 226, 175)
    );
    assert_eq!(
        buffer[(ready_glyph, workspace_row)].fg,
        Color::Rgb(148, 226, 213)
    );

    let mut project_dashboard = dashboard_fixture();
    project_dashboard.install_hierarchy(hierarchy);
    let project_row = sidebar_position(&rendered_rows(&project_dashboard, 120, 40), "CONSIGINT");
    assert_eq!(
        click(&mut project_dashboard, 0, project_row as u16),
        ovrcr::tui::DashboardAction::Redraw
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &project_dashboard, 0))
        .unwrap();
    let folded_project_row =
        sidebar_position(&rendered_rows(&project_dashboard, 120, 40), "CONSIGINT") as u16;
    assert!(
        sidebar_row(&terminal, folded_project_row).contains("?1 ✓1 ▸ 2 ws"),
        "folded project counts all descendant sessions: {}",
        sidebar_row(&terminal, folded_project_row)
    );

    ready.unread = None;
    workspace_dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
        Box::new(ready),
    )));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &workspace_dashboard, 0))
        .unwrap();
    let workspace_row =
        sidebar_position(&rendered_rows(&workspace_dashboard, 120, 40), " auth") as u16;
    let row = sidebar_row(&terminal, workspace_row);
    assert!(row.contains("?1"), "open Input remains counted: {row}");
    assert!(!row.contains("✓1"), "reviewed Ready disappears: {row}");

    input.agent.as_mut().unwrap().input_requests.clear();
    input
        .agent
        .as_mut()
        .unwrap()
        .activity
        .as_mut()
        .unwrap()
        .state = AgentActivity::Idle;
    workspace_dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
        Box::new(input),
    )));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &workspace_dashboard, 0))
        .unwrap();
    let workspace_row =
        sidebar_position(&rendered_rows(&workspace_dashboard, 120, 40), " auth") as u16;
    let row = sidebar_row(&terminal, workspace_row);
    assert!(
        !row.contains("?1") && !row.contains("✓1"),
        "closing the last request removes folded counts: {row}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn live_server_dashboard_renders_reported_unread_input_and_folded_counts() {
    use crate::live::{self, Live};
    use ovrcr::protocol as p;
    use std::time::{Duration, Instant};

    fn create_agent(live: &Live, name: &str) -> SessionSummary {
        let Response::CreatedSession(session) = live.request(Request::CreateSession(
            p::CreateSessionRequest {
                project: live::PROJECT.into(),
                workspace: live::WORKSPACE.into(),
                name: name.into(),
                label: None,
                argv: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    format!(
                        "umask 077; printf '%s' \"$OVRCR_HOOK_TOKEN\" > '{}'; while :; do sleep 1; done",
                        live.root.path().join(format!("{name}.capability")).display()
                    )
                    .into(),
                ],
                kind: p::SessionKind::Agent {
                    name: "fixture-agent".into(),
                },
            },
        ))
        else {
            panic!("live fixture Agent session was not created");
        };
        *session
    }

    fn report(
        live: &Live,
        session: &SessionSummary,
        input: bool,
    ) -> std::os::unix::net::UnixStream {
        let path = live
            .root
            .path()
            .join(format!("{}.capability", session.name));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "Agent child did not write its capability"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let token = std::fs::read_to_string(path).unwrap();
        assert_eq!(token.len(), 64, "fixture capability shape");
        let capability_bytes = std::array::from_fn(|index| {
            u8::from_str_radix(&token[index * 2..index * 2 + 2], 16).unwrap()
        });
        let capability = p::AgentSecret(capability_bytes);
        let mut admission = p::connect_server(&live.socket).unwrap();
        admission
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let Response::AgentOperation(p::AgentOperationResult::Reserved(reservation)) =
            p::client::request(
                &mut admission,
                1,
                Request::ReserveAgent(p::ReserveAgent {
                    pi_resume: None,
                    session: session.id,
                    capability: capability.clone(),
                    operation: "sidebar-fixture-reserve".into(),
                    expected_epoch: session.agent_epoch,
                    invocation: format!("fixture-{}", session.id.0),
                    provider: p::AgentProvider::Codex,
                }),
            )
            .unwrap()
        else {
            panic!("live fixture reporter was not reserved");
        };
        let auth = p::SupervisorAuth {
            session: session.id,
            lease: reservation.lease,
        };
        let mut supervisor = p::connect_server(&live.socket).unwrap();
        supervisor
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(
            p::client::request(&mut supervisor, 1, Request::SupervisorHello(auth.clone())).unwrap(),
            Response::Ok
        );
        drop(admission);
        let Response::AgentOperation(p::AgentOperationResult::Bound(binding)) = p::client::request(
            &mut supervisor,
            2,
            Request::Supervisor(p::SupervisorRequest {
                auth,
                operation: "sidebar-fixture-bind".into(),
                command: p::AgentCommand::Bind {
                    expected_binding: None,
                    conversation: format!("conversation-{}", session.id.0),
                },
            }),
        )
        .unwrap() else {
            panic!("live fixture reporter was not bound");
        };
        let observation = if input {
            p::AgentObservation::Input(vec![p::InputRequest {
                id: "question:sidebar-fixture".into(),
                kind: p::InputKind::Confirm,
            }])
        } else {
            p::AgentObservation::Activity(p::ActivitySample {
                state: p::AgentActivity::ResponseReady,
                quality: p::SampleQuality::Observed,
                turn: Some("fixture-turn".into()),
            })
        };
        assert_eq!(
            live.request(Request::AgentReport(p::AgentReport {
                session: session.id,
                capability: capability_bytes,
                sequence: None,
                update: p::AgentUpdate::Provider(p::ProviderReport {
                    binding,
                    revision: 1,
                    observation,
                }),
            })),
            Response::Ok
        );
        supervisor
    }

    let live = Live::binary();
    live.ready("feature/sidebar-unread");
    let unread = create_agent(&live, "live-unread");
    let input = create_agent(&live, "live-input");
    let _leases = [report(&live, &unread, false), report(&live, &input, true)];

    let Response::Hierarchy(current) = live.request(Request::List) else {
        panic!("live session list did not return hierarchy");
    };
    let published_input = current
        .projects
        .iter()
        .flat_map(|project| &project.workspaces)
        .flat_map(|workspace| &workspace.sessions)
        .find(|session| session.id == input.id)
        .expect("live input session");
    assert_eq!(
        published_input
            .agent
            .as_ref()
            .expect("live reporter snapshot")
            .input_requests
            .len(),
        1,
        "Server published the open Input request"
    );

    let mut stream = p::connect_server(&live.socket).unwrap();
    let Response::Hierarchy(hierarchy) =
        p::client::request(&mut stream, 900_000, Request::DashboardHello).unwrap()
    else {
        panic!("live Dashboard handshake did not return hierarchy");
    };
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(unread.id);

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    for name in ["live-unread", "live-input"] {
        let row = (0..20)
            .find(|y| {
                (0..39)
                    .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                    .collect::<String>()
                    .contains(name)
            })
            .unwrap_or_else(|| panic!("live session row {name} was not drawn"));
        let title = &terminal.backend().buffer()[(7, row)];
        assert_eq!(title.fg, Color::Rgb(205, 214, 244), "{name}");
        assert!(
            title.modifier.contains(Modifier::BOLD),
            "{name}: row={} title={title:?}",
            sidebar_row(&terminal, row)
        );
    }
    let workspace_row = (0..20)
        .find(|y| sidebar_row(&terminal, *y).contains("feature/sidebar-unread"))
        .expect("live fixture workspace row");
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 2,
                row: workspace_row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        ),
        DashboardAction::Redraw
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, workspace_row).contains("?1 ✓1 ▸ 3"),
        "live folded workspace includes Server-reported states: {}",
        sidebar_row(&terminal, workspace_row)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn live_server_dashboard_workspace_menu_opens_agent_form_for_that_workspace() {
    use crate::live::Live;
    use ovrcr::protocol as p;

    let live = Live::binary();
    live.ready("feature/sidebar-create-menu");
    let mut stream = p::connect_server(&live.socket).unwrap();
    let Response::Hierarchy(hierarchy) =
        p::client::request(&mut stream, 900_000, Request::DashboardHello).unwrap()
    else {
        panic!("live Dashboard handshake did not return hierarchy");
    };

    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: area.height,
        cols: area.width,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(hierarchy.clone());
    if let Some(session) = hierarchy
        .projects
        .iter()
        .flat_map(|project| &project.workspaces)
        .flat_map(|workspace| &workspace.sessions)
        .next()
    {
        dashboard.install_focus(session.id);
    }

    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let workspace_row = (0..area.height)
        .find(|row| sidebar_row(&terminal, *row).contains("feature/sidebar-create-menu"))
        .expect("live workspace row");
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 5,
            row: workspace_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let (plus_column, plus_row) = rendered_cell_sequence(&terminal, "[+]", area.width, area.height)
        .expect("hovered live [+]");
    assert_eq!(plus_row, workspace_row);
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: plus_column + 1,
                row: plus_row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        DashboardAction::Redraw
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let (agent_column, agent_row) =
        rendered_cell_sequence(&terminal, "New agent session", area.width, area.height)
            .expect("workspace menu action rendered");
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: agent_column,
                row: agent_row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ),
        DashboardAction::Redraw
    );

    for message in dashboard.drain_outbox() {
        let request_id = message.request_id;
        let request = message.request;
        if matches!(&request, Request::Inspect | Request::List) {
            let response = p::client::request(&mut stream, request_id, request)
                .expect("live Server answers Dashboard form requests");
            dashboard.handle_server_message(ServerMessage::Response {
                request_id,
                response,
            });
        }
    }
    let form = rendered_rows(&dashboard, area.width, area.height).join("\n");
    assert!(form.contains("New agent session"), "{form}");
    assert!(
        form.contains("fixture / feature/sidebar-create-menu"),
        "live project and workspace context did not fill the existing form: {form}"
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
        sidebar_row_with(&terminal, "lifecycle") < 40,
        "j from folded auth should land on the next visible workspace"
    );
    assert_selected_sidebar_row(&terminal, "lifecycle");
}

#[test]
fn jk_from_a_hidden_session_steps_off_the_folded_header() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: auth_row,
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
    assert_selected_sidebar_row(&terminal, "lifecycle");
    assert!(
        sidebar_row_with(&terminal, "lifecycle") < 40,
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
        usage: ovrcr_protocol::UsageSnapshot::default(),
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
    let project_row = (0..40)
        .find(|row| sidebar_row(&terminal, *row).contains("PROJECT"))
        .expect("project is visible in the tree viewport");
    assert_eq!(selection_bar_row(&terminal), Some(project_row));
}

#[test]
fn container_selection_does_not_close_or_switch_a_split() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    let action = dashboard.key(KeyCode::Char('v'));
    // `v` offers a session for the new pane; Esc keeps the suggestion.
    dashboard.key(KeyCode::Esc);
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
    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: auth_row,
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
    let project_row = sidebar_row_with(&terminal, "CONSIGINT");
    assert!(
        !sidebar_row(&terminal, project_row).contains("▸"),
        "Enter must not fold a project that was removed"
    );
}

#[test]
fn container_click_in_terminal_mode_returns_to_browse() {
    let mut dashboard = dashboard_fixture();
    let project_row = dashboard_sidebar_row(&dashboard, "CONSIGINT", 120, 40);
    enter_terminal(&mut dashboard);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: project_row,
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
    let review_row = sidebar_row_with(&terminal, "review");
    let local_row = sidebar_rows_with(&terminal, "$ local")[0];
    let project_row = sidebar_row_with(&terminal, "CONSIGINT");
    let review = sidebar_row(&terminal, review_row);
    assert!(
        !review.contains("[x]") && review.contains("claude"),
        "a highlighted row hides the mark until the pointer is over it, got {review:?}"
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: review_row,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let review = sidebar_row(&terminal, review_row);
    assert!(
        review.trim_end().ends_with("[x]") && !review.contains("claude"),
        "hover replaces the agent label with the close mark, got {review:?}"
    );
    let mark_x = review.trim_end().len() - 3;
    assert_eq!(
        terminal.backend().buffer()[(mark_x as u16, review_row)].fg,
        Color::Rgb(203, 166, 247),
        "close mark uses the title-bar action colour"
    );
    let local = sidebar_row(&terminal, local_row);
    assert!(
        !local.contains("[x]"),
        "unselected session must not show the close mark, got {local:?}"
    );
    let project = sidebar_row(&terminal, project_row);
    assert!(
        !project.contains("[x]"),
        "project row must not show the close mark, got {project:?}"
    );
}

#[test]
fn clicking_the_close_mark_confirms_archive_of_the_highlighted_session() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    hover(&mut dashboard, 10, review_row, area);
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: review_row,
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    hover(&mut dashboard, 10, review_row, area);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: review_row,
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
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    enter_terminal(&mut dashboard);
    hover(&mut dashboard, 10, review_row, area);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: review_row,
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
    let implement_row = dashboard_sidebar_row(&dashboard, "implement", 120, 40);
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    dashboard.mouse_action(moved(10, implement_row), area);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, implement_row).contains("[x]"),
        "hovered session must show the close mark, got {:?}",
        sidebar_row(&terminal, implement_row)
    );
    assert!(
        !sidebar_row(&terminal, review_row).contains("[x]"),
        "only the hovered row shows the close mark"
    );
    dashboard.mouse_action(moved(80, implement_row), area);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, implement_row).contains("[x]"));
    assert!(!sidebar_row(&terminal, review_row).contains("[x]"));
}

#[test]
fn clicking_a_hovered_close_mark_archives_that_row_without_switching_the_pane() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let local_row = dashboard_sidebar_rows(&dashboard, "$ local", 120, 40)[0];
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: local_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: local_row,
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
    let local_row = dashboard_sidebar_rows(&dashboard, "$ local", 120, 40)[0];
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: local_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: local_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, local_row).starts_with('▌'),
        "confirm must highlight the clicked row, got {:?}",
        sidebar_row(&terminal, local_row)
    );
    assert!(
        !sidebar_row(&terminal, review_row).starts_with('▌'),
        "focused pane session must not stay highlighted during confirm, got {:?}",
        sidebar_row(&terminal, review_row)
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, local_row).starts_with('▌'));
    assert!(sidebar_row(&terminal, review_row).starts_with('▌'));
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
    // `v` offers a session for the new pane; Esc keeps the suggestion.
    dashboard.key(KeyCode::Esc);
    pump_view(&mut dashboard, split);
    dashboard.key(KeyCode::Tab);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    let local_row = dashboard_sidebar_rows(&dashboard, "$ local", 120, 40)[1];
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: local_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: local_row,
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
    let implement_row = dashboard_sidebar_row(&dashboard, "implement", 120, 40);
    let lifecycle_row = dashboard_sidebar_row(&dashboard, "lifecycle", 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: implement_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: lifecycle_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, implement_row).contains("[x]"));
    let review_row = sidebar_row_with(&terminal, "review");
    assert!(!sidebar_row(&terminal, review_row).contains("[x]"));
    assert!(
        sidebar_row(&terminal, lifecycle_row).contains("[x]"),
        "{:?}",
        sidebar_row(&terminal, lifecycle_row)
    );
}

#[test]
fn palette_keeps_the_mouse_from_a_second_close_mark() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    let implement_row = dashboard_sidebar_row(&dashboard, "implement", 120, 40);
    hover(&mut dashboard, 10, review_row, area);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: review_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: implement_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: implement_row,
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
    let implement_row = dashboard_sidebar_row(&dashboard, "implement", 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: implement_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.event_action(Event::FocusLost);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(!sidebar_row(&terminal, implement_row).contains("[x]"));
    let review_row = sidebar_row_with(&terminal, "review");
    assert!(!sidebar_row(&terminal, review_row).contains("[x]"));
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
    assert_eq!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Redraw
    );
    let begin = only_request(dashboard.drain_outbox());
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
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    hover(&mut dashboard, 10, review_row, Rect::new(0, 0, 120, 40));
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 36,
                row: review_row,
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
    let implement_row = dashboard_sidebar_row(&dashboard, "implement", 120, 40);
    hover(&mut dashboard, 10, implement_row, Rect::new(0, 0, 120, 40));
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 36,
                row: implement_row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    let action = dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: review_row,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert!(
        {
            let _ = action;
            matches!(
                dashboard.drain_outbox().as_slice(),
                [ClientMessage {
                    request: Request::SetView { .. },
                    ..
                }]
            )
        },
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
    let review_row = dashboard_sidebar_row(&dashboard, "review", 120, 40);
    assert!(!sidebar_contains_close_mark(&dashboard));
    dashboard.event_action(Event::FocusLost);
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 36,
                row: review_row,
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
    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: auth_row,
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
    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    let local_row = dashboard_sidebar_rows(&dashboard, "$ local", 120, 40)[0];
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: auth_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: local_row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        sidebar_row(&terminal, local_row).contains("[x]"),
        "{:?}",
        sidebar_row(&terminal, local_row)
    );
    let review_row = sidebar_row_with(&terminal, "review");
    assert!(!sidebar_row(&terminal, review_row).contains("[x]"));
}

#[test]
fn hovering_a_workspace_shows_close_mark_and_click_opens_remove() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let auth_row = dashboard_sidebar_row(&dashboard, "auth", 120, 40);
    let project_row = dashboard_sidebar_row(&dashboard, "CONSIGINT", 120, 40);
    hover(&mut dashboard, 10, auth_row, area);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let row = sidebar_row(&terminal, auth_row);
    assert!(row.contains("[x]") && row.contains("auth"), "{row:?}");
    assert!(!sidebar_row(&terminal, project_row).contains("[x]"));
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: auth_row,
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
