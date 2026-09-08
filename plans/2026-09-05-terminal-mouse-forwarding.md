# Terminal Mouse Forwarding Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Status:** Superseded on 2026-09-08 by Task 8 of `plans/2026-09-08-tui-creation.md`, which carries the final decisions and gates; this file is retained for its step-level sketches and byte tables. Its Luna execution contract no longer applies.

**Goal:** Forward mouse input to the selected terminal application only when that application requests a supported mouse protocol and the dashboard is accepting terminal input.

**Architecture:** Keep the synchronous server, existing `Request::Input` frames, and existing server/client `vt100` parsers. Map Crossterm events through the drawn terminal rectangle, encode using the selected application's parsed modes, and retain only enough local state to finish or cancel an owned button gesture. Outer capture follows dashboard ownership and application requests.

**Tech stack:** Rust 2024, Crossterm 0.29, Ratatui 0.30, vt100 0.16, portable-pty 0.9; no new dependency or thread.

**Spec references:** `README.md`, roadmap checkbox “Mouse forwarding to applications running inside a terminal”; `plans/2026-09-04-ovrcr-mvp-design.md`, Terminal data flow, Dashboard/Input modes, Local protocol, Failure handling, and Verification. The approved MVP explicitly defers this feature; this plan proposes extending that boundary.

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → session restore**. Multiple dashboards is deferred as of 2026-09-07 and excluded from this sequence until explicitly resumed. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --workspace --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Global constraints

- “The MVP targets macOS and Linux, supports up to 50 live sessions, and assumes one connected dashboard.”
- “It favors a small synchronous implementation over an async runtime.”
- “OVRCR does not use Tokio, a database, a shell-command builder, or an agent SDK.”
- “The dashboard sends input only for the selected session while in terminal mode.”
- “TUI exit or panic: Restore raw mode, cursor visibility, mouse capture, and the outer terminal's alternate screen.”
- Preserve Ctrl-g as the terminal-to-browse escape chord and preserve keyboard/paste encoding.
- Preserve the dense sidebar, fixed divider, current-screen rendering, bounded queues, and event-loop batching.

## Source grounding

Inspected baseline: `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb` on 2026-09-05. The symbol names below are the source anchors; resolve them again before implementation.

| Source | Relevant existing behavior |
|---|---|
| `crates/ovrcr-tui/src/dashboard/{mod,state,input,render,event_loop}.rs` | `Dashboard` has one `selected`, `parser`, `pane_size`, and browse/terminal mode; mouse routing, server-message handling, request construction, capture reconciliation, event batching, and drawn geometry live in these child modules. |
| `crates/ovrcr-runtime/src/session/mod.rs` | `current_screen` uses `state_formatted()`, which includes input modes. No extra mouse snapshot field is required. |
| `src/gui/input.rs` | The optional GUI helper encodes button press/release only. It does not currently emit wheel or motion input. |
| `src/bin/ovrcr-gui.rs` | The GUI input loop calls that stateless encoder; passing GUI clicks alone cannot prove drag/wheel acceptance. |
| `tests/terminal_acceptance.rs` | `OuterDashboard` drives the real dashboard executable through an outer PTY and parses its rendered output. |

Dependency source verified in the local Cargo registry: `vt100-0.16.2/src/screen.rs` defines the mode/encoding enums, DECSET/DECRST handling, `state_formatted`, and mouse accessors. It exposes `None`, `Press` (9), `PressRelease` (1000), `ButtonMotion` (1002), and `AnyMotion` (1003); encodings are `Default`, `Utf8` (1005), and `Sgr` (1006). Its current mode is the authority; do not build a second escape-sequence parser.

`crossterm-0.29.0/src/event.rs` shows that `EnableMouseCapture` requests 1000, 1002, 1003, 1015, and 1006. Therefore the outer terminal may send all motion even when the inner application requests fewer events; filtering is required. Its focus-change commands are available independently.

The [xterm mouse protocol reference](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Mouse-Tracking) supplies wire semantics: coordinates are one based; modifiers use bits 4/8/16; motion adds 32; wheels use 64–67 and have no releases. SGR uses decimal coordinates and a lowercase final `m` for release. Legacy coordinates stop at 223 and UTF-8 coordinates at 2015. X10 sends presses without modifiers. These limits inform the encoder and its boundary tests.

## Proposed behavior and boundaries

1. Browse mode retains existing sidebar ownership. Terminal-content clicks in browse mode send no application bytes. Terminal mode never uses a sidebar click to select a different session.
2. A live, ready selected session in terminal mode receives only mouse events permitted by its parser's current tracking mode. Without an application request, capture is disabled and injected/queued mouse events are still suppressed. Encoding enablement by itself does not enable tracking.
3. Outer capture is enabled in browse mode, or while terminal mouse forwarding is eligible. Reconcile capture after output/snapshot processing as well as keyboard input; applications can enable or disable tracking without producing visible text. Use the existing `EnableMouseCapture`/`DisableMouseCapture`, not per-application outer modes.
4. Forward left/middle/right presses. Forward their releases in 1000/1002/1003, drag in 1002/1003, and unpressed movement only in 1003. Forward vertical/horizontal wheel ticks in 1000/1002/1003; X10 supports the three physical button presses only. Wheel has no held-button state.
5. Use Shift/Alt/Control modifiers for normal tracking and omit them for X10. Do not implement an OVRCR Shift-click bypass; a host terminal's own selection override may intercept the gesture before Crossterm receives it.
6. Calculate `(column - inner.x, row - inner.y)` only after bounds checks. Reject metadata, divider, footer, sidebar, zero-cell layouts, and cells beyond the parser's size. A wide glyph still occupies terminal cells: do not convert mouse columns into character indices.
7. A press owns its selected session until release/cancellation. Never start a gesture from an orphan drag/release. Track up to three held buttons; repeated presses do not create duplicate ownership. Suppress duplicate motion at the same cell with the same kind and modifiers.
8. When an owned drag/release leaves the content rectangle, emit a release at the last valid position and clear that button. Do not clamp an outside press into the terminal or continue dragging through the sidebar. Re-entering while physically held does not start a new drag.
9. Before Ctrl-g, selection replacement, resize, or orderly detach, release held buttons at their last valid coordinates using the still-active encoding and old session ID. Send these cleanup bytes before a selection/resize request. They finish prior input; they do not open a new input route from browse mode.
10. Enable outer focus reporting for the dashboard lifetime. On `FocusLost`, finish owned gestures, mark mouse input unfocused, and suppress forwarding until `FocusGained`. No focus sequences are forwarded to the child application in this feature.
11. A parsed application tracking/encoding change invalidates held state. If the application has withdrawn or changed the protocol, clear ownership without sending old-protocol releases into its new input mode. A disconnected socket cannot deliver cleanup: clear local state, restore the terminal, and report the disconnect normally. Do not claim release delivery after a crash.
12. During selection/screen resynchronization, disable terminal mouse forwarding until the matching snapshot is installed. On resize, release first and block mouse input until that resize's acknowledgement. Empty/exited/removed sessions never receive new mouse input.

No historical scrolling, copy selection, pane splitting, configurable bindings, mouse-driven focus switching in terminal mode, click-to-enter behavior, clipboard, process pause, multiple dashboards, or session persistence is included. Unsupported tracking modes (1001/DEC locator) and encodings (1015/1016) remain outside the parser's advertised support; do not pretend to provide them or patch dependency parsing in this change.

## File and interface map

Production changes stay in `crates/ovrcr-tui/src/dashboard/{input,state,event_loop,mod,terminal_guard}.rs` and `render.rs` only where the existing terminal rectangle is consumed. Existing runtime/protocol/server code needs no new wire type. Add focused tests to root `tests/tui.rs` and `tests/terminal_acceptance.rs`; add the raw fixture in `tests/support/mouse_app.rs`. `src/lib.rs` continues to re-export the TUI facade and `src/main.rs`/`src/cli/` are outside this feature. `README.md` can be updated only during later implementation, after acceptance; this planning task edits this plan alone.

Expose the pure encoder alongside `encode_key` and `encode_paste`:

```rust
pub fn encode_mouse(
    event: MouseEvent, // zero-based coordinates already relative to terminal
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
) -> Option<Vec<u8>>;
```

The encoder filters the protocol's event classes and representability. It does not know about the sidebar, selected session, readiness, or held-button ownership. `None` means no complete event can be represented; never return a truncated escape sequence.

Keep private state types local to `tui.rs`:

```rust
struct HeldMouse {
    session: SessionId,
    event: MouseEvent, // latest valid relative cell/modifiers; kind identifies button
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
}

#[derive(Default)]
struct MouseForwarding {
    held: [Option<HeldMouse>; 3],
    last_motion: Option<MouseEvent>,
    pending_cleanup: Option<ClientMessage>,
}

enum MouseAwaiting {
    Screen { request_id: u64, session: SessionId },
    Resize { request_id: u64, session: SessionId },
}
```

Add `mouse: MouseForwarding`, `mouse_focused: bool` (initially true), `mouse_ready: bool` (initially false), and `mouse_awaiting: Option<MouseAwaiting>` to `Dashboard`. `pending_cleanup` is bounded to one frame containing at most three release sequences for one session; no queue growth is allowed. Consume it before processing another transition.

```rust
// Dashboard methods, with public visibility only where integration tests use them:
pub fn mouse_capture_required(&self) -> bool;
pub fn cancel_mouse_gesture(&mut self); // prepare bounded old-session cleanup
pub fn take_mouse_cleanup(&mut self) -> Option<ClientMessage>;
fn terminal_mouse_enabled(&self) -> bool;
fn reconcile_mouse_protocol(&mut self); // clear state after parsed mode changes
```

`terminal_mouse_enabled` combines terminal input mode, focus, readiness, selected running session, and non-None tracking. Store the old session ID in cleanup rather than calling `input_request` after selection changes. Every production path must drain cleanup before writing its ordinary action/request, including `next_dashboard_messages`, `process_dashboard_input`, and the event loop's direct resize paths.

## Current integration paths and task checkpoints

### Workspace handoff

- Mouse routing and parser-mode inspection belong to `ovrcr-tui::dashboard::{input,state,event_loop}` with rendering geometry in `dashboard::render`; the application imports the public facade from `ovrcr_tui` through root `src/lib.rs`.
- The selected-session identity, terminal size, and other shared request records are re-exported by `ovrcr_protocol`; their wire definitions remain in `crates/ovrcr-protocol/src/wire.rs`. `MouseProtocolMode` and `MouseProtocolEncoding` are VT100 types reached through the `ovrcr_terminal::vt100` re-export, while PTY/parser ownership stays in `ovrcr_runtime::session`; no new runtime or protocol message is needed.
- Focused checks are `rtk proxy cargo test -p ovrcr --test tui mouse_ -- --nocapture`, `rtk proxy cargo test -p ovrcr --test terminal_acceptance mouse_ -- --nocapture`, and `rtk proxy cargo check -p ovrcr-tui --all-targets`; the real executable path is `rtk proxy cargo run -p ovrcr --`.
- The root `src/gui.rs`, `src/gui/input.rs`, `src/bin/ovrcr-gui.rs`, and `tests/gui.rs` remain optional helper surfaces. Their current click-only encoder cannot establish wheel/drag acceptance and is not part of this plan's production path.

In the recommended order, split panes and copy/history already exist. Use the landed `pane_rects` and focused `PaneState` as the only geometry/parser source. Browse retains sidebar/pane focus clicks; History and Copy consume mouse without PTY forwarding; Terminal forwards only inside the ready, Running, focused pane. A click in the other pane while in Terminal must neither forward there nor switch focus. Paused sessions never admit new mouse input.

Reuse the pane's view/readiness state instead of introducing an independently authoritative `mouse_ready` boolean. For a singleton baseline the `MouseAwaiting` sketches below apply; with SetView, eligibility requires the focused pane's matching Screen **and** that view request's final Ok. A snapshot for another pane, old revision, or old request cannot enable capture. Resize and dirty refresh clear readiness before the request and reconcile capture after the new view completes. Output that only changes DEC mouse modes must still trigger reconciliation.

Extend held gesture identity to include the committed view revision. Build cleanup using the old `(session, revision)` before changing focus, mode, or geometry, and drain its single bounded frame before SetView. Later multiple-dashboard integration also tags the old control epoch: if authority was already revoked, discard held state without sending unauthorized cleanup. A server pause/exit similarly clears state; cleanup cannot bypass server input admission. On voluntary Ctrl-g/resize while authority is valid, cleanup is the last admitted terminal action, then the mode/view transition proceeds.

Define the response-order test in Task 3 as: press → request new view → receive wrong-pane/old-revision Screen → receive matching focused Screen → receive final Ok. Assert exactly one old-session release before SetView, no mouse forwarding through the pending interval, and restored eligibility only at final Ok. For protocol withdrawal, assert held state clears with **no** old-protocol release. Add History, Copy, and Paused to `mouse_state_capture_tracks_output_and_focus`, avoiding new mode-specific routing copies.

The pure encoder tests belong in `tests/tui.rs` and may use its local fixture. The raw child fixture belongs to `tests/terminal_acceptance.rs` via `#[path = "support/mouse_app.rs"] mod mouse_app;` (or the file's existing support module); invoke its ignored helper in that same integration-test executable. Do not add a new ordinary integration binary merely to access its private OuterDashboard. The ignored helper is excluded from pass counts; actual acceptance must show emitted mouse bytes reaching the owned child's raw input, not terminal echo.

In Task 3, remove `run_dashboard`'s final `guard.mark_mouse(dashboard.mode == InputMode::Browse)` assumption. The guard already enabled capture at entry; keep its lifetime cleanup obligation through exit even when capture was temporarily disabled. Unconditionally issuing its idempotent DisableMouseCapture on restoration is sufficient; do not add shared state just to avoid that command. Track focus-reporting teardown with the same lifetime discipline. Test enabling child mouse mode while in Terminal followed by normal/error exit and require both capture/focus disable bytes in outer output.

Centralize outbound ordering in `send_dashboard_action(stream, dashboard, action) -> anyhow::Result<()>`: take/write pending cleanup once, then write the ordinary request/action. Route input, resize, focus, and detach through this helper. Cleanup is drained on successful socket write, not held awaiting a separate acknowledgement; a second cancellation with no held buttons emits nothing. If cleanup write fails, stop and propagate the connection error without sending the later action. Handle FocusLost explicitly in `process_dashboard_input`: cancel while old eligibility is valid, mark unfocused, drain cleanup, and redraw/reconcile capture; FocusGained only restores focus and re-evaluates eligibility.

Launch the raw fixture with `std::env::current_exe()` plus `--ignored --exact mouse_fixture_child --nocapture`, through an ordinary managed CreateSession request in the fixture workspace. Pass test-only mode/marker values through that child argv/environment, never global environment mutation. Add any launch convenience method directly on the local AcceptanceFixture and show its CreateSession body; there is no existing `start_mouse_fixture` API. Its recorded PGID joins the fixture's existing bounded cleanup.

Use [computer-use testing](../docs/testing-computer-use.md) for the native gate and record click, release, drag, wheel, focus loss, and terminal restoration separately. The optional GUI helper's current press/release encoder cannot establish wheel or drag behavior. No GUI input implementation is required by this plan.

## Task 1: Encode the parser's supported mouse protocols

**Files:** `crates/ovrcr-tui/src/dashboard/input.rs`; `tests/tui.rs`.

**Consumes:** Crossterm `MouseEvent`, vt100 mouse enums. **Produces:** `encode_mouse` above.

- [ ] Add `mouse_encode_sgr_events`, `mouse_encode_tracking_filters`, and `mouse_encode_legacy_boundaries`. Use table assertions inside these three tests rather than one test per permutation.

```rust
let event = MouseEvent {
    kind: MouseEventKind::Down(MouseButton::Left),
    column: 2, row: 3, modifiers: KeyModifiers::SHIFT | KeyModifiers::CONTROL,
};
assert_eq!(encode_mouse(event, vt100::MouseProtocolMode::PressRelease,
    vt100::MouseProtocolEncoding::Sgr), Some(b"\x1b[<20;3;4M".to_vec()));
let up = MouseEvent { kind: MouseEventKind::Up(MouseButton::Left), ..event };
assert_eq!(encode_mouse(up, vt100::MouseProtocolMode::PressRelease,
    vt100::MouseProtocolEncoding::Sgr), Some(b"\x1b[<20;3;4m".to_vec()));
assert_eq!(encode_mouse(up, vt100::MouseProtocolMode::Press,
    vt100::MouseProtocolEncoding::Default), None);
```

- [ ] Run `rtk proxy cargo test -p ovrcr --test tui mouse_encode_ -- --nocapture`; expect RED before the function exists and exactly 3 passing tests after implementation. Zero matching tests is failure.
- [ ] Implement button IDs 0/1/2, unpressed movement 3+32, drag button+32, and wheel up/down/left/right 64/65/66/67. Add modifiers except in X10. Use SGR's `m` only for `Up`; use legacy release button 3 plus modifiers. Serialize legacy fields with checked additions/conversions.

```rust
// For legacy/UTF-8 after class filtering, code selection, and zero-based input:
let x = u32::from(event.column) + 1;
let y = u32::from(event.row) + 1;
let limit = if encoding == vt100::MouseProtocolEncoding::Utf8 { 2015 } else { 223 };
if x > limit || y > limit { return None; }
// Prefix ESC [ M, then code+32, x+32, y+32; UTF-8 encodes each scalar.
```

- [ ] Complete the tables with all supported event classes in every mode, all three button IDs, all wheel axes, combined modifiers, SGR coordinates above 223, legacy cell 222 accepted and 223 rejected (zero based), UTF-8 cell 2014 accepted and 2015 rejected, and `u16::MAX`. Out-of-range input produces no bytes, never wraps or truncates. Verify X10 drops wheel/motion/release and strips modifiers.

## Task 2: Route owned gestures through the real terminal geometry

**Files:** `crates/ovrcr-tui/src/dashboard/{input,state,event_loop,terminal_guard}.rs`; `tests/tui.rs`.

**Consumes:** Task 1 encoder and `actual_drawn_inner_rect`. **Produces:** terminal branch of `mouse_action`, held-state cancellation, and bounded cleanup.

- [ ] Add `mouse_route_geometry_and_sidebar`, `mouse_route_drag_release_and_modifiers`, and `mouse_route_cancellation_keeps_old_session`. Reuse `dashboard_fixture`, feed `\x1b[?1002h\x1b[?1006h`, and install a matching screen response through the real readiness path.

```rust
let area = Rect::new(0, 0, 120, 40);
let inner = ovrcr::tui::actual_drawn_inner_rect(area);
let click = MouseEvent {
    kind: MouseEventKind::Down(MouseButton::Left),
    column: inner.x + 2, row: inner.y + 3, modifiers: KeyModifiers::NONE,
};
assert_eq!(d.mouse_action(click, area),
    DashboardAction::PtyBytes(b"\x1b[<0;3;4M".to_vec()));
d.cancel_mouse_gesture();
let cleanup = d.take_mouse_cleanup().unwrap();
assert!(matches!(cleanup.request, Request::Input { session, bytes }
    if session == SessionId(1) && bytes == b"\x1b[<0;3;4m"));
assert!(d.take_mouse_cleanup().is_none());
```

- [ ] Run `rtk proxy cargo test -p ovrcr --test tui mouse_route_ -- --nocapture`; expect RED, then exactly 3 passing tests.
- [ ] Branch on browse versus terminal ownership first. Preserve current sidebar logic in browse. In terminal mode, reject an invalid rectangle before subtraction, intersect dimensions with `screen.size()`, gate through `terminal_mouse_enabled`, then call `encode_mouse`. Record a held press only when complete bytes are emitted.
- [ ] For motion, require an owned button for `Drag`; permit `Moved` only with no held button. Track the last delivered event to suppress duplicates. Outside drag/release cancels ownership using the last valid position. Treat a coordinate beyond the negotiated encoding limit the same way: release at the last representable cell rather than silently dropping the final release. An orphan release does nothing. A release clears only its matching button; check a two-button chord and its two ordered releases.
- [ ] Add cancellation before selected-ID mutation in `select_session`, `move_selection`, and `select_request`, before resize, and on Ctrl-g. Concatenate held releases into the one old-session cleanup frame, allocate its request ID once, and clear ownership immediately. Prevent a second transition from overwriting undrained cleanup; central call sites drain it synchronously.
- [ ] Verify clicks at every content edge, nonzero outer origins, metadata/sidebar/footer exclusion, zero-sized layout, parser smaller than drawn area, drag outside then back, duplicate motion, and replacement with a different session. None of these may produce a fresh press in the wrong PTY.

## Task 3: Reconcile capture, focus, snapshots, and event-loop cleanup

**Files:** `crates/ovrcr-tui/src/dashboard/{input,state,event_loop,terminal_guard}.rs`; `tests/tui.rs`.

**Consumes:** Task 2 state/cleanup. **Produces:** mode-responsive capture and an input route safe across snapshot and geometry changes.

- [ ] Add `mouse_state_capture_tracks_output_and_focus`, `mouse_state_snapshot_and_resize_readiness`, and `mouse_state_protocol_changes_cancel_ownership`.

```rust
assert!(!d.mouse_capture_required()); // ready terminal with tracking disabled
d.parser.process(b"\x1b[?1000h\x1b[?1006h");
assert!(d.mouse_capture_required());
d.event_action(Event::FocusLost);
assert!(!d.mouse_capture_required());
d.event_action(Event::FocusGained);
assert!(d.mouse_capture_required());
d.parser.process(b"\x1b[?1000l");
assert!(!d.mouse_capture_required());
```

- [ ] Run `rtk proxy cargo test -p ovrcr --test tui mouse_state_ -- --nocapture`; expect RED, then exactly 3 passing tests.
- [ ] Set readiness false whenever requesting a selected screen or resize, record that request/session, and allow mouse again only on its matching `Screen`/resize `Ok`. Ignore stale screen replies rather than letting them replace the parser. Clear readiness on matching error, selected exit/removal, and connection loss. Keep mouse blocked while a `ScreenDirty` refresh is outstanding.
- [ ] Around output/snapshot parser changes, compare tracking/encoding; call `reconcile_mouse_protocol` and clear invalid ownership. A fresh reattach starts with empty ownership and learns modes from `state_formatted`; add a snapshot round-trip assertion for each supported tracking mode and encoding.
- [ ] Make `update_mouse_capture` use `mouse_capture_required`. Reconcile after bounded server batches and immediately after input transitions, retaining the current 64-message/32-event batching and frame cadence. Remove the separate EnterBrowse capture special case so there is one capture policy.
- [ ] Add `EnableFocusChange`/`DisableFocusChange` to terminal setup/guard teardown, including partial-setup and panic restoration. Dispatch focus events through `event_action`; `FocusLost` cancels before disabling forwarding. Teardown must restore capture even if the last mode was terminal-with-mouse; do not derive guard cleanup solely from `mode == Browse`.
- [ ] Drain `take_mouse_cleanup` before every ordinary input/request emission and before every direct resize emission. In `next_dashboard_messages`, write cleanup before the returned `Select` refresh. On socket errors, use the existing propagated error/RAII cleanup and do not retry writes into an unknown session.
- [ ] Preserve geometry-free `event_to_request` as keyboard/paste only; it must not guess a mouse rectangle. Route its Ctrl-g through the same cancellation method, leaving cleanup available to its caller. Test both Ctrl-g entry points. Keep the existing keyboard/paste checks in their current suites.

## Task 4: Prove enabled and disabled forwarding through an outer PTY

**Files:** `tests/terminal_acceptance.rs`; create `tests/support/mouse_app.rs`.

**Consumes:** real dashboard/server/input path. **Produces:** one new acceptance test plus an explicitly ignored child fixture entry point.

- [ ] Add `mouse_forwarding_outer_pty_round_trip` and `#[ignore] fn mouse_fixture_child()`. The latter calls `support::mouse_app::run() -> anyhow::Result<()>`; invoke it as a child with the current integration-test executable, `--ignored --exact mouse_fixture_child --nocapture`. It is a fixture, not a claimed passing acceptance test.
- [ ] Implement the child with libc termios raw mode, a restoration guard, blocking stdin read, and flushed stdout. After entering raw mode, print and flush `MOUSE_FIXTURE_READY`; acceptance waits for this marker before sending input. Commands `E`, `D`, and `Q` are control bytes: `E` emits 1002+1006 enable and `MOUSE_ENABLED`; `D` disables 1002 and emits `MOUSE_DISABLED`; `Q` emits a monotonically numbered checkpoint plus hex of all other received bytes, then clears that buffer. Parse these ASCII controls only outside collected CSI mouse reports, so the `M` report terminator is ordinary data. No Python or external terminal application is required.

```rust
// Fixture checkpoint core; all reports are accumulated as bytes, not interpreted:
checkpoint += 1;
let hex = observed.iter().map(|b| format!("{b:02x}")).collect::<String>();
writeln!(stdout, "\r\nMOUSE_CHECK_{checkpoint}:{hex}:END\r")?;
stdout.flush()?;
observed.clear();
```

- [ ] Extend `AcceptanceFixture` with `start_mouse_fixture(&mut self) -> Result<()>`: use its existing `new --project fixture --workspace work` path, name the session `mouse-protocol`, launch the test executable with the exact fixture arguments, and refresh `managed_pgids`. Reuse existing drop/shutdown behavior.
- [ ] Select that session through the real sidebar/browse keys, enter terminal mode, send `E`, and wait for the fresh marker plus parsed outer mouse mode becoming enabled. Add `OuterDashboard::wait_for_mouse_capture(&mut self, enabled: bool, timeout: Duration) -> Result<()>`, which drains actual bytes and checks the outer parser's mode with a deadline.
- [ ] Send SGR press, drag, release, and wheel at positions derived from `actual_drawn_inner_rect`, followed by `Q`. Assert the exact relative bytes from the checkpoint, for example:

```rust
let inner = ovrcr::tui::actual_drawn_inner_rect(Rect::new(0, 0, 100, 30));
dashboard.send(format!("\x1b[<0;{};{}M", inner.x + 3, inner.y + 4).as_bytes())?;
dashboard.send(format!("\x1b[<0;{};{}mQ", inner.x + 3, inner.y + 4).as_bytes())?;
dashboard.wait_for(b"MOUSE_CHECK_1:1b5b3c303b333b344d1b5b3c303b333b346d:END",
    Duration::from_secs(3))?;
```

- [ ] Send `D`; wait for disabled marker and capture-off state. Inject the same outer reports followed by `Q` and require the next checkpoint's empty payload. This ordered checkpoint proves suppression; a sleep or absence of screen output does not.
- [ ] Re-enable, hold a button, send Ctrl-g, select the existing `mouse` shell, enter it, and verify `MOUSE_TOKEN` reaches that shell. Return to the fixture and checkpoint its cleanup release. Check no duplicate release after an orphan Up. Exercise sidebar/metadata exclusion, resize while held, focus-out while held, and fresh forwarding after focus-in.
- [ ] Detach and reattach while the fixture still requests mouse; select it, enter terminal mode, and require capture plus a fresh encoded click without sending `E` again. Finish via existing bounded shutdown, child joins, socket absence, and process-group absence checks.
- [ ] Run `rtk proxy cargo test -p ovrcr --test terminal_acceptance mouse_forwarding_outer_pty_round_trip -- --exact --nocapture`; expect RED before integration and exactly 1 passing test afterward. Any missing marker, wrong byte, ignored acceptance, or timeout is failure.

## Task 5: Focused regression and visible acceptance

**Files:** existing tests; later implementation may update `README.md` after observed acceptance.

- [ ] Run `rtk proxy cargo test -p ovrcr --test tui`; require all discovered nonignored tests to run, including the 9 new mouse tests. Record the actual nonzero total; do not invent a baseline total.
- [ ] Run `rtk proxy cargo test -p ovrcr --test terminal_acceptance default_dashboard_acceptance_wrapper_exercises_pty_controls -- --exact --nocapture`; require exactly 1 test, existing nonzero latency samples, and its p95 assertions.
- [ ] Run `rtk proxy cargo test -p ovrcr-tui --lib -- --nocapture` for the TUI unit suite (3 existing dashboard tests), then `rtk proxy cargo test -p ovrcr --test tui terminal_guard_ -- --nocapture` for the 3 existing root terminal-guard tests. Run `rtk proxy cargo fmt --all -- --check` and `rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings` after source changes.
- [ ] In a real macOS/Linux terminal, use a mouse-aware application such as Vim with `:set mouse=a`: click to position, drag selection, wheel, press Ctrl-g, change sessions, resize while held, and return. Record the app/host versions and visible result. Turn application mouse off and verify host selection behavior returns. Unavailable platform/UI evidence remains unverified.
- [ ] The disposable GUI helper can validate clicks and Ctrl-g with `rtk proxy just gui`, but its current encoder lacks drag/wheel. Use a real terminal for the complete gate. If extending that helper is separately authorized, change `src/gui/input.rs`, `src/bin/ovrcr-gui.rs`, and `tests/gui.rs` to add pointer position/button state and wheel/motion translation; do not claim those events were covered by today's click-only helper.

## Acceptance and feature interactions

- Application requests control forwarding; enabling only SGR encoding sends nothing. Mode changes from output and snapshots change capture without a keyboard event.
- Exact byte assertions cover supported modes/encodings and coordinate limits. Real outer PTY evidence proves events cross Crossterm, dashboard framing, server routing, and the managed PTY.
- Browse/sidebar ownership, terminal geometry, Ctrl-g, keyboard/paste, and terminal restoration remain intact. Held buttons cannot migrate to another session; orderly transitions deliver cleanup before changing targets.
- **Split panes:** no dependency. Implement against today's single `selected`/`parser` and `actual_drawn_inner_rect`. If `plans/2026-09-05-split-panes.md` lands first, replace that lookup with its `pane_rects` and ready focused `PaneState`; permit forwarding only inside the focused visible pane. Browse clicks may focus per that plan, but terminal clicks on an unfocused pane do nothing. Cancel before focus/view revision changes and retain the gesture's original session ID. Do not create a separate geometry model or assume sibling APIs exist today.
- **Copy mode/history:** no dependency. If implemented first, extend `terminal_mouse_enabled` to require live viewport and absence of copy mode. Their entry transition must call cancellation while still in live terminal mode, then suppress new mouse/keyboard/paste input. A scrollback screen or selection parser must never supply application mouse modes.
- **Multiple dashboards:** no dependency. This proposal keeps the existing single-dashboard authority. A later implementation must apply its input-owner check to cleanup as well as ordinary mouse reports.
- **Known limits:** child mode changes and incoming user events are ordered only as the current socket/event loop observes them. Do not promise zero stale input across unseen child output. Host modifier interception, physical releases lost outside the host window, and abrupt dashboard death cannot be repaired by byte encoding; observed focus loss and orderly transitions are covered. Avoid an unrequested server-side gesture protocol or timer workaround.

## Self-review

The plan covers the README checkbox while retaining synchronous ownership and current rendering. Verified parser/snapshot APIs eliminate a new protocol field or dependency. Standalone geometry, future pane adaptation, input suppression, cancellation, capture restoration, and actual PTY acceptance are explicit. Test commands describe future RED/GREEN checks with named nonzero counts; no implementation, test execution, or GUI success is claimed here.
