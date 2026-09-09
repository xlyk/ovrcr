//! Best-effort suggestions only. Git remains authoritative at submission.
use anyhow::{Context, Result, bail};
use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct Hints {
    pub branches: Vec<String>,
    pub base: String,
}

type HintResult = (String, Result<Hints, String>);

pub(super) struct Worker {
    jobs: SyncSender<(String, PathBuf)>,
    results: Receiver<HintResult>,
    cancelled: Arc<AtomicBool>,
}

impl Worker {
    pub fn start() -> std::io::Result<Self> {
        let (jobs, requests) = mpsc::sync_channel::<(String, PathBuf)>(1);
        let (results, completed) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        std::thread::Builder::new()
            .name("workspace-git-hints".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let Ok((project, repo)) = requests.recv() else {
                        break;
                    };
                    let result = read_hints(&repo, &stop).map_err(|error| error.to_string());
                    if results.send((project, result)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            jobs,
            results: completed,
            cancelled,
        })
    }

    pub fn request(&self, project: String, repo: PathBuf) -> bool {
        self.jobs.try_send((project, repo)).is_ok()
    }

    pub fn poll(&self) -> Option<HintResult> {
        self.results.try_recv().ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn read_hints(repo: &Path, cancelled: &AtomicBool) -> Result<Hints> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let branches = local_branches(repo, deadline, cancelled)?;
    let base = default_branch(repo, &branches, deadline, cancelled)?;
    Ok(Hints { branches, base })
}

fn local_branches(repo: &Path, deadline: Instant, cancelled: &AtomicBool) -> Result<Vec<String>> {
    Ok(git_output(
        repo,
        &["for-each-ref", "refs/heads", "--format=%(refname:short)"],
        deadline,
        cancelled,
    )?
    .lines()
    .map(str::to_owned)
    .collect())
}

fn default_branch(
    repo: &Path,
    branches: &[String],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<String> {
    // A repository whose sole remote is not named `origin` still has a default
    // branch worth suggesting; with none or several, `origin` stays the guess.
    let listed = git_output(repo, &["remote"], deadline, cancelled).unwrap_or_default();
    let mut names = listed
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let remote_name = match (names.next(), names.next()) {
        (Some(sole), None) => sole.to_owned(),
        _ => "origin".to_owned(),
    };
    let prefix = format!("refs/remotes/{remote_name}/");
    // Git resolves loose/packed refs and .git files in linked worktrees for us.
    let remote = git_output(
        repo,
        &["symbolic-ref", "--quiet", &format!("{prefix}HEAD")],
        deadline,
        cancelled,
    );
    if let Ok(remote) = remote
        && let Some(branch) = remote.trim().strip_prefix(&prefix)
    {
        return Ok(if branches.iter().any(|local| local == branch) {
            branch.into()
        } else {
            remote.trim().into()
        });
    }
    if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
        bail!("Git lookup timed out or cancelled");
    }
    for branch in ["main", "master"] {
        if branches.iter().any(|candidate| candidate == branch) {
            return Ok(branch.into());
        }
    }
    let head = git_output(
        repo,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        deadline,
        cancelled,
    )?;
    let head = head.trim();
    // A detached checkout reports the literal `HEAD`, which is not a branch.
    if head.is_empty() || head == "HEAD" {
        bail!("No default branch found");
    }
    Ok(head.to_owned())
}

fn git_output(
    repo: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<String> {
    run_command(
        Command::new("git").arg("-C").arg(repo).args(args),
        deadline,
        cancelled,
    )
}

fn run_command(command: &mut Command, deadline: Instant, cancelled: &AtomicBool) -> Result<String> {
    if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
        bail!("Git lookup timed out or cancelled");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("start Git lookup")?;
    let result = (|| {
        let mut stdout = child.stdout.take().context("Git stdout")?;
        let fd = stdout.as_raw_fd();
        // Nonblocking reads let the same worker enforce the deadline without an
        // unbounded reader thread or a pipe-full wait deadlock.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
                bail!("Git lookup timed out or cancelled");
            }
            match stdout.read(&mut buffer) {
                Ok(0) => {
                    if let Some(status) = child.try_wait()? {
                        if !status.success() {
                            bail!("Git lookup failed");
                        }
                        return String::from_utf8(output).context("Git output is not UTF-8");
                    }
                }
                Ok(count) => {
                    if output.len() + count > 256 * 1024 {
                        bail!("Git suggestions exceed 256 KiB");
                    }
                    output.extend_from_slice(&buffer[..count]);
                    continue;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    // Only this worker's direct child is owned. Always reap it, including failure.
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn local_branches(repo: &Path) -> Result<Vec<String>> {
        super::local_branches(
            repo,
            Instant::now() + Duration::from_secs(2),
            &AtomicBool::new(false),
        )
    }

    fn default_branch(repo: &Path) -> Result<String> {
        read_hints(repo, &AtomicBool::new(false)).map(|hints| hints.base)
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn branches_and_remote_default_from_real_repository() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-b", "main"]);
        git(
            repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(repo, &["branch", "topic"]);
        assert_eq!(local_branches(repo).unwrap(), ["main", "topic"]);
        assert_eq!(default_branch(repo).unwrap(), "main");
        git(repo, &["update-ref", "refs/remotes/origin/topic", "HEAD"]);
        git(
            repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/topic",
            ],
        );
        git(repo, &["pack-refs", "--all"]);
        assert_eq!(default_branch(repo).unwrap(), "topic");
        git(repo, &["branch", "-D", "topic"]);
        assert_eq!(default_branch(repo).unwrap(), "refs/remotes/origin/topic");
        git(repo, &["branch", "topic", "refs/remotes/origin/topic"]);
        let worktree = dir.path().join("linked");
        git(
            repo,
            &["worktree", "add", worktree.to_str().unwrap(), "topic"],
        );
        assert_eq!(default_branch(&worktree).unwrap(), "topic");
        assert_eq!(local_branches(&worktree).unwrap(), ["main", "topic"]);
    }

    #[test]
    fn timeout_reaps_owned_child_and_output_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        let start = Instant::now();
        let error = run_command(
            Command::new("/bin/sh")
                .args(["-c", "echo $$ > \"$1\"; exec sleep 10", "fixture"])
                .arg(&pidfile),
            start + Duration::from_millis(200),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(start.elapsed() < Duration::from_secs(2));
        let pid: libc::pid_t = std::fs::read_to_string(pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        let error = run_command(
            Command::new("yes").arg("branch"),
            Instant::now() + Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("256 KiB"), "{error}");
        assert!(read_hints(dir.path(), &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn detached_head_yields_no_base_suggestion() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-b", "trunk"]);
        git(
            repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(repo, &["checkout", "--detach", "HEAD"]);
        let error = default_branch(repo).unwrap_err().to_string();
        assert!(error.contains("default branch"), "{error}");
    }

    #[test]
    fn sole_non_origin_remote_provides_the_base() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-b", "work"]);
        git(
            repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            repo,
            &[
                "remote",
                "add",
                "upstream",
                "https://example.invalid/repo.git",
            ],
        );
        git(
            repo,
            &["update-ref", "refs/remotes/upstream/release", "HEAD"],
        );
        git(
            repo,
            &[
                "symbolic-ref",
                "refs/remotes/upstream/HEAD",
                "refs/remotes/upstream/release",
            ],
        );
        assert_eq!(
            default_branch(repo).unwrap(),
            "refs/remotes/upstream/release"
        );
        git(
            repo,
            &["branch", "release", "refs/remotes/upstream/release"],
        );
        assert_eq!(default_branch(repo).unwrap(), "release");
    }

    #[test]
    fn missing_origin_uses_master_then_current_head() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-b", "master"]);
        git(
            repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        assert_eq!(default_branch(repo).unwrap(), "master");
        git(repo, &["branch", "-m", "trunk"]);
        assert_eq!(default_branch(repo).unwrap(), "trunk");
    }
}
