use std::{
    ffi::OsString,
    process::{Command, Stdio},
};

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn pi_interactive_grammar_rejects_headless_help_version_and_package_modes() {
    for values in [
        vec!["pi"],
        vec!["pi", "--model", "anthropic/claude-sonnet-4", "hello"],
        vec!["pi", "-e", "/tmp/user-extension.ts", "--resume"],
        vec!["pi", "--session", "abc123", "--thinking", "high"],
        vec!["/usr/local/bin/pi", "--", "install this"],
    ] {
        assert!(
            ovrcr::report::pi::eligible_argv(&args(&values)),
            "{values:?}"
        );
    }
    for values in [
        vec!["pi", "--mode", "rpc"],
        vec!["pi", "--mode", "json"],
        vec!["pi", "-p", "hello"],
        vec!["pi", "--print", "hello"],
        vec!["pi", "--help"],
        vec!["pi", "-h"],
        vec!["pi", "--version"],
        vec!["pi", "-v"],
        vec!["pi", "--export", "out.html"],
        vec!["pi", "--list-models"],
        vec!["pi", "install", "some-package"],
        vec!["pi", "auth", "print-api-key"],
        vec!["other"],
    ] {
        assert!(
            !ovrcr::report::pi::eligible_argv(&args(&values)),
            "{values:?}"
        );
    }
}

#[test]
fn omp_interactive_grammar_rejects_headless_help_and_version_modes() {
    for values in [
        vec!["omp"],
        vec!["omp", "--model", "x", "hello"],
        vec!["omp", "--yolo"],
    ] {
        assert!(
            ovrcr::report::omp::eligible_argv(&args(&values)),
            "{values:?}"
        );
    }
    for values in [
        vec!["omp", "--mode", "rpc"],
        vec!["omp", "--mode", "acp"],
        vec!["omp", "--mode", "rpc-ui"],
        vec!["omp", "-p"],
        vec!["omp", "--print"],
        vec!["omp", "--help"],
        vec!["omp", "--version"],
        vec!["pi"],
    ] {
        assert!(
            !ovrcr::report::omp::eligible_argv(&args(&values)),
            "{values:?}"
        );
    }
}

#[test]
fn pi_and_omp_native_argv_stdout_and_exit_survive_missing_reporting() {
    for provider in ["pi", "omp"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args([
                "agent",
                "run",
                provider,
                "--",
                "/bin/sh",
                "-c",
                "printf '%s|%s' \"$0\" \"$1\"; exit 23",
                "native-zero",
                "kept arg",
            ])
            .env_remove("OVRCR_HOOK_SOCKET")
            .env_remove("OVRCR_SESSION_ID")
            .env_remove("OVRCR_HOOK_TOKEN")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(23), "{provider}");
        assert_eq!(output.stdout, b"native-zero|kept arg", "{provider}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("reporting unavailable"),
            "{provider}: {stderr}"
        );
        assert!(
            !stderr.contains("OVRCR_AGENT_TOKEN"),
            "{provider}: {stderr}"
        );
    }
}

#[test]
fn pi_setup_prints_managed_launch_contract_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args(["agent", "setup", "pi", "--print"])
        .env("OVRCR_CONFIG", dir.path().join("registry.toml"))
        .env("OVRCR_SOCKET", dir.path().join("socket"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("agent run pi -- pi"), "{stdout}");
    assert!(stdout.contains("no settings"), "{stdout}");
    assert!(!dir.path().join("socket").exists());
    assert!(!dir.path().join("registry.toml").exists());
}
