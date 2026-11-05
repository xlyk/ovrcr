#![cfg(feature = "gui")]
#[path = "support/live.rs"]
mod live;

use anyhow::{Result, ensure};
use ovrcr::gui::Terminal;
use portable_pty::CommandBuilder;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn wait(terminal: &Terminal, needle: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = terminal.screen().contents();
        if screen.contains(needle) {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "missing {needle:?}: {screen}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn profiles(fixture: &live::Live) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(fixture.root.path().join(".claude"))?;
    fs::create_dir_all(fixture.root.path().join(".codex"))?;
    fs::write(
        fixture.root.path().join(".claude/settings.json"),
        r#"{"env":{"KEEP":"local"}}"#,
    )?;
    fs::write(
        fixture.root.path().join(".codex/config.toml"),
        "# Keep this comment\napproval_policy = 'never'\n",
    )?;
    let codex = fixture.claude.command.parent().unwrap().join("codex");
    fs::write(&codex, "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(codex, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn launch(fixture: &live::Live) -> Result<Terminal> {
    // The fixture tests hook offers, not install assets from a shared build.
    let directory = fixture.root.path().join("startup-client");
    fs::create_dir_all(&directory)?;
    let executable = directory.join("ovrcr");
    fs::copy(&fixture.executable, &executable)?;
    let mut command = CommandBuilder::new(&executable);
    command.env("OVRCR_SERVER_EXECUTABLE", &fixture.executable);
    command.env("HOME", fixture.root.path());
    command.env("OVRCR_HOME", &fixture.config);
    command.env("OVRCR_SOCKET", &fixture.socket);
    command.env("CLAUDE_CONFIG_DIR", fixture.root.path().join(".claude"));
    command.env("CODEX_HOME", fixture.root.path().join(".codex"));
    command.env("GROK_HOME", fixture.root.path().join(".grok"));
    let path = std::env::join_paths([
        fixture.claude.command.parent().unwrap(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])?;
    command.env("PATH", path);
    command.env("TERM", "xterm-256color");
    command.env_remove("NO_COLOR");
    command.env("SHELL", "/bin/sh");
    Terminal::start(command, 40, 160, Default::default())
}

/// One Connect agent status screen lists both files and names its answers.
fn wait_connect_screen(fixture: &live::Live, terminal: &Terminal) -> Result<()> {
    wait(terminal, "OVRCR · startup")?;
    wait(terminal, "[y] Connect  [n] Not now")?;
    let screen = terminal.screen().contents();
    ensure!(screen.contains("Connect agent status"), "{screen}");
    ensure!(!screen.contains("[y/n]"), "{screen}");
    for file in [".claude/settings.json", ".codex/config.toml"] {
        let path = fixture.root.path().join(file);
        ensure!(
            screen.contains(path.to_str().unwrap()),
            "missing {path:?}: {screen}"
        );
    }
    Ok(())
}

#[test]
fn dashboard_startup_offers_missing_hooks_and_decline_preserves_profiles() -> Result<()> {
    let fixture = live::Live::binary();
    profiles(&fixture)?;
    let claude = fixture.root.path().join(".claude/settings.json");
    let codex = fixture.root.path().join(".codex/config.toml");
    let before = (fs::read(&claude)?, fs::read(&codex)?);
    let mut terminal = launch(&fixture)?;
    let result = (|| -> Result<()> {
        wait_connect_screen(&fixture, &terminal)?;
        let screen = terminal.screen();
        let heading = screen
            .contents()
            .lines()
            .position(|line| line.contains("OVRCR · startup"))
            .unwrap();
        assert_eq!(
            screen.cell(heading as u16, 1).unwrap().bgcolor(),
            ovrcr_terminal::vt100::Color::Rgb(203, 166, 247)
        );
        assert_eq!(
            screen.cell(heading as u16, 1).unwrap().fgcolor(),
            ovrcr_terminal::vt100::Color::Rgb(30, 30, 46)
        );
        // Enter is Not now.
        terminal.send(b"\r")?;
        // Decline any later optional offer, then the Dashboard says so calmly.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut declined = 0;
        loop {
            let screen = terminal.screen();
            let text = screen.contents();
            if screen.alternate_screen()
                && text.contains("Claude and Codex status off — :connect to set up")
            {
                ensure!(!text.contains("ERROR"), "{text}");
                break;
            }
            if !screen.alternate_screen()
                && text.matches("[y/n] Enter skips · default n").count() > declined
            {
                declined += 1;
                terminal.send(b"n\r")?;
            }
            ensure!(Instant::now() < deadline, "no status-off note: {text}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read(&claude)?, before.0);
        assert_eq!(fs::read(&codex)?, before.1);
        assert!(
            !fixture
                .root
                .path()
                .join("Applications/OVRCR Bridge.app")
                .exists()
        );
        Ok(())
    })();
    terminal.stop()?;
    result
}

#[test]
fn dashboard_accepts_hook_offers_without_changing_native_approvals() -> Result<()> {
    let fixture = live::Live::binary();
    profiles(&fixture)?;
    let mut terminal = launch(&fixture)?;
    let result = (|| -> Result<()> {
        wait_connect_screen(&fixture, &terminal)?;
        // One `y` connects both providers.
        terminal.send(b"y\r")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let claude = fixture.root.path().join(".claude/settings.json");
        let codex = fixture.root.path().join(".codex/config.toml");
        loop {
            let text = fs::read_to_string(&codex)?;
            let settings: serde_json::Value = serde_json::from_str(&fs::read_to_string(&claude)?)?;
            if text.contains("report codex --stdin") && settings["hooks"]["Stop"].is_array() {
                assert!(text.contains("# Keep this comment"));
                assert!(text.contains("approval_policy = 'never'"));
                assert_eq!(settings["env"]["KEEP"], "local");
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "hooks were not installed by one y"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    terminal.stop()?;
    result
}

#[test]
fn inspection_commands_skip_all_startup_offers() -> Result<()> {
    let fixture = live::Live::idle();
    profiles(&fixture)?;
    let claude = fixture.root.path().join(".claude/settings.json");
    let codex = fixture.root.path().join(".codex/config.toml");
    let before = (fs::read(&claude)?, fs::read(&codex)?);
    for args in [
        &["--version"][..],
        &["--help"],
        &["list", "--json"],
        &["settings", "--json"],
    ] {
        let output = fixture.command().args(args).output()?;
        assert!(output.status.success(), "{args:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("OVRCR · startup"));
        assert!(!fixture.socket.exists());
        assert_eq!(fs::read(&claude)?, before.0);
        assert_eq!(fs::read(&codex)?, before.1);
    }
    Ok(())
}
