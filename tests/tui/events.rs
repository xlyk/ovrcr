//! The Events popup: word filter, component filter, and live append.

use crate::*;
use crossterm::event::KeyCode;
use ovrcr::protocol::{Event as Recorded, EventComponent, Request, Response};

fn screen(dashboard: &Dashboard) -> String {
    rendered_rows(dashboard, 100, 40).join("\n")
}

fn event(component: EventComponent, subject: Option<&str>, message: &str) -> Recorded {
    Recorded {
        time_unix_ms: 1_700_000_000_000,
        component,
        subject: subject.map(str::to_string),
        message: message.into(),
    }
}

fn deliver(dashboard: &mut Dashboard, action: DashboardAction, events: Vec<Recorded>) {
    let DashboardAction::Request(message) = action else {
        panic!("expected an Events request, got {action:?}");
    };
    assert!(
        matches!(message.request, Request::Events { follow: false }),
        "the popup reads the ring, not a follow connection: {:?}",
        message.request
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Events(events),
    });
}

fn recorded(dashboard: &mut Dashboard, event: Recorded) -> Vec<ovrcr::protocol::ClientMessage> {
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Recorded(event)))
}

fn open_from_menu(dashboard: &mut Dashboard) -> DashboardAction {
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('v'));
    assert!(
        screen(dashboard).contains("e  Events"),
        "View menu lists Events:
{}",
        screen(dashboard)
    );
    dashboard.key(KeyCode::Char('e'))
}

fn open_from_palette(dashboard: &mut Dashboard) -> DashboardAction {
    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste("events".into()));
    dashboard.key(KeyCode::Enter)
}

fn open(dashboard: &Dashboard) -> bool {
    screen(dashboard).contains("Events · / filter")
}

#[test]
fn slash_word_filter_keeps_events_that_contain_every_word() {
    let mut dashboard = dashboard_fixture();
    let action = open_from_palette(&mut dashboard);
    assert!(screen(&dashboard).contains("Waiting for the Server's events."));
    deliver(
        &mut dashboard,
        action,
        vec![
            event(EventComponent::Titles, Some("12"), "call sent"),
            event(EventComponent::Titles, Some("12"), "call failed"),
            event(EventComponent::Quota, Some("codex"), "refresh finished"),
        ],
    );
    let text = screen(&dashboard);
    let sent = text.find("call sent").expect(&text);
    let failed = text.find("call failed").expect(&text);
    let refresh = text.find("refresh finished").expect(&text);
    assert!(sent < failed && failed < refresh, "newest last:\n{text}");

    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("CALL sent".into()));
    let text = screen(&dashboard);
    assert!(text.contains("Filter: CALL sent"), "{text}");
    assert!(text.contains("call sent"), "{text}");
    assert!(!text.contains("call failed"), "{text}");
    assert!(!text.contains("refresh finished"), "{text}");

    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("codex refresh".into()));
    dashboard.key(KeyCode::Enter);
    let text = screen(&dashboard);
    assert!(text.contains("refresh finished"), "{text}");
    assert!(!text.contains("call sent"), "{text}");
    // Escape clears the filter before it closes the popup.
    dashboard.key(KeyCode::Esc);
    assert!(screen(&dashboard).contains("call sent"));
    dashboard.key(KeyCode::Esc);
    assert!(!open(&dashboard));
}

#[test]
fn tab_cycles_the_component_filter() {
    let mut dashboard = dashboard_fixture();
    let action = open_from_menu(&mut dashboard);
    deliver(
        &mut dashboard,
        action,
        vec![
            event(EventComponent::Titles, None, "title applied"),
            event(EventComponent::Settings, None, "settings reloaded"),
            event(EventComponent::Quota, None, "quota refreshed"),
        ],
    );
    assert!(screen(&dashboard).contains("Component: all"));
    dashboard.key(KeyCode::Tab);
    let text = screen(&dashboard);
    assert!(text.contains("Component: titles"), "{text}");
    assert!(text.contains("title applied"), "{text}");
    assert!(!text.contains("settings reloaded"), "{text}");
    assert!(!text.contains("quota refreshed"), "{text}");

    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Tab);
    let text = screen(&dashboard);
    assert!(text.contains("Component: quota"), "{text}");
    assert!(text.contains("quota refreshed"), "{text}");
    assert!(!text.contains("title applied"), "{text}");

    // The word filter applies on top of the component.
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("missing".into()));
    let text = screen(&dashboard);
    assert!(text.contains("No events match."), "{text}");
    assert!(text.contains("Component: quota"), "{text}");

    dashboard.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    // Still typing the word filter, so the component does not change.
    assert!(screen(&dashboard).contains("Component: quota"));
    dashboard.key(KeyCode::Enter);
    dashboard.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    let text = screen(&dashboard);
    assert!(text.contains("Component: settings"), "{text}");
    assert!(text.contains("No events match."), "{text}");
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::BackTab);
    let text = screen(&dashboard);
    assert!(text.contains("Component: titles"), "{text}");
    assert!(text.contains("title applied"), "{text}");
    assert!(!text.contains("quota refreshed"), "{text}");
}

#[test]
fn recorded_events_append_while_the_popup_is_open() {
    let mut dashboard = dashboard_fixture();
    recorded(
        &mut dashboard,
        event(EventComponent::Titles, None, "before open"),
    );
    let action = open_from_menu(&mut dashboard);
    deliver(
        &mut dashboard,
        action,
        vec![event(EventComponent::Titles, None, "already recorded")],
    );
    let text = screen(&dashboard);
    assert!(text.contains("already recorded"), "{text}");
    assert!(
        !text.contains("before open"),
        "a line recorded while closed is not kept ahead of the snapshot:\n{text}"
    );

    let extra = recorded(
        &mut dashboard,
        event(EventComponent::Quota, Some("grok"), "while open"),
    );
    assert!(
        extra
            .iter()
            .all(|message| !matches!(message.request, Request::Events { .. })),
        "live append does not ask for the ring again: {extra:?}"
    );
    let text = screen(&dashboard);
    let older = text.find("already recorded").expect(&text);
    let newer = text.find("while open").expect(&text);
    assert!(older < newer, "appended after the snapshot:\n{text}");
    assert!(text.contains("grok"), "{text}");

    dashboard.key(KeyCode::Esc);
    assert!(!open(&dashboard));
    recorded(
        &mut dashboard,
        event(EventComponent::Settings, None, "after close"),
    );
    let action = open_from_palette(&mut dashboard);
    deliver(
        &mut dashboard,
        action,
        vec![event(EventComponent::Titles, None, "already recorded")],
    );
    let text = screen(&dashboard);
    assert!(!text.contains("while open"), "{text}");
    assert!(!text.contains("after close"), "{text}");
    assert!(text.contains("already recorded"), "{text}");
}

#[test]
fn events_open_on_the_newest_line_and_home_shows_the_oldest() {
    let mut dashboard = dashboard_fixture();
    let events = (0..40)
        .map(|index| event(EventComponent::Settings, None, &format!("ev-{index:02}")))
        .collect();
    let action = open_from_menu(&mut dashboard);
    deliver(&mut dashboard, action, events);
    let text = screen(&dashboard);
    assert!(text.contains("ev-39"), "{text}");
    assert!(!text.contains("ev-00"), "opened on the newest:\n{text}");
    dashboard.key(KeyCode::Home);
    let text = screen(&dashboard);
    assert!(text.contains("ev-00"), "{text}");
    assert!(!text.contains("ev-39"), "home shows the oldest:\n{text}");
    dashboard.key(KeyCode::End);
    let text = screen(&dashboard);
    assert!(text.contains("ev-39"), "{text}");
    assert!(!text.contains("ev-00"), "{text}");
}
