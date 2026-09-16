#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT, WORKSPACE};
use ovrcr::protocol::{
    BranchRequest, CreateSessionRequest, ErrorCode, Request, Response, ServerEvent, ServerMessage,
    SessionId, SessionLaunch, SessionPhase, SessionSummary, client, connect_server, read_frame,
};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

fn fixture() -> Live {
    let live = Live::thread().bounded();
    assert_eq!(
        live.request(Request::AddProject {
            name: PROJECT.into(),
            repo: live.repo.clone(),
            workspace_root: live.workspace_root.clone(),
        }),
        Response::Ok
    );
    live
}

fn workspace(launch: Option<SessionLaunch>) -> Request {
    Request::CreateWorkspaceWithLaunch {
        project: PROJECT.into(),
        name: WORKSPACE.into(),
        branch: BranchRequest::New {
            branch: "title-work".into(),
            base: "main".into(),
        },
        launch,
    }
}

fn title_program() -> SessionLaunch {
    SessionLaunch { kind: ovrcr_protocol::SessionKind::Terminal, argv: vec!["/bin/sh".into(), "-c".into(),
        "stty -echo; printf 'TITLE-READY\\n'; while IFS= read -r title; do [ \"$title\" = EXIT ] && exit 0; printf '\\033]2;%s\\007applied:%s\\n' \"$title\" \"$title\"; done".into()],
    label: Some("owned title fixture".into()), }
}

fn created(live: &Live, request: Request) -> SessionSummary {
    let response = live.request(request);
    let Response::CreatedSession(summary) = response else {
        panic!(
            "session creation failed: {response:?}; inventory={:?}",
            sessions(live)
        )
    };
    if let Some(pid) = summary.pid {
        live.own_group(pid as libc::pid_t);
    }
    *summary
}

fn sessions(live: &Live) -> Vec<SessionSummary> {
    let Response::Hierarchy(hierarchy) = live.request(Request::List) else {
        panic!("list failed")
    };
    hierarchy
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .collect()
}

fn terminal(live: &Live, session: SessionId) -> String {
    match live.request(Request::ReadTerminal {
        session,
        max_lines: None,
    }) {
        Response::TerminalText { text, .. } => text,
        other => panic!("unexpected terminal response: {other:?}"),
    }
}

fn wait_for(live: &Live, session: SessionId, marker: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !terminal(live, session).contains(marker) {
        assert!(Instant::now() < deadline, "missing output {marker}");
        std::thread::yield_now();
    }
}

fn emit(live: &Live, session: SessionId, title: &str) {
    assert_eq!(
        live.request(Request::SendTerminal {
            session,
            text: title.into(),
            submit: true
        }),
        Response::Ok
    );
    wait_for(live, session, &format!("applied:{title}"));
}

fn dashboard(live: &Live) -> UnixStream {
    let mut stream = connect_server(&live.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let response = client::request(&mut stream, 1, Request::DashboardHello).unwrap();
    assert!(matches!(response, Response::Hierarchy(_)), "{response:?}");
    stream
}

fn changed(stream: &mut UnixStream, id: SessionId, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "missing title event {expected}");
        match read_frame::<ServerMessage>(stream).unwrap() {
            ServerMessage::Event(ServerEvent::SessionChanged(summary))
                if summary.id == id && summary.display_name() == expected =>
            {
                return;
            }
            ServerMessage::Event(ServerEvent::Output { .. }) => {
                panic!("hidden session must not deliver screen output")
            }
            _ => {}
        }
    }
}

#[test]
fn automatic_workspace_launch_titles_pin_reset_and_reopen_keep_identity_and_reset_output() {
    let live = fixture();
    let original = created(&live, workspace(Some(title_program())));
    assert_eq!(original.name, WORKSPACE);
    assert_eq!(original.display_name(), WORKSPACE);
    assert_eq!(original.label, "owned title fixture");
    assert_eq!(
        sessions(&live).len(),
        1,
        "selected launch replaces default shell"
    );
    wait_for(&live, original.id, "TITLE-READY");
    let mut dashboard = dashboard(&live); // no pane selected: output remains hidden
    emit(&live, original.id, "first title 🦀");
    changed(&mut dashboard, original.id, "first title 🦀");
    assert_eq!(
        live.request(Request::SetSessionTitle {
            session: original.id,
            title: Some("  pinned  ".into())
        }),
        Response::Ok
    );
    changed(&mut dashboard, original.id, "pinned");
    emit(&live, original.id, "latest application");
    assert_eq!(sessions(&live)[0].display_name(), "pinned");
    assert_eq!(
        live.request(Request::SetSessionTitle {
            session: original.id,
            title: None
        }),
        Response::Ok
    );
    changed(&mut dashboard, original.id, "latest application");
    assert_eq!(sessions(&live)[0].name, WORKSPACE);
    let reused = created(
        &live,
        Request::ReopenSession {
            session: original.id,
            expected_run: original.run,
            acknowledge_stopped: false,
        },
    );
    assert_eq!(
        (reused.id, reused.run, reused.pid),
        (original.id, original.run, original.pid)
    );
    assert!(matches!(
        live.request(Request::SetSessionTitle {
            session: original.id,
            title: Some(" \n\t".into())
        }),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert_eq!(
        live.request(Request::SendTerminal {
            session: original.id,
            text: "EXIT".into(),
            submit: true
        }),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(sessions(&live)[0].phase, SessionPhase::Exited { .. }) {
        assert!(Instant::now() < deadline, "session did not exit");
        std::thread::yield_now();
    }
    assert!(terminal(&live, original.id).contains("TITLE-READY"));
    let reopened = created(
        &live,
        Request::ReopenSession {
            session: original.id,
            expected_run: original.run,
            acknowledge_stopped: false,
        },
    );
    assert_eq!(reopened.id, original.id);
    assert_eq!(reopened.run.0, original.run.0 + 1);
    assert_eq!(reopened.name, original.name);
    assert_eq!(reopened.display_name(), "latest application");
    assert!(!terminal(&live, reopened.id).contains("TITLE-READY"));
    assert_eq!(
        live.request(Request::SendTerminal {
            session: reopened.id,
            text: r"printf '\033]2;fresh title\007'; printf 'SHELL_%s\n' TITLE_SET".into(),
            submit: true,
        }),
        Response::Ok
    );
    wait_for(&live, reopened.id, "SHELL_TITLE_SET");
    changed(&mut dashboard, reopened.id, "fresh title");
    assert_eq!(sessions(&live).len(), 1);
}

#[test]
fn automatic_empty_workspace_and_unique_names_preserve_explicit_titles() {
    let live = fixture();
    assert_eq!(live.request(workspace(None)), Response::Ok);
    assert!(sessions(&live).is_empty());
    let registry = ovrcr::config::load_registry(&live.config).unwrap();
    assert!(
        registry
            .workspace(PROJECT, WORKSPACE)
            .unwrap()
            .path
            .join(".git")
            .is_file()
    );
    let new = |name: &str| {
        let launch = title_program();
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: name.into(),
            label: launch.label,
            argv: launch.argv,
        })
    };
    let explicit = created(&live, new(WORKSPACE));
    let automatic = created(&live, new(""));
    assert_eq!(automatic.name, format!("{WORKSPACE}-2"));
    wait_for(&live, explicit.id, "TITLE-READY");
    emit(&live, explicit.id, "application override");
    assert_eq!(
        sessions(&live)
            .into_iter()
            .find(|session| session.id == explicit.id)
            .unwrap()
            .display_name(),
        WORKSPACE
    );
    assert!(matches!(
        live.request(new(WORKSPACE)),
        Response::Error {
            code: ErrorCode::AlreadyExists,
            ..
        }
    ));
    assert_eq!(
        live.request(Request::SendTerminal {
            session: explicit.id,
            text: "EXIT".into(),
            submit: true
        }),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !sessions(&live).into_iter().any(|session| {
        session.id == explicit.id && matches!(session.phase, SessionPhase::Exited { .. })
    }) {
        assert!(Instant::now() < deadline, "explicit session did not exit");
        std::thread::yield_now();
    }
    let reopened = created(
        &live,
        Request::ReopenSession {
            session: explicit.id,
            expected_run: explicit.run,
            acknowledge_stopped: false,
        },
    );
    assert_eq!(reopened.id, explicit.id);
    assert_eq!(reopened.run.0, explicit.run.0 + 1);
    assert_eq!(reopened.name, explicit.name);
    assert_eq!(
        reopened.display_name(),
        WORKSPACE,
        "reopen keeps pinned title"
    );
    assert_eq!(
        live.request(Request::SendTerminal {
            session: reopened.id,
            text: r"printf '\033]2;still pinned\007'; printf 'PIN_%s\n' KEPT".into(),
            submit: true,
        }),
        Response::Ok
    );
    wait_for(&live, reopened.id, "PIN_KEPT");
    assert_eq!(
        sessions(&live)
            .into_iter()
            .find(|session| session.id == reopened.id)
            .unwrap()
            .display_name(),
        WORKSPACE,
    );
}

#[test]
fn automatic_workspace_launch_failure_retains_worktree_and_retry_uses_existing_workspace() {
    let live = fixture();
    let response = live.request(workspace(Some(SessionLaunch {
        kind: ovrcr_protocol::SessionKind::Terminal,
        argv: vec!["/does-not-exist/ovrcr-owned-test".into()],
        label: None,
    })));
    assert!(
        matches!(
            response,
            Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            }
        ),
        "{response:?}"
    );
    let registry = ovrcr::config::load_registry(&live.config).unwrap();
    assert!(
        registry
            .workspace(PROJECT, WORKSPACE)
            .unwrap()
            .path
            .join(".git")
            .is_file()
    );
    assert!(sessions(&live).is_empty());
    assert!(matches!(
        live.request(workspace(None)),
        Response::Error {
            code: ErrorCode::AlreadyExists,
            ..
        }
    ));
    let launch = title_program();
    let retry = created(
        &live,
        Request::CreateSession(CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: PROJECT.into(),
            workspace: WORKSPACE.into(),
            name: String::new(),
            argv: launch.argv,
            label: launch.label,
        }),
    );
    wait_for(&live, retry.id, "TITLE-READY");
    assert_eq!(retry.name, WORKSPACE);
    assert!(matches!(
        live.request(Request::ReopenSession {
            session: SessionId(u64::MAX),
            expected_run: ovrcr::protocol::SessionRunId(1),
            acknowledge_stopped: false,
        }),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
}
