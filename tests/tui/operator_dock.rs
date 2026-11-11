//! The Operator dock is the last sidebar row: the first project leads the
//! sidebar, attention and quota stay between the tree and the dock, and the
//! title bar carries no Operator shortcut. Every case drives the production
//! Dashboard input/render path with real protocol replies.

use crate::*;
use ovrcr::protocol::{
    OperatorConversationId, OperatorConversationSummary, OperatorRequest, OperatorResponse,
    QuotaSnapshot, QuotaState, QuotaWindow, SessionKind, SessionRunId,
};

fn summary() -> OperatorConversationSummary {
    OperatorConversationSummary {
        reasoning: None,
        run: SessionRunId(1),
        worker_stopped: true,
        id: OperatorConversationId(7),
        session: SessionId(99),
        model: None,
        title: "Operator".into(),
        current_turn: None,
    }
}

fn operator_requests(messages: &[ClientMessage]) -> Vec<&OperatorRequest> {
    messages
        .iter()
        .filter_map(|message| match &message.request {
            Request::Operator(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn respond(dashboard: &mut Dashboard, request_id: u64, value: OperatorConversationSummary) {
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Operator(OperatorResponse::Opened(value)),
    });
}

fn click(dashboard: &mut Dashboard, area: Rect, column: u16, row: u16) -> DashboardAction {
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            KeyModifiers::NONE,
        ),
        area,
    )
}

/// Clicks the dock once and returns the single Open it sends.
fn open_once(dashboard: &mut Dashboard, area: Rect, column: u16, row: u16) -> u64 {
    assert_eq!(
        click(dashboard, area, column, row),
        DashboardAction::Redraw,
        "the dock row is navigation"
    );
    let mut out = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&out),
        [&OperatorRequest::Open],
        "one Open from the dock: {out:?}"
    );
    out.drain(..)
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("an Open request")
        .request_id
}

fn session_id_in(row: &str) -> Option<u64> {
    row.split("session-")
        .nth(1)?
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn quota_with_windows() -> QuotaSnapshot {
    let mut snapshot = QuotaSnapshot::default();
    for provider in [
        &mut snapshot.claude,
        &mut snapshot.codex,
        &mut snapshot.grok,
        &mut snapshot.cursor,
    ] {
        provider.state = QuotaState::Current;
        provider.reason = None;
        provider.observed_unix_ms = Some(0);
        provider.checked_unix_ms = Some(0);
        provider.next_check_unix_ms = None;
        provider.windows = ["5h", "7d"]
            .into_iter()
            .map(|label| QuotaWindow {
                id: label.into(),
                label: label.into(),
                general: true,
                used_basis_points: Some(4_200),
                over_limit: false,
                resets_unix_ms: None,
            })
            .collect();
    }
    snapshot
}

#[test]
fn the_first_project_leads_the_sidebar_and_the_dock_ends_it() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        rows[0].starts_with("󰚩 OVRCR"),
        "the title keeps its brand: {:?}",
        rows[0]
    );
    assert!(
        !rows[0].contains("Operator") && !rows[0].contains("Ctrl-o"),
        "the title bar carries no Operator shortcut: {:?}",
        rows[0]
    );
    assert!(
        rows[1].contains("CONSIGINT"),
        "the first project sits directly below the title: {:?}",
        &rows[..3]
    );
    assert!(
        rows[38].contains("Operator"),
        "the dock is the last sidebar row above the footer: {:?}",
        &rows[36..]
    );
    assert!(rows[39].starts_with("BROWSE"), "{:?}", rows[39]);
}

#[test]
fn retired_title_controls_open_neither_surface() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    let before = rendered_rows(&dashboard, 120, 40);

    // The old `Operator [o]` label and `[Ctrl-o]` control are inert.
    assert_eq!(click(&mut dashboard, area, 10, 0), DashboardAction::None);
    assert_eq!(click(&mut dashboard, area, 34, 0), DashboardAction::None);
    let out = dashboard.drain_outbox();
    assert!(
        operator_requests(&out).is_empty(),
        "a retired title control must not open Operator: {out:?}"
    );
    let after = rendered_rows(&dashboard, 120, 40);
    assert!(
        !after.iter().any(|row| row.contains("Quick message")),
        "the retired Quick control opens nothing: {after:?}"
    );
    assert!(
        !after.iter().any(|row| row.contains("Operator · pi")),
        "the retired label opens nothing: {after:?}"
    );
    assert_eq!(
        after.iter().map(|row| row.trim_end()).collect::<Vec<_>>(),
        before.iter().map(|row| row.trim_end()).collect::<Vec<_>>(),
        "the frame is unchanged"
    );

    // Browse `o` still opens the shared conversation.
    dashboard.key(KeyCode::Char('o'));
    let out = dashboard.drain_outbox();
    assert_eq!(operator_requests(&out), [&OperatorRequest::Open]);
    let request = out
        .iter()
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("the keyboard route sends Open");
    respond(&mut dashboard, request.request_id, summary());
    let open = rendered_rows(&dashboard, 120, 40);
    assert!(
        open.iter().any(|row| row.contains("Operator · pi")),
        "{open:?}"
    );

    // Commands still reaches Quick message after the title control retired.
    let mut commands = dashboard_fixture();
    commands.install_area(area);
    palette_search(&mut commands, "Quick message");
    commands.key(KeyCode::Enter);
    let quick = rendered_rows(&commands, 120, 40);
    assert!(
        quick.iter().any(|row| row.contains("Quick message")),
        "the Commands entry still opens Quick message: {quick:?}"
    );
}

#[test]
fn both_ends_of_the_dock_open_the_same_conversation() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();

    // Left end of the row.
    let open = open_once(&mut dashboard, area, 0, 38);
    respond(&mut dashboard, open, summary());
    dashboard.drain_outbox();
    let open_rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        open_rows[38].starts_with('▌'),
        "the open conversation draws its selection marker: {:?}",
        open_rows[38]
    );
    assert!(open_rows[38].contains("Operator"), "{:?}", open_rows[38]);

    // Returning to Browse restores the terminal panes and the quiet row.
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::EnterBrowse);
    let browse_rows = rendered_rows(&dashboard, 120, 40);
    assert!(!browse_rows[38].starts_with('▌'), "{:?}", browse_rows[38]);
    assert!(
        browse_rows[38].contains("Operator"),
        "{:?}",
        browse_rows[38]
    );
    assert!(
        browse_rows[1].contains("CONSIGINT"),
        "the tree is back: {:?}",
        &browse_rows[..3]
    );

    // Right end of the row opens the same singleton.
    let open = open_once(&mut dashboard, area, 38, 38);
    respond(&mut dashboard, open, summary());
    let reopened = rendered_rows(&dashboard, 120, 40);
    assert!(reopened[38].starts_with('▌'), "{:?}", reopened[38]);
    assert!(
        reopened.iter().any(|row| row.contains("Operator · pi")),
        "{reopened:?}"
    );
}

#[test]
fn representative_frames_keep_the_dock_at_the_sidebar_foot() {
    for (cols, rows) in [(140, 40), (100, 24), (80, 24), (60, 24), (60, 16)] {
        let area = Rect::new(0, 0, cols, rows);
        let mut dashboard = dashboard_fixture();
        dashboard.install_area(area);
        dashboard.drain_outbox();
        let frame = rendered_rows(&dashboard, cols, rows);
        let dock = usize::from(rows) - 2;
        assert!(
            frame[0].starts_with("󰚩 OVRCR") && !frame[0].contains("Operator"),
            "{cols}x{rows}: {:?}",
            frame[0]
        );
        assert!(
            frame[1].contains("CONSIGINT"),
            "{cols}x{rows}: the first project leads: {:?}",
            &frame[..3]
        );
        assert!(
            frame[dock].contains("Operator"),
            "{cols}x{rows}: the dock is the last sidebar row: {:?}",
            &frame[dock - 2..]
        );

        let open = open_once(&mut dashboard, area, 0, dock as u16);
        respond(&mut dashboard, open, summary());
        let open_frame = rendered_rows(&dashboard, cols, rows);
        let header: String = if cols < 100 {
            // Narrow frames open the conversation full-width by default.
            assert!(
                open_frame[1].starts_with("Operator · pi"),
                "{cols}x{rows}: full-width conversation: {:?}",
                &open_frame[..3]
            );
            open_frame[1].chars().take(14).collect()
        } else {
            assert!(
                open_frame[1].contains("CONSIGINT"),
                "{cols}x{rows}: the sidebar stays: {:?}",
                &open_frame[..3]
            );
            open_frame[1].chars().skip(40).take(14).collect()
        };
        assert_eq!(header, "Operator · pi ", "{cols}x{rows}");

        if cols < 100 {
            // The ordinary sidebar stays available on demand, dock included.
            dashboard.ctrl('l');
            let shown = rendered_rows(&dashboard, cols, rows);
            assert!(
                shown[1].contains("CONSIGINT"),
                "{cols}x{rows}: Ctrl-l reveals the sidebar: {:?}",
                &shown[..3]
            );
            assert!(
                shown[dock].starts_with('▌') && shown[dock].contains("Operator"),
                "{cols}x{rows}: the selected dock returns with the sidebar: {:?}",
                shown[dock]
            );
        }
    }
}

#[test]
fn the_floor_keeps_the_composer_and_drafts_survive_below_it() {
    let area = Rect::new(0, 0, 60, 16);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    let open = open_once(&mut dashboard, area, 0, 14);
    respond(&mut dashboard, open, summary());
    dashboard.drain_outbox();
    for character in "unsent draft".chars() {
        dashboard.key(KeyCode::Char(character));
    }
    let floor = rendered_rows(&dashboard, 60, 16);
    assert!(
        floor.iter().any(|row| row.contains("unsent draft")),
        "the composer works at 60×16: {floor:?}"
    );

    dashboard.install_area(Rect::new(0, 0, 59, 16));
    let below = rendered_rows(&dashboard, 59, 16);
    assert!(
        below.iter().any(|row| row.contains("60×16")),
        "below the floor the resize guidance stays: {below:?}"
    );

    dashboard.install_area(area);
    let returned = rendered_rows(&dashboard, 60, 16);
    assert!(
        returned.iter().any(|row| row.contains("unsent draft")),
        "the draft survives the floor: {returned:?}"
    );
}

#[test]
fn a_hidden_or_short_sidebar_yields_the_dock_but_keeps_the_keyboard_route() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    assert!(rendered_rows(&dashboard, 120, 40)[38].contains("Operator"));

    // A hidden sidebar hides the dock row too.
    dashboard.key(KeyCode::Char('b'));
    let hidden = rendered_rows(&dashboard, 120, 40);
    assert!(
        !hidden.iter().any(|row| row.contains("Operator")),
        "{hidden:?}"
    );
    assert!(!hidden[1].contains("CONSIGINT"), "{hidden:?}");
    dashboard.drain_outbox();
    dashboard.key(KeyCode::Char('o'));
    let out = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&out),
        [&OperatorRequest::Open],
        "the keyboard route works without the dock: {out:?}"
    );
    let request = out
        .iter()
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("Open");
    respond(&mut dashboard, request.request_id, summary());
    let open = rendered_rows(&dashboard, 120, 40);
    assert!(open[1].starts_with("Operator · pi"), "{:?}", &open[..3]);
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char('b'));

    // Six rows leave four sidebar lines: three tree lines plus the dock row.
    dashboard.install_area(Rect::new(0, 0, 120, 6));
    let roomy = rendered_rows(&dashboard, 120, 6);
    assert!(roomy[4].contains("Operator"), "{roomy:?}");
    for row in &roomy[1..4] {
        assert!(
            !row.contains("QUOTA"),
            "the tree keeps three lines: {roomy:?}"
        );
    }

    // Five rows leave three: the dock yields them to the tree, whose three
    // drawn lines are the same.
    dashboard.install_area(Rect::new(0, 0, 120, 5));
    let tight = rendered_rows(&dashboard, 120, 5);
    assert!(
        !tight.iter().any(|row| row.contains("Operator")),
        "{tight:?}"
    );
    assert_eq!(
        &roomy[1..4],
        &tight[1..4],
        "the short sidebar gives the dock row to the tree"
    );
    assert!(roomy[5].starts_with("BROWSE"), "{roomy:?}");
    assert!(tight[4].starts_with("BROWSE"), "{tight:?}");
    dashboard.drain_outbox();
    dashboard.key(KeyCode::Char('o'));
    let out = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&out),
        [&OperatorRequest::Open],
        "the keyboard route works in a short sidebar: {out:?}"
    );
}

#[test]
fn a_fifty_session_tree_scrolls_under_a_pinned_dock() {
    let area = Rect::new(0, 0, 120, 40);
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    let template = hierarchy.projects[0].workspaces[0].sessions[0].clone();
    hierarchy.projects[0].workspaces[0].sessions = (1..=50)
        .map(|id| {
            let mut session = template.clone();
            session.id = SessionId(id);
            session.run = SessionRunId(id);
            session.name = format!("session-{id}");
            session.label = "sh".into();
            session.agent = None;
            session.unread = None;
            session.activity = AgentActivity::Unknown;
            session
        })
        .collect();
    let mut waiting = hierarchy.projects[0].workspaces[0].sessions[0].clone();
    waiting.activity = AgentActivity::WaitingInput;
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(50));
    dashboard.drain_outbox();

    let initial = rendered_rows(&dashboard, 120, 40);
    assert!(
        initial[1].contains("session-"),
        "the scrolled tree leads the sidebar: {:?}",
        &initial[..3]
    );
    assert!(initial[38].contains("Operator"), "{:?}", &initial[36..]);
    let quota_row = initial
        .iter()
        .position(|row| row.contains("QUOTA LEFT"))
        .expect("quota section");

    // Scrolling the tree never moves the dock.
    for _ in 0..3 {
        assert_eq!(
            click_wheel(&mut dashboard, area, MouseEventKind::ScrollUp, 10, 5),
            DashboardAction::Redraw
        );
    }
    let scrolled = rendered_rows(&dashboard, 120, 40);
    assert_eq!(
        scrolled[38], initial[38],
        "the dock is pinned while the tree scrolls"
    );
    assert_ne!(
        &scrolled[1..quota_row],
        &initial[1..quota_row],
        "the tree scrolled"
    );

    // The last visible session row selects exactly the session it draws.
    let last = (1..quota_row)
        .rev()
        .find(|&row| scrolled[row].contains("session-"))
        .expect("a visible session row");
    let identity = session_id_in(&scrolled[last]).expect("the drawn session id");
    assert_eq!(
        click(&mut dashboard, area, 4, last as u16),
        DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(identity)));

    // A quota update changes the section but not the dock.
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
        quota_with_windows(),
    ))));
    dashboard.drain_outbox();
    let requoted = rendered_rows(&dashboard, 120, 40);
    assert_eq!(
        requoted[38], initial[38],
        "a quota update leaves the dock in place"
    );
    assert_ne!(
        requoted[..38],
        initial[..38],
        "the quota section changed shape"
    );

    // An attention update adds the strip above quota without moving the dock.
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        waiting,
    ))));
    dashboard.drain_outbox();
    let attended = rendered_rows(&dashboard, 120, 40);
    assert_eq!(
        attended[38], initial[38],
        "an attention update leaves the dock in place"
    );
    let strip = attended
        .iter()
        .position(|row| row.contains("needs input") || row.contains("need input"))
        .expect("attention strip");
    assert_eq!(
        click(&mut dashboard, area, 8, strip as u16),
        DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(1)),
        "the strip jumps to the session it counts"
    );

    // The quota section opens its details at its drawn row.
    let quota = attended
        .iter()
        .position(|row| row.contains("QUOTA LEFT"))
        .expect("quota section");
    assert_eq!(
        click(&mut dashboard, area, 2, quota as u16),
        DashboardAction::Redraw
    );
    let details = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(details.contains("Quota details"), "{details}");
    dashboard.key(KeyCode::Esc);

    // The divider responds on the dock's own row, and the dock keeps the last row.
    for (kind, column) in [
        (MouseEventKind::Down(MouseButton::Left), 39),
        (MouseEventKind::Drag(MouseButton::Left), 19),
        (MouseEventKind::Up(MouseButton::Left), 19),
    ] {
        assert_eq!(
            dashboard.mouse_action(mouse_event(kind, column, 38, KeyModifiers::NONE), area),
            DashboardAction::Redraw
        );
    }
    assert_eq!(dashboard.pane_rects(area)[0].terminal.x, 20);
    let resized = rendered_rows(&dashboard, 120, 40);
    assert!(
        resized[38].contains("Operator"),
        "the dock keeps the last row at the minimum width: {:?}",
        &resized[36..]
    );
    let open = open_once(&mut dashboard, area, 18, 38);
    respond(&mut dashboard, open, summary());
}

fn click_wheel(
    dashboard: &mut Dashboard,
    area: Rect,
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> DashboardAction {
    dashboard.mouse_action(mouse_event(kind, column, row, KeyModifiers::NONE), area)
}

fn jump_hierarchy(count: u64) -> HierarchySnapshot {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    hierarchy.projects[0].workspaces.truncate(1);
    let project = hierarchy.projects[0].name.clone();
    let workspace = hierarchy.projects[0].workspaces[0].name.clone();
    hierarchy.projects[0].workspaces[0].sessions = (1..=count)
        .map(|id| {
            let mut session = session_summary(
                id,
                &project,
                &workspace,
                &format!("agent-{id}"),
                "fixture-agent",
                Some(u32::try_from(id).unwrap()),
                0,
            );
            session.kind = SessionKind::Agent {
                name: "fixture-agent".into(),
            };
            session.phase = SessionPhase::Running;
            session.run = SessionRunId(id);
            session
        })
        .collect();
    hierarchy
}

#[test]
fn jump_labels_stay_in_the_tree_and_never_mark_the_dock() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(jump_hierarchy(2));
    dashboard.install_focus(SessionId(2));
    dashboard.install_screen(SessionId(2), &[]);
    dashboard.drain_outbox();
    dashboard.ctrl('g');
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        rows.iter().any(|row| row.contains("[1]")),
        "Agent sessions keep their labels: {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("[2]")),
        "Agent sessions keep their labels: {rows:?}"
    );
    assert!(
        !rows[38].contains('['),
        "the dock is outside the numbered targets: {:?}",
        rows[38]
    );
    assert!(rows[38].contains("Operator"), "{:?}", rows[38]);

    // A number still addresses the Agent session it marks.
    dashboard.key(KeyCode::Char('1'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
}
