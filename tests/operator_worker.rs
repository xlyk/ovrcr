use std::process::Command;

#[test]
fn operator_cli_exposes_auth_commands_and_hides_worker() {
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args(["operator", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    for command in ["login", "status", "models", "logout"] {
        assert!(help.contains(command), "missing {command}: {help}");
    }
    assert!(!help.contains("worker"));
}

#[test]
fn worker_requires_managed_lease_before_auth_or_inference() {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .env_clear()
        .env("HOME", home.path())
        .env("OVRCR_HOME", home.path())
        .args([
            "operator",
            "worker",
            "--conversation",
            "1",
            "--turn",
            "1",
            "--session",
            "1",
            "--run",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("managed operator reservation required"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!home.path().join("operator-auth").exists());
}

#[test]
fn bash_environment_isolates_model_and_reporting_credentials() {
    let home = tempfile::tempdir().unwrap();
    let hook = home.path().join("startup-hook");
    std::fs::write(
        &hook,
        format!("touch '{}'", home.path().join("sourced").display()),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "bash_environment_probe",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("OVRCR_SOCKET", "public-fixture-socket")
        .env("BASH_ENV", hook)
        .env("OPENAI_API_KEY", "synthetic-openai")
        .env("XAI_API_KEY", "synthetic-xai")
        .env("OVRCR_HOOK_TOKEN", "synthetic-hook")
        .env("OVRCR_AGENT_TOKEN", "synthetic-agent")
        .env("OVRCR_DELEGATION_TOKEN", "synthetic-delegation")
        .env("GITHUB_TOKEN", "synthetic-user-tool")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(!home.path().join("sourced").exists());
}
#[test]
#[ignore = "subprocess entrypoint with synthetic environment only"]
fn bash_environment_probe() {
    use ovrcr::operator::bash::{Bash, BashArgs};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let cwd=std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let executable=std::env::current_exe().unwrap();
        let tool=Bash::new(cwd,executable.clone());
        let result=tool.execute(BashArgs { command:r#"printf '%s|%s|%s|%s|%s|%s|%s|%s|%s' "$OPENAI_API_KEY" "$XAI_API_KEY" "$OVRCR_HOOK_TOKEN" "$OVRCR_AGENT_TOKEN" "$OVRCR_DELEGATION_TOKEN" "$GITHUB_TOKEN" "$OVRCR_SOCKET" "$BASH_ENV" "$OVRCR_EXECUTABLE""#.into(),cwd:None,timeout_seconds:None }).await.unwrap().result;
        assert_eq!(result.exit_code,Some(0));
        assert_eq!(result.output,format!("|||||synthetic-user-tool|public-fixture-socket||{}",executable.display()));
    });
}
#[test]
fn auth_cli_status_and_logout_are_local_and_do_not_start_a_server() {
    let home = tempfile::tempdir().unwrap();
    for provider in ["chatgpt", "grok"] {
        for action in ["status", "logout"] {
            let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
                .env_clear()
                .env("HOME", home.path())
                .env("OVRCR_HOME", home.path())
                .args(["--json", "operator", action, provider])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(json["signed_in"], false);
        }
    }
    assert!(!home.path().join("server.sock").exists());
}
