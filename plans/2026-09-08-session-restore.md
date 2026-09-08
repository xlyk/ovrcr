# Session Restore After Server Loss Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Explicitly saved sessions can be restored after a server crash or reboot by creating new PTYs from durable launch intent, with a Claude Code conversation adapter.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- Opt-in at creation; default sessions, `local` shells, and the operator stay ephemeral.
- Startup never launches anything; restore is explicit, one record at a time, and an interrupted attempt needs an operator acknowledgement that old descendants are gone.
- OVRCR never signals a saved PID and never presents an old process or screen as restored.

**Dependencies:** Independent of the other dashboard plans; the which-key Session group from `plans/2026-09-08-which-key-popup.md` gains `R` for saved-only rows.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

---

### Task 1: Session restore after server loss



**Source:** `plans/archived/2026-09-05-session-restore.md` for the state machine table, storage rules, and step sketches. Decisions below are final.

**Files:**
- Create: `crates/ovrcr-runtime/src/restore.rs`
- Modify: `crates/ovrcr-runtime/src/config.rs` (extract `write_atomic(path, bytes, mode) -> Result<(), SaveFailure>`), `crates/ovrcr-runtime/src/server/{mod,dispatch,startup}.rs`, `crates/ovrcr-runtime/src/session/mod.rs`
- Modify: `crates/ovrcr-protocol/src/wire.rs` and `session.rs` (`restore_mode` on `CreateSessionRequest`, `Request::SavedSessions`, `Request::RestoreSession`, `ack_orphans_gone` on `RemoveSession`, `Response::SavedSessions`, `incarnation` on `SessionSummary`, `SessionPhase::Recoverable { reason }`; bump `PROTOCOL_VERSION`, regenerate the snapshot)
- Modify: `src/cli/{args,mod,output}.rs`, `crates/ovrcr-tui/src/dashboard/{state,render}.rs`
- Test: `tests/server_lifecycle.rs`, `tests/cli.rs`, `tests/tui.rs`, `crates/ovrcr-runtime/src/restore.rs` unit tests

**Decisions:**
- Opt-in at creation: `new --restore-mode relaunch`, or `new --restore-mode claude --conversation-id UUID -- claude`. Default sessions, workspace `local` shells, and the operator stay ephemeral. No retrospective recording.
- Saved intent is the canonical executable (resolved on the server's PATH at creation), exact argv bytes, the canonical workspace path, the mode, and an attempt state. No environment, credentials, screen contents, or transcripts. File mode 0600; argv never appears in list output or errors, and CLI help warns that argv can contain secrets.
- Storage is `<config-filename>.sessions.toml` beside the registry, written with the shared atomic writer, guarded by a lifetime `flock` on `<config-filename>.sessions.lock` acquired before the registry loads or the socket binds; two servers on one config are an ownership conflict. Version 1 schema, unknown fields rejected, at most 512 records and 64 KiB of argv per record; malformed, oversized, or duplicate files fail closed without replacement.
- Session ids are reserved durably before every creation, including ephemeral ones, so a saved id is never reused after restart.
- Startup never launches anything. `session saved` lists records without starting a server. `session restore ID --expected-incarnation N` starts the server if needed and restores exactly one record; there is no automatic or batch restore. An interrupted attempt is `MayBeRunning` and requires `--ack-orphans-gone`, an operator assertion that old descendants are gone; OVRCR never signals a saved PID. `kill` on an uncertain saved-only record refuses with inspection instructions. `session remove` of an uncertain record also needs the acknowledgement. Saved records block workspace removal; `shutdown --kill` refuses unresolved uncertain records; ordinary shutdown may leave safely stopped saved records.
- A restore creates a new PTY, PID, group, incarnation, and start time. Events carry the incarnation and mismatches are discarded. The dashboard shows saved-only rows as "not running" with the restore reason, never an elapsed time since 1970, and the which-key Session group gains `R` "Restore this saved session" with the reason as its disabled text.
- Claude Code is the only adapter: initial argv `[claude, --session-id, uuid]`, restore `[claude, --resume, uuid]`, one input argv element only, a five-second `--help` probe requiring both option tokens before committing intent, refusal when `CLAUDE_CODE_SKIP_PROMPT_HISTORY` is truthy, and a successful spawn reported as "resume launched", never "conversation restored".

**Interfaces:**

```rust
pub struct RestoreFile { pub version: u32, pub next_id: u64, pub records: Vec<SavedSession> }
pub struct SavedSession { pub id: SessionId, pub incarnation: u64, pub project: String, pub workspace: String, pub name: String, pub label: String, pub cwd: Vec<u8>, pub argv: Vec<Vec<u8>>, pub mode: RestoreMode, pub attempt: AttemptState }
pub enum RestoreMode { Relaunch, Claude { conversation_id: String } }
pub enum AttemptState { MayBeRunning, Stopped }
pub fn load(path: &Path) -> Result<RestoreFile>; pub fn save(path: &Path, data: &RestoreFile) -> Result<(), SaveFailure>;
pub fn acquire_owner(config: &Path) -> Result<File>; pub fn decode_spec(record: &SavedSession) -> Result<SessionSpec>;
pub fn launch_argv(record: &SavedSession, restoring: bool) -> Result<Vec<OsString>>; pub fn check_claude(executable: &Path, cwd: &Path) -> Result<()>;
// ServerState::restore_session(id, expected: u64, ack: bool) -> Result<SessionSummary>; saved_sessions() -> Vec<SavedSessionSummary>
```

Lock order is mutation, then sessions, then restore; the dispatcher persists exits under the restore mutex only, after applying the event, and never takes the mutation lock. `SaveFailure { source, replaced }` distinguishes a pre-rename failure (nothing changed) from a post-rename sync failure (durable mutations freeze until restart).

- [ ] **Step 1: RED tests.** Unit: round trip with non-UTF-8 argv, rejection of each malformed shape, oversize refusal, lock conflict between two owners, `launch_argv` for both modes, `check_claude` against a fake `claude` that prints or omits the tokens. Server: `id_reservation_survives_restart`, `intent_is_durable_before_spawn`, `interrupted_attempt_blocks_until_acknowledged`, `restore_reuses_id_and_increments_incarnation`, `stale_incarnation_events_are_discarded`, `saved_records_block_workspace_removal_and_kill_shutdown`. CLI: `session saved` offline, `session restore` with wrong and right incarnation, `--ack-orphans-gone` on remove. TUI: recoverable rows render "not running" and the reason. Crash acceptance: start a saved relaunch session through the real binary, SIGKILL the server, prove the descendant survives and restore is refused, acknowledge, restore, and prove a new PID and incarnation.
- [ ] **Step 2: Implement** in the source plan's order: durable storage and owner lock, intent before spawn, explicit reconciliation and gates, CLI and adapter, then the crash acceptance and documentation.
- [ ] **Step 3: Verify** `rtk proxy cargo test --workspace` plus the crash acceptance run; measure the 50-session lifecycle gate before and after, since each allocation, intent, stop, and remove adds one fsync transaction.

**Gate:** the acceptance list in the source plan: saved intent survives process loss, no old process or screen is ever presented as restored, every launch has durable pre-spawn intent, ambiguous attempts stay recoverable without an automatic duplicate, crash tests prove surviving descendants are refused until acknowledged, and provider identity is unchanged across restore.

---

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Document `--restore-mode`, `session saved`, `session restore`, and `--ack-orphans-gone` in the README "Session lifecycle" section with the secrets warning, and add a smoke-check step to `docs/testing-computer-use.md` that restores a saved session after a server kill.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. Start a session with `--restore-mode relaunch`, kill the server with SIGKILL, run `ovrcr session saved`, restore it with the acknowledgement, and confirm a new PID and incarnation.
2. Run the computer-use smoke check in `docs/testing-computer-use.md`.
