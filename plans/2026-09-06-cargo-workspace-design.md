# Cargo workspace design

Approved by the user on 2026-09-06: “After this feature split things up like how you recommended”. Migrate after reviewed context usage and before historical scrollback. Base: `0a10c923dc061794bb1cfab8af698b0babb0219a`.

## Purpose

Give growing runtime and dashboard code clear ownership, focused modules, and dependency boundaries enforced by Cargo. Keep one repository, one lockfile, one target directory and the existing application executable. This is a behavior-preserving reorganization.

## Packages and dependencies

| Package | Responsibility | Local dependencies |
| --- | --- | --- |
| `ovrcr-protocol` | Shared session/config data, context values/validation, messages, frame codec | None |
| `ovrcr-terminal` | Existing VT100 implementation re-export and shared paste encoding; future pure history operations | None now; protocol only when needed |
| `ovrcr-runtime` | Server dispatch/connections, sessions, PTYs, process groups, Git workspaces and registry I/O | protocol, terminal |
| `ovrcr-tui` | Dashboard state, input, rendering, terminal guard and event loop | protocol, terminal |
| `ovrcr` (root) | CLI commands, server connection/startup, report helper transport and optional GUI helper | protocol, terminal, runtime, tui |

Arrows point from consumer to dependency. Runtime and TUI must not depend on one another or on the application. Protocol must not depend on terminal implementation. No new service, framework, async runtime, dependency version upgrade or crate per roadmap feature. Internal packages use `publish = false`; no crates.io publication is planned.

## Behavior and visibility contract

Preserve all CLI names/aliases/defaults, existing output and error text, environment names, executable names, socket paths, frame bytes, serde fields/defaults and enum ordering. Preserve lock acquisition order, queue bounds, admission/ordering, deadlines, spawn/cleanup and process-group ownership. Keep actual provider verification gaps and unchecked roadmap checkboxes explicit.

Shared types move without shape changes: SessionId, TerminalSize, SessionPhase, AgentActivity, SessionSummary; Registry, ProjectRecord, WorkspaceRecord; context report/snapshot and existing pure context helpers. Context adapter parsing remains pure and can stay with context helpers for this migration.

DispatchMessage and SessionEvent are runtime-internal process messages, not protocol data. DispatchMessage moves into runtime server/dispatch; SessionEvent remains session-owned. SessionSpec, HookEnvironment, ReportOrder, SessionState, Session and synchronization stay runtime-owned. Do not expose fields just to make tests compile.

Registry's pure lookup/mutation/validation methods live with its shared type. Make pure `Registry::validate(&self) -> anyhow::Result<()>` public for runtime I/O. Move file operations to `load_registry(path: &Path) -> Result<Registry>` and `save_registry_atomic(registry: &Registry, path: &Path) -> Result<()>`. Preserve atomic rename/fsync and diagnostic behavior. Do not add an extension trait or wrapper only to retain inherent method syntax.

Move `encode_paste(text: &str, bracketed: bool) -> Vec<u8>` into terminal and use it directly from runtime and TUI. Re-export pinned vt100 through terminal without a parser wrapper. Crossterm event encoding remains TUI-owned; PTY spawning remains runtime-owned.

Root library may provide thin re-exports of existing logical modules for current integration tests and helper consumers. No duplicate implementation and no backwards runtime dependency. Root `client` owns connect_if_running/connect_or_start. Root server facade may re-export those client helpers alongside runtime server APIs to keep existing consumer paths working.

## Module layout

- protocol: `lib.rs` re-exports, `session.rs`, `registry.rs`, `context.rs`, `wire.rs`, `codec.rs`. Codec tests stay private; no filesystem I/O in registry.
- terminal: `lib.rs`, `paste.rs`; VT100 stays the same dependency.
- runtime: `lib.rs`, `config.rs`, `git.rs`; `session/mod.rs` owns state/lifecycle, `session/process.rs` owns group helpers, `session/io.rs` owns reader/waiter helpers, `session/tests.rs` private tests; `server/mod.rs` owns state and resource mutation, `server/dispatch.rs` dispatcher and runtime message, `server/outbound.rs` dashboard queue/sink, `server/connections.rs` request routing/connection handling, `server/startup.rs` server/socket startup and locks, `server/tests.rs` private tests. Keep small helpers with their callers. Module internals can use narrowly scoped `pub(super)`.
- tui: `lib.rs` public re-exports; `dashboard/mod.rs` shared state root; `dashboard/state.rs`, `input.rs`, `render.rs`, `event_loop.rs`, `terminal_guard.rs`, `tests.rs`. Parent-owned private state remains visible to children; avoid public fields for sibling access.
- application: `src/main.rs` thin entry, `src/cli/mod.rs` dispatch/errors, `args.rs`, `resources.rs`, `report.rs`, `output.rs`; `src/client.rs` startup/client connection; existing `src/report.rs` hook transport and `src/gui.rs` optional GUI remain application modules.

The goal is responsibilities a worker can read independently, not an arbitrary line limit. Keep tightly coupled methods together and never split a critical section across a changed execution path.

## Tests, tooling and rollout

Keep executable integration tests in root `tests/`: Cargo's `CARGO_BIN_EXE_ovrcr` continues to resolve the application. Move unit tests with implementation as private children; preserve cases/assertions and test inventory. Existing behavioral tests validate the move; do not create artificial failing rename tests or tests of Cargo's cycle detection.

Use resolver 3 with edition 2024, explicit workspace membership/default-members for all five packages, shared dependency declarations and unchanged versions. Application-only optional GUI dependencies remain optional and do not activate through runtime/TUI.

Each extraction must compile and pass relevant existing tests before review. Use package-targeted unit gates while moving crates, then run the complete workspace/GUI/CLI/PTY gates after the final move. Preserve genuine ignored tests and identify them; zero matching tests is not a pass. Existing intentionally caught panic output must be recorded accurately. No claim of pristine Clippy unless it passes.

Update just commands to cover the workspace, preserve `just run`/`just gui` and GUI packaging paths, and refresh the six unstarted roadmap plans with actual package paths and test commands. Leave completed plans as historical records. Open scheduling and command-palette branches are not merged by this change; when integrated their execution belongs in runtime, shared types in protocol and UI in TUI.

Publish reviewed steps as stacked PRs; finish with one whole-migration review before proceeding to scrollback. Do not merge main or alter unrelated branches.

## References

[Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html), [packages and crates](https://doc.rust-lang.org/book/ch07-01-packages-and-crates.html), and [inherent implementation ownership](https://doc.rust-lang.org/reference/items/implementations.html#inherent-implementations).
