# Deep Modules Implementation Plan (architecture review candidates 1–6)

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Workers implement; a different reviewer checks each committed unit. Merge requires Kyle's authorization.

**Goal:** Deepen six shallow seams found by the 2026-09-12 architecture review so that request pairing, the server's active dashboard, the TUI view handshake, pane retargeting, outbound requests, and agent readiness each live behind one small interface with one test surface, and fix the three live defects those seams hide.

**Architecture:** Three phases that compose on one branch. Phase A adds a request/response module to `ovrcr-protocol` and moves every root-crate caller onto it (fixes the `count_sessions` List/Inventory mismatch). Phase B folds the four dashboard fields and nine free functions in `ovrcr-runtime::server` into one `ActiveDashboard` module with a single owner check and a single teardown. Phase C, inside `ovrcr-tui::dashboard`, adds `ready.rs` (one readiness predicate), `outbox.rs` (one drain, fixes the dropped `HistoryEnd`), `retarget(PaneChange)` (one ritual, fixes the unreleased capture on empty selection), and `view_handshake.rs` (readiness derived, not written). No new crates or dependencies.

**Tech Stack:** Existing Rust 2024 workspace (`anyhow`, `bincode` frames, `std::sync::mpsc`, `ratatui`, `vt100`). Tests: `cargo test -p <crate>` per task, `just verify` at phase ends.

**Spec:** the review report `$TMPDIR/architecture-review-20260912-212458.html` (candidates 1–6) and the domain glossary [CONTEXT.md](../CONTEXT.md). Vocabulary: module, interface, seam, adapter, depth, leverage, locality (codebase-design skill).

## Global constraints

- One server owner, one active dashboard, fifty sessions (CONTEXT.md). Behavior of the wire protocol does not change; `PROTOCOL_VERSION` stays 7 (no wire type is added, removed, or reordered).
- Every invariant in [docs/development/tui.md](../docs/development/tui.md) is preserved: revoke input on focus/assignment/geometry change; match snapshots by request, revision, and session; one view request in flight; hidden panes keep assignments; History/Copy tied to the captured session; mouse cleanup precedes the replacement `SetView`.
- Never weaken an assertion to get green. A test that changes expectation must cite the intended behavior change in its commit message.
- Test the real entry point (`docs/development/testing.md`): new modules get unit tests through their interface **and** the existing suites keep running through `handle_server_message`, `handle_connection`, and `dashboard_loop`.
- Work on an isolated worktree branch `refactor/deep-modules` from current `main` (b028d35 or later). One commit per task. Do not push, open, or merge a PR without Kyle's say-so.
- Commands go through `rtk` (hook rewrites them). Use `CARGO_INCREMENTAL=0` if disk is constrained.
- Docs, CLI help, and CONTEXT.md update in the same task that changes the behavior or names a concept.

## Execution order

| Order | Task | Candidate | Depends on |
| --- | --- | --- | --- |
| 1 | `ovrcr_protocol::client` request pairing | 1 | — |
| 2 | Fix `count_sessions`, migrate `src/service.rs`, fix the fake server | 1 | 1 |
| 3 | Migrate `src/cli/mod.rs`, `src/task_cli.rs`, `src/report.rs` | 1 | 1 |
| 4 | Migrate test request helpers | 1 | 1 |
| 5 | `ActiveDashboard` module (fields + constructors) | 2 | — |
| 6 | Route every owner check and send through `ActiveDashboard`; delete free functions | 2 | 5 |
| 7 | Server tests assert on the authority | 2 | 6 |
| 8 | `ready.rs`: one readiness predicate | 6 | — |
| 9 | `outbox.rs`: one drain | 5 | — |
| 10 | `retarget(PaneChange)` | 4 | 9 |
| 11 | `view_handshake.rs`: readiness derived | 3 | 10 |
| 12 | Docs, CONTEXT.md, final verification | all | 1–11 |

Phases A (1–4), B (5–7), and C (8–11) are independent of each other and may run in parallel worktrees, merging into the branch in that order. Tasks inside a phase are sequential.

---

## Phase A — pair request with response at the socket seam

### Task 1: `ovrcr_protocol::client`

**Files:**
- Create: `crates/ovrcr-protocol/src/client.rs`
- Modify: `crates/ovrcr-protocol/src/lib.rs` (add `pub mod client;`)

**Interfaces:**
- Produces:
  - `pub struct ServerError { pub code: ErrorCode, pub message: String }` (`Display`, `std::error::Error`)
  - `pub fn request<S: Read + Write>(io: &mut S, request_id: u64, request: Request) -> anyhow::Result<Response>` — writes one `ClientMessage`, then reads frames until the first `ServerMessage::Response` whose `request_id` matches, skipping `Event`s and other ids. Returns the raw `Response` (including `Response::Error`).
  - `pub fn expect_ok(response: Response) -> anyhow::Result<()>` — `Ok` → `Ok(())`, `Error{code,message}` → `Err(ServerError)`, anything else → `Err("unexpected server response …")`.
  - `pub fn list<S>(io, request_id) -> anyhow::Result<HierarchySnapshot>`
  - `pub fn inspect<S>(io, request_id) -> anyhow::Result<(Registry, Vec<SessionSummary>)>`
  - `pub fn shutdown<S>(io, request_id, kill: bool) -> anyhow::Result<()>`
  - `pub fn session_count(snapshot: &HierarchySnapshot) -> usize`

- [ ] **Step 1: Write the failing tests** (bottom of the new file)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMessage, ServerEvent, ServerMessage, SessionId};
    use std::os::unix::net::UnixStream;
    use std::thread;

    /// Answer one request on `server` with the frames in `reply`, in order.
    fn responder(mut server: UnixStream, reply: Vec<ServerMessage>) -> thread::JoinHandle<ClientMessage> {
        thread::spawn(move || {
            let received = crate::read_frame::<ClientMessage>(&mut server).unwrap();
            for frame in reply {
                crate::write_frame(&mut server, &frame).unwrap();
            }
            received
        })
    }

    #[test]
    fn request_skips_events_and_foreign_ids() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let handle = responder(server, vec![
            ServerMessage::Event(ServerEvent::ScreenDirty { session: SessionId(1), revision: 1 }),
            ServerMessage::Response { request_id: 99, response: Response::Ok },
            ServerMessage::Response { request_id: 7, response: Response::Hierarchy(HierarchySnapshot { projects: Vec::new() }) },
        ]);
        let response = request(&mut client, 7, Request::List).unwrap();
        assert_eq!(response, Response::Hierarchy(HierarchySnapshot { projects: Vec::new() }));
        assert_eq!(handle.join().unwrap(), ClientMessage { request_id: 7, request: Request::List });
    }

    #[test]
    fn list_returns_hierarchy_and_rejects_inventory() {
        let (mut client, server) = UnixStream::pair().unwrap();
        responder(server, vec![ServerMessage::Response {
            request_id: 1,
            response: Response::Inventory { registry: Registry::default(), sessions: Vec::new() },
        }]);
        let error = list(&mut client, 1).unwrap_err().to_string();
        assert!(error.contains("unexpected server response"), "{error}");
    }

    #[test]
    fn expect_ok_surfaces_server_error_with_code() {
        let error = expect_ok(Response::Error { code: ErrorCode::SessionsRemain, message: "sessions remain".into() }).unwrap_err();
        let server = error.downcast_ref::<ServerError>().expect("ServerError");
        assert_eq!(server.code, ErrorCode::SessionsRemain);
        assert_eq!(server.message, "sessions remain");
        assert!(expect_ok(Response::Ok).is_ok());
    }

    #[test]
    fn peer_close_before_response_is_an_error() {
        let (mut client, server) = UnixStream::pair().unwrap();
        drop(server);
        assert!(request(&mut client, 1, Request::List).is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ovrcr-protocol client::` — Expected: compile error, `client` module does not exist.

- [ ] **Step 3: Write the module**

```rust
//! One request, one matched response. The only place a caller learns request
//! ids, event interleaving, and which `Response` variant answers a `Request`.
use crate::{
    ClientMessage, ErrorCode, HierarchySnapshot, Registry, Request, Response, ServerMessage,
    SessionSummary, read_frame, write_frame,
};
use anyhow::{Result, bail};
use std::io::{Read, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerError {
    pub code: ErrorCode,
    pub message: String,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServerError {}

/// Send `request` under `request_id` and return the first response frame that
/// carries that id. Events and responses to other ids are skipped.
pub fn request<S: Read + Write>(io: &mut S, request_id: u64, request: Request) -> Result<Response> {
    write_frame(io, &ClientMessage { request_id, request })?;
    loop {
        match read_frame::<ServerMessage>(io)? {
            ServerMessage::Response { request_id: id, response } if id == request_id => {
                return Ok(response);
            }
            ServerMessage::Response { .. } | ServerMessage::Event(_) => {}
        }
    }
}

pub fn expect_ok(response: Response) -> Result<()> {
    match response {
        Response::Ok => Ok(()),
        Response::Error { code, message } => Err(ServerError { code, message }.into()),
        other => bail!("unexpected server response: {other:?}"),
    }
}

pub fn list<S: Read + Write>(io: &mut S, request_id: u64) -> Result<HierarchySnapshot> {
    match request(io, request_id, Request::List)? {
        Response::Hierarchy(snapshot) => Ok(snapshot),
        Response::Error { code, message } => Err(ServerError { code, message }.into()),
        other => bail!("unexpected server response: {other:?}"),
    }
}

pub fn inspect<S: Read + Write>(io: &mut S, request_id: u64) -> Result<(Registry, Vec<SessionSummary>)> {
    match request(io, request_id, Request::Inspect)? {
        Response::Inventory { registry, sessions } => Ok((registry, sessions)),
        Response::Error { code, message } => Err(ServerError { code, message }.into()),
        other => bail!("unexpected server response: {other:?}"),
    }
}

pub fn shutdown<S: Read + Write>(io: &mut S, request_id: u64, kill: bool) -> Result<()> {
    expect_ok(request(io, request_id, Request::Shutdown { kill })?)
}

pub fn session_count(snapshot: &HierarchySnapshot) -> usize {
    snapshot
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .map(|workspace| workspace.sessions.len())
        .sum()
}
```

In `lib.rs` add `pub mod client;` after `mod codec;`.

- [ ] **Step 4: Run tests** — `cargo test -p ovrcr-protocol client::` Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ovrcr-protocol/src/client.rs crates/ovrcr-protocol/src/lib.rs
git commit -m "feat(protocol): pair requests with responses in one client module"
```

### Task 2: fix `count_sessions` and the fake server that hid it

**Files:**
- Modify: `src/service.rs:281-300` (`install`), `:483-527` (`graceful_shutdown`, `request_shutdown`, `count_sessions`)
- Modify: `tests/service.rs:368` (fake server answers `List`)

**Interfaces:**
- Consumes: `ovrcr::protocol::client::{list, session_count, shutdown, ServerError}`.

- [ ] **Step 1: Make the existing test honest (red)**

In `tests/service.rs` the fake server answers `Request::List` with `Response::Inventory`, which the real server never does (`connections.rs:410` answers `Hierarchy`). Replace that arm:

```rust
                Request::List => Response::Hierarchy(HierarchySnapshot {
                    projects: vec![ProjectSummary {
                        name: "demo".into(),
                        workspaces: vec![WorkspaceSummary {
                            project: "demo".into(),
                            name: "main".into(),
                            path: "/tmp/demo-main".into(),
                            sessions: vec![SessionSummary {
                                id: SessionId(7),
                                project: "demo".into(),
                                workspace: "main".into(),
                                name: "shell".into(),
                                label: "shell".into(),
                                pid: Some(1),
                                started_unix_ms: 0,
                                phase: SessionPhase::Running,
                                activity: AgentActivity::Idle,
                                context_usage: None,
                                agent: None,
                                agent_epoch: 0,
                                unread: None,
                            }],
                        }],
                    }],
                }),
```

Add `HierarchySnapshot, ProjectSummary, WorkspaceSummary` to that test's `use ovrcr::protocol::{…}` and drop `Registry`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ovrcr --test service service_install_refuses_when_sessions_exist` — Expected: FAIL, error text contains `unexpected OVRCR session list response` instead of `1 session`.

- [ ] **Step 3: Rewrite the three helpers on the module**

```rust
fn graceful_shutdown(stream: &mut UnixStream) -> Result<()> {
    client::shutdown(stream, 1, true).map_err(|error| match error.downcast_ref::<client::ServerError>() {
        Some(refused) => anyhow!("OVRCR refused shutdown: {}", refused.message),
        None => error.context("request graceful OVRCR shutdown"),
    })
}

fn count_sessions(config: &ServiceConfig) -> Result<usize> {
    let mut stream = connect_if_running(&config.server_paths)?
        .context("OVRCR server stopped while refusing shutdown")?;
    let snapshot = client::list(&mut stream, 1).context("request OVRCR session list")?;
    Ok(client::session_count(&snapshot))
}
```

and in `install`, replace the `match request_shutdown(&mut stream, kill_sessions)? { … }` block with:

```rust
    if let Some(mut stream) = connection {
        if let Err(error) = client::shutdown(&mut stream, 1, kill_sessions) {
            match error.downcast_ref::<client::ServerError>() {
                Some(refused) if refused.code == ErrorCode::SessionsRemain => {
                    let sessions = count_sessions(config)?;
                    bail!(
                        "OVRCR server refused shutdown ({}): {sessions} session(s) open; \
                         close them or rerun install with --kill-sessions",
                        refused.message
                    );
                }
                Some(refused) => bail!("OVRCR refused shutdown: {}", refused.message),
                None => return Err(error.context("request graceful OVRCR shutdown")),
            }
        }
    }
```

Delete `request_shutdown`. Imports: `use crate::protocol::client;` and drop `ClientMessage, Request, Response, ServerMessage, read_frame, write_frame` from the `use` if now unused (`cargo check` tells you).

- [ ] **Step 4: Run** — `cargo test -p ovrcr --test service` Expected: all pass, including the refusal test now asserting `1 session`.

- [ ] **Step 5: Commit**

```bash
git add src/service.rs tests/service.rs
git commit -m "fix(service): count sessions from the Hierarchy the server actually sends

Request::List is answered with Response::Hierarchy; count_sessions matched
Inventory and the fake server in tests/service.rs agreed with the bug."
```

### Task 3: root-crate callers use the module

**Files:**
- Modify: `src/cli/mod.rs:225-262` (`send_request`)
- Modify: `src/task_cli.rs:282-312` (`request_raw`)
- Modify: `src/report.rs:685-706` (`agent_exchange`), `:140-165` (`send_payload` frame exchange)

- [ ] **Step 1: `send_request` keeps its timeouts and delegates**

Replace the body after the two `set_*_timeout` calls with:

```rust
    client::request(stream, 1, request).map_err(|error| {
        let timed_out = error.chain().any(|cause| {
            cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
                matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
            })
        });
        if timed_out {
            anyhow::anyhow!(
                "timed out after {timeout:?} waiting for the server's response; the server may be wedged, see its log"
            )
        } else {
            error
        }
    })
```

The old `ServerMessage::Event(_) => bail!("server sent an event before the response")` arm is gone: control connections never receive events, and the module skips them by contract.

- [ ] **Step 2: `request_raw` in task_cli.rs**

```rust
    match client::request(&mut connection, 1, Request::Task(Box::new(req)))? {
        Response::Task(value) => Ok(*value),
        Response::Error { code, message } => bail!("{code:?}: {message}"),
        _ => bail!("unexpected task response; update and restart the OVRCR server"),
    }
```

- [ ] **Step 3: `agent_exchange` and `send_payload` in report.rs**

```rust
fn agent_exchange(stream: &mut UnixStream, request_id: u64, request: Request, deadline: Instant) -> Result<Response> {
    client::request(&mut DeadlineIo::new(stream, deadline), request_id, request)
        .map_err(|_| anyhow::anyhow!("invalid supervisor response"))
}
```

In `send_payload`, replace the `write_frame … read_frame … match response` block with:

```rust
    let response = client::request(&mut io, 1, Request::AgentReport(AgentReport { session: identity.session, capability: identity.capability, sequence, update }))
        .map_err(|error| map_transport_error(&error))?;
    match response {
        Response::Ok => Ok(()),
        Response::Error { code, .. } => Err(report_error(code, ReportFailure::Unavailable)),
        _ => Err(report_error(ErrorCode::Internal, ReportFailure::Unavailable)),
    }
```

`map_transport_error` still sees the io error chain because `client::request` wraps `read_frame`/`write_frame` errors unchanged.

- [ ] **Step 4: Run** — `cargo test -p ovrcr --test cli --test agent_setup --test claude_reporting --test codex_reporting` and `cargo test -p ovrcr --lib`. Expected: pass; zero tests filtered out is not a pass, check counts.

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/task_cli.rs src/report.rs
git commit -m "refactor: route CLI, task, and report requests through protocol::client"
```

### Task 4: test helpers stop re-implementing the protocol

**Files:**
- Modify: `tests/server_lifecycle.rs:207-219` (`ServerFixture::request`), `:2574-2590` (`request_with_timeout`), `:2650-2665` (`dashboard_request`), `:3755-3772` (`request_on_stream`), `:5332-5346` (`ControlFixture::request`)
- Modify: `tests/cli.rs:1240-1260` (the `request` helper around line 1245)

Leave `dashboard_select` (it waits for `Screen` then `Ok` under one id, a dashboard-only shape).

- [ ] **Step 1: Replace each body with one call**

`ServerFixture::request` returns `ServerMessage` today and callers match on it; keep the signature and wrap: `ServerMessage::Response { request_id: 1, response: client::request(&mut stream, 1, request).unwrap() }`. The other four return `Response` or `Option<Response>`; their bodies become `client::request(stream, request_id, request)` plus the existing `.unwrap()` / `.ok()?` and timeout setup. `dashboard_request`'s manual id-matching loop is exactly what the module does; its body becomes `client::request(stream, request_id, request).unwrap()`.

- [ ] **Step 2: Run** — `cargo test -p ovrcr --test server_lifecycle` (long; PTY permission required) and `cargo test -p ovrcr --test cli`. Expected: same pass counts as before the change (record them in the commit message).

- [ ] **Step 3: Commit**

```bash
git add tests/server_lifecycle.rs tests/cli.rs
git commit -m "test: use protocol::client for request round trips"
```


---

## Phase B — one `ActiveDashboard` module in the runtime server

Today "one active dashboard" is four `ServerState` fields (`view`, `dashboard`, `dashboard_size`, `dashboard_slot`), nine free functions in `outbound.rs` taking `&ServerState`, fifteen open-coded `Arc::ptr_eq` owner checks across three files, and two teardown bodies (`DashboardOwnership::drop` in `connections.rs:22` and `disconnect_dashboard` in `outbound.rs:531`). `ServerState::dashboard` has zero production readers and fourteen test readers. Tasks 5–7 replace all of that with one type whose interface is `claim / owns / send* / with_owned / view / publish_view / clear_view* / geometry / forget_session / disconnect / release`.

### Task 5: the module and the field

**Files:**
- Create: `crates/ovrcr-runtime/src/server/dashboard.rs`
- Modify: `crates/ovrcr-runtime/src/server/mod.rs:174-195` (`ServerState` fields), `:44-50` (imports), `:470-505` (`with_tasks_for_test`), `:197-200` (`DashboardGeometry` struct: delete)
- Modify: `crates/ovrcr-runtime/src/server/outbound.rs:401-415` (move `DashboardSlot`, `DashboardSnapshot` out)
- Modify: `crates/ovrcr-runtime/src/server/startup.rs:185-206` (state literal)
- Modify: `crates/ovrcr-runtime/src/server/tests.rs:2373-2440`, `:4574-4600`, `:4781-4810` (state literals)

**Interfaces:**
- Produces (all `pub(super)` unless noted):

```rust
pub struct ActiveDashboard { /* private */ }
impl Default for ActiveDashboard
fn claim(&self, sink: Arc<DashboardSink>, stream: UnixStream) -> Option<Arc<()>>  // None when occupied
fn is_claimed(&self) -> bool
fn owns(&self, owner: &Arc<()>) -> bool            // identity matches AND sink not closing
fn snapshot(&self) -> Option<DashboardSnapshot>     // whoever holds the slot
fn snapshot_owned(&self, owner: &Arc<()>) -> Option<DashboardSnapshot>
fn try_send(&self, message: ServerMessage) -> bool
fn send(&self, message: ServerMessage, completion: Option<SyncSender<Result<(), String>>>) -> bool
fn send_owner(&self, owner: &Arc<()>, message: ServerMessage) -> bool
fn send_owner_with_completion(&self, owner, message, completion) -> bool
fn send_owner_terminal(&self, owner, message, completion) -> bool
fn with_owned<R>(&self, owner: &Arc<()>, f: impl FnOnce(&mut DashboardSlot) -> R) -> Option<R>
fn view(&self) -> Option<DashboardView>
fn next_revision(&self) -> u64
fn publish_view(&self, owner, published: DashboardView, focus_changed: bool, before_publish: impl FnOnce()) -> bool
fn clear_view(&self, fallback_revision: Option<u64>)
fn clear_view_for(&self, owner: &Arc<()>, fallback_revision: Option<u64>)
fn geometry(&self) -> Option<TerminalSize>
fn forget_session(&self, id: SessionId)
fn disconnect(&self, snapshot: DashboardSnapshot)
fn release(&self, owner: &Arc<()>)
fn spawn_writer(state: Arc<ServerState>, sink: Arc<DashboardSink>, identity: Arc<()>, stream: UnixStream) -> io::Result<JoinHandle<()>>
#[cfg(test)] fn claim_with_identity(&self, sink, identity: Arc<()>, stream) 
#[cfg(test)] fn install_view_for_test(&self, view: Option<DashboardView>)
#[cfg(test)] fn slot_for_test(&self) -> MutexGuard<'_, Option<DashboardSlot>>
```

- [ ] **Step 1: Write the failing unit tests** (in `dashboard.rs`, `#[cfg(test)] mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (UnixStream, UnixStream) { UnixStream::pair().unwrap() }

    #[test]
    fn claim_is_exclusive_until_release() {
        let dashboard = ActiveDashboard::default();
        let (_a, stream_a) = pair();
        let owner = dashboard.claim(DashboardSink::new(), stream_a).expect("first claim");
        let (_b, stream_b) = pair();
        assert!(dashboard.claim(DashboardSink::new(), stream_b).is_none());
        assert!(dashboard.owns(&owner));
        assert!(!dashboard.owns(&Arc::new(())));
        dashboard.release(&owner);
        assert!(!dashboard.is_claimed());
        assert!(!dashboard.owns(&owner));
    }

    #[test]
    fn release_by_a_stale_owner_leaves_the_replacement_untouched() {
        let dashboard = ActiveDashboard::default();
        let (_a, stream_a) = pair();
        let old = dashboard.claim(DashboardSink::new(), stream_a).unwrap();
        dashboard.release(&old);
        let (_b, stream_b) = pair();
        let new = dashboard.claim(DashboardSink::new(), stream_b).unwrap();
        dashboard.install_view_for_test(Some(DashboardView { revision: 3, panes: Vec::new(), focused: None }));
        dashboard.release(&old);
        assert!(dashboard.owns(&new));
        assert_eq!(dashboard.view().map(|view| view.revision), Some(3));
    }

    #[test]
    fn disconnect_and_release_clear_the_same_state() {
        for use_disconnect in [true, false] {
            let dashboard = ActiveDashboard::default();
            let (_client, server) = pair();
            let owner = dashboard.claim(DashboardSink::new(), server).unwrap();
            assert!(dashboard.publish_view(&owner, DashboardView {
                revision: 1,
                panes: vec![PaneTarget { session: SessionId(1), size: TerminalSize { rows: 5, cols: 7 } }],
                focused: Some(SessionId(1)),
            }, false, || {}));
            assert_eq!(dashboard.geometry(), Some(TerminalSize { rows: 5, cols: 7 }));
            if use_disconnect {
                let snapshot = dashboard.snapshot_owned(&owner).unwrap();
                dashboard.disconnect(snapshot);
            } else {
                dashboard.release(&owner);
            }
            assert!(!dashboard.is_claimed());
            assert!(dashboard.view().is_none());
            assert!(dashboard.geometry().is_none());
        }
    }

    #[test]
    fn owns_is_false_once_the_sink_is_closing() {
        let dashboard = ActiveDashboard::default();
        let (_client, server) = pair();
        let sink = DashboardSink::new();
        let owner = dashboard.claim(Arc::clone(&sink), server).unwrap();
        assert!(dashboard.send_owner_terminal(&owner, ServerMessage::Response { request_id: 1, response: Response::Ok }, None));
        assert!(!dashboard.owns(&owner));
        assert!(!dashboard.publish_view(&owner, DashboardView { revision: 1, panes: Vec::new(), focused: None }, false, || {}));
        assert!(dashboard.with_owned(&owner, |_| ()).is_some(), "history requests still see the draining slot");
    }

    #[test]
    fn forget_session_drops_history_and_focused_view() {
        let dashboard = ActiveDashboard::default();
        let (_client, server) = pair();
        let owner = dashboard.claim(DashboardSink::new(), server).unwrap();
        dashboard.install_view_for_test(Some(DashboardView {
            revision: 2,
            panes: vec![PaneTarget { session: SessionId(1), size: TerminalSize { rows: 1, cols: 1 } }, PaneTarget { session: SessionId(2), size: TerminalSize { rows: 1, cols: 1 } }],
            focused: Some(SessionId(2)),
        }));
        dashboard.forget_session(SessionId(1));
        assert_eq!(dashboard.view().unwrap().panes.len(), 1);
        dashboard.forget_session(SessionId(2));
        let view = dashboard.view().unwrap();
        assert!(view.panes.is_empty() && view.focused.is_none() && view.revision == 2);
        assert!(dashboard.owns(&owner));
    }
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test -p ovrcr-runtime server::dashboard::` Expected: compile error, module missing.

- [ ] **Step 3: Write the module**

```rust
//! The server's one active dashboard: who holds it, what may be sent to it,
//! and the view it has acknowledged. Every owner check and every teardown is here.
use super::*;

pub(super) struct DashboardSlot {
    pub(super) sink: Arc<DashboardSink>,
    pub(super) identity: Arc<()>,
    pub(super) stream: UnixStream,
    pub(super) history: Option<ovrcr_terminal::history::FrozenHistory>,
    pub(super) next_history_id: u64,
}

pub(super) struct DashboardSnapshot {
    pub(super) sink: Arc<DashboardSink>,
    pub(super) identity: Arc<()>,
    pub(super) stream: UnixStream,
}

struct Geometry {
    owner: Arc<()>,
    size: TerminalSize,
}

#[derive(Default)]
pub struct ActiveDashboard {
    slot: Mutex<Option<DashboardSlot>>,
    view: Mutex<Option<DashboardView>>,
    geometry: Mutex<Option<Geometry>>,
}

impl ActiveDashboard {
    pub(super) fn claim(&self, sink: Arc<DashboardSink>, stream: UnixStream) -> Option<Arc<()>> {
        let identity = Arc::new(());
        let mut slot = self.slot.lock().unwrap();
        if slot.is_some() {
            return None;
        }
        *slot = Some(DashboardSlot { sink, identity: Arc::clone(&identity), stream, history: None, next_history_id: 1 });
        Some(identity)
    }

    pub(super) fn is_claimed(&self) -> bool {
        self.slot.lock().unwrap().is_some()
    }

    /// The owner still holds the slot and its queue is not draining a terminal frame.
    /// Publishing a view or resizing for a draining dashboard would admit input for a
    /// connection that is already going away.
    pub(super) fn owns(&self, owner: &Arc<()>) -> bool {
        self.slot.lock().unwrap().as_ref().is_some_and(|current| {
            Arc::ptr_eq(&current.identity, owner) && !current.sink.is_closing()
        })
    }

    pub(super) fn snapshot(&self) -> Option<DashboardSnapshot> {
        let slot = self.slot.lock().unwrap();
        let slot = slot.as_ref()?;
        Some(DashboardSnapshot { sink: Arc::clone(&slot.sink), identity: Arc::clone(&slot.identity), stream: slot.stream.try_clone().ok()? })
    }

    pub(super) fn snapshot_owned(&self, owner: &Arc<()>) -> Option<DashboardSnapshot> {
        self.snapshot().filter(|snapshot| Arc::ptr_eq(&snapshot.identity, owner))
    }

    /// Queue on a resolved dashboard, disconnecting only when the queue is closed. A
    /// draining queue still owes its client a terminal frame, so the socket stays open.
    fn queue(&self, snapshot: DashboardSnapshot, outbound: DashboardOutbound, terminal: bool) -> bool {
        let result = if terminal { snapshot.sink.enqueue_terminal(outbound) } else { snapshot.sink.enqueue(outbound) };
        match result {
            Enqueue::Queued => true,
            Enqueue::Draining => false,
            Enqueue::Closed => {
                self.disconnect(snapshot);
                false
            }
        }
    }

    pub(super) fn try_send(&self, message: ServerMessage) -> bool {
        self.send(message, None)
    }

    pub(super) fn send(&self, message: ServerMessage, completion: Option<SyncSender<Result<(), String>>>) -> bool {
        let Some(snapshot) = self.snapshot() else { return false };
        self.queue(snapshot, DashboardOutbound { message, completion }, false)
    }

    pub(super) fn send_owner(&self, owner: &Arc<()>, message: ServerMessage) -> bool {
        self.send_owner_with_completion(owner, message, None)
    }

    /// A response belongs to the dashboard that asked. A replacement owner must never
    /// receive an evicted connection's late answer under its own request id.
    pub(super) fn send_owner_with_completion(&self, owner: &Arc<()>, message: ServerMessage, completion: Option<SyncSender<Result<(), String>>>) -> bool {
        let Some(snapshot) = self.snapshot_owned(owner) else { return false };
        self.queue(snapshot, DashboardOutbound { message, completion }, false)
    }

    pub(super) fn send_owner_terminal(&self, owner: &Arc<()>, message: ServerMessage, completion: Option<SyncSender<Result<(), String>>>) -> bool {
        let Some(snapshot) = self.snapshot_owned(owner) else { return false };
        self.queue(snapshot, DashboardOutbound { message, completion }, true)
    }

    /// Locked access to the owned slot for history bookkeeping. `None` when `owner` no
    /// longer holds the dashboard. Callers that already hold `sessions` keep the
    /// sessions → slot lock order this preserves.
    pub(super) fn with_owned<R>(&self, owner: &Arc<()>, f: impl FnOnce(&mut DashboardSlot) -> R) -> Option<R> {
        let mut slot = self.slot.lock().unwrap();
        let current = slot.as_mut().filter(|current| Arc::ptr_eq(&current.identity, owner))?;
        Some(f(current))
    }

    pub(super) fn view(&self) -> Option<DashboardView> {
        self.view.lock().unwrap().clone()
    }

    pub(super) fn next_revision(&self) -> u64 {
        self.view.lock().unwrap().as_ref().map_or(1, |view| view.revision.saturating_add(1))
    }

    /// Publish `published` for `owner` and record the focused pane's geometry. False
    /// when the owner lost the slot or its queue is draining; nothing is published then.
    pub(super) fn publish_view(&self, owner: &Arc<()>, published: DashboardView, focus_changed: bool, before_publish: impl FnOnce()) -> bool {
        let mut slot = self.slot.lock().unwrap();
        let Some(current) = slot.as_mut().filter(|current| Arc::ptr_eq(&current.identity, owner) && !current.sink.is_closing()) else {
            return false;
        };
        before_publish();
        if focus_changed {
            current.history.take();
        }
        let focused_size = published.focused.and_then(|focused| published.panes.iter().find(|pane| pane.session == focused)).map(|pane| pane.size);
        *self.view.lock().unwrap() = Some(published);
        if let Some(size) = focused_size {
            *self.geometry.lock().unwrap() = Some(Geometry { owner: Arc::clone(owner), size });
        }
        true
    }

    pub(super) fn clear_view(&self, fallback_revision: Option<u64>) {
        let mut view = self.view.lock().unwrap();
        match view.as_mut() {
            Some(current) => {
                current.panes.clear();
                current.focused = None;
            }
            None => {
                if let Some(revision) = fallback_revision {
                    *view = Some(DashboardView { revision, panes: Vec::new(), focused: None });
                }
            }
        }
    }

    pub(super) fn clear_view_for(&self, owner: &Arc<()>, fallback_revision: Option<u64>) {
        let slot = self.slot.lock().unwrap();
        if slot.as_ref().is_some_and(|current| Arc::ptr_eq(&current.identity, owner)) {
            self.clear_view(fallback_revision);
        }
    }

    pub(super) fn geometry(&self) -> Option<TerminalSize> {
        self.geometry.lock().unwrap().as_ref().map(|geometry| geometry.size)
    }

    /// A removed session leaves no frozen history and no pane behind; if it was the
    /// focused pane the view is cleared so nothing admits input for it.
    pub(super) fn forget_session(&self, id: SessionId) {
        if let Some(slot) = self.slot.lock().unwrap().as_mut()
            && slot.history.as_ref().is_some_and(|history| history.opened().session == id)
        {
            slot.history.take();
        }
        let mut view = self.view.lock().unwrap();
        if view.as_ref().is_some_and(|current| current.focused == Some(id)) {
            drop(view);
            self.clear_view(None);
        } else if let Some(current) = view.as_mut() {
            current.panes.retain(|pane| pane.session != id);
        }
    }

    /// The one teardown. Removes `identity`'s slot if it still holds the dashboard and
    /// clears the view and geometry it owned. A stale identity changes nothing.
    fn vacate(&self, identity: &Arc<()>) {
        let mut slot = self.slot.lock().unwrap();
        if !slot.as_ref().is_some_and(|current| Arc::ptr_eq(&current.identity, identity)) {
            return;
        }
        if let Some(current) = slot.take() {
            current.sink.close();
        }
        *self.view.lock().unwrap() = None;
        let mut geometry = self.geometry.lock().unwrap();
        if geometry.as_ref().is_some_and(|current| Arc::ptr_eq(&current.owner, identity)) {
            *geometry = None;
        }
    }

    pub(super) fn disconnect(&self, snapshot: DashboardSnapshot) {
        let _ = snapshot.stream.shutdown(std::net::Shutdown::Both);
        snapshot.sink.close();
        self.vacate(&snapshot.identity);
    }

    pub(super) fn release(&self, owner: &Arc<()>) {
        self.vacate(owner);
    }

    #[cfg(test)]
    pub(super) fn claim_with_identity(&self, sink: Arc<DashboardSink>, identity: Arc<()>, stream: UnixStream) {
        *self.slot.lock().unwrap() = Some(DashboardSlot { sink, identity, stream, history: None, next_history_id: 1 });
    }

    #[cfg(test)]
    pub(super) fn install_view_for_test(&self, view: Option<DashboardView>) {
        *self.view.lock().unwrap() = view;
    }

    #[cfg(test)]
    pub(super) fn slot_for_test(&self) -> std::sync::MutexGuard<'_, Option<DashboardSlot>> {
        self.slot.lock().unwrap()
    }
}

/// The dashboard writer thread: pops deliveries, writes frames, and disconnects on the
/// first failed or terminal write. Moved verbatim from `handle_connection`.
pub(super) fn spawn_writer(state: Arc<ServerState>, sink: Arc<DashboardSink>, identity: Arc<()>, stream: UnixStream) -> io::Result<JoinHandle<()>> {
    let mut output = stream.try_clone().ok();
    let close_stream = stream;
    thread::Builder::new().name("ovrcr-dashboard-writer".into()).spawn(move || {
        while let Some(delivery) = sink.next() {
            let (message, completion, dirty, terminal) = match delivery {
                DashboardDelivery::Message(outbound) => (outbound.message, outbound.completion, None, false),
                DashboardDelivery::Terminal(outbound) => (outbound.message, outbound.completion, None, true),
                DashboardDelivery::Dirty { revision, session } => (
                    ServerMessage::Event(ServerEvent::ScreenDirty { session, revision }),
                    None,
                    Some((revision, session)),
                    false,
                ),
            };
            #[cfg(test)]
            if let Some(hook) = state.before_dashboard_write_hook.lock().unwrap().clone() {
                hook();
            }
            let result = match output.as_mut() {
                Some(stream) => write_frame(stream, &message).map_err(|error| error.to_string()),
                None => Err("dashboard writer stream unavailable".into()),
            };
            if result.is_ok() && let Some((revision, session)) = dirty {
                sink.dirty_sent(revision, session);
            }
            if let Some(completion) = completion {
                let _ = completion.send(result.clone());
            }
            if result.is_err() || terminal {
                sink.close();
                let _ = close_stream.shutdown(std::net::Shutdown::Both);
                state.dashboard.disconnect(DashboardSnapshot { sink, identity, stream: close_stream });
                break;
            }
        }
    })
}
```

Then:
- `mod.rs`: add `mod dashboard;` and `pub use dashboard::ActiveDashboard; use dashboard::{DashboardSlot, DashboardSnapshot, spawn_writer};`. In `ServerState` delete `view`, `dashboard`, `dashboard_size`, `dashboard_slot`; add `pub(super) dashboard: ActiveDashboard,`. Delete `pub struct DashboardGeometry`. Remove `DashboardSlot`/`DashboardSnapshot` from the `use outbound::{…}` list and the `Enqueue` cfg(test) import stays (the module uses it via `super::*`, so add `use outbound::Enqueue;` unconditionally and drop the `#[cfg(test)]` on it).
- `outbound.rs`: delete lines 401–415 (the two structs). Leave the nine free functions in place for this task; Task 6 deletes them after migrating callers. To compile now, make them thin wrappers: e.g. `pub(super) fn dashboard_snapshot(state: &ServerState) -> Option<DashboardSnapshot> { state.dashboard.snapshot() }`, `dashboard_owner_matches → state.dashboard.owns(owner)`, `dashboard_send → state.dashboard.send(...)`, `dashboard_send_owner* → state.dashboard.send_owner*`, `disconnect_dashboard → state.dashboard.disconnect(snapshot)`.
- `dispatch.rs`: `set_dashboard_geometry` / `clear_dashboard_geometry` are now inside `publish_view` / `vacate`; make them thin wrappers this task too (`clear_view_subscription → state.dashboard.clear_view(..)`, `clear_view_subscription_for_owner → clear_view_for`, delete the geometry pair and update `mod.rs` imports).
- Every literal `ServerState { … }` (startup.rs:185, mod.rs:470, tests.rs ×4) drops the four fields and gains `dashboard: ActiveDashboard::default(),`. In `test_state_with_dispatch`, after construction: `if let Some((identity, stream)) = stream { state.dashboard.claim_with_identity(dashboard.expect("sink"), identity, stream); }`.
- Direct field uses that must move now so it compiles: `mod.rs:388` → `self.dashboard.geometry().unwrap_or(TerminalSize { rows: 40, cols: 120 })`; `mod.rs:451-475` (`remove_session_locked` tail) → `self.dashboard.forget_session(id);`; `connections.rs:22-34` `Drop` body → `self.state.dashboard.release(&self.identity);`; `connections.rs:161-200` claim block → see Task 6 step 1 (do it now, it is the same edit); `connections.rs:591` → `state.dashboard.view()`; `dispatch.rs:550` → `state.dashboard.view()`; `dispatch.rs:684-707` → `if !state.dashboard.publish_view(owner, published, focus_changed, || { #[cfg(test)] if let Some(hook) = state.before_view_publish_hook.lock().unwrap().clone() { hook(); } }) { let _ = completion.send(DispatchCompletion::Complete); return; }` followed by `drop(registered); let _ = completion.send(DispatchCompletion::Complete);`. Tests that read `state.view.lock()` (27) become `state.dashboard.view()`; writes (9) become `state.dashboard.install_view_for_test(Some(..))`; `state.dashboard.lock()` reads (14) become `state.dashboard.is_claimed()` (or `owns(&identity)` where the test compares identities); `dashboard_size` reads (3) become `state.dashboard.geometry()`; `dashboard_slot` literal installs (8) become `state.dashboard.claim_with_identity(sink, owner, stream)`; the remaining `dashboard_slot` accessors become `state.dashboard.slot_for_test()` with the same chain.

- [ ] **Step 4: Run** — `cargo test -p ovrcr-runtime` Expected: all pass (new module tests plus the existing 48 server tests unchanged in behavior).

- [ ] **Step 5: Commit**

```bash
git add crates/ovrcr-runtime/src/server
git commit -m "refactor(server): own the active dashboard in one module"
```

### Task 6: route every owner check through the module; delete the free functions

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/connections.rs` (claim block `:161-283`, sends `:146-160`, `:310-357`, closing block `:376-384`)
- Modify: `crates/ovrcr-runtime/src/server/dispatch.rs:198-364` (history), `:484-502` (`view_error`), `:541-650` (`dispatch_set_view_with_resize`), `:713-760` (clear subscription wrappers)
- Modify: `crates/ovrcr-runtime/src/server/outbound.rs` (delete the wrappers left by Task 5)
- Modify: `crates/ovrcr-runtime/src/server/mod.rs` (imports)

- [ ] **Step 1: The claim block in `handle_connection`**

Replace lines 161–283 (from `let identity = Arc::new(());` through the `match spawned { … }`) with:

```rust
            let Ok(close_stream) = stream.try_clone() else { break };
            let Some(identity) = state.dashboard.claim(Arc::clone(&dashboard_sink), close_stream) else {
                let _ = send_direct(&mut stream, message.request_id, error_response(ErrorCode::Conflict, "another dashboard is already connected"));
                break;
            };
            #[cfg(feature = "acceptance-diagnostics")]
            if let Some(monitor) = &state.dashboard_monitor {
                monitor.register(&dashboard_sink);
            }
            ownership = Some(DashboardOwnership { state: Arc::clone(&state), identity: Arc::clone(&identity) });
            #[cfg(test)]
            if PANIC_AFTER_DASHBOARD_REGISTRATION.with(|armed| armed.replace(false)) {
                panic!("injected panic after dashboard registration");
            }
            role = ClientRole::Dashboard;
            let Ok(writer_stream) = stream.try_clone() else { break };
            match spawn_writer(Arc::clone(&state), Arc::clone(&dashboard_sink), Arc::clone(&identity), writer_stream) {
                Ok(handle) => writer = Some(handle),
                Err(_) => break,
            }
```

The `*state.dashboard.lock().unwrap() = Some(…)` mirror write is gone with the field.

- [ ] **Step 2: Sends in `connections.rs`**

Every `dashboard_send_owner(&state, &owned.identity, m)` → `state.dashboard.send_owner(&owned.identity, m)`; every `dashboard_send_owner_with_completion(&state, &owned.identity, m, c)` → `state.dashboard.send_owner_with_completion(&owned.identity, m, c)`. The closing block at `:376-384` becomes:

```rust
        if dashboard_sink.is_closing() {
            if let Some(owned) = ownership.as_ref()
                && let Some(snapshot) = state.dashboard.snapshot_owned(&owned.identity)
            {
                state.dashboard.disconnect(snapshot);
            }
            break;
        }
```

`Request::Select` revision at `:492-497` → `let revision = state.dashboard.next_revision();`.

- [ ] **Step 3: History dispatch**

`dispatch_history_begin`: replace the first two blocks with

```rust
    if !state.dashboard.owns(owner) {
        return;
    }
    let Some(snapshot) = state.dashboard.with_owned(owner, |current| {
        current.next_history_id.checked_add(1).map(|next| {
            let snapshot = HistorySnapshotId(current.next_history_id);
            current.next_history_id = next;
            current.history.take();
            snapshot
        })
    }) else { return };
```

and the install block with

```rust
    let install = {
        let sessions = state.sessions.lock().unwrap();
        if !sessions.get(&id).is_some_and(|registered| Arc::ptr_eq(registered, &session)) {
            Err(())
        } else {
            state.dashboard.with_owned(owner, |current| current.history = Some(history)).ok_or(())
        }
    };
```

(`Err(()) if dashboard_owner_matches(state, owner)` → `Err(()) if state.dashboard.owns(owner)`.)

`dispatch_history_page`: the `let response = { … }` block becomes

```rust
    let Some(response) = state.dashboard.with_owned(owner, |current| match current.history.as_mut() {
        None => error_response(ErrorCode::NotFound, "history snapshot not found"),
        Some(history) if history.opened().session != session || history.opened().snapshot != snapshot => error_response(ErrorCode::NotFound, "history snapshot not found"),
        Some(history) if start_row > history.opened().total_rows => error_response(ErrorCode::InvalidRequest, "history row start is out of bounds"),
        Some(history) => match history.page(start_row, rows, start_col, cols) {
            Ok(page) => Response::HistoryRows(page),
            Err(error) => error_response(ErrorCode::InvalidRequest, error.to_string()),
        },
    }) else { return };
```

`dispatch_history_end`:

```rust
    if state.dashboard.with_owned(owner, |current| {
        if current.history.as_ref().is_some_and(|history| history.opened().session == session && history.opened().snapshot == snapshot) {
            current.history.take();
        }
    }).is_none() {
        return;
    }
    send_history_response(state, owner, request_id, Response::Ok);
```

`send_history_response` and `view_error` call `state.dashboard.send_owner(owner, …)`.

- [ ] **Step 4: `dispatch_set_view_with_resize`**

`if !dashboard_owner_matches(state, owner)` → `if !state.dashboard.owns(owner)`; `dashboard_snapshot(state).filter(|s| Arc::ptr_eq(&s.identity, owner))` → `state.dashboard.snapshot_owned(owner)`; `disconnect_dashboard(state, snapshot)` → `state.dashboard.disconnect(snapshot)`. The tail was replaced in Task 5. `clear_view_subscription_for_owner(state, owner, r)` callers → `state.dashboard.clear_view_for(owner, r)`; `clear_view_subscription(state, r)` → `state.dashboard.clear_view(r)`; delete both wrapper functions.

- [ ] **Step 5: Delete the nine wrappers in `outbound.rs` and prune `mod.rs` imports.** Then `rg -n 'Arc::ptr_eq' crates/ovrcr-runtime/src/server/{connections,dispatch,outbound,mod}.rs` must return only the `sessions.get(&id) … Arc::ptr_eq(registered, &session)` check in `dispatch_history_begin` (that one compares sessions, not owners).

- [ ] **Step 6: Run** — `cargo test -p ovrcr-runtime` and `cargo clippy -p ovrcr-runtime --all-targets --all-features -- -D warnings`. Expected: pass, no unused-function warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/ovrcr-runtime/src/server
git commit -m "refactor(server): one owner check and one teardown for the active dashboard"
```

### Task 7: server tests assert on the authority

**Files:**
- Modify: `crates/ovrcr-runtime/src/server/tests.rs`

- [ ] **Step 1:** For each of the fourteen former `state.dashboard.lock().unwrap().is_none()/is_some()` assertions (Task 5 turned them into `is_claimed()`), check the surrounding test: where it also holds an `identity`, prefer `assert!(!state.dashboard.owns(&identity))` so the assertion names the owner it means. Where a test installed a slot by hand and then asserted the mirror was `None` while the slot was still `Some` (the drift the review found), the new assertion will fail: that is a real finding, fix the test's expectation to the authority and note it in the commit message.

- [ ] **Step 2: Add one regression for the teardown split**

```rust
#[test]
fn dropped_ownership_and_writer_disconnect_leave_identical_state() {
    let (client, server) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let state = test_state(None, None);
    let owner = state.dashboard.claim(Arc::clone(&sink), server).unwrap();
    state.dashboard.install_view_for_test(Some(DashboardView { revision: 1, panes: Vec::new(), focused: None }));
    drop(DashboardOwnership { state: Arc::clone(&state), identity: Arc::clone(&owner) });
    assert!(!state.dashboard.is_claimed());
    assert!(state.dashboard.view().is_none());
    assert!(state.dashboard.geometry().is_none());
    assert!(!state.dashboard.owns(&owner));
    drop(client);
}
```

(`DashboardOwnership` is private to `connections.rs`; make it `pub(super)` with `pub(super)` fields.)

- [ ] **Step 3: Run** — `cargo test -p ovrcr-runtime` Expected: pass. Record the count (was 48 server tests + module tests).

- [ ] **Step 4: Commit**

```bash
git add crates/ovrcr-runtime/src/server/tests.rs crates/ovrcr-runtime/src/server/connections.rs
git commit -m "test(server): assert dashboard ownership on the authority, not a mirror"
```


---

## Phase C — the Dashboard's four inner modules

All files are under `crates/ovrcr-tui/src/dashboard/`. The `Dashboard` struct is already private behind `install_*` / `request_view_at` / `handle_server_message` (PR #74), so these tasks change internals plus a handful of `pub(super)` helpers; the public seam and the `tests/tui` suites keep compiling. Run `cargo test -p ovrcr-tui --lib` and `cargo test -p ovrcr --test tui` after every task.

### Task 8: `ready.rs`, one readiness predicate

**Files:**
- Create: `ready.rs`
- Modify: `mod.rs` (add `mod ready;`), `render.rs:554,580,813,1126,1187-1195`, `desktop.rs:254-263`

**Interfaces:**
- Produces (`pub(super)`):
  - `fn activity(session: &SessionSummary) -> AgentActivity` — the agent sample's state when an agent is bound, else the legacy rollup `session.activity`.
  - `fn ready(session: &SessionSummary) -> Option<&ActivitySample>` — the `ResponseReady` sample, if any.
  - `fn delivery_live(session: &SessionSummary) -> bool` — Ready per CONTEXT.md and deliverable: Running, Codex, reporter Connected, non-empty turn.

- [ ] **Step 1: Failing test table** (in `ready.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, ReporterHealth, SampleQuality, SessionId, SessionPhase};

    fn session(phase: SessionPhase, provider: AgentProvider, health: ReporterHealth, state: AgentActivity, turn: Option<&str>) -> SessionSummary {
        SessionSummary {
            id: SessionId(1), project: "p".into(), workspace: "w".into(), name: "s".into(), label: "l".into(),
            pid: Some(1), started_unix_ms: 0, phase, activity: AgentActivity::Unknown, context_usage: None, agent_epoch: 1, unread: None,
            agent: Some(AgentSnapshot {
                binding: AgentBinding { provider, invocation: "i".into(), conversation: "c".into(), generation: 1 },
                activity: Some(ActivitySample { state, quality: SampleQuality::Observed, turn: turn.map(Into::into) }),
                metrics: None,
                health: HealthSample { state: health, reason: None },
                activity_revision: 1, metrics_revision: 0, health_revision: 0,
            }),
        }
    }

    #[test]
    fn readiness_table() {
        use AgentActivity::*; use AgentProvider::*; use ReporterHealth::*; use SessionPhase::*;
        let cases = [
            // phase, provider, health, state, turn, expect ready, expect live
            (Running, Codex, Connected, ResponseReady, Some("t"), true, true),
            (Running, Codex, Unavailable, ResponseReady, Some("t"), true, false),
            (Running, Codex, Connected, ResponseReady, Some(""), true, false),
            (Running, Codex, Connected, ResponseReady, None, true, false),
            (Running, Claude, Connected, ResponseReady, Some("t"), true, false),
            (Paused, Codex, Connected, ResponseReady, Some("t"), true, false),
            (Running, Codex, Connected, Busy, Some("t"), false, false),
        ];
        for (phase, provider, health, state, turn, want_ready, want_live) in cases {
            let s = session(phase.clone(), provider, health, state, turn);
            assert_eq!(ready(&s).is_some(), want_ready, "{phase:?} {provider:?} {health:?} {state:?} {turn:?}");
            assert_eq!(delivery_live(&s), want_live, "{phase:?} {provider:?} {health:?} {state:?} {turn:?}");
            assert_eq!(activity(&s), state);
        }
    }

    #[test]
    fn activity_falls_back_to_the_rollup_without_an_agent() {
        let mut s = session(SessionPhase::Running, AgentProvider::Codex, ReporterHealth::Connected, AgentActivity::Busy, None);
        s.agent = None;
        s.activity = AgentActivity::WaitingInput;
        assert_eq!(activity(&s), AgentActivity::WaitingInput);
        assert!(ready(&s).is_none());
    }
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test -p ovrcr-tui --lib ready::` Expected: compile error, module missing.

- [ ] **Step 3: Write the module**

```rust
//! Ready (CONTEXT.md): the accepted Codex root observation of a completed turn. This is the
//! only place the Dashboard decides whether a session is ready and whether that readiness
//! may be delivered as an alert. The sidebar glyph, the metadata line, and desktop delivery
//! all ask here, so they cannot disagree.
use ovrcr_protocol::{ActivitySample, AgentActivity, AgentProvider, ReporterHealth, SessionPhase, SessionSummary};

pub(super) fn activity(session: &SessionSummary) -> AgentActivity {
    session.agent.as_ref().and_then(|agent| agent.activity.as_ref()).map_or(session.activity, |sample| sample.state)
}

pub(super) fn ready(session: &SessionSummary) -> Option<&ActivitySample> {
    session.agent.as_ref()?.activity.as_ref().filter(|sample| sample.state == AgentActivity::ResponseReady)
}

pub(super) fn delivery_live(session: &SessionSummary) -> bool {
    session.phase == SessionPhase::Running
        && session.agent.as_ref().is_some_and(|agent| {
            agent.binding.provider == AgentProvider::Codex && agent.health.state == ReporterHealth::Connected
        })
        && ready(session).and_then(|sample| sample.turn.as_deref()).is_some_and(|turn| !turn.is_empty())
}
```

- [ ] **Step 4: Replace the spellings**

- `render.rs:554`, `:580`: `session.agent.as_ref().and_then(|agent| agent.activity.as_ref()).is_some_and(|activity| activity.state == AgentActivity::ResponseReady)` → `super::ready::ready(session).is_some()`.
- `render.rs:813` (`ready_agent`): `let ready_agent = session.filter(|session| super::ready::ready(session).is_some()).and_then(|session| session.agent.as_ref());`
- `render.rs:1126` `session_status_glyph`: `match (&session.phase, session.activity)` → `match (&session.phase, super::ready::activity(session))`. This is the behavior change the review named: the glyph now reads the same field as the notification path. Because the runtime already mirrors the agent state into `session.activity`, existing sidebar tests keep passing; the `tests/tui/unread.rs` and `desktop.rs` fixtures that set both fields prove it.
- `render.rs:1191-1193` in `provider_activity`: `if let Some(activity) = super::ready::ready(session) { … }` replacing the `if let Some(activity) = &agent.activity && activity.state == ResponseReady`.
- `desktop.rs:254-263`: delete `delivery_live` and import `use super::ready::delivery_live;`.

- [ ] **Step 5: Run** — `cargo test -p ovrcr-tui --lib` and `cargo test -p ovrcr --test tui`. Expected: pass, counts unchanged plus 2 new.

- [ ] **Step 6: Commit**

```bash
git add crates/ovrcr-tui/src/dashboard/ready.rs crates/ovrcr-tui/src/dashboard/mod.rs crates/ovrcr-tui/src/dashboard/render.rs crates/ovrcr-tui/src/dashboard/desktop.rs
git commit -m "refactor(tui): decide readiness in one module"
```

### Task 9: `outbox.rs`, one drain for outbound requests

Today the event loop polls seven drains (`poll_palette`, `view_request`, `history_request_if_needed`, `take_pending_history_end`, `take_deferred_history_request`, `take_mouse_cleanup`, `take_pending_history_copy`) at asymmetric sites, and `history_end_after_selection` is a single slot: `split_pane`/`focus_pane`/`close_focused_pane` fill it and return `Redraw` (nothing drains it that tick), then `leave_history` overwrites it unconditionally (`state.rs:1707`). The server-side capture leaks. After this task producers push `ClientMessage`s into one FIFO and the loop drains it in one place. `take_pending_history_copy` is clipboard text, not a request; it stays.

**Files:**
- Create: `outbox.rs`
- Modify: `mod.rs` (add `mod outbox;`, field `outbox: outbox::Outbox`, delete `history_end_after_selection` and `MouseForwarding::pending_cleanup`)
- Modify: `state.rs` (`new`, `leave_history`, `release_for_selection_change`, `take_pending_history_end` (delete), `take_mouse_cleanup` (delete), `queue_held_releases`, `request_selected`, `handle_server_message` tail, `update_hierarchy:3159-3162`, the Ok-granted arm's `take_deferred_history_request` push, `request_view_at`)
- Modify: `palette.rs:415` (`poll_palette` returns `bool`)
- Modify: `event_loop.rs` (`dashboard_loop`, `next_dashboard_messages`, `emit_view_request`, `drain_dashboard_input_then_emit_with`, `send_dashboard_action`, `run_dashboard:75-85`)
- Modify tests: `tests/tui/main.rs:359`, `tests/tui/palette.rs:1031`, `palette.rs:2126-2145` (`poll_palette().1` → `poll_palette(); dashboard.drain_outbox()`)

**Interfaces:**
- Produces:
  - `pub(super) struct Outbox { queue: VecDeque<ClientMessage> }` with `push`, `drain(&mut self) -> Vec<ClientMessage>`, `is_empty`.
  - `Dashboard::push_request(&mut self, message: ClientMessage)` (`pub(super)`)
  - `Dashboard::drain_outbox(&mut self) -> Vec<ClientMessage>` (**pub**) — drains the queue then appends `history_request_if_needed()`; the event loop and tests call this.
  - `Dashboard::drained_action(&mut self) -> DashboardAction` (`pub(super)`) — `None` / `Request(one)` / `RequestBatch(many)` from `drain_outbox()`, so key/mouse paths keep returning the actions tests match on.
  - `Dashboard::poll_palette(&mut self) -> bool` (was `(bool, Option<ClientMessage>)`; the request now goes to the outbox).
- Invariant (documented on `push_request`): a synthetic mouse release is pushed by `queue_held_releases` before any `SetView` that moves focus, because every retarget path calls `cancel_mouse_gesture` before `view_request` runs. The order test below pins it.

- [ ] **Step 1: Failing tests** (append to `dashboard/tests.rs`)

```rust
#[test]
fn history_end_survives_split_and_leave_history() {
    // Regression: release_for_selection_change parked HistoryEnd in a slot that split/focus/close
    // never drained and leave_history overwrote.
    let mut dashboard = staged_history_copy_dashboard(); // history open on session 1
    assert!(dashboard.split_pane());
    let batch = dashboard.drain_outbox();
    assert!(
        batch.iter().any(|m| matches!(m.request, Request::HistoryEnd { session: SessionId(1), .. })),
        "split must release the captured history: {batch:?}"
    );
    assert!(dashboard.drain_outbox().is_empty(), "drained once");
}

#[test]
fn mouse_cleanup_precedes_the_replacement_set_view() {
    let mut dashboard = staged_mouse_dashboard(); // helper exists in tests.rs mouse section; a held button on session 1
    dashboard.select_session(SessionId(2));
    let _ = dashboard.request_view_at(dashboard_area(&dashboard));
    let batch = dashboard.drain_outbox();
    let cleanup = batch.iter().position(|m| matches!(m.request, Request::Input { .. }));
    let set_view = batch.iter().position(|m| matches!(m.request, Request::SetView { .. }));
    assert!(cleanup < set_view, "{batch:?}");
}
```

If `staged_mouse_dashboard`/`dashboard_area` do not exist under those names, use the held-mouse fixture already used by the `queue_held_releases` tests in `tests.rs` (search `held: [`), and `Rect::new(0, 0, cols, rows)` from the fixture size.

- [ ] **Step 2: Run to verify it fails** — `cargo test -p ovrcr-tui --lib history_end_survives` Expected: compile error (`drain_outbox` missing).

- [ ] **Step 3: Write the module and wire producers**

`outbox.rs`:

```rust
//! Every request the Dashboard wants sent, in the order it decided to send it. The event
//! loop drains it once per pass; nothing else holds a request in a private slot.
use ovrcr_protocol::ClientMessage;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Outbox {
    queue: VecDeque<ClientMessage>,
}

impl Outbox {
    pub(super) fn push(&mut self, message: ClientMessage) {
        self.queue.push_back(message);
    }
    pub(super) fn drain(&mut self) -> Vec<ClientMessage> {
        self.queue.drain(..).collect()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
```

In `state.rs` (impl Dashboard):

```rust
    /// Queue a request for the next drain. A synthetic mouse release must precede the
    /// `SetView` that moves focus; every retarget calls `cancel_mouse_gesture` before
    /// `view_request`, so push order is send order.
    pub(super) fn push_request(&mut self, message: ClientMessage) {
        self.outbox.push(message);
    }

    /// Everything queued since the last drain, then the next history page if one is due.
    pub fn drain_outbox(&mut self) -> Vec<ClientMessage> {
        let mut batch = self.outbox.drain();
        if let Some(request) = self.history_request_if_needed() {
            batch.push(request);
        }
        batch
    }

    pub(super) fn drained_action(&mut self) -> DashboardAction {
        let mut batch = self.drain_outbox();
        match batch.len() {
            0 => DashboardAction::Redraw,
            1 => DashboardAction::Request(batch.remove(0)),
            _ => DashboardAction::RequestBatch(batch),
        }
    }
```

Producers:
- `leave_history`: `self.history_end_after_selection = end;` → `if let Some(end) = end { self.push_request(end); }`.
- `release_for_selection_change`: the `self.history_end_after_selection = Some(ClientMessage{…})` → `let end = ClientMessage { … }; self.push_request(end);`.
- `queue_held_releases`: the guard `if self.mouse.pending_cleanup.is_some() { self.clear_held_mouse(); return; }` is deleted (there is no slot to protect); the tail `self.mouse.pending_cleanup = Some(ClientMessage{…})` → `self.push_request(ClientMessage { … })`.
- Delete `take_pending_history_end`, `take_mouse_cleanup`, the `history_end_after_selection` field and its `new()` line, and `MouseForwarding::pending_cleanup`.
- `request_selected`: body becomes `let _ = self.request_view_at(self.outer_area); self.drained_action()`. (`request_view_at` keeps returning the `SetView` for tests; make it also push: `pub fn request_view_at(&mut self, area) -> Option<ClientMessage> { let id = self.next_request_id(); let request = self.view_request(area, id).ok().flatten(); if let Some(request) = &request { self.push_request(request.clone()); } request }`.) Callers in `palette.rs:1416,1472` that use `select_request(session, request_id)` and wrap the result in `DashboardAction::Request` switch to `self.select_session(session); self.drained_action()`; delete `select_request`.
- `update_hierarchy:3159-3162`: delete the `take_pending_history_end` push (the release already pushed).
- `handle_server_message`: replace the local `let mut outgoing = Vec::new();` and every `outgoing.push(x)` / `outgoing.extend(xs)` with `self.push_request(x)` / `for x in xs { self.push_request(x) }`; the Ok-granted arm's `take_deferred_history_request` result is pushed the same way; the function ends with `self.drain_outbox()`. `palette_response` and `attach_created_workspace` keep returning `Vec<ClientMessage>`; push what they return.
- `poll_palette`: change the return to `bool` and push the request it used to return.
- `run_dashboard:75-85`: `let view = dashboard.request_view_at(area).context("create initial dashboard view")?;` then `for message in dashboard.drain_outbox() { write_frame(&mut stream, &message)?; }` (the `SetView` is in the drain; do not also write `view`), keep `expected_screens` from `view`.

Event loop, one helper replaces four repeated blocks:

```rust
fn flush(stream: &mut UnixStream, dashboard: &mut Dashboard) -> Result<()> {
    for message in dashboard.drain_outbox() {
        write_frame(stream, &message)?;
    }
    Ok(())
}

fn emit_view_request(stream: &mut UnixStream, dashboard: &mut Dashboard, area: Rect) -> Result<()> {
    let _ = dashboard.request_view_at(area);
    flush(stream, dashboard)
}
```

In `dashboard_loop`: `let (palette_redraw, palette_request) = …; if let Some(request) … write_frame` → `pending_redraw |= dashboard.poll_palette();` (its request is flushed by the `emit_view_request` that follows). Delete the four `if let Some(request) = dashboard.history_request_if_needed() { write_frame … }` blocks (the drain appends it). `next_dashboard_messages`: `let requests = dashboard.handle_server_message(message); … take_mouse_cleanup … for request in requests { write_frame }` → `for request in dashboard.handle_server_message(message) { write_frame(stream, &request)?; }`. `send_dashboard_action`: delete the leading `take_mouse_cleanup` write and the `EnterBrowse` arm's `take_pending_history_end`; after the `match`, `flush(stream, dashboard)?;`. `drain_dashboard_input_then_emit_with`: the `history_request_if_needed` write after `emit_pending_history_copy` → `flush(stream, dashboard)?`.

- [ ] **Step 4: Run** — `cargo test -p ovrcr-tui --lib` and `cargo test -p ovrcr --test tui`. Expected: pass. Tests that matched `DashboardAction::Request(view)` from a selection still match because `drained_action` returns `Request` for a single message; a test that now sees `RequestBatch([HistoryEnd, SetView])` where it saw `Request(SetView)` was asserting the leak — update it and say so in the commit.

- [ ] **Step 5: Commit**

```bash
git add crates/ovrcr-tui/src/dashboard tests/tui
git commit -m "fix(tui): queue outbound requests in one outbox

split/focus/close parked a HistoryEnd in a slot nothing drained that tick and
leave_history overwrote it; the server-side capture leaked. Producers now push
into one FIFO the event loop drains in one place."
```


### Task 10: `retarget(PaneChange)`, one ritual

Seven call sites run a subset of `mark_pending_parser_discarded` / `release_for_selection_change` / `pending_user_view_change = true` / `invalidate_view_readiness` / `mode = Browse`. Three diverge, two deliberately (`apply_pane_sizes`, `update_hierarchy`), one by omission (`move_selection` with no sessions never releases the History/Copy capture bound to the vanished session). The variant, not the caller, decides.

**Files:**
- Modify: `state.rs` (`split_pane:758-767`, `focus_pane:775-780`, `close_focused_pane:788-795`, `move_selection:1087-1095`, `select_session:1113-1127`, `apply_pane_sizes:2204-2206`, `select_container:2235-2247`, `update_hierarchy:3149-3165`; add `PaneChange` + `retarget`)

**Interfaces:**
- Produces (`pub(super)`):

```rust
pub(super) enum PaneChange {
    Split,
    Focus,
    Close,
    Select,
    Container,
    /// The tree has no sessions; the focused pane is being emptied.
    Cleared,
    /// Drawn pane geometry changed (split drag, sidebar drag). History keeps its frozen cells.
    Resize,
    /// The server removed a session shown in a pane.
    ServerRemoved { focused_survives: bool },
}
fn retarget(&mut self, change: PaneChange)
```

- [ ] **Step 1: Failing regression** (append to `dashboard/tests.rs`)

```rust
#[test]
fn emptying_the_tree_releases_the_captured_history() {
    let mut dashboard = staged_history_copy_dashboard(); // history open on session 1
    dashboard.install_hierarchy(HierarchySnapshot { projects: Vec::new() });
    // install_hierarchy retains no panes; force the empty-selection path explicitly too:
    dashboard.move_selection(1);
    let batch = dashboard.drain_outbox();
    assert!(batch.iter().any(|m| matches!(m.request, Request::HistoryEnd { session: SessionId(1), .. })), "{batch:?}");
    assert_eq!(dashboard.mode(), InputMode::Browse);
}
```

If `Dashboard::mode()` does not exist, add `pub(super) fn mode(&self) -> InputMode { self.mode }` (tests.rs is a child module, so `pub(super)` is enough).

- [ ] **Step 2: Run to verify it fails** — `cargo test -p ovrcr-tui --lib emptying_the_tree` Expected: FAIL, no `HistoryEnd` in the batch.

- [ ] **Step 3: Write `retarget` and use it everywhere**

```rust
    /// Every path that changes what a pane shows comes through here. The variant decides
    /// which releases apply, so a reader can tell "deliberate" from "forgotten" in one
    /// place instead of seven.
    pub(super) fn retarget(&mut self, change: PaneChange) {
        use PaneChange::*;
        let discard_pending_parser = !matches!(change, Focus | Resize);
        let release_capture = !matches!(change, Resize | ServerRemoved { focused_survives: true });
        let user_change = !matches!(change, ServerRemoved { .. } | Cleared);
        let invalidate = !matches!(change, ServerRemoved { focused_survives: true });
        let browse = !matches!(change, Resize | ServerRemoved { focused_survives: true });
        if discard_pending_parser {
            self.mark_pending_parser_discarded();
        }
        if matches!(change, ServerRemoved { .. }) {
            // `retain` shifts pane indices, so a parked wheel deferral no longer names the
            // pane the user scrolled, even when the focused session survives.
            self.deferred_history_at_tail = None;
        }
        if release_capture {
            self.release_for_selection_change();
        }
        if user_change {
            self.pending_user_view_change = true;
        }
        if invalidate {
            self.invalidate_view_readiness();
        }
        if browse {
            self.mode = InputMode::Browse;
        }
    }
```

Call sites (each keeps its own pane mutation; the ritual lines are replaced):
- `split_pane`: lines 758–759 → `self.retarget(PaneChange::Split);` placed *before* the pane push (release must see the old focus); delete 765–767.
- `focus_pane`: 775 → `self.retarget(PaneChange::Focus);` delete 778–780.
- `close_focused_pane`: 788–789 → `self.retarget(PaneChange::Close);` delete 793–795.
- `move_selection` empty branch: 1088 → `self.retarget(PaneChange::Cleared);` keep the `pane.session = None` (the `ready`/`snapshot_installed` writes disappear in Task 11; leave them for now).
- `select_session`: 1113–1114 → `self.retarget(PaneChange::Select);` delete 1125–1127.
- `apply_pane_sizes`: 2205–2206 → `self.retarget(PaneChange::Resize);` (`queue_held_releases` stays before it).
- `select_container`: 2235–2236 → `self.retarget(PaneChange::Container);` delete 2245–2247.
- `update_hierarchy`: 3149–3165 → `self.retarget(PaneChange::ServerRemoved { focused_survives }); self.panes.retain(…);` computing `focused_survives` first. The old block also pushed the pending history end; the outbox already holds it.

`invalidate_view_readiness`, `release_for_selection_change`, and `mark_pending_parser_discarded` become private helpers called only from `retarget` (grep confirms; `view_request` still calls `cancel_mouse_gesture`/`cancel_copy` directly for geometry change, which is not a retarget).

- [ ] **Step 4: Run** — `cargo test -p ovrcr-tui --lib` and `cargo test -p ovrcr --test tui`. Expected: pass. Existing `split.rs`/`copy_history.rs` tests cover the deliberate omissions (Resize keeps History; a surviving focused session keeps readiness).

- [ ] **Step 5: Commit**

```bash
git add crates/ovrcr-tui/src/dashboard/state.rs crates/ovrcr-tui/src/dashboard/tests.rs
git commit -m "fix(tui): one retarget ritual per pane change

move_selection with no sessions emptied the pane without releasing the
History/Copy capture. PaneChange names each path and decides its releases."
```

### Task 11: `view_handshake.rs`, readiness derived

Nine `Dashboard` fields are the view state machine; `pane.ready = false` is written at eleven sites and `= true` at one; `snapshot_installed` (12 writes) and `last_view_request_id` (1 write) have **no readers**. After this task readiness is a predicate over the handshake: a pane is ready iff no request is in flight, the acknowledged view contains its session, and no `ScreenDirty` has marked it stale since.

**Files:**
- Create: `view_handshake.rs`
- Modify: `mod.rs` (`Dashboard` fields, `PaneState` loses `ready`/`snapshot_installed`, delete `RequestedView`/`PendingView` there — they move), `state.rs` (`new`, `install_unready`, `install_screen`, `view_revision`, `view_request`, `apply_screen`, `invalidate_view_readiness`, `mark_pending_parser_discarded`, `view_retry_deadline`, `handle_server_message` Screen/Ok/Error/Output/ScreenDirty arms, `retire_request_id`, `input_is_allowed`, `refuse_input`, `take_deferred_history_request`, the three `pane.ready` reads at `:1296,1432,2395`), `render.rs:521,647,800,817` (`pane.ready` → `dashboard.pane_ready(pane)`), `hints.rs:418-419`, `event_loop.rs:79,738` (`pending_view` → `dashboard.pending_targets()`), `dashboard/tests.rs:280,315,341,379,440`

**Interfaces:**
- Produces:

```rust
// view_handshake.rs
#[derive(Clone)] pub(super) struct RequestedView { pub revision: u64, pub targets: Vec<(SessionId, TerminalSize)>, pub focused: Option<SessionId> }
pub(super) enum Desire { Send { request: ClientMessage, owns_error: bool }, Coalesced, Unchanged, Waiting }
pub(super) enum Acknowledged { Granted { view: RequestedView }, Incomplete { view: RequestedView }, Refresh }
#[derive(Default)] pub(super) struct ViewHandshake { /* revision, pending, acknowledged, failed, force_refresh, request_ids, snapshots, stale, user_change */ }
impl ViewHandshake {
    fn revision(&self) -> u64
    fn note_user_change(&mut self)
    fn force_refresh(&mut self)
    fn desire(&mut self, desired: RequestedView, request_id: u64, now: Instant) -> anyhow::Result<Desire>
    fn snapshot_matches(&self, request_id: u64, revision: u64, session: SessionId) -> bool
    fn record_snapshot(&mut self, session: SessionId)
    fn acknowledge(&mut self, request_id: u64, desired_now: &RequestedView, now: Instant) -> Option<Acknowledged>
    fn refuse(&mut self, request_id: u64, now: Instant) -> Option<RequestedView>
    fn invalidate(&mut self)
    fn mark_stale(&mut self, session: SessionId)
    fn mark_parser_discarded(&mut self)
    fn ready(&self, session: SessionId) -> bool
    fn acknowledged(&self) -> Option<&RequestedView>
    fn pending_targets(&self) -> usize
    fn retry_deadline(&self) -> Option<Instant>
    fn was_view_request(&self, request_id: u64) -> bool
    fn retire(&mut self, request_id: u64)
    fn install_acknowledged(&mut self, view: RequestedView)   // test seam used by install_screen
}
// Dashboard
pub(super) fn pane_ready(&self, pane: &PaneState) -> bool  // pane.session.is_some_and(|s| self.handshake.ready(s))
```

- [ ] **Step 1: Failing tests for the three invariants** (in `view_handshake.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{Request, SessionId, TerminalSize};
    use std::time::Instant;

    fn view(sessions: &[u64]) -> RequestedView {
        RequestedView {
            revision: 0,
            targets: sessions.iter().map(|id| (SessionId(*id), TerminalSize { rows: 10, cols: 20 })).collect(),
            focused: sessions.first().map(|id| SessionId(*id)),
        }
    }
    fn send(h: &mut ViewHandshake, v: &RequestedView, id: u64) -> u64 {
        match h.desire(v.clone(), id, Instant::now()).unwrap() {
            Desire::Send { request, .. } => match request.request { Request::SetView { view } => view.revision, _ => panic!() },
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn one_request_in_flight_and_readiness_only_after_full_acknowledgement() {
        let mut h = ViewHandshake::default();
        let v = view(&[1, 2]);
        let revision = send(&mut h, &v, 10);
        assert!(matches!(h.desire(v.clone(), 11, Instant::now()).unwrap(), Desire::Coalesced));
        assert!(!h.ready(SessionId(1)));
        assert!(h.snapshot_matches(10, revision, SessionId(1)));
        assert!(!h.snapshot_matches(10, revision + 1, SessionId(1)), "wrong revision");
        assert!(!h.snapshot_matches(99, revision, SessionId(1)), "wrong request");
        assert!(!h.snapshot_matches(10, revision, SessionId(3)), "wrong session");
        h.record_snapshot(SessionId(1));
        assert!(matches!(h.acknowledge(10, &v, Instant::now()), Some(Acknowledged::Incomplete { .. })));
        assert!(!h.ready(SessionId(1)));
        let revision = send(&mut h, &v, 12);
        h.record_snapshot(SessionId(1));
        h.record_snapshot(SessionId(2));
        assert!(matches!(h.acknowledge(12, &v, Instant::now()), Some(Acknowledged::Granted { .. })));
        assert!(h.ready(SessionId(1)) && h.ready(SessionId(2)));
        assert_eq!(h.revision(), revision);
        assert!(matches!(h.desire(v.clone(), 13, Instant::now()).unwrap(), Desire::Unchanged));
    }

    #[test]
    fn change_revokes_readiness_and_stale_responses_are_ignored() {
        let mut h = ViewHandshake::default();
        let v = view(&[1]);
        send(&mut h, &v, 1);
        h.record_snapshot(SessionId(1));
        assert!(matches!(h.acknowledge(1, &v, Instant::now()), Some(Acknowledged::Granted { .. })));
        h.invalidate();
        assert!(!h.ready(SessionId(1)));
        let other = view(&[2]);
        send(&mut h, &other, 2);
        assert!(h.acknowledge(1, &other, Instant::now()).is_none(), "late Ok for a retired request");
        assert!(h.refuse(1, Instant::now()).is_none());
        h.record_snapshot(SessionId(2));
        assert!(matches!(h.acknowledge(2, &other, Instant::now()), Some(Acknowledged::Granted { .. })));
        h.mark_stale(SessionId(2));
        assert!(!h.ready(SessionId(2)));
    }

    #[test]
    fn refusal_waits_out_the_backoff_then_resends() {
        let mut h = ViewHandshake::default();
        let v = view(&[1]);
        send(&mut h, &v, 1);
        assert!(h.refuse(1, Instant::now()).is_some());
        assert!(matches!(h.desire(v.clone(), 2, Instant::now()).unwrap(), Desire::Waiting));
        let later = Instant::now() + VIEW_RETRY_BACKOFF + std::time::Duration::from_millis(1);
        assert!(matches!(h.desire(v.clone(), 3, later).unwrap(), Desire::Send { .. }));
    }

    #[test]
    fn user_change_owns_the_error_banner_once() {
        let mut h = ViewHandshake::default();
        let v = view(&[1]);
        h.note_user_change();
        assert!(matches!(h.desire(v.clone(), 1, Instant::now()).unwrap(), Desire::Send { owns_error: true, .. }));
        h.record_snapshot(SessionId(1));
        h.acknowledge(1, &v, Instant::now());
        h.force_refresh();
        assert!(matches!(h.desire(v.clone(), 2, Instant::now()).unwrap(), Desire::Send { owns_error: false, .. }));
    }
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test -p ovrcr-tui --lib view_handshake::` Expected: compile error.

- [ ] **Step 3: Write the module**

```rust
//! The view handshake with the server: one `SetView` in flight, snapshots matched by
//! request, revision, and session, readiness granted only by a complete final `Ok`.
//! Readiness is derived here, never written by a caller.
use ovrcr_protocol::{ClientMessage, DashboardView, PaneTarget, Request, SessionId, TerminalSize};
use anyhow::anyhow;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How long a refused view waits before the same view is sent again.
pub(super) const VIEW_RETRY_BACKOFF: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub(super) struct RequestedView {
    pub revision: u64,
    pub targets: Vec<(SessionId, TerminalSize)>,
    pub focused: Option<SessionId>,
}

impl RequestedView {
    fn same(&self, other: &Self) -> bool {
        self.targets == other.targets && self.focused == other.focused
    }
    fn contains(&self, session: SessionId) -> bool {
        self.targets.iter().any(|(id, _)| *id == session)
    }
}

struct Pending {
    request_id: u64,
    view: RequestedView,
    parser_discarded: bool,
}

#[derive(Debug)]
pub(super) enum Desire {
    Send { request: ClientMessage, owns_error: bool },
    Coalesced,
    Unchanged,
    Waiting,
}

#[derive(Debug)]
pub(super) enum Acknowledged {
    Granted { view: RequestedView },
    Incomplete { view: RequestedView },
    Refresh,
}

#[derive(Default)]
pub(super) struct ViewHandshake {
    revision: u64,
    pending: Option<Pending>,
    acknowledged: Option<RequestedView>,
    failed: Option<(RequestedView, Instant)>,
    force_refresh: bool,
    request_ids: HashSet<u64>,
    snapshots: HashSet<SessionId>,
    stale: HashSet<SessionId>,
    user_change: bool,
}

impl ViewHandshake {
    pub(super) fn revision(&self) -> u64 { self.revision }
    pub(super) fn note_user_change(&mut self) { self.user_change = true; }
    pub(super) fn force_refresh(&mut self) { self.force_refresh = true; }
    pub(super) fn acknowledged(&self) -> Option<&RequestedView> { self.acknowledged.as_ref() }
    pub(super) fn pending_targets(&self) -> usize { self.pending.as_ref().map_or(0, |p| p.view.targets.len()) }
    pub(super) fn was_view_request(&self, request_id: u64) -> bool { self.request_ids.contains(&request_id) }
    pub(super) fn retire(&mut self, request_id: u64) { self.request_ids.remove(&request_id); }
    pub(super) fn mark_parser_discarded(&mut self) { if let Some(p) = self.pending.as_mut() { p.parser_discarded = true; } }
    pub(super) fn mark_stale(&mut self, session: SessionId) { self.stale.insert(session); self.force_refresh = true; }
    pub(super) fn retry_deadline(&self) -> Option<Instant> { self.failed.as_ref().map(|(_, at)| *at + VIEW_RETRY_BACKOFF) }

    pub(super) fn ready(&self, session: SessionId) -> bool {
        self.pending.is_none()
            && !self.stale.contains(&session)
            && self.acknowledged.as_ref().is_some_and(|view| view.contains(session))
    }

    /// Selection, assignment, or geometry changed: nothing is acknowledged any more.
    pub(super) fn invalidate(&mut self) {
        self.acknowledged = None;
    }

    fn retry_is_waiting(&mut self, desired: &RequestedView, now: Instant) -> bool {
        let Some((refused, at)) = self.failed.as_ref() else { return false };
        if refused.same(desired) && now.duration_since(*at) < VIEW_RETRY_BACKOFF {
            return true;
        }
        self.failed = None;
        false
    }

    pub(super) fn desire(&mut self, desired: RequestedView, request_id: u64, now: Instant) -> anyhow::Result<Desire> {
        if self.retry_is_waiting(&desired, now) {
            return Ok(Desire::Waiting);
        }
        if self.pending.is_some() {
            // Coalesced into the request this pending view's completion sends; banner
            // ownership waits here rather than being dropped.
            return Ok(Desire::Coalesced);
        }
        if !self.force_refresh && self.acknowledged.as_ref().is_some_and(|a| a.same(&desired)) {
            // A true no-op completes nothing, so it must not hand a pending user change's
            // banner ownership to whichever request the server asks for next.
            self.user_change = false;
            return Ok(Desire::Unchanged);
        }
        let revision = self.revision.checked_add(1).ok_or_else(|| anyhow!("dashboard view revision exhausted"))?;
        self.revision = revision;
        let mut view = desired;
        view.revision = revision;
        self.force_refresh = false;
        self.request_ids.insert(request_id);
        let owns_error = std::mem::take(&mut self.user_change);
        self.snapshots.clear();
        let request = ClientMessage {
            request_id,
            request: Request::SetView {
                view: DashboardView {
                    revision,
                    panes: view.targets.iter().map(|(session, size)| PaneTarget { session: *session, size: *size }).collect(),
                    focused: view.focused,
                },
            },
        };
        self.pending = Some(Pending { request_id, view, parser_discarded: false });
        Ok(Desire::Send { request, owns_error })
    }

    pub(super) fn snapshot_matches(&self, request_id: u64, revision: u64, session: SessionId) -> bool {
        revision == self.revision
            && self.pending.as_ref().is_some_and(|p| p.request_id == request_id && p.view.revision == revision && p.view.contains(session))
    }

    pub(super) fn record_snapshot(&mut self, session: SessionId) {
        self.snapshots.insert(session);
    }

    /// The final `Ok` for `request_id`. `None` when it is not the in-flight request.
    pub(super) fn acknowledge(&mut self, request_id: u64, desired_now: &RequestedView, now: Instant) -> Option<Acknowledged> {
        if self.pending.as_ref()?.request_id != request_id {
            return None;
        }
        let pending = self.pending.take().expect("checked above");
        let complete = pending.view.targets.iter().all(|(session, _)| self.snapshots.contains(session));
        self.snapshots.clear();
        if !complete {
            // `Ok` is final and every snapshot precedes it, so a missing one is a failed view.
            // Recorded after the caller re-requests so the first retry is immediate and a
            // server that keeps answering without snapshots is throttled.
            self.force_refresh = true;
            self.failed = Some((pending.view.clone(), now));
            return Some(Acknowledged::Incomplete { view: pending.view });
        }
        if !self.force_refresh && pending.view.same(desired_now) && !pending.parser_discarded {
            self.stale.clear();
            self.failed = None;
            self.acknowledged = Some(pending.view.clone());
            return Some(Acknowledged::Granted { view: pending.view });
        }
        // Desired changed while in flight, or the parser was discarded: nothing is ready and
        // the next `desire` mints a fresh request.
        self.acknowledged = None;
        self.force_refresh |= pending.parser_discarded;
        Some(Acknowledged::Refresh)
    }

    /// A refused in-flight view. Input stays revoked and the view waits out one backoff.
    pub(super) fn refuse(&mut self, request_id: u64, now: Instant) -> Option<RequestedView> {
        if self.pending.as_ref()?.request_id != request_id {
            return None;
        }
        let pending = self.pending.take().expect("checked above");
        self.snapshots.clear();
        self.acknowledged = None;
        self.failed = Some((pending.view.clone(), now));
        Some(pending.view)
    }

    /// Test seam for `Dashboard::install_screen`: the desired view counts as acknowledged.
    pub(super) fn install_acknowledged(&mut self, view: RequestedView) {
        self.pending = None;
        self.stale.clear();
        self.user_change = false;
        self.acknowledged = Some(view);
    }
}
```

Note on `Incomplete`: the old code re-requested *before* recording `failed`, then recorded it. `acknowledge` records `failed` first and returns `Incomplete`; the caller's next `desire` sees `retry_is_waiting` and returns `Waiting`. That changes the "first retry is immediate" behavior. Preserve it: in `Dashboard`'s Ok arm, on `Incomplete`, call `self.handshake.clear_failed()` (add `pub(super) fn clear_failed(&mut self) { self.failed = None; }`) **after** the immediate re-request succeeds, i.e.:

```rust
Some(Acknowledged::Incomplete { view }) => {
    self.handshake.clear_failed();          // immediate retry
    self.request_view_at(self.outer_area);  // pushes SetView, sets pending
    self.handshake.record_failure(view);    // throttle the next one
}
```

with `pub(super) fn record_failure(&mut self, view: RequestedView) { self.failed = Some((view, Instant::now())); }`. Add both to the interface list.

- [ ] **Step 4: Rewire `Dashboard`**

- `mod.rs`: add `mod view_handshake;`; delete fields `view_revision`, `last_view_request_id`, `pending_view`, `requested_view`, `failed_view`, `force_view_refresh`, `view_request_ids`, `pending_user_view_change`, `pending_snapshot_sessions`; add `handshake: view_handshake::ViewHandshake`; delete `RequestedView`, `PendingView` structs (import `view_handshake::RequestedView` where `desired_view` builds one); `PaneState` loses `ready` and `snapshot_installed`. Keep `error_owned_by_view` and `error_owning_requests` on `Dashboard`: they are banner ownership, shared with non-view requests.
- `state.rs`:
  - `new`: drop the deleted fields, add `handshake: Default::default()`; delete `VIEW_RETRY_BACKOFF` here (moved).
  - `view_revision()` → `self.handshake.revision()`; `view_retry_deadline()` → `self.handshake.retry_deadline()`.
  - `view_request(area, request_id)`: keep the geometry-change cancels and `outer_area`; then
    ```rust
    let desired = self.desired_view();
    for rect in self.pane_rects(area) {
        if let Some(pane) = self.panes.get_mut(rect.pane_index) {
            pane.desired_size = TerminalSize { rows: rect.terminal.height, cols: rect.terminal.width };
        }
    }
    match self.handshake.desire(desired, request_id, Instant::now())? {
        Desire::Send { request, owns_error } => {
            for pane in &mut self.panes {
                if pane.session.is_some_and(|s| matches!(&request.request, Request::SetView { view } if view.panes.iter().any(|p| p.session == s))) {
                    pane.error = None;
                }
            }
            if owns_error {
                self.error_owning_requests.insert(request_id);
            }
            Ok(Some(request))
        }
        Desire::Coalesced | Desire::Unchanged | Desire::Waiting => Ok(None),
    }
    ```
    The eleven `pane.ready = false` / `snapshot_installed = false` writes in this function are gone: readiness is derived.
  - `apply_screen`: the guard becomes `if !self.handshake.snapshot_matches(request_id, revision, session) { return; }` (add `request_id` as the first parameter; the one caller is the `Response::Screen` arm, whose own `matched_screen` check is deleted); replace `self.pending_snapshot_sessions.insert(session)` (both sites) with `self.handshake.record_snapshot(session)`; delete `pane.snapshot_installed = true`.
  - `invalidate_view_readiness`: body becomes `self.deferred_history_at_tail = None; self.handshake.invalidate();`. `mark_pending_parser_discarded` → `self.handshake.mark_parser_discarded()`.
  - `pending_user_view_change = true` (inside `retarget`) → `self.handshake.note_user_change()`; `install_screen`'s `pending_user_view_change = false; requested_view = Some(desired)` → `let desired = self.desired_view(); self.handshake.install_acknowledged(desired);` and delete its `ready`/`snapshot_installed` writes; `install_unready(session)` → `self.handshake.mark_stale(session)`.
  - `retire_request_id`: `was_view_request = self.handshake.was_view_request(id)`; on final responses call `self.handshake.retire(id)` alongside `error_owning_requests.remove`.
  - `handle_server_message`:
    - `Response::Screen { session, revision, size, bytes }` → `self.apply_screen(request_id, revision, session, size, &bytes); if let Some(view) = self.history.as_mut() && view.opened.session == session && self.handshake.snapshot_matches(request_id, revision, session) { view.new_output = true; }` (compute the match once before applying).
    - `Response::Ok` (both arms merge): 
      ```rust
      Response::Ok => {
          let desired_now = self.desired_view();
          match self.handshake.acknowledge(request_id, &desired_now, Instant::now()) {
              Some(Acknowledged::Granted { .. }) => {
                  if self.error_owned_by_view || owns_error { self.error = None; self.error_owned_by_view = false; }
                  if let Some(request) = self.take_deferred_history_request() { self.push_request(request); }
              }
              Some(Acknowledged::Incomplete { view }) => {
                  self.handshake.clear_failed();
                  let _ = self.request_view_at(self.outer_area);
                  self.handshake.record_failure(view);
              }
              Some(Acknowledged::Refresh) => { let _ = self.request_view_at(self.outer_area); }
              None => {
                  if owns_error && !self.history_page_error { self.error = None; self.error_owned_by_view = false; }
              }
          }
      }
      ```
    - `Response::Error`: `let matched_view = self.pending_view…` block → `let refused = self.handshake.refuse(request_id, Instant::now()); if let Some(view) = &refused { for (session, _) in &view.targets { if let Some(pane) = self.panes.iter_mut().find(|p| p.session == Some(*session)) { pane.error = Some(message.clone()); } } } let matched_view = refused.is_some();`.
    - `ServerEvent::Output`: `revision == self.view_revision && … pane.ready` → `revision == self.handshake.revision() && self.handshake.ready(session)`.
    - `ServerEvent::ScreenDirty`: `pane.ready = false; pane.snapshot_installed = false; self.force_view_refresh = true;` → `self.handshake.mark_stale(session);` then `if self.handshake.pending_targets() == 0 { let _ = self.request_view_at(self.outer_area); }` (was `pending_view.is_none()`; add `pub(super) fn in_flight(&self) -> bool` if `pending_targets` reads poorly).
  - `input_is_allowed`: `self.focused_pane().is_some_and(|pane| pane.ready) && self.requested_view…` → `self.handshake.ready(session) && self.handshake.acknowledged().is_some_and(|view| view.focused == Some(session))`.
  - The remaining `pane.ready` reads (`:1296`, `:1432`, `:2395`, `refuse_input`) → `self.focused_pane().is_some_and(|pane| self.pane_ready(pane))`; add `pub(super) fn pane_ready(&self, pane: &PaneState) -> bool { pane.session.is_some_and(|s| self.handshake.ready(s)) }`.
  - `move_selection` empty branch and `select_session`: delete their `ready`/`snapshot_installed` writes (the `retarget` call already invalidates).
- `render.rs:521,647,800,817`: `pane.ready` → `dashboard.pane_ready(pane)`.
- `hints.rs:418-419`: same.
- `event_loop.rs:79` (`expected_screens`) and `:738`: `dashboard.pending_view.as_ref().map_or(0, |p| p.view.targets.len())` → `dashboard.pending_targets()` (add `pub fn pending_targets(&self) -> usize` on `Dashboard`).
- `dashboard/tests.rs:280,315` (`pending_view.is_some/none`) → `dashboard.pending_targets() > 0` / `== 0`; `:341,379` (`dashboard.view_revision`) → `dashboard.view_revision()`; `:440` (`requested_view.is_some()`) → `dashboard.handshake.acknowledged().is_some()` (tests.rs is a child of `dashboard`, so the field is reachable). Delete `tests.rs:700-701` and `:1533` (`ready = true` shortcuts) and use `install_screen` there instead.

- [ ] **Step 5: Run** — `cargo test -p ovrcr-tui --lib`, `cargo test -p ovrcr --test tui`, then `cargo test -p ovrcr --test terminal_acceptance` (real PTY path through `run_dashboard`). Expected: pass. `rg -n '\.ready\b|snapshot_installed|pending_view|requested_view|view_request_ids|failed_view|force_view_refresh|pending_snapshot_sessions|last_view_request_id' crates/ovrcr-tui/src` must match only inside `view_handshake.rs`.

- [ ] **Step 6: Commit**

```bash
git add crates/ovrcr-tui/src/dashboard tests/tui
git commit -m "refactor(tui): derive pane readiness from one view handshake

Nine Dashboard fields and eleven writers of pane.ready collapse into
ViewHandshake; readiness is a predicate over the acknowledged view."
```

---

## Task 12: docs, vocabulary, and final verification

**Files:**
- Modify: `CONTEXT.md`, `docs/development/architecture.md`, `docs/development/tui.md`, `docs/development/runtime.md`

- [ ] **Step 1: CONTEXT.md** — add the concepts the new modules are named after, in the existing format:

```markdown
**Active dashboard**:
The server-side seat the one Dashboard connection holds: its identity, outbound queue, acknowledged view, and focused geometry.
_Avoid_: the dashboard slot, the sink, the view subscription as separate things

**View handshake**:
The Dashboard's exchange with the server that acknowledges a view: one `SetView` in flight, snapshots matched by request, revision, and session, readiness granted by a complete final `Ok`.
_Avoid_: pane ready as a flag anyone sets

**Retarget**:
Any change to what a pane shows: split, focus, close, select, container, cleared, resize, or server removal. Each names which releases apply.
_Avoid_: calling the release steps individually

**Outbox**:
The Dashboard's queue of requests to send, drained once per event-loop pass in push order.
_Avoid_: private slots that hold a request
```

- [ ] **Step 2: Development guides**
  - `architecture.md`: after the table add "Root `src/` and every test speak to the server through `ovrcr_protocol::client`; do not hand-roll `ClientMessage` frames."
  - `runtime.md`: add "The active dashboard (identity, queue, view, geometry) lives in `server/dashboard.rs`. Prove ownership with `ActiveDashboard::owns`; never compare identities inline."
  - `tui.md`: add "Readiness is derived by `ViewHandshake::ready`; pane changes go through `retarget(PaneChange)`; outbound requests go through the outbox and are drained by `drain_outbox`."

- [ ] **Step 3: Full verification**

```bash
just verify
```

Expected: fmt clean, `cargo check --workspace --all-targets`, clippy with `-D warnings`, and `cargo test --workspace --all-targets --all-features` all pass. Record the test counts per crate in the PR body; any test filtered to zero is not a pass. Run `cargo test -p ovrcr --test server_lifecycle` with PTY permission if `just verify` had to skip it, and say so.

- [ ] **Step 4: Commit**

```bash
git add CONTEXT.md docs/development
git commit -m "docs: name the active dashboard, view handshake, retarget, and outbox"
```

- [ ] **Step 5: Hand off.** Report revision, per-crate test counts, and the three defects fixed (List/Inventory mismatch, dropped `HistoryEnd`, unreleased capture on empty selection). Do not push or open the PR without Kyle's authorization; suggest one PR per phase (A, B, C) stacked on `refactor/deep-modules` if the reviewer wants smaller diffs.

## Self-review

- **Coverage:** candidate 1 → Tasks 1–4; 2 → 5–7; 3 → 11; 4 → 10; 5 → 9; 6 → 8; vocabulary and docs → 12.
- **Type consistency:** `client::request/list/inspect/shutdown/expect_ok/session_count/ServerError` (Task 1) are the names used in Tasks 2–4. `ActiveDashboard::{claim, claim_with_identity, is_claimed, owns, snapshot, snapshot_owned, send, try_send, send_owner, send_owner_with_completion, send_owner_terminal, with_owned, view, next_revision, publish_view, clear_view, clear_view_for, geometry, forget_session, disconnect, release, install_view_for_test, slot_for_test}` and `spawn_writer` (Task 5) are the names used in Tasks 6–7. `Dashboard::{push_request, drain_outbox, drained_action, poll_palette -> bool}` (Task 9) are used in Tasks 10–11. `PaneChange`/`retarget` (Task 10) are used in Task 11. `ViewHandshake::{desire, snapshot_matches, record_snapshot, acknowledge, refuse, invalidate, mark_stale, mark_parser_discarded, ready, acknowledged, pending_targets, retry_deadline, was_view_request, retire, install_acknowledged, note_user_change, force_refresh, clear_failed, record_failure, revision}` and `Dashboard::{pane_ready, pending_targets}` (Task 11) match their call sites.
- **Known judgment calls:** `owns()` folds `!is_closing()` in; history requests deliberately use `with_owned` (identity only) so a draining dashboard still gets its `Draining` answer. `Acknowledged::Incomplete` keeps the old "first retry immediate, then throttle" order via `clear_failed`/`record_failure`. `activity()` in `ready.rs` is the one visible glyph change and is covered by existing fixtures that set both fields.
