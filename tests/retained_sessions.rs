#[path = "support/live.rs"]
mod live;

use live::Live;
use serde_json::Value;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn cli(live: &Live, args: &[&str]) -> Output {
    Command::new(&live.executable)
        .args(args)
        .env("OVRCR_HOME", &live.config)
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
fn codex_reopen_uses_exact_identity_without_prompt_across_two_restarts() {
    use std::os::unix::fs::PermissionsExt;
    let live = Live::idle().bounded();
    let home = live.root.path().join("codex-home");
    std::fs::create_dir(&home).unwrap();
    let environment = [("CODEX_HOME", home.as_os_str())];
    live.start_binary_env(&environment);
    live.ready("feature/codex-recovery");
    let cwd = ovrcr::config::load_registry(&live.config)
        .unwrap()
        .workspace(live::PROJECT, live::WORKSPACE)
        .unwrap()
        .path
        .clone();
    let native = live.root.path().join("codex");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\"'\"'"));
    std::fs::write(&native, format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'codex-cli 0.153.0\\n'; exit 0; fi\nexport RETAINED_CODEX_ROOT={} RETAINED_CODEX_MODE=\"$1\"\nprintf '%s\\n' \"$@\" >> \"$RETAINED_CODEX_ROOT/argv\"\nif [ -f \"$RETAINED_CODEX_ROOT/fail-resume\" ]; then printf 'NATIVE_CODEX_RESUME_FAILED\\n'; exit 23; fi\nexec {} --ignored --exact retained_codex_native_helper --nocapture\n",
        quote(live.root.path()), quote(&std::env::current_exe().unwrap())
    )).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let created = create_terminal(
        &live,
        "codex-retained",
        &[
            live.executable.to_str().unwrap(),
            "agent",
            "run",
            "codex",
            "--",
            native.to_str().unwrap(),
            "PRIVATE_CODEX_PROMPT_118",
        ],
    );
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    wait_output(&live, &id_arg, "RETAINED_CODEX_READY");
    json(&live, &["terminal", "rename", &id_arg, "Codex continuity"]);
    json(&live, &["terminal", "send", &id_arg, "--text", "startup"]);
    wait_output(&live, &id_arg, "RETAINED_CODEX_STARTUP");
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(
        row["recovery"]["conversation"],
        "01a08e5c-7480-7052-9964-9224aadebef0"
    );
    assert_eq!(row["activity"], "unknown", "startup invented activity");
    json(&live, &["terminal", "send", &id_arg, "--text", "foreign"]);
    wait_output(&live, &id_arg, "RETAINED_CODEX_FOREIGN_IGNORED");
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(
        row["recovery"]["conversation"],
        "01a08e5c-7480-7052-9964-9224aadebef0"
    );
    json(&live, &["terminal", "send", &id_arg, "--text", "attach"]);
    wait_output(&live, &id_arg, "RETAINED_CODEX_ATTACHED");
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert!(
        row["unread"].is_object(),
        "fixture never established old Ready/Unread: {row}"
    );
    let conversation = "01a08e5c-7480-7052-9964-9224aadebef0";
    // A newer neighboring conversation must never be selected instead.
    let decoy = home.join("sessions/2026/09/10/rollout-2026-09-10T20-00-00-01a08e94-0deb-76c3-a2b5-540c4874a53f.jsonl");
    std::fs::write(&decoy, "DO_NOT_READ_OR_RESUME_THIS_CONVERSATION").unwrap();
    for attempt in 1..=2 {
        json(&live, &["shutdown", "--kill"]);
        live.join();
        let rows = json(&live, &["terminal", "list"]);
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        assert_eq!(row["recovery"]["conversation"], conversation, "{row}");
        assert!(row["recovery"]["unavailable"].is_null(), "{row}");
        assert_eq!(row["title"], "Codex continuity");
        assert!(row["agent"].is_null());
        live.start_binary_env(&environment);
        if attempt == 1 {
            let history = home
                .join("sessions/2026/09/10")
                .join(format!("rollout-2026-09-10T19-46-58-{conversation}.jsonl"));
            for path in [&history, &home, &native, &cwd] {
                let displaced = path.with_extension("unavailable");
                std::fs::rename(path, &displaced).unwrap();
                let failed = cli(&live, &["terminal", "reopen", &id_arg]);
                std::fs::rename(&displaced, path).unwrap();
                assert!(
                    !failed.status.success(),
                    "missing prerequisite launched fresh"
                );
                assert_eq!(
                    std::fs::read_to_string(live.root.path().join("argv")).unwrap(),
                    "PRIVATE_CODEX_PROMPT_118\n--full-auto\n"
                );
            }
        }
        let old_run = row["run"].as_u64().unwrap();
        let reopened = json(&live, &["terminal", "reopen", &id_arg]);
        let duplicate = live.request(ovrcr::protocol::Request::ReopenSession {
            session: ovrcr::protocol::SessionId(id),
            expected_run: ovrcr::protocol::SessionRunId(old_run),
            acknowledge_stopped: false,
        });
        let ovrcr::protocol::Response::CreatedSession(duplicate) = duplicate else {
            panic!("{duplicate:?}")
        };
        assert_eq!(duplicate.run.0, reopened["run"].as_u64().unwrap());
        track_groups(&live);
        wait_output(&live, &id_arg, "RETAINED_CODEX_READY");
        wait_output(
            &live,
            &id_arg,
            &format!("RETAINED_CODEX_CWD:{}", cwd.display()),
        );
        wait_output(&live, &id_arg, "agent awaiting certified resume");
        let rows = json(&live, &["terminal", "list"]);
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        assert_eq!(row["recovery"]["conversation"], conversation);
        assert_eq!(row["recovery"]["attached"], false);
        assert!(
            row["unread"].is_null(),
            "reopen restored historical Unread: {row}"
        );
        assert_eq!(
            std::fs::read_to_string(live.root.path().join("argv")).unwrap(),
            format!(
                "PRIVATE_CODEX_PROMPT_118\n--full-auto\n{}",
                format!("resume\n{conversation}\n--full-auto\n").repeat(attempt)
            )
        );
        // Repeated restart before another provider callback must keep the reference.
    }
    // Historical Stop before certified resume identity cannot invent Ready.
    json(
        &live,
        &["terminal", "send", &id_arg, "--text", "historical-stop"],
    );
    wait_output(&live, &id_arg, "RETAINED_CODEX_HISTORICAL_IGNORED");
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(row["recovery"]["attached"], false);
    assert!(
        row["unread"].is_null(),
        "historical Stop created Unread: {row}"
    );
    // Matching SessionStart(source=resume) establishes attachment without Ready.
    json(
        &live,
        &["terminal", "send", &id_arg, "--text", "resume-attach"],
    );
    wait_output(&live, &id_arg, "RETAINED_CODEX_RESUME_ATTACHED");
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(row["recovery"]["conversation"], conversation);
    assert_eq!(row["recovery"]["attached"], true, "{row}");
    assert!(
        row["unread"].is_null(),
        "resume attach invented Ready: {row}"
    );
    // A new resumed turn reports Busy then Observed Ready/Unread once.
    json(&live, &["terminal", "send", &id_arg, "--text", "attach"]);
    wait_output(&live, &id_arg, "RETAINED_CODEX_RESUMED_READY");
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(row["activity"], "response_ready", "{row}");
    assert_eq!(row["agent"]["activity"]["quality"], "Observed", "{row}");
    assert!(
        row["unread"].is_object(),
        "resumed Ready missing Unread: {row}"
    );
    assert_eq!(row["recovery"]["attached"], true);
    json(&live, &["shutdown", "--kill"]);
    live.join();
    let database = std::fs::read(ovrcr::config::database_path(&live.config)).unwrap();
    for private in [
        "PRIVATE_CODEX_PROMPT_118",
        "RETAINED_CODEX_READY",
        "DO_NOT_READ_OR_RESUME_THIS_CONVERSATION",
    ] {
        assert!(
            !database
                .windows(private.len())
                .any(|bytes| bytes == private.as_bytes())
        );
    }
    let changed = live.root.path().join("different-codex-home");
    std::fs::create_dir(&changed).unwrap();
    live.start_binary_env(&[("CODEX_HOME", changed.as_os_str())]);
    let failed = cli(&live, &["terminal", "reopen", &id_arg]);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("configuration"));
    json(&live, &["shutdown", "--kill"]);
    live.join();
    let fail = live.root.path().join("fail-resume");
    std::fs::write(&fail, "").unwrap();
    live.start_binary_env(&environment);
    json(&live, &["terminal", "reopen", &id_arg]);
    track_groups(&live);
    wait_output(&live, &id_arg, "NATIVE_CODEX_RESUME_FAILED");
    // The native failure leaves a shell; closing it retains the native exit code.
    json(&live, &["terminal", "send", &id_arg, "--text", "exit $?"]);
    wait_phase(&live, id, "exited");
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(row["exit_code"], 23);
    assert_eq!(row["recovery"]["conversation"], conversation);
    assert!(
        row["recovery"]["failure"]
            .as_str()
            .unwrap()
            .contains("Retry")
    );
    std::fs::remove_file(fail).unwrap();
    json(&live, &["terminal", "reopen", &id_arg, "--ack-stopped"]);
    track_groups(&live);
    wait_output(&live, &id_arg, "RETAINED_CODEX_READY");
    assert_eq!(
        std::fs::read_to_string(live.root.path().join("argv")).unwrap(),
        format!(
            "PRIVATE_CODEX_PROMPT_118\n--full-auto\n{}",
            format!("resume\n{conversation}\n--full-auto\n").repeat(4)
        )
    );
    assert_eq!(
        std::fs::read_to_string(decoy).unwrap(),
        "DO_NOT_READ_OR_RESUME_THIS_CONVERSATION"
    );
}

#[test]
fn grok_managed_launch_retains_its_history_file_for_titles_without_resume() {
    use std::os::unix::fs::PermissionsExt;
    let live = Live::idle().bounded();
    let home = live.root.path().join("grok-home");
    std::fs::create_dir(&home).unwrap();
    let environment = [("GROK_HOME", home.as_os_str())];
    live.start_binary_env(&environment);
    live.ready("feature/grok-history");
    let cwd = ovrcr::config::load_registry(&live.config)
        .unwrap()
        .workspace(live::PROJECT, live::WORKSPACE)
        .unwrap()
        .path
        .clone();
    let native = live.root.path().join("grok");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\"'\"'"));
    std::fs::write(&native, format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'grok 1.0.40 (fixture) [stable]\\n'; exit 0; fi\nprintf '%s\\n' \"$@\" > {}/argv\nprintf 'RETAINED_GROK_READY\\n'\nwhile read -r line; do [ \"$line\" = quit ] && exit 0; done\n",
        quote(live.root.path())
    )).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let created = create_terminal(
        &live,
        "grok-retained",
        &[
            live.executable.to_str().unwrap(),
            "agent",
            "run",
            "grok",
            "--",
            native.to_str().unwrap(),
            "PRIVATE_GROK_PROMPT_184",
        ],
    );
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    wait_output(&live, &id_arg, "RETAINED_GROK_READY");
    let argv = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    assert_eq!(argv.len(), 3, "{argv:?}");
    assert_eq!(argv[0], "--session-id");
    assert_eq!(
        argv[2], "PRIVATE_GROK_PROMPT_184",
        "native prompt rewritten"
    );
    let conversation = argv[1].to_owned();
    assert_eq!(conversation.len(), 36);
    let find = |rows: &Value| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .cloned()
            .unwrap()
    };
    let row = find(&json(&live, &["terminal", "list"]));
    assert_eq!(row["recovery"]["conversation"], conversation, "{row}");
    assert!(
        row["recovery"]["unavailable"]
            .as_str()
            .unwrap()
            .contains("not available"),
        "a retained Grok history advertised resume: {row}"
    );
    assert_eq!(
        row["activity"], "unknown",
        "launch invented activity: {row}"
    );
    // The durable record names the one file Grok's documented store keeps for this
    // conversation in this working directory; nothing else about the launch is stored.
    let mut group = String::new();
    for byte in cwd.to_str().unwrap().bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            group.push(byte as char);
        } else {
            group.push_str(&format!("%{byte:02X}"));
        }
    }
    let history = home
        .join("sessions")
        .join(group)
        .join(&conversation)
        .join("updates.jsonl");
    let database = ovrcr::config::database_path(&live.config);
    let stored: String = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row(
        "SELECT reference FROM agent_conversations WHERE session = ?1",
        [id as i64],
        |row| row.get(0),
    )
    .unwrap();
    let stored: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["Grok"]["conversation"], conversation, "{stored}");
    assert_eq!(
        stored["Grok"]["history"],
        history.to_str().unwrap(),
        "{stored}"
    );
    assert_eq!(stored["Grok"].as_object().unwrap().len(), 2, "{stored}");
    // The identity check a reader applies: the file must belong to this conversation.
    let reference = ovrcr::protocol::GrokConversation {
        conversation: conversation.clone(),
        history: history.clone(),
    };
    assert!(
        ovrcr::grok_recovery::validate_history(&reference).is_err(),
        "no file yet"
    );
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    std::fs::write(&history, "{\"method\":\"session/update\",\"params\":{\"sessionId\":\"01a0c579-40ff-7981-a9cb-0fe920aef561\",\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"GROK_FIXTURE_TEXT_184\"}}}}\n").unwrap();
    assert!(
        ovrcr::grok_recovery::validate_history(&reference).is_err(),
        "another conversation's file passed the identity check"
    );
    std::fs::write(&history, format!("{{\"method\":\"session/update\",\"params\":{{\"sessionId\":\"{conversation}\",\"update\":{{\"sessionUpdate\":\"user_message_chunk\",\"content\":{{\"type\":\"text\",\"text\":\"GROK_FIXTURE_TEXT_184\"}}}}}}}}\n")).unwrap();
    ovrcr::grok_recovery::validate_history(&reference).unwrap();
    // The reference and its unavailability survive the server, and the record never
    // holds the prompt, the file's text, or the terminal's output.
    for attempt in 1..=2 {
        json(&live, &["shutdown", "--kill"]);
        live.join();
        let row = find(&json(&live, &["terminal", "list"]));
        assert_eq!(row["recovery"]["conversation"], conversation, "{row}");
        assert!(
            row["recovery"]["unavailable"]
                .as_str()
                .unwrap()
                .contains("not available"),
            "{row}"
        );
        assert_eq!(row["name"], "grok-retained");
        assert_eq!(row["display_name"], "grok-retained");
        assert!(
            row["title"].is_null(),
            "an Agent creation name is not a pin: {row}"
        );
        let bytes = std::fs::read(&database).unwrap();
        for private in [
            "PRIVATE_GROK_PROMPT_184",
            "GROK_FIXTURE_TEXT_184",
            "RETAINED_GROK_READY",
            "--session-id",
        ] {
            assert!(
                !bytes
                    .windows(private.len())
                    .any(|window| window == private.as_bytes()),
                "attempt {attempt}: {private} reached the retained record"
            );
        }
        live.start_binary_env(&environment);
    }
    let failed = cli(&live, &["terminal", "reopen", &id_arg]);
    assert!(
        !failed.status.success(),
        "Grok resume launched: {}",
        String::from_utf8_lossy(&failed.stdout)
    );
    assert!(String::from_utf8_lossy(&failed.stderr).contains("not available"));
    assert_eq!(
        std::fs::read_to_string(&history)
            .unwrap()
            .matches("GROK_FIXTURE_TEXT_184")
            .count(),
        1,
        "the retained file was rewritten"
    );
}

#[test]
#[ignore = "controlled native executable entered only by the retained-session fixture"]
fn retained_codex_native_helper() {
    use std::io::BufRead;
    let home = std::path::PathBuf::from(std::env::var_os("CODEX_HOME").unwrap());
    let id = "01a08e5c-7480-7052-9964-9224aadebef0";
    let history = home
        .join("sessions/2026/09/10")
        .join(format!("rollout-2026-09-10T19-46-58-{id}.jsonl"));
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    std::fs::write(&history, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cli_version\":\"0.153.0\",\"source\":\"cli\"}}}}\n")).unwrap();
    let resumed =
        std::env::var_os("RETAINED_CODEX_MODE").as_deref() == Some(std::ffi::OsStr::new("resume"));
    let session_source = if resumed { "resume" } else { "startup" };
    println!(
        "RETAINED_CODEX_CWD:{}",
        std::env::current_dir().unwrap().display()
    );
    println!("RETAINED_CODEX_READY");
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if line == "foreign" {
            // A valid envelope from the provider process itself is not the required
            // direct-child native hook. It must not bind even with the private token.
            let payload = serde_json::to_vec(&serde_json::json!({
                "hook_event_name": "SessionStart", "source": "startup",
                "session_id": "01a08e94-0deb-76c3-a2b5-540c4874a53f", "transcript_path": history,
            }))
            .unwrap();
            ovrcr::report::send_codex_hook(&payload, Instant::now() + Duration::from_secs(2))
                .unwrap();
            println!("RETAINED_CODEX_FOREIGN_IGNORED");
        }
        if line == "historical-stop" {
            // A Stop restored during resume attach must not invent Ready.
            let payload = serde_json::to_vec(&serde_json::json!({
                "hook_event_name": "Stop", "session_id": id, "turn_id": "historical",
                "transcript_path": history,
            }))
            .unwrap();
            use std::io::Write;
            let mut hook = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
                .args(["report", "codex", "--stdin"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            hook.stdin.take().unwrap().write_all(&payload).unwrap();
            assert!(hook.wait().unwrap().success());
            println!("RETAINED_CODEX_HISTORICAL_IGNORED");
        }
        if line == "attach" || line == "startup" || line == "resume-attach" {
            let (events, turn, marker) = if line == "startup" || line == "resume-attach" {
                (
                    &["SessionStart"][..],
                    "turn-1",
                    if resumed {
                        "RETAINED_CODEX_RESUME_ATTACHED"
                    } else {
                        "RETAINED_CODEX_STARTUP"
                    },
                )
            } else {
                (
                    &["UserPromptSubmit", "Stop"][..],
                    if resumed { "resumed-turn" } else { "turn-1" },
                    if resumed {
                        "RETAINED_CODEX_RESUMED_READY"
                    } else {
                        "RETAINED_CODEX_ATTACHED"
                    },
                )
            };
            for event in events {
                let payload = serde_json::to_vec(&serde_json::json!({
                    "hook_event_name": event,
                    "source": session_source,
                    "session_id": id,
                    "turn_id": turn,
                    "transcript_path": history,
                }))
                .unwrap();
                use std::io::Write;
                let mut hook = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
                    .args(["report", "codex", "--stdin"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                hook.stdin.take().unwrap().write_all(&payload).unwrap();
                assert!(hook.wait().unwrap().success());
            }
            println!("{marker}");
        }
    }
}

#[test]
fn extension_conversations_switch_and_survive_repeated_restart() {
    use std::os::unix::fs::PermissionsExt;
    let node = std::env::var_os("OVRCR_TEST_NODE_EXECUTABLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "node".into());
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
            "#!/bin/sh\nexport OVRCR_TEST_HISTORY={}\nprintf '%s\\n' \"$@\" >> {}\nexec {} {} \"$@\"\n",
            quote(&config), quote(&config.join("argv")), quote(&node), quote(&host),
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
    // The legacy spelling is an alias for reset, not application-following mode.
    json(&live, &["terminal", "rename", &id_arg, "--automatic"]);
    let reset = json(&live, &["terminal", "list"]);
    assert!(reset[0]["title"].is_null());
    assert_eq!(reset[0]["display_name"], name);
    assert_eq!(reset[0]["id"], id);
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
    let path = live.workspace_root.join(live::WORKSPACE);
    let path = path.to_str().unwrap();
    let mut args = vec![
        "terminal",
        "create",
        "--project",
        live::PROJECT,
        "--path",
        path,
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
        assert!(
            Instant::now() < deadline,
            "missing output marker {marker}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::thread::yield_now();
    }
}

#[test]
fn claude_reopen_retains_exact_conversation_before_another_callback() {
    for initial_resume in [false, true] {
        assert_claude_recovery(initial_resume);
    }
}

fn install_claude_fixture(live: &Live, native: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let helper = std::env::current_exe().unwrap();
    // Only the provider is controlled; managed launch, Reporter, server and PTY are real.
    let quote =
        |path: &std::path::Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\"'\"'"));
    std::fs::write(native, format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '2.1.268 (Claude Code)\\n'; exit 0; fi\nexport RETAINED_CLAUDE_ROOT={} RETAINED_CLAUDE_SOURCE=\"$1\" RETAINED_CLAUDE_ID=\"$2\"\nprintf '%s\\n' \"$@\" >> \"$RETAINED_CLAUDE_ROOT/argv\"\nexec {} --ignored --exact retained_claude_native_helper --nocapture\n",
        quote(live.root.path()), quote(&helper),
    )).unwrap();
    std::fs::set_permissions(native, std::fs::Permissions::from_mode(0o700)).unwrap();
}

fn assert_claude_recovery(initial_resume: bool) {
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
    install_claude_fixture(&live, &native);
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
                format!("--resume\n{conversation}\n--dangerously-skip-permissions\n")
                    .repeat(attempt)
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
            format!("--resume\n{conversation}\n--dangerously-skip-permissions\n").repeat(2)
        )
    );
}

fn prepare_interrupted_claude() -> (Live, u64, u64) {
    let live = Live::idle().bounded();
    let config_dir = live.root.path().join("claude-config");
    std::fs::create_dir(&config_dir).unwrap();
    live.start_binary_env(&[("CLAUDE_CONFIG_DIR", config_dir.as_os_str())]);
    live.ready("feature/automatic-recovery");
    let native = live.root.path().join("claude");
    install_claude_fixture(&live, &native);
    let created = create_terminal(
        &live,
        "automatic-agent",
        &[
            live.executable.to_str().unwrap(),
            "agent",
            "run",
            "claude",
            "--",
            native.to_str().unwrap(),
        ],
    );
    let id = created["id"].as_u64().unwrap();
    let run = created["run"].as_u64().unwrap();
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_READY");
    json(
        &live,
        &["terminal", "send", &id.to_string(), "--text", "attach"],
    );
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_ATTACHED");
    json(&live, &["shutdown", "--kill"]);
    live.join();
    // Simulate an interrupted run from another boot, after stopping all fixture-owned
    // processes. Discard the diagnostic produced by that deliberate fixture cleanup.
    let current = ovrcr::retained::current_boot_id().expect("native boot identity");
    let other = if current.starts_with('a') { "b" } else { "a" }.to_owned() + &current[1..];
    let database = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    assert_eq!(
        database
            .execute(
                "UPDATE retained_sessions SET stopped = 0, failure = NULL, boot_id = ?1 WHERE id = ?2",
                rusqlite::params![other, i64::try_from(id).unwrap()]
            )
            .unwrap(),
        1
    );
    (live, id, run)
}

fn restart_claude_fixture(live: &Live) {
    let config_dir = live.root.path().join("claude-config");
    live.start_binary_env(&[("CLAUDE_CONFIG_DIR", config_dir.as_os_str())]);
}

#[test]
fn displayed_interrupted_claude_recovers_through_real_dashboard_and_coalesces_duplicates() {
    use ovrcr::protocol::{
        Request, Response, ServerMessage, SessionId, SessionRunId, TerminalSize, client,
        read_frame, write_frame,
    };
    let (live, id, run) = prepare_interrupted_claude();
    let initial_args = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
    let conversation = initial_args.lines().nth(1).unwrap();
    restart_claude_fixture(&live);
    assert!(
        live.session_groups().is_empty(),
        "inventory startup launched work"
    );
    let mut stream = ovrcr::protocol::connect_server(&live.socket).unwrap();
    stream
        .set_read_timeout(Some(live::wait_deadline()))
        .unwrap();
    stream
        .set_write_timeout(Some(live::wait_deadline()))
        .unwrap();
    let Response::Hierarchy(hierarchy) =
        client::request(&mut stream, 1000, Request::DashboardHello).unwrap()
    else {
        panic!("dashboard handshake")
    };
    let mut dashboard = ovrcr::tui::Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(id));
    dashboard.request_view_at(ratatui::layout::Rect::new(0, 0, 120, 40));
    let requests = dashboard.drain_outbox();
    let automatic = requests
        .iter()
        .find(|request| matches!(request.request, Request::RecoverSession { .. }))
        .unwrap()
        .request
        .clone();
    assert_eq!(
        requests
            .iter()
            .filter(|request| matches!(request.request, Request::RecoverSession { .. }))
            .count(),
        1
    );
    for request in requests {
        write_frame(&mut stream, &request).unwrap();
    }
    // Queue duplicates before consuming any launch receipt. The synchronous connection
    // imposes the exact recover -> explicit -> recover ordering, not a sleep-based race.
    for (request_id, request) in [
        (
            1001,
            Request::ReopenSession {
                session: SessionId(id),
                expected_run: SessionRunId(run),
                acknowledge_stopped: false,
            },
        ),
        (1002, automatic.clone()),
    ] {
        write_frame(
            &mut stream,
            &ovrcr::protocol::ClientMessage {
                request_id,
                request,
            },
        )
        .unwrap();
    }
    let mut duplicate_receipts = 0;
    let deadline = Instant::now() + live::wait_deadline();
    let mut replacement_screen = false;
    loop {
        assert!(
            Instant::now() < deadline,
            "replacement view was not acknowledged"
        );
        let message: ServerMessage = read_frame(&mut stream).unwrap();
        if let ServerMessage::Response {
            request_id: 1001 | 1002,
            response,
        } = &message
        {
            let Response::CreatedSession(summary) = response else {
                panic!("duplicate: {response:?}")
            };
            assert_eq!(summary.run, SessionRunId(run + 1));
            duplicate_receipts += 1;
        }
        if matches!(&message, ServerMessage::Response { response: Response::Screen { session, run: current, .. }, .. } if *session == SessionId(id) && *current == SessionRunId(run + 1))
        {
            replacement_screen = true;
        }
        let acknowledged = replacement_screen
            && matches!(
                &message,
                ServerMessage::Response {
                    response: Response::Ok,
                    ..
                }
            );
        for request in dashboard.handle_server_message(message) {
            write_frame(&mut stream, &request).unwrap();
        }
        // The production event loop requests the current view after each message batch.
        dashboard.request_view_at(ratatui::layout::Rect::new(0, 0, 120, 40));
        for request in dashboard.drain_outbox() {
            write_frame(&mut stream, &request).unwrap();
        }
        if acknowledged {
            break;
        }
    }
    assert_eq!(duplicate_receipts, 2);
    track_groups(&live);
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_READY");
    assert_eq!(
        std::fs::read_to_string(live.root.path().join("argv")).unwrap(),
        format!("{initial_args}--resume\n{conversation}\n--dangerously-skip-permissions\n")
    );
    // Force delayed duplicates after publication, including an explicit request from another client.
    for request in [
        automatic.clone(),
        Request::ReopenSession {
            session: SessionId(id),
            expected_run: SessionRunId(run),
            acknowledge_stopped: false,
        },
    ] {
        let Response::CreatedSession(summary) = live.request(request) else {
            panic!("duplicate did not reuse run")
        };
        assert_eq!(summary.run, SessionRunId(run + 1));
    }
    // Reconnecting and redrawing the live row does not request another launch.
    let Response::Hierarchy(hierarchy) = live.request(Request::List) else {
        panic!("hierarchy")
    };
    let mut reconnected = ovrcr::tui::Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    reconnected.install_hierarchy(hierarchy);
    reconnected.install_focus(SessionId(id));
    reconnected.request_view_at(ratatui::layout::Rect::new(0, 0, 120, 40));
    assert!(
        reconnected
            .drain_outbox()
            .iter()
            .all(|r| !matches!(r.request, Request::RecoverSession { .. }))
    );
    json(&live, &["terminal", "close", &id.to_string()]);
    let response = live.request(automatic);
    assert!(!matches!(response, Response::CreatedSession(ref row) if row.phase.is_live()));
    assert!(
        live.session_groups().is_empty(),
        "late display restarted closed session"
    );
}

#[test]
fn automatic_recovery_failure_requires_explicit_retry_even_after_reconnect() {
    use ovrcr::protocol::{Request, Response, SessionId, SessionRunId};
    let (live, id, run) = prepare_interrupted_claude();
    let arguments = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
    let conversation = arguments.lines().nth(1).unwrap();
    let history = live.root.path().join(format!("{conversation}.jsonl"));
    let displaced = history.with_extension("missing");
    std::fs::rename(&history, &displaced).unwrap();
    restart_claude_fixture(&live);
    let automatic = Request::RecoverSession {
        session: SessionId(id),
        expected_run: SessionRunId(run),
    };
    assert!(matches!(
        live.request(automatic.clone()),
        Response::Error { .. }
    ));
    std::fs::rename(displaced, history).unwrap();
    assert!(matches!(
        live.request(automatic.clone()),
        Response::Error { .. }
    ));
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    assert!(
        row["recovery"]["failure"]
            .as_str()
            .unwrap()
            .contains("history")
    );
    json(&live, &["shutdown", "--kill"]);
    live.join();
    restart_claude_fixture(&live);
    assert!(matches!(live.request(automatic), Response::Error { .. }));
    assert_eq!(
        std::fs::read_to_string(live.root.path().join("argv")).unwrap(),
        arguments
    );
    assert!(matches!(
        live.request(Request::ReopenSession {
            session: SessionId(id),
            expected_run: SessionRunId(run),
            acknowledge_stopped: false
        }),
        Response::CreatedSession(_)
    ));
    track_groups(&live);
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_READY");
}

#[test]
fn automatic_recovery_rechecks_ownership_disposition_and_identity_on_server() {
    use ovrcr::protocol::{Request, Response, SessionId, SessionRunId};
    let (live, id, run) = prepare_interrupted_claude();
    let arguments = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
    let current = ovrcr::retained::current_boot_id().unwrap();
    let sql_id = i64::try_from(id).unwrap();
    let database_path = ovrcr::config::database_path(&live.config);
    let saved_boot: String = rusqlite::Connection::open(&database_path)
        .unwrap()
        .query_row(
            "SELECT boot_id FROM retained_sessions WHERE id = ?1",
            [sql_id],
            |row| row.get(0),
        )
        .unwrap();
    for case in [
        "same boot",
        "missing boot",
        "malformed boot",
        "stopped",
        "archived",
        "unarchived",
        "invalid identity",
    ] {
        let database = rusqlite::Connection::open(&database_path).unwrap();
        database.execute("UPDATE retained_sessions SET stopped = 0, disposition = 0, boot_id = ?1 WHERE id = ?2", rusqlite::params![saved_boot, sql_id]).unwrap();
        match case {
            "same boot" => {
                database
                    .execute(
                        "UPDATE retained_sessions SET boot_id = ?1 WHERE id = ?2",
                        rusqlite::params![current, sql_id],
                    )
                    .unwrap();
            }
            "missing boot" => {
                database
                    .execute(
                        "UPDATE retained_sessions SET boot_id = NULL WHERE id = ?1",
                        [sql_id],
                    )
                    .unwrap();
            }
            "malformed boot" => {
                database
                    .execute(
                        "UPDATE retained_sessions SET boot_id = 'not-a-boot' WHERE id = ?1",
                        [sql_id],
                    )
                    .unwrap();
            }
            "stopped" => {
                database
                    .execute(
                        "UPDATE retained_sessions SET stopped = 1 WHERE id = ?1",
                        [sql_id],
                    )
                    .unwrap();
            }
            "archived" => {
                database
                    .execute(
                        "UPDATE retained_sessions SET disposition = 1 WHERE id = ?1",
                        [sql_id],
                    )
                    .unwrap();
            }
            "unarchived" => {
                database
                    .execute(
                        "UPDATE retained_sessions SET disposition = 2 WHERE id = ?1",
                        [sql_id],
                    )
                    .unwrap();
            }
            "invalid identity" => {
                database
                    .execute(
                        "UPDATE agent_conversations SET invalid = 1 WHERE session = ?1",
                        [sql_id],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(database);
        restart_claude_fixture(&live);
        assert!(
            matches!(
                live.request(Request::RecoverSession {
                    session: SessionId(id),
                    expected_run: SessionRunId(run)
                }),
                Response::Error { .. }
            ),
            "{case}"
        );
        assert!(live.session_groups().is_empty(), "{case}");
        json(&live, &["shutdown", "--kill"]);
        live.join();
    }
    assert_eq!(
        std::fs::read_to_string(live.root.path().join("argv")).unwrap(),
        arguments
    );
}

#[test]
fn attached_agent_natural_exit_stays_explicit_after_a_verified_reboot() {
    use ovrcr::protocol::{Request, Response, SessionId, SessionRunId};
    let (live, id, run) = prepare_interrupted_claude();
    let database_path = ovrcr::config::database_path(&live.config);
    let sql_id = i64::try_from(id).unwrap();
    let prior_boot: String = rusqlite::Connection::open(&database_path)
        .unwrap()
        .query_row(
            "SELECT boot_id FROM retained_sessions WHERE id = ?1",
            [sql_id],
            |row| row.get(0),
        )
        .unwrap();
    restart_claude_fixture(&live);
    assert!(matches!(
        live.request(Request::RecoverSession {
            session: SessionId(id),
            expected_run: SessionRunId(run)
        }),
        Response::CreatedSession(_)
    ));
    track_groups(&live);
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_READY");
    json(
        &live,
        &["terminal", "send", &id.to_string(), "--text", "attach"],
    );
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_ATTACHED");
    json(
        &live,
        &["terminal", "send", &id.to_string(), "--text", "exit"],
    );
    wait_output(&live, &id.to_string(), "test result: ok");
    json(
        &live,
        &[
            "terminal",
            "send",
            &id.to_string(),
            "--text",
            "printf 'RESTORE_%s\\n' SHELL; exit",
        ],
    );
    wait_output(&live, &id.to_string(), "RESTORE_SHELL");
    wait_phase(&live, id, "exited");
    json(&live, &["shutdown", "--kill"]);
    live.join();
    let database = rusqlite::Connection::open(database_path).unwrap();
    database
        .execute(
            "UPDATE retained_sessions SET boot_id = ?1 WHERE id = ?2",
            rusqlite::params![prior_boot, sql_id],
        )
        .unwrap();
    drop(database);
    restart_claude_fixture(&live);
    assert!(
        matches!(
            live.request(Request::RecoverSession {
                session: SessionId(id),
                expected_run: SessionRunId(run + 1)
            }),
            Response::Error { .. }
        ),
        "natural exit was mistaken for interrupted work after reboot"
    );
    assert!(live.session_groups().is_empty());
}

#[test]
fn full_capacity_records_automatic_failure_without_retrying_when_a_slot_opens() {
    use ovrcr::protocol::{
        CreateSessionRequest, Request, Response, SessionId, SessionKind, SessionRunId,
    };
    let (live, id, run) = prepare_interrupted_claude();
    restart_claude_fixture(&live);
    let mut first = None;
    for index in 0..50 {
        let Response::CreatedSession(session) =
            live.request(Request::CreateSession(CreateSessionRequest {
                project: live::PROJECT.into(),
                workspace: live::WORKSPACE.into(),
                name: format!("capacity-{index}"),
                label: None,
                argv: vec!["/bin/sh".into()],
                kind: SessionKind::Terminal,
            }))
        else {
            panic!("slot {index} not admitted")
        };
        first.get_or_insert(session.id);
        let pgid = unsafe { libc::getpgid(session.pid.unwrap() as i32) };
        assert!(pgid > 1);
        live.own_group(pgid);
    }
    let automatic = Request::RecoverSession {
        session: SessionId(id),
        expected_run: SessionRunId(run),
    };
    assert!(matches!(
        live.request(automatic.clone()),
        Response::Error { .. }
    ));
    let rows = json(&live, &["terminal", "list"]);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    assert!(
        row["recovery"]["failure"].is_string(),
        "capacity must leave visible Retry state: {row}"
    );
    assert!(matches!(
        live.request(Request::KillSession {
            session: first.unwrap()
        }),
        Response::Ok
    ));
    assert!(
        matches!(live.request(automatic), Response::Error { .. }),
        "free capacity must not retry automatically"
    );
    assert!(matches!(
        live.request(Request::ReopenSession {
            session: SessionId(id),
            expected_run: SessionRunId(run),
            acknowledge_stopped: false
        }),
        Response::CreatedSession(_)
    ));
    track_groups(&live);
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_READY");
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
        if line == "exit" {
            break;
        }
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
        force: false,
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
    // The archived row is hidden from `terminal list`; the refusal must name it.
    let response = live.request(remove.clone());
    assert!(
        matches!(&response, Response::Error { code: ErrorCode::SessionsRemain, message }
            if message.contains(&format!("#{id} (archived)")) && message.contains("terminal remove")),
        "archived uncertainty must block removal and be named: {response:?}"
    );
    json(&live, &["terminal", "unarchive", &id.to_string()]);
    json(&live, &["terminal", "acknowledge-stopped", &id.to_string()]);
    // Drop transient exit/timing information before comparing durable snapshots.
    json(&live, &["shutdown", "--kill"]);
    live.join();
    live.start_binary();
    let worktree_path = live.workspace_root.join(live::WORKSPACE);
    let worktree = worktree_path.to_str().unwrap();
    let workspace = json(
        &live,
        &[
            "workspace",
            "get",
            "--project",
            live::PROJECT,
            "--path",
            worktree,
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
    let mut unavailable_sessions = saved.clone();
    for session in unavailable_sessions.as_array_mut().unwrap() {
        session["workspace"] = Value::String(ovrcr::git::UNAVAILABLE_CHECKOUT.into());
    }
    assert_eq!(json(&live, &["terminal", "list"]), unavailable_sessions);
    assert!(
        json(&live, &["terminal", "list", "--archived"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut unavailable_workspace = workspace.clone();
    unavailable_workspace["name"] = Value::String(ovrcr::git::UNAVAILABLE_CHECKOUT.into());
    unavailable_workspace["branch"] = Value::String(ovrcr::git::UNAVAILABLE_CHECKOUT.into());
    assert_eq!(
        json(
            &live,
            &[
                "workspace",
                "get",
                "--project",
                live::PROJECT,
                "--path",
                worktree
            ]
        ),
        unavailable_workspace
    );
    db.execute_batch("DROP TRIGGER reject_commit; DROP TABLE commit_guard;")
        .unwrap();
    json(&live, &["shutdown", "--kill"]);
    live.join();
    assert_eq!(json(&live, &["terminal", "list"]), unavailable_sessions);
}

#[test]
fn workspace_removal_protects_provider_history_inside_an_ignored_directory() {
    use ovrcr::protocol::{
        CodexConversation, ConversationReference, ErrorCode, ExtensionConversation, Request,
        Response,
    };
    for codex in [false, true] {
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
        let reference = if codex {
            ConversationReference::Codex(CodexConversation {
                conversation: "11111111-2222-3333-4444-555555555555".into(),
                executable: "/bin/codex".into(),
                history: Some(history.clone()),
                config_dir: live.root.path().to_path_buf(),
                options: vec![],
            })
        } else {
            ConversationReference::Pi(ExtensionConversation {
                conversation: "history-owner".into(),
                executable: "/bin/pi".into(),
                history: Some(history.clone()),
                config_dir: live.root.path().to_path_buf(),
                options: vec![],
            })
        };
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
            force: false,
        });
        let holder = format!("#{} (archived)", created["id"]);
        assert!(
            matches!(&response, Response::Error { code: ErrorCode::Conflict, message }
                if message.contains("provider history") && message.contains(&holder) && message.contains("terminal remove")),
            "{response:?}"
        );
        assert_eq!(
            std::fs::read_to_string(history).unwrap(),
            "provider-owned history\n"
        );
        assert!(cwd.is_dir());
    }
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
    let worktree_path = live.workspace_root.join(live::WORKSPACE);
    let worktree = worktree_path.to_str().unwrap();
    let workspace = json(
        &live,
        &[
            "workspace",
            "get",
            "--project",
            live::PROJECT,
            "--path",
            worktree,
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
            "--path",
            worktree,
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
    assert_eq!(saved["workspace"], format!("unavailable @ {path}"));
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

#[test]
fn restore_without_reporting_executes_native_picker_in_shell() {
    assert_native_picker_in_shell("/bin/sh");
}

#[test]
#[cfg(target_os = "macos")]
fn restore_native_picker_in_zsh_after_loading_user_configuration() {
    assert_native_picker_in_shell("/bin/zsh");
}

fn assert_native_picker_in_shell(shell: &str) {
    use std::os::unix::fs::PermissionsExt;
    let live = Live::idle().bounded();
    let bin = live.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let native = bin.join("claude");
    std::fs::write(&native, "#!/bin/sh\nif [ \"$1\" = --resume ]; then printf 'NATIVE_%s\\n' PICKER; else printf 'INITIAL_%s\\n' EXIT; fi\n").unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let home = live.root.path().join("shell-home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "export RESTORE_SHELL_CONFIG=loaded\n").unwrap();
    live.start_binary_env(&[
        ("PATH", std::ffi::OsStr::new(&path)),
        ("SHELL", std::ffi::OsStr::new(shell)),
        ("HOME", home.as_os_str()),
        ("ZDOTDIR", home.as_os_str()),
    ]);
    live.ready("feature/native-picker");
    let created = create_terminal(&live, "picker", &[native.to_str().unwrap()]);
    let id = created["id"].as_u64().unwrap().to_string();
    wait_output(&live, &id, "INITIAL_EXIT");
    wait_phase(&live, created["id"].as_u64().unwrap(), "exited");
    let restored = json(&live, &["terminal", "reopen", &id, "--ack-stopped"]);
    assert_eq!(restored["id"], created["id"]);
    wait_output(&live, &id, "NATIVE_PICKER");
    // The agent exits, leaving the interactive terminal usable.
    json(
        &live,
        &[
            "terminal",
            "send",
            &id,
            "--text",
            "printf 'SHELL_%s\\n' REMAINS",
        ],
    );
    wait_output(&live, &id, "SHELL_REMAINS");
    if shell.ends_with("zsh") {
        json(
            &live,
            &[
                "terminal",
                "send",
                &id,
                "--text",
                "printf 'CONFIG_%s\\n' \"$RESTORE_SHELL_CONFIG\"",
            ],
        );
        wait_output(&live, &id, "CONFIG_loaded");
    }
    track_groups(&live);
}

#[test]
fn exact_resume_preserves_long_arguments_before_shell_startup() {
    use std::os::unix::fs::PermissionsExt;
    let (live, id, _) = prepare_interrupted_claude();
    let database = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    let sql_id = i64::try_from(id).unwrap();
    let encoded: String = database
        .query_row(
            "SELECT reference FROM agent_conversations WHERE session = ?1",
            [sql_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut reference: ovrcr::protocol::ConversationReference =
        serde_json::from_str(&encoded).unwrap();
    let long = "x".repeat(1800);
    let ovrcr::protocol::ConversationReference::Claude(claude) = &mut reference else {
        panic!("Claude reference")
    };
    claude.options = vec!["--model".into(), long.clone()];
    database
        .execute(
            "UPDATE agent_conversations SET reference = ?1 WHERE session = ?2",
            rusqlite::params![serde_json::to_string(&reference).unwrap(), sql_id],
        )
        .unwrap();
    drop(database);
    let gate = live.root.path().join("shell-gate");
    let c_gate = std::ffi::CString::new(gate.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_gate.as_ptr(), 0o600) }, 0);
    let shell = live.root.path().join("delayed-shell");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\ncat '{}' >/dev/null\nexec /bin/sh\n",
            gate.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = live.root.path().join("claude-config");
    live.start_binary_env(&[
        ("SHELL", shell.as_os_str()),
        ("CLAUDE_CONFIG_DIR", config.as_os_str()),
    ]);
    json(&live, &["terminal", "reopen", &id.to_string()]);
    // Receipt and terminal reads remain available while the shell blocks startup.
    wait_output(&live, &id.to_string(), "eval \"$OVRCR_RESTORE_COMMAND\"");
    std::fs::write(&gate, b"ready\n").unwrap();
    track_groups(&live);
    wait_output(&live, &id.to_string(), "RETAINED_CLAUDE_READY");
    let argv = std::fs::read_to_string(live.root.path().join("argv")).unwrap();
    assert!(
        argv.ends_with(&format!(
            "--model\n{long}\n--dangerously-skip-permissions\n"
        )),
        "long resume argument changed"
    );
}
