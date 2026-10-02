//! The Server's settings reading: what the Dashboard adopts, the footer line,
//! the read-only Settings popup, and the save requests toggles send.

use crate::*;
use ovrcr::protocol::{
    SettingOwner, SettingRow, SettingSource, Settings, SettingsFinding, SettingsReport,
};

fn report(path: PathBuf, findings: usize) -> SettingsReport {
    SettingsReport {
        path,
        read_unix_ms: 0,
        settings: Settings {
            ready_sound: true,
            branch_prefix: "fix/".into(),
            ..Default::default()
        },
        rows: vec![
            SettingRow {
                key: "ready_sound".into(),
                owner: SettingOwner::Dashboard,
                value: Some("true".into()),
                source: SettingSource::Document,
                off_state: None,
            },
            SettingRow {
                key: "title_model".into(),
                owner: SettingOwner::Server,
                value: None,
                source: SettingSource::Default,
                off_state: Some("Titles off: set `title_model` in dashboard.toml".into()),
            },
        ],
        findings: (0..findings)
            .map(|index| SettingsFinding {
                key: Some(format!("unknown_{index}")),
                message: "unknown key".into(),
                line: Some(index as u32 + 3),
            })
            .collect(),
    }
}

fn publish(dashboard: &mut Dashboard, report: SettingsReport) {
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
        Box::new(report),
    )));
}

fn screen(dashboard: &Dashboard) -> String {
    rendered_rows(dashboard, 120, 40).join("\n")
}

#[test]
fn settings_footer_reports_findings_at_attach_and_when_the_count_changes() {
    let mut quiet = dashboard_fixture();
    publish(&mut quiet, report("/d.toml".into(), 0));
    assert!(!rendered_footer(&quiet, 120).contains("settings finding"));

    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, report("/d.toml".into(), 2));
    assert!(
        rendered_footer(&dashboard, 120).contains("2 settings findings; see Settings"),
        "{}",
        rendered_footer(&dashboard, 120)
    );
    // Any key clears the notice; the same count does not bring it back.
    dashboard.key(KeyCode::Char('j'));
    publish(&mut dashboard, report("/d.toml".into(), 2));
    assert!(!rendered_footer(&dashboard, 120).contains("settings finding"));
    publish(&mut dashboard, report("/d.toml".into(), 1));
    assert!(rendered_footer(&dashboard, 120).contains("1 settings finding; see Settings"));
    publish(&mut dashboard, report("/d.toml".into(), 0));
    assert!(rendered_footer(&dashboard, 120).contains("No settings findings"));
}

#[test]
fn settings_popup_opens_from_the_palette_with_findings_first() {
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, report("/srv/dashboard.toml".into(), 1));
    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste("settings".into()));
    dashboard.key(KeyCode::Enter);
    let text = screen(&dashboard);
    assert!(text.contains("Settings · read-only"), "{text}");
    assert!(text.contains("Document: /srv/dashboard.toml"), "{text}");
    let finding = text
        .find("line 3, unknown_0: unknown key")
        .expect("finding row");
    let row = text
        .find("ready_sound = true  (Dashboard, document)")
        .expect("setting row");
    assert!(finding < row, "findings come before settings:\n{text}");
    assert!(text.contains("title_model = unset  (Server, default)"));
    assert!(text.contains("Titles off: set `title_model` in dashboard.toml"));
    dashboard.key(KeyCode::Esc);
    assert!(!screen(&dashboard).contains("Settings · read-only"));
}

#[test]
fn settings_popup_opens_from_the_menu_and_has_no_browse_key() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(','));
    assert!(!screen(&dashboard).contains("Settings · read-only"));
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('v'));
    assert!(screen(&dashboard).contains(",  Settings"));
    dashboard.key(KeyCode::Char(','));
    let text = screen(&dashboard);
    assert!(text.contains("Settings · read-only"), "{text}");
    assert!(text.contains("Waiting for the Server's settings reading."));
}

#[test]
fn settings_toggle_asks_the_server_and_applies_only_its_reading() {
    let root = tempfile::tempdir().unwrap();
    let published = root.path().join("published.toml");
    std::fs::write(&published, "ready_sound = true # mine\n").unwrap();
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, report(published.clone(), 0));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Char('S')) else {
        panic!("S sent no request");
    };
    assert_eq!(
        message.request,
        ovrcr::protocol::Request::SetSetting {
            path: "ready_sound".into(),
            value: Some("false".into()),
        }
    );
    assert_eq!(
        std::fs::read_to_string(&published).unwrap(),
        "ready_sound = true # mine\n",
        "the Dashboard touches no file"
    );
    assert!(!rendered_footer(&dashboard, 120).contains("Ready sound: off"));
    let mut saved = report(published, 0);
    saved.settings.ready_sound = false;
    publish(&mut dashboard, saved);
    assert!(rendered_footer(&dashboard, 120).contains("Ready sound: off"));
}
