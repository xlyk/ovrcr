//! Settings lifecycle against a real `ovrcr` (item 5, tier 2).
//!
//! Each case uses a private HOME and no `OVRCR_HOME`, so the default instance
//! directory is the one under test. Fixture binaries stand in for providers.

#[path = "support/live.rs"]
mod live;

use live::claude_auth;

use ovrcr::protocol::{
    CLAUDE_WAITING, ClientMessage, ErrorCode, QuotaSnapshot, QuotaState, Request, Response,
    ServerEvent, ServerMessage, SettingRow, SettingSource, SettingsReport, connect_server,
    read_frame, write_frame,
};
use ovrcr::server::ServerPaths;
use ovrcr::service::{ServiceCommand, ServiceConfig, ServicePlatform, run_with};
use ovrcr::settings;
use serde_json::{Value, json};
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
