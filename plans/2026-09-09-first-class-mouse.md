# First-class Mouse Implementation Plan

> Execute with subagent-driven-development, one implementer and independent review per stage.

**Goal:** Make OVRCR's existing dashboard controls usable with the mouse, including dragging pane dividers.

**Architecture:** Use the existing keyboard actions and shared layout geometry for mouse hit testing. Keep one geometry authority for drawing, subscriptions, cursor placement, and input. Keep terminal mouse forwarding separate from dashboard control gestures.

**Tech stack:** Existing Rust, Ratatui, Crossterm, vt100, synchronous runtime. No dependencies or protocol changes.

**Spec:** User-approved interaction design in this task, reproduced below with the approved pane-resizing addition.

## Approved interaction contract

- Panes: click to focus and type; a focus-changing click must not reach the newly selected application. Drag the divider between the supported two panes to resize them. Clamp to usable widths; preserve layout preference across window resize and temporary hiding.
- Sidebar: click row labels to select; click disclosure arrows to collapse/expand; wheel scrolls the tree. Usable from Terminal as well as Browse mode.
- Forms: click fields, position text cursor, select options, toggle settings, browse directories, and use visible Submit/Cancel buttons.
- Actions and Tasks: clickable menus, task rows, controls, and explicit confirmations for destructive actions.
- Copy/History: drag to select text, wheel to scroll History, clickable Copy/Close.
- Clicks follow visible geometry. Overlays capture input without click-through. Terminal applications retain their mouse protocols and held-button cleanup.

## Global constraints

- Preserve one server owner, one active dashboard, two-pane limit, and supported 50 sessions.
- Runtime owns live PTYs; TUI owns interaction state. No protocol changes, dependencies, or persistent layout configuration required.
- Revoke readiness on focus/geometry changes; use the existing coalesced view request and matching snapshot/acknowledgement barrier. Never emit zero PTY dimensions.
- Shared geometry includes scrolling, Unicode display width, clipping, tiny windows and pane metadata boundaries.
- Use existing requests and confirmation paths; mouse actions must not bypass pending-request guards or leak input through overlays.
- All shell commands use `rtk`; use `rtk proxy` for raw test output. Keep distinct evidence logs, including RED attempts and revision/count/exit status.
- Native acceptance uses an isolated owned `just gui` fixture, screenshots/accessibility text, real output, and verified cleanup.

## Task 1: Pane and sidebar control

Files: `crates/ovrcr-tui/src/dashboard/{mod,state,render}.rs`, a focused mouse module if needed, `tests/tui/` and dashboard unit tests.

Interfaces: preserve public `pane_rects(area: Rect, pane_count: usize, focused: usize) -> Vec<PaneRects>` compatibility; introduce a dashboard-owned geometry method so all live callers consume the same split preference. Mouse entry remains `Dashboard::mouse_action(MouseEvent, Rect) -> DashboardAction`.

- [ ] Add failing event-path assertions: terminal-mode click changes focus without Input bytes; sidebar label selects without collapsing, disclosure toggles; wheel uses scrolled rows; divider drag changes both desired sizes and disables input until acknowledgement; no-op drag preserves readiness; tiny/nonzero-origin layouts and focus loss end drag safely.
- [ ] Run focused tests and retain the expected behavioral failures.
- [ ] Implement gesture routing before terminal forwarding, shared split geometry, clamped divider drag, and sidebar controls. Reuse `focus_pane`, view coalescing and held release paths.
- [ ] Verify affected TUI suites and self-review; commit only stage files.
- [ ] Independent review of committed diff and evidence, then correct findings.

## Task 2: Palette, forms, menus and discoverability

Files: `dashboard/{palette,picker,whichkey,hints,render,state}.rs`, focused palette tests.

Interfaces: add `Dashboard::palette_mouse(MouseEvent, Rect) -> DashboardAction`; route it from `mouse_action` while palette is open and keep capture enabled. Reuse palette commands/request generation; keyboard and mouse text edits share the same cursor state.

- [ ] Write failing tests using rendered cell coordinates: select nonactive field; Unicode cursor insertion/deletion; toggle; select scrolled pick/path row; Submit/Cancel; search action; pending request/confirmation/outside overlay handling; narrow form scrolling.
- [ ] Run RED tests, implement shared render/hit geometry and text editing cursor, rerun to GREEN.
- [ ] Add visible clickable action entry points so mouse users can open menus/forms without keyboard shortcuts; preserve compact dense layout.
- [ ] Verify affected suites, self-review, commit and independently review.

## Task 3: Tasks and Copy/History

Files: `crates/ovrcr-tui/src/task_tui.rs`, `dashboard/{copy,state,render}.rs`, task/copy/history tests.

Interfaces: Tasks mouse input returns existing task actions through dashboard dispatch. Copy/History mouse selection uses existing captured-session and clipboard lifecycle, without separate selection or request ownership.

- [ ] Add failing assertions for Tasks list selection/scroll, editor fields/cursor/pickers, task controls/confirmation, transcript scrolling; Copy/History drag including wide characters, captured identity, resize and stale clipboard completion; Copy/Close controls and overlay priority.
- [ ] Run RED tests; implement render-consistent hit testing and reuse existing actions; rerun GREEN.
- [ ] Verify focused suites, self-review, commit and independent review.

## Task 4: Integration and acceptance

Files: `docs/dashboard.md`, `docs/testing-computer-use.md`, acceptance tests if needed, this plan's checkpoint, work diary.

- [ ] Update mouse reference and GUI acceptance procedure to cover each approved interaction.
- [ ] Run `rtk proxy cargo test --workspace --all-targets --all-features`, `rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings`, `rtk proxy cargo fmt --all -- --check`, and `rtk proxy git diff --check` once at the integrated checkpoint.
- [ ] Exercise actual GUI: switch panes from active terminal, drag divider and verify real PTY sizes, collapse/scroll tree, edit/submit/cancel forms, task controls, Copy/History drag, and application mouse forwarding. Preserve independent evidence for unavailable platforms or checks.
- [ ] Independently review final committed branch against contract; correct findings, update checkpoint and work diary. Do not merge or push without authorization.
