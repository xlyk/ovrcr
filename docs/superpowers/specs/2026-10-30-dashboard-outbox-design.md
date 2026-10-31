# Dashboard Outbox owns fully formed requests

Status: specification published as [GitHub issue #383](https://github.com/xlyk/ovrcr/issues/383),
after Kyle confirmed the primary test seam on 2026-10-31. Implementation was
authorized through `/implement` under [ticket #384](https://github.com/xlyk/ovrcr/issues/384).
The [implementation plan](../plans/2026-10-31-dashboard-outbox.md) records its
reconciled base and verification; the design-stage observations below remain historical.

## Intent and agreed scope

Make the existing Dashboard outbound-request module deep enough that callers no
longer coordinate a queue and a second stream of action-carried requests.

Kyle selected architecture candidate 1 and confirmed:

- Include request-order defects within this path only when a failing regression
  demonstrates the defect before its correction.
- Unify fully formed requests. Keep fresh-input authorization, clipboard effects
  and startup handling at their existing seams.

Success means one owner for producing and draining those ordered requests, fewer
queue/action conversions, and the same visible behavior and wire contract except
for independently reproduced, scoped ordering fixes.

Source surveyed: `0e63a8c370533001c96132ffca0e4914c95d011b`.
The ownership problem also survives the subsequently observed cached `07b792fb`.
Reconcile this design with the implementation base before editing product code.

## Approaches

| Approach | Tradeoff | Decision |
| --- | --- | --- |
| Unify fully formed steady-state requests in the existing Outbox responsibility | Removes duplicate request representations and caller coordination; retains a separate late-input send step. | Kyle chose A. |
| Also move fresh-input intent and authorization into outbound ownership | Greater consolidation, but changes the timing-sensitive input seam and expands regression scope. | Not selected. |
| Expand to clipboard, other terminal effects and startup handshake | Crosses different effects and lifecycle phases without being needed for the selected goal. | Excluded. |

## Ownership and interface

The owning module is the Dashboard's existing outbound-request responsibility,
implemented with `dashboard/outbox.rs` and its private Dashboard methods. Reuse
the current queue, request IDs, history scheduler, navigation fences and writer;
introduce no dependency, transport, framework or general effects abstraction.

Steady-state producers of preconstructed `ClientMessage` requests enqueue them
once. The event-loop adapter drains and transports the resulting order. History,
palette, lifecycle and Server-message follow-ups must not drain requests into
`DashboardAction::Request` / `RequestBatch` and later unpack or requeue them.
Remove those two action variants and migrate their repository callers and tests
together. The crate is unpublished (`crates/ovrcr-tui/Cargo.toml:7`); no parallel
compatibility path is required.

`DashboardAction` continues to describe remaining terminal actions such as raw
PTY bytes, clipboard text and detach. Newly authorized PTY input remains a
separate send step after queued requests flush. Startup Hello/Geometry also
remain separate. These are explicit limits of the chosen scope.

The interface is the public Dashboard event/key/message path plus its outgoing
drain, exercised through the actual event-loop writer. Queue storage is an
internal implementation detail. Exact private method names and source-file
placement are left to the builder.

## Behavior that must survive

1. **Causal order and exactly-once publication.** Previously queued synthetic
   releases and HistoryEnd requests lead the requests produced by the next action.
   Preserve batch order, every release, request correlation and error ownership.
   Removing the request action variants must remove conversions, not add a second
   way to send or an extra drain.
2. **History timing.** Drain is active work: it can resolve a cursor, schedule a
   page, mark it pending or set an error. Preserve its existing scheduling points,
   priority and suppression. In particular, putting an action's request into the
   queue must not silently move a due History page from before that request to
   after it. Characterize the current order through the real dispatcher first.
3. **Server-message timing.** Preserve follow-up delivery before the next Server
   message, navigation's bounded input-discard break, and ignored responses that
   currently return without draining. Do not replace these with an unconditional
   end-of-batch drain.
4. **Fresh input versus captured cleanup.** Keep late fresh-input authorization
   after queued requests flush. Cleanup retains its captured session, run, control
   epoch and view revision; never rebind it to the newly focused session. Keep
   navigation/control changes filtering unsent input without removing unrelated
   submitted operation targets.
5. **Other effects and startup.** Keep input-drain-before-clipboard emission,
   immediate CopyText ordering, failure/retry selection, and bounded input batches.
   Preserve startup Hello/Geometry IDs 1/2 and handshake interleaving. No new retry
   or replay on socket-write failure; retain the existing error/connection-exit
   behavior.
6. **New-session usability.** A new pane's SetView must precede the synced
   remembered-launch-choice save. Preserve the corresponding request/error IDs
   and caller-specific save-failure behavior.

These are preservation requirements, not claims that the current code has a
demonstrated defect. Any correction needs the agreed failing regression first.

## Reach

Request producers and conversion sites include `dashboard/state.rs`,
`palette.rs`, `lifecycle_ui.rs`, `assignment_control.rs`, `settings_editor.rs`,
`desktop.rs`, `events.rs`, `iterm.rs`, `quota.rs`, `operator.rs` and
`navigation.rs`. Change a caller only where its outgoing ownership requires it.
`event_loop.rs` remains the socket/terminal adapter. Preserve the public Dashboard
test entry points while updating assertions that inspect removed action variants.
Update `docs/development/tui.md` and the Outbox's module documentation to describe
the chosen scope accurately; keep the domain definition of Outbox unless its
wording needs clarification.

No Server, protocol, persistence, settings schema, key-binding or visible-layout
change is part of this refactor. Preserve the synchronous Server, one active
Dashboard and fifty-session contract. Unrelated defects remain separate findings.

## Verification contract

Use existing fixtures and observable Dashboard/socket behavior. Do not substitute
queue-helper tests for production dispatch or weaken existing assertions.

| Contract | Verification |
| --- | --- |
| Queue plus next-action order | Drive the actual event/key/message dispatcher with earlier cleanup, due History work and a newly produced request; inspect ordered socket frames and exactly-once delivery. |
| Retarget, captured authority and filtering | Retain release-before-SetView, multiple HistoryEnd, stale-input filtering, control-epoch and navigation cutover coverage. |
| History scheduling and errors | Cover a due page, pending-page suppression, deferred HistoryBegin and cursor-resolution failure through the Dashboard path. |
| Mixed terminal effects | Retain input continuation, clipboard cancellation/failure and Operator/terminal independence through the real writer. |
| Caller migration | Compile and exercise public Dashboard integration callers, including `crates/ovrcr-tui/tests/settings_theme_preview.rs`. |
| Real application path | Use an isolated real Server/Dashboard case for retarget cleanup and subsequent input; keep scoped native input/visible-outcome evidence from the reviewed checkout. |

Focused tests come first; affected suites and the repository's required final
regression/lint gates follow once. Every defect correction must show an actual
failing behavioral assertion on the original code and a passing result afterward.
Application/native checks have not been run during this design work. If CUA or a
required platform is unavailable, record that acceptance gap explicitly.

## Evidence anchors

Paths below are relative to `crates/ovrcr-tui/src/dashboard/` at the survey pin:

- `outbox.rs:1-27`, `state.rs:936-955,3518-3540`: queue/action conversions.
- `event_loop.rs:543-581`: flush-before-action and late input authorization.
- `state.rs:2113-2238,2903-2915`: active History scheduling and deferred begin.
- `palette.rs:3194-3222`, `lifecycle_ui.rs:361-383`: view before remembered save.
- `event_loop.rs:365-394,460-517,727-823`: message cutpoints, clipboard and startup.
- `tests.rs:810-954,1230-1370,2731-2843,4050-4171` and
  `event_loop.rs:1403-1630`: existing behavior coverage, inspected but not run.

## Published specification

Kyle requested `/to-spec` and approved the primary test seam. The resulting
[specification, issue #383](https://github.com/xlyk/ovrcr/issues/383), is published
with `ready-for-agent` and is the implementation contract. This local design
remains supporting background. No implementation plan, product edits, live
provider work, PR or merge was performed as part of specification publication.
