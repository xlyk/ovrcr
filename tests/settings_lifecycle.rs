//! Settings lifecycle against a real `ovrcr` (item 5 of the usage-defaults
//! plan). For now this holds the proofs of the fixtures the lifecycle cases
//! stand on: a fresh install's private HOME, and a stand-in `claude`.

#[path = "support/live.rs"]
mod live;

use live::claude_auth;

use serde_json::Value;

#[test]
fn isolated_home_runs_the_server_and_cli_on_the_default_config_path() {
    let live = live::Live::isolated_home();
    let home = live.home.clone().unwrap();
    let directory = home.join(live::DEFAULT_CONFIG_DIR);
    assert_eq!(live.config, directory.join("config.toml"));
    assert_eq!(
        std::fs::read_dir(&home).unwrap().count(),
        0,
        "HOME is empty"
    );

    live.start_binary();
    live.ready("feature/fresh");
    // The Server found its instance identity under the private HOME, with no
    // OVRCR_CONFIG to point it there.
    assert!(
        directory.join("config.toml.sqlite3").is_file(),
        "{:?}",
        std::fs::read_dir(&home).unwrap().collect::<Vec<_>>()
    );

    let output = live
        .command()
        .args(["settings", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["path"],
        directory.join("dashboard.toml").display().to_string()
    );
    assert_eq!(report["findings"], serde_json::json!([]));
    let rows = report["rows"].as_array().unwrap();
    assert!(
        rows.iter().all(|row| row["source"] == "Default"),
        "{rows:?}"
    );
    // `~` is the private HOME, and nothing else in the default roots exists there.
    let roots = rows
        .iter()
        .find(|row| row["key"] == "picker_roots")
        .unwrap();
    assert_eq!(
        roots["value"],
        format!("[{}]", toml::Value::String(home.display().to_string()))
    );
}

#[test]
fn claude_fixture_answers_auth_status_from_its_current_state() {
    let root = tempfile::tempdir().unwrap();
    let claude = claude_auth::ClaudeAuth::install(root.path());
    let status = || {
        let output = std::process::Command::new(&claude.command)
            .args(["auth", "status", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let state = |status: Value| {
        (
            status["loggedIn"].clone(),
            status["authMethod"].clone(),
            status["subscriptionType"].clone(),
        )
    };
    assert_eq!(
        state(status()),
        (true.into(), "claude.ai".into(), "max".into())
    );
    claude.set(false, "none", None);
    assert_eq!(state(status()), (false.into(), "none".into(), Value::Null));
    claude.set(true, "api_key", None);
    assert_eq!(
        state(status()),
        (true.into(), "api_key".into(), Value::Null)
    );

    let other = std::process::Command::new(&claude.command)
        .args(["-p", "hello"])
        .output()
        .unwrap();
    assert_eq!(other.status.code(), Some(2));
}
