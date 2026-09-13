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

- [x] Add failing event-path assertions: terminal-mode click changes focus without Input bytes; sidebar label selects without collapsing, disclosure toggles; wheel uses scrolled rows; divider drag changes both desired sizes and disables input until acknowledgement; no-op drag preserves readiness; tiny/nonzero-origin layouts and focus loss end drag safely.
- [x] Run focused tests and retain the expected behavioral failures.
- [x] Implement gesture routing before terminal forwarding, shared split geometry, clamped divider drag, and sidebar controls. Reuse `focus_pane`, view coalescing and held release paths.
- [x] Verify affected TUI suites and self-review; commit only stage files.
- [x] Independent review of committed diff and evidence, then correct findings.

## Task 2: Palette, forms, menus and discoverability

Files: `dashboard/{palette,picker,whichkey,hints,render,state}.rs`, focused palette tests.

Interfaces: add `Dashboard::palette_mouse(MouseEvent, Rect) -> DashboardAction`; route it from `mouse_action` while palette is open and keep capture enabled. Reuse palette commands/request generation; keyboard and mouse text edits share the same cursor state.

- [x] Write failing tests using rendered cell coordinates: select nonactive field; Unicode cursor insertion/deletion; toggle; select scrolled pick/path row; Submit/Cancel; search action; pending request/confirmation/outside overlay handling; narrow form scrolling.
- [x] Run RED tests, implement shared render/hit geometry and text editing cursor, rerun to GREEN.
- [x] Add visible clickable action entry points so mouse users can open menus/forms without keyboard shortcuts; preserve compact dense layout.
- [x] Verify affected suites, self-review, commit and independently review.

## Task 3: Tasks and Copy/History

Files: `crates/ovrcr-tui/src/task_tui.rs`, `dashboard/{copy,state,render}.rs`, task/copy/history tests.

Interfaces: Tasks mouse input returns existing task actions through dashboard dispatch. Copy/History mouse selection uses existing captured-session and clipboard lifecycle, without separate selection or request ownership.

- [x] Add failing assertions for Tasks list selection/scroll, editor fields/cursor/pickers, task controls/confirmation, transcript scrolling; Copy/History drag including wide characters, captured identity, resize and stale clipboard completion; Copy/Close controls and overlay priority.
- [x] Run RED tests; implement render-consistent hit testing and reuse existing actions; rerun GREEN.
- [x] Verify focused suites, self-review, commit and independent review.

## Task 4: Integration and acceptance

Files: `docs/dashboard.md`, `docs/testing-computer-use.md`, acceptance tests if needed, this plan's checkpoint, work diary.

- [x] Update mouse reference and GUI acceptance procedure to cover each approved interaction.
- [ ] Run `rtk proxy cargo test --workspace --all-targets --all-features`, `rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings`, `rtk proxy cargo fmt --all -- --check`, and `rtk proxy git diff --check` once at the integrated checkpoint.
- [x] Exercise actual GUI: switch panes from active terminal, drag divider and verify real PTY sizes, collapse/scroll tree, edit/submit/cancel forms, task controls, Copy/History drag, and application mouse forwarding. Preserve independent evidence for unavailable platforms or checks.
- [x] Independently review final committed branch against contract; correct findings, update checkpoint and work diary. Do not merge or push without authorization.

## Acceptance checkpoint — 2026-09-10

The mouse implementation is complete and independently approved at `3db051d5a28927a9be735c17fd14ee0f948c6151`. Final affected suites passed: 180 dashboard integration tests, 49 TUI library tests, and 28 Tasks tests. Final workspace Clippy with warnings denied, formatting, and diff checks passed.

**The final full-workspace test gate remains open.** The earlier `fbd6dc7` run passed 540 tests with 6 ignored. Two default full-workspace runs at the final commit failed while `timeout_cancel_and_parent_loss_remove_detached_descendants` waited for fixture startup. A serial full-workspace run failed during `split_server_two_streams_resize_resync_and_detach` cleanup. Both tests passed individually with all features; the entire task-runner test binary also passed independently. These test files and runtime code are unchanged by this feature. No assertions were weakened or skipped, and the failed logs are retained. See `workspace-retry-investigation.md` in the evidence directory. A clean final full-workspace pass is still needed.

Native macOS acceptance used owned disposable fixtures: menu/pane focus, divider widths checked with kernel `stty size`, narrow-window pane restoration, sidebar collapse/scroll, forms and path/toggle controls, Tasks save/pause/resume/delete confirmation, Copy/History drag and Close, frozen History during new live output, and application SGR press/release. All owned GUI fixture processes, groups, sockets and roots were cleaned. Native evidence is from `773a428` and `fbd6dc7`; the final overlay acquisition correction has deterministic dashboard event-path RED/GREEN evidence, with no additional native claim.

Native clipboard delivery, provider execution, Linux UI behavior, and Tasks Unicode text injection remain unverified. Headless Unicode editing and clipboard lifecycle tests pass. Evidence is retained under `.superpowers/sdd/2026-09-09-first-class-mouse/`.
