use super::*;

#[test]
fn single_digit_jump_selects_the_ninth_agent_through_the_compiled_dashboard() -> Result<()> {
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
    let proxy_socket = actual_socket.with_file_name("single-digit-view-gate.sock");
    let (held, release, proxy) = gate_agent_view(&proxy_socket, &actual_socket, agents[8].id)?;
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
    dashboard.send(b"\x1b[57442;1:1u")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("[1]") && screen.contains("[9]"),
        wait_deadline(),
    )?;
    let labeled_screen = dashboard.rendered();
    for index in 1..=9 {
        let row = labeled_screen
            .lines()
            .find(|line| line.contains(&format!("jump-{index:02}")))
            .with_context(|| format!("visible Agent row {index}"))?;
        assert!(row.contains(&format!("[{index}]")), "{row}");
    }
    let tenth_row = labeled_screen
        .lines()
        .find(|line| line.contains("jump-10"))
        .context("visible tenth Agent row")?;
    assert!(
        !tenth_row.contains("[10]") && !tenth_row.contains("[1]"),
        "tenth Agent remains visible and unlabelled: {tenth_row}"
    );

    dashboard.send(b"\x1b[48;5:1u")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("No visible Agent for that number"),
        wait_deadline(),
    )?;
    assert!(
        !dashboard.rendered().contains("Session selected"),
        "zero does not select a target"
    );

    dashboard.send(b"\x1b[57;5:1u")?;
    held.recv_timeout(wait_deadline())?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Session selected"),
        wait_deadline(),
    )?;

    // The completed jump is latched until Control-up. No input before its
    // release and the gated view acknowledgement may reach the destination.
    dashboard.send(b"CODE_HOLD_REJECT\r")?;
    dashboard.send(b"\x1b[57442;1:3u")?;
    dashboard.send(b"\x1b[200~READY_GATE_PASTE_REJECT\x1b[201~")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("paste was not sent"),
        wait_deadline(),
    )?;
    release.send(())?;
    dashboard.wait_for_screen(|screen| screen.contains("Terminal mode"), wait_deadline())?;
    dashboard.wait_for_screen(|screen| screen.contains("READY_jump-09"), wait_deadline())?;
    dashboard.send(b"JUMP_NINE_OK\r")?;
    dashboard.wait_for(b"RECEIVED_jump-09_JUMP_NINE_OK", wait_deadline())?;

    for agent in &agents {
        let terminal = fixture.read_terminal(agent.id)?;
        assert!(
            !terminal.contains("CODE_HOLD_REJECT") && !terminal.contains("READY_GATE_PASTE_REJECT"),
            "input before Control-up reached Agent {}: {terminal}",
            agent.name
        );
        if agent.id != agents[8].id {
            assert!(
                !terminal.contains("JUMP_NINE_OK"),
                "payload reached an unintended Agent {}: {terminal}",
                agent.name
            );
        }
    }
    let destination = fixture.read_terminal(agents[8].id)?;
    assert!(destination.contains("RECEIVED_jump-09_JUMP_NINE_OK"));
    assert!(!destination.contains("RECEIVED_jump-09_9"));

    dashboard.detach()?;
    proxy
        .join()
        .map_err(|_| anyhow::anyhow!("view proxy panicked"))??;
    fixture.shutdown()?;
    Ok(())
}
