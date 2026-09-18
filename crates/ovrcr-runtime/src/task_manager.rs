//! Persistent scheduling and server-side lifecycle of independent Pi runs.
use crate::server::ServerState;
use crate::tasks::*;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub use ovrcr_protocol::{TaskRequest, TaskResponse};

struct Job {
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}
pub struct TaskManager {
    pub directory: PathBuf,
    store: Mutex<TaskStore>,
    jobs: Mutex<HashMap<RunId, Job>>,
    preparation: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    stopping: AtomicBool,
    stop_lock: Mutex<()>,
    admission: Mutex<()>,
    #[cfg(test)]
    after_admit: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    before_execute: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    scheduler: Mutex<Option<JoinHandle<()>>>,
    _owner: File,
}
impl TaskManager {
    pub fn open(registry: &Path) -> Result<Arc<Self>> {
        let directory = tasks_dir(registry);
        fs::create_dir_all(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let owner = lock_file(&directory.join("owner.lock"))?
            .context("another server owns this task store")?;
        let mut store = TaskStore::load(&directory.join("state.toml"))?;
        merge_runs(&directory, &mut store)?;
        // A supervisor retains its lock while handling parent EOF. Never replay a claimed occurrence.
        for run in &mut store.runs {
            if run.status.occupies_slot() {
                let dir = run_dir(&directory, run.id);
                fs::create_dir_all(&dir)?;
                let deadline = Instant::now() + Duration::from_secs(20);
                let lock = loop {
                    if let Some(lock) = lock_file(&dir.join("run.lock"))? {
                        break lock;
                    }
                    if Instant::now() >= deadline {
                        bail!(
                            "run {} is still cleaning up; scheduler recovery blocked",
                            run.id.0
                        );
                    }
                    thread::sleep(Duration::from_millis(50));
                };
                if let Ok(latest) = read_run(&dir) {
                    *run = latest;
                }
                if run.status.occupies_slot() {
                    run.status = RunStatus::Interrupted;
                    run.finished_at = Some(now());
                    run.error = Some(
                        "server stopped before this run completed; manual rerun required".into(),
                    );
                    write_run(&dir, run)?;
                }
                drop(lock);
            }
        }
        store.save_atomic(&directory.join("state.toml"))?;
        Ok(Arc::new(Self {
            directory,
            store: Mutex::new(store),
            jobs: Mutex::new(HashMap::new()),
            preparation: Mutex::new(HashMap::new()),
            stopping: AtomicBool::new(false),
            stop_lock: Mutex::new(()),
            admission: Mutex::new(()),
            #[cfg(test)]
            after_admit: Mutex::new(None),
            #[cfg(test)]
            before_execute: Mutex::new(None),
            scheduler: Mutex::new(None),
            _owner: owner,
        }))
    }
    pub fn start(self: &Arc<Self>, server: Weak<ServerState>) {
        let manager = Arc::downgrade(self);
        *self.scheduler.lock().unwrap() = Some(thread::spawn(move || {
            loop {
                let Some(manager) = manager.upgrade() else {
                    break;
                };
                let Some(server) = server.upgrade() else {
                    break;
                };
                if manager.stopping.load(Ordering::Acquire)
                    || server.shutdown.load(Ordering::Acquire)
                {
                    break;
                }
                if let Err(error) = manager.tick(&server) {
                    eprintln!("task scheduler: {error:#}");
                }
                drop(server);
                drop(manager);
                thread::sleep(Duration::from_millis(100));
            }
        }));
    }
    fn change<T>(&self, f: impl FnOnce(&mut TaskStore) -> Result<T>) -> Result<T> {
        let mut store = self.store.lock().unwrap();
        let mut next = store.clone();
        merge_runs(&self.directory, &mut next)?;
        let result = f(&mut next)?;
        if next != *store {
            next.save_atomic(&self.directory.join("state.toml"))?;
        }
        *store = next;
        Ok(result)
    }
    pub(crate) fn admission_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.admission.lock().unwrap()
    }
    pub(crate) fn quiesce(&self) {
        self.stopping.store(true, Ordering::Release);
    }
    pub fn references_project(&self, project: &str) -> bool {
        self.store.lock().unwrap().tasks.iter().any(|task| matches!(&task.spec.target, TaskTarget::Git { project: name, .. } if name == project))
    }
    pub(crate) fn occupies_workspace(&self, project: &str, workspace: &str) -> Result<bool> {
        let mut store = self.store.lock().unwrap().clone();
        merge_runs(&self.directory, &mut store)?;
        Ok(store.runs.iter().any(|run| {
            run.status.occupies_slot()
                && matches!(&run.spec.target, TaskTarget::Git { project: git_project, .. } if git_project == project)
                && (run.workspace.as_deref() == Some(workspace)
                    || workspace == format!("task-{}-run-{}", run.task_id.0, run.id.0))
        }))
    }
    pub(crate) fn release_workspace_ownership(
        &self,
        project: &str,
        workspace: &crate::config::WorkspaceRecord,
    ) -> Result<()> {
        self.change(|store| {
            merge_runs(&self.directory, store)?;
            for run in &mut store.runs {
                if run.status.is_terminal()
                    && matches!(&run.spec.target, TaskTarget::Git { project: name, .. } if name == project)
                    && run.workspace.as_deref() == Some(workspace.id.as_str())
                    && run.directory.as_ref() == Some(&workspace.path)
                {
                    run.workspace = None;
                    write_run(&run_dir(&self.directory, run.id), run)?;
                }
            }
            Ok(())
        })
    }
    /// Re-read one run's `run.json` regardless of status, after another writer changed it.
    fn refresh_run(&self, id: RunId) -> Result<()> {
        self.change(|store| {
            let dir = run_dir(&self.directory, id);
            if let Some(run) = store.runs.iter_mut().find(|run| run.id == id)
                && dir.join("run.json").exists()
            {
                let latest = read_run(&dir)?;
                if latest.id == id {
                    *run = latest;
                }
            }
            Ok(())
        })
    }

    fn tick(self: &Arc<Self>, server: &Arc<ServerState>) -> Result<()> {
        let _admission = self.admission_guard();
        if self.stopping.load(Ordering::Acquire) {
            return Ok(());
        }
        {
            let mut jobs = self.jobs.lock().unwrap();
            let finished = jobs
                .iter()
                .filter_map(|(id, job)| job.thread.is_finished().then_some(*id))
                .collect::<Vec<_>>();
            for id in finished {
                if let Some(job) = jobs.remove(&id) {
                    let _ = job.thread.join();
                }
            }
        }
        let (admitted, pruned) = self.change(|store| {
            merge_runs(&self.directory, store)?;
            store.advance_due(now())?;
            let pruned = store.prune(now());
            Ok((store.admit(now()), pruned))
        })?;
        for id in pruned {
            match fs::remove_dir_all(run_dir(&self.directory, id)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => eprintln!("prune run {}: {error}", id.0),
            }
        }
        #[cfg(test)]
        if !admitted.is_empty()
            && let Some(hook) = self.after_admit.lock().unwrap().as_ref()
        {
            hook();
        }
        for run in admitted {
            let cancel = Arc::new(AtomicBool::new(false));
            let manager = Arc::clone(self);
            let worker_server = Arc::clone(server);
            let flag = Arc::clone(&cancel);
            let id = run.id;
            let worker = thread::spawn(move || {
                if let Err(error) = manager.execute(&worker_server, run.clone(), &flag) {
                    let mut result = read_run(&run_dir(&manager.directory, id)).unwrap_or(run);
                    if !result.status.is_terminal() {
                        result.status = if flag.load(Ordering::Acquire) {
                            RunStatus::Cancelled
                        } else {
                            RunStatus::Failed
                        };
                        result.finished_at = Some(now());
                        result.error = Some(format!("{error:#}"));
                    }
                    if let Err(write_error) = write_run(&run_dir(&manager.directory, id), &result) {
                        eprintln!("save run {}: {write_error:#}", id.0);
                    }
                }
                let _ = manager.change(|store| merge_runs(&manager.directory, store));
            });
            self.jobs.lock().unwrap().insert(
                id,
                Job {
                    cancel,
                    thread: worker,
                },
            );
        }
        Ok(())
    }
    fn execute(&self, server: &ServerState, mut run: Run, cancel: &AtomicBool) -> Result<()> {
        #[cfg(test)]
        if let Some(hook) = self.before_execute.lock().unwrap().as_ref() {
            hook();
            bail!("controlled execution boundary");
        }
        let dir = run_dir(&self.directory, run.id);
        write_run(&dir, &run)?;
        if cancel.load(Ordering::Acquire) {
            bail!("run cancelled before preparation");
        }
        match &run.spec.target {
            TaskTarget::Scratch => {
                let cwd = dir.join("work");
                fs::create_dir(&cwd).context("create scratch directory")?;
                run.directory = Some(cwd);
                write_run(&dir, &run)?;
            }
            TaskTarget::Git {
                project,
                remote,
                branch,
            } => {
                let preparation = {
                    let mut locks = self.preparation.lock().unwrap();
                    Arc::clone(
                        locks
                            .entry(project.clone())
                            .or_insert_with(|| Arc::new(Mutex::new(()))),
                    )
                };
                let _guard = preparation.lock().unwrap();
                let record = server.registry.lock().unwrap().project(project)?.clone();
                if remote.is_empty()
                    || remote.starts_with('-')
                    || branch.is_empty()
                    || branch.starts_with('-')
                {
                    bail!("invalid remote or branch");
                }
                git_checked(
                    &record.repo,
                    &["check-ref-format", &format!("refs/heads/{branch}")],
                    &dir,
                    cancel,
                )?;
                git_checked(&record.repo, &["remote", "get-url", remote], &dir, cancel)?;
                git_checked(
                    &record.repo,
                    &[
                        "fetch",
                        "--no-tags",
                        "--",
                        remote,
                        &format!("refs/heads/{branch}"),
                    ],
                    &dir,
                    cancel,
                )?;
                let base = git_checked(
                    &record.repo,
                    &["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
                    &dir,
                    cancel,
                )?
                .trim()
                .to_owned();
                let name = format!("task-{}-run-{}", run.task_id.0, run.id.0);
                // Save intent before worktree creation so a partial preparation can be inspected.
                run.directory = Some(record.workspace_root.join(&name));
                run.base_commit = Some(base.clone());
                write_run(&dir, &run)?;
                server.create_task_workspace(
                    project.clone(),
                    name.clone(),
                    format!("ovrcr/task-{}/run-{}", run.task_id.0, run.id.0),
                    base,
                )?;
                // A directory is preparation intent; only successful creation grants ownership.
                run.workspace = Some(name);
                write_run(&dir, &run)?;
            }
        }
        if cancel.load(Ordering::Acquire) {
            bail!("run cancelled during preparation");
        }
        let pi = std::env::var_os("OVRCR_PI_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("pi"));
        let exe = std::env::var_os("OVRCR_SERVER_EXECUTABLE")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_exe)?;
        let mut child = Command::new(exe)
            .arg("__task-runner")
            .arg(&dir)
            .arg(pi)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("supervisor.log"))?,
            ))
            .process_group(0)
            .spawn()
            .context("start Pi run supervisor")?;
        let mut input = child.stdin.take().context("supervisor control pipe")?;
        let mut cancel_sent = None;
        loop {
            if let Some(status) = child.try_wait()? {
                drop(input);
                let final_run = read_run(&dir)?;
                if !final_run.status.is_terminal() {
                    bail!("Pi supervisor exited ({status}) before recording an outcome");
                }
                return Ok(());
            }
            if cancel.load(Ordering::Acquire) && cancel_sent.is_none() {
                let _ = input.write_all(b"cancel\n");
                let _ = input.flush();
                cancel_sent = Some(Instant::now());
            }
            if cancel_sent.is_some_and(|at| at.elapsed() > Duration::from_secs(20)) {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGTERM);
                }
                // The supervisor's own bounded cleanup takes up to four seconds.
                let deadline = Instant::now() + Duration::from_secs(5);
                while child.try_wait()?.is_none() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(20));
                }
                if child.try_wait()?.is_none() {
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.wait();
                }
                // Pi and its anchors live in their own groups; finish what the supervisor recorded.
                if let Err(error) = crate::task_runner::kill_recorded_processes(&dir) {
                    eprintln!("run {}: kill recorded processes: {error:#}", run.id.0);
                }
                let mut failed = read_run(&dir)?;
                if failed.status.is_terminal() {
                    return Ok(());
                }
                failed.status = RunStatus::CleanupFailed;
                failed.finished_at = Some(now());
                failed.error=Some("Pi supervisor exceeded cancellation deadline; inspect retained process logs before rerunning".into());
                write_run(&dir, &failed)?;
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn has_active(&self) -> bool {
        self.store
            .lock()
            .unwrap()
            .runs
            .iter()
            .any(|run| run.status.occupies_slot())
    }
    pub fn stop(&self) -> Result<()> {
        let _stop = self.stop_lock.lock().unwrap();
        {
            let _admission = self.admission_guard();
            self.quiesce();
        }
        if let Some(thread) = self.scheduler.lock().unwrap().take() {
            let _ = thread.join();
        }
        let jobs = std::mem::take(&mut *self.jobs.lock().unwrap());
        for job in jobs.values() {
            job.cancel.store(true, Ordering::Release);
        }
        let mut failure = None;
        for (_, job) in jobs {
            if job.thread.join().is_err() {
                failure = Some(anyhow::anyhow!("task worker panicked"));
            }
        }
        let saved = self.change(|store| merge_runs(&self.directory, store));
        if let Some(error) = failure {
            return Err(error);
        }
        saved
    }
    pub fn handle(&self, server: &ServerState, request: TaskRequest) -> Result<TaskResponse> {
        if request.is_read_only() {
            let mut store = self.store.lock().unwrap().clone();
            merge_runs(&self.directory, &mut store)?;
            return inspect_store(&self.directory, &store, request);
        }
        if self.stopping.load(Ordering::Acquire) {
            bail!("server is stopping");
        }
        match request {
            TaskRequest::Create(spec) => {
                let _mutation = server.task_mutation_guard();
                validate_target(server, &spec)?;
                self.change(|s| s.create(spec, now()).map(TaskResponse::Task))
            }
            TaskRequest::Update { id, spec } => {
                let _mutation = server.task_mutation_guard();
                validate_target(server, &spec)?;
                self.change(|s| s.update(id, spec, now()).map(TaskResponse::Task))
            }
            TaskRequest::Pause(id) => self.change(|s| {
                s.pause(id, now())?;
                Ok(TaskResponse::Ok)
            }),
            TaskRequest::Resume(id) => self.change(|s| {
                s.resume(id, now())?;
                Ok(TaskResponse::Ok)
            }),
            TaskRequest::Delete(id) => self.change(|s| {
                s.delete(id, now())?;
                Ok(TaskResponse::Ok)
            }),
            TaskRequest::Enqueue(id) => self.change(|s| {
                s.enqueue(id, RunTrigger::Manual, now())
                    .map(TaskResponse::Run)
            }),
            TaskRequest::Concurrency(Some(limit)) => self.change(|s| {
                if limit == 0 {
                    bail!("concurrency must be positive");
                }
                s.max_concurrent = limit;
                Ok(TaskResponse::Concurrency(limit))
            }),
            TaskRequest::Cancel(id) => self.cancel_run(id),
            TaskRequest::Clean { id, confirmed } => {
                let mut store = self.store.lock().unwrap().clone();
                merge_runs(&self.directory, &mut store)?;
                let run = store
                    .runs
                    .iter()
                    .find(|r| r.id == id)
                    .context("run not found")?;
                if !run.status.is_terminal() {
                    bail!("run is still active");
                }
                if let Some(cwd) = &run.directory {
                    match &run.spec.target {
                        TaskTarget::Git { .. } => {
                            server.remove_task_workspace(run, &run_dir(&self.directory, id))?;
                            self.refresh_run(id)?;
                        }
                        TaskTarget::Scratch => {
                            if !confirmed {
                                bail!("scratch cleanup requires --yes confirmation");
                            }
                            let expected = run_dir(&self.directory, id).join("work");
                            if cwd != &expected {
                                bail!("unexpected scratch directory");
                            }
                            if cwd.exists() {
                                let metadata = fs::symlink_metadata(cwd)?;
                                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                                    bail!("unexpected scratch directory type");
                                }
                                fs::remove_dir_all(cwd)?;
                            }
                        }
                    }
                }
                Ok(TaskResponse::Ok)
            }
            _ => unreachable!("read-only request handled above"),
        }
    }
    fn cancel_run(&self, id: RunId) -> Result<TaskResponse> {
        let _admission = self.admission_guard();
        self.change(|store| {
            let run = store
                .runs
                .iter_mut()
                .find(|run| run.id == id)
                .context("run not found")?;
            if run.status == RunStatus::Queued {
                run.status = RunStatus::Cancelled;
                run.finished_at = Some(now());
            } else if !run.status.is_terminal() {
                let jobs = self.jobs.lock().unwrap();
                let job = jobs.get(&id).context("active run has no supervisor")?;
                job.cancel.store(true, Ordering::Release);
                run.status = RunStatus::Cancelling;
            }
            Ok(TaskResponse::Ok)
        })
    }
    pub fn inspect_offline(registry: &Path, request: TaskRequest) -> Result<TaskResponse> {
        let directory = tasks_dir(registry);
        let mut store = TaskStore::load(&directory.join("state.toml"))?;
        merge_runs(&directory, &mut store)?;
        inspect_store(&directory, &store, request)
    }
}
fn validate_target(server: &ServerState, spec: &TaskSpec) -> Result<()> {
    spec.validate()?;
    if let TaskTarget::Git {
        project, remote, ..
    } = &spec.target
    {
        let repo = server
            .registry
            .lock()
            .unwrap()
            .project(project)?
            .repo
            .clone();
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("remote")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .stdin(Stdio::null())
            .output()
            .context("list project remotes")?;
        if !output.status.success()
            || !String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|name| name == remote)
        {
            bail!("--remote must name an existing project remote");
        }
    }
    Ok(())
}
fn inspect_store(dir: &Path, store: &TaskStore, request: TaskRequest) -> Result<TaskResponse> {
    Ok(match request {
        TaskRequest::ListTasks => return crate::task_paging::tasks(&store.tasks, 0),
        TaskRequest::PageTasks { offset } => {
            return crate::task_paging::tasks(&store.tasks, offset);
        }
        TaskRequest::PageRuns { task, offset } => {
            return crate::task_paging::runs(&store.runs, task, offset);
        }
        TaskRequest::GetTask(id) => TaskResponse::Task(
            store
                .tasks
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .context("task not found")?,
        ),
        TaskRequest::ListRuns(task) => return crate::task_paging::runs(&store.runs, task, 0),
        TaskRequest::GetRun(id) => TaskResponse::Run(
            store
                .runs
                .iter()
                .find(|r| r.id == id)
                .cloned()
                .context("run not found")?,
        ),
        TaskRequest::Concurrency(None) => TaskResponse::Concurrency(store.max_concurrent),
        TaskRequest::ReadLog {
            id,
            offset,
            max_bytes,
        } => {
            let run = store
                .runs
                .iter()
                .find(|r| r.id == id)
                .context("run not found")?;
            let path = run_dir(dir, id);
            let (bytes, next_offset) = if path.join("events.jsonl").exists() {
                crate::task_runner::read_log(&path, offset, max_bytes.clamp(1, 64 * 1024))?
            } else {
                (Vec::new(), offset)
            };
            TaskResponse::Log {
                bytes,
                next_offset,
                terminal: run.status.is_terminal(),
            }
        }
        _ => bail!("request requires a running server"),
    })
}
/// Refresh in-flight runs from their `run.json`. Terminal runs are history and are
/// never re-read; a damaged file interrupts its run instead of failing the caller.
fn merge_runs(dir: &Path, store: &mut TaskStore) -> Result<()> {
    for run in &mut store.runs {
        if run.status == RunStatus::Queued || run.status.is_terminal() {
            continue;
        }
        let path = run_dir(dir, run.id);
        if !path.join("run.json").exists() {
            continue;
        }
        match read_run(&path).and_then(|latest| {
            if latest.id != run.id {
                bail!(
                    "run metadata ID mismatch in {}",
                    path.join("run.json").display()
                );
            }
            Ok(latest)
        }) {
            Ok(latest) => {
                // The supervisor does not know about cancellation until it finishes.
                let cancelling =
                    run.status == RunStatus::Cancelling && !latest.status.is_terminal();
                *run = latest;
                if cancelling {
                    run.status = RunStatus::Cancelling;
                }
            }
            Err(error) => {
                eprintln!("task run {}: {error:#}; marking it interrupted", run.id.0);
                run.status = RunStatus::Interrupted;
                run.finished_at = Some(now());
                run.error = Some(format!("{error:#}"));
            }
        }
    }
    Ok(())
}
pub fn lock_file(path: &Path) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(Some(file))
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(error.into())
        }
    }
}
fn git_checked(repo: &Path, args: &[&str], dir: &Path, cancel: &AtomicBool) -> Result<String> {
    let output = dir.join("git.stdout");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .stdin(Stdio::null())
        .stdout(Stdio::from(File::create(&output)?))
        .stderr(Stdio::from(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("prepare.log"))?,
        ))
        .process_group(0)
        .spawn()
        .context("start git preparation")?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!(
                    "git {} failed; see {}",
                    args[0],
                    dir.join("prepare.log").display()
                );
            }
            return fs::read_to_string(output).context("read git result");
        }
        if cancel.load(Ordering::Acquire) || Instant::now() > deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!("git preparation cancelled or timed out");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{ErrorCode, Response};
    use std::sync::mpsc;
    type Hook = Arc<dyn Fn() + Send + Sync>;
    fn gate() -> (Hook, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
        let (arrive, arrived) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let released = Mutex::new(released);
        (
            Arc::new(move || {
                arrive.send(()).unwrap();
                released.lock().unwrap().recv().unwrap();
            }),
            arrived,
            release,
        )
    }
    fn fixture() -> (tempfile::TempDir, Arc<TaskManager>, Arc<ServerState>, RunId) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let manager = TaskManager::open(&path).unwrap();
        let server = ServerState::with_tasks_for_test(manager.clone(), path);
        let id = manager
            .change(|s| {
                let task = s.create(
                    TaskSpec {
                        name: "boundary".into(),
                        prompt: "test".into(),
                        target: TaskTarget::Scratch,
                        schedule: Schedule::Interval { seconds: 86400 },
                        model: "fixture/model".into(),
                        thinking: "off".into(),
                        timeout_seconds: 3600,
                    },
                    now(),
                )?;
                Ok(s.enqueue(task.id, RunTrigger::Manual, now())?.id)
            })
            .unwrap();
        (root, manager, server, id)
    }
    #[test]
    fn corrupt_run_file_does_not_block_open() {
        let (root, manager, server, id) = fixture();
        manager
            .change(|store| {
                store.runs[0].status = RunStatus::Running;
                Ok(())
            })
            .unwrap();
        let dir = run_dir(&manager.directory, id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("run.json"), "{not json").unwrap();
        drop(server);
        drop(manager);
        let manager = TaskManager::open(&root.path().join("config.toml")).unwrap();
        let run = manager.store.lock().unwrap().runs[0].clone();
        assert_eq!(run.status, RunStatus::Interrupted);
        assert!(
            run.error
                .as_deref()
                .is_some_and(|error| error.contains(dir.join("run.json").to_str().unwrap())),
            "{:?}",
            run.error
        );
    }
    #[test]
    fn tick_does_not_reread_terminal_runs() {
        let (_root, manager, server, _id) = fixture();
        manager
            .change(|store| {
                let task = store.tasks[0].id;
                for _ in 0..500 {
                    let mut run = store.enqueue(task, RunTrigger::Manual, now())?;
                    run.status = RunStatus::Succeeded;
                    run.finished_at = Some(now());
                    let dir = run_dir(&manager.directory, run.id);
                    fs::create_dir_all(&dir)?;
                    fs::write(dir.join("run.json"), serde_json::to_vec(&run)?)?;
                    let id = run.id;
                    *store.runs.iter_mut().find(|r| r.id == id).unwrap() = run;
                }
                Ok(())
            })
            .unwrap();
        READ_RUN_CALLS.with(|calls| calls.set(0));
        for _ in 0..10 {
            manager.tick(&server).unwrap();
        }
        assert_eq!(READ_RUN_CALLS.with(|calls| calls.get()), 0);
    }
    #[test]
    fn cancellation_waits_for_admission_to_publish_its_worker() {
        let (_root, manager, server, id) = fixture();
        let (hook, admitted, release) = gate();
        *manager.after_admit.lock().unwrap() = Some(hook);
        let (hook, executing, finish) = gate();
        *manager.before_execute.lock().unwrap() = Some(hook);
        let tick = {
            let manager = manager.clone();
            thread::spawn(move || manager.tick(&server).unwrap())
        };
        admitted.recv_timeout(Duration::from_secs(2)).unwrap();
        let (entered, ready) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let cancel = {
            let manager = manager.clone();
            thread::spawn(move || {
                entered.send(()).unwrap();
                tx.send(manager.cancel_run(id)).unwrap();
            })
        };
        ready.recv().unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release.send(()).unwrap();
        executing.recv_timeout(Duration::from_secs(2)).unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        assert!(
            manager.jobs.lock().unwrap()[&id]
                .cancel
                .load(Ordering::Acquire)
        );
        finish.send(()).unwrap();
        tick.join().unwrap();
        cancel.join().unwrap();
        manager.stop().unwrap();
        assert_eq!(
            manager.store.lock().unwrap().runs[0].status,
            RunStatus::Cancelled
        );
    }
    #[test]
    fn graceful_shutdown_refuses_a_run_crossing_admission() {
        let (_root, manager, server, _id) = fixture();
        let (hook, admitted, release) = gate();
        *manager.after_admit.lock().unwrap() = Some(hook);
        let (hook, executing, finish) = gate();
        *manager.before_execute.lock().unwrap() = Some(hook);
        let tick = {
            let manager = manager.clone();
            let server = server.clone();
            thread::spawn(move || manager.tick(&server).unwrap())
        };
        admitted.recv_timeout(Duration::from_secs(2)).unwrap();
        let (tx, rx) = mpsc::channel();
        let shutdown = thread::spawn(move || tx.send(server.request_shutdown(false)).unwrap());
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release.send(()).unwrap();
        executing.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Response::Error {
                code: ErrorCode::SessionsRemain,
                ..
            }
        ));
        assert!(!manager.stopping.load(Ordering::Acquire));
        finish.send(()).unwrap();
        tick.join().unwrap();
        shutdown.join().unwrap();
        manager.stop().unwrap();
    }
    #[test]
    fn cleanup_reports_retained_directory_without_registry_ownership() {
        let (root, manager, server, id) = fixture();
        let cwd = root.path().join("retained");
        fs::create_dir(&cwd).unwrap();
        manager
            .change(|store| {
                let run = store.runs.iter_mut().find(|run| run.id == id).unwrap();
                run.spec.target = TaskTarget::Git {
                    project: "fixture".into(),
                    remote: "origin".into(),
                    branch: "main".into(),
                };
                run.status = RunStatus::Failed;
                run.directory = Some(cwd.clone());
                run.workspace = Some("retained".into());
                Ok(())
            })
            .unwrap();
        let error = manager
            .handle(
                &server,
                TaskRequest::Clean {
                    id,
                    confirmed: true,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("not registered"));
        assert!(cwd.exists());
    }
    #[test]
    fn workspace_removal_protects_admitted_tasks_before_run_metadata_is_written() {
        let (root, manager, server, id) = fixture();
        let repo = root.path().join("repo");
        let workspaces = root.path().join("workspaces");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&workspaces).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
            vec!["commit", "--allow-empty", "-m", "base"],
        ] {
            let out = Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let repo = repo.canonicalize().unwrap();
        let workspaces = workspaces.canonicalize().unwrap();
        // This fixture exercises task admission, not interactive root-shell setup.
        // Seed the legitimate state after a root shell has been closed and deleted.
        server
            .registry
            .lock()
            .unwrap()
            .add_project(crate::config::ProjectRecord {
                name: "fixture".into(),
                repo: repo.clone(),
                workspace_root: workspaces.clone(),
                workspaces: vec![crate::config::WorkspaceRecord {
                    id: "root".into(),
                    path: repo.clone(),
                    branch: "main".into(),
                    git_identity: Some(crate::git::capture_worktree_identity(&repo).unwrap()),
                    setup_pending: false,
                }],
            })
            .unwrap();
        server
            .create_task_workspace(
                "fixture".into(),
                "task-1-run-1".into(),
                "ovrcr/task-1/run-1".into(),
                "main".into(),
            )
            .unwrap();
        assert!(!run_dir(&manager.directory, id).join("run.json").exists());
        for status in [
            RunStatus::Preparing,
            RunStatus::Running,
            RunStatus::Cancelling,
        ] {
            manager
                .change(|store| {
                    let run = &mut store.runs[0];
                    run.spec.target = TaskTarget::Git {
                        project: "fixture".into(),
                        remote: "origin".into(),
                        branch: "main".into(),
                    };
                    run.status = status;
                    Ok(())
                })
                .unwrap();
            assert!(
                server.remove_workspace("fixture", "task-1-run-1").is_err(),
                "removed an admitted task's intended worktree"
            );
            assert!(workspaces.join("task-1-run-1").is_dir());
            assert!(
                server
                    .registry
                    .lock()
                    .unwrap()
                    .workspace("fixture", "task-1-run-1")
                    .is_ok()
            );
        }
        manager
            .change(|store| {
                store.runs[0].status = RunStatus::Failed;
                Ok(())
            })
            .unwrap();
        server.remove_workspace("fixture", "task-1-run-1").unwrap();
        assert!(!workspaces.join("task-1-run-1").exists());
    }
}
