# Architecture

| Location | Responsibility |
| --- | --- |
| `crates/ovrcr-protocol` | Shared wire types, validation, and framing. |
| `crates/ovrcr-terminal` | Terminal parsing, encoding, and screen/history primitives. |
| `crates/ovrcr-runtime` | Server, PTY/session ownership, registry persistence, Git/workspaces, and task execution. |
| `crates/ovrcr-tui` | Dashboard state, input routing, rendering, and task UI. |
| Root `src/` | Application composition, CLI/client, reporting, service integration, and optional GUI helper. |

- Runtime and TUI depend on protocol/terminal primitives, not on each other. Keep root facades thin; put implementations in the owning crate.
- Keep modules focused and exports small. Reuse existing helpers and dependencies before adding abstractions or libraries. Do not introduce Tokio, a database, an agent SDK, or a new crate without a concrete requirement in the approved design.
- Update constructors, exhaustive matches, public re-exports, CLI output, and optional-feature callers together when shared types change.

Root `src/` and every test speak to the server through `ovrcr_protocol::client`; do not hand-roll `ClientMessage` frames.
