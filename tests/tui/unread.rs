use super::*;
use ovrcr::protocol::{AgentBinding, AgentProvider, ReadyObservation};
use ovrcr::tui::DashboardAction;

fn observation(turn: &str, revision: u64) -> ReadyObservation {
    observation_for(AgentProvider::Codex, turn, revision)
}

fn observation_for(provider: AgentProvider, turn: &str, revision: u64) -> ReadyObservation {
    ReadyObservation {
        binding: AgentBinding {
            provider,
            invocation: "private-invocation".into(),
            conversation: "private-conversation".into(),
            generation: 1,
        },
        turn: Some(turn.into()),
        activity_revision: revision,
    }
}

fn focused_summary(d: &Dashboard) -> SessionSummary {
    let id = d.focused_session().unwrap();
    fixture_hierarchy()
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|session| session.id == id)
        .expect("focused fixture session")
}

fn publish_session(d: &mut Dashboard, summary: SessionSummary) {
    d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));
}

fn draw(d: &mut Dashboard, width: u16, height: u16) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    d.draw(&mut terminal).unwrap();
}

#[test]
fn unread_review_key_captures_visible_observation_without_optimistic_clear() {
    let mut d = screen_ready_dashboard(b"VISIBLE_RESPONSE");
    let unread = observation("one", 2);
    let mut summary = focused_summary(&d);
    summary.unread = Some(unread.clone());
    publish_session(&mut d, summary);
    draw(&mut d, 120, 30);
    let id = d.focused_session().unwrap();
    d.install_focus(id);
    d.key(KeyCode::Enter);
    assert_eq!(
        d.key(KeyCode::Char('R')),
        DashboardAction::PtyBytes(b"R".to_vec())
    );
    d.ctrl('g');
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        assert!(!matches!(
            d.key_action(KeyEvent::new_with_kind(
                KeyCode::Char('R'),
                KeyModifiers::NONE,
                kind
            )),
            DashboardAction::Request(_)
        ));
    }
    let DashboardAction::Request(message) = d.key(KeyCode::Char('R')) else {
        panic!("Browse R must submit mark-reviewed");
    };
    assert_eq!(
        message.request,
        Request::MarkReviewed {
            session: id,
            expected: unread
        }
    );
    let mut newer = focused_summary(&d);
    newer.unread = Some(observation("two", 4));
    publish_session(&mut d, newer);
    d.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    draw(&mut d, 120, 30);
    let DashboardAction::Request(later) = d.key(KeyCode::Char('R')) else {
        panic!("new unread must stay reviewable after a late ack");
    };
    assert_eq!(
        later.request,
        Request::MarkReviewed {
            session: id,
            expected: observation("two", 4),
        },
        "late acknowledgement cannot clear new snapshot"
    );
    d.key(KeyCode::Char(':'));
    assert!(
        !matches!(
            d.key(KeyCode::Char('R')),
            DashboardAction::Request(ClientMessage {
                request: Request::MarkReviewed { .. },
                ..
            })
        ),
        "palette owns keys"
    );
}

#[test]
fn unread_and_unavailable_remain_visible_in_narrow_single_and_split_panes() {
    for width in [80, 100, 160] {
        for split in [false, true] {
            let mut d = dashboard_fixture();
            let mut session = focused_summary(&d);
            session.unread = Some(observation("one", 2));
            session.activity = AgentActivity::Busy;
            session.name = "very-long-session-name-for-clipping".into();
            session.agent = None;
            publish_session(&mut d, session);
            if split {
                d.split_pane();
                let _ = d.ctrl('g');
                d.key(KeyCode::Tab);
            }
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal.draw(|f| draw_dashboard_at(f, &d, 0)).unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
            assert!(
                text.contains("Unread Unavailable"),
                "{width} split={split}: {text}"
            );
            assert!(
                text.contains("⠋ ●"),
                "separate activity and unread glyphs: {text}"
            );
            assert!(!text.contains("private-"));
        }
    }
}

#[test]
fn unread_review_action_is_discoverable_in_palette_and_terminal_keys() {
    let mut d = screen_ready_dashboard(b"REVIEWABLE");
    let mut summary = focused_summary(&d);
    summary.unread = Some(observation("one", 2));
    publish_session(&mut d, summary);
    draw(&mut d, 120, 30);
    d.key(KeyCode::Char(':'));
    for ch in "Mark reviewed".chars() {
        d.key(KeyCode::Char(ch));
    }
    assert!(palette_text(&d).contains("Mark reviewed"));
    assert!(matches!(
        d.key(KeyCode::Enter),
        DashboardAction::Request(ClientMessage {
            request: Request::MarkReviewed { .. },
            ..
        })
    ));
    d.key(KeyCode::Char(' '));
    d.key(KeyCode::Char('t'));
    assert!(palette_text(&d).contains("Mark reviewed"));
    assert!(matches!(
        d.key(KeyCode::Char('R')),
        DashboardAction::Request(ClientMessage {
            request: Request::MarkReviewed { .. },
            ..
        })
    ));
}

#[test]
fn pi_unread_is_reviewable_with_the_same_key_and_identity() {
    let mut dashboard = dashboard_fixture();
    let mut summary = focused_summary(&dashboard);
    summary.unread = Some(observation_for(AgentProvider::Pi, "cycle-1", 3));
    publish_session(&mut dashboard, summary.clone());
    draw(&mut dashboard, 120, 30);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Char('R')) else {
        panic!("R did not mint a request");
    };
    assert_eq!(
        message.request,
        Request::MarkReviewed {
            session: summary.id,
            expected: summary.unread.clone().unwrap()
        }
    );
}
