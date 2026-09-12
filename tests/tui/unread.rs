use super::*;
use ovrcr::protocol::{AgentBinding, AgentProvider, ReadyObservation};
use ovrcr::tui::{DashboardAction, InputMode};

fn observation(turn: &str, revision: u64) -> ReadyObservation {
    ReadyObservation {
        binding: AgentBinding {
            provider: AgentProvider::Codex,
            invocation: "private-invocation".into(),
            conversation: "private-conversation".into(),
            generation: 1,
        },
        turn: Some(turn.into()),
        activity_revision: revision,
    }
}

fn selected(d: &mut Dashboard) -> &mut SessionSummary {
    let id = d.focused_session().unwrap();
    d.hierarchy
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.workspaces)
        .flat_map(|w| &mut w.sessions)
        .find(|s| s.id == id)
        .unwrap()
}

#[test]
fn unread_review_key_captures_visible_observation_without_optimistic_clear() {
    let mut d = screen_ready_dashboard(b"VISIBLE_RESPONSE");
    let unread = observation("one", 2);
    selected(&mut d).unread = Some(unread.clone());
    let original = selected(&mut d).clone();
    let id = original.id;
    // Selection and terminal entry leave review state alone.
    d.select_session(id);
    d.key(KeyCode::Enter);
    assert_eq!(
        d.key(KeyCode::Char('R')),
        DashboardAction::PtyBytes(b"R".to_vec())
    );
    assert_eq!(selected(&mut d), &original);
    d.ctrl('g');
    assert_eq!(d.mode, InputMode::Browse);
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
    assert_eq!(
        selected(&mut d),
        &original,
        "only server publication clears unread"
    );
    selected(&mut d).unread = Some(observation("two", 4));
    d.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        selected(&mut d).unread,
        Some(observation("two", 4)),
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
            let session = selected(&mut d);
            session.unread = Some(observation("one", 2));
            session.activity = AgentActivity::Busy;
            session.name = "very-long-session-name-for-clipping".into();
            // No current agent snapshot after a legacy shell report; unread still survives.
            session.agent = None;
            if split {
                d.split_pane();
                d.focus_pane(0);
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
    selected(&mut d).unread = Some(observation("one", 2));
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
