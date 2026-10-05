#[path = "support/live.rs"]
mod live;

use live::{Live, git};
use ovrcr::protocol::{
    BranchRequest, CreateSessionRequest, Request, Response, SessionId, SessionKind,
    WorkspaceSummary,
};
use std::time::{Duration, Instant};

#[test]
fn workspace_tracks_checkout_without_replacing_its_directory() {
    let fixture = Live::binary();
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
        }),
        Response::Ok
    );
    assert!(matches!(
        fixture.request(Request::CreateWorkspaceWithLaunch {
            project: "fixture".into(),
            id: "arbitrary".into(),
            branch: BranchRequest::New {
                branch: "feature/first".into(),
                base: "main".into(),
            },
            launch: None,
        }),
        Response::Ok
    ));
    let Response::Hierarchy(before) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    let workspace = before.projects[0]
        .workspaces
        .iter()
        .find(|workspace| workspace.path != fixture.repo)
        .unwrap();
    assert_eq!(workspace.name, "feature/first");
    let directory = workspace.path.clone();
    let identity = workspace.id.clone();
    let Response::CreatedSession(shell) = launch(&fixture, &identity) else {
        panic!("expected feature shell");
    };
    let mut dashboard = ovrcr::protocol::connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert!(matches!(
        ovrcr::protocol::client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    git(&directory, &["switch", "-c", "feature/second"]);
    loop {
        let message = ovrcr::protocol::read_frame::<ovrcr::protocol::ServerMessage>(&mut dashboard)
            .expect("branch change must reach an idle Dashboard");
        if let ovrcr::protocol::ServerMessage::Event(
            ovrcr::protocol::ServerEvent::HierarchyChanged(hierarchy),
        ) = message
            && hierarchy
                .projects
                .iter()
                .flat_map(|project| &project.workspaces)
                .any(|workspace| workspace.id == identity && workspace.name == "feature/second")
        {
            break;
        }
    }
    let Response::Hierarchy(after) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    let workspace = after.projects[0]
        .workspaces
        .iter()
        .find(|workspace| workspace.path == directory)
        .unwrap();
    assert_eq!(workspace.name, "feature/second");
    assert_eq!(workspace.path, directory);
    assert_eq!(workspace.id, identity);
    assert_eq!(workspace.sessions[0].id, shell.id);
    command_output(
        &fixture,
        shell.id,
        "printf 'BRANCH_%s\\n' ALIVE",
        "BRANCH_ALIVE",
    );
    git(&directory, &["switch", "--detach", "HEAD"]);
    let Response::Hierarchy(detached) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    let workspace = detached.projects[0]
        .workspaces
        .iter()
        .find(|workspace| workspace.id == identity)
        .unwrap();
    assert!(workspace.name.starts_with("detached @ "));
    assert_eq!(workspace.sessions[0].id, shell.id);
    command_output(
        &fixture,
        shell.id,
        "printf 'DETACHED_%s\\n' ALIVE",
        "DETACHED_ALIVE",
    );
    git(&directory, &["switch", "feature/first"]);
    let Response::Hierarchy(restored) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    let workspace = restored.projects[0]
        .workspaces
        .iter()
        .find(|workspace| workspace.id == identity)
        .unwrap();
    assert_eq!(workspace.name, "feature/first");
    assert_eq!(workspace.sessions[0].id, shell.id);
}

fn register(fixture: &Live) -> Response {
    fixture.request(Request::AddProject {
        name: "fixture".into(),
        repo: fixture.repo.clone(),
        workspace_root: fixture.workspace_root.clone(),
    })
}

fn root_workspace(fixture: &Live) -> WorkspaceSummary {
    let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    hierarchy
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .find(|workspace| workspace.path == fixture.repo)
        .expect("repository-root workspace")
}

fn launch(fixture: &Live, workspace: &str) -> Response {
    fixture.request(Request::CreateSession(CreateSessionRequest {
        project: "fixture".into(),
        workspace: workspace.into(),
        name: String::new(),
        label: None,
        argv: vec!["/bin/sh".into()],
        kind: SessionKind::Terminal,
    }))
}

fn command_output(fixture: &Live, session: SessionId, command: &str, expected: &str) {
    assert_eq!(
        fixture.request(Request::SendTerminal {
            session,
            text: command.into(),
            submit: true,
        }),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let Response::TerminalText { text, .. } = fixture.request(Request::ReadTerminal {
            session,
            max_lines: None,
        }) else {
            panic!("expected terminal text");
        };
        if text.contains(expected) {
            return;
        }
        assert!(Instant::now() < deadline, "missing {expected:?}: {text}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn root_setup_launches_shell_in_repository_and_never_restarts_it_implicitly() {
    let fixture = Live::binary();
    assert_eq!(register(&fixture), Response::Ok);
    let root = root_workspace(&fixture);
    assert!(root.root);
    assert_eq!(root.name, "main");
    assert_eq!(root.warning, None);
    assert_eq!(root.sessions.len(), 1);
    let session = &root.sessions[0];
    command_output(
        &fixture,
        session.id,
        "printf 'ROOT_%s:%s\\n' CWD \"$PWD\"",
        &format!("ROOT_CWD:{}", fixture.repo.display()),
    );
    assert!(matches!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: root.id.clone(),
            force: true,
        }),
        Response::Error {
            code: ovrcr::protocol::ErrorCode::Conflict,
            ..
        }
    ));
    assert!(fixture.repo.join(".git").is_dir());
    assert_eq!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    fixture.join();
    fixture.start_binary();
    let restored = root_workspace(&fixture);
    assert_eq!(restored.id, root.id);
    assert_eq!(restored.sessions.len(), 1);
    assert_eq!(restored.sessions[0].id, session.id);
    assert!(!restored.sessions[0].phase.is_live());
}

#[test]
fn root_drift_blocks_new_processes_but_preserves_the_existing_shell() {
    let fixture = Live::binary();
    assert_eq!(register(&fixture), Response::Ok);
    let root = root_workspace(&fixture);
    let original = &root.sessions[0];
    let Response::CreatedSession(extra) = launch(&fixture, &root.id) else {
        panic!("expected second root shell");
    };
    assert_eq!(
        fixture.request(Request::CloseTerminal {
            session: extra.id,
            expected_run: extra.run,
        }),
        Response::Ok
    );
    let Response::Inventory { sessions, .. } = fixture.request(Request::Inspect) else {
        panic!("expected inventory");
    };
    let closed = sessions
        .iter()
        .find(|session| session.id == extra.id)
        .unwrap();
    assert_eq!(
        fixture.request(Request::UnarchiveSession {
            session: closed.id,
            expected_run: closed.run,
        }),
        Response::Ok
    );
    git(&fixture.repo, &["switch", "-c", "feature/drift"]);
    let drifted = root_workspace(&fixture);
    assert_eq!(drifted.id, root.id);
    assert_eq!(drifted.name, "feature/drift");
    assert!(
        drifted
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("main"))
    );
    assert!(matches!(launch(&fixture, &root.id), Response::Error { .. }));
    let stopped = drifted
        .sessions
        .iter()
        .find(|session| session.id == extra.id)
        .unwrap();
    assert!(matches!(
        fixture.request(Request::ReopenSession {
            session: stopped.id,
            expected_run: stopped.run,
            acknowledge_stopped: false,
        }),
        Response::Error { .. }
    ));
    assert!(matches!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: root.id.clone(),
            force: false,
        }),
        Response::Error { .. }
    ));
    command_output(
        &fixture,
        original.id,
        "printf 'DRIFT_%s\\n' ALIVE",
        "DRIFT_ALIVE",
    );
    git(&fixture.repo, &["switch", "--detach", "HEAD"]);
    let detached = root_workspace(&fixture);
    assert!(detached.name.starts_with("detached @ "));
    assert!(detached.warning.is_some());
    assert!(matches!(launch(&fixture, &root.id), Response::Error { .. }));
    git(&fixture.repo, &["switch", "main"]);
    assert_eq!(root_workspace(&fixture).warning, None);
    assert!(matches!(
        launch(&fixture, &root.id),
        Response::CreatedSession(_)
    ));
}

#[test]
fn registration_refuses_dirty_root_on_wrong_default_without_changing_git() {
    let fixture = Live::binary();
    git(&fixture.repo, &["switch", "-c", "feature/not-default"]);
    std::fs::write(fixture.repo.join("keep.txt"), "user changes").unwrap();
    let response = register(&fixture);
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
    assert_eq!(
        git(&fixture.repo, &["branch", "--show-current"]).trim(),
        "feature/not-default"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("keep.txt")).unwrap(),
        "user changes"
    );
    git(&fixture.repo, &["switch", "main"]);
    assert_eq!(register(&fixture), Response::Ok);
    assert_eq!(root_workspace(&fixture).name, "main");
}

#[test]
fn migration_keeps_legacy_workspace_path_and_identity_without_name_aliases() {
    let fixture = Live::idle();
    let directory = fixture.workspace_root.join("old-title");
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/migrated",
            directory.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(ovrcr::config::legacy_identity_path(&fixture.config), format!(
        "[[projects]]\nname = \"fixture\"\nrepo = {:?}\nworkspace_root = {:?}\n\
         [[projects.workspaces]]\nname = \"old-title\"\npath = {:?}\nbranch = \"feature/migrated\"\n",
        fixture.repo.to_str().unwrap(), fixture.workspace_root.to_str().unwrap(),
        directory.to_str().unwrap(),
    )).unwrap();
    fixture.start_binary();
    let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    let workspace = hierarchy.projects[0]
        .workspaces
        .iter()
        .find(|workspace| workspace.path == directory)
        .unwrap();
    assert_eq!(workspace.name, "feature/migrated");
    let id = workspace.id.clone();
    assert_eq!(root_workspace(&fixture).sessions.len(), 1);
    assert_eq!(
        fixture.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    fixture.join();
    let lookup = |branch: &str| {
        std::process::Command::new(&fixture.executable)
            .args([
                "workspace",
                "get",
                "--project",
                "fixture",
                "--branch",
                branch,
            ])
            .env("OVRCR_HOME", &fixture.config)
            .env("OVRCR_SOCKET", &fixture.socket)
            .output()
            .unwrap()
    };
    let current = lookup("feature/migrated");
    assert!(
        current.status.success(),
        "{}",
        String::from_utf8_lossy(&current.stderr)
    );
    assert!(
        !lookup("old-title").status.success(),
        "legacy name must not remain a CLI alias"
    );
    assert!(
        !fixture.socket.exists(),
        "offline lookup must not start the server"
    );
    fixture.start_binary();
    let Response::Hierarchy(restarted) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    let workspace = restarted.projects[0]
        .workspaces
        .iter()
        .find(|workspace| workspace.path == directory)
        .unwrap();
    assert_eq!(workspace.id, id);
    assert_eq!(workspace.name, "feature/migrated");
    assert!(directory.is_dir());
}

#[test]
fn root_setup_respects_a_non_origin_remote_default() {
    let fixture = Live::binary();
    git(&fixture.repo, &["branch", "-m", "main", "trunk"]);
    git(&fixture.repo, &["remote", "add", "upstream", "."]);
    git(
        &fixture.repo,
        &["update-ref", "refs/remotes/upstream/trunk", "HEAD"],
    );
    git(
        &fixture.repo,
        &[
            "symbolic-ref",
            "refs/remotes/upstream/HEAD",
            "refs/remotes/upstream/trunk",
        ],
    );
    assert_eq!(register(&fixture), Response::Ok);
    let root = root_workspace(&fixture);
    assert_eq!(root.name, "trunk");
    assert!(root.root);
    assert_eq!(root.warning, None);
    assert_eq!(root.sessions.len(), 1);
}

#[test]
fn unregistering_a_project_keeps_its_protected_repository_checkout() {
    let fixture = Live::binary();
    assert_eq!(register(&fixture), Response::Ok);
    fixture.clear_root_shell();
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into()
        }),
        Response::Ok
    );
    assert!(fixture.repo.join(".git").is_dir());
    assert_eq!(
        git(&fixture.repo, &["branch", "--show-current"]).trim(),
        "main"
    );
    let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    assert!(hierarchy.projects.is_empty());
}

#[test]
fn migration_reuses_an_existing_root_registration_and_initializes_its_shell() {
    let fixture = Live::idle();
    std::fs::write(
        &fixture.config,
        format!(
            "[[projects]]\nname = \"fixture\"\nrepo = {:?}\nworkspace_root = {:?}\n\
         [[projects.workspaces]]\nname = \"old-root\"\npath = {:?}\nbranch = \"main\"\n",
            fixture.repo.to_str().unwrap(),
            fixture.workspace_root.to_str().unwrap(),
            fixture.repo.to_str().unwrap(),
        ),
    )
    .unwrap();
    fixture.start_binary();
    let root = root_workspace(&fixture);
    assert_eq!(root.id, "old-root");
    assert_eq!(root.name, "main");
    assert_eq!(
        root.sessions.len(),
        1,
        "existing root registration still needs initial shell setup"
    );
    let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    assert_eq!(hierarchy.projects[0].workspaces.len(), 1);
}

#[test]
fn missing_replacement_worktree_cannot_be_pruned_by_the_old_registration() {
    let fixture = Live::binary();
    assert_eq!(register(&fixture), Response::Ok);
    assert_eq!(
        fixture.request(Request::CreateWorkspaceWithLaunch {
            project: "fixture".into(),
            id: "owned".into(),
            branch: BranchRequest::New {
                branch: "feature/owned".into(),
                base: "main".into()
            },
            launch: None,
        }),
        Response::Ok
    );
    let directory = fixture.workspace_root.join("owned");
    let old_admin = git(&directory, &["rev-parse", "--absolute-git-dir"]);
    // Keep the removed inode alive so the replacement cannot reuse its identity.
    let _old_admin = std::fs::File::open(old_admin.trim()).unwrap();
    git(
        &fixture.repo,
        &["worktree", "remove", directory.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/replacement",
            directory.to_str().unwrap(),
            "main",
        ],
    );
    let replacement_admin = git(&directory, &["rev-parse", "--absolute-git-dir"]);
    std::fs::remove_dir_all(&directory).unwrap();
    let response = fixture.request(Request::RemoveWorkspace {
        project: "fixture".into(),
        name: "owned".into(),
        force: true,
    });
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
    assert!(std::path::Path::new(replacement_admin.trim()).is_dir());
}

#[test]
fn reopening_an_old_root_session_cannot_bypass_the_new_root_guard() {
    let fixture = Live::binary();
    assert_eq!(register(&fixture), Response::Ok);
    let root = root_workspace(&fixture);
    let shell = &root.sessions[0];
    assert_eq!(
        fixture.request(Request::CloseTerminal {
            session: shell.id,
            expected_run: shell.run,
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into()
        }),
        Response::Ok
    );
    assert_eq!(register(&fixture), Response::Ok);
    assert_ne!(root_workspace(&fixture).id, root.id);
    git(&fixture.repo, &["switch", "-c", "feature/drift"]);
    let Response::Inventory { sessions, .. } = fixture.request(Request::Inspect) else {
        panic!("expected inventory");
    };
    let archived = sessions
        .iter()
        .find(|session| session.id == shell.id)
        .unwrap();
    assert_eq!(
        fixture.request(Request::UnarchiveSession {
            session: archived.id,
            expected_run: archived.run,
        }),
        Response::Ok
    );
    let Response::Inventory { sessions, .. } = fixture.request(Request::Inspect) else {
        panic!("expected inventory");
    };
    let stopped = sessions
        .iter()
        .find(|session| session.id == shell.id)
        .unwrap();
    let response = fixture.request(Request::ReopenSession {
        session: stopped.id,
        expected_run: stopped.run,
        acknowledge_stopped: false,
    });
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
}

#[test]
fn slow_checkout_inspection_does_not_block_dashboard_view_acknowledgements() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Live::idle();
    let bin = fixture.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let gate = fixture.root.path().join("git-gate");
    let entered = fixture.root.path().join("git-entered");
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, "#!/bin/sh\nif [ -f \"$OVRCR_GIT_GATE\" ]; then\n  : > \"$OVRCR_GIT_ENTERED\"\n  while [ -f \"$OVRCR_GIT_GATE\" ]; do /bin/sleep 0.01; done\nfi\nexec /usr/bin/git \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(paths).unwrap();
    fixture.start_binary_env(&[
        ("PATH", path.as_os_str()),
        ("OVRCR_GIT_GATE", gate.as_os_str()),
        ("OVRCR_GIT_ENTERED", entered.as_os_str()),
    ]);
    assert_eq!(register(&fixture), Response::Ok);
    let mut dashboard = ovrcr::protocol::connect_server(&fixture.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert!(matches!(
        ovrcr::protocol::client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    std::fs::write(&gate, "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !entered.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !entered.exists() {
        std::fs::remove_file(&gate).unwrap();
        panic!("checkout observer did not reach the controlled Git boundary");
    }
    dashboard
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let response = ovrcr::protocol::client::request(
        &mut dashboard,
        2,
        Request::SetView {
            view: ovrcr::protocol::DashboardView {
                revision: 1,
                panes: Vec::new(),
                focused: None,
            },
        },
    );
    // Unblock the owned Git child before any assertion can unwind the fixture.
    std::fs::remove_file(&gate).unwrap();
    assert_eq!(response.unwrap(), Response::Ok);
}

fn cli(fixture: &Live, args: &[&str]) -> std::process::Output {
    std::process::Command::new(&fixture.executable)
        .arg("--json")
        .args(args)
        .env("OVRCR_HOME", &fixture.config)
        .env("OVRCR_SOCKET", &fixture.socket)
        .env("SHELL", "/bin/sh")
        .output()
        .unwrap()
}

#[test]
fn ambiguous_and_detached_workspaces_are_addressed_by_path() {
    let fixture = Live::binary();
    assert_eq!(register(&fixture), Response::Ok);
    for (id, branch) in [("one", "feature/one"), ("two", "feature/two")] {
        assert_eq!(
            fixture.request(Request::CreateWorkspaceWithLaunch {
                project: "fixture".into(),
                id: id.into(),
                branch: BranchRequest::New {
                    branch: branch.into(),
                    base: "main".into()
                },
                launch: None,
            }),
            Response::Ok
        );
    }
    let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
        panic!("expected hierarchy");
    };
    let first = hierarchy.projects[0]
        .workspaces
        .iter()
        .find(|w| w.name == "feature/one")
        .unwrap()
        .path
        .clone();
    let second = hierarchy.projects[0]
        .workspaces
        .iter()
        .find(|w| w.name == "feature/two")
        .unwrap()
        .path
        .clone();
    git(
        &second,
        &["switch", "--ignore-other-worktrees", "feature/one"],
    );
    let ambiguous = cli(
        &fixture,
        &[
            "workspace",
            "get",
            "--project",
            "fixture",
            "--branch",
            "feature/one",
        ],
    );
    assert!(!ambiguous.status.success());
    let diagnostic = String::from_utf8_lossy(&ambiguous.stderr);
    assert!(diagnostic.contains(first.to_str().unwrap()), "{diagnostic}");
    assert!(
        diagnostic.contains(second.to_str().unwrap()),
        "{diagnostic}"
    );
    let selected = cli(
        &fixture,
        &[
            "workspace",
            "get",
            "--project",
            "fixture",
            "--path",
            second.to_str().unwrap(),
        ],
    );
    assert!(
        selected.status.success(),
        "{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    let selected: serde_json::Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert_eq!(selected["path"], second.to_str().unwrap());
    let terminal = cli(
        &fixture,
        &[
            "terminal",
            "create",
            "--project",
            "fixture",
            "--path",
            second.to_str().unwrap(),
            "--",
            "/bin/sh",
        ],
    );
    assert!(
        terminal.status.success(),
        "{}",
        String::from_utf8_lossy(&terminal.stderr)
    );
    let terminal: serde_json::Value = serde_json::from_slice(&terminal.stdout).unwrap();
    let session = SessionId(terminal["id"].as_u64().unwrap());
    command_output(
        &fixture,
        session,
        "printf 'TARGET_%s:%s\\n' CWD \"$PWD\"",
        &format!("TARGET_CWD:{}", second.display()),
    );
    for directory in [&first, &second] {
        git(directory, &["switch", "--detach", "HEAD"]);
    }
    let detached = cli(
        &fixture,
        &[
            "workspace",
            "get",
            "--project",
            "fixture",
            "--path",
            second.to_str().unwrap(),
        ],
    );
    assert!(detached.status.success());
    let detached: serde_json::Value = serde_json::from_slice(&detached.stdout).unwrap();
    let label = detached["name"].as_str().unwrap();
    assert!(label.starts_with("detached @ "));
    assert!(
        !cli(
            &fixture,
            &[
                "workspace",
                "get",
                "--project",
                "fixture",
                "--branch",
                label
            ]
        )
        .status
        .success()
    );
    command_output(
        &fixture,
        session,
        "printf 'PATH_%s\\n' STILL_ALIVE",
        "PATH_STILL_ALIVE",
    );
}

#[test]
fn a_repository_root_cannot_be_removed_through_another_projects_workspace() {
    let fixture = Live::binary();
    assert_eq!(register(&fixture), Response::Ok);
    assert_eq!(
        fixture.request(Request::CreateWorkspaceWithLaunch {
            project: "fixture".into(),
            id: "nested".into(),
            branch: BranchRequest::New {
                branch: "feature/nested".into(),
                base: "main".into()
            },
            launch: None,
        }),
        Response::Ok
    );
    let nested = fixture.workspace_root.join("nested");
    git(&fixture.repo, &["switch", "-c", "feature/root-drift"]);
    git(&nested, &["switch", "main"]);
    assert_eq!(
        fixture.request(Request::AddProject {
            name: "nested-project".into(),
            repo: nested.clone(),
            workspace_root: fixture.root.path().join("nested-workspaces"),
        }),
        Response::Ok
    );
    let result = fixture.request(Request::RemoveWorkspace {
        project: "fixture".into(),
        name: "nested".into(),
        force: true,
    });
    assert!(matches!(result, Response::Error { .. }), "{result:?}");
    assert!(
        nested.join(".git").is_file(),
        "a registered repository checkout must survive"
    );
}
