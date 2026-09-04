use std::path::{Path, PathBuf};
use std::process::Command;

use ovrcr::config::{ProjectRecord, WorkspaceRecord};
use ovrcr::git::{BranchSpec, create_worktree, inspect_worktree, remove_worktree};

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
            .map(|path| std::fs::canonicalize(&path).unwrap_or(path))
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

    let error = remove_worktree(&fixture.project, &workspace)
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

    let error = remove_worktree(&fixture.project, &workspace)
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

    let error = remove_worktree(&fixture.project, &workspace)
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

    let error = remove_worktree(&fixture.project, &claimed)
        .unwrap_err()
        .to_string();
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

    let error = remove_worktree(&fixture.project, &workspace)
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

    remove_worktree(&fixture.project, &workspace).unwrap();
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
