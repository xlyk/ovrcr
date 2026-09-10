# Agent reporting support record

Claude contract review date: 2026-09-10. The installed executable was verified as **2.1.267** by the Task 1 coordinator. No minimum supported version is certified. This record describes reporting research for the [Claude implementation plan](../plans/2026-09-09-claude-code-reporting.md); it does not declare the planned integration shipped.

The [remaining-requirement matrix](../research/claude-reporting-acceptance/remaining-matrix.md) reconciles this rolling record with the delivered callers, assertions, tested revisions and native evidence. It splits passing parts from open provider and platform clauses.

The [fixture manifest](../tests/fixtures/agent-reporting/claude/manifest.json) contains 23 **source-derived** payload examples. Every identity, path and content value is synthetic. Shared IDs illustrate relationships; file order does not represent observed callback order. The rolling documentation may describe behavior that differs from the installed release. No fixture in this directory is a live capture.

## Documented sources

| Surface | Fields and interpretation |
| --- | --- |
| Hook identity | `session_id`, optional `prompt_id`, and `transcript_path`; `agent_id` identifies a child. `agent_type` can also name a root session. |
| Session transitions | `SessionStart.source` includes startup, resume, clear, compact and fork. Fork covers both foreground branch and background fork operations. |
| Completion observations | Stop can cause continuation. Interrupted turns do not emit Stop. StopFailure covers API failure. Transcript writes can lag callbacks. |

These fields come from the [official hooks reference](https://code.claude.com/docs/en/hooks), refreshed on the review date. Prompt identity offers correlation; this review does not certify callback order or a final settled notification.

| Measurement | Source and scope |
| --- | --- |
| Current context | `context_window.current_usage`; occupancy uses fresh input plus cache writes and reads, excluding output. Capacity comes from `context_window_size`. Null usage is unknown. |
| Context totals | `total_input_tokens` and `total_output_tokens` describe current context, not cumulative conversation consumption. |
| Cost | `cost.total_cost_usd` is a client-side estimate for the session; clear resets it. Actual billing may differ. |

The [official status-line reference](https://code.claude.com/docs/en/statusline) supplies these semantics. The fixtures use small synthetic values: 30 fresh input + 10 cache-write input + 40 cache-read input = 80 occupied tokens. The pre-response null fixture does not establish post-compaction timing.

## Foreground admission support

| Transition | Task 1 acceptance still needed |
| --- | --- |
| Initial start | Certified for the narrow interactive Claude Code 2.1.267 launch grammar, supervisor-selected UUID and root startup discriminator; actual managed routing captured in attempt09. |
| Foreground branch | Distinguish the new foreground conversation from a background fork. |
| Background fork | Prove the current foreground binding remains unchanged. |
| Clear | Capture identity replacement and callbacks arriving after the next transition. |
| Resume | Capture foreground replacement and exclusion of stale callbacks. |
| Compaction | Prove same-conversation identity persists and capture context replacement. |

The approved design requires identity to become unavailable for an ambiguous candidate. New metrics for that candidate must stop, and a new managed invocation is required. In-process switching is not certified. These restrictions must also appear in setup and doctor output when those commands are implemented; this research record does not implement them.

## Blocking dependencies

| Dependency | Evidence needed before certification |
| --- | --- |
| `live-payloads` | Isolated interactive start/resume, prompts, actual approval wait/allow/deny, tool success/failure, Stop continuation, cancellation, API failure, compaction, session end, children and new/resume/fork captures. |
| `settled-transition` | A supported or verified event after every Stop decision, correlated with the current work. Empty background arrays, assistant text, silence and callback completion do not discharge this gate. |
| `cumulative-accounting` | A supported aggregate or a versioned transcript reader with certified record identity, last-record replacement, replay, resumed/forked history and child/auxiliary coverage. |
| `final-source-completion` | A reliable final-accounting boundary, including delayed writes and abrupt native exit. |
| `cost-scope` | Comparison with native totals establishing child, auxiliary, resumed and forked attribution independently of token coverage. |

Live captures establish equal-usage duplicate rows with the same `(message.id, requestId)` and different record UUIDs. They do not establish differing-value replacement. The partial reader must stop before applying a conflicting counted value and label its retained result unavailable; it cannot claim the retained number remains current or is a lower bound. Recognized root-transcript records do not cover every native usage category. Missing cost remains unknown; explicit zero is a value.

Task 1 remains open. The source fixtures support later parser work only after its prerequisite contracts are resolved. Existing manual reporting instructions remain in [agent-reporting.md](agent-reporting.md).

## Local execution evidence

The [acceptance checkpoint](../research/claude-reporting-acceptance/checkpoint.md) records the executable-version check, three passing existing Claude tests, sandbox-network failure, and isolated native onboarding attempt. Network permission resolved connectivity; the disposable configuration then required authentication. No authenticated turn or live hook payload was captured. Owned fixture PIDs 98073, 98075 and 98076 were terminated and verified absent.

## Authenticated follow-up, 2026-09-10

Authentication was authorized and succeeded after the earlier blocked attempt. [Attempt 05](../research/claude-reporting-acceptance/attempt-05-summary.md) records real startup, consecutive turns, continued Stop, clear, resume, permission wait/allow, tool completion and sanitized usage samples. The source-derived fixture directory remains source-only; live evidence is stored separately. Final settling, complete accounting and transition admission remain uncertified.

## Compaction and implementation gate follow-up

[Attempt 07](../research/claude-reporting-acceptance/attempt-07-summary.md) observed same-conversation compaction and a foreground branch with a new ID. The restricted fixture refused background fork, so that case remains unverified. The approved fallback remains unavailable identity for ambiguous transitions.

Task2 shared runtime work is permitted by the plan's source-or-named-blocker gate. It does not certify a Claude adapter: initial foreground admission, complete final accounting and confirmed settling remain independent provider gates.

## Initial startup evidence follow-up

[Attempt08](../research/claude-reporting-acceptance/attempt-08-summary.md) observed a supervisor-selected UUID on a named root startup, with `agent_type` present and `agent_id` absent. Child lifecycle events carried the same session ID and a distinct `agent_id`, including cases with an empty `agent_type`. Independent review permits implementing this narrow startup discriminator, subject to actual invocation-routing tests and explicit launch-option eligibility. Initial startup remains uncertified until those tests pass; the earlier unsupported-switch limitations remain.

## Reviewed implementation checkpoints

The implementation branch now has independently reviewed shared ownership/protocol, managed launcher, narrow initial admission, observed activity, pure metrics arithmetic and compact dashboard presentation. [Attempt09](../research/claude-reporting-acceptance/attempt-09-managed/commands.md) exercised real managed startup, native output, clear/exit and owned-process cleanup. [Attempt10](../research/claude-reporting-acceptance/attempt-10-sources/summary.md) linked an unanswered approval selector to its correlated permission notification and the exact reported transcript path to matching file records. Earlier research-only statements above describe their original checkpoint, not the current branch.

Activity support requires synchronous hooks. Matching permission_prompt means observed waiting; PermissionRequest alone does not. Stop means observed completion, never confirmed settling. Native model-not-found StopFailure now has correlated evidence in [attempt11](../research/claude-reporting-acceptance/attempt-11-api-failure/summary.md); broader API failures remain unverified. Collector integration, setup/doctor, full metrics publication and final release gates remain in progress. No Linux or native GUI acceptance is claimed.

## Assembled implementation follow-up, 2026-09-10

Managed context/cost publication, exact-path partial transcript collection, native
finalization, setup/doctor and JSON inspection are implemented. Independent review
accepted the post-exit scan correction `05c3851` and conservative supplied-file
diagnostics `d139563`. Finalization requires a scan requested after native
completion, keeps the original two-second deadline, and always retains Partial
coverage. Neither EOF nor this scan establishes provider accounting completion.

Collector cleanup now retains eventual child-reaping ownership at an expired
deadline (`5585702`) and handles macOS zombie-only process groups by reaping the
owned child and checking group absence without signaling an unanchored group
(`e1bbe4b`). Actual failing and passing process regressions are preserved.

The [setup guide](claude-code-setup.md) describes configuration composition,
legacy migration, diagnostics and removal. The selected dashboard header now uses
the same managed activity quality/health as the sidebar (`2c432e0`). Native GUI
synthetic evidence and the blocked real-Claude trust boundary are recorded in
[the GUI review](../research/claude-reporting-acceptance/task7-gui/review.md).
That trust statement describes the first attempt; the authorized native follow-up
below resolved it and cleaned the fixture. This does not certify native provider
totals or final settling. Full release gates
remain open until all required provider/platform evidence is available.

Automated final checks passed586 workspace tests (11 intentional ignores),
workspace clippy and formatting. The separately executed
[50-session capacity check](../research/claude-reporting-acceptance/task7-load/scope.md)
passed its measured subset:30,000 updates/60s,3,000 stale rejections, five helper
failures, control p99=1.205ms/max=2.822ms and sampled RSS growth1.354GiB.
Internal queue counters remain unmeasured. The corrected selected-header quality
was recaptured in the native GUI with synthetic reports; actual Claude trust
approval and full provider/platform release gates remain open.


2026-09-10 authorized native follow-up: trust approval completed; two native Claude turns, observed activity, partial metrics, cost comparison, detach/reattach and exit0 verified. All recorded fixture processes, socket and disposable directory cleaned. See `research/claude-reporting-acceptance/task7-gui-native/review.md`. Earlier pending trust/cleanup statements are historical; provider completeness and Linux gates remain open.
