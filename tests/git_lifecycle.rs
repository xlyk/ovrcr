use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use ovrcr::config::{
    ProjectRecord, Registry, WorkspaceRecord, load_registry, save_registry_atomic,
};
use ovrcr::git::{BranchSpec, create_worktree, inspect_worktree, remove_worktree};
use ovrcr::protocol::{
    ClientMessage, Request, Response, ServerMessage, connect_server, read_frame, write_frame,
};
use ovrcr::server::{ServerPaths, run_server};

struct GitFixture {
    dir: tempfile::TempDir,
    project: ProjectRecord,
}

impl GitFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let workspace_root = dir.path().join("workspaces");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&workspace_root).unwrap();
        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.name", "OVRCR Tests"]);
        run_git(
            &repo,
            &["config", "user.email", "ovrcr-tests@example.invalid"],
        );
        std::fs::write(repo.join("README"), "fixture\n").unwrap();
        run_git(&repo, &["add", "README"]);
        run_git(&repo, &["commit", "-m", "initial"]);
        let repo = repo.canonicalize().unwrap();
        let workspace_root = workspace_root.canonicalize().unwrap();
        Self {
            dir,
            project: ProjectRecord {
                name: "fixture".into(),
                repo,
                workspace_root,
                workspaces: Vec::new(),
            },
        }
    }

    fn git(&self, args: &[&str]) {
        run_git(&self.project.repo, args);
    }

    fn worktree_paths(&self) -> Vec<PathBuf> {
        let output = git_output(&self.project.repo, &["worktree", "list", "--porcelain"]);
        output
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(PathBuf::from)
            .collect()
    }
}

fn run_git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed: {status}", args);
}

fn git_output(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {:?} failed", args);
    String::from_utf8(output.stdout).unwrap()
}

/// Drives a live server over its own Unix socket, with its own registry
/// file and temporary workspace root, mirroring the `ControlFixture`
/// pattern in `tests/server_lifecycle.rs`.
struct ServerFixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    registry_path: PathBuf,
    socket: PathBuf,
    thread: Option<thread::JoinHandle<()>>,
}

impl ServerFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.name", "OVRCR Tests"]);
        run_git(
            &repo,
            &["config", "user.email", "ovrcr-tests@example.invalid"],
        );
        std::fs::write(repo.join("README"), "fixture\n").unwrap();
        run_git(&repo, &["add", "README"]);
        run_git(&repo, &["commit", "-m", "initial"]);
        let repo = repo.canonicalize().unwrap();

        let registry_path = root.path().join("config.toml");
        save_registry_atomic(&Registry::default(), &registry_path).unwrap();
        let socket = root.path().join("private").join("server.sock");
        let paths = ServerPaths {
            socket: socket.clone(),
        };
        let thread_registry_path = registry_path.clone();
        let thread = thread::spawn(move || run_server(paths, thread_registry_path).unwrap());
        let fixture = Self {
            root,
            repo,
            registry_path,
            socket,
            thread: Some(thread),
        };
        fixture.wait_for_socket();
        fixture
    }

    fn tmp_path(&self) -> &Path {
        self.root.path()
    }

    fn wait_for_socket(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if UnixStream::connect(&self.socket).is_ok() {
                return;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        panic!("control server did not start");
    }

    fn request(&self, request: Request) -> Response {
        let mut stream = connect_server(&self.socket).unwrap();
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 1,
                request,
            },
        )
        .unwrap();
        match read_frame::<ServerMessage>(&mut stream).unwrap() {
            ServerMessage::Response { response, .. } => response,
            ServerMessage::Event(event) => panic!("unexpected event: {event:?}"),
        }
    }
}

impl Drop for ServerFixture {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.request(Request::Shutdown { kill: true });
            let _ = thread.join();
        }
    }
}

#[test]
fn add_project_creates_a_missing_workspace_root() {
    let fixture = ServerFixture::new();

    let workspace_root = fixture.tmp_path().join("workspaces").join("demo");
    assert!(!workspace_root.exists());

    assert_eq!(
        fixture.request(Request::AddProject {
            name: "demo".into(),
            repo: fixture.repo.clone(),
            workspace_root: workspace_root.clone(),
        }),
        Response::Ok
    );
    assert!(workspace_root.is_dir());

    let registry = load_registry(&fixture.registry_path).unwrap();
    let project = registry.project("demo").unwrap();
    assert_eq!(
        project.workspace_root,
        workspace_root.canonicalize().unwrap()
    );

    // A workspace root whose parent is a regular file cannot be created;
    // the resulting error must still name the workspace root.
    let blocker = fixture.tmp_path().join("blocked-parent");
    std::fs::write(&blocker, "not a directory\n").unwrap();
    let blocked_root = blocker.join("child");
    let message = match fixture.request(Request::AddProject {
        name: "second".into(),
        repo: fixture.repo.clone(),
        workspace_root: blocked_root.clone(),
    }) {
        Response::Error { message, .. } => message,
        response => panic!("unexpected response: {response:?}"),
    };
    assert!(
        message.contains(&blocked_root.display().to_string()),
        "error should name the workspace root: {message}"
    );
}

#[test]
fn creates_new_and_existing_branch_worktrees() {
    let fixture = GitFixture::new();
    let first = create_worktree(
        &fixture.project,
        "new-work",
        BranchSpec::New {
            branch: "feature/new".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    assert_eq!(first.branch, "feature/new");

    fixture.git(&["branch", "feature/existing", "main"]);
    let second = create_worktree(
        &fixture.project,
        "existing-work",
        BranchSpec::Existing {
            branch: "feature/existing".into(),
        },
    )
    .unwrap();
    assert_eq!(second.branch, "feature/existing");
}

#[test]
fn rejects_invalid_base_and_nonlocal_existing_ref_before_worktree_creation() {
    let fixture = GitFixture::new();
    let invalid_destination = fixture.project.workspace_root.join("invalid-base");
    let error = create_worktree(
        &fixture.project,
        "invalid-base",
        BranchSpec::New {
            branch: "feature/invalid-base".into(),
            base: "--force".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("validate worktree base"));
    assert!(!invalid_destination.exists());
    assert!(
        !fixture
            .worktree_paths()
            .iter()
            .any(|path| path == &invalid_destination)
    );

    fixture.git(&["tag", "tag-only"]);
    let tag_destination = fixture.project.workspace_root.join("tag-only");
    let error = create_worktree(
        &fixture.project,
        "tag-only",
        BranchSpec::Existing {
            branch: "tag-only".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("local branch does not exist"));
    assert!(!tag_destination.exists());
}

#[test]
fn rejects_workspace_path_traversal_before_git() {
    let fixture = GitFixture::new();
    let error = create_worktree(
        &fixture.project,
        "../escape",
        BranchSpec::New {
            branch: "feature/escape".into(),
            base: "main".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("invalid workspace name"));
    assert!(!fixture.dir.path().join("escape").exists());
}

#[test]
fn rejects_existing_worktree_destination() {
    let fixture = GitFixture::new();
    let destination = fixture.project.workspace_root.join("already-there");
    std::fs::create_dir(&destination).unwrap();
    let error = create_worktree(
        &fixture.project,
        "already-there",
        BranchSpec::New {
            branch: "feature/already-there".into(),
            base: "main".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("destination already exists"));
    assert!(destination.exists());
    assert!(!fixture.worktree_paths().contains(&destination));
}

#[test]
fn inspects_clean_and_dirty_worktrees() {
    let fixture = GitFixture::new();
    let created = create_worktree(
        &fixture.project,
        "inspect-work",
        BranchSpec::New {
            branch: "feature/inspect".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let clean = inspect_worktree(&fixture.project, &created).unwrap();
    assert_eq!(clean.canonical_path, created.path.canonicalize().unwrap());
    assert_eq!(clean.branch, "feature/inspect");
    assert!(!clean.dirty);

    std::fs::write(created.path.join("untracked"), "change\n").unwrap();
    assert!(inspect_worktree(&fixture.project, &created).unwrap().dirty);
}

#[test]
fn refuses_to_remove_worktree_with_tracked_changes() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "tracked-change",
        BranchSpec::New {
            branch: "feature/tracked-change".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    std::fs::write(workspace.path.join("README"), "modified\n").unwrap();
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };

    let error = remove_worktree(&project, &workspace)
        .unwrap_err()
        .to_string();
    assert!(error.contains("worktree has changes"));
    assert!(workspace.path.exists());
    assert!(fixture.worktree_paths().contains(&workspace.path));
}

#[test]
fn refuses_to_remove_worktree_with_untracked_files() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "untracked-change",
        BranchSpec::New {
            branch: "feature/untracked-change".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    std::fs::write(workspace.path.join("untracked"), "change\n").unwrap();
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };

    let error = remove_worktree(&project, &workspace)
        .unwrap_err()
        .to_string();
    assert!(error.contains("worktree has changes"));
    assert!(workspace.path.exists());
    assert!(fixture.worktree_paths().contains(&workspace.path));
}

#[test]
fn refuses_to_remove_worktree_outside_registered_root() {
    let fixture = GitFixture::new();
    let outside = fixture.dir.path().join("outside-work");
    fixture.git(&[
        "worktree",
        "add",
        "-b",
        "feature/outside",
        outside.to_str().unwrap(),
        "main",
    ]);
    let outside = outside.canonicalize().unwrap();
    let workspace = WorkspaceRecord {
        name: "outside-work".into(),
        path: outside.clone(),
        branch: "feature/outside".into(),
    };
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };

    let error = remove_worktree(&project, &workspace)
        .unwrap_err()
        .to_string();
    assert!(error.contains("outside workspace root"));
    assert!(workspace.path.exists());
    assert!(fixture.worktree_paths().contains(&workspace.path));
}

#[test]
fn refuses_to_remove_branch_checked_out_elsewhere() {
    let fixture = GitFixture::new();
    let elsewhere = fixture.dir.path().join("elsewhere");
    fixture.git(&[
        "worktree",
        "add",
        "-b",
        "feature/shared",
        elsewhere.to_str().unwrap(),
        "main",
    ]);
    let workspace = create_worktree(
        &fixture.project,
        "shared-claim",
        BranchSpec::New {
            branch: "feature/claim".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let claimed = WorkspaceRecord {
        branch: "feature/shared".into(),
        ..workspace
    };
    let project = ProjectRecord {
        workspaces: vec![claimed.clone()],
        ..fixture.project.clone()
    };

    let error = remove_worktree(&project, &claimed).unwrap_err().to_string();
    assert!(error.contains("branch checked out elsewhere"));
    assert!(claimed.path.exists());
    assert!(fixture.worktree_paths().contains(&claimed.path));
    assert!(elsewhere.exists());
}

#[test]
fn refuses_registry_and_git_path_disagreement() {
    let fixture = GitFixture::new();
    let actual = create_worktree(
        &fixture.project,
        "actual-work",
        BranchSpec::New {
            branch: "feature/actual".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let workspace = WorkspaceRecord {
        path: fixture.project.workspace_root.join("different"),
        ..actual.clone()
    };
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };

    let error = remove_worktree(&project, &workspace)
        .unwrap_err()
        .to_string();
    assert!(error.contains("registry/Git path disagreement"));
    assert!(actual.path.exists());
    assert!(fixture.worktree_paths().contains(&actual.path));
}

#[test]
fn removes_clean_registered_worktree_and_preserves_branch() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "remove-work",
        BranchSpec::New {
            branch: "feature/remove".into(),
            base: "main".into(),
        },
    )
    .unwrap();

    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };
    remove_worktree(&project, &workspace).unwrap();
    assert!(!workspace.path.exists());
    assert!(!fixture.worktree_paths().contains(&workspace.path));
    assert!(
        git_output(
            &fixture.project.repo,
            &["show-ref", "--verify", "refs/heads/feature/remove"]
        )
        .contains("feature/remove")
    );
}

#[test]
fn removes_workspace_whose_directory_is_gone() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "vanished-work",
        BranchSpec::New {
            branch: "feature/vanished".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };
    std::fs::remove_dir_all(&workspace.path).unwrap();
    assert!(fixture.worktree_paths().contains(&workspace.path));

    remove_worktree(&project, &workspace).unwrap();

    assert!(!fixture.worktree_paths().contains(&workspace.path));
    assert!(
        git_output(
            &fixture.project.repo,
            &["show-ref", "--verify", "refs/heads/feature/vanished"]
        )
        .contains("feature/vanished"),
        "pruning must preserve the branch"
    );

    // A missing directory whose path Git does not list is refused: the
    // registry alone never authorizes a prune.
    let unknown = WorkspaceRecord {
        name: "never-created".into(),
        path: fixture.project.workspace_root.join("never-created"),
        branch: "feature/vanished".into(),
    };
    let project = ProjectRecord {
        workspaces: vec![unknown.clone()],
        ..fixture.project.clone()
    };
    let error = remove_worktree(&project, &unknown).unwrap_err().to_string();
    assert!(error.contains("registry/Git path disagreement"), "{error}");
}

#[test]
fn refuses_to_remove_unregistered_matching_worktree() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "unregistered-work",
        BranchSpec::New {
            branch: "feature/unregistered".into(),
            base: "main".into(),
        },
    )
    .unwrap();

    let error = remove_worktree(&fixture.project, &workspace)
        .unwrap_err()
        .to_string();
    assert!(error.contains("workspace is not registered"));
    assert!(workspace.path.exists());
    assert!(fixture.worktree_paths().contains(&workspace.path));
}

#[test]
#[cfg(unix)]
fn refuses_symlink_substitution_without_touching_either_worktree() {
    use std::os::unix::fs::symlink;

    let fixture = GitFixture::new();
    let first = create_worktree(
        &fixture.project,
        "worktree-a",
        BranchSpec::New {
            branch: "feature/a".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let second = create_worktree(
        &fixture.project,
        "worktree-b",
        BranchSpec::New {
            branch: "feature/b".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let project = ProjectRecord {
        workspaces: vec![first.clone(), second.clone()],
        ..fixture.project.clone()
    };

    let moved_first = fixture.dir.path().join("worktree-a-real");
    std::fs::rename(&first.path, &moved_first).unwrap();
    symlink(&second.path, &first.path).unwrap();

    let error = remove_worktree(&project, &first).unwrap_err().to_string();
    assert!(error.contains("unexpected canonical path"));
    assert!(first.path.exists());
    assert!(second.path.exists());
    assert!(fixture.worktree_paths().contains(&first.path));
    assert!(fixture.worktree_paths().contains(&second.path));
}
