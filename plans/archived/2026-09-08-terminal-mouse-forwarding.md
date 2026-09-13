# Terminal Mouse Forwarding and Pane Wheel Scrolling Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Mouse input reaches the application inside the focused terminal when that application has asked for a mouse protocol, and the wheel scrolls a plain terminal's history when it has not.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- Application requests control forwarding; encoding enablement alone forwards nothing.
- Held buttons never migrate between sessions; cleanup releases precede any selection or resize request.
- Wheel over a pane without tracking opens the history view at the tail; wheel-down at the newest row returns to live.

**Dependencies:** Depends on `pane_rects` from `plans/2026-09-08-split-panes.md` and on the shipped historical scrollback view.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

---

### Task 1: Terminal mouse forwarding and pane wheel scrolling



**Source:** `plans/archived/2026-09-05-terminal-mouse-forwarding.md` for the step-level sketches and byte tables. Decisions below are final, plus the wheel-scrolling addition that closes the gap between that plan and historical scrollback.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/{input,state,event_loop,mod,terminal_guard}.rs`, `render.rs` where the pane rectangle is consumed
- Create: `tests/support/mouse_app.rs` (a raw fixture that enables a tracking mode and echoes received bytes)
- Test: `tests/tui.rs`, `tests/terminal_acceptance.rs`

No wire change; mouse bytes travel in the existing `Request::Input`.

**Decisions:**
- Forward only to the focused, ready, live pane's session, only in terminal mode, only when that session's parser reports a tracking mode (1000, 1002, 1003, or X10) it has requested. Encoding enablement alone forwards nothing.
- Forward left, middle, and right presses; releases in 1000/1002/1003; drag in 1002/1003; unpressed motion only in 1003; wheel up/down/left/right in 1000/1002/1003; X10 gets the three presses only. Shift, Alt, and Control modifiers except in X10. No Shift-click bypass.
- Coordinates are computed only after bounds checks against `pane_rects` from `plans/2026-09-08-split-panes.md`; metadata rows, separators, footer, sidebar, and cells beyond the parser's size are rejected. Columns are cells, never character indices.
- A press owns its session until release or cancellation; up to three held buttons; duplicate motion at the same cell is suppressed. Leaving the rectangle emits a release at the last valid position. Before Ctrl-g, selection replacement, resize, or detach, held buttons are released with the old encoding and old session id, and that cleanup frame is written before the selection or resize request.
- Outer mouse capture is on in browse mode or while forwarding is eligible, reconciled after output and snapshots as well as keys, using the existing enable and disable commands. Focus reporting is on for the dashboard lifetime; `FocusLost` finishes gestures and suppresses forwarding until `FocusGained`.
- A parsed protocol change clears held state without sending old-protocol releases. During snapshot resynchronization or a pending resize, forwarding is disabled until the matching acknowledgement. Exited or removed sessions never receive mouse input.
- **Pane wheel scrolling (new):** in browse mode, or in terminal mode when the focused application has not requested a tracking mode, a wheel-up over a pane opens the historical view (`begin_history_request`) for that pane's session positioned at the captured tail, and further wheel ticks page it one row per tick; a wheel-down at the newest row closes history and returns to the live pane. When the application has requested tracking, the wheel is forwarded to it instead. The which-key table's History entry gains "or scroll the wheel over the pane".

**Interfaces:**

```rust
pub fn encode_mouse(event: MouseEvent, mode: vt100::MouseProtocolMode, encoding: vt100::MouseProtocolEncoding) -> Option<Vec<u8>>;
// Dashboard: mouse: MouseForwarding { held: [Option<HeldMouse>; 3], last_motion, pending_cleanup: Option<ClientMessage> },
//   mouse_focused: bool, mouse_ready: bool, mouse_awaiting: Option<MouseAwaiting>
// pub fn mouse_capture_required(&self) -> bool; pub fn cancel_mouse_gesture(&mut self);
// pub fn take_mouse_cleanup(&mut self) -> Option<ClientMessage>; fn terminal_mouse_enabled(&self) -> bool; fn reconcile_mouse_protocol(&mut self);
```

`None` from `encode_mouse` means the event is not representable; it never returns a truncated sequence. `pending_cleanup` holds at most one frame of at most three releases. Every path that writes a request drains cleanup first.

- [ ] **Step 1: RED tests.** Unit: exact byte tables for every supported mode and encoding, all three buttons, all wheel axes, combined modifiers, SGR coordinates above 223, legacy cell 222 accepted and 223 rejected. TUI: `browse_click_in_pane_focuses_without_bytes`, `press_outside_rectangle_is_not_clamped`, `leaving_the_rectangle_releases_at_last_valid_cell`, `ctrl_g_releases_held_buttons_before_switching`, `protocol_change_clears_held_state`, `wheel_over_a_pane_without_tracking_opens_history_at_the_tail`, `wheel_down_at_newest_row_returns_to_live`, `wheel_is_forwarded_when_tracking_is_enabled`. Acceptance: through the outer PTY with `tests/support/mouse_app.rs`, send SGR press, drag, release, and wheel and assert the exact relative bytes; then the same gestures against a plain shell show no bytes and the wheel opens history.
- [ ] **Step 2: Implement** encoder, gesture ownership through `pane_rects`, capture and focus reconciliation, snapshot and resize gating, then the wheel rule.
- [ ] **Step 3: Verify** `rtk proxy cargo test --workspace`, the acceptance suite, and a manual pass in a real terminal with Vim `:set mouse=a` and the computer-use guide for the native gate.

**Gate:** application requests control forwarding; browse ownership, Ctrl-g, keyboard and paste encoding, and terminal restoration are unchanged; held buttons never migrate between sessions; the wheel scrolls a plain shell's history and drives Vim when it asks.

---

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Document forwarding rules and wheel scrolling in the README dashboard section, and add a smoke-check step to `docs/testing-computer-use.md`: wheel over a plain shell and confirm history opens; in Vim with `:set mouse=a`, click and confirm the cursor moves.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. In Vim with `:set mouse=a` inside a pane, click, drag, and wheel; then over a plain shell, wheel up and confirm the history view opens at the tail.
2. Run the computer-use smoke check in `docs/testing-computer-use.md`.
