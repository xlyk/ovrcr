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
    let mut command = CommandBuilder::new(&fixture.executable);
    command.env("HOME", fixture.root.path());
    command.env("OVRCR_CONFIG", &fixture.config);
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

#[test]
fn dashboard_startup_offers_missing_hooks_and_decline_preserves_profiles() -> Result<()> {
    let fixture = live::Live::binary();
    profiles(&fixture)?;
    let claude = fixture.root.path().join(".claude/settings.json");
    let codex = fixture.root.path().join(".codex/config.toml");
    let before = (fs::read(&claude)?, fs::read(&codex)?);
    let mut terminal = launch(&fixture)?;
    let result = (|| -> Result<()> {
        wait(&terminal, "OVRCR · startup")?;
        wait(&terminal, "Claude")?;
        wait(&terminal, "[y/n]")?;
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
        terminal.send(b"n\r")?;
        wait(&terminal, "Codex")?;
        terminal.send(b"n\r")?;
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
        wait(&terminal, "[y/n]")?;
        terminal.send(b"y\r")?;
        wait(&terminal, "Codex")?;
        terminal.send(b"y\r")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let claude = fixture.root.path().join(".claude/settings.json");
        let codex = fixture.root.path().join(".codex/config.toml");
        loop {
            let text = fs::read_to_string(&codex)?;
            if text.contains("report codex --stdin") {
                assert!(text.contains("# Keep this comment"));
                assert!(text.contains("approval_policy = 'never'"));
                let settings: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(&claude)?)?;
                assert_eq!(settings["env"]["KEEP"], "local");
                assert!(settings["hooks"]["Stop"].is_array());
                return Ok(());
            }
            ensure!(Instant::now() < deadline, "Codex hooks were not installed");
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
        assert!(!String::from_utf8_lossy(&output.stderr).contains("[y/n]"));
        assert!(!fixture.socket.exists());
        assert_eq!(fs::read(&claude)?, before.0);
        assert_eq!(fs::read(&codex)?, before.1);
    }
    Ok(())
}
