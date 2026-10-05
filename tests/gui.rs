#![cfg(feature = "gui")]

#[path = "support/dashboard_proxy.rs"]
mod dashboard_proxy;

use anyhow::{Result, bail};
use eframe::egui::{self, Event, Key, Modifiers, MouseWheelUnit, TouchPhase};
use ovrcr::gui::input::{Mouse, encode_event};
use ovrcr::gui::{Demo, Terminal};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

fn private_dashboard_executable(root: &Path) -> Result<std::path::PathBuf> {
    let directory = root.join("dashboard-client");
    std::fs::create_dir_all(&directory)?;
    let executable = directory.join("ovrcr");
    if !executable.exists() {
        std::fs::copy(env!("CARGO_BIN_EXE_ovrcr"), &executable)?;
    }
    Ok(executable)
}

fn wait_screen(terminal: &Terminal, needle: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text = terminal.screen().contents();
        if text.contains(needle) {
            return Ok(());
        }
        if Instant::now() > deadline {
            bail!("screen never contained {needle:?}:\n{text}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "macos")]
fn desktop_alert_environment(root: &Path) -> Result<(std::path::PathBuf, std::ffi::OsString)> {
    use ovrcr::protocol::{BRIDGE_SCHEMA_VERSION, BridgeReply, BridgeStatus, PROTOCOL_VERSION};
    use std::os::unix::fs::PermissionsExt;
    let home = root.join("home");
    let client = home.join("Applications/OVRCR Bridge.app/Contents/MacOS/OVRCRBridge");
    std::fs::create_dir_all(client.parent().unwrap())?;
    let reply = serde_json::to_string(&BridgeReply {
        schema: BRIDGE_SCHEMA_VERSION,
        server_wire: PROTOCOL_VERSION,
        status: BridgeStatus::Available,
        sound_unavailable: None,
    })?;
    std::fs::write(
        &client,
        format!(
            "#!/bin/sh\n[ \"$1\" = --client ] && [ \"$#\" = 1 ] || exit 99\ncat > \"$0.request\"\nprintf '%s\\n' '{reply}'\n"
        ),
    )?;
    std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755))?;
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin)?;
    let player = bin.join("afplay");
    std::fs::write(&player, "#!/bin/sh\nexit 0\n")?;
    std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o755))?;
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))?;
    Ok((home, path))
}

#[test]
fn command_k_is_not_forwarded_to_terminal() {
    let parser = vt100::Parser::new(24, 80, 0);
    let bytes = encode_event(
        &Event::Key {
            key: Key::K,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                mac_cmd: true,
                command: true,
                ..Default::default()
            },
        },
        parser.screen(),
        egui::Rect::NOTHING,
        egui::vec2(8.0, 16.0),
        egui::pos2(0.0, 0.0),
        &mut Mouse::default(),
    );
    assert!(bytes.is_empty());
}

#[test]
fn palette_creates_switches_and_closes_a_real_terminal() -> Result<()> {
    let mut demo = Demo::start(Path::new(env!("CARGO_BIN_EXE_ovrcr")))?;
    let root = demo.root().to_owned();
    let mut pgids = demo_session_groups(&root)?;
    let mut terminal = demo.dashboard(40, 120, Default::default())?;
    // The complete Demo inventory is established above, but the initial frame
    // arrives in PTY chunks. Wait for the last required sidebar label.
    wait_screen(&terminal, "claude:opus-4")?;
    let initial = terminal.screen().contents();
    let sidebar = initial
        .lines()
        .filter_map(|line| line.split_once('│').map(|(left, _)| left))
        .collect::<Vec<_>>()
        .join("\n");
    for agent in ["claude", "codex", "pi", "grok"] {
        assert!(sidebar.contains(agent), "missing agent {agent}: {sidebar}");
    }
    for label in [
        "claude:sonnet-4",
        "codex:gpt-5.4",
        "pi:grok-4.6",
        "grok:4.6",
        "claude:opus-4",
    ] {
        assert!(
            sidebar.contains(label),
            "missing agent and model {label}: {sidebar}"
        );
    }
    terminal.send(b":create terminal\r")?;
    wait_screen(&terminal, "┌ Create terminal")?;
    // Explicitly choose Terminal, accept the workspace, and pin an optional
    // Name. A blank Command requests the configured shell.
    terminal.send(b"\x15Terminal\t\t\x15palette-check\t\x15\r")?;
    wait_screen(&terminal, "palette-check")?;
    // Wait for the palette to finish; its form also contains the new name.
    let deadline = Instant::now() + Duration::from_secs(5);
    while terminal.screen().contents().contains("┌ Create terminal") {
        assert!(
            Instant::now() < deadline,
            "create never completed: {}",
            terminal.screen().contents()
        );
        std::thread::yield_now();
    }
    enter_selected_session(&mut terminal, "palette-check")?;
    terminal.send(b"printf 'PALETTE_%s\\n' LIVE\r")?;
    wait_screen(&terminal, "PALETTE_LIVE")?;
    let inventory = std::process::Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args(["terminal", "list", "--json"])
        .env("OVRCR_SOCKET", root.join("server.sock"))
        .env("OVRCR_CONFIG", root.join("config.toml"))
        .output()?;
    assert!(inventory.status.success());
    let records: serde_json::Value = serde_json::from_slice(&inventory.stdout)?;
    let created = records
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "palette-check")
        .unwrap();
    let pgid = unsafe { libc::getpgid(created["pid"].as_i64().unwrap() as i32) };
    assert!(pgid > 1);
    pgids.push(pgid);
    terminal.send(b"\x07:switch terminal palette-check\r")?;
    wait_screen(&terminal, "PALETTE_LIVE")?;
    terminal.send(b":close terminal\r")?;
    wait_screen(&terminal, "Close terminal?")?;
    terminal.send(b"\r")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = terminal.screen().contents();
        if !text.contains("Close terminal?") && !text.contains("palette-check") {
            break;
        }
        assert!(Instant::now() < deadline, "close never completed: {text}");
        std::thread::yield_now();
    }
    assert_eq!(unsafe { libc::kill(-pgid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    // Removal and replacement SetView complete asynchronously. Select the
    // surviving shell and wait for typing readiness before sending shell text.
    select_sidebar_session(&mut terminal, "$ local")?;
    enter_selected_session(&mut terminal, "local (#1)")?;
    terminal.send(b"printf 'AFTER_%s\\n' CLOSE\r")?;
    wait_screen(&terminal, "AFTER_CLOSE")?;
    terminal.stop()?;
    demo.shutdown()?;
    assert!(!root.exists());
    for pgid in pgids {
        assert_eq!(unsafe { libc::kill(-pgid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    Ok(())
}

#[test]
fn real_dashboard_accepts_input_reattaches_and_cleans_up_demo() -> Result<()> {
    let mut demo = Demo::start(Path::new(env!("CARGO_BIN_EXE_ovrcr")))?;
    let root = demo.root().to_owned();
    let pgids = demo_session_groups(&root)?;
    let mut terminal = demo.dashboard(40, 120, Default::default())?;
    wait_screen(&terminal, "claude:sonnet-4")?;
    select_sidebar_session(&mut terminal, "claude:sonnet-4")?;
    enter_selected_session(&mut terminal, "fixture: lifecycle cleanup")?;
    terminal.send(b"printf 'GUI_%s\\n' CHECKPOINT\r")?;
    wait_screen(&terminal, "GUI_CHECKPOINT")?;
    terminal.send(b"\x07q")?;
    terminal.stop()?;
    let mut terminal = demo.dashboard(40, 120, Default::default())?;
    wait_screen(&terminal, "claude:sonnet-4")?;
    select_sidebar_session(&mut terminal, "claude:sonnet-4")?;
    enter_selected_session(&mut terminal, "GUI_CHECKPOINT")?;
    terminal.stop()?;
    demo.shutdown()?;
    assert!(!root.exists(), "successful cleanup must remove the demo");
    for pgid in pgids {
        assert_eq!(
            unsafe { libc::kill(-pgid, 0) },
            -1,
            "demo process group {pgid} survived cleanup"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    Ok(())
}

#[test]
fn demo_shells_inherit_paths_and_cli_reaches_fixture() -> Result<()> {
    let mut demo = Demo::start(Path::new(env!("CARGO_BIN_EXE_ovrcr")))?;
    let root = demo.root().to_owned();
    let pgids = demo_session_groups(&root)?;
    eprintln!("fixture={} session_pgids={pgids:?}", root.display());
    let mut terminal = demo.dashboard(40, 160, Default::default())?;
    let result = (|| -> Result<()> {
        wait_screen(&terminal, "claude:sonnet-4")?;
        for (index, name, ready) in [
            (0, "claude:sonnet-4", "fixture: lifecycle cleanup"),
            (1, "local", "Terminal mode"),
        ] {
            select_sidebar_session(&mut terminal, name)?;
            enter_selected_session(&mut terminal, ready)?;
            let env_file = root.join(format!("child-env-{index}"));
            let list_file = root.join(format!("child-list-{index}.json"));
            // Refuse the CLI call if either path is wrong: a broken fixture must
            // never query the user's default socket, even in the red test.
            let script = format!(
                "printf '%s\\n' \"$OVRCR_CONFIG\" \"$OVRCR_SOCKET\" > '{}'; if [ \"$OVRCR_CONFIG\" = '{}/config.toml' ] && [ \"$OVRCR_SOCKET\" = '{}/server.sock' ]; then '{}' terminal list --json > '{}'; fi; printf 'ENV_{index}_%s\\n' DONE",
                env_file.display(),
                root.display(),
                root.display(),
                env!("CARGO_BIN_EXE_ovrcr"),
                list_file.display(),
            );
            // Use the GUI's paste path, rather than flooding the dispatcher with
            // hundreds of separate keystrokes for this long shell command.
            let paste = encode_event(
                &Event::Paste(script),
                &terminal.screen(),
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                &mut Mouse::default(),
            );
            terminal.send(&paste)?;
            terminal.send(b"\r")?;
            wait_screen(&terminal, &format!("ENV_{index}_DONE"))?;
            anyhow::ensure!(
                std::fs::read_to_string(env_file)?
                    == format!(
                        "{}/config.toml\n{}/server.sock\n",
                        root.display(),
                        root.display()
                    ),
                "fixture shell did not inherit both paths"
            );
            let records: serde_json::Value = serde_json::from_slice(&std::fs::read(list_file)?)?;
            let records = records.as_array().unwrap();
            anyhow::ensure!(records.len() == 12, "CLI did not return the demo sessions");
            let mut child_pgids: Vec<_> = records
                .iter()
                .map(|record| unsafe { libc::getpgid(record["pid"].as_i64().unwrap() as i32) })
                .collect();
            child_pgids.sort_unstable();
            let mut expected = pgids.clone();
            expected.sort_unstable();
            anyhow::ensure!(child_pgids == expected, "CLI reached a different server");
            terminal.send(b"\x07")?;
            wait_screen(&terminal, "BROWSE")?;
        }
        Ok(())
    })();
    terminal.stop()?;
    demo.shutdown()?;
    assert!(!root.exists());
    for pgid in pgids {
        assert_eq!(unsafe { libc::kill(-pgid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    eprintln!("fixture cleanup verified: {}", root.display());
    result
}

fn select_sidebar_session(terminal: &mut Terminal, name: &str) -> Result<()> {
    let screen = terminal.screen();
    let (_, cols) = screen.size();
    let Some(row) = screen.rows(0, cols).position(|text| text.contains(name)) else {
        bail!("sidebar never rendered {name:?}");
    };
    let click = format!("\x1b[<0;5;{}M", row + 1);
    terminal.send(click.as_bytes())
}

fn enter_selected_session(terminal: &mut Terminal, needle: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        terminal.send(b"\r")?;
        let poll_deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < poll_deadline {
            let text = terminal.screen().contents();
            if text.contains(needle) && text.contains("Terminal mode") {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        if Instant::now() >= deadline {
            bail!(
                "selected session never reached terminal mode with {needle:?}:\n{}",
                terminal.screen().contents()
            );
        }
    }
}

fn demo_session_groups(root: &Path) -> Result<Vec<i32>> {
    use ovrcr::protocol::{
        ClientMessage, Request, Response, ServerMessage, read_frame, write_frame,
    };
    let mut stream = ovrcr::protocol::connect_server(root.join("server.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 1,
            request: Request::List,
        },
    )?;
    let ServerMessage::Response {
        response: Response::Hierarchy(hierarchy),
        ..
    } = read_frame(&mut stream)?
    else {
        bail!("demo did not return its sessions");
    };
    let sessions: Vec<_> = hierarchy
        .projects
        .iter()
        .flat_map(|project| {
            project.workspaces.iter().flat_map(move |workspace| {
                workspace.sessions.iter().map(move |session| {
                    (
                        project.name.as_str(),
                        workspace.name.as_str(),
                        session.name.as_str(),
                        session.label.as_str(),
                        session.pid,
                    )
                })
            })
        })
        .collect();
    assert_eq!(
        sessions
            .iter()
            .map(|(project, workspace, name, _, _)| (*project, *workspace, *name))
            .collect::<Vec<_>>(),
        vec![
            ("consigint", "main", "local"),
            ("consigint", "gui-consigint-auth-handoff", "local"),
            (
                "consigint",
                "gui-consigint-auth-handoff",
                "review auth handoff"
            ),
            ("consigint", "gui-consigint-worktree-lifecycle", "local"),
            (
                "consigint",
                "gui-consigint-worktree-lifecycle",
                "implement lifecycle cleanup"
            ),
            (
                "consigint",
                "gui-consigint-worktree-lifecycle",
                "review websocket shutdown"
            ),
            (
                "consigint",
                "gui-consigint-worktree-lifecycle",
                "plan snapshot restore"
            ),
            ("spacelift-agent", "main", "local"),
            (
                "spacelift-agent",
                "gui-spacelift-agent-pipeline-progress-v2",
                "local"
            ),
            (
                "spacelift-agent",
                "gui-spacelift-agent-pipeline-progress-v2",
                "build pipeline progress"
            ),
            (
                "spacelift-agent",
                "gui-spacelift-agent-scope-quality",
                "local"
            ),
            (
                "spacelift-agent",
                "gui-spacelift-agent-scope-quality",
                "review scope quality"
            ),
        ]
    );
    assert_eq!(
        sessions
            .iter()
            .filter(|(_, _, name, _, _)| *name != "local")
            .map(|(_, _, _, label, _)| *label)
            .collect::<Vec<_>>(),
        vec![
            "grok / grok-4.6",
            "claude / sonnet-4",
            "codex / gpt-5.4",
            "pi / grok-4.6",
            "claude / opus-4",
            "codex / gpt-5.4",
        ]
    );
    let pgids: Vec<_> = sessions
        .into_iter()
        .filter_map(|(_, _, _, _, pid)| pid)
        .map(|pid| unsafe { libc::getpgid(pid as i32) })
        .collect();
    assert_eq!(pgids.len(), 12);
    assert!(pgids.iter().all(|pgid| *pgid > 1));
    Ok(pgids)
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn input_preserves_control_keys_terminal_modes_and_mouse_cells() {
    let mut parser = vt100::Parser::new(10, 20, 0);
    let rect = egui::Rect::from_min_size(egui::pos2(12.0, 30.0), egui::vec2(160.0, 160.0));
    let cell = egui::vec2(8.0, 16.0);
    let mut mouse = Mouse::default();
    let mut encode = |event: Event, screen: &vt100::Screen| {
        encode_event(
            &event,
            screen,
            rect,
            cell,
            egui::pos2(29.0, 79.0),
            &mut mouse,
        )
    };
    assert_eq!(
        encode(key(Key::G, Modifiers::CTRL), parser.screen()),
        b"\x07"
    );
    assert_eq!(
        encode(key(Key::C, Modifiers::CTRL), parser.screen()),
        b"\x03"
    );
    assert_eq!(
        encode(key(Key::ArrowUp, Modifiers::NONE), parser.screen()),
        b"\x1b[A"
    );
    assert_eq!(
        encode(key(Key::Tab, Modifiers::SHIFT), parser.screen()),
        b"\x1b[Z"
    );
    assert_eq!(
        encode(Event::Text("é界".into()), parser.screen()),
        "é界".as_bytes()
    );
    assert_eq!(
        encode(Event::Paste("one\ntwo".into()), parser.screen()),
        b"one\ntwo"
    );
    let click = Event::PointerButton {
        pos: egui::pos2(29.0, 79.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    };
    assert!(encode(click.clone(), parser.screen()).is_empty());
    parser.process(b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        encode(key(Key::ArrowUp, Modifiers::NONE), parser.screen()),
        b"\x1bOA"
    );
    assert_eq!(
        encode(Event::Paste("one\ntwo".into()), parser.screen()),
        b"\x1b[200~one\ntwo\x1b[201~"
    );
    assert_eq!(encode(click, parser.screen()), b"\x1b[<0;3;4M");
    let outside = Event::PointerButton {
        pos: egui::pos2(11.0, 79.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    };
    assert!(encode(outside, parser.screen()).is_empty());
    let wheel = |delta| Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta,
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    };
    parser.process(b"\x1b[?1000l");
    assert!(encode(wheel(egui::vec2(0.0, 1.0)), parser.screen()).is_empty());
    parser.process(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        encode(wheel(egui::vec2(0.0, 1.0)), parser.screen()),
        b"\x1b[<64;3;4M"
    );
    assert_eq!(
        encode(wheel(egui::vec2(0.0, -1.0)), parser.screen()),
        b"\x1b[<65;3;4M"
    );
}

#[test]
fn outer_pty_parses_attributes_and_resize_reaches_the_tty() -> Result<()> {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.args([
        "-c",
        r"printf '\033[38;2;12;34;56mREADY\033[0m'; while IFS= read -r line; do stty size; done",
    ]);
    let mut terminal = Terminal::start(command, 24, 80, Default::default())?;
    wait_screen(&terminal, "READY")?;
    assert_eq!(
        terminal.screen().cell(0, 0).unwrap().fgcolor(),
        vt100::Color::Rgb(12, 34, 56)
    );
    terminal.resize(32, 100)?;
    terminal.send(b"size\n")?;
    wait_screen(&terminal, "32 100")?;
    assert_eq!(terminal.screen().size(), (32, 100));
    terminal.stop()?;
    Ok(())
}

#[test]
fn real_dashboard_initial_selection_survives_quota_events_before_startup_responses() -> Result<()> {
    use ovrcr::protocol::{
        ClientMessage, QuotaSnapshot, QuotaState, Response, ServerEvent, ServerMessage,
    };
    use std::os::unix::net::UnixListener;
    let mut demo = Demo::start(Path::new(env!("CARGO_BIN_EXE_ovrcr")))?;
    let root = demo.root().to_owned();
    let pgids = demo_session_groups(&root)?;
    let socket = root.join("startup-proxy.sock");
    let listener = UnixListener::bind(&socket)?;
    let proxy_executable = dashboard_proxy::publish_build(&socket)?;
    let upstream = root.join("server.sock");
    let proxy = thread::spawn(move || -> Result<()> {
        let (mut front, _) = listener.accept()?;
        ovrcr::protocol::exchange_preamble(&mut front)?;
        let mut back = ovrcr::protocol::connect_server(upstream)?;
        let mut front_read = front.try_clone()?;
        let mut back_write = back.try_clone()?;
        let forward = thread::spawn(move || -> Result<()> {
            while let Ok(message) = ovrcr::protocol::read_frame::<ClientMessage>(&mut front_read) {
                if ovrcr::protocol::write_frame(&mut back_write, &message).is_err() {
                    break;
                }
            }
            // Relay detach EOF to the real owner, including when the test fails.
            let _ = back_write.shutdown(std::net::Shutdown::Write);
            Ok(())
        });
        let result = (|| -> Result<()> {
            while let Ok(message) = ovrcr::protocol::read_frame::<ServerMessage>(&mut back) {
                // The Server publishes its own quota snapshot (Claude auth
                // status is not the silent default). This test injects the
                // events it asserts on; forwarding the real ones replaces
                // "invalid report" before the screen can show it.
                if matches!(message, ServerMessage::Event(ServerEvent::QuotaChanged(_))) {
                    continue;
                }
                let count = match &message {
                    ServerMessage::Response {
                        request_id: 1,
                        response: Response::Hierarchy(_),
                    } => 2,
                    ServerMessage::Response {
                        request_id: 2,
                        response: Response::Ok,
                    } => 1,
                    _ => 0,
                };
                for _ in 0..count {
                    let mut quota = QuotaSnapshot::default();
                    quota.grok.state = QuotaState::Invalid;
                    ovrcr::protocol::write_frame(
                        &mut front,
                        &ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(quota))),
                    )?;
                }
                // Live quota publishes (claude auth, native workers) replace the
                // injected Invalid row before the screen can show it. The events
                // under test are the ones injected above, ahead of startup replies.
                if matches!(message, ServerMessage::Event(ServerEvent::QuotaChanged(_))) {
                    continue;
                }
                if ovrcr::protocol::write_frame(&mut front, &message).is_err() {
                    break;
                }
            }
            Ok(())
        })();
        let _ = front.shutdown(std::net::Shutdown::Both);
        let _ = back.shutdown(std::net::Shutdown::Both);
        forward
            .join()
            .map_err(|_| anyhow::anyhow!("startup forwarder panicked"))??;
        result
    });
    let mut command = portable_pty::CommandBuilder::new(private_dashboard_executable(&root)?);
    command.env("OVRCR_CONFIG", root.join("config.toml"));
    command.env("OVRCR_SOCKET", &socket);
    command.env("OVRCR_SERVER_EXECUTABLE", proxy_executable);
    #[cfg(target_os = "macos")]
    {
        let (home, path) = desktop_alert_environment(&root)?;
        command.env("HOME", home);
        command.env("PATH", path);
    }
    command.env("TERM", "xterm-256color");
    let mut terminal = Terminal::start(command, 40, 160, Default::default())?;
    // The actual first session's child output proves selection, not just rows.
    wait_screen(&terminal, "local (#1)")?;
    wait_screen(&terminal, "Grok")?;
    wait_screen(&terminal, "invalid report")?;
    terminal.send(b"L")?;
    wait_screen(&terminal, "Automatic local terminals: off")?;
    terminal.send(b"N")?;
    wait_screen(&terminal, "Desktop notifications: on")?;
    let settings: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("dashboard.toml"))?)?;
    assert_eq!(settings["automatic_local_terminals"].as_str(), Some("off"));
    assert_eq!(settings["desktop_notifications"].as_bool(), Some(true));
    terminal.send(b"\rprintf 'STARTUP_%s\\n' INPUT_OK\r")?;
    wait_screen(&terminal, "STARTUP_INPUT_OK")?;
    terminal.stop()?;
    proxy
        .join()
        .map_err(|_| anyhow::anyhow!("startup proxy panicked"))??;
    demo.shutdown()?;
    assert!(!root.exists());
    for pgid in pgids {
        assert_eq!(
            unsafe { libc::kill(-pgid, 0) },
            -1,
            "owned group {pgid} survived cleanup"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    Ok(())
}

#[test]
fn real_dashboard_cycles_local_policy_and_same_server_provisions_from_saved_setting() -> Result<()>
{
    use ovrcr::protocol::{
        BranchRequest, CreateSessionRequest, Request, Response, SessionKind, client,
    };
    use std::os::unix::fs::PermissionsExt;

    let mut demo = Demo::start(Path::new(env!("CARGO_BIN_EXE_ovrcr")))?;
    let root = demo.root().to_owned();
    let mut pgids = demo_session_groups(&root)?;
    let settings = root.join("dashboard.toml");
    // Control connections serve one request each. Every request uses this
    // fixture's socket; check the original live session identities after reload.
    let request = |request| -> Result<Response> {
        let mut control = ovrcr::protocol::connect_server(root.join("server.sock"))?;
        control.set_read_timeout(Some(Duration::from_secs(5)))?;
        client::request(&mut control, 1, request)
    };
    let Response::Hierarchy(initial) = request(Request::List)? else {
        bail!("initial hierarchy");
    };
    let saved_policy = || -> Result<String> {
        let saved: toml::Table = toml::from_str(&std::fs::read_to_string(&settings)?)?;
        Ok(saved["automatic_local_terminals"]
            .as_str()
            .unwrap()
            .to_owned())
    };
    let provision = |id: &str, locals: usize| -> Result<Vec<ovrcr::protocol::SessionSummary>> {
        client::expect_ok(request(Request::CreateWorkspace {
            project: "consigint".into(),
            id: id.into(),
            branch: BranchRequest::New {
                branch: format!("feature/{id}"),
                base: "main".into(),
            },
        })?)?;
        let Response::Hierarchy(hierarchy) = request(Request::List)? else {
            bail!("provisioned hierarchy");
        };
        let workspace = hierarchy
            .projects
            .iter()
            .find(|p| p.name == "consigint")
            .unwrap()
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .unwrap();
        assert_eq!(workspace.sessions.len(), locals, "workspace={id}");
        assert!(
            workspace
                .sessions
                .iter()
                .all(|s| s.name == "local" && s.phase.is_live())
        );
        Ok(workspace.sessions.clone())
    };
    let mut terminal = demo.dashboard(40, 160, Default::default())?;
    wait_screen(&terminal, "claude:sonnet-4")?;
    assert_eq!(saved_policy()?, "on");
    terminal.send(b"L")?;
    wait_screen(&terminal, "Automatic local terminals: off")?;
    assert_eq!(saved_policy()?, "off");
    provision("policy-off", 0)?;

    let Response::CreatedSession(manual) = request(Request::CreateSession(CreateSessionRequest {
        project: "consigint".into(),
        workspace: "policy-off".into(),
        name: "policy-manual".into(),
        label: None,
        kind: SessionKind::Terminal,
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf 'POLICY_%s\\n' EXPLICIT; exec /bin/sh -i".into(),
        ],
    }))?
    else {
        bail!("explicit terminal launch failed under off");
    };
    assert!(manual.phase.is_live());
    pgids.push(manual.pid.unwrap() as i32);
    wait_screen(&terminal, "policy-manual")?;
    select_sidebar_session(&mut terminal, "policy-manual")?;
    enter_selected_session(&mut terminal, "POLICY_EXPLICIT")?;
    terminal.send(b"\x07")?;
    wait_screen(&terminal, "BROWSE")?;
    terminal.stop()?;

    let mut terminal = demo.dashboard(40, 160, Default::default())?;
    wait_screen(&terminal, "claude:sonnet-4")?;
    terminal.send(b"L")?;
    wait_screen(&terminal, "Automatic local terminals: default branch only")?;
    assert_eq!(saved_policy()?, "default_branch_only");
    provision("policy-default", 0)?;
    terminal.send(b"L")?;
    wait_screen(&terminal, "Automatic local terminals: on")?;
    assert_eq!(saved_policy()?, "on");
    for session in provision("policy-on", 1)? {
        pgids.push(session.pid.unwrap() as i32);
    }
    let Response::Hierarchy(current) = request(Request::List)? else {
        bail!("current hierarchy");
    };
    for previous in initial
        .projects
        .iter()
        .flat_map(|p| &p.workspaces)
        .flat_map(|w| &w.sessions)
    {
        let session = current
            .projects
            .iter()
            .flat_map(|p| &p.workspaces)
            .flat_map(|w| &w.sessions)
            .find(|s| s.id == previous.id)
            .unwrap();
        assert_eq!(session.run, previous.run);
        assert_eq!(session.pid, previous.pid);
        assert!(
            session.phase.is_live(),
            "existing session changed: {}",
            session.name
        );
    }
    terminal.send(b"L")?;
    wait_screen(&terminal, "Automatic local terminals: off")?;
    let before = std::fs::read_to_string(&settings)?;
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o444))?;
    terminal.send(b"L")?;
    wait_screen(&terminal, "could not save automatic_local_terminals")?;
    assert_eq!(std::fs::read_to_string(&settings)?, before);
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o600))?;
    terminal.send(b"L")?;
    wait_screen(&terminal, "Automatic local terminals: default branch only")?;
    assert_eq!(
        saved_policy()?,
        "default_branch_only",
        "failed save must leave the in-memory cycle unchanged"
    );
    terminal.stop()?;
    demo.shutdown()?;
    assert!(!root.exists());
    for pgid in pgids {
        assert_eq!(
            unsafe { libc::kill(-pgid, 0) },
            -1,
            "demo group {pgid} survived cleanup"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        eprintln!("local policy fixture owned_pgid={pgid} absent=true");
    }
    Ok(())
}

#[test]
fn real_dashboard_saves_alert_preferences_and_reloads_config() -> Result<()> {
    let mut demo = Demo::start(Path::new(env!("CARGO_BIN_EXE_ovrcr")))?;
    let root = demo.root().to_owned();
    let pgids = demo_session_groups(&root)?;
    // The client names its own document; the Server (started without it) reads
    // the one beside config.toml. Toggles must follow the Server's reading.
    let client_only = root.join("preferences/custom.toml");
    let settings = root.join("dashboard.toml");
    #[cfg(target_os = "macos")]
    let (home, path) = desktop_alert_environment(&root)?;
    let launch = || -> Result<Terminal> {
        let mut command = portable_pty::CommandBuilder::new(private_dashboard_executable(&root)?);
        command.env("OVRCR_SERVER_EXECUTABLE", env!("CARGO_BIN_EXE_ovrcr"));
        command.env("OVRCR_CONFIG", root.join("config.toml"));
        command.env("OVRCR_SOCKET", root.join("server.sock"));
        command.env("OVRCR_DASHBOARD_CONFIG", &client_only);
        #[cfg(target_os = "macos")]
        {
            command.env("HOME", &home);
            command.env("PATH", &path);
        }
        command.env("TERM", "xterm-256color");
        Terminal::start(command, 40, 160, Default::default())
    };
    let mut terminal = launch()?;
    wait_screen(&terminal, "claude:sonnet-4")?;
    terminal.send(b"N")?;
    wait_screen(&terminal, "Desktop notifications: on")?;
    terminal.send(b"S")?;
    wait_screen(&terminal, "Ready sound: on")?;
    let saved: toml::Table = toml::from_str(&std::fs::read_to_string(&settings)?)?;
    assert_eq!(saved["desktop_notifications"].as_bool(), Some(true));
    assert_eq!(saved["ready_sound"].as_bool(), Some(true));
    assert_eq!(
        saved["automatic_local_terminals"].as_str(),
        Some("on"),
        "the demo's own setting survives the toggles"
    );
    assert!(
        !client_only.exists(),
        "the Dashboard must not save to a document the Server does not read"
    );
    terminal.stop()?;

    let mut terminal = launch()?;
    wait_screen(&terminal, "claude:sonnet-4")?;
    terminal.send(b":desktop notifications")?;
    wait_screen(&terminal, "Disable desktop notifications")?;
    terminal.send(b"\r")?;
    wait_screen(&terminal, "Desktop notifications: off")?;
    terminal.send(b"S")?;
    wait_screen(&terminal, "Ready sound: off")?;
    let saved: toml::Table = toml::from_str(&std::fs::read_to_string(&settings)?)?;
    assert_eq!(saved["desktop_notifications"].as_bool(), Some(false));
    assert_eq!(saved["ready_sound"].as_bool(), Some(false));
    terminal.stop()?;

    std::fs::write(
        &settings,
        "automatic_local_terminals = \"on\"\ndesktop_notifications = false\nready_sound = true\n",
    )?;
    let mut terminal = launch()?;
    wait_screen(&terminal, "claude:sonnet-4")?;
    terminal.send(b"N")?;
    wait_screen(&terminal, "Desktop notifications: on")?;
    terminal.send(b"S")?;
    wait_screen(&terminal, "Ready sound: off")?;
    // A real filesystem error must leave the toggle off and report the failure.
    std::fs::remove_file(&settings)?;
    std::fs::create_dir(&settings)?;
    terminal.send(b"S")?;
    wait_screen(&terminal, "could not save ready_sound")?;
    terminal.send(b":ready sound")?;
    wait_screen(&terminal, "Enable ready sound")?;
    terminal.send(b"\x1b")?;
    terminal.stop()?;
    demo.shutdown()?;
    assert!(!root.exists());
    for pgid in pgids {
        assert_eq!(unsafe { libc::kill(-pgid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    Ok(())
}
