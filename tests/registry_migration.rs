#[path = "support/live.rs"]
mod live;

use live::{Live, wait_deadline};
use rusqlite::{Connection, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

fn database(live: &Live) -> PathBuf {
    ovrcr::config::database_path(&live.config)
}

fn identity(live: &Live) -> PathBuf {
    ovrcr::config::legacy_identity_path(&live.config)
}

fn seed_project(live: &Live, name: &str) -> String {
    let original = toml::to_string(&ovrcr::config::Registry {
        projects: vec![ovrcr::config::ProjectRecord {
            name: name.into(),
            repo: live.repo.clone(),
            workspace_root: live.workspace_root.clone(),
            workspaces: Vec::new(),
        }],
    })
    .unwrap();
    std::fs::write(identity(live), &original).unwrap();
    original
}

fn run(live: &Live, args: &[&str]) -> Output {
    Command::new(&live.executable)
        .args(args)
        .env("OVRCR_HOME", &live.config)
        .env("OVRCR_SOCKET", &live.socket)
        .env("SHELL", "/bin/sh")
        .output()
        .unwrap()
}

fn json(live: &Live, args: &[&str]) -> serde_json::Value {
    let mut cli_args = vec!["--json"];
    cli_args.extend_from_slice(args);
    let out = run(live, &cli_args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).unwrap()
}

fn fails(live: &Live, args: &[&str]) -> Output {
    let out = run(live, args);
    assert!(
        !out.status.success(),
        "{args:?} succeeded: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    out
}

fn add_project(live: &Live, name: &str) -> Output {
    run(
        live,
        &[
            "project",
            "add",
            name,
            live.repo.to_str().unwrap(),
            "--workspace-root",
            live.workspace_root.to_str().unwrap(),
        ],
    )
}

fn refuse_startup(live: &Live) -> ExitStatus {
    let mut child = Command::new(&live.executable)
        .arg("server")
        .env("OVRCR_SOCKET", &live.socket)
        .env("OVRCR_HOME", &live.config)
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn isolated OVRCR server");
    let deadline = Instant::now() + wait_deadline();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let status = child.wait().expect("reap refused server");
                panic!(
                    "server did not exit after refused startup: {status}; socket exists: {}",
                    live.socket.exists()
                );
            }
            Ok(None) => std::thread::park_timeout(Duration::from_millis(5)),
            Err(error) => panic!("wait for refused server: {error}"),
        }
    }
}

fn project_names(live: &Live) -> Vec<String> {
    json(live, &["project", "list"])
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["name"].as_str().unwrap().to_owned())
        .collect()
}

fn refuse_existing_sqlite(arrange: impl FnOnce(&Path)) {
    let live = Live::idle().bounded();
    let original = seed_project(&live, "legacy-only");
    let database = database(&live);
    arrange(&database);
    let sqlite_before = std::fs::read(&database).unwrap();

    fails(&live, &["--json", "project", "get", "legacy-only"]);
    fails(&live, &["--json", "project", "get", "hijacked"]);
    assert!(!live.socket.exists());

    let status = refuse_startup(&live);
    assert!(!status.success(), "incompatible storage must not start");
    assert!(
        !live.socket.exists(),
        "refused startup must not publish a socket"
    );
    fails(&live, &["--json", "project", "list"]);
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);
    assert_eq!(std::fs::read(&database).unwrap(), sqlite_before);
}

#[test]
fn offline_inventory_before_startup_does_not_create_sqlite() {
    let live = Live::idle().bounded();
    let original = seed_project(&live, "alpha");

    let project = json(&live, &["project", "get", "alpha"]);
    assert_eq!(project["name"], "alpha");
    assert_eq!(project["workspace_count"], 0);
    assert!(!database(&live).exists());
    assert!(!live.socket.exists());
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);
}

#[test]
fn malformed_legacy_startup_and_offline_query_fail_without_replacing_source() {
    let live = Live::idle().bounded();
    let original = "[[projects]\n";
    std::fs::write(identity(&live), original).unwrap();

    fails(&live, &["--json", "project", "list"]);
    assert!(!live.socket.exists());

    let status = refuse_startup(&live);
    assert!(!status.success(), "malformed legacy must not start");
    assert!(
        !live.socket.exists(),
        "failed startup must not publish inventory"
    );
    fails(&live, &["--json", "project", "list"]);
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);
}

#[test]
fn incompatible_sqlite_refuses_startup_and_offline_without_legacy_fallback() {
    refuse_existing_sqlite(|database| {
        let connection = Connection::open(database).unwrap();
        connection
            .pragma_update(None, "application_id", 0x4f565243i64)
            .unwrap();
        connection
            .pragma_update(None, "user_version", 99i64)
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE hijacked (name TEXT);
                 INSERT INTO hijacked VALUES ('hijacked');",
            )
            .unwrap();
    });
}

#[test]
fn corrupt_sqlite_refuses_startup_and_offline_without_legacy_fallback() {
    refuse_existing_sqlite(|database| {
        std::fs::write(database, b"this is not a sqlite database").unwrap();
    });
}

#[test]
fn nonempty_unversioned_sqlite_refuses_startup_without_legacy_fallback() {
    refuse_existing_sqlite(|database| {
        Connection::open(database)
            .unwrap()
            .execute_batch(
                "CREATE TABLE sqliteX (body TEXT);
                 INSERT INTO sqliteX VALUES ('foreign');",
            )
            .unwrap();
    });
}

#[test]
#[ignore = "uncommitted registry writer killed by the interrupted-migration test"]
fn hold_uncommitted_sqlite_helper() {
    let database = std::env::var("OVRCR_TEST_DATABASE").unwrap();
    let ready = std::env::var("OVRCR_TEST_READY").unwrap();
    let mut connection = Connection::open(database).unwrap();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    transaction
        .execute_batch(
            "CREATE TABLE interrupted (n INTEGER);
             INSERT INTO interrupted VALUES (1);",
        )
        .unwrap();
    std::fs::write(ready, b"ready").unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn interrupted_initial_sqlite_transaction_recovers_into_one_time_migration() {
    let live = Live::idle().bounded();
    let original = seed_project(&live, "recover");
    let database = database(&live);
    let ready = live.root.path().join("interrupt.ready");
    let mut holder = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "hold_uncommitted_sqlite_helper",
            "--nocapture",
        ])
        .env("OVRCR_TEST_DATABASE", &database)
        .env("OVRCR_TEST_READY", &ready)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn uncommitted sqlite holder");
    let deadline = Instant::now() + wait_deadline();
    while !ready.exists() && Instant::now() < deadline {
        std::thread::park_timeout(Duration::from_millis(5));
    }
    let transaction_started = ready.exists();
    holder.kill().expect("kill owned uncommitted sqlite holder");
    holder.wait().expect("reap owned uncommitted sqlite holder");
    assert!(transaction_started, "holder did not begin its transaction");

    let sqlite_before = std::fs::read(&database).unwrap();
    assert_eq!(project_names(&live), ["recover"]);
    assert!(
        !live.socket.exists(),
        "offline inspection must not start a server"
    );
    assert_eq!(std::fs::read(&database).unwrap(), sqlite_before);
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);

    live.start_binary();
    assert_eq!(project_names(&live), ["recover"]);
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);

    let shutdown = run(&live, &["shutdown", "--kill"]);
    assert!(
        shutdown.status.success(),
        "{}",
        String::from_utf8_lossy(&shutdown.stderr)
    );
    live.join();
    assert_eq!(project_names(&live), ["recover"]);
    let workspaces = json(&live, &["workspace", "list", "--project", "recover"]);
    let workspaces = workspaces.as_array().unwrap();
    assert_eq!(
        workspaces.len(),
        1,
        "interrupted migration must create one protected root"
    );
    assert_eq!(workspaces[0]["path"], live.repo.to_str().unwrap());
    assert_eq!(workspaces[0]["branch"], "main");
    assert!(!live.socket.exists());
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);

    live.start_binary();
    assert_eq!(project_names(&live), ["recover"]);
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);
}

#[test]
fn held_sqlite_write_lock_rejects_mutation_boundedly_then_recovers() {
    let live = Live::idle().bounded();
    seed_project(&live, "held");
    live.start_binary();
    assert_eq!(project_names(&live), ["held"]);

    let mut connection = Connection::open(database(&live)).unwrap();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let started = Instant::now();
    let blocked = add_project(&live, "blocked");
    assert!(
        !blocked.status.success(),
        "locked write must not publish a mutation: {}",
        String::from_utf8_lossy(&blocked.stdout)
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "locked mutation must fail boundedly, took {:?}",
        started.elapsed()
    );
    assert_eq!(project_names(&live), ["held"]);
    drop(transaction);

    let recovered = add_project(&live, "blocked");
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(project_names(&live), ["blocked", "held"]);
}

#[test]
fn custom_home_keeps_task_and_settings_namespaces() {
    let mut live = Live::idle().bounded();
    let home = live.root.path().join("alpha");
    std::fs::create_dir_all(&home).unwrap();
    live.config = home;
    let original = seed_project(&live, "alpha");

    let settings = live.config.join("dashboard.toml");
    let settings_body = "# keep-namespace-sentinel\nready_sound = true\n";
    std::fs::write(&settings, settings_body).unwrap();
    let decoy = live.config.join("config.sqlite3");
    std::fs::write(&decoy, "decoy-wrong-extension-replacement").unwrap();

    let other = live.root.path().join("beta");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        ovrcr::config::legacy_identity_path(&other),
        "projects = []\n",
    )
    .unwrap();

    assert_eq!(json(&live, &["project", "get", "alpha"])["name"], "alpha");
    assert!(!database(&live).exists());
    assert!(!live.socket.exists());
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);

    live.start_binary();
    let task = json(
        &live,
        &[
            "task",
            "create",
            "alpha-sentinel",
            "--scratch",
            "--every",
            "1h",
            "--model",
            "fixture/model",
            "--prompt",
            "keep-me-on-custom-config",
        ],
    );
    assert_eq!(task["spec"]["name"], "alpha-sentinel");
    let shutdown = run(&live, &["shutdown", "--kill"]);
    assert!(
        shutdown.status.success(),
        "{}",
        String::from_utf8_lossy(&shutdown.stderr)
    );
    live.join();

    assert!(database(&live).exists());
    assert!(
        database(&live).ends_with("registry.sqlite3"),
        "sqlite path must be registry.sqlite3 in the instance directory: {}",
        database(&live).display()
    );
    assert_eq!(
        std::fs::read_to_string(&decoy).unwrap(),
        "decoy-wrong-extension-replacement"
    );
    assert_eq!(std::fs::read_to_string(identity(&live)).unwrap(), original);
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), settings_body);
    assert!(!live.socket.exists());
    assert_eq!(json(&live, &["project", "get", "alpha"])["name"], "alpha");

    let tasks = json(&live, &["task", "list"]);
    assert!(
        tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["spec"]["name"] == "alpha-sentinel"),
        "{tasks}"
    );

    let other_tasks = Command::new(&live.executable)
        .args(["--json", "task", "list"])
        .env("OVRCR_HOME", &other)
        .env("OVRCR_SOCKET", live.root.path().join("beta.sock"))
        .env("SHELL", "/bin/sh")
        .output()
        .unwrap();
    assert!(
        other_tasks.status.success(),
        "{}",
        String::from_utf8_lossy(&other_tasks.stderr)
    );
    let other_tasks: serde_json::Value = serde_json::from_slice(&other_tasks.stdout).unwrap();
    assert_eq!(other_tasks, serde_json::json!([]));
    assert!(!live.root.path().join("beta.sock").exists());
}
