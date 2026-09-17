# Provider conversation recovery

This contract enables #117, #118 and #119 to develop against the shared reopen path. Claude, Pi and Oh My Pi have installed recovery adapters. Other providers remain unavailable; a reference or reporting binding alone never advertises support. Pi/OMP native acceptance is still required independently in #129/#130.

## Shared boundary

- `ovrcr_protocol::ConversationReference` is a provider-tagged enum of nonsecret typed payloads. `provider()`, `identity()` and `matches_binding()` define exact ownership. Add verified provider payloads as appended variants; do not use an untyped map, newest-file lookup, resume-last or stored prompt.
- `ovrcr_runtime::recovery::{supported, unavailable, validate, resume_argv}` owns adapter selection. The adapter validates its own references and constructs managed argv without spawning. `unavailable` is the metadata capability decision; filesystem checks stay in the explicit launch validation. Missing prerequisites produce a recorded failure through the existing server path.
- `Reporter::retain_conversation(reference, deadline)` uses the current certified binding. Receivers must first pass their existing producer/sequence and native-identity checks, then bind the accepted identity and retain it. A false return is not a durable acknowledgment. An accepted replacement supersedes pending retention from the prior generation. Retry operation IDs preserve the exact binding and reference.
- `Reporter::invalidate_conversation(deadline)` permanently closes recovery for an unsupported transition in the current invocation. It cancels pending retention, retries failed persistence, and preserves the existing lost-bind receipt handling. Supported Pi/OMP changes use bind + retain, not invalidation. Their nullable native history path records an ephemeral replacement as unavailable instead of leaving the old conversation eligible.
- `RetainConversation` and `InvalidateConversation` supervisor commands use the existing lease and current session/run. Retention checks the exact provider, identity and binding generation before the shared durable write. Successful receipts are cached only after persistence. A retired lease cannot change recovery metadata.
- SQLite schema 5 owns one `agent_conversations` table. Schema 3 from archive mainline and the earlier Claude draft is distinguished by table/column markers; schema 4 from the interface amendment is also accepted. Migration preserves archive disposition, exact references and invalidation flags; offline reads remain read-only. Do not add a provider-specific store. Protocol 18 appends Pi (tag 1) and OMP (tag 2) after Claude (tag 0); subsequent provider registrations must append rather than reorder these tags. No database migration is needed.
- `ServerState` remains the only process owner. All launches use the existing reopen operation, mutation lock, capacity admission, boot acknowledgment, run fencing and production spawn. Inventory reads never launch work. Current reporting resets on reopen; `recovery.attached` requires a matching provider and identity from the new invocation.

## Parallel work ownership

| Ticket | Owns | Uses without duplicating |
| --- | --- | --- |
| #117 | Dashboard display trigger, request coalescing, race handling and GUI acceptance | Existing recovery availability and reopen request; no provider detection in the Dashboard |
| #118 | Codex authoritative capture and resume adapter; Codex-specific tests | Reporter retention, generic reference store and shared reopen |
| #119 | Pi/OMP accepted identity changes in the shared extension receiver, separate native adapters and acceptance | Reporter producer fences, retention and shared reopen |

The common base includes archive prerequisite #115 from `c633103`. Use separate worktrees from the merged common base. Shared enum additions, adapter registration, protocol snapshots/version and capability documentation require a single integration owner: agree appended variant order and land each registration sequentially. Provider modules and their tests can proceed concurrently. Do not independently redesign the shared contract or persistence schema. #117 needs no changes to adapter registration.

Native continuity, GUI input and Linux acceptance from #116 remain open. This interface amendment does not satisfy those gates, authorize paid provider runs, or authorize a merge. The existing limitation for persistent storage failure followed by owner death before invalidation commits remains unchanged.
