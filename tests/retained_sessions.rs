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
