use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use ovrcr_protocol::{
    Run, RunId, RunStatus, RunTrigger, Schedule, Task, TaskId, TaskRequest, TaskResponse, TaskSpec,
    TaskTarget,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStore {
    pub tasks: Vec<Task>,
    pub runs: Vec<Run>,
    pub max_concurrent: usize,
    #[serde(default = "first_id")]
    next_task_id: u64,
    #[serde(default = "first_id")]
    next_run_id: u64,
}

const fn first_id() -> u64 {
    1
}
/// Runs kept per task regardless of age.
pub const RETAINED_RUNS_PER_TASK: usize = 200;
/// Runs younger than this are kept regardless of count.
pub const RUN_RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl Default for TaskStore {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            runs: Vec::new(),
            max_concurrent: 3,
            next_task_id: 1,
            next_run_id: 1,
        }
    }
}

fn reject_past_once(schedule: &Schedule, now: i64) -> Result<()> {
    if let Schedule::Once { at } = schedule
        && *at <= now
    {
        bail!("one-time task must be scheduled in the future");
    }
    Ok(())
}

impl TaskStore {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("read task store {}", path.display()));
            }
        };
        let mut store: Self = toml::from_str(&contents).context("parse task store")?;
        store.validate().context("validate task store")?;
        store.next_task_id = store.next_task_id.max(
            store
                .tasks
                .iter()
                .map(|task| task.id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        );
        store.next_run_id = store.next_run_id.max(
            store
                .runs
                .iter()
                .map(|run| run.id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        );
        Ok(store)
    }

    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        // Terminal run specs are frozen history validated at load; skip them on every save.
        self.check(false).context("validate task store")?;
        let contents = toml::to_string_pretty(self).context("serialize task store")?;
        write_atomic(path, contents.as_bytes(), "task store")
    }

    pub fn create(&mut self, spec: TaskSpec, now: i64) -> Result<Task> {
        spec.validate()?;
        reject_past_once(&spec.schedule, now)?;
        let id = TaskId(self.take_task_id()?);
        let task = Task {
            id,
            next_due_at: spec.schedule.next_after(now)?,
            spec,
            enabled: true,
        };
        self.tasks.push(task.clone());
        Ok(task)
    }

    pub fn update(&mut self, id: TaskId, spec: TaskSpec, now: i64) -> Result<Task> {
        spec.validate()?;
        reject_past_once(&spec.schedule, now)?;
        let current = self.task(id)?;
        let enabled = current.enabled;
        let next_due_at = if !enabled {
            None
        } else if current.spec.schedule == spec.schedule {
            current.next_due_at
        } else {
            spec.schedule.next_after(now)?
        };
        let task = self.task_mut(id)?;
        task.spec = spec;
        task.next_due_at = next_due_at;
        Ok(task.clone())
    }

    pub fn pause(&mut self, id: TaskId, now: i64) -> Result<()> {
        let task = self.task_mut(id)?;
        task.enabled = false;
        task.next_due_at = None;
        for run in &mut self.runs {
            if run.task_id == id
                && run.status == RunStatus::Queued
                && run.trigger == RunTrigger::Scheduled
            {
                run.status = RunStatus::Cancelled;
                run.finished_at = Some(now);
            }
        }
        Ok(())
    }

    pub fn resume(&mut self, id: TaskId, now: i64) -> Result<()> {
        let next_due_at = self.task(id)?.spec.schedule.next_after(now)?;
        let task = self.task_mut(id)?;
        task.enabled = true;
        task.next_due_at = next_due_at;
        Ok(())
    }

    pub fn delete(&mut self, id: TaskId, now: i64) -> Result<()> {
        let index = self
            .tasks
            .iter()
            .position(|task| task.id == id)
            .with_context(|| format!("task not found: {}", id.0))?;
        if self
            .runs
            .iter()
            .any(|run| run.task_id == id && run.status.occupies_slot())
        {
            bail!("cannot delete task {} while a run is active", id.0);
        }
        for run in &mut self.runs {
            if run.task_id == id && run.status == RunStatus::Queued {
                run.status = RunStatus::Cancelled;
                run.finished_at = Some(now);
            }
        }
        self.tasks.remove(index);
        Ok(())
    }

    pub fn enqueue(&mut self, id: TaskId, trigger: RunTrigger, now: i64) -> Result<Run> {
        let task = self.task(id)?.clone();
        self.enqueue_snapshot(&task, trigger, now, now)
    }

    pub fn advance_due(&mut self, now: i64) -> Result<bool> {
        let due = self
            .tasks
            .iter()
            .filter_map(|task| {
                task.enabled
                    .then_some(task.next_due_at)
                    .flatten()
                    .filter(|due| *due <= now)
                    .map(|due| (task.clone(), due))
            })
            .collect::<Vec<_>>();
        if due.is_empty() {
            return Ok(false);
        }

        for (task, scheduled_at) in due {
            self.enqueue_snapshot(&task, RunTrigger::Scheduled, scheduled_at, now)?;
            let next = match &task.spec.schedule {
                Schedule::Interval { seconds } => next_interval_after(scheduled_at, now, *seconds)?,
                Schedule::Cron { .. } => task.spec.schedule.next_after(now)?,
                Schedule::Once { .. } => None,
            };
            self.task_mut(task.id)?.next_due_at = next;
        }
        Ok(true)
    }

    pub fn admit(&mut self, now: i64) -> Vec<Run> {
        let mut slots = self.max_concurrent.saturating_sub(
            self.runs
                .iter()
                .filter(|run| run.status.occupies_slot())
                .count(),
        );
        let mut active_tasks = self
            .runs
            .iter()
            .filter(|run| run.status.occupies_slot())
            .map(|run| run.task_id)
            .collect::<HashSet<_>>();
        let mut queued = self
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.status == RunStatus::Queued)
            .map(|(index, run)| (index, run.created_at, run.id))
            .collect::<Vec<_>>();
        queued.sort_by_key(|(_, created_at, id)| (*created_at, *id));
        let mut admitted = Vec::new();
        for (index, _, _) in queued {
            if slots == 0 {
                break;
            }
            let task_id = self.runs[index].task_id;
            if !active_tasks.insert(task_id) {
                continue;
            }
            let run = &mut self.runs[index];
            run.status = RunStatus::Preparing;
            run.started_at = Some(now);
            admitted.push(run.clone());
            slots -= 1;
        }
        admitted
    }

    fn enqueue_snapshot(
        &mut self,
        task: &Task,
        trigger: RunTrigger,
        scheduled_at: i64,
        created_at: i64,
    ) -> Result<Run> {
        if let Some(run) = self
            .runs
            .iter()
            .find(|run| run.task_id == task.id && run.status == RunStatus::Queued)
        {
            return Ok(run.clone());
        }
        let id = RunId(self.take_run_id()?);
        let run = Run {
            id,
            task_id: task.id,
            spec: task.spec.clone(),
            trigger,
            scheduled_at,
            created_at,
            started_at: None,
            finished_at: None,
            status: RunStatus::Queued,
            directory: None,
            workspace: None,
            base_commit: None,
            pi_version: None,
            session_file: None,
            error: None,
        };
        self.runs.push(run.clone());
        Ok(run)
    }

    fn task(&self, id: TaskId) -> Result<&Task> {
        self.tasks
            .iter()
            .find(|task| task.id == id)
            .with_context(|| format!("task not found: {}", id.0))
    }

    fn task_mut(&mut self, id: TaskId) -> Result<&mut Task> {
        self.tasks
            .iter_mut()
            .find(|task| task.id == id)
            .with_context(|| format!("task not found: {}", id.0))
    }

    fn take_task_id(&mut self) -> Result<u64> {
        let id = self.next_task_id;
        self.next_task_id = id.checked_add(1).context("task ID space exhausted")?;
        Ok(id)
    }

    fn take_run_id(&mut self) -> Result<u64> {
        let id = self.next_run_id;
        self.next_run_id = id.checked_add(1).context("run ID space exhausted")?;
        Ok(id)
    }

    /// Drop terminal runs that are both older than the retention window and outside the
    /// newest `RETAINED_RUNS_PER_TASK` of their task. Returns the dropped run IDs.
    pub fn prune(&mut self, now: i64) -> Vec<RunId> {
        let mut by_task: HashMap<TaskId, Vec<(i64, RunId)>> = HashMap::new();
        for run in &self.runs {
            by_task
                .entry(run.task_id)
                .or_default()
                .push((run.finished_at.unwrap_or(run.created_at), run.id));
        }
        let mut expired = HashSet::new();
        for runs in by_task.values_mut() {
            runs.sort_unstable_by(|a, b| b.cmp(a));
            for (finished_at, id) in runs.iter().skip(RETAINED_RUNS_PER_TASK) {
                if now.saturating_sub(*finished_at) > RUN_RETENTION_SECONDS {
                    expired.insert(*id);
                }
            }
        }
        let mut pruned = Vec::new();
        self.runs.retain(|run| {
            let prune = run.status.is_terminal() && expired.contains(&run.id);
            if prune {
                pruned.push(run.id);
            }
            !prune
        });
        pruned
    }

    fn validate(&self) -> Result<()> {
        self.check(true)
    }

    fn check(&self, every_run_spec: bool) -> Result<()> {
        if self.max_concurrent == 0 {
            bail!("maximum concurrency must be greater than zero");
        }
        let mut task_ids = HashSet::new();
        for task in &self.tasks {
            if !task_ids.insert(task.id) {
                bail!("duplicate task ID: {}", task.id.0);
            }
            task.spec.validate()?;
        }
        let mut run_ids = HashSet::new();
        let mut queued_tasks = HashSet::new();
        for run in &self.runs {
            if !run_ids.insert(run.id) {
                bail!("duplicate run ID: {}", run.id.0);
            }
            if every_run_spec || !run.status.is_terminal() {
                run.spec.validate()?;
            }
            if run.status == RunStatus::Queued && !queued_tasks.insert(run.task_id) {
                bail!("multiple queued runs for task {}", run.task_id.0);
            }
        }
        Ok(())
    }
}

fn next_interval_after(previous_due: i64, now: i64, seconds: u64) -> Result<Option<i64>> {
    let step = i128::from(seconds);
    let previous = i128::from(previous_due);
    let now = i128::from(now);
    let jumps = (now - previous).div_euclid(step) + 1;
    let next = previous
        .checked_add(
            jumps
                .checked_mul(step)
                .context("next interval occurrence overflows timestamp")?,
        )
        .context("next interval occurrence overflows timestamp")?;
    Ok(Some(
        i64::try_from(next).context("next interval occurrence overflows timestamp")?,
    ))
}

pub fn tasks_dir(registry_path: &Path) -> PathBuf {
    registry_path.with_extension("tasks")
}

pub fn run_dir(tasks_dir: &Path, id: RunId) -> PathBuf {
    tasks_dir.join("runs").join(id.0.to_string())
}

#[cfg(test)]
thread_local! {
    pub(crate) static READ_RUN_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub fn read_run(run_dir: &Path) -> Result<Run> {
    #[cfg(test)]
    READ_RUN_CALLS.with(|calls| calls.set(calls.get() + 1));
    let path = run_dir.join("run.json");
    let contents =
        fs::read(&path).with_context(|| format!("read run metadata {}", path.display()))?;
    serde_json::from_slice(&contents)
        .with_context(|| format!("parse run metadata {}", path.display()))
}

pub fn write_run(run_dir: &Path, run: &Run) -> Result<()> {
    let contents = serde_json::to_vec_pretty(run).context("serialize run metadata")?;
    write_atomic(&run_dir.join("run.json"), &contents, "run metadata")
}

pub fn now() -> i64 {
    Utc::now().timestamp()
}

fn write_atomic(path: &Path, contents: &[u8], label: &str) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create {label} directory {}", parent.display()))?;
    let mut selected = None;
    for _ in 0..100 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".ovrcr-tasks-{}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                selected = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("create temporary {label} in {}", parent.display()));
            }
        }
    }
    let (temporary, mut file) = selected.context("choose temporary task file")?;
    let write_result = (|| -> Result<()> {
        file.write_all(contents)
            .with_context(|| format!("write temporary {label}"))?;
        file.flush()
            .with_context(|| format!("flush temporary {label}"))?;
        file.sync_all()
            .with_context(|| format!("sync temporary {label}"))
    })();
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("replace {label} {}", path.display()));
    }
    #[cfg(unix)]
    File::open(parent)
        .with_context(|| format!("open {label} directory {}", parent.display()))?
        .sync_all()
        .with_context(|| format!("sync {label} directory {}", parent.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_recent_runs_and_the_newest_per_task() {
        let mut store = TaskStore::default();
        let task = store
            .create(
                TaskSpec {
                    name: "history".into(),
                    prompt: "test".into(),
                    target: TaskTarget::Scratch,
                    schedule: Schedule::Interval { seconds: 60 },
                    model: "fixture/model".into(),
                    thinking: "off".into(),
                    timeout_seconds: 60,
                },
                0,
            )
            .unwrap();
        let old = RUN_RETENTION_SECONDS + 1;
        for index in 0..(RETAINED_RUNS_PER_TASK as i64 + 3) {
            let mut run = store.enqueue(task.id, RunTrigger::Manual, index).unwrap();
            run.status = RunStatus::Succeeded;
            run.finished_at = Some(index);
            let id = run.id;
            *store.runs.iter_mut().find(|r| r.id == id).unwrap() = run;
        }
        // The oldest three fall outside the newest 200; expiry follows the clock and an
        // active run is never pruned.
        store.runs[1].status = RunStatus::Running;
        assert_eq!(store.prune(old + 1), vec![RunId(1)]);
        assert_eq!(store.prune(old + 2), vec![RunId(3)]);
        assert_eq!(store.prune(old + 3), vec![]);
        assert_eq!(store.runs.len(), RETAINED_RUNS_PER_TASK + 1);
        assert!(store.runs.iter().any(|run| run.id == RunId(2)));
    }
}
