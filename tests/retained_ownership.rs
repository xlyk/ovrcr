#[path = "support/live.rs"]
mod live;

use live::Live;
use ovrcr::protocol::{AgentReport, AgentUpdate, ErrorCode, Request, Response, SessionRunId};
use ovrcr::session::{AgentActivity, SessionId};
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;
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

fn json_error(live: &Live, args: &[&str]) -> Value {
    let output = cli(live, &[&["--json"], args].concat());
    assert!(
        !output.status.success(),
        "expected failure, got {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stderr).unwrap()
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

fn row(live: &Live, id: u64) -> Value {
    json(live, &["terminal", "list"])
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("inventory lost session {id}"))
}

fn wait_text(live: &Live, id: u64, needle: &str) -> String {
    let id_arg = id.to_string();
    let deadline = Instant::now() + live::wait_deadline();
    loop {
        let output = cli(live, &["terminal", "read", &id_arg]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if text.contains(needle) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "fresh shell did not produce {needle}: {text}"
        );
        std::thread::yield_now();
    }
}

fn pid_exists(pid: libc::pid_t) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn wait_pid_file(path: &Path) -> libc::pid_t {
    let deadline = Instant::now() + live::wait_deadline();
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse::<libc::pid_t>()
            && pid > 1
            && pid_exists(pid)
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "owned descendant did not publish pid at {}",
            path.display()
        );
        std::thread::yield_now();
    }
}

fn crash_server_pid_only(live: &Live) {
    let pid = live.server_pid().expect("binary server pid") as libc::pid_t;
    assert!(pid > 1, "server pid is not a real child");
    // The server often shares the test's process group. Never signal -pgid.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let stop = live.join_within(live::wait_deadline());
    match stop {
        live::Stop::Exited(_) => {}
        other => panic!("SIGKILL of the server pid did not exit the hosted child: {other:?}"),
    }
}

fn reap_group(live: &Live, pgid: libc::pid_t) {
    assert!(pgid > 1);
    assert_ne!(
        pgid,
        unsafe { libc::getpgrp() },
        "refusing to signal the test process group"
    );
    if live::group_exists(pgid) {
        let sent = unsafe { libc::kill(-pgid, libc::SIGKILL) };
        assert!(
            sent == 0 || !live::group_exists(pgid),
            "could not signal owned group {pgid}"
        );
    }
    assert!(
        live::wait_group_absent(pgid, Duration::from_secs(2)),
        "owned group {pgid} still exists"
    );
    live.forget_group(pgid);
}

fn stop_server(live: &Live) {
    let stopped = cli(live, &["shutdown", "--kill"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    live.join();
    for group in live.owned_groups() {
        assert!(
            live::wait_group_absent(group, live::wait_deadline()),
            "owned group {group} survived shutdown --kill"
        );
        live.forget_group(group);
    }
}

fn retained_sql(live: &Live) -> Connection {
    Connection::open(ovrcr::config::database_path(&live.config)).unwrap()
}

fn patch_saved_boot(live: &Live, id: u64, boot_id: Option<&str>, stopped: i64) {
    let changed = retained_sql(live)
        .execute(
            "UPDATE retained_sessions SET boot_id = ?1, stopped = ?2 WHERE id = ?3",
            rusqlite::params![boot_id, stopped, i64::try_from(id).unwrap()],
        )
        .unwrap();
    assert_eq!(changed, 1, "retained row {id} missing from owned storage");
}

fn valid_boot_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
        && id.bytes().any(|byte| byte != b'0' && byte != b'-')
}

fn other_boot_uuid(current: &str) -> String {
    assert!(
        valid_boot_uuid(current),
        "native boot id is not a UUID: {current}"
    );
    let mut bytes = current.as_bytes().to_vec();
    bytes[0] = if bytes[0].eq_ignore_ascii_case(&b'a') {
        b'b'
    } else {
        b'a'
    };
    let other = String::from_utf8(bytes).unwrap();
    assert!(valid_boot_uuid(&other));
    assert!(!other.eq_ignore_ascii_case(current));
    other
}

fn independent_os_boot_id() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let value = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        let value = value.trim();
        valid_boot_uuid(value).then(|| value.to_ascii_lowercase())
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("sysctl")
            .args(["-n", "kern.bootsessionuuid"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8(output.stdout).ok()?;
        let value = value.trim().trim_end_matches('\0');
        valid_boot_uuid(value).then(|| value.to_ascii_lowercase())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

fn assert_requires_ack(row: &Value, expected: bool) {
    assert_eq!(row["recovery"]["requires_ack"], expected, "{row}");
}

fn assert_ownership_uncertain(error: &Value) {
    assert_eq!(error["error"]["code"], "OwnershipUncertain", "{error}");
    let message = error["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("acknowledge")
            || message.contains("ack-stopped")
            || message.contains("confirm"),
        "refusal did not ask for acknowledgement: {error}"
    );
}

fn assert_no_live_work(live: &Live) {
    assert!(
        live.session_groups().is_empty(),
        "inventory restoration launched work"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn current_boot_id_matches_macos_or_linux_os_source() {
    // Kernel boot UUID only. A container restart is not a reboot and is not claimed here.
    let from_os = independent_os_boot_id();
    let from_reader = ovrcr::retained::current_boot_id();
    assert_eq!(from_reader, from_os);
    assert!(
        from_os.is_some(),
        "OS boot UUID source produced no valid id on this host"
    );
}

#[test]
fn natural_leader_exit_with_attached_job_requires_ack_before_reopen() {
    let live = Live::binary();
    live.ready("feature/natural-exit-ownership");
    let pid_file = live.root.path().join("natural-survivor.pid");
    let helper = std::env::current_exe().unwrap();
    // The survivor is this test binary in `natural_survivor_helper` mode: its own
    // session and group, stdio off the PTY so the leader's exit reaches the
    // reader, and the leader waits for the published pid instead of racing a
    // fixed sleep against process startup.
    let created = create_terminal(
        &live,
        "natural-survivor",
        &[
            "/bin/sh",
            "-c",
            "OVRCR_SURVIVOR_PID_FILE=\"$2\" \"$1\" --ignored --exact natural_survivor_helper --nocapture </dev/null >/dev/null 2>&1 &\n\
             i=0; while [ ! -s \"$2\" ] && [ \"$i\" -lt 200 ]; do sleep 0.05; i=$((i+1)); done\n\
             exit 0",
            "natural-leader",
            helper.to_str().unwrap(),
            pid_file.to_str().unwrap(),
        ],
    );
    let id = created["id"].as_u64().unwrap();
    let leader_pid = created["pid"].as_u64().unwrap() as libc::pid_t;
    let leader_pgid = unsafe { libc::getpgid(leader_pid) };
    let survivor = wait_pid_file(&pid_file);
    let survivor_pgid = unsafe { libc::getpgid(survivor) };
    assert_ne!(
        survivor_pgid, leader_pgid,
        "survivor must occupy its own job-control group"
    );
    live.own_group(survivor_pgid);
    wait_phase(&live, id, "exited");
    let row = row(&live, id);
    assert_eq!(row["phase"], "exited");
    assert_requires_ack(&row, true);
    let id_arg = id.to_string();
    assert_ownership_uncertain(&json_error(&live, &["terminal", "reopen", &id_arg]));
    assert!(
        pid_exists(survivor),
        "natural leader exit reaped its attached job"
    );
    reap_group(&live, survivor_pgid);
    if leader_pgid > 1 {
        reap_group(&live, leader_pgid);
    }
}

/// Child of `natural_leader_exit_with_attached_job_requires_ack_before_reopen`;
/// never meaningful on its own. Leaves the leader's session, ignores its hangup,
/// publishes its pid atomically and waits to be reaped.
#[test]
#[ignore]
fn natural_survivor_helper() {
    let pid_file = std::path::PathBuf::from(std::env::var_os("OVRCR_SURVIVOR_PID_FILE").unwrap());
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
        assert_ne!(
            libc::setsid(),
            -1,
            "survivor could not leave the leader's session"
        );
    }
    let staged = pid_file.with_extension("pid.tmp");
    std::fs::write(&staged, std::process::id().to_string()).unwrap();
    std::fs::rename(&staged, &pid_file).unwrap();
    std::thread::sleep(Duration::from_secs(3600));
}

fn wait_capability(path: &Path) -> [u8; 32] {
    let deadline = Instant::now() + live::wait_deadline();
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            let text = text.trim();
            if text.len() == 64
                && let Ok(bytes) = (0..32)
                    .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16))
                    .collect::<Result<Vec<u8>, _>>()
            {
                return bytes.try_into().unwrap();
            }
        }
        assert!(
            Instant::now() < deadline,
            "reopened shell did not publish its hook capability at {}",
            path.display()
        );
        std::thread::yield_now();
    }
}

#[test]
fn stale_close_of_previous_run_cannot_revoke_or_stop_reopened_run() {
    let live = Live::binary();
    live.ready("feature/stale-close-fence");
    let created = create_terminal(&live, "stale-close", &["/bin/sh", "-c", "exit 0"]);
    let id = created["id"].as_u64().unwrap();
    let old_run = created["run"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    let id_arg = id.to_string();
    json(&live, &["terminal", "acknowledge-stopped", &id_arg]);
    let reopened = json(&live, &["terminal", "reopen", &id_arg]);
    track_groups(&live);
    assert_eq!(reopened["run"], old_run + 1);
    assert_eq!(reopened["phase"], "running");
    let pid = reopened["pid"].as_u64().unwrap() as libc::pid_t;
    let pgid = unsafe { libc::getpgid(pid) };
    assert!(pgid > 1, "reopened shell has no process group");

    // The fresh shell publishes the capability the server issued to run N+1.
    let token_file = live.root.path().join("reopened.token");
    let publish = format!(
        "printf '%s' \"$OVRCR_HOOK_TOKEN\" > '{}'",
        token_file.display()
    );
    json(&live, &["terminal", "send", &id_arg, "--text", &publish]);
    let capability = wait_capability(&token_file);

    // A close captured before the reopen (TUI confirm, CLI inventory, Enter
    // before HierarchyChanged arrives) still names run N. It must conflict
    // without stopping run N+1 or revoking its capability.
    let session = SessionId(id);
    let stale = live.request(Request::CloseTerminal {
        session,
        expected_run: SessionRunId(old_run),
    });
    assert!(
        matches!(
            stale,
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ),
        "stale close must conflict: {stale:?}"
    );
    let current = row(&live, id);
    assert_eq!(current["run"], old_run + 1);
    assert_eq!(current["phase"], "running");
    assert!(pid_exists(pid), "stale close stopped run N+1");
    assert_eq!(
        live.request(Request::AgentReport(AgentReport {
            session,
            capability,
            sequence: None,
            update: AgentUpdate::Activity(AgentActivity::Busy),
        })),
        Response::Ok,
        "stale close revoked the capability of run N+1"
    );

    // The same request naming the current run is the real close.
    assert_eq!(
        live.request(Request::CloseTerminal {
            session,
            expected_run: SessionRunId(old_run + 1),
        }),
        Response::Ok
    );
    assert!(
        live::wait_group_absent(pgid, Duration::from_secs(2)),
        "current-run close left the shell group alive"
    );
    live.forget_group(pgid);
}

#[test]
fn ordinary_natural_exit_requires_ack_and_accepts_acknowledgement() {
    let live = Live::binary();
    live.ready("feature/natural-exit-ack");
    let created = create_terminal(&live, "natural-exit", &["/bin/sh", "-c", "exit 0"]);
    let id = created["id"].as_u64().unwrap();
    let old_run = created["run"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    let exited = row(&live, id);
    assert_eq!(exited["id"], id);
    assert_eq!(exited["run"], old_run);
    assert_eq!(exited["phase"], "exited");
    assert_requires_ack(&exited, true);
    let stopped: i64 = retained_sql(&live)
        .query_row(
            "SELECT stopped FROM retained_sessions WHERE id = ?1",
            [i64::try_from(id).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stopped, 0,
        "natural exit must not persist stop proof: {exited}"
    );

    let id_arg = id.to_string();
    assert_ownership_uncertain(&json_error(&live, &["terminal", "reopen", &id_arg]));
    assert_eq!(row(&live, id)["run"], old_run);

    json(&live, &["terminal", "acknowledge-stopped", &id_arg]);
    let acknowledged = row(&live, id);
    assert_eq!(acknowledged["id"], id);
    assert_eq!(acknowledged["run"], old_run);
    assert_requires_ack(&acknowledged, false);
    let stopped: i64 = retained_sql(&live)
        .query_row(
            "SELECT stopped FROM retained_sessions WHERE id = ?1",
            [i64::try_from(id).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stopped, 1,
        "acknowledgement must persist stop: {acknowledged}"
    );

    let reopened = json(&live, &["terminal", "reopen", &id_arg]);
    track_groups(&live);
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["run"], old_run + 1);
    assert_eq!(reopened["phase"], "running");
}

#[test]
fn acknowledged_natural_exit_close_removes_row() {
    let live = Live::binary();
    live.ready("feature/ack-close");
    let created = create_terminal(&live, "ack-close", &["/bin/sh", "-c", "exit 0"]);
    let id = created["id"].as_u64().unwrap();
    let old_run = created["run"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    let id_arg = id.to_string();
    assert_ownership_uncertain(&json_error(&live, &["terminal", "close", &id_arg]));
    assert_eq!(row(&live, id)["run"], old_run);
    assert_requires_ack(&row(&live, id), true);

    json(&live, &["terminal", "acknowledge-stopped", &id_arg]);
    assert_requires_ack(&row(&live, id), false);
    assert_eq!(
        json(&live, &["terminal", "close", &id_arg]),
        serde_json::json!({"ok": true})
    );
    let remaining = json(&live, &["terminal", "list"]);
    assert!(
        remaining
            .as_array()
            .unwrap()
            .iter()
            .all(|session| session["id"] != id),
        "acknowledged close must remove the row: {remaining}"
    );
}

#[test]
fn acknowledge_stopped_starts_only_the_server_when_cold() {
    let live = Live::binary();
    live.ready("feature/cold-ack");
    let created = create_terminal(
        &live,
        "cold-ack",
        &["/bin/sh", "-c", "while :; do sleep 1; done"],
    );
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    assert_eq!(cli(&live, &["shutdown", "--kill"]).status.code(), Some(0));
    live.join();
    assert!(!live.socket.exists());

    let acknowledged = json(&live, &["terminal", "acknowledge-stopped", &id_arg]);
    assert_eq!(acknowledged["ok"], true);
    let stopped = row(&live, id);
    assert_eq!(stopped["phase"], "stopped");
    assert_requires_ack(&stopped, false);
    assert!(
        live.socket.exists(),
        "cold acknowledgement should start the server"
    );
    assert_eq!(
        json(&live, &["terminal", "list"])
            .as_array()
            .unwrap()
            .iter()
            .filter(|session| session["pid"].is_u64())
            .count(),
        0,
        "acknowledgement must not launch a session",
    );
    assert_eq!(cli(&live, &["shutdown", "--kill"]).status.code(), Some(0));
}
#[test]
fn same_boot_crash_with_surviving_descendant_requires_ack_before_fresh_shell_reopen() {
    let live = Live::binary();
    live.ready("feature/ownership-crash");
    let pid_file = live.root.path().join("survivor.pid");
    let created = create_terminal(
        &live,
        "owned-survivor",
        &[
            "/bin/sh",
            "-c",
            r#"/bin/sh -c '
            trap "" HUP TERM
            printf "%s\n" "$$" > "$1"
            while :; do /bin/sleep 3600; done
        ' owned-descendant "$1" &
        wait"#,
            "owned-survivor",
            pid_file.to_str().unwrap(),
        ],
    );
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    let old_run = created["run"].as_u64().unwrap();
    let session_pgid = unsafe { libc::getpgid(created["pid"].as_u64().unwrap() as libc::pid_t) };
    assert!(session_pgid > 1);
    let descendant = wait_pid_file(&pid_file);
    let descendant_pgid = unsafe { libc::getpgid(descendant) };
    assert!(descendant_pgid > 1, "descendant has no process group");
    live.own_group(descendant_pgid);
    assert_ne!(descendant_pgid, unsafe { libc::getpgrp() });

    crash_server_pid_only(&live);
    assert!(
        pid_exists(descendant),
        "descendant died with the server pid"
    );
    assert!(
        live::group_exists(descendant_pgid),
        "descendant group died with the server pid"
    );

    live.start_binary();
    let restored = row(&live, id);
    assert_eq!(restored["id"], id);
    assert_eq!(restored["run"], old_run);
    assert_eq!(restored["name"], created["name"]);
    assert_eq!(restored["phase"], "interrupted");
    assert!(restored["pid"].is_null(), "{restored}");
    assert!(restored["started_unix_ms"].is_null(), "{restored}");
    assert_requires_ack(&restored, true);
    assert_no_live_work(&live);
    assert!(
        pid_exists(descendant),
        "restart reaped the surviving descendant"
    );

    let refused = json_error(&live, &["terminal", "reopen", &id_arg]);
    assert_ownership_uncertain(&refused);
    assert_eq!(row(&live, id)["run"], old_run);
    assert_no_live_work(&live);
    assert!(
        pid_exists(descendant),
        "unacknowledged reopen launched over a live descendant"
    );

    reap_group(&live, descendant_pgid);
    if session_pgid != descendant_pgid {
        reap_group(&live, session_pgid);
    }
    assert!(
        !pid_exists(descendant),
        "descendant still exists after reap"
    );
    for group in live.owned_groups() {
        if !live::group_exists(group) {
            live.forget_group(group);
        }
    }

    json(&live, &["terminal", "acknowledge-stopped", &id_arg]);
    let acknowledged = row(&live, id);
    assert_eq!(acknowledged["id"], id);
    assert_eq!(acknowledged["run"], old_run);
    assert_eq!(acknowledged["phase"], "stopped");
    assert_requires_ack(&acknowledged, false);
    assert_no_live_work(&live);

    let reopened = json(&live, &["terminal", "reopen", &id_arg]);
    track_groups(&live);
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["name"], created["name"]);
    assert_eq!(reopened["run"], old_run + 1);
    assert_eq!(reopened["phase"], "running");
    assert!(reopened["pid"].is_u64(), "{reopened}");
    json(
        &live,
        &[
            "terminal",
            "send",
            &id_arg,
            "--text",
            "printf 'NEW_%s\\n' SHELL_114; printf 'CWD='; pwd -P",
        ],
    );
    let expected_cwd = live
        .workspace_root
        .join(live::WORKSPACE)
        .canonicalize()
        .unwrap();
    let text = wait_text(&live, id, &format!("CWD={}", expected_cwd.display()));
    assert!(text.contains("NEW_SHELL_114"));
    assert!(
        !pid_exists(descendant),
        "fresh shell reused the old descendant pid"
    );
    assert!(
        !text.contains("owned-survivor"),
        "old command leaked into the replacement shell: {text}"
    );
}

#[test]
fn simulated_prior_boot_uuid_allows_reopen_without_ack() {
    let current = ovrcr::retained::current_boot_id()
        .expect("native boot id reader must work on macOS and Linux");
    let prior = other_boot_uuid(&current);
    let live = Live::binary();
    live.ready("feature/ownership-prior-boot");
    let created = create_terminal(&live, "prior-boot", &["/bin/sh", "-c", "exit 0"]);
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    let old_run = created["run"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    stop_server(&live);

    // Simulated saved boot identity, not an OS reboot.
    patch_saved_boot(&live, id, Some(&prior), 0);
    let offline = json(&live, &["terminal", "list"]);
    let offline_row = offline
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    assert_eq!(offline_row["phase"], "interrupted");
    assert_requires_ack(offline_row, false);
    assert!(!live.socket.exists(), "offline inspect started a server");

    live.start_binary();
    let restored = row(&live, id);
    assert_eq!(restored["phase"], "interrupted");
    assert_requires_ack(&restored, false);
    assert_no_live_work(&live);

    let reopened = json(&live, &["terminal", "reopen", &id_arg]);
    track_groups(&live);
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["run"], old_run + 1);
    assert_eq!(reopened["phase"], "running");
}

#[test]
fn simulated_absent_and_malformed_saved_boot_id_fail_closed() {
    let live = Live::binary();
    live.ready("feature/ownership-boot-fail-closed");
    let absent = create_terminal(&live, "absent-boot", &["/bin/sh", "-c", "exit 0"]);
    let malformed = create_terminal(&live, "malformed-boot", &["/bin/sh", "-c", "exit 0"]);
    let absent_id = absent["id"].as_u64().unwrap();
    let malformed_id = malformed["id"].as_u64().unwrap();
    wait_phase(&live, absent_id, "exited");
    wait_phase(&live, malformed_id, "exited");
    stop_server(&live);

    // Simulated saved boot fields, not an OS reboot.
    patch_saved_boot(&live, absent_id, None, 0);
    patch_saved_boot(&live, malformed_id, Some("not-a-uuid"), 0);
    live.start_binary();

    for id in [absent_id, malformed_id] {
        let restored = row(&live, id);
        assert_eq!(restored["phase"], "interrupted", "{restored}");
        assert_requires_ack(&restored, true);
        let refused = json_error(&live, &["terminal", "reopen", &id.to_string()]);
        assert_ownership_uncertain(&refused);
        assert_eq!(row(&live, id)["run"], restored["run"]);
        assert_eq!(row(&live, id)["id"], id);
    }
    assert_no_live_work(&live);
    let acknowledged = json(
        &live,
        &[
            "terminal",
            "reopen",
            &absent_id.to_string(),
            "--ack-stopped",
        ],
    );
    track_groups(&live);
    assert_eq!(acknowledged["id"], absent_id);
    assert_eq!(acknowledged["run"], absent["run"].as_u64().unwrap() + 1);
    assert_requires_ack(&row(&live, malformed_id), true);
    assert!(row(&live, malformed_id)["pid"].is_null());
}

#[test]
fn missing_recorded_directory_refuses_reopen_without_fallback() {
    let live = Live::binary();
    live.ready("feature/ownership-missing-cwd");
    let created = create_terminal(&live, "missing-cwd", &["/bin/sh", "-c", "exit 0"]);
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    let old_run = created["run"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    stop_server(&live);

    let cwd = live
        .workspace_root
        .join(live::WORKSPACE)
        .canonicalize()
        .unwrap();
    assert!(
        cwd.is_dir(),
        "recorded cwd was already missing: {}",
        cwd.display()
    );
    let hidden = live.root.path().join("hidden-recorded-cwd");
    let decoy = live.root.path().join("decoy-cwd");
    std::fs::rename(&cwd, &hidden).unwrap();
    std::fs::create_dir(&decoy).unwrap();

    live.start_binary();
    let refused = json_error(&live, &["terminal", "reopen", &id_arg, "--ack-stopped"]);
    assert_eq!(refused["error"]["code"], "NotFound", "{refused}");
    let message = refused["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&cwd.display().to_string()),
        "reopen hid the recorded directory: {refused}"
    );
    assert!(
        !message.contains(&decoy.display().to_string()),
        "reopen mentioned a fallback directory: {refused}"
    );
    let kept = row(&live, id);
    assert_eq!(kept["id"], id);
    assert_eq!(kept["run"], old_run);
    assert!(!kept["recovery"]["failure"].is_null(), "{kept}");
    assert_no_live_work(&live);

    std::fs::rename(&hidden, &cwd).unwrap();
    let reopened = json(&live, &["terminal", "reopen", &id_arg]);
    track_groups(&live);
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["run"], old_run + 1);
    assert_eq!(reopened["phase"], "running");
}

#[test]
fn exclusive_registry_lock_does_not_publish_reopen_success() {
    let live = Live::binary();
    live.ready("feature/ownership-storage-lock");
    let created = create_terminal(&live, "storage-lock", &["/bin/sh", "-c", "exit 0"]);
    let id = created["id"].as_u64().unwrap();
    let id_arg = id.to_string();
    let old_run = created["run"].as_u64().unwrap();
    wait_phase(&live, id, "exited");
    let before_groups = live.session_groups();

    {
        let blocker = retained_sql(&live);
        blocker.busy_timeout(Duration::from_millis(50)).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let refused = json_error(&live, &["terminal", "reopen", &id_arg, "--ack-stopped"]);
        assert_ne!(refused["error"]["code"], Value::Null, "{refused}");
        assert_eq!(row(&live, id)["id"], id);
        assert_eq!(row(&live, id)["run"], old_run);
        assert_eq!(
            live.session_groups(),
            before_groups,
            "failed reopen changed live processes"
        );
        assert!(row(&live, id)["pid"].is_null());
    }

    let reopened = json(&live, &["terminal", "reopen", &id_arg, "--ack-stopped"]);
    track_groups(&live);
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["run"], old_run + 1);
    assert_eq!(reopened["phase"], "running");
}
