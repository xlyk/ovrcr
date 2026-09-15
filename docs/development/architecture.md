# Architecture

| Location | Responsibility |
| --- | --- |
| `crates/ovrcr-protocol` | Shared wire types, validation, and framing. |
| `crates/ovrcr-terminal` | Terminal parsing, encoding, and screen/history primitives. |
| `crates/ovrcr-runtime` | Server, PTY/session ownership, registry persistence, Git/workspaces, and task execution. |
| `crates/ovrcr-tui` | Dashboard state, input routing, rendering, and task UI. |
| Root `src/` | Application composition, CLI/client, reporting, service integration, and optional GUI helper. |

One Reporter (`src/report/reporter.rs`) owns every managed invocation's reporting lifecycle: lease ownership, bind receipts, the single revision set, publish-or-disable, Producer fencing and the identity budget, pause and recovery, and teardown. `report/extension.rs` (Pi, Oh My Pi), `report/codex.rs`, and `report/admission.rs` (Claude) are frame receivers: they translate one provider's frames into observations and hold nothing else. Each receiver chooses which lifecycle parts its provider needs and they differ — only the extension receivers pause and recover, only Claude finalizes its accounting or retries a lost bind receipt, and only receivers with their own transport check the native root — so read the Reporter's own documentation for which is which. Add lifecycle behaviour to the Reporter, never to a receiver.

- Runtime and TUI depend on protocol/terminal primitives, not on each other. Keep root facades thin; put implementations in the owning crate.
- Keep modules focused and exports small. Reuse existing helpers and dependencies before adding abstractions or libraries. Do not introduce Tokio, an agent SDK, or a new crate without a concrete requirement in the approved design. SQLite is already approved for project and workspace metadata (#112 / #113).
- Update constructors, exhaustive matches, public re-exports, CLI output, and optional-feature callers together when shared types change. Which providers may produce Ready, Unread, review targets and alerts is decided once by `AgentProvider::supports_readiness`; never compare a provider literal at a consumer.

Root `src/` and control-role tests speak to the server through `ovrcr_protocol::client` for request/response round trips; dashboard-role code that consumes events reads frames directly. Do not hand-roll `ClientMessage` frames for a plain request.

Project and workspace metadata use SQLite as the sole writable store, per #112.
The database path is the full `config.toml` path with `.sqlite3` appended. First
server start creates the schema (`application_id` `0x4f565243`, `user_version` 1)
and imports leftover TOML in the same transaction. Later starts and every save
use SQLite only. Offline readers open the database read-only when it exists;
otherwise they read leftover TOML and do not migrate. An empty uninitialized file
at that path can still be initialized. A foreign, nonempty unversioned, or
unsupported database fails without replacement and without falling back to TOML.
`dashboard.toml` and the `config.tasks` directory stay outside this store.
Session retention and native resume are later work (#114–#120).
