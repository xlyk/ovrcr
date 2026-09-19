#![cfg(feature = "gui")]

use anyhow::{Result, bail};
use eframe::egui::{self, Event, Key, Modifiers, MouseWheelUnit, TouchPhase};
use ovrcr::gui::input::{Mouse, encode_event};
use ovrcr::gui::{Demo, Terminal};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

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
    wait_screen(&terminal, "implement lifecycle")?;
    let initial = terminal.screen().contents();
    let sidebar = initial
        .lines()
        .filter_map(|line| line.split_once('│').map(|(left, _)| left))
        .collect::<Vec<_>>()
        .join("\n");
    for agent in ["claude", "codex", "pi", "grok"] {
        assert!(sidebar.contains(agent), "missing agent {agent}: {sidebar}");
    }
    for model in ["sonnet-4", "gpt-5.4", "grok-4.6", "opus-4"] {
        assert!(
            !sidebar.contains(model),
            "model shown instead of agent: {sidebar}"
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
    terminal.send(b"\rprintf 'PALETTE_%s\\n' LIVE\r")?;
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
    wait_screen(&terminal, "Confirm action")?;
    terminal.send(b"\r")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = terminal.screen().contents();
        if !text.contains("┌ Confirm action") && !text.contains("palette-check") {
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
    terminal.send(b"\rprintf 'AFTER_%s\\n' CLOSE\r")?;
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
    wait_screen(&terminal, "implement lifecycle")?;
    select_sidebar_session(&mut terminal, "implement lifecycle")?;
    enter_selected_session(&mut terminal, "fixture: lifecycle cleanup")?;
    terminal.send(b"printf 'GUI_%s\\n' CHECKPOINT\r")?;
    wait_screen(&terminal, "GUI_CHECKPOINT")?;
    terminal.send(b"\x07q")?;
    terminal.stop()?;
    let mut terminal = demo.dashboard(40, 120, Default::default())?;
    wait_screen(&terminal, "implement lifecycle")?;
    select_sidebar_session(&mut terminal, "implement lifecycle")?;
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
        wait_screen(&terminal, "implement lifecycle")?;
        for (index, name, ready) in [
            (0, "implement lifecycle", "fixture: lifecycle cleanup"),
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
fn real_dashboard_saves_alert_preferences_and_reloads_config() -> Result<()> {
    let mut demo = Demo::start(Path::new(env!("CARGO_BIN_EXE_ovrcr")))?;
    let root = demo.root().to_owned();
    let pgids = demo_session_groups(&root)?;
    let settings = root.join("preferences/custom.toml");
    let launch = || -> Result<Terminal> {
        let mut command = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_ovrcr"));
        command.env("OVRCR_CONFIG", root.join("config.toml"));
        command.env("OVRCR_SOCKET", root.join("server.sock"));
        command.env("OVRCR_DASHBOARD_CONFIG", &settings);
        command.env("TERM", "xterm-256color");
        Terminal::start(command, 40, 160, Default::default())
    };
    let mut terminal = launch()?;
    wait_screen(&terminal, "implement lifecycle")?;
    terminal.send(b"N")?;
    wait_screen(&terminal, "Desktop notifications: on")?;
    terminal.send(b"S")?;
    wait_screen(&terminal, "Ready sound: on")?;
    let saved: toml::Table = toml::from_str(&std::fs::read_to_string(&settings)?)?;
    assert_eq!(saved["desktop_notifications"].as_bool(), Some(true));
    assert_eq!(saved["ready_sound"].as_bool(), Some(true));
    assert!(
        !root.join("dashboard.toml").exists(),
        "must save to the selected path"
    );
    terminal.stop()?;

    let mut terminal = launch()?;
    wait_screen(&terminal, "implement lifecycle")?;
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
        "desktop_notifications = false\nready_sound = true\n",
    )?;
    let mut terminal = launch()?;
    wait_screen(&terminal, "implement lifecycle")?;
    terminal.send(b"N")?;
    wait_screen(&terminal, "Desktop notifications: on")?;
    terminal.send(b"S")?;
    wait_screen(&terminal, "Ready sound: off")?;
    // A real filesystem error must leave the toggle off and report the failure.
    std::fs::remove_file(&settings)?;
    std::fs::create_dir(&settings)?;
    terminal.send(b"S")?;
    wait_screen(&terminal, "Could not save Ready sound")?;
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
