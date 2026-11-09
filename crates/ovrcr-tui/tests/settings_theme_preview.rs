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
    // Menu -> Settings opens on the category column; Appearance holds Theme.
    for key in [KeyCode::Char(' '), KeyCode::Char('v'), KeyCode::Char(',')] {
        dashboard.key(key);
    }
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let text = visible_text(dashboard, 120, 40);
    assert!(
        text.lines()
            .any(|line| line.contains("› ") && line.contains("Saved"))
            && text.contains("Saved: "),
        "the Theme picker must open on the Server's saved palette:\n{text}"
    );
}

/// Settings owns the whole Dashboard view; its category column is the marker.
fn settings_open(dashboard: &Dashboard) -> bool {
    let text = visible_text(dashboard, 120, 40);
    text.contains("Remembered launches") && text.contains("Delegation")
}

fn theme_background(theme: ThemeId) -> Color {
    let mut reference = dashboard();
    reference.install_settings(Settings {
        theme,
        ..Settings::default()
    });
    background(&reference)
}

fn visible_rows(dashboard: &Dashboard, cols: u16, rows: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..rows)
        .map(|y| {
            (0..cols)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn visible_text(dashboard: &Dashboard, cols: u16, rows: u16) -> String {
    visible_rows(dashboard, cols, rows).join("\n")
}

/// The theme whose option line carries the picker's focus marker.
fn highlighted_theme(dashboard: &Dashboard) -> ThemeId {
    for line in visible_rows(dashboard, 120, 40) {
        for (pos, _) in line.match_indices("› ") {
            let rest = &line[pos + "› ".len()..];
            let matched = ThemeId::ALL
                .iter()
                .filter(|theme| rest.starts_with(theme.label()))
                .max_by_key(|theme| theme.label().len());
            if let Some(theme) = matched {
                return *theme;
            }
        }
    }
    panic!(
        "no highlighted theme:\n{}",
        visible_text(dashboard, 120, 40)
    );
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
        for (cols, rows) in [(120, 40), (80, 24)] {
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
    dashboard.key(KeyCode::Enter);
    let mut requests = dashboard.drain_outbox();
    assert_eq!(requests.len(), 1, "Enter sends one save");
    let ClientMessage {
        request: Request::SetSetting { path, value },
        ..
    } = requests.remove(0)
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
    assert_eq!(highlighted_theme(&dashboard), ThemeId::Dark);
    assert!(matches!(
        dashboard.key(KeyCode::PageDown),
        DashboardAction::Redraw
    ));
    let paged = highlighted_theme(&dashboard);
    assert_ne!(paged, ThemeId::Dark, "PageDown moves the highlight");
    assert_eq!(background(&dashboard), theme_background(paged));
    dashboard.key(KeyCode::PageUp);
    assert_eq!(background(&dashboard), saved);
    assert_eq!(highlighted_theme(&dashboard), ThemeId::Dark);

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
    assert!(settings_open(&dashboard));

    // The Theme row backs out to categories, then closes Settings with no
    // transient color left behind.
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), dark);
    assert!(!settings_open(&dashboard));

    // Reopening and returning with Ctrl-g follows the Settings "back" path.
    open_theme_picker(&mut dashboard);
    dashboard.key(KeyCode::Down);
    assert_ne!(background(&dashboard), dark);
    dashboard.key(KeyCode::Esc);
    dashboard.ctrl('g');
    assert_eq!(background(&dashboard), dark);
}

fn assert_preview(dashboard: &Dashboard, theme: ThemeId) {
    assert_eq!(
        highlighted_theme(dashboard),
        theme,
        "the picker must highlight {theme}:\n{}",
        visible_text(dashboard, 120, 40)
    );
    assert_eq!(background(dashboard), theme_background(theme), "{theme}");
}

fn save_theme(dashboard: &mut Dashboard, theme: ThemeId) -> u64 {
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Enter);
    let mut requests = dashboard.drain_outbox();
    assert_eq!(requests.len(), 1, "Enter sends one save");
    let ClientMessage {
        request_id,
        request: Request::SetSetting { path, value },
    } = requests.remove(0)
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
    dashboard.ctrl('g');
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
    assert!(!settings_open(&dashboard));
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
    assert!(!settings_open(&dashboard));
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
    let text = visible_text(&dashboard, 120, 40);
    assert!(text.contains("0 of 80 themes"), "{text}");
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
    assert!(matches!(
        dashboard.key(KeyCode::Enter),
        DashboardAction::Redraw
    ));
    assert!(visible_text(&dashboard, 120, 40).contains("0 of 80 themes"));
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

/// The saved and previewed palettes stay named together while browsing.
#[test]
fn saved_and_preview_theme_names_remain_visible_while_highlighting() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    let text = visible_text(&dashboard, 120, 40);
    assert!(text.contains("Saved: Catppuccin Mocha"), "{text}");
    assert!(!text.contains("Preview:"), "{text}");
    dashboard.key(KeyCode::Down);
    let preview = highlighted_theme(&dashboard);
    assert_ne!(preview, ThemeId::Dark);
    let text = visible_text(&dashboard, 120, 40);
    assert!(
        text.contains(&format!("Preview: {}", preview.label())),
        "{text}"
    );
    assert!(
        text.contains("Saved: Catppuccin Mocha"),
        "the saved palette stays visible:\n{text}"
    );
    assert_eq!(background(&dashboard), theme_background(preview));
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
}

/// Wide keeps the sidebar; 60 columns uses the picker breadcrumb. A mouse
/// wheel over the options moves the highlight and repaints without saving.
#[test]
fn theme_picker_presentation_at_80_and_60_and_mouse_preview() {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    let wide = visible_rows(&dashboard, 80, 24);
    assert!(
        wide.iter().any(|line| line.contains("Find a theme:")),
        "{wide:?}"
    );
    assert!(
        wide.iter().any(|line| line.contains("Appearance")),
        "the sidebar stays at 80 columns: {wide:?}"
    );
    let narrow = visible_rows(&dashboard, 60, 24);
    assert_eq!(
        narrow[0].trim(),
        "Settings / Appearance / Theme",
        "{narrow:?}"
    );
    assert!(
        narrow.iter().any(|line| line.contains("Find a theme:")),
        "{narrow:?}"
    );
    let option = narrow
        .iter()
        .position(|line| line.contains("› Catppuccin Mocha"))
        .expect("saved option");
    let x = narrow[option].find("› ").unwrap() as u16;
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: x,
                row: option as u16,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 60, 24),
        ),
        DashboardAction::Redraw
    );
    let preview = highlighted_theme(&dashboard);
    assert_ne!(preview, ThemeId::Dark);
    assert_eq!(background(&dashboard), theme_background(preview));
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
}

/// A wide picker click in the sidebar must not preview the theme option that
/// happens to share its row; clicking the option itself still previews.
#[test]
fn sidebar_click_aligned_with_a_theme_option_does_not_preview() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    let rows = visible_rows(&dashboard, 80, 24);
    let y = rows
        .iter()
        .position(|line| line.contains("Tokyo Night"))
        .expect("Tokyo Night option");
    assert!(
        rows[y].contains("Agents"),
        "the fixture aligns the Agents category with the option row:\n{}",
        rows.join("\n")
    );
    let column = |line: &str, needle: &str| {
        let byte = line.find(needle).unwrap();
        line[..byte].chars().count() as u16
    };
    let option_x = column(&rows[y], "Tokyo Night");
    let click = |dashboard: &mut Dashboard, x: u16| {
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y as u16,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 80, 24),
        )
    };

    let _ = visible_rows(&dashboard, 80, 24);
    assert_eq!(click(&mut dashboard, 2), DashboardAction::Redraw);
    assert_eq!(
        highlighted_theme(&dashboard),
        ThemeId::Dark,
        "a sidebar click must not preview an aligned option"
    );
    assert_eq!(background(&dashboard), theme_background(ThemeId::Dark));
    assert!(dashboard.drain_outbox().is_empty());

    let _ = visible_rows(&dashboard, 80, 24);
    click(&mut dashboard, option_x);
    assert_eq!(
        highlighted_theme(&dashboard),
        ThemeId::TokyoNight,
        "clicking the option itself previews it"
    );
    assert_eq!(
        background(&dashboard),
        theme_background(ThemeId::TokyoNight)
    );
    assert!(dashboard.drain_outbox().is_empty());
}

/// A picker whose option area has zero height reports the resize and cannot
/// submit; the same draft and highlight return once enough rows exist.
#[test]
fn zero_height_theme_options_block_saves_and_preserve_the_draft() {
    let mut dashboard = dashboard();
    open_theme_picker(&mut dashboard);
    dashboard.event_action(Event::Paste("tokyo".into()));
    assert_eq!(highlighted_theme(&dashboard), ThemeId::TokyoNight);

    for rows in [8u16, 9u16] {
        let text = visible_text(&dashboard, 60, rows).to_lowercase();
        assert!(
            text.contains("resize") || text.contains("larger terminal"),
            "a zero-height option area must explain the resize at 60x{rows}:\n{text}"
        );
        dashboard.key(KeyCode::Enter);
        assert!(
            dashboard.drain_outbox().is_empty(),
            "Enter must not save with no visible options at 60x{rows}"
        );
    }

    let text = visible_text(&dashboard, 60, 24);
    assert!(
        text.contains("Find a theme: tokyo"),
        "the filter draft survives:\n{text}"
    );
    assert!(
        text.contains("› Tokyo Night"),
        "the saved highlight survives:\n{text}"
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
        dashboard.ctrl('g');
        assert_eq!(background(&dashboard), theme_background(theme), "{theme}");
        assert!(!settings_open(&dashboard), "{theme}");
        assert!(dashboard.drain_outbox().is_empty());
        open_theme_picker(&mut dashboard);
        assert_preview(&dashboard, theme);
    }
}
