#[path = "support/live.rs"]
mod live;
#[path = "support/operator_harness.rs"]
mod operator_harness;

use live::{Live, wait_deadline};
use ovrcr::protocol::{Request, Response};
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
fn schema_30_migrates_retained_sessions_with_empty_last_activity() {
    let live = Live::idle().bounded();
    live.start_binary();
    live.ready("feature/last-activity");
    let workspace = live.workspace_path(live::WORKSPACE);
    let created = json(
        &live,
        &[
            "terminal",
            "create",
            "--project",
            live::PROJECT,
            "--path",
            workspace.to_str().unwrap(),
            "--name",
            "migrated-last-activity",
            "--",
            "/bin/sh",
        ],
    );
    let id = created["id"].as_u64().unwrap();
    let shutdown = run(&live, &["shutdown", "--kill"]);
    assert!(
        shutdown.status.success(),
        "{}",
        String::from_utf8_lossy(&shutdown.stderr)
    );
    live.join();

    let connection = Connection::open(database(&live)).unwrap();
    let has_last_activity = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('retained_sessions') WHERE name='last_activity_unix_ms')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap();
    if has_last_activity {
        connection
            .execute(
                "ALTER TABLE retained_sessions DROP COLUMN last_activity_unix_ms",
                [],
            )
            .unwrap();
    }
    connection
        .pragma_update(None, "user_version", 30i64)
        .unwrap();
    drop(connection);

    live.start_binary();
    assert!(matches!(
        live.request(Request::List),
        Response::Hierarchy(_)
    ));
    let sessions = json(&live, &["terminal", "list"]);
    let session = sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["id"] == id)
        .unwrap();
    assert!(
        session
            .as_object()
            .is_some_and(|session| session.contains_key("last_activity_unix_ms")),
        "migrated session summary omitted Last activity: {session}"
    );
    assert!(session["last_activity_unix_ms"].is_null());
    let shutdown = run(&live, &["shutdown"]);
    assert!(
        shutdown.status.success(),
        "{}",
        String::from_utf8_lossy(&shutdown.stderr)
    );
    live.join();
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

/// The schema-28 Operator shape, applied to the fixture's own registry.
fn downgrade_operator_to_schema_28(connection: &Connection) {
    connection
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    connection
        .execute_batch(
            "DROP INDEX IF EXISTS operator_single_conversation;
             DROP INDEX IF EXISTS operator_one_running_turn;
             CREATE TABLE operator_conversations_28 (
                id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
                session INTEGER NOT NULL UNIQUE REFERENCES retained_sessions(id) ON DELETE RESTRICT,
                provider TEXT NOT NULL, model TEXT NOT NULL, title TEXT NOT NULL);
             INSERT INTO operator_conversations_28(id,session,provider,model,title)
                SELECT id,session,provider,model,title FROM operator_conversations;
             DROP TABLE operator_conversations;
             ALTER TABLE operator_conversations_28 RENAME TO operator_conversations;
             CREATE TABLE operator_turns_28 (
                id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
                conversation INTEGER NOT NULL REFERENCES operator_conversations(id) ON DELETE CASCADE,
                run INTEGER NOT NULL CHECK(run>0), state TEXT NOT NULL,
                sequence INTEGER NOT NULL DEFAULT 0, UNIQUE(conversation,run));
             INSERT INTO operator_turns_28(id,conversation,run,state,sequence)
                SELECT id,conversation,run,state,sequence FROM operator_turns;
             DROP TABLE operator_turns;
             ALTER TABLE operator_turns_28 RENAME TO operator_turns;
             PRAGMA user_version = 28;",
        )
        .unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
}

#[test]
fn schema_28_operator_rows_survive_upgrade_and_no_singleton_exists_until_open() {
    let live = Live::idle().bounded();
    live.start_binary();
    let opened = {
        let mut client = operator_harness::OperatorClient::connect(&live);
        client.open()
    };
    let shutdown = run(&live, &["shutdown", "--kill"]);
    assert!(
        shutdown.status.success(),
        "{}",
        String::from_utf8_lossy(&shutdown.stderr)
    );
    live.join();

    let database = database(&live);
    let connection = Connection::open(&database).unwrap();
    let first_conversation: i64 = connection
        .query_row("SELECT id FROM operator_conversations", [], |row| {
            row.get(0)
        })
        .unwrap();
    let first_session: i64 = connection
        .query_row(
            "SELECT session FROM operator_conversations WHERE id=?1",
            [first_conversation],
            |row| row.get(0),
        )
        .unwrap();
    // A legacy chat keeps its pre-singleton provider/model dialect bytes.
    connection
        .execute(
            "UPDATE operator_conversations SET provider='Grok',model='legacy-model-9',title='Legacy Chat One',singleton=0 WHERE id=?1",
            [first_conversation],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE retained_sessions SET disposition=1, stopped=1 WHERE id=?1",
            [first_session],
        )
        .unwrap();
    // A second legacy conversation with its own retained session.
    let second_session = {
        connection
            .execute(
                "INSERT INTO retained_sessions (run,project,workspace,name,label,cwd,kind,title_revision,stopped,disposition,scope)
                 SELECT 0,project,workspace,'operator-legacy-two','Legacy Two',cwd,kind,0,1,0,?1 FROM retained_sessions WHERE id=?2",
                rusqlite::params![
                    format!("{{\"OperatorConversation\":{{\"conversation\":{}}}}}", first_conversation + 1),
                    first_session
                ],
            )
            .unwrap();
        connection.last_insert_rowid()
    };
    connection
        .execute(
            "INSERT INTO operator_conversations(id,session,provider,model,title,singleton)
             VALUES(?1,?2,'ChatGpt','legacy-two-model','Legacy Chat Two',0)",
            rusqlite::params![first_conversation + 1, second_session],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO operator_turns(conversation,run,state,sequence) VALUES(?1,1,'Complete',3)",
            [first_conversation],
        )
        .unwrap();
    let first_turn = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO operator_turns(conversation,run,state,sequence) VALUES(?1,1,'Error',1)",
            [first_conversation + 1],
        )
        .unwrap();
    let second_turn = connection.last_insert_rowid();
    let legacy_payloads = [
        (first_conversation, first_turn, "{\"User\":{\"text\":\"legacy one\",\"context\":{\"project\":null,\"workspace\":null,\"workspace_id\":null,\"cwd\":null,\"session\":null,\"run\":null}}}".to_string()),
        (first_conversation, first_turn, "{\"Worker\":{\"AssistantDelta\":\"legacy answer one\"}}".to_string()),
        (first_conversation + 1, second_turn, "{\"Worker\":{\"Failed\":\"legacy provider failure\"}}".to_string()),
    ];
    for (conversation, turn, content) in &legacy_payloads {
        connection
            .execute(
                "INSERT INTO operator_messages(conversation,turn,content) VALUES(?1,?2,?3)",
                rusqlite::params![conversation, turn, content],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO operator_batches(turn,sequence,digest) VALUES(?1,1,'legacy-digest')",
            [first_turn],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO operator_model_history(conversation,turn,history) VALUES(?1,?2,'{\"legacy\":true}')",
            rusqlite::params![first_conversation, first_turn],
        )
        .unwrap();
    let before: Vec<String> = connection
        .prepare("SELECT content FROM operator_messages ORDER BY ordinal")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(before.len(), legacy_payloads.len());
    downgrade_operator_to_schema_28(&connection);
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        28
    );
    drop(connection);

    // The application upgrade is the migration path under test.
    live.start_binary();
    // The published socket can precede the first registry open; wait for the
    // application's own upgrade to become visible.
    assert!(matches!(
        live.request(Request::List),
        Response::Hierarchy(_)
    ));
    let connection = Connection::open(&database).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut version: i64 = 0;
    while version != 32 && std::time::Instant::now() < deadline {
        version = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(version, 32, "the application upgrades schema 28 to 32");
    let columns: Vec<String> = connection
        .prepare("SELECT name FROM pragma_table_info('operator_conversations')")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(columns.contains(&"singleton".to_string()));
    assert!(columns.contains(&"pi_checkpoint_turn".to_string()));
    assert!(columns.contains(&"reasoning".to_string()));
    let turn_columns: Vec<String> = connection
        .prepare("SELECT name FROM pragma_table_info('operator_turns')")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(turn_columns.contains(&"model_override".to_string()));
    assert!(turn_columns.contains(&"thinking_level".to_string()));
    assert!(turn_columns.contains(&"started_unix_ms".to_string()));
    assert!(turn_columns.contains(&"completed_unix_ms".to_string()));
    let legacy_timing: Vec<(i64, Option<i64>, Option<i64>)> = connection
        .prepare("SELECT id,started_unix_ms,completed_unix_ms FROM operator_turns ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        legacy_timing,
        vec![(first_turn, None, None), (second_turn, None, None)],
        "legacy turns stay explicitly untimed instead of inventing timestamps"
    );

    let after: Vec<String> = connection
        .prepare("SELECT content FROM operator_messages ORDER BY ordinal")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(after, before, "legacy journal bytes are preserved verbatim");
    let legacy: Vec<(String, String, String)> = connection
        .prepare("SELECT provider,model,title FROM operator_conversations ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        legacy,
        vec![
            (
                "Grok".into(),
                "legacy-model-9".into(),
                "Legacy Chat One".into()
            ),
            (
                "ChatGpt".into(),
                "legacy-two-model".into(),
                "Legacy Chat Two".into()
            ),
        ],
        "legacy provider dialect bytes are never decoded or rewritten"
    );
    let model_history: String = connection
        .query_row("SELECT history FROM operator_model_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(model_history, "{\"legacy\":true}");
    let singleton_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM operator_conversations WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(singleton_count, 0, "migration creates no conversation");
    let violations: Vec<String> = connection
        .prepare("SELECT \"table\" FROM pragma_foreign_key_check")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(
        violations.is_empty(),
        "foreign keys stay valid: {violations:?}"
    );
    drop(connection);

    // Current and reconnect start nothing and create nothing.
    let mut client = operator_harness::OperatorClient::connect(&live);
    assert!(
        client.current().is_none(),
        "no singleton exists before Open"
    );
    drop(client);
    let mut client = operator_harness::OperatorClient::connect(&live);
    assert!(client.current().is_none());
    let fresh = client.open();
    assert_ne!(fresh.id.0, opened.id.0);
    assert_eq!(fresh.title, "Operator");
    let registry = operator_harness::Registry::of(&live);
    let ids = registry.conversation_ids();
    assert_eq!(
        ids.len(),
        3,
        "two legacy rows plus the fresh singleton: {ids:?}"
    );
    assert!(ids.contains(&opened.id.0));
    assert_eq!(
        client.session(fresh.session).pid,
        None,
        "Open starts no worker"
    );
    assert!(matches!(
        client.exchange(Request::Operator(ovrcr::protocol::OperatorRequest::Read {
            conversation: opened.id,
            after: 0,
            limit: 100,
        })),
        Response::Error { .. }
    ));
    assert!(matches!(
        live.request(Request::UnarchiveSession {
            session: opened.session,
            expected_run: ovrcr::protocol::SessionRunId(0),
        }),
        Response::Error { .. }
    ));

    let shutdown = run(&live, &["shutdown", "--kill"]);
    assert!(shutdown.status.success());
    live.join();
}
