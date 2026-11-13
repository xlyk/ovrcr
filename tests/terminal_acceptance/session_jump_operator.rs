use super::*;
use anyhow::Context;

fn ctrl_g() -> &'static [u8] {
    b"\x1b[103;5:1u"
}

fn digit(digit: char) -> Vec<u8> {
    digit.to_string().into_bytes()
}

#[test]
fn operator_ctrl_g_uses_the_live_agent_and_preserves_its_unsent_draft() -> Result<()> {
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

    let mut dashboard = OuterDashboard::start(
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
    // The full view opens the one conversation directly; no form or chat list exists.
    dashboard.wait_for_screen(
        |screen| screen.contains("Operator ·") && screen.contains("ready"),
        wait_deadline(),
    )?;
    assert!(
        !dashboard.rendered().contains("New Conversation"),
        "the singleton has no creation form: {}",
        dashboard.rendered()
    );
    dashboard.send(b"OPERATOR_DRAFT")?;
    dashboard.wait_for_screen(|screen| screen.contains("OPERATOR_DRAFT"), wait_deadline())?;

    dashboard.send(ctrl_g())?;
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
    // The fixed Operator navigation row is not a conversation to jump into.
    let picker = dashboard.rendered();
    let singleton_row = picker
        .lines()
        .find(|line| {
            line.split('│').next().is_some_and(|sidebar| {
                sidebar.contains("󰚩 Operator") && !sidebar.contains("operator-agent")
            })
        })
        .context("fixed Operator navigation row")?;
    assert!(
        !singleton_row.contains("[1]"),
        "the Operator conversation is not a jump target: {singleton_row}"
    );
    dashboard.send(&digit('1'))?;
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
    let old_text = fixture.read_terminal(old_program.id)?;
    assert!(
        !old_text.contains("OPERATOR_TO_AGENT_OK")
            && !old_text.contains("OPERATOR_DRAFT")
            && !old_text.contains("RECEIVED_local_1"),
        "Operator input or the destination marker reached the original program: {old_text}"
    );

    dashboard.send(b"\x07\x07")?;
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
