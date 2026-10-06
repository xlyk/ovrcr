//! Linux TUI dashboard smoke for Atelier Option A Quota left redesign.
//! Isolated OVRCR_HOME, empty instance directory, real server + Dashboard render path.

#[path = "support/live.rs"]
mod live;

use ovrcr::protocol::*;
use ovrcr::session::TerminalSize;
use std::io::Write;
use std::time::Duration;

#[test]
fn quota_left_option_a_linux_dashboard_smoke() {
    let fixture = live::Live::idle().bounded();
    assert!(fixture.config.is_dir());
    assert!(!fixture.config.join("config.toml").exists());
    assert!(!fixture.config.join("registry.sqlite3").exists());
    let server_log = fixture.root.path().join("server.log");
    fixture.start_binary_logged(&[], &server_log);

    let mut socket = connect_server(&fixture.socket).unwrap();
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

    let mut dashboard = ovrcr::tui::Dashboard::new(TerminalSize {
        rows: 32,
        cols: 120,
    });
    dashboard.install_area(ratatui::layout::Rect::new(0, 0, 120, 32));
    dashboard.handle_server_message(read_frame::<ServerMessage>(&mut socket).unwrap());

    let empty = dump(&dashboard, 120, 32, None);
    let out = fixture.root.path().join("option-a-smoke.txt");
    // Keep a copy under /tmp for the PR comment even after TempDir drops.
    let public = std::path::PathBuf::from("/tmp/ovrcr-quota-ui-a-smoke/option-a-smoke.txt");
    let _ = std::fs::create_dir_all(public.parent().unwrap());
    std::fs::write(&out, &empty).unwrap();
    std::fs::write(&public, &empty).unwrap();

    assert!(
        empty
            .lines()
            .any(|line| line.contains("QUOTA LEFT") && line.contains('─')),
        "header rule missing:\n{empty}"
    );
    assert!(
        empty.contains("Claude · checking"),
        "state separator · missing:\n{empty}"
    );
    assert!(
        empty.contains("Codex · usage off") && empty.contains("Grok · usage off"),
        "usage-off rows:\n{empty}"
    );
    assert!(
        !empty.contains('█') && !empty.contains('░'),
        "legacy bar glyphs still present:\n{empty}"
    );

    let now = 1_800_000_000_000u64;
    let snapshot = QuotaSnapshot {
        cursor: QuotaSnapshot::default().cursor,
        claude: ProviderQuota {
            observed_unix_ms: Some(now),
            checked_unix_ms: Some(now),
            windows: vec![
                QuotaWindow {
                    id: "five_hour".into(),
                    label: "5h".into(),
                    general: true,
                    used_basis_points: Some(4_200),
                    over_limit: false,
                    resets_unix_ms: Some(now + 3_600_000),
                },
                QuotaWindow {
                    id: "seven_day".into(),
                    label: "7d".into(),
                    general: true,
                    used_basis_points: Some(2_000),
                    over_limit: false,
                    resets_unix_ms: Some(now + 86_400_000),
                },
            ],
            ..ProviderQuota::unknown(QuotaProvider::Claude, QuotaState::Current)
        },
        codex: ProviderQuota {
            observed_unix_ms: Some(now),
            checked_unix_ms: Some(now),
            windows: vec![QuotaWindow {
                id: "five_hour".into(),
                label: "5h".into(),
                general: true,
                used_basis_points: Some(6_300),
                over_limit: false,
                resets_unix_ms: Some(now + 3_600_000),
            }],
            ..ProviderQuota::unknown(QuotaProvider::Codex, QuotaState::Current)
        },
        grok: ProviderQuota {
            observed_unix_ms: Some(now),
            checked_unix_ms: Some(now),
            windows: vec![QuotaWindow {
                id: "week".into(),
                label: "wk".into(),
                general: true,
                used_basis_points: Some(2_400),
                over_limit: false,
                resets_unix_ms: Some(now + 604_800_000),
            }],
            ..ProviderQuota::unknown(QuotaProvider::Grok, QuotaState::Current)
        },
    };
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
        snapshot,
    ))));

    let bars = dump(&dashboard, 120, 32, Some(now));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&public)
        .unwrap();
    writeln!(file, "\n--- with allowance ---\n{bars}").unwrap();

    assert!(
        bars.lines()
            .any(|line| line.contains("QUOTA LEFT") && line.contains('─')),
        "{bars}"
    );
    assert!(
        bars.lines().any(|line| {
            line.contains("Claude")
                && line.contains("5h")
                && line.contains('━')
                && line.contains('%')
                && !line.contains('[')
        }),
        "Option A Claude bar missing:\n{bars}"
    );
    assert!(
        bars.lines()
            .any(|line| line.contains("Codex") && line.contains('━') && line.contains("37%")),
        "{bars}"
    );
    assert!(
        bars.lines()
            .any(|line| line.contains("Grok") && line.contains("wk") && line.contains('━')),
        "{bars}"
    );
    eprintln!("option-a smoke ok; dump {}", public.display());
}

fn dump(dashboard: &ovrcr::tui::Dashboard, width: u16, height: u16, now: Option<u64>) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| match now {
            Some(stamp) => ovrcr::tui::draw_dashboard_at(frame, dashboard, stamp),
            None => ovrcr::tui::draw_dashboard(frame, dashboard),
        })
        .unwrap();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
