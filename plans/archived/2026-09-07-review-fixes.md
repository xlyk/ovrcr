# OVRCR Review Fixes Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Complete the tasks in order; each task is one PR against `main`.

**Outcome:** Every finding from the 2026-09-07 review of `origin/main` at `a4a3d5a` is fixed, tested, and documented, without changing product scope or adding an async runtime.

**Source review:** Twelve tasks below map to the review's high, medium, and low findings. Findings marked *verified* were reproduced by running the built binary; the rest were confirmed by reading the code.

**Baseline on `main`:** `cargo fmt --all -- --check` clean; `cargo test --workspace` green (290 tests, 5 ignored fixtures); `cargo test --features gui --test gui` green (5 tests); **`just lint` fails** (see Task 1).

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- Add a dependency only where a task names one.
- Stay inside each task's file list. Raise `CONCERN:` before touching another file.
- Every behavior change gets the smallest test that would have caught the finding. Skip matrices and speculative coverage.
- Give every live fixture its own `OVRCR_CONFIG`, `OVRCR_SOCKET`, and temporary workspace, per `docs/testing-computer-use.md` and the README.
- Prefix every executable and pipeline stage with `rtk`.
- Before committing: `rtk proxy just verify` (fmt-check, check, lint, test) plus the task's named tests.
- Any change to a `Serialize` type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` (introduced in Task 6) and updates the wire snapshot test.

## Priority order

| Task | Finding | Why this order |
| --- | --- | --- |
| 1 | Lint gate fails | Ten-minute fix that unblocks `just verify` for every later PR |
| 2 | Kill leaks background jobs (*verified*) | Highest impact; orphans survive on every close |
| 3 | 5s per kill, serial shutdown, global lock (*verified*) | Builds on Task 2's signal path |
| 4 | Socket directory chmod (*verified*) | Small, isolated, security-adjacent |
| 5 | Blind server start, no `--version` (*verified*) | Needed to debug everything after it |
| 6 | No protocol version | Touches the handshake on every client path |
| 7 | Corrupt run file blocks startup; O(history) ticks | Scheduler durability |
| 8 | Forced cancel orphans Pi; status gaps; install kills sessions | Scheduler correctness |
| 9 | Dashboard slot leak, accept-loop exit, unremovable workspace | Server robustness |
| 10 | Stale pane, key encoding, palette lock, transcript wrap | Dashboard |
| 11 | Daemon side effects (*verified*), error chains, timeouts, docs | CLI and docs |
| 12 | CI, rust-version, LICENSE, AGENTS.md | Hygiene, last |

---

### Task 1: Make the lint gate pass

**Finding:** `rtk proxy just lint` fails on the current toolchain: clippy `large_enum_variant` on `Response` (`crates/ovrcr-protocol/src/wire.rs:233`) and `ServerEvent` (`wire.rs:261`). The largest variants hold a `SessionSummary` by value.

**Files:**
- Modify: `crates/ovrcr-protocol/src/wire.rs`
- Modify: every match site the compiler reports (runtime, tui, cli)

- [ ] **Step 1: Box the large payloads**

Change `Response::CreatedSession(SessionSummary)` to `CreatedSession(Box<SessionSummary>)` and `ServerEvent::SessionChanged(SessionSummary)` to `SessionChanged(Box<SessionSummary>)`. Serde encodes `Box<T>` identically to `T`, so the wire format does not change. Fix the construction and match sites the compiler reports.

- [ ] **Step 2: Verify**

```sh
rtk proxy just verify
```

**Gate:** `just lint` exits 0. Full suite green.

---

### Task 2: Terminate every process group attached to the session's terminal

**Finding (*verified*):** Signals go only to the process group captured at spawn (`crates/ovrcr-runtime/src/session/process.rs:61`). An interactive shell puts each job in its own group, so anything started with `&` or `nohup` survives `kill`, `terminal close`, and `shutdown --kill`, and the CLI reports success. Reproduced on macOS: `sleep 300 &` inside a `bash -i` session was still alive after `ovrcr kill`, reparented to PID 1. On Linux the PTY stays open, so the session also becomes unremovable, which blocks workspace removal and makes `shutdown --kill` return `PartialFailure` forever. Pause and resume (`session/mod.rs:335`) have the same blind spot.

**Files:**
- Modify: `crates/ovrcr-runtime/src/session/mod.rs`
- Modify: `crates/ovrcr-runtime/src/session/process.rs`
- Test: `crates/ovrcr-runtime/src/session/tests.rs`

**Design:** Ownership is defined by the controlling terminal. At spawn, record the slave device name with `libc::ptsname` on the master fd (fall back to `ps -o tty= -p <leader>` if that returns null). At terminate, pause, and resume time, list attached process groups once with `ps -axo pid=,pgid=,tty=` and select rows whose tty matches. Processes that called `setsid` drop the tty and are intentionally out of scope. Signal the shell's own group first with SIGHUP, which makes bash and zsh hang up their jobs, then apply the existing escalation to every attached group.

- [ ] **Step 1: Add a RED test**

In `session/tests.rs`, `terminate_removes_job_control_subgroups`: spawn `bash --norc +H -i`, write `sleep 300 & echo BGPID=$!\r`, wait until the screen contains `BGPID=`, parse the PID, assert `getpgid(pid) != session.pgid`, call `terminate(Duration::from_secs(5))`, then assert `kill(pid, 0)` fails with `ESRCH`. Time the call and assert under 2 seconds.

- [ ] **Step 2: Record the terminal device**

Add `tty: Option<String>` to `Session`, populated in `spawn_internal` right after `process_group_leader()`.

- [ ] **Step 3: Add `attached_groups`**

In `process.rs`, add `fn attached_groups(tty: &str) -> Result<BTreeSet<libc::pid_t>>` that runs `ps -axo pid=,pgid=,tty=` once and returns distinct pgids whose tty column matches. Apply the existing `verify_group_identity` safety rules to each group (never `<= 1`, never the server's own group). Treat a `ps` failure as "leader group only" and record it in the returned error context rather than aborting.

- [ ] **Step 4: Signal all attached groups**

In `Session::terminate`:
1. Send SIGHUP to `self.pgid`, then SIGTERM and SIGCONT to `self.pgid` and every other attached group.
2. Wait the grace period for all of them.
3. SIGKILL any group still present, then wait again.
4. Keep the existing exit-event and thread-join tail unchanged.

Apply the same group enumeration to `pause` and `resume` so SIGSTOP and SIGCONT reach running jobs, not just the shell.

- [ ] **Step 5: Verify**

```sh
rtk proxy cargo test -p ovrcr-runtime session::
```

Confirm `terminate_removes_the_whole_process_group` (which traps HUP) still passes through the SIGKILL path.

**Gate:** New test passes in under 2 seconds. All `session::` tests pass.

---

### Task 3: Parallel shutdown, and release the mutation lock while waiting

**Finding (*verified*):** `kill` on an interactive shell takes 5.0s because SIGTERM is ignored (Task 2's SIGHUP removes the common case). `request_shutdown` at `crates/ovrcr-runtime/src/server/mod.rs:388` terminates sessions one at a time: measured 10.1s for two. `kill_session` (`mod.rs:290`) and `close_terminal` hold `mutation_lock` for the entire wait, which also gates `DashboardGeometry`, `inventory`, and every create or remove, so a resize during a kill freezes dashboard input. The signal-driven shutdown in `startup.rs:157` inherits all of this.

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/mod.rs`
- Test: `tests/server_lifecycle.rs`

- [ ] **Step 1: Add RED tests**

1. `shutdown_kill_terminates_sessions_concurrently`: with `OVRCR_KILL_GRACE_MS=500`, create four sessions running `sh -c 'trap "" HUP TERM; sleep 300'`, time `Request::Shutdown { kill: true }`, assert under 2 seconds.
2. `kill_does_not_block_dashboard_geometry`: start a kill of a HUP- and TERM-ignoring session on one connection; on a dashboard connection send `DashboardGeometry` and assert it is answered within 500ms.

- [ ] **Step 2: Release the lock around the wait**

In `kill_session` and `close_terminal`: under `mutation_lock`, run `reject_if_stopping`, fetch the session with `session_for_control`, and revoke the hook capability. Drop the guard. Call `terminate`. Re-acquire the guard for `refresh_session_locked`, and treat a `NotFound` there (the record was removed while we waited) as success. Concurrent kills of one session are already serialized by `Session::terminate_lock`.

- [ ] **Step 3: Terminate in parallel during shutdown**

In `request_shutdown`, when `kill` is true, spawn one thread per session that revokes the capability and calls `terminate(Duration::from_secs(5))`, join them all, then run each `refresh_session_locked` and collect failures into the existing `PartialFailure` response. Keep `mutation_lock` held so new sessions cannot appear mid-shutdown.

- [ ] **Step 4: Verify**

```sh
rtk proxy cargo test --test server_lifecycle
```

**Gate:** Both new tests pass. `shutdown_termination_failure_is_partial_and_server_remains_available` still passes.

---

### Task 4: Stop changing permissions on directories OVRCR did not create

**Finding (*verified*):** `run_server` at `crates/ovrcr-runtime/src/server/startup.rs:84-87` runs `create_dir_all(parent)` then `set_permissions(parent, 0o700)` on whatever directory contains the socket. A 755 directory became 700 in testing. With `OVRCR_SOCKET` under `$HOME` this changes the home directory's mode; under `/tmp` it fails with EPERM. Only the socket leaf is symlink-checked, and `.server.lock` is also dropped in that directory.

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/startup.rs`
- Modify: `README.md`
- Test: `tests/server_lifecycle.rs`

**Rule:** If the parent does not exist, create it with mode 0700 using `DirBuilderExt::mode`. If it exists, never modify it: bail when it is a symlink or owned by another user, naming the directory. Do not refuse a shared mode, because `tempfile` creates fixture directories as 0755 and the GUI helper relies on that; instead chmod the bound socket file to 0700 after `bind`, which is what gates `connect`.

- [ ] **Step 1: Add a RED test**

`startup_refuses_shared_existing_socket_directory`: pre-create the fixture's socket parent with mode 0755, assert `run_server` returns an error containing "socket directory". Update any existing test that pre-creates the directory to create it with mode 0700.

- [ ] **Step 2: Implement the rule and document it**

Replace the two calls with a `secure_socket_directory(parent)` helper implementing the rule above. Add one sentence to the README install section: the socket directory must be private, and OVRCR refuses to start otherwise.

**Gate:** New test passes; the existing socket-directory privacy test still passes.

---

### Task 5: Log the detached server and add `--version`

**Finding (*verified*):** `connect_or_start` at `src/client.rs:41-46` spawns `ovrcr server` with stdin, stdout, and stderr set to `/dev/null`, never calls `try_wait` on the child, and there is no log file. A corrupt registry, a refused socket directory, or a corrupt task run file surfaces only as "timed out waiting for server startup" after 5 seconds. `ovrcr --version` does not exist (`src/cli/args.rs:6-8`), so users cannot compare client and server binaries.

**Files:**
- Modify: `src/client.rs`
- Modify: `src/cli/args.rs`
- Modify: `crates/ovrcr-runtime/src/server/startup.rs`
- Modify: `README.md`
- Test: `tests/server_lifecycle.rs`, `tests/cli.rs`

- [ ] **Step 1: Add RED tests**

1. `startup_failure_reports_server_log`: write an invalid `config.toml`, call `connect_or_start` with `OVRCR_SERVER_EXECUTABLE` pointing at the built binary, assert the error contains "parse registry" and the log path, and that it returns in well under 5 seconds.
2. In `tests/cli.rs`: `version_flag_prints_package_version` asserting `ovrcr --version` prints `ovrcr 0.1.0`.

- [ ] **Step 2: Redirect server output to a log**

In `connect_or_start`, open `<socket parent>/server.log` for append with mode 0600 (the directory is private after Task 4) and pass clones as stdout and stderr. In the startup poll loop, call `child.try_wait()`; if the child exited, stop polling immediately and return an error with the exit status, the log path, and the last 20 log lines.

- [ ] **Step 3: Write lifecycle lines from the server**

In `run_server`, print `listening on <socket>` after bind and `stopped` before returning, and print the full error chain with `{error:#}` on failure. The `eprintln!` at `startup.rs:159` already lands in the log.

- [ ] **Step 4: Add `--version`**

Add `#[command(version)]` to `Cli`. Include the protocol version from Task 6 in the string once it exists.

- [ ] **Step 5: Document**

Add a "Troubleshooting" section to the README naming the log path and suggesting `ovrcr server` in the foreground for live output.

**Gate:** Both new tests pass. Full `server_lifecycle` suite passes.

---

### Task 6: Version the protocol

**Finding:** Frames are `u32 length + bincode` with no magic or version (`crates/ovrcr-protocol/src/codec.rs:8-42`). bincode encodes enum variants by index, and `PauseSession` and `ResumeSession` were inserted mid-enum in `wire.rs:147-152` with the same `{ session }` shape as their neighbors. A newer CLI against a still-running older server can silently execute a different request, or hit a decode error with no explanation.

**Files:**
- Modify: `crates/ovrcr-protocol/src/codec.rs`
- Modify: `crates/ovrcr-protocol/src/lib.rs`
- Modify: `crates/ovrcr-runtime/src/server/connections.rs`
- Modify: `src/client.rs`
- Modify: `src/cli/args.rs`
- Test: `crates/ovrcr-protocol/src/codec.rs`, `crates/ovrcr-protocol/src/wire.rs`, `tests/server_lifecycle.rs`

**Design:** Every connection begins with an 8-byte preamble from each side: `OVRC` followed by big-endian `u32` `PROTOCOL_VERSION`. Each side reads the peer's preamble before any frame. On mismatch, close and report both versions.

- [ ] **Step 1: Add RED tests**

1. `codec.rs`: `preamble_round_trip` and `preamble_rejects_wrong_magic_and_version` over a `UnixStream::pair`.
2. `wire.rs`: `request_wire_snapshot`: encode one instance of every `Request` variant and compare against a checked-in list of hex strings. Inserting a variant mid-enum fails this test until the snapshot and `PROTOCOL_VERSION` are both updated. Do the same for `Response` and `ServerEvent`.
3. `tests/server_lifecycle.rs`: `client_with_wrong_protocol_version_is_refused`: write a preamble with `PROTOCOL_VERSION + 1` and assert the server closes the connection; assert `connect_if_running` against a peer answering a different version returns an error containing "protocol version".

- [ ] **Step 2: Implement**

Add `pub const PROTOCOL_VERSION: u32 = 1`, `write_preamble`, and `read_preamble` to `codec.rs`. Server: at the top of `handle_connection`, write then read; on mismatch or read error, drop the stream. The `wake_accept` and stale-socket probes connect and drop immediately, which the server already tolerates. Client: in `connect_if_running`, after connect, write then read; on mismatch return an error naming both versions and suggesting `ovrcr shutdown --kill`. The hook path in `src/report.rs` uses its own deadline I/O and must add the preamble too.

- [ ] **Step 3: Bump discipline**

Comment above `PROTOCOL_VERSION`: any change to a serialized type bumps it and regenerates the snapshot. Task 1 already changed `Response` and `ServerEvent` layouts in a wire-compatible way; confirm the snapshot test proves that.

**Gate:** New tests pass. All integration suites pass, which proves every existing client path sends the preamble.

---

### Task 7: Make the task store tolerate damage and stop rewriting history every tick

**Findings:** `merge_runs` (`crates/ovrcr-runtime/src/task_manager.rs:612`) propagates any unreadable `run.json` with `?`; `TaskManager::open` calls it before the socket is bound (`startup.rs:120`), so one corrupt file stops the server from starting and, once running, fails every mutation every 100ms. `change` (`task_manager.rs:123`) clones the store, re-reads every run file, and `save_atomic` re-validates every task and run spec before rewriting `state.toml`; `tick` (`:173`) does this every 100ms and nothing prunes runs. `discover` (`task_runner.rs:427`) bails on any non-zero `ps` exit, and the supervisor loop treats that as run failure while forking `ps` ten times a second.

**Files:**
- Modify: `crates/ovrcr-runtime/src/task_manager.rs`
- Modify: `crates/ovrcr-runtime/src/tasks.rs`
- Modify: `crates/ovrcr-runtime/src/task_runner.rs`
- Modify: `docs/scheduled-tasks.md`
- Test: `tests/task_execution.rs`, `crates/ovrcr-runtime/src/task_manager.rs`

- [ ] **Step 1: Add RED tests**

1. `corrupt_run_file_does_not_block_open`: write garbage to one `runs/<id>/run.json`, call `TaskManager::open`, assert it succeeds and the run is marked `Interrupted` with an error naming the file.
2. `tick_does_not_reread_terminal_runs`: seed 500 terminal runs, count `read_run` calls across ten ticks via a test hook, assert zero.
3. `transient_ps_failure_does_not_fail_run`: in `task_runner` unit tests, inject a `discover` that fails twice then succeeds; assert the run continues.

- [ ] **Step 2: Tolerate damage**

In `merge_runs`, on a read or id-mismatch failure, log once, set the run's status to `Interrupted` with the error text, and continue. Apply the same policy in `open`.

- [ ] **Step 3: Cut the per-tick cost**

Merge only runs whose status is not terminal. In `save_atomic` validation, validate task specs and only runs that changed since load (track a dirty set on `TaskStore`). Add retention: keep the newest 200 runs per task and anything younger than 30 days; prune older run directories during `tick` after they are terminal, documenting the rule in `docs/scheduled-tasks.md`.

- [ ] **Step 4: Harden discovery**

In the supervisor loop, tolerate three consecutive `discover` failures before failing the run, and lower the scan interval to 250ms.

**Gate:** New tests pass. `task_execution`, `task_runner`, and `task_schedule` suites pass.

---

### Task 8: Clean up forced cancellations and report statuses honestly

**Findings:** After the 20-second cancel deadline, `execute` (`task_manager.rs:359-372`) kills the supervisor's process group, but Pi runs in its own group (`task_runner.rs:115`) and each bash anchor is detached, so they survive. The same code then overwrites `run.json` with `CleanupFailed` even when the supervisor already recorded `Cancelled` (`:373-377`). `RunStatus::Cancelling` is never assigned (`cancel_run`, `:507-514`). `service install` sends `Shutdown { kill: true }` to a managed server (`src/service.rs:271`), terminating live sessions, while `docs/scheduled-tasks.md` promises installation never replaces a running server.

**Files:**
- Modify: `crates/ovrcr-runtime/src/task_manager.rs`
- Modify: `crates/ovrcr-runtime/src/task_runner.rs`
- Modify: `crates/ovrcr-runtime/src/tasks.rs`
- Modify: `src/service.rs`
- Modify: `docs/scheduled-tasks.md`
- Test: `tests/task_execution.rs`, `tests/service.rs`

- [ ] **Step 1: Add RED tests**

1. `forced_cancel_kills_pi_process_group`: use the existing fake-Pi fixture with a supervisor that ignores the cancel command; after the deadline, assert the fake Pi's pgid is gone.
2. `cancel_marks_run_cancelling_immediately`: cancel an active run and assert the next `task get` shows `Cancelling`.
3. `service_install_refuses_when_sessions_exist`: with a managed server holding one session, assert `install` returns an error naming the session count and the server is still running.

- [ ] **Step 2: Own the cleanup in the supervisor**

Install a SIGTERM handler in the supervisor (`signal-hook` is already a dependency) that runs the existing Pi and anchor escalation before exiting. Have the supervisor write its tracked process groups into `run.json` on every scan so the manager can, after SIGKILL, signal any group still listed there.

- [ ] **Step 3: Preserve the supervisor's outcome**

Before writing `CleanupFailed`, re-read `run.json`; if its status is already terminal, keep it.

- [ ] **Step 4: Set `Cancelling`**

In `cancel_run`, set the status to `Cancelling` when the run is active, and persist it.

- [ ] **Step 5: Match the install promise**

In `service::install`, send `Shutdown { kill: false }`; on `SessionsRemain`, return an error telling the user to close sessions or pass a new `--kill-sessions` flag. Update the doc sentence to match.

**Gate:** New tests pass. `task_execution` and `service` suites pass.

---

### Task 9: Server robustness

**Findings:** In `handle_connection` (`crates/ovrcr-runtime/src/server/connections.rs:48-66`), the dashboard slot is registered and then `try_clone().expect(...)` and `spawn(...).expect(...)` can panic before the cleanup block at `:184-200`, leaving the slot owned forever. The accept loop (`startup.rs:175-177`) uses `?` on `thread::Builder::spawn`, so one EAGAIN exits `run_server` and closes every PTY. `remove_workspace_locked` (`mod.rs:701`) calls `inspect_worktree`, which canonicalizes the path (`git.rs:250`), so a workspace whose directory was deleted externally can never be removed.

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/connections.rs`
- Modify: `crates/ovrcr-runtime/src/server/startup.rs`
- Modify: `crates/ovrcr-runtime/src/server/mod.rs`
- Modify: `crates/ovrcr-runtime/src/git.rs`
- Modify: `README.md`
- Test: `tests/server_lifecycle.rs`, `tests/git_lifecycle.rs`

- [ ] **Step 1: Add RED tests**

1. `dashboard_slot_is_released_when_registration_panics`: inject a panic after slot registration via a test hook; assert a second `DashboardHello` succeeds.
2. `accept_loop_survives_thread_spawn_failure`: inject a spawn failure once via a test hook; assert the server still answers `List` afterward.
3. In `git_lifecycle.rs`: `removes_workspace_whose_directory_is_gone`: create a worktree, `rm -rf` it, assert `remove_worktree` succeeds and `git worktree list` no longer shows it.

- [ ] **Step 2: Guard the slot**

Wrap slot ownership in a `DashboardSlotGuard` whose `Drop` runs the cleanup block. Replace the two `expect` calls with error returns.

- [ ] **Step 3: Keep accepting**

On spawn failure, log, sleep 50ms, and continue the loop.

- [ ] **Step 4: Allow removal of a vanished worktree**

In `remove_worktree`, when `workspace.path` does not exist: verify `git worktree list --porcelain` lists the path as `prunable`, verify no sessions reference the workspace, run `git worktree prune`, and remove the registry record. Document the behavior in the README workspace-removal paragraph.

**Gate:** New tests pass. `server_lifecycle` and `git_lifecycle` pass.

---

### Task 10: Dashboard fixes

**Findings:** `select_request` (`crates/ovrcr-tui/src/dashboard/state.rs:1757`) clears `screen_session` but keeps the old parser, and `Output` events for the newly selected session are applied to it (`:1573`) until the screen response arrives. `encode_key` (`dashboard/input.rs:12-19`) drops Ctrl with anything but a letter and ignores modifiers on Enter, Backspace, arrows, Home, End, and paging keys, so Shift+Enter submits instead of inserting a newline. `palette_key` (`palette.rs:158`) swallows Escape while a request is pending, with no timeout. `mark_mouse` (`event_loop.rs:106`) derives the mouse state from the mode rather than the actual capture state. The tasks view re-wraps the entire transcript per frame with two allocations per character (`task_tui.rs:933-947`) at 50ms.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/input.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/event_loop.rs`
- Modify: `crates/ovrcr-tui/src/task_tui.rs`
- Modify: `crates/ovrcr-tui/Cargo.toml` (add `unicode-width`, already in the dependency tree via ratatui)
- Modify: `README.md`
- Test: `tests/tui.rs`

- [ ] **Step 1: Add RED tests**

1. `output_for_newly_selected_session_waits_for_screen`: select session B while A's screen is shown, deliver `Output` for B, assert the parser still shows A untouched or a blank pane, then deliver `Screen` and assert B renders.
2. `control_punctuation_and_modified_keys_encode`: assert Ctrl-space and Ctrl-@ give `0x00`; Ctrl-\ , Ctrl-], Ctrl-^, Ctrl-_ give `0x1c` through `0x1f` (crossterm delivers these as `Char('4'..'7')` with CONTROL on legacy terminals, and as the punctuation itself under kitty keyboard protocol; handle both); Shift-Up gives `ESC [ 1 ; 2 A`; Ctrl-Right gives `ESC [ 1 ; 5 C`; Alt-Backspace gives `ESC 0x7f`; Shift+Enter gives `ESC [ 13 ; 2 u`; Shift-PageUp gives `ESC [ 5 ; 2 ~`.
3. `palette_escape_cancels_pending_request`: with `pending` set, press Escape, assert the palette closes and a late response for that request id is ignored.
4. `transcript_wrap_is_cached_between_frames`: render the tasks view twice with an unchanged transcript and assert the wrap function ran once.

- [ ] **Step 2: Implement**

- `state.rs`: when `screen_session` changes in `select_request`, replace `parser` with an empty parser of `pane_size` and render a one-line "loading" placeholder; gate the `Output` arm on `self.screen_session == Some(session)` instead of `selected`.
- `input.rs`: add `modifier_param(modifiers) -> Option<u8>` (`1 + shift(1) + alt(2) + ctrl(4)`); emit `ESC [ 1 ; m X` for arrows, Home, and End, `ESC [ n ; m ~` for Insert, Delete, PageUp, PageDown, `ESC [ 13 ; m u` for modified Enter, `ESC 0x7f` for Alt-Backspace; map Ctrl punctuation as listed. Leave Ctrl-g reserved.
- `palette.rs`: on Escape with `pending`, record the pending request id in `ignored_responses`, clear `pending`, close the palette.
- `event_loop.rs`: pass the real `mouse_enabled` into `mark_mouse`, and always emit `DisableMouseCapture` on restore since it is idempotent.
- `task_tui.rs`: replace `Line::raw(ch.to_string()).width()` with `unicode_width::UnicodeWidthChar::width`; cache wrapped lines keyed by `(transcript.len(), inner.width)`.
- README: note that Shift+Enter is forwarded as a CSI-u sequence and that terminals without kitty keyboard support cannot distinguish it.

**Gate:** New tests pass. `tui` and `terminal_acceptance` suites pass.

---

### Task 11: CLI behavior and documentation

**Findings:** `kill`, `session remove`, `terminal kill|remove`, `project remove`, and `workspace remove` go through `mutate_started` and auto-start a daemon (*verified*: `ovrcr kill 99` with no server left one running). `RuntimeError::internal` (`src/cli/mod.rs:40`) uses `to_string()`, dropping the cause chain. `send_request` (`:200`) has no timeout. `remove_project` reports "workspaces remain" as `SessionsRemain` (`server/mod.rs:499`). `validate_local_branch` (`git.rs:235`) bypasses `run_git` env scrubbing. Name validation is duplicated between `crates/ovrcr-protocol/src/registry.rs:131` and `git.rs:194`. The `#[cfg(test)]` shutdown shim in `connections.rs:606-625` accepts exited records that production rejects. README says context usage is unknown while documenting it; roadmap boxes for shipped features stay unchecked; `OVRCR_SERVER_EXECUTABLE`, `OVRCR_KILL_GRACE_MS`, and `OVRCR_ENV_FILE` are undocumented.

**Files:**
- Modify: `src/cli/mod.rs`, `src/cli/resources.rs`, `src/cli/args.rs`
- Modify: `crates/ovrcr-protocol/src/wire.rs` (new `WorkspacesRemain` code; bump `PROTOCOL_VERSION`)
- Modify: `crates/ovrcr-runtime/src/server/mod.rs`, `connections.rs`, `git.rs`
- Modify: `README.md`
- Test: `tests/cli.rs`, `tests/resource_cli.rs`, `tests/server_lifecycle.rs`

- [ ] **Step 1: Add RED tests**

1. `mutations_do_not_start_a_server`: with no server, run each of the five commands above and assert the exit is 1, the message says the server is not running, and no socket exists afterward.
2. `runtime_errors_include_the_cause_chain`: point `OVRCR_SOCKET` at an unreadable path and assert the message contains the OS error text.
3. `requests_time_out_against_a_wedged_server`: bind a listener that accepts and never replies; assert the CLI fails within the timeout with "timed out".
4. `remove_project_reports_workspaces_remain`: assert the JSON error code is `WorkspacesRemain`.

- [ ] **Step 2: Implement**

- Route the five mutating commands through `mutate_without_start`.
- `RuntimeError::internal` formats with `{error:#}`.
- Add a 30-second read deadline to `send_request` using the existing `DeadlineIo` from `src/report.rs`, raised to 60 seconds for kill and close.
- Add `ErrorCode::WorkspacesRemain`, use it in `remove_project`, bump `PROTOCOL_VERSION`, update the snapshot.
- Route `validate_local_branch` through `run_git`; delete `git::validate_workspace_name` in favor of the protocol crate's validator.
- Delete the test-only `handle_shutdown` shim and point its tests at `request_shutdown`.
- README: fix the "unknown in the MVP" sentence, tick the shipped roadmap items, add a "Test hooks and advanced variables" subsection documenting the three variables and `OVRCR_PI_EXECUTABLE`, and add `about` text to each top-level command in `args.rs` so `--help` is useful.

**Gate:** New tests pass. `cli`, `resource_cli`, and `server_lifecycle` pass.

---

### Task 12: Repository hygiene

**Findings:** No CI, no LICENSE, no `rust-version` (the workspace uses let-chains, a 1.88 floor, and the README says the GUI helper needs 1.95). `croner = "=3.0.1"` is pinned without a comment. `eframe` enables `x11` for a macOS-only helper. `AGENTS.md` exists only as an untracked file in the main checkout.

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `LICENSE`
- Modify: `Cargo.toml`
- Modify: `README.md`
- Add: `AGENTS.md`

- [ ] **Step 1: CI**

Workflow on push and pull request for `ubuntu-latest` and `macos-latest` running `just verify` with `rtk` available, or the four cargo commands directly if `rtk` is not installable in CI. Set `git config user.name` and `user.email` in the job so Git lifecycle tests can commit.

- [ ] **Step 2: Minimum Rust version**

Set `rust-version = "1.95"` in `[workspace.package]` to match the README's stated requirement, unless `rtk proxy cargo +1.88 check --workspace` passes, in which case use `1.88` and update the README. State the number once in the README install section.

- [ ] **Step 3: License and AGENTS.md**

**Decision needed from the repository owner:** choose a license. Add its text as `LICENSE` and the SPDX id to `[workspace.package]`. Commit `AGENTS.md` from the main checkout if it is meant to be shared; the README and `docs/testing-computer-use.md` already refer to its rules.

- [ ] **Step 4: Dependencies**

Add a comment on the `croner` pin naming the reason. Drop the `x11` feature from `eframe`, or move the GUI dependencies under `[target.'cfg(target_os = "macos")'.dependencies]`.

**Gate:** CI green on both platforms for the merged branch.

---

## Final verification

After Task 12 merges, from a clean checkout of `main`:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then repeat the review's reproductions with a release build and record the results in the closing PR:

1. `sleep 300 &` inside a `bash -i` session is gone after `ovrcr kill`.
2. `ovrcr kill` on an interactive bash session finishes in under 2 seconds.
3. `ovrcr shutdown --kill` with four HUP- and TERM-ignoring sessions and a 500ms grace finishes in under 3 seconds.
4. Starting the server with `OVRCR_SOCKET` inside a pre-existing 0755 directory refuses with an error naming the directory, and the mode is unchanged.
5. `ovrcr kill 99` with no server running exits 1 and leaves no socket behind.
6. `ovrcr --version` prints the package and protocol versions.
7. A garbage `run.json` under the task store does not stop `ovrcr server` from starting.

Finally, run the computer-use smoke check in `docs/testing-computer-use.md` against the release build and attach its evidence.
