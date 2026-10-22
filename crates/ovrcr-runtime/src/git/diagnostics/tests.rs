use super::*;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;

fn shared(directory: &Path) -> Shared {
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    Arc::new(Mutex::new(
        Journal::open(directory, "git-owner-test").unwrap(),
    ))
}
fn rows(directory: &Path) -> Vec<Value> {
    fs::read_to_string(directory.join("git-owner.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}
fn scoped<T>(journal: Shared, f: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_JOURNAL.with(|slot| *slot.borrow_mut() = None);
        }
    }
    TEST_JOURNAL.with(|slot| *slot.borrow_mut() = Some(journal));
    let _reset = Reset;
    f()
}

#[test]
fn production_fast_git_receipts_and_missing_zombie_identity_fail_closed() {
    let temporary = tempfile::tempdir().unwrap();
    let journal = shared(temporary.path());
    // Original output succeeds but supplies no constructor receipt (baseline gap).
    let mut legacy = Command::new("/usr/bin/git");
    legacy.arg("--version");
    assert!(
        output_observed(&mut legacy, None, &|_| panic!("disabled inspection"))
            .unwrap()
            .status
            .success()
    );
    assert_eq!(locked(&journal).calls, 0);
    scoped(journal.clone(), || {
        for _ in 0..8 {
            let output =
                super::super::run_git(temporary.path(), &[OsString::from("--version")]).unwrap();
            assert!(output.stdout.starts_with(b"git version "));
        }
    });
    locked(&journal).finish();
    let evidence = rows(temporary.path());
    assert_eq!(evidence.iter().filter(|r| r["kind"] == "spawn").count(), 8);
    for row in evidence.iter().filter(|r| r["kind"] == "spawn") {
        assert_eq!(row["child_pid"], row["identity"]["pid"], "{row}");
        assert_eq!(row["identity"]["parent"], std::process::id());
        assert!(row["identity"]["group"].as_i64().unwrap() > 1);
        assert!(row["identity"]["birth"][0].as_u64().unwrap() > 0);
    }
    assert_eq!(
        evidence
            .iter()
            .filter(|r| r["kind"] == "wait"
                && r["original_wait"] == true
                && r["code"] == 0
                && r["signal"].is_null()
                && r["pipes_drained"] == true)
            .count(),
        8
    );
    assert_eq!(evidence.last().unwrap()["failed"], false);

    let zombie_directory = tempfile::tempdir().unwrap();
    let zombie_journal = shared(zombie_directory.path());
    let had_identity = std::cell::Cell::new(false);
    // Termination is observed without reaping; the SAME original Child still waits.
    let inspect = |pid| {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let status = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        assert_eq!(status, 0);
        let measured = crate::task_runner::diagnostic_process_info(pid);
        had_identity.set(measured.as_ref().is_ok_and(|row| row.is_some()));
        measured
    };
    let mut command = Command::new("/usr/bin/git");
    command.arg("--version");
    let actual = output_observed(&mut command, Some(zombie_journal.clone()), &inspect).unwrap();
    assert!(actual.status.success());
    assert!(actual.stdout.starts_with(b"git version "));
    locked(&zombie_journal).finish();
    let zombie = rows(zombie_directory.path());
    let spawn = zombie.iter().find(|r| r["kind"] == "spawn").unwrap();
    let wait = zombie.iter().find(|r| r["kind"] == "wait").unwrap();
    assert_eq!(wait["original_wait"], true);
    assert_eq!(wait["code"], 0);
    assert_eq!(wait["pipes_drained"], true);
    #[cfg(target_os = "macos")]
    assert!(
        !had_identity.get(),
        "Darwin's existing reader cannot inspect this zombie"
    );
    #[cfg(not(target_os = "macos"))]
    assert!(
        had_identity.get(),
        "the existing /proc stat reader retains this zombie"
    );
    assert_eq!(spawn["identity"].is_null(), !had_identity.get());
    assert_eq!(zombie.last().unwrap()["failed"], !had_identity.get());
}

#[test]
fn output_stdin_eof_nonzero_signal_status_and_private_content_are_preserved() {
    let temporary = tempfile::tempdir().unwrap();
    let journal = shared(temporary.path());
    let mut command = shell(
        "read -r ignored; printf ARG_SECRET_SENTINEL; printf STDERR_SECRET_SENTINEL >&2; exit 7",
    );
    command
        .env("AUTH_SECRET_SENTINEL", "ENV_SECRET_SENTINEL")
        .current_dir(temporary.path());
    let captured = output_observed(
        &mut command,
        Some(journal.clone()),
        &crate::task_runner::diagnostic_process_info,
    )
    .unwrap();
    assert_eq!(captured.stdout, b"ARG_SECRET_SENTINEL");
    assert_eq!(captured.stderr, b"STDERR_SECRET_SENTINEL");
    assert_eq!(captured.status.code(), Some(7));
    scoped(journal.clone(), || {
        assert_eq!(status(&mut shell("exit 9")).unwrap().code(), Some(9));
        let signaled = output(&mut shell("kill -TERM $$")).unwrap();
        assert_eq!(signaled.status.code(), None);
        assert_eq!(signaled.status.signal(), Some(libc::SIGTERM));
    });
    locked(&journal).finish();
    let raw = fs::read_to_string(temporary.path().join("git-owner.jsonl")).unwrap();
    for secret in [
        "ARG_SECRET",
        "STDERR_SECRET",
        "AUTH_SECRET",
        "ENV_SECRET",
        temporary.path().to_str().unwrap(),
    ] {
        assert!(!raw.contains(secret));
    }
    let evidence = rows(temporary.path());
    assert!(
        evidence
            .iter()
            .any(|r| r["kind"] == "wait" && r["code"] == 7 && r["pipes_drained"] == true)
    );
    assert!(
        evidence
            .iter()
            .any(|r| r["kind"] == "wait" && r["code"] == 9 && r["pipes_drained"] == false)
    );
    assert!(
        evidence
            .iter()
            .any(|r| r["kind"] == "wait" && r["code"].is_null() && r["signal"] == libc::SIGTERM)
    );
    assert_eq!(evidence.last().unwrap()["failed"], false);
}

#[test]
fn missing_identity_and_failed_receipts_still_drain_and_wait_original_child() {
    for missing_identity in [true, false] {
        let temporary = tempfile::tempdir().unwrap();
        let journal = shared(temporary.path());
        if !missing_identity {
            locked(&journal).bytes = MAX_BYTES - SEAL_RESERVE;
        }
        let inspect = |pid| {
            if missing_identity {
                Ok(None)
            } else {
                crate::task_runner::diagnostic_process_info(pid)
            }
        };
        let output = output_observed(
            &mut shell("printf original-output; exit 7"),
            Some(journal.clone()),
            &inspect,
        )
        .unwrap();
        assert_eq!(output.stdout, b"original-output");
        assert_eq!(output.status.code(), Some(7));
        locked(&journal).finish();
        let evidence = rows(temporary.path());
        assert_eq!(evidence.last().unwrap()["kind"], "seal");
        assert_eq!(evidence.last().unwrap()["failed"], true);
        if missing_identity {
            assert!(
                evidence
                    .iter()
                    .any(|r| r["kind"] == "spawn" && r["identity"].is_null())
            );
            assert!(
                evidence
                    .iter()
                    .any(|r| r["kind"] == "wait" && r["original_wait"] == true && r["code"] == 7)
            );
        }
        assert_eq!(locked(&journal).pending, 0);
    }
}

#[test]
fn spawn_and_wait_failure_never_create_success_or_expose_error_body() {
    let temporary = tempfile::tempdir().unwrap();
    let journal = shared(temporary.path());
    let mut command = Command::new(temporary.path().join("PRIVATE_MISSING_PROGRAM"));
    let error = output_observed(
        &mut command,
        Some(journal.clone()),
        &crate::task_runner::diagnostic_process_info,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    // Classification-only injection: not represented as an actual OS wait proof.
    let mut state = locked(&journal);
    let id = state.begin().unwrap();
    state.completed(id, Err(&io::Error::other("PRIVATE_WAIT_BODY")), false);
    state.finish();
    drop(state);
    let evidence = rows(temporary.path());
    assert!(evidence.iter().any(|r| r["kind"] == "spawn_error"));
    assert!(evidence.iter().any(|r| r["kind"] == "wait"
        && r["original_wait"] == false
        && r["pipes_drained"] == false));
    assert_eq!(evidence.last().unwrap()["failed"], true);
    let raw = fs::read_to_string(temporary.path().join("git-owner.jsonl")).unwrap();
    assert!(!raw.contains("PRIVATE_MISSING_PROGRAM") && !raw.contains("PRIVATE_WAIT_BODY"));
}

#[test]
fn private_file_noclobber_and_invalid_scope_remain_unqualified() {
    let temporary = tempfile::tempdir().unwrap();
    let journal = shared(temporary.path());
    locked(&journal).finish();
    let path = temporary.path().join("git-owner.jsonl");
    let before = fs::read(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(Journal::open(temporary.path(), "second").is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let other = tempfile::tempdir().unwrap();
    fs::set_permissions(other.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Journal::open(other.path(), "test").is_err());
    fs::set_permissions(other.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(Journal::open(other.path(), "../not-a-run").is_err());
    assert!(!other.path().join("git-owner.jsonl").exists());
}

#[test]
fn bounded_calls_rows_pending_and_reserved_unhealthy_seal() {
    for case in 0..3 {
        let temporary = tempfile::tempdir().unwrap();
        let journal = shared(temporary.path());
        let mut state = locked(&journal);
        match case {
            0 => {
                state.calls = MAX_CALLS;
                assert_eq!(state.begin(), None);
            }
            1 => {
                state.begin();
            }
            _ => state.record(
                json!({"kind":"invalid", "value":"x".repeat(MAX_ROW)}),
                false,
            ),
        }
        state.finish();
        drop(state);
        let evidence = rows(temporary.path());
        assert_eq!(evidence.last().unwrap()["failed"], true);
        assert!(
            fs::metadata(temporary.path().join("git-owner.jsonl"))
                .unwrap()
                .len()
                <= MAX_BYTES as u64
        );
    }
    // Synthetic worst-shaped valid field budget, not an OS process/wait claim.
    let common = json!({"schema":1,"seq":u64::MAX,"construction":u64::MAX});
    let mut begin = common.clone();
    begin["kind"] = json!("begin");
    let mut spawn = common.clone();
    spawn["kind"] = json!("spawn");
    spawn["child_pid"] = json!(u32::MAX);
    spawn["identity"] =
        json!({"pid":i32::MAX,"parent":i32::MAX,"group":i32::MAX,"birth":[u64::MAX,999999]});
    let mut wait = common;
    wait["kind"] = json!("wait");
    wait["original_wait"] = json!(true);
    wait["code"] = json!(i32::MIN);
    wait["signal"] = Value::Null;
    wait["pipes_drained"] = json!(true);
    let bytes: usize = [&begin, &spawn, &wait]
        .iter()
        .map(|row| serde_json::to_vec(row).unwrap().len() + 1)
        .sum();
    assert!(bytes * MAX_CALLS as usize + MAX_ROW + SEAL_RESERVE <= MAX_BYTES);
}

#[test]
fn terminal_sync_failure_cannot_qualify_healthy_seal_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let journal = shared(temporary.path());
    locked(&journal).fail_terminal_sync = true;
    scoped(journal.clone(), || {
        let result = output(&mut shell("printf original-result; exit 7")).unwrap();
        assert_eq!(result.stdout, b"original-result");
        assert_eq!(result.status.code(), Some(7));
        assert!(super::super::finish_diagnostics().is_err());
    });
    let evidence = rows(temporary.path());
    assert_eq!(evidence.last().unwrap()["kind"], "seal");
    // Full healthy-looking bytes are present, but original terminal status MUST fail.
    assert_eq!(evidence.last().unwrap()["failed"], false);
    {
        let state = locked(&journal);
        assert!(state.failed && state.write_failed);
    }
    assert_eq!(locked(&journal).pending, 0);
}

#[test]
fn terminal_health_rejects_pending_unavailable_late_and_duplicate_finish() {
    let temporary = tempfile::tempdir().unwrap();
    let journal = shared(temporary.path());
    scoped(journal.clone(), || {
        assert!(super::super::finish_diagnostics().is_ok());
        assert!(super::super::finish_diagnostics().is_err());
    });
    let pending_directory = tempfile::tempdir().unwrap();
    let pending = shared(pending_directory.path());
    assert!(locked(&pending).begin().is_some());
    scoped(pending, || {
        assert!(super::super::finish_diagnostics().is_err())
    });
    let unavailable = Arc::new(Mutex::new(Journal::unavailable()));
    scoped(unavailable, || {
        assert!(super::super::finish_diagnostics().is_err())
    });
    let late_directory = tempfile::tempdir().unwrap();
    let late = shared(late_directory.path());
    assert!(locked(&late).finish());
    // Gap persistence also fails: the prior healthy seal remains on disk.
    locked(&late).write_failed = true;
    assert_eq!(locked(&late).begin(), None);
    scoped(late, || {
        assert!(super::super::finish_diagnostics().is_err())
    });
    assert_eq!(rows(late_directory.path()).last().unwrap()["failed"], false);
}
