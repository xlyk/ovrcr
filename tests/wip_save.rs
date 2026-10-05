//! Uncommitted work is saved to `origin/wip/<branch>` only at removal or shutdown.

#[path = "support/live.rs"]
mod live;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use live::Live;
use ovrcr::protocol::{
    BranchRequest, ClientMessage, ErrorCode, Request, Response, ServerEvent, ServerMessage,
    connect_server, read_frame, write_frame,
};

struct World {
    live: Live,
    origin: PathBuf,
}

impl World {
    fn new() -> Self {
        let live = Live::thread();
        let origin = live.root.path().join("origin.git");
        std::fs::create_dir(&origin).unwrap();
        live::git(&origin, &["init", "--bare"]);
        live::git(
            &live.repo,
            &[
                "remote",
                "add",
                "origin",
                origin.to_str().expect("utf-8 origin"),
            ],
        );
        live::git(&live.repo, &["push", "origin", "main"]);
        assert_eq!(
            live.request(Request::AddProject {
                name: "wip".into(),
                repo: live.repo.clone(),
                workspace_root: live.workspace_root.clone(),
            }),
            Response::Ok
        );
        live.clear_root_shell();
        Self { live, origin }
    }

    fn workspace(&self, id: &str, branch: &str) {
        assert_eq!(
            self.live.request(Request::CreateWorkspace {
                project: "wip".into(),
                id: id.into(),
                branch: BranchRequest::New {
                    branch: branch.into(),
                    base: "main".into(),
                },
            }),
            Response::Ok
        );
    }

    fn path(&self, id: &str) -> PathBuf {
        let Response::Hierarchy(hierarchy) = self.live.request(Request::List) else {
            panic!("expected hierarchy");
        };
        hierarchy
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .find(|workspace| workspace.id == id)
            .unwrap_or_else(|| panic!("missing workspace {id}"))
            .path
    }

    fn tip(&self, branch: &str) -> String {
        live::git(&self.live.repo, &["rev-parse", branch])
    }

    fn listed(&self, id: &str) -> bool {
        let Response::Hierarchy(hierarchy) = self.live.request(Request::List) else {
            panic!("expected hierarchy");
        };
        hierarchy
            .projects
            .iter()
            .flat_map(|project| &project.workspaces)
            .any(|workspace| workspace.id == id)
    }

    fn remote_names(&self, branch: &str) -> String {
        live::git(
            &self.origin,
            &[
                "ls-tree",
                "-r",
                "--name-only",
                &format!("refs/heads/wip/{branch}"),
            ],
        )
    }
}

fn dirty(path: &Path) {
    std::fs::write(path.join(".gitignore"), "*.tmp\n").unwrap();
    std::fs::write(path.join("note.txt"), "keep me\n").unwrap();
    std::fs::write(path.join("skip.tmp"), "nope\n").unwrap();
}

#[test]
fn removing_a_dirty_workspace_without_the_setting_refuses() {
    let world = World::new();
    world.workspace("one", "feature/one");
    dirty(&world.path("one"));
    let before = world.tip("feature/one");
    match world.live.request(Request::RemoveWorkspace {
        project: "wip".into(),
        name: "one".into(),
        force: false,
    }) {
        Response::Error {
            code: ErrorCode::DirtyWorktree,
            ..
        } => {}
        response => panic!("dirty removal must be refused: {response:?}"),
    }
    assert!(world.listed("one"));
    assert_eq!(world.tip("feature/one"), before);
    let listed = live::git(&world.origin, &["ls-remote", ".", "refs/heads/wip/*"]);
    assert!(listed.is_empty(), "nothing was pushed: {listed}");
}

#[test]
fn the_setting_saves_and_removes_without_asking() {
    let world = World::new();
    world.workspace("one", "feature/one");
    dirty(&world.path("one"));
    let before = world.tip("feature/one");
    assert_eq!(
        world.live.request(Request::SetSetting {
            path: "save_uncommitted_work".into(),
            value: Some("true".into()),
        }),
        Response::Ok
    );
    assert_eq!(
        world.live.request(Request::RemoveWorkspace {
            project: "wip".into(),
            name: "one".into(),
            force: false,
        }),
        Response::Ok
    );
    assert!(!world.listed("one"));
    assert_eq!(world.tip("feature/one"), before);
    let tree = world.remote_names("feature/one");
    assert!(tree.lines().any(|line| line == "note.txt"), "{tree}");
    assert!(!tree.lines().any(|line| line == "skip.tmp"), "{tree}");
}

#[test]
fn shutdown_asks_twice_and_pushes_two_refs() {
    let world = World::new();
    world.workspace("one", "feature/one");
    world.workspace("two", "feature/two");
    world.workspace("clean", "feature/clean");
    dirty(&world.path("one"));
    dirty(&world.path("two"));
    std::fs::write(world.live.repo.join("dirty-root.txt"), "root\n").unwrap();
    let before_one = world.tip("feature/one");
    let before_two = world.tip("feature/two");
    let before_main = world.tip("main");

    let mut dashboard = connect_server(&world.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    loop {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Response {
                response: Response::Hierarchy(_),
                ..
            } => break,
            _ => {}
        }
    }

    let socket = world.live.socket.clone();
    let shutdown = thread::spawn(move || {
        live::request_with_timeout(
            &socket,
            2,
            Request::Shutdown { kill: true },
            Duration::from_secs(20),
        )
    });

    let mut branches = Vec::new();
    let mut answers = 0;
    while answers < 2 {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Event(ServerEvent::WipSavePrompt {
                project,
                workspace,
                branch,
            }) => {
                branches.push(branch.clone());
                write_frame(
                    &mut dashboard,
                    &ClientMessage {
                        request_id: 10 + branches.len() as u64,
                        request: Request::AnswerWipSave {
                            project,
                            workspace,
                            save: true,
                        },
                    },
                )
                .unwrap();
            }
            ServerMessage::Response {
                response: Response::Ok,
                ..
            } => answers += 1,
            ServerMessage::Response { response, .. } => {
                panic!("shutdown answer failed: {response:?}")
            }
            ServerMessage::Event(_) => {}
        }
    }
    branches.sort();
    assert_eq!(
        branches,
        ["feature/one".to_string(), "feature/two".to_string()],
        "a clean worktree and the root checkout are not asked"
    );
    assert_eq!(shutdown.join().unwrap(), Some(Response::Ok));
    assert_eq!(world.tip("feature/one"), before_one);
    assert_eq!(world.tip("feature/two"), before_two);
    assert_eq!(world.tip("main"), before_main);
    for branch in ["feature/one", "feature/two"] {
        let tree = world.remote_names(branch);
        assert!(
            tree.lines().any(|line| line == "note.txt"),
            "{branch}: {tree}"
        );
        assert!(
            !tree.lines().any(|line| line == "skip.tmp"),
            "{branch}: {tree}"
        );
    }
    let root_ref = live::git(&world.origin, &["ls-remote", ".", "refs/heads/wip/main"]);
    assert!(
        root_ref.is_empty(),
        "the root checkout is not saved: {root_ref}"
    );
}

#[test]
fn the_setting_pushes_on_shutdown_without_asking() {
    let world = World::new();
    world.workspace("one", "feature/one");
    world.workspace("two", "feature/two");
    dirty(&world.path("one"));
    dirty(&world.path("two"));
    let before_one = world.tip("feature/one");
    let before_two = world.tip("feature/two");
    assert_eq!(
        world.live.request(Request::SetSetting {
            path: "save_uncommitted_work".into(),
            value: Some("true".into()),
        }),
        Response::Ok
    );

    let mut dashboard = connect_server(&world.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    loop {
        match read_frame::<ServerMessage>(&mut dashboard).unwrap() {
            ServerMessage::Response {
                response: Response::Hierarchy(_),
                ..
            } => break,
            ServerMessage::Event(ServerEvent::WipSavePrompt { .. }) => {
                panic!("the setting must not ask")
            }
            _ => {}
        }
    }

    assert_eq!(
        world.live.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    dashboard
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let drain_until = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < drain_until {
        match read_frame::<ServerMessage>(&mut dashboard) {
            Ok(ServerMessage::Event(ServerEvent::WipSavePrompt { .. })) => {
                panic!("shutdown must not ask when the setting is on")
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert_eq!(world.tip("feature/one"), before_one);
    assert_eq!(world.tip("feature/two"), before_two);
    for branch in ["feature/one", "feature/two"] {
        let tree = world.remote_names(branch);
        assert!(
            tree.lines().any(|line| line == "note.txt"),
            "{branch}: {tree}"
        );
    }
}
