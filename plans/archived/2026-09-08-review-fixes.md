# OVRCR Review Fixes Plan (2026-09-08)

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Complete the tasks in order; each task is one PR against `main`.

**Outcome:** Every finding from the 2026-09-08 review of `main` at `c392059` (after PR #33) is fixed, tested, and documented, without changing product scope or adding an async runtime.

**Source review:** Fifteen tasks below map to the review's findings across the runtime, protocol, dashboard, palette, GUI helper, tests, CI, and docs. Findings marked *confirmed* were re-read in the source by the reviewer; the rest are labeled *plausible* and the task's first step reproduces them.

**Baseline on `main` at `c392059`:** `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean; `cargo test --workspace --all-targets --all-features` green (407 passed, 6 ignored fixtures).

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- Add no dependencies. Every task below is achievable with what the workspace already declares.
- Stay inside each task's file list. Raise `CONCERN:` before touching another file.
- Every behavior change gets the smallest test that would have caught the finding, written first and observed failing. Skip matrices and speculative coverage.
- Give every live fixture its own `OVRCR_CONFIG`, `OVRCR_SOCKET`, and temporary workspace, per `docs/testing-computer-use.md` and the README.
- Prefix every executable and pipeline stage with `rtk`.
- Before committing: `rtk proxy just verify` (fmt-check, check, lint, test) plus the task's named tests.
- Any change to a `Serialize` type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot test in `wire.rs`.
- Follow `AGENTS.md`: one authority per piece of state, owner verification atomic with publication, and no test that proves a negative with a fixed sleep.

## Priority order

| Task | Finding | Why this order |
| --- | --- | --- |
| 1 | Register-project sends `~/…` unexpanded (*confirmed*) | The `a` flow fails for every root-based completion |
| 2 | Default workspace root never exists (*confirmed*) | Same flow fails with the form's own default |
| 3 | Typed branch replaced by first hint; palette stuck on "Working…" (*confirmed*/*plausible*) | Silent wrong-branch workspace creation |
| 4 | Path picker `/` lists `$HOME`; listing on render thread; Tab dead end; small palette fixes (*confirmed*) | Rest of the picker and palette findings |
| 5 | Pending terminal frame dropped; responses routed to the wrong dashboard (*confirmed*) | Server correctness under dashboard replacement |
| 6 | Spawn holds the sessions lock up to 5s and leaks the child on timeout (*confirmed*) | Server liveness |
| 7 | Pane sizes unbounded (*confirmed*) | Protocol validation, needs a version bump |
| 8 | Mouse cleanup after SetView; wheel on the unfocused pane always errors; held buttons survive pause (*confirmed*/*plausible*) | Dashboard mouse |
| 9 | View Error leaves no retry state; incomplete Ok stalls; request-id set leaks; Ok clears unrelated errors (*confirmed*) | Dashboard view state machine |
| 10 | Panic hook restores the terminal from non-main threads (*plausible*) | Dashboard lifecycle |
| 11 | Native glyph cache thrashes past 256 keys (*confirmed*) | GUI performance |
| 12 | GUI keys: modifiers dropped, no Alt-letter, no IME (*confirmed*) | GUI input |
| 13 | GUI mouse: one line per wheel event, hand-rolled press/release encoding (*confirmed*) | GUI input |
| 14 | Env race in a lifecycle test; missing acceptance coverage; `tests/tui.rs` at 9.7k lines (*confirmed*) | Test suite |
| 15 | CI gate drift, cache keys, README roadmap, plan statuses, `.worktrees` | Hygiene, last |

---

### Task 1: Expand picker paths before sending AddProject

**Finding (*confirmed*):** `complete_path` in `crates/ovrcr-tui/src/dashboard/picker.rs:126` returns display strings such as `~/Code/foo/` because the default roots are shown through `display_path`. The register form at `crates/ovrcr-tui/src/dashboard/palette.rs:674` ships `values[0]` and `values[2]` verbatim into `Request::AddProject`, and the server's `git::validate_project` canonicalizes `~/Code/foo/` relative to its own cwd, so every root-based Tab completion fails with "canonicalize Git repository ~/Code/foo/: No such file". The CLI resolves paths with `resolve_cli_path` before sending; the dashboard does not. The existing `register project` case in `tests/tui.rs:423` only types an absolute path.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/picker.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Test: `crates/ovrcr-tui/src/dashboard/picker.rs` (unit), `tests/tui.rs`

- [ ] **Step 1: Add RED tests**

1. In `picker.rs` tests, `expand_path_resolves_tilde_and_keeps_absolute`: inside `with_home(dir, …)`, assert `expand_path("~/Code/repo/") == dir.join("Code/repo")`, `expand_path("~") == dir`, and `expand_path("/tmp/x") == PathBuf::from("/tmp/x")`.
2. In `tests/tui.rs`, add a row to the palette form table: values `["~/Code/repo/", "repo", "~/worktrees"]` expecting `Request::AddProject { name: "repo", repo: home.join("Code/repo"), workspace_root: home.join("worktrees") }`, where `home` is `PathBuf::from(std::env::var_os("HOME").unwrap())` read at test time. Do not set `HOME` in the integration test; it runs in parallel with other tests.

- [ ] **Step 2: Add `expand_path`**

In `picker.rs`:

```rust
pub(crate) fn expand_path(input: &str) -> PathBuf {
    let trimmed = input.trim().trim_end_matches('/');
    match trimmed {
        "" if input.trim().starts_with('/') => PathBuf::from("/"),
        "" | "~" => home_dir(),
        _ => match trimmed.strip_prefix("~/") {
            Some(rest) => home_dir().join(rest),
            None => PathBuf::from(trimmed),
        },
    }
}
```

Have `expand_dir` delegate to it (Task 4 fixes the `/` case through the same function).

- [ ] **Step 3: Use it at submit**

In the `Command::RegisterProject` arm of `palette_key`, build `repo: expand_path(&values[0])` and `workspace_root: expand_path(&values[2])`. Leave `name` as typed.

- [ ] **Step 4: Verify**

```sh
rtk proxy cargo test -p ovrcr-tui picker::
rtk proxy cargo test -p ovrcr --test tui palette
```

**Gate:** Both new tests pass. The existing `a_opens_project_form_with_roots_and_derives_name_and_root` still passes.

---

### Task 2: Create the workspace root when registering a project

**Finding (*confirmed*):** The register form prefills Workspace root with `<config dir>/workspaces/<name>` (`palette.rs:774`), and the README documents that default. `Server::add_project` at `crates/ovrcr-runtime/src/server/mod.rs:503` calls `git::validate_project`, which canonicalizes the root and requires `is_dir()`. Nothing on either side creates the directory, so a fresh register with the form's own default fails with "canonicalize workspace root …/workspaces/<name>". `project add` from the CLI has the same requirement.

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/mod.rs`
- Modify: `README.md`
- Test: `tests/git_lifecycle.rs`

- [ ] **Step 1: Add a RED test**

`add_project_creates_a_missing_workspace_root`: through the fixture `git_lifecycle` already uses, initialize a Git repository in a temp dir and send `AddProject` with `workspace_root = tmp.join("workspaces").join("demo")` that does not exist, assert `Ok`, assert the directory now exists, and assert the registry record stores its canonical path. Add a second assertion that a root whose *parent* is a regular file still returns an error naming the workspace root.

- [ ] **Step 2: Create before validating**

In `add_project`, under `mutation_lock` and after `reject_if_stopping`, run:

```rust
if !workspace_root.exists() {
    fs::create_dir_all(&workspace_root)
        .with_context(|| format!("create workspace root {}", workspace_root.display()))?;
}
```

before `git::validate_project`. Keep validation unchanged so an existing non-directory still fails. A directory created before a later registry-save failure is left in place; the error message must not claim rollback.

- [ ] **Step 3: Document**

In README's register-project paragraph (line 166), change "Workspace root is `<config dir>/workspaces/<name>` until you edit it." to add "It is created on registration if missing." Add the same sentence to the `project add` CLI section.

**Gate:** New test passes. `git_lifecycle` suite passes.

---

### Task 3: Keep a typed branch when Git hints arrive, and never leave the palette on "Working…"

**Finding (*confirmed*):** In existing-branch mode with hints still loading, Branch is a `Text` field. The user types `release-2`, presses Enter, and `submit_when_ready` is set (`palette.rs:652`). When the hint result lands, `refresh_workspace_form` (`palette.rs:291`) swaps the field to a `Pick`; `select_value` finds no match, so `selected` stays 0. `poll_palette` replays Enter, `accept_pick` (`palette.rs:165`) overwrites the value with `branches[0]`, usually `main`, and the workspace is created on the wrong branch with no error.

**Finding (*plausible*):** After `Ok` for CreateWorkspace, `attach_created_workspace` (`palette.rs:1080`) only closes the palette once a `local` session in `Running` phase exists. If `$SHELL` exits immediately, the session is `Exited`, `pending` stays set, and every key but Esc is swallowed behind "Working…".

**Finding (*confirmed*):** The base fallback in `git_hints.rs:95-118` runs `rev-parse --abbrev-ref HEAD` when neither `origin/HEAD` nor a local `main` or `master` exists, so a detached primary checkout prefills Base with the literal `HEAD`, and repositories whose only remote is not named `origin` get no remote-derived suggestion.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/git_hints.rs`
- Test: `tests/tui.rs`, `git_hints.rs` (unit)

- [ ] **Step 1: Add RED tests**

0. `git_hints.rs`: `detached_head_yields_no_base_suggestion` (a repo checked out at a commit, no `main`, expect `Err` with the manual-entry note) and `sole_non_origin_remote_provides_the_base` (remote named `upstream` with `HEAD` set, expect its default branch).
1. `typed_existing_branch_survives_hint_arrival`: open Create workspace, choose `existing`, type `release-2` in Branch, press Enter while hints are absent from `suggestions.cache`, then insert a hint result with branches `["main", "release-2"]` and call `poll_palette`. Assert the produced request is `BranchRequest::Existing { branch: "release-2" }`.
2. `typed_branch_missing_from_hints_is_refused_not_replaced`: same setup with branches `["main"]`. Assert `poll_palette` produces no request, the palette is still open, and its error names the branch.
3. `workspace_created_with_exited_local_shell_closes_palette`: after `Ok`, deliver a `HierarchyChanged` where the new workspace's `local` session is `Exited`. Assert the palette is `None` and no `Select` request is sent.

- [ ] **Step 2: Seed the pick list from the typed value**

In `refresh_workspace_form`, when converting field 3 to a `Pick` and `fields[3].value` is non-empty: if an exact branch exists, `select_value` as today; otherwise set `list.query = fields[3].value.clone()` before installing the list so the pick list is filtered to the typed text and `accepted()` returns `None` when nothing matches. In `palette_key`'s Enter arm, when `accept_pick` returns `false` on a required field, set `palette.error = Some(format!("{} not found in repository", fields[*active].label))` and clear `submit_when_ready`.

- [ ] **Step 3: Close the palette on any acknowledged workspace**

In `attach_created_workspace`, find the workspace first; if it exists, set `self.palette = None` regardless of session phase. Only send `select_request` and enter `InputMode::Terminal` when a `Running` `local` session exists; otherwise return the empty vector and leave the mode as is.

- [ ] **Step 4: Fix the base fallback**

In `git_hints.rs`, treat an `abbrev-ref` result of `HEAD` as no suggestion and return the existing error path. Before falling back to `origin/HEAD`, run `git remote` and, when exactly one remote exists, use `<remote>/HEAD` instead of assuming `origin`.

- [ ] **Step 5: Verify**

```sh
rtk proxy cargo test -p ovrcr-tui git_hints::
rtk proxy cargo test -p ovrcr --test tui workspace
```

**Gate:** Five new tests pass; `existing_branch_pick_*` tests from PR #33 still pass.

---

### Task 4: Path picker root, listing cache, Tab fall-through, and small palette fixes

**Finding (*confirmed*):** `expand_dir("/")` (`picker.rs:181`) trims to an empty string and takes the home branch, so typing `/U` lists `$HOME` filtered by `U` and Tab completes to a path that does not exist. `draw_palette` (`palette.rs:1221`) calls `list_path_entries` on every frame while a Path field is active, doing a `read_dir` plus two `stat`s per entry before truncating at 500, so a large or slow directory freezes each redraw. Tab on a Path field (`palette.rs:567`) always returns after `complete_path`, so with nothing to complete it is a no-op although the footer says "Tab accept/next". The Search page never clamps `selected` when entries shrink (`palette.rs:544`), so removing the highlighted session renders an empty list until Up is pressed. An `[[agents]]` override named `Custom` is pushed as `Detected` (`agents.rs:88`), so the Command field shows but its value is ignored.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/picker.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/agents.rs`
- Test: `picker.rs`, `agents.rs` (unit), `tests/tui.rs`

- [ ] **Step 1: Add RED tests**

1. `picker.rs`: `slash_prefix_lists_filesystem_root`: with `with_home(tmp, …)` and a root `tmp/Code`, assert `list_path_entries("/", &roots)` contains no entry from `tmp` and `expand_path("/")` is `/`.
2. `picker.rs`: `listing_is_cached_until_input_changes`: create a `PathPicker`, call `picker.listing("~/Code/", roots)` twice, delete a subdirectory between the calls, assert the second result equals the first; call with `"~/Code/r"` and assert it recomputed.
3. `tests/tui.rs`: `tab_on_a_leaf_path_advances_to_the_next_field`: with the Repository field holding a directory that has no children, press Tab and assert the active field is now Name.
4. `tests/tui.rs`: `search_selection_clamps_when_entries_shrink`: open `:`, move to the last entry, remove that session via `HierarchyChanged`, assert Enter acts on the new last entry.
5. `agents.rs`: `custom_override_replaces_the_custom_entry`: an override named `Custom` yields exactly one `Custom` entry and no `Detected` entry of that name.

- [ ] **Step 2: Cache the listing on the picker**

Add `cached: Option<(String, PathListing)>` to `PathPicker` and

```rust
pub fn listing(&mut self, input: &str, roots: &[PathBuf]) -> &PathListing {
    if self.cached.as_ref().is_none_or(|(key, _)| key != input) {
        self.cached = Some((input.to_owned(), list_path_entries(input, roots)));
    }
    &self.cached.as_ref().unwrap().1
}
```

Refresh it in `palette_key` after every edit of a Path field (typing, Backspace, Ctrl-u, Tab) and in `open_register_project`. Change `draw_palette` to read `picker.cached` and draw nothing when it is `None`. Stop the `.git` stat once `entries.len()` passes `PATH_LIST_LIMIT`.

- [ ] **Step 3: Tab falls through, selection clamps, Custom override**

In the Tab arm, when `complete_path` returns `None`, fall through to `move_form_field`. In the Search Enter arm and in `draw_palette`, use `selected.min(entries.len().saturating_sub(1))`. In `apply_overrides`, match an override whose name equals the popped `custom` entry's name and replace that entry's argv, mirroring the `shell` branch.

**Gate:** Five new tests pass; `picker::`, `agents::`, and `tests/tui.rs` palette tests pass.

---

### Task 5: Deliver the terminal frame, and route dashboard responses to their requester

**Finding (*confirmed*):** `DashboardSink::enqueue` (`crates/ovrcr-runtime/src/server/outbound.rs:70`) returns `false` once `closing` is set by the terminal path at line 179. Every caller treats `false` as "disconnect now": `dashboard_send` and `dashboard_send_owner` call `disconnect_dashboard`, which shuts the stream down at line 351 before the writer has written the terminal frame. A partial-resize error followed by any `SessionChanged` or `HierarchyChanged` therefore reaches the dashboard as a bare EOF. `partial_resize_connection_delivers_error_before_owner_close` covers only the no-concurrent-send case.

**Finding (*confirmed*):** The general Dashboard response path at `connections.rs:239` uses `dashboard_try_send`, which sends to whoever owns the slot with no identity check, although `ownership` is in scope. The `DashboardHello` Hierarchy reply at line 184, the conflict reply at line 73, and `DashboardGeometry` at line 580 (which publishes under the current owner's identity, ignoring the requester) have the same gap. A slow `CreateWorkspace` from an evicted dashboard lands in its replacement's queue with a request id the replacement may recognize.

**Finding (*plausible*):** `SetView` in `dispatch.rs:482-515` resolves sessions without holding any lock across the operation, and `remove_session_locked` runs on a connection thread. A session removed between resolution and publication (`dispatch.rs:607`) is still published as a focused pane at a valid revision; input to it is admitted by the view check and then fails with `NotFound`.

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/outbound.rs`
- Modify: `crates/ovrcr-runtime/src/server/connections.rs`
- Modify: `crates/ovrcr-runtime/src/server/dispatch.rs`
- Test: `crates/ovrcr-runtime/src/server/tests.rs`

- [ ] **Step 1: Add RED tests**

0. `set_view_drops_a_session_removed_before_publication`: use `before_view_publish_hook` to remove the focused session between resolution and publication; assert the response is `Error { NotFound }` (or the published view omits that pane and the response says so) and no `Input` to that session is accepted afterwards.
1. `terminal_frame_survives_a_concurrent_lifecycle_event`: register a dashboard whose reader does not drain, force a partial-resize failure exactly as `partial_resize_connection_delivers_error_before_owner_close` does, then before the writer runs (use the existing `before_view_publish_hook` or a blocked writer as in `partial_resize_blocked_writer_times_out_and_closes_owner`) call `dashboard_try_send(&state, HierarchyChanged)`. Assert the client reads the Error response, not EOF.
2. `late_response_does_not_reach_a_replacement_dashboard`: dashboard D1 sends a request whose handler blocks on a test hook; disconnect D1; register D2; release the hook. Assert D2 receives nothing with D1's request id and D2's next own request is answered normally.
3. `stale_geometry_does_not_overwrite_replacement_size`: same replacement pattern with `DashboardGeometry`; assert `state.dashboard_size` still holds D2's size.

- [ ] **Step 2: Drain instead of disconnect**

Introduce an enum:

```rust
pub(super) enum Enqueue { Queued, Draining, Closed }
```

`enqueue` returns `Draining` when `closing && !closed` (message dropped, socket left open for the writer to flush the terminal frame) and `Closed` when `closed`. Update `dashboard_send`, `dashboard_send_owner`, and `dashboard_send_owner_terminal` so only `Closed` triggers `disconnect_dashboard`; `Draining` returns `false` without side effects.

- [ ] **Step 3: Owner-check every dashboard response**

In `connections.rs`, replace `dashboard_try_send` at lines 184 and 239 and `dashboard_send` at line 73 with `dashboard_send_owner(&state, &owned.identity, …)` using the connection's `ownership`. Change `DashboardGeometry` to require `dashboard_owner_matches(state, owner)` and pass `owner` to `set_dashboard_geometry`; return `Conflict` otherwise. Keep the shutdown path's completion channel by adding an owner-checked variant with a completion argument.

- [ ] **Step 4: Re-check membership at publish**

At `dispatch.rs:607`, take `state.sessions` inside the `state.view` lock and drop any pane whose session is gone; if the focused pane was dropped, answer `Error { NotFound }` instead of `Ok` and do not publish.

**Gate:** Four new tests pass. `split_delivery_*`, `partial_resize_*`, and `dashboard_overflow_*` tests pass.

---

### Task 6: Spawn sessions outside the sessions lock and kill on group-leader timeout

**Finding (*confirmed*):** `create_session_locked` (`server/mod.rs:265`) holds `state.sessions` from the duplicate check through `Session::spawn`. `spawn_internal` (`session/mod.rs:262`) now polls `wait_for_group_leader` up to `GROUP_LEADER_TIMEOUT` (5 s). While it waits, `dispatch_session_event` blocks on the same lock, so all dashboard output stalls. On timeout the function bails without `child.kill()`, so a wedged child is neither killed nor reaped, while the pgid-mismatch branch two lines later does kill.

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/mod.rs`
- Modify: `crates/ovrcr-runtime/src/session/mod.rs`
- Test: `crates/ovrcr-runtime/src/session/tests.rs`, `crates/ovrcr-runtime/src/server/tests.rs`

- [ ] **Step 1: Add RED tests**

1. `session/tests.rs`: `group_leader_timeout_kills_the_child`: set `OVRCR_GROUP_LEADER_TIMEOUT_MS` (add this override if absent; default unchanged) to 200, spawn a command that never becomes a group leader (a test helper executable that calls `setpgid(0, getppid())`, or a `sh -c` that execs after `setsid` when available), assert the spawn error mentions the timeout and `kill(pid, 0)` returns `ESRCH` within one second.
2. `server/tests.rs`: `session_output_flows_while_another_session_spawns`: block one spawn with the same slow-leader fixture, write to an existing session, assert its `Output` event is delivered to the dashboard within 500 ms while the spawn is still pending. Use a channel to prove the spawn is inside `wait_for_group_leader`, not a sleep.

- [ ] **Step 2: Kill on timeout**

In `spawn_internal`, map the `wait_for_group_leader` error through `child.kill()` before returning it.

- [ ] **Step 3: Spawn without the lock**

In `create_session_locked`: take `sessions`, run the duplicate check, reserve the id, drop the guard. Spawn. Re-take the guard and insert. No second duplicate check is needed: all three callers (`create_session`, `create_session_ready`, and `create_workspace_inner`) hold `mutation_lock`, which serializes creation; add a one-line comment saying so above the function.

**Gate:** Both new tests pass; `session::` and server tests pass.

---

### Task 7: Bound pane dimensions

**Finding (*confirmed*):** `DashboardView::validate` (`crates/ovrcr-protocol/src/wire.rs:87`) rejects only zero. A `PaneTarget` of 65535 by 65535 reaches `vt100::Parser::set_size` and the PTY, allocating billions of cells; a merely large pane produces a `Screen` over `MAX_FRAME_BYTES`, so `replace_view` closes the sink and the dashboard is disconnected with no error after the PTYs were already resized.

**Files:**
- Modify: `crates/ovrcr-protocol/src/wire.rs`
- Modify: `crates/ovrcr-runtime/src/server/dispatch.rs`
- Test: `wire.rs` (unit), `crates/ovrcr-runtime/src/server/tests.rs`

- [ ] **Step 1: Add RED tests**

1. `wire.rs`: `validate_rejects_oversized_panes`: a pane of `rows: 1001` or `cols: 1001` returns an error naming the limit; `1000` by `1000` passes.
2. `server/tests.rs`: `oversized_set_view_returns_invalid_request_without_resizing`: send a view with `rows: 5000`; assert the response is `Error { code: InvalidRequest }` and the session's PTY size (via `MasterPty::get_size()`) is unchanged.

- [ ] **Step 2: Add the constants and check**

```rust
pub const MAX_PANE_ROWS: u16 = 1000;
pub const MAX_PANE_COLS: u16 = 1000;
```

Check them in `validate()` beside the zero check. Bump `PROTOCOL_VERSION`, regenerate the wire snapshot, and mention the limit in the README protocol section.

**Gate:** Both new tests pass; protocol and server suites pass.

---

### Task 8: Mouse cleanup ordering, wheel on the unfocused pane, and pause

**Finding (*confirmed*):** When a held button's session is removed, `update_hierarchy` (`state.rs:2528-2560`) parks the synthetic release in `mouse.pending_cleanup` but returns the replacement `SetView` in `outgoing`, which `next_dashboard_messages` (`event_loop.rs:355`) writes immediately. The release waits for the next `emit_view_request`, so the server applies the new focus first and rejects the release with "input is only accepted for the focused dashboard session".

**Finding (*confirmed*):** `pane_wheel_history` (`state.rs:1958`) calls `focus_pane` for the unfocused pane, which invalidates readiness, then `begin_history_request`, which refuses at `state.rs:1262` with "Pane is loading; retry history". The documented wheel rule never works on the first tick over the other pane.

**Finding (*plausible*):** `update_mode_for_selected_phase` (`state.rs:2630`) forces Browse on pause without `cancel_mouse_gesture`, so a held button survives and its eventual release targets a paused session.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/event_loop.rs`
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui.rs`

- [ ] **Step 1: Add RED tests**

1. `dashboard/tests.rs`: `mouse_release_precedes_the_replacement_view_request`: hold a button in pane A with tracking on, deliver `HierarchyChanged` without A through `next_dashboard_messages` against a socket pair, and assert the first frame written is `Input { session: A }` and the second is `SetView`.
2. `tests/tui.rs`: `wheel_up_over_the_unfocused_pane_opens_its_history`: split view, both ready, wheel up over pane B. Assert focus moved to B, no error is set, and once the view completes (deliver the snapshots and `Ok`), a `BeginHistory` request for B is produced.
3. `tests/tui.rs`: `pause_cancels_a_held_mouse_gesture`: hold a button, deliver a `SessionChanged` to `Paused`, assert `take_mouse_cleanup()` yields the release for that session and no later event forwards mouse bytes.

- [ ] **Step 2: Write cleanup first**

In `next_dashboard_messages`, before writing each `handle_server_message` result, write `dashboard.take_mouse_cleanup()` if present. Keep the same in `emit_view_request`.

- [ ] **Step 3: Defer history after a wheel focus change**

Add `deferred_history_at_tail: Option<usize>` (pane index) to `Dashboard`. In `pane_wheel_history`, when focus changed, set it and return `Redraw`. In the view `Ok` branch that sets `pane.ready = true`, if the deferred pane is now ready and focused, call `begin_history_request(true)` and push its request into `outgoing`. Clear the field in `invalidate_view_readiness` callers that change selection.

- [ ] **Step 4: Cancel on pause**

In `update_mode_for_selected_phase`, call `self.cancel_mouse_gesture()` when switching to Browse.

**Gate:** Three new tests pass; `mouse_*`, `wheel_*`, and `split_*` tests pass.

---

### Task 9: View error retry state, incomplete Ok, request-id cleanup, and error clearing

**Finding (*confirmed*):** A matched view `Error` (`state.rs:2260`) drops `pending_view` and marks panes not ready but leaves `requested_view` at the last acked view with no failure record. If the desired view equals the old one, `unchanged` is true in `view_request` (`state.rs:710`) and nothing is retried: the pane stays loading until the user changes focus. If it differs, every Error wakes the loop and re-sends `SetView` immediately, a tight loop with no backoff.

**Finding (*confirmed*):** `Ok` with missing snapshots (`state.rs:2175`) restores `pending_view` permanently; `Ok` is final, so no view request is ever issued again. The server produces this only on owner mismatch, but the client shows a silent stall.

**Finding (*confirmed*):** `view_request_ids` (`state.rs:780`) grows by one per `SetView` forever. Any plain `Ok` clears `self.error` (`state.rs:2208`), which wipes the settings-load error set at `event_loop.rs:56` before the geometry handshake and wipes fresh errors like "No other visible session to split" when the mouse cleanup `Input` is acknowledged. `resize_request` (`state.rs:2574`) derives the outer area from the focused pane and forces `requested_view = None`, breaking "a true no-op preserves readiness"; only tests call it.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/event_loop.rs`
- Test: `tests/tui.rs`

- [ ] **Step 1: Add RED tests**

1. `view_error_on_unchanged_view_retries_once_after_backoff`: complete a view, deliver `ScreenDirty`, answer the re-request with `Error { NotFound }`, assert the next `view_request` call within 250 ms returns `None` and a call after the backoff returns a `SetView`.
2. `view_error_on_changed_view_does_not_spin`: geometry changes, answer with `Error { Internal }`, call `handle_server_message` again with another `Error` for the retry, assert at most one `SetView` per backoff window.
3. `incomplete_final_ok_marks_panes_failed_and_refreshes`: deliver `Ok` for a two-pane view after only one `Screen`; assert `pending_view` is `None`, both panes are not ready, `force_view_refresh` is set, and the next `view_request` returns a new `SetView`.
4. `view_request_ids_are_released_on_final_response`: after ten completed view requests, assert `view_request_ids.len() <= 1`.
5. `settings_error_survives_the_geometry_ack`: set `dashboard.error` before handling `Ok` for request 2, assert it is still set. Add `split_error_survives_mouse_cleanup_ack` in the same style.

- [ ] **Step 2: Record failures**

Add `failed_view: Option<(DesiredView, Instant)>` to `Dashboard`. On a matched view Error, set `requested_view = None` and `failed_view = Some((pending.view, Instant::now()))`. In `view_request`, when `failed_view` matches `desired` and less than `VIEW_RETRY_BACKOFF` (250 ms) has elapsed, return `Ok(None)`; clear `failed_view` when the desired view differs or the backoff passed. Ask the event loop to wake at the backoff deadline (add the instant to the existing wait computation rather than a sleep).

- [ ] **Step 3: Treat incomplete Ok as failure**

In the `!complete` branch, do not restore `pending_view`; mark each target pane `ready = false`, `snapshot_installed = false`, set `force_view_refresh = true`, and push the next `view_request` into `outgoing`.

- [ ] **Step 4: Release ids and scope error clearing**

Remove the request id from `view_request_ids` in both the `Ok` and `Error` arms. Replace the blanket `Response::Ok if !self.history_page_error => self.error = None` with clearing only for ids the dashboard recorded as error-owning (select, split, input, and history requests: track them in a small `HashSet<u64>` the same way as `view_request_ids`). Apply the settings error after the geometry ack in `event_loop.rs`. Delete `resize_request` and update its callers in tests to drive `view_request` with a new `Rect`.

**Gate:** Five new tests pass; every `view_*`, `split_*`, and `history_*` test in `tests/tui.rs` passes.

---

### Task 10: Restore the terminal only from the main thread's panic

**Finding (*plausible*):** The hook installed at `event_loop.rs:105` is process-global. A panic on the reader or task-worker thread restores raw mode and the alternate screen while the main loop keeps drawing on a cooked terminal; `PANIC_TERMINAL_RESTORED` is thread-local, so the guard restores again at exit.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/event_loop.rs`
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`

- [ ] **Step 1: Add a RED test**

`panic_hook_ignores_non_main_threads`: install the hook through a helper that takes a writer, panic inside a spawned thread with `catch_unwind`, and assert the writer received no restore sequence; panic on the installing thread and assert it did.

- [ ] **Step 2: Compare thread ids**

Capture `thread::current().id()` before `set_hook`; in the hook, if `thread::current().id()` differs, delegate to the prior hook and return without touching the terminal.

**Gate:** New test passes; `dashboard::` tests pass.

---

### Task 11: Evict native glyphs per frame instead of clearing at 256

**Finding (*confirmed*):** `src/bin/ovrcr-gui/native_glyph.rs:133` clears the whole cache on the 257th distinct key, and the key includes text, color, bold, italic, size, and scale. A 40 by 120 screen of CJK text has far more than 256 distinct keys, so every frame after the clear re-rasterizes every fallback cell through Core Text and uploads a texture per cell.

**Files:**
- Modify: `src/bin/ovrcr-gui/native_glyph.rs`
- Modify: `src/bin/ovrcr-gui.rs`
- Test: `src/bin/ovrcr-gui.rs` (unit, macOS)

- [ ] **Step 1: Add a RED test**

`native_glyph_cache_keeps_a_screen_of_distinct_glyphs`: paint a screen with 600 distinct CJK characters twice; assert the number of `rasterize` calls (count through a test-only counter on the cache) on the second paint is zero.

- [ ] **Step 2: Stamp and sweep**

Store `last_used: u64` in `Glyph`, bump a frame counter in `paint` once per frame, stamp entries on use, and after the frame remove entries whose stamp is older than the previous frame when the cache exceeds 4096 entries. Drop the 256 clear.

**Gate:** New test passes; `cargo test -p ovrcr --features gui --bin ovrcr-gui` passes on macOS.

---

### Task 12: GUI keys: modifiers on special keys, Alt-letter, IME

**Finding (*confirmed*):** `src/gui/input.rs:70` builds `KeyEvent::new(code, KeyModifiers::NONE)`, so Shift-Enter, Ctrl or Alt with arrows, Alt-Backspace, and modified Home, End, and F-keys arrive unmodified although `encode_key` already emits the CSI-u and `1;m` forms. Alt with a letter has no meta encoding: egui delivers the Option-composed glyph as `Text` and the `Key` arm returns nothing. IME is never enabled: only egui's `TextEdit` sets `PlatformOutput.ime`, so composition and dead keys cannot reach the terminal, and the `ImeEvent::Commit` arm is dead code.

**Files:**
- Modify: `src/gui/input.rs`
- Modify: `src/bin/ovrcr-gui.rs`
- Test: `src/gui/input.rs` (unit)

- [ ] **Step 1: Add RED tests**

1. `special_keys_carry_modifiers`: `Event::Key { key: ArrowUp, modifiers: ctrl }` encodes to `ESC [ 1 ; 5 A`; Shift-Enter encodes to the CSI-u form `encode_key` produces.
2. `alt_letter_encodes_meta`: `Event::Key { key: B, modifiers: alt }` encodes to `ESC b`, and the paired `Event::Text("∫")` in the same frame is dropped.

- [ ] **Step 2: Pass modifiers and handle Alt**

Use `KeyEvent::new(code, key_modifiers(*modifiers))`. Add an `alt && key.name().len() == 1` arm returning `[0x1b, letter]`, and in `ovrcr-gui.rs`'s event loop skip a `Text` event that immediately follows an Alt `Key` press in the same frame.

- [ ] **Step 3: Enable IME**

In `ovrcr-gui.rs`, when the terminal response has focus, set `ui.output_mut(|o| o.ime = Some(egui::output::IMEOutput { rect, cursor_rect }))` with `cursor_rect` at the cursor cell each frame. Leave `Preedit` unforwarded. Verify manually with a CJK input method and record the result under `docs/testing-computer-use.md` as native evidence; an automated egui test cannot prove composition.

**Gate:** Both unit tests pass; GUI tests pass on macOS. Native IME evidence recorded or reported as unverified.

---

### Task 13: GUI mouse: wheel distance and shared encoding

**Finding (*confirmed*):** `src/gui/input.rs:146` forwards exactly one line per `MouseWheel` event regardless of `unit` or magnitude, so trackpad scroll speed tracks event rate rather than distance. The press and release arms (`input.rs:78-139`) hand-roll the encoding and diverge from `encode_mouse`: they add modifier bits in X10 mode and skip the legacy 223 and 2015 coordinate limits. Pointer motion is never forwarded, so drag reports never happen.

**Files:**
- Modify: `src/gui/input.rs`
- Modify: `src/bin/ovrcr-gui.rs`
- Test: `src/gui/input.rs` (unit)

- [ ] **Step 1: Add RED tests**

1. `wheel_points_accumulate_into_lines`: with `cell.y = 16`, three `MouseWheel` events of `Point` delta 6 produce one `ScrollDown` report after the third, not three.
2. `press_encoding_matches_encode_mouse`: for X10 mode with Shift held, the GUI encoding equals `encode_mouse` output (no modifier bits).
3. `drag_is_forwarded_in_button_motion_mode`: a `PointerMoved` while a button is held in mode 1002 produces a drag report.

- [ ] **Step 2: Accumulate and delegate**

Add `wheel_carry: egui::Vec2` to the app state; convert `Point` deltas by dividing by the cell size, `Line` as-is, `Page` by the visible rows, add the carry, emit `trunc()` reports, keep the remainder. Map `PointerButton` to `MouseEventKind::Down` and `Up` and `PointerMoved` to `Drag` or `Moved`, then call `encode_mouse` for all of them; delete the hand-rolled encoder.

**Gate:** Three unit tests pass; GUI tests pass on macOS.

---

### Task 14: Test suite: env race, acceptance gaps, and splitting `tests/tui.rs`

**Finding (*confirmed*):** `startup_concurrent_attempts_leave_one_server` (`tests/server_lifecycle.rs:1173`) sets `OVRCR_SERVER_EXECUTABLE`, `OVRCR_SOCKET`, and `OVRCR_CONFIG` without `env_lock()`, while every sibling that touches env takes it. Any parallel test that spawns a CLI child can inherit this test's socket.

**Coverage gaps from the feature plans:** `FocusLost` and `FocusGained` gesture handling has no test; "wheel opens history" through the outer PTY is asserted only in-process; the split acceptance asserts dashboard text `39x36` and `40x36` rather than kernel PTY sizes; `Resize` on a split view being rejected and view overflow disconnecting rather than dropping lifecycle frames have no server test. `tests/tui.rs` is one 9,715-line file with 109 tests and no module seams.

**Files:**
- Modify: `tests/server_lifecycle.rs`
- Modify: `tests/terminal_acceptance.rs`
- Modify: `crates/ovrcr-runtime/src/server/tests.rs`
- Create: `tests/tui/main.rs`, `tests/tui/palette.rs`, `tests/tui/copy_history.rs`, `tests/tui/split.rs`, `tests/tui/mouse.rs`, `tests/tui/sidebar.rs`
- Delete: `tests/tui.rs`

- [ ] **Step 1: Take the env lock**

Wrap the env mutations in `startup_concurrent_attempts_leave_one_server` with `let _env = env_lock();` held until after the `remove_var` calls.

- [ ] **Step 2: Add the missing assertions**

1. `tests/tui` (mouse module): `focus_lost_finishes_gestures_and_focus_gained_resumes`: hold a button, deliver `Event::FocusLost`, assert the release is queued and further mouse events are dropped until `FocusGained`.
2. `terminal_acceptance.rs`: extend `mouse_forwarding_outer_pty_round_trip` to send a wheel-up through the outer PTY against a plain shell and wait for the HISTORY marker.
3. `terminal_acceptance.rs`: in `split_terminal_acceptance_preserves_input_and_geometry`, run `stty size` in each pane's shell and wait for `36 39` and `36 40` in the child output.
4. `server/tests.rs`: `resize_is_rejected_for_a_split_view` and `set_view_overflow_disconnects_instead_of_dropping_lifecycle_frames`, named as the split plan named them.
5. `tests/tui` (palette module): `n_is_listed_in_the_hint_table`, the footer-hint criterion from the detected-agents plan.

- [ ] **Step 3: Replace timing guesses with signals**

1. `server/tests.rs:1458-1488` (`partial_resize_blocked_writer_times_out_and_closes_owner`): replace the 256 `yield_now` calls with a channel the blocked writer signals from inside its write path (a test hook beside `before_view_publish_hook`), and drop the `elapsed < 5s` assertion in favor of asserting the disconnect happened.
2. `crates/ovrcr-tui/src/dashboard/tests.rs:340-353`: replace the 20 ms sleep plus `elapsed < 250ms` with a barrier the helper thread passes after it has parked.
3. Introduce one `fn wait_deadline() -> Duration` in `tests/support` that returns 3 s locally and 15 s when `CI` is set, and use it for the 114 `Duration::from_secs(3)` deadlines in `tests/terminal_acceptance.rs` and `tests/tui.rs`.

- [ ] **Step 4: Split `tests/tui.rs`**

Move the file to `tests/tui/main.rs` and cut it along its existing clusters into the modules listed above: palette and creation forms (lines 393-900), sidebar and agent-hook rendering (1459-1800), copy and history (2203-6600), split panes (6669-9200), mouse (9214-end). Shared helpers (`dashboard_fixture`, `palette_text`, `mouse_event`) go in `main.rs` as `pub(crate)`. The binary name stays `tui`, so `cargo test -p ovrcr --test tui` and `scripts/ci-tests.sh` keep working. Confirm with `rtk proxy cargo test -p ovrcr --test tui -- --list | rtk proxy tail -1` that the count is 109 plus the tests added in this task.

**Gate:** All moved tests pass under the same names; new assertions pass; `lifecycle` and `core` CI groups pass locally.

---

### Task 15: CI gates, cache keys, README, plan statuses, and ignores

**Findings:** `scripts/ci-tests.sh` runs `--lib --bins`, `--doc`, and per-target integration tests with default features, not the `cargo test --workspace --all-targets --all-features` gate AGENTS.md names, so the `core` group can pass while an all-features target fails to build. All three CI jobs share `shared-key: verify` while building different feature sets, so the lint and GUI jobs largely miss the cache, and `timeout-minutes: 10` is tight for a cold eframe build. `README.md:769` still lists split panes as unshipped while lines 79-98 document them. Four of the five plans archived on 2026-09-08 say `Status: Implemented` with every step checkbox open and no evidence section; the workspace-picks plan lists three unchecked "Remaining acceptance" items. `.worktrees/` holds two stale worktrees and is not ignored.

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `scripts/ci-tests.sh`
- Modify: `README.md`
- Modify: `plans/archived/2026-09-08-split-panes.md`, `…-terminal-mouse-forwarding.md`, `…-detected-agents.md`, `…-project-path-picker.md`, `…-workspace-creation-picks.md`
- Modify: `.gitignore`

- [ ] **Step 1: Match the documented gate**

The build half of the gate is already covered by the clippy job on both platforms. Close the test half: in `ci-tests.sh`, when the runner is macOS and the suite is `core`, run `cargo test --workspace --all-targets --all-features` verbatim and skip the per-target loop; keep Linux on the per-target loop because the GUI feature has no Linux backend. Then fold the separate `gui` job into that macOS `core` run and delete it. Give each remaining job its own `shared-key` (`lint`, `tests`) and raise the macOS `core` timeout to 20 minutes for the cold eframe build.

- [ ] **Step 2: Fix the README roadmap**

Mark split panes `[x]`.

- [ ] **Step 3: Reconcile plan statuses**

For each of the four plans with open checkboxes, either tick the boxes against the merged PR's test names (one line per criterion naming the test) or change the status line to "Implemented; acceptance evidence not recorded". For workspace-creation-picks, list the three remaining acceptance items as open under a "Remaining" heading rather than under "Implemented".

- [ ] **Step 4: Ignore local worktrees**

Add `/.worktrees/` to `.gitignore`. Do not delete the directories; they are the developer's checkouts.

**Gate:** CI green on both platforms for the merged branch, including the GUI job.

---

## Deferred findings

Reviewed, judged real, and left out of this plan because each is pre-existing, design-level, or below the cost of a task. Revisit when the surrounding code changes.

- Removing the focused session of a two-pane view clears every pane's subscription (`server/mod.rs:356-362`); the dashboard recovers by re-requesting on `HierarchyChanged`. A `ScreenDirty` for the survivor would make recovery explicit.
- `wait_for_child` (`session/io.rs:57`) loops while `group_exists(pgid)` after reaping, so a reused pgid delays the `Exited` event; `EPERM` also counts as existing.
- The GUI helper writes to the PTY on the paint thread (`src/gui.rs:443`); a large paste while the dashboard is not draining stalls the UI.
- `tests/gui.rs` and `ovrcr-gui.rs:533` busy-wait with `yield_now` while cloning the screen under the parser lock; reuse the 10 ms `wait_screen` polling.
- `state.rs` at 2,665 lines splits cleanly into `history.rs`, `view.rs`, and `mouse.rs` along the seams Task 9 and Task 8 touch; do that refactor as its own PR after both land.

## Final verification

After Task 15 merges, on `main`:

```sh
rtk proxy cargo fmt --all -- --check
rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk proxy cargo test --workspace --all-targets --all-features
rtk proxy cargo test -p ovrcr --features gui --bin ovrcr-gui --test gui
```

Then the manual checks that no automated test proves, recorded under `docs/testing-computer-use.md` with the tested revision:

1. Press `a`, Tab into `~/Code`, Tab onto a repository, Enter through the defaults. The project appears with its workspace root created.
2. Create a workspace on an existing branch by typing its name before hints arrive. The worktree is on that branch.
3. In a split, wheel up over the unfocused pane. History opens for that pane with no error in the footer.
4. In the GUI helper with a CJK input method active, compose and commit a character into the shell.
