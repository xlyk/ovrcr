use std::{
    ffi::OsString,
    process::{Command, Stdio},
};

#[test]
fn codex_hook_outside_managed_invocation_is_silent_without_reading_stdin() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
    command
        .args(["report", "codex", "--stdin"])
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
        .env("OVRCR_HOOK_SOCKET", "/not/a/live/socket")
        .env("OVRCR_SESSION_ID", "1")
        .env("OVRCR_HOOK_TOKEN", "legacy-must-not-be-used")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let _held_stdin = child.stdin.take().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("unmanaged reporter waited on stdin");
        }
        std::thread::yield_now();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn codex_fresh_grammar_rejects_resume_remote_and_unknown_forms() {
    let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
    for values in [
        vec!["codex"],
        vec!["codex", "--no-alt-screen", "-C", "/tmp", "hello"],
        vec!["codex", "--", "resume"],
    ] {
        assert!(
            ovrcr::report::codex::eligible_argv(&args(&values)),
            "{values:?}"
        );
    }
    for values in [
        vec!["codex", "resume"],
        vec!["codex", "fork"],
        vec!["codex", "exec"],
        vec!["codex", "cloud"],
        vec!["codex", "--remote"],
        vec!["codex", "--model"],
        vec!["codex", "--help"],
        vec!["codex", "a", "b"],
        vec!["other"],
        vec!["codex", "--"],
    ] {
        assert!(
            !ovrcr::report::codex::eligible_argv(&args(&values)),
            "{values:?}"
        );
    }
}

#[test]
fn codex_native_argv_stdout_and_exit_survive_missing_reporting() {
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args([
            "agent",
            "run",
            "codex",
            "--",
            "/bin/sh",
            "-c",
            "printf 'NATIVE:%s:%s\\n' \"$1\" \"$2\"; exit 19",
            "fixture",
            "a b",
            "--resume",
        ])
        .env_remove("OVRCR_HOOK_SOCKET")
        .env_remove("OVRCR_SESSION_ID")
        .env_remove("OVRCR_HOOK_TOKEN")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(19));
    assert_eq!(output.stdout, b"NATIVE:a b:--resume\n");
}

#[test]
fn codex_version_probe_accepts_only_exact_successful_pinned_output() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("codex");
    for (output, code, accepted) in [
        ("codex-cli 0.153.0", 0, true),
        ("codex-cli 0.153.1", 0, false),
        ("codex-cli 0.153.0", 1, false),
        ("unrecognized", 0, false),
    ] {
        std::fs::write(
            &executable,
            format!("#!/bin/sh\nprintf '{output}\\n'\nexit {code}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            ovrcr::report::codex::supported_version(executable.as_os_str()),
            accepted
        );
    }
}
