# Pause and Resume Controls Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Goal:** Let CLI and dashboard users pause and resume the process group owned by a live OVRCR session without losing its PTY, screen, or cleanup guarantees.

**Architecture:** Add a paused lifecycle phase and explicit Unix STOP/CONT controls to the existing synchronous session owner. Reuse the session termination mutex, server mutation mutex, framed protocol, and dashboard metadata delivery. Keep agent activity separate and preserve the existing final-output/exit ordering.

**Tech Stack:** Rust 2024, blocking threads and mutexes, existing libc/portable-pty, Serde/bincode, Clap, Ratatui/Crossterm; no additional dependencies.

**Spec:** [README feature roadmap](../README.md#feature-roadmap), checkbox “Pause and resume controls for sessions.” The [original MVP design](2026-09-04-ovrcr-mvp-design.md), especially Session lifecycle, Terminal data flow, Failure handling, and Verification, supplies the retained constraints. Its Deferred work section excludes pause/resume from the original MVP; this proposal adds that feature only.

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → multiple dashboards → session restore**. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Global constraints

- “OVRCR owns every session it manages. It cannot adopt a process launched through another terminal or PTY.”
- “The server stores live sessions in memory.”
- “OVRCR does not use Tokio, a database, a shell-command builder, or an agent SDK.”
- “Only one dashboard may remain connected.”
- “Responses and lifecycle events are preserved; if the bounded queue cannot accept them, the server disconnects the dashboard so it can reattach.”
- “Tests use bounded waits and observable process, PTY, Git, and socket state. They do not use arbitrary sleeps as proof of lifecycle completion.”
- Retain macOS/Linux support, 50 live sessions, bounded lossless output queues, and five-second production termination grace.
- Planning changes only this document. Implementation tasks below are future work; no tests or implementation commands were run to prepare it.

## Source grounding and proposed defaults

Inspected HEAD: `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb`. The README roadmap and resource CLI documentation are committed; preserve unrelated sections.

| Current source | Consequence for this feature |
| --- | --- |
| `src/session.rs`: `SessionPhase`, `SessionState`, `summary` | Extend the existing phase enum; do not add a parallel lifecycle cache. |
| `Session::terminate`, `terminate_lock`, `verify_owned_group`, `should_signal_group`, `signal_group` | Share serialization and ownership checks; `signal_group` currently treats ESRCH as success and must expose disappearance to pause/resume. |
| `Session::write` checks state before blocking on its writer | Admission must reject paused input without holding lifecycle/state locks across PTY writes. |
| `wait_for_child` calls `child.wait()`, then waits for reader drainage and group disappearance | STOP/CONT are not final exit; retain this waiter and do not add a competing waitpid consumer. |
| `ServerState::kill_session`, `request_shutdown`, `remove_session` | Keep mutation ordering; removal currently rejects only Running and must reject every non-Exited phase. |
| `run_dispatcher`, `dispatch_session_event`, `ServerEvent::SessionChanged` | Publish refreshed summaries through the existing dispatcher, avoiding captured stale paused summaries after exit. |
| `src/tui.rs`: `busy_sessions`, `key_action`, `input_request`, `tree_line_text` | Keep explicit activity intact; paused status takes display priority without changing busy/idle data. |
| `tests/server_lifecycle.rs`: `ControlFixture`, backpressured-input test, cleanup helpers | Extend established real socket/process tests and preserve the kill-under-backpressure gate. |
| `tests/tui.rs`, `tests/terminal_acceptance.rs` | Verify exact dense rows and real dashboard keyboard input; reuse existing fixtures. |

These defaults require review before implementation:

1. Add top-level `ovrcr pause ID` and `ovrcr resume ID`, matching existing `kill ID`. Browse-mode `p` pauses and `r` resumes the selected session; explicit commands avoid a stale-state toggle race.
2. `SessionPhase::Paused` means OVRCR successfully sent SIGSTOP to its validated original PGID. It is control state, not proof that every possible descendant has stopped. Signal delivery is asynchronous; output already in a PTY or dispatcher queue can still appear.
3. Both operations revalidate the original PGID and send their signal even when the recorded phase already matches. Repeating the operation succeeds without a new state transition, while reasserting the requested control after an external signal.
4. Missing IDs return NotFound. Exited sessions, vanished groups awaiting final exit publication, unsafe ownership, and server-stopping refusals return Conflict with concrete text. Do not mark a vanished group Paused or Running merely because kill returned ESRCH.
5. Newly admitted PTY input is rejected while Paused. Input admitted before pause may finish writing or remain buffered and execute after resume. This feature does not purge user input or guarantee cancellation of an in-flight write.
6. A paused session remains selectable, resizable, attached, and non-removable. Wall-clock elapsed time keeps advancing. Resume restores Running without resetting parser, PID, start time, or activity data.
7. Kill and shutdown send SIGTERM, then SIGCONT to a still-owned existing group, then start the normal grace interval. Send CONT even when OVRCR recorded Running, because external job control may have stopped it. SIGKILL remains the fallback.
8. External SIGCONT/SIGSTOP and interactive shell job control are not monitored. A shell can create foreground/background PGIDs different from the original PGID, and descendants can call setsid/setpgid. These controls cover members of the recorded group only; do not walk or adopt arbitrary process trees.

## Scope, files, and interfaces

Modify only these implementation files when this proposal is accepted:

- `src/session.rs`: phase, pause API, signaling result, admission in both `write` and `send_text`, stopped-group termination, focused unit tests.
- `src/protocol.rs`: pause/resume request variants and a dispatcher refresh command; protocol round-trip test.
- `src/server.rs`: serialized control methods, lifecycle guards, refreshed metadata delivery, server unit tests.
- `src/main.rs`: CLI variants, dispatch, and phase suffix in list output.
- `src/tui.rs`: browse controls, paused input guard, existing-row status and footer.
- `tests/server_lifecycle.rs`: real process helper and lifecycle/control acceptance using existing fixtures.
- `tests/cli.rs`, `tests/tui.rs`, `tests/terminal_acceptance.rs`: user-facing acceptance.
- `README.md`: only lifecycle/control documentation and this roadmap checkbox after all gates pass.

No new modules, manifests, dependencies, persistent state, agent adapters, process-tree manager, scheduler, pause timers, CPU accounting, panes, GUI controls, or session restoration.

Defined production interfaces:

```rust
// src/session.rs; retain existing derives and Exited payload.
pub enum SessionPhase { Running, Paused, Exited { code: Option<u32>, signal: Option<String> } }
pub fn set_paused(&self, paused: bool) -> anyhow::Result<bool>; // Session method; true = phase changed
fn signal_group(pgid: libc::pid_t, signal: libc::c_int) -> anyhow::Result<bool>; // false = ESRCH
// src/protocol.rs additions to existing enums:
Request::PauseSession { session: SessionId }
Request::ResumeSession { session: SessionId }
DispatchMessage::RefreshSession { session: SessionId }
// src/server.rs; method queues refresh after each completed control attempt.
pub fn set_session_paused(&self, id: SessionId, paused: bool) -> anyhow::Result<()>;
// src/tui.rs; reuse DashboardAction::Request and existing request IDs.
fn pause_request(&mut self, paused: bool) -> DashboardAction;
```

The enum-variant lines above describe additions, not standalone Rust declarations. No wire-version negotiation is introduced: server and clients must use the same binary version, as required by the original design.

## Current integration paths and task checkpoints

Treat the resource CLI as a second PTY input path. In Task 1, the shared admission check must cover both `Session::write` and `Session::send_text`; the latter currently encodes bracketed paste and writes text plus optional carriage return under one writer lock. Preserve that atomic text/submit operation. Check lifecycle before blocking I/O, without holding `terminate_lock` or the state mutex during the write.

In Task 3, update `ServerState::send_terminal` as well as dashboard Input routing. A paused precheck or a pause racing admission must return structured `Conflict`, not `Internal`; an already admitted write retains the documented exception. Update `terminal_value` in `src/main.rs` to emit `phase: "paused"`, null exit fields, and the existing live PID/start time. The new commands use `mutate_without_start` and preserve the global `--json` envelope.

Termination has three production entry points: `kill_session`, `close_terminal`, and `request_shutdown`. All must retain TERM→CONT→grace→KILL cleanup and refreshed metadata on partial failure. `CloseTerminal` removes only after successful cleanup; `RemoveSession` still refuses Paused. Test-only `handle_shutdown` is not proof of production shutdown behavior.

Add `tests/resource_cli.rs` to Tasks 3/6's allowed files. Using its own `Fixture`, add `pause_resume_resource_cli_preserves_input_and_close_contract`: capture the managed PGID; pause; require JSON phase `paused`; require `terminal send` with its default submit behavior to fail with Conflict; resume and observe a unique child-generated acknowledgement; pause again and close; require record removal and PGID absence. Exercise the rejected send through the resource CLI, not a direct `Session::write` call. Run `rtk proxy cargo test --test resource_cli pause_resume_resource_cli_preserves_input_and_close_contract -- --exact --nocapture` and require one executed test.

Before proceeding from Task 3, compile every `SessionPhase` match in `src/main.rs`, `src/tui.rs`, `src/server.rs`, and optional GUI code. Before finishing, rerun `terminal_cli_drives_real_session_and_preserves_workspace_removal_guards` in `--test resource_cli` with `--exact`; require one executed regression. Hooks and context are later plans: do not introduce their fields here.

## Task 1: Serialize process-group controls and protect lifecycle state

**Files:** Modify `src/session.rs`; tests remain in its existing `tests` module.

**Consumes:** Existing `Session`, `SessionState`, `terminate_lock`, process ownership checks, `dispatch_test_events`, `wait_for_screen`.
**Produces:** `SessionPhase::Paused`, `Session::set_paused(bool) -> Result<bool>`, signaling presence result, admission errors.

- [ ] Add `pause_resume_phase_and_input_admission` using `spawn_test_shell`, with a termination guard on every panic/error path. Wait for an explicit printed READY marker before controlling it.

```rust
assert!(session.set_paused(true).unwrap());
assert_eq!(session.summary().phase, SessionPhase::Paused);
assert!(!session.set_paused(true).unwrap());
assert!(session.write(b"SHOULD_NOT_BE_ACCEPTED\r").is_err());
assert!(session.set_paused(false).unwrap());
assert!(!session.set_paused(false).unwrap());
session.write(b"printf 'RESUME_%s\\n' ACK\r").unwrap();
assert!(wait_for_screen(&session, "RESUME_ACK", Duration::from_secs(2)));
session.terminate(Duration::from_millis(200)).unwrap();
assert!(session.set_paused(true).is_err());
assert!(session.set_paused(false).is_err());
```

- [ ] Run `rtk proxy cargo test --lib session::tests::pause_resume_phase_and_input_admission -- --exact --nocapture`. Expected RED: missing phase/method; after implementation expect exactly 1 passing test.
- [ ] Implement the phase and control method with this lock order and transition core:

```rust
let _control = self.terminate_lock.lock().unwrap();
let mut state = self.state.lock().unwrap();
if matches!(state.phase, SessionPhase::Exited { .. }) { bail!("session has exited"); }
verify_owned_group(self)?;
let signal = if paused { libc::SIGSTOP } else { libc::SIGCONT };
if !signal_group(self.pgid, signal)? { bail!("PTY process group no longer exists"); }
let next = if paused { SessionPhase::Paused } else { SessionPhase::Running };
let changed = state.phase != next;
state.phase = next;
self.state_changed.notify_all();
Ok(changed)
```

- [ ] Return `Ok(false)` for ESRCH and `Ok(true)` for successful delivery from `signal_group`; retain concrete errors for other errno values. Existing termination call sites may discard the boolean because disappearance is a successful cleanup condition.
- [ ] Reject Paused in both `write` and `send_text` using the same short admission check: `bail!("session is paused; resume it before sending input")`. Release the state lock before acquiring the writer or performing blocking I/O. Never acquire `terminate_lock` in `write`.
- [ ] Add `pause_resume_exit_event_cannot_be_overwritten` and `pause_resume_rejects_unsafe_group`. Let a shell exit naturally; place a Barrier in the test dispatcher immediately before applying its real Exited event, after reader completion and group disappearance. Race event application with a control attempt and require final Exited plus a control refusal. Never inject Exited while live fixture processes remain: that would bypass signaling and hang cleanup joins. Unit-test invalid PGIDs `0`, `1`, and own process group at the validation boundary without sending signals to them.
- [ ] Run `rtk proxy cargo test --lib pause_resume_ -- --nocapture`; expect at least 3 passing tests. Unit state checks are preliminary; actual group-stop evidence is Task 4.

## Task 2: Preserve graceful termination of stopped groups

**Files:** Modify `src/session.rs`; add focused tests in its existing module.

**Consumes:** Task 1 serialization and signal presence result.
**Produces:** Existing `terminate(Duration) -> Result<()>` retains its signature and cleanup guarantees for paused/external-stopped sessions.

- [ ] Add `pause_resume_terminate_runs_term_handler`: spawn `sh -c` with `trap 'printf TERM_HANDLED; exit 0' TERM; printf READY; while :; do read line; done`, await READY, stop it, terminate with a short grace, and require TERM_HANDLED in the final parsed screen. Use Task 4's multi-process helper for descendant handler coverage.
- [ ] Run `rtk proxy cargo test --lib session::tests::pause_resume_terminate_runs_term_handler -- --exact --nocapture`. Expected RED: final marker missing without CONT; later exactly 1 pass.
- [ ] Under the existing termination lock, replace the initial signaling sequence:

```rust
if should_signal_group(self)? {
    signal_group(self.pgid, libc::SIGTERM)?;
    if should_signal_group(self)? && signal_group(self.pgid, libc::SIGCONT)? {
        let mut state = self.state.lock().unwrap();
        if matches!(state.phase, SessionPhase::Paused) {
            state.phase = SessionPhase::Running;
            self.state_changed.notify_all();
        }
    }
}
let deadline = Instant::now() + grace;
```

- [ ] Keep the existing verified KILL escalation, `wait_until_exited`, join of reader/waiter, and final group-absence check. Do not hold the state or sessions-map mutex while waiting. For termination, SIGCONT returning false/ESRCH means the group disappeared after TERM; continue existing exit/drain/join checks without inventing a Running transition. Other SIGCONT errors must return an error; shutdown must retain the server and report partial failure rather than pretend cleanup finished.
- [ ] Add `pause_resume_terminate_serializes_competing_controls`: start pause, resume, and terminate on Arc clones behind a Barrier; all threads must finish within a channel deadline, final phase must be Exited, and the PGID must be absent. Valid losers may return Conflict after termination; none may revive an exited summary.
- [ ] Run `rtk proxy cargo test --lib pause_resume_ -- --nocapture`; expect at least 5 passing tests. Run `rtk proxy cargo test --lib session::tests::terminate_removes_the_whole_process_group -- --exact --nocapture`; expect exactly 1 pass.

## Task 3: Expose serialized controls, safe removal, and current metadata

**Files:** Modify `src/protocol.rs`, `src/server.rs`, and `src/main.rs`; test `tests/cli.rs` and existing protocol/server unit modules.

**Consumes:** Task 1 `set_paused` and Task 2 termination behavior.
**Produces:** Defined Request/DispatchMessage variants, `ServerState::set_session_paused`, top-level CLI commands, visible phase in List.

- [ ] Add `pause_resume_requests_round_trip`, serializing both requests and a Paused `SessionSummary` through existing frame helpers. Run `rtk proxy cargo test --lib protocol::tests::pause_resume_requests_round_trip -- --exact`; expect missing variants initially and exactly 1 pass afterward.
- [ ] Add the request variants and server method. Acquire `mutation_lock`, call `reject_if_stopping`, clone the Arc from `sessions`, release the map guard, then call `set_paused`. Missing ID uses existing `lifecycle_error(NotFound, ...)`; other control errors use Conflict through `error_for_lifecycle`.
- [ ] Enqueue `DispatchMessage::RefreshSession { session: id }` before releasing the mutation guard after a control attempt on an existing session. Dispatcher looks up the session and sends `SessionChanged(session.summary())`; do not enqueue an earlier captured summary. Missing records at dispatch time are ignored. Use `try_send` for this refresh, not a potentially blocking bounded-channel send. Full/closed dispatch returns PartialFailure when the signal succeeded, or preserves the original Conflict with notification failure details when it did not. Do not undo a delivered signal. Take an owner-checked `dashboard_snapshot` and call existing `disconnect_dashboard` on refresh failure so a still-connected TUI cannot indefinitely display stale control state; a new hierarchy/List reads current session truth. Test queue saturation and disconnected dispatch with no stuck control thread.
- [ ] Use the same refresh message after `kill_session` attempts and after each shutdown termination attempt, including failures: CONT may have changed Paused to Running even if later cleanup failed. Preserve the original operation error if notification also fails, with both causes in the returned message.
- [ ] Do not make exit application acquire `terminate_lock`: termination waits for dispatcher-applied Exited and would deadlock. Control signals change phase under the short state mutex; the final Exited event remains authoritative. The refresh handler reads current state, so a refresh queued before exit cannot restore a captured Paused value after it.
- [ ] Replace removal's Running-only condition with an Exited-only permission:

```rust
if !matches!(session.summary().phase, SessionPhase::Exited { .. }) {
    return Err(lifecycle_error(ErrorCode::SessionRunning, "session is still live; kill it before removal"));
}
```

- [ ] Inspect every `SessionPhase::Running` comparison. Keep busy animation's Running-only check; update live-session guards and the test-only `handle_shutdown` guard to include Paused. Workspace removal already checks all records; production shutdown already refuses all records without `--kill`.
- [ ] Add `Command::Pause { id: u64 }` and `Command::Resume { id: u64 }` using `mutate_without_start(request, json_output)` in the current `run` dispatcher. A missing server returns NotFound without launching one. Support the existing global `--json` success/error envelope. Preserve the first three list fields; append `running`, `paused`, or `exited` after the session name.

```rust
let phase = match session.phase {
    SessionPhase::Running => "running",
    SessionPhase::Paused => "paused",
    SessionPhase::Exited { .. } => "exited",
};
println!("    session {} {} {phase}", session.id.0, session.name);
```

- [ ] Add `pause_resume_cli_requires_id` in `tests/cli.rs`: invoke both commands without ID and with a nonnumeric ID; require nonzero status and Clap's diagnostic, without starting a server. Add `pause_resume_server_refuses_removal_and_late_mutation` using existing server fixtures; paused removal must return SessionRunning and accepted shutdown must reject later controls.
- [ ] Run `rtk proxy cargo test --test cli pause_resume_cli_requires_id -- --exact`; expect exactly 1 pass. Run `rtk proxy cargo test --lib pause_resume_server_ -- --nocapture`; expect at least 1 pass.

## Task 4: Prove group behavior through real process and socket evidence

**Files:** Modify `tests/server_lifecycle.rs` only; reuse `ControlFixture`, request framing, group absence checks, and cleanup conventions.

**Consumes:** Tasks 1–3. **Produces:** A test-only process fixture and five ordinary integration tests, plus one ignored helper entry point used only as a child executable.

- [ ] Define the following test-local interfaces:

```rust
struct PausePeer { pid: libc::pid_t, pgid: libc::pid_t, address: PathBuf }
fn pause_resume_child_fixture(); // #[test] #[ignore]; env selects leader or descendant role
fn recv_pause_ready(socket: &std::os::unix::net::UnixDatagram) -> PausePeer;
fn wait_peer_stopped(peer: &PausePeer, timeout: Duration);
fn send_peer_command(socket: &std::os::unix::net::UnixDatagram, peer: &PausePeer, token: &str);
fn expect_peer_reply(socket: &std::os::unix::net::UnixDatagram, token: &str);
```

- [ ] Implement the helper by launching `current_exe()` with `--ignored --exact pause_resume_child_fixture --nocapture` inside the managed PTY. Pass fixture paths/role with per-child environment; do not mutate the parent test environment. The leader launches the same helper as a descendant with inherited PGID. Build the managed argv explicitly:

```rust
let argv = vec![
    OsString::from("env"),
    OsString::from("OVRCR_PAUSE_ROLE=leader"),
    OsString::from(format!("OVRCR_PAUSE_DIR={}", dir.path().display())),
    std::env::current_exe().unwrap().into_os_string(),
    "--ignored".into(), "--exact".into(),
    "pause_resume_child_fixture".into(), "--nocapture".into(),
];
```
- [ ] Each helper binds a distinct UnixDatagram address, reports PID/PGID and READY to the parent socket, and echoes nonce commands. The leader also polls raw, no-echo PTY input and reports input tokens. Use libc termios/poll already available; handle EINTR. Register TERM with existing `signal_hook::flag`, send `TERM_ACK:<pid>` before exit, and have the leader reap its descendant before printing `FINAL_AFTER_TERM`.
- [ ] `wait_peer_stopped` calls `ps -o stat= -p PID` with fixed argv on macOS/Linux and bounded polling until the returned state starts with `T`; assert `getpgid(peer.pid) == peer.pgid`. Both READY handshakes must establish the same owned PGID before any pause assertion. Production code must not invoke ps.
- [ ] Add `pause_resume_stops_group_and_rejects_input`: create fixture workspace and managed helper, select it through a dashboard socket, receive both READY messages, pause through control socket, and observe both peer processes in stopped state. A post-pause Input request must fail; send a unique datagram nonce to each stopped peer, resume, and require both exact replies. Send a different PTY input token after resume and require its reply with no rejected token present. Verify ID/PID/current screen persist, and paused removal/shutdown-without-kill refuse.

```rust
assert_eq!(fixture.request(Request::PauseSession { session: id }), Response::Ok);
wait_peer_stopped(&leader, Duration::from_secs(2));
wait_peer_stopped(&descendant, Duration::from_secs(2));
send_peer_command(&control_socket, &leader, "RESUME_LEADER_1");
send_peer_command(&control_socket, &descendant, "RESUME_DESCENDANT_1");
assert_eq!(fixture.request(Request::ResumeSession { session: id }), Response::Ok);
expect_peer_reply(&control_socket, "RESUME_LEADER_1");
expect_peer_reply(&control_socket, "RESUME_DESCENDANT_1");
```

- [ ] Add `pause_resume_kill_runs_group_handlers`: stop both peers, KillSession, require both TERM_ACKs and FINAL_AFTER_TERM in the retained final screen, Exited publication, PID/PGID absence, then successful removal. Repeat the external-stop case by sending SIGSTOP directly while the summary remains Running.
- [ ] Add `pause_resume_shutdown_cleans_stopped_groups`: pause two managed fixtures, shutdown with kill, require handler evidence, absent groups, joined server, and removed socket. This verifies the shutdown path rather than inferring it from KillSession.
- [ ] Add `pause_resume_control_races_converge`: synchronize separate socket clients with a Barrier for pause/resume/kill. Require bounded responses, only documented successes/conflicts, eventual Exited, final parsed marker where TERM handlers run, and absent group. For the reaped-leader case, let the descendant ignore SIGHUP before READY and give the leader an `EXIT_LEADER` datagram command that acknowledges then exits without waiting for its child. Wait until the leader PID is absent, assert the descendant still belongs to the recorded PGID, pause/resume that survivor, then kill it and verify final drainage and absence.
- [ ] Add `pause_resume_backpressured_input_keeps_controls_available`: extend the existing blocked-input setup with control-client pause then resume before kill. Control responses and List must complete while the dashboard input write is blocked. This exercises the admission exception and proves pause does not wait on the PTY writer.
- [ ] Use socket read timeouts, bounded process-state polling, and cleanup guards that confirm all recorded PGIDs disappear even on assertions. Never prove pause solely by quiet output or an elapsed sleep. Preserve fixture paths if cleanup fails.
- [ ] Run `rtk proxy cargo test --test server_lifecycle pause_resume_ -- --nocapture`; expect exactly 5 ordinary passing tests and 1 ignored helper, not zero matches. Run the existing `backpressured_input_and_send_do_not_block_inspect_or_kill` filter with `--exact`; expect exactly 1 pass.

## Task 5: Add dense dashboard controls and paused presentation

**Files:** Modify `src/tui.rs`, `tests/tui.rs`, and `tests/terminal_acceptance.rs`.

**Consumes:** Paused phase, pause/resume requests, current SessionChanged delivery.
**Produces:** Browse `p`/`r`, visible paused status, guarded keyboard/paste paths, and real terminal acceptance.

- [ ] Add `pause_resume_browse_keys_send_explicit_requests` using `dashboard_fixture`. `p` emits PauseSession, `r` emits ResumeSession, each with the selected ID and a fresh request ID. No selected session or Exited emits no request and displays a short refusal. In Running terminal mode, both characters remain ordinary PTY bytes.
- [ ] Implement `pause_request(bool)` using `find_session`, current selection, and `next_request_id`; do not optimistically alter phase on a keypress. Server updates determine the rendered status. Keep repeats idempotent and the request method independent of busy_sessions.
- [ ] Add `pause_resume_input_and_paste_stay_guarded`: Enter on Paused stays in Browse with `Session paused; press r to resume`. If a SessionChanged/Hierarchy update pauses the selected terminal, move it to Browse; retain Ctrl-g handling. Also guard `input_request` so runtime `event_to_request`, key, and paste paths cannot bypass state checks. A stale dashboard request remains protected by server admission.
- [ ] Add `pause_resume_dense_status_has_priority`: use TestBackend and fixed timestamp. Put ASCII `P` in the existing one-character status slot, replacing the spinner while paused; append `paused` to selected metadata. Keep the exact three-line hierarchy, full-width selection, tree connectors, ctx field, clipping, and terminal geometry.

```rust
let status = if matches!(session.phase, SessionPhase::Paused) {
    'P'
} else if dashboard.session_is_busy(*id) {
    SPINNER_FRAMES[((now_unix_ms / 100) % SPINNER_FRAMES.len() as u64) as usize]
} else { ' ' };
```

- [ ] Footer in Browse includes `p pause  r resume`; paused selection makes `r resume` visible before optional hints on narrow layouts. Keep mode and Ctrl-g escape instructions visible in Terminal. Add literal text assertions at normal and small supported dimensions; do not add cards or a sidebar panel.
- [ ] Add `pause_resume_dashboard_round_trip` to existing PTY acceptance: wait for prompt/marker, browse `p`, observe `paused` in the rendered screen and List, detach/reattach and require paused metadata, browse `r`, Enter, send a nonce-producing command, require its output, then run bounded cleanup. A Ctrl-g transition returns to Browse before p/r controls.
- [ ] Run `rtk proxy cargo test --test tui pause_resume_ -- --nocapture`; expect exactly 3 passes. Run `rtk proxy cargo test --test terminal_acceptance pause_resume_dashboard_round_trip -- --exact --nocapture`; expect exactly 1 pass.

## Task 6: Final acceptance and narrow documentation

**Files:** Update `README.md` only after implementation gates; no edits to the approved original design.

- [ ] Add `pause ID`/`resume ID` examples beside kill, document browse p/r, input admission, queued output, stopped-group TERM/CONT cleanup, original-PGID limits, and the distinction from agent activity. Check only this roadmap checkbox after acceptance succeeds.
- [ ] Run `rtk proxy cargo fmt --check`, then `rtk proxy cargo test`. Require a nonzero executed count in every relevant unit/integration target and all new tests; the one explicitly ignored fixture helper is intentional. Record actual counts and outcomes, never substitute planned counts for results.
- [ ] Run `rtk proxy cargo test --test server_lifecycle fifty_sessions_survive_detach_and_leave_no_process_groups -- --exact --nocapture` only if it was not already executed by the preceding suite. Require exactly 1 pass and real PGID cleanup evidence.
- [ ] On macOS and Linux, run the five Task 4 tests with the platform's actual ps and PTY implementation. Record an untested platform as outstanding, not passed. In the existing GUI helper, exercise real p/r input and confirm visible/accessibility-row paused text, resume output, and clean window-close shutdown; no helper code changes are planned.
- [ ] Review the diff against the exact file scope and acceptance criteria below. Hand off results and any blockers; integration, commits, and work-diary handling follow the user's later execution instructions.

## Acceptance, risks, and dependencies

- CLI and TUI share server control logic; an attached or detached live session can pause/resume and reattach without replacement. Agent hooks are not a prerequisite.
- READY handshakes establish two owned same-PGID processes; stopped-state evidence and nonce replies establish real STOP/CONT behavior. Successful syscall return alone is not the acceptance proof.
- Pause/resume never publish Exited; final PTY bytes, group disappearance, and reader/waiter completion still precede removal. Paused records block workspace removal and ordinary shutdown.
- Races serialize on existing locks, input backpressure cannot block controls, and exit wins over late metadata refresh. Kill and shutdown resume stopped handlers before grace and retain explicit cleanup failure reporting.
- A signal delivered just before group exit may report success, followed immediately by Exited. ESRCH before delivery is a conflict. There is no portable atomic ownership-and-signal primitive for an entire PGID: retain current validation and short critical sections, document the inherited PID/PGID reuse limit, and never claim this introduces a process-identity sandbox.
- External job control can change process membership or stop state. No exhaustive descendant freeze, cancellation of remote agent work, or pause of remote services is promised. Network timeouts can expire while a local process is stopped.
- Existing protocol compatibility assumes a single binary version. Rebuild and restart the disposable server for acceptance; do not test new client enums against an older running server.
- Agent-hook plans may extend summaries and activity rendering; coordinate the one-character status priority and preserve their independent activity data. Pane plans may change selection ownership; apply p/r to the focused selected session through their final selection interface. Neither is a dependency for this feature.
- Self-review: roadmap scope covered by Tasks 1–6; every production interface is declared above; no implementation or tests were executed during planning; no stronger all-descendant guarantee is implied by the Paused label.
