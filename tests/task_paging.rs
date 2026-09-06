use std::process::{Command, Output};
use std::time::{Duration, Instant};
struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(args)
            .env("OVRCR_CONFIG", self.0.path().join("config.toml"))
            .env("OVRCR_SOCKET", self.0.path().join("server.sock"))
            .env("OVRCR_PI_EXECUTABLE", "/definitely/missing/pi")
            .output()
            .unwrap()
    }
    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let out = self.run(&all);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn stop(&self) {
        let _ = self.run(&["shutdown", "--kill"]);
        let deadline = Instant::now() + Duration::from_secs(8);
        while self.0.path().join("server.sock").exists() {
            assert!(Instant::now() < deadline, "server did not stop");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}
#[test]
fn cli_lists_all_large_tasks_and_runs_online_and_offline() {
    let f = Fixture::new();
    let prompt = "x".repeat(190 * 1024);
    let path = f.0.path().join("prompt.txt");
    std::fs::write(&path, &prompt).unwrap();
    // Eleven rows require three pages, exercising decreasing run cursors twice.
    for i in 0..11 {
        let task = f.json(&[
            "task",
            "create",
            &format!("large-{i}"),
            "--scratch",
            "--every",
            "1h",
            "--model",
            "fixture/model",
            "--prompt-file",
            path.to_str().unwrap(),
        ]);
        let id = task["id"].as_u64().unwrap().to_string();
        f.json(&["task", "run", &id]);
    }
    for online in [true, false] {
        if !online {
            f.stop();
        }
        let tasks = f.json(&["task", "list"]);
        let runs = f.json(&["run", "list"]);
        assert_eq!(tasks.as_array().unwrap().len(), 11);
        assert_eq!(runs.as_array().unwrap().len(), 11);
        for (i, task) in tasks.as_array().unwrap().iter().enumerate() {
            assert_eq!(task["id"], i + 1);
            assert_eq!(task["spec"]["prompt"], prompt);
        }
        for (i, run) in runs.as_array().unwrap().iter().enumerate() {
            assert_eq!(run["id"], 11 - i);
            assert_eq!(run["spec"]["prompt"], prompt);
        }
        let filtered = f.json(&["run", "list", "--task", "3"]);
        assert_eq!(filtered.as_array().unwrap().len(), 1);
        assert_eq!(filtered[0]["task_id"], 3);
        if !online {
            assert!(!f.0.path().join("server.sock").exists());
        }
    }
}

#[test]
fn serialized_pages_are_bounded_and_keep_task_order_and_filtered_run_order() {
    use ovrcr::task_manager::TaskResponse;
    use ovrcr::tasks::*;
    let mut store = TaskStore::default();
    for i in 0..9 {
        let task = store
            .create(
                TaskSpec {
                    name: format!("task-{i}"),
                    prompt: "界".repeat(70 * 1024),
                    target: TaskTarget::Scratch,
                    schedule: Schedule::Interval { seconds: 3600 },
                    model: "fixture/model".into(),
                    thinking: "off".into(),
                    timeout_seconds: 3600,
                },
                0,
            )
            .unwrap();
        store.enqueue(task.id, RunTrigger::Manual, 1).unwrap();
    }
    let mut offset = 0;
    let mut ids = Vec::new();
    let mut pages = 0;
    loop {
        let response = ovrcr::task_paging::tasks(&store.tasks, offset).unwrap();
        assert!(
            bincode::serde::encode_to_vec(&response, bincode::config::standard())
                .unwrap()
                .len()
                < ovrcr::protocol::MAX_FRAME_BYTES - 1024
        );
        let TaskResponse::TasksPage { items, next_offset } = response else {
            panic!()
        };
        ids.extend(items.iter().map(|t| t.id.0));
        pages += 1;
        match next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert!(pages > 1);
    assert_eq!(ids, (1..=9).collect::<Vec<_>>());
    let mut offset = 0;
    let mut ids = Vec::new();
    let mut pages = 0;
    loop {
        let response = ovrcr::task_paging::runs(&store.runs, None, offset).unwrap();
        assert!(
            bincode::serde::encode_to_vec(&response, bincode::config::standard())
                .unwrap()
                .len()
                < ovrcr::protocol::MAX_FRAME_BYTES - 1024
        );
        let TaskResponse::RunsPage { items, next_offset } = response else {
            panic!()
        };
        ids.extend(items.iter().map(|t| t.id.0));
        pages += 1;
        match next_offset {
            Some(next) => {
                assert!(offset == 0 || next < offset);
                offset = next;
            }
            None => break,
        }
    }
    assert!(pages > 1);
    assert_eq!(ids, (1..=9).rev().collect::<Vec<_>>());
    store
        .runs
        .iter_mut()
        .find(|run| run.task_id == TaskId(3))
        .unwrap()
        .status = RunStatus::Failed;
    store.enqueue(TaskId(3), RunTrigger::Manual, 2).unwrap();
    let TaskResponse::RunsPage { items, next_offset } =
        ovrcr::task_paging::runs(&store.runs, Some(TaskId(3)), 0).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        items.iter().map(|r| r.id.0).collect::<Vec<_>>(),
        vec![10, 3]
    );
    assert_eq!(next_offset, None);
    assert_eq!(
        ovrcr::task_paging::tasks(&store.tasks, usize::MAX).unwrap(),
        TaskResponse::TasksPage {
            items: vec![],
            next_offset: None
        }
    );
}
#[test]
fn oversized_legacy_row_returns_an_error_instead_of_a_stuck_empty_page() {
    use ovrcr::tasks::*;
    let task = Task {
        id: TaskId(1),
        spec: TaskSpec {
            name: "legacy".into(),
            prompt: "x".repeat(ovrcr::protocol::MAX_FRAME_BYTES),
            target: TaskTarget::Scratch,
            schedule: Schedule::Interval { seconds: 3600 },
            model: "fixture/model".into(),
            thinking: "off".into(),
            timeout_seconds: 3600,
        },
        enabled: true,
        next_due_at: None,
    };
    assert!(
        ovrcr::task_paging::tasks(&[task], 0)
            .unwrap_err()
            .to_string()
            .contains("too large")
    );
}

fn large_store() -> ovrcr::tasks::TaskStore {
    use ovrcr::tasks::*;
    let mut store = TaskStore::default();
    for i in 0..9 {
        let task = store
            .create(
                TaskSpec {
                    name: format!("task-{i}"),
                    prompt: "x".repeat(210 * 1024),
                    target: TaskTarget::Scratch,
                    schedule: Schedule::Interval { seconds: 3600 },
                    model: "fixture/model".into(),
                    thinking: "off".into(),
                    timeout_seconds: 3600,
                },
                0,
            )
            .unwrap();
        store.enqueue(task.id, RunTrigger::Manual, 1).unwrap();
    }
    store
}
#[test]
fn new_runs_between_pages_do_not_duplicate_or_omit_original_runs() {
    use ovrcr::task_manager::TaskResponse;
    use ovrcr::tasks::*;
    let mut store = large_store();
    let TaskResponse::RunsPage {
        items,
        mut next_offset,
    } = ovrcr::task_paging::runs(&store.runs, None, 0).unwrap()
    else {
        panic!()
    };
    let mut ids = items.iter().map(|run| run.id.0).collect::<Vec<_>>();
    assert!(next_offset.is_some());
    store.runs[0].status = RunStatus::Succeeded;
    store.enqueue(TaskId(1), RunTrigger::Manual, 2).unwrap();
    while let Some(cursor) = next_offset {
        let TaskResponse::RunsPage {
            items,
            next_offset: next,
        } = ovrcr::task_paging::runs(&store.runs, None, cursor).unwrap()
        else {
            panic!()
        };
        ids.extend(items.iter().map(|run| run.id.0));
        next_offset = next;
    }
    assert_eq!(ids, (1..=9).rev().collect::<Vec<_>>());
}
#[test]
fn deleting_an_earlier_task_between_pages_keeps_original_tasks_once() {
    use ovrcr::task_manager::TaskResponse;
    use ovrcr::tasks::TaskId;
    let mut store = large_store();
    let TaskResponse::TasksPage {
        items,
        mut next_offset,
    } = ovrcr::task_paging::tasks(&store.tasks, 0).unwrap()
    else {
        panic!()
    };
    let mut ids = items.iter().map(|task| task.id.0).collect::<Vec<_>>();
    assert!(next_offset.is_some());
    store.delete(TaskId(1), 2).unwrap();
    while let Some(cursor) = next_offset {
        let TaskResponse::TasksPage {
            items,
            next_offset: next,
        } = ovrcr::task_paging::tasks(&store.tasks, cursor).unwrap()
        else {
            panic!()
        };
        ids.extend(items.iter().map(|task| task.id.0));
        next_offset = next;
    }
    assert_eq!(ids, (1..=9).collect::<Vec<_>>());
}
