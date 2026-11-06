use super::*;
use anyhow::Context;

fn command_press() -> &'static [u8] {
    b"\x1b[57444;1:1u"
}

fn command_release() -> &'static [u8] {
    b"\x1b[57444;1:3u"
}

fn command_digit(digit: char, kind: u8) -> Vec<u8> {
    format!("\x1b[{};9:{kind}u", u32::from(digit)).into_bytes()
}

#[test]
fn operator_command_jump_uses_the_live_agent_and_preserves_its_unsent_draft() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let target = create_search_agent(&mut fixture, "operator-agent")?;
    let old_program = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|session| session.name == "local")
        .context("original local shell")?;

    let mut dashboard = OuterDashboard::start_with_verified_gui_profile(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.wait_for_screen(dashboard_header_visible, wait_deadline())?;
    // The fixture may show its one-time optional status-off notice in place of the mode label.
    dashboard.send(b"\x1b")?;
    dashboard.wait_for_screen(|screen| screen.contains("BROWSE"), wait_deadline())?;
    dashboard.send(b"o")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("New Conversation"),
        wait_deadline(),
    )?;
    // Create only a local Operator conversation; no model turn is submitted.
    dashboard.send(b"\r")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Operator ·") && screen.contains("ready"),
        wait_deadline(),
    )?;
    dashboard.send(b"OPERATOR_DRAFT")?;
    dashboard.wait_for_screen(|screen| screen.contains("OPERATOR_DRAFT"), wait_deadline())?;

    dashboard.send(command_press())?;
    dashboard.wait_for_screen(
        |screen| {
            screen.lines().any(|line| {
                line.split('│').next().is_some_and(|sidebar| {
                    sidebar.contains("operator-agent") && sidebar.contains("[1]")
                })
            })
        },
        wait_deadline(),
    )?;
    dashboard.send(&command_digit('1', 1))?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Session selected"),
        wait_deadline(),
    )?;
    // A repeat and another code after selection cannot type into the composer or target.
    dashboard.send(&command_digit('1', 2))?;
    dashboard.send(&command_digit('2', 1))?;
    dashboard.send(command_release())?;
    dashboard.wait_for_screen(
        |screen| screen.contains("READY_operator-agent") && screen.contains("Terminal mode"),
        wait_deadline(),
    )?;
    dashboard.send(b"OPERATOR_TO_AGENT_OK\r")?;
    dashboard.wait_for(
        b"RECEIVED_operator-agent_OPERATOR_TO_AGENT_OK",
        wait_deadline(),
    )?;

    let target_text = fixture.read_terminal(target.id)?;
    assert!(target_text.contains("RECEIVED_operator-agent_OPERATOR_TO_AGENT_OK"));
    assert!(
        !target_text.contains("RECEIVED_operator-agent_1")
            && !target_text.contains("RECEIVED_operator-agent_2"),
        "Command codes or repeats reached the target Agent: {target_text}"
    );
    let old_text = fixture.read_terminal(old_program.id)?;
    assert!(
        !old_text.contains("OPERATOR_TO_AGENT_OK")
            && !old_text.contains("OPERATOR_DRAFT")
            && !old_text.contains("RECEIVED_local_1"),
        "Operator input or the destination marker reached the original program: {old_text}"
    );

    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(|screen| screen.contains("BROWSE"), wait_deadline())?;
    dashboard.send(b"o")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("OPERATOR_DRAFT") && screen.contains("Operator ·"),
        wait_deadline(),
    )?;
    let returned = dashboard.rendered();
    assert!(
        returned.contains("OPERATOR_DRAFT"),
        "draft returned with Operator: {returned}"
    );
    assert!(
        !returned.contains("OPERATOR_DRAFT1") && !returned.contains("OPERATOR_DRAFT2"),
        "Command codes or repeats became composer text: {returned}"
    );

    eprintln!(
        "Operator jump fixture: root={} socket={} old_session={} target_session={} groups={:?}",
        fixture.root.path().display(),
        fixture.socket.display(),
        old_program.id.0,
        target.id.0,
        fixture.managed_pgids
    );
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}
