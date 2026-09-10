use ovrcr::report::collector::{CollectorController, CollectorSnapshot, CollectorSource};
use ovrcr_protocol::UsageCoverage;
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

fn row(id: &str, input: u64) -> String {
    format!(
        "{}\n",
        json!({"type":"assistant","sessionId":"root","isSidechain":false,
        "requestId":id,"message":{"id":id,"usage":{"input_tokens":input,
        "cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":1}}})
    )
}
fn launch(path: &Path) -> CollectorController {
    CollectorController::spawn(
        Path::new(env!("CARGO_BIN_EXE_ovrcr")),
        CollectorSource {
            path: path.into(),
            conversation: "root".into(),
        },
    )
    .unwrap()
}
fn next(controller: &mut CollectorController) -> CollectorSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(snapshot) = controller.advance().unwrap() {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "collector response deadline");
        std::thread::yield_now();
    }
}
fn caught_up(controller: &mut CollectorController) -> CollectorSnapshot {
    for _ in 0..512 {
        let snapshot = next(controller);
        if snapshot.caught_up || snapshot.diagnostic.is_some() && !snapshot.rebuilding {
            return snapshot;
        }
    }
    panic!("collector did not catch up");
}

#[test]
fn collector_actual_helper_appends_partial_record_and_equal_replay() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    let first = row("first", 10);
    let second = row("second", 20);
    fs::write(&path, format!("{first}{}", &second[..second.len() / 2])).unwrap();
    let mut controller = launch(&path);
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, Some(10));
    assert_eq!(sample.usage.coverage, UsageCoverage::Partial);
    assert_eq!(sample.source_revision, None);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    write!(file, "{}{first}", &second[second.len() / 2..]).unwrap();
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, Some(30));
    assert_eq!(sample.retained_identities, 2);
    controller
        .cancel(Instant::now() + Duration::from_secs(1))
        .unwrap();
}

#[test]
fn collector_differing_duplicate_freezes_before_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    fs::write(
        &path,
        format!("{}{}{}", row("same", 10), row("same", 20), row("later", 30)),
    )
    .unwrap();
    let mut controller = launch(&path);
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, Some(10));
    assert_eq!(
        sample.diagnostic.as_deref(),
        Some("conflicting_usage_record")
    );
    assert_eq!(sample.retained_identities, 1);
    assert_eq!(caught_up(&mut controller).usage.input_tokens, Some(10));
}

#[test]
fn collector_replacement_does_not_publish_a_smaller_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    fs::write(&path, row("first", 10)).unwrap();
    let mut controller = launch(&path);
    assert_eq!(caught_up(&mut controller).usage.input_tokens, Some(10));
    let replacement = temp.path().join("replacement");
    fs::write(&replacement, row("other", 5)).unwrap();
    fs::rename(&replacement, &path).unwrap();
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, Some(10));
    assert_eq!(sample.diagnostic.as_deref(), Some("usage_regression"));
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(row("later", 15).as_bytes())
        .unwrap();
    assert_eq!(caught_up(&mut controller).usage.input_tokens, Some(20));
}

#[test]
fn collector_foreign_usage_and_nonregular_source_fail_without_content_leaks() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("PRIVATE_PATH.jsonl");
    fs::write(
        &path,
        row("PRIVATE_BODY", 1).replace("\"root\"", "\"foreign\""),
    )
    .unwrap();
    let mut controller = launch(&path);
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, None);
    assert_eq!(sample.diagnostic.as_deref(), Some("foreign_usage_record"));
    let mut directory = launch(temp.path());
    let sample = caught_up(&mut directory);
    assert_eq!(sample.diagnostic.as_deref(), Some("source_unavailable"));
}

#[test]
fn collector_missing_file_retries_same_path_and_truncation_preserves_watermark() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    let mut controller = launch(&path);
    assert_eq!(
        next(&mut controller).diagnostic.as_deref(),
        Some("source_unavailable")
    );
    fs::write(&path, row("first", 10)).unwrap();
    assert_eq!(caught_up(&mut controller).usage.input_tokens, Some(10));
    fs::write(&path, "").unwrap();
    let rebuilding = next(&mut controller);
    assert!(rebuilding.rebuilding);
    assert_eq!(rebuilding.usage.input_tokens, Some(10));
    assert_eq!(
        caught_up(&mut controller).diagnostic.as_deref(),
        Some("usage_regression")
    );
    fs::write(&path, row("later", 30)).unwrap();
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, Some(30));
    assert_eq!(sample.diagnostic, None);
}

#[test]
fn collector_reads_only_one_chunk_and_excludes_sidechains() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    let metadata = format!(
        "{{\"type\":\"user\",\"body\":\"{}\"}}\n",
        "x".repeat(256 * 1024)
    );
    let child = row("child", 500).replace("\"isSidechain\":false", "\"isSidechain\":true");
    fs::write(
        &path,
        format!("{}{metadata}{child}{}", row("first", 10), row("second", 20)),
    )
    .unwrap();
    let mut controller = launch(&path);
    let first = next(&mut controller);
    assert!(!first.caught_up);
    assert_eq!(first.usage.input_tokens, Some(10));
    assert_eq!(first.retained_identities, 1);
    assert_eq!(caught_up(&mut controller).usage.input_tokens, Some(30));
}

fn assert_gone(pid: i32) {
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    assert_eq!(unsafe { libc::kill(-pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

fn stop_owned(pid: i32) {
    assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let mut status = 0;
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED | libc::WNOHANG) };
        if result == pid {
            assert!(libc::WIFSTOPPED(status));
            break;
        }
        assert_eq!(result, 0);
        assert!(Instant::now() < deadline, "owned helper did not stop");
        std::thread::yield_now();
    }
}

#[test]
fn collector_stalled_helper_is_nonblocking_and_owned_group_cleanup_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    fs::write(&path, row("first", 10)).unwrap();
    let mut controller = launch(&path);
    caught_up(&mut controller);
    let pid = controller.process_id().unwrap() as i32;
    assert_eq!(unsafe { libc::getpgid(pid) }, pid);
    stop_owned(pid);
    let start = Instant::now();
    assert!(controller.advance().unwrap().is_none());
    assert!(start.elapsed() < Duration::from_millis(100));
    controller
        .cancel(Instant::now() + Duration::from_millis(250))
        .unwrap();
    assert_gone(pid);
    println!("owned stopped helper PID/PGID {pid} killed and reaped");
}

#[test]
fn collector_drop_is_bounded_for_a_stalled_helper() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    fs::write(&path, "").unwrap();
    let mut controller = launch(&path);
    caught_up(&mut controller);
    let pid = controller.process_id().unwrap() as i32;
    stop_owned(pid);
    let start = Instant::now();
    drop(controller);
    assert!(start.elapsed() < Duration::from_millis(250));
    assert_gone(pid);
    println!("owned helper PID/PGID {pid} cleaned by bounded Drop");
}

#[test]
fn collector_raw_record_limit_preserves_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(row("first", 10).as_bytes()).unwrap();
    file.write_all(b"{\"type\":\"user\",\"body\":\"").unwrap();
    for _ in 0..129 {
        file.write_all(&vec![b'x'; 256 * 1024]).unwrap();
    }
    file.write_all(b"\"}\n").unwrap();
    let mut controller = launch(&path);
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, Some(10));
    assert_eq!(sample.diagnostic.as_deref(), Some("record_limit"));
    assert_eq!(sample.retained_identities, 1);
}

#[test]
fn collector_production_identity_cap_freezes_later_unique_usage() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    for index in 0..65_537 {
        file.write_all(row(&index.to_string(), 1).as_bytes())
            .unwrap();
    }
    let mut controller = launch(&path);
    let sample = caught_up(&mut controller);
    assert_eq!(sample.usage.input_tokens, Some(65_536));
    assert_eq!(sample.diagnostic.as_deref(), Some("accounting_limit"));
    assert_eq!(sample.retained_identities, 65_536);
    assert!(sample.retained_bytes <= 16 * 1024 * 1024);
}

#[test]
fn collector_production_byte_cap_freezes_before_identity_cap() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    for index in 0..30_000 {
        let key = format!("{index:0256}");
        file.write_all(row(&key, 1).as_bytes()).unwrap();
    }
    let mut controller = launch(&path);
    let sample = caught_up(&mut controller);
    assert_eq!(sample.diagnostic.as_deref(), Some("accounting_limit"));
    assert!(sample.retained_identities < 30_000);
    assert!(sample.retained_bytes <= 16 * 1024 * 1024);
    assert_eq!(
        sample.usage.input_tokens,
        Some(sample.retained_identities as u64)
    );
}

#[test]
fn collector_rejects_oversized_configuration_before_spawn() {
    let result = CollectorController::spawn(
        Path::new("/missing/executable"),
        CollectorSource {
            path: format!("/{}", "x".repeat(65_536)).into(),
            conversation: "root".into(),
        },
    );
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("message exceeds limit")
    );
}

#[test]
fn collector_launch_clears_inherited_environment() {
    use std::os::unix::fs::PermissionsExt;
    assert!(
        std::env::var_os("HOME").is_some(),
        "fixture needs an inherited HOME to detect leakage"
    );
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("environment-marker");
    let executable = temp.path().join("environment-probe");
    let quoted_marker = format!("'{}'", marker.to_str().unwrap().replace('\'', "'\"'\"'"));
    fs::write(&executable,format!("#!/bin/sh\nif [ -n \"${{HOME+x}}${{OVRCR_SESSION_TOKEN+x}}${{OVRCR_SESSION_ID+x}}${{OVRCR_INVOCATION_CHANNEL+x}}\" ]; then printf leaked > {quoted_marker}; else printf clean > {quoted_marker}; fi\nexec /bin/sleep 300\n")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut controller = CollectorController::spawn(
        &executable,
        CollectorSource {
            path: temp.path().join("unused"),
            conversation: "root".into(),
        },
    )
    .unwrap();
    let pid = controller.process_id().unwrap() as i32;
    let deadline = Instant::now() + Duration::from_secs(2);
    let value = loop {
        if let Ok(value) = fs::read_to_string(&marker)
            && !value.is_empty()
        {
            break value;
        }
        assert!(Instant::now() < deadline, "environment probe deadline");
        std::thread::yield_now();
    };
    controller
        .cancel(Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert_gone(pid);
    assert_eq!(value, "clean");
    println!("owned environment probe PID/PGID {pid} cleaned");
}
