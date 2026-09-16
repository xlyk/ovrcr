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
