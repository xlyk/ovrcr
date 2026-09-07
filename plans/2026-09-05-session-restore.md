# Session Restore Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Goal:** Restore explicitly saved sessions after server loss by creating new PTYs, with one supported agent conversation adapter.

**Architecture:** Keep the synchronous Rust server as the sole live PTY owner. Persist a small launch-intent file through the existing atomic TOML pattern and reconcile it without spawning anything. Explicit restore reuses a logical session ID, increments its incarnation, and either relaunches its command or resumes its recorded Claude conversation.

**Tech Stack:** Existing Rust, serde, toml, portable-pty, libc, clap, Ratatui, and standard library; no added dependencies.

**Spec:** [MVP design](2026-09-04-ovrcr-mvp-design.md), especially Terminology, Persistent registry, Session lifecycle, Failure handling, and Deferred work; [README roadmap](../README.md#feature-roadmap).

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → multiple dashboards → session restore**. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --workspace --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Global constraints and evidence

- Proposed extension of the MVP's explicit no-restore boundary; the original design remains the baseline for everything else.
- One owner, one active dashboard, synchronous blocking I/O and threads, macOS and Linux, tested 50-session scale.
- No database, async runtime, agent SDK, keeper process, or surviving-process adoption.
- Only registered workspaces; retain canonical-path and Git ownership checks before launch or removal.
- Implementation commands use `rtk`; no implementation, tests, commits, or external mutations were performed while writing this plan.
- Source inspected at `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb` on 2026-09-05; refresh these anchors before execution.
- `crates/ovrcr-runtime/src/config.rs::save_registry_atomic` already uses a same-directory exclusive temporary file, file sync, rename, and directory sync.
- `crates/ovrcr-runtime/src/server/{mod,startup}.rs::create_session_locked` holds the sessions guard during spawn; `next_session_id` currently restarts at 1.
- `run_server` currently locks startup by socket directory, loads only project/workspace TOML, and creates an empty live map.
- `crates/ovrcr-runtime/src/session/mod.rs::SessionSpec` carries `Vec<OsString>`; PTY creation invokes argv directly at the workspace path.
- `wait_for_child` waits for PTY drainage and process-group disappearance before dispatching exit; preserve that ordering.
- Production `ServerState::request_shutdown` currently refuses every retained session without `--kill`, including exited records. The looser `handle_shutdown` helper is test-only and is not the production contract.
- `tests/server_lifecycle.rs` provides real socket/Git/process fixtures, but its in-process fixture cannot safely simulate server SIGKILL.

## Product decisions

1. Saving launch intent is opt-in on creation: `new --restore-mode relaunch`, or `new --restore-mode claude --conversation-id UUID -- claude`.
2. Default sessions and the automatic workspace `local` shell retain their existing ephemeral behavior. No retrospective discovery or recording.
3. An omitted command resolves `$SHELL` once on the requesting CLI, as today; save that exact resolved executable and argv, never reevaluate `$SHELL` during restore.
4. Relaunch saves argument bytes exactly, including empty and non-UTF-8 arguments. Never serialize argv through display strings or a joined shell command.
5. Save the canonical workspace root as cwd bytes. Restore requires the same registered, canonical, Git-owned worktree; changed or missing paths block that record.
6. Do not save environment values, credentials, terminal contents, stdin, shell history, or provider transcripts. Restored processes inherit the current server environment, as initial processes do.
7. Persisted argv can contain secrets supplied as command arguments. CLI help states this before opt-in; file permissions are 0600. No argv in ordinary list output or errors.
8. Resolve argv[0] using the server's current PATH at initial opt-in creation and persist the absolute canonical executable path. Later missing executables fail without PATH substitution.
9. Preserve all remaining argv bytes; do not add a login-shell wrapper, modify permission flags, or inject a prompt during restore.
10. `session saved` lists durable records without starting a missing server. When a server exists, request its authoritative snapshot instead of racing its file writes.
11. `session restore ID --expected-incarnation N` starts the server if needed. Only one named record is restored; there is no automatic restore, batch restore, or retry loop.
12. Startup never launches recorded commands, even after a known reboot. An interrupted attempt requires `--ack-orphans-gone` on the restore request.
13. That flag is an explicit operator assertion that all old descendants are gone, normally following a reboot or manual inspection and cleanup. It is not proof supplied by OVRCR.
14. Without that assertion, a saved interrupted attempt stays visibly blocked. No saved PID or PGID is persisted, probed as ownership proof, or used to send signals.
15. `kill` on a saved-only uncertain record refuses with instructions to inspect old descendants. OVRCR cannot safely terminate those descendants after losing ownership.
16. A normal shell exit or successful managed kill produces a stopped record that can be explicitly restored without the orphan assertion.
17. `session remove ID` removes a stopped saved record durably. An uncertain saved-only record additionally requires `--ack-orphans-gone`; deletion never implies process cleanup.
18. Every retained saved record, including blocked records, prevents workspace removal. `shutdown --kill` refuses unresolved uncertain records and preserves the available server on partial failure.
19. Proposed new behavior: ordinary shutdown may leave safely stopped saved records for later explicit restore; it refuses any live or unresolved uncertain record. Preserve the current refusal for retained ephemeral exited records until they are explicitly removed.
20. A restore creates a new terminal screen, PID, process group, and start time. Detach/reattach to a surviving server continues using its original PTY and screen.

## Durable state and interfaces

Create `crates/ovrcr-runtime/src/restore.rs`, exported from `crates/ovrcr-runtime/src/lib.rs`. Keep launch metadata separate from the existing project/workspace schema; root `src/lib.rs` remains the application facade.
Store it beside the canonical config path as `<config-filename>.sessions.toml`; use `<config-filename>.sessions.lock` for a lifetime owner lock.
Canonicalize the parent, reject symlink state/lock files, and acquire nonblocking exclusive `flock` before loading either registry or binding a server socket.
Hold the lock file until server teardown; set close-on-exec so PTY children cannot retain it. Never unlink a lock file while using it.
Different sockets sharing one config must not become two owners; return a concrete ownership conflict. Preserve the existing socket startup lock separately.

```rust
// Derive Clone, Debug, PartialEq, Eq, Serialize, Deserialize on durable data types;
// reject unknown fields and schema versions. RestoreStore is runtime state.
pub struct RestoreFile {
    pub version: u32,                  // exactly 1
    pub next_id: u64,                  // 1..=i64::MAX, checked increments
    pub records: Vec<SavedSession>,
}
pub struct SavedSession {
    pub id: SessionId,
    pub incarnation: u64,              // 0 before any launch, then increasing
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: String,
    pub cwd: Vec<u8>,
    pub argv: Vec<Vec<u8>>,            // argv[0] absolute executable bytes
    pub mode: RestoreMode,
    pub attempt: AttemptState,
}
pub enum RestoreMode { Relaunch, Claude { conversation_id: String } }
pub enum AttemptState { MayBeRunning, Stopped }
pub struct RestoreStore {
    pub path: PathBuf,
    pub data: RestoreFile,
    pub write_blocked: bool,           // ambiguous post-rename failure
    pub storage_error: Option<String>, // last commit failure, no argv/secrets
}
pub fn load(path: &Path) -> anyhow::Result<RestoreFile>;
pub fn save(path: &Path, data: &RestoreFile) -> Result<(), SaveFailure>;
pub fn acquire_owner(config: &Path) -> anyhow::Result<File>;
pub fn decode_spec(record: &SavedSession) -> anyhow::Result<SessionSpec>;
pub fn launch_argv(record: &SavedSession, restoring: bool) -> anyhow::Result<Vec<OsString>>;
pub fn check_claude(executable: &Path, cwd: &Path) -> anyhow::Result<()>;
```

`SaveFailure { source: anyhow::Error, replaced: bool }` identifies whether rename succeeded; implement `Debug`, `Display`, and `std::error::Error` so the sketched `?` conversions work.
Extract `config::write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), SaveFailure>` from the existing writer; registry saving delegates with its existing permissions policy.
The restore writer always supplies 0600. Sync newly created parent directories where needed, file contents, then the containing directory after rename.
On pre-rename failure, discard only this call's temporary file and keep memory/disk unchanged. On post-rename sync failure, reload visible bytes for diagnostics and block further durable mutations until restart; do not claim rollback or launch a process.
Missing state means a version-1 empty store; present empty, malformed, oversized (>1 MiB), unknown-version, duplicate-ID/name, invalid UUID, NUL-containing argv/cwd, nonabsolute cwd/executable, and overflowing counters fail closed without replacement.
Cap saved records at 512 and aggregate decoded argv per record at 64 KiB; refuse oversize serialization before opening a temporary file.
Use Unix `OsStrExt::as_bytes` / `OsStringExt::from_vec`; permit empty nonzero argv elements, reject empty argv[0].
The current `SessionId(u64)` stays public. Reserve each ID durably before *every* session creation, even ephemeral creation; only the counter is persisted for default sessions.
This small counter write prevents a saved ID being reused by an ephemeral session after restart. Never decrement or recycle it after failure/removal; old MVP ephemeral IDs have no cross-restart identity guarantee.

### Attempt state machine and crash boundaries

| Boundary | Durable record | Restart behavior |
|---|---|---|
| Validation or initial save fails before rename | Previous complete file | No new process or successful response |
| Intent committed, before PTY spawn | New incarnation, `MayBeRunning` | Block until explicit orphan assertion |
| Child spawned, before live-map registration/response | Same committed intent | Block; old descendants may survive |
| Live process registered and response lost | Same intent; live map authoritative | Retry of old expected incarnation returns same live attempt |
| Exit event queued, before dispatcher applies final bytes | `MayBeRunning` | Block; do not infer complete cleanup |
| Final exit applied, before stopped record synced | `MayBeRunning` | Conservative block despite possible actual exit |
| Stopped update fully synced | `Stopped` | Explicit restore allowed |
| Remove rename succeeds but directory sync fails | Visible file may lack record | Partial failure; freeze durable mutations, no workspace deletion |

There is deliberately no post-spawn “running commit.” `MayBeRunning` is the durable conservative truth until completed cleanup.
The live map supplies current running/exited status. This avoids claiming atomicity between fork and filesystem writes.
Any spawn error after intent commit is conservatively uncertain, even if it likely occurred before fork; report ID/incarnation and no successful launch claim.
Existing owned handles remain in the live map on later failures so kill/cleanup remain possible; do not erase them merely because persistence failed.
If spawn fails before returning a handle, the record still blocks replacement; never manufacture a live handle from an observed or stored PID.

### Request and presentation changes

Add `restore_mode: Option<RestoreMode>` to `CreateSessionRequest`; update every Rust constructor explicitly.
Add `Request::SavedSessions`, `Request::RestoreSession { session: SessionId, expected_incarnation: u64, ack_orphans_gone: bool }`, and the acknowledgement flag to `RemoveSession`.
Add `Response::SavedSessions(Vec<SavedSessionSummary>)`; do not expose saved argv through the hierarchy.
Define `SavedSessionSummary { id: SessionId, incarnation: u64, project: String, workspace: String, name: String, label: String, state: RecoveryState }`.
Define `RecoveryState { Stopped, NeedsOrphanAcknowledgement, Blocked(String), Live }`; compute workspace/executable errors rather than rewriting them on every list.
Extend `SessionSummary` with `incarnation: u64` (ephemeral sessions use 1) and add `SessionPhase::Recoverable { reason: String }` for saved-only hierarchy rows.
Saved-only summaries use `pid: None`, `started_unix_ms: 0`; UI renders “not running” and a restore reason, not elapsed time since 1970.
Use new `ServerState::restore_session(id: SessionId, expected: u64, ack: bool) -> Result<SessionSummary>` and `saved_sessions() -> Vec<SavedSessionSummary>`.
All launch/remove/kill/shutdown decisions remain serialized by `mutation_lock`; preserve sessions-guard-through-spawn ordering.
Dispatcher exit persistence takes only the restore-store mutex, after applying the session event; never take `mutation_lock` there because kill waits for the dispatcher.
Maintain lock order: mutation → sessions → restore; dispatcher drops session-map guards before locking restore. Never hold restore while waiting for exit.
Add `incarnation` to `SessionSpec` and `SessionEvent::{Output, Exited}`; reject mismatched event incarnations at dispatch/application.
Before replacement, require the previous owned session's completed exit and joined threads; clear selected-screen state and request a fresh snapshot.

## Supported conversation adapter

Claude Code is the only adapter in this plan. `--session-id UUID` chooses initial conversation identity; `--resume UUID` resumes that identity without `--fork-session` or “most recent” lookup. These flags were verified in [official CLI documentation](https://code.claude.com/docs/en/cli-reference) and installed `/Users/xlyk/.local/bin/claude` version `2.1.260` help on 2026-09-05.
Claude owns conversation persistence and retrieval; OVRCR does not duplicate or parse its transcripts. See [Claude session documentation](https://code.claude.com/docs/en/sessions).
Require a caller-supplied standard 36-character hexadecimal UUID (hyphens at 8/13/18/23); normalize to lowercase and reject duplicate Claude conversation IDs across saved records.
The adapter accepts exactly one input argv element, its executable. Additional provider options, prompts, nested shells, background modes, forks, and automatic identity discovery are outside this first adapter.
Generate initial `[exe, "--session-id", uuid]` and restored `[exe, "--resume", uuid]`; never retry failed resume as a fresh conversation.
Probe the exact canonical executable with `--help` before committing initial or restored intent; require success and the literal option tokens `--session-id` and `--resume`.
This is a conservative support policy: absence from help means “capability not verified,” even though the official docs note that help can omit some flags.
Use `Command` directly at recorded cwd, null stdin, piped stdout/stderr drained concurrently with a 64-KiB combined cap, and a five-second monotonic deadline; kill/reap only the directly owned probe child on timeout and return an error.
Do not block forever joining pipe readers if a descendant inherits a probe pipe: use nonblocking pipe reads with `poll` and close them at the deadline.
Reject adapter launch if `CLAUDE_CODE_SKIP_PROMPT_HISTORY` is set to a truthy value in the current server environment; do not silently unset it.
Changed authentication, settings, HOME, deleted transcripts, or provider-side behavior can still cause resume failure. Show the actual agent terminal/exit; successful PTY spawn means “resume launched,” never “conversation restored.”
No dependency on agent hooks, context accounting, or a transcript-discovery API is introduced.

## Current integration paths and task checkpoints

### Workspace handoff

- Durable restore state, owner locking, decode/launch helpers, and storage failures belong in `crates/ovrcr-runtime/src/restore.rs`; registry/config persistence remains in `crates/ovrcr-runtime/src/config.rs`. Runtime startup, server lifecycle, and session/incarnation checks belong in `crates/ovrcr-runtime/src/server/{startup,mod,dispatch,connections,outbound}.rs` and `crates/ovrcr-runtime/src/session/{mod,process,io}.rs`.
- Shared `SavedSessionSummary`, `RecoveryState`, `RestoreMode`, and wire request/response records belong in `crates/ovrcr-protocol/src/{session,wire}.rs` and are consumed through the public `ovrcr_protocol` re-exports; the `session` and `wire` modules are currently private. The client presentation belongs in `crates/ovrcr-tui/src/dashboard/{mod,state,input,render,event_loop}.rs`.
- The resource CLI is split across root `src/cli/{mod,args,resources,output}.rs`; `src/main.rs` stays a thin entry point. CLI lifecycle checks use `rtk proxy cargo test -p ovrcr --test resource_cli -- --nocapture` and root behavior uses `rtk proxy cargo run -p ovrcr --`.
- Package-focused checks are `rtk proxy cargo test -p ovrcr-runtime --lib restore_`, `rtk proxy cargo test -p ovrcr-protocol --lib restore_`, and `rtk proxy cargo test -p ovrcr-tui --lib restore_` as applicable. Crash/lifecycle acceptance remains in root `tests/session_restore.rs` and `tests/server_lifecycle.rs`; existing GUI files remain root-owned and are adapted only for changed protocol fields.

This plan runs last because it adds durable identity to all earlier features. Break Task 2 into compiling checkpoints: (a) reserve IDs and add incarnation fields/constructors; (b) commit opt-in launch intent before spawn; (c) restore an existing record; (d) wire lifecycle persistence. Do not combine disk failure injection, provider probing, and crash acceptance in the first edit. Complete Task 1's commit-boundary tests before introducing a process spawn that depends on them.

In Task 1, wire startup explicitly: acquire the config-derived owner lock, then the existing socket startup lock; load/validate registry and restore state before binding the socket; construct ServerState with `restore: Mutex<RestoreStore>`; release only the socket startup lock after binding. Keep the config owner File alive until server threads/owned sessions are torn down. Test a second socket using the same config is refused and can acquire the owner lock after the first owner exits. In Task 2, remove the independently authoritative `next_session_id: AtomicU64`; allocation reads and commits `store.data.next_id` under mutation/store locks, including automatic local shells. Update server unit fixture constructors with isolated temporary stores.

After `Session::spawn` returns an Arc, the immediate next step under the held sessions guard is insertion into the owned map. Put no fallible disk/provider/dashboard work between those operations. Perform fallible validation before spawn; publication failure after insertion retains the map entry. Errors before spawn returns leave conservative MayBeRunning and no fabricated handle; retain Session's existing internal spawn cleanup behavior. Extend the ready-callback registration test to prove a concurrently arriving startup event/report finds the inserted handle once the map guard is released.

For restore retries, look up the durable record first for eligibility/expected incarnation, then the owned map under mutation serialization. The live-map exception applies only when **both** the saved record and owned Session have the same `expected + 1` incarnation. That owned summary takes presentation precedence, including Exited; any disagreement is Conflict, never another spawn. Extend the retry test to let the first launch finish before replaying the lost-response request and prove its launch-count marker remains one.

Add `RestoreStore::commit(&mut self, next: RestoreFile) -> Result<(), SaveFailure>` and use it for **every** counter/intent/stopped/removal write. It rejects an already write-blocked store before touching disk; successful save swaps memory; pre-rename failure keeps old memory; post-rename failure sets `write_blocked = true`, keeps conservative authoritative state, and records the storage error for presentation. Record pre-rename errors as well; a later successful commit clears `storage_error`. Diagnostic reloading must not silently make a failed operation successful. The bare save snippet in Task 2 must call this method. Check the signed TOML counter limit before increment: `next_id == i64::MAX as u64` means allocation is exhausted, never an out-of-range file write.

When storage is blocked, refuse creates/restores/removal/workspace deletion before side effects. **Owned process cleanup still runs** through kill/close/shutdown; failed stopped/deletion persistence returns PartialFailure and keeps the owned record plus the durable conservative record. It never reports success just because the PGID is gone. The dispatcher may attempt the stopped commit after final Exited, but it must not take `mutation_lock` because termination waits for that event. A failed commit cannot revert Exited into Running or remove the last cleanup handle.

`restore_session` returns `Response::CreatedSession(current_summary)`. Retry identity is `(id, incarnation)`, not byte equality of a changing summary: a completed attempt can become Exited or acquire new activity between replies. For `current == expected + 1`, return the current owned attempt even if Exited; assert one launch and stable identity rather than identical response payloads. Another restore uses that current incarnation explicitly. Use checked arithmetic for expected+1. No owned attempt after restart means no idempotence proof, so require a new request against the saved current incarnation.

`decode_spec` only decodes validated saved bytes and fields; it never calls `launch_argv`. `launch_argv` may consume that decoded argv and append the adapter's exact flags once. At launch, set the new incarnation and fresh hook identity after decoding. Reset activity/context and parser/history; capabilities and report watermarks never come from disk. Saved-only summaries use Unknown activity, no context, no PID, and no elapsed-runtime display. Add `incarnation` to pane/screen readiness identity and invalidate matching frozen histories/copy jobs before replacing a logical ID; reject old raw events before parsing, then publish a fresh authoritative screen/control epoch for its viewers.

The resource CLI is part of Tasks 2–4. `NewArgs` is shared by `new` and `terminal create`: both accept the same opt-in flags. Update `terminal_value` for incarnation and `phase: "recoverable"` with a separate recovery reason, null PID/exit fields, and unknown activity/context. Include saved-only records in online `Inspect` inventory and terminal counts exactly once by ID; saved records with no registered parent remain visible through `session saved`. Offline resource inventory uses the same owner-locked read policy as `session saved` and must not silently report zero saved terminals. `ReadTerminal`, SendTerminal, pause/resume, and control takeover on a saved-only ID return Conflict with the recovery reason. `CloseTerminal` on an owned saved session cleans the group then durably removes its record; on a saved-only uncertain record it refuses and directs the explicit remove/acknowledgement path.

Add `tests/resource_cli.rs` to constructor and acceptance scope. Add `restore_resource_cli_matches_saved_lifecycle`: opt-in `terminal create`, exit/restart, compare saved/terminal inventory, refuse text/input on saved-only state, restore by expected incarnation, and close with both record removal and PGID absence. Use the file's own Fixture for live CLI cases; the crash task's new subprocess fixture remains local to `tests/session_restore.rs`, with no import of private ControlFixture. Put uncertain saved-only CloseTerminal refusal in the subprocess crash test, where uncertainty is real. Run the exact resource test (one executed) and the existing resource regression. Keep deterministic fake-provider gates separate from actual provider resume acceptance.

## Task 1: Durable intent and single-writer storage

**Files:** create `crates/ovrcr-runtime/src/restore.rs`; modify `crates/ovrcr-runtime/src/{config,lib}.rs` and `crates/ovrcr-runtime/src/server/startup.rs::run_server`; tests colocated in runtime restore/config modules.
**Interfaces:** produce the durable types, `load`, `save`, `acquire_owner`, `SaveFailure`, and `write_atomic` defined above.

- [ ] Add a byte-preserving round-trip test, corruption tests, file-mode assertion, and two-owner lock refusal.

```rust
#[test]
fn restore_round_trip_keeps_non_utf8_and_empty_args() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sessions.toml");
    let record = SavedSession { id: SessionId(7), incarnation: 1,
        project: "p".into(), workspace: "w".into(), name: "agent".into(),
        label: "sh".into(), cwd: b"/tmp/w".to_vec(),
        argv: vec![b"/bin/sh".to_vec(), vec![255], vec![]],
        mode: RestoreMode::Relaunch, attempt: AttemptState::MayBeRunning };
    let data = RestoreFile { version: 1, next_id: 8, records: vec![record] };
    save(&path, &data).unwrap();
    assert_eq!(load(&path).unwrap(), data);
    assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
}
```

- [ ] Run `rtk proxy cargo test -p ovrcr-runtime --lib restore::tests::restore_round_trip_keeps_non_utf8_and_empty_args -- --exact`; initial failure must be the absent module/interface, then exactly 1 passing test after implementation.
- [ ] Extract the atomic byte writer without changing project/workspace format. Carry a `replaced` boolean set only after successful `fs::rename`.

```rust
let mut replaced = false;
let result = (|| -> anyhow::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    replaced = true;
    File::open(parent)?.sync_all()?;
    Ok(())
})();
result.map_err(|source| SaveFailure { source, replaced })
```

- [ ] Keep existing exclusive temp-name allocation/cleanup. Use a test-only closure seam at before-rename/after-rename checkpoints to inject errors, never production environment failpoints.
- [ ] Add `restore_corruption_keeps_original_bytes`, `restore_atomic_failure_reports_commit_boundary`, `restore_owner_lock_excludes_another_socket`, and `restore_validation_rejects_ambiguous_identity`.
- [ ] Run `rtk proxy cargo test -p ovrcr-runtime --lib restore_ -- --nocapture`; require at least 5 nonzero passing tests, including old-file/new-file byte assertions and lock reacquisition after owner drop.

## Task 2: Save launch intent before creating a process

**Files:** modify `crates/ovrcr-runtime/src/server/{mod,startup,dispatch,connections,outbound}.rs`, `crates/ovrcr-runtime/src/session/mod.rs`, `crates/ovrcr-protocol/src/{session,wire}.rs`, and root `src/cli/{args,resources,mod}.rs`; update affected struct literals in root `src/gui.rs`, `tests/cli.rs`, `tests/server_lifecycle.rs`, `tests/tui.rs`, and `tests/terminal_acceptance.rs`.
**Interfaces:** consume Task 1 store; produce optional `restore_mode`, `incarnation`, and `restore_session` state transitions.

- [ ] Add `restore_launch_intent_precedes_spawn` and `restore_ephemeral_id_never_collides` server unit tests using the existing ready-callback seam plus a new test-only pre-spawn checkpoint.

```rust
// Inside the pre-spawn callback, after a successful durable commit:
let disk = crate::restore::load(&state_path).unwrap();
assert_eq!(disk.records[0].incarnation, 1);
assert_eq!(disk.records[0].attempt, AttemptState::MayBeRunning);
assert!(disk.next_id > disk.records[0].id.0);
assert!(!child_marker.exists()); // executable writes this only when launched
```

- [ ] Run `rtk proxy cargo test -p ovrcr-runtime --lib restore_ -- --nocapture`; first show the new assertions failing, then require at least 2 passing tests for this task.
- [ ] Resolve/validate workspace, executable, names, command bytes, and optional adapter; check duplicate names against the union of saved and live records.
- [ ] Under the mutation lock, clone durable state, reserve the ID and incarnation with checked arithmetic, then save before touching the PTY.

```rust
let next_id = store.data.next_id.checked_add(1).context("session ID exhausted")?;
let mut next = store.data.clone();
next.next_id = next_id;
// For opt-in creation, push the validated record with incarnation=1,
// attempt=MayBeRunning; ephemeral creation persists only the counter.
store.commit(next)?; // handles pre/post-rename failure and write_blocked
```

- [ ] Release the store guard before spawning; keep the existing sessions guard until registration. Every post-commit error returns `PartialFailure` with logical ID/incarnation and blocks another launch without acknowledgement.
- [ ] Add incarnation to output/exit events and discard stale events before parser writes. For ephemeral creation, a failure burns its reserved ID but does not create a saved record.
- [ ] Test actual command argv/cwd output, unset/changed `$SHELL`, injected pre-rename failure with no child marker, and after-rename failure with no launch; require at least 6 server `restore_` tests when these cases are added.

## Task 3: Explicit reconciliation, idempotence, and lifecycle gates

**Files:** modify `crates/ovrcr-runtime/src/server/{startup,mod,dispatch,connections,outbound}.rs` startup/create/kill/remove/shutdown/dispatcher/hierarchy paths and `crates/ovrcr-runtime/src/restore.rs`; extend root `tests/server_lifecycle.rs`.
**Interfaces:** consume saved/live identity and produce `saved_sessions`, `RecoveryState`, and incarnation-guarded restore.

- [ ] Add `restore_startup_never_launches`, `restore_retry_returns_existing_attempt`, and `restore_records_guard_workspace_removal` as exact integration test names.

```rust
// Request-level idempotence, using a disposable server and fixture session ID:
let request = Request::RestoreSession { session: id,
    expected_incarnation: 1, ack_orphans_gone: true };
let first = fixture.request(request.clone());
let again = fixture.request(request);
let summary = |response| match response {
    Response::CreatedSession(summary) => summary,
    other => panic!("expected restore success, got {other:?}"),
};
let first_summary = summary(first);
let again_summary = summary(again);
assert_eq!((first_summary.id, first_summary.incarnation),
    (again_summary.id, again_summary.incarnation));
assert_eq!(std::fs::read_to_string(&launch_count).unwrap(), "1\n");
```

- [ ] The `fixture` is the existing `ControlFixture`; create `launch_count` under its temporary directory and have the spawned shell append once before waiting on PTY input.
- [ ] Reject expected-incarnation mismatches with `Conflict`, except `current == expected + 1` with an existing live attempt: return that attempt without spawning, even if it has since exited.
- [ ] On restart there is no live attempt to prove a lost response's result; return the saved current incarnation and require a newly issued request against that incarnation.
- [ ] A valid restore increments incarnation and commits `MayBeRunning` before spawn. A stopped record allows the operation directly; an interrupted record requires the acknowledgement flag.
- [ ] Validate cwd through registry equality, canonical containment, and Git worktree-list membership; do not demand a clean worktree for restore because ongoing work may be dirty.
- [ ] After the dispatcher applies the final exit event, persist `Stopped`; keep filesystem errors visible as `Blocked` and preserve the process exit truth in the live map.

```rust
if summary.incarnation == record.incarnation
    && matches!(summary.phase, SessionPhase::Exited { .. }) {
    record.attempt = AttemptState::Stopped;
}
// Save a cloned file before swapping in-memory durable data; on error retain
// the conservative previous attempt and surface the storage failure.
```

- [ ] Kill waits for existing complete termination, then flushes the stopped record before success. Remove persists deletion before forgetting a saved/live entry; a repeated removal of a missing ID remains `NotFound` and does not recreate anything.
- [ ] Shutdown preflights unknown orphan records before signaling owned groups, persists each completed stop, and reports per-ID partial failures without setting stopping on failure.
- [ ] Modify production `ServerState::request_shutdown`, not only the test helper; add assertions covering stopped saved versus stopped ephemeral records under ordinary shutdown.
- [ ] Add `restore_shutdown_partial_failure_keeps_records` and `restore_old_incarnation_output_is_ignored`; run `rtk proxy cargo test -p ovrcr --test server_lifecycle restore_ -- --nocapture` and require at least 5 passing tests.

## Task 4: CLI listing, recoverable rows, and Claude adapter

**Files:** modify root `src/cli/{mod,args,resources,output}.rs`, `crates/ovrcr-protocol/src/{session,wire}.rs`, `crates/ovrcr-runtime/src/restore.rs`, and `crates/ovrcr-tui/src/dashboard/{mod,state,input,render}.rs`; tests in root `tests/{cli,tui}.rs` and runtime restore modules.
**Interfaces:** produce the CLI/protocol/presentation contracts and `launch_argv`/`check_claude` defined above.

- [ ] Add parser tests `restore_cli_requires_explicit_conversation_id` and `restore_cli_requires_expected_incarnation`; invoke the binary with malformed arguments and assert nonzero status plus no created socket.
- [ ] Implement clap flags on `new` and `session restore/remove`; reject `--conversation-id` unless mode is Claude and require it for that mode.
- [ ] Implement direct-byte argv assembly with tests for initial identity, resume identity, and unsupported capability rejection.

```rust
fn launch_argv(record: &SavedSession, restoring: bool) -> anyhow::Result<Vec<OsString>> {
    let spec = decode_spec(record)?;
    Ok(match &record.mode {
        RestoreMode::Relaunch => spec.argv,
        RestoreMode::Claude { conversation_id } => vec![spec.argv[0].clone(),
            if restoring { "--resume".into() } else { "--session-id".into() },
            conversation_id.into()],
    })
}
// In restore_claude_uses_exact_identity, given a validated Claude record:
assert_eq!(launch_argv(&record, true).unwrap()[1], OsString::from("--resume"));
assert_eq!(launch_argv(&record, true).unwrap()[2], OsString::from(uuid));
```

- [ ] Implement bounded help probing with direct argv and nonblocking pipes; test missing flags, nonzero exit, timeout, and inherited open pipe using local fake executables, with no real authentication or model calls.
- [ ] `session saved` acquires the owner lock for an offline read; if locked while its socket is unavailable, return a busy/conflict error instead of pretending the state is current.
- [ ] Merge saved-only rows into hierarchy by ID; failed workspace registration remains visible in `session saved` even when no hierarchy parent exists.
- [ ] Selecting a saved-only row clears the previous terminal and displays its recovery reason; Input/Resize reject it, and Enter stays in browse mode.
- [ ] Test `restore_rows_show_no_pid_or_elapsed_runtime` using the existing Ratatui TestBackend; assert “not running,” no 1970-derived runtime, cleared prior screen, and no input request.
- [ ] Run `rtk proxy cargo test -p ovrcr-runtime --lib restore_ -- --nocapture` plus the root CLI/TUI filters; require all prior tests plus at least 2 CLI, 1 TUI, and 5 adapter unit tests; record actual nonzero counts by target.

## Task 5: Real crash acceptance and documentation

**Files:** create `tests/session_restore.rs`; modify `README.md` only during implementation to document opt-in, new PTYs, orphan acknowledgement, environment policy, and adapter limits.
**Interfaces:** consume the completed public CLI and protocol; no production-only crash flags.

- [ ] Build a subprocess fixture using `env!("CARGO_BIN_EXE_ovrcr")`, isolated config/socket, real disposable Git repo/worktree, and explicit `server` child handle. `crash()` must kill and wait/reap that exact child before restart; normal cleanup shuts down and wait/reaps the restarted child, confirms socket removal and all tagged groups absent, and retains the fixture with an error if any bounded cleanup fails. Never assume SIGKILL performed the server's normal shutdown cleanup.
- [ ] Expose fixture methods `start()`, `crash()`, `request(Request) -> Response`, and `wait_marker(&Path, &[u8])`; bounded socket/marker waits use a monotonic deadline and assert the observed state.
- [ ] Use a shell child that ignores HUP and starts a descendant; exchange its readiness marker through PTY output before killing the server child handle with SIGKILL.

```rust
// Create control_fifo with libc::mkfifo(..., 0o600) in the disposable fixture.
// Open it O_RDWR|O_NONBLOCK in the test before launching, so open cannot hang.
let argv = vec![OsString::from("/bin/sh"), OsString::from("-c"),
    OsString::from("trap '' HUP; printf '1\\n' >> \"$2\"; \
        (trap '' HUP; read release < \"$1\") & \
        descendant=$!; printf 'READY %s %s\\n' \"$$\" \"$descendant\"; \
        wait \"$descendant\""),
    OsString::from("ovrcr-restore-fixture"), control_fifo.as_os_str().to_owned(),
    launches.as_os_str().to_owned()];
// After crash/refusal assertions, write b"release\n" to the owned FIFO.
// Observe both tagged PIDs and their original group disappear before restore.
```

```rust
let old_pid = live.pid.unwrap();
fixture.crash();
fixture.start();
assert!(matches!(fixture.request(Request::RestoreSession {
    session: live.id, expected_incarnation: live.incarnation,
    ack_orphans_gone: false,
}), Response::Error { code: ErrorCode::Conflict, .. }));
assert_eq!(std::fs::read_to_string(&launches).unwrap(), "1\n");
assert_eq!(unsafe { libc::kill(old_pid as i32, 0) }, 0);
```

- [ ] Keep cleanup authority in test-owned child/process fixtures, not saved PID records. Tell the descendant to exit over a test-owned FIFO, then verify its PID/group disappearance before acknowledging orphan cleanup and restoring.
- [ ] Add exactly named tests: `restore_crash_blocks_surviving_descendants`, `restore_new_pty_keeps_id_changes_incarnation`, `restore_failed_spawn_is_not_automatically_retried`, and `restore_reboot_equivalent_has_no_old_screen`.
- [ ] The reboot-equivalent test explicitly finishes every fixture process, restarts the owner, and asserts new PID/PTY, empty previous screen, preserved metadata/argv/cwd, and no launch before explicit restore. It does not claim to reboot the host.
- [ ] Add `restore_claude_missing_transcript_does_not_start_fresh` using a fake CLI that exits nonzero for its exact saved UUID; assert one resume invocation and no subsequent initial-conversation invocation.
- [ ] Run `rtk proxy cargo test -p ovrcr --test session_restore -- --nocapture`; require exactly 5 passing tests and confirmed cleanup, including a child that actually survives owner death.
- [ ] Run `rtk proxy cargo test --workspace --all-targets --all-features`, `rtk proxy cargo test -p ovrcr --features gui --test gui`, and `rtk proxy cargo fmt --all -- --check`; require nonzero test totals and no failed lifecycle tests. Refresh exhaustive variant matches in the root GUI if compilation identifies them.
- [ ] Manually use a disposable workspace and installed supported Claude CLI: save UUID at launch, establish a harmless memorable conversation, stop the original process, restart OVRCR, explicitly resume, and verify that exact prior conversation in the new PTY. Record CLI version and observed result; do not claim adapter acceptance if auth/transcripts prevent it.

## Acceptance, risks, and dependencies

- Saved intent survives process loss; old terminal contents and processes are never represented as restored. Running, stopped, blocked, and resume-launched states are distinguishable.
- Every launch has durable pre-spawn intent, every completed durable mutation is synced, and any ambiguous write/attempt remains recoverable without an automatic duplicate launch.
- Crash tests prove surviving descendants exist and replacement is refused; no product code signals a saved PID or adopts an orphan PTY.
- Provider identity is explicit and remains unchanged across restore; shell relaunch makes no conversation-resumption promise.
- Permission, byte fidelity, corruption, lock ownership, current environment, path validation, partial failure, retries, and deletion/shutdown gates have concrete tests above.
- Filesystem guarantees depend on successful platform fsync/rename semantics; directory-sync failure stays visible. Do not claim to simulate power-loss hardware guarantees with a process-kill test.
- Operator orphan acknowledgement can be mistaken. This deliberately conservative first release cannot prove descendant absence across server loss; stronger proof requires a separately designed durable process owner.
- Synchronous storage adds one small fsync transaction per allocation/intent/stop/remove, never per output chunk. Measure the 50-session lifecycle gate before adding batching.
- Claude help checks are a conservative capability gate, not a proof of conversation availability; real adapter acceptance remains a distinct manual result.
- No sibling roadmap plan is required. Coordinate overlapping protocol/TUI edits if other features land first; discard stale incarnation events regardless of future hook integration.
- If hooks/context/pause/mouse/multiple-dashboard work lands first, every process/input operation must require an owned live handle for the current incarnation; a `Recoverable` row never satisfies that gate. Reset reported activity, usage, pause state, and terminal capabilities when incarnation changes; old reports and client selections cannot attach those values to the replacement process.
- Self-review completed against the original design and current source: restore is an explicit scope extension, ownership/drainage gates remain, proposed tests are not presented as results, and no implementation is authorized by this document.
