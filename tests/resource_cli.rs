#[path = "support/live.rs"]
mod live;

use live::Live;
use ovrcr::protocol::{Request, Response};
use ovrcr::session::{SessionId, SessionPhase, SessionSummary};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

/// A live server hosted by the compiled `ovrcr` binary, plus the CLI helpers
/// this suite drives it with.
struct Fixture(Live);

impl std::ops::Deref for Fixture {
    type Target = Live;

    fn deref(&self) -> &Live {
        &self.0
    }
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self(Live::binary());
        fixture.ok(&[
            "project",
            "add",
            "fixture",
            fixture.repo.to_str().unwrap(),
            "--workspace-root",
            fixture.workspace_root.to_str().unwrap(),
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

    fn run(&self, args: &[&str]) -> Output {
        Command::new(&self.executable)
            .args(args)
            .env("OVRCR_CONFIG", &self.config)
            .env("OVRCR_SOCKET", &self.socket)
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
        let Response::Hierarchy(snapshot) = self.request(Request::List) else {
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
        for pgid in self.session_groups() {
            self.own_group(pgid);
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
        let stopped = live::wait_for_absent(&self.socket, Duration::from_secs(5));
        let reaped = self.join_within(Duration::from_secs(5)).is_finished();
        let clean = out.status.success()
            && stopped
            && reaped
            && self
                .owned_groups()
                .into_iter()
                .all(|pgid| !live::group_exists(pgid));
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
    let worktree = fixture.workspace_root.join("demo");
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
    live::git(
        &fixture.repo,
        &["show-ref", "--verify", "refs/heads/feature/demo"],
    );
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
    let group_absent = live::wait_group_absent(original_pgid, Duration::from_secs(2));
    eprintln!(
        "paused close cleanup: original_pid={original_pid} original_pgid={original_pgid} record_removed={record_removed} original_pgid_absent={group_absent} before_fixture_teardown=true"
    );
    assert!(
        group_absent,
        "original managed PGID {original_pgid} remains after paused close"
    );
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
    let group_absent = live::wait_group_absent(original_pgid, Duration::from_secs(2));
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

#[test]
fn managed_usage_inspection_preserves_scope_unknowns_and_component_ages() {
    use ovrcr::protocol::*;
    use serde_json::json;
    use std::os::unix::net::UnixListener;
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("usage.sock");
    let measurement = |value| json!({"value":value,"source":"fixture"});
    let mut agent = json!({
        "binding":{"provider":"Claude","invocation":"invocation-a","conversation":"conversation-a","generation":1},
        "activity":{"state":"Idle","quality":"Observed","turn":"prompt-a"},
        "metrics":{
            "sample":{
                "model":"fixture-model",
                "context":measurement(json!({"used_tokens":null,"capacity_tokens":100,"quality":"Observed"})),
                "usage":measurement(json!({"scope":"Conversation","coverage":"Partial","input_tokens":12,"output_tokens":3,"cache_read_tokens":4,"cache_write_tokens":2,"reasoning_output_tokens":null})),
                "cost":measurement(json!(null))
            },
            "received_unix_ms":4000,"context_received_unix_ms":1000,"usage_received_unix_ms":3000,"cost_received_unix_ms":2000
        },
        "health":{"state":"Unavailable","reason":"source_completion_unverified"},
        "activity_revision":2,"metrics_revision":3,"health_revision":1,
        "input_requests":[],"input_revision":0
    });
    for scenario in 0..6 {
        let case = scenario / 2;
        let inventory = scenario % 2 == 1;
        if case == 1 {
            agent["metrics"]["sample"]["cost"]["value"] =
                json!({"usd_ticks":0,"kind":"Estimated","scope":"Invocation"});
        }
        let expected = if case == 2 {
            json!(null)
        } else {
            agent.clone()
        };
        let snapshot: HierarchySnapshot = serde_json::from_value(json!({"projects":[{"name":"fixture","workspaces":[{"project":"fixture","name":"demo","path":"/fixture","sessions":[{
            "id":7,"project":"fixture","workspace":"demo","name":"native","label":"claude","pid":null,"started_unix_ms":1,"phase":{"Exited":{"code":0,"signal":null}},"activity":"Idle","agent":expected,"agent_epoch":1,"context_usage":null
        }]}]}]})).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        exchange_preamble(&mut stream).unwrap();
                        let message: ClientMessage = read_frame(&mut stream).unwrap();
                        let response = if inventory {
                            assert!(matches!(message.request, Request::Inspect));
                            Response::Inventory {
                                registry: ovrcr::config::Registry::default(),
                                sessions: snapshot.projects[0].workspaces[0].sessions.clone(),
                            }
                        } else {
                            assert!(matches!(message.request, Request::List));
                            Response::Hierarchy(snapshot)
                        };
                        write_frame(
                            &mut stream,
                            &ServerMessage::Response {
                                request_id: message.request_id,
                                response,
                            },
                        )
                        .unwrap();
                        return;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => return,
                }
            }
        });
        let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(if inventory {
                vec!["--json", "terminal", "list"]
            } else {
                vec!["session", "usage", "7"]
            })
            .env("OVRCR_CONFIG", root.path().join("config.toml"))
            .env("OVRCR_SOCKET", &socket)
            .output()
            .unwrap();
        server.join().unwrap();
        std::fs::remove_file(&socket).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let value = if inventory { &response[0] } else { &response };
        assert_eq!(value[if inventory { "id" } else { "session" }], 7);
        assert_eq!(value["agent"], expected);
        if case < 2 {
            assert_eq!(value["reporting_unavailable"], true);
            let ages = &value["measurement_age_ms"];
            assert_eq!(
                ages["context"].as_u64().unwrap() - ages["usage"].as_u64().unwrap(),
                2000
            );
            assert_eq!(
                ages["cost"].as_u64().unwrap() - ages["usage"].as_u64().unwrap(),
                1000
            );
            assert!(
                value["agent"]["metrics"]["sample"]["context"]["value"]["used_tokens"].is_null()
            );
        } else {
            assert!(value["reporting_unavailable"].is_null());
            assert!(value["measurement_age_ms"].is_null());
        }
    }
}

#[test]
fn sqlite_migration_is_authoritative_across_online_offline_and_restart() {
    let mut fixture = Fixture(Live::idle().bounded());
    let legacy_workspace = fixture.workspace_root.join("legacy");
    live::git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature/legacy",
            legacy_workspace.to_str().unwrap(),
            "main",
        ],
    );
    let original = toml::to_string(&ovrcr::config::Registry {
        projects: vec![ovrcr::config::ProjectRecord {
            name: "fixture".into(),
            repo: fixture.repo.clone(),
            workspace_root: fixture.workspace_root.clone(),
            workspaces: vec![ovrcr::config::WorkspaceRecord {
                name: "legacy".into(),
                path: legacy_workspace.clone(),
                branch: "feature/legacy".into(),
            }],
        }],
    })
    .unwrap();
    std::fs::write(&fixture.config, &original).unwrap();
    fixture.start_binary();
    assert_eq!(
        fixture.json(&["project", "get", "fixture"])["workspace_count"],
        1
    );
    let imported = fixture.json(&[
        "workspace",
        "get",
        "--project",
        "fixture",
        "--name",
        "legacy",
    ]);
    assert_eq!(imported["path"], legacy_workspace.to_str().unwrap());
    assert_eq!(imported["branch"], "feature/legacy");
    fixture.ok(&[
        "workspace",
        "create",
        "--project",
        "fixture",
        "--name",
        "migrated",
        "--new-branch",
        "feature/migrated",
        "--base",
        "main",
    ]);
    fixture.capture();
    let online = fixture.json(&[
        "workspace",
        "get",
        "--project",
        "fixture",
        "--name",
        "migrated",
    ]);
    assert_eq!(online["branch"], "feature/migrated");
    assert_eq!(
        std::fs::read_to_string(&fixture.config).unwrap(),
        original,
        "migration and later mutations must leave recoverable legacy data untouched"
    );
    fixture.ok(&["shutdown", "--kill"]);
    fixture.join();

    // Once imported, even a broken legacy file cannot replace committed inventory.
    std::fs::write(&fixture.config, "[[projects]\n").unwrap();
    let offline = fixture.json(&[
        "workspace",
        "get",
        "--project",
        "fixture",
        "--name",
        "migrated",
    ]);
    assert_eq!(offline["path"], online["path"]);
    assert_eq!(offline["branch"], online["branch"]);
    assert_eq!(offline["terminal_count"], 0);
    assert!(
        !fixture.socket.exists(),
        "offline inspection must not start a server"
    );

    fixture.start_binary();
    assert_eq!(
        fixture.json(&["project", "get", "fixture"])["workspace_count"],
        2,
        "restart must neither discard nor re-import workspace records"
    );
    fixture.ok(&[
        "workspace",
        "remove",
        "--project",
        "fixture",
        "--name",
        "migrated",
    ]);
    fixture.ok(&[
        "workspace",
        "remove",
        "--project",
        "fixture",
        "--name",
        "legacy",
    ]);
    fixture.ok(&["project", "remove", "fixture"]);
    assert_eq!(fixture.json(&["project", "list"]), serde_json::json!([]));
}
