//! Settings lifecycle against a real `ovrcr` (item 5, tier 2).
//!
//! Each case uses a private HOME and no `OVRCR_HOME`, so the default instance
//! directory is the one under test. Fixture binaries stand in for providers.

#[path = "support/live.rs"]
mod live;

use live::claude_auth;

use crossterm::event::{Event, KeyCode};
use ovrcr::protocol::{
    CLAUDE_WAITING, ClientMessage, ErrorCode, QuotaSnapshot, QuotaState, Request, Response,
    ServerEvent, ServerMessage, SettingRow, SettingSource, SettingsReport, TerminalSize, ThemeId,
    connect_server, read_frame, write_frame,
};
use ovrcr::server::ServerPaths;
use ovrcr::service::{ServiceCommand, ServiceConfig, ServicePlatform, run_with};
use ovrcr::settings;
use ovrcr::tui::{Dashboard, DashboardAction, draw_dashboard_at};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn fresh_install_reports_every_default_and_matches_the_cli_byte_for_byte() {
    let live = fresh();
    live.start_binary();
    // A reachable startup listener does not prove the registry is initialized.
    // fresh() bounds this public request, admitted only after initialization.
    assert!(
        matches!(live.request(Request::List), Response::Hierarchy(_)),
        "the fresh Server did not admit its initial hierarchy request"
    );
    // The socket is bound before storage opens; the greeting proves readiness.
    let mut socket = attach(&live);
    let home = live.home.clone().unwrap();
    let directory = home.join(live::DEFAULT_CONFIG_DIR);
    assert!(
        directory.join("registry.sqlite3").is_file(),
        "the Server did not start on the default instance directory"
    );

    let (cli_stdout, cli) = cli_settings(&live);
    assert_eq!(cli.path, directory.join("dashboard.toml"));
    assert!(cli.findings.is_empty(), "{:?}", cli.findings);
    assert!(!cli.unparseable);
    assert!(
        cli.rows
            .iter()
            .all(|row| row.source == SettingSource::Default),
        "{:?}",
        cli.rows
    );
    let roots = row(&cli, "picker_roots");
    assert_eq!(
        roots.value.as_deref(),
        Some(format!("[{}]", toml::Value::String(home.display().to_string())).as_str())
    );

    let hello = settings_event(&mut socket);
    // Both loads stamp the clock. The reading itself is the bytes.
    assert_eq!(
        align_read_time(&cli_stdout, hello.read_unix_ms),
        serde_json::to_string(&hello).unwrap(),
        "hello snapshot and `ovrcr settings --json` disagree"
    );
}

#[test]
fn settings_set_survives_a_restart_and_changes_nothing_else() {
    let live = fresh();
    live.start_binary();
    let before = cli_settings(&live).1;

    let set = live
        .command()
        .args(["settings", "set", "branch_prefix", "kept/"])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    restart(&live);

    let mut socket = attach(&live);
    let hello = settings_event(&mut socket);
    let after = cli_settings(&live).1;
    assert_eq!(hello.settings.branch_prefix, "kept/");
    assert_eq!(row(&hello, "branch_prefix").source, SettingSource::Document);
    assert_eq!(row(&hello, "branch_prefix").value.as_deref(), Some("kept/"));
    assert_eq!(
        align_read_time(&serde_json::to_string(&after).unwrap(), hello.read_unix_ms),
        serde_json::to_string(&hello).unwrap()
    );
    for row in &hello.rows {
        if row.key == "branch_prefix" {
            continue;
        }
        assert_eq!(
            row,
            before_row(&before, &row.key),
            "restart changed {}",
            row.key
        );
    }
}

#[test]
fn delegation_policy_is_published_and_survives_server_restart() {
    let live = fresh();
    live.start_binary();
    for (path, value) in [
        ("delegation.progress_deadline_ms", "12345"),
        ("delegation.enabled", "false"),
        ("delegation.allowed_harnesses", "[\"pi\", \"codex\"]"),
        ("delegation.allowed_models", "[\"test/model\"]"),
        ("delegation.default_harness", "pi"),
        ("delegation.default_model", "test/model"),
        ("delegation.max_live_children", "6"),
        ("delegation.max_execution_attempts", "2"),
    ] {
        let output = live
            .command()
            .args(["settings", "set", path, value])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let before = settings_event(&mut attach(&live));
    assert!(before.findings.is_empty(), "{:?}", before.findings);
    assert_eq!(before.settings.delegation.progress_deadline_ms, 12345);
    assert!(!before.settings.delegation.enabled);
    assert_eq!(before.settings.delegation.max_live_children, 6);
    assert_eq!(before.settings.delegation.max_execution_attempts, Some(2));
    assert_eq!(
        before.settings.delegation.default_model.as_deref(),
        Some("test/model")
    );
    restart(&live);
    let after = settings_event(&mut attach(&live));
    assert_eq!(after.settings.delegation.progress_deadline_ms, 12345);
    assert_eq!(after.settings.delegation, before.settings.delegation);
    for row in after
        .rows
        .iter()
        .filter(|row| row.key.starts_with("delegation."))
    {
        assert_eq!(row.source, SettingSource::Document, "{}", row.key);
        assert_eq!(row, before_row(&before, &row.key), "{}", row.key);
    }
    let cli = cli_settings(&live).1;
    assert_eq!(cli.settings.delegation, after.settings.delegation);
    let output = live
        .command()
        .args(["settings", "reset", "delegation.max_execution_attempts"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reset = settings_event(&mut attach(&live));
    assert_eq!(reset.settings.delegation.max_execution_attempts, None);
    assert_eq!(reset.settings.delegation.max_live_children, 6);
}

#[test]
fn quota_enabled_flipped_in_the_file_starts_and_stops_the_collector() {
    let live = fresh();
    let script = codex_fixture_script(&live);
    let pids = live.root.path().join("native-pids");
    let document = settings_document(&live);
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    let quota = |enabled: bool| {
        format!(
            "[quota]\nenabled = {enabled}\n[quota.codex]\ncommand = {}\n[quota.grok]\ncommand = '/no/such-grok-fixture'\n",
            toml::Value::String(script.display().to_string())
        )
    };
    std::fs::write(&document, quota(false)).unwrap();
    live.start_binary_env(&[("OVRCR_QUOTA_FIXTURE_PIDS", pids.as_os_str())]);
    let mut socket = attach(&live);
    // Off at hello is the Dashboard's own default, so no row is published yet.
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        !pids.exists(),
        "disabled quota spawned a collector: {}",
        std::fs::read_to_string(&pids).unwrap_or_default()
    );

    std::fs::write(&document, quota(true)).unwrap();
    let enabled = quota_until(
        &mut socket,
        "enabling quota did not reach the Dashboard",
        |snapshot| snapshot.codex.state == QuotaState::Current,
    );
    assert_ne!(enabled.codex.state, QuotaState::Disabled);
    let first = newest_pid(&pids);
    assert!(alive(first), "collector exited before quota was turned off");

    std::fs::write(&document, quota(false)).unwrap();
    let disabled = quota_until(
        &mut socket,
        "disabling quota did not reach the Dashboard",
        |snapshot| {
            snapshot.codex.state == QuotaState::Disabled
                && snapshot.grok.state == QuotaState::Disabled
        },
    );
    assert_eq!(
        disabled.codex.reason.as_deref(),
        Some(ovrcr::settings::QUOTA_OFF)
    );
    wait_until("disabling quota did not stop the collector", || {
        !alive(first)
    });
}

#[test]
fn dashboard_shows_the_server_reading_when_the_client_cannot_read_the_document() {
    let live = fresh();
    let document = settings_document(&live);
    live.start_binary();
    // The instance identity appears as the Server starts. Write after that so
    // the watcher's stamp is the readable file, then drop the mode. Mode does
    // not change length or mtime, so a later attach still gets this reading.
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(&document, "branch_prefix = \"parity/\"\n").unwrap();
    let mut socket = attach(&live);
    let hello = settings_until(
        &mut socket,
        "the Server never read the document",
        |report| report.settings.branch_prefix == "parity/",
    );
    assert_eq!(hello.path, document);
    assert_eq!(row(&hello, "branch_prefix").source, SettingSource::Document);
    drop(socket);

    std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(
        std::fs::read_to_string(&document).is_err(),
        "the client could still read the document"
    );
    let mut socket = attach(&live);
    let again = settings_event(&mut socket);
    assert_eq!(again.path, document);
    assert_eq!(again.settings.branch_prefix, "parity/");
    assert_eq!(row(&again, "branch_prefix").source, SettingSource::Document);
    assert!(again.findings.is_empty(), "{:?}", again.findings);

    let cli = cli_settings(&live).1;
    assert_ne!(cli.settings.branch_prefix, "parity/");
    assert!(
        cli.findings
            .iter()
            .any(|finding| finding.message.contains("cannot read")),
        "the client's own load hid the unreadable document: {cli:?}"
    );
    std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn dashboard_theme_save_and_cancel_round_trip_through_the_server() {
    let live = fresh();
    let document = settings_document(&live);
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(
        &document,
        "# keep this comment\nbranch_prefix = \"kept/\"\n",
    )
    .unwrap();
    live.start_binary();

    let (mut attached, initial) = attach_dashboard(&live);
    assert_eq!(initial.path, document);
    assert_eq!(initial.settings.theme, ThemeId::Dark);
    assert_eq!(initial.settings.branch_prefix, "kept/");
    assert_eq!(row(&initial, ThemeId::KEY).source, SettingSource::Default);
    assert_eq!(
        row(&initial, ThemeId::KEY).value.as_deref(),
        Some("catppuccin-mocha")
    );
    assert!(initial.findings.is_empty(), "{:?}", initial.findings);

    // Menu -> Settings opens category focus on Alerts; Down reveals Appearance.
    open_settings(&mut attached);
    assert_eq!(
        attached.dashboard.key(KeyCode::Down),
        DashboardAction::Redraw
    );
    assert!(
        focused_dashboard_line(&attached.dashboard).contains("Appearance"),
        "{}",
        dashboard_text(&attached.dashboard)
    );
    attached.dashboard.key(KeyCode::Enter);
    focus_settings_row(&mut attached.dashboard, "Theme");
    attached.dashboard.key(KeyCode::Enter);
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("Dark"), "{text}");
    assert!(text.contains("Saved: Catppuccin Mocha"), "{text}");
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Catppuccin Mocha").contains("Saved"),
        "the saved palette must open focused and labelled:\n{text}"
    );
    assert!(
        attached.dashboard.drain_outbox().is_empty(),
        "opening the theme picker wrote"
    );

    // End reaches the Light group without forcing heading into the first page.
    attached.dashboard.key(KeyCode::End);
    let text = dashboard_text(&attached.dashboard);
    assert!(
        text.contains("Light"),
        "End must reach the Light group:\n{text}"
    );
    assert!(
        text.contains("Preview:"),
        "End must preview another palette:\n{text}"
    );
    assert!(
        attached.dashboard.drain_outbox().is_empty(),
        "paging and previewing wrote"
    );

    // Home restores the saved first palette without submitting it.
    attached.dashboard.key(KeyCode::Home);
    let text = dashboard_text(&attached.dashboard);
    assert!(
        text.contains("Saved: Catppuccin Mocha"),
        "Home must restore the saved palette:\n{text}"
    );
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Catppuccin Mocha").contains("Saved"),
        "Home must leave the saved palette focused:\n{text}"
    );
    assert!(
        attached.dashboard.drain_outbox().is_empty(),
        "restoring the saved preview wrote"
    );

    attached
        .dashboard
        .event_action(Event::Paste("Solarized Light".into()));
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("› Solarized Light"), "{text}");
    assert!(
        text.contains("Preview: Solarized Light · Saved: Catppuccin Mocha"),
        "{text}"
    );
    assert!(
        attached.dashboard.drain_outbox().is_empty(),
        "filtering and previewing wrote"
    );

    attached.dashboard.key(KeyCode::Enter);
    assert!(
        dashboard_text(&attached.dashboard).contains("saving…"),
        "Enter must show a pending save:\n{}",
        dashboard_text(&attached.dashboard)
    );
    let save = take_dashboard_request(&mut attached.dashboard);
    let request_id = save.request_id;
    let request = save.request;
    let Request::SetSetting { path, value } = request.clone() else {
        panic!("Enter must send SetSetting, got {request:?}");
    };
    assert_eq!(path, ThemeId::KEY);
    assert_eq!(value.as_deref(), Some("\"solarized-light\""));
    let observed = attached.drive(vec![ClientMessage {
        request_id,
        request,
    }]);
    assert!(
        observed.iter().any(|message| matches!(
            message,
            ServerMessage::Response {
                request_id: id,
                response: Response::Ok,
            } if *id == request_id
        )),
        "the Server refused the theme save: {observed:?}"
    );
    let saved = observed
        .iter()
        .rev()
        .find_map(settings_report_of)
        .filter(|report| report.settings.theme == ThemeId::SolarizedLight)
        .unwrap_or_else(|| {
            attached.await_settings("the theme save was not published", |report| {
                report.settings.theme == ThemeId::SolarizedLight
            })
        });
    assert_eq!(saved.path, document);
    assert_eq!(saved.settings.theme, ThemeId::SolarizedLight);
    assert_eq!(saved.settings.branch_prefix, "kept/");
    assert_eq!(row(&saved, ThemeId::KEY).source, SettingSource::Document);
    assert_eq!(
        row(&saved, ThemeId::KEY).value.as_deref(),
        Some("solarized-light")
    );
    assert!(saved.findings.is_empty(), "{:?}", saved.findings);
    assert!(
        !dashboard_text(&attached.dashboard).contains("saving…"),
        "the published reading must end the pending state"
    );

    let persisted = std::fs::read_to_string(&document).unwrap();
    assert!(persisted.contains("# keep this comment"), "{persisted}");
    assert!(
        persisted.contains("theme = \"solarized-light\""),
        "{persisted}"
    );
    let from_file = settings::parse(&document, &persisted);
    assert_eq!(from_file.settings.theme, ThemeId::SolarizedLight);
    assert_eq!(from_file.settings.branch_prefix, "kept/");
    assert_eq!(
        row(&from_file, ThemeId::KEY).source,
        SettingSource::Document
    );
    assert!(from_file.findings.is_empty(), "{:?}", from_file.findings);

    // Reopen: the picker starts on the Server's saved theme.
    close_settings(&mut attached);
    open_settings(&mut attached);
    open_theme_picker(&mut attached);
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("Saved: Solarized Light"), "{text}");
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Solarized Light").contains("Saved"),
        "the reopened picker must start on the saved palette:\n{text}"
    );

    let persisted = std::fs::read_to_string(&document).unwrap();
    attached.dashboard.key(KeyCode::Down);
    let preview = dashboard_line(&attached.dashboard, "Preview:");
    assert!(preview.contains("Saved: Solarized Light"), "{preview}");
    assert!(!preview.contains("Preview: Solarized Light"), "{preview}");
    assert!(
        attached.dashboard.drain_outbox().is_empty(),
        "highlighting another palette wrote"
    );
    attached.dashboard.key(KeyCode::Esc);
    let text = dashboard_text(&attached.dashboard);
    assert!(
        !text.contains("Preview:"),
        "Esc must cancel the preview:\n{text}"
    );
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Theme").contains("Theme"),
        "cancel must return to the Theme row:\n{text}"
    );
    assert!(
        text.contains("Solarized Light") || text.contains("solarized-light"),
        "the saved value must remain after cancel:\n{text}"
    );
    assert_eq!(
        std::fs::read_to_string(&document).unwrap(),
        persisted,
        "canceling a preview changed the document"
    );
    assert!(attached.dashboard.drain_outbox().is_empty());
    close_settings(&mut attached);
    drop(attached);

    // The Server reading survives a restart with the same private document.
    restart(&live);
    let mut socket = attach(&live);
    let hello = settings_event(&mut socket);
    assert_eq!(hello.path, document);
    assert_eq!(hello.settings.theme, ThemeId::SolarizedLight);
    assert_eq!(hello.settings.branch_prefix, "kept/");
    assert_eq!(row(&hello, ThemeId::KEY).source, SettingSource::Document);
    assert_eq!(
        row(&hello, ThemeId::KEY).value.as_deref(),
        Some("solarized-light")
    );
    assert!(hello.findings.is_empty(), "{:?}", hello.findings);
    assert!(
        std::fs::read_to_string(&document)
            .unwrap()
            .contains("# keep this comment")
    );
}

#[test]
fn dashboard_same_document_theme_save_settles_without_a_publish() {
    let live = fresh();
    let document = settings_document(&live);
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(&document, "# kept\ntheme = \"nord\"\n").unwrap();
    live.start_binary();

    let (mut attached, initial) = attach_dashboard(&live);
    assert_eq!(initial.path, document);
    assert_eq!(initial.settings.theme, ThemeId::Nord);
    assert_eq!(row(&initial, ThemeId::KEY).source, SettingSource::Document);
    assert_eq!(row(&initial, ThemeId::KEY).value.as_deref(), Some("nord"));
    assert!(initial.findings.is_empty(), "{:?}", initial.findings);

    open_settings(&mut attached);
    open_theme_picker(&mut attached);
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("Saved: Nord"), "{text}");
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Nord").contains("Saved"),
        "the documented theme must open focused:\n{text}"
    );
    assert!(attached.dashboard.drain_outbox().is_empty());

    // Enter on the already-documented choice still submits its canonical ID.
    attached.dashboard.key(KeyCode::Enter);
    assert!(
        dashboard_text(&attached.dashboard).contains("saving…"),
        "Enter must show a pending save:\n{}",
        dashboard_text(&attached.dashboard)
    );
    let save = take_dashboard_request(&mut attached.dashboard);
    let request_id = save.request_id;
    let request = save.request;
    let Request::SetSetting { path, value } = request.clone() else {
        panic!("Enter must send SetSetting, got {request:?}");
    };
    assert_eq!(path, ThemeId::KEY);
    assert_eq!(value.as_deref(), Some("\"nord\""));
    let observed = attached.drive(vec![ClientMessage {
        request_id,
        request,
    }]);
    assert!(
        observed.iter().any(|message| matches!(
            message,
            ServerMessage::Response {
                request_id: id,
                response: Response::Ok,
            } if *id == request_id
        )),
        "the Server refused the unchanged save: {observed:?}"
    );
    assert!(
        observed
            .iter()
            .all(|message| settings_report_of(message).is_none()),
        "an unchanged reading must not be republished: {observed:?}"
    );
    assert!(
        !dashboard_text(&attached.dashboard).contains("saving…"),
        "the ack must settle against the confirmed reading without a report:\n{}",
        dashboard_text(&attached.dashboard)
    );
    assert!(attached.dashboard.drain_outbox().is_empty());

    close_settings(&mut attached);
    open_settings(&mut attached);
    open_theme_picker(&mut attached);
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("Saved: Nord"), "{text}");
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Nord").contains("Saved"),
        "reopening must still show the documented theme:\n{text}"
    );
    assert!(attached.dashboard.drain_outbox().is_empty());
    close_settings(&mut attached);
    drop(attached);

    // The private document and the Server reading still agree.
    let mut socket = attach(&live);
    let hello = settings_event(&mut socket);
    assert_eq!(hello.path, document);
    assert_eq!(hello.settings.theme, ThemeId::Nord);
    assert_eq!(row(&hello, ThemeId::KEY).source, SettingSource::Document);
    assert_eq!(row(&hello, ThemeId::KEY).value.as_deref(), Some("nord"));
    assert!(hello.findings.is_empty(), "{:?}", hello.findings);
    let persisted = std::fs::read_to_string(&document).unwrap();
    assert!(persisted.contains("# kept"), "{persisted}");
    let from_file = settings::parse(&document, &persisted);
    assert_eq!(from_file.settings.theme, ThemeId::Nord);
    assert_eq!(
        row(&from_file, ThemeId::KEY).source,
        SettingSource::Document
    );
}

#[test]
fn dashboard_default_theme_save_waits_for_the_document_override_reading() {
    let live = fresh();
    let document = settings_document(&live);
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(&document, "# kept\nbranch_prefix = \"kept/\"\n").unwrap();
    live.start_binary();

    let (mut attached, initial) = attach_dashboard(&live);
    assert_eq!(initial.settings.theme, ThemeId::Dark);
    assert_eq!(row(&initial, ThemeId::KEY).source, SettingSource::Default);
    assert_eq!(
        row(&initial, ThemeId::KEY).value.as_deref(),
        Some("catppuccin-mocha")
    );
    assert_eq!(
        row(&initial, ThemeId::KEY).default.as_deref(),
        Some("catppuccin-mocha")
    );
    assert!(initial.findings.is_empty(), "{:?}", initial.findings);

    open_settings(&mut attached);
    open_theme_picker(&mut attached);
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("Saved: Catppuccin Mocha"), "{text}");
    assert!(attached.dashboard.drain_outbox().is_empty());

    // The effective value equals the default, but the write creates a
    // document override: its changed source must arrive as a reading.
    attached.dashboard.key(KeyCode::Enter);
    assert!(
        dashboard_text(&attached.dashboard).contains("saving…"),
        "Enter must show a pending save:\n{}",
        dashboard_text(&attached.dashboard)
    );
    let save = take_dashboard_request(&mut attached.dashboard);
    let request_id = save.request_id;
    let request = save.request;
    let Request::SetSetting { path, value } = request.clone() else {
        panic!("Enter must send SetSetting, got {request:?}");
    };
    assert_eq!(path, ThemeId::KEY);
    assert_eq!(value.as_deref(), Some("\"catppuccin-mocha\""));
    let observed = attached.drive(vec![ClientMessage {
        request_id,
        request,
    }]);
    assert!(
        observed.iter().any(|message| matches!(
            message,
            ServerMessage::Response {
                request_id: id,
                response: Response::Ok,
            } if *id == request_id
        )),
        "the Server refused the default override: {observed:?}"
    );
    let saved = observed
        .iter()
        .rev()
        .find_map(settings_report_of)
        .filter(|report| {
            report.settings.theme == ThemeId::Dark
                && row(report, ThemeId::KEY).source == SettingSource::Document
        })
        .unwrap_or_else(|| {
            attached.await_settings("the document override was not published", |report| {
                report.settings.theme == ThemeId::Dark
                    && row(report, ThemeId::KEY).source == SettingSource::Document
            })
        });
    assert_eq!(saved.settings.theme, ThemeId::Dark);
    assert_eq!(row(&saved, ThemeId::KEY).source, SettingSource::Document);
    assert_eq!(
        row(&saved, ThemeId::KEY).value.as_deref(),
        Some("catppuccin-mocha")
    );
    assert_eq!(saved.settings.branch_prefix, "kept/");
    assert!(saved.findings.is_empty(), "{:?}", saved.findings);
    assert!(
        !dashboard_text(&attached.dashboard).contains("saving…"),
        "the published override must end the pending state:\n{}",
        dashboard_text(&attached.dashboard)
    );
    assert!(attached.dashboard.drain_outbox().is_empty());

    let persisted = std::fs::read_to_string(&document).unwrap();
    assert!(persisted.contains("# kept"), "{persisted}");
    let from_file = settings::parse(&document, &persisted);
    assert_eq!(from_file.settings.theme, ThemeId::Dark);
    assert_eq!(
        row(&from_file, ThemeId::KEY).source,
        SettingSource::Document
    );
    assert_eq!(from_file.settings.branch_prefix, "kept/");
    assert!(from_file.findings.is_empty(), "{:?}", from_file.findings);

    close_settings(&mut attached);
    open_settings(&mut attached);
    open_theme_picker(&mut attached);
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("Saved: Catppuccin Mocha"), "{text}");
    assert!(attached.dashboard.drain_outbox().is_empty());
    close_settings(&mut attached);
}

#[test]
fn dashboard_picker_root_reorder_survives_reopen_with_unrelated_values() {
    let live = fresh();
    let document = settings_document(&live);
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(
        &document,
        "# kept\npicker_roots = [\"/fixture/one\", \"/fixture/two\", \"/fixture/three\"] # roots\nbranch_prefix = \"kept/\" # mine\n",
    )
    .unwrap();
    live.start_binary();

    let (mut attached, initial) = attach_dashboard(&live);
    let original = picker_roots(&["/fixture/one", "/fixture/two", "/fixture/three"]);
    assert_eq!(initial.path, document);
    assert_eq!(initial.settings.picker_roots, original);
    assert_eq!(initial.settings.branch_prefix, "kept/");
    assert_eq!(
        row(&initial, "picker_roots").source,
        SettingSource::Document
    );
    assert!(initial.findings.is_empty(), "{:?}", initial.findings);

    open_settings(&mut attached);
    assert_eq!(
        attached.dashboard.key(KeyCode::Down),
        DashboardAction::Redraw
    );
    assert_eq!(
        attached.dashboard.key(KeyCode::Down),
        DashboardAction::Redraw
    );
    assert!(
        focused_dashboard_line(&attached.dashboard).contains("Workspaces"),
        "{}",
        dashboard_text(&attached.dashboard)
    );
    attached.dashboard.key(KeyCode::Enter);
    focus_settings_row(&mut attached.dashboard, "Picker roots");
    let text = dashboard_text(&attached.dashboard);
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Picker roots").contains("3 entries"),
        "the collection summary must name its size:\n{text}"
    );
    attached.dashboard.key(KeyCode::Enter);
    let text = dashboard_text(&attached.dashboard);
    assert!(text.contains("Workspaces / Picker roots"), "{text}");
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "/fixture/one").contains("/fixture/one"),
        "the manager must open on its first entry:\n{text}"
    );
    assert_paths_in_order(
        &attached.dashboard,
        &["/fixture/one", "/fixture/two", "/fixture/three"],
    );

    // "]" moves the selected root later and writes the whole list once.
    attached.dashboard.key(KeyCode::Char(']'));
    let reorder = take_dashboard_request(&mut attached.dashboard);
    let request_id = reorder.request_id;
    let request = reorder.request;
    let Request::SetSetting { path, value } = request.clone() else {
        panic!("] must send SetSetting, got {request:?}");
    };
    assert_eq!(path, "picker_roots");
    let value = value.expect("reordering writes the whole root list");
    let document_with_roots: toml::Value = toml::from_str(&format!("roots = {value}")).unwrap();
    assert_eq!(
        document_with_roots.get("roots"),
        Some(&toml::Value::Array(
            ["/fixture/two", "/fixture/one", "/fixture/three"]
                .into_iter()
                .map(|path| toml::Value::String(path.into()))
                .collect()
        ))
    );
    let observed = attached.drive(vec![ClientMessage {
        request_id,
        request,
    }]);
    assert!(
        observed.iter().any(|message| matches!(
            message,
            ServerMessage::Response {
                request_id: id,
                response: Response::Ok,
            } if *id == request_id
        )),
        "the Server refused the reorder: {observed:?}"
    );
    let reordered = picker_roots(&["/fixture/two", "/fixture/one", "/fixture/three"]);
    let saved = observed
        .iter()
        .rev()
        .find_map(settings_report_of)
        .filter(|report| report.settings.picker_roots == reordered)
        .unwrap_or_else(|| {
            attached.await_settings("the reorder was not published", |report| {
                report.settings.picker_roots == reordered
            })
        });
    assert_eq!(saved.path, document);
    assert_eq!(saved.settings.branch_prefix, "kept/");
    assert_eq!(row(&saved, "picker_roots").source, SettingSource::Document);
    assert!(saved.findings.is_empty(), "{:?}", saved.findings);
    assert_paths_in_order(
        &attached.dashboard,
        &["/fixture/two", "/fixture/one", "/fixture/three"],
    );

    attached.dashboard.key(KeyCode::Esc);
    assert!(
        focused_dashboard_line_with(&attached.dashboard, "Picker roots").contains("3 entries"),
        "Esc must return to the Picker roots summary:\n{}",
        dashboard_text(&attached.dashboard)
    );
    close_settings(&mut attached);

    let persisted = std::fs::read_to_string(&document).unwrap();
    assert!(persisted.contains("# kept"), "{persisted}");
    assert!(persisted.contains("# roots"), "{persisted}");
    assert!(persisted.contains("# mine"), "{persisted}");
    let from_file = settings::parse(&document, &persisted);
    assert_eq!(from_file.settings.picker_roots, reordered);
    assert_eq!(
        row(&from_file, "picker_roots").source,
        SettingSource::Document
    );
    assert_eq!(from_file.settings.branch_prefix, "kept/");
    assert!(from_file.findings.is_empty(), "{:?}", from_file.findings);
    assert_paths_ordered(
        &persisted,
        &["/fixture/two", "/fixture/one", "/fixture/three"],
    );
    drop(attached);

    // A fresh Dashboard sees the Server's stored reading, and reopening the
    // manager shows the new order with every neighboring root intact.
    let (mut reopened, reread) = attach_dashboard(&live);
    assert_eq!(reread.settings.picker_roots, reordered);
    assert_eq!(reread.settings.branch_prefix, "kept/");
    assert_eq!(row(&reread, "picker_roots").source, SettingSource::Document);
    assert!(reread.findings.is_empty(), "{:?}", reread.findings);
    for (before, after) in initial.rows.iter().zip(reread.rows.iter()) {
        if before.key == "picker_roots" {
            continue;
        }
        assert_eq!(before, after, "reopening changed {}", before.key);
    }
    open_settings(&mut reopened);
    focus_settings_category(&mut reopened, "Workspaces");
    reopened.dashboard.key(KeyCode::Enter);
    focus_settings_row(&mut reopened.dashboard, "Picker roots");
    assert!(
        focused_dashboard_line_with(&reopened.dashboard, "Picker roots").contains("3 entries"),
        "the reopened collection summary must still name three entries:\n{}",
        dashboard_text(&reopened.dashboard)
    );
    reopened.dashboard.key(KeyCode::Enter);
    assert!(dashboard_text(&reopened.dashboard).contains("Workspaces / Picker roots"));
    assert_paths_in_order(
        &reopened.dashboard,
        &["/fixture/two", "/fixture/one", "/fixture/three"],
    );
    assert!(reopened.dashboard.drain_outbox().is_empty());
    close_settings(&mut reopened);
}

#[test]
fn service_document_is_the_servers_and_never_the_clients() {
    let live = fresh();
    let home = live.home.clone().unwrap();
    let server_config = live.root.path().join("server-instance");
    std::fs::create_dir_all(&server_config).unwrap();
    let client_document = live.root.path().join("client-dashboard.toml");
    std::fs::write(&client_document, "branch_prefix = \"client-side\"\n").unwrap();
    let server_document = server_config.join("dashboard.toml");

    let definition = live.root.path().join("service-definition");
    let manager = live.root.path().join("service-manager");
    let pidfile = live.root.path().join("service.pid");
    let manager_log = live.root.path().join("service-manager.log");
    let server_log = live.root.path().join("service-server.log");
    write_service_manager(
        &manager,
        &definition,
        &live.executable,
        &pidfile,
        &manager_log,
        &server_log,
    );

    let config = service_config(
        &definition,
        &manager,
        &live.executable,
        &server_config,
        &live.socket,
        &home,
    );
    // A manager that cannot be executed fails the case with the reason. It is
    // not skipped.
    let mut missing = config.clone();
    missing.manager_executable = PathBuf::from("/no/such/service-manager");
    missing.definition_path = live.root.path().join("missing-manager-definition");
    let error = run_with(
        &missing,
        ServiceCommand::Install {
            environment_file: None,
            kill_sessions: false,
        },
        false,
    )
    .expect_err("an unavailable service manager must fail the case");
    let reason = format!("{error:#}");
    assert!(
        reason.contains("No such file or directory") || reason.contains("service manager"),
        "{reason}"
    );

    let _stop = ServiceStop {
        config: config.clone(),
        pidfile: pidfile.clone(),
    };
    run_with(
        &config,
        ServiceCommand::Install {
            environment_file: None,
            kill_sessions: false,
        },
        false,
    )
    .unwrap_or_else(|error| panic!("service manager unavailable: {error:#}"));
    assert!(
        live::wait_for_socket(&live.socket, Duration::from_secs(5)),
        "service did not start the Server\nmanager:\n{}\ndefinition:\n{}\nserver:\n{}",
        std::fs::read_to_string(&manager_log).unwrap_or_default(),
        std::fs::read_to_string(&definition).unwrap_or_default(),
        std::fs::read_to_string(&server_log).unwrap_or_default()
    );
    let definition_text = std::fs::read_to_string(&definition).unwrap();
    assert!(
        definition_text.contains(&server_config.display().to_string()),
        "{definition_text}"
    );
    assert!(!definition_text.contains(&client_document.display().to_string()));

    let mut socket = attach(&live);
    let hello = settings_event(&mut socket);
    assert_eq!(hello.path, server_document);
    assert_eq!(hello.settings.branch_prefix, "feature/");

    let client_view = cli_settings_with(
        &live,
        &[("OVRCR_DASHBOARD_CONFIG", client_document.as_os_str())],
    );
    assert_eq!(client_view.path, client_document);
    assert_eq!(client_view.settings.branch_prefix, "client-side");

    let set = live
        .command()
        .env("OVRCR_DASHBOARD_CONFIG", &client_document)
        .args(["settings", "set", "branch_prefix", "from-dashboard"])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let updated = settings_event(&mut socket);
    assert_eq!(updated.path, server_document);
    assert_eq!(updated.settings.branch_prefix, "from-dashboard");
    let server_text = std::fs::read_to_string(&server_document).unwrap();
    assert!(
        server_text.contains("from-dashboard"),
        "the Server document was not written: {server_text}"
    );
    assert_eq!(
        std::fs::read_to_string(&client_document).unwrap(),
        "branch_prefix = \"client-side\"\n",
        "a client OVRCR_DASHBOARD_CONFIG was written"
    );
}

#[test]
fn account_change_moves_the_codex_and_claude_rows_without_restart() {
    let live = fresh();
    live.claude.set(true, "claude.ai", Some("max"));
    let calls = auth_call_log(&live);
    let script = codex_fixture_script(&live);
    let pids = live.root.path().join("native-pids");
    live.start_binary_env(&[
        ("OVRCR_QUOTA_FIXTURE_PIDS", pids.as_os_str()),
        ("OVRCR_QUOTA_FIXTURE_MODE", std::ffi::OsStr::new("switch")),
    ]);
    let pid = live.server_pid();
    let mut socket = attach(&live);
    // Signed in through claude.ai the row is the Dashboard's own default, so
    // nothing is published yet. `claude auth status` is asked again when a
    // quota setting changes, not on a timer (docs/provider-quota.md). Wait
    // until that startup check has run before flipping the fixture.
    wait_until("claude auth status was not asked", || {
        std::fs::read_to_string(&calls)
            .unwrap_or_default()
            .lines()
            .count()
            >= 1
    });
    live.claude.set(false, "none", None);
    set_setting(&live, "quota.codex.command", "codex-two");
    let signed_out = quota_until(
        &mut socket,
        "signing out did not move the Claude row",
        |snapshot| snapshot.claude.state == QuotaState::NotSignedIn,
    );
    assert_eq!(
        signed_out.claude.reason.as_deref(),
        Some("run claude auth login")
    );

    live.claude.set(true, "claude.ai", Some("max"));
    set_setting(&live, "quota.codex.command", "codex-three");
    quota_until(
        &mut socket,
        "signing back in did not move the Claude row",
        |snapshot| {
            snapshot.claude.state == QuotaState::Checking
                && snapshot.claude.reason.as_deref() == Some(CLAUDE_WAITING)
        },
    );

    // Codex A to B to A, the same fixture behaviour as
    // `account_a_to_b_to_a_during_native_read_cannot_publish_old_allowance`.
    set_setting(
        &live,
        "quota.codex.command",
        &toml::Value::String(script.display().to_string()).to_string(),
    );
    set_setting(&live, "quota.grok.command", "\"/no/such-grok-fixture\"");
    set_setting(&live, "quota.enabled", "true");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        assert!(Instant::now() < deadline, "account change was not fenced");
        if let ServerMessage::Event(ServerEvent::QuotaChanged(quota)) =
            read_frame::<ServerMessage>(&mut socket).unwrap()
        {
            assert_ne!(
                quota.codex.state,
                QuotaState::Current,
                "cross-generation result admitted"
            );
            if quota.codex.state == QuotaState::SourceConflict {
                assert!(quota.codex.windows.is_empty());
                assert!(quota.codex.checked_unix_ms.is_none());
                break;
            }
        }
    }
    assert_eq!(live.server_pid(), pid, "the Server restarted");
}

#[test]
fn production_defaults_match_the_docs_and_the_demo_only_by_its_allowlist() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("Code")).unwrap();
    std::fs::create_dir(home.path().join("src")).unwrap();
    let _home = HomeGuard::set(home.path());
    let block = documented_defaults();
    let documented = settings::parse(Path::new("dashboard.toml"), &block);
    assert!(
        documented.findings.is_empty(),
        "the documented default document is not valid defaults: {:?}",
        documented.findings
    );
    assert_eq!(documented.settings, settings::defaults());

    assert_demo_allowlist();
}

#[cfg(feature = "gui")]
fn assert_demo_allowlist() {
    use ovrcr::protocol::AutomaticLocalTerminals;
    assert_eq!(
        ovrcr::gui::DEMO_SETTINGS_ALLOWLIST,
        ["automatic_local_terminals", "quota"]
    );
    let production = settings::parse(Path::new("dashboard.toml"), "");
    let bare = settings::parse(
        Path::new("dashboard.toml"),
        &ovrcr::gui::demo_dashboard_document(None),
    );
    assert_eq!(
        bare.settings.automatic_local_terminals,
        AutomaticLocalTerminals::On
    );
    assert_only_allowlisted(&bare, &production);

    let fragment = "[quota]\nenabled = true\n[quota.codex]\ncommand = \"/opt/codex\"\n";
    let with_quota = settings::parse(
        Path::new("dashboard.toml"),
        &ovrcr::gui::demo_dashboard_document(Some(fragment)),
    );
    assert!(with_quota.settings.quota.enabled);
    assert_eq!(
        with_quota.settings.quota.codex.command,
        PathBuf::from("/opt/codex")
    );
    assert_only_allowlisted(&with_quota, &production);
}

#[cfg(not(feature = "gui"))]
fn assert_demo_allowlist() {
    // This binary was not built with the demo helper. The comparison still
    // reads the allowlist the demo writes, so a second constant cannot appear
    // unnoticed. `cargo test --all-features` checks the parsed documents.
    let source = include_str!("../src/gui.rs");
    let start = source
        .find("pub const DEMO_SETTINGS_ALLOWLIST")
        .expect("demo allowlist constant missing from src/gui.rs");
    let end = source[start..]
        .find(';')
        .map(|offset| start + offset)
        .unwrap();
    let declaration = &source[start..=end];
    assert!(
        declaration.contains("\"automatic_local_terminals\"") && declaration.contains("\"quota\""),
        "{declaration}"
    );
    assert_eq!(
        declaration.matches('"').count(),
        4,
        "the allowlist must name only automatic_local_terminals and quota: {declaration}"
    );
}

#[test]
fn claude_fixture_answers_auth_status_from_its_current_state() {
    let root = tempfile::tempdir().unwrap();
    let claude = claude_auth::ClaudeAuth::install(root.path());
    let status = || {
        let output = std::process::Command::new(&claude.command)
            .args(["auth", "status", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let state = |status: Value| {
        (
            status["loggedIn"].clone(),
            status["authMethod"].clone(),
            status["subscriptionType"].clone(),
        )
    };
    assert_eq!(
        state(status()),
        (true.into(), "claude.ai".into(), "max".into())
    );
    claude.set(false, "none", None);
    assert_eq!(state(status()), (false.into(), "none".into(), Value::Null));
    claude.set(true, "api_key", None);
    assert_eq!(
        state(status()),
        (true.into(), "api_key".into(), Value::Null)
    );

    let other = std::process::Command::new(&claude.command)
        .args(["-p", "hello"])
        .output()
        .unwrap();
    assert_eq!(other.status.code(), Some(2));
}

/// Codex/Grok stand-in. `OVRCR_QUOTA_FIXTURE_MODE=switch` emits account B then
/// A during the rate-limit read, which is the A to B to A case.
#[test]
#[ignore = "task-owned native RPC fixture process"]
fn native_quota_rpc_fixture() {
    let log = std::env::var_os("OVRCR_QUOTA_FIXTURE_LOG");
    if let Some(pids) = std::env::var_os("OVRCR_QUOTA_FIXTURE_PIDS") {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pids)
            .unwrap()
            .write_all(format!("{}\n", std::process::id()).as_bytes())
            .unwrap();
    }
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let method = request["method"].as_str().unwrap();
        if let Some(log) = &log {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .unwrap()
                .write_all(format!("{method}\n").as_bytes())
                .unwrap();
        }
        let Some(id) = request.get("id") else {
            continue;
        };
        let mode = std::env::var("OVRCR_QUOTA_FIXTURE_MODE").unwrap_or_default();
        if method == "account/rateLimits/read" && mode == "switch" {
            println!(
                "{}",
                json!({"method":"account/updated","params":{"account":"B"}})
            );
            println!(
                "{}",
                json!({"method":"account/updated","params":{"account":"A"}})
            );
        }
        let reset = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3_600;
        let result = match method {
            "initialize" => json!({"capabilities": {}}),
            "account/read" => {
                json!({"account": {"type":"chatgpt", "email":"fixture@example.invalid", "planType":"plus"}})
            }
            "account/rateLimits/read" => {
                json!({"rateLimits": {"limitId":"codex", "primary": {"usedPercent":67, "windowDurationMins":300, "resetsAt":reset},
                    "secondary": {"usedPercent":31, "windowDurationMins":10080, "resetsAt":reset + 86_400}}, "rateLimitsByLimitId":{}})
            }
            other => panic!("unexpected native method {other}"),
        };
        println!("{}", json!({"jsonrpc":"2.0", "id": id, "result": result}));
        std::io::stdout().flush().unwrap();
    }
}

fn fresh() -> live::Live {
    live::Live::isolated_home().bounded()
}

fn settings_document(live: &live::Live) -> PathBuf {
    live.config.join("dashboard.toml")
}

fn restart(live: &live::Live) {
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    assert!(
        live.join_within(Duration::from_secs(5)).is_finished(),
        "server did not stop for the restart"
    );
    live.start_binary();
}

fn cli_settings(live: &live::Live) -> (String, SettingsReport) {
    let output = live
        .command()
        .args(["settings", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout.clone()).unwrap();
    let report = serde_json::from_slice(&output.stdout).unwrap();
    (text, report)
}

fn cli_settings_with(
    live: &live::Live,
    environment: &[(&str, &std::ffi::OsStr)],
) -> SettingsReport {
    let output = live
        .command()
        .envs(environment.iter().copied())
        .args(["settings", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The CLI's JSON with its clock replaced by `read_unix_ms`. Re-parsing would
/// reorder keys, so this is a textual swap of the one timestamp field.
fn align_read_time(cli_stdout: &str, read_unix_ms: u64) -> String {
    let cli = cli_stdout.trim_end_matches('\n');
    let value: Value = serde_json::from_str(cli).unwrap();
    let stamped = value["read_unix_ms"].as_u64().unwrap();
    let needle = format!("\"read_unix_ms\":{stamped}");
    assert!(cli.contains(&needle), "timestamp not in CLI JSON");
    cli.replace(&needle, &format!("\"read_unix_ms\":{read_unix_ms}"))
}

fn row<'a>(report: &'a SettingsReport, key: &str) -> &'a SettingRow {
    report
        .rows
        .iter()
        .find(|row| row.key == key)
        .unwrap_or_else(|| panic!("no row {key}"))
}

fn before_row<'a>(report: &'a SettingsReport, key: &str) -> &'a SettingRow {
    row(report, key)
}

fn set_setting(live: &live::Live, path: &str, value: &str) {
    assert_eq!(
        live.request(Request::SetSetting {
            path: path.into(),
            value: Some(value.into()),
        }),
        Response::Ok,
        "set {path}"
    );
}

fn attach(live: &live::Live) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut socket = connect_server(&live.socket).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write_frame(
            &mut socket,
            &ClientMessage {
                request_id: 1,
                request: Request::DashboardHello,
            },
        )
        .unwrap();
        match read_frame::<ServerMessage>(&mut socket).unwrap() {
            ServerMessage::Response {
                response:
                    Response::Error {
                        code: ErrorCode::Conflict,
                        ..
                    },
                ..
            } => {
                assert!(Instant::now() < deadline, "dashboard slot was not released");
                std::thread::park_timeout(Duration::from_millis(10));
            }
            ServerMessage::Response {
                response: Response::Hierarchy(_),
                ..
            } => return socket,
            other => panic!("unexpected greeting: {other:?}"),
        }
    }
}

fn settings_event(socket: &mut UnixStream) -> SettingsReport {
    settings_until(socket, "no settings snapshot from the Server", |_| true)
}

fn settings_until(
    socket: &mut UnixStream,
    what: &str,
    mut done: impl FnMut(&SettingsReport) -> bool,
) -> SettingsReport {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "{what}");
        if let ServerMessage::Event(ServerEvent::SettingsChanged(report)) =
            read_frame::<ServerMessage>(socket).unwrap()
            && done(&report)
        {
            return *report;
        }
    }
}

fn quota_until(
    socket: &mut UnixStream,
    what: &str,
    mut done: impl FnMut(&QuotaSnapshot) -> bool,
) -> QuotaSnapshot {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        assert!(Instant::now() < deadline, "{what}");
        if let ServerMessage::Event(ServerEvent::QuotaChanged(snapshot)) =
            read_frame::<ServerMessage>(socket).unwrap()
            && done(&snapshot)
        {
            return *snapshot;
        }
    }
}

/// A Dashboard admitted on its own socket, with every request it sends routed
/// to the real Server under test and every reply and event returned to it.
struct AttachedDashboard {
    dashboard: Dashboard,
    connection: UnixStream,
}

impl AttachedDashboard {
    /// Write `initial` and every follow-up the Dashboard emits, then read until
    /// those requests have replies. Events seen on the way are also returned.
    fn drive(&mut self, initial: Vec<ClientMessage>) -> Vec<ServerMessage> {
        let mut awaiting = BTreeSet::new();
        for request in initial {
            assert!(
                awaiting.insert(request.request_id),
                "reused Dashboard request id {}",
                request.request_id
            );
            write_frame(&mut self.connection, &request).expect("send Dashboard request");
        }
        let mut observed = Vec::new();
        while !awaiting.is_empty() {
            let message =
                read_frame::<ServerMessage>(&mut self.connection).expect("read Dashboard response");
            let response_id = match &message {
                ServerMessage::Response { request_id, .. } => Some(*request_id),
                ServerMessage::Event(_) => None,
            };
            for followup in self.dashboard.handle_server_message(message.clone()) {
                assert!(
                    awaiting.insert(followup.request_id),
                    "reused Dashboard follow-up id {}",
                    followup.request_id
                );
                write_frame(&mut self.connection, &followup).expect("send Dashboard follow-up");
            }
            if let Some(request_id) = response_id {
                awaiting.remove(&request_id);
            }
            observed.push(message);
        }
        observed
    }

    fn await_settings(
        &mut self,
        what: &str,
        mut done: impl FnMut(&SettingsReport) -> bool,
    ) -> SettingsReport {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "{what}");
            let message = read_frame::<ServerMessage>(&mut self.connection)
                .expect("read a Dashboard settings event");
            let direct = settings_report_of(&message);
            let followups = self.dashboard.handle_server_message(message);
            let latest = if followups.is_empty() {
                None
            } else {
                self.drive(followups)
                    .iter()
                    .rev()
                    .find_map(settings_report_of)
            };
            if let Some(report) = latest
                && done(&report)
            {
                return report;
            }
            if let Some(report) = direct
                && done(&report)
            {
                return report;
            }
        }
    }
}

fn settings_report_of(message: &ServerMessage) -> Option<SettingsReport> {
    match message {
        ServerMessage::Event(ServerEvent::SettingsChanged(report)) => Some((**report).clone()),
        _ => None,
    }
}

/// Attach a Dashboard to the disposable Server, retrying while the previous
/// connection's dashboard slot is released, and return its first reading.
fn attach_dashboard(live: &live::Live) -> (AttachedDashboard, SettingsReport) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 40,
            cols: 120,
        });
        dashboard.install_config_dir(live.config.clone());
        let connection = connect_server(&live.socket).expect("connect Dashboard socket");
        connection
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        connection
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut attached = AttachedDashboard {
            dashboard,
            connection,
        };
        let greeting = attached.drive(vec![ClientMessage {
            request_id: 90_001,
            request: Request::DashboardHello,
        }]);
        if greeting.iter().any(|message| {
            matches!(
                message,
                ServerMessage::Response {
                    response: Response::Hierarchy(_),
                    ..
                }
            )
        }) {
            let report = greeting
                .iter()
                .rev()
                .find_map(settings_report_of)
                .unwrap_or_else(|| {
                    attached.await_settings("the Server published no settings reading", |_| true)
                });
            return (attached, report);
        }
        assert!(
            greeting.iter().any(|message| matches!(
                message,
                ServerMessage::Response {
                    response: Response::Error {
                        code: ErrorCode::Conflict,
                        ..
                    },
                    ..
                }
            )),
            "unexpected Dashboard greeting: {greeting:?}"
        );
        assert!(
            Instant::now() < deadline,
            "the previous Dashboard slot was not released"
        );
        drop(attached);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn dashboard_lines(dashboard: &Dashboard) -> Vec<String> {
    const COLS: u16 = 120;
    const ROWS: u16 = 40;
    let mut terminal = Terminal::new(TestBackend::new(COLS, ROWS)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..ROWS)
        .map(|y| {
            (0..COLS)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

fn dashboard_text(dashboard: &Dashboard) -> String {
    dashboard_lines(dashboard).join("\n")
}

fn dashboard_line(dashboard: &Dashboard, needle: &str) -> String {
    dashboard_lines(dashboard)
        .into_iter()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?}:\n{}", dashboard_text(dashboard)))
}

/// The focus text of every focused pane segment on one rendered line. The
/// wide layout puts the sidebar marker at the line start and the list marker
/// after the pane divider, so a line-start scan misses the list.
fn focused_segments(line: &str) -> impl Iterator<Item = &str> {
    line.split('│')
        .filter_map(|segment| segment.trim_start().strip_prefix("› "))
}

/// The one Settings line carrying the monochrome focus marker.
fn focused_dashboard_line(dashboard: &Dashboard) -> String {
    dashboard_lines(dashboard)
        .into_iter()
        .find(|line| focused_segments(line).next().is_some())
        .unwrap_or_else(|| panic!("no focused Settings line:\n{}", dashboard_text(dashboard)))
}

/// The focus-marked pane segment containing `needle`. In list focus both the
/// category and the selected row carry the marker, in different pane segments.
fn focused_dashboard_line_with(dashboard: &Dashboard, needle: &str) -> String {
    dashboard_lines(dashboard)
        .into_iter()
        .find(|line| focused_segments(line).any(|text| text.contains(needle)))
        .unwrap_or_else(|| {
            panic!(
                "no focused Settings line with {needle:?}:\n{}",
                dashboard_text(dashboard)
            )
        })
}

fn settings_open(dashboard: &Dashboard) -> bool {
    let text = dashboard_text(dashboard);
    [
        "Alerts",
        "Appearance",
        "Workspaces",
        "Remembered launches",
        "Remote",
    ]
    .iter()
    .all(|category| text.contains(*category))
}

/// Open Settings through the Browse menu, where category focus starts on Alerts.
fn open_settings(attached: &mut AttachedDashboard) {
    let dashboard = &mut attached.dashboard;
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Char(','));
    assert!(settings_open(dashboard), "{}", dashboard_text(dashboard));
    assert!(
        focused_dashboard_line(dashboard).contains("Alerts"),
        "the menu must open category focus on Alerts:\n{}",
        dashboard_text(dashboard)
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "opening Settings sent a request"
    );
}

/// Back out one level at a time until Settings closes, without sending writes.
fn close_settings(attached: &mut AttachedDashboard) {
    for _ in 0..6 {
        if !settings_open(&attached.dashboard) {
            assert!(attached.dashboard.drain_outbox().is_empty());
            return;
        }
        attached.dashboard.key(KeyCode::Esc);
    }
    panic!(
        "Settings did not close:\n{}",
        dashboard_text(&attached.dashboard)
    );
}

fn focus_settings_category(attached: &mut AttachedDashboard, category: &str) {
    for _ in 0..ovrcr::protocol::entries().len() {
        if focused_dashboard_line(&attached.dashboard).contains(category) {
            return;
        }
        attached.dashboard.key(KeyCode::Down);
    }
    panic!(
        "no Settings category {category:?}:\n{}",
        dashboard_text(&attached.dashboard)
    );
}

fn focus_settings_row(dashboard: &mut Dashboard, label: &str) {
    for _ in 0..ovrcr::protocol::entries().len() + 8 {
        if dashboard_lines(dashboard)
            .iter()
            .any(|line| focused_segments(line).any(|text| text.contains(label)))
        {
            return;
        }
        dashboard.key(KeyCode::Down);
    }
    panic!(
        "no focused Settings row {label:?}:\n{}",
        dashboard_text(dashboard)
    );
}

/// Navigate from category focus to the Theme picker and open it.
fn open_theme_picker(attached: &mut AttachedDashboard) {
    focus_settings_category(attached, "Appearance");
    attached.dashboard.key(KeyCode::Enter);
    focus_settings_row(&mut attached.dashboard, "Theme");
    attached.dashboard.key(KeyCode::Enter);
}

fn take_dashboard_request(dashboard: &mut Dashboard) -> ClientMessage {
    let mut requests = dashboard.drain_outbox();
    assert_eq!(requests.len(), 1, "expected one request, got {requests:?}");
    requests.remove(0)
}

fn picker_roots(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(|path| PathBuf::from(*path)).collect()
}

fn assert_paths_in_order(dashboard: &Dashboard, paths: &[&str]) {
    assert_paths_ordered(&dashboard_text(dashboard), paths);
}

fn assert_paths_ordered(text: &str, paths: &[&str]) {
    let positions: Vec<usize> = paths
        .iter()
        .map(|path| {
            text.find(*path)
                .unwrap_or_else(|| panic!("no {path:?} in:\n{text}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{paths:?} are out of order at {positions:?}:\n{text}"
    );
}

fn auth_call_log(live: &live::Live) -> PathBuf {
    let log = live.root.path().join("claude-auth-calls");
    let script = std::fs::read_to_string(&live.claude.command).unwrap();
    let needle = "then exec cat";
    assert!(script.contains(needle), "{script}");
    let script = script.replacen(
        needle,
        &format!("then echo call >> '{}'; exec cat", log.display()),
        1,
    );
    std::fs::write(&live.claude.command, script).unwrap();
    log
}

fn codex_fixture_script(live: &live::Live) -> PathBuf {
    let script = live.root.path().join("codex-fixture");
    let executable = std::env::current_exe().unwrap();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'codex-cli 0.155.1'; exit 0; fi\nexec '{}' --ignored --exact native_quota_rpc_fixture --nocapture --quiet\n",
            executable.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    await_executable(&script);
    script
}

fn newest_pid(path: &Path) -> i32 {
    wait_until("collector pid was not recorded", || path.is_file());
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .next_back()
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn alive(pid: i32) -> bool {
    let status = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    matches!(status, Ok(status) if status.success())
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn await_executable(path: &Path) {
    for _ in 0..50 {
        match Command::new(path).arg("__ovrcr_fixture_probe").output() {
            Ok(_) => return,
            Err(error) if error.raw_os_error() == Some(26) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("probe {}: {error}", path.display()),
        }
    }
    panic!("executable stayed busy: {}", path.display());
}

fn documented_defaults() -> String {
    let docs = include_str!("../docs/dashboard.md");
    let mut rest = docs;
    while let Some(start) = rest.find("```toml\n") {
        let after = &rest[start + "```toml\n".len()..];
        let end = after
            .find("\n```")
            .expect("unclosed toml fence in docs/dashboard.md");
        let block = &after[..end];
        if block.contains("desktop_notifications") {
            return block.to_owned();
        }
        rest = &after[end + 4..];
    }
    panic!("docs/dashboard.md has no settings toml block");
}

#[cfg(feature = "gui")]
fn assert_only_allowlisted(demo: &SettingsReport, production: &SettingsReport) {
    let mut changed = Vec::new();
    for (demo_row, production_row) in demo.rows.iter().zip(production.rows.iter()) {
        assert_eq!(demo_row.key, production_row.key);
        if demo_row.value != production_row.value {
            changed.push(demo_row.key.as_str());
        }
    }
    assert!(
        !changed.is_empty(),
        "the demo document matches production exactly"
    );
    for key in changed {
        let allowed = ovrcr::gui::DEMO_SETTINGS_ALLOWLIST
            .iter()
            .any(|prefix| key == *prefix || key.starts_with(&format!("{prefix}.")));
        assert!(
            allowed,
            "{key} differs from production but is not in the demo allowlist"
        );
    }
}

struct HomeGuard {
    previous: Option<std::ffi::OsString>,
}

impl HomeGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", path) };
        Self { previous }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}

fn service_config(
    definition: &Path,
    manager: &Path,
    executable: &Path,
    registry: &Path,
    socket: &Path,
    home: &Path,
) -> ServiceConfig {
    #[cfg(target_os = "macos")]
    let (platform, service_name) = (ServicePlatform::Launchd, "com.ovrcr.server.test".into());
    #[cfg(target_os = "linux")]
    let (platform, service_name) = (ServicePlatform::Systemd, "ovrcr-test.service".into());
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    panic!("service manager unavailable: background service is supported only on macOS and Linux");
    ServiceConfig {
        platform,
        service_name,
        definition_path: definition.to_path_buf(),
        manager_executable: manager.to_path_buf(),
        executable: executable.to_path_buf(),
        registry_path: registry.to_path_buf(),
        server_paths: ServerPaths {
            socket: socket.to_path_buf(),
        },
        environment: vec![
            ("HOME".into(), home.display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("SHELL".into(), "/bin/sh".into()),
        ],
    }
}

fn write_service_manager(
    path: &Path,
    definition: &Path,
    executable: &Path,
    pidfile: &Path,
    log: &Path,
    err: &Path,
) {
    let launcher = path.with_file_name("service-launcher.py");
    // The shell stand-in only accepts launchctl/systemctl verbs. Python
    // parses the definition and starts the Server in its own session:
    // macOS bash 3.2 has no `mapfile`, and a child left in this script's
    // process group dies with the script before it can bind.
    std::fs::write(
        &launcher,
        r#"import re, subprocess, sys, time

definition, executable, pidfile, err = sys.argv[1:5]
text = open(definition, encoding="utf-8").read()
env = {}
if "<plist" in text:
    def unescape(value):
        return (
            value.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", '"')
            .replace("&apos;", "'")
            .replace("&amp;", "&")
        )

    for key, value in re.findall(r"<key>([^<]+)</key>\s*<string>([^<]*)</string>", text):
        if key == "Label" or key.startswith("Standard"):
            continue
        env[key] = unescape(value)
else:
    for line in text.splitlines():
        if line.startswith('Environment="') and line.endswith('"'):
            key, value = line[len('Environment="') : -1].split("=", 1)
            env[key] = value
log = open(err, "ab", buffering=0)
if not env:
    log.write(b"service definition has no environment\n")
    sys.exit(1)
try:
    proc = subprocess.Popen(
        [executable, "server"],
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
except OSError as error:
    log.write(f"failed to start server: {error}\n".encode())
    sys.exit(1)
open(pidfile, "w", encoding="utf-8").write(str(proc.pid))
time.sleep(0.1)
code = proc.poll()
if code is not None:
    log.write(f"server exited {code}\n".encode())
"#,
    )
    .unwrap();
    let script = format!(
        r#"#!/bin/bash
set -u
printf '%s\n' "$*" >> '{log}'
pidfile='{pidfile}'
launcher='{launcher}'
definition='{definition}'
executable='{executable}'
err='{err}'
alive() {{ [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; }}
stop_server() {{
  if alive; then
    pid=$(cat "$pidfile")
    kill -TERM "$pid" 2>/dev/null || true
    sleep 0.2
    kill -KILL "$pid" 2>/dev/null || true
  fi
  rm -f "$pidfile"
}}
start_server() {{
  if alive; then return 0; fi
  python3 "$launcher" "$definition" "$executable" "$pidfile" "$err" || exit 1
}}
if [ "$1" = print ]; then
  alive || exit 1
  printf '\tpid = %s\n' "$(cat "$pidfile")"
  exit 0
fi
if [ "$1" = bootout ]; then stop_server; exit 0; fi
if [ "$1" = bootstrap ] || [ "$1" = kickstart ]; then start_server; exit 0; fi
if [ "$1" = "--user" ]; then
  case "$2" in
    is-active) alive; exit $? ;;
    show) alive || exit 1; cat "$pidfile"; exit 0 ;;
    stop|disable) stop_server; exit 0 ;;
    restart|start) start_server; exit 0 ;;
    *) exit 0 ;;
  esac
fi
exit 0
"#,
        log = log.display(),
        pidfile = pidfile.display(),
        launcher = launcher.display(),
        definition = definition.display(),
        executable = executable.display(),
        err = err.display(),
    );
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    await_executable(path);
}

struct ServiceStop {
    config: ServiceConfig,
    pidfile: PathBuf,
}

impl Drop for ServiceStop {
    fn drop(&mut self) {
        let _ = run_with(&self.config, ServiceCommand::Stop, false);
        if let Ok(text) = std::fs::read_to_string(&self.pidfile)
            && let Ok(pid) = text.trim().parse::<i32>()
            && pid > 1
        {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}
