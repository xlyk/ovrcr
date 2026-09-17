#[path = "support/live.rs"]
mod live;

use live::Live;
use serde_json::Value;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn cli(live: &Live, args: &[&str]) -> Output {
    Command::new(&live.executable)
        .args(args)
        .env("OVRCR_CONFIG", &live.config)
        .env("OVRCR_SOCKET", &live.socket)
        .env("SHELL", "/bin/sh")
        .output()
        .unwrap()
}

fn json(live: &Live, args: &[&str]) -> Value {
    let output = cli(live, &[&["--json"], args].concat());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn extension_conversations_switch_and_survive_repeated_restart() {
    use std::os::unix::fs::PermissionsExt;
    for provider in ["pi", "omp"] {
        let live = Live::idle().bounded();
        let config = live.root.path().join("provider-config");
        std::fs::create_dir(&config).unwrap();
        // Runner/user profiles must not change this fixture's recovery capability.
        // Inherited XDG overrides make OMP recovery intentionally unavailable.
        let empty = std::ffi::OsStr::new("");
        let environment = [
            ("PI_CODING_AGENT_DIR", config.as_os_str()),
            ("OMP_PROFILE", empty),
            ("PI_PROFILE", empty),
            ("PI_CONFIG_DIR", empty),
            ("XDG_CONFIG_HOME", empty),
            ("XDG_DATA_HOME", empty),
            ("XDG_STATE_HOME", empty),
            ("XDG_CACHE_HOME", empty),
        ];
        live.start_binary_env(&environment);
        live.ready("feature/extension-recovery");
        let native = live.root.path().join(provider);
        let host =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
        let quote = |path: &std::path::Path| {
            format!("'{}'", path.to_str().unwrap().replace('\'', "'\"'\"'"))
        };
        std::fs::write(&native, format!(
            "#!/bin/sh\nexport OVRCR_TEST_HISTORY={}\nprintf '%s\\n' \"$@\" >> {}\nexec node {} \"$@\"\n",
            quote(&config), quote(&config.join("argv")), quote(&host),
        )).unwrap();
        std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Capture the initial run's capability before the managed launcher
        // deliberately removes it from the native provider's environment.
        let launcher = live.root.path().join("ovrcr");
        let capability_file = live.root.path().join("initial-capability");
        std::fs::write(
            &launcher,
            format!(
                "#!/bin/sh\numask 077\nprintf '%s' \"$OVRCR_HOOK_TOKEN\" > {}\nexec {} \"$@\"\n",
                quote(&capability_file),
                quote(&live.executable)
            ),
        )
        .unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let created = create_terminal(
            &live,
            "retained-extension",
            &[
                launcher.to_str().unwrap(),
                "agent",
                "run",
                provider,
                "--",
                native.to_str().unwrap(),
                "PRIVATE_PROMPT_119",
            ],
        );
        let id = created["id"].as_u64().unwrap();
        let id_arg = id.to_string();
        wait_output(&live, &id_arg, "PI_NATIVE_READY");
        json(
            &live,
            &["terminal", "rename", &id_arg, "Conversation continuity"],
        );
        for (index, conversation) in ["session-a", "session-b", "session-a"].iter().enumerate() {
            let command = if index == 0 {
                "session_start"
            } else if provider == "pi" {
                "session_replace"
            } else {
                "session_switch"
            };
            json(
                &live,
                &[
                    "terminal",
                    "send",
                    &id_arg,
                    "--text",
                    &format!("{command}:{conversation}"),
                ],
            );
            wait_output(&live, &id_arg, &format!("PI_CALLBACK={index}"));
            let rows = json(&live, &["terminal", "list"]);
            let row = rows
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            assert_eq!(
                row["recovery"]["conversation"], *conversation,
                "{provider}: {row}"
            );
            assert!(row["recovery"]["unavailable"].is_null(), "{row}");
        }
        // Retire a producer, then deliver its late conversation change through
        // the same native helper and authenticated receiver.
        let replace = if provider == "pi" {
            "session_replace:session-a"
        } else {
            "producer_replace:session-a"
        };
        for (index, command) in [replace, "retired_start:session-b"].iter().enumerate() {
            json(&live, &["terminal", "send", &id_arg, "--text", command]);
            wait_output(&live, &id_arg, &format!("PI_CALLBACK={}", index + 3));
            let rows = json(&live, &["terminal", "list"]);
            let row = rows
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            assert_eq!(
                row["recovery"]["conversation"], "session-a",
                "retired producer changed {provider} recovery"
            );
        }
        let database =
            rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
        database.execute_batch("CREATE TRIGGER reject_extension_retention BEFORE INSERT ON agent_conversations BEGIN SELECT RAISE(FAIL, 'fixture rejects retention'); END;").unwrap();
        json(
            &live,
            &["terminal", "send", &id_arg, "--text", "agent_start"],
        );
        wait_output(&live, &id_arg, "PI_CALLBACK=5");
        let rows = json(&live, &["terminal", "list"]);
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert_eq!(
            row["agent"]["activity"]["state"], "Busy",
            "recovery failure suppressed {provider} activity"
        );
        database
            .execute_batch("DROP TRIGGER reject_extension_retention")
            .unwrap();
        drop(database);
        json(
            &live,
            &[
                "terminal",
                "send",
                &id_arg,
                "--text",
                "session_shutdown:quit",
            ],
        );
        wait_output(&live, &id_arg, "PI_CALLBACK=6");
        let deadline = Instant::now() + live::wait_deadline();
        let epoch = loop {
            let rows = json(&live, &["terminal", "list"]);
            let row = rows
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            if row["agent"]["health"]["state"] == "Unavailable" {
                break row["agent_epoch"].as_u64().unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "reporter did not release its lease"
            );
            std::thread::yield_now();
        };
        // Reserve a real supervisor on run N and bind B without committing its
        // retention. Deliver that delayed write after run N+1 has launched.
        use ovrcr::protocol::{
            AgentCommand, AgentOperationResult, AgentProvider, AgentSecret, ConversationReference,
            ExtensionConversation, Request, ReserveAgent, Response, SessionId, SupervisorAuth,
            SupervisorRequest, client, exchange_preamble,
        };
        let secret = std::fs::read_to_string(&capability_file).unwrap();
        let mut capability = [0u8; 32];
        assert_eq!(secret.len(), 64);
        for (index, byte) in capability.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&secret[index * 2..index * 2 + 2], 16).unwrap();
        }
        let mut old_watch = std::os::unix::net::UnixStream::connect(&live.socket).unwrap();
        old_watch
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        old_watch
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        exchange_preamble(&mut old_watch).unwrap();
        let reserved = client::request(
            &mut old_watch,
            1,
            Request::ReserveAgent(ReserveAgent {
                session: SessionId(id),
                capability: AgentSecret(capability),
                operation: "old-run-reserve".into(),
                expected_epoch: epoch,
                invocation: "delayed-old-run".into(),
                provider: AgentProvider::from_name(provider).unwrap(),
            }),
        )
        .unwrap();
        let Response::AgentOperation(AgentOperationResult::Reserved(reservation)) = reserved else {
            panic!("reserve failed: {reserved:?}")
        };
        let auth = SupervisorAuth {
            session: SessionId(id),
            lease: reservation.lease,
        };
        let bound = client::request(
            &mut old_watch,
            2,
            Request::Supervisor(SupervisorRequest {
                auth: auth.clone(),
                operation: "old-run-bind".into(),
                command: AgentCommand::Bind {
                    expected_binding: None,
                    conversation: "session-b".into(),
                },
            }),
        )
        .unwrap();
        let Response::AgentOperation(AgentOperationResult::Bound(binding)) = bound else {
            panic!("bind failed: {bound:?}")
        };
        let reference = ExtensionConversation {
            conversation: "session-b".into(),
            executable: native.clone(),
            history: Some(config.join("session-b.jsonl")),
            config_dir: config.clone(),
            options: vec![],
        };
        let delayed_retention = Request::Supervisor(SupervisorRequest {
            auth,
            operation: "delayed-old-run-retention".into(),
            command: AgentCommand::RetainConversation {
                binding,
                reference: Box::new(if provider == "pi" {
                    ConversationReference::Pi(reference)
                } else {
                    ConversationReference::Omp(reference)
                }),
            },
        });
        let cwd = ovrcr::config::load_registry(&live.config)
            .unwrap()
            .workspace(live::PROJECT, live::WORKSPACE)
            .unwrap()
            .path
            .clone();
        for attempt in 1..=2 {
            json(&live, &["shutdown", "--kill"]);
            live.join();
            live.start_binary_env(&environment);
            if attempt == 1 {
                for (path, diagnostic) in [
                    (config.join("session-a.jsonl"), "history"),
                    (native.clone(), "executable"),
                    (config.clone(), "configuration"),
                    (cwd.clone(), "directory"),
                ] {
                    let before = std::fs::read_to_string(config.join("argv")).unwrap();
                    let displaced = path.with_extension("unavailable");
                    std::fs::rename(&path, &displaced).unwrap();
                    let result = cli(&live, &["terminal", "reopen", &id_arg]);
                    std::fs::rename(&displaced, &path).unwrap();
                    assert!(
                        !result.status.success(),
                        "missing {diagnostic} launched {provider}"
                    );
                    assert!(
                        String::from_utf8_lossy(&result.stderr).contains(diagnostic),
                        "{}",
                        String::from_utf8_lossy(&result.stderr)
                    );
                    assert_eq!(
                        std::fs::read_to_string(config.join("argv")).unwrap(),
                        before
                    );
                }
                let history = config.join("session-a.jsonl");
                let original = std::fs::read(&history).unwrap();
                std::fs::write(&history, "{\"type\":\"session\",\"id\":\"different\"}\n").unwrap();
                let result = cli(&live, &["terminal", "reopen", &id_arg]);
                std::fs::write(&history, original).unwrap();
                assert!(!result.status.success());
                assert!(String::from_utf8_lossy(&result.stderr).contains("identity"));
            }
            json(&live, &["terminal", "reopen", &id_arg]);
            track_groups(&live);
            wait_output(&live, &id_arg, "PI_NATIVE_READY");
            assert!(
                matches!(
                    live.request(delayed_retention.clone()),
                    Response::Error { .. }
                ),
                "old run changed retained identity"
            );
            let rows = json(&live, &["terminal", "list"]);
            let row = rows
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            assert_eq!(row["recovery"]["conversation"], "session-a");
            assert_eq!(row["recovery"]["attached"], false);
            assert_eq!(row["title"], "Conversation continuity");
            let args = std::fs::read_to_string(config.join("argv")).unwrap();
            assert_eq!(args.matches("PRIVATE_PROMPT_119").count(), 1);
            let flag = if provider == "pi" {
                "--session"
            } else {
                "--resume"
            };
            assert_eq!(
                args.matches(&format!(
                    "{flag}\n{}\n",
                    config.join("session-a.jsonl").display()
                ))
                .count(),
                attempt
            );
        }
        json(
            &live,
            &[
                "terminal",
                "send",
                &id_arg,
                "--text",
                "session_ephemeral:ephemeral-b",
            ],
        );
        wait_output(&live, &id_arg, "PI_CALLBACK=0");
        json(&live, &["shutdown", "--kill"]);
        live.join();
        let rows = json(&live, &["terminal", "list"]);
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert_eq!(row["recovery"]["conversation"], "ephemeral-b");
        assert!(
            row["recovery"]["unavailable"]
                .as_str()
                .unwrap()
                .contains("no native history")
        );
        let database = std::fs::read(ovrcr::config::database_path(&live.config)).unwrap();
        assert!(
            !database
                .windows(b"PRIVATE_PROMPT_119".len())
                .any(|bytes| bytes == b"PRIVATE_PROMPT_119")
        );
        live.start_binary_env(&environment);
        let failure = cli(&live, &["terminal", "reopen", &id_arg]);
        assert!(
            !failure.status.success(),
            "ephemeral conversation reopened old A"
        );
    }
}

#[test]
fn retained_rows_preserve_identity_and_title_without_restoring_output_or_live_state() {
    let live = Live::binary();
    live.ready("feature/retained");
    for group in live.session_groups() {
        live.own_group(group);
    }
    let initial = json(&live, &["terminal", "list"]);
    let id = initial[0]["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    let name = initial[0]["name"].clone();
    json(&live, &["terminal", "rename", &id_arg, "Keep this title"]);
    json(
        &live,
        &[
            "terminal",
            "send",
            &id_arg,
            "--text",
            r"printf '\033]2;Application title\007'; printf 'PRIVATE_%s\n' RETAINED_114",
        ],
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = cli(&live, &["terminal", "read", &id_arg]);
        assert!(output.status.success());
        if String::from_utf8_lossy(&output.stdout).contains("PRIVATE_RETAINED_114") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned shell did not produce marker"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        json(&live, &["terminal", "list"])[0]["title"],
        "Keep this title"
    );
    let stopped = cli(&live, &["shutdown", "--kill"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    live.join();

    let offline = json(&live, &["terminal", "list"]);
    assert_eq!(
        offline[0]["id"], id,
        "offline inventory lost the retained row"
    );
    assert_eq!(offline[0]["name"], name);
    assert_eq!(offline[0]["title"], "Keep this title");
    assert_eq!(offline[0]["phase"], "stopped");
    assert!(offline[0]["pid"].is_null());
    assert!(offline[0]["started_unix_ms"].is_null());
    assert!(offline[0]["agent"].is_null());
    assert!(offline[0]["unread"].is_null());
    assert!(!live.socket.exists(), "inventory alone started a server");

    let database = std::fs::read(ovrcr::config::database_path(&live.config)).unwrap();
    assert!(
        !database
            .windows(b"PRIVATE_RETAINED_114".len())
            .any(|bytes| bytes == b"PRIVATE_RETAINED_114")
    );
    live.start_binary();
    let restored = json(&live, &["terminal", "list"]);
    assert_eq!(restored, offline);
    json(&live, &["terminal", "rename", &id_arg, "--automatic"]);
    assert_eq!(
        json(&live, &["terminal", "list"])[0]["title"],
        "Application title"
    );
    assert!(
        live.session_groups().is_empty(),
        "inventory restoration launched work"
    );
}

fn track_groups(live: &Live) {
    for group in live.session_groups() {
        live.own_group(group);
    }
}

fn wait_phase(live: &Live, id: u64, phase: &str) -> Value {
    let deadline = Instant::now() + live::wait_deadline();
    loop {
        let rows = json(live, &["terminal", "list"]);
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        if row["phase"] == phase {
            return row.clone();
        }
        assert!(
            Instant::now() < deadline,
            "session did not become {phase}: {row}"
        );
        std::thread::yield_now();
    }
}

fn create_terminal(live: &Live, name: &str, argv: &[&str]) -> Value {
    let mut args = vec![
        "terminal",
        "create",
        "--project",
        live::PROJECT,
        "--workspace",
        live::WORKSPACE,
        "--name",
        name,
        "--",
    ];
    args.extend_from_slice(argv);
    let result = json(live, &args);
    track_groups(live);
    result
}

fn wait_output(live: &Live, id: &str, marker: &str) {
    let deadline = Instant::now() + live::wait_deadline();
    loop {
        let output = cli(live, &["terminal", "read", id]);
        assert!(output.status.success());
        if String::from_utf8_lossy(&output.stdout).contains(marker) {
            return;
        }
        assert!(Instant::now() < deadline, "missing output marker {marker}");
        std::thread::yield_now();
    }
}

#[test]
fn claude_reopen_retains_exact_conversation_before_another_callback() {
    for initial_resume in [false, true] {
        assert_claude_recovery(initial_resume);
    }
}

fn assert_claude_recovery(initial_resume: bool) {
    use std::os::unix::fs::PermissionsExt;
    let live = Live::idle().bounded();
    let config_dir = live.root.path().join("claude-config");
    std::fs::create_dir(&config_dir).unwrap();
    let environment = [("CLAUDE_CONFIG_DIR", config_dir.as_os_str())];
    live.start_binary_env(&environment);
    live.ready("feature/claude-reopen");
    let native = live.root.path().join("claude");
    let cwd = ovrcr::config::load_registry(&live.config)
        .unwrap()
        .workspace(live::PROJECT, live::WORKSPACE)
        .unwrap()
        .path
        .clone();
    let helper = std::env::current_exe().unwrap();
    // The script is an executable fixture only; the production managed launcher,
    // Reporter, server and PTY all run normally.
    let quote =
        |path: &std::path::Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\"'\"'"));
    std::fs::write(&native, format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '2.1.268 (Claude Code)\\n'; exit 0; fi\nexport RETAINED_CLAUDE_ROOT={} RETAINED_CLAUDE_SOURCE=\"$1\" RETAINED_CLAUDE_ID=\"$2\"\nprintf '%s\\n' \"$@\" >> \"$RETAINED_CLAUDE_ROOT/argv\"\nexec {} --ignored --exact retained_claude_native_helper --nocapture\n",
        quote(live.root.path()), quote(&helper),
    )).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut launch = vec![
        live.executable.to_str().unwrap(),
        "agent",
        "run",
        "claude",
        "--",
        native.to_str().unwrap(),
    ];
    if initial_resume {
        launch.extend(["--resume", "5ebc5f9b-54b5-4928-9955-dc81c23743dd"]);
    } else {
        launch.push("PRIVATE_RETAINED_PROMPT_116");
    }
    let created = create_terminal(&live, "claude-retained", &launch);
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    wait_output(&live, &id_arg, "RETAINED_CLAUDE_READY");
    json(&live, &["terminal", "rename", &id_arg, "Claude continuity"]);
    let database = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    database.execute_batch("CREATE TRIGGER reject_retention BEFORE INSERT ON agent_conversations WHEN NEW.invalid = 0 BEGIN SELECT RAISE(FAIL, 'fixture rejects retention'); END;").unwrap();
    json(&live, &["terminal", "send", &id_arg, "--text", "attach"]);
    wait_output(&live, &id_arg, "RETAINED_CLAUDE_REPORT_UNAVAILABLE");
    database
        .execute_batch("DROP TRIGGER reject_retention")
        .unwrap();
    json(&live, &["terminal", "send", &id_arg, "--text", "attach"]);
    wait_output(&live, &id_arg, "RETAINED_CLAUDE_ATTACHED");
    let arguments = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
    let conversation = arguments.lines().nth(1).unwrap().to_owned();
    assert!(arguments.starts_with(if initial_resume {
        "--resume\n"
    } else {
        "--session-id\n"
    }));
    for attempt in 1..=2 {
        json(&live, &["shutdown", "--kill"]);
        live.join();
        if initial_resume && attempt == 1 {
            restore_legacy_claude_schema(&live);
        }
        let database = std::fs::read(ovrcr::config::database_path(&live.config)).unwrap();
        assert!(
            !database
                .windows(b"PRIVATE_RETAINED_PROMPT_116".len())
                .any(|bytes| bytes == b"PRIVATE_RETAINED_PROMPT_116")
        );
        let rows = json(&live, &["terminal", "list"]);
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert!(row["recovery"]["unavailable"].is_null(), "{row}");
        assert_eq!(row["recovery"]["conversation"], conversation);
        assert_eq!(row["recovery"]["attached"], false);
        assert_eq!(row["title"], "Claude continuity");
        live.start_binary_env(&environment);
        if attempt == 1 {
            for (path, diagnostic) in [
                (
                    live.root.path().join(format!("{conversation}.jsonl")),
                    "history",
                ),
                (native.clone(), "executable"),
                (config_dir.clone(), "configuration"),
                (cwd.clone(), "directory"),
            ] {
                let displaced = path.with_extension("temporarily-unavailable");
                std::fs::rename(&path, &displaced).unwrap();
                let failure = cli(&live, &["terminal", "reopen", &id_arg]);
                std::fs::rename(&displaced, &path).unwrap();
                assert!(
                    !failure.status.success(),
                    "missing {diagnostic} launched fresh"
                );
                assert!(
                    String::from_utf8_lossy(&failure.stderr).contains(diagnostic),
                    "{}",
                    String::from_utf8_lossy(&failure.stderr)
                );
                assert_eq!(
                    std::fs::read_to_string(live.root.path().join("argv")).unwrap(),
                    arguments
                );
            }
        }
        json(&live, &["terminal", "reopen", &id_arg]);
        track_groups(&live);
        wait_output(&live, &id_arg, "RETAINED_CLAUDE_READY");
        let rows = json(&live, &["terminal", "list"]);
        let resumed = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert_eq!(resumed["recovery"]["attached"], false);
        assert_eq!(resumed["recovery"]["conversation"], conversation);
        assert!(resumed["agent"].is_null());
        let actual = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
        assert_eq!(
            actual,
            format!(
                "{arguments}{}",
                format!("--resume\n{conversation}\n").repeat(attempt)
            )
        );
        // No new callback or prompt: the second restart must still be eligible.
    }
    // A clear before the replacement's first callback invalidates the retained
    // reference too; an absent callback by itself did not invalidate it above.
    let database = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    database.execute_batch("CREATE TRIGGER reject_invalidation BEFORE INSERT ON agent_conversations WHEN NEW.invalid = 1 BEGIN SELECT RAISE(FAIL, 'fixture rejects invalidation'); END;").unwrap();
    json(&live, &["terminal", "send", &id_arg, "--text", "clear"]);
    wait_output(&live, &id_arg, "RETAINED_CLAUDE_REPORT_UNAVAILABLE");
    database
        .execute_batch("DROP TRIGGER reject_invalidation")
        .unwrap();
    json(&live, &["terminal", "send", &id_arg, "--text", "clear"]);
    wait_output(&live, &id_arg, "RETAINED_CLAUDE_CLEARED");
    json(&live, &["shutdown", "--kill"]);
    live.join();
    if initial_resume {
        restore_legacy_claude_schema(&live);
        let offline = json(&live, &["terminal", "list"]);
        let row = offline
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert!(
            row["recovery"]["unavailable"]
                .as_str()
                .unwrap()
                .contains("unsupported")
        );
    }
    live.start_binary_env(&environment);
    let failure = cli(&live, &["terminal", "reopen", &id_arg]);
    assert!(
        !failure.status.success(),
        "clear reopened stale Claude identity"
    );
    assert!(String::from_utf8_lossy(&failure.stderr).contains("unsupported"));
    let actual = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
    assert_eq!(
        actual,
        format!(
            "{arguments}{}",
            format!("--resume\n{conversation}\n").repeat(2)
        )
    );
}

// Reproduce the metadata schema written by the previous PR revision. Both
// offline reads and the next production startup must preserve its exact state.
fn restore_legacy_claude_schema(live: &Live) {
    let mut database =
        rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    let transaction = database.transaction().unwrap();
    let (id, encoded): (i64, String) = transaction
        .query_row(
            "SELECT session, reference FROM agent_conversations",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let tagged: Value = serde_json::from_str(&encoded).unwrap();
    assert!(tagged["Claude"].is_object());
    transaction
        .execute(
            "UPDATE agent_conversations SET reference = ?1 WHERE session = ?2",
            rusqlite::params![serde_json::to_string(&tagged["Claude"]).unwrap(), id],
        )
        .unwrap();
    transaction.execute_batch("ALTER TABLE agent_conversations RENAME TO claude_conversations; ALTER TABLE retained_sessions DROP COLUMN disposition; PRAGMA user_version = 3;").unwrap();
    transaction.commit().unwrap();
}

#[test]
#[ignore = "controlled native executable entered only by the retained-session fixture"]
fn retained_claude_native_helper() {
    use std::io::BufRead;
    let root = std::path::PathBuf::from(std::env::var_os("RETAINED_CLAUDE_ROOT").unwrap());
    let id = std::env::var("RETAINED_CLAUDE_ID").unwrap();
    let source = if std::env::var("RETAINED_CLAUDE_SOURCE").unwrap() == "--resume" {
        "resume"
    } else {
        "startup"
    };
    let history = root.join(format!("{id}.jsonl"));
    std::fs::write(&history, "").unwrap();
    println!("RETAINED_CLAUDE_READY");
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if line == "attach" || line == "clear" {
            let payload = serde_json::to_vec(&serde_json::json!({
                "hook_event_name": "SessionStart", "source": if line == "clear" { "clear" } else { source },
                "session_id": id, "transcript_path": history,
            })).unwrap();
            if ovrcr::report::send_claude_hook(&payload, Instant::now() + Duration::from_secs(1))
                .is_err()
            {
                println!("RETAINED_CLAUDE_REPORT_UNAVAILABLE");
                continue;
            }
            println!(
                "{}",
                if line == "clear" {
                    "RETAINED_CLAUDE_CLEARED"
                } else {
                    "RETAINED_CLAUDE_ATTACHED"
                }
            );
        }
    }
}

#[test]
fn reopen_keeps_the_row_starts_a_fresh_shell_and_coalesces_delayed_duplicates() {
    use ovrcr::protocol::{Request, Response, SessionId, SessionPhase, SessionRunId};
    let live = Live::binary();
    live.ready("feature/reopen");
    let marker = live.root.path().join("original-command");
    let created = create_terminal(
        &live,
        "stable",
        &[
            "/bin/sh",
            "-c",
            "printf 'OLD_%s\\n' OUTPUT_114; printf 'once\\n' >> \"$1\"; exit 0",
            "owned-command",
            marker.to_str().unwrap(),
        ],
    );
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    let old_run = created["run"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    let reopened = json(&live, &["terminal", "reopen", &id_arg, "--ack-stopped"]);
    track_groups(&live);
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["name"], created["name"]);
    assert_eq!(reopened["title"], created["title"]);
    assert_eq!(reopened["run"], old_run + 1);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "once\n");
    json(
        &live,
        &[
            "terminal",
            "send",
            &id_arg,
            "--text",
            "printf 'NEW_%s\\n' SHELL_114",
        ],
    );
    let deadline = Instant::now() + live::wait_deadline();
    loop {
        let output = cli(&live, &["terminal", "read", &id_arg]);
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            !text.contains("OLD_OUTPUT_114"),
            "old run screen leaked into replacement"
        );
        if text.contains("NEW_SHELL_114") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fresh shell did not execute input: {text}"
        );
        std::thread::yield_now();
    }
    json(&live, &["terminal", "send", &id_arg, "--text", "exit"]);
    wait_phase(&live, id, "exited");

    // Both delayed requests name the same retired run. Even after its successor exited,
    // replaying that request must not start a third process.
    let duplicate = Request::ReopenSession {
        session: SessionId(id),
        expected_run: SessionRunId(old_run),
        acknowledge_stopped: false,
    };
    let gate = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            gate.wait();
            live.request(duplicate.clone())
        });
        let second = scope.spawn(|| {
            gate.wait();
            live.request(duplicate.clone())
        });
        gate.wait();
        for response in [first.join().unwrap(), second.join().unwrap()] {
            let Response::CreatedSession(summary) = response else {
                panic!("duplicate failed: {response:?}")
            };
            assert_eq!(summary.id, SessionId(id));
            assert_eq!(summary.run, SessionRunId(old_run + 1));
            assert!(matches!(summary.phase, SessionPhase::Exited { .. }));
            assert!(summary.pid.is_none());
        }
    });
    let explicit_next = json(&live, &["terminal", "reopen", &id_arg, "--ack-stopped"]);
    track_groups(&live);
    assert_eq!(explicit_next["run"], old_run + 2);
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "once\n");
}

#[test]
fn workspace_removal_blocks_live_and_uncertain_rows_and_preserves_records_on_failures() {
    use ovrcr::protocol::{ErrorCode, Request, Response};
    let live = Live::binary();
    live.ready("feature/removal-failures");
    track_groups(&live);
    let remove = Request::RemoveWorkspace {
        project: live::PROJECT.into(),
        name: live::WORKSPACE.into(),
    };
    assert!(matches!(
        live.request(remove.clone()),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    for row in json(&live, &["terminal", "list"]).as_array().unwrap() {
        json(&live, &["terminal", "kill", &row["id"].to_string()]);
    }
    let exited = create_terminal(&live, "uncertain", &["/bin/sh", "-c", "exit 0"]);
    let id = exited["id"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    json(&live, &["terminal", "close", &id.to_string()]);
    assert!(
        matches!(
            live.request(remove.clone()),
            Response::Error {
                code: ErrorCode::SessionsRemain,
                ..
            }
        ),
        "archived uncertainty must block removal too"
    );
    json(&live, &["terminal", "unarchive", &id.to_string()]);
    json(&live, &["terminal", "acknowledge-stopped", &id.to_string()]);
    // Drop transient exit/timing information before comparing durable snapshots.
    json(&live, &["shutdown", "--kill"]);
    live.join();
    live.start_binary();
    let workspace = json(
        &live,
        &[
            "workspace",
            "get",
            "--project",
            live::PROJECT,
            "--name",
            live::WORKSPACE,
        ],
    );
    let path = std::path::Path::new(workspace["path"].as_str().unwrap());
    let saved = json(&live, &["terminal", "list"]);
    let dirty = path.join("dirty");
    std::fs::write(&dirty, "keep me").unwrap();
    assert!(matches!(
        live.request(remove.clone()),
        Response::Error {
            code: ErrorCode::DirtyWorktree,
            ..
        }
    ));
    assert_eq!(std::fs::read_to_string(&dirty).unwrap(), "keep me");
    std::fs::remove_file(dirty).unwrap();
    live::git(&live.repo, &["worktree", "lock", path.to_str().unwrap()]);
    assert!(matches!(
        live.request(remove.clone()),
        Response::Error { .. }
    ));
    assert!(path.exists());
    assert_eq!(json(&live, &["terminal", "list"]), saved);
    live::git(&live.repo, &["worktree", "unlock", path.to_str().unwrap()]);
    let db = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_archive BEFORE UPDATE OF disposition ON retained_sessions BEGIN SELECT RAISE(FAIL, 'fixture archive failure'); END;").unwrap();
    assert!(matches!(
        live.request(remove.clone()),
        Response::Error { .. }
    ));
    assert!(
        path.exists(),
        "storage preflight must precede destructive Git work"
    );
    assert_eq!(json(&live, &["terminal", "list"]), saved);
    db.execute_batch("DROP TRIGGER reject_archive;
        CREATE TABLE commit_guard (id INTEGER REFERENCES retained_sessions(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER reject_commit AFTER UPDATE OF disposition ON retained_sessions BEGIN INSERT INTO commit_guard VALUES (-1); END;").unwrap();
    let response = live.request(remove);
    assert!(
        matches!(&response, Response::Error { code: ErrorCode::PartialFailure, message } if message.contains("metadata was not committed")),
        "{response:?}"
    );
    assert!(!path.exists(), "commit failure occurs after Git removal");
    assert_eq!(json(&live, &["terminal", "list"]), saved);
    assert!(
        json(&live, &["terminal", "list", "--archived"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        json(
            &live,
            &[
                "workspace",
                "get",
                "--project",
                live::PROJECT,
                "--name",
                live::WORKSPACE
            ]
        ),
        workspace
    );
    db.execute_batch("DROP TRIGGER reject_commit; DROP TABLE commit_guard;")
        .unwrap();
    json(&live, &["shutdown", "--kill"]);
    live.join();
    assert_eq!(json(&live, &["terminal", "list"]), saved);
}

#[test]
fn workspace_removal_protects_provider_history_inside_an_ignored_directory() {
    use ovrcr::protocol::{
        ConversationReference, ErrorCode, ExtensionConversation, Request, Response,
    };
    let live = Live::binary();
    live.ready("feature/embedded-history");
    track_groups(&live);
    let created = create_terminal(&live, "history-owner", &["/bin/sh"]);
    for row in json(&live, &["terminal", "list"]).as_array().unwrap() {
        json(&live, &["terminal", "kill", &row["id"].to_string()]);
    }
    let cwd = std::path::Path::new(created["cwd"].as_str().unwrap());
    let history = cwd.join("ignored-history.jsonl");
    std::fs::write(&history, "provider-owned history\n").unwrap();
    let exclude = live.repo.join(".git/info/exclude");
    std::fs::write(exclude, "ignored-history.jsonl\n").unwrap();
    json(&live, &["shutdown", "--kill"]);
    live.join();
    // Seed the exact persisted provider reference. Capture itself is covered by
    // extension_conversations_switch_and_survive_repeated_restart; removal must
    // protect history restored from storage, including archives and ignored files.
    let reference = ConversationReference::Pi(ExtensionConversation {
        conversation: "history-owner".into(),
        executable: "/bin/pi".into(),
        history: Some(history.clone()),
        config_dir: live.root.path().to_path_buf(),
        options: vec![],
    });
    let db = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    db.execute(
        "INSERT INTO agent_conversations (session, reference, invalid) VALUES (?1, ?2, 0)",
        rusqlite::params![
            created["id"].as_i64().unwrap(),
            serde_json::to_string(&reference).unwrap()
        ],
    )
    .unwrap();
    drop(db);
    live.start_binary();
    json(&live, &["terminal", "close", &created["id"].to_string()]);
    let response = live.request(Request::RemoveWorkspace {
        project: live::PROJECT.into(),
        name: live::WORKSPACE.into(),
    });
    assert!(
        matches!(&response, Response::Error { code: ErrorCode::Conflict, message } if message.contains("provider history")),
        "{response:?}"
    );
    assert_eq!(
        std::fs::read_to_string(history).unwrap(),
        "provider-owned history\n"
    );
    assert!(cwd.is_dir());
}

#[test]
fn workspace_removal_retains_archive_context_through_project_removal_and_restart() {
    let live = Live::binary();
    live.ready("feature/remove-retained");
    track_groups(&live);
    let created = create_terminal(&live, "retained-work", &["/bin/sh"]);
    let id = created["id"].as_u64().unwrap();
    let arg = id.to_string();
    let history = live.root.path().join("provider-history.jsonl");
    std::fs::write(&history, "provider-owned history\n").unwrap();
    json(&live, &["terminal", "rename", &arg, "Original context"]);
    // Controlled stops leave active retained records, rather than deleting them.
    for row in json(&live, &["terminal", "list"]).as_array().unwrap() {
        json(&live, &["terminal", "kill", &row["id"].to_string()]);
    }
    let workspace = json(
        &live,
        &[
            "workspace",
            "get",
            "--project",
            live::PROJECT,
            "--name",
            live::WORKSPACE,
        ],
    );
    let path = workspace["path"].as_str().unwrap();
    json(
        &live,
        &[
            "workspace",
            "remove",
            "--project",
            live::PROJECT,
            "--name",
            live::WORKSPACE,
        ],
    );
    assert!(!std::path::Path::new(path).exists());
    json(&live, &["project", "remove", live::PROJECT]);
    let archived = json(&live, &["terminal", "list", "--archived"]);
    let saved = archived
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(saved["title"], "Original context");
    assert_eq!(saved["project"], live::PROJECT);
    assert_eq!(saved["workspace"], live::WORKSPACE);
    assert_eq!(saved["cwd"], path);
    json(&live, &["shutdown", "--kill"]);
    live.join();
    assert_eq!(json(&live, &["terminal", "list", "--archived"]), archived);
    live.start_binary();
    json(&live, &["terminal", "unarchive", &arg]);
    assert!(live.session_groups().is_empty());
    let output = cli(&live, &["terminal", "reopen", &arg]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Recorded working directory is unavailable")
    );
    let row = wait_phase(&live, id, "stopped");
    assert_eq!(row["cwd"], path);
    let ovrcr::protocol::Response::Hierarchy(hierarchy) =
        live.request(ovrcr::protocol::Request::List)
    else {
        panic!("expected hierarchy")
    };
    assert!(
        hierarchy
            .projects
            .iter()
            .flat_map(|p| &p.workspaces)
            .flat_map(|w| &w.sessions)
            .any(|row| row.id.0 == id),
        "unarchived orphan must remain visible in Dashboard"
    );
    assert!(row["recovery"]["failure"].as_str().unwrap().contains(path));
    assert_eq!(
        std::fs::read_to_string(history).unwrap(),
        "provider-owned history\n"
    );
}

#[test]
fn close_exited_row_moves_it_to_the_archive_without_process_acknowledgement() {
    let live = Live::binary();
    live.ready("feature/archive-exited");
    let created = create_terminal(&live, "finished", &["/bin/sh", "-c", "exit 0"]);
    let id = created["id"].as_u64().unwrap();
    let arg = id.to_string();
    wait_phase(&live, id, "exited");
    json(&live, &["terminal", "close", &arg]);
    let active = json(&live, &["terminal", "list"]);
    assert!(active.as_array().unwrap().iter().all(|row| row["id"] != id));
    let archive = json(&live, &["terminal", "list", "--archived"]);
    assert_eq!(archive.as_array().unwrap().len(), 1);
    assert_eq!(archive[0]["id"], id);
    assert_eq!(archive[0]["name"], "finished");
    assert_eq!(archive[0]["archived"], true);
    assert!(archive[0]["pid"].is_null());
    assert_eq!(
        archive[0]["recovery"]["requires_ack"], true,
        "filing a row must not certify background processes stopped"
    );
}

#[test]
fn archive_round_trip_survives_restart_and_rejects_old_recovery_without_deleting_history() {
    use ovrcr::protocol::{Request, Response, SessionId, SessionRunId};
    let live = Live::binary();
    live.ready("feature/archive-round-trip");
    track_groups(&live);
    let history = live.root.path().join("provider-history.jsonl");
    std::fs::write(&history, "provider-owned conversation\n").unwrap();
    let created = create_terminal(&live, "round-trip", &["/bin/sh"]);
    let id = created["id"].as_u64().unwrap();
    let arg = id.to_string();
    let old_run = SessionRunId(created["run"].as_u64().unwrap());
    let pid = created["pid"].as_u64().unwrap() as i32;
    let pgid = unsafe { libc::getpgid(pid) };
    assert!(pgid > 1);
    live.own_group(pgid);
    json(&live, &["terminal", "rename", &arg, "Remember archive"]);
    json(&live, &["terminal", "close", &arg]);
    assert!(
        !live::group_exists(pgid),
        "close left the owned group running"
    );
    let saved = json(&live, &["terminal", "list", "--archived"])[0].clone();
    assert!(
        matches!(
            live.request(Request::RemoveSession {
                session: SessionId(id)
            }),
            Response::Error { .. }
        ),
        "unfenced legacy removal deleted an archive"
    );
    assert_eq!(saved["title"], "Remember archive");
    let stale = Request::ReopenSession {
        session: SessionId(id),
        expected_run: old_run,
        acknowledge_stopped: true,
    };
    assert!(matches!(
        live.request(stale.clone()),
        Response::Error { .. }
    ));
    json(&live, &["shutdown", "--kill"]);
    live.join();
    assert_eq!(json(&live, &["terminal", "list", "--archived"])[0], saved);
    assert!(
        !live.socket.exists(),
        "offline archive inspection launched server"
    );
    live.start_binary();
    assert_eq!(json(&live, &["terminal", "list", "--archived"])[0], saved);
    assert!(matches!(
        live.request(stale.clone()),
        Response::Error { .. }
    ));
    json(&live, &["terminal", "unarchive", &arg]);
    assert!(
        live.session_groups().is_empty(),
        "unarchive launched a process"
    );
    let restored = json(&live, &["terminal", "list"])
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap()
        .clone();
    assert_eq!(restored["phase"], "stopped");
    assert_eq!(restored["title"], "Remember archive");
    assert_eq!(restored["archived"], false);
    json(&live, &["terminal", "rename", &arg, "--automatic"]);
    let rows = json(&live, &["terminal", "list"]);
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()["title"]
            .is_null()
    );
    json(
        &live,
        &["terminal", "rename", &arg, "Renamed while stopped"],
    );
    let rows = json(&live, &["terminal", "list"]);
    assert_eq!(
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()["title"],
        "Renamed while stopped"
    );
    assert!(
        matches!(live.request(stale), Response::Error { .. }),
        "pre-archive recovery crossed unarchive"
    );
    assert!(matches!(
        live.request(Request::CloseTerminal {
            session: SessionId(id),
            expected_run: old_run
        }),
        Response::Error { .. }
    ));
    json(&live, &["shutdown", "--kill"]);
    live.join();
    live.start_binary();
    assert!(
        json(&live, &["terminal", "list", "--archived"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    json(&live, &["terminal", "close", &arg]);
    json(&live, &["terminal", "remove", &arg]);
    json(&live, &["shutdown", "--kill"]);
    live.join();
    live.start_binary();
    assert!(
        json(&live, &["terminal", "list", "--archived"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        json(&live, &["terminal", "list"])
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"] != id)
    );
    assert_eq!(
        std::fs::read_to_string(history).unwrap(),
        "provider-owned conversation\n"
    );
}

#[test]
fn real_process_discovery_failure_does_not_archive_the_record() {
    use std::os::unix::fs::PermissionsExt;
    let mut live = Live::idle();
    let bin = live.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let fail = live.root.path().join("fail-discovery");
    let ps = bin.join("ps");
    std::fs::write(
        &ps,
        format!(
            "#!/bin/sh\nif [ -e '{}' ]; then exit 1; fi\nexec /bin/ps \"$@\"\n",
            fail.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&ps, std::fs::Permissions::from_mode(0o700)).unwrap();
    let wrapper = live.root.path().join("server-wrapper");
    std::fs::write(&wrapper, format!("#!/bin/sh\nif [ \"$1\" = server ]; then export PATH='{}':$PATH; fi\nexec '{}' \"$@\"\n", bin.display(), live.executable.display())).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    live.executable = wrapper;
    live.start_binary();
    let live = live.bounded();
    live.ready("feature/archive-stop-failure");
    track_groups(&live);
    let created = create_terminal(&live, "failed-close", &["/bin/sh"]);
    let id = created["id"].as_u64().unwrap();
    std::fs::write(&fail, "fail").unwrap();
    let output = cli(&live, &["terminal", "close", &id.to_string()]);
    std::fs::remove_file(&fail).unwrap();
    assert!(
        !output.status.success(),
        "real process discovery failure reported success"
    );
    assert!(
        json(&live, &["terminal", "list", "--archived"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        json(&live, &["terminal", "list"])
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == id),
        "failed close lost the actionable row"
    );
}
