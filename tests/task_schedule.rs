use std::path::PathBuf;

use ovrcr::tasks::{
    Run, RunId, RunStatus, RunTrigger, Schedule, TaskId, TaskSpec, TaskStore, TaskTarget, read_run,
    run_dir, tasks_dir, write_run,
};

fn spec(name: &str, schedule: Schedule) -> TaskSpec {
    TaskSpec {
        name: name.into(),
        prompt: format!("run {name}"),
        target: TaskTarget::Scratch,
        schedule,
        model: "anthropic/claude-sonnet-4".into(),
        thinking: "off".into(),
        timeout_seconds: 3_600,
    }
}

fn run(id: u64, task_id: TaskId, status: RunStatus, created_at: i64) -> Run {
    Run {
        id: RunId(id),
        task_id,
        spec: spec("snapshot", Schedule::Interval { seconds: 60 }),
        trigger: RunTrigger::Manual,
        scheduled_at: created_at,
        created_at,
        started_at: None,
        finished_at: None,
        status,
        directory: None,
        workspace: None,
        base_commit: None,
        pi_version: None,
        session_file: None,
        error: None,
    }
}

#[test]
fn schedules_validate_and_find_the_next_zoned_occurrence() {
    let cron = Schedule::Cron {
        expression: "0 9 * * *".into(),
        timezone: "America/New_York".into(),
    };
    cron.validate().unwrap();
    assert_eq!(cron.next_after(1_704_115_800).unwrap(), Some(1_704_117_600));
    assert_eq!(
        Schedule::Interval { seconds: 30 }.next_after(100).unwrap(),
        Some(130)
    );
    assert_eq!(
        Schedule::Once { at: 101 }.next_after(100).unwrap(),
        Some(101)
    );
    assert_eq!(Schedule::Once { at: 100 }.next_after(100).unwrap(), None);
    assert!(Schedule::Interval { seconds: 0 }.validate().is_err());
    assert!(
        Schedule::Cron {
            expression: "0 0 * *".into(),
            timezone: "UTC".into()
        }
        .validate()
        .is_err()
    );
    assert!(
        Schedule::Cron {
            expression: "0 0 * * *".into(),
            timezone: "Mars/Olympus".into()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn task_specs_reject_missing_execution_inputs() {
    let mut candidate = spec("nightly", Schedule::Interval { seconds: 60 });
    candidate.prompt.clear();
    assert!(candidate.validate().is_err());
    candidate = spec("nightly", Schedule::Interval { seconds: 60 });
    candidate.timeout_seconds = 0;
    assert!(candidate.validate().is_err());
    candidate = spec("nightly", Schedule::Interval { seconds: 60 });
    candidate.target = TaskTarget::Git {
        project: "repo".into(),
        remote: String::new(),
        branch: "main".into(),
    };
    assert!(candidate.validate().is_err());
    for model in ["claude", "/claude", "anthropic/"] {
        candidate = spec("nightly", Schedule::Interval { seconds: 60 });
        candidate.model = model.into();
        assert!(
            candidate.validate().is_err(),
            "accepted invalid model {model:?}"
        );
    }
    candidate = spec("nightly", Schedule::Interval { seconds: 60 });
    candidate.thinking = "extreme".into();
    assert!(candidate.validate().is_err());
    for (remote, branch) in [("-origin", "main"), ("origin", "-main")] {
        candidate = spec("nightly", Schedule::Interval { seconds: 60 });
        candidate.target = TaskTarget::Git {
            project: "repo".into(),
            remote: remote.into(),
            branch: branch.into(),
        };
        assert!(candidate.validate().is_err());
    }
}

#[test]
fn create_and_update_reject_past_one_time_schedules_but_resume_expires_them() {
    let mut store = TaskStore::default();
    assert!(
        store
            .create(spec("old", Schedule::Once { at: 99 }), 100)
            .is_err()
    );
    let task = store
        .create(spec("future", Schedule::Once { at: 200 }), 100)
        .unwrap();
    assert!(
        store
            .update(task.id, spec("old", Schedule::Once { at: 100 }), 100)
            .is_err()
    );
    store.pause(task.id, 150).unwrap();
    store.resume(task.id, 250).unwrap();
    assert!(store.tasks[0].enabled);
    assert_eq!(store.tasks[0].next_due_at, None);
}

#[test]
fn create_update_pause_and_resume_use_the_injected_clock() {
    let mut store = TaskStore::default();
    let created = store
        .create(spec("hourly", Schedule::Interval { seconds: 60 }), 1_000)
        .unwrap();
    assert_eq!(created.id, TaskId(1));
    assert_eq!(created.next_due_at, Some(1_060));
    let renamed = store
        .update(
            created.id,
            spec("renamed", Schedule::Interval { seconds: 60 }),
            1_010,
        )
        .unwrap();
    assert_eq!(
        renamed.next_due_at,
        Some(1_060),
        "non-schedule edits preserve interval cadence"
    );
    let updated = store
        .update(
            created.id,
            spec("daily", Schedule::Interval { seconds: 300 }),
            1_010,
        )
        .unwrap();
    assert_eq!(updated.next_due_at, Some(1_310));
    store.pause(created.id, 1_020).unwrap();
    assert!(!store.tasks[0].enabled);
    assert_eq!(store.tasks[0].next_due_at, None);
    store.resume(created.id, 1_030).unwrap();
    assert!(store.tasks[0].enabled);
    assert_eq!(store.tasks[0].next_due_at, Some(1_330));
}

#[test]
fn advancing_a_late_interval_enqueues_once_and_preserves_its_anchor() {
    let mut store = TaskStore::default();
    let task = store
        .create(spec("poll", Schedule::Interval { seconds: 60 }), 100)
        .unwrap();
    assert!(store.advance_due(350).unwrap());
    assert_eq!(store.runs.len(), 1);
    assert_eq!(store.runs[0].scheduled_at, 160);
    assert_eq!(store.runs[0].created_at, 350);
    assert_eq!(store.runs[0].trigger, RunTrigger::Scheduled);
    assert_eq!(store.tasks[0].next_due_at, Some(400));
    assert!(!store.advance_due(350).unwrap());
    assert!(store.advance_due(400).unwrap());
    assert_eq!(store.tasks[0].next_due_at, Some(460));
    assert_eq!(store.runs.len(), 1, "queued occurrences coalesce");
    assert_eq!(store.runs[0].task_id, task.id);
}

#[test]
fn manual_enqueue_coalesces_without_changing_the_schedule() {
    let mut store = TaskStore::default();
    let task = store
        .create(spec("poll", Schedule::Interval { seconds: 60 }), 100)
        .unwrap();
    let first = store.enqueue(task.id, RunTrigger::Manual, 110).unwrap();
    let second = store.enqueue(task.id, RunTrigger::Scheduled, 120).unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(store.runs.len(), 1);
    assert_eq!(store.tasks[0].next_due_at, Some(160));
    assert_eq!(store.runs[0].spec.name, "poll");
}

#[test]
fn admission_respects_capacity_fifo_and_one_active_run_per_task() {
    let mut store = TaskStore::default();
    store.max_concurrent = 2;
    let first = store
        .create(spec("first", Schedule::Interval { seconds: 60 }), 0)
        .unwrap();
    let second = store
        .create(spec("second", Schedule::Interval { seconds: 60 }), 0)
        .unwrap();
    let third = store
        .create(spec("third", Schedule::Interval { seconds: 60 }), 0)
        .unwrap();
    store.runs.push(run(10, first.id, RunStatus::Running, 1));
    store.runs.push(run(11, first.id, RunStatus::Queued, 2));
    store.runs.push(run(12, second.id, RunStatus::Queued, 3));
    store.runs.push(run(13, third.id, RunStatus::Queued, 4));
    let admitted = store.admit(10);
    assert_eq!(
        admitted.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![RunId(12)]
    );
    assert_eq!(store.runs[2].status, RunStatus::Preparing);
    assert_eq!(store.runs[1].status, RunStatus::Queued);
    assert_eq!(store.runs[3].status, RunStatus::Queued);
}

#[test]
fn pause_cancels_only_scheduled_pending_work() {
    let mut store = TaskStore::default();
    let task = store
        .create(spec("poll", Schedule::Interval { seconds: 60 }), 0)
        .unwrap();
    let mut scheduled = run(1, task.id, RunStatus::Queued, 10);
    scheduled.trigger = RunTrigger::Scheduled;
    store
        .runs
        .extend([scheduled, run(2, task.id, RunStatus::Queued, 11)]);
    store.pause(task.id, 20).unwrap();
    assert_eq!(store.runs[0].status, RunStatus::Cancelled);
    assert_eq!(store.runs[0].finished_at, Some(20));
    assert_eq!(store.runs[1].status, RunStatus::Queued);
}

#[test]
fn delete_refuses_active_then_cancels_pending_and_preserves_history() {
    let mut store = TaskStore::default();
    let task = store
        .create(spec("poll", Schedule::Interval { seconds: 60 }), 0)
        .unwrap();
    store.runs.push(run(1, task.id, RunStatus::Running, 1));
    assert!(store.delete(task.id, 20).is_err());
    assert_eq!(store.tasks.len(), 1);
    store.runs[0].status = RunStatus::Succeeded;
    store.runs.push(run(2, task.id, RunStatus::Queued, 2));
    store.delete(task.id, 30).unwrap();
    assert!(store.tasks.is_empty());
    assert_eq!(store.runs.len(), 2);
    assert_eq!(store.runs[1].status, RunStatus::Cancelled);
    assert_eq!(store.runs[1].finished_at, Some(30));
}

#[test]
fn store_and_run_metadata_round_trip_through_real_files() {
    let root = tempfile::tempdir().unwrap();
    let registry = root.path().join("config.toml");
    let state = tasks_dir(&registry).join("state.toml");
    assert_eq!(tasks_dir(&registry), root.path().join("config.tasks"));
    let mut store = TaskStore::default();
    let task = store
        .create(spec("poll", Schedule::Interval { seconds: 60 }), 100)
        .unwrap();
    let queued = store.enqueue(task.id, RunTrigger::Manual, 101).unwrap();
    store.save_atomic(&state).unwrap();
    let mut loaded = TaskStore::load(&state).unwrap();
    assert_eq!(loaded, store);
    assert_eq!(
        loaded
            .create(spec("next", Schedule::Once { at: 500 }), 200)
            .unwrap()
            .id,
        TaskId(2)
    );
    assert_eq!(
        loaded
            .enqueue(TaskId(2), RunTrigger::Manual, 201)
            .unwrap()
            .id,
        RunId(2)
    );
    let directory = run_dir(&tasks_dir(&registry), queued.id);
    write_run(&directory, &queued).unwrap();
    assert_eq!(read_run(&directory).unwrap(), queued);
    assert!(directory.join("run.json").is_file());
}

#[test]
fn run_status_classification_matches_queue_and_capacity_semantics() {
    assert!(!RunStatus::Queued.is_terminal());
    assert!(!RunStatus::Queued.occupies_slot());
    assert!(RunStatus::Preparing.occupies_slot());
    assert!(RunStatus::Running.occupies_slot());
    assert!(RunStatus::Cancelling.occupies_slot());
    assert!(RunStatus::Succeeded.is_terminal());
    assert!(RunStatus::CleanupFailed.is_terminal());
}

#[test]
fn helper_paths_have_the_documented_shape() {
    assert_eq!(
        run_dir(PathBuf::from("/tmp/ovrcr.tasks").as_path(), RunId(42)),
        PathBuf::from("/tmp/ovrcr.tasks/runs/42")
    );
}
