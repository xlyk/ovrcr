//! Actual outer-PTY Dashboard input and real assigned managed child.
use super::*;
use ovrcr::protocol::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn wait_file(path: &Path) -> Result<()> {
    let deadline = Instant::now() + wait_deadline();
    while !path.exists() {
        if Instant::now() >= deadline {
            bail!("missing fixture file {}", path.display());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn tool(
    fixture: &AcceptanceFixture,
    parent: SessionId,
    name: &str,
    args: serde_json::Value,
    file: &str,
) -> Result<serde_json::Value> {
    let output = fixture.root.path().join(file);
    let command = format!(
        "tool:{}",
        serde_json::json!({"name":name,"args":args,"output":output})
    );
    if fixture.request(Request::SendTerminal {
        session: parent,
        text: command,
        submit: true,
    }) != Response::Ok
    {
        bail!("parent tool send failed");
    }
    wait_file(&output)?;
    let result: serde_json::Value = serde_json::from_slice(&fs::read(&output)?)?;
    if result.get("error").is_some() {
        bail!("fixture tool failed: {result}");
    }
    Ok(result["details"].clone())
}

fn fixture() -> Result<(AcceptanceFixture, AssignmentSnapshot, SessionId)> {
    let live = Live::idle().bounded();
    let bin = live.claude.command.parent().context("fixture bin")?;
    fs::write(
        bin.join("pi"),
        include_bytes!("../fixtures/pi/research_pi.sh"),
    )?;
    fs::set_permissions(bin.join("pi"), fs::Permissions::from_mode(0o700))?;
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
    let gate = live.root.path().join("result-gate");
    fs::write(&gate, "1")?;
    let bytes = live.root.path().join("input-bytes");
    fs::create_dir(&bytes)?;
    live.start_binary_env(&[
        ("OVRCR_TEST_DELEGATION", "1".as_ref()),
        ("OVRCR_TEST_PI_HOST", host.as_os_str()),
        ("OVRCR_TEST_DELEGATION_RESULT_GATE", gate.as_os_str()),
        ("OVRCR_TEST_DELEGATION_BYTES", bytes.as_os_str()),
    ]);
    live.ready("takeover-outer");
    let mut fixture = AcceptanceFixture {
        live,
        managed_pgids: vec![],
    };
    let workspace = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|p| p.workspaces)
        .find(|w| w.name == "takeover-outer")
        .context("research workspace")?
        .id;
    let Response::CreatedSession(parent) =
        fixture.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(),
            workspace: workspace.clone(),
            name: "takeover-parent".into(),
            label: None,
            argv: vec![
                env!("CARGO_BIN_EXE_ovrcr").into(),
                "agent".into(),
                "run".into(),
                "pi".into(),
                "--".into(),
                "pi".into(),
                "--model".into(),
                "fixture-model".into(),
            ],
            kind: SessionKind::Agent { name: "pi".into() },
        }))
    else {
        bail!("parent launch failed");
    };
    fixture.managed_pgids = fixture.session_pgids()?;
    let deadline = Instant::now() + wait_deadline();
    loop {
        let Response::Inventory { sessions, .. } = fixture.request(Request::Inspect) else {
            bail!("inventory");
        };
        if sessions
            .iter()
            .any(|s| s.id == parent.id && s.agent.is_some())
        {
            break;
        }
        if Instant::now() >= deadline {
            bail!("parent binding timeout");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let assignment: AssignmentSnapshot = serde_json::from_value(tool(
        &fixture,
        parent.id,
        "ovrcr_start",
        serde_json::json!({
        "project":"fixture","workspace":workspace,"scope":"Read README only","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}),
        "start.json",
    )?)?;
    wait_file(&assignment.brief.with_extension("json.published"))?;
    fixture.managed_pgids = fixture.session_pgids()?;
    Ok((fixture, assignment, parent.id))
}

#[test]
fn assigned_child_is_view_only_until_explicit_dashboard_takeover_and_return() -> Result<()> {
    let (mut fixture, assignment, _parent) = fixture()?;
    let child = assignment.identity.session.context("assigned child")?;
    let before_title = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|p| p.workspaces)
        .flat_map(|w| w.sessions)
        .find(|s| s.id == child)
        .context("child before title")?;
    let title = "takeover-child-323";
    assert_eq!(
        fixture.request(Request::SetSessionTitle {
            session: child,
            title: Some(title.into()),
        }),
        Response::Ok
    );
    let renamed = fixture.list()?;
    assert_eq!(
        renamed
            .projects
            .iter()
            .flat_map(|p| &p.workspaces)
            .flat_map(|w| &w.sessions)
            .filter(|s| s.display_name() == title)
            .count(),
        1
    );
    // In this fresh fixture the child is last; split wraps to the local terminal.
    let other = renamed
        .projects
        .iter()
        .flat_map(|p| &p.workspaces)
        .flat_map(|w| &w.sessions)
        .find(|s| s.workspace == before_title.workspace && s.name == "local")
        .context("other local pane")?
        .clone();
    assert_eq!(other.kind, SessionKind::Terminal);
    let initial = renamed
        .projects
        .into_iter()
        .flat_map(|p| p.workspaces)
        .flat_map(|w| w.sessions)
        .find(|s| s.id == child)
        .context("child summary")?;
    assert_eq!(initial.display_name(), title);
    assert_eq!(initial.run, before_title.run);
    assert_eq!(initial.assignment_control, before_title.assignment_control);
    assert_eq!(
        initial.assignment_control.as_ref().unwrap().control,
        AssignmentControl::default()
    );
    let candidate = format!("› {title} (#{})", child.0);
    let sole_child_candidate = |screen: &str| {
        let rows: Vec<_> = screen
            .lines()
            .skip_while(|line| !line.contains("┌ Agents"))
            .take_while(|line| !line.contains('└'))
            .filter(|line| line.contains("(#"))
            .collect();
        rows.len() == 1 && rows[0].contains(&candidate)
    };
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.send(format!("s{}", initial.display_name()).as_bytes())?;
    dashboard.wait_for_screen(sole_child_candidate, wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains(title)
                && screen.contains("Pi control")
                && screen.contains("view only")
                && screen.contains("CHILD_RESULT_PUBLISHED")
                && !screen.contains("┌ Agents")
        },
        wait_deadline(),
    )?;
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(|screen| screen.contains("BROWSE"), wait_deadline())?;
    dashboard.send(b"\x1b[200~FORBIDDEN_BEFORE_TAKEOVER\x1b[201~")?;
    // Passive split/focus/resize/hide affects only view subscriptions. The
    // unread child metadata hides geometry, so use the local pane's ready
    // dimensions: the global handshake acknowledges all target snapshots.
    let split = ovrcr::tui::pane_rects(ratatui::layout::Rect::new(0, 0, 160, 40), 2, 0);
    let other_ready = format!(
        "{} {}x{}",
        other.display_name(),
        split[1].terminal.width,
        split[1].terminal.height
    );
    let metadata = |screen: &str, rect: ratatui::layout::Rect| -> String {
        screen
            .lines()
            .skip(usize::from(rect.y))
            .take(usize::from(rect.height))
            .map(|line| {
                line.chars()
                    .skip(usize::from(rect.x))
                    .take(usize::from(rect.width))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    dashboard.send(b"v")?;
    dashboard.wait_for_screen(
        |screen| {
            metadata(screen, split[1].metadata).starts_with(&format!("> {other_ready}"))
                && metadata(screen, split[0].terminal).contains("CHILD_RESULT_PUBLISHED")
        },
        wait_deadline(),
    )?;
    dashboard.find_text_in_rect(split[0].terminal, "CHILD_RESULT_PUBLISHED")?;
    dashboard.send(b"\t")?;
    dashboard.wait_for_screen(
        |screen| {
            metadata(screen, split[1].metadata).starts_with(&format!("  {other_ready}"))
                && metadata(screen, split[0].terminal).contains("CHILD_RESULT_PUBLISHED")
        },
        wait_deadline(),
    )?;
    dashboard.find_text_in_rect(split[0].terminal, "CHILD_RESULT_PUBLISHED")?;
    dashboard.resize(42, 170)?;
    let resized = ovrcr::tui::pane_rects(ratatui::layout::Rect::new(0, 0, 170, 42), 2, 0);
    let resized_other = format!(
        "  {} {}x{}",
        other.display_name(),
        resized[1].terminal.width,
        resized[1].terminal.height
    );
    dashboard.wait_for_screen(
        |screen| {
            metadata(screen, resized[1].metadata).starts_with(&resized_other)
                && metadata(screen, resized[0].terminal).contains("CHILD_RESULT_PUBLISHED")
        },
        wait_deadline(),
    )?;
    dashboard.find_text_in_rect(resized[0].terminal, "CHILD_RESULT_PUBLISHED")?;
    dashboard.send(b"x")?;
    let other_pid = other.pid.context("local pane PID")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("BROWSE")
                && screen.contains(other.display_name())
                && screen.contains(&format!("pid: {other_pid}"))
                && !screen.contains("CHILD_RESULT_PUBLISHED")
                && screen.lines().all(|line| {
                    line.chars().nth(usize::from(resized[0].terminal.right())) != Some('│')
                })
        },
        wait_deadline(),
    )?;
    dashboard.send(format!("s{}", initial.display_name()).as_bytes())?;
    dashboard.wait_for_screen(sole_child_candidate, wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains(title)
                && screen.contains("Pi control")
                && screen.contains("view only")
                && screen.contains("CHILD_RESULT_PUBLISHED")
        },
        wait_deadline(),
    )?;
    let before = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|p| p.workspaces)
        .flat_map(|w| w.sessions)
        .find(|s| s.id == child)
        .context("viewed child")?;
    assert_eq!(before.run, initial.run);
    assert_eq!(before.phase, SessionPhase::Running);
    assert_eq!(
        before.assignment_control.as_ref().unwrap().control,
        AssignmentControl::default()
    );
    assert!(
        !fixture
            .read_terminal(child)?
            .contains("FORBIDDEN_BEFORE_TAKEOVER")
    );
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(|screen| screen.contains("BROWSE"), wait_deadline())?;
    dashboard.send(b"H")?;
    dashboard.wait_for_screen(|screen| screen.contains("Human control"), wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.wait_for_screen(|screen| screen.contains("Terminal mode"), wait_deadline())?;
    dashboard.send(b"human_marker:OUTER_TAKEOVER_INPUT_OK_323\r")?;
    dashboard.wait_for(
        b"RECEIVED_HUMAN_OUTER_TAKEOVER_INPUT_OK_323",
        wait_deadline(),
    )?;
    assert!(
        fixture
            .read_terminal(child)?
            .contains("RECEIVED_HUMAN_OUTER_TAKEOVER_INPUT_OK_323")
    );
    dashboard.send(b"\x07A")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("return pending reconciliation"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b[200~FORBIDDEN_AFTER_RETURN\x1b[201~")?;
    assert!(
        !fixture
            .read_terminal(child)?
            .contains("FORBIDDEN_AFTER_RETURN")
    );
    let after = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|p| p.workspaces)
        .flat_map(|w| w.sessions)
        .find(|s| s.id == child)
        .context("returned child")?;
    let control = &after.assignment_control.as_ref().unwrap().control;
    assert_eq!(after.run, initial.run);
    assert_eq!(after.phase, SessionPhase::Running);
    assert_eq!(control.owner, ControlOwner::Controller);
    assert_eq!(control.epoch, 3);
    assert!(matches!(
        control.barrier,
        Some(ControlBarrier::HumanReturn { .. })
    ));
    eprintln!(
        "OUTER_323 root={} socket={} groups={:?} marker=RECEIVED_HUMAN_OUTER_TAKEOVER_INPUT_OK_323 epoch={}",
        fixture.root.path().display(),
        fixture.socket.display(),
        fixture.managed_pgids,
        control.epoch
    );
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}
