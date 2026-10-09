use crossterm::event::{Event, KeyCode};
use ovrcr_protocol::{
    ClientMessage, ErrorCode, Request, Response, ServerEvent, ServerMessage, SettingOwner,
    SettingRow, SettingSource, Settings, SettingsReport, TerminalSize, ThemeAppearance, ThemeId,
};
use ovrcr_tui::{Dashboard, DashboardAction, draw_dashboard_at};
use ratatui::{Terminal, backend::TestBackend, style::Color};

fn settings_changed(theme: ThemeId) -> ServerMessage {
    ServerMessage::Event(ServerEvent::SettingsChanged(Box::new(SettingsReport {
        path: "/fixture/dashboard.toml".into(),
        read_unix_ms: 0,
        settings: Settings {
            theme,
            ..Settings::default()
        },
        rows: vec![SettingRow {
            key: ThemeId::KEY.into(),
            owner: SettingOwner::Dashboard,
            value: Some(format!("\"{}\"", theme.as_str())),
            source: if theme == ThemeId::Dark {
                SettingSource::Default
            } else {
                SettingSource::Document
            },
            default: Some("\"catppuccin-mocha\"".into()),
            off_state: None,
        }],
        findings: Vec::new(),
        unparseable: false,
    })))
}

fn dashboard() -> Dashboard {
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.handle_server_message(settings_changed(ThemeId::Dark));
    dashboard
}

fn background(dashboard: &Dashboard) -> Color {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    terminal.backend().buffer()[(0, 1)].bg
}

fn open_theme_picker(dashboard: &mut Dashboard) {
    for key in [KeyCode::Char(' '), KeyCode::Char('v'), KeyCode::Char(',')] {
        dashboard.key(key);
    }
    for _ in ovrcr_protocol::entries() {
        if visible_text(dashboard, 120, 40).contains("key: theme") {
            dashboard.key(KeyCode::Enter);
            return;
        }
        dashboard.key(KeyCode::Down);
    }
    panic!("the Theme row must be reachable in Settings");
}

fn theme_background(theme: ThemeId) -> Color {
    let mut reference = dashboard();
    reference.install_settings(Settings {
        theme,
        ..Settings::default()
    });
    background(&reference)
}

fn visible_text(dashboard: &Dashboard, cols: u16, rows: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn all_80_highlighted_choices_repaint_without_sending_a_save() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    let mut choices = ThemeId::ALL.to_vec();
    choices.sort_by_key(|theme| match theme.appearance() {
        ThemeAppearance::Dark => 0,
        ThemeAppearance::Light => 1,
    });
    assert_eq!(choices.len(), 80);
    assert_eq!(choices[54], ThemeId::Light);
    for (index, theme) in choices.iter().copied().enumerate() {
        if index > 0 {
            assert!(matches!(
                dashboard.key(KeyCode::Down),
                DashboardAction::Redraw
            ));
        }
        assert_eq!(background(&dashboard), theme_background(theme), "{theme}");
        for (cols, rows) in [(120, 40), (80, 22)] {
            assert!(
                visible_text(&dashboard, cols, rows).contains(&format!("› {}", theme.label())),
                "highlighted choice must remain visible at {cols}x{rows}: {theme}"
            );
        }
        assert!(
            dashboard.drain_outbox().is_empty(),
            "preview must not send a save: {theme}"
        );
    }
    let last = *choices.last().unwrap();
    let DashboardAction::Request(ClientMessage {
        request: Request::SetSetting { path, value },
        ..
    }) = dashboard.key(KeyCode::Enter)
    else {
        panic!("Enter must send the save request")
    };
    assert_eq!(path, ThemeId::KEY);
    assert_eq!(value, Some(format!("\"{}\"", last.as_str())));
    assert_eq!(background(&dashboard), theme_background(last));
}

#[test]
fn paging_and_pasted_search_preview_through_public_event_dispatch() {
    let mut dashboard = dashboard();
    let saved = background(&dashboard);
    open_theme_picker(&mut dashboard);
    assert!(matches!(
        dashboard.key(KeyCode::PageDown),
        DashboardAction::Redraw
    ));
    assert_eq!(
        background(&dashboard),
        theme_background(ThemeId::CatppuccinMacchiato)
    );
    dashboard.key(KeyCode::PageUp);
    assert_eq!(background(&dashboard), saved);

    dashboard.event_action(Event::Paste("Solarized Light".into()));
    assert_eq!(
        background(&dashboard),
        theme_background(ThemeId::SolarizedLight)
    );
    dashboard.event_action(Event::Paste("no-such-theme".into()));
    assert_eq!(background(&dashboard), saved);
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), saved);
}

#[test]
fn settings_theme_preview_repaints_the_dashboard_then_restores_on_cancel() {
    let mut dashboard = dashboard();
    let dark = background(&dashboard);

    // Open Settings through its normal Dashboard key path.
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);

    let preview = background(&dashboard);
    assert_ne!(
        preview, dark,
        "highlighting another theme repaints the frame"
    );

    // Esc cancels only the picker; its palette returns to the saved theme.
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), dark);

    // Esc again closes Settings, with no transient color left behind.
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), dark);

    // Reopening and returning with Ctrl-g follows the Settings "back" path.
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);
    assert_ne!(background(&dashboard), dark);
    dashboard.key(KeyCode::Esc);
    dashboard.ctrl('g');
    assert_eq!(background(&dashboard), dark);
}

fn assert_preview(dashboard: &Dashboard, theme: ThemeId) {
    assert!(
        visible_text(dashboard, 120, 40).contains(&format!("› {}", theme.label())),
        "the picker must highlight {theme}"
    );
    assert_eq!(background(dashboard), theme_background(theme), "{theme}");
}

fn save_theme(dashboard: &mut Dashboard, theme: ThemeId) -> u64 {
    assert!(dashboard.drain_outbox().is_empty());
    let DashboardAction::Request(ClientMessage {
        request_id,
        request: Request::SetSetting { path, value },
    }) = dashboard.key(KeyCode::Enter)
    else {
        panic!("Enter must save {theme}");
    };
    assert_eq!(path, ThemeId::KEY);
    assert_eq!(value, Some(format!("\"{}\"", theme.as_str())));
    assert!(
        dashboard.drain_outbox().is_empty(),
        "save must not be duplicated"
    );
    request_id
}

#[test]
fn older_save_refusal_preserves_a_newer_highlighted_preview() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);
    let older_request = save_theme(&mut dashboard, ThemeId::TokyoNight);

    // Reopen before the earlier request completes and browse a different theme.
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Down);
    assert_preview(&dashboard, ThemeId::Dracula);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: older_request,
        response: Response::Error {
            code: ErrorCode::InvalidRequest,
            message: "fixture older theme save refused".into(),
        },
    });
    assert_preview(&dashboard, ThemeId::Dracula);
    assert!(visible_text(&dashboard, 120, 40).contains("fixture older theme save refused"));
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
}

#[test]
fn external_update_preserves_a_highlight_that_matched_the_saved_theme() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Up);
    assert_preview(&dashboard, ThemeId::Dark);

    dashboard.handle_server_message(settings_changed(ThemeId::Light));
    assert_preview(&dashboard, ThemeId::Dark);
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), theme_background(ThemeId::Light));
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn reopening_before_confirmation_previews_the_new_picker_selection() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);
    let request_id = save_theme(&mut dashboard, ThemeId::TokyoNight);
    assert_eq!(
        background(&dashboard),
        theme_background(ThemeId::TokyoNight)
    );

    dashboard.key(KeyCode::Enter);
    assert_preview(&dashboard, ThemeId::Dark);
    // The report belongs to the prior save, not the newly opened draft.
    dashboard.handle_server_message(settings_changed(ThemeId::TokyoNight));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Ok,
    });
    assert_preview(&dashboard, ThemeId::Dark);
    dashboard.key(KeyCode::Esc);
    assert_eq!(
        background(&dashboard),
        theme_background(ThemeId::TokyoNight)
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn newer_save_of_original_theme_survives_the_older_save_report() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);
    let older_request = save_theme(&mut dashboard, ThemeId::TokyoNight);

    // Save the original choice again before the first save is published.
    dashboard.key(KeyCode::Enter);
    assert_preview(&dashboard, ThemeId::Dark);
    let newer_request = save_theme(&mut dashboard, ThemeId::Dark);
    assert_ne!(older_request, newer_request);
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));

    dashboard.handle_server_message(settings_changed(ThemeId::TokyoNight));
    assert_eq!(
        background(&dashboard),
        theme_background(ThemeId::Dark),
        "the older save report must not replace the latest submitted preview"
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: older_request,
        response: Response::Ok,
    });
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
    assert!(dashboard.drain_outbox().is_empty());

    dashboard.handle_server_message(settings_changed(ThemeId::Dark));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: newer_request,
        response: Response::Ok,
    });
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
    assert!(!visible_text(&dashboard, 120, 40).contains("Settings · Enter edit"));
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn ctrl_g_from_the_active_picker_closes_settings_and_restores_latest_saved_theme() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);
    assert_preview(&dashboard, ThemeId::TokyoNight);
    dashboard.handle_server_message(settings_changed(ThemeId::Light));
    assert_preview(&dashboard, ThemeId::TokyoNight);

    assert!(matches!(dashboard.ctrl('g'), DashboardAction::Redraw));
    assert_eq!(background(&dashboard), theme_background(ThemeId::Light));
    assert!(!visible_text(&dashboard, 120, 40).contains("Settings · Enter edit"));
    assert!(dashboard.drain_outbox().is_empty());
    open_theme_picker(&mut dashboard);
    assert_preview(&dashboard, ThemeId::Light);
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), theme_background(ThemeId::Light));
}

#[test]
fn typed_search_recovers_from_no_matches_without_saving_until_enter() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    for ch in "Solarized Light".chars() {
        assert!(matches!(
            dashboard.key(KeyCode::Char(ch)),
            DashboardAction::Redraw
        ));
        assert!(dashboard.drain_outbox().is_empty());
    }
    assert_preview(&dashboard, ThemeId::SolarizedLight);
    dashboard.key(KeyCode::Char('!'));
    assert!(visible_text(&dashboard, 120, 40).contains("No matches"));
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
    assert!(matches!(
        dashboard.key(KeyCode::Enter),
        DashboardAction::Redraw
    ));
    assert!(visible_text(&dashboard, 120, 40).contains("No matches"));
    assert!(dashboard.drain_outbox().is_empty());

    dashboard.key(KeyCode::Backspace);
    assert_preview(&dashboard, ThemeId::SolarizedLight);
    let request_id = save_theme(&mut dashboard, ThemeId::SolarizedLight);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Ok,
    });
    dashboard.handle_server_message(settings_changed(ThemeId::SolarizedLight));
    dashboard.key(KeyCode::Esc);
    assert_eq!(
        background(&dashboard),
        theme_background(ThemeId::SolarizedLight)
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn enter_sends_the_canonical_id_for_every_grouped_choice() {
    let mut choices = ThemeId::ALL.to_vec();
    choices.sort_by_key(|theme| match theme.appearance() {
        ThemeAppearance::Dark => 0,
        ThemeAppearance::Light => 1,
    });
    assert_eq!(choices.len(), 80);
    for (index, theme) in choices.into_iter().enumerate() {
        let mut dashboard = dashboard();
        open_theme_picker(&mut dashboard);
        for _ in 0..index {
            assert!(matches!(
                dashboard.key(KeyCode::Down),
                DashboardAction::Redraw
            ));
        }
        assert_preview(&dashboard, theme);
        let request_id = save_theme(&mut dashboard, theme);
        // The published report is authoritative even if the response arrives later.
        dashboard.handle_server_message(settings_changed(theme));
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Ok,
        });
        dashboard.key(KeyCode::Esc);
        assert_eq!(background(&dashboard), theme_background(theme), "{theme}");
        assert!(dashboard.drain_outbox().is_empty());
        open_theme_picker(&mut dashboard);
        assert_preview(&dashboard, theme);
    }
}
