use ovrcr::protocol::{
    ClientMessage, Request, Response, ServerMessage, connect_server, read_frame, write_frame,
};
use ovrcr::session::{SessionId, SessionPhase, SessionSummary};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

struct Fixture {
    root: tempfile::TempDir,
    pgids: Vec<i32>,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            pgids: Vec::new(),
        };
        std::fs::create_dir(fixture.root.path().join("repo")).unwrap();
        std::fs::create_dir(fixture.root.path().join("workspaces")).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "OVRCR Tests"],
            vec!["config", "user.email", "tests@example.invalid"],
            vec!["commit", "--allow-empty", "-m", "fixture"],
        ] {
            fixture.git(&args);
        }
        fixture.ok(&[
            "project",
            "add",
            "fixture",
            fixture.root.path().join("repo").to_str().unwrap(),
            "--workspace-root",
            fixture.root.path().join("workspaces").to_str().unwrap(),
        ]);
        fixture.ok(&[
            "workspace",
            "create",
            "--project",
            "fixture",
            "--name",
            "demo",
            "--new-branch",
            "feature/demo",
            "--base",
            "main",
        ]);
        fixture
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.root.path().join("repo"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(args)
            .env("OVRCR_CONFIG", self.root.path().join("config.toml"))
            .env("OVRCR_SOCKET", self.root.path().join("server.sock"))
            .env("SHELL", "/bin/sh")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty(), "unexpected stderr: {:?}", out.stderr);
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut cli_args = vec!["--json"];
        cli_args.extend_from_slice(args);
        serde_json::from_str(&self.ok(&cli_args)).unwrap()
    }

    fn sessions(&self) -> Vec<SessionSummary> {
        let mut stream = connect_server(self.root.path().join("server.sock")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 1,
                request: Request::List,
            },
        )
        .unwrap();
        let ServerMessage::Response {
            response: Response::Hierarchy(snapshot),
            ..
        } = read_frame(&mut stream).unwrap()
        else {
            panic!("expected hierarchy")
        };
        snapshot
            .projects
            .into_iter()
            .flat_map(|p| p.workspaces)
            .flat_map(|w| w.sessions)
            .collect()
    }

    fn capture(&mut self) {
        for summary in self.sessions() {
            if let Some(pid) = summary.pid {
                let pgid = unsafe { libc::getpgid(pid as i32) };
                if pgid > 1 && !self.pgids.contains(&pgid) {
                    self.pgids.push(pgid);
                }
            }
        }
    }

    fn wait_text(&self, id: &str, marker: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let text = self.ok(&["terminal", "read", id]);
            if text.contains(marker) {
                return text;
            }
            assert!(Instant::now() < deadline, "missing {marker:?}: {text:?}");
            std::thread::yield_now();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let out = self.run(&["shutdown", "--kill"]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.root.path().join("server.sock").exists() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let clean = out.status.success()
            && !self.root.path().join("server.sock").exists()
            && self
                .pgids
                .iter()
                .all(|pgid| unsafe { libc::kill(-*pgid, 0) } == -1);
        if !clean {
            eprintln!("cleanup failed: {}", String::from_utf8_lossy(&out.stderr));
            assert!(
                std::thread::panicking(),
                "fixture server or process group remained"
            );
        }
    }
}

#[test]
fn terminal_cli_drives_real_session_and_preserves_workspace_removal_guards() {
    let mut fixture = Fixture::new();
    fixture.capture();
    let id = fixture.ok(&["terminals", "create", "--project", "fixture", "--workspace", "demo", "--name", "reader", "--", "/bin/sh", "-c", "stty -echo; printf '\\033[?1049h\\033[2J\\033[HREADY\\n'; IFS= read -r value; printf '\\033[2J\\033[Hfirst\\nACK:%s\\n' \"$value\""]);
    let id = id.trim().to_owned();
    let numeric_id: u64 = id.parse().unwrap();
    fixture.capture();
    assert!(fixture.ok(&["projects", "get", "fixture"]).contains("repo"));
    assert!(
        fixture
            .ok(&[
                "workspaces",
                "get",
                "--project",
                "fixture",
                "--name",
                "demo"
            ])
            .contains("feature/demo")
    );
    let project = fixture.json(&["project", "get", "fixture"]);
    assert_eq!(project["workspace_count"], 1);
    assert_eq!(
        project["repo"],
        fixture
            .root
            .path()
            .join("repo")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    let workspace = fixture.json(&["workspace", "get", "--project", "fixture", "--name", "demo"]);
    assert_eq!(workspace["branch"], "feature/demo");
    assert_eq!(workspace["terminal_count"], 2);
    let terminals = fixture.json(&[
        "terminal",
        "list",
        "--project",
        "fixture",
        "--workspace",
        "demo",
    ]);
    let terminals = terminals.as_array().unwrap();
    assert_eq!(terminals.len(), 2);
    assert!(terminals[0]["id"].as_u64().unwrap() < terminals[1]["id"].as_u64().unwrap());
    let hierarchy = fixture.json(&["list"]);
    assert_eq!(
        hierarchy[0]["workspaces"][0]["terminals"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let refused_shutdown = fixture.run(&["shutdown", "--json"]);
    assert_eq!(refused_shutdown.status.code(), Some(1));
    assert!(refused_shutdown.stdout.is_empty());
    let refusal: serde_json::Value = serde_json::from_slice(&refused_shutdown.stderr).unwrap();
    assert_eq!(refusal["error"]["code"], "SessionsRemain");
    fixture.wait_text(&id, "READY");
    fixture.ok(&["terminal", "send", &id, "--text", "hello ", "--no-submit"]);
    fixture.ok(&["terminal", "send", &id, "--text", "λ"]);
    fixture.wait_text(&id, "ACK:hello λ");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if fixture.sessions().iter().any(|s| {
            s.id == SessionId(numeric_id) && matches!(s.phase, SessionPhase::Exited { .. })
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "terminal did not exit");
        std::thread::yield_now();
    }
    assert_eq!(
        fixture
            .ok(&["terminal", "read", &id, "--max-lines", "1"])
            .trim_end(),
        "ACK:hello λ"
    );
    let screen = fixture.json(&["terminal", "read", &id]);
    assert_eq!(screen["id"], numeric_id);
    assert_eq!(screen["rows"], 40);
    assert_eq!(screen["cols"], 120);
    assert!(screen["text"].as_str().unwrap().contains("ACK:hello λ"));
    let retained = fixture.json(&["terminal", "list", "--project", "fixture"]);
    let retained = retained
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == numeric_id)
        .unwrap();
    assert_eq!(retained["phase"], "exited");
    assert_eq!(retained["exit_code"], 0);
    assert!(retained["pid"].is_null());
    let exited_send = fixture.run(&["terminal", "send", &id, "--text", "again", "--json"]);
    assert_eq!(exited_send.status.code(), Some(1));
    assert!(exited_send.stdout.is_empty());
    assert!(String::from_utf8_lossy(&exited_send.stderr).contains("Conflict"));
    let occupied = fixture.run(&[
        "workspace",
        "delete",
        "--project",
        "fixture",
        "--name",
        "demo",
    ]);
    assert!(!occupied.status.success());
    fixture.ok(&["terminal", "close", &id]);
    assert!(
        !fixture
            .sessions()
            .iter()
            .any(|s| s.id == SessionId(numeric_id))
    );
    let active = fixture.json(&[
        "terminal",
        "create",
        "--project",
        "fixture",
        "--workspace",
        "demo",
        "--name",
        "group",
        "--",
        "/bin/sh",
        "-c",
        "sleep 300 & wait",
    ]);
    assert_eq!(active["phase"], "running");
    let active = active["id"].as_u64().unwrap().to_string();
    fixture.capture();
    let pid = fixture
        .sessions()
        .into_iter()
        .find(|s| s.id.0.to_string() == active)
        .unwrap()
        .pid
        .unwrap() as i32;
    assert_eq!(
        fixture.json(&["terminal", "close", &active]),
        serde_json::json!({"ok": true})
    );
    assert_eq!(unsafe { libc::kill(-pid, 0) }, -1, "closed group remains");
    for session in fixture.sessions() {
        fixture.ok(&["terminal", "close", &session.id.0.to_string()]);
    }
    let worktree = fixture.root.path().join("workspaces/demo");
    std::fs::write(worktree.join("dirty"), "preserve me").unwrap();
    let dirty = fixture.run(&[
        "workspace",
        "delete",
        "--project",
        "fixture",
        "--name",
        "demo",
        "--json",
    ]);
    assert_eq!(dirty.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&dirty.stderr).contains("DirtyWorktree"));
    assert_eq!(
        std::fs::read_to_string(worktree.join("dirty")).unwrap(),
        "preserve me"
    );
    std::fs::remove_file(worktree.join("dirty")).unwrap();
    fixture.ok(&[
        "workspace",
        "delete",
        "--project",
        "fixture",
        "--name",
        "demo",
    ]);
    assert!(!worktree.exists());
    fixture.git(&["show-ref", "--verify", "refs/heads/feature/demo"]);
    fixture.ok(&["project", "delete", "fixture"]);
}

#[test]
fn pause_resume_resource_cli_preserves_input_and_close_contract() {
    let mut fixture = Fixture::new();
    let id = fixture
        .ok(&[
            "terminals",
            "create",
            "--project",
            "fixture",
            "--workspace",
            "demo",
            "--name",
            "pause-resume",
            "--",
            "/bin/sh",
            "-c",
            "printf 'READY\\n'; IFS= read -r value; printf 'ACK:%s\\n' \"$value\"; sleep 30",
        ])
        .trim()
        .to_owned();
    let numeric_id: u64 = id.parse().unwrap();
    let created = fixture
        .sessions()
        .into_iter()
        .find(|session| session.id == SessionId(numeric_id))
        .expect("created session must be listed");
    let original_pid = created
        .pid
        .expect("created session must report its managed PID") as i32;
    assert!(original_pid > 1, "created session has no valid managed PID");
    let original_pgid = unsafe { libc::getpgid(original_pid) };
    assert!(original_pgid > 1, "managed PID has no valid original PGID");
    fixture.capture();
    fixture.wait_text(&id, "READY");

    assert_eq!(
        fixture.json(&["pause", &id]),
        serde_json::json!({"ok": true})
    );
    let paused = fixture.json(&["terminal", "list"]);
    let paused = paused
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["id"] == numeric_id)
        .unwrap();
    assert_eq!(paused["phase"], "paused");
    assert!(paused["exit_code"].is_null());
    assert!(paused["exit_signal"].is_null());

    let refused = fixture.run(&["terminal", "send", &id, "--text", "blocked", "--json"]);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&refused.stderr).unwrap()["error"]["code"],
        "Conflict"
    );

    assert_eq!(
        fixture.json(&["resume", &id]),
        serde_json::json!({"ok": true})
    );
    fixture.ok(&["terminal", "send", &id, "--text", "hello"]);
    fixture.wait_text(&id, "ACK:hello");
    assert_eq!(
        fixture.json(&["pause", &id]),
        serde_json::json!({"ok": true})
    );
    assert_eq!(
        fixture.json(&["terminal", "close", &id]),
        serde_json::json!({"ok": true})
    );
    let record_removed = !fixture
        .sessions()
        .iter()
        .any(|session| session.id == SessionId(numeric_id));
    assert!(record_removed, "closed session record remains");
    let group_absent = wait_group_absent(original_pgid, Duration::from_secs(2));
    eprintln!(
        "paused close cleanup: original_pid={original_pid} original_pgid={original_pgid} record_removed={record_removed} original_pgid_absent={group_absent} before_fixture_teardown=true"
    );
    assert!(
        group_absent,
        "original managed PGID {original_pgid} remains after paused close"
    );
}

fn wait_group_absent(pgid: libc::pid_t, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let result = unsafe { libc::kill(-pgid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::yield_now();
    }
}

#[test]
fn agent_hook_resource_inventory_reports_activity() {
    let mut fixture = Fixture::new();
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let id = fixture
        .ok(&[
            "terminal",
            "create",
            "--project",
            "fixture",
            "--workspace",
            "demo",
            "--name",
            "hook-activity",
            "--",
            "/bin/sh",
            "-c",
            r#""$1" --json report activity --state busy --sequence 1 && printf HOOK_DONE; while IFS= read -r line; do :; done"#,
            "hook-child",
            bin,
        ])
        .trim()
        .to_owned();
    fixture.capture();
    fixture.wait_text(&id, "HOOK_DONE");

    let records = fixture.json(&[
        "terminal",
        "list",
        "--project",
        "fixture",
        "--workspace",
        "demo",
    ]);
    let record = records
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id.parse::<u64>().unwrap())
        .expect("managed hook terminal is listed");
    assert_eq!(record["activity"], "busy");
    for forbidden in ["capability", "hook_socket", "hook_token"] {
        assert!(!record.as_object().unwrap().contains_key(forbidden));
    }
}

#[test]
fn context_resource_inventory_matches_inspection() {
    let mut fixture = Fixture::new();
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let marker = fixture.root.path().join("context-marker");
    let first_gate = fixture.root.path().join("context-first");
    let second_gate = fixture.root.path().join("context-second");
    let final_gate = fixture.root.path().join("context-final");
    let id = fixture
        .ok(&[
            "terminal",
            "create",
            "--project",
            "fixture",
            "--workspace",
            "demo",
            "--name",
            "context-inventory",
            "--",
            "/bin/sh",
            "-c",
            r#"
printf READY > "$2"
while [ ! -e "$3" ]; do sleep 0.01; done
printf '%s' '{"source":"generic","model":"inventory-model","conversation":"inventory-conversation","used_tokens":25,"capacity_tokens":100}' | "$1" report context --stdin-json
printf CONTEXT1 >> "$2"
while [ ! -e "$4" ]; do sleep 0.01; done
printf '%s' '{"source":"generic","model":"replacement-model","conversation":"replacement-conversation","used_tokens":40}' | "$1" report context --stdin-json
printf CONTEXT2 >> "$2"
printf WAITING >> "$2"
while [ ! -e "$5" ]; do sleep 0.01; done
"#,
            "context-inventory",
            bin,
            marker.to_str().unwrap(),
            first_gate.to_str().unwrap(),
            second_gate.to_str().unwrap(),
            final_gate.to_str().unwrap(),
        ])
        .trim()
        .to_owned();
    let id_number: u64 = id.parse().unwrap();
    let original_pid = fixture
        .sessions()
        .into_iter()
        .find(|session| session.id == SessionId(id_number))
        .and_then(|session| session.pid)
        .expect("managed context terminal PID") as libc::pid_t;
    let original_pgid = unsafe { libc::getpgid(original_pid) };
    fixture.capture();
    let ready_deadline = Instant::now() + Duration::from_secs(3);
    while (!marker.exists()
        || !std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .contains("READY"))
        && Instant::now() < ready_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "READY");

    let unknown = fixture.json(&[
        "terminal",
        "list",
        "--project",
        "fixture",
        "--workspace",
        "demo",
    ]);
    let unknown_record = unknown
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id_number)
        .expect("unknown context terminal is listed");
    assert!(unknown_record["context_usage"].is_null());
    assert!(unknown_record["context_stale"].is_null());
    let unknown_inspection: serde_json::Value =
        serde_json::from_str(&fixture.ok(&["session", "context", &id])).unwrap();
    assert_eq!(unknown_inspection["session"], id_number);
    assert!(unknown_inspection["context_usage"].is_null());
    assert!(unknown_inspection["stale"].is_null());

    std::fs::write(&first_gate, b"go").unwrap();
    let first_deadline = Instant::now() + Duration::from_secs(3);
    while (!marker.exists()
        || !std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .contains("CONTEXT1"))
        && Instant::now() < first_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        std::fs::read_to_string(&marker)
            .unwrap()
            .contains("CONTEXT1")
    );
    let first_inventory = fixture.json(&[
        "terminal",
        "list",
        "--project",
        "fixture",
        "--workspace",
        "demo",
    ]);
    let first_record = first_inventory
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id_number)
        .unwrap();
    let first_inspection: serde_json::Value =
        serde_json::from_str(&fixture.ok(&["session", "context", &id])).unwrap();
    assert_eq!(
        first_record["context_usage"],
        first_inspection["context_usage"]
    );
    assert_eq!(first_record["context_stale"], first_inspection["stale"]);
    assert_eq!(
        first_record["context_usage"]["report"]["capacity_tokens"],
        100
    );

    std::fs::write(&second_gate, b"go").unwrap();
    let second_deadline = Instant::now() + Duration::from_secs(3);
    while (!marker.exists()
        || !std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .contains("CONTEXT2WAITING"))
        && Instant::now() < second_deadline
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        std::fs::read_to_string(&marker)
            .unwrap()
            .contains("CONTEXT2WAITING")
    );
    let replacement: serde_json::Value =
        serde_json::from_str(&fixture.ok(&["session", "context", &id])).unwrap();
    assert_eq!(replacement["context_usage"]["report"]["used_tokens"], 40);
    assert!(replacement["context_usage"]["report"]["capacity_tokens"].is_null());
    assert_eq!(replacement["stale"], false);
    let replacement_inventory = fixture.json(&["terminal", "list"]);
    let replacement_record = replacement_inventory
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id_number)
        .unwrap();
    assert_eq!(
        replacement_record["context_usage"],
        replacement["context_usage"]
    );
    assert_eq!(replacement_record["context_stale"], replacement["stale"]);
    assert_eq!(replacement_record["context_stale"], false);
    assert!(replacement_record["context_usage"]["report"]["capacity_tokens"].is_null());

    std::fs::write(&final_gate, b"release").unwrap();
    let exited_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < exited_deadline
        && !fixture.sessions().into_iter().any(|session| {
            session.id == SessionId(id_number)
                && matches!(session.phase, SessionPhase::Exited { .. })
        })
    {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let stale_inspection: serde_json::Value =
        serde_json::from_str(&fixture.ok(&["session", "context", &id])).unwrap();
    assert_eq!(stale_inspection["stale"], true);
    let stale_inventory = fixture.json(&["terminal", "list"]);
    let stale_record = stale_inventory
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id_number)
        .unwrap();
    assert_eq!(
        stale_record["context_usage"],
        stale_inspection["context_usage"]
    );
    assert_eq!(stale_record["context_stale"], true);
    let group_absent = wait_group_absent(original_pgid, Duration::from_secs(2));
    eprintln!(
        "context inventory cleanup: original_pgid={original_pgid} original_pgid_absent={group_absent} before_fixture_teardown=true"
    );
    assert!(
        group_absent,
        "managed process group {original_pgid} remained"
    );
}

#[test]
fn remove_project_reports_workspaces_remain() {
    let mut fixture = Fixture::new();
    fixture.capture();
    let output = fixture.run(&["--json", "project", "remove", "fixture"]);
    assert_eq!(output.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "WorkspacesRemain");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("workspaces remain")
    );
    drop(fixture);
}
