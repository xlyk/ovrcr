# Provider conversation recovery

This contract enables #117, #118 and #119 to develop against the shared reopen path. Claude is currently the only installed recovery adapter. Other providers remain unavailable until their own implementation and acceptance; a reference or reporting binding alone never advertises support.

## Shared boundary

- `ovrcr_protocol::ConversationReference` is a provider-tagged enum of nonsecret typed payloads. `provider()`, `identity()` and `matches_binding()` define exact ownership. Add verified provider payloads as appended variants; do not use an untyped map, newest-file lookup, resume-last or stored prompt.
- `ovrcr_runtime::recovery::{supported, unavailable, validate, resume_argv}` owns adapter selection. The adapter validates its own references and constructs managed argv without spawning. `unavailable` is the metadata capability decision; filesystem checks stay in the explicit launch validation. Missing prerequisites produce a recorded failure through the existing server path.
- `Reporter::retain_conversation(reference, deadline)` uses the current certified binding. Receivers must first pass their existing producer/sequence and native-identity checks, then bind the accepted identity and retain it. A false return is not a durable acknowledgment. An accepted replacement supersedes pending retention from the prior generation. Retry operation IDs preserve the exact binding and reference.
- `Reporter::invalidate_conversation(deadline)` permanently closes recovery for an unsupported transition in the current invocation. It cancels pending retention, retries failed persistence, and preserves the existing lost-bind receipt handling. Supported Pi/OMP changes will use bind + retain, not invalidation.
- `RetainConversation` and `InvalidateConversation` supervisor commands use the existing lease and current session/run. Retention checks the exact provider, identity and binding generation before the shared durable write. Successful receipts are cached only after persistence. A retired lease cannot change recovery metadata.
- SQLite schema 5 owns one `agent_conversations` table. Schema 3 from archive mainline and the earlier Claude draft is distinguished by table/column markers; schema 4 from the interface amendment is also accepted. Migration preserves archive disposition, exact references and invalidation flags; offline reads remain read-only. Do not add a provider-specific store. Protocol 17 adds the reference tag.
- `ServerState` remains the only process owner. All launches use the existing reopen operation, mutation lock, capacity admission, boot acknowledgment, run fencing and production spawn. Inventory reads never launch work. Current reporting resets on reopen; `recovery.attached` requires a matching provider and identity from the new invocation.

## Display-triggered recovery (protocol 18)

`Request::RecoverSession { session, expected_run }` is the automatic counterpart
to explicit `ReopenSession`. The Dashboard emits it once per displayed eligible
run using the same nonzero pane geometry as `SetView`. Sidebar inventory and hidden
pane assignments are not displays. `SessionSummary::can_auto_recover` shares the
eligibility predicate: interrupted Agent, unarchived, supported reference, no
ownership acknowledgement required and no recorded failure.

The server rechecks eligibility under the existing mutation lock and calls the
same locked reopen implementation. It records pre-spawn failures, including full
capacity, so reconnects cannot retry them. Explicit reopen remains Retry; automatic
requests cannot acknowledge stopped processes. Natural Agent exits also retain an
explicit-action diagnostic in the existing recovery failure field; they never mark
process ownership as stopped. This prevents a later reboot from making a completed
conversation look like interrupted work. Duplicate old-run requests reuse the
successor, while archive/unarchive and stale-run fences still apply. Hierarchy events
install replacement runs; late automatic receipts cannot retarget panes or clear a
new pane's error. No persistence schema or provider registration changes are needed.

Native automatic-recovery acceptance belongs to #127, separate from development.

## Parallel work ownership

| Ticket | Owns | Uses without duplicating |
| --- | --- | --- |
| #117 | Dashboard display trigger, request coalescing, race handling and automated tests (#127 owns native acceptance) | Existing recovery availability and reopen request; no provider detection in the Dashboard |
| #118 | Codex authoritative capture and resume adapter; Codex-specific tests | Reporter retention, generic reference store and shared reopen |
| #119 | Pi/OMP accepted identity changes in the shared extension receiver, separate native adapters and acceptance | Reporter producer fences, retention and shared reopen |

The common base includes archive prerequisite #115 from `c633103`. Use separate worktrees from the merged common base. Shared enum additions, adapter registration, protocol snapshots/version and capability documentation require a single integration owner: agree appended variant order and land each registration sequentially. Provider modules and their tests can proceed concurrently. Do not independently redesign the shared contract or persistence schema. #117 needs no changes to adapter registration.

Native Claude continuity and macOS GUI input from #116 are verified on `8347563`; automated macOS/Linux, capacity and memory checks passed on that revision. See the [acceptance record](../../research/issue-116-claude-recovery/native-continuity-20260917/README.md) for the two-restart proof and platform boundaries. Native Linux GUI was not exercised. Downstream tickets require their own provider acceptance; this evidence does not authorize additional paid runs or a merge. The existing limitation for persistent storage failure followed by owner death before invalidation commits remains unchanged.
