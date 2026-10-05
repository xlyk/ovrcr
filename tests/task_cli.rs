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
            .env("OVRCR_HOME", self.0.path())
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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.run(&["shutdown", "--kill"]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.0.path().join("server.sock").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn offline_task_and_run_reads_do_not_start_a_server() {
    let f = Fixture::new();
    assert_eq!(f.json(&["task", "list"]), serde_json::json!([]));
    assert_eq!(f.json(&["run", "list"]), serde_json::json!([]));
    assert!(!f.0.path().join("server.sock").exists());
}

#[test]
fn task_crud_persists_and_manual_failure_retains_a_run_record() {
    let f = Fixture::new();
    let task = f.json(&[
        "task",
        "create",
        "daily",
        "--scratch",
        "--every",
        "1h",
        "--model",
        "fixture/model",
        "--prompt",
        "Read the directory",
    ]);
    let id = task["id"].as_u64().unwrap().to_string();
    assert_eq!(task["spec"]["timeout_seconds"], 3600);
    assert_eq!(f.json(&["task", "concurrency"])["max_concurrent"], 3);
    f.json(&["task", "concurrency", "2"]);
    f.json(&["task", "pause", &id]);
    assert_eq!(f.json(&["task", "get", &id])["enabled"], false);
    f.json(&["task", "update", &id, "--prompt", "New instructions"]);
    let run = f.json(&["task", "run", &id]);
    let run_id = run["id"].as_u64().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let run = f.json(&["run", "get", &run_id]);
        if run["status"] == "Failed" {
            assert!(run["error"].as_str().unwrap().contains("pi"));
            break;
        }
        assert!(Instant::now() < deadline, "run did not fail: {run}");
        std::thread::sleep(Duration::from_millis(20));
    }
    f.json(&["task", "delete", &id]);
    assert_eq!(f.json(&["task", "list"]), serde_json::json!([]));
    assert_eq!(
        f.json(&["run", "list"])[0]["spec"]["prompt"],
        "New instructions"
    );
    f.json(&["shutdown", "--kill"]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while f.0.path().join("server.sock").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(f.json(&["run", "get", &run_id])["status"], "Failed");
    assert!(!f.0.path().join("server.sock").exists());
}

#[test]
fn unicode_duration_returns_validation_error_without_starting_server() {
    let f = Fixture::new();
    let out = f.run(&[
        "task",
        "create",
        "bad",
        "--scratch",
        "--every",
        "é",
        "--model",
        "provider/model",
        "--prompt",
        "noop",
    ]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("duration"));
    assert!(!f.0.path().join("server.sock").exists());
}
