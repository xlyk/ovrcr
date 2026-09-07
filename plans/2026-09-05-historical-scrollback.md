# Historical Scrollback Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Goal:** Let a dashboard revisit a session's retained output beyond its current screen while that session continues running.

**Architecture:** Keep 512 physical scrollback rows in each server-owned `vt100` parser. On entering history, clone its screen once at an ordered dispatcher boundary and serve bounded cell tiles from that immutable snapshot; the dashboard continues parsing live output separately. Reading anchors refer to snapshot rows, so new output, eviction, and resize cannot move the text being read.

**Tech Stack:** Existing Rust 2024 package, synchronous threads and bounded channels, `vt100` 0.16.2 as resolved in `Cargo.lock`, Serde/Bincode, Ratatui and Crossterm. No new dependencies, database, parser fork, or runtime.

**Spec:** The unchecked historical-scrollback item in [`README.md`](../README.md); architecture constraints in [`2026-09-04-ovrcr-mvp-design.md`](2026-09-04-ovrcr-mvp-design.md). The MVP explicitly excludes history; this is a **proposed extension**, not an approved feature specification or an implemented result. Defaults below are assumptions for review.

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → multiple dashboards → session restore**. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --workspace --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Global constraints and boundaries

- Preserve: “Readers send chunks through bounded, lossless queues to one ordered dispatcher”. Every raw byte still reaches the live parser, including background/final output.
- Preserve: “Socket writes never block the parser dispatcher.” History uses the existing bounded response delivery and disconnect behavior.
- Preserve: “The server stores live sessions in memory.” History disappears on session removal or server death; no disk transcripts or restoration.
- Preserve the single-dashboard synchronous architecture and tested 50-session target. No global terminal geometry restriction or new session admission policy.
- Keep existing current-screen snapshot-before-output ordering, dirty-screen resynchronization, and final-output-before-exit behavior.
- This plan covers history navigation only. Selection, clipboard, searching, shell integration, logical-line reflow, configurable retention, multiple dashboards, and split panes are separate features.
- Commands shown here are future implementation checks, not results from plan authoring. Every shell command uses `rtk`; require nonzero executed-test counts.

## Current code grounding

Inspected at Git HEAD `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb`; recheck HEAD and the working tree before implementation.

| Location | Existing behavior and planned change |
| --- | --- |
| `crates/ovrcr-runtime/src/session/mod.rs`, `apply_event`, `resize`, `current_screen`, `terminal_text`, `send_text`, `wait_for_output` | Parser currently has zero history. Add ring retention and capture entry points under one terminal-state lock. |
| `crates/ovrcr-runtime/src/server/dispatch.rs` and `crates/ovrcr-runtime/src/server/mod.rs`, dispatcher entry points | Ordered event processing and selected output. Add session-scoped history requests through the dispatcher. |
| `crates/ovrcr-runtime/src/server/connections.rs` and `outbound.rs` | Bounded messages, dirty notifications, and the connection owner. Store one history snapshot with that owner and enqueue ordinary responses. |
| `crates/ovrcr-protocol/src/wire.rs` (shared wire declarations); `crates/ovrcr-protocol/src/codec.rs` (framing and `MAX_FRAME_BYTES = 1_048_576`) | Add bounded tiled history messages, without sending a full history frame. |
| `crates/ovrcr-tui/src/dashboard/{state,input,render}.rs` and `dashboard/mod.rs` | One live parser and Browse/Terminal modes. Add separate frozen history navigation/rendering state. |
| `tests/server_lifecycle.rs` | Real `ControlFixture`, dirty recovery, selected-snapshot ordering and 50-session lifecycle gates. Extend these patterns. |
| `tests/tui.rs`, `tests/terminal_acceptance.rs` | `dashboard_fixture`, Ratatui buffers and real outer PTY acceptance harness. Reuse them. |

The locally resolved vt100 sources establish the feasibility and limits: `Screen` is `Clone`; `set_scrollback(usize::MAX)` clamps to retained length; `cell` and `row_wrapped` read the visible offset. `grid.rs::scroll_up` retains only normal-screen, full-scroll-region rows; the alternate grid has zero history. `set_size` resizes current rows but leaves old history rows at their recorded width. Cells use fixed 32-byte storage and contain at most 22 UTF-8 bytes. Public callbacks do not expose scrolling and there is no public runtime history-byte-budget setter. Do not reconstruct the live parser to trim history: that can lose an incomplete escape sequence or alternate-screen state.

## Proposed behavior and resource limits

1. **Entry:** Browse-mode PageUp opens history for the selected session and starts one viewport above the captured tail, clamped to row zero. Terminal-mode PageUp keeps its existing PTY meaning. Opening without selection does nothing. An alternate-screen application returns `Conflict` with `History is unavailable on the alternate screen`; copy mode may independently capture its current screen.
2. **Navigation:** History uses Up/Down or k/j for one row, PageUp/PageDown for one viewport, Home/End for the oldest/captured-newest row, and h/l or Left/Right for one column. Escape, q or Ctrl-g leaves history and returns to Browse. End remains in the frozen snapshot; reopening obtains newer output. Enter and paste do not forward input while reading history.
3. **Live activity:** Output continues through the original live parser and dirty recovery path. Show `HISTORY · frozen · new output` after selected-session output or a dirty notification arrives. No automatic jump to the tail. A child entering alternate screen after capture does not invalidate that already frozen snapshot.
4. **Resize:** Resize the live PTY/parser normally. Retain the frozen snapshot's original cells, widths, wrapped flags and row/column anchors; clip the history viewport and request newly visible tiles. Do not resize or reflow the frozen clone. Current live-screen shrink behavior remains vt100's behavior; rows discarded by resize are not promised as history.
5. **Retention and eviction:** Keep the newest 512 physical rows according to vt100's scroll semantics. A long wrapped logical line may span many retained rows and may be partially evicted. Once captured, a snapshot preserves its own rows until it is released. Reopening uses the currently retained rows; never remap an old row number to a different snapshot.
6. **Lifetime:** One snapshot per dashboard connection, even if a later split-pane feature exists. Beginning another replaces it. Leaving history, changing session, removing that session, or disconnecting releases it. Reconnect starts at the live screen; a fresh history request can still retrieve surviving server history, but old snapshot IDs are invalid.
7. **Memory:** The live parser retains at most 512 historical row objects per session, plus its existing screens. One connection may hold one additional cloned screen/history set. This is a **row bound, not a fixed byte or aggregate process-memory bound**: memory scales with row width and retained session records. At 80 columns history cell payload is about 1.25 MiB/session; at 512 columns it is about 8 MiB/session, or 400 MiB for 50 sessions, excluding allocation overhead and current screens. Exited records retain history until removed. Existing arbitrary geometry/session counts prevent an honest universal byte bound without broader policy or dependency changes.
8. **Transport/client memory:** A page contains at most 16 rows × 128 cells, each preserving vt100's bounded cell text. Require encoded responses at most 128 KiB and below the existing frame limit. The client retains at most sixteen pages, one request in flight, and one desired viewport; repeated keys coalesce to the latest viewport. History display is capped at 64 rows × 256 columns inside the existing pane; wider/taller panes show the bounded history area with a limit hint. Horizontal navigation reveals wider captured rows. No full-history copy in the client.

These are deliberate minimal defaults. Measuring worst-case history overhead at supported test geometries is an acceptance gate; an absolute byte-budget policy is a separate design decision, not a claimed property of this implementation.

## Shared interfaces

Place shared history wire data in `crates/ovrcr-protocol/src/wire.rs`; do not create a root `src/history.rs`. All shared wire types below derive `Clone, Debug, PartialEq, Eq, Serialize, Deserialize`; IDs also derive `Copy`. These types are the optional integration contract for [`2026-09-05-copy-mode.md`](2026-09-05-copy-mode.md).

The future implementation boundary is explicit: `HistorySnapshotId`, `HistoryColor`, `HistoryCell`, `HistoryRow`, `HistoryOpened`, `HistoryRows`, and the page limits belong to the protocol crate. `FrozenHistory` and its VT100 capture/page implementation belong in a new `crates/ovrcr-terminal/src/history.rs` module; the terminal crate may add its direct protocol dependency when this feature is implemented. `Session::capture_history` remains in `crates/ovrcr-runtime/src/session/mod.rs`. `HistoryRequest` and `DispatchMessage::History`, including owner and completion channels, belong in `crates/ovrcr-runtime/src/server/dispatch.rs`. The client `HistoryView`, pending requests, and cache belong in `crates/ovrcr-tui/src/dashboard/state.rs`; history key routing and rendering belong in `dashboard/input.rs` and `dashboard/render.rs`.

```rust
pub const HISTORY_ROWS: usize = 512;
pub const PAGE_ROWS: u16 = 16;
pub const PAGE_COLS: u16 = 128;
pub const PAGE_BYTES: usize = 128 * 1024;
pub struct HistorySnapshotId(pub u64);
pub enum HistoryColor { Default, Indexed(u8), Rgb(u8, u8, u8) }
pub struct HistoryCell {
    pub text: String,
    pub width: u8, // 0 = wide continuation, 1 = ordinary/blank, 2 = wide start
    pub fg: HistoryColor,
    pub bg: HistoryColor,
    pub attributes: u8, // bits 0..4 = bold, dim, italic, underline, inverse
}
pub struct HistoryRow {
    pub width: u16, // complete physical row width, not this tile's width
    pub cells: Vec<HistoryCell>,
    pub wrapped: bool, // joins to the following physical row
}
pub struct HistoryOpened {
    pub session: SessionId,
    pub snapshot: HistorySnapshotId,
    pub revision: u64,
    pub size: TerminalSize,
    pub history_rows: u32,
    pub total_rows: u32, // history plus captured current normal screen
}
pub struct HistoryRows {
    pub session: SessionId,
    pub snapshot: HistorySnapshotId,
    pub start_row: u32,
    pub start_col: u16,
    pub rows: Vec<HistoryRow>,
}
```

Add `Request::HistoryBegin { session }`, `Request::HistoryPage { session, snapshot, start_row, rows, start_col, cols }`, and `Request::HistoryEnd { session, snapshot }`, using the types above (`start_row: u32`; row/column counts and `start_col: u16`). Responses are `Response::HistoryOpened(HistoryOpened)`, `Response::HistoryRows(HistoryRows)` and existing `Response::Ok`/`Error`.

Server-local `HistoryRequest` has `Begin { session }`, `Page { session, snapshot, start_row, rows, start_col, cols }`, and `End { session, snapshot }` with exactly the same field types. Add `DispatchMessage::History { owner: Arc<()>, request_id: u64, request: HistoryRequest, completion: SyncSender<()> }`; the owner must still match the registered dashboard before capture, page service or release. Requests depend on explicit session identity, never `state.selected`.

`FrozenHistory` owns `opened: HistoryOpened` and a private `screen: vt100::Screen`. Define:

```rust
impl FrozenHistory {
    pub fn capture(session: SessionId, snapshot: HistorySnapshotId,
        revision: u64, screen: vt100::Screen) -> anyhow::Result<Self>;
    pub fn opened(&self) -> &HistoryOpened;
    pub fn page(&mut self, start_row: u32, rows: u16,
        start_col: u16, cols: u16) -> anyhow::Result<HistoryRows>;
}
// Capture locks terminal state once; clone before releasing that lock.
impl Session {
    pub fn capture_history(&self, id: HistorySnapshotId)
        -> anyhow::Result<FrozenHistory>;
}
```

Changing only the clone's scroll offset during paging is allowed; its cells, dimensions and metadata stay immutable. Snapshot IDs increase with checked arithmetic; overflow returns `Internal`, never reuses an active ID. Missing session/snapshot returns `NotFound`, alternate-screen capture `Conflict`, invalid dimensions/ranges `InvalidRequest`. `HistoryEnd` with an already released token is idempotent `Ok` and must never release a newer token. New history features must not bypass normal dashboard-role checks.

## Current integration paths and task checkpoints

### Workspace handoff

- Direct imports cross package boundaries through the public `ovrcr_protocol` re-exports backed by `crates/ovrcr-protocol/src/wire.rs` for shared history data, `ovrcr_runtime::session`/`server` for ownership and dispatch, and the existing `ovrcr_tui` facade for client state and rendering. The application facade in root `src/lib.rs` and CLI in `src/cli/` remain integration consumers; the protocol crate's `wire` module is currently private.
- Focused package checks are `rtk proxy cargo test -p ovrcr-protocol --lib`, `rtk proxy cargo test -p ovrcr-terminal --lib`, `rtk proxy cargo test -p ovrcr-runtime --lib history_`, and `rtk proxy cargo test -p ovrcr-tui --lib history_`. Use the package test command that owns the changed unit module.
- Root integration commands are `rtk proxy cargo test -p ovrcr --test server_lifecycle history_`, `rtk proxy cargo test -p ovrcr --test tui history_`, and `rtk proxy cargo test -p ovrcr --test terminal_acceptance history_`; all final executable behavior is exercised through the root `ovrcr` binary.

Task 1 replaces the parser/revision mutex arrangement, so migrate every existing caller together: `apply_event`, `resize`, `current_screen`, `terminal_text`, `send_text`, and `wait_for_output`. `terminal_text` remains the resource CLI's **live current-screen** read; it must not read the frozen clone or change its existing max-lines behavior. `send_text` samples bracketed-paste mode under the terminal-state lock, releases it, then follows pause admission and the existing writer lock. The condition-variable wait must use the same mutex that protects the revision it observes.

Use these explicit client request records in Task 3 instead of an unqualified request-ID field:

```rust
struct PendingHistoryBegin { request_id: u64, session: SessionId, cancelled: bool }
struct PendingHistoryPage {
    request_id: u64, session: SessionId, snapshot: HistorySnapshotId,
    start_row: u32, rows: u16, start_col: u16, cols: u16,
}
```

`HistoryView.pending` holds `Option<PendingHistoryPage>` and the dashboard's begin state holds `Option<PendingHistoryBegin>`. Validate response ID, session, snapshot, starts, returned row count, cell count per row, and bounds before caching. A short last page is allowed; unsolicited or oversized data never changes the viewport. Cache keys include snapshot and both tile starts, not just row. Keep one request outstanding across navigation and later historical copying.

Cancellation must handle an accepted Begin whose response arrives late. Keep a small pending-begin tombstone until that response/error arrives; if it opens a snapshot after exit/cancel, send `HistoryEnd` for that exact token without entering History. Do not allow another Begin until the pending Begin has resolved. End for an older token cannot release a newer snapshot. A transport disconnect releases the server owner and all pending client state. Add `history_cancelled_begin_releases_late_snapshot` to the TUI tests with Begin → Escape → delayed Opened → End and no mode change; assert a later Begin works.

Connect cancellation to every actual selection/removal/connection reset path. A resize keeps the frozen snapshot and changes only desired page coordinates; late pages for the old viewport may populate the bounded cache only when they still match the outstanding request. They do not restore the old desired viewport. History keeps consuming the live parser's Output and same-session dirty snapshots.

The split-pane plan runs later and must preserve one snapshot per dashboard, tagged with its captured session. Before finishing Task 1 and final acceptance, run `rtk proxy cargo test -p ovrcr --test resource_cli terminal_cli_drives_real_session_and_preserves_workspace_removal_guards -- --exact --nocapture` (one executed test) to cover the migrated text/paste paths. A separate history test cannot stand in for that regression.

## Task 1: Retain rows and expose frozen, Unicode-safe tiles

**Files:** Create `crates/ovrcr-terminal/src/history.rs` for `FrozenHistory`; modify `crates/ovrcr-protocol/src/wire.rs` for shared history records, `crates/ovrcr-runtime/src/session/mod.rs` for capture, and the owning private unit-test modules. The root `src/lib.rs` facade remains unchanged unless a later public re-export is required by an accepted interface.

**Consumes:** Existing `vt100`, `SessionId`, `TerminalSize`, `apply_event`, `current_screen`.
**Produces:** All shared types and `FrozenHistory`/`Session::capture_history` methods above.

- [ ] Write `history_retention_is_frozen_and_evicts_oldest` as the first failing test:

```rust
#[test]
fn history_retention_is_frozen_and_evicts_oldest() {
    let mut parser = vt100::Parser::new(2, 12, HISTORY_ROWS);
    parser.process(b"ONE\r\nTWO\r\nTHREE");
    let mut frozen = FrozenHistory::capture(SessionId(1),
        HistorySnapshotId(1), 7, parser.screen().clone()).unwrap();
    assert_eq!(frozen.opened().history_rows, 1);
    let before = frozen.page(0, 1, 0, 12).unwrap();
    assert_eq!(before.rows[0].cells.iter().map(|c| c.text.as_str())
        .collect::<String>().trim_end(), "ONE");
    for n in 0..600 { parser.process(format!("\r\nNEW_{n}").as_bytes()); }
    assert_eq!(frozen.page(0, 1, 0, 12).unwrap(), before);
    parser.screen_mut().set_scrollback(usize::MAX);
    assert_eq!(parser.screen().scrollback(), HISTORY_ROWS);
}
```

- [ ] Run `rtk proxy cargo test -p ovrcr-terminal --lib history::tests::history_retention_is_frozen_and_evicts_oldest -- --exact --nocapture`. Expected before implementation: compile failure for missing types, then one failing behavioral test; after implementation: exactly **1 passed**. Use the same command for RED/GREEN, not a zero-match filter.
- [ ] Implement capture and paging without mutating the live parser. Capture rejects alternate screen, sets the clone's scrollback to `usize::MAX` to obtain `history_rows`, and restores zero. For absolute row `r < history_rows`, set clone offset `history_rows-r` and read visible row zero; otherwise set offset zero and read row `r-history_rows`. Validate requested start/count with checked arithmetic; clamp only the returned end to `total_rows`.
- [ ] Convert `screen.cell(row,col)` to the specified text, width, colors and five flags. Find physical width with binary search for the first missing cell over `0..=u16::MAX`; public `cell` addresses retained rows at their original width. Read only the requested column tile, leaving `cells` empty when `start_col >= width`. Preserve `wrapped`, including across tile boundaries. Example conversion core:

```rust
let width = if cell.is_wide_continuation() { 0 }
    else if cell.is_wide() { 2 } else { 1 };
let attributes = u8::from(cell.bold()) | (u8::from(cell.dim()) << 1)
    | (u8::from(cell.italic()) << 2) | (u8::from(cell.underline()) << 3)
    | (u8::from(cell.inverse()) << 4);
```

- [ ] Use `TerminalState { parser: vt100::Parser, revision: u64 }` under one `Mutex` in `Session`; keep `parser_changed` and use this mutex with its wait. Instantiate the parser with `HISTORY_ROWS`, increment revision on each parsed event and parser resize, and preserve `current_screen()` at offset zero. Capture clone and revision under that same lock. Do not change reader chunking or event queues.
- [ ] Add five more named unit cases: `history_tiles_preserve_wide_combining_and_wrap`, `history_resize_keeps_old_width`, `history_alternate_screen_has_no_transcript`, `history_partial_escape_survives_capture`, `history_bounds_reject_bad_ranges`. Use `"界e\u{301}X"` at width 4 to assert wide continuation, combined `é`, and soft wrapping after another printable byte; use split UTF-8/CSI calls with capture between them and compare the live parser to an uninterrupted parser. Resize 12→4→12 and assert old 12-cell history still pages fully. Enter/exit `\x1b[?1049h`/`l`, asserting capture refusal inside and unchanged primary history afterward.
- [ ] Run `rtk proxy cargo test -p ovrcr-terminal --lib history_ -- --nocapture`; expected **at least 6 tests**, all pass. Re-run `rtk proxy cargo test -p ovrcr-runtime --lib session::tests -- --nocapture`; expected **at least 4 tests**, retaining real PTY resize/final-output checks.

## Task 2: Deliver session-scoped bounded history responses

**Files:** Modify `crates/ovrcr-protocol/src/wire.rs` and `crates/ovrcr-runtime/src/server/{dispatch,connections,outbound,mod}.rs`; unit tests stay in the owning protocol/runtime modules.
**Consumes:** Task 1's frozen types/capture. **Produces:** The exact request/response/dispatcher contract above; one owner-bound snapshot in `DashboardSlot`.

- [ ] Add `history_page_round_trip_and_size_bound`, building the maximum 16×128 tile, every text field 22 bytes with RGB/style/wide flags, then round-trip it through `write_frame`/`read_frame` and assert the encoded frame payload is `<= PAGE_BYTES`. Include explicit request validation cases for zero/oversize tile counts, invalid row start, and overflowing range arithmetic. Do not allocate based on a caller's requested counts before validating them.
- [ ] Run `rtk proxy cargo test -p ovrcr-protocol --lib history_page_round_trip_and_size_bound -- --nocapture`; RED is missing protocol variants, GREEN must execute **1 test**.
- [ ] Implement dashboard request routing by extending the existing special `Select` response path: history responses are queued exactly once by the dispatcher, while the connection thread waits only for completion. The dispatcher never waits for a socket write. Code shape:

```rust
DispatchMessage::History { owner, request_id, request, completion } => {
    dispatch_history(&state, &owner, request_id, request);
    let _ = completion.send(());
}
// dispatch_history(state: &ServerState, owner: &Arc<()>, request_id: u64,
//                  request: HistoryRequest) -> ()
```

- [ ] Store `history: Option<FrozenHistory>` and `next_history_id: u64` in the dashboard-owned state. Check `Arc::ptr_eq` against the current owner before work and response delivery. Release old snapshot before cloning its replacement. Capture a referenced session outside the sessions-map lock, using the established lock order; no sink/slot lock while waiting on a session/parser lock. Release history on removal/disconnect, without holding a parser lock during socket operations.
- [ ] Add `history_owner_and_token_isolation`, `history_page_overflow_disconnects_without_parser_wait`, and `history_capture_orders_with_output`. Exercise owner mismatch after reconnect, old End versus new token, a saturated response queue, and dispatcher commands `Output(A) → Begin → Output(B) → Page`. The page includes A and excludes B while the live screen includes both. Use sync channels to establish order, not sleeps.
- [ ] Run the owning package gates separately: `rtk proxy cargo test -p ovrcr-terminal --lib history_ -- --nocapture` (at least **6 tests**), `rtk proxy cargo test -p ovrcr-protocol --lib history_page_round_trip_and_size_bound -- --nocapture` (exactly **1 test**), and `rtk proxy cargo test -p ovrcr-runtime --lib history_ -- --nocapture` (exactly **3 tests** from Task 2). Re-run `rtk proxy cargo test -p ovrcr-runtime --lib session::tests -- --nocapture` (at least **4 existing session tests** from Task 1), then run `rtk proxy cargo test -p ovrcr --test server_lifecycle selection_snapshot_precedes_later_quiet_tail_output -- --nocapture` (exactly **1 root integration test**). Frame-size and existing current-screen protocol limits remain unchanged.

## Task 3: Add a bounded history reading state to the dashboard

**Files:** Modify `crates/ovrcr-tui/src/dashboard/{mod,state,input,render}.rs` and root `tests/tui.rs`.
**Consumes:** Task 2 messages; existing live parser and request IDs. **Produces:** `InputMode::History`, `Dashboard.history: Option<HistoryView>`, `HistoryView::accept_page`, navigation and rendering.

Define `HistoryView { opened: HistoryOpened, top: u32, left: u16, new_output: bool, pages: VecDeque<HistoryRows>, pending: Option<PendingHistoryPage> }`; keep at most sixteen pages. Dashboard also tracks `history_begin_request: Option<PendingHistoryBegin>`. `HistoryView::accept_page(&mut self, request_id: u64, page: HistoryRows) -> bool` accepts only the outstanding request and matching session/snapshot; false means discard. `history_view_size(pane: TerminalSize) -> TerminalSize` returns positive dimensions clamped to 64×256. Define `render_history(frame: &mut Frame<'_>, area: Rect, view: &HistoryView)` for direct cell rendering.

- [ ] Add `history_navigation_never_writes_to_pty`, using `dashboard_fixture()` and a synthetic `HistoryOpened`. Assert Browse PageUp requests Begin; Terminal PageUp remains PTY bytes; then assert all history navigation, Enter and paste yield no `PtyBytes` action. Test q returns Browse, and End remains History. RED/GREEN command: `rtk proxy cargo test -p ovrcr --test tui history_navigation_never_writes_to_pty -- --nocapture`; expected **1 test**.
- [ ] Implement navigation with saturating row arithmetic and `u32` intermediate column arithmetic. Clamp top to `total_rows.saturating_sub(view_rows)`; clamp left to the terminal-coordinate ceiling. An empty tile must render blanks, not stale cells. Request only missing visible tiles aligned to 16 rows/128 columns; maintain one outstanding page and update the desired viewport on subsequent keys. Example navigation core:

```rust
let max_top = view.opened.total_rows.saturating_sub(u32::from(height));
view.top = view.top.saturating_add_signed(delta).min(max_top);
// delta: i32; height: u16 from history_view_size(self.pane_size)
```

- [ ] Continue the existing `Output` parser path while history is visible. Dirty notifications continue requesting the ordinary live snapshot and set `new_output`; live `Screen` responses never replace frozen history or its anchors. Match history responses by request ID as well as snapshot/session. If Begin completes after cancellation, send matching End to release it. Session switch releases history and clears pending state; stale responses cannot reopen it.
- [ ] Render cell tiles with the existing terminal colors/styles; hide the PTY cursor. Draw history position and frozen/new-output/viewport-limit hints in existing footer/metadata areas. Wide continuation cells are never rendered as independent glyphs; blank a clipped half-wide glyph at either viewport edge. Combining strings remain attached to the leading cell. Missing pages display a loading indicator. History draws at most 64×256 cells; no repainting or parsing the entire history on each input.
- [ ] Add `history_live_output_preserves_anchor`, `history_resize_preserves_capture`, `history_stale_response_is_ignored`, `history_pending_keys_coalesce`, and `history_render_preserves_cells_and_clips`. Test live parser receives a marker while a prior frozen page remains byte-for-byte equal; inject an old snapshot after session switch; flood 100 PageUp events and assert only one request remains outstanding and cache length never exceeds sixteen. Use Ratatui `TestBackend` for colored `界é`, a continuation at the left edge, width clipping and hidden cursor.
- [ ] Run `rtk proxy cargo test -p ovrcr --test tui history_ -- --nocapture`; expected **at least 6 tests**. Run `rtk proxy cargo test -p ovrcr --test tui -- --nocapture`; require a nonzero full suite, preserving sidebar browse navigation, paste, cursor and resize expectations.

## Task 4: Prove detach, live output, eviction and resource behavior with real PTYs

**Files:** Modify `tests/server_lifecycle.rs`, `tests/terminal_acceptance.rs`; adjust implementation files above only for failures demonstrated by these checks.
**Consumes:** Tasks 1–3. **Produces:** Real socket/PTY acceptance evidence and explicit memory measurements.

- [ ] Add three lifecycle cases using existing `ControlFixture`: `history_reattach_reads_retained_output`, `history_frozen_page_survives_eviction_and_exit`, `history_slow_dashboard_recovers_after_finite_burst`. Helpers stay in this test file: `history_request(stream: &mut UnixStream, id: u64, request: Request) -> Response` writes once, reads until the matching response under a two-second socket timeout, and forwards interleaved events to a test dashboard parser; `history_ready_shell(fixture: &ControlFixture) -> SessionId` creates a shell emitting an explicit first marker, waiting for input, then emitting numbered lines and a final marker.
- [ ] Use this shell body for the frozen-eviction case; enter history after observing `HISTORY_READY` on the parsed screen, then send the gate input:

```sh
printf 'OLD_VISIBLE\nHISTORY_READY\n'
read gate
i=0
while [ "$i" -lt 700 ]; do printf 'NEW_%04d\n' "$i"; i=$((i+1)); done
printf 'FINAL_HISTORY_MARKER\n'
```

Assert old frozen cells remain unchanged, a newly captured snapshot has evicted `OLD_VISIBLE`, and its current/history rows contain `FINAL_HISTORY_MARKER` only after the real session becomes Exited. Detach and reconnect before capture in the reattach case, proving historical output was retained by the server while no dashboard existed. Read all needed tiles with one request outstanding and assert every encoded response stays within 128 KiB. In the slow-reader case stop output at the final marker, drain existing dirty recovery, and obtain history successfully without needing another PTY byte.
- [ ] Run `rtk proxy cargo test -p ovrcr --test server_lifecycle history_ -- --nocapture`; expected **3 tests**. RED must be an observed missing historical marker, ordering failure or missing interface; do not use a passing zero-match filter. Cleanup uses existing fixture shutdown and process-group assertions even on failure.
- [ ] Add `history_keyboard_reads_old_output_during_live_session` to the existing outer-PTY harness in `tests/terminal_acceptance.rs`. Print 100 numbered lines, wait for the final rendered marker, use Ctrl-g then PageUp, and verify an older marker and HISTORY footer through the outer parser. Have the child wait on a fixture-owned FIFO after its initial output; write a token to that FIFO to trigger more output while history is frozen. Do not send `Request::Input` from a control connection, which the existing server rejects. Verify the old marker stays, resize the outer PTY, and leave history to verify the new live marker. Use harness polling deadlines/observed screen content, not fixed sleeps as evidence.
- [ ] Run `rtk proxy cargo test -p ovrcr --test terminal_acceptance history_keyboard_reads_old_output_during_live_session -- --nocapture`; expected **1 test**. Run existing `rtk proxy cargo test -p ovrcr --test server_lifecycle fifty_sessions_survive_detach_and_leave_no_process_groups -- --nocapture` and `rtk proxy cargo test -p ovrcr --test server_lifecycle slow_dashboard_recovers_after_output_burst -- --nocapture`; each must execute **1 test**.
- [ ] During a disposable 50-session acceptance run, fill each session past 512 lines at 80 columns, then repeat at 512 columns; capture exactly one history snapshot. Record retained-row counts and process peak/resident memory before/after fill and capture (`rtk proxy /usr/bin/time -l` on macOS or `rtk proxy /usr/bin/time -v` on Linux around the bounded fixture run). Use fixed finite output and lifecycle completion barriers. Report measured overhead and geometry with no claimed universal byte ceiling. Fail retention acceptance if repeated finite bursts grow retained row counts beyond 512 or create more than one snapshot; investigate memory growth beyond the explained row/grid allocations before sign-off.
- [ ] Finish with `rtk proxy cargo fmt --all -- --check`, `rtk proxy cargo test --workspace --all-targets --all-features`, and `rtk proxy cargo test -p ovrcr --features gui --test gui`. Require nonzero test totals and record actual counts. The optional GUI helper uses the real TUI, so manually verify its history keys, clipped wide cells and resize if its desktop surface is available; otherwise explicitly retain the GUI acceptance gap.

## Acceptance criteria and dependencies

- Older normal-screen output is readable for background, selected, exited and reattached sessions within the retained 512 physical rows.
- History does not drop/reorder raw PTY bytes or delay lifecycle completion behind socket writes; existing finite-burst dirty recovery still succeeds.
- Frozen row/column anchors remain stable through output, eviction and live resize. Evicted prefix fragments and non-reflow behavior are documented accurately.
- Alternate-screen history entry refuses clearly; a preexisting normal-screen snapshot remains readable when the live application changes buffers.
- UTF-8 split across reads, wide continuations, combining text, colors and soft-wrap flags survive capture/page/rendering without ASCII-based slicing.
- One server snapshot per connection, sixteen bounded client tiles, one outstanding page and <=128-KiB history payloads are verified. Row bounds and geometry-dependent memory costs are stated separately.
- The plan works without copy mode, split panes, hooks or persistence. Copy mode may use `(session, snapshot, row, column)` and the same paged cell contract; selection must join only `wrapped` rows and fetch missing column tiles. Its current-screen-only implementation remains independent.
- Split panes may supply session identity through focused-pane state. History does not change focus/subscription ownership or assume a `SetView` protocol exists; later integration must preserve this explicit session/owner identity and one-snapshot policy.
- No benchmark or GUI success is claimed by this plan. The largest risks are clone latency at unusually large geometry, width-dependent retention cost, existing vt100 Unicode/resize limitations, and owner/request races during cancellation/reconnect. The finite resource and lifecycle checks above are the implementation gates.

## Author self-review

- Coverage checked against the roadmap request and MVP terminal-data-flow constraints: retention, reading during output, alternate buffers, resize, reconnect, bounded delivery, Unicode/wrap metadata, eviction and lifecycle are assigned to Tasks 1–4.
- Proposed interfaces are defined above and session-scoped; there are no assumptions that sibling plans have shipped.
- Retention bounds are described as rows, not fabricated byte guarantees. No dependency extension, parser reset, geometry restriction or disk history is proposed.
- This file is an implementation plan only; no source changes, tests, commits, roadmap checkbox edits or feature completion are recorded by authoring it.
