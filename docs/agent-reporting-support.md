# Agent reporting support record

Claude contract review date: 2026-09-10. The installed executable was verified as **2.1.267** by the Task 1 coordinator. No minimum supported version is certified. This record describes reporting research for the [Claude implementation plan](../plans/2026-09-09-claude-code-reporting.md); it does not declare the planned integration shipped.

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

## Foreground admission remains uncertified

| Transition | Task 1 acceptance still needed |
| --- | --- |
| Initial start | Prove the candidate belongs to the supervisor-owned foreground invocation. |
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

No transcript usage fixture is fabricated. The proposed `(message.id, requestId)` reader key remains an implementation-plan hypothesis until source or live evidence certifies it. Root-only accounting cannot claim complete native usage while excluded categories remain unresolved. Missing cost remains unknown; explicit zero is a value.

Task 1 remains open. The source fixtures support later parser work only after its prerequisite contracts are resolved. Existing manual reporting instructions remain in [agent-reporting.md](agent-reporting.md).

## Local execution evidence

The [acceptance checkpoint](../research/claude-reporting-acceptance/checkpoint.md) records the executable-version check, three passing existing Claude tests, sandbox-network failure, and isolated native onboarding attempt. Network permission resolved connectivity; the disposable configuration then required authentication. No authenticated turn or live hook payload was captured. Owned fixture PIDs 98073, 98075 and 98076 were terminated and verified absent.

## Authenticated follow-up, 2026-09-10

Authentication was authorized and succeeded after the earlier blocked attempt. [Attempt 05](../research/claude-reporting-acceptance/attempt-05-summary.md) records real startup, consecutive turns, continued Stop, clear, resume, permission wait/allow, tool completion and sanitized usage samples. The source-derived fixture directory remains source-only; live evidence is stored separately. Final settling, complete accounting and transition admission remain uncertified.

## Compaction and implementation gate follow-up

[Attempt 07](../research/claude-reporting-acceptance/attempt-07-summary.md) observed same-conversation compaction and a foreground branch with a new ID. The restricted fixture refused background fork, so that case remains unverified. The approved fallback remains unavailable identity for ambiguous transitions.

Task2 shared runtime work is permitted by the plan's source-or-named-blocker gate. It does not certify a Claude adapter: initial foreground admission, complete final accounting and confirmed settling remain independent provider gates.
