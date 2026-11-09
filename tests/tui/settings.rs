//! The Server's settings reading: what the Dashboard adopts, the footer line,
//! and the Settings editor. Readings come from the real loader and edits go
//! through the real writer, exactly as the Server answers `SetSetting`.

use crate::*;
use ovrcr::protocol::{ErrorCode, Request, Response, SettingsReport};
use ovrcr::tui::ConnectStatus;
use std::path::Path;

#[cfg(target_os = "macos")]
use crate::live;

/// The Server's reading of `document`, as published after hello or an edit.
fn publish(dashboard: &mut Dashboard, document: &Path) {
    let text = std::fs::read_to_string(document).unwrap_or_default();
    publish_report(dashboard, ovrcr::settings::parse(document, &text));
}

fn publish_report(dashboard: &mut Dashboard, report: SettingsReport) {
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
        Box::new(report),
    )));
}

/// One `SetSetting` request, answered as the Server answers it: the writer
/// applies it, the new reading is published, then `Ok`; or the writer's
/// refusal comes back as the error, and nothing is published.
fn serve(dashboard: &mut Dashboard, document: &Path, key: KeyCode) -> (String, Option<String>) {
    let action = dashboard.key(key);
    assert_eq!(action, DashboardAction::Redraw);
    let message = only_request(dashboard.drain_outbox());
    answer(dashboard, document, message)
}

fn answer(
    dashboard: &mut Dashboard,
    document: &Path,
    message: ClientMessage,
) -> (String, Option<String>) {
    let Request::SetSetting { path, value } = message.request else {
        panic!("expected SetSetting, got {:?}", message.request);
    };
    let response = match ovrcr::settings::set(document, &path, value.as_deref()) {
        Ok(()) => {
            publish(dashboard, document);
            Response::Ok
        }
        Err(error) => Response::Error {
            code: ErrorCode::InvalidRequest,
            message: format!("{error:#}"),
        },
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response,
    });
    (path, value)
}

fn screen(dashboard: &Dashboard) -> String {
    rendered_rows(dashboard, 120, 60).join("\n")
}

/// Join the main pane's wrapped text into one whitespace-normalized line.
/// The wide sidebar and its `│` divider are stripped per row so a word wrapped
/// across rows stays adjacent instead of acquiring sidebar glyphs.
fn flatten_frame(rows: Vec<String>) -> String {
    rows.into_iter()
        .map(|line| match line.chars().position(|ch| ch == '│') {
            Some(divider) => line.chars().skip(divider + 1).collect::<String>(),
            None => line,
        })
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn count_label(entries: usize) -> String {
    match entries {
        1 => "1 entry".into(),
        n => format!("{n} entries"),
    }
}

/// The display-cell column of `needle`, not its UTF-8 byte offset.
fn cell_index(line: &str, needle: &str) -> usize {
    let byte = line
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in {line:?}"));
    line[..byte].chars().count()
}

/// The first cell holding `needle` in a rendered frame.
fn cell_of(rows: &[String], needle: &str) -> (u16, u16) {
    for (y, line) in rows.iter().enumerate() {
        if line.contains(needle) {
            return (cell_index(line, needle) as u16, y as u16);
        }
    }
    panic!("no cell with {needle:?}:\n{}", rows.join("\n"));
}

/// A Dashboard attached to a Server reading `contents`, with Settings open
/// from the palette.
fn editor(contents: &str) -> (Dashboard, tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, contents).unwrap();
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, &document);
    dashboard.key(KeyCode::Char(':'));
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    dashboard.event_action(Event::Paste("settings".into()));
    dashboard.key(KeyCode::Enter);
    (dashboard, root, document)
}

/// Open global Settings search, type `query`, and open its first match in the
/// owning category's list. The match is focused, not edited.
fn select(dashboard: &mut Dashboard, query: &str) {
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste(query.into()));
    dashboard.key(KeyCode::Enter);
}

/// Move the category column's focus to `category` from category focus.
fn focus_category(dashboard: &mut Dashboard, category: &str) {
    let markers = [format!("› {category}"), format!("• {category}")];
    let steps = ovrcr::protocol::entries().len();
    for key in [KeyCode::Down, KeyCode::Up] {
        for _ in 0..steps {
            if rendered_rows(dashboard, 120, 60)
                .into_iter()
                .any(|line| markers.iter().any(|marker| line.contains(marker)))
            {
                return;
            }
            dashboard.key(key);
        }
    }
    panic!("no category {category:?}:\n{}", screen(dashboard));
}

/// Move list focus to the row whose marker is followed by `label`.
fn focus_row(dashboard: &mut Dashboard, label: &str) {
    let marker = format!("› {label}");
    let focused = |dashboard: &Dashboard| {
        rendered_rows(dashboard, 120, 60)
            .into_iter()
            .any(|line| line.contains(&marker))
    };
    let steps = ovrcr::protocol::entries().len() + 8;
    for _ in 0..steps {
        if focused(dashboard) {
            return;
        }
        dashboard.key(KeyCode::Down);
    }
    for _ in 0..steps {
        if focused(dashboard) {
            return;
        }
        dashboard.key(KeyCode::Up);
    }
    panic!("no focused row {label:?}:\n{}", screen(dashboard));
}

/// Settings owns the view: its whole catalog's category column is visible.
fn open(dashboard: &Dashboard) -> bool {
    let text = screen(dashboard);
    [
        "Alerts",
        "Appearance",
        "Workspaces",
        "Remembered launches",
        "Remote",
    ]
    .iter()
    .all(|category| text.contains(category))
}

/// Open Settings through the Browse menu: Space shows the menu, `v` shows the
/// view actions and `,` runs Settings. The palette route is separate.
fn open_from_menu(dashboard: &mut Dashboard) {
    assert_eq!(dashboard.key(KeyCode::Char(' ')), DashboardAction::Redraw);
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Char(','));
    assert!(open(dashboard), "{}", screen(dashboard));
}

/// A Dashboard attached to an empty document with Settings open from the menu.
fn menu_editor() -> (Dashboard, tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, "").unwrap();
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, &document);
    open_from_menu(&mut dashboard);
    (dashboard, root, document)
}

/// The focused category line. On wide screens the category column is the row
/// prefix; at 60 columns the marker also sits at the line start.
fn focused_row(dashboard: &Dashboard) -> String {
    rendered_rows(dashboard, 120, 60)
        .into_iter()
        .find(|line| line.contains("› "))
        .unwrap_or_else(|| panic!("no focused Settings row:\n{}", screen(dashboard)))
}

/// The focus-marked line whose label is `needle`. Wide list focus marks both
/// the category (mid-line, after the sidebar) and the selected row.
fn focused_line_with(dashboard: &Dashboard, needle: &str) -> String {
    let marker = format!("› {needle}");
    rendered_rows(dashboard, 120, 60)
        .into_iter()
        .find(|line| line.contains(&marker))
        .unwrap_or_else(|| panic!("no focused line with {needle:?}:\n{}", screen(dashboard)))
}

/// The manager row that visibly owns the focus marker. A details fallback
/// alone is not focus; this panics when no rendered manager row is marked.
fn manager_focus(dashboard: &Dashboard) -> String {
    let rows = rendered_rows(dashboard, 120, 60);
    let divider = rows
        .iter()
        .find_map(|line| line.chars().position(|ch| ch == '│'))
        .expect("pane divider");
    rows.into_iter()
        .find_map(|line| {
            let pane: String = line.chars().skip(divider + 1).collect();
            pane.strip_prefix("› ").map(|row| row.trim().to_string())
        })
        .unwrap_or_else(|| panic!("no visibly focused manager row:\n{}", screen(dashboard)))
}

/// Category focus opens on Alerts without rendering another category's rows.
/// Down moves the active category to Appearance, Enter enters its list, Enter
/// opens the Theme picker without writing, and Esc unwinds one level at a time
/// back to the Dashboard's session.
#[test]
fn settings_navigation_enters_appearance_theme_and_unwinds_to_the_workspace() {
    let (mut dashboard, _root, _document) = menu_editor();
    assert!(
        focused_row(&dashboard).contains("Alerts"),
        "{}",
        screen(&dashboard)
    );
    for category in [
        "Alerts",
        "Appearance",
        "Workspaces",
        "Titles",
        "Usage",
        "Agents",
        "Remembered launches",
        "Operator",
        "Delegation",
        "Remote",
    ] {
        assert!(
            screen(&dashboard).contains(category),
            "the category column must list {category}:\n{}",
            screen(&dashboard)
        );
    }
    assert!(
        !screen(&dashboard).contains("Theme"),
        "another category's rows must not show while Alerts is active:\n{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());

    // Down moves category focus only; Appearance's own rows appear beside it.
    assert_eq!(dashboard.key(KeyCode::Down), DashboardAction::Redraw);
    assert!(
        focused_row(&dashboard).contains("Appearance"),
        "{}",
        screen(&dashboard)
    );
    assert!(
        screen(&dashboard).contains("Theme"),
        "Appearance's rows must be readable while it is active:\n{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());

    // Enter enters the list: the category and its selected row are both marked.
    dashboard.key(KeyCode::Enter);
    assert!(
        focused_line_with(&dashboard, "Theme").contains("Theme"),
        "{}",
        screen(&dashboard)
    );
    assert!(
        screen(&dashboard).contains("• Appearance"),
        "the sidebar marks the open category while the row carries focus:\n{}",
        screen(&dashboard)
    );

    // Enter opens the dedicated picker on the saved palette, sending no write.
    dashboard.key(KeyCode::Enter);
    let text = screen(&dashboard);
    assert!(text.contains("Dark"), "{text}");
    assert!(
        focused_line_with(&dashboard, "Catppuccin Mocha").contains("Saved"),
        "the saved palette opens highlighted and labelled:\n{text}"
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "opening the theme picker must not write"
    );

    // Esc unwinds picker -> list -> categories.
    dashboard.key(KeyCode::Esc);
    assert!(
        focused_line_with(&dashboard, "Theme").contains("Theme"),
        "{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Esc);
    assert!(
        focused_row(&dashboard).contains("Appearance"),
        "{}",
        screen(&dashboard)
    );
    assert!(
        !screen(&dashboard).contains("› Theme"),
        "no row marker remains in category focus:\n{}",
        screen(&dashboard)
    );

    // Global search names its owning category and opens the match in its list
    // without editing it; Enter in the list then opens the editor.
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("branch prefix".into()));
    let text = screen(&dashboard);
    assert!(
        text.contains("Workspaces") && text.contains("Branch prefix"),
        "search must identify the owning category and row:\n{text}"
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "typing a search query must not write"
    );
    dashboard.key(KeyCode::Enter);
    assert!(
        focused_line_with(&dashboard, "Branch prefix").contains("Branch prefix"),
        "{}",
        screen(&dashboard)
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "opening a search result must not edit or write"
    );
    dashboard.key(KeyCode::Enter);
    assert!(
        rendered_footer(&dashboard, 120).contains("cancel"),
        "the footer names the editor's cancel action:\n{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    assert!(
        focused_line_with(&dashboard, "Branch prefix").contains("Branch prefix"),
        "Esc cancels only the draft:\n{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());

    // Esc unwinds the search-opened list back to the category it came from,
    // then closes Settings and restores the workspace selection.
    dashboard.key(KeyCode::Esc);
    assert!(
        focused_row(&dashboard).contains("Appearance"),
        "backing out of search restores the prior category:\n{}",
        screen(&dashboard)
    );
    assert_eq!(
        dashboard.key(KeyCode::Esc),
        DashboardAction::Redraw,
        "{}",
        screen(&dashboard)
    );
    assert!(
        !open(&dashboard),
        "Settings must close:\n{}",
        screen(&dashboard)
    );
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(1)),
        "closing Settings returns to the Dashboard selection it came from"
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn settings_footer_reports_findings_at_attach_and_when_the_count_changes() {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    let findings = |count: usize| {
        (0..count)
            .map(|index| format!("unknown_{index} = 1\n"))
            .collect::<String>()
    };
    let mut quiet = dashboard_fixture();
    publish(&mut quiet, &document);
    assert!(!rendered_footer(&quiet, 120).contains("settings finding"));

    let mut dashboard = dashboard_fixture();
    std::fs::write(&document, findings(2)).unwrap();
    publish(&mut dashboard, &document);
    assert!(
        rendered_footer(&dashboard, 120).contains("2 settings findings; see Settings"),
        "{}",
        rendered_footer(&dashboard, 120)
    );
    // Any key clears the notice; the same count does not bring it back.
    dashboard.key(KeyCode::Char('j'));
    publish(&mut dashboard, &document);
    assert!(!rendered_footer(&dashboard, 120).contains("settings finding"));
    std::fs::write(&document, findings(1)).unwrap();
    publish(&mut dashboard, &document);
    assert!(rendered_footer(&dashboard, 120).contains("1 settings finding; see Settings"));
    std::fs::write(&document, findings(0)).unwrap();
    publish(&mut dashboard, &document);
    assert!(rendered_footer(&dashboard, 120).contains("No settings findings"));
}

/// An error banner raised after hello's settings reading takes the footer. The
/// findings line must wait behind that banner, including across a key press, and
/// show once the banner is dismissed.
#[test]
fn startup_error_banner_does_not_hide_the_settings_findings_notice() {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, "unknown_0 = 1\nunknown_1 = 1\n").unwrap();
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, &document);
    assert!(
        rendered_footer(&dashboard, 120).contains("2 settings findings; see Settings"),
        "{}",
        rendered_footer(&dashboard, 120)
    );

    // An error the Server returns writes the banner through `set_error`.
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9_000,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "startup warning".into(),
        },
    });
    let banner = rendered_footer(&dashboard, 120);
    assert!(
        banner.starts_with("ERROR:"),
        "startup warning must own the footer, got {banner:?}"
    );
    assert!(
        !banner.contains("settings finding"),
        "the banner covers the findings line, got {banner:?}"
    );

    // A key while the banner is up used to drop the notice the user never saw.
    assert_eq!(dashboard.key(KeyCode::F(12)), DashboardAction::None);
    assert!(
        rendered_footer(&dashboard, 120).starts_with("ERROR:"),
        "the banner stays until it is dismissed"
    );

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9_001,
        response: Response::TerminalText {
            session: SessionId(1),
            size: TerminalSize { rows: 1, cols: 1 },
            text: String::new(),
        },
    });
    let footer = rendered_footer(&dashboard, 120);
    assert!(
        footer.contains("2 settings findings; see Settings"),
        "queued findings notice must show once the banner is dismissed, got {footer:?}"
    );
}

const STATUS_OFF: &str = "Claude status off — :connect to set up";

fn no_connect(_: bool) -> ConnectStatus {
    panic!("nothing chose Connect agent status")
}

/// "Not now" at startup is not an error: `run_dashboard` hands the note to
/// `status_off`, which shows it as a plain notice, once, behind a notice already
/// in the footer.
#[test]
fn startup_status_off_is_a_calm_one_time_note_behind_the_findings_notice() {
    let mut quiet = dashboard_fixture();
    quiet.status_off(STATUS_OFF.into(), no_connect);
    assert_eq!(rendered_footer(&quiet, 120), STATUS_OFF);

    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, "unknown_0 = 1\n").unwrap();
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, &document);
    dashboard.status_off(STATUS_OFF.into(), no_connect);
    let footer = rendered_footer(&dashboard, 120);
    assert!(
        footer.contains("1 settings finding; see Settings"),
        "{footer}"
    );
    assert_eq!(dashboard.key(KeyCode::F(12)), DashboardAction::None);
    assert_eq!(rendered_footer(&dashboard, 120), STATUS_OFF);
    assert_eq!(dashboard.key(KeyCode::F(12)), DashboardAction::None);
    let footer = rendered_footer(&dashboard, 120);
    assert!(
        !footer.contains("status off") && !footer.contains("ERROR"),
        "{footer}"
    );
}

/// The palette lists Connect agent status only while startup left status off. A
/// failure is reported and keeps the entry; success replaces that banner with a
/// notice and removes the entry.
#[test]
fn palette_connects_agent_status_until_it_succeeds() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static PREVIEWS: AtomicUsize = AtomicUsize::new(0);
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn connect(apply: bool) -> ConnectStatus {
        if !apply {
            PREVIEWS.fetch_add(1, Ordering::SeqCst);
            return ConnectStatus::Confirm(
                "Lets OVRCR show when your Claude sessions are working, need you or are done.\nAdds or updates OVRCR hooks in:\n  /p/settings.json\nYour other settings, status line and approvals stay as they are.".into(),
            );
        }
        ConnectStatus::Done(match CALLS.fetch_add(1, Ordering::SeqCst) {
            0 => Err("Couldn't connect Claude status: native settings file is read-only in /p/settings.json; fix the file and try again".into()),
            _ => Ok("Claude status connected for new sessions".into()),
        })
    }
    fn listed(dashboard: &mut Dashboard) -> bool {
        dashboard.key(KeyCode::Char(':'));
        let inspect = only_request(dashboard.drain_outbox());
        assert_eq!(inspect.request, Request::Inspect);
        dashboard.event_action(Event::Paste("connect".into()));
        let shown = screen(dashboard).contains("Connect agent status");
        dashboard.key(KeyCode::Esc);
        shown
    }
    fn choose(dashboard: &mut Dashboard) {
        dashboard.key(KeyCode::Char(':'));
        only_request(dashboard.drain_outbox());
        dashboard.event_action(Event::Paste("connect agent status".into()));
        // The entry opens the startup screen's text; nothing is written yet.
        let calls = CALLS.load(Ordering::SeqCst);
        assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
        let page = screen(dashboard);
        assert!(page.contains("Adds or updates OVRCR hooks in:"), "{page}");
        assert!(page.contains("/p/settings.json"), "{page}");
        assert!(page.contains("Enter confirm · Esc cancel"), "{page}");
        assert_eq!(CALLS.load(Ordering::SeqCst), calls);
        assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
        assert!(
            dashboard.drain_outbox().is_empty(),
            "connecting is local, not a Server request"
        );
        assert!(!screen(dashboard).contains("Adds or updates OVRCR hooks in:"));
    }

    let mut dashboard = dashboard_fixture();
    assert!(!listed(&mut dashboard), "nothing to connect");
    dashboard.status_off(STATUS_OFF.into(), connect);
    assert!(listed(&mut dashboard));

    // Esc on the confirmation page writes nothing and keeps the entry.
    dashboard.key(KeyCode::Char(':'));
    only_request(dashboard.drain_outbox());
    dashboard.event_action(Event::Paste("connect agent status".into()));
    dashboard.key(KeyCode::Enter);
    assert!(screen(&dashboard).contains("Adds or updates OVRCR hooks in:"));
    dashboard.key(KeyCode::Esc);
    assert_eq!(CALLS.load(Ordering::SeqCst), 0, "Esc must not connect");
    assert_eq!(PREVIEWS.load(Ordering::SeqCst), 1);
    assert!(!screen(&dashboard).contains("Adds or updates OVRCR hooks in:"));
    assert!(!rendered_footer(&dashboard, 200).contains("connected"));
    assert!(listed(&mut dashboard));

    choose(&mut dashboard);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    let footer = rendered_footer(&dashboard, 200);
    assert!(
        footer.starts_with("ERROR: Couldn't connect Claude status:"),
        "{footer}"
    );
    assert!(listed(&mut dashboard), "a failed attempt can be retried");

    choose(&mut dashboard);
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(
        rendered_footer(&dashboard, 200),
        "Claude status connected for new sessions"
    );
    assert!(
        !listed(&mut dashboard),
        "connected status has nothing to offer"
    );
}

/// A successful connect retires the startup note even when it was waiting behind a
/// banner, so the next key does not bring back `… status off`.
#[test]
fn connecting_retires_a_queued_status_off_note() {
    fn connect(apply: bool) -> ConnectStatus {
        if apply {
            ConnectStatus::Done(Ok("Claude status connected for new sessions".into()))
        } else {
            ConnectStatus::Confirm("Adds or updates OVRCR hooks in:\n  /p/settings.json".into())
        }
    }
    let mut dashboard = dashboard_fixture();
    dashboard.status_off(STATUS_OFF.into(), connect);
    assert_eq!(rendered_footer(&dashboard, 120), STATUS_OFF);
    // A banner arrives while the note shows: the note waits behind it.
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9_000,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "unrelated failure".into(),
        },
    });
    assert!(rendered_footer(&dashboard, 120).starts_with("ERROR:"));
    dashboard.key(KeyCode::Char(':'));
    only_request(dashboard.drain_outbox());
    dashboard.event_action(Event::Paste("connect agent status".into()));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        rendered_footer(&dashboard, 200),
        "Claude status connected for new sessions"
    );
    for _ in 0..3 {
        dashboard.key(KeyCode::F(12));
        let footer = rendered_footer(&dashboard, 200);
        assert!(!footer.contains("status off"), "stale note: {footer}");
    }
}

#[test]
fn settings_opens_from_the_menu_and_has_no_browse_key() {
    let mut dashboard = dashboard_fixture();
    let document = tempfile::tempdir().unwrap();
    let document = document.path().join("dashboard.toml");
    publish(&mut dashboard, &document);
    dashboard.key(KeyCode::Char(','));
    assert!(!open(&dashboard), "comma is not a Browse key");
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('v'));
    assert!(screen(&dashboard).contains(",  Settings"));
    dashboard.key(KeyCode::Char(','));
    assert!(open(&dashboard), "{}", screen(&dashboard));
    assert!(
        focused_row(&dashboard).contains("Alerts"),
        "the menu opens category focus on Alerts:\n{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());
}

/// Every catalog group, including the ones with no Dashboard shortcuts, is a
/// category; a palette setting entry opens its owning category with that row
/// selected in list focus.
#[test]
fn every_catalog_category_is_reachable_and_palette_settings_open_context() {
    let (mut dashboard, _root, _document) = menu_editor();
    let categories = [
        "Alerts",
        "Appearance",
        "Workspaces",
        "Titles",
        "Usage",
        "Agents",
        "Remembered launches",
        "Operator",
        "Delegation",
        "Remote",
    ];
    for (index, category) in categories.iter().enumerate() {
        assert!(
            focused_row(&dashboard).contains(category),
            "category {index} must be focused:\n{}",
            screen(&dashboard)
        );
        if index + 1 < categories.len() {
            assert_eq!(dashboard.key(KeyCode::Down), DashboardAction::Redraw);
        }
    }
    assert!(dashboard.drain_outbox().is_empty());

    // Commands entries for individual settings open that setting in context.
    let mut dashboard = dashboard_fixture();
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    publish(&mut dashboard, &document);
    dashboard.key(KeyCode::Char(':'));
    only_request(dashboard.drain_outbox());
    dashboard.event_action(Event::Paste("Settings: Theme".into()));
    dashboard.key(KeyCode::Enter);
    assert!(open(&dashboard), "{}", screen(&dashboard));
    assert!(
        focused_line_with(&dashboard, "Theme").contains("Theme"),
        "the palette must reveal Theme in list focus:\n{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());
}

/// Settings written before the first reading explains why nothing can be
/// edited yet; the next published reading restores the controls.
#[test]
fn settings_without_a_reading_explains_and_recovers_on_publish() {
    let mut dashboard = dashboard_fixture();
    open_from_menu(&mut dashboard);
    let text = screen(&dashboard);
    assert!(
        text.contains("Waiting for the Server's settings reading."),
        "{text}"
    );
    assert!(
        !text.contains("No setting matches"),
        "no misleading filter result:\n{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());

    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, "branch_prefix = \"kh/\"\n").unwrap();
    publish(&mut dashboard, &document);
    let text = screen(&dashboard);
    assert!(!text.contains("Waiting for the Server's settings reading."));
    focus_category(&mut dashboard, "Workspaces");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "Branch prefix");
    let text = screen(&dashboard);
    assert!(text.contains("Source: dashboard.toml"), "{text}");
    assert!(text.contains("Value: kh/"), "{text}");
}

/// Rows stay one line each and show only the effective value; source,
/// default, reset scope, raw key and the document reading live in the fixed
/// details area for the focused row.
#[test]
fn rows_show_the_value_and_details_carry_source_default_and_reset() {
    let (mut dashboard, _root, document) = editor("branch_prefix = \"kh/\"\nready_sound = true\n");
    focus_category(&mut dashboard, "Workspaces");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "Branch prefix");
    let row = focused_line_with(&dashboard, "Branch prefix");
    assert!(
        row.contains("kh/") && !row.contains(" set") && !row.contains("default"),
        "the row carries the effective value only:\n{}",
        screen(&dashboard)
    );
    let text = screen(&dashboard);
    assert!(text.contains("Source: dashboard.toml"), "{text}");
    assert!(text.contains("Default: feature/"), "{text}");
    assert!(text.contains("Reset:"), "{text}");
    assert!(text.contains("Value: kh/"), "{text}");
    assert!(text.contains("key: branch_prefix"), "{text}");
    assert!(
        text.contains(&format!("Document: {}", document.display())),
        "{text}"
    );

    focus_row(&mut dashboard, "Automatic local terminals");
    let text = screen(&dashboard);
    assert!(text.contains("Source: default"), "{text}");
    assert!(text.contains("Default: default_branch_only"), "{text}");
    assert!(
        !text.contains("Reset:"),
        "a default value has no reset target:\n{text}"
    );
    assert_eq!(
        focused_line_with(&dashboard, "Automatic local terminals")
            .matches("default_branch_only")
            .count(),
        1,
        "the row shows the value once; source is in details:\n{text}"
    );
}

/// Moving the selection changes only the focus marker and details; every row
/// keeps its line and its column, and values share one column.
#[test]
fn moving_the_selection_leaves_neighboring_rows_and_columns_in_place() {
    let (mut dashboard, _root, _document) = editor("");
    focus_category(&mut dashboard, "Alerts");
    dashboard.key(KeyCode::Enter);
    let labels = ["Desktop notifications", "Ready sound", "Exact iTerm focus"];
    let geometry = |dashboard: &Dashboard| {
        let rows = rendered_rows(dashboard, 120, 60);
        let divider = rows
            .iter()
            .find_map(|line| line.chars().position(|ch| ch == '│'))
            .expect("pane divider");
        labels
            .iter()
            .map(|label| {
                let index = rows
                    .iter()
                    .position(|line| {
                        let pane: String = line.chars().skip(divider + 1).collect();
                        pane.starts_with(&format!("› {label}"))
                            || pane.starts_with(&format!("  {label}"))
                    })
                    .unwrap_or_else(|| panic!("no row {label:?}:\n{}", rows.join("\n")));
                let pane: String = rows[index].chars().skip(divider + 1).collect();
                let column = divider + 1 + cell_index(&pane, label);
                (index, column)
            })
            .collect::<Vec<_>>()
    };
    let before = geometry(&dashboard);
    let detail_line = rendered_rows(&dashboard, 120, 60)
        .iter()
        .position(|line| line.contains("Value:"))
        .expect("details");
    let footer = rendered_footer(&dashboard, 120);
    assert!(footer.contains("Enter turn on"), "{footer}");
    // One step keeps a toggle focused (Desktop notifications -> Ready sound).
    assert_eq!(dashboard.key(KeyCode::Down), DashboardAction::Redraw);
    assert_eq!(geometry(&dashboard), before, "rows moved");
    assert!(
        rendered_footer(&dashboard, 120).contains("Enter turn on"),
        "the focused toggle's action stays named"
    );
    assert_eq!(
        rendered_rows(&dashboard, 120, 60)
            .iter()
            .position(|line| line.contains("Value:")),
        Some(detail_line),
        "the fixed details block stays put while its description height changes"
    );

    let rows = rendered_rows(&dashboard, 120, 60);
    let value_column = |label: &str, value: &str| {
        let line = rows.iter().find(|line| line.contains(label)).unwrap();
        cell_index(line, value)
    };
    let off = value_column("Desktop notifications", "off");
    assert_eq!(value_column("Ready sound", "off"), off);
    assert_eq!(value_column("Exact iTerm focus", "off"), off);
    assert!(dashboard.drain_outbox().is_empty());
}

/// Findings are visible from another category. `d` opens the document pane
/// with the full diagnostics; `f` cycles keyed findings into their category
/// and row, then falls back to the document for findings without a row.
#[test]
fn findings_surface_across_categories_and_route_by_f_and_d() {
    let (mut dashboard, _root, document) = editor("colour = \"blue\"\nready_sound = \"loud\"\n");
    let text = screen(&dashboard);
    assert!(
        text.contains("! 2 findings · f next"),
        "the header counts findings outside Alerts:\n{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.contains("Alerts") && line.contains(" !")),
        "the affected category carries a finding mark:\n{text}"
    );
    assert_eq!(dashboard.key(KeyCode::Char('d')), DashboardAction::Redraw);
    let text = screen(&dashboard);
    assert!(text.contains("Settings / Document"), "{text}");
    assert!(text.contains("Findings"), "{text}");
    assert!(
        text.contains("colour") && text.contains("unknown setting"),
        "the document keeps the unknown-key diagnostic:\n{text}"
    );
    assert!(
        text.contains(&format!("Document: {}", document.display())),
        "{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());

    // `f` cycles both findings: one lands on the row that owns Ready sound,
    // the unknown key has no row and opens the document instead.
    dashboard.key(KeyCode::Esc);
    let mut saw_row = false;
    let mut saw_document = false;
    for _ in 0..2 {
        assert_eq!(dashboard.key(KeyCode::Char('f')), DashboardAction::Redraw);
        if screen(&dashboard).contains("Settings / Document") {
            saw_document = true;
            assert!(
                screen(&dashboard).contains("colour"),
                "{}",
                screen(&dashboard)
            );
            dashboard.key(KeyCode::Esc);
        } else {
            saw_row = true;
            assert!(
                focused_line_with(&dashboard, "Ready sound").contains("Ready sound"),
                "f reveals the keyed finding's row:\n{}",
                screen(&dashboard)
            );
            assert!(
                screen(&dashboard).contains("expected a boolean"),
                "the row's finding is in details:\n{}",
                screen(&dashboard)
            );
        }
    }
    assert!(
        saw_row && saw_document,
        "both finding routes must be reachable"
    );

    // A valid published reading clears the banner and controls come back.
    std::fs::write(&document, "ready_sound = true\n").unwrap();
    publish(&mut dashboard, &document);
    assert!(
        !screen(&dashboard).contains("findings"),
        "resolved findings clear:\n{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Esc);
    assert!(
        focused_row(&dashboard).contains("Alerts"),
        "{}",
        screen(&dashboard)
    );
    assert!(!screen(&dashboard).contains("! 2 findings"));
    assert!(dashboard.drain_outbox().is_empty());
}

/// Consent consequences appear before the action, so Enter is informed.
#[test]
fn consent_consequences_show_before_activation() {
    let (mut dashboard, _root, _document) = editor("");
    select(&mut dashboard, "title_model");
    let text = screen(&dashboard);
    assert!(
        text.contains("Setting a model makes paid calls to title sessions."),
        "{text}"
    );
    assert!(text.contains("Source: default"), "{text}");
    assert!(
        rendered_footer(&dashboard, 120).contains("Enter"),
        "the footer names the consent action:\n{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());
}

/// A saved alert choice shows why it is inactive and the short picker filters
/// without writing.
#[cfg(target_os = "macos")]
#[test]
fn alert_dependency_and_short_picker_guidance_precede_the_action() {
    let (mut dashboard, _root, document) = editor("ready_sound_choice = \"tap\"\n");
    select(&mut dashboard, "ready_sound_choice");
    let text = screen(&dashboard);
    assert!(
        text.contains("Inactive: Desktop notifications are off"),
        "{text}"
    );
    assert!(
        focused_line_with(&dashboard, "Ready sound choice").contains("inactive"),
        "{text}"
    );

    std::fs::write(
        &document,
        "desktop_notifications = true\nready_sound_choice = \"tap\"\n",
    )
    .unwrap();
    publish(&mut dashboard, &document);
    let text = screen(&dashboard);
    assert!(
        text.contains("Inactive: Ready sound is off"),
        "the next missing prerequisite is named:\n{text}"
    );

    std::fs::write(
        &document,
        "desktop_notifications = true\nready_sound = true\nready_sound_choice = \"tap\"\n",
    )
    .unwrap();
    publish(&mut dashboard, &document);
    let text = screen(&dashboard);
    assert!(!text.contains("Inactive:"), "{text}");
    assert!(
        !focused_line_with(&dashboard, "Ready sound choice").contains("inactive"),
        "{text}"
    );

    // Choosing the sound opens the short list; filtering is local.
    dashboard.key(KeyCode::Enter);
    let text = screen(&dashboard);
    assert!(text.contains("Find an option:"), "{text}");
    for option in ["System default", "Tap", "Chime", "Rise"] {
        assert!(text.contains(option), "missing {option}:\n{text}");
    }
    assert!(
        focused_line_with(&dashboard, "Tap").contains("Tap"),
        "the saved choice is highlighted:\n{text}"
    );
    dashboard.event_action(Event::Paste("ri".into()));
    assert!(
        screen(&dashboard).contains("Find an option: ri"),
        "{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("ready_sound_choice", Some("\"rise\""))
    );
    assert!(
        focused_line_with(&dashboard, "Ready sound choice").contains("Rise"),
        "{}",
        screen(&dashboard)
    );
}

/// A value too long for one row is fully readable in the wrapped details; an
/// overflowing details block advertises Ctrl-d/Ctrl-u scrolling.
#[test]
fn long_values_and_explanations_are_reachable_at_narrow_width() {
    let long = format!("kept/{}", "word ".repeat(40));
    let (mut dashboard, _root, _document) = editor(&format!("branch_prefix = \"{long}\"\n"));
    // A single-pane frame keeps the whole value in uninterrupted wrapped
    // details (no sidebar interleaves the detail lines).
    let mut rows = rendered_rows(&dashboard, 60, 24);
    assert!(rows[0].contains("Settings /"), "{rows:?}");
    select(&mut dashboard, "branch_prefix");
    rows = rendered_rows(&dashboard, 60, 24);
    let text = rows.join("\n");
    assert!(text.contains("Value:"), "{text}");
    let flattened = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flattened(&text).contains(&flattened(&long)),
        "the full effective value wraps in details:\n{text}"
    );
    assert!(
        rendered_footer(&dashboard, 60).contains("Ctrl-d details"),
        "overflowing details advertise scrolling:\n{text}"
    );
    assert!(rows.iter().any(|line| line.contains("Source:")));
    let detail_block = rows[16..23].to_vec();
    dashboard.ctrl('d');
    let scrolled = rendered_rows(&dashboard, 60, 24);
    let scrolled_text = scrolled.join("\n");
    assert!(
        scrolled_text.contains("Ctrl-u details"),
        "after scrolling the footer points back:\n{scrolled_text}"
    );
    assert_ne!(
        scrolled[16..23].to_vec(),
        detail_block,
        "the fixed details area really scrolled:\n{scrolled_text}"
    );
    dashboard.ctrl('u');
    let restored = rendered_rows(&dashboard, 60, 24);
    assert_eq!(
        restored[16..23].to_vec(),
        detail_block,
        "Ctrl-u returns to the top of details"
    );
    assert!(
        rendered_footer(&dashboard, 60).contains("Ctrl-d details"),
        "{}",
        restored.join("\n")
    );
}

#[test]
fn toggle_saves_at_once_and_applies_only_the_servers_reading() {
    let (mut dashboard, _root, document) = editor("ready_sound = true # mine\n");
    select(&mut dashboard, "desktop_notifications");
    let row = focused_line_with(&dashboard, "Desktop notifications");
    assert!(row.contains("off"), "{row}");
    assert!(
        rendered_footer(&dashboard, 120).contains("Enter turn on"),
        "the footer names the toggle's direction:\n{}",
        screen(&dashboard)
    );
    let action = dashboard.key(KeyCode::Enter);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let message = only_request(dashboard.drain_outbox());
    assert_eq!(
        message.request,
        Request::SetSetting {
            path: "desktop_notifications".into(),
            value: Some("true".into()),
        }
    );
    // Nothing changes until the Server's reading arrives; no confirmation.
    assert!(focused_line_with(&dashboard, "Desktop notifications").contains("off"));
    assert!(
        focused_line_with(&dashboard, "Desktop notifications").contains("saving…"),
        "{}",
        screen(&dashboard)
    );
    answer(&mut dashboard, &document, message);
    let row = focused_line_with(&dashboard, "Desktop notifications");
    assert!(row.contains("on"), "{}", screen(&dashboard));
    assert!(!row.contains("saving…"));
    let text = screen(&dashboard);
    assert!(text.contains("Source: dashboard.toml"), "{text}");
    assert!(text.contains("Default: off"), "{text}");
    assert!(text.contains("Reset:"), "{text}");
    assert!(
        rendered_footer(&dashboard, 120).contains("Enter turn off"),
        "{}",
        screen(&dashboard)
    );
    assert_eq!(
        std::fs::read_to_string(&document).unwrap(),
        "ready_sound = true # mine\ndesktop_notifications = true\n"
    );
}

/// At 80x24 the category sidebar keeps its column and divider, setting rows
/// stay in the pane, and the details block occupies its fixed seven rows
/// between one blank separator and the footer.
#[test]
fn wide_80x24_frame_pins_sidebar_rows_details_and_footer() {
    let (mut dashboard, _root, _document) = menu_editor();
    let rows = rendered_rows(&dashboard, 80, 24);
    assert_eq!(rows.len(), 24);
    assert!(rows[0].contains("Settings"), "{rows:?}");
    assert!(
        rows[23].contains("Enter"),
        "the footer names the category action: {:?}",
        rows[23]
    );
    assert!(
        rows[15].chars().skip(21).all(|ch| ch == ' '),
        "one blank separator in the main pane: {rows:?}"
    );
    let alerts_row = rows
        .iter()
        .position(|line| line.starts_with("› Alerts"))
        .expect("Alerts sidebar row");
    for y in [alerts_row, 14, 16, 22] {
        assert_eq!(
            rows[y].chars().nth(20),
            Some('│'),
            "the 20-column sidebar divider must be fixed on row {y}: {:?}",
            rows[y]
        );
    }
    let categories = [
        "Alerts",
        "Appearance",
        "Workspaces",
        "Titles",
        "Usage",
        "Agents",
        "Remembered launches",
        "Operator",
        "Delegation",
        "Remote",
    ];
    for category in categories {
        let (x, _) = cell_of(&rows, category);
        assert!(x < 20, "{category} belongs in the sidebar: {rows:?}");
    }
    assert!(rows.iter().any(|line| line.starts_with("› Alerts")));
    let (desktop_x, _) = cell_of(&rows, "Desktop notifications");
    assert!(
        desktop_x > 20,
        "the active category's rows occupy the pane: {rows:?}"
    );

    // List focus keeps the category marker and adds the row marker; the
    // selected row keeps its line and the details take the fixed block.
    dashboard.key(KeyCode::Enter);
    let focused = rendered_rows(&dashboard, 80, 24);
    assert!(focused[0].contains("Settings"));
    assert!(focused[15].chars().skip(21).all(|ch| ch == ' '));
    assert!(
        focused[23].contains("Enter turn on"),
        "the footer names the focused toggle's action: {:?}",
        focused[23]
    );
    assert!(
        focused.iter().any(|line| line.contains("• Alerts")),
        "the sidebar marks the open category: {focused:?}"
    );
    assert_eq!(
        focused
            .iter()
            .filter(|line| line.matches("› ").count() == 1)
            .count(),
        1,
        "only the row carries the focus marker: {focused:?}"
    );
    let row = focused
        .iter()
        .find(|line| line.contains("› Desktop notifications"))
        .expect("selected row marker");
    assert!(
        cell_index(row, "› Desktop notifications") > 20,
        "the selected row stays in the main pane: {row:?}"
    );
    assert!(
        focused.iter().any(|line| line.contains("Value: off")),
        "the effective value is in the fixed details block: {focused:?}"
    );
    assert!(
        flatten_frame(focused.clone()).contains("permission prompt"),
        "the full permission warning is visible before scrolling: {focused:?}"
    );
    assert!(
        !focused.iter().any(|line| line.contains("Source:")),
        "metadata is on demand behind the initial guidance: {focused:?}"
    );

    // Ctrl-d scrolls the fixed details block to the on-demand metadata without
    // moving the selected list row.
    for _ in 0..3 {
        let text = rendered_rows(&dashboard, 80, 24);
        if text.iter().any(|line| line.contains("Source:"))
            && text.iter().any(|line| line.contains("Default: off"))
        {
            break;
        }
        dashboard.ctrl('d');
    }
    let scrolled = rendered_rows(&dashboard, 80, 24);
    assert!(
        scrolled
            .iter()
            .any(|line| line.contains("› Desktop notifications")),
        "the list selection does not move: {scrolled:?}"
    );
    assert!(
        scrolled.iter().any(|line| line.contains("Default: off")),
        "the default is reachable on demand: {scrolled:?}"
    );
    let (source_x, source_y) = cell_of(&scrolled, "Source:");
    assert!(
        (16..=22).contains(&source_y) && source_x > 20,
        "details keep the fixed block while scrolled: {scrolled:?}"
    );
    assert!(
        scrolled[23].contains("Enter turn on"),
        "the footer keeps naming the toggle action: {:?}",
        scrolled[23]
    );
    assert!(dashboard.drain_outbox().is_empty());
}

/// At 60 columns Settings shows one pane: a category index first, then a
/// breadcrumbed list, then a breadcrumbed editor or picker.
#[test]
fn narrow_60x24_frame_shows_one_pane_with_breadcrumbs() {
    let (mut dashboard, _root, _document) = menu_editor();
    let mut rows = rendered_rows(&dashboard, 60, 24);
    assert_eq!(rows[0].trim(), "Settings / Categories", "{rows:?}");
    assert!(rows[23].contains("Enter"), "{rows:?}");
    assert!(
        rows.iter().all(|line| !line.contains('│')),
        "no sidebar divider at 60 columns: {rows:?}"
    );
    assert!(rows.iter().any(|line| line.starts_with("› Alerts")));
    assert!(
        !rows
            .iter()
            .any(|line| line.contains("Desktop notifications"))
    );

    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Enter);
    rows = rendered_rows(&dashboard, 60, 24);
    assert_eq!(rows[0].trim(), "Settings / Appearance", "{rows:?}");
    assert!(rows.iter().any(|line| line.contains("› Theme")));
    dashboard.key(KeyCode::Enter);
    rows = rendered_rows(&dashboard, 60, 24);
    assert_eq!(rows[0].trim(), "Settings / Appearance / Theme", "{rows:?}");
    assert!(
        rows.iter().any(|line| line.contains("Find a theme:")),
        "{rows:?}"
    );
    assert!(dashboard.drain_outbox().is_empty());

    dashboard.key(KeyCode::Esc);
    assert!(
        rendered_rows(&dashboard, 60, 24)[0]
            .trim()
            .ends_with("/ Appearance")
    );
    dashboard.key(KeyCode::Esc);
    assert_eq!(
        rendered_rows(&dashboard, 60, 24)[0].trim(),
        "Settings / Categories"
    );
}

/// A too-small frame explains the required size; restoring space recovers the
/// same category, row and scroll, and a draft survives both transitions.
#[test]
fn too_small_frame_recovers_focus_scroll_and_draft() {
    let (mut dashboard, _root, _document) = menu_editor();
    focus_category(&mut dashboard, "Usage");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "Cursor state database");
    let before = rendered_rows(&dashboard, 120, 12);
    let detail_start = before
        .iter()
        .position(|line| line.contains("Source:"))
        .expect("details");
    let visible_before = before[1..detail_start].to_vec();

    let small = rendered_rows(&dashboard, 47, 7).join("\n");
    assert!(
        small.contains("Settings needs a larger terminal"),
        "{small}"
    );
    assert!(
        small.contains("Resize to at least 48 columns × 8 rows."),
        "{small}"
    );
    assert!(!small.contains("Alerts"), "{small}");
    assert_eq!(
        dashboard.event_action(Event::Resize(120, 16)),
        DashboardAction::Redraw
    );
    let after = rendered_rows(&dashboard, 120, 12);
    let detail_start = after
        .iter()
        .position(|line| line.contains("Source:"))
        .expect("details");
    assert_eq!(
        after[1..detail_start].to_vec(),
        visible_before,
        "the same row and scroll survive the too-small frame:\n{after:?}"
    );

    // A text draft survives narrowing and a too-small frame.
    dashboard.key(KeyCode::Esc);
    focus_category(&mut dashboard, "Workspaces");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "Branch prefix");
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("draft".into()));
    assert_eq!(
        dashboard.event_action(Event::Resize(60, 24)),
        DashboardAction::Redraw
    );
    let narrow = rendered_rows(&dashboard, 60, 24).join("\n");
    assert!(narrow.contains("feature/draft"), "{narrow}");
    assert!(narrow.contains("Settings / Workspaces"), "{narrow}");
    let _ = rendered_rows(&dashboard, 40, 7);
    assert_eq!(
        dashboard.event_action(Event::Resize(80, 24)),
        DashboardAction::Redraw
    );
    let wide = rendered_rows(&dashboard, 80, 24).join("\n");
    assert!(wide.contains("feature/draft"), "{wide}");
    assert_eq!(
        dashboard.key(KeyCode::Esc),
        DashboardAction::Redraw,
        "Esc cancels the preserved draft"
    );
    assert!(focused_line_with(&dashboard, "Branch prefix").contains("Branch prefix"));
    assert!(dashboard.drain_outbox().is_empty());
}

/// A click selects a category and a second click enters its list; wheel and
/// clicks over the list focus rows without editing; a click selects a search
/// result but Enter opens it.
#[test]
fn mouse_wheel_and_clicks_navigate_categories_rows_and_results() {
    use crossterm::event::MouseButton;
    let (mut dashboard, _root, _document) = menu_editor();
    let area = Rect::new(0, 0, 80, 24);
    let click = |dashboard: &mut Dashboard, x: u16, y: u16| {
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                x,
                y,
                KeyModifiers::NONE,
            ),
            area,
        )
    };

    // Rendering the same size keeps the hit-testing frame in step.
    let rows = rendered_rows(&dashboard, 80, 24);
    let (x, y) = cell_of(&rows, "Appearance");
    assert_eq!(click(&mut dashboard, x, y), DashboardAction::Redraw);
    assert!(focused_row(&dashboard).contains("Appearance"));
    let _ = rendered_rows(&dashboard, 80, 24);
    click(&mut dashboard, x, y);
    assert!(
        focused_line_with(&dashboard, "Theme").contains("Theme"),
        "a second click enters the category:\n{}",
        screen(&dashboard)
    );

    // Re-enter Alerts, then wheel over its list.
    dashboard.key(KeyCode::Esc);
    let _ = rendered_rows(&dashboard, 80, 24);
    let rows = rendered_rows(&dashboard, 80, 24);
    let (x, y) = cell_of(&rows, "Alerts");
    click(&mut dashboard, x, y);
    assert!(focused_row(&dashboard).contains("Alerts"));
    let _ = rendered_rows(&dashboard, 80, 24);
    click(&mut dashboard, x, y);
    let _ = rendered_rows(&dashboard, 80, 24);
    let rows = rendered_rows(&dashboard, 80, 24);
    let (x, y) = cell_of(&rows, "Desktop notifications");
    dashboard.mouse_action(
        mouse_event(MouseEventKind::ScrollDown, x, y, KeyModifiers::NONE),
        area,
    );
    assert!(
        focused_line_with(&dashboard, "Ready sound").contains("Ready sound"),
        "{}",
        screen(&dashboard)
    );
    assert!(dashboard.drain_outbox().is_empty());

    // A row click only focuses; it never opens the editor.
    let _ = rendered_rows(&dashboard, 80, 24);
    let rows = rendered_rows(&dashboard, 80, 24);
    let (x, y) = cell_of(&rows, "Exact iTerm focus");
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            x,
            y,
            KeyModifiers::NONE,
        ),
        area,
    );
    assert!(focused_line_with(&dashboard, "Exact iTerm focus").contains("Exact iTerm focus"));
    assert!(dashboard.drain_outbox().is_empty());

    // Search result click selects; Enter then reveals the setting row.
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("branch_prefix".into()));
    let rows = rendered_rows(&dashboard, 80, 24);
    let (x, y) = cell_of(&rows, "Workspaces · Branch prefix");
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            x,
            y,
            KeyModifiers::NONE,
        ),
        area,
    );
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Enter);
    assert!(focused_line_with(&dashboard, "Branch prefix").contains("Branch prefix"));
    assert!(dashboard.drain_outbox().is_empty());
}

/// Collect actual Dashboard-role frames, retaining the request's reply and,
/// when expected, the Server's settings reading. Delivery to the Dashboard
/// stays under the test's control so two outstanding edits can interleave.
#[cfg(target_os = "macos")]
fn settings_frames(
    stream: &mut std::os::unix::net::UnixStream,
    message: &ClientMessage,
    expect_reading: bool,
) -> anyhow::Result<Vec<ServerMessage>> {
    use ovrcr::protocol::{read_frame, write_frame};
    use std::time::Instant;

    let deadline = Instant::now() + live::wait_deadline();
    write_frame(stream, message)?;
    let mut frames = Vec::new();
    let mut answered = false;
    let mut read = !expect_reading;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(!remaining.is_zero(), "settings request did not complete");
        stream.set_read_timeout(Some(remaining))?;
        let frame = read_frame::<ServerMessage>(stream)?;
        answered |= matches!(
            &frame,
            ServerMessage::Response { request_id, .. } if *request_id == message.request_id
        );
        read |= matches!(
            &frame,
            ServerMessage::Event(ServerEvent::SettingsChanged(_))
        );
        frames.push(frame);
        if answered && read {
            return Ok(frames);
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn pending_root_move_and_sound_save_match_interleaved_server_replies() {
    use ovrcr::protocol::{ReadySoundChoice, exchange_preamble};
    use std::os::unix::net::UnixStream;

    let original = "\
desktop_notifications = false # keep banners off
ready_sound = true # keep saved sound-only opt-in
ready_sound_choice = \"tap\" # tone
picker_roots = [\"/a\", \"/b\", \"/c\"] # roots
branch_prefix = 'keep/' # unrelated
[quota]
enabled = false
";
    for refuse_root in [false, true] {
        for accept_first in [false, true] {
            let fixture = live::Live::idle().bounded();
            let document = fixture.config.join("dashboard.toml");
            std::fs::write(&document, original).unwrap();
            fixture.start_binary();
            let mut stream = UnixStream::connect(&fixture.socket).unwrap();
            stream.set_read_timeout(fixture.timeout()).unwrap();
            stream.set_write_timeout(fixture.timeout()).unwrap();
            exchange_preamble(&mut stream).unwrap();
            let mut dashboard = Dashboard::new(TerminalSize {
                rows: 60,
                cols: 120,
            });
            dashboard.install_area(Rect::new(0, 0, 120, 60));
            for frame in settings_frames(
                &mut stream,
                &ClientMessage {
                    request_id: u64::MAX,
                    request: Request::DashboardHello,
                },
                true,
            )
            .unwrap()
            {
                if let ServerMessage::Event(ServerEvent::SettingsChanged(report)) = &frame {
                    assert_eq!(report.path, document);
                    assert!(!report.settings.desktop_notifications);
                    assert!(report.settings.ready_sound);
                    assert_eq!(
                        report.settings.ready_sound_choice,
                        Some(ReadySoundChoice::Tap)
                    );
                }
                dashboard.handle_server_message(frame);
            }
            dashboard.key(KeyCode::Char(':'));
            let inspect = only_request(dashboard.drain_outbox());
            assert_eq!(inspect.request, Request::Inspect);
            dashboard.event_action(Event::Paste("settings".into()));
            dashboard.key(KeyCode::Enter);

            // Reorder /b after /c from the picker-roots manager.
            select(&mut dashboard, "picker_roots");
            dashboard.key(KeyCode::Enter);
            focus_row(&mut dashboard, "/b");
            assert_eq!(
                dashboard.key(KeyCode::Char(']')),
                ovrcr::tui::DashboardAction::Redraw
            );
            let root_move = only_request(dashboard.drain_outbox());
            assert_eq!(
                root_move.request,
                Request::SetSetting {
                    path: "picker_roots".into(),
                    value: Some("[\"/a\", \"/c\", \"/b\"]".into()),
                }
            );
            // The affected folder keeps the manager's selection while the
            // whole-list write is pending; the summary also names it pending.
            assert!(
                focused_line_with(&dashboard, "/b").contains("/b"),
                "the moved folder stays selected:\n{}",
                screen(&dashboard)
            );
            let manager_pending = screen(&dashboard).contains("saving…");
            dashboard.key(KeyCode::Esc);
            let summary_pending = focused_line_with(&dashboard, "Picker roots").contains("saving…");
            assert!(
                manager_pending || summary_pending,
                "a pending root move is visible for the affected folder:\n{}",
                screen(&dashboard)
            );

            // Leave the list, then save a different setting while the root
            // move is still pending.
            dashboard.key(KeyCode::Esc);
            select(&mut dashboard, "ready_sound_choice");
            assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
            dashboard.event_action(Event::Paste("Rise".into()));
            assert_eq!(
                dashboard.key(KeyCode::Enter),
                ovrcr::tui::DashboardAction::Redraw
            );
            let sound_save = only_request(dashboard.drain_outbox());
            assert_eq!(
                sound_save.request,
                Request::SetSetting {
                    path: "ready_sound_choice".into(),
                    value: Some("\"rise\"".into()),
                }
            );
            assert_ne!(root_move.request_id, sound_save.request_id);
            let tone = focused_line_with(&dashboard, "Ready sound choice");
            assert!(tone.contains("Tap") && tone.contains("saving…"), "{tone}");
            assert_eq!(std::fs::read_to_string(&document).unwrap(), original);

            // Re-enter the manager on the affected /b and keep it open while
            // both replies arrive; its selection must not jump to a target
            // index before the Server confirms the new order.
            dashboard.key(KeyCode::Esc);
            select(&mut dashboard, "picker_roots");
            dashboard.key(KeyCode::Enter);
            focus_row(&mut dashboard, "/b");
            assert!(focused_line_with(&dashboard, "/b").contains("/b"));

            let (refused, accepted) = if refuse_root {
                (&root_move, &sound_save)
            } else {
                (&sound_save, &root_move)
            };
            // A real Server write refusal, without changing document bytes or
            // asking the watcher to observe a temporary invalid document.
            let permissions = std::fs::metadata(&document).unwrap().permissions();
            let mut read_only = permissions.clone();
            read_only.set_readonly(true);
            std::fs::set_permissions(&document, read_only).unwrap();
            let refusal = settings_frames(&mut stream, refused, false);
            std::fs::set_permissions(&document, permissions).unwrap();
            let refusal = refusal.unwrap();
            let refused_path = if refuse_root {
                "picker_roots"
            } else {
                "ready_sound_choice"
            };
            assert!(
                refusal.iter().any(|frame| matches!(
                    frame,
                    ServerMessage::Response {
                        request_id,
                        response: Response::Error { code: ErrorCode::InvalidRequest, message },
                    } if *request_id == refused.request_id
                        && message.contains(&format!("could not save {refused_path}"))
                        && message.contains("read-only")
                )),
                "{refusal:?}"
            );
            assert_eq!(std::fs::read_to_string(&document).unwrap(), original);
            let accepted_frames = settings_frames(&mut stream, accepted, true).unwrap();
            assert!(
                accepted_frames.iter().any(|frame| matches!(
                    frame,
                    ServerMessage::Response { request_id, response: Response::Ok }
                        if *request_id == accepted.request_id
                )),
                "{accepted_frames:?}"
            );
            let saved = original.replace(
                if refuse_root {
                    "\"tap\""
                } else {
                    "[\"/a\", \"/b\", \"/c\"]"
                },
                if refuse_root {
                    "\"rise\""
                } else {
                    "[\"/a\", \"/c\", \"/b\"]"
                },
            );
            assert_eq!(std::fs::read_to_string(&document).unwrap(), saved);

            let (first, second) = if accept_first {
                (accepted_frames, refusal)
            } else {
                (refusal, accepted_frames)
            };
            let (mut root_accepted, mut sound_accepted) = (false, false);
            let (mut root_refused, mut sound_refused) = (false, false);
            for (index, frames) in [first, second].into_iter().enumerate() {
                for frame in frames {
                    if let ServerMessage::Event(ServerEvent::SettingsChanged(report)) = &frame {
                        assert!(!report.settings.desktop_notifications);
                        assert!(report.settings.ready_sound);
                        assert_eq!(
                            report.settings.ready_sound_choice,
                            Some(if refuse_root {
                                ReadySoundChoice::Rise
                            } else {
                                ReadySoundChoice::Tap
                            })
                        );
                        assert_eq!(
                            report.settings.picker_roots,
                            (if refuse_root {
                                ["/a", "/b", "/c"]
                            } else {
                                ["/a", "/c", "/b"]
                            })
                            .map(PathBuf::from)
                        );
                    }
                    dashboard.handle_server_message(frame);
                }
                let accepted_now = (index == 0) == accept_first;
                if refuse_root {
                    if accepted_now {
                        sound_accepted = true;
                    } else {
                        root_refused = true;
                    }
                } else if accepted_now {
                    root_accepted = true;
                } else {
                    sound_refused = true;
                }

                // The manager stays open on the affected /b: confirmation may
                // change its index but never its identity. Only the root move
                // is pending here; a sound reply cannot clear or replace it.
                assert!(
                    focused_line_with(&dashboard, "/b").contains("/b"),
                    "the affected folder keeps selection:\n{}",
                    screen(&dashboard)
                );
                let text = screen(&dashboard);
                if root_accepted {
                    assert!(
                        text.find("/a").unwrap() < text.find("/c").unwrap()
                            && text.find("/c").unwrap() < text.find("/b").unwrap(),
                        "{text}"
                    );
                } else {
                    assert!(
                        text.find("/a").unwrap() < text.find("/b").unwrap()
                            && text.find("/b").unwrap() < text.find("/c").unwrap(),
                        "{text}"
                    );
                }
                assert_eq!(
                    text.contains("saving…"),
                    !root_accepted && !root_refused,
                    "the manager carries its own pending state:\n{text}"
                );
            }
            dashboard.key(KeyCode::Esc);
            if root_refused {
                assert!(
                    screen(&dashboard).contains("refused:"),
                    "{}",
                    screen(&dashboard)
                );
            }
            assert!(
                !focused_line_with(&dashboard, "Picker roots").contains("saving…"),
                "{}",
                screen(&dashboard)
            );

            // The sound row keeps its own confirmed value and refusal.
            select(&mut dashboard, "ready_sound_choice");
            let row = focused_line_with(&dashboard, "Ready sound choice");
            assert!(
                row.contains(if sound_accepted { "Rise" } else { "Tap" }),
                "{row}"
            );
            assert!(!row.contains("saving…"), "{row}");
            if sound_refused {
                assert!(
                    screen(&dashboard).contains("refused:"),
                    "{}",
                    screen(&dashboard)
                );
            }
            assert!(!rendered_footer(&dashboard, 120).contains("could not save"));
            dashboard.key(KeyCode::Esc);
            select(&mut dashboard, "desktop_notifications");
            assert!(focused_line_with(&dashboard, "Desktop notifications").contains("off"));
            dashboard.key(KeyCode::Esc);
            select(&mut dashboard, "ready_sound");
            assert!(focused_line_with(&dashboard, "Ready sound").contains("on"));
            assert_eq!(std::fs::read_to_string(&document).unwrap(), saved);
            drop(stream);
            assert_eq!(
                fixture.request(Request::Shutdown { kill: true }),
                Response::Ok
            );
            fixture.join();
        }
    }
}

#[test]
fn enum_edits_in_place_with_a_pick_list() {
    let (mut dashboard, _root, document) = editor("");
    select(&mut dashboard, "automatic_local_terminals");
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let text = screen(&dashboard);
    assert!(text.contains("Find an option:"), "{text}");
    assert!(focused_line_with(&dashboard, "default_branch_only").contains("default_branch_only"));
    dashboard.key(KeyCode::Up);
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("automatic_local_terminals", Some("\"off\""))
    );
    let text = screen(&dashboard);
    assert!(focused_line_with(&dashboard, "Automatic local terminals").contains("off"));
    assert!(text.contains("Source: dashboard.toml"), "{text}");
    assert!(text.contains("Default: default_branch_only"), "{text}");
}

#[test]
fn string_edits_are_prefilled_and_escape_cancels_only_that_edit() {
    let (mut dashboard, _root, document) = editor("# keep me\n");
    select(&mut dashboard, "branch_prefix");
    dashboard.key(KeyCode::Enter);
    assert!(
        screen(&dashboard).contains("Settings / Workspaces / Branch prefix"),
        "{}",
        screen(&dashboard)
    );
    assert!(rendered_footer(&dashboard, 120).contains("cancel"));
    dashboard.event_action(Event::Paste("zz".into()));
    assert!(
        screen(&dashboard).contains("feature/zz"),
        "the effective value is prefilled and edited in place:\n{}",
        screen(&dashboard)
    );
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    assert!(open(&dashboard), "Escape cancels the edit, not the view");
    assert!(focused_line_with(&dashboard, "Branch prefix").contains("Branch prefix"));
    let text = screen(&dashboard);
    assert!(text.contains("Value: feature/"), "{text}");
    assert!(!text.contains("feature/zz"), "{text}");
    assert!(dashboard.drain_outbox().is_empty());

    dashboard.key(KeyCode::Enter);
    for _ in 0.."feature/".len() {
        dashboard.key(KeyCode::Backspace);
    }
    for ch in "kh/".chars() {
        dashboard.key(KeyCode::Char(ch));
    }
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("branch_prefix", Some("\"kh/\""))
    );
    let text = screen(&dashboard);
    assert!(focused_line_with(&dashboard, "Branch prefix").contains("kh/"));
    assert!(text.contains("Source: dashboard.toml"), "{text}");
    assert!(text.contains("Default: feature/"), "{text}");
    let saved = std::fs::read_to_string(&document).unwrap();
    assert!(saved.contains("# keep me\n"), "{saved}");
    assert!(saved.contains("branch_prefix = \"kh/\"\n"), "{saved}");
}

/// A collection is one summary row; Enter opens its manager. Agent entries
/// keep their order while one entry is edited, added and removed.
#[test]
fn collection_manager_edits_agents_without_reordering_neighbors() {
    let (mut dashboard, _root, document) = editor(
        "[[agents]]\nname = \"claude\"\nargv = [\"claude\"]\n\n[launch_choices.demo]\nkind = \"Terminal\"\n",
    );
    select(&mut dashboard, "agent overrides");
    let summary = focused_line_with(&dashboard, "Agent overrides");
    assert!(
        summary.contains("1 entry"),
        "the collection is one summary row:\n{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Enter);
    assert!(
        screen(&dashboard).contains("Settings / Agents / Agent overrides"),
        "{}",
        screen(&dashboard)
    );
    assert!(focused_line_with(&dashboard, "claude").contains("claude"));

    // argv is edited as a TOML array literal.
    dashboard.key(KeyCode::Enter);
    for _ in 0..20 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("[\"claude\", \"--verbose\"]".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "agents[0].argv");
    assert_eq!(value.as_deref(), Some("[\"claude\", \"--verbose\"]"));
    assert!(focused_line_with(&dashboard, "claude").contains("[\"claude\", \"--verbose\"]"));

    // Add appends at the end.
    dashboard.key(KeyCode::Char('a'));
    dashboard.event_action(Event::Paste("codex".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "agents[1]");
    assert_eq!(
        value.as_deref(),
        Some("{ name = \"codex\", argv = [\"codex\"] }")
    );
    let text = screen(&dashboard);
    assert!(
        text.find("claude").unwrap() < text.find("codex").unwrap(),
        "adding must not reorder entries:\n{text}"
    );

    // Remove takes one entry; the other survives in order.
    focus_row(&mut dashboard, "codex");
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char('x'));
    assert_eq!((path.as_str(), value), ("agents[1]", None));
    let text = screen(&dashboard);
    assert!(!text.contains("codex"), "{text}");
    assert!(text.contains("claude"), "{text}");
    assert_eq!(
        dashboard_reading(&document).settings.agents.len(),
        1,
        "one entry was removed"
    );
    dashboard.key(KeyCode::Esc);
    assert!(focused_line_with(&dashboard, "Agent overrides").contains("1 entry"));
    assert!(
        std::fs::read_to_string(&document)
            .unwrap()
            .contains("[launch_choices.demo]"),
        "unrelated entries survive"
    );
}

/// An open Add-agent draft must append to the reading published after it
/// opened, preserving agents added elsewhere instead of overwriting one.
#[test]
fn add_agent_draft_appends_after_an_external_reading() {
    let original = "[[agents]]\nname = \"claude\"\nargv = [\"claude\"]\n";
    let (mut dashboard, _root, document) = editor(original);
    select(&mut dashboard, "agent overrides");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Char('a'));
    assert!(
        screen(&dashboard).contains("Add agent"),
        "the draft names the collection:\n{}",
        screen(&dashboard)
    );

    let external = format!(
        "{original}\n[[agents]]\nname = \"codex\"\nargv = [\"codex\"]\n\n[launch_choices.demo]\nkind = \"Terminal\"\n"
    );
    std::fs::write(&document, &external).unwrap();
    publish(&mut dashboard, &document);
    dashboard.event_action(Event::Paste("new".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert!(path.starts_with("agents"), "{path}");
    assert!(value.is_some(), "{value:?}");
    let reading = dashboard_reading(&document);
    assert_eq!(
        reading
            .settings
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect::<Vec<_>>(),
        ["claude", "codex", "new"],
        "the draft appends after externally added agents"
    );
    assert!(
        reading.settings.launch_choices.contains_key("demo"),
        "unrelated settings survive the append"
    );
}

/// A saved sound remains the picker's canonical selection while its row reads
/// inactive; Enter saves that sound, not the default option.
#[cfg(target_os = "macos")]
#[test]
fn inactive_saved_sound_picker_highlights_the_saved_choice() {
    for (choice, label, contents) in [
        ("tap", "Tap", "ready_sound_choice = \"tap\"\n"),
        (
            "chime",
            "Chime",
            "desktop_notifications = true\nready_sound_choice = \"chime\"\n",
        ),
        ("rise", "Rise", "ready_sound_choice = \"rise\"\n"),
    ] {
        let (mut dashboard, _root, document) = editor(contents);
        select(&mut dashboard, "ready_sound_choice");
        assert!(
            focused_line_with(&dashboard, "Ready sound choice").contains("inactive"),
            "{contents}"
        );
        assert!(dashboard.drain_outbox().is_empty());
        dashboard.key(KeyCode::Enter);
        assert!(
            focused_line_with(&dashboard, label).contains(label),
            "the saved {label} choice stays highlighted:\n{}",
            screen(&dashboard)
        );
        assert!(
            !screen(&dashboard).contains("› System default"),
            "the default must not replace the saved choice:\n{}",
            screen(&dashboard)
        );
        let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
        assert_eq!(path, "ready_sound_choice");
        let expected = format!("\"{choice}\"");
        assert_eq!(value.as_deref(), Some(expected.as_str()));
        assert!(
            focused_line_with(&dashboard, "Ready sound choice").contains(label),
            "{contents}"
        );
    }
}

/// Empty collection drafts and collection-boundary actions leave visible
/// feedback and never submit a setting.
#[test]
fn collection_drafts_and_boundaries_report_feedback_without_writes() {
    let (mut dashboard, _root, document) = editor("");
    let before = std::fs::read_to_string(&document).unwrap();
    select(&mut dashboard, "agent overrides");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    assert!(
        dashboard.drain_outbox().is_empty(),
        "an empty add must not write"
    );
    assert_eq!(std::fs::read_to_string(&document).unwrap(), before);
    let text = screen(&dashboard).to_lowercase();
    assert!(
        text.contains("name") && (text.contains("saving") || text.contains("save")),
        "the empty add explains what is missing:\n{}",
        screen(&dashboard)
    );

    let (mut dashboard, _root, document) = editor("picker_roots = [\"/a\", \"/b\"]\n");
    select(&mut dashboard, "picker_roots");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "/a");
    dashboard.key(KeyCode::Char('['));
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        screen(&dashboard)
            .to_lowercase()
            .contains("first picker root"),
        "the first-root boundary is reported:\n{}",
        screen(&dashboard)
    );
    focus_row(&mut dashboard, "/b");
    dashboard.key(KeyCode::Char(']'));
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        screen(&dashboard)
            .to_lowercase()
            .contains("last picker root"),
        "the last-root boundary is reported:\n{}",
        screen(&dashboard)
    );
    assert_eq!(
        dashboard_reading(&document).settings.picker_roots,
        [PathBuf::from("/a"), PathBuf::from("/b")]
    );

    let (mut dashboard, _root, document) = editor("");
    select(&mut dashboard, "picker_roots");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Char('r'));
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        screen(&dashboard)
            .to_lowercase()
            .contains("already its default"),
        "resetting an inherited collection is reported:\n{}",
        screen(&dashboard)
    );
    assert_eq!(
        std::fs::read_to_string(&document).unwrap(),
        "",
        "a default collection reset writes nothing"
    );
}

/// Two queued agent edits share the parent report row. A reading that changed
/// only the first must leave the second pending, so its refusal stays visible.
#[test]
fn queued_agent_edits_keep_their_own_reply_association() {
    let contents = "\
[[agents]]
name = \"claude\"
argv = [\"claude\"]

[[agents]]
name = \"codex\"
argv = [\"codex\"]
";
    let (mut dashboard, _root, document) = editor(contents);
    select(&mut dashboard, "agent overrides");
    dashboard.key(KeyCode::Enter);

    focus_row(&mut dashboard, "claude");
    dashboard.key(KeyCode::Enter);
    for _ in 0..24 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("[\"claude\", \"--verbose\"]".into()));
    dashboard.key(KeyCode::Enter);
    let first = only_request(dashboard.drain_outbox());

    focus_row(&mut dashboard, "codex");
    dashboard.key(KeyCode::Enter);
    for _ in 0..24 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("[\"codex\", \"--verbose\"]".into()));
    dashboard.key(KeyCode::Enter);
    let second = only_request(dashboard.drain_outbox());
    assert_ne!(first.request_id, second.request_id);

    // The first Server write updates only claude; codex is still in flight.
    let first_saved = "\
[[agents]]
name = \"claude\"
argv = [\"claude\", \"--verbose\"]

[[agents]]
name = \"codex\"
argv = [\"codex\"]
";
    std::fs::write(&document, first_saved).unwrap();
    publish(&mut dashboard, &document);
    focus_row(&mut dashboard, "codex");
    assert!(
        focused_line_with(&dashboard, "codex").contains("saving…"),
        "the second queued edit is still pending:\n{}",
        screen(&dashboard)
    );

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::Error {
            code: ErrorCode::InvalidRequest,
            message: "fixture second agent refused".into(),
        },
    });
    assert!(
        screen(&dashboard).contains("fixture second agent refused"),
        "the second reply keeps its own visible refusal:\n{}",
        screen(&dashboard)
    );
    assert!(
        !focused_line_with(&dashboard, "codex").contains("saving…"),
        "the refused edit stops pending"
    );
    assert_eq!(std::fs::read_to_string(&document).unwrap(), first_saved);
}

/// A valid TOML integer with separators is published as its canonical value;
/// the matching acknowledgement must clear the pending state.
#[test]
fn typed_integer_normalization_clears_pending_after_confirmation() {
    let (mut dashboard, _root, document) = editor("");
    select(&mut dashboard, "delegation.max_live_children");
    assert!(
        focused_line_with(&dashboard, "Maximum live children").contains("Maximum live children")
    );
    dashboard.key(KeyCode::Enter);
    for _ in 0..20 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("1_0".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "delegation.max_live_children");
    assert_eq!(value.as_deref(), Some("1_0"));
    let reading = dashboard_reading(&document);
    assert_eq!(reading.settings.delegation.max_live_children, 10);
    let row = focused_line_with(&dashboard, "Maximum live children");
    assert!(row.contains("10"), "{row}");
    assert!(!row.contains("saving…"), "{row}");
    assert!(
        screen(&dashboard).contains("Value: 10"),
        "the published canonical value is shown:\n{}",
        screen(&dashboard)
    );
}

/// Notification and iTerm permission consequences are readable before the
/// user commits, on both compact wide and single-pane frames.
#[cfg(target_os = "macos")]
#[test]
fn alert_permission_consequences_are_visible_before_activation() {
    let (mut dashboard, _root, _document) = menu_editor();
    dashboard.key(KeyCode::Enter);
    for (cols, rows) in [(80, 24), (60, 24)] {
        let text = flatten_frame(rendered_rows(&dashboard, cols, rows));
        assert!(
            text.contains("permission prompt"),
            "notification permission consequence must be visible at {cols}x{rows}:\n{text}"
        );
    }
    focus_row(&mut dashboard, "Exact iTerm focus");
    for (cols, rows) in [(80, 24), (60, 24)] {
        let text = flatten_frame(rendered_rows(&dashboard, cols, rows));
        assert!(
            text.contains("automation permission"),
            "iTerm permission consequence must be visible at {cols}x{rows}:\n{text}"
        );
    }
    assert!(dashboard.drain_outbox().is_empty());
}

/// An unparseable reading stops a collection draft from enqueueing a write;
/// a valid reading restores collection controls.
#[test]
fn unparseable_reading_blocks_manager_drafts_until_recovery() {
    let valid = "[[agents]]\nname = \"claude\"\nargv = [\"claude\"]\n";
    let (mut dashboard, _root, document) = editor(valid);
    select(&mut dashboard, "agent overrides");
    dashboard.key(KeyCode::Enter);

    let invalid = "not toml {\n";
    std::fs::write(&document, invalid).unwrap();
    publish(&mut dashboard, &document);
    assert!(
        screen(&dashboard).contains("cannot be parsed"),
        "{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Char('a'));
    dashboard.event_action(Event::Paste("new".into()));
    dashboard.key(KeyCode::Enter);
    assert!(
        dashboard.drain_outbox().is_empty(),
        "an unparseable reading cannot enqueue a collection write"
    );
    assert_eq!(std::fs::read_to_string(&document).unwrap(), invalid);
    assert!(screen(&dashboard).contains("cannot be parsed"));

    std::fs::write(&document, valid).unwrap();
    publish(&mut dashboard, &document);
    assert!(
        !screen(&dashboard).contains("cannot be parsed"),
        "{}",
        screen(&dashboard)
    );
    if screen(&dashboard).contains("Settings / Agents / + Add agent") {
        dashboard.key(KeyCode::Esc);
    }
    assert!(
        screen(&dashboard).contains("Settings / Agents / Agent overrides"),
        "{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Char('a'));
    dashboard.event_action(Event::Paste("new".into()));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let message = only_request(dashboard.drain_outbox());
    let (path, _) = answer(&mut dashboard, &document, message);
    assert!(path.starts_with("agents"), "{path}");
    assert!(
        dashboard_reading(&document)
            .settings
            .agents
            .iter()
            .any(|agent| agent.name == "new"),
        "recovered controls add the agent"
    );
}

/// Picker roots are ordered; reordering and removing one entry write the whole
/// list, and resetting the collection writes None only when it is overridden.
#[test]
fn picker_roots_manager_preserves_order_and_distinguishes_reset() {
    let (mut dashboard, _root, document) = editor("picker_roots = [\"/a\", \"/b\", \"/c\"]\n");
    select(&mut dashboard, "picker_roots");
    assert!(focused_line_with(&dashboard, "Picker roots").contains("3 entries"));
    dashboard.key(KeyCode::Enter);
    assert!(
        screen(&dashboard).contains("Settings / Workspaces / Picker roots"),
        "{}",
        screen(&dashboard)
    );
    assert!(screen(&dashboard).contains("+ Add folder"));
    focus_row(&mut dashboard, "/b");
    assert!(
        screen(&dashboard).contains("x removes this entry"),
        "the manager distinguishes one entry from the collection:\n{}",
        screen(&dashboard)
    );
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char(']'));
    assert_eq!(path, "picker_roots");
    assert_eq!(value.as_deref(), Some("[\"/a\", \"/c\", \"/b\"]"));
    let text = screen(&dashboard);
    assert!(
        text.find("/a").unwrap() < text.find("/c").unwrap()
            && text.find("/c").unwrap() < text.find("/b").unwrap(),
        "reordering preserves the neighbors:\n{text}"
    );

    focus_row(&mut dashboard, "/b");
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char('x'));
    assert_eq!(path, "picker_roots[2]");
    assert_eq!(value, None);
    let text = screen(&dashboard);
    assert!(!text.contains("/b"), "{text}");
    assert!(text.contains("/a") && text.contains("/c"), "{text}");
    let focus = manager_focus(&dashboard);
    assert!(
        ["/a", "/c", "+ Add folder"]
            .iter()
            .any(|row| focus.starts_with(*row)),
        "removing one entry leaves a real row focused:\n{text}"
    );

    // Reset from the open manager; the published default list must leave a
    // rendered manager row focused even though the old selected id is gone.
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char('r'));
    assert_eq!((path.as_str(), value), ("picker_roots", None));
    let defaults = dashboard_reading(&document).settings.picker_roots;
    let default_roots: Vec<String> = defaults
        .iter()
        .map(|root| root.display().to_string())
        .collect();
    let focus = manager_focus(&dashboard);
    assert!(
        focus.starts_with("+ Add folder")
            || default_roots.iter().any(|root| focus.starts_with(root)),
        "reset leaves a real manager row focused: {focus:?}"
    );

    dashboard.key(KeyCode::Esc);
    let summary = focused_line_with(&dashboard, "Picker roots");
    assert!(summary.contains(&count_label(defaults.len())), "{summary}");
    let text = screen(&dashboard);
    assert!(text.contains("Source: default"), "{text}");
    assert!(!text.contains("Reset:"), "{text}");
}

/// A published collection reading that removes the selected entry leaves the
/// open manager on a real rendered row, not on a details-only fallback.
#[test]
fn external_collection_reading_leaves_visible_manager_focus() {
    let (mut dashboard, _root, document) = editor("picker_roots = [\"/a\", \"/b\"]\n");
    select(&mut dashboard, "picker_roots");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "/b");
    assert!(focused_line_with(&dashboard, "/b").contains("/b"));

    std::fs::write(&document, "picker_roots = []\n").unwrap();
    publish(&mut dashboard, &document);
    assert!(
        screen(&dashboard).contains("Settings / Workspaces / Picker roots"),
        "{}",
        screen(&dashboard)
    );
    assert_eq!(
        manager_focus(&dashboard),
        "+ Add folder",
        "the emptied manager visibly focuses its only row:\n{}",
        screen(&dashboard)
    );
    assert!(!screen(&dashboard).contains("/b"));
}

/// Editing an existing picker root starts from the complete folder path, so a
/// suffix edit preserves the rest of the path and its neighboring roots.
#[test]
fn picker_root_edit_prefills_the_full_path_and_keeps_neighbors() {
    let (mut dashboard, _root, document) = editor("picker_roots = [\"/a\", \"/b\"]\n");
    select(&mut dashboard, "picker_roots");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "/a");
    dashboard.key(KeyCode::Enter);
    assert!(
        screen(&dashboard).contains("/a"),
        "the path editor starts from the full value:\n{}",
        screen(&dashboard)
    );
    dashboard.event_action(Event::Paste("-new".into()));
    assert!(
        screen(&dashboard).contains("/a-new"),
        "the suffix appends to the full path:\n{}",
        screen(&dashboard)
    );
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "picker_roots[0]");
    assert_eq!(value.as_deref(), Some("\"/a-new\""));
    assert_eq!(
        dashboard_reading(&document).settings.picker_roots,
        vec![PathBuf::from("/a-new"), PathBuf::from("/b")]
    );
    let text = screen(&dashboard);
    assert!(
        text.find("/a-new").unwrap() < text.find("/b").unwrap(),
        "the neighboring root keeps its place:\n{text}"
    );
}

/// Adding a root to the default list publishes a document override immediately:
/// the open manager then shows its source and reset action, and resetting keeps
/// unrelated keys and comments.
#[test]
fn adding_root_to_default_list_publishes_source_and_reset_keeps_unrelated() {
    let (mut dashboard, root, document) = editor("# keep\nbranch_prefix = \"keep/\"\n");
    let defaults = dashboard_reading(&document).settings.picker_roots;
    let added = root.path().display().to_string();
    select(&mut dashboard, "picker_roots");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Char('a'));
    dashboard.event_action(Event::Paste(added.clone()));
    let (path, _) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "picker_roots");
    let mut expected = defaults.clone();
    expected.push(root.path().to_path_buf());
    assert_eq!(dashboard_reading(&document).settings.picker_roots, expected);

    let text = screen(&dashboard);
    assert!(
        text.contains("Source: dashboard.toml"),
        "the open manager adopts the published source:\n{text}"
    );
    assert!(
        text.contains("Reset:"),
        "the open manager offers collection reset:\n{text}"
    );

    dashboard.key(KeyCode::Esc);
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char('r'));
    assert_eq!((path.as_str(), value), ("picker_roots", None));
    assert_eq!(dashboard_reading(&document).settings.picker_roots, defaults);
    let saved = std::fs::read_to_string(&document).unwrap();
    assert!(saved.contains("# keep\n"), "{saved}");
    assert!(saved.contains("branch_prefix = \"keep/\""), "{saved}");
    assert!(!saved.contains("picker_roots"), "{saved}");
}

/// An empty collection still occupies one summary row and opens a manager that
/// offers its add action.
#[test]
fn empty_collection_keeps_one_summary_row_and_opens_its_manager() {
    let (mut dashboard, _root, _document) = editor("picker_roots = []\n");
    select(&mut dashboard, "picker_roots");
    let summary = focused_line_with(&dashboard, "Picker roots");
    assert!(summary.contains("0 entries"), "{summary}");
    assert!(!screen(&dashboard).contains("+ Add folder"));
    dashboard.key(KeyCode::Enter);
    assert!(
        screen(&dashboard).contains("Settings / Workspaces / Picker roots"),
        "{}",
        screen(&dashboard)
    );
    assert!(
        focused_line_with(&dashboard, "+ Add folder").contains("+ Add folder"),
        "{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Esc);
    assert!(focused_line_with(&dashboard, "Picker roots").contains("0 entries"));
}

/// While `picker_roots` is its default, no list is in the document, so an
/// added root writes the default list with the new root appended.
#[test]
fn adding_to_a_default_list_keeps_its_default_elements() {
    let (mut dashboard, root, document) = editor("");
    let defaults = dashboard_reading(&document).settings.picker_roots;
    let added = root.path().display().to_string();
    select(&mut dashboard, "picker_roots");
    let summary = focused_line_with(&dashboard, "Picker roots");
    assert!(
        summary.contains(&count_label(defaults.len())),
        "the default collection is still one summary row:\n{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Char('a'));
    dashboard.event_action(Event::Paste(added.clone()));
    let (path, _) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "picker_roots");
    let mut expected = defaults;
    expected.push(root.path().to_path_buf());
    assert_eq!(dashboard_reading(&document).settings.picker_roots, expected);
    dashboard.key(KeyCode::Esc);
    assert!(focused_line_with(&dashboard, "Picker roots").contains(&count_label(expected.len())));
}

fn dashboard_reading(document: &Path) -> SettingsReport {
    ovrcr::settings::parse(
        document,
        &std::fs::read_to_string(document).unwrap_or_default(),
    )
}

#[test]
fn reset_is_offered_only_for_a_value_the_document_sets() {
    let (mut dashboard, _root, document) =
        editor("# top\nbranch_prefix = \"kh/\"\nready_sound = true\n");
    select(&mut dashboard, "title_model");
    let text = screen(&dashboard);
    assert!(text.contains("Source: default"), "{text}");
    assert!(!text.contains("Reset:"), "{text}");
    assert_eq!(dashboard.key(KeyCode::Char('r')), DashboardAction::Redraw);
    assert!(
        dashboard.drain_outbox().is_empty(),
        "reset must not write for a default value"
    );
    assert!(screen(&dashboard).contains("default"));

    select(&mut dashboard, "branch_prefix");
    let scope = screen(&dashboard)
        .lines()
        .find(|line| line.contains("Reset:"))
        .unwrap_or_else(|| panic!("{}", screen(&dashboard)))
        .to_owned();
    assert!(
        scope.contains("feature/"),
        "reset names the value it falls back to: {scope}"
    );
    assert!(
        rendered_footer(&dashboard, 120).contains("r reset"),
        "the focused override offers reset"
    );
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char('r'));
    assert_eq!((path.as_str(), value), ("branch_prefix", None));
    let text = screen(&dashboard);
    assert!(focused_line_with(&dashboard, "Branch prefix").contains("feature/"));
    assert!(text.contains("Source: default"), "{text}");
    assert!(!text.contains("Reset:"), "{text}");
    assert_eq!(
        std::fs::read_to_string(&document).unwrap(),
        "# top\nready_sound = true\n"
    );
}

/// An explicit Off and an inherited Off share the row value but differ in
/// source, reset and default detail.
#[test]
fn explicit_and_inherited_off_are_distinguishable() {
    let (mut dashboard, _root, document) = editor("");
    select(&mut dashboard, "desktop_notifications");
    let text = screen(&dashboard);
    assert!(focused_line_with(&dashboard, "Desktop notifications").contains("off"));
    assert!(text.contains("Source: default"), "{text}");
    assert!(text.contains("Default: off"), "{text}");
    assert!(
        !rendered_footer(&dashboard, 120).contains("r reset"),
        "an inherited value offers no reset:\n{text}"
    );

    std::fs::write(&document, "desktop_notifications = false # mine\n").unwrap();
    publish(&mut dashboard, &document);
    let text = screen(&dashboard);
    assert!(focused_line_with(&dashboard, "Desktop notifications").contains("off"));
    assert!(text.contains("Source: dashboard.toml"), "{text}");
    assert!(text.contains("Default: off"), "{text}");
    assert!(
        rendered_footer(&dashboard, 120).contains("r reset"),
        "an explicit Off offers reset:\n{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn a_rejected_value_shows_in_details_and_leaves_the_document_unchanged() {
    let contents = "title_model = \"anthropic/claude\"\n";
    let (mut dashboard, _root, document) = editor(contents);
    select(&mut dashboard, "title_model");
    dashboard.key(KeyCode::Enter);
    for _ in 0..40 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("no-slash".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("title_model", Some("\"no-slash\""))
    );
    assert_eq!(std::fs::read_to_string(&document).unwrap(), contents);
    let text = screen(&dashboard);
    assert!(
        text.contains("refused: could not save title_model"),
        "{text}"
    );
    assert!(
        focused_line_with(&dashboard, "Title model").contains("anthropic/claude"),
        "the effective value remains the Server's reading:\n{text}"
    );
    assert!(
        !rendered_footer(&dashboard, 120).contains("could not save"),
        "the refusal belongs to the setting, not the footer"
    );
}

#[test]
fn an_unparseable_document_disables_editing_and_recovers_on_a_valid_reading() {
    let (mut dashboard, _root, document) = editor("ready_sound = true\nnot toml {\n");
    let text = screen(&dashboard);
    assert!(
        text.contains("! dashboard.toml cannot be parsed · editing unavailable"),
        "{text}"
    );
    assert!(text.contains("Editing is off until dashboard.toml is fixed by hand."));
    dashboard.key(KeyCode::Char('d'));
    let text = screen(&dashboard);
    assert!(text.contains("Settings / Document"), "{text}");
    assert!(text.contains("Findings"), "{text}");
    assert!(
        text.contains("not valid TOML") || text.contains("cannot be parsed"),
        "{text}"
    );
    dashboard.key(KeyCode::Esc);
    select(&mut dashboard, "ready_sound");
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Char('r')), DashboardAction::Redraw);
    assert!(
        dashboard.drain_outbox().is_empty(),
        "invalid documents cannot submit edits"
    );

    std::fs::write(&document, "ready_sound = true\n").unwrap();
    publish(&mut dashboard, &document);
    let text = screen(&dashboard);
    assert!(
        !text.contains("cannot be parsed") && !text.contains("Editing is off"),
        "a valid reading restores the editor:\n{text}"
    );
    select(&mut dashboard, "desktop_notifications");
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("desktop_notifications", Some("true"))
    );
}

/// Global search matches labels and keys across categories, names the owning
/// category, and opening a result does not edit or write.
#[test]
fn global_search_round_trips_to_a_setting_and_back() {
    let (mut dashboard, _root, _document) = menu_editor();
    dashboard.key(KeyCode::Char('/'));
    assert!(
        screen(&dashboard).contains("Settings / Search"),
        "{}",
        screen(&dashboard)
    );
    dashboard.event_action(Event::Paste("home".into()));
    let text = screen(&dashboard);
    assert!(text.contains("Search: home"), "{text}");
    assert!(text.contains("› Usage · Codex home"), "{text}");
    assert!(
        text.contains("  Usage · Grok home"),
        "both categories and rows are named:\n{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());

    // Revising the query replaces the result list.
    for _ in 0.."home".len() {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("theme".into()));
    assert!(
        screen(&dashboard).contains("› Appearance · Theme"),
        "{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Enter);
    assert!(
        focused_line_with(&dashboard, "Theme").contains("Theme"),
        "{}",
        screen(&dashboard)
    );
    assert!(screen(&dashboard).contains("Settings / Appearance"));
    assert!(
        dashboard.drain_outbox().is_empty(),
        "opening a result focuses the row, it does not edit"
    );
    dashboard.key(KeyCode::Esc);
    assert!(focused_row(&dashboard).contains("Alerts"));
}

/// An empty result explains recovery, cannot submit, and Esc restores the
/// category the search started from.
#[test]
fn empty_global_search_explains_recovery_and_writes_nothing() {
    let (mut dashboard, _root, _document) = menu_editor();
    focus_category(&mut dashboard, "Appearance");
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("zzzz".into()));
    let text = screen(&dashboard);
    assert!(text.contains("No settings match \"zzzz\"."), "{text}");
    assert!(
        text.contains("Backspace to revise · Esc to cancel"),
        "{text}"
    );
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(!screen(&dashboard).contains("Theme"));
    assert!(dashboard.drain_outbox().is_empty());
    for _ in 0..4 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("branch".into()));
    assert!(
        screen(&dashboard).contains("Workspaces · Branch prefix"),
        "{}",
        screen(&dashboard)
    );
    dashboard.key(KeyCode::Esc);
    assert!(focused_row(&dashboard).contains("Appearance"));
    assert!(dashboard.drain_outbox().is_empty());
}

/// Backing out of a search-opened list restores the exact prior category, row
/// and scroll position.
#[test]
fn backing_out_of_search_restores_the_prior_list_row_and_scroll() {
    let (mut dashboard, _root, _document) = menu_editor();
    focus_category(&mut dashboard, "Usage");
    dashboard.key(KeyCode::Enter);
    focus_row(&mut dashboard, "Cursor state database");
    let before = rendered_rows(&dashboard, 120, 12);
    let detail_start = before
        .iter()
        .position(|line| line.contains("Source:"))
        .expect("details");
    let list_before = before[1..detail_start].to_vec();
    assert!(
        list_before
            .iter()
            .any(|line| line.contains("› Cursor state database")),
        "{before:?}"
    );

    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("branch_prefix".into()));
    dashboard.key(KeyCode::Enter);
    assert!(focused_line_with(&dashboard, "Branch prefix").contains("Branch prefix"));
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);

    assert!(
        screen(&dashboard).contains("• Usage"),
        "search restores the prior category:\n{}",
        screen(&dashboard)
    );
    let after = rendered_rows(&dashboard, 120, 12);
    let detail_start = after
        .iter()
        .position(|line| line.contains("Source:"))
        .expect("details");
    assert_eq!(
        after[1..detail_start].to_vec(),
        list_before,
        "search must restore the prior row and scroll:\n{after:?}"
    );
    assert!(
        focused_line_with(&dashboard, "Cursor state database").contains("Cursor state database")
    );
    assert!(dashboard.drain_outbox().is_empty());
}

/// Ordinary characters stay in a text draft; global search only starts from
/// navigation, and picker filtering is a local query.
#[test]
fn editor_typing_and_picker_filtering_stay_distinct_from_global_search() {
    let (mut dashboard, _root, _document) = editor("");
    select(&mut dashboard, "branch_prefix");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("home".into()));
    let text = screen(&dashboard);
    assert!(text.contains("/home"), "{text}");
    assert!(!text.contains("Search:"), "{text}");
    assert!(
        text.contains("Settings / Workspaces / Branch prefix"),
        "{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Esc);

    focus_category(&mut dashboard, "Appearance");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("solar".into()));
    let text = screen(&dashboard);
    assert!(text.contains("Find a theme: solar"), "{text}");
    assert!(text.contains("of 80 themes"), "{text}");
    assert!(
        !text.contains("Search: solar"),
        "the picker filter is not global search:\n{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Esc);
    assert!(
        focused_line_with(&dashboard, "Theme").contains("Theme"),
        "{}",
        screen(&dashboard)
    );
}

/// A remembered launch edits kind and preset together in its manager.
#[test]
fn remembered_launch_picks_kind_and_preset_together() {
    let (mut dashboard, _root, document) =
        editor("[launch_choices.demo]\nkind = \"Agent\"\npreset = \"fixture\"\n");
    select(&mut dashboard, "launch_choices");
    let summary = rendered_rows(&dashboard, 120, 60)
        .into_iter()
        .find(|line| line.contains("› Remembered launches") && line.contains("1 entry"))
        .unwrap_or_else(|| panic!("{}", screen(&dashboard)));
    assert!(summary.contains("1 entry"));
    dashboard.key(KeyCode::Enter);
    assert!(
        screen(&dashboard).contains("Settings / Remembered launches / Remembered launches"),
        "{}",
        screen(&dashboard)
    );
    assert!(focused_line_with(&dashboard, "demo").contains("Agent: fixture"));
    dashboard.key(KeyCode::Enter);
    let text = screen(&dashboard);
    assert!(text.contains("Find an option:"), "{text}");
    assert!(focused_line_with(&dashboard, "Agent: fixture").contains("Agent: fixture"));
    dashboard.event_action(Event::Paste("terminal".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "launch_choices.demo");
    assert_eq!(value.as_deref(), Some("{ kind = \"Terminal\" }"));
    assert!(focused_line_with(&dashboard, "demo").contains("Terminal"));
    let saved = std::fs::read_to_string(&document).unwrap();
    assert!(!saved.contains("preset"), "{saved}");
}
