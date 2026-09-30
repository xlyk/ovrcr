#[path = "support/live.rs"]
mod live;

use std::path::{Path, PathBuf};

use live::Live;
use ovrcr::config::{ProjectRecord, WorkspaceRecord, load_registry};
use ovrcr::git::{
    BranchSpec, UNAVAILABLE_CHECKOUT, checkout_name, create_worktree, default_branch,
    inspect_worktree, observe_registry, remove_worktree, root_warning, worktree_identity,
};
use ovrcr::protocol::{BranchRequest, ErrorCode, Request, Response};

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
        live::init_repo(&repo);
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
        live::git(&self.project.repo, args);
    }

    fn worktree_paths(&self) -> Vec<PathBuf> {
        let output = live::git(&self.project.repo, &["worktree", "list", "--porcelain"]);
        output
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(PathBuf::from)
            .collect()
    }
}

#[test]
fn add_project_creates_a_missing_workspace_root() {
    let fixture = Live::binary();

    let workspace_root = fixture.root.path().join("workspaces").join("demo");
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

    let registry = load_registry(&fixture.config).unwrap();
    let project = registry.project("demo").unwrap();
    assert_eq!(
        project.workspace_root,
        workspace_root.canonicalize().unwrap()
    );

    // A workspace root whose parent is a regular file cannot be created;
    // the resulting error must still name the workspace root.
    let blocker = fixture.root.path().join("blocked-parent");
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
fn add_project_refuses_relative_paths_without_creating_anything() {
    let fixture = Live::thread();

    // A relative workspace root is created under the server's working directory, nowhere near
    // the path the caller named, so it is refused before anything is created.
    let relative_root = Path::new("ovrcr-relative-workspace-root-check").join("demo");
    let (code, message) = match fixture.request(Request::AddProject {
        name: "relative-root".into(),
        repo: fixture.repo.clone(),
        workspace_root: relative_root.clone(),
    }) {
        Response::Error { code, message } => (code, message),
        response => panic!("unexpected response: {response:?}"),
    };
    assert_eq!(code, ErrorCode::InvalidRequest);
    assert!(
        message.contains(&relative_root.display().to_string()),
        "error should name the rejected path: {message}"
    );
    assert!(
        !relative_root.exists(),
        "a refused workspace root must not be created in the server's working directory"
    );
    assert!(
        load_registry(&fixture.config)
            .unwrap()
            .project("relative-root")
            .is_err(),
        "a refused project must not be registered"
    );

    // The repository half is refused the same way, and its workspace root stays uncreated.
    let relative_repo = PathBuf::from("ovrcr-relative-repository-check");
    let absolute_root = fixture.root.path().join("workspaces").join("relative-repo");
    let (code, message) = match fixture.request(Request::AddProject {
        name: "relative-repo".into(),
        repo: relative_repo.clone(),
        workspace_root: absolute_root.clone(),
    }) {
        Response::Error { code, message } => (code, message),
        response => panic!("unexpected response: {response:?}"),
    };
    assert_eq!(code, ErrorCode::InvalidRequest);
    assert!(
        message.contains(&relative_repo.display().to_string()),
        "error should name the rejected path: {message}"
    );
    assert!(
        !absolute_root.exists(),
        "a refused request must not create its workspace root"
    );
}

#[test]
fn add_project_leaves_no_workspace_root_when_the_repository_is_invalid() {
    let fixture = Live::thread();
    let workspace_root = fixture.root.path().join("workspaces").join("unvalidated");
    let missing_repo = fixture.root.path().join("not-a-repository");
    assert!(!workspace_root.exists());

    let message = match fixture.request(Request::AddProject {
        name: "bad-repo".into(),
        repo: missing_repo.clone(),
        workspace_root: workspace_root.clone(),
    }) {
        Response::Error { message, .. } => message,
        response => panic!("unexpected response: {response:?}"),
    };
    assert!(
        message.contains(&missing_repo.display().to_string()),
        "error should name the repository: {message}"
    );
    assert!(
        !workspace_root.exists(),
        "a failed repository check must not leave an empty workspace root behind"
    );
    assert!(
        !workspace_root.parent().unwrap().exists(),
        "a failed repository check must not create the workspace root's parents either"
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
    assert_eq!(first.id, "new-work");
    assert_eq!(first.path, fixture.project.workspace_root.join("new-work"));
    assert!(!first.setup_pending);
    assert_eq!(
        first.git_identity.as_deref(),
        Some(worktree_identity(&first.path).unwrap()).as_deref()
    );

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
    assert!(second.git_identity.is_some());
    assert!(!second.setup_pending);
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

    let error = remove_worktree(&project, &workspace, false)
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

    let error = remove_worktree(&project, &workspace, false)
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
        id: "outside-work".into(),
        path: outside.clone(),
        branch: "feature/outside".into(),
        git_identity: ovrcr::git::worktree_identity(&outside).ok(),
        setup_pending: false,
    };
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };

    let error = remove_worktree(&project, &workspace, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("outside workspace root"));
    assert!(workspace.path.exists());
    assert!(fixture.worktree_paths().contains(&workspace.path));
}

#[test]
fn refuses_replacement_worktree_with_different_identity() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "shared-claim",
        BranchSpec::New {
            branch: "feature/claim".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };
    live::git(
        &fixture.project.repo,
        &["worktree", "remove", workspace.path.to_str().unwrap()],
    );
    live::git(
        &fixture.project.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/replacement",
            workspace.path.to_str().unwrap(),
            "main",
        ],
    );

    let error = inspect_worktree(&fixture.project, &workspace)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("replacement") || error.contains("unrelated"),
        "{error}"
    );
    let error = remove_worktree(&project, &workspace, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("replacement") || error.contains("unrelated"),
        "{error}"
    );
    assert!(workspace.path.exists());
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
    std::fs::create_dir(&workspace.path).unwrap();

    let error = remove_worktree(&project, &workspace, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("registry/Git path disagreement"));
    assert!(actual.path.exists());
    assert!(fixture.worktree_paths().contains(&actual.path));

    // With no directory at the registered path and no Git registration for
    // it, dropping the record is the only remaining action; the unrelated
    // worktree Git does list must stay untouched.
    std::fs::remove_dir(&workspace.path).unwrap();
    remove_worktree(&project, &workspace, false).unwrap();
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
    remove_worktree(&project, &workspace, false).unwrap();
    assert!(!workspace.path.exists());
    assert!(!fixture.worktree_paths().contains(&workspace.path));
    assert!(
        live::git(
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

    remove_worktree(&project, &workspace, false).unwrap();

    assert!(!fixture.worktree_paths().contains(&workspace.path));
    assert!(
        live::git(
            &fixture.project.repo,
            &["show-ref", "--verify", "refs/heads/feature/vanished"]
        )
        .contains("feature/vanished"),
        "pruning must preserve the branch"
    );

    // A directory deleted and pruned outside OVRCR leaves Git with nothing to
    // remove; the stale registry record must still be removable.
    let pruned = create_worktree(
        &fixture.project,
        "pruned-outside",
        BranchSpec::New {
            branch: "feature/pruned-outside".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let project = ProjectRecord {
        workspaces: vec![pruned.clone()],
        ..fixture.project.clone()
    };
    std::fs::remove_dir_all(&pruned.path).unwrap();
    live::git(&fixture.project.repo, &["worktree", "prune"]);
    assert!(!fixture.worktree_paths().contains(&pruned.path));

    remove_worktree(&project, &pruned, false).unwrap();

    assert!(
        live::git(
            &fixture.project.repo,
            &["show-ref", "--verify", "refs/heads/feature/pruned-outside"]
        )
        .contains("feature/pruned-outside"),
        "removing a forgotten worktree must preserve the branch"
    );
}

#[test]
fn missing_replacement_worktree_cannot_be_pruned_by_the_old_registration() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "owned",
        BranchSpec::New {
            branch: "feature/owned".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };
    let old_admin = live::git(&workspace.path, &["rev-parse", "--absolute-git-dir"]);
    let _old_admin = std::fs::File::open(old_admin.trim()).unwrap();
    live::git(
        &fixture.project.repo,
        &["worktree", "remove", workspace.path.to_str().unwrap()],
    );
    live::git(
        &fixture.project.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/replacement",
            workspace.path.to_str().unwrap(),
            "main",
        ],
    );
    let replacement_admin = live::git(&workspace.path, &["rev-parse", "--absolute-git-dir"]);
    std::fs::remove_dir_all(&workspace.path).unwrap();

    let error = remove_worktree(&project, &workspace, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("replacement") || error.contains("unrelated") || error.contains("identity"),
        "{error}"
    );
    assert!(
        std::path::Path::new(replacement_admin.trim()).is_dir(),
        "replacement admin must survive a prune of the old registration"
    );
}

#[test]
fn missing_replacement_without_held_inode_cannot_be_pruned() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "owned",
        BranchSpec::New {
            branch: "feature/owned".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };
    live::git(
        &fixture.project.repo,
        &["worktree", "remove", workspace.path.to_str().unwrap()],
    );
    live::git(
        &fixture.project.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/replacement",
            workspace.path.to_str().unwrap(),
            "main",
        ],
    );
    let replacement_admin = live::git(&workspace.path, &["rev-parse", "--absolute-git-dir"]);
    std::fs::remove_dir_all(&workspace.path).unwrap();

    let error = remove_worktree(&project, &workspace, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("replacement") || error.contains("unrelated") || error.contains("identity"),
        "{error}"
    );
    assert!(std::path::Path::new(replacement_admin.trim()).is_dir());
}

#[test]
fn missing_worktree_prune_leaves_unrelated_prunable_admin() {
    let fixture = GitFixture::new();
    let owned = create_worktree(
        &fixture.project,
        "owned",
        BranchSpec::New {
            branch: "feature/owned".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let other = create_worktree(
        &fixture.project,
        "other",
        BranchSpec::New {
            branch: "feature/other".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let other_admin = live::git(&other.path, &["rev-parse", "--absolute-git-dir"]);
    let project = ProjectRecord {
        workspaces: vec![owned.clone()],
        ..fixture.project.clone()
    };
    std::fs::remove_dir_all(&owned.path).unwrap();
    std::fs::remove_dir_all(&other.path).unwrap();

    remove_worktree(&project, &owned, false).unwrap();

    assert!(!fixture.worktree_paths().contains(&owned.path));
    assert!(
        fixture.worktree_paths().contains(&other.path),
        "unrelated prunable admin must not be swept by another workspace's cleanup"
    );
    assert!(std::path::Path::new(other_admin.trim()).is_dir());
}

#[test]
fn refuses_to_prune_missing_worktree_without_git_identity() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "no-identity",
        BranchSpec::New {
            branch: "feature/no-identity".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let admin = live::git(&workspace.path, &["rev-parse", "--absolute-git-dir"]);
    let mut missing_identity = workspace.clone();
    missing_identity.git_identity = None;
    let project = ProjectRecord {
        workspaces: vec![missing_identity.clone()],
        ..fixture.project.clone()
    };
    std::fs::remove_dir_all(&workspace.path).unwrap();

    let error = remove_worktree(&project, &missing_identity, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("identity") || error.contains("replacement"),
        "{error}"
    );
    assert!(std::path::Path::new(admin.trim()).is_dir());
    assert!(fixture.worktree_paths().contains(&workspace.path));
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

    let error = remove_worktree(&fixture.project, &workspace, false)
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

    let error = remove_worktree(&project, &first, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unexpected canonical path"));
    assert!(first.path.exists());
    assert!(second.path.exists());
    assert!(fixture.worktree_paths().contains(&first.path));
    assert!(fixture.worktree_paths().contains(&second.path));
}

#[test]
fn inspect_and_remove_survive_branch_switch_and_detach() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "switch-work",
        BranchSpec::New {
            branch: "feature/switch".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let identity = workspace.git_identity.clone().unwrap();
    live::git(&workspace.path, &["checkout", "-b", "feature/renamed"]);
    let inspected = inspect_worktree(&fixture.project, &workspace).unwrap();
    assert_eq!(inspected.branch, "feature/renamed");
    assert_eq!(checkout_name(&workspace.path).unwrap(), "feature/renamed");
    assert_eq!(worktree_identity(&workspace.path).unwrap(), identity);

    live::git(&workspace.path, &["checkout", "--detach", "HEAD"]);
    let inspected = inspect_worktree(&fixture.project, &workspace).unwrap();
    assert!(
        inspected.branch.starts_with("detached @ "),
        "{}",
        inspected.branch
    );
    assert_eq!(worktree_identity(&workspace.path).unwrap(), identity);

    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };
    remove_worktree(&project, &workspace, false).unwrap();
    assert!(!workspace.path.exists());
}

#[test]
fn forced_dirty_removal_keeps_generation_and_root_guards() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "force-dirty",
        BranchSpec::New {
            branch: "feature/force-dirty".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let identity = workspace.git_identity.clone().unwrap();
    live::git(
        &workspace.path,
        &["checkout", "-b", "feature/force-renamed"],
    );
    std::fs::write(workspace.path.join("README"), "dirty\n").unwrap();
    assert_eq!(worktree_identity(&workspace.path).unwrap(), identity);
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };

    let error = remove_worktree(&project, &workspace, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("worktree has changes"), "{error}");
    assert!(workspace.path.exists());

    remove_worktree(&project, &workspace, true).unwrap();
    assert!(!workspace.path.exists());
    assert!(!fixture.worktree_paths().contains(&workspace.path));

    let root = WorkspaceRecord {
        id: "root".into(),
        path: fixture.project.repo.clone(),
        branch: "main".into(),
        git_identity: worktree_identity(&fixture.project.repo).ok(),
        setup_pending: false,
    };
    let project = ProjectRecord {
        workspaces: vec![root.clone()],
        ..fixture.project.clone()
    };
    let error = remove_worktree(&project, &root, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("repository checkout"), "{error}");
    assert!(fixture.project.repo.join("README").exists());

    let claimed = create_worktree(
        &fixture.project,
        "force-claim",
        BranchSpec::New {
            branch: "feature/force-claim".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let project = ProjectRecord {
        workspaces: vec![claimed.clone()],
        ..fixture.project.clone()
    };
    live::git(
        &fixture.project.repo,
        &["worktree", "remove", claimed.path.to_str().unwrap()],
    );
    live::git(
        &fixture.project.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/force-replacement",
            claimed.path.to_str().unwrap(),
            "main",
        ],
    );
    let error = remove_worktree(&project, &claimed, true)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("replacement") || error.contains("unrelated"),
        "{error}"
    );
    assert!(claimed.path.exists());
}

#[test]
fn refuses_to_remove_or_inspect_repository_checkout() {
    let fixture = GitFixture::new();
    let workspace = WorkspaceRecord {
        id: "root".into(),
        path: fixture.project.repo.clone(),
        branch: "main".into(),
        git_identity: worktree_identity(&fixture.project.repo).ok(),
        setup_pending: false,
    };
    let project = ProjectRecord {
        workspaces: vec![workspace.clone()],
        ..fixture.project.clone()
    };
    let error = remove_worktree(&project, &workspace, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("repository checkout"), "{error}");
    let error = inspect_worktree(&fixture.project, &workspace)
        .unwrap_err()
        .to_string();
    assert!(error.contains("repository checkout"), "{error}");
    assert!(fixture.project.repo.join("README").exists());
}

#[test]
fn default_branch_matches_tui_resolver_semantics() {
    let fixture = GitFixture::new();
    assert_eq!(default_branch(&fixture.project.repo).unwrap(), "main");
    fixture.git(&["branch", "topic"]);
    fixture.git(&["update-ref", "refs/remotes/origin/topic", "HEAD"]);
    fixture.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/topic",
    ]);
    assert_eq!(default_branch(&fixture.project.repo).unwrap(), "topic");
    fixture.git(&["branch", "-D", "topic"]);
    assert_eq!(
        default_branch(&fixture.project.repo).unwrap(),
        "refs/remotes/origin/topic"
    );
}

#[test]
fn root_warning_and_observe_preserve_setup_default() {
    let fixture = GitFixture::new();
    let feature = create_worktree(
        &fixture.project,
        "feature-work",
        BranchSpec::New {
            branch: "feature/observe".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let root = WorkspaceRecord {
        id: "root".into(),
        path: fixture.project.repo.clone(),
        branch: "main".into(),
        git_identity: worktree_identity(&fixture.project.repo).ok(),
        setup_pending: false,
    };
    let registry = ovrcr::config::Registry {
        projects: vec![ProjectRecord {
            workspaces: vec![root.clone(), feature.clone()],
            ..fixture.project.clone()
        }],
    };
    assert!(root_warning(&registry.projects[0]).is_none());

    live::git(&feature.path, &["checkout", "-b", "feature/moved"]);
    live::git(&fixture.project.repo, &["checkout", "-b", "feature/drift"]);
    let mut observed = registry.clone();
    observe_registry(&mut observed);
    assert_eq!(observed.projects[0].workspaces[0].branch, "feature/drift");
    assert_eq!(observed.projects[0].workspaces[1].branch, "feature/moved");
    assert_eq!(registry.projects[0].workspaces[0].branch, "main");
    assert_eq!(registry.projects[0].workspaces[1].branch, "feature/observe");
    let warning = root_warning(&registry.projects[0]).expect("root drift");
    assert!(warning.contains("feature/drift"), "{warning}");
    assert!(warning.contains("main"), "{warning}");

    live::git(&fixture.project.repo, &["checkout", "main"]);
    assert!(root_warning(&registry.projects[0]).is_none());
}

#[test]
fn observe_marks_missing_feature_workspace_unavailable() {
    let fixture = GitFixture::new();
    let missing = WorkspaceRecord {
        id: "gone".into(),
        path: fixture.project.workspace_root.join("gone"),
        branch: "feature/gone".into(),
        git_identity: None,
        setup_pending: false,
    };
    let mut registry = ovrcr::config::Registry {
        projects: vec![ProjectRecord {
            workspaces: vec![missing],
            ..fixture.project.clone()
        }],
    };
    observe_registry(&mut registry);
    assert_eq!(
        registry.projects[0].workspaces[0].branch,
        UNAVAILABLE_CHECKOUT
    );
}

#[test]
fn head_fallback_does_not_bless_root_drift() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        ["init", "-b", "trunk"].as_slice(),
        ["config", "user.name", "OVRCR Tests"].as_slice(),
        ["config", "user.email", "tests@example.invalid"].as_slice(),
        ["commit", "--allow-empty", "-m", "initial"].as_slice(),
    ] {
        live::git(&repo, args);
    }
    let workspace_root = dir.path().join("workspaces");
    std::fs::create_dir(&workspace_root).unwrap();
    let project = ProjectRecord {
        name: "fixture".into(),
        repo: repo.canonicalize().unwrap(),
        workspace_root: workspace_root.canonicalize().unwrap(),
        workspaces: vec![WorkspaceRecord {
            id: "root".into(),
            path: repo.canonicalize().unwrap(),
            branch: "trunk".into(),
            git_identity: worktree_identity(&repo).ok(),
            setup_pending: false,
        }],
    };
    assert_eq!(default_branch(&project.repo).unwrap(), "trunk");
    assert!(root_warning(&project).is_none());
    live::git(&project.repo, &["checkout", "-b", "feature/other"]);
    assert_eq!(default_branch(&project.repo).unwrap(), "feature/other");
    let warning = root_warning(&project).expect("stored trunk must remain expected");
    assert!(warning.contains("trunk"), "{warning}");
    assert!(warning.contains("feature/other"), "{warning}");
}

#[test]
fn refuses_existing_worktree_without_git_identity() {
    let fixture = GitFixture::new();
    let workspace = create_worktree(
        &fixture.project,
        "no-identity",
        BranchSpec::New {
            branch: "feature/no-identity".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let mut missing_identity = workspace.clone();
    missing_identity.git_identity = None;
    let project = ProjectRecord {
        workspaces: vec![missing_identity.clone()],
        ..fixture.project.clone()
    };
    let error = inspect_worktree(&fixture.project, &missing_identity)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("identity") || error.contains("replacement"),
        "{error}"
    );
    let error = remove_worktree(&project, &missing_identity, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("identity") || error.contains("replacement"),
        "{error}"
    );
    assert!(workspace.path.exists());
}

#[test]
fn refuses_same_id_with_different_registered_path() {
    let fixture = GitFixture::new();
    let actual = create_worktree(
        &fixture.project,
        "owned-work",
        BranchSpec::New {
            branch: "feature/owned".into(),
            base: "main".into(),
        },
    )
    .unwrap();
    let forged = WorkspaceRecord {
        path: fixture.project.workspace_root.join("forged"),
        ..actual.clone()
    };
    let project = ProjectRecord {
        workspaces: vec![actual.clone()],
        ..fixture.project.clone()
    };
    let error = remove_worktree(&project, &forged, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("workspace is not registered"), "{error}");
    assert!(actual.path.exists());
}

#[test]
fn detached_root_warning_uses_stored_default() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        ["init", "-b", "trunk"].as_slice(),
        ["config", "user.name", "OVRCR Tests"].as_slice(),
        ["config", "user.email", "tests@example.invalid"].as_slice(),
        ["commit", "--allow-empty", "-m", "initial"].as_slice(),
    ] {
        live::git(&repo, args);
    }
    let workspace_root = dir.path().join("workspaces");
    std::fs::create_dir(&workspace_root).unwrap();
    let project = ProjectRecord {
        name: "fixture".into(),
        repo: repo.canonicalize().unwrap(),
        workspace_root: workspace_root.canonicalize().unwrap(),
        workspaces: vec![WorkspaceRecord {
            id: "root".into(),
            path: repo.canonicalize().unwrap(),
            branch: "trunk".into(),
            git_identity: worktree_identity(&repo).ok(),
            setup_pending: false,
        }],
    };
    live::git(&project.repo, &["checkout", "--detach", "HEAD"]);
    let warning = root_warning(&project).expect("detached root");
    assert!(warning.contains("trunk"), "{warning}");
    assert!(warning.contains("detached"), "{warning}");
}

#[test]
fn automatic_local_terminals_policy_controls_provisioning_paths() {
    use ovrcr::protocol::{CreateSessionRequest, SessionKind, SessionLaunch};
    use std::time::{Duration, Instant};

    for (policy, expect_root, expect_feature) in [
        (None, true, false), // missing key → default_branch_only
        (Some("on"), true, true),
        (Some("off"), false, false),
        (Some("default_branch_only"), true, false),
    ] {
        let fixture = Live::binary();
        live::git(&fixture.repo, &["branch", "-m", "trunk"]);
        let dashboard = fixture.root.path().join("dashboard.toml");
        match policy {
            Some(value) => {
                std::fs::write(
                    &dashboard,
                    format!("automatic_local_terminals = \"{value}\"\n"),
                )
                .unwrap();
            }
            None => {
                let _ = std::fs::remove_file(&dashboard);
            }
        }

        assert_eq!(
            fixture.request(Request::AddProject {
                name: "demo".into(),
                repo: fixture.repo.clone(),
                workspace_root: fixture.workspace_root.clone(),
            }),
            Response::Ok
        );
        let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
            panic!("hierarchy");
        };
        let root = hierarchy.projects[0]
            .workspaces
            .iter()
            .find(|workspace| workspace.path == fixture.repo)
            .unwrap();
        let root_locals = root
            .sessions
            .iter()
            .filter(|session| session.name == "local" && session.phase.is_live())
            .count();
        assert_eq!(root.name, "trunk");
        assert_eq!(
            root_locals,
            usize::from(expect_root),
            "policy={policy:?} root locals={root_locals}"
        );

        assert_eq!(
            fixture.request(Request::CreateWorkspace {
                project: "demo".into(),
                id: "feature".into(),
                branch: BranchRequest::New {
                    branch: "feature/policy".into(),
                    base: "trunk".into(),
                },
            }),
            Response::Ok
        );
        let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
            panic!("hierarchy after feature");
        };
        let feature = hierarchy.projects[0]
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "feature")
            .unwrap();
        let feature_locals = feature
            .sessions
            .iter()
            .filter(|session| session.name == "local" && session.phase.is_live())
            .count();
        assert_eq!(
            feature_locals,
            usize::from(expect_feature),
            "policy={policy:?} feature locals={feature_locals}"
        );

        // Dashboard explicit workspace launches and direct terminal/agent
        // launches must remain independent of the automatic preference.
        for (label, kind) in [
            ("terminal", SessionKind::Terminal),
            (
                "agent",
                SessionKind::Agent {
                    name: "policy-fixture".into(),
                },
            ),
        ] {
            let argv = vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'POLICY_%s\\n' EXPLICIT; exec /bin/sh -i".into(),
            ];
            for response in [
                fixture.request(Request::CreateSession(CreateSessionRequest {
                    project: "demo".into(),
                    workspace: "feature".into(),
                    name: format!("explicit-{label}"),
                    label: None,
                    argv: argv.clone(),
                    kind: kind.clone(),
                })),
                fixture.request(Request::CreateWorkspaceWithLaunch {
                    project: "demo".into(),
                    id: format!("explicit-{label}"),
                    branch: BranchRequest::New {
                        branch: format!("feature/explicit-{label}"),
                        base: "trunk".into(),
                    },
                    launch: Some(SessionLaunch {
                        argv: argv.clone(),
                        label: None,
                        kind: kind.clone(),
                    }),
                }),
            ] {
                let Response::CreatedSession(session) = response else {
                    panic!("policy={policy:?} {label} launch: {response:?}");
                };
                assert_eq!(session.kind, kind);
                assert!(session.phase.is_live(), "policy={policy:?} {label}");
                fixture.own_group(session.pid.unwrap() as libc::pid_t);
                let deadline = Instant::now() + live::wait_deadline();
                loop {
                    let Response::TerminalText { text, .. } =
                        fixture.request(Request::ReadTerminal {
                            session: session.id,
                            max_lines: Some(24),
                        })
                    else {
                        panic!("read explicit {label}");
                    };
                    if text.contains("POLICY_EXPLICIT") {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "policy={policy:?} {label}: {text}"
                    );
                    std::thread::park_timeout(Duration::from_millis(10));
                }
            }
        }
        let Response::Hierarchy(hierarchy) = fixture.request(Request::List) else {
            panic!("hierarchy after explicit launches");
        };
        for label in ["terminal", "agent"] {
            let workspace = hierarchy.projects[0]
                .workspaces
                .iter()
                .find(|workspace| workspace.id == format!("explicit-{label}"))
                .unwrap();
            assert_eq!(workspace.sessions.len(), 1, "policy={policy:?} {label}");
        }
        for pgid in fixture.session_groups() {
            fixture.own_group(pgid);
        }
    }
}
