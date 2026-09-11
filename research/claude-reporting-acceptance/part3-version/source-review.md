# Part 3 independent provider source review

Reviewed 2026-09-10 at `fcf26febf9672c62ed8cb1de73b0f3dca3c05f18`. Review inputs were the retained Claude Code 2.1.268 evidence under `research/claude-reporting-acceptance/part3-version/`, the Part 3 source investigation, the current capability record, and the current admission and exact-source collector code. No provider process, test, authentication flow, staging, or commit was performed.

## Decision

**Approved as a provider source contract, with integrated acceptance still required.** The evidence is sufficient to add the exact output `2.1.268 (Claude Code)\n` to the supported-version decision while preserving the existing Partial accounting and Observed activity contract. It is also sufficient to admit the exact separate-token form `-r <canonical-lowercase-UUIDv4>` on 2.1.268 only.

The current executable decision accepts only 2.1.267, so every 2.1.268 inventory snapshot correctly has `agent: null`. This review does not claim that current production code reports 2.1.268. After the code change, the real OVRCR path must pass the Task 2 native integration, including the remaining source-recovery case, before 2.1.268 support is complete.

## Evidence supporting exact 2.1.268

- `2.1.268-version.txt` and every entry in `launches.jsonl` identify the exact provider build. The retained 2.1.268 help exposes the same relevant interactive grammar as 2.1.267, but the version decision must remain an explicit two-value allowlist rather than a range.
- The fresh invocation emitted a root `SessionStart(source=startup)` with one session ID and exact transcript path. Subsequent root prompt, Stop, status-line, permission, tool, and exit records retained that identity.
- A Stop hook continuation retained the same root prompt identity through later work and the final Stop. This supports the existing Observed working/idle source shapes; it does not certify the separate launch flag `--continue`.
- The permission case ordered root `PreToolUse`, `Notification(permission_prompt)`, a status-line observation while waiting, root `PostToolUse` after approval, and root `Stop`. The synchronous hook settings and native PTY evidence match the existing contract.
- The child failure case emitted `PreToolUse` and `PostToolUseFailure` with the same nonempty `agent_id`. Existing parsing excludes both before root-state mutation. This certifies the captured child failure shape, not every child or background form.
- Same-conversation compaction emitted root `SessionStart(source=compact)` with the unchanged session ID and transcript path. Its next status-line sample carried explicit zero-valued `current_usage`. Existing Partial cumulative usage and estimated cost remain independent from current context.
- The separate long-form launch preserved `--resume 3acc3193-a5b5-4f41-a93b-252c30e131bc` without `--session-id`, owned the foreground terminal process group, emitted a matching root `SessionStart(source=resume)`, retained pre-existing recognized rows, and visibly recalled `copper`. The absent canonical UUID printed `No conversation found`, emitted `SessionEnd(reason=other)` without a `SessionStart`, and never supplied an admissible root binding.
- The exact-source usage snapshots show the same root `sessionId`, explicit `isSidechain=false`, and stable `(message.id, requestId)` identities for the retained subset. They support the existing `coverage=Partial`, `scope=Conversation` contract only.
- After `/clear`, 2.1.268 emitted `SessionEnd(reason=clear)` for the old ID, `SessionStart(source=clear)` for a new ID, then a status-line sample whose `current_usage` was null. This agrees with the existing representation of unknown current context, while the identity change remains unsupported.

No observed source shape requires a protocol change, a new accounting scope, Complete coverage, or a stronger activity quality. The minimum version change is the exact version decision plus its setup/doctor agreement and adjacent-version rejection tests.

## Short resume contract for 2.1.268 only

The newly approved grammar is exactly:

```text
-r 3acc3193-a5b5-4f41-a93b-252c30e131bc
```

All of these conditions are required:

1. Claude reports exactly `2.1.268 (Claude Code)\n` during the bounded version probe.
2. `-r` and the value are separate argv tokens.
3. The value passes the existing canonical lowercase UUIDv4 grammar, including the version and RFC variant nibbles.
4. No positional prompt, duplicate resume option, `--session-id`, print/background form, or other unsupported option is present; the existing non-transition safe options remain unchanged.
5. OVRCR preserves the native argv and does not rewrite the short flag or inject `--session-id`.
6. Before the first binding, only a root `SessionStart(source=resume)` whose `session_id` exactly matches the argv UUID may bind; its `transcript_path` becomes the one exact collector source.
7. A child or foreign callback cannot bind first, a missing UUID cannot bind without a matching SessionStart, and a contradictory root start permanently closes the initial admission path under the existing contract.
8. After Bound, resume, clear, and fork starts retain the current identity-transition freeze behavior. Same-ID compaction remains the sole separately certified SessionStart exception.

`launches.jsonl` and `14-short-resume-processes.json` prove the exact `-r UUID` argv and foreground native child on 2.1.268. `signals.jsonl` records the matching root resume start. `14-short-resume.ax.txt` proves retained conversational context with `OVRCR_268_SHORT_RESUME_OK copper`. The separate absent UUID printed the same native failure and emitted only `SessionEnd(reason=other)`.

This evidence does **not** authorize `-r UUID` on 2.1.267. Identical help text is documentation of an alias, not exact-version runtime evidence. It also does not authorize `-r` without a value, names, search terms, picker use, equals syntax, uppercase or alternate UUID spelling, `--continue`, `--from-pr`, or `--fork-session`.

Implementation should make eligibility depend on both the exact resolved version and the launch form. A global acceptance of `-r` inside `eligible_launch` would silently expand 2.1.267 and fails this contract.

## Boundaries that remain blocked

### In-process identity transitions

The capture observed `A -> fork B -> resume A -> clear C` with paired root SessionEnd/SessionStart records. It did not establish that the pair is exclusive to foreground intent, distinguish background fork behavior, or cover cancelled/failed switching and stale callbacks. Those private events remain observations. Clear, in-process resume, and fork must continue to freeze reporting after a binding rather than rebind.

The post-clear null `current_usage` only establishes that the new conversation's current context is unknown. It does not authorize the identity transition or reset cumulative accounting in an existing binding.

### Complete and final accounting

The local delta OpenTelemetry receiver recorded only `claude_code.session.count` before input on resume. After one new resumed turn it recorded `query_source=main` cost and input, output, cacheRead, and cacheCreation token points for that new request. It did not export the resumed transcript's historical usage.

The OTEL rows have no prompt, request, transcript-record, or correction identity. This capture does not cover child or auxiliary accounting, retry/correction behavior, cumulative temporality, shutdown flush, exporter acknowledgement, or ordering against final transcript writes. Empty polling exports and a normal process exit do not create a final-source boundary. OTEL cannot replace the exact-source Partial transcript collector or support Complete coverage on this evidence.

### Confirmed activity

Matching `idle_prompt` notifications were observed for selected successful prompts, including continued work. The capture does not prove delivery for permission denial/cancellation, failed or blocking Stop paths, child failure, stale prompt timers, or new input near the timer. Silence is still ambiguous because user input can cancel the notification. Activity therefore remains Observed; no Confirmed transition is authorized.

## Required implementation gates

The implementer may modify the exact version decision and add the 2.1.268-only short form under the contracts above. Completion still requires:

1. unit tests proving exact 2.1.267 and 2.1.268 version acceptance, adjacent-version rejection, and setup/doctor agreement;
2. version-aware argv tests proving long `--resume UUID` stays supported on both certified versions while short `-r UUID` is accepted only on 2.1.268 and all excluded forms remain ineligible;
3. real-path receiver/runtime tests for matching short resume, foreign/child-first input, contradictory root input, missing UUID, Partial history restoration, and later transition freeze;
4. native 2.1.268 acceptance from the reviewed implementation revision for fresh and both enabled resume spellings, including bound identity, synchronous callbacks, foreground ownership, Partial history, normal exit, task-owned cleanup, and the Task 2 source-recovery case;
5. final evidence retention and cleanup reconciliation for the discovery fixture. This is packaging and ownership work still in progress at review time; it does not broaden the source contract above.

No work on in-process transitions, OTEL production collection, Complete/final accounting, or Confirmed activity should follow from this approval.
