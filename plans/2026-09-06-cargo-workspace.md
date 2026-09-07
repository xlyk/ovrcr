# Cargo Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reorganize OVRCR into five responsibility-based packages while preserving existing behavior.

**Architecture:** Root application composes runtime and TUI; both consume protocol and terminal foundations. Modules separate state, I/O, rendering and startup without changing ownership or control flow.

**Tech Stack:** Rust edition 2024, Cargo resolver 3, existing dependency versions, synchronous Unix sockets/PTYs, Ratatui and optional eframe helper.

**Spec:** `plans/2026-09-06-cargo-workspace-design.md`

## Global Constraints

- Base: `0a10c923dc061794bb1cfab8af698b0babb0219a`; isolated implementation worktree; no main or unrelated branch changes.
- Exactly five packages: root `ovrcr`, `ovrcr-protocol`, `ovrcr-terminal`, `ovrcr-runtime`, `ovrcr-tui`; internal packages `publish = false`.
- Runtime and TUI depend on protocol and terminal, never on each other or the application. Protocol has no runtime, terminal, filesystem I/O or process ownership.
- Preserve CLI names/aliases/defaults/output/errors, environment names, executable names, socket paths, serde fields/defaults, enum ordering and frame encoding.
- Preserve locks, queues, deadlines, report ordering/admission, PTY lifecycle and process-group ownership; no behavior feature or dependency version upgrade.
- Keep integration tests in root `tests/`; move existing unit tests as private children with assertions intact. No blanket public visibility or duplicate implementations.
- Optional GUI stays application-owned. No new service/framework/async runtime or speculative wrapper/extension trait.
- Use sequential Luna xhigh implementation workers; independent task reviews, then one whole-migration review. Controller publishes reviewed steps; workers do not push, merge or create PRs.
- Prefix every shell executable/pipeline stage with `rtk`. Record exact commands, exit codes, named tests/counts, failures and intentional ignores.
- Real provider hooks/statusline verification is still unavailable; leave those roadmap checkboxes unchecked.

## Verification approach

This is a refactor of already tested behavior. Preserve and run existing cases; there is no new behavior for artificial RED tests. Capture the baseline test inventory before the first extraction and map moved names to new crate targets at the end. If an actual behavior fix becomes necessary, report it to the controller before changing behavior, and demonstrate its focused failing case.

Full check means `rtk proxy cargo check --workspace --all-targets --all-features`. Formatting means `rtk proxy cargo fmt --all -- --check`. These can run after each extraction; avoid repeatedly running unchanged broad suites. Real sockets/PTY tests require scoped sandbox elevation. Unit tests using relative registry paths need a writable working directory.

---

### Task 1: Extract shared protocol and terminal foundations

**Files:**
- Create `crates/ovrcr-protocol/Cargo.toml`, `src/lib.rs`, `src/session.rs`, `src/registry.rs`, `src/context.rs`, `src/wire.rs`, `src/codec.rs` beneath that crate.
- Create `crates/ovrcr-terminal/Cargo.toml`, `src/lib.rs`, `src/paste.rs` beneath that crate.
- Modify root `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/config.rs`, `src/session.rs`, `src/server.rs`, `src/tui.rs`, `src/main.rs`, `src/gui.rs`, `src/report.rs` only as required by these moved interfaces.
- Replace/remove old `src/protocol.rs` and `src/context.rs` implementations; root re-exports new owners.
- Move DispatchMessage to new `src/server/dispatch.rs` (server is still `src/server.rs` in this task).
- Mechanically update `tests/server_lifecycle.rs` and `tests/terminal_acceptance.rs` Registry I/O and DispatchMessage imports; other affected test imports only.
- Existing tests: protocol frame tests, context tests, config tests, TUI key/paste cases.

**Interfaces:**
- Produces `ovrcr_protocol::{SessionId, TerminalSize, SessionPhase, AgentActivity, SessionSummary, Registry, ProjectRecord, WorkspaceRecord}`, all wire public types and read_frame/write_frame/MAX_FRAME_BYTES; `ovrcr_protocol::context` unchanged pure API.
- Produces `Registry::validate(&self) -> anyhow::Result<()>`; all other pure Registry methods unchanged.
- Produces root runtime-config functions `load_registry(path: &Path) -> Result<Registry>` and `save_registry_atomic(registry: &Registry, path: &Path) -> Result<()>`.
- Produces `ovrcr_terminal::encode_paste(text: &str, bracketed: bool) -> Vec<u8>` and `ovrcr_terminal::vt100`.
- Produces `crate::server::DispatchMessage` with unchanged variants; protocol no longer exports this runtime type.

- [ ] **Step 1: Capture baseline inventory without inventing behavior tests.**

Run `rtk proxy cargo test --all-targets --all-features -- --list` once, writing output to the task scratch directory. Record every target/test count and intentional ignored cases. Read current config/protocol/session/context and paste tests. Confirm HEAD before editing. Do not run the existing full suite again merely to generate another baseline.

- [ ] **Step 2: Add the two actual foundation members and shared dependency declarations.**

Use this manifest structure (runtime/TUI members are added by their own tasks):
```toml
[workspace]
members = [".", "crates/ovrcr-protocol", "crates/ovrcr-terminal"]
default-members = [".", "crates/ovrcr-protocol", "crates/ovrcr-terminal"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
ovrcr-protocol = { path = "crates/ovrcr-protocol" }
ovrcr-terminal = { path = "crates/ovrcr-terminal" }
# Move the exact existing third-party version/features declarations here.
# Keep optional=true at the consuming package, not in this shared table.
```
Each new crate inherits version/edition and declares `publish = false`. Protocol consumes anyhow, bincode/serde, serde, serde_json; terminal consumes vt100. Do not put directories/toml/runtime dependencies in protocol. Root still needs existing deps until later moves; defer final pruning to Task4.

- [ ] **Step 3: Move shared types, codec and pure behavior verbatim; break the cycles.**

Protocol layout:
```rust
// crates/ovrcr-protocol/src/lib.rs
pub mod context;
mod codec;
mod registry;
mod session;
mod wire;
pub use codec::{MAX_FRAME_BYTES, read_frame, write_frame};
pub use registry::{ProjectRecord, Registry, WorkspaceRecord};
pub use session::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
pub use wire::*;
```
Move SessionId/TerminalSize/SessionPhase/AgentActivity/SessionSummary only from session; keep all runtime state/events there. Move protocol wire declarations unchanged, except DispatchMessage to server/dispatch.rs. Re-export it from server, update server/unit/integration imports. Move frame functions/tests into codec; fix their imports to the actual protocol root. Move context implementation/tests unchanged, and pure Registry lookup/mutation/validation to registry. Preserve serde field order/defaults and redacted Debug implementations.

Root aliases are thin:
```rust
pub use ovrcr_protocol as protocol;
pub use ovrcr_protocol::context;
// src/session.rs retains runtime code, and:
pub use ovrcr_protocol::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
```

- [ ] **Step 4: Separate Registry persistence and shared paste without changing behavior.**

In config, re-export data types, retain RegistryPath and all file/atomic code. Convert inherent I/O methods to free functions with the exact same bodies, changing Self to Registry and self to registry only where necessary:
```rust
pub fn load_registry(path: &Path) -> Result<Registry>;
pub fn save_registry_atomic(registry: &Registry, path: &Path) -> Result<()>;
```
These signatures describe free functions implemented by relocating the existing complete load/save bodies verbatim. Update call sites: `Registry::load(&path)` becomes `load_registry(&path)`; `registry.save_atomic(&path)` becomes `save_registry_atomic(&registry, &path)`. Keep pure config tests with registry where practical and filesystem cases with runtime config.

Move the exact current encode_paste body to terminal/paste.rs; terminal lib re-exports it and `pub use vt100;`. TUI may re-export encode_paste to preserve callers. Runtime send_text calls terminal directly. Runtime/TUI VT100 imports use `use ovrcr_terminal::vt100;`; no wrapper/new parser behavior.

- [ ] **Step 5: Verify, self-review, commit.**

Run full check, formatting, `rtk proxy cargo test -p ovrcr-protocol -p ovrcr-terminal`, `rtk proxy cargo test -p ovrcr --lib config::`, and `rtk proxy cargo test -p ovrcr --test tui` (contains existing paste coverage). Record nonzero counts, diagnostics and exit codes. Inspect moved serialization definitions against base for exact field/variant order and inspect Cargo.lock for dependency churn. Run `rtk git diff --check`. Commit only task files: `refactor: extract protocol and terminal crates`. Do not add scratch reports.

### Task 2: Extract runtime and separate lifecycle, transport and startup modules

**Files:**
- Create `crates/ovrcr-runtime/Cargo.toml`, `src/lib.rs`, `src/config.rs`, `src/git.rs`, `src/session/{mod,process,io,tests}.rs`, `src/server/{mod,dispatch,outbound,connections,startup,tests}.rs`.
- Move source bodies from root config/git/session/server and Task1 server/dispatch; remove old source copies.
- Create root `src/client.rs`; modify root `src/lib.rs`, `src/main.rs`, `src/gui.rs` and manifests/lock.
- Mechanically update existing tests only if imports require it; root integration tests stay in place.

**Interfaces:**
- Consumes Task1 shared data/codec/context and terminal::encode_paste/vt100.
- Produces `ovrcr_runtime::{config,git,session,server}` modules preserving existing public runtime API except ownership relocation.
- Produces `ovrcr_runtime::server::DispatchMessage`.
- Produces application `client::connect_if_running(paths: &ServerPaths) -> Result<Option<UnixStream>>`, `client::connect_or_start(paths: &ServerPaths) -> Result<UnixStream>`; signatures and behavior unchanged.

- [ ] **Step 1: Declare actual runtime dependencies and split source by responsibility.**

Add runtime member/default-member/path dependency. Runtime depends on protocol, terminal, anyhow, directories, libc, portable-pty, signal-hook, toml; include serde/serde_json only if an actual runtime use remains. tempfile is dev-only. It must not depend on TUI, Ratatui, Crossterm, clap or eframe.
```rust
// crates/ovrcr-runtime/src/lib.rs
pub mod config;
pub mod git;
pub mod server;
pub mod session;
```
Move config and git with private tests. Rewrite internal protocol/context imports to direct foundation dependencies. Unit tests retain access through their private parent, not public implementation fields.

- [ ] **Step 2: Split session without changing critical sections.**

`session/mod.rs`: runtime types/Session fields, ReportOrder and Session impl. `process.rs`: verify_group_identity, verify_owned_group, should_signal_group, signal_group, group_exists, wait_for_group_exit, preserving every cfg and deadline. `io.rs`: read_pty and wait_for_child. `tests.rs`: current cfg(test) module contents, unchanged assertions.
```rust
mod io;
mod process;
use io::{read_pty, wait_for_child};
use process::*;
#[cfg(test)]
mod tests;
```
Child helpers may be `pub(super)` only as needed by parent; keep Session fields private. Use a non-conflicting alias for std::io if the new io child collides. Keep spawn_with_ready runtime-visible, not public merely for organization.

- [ ] **Step 3: Split server by ownership and move application startup out.**

`server/mod.rs`: ServerState, geometry, lifecycle resource mutation and snapshot state; small error helpers alongside callers.
`dispatch.rs`: DispatchMessage, bridge_events, run_dispatcher, dispatch_* and geometry ownership helpers.
`outbound.rs`: DashboardOutbound/Queue/Sink/Delivery/Slot/Snapshot, bounded sink impl, dashboard_send/try_send/disconnect helpers.
`connections.rs`: handle_connection, handle_request_with_id, request responses/errors/shutdown routing.
`startup.rs`: ServerPaths, socket resolution/validation/capability generation/startup lock, run_server, accept wake.
`tests.rs`: old private test module, helpers accessible through parent private re-exports as needed.
Move each full function/impl unchanged; only narrow imports/visibility change. Preserve existing public server APIs through re-exports:
```rust
pub use dispatch::{DispatchMessage, run_dispatcher};
pub use startup::{ServerPaths, run_server};
// Re-export the existing public queue/sink types and constants from their new owners.
```
Avoid duplicate functions or public wildcard exports of private internals.

Move only connect_if_running/connect_or_start to root client with existing OVRCR_SERVER_EXECUTABLE/current_exe, setsid and five-second wait behavior. Root lib compatibility:
```rust
pub mod client;
pub use ovrcr_runtime::{config, git, session};
pub mod server {
    pub use ovrcr_runtime::server::*;
    pub use crate::client::{connect_if_running, connect_or_start};
}
```
Root CLI calls client explicitly. Root GUI can retain facade paths. Neither runtime nor TUI imports application code.

- [ ] **Step 4: Verify runtime ownership and behavior, self-review, commit.**

Run full check, formatting, `rtk proxy cargo test -p ovrcr-runtime --lib`, and `rtk proxy cargo test -p ovrcr --test server_lifecycle --test resource_cli`. Preserve real process cleanup evidence and report intentional ignores. Inspect relocation diff for non-mechanical behavior changes and check `rtk proxy cargo tree -p ovrcr-runtime --edges normal` for no UI/app dependencies. Run diff-check and commit `refactor: isolate runtime ownership and modules`.

### Task 3: Extract TUI state, input, rendering and event loop

**Files:**
- Create `crates/ovrcr-tui/Cargo.toml`, `src/lib.rs`, `src/dashboard/{mod,state,input,render,event_loop,terminal_guard,tests}.rs`.
- Replace root `src/tui.rs` with `pub use ovrcr_tui as tui;` in root lib.
- Update root manifest/lock and TUI imports; root integration tests remain.
- Tests: existing root `tests/tui.rs`, `tests/terminal_acceptance.rs`, moved TUI private tests.

**Interfaces:**
- Consumes protocol message/session/context types and terminal::vt100/encode_paste directly.
- Produces same public TUI API: Dashboard, TreeRow, InputMode, DashboardAction, KeyEncoding, TerminalGuard, run_dashboard, draw_dashboard/draw_dashboard_at, render_terminal, encode_key/encode_paste, event_to_request, dashboard_message_channel, actual_drawn_inner_rect and DASHBOARD_READER_QUEUE_CAPACITY.
- No runtime or application dependency.

- [ ] **Step 1: Declare TUI package and move shared state to its parent module.**

TUI consumes protocol, terminal, anyhow, crossterm, ratatui, libc, signal-hook only where used; tempfile dev-only if actual tests use it. Move shared Dashboard fields/types to dashboard/mod.rs so children can access parent private fields. Re-export public surface explicitly through lib; do not enlarge public fields.

```rust
// src/lib.rs
mod dashboard;
pub use dashboard::{
    Dashboard, DashboardAction, InputMode, KeyEncoding, TreeRow, TerminalGuard,
    DASHBOARD_READER_QUEUE_CAPACITY, actual_drawn_inner_rect, dashboard_message_channel,
    draw_dashboard, draw_dashboard_at, encode_key, event_to_request,
    render_terminal, run_dashboard,
};
pub use ovrcr_terminal::encode_paste;
```

- [ ] **Step 2: Move complete behavior blocks to focused children.**

- state.rs: Dashboard impl and tree/selection/state transitions.
- input.rs: encode_key, browse/cursor/function helpers, event_to_request; uses terminal::encode_paste.
- render.rs: colors/layout constants, draw_dashboard*, render_terminal/color mapping, tree text/line helpers, clipping and elapsed formatting, pane_size/actual_drawn_inner_rect where their callers require.
- terminal_guard.rs: TerminalGuard and Drop/raw-mode restoration.
- event_loop.rs: run_dashboard, dashboard_loop, reader channel, input batch/idle wake/poll, mouse-capture state, transport read/write, terminal_size.
- tests.rs: current inline TUI tests, preserving private access via parent.
Shared constants/helpers belong to their owning child with `pub(super)` and explicit parent imports only when needed. Preserve exact literal spacing/glyph/color/status and frame/idle timing. Keep whole critical cleanup/error branches intact.

- [ ] **Step 3: Verify rendering and terminal behavior, self-review, commit.**

Run full check, formatting, `rtk proxy cargo test -p ovrcr-tui --lib` and `rtk proxy cargo test -p ovrcr --test tui --test terminal_acceptance`. Existing private tests may include intentionally caught panic; report it accurately. Check `rtk proxy cargo tree -p ovrcr-tui --edges normal` contains no runtime/application/PTY dependency. Run diff-check and commit `refactor: separate dashboard state input and rendering`.

### Task 4: Split application commands and refresh workspace handoffs

**Files:**
- Create `src/cli/{mod,args,resources,report,output}.rs`; reduce `src/main.rs`.
- Modify root `Cargo.toml`, `Cargo.lock`, `justfile`, `README.md`, `scripts/gui.sh` only if workspace selection requires it.
- Update these six unstarted plans: `plans/2026-09-05-historical-scrollback.md`, `copy-mode.md`, `split-panes.md`, `terminal-mouse-forwarding.md`, `multiple-dashboards.md`, `session-restore.md` (same date prefix).
- Keep root `src/report.rs`, `src/client.rs`, `src/gui.rs`, `src/bin/ovrcr-gui.rs`; mechanical imports only where needed.
- Existing tests: all workspace/all-targets including root CLI/resource/server/PTY and feature-gated GUI.

**Interfaces:**
- Consumes accepted crates and root report/client helpers.
- Produces thin executable entry `mod cli; fn main() { cli::main(); }`.
- CLI children use parent-private RuntimeError/AppResult and exact current command/output functions.
- Produces updated path/test-command handoffs for all six future features.

- [ ] **Step 1: Move CLI definitions and full handlers without behavior changes.**

`cli/mod.rs`: current main entry body as `pub(crate) fn main()`, run dispatch, RuntimeError/AppResult/error conversion, request helpers and inspection orchestration.
`args.rs`: Clap structs/enums/argument parsers and path resolution; types/fields `pub(super)` only as parent/sibling use requires. `Cli::parse()` stays in entry.
`resources.rs`: run_project/run_workspace/run_terminal/create_terminal/inspect_session_context and resource lookup helpers.
`report.rs`: run_report and all run_report_* handlers, calling application `ovrcr::report` transport (avoid shadowing with explicit alias).
`output.rs`: project_value/workspace_value/terminal_value, print_* and legacy projection/output/error functions.
Move full functions and use parent imports explicitly. Code examples:
```rust
// src/main.rs
mod cli;
fn main() { cli::main(); }

// src/cli/mod.rs
mod args;
mod output;
mod report;
mod resources;
use args::*;
use output::*;
use resources::*;
```
No new generic command framework, trait or output schema. Keep one observation timestamp per output operation.

- [ ] **Step 2: Finish manifest ownership and workspace-aware commands.**

Final members/default-members contain all five packages. All unchanged third-party versions are declared once in workspace.dependencies; consuming packages inherit with workspace=true and correct optional/dev flags. Remove root normal dependencies only when actual root code no longer consumes them; root GUI still uses portable-pty/vt100/libc where present. Root tests may legitimately retain dev dependencies.

Update just check/test/lint/build to explicitly cover workspace as appropriate; `run`/`run-release` select `-p ovrcr`. Keep all existing recipes and positional argument behavior:
```make
check:
    rtk proxy cargo check --workspace --all-targets

test *args:
    rtk proxy cargo test --workspace "$@"

run *args:
    rtk proxy cargo run -p ovrcr -- "$@"
```
GUI script must still build and copy `target/debug/ovrcr` and `target/debug/ovrcr-gui`; use `-p ovrcr` only if needed. No new CI provider/config when none exists.

- [ ] **Step 3: Update architecture docs and future task handoffs to actual files.**

README adds a concise package responsibility/dependency map and workspace commands, preserving roadmap states. Each unstarted plan gets current crate/module paths, direct imports, new package-specific unit commands and root executable integration commands. Map conceptual `src/session.rs` to runtime/session/mod.rs, `src/server.rs` to the relevant runtime server child, wire types to protocol/wire.rs, pure terminal history to terminal, TUI state/input/render to its actual child, CLI to its actual child. Do not blindly replace every server/TUI path with mod.rs.

History data transferred over wire belongs protocol; FrozenHistory/VT100 implementation belongs terminal. Runtime owns ring/PTY state; TUI owns viewport/copy/panes. Multiple-dashboard connection handling belongs runtime/connections/outbound/dispatch; restore persistence belongs runtime/config/session with shared saved record shapes in protocol only if crossing boundary. Preserve all future feature requirements, exact constants, acceptance tests and deferred ordering; this task implements none of them.

Document open scheduling/command-palette integration ownership without merging or editing those branches. Completed plans remain historical records.

- [ ] **Step 4: Run final migration gates once, reconcile inventory, self-review and commit.**

Run formatting and full check; then `rtk proxy cargo test --workspace --all-targets --all-features` once with required IPC/PTY permissions and a bounded outer timeout. Reconcile named tests with Task1 baseline (crate/module prefixes may change, test bodies may not disappear). Run `rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings` once; if it fails, report actual diagnostics/provenance and ask controller about any behavior/out-of-scope fix rather than claiming pass or cleaning broadly. Run `rtk proxy just --list`, inspect cargo tree normal edges and check dependency versions unchanged. Run `rtk git diff --check` and commit `refactor: organize application commands and workspace workflow`.

Controller performs Linux workspace verification and a native disposable GUI smoke using real keyboard input, detach/reattach and cleanup before final migration acceptance; worker does not open GUI apps or run parallel Docker tests. Final review sees all four task reports and these gates.
