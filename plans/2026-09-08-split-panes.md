# Split Panes Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Two different live sessions render side by side, with keyboard input reaching only the focused pane.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- One or two panes, always side by side, no split tree or ratios.
- Closing, replacing, or hiding a pane never kills, removes, or recreates a session.
- Layout is not persisted; reattach starts with one pane.

**Dependencies:** Independent of the creation plans; the which-key table from `plans/2026-09-08-which-key-popup.md` gains `v`, Tab, and the two-pane meaning of `x` when both exist. `plans/2026-09-08-terminal-mouse-forwarding.md` builds on this plan's `pane_rects`.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

---

### Task 1: Split panes



**Source:** `plans/archived/2026-09-05-split-panes.md` for the step-level sketches. Decisions below are final.

**Files:**
- Modify: `crates/ovrcr-protocol/src/wire.rs` (`PaneTarget`, `DashboardView`, `Request::SetView`, `revision` on `Response::Screen`, `ServerEvent::Output`, `ServerEvent::ScreenDirty`; bump `PROTOCOL_VERSION`, regenerate the snapshot)
- Modify: `crates/ovrcr-runtime/src/server/{mod,dispatch,connections,outbound}.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/{mod,state,input,render,event_loop}.rs`
- Test: `tests/tui.rs`, `tests/server_lifecycle.rs`, `tests/terminal_acceptance.rs`, protocol and runtime unit test modules

**Decisions:**
- One or two panes, always side by side, a one-column `│` separator, integer width split with the extra column on the right, each pane keeping its two metadata rows. No split tree, horizontal split, ratios, drag handles, or third pane.
- Browse mode: `v` opens the second pane with the next different visible session and focuses it ("No other visible session to split" when none); repeated `v` is a no-op; Tab and Shift-Tab switch focus; `x` closes the focused pane when there are two and expands the survivor, and closes the selected session's record when there is one (the meaning from `plans/2026-09-08-which-key-popup.md`). These join the which-key table from `plans/2026-09-08-which-key-popup.md` with descriptions naming the pane's session.
- Browsing the sidebar replaces the focused pane's session; selecting a session already shown in the other pane focuses that pane instead, so one PTY never receives two sizes. A browse-mode click in a pane focuses it and sends no bytes.
- Enter attaches the focused, ready, live pane; Ctrl-g returns to browse. No broadcast input. A pane awaiting its snapshot refuses input with "Pane is loading; retry input". Exited panes keep their final screen.
- Each split pane needs at least 20 columns and one row; when the right area cannot fit both, keep both slots but draw and subscribe only the focused pane at full width with "split hidden: terminal too small", restoring automatically. Never submit a zero size.
- Closing, replacing, or hiding a pane unsubscribes its session; the server parser keeps consuming and the PTY keeps its last size. The focused visible pane's size is the default for new sessions.
- Detach and reattach start in browse mode with one pane; layout is not persisted. Closing a pane never kills, removes, or recreates a session.
- The operator row from `plans/2026-09-08-operator-agent.md` is an ordinary session for pane purposes; `o` assigns it to the focused pane.

**Wire and server contract:**

```rust
pub struct PaneTarget { pub session: SessionId, pub size: TerminalSize }
pub struct DashboardView { pub revision: u64, pub panes: Vec<PaneTarget>, pub focused: Option<SessionId> }
// Request::SetView { view: DashboardView }
// revision: u64 added to Response::Screen, ServerEvent::Output, ServerEvent::ScreenDirty
impl DashboardView { pub fn validate(&self) -> Result<(), String> }
```

`validate` rejects zero revision, more than two panes, duplicate sessions, zero rows or columns, and a focus outside the list; empty requires no focus, nonempty requires focus. `ServerState.selected` becomes `view: Mutex<Option<DashboardView>>`; `last_user_selection` from the operator plan follows the focused pane. `DispatchMessage::SetView { owner, request_id, view, completion }` validates every session before resizing any PTY, resizes only changed geometries, captures each screen, and calls `DashboardSink::replace_view(&view, request_id, screens) -> bool`, which atomically discards obsolete output and dirty flags and queues one `Screen` per visible pane followed by one `Ok`. Preflight queue room for all of them or disconnect the dashboard rather than drop lifecycle frames. A failed second resize reports `PartialFailure` and disconnects that dashboard, preserving processes. `Select` and `Resize` remain as adapters: `Select` builds a singleton view at the next revision; `Resize` is accepted only for a singleton view and answers `InvalidRequest: use SetView for split geometry` otherwise. Output is emitted as `(revision, session, bytes)` only for committed view members; dirty keys are `(revision, SessionId)`; on a dirty event the client re-requests its whole view at a new revision.

**Dashboard contract:**

```rust
pub struct PaneState { pub session: Option<SessionId>, pub parser: vt100::Parser, pub size: TerminalSize, pub desired_size: TerminalSize, pub snapshot_installed: bool, pub ready: bool, pub error: Option<String> }
pub struct PaneRects { pub pane_index: usize, pub metadata: Rect, pub terminal: Rect }
pub fn pane_rects(area: Rect, pane_count: usize, focused: usize) -> Vec<PaneRects>;
// Dashboard: panes: Vec<PaneState>, focused_pane: usize, view_revision: u64
// focused_session(), split_pane(), focus_pane(i), close_focused_pane(),
// view_request(area, request_id) -> Result<Option<ClientMessage>>, apply_screen(revision, session, size, bytes)
```

`view_request` increments the revision with checked arithmetic and marks visible targets unready; `apply_screen` accepts only the current revision for a visible assigned session and replaces only that pane's parser; the matching `Ok` marks panes ready. `select_session` stays the sidebar entry point. Copy mode and history bind to the focused pane's session and reset when its assignment changes.

- [ ] **Step 1: RED tests.** Protocol: `split_view_validation_rejects_ambiguous_targets`, `split_view_frames_round_trip`. Runtime: `set_view_publishes_two_ordered_snapshots_then_ok`, `set_view_overflow_disconnects_instead_of_dropping_lifecycle_frames`, `resize_is_rejected_for_a_split_view`. TUI: `v_splits_to_the_next_visible_session_and_focuses_it`, `selecting_the_other_panes_session_focuses_it`, `pane_rects_at_120x40_are_39x36_and_40x36`, `narrow_terminal_hides_the_split_and_restores_it`, `input_reaches_only_the_focused_ready_pane`, `stale_screen_cannot_replace_a_reassigned_pane`. Acceptance: two real sessions render side by side through the outer PTY, keystrokes reach only the focused one, a burst on both recovers both screens, detach and reattach preserve both PTYs.
- [ ] **Step 2: Implement** in the source plan's order: view contract, ordered snapshots and overflow recovery, pane-local focus and modes, exact rectangles and per-pane resize, then the acceptance run.
- [ ] **Step 3: Verify** `rtk proxy cargo test --workspace` and the acceptance suite.

**Gate:** the six acceptance criteria in the source plan hold: independent PTY sizes (39×36 and 40×36 at 120×40), focused-only input with per-pane cursor and paste modes, snapshots precede increments per revision, narrow terminals never send zero sizes, close/hide/detach/reattach preserve PTYs, and the existing sidebar, restoration, and 50-session gates still pass.

---

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Document `v`, Tab, Shift-Tab, and `x` for panes, the minimum pane width, and the hidden-split message in the README dashboard section, and add a smoke-check step to `docs/testing-computer-use.md`: press `v`, confirm two panes, type into the focused one, press `x`.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. Press `v` on a session and confirm two panes render with independent sizes and that typing reaches only the focused one; press `x` to close it.
2. Run the computer-use smoke check in `docs/testing-computer-use.md`.
