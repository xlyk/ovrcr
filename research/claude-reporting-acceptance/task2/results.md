# Task 2 implementation evidence

Implemented the provider-independent binding/reporting foundation only. Provider admission, adapters, launcher, collection, setup and UI presentation remain later tasks. Claude foreground admission, final settling and full native accounting remain unverified Task 1 dependencies. Independent review of commit17548373 found no blocking findings; see review.md.

## Final verification

The final source tree is the Task 2 code in the accompanying commit. Attempts recorded `revision` before that commit; those values identify the base HEAD with the then-current uncommitted Task 2 source, not an assertion that base HEAD already contained the implementation. Source was not edited after delivery gates 33–37. Trailing blank lines in captured logs were trimmed after staging identified EOF whitespace; substantive output is preserved.

- `33-delivery-gate.txt`: `rtk proxy cargo test -p ovrcr-protocol -p ovrcr-runtime --lib` — protocol 22 passed; runtime 90 passed; zero failures. Ran with permission for owned PTY/socket fixtures.
- `34-delivery-gate.txt`: affected protocol/runtime Clippy with `-D warnings` passed.
- `35-delivery-gate.txt`: workspace check with all targets and all features passed.
- `36-delivery-gate.txt`, `37-delivery-gate.txt`: formatting and diff checks passed.
- `16-gui-authorized-retry.txt`: the isolated `command_palette_opens_when_another_control_has_focus` test passed (1 executed) with socket permission. This is that automated fixture test, not native Claude/dashboard acceptance.
- The broad workspace attempt in `14-workspace-regression.txt` stopped at GUI fixture startup because Unix socket binding was denied. The retained log is copied in `14-gui-environment-cause.txt`; its failed attempt is preserved. Only the failing test was rerun with permission; the rest of the full workspace regression was not rerun in Task 2.

## Behavioral failure evidence

- `02-protocol-behavioral-red.txt`: one executed test failed because ReserveAgent was an unknown wire variant.
- `03-runtime-red-attempt.txt`: two executed dispatcher tests failed at the explicitly unimplemented supervisor endpoint.
- `05-supervisor-behavioral-red.txt`: dedicated supervisor connection test failed while four other tests passed.
- `08-decrease-behavioral-red.txt`: decreasing known totals after an unknown sample were incorrectly accepted.
- `19-orphan-reservation-red.txt`: connection loss before bind/Hello retained an active reservation.
- `26-legacy-reservation-red.txt`: reservation incorrectly reset existing legacy Busy state.
- `32-legacy-publication-red.txt`: legacy takeover changed the projection without publishing an event when the prior legacy value matched.

The macro error in attempt 01, the invalid test-construction error and expected wire-snapshot mismatch in attempt 07, and Clippy failures in attempts 13/15 are not behavioral RED proof. They remain separate from passing attempts.

## Resolved shared contracts

- Wire version is 5. `AgentUpdate::Provider` carries independently revised activity/metrics/health observations. Metrics values are boxed only to bound enum sizes; wire encoding remains transparent. New request/result variants have pinned bincode snapshots and round-trip tests; capabilities and leases have redacted Debug output.
- `ReserveAgent` must arrive on a watched persistent connection. The runtime installs its connection token atomically with allocation. A matching active reservation retry returns the existing lease and attaches that connection; late cleanup from a replaced socket cannot release it. Once released, exact reservation retry returns `Released`, never another allocation. A fresh operation must use the retained epoch. No orphan timeout is used.
- Supervisor operations are `Request::Supervisor(SupervisorRequest)` with `AgentCommand::{Bind, Finalize, Release, Health}`. Bind and Release compare the exact expected current binding. Finalize publishes validated final metrics and releases atomically. A→B→A advances generation each time; generation and reservation epoch survive release throughout the PTY lifetime.
- Bounded retry storage retains the current epoch's reservation receipt and latest successful supervisor mutation, including finalization/release acknowledgement recovery until the next reservation. Old requests fail their retained epoch/binding/revision guards. Operation IDs are private caller-generated identities, limited to 256 UTF-8 bytes; the launcher must generate unpredictable IDs and never expose them to native children. Status/retry authenticates the original lease or the original reservation capability plus matching private operation and arguments.
- `SupervisorHello` reconnects a known lease without changing activity or metrics. Ordinary one-shot report connection closure is not a health event. Supervisor connection loss releases ownership, marks unfinalized usage partial and health unavailable, and preserves activity. Explicit health updates require the supervisor lease and current binding, not the hook capability alone.
- Identified component measurements require source identity plus a positive certified `source_sequence`. Component sources stay fixed within a binding. Lower sequence is stale; equal sequence must match identity and value and preserves age; higher sequence must identify a new measurement. Uncertain measurements omit identity/order, never prove a fresh measurement, and do not erase the last identified watermark when clearing a displayed value. No callback-arrival ordinal or historical ID cache is invented.
- Context, token usage and cost retain separate ages. The outer transport receipt is separate. Atomic metrics validation occurs before any watermark or visible value changes. Same-binding/scope decreases are rejected, including across unknown samples. No verified provider correction path exists yet, so no correction bypass is implemented. Context occupancy may decrease.
- Existing legacy activity/context stay available and unchanged during an unbound reservation. An active binding becomes the sole projection and excludes legacy updates. Legacy takeover after release publishes even if its prior legacy value matches. PTY exit preserves managed activity/last metrics, marks unfinished accounting partial and revokes all subsequent reporting.

## Test entry points and boundaries

Nine new server tests use the actual synchronous dispatcher; socket tests use `handle_connection` over UnixStream pairs. They reuse the existing server/session fixture helpers. Cases include authentication, stale reserve/bind/release, dropped reservation completion, same-connection and replacement-connection retries, A→B→A, independent revisions, component replay/conflict/unknown clearing, source changes, collector loss, delayed attachment, finalization retry/failure, post-exit rejection and legacy projection publication. A deterministic source-age unit test uses explicit times to distinguish source age from transport receipt. Owned fixture PID/PGID creation and kernel absence checks are recorded in the nocapture attempts.

Provider event causality, native foreground ownership, post-Stop settling, accounting-source completion, launcher finalization deadlines and native hook privacy remain later-task responsibilities. No provider claims are inferred from these normalized transport tests. Coordinator-owned design/support/checkpoint files and Task 1 live attempts are excluded from this code unit. Work diary closeout belongs to the coordinator.
