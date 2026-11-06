use super::*;
use anyhow::Context;
use ovrcr::protocol::{CreateSessionRequest, SessionKind, SessionPhase, SessionRunId};

fn create_exited_agent(
    fixture: &mut AcceptanceFixture,
    name: &str,
) -> Result<ovrcr::protocol::SessionSummary> {
    let workspace = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .find(|workspace| workspace.name == "feature/acceptance")
        .context("fixture workspace")?;
    let response = fixture.request(Request::CreateSession(CreateSessionRequest {
        project: "fixture".into(),
        workspace: workspace.id,
        name: name.into(),
        label: None,
        kind: SessionKind::Agent {
            name: "fixture-agent".into(),
        },
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf 'RETAINED_PROCESS_EXITED\\n'".into(),
        ],
    }));
    fixture.managed_pgids = fixture.session_pgids()?;
    match response {
        Response::CreatedSession(session) => Ok(*session),
        other => bail!("retained Agent launch failed: {other:?}"),
    }
}

fn find_session(
    hierarchy: HierarchySnapshot,
    session: ovrcr::session::SessionId,
) -> Result<ovrcr::protocol::SessionSummary> {
    hierarchy
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|candidate| candidate.id == session)
        .context("session remains in the retained hierarchy")
}

fn wait_for_exited_agent(
    fixture: &AcceptanceFixture,
    session: ovrcr::session::SessionId,
) -> Result<ovrcr::protocol::SessionSummary> {
    let deadline = Instant::now() + wait_deadline();
    loop {
        let current = find_session(fixture.list()?, session)?;
        if matches!(current.phase, SessionPhase::Exited { .. }) && current.pid.is_none() {
            return Ok(current);
        }
        if Instant::now() >= deadline {
            bail!("fixture Agent did not become retained: {current:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn jump_digit(screen: &str, title: &str) -> Result<char> {
    let line = screen
        .lines()
        .find(|line| {
            line.split('│')
                .next()
                .is_some_and(|sidebar| sidebar.contains(title))
        })
        .with_context(|| format!("visible title row {title:?} missing from:\n{screen}"))?;
    ['1', '2']
        .into_iter()
        .find(|digit| line.contains(&format!("[{digit}]")))
        .with_context(|| format!("title row {title:?} has no jump label:\n{line}"))
}

fn command_press() -> &'static [u8] {
    b"\x1b[57444;1:1u"
}

fn command_release() -> &'static [u8] {
    b"\x1b[57444;1:3u"
}

fn command_digit(digit: char) -> Vec<u8> {
    format!("\x1b[{};9:1u", u32::from(digit)).into_bytes()
}

#[test]
fn retained_jump_inspects_without_restarting_then_running_jump_accepts_only_post_release_input()
-> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let retained = create_exited_agent(&mut fixture, "retained-agent")?;
    let retained = wait_for_exited_agent(&fixture, retained.id)?;
    assert_eq!(retained.run, SessionRunId(1));
    let running = create_search_agent(&mut fixture, "running-agent")?;
    let groups_before_inspection = {
        let mut groups = fixture.session_pgids()?;
        groups.sort_unstable();
        groups
    };

    let mut dashboard = OuterDashboard::start_with_verified_gui_profile(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.wait_for_screen(
        |screen| screen.contains("retained-agent") && screen.contains("running-agent"),
        wait_deadline(),
    )?;
    dashboard.send(command_press())?;
    dashboard.wait_for_screen(
        |screen| {
            screen.lines().any(|line| {
                line.split('│').next().is_some_and(|sidebar| {
                    sidebar.contains("retained-agent")
                        && (sidebar.contains("[1]") || sidebar.contains("[2]"))
                })
            })
        },
        wait_deadline(),
    )?;
    let retained_code = jump_digit(&dashboard.rendered(), "retained-agent");
    let retained_code = retained_code?;
    dashboard.send(&command_digit(retained_code))?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Session selected"),
        wait_deadline(),
    )?;
    dashboard.send(b"RETAINED_SELECTION_LEAK\r")?;
    dashboard.send(command_release())?;
    dashboard.wait_for_screen(
        |screen| screen.contains("BROWSE") && !screen.contains("Session selected"),
        wait_deadline(),
    )?;

    let stability_deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < stability_deadline {
        let current = find_session(fixture.list()?, retained.id)?;
        assert_eq!(
            current.run, retained.run,
            "inspection preserves the captured run"
        );
        assert!(matches!(current.phase, SessionPhase::Exited { .. }));
        assert!(current.pid.is_none(), "inspection must not start a process");
        thread::sleep(Duration::from_millis(10));
    }
    let groups_after_inspection = {
        let mut groups = fixture.session_pgids()?;
        groups.sort_unstable();
        groups
    };
    assert_eq!(
        groups_after_inspection, groups_before_inspection,
        "retained inspection must not create a process group"
    );
    assert!(
        !fixture
            .read_terminal(retained.id)?
            .contains("RETAINED_SELECTION_LEAK"),
        "keys held after retained selection must not reach an Agent"
    );

    dashboard.send(command_press())?;
    dashboard.wait_for_screen(
        |screen| {
            screen.lines().any(|line| {
                line.split('│').next().is_some_and(|sidebar| {
                    sidebar.contains("running-agent")
                        && (sidebar.contains("[1]") || sidebar.contains("[2]"))
                })
            })
        },
        wait_deadline(),
    )?;
    let running_code = jump_digit(&dashboard.rendered(), "running-agent")?;
    dashboard.send(&command_digit(running_code))?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Session selected"),
        wait_deadline(),
    )?;
    dashboard.send(command_release())?;
    dashboard.wait_for_screen(
        |screen| screen.contains("READY_running-agent") && screen.contains("Terminal mode"),
        wait_deadline(),
    )?;
    dashboard.send(b"RETAINED_JUMP_RUNNING_OK\r")?;
    dashboard.wait_for(
        b"RECEIVED_running-agent_RETAINED_JUMP_RUNNING_OK",
        wait_deadline(),
    )?;

    let final_retained = find_session(fixture.list()?, retained.id)?;
    assert_eq!(final_retained.run, retained.run);
    assert!(matches!(final_retained.phase, SessionPhase::Exited { .. }));
    assert!(final_retained.pid.is_none());
    assert!(
        fixture
            .read_terminal(running.id)?
            .contains("RECEIVED_running-agent_RETAINED_JUMP_RUNNING_OK")
    );
    for session in [retained.id, running.id] {
        let text = fixture.read_terminal(session)?;
        if session == retained.id {
            assert!(!text.contains("RETAINED_JUMP_RUNNING_OK"));
            assert!(!text.contains("RECEIVED_running-agent_RETAINED_JUMP_RUNNING_OK"));
        } else {
            assert!(text.contains("RECEIVED_running-agent_RETAINED_JUMP_RUNNING_OK"));
        }
    }

    eprintln!(
        "retained jump fixture: root={} socket={} retained_session={} retained_run={:?} running_session={} groups={:?}",
        fixture.root.path().display(),
        fixture.socket.display(),
        retained.id.0,
        retained.run,
        running.id.0,
        fixture.session_pgids()?
    );
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}
