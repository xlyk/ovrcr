# Codex reporting follow-ups: implementation specification

Status: published as [GitHub issue #57](https://github.com/xlyk/ovrcr/issues/57). This document instructs future implementation; it does not authorize a merge, release, live configuration change or an expansion of provider access. Tracker label: `ready-for-agent` for this coordinating specification; downstream source-dependent implementation must retain its explicit blocked gates.

## Problem Statement

The user primarily wants to know when a Codex response is ready to review without continually watching its terminal. The hooks-only implementation provides a visible Ready observation, but it has not been merged or installed for normal use. It does not actively notify the user or distinguish reviewed responses. Broader version/launch support and metrics require additional evidence, and some reporting guarantees remain unavailable.

The old metrics plan contains historical unchecked tasks whose transport and setup portions overlap completed hooks work. Treating every historical checkbox as fresh implementation would duplicate working mechanisms and obscure the actual source blocker.

## Solution

Deliver the existing Ready feature first. Then add optional notifications and a small explicit unread workflow using the same authenticated root events and server snapshots. Extend compatibility one verified version or launch form at a time. Keep metrics and finality behind separate source-contract gates. Address local test reliability and additional platform/capacity acceptance as independent assurance work.

The baseline is the reviewed hooks implementation in PR #56 at revision 86ef46b8eff3128566f76022c2cfef49714031ac. At the recorded delivery checkpoint it was open, ready for review, mergeable, and all four current-head checks passed. Source acceptance covers exact Codex CLI 0.153.0, restricted fresh interactive launch, root prompt→Busy, matching root Stop→ResponseReady/Observed, Interrupt→Idle, next-prompt rebinding and retained observations with independent reporter health. This is a baseline to verify at execution time, not a requirement to rebuild it.

## User Stories

1. As a user, I want the accepted Ready feature available in my installed OVRCR, so that normal managed Codex sessions report response readiness.
2. As a user, I want installation to preserve unrelated configuration and hooks, so that enabling reporting does not disrupt my setup.
3. As a user, I want to review and trust reporting hooks through native Codex, so that hook execution remains under my control.
4. As a user, I want clear managed-launch instructions, so that I understand why a raw Codex launch is untracked.
5. As a user, I want doctor to distinguish accepted release support from effective hook delivery, so that a configuration check does not promise a working session.
6. As a user, I want a notification when a background root response becomes ready, so that I can return to it promptly.
7. As a user, I want notification preferences, so that I can choose visual alerts, sound or neither.
8. As a user, I want duplicate callbacks to produce at most one notification, so that reporting does not become noisy.
9. As a user, I want child completion to leave the parent notification unchanged, so that I am only interrupted for the root response I am waiting on.
10. As a user, I want interruptions and process exits to avoid success notifications, so that aborted work is not presented as a completed response.
11. As a user, I want reconnecting to avoid replaying old alerts, so that opening the dashboard does not create a notification burst.
12. As a user, I want alert content to identify the terminal without copying conversation text, so that private response content is not exposed on my desktop.
13. As a user, I want notification failures to leave Codex and its Ready label usable, so that optional alerts cannot block my work.
14. As a user, I want unread status separate from activity, so that marking something reviewed does not change the observed provider state.
15. As a user, I want an explicit mark-reviewed action, so that selecting a terminal does not silently acknowledge a response.
16. As a user, I want acknowledgement to apply to the response I saw, so that a newer response is not accidentally cleared by a delayed action.
17. As a user, I want unread status retained across dashboard reconnect while the server lives, so that reconnecting does not lose my review state.
18. As a user, I want the lifetime of unread state documented, so that I do not expect recovery after server death.
19. As a user, I want Ready, unread status, process state and unavailable health readable in narrow panes, so that clipping does not hide important conditions.
20. As a user, I want newer supported Codex versions identified explicitly, so that upgrading does not silently weaken reporting guarantees.
21. As a user, I want unsupported versions to remain natively usable, so that telemetry compatibility does not prevent work.
22. As a user, I want supported resume forms to preserve exact native arguments and conversation ownership, so that reconnecting to history does not attach reporting to another conversation.
23. As a user, I want metrics to state which conversation they describe, so that historical usage is not mistaken for the conversation currently displayed.
24. As a user, I want unavailable metric components shown as unknown, so that missing data is not presented as zero.
25. As a user, I want root metrics to exclude child usage unless explicitly defined otherwise, so that totals have a consistent meaning.
26. As a user, I want replay and reconnect to avoid double-counting, so that restored reporting remains trustworthy.
27. As a user, I want source loss or history changes to invalidate unsafe metrics, so that stale totals are not presented as current.
28. As a user, I want complete accounting only when its categories and finality are proven, so that a partial total is not overstated.
29. As a user, I want Confirmed completion only after continuation decisions settle, so that an ordinary Stop is not promoted into task success.
30. As a maintainer, I want local lifecycle failures diagnosed with retained evidence, so that passing hosted CI does not erase unresolved local behavior.
31. As a Linux user, I want native Codex reporting tested on Linux, so that synthetic platform coverage is not mistaken for provider acceptance.
32. As a user running many sessions, I want bounded reporting and measured capacity, so that notification or metrics work does not harm the synchronous server.
33. As a maintainer, I want independently reviewable implementation units, so that source-contract decisions and product changes can be assessed separately.
34. As a user, I want test fixtures and credential copies cleaned up only when ownership is proven, so that acceptance cannot damage live sessions or retain credentials.

## Implementation Decisions

### Shared constraints and execution order

- First follow-up product scope: optional response-ready notifications and explicit unread acknowledgement. Compatibility, metrics, accounting/finality and additional assurance are separate workstreams; their inclusion here does not admit them into the first release.

- Preserve the synchronous runtime, one server owner, one active dashboard and 50-session support. Keep provider interpretation in the application reporting layer and process/socket ownership in runtime. Do not add a second watcher, service, database, async runtime or replacement terminal.
- Use this order: A operational delivery; B notifications; C unread acknowledgement. D compatibility and G assurance are independent bounded workstreams. E metrics requires its source gate. F complete accounting/finality requires additional guarantees and must not be folded into E by implication.
- At each unit, verify the current branch, PR and base; reconcile shared mainline changes without modifying another provider's worktree. Read the affected callers and development guides. Preserve unrelated work and historical evidence.
- Use a scoped implementation worker and a separate reviewer. The coordinator alone stages intended files. Review exact committed revisions, correct findings, update support/setup documentation and the existing work diary, and require current-head CI before delivery.
- The notification/unread defaults below are proposed implementation boundaries, not previously expressed user preferences. Record any change to these defaults in the unit's design before dependent edits. Do not silently broaden persistence, notification ownership or metric semantics.

### A. Merge, install and enable the existing feature

1. Refresh PR #56's exact revision, review requirements and checks. Address any new review finding or actual mainline conflict; do not rebuild completed hooks, transport or setup work.
2. Obtain applicable merge authorization before merging. Build/install from the approved revision through the existing delivery path. A release remains a separate authorized action.
3. Verify the installed Codex version is in the accepted allowlist. Generate composed hook configuration, preserve existing handlers/order and native approval behavior, and apply it only to the intended configuration with applicable authorization.
4. Review/trust the generated hooks through native Codex before the first tracked prompt. Do not copy trust hashes or bypass approval/trust behavior.
5. Verify the actual installed binary through a managed terminal: root prompt, distinct assistant marker, Ready, next Busy, interruption and exit health. Record installation paths, versions, rollback instructions and the observed result.
6. Completion: the approved build works through the user's intended launch path and setup instructions. CI success alone does not prove installation.

### B. Optional response-ready notifications

1. Consume accepted runtime activity changes, not raw hook input, terminal text, silence or process exit. Preserve Observed quality; an alert means a root response is available to review, not task success.
2. Use the existing binding identity, generation, turn and accepted activity revision to recognize a new Ready observation. Deduplicate before alert dispatch. A child callback, stale event, duplicate or unavailable historical snapshot must not trigger a new alert.
3. Default first implementation to the active dashboard as the notification owner, with alerts only for terminals whose output is not currently shown in a visible pane. Visibility suppresses alerts only; it does not mark the response reviewed. No active dashboard means no alert delivery; document this boundary. Server-owned alerts after dashboard exit would require a separate architecture decision.
4. On initial attachment or reconnect, establish the existing Ready observations as a baseline without emitting old alerts. Subsequent accepted root Ready events can notify once. Do not promise alerts for responses completed while disconnected.
5. Add opt-in notification preferences through the existing configuration and dashboard action mechanisms. Default both sound and desktop notifications off. Keep them independently selectable and avoid introducing a preference subsystem.
6. Add the smallest host adapter needed for supported sound/desktop delivery. Native GUI audio delivery was previously unproven; establish a bounded working host path before claiming sound support. A terminal bell alone is not proof that the user heard anything.
7. Include only configured project/workspace/terminal identification in the notification, never prompt or response bodies. Notification permission denial or delivery failure must not block reporting, input, native approvals or exit. Avoid unbounded retries.
8. Completion: one real background root Ready produces one enabled alert; visible-pane suppression, duplicates, reconnect, child completion, interruption, unavailable health and disabled preferences behave as specified on each claimed host.

### C. Explicit unread acknowledgement

1. Keep activity and unread state separate. Mark-reviewed must not rewrite ResponseReady, Idle, quality, reporter health or native state.
2. Default to one unread indicator per terminal for the most recent unreviewed Ready observation, not a queue or count of every historical response. A dashboard aggregate, if needed, counts terminals with unread state.
3. Add only the minimal server-owned in-memory review state needed to survive dashboard reconnect. A Ready observation establishes/replaces the latest unread identity; a new Busy state does not itself acknowledge an earlier response. Reporter loss may retain historical unread state with Unavailable health visibly identified.
4. Make acknowledgement an explicit dashboard action and expose the corresponding existing CLI request path. Merely selecting or opening a terminal does not acknowledge it.
5. Require the acknowledgement to name the expected unread observation. A delayed acknowledgement of an older turn must not clear a newer Ready observation. Duplicate acknowledgement is safe.
6. Include review state in existing snapshots and follow established wire-version/compatibility rules for any protocol additions. Bound storage to terminal/session lifetime; do not add persistence across server death.
7. Completion: actual CLI/dashboard acknowledgement changes only the intended unread state, survives reconnect while the server lives, rejects stale clearing, and remains legible alongside activity/health/process state in narrow views.

### D. Verified compatibility expansion

1. Select one concrete new Codex version or launch form per unit. Inspect its exact native hook schema, launch grammar and process behavior; never infer compatibility from a minimum version or a similar help string.
2. Verify unchanged argv, synchronous direct-exec attribution, root/child identity, prompt/ending order, native lifetime fencing and failure behavior through the managed launch path.
3. For resume/picker/fork forms, establish exact conversation ownership before reporting. A first callback, newest file or terminal label is not ownership evidence. Preserve existing next-prompt rebinding semantics unless an explicitly reviewed contract changes them.
4. Enable only the verified version/form; update allowlists, setup/doctor diagnostics and the capability matrix together. Unsupported forms retain native usability with reporting unavailable.
5. Completion: focused real-process regressions plus native acceptance for the selected addition pass. Stop after the bounded case; do not expand into an open-ended provider matrix.

### E. Context and token metrics behind a source gate

1. Resolve the semantics before implementing a collector. The original current-foreground-conversation contract remains blocked because the verified native backtrack path can switch history without an immediate observation/invalidation signal.
2. Proceed only if either a concrete passive/native source provides reliable foreground invalidation, or a separately approved contract explicitly describes metrics for the last observed conversation. The readiness scope reduction does not automatically approve that metrics scope reduction.
3. Do not repeat the previous general investigation without a new concrete source candidate or contract change. Timebox the selected candidate investigation and retain a precise pass/blocker decision.
4. Verify source identity, schema, root/child distinction, snapshot versus delta meaning, history/replay behavior, context occupancy/capacity, cache/reasoning subset relationships, source loss and native lifetime. Use literal native references. Cost remains unknown without an authoritative cost contract.
5. If the gate passes, reuse the existing bounded collector exchange, lease/receipt handling and runtime metric snapshot. Add a provider parser only; do not route Codex data through Claude-specific accumulation or add an unbounded file watcher.
6. Replace authoritative cumulative snapshots instead of summing repeated notifications. Preserve source watermarks across supported recovery. Conflicts, unsupported history mutation, overflow and source replacement must follow the certified fail-closed rule.
7. Preserve existing envelope, processing, memory and completion deadlines. Keep activity, context utilization and usage/cost totals independent. Partial stays Partial; missing fields stay unknown.
8. Completion: source-to-collector-to-runtime/CLI tests and native numeric comparisons prove the enabled fields without duplicate or child mutation. If the source guarantee remains missing, publish the blocker and stop dependent implementation.

### F. Complete accounting and Confirmed completion

1. Treat these as separate capabilities. Neither is required for useful Ready notifications or partial metrics.
2. For complete accounting, obtain authoritative category coverage, cache/reasoning relationships, imported baseline rules, correction/decrease behavior, conversation versus invocation scope, and a final publication boundary. A cumulative field by itself is insufficient.
3. For Confirmed completion, obtain an event after native continuation decisions and relevant Stop hooks settle. An observed Stop, process exit, timeout or successful-looking text is insufficient.
4. Specify late-event, correction, replay, cancellation and source-disconnect behavior before changing quality/coverage labels. Missing guarantees leave the existing Observed/Partial/unknown states intact.
5. Completion: independent source-contract review and deterministic adversarial/native evidence establish each claimed guarantee. Otherwise stop with a named dependency; do not turn this unit into repeated certification rounds.

### G. Reliability and additional platform/capacity assurance

1. Diagnose the retained local lifecycle failures independently of feature expansion. Reproduce a concrete failed case through the actual managed CLI/PTY/socket path and distinguish fixture cold startup, host contention, deadline behavior, ownership cleanup and real product defects.
2. Keep prior failed runs failed. Do not relax assertions, disable checks or extend production deadlines merely for green results. A test-resource change must preserve its behavioral contract and state what concurrency coverage it no longer provides.
3. If a product defect is found, add a meaningful failing regression before correction where practical and return the scoped change for independent review. Do not mix unrelated fixture cleanup into notifications or metrics.
4. For Linux native-provider acceptance, use an isolated Linux host, the exact selected Codex version and applicable credential authorization. Verify actual root/child attribution, two turns, interruption, native approvals remaining native, output/exit and owned cleanup. Existing Linux synthetic fixtures are regression evidence, not this acceptance.
5. For Codex-specific concurrent launch capacity, use an isolated host and declared resource/request budgets. Measure launch eligibility, reporting latency, queue bounds, false/missing/duplicate Ready events, native usability, memory and owned cleanup up to the required 50-session level. Separate deterministic transport stress from credentialed provider/network load.
6. Do not run that load on the busy local host. The existing shared/Claude capacity and collector-memory gates remain required regressions, but do not certify simultaneous native Codex cold starts.
7. Completion: report local deterministic results, Linux native results and capacity measurements separately, with exact revisions and remaining gaps.

## Testing Decisions

- Primary seam: the public managed CLI launching a real child under an actual OVRCR server, PTY and private socket. Assert externally observable session snapshots, CLI results, displayed state, native output/exit and owned process-group cleanup. Prefer existing lifecycle fixtures and synchronization barriers over new seams or sleeps.
- Native acceptance remains necessary for provider semantics and visible/audio/desktop outcomes. Synthetic children can prove rejection, ordering and replay behavior, but cannot certify an installed Codex version or audible delivery.
- Notification dispatch is the only proposed additional boundary: a host-effect sink. Use a deterministic recorder for ordering/deduplication regressions, followed by actual host delivery acceptance. A mock success does not prove a visible notification or sound.
- Test the reporting receiver, runtime snapshot/acknowledgement behavior, CLI/setup diagnostics, dashboard rendering/input and host notification adapter through their highest practical existing callers. Avoid tests that mirror private implementation details.
- Prior art includes managed Codex hook lifecycle tests, runtime reconnect/revision tests, public setup/doctor tests, pane-specific clipping regressions, controlled native child gates, and existing hosted capacity/memory jobs.
- For notifications, cover duplicate/stale/child events, initial snapshot, reconnect, hidden versus visible panes, disabled preferences, permission denial, delivery failure, Interrupt and native exit.
- For unread state, cover real acknowledgement requests, a newer Ready racing an old acknowledgement, duplicate acknowledgement, Busy without acknowledgement, reporter loss, reconnect and server restart semantics.
- For metrics, cover literal numeric references, cumulative replacement, replay, child exclusion, stale generations, lost receipts, bounded malformed/oversized input, source loss/recovery and unsupported history changes.
- Retain failed attempts with source revision, command, result and scope of evidence. Use direct typing and inspect composed native test prompts before submission; the previous clipboard race must not be repeated unnoticed.
- Run focused regressions during development and required current-head macOS/Linux CI plus hosted resource gates before delivery. Do not repeatedly run general certification matrices when only a scoped correction changed.
- Test configuration, sockets, workspaces and credentials must be isolated. Never log credential values or capture unrelated content. Record PIDs/groups before exit and remove only resources whose task ownership is proven.
- These seams carry forward the user's existing explicit CLI/PTY/socket and reviewed native-GUI requirements; they are not a new interview or a request to approve previously authorized testing.

## Out of Scope

- Reimplementing the completed hooks-only adapter, supervised launcher, protocol transport, lease/receipt mechanism or snapshot dashboard.
- Automatic merge, release, live hook installation, trust bypass, provider approval handling or credential changes as a consequence of writing this specification.
- Terminal-pattern telemetry, newest-file discovery, first-callback ownership, mutating resume/attachment merely to observe telemetry, or fabricated completion from silence/exit.
- Durable unread history, per-response archival queues and alerts after server death. Server-owned alerts without a dashboard require a separate design decision.
- Automatically enabling unverified versions, launch forms, metrics, complete accounting or Confirmed quality.
- Other provider implementations, broad API-failure inference, arbitrary WaitingInput inference and unrelated repository cleanup.

## Further Notes

Dependencies: A delivers the existing feature. B and C build user-facing review workflow on its accepted events; B does not require C or metrics. D and G can proceed independently in isolated units. E and F remain gated by the explicit source guarantees above.

Tracker: GitHub Issues in `xlyk/ovrcr`, with `ready-for-agent` for this coordinating specification. Downstream collector/finality implementation remains blocked until its explicit source gates pass.

Reference baseline: https://github.com/xlyk/ovrcr/pull/56. Accepted current-head CI at baseline: https://github.com/xlyk/ovrcr/actions/runs/34572184009. Recheck external status and installed versions at execution time. The existing native evidence and support contract distinguish the hooks milestone from the older blocked metrics investigation.
