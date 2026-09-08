# Multiple Dashboards Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Status:** Deferred by user decision on 2026-09-07. Retain this plan for later; do not assign implementation until the user explicitly resumes it. Session restore can proceed without this feature. The defaults below remain proposed assumptions for review, not an approved specification or existing behavior.

**Goal:** Connect several dashboards to one server, independently select sessions, and deliberately transfer control when two dashboards watch the same session.

**Architecture:** Keep one synchronous server, the ordered PTY dispatcher, and blocking socket threads. Replace the singleton dashboard with a bounded connection registry; each connection owns its selection, geometry, output queue, and writer. Give each session one controller whose identity and epoch authorize both input and kernel PTY resizing.

**Tech Stack:** Existing Rust edition 2024 package; Ratatui 0.30, Crossterm 0.29, portable-pty 0.9, vt100 0.16, Serde/Bincode, and standard-library locks, threads, and channels. No new dependency or Cargo feature.

**Spec:** `README.md`, checkbox “Multiple dashboards connected to the same server”; `plans/2026-09-04-ovrcr-mvp-design.md`, Architecture, Terminal data flow, Local protocol, and Failure handling. The approved MVP explicitly excludes this feature; this document proposes the extension and records the changed assumptions.

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → session restore**. Multiple dashboards is deferred as of 2026-09-07 and excluded from this sequence until explicitly resumed. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --workspace --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Global constraints

- Preserve: “The MVP targets macOS and Linux, supports up to 50 live sessions”. The one-dashboard assumption is the only architecture limit this feature deliberately replaces.
- Preserve: “The server stores live sessions in memory.” Connection and ownership state also disappear with the server.
- Preserve: “OVRCR does not use Tokio, a database, a shell-command builder, or an agent SDK.”
- Preserve: “Socket writes never block the parser dispatcher.” Keep the bounded, lossless raw PTY queues and final-output-before-exit ordering.
- Preserve private Unix-socket access, one mutating-operation owner, ordinary CLI lifecycle commands, and process-group/worktree removal gates.
- Use `rtk` before every execution command; `rtk proxy cargo test` exposes actual test counts. Commands below are proposed checks, not executed results.

## Current code grounding

Inspected at HEAD `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb` on 2026-09-05. Refresh HEAD and read the final peer plans before implementation.

| File and current entry point | Relevant behavior |
| --- | --- |
| `crates/ovrcr-runtime/src/server/{mod,connections,dispatch,outbound}.rs`, `ServerState` | Global `selected`, `dashboard`, `dashboard_size`, and `dashboard_slot` assume one dashboard. |
| `crates/ovrcr-runtime/src/server/connections.rs`, `handle_connection` | Rejects a second hello; creates one writer; identity uses `Arc<()>` to protect replacement cleanup. |
| `crates/ovrcr-runtime/src/server/dispatch.rs`, `run_dispatcher` | Select and PTY output share an ordering boundary; Input and Resize currently execute in connection readers. |
| `crates/ovrcr-runtime/src/server/outbound.rs`, `DashboardSink` | 64 queued messages; incremental overflow creates per-session dirty recovery; reliable overflow disconnects. |
| `crates/ovrcr-runtime/src/server/mod.rs`, `create_session_locked` | New sessions inherit singleton geometry, otherwise 120×40. |
| `crates/ovrcr-runtime/src/session/{mod,io}.rs`, `write` | Blocking `write_all`; never move it into the dispatcher or hold a registry lock across it. |
| `crates/ovrcr-runtime/src/session/mod.rs`, `resize` | Changes kernel PTY dimensions and parser dimensions together; each session has only one physical terminal size. |
| `crates/ovrcr-tui/src/dashboard/{mod,state,input,event_loop}.rs`, `handle_server_message` | Filters only by session; overwrites pane geometry from snapshots and locally resizes its parser on outer resize. |
| root `tests/server_lifecycle.rs` | Real `ControlFixture`, slow-reader recovery, input backpressure, selection ordering, shutdown acknowledgement, and 50-session coverage. |
| root `tests/tui.rs`, `tests/terminal_acceptance.rs` | TestBackend assertions and real outer-PTY `AcceptanceFixture`/`OuterDashboard` provide existing acceptance seams. |

## Proposed interaction policy

1. Admit at most **8 dashboard connections** per server. A ninth gets `Conflict: dashboard limit (8) reached`, then closes. CLI connections do not consume dashboard slots. Repeated hello on an admitted connection retains the current single-writer refusal and disconnect behavior.
2. Selection is independent. Selecting an unowned running session claims its input and resize authority; selecting an already controlled session subscribes read-only. Merely resizing an observer never steals control. Ctrl-g and switching focus among visible panes retain control; removing a session from the visible view or detaching releases it. No automatic promotion of existing observers: they use `t` to claim a released session.
3. In Browse, **t takes control** of the selected session using the last observed control epoch. A successful takeover moves input and PTY-size authority together. A concurrent stale takeover gets `Conflict: control changed; retry`; it does not immediately take control back. Enter is available only after an authoritative controller snapshot; observers remain in Browse.
4. Display `CONTROL · size 80×24` or `READ ONLY · dashboard 3 · size 80×24 · t take control`; when unowned, display `READ ONLY · unowned · t take control`. Display `input finishing; retry takeover` for an accepted write still in progress. Losing control forces Browse and suppresses keys, paste, and future terminal-mouse input until control is reacquired.
5. Store each dashboard's desired pane size independently. A controller's desired size becomes the real PTY/parser size. Observers keep parsers at the **server's actual size**, clip the top-left screen to their local pane, and pad a larger viewport; show `clipped` if either actual dimension exceeds the viewport. Do not reflow the same byte stream at an observer's width or repeatedly resize to match it.
6. Every authority change or successful controller resize advances a per-session epoch and pushes a new authoritative current screen to every subscriber before subsequent output. Input from older epochs is refused. Resize failure publishes the actual retained geometry and an error; failed takeover leaves the previous owner intact.
7. Initial geometry for creation uses the requesting dashboard's latest geometry. A CLI creation uses the sole connected dashboard's geometry only when exactly one exists; with zero or several dashboards use the existing 120×40 default. Thus adding another dashboard cannot make CLI creation depend on hash-map iteration order. New sessions are unowned until selected.
8. Exited sessions remain viewable but unowned and never accept control/input/PTY resize. An observer resizing its viewport can still update local geometry. Removing a retained session clears every matching subscription and notifies every dashboard.

**Accepted-input boundary:** Existing PTY writes can block. Admit at most one write per session, retain its in-flight token, and perform the blocking write in that dashboard's existing connection reader. Takeover while a write is in flight returns Conflict immediately; it never waits in the dispatcher. A disconnect revokes the holder but an already admitted write may finish; no new holder is granted until its token clears. This deliberately excludes cancellation of partially written input and guarantees that no former-controller bytes arrive after a *successful* takeover acknowledgement. A disconnected blocked reader may be noticed only when its write completes or another socket operation detects failure. `kill` and `shutdown --kill` remain available to end a non-reading child. Do not add a PTY writer subsystem in this feature.

## Scope and file map

Modify `crates/ovrcr-protocol/src/wire.rs` for identity, revision, epoch, and control messages; modify `crates/ovrcr-runtime/src/server/{mod,connections,dispatch,outbound}.rs` for routing, membership, authority, and teardown; modify `crates/ovrcr-tui/src/dashboard/{mod,state,input,render,event_loop}.rs` for control state, clipping, and takeover. Keep `Session::write` unchanged; use existing `ovrcr_runtime::session::Session::resize` and `current_screen`. Modify root tests in `tests/server_lifecycle.rs`, `tests/tui.rs`, and `tests/terminal_acceptance.rs`, plus protocol/runtime unit tests in their owning crate modules. Update the relevant README usage and one roadmap checkbox only after implementation acceptance. No new production module, dependency, config format, or persisted identity is needed.

Exclude permissions between different users, remote access, named clients, leader election, server failover, session restoration, mirrored keyboard input, forced cancellation of accepted input, shared pane layouts, and unlimited dashboard counts. Split panes and history are independent plans, with integration rules below.

## Shared interfaces and ordering

Wire types derive the existing serialization/equality traits; IDs also derive Copy and Hash. Add these types to `crates/ovrcr-protocol/src/wire.rs` and re-export them from `ovrcr_protocol`:

```rust
pub struct DashboardId(pub u64);
pub struct ControlState {
    pub holder: Option<DashboardId>,
    pub epoch: u64,
    pub size: TerminalSize,
    pub input_in_flight: bool,
}
// New response:
// Response::DashboardReady { dashboard: DashboardId, hierarchy: HierarchySnapshot }
// Replace Select and Resize shapes; revision is strictly increasing per connection:
// Request::Select { revision: u64, session: SessionId, size: TerminalSize }
// Request::Resize { revision: u64, session: SessionId, size: TerminalSize }
// Request::Input { revision: u64, session: SessionId, epoch: u64, bytes: Vec<u8> }
// Request::TakeControl { revision: u64, session: SessionId, expected_epoch: u64 }
// Response::Screen { revision: u64, session: SessionId,
//     control: ControlState, bytes: Vec<u8> }
// ServerEvent::Screen { revision: u64, session: SessionId,
//     control: ControlState, bytes: Vec<u8> } // unsolicited authoritative reset
// ServerEvent::Output { revision: u64, session: SessionId, epoch: u64, bytes: Vec<u8> }
// ServerEvent::ScreenDirty { revision: u64, session: SessionId, epoch: u64 }
// ServerEvent::ControlChanged { session: SessionId, control: ControlState }
```

ControlChanged updates ownership/busy hints without resetting parser state. A new epoch requires a Screen before input or output at that epoch is consumed; ControlChanged alone cannot mark the pane ready. Screen embeds actual dimensions via `control.size`. Every request response is addressed by connection and request ID; hierarchy/lifecycle events fan out, selected output does not.

`Resize` advances selection revision and returns Screen even for an observer; it records observer geometry without changing the PTY. `TakeControl` and Input must exactly match the committed revision. Select/Resize require a greater nonzero revision, nonzero dimensions, and an existing selected/session target as applicable. A request superseded in the client disables input until its matching Screen arrives. Empty view is produced by removal/disconnection, not by forging a SessionId. All counters use checked increment; exhaustion closes that connection or refuses the operation without wrapping/reusing identity.

Replace singleton state with the following server-local structures in `crates/ovrcr-runtime/src/server/{mod,connections,dispatch,outbound}.rs`:

```rust
struct DashboardSlot {
    id: DashboardId,
    selected: Option<SessionId>,
    revision: u64,
    geometry: Option<TerminalSize>,
    sink: Arc<DashboardSink>,
    stream: UnixStream,
}
struct SessionControl {
    state: ControlState,
    in_flight: Option<u64>, // unique input token, retained across disconnect
}
struct Dashboards {
    clients: HashMap<DashboardId, DashboardSlot>,
    controls: HashMap<SessionId, SessionControl>,
    next_id: u64,
    next_input: u64,
}
// ServerState: dashboards: Mutex<Dashboards>; remove all four singleton fields.
// Connection context: Option<DashboardId>; role alone never authorizes input.
// ServerState::handle_request gains dashboard: Option<DashboardId>.
// handle_request_with_id likewise gains dashboard: Option<DashboardId>.
// DispatchMessage::Dashboard { dashboard: DashboardId, request_id: u64,
//     request: Request, completion: SyncSender<()> }
// fn dispatch_dashboard(state: &ServerState, dashboard: DashboardId,
//     request_id: u64, request: Request, completion: SyncSender<()>);
// fn send_dashboard(state: &ServerState, id: DashboardId,
//     message: ServerMessage, completion: Option<SyncSender<Result<(), String>>>) -> bool;
// fn broadcast_dashboards(state: &ServerState, message: ServerMessage);
// fn disconnect_dashboard(state: &ServerState, id: DashboardId);
// fn begin_input(state: &ServerState, id: DashboardId, revision: u64,
//     session: SessionId, epoch: u64) -> Result<(Arc<Session>, u64)>;
// fn finish_input(state: &ServerState, session: SessionId, token: u64);
```

Route Select, Resize, TakeControl through the existing ordered dispatcher. `begin_input` checks live identity, stopping flag, selected session/revision, holder/epoch and no in-flight token under the same dashboard lock; it reserves a token, drops locks, and returns the session. The reader calls `session.write`; use a scope guard to call `finish_input` on success, error or unwind. Token completion clears only the matching token; it never reinstates a holder. Select/SetView releases leases only for sessions leaving that client's visible view, without disturbing pending tokens.

Initialize SessionControl alongside successful session registration with holder None, epoch 1, actual spawn size, and no input token. Clear ownership on Exited and remove the control record on session removal. Lock order is sessions map → dashboards → individual sink; clone session Arcs and drop the sessions guard promptly. Never acquire sessions/mutation locks while holding dashboards, never hold dashboards across socket or PTY writes, and never join a thread under these locks. The dispatcher may hold dashboards through the short resize ioctl and authoritative snapshot enqueue to serialize authority against input admission. Refuse controller geometry-changing resize while input is in flight with the same retryable Conflict, avoiding epoch changes mid-write. Observer viewport changes and same-size screen refresh remain allowed.

Reliable screen publication and subscription commit occur before the next PTY event. Hold dashboards across parser event application and outgoing enqueue as well, so direct disconnect/lease release can acquire that same lock and publish an unowned epoch plus screen without racing a duplicate output chunk. Clone affected session Arcs before taking dashboards. On resize/takeover/release, enqueue resets for all viewers at their own revisions and the new shared epoch; purge their superseded increments/dirty entries. Bytes already held by a writer arrive before its reset. Include `(revision, session, epoch)` in dirty keys and write-completion callbacks, so an old dirty completion cannot mark a newer stream dirty. While dashboards is held, call the already captured sink's enqueue directly; collect failed IDs and disconnect only after releasing the guard, never recursively call send_dashboard or disconnect under that lock. Send errors only to the requester; continue fanout to healthy sinks.

## Current integration paths and task checkpoints

### Workspace handoff

- Wire definitions live in `crates/ovrcr-protocol/src/wire.rs` and are consumed through the public `ovrcr_protocol` re-exports; the current `wire` module itself is private. Runtime authority and dispatch are in `ovrcr_runtime::server::{mod,connections,dispatch,outbound}`, while client state/input/rendering are private children of `ovrcr_tui::dashboard` exposed through the existing facade in root `src/lib.rs`.
- Package-focused checks are `rtk proxy cargo test -p ovrcr-protocol --lib multi_dashboard_`, `rtk proxy cargo test -p ovrcr-runtime --lib multi_dashboard_`, and `rtk proxy cargo test -p ovrcr-tui --lib multiple_dashboards_` as applicable. Root behavior remains exercised through `rtk proxy cargo run -p ovrcr --` and the root integration targets below.
- Root integration commands are `rtk proxy cargo test -p ovrcr --test server_lifecycle multiple_dashboards_ -- --nocapture`, `rtk proxy cargo test -p ovrcr --test tui multiple_dashboards_ -- --nocapture`, and `rtk proxy cargo test -p ovrcr --test terminal_acceptance multiple_dashboards_ -- --nocapture`.
- The root `src/cli/` resource commands remain a separate synchronous control-client path. They must use the same runtime authority checks and are covered by `rtk proxy cargo test -p ovrcr --test resource_cli -- --nocapture`; they do not belong in the TUI crate.

With split panes already landed, implement this plan using `DashboardSlot.view: DashboardView` and its single revision; the singleton structs below describe the pre-split baseline, not a second state model to add. Store each client's frozen history state with that slot. Subscription means membership in its visible view; Input/TakeControl additionally require its focused session. Reuse SetView and add control epochs to its Screen/Output/ScreenDirty contract. A full SetView response still completes with one final Ok after its pane snapshots; unsolicited epoch-reset Screens are events, never extra responses to a completed request.

Canonical integrated wire shape: retain `Response::Screen { revision, session, control: ControlState, bytes }`, with **actual size only in `control.size`**; unsolicited resets use the same fields in `ServerEvent::Screen`. `Output` and `ScreenDirty` carry `(revision, session, epoch)` as listed below. Migrate every split-pane Screen consumer/fixture together rather than adding a second, potentially inconsistent size field. For SetView, use each viewer's revision and each session's current epoch; the final Ok belongs only to the originating request.

Route existing `Request::DashboardGeometry { size }` by the admitted dashboard ID, validate nonzero size, and store it in that slot even before it selects a session. SetView updates that slot's creation geometry from the focused PaneTarget; an empty view retains its last nonzero geometry. `create_session_locked` chooses the requesting dashboard's geometry, or for Control clients the sole dashboard's geometry when exactly one exists, otherwise 120×40. Initialize SessionControl immediately with successful registration; remove it and all matching subscriptions in `remove_session_locked`. Extend Task 2 admission and Task 3 kernel-size tests with zero/one/two dashboards, ReadTerminal actual-size assertions, and create→observe from two clients→close→both subscriptions cleared.

Use one `release_control_locked(dashboards, dashboard_id, session_id)` transition under the dashboards mutex, called only after collecting required session handles in the established lock order. It clears the matching holder, keeps an in-flight token, advances epoch once, and queues ControlChanged followed by authoritative Screen to remaining viewers before later output. Repeated/non-holder cleanup is a no-op. Voluntary view removal, disconnect, and Exited use this transition; Exited screens remain readable. Ctrl-g/focus-only changes do not release. No extra dispatcher wait is introduced from a dispatcher callback.

Complete server lock order is `mutation → registry → sessions → dashboards → session state/terminal locks → sink`; take only the needed subset. Snapshot helpers release per-session state guards before taking dashboards. Writer callbacks release sink guards before invoking disconnect. `disconnect_dashboard(id)` removes/marks only that ID under lock, then shuts down sockets outside it; the reader owns writer joining and never joins itself. Extend the old-ID disconnect test with a late writer callback after a fresh ID attaches. Never wait for a PTY write, dispatcher completion, or thread join while holding a server lock.

The current resource CLI adds another blocking input path: `Request::SendTerminal` → `ServerState::send_terminal` → `Session::send_text`. Keep that explicit control-client command available as scripting authority, but make it reserve the **same per-session in-flight token** as dashboard Input. Introduce `begin_control_input(state, session) -> Result<(Arc<Session>, u64)>`: validate Control role before dispatch, stopping/lifecycle, and no outstanding token; reserve without changing holder or epoch. The existing connection reader then runs the single `send_text(text, submit)` operation and drops a completion guard. It must not execute in the ordered dispatcher.

A Dashboard-role connection is forbidden from sending SendTerminal, so an observer cannot bypass Input's holder/revision/epoch checks. A Control-role send is an explicit user scripting operation; document that it may write to a controlled terminal and that takeover/controller resize returns Conflict until that admitted operation finishes. Never hold dashboards, sessions, or mutation locks while either write blocks. Pause still rejects new input on both paths; kill, close, and shutdown remain available to unblock a non-reading child.

Add `multiple_dashboards_control_send_obeys_input_reservation` in `tests/server_lifecycle.rs`: admit a control-client send to a deliberately non-reading child, observe its in-flight state, refuse a second send and takeover without waiting, refuse a Dashboard-role SendTerminal, and prove List plus KillSession remain responsive. After a normal admitted send finishes, require takeover success and exactly one child-generated marker. Use the existing backpressure fixture's bounded coordination rather than a sleep as admission proof. Run the exact filter (one executed test), in addition to the original six integration cases.

Treat pane focus separately from ownership. Ctrl-g and focus switching preserve a lease for still-visible sessions; removing/hiding a pane releases its lease and publishes an epoch reset to its other viewers. Running sessions may be claimed; Paused sessions remain observable/resizable by an existing holder but receive no new input. A free Paused session remains unowned until resumed and explicitly claimed. Copy/History never enable input merely because the client holds control. Mouse state is cleared on control loss without sending stale-epoch release bytes.

Task 2's caller audit includes Inspect/List, ReadTerminal, SendTerminal, CloseTerminal, current spawn-size selection, and every cleanup callback formerly referring to the singleton owner. Run the existing resource CLI regression once at completion. Record the integrated split/history/mouse tests separately from the single-pane acceptance counts; do not rebuild those features here.

## Ordered implementation tasks

Each task includes a RED/GREEN cycle. Run the listed focused command before and after implementation; first compile failures are acceptable initial RED, but final assertions must execute. Expected counts below refer to the named tests added by that task, not existing results.

### Task 1: Define connection and control protocol

**Files:** `crates/ovrcr-protocol/src/wire.rs`; migrate message constructors/exhaustive matches in `crates/ovrcr-runtime/src/server/{mod,connections,dispatch,outbound}.rs`, `crates/ovrcr-tui/src/dashboard/{mod,state,input,event_loop}.rs`, and existing root tests.
**Consumes/produces:** Existing frame codec → DashboardId, ControlState and all revised message shapes above. Keep the 1,048,576-byte frame maximum and same-binary-version expectation.

- [ ] Add `multi_dashboard_control_frames_round_trip` and `multi_dashboard_zero_identity_is_not_client_supplied` in the protocol/server test modules respectively. Hello never accepts an ID; server allocation starts at 1. Round-trip raw input bytes and control/screen messages:

```rust
let message = ClientMessage { request_id: 12, request: Request::Input {
    revision: 3, session: SessionId(7), epoch: 4, bytes: vec![0, 27, 255],
}};
let mut bytes = Vec::new();
write_frame(&mut bytes, &message).unwrap();
assert_eq!(read_frame::<ClientMessage>(&mut bytes.as_slice()).unwrap(), message);
```

- [ ] Run the owning unit gates separately: `rtk proxy cargo test -p ovrcr-protocol --lib multi_dashboard_control_frames_round_trip -- --nocapture` (exactly **1 test**) and `rtk proxy cargo test -p ovrcr-runtime --lib server::tests::multi_dashboard_zero_identity_is_not_client_supplied -- --exact --nocapture` (exactly **1 test**). Implement the types and constructor migration; retain the existing malformed/oversize checks.
- [ ] Replace role-only dashboard authorization in `handle_request` test callers with an actually registered ID; control callers pass None. Do not create a compatibility path that bypasses registration.

### Task 2: Give every dashboard independent routing and lifetime

**Files:** `crates/ovrcr-runtime/src/server/{mod,connections,dispatch,outbound}.rs`, `tests/server_lifecycle.rs`.
**Consumes/produces:** Task 1 identities → Dashboards registry, targeted send, bounded admission, independent selection and cleanup.

- [ ] Add unit test `multi_dashboard_disconnect_is_idempotent_and_local`: allocate A/B, remove A, register C, invoke A's cleanup twice, and assert B/C remain with their original selection/geometry. Extend the existing `test_state` fixture to register actual slots instead of building singleton aliases.
- [ ] Add integration tests `multiple_dashboards_independent_selection` and `multiple_dashboards_limit_and_duplicate_hello`. Use two sessions, colliding request IDs, and different dimensions; verify each Screen/response goes only to its requester and output only to that session's subscribers. Register 8, reject 9, close one, then admit a new, strictly greater ID.
- [ ] Add a local `DashboardClient` test helper with `connect(socket: &Path) -> Self`, `request(request: Request) -> Response`, and `next() -> ServerMessage`. It owns a UnixStream, assigned DashboardId, incrementing request ID, and a `VecDeque<ServerEvent>`; request queues events while waiting for its exact response. `connect` sends hello and requires DashboardReady. Set socket read/write timeouts to three seconds, never discard partially read timed-out frames and resume decoding them.
- [ ] Implement connection-scoped dispatch/completions and idempotent teardown. Closing a slot calls `sink.close()` and `stream.shutdown(Both)` before joining its writer; take the slot out under lock, then close outside it. Connection IDs are never reused, so late reader/writer cleanup cannot remove a replacement.

```rust
let recipients = {
    let dashboards = state.dashboards.lock().unwrap();
    dashboards.clients.keys().copied().collect::<Vec<_>>()
};
for id in recipients { send_dashboard(state, id, message.clone(), None); }
```

- [ ] Run `rtk proxy cargo test -p ovrcr --test server_lifecycle multiple_dashboards_ -- --nocapture`: **2 root integration tests passed** at this step. Run `rtk proxy cargo test -p ovrcr-runtime --lib server::tests::multi_dashboard_disconnect_is_idempotent_and_local -- --exact --nocapture`: **1 runtime unit test passed**.

### Task 3: Serialize input authority and actual PTY geometry

**Files:** `crates/ovrcr-runtime/src/server/{mod,connections,dispatch,outbound}.rs`, `tests/server_lifecycle.rs`; existing `crates/ovrcr-runtime/src/session/mod.rs` methods unchanged.
**Consumes/produces:** Registered identities and ordered dispatch → SessionControl, begin/finish input, atomic takeover, observer snapshots.

- [ ] Add `multiple_dashboards_control_and_kernel_resize` and `multiple_dashboards_takeover_rejects_stale_or_inflight_input`. Extend the fixture setup already used by `backpressured_input_and_send_do_not_block_inspect_or_kill`; register its real Git project/workspace and create real PTY shells.
- [ ] Test the same-session sequence A selects 24×80, B selects 40×120, B attempts input, B resizes to 30×100, A explicitly sends its tagged size command and receives SIZE_ACK, then B takes control with A's observed epoch and explicitly sends its own differently tagged size command. Assert B's rejected input creates no file; A's accepted shell command `stty size > size.txt; printf SIZE_ACK` records **24 80** before takeover and B's accepted command records **30 100** afterward. Wait for matching SIZE_ACK and file contents, using unique marker/file names per step. A's subsequent stale input must not create its marker file.
- [ ] Define server-local `InputCompletion<'a> { state: &'a ServerState, session: SessionId, token: u64 }` with Drop calling `finish_input(self.state, self.session, self.token)`. Reserve input before constructing this guard, then write without locks. The essential boundary is:

```rust
// Inside begin_input, with dashboards locked and validated selected revision:
if control.state.holder != Some(id) || control.state.epoch != epoch
    || control.in_flight.is_some() {
    return Err(lifecycle_error(ErrorCode::Conflict, "control changed or input finishing"));
}
control.in_flight = Some(token);
control.state.input_in_flight = true;
// Drop locks before session.write(&bytes); finish_input clears matching token only.
```

- [ ] Use a cfg(test) channel latch immediately after token reservation and before Session::write to exercise the race deterministically. While latched, require B takeover to return Conflict, disconnect A, and require claim to remain Conflict. Release the latch, observe accepted bytes and completion, then acquire B and prove no A bytes arrive after B's success. A delayed completion with the old token cannot clear a newer token.
- [ ] In the real blocked-child fixture, send enough input to backpressure `write_all`; use the reservation latch/observable server token to establish admission, not a sleep. Verify another dashboard on another PTY exchanges an acknowledgement and CLI List/Kill succeed. Keep existing raw reader/parser paths running throughout.
- [ ] Controller resize/takeover invokes existing Session::resize only after validation; commit holder/epoch after ioctl succeeds, then reset all subscribers. Observer resize only changes its slot and returns the current screen. Validate all targeted sessions before changing selection; an invalid target preserves the old lease and subscription.
- [ ] Run `rtk proxy cargo test -p ovrcr --test server_lifecycle multiple_dashboards_ -- --nocapture`: now **4 passed**. Run `rtk proxy cargo test -p ovrcr --test server_lifecycle backpressured_input_and_send_do_not_block_inspect_or_kill -- --exact --nocapture`: **1 passed**.

### Task 4: Preserve slow-reader isolation and bounded shutdown

**Files:** `crates/ovrcr-runtime/src/server/{mod,connections,dispatch,outbound}.rs`, `tests/server_lifecycle.rs`.
**Consumes/produces:** Per-client sinks and per-session epoch → isolated resynchronization, byte-accounted queues, complete teardown.

- [ ] Add `multiple_dashboards_slow_reader_does_not_stall_peer` and `multiple_dashboards_shutdown_closes_every_connection`. Extend the existing finite-burst test with two viewers of one session; force one socket's send buffer small and stop reading it, while a reader thread continuously consumes the healthy peer.
- [ ] Keep 64 messages per sink and add **4 MiB of encoded queued payload per dashboard**, plus at most one in-flight frame (≤1 MiB). Account Bincode encoded size before enqueue with the bounded counting writer below; oversized frames fail only that connection. Incremental overflow purges that client's superseded output and schedules one dirty key; reliable-response overflow closes only that client. Dequeue/purge/close subtract byte charges exactly; no shared queue or unbounded staging list.

```rust
const DASHBOARD_QUEUE_BYTES: usize = 4 * 1024 * 1024;
struct FrameCharge(usize);
impl std::io::Write for FrameCharge {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > crate::protocol::MAX_FRAME_BYTES.saturating_sub(self.0) {
            return Err(std::io::Error::other("frame too large"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn encoded_frame_charge(message: &ServerMessage) -> anyhow::Result<usize> {
    let mut charge = FrameCharge(0);
    bincode::serde::encode_into_std_write(message, &mut charge,
        bincode::config::standard())?;
    Ok(charge.0)
}
// In DashboardSink::enqueue, before taking its queue lock:
let Ok(charge) = encoded_frame_charge(&outbound.message) else {
    self.close();
    return false;
};
let mut queue = self.queue.lock().unwrap();
let full = queue.messages.len() == DASHBOARD_QUEUE
    || charge > DASHBOARD_QUEUE_BYTES.saturating_sub(queue.bytes);
// `full` enters the existing incremental-dirty / reliable-disconnect branches.
```

- [ ] Carry the charged encoded length beside each DashboardOutbound; serialization scratch is one bounded frame, not cached copies of all frames. Set dashboard socket write timeout to two seconds. A timed-out partial frame closes the socket; never resume with another frame. A short stall recovers via dirty notification, and a longer stall disconnects only that dashboard.
- [ ] Assert healthy peer receives the final burst marker after output stops; stalled peer either receives exactly one dirty notification followed by a matching fresh screen, or EOF after timeout. Separately force queue saturation in a unit test to prove dirty recovery without relying on kernel buffer timing. Verify both peers still get hierarchy changes where connected.
- [ ] Shutdown marks stopping under mutation_lock before termination and rejects later input/claims/mutations; on partial termination failure restore the available state and return PartialFailure. Reject ordinary shutdown with retained sessions before setting stopping. No input completion needs mutation_lock, so a blocked admitted write stays killable. Deliver the requester's acknowledgement through its writer with a **three-second completion deadline**, then close all dashboard sockets whether acknowledgement succeeded or timed out; preserve the existing disconnected-requester wakeup. Reader handlers must join their writers. Add `active_dashboard_handlers: Mutex<usize>` and `dashboard_handlers_changed: Condvar` to ServerState; increment upon admitted hello and decrement/notify from a reader scope guard after writer join. Wait at most three seconds for zero after terminating sessions and closing sockets, returning a teardown error instead of claiming clean shutdown on timeout.
- [ ] Verify a healthy shutdown requester succeeds despite another blocked writer; also test the blocked dashboard as requester. Require socket removal, every dashboard EOF, no managed PGIDs, and zero active dashboard handlers. Run `rtk proxy cargo test -p ovrcr --test server_lifecycle multiple_dashboards_ -- --nocapture`: now **6 root integration tests passed**. Add unit `multi_dashboard_queue_bytes_and_epoch_recovery` in `crates/ovrcr-runtime/src/server/tests.rs`; run `rtk proxy cargo test -p ovrcr-runtime --lib server::tests::multi_dashboard_queue_bytes_and_epoch_recovery -- --exact --nocapture` and require **1 runtime unit test passed**.

### Task 5: Show ownership and make observer rendering truthful

**Files:** `crates/ovrcr-tui/src/dashboard/{mod,state,input,render,event_loop}.rs`, `tests/tui.rs`.
**Consumes/produces:** DashboardReady and control/screen revisions → independent local viewport, truthful indicators, gated input and explicit takeover.

- [ ] Add Dashboard fields `id: Option<DashboardId>`, `view_revision: u64`, `control: Option<ControlState>`, `ready: bool`; keep `pane_size` as desired local geometry. The parser's dimensions come exclusively from authoritative Screen messages. `select_request`/`resize_request` advance revision and clear ready; they never resize the parser optimistically.
- [ ] Add `multiple_dashboards_observer_cannot_forward_input` and `multiple_dashboards_observer_clips_without_reflow` in `tests/tui.rs`. Use existing dashboard_fixture and TestBackend, with actual control screen 80×24 in a 40×12 pane. Assert parser dimensions remain 80×24, visible cells match the top-left region, offscreen cursor is hidden, and `READ ONLY` plus `clipped` appears. Expand the local pane and assert padding rather than artificial wrapping.

```rust
let owns = self.control.as_ref().is_some_and(|c|
    matches!(c.holder, Some(id) if Some(id) == self.id));
let running = self.selected.and_then(|id| find_session(self, id))
    .is_some_and(|s| matches!(s.phase, SessionPhase::Running));
let can_input = owns && running && self.ready && self.mode == InputMode::Terminal;
// input_request returns None unless can_input, then uses current revision and epoch.
// Browse Enter changes mode only when owns && running && ready.
// Browse 't' emits TakeControl with revision and control.epoch; it never sends bytes.
```

- [ ] Gate both `event_action` and `event_to_request`, including paste; do not rely solely on key-action filtering. A newer ControlChanged revokes readiness and forces Browse; the subsequent matching Screen establishes the new parser. Ignore old-revision Screens and old-epoch Output/ScreenDirty, and surface stale takeover Conflict without automatically retrying.
- [ ] Keep existing redraw/input batching and terminal restoration. Run `rtk proxy cargo test -p ovrcr --test tui multiple_dashboards_ -- --nocapture`: **2 passed**. Add `multiple_dashboards_old_epoch_never_reenables_input`, exercising loss, stale Screen, stale dirty callback, then current Screen; the same command must then report **3 passed**.

### Task 6: Prove two real dashboards and close the feature boundary

**Files:** `tests/terminal_acceptance.rs`, `README.md`; integration fixture cleanup where needed. The executable under test is the root `ovrcr` package and the optional helper remains `src/gui.rs`/`src/bin/ovrcr-gui.rs`.
**Consumes/produces:** Complete protocol/server/TUI → real two-terminal acceptance and documented behavior.

- [ ] Add `multiple_dashboards_outer_pty_takeover_and_detach` using two `OuterDashboard::start(&fixture, PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })` calls against one AcceptanceFixture. Select the same shell: A shows CONTROL, B READ ONLY. B Enter/paste must produce no shell marker; B sends browse `t`, waits for CONTROL, enters terminal mode, and runs `stty size` with a tagged result. A must show READ ONLY before another input attempt. Resize A and prove B's child dimensions remain unchanged; resize B and prove both rendered screens reset to B's dimensions.
- [ ] Then select different sessions and require independent marker acknowledgements/dimensions. Detach A; B continues. Reattach A as a new dashboard ID with no inherited ownership; explicitly select/take control. Finally terminate through existing fixture cleanup and require socket/PGID disappearance. Use rendered markers and PTY acknowledgements with deadlines, not arbitrary sleeps.
- [ ] Run `rtk proxy cargo test -p ovrcr --test terminal_acceptance multiple_dashboards_outer_pty_takeover_and_detach -- --exact --nocapture`: **1 passed**. Run `rtk proxy cargo test -p ovrcr --test server_lifecycle fifty_sessions_survive_detach_and_leave_no_process_groups -- --exact --nocapture`: **1 passed**, preserving the 50-session gate.
- [ ] Run `rtk proxy cargo fmt --all -- --check` and `rtk proxy cargo test --workspace --all-targets --all-features`; record actual nonzero counts per binary. Run the optional helper regression once with `rtk proxy cargo test -p ovrcr --features gui --test gui -- --nocapture` and require a nonzero count. Do not claim graphical acceptance from headless tests; record the two-terminal walkthrough on macOS and Linux or explicitly state which platform remains unverified.
- [ ] Update README with the 8-dashboard ceiling, t takeover, read-only/clipping behavior, creation-size rule and in-flight-input limitation; check only this feature's roadmap item after acceptance. Follow the user's normal diary/review requirements at execution closeout; this planning task changes no README or diary.

## Peer-plan integration and dependencies

- **Standalone baseline:** This plan works against current Select/Resize APIs without implementing panes or history. The shared surfaces are `crates/ovrcr-protocol/src/wire.rs`, `crates/ovrcr-runtime/src/server/{mod,dispatch,outbound}.rs`, and `crates/ovrcr-tui/src/dashboard/{mod,state,input,render}.rs`; rebase and reconcile these interfaces before sequential implementation touches those files.
- **Split panes:** `plans/2026-09-05-split-panes.md` proposes `SetView { view: DashboardView { revision, panes: Vec<PaneTarget { session, size }>, focused } }`. Move its entire view/revision into each DashboardSlot; substitute per-client view membership for selected checks. Input and TakeControl require that client's focused session. A dashboard may control both visible sessions; changing pane focus does not release a still-visible lease, and removing a pane does. Same session in two dashboards still has one holder/size. Map this plan's Screen/Output/ScreenDirty revision to the existing view revision and add the shared control epoch; do not create two independent revision counters. Observer PaneTarget sizes describe local viewports only; they never resize another holder's PTY. SetView validates all targets, then applies only owned/free-session geometry and reports authoritative snapshots for observers.
- **History/copy:** `plans/2026-09-05-historical-scrollback.md` holds one immutable snapshot per dashboard connection. Store it in that DashboardSlot, validate ID ownership on every history operation, and release only that client's snapshot on disconnect/session change. Observers may read history/copy; only the controller may resize the live PTY. Frozen snapshots retain their original dimensions through takeover. Copy mode must obey the same input gate and may share the one snapshot; it cannot create unbounded additional clones.
- **Per-client resource statement:** Without history, each server connection has ≤64 queued frames/4 MiB payload, one in-flight frame ≤1 MiB, one reader request ≤1 MiB, and bounded selection/dirty metadata; with 8 clients the queued-plus-in-flight payload ceiling is 40 MiB, excluding request buffers, serialization scratch, threads and sockets. History adds at most one frozen screen/history clone **per connection**, thus at most 8 clones. Its 512-row retention bound is not an absolute byte bound: dimensions and retained widths govern clone memory, as the history plan explains. Do not advertise a total process-memory ceiling without a separate geometry policy.

## Acceptance, risks, and review gate

Acceptance requires independent selections/responses, healthy-peer output under slow-reader pressure, real kernel-size evidence on both sides of takeover, rejected observer/stale input with absent external markers, idempotent old-ID cleanup, bounded shutdown, truthful read-only/clipped rendering, and unchanged 50-session cleanup. Require the **6 integration + 3 TUI + 1 outer-PTY** tests named above to execute; unit and existing regression counts are recorded separately.

The principal deliberate limitation is rejected takeover while an accepted PTY write is unfinished. Automatic controller election, cancellation of partially accepted input, arbitrary observer panning, and cross-version compatibility are separate work. If immediate force takeover becomes a requirement, revisit cancellable input delivery explicitly rather than weakening the successful-takeover boundary. Socket timeouts may disconnect a temporarily stalled dashboard; reconnect leaves its PTYs intact. Eight snapshots can materially amplify history memory, so measure at the history plan's supported test geometries.

Self-review before execution: reconcile final peer interfaces; verify every input route uses identity/revision/epoch; check snapshot-before-output and per-client dirty keys; inspect every creation/cleanup singleton call site; and retain actual test counts and platform limitations. No implementation or tests were executed to author this proposal.
