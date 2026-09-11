# Integrated Claude 2.1.268 native acceptance

2026-09-10. Built the actual disposable GUI from clean tracked `dd08be0`, whose product implementation is the independently approved `c169939`. `source-prebuild.txt`, `status-prebuild.txt`, and `launcher.log` retain build provenance. Later `8166712` changes only the hosted memory fixture. The native executable was checked as exactly 2.1.268 before each invocation. Existing login access was explicitly approved; the token was passed only in process memory, never retained here.

All input used native CUA in the runtime PTY. Fixture settings, trust, provider configuration, and sockets were isolated. Hook bytes were forwarded unchanged; `signals.jsonl` retains selected identity/numeric fields and forwarding status, not complete payloads. Screenshots retain visible assistant output, not just submitted prompts.

| Case | Evidence | Result |
| --- | --- | --- |
| Fresh startup and response | 01/02 JSON, AX/JPG, usage and process files | Bound UUID `2140c13e-36a9-4f89-b219-39bdb01aa94d`; actual `OVRCR_PART3_SEED_OK`; Idle/Observed and Partial Conversation usage. Before the first prompt the transcript did not yet exist, so source_unavailable and unknown context were expected. |
| Exact source recovery | 03/04 JSON | Temporarily renamed only the bound transcript, observed source_unavailable, restored it, and observed Connected with unchanged binding and totals. |
| Long UUID resume | 06/07 | Same UUID, distinct invocation; imported 41,233 input / 18 output before new input. Actual `OVRCR_PART3_LONG_RESUME_OK amber`. |
| Short UUID resume | 10/11 | Native `-r UUID`, same UUID and a third invocation; imported 210,443 input / 374 output before new input. Actual `OVRCR_PART3_SHORT_RESUME_OK amber`. No injected session-id on either resume. |
| Actual child API failure | 08/13 | Deliberately nonexistent child-only model returned model_not_found HTTP 404; valid parent continued and produced the separate completion marker. |
| Child failure isolation | 13 and child-callback-comparison.json | Actual child-tagged StopFailure forwarded successfully. Complete before/after agent objects are equal: binding, activity/revisions, health, and metrics unchanged. |
| Exit and ownership | 05/09/12/14, cleanup.json | All four native sessions exited 0. All 29 recorded PIDs and 24 groups absent after GUI launcher exit 0; socket and fixture directory absent. |

`numeric-comparison.json` reconciles all five token components against retained exact-source root records for seven snapshots. Equal `(requestId, message_id)` duplicates are counted once; foreign and sidechain rows are excluded. Every comparison matches. This proves recognized root accounting, not coverage of child/auxiliary requests, correction semantics, finality, billing accuracy, or Complete quality. Cost remains independently estimated.

The initial child capture used OVRCR_SESSION_ID, which the native launcher intentionally strips. Those before/after samples are unavailable and are not treated as passed. The corrected fixture helper uses a task-owned inspection-session file and explicit isolated config/socket. One repeat resolved this observation gap. During its SubagentStart window a concurrent root activity revision advanced; that window is not claimed to prove an unchanged object. The actual StopFailure window and both later captured SubagentStop windows have equal complete agent objects. The checked-in capture.py is the corrected helper; earlier failed observations remain in signals.jsonl.

Process files retain PID/PPID/PGID/foreground TPGID/TTY and safe argv before native exit, including each separately owned collector group. These are captured process inventories, not a claim to observe every transient hook process. No unrelated sessions were stopped. Broader transition, Complete, Confirmed, and universal memory guarantees remain disabled/unresolved.
