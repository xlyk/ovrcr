use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

// These setup/doctor fixtures launch temporary executables under bounded native
// probe deadlines. Coordinate their cold starts; this suite checks reporting and
// cleanup contracts, not concurrent provider-launch throughput. Use the same
// poison-tolerant env_lock pattern as server_lifecycle so every assertion still
// runs after an unrelated fixture failure.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn command(root: &tempfile::TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
    command
        .env("OVRCR_CONFIG", root.path().join("registry.toml"))
        .env("OVRCR_SOCKET", root.path().join("socket"))
        .env_remove("OVRCR_SESSION_ID")
        .env_remove("OVRCR_HOOK_TOKEN")
        .env_remove("OVRCR_HOOK_SOCKET")
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN");
    command
}
fn setup(root: &tempfile::TempDir, value: &Value) -> (Value, String) {
    let path = root.path().join("settings ' quoted.json");
    let original = serde_json::to_vec(value).unwrap();
    std::fs::write(&path, &original).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let output = command(root)
        .args(["agent", "setup", "claude", "--print", "--settings"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!root.path().join("socket").exists());
    assert!(!root.path().join("registry.toml").exists());
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}
#[test]
fn setup_preserves_settings_and_external_renderer_bytes_once() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    let output_path = root.path().join("received");
    let calls = root.path().join("calls");
    let renderer = format!(
        "printf x >> '{}'; cat > '{}'; printf 'custom output'",
        calls.display(),
        output_path.display()
    );
    let original = json!({"permissions":{"allow":["Read"],"deny":["Bash(secret)"]}, "custom":"preserve", "hooks":{"SessionStart":[{"matcher":"startup", "hooks":[{"type":"command","command":"echo existing","timeout":8}]},{"matcher":"resume","hooks":[]}]}, "statusLine":{"type":"command", "command":renderer, "padding":2}});
    let (value, notes) = setup(&root, &original);
    assert_eq!(value["permissions"], original["permissions"]);
    assert_eq!(
        value["hooks"]["SessionStart"][0],
        original["hooks"]["SessionStart"][0]
    );
    assert_eq!(
        value["hooks"]["SessionStart"][1],
        original["hooks"]["SessionStart"][1]
    );
    assert_eq!(value["statusLine"]["padding"], 2);
    assert!(notes.contains("No file was written"));
    assert!(notes.contains("Removal:"));
    let text = value["statusLine"]["command"].as_str().unwrap();
    let mut child = Command::new("sh")
        .args(["-c", text])
        .env_remove("OVRCR_HOOK_TOKEN")
        .env_remove("OVRCR_HOOK_SOCKET")
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let bytes = b"{\"context_window\":null,\"extra\":\"original\"}\n";
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"custom output");
    assert_eq!(std::fs::read(output_path).unwrap(), bytes);
    assert_eq!(std::fs::read(calls).unwrap(), b"x");
    let (repeated, _) = setup(&root, &value);
    assert_eq!(repeated, value);
}
#[test]
fn setup_migrates_only_exact_legacy_statusline_and_marks_hooks() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    for prefix in ["ovrcr".to_string(), env!("CARGO_BIN_EXE_ovrcr").to_string()] {
        let (value, _) = setup(
            &root,
            &json!({"statusLine":{"type":"command", "command":format!("{prefix} report claude-context --stdin-json")}}),
        );
        let renderer = value["statusLine"]["command"].as_str().unwrap();
        assert!(renderer.contains("report claude-statusline --stdin-json"));
        assert!(!renderer.contains("--render-command"));
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "Notification",
            "Stop",
            "SessionEnd",
        ] {
            assert_eq!(value["hooks"][event][0]["hooks"][0]["async"], false);
            assert!(
                value["hooks"][event][0]["hooks"][0]["command"]
                    .as_str()
                    .unwrap()
                    .starts_with(": ovrcr-managed-claude-v1; ")
            );
        }
    }
    let wrapper = "sh -c 'ovrcr report claude-context --stdin-json'";
    let (value, notes) = setup(
        &root,
        &json!({"statusLine":{"type":"command", "command":wrapper}}),
    );
    assert_eq!(value["statusLine"]["command"], wrapper);
    assert!(notes.contains("Manual migration required"));
}
#[test]
fn setup_quotes_actual_executable_path() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("ovrcr ' quoted");
    std::fs::copy(env!("CARGO_BIN_EXE_ovrcr"), &binary).unwrap();
    let output = Command::new(&binary)
        .args(["agent", "setup", "claude", "--print"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut child = Command::new("sh")
        .args(["-c", value["statusLine"]["command"].as_str().unwrap()])
        .env_remove("OVRCR_HOOK_TOKEN")
        .env_remove("OVRCR_HOOK_SOCKET")
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"context_window":{"context_window_size":100,"current_usage":{"input_tokens":20,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("ctx 20%"));
}
#[test]
fn doctor_reports_partial_config_proof_missing_and_unsupported_versions_without_secrets() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("claude");
    for (version, expected_status, expected_version) in [
        ("2.1.267 (Claude Code)", "supported", "2.1.267"),
        ("2.1.268 (Claude Code)", "supported", "2.1.268"),
        ("2.1.266 (Claude Code)", "unsupported", "2.1.266"),
        ("2.1.269 (Claude Code)", "unsupported", "2.1.269"),
    ] {
        std::fs::write(
            &executable,
            format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (mut settings, _) = setup(&root, &json!({"permissions":{"secret":"NEVER_PRINT_ME"}}));
        settings["hooks"]["Stop"][0]["hooks"][0]["async"] = json!(true);
        let path = root.path().join("doctor.json");
        std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let output = command(&root)
            .args([
                "agent",
                "doctor",
                "claude",
                "--json",
                "--session",
                "1",
                "--executable",
            ])
            .arg(&executable)
            .arg("--settings")
            .arg(path)
            .env("OVRCR_HOOK_TOKEN", "NEVER_PRINT_ME")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["probe_status"], expected_status);
        assert_eq!(value["version"], expected_version);
        assert_eq!(value["supported_versions"], json!(["2.1.267", "2.1.268"]));
        assert_eq!(
            value["capabilities"]["initial_invocation"]["resume_forms"],
            if expected_version == "2.1.268" {
                json!(["--resume", "-r"])
            } else if expected_version == "2.1.267" {
                json!(["--resume"])
            } else {
                json!([])
            }
        );
        assert_eq!(
            value["configuration"]["effective_configuration"],
            "unverified"
        );
        assert!(
            value["configuration"]["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "Stop:asynchronous_reporting_unsupported")
        );
        assert_eq!(value["session_status"], "session_not_found");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("NEVER_PRINT_ME"));
        assert!(!root.path().join("socket").exists());
    }
    let output = command(&root)
        .args(["agent", "doctor", "claude", "--json", "--executable"])
        .arg(root.path().join("missing"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], Value::Null);
    assert_eq!(value["probe_status"], "unavailable");
    assert_eq!(value["configuration"]["status"], "unverified");
}

#[test]
fn doctor_inspects_unbound_and_unavailable_sessions_without_private_values() {
    let _env_lock = env_lock();
    use ovrcr::protocol::*;
    use std::os::unix::net::UnixListener;
    for bound in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(root.path().join("socket")).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            exchange_preamble(&mut stream).unwrap();
            let request: ClientMessage = read_frame(&mut stream).unwrap();
            assert_eq!(request.request, Request::Inspect);
            let session = SessionSummary {
                run: ovrcr_protocol::SessionRunId(1),
                kind: ovrcr_protocol::SessionKind::Terminal,
                recovery: None,
                title: None,
                id: SessionId(7),
                project: "p".into(),
                workspace: "w".into(),
                name: "n".into(),
                label: "claude".into(),
                pid: None,
                started_unix_ms: Some(0),
                phase: SessionPhase::Running,
                activity: AgentActivity::Unknown,
                context_usage: None,
                agent_epoch: 1,
                unread: None,
                agent: bound.then(|| AgentSnapshot {
                    binding: AgentBinding {
                        provider: AgentProvider::Claude,
                        invocation: "NEVER_PRIVATE_INVOCATION".into(),
                        conversation: "NEVER_PRIVATE_CONVERSATION".into(),
                        generation: 2,
                    },
                    activity: None,
                    metrics: None,
                    health: HealthSample {
                        state: ReporterHealth::Unavailable,
                        reason: Some("NEVER_PRIVATE_REASON".into()),
                    },
                    activity_revision: 0,
                    metrics_revision: 0,
                    health_revision: 1,
                    input_requests: Vec::new(),
                    input_revision: 0,
                }),
            };
            write_frame(
                &mut stream,
                &ServerMessage::Response {
                    request_id: request.request_id,
                    response: Response::Inventory {
                        registry: Default::default(),
                        sessions: vec![session],
                    },
                },
            )
            .unwrap();
        });
        let output = command(&root)
            .args(["agent", "doctor", "claude", "--json", "--executable"])
            .arg(root.path().join("missing"))
            .env("OVRCR_SESSION_ID", "7")
            .output()
            .unwrap();
        server.join().unwrap();
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value["session_status"],
            if bound { "bound" } else { "unbound" }
        );
        if bound {
            assert_eq!(value["source_health"]["state"], "Unavailable");
        } else {
            assert_eq!(value["binding"], Value::Null);
        }
        assert!(!String::from_utf8_lossy(&output.stdout).contains("NEVER_PRIVATE"));
    }
}

#[test]
fn doctor_interrupt_cleans_its_owned_version_probe_before_exit() {
    let _env_lock = env_lock();
    use std::os::unix::process::ExitStatusExt;
    use std::time::{Duration, Instant};
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("owned-probe");
        let executable = root.path().join("claude");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 60\n",
                ready.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut doctor = command(&root)
            .args(["agent", "doctor", "claude", "--json", "--executable"])
            .arg(&executable)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let group = loop {
            if let Ok(value) = std::fs::read_to_string(&ready)
                && let Ok(pid) = value.parse::<libc::pid_t>()
            {
                break pid;
            }
            if Instant::now() >= deadline {
                doctor.kill().unwrap();
                doctor.wait().unwrap();
                panic!("owned probe did not publish readiness");
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(unsafe { libc::getpgid(group) }, group);
        assert_eq!(unsafe { libc::kill(doctor.id() as libc::pid_t, signal) }, 0);
        let status = loop {
            if let Some(status) = doctor.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
                doctor.kill().unwrap();
                doctor.wait().unwrap();
                panic!("doctor exceeded bounded interrupt cleanup");
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let absent = unsafe { libc::kill(-group, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if !absent {
            assert_eq!(unsafe { libc::kill(-group, libc::SIGKILL) }, 0);
        }
        assert!(
            absent,
            "doctor left owned probe group {group} after signal {signal}"
        );
        assert_eq!(status.signal(), Some(signal));
        eprintln!("signal={signal} doctor_status={status} owned_probe_pgid={group} absent=ESRCH");
    }
}

#[test]
fn doctor_does_not_certify_filtered_hooks_wrong_types_or_statusline_suffixes() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    let (base, _) = setup(&root, &json!({}));
    for (case, expected) in [
        ("base", "supplied_file_supported"),
        ("filtered", "supplied_file_unsupported_or_unverified"),
        ("wrong-hook-type", "supplied_file_unsupported_or_unverified"),
        (
            "wrong-status-type",
            "supplied_file_unsupported_or_unverified",
        ),
        ("status-typo", "supplied_file_unsupported_or_unverified"),
        ("all-matcher", "supplied_file_supported"),
        ("composed", "supplied_file_supported"),
    ] {
        let mut value = base.clone();
        match case {
            "filtered" => value["hooks"]["SessionStart"][0]["matcher"] = json!("resume"),
            "wrong-hook-type" => {
                value["hooks"]["SessionStart"][0]["hooks"][0]["type"] = json!("prompt")
            }
            "wrong-status-type" => value["statusLine"]["type"] = json!("prompt"),
            "status-typo" => {
                value["statusLine"]["command"] = json!(format!(
                    "{}-typo",
                    value["statusLine"]["command"].as_str().unwrap()
                ))
            }
            "all-matcher" => value["hooks"]["SessionStart"][0]["matcher"] = json!("*"),
            "composed" => {
                value = setup(
                    &root,
                    &json!({"statusLine":{"type":"command", "command":"printf \"a'b\""}}),
                )
                .0
            }
            _ => {}
        }
        let path = root.path().join("doctor-matcher.json");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let output = command(&root)
            .args(["agent", "doctor", "claude", "--json", "--executable"])
            .arg(root.path().join("missing"))
            .arg("--settings")
            .arg(path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["configuration"]["status"], expected, "case {case}");
        assert_eq!(
            result["configuration"]["effective_configuration"],
            "unverified"
        );
    }
}

#[test]
fn codex_setup_preserves_handlers_trust_and_quotes_noop_helper() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("ovrcr ' quoted");
    std::fs::copy(env!("CARGO_BIN_EXE_ovrcr"), &binary).unwrap();
    let path = root.path().join("config with spaces.toml");
    let original = r#"
[projects.example]
trust_level = "trusted"
[hooks.state.example]
trusted_hash = "preserve-only"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "echo first"
[[hooks.Stop.hooks]]
type = "command"
command = "echo second"
[[hooks.PermissionRequest]]
[[hooks.PermissionRequest.hooks]]
type = "command"
command = "my-approval-handler"
"#;
    std::fs::write(&path, original).unwrap();
    let run = || {
        Command::new(&binary)
            .args(["agent", "setup", "codex", "--print", "--settings"])
            .arg(&path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let value: toml::Value = toml::from_str(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    let before: toml::Value = toml::from_str(original).unwrap();
    assert_eq!(value["projects"], before["projects"]);
    assert_eq!(value["hooks"]["state"], before["hooks"]["state"]);
    assert_eq!(
        value["hooks"]["PermissionRequest"],
        before["hooks"]["PermissionRequest"]
    );
    assert_eq!(value["hooks"]["Stop"][0], before["hooks"]["Stop"][0]);
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "Stop",
        "Interrupt",
        "SessionEnd",
    ] {
        let groups = value["hooks"][event].as_array().unwrap();
        let cmd = groups.last().unwrap()["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(cmd.starts_with("exec '"));
        // Execute the actual emitted shell command, including its quoted absolute path.
        let mut helper = Command::new("sh");
        helper
            .env("OVRCR_CONFIG", root.path().join("registry.toml"))
            .env("OVRCR_SOCKET", root.path().join("socket"));
        helper
            .args(["-c", cmd])
            .env_remove("OVRCR_HOOK_TOKEN")
            .env_remove("OVRCR_HOOK_SOCKET")
            .env_remove("OVRCR_AGENT_TOKEN")
            .env_remove("OVRCR_AGENT_SOCKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = helper.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{{\"hook_event_name\":\"{event}\"}}\n").as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty());
    }
    std::fs::write(&path, &output.stdout).unwrap();
    let repeated = run();
    assert_eq!(output.stdout, repeated.stdout);
    assert!(!root.path().join("socket").exists());
}

#[test]
fn codex_doctor_defaults_dispatch_and_rejects_versions_without_server_or_secrets() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    for (provider, response) in [
        ("codex", "codex-cli 0.153.0"),
        ("claude", "2.1.268 (Claude Code)"),
    ] {
        let executable = root.path().join(provider);
        std::fs::write(&executable, format!("#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --version ] || exit 81\nprintf '%s\\n' '{response}'\n")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = command(&root)
            .args(["agent", "doctor", provider, "--json"])
            .env("PATH", root.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["provider"], provider);
        assert_eq!(value["executable"], provider);
        assert_eq!(value["probe_status"], "supported");
    }
    let executable = root.path().join("codex version with spaces");
    for response in ["codex-cli 0.152.0", "codex-cli 0.153.1", "NEVER_PRINT_ME"] {
        std::fs::write(
            &executable,
            format!("#!/bin/sh\nprintf '%s\\n' '{response}'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = command(&root)
            .args(["agent", "doctor", "codex", "--json", "--executable"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("NEVER_PRINT_ME"));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["probe_status"], "unsupported");
        assert_eq!(value["capabilities"]["initial_invocation"]["fresh"], false);
    }
    assert!(!root.path().join("socket").exists());
    assert!(!root.path().join("registry.toml").exists());
}

#[test]
fn codex_doctor_checks_supplied_hooks_without_certifying_trust() {
    let _env_lock = env_lock();
    let root = tempfile::tempdir().unwrap();
    let setup = command(&root)
        .args(["agent", "setup", "codex", "--print"])
        .output()
        .unwrap();
    assert!(setup.status.success());
    let base: toml::Value = toml::from_str(std::str::from_utf8(&setup.stdout).unwrap()).unwrap();
    let path = root.path().join("settings.toml");
    for case in ["base", "filtered", "async", "wrong_type", "invalid"] {
        let mut value = base.clone();
        match case {
            "filtered" => {
                value["hooks"]["Stop"][0]
                    .as_table_mut()
                    .unwrap()
                    .insert("matcher".into(), "never".into());
            }
            "async" => {
                value["hooks"]["Stop"][0]["hooks"][0]
                    .as_table_mut()
                    .unwrap()
                    .insert("async".into(), true.into());
            }
            "wrong_type" => value["hooks"]["Stop"][0]["hooks"][0]["type"] = "prompt".into(),
            _ => {}
        }
        let bytes = if case == "invalid" {
            "NEVER_PRINT_ME = [".into()
        } else {
            toml::to_string(&value).unwrap()
        };
        std::fs::write(&path, &bytes).unwrap();
        let output = command(&root)
            .args(["agent", "doctor", "codex", "--json", "--settings"])
            .arg(&path)
            .arg("--executable")
            .arg(root.path().join("missing"))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("NEVER_PRINT_ME"));
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["probe_status"], "unavailable");
        assert_eq!(
            result["configuration"]["status"],
            if case == "base" {
                "supplied_file_supported"
            } else if case == "invalid" {
                "unverified"
            } else {
                "supplied_file_unsupported_or_unverified"
            }
        );
        assert_eq!(
            result["release_status"],
            "accepted_exact_0.153.0_hooks_only"
        );
        assert_eq!(result["configuration"]["hook_trust"], "unverified");
        assert_eq!(result["configuration"]["delivery"], "unverified");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
        if case == "invalid" {
            let output = command(&root)
                .args(["agent", "setup", "codex", "--print", "--settings"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("NEVER_PRINT_ME"));
        }
    }
}
