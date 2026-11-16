// Controlled model plans over real native tools; not a production cleanup engine.
use super::*;

fn retains_task_ownership(run: &Value) -> bool {
    !(run["spec"]["target"].get("Git").is_some()
        && run["workspace"].is_null()
        && matches!(
            run["status"].as_str(),
            Some(
                "Succeeded" | "Failed" | "Cancelled" | "TimedOut" | "Interrupted" | "CleanupFailed"
            )
        ))
}

pub(super) fn plan(body: &Value, step: usize) -> Plan {
    let user = operator_provider::last_user_text(body);
    let request = user.split("User request:\n").last().unwrap();
    if let Some(target) = request.strip_prefix("Catchup cleanup ") {
        let parts: Vec<u64> = target
            .split(':')
            .map(|part| part.parse().unwrap())
            .collect();
        let (id, old, current) = (parts[0], parts[1], parts[2]);
        return match step % 4 {
            0 => Plan::tool(
                "stale-watch",
                "watch_sessions",
                json!({"action":"add", "sessions":[{"session_id":id,"label":"catchup-old-run","baseline":{"run":old,"activity_revision":0,"unread_turn":null}}]}),
            ),
            1 => Plan::tool(
                "archive-current-not-watched",
                "bash",
                json!({"command":format!("\"$OVRCR_EXECUTABLE\" --json terminal archive-stopped {id} --expected-run {current}")}),
            ),
            2 => Plan::tool(
                "catchup-pending",
                "watch_sessions",
                json!({"action":"list"}),
            ),
            _ => {
                let results = operator_provider::tool_result_texts(body);
                let pending = results
                    .iter()
                    .rev()
                    .filter_map(|text| serde_json::from_str::<Value>(text).ok())
                    .find(|value| value.get("pending_reports").is_some())
                    .unwrap();
                let report = pending["pending_reports"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["session_id"] == id)
                    .unwrap();
                assert_eq!(report["watched_run"], old);
                assert_eq!(report["run"], current);
                assert_eq!(report["reason"], "run_changed");
                assert_eq!(report["protected"], false);
                assert!(results.iter().any(|text| text.contains("\"ok\":true")));
                Plan::text(
                    "CATCHUP_ORIGINAL_RUN_ONLY: replacement archived by separate exact authorization; old report retained.",
                )
            }
        };
    }
    if let Some(targets) = request.strip_prefix("Approve cleanup archive ") {
        let targets: Vec<(u64, u64)> = targets
            .split(',')
            .map(|target| {
                let (id, run) = target.trim().split_once(':').unwrap();
                (id.parse().unwrap(), run.parse().unwrap())
            })
            .collect();
        return match step % 3 {
            0 => {
                assert!(
                    user.contains("terminal archive-stopped"),
                    "approved execution must receive the shipped guarded procedure"
                );
                Plan::tool(
                    "archive-watch-refresh",
                    "watch_sessions",
                    json!({"action": "list"}),
                )
            }
            1 => Plan::tool(
                "archive-selected",
                "bash",
                json!({"command": format!(
                "python3 - <<'PY'\nimport os,json,subprocess\nexe=os.environ['OVRCR_EXECUTABLE']\nselected={}\nresults=[]\nfor sid,run in selected:\n p=subprocess.run([exe,'--json','terminal','archive-stopped',str(sid),'--expected-run',str(run)],capture_output=True,text=True)\n results.append({{'id':sid,'approved_run':run,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr}})\ndef read(*args):\n return json.loads(subprocess.check_output([exe,'--json',*args]))\nprint('CLEANUP_ARCHIVE='+json.dumps({{'results':results,'active':read('terminal','list'),'archived':read('terminal','list','--archived')}}))\nPY", serde_json::to_string(&targets).unwrap())}),
            ),
            _ => {
                let result = marked_tool_json(body, "CLEANUP_ARCHIVE=");
                let mut text = String::from("Approved archive results:\n");
                for action in result["results"].as_array().unwrap() {
                    let archived = result["archived"].as_array().unwrap().iter().any(|row| {
                        row["id"] == action["id"]
                            && row["run"].as_u64() == action["approved_run"].as_u64().map(|n| n + 1)
                    });
                    let outcome = if action["exit"] == 0 && archived {
                        "verified completed"
                    } else if action["stderr"].as_str().unwrap().contains("Conflict") {
                        "changed/skipped"
                    } else if action["exit"] != 0 {
                        "refused; reconcile original action before any retry"
                    } else {
                        "uncertain; reconcile original action without replay"
                    };
                    text.push_str(&format!(
                        "Session {} run {}: {outcome}.\n",
                        action["id"], action["approved_run"]
                    ));
                }
                text.push_str(
                    "Unapproved records, provider history and Workspace files preserved.",
                );
                Plan::text(&text)
            }
        };
    }
    let option = |name: &str| {
        request
            .split_whitespace()
            .find_map(|word| word.strip_prefix(name))
    };
    let at = option("at=")
        .map(|n| n.parse::<u64>().unwrap())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
        });
    let hours = option("hours=")
        .map(|n| n.parse::<u64>().unwrap())
        .unwrap_or(24);
    let project = option("project=");
    match step % 4 {
        0 => {
            let system = operator_provider::system_text(body);
            let location = system
                .split("<name>ovrcr-cleanup</name>")
                .nth(1)
                .and_then(|tail| tail.split("<location>").nth(1))
                .and_then(|tail| tail.split("</location>").next())
                .expect("shipped cleanup advertised");
            Plan::tool("cleanup-instructions", "read", json!({"path": location}))
        }
        1 => {
            let instructions = operator_provider::tool_result_texts(body)
                .join("\n")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            for required in [
                "proposal-first",
                "Cleanup execution unavailable",
                "all registered Projects",
                "24 hours",
                "age >= threshold",
                "Agent-state change or user input",
                "Null, unavailable or future",
                "Busy or paused",
                "Unread or WaitingInput",
                "active watches or task-owned",
                "Controller, Human or Uncertain",
                "Ready, a title",
                "completion trailer",
                "archive stopped record",
                "stop live Agent, then archive record",
                "remove Workspace checkout",
                "including archived rows",
                "provider-history location unverified",
                "unmerged and unpublished",
                "not remote cancellation",
                "no periodic check",
            ] {
                assert!(
                    instructions.contains(required),
                    "missing cleanup rule: {required}"
                );
            }
            Plan::tool(
                "cleanup-watches",
                "watch_sessions",
                json!({"action": "list"}),
            )
        }
        2 => Plan::tool(
            "cleanup-inventory",
            "bash",
            json!({"command": format!(
            "python3 - <<'PY'\n{}\nPY", include_str!("cleanup-inventory.py"))}),
        ),
        _ => {
            let inventory = marked_tool_json(body, "CLEANUP_INVENTORY=");
            let watches = operator_provider::tool_result_texts(body)
                .iter()
                .rev()
                .filter_map(|text| serde_json::from_str::<Value>(text).ok())
                .find(|value| value.get("watches").is_some())
                .expect("native watch list");
            let watched: Vec<_> = watches["watches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| w["session_id"].clone())
                .collect();
            let rows = inventory["sessions"].as_array().unwrap();
            let archived = inventory["archived"].as_array().unwrap();
            let mut text = format!(
                "Scope: {}. Observation {at}; threshold {hours} hours.\n",
                project
                    .map(|p| format!("Project {p}"))
                    .unwrap_or_else(|| "all registered Projects in this instance".into())
            );
            for row in rows
                .iter()
                .filter(|r| r["scope"] == "Workspace" && project.is_none_or(|p| r["project"] == p))
            {
                let age = row["last_activity_unix_ms"]
                    .as_u64()
                    .and_then(|last| at.checked_sub(last));
                let mut blockers = Vec::new();
                if matches!(row["activity"].as_str(), Some("busy")) {
                    blockers.push("Busy");
                }
                if row["phase"] == "paused" {
                    blockers.push("paused");
                }
                if !row["unread"].is_null() {
                    blockers.push("Unread");
                }
                if row["activity"] == "waiting_input"
                    || row["agent"]["input_requests"]
                        .as_array()
                        .is_some_and(|v| !v.is_empty())
                {
                    blockers.push("WaitingInput");
                }
                if row["recovery"]["requires_ack"] == true || row["phase"] == "interrupted" {
                    blockers.push("uncertain ownership");
                }
                if watched.contains(&row["id"]) {
                    blockers.push("active watch");
                }
                if row.get("assignment_control").is_some() {
                    blockers.push("task-owned assignment");
                }
                if inventory["runs"].as_array().unwrap().iter().any(|run| {
                    retains_task_ownership(run)
                        && run["directory"] == row["cwd"]
                        && !run["directory"].is_null()
                }) {
                    blockers.push("task-owned Workspace");
                }
                let live = matches!(row["phase"].as_str(), Some("running" | "paused"));
                if live
                    && (row["agent"].is_null()
                        || row["reporting_unavailable"] == true
                        || row["activity"] == "unknown")
                {
                    blockers.push("reporting unavailable");
                }
                let effect = if age.is_none() {
                    "unknown age; preserve"
                } else if age.is_some_and(|age| age < hours * 3_600_000) {
                    "below threshold; preserve"
                } else if !blockers.is_empty() {
                    "protected; preserve"
                } else if matches!(row["phase"].as_str(), Some("stopped" | "exited")) {
                    "propose archive stopped record"
                } else {
                    "review required before proposing stop live Agent, then archive record"
                };
                let workspace = inventory["workspaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|w| w["path"] == row["cwd"]);
                text.push_str(&format!("Session {} run {} Project {} Workspace {} path {}: last={} age_ms={:?}; phase={} activity={} health={} unread={} requests={}; {effect}; blockers={}.\n",
                    row["id"], row["run"], row["project"], workspace.map(|w| &w["id"]).unwrap_or(&Value::Null), row["cwd"],
                    row["last_activity_unix_ms"], age, row["phase"], row["activity"], row["agent"]["health"], row["unread"], row["agent"]["input_requests"], blockers.join(", ")));
            }
            for w in inventory["workspaces"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|w| project.is_none_or(|p| w["project"] == p))
            {
                let mut blockers = Vec::new();
                if inventory["projects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["repo"] == w["path"])
                {
                    blockers.push("Root");
                }
                if w["git"]["dirty"]["text"] != "" {
                    blockers.push("dirty");
                }
                let occupants: Vec<_> = rows
                    .iter()
                    .chain(archived)
                    .filter(|r| r["cwd"] == w["path"])
                    .collect();
                if occupants.iter().any(|r| r["archived"] == false) {
                    blockers.push("occupied");
                }
                if occupants
                    .iter()
                    .any(|r| r["recovery"]["requires_ack"] == true)
                {
                    blockers.push("uncertain archived ownership");
                }
                if rows
                    .iter()
                    .chain(archived)
                    .any(|r| !r["recovery"]["conversation"].is_null())
                {
                    blockers.push("provider-history location unverified");
                }
                if inventory["runs"].as_array().unwrap().iter().any(|r| {
                    retains_task_ownership(r)
                        && (r["directory"] == w["path"] || r["workspace"] == w["id"])
                }) {
                    blockers.push("task-owned Workspace");
                }
                text.push_str(&format!("Workspace {} Project {} path {} HEAD {}: {}; blockers={}; unmerged={}; unpublished={}.\n", w["id"], w["project"], w["path"], w["git"]["head"]["text"],
                    if blockers.is_empty() { "propose separately approved remove Workspace checkout" } else if blockers == ["provider-history location unverified"] { "separately approved remove Workspace checkout blocked pending history verification; preserve" } else { "preserve" }, blockers.join(", "), w["git"]["unmerged"]["text"], w["git"]["unpublished"]["text"]));
            }
            text.push_str("No resources changed. Await approval of selected stopped-record archives. Live stop/Workspace removal: Cleanup execution unavailable. Branches, Retained records and provider history preserved. Operator excluded. Approval never implies Project/branch/record deletion or remote cancellation.");
            Plan::text(&text)
        }
    }
}

#[test]
#[ignore = "requires installed Pi; real read-only cleanup preview, watches, tasks and Git"]
fn installed_pi_operator_cleanup_preview_preserves_resources() {
    cleanup_case(false);
}

#[test]
#[ignore = "requires installed Pi; approved partial archive through native tools and CLI"]
fn installed_pi_operator_cleanup_archive_stopped_partial_approval() {
    cleanup_case(true);
}

fn cleanup_case(archive: bool) {
    use ovrcr::protocol::{
        BranchRequest, ConversationReference, CreateSessionRequest, ExtensionConversation,
        SessionKind,
    };
    let pi = pi_executable();
    let mut live = live::Live::idle().bounded();
    let relocated = live.root.path().join("installed/ovrcr");
    std::fs::create_dir(relocated.parent().unwrap()).unwrap();
    std::fs::copy(&live.executable, &relocated).unwrap();
    live.executable = relocated;
    let provider = Provider::start(scenario_handler(
        live.repo.clone(),
        live.root.path().join("marker"),
        live.root.path().join("background"),
    ));
    provider.seed_profile(&live.config);
    provider.seed_workspace_profile(live.root.path());
    install_personal_skill(live.root.path());
    live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
    live.ready("cleanup-work");
    let git = |path: &Path, args: &[&str]| {
        let output = Command::new("git")
            .env("HOME", live.root.path())
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    for name in ["clean", "dirty", "history", "uncertain"] {
        assert_eq!(
            live.request(Request::CreateWorkspace {
                project: "fixture".into(),
                id: name.into(),
                branch: BranchRequest::New {
                    branch: name.into(),
                    base: "main".into()
                }
            }),
            Response::Ok
        );
    }
    let dirty = live.workspace_path("dirty").join("keep.txt");
    std::fs::write(&dirty, "uncommitted work\n").unwrap();
    // A real unmerged/unpublished commit in an otherwise clean, unused checkout.
    let clean = live.workspace_path("clean");
    std::fs::write(clean.join("committed.txt"), "keep branch\n").unwrap();
    git(&clean, &["add", "committed.txt"]);
    git(
        &clean,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "unpublished cleanup fixture",
        ],
    );
    let other = live.root.path().join("other-repo");
    git(
        live.root.path(),
        &[
            "clone",
            "--quiet",
            live.repo.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    assert_eq!(
        live.request(Request::AddProject {
            name: "other".into(),
            repo: other,
            workspace_root: live.root.path().join("other-workspaces")
        }),
        Response::Ok
    );
    let mut seeded = Vec::new();
    for (name, workspace) in [
        ("older", "work"),
        ("boundary", "work"),
        ("younger", "work"),
        ("null", "work"),
        ("history", "history"),
        ("uncertain", "uncertain"),
    ] {
        let Response::CreatedSession(row) =
            live.request(Request::CreateSession(CreateSessionRequest {
                project: "fixture".into(),
                workspace: workspace.into(),
                name: name.into(),
                label: None,
                kind: SessionKind::Terminal,
                argv: vec!["/bin/sh".into()],
            }))
        else {
            panic!("create cleanup session")
        };
        live.own_group(row.pid.unwrap() as i32);
        seeded.push((name, row.id));
    }
    // A retained reference can depend on a different checkout than its session cwd.
    let history_file = clean.join("ignored-history.jsonl");
    std::fs::write(&history_file, "provider-owned history\n").unwrap();
    std::fs::write(
        live.repo.join(".git/info/exclude"),
        "ignored-history.jsonl\n",
    )
    .unwrap();
    for name in ["history", "uncertain"] {
        let id = seeded.iter().find(|(n, _)| *n == name).unwrap().1;
        status_cli(&live, &["terminal", "close", &id.0.to_string()]);
    }
    status_cli(&live, &["shutdown", "--kill"]);
    live.join();
    // Existing retained-session fixture seam: seed historical timestamps and
    // same-boot uncertainty only while every task-owned process is stopped.
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let db = rusqlite::Connection::open(ovrcr::config::database_path(&live.config)).unwrap();
    for (name, id) in &seeded {
        let age = match *name {
            "older" => Some(86_400_001u64),
            "boundary" => Some(86_400_000),
            "younger" => Some(86_399_999),
            "null" => None,
            _ => Some(172_800_000),
        };
        db.execute(
            "UPDATE retained_sessions SET last_activity_unix_ms = ?1 WHERE id = ?2",
            rusqlite::params![age.map(|age| (at - age) as i64), id.0 as i64],
        )
        .unwrap();
    }
    let uncertain = seeded.iter().find(|(n, _)| *n == "uncertain").unwrap().1;
    db.execute(
        "UPDATE retained_sessions SET stopped = 0 WHERE id = ?1",
        [uncertain.0 as i64],
    )
    .unwrap();
    let history_id = seeded.iter().find(|(n, _)| *n == "history").unwrap().1;
    let reference = ConversationReference::Pi(ExtensionConversation {
        conversation: "cleanup-history".into(),
        executable: pi.clone(),
        history: Some(history_file.clone()),
        config_dir: live.root.path().to_owned(),
        options: vec![],
    });
    db.execute(
        "INSERT INTO agent_conversations (session, reference, invalid) VALUES (?1, ?2, 0)",
        rusqlite::params![
            history_id.0 as i64,
            serde_json::to_string(&reference).unwrap()
        ],
    )
    .unwrap();
    let older_id = seeded.iter().find(|(n, _)| *n == "older").unwrap().1;
    db.execute(
        "INSERT INTO agent_conversations (session, reference, invalid) VALUES (?1, ?2, 0)",
        rusqlite::params![
            older_id.0 as i64,
            serde_json::to_string(&reference).unwrap()
        ],
    )
    .unwrap();
    drop(db);
    live.start_binary_env(&[("OVRCR_PI_EXECUTABLE", pi.as_os_str())]);
    let mut owners = Vec::new();
    let mut protected = Vec::new();
    for (name, reason) in [
        ("care-busy", "Busy"),
        ("care-idle", "paused"),
        ("status-ready", "Unread"),
        ("status-input", "WaitingInput"),
        ("status-unavailable", "reporting unavailable"),
    ] {
        let (id, owner) = status_agent(&live, name);
        owners.push(owner);
        if name == "care-idle" {
            status_cli(&live, &["pause", &id.0.to_string()]);
        }
        status_cli(
            &live,
            &[
                "terminal",
                "rename",
                &id.0.to_string(),
                "finished OVRCR_WORKER_DONE",
            ],
        );
        protected.push((id, reason));
    }
    let Response::CreatedSession(missing) =
        live.request(Request::CreateSession(CreateSessionRequest {
            project: "fixture".into(),
            workspace: "work".into(),
            name: "missing-reporter".into(),
            label: None,
            kind: SessionKind::Agent { name: "pi".into() },
            argv: vec!["/bin/sh".into()],
        }))
    else {
        panic!("unreported Agent fixture")
    };
    live.own_group(missing.pid.unwrap() as i32);
    protected.push((missing.id, "reporting unavailable"));
    let (idle, idle_owner) = status_agent(&live, "care-omp");
    owners.push(idle_owner);
    // The native task runner remains held in a real Git Workspace throughout preview.
    let remote = live.root.path().join("remote.git");
    git(
        live.root.path(),
        &[
            "clone",
            "--bare",
            "--quiet",
            live.repo.to_str().unwrap(),
            remote.to_str().unwrap(),
        ],
    );
    git(
        &live.repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let task = status_cli(
        &live,
        &[
            "task",
            "create",
            "cleanup-protected",
            "--project",
            "fixture",
            "--remote",
            "origin",
            "--branch",
            "main",
            "--every",
            "1d",
            "--model",
            "fixture/operator-fixture",
            "--prompt",
            "CLEANUP_TASK_HOLD",
        ],
    );
    let run = status_cli(&live, &["task", "run", &task["id"].to_string()]);
    let deadline = Instant::now() + BUDGET;
    loop {
        let observed = status_cli(&live, &["run", "get", &run["id"].to_string()]);
        if observed["status"] == "Running"
            && provider
                .requests()
                .iter()
                .any(|r| operator_provider::last_user_text(&r.body) == "CLEANUP_TASK_HOLD")
        {
            break;
        }
        assert!(Instant::now() < deadline, "task did not run: {observed}");
        thread::sleep(Duration::from_millis(20));
    }
    let mut driver = DashboardDriver::connect(&live);
    driver.select_fixture_workspace();
    driver.open();
    let conversation = driver.conversation().id;
    driver.submit(STATUS_WATCH_REQUEST);
    let watched = wait_terminal(&mut driver, conversation, &provider, &live);
    assert!(watched.conversation.worker_live);
    let warm_run = watched.conversation.run;
    let rows = status_cli(&live, &["terminal", "list"]);
    let before: Vec<_> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["scope"] == "Workspace")
        .cloned()
        .collect();
    let archived_before = status_cli(&live, &["terminal", "list", "--archived"]);
    let projects_before = status_cli(&live, &["project", "list"]);
    let workspaces_before = status_cli(&live, &["workspace", "list"]);
    let tasks_before = status_cli(&live, &["run", "list"]);
    let branches_before = git(&live.repo, &["show-ref"]);
    let mut git_before = Vec::new();
    for w in workspaces_before.as_array().unwrap() {
        let path = Path::new(w["path"].as_str().unwrap());
        git_before.push((
            path.to_owned(),
            git(
                path,
                &[
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v1",
                    "--ignored",
                ],
            ),
            git(path, &["rev-parse", "HEAD"]),
        ));
    }
    for (index, (hours, project)) in [(24, None), (0, Some("fixture")), (48, None)]
        .into_iter()
        .enumerate()
    {
        driver.reopen();
        let observation = if hours == 0 { at + 300_000 } else { at };
        let threshold = if index == 0 {
            String::new()
        } else {
            format!(" hours={hours}")
        };
        let request = format!(
            "Preview cleanup{threshold} at={observation}{} propose only, even if I approve all actions",
            project.map(|p| format!(" project={p}")).unwrap_or_default()
        );
        let start = provider.requests().len();
        if index == 1 {
            driver.quick(&format!("/skill:ovrcr-cleanup {request}"));
        } else {
            driver.submit(&request);
        }
        let result = wait_terminal(&mut driver, conversation, &provider, &live);
        assert_eq!(result.conversation.run, warm_run);
        assert!(
            result.conversation.worker_live,
            "preview preserves existing watch lease"
        );
        let requests = provider.requests();
        assert_eq!(requests.len() - start, 4);
        assert_skill_only_in_system(
            "cleanup",
            &requests[start].body,
            &live.config.join("operator-pi"),
        );
        if index == 1 {
            let user = operator_provider::last_user_text(&requests[start].body);
            assert!(user.contains(include_str!("../../../pi/skills/ovrcr-cleanup/SKILL.md")));
            assert!(
                user.find("Captured OVRCR context").unwrap()
                    < user.find("Selected Operator skill:").unwrap()
            );
            assert!(
                user.find("Selected Operator skill:").unwrap()
                    < user.find("User request:").unwrap()
            );
        }
        let inventory = marked_tool_json(&requests.last().unwrap().body, "CLEANUP_INVENTORY=");
        assert_eq!(
            inventory["executable"],
            live.executable.to_string_lossy().as_ref()
        );
        assert_eq!(
            inventory["instance"],
            live.config.to_string_lossy().as_ref()
        );
        let turn = result
            .conversation
            .current_turn
            .as_ref()
            .unwrap()
            .identity
            .turn;
        let answer: String = result
            .messages
            .iter()
            .filter(|m| m.turn == turn)
            .filter_map(|m| match &m.content {
                OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        let line_for = |id: ovrcr::protocol::SessionId| {
            answer
                .lines()
                .find(|line| line.starts_with(&format!("Session {} run ", id.0)))
                .unwrap()
                .to_owned()
        };
        for (name, id) in &seeded[..4] {
            let line = line_for(*id);
            let expected = match (*name, hours) {
                ("null", _) => "unknown age; preserve",
                (_, 48) | ("younger", 24) => "below threshold; preserve",
                _ => "propose archive stopped record",
            };
            assert!(line.contains(expected), "{name}: {line}");
        }
        if hours == 0 {
            for (id, reason) in &protected {
                let line = line_for(*id);
                assert!(
                    line.contains("protected; preserve") && line.contains(reason),
                    "{line}"
                );
            }
            assert!(
                line_for(idle).contains(
                    "review required before proposing stop live Agent, then archive record"
                )
            );
            let ready = protected[2].0;
            assert!(line_for(ready).contains("active watch"));
            assert!(
                !answer.contains("Project \"other\""),
                "narrow scope: {answer}"
            );
        } else {
            assert!(answer.contains("Project \"other\""));
        }
        for required in [
            "Root",
            "dirty",
            "occupied",
            "uncertain archived ownership",
            "provider-history location unverified",
            "task-owned Workspace",
            "unpublished cleanup fixture",
            "separately approved remove Workspace checkout",
            "Cleanup execution unavailable",
        ] {
            assert!(answer.contains(required), "missing {required}: {answer}");
        }
        let clean_line = answer
            .lines()
            .find(|line| line.starts_with("Workspace \"clean\" "))
            .unwrap();
        assert!(
            clean_line.contains("blocked pending history verification; preserve"),
            "{clean_line}"
        );
        assert!(
            !answer.contains(&format!("Session {} run", driver.conversation().session.0)),
            "Operator is excluded"
        );
        assert!(driver.screen().contains("Cleanup execution unavailable"));
        assert!(
            tool_outputs_with_status(&result)
                .iter()
                .all(|(_, _, status)| *status == OperatorToolStatus::Succeeded)
        );
        let after = status_cli(&live, &["terminal", "list"]);
        assert_eq!(
            after
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["scope"] == "Workspace")
                .cloned()
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(
            status_cli(&live, &["terminal", "list", "--archived"]),
            archived_before
        );
        assert_eq!(status_cli(&live, &["run", "list"]), tasks_before);
        eprintln!(
            "INSTALLED_PI_CLEANUP_CASE hours={hours} project={project:?} requests=4 unchanged=true"
        );
    }
    assert_eq!(status_cli(&live, &["project", "list"]), projects_before);
    assert_eq!(status_cli(&live, &["workspace", "list"]), workspaces_before);
    assert_eq!(git(&live.repo, &["show-ref"]), branches_before);
    for (path, status, head) in git_before {
        assert_eq!(
            git(
                &path,
                &[
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v1",
                    "--ignored"
                ]
            ),
            status
        );
        assert_eq!(git(&path, &["rev-parse", "HEAD"]), head);
    }
    assert_eq!(
        std::fs::read_to_string(&history_file).unwrap(),
        "provider-owned history\n"
    );
    assert_eq!(
        std::fs::read_to_string(&dirty).unwrap(),
        "uncommitted work\n"
    );
    for name in [
        "care-busy",
        "care-idle",
        "care-omp",
        "status-ready",
        "status-input",
        "status-unavailable",
    ] {
        assert!(
            !live
                .root
                .path()
                .join(format!("{name}.identity.input"))
                .exists()
        );
    }
    if archive {
        let id_for = |name: &str| seeded.iter().find(|(n, _)| *n == name).unwrap().1;
        let older = id_for("older");
        let younger = id_for("younger");
        let boundary = id_for("boundary");
        let approved_run = |id| {
            before.iter().find(|row| row["id"] == id).unwrap()["run"]
                .as_u64()
                .unwrap()
        };
        let older_run = approved_run(older.0);
        let younger_run = approved_run(younger.0);
        status_cli(
            &live,
            &[
                "terminal",
                "reopen",
                &younger.0.to_string(),
                "--expected-run",
                &younger_run.to_string(),
            ],
        );
        for group in live.session_groups() {
            live.own_group(group);
        }
        driver.reopen();
        let start = provider.requests().len();
        driver.quick(&format!(
            "/skill:ovrcr-cleanup Approve cleanup archive {}:{older_run},{}:{younger_run}",
            older.0, younger.0
        ));
        let result = wait_terminal(&mut driver, conversation, &provider, &live);
        assert_eq!(provider.requests().len() - start, 3);
        let archived = status_cli(&live, &["terminal", "list", "--archived"]);
        let archived = archived.as_array().unwrap();
        let record = archived
            .iter()
            .find(|row| row["id"] == older.0)
            .expect("selected exact record archived");
        assert_eq!(record["run"], older_run + 1);
        assert_eq!(record["recovery"]["conversation"], "cleanup-history");
        assert_eq!(
            record["cwd"],
            before.iter().find(|row| row["id"] == older.0).unwrap()["cwd"]
        );
        let active = status_cli(&live, &["terminal", "list"]);
        assert!(
            active
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == boundary.0
                    && row["run"].as_u64() == Some(approved_run(boundary.0)))
        );
        assert!(
            active
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == younger.0
                    && row["phase"] == "running"
                    && row["run"] == younger_run + 1)
        );
        let output = marked_tool_json(
            &provider.requests().last().unwrap().body,
            "CLEANUP_ARCHIVE=",
        );
        assert_eq!(output["results"][0]["exit"], 0);
        assert_ne!(output["results"][1]["exit"], 0);
        assert!(driver.screen().contains("verified completed"));
        assert!(driver.screen().contains("changed/skipped"));
        assert_eq!(
            std::fs::read_to_string(&history_file).unwrap(),
            "provider-owned history\n"
        );
        let mut expected_workspaces = workspaces_before.clone();
        let workspace = expected_workspaces
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["project"] == "fixture" && row["id"] == "work")
            .unwrap();
        workspace["terminal_count"] = json!(workspace["terminal_count"].as_u64().unwrap() - 1);
        assert_eq!(
            status_cli(&live, &["workspace", "list"]),
            expected_workspaces
        );
        assert_eq!(git(&live.repo, &["show-ref"]), branches_before);
        assert!(
            tool_outputs_with_status(&result)
                .iter()
                .all(|(_, _, status)| *status == OperatorToolStatus::Succeeded)
        );
        eprintln!(
            "INSTALLED_PI_ARCHIVE_PASS selected=2 completed=1 changed=1 unapproved=preserved history=preserved"
        );
    }
    if archive {
        let Response::CreatedSession(catchup) =
            live.request(Request::CreateSession(CreateSessionRequest {
                project: "fixture".into(),
                workspace: "work".into(),
                name: "catchup-old-run".into(),
                label: None,
                kind: SessionKind::Agent { name: "pi".into() },
                argv: vec!["/bin/sh".into()],
            }))
        else {
            panic!("catchup fixture")
        };
        live.own_group(catchup.pid.unwrap() as i32);
        status_cli(&live, &["terminal", "close", &catchup.id.0.to_string()]);
        status_cli(&live, &["terminal", "unarchive", &catchup.id.0.to_string()]);
        driver.reopen();
        driver.submit(&format!(
            "Catchup cleanup {}:{}:{}",
            catchup.id.0,
            catchup.run.0,
            catchup.run.0 + 2
        ));
        wait_terminal(&mut driver, conversation, &provider, &live);
        driver.wait_history(conversation, |history| history.messages.iter().any(|message| matches!(
            &message.content, OperatorMessageContent::AsyncWakeup { text } if text.contains("catchup-old-run") && text.contains("run_changed")
        )) && history.conversation.current_turn.as_ref().is_some_and(|turn| turn.state == OperatorTurnState::Complete));
        let rows = status_cli(&live, &["terminal", "list", "--archived"]);
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == catchup.id.0 && row["run"] == catchup.run.0 + 3)
        );
        eprintln!(
            "INSTALLED_PI_WATCH_CATCHUP_PASS original_run_report=true replacement_unprotected=true actual_handshake=true"
        );
    }
    driver.submit(STATUS_UNWATCH_REQUEST);
    wait_terminal(&mut driver, conversation, &provider, &live);
    wait_stopped(&mut driver, conversation);
    if archive {
        for id in [protected[0].0, protected[2].0] {
            let row = before.iter().find(|row| row["id"] == id.0).unwrap();
            let expected_run = ovrcr::protocol::SessionRunId(row["run"].as_u64().unwrap());
            assert_eq!(
                live.request(Request::ControlSession {
                    session: id,
                    expected_run,
                    action: ovrcr::protocol::SessionControlAction::Stop
                }),
                Response::Ok
            );
            assert!(
                matches!(
                    live.request(Request::ControlSession {
                        session: id,
                        expected_run,
                        action: ovrcr::protocol::SessionControlAction::ArchiveStopped
                    }),
                    Response::Error {
                        code: ovrcr::protocol::ErrorCode::Conflict,
                        ..
                    }
                ),
                "stopped Busy/Unread results remain protected, independently of watches"
            );
        }
        let task_workspace = tasks_before
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == run["id"])
            .unwrap()["workspace"]
            .as_str()
            .unwrap();
        let Response::CreatedSession(task_record) =
            live.request(Request::CreateSession(CreateSessionRequest {
                project: "fixture".into(),
                workspace: task_workspace.into(),
                name: "task-owned-stopped".into(),
                label: None,
                kind: SessionKind::Terminal,
                argv: vec!["/bin/sh".into()],
            }))
        else {
            panic!("task Workspace terminal")
        };
        live.own_group(task_record.pid.unwrap() as i32);
        assert_eq!(
            live.request(Request::ControlSession {
                session: task_record.id,
                expected_run: task_record.run,
                action: ovrcr::protocol::SessionControlAction::Stop
            }),
            Response::Ok
        );
        assert!(matches!(
            live.request(Request::ControlSession {
                session: task_record.id,
                expected_run: task_record.run,
                action: ovrcr::protocol::SessionControlAction::ArchiveStopped
            }),
            Response::Error {
                code: ovrcr::protocol::ErrorCode::Conflict,
                ..
            }
        ));
    }
    provider.release();
    drop(driver);
    assert_eq!(live.request(Request::Shutdown { kill: true }), Response::Ok);
    live.join();
    drop(owners);
    eprintln!(
        "INSTALLED_PI_CLEANUP_PASS cases=3 boundary_ms=1 real_tasks=true archived_history=true full_and_quick=true shutdown=verified"
    );
}
