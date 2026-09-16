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

Project, workspace, and retained interactive-session metadata use SQLite as the
sole writable store. The database path is the full `config.toml` path with
`.sqlite3` appended. Initial migration imports legacy projects/workspaces;
schema version 2 adds session metadata without changing settings or task storage.
After each migration transaction commits,
later starts and every save use SQLite only. Offline readers open the database
read-only. They read preserved TOML only if the database is absent, or both its
application ID and version are zero and it has no user objects. That uninitialized
state can be retried on startup. A foreign, nonempty unversioned, or unsupported
database fails without replacement and without falling back to TOML.
`dashboard.toml` and the `config.tasks` directory stay outside this store.
`retained::SessionStore` owns durable session IDs, per-row run identities, title
metadata, original directory and launch kind, and boot-aware recovery state.
Current process runs remain `Arc<Session>` values; inactive rows allocate no PTY
or terminal parser. The existing mutation lock reserves one of fifty live slots
from admission through spawn publication. Reopening retires the prior run's
capability, view and history; run-tagged events and snapshots cannot affect its
replacement. No argv, environments, output, prompts, live activity or timing are
stored. Native provider resume and archive behavior remain later work.
