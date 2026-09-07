# Split Panes Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Status:** Proposed future design, not an approved specification or implemented behavior. All choices below marked as defaults are design assumptions for review.

**Goal:** View two different live sessions side by side, with keyboard input reaching only the focused pane.

**Architecture:** Keep one synchronous server, one dashboard connection, the existing ordered PTY dispatcher, and the existing bounded dashboard writer. Replace the dashboard's single visible-session subscription with a revision-tagged view of at most two sessions; keep one local parser per pane. Use the existing Ratatui terminal renderer twice, with one shared geometry calculation for drawing, PTY sizes, and hit testing.

**Tech stack:** Rust edition 2024; existing Ratatui 0.30, Crossterm 0.29, portable-pty 0.9, vt100 0.16, serde/bincode, and standard-library blocking I/O, threads, channels, and locks. No new dependency or Cargo feature.

**Spec references:** `README.md`, feature-roadmap checkbox “Split panes to view multiple sessions side by side”; `plans/2026-09-04-ovrcr-mvp-design.md`, especially Terminal data flow, Dashboard, Local protocol, and Failure handling. The approved MVP explicitly defers panes; this proposal extends its single-pane rules rather than claiming approval from that document.

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → multiple dashboards → session restore**. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --workspace --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Global constraints

- “The MVP targets macOS and Linux, supports up to 50 live sessions, and assumes one connected dashboard.” Two panes do not change the session or dashboard limits.
- “OVRCR does not use Tokio, a database, a shell-command builder, or an agent SDK.”
- “The server stores live sessions in memory.” Pane layout also stays in dashboard memory.
- “Socket writes never block the parser dispatcher.” Raw PTY output remains bounded and lossless before parsing.
- “OVRCR owns every session it manages.” Closing a pane never kills, removes, adopts, or recreates a session.
- Preserve final-output-before-exit ordering, process-group cleanup, worktree removal gates, and outer-terminal restoration on exit and panic.
- Prefix every execution command with `rtk`; use `rtk proxy` for unfiltered test counts. Do not claim execution evidence from the sketches below.

## Current code grounding

Read against Git HEAD `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb` on 2026-09-05. Refresh these references before implementation if HEAD changes.

| Existing location | Relevant behavior |
| --- | --- |
| `crates/ovrcr-tui/src/dashboard/{state,mod}.rs` | `Dashboard` holds one `selected`, `parser`, and `pane_size`. |
| `crates/ovrcr-tui/src/dashboard/{input,event_loop}.rs` | Keyboard/paste encoding reads that parser's modes; incoming output is accepted only for the selected session. |
| `crates/ovrcr-tui/src/dashboard/event_loop.rs` | Event loop limits input to 32 events, drains at most 64 server messages per batch, coalesces frames at 16 ms, and drains messages before drawing. |
| `crates/ovrcr-tui/src/dashboard/render.rs` | Dense sidebar and two metadata rows precede one terminal rectangle. At 120×40 the terminal is `(40, 3, 80, 36)`. |
| `crates/ovrcr-protocol/src/wire.rs` | `Select` returns a `Screen`; `Input`, `Resize`, `Output`, and `ScreenDirty` carry a session ID. |
| `crates/ovrcr-runtime/src/server/{dispatch,mod}.rs` | Server stores one selected session; selection joins the same dispatcher as PTY output so its snapshot precedes subsequent increments. |
| `crates/ovrcr-runtime/src/server/outbound.rs` | `DashboardSink` tracks dirty state per session, but `replace_selection` discards output for the old and new selection. |
| `crates/ovrcr-runtime/src/session/mod.rs` | `Session::resize` changes the real PTY and parser; `current_screen` uses `state_formatted()` to reconstruct terminal state. |
| root `tests/{tui,server_lifecycle,terminal_acceptance}.rs` | Existing TestBackend checks, socket/PTY fixtures, slow-dashboard recovery, 50-session cleanup, and real outer-PTY acceptance can be extended. |

## Proposed defaults and boundaries

1. Support one or two panes only, always side by side. The sidebar stays unchanged. A one-column `│` separator divides the right area; split the remaining width with integer division, giving the right pane the extra column. Each pane keeps the current two metadata rows. No recursive split tree, drag handles, horizontal splits, ratio configuration, or third pane.
2. In browse mode, `v` opens the second pane with the next different session in existing visible tree order and focuses it. If only one visible session exists, show `No other visible session to split`. Repeated `v` with two panes is a no-op. Tab/Shift-Tab switch focus; `x` closes the focused pane when there are two and expands the survivor. With one pane, `x` is a no-op. The footer names these controls and the current mode.
3. Browsing the sidebar replaces the focused pane's session. Selecting a session already in the other pane focuses that pane instead, so the same PTY never receives conflicting pane sizes. The sidebar's three-line highlight follows the focused session. Browse-mode left-click in either terminal rectangle focuses that pane; it sends no PTY bytes.
4. Enter enters terminal mode for a ready, live focused pane. Ctrl-g returns to browse mode. Tab, `v`, `x`, paste, arrows, and ordinary keys retain their normal application meanings in terminal mode. There is no broadcast input. For a pane awaiting its new snapshot, refuse input with `Pane is loading; retry input`; do not silently send through a stale parser. Exited panes retain their final screen and refuse terminal input.
5. Each split terminal needs at least 20 columns and one drawable row. If the right area cannot fit both, retain both local slots but draw and subscribe only the focused pane at full right width. Display `split hidden: terminal too small`; restore both automatically when space returns. A zero-cell outer layout remains drawable: do not submit zero sizes, hide the cursor, and retain the PTYs' last sizes.
6. Closing, replacing, or temporarily hiding a pane unsubscribes its old session; its server parser continues consuming output and its PTY retains its last dimensions. The focused visible pane's size remains the default for newly created sessions while connected. With no visible terminal, preserve the existing connected-empty geometry policy; detached creation keeps the existing 120×40 default.
7. Pane state consists of its session, current-screen parser, last accepted size, readiness, and error. No historical scrollback, selection, clipboard state, or per-pane process ownership is introduced. Removed session records clear their pane; collapse to the remaining pane, or select the first remaining tree session if none survives. A retained exited record remains viewable.
8. `q` still detaches and ends the dashboard. Socket loss still restores the terminal and reports connection loss. Running `ovrcr` again starts in browse mode with one pane and the existing first-session selection; users can reopen a split. No automatic reconnect loop or persisted layout. Every new attachment rebuilds screens from the surviving server, including alternate-screen and input modes.

## Files and interfaces

Modify only these implementation files when execution is authorized: `crates/ovrcr-protocol/src/wire.rs`, `crates/ovrcr-runtime/src/server/{dispatch,connections,outbound,mod}.rs`, `crates/ovrcr-tui/src/dashboard/{state,input,render,event_loop,mod}.rs`, and root `tests/{tui,server_lifecycle,terminal_acceptance}.rs`. Add unit tests inside the existing protocol/runtime test modules. `crates/ovrcr-runtime/src/session/mod.rs` is an inspected dependency, not a planned lifecycle rewrite. No changes to workspace manifests, registry storage, CLI commands, GUI helper, or Git/worktree code are needed. Existing protocol constructors and exhaustive matches in these files must be migrated together. Document shipped controls in `README.md` only during a separately authorized implementation closeout; leave the roadmap unchecked while this is a proposal.

### Wire and server contract

Add these types to `crates/ovrcr-protocol/src/wire.rs` with the existing serde/clone/debug/equality derives:

```rust
pub struct PaneTarget {
    pub session: SessionId,
    pub size: TerminalSize,
}
pub struct DashboardView {
    pub revision: u64,
    pub panes: Vec<PaneTarget>, // zero through two; empty means no visible PTY
    pub focused: Option<SessionId>,
}
// Add to Request:
// SetView { view: DashboardView }
// Add `revision: u64` to Response::Screen and to
// ServerEvent::{Output, ScreenDirty}; retain their other fields.
```

`DashboardView::validate(&self) -> Result<(), String>` rejects zero revision, more than two panes, duplicate session IDs, zero rows/columns, and focus outside the pane list; empty requires no focus and nonempty requires focus. Session existence and a strictly increasing revision are checked at the server. Preserve the existing frame-length maximum; serialize each pane snapshot separately, never aggregate two potentially large screens into one frame.

Replace `ServerState::selected` with `view: Mutex<Option<DashboardView>>`. Extend the existing `DispatchMessage::Select` pattern with `SetView { owner: Arc<()>, request_id: u64, view: DashboardView, completion: SyncSender<()> }`. Carry the current dashboard's existing `Arc<()>` identity from `handle_connection` into dispatched view requests, verify it still owns the slot at processing time, and scope view clearing to that owner. This is still one connection, not a client registry.

`dispatch_set_view` validates all session references before changing any PTY. It resizes only changed geometries, captures each current screen, and asks `DashboardSink::replace_view(&self, view: &DashboardView, request_id: u64, screens: Vec<Vec<u8>>) -> bool` to atomically discard obsolete increments/dirty flags and queue one `Response::Screen` per visible pane, followed by one `Response::Ok`, all carrying that request ID. Only then publish the view and resume dispatch. Input sent afterward is checked against the committed focused session. Empty views queue only `Ok`. Route view errors through the sole dashboard writer exactly once, including dispatcher-send failure; update the special deferred-response branch currently used for `Select` so an error is never suppressed.

The queue remains 64 entries. Preflight room for all snapshots plus the acknowledgement and each frame's serialized size; if they cannot fit, disconnect that dashboard rather than drop lifecycle/response frames. A real PTY resize is not transactional: if the second resize fails, report `PartialFailure`, clear that owner's subscription and disconnect it, preserving processes and any completed first resize. Do not claim rollback. A later attachment obtains actual current screens and applies its geometry again.

Keep `Select` and `Resize` as existing same-version API adapters while migrating tests: `Select` constructs a singleton view with the next revision; `Resize` is accepted only for a singleton view and changes its geometry through the dispatcher. Reject `Resize` for a two-pane view with `InvalidRequest: use SetView for split geometry`. Both now return exactly one Screen followed by one Ok for the same request ID through the snapshot-before-increment path; migrate all singleton test callers to drain both before issuing another request. New dashboard code uses only `SetView`. No cross-version compatibility promise is added.

Output dispatch parses every PTY before testing membership in the committed view. Emit `(revision, session, bytes)` only for visible members. Dirty keys become `(revision, SessionId)`, including the writer's Pending/Sending/Sent callback; an old dirty write completion must never mark a new view dirty. On a dirty event, the client requests its whole desired view at a new revision. Once that happens, other old-revision dirty events are ignored. This intentionally refreshes both screens to avoid a separate partial-resubscription protocol.

### Dashboard contract

In `crates/ovrcr-tui/src/dashboard/{state,mod}.rs`, replace the three single-pane fields with these concrete fields and helpers. Keep hierarchy, sidebar collapse/scroll, and global mode on `Dashboard`:

```rust
pub struct PaneState {
    pub session: Option<SessionId>,
    pub parser: vt100::Parser,
    pub size: TerminalSize, // actual size of installed snapshot
    pub desired_size: TerminalSize, // latest measured target
    pub snapshot_installed: bool, // for the pending view only
    pub ready: bool,
    pub error: Option<String>,
}
// Dashboard fields:
// pub panes: Vec<PaneState> (one or two slots)
// pub focused_pane: usize
// pub view_revision: u64
// last_view_request_id: Option<u64>
// outer_area: Rect
// retain InputMode as one dashboard-level browse/terminal switch

pub struct PaneRects {
    pub pane_index: usize,
    pub metadata: Rect,
    pub terminal: Rect,
}
// Pure, shared drawing/hit-test/resize source:
pub fn pane_rects(area: Rect, pane_count: usize, focused: usize) -> Vec<PaneRects>;
// Dashboard methods:
// focused_session(&self) -> Option<SessionId>
// split_pane(&mut self) -> bool
// focus_pane(&mut self, index: usize) -> bool
// close_focused_pane(&mut self) -> bool
// view_request(&mut self, area: Rect, request_id: u64) -> anyhow::Result<Option<ClientMessage>>
// apply_screen(&mut self, revision: u64, session: SessionId,
//              size: TerminalSize, bytes: &[u8])
```

`view_request` increments revision with checked addition (exhaustion reports an error and ends the dashboard), computes visible nonempty targets, and marks them unready. `apply_screen` accepts only the current revision and a currently visible assigned session, replaces only that pane's parser, processes the snapshot, and marks `snapshot_installed`. Only the matching final Ok marks visible panes ready, after all requested snapshots are installed. Apply incremental output only to a ready pane of the current revision. Ignore stale screens, stale dirty events, stale `Ok`, and stale view errors using revision/request ID; ordinary control errors retain their existing display behavior. On failed view acknowledgement keep input disabled and show the error.

Retain `select_session` as the sidebar entry point, but make it assign the focused slot or focus the other slot when already displayed. Migrate tests that directly write `selected`/`parser` to the new pane fields. `input_request` and both `event_action` and `event_to_request` must consult the same ready focused pane. No duplicate global parser alias is retained.

## Ordered implementation tasks

Each task follows RED → implementation → GREEN. Run the named command before and after the change, record actual output and nonzero test counts, and stop on unexpected regressions. A missing API/compiler error may establish the initial RED, but behavioral assertions must then run. Counts below are expected future counts, not claimed results.

### Current integration paths and task checkpoints

### Workspace handoff

- Direct imports use the public `ovrcr_protocol` re-exports backed by `crates/ovrcr-protocol/src/wire.rs` for pane/view messages; the current `wire` module is private. Runtime dispatch and authority use `ovrcr_runtime::server`, while pane state/input/rendering remain private children of `ovrcr_tui::dashboard` exposed through its facade. The root application continues to consume that facade through `src/lib.rs`; do not add an application compatibility module.
- Package-focused unit commands are `rtk proxy cargo test -p ovrcr-protocol --lib split_view_ -- --nocapture` (**2 protocol tests**) and `rtk proxy cargo test -p ovrcr-runtime --lib split_delivery_ -- --nocapture` (**3 runtime tests**). The dashboard state/layout cases are root integration tests in `tests/tui.rs`, so run `rtk proxy cargo test -p ovrcr --test tui split_state_ -- --nocapture` (**3 root tests**) and `rtk proxy cargo test -p ovrcr --test tui split_layout_ -- --nocapture` (**2 root tests**).
- Root integration commands are `rtk proxy cargo test -p ovrcr --test server_lifecycle split_server_ -- --nocapture` (**1 root test**) and `rtk proxy cargo test -p ovrcr --test terminal_acceptance split_terminal_ -- --nocapture` (**1 root test**); dashboard executable checks use `rtk proxy cargo run -p ovrcr --`.

With the recommended order, the dashboard already supports Paused, reported activity/context, History, and Copy. In Task 3, replace single-pane parser access through `focused_session` and pane helpers, including all input, metadata, animation, history-entry, copy-entry, and dirty-recovery callers. Preserve exhaustive mode routing: only Terminal can send application keys/paste; only Browse can split/focus/close by their bindings. The Enter predicate is **Running and ready**, not merely non-Exited.

Keep History/Copy state at dashboard scope, tagged by captured session. A focus change or focused-session replacement ends history (including pending-Begin cancellation) and cancels copy before building the new view. Resizing cancels current-screen Copy; frozen History retains original cells and changes only its viewport. An unfocused pane's output updates that pane's live parser without replacing the focused frozen view. Closing/hiding a pane changes subscriptions only; it must never reach `CloseTerminal`, kill, or remove.

Keep `PaneState.size` as the installed screen size and `desired_size` as the latest measured geometry. Allow one SetView in flight; coalesce resize/focus changes into one desired view instead of sending on every draw. An acknowledgement completes the recorded request; if desired state changed meanwhile, send the next checked revision immediately and keep input disabled. `view_request` first records the desired view, returns `Ok(None)` while a request is pending or nothing changed, and returns `Ok(Some(message))` for a new request. Revision exhaustion returns an error. Callers write only Some and preserve input refusal while changes remain pending. Match Screen by request ID, revision, and session, and match final Ok/Error by the stored request ID. Do not let an old acknowledgement clear a newer error or pending desired state.

Server lock order is `mutation → registry → sessions → dashboard-slot → view → sink` when those locks are needed together. Clone session handles and release registry/map guards before session resize/snapshot operations. Never acquire an earlier lock while holding a later one; sink writers call owner cleanup only after releasing sink locks. The dispatcher is the sole view committer: recheck owner after short session work before publishing. Extend the owner-isolation delivery test with an old writer finishing after replacement; only the old owner may be cleared.

In Task 4, clip wide glyphs by their terminal display width: if a two-cell glyph cannot fit completely inside a pane's terminal rectangle, draw a blank at that edge and never overwrite its separator or neighbor. Test wide leaders in the last and penultimate cells of both panes. In Task 5, query the hidden third session using `ReadTerminal` before/after the view change and assert its returned `TerminalText.size` remains unchanged.

Finish Task 2 before changing the running TUI to SetView. Its server checkpoint must prove all snapshots followed by the final Ok for one request, even under output pressure. Finish Task 3 with both pane parsers installed **and** the matching final acknowledgement before input becomes ready. Clearing readiness before sending a new revision is mandatory, including focus-only changes and dirty refreshes. Tests must cover late first-pane Screen, old final Ok, and a failed second resize, not only successful side-by-side drawing.

`Select`/`Resize` adapters are for existing singleton callers only. Keep their next-revision allocation in the dispatcher under the same owner check as SetView; never read/increment the revision in a connection thread. Their response sequence is Screen then Ok for the same request ID; migrate both the caller and its assertions together in Task 2. Do not leave one caller reading a Screen while an extra Ok remains queued as the next request's response.

Expand the constructor-update allowance to `src/gui.rs` and existing integration fixtures **only where changed protocol fields require it**. No GUI behavior or CLI command changes are part of this plan. Add `split_preserves_history_copy_and_paused_input` in `tests/tui.rs`: open history/copy on A, feed B output, switch focus and require cancellation, then mark B Paused and require Enter/paste refusal. Run its exact filter and require one executed test in addition to the existing task counts. Keep resource CLI input/read/close regression passing after server selection migration.

## Task 1: Define the bounded view contract

**Files:** `crates/ovrcr-protocol/src/wire.rs` and existing constructors/matches in `crates/ovrcr-runtime/src/server/{mod,dispatch,connections,outbound}.rs`, `crates/ovrcr-tui/src/dashboard/{mod,state,input,event_loop}.rs`, `tests/tui.rs`, `tests/server_lifecycle.rs`.

**Consumes:** Existing `SessionId`, `TerminalSize`, framed serialization.
**Produces:** `PaneTarget`, `DashboardView::validate`, revision fields and `Request::SetView` above.

- [ ] Add `split_view_validation_rejects_ambiguous_targets` and `split_view_frames_round_trip` to the protocol test module. The first also checks three distinct targets, zero dimensions/revision, nonempty without focus, and empty without/with focus. Use this exact duplicate/focus sketch:

```rust
let target = PaneTarget {
    session: SessionId(1), size: TerminalSize { rows: 36, cols: 39 },
};
let mut view = DashboardView {
    revision: 1, panes: vec![target.clone()], focused: Some(SessionId(1)),
};
assert!(view.validate().is_ok());
view.panes.push(target);
assert!(view.validate().is_err());
view.panes.pop();
view.focused = Some(SessionId(2));
assert!(view.validate().is_err());
let message = ClientMessage { request_id: 2, request: Request::SetView { view } };
let mut wire = Vec::new();
write_frame(&mut wire, &message).unwrap();
assert_eq!(read_frame::<ClientMessage>(&mut wire.as_slice()).unwrap(), message);
```

- [ ] Run `rtk proxy cargo test -p ovrcr-protocol --lib split_view_ -- --nocapture`; expect initial RED, then exactly 2 tests after implementation.
- [ ] Implement validation with a length check and a maximum-two nested comparison; no general subscription manager. Add exhaustive match handling and migrate constructors with explicit revision values so later tasks start from a compiling contract. Until Task 2 lands, return a concrete unsupported-request error for `SetView`.

### Task 2: Publish two ordered snapshots and bound overflow recovery

**Files:** `crates/ovrcr-protocol/src/wire.rs` for the wire-level `Request::SetView` shape, and `crates/ovrcr-runtime/src/server/{mod,dispatch,connections,outbound}.rs` plus its owning unit tests for `DispatchMessage::SetView`, dispatch ownership, and completion channels.

**Consumes:** Task 1 view contract and existing dashboard owner identity.
**Produces:** `dispatch_set_view`, `DashboardSink::replace_view`, owner-scoped view state and revision-tagged delivery.

- [ ] Add `split_delivery_snapshots_precede_increments`, `split_delivery_dirty_revisions_are_isolated`, and `split_delivery_rejects_stale_owner_and_input`. In the first, enqueue old output, replace with view revision 2 for sessions 1/2, enqueue revision-2 output, and assert exact delivery order: screen 1, screen 2, Ok, output. Exercise capacity with 62 preserved non-output responses plus three new view frames: replacement must fail and close the sink.

```rust
let sink = DashboardSink::new();
let view = DashboardView {
    revision: 2,
    panes: vec![
        PaneTarget { session: SessionId(1), size: TerminalSize { rows: 36, cols: 39 } },
        PaneTarget { session: SessionId(2), size: TerminalSize { rows: 36, cols: 40 } },
    ],
    focused: Some(SessionId(2)),
};
assert!(sink.replace_view(&view, 10, vec![b"LEFT".to_vec(), b"RIGHT".to_vec()]));
for id in [SessionId(1), SessionId(2)] {
    assert!(matches!(sink.next(), Some(DashboardDelivery::Message(DashboardOutbound {
        message: ServerMessage::Response {
            response: Response::Screen { revision: 2, session, .. }, ..
        }, ..
    })) if session == id));
}
assert!(matches!(sink.next(), Some(DashboardDelivery::Message(DashboardOutbound {
    message: ServerMessage::Response { response: Response::Ok, .. }, ..
}))));
```

- [ ] Run `rtk proxy cargo test -p ovrcr-runtime --lib split_delivery_ -- --nocapture`; expect RED, then exactly 3 tests.
- [ ] Implement the ordered path. The dispatcher algorithm is: verify owner → validate revision and every session → resize changed sessions → obtain screens → reserve/enqueue snapshots+Ok → commit view → signal completion. Reuse `Session::resize/current_screen`; never hold the view lock across PTY writes or socket operations. The request reader waits for dispatcher completion, preserving geometry/focus-before-subsequent-input ordering.
- [ ] Extend the dirty test to fill the 64-entry queue with interleaved sessions, force dirty state for both, replace the view, and deliver the old writer completion. Assert dirty keys are bounded by the current two members and old completion does not alter the replacement. Suppress further increments only for the dirty `(revision, session)`. A finite burst must leave one deliverable dirty marker after queued messages drain.
- [ ] Extend the owner/input test using the existing server-state fixture: owner A's queued view cannot mutate owner B; stale revisions and unknown sessions leave committed view and PTY geometry unchanged; nonfocused/hidden-session and control-client input are rejected. An injected second-resize failure leaves no subscription and reports `PartialFailure` without terminating either process. Extract the local resize loop as `resize_view_targets(targets: &[(Arc<Session>, TerminalSize)], resize: impl FnMut(&Session, TerminalSize) -> anyhow::Result<()>) -> anyhow::Result<()>`; production passes `Session::resize`, tests return a concrete error on the second call. No new public session trait.
- [ ] Adapt removal/disconnect clearing and singleton Select/Resize adapters. Removal of an exited focused record clears focus immediately, emits hierarchy change, and waits for the client to submit its next view; never redirect pending input to a different session.

### Task 3: Make focus, terminal modes, and replacement pane-local

**Files:** `crates/ovrcr-tui/src/dashboard/{mod,state,input,event_loop}.rs`, `tests/tui.rs`.

**Consumes:** Revision-tagged screens/output from Tasks 1–2.
**Produces:** Pane state and dashboard methods declared above; existing action/input paths use them.

- [ ] Add `split_state_focus_and_close_preserve_sessions`, `split_state_modes_and_input_are_local`, and `split_state_ignores_stale_revisions_and_removed_sessions`. Reuse `dashboard_fixture`; initialize its first pane to session 1, then use this state/mode sketch:

```rust
let mut d = dashboard_fixture();
assert!(d.split_pane());
assert_eq!(d.panes.len(), 2);
assert_eq!(d.focused_pane, 1);
let right = d.focused_session().unwrap();
assert_ne!(right, SessionId(1));
d.view_request(Rect::new(0, 0, 120, 40), 20).unwrap().unwrap();
let revision = d.view_revision;
d.apply_screen(revision, right, TerminalSize { rows: 36, cols: 40 },
    b"\x1b[?1h\x1b[?2004hRIGHT");
d.mode = InputMode::Terminal;
assert_eq!(d.event_action(Event::Paste("x".into())),
    DashboardAction::PtyBytes(b"\x1b[200~x\x1b[201~".to_vec()));
assert_eq!(d.key(KeyCode::Up), DashboardAction::PtyBytes(b"\x1bOA".to_vec()));
assert_eq!(d.input_request(vec![b'x'], 21).unwrap().request,
    Request::Input { session: right, bytes: vec![b'x'] });
```

- [ ] Run `rtk proxy cargo test -p ovrcr --test tui split_state_ -- --nocapture`; expect RED, then exactly 3 tests.
- [ ] Implement the fixed two-slot transitions and map browse controls to them. Use existing sorted visible tree rows for choosing the next different session. The left parser must remain normal-cursor/unbracketed when right has enabled modes. Test Ctrl-g, literal Tab/`v`/`x` forwarding in terminal mode, empty/exited/loading input refusal, and same-session selection focusing the other slot.
- [ ] Implement message dispatch by revision and session. In the stale test: assign A, then B, then A again; inject the first A snapshot/output/dirty marker and assert the new A remains unready and unchanged. Apply the current A screen and assert readiness. Inject a hierarchy without A, rebuild the desired view, and verify no old response can populate its replacement. Closing a pane returns a view update, never `KillSession`/`RemoveSession`.

### Task 4: Draw exact pane rectangles and resize every visible PTY

**Files:** `crates/ovrcr-tui/src/dashboard/{mod,state,input,render,event_loop}.rs`, `tests/tui.rs`.

**Consumes:** Task 3 pane state and `view_request`.
**Produces:** `pane_rects`, shared rendering/resize/hit-test geometry and unchanged bounded event-loop behavior.

- [ ] Add `split_layout_geometry_and_narrow_fallback` and `split_layout_renders_independent_cells_and_cursor`. Use exact geometry checks:

```rust
let rects = pane_rects(Rect::new(0, 0, 120, 40), 2, 1);
assert_eq!(rects[0].terminal, Rect::new(40, 3, 39, 36));
assert_eq!(rects[1].terminal, Rect::new(80, 3, 40, 36));
let narrow = pane_rects(Rect::new(0, 0, 80, 24), 2, 1);
assert_eq!(narrow.len(), 1);
assert_eq!(narrow[0].pane_index, 1);
assert_eq!(narrow[0].terminal, Rect::new(40, 3, 40, 20));
assert!(pane_rects(Rect::new(0, 0, 1, 1), 2, 1).is_empty());
```

- [ ] Run `rtk proxy cargo test -p ovrcr --test tui split_layout_ -- --nocapture`; expect RED, then exactly 2 tests.
- [ ] Implement `pane_rects` by splitting the existing right body before applying its metadata height. Preserve `actual_drawn_inner_rect` as the existing single-pane helper. Draw the separator through the right body's height and use each pane's metadata width for clipping. In split mode the metadata starts with `> waiting 39x36` for a ready focused session named waiting, or `  mouse 40x36` for the other pane; append PID/elapsed fields as width permits. An unready pane says `loading waiting` instead. Keep the current singleton metadata layout. Retain dense tree rows, accent focus color, and square separators. Give only the focused ready pane permission to set the cursor, and only in terminal mode.
- [ ] In the TestBackend test, put red `LEFT` and green `RIGHT` in separate parsers, put a wide character at the penultimate column and a marker on each last row, then assert cells do not spill into the separator. Assert the exact cursor offset with left/right focus, hidden application cursor, browse mode, tiny fallback, and return to split width. Click terminal rectangles in browse mode to select focus; separator/metadata clicks and all terminal-mode clicks send no bytes.
- [ ] Replace all three single-size calculations in `dashboard_loop` with shared view geometry reconciliation. Send a `SetView` only when assignment, focus, or visible sizes change, or a dirty notification requests refresh. Do not reset local parsers on every poll or optimistically resize them before the authoritative snapshot. Preserve geometry-before-input ordering when SIGWINCH and input arrive together.
- [ ] Keep the 64-frame server-message batch, 32-input-event batch, 16 ms frame interval, and one pre-render drain. Rendering walks at most two current screens once per due frame; no thread, timer, redraw loop, or unbounded cache per pane. Do not multiply channel capacities by pane count. Test one finite burst from each pane and an acknowledgement in the other to catch starvation.

### Task 5: Prove real geometry, two-stream recovery, and detach cleanup

**Files:** `tests/server_lifecycle.rs`, `tests/terminal_acceptance.rs`.

**Consumes:** All previous tasks and existing `ControlFixture`, `AcceptanceFixture`, and `OuterDashboard`.
**Produces:** One new server integration test and one new real-terminal acceptance test, plus existing singleton regression coverage.

- [ ] Add `split_server_two_streams_resize_resync_and_detach`. Reuse `ControlFixture` project/workspace setup from `selection_snapshot_precedes_later_quiet_tail_output`. Start two shells with this command body, differing only in `SIDE=LEFT` / `SIDE=RIGHT`:

```sh
SIDE=LEFT; printf '%s_READY\n' "$SIDE"; while IFS= read -r line; do case "$line" in SIZE) printf '%s_SIZE_%s\n' "$SIDE" "$(stty size)";; BURST) i=0; while [ "$i" -lt 200000 ]; do printf '%s_%06d\n' "$SIDE" "$i"; i=$((i+1)); done; printf '%s_FINAL\n' "$SIDE";; *) printf '%s_ACK_%s\n' "$SIDE" "$line";; esac; done
```

This is a PTY fixture command body passed through `CreateSessionRequest`, not a developer shell command. Use `write_frame`/`read_frame` with two-second read deadlines, revisions 1 onward, and an absolute deadline for each predicate. Submit a view with sizes 36×39 and 36×40, wait for both snapshots and Ok, input `SIZE\n` to the focus, switch focus and repeat. Parse `LEFT_SIZE_36 39` and `RIGHT_SIZE_36 40` from actual output. Update both to 26×29/26×30, then verify both reported sizes again; assert a hidden third session retains its previous PTY size.
- [ ] Trigger both finite bursts, stop reading the dashboard while polling CLI hierarchy/control responsiveness, then drain until dirty notifications arrive and request a new view. Maintain a `vt100::Parser` for each session and assert both reconstructed final markers, including when bursts have already stopped. Check no old-revision output is applied after replacement. Disconnect, assert both PIDs/process groups remain live, attach a new singleton then two-pane view and verify markers, and finish with existing shutdown-and-join cleanup plus process-group absence checks. A socket error, missing marker, zero test count, or timeout is a failure.
- [ ] Run `rtk proxy cargo test -p ovrcr --test server_lifecycle split_server_ -- --nocapture`; expect RED before full integration, then exactly 1 test. Also run `rtk proxy cargo test -p ovrcr-runtime --lib split_delivery_ -- --nocapture` to prove queue limits deterministically rather than relying only on socket buffering to trigger overflow.
- [ ] Add `split_terminal_acceptance_preserves_input_and_geometry` using the existing fixture's `waiting` and `mouse` sessions. This executable core selects waiting, opens mouse in the second pane, verifies distinct commands, then resizes both:

```rust
let mut fixture = AcceptanceFixture::new()?;
fixture.setup()?;
let mut d = OuterDashboard::start(&fixture, PtySize {
    rows: 40, cols: 120, pixel_width: 0, pixel_height: 0,
})?;
d.send(b"j")?;
d.wait_for(b"WAITING_READY", Duration::from_secs(3))?;
d.send(b"v")?;
d.wait_for(b"MOUSE_READY", Duration::from_secs(3))?;
d.send(b"\rMOUSE_TOKEN\r")?;
d.wait_for(b"MOUSE_ACK", Duration::from_secs(3))?;
d.send(b"\x07\t")?;
d.wait_for(b"> waiting", Duration::from_secs(3))?;
d.send(b"\rWAITING_TOKEN\r")?;
d.wait_for(b"WAITING_ACK", Duration::from_secs(3))?;
d.resize(30, 100)?;
d.wait_for(b"> waiting 29x26", Duration::from_secs(3))?;
d.send(b"SIZE_TOKEN\r")?;
d.wait_for(b"SIZE_ACK_26 29", Duration::from_secs(3))?;
d.send(b"\x07\t")?;
d.wait_for(b"> mouse 30x26", Duration::from_secs(3))?;
d.send(b"\rSIZE_TOKEN\r")?;
d.wait_for(b"SIZE_ACK_26 30", Duration::from_secs(3))?;
d.send(b"\x07")?;
d.detach()?;
fixture.shutdown()?;
```

- [ ] Complete that test with per-rectangle assertions against `d.parser.screen()`: `WAITING_ACK` only in the left terminal, `MOUSE_ACK` only in the right. Wait for a post-resize screen/readiness marker rather than treating a previously rendered title as evidence that resize completed. Exercise narrow fallback, expansion, `x`, reattachment into one pane, and rebuilding two panes without changing managed PIDs. Reuse fixture `Drop` and bounded shutdown so failing assertions still clean up or preserve the fixture with a concrete error.
- [ ] Run `rtk proxy cargo test -p ovrcr --test terminal_acceptance split_terminal_ -- --nocapture`; expect exactly 1 new test. Run `rtk proxy cargo test -p ovrcr --test terminal_acceptance default_dashboard_acceptance_wrapper_exercises_pty_controls -- --exact --nocapture`; expect exactly 1 existing test, including its recorded nonzero latency samples and p95 <100 ms assertions.

### Task 6: Review regression evidence and visible acceptance

**Files:** No new implementation files; review the files above and existing test output.

- [ ] Run `rtk proxy cargo fmt --all -- --check`, then `rtk proxy cargo test --workspace --all-targets --all-features`. The new filters account for 12 tests (2 protocol, 3 delivery, 3 state, 2 layout, 1 socket, 1 outer-PTY); require each named test to execute and retain existing suites. Do not assert a fixed combined count before collecting it.
- [ ] Run `rtk proxy cargo test -p ovrcr --test server_lifecycle fifty_sessions_survive_detach_and_leave_no_process_groups -- --exact --nocapture` only if that named test was absent from the preceding run or needs a failure rerun; expected 1 test. It must retain the existing cleanup assertions, not just return a successful exit code.
- [ ] Use the existing disposable GUI helper with `rtk proxy just gui`. In a real terminal or that helper, show two independent full-screen applications, switch focus through browse mode, send bracketed paste, resize through split→narrow→split, close a pane, detach and reattach. Verify actual visible text, cursor, colors, sidebar line spacing, and that an unfocused pane continues rendering. On macOS use the existing accessibility rows to confirm both pane labels. Record macOS/Linux checks separately; unavailable GUI/platform evidence remains explicitly unverified.
- [ ] Review that no pane action invokes session kill/remove, no new dependency/runtime/persistence exists, and no closed/hidden pane retains a live output subscription. Report only actual test results. Completing this plan document does not run these implementation checks.

## Acceptance criteria

- Two different sessions render simultaneously at their independently measured PTY sizes; at 120×40 they are 39×36 and 40×36.
- The focused ready live pane alone receives keyboard/paste input, with its own application-cursor and bracketed-paste modes. A stale response cannot replace a reassigned pane.
- View snapshots precede incremental output for that revision; bounded overflow recovers both screens after finite output stops. Responses and lifecycle events are preserved or the dashboard disconnects cleanly.
- Narrow terminals retain local pane assignments and restore split geometry without zero-sized PTY requests or cursor spill.
- Close, hide, detach, and reattach preserve PTYs/process groups. Explicit shutdown still drains final output, terminates groups, and joins cleanup as before.
- Existing sidebar, terminal restoration, singleton input/resize, and 50-session gates continue to execute. Both pane state and layout have deterministic checks plus real PTY evidence.

## Risks and cross-plan dependencies

- **Protocol overlap:** This changes `Screen`, `Output`, and `ScreenDirty` constructors and deferred-response handling. Rebase other plans touching `protocol.rs`/`server.rs` onto this contract, or agree a single replacement before implementation. Server and clients still require the same binary version.
- **Multiple dashboards:** Not a dependency. This plan retains one dashboard. If that feature lands first, store `DashboardView`, revision, dirty keys, and default geometry under each dashboard identity and include that identity in dispatcher commands. The same PTY shown by different dashboards still has only one kernel size: use that feature's explicit writer/geometry authority; never silently apply last-resizer-wins. Its input authority and this pane focus must both permit a write.
- **Mouse forwarding:** Not a dependency. `pane_rects` is the shared seam for mapping outer coordinates to a visible pane and then PTY-relative coordinates. This plan consumes browse clicks for focus only. A later forwarding implementation must exclude metadata/separators, honor per-pane mouse modes, and resolve focus/drag ownership explicitly.
- **Scrollback and copy mode:** Not dependencies. Bind viewport/selection state to the pane's session assignment and reset it when that assignment changes. Follow each sibling's capture lifetime: history permits one frozen server snapshot per dashboard and releases it on selected-session change; panes do not multiply that allowance. Copy mode follows its own cancellation rules. Their modes must suppress PTY input and use this plan's geometry, rather than resurrect a global parser or global selection.
- **Snapshots and resize:** Full view refresh on focus and dirty recovery is a deliberate two-pane ceiling. It costs at most two current-screen snapshots per change and keeps one ordering rule. Do not introduce delta snapshots or per-pane wire protocols without measured need. Kernel resize can partly succeed; the concrete failure/disconnect policy above prevents stale input assumptions without claiming process rollback.
- **Full-screen fidelity:** `state_formatted()` is existing reconstruction behavior, not proof every agent terminal mode survives snapshots. Validate alternate screens, cursor visibility, wide cells, and the actual target CLI during visible acceptance; report parser limitations rather than adding an unrelated terminal emulator.

## Plan self-review

Reviewed against the README checkbox and the approved MVP's ownership, queue, geometry, and lifecycle rules. The explicit changes are two visible PTYs, pane focus, and view-scoped output; other deferred features remain independent. Interfaces, geometry arithmetic, task test filters, and expected new test counts are internally aligned. No implementation or test execution is claimed by this document.
