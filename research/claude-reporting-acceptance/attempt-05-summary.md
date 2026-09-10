# Native Claude contract capture — attempt 05

2026-09-10, Claude Code 2.1.267, OVRCR baseline 188a6456e42607634aedc72d3a93697faebcb0e1. Two isolated interactive launches using script(1), explicit settings and empty setting-sources. The user authorized reuse of their subscription authentication. Temporary copied credentials were removed after both sessions exited normally (exit 0). Live settings were not edited.

## Verified observations

- Explicit session ID appeared in startup hooks. Two ordinary turns completed with distinct prompt identities where capture included that field.
- A deliberately blocking Stop hook produced another response and another Stop with the same prompt ID and stop_hook_active=true. This does not prove a post-decision settled signal.
- /clear emitted SessionEnd(reason=clear), then SessionStart(source=clear) with a different session ID while the native process remained alive. Its new context was unknown and cost reset to zero.
- A second invocation resumed the original conversation. A harmless printf command completed. A Python command writing only permission-fixture.txt produced an actual approval prompt; choosing Yes once allowed it. The output file contained OVRCR_PERMISSION. No persistent permission grant was selected.
- Captured PermissionRequest/PostToolUse and child lifecycle metadata. Metadata records are evidence of these events, not a complete root/child attribution certification.
- Transcript assistant records contain requestId, message.id and token/cache usage. This captures their existence; two rows repeat the same message/request usage with different UUIDs, demonstrating UUID-based double-count risk. Changing-value replacement, fork and auxiliary completeness remain unverified.
- A status-line cost was 0.19044019999999998. Decimal-to-tick conversion must specify rounding instead of rejecting harmless provider precision residue or silently truncating.

## Capture limits

The initial capture allowlist omitted prompt_id; later records include it and field_names. Missing keys in early captures must not be interpreted as absent native fields. Records include the earlier unsuccessful onboarding attempt and are not a causal event stream across launches. IDs are consistently pseudonymized within this final bundle; no prompts, responses, tool arguments, credentials or account identity are stored.

37 hook/status records and 9 assistant usage records are saved in attempt-05-live-hooks.jsonl and attempt-05-live-usage.jsonl. Attempt 04 is preserved as an earlier partial capture.

## Remaining gates

Foreground/background fork admission, compaction, cancellation, approval denial, terminal API failure, complete child/auxiliary accounting, final-write boundary and authoritative settled state are not certified. SessionEnd must discriminate clear/conversation-switch from process closing. Task 1 remains open; no new runtime reporting implementation is claimed.

Independent live-contract review confirmed the observations and kept remaining gates open. Ruling: SessionEnd(clear) preserves supervisor ownership and uses conversation-transition admission; native completion governs final release. Ruling: USD conversion uses checked decimal rounding to nearest tick with ties to even; this preserves sub-tick accuracy within half a tick while accepting observed provider residue. These evidence-driven corrections are recorded in the worktree plan/design.
