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

fn helper_is_silent_without_reading_stdin(provider: &str) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
    command
        .args(["report", provider, "--stdin"])
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
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
            panic!("unmanaged {provider} helper waited on stdin");
        }
        std::thread::yield_now();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn pi_helper_outside_managed_invocation_is_silent_without_reading_stdin() {
    helper_is_silent_without_reading_stdin("pi");
}

#[test]
fn omp_helper_outside_managed_invocation_is_silent_without_reading_stdin() {
    helper_is_silent_without_reading_stdin("omp");
}

#[test]
fn pi_extension_source_embeds_the_json_escaped_binary_path() {
    let source =
        ovrcr::report::pi::extension_source(std::path::Path::new("/opt/o v r/\"ovrcr\"")).unwrap();
    assert!(
        source.contains(r#"const OVRCR_BINARY = "/opt/o v r/\"ovrcr\"";"#),
        "{source}"
    );
    assert!(!source.contains("__OVRCR_BINARY__"));
    assert!(source.contains("[\"report\", \"pi\", \"--stdin\"]"));
}

fn fake_harness(dir: &std::path::Path, name: &str, version_line: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '{version_line}\\n'; exit; fi\nexit 3\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn pi_and_omp_doctor_classify_versions_as_evidence_not_an_allowlist() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        (
            "pi",
            fake_harness(dir.path(), "pi-tested", "0.85.1"),
            "probed",
            Some("0.85.1"),
            "tested",
            "\"available\"",
        ),
        (
            "pi",
            fake_harness(dir.path(), "pi-newer", "0.86.0"),
            "probed",
            Some("0.86.0"),
            "unverified_compatible_until_proven_otherwise",
            "\"available\"",
        ),
        (
            "pi",
            std::path::PathBuf::from("/nonexistent/harness"),
            "unavailable",
            None,
            "unknown",
            "\"available\"",
        ),
        (
            "omp",
            fake_harness(dir.path(), "omp-tested", "omp/18.1.19"),
            "probed",
            Some("18.1.19"),
            "tested",
            "\"available\"",
        ),
    ];
    for (provider, executable, probe, version, status, reporting) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(["agent", "doctor", provider, "--json", "--executable"])
            .arg(&executable)
            .env("OVRCR_CONFIG", dir.path().join("registry.toml"))
            .env("OVRCR_SOCKET", dir.path().join("socket"))
            .env_remove("OVRCR_SESSION_ID")
            .output()
            .unwrap();
        assert!(output.status.success(), "{provider} {executable:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["provider"], provider);
        assert_eq!(report["probe_status"], probe, "{executable:?}");
        assert_eq!(
            report["version"],
            version.map_or(serde_json::Value::Null, |v| v.into())
        );
        assert_eq!(report["version_status"], status, "{executable:?}");
        assert_eq!(report["capabilities"]["managed_launch"], true);
        assert_eq!(report["capabilities"]["reporting"].to_string(), reporting);
        assert_eq!(report["capabilities"]["metrics"], "absent");
        // #95 split `input_requests` into the two surfaces it actually covers. Pi has no
        // approval surface of its own beyond its prompts, and no question surface at all;
        // Oh My Pi reports both, its questions from the ask tool's lifetime.
        assert_eq!(report["capabilities"]["approvals"], "available");
        assert_eq!(
            report["capabilities"]["questions"],
            if provider == "pi" {
                "unavailable_no_provider_surface"
            } else {
                "available_tool_lifetime"
            }
        );
        assert!(
            report["capabilities"]["input_requests"].is_null(),
            "the merged field is gone"
        );
        assert_eq!(report["session_status"], "not_requested");
        assert!(
            report["tested_versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v
                    == if provider == "pi" {
                        "0.85.1"
                    } else {
                        "18.1.19"
                    })
        );
        assert!(
            !dir.path().join("socket").exists(),
            "doctor must not start a server"
        );
    }
}
