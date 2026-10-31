# Dashboard Outbox native acceptance

Native evidence for [specification #383](https://github.com/xlyk/ovrcr/issues/383) and [implementation #384](https://github.com/xlyk/ovrcr/issues/384). The actual reviewed checkout’s GUI accepted input before and after frozen History release and a terminal retarget.

The [verification report](verification.md) records automated checks, reviews, earlier failures and remaining limits.

Tested source identity:

- Base: `07b792fb9854e38db671ec51fc81ecee25d70907`.
- Frozen Dashboard source patch SHA256: `ac33661614537a86e1d1ee0c8126cd59348d27137202735afa527cabad20a8a5`.
- Hash input: exact stdout bytes of `git diff 07b792fb9854e38db671ec51fc81ecee25d70907 -- crates/ovrcr-tui/src` (run through `rtk proxy`). This identifies the owning-crate source frozen before the native build, including embedded test modules; it excludes integration tests and documentation.
- [Binary receipt](binaries.json): exact CLI/GUI bundle identities used for the proof. Final revision and regression-gate receipts are recorded separately by the task owner.

## Visible native outcomes

| Capture and full accessibility text | Observed result |
| --- | --- |
| [01 original](01-original-0.jpg), [AX](01-original.ax.txt.gz) | Session #1/PID 10363 executes printf and displays separate `OUTBOX_ORIGINAL_OK` output. |
| [02 History](02-history-frozen-0.jpg), [AX](02-history-frozen.ax.txt.gz) | Same session/PID; `HISTORY · frozen · loaded` and original output remain visible. |
| [03 replacement](03-replacement-0.jpg), [AX](03-replacement.ax.txt.gz) | After leaving History and retargeting, session #4/PID 11277 displays separate `OUTBOX_REPLACEMENT_OK` output in Terminal mode. |
| [04 return](04-original-return-0.jpg), [AX](04-original-return.ax.txt.gz) | Returning to #1 retains PID 10363 and original output, then displays separate `OUTBOX_RETURN_OK` output. |

AX receipts are gzip-compressed unchanged, preserving the terminal spacing. All four image/AX pairs come from this task’s own CUA calls. Image bytes are unchanged; the three later captures were decoded verbatim from the original tool output. Per-capture provenance records the original call IDs; JSON receipts also record timestamps and arguments. Source rollout: `rollout-2026-10-30T19-59-33-01a18fe5-f10d-78b1-8b0d-7be58bc03c48.jsonl`. Screenshots and accessibility text were inspected together. Raw attempts and the full acceptance mapping remain outside the repository in `/private/tmp/ovrcr-384-evidence`.

## Owned cleanup and limits

The native window closed normally through CUA call `call_4781064ceb314f7e977ee0814fc91c53`. [Original launcher wait](launcher-wait.json) records session 33617 exit 0. [Cleanup](cleanup.json) independently checked only the 12 recorded [owned groups](owned-groups.json): every `os.kill(-pgid, 0)` raised `ProcessLookupError`. The recorded fixture root and socket are absent; permission denial was not counted as absence. No app query or relaunch occurred after close.

Native held-mouse retarget input, clipboard delivery and live providers were not exercised. Captured mouse cleanup/authority/order have separate real socket and outer-PTY child coverage. Fixture provider labels represent synthetic shell fixtures. This record makes no blanket final-gate or all-layout claim.
