use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ovrcr::protocol::{
    ClientMessage, HierarchySnapshot, OperatorContext, OperatorConversationId,
    OperatorConversationSummary, OperatorHistory, OperatorMessage, OperatorMessageContent,
    OperatorRequest, OperatorResponse, OperatorTurnId, Request, Response, ServerMessage,
    SessionRunId,
};
use ovrcr::session::{SessionId, TerminalSize};
use ovrcr::tui::Dashboard;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::ffi::OsString;

const AREA: Rect = Rect::new(0, 0, 120, 40);

struct PiExecutableGuard {
    previous: Option<OsString>,
    missing: std::path::PathBuf,
}

impl PiExecutableGuard {
    fn missing() -> Self {
        let previous = std::env::var_os("OVRCR_PI_EXECUTABLE");
        let missing = std::env::temp_dir().join(format!(
            "ovrcr-review-missing-pi-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        assert!(
            !missing.exists(),
            "unique missing executable path: {missing:?}"
        );
        // This test is in its own single-test integration binary, so changing the
        // process environment cannot race other TUI tests.
        unsafe { std::env::set_var("OVRCR_PI_EXECUTABLE", &missing) };
        Self { previous, missing }
    }
}

impl Drop for PiExecutableGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => unsafe { std::env::set_var("OVRCR_PI_EXECUTABLE", previous) },
            None => unsafe { std::env::remove_var("OVRCR_PI_EXECUTABLE") },
        }
    }
}

fn new_dashboard() -> Dashboard {
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: AREA.height,
        cols: AREA.width,
    });
    dashboard.install_area(AREA);
    dashboard.install_hierarchy(HierarchySnapshot {
        usage: Default::default(),
        projects: Vec::new(),
    });
    let mut settings = ovrcr::tui::Settings::default();
    settings.operator.sidebar = true;
    dashboard.install_settings(settings);
    dashboard
}

fn conversation() -> OperatorConversationSummary {
    OperatorConversationSummary {
        id: OperatorConversationId(7),
        session: SessionId(99),
        run: SessionRunId(1),
        model: None,
        title: "Operator".into(),
        current_turn: None,
        reasoning: None,
        current_turn_started_unix_ms: None,
        last_reply_unix_ms: None,
        worker_stopped: true,
        worker_live: false,
    }
}

fn screen(dashboard: &mut Dashboard) -> String {
    let mut terminal = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    dashboard.draw(&mut terminal).unwrap();
    (0..AREA.height)
        .map(|row| {
            (0..AREA.width)
                .map(|column| {
                    terminal
                        .backend()
                        .buffer()
                        .cell((column, row))
                        .unwrap()
                        .symbol()
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn open_message(dashboard: &mut Dashboard) -> ClientMessage {
    dashboard
        .drain_outbox()
        .into_iter()
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("opening Operator submits the singleton Open request")
}

fn settle_empty_open(dashboard: &mut Dashboard, open: ClientMessage) {
    let summary = conversation();
    let opened = dashboard.handle_server_message(ServerMessage::Response {
        request_id: open.request_id,
        response: Response::Operator(OperatorResponse::Opened(summary.clone())),
    });
    let read = opened
        .into_iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Read { .. })
            )
        })
        .expect("opening the existing singleton reads its history");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: read.request_id,
        response: Response::Operator(OperatorResponse::History(OperatorHistory {
            conversation: summary,
            messages: Vec::new(),
            next_after: None,
        })),
    });
}

fn settle_history_open(dashboard: &mut Dashboard, open: ClientMessage) {
    let summary = conversation();
    let opened = dashboard.handle_server_message(ServerMessage::Response {
        request_id: open.request_id,
        response: Response::Operator(OperatorResponse::Opened(summary.clone())),
    });
    let read = opened
        .into_iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Read { .. })
            )
        })
        .expect("opening the singleton reads its history");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: read.request_id,
        response: Response::Operator(OperatorResponse::History(OperatorHistory {
            conversation: summary,
            messages: vec![OperatorMessage {
                ordinal: 1,
                turn: OperatorTurnId(1),
                content: OperatorMessageContent::User {
                    text: "saved journal entry".into(),
                    context: OperatorContext::default(),
                },
            }],
            next_after: None,
        })),
    });
}

#[test]
fn missing_pi_remains_not_set_up_after_sidebar_and_hidden_conversation_opens() {
    let pi = PiExecutableGuard::missing();

    unsafe { std::env::set_var("OVRCR_PI_EXECUTABLE", "/bin/sh") };
    let mut first_open = new_dashboard();
    let before = screen(&mut first_open);
    assert!(before.contains("Off"), "before first open: {before}");
    first_open.key(KeyCode::Char('o'));
    let pending = screen(&mut first_open);
    assert!(
        pending.contains("Idle"),
        "queued Open is visible as Idle: {pending}"
    );
    assert!(
        !pending.contains("Off"),
        "queued Open is no longer Off: {pending}"
    );
    let open = open_message(&mut first_open);
    settle_empty_open(&mut first_open, open);
    let opened = screen(&mut first_open);
    assert!(opened.contains("Idle"), "successful first Open: {opened}");
    assert!(
        !opened.contains("Off"),
        "successful Open remains visible: {opened}"
    );

    unsafe { std::env::set_var("OVRCR_PI_EXECUTABLE", &pi.missing) };

    // The shared Open path records setup state even before the first draw and
    // while the sidebar is disabled; revealing it later keeps the reason.
    let mut hidden = new_dashboard();
    let mut settings = ovrcr::tui::Settings::default();
    settings.operator.sidebar = false;
    hidden.install_settings(settings.clone());
    hidden.key(KeyCode::Char('o'));
    let open = open_message(&mut hidden);
    settle_empty_open(&mut hidden, open);
    settings.operator.sidebar = true;
    hidden.install_settings(settings);
    let revealed = screen(&mut hidden);
    assert!(revealed.contains("Not set up"), "{revealed}");
    assert!(revealed.contains("run ovrcr operator setup"), "{revealed}");

    let mut dashboard = new_dashboard();
    let before = screen(&mut dashboard);
    assert!(before.contains("Not set up"), "{before}");

    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            AREA,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    let open = open_message(&mut dashboard);
    settle_history_open(&mut dashboard, open);

    let after = screen(&mut dashboard);
    assert!(after.contains("Not set up"), "{after}");
    assert!(after.contains("run ovrcr operator setup"), "{after}");
    assert!(
        after.contains("saved journal entry"),
        "history remains visible: {after}"
    );

    unsafe { std::env::set_var("OVRCR_PI_EXECUTABLE", "/bin/sh") };
    let recovered = screen(&mut dashboard);
    assert!(recovered.contains("Idle"), "{recovered}");
    assert!(!recovered.contains("Not set up"), "{recovered}");

    // Reopening while Pi is available clears the latch; a previously loaded
    // chat is not reclassified merely because the executable later disappears.
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char('o'));
    let _retry = open_message(&mut dashboard);
    unsafe { std::env::set_var("OVRCR_PI_EXECUTABLE", &pi.missing) };
    let loaded = screen(&mut dashboard);
    assert!(loaded.contains("Idle"), "{loaded}");
    assert!(!loaded.contains("Not set up"), "{loaded}");
}
