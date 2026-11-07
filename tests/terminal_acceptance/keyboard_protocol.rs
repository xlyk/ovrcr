use super::*;

fn browse_shortcut(event: impl FnOnce(u8) -> Vec<u8>, title: &str) -> Result<()> {
    let fixture = AcceptanceFixture::new()?;
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.send(b"\x07")?;
    let flags = String::from_utf8_lossy(&dashboard.startup_output)
        .split("\x1b[>")
        .skip(1)
        .filter_map(|part| part.split_once('u')?.0.parse::<u8>().ok())
        .last()
        .context("Dashboard did not request keyboard reporting")?;
    dashboard.send(&event(flags))?;
    dashboard
        .wait_until(|screen| screen.contains(title), Duration::from_secs(3))
        .with_context(|| format!("key did not open {title}; flags={flags}"))?;
    Ok(())
}

fn shifted_browse_shortcut(base: char, shifted: char, title: &str) -> Result<()> {
    browse_shortcut(
        |flags| {
            // A conforming terminal reports the base key unless alternate keys were requested.
            if flags & 4 != 0 {
                format!("\x1b[{}:{};2u", u32::from(base), u32::from(shifted))
            } else {
                format!("\x1b[{};2u", u32::from(base))
            }
            .into_bytes()
        },
        title,
    )
}

#[test]
fn shifted_browse_colon_opens_palette_with_negotiated_keyboard_reporting() -> Result<()> {
    shifted_browse_shortcut(';', ':', "Command palette")
}

#[test]
fn shifted_browse_question_opens_help_with_negotiated_keyboard_reporting() -> Result<()> {
    shifted_browse_shortcut('/', '?', "Which key")
}

#[test]
fn legacy_browse_shortcuts_open_palette_and_help() -> Result<()> {
    for (character, title) in [(':', "Command palette"), ('?', "Which key")] {
        browse_shortcut(|_| vec![character as u8], title)?;
    }
    Ok(())
}

#[test]
fn shifted_character_events_open_browse_palette_and_help() -> Result<()> {
    for (character, title) in [(':', "Command palette"), ('?', "Which key")] {
        browse_shortcut(
            |_| format!("\x1b[{};2u", u32::from(character)).into_bytes(),
            title,
        )?;
    }
    Ok(())
}

#[test]
fn alternate_key_reporting_preserves_quick_message_control_shortcuts() -> Result<()> {
    let fixture = AcceptanceFixture::new()?;
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.send(b"\x07\x0f")?;
    dashboard.wait_until(|screen| screen.contains("Quick message"), wait_deadline())?;
    dashboard.send(b"DRAFT_TOKEN\x1b[99:67;6u_SUFFIX")?;
    dashboard.wait_until(
        |screen| screen.contains("DRAFT_TOKEN_SUFFIX"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b[99;5u")?;
    dashboard.wait_until(
        |screen| screen.contains("Quick message") && !screen.contains("DRAFT_TOKEN"),
        wait_deadline(),
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn iterm2_control_hold_selects_an_agent_through_the_production_environment() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let first = create_search_agent(&mut fixture, "iterm-a")?;
    let second = create_search_agent(&mut fixture, "iterm-b")?;
    let mut dashboard = OuterDashboard::start_with_input_environment(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
        &[
            ("TERM_PROGRAM", "iTerm.app"),
            ("TERM_PROGRAM_VERSION", "3.7.7"),
        ],
    )?;
    dashboard.wait_until(|screen| screen.contains("iterm-b"), wait_deadline())?;
    // These physical-key packets were captured from native iTerm2 3.7.7.
    dashboard.send(b"\x1b[57442;5:1u")?;
    dashboard.wait_until(
        |screen| {
            screen
                .lines()
                .any(|row| row.contains("[1]") && row.contains("iterm-a"))
        },
        Duration::from_secs(3),
    )?;
    dashboard.send(b"\x1b[49;5:1u")?;
    dashboard.wait_until(
        |screen| screen.contains("Session selected"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b[57442;1:3u")?;
    dashboard.wait_until(
        |screen| screen.contains("Terminal mode") && screen.contains("READY_iterm-a"),
        wait_deadline(),
    )?;
    dashboard.send(b"ITERM_INPUT_TOKEN\r")?;
    let deadline = Instant::now() + wait_deadline();
    loop {
        if fixture
            .read_terminal(first.id)?
            .contains("RECEIVED_iterm-a_ITERM_INPUT_TOKEN")
        {
            break;
        }
        if Instant::now() >= deadline {
            bail!("iTerm jump destination did not receive post-release input");
        }
        thread::yield_now();
    }
    assert!(
        !fixture
            .read_terminal(second.id)?
            .contains("ITERM_INPUT_TOKEN")
    );
    Ok(())
}

#[test]
fn iterm2_metadata_through_a_proxy_keeps_control_input_out_of_browse_actions() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    create_search_agent(&mut fixture, "proxy-agent")?;
    let mut dashboard = OuterDashboard::start_with_input_environment(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
        &[
            ("TERM_PROGRAM", "iTerm.app"),
            ("TERM_PROGRAM_VERSION", "3.7.7"),
            ("TMUX", "fixture"),
        ],
    )?;
    dashboard.wait_until(|screen| screen.contains("proxy-agent"), wait_deadline())?;
    dashboard.send(b"\x1b[57442;5:1u\x1b[49;5:1u\x1b[57442;1:3u\x1b[59:58;2u")?;
    dashboard.wait_until(|screen| screen.contains("Command palette"), wait_deadline())?;
    assert!(!dashboard.rendered().contains("Session selected"));
    assert!(!dashboard.rendered().contains("[1]"));
    Ok(())
}
