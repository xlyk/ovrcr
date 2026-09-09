use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
    time::{Duration, Instant},
};
struct Fixture {
    root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            root: tempfile::tempdir().unwrap(),
        };
        let path = f.root.path().join("pi-fixture");
        fs::write(&path,r#"#!/usr/bin/env python3
import sys,json,os,pathlib,select
if '--version' in sys.argv:
 print('0.84.4');sys.exit(0)
def emit(e): print(json.dumps(e),flush=True)
def settle(reason='stop'):
 emit({'type':'message_end','message':{'role':'assistant','content':[{'type':'text','text':'fixture response'}],'stopReason':reason,'errorMessage':'fixture failure' if reason=='error' else None}})
 emit({'type':'agent_end','messages':[],'willRetry':False});emit({'type':'agent_settled'})
active=False
pending=b''
while True:
 ready,_,_=select.select([sys.stdin],[],[],0.05)
 if not ready:
  if active and pathlib.Path('release').exists():settle();active=False
  continue
 chunk=os.read(0,65536)
 if not chunk:break
 pending+=chunk
 lines=pending.split(b'\n');pending=lines.pop()
 for line in lines:
  c=json.loads(line);kind=c['type'];id=c.get('id')
  if kind=='get_state':
   emit({'type':'response','id':id,'command':kind,'success':True,'data':{'isStreaming':active,'model':{'id':'model','provider':'fixture'},'sessionFile':None}})
  elif kind=='prompt':
   pathlib.Path('started.tmp').write_text(str(os.getpid()));pathlib.Path('started.tmp').replace('started')
   pathlib.Path('received-prompt').write_text(c['message'])
   emit({'type':'response','id':id,'command':kind,'success':True})
   emit({'type':'agent_start'})
   emit({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'streamed fixture text\u2028ok'}})
   if c['message']=='HOLD-BACKGROUND':
    import subprocess
    p=subprocess.Popen(['sleep','300'],start_new_session=True)
    pathlib.Path('background.tmp').write_text(str(p.pid));pathlib.Path('background.tmp').replace('background')
   if c['message'] in ('HOLD','HOLD-BACKGROUND'):active=True
   else:settle('error' if c['message']=='FAIL' else 'stop')
  elif kind=='abort':
   if active:settle('aborted');active=False
   emit({'type':'response','id':id,'command':kind,'success':True})
  else:emit({'type':'response','id':id,'command':kind,'success':True,'data':{}})
"#).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        f
    }
    fn raw(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(args)
            .env("OVRCR_CONFIG", self.root.path().join("config.toml"))
            .env("OVRCR_SOCKET", self.root.path().join("server.sock"))
            .env("OVRCR_PI_EXECUTABLE", self.root.path().join("pi-fixture"))
            .output()
            .unwrap()
    }
    fn call(&self, args: &[&str]) -> Value {
        let mut argv = vec!["--json"];
        argv.extend_from_slice(args);
        let output = self.raw(&argv);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn task(&self, name: &str, prompt: &str) -> String {
        self.call(&[
            "task",
            "create",
            name,
            "--scratch",
            "--every",
            "1d",
            "--model",
            "fixture/model",
            "--prompt",
            prompt,
        ])["id"]
            .as_u64()
            .unwrap()
            .to_string()
    }
    fn start(&self, task: &str) -> String {
        self.call(&["task", "run", task])["id"]
            .as_u64()
            .unwrap()
            .to_string()
    }
    fn wait(&self, id: &str, status: &str) -> Value {
        self.wait_for(id, status, Duration::from_secs(15))
    }
    fn wait_for(&self, id: &str, status: &str, limit: Duration) -> Value {
        let end = Instant::now() + limit;
        loop {
            let run = self.call(&["run", "get", id]);
            if run["status"] == status
                && (status != "Running"
                    || run["directory"]
                        .as_str()
                        .is_some_and(|dir| std::path::Path::new(dir).join("started").exists()))
            {
                return run;
            }
            assert!(Instant::now() < end, "wanted {status}, got {run}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Fixture {
    fn run_dir(&self, id: &str) -> std::path::PathBuf {
        self.root.path().join("config.tasks/runs").join(id)
    }
    /// The supervisor records its PID in run.lock once it owns the run.
    fn supervisor_pid(&self, id: &str) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(pid) = fs::read_to_string(self.run_dir(id).join("run.lock"))
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "supervisor never claimed the run"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.raw(&["shutdown", "--kill"]);
        let end = Instant::now() + Duration::from_secs(8);
        while self.root.path().join("server.sock").exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn three_runs_stream_independently_and_cancel_admits_the_fourth() {
    let f = Fixture::new();
    let mut runs = Vec::new();
    for name in ["one", "two", "three", "four"] {
        let task = f.task(name, "HOLD");
        runs.push(f.start(&task));
    }
    let mut dirs = Vec::new();
    for run in &runs[..3] {
        dirs.push(
            f.wait(run, "Running")["directory"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert_ne!(dirs[0], dirs[1]);
    assert_eq!(f.call(&["run", "get", &runs[3]])["status"], "Queued");
    // `started` precedes the delta; wait for the supervisor to persist it too.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let output = f.raw(&["run", "logs", &runs[0]]);
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        if text.contains("streamed fixture text") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "streamed output never arrived: {text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    f.call(&["run", "cancel", &runs[0]]);
    f.wait(&runs[0], "Cancelled");
    f.wait(&runs[3], "Running");
    assert_eq!(f.call(&["run", "get", &runs[1]])["status"], "Running");
    let pid = fs::read_to_string(std::path::Path::new(&dirs[0]).join("started"))
        .unwrap()
        .parse::<i32>()
        .unwrap();
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "cancelled Pi must be reaped"
    );
    for run in &runs[1..] {
        f.call(&["run", "cancel", run]);
        f.wait(run, "Cancelled");
    }
}

#[test]
fn cancel_marks_run_cancelling_immediately() {
    let f = Fixture::new();
    let task = f.task("cancelling", "HOLD");
    let id = f.start(&task);
    f.wait(&id, "Running");
    // A stopped supervisor cannot acknowledge, so the status must come from the server.
    let supervisor = f.supervisor_pid(&id);
    assert_eq!(unsafe { libc::kill(supervisor, libc::SIGSTOP) }, 0);
    f.call(&["run", "cancel", &id]);
    let observed = f.call(&["run", "get", &id])["status"].clone();
    assert_eq!(unsafe { libc::kill(supervisor, libc::SIGCONT) }, 0);
    assert_eq!(observed, "Cancelling");
    f.wait(&id, "Cancelled");
}

#[test]
fn forced_cancel_kills_pi_process_group() {
    let f = Fixture::new();
    let task = f.task("forced", "HOLD-BACKGROUND");
    let id = f.start(&task);
    let run = f.wait(&id, "Running");
    let cwd = std::path::Path::new(run["directory"].as_str().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cwd.join("background").exists() {
        assert!(Instant::now() < deadline, "fixture never detached a child");
        std::thread::sleep(Duration::from_millis(10));
    }
    let pi: i32 = fs::read_to_string(cwd.join("started"))
        .unwrap()
        .parse()
        .unwrap();
    let background: i32 = fs::read_to_string(cwd.join("background"))
        .unwrap()
        .parse()
        .unwrap();
    // Wait until the supervisor has recorded the detached child before freezing it.
    let processes = f.run_dir(&id).join("processes.json");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fs::read_to_string(&processes)
        .unwrap_or_default()
        .contains(&format!("\"pid\": {background}"))
    {
        assert!(
            Instant::now() < deadline,
            "supervisor never recorded {background}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let supervisor = f.supervisor_pid(&id);
    assert_eq!(unsafe { libc::kill(supervisor, libc::SIGSTOP) }, 0);
    f.call(&["run", "cancel", &id]);
    let run = f.wait_for(&id, "CleanupFailed", Duration::from_secs(40));
    assert!(
        !alive(supervisor),
        "supervisor survived forced cancellation"
    );
    let survivors: Vec<i32> = [pi, background]
        .into_iter()
        .filter(|pid| alive(*pid))
        .collect();
    for pid in &survivors {
        unsafe {
            libc::kill(*pid, libc::SIGKILL);
        }
    }
    assert!(
        survivors.is_empty(),
        "processes survived forced cancellation: {survivors:?}"
    );
    assert_eq!(
        unsafe { libc::kill(-pi, 0) },
        -1,
        "Pi process group survived"
    );
    assert!(run["error"].as_str().unwrap().contains("deadline"));
}

#[test]
fn scratch_output_survives_completion_and_requires_confirmed_cleanup() {
    let f = Fixture::new();
    let task = f.task("write", "SUCCESS");
    let id = f.start(&task);
    let run = f.wait(&id, "Succeeded");
    let dir = std::path::Path::new(run["directory"].as_str().unwrap());
    assert!(dir.join("received-prompt").exists());
    assert!(!f.raw(&["run", "clean", &id]).status.success());
    assert!(dir.exists());
    f.call(&["run", "clean", &id, "--yes"]);
    assert!(!dir.exists());
    assert_eq!(f.call(&["run", "get", &id])["status"], "Succeeded");
    let logs = f.raw(&["run", "logs", &id]);
    assert!(String::from_utf8_lossy(&logs.stdout).contains("streamed fixture text"));
}

#[test]
fn timeout_is_distinct_from_failure_and_same_task_pending_is_coalesced() {
    let f = Fixture::new();
    let task = f.task("timeout", "HOLD");
    let first = f.start(&task);
    let active = f.wait(&first, "Running");
    // Keep admission blocked by an acknowledged HOLD run while queueing. The
    // queued snapshot has a short timeout; no assertion races that one-second window.
    f.call(&["task", "update", &task, "--timeout", "1s"]);
    let pending = f.start(&task);
    let repeated = f.start(&task);
    assert_eq!(pending, repeated);
    fs::write(
        std::path::Path::new(active["directory"].as_str().unwrap()).join("release"),
        "",
    )
    .unwrap();
    f.wait(&first, "Succeeded");
    f.wait(&pending, "TimedOut");
    let failed = f.task("failure", "FAIL");
    let run = f.start(&failed);
    assert_eq!(f.wait(&run, "Failed")["status"], json!("Failed"));
}

#[test]
fn git_runs_fetch_remote_head_keep_files_and_preserve_branches_on_cleanup() {
    let f = Fixture::new();
    let remote = f.root.path().join("remote");
    let repo = f.root.path().join("checkout");
    fs::create_dir(&remote).unwrap();
    let git = |cwd: &std::path::Path, args: &[&str]| -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.invalid"],
        vec!["commit", "--allow-empty", "-m", "initial"],
    ] {
        git(&remote, &args);
    }
    git(
        f.root.path(),
        &["clone", remote.to_str().unwrap(), repo.to_str().unwrap()],
    );
    // Move only the remote after cloning: each run must use its fetched commit.
    fs::write(remote.join("remote-file"), "latest remote content").unwrap();
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-m", "new remote head"]);
    let sha = git(&remote, &["rev-parse", "HEAD"]);
    fs::create_dir(f.root.path().join("workspaces")).unwrap();
    f.call(&[
        "project",
        "add",
        "fixture",
        repo.to_str().unwrap(),
        "--workspace-root",
        f.root.path().join("workspaces").to_str().unwrap(),
    ]);
    let rejected = f.raw(&[
        "task",
        "create",
        "bad-remote",
        "--project",
        "fixture",
        "--remote",
        "https://user:secret@example.invalid/repo",
        "--branch",
        "main",
        "--every",
        "1d",
        "--model",
        "fixture/model",
        "--prompt",
        "SUCCESS",
    ]);
    assert!(!rejected.status.success());
    assert!(
        !fs::read_to_string(f.root.path().join("config.tasks/state.toml"))
            .unwrap()
            .contains("user:secret")
    );
    let task = f.call(&[
        "task",
        "create",
        "git-run",
        "--project",
        "fixture",
        "--remote",
        "origin",
        "--branch",
        "main",
        "--every",
        "1d",
        "--model",
        "fixture/model",
        "--prompt",
        "SUCCESS",
    ])["id"]
        .as_u64()
        .unwrap()
        .to_string();
    assert!(
        !f.raw(&["project", "delete", "fixture"]).status.success(),
        "task dependency must guard project deletion"
    );
    let id = f.start(&task);
    let run = f.wait(&id, "Succeeded");
    let cwd = std::path::Path::new(run["directory"].as_str().unwrap());
    assert_eq!(run["base_commit"], sha);
    assert_eq!(
        fs::read_to_string(cwd.join("remote-file")).unwrap(),
        "latest remote content"
    );
    assert_eq!(git(cwd, &["rev-parse", "HEAD"]), sha);
    assert!(
        f.call(&["terminal", "list", "--project", "fixture"])
            .as_array()
            .unwrap()
            .is_empty(),
        "task preparation must not start a shell"
    );
    assert!(
        !f.raw(&["run", "clean", &id, "--yes"]).status.success(),
        "dirty worktree must be retained"
    );
    for name in ["started", "received-prompt"] {
        fs::remove_file(cwd.join(name)).unwrap();
    }
    f.call(&["run", "clean", &id]);
    assert!(!cwd.exists());
    git(
        &repo,
        &[
            "show-ref",
            "--verify",
            &format!("refs/heads/ovrcr/task-{task}/run-{id}"),
        ],
    );
    assert_eq!(f.call(&["run", "get", &id])["status"], "Succeeded");
}

#[test]
fn server_crash_interrupts_pi_and_recovery_does_not_replay_the_run() {
    let f = Fixture::new();
    let mut server = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .arg("server")
        .env("OVRCR_CONFIG", f.root.path().join("config.toml"))
        .env("OVRCR_SOCKET", f.root.path().join("server.sock"))
        .env("OVRCR_PI_EXECUTABLE", f.root.path().join("pi-fixture"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(
            fs::File::create(f.root.path().join("server.log")).unwrap(),
        ))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f.root.path().join("server.sock").exists() {
        assert!(Instant::now() < deadline, "server startup timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    let task = f.task("crash", "HOLD");
    let id = f.start(&task);
    let run = f.wait(&id, "Running");
    let cwd = std::path::Path::new(run["directory"].as_str().unwrap());
    let pid: i32 = fs::read_to_string(cwd.join("started"))
        .unwrap()
        .parse()
        .unwrap();
    server.kill().unwrap();
    server.wait().unwrap();
    // A write restarts the server, which must await supervisor cleanup before admission.
    f.call(&["task", "concurrency", "3"]);
    f.wait(&id, "Interrupted");
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "orphan Pi process remains"
    );
    assert_eq!(
        f.call(&["run", "list", "--task", &task])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(cwd.join("received-prompt").exists());
}

#[test]
fn shutdown_finishes_even_when_final_state_persistence_fails() {
    let f = Fixture::new();
    let task = f.task("shutdown-fault", "HOLD");
    let id = f.start(&task);
    f.wait(&id, "Running");
    let state = f.root.path().join("config.tasks/state.toml");
    let backup = state.with_extension("backup");
    fs::rename(&state, &backup).unwrap();
    fs::create_dir(&state).unwrap();
    let result = f.raw(&["shutdown", "--kill"]);
    assert!(
        !result.status.success(),
        "persistence fault must be reported"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while f.root.path().join("server.sock").exists() {
        assert!(Instant::now() < deadline, "server left half-stopped");
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::remove_dir(&state).unwrap();
    fs::rename(&backup, &state).unwrap();
    f.call(&["task", "concurrency", "3"]);
    f.wait(&id, "Cancelled");
}

#[test]
fn signal_shutdown_cleans_up_despite_final_state_write_failure() {
    let f = Fixture::new();
    let mut server = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .arg("server")
        .env("OVRCR_CONFIG", f.root.path().join("config.toml"))
        .env("OVRCR_SOCKET", f.root.path().join("server.sock"))
        .env("OVRCR_PI_EXECUTABLE", f.root.path().join("pi-fixture"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f.root.path().join("server.sock").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let task = f.task("signal-fault", "HOLD");
    let id = f.start(&task);
    f.wait(&id, "Running");
    let state = f.root.path().join("config.tasks/state.toml");
    let backup = state.with_extension("backup");
    fs::rename(&state, &backup).unwrap();
    fs::create_dir(&state).unwrap();
    assert_eq!(unsafe { libc::kill(server.id() as i32, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "signal left server half-stopped");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!f.root.path().join("server.sock").exists());
    fs::remove_dir(&state).unwrap();
    fs::rename(&backup, &state).unwrap();
    assert_eq!(f.call(&["run", "get", &id])["status"], "Cancelled");
}

fn git(cwd: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
fn git_task_fixture(f: &Fixture) -> (std::path::PathBuf, String) {
    let repo = f.root.path().join("repo");
    let workspaces = f.root.path().join("workspaces");
    fs::create_dir(&repo).unwrap();
    fs::create_dir(&workspaces).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.invalid"],
        vec!["commit", "--allow-empty", "-m", "base"],
    ] {
        git(&repo, &args);
    }
    git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    f.call(&[
        "project",
        "add",
        "fixture",
        repo.to_str().unwrap(),
        "--workspace-root",
        workspaces.to_str().unwrap(),
    ]);
    let task = f.call(&[
        "task",
        "create",
        "git-task",
        "--project",
        "fixture",
        "--remote",
        "origin",
        "--branch",
        "main",
        "--every",
        "1d",
        "--model",
        "fixture/model",
        "--prompt",
        "HOLD",
    ])["id"]
        .as_u64()
        .unwrap()
        .to_string();
    (repo, task)
}
fn create_unrelated_workspace(f: &Fixture, name: &str, branch: &str, new_branch: bool) {
    let mut args = vec![
        "workspace",
        "create",
        "--project",
        "fixture",
        "--name",
        name,
        if new_branch {
            "--new-branch"
        } else {
            "--branch"
        },
        branch,
    ];
    if new_branch {
        args.extend(["--base", "main"]);
    }
    f.call(&args);
    for session in f
        .call(&["terminal", "list", "--project", "fixture"])
        .as_array()
        .unwrap()
    {
        let id = session["id"].as_u64().unwrap().to_string();
        f.call(&["terminal", "kill", &id]);
        f.call(&["terminal", "remove", &id]);
    }
}

#[test]
fn git_run_cleanup_rejects_a_workspace_that_preexisted_preparation() {
    for branch in ["existing-owner", "ovrcr/task-1/run-1"] {
        let f = Fixture::new();
        let (repo, task) = git_task_fixture(&f);
        create_unrelated_workspace(&f, "task-1-run-1", branch, true);
        let id = f.start(&task);
        let run = f.wait(&id, "Failed");
        assert!(
            run["error"]
                .as_str()
                .unwrap()
                .contains("duplicate workspace")
        );
        let clean = f.raw(&["run", "clean", &id]);
        assert!(!clean.status.success(), "cleanup claimed another workspace");
        assert!(f.root.path().join("workspaces/task-1-run-1").is_dir());
        f.call(&[
            "workspace",
            "get",
            "--project",
            "fixture",
            "--name",
            "task-1-run-1",
        ]);
        git(
            &repo,
            &["show-ref", "--verify", &format!("refs/heads/{branch}")],
        );
    }
}

#[test]
fn git_workspace_removal_refuses_a_running_task_in_a_clean_worktree() {
    let f = Fixture::new();
    let (repo, task) = git_task_fixture(&f);
    let id = f.start(&task);
    let run = f.wait(&id, "Running");
    let cwd = std::path::Path::new(run["directory"].as_str().unwrap());
    let marker_deadline = Instant::now() + Duration::from_secs(15);
    while !["started", "received-prompt"]
        .iter()
        .all(|name| cwd.join(name).is_file())
    {
        assert!(
            Instant::now() < marker_deadline,
            "fixture readiness markers did not appear in {}",
            cwd.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    for name in ["started", "received-prompt"] {
        fs::remove_file(cwd.join(name)).unwrap();
    }
    assert!(git(cwd, &["status", "--porcelain"]).is_empty());
    let removed = f.raw(&[
        "workspace",
        "remove",
        "--project",
        "fixture",
        "--name",
        "task-1-run-1",
    ]);
    assert!(
        !removed.status.success(),
        "removed an active task's worktree"
    );
    assert!(cwd.is_dir());
    assert!(git(&repo, &["worktree", "list", "--porcelain"]).contains(cwd.to_str().unwrap()));
    assert_eq!(f.call(&["run", "get", &id])["status"], "Running");
    f.call(&["run", "cancel", &id]);
    f.wait(&id, "Cancelled");
    f.call(&["run", "clean", &id]);
    assert!(!cwd.exists());
}

#[test]
fn git_run_cleanup_rejects_a_replacement_workspace_after_cleanup() {
    for (branch, new_branch, run_cleanup) in [
        ("replacement-owner", true, true),
        ("ovrcr/task-1/run-1", false, true),
        ("ovrcr/task-1/run-1", false, false),
    ] {
        let f = Fixture::new();
        let (repo, task) = git_task_fixture(&f);
        let id = f.start(&task);
        let run = f.wait(&id, "Running");
        f.call(&["run", "cancel", &id]);
        f.wait(&id, "Cancelled");
        let cwd = std::path::Path::new(run["directory"].as_str().unwrap());
        for name in ["started", "received-prompt"] {
            fs::remove_file(cwd.join(name)).unwrap();
        }
        if run_cleanup {
            f.call(&["run", "clean", &id]);
            f.call(&["run", "clean", &id]);
        } else {
            f.call(&[
                "workspace",
                "remove",
                "--project",
                "fixture",
                "--name",
                "task-1-run-1",
            ]);
        }
        create_unrelated_workspace(&f, "task-1-run-1", branch, new_branch);
        let clean = f.raw(&["run", "clean", &id]);
        assert!(
            !clean.status.success(),
            "old run claimed its replacement workspace"
        );
        assert!(cwd.is_dir());
        f.call(&[
            "workspace",
            "get",
            "--project",
            "fixture",
            "--name",
            "task-1-run-1",
        ]);
        git(
            &repo,
            &["show-ref", "--verify", &format!("refs/heads/{branch}")],
        );
    }
}
