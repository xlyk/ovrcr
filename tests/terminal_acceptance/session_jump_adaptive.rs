use super::*;

#[test]
fn two_digit_jump_selects_the_tenth_agent_through_the_compiled_dashboard() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let mut agents = Vec::new();
    for index in 1..=10 {
        agents.push(create_search_agent(
            &mut fixture,
            &format!("jump-{index:02}"),
        )?);
    }

    let actual_socket = fixture.live.socket.clone();
    let proxy_socket = actual_socket.with_file_name("two-digit-view-gate.sock");
    let (held, release, proxy) = gate_agent_view(&proxy_socket, &actual_socket, agents[9].id)?;
    let proxy_executable = dashboard_proxy::publish_build(&proxy_socket)?;
    fixture.live.socket = proxy_socket;
    let outer = OuterDashboard::start_with_server_executable_and_verified_gui_profile(
        &fixture,
        PtySize {
            rows: 50,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
        &proxy_executable,
    );
    fixture.live.socket = actual_socket;
    let mut dashboard = outer?;
    dashboard.send(b"\x1b[57444;1:1u")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("[01]") && screen.contains("[10]"),
        wait_deadline(),
    )?;
    let labeled_screen = dashboard.rendered();
    let tenth_row = labeled_screen
        .lines()
        .find(|line| line.contains("jump-10"))
        .context("visible tenth Agent row")?;
    assert!(
        tenth_row.contains("[10]"),
        "label follows sidebar order: {tenth_row}"
    );

    dashboard.send(b"\x1b[49;9:1u")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Cmd+1") && screen.contains("next digit"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b[48;9:1u")?;
    held.recv_timeout(wait_deadline())?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Session selected"),
        wait_deadline(),
    )?;

    // The completed code is latched until Command-up; no bytes from the
    // completed gesture may enter the destination or another Agent. Release
    // Command before the gated final view acknowledgment to cover that ordering.
    dashboard.send(b"CODE_HOLD_REJECT\r")?;
    dashboard.send(b"\x1b[57444;1:3u")?;
    dashboard.send(b"\x1b[200~READY_GATE_PASTE_REJECT\x1b[201~")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("paste was not sent"),
        wait_deadline(),
    )?;
    release.send(())?;
    dashboard.wait_for_screen(|screen| screen.contains("Terminal mode"), wait_deadline())?;
    dashboard.wait_for_screen(|screen| screen.contains("READY_jump-10"), wait_deadline())?;
    dashboard.send(b"JUMP_TEN_OK\r")?;
    dashboard.wait_for(b"RECEIVED_jump-10_JUMP_TEN_OK", wait_deadline())?;

    for agent in &agents {
        let terminal = fixture.read_terminal(agent.id)?;
        assert!(
            !terminal.contains("CODE_HOLD_REJECT") && !terminal.contains("READY_GATE_PASTE_REJECT"),
            "input before Command-up reached Agent {}: {terminal}",
            agent.name
        );
        if agent.id != agents[9].id {
            assert!(
                !terminal.contains("JUMP_TEN_OK"),
                "payload reached an unintended Agent {}: {terminal}",
                agent.name
            );
        }
    }
    let destination = fixture.read_terminal(agents[9].id)?;
    assert!(destination.contains("RECEIVED_jump-10_JUMP_TEN_OK"));
    assert!(!destination.contains("RECEIVED_jump-10_10"));

    dashboard.detach()?;
    proxy
        .join()
        .map_err(|_| anyhow::anyhow!("view proxy panicked"))??;
    fixture.shutdown()?;
    Ok(())
}
