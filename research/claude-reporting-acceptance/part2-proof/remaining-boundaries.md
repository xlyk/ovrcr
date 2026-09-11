# Part 2 remaining boundaries

The 2026-09-10 continuation preserves the full milestone. Independent review
approved the existing supervisor socket-owner disconnect proof at `b8fce8c` and
the initial explicit UUID resume provider contract at `36b3f59`. Source
certification alone does not establish integrated resume support.

The following boundaries remain independent of that narrow resume work:

| Boundary | Current evidence and missing proof |
| --- | --- |
| Total memory | Application retained-payload/index bounds and sampled RSS passed on macOS and Linux. Full heap/allocator maxima, kernel socket-buffer memory, and sub-second peaks are unproved. The capacity fixture does not simultaneously fill fifty 32 MiB incomplete-record buffers. |
| Native null context | Real callback/runtime replacement is covered. Native compaction produced explicit zero, so native null timing remains unobserved. |
| Child API errors | Real child tool failure and SubagentStop isolation passed. No arbitrary child API StopFailure variant or complete child accounting is certified. |
| Confirmed activity | No certified matching-root post-decision settling signal. Ordinary Stop remains Idle/Observed. |
| Complete accounting | Auxiliary/child category coverage, differing-record corrections, and a final-source boundary remain uncertified. Partial totals and conflict freeze remain required. |
| Broader transitions | Continue, alternative resume forms, in-process resume/clear, and foreground/background fork discrimination remain uncertified. |
| Supervisor process/descriptor proof | The real socket-owner disconnect path is covered. A literal supervisor process-kill assertion and provider exec descriptor-inheritance assertion remain separate gaps. |
| Docker cleanup | A fresh read-only Docker info probe timed out after five seconds. The known task-owned container `ovrcr-claude-linux-reap-01a08c3f` and image `ovrcr-claude-linux-cache:01a08c3f` remain pending engine availability. No Docker Desktop restart was performed or authorized. |

These exclusions do not justify a narrower release or a complete milestone
claim. PR 55 must remain draft while the full milestone is unresolved. Merge and
release are not authorized.
