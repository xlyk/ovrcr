# Initial explicit UUID resume source certification

Capture date: 2026-09-10. Reviewed source: `c2fb7eae2c466d45906821521748bfaa2b523fff`. Provider: retained Claude Code `2.1.267` binary. This capture certifies the provider source contract for one initial interactive foreground launch using the exact separate-token form `--resume <uuid>`. Runtime admission and integrated native acceptance remain pending.

## Certified invocation

The only newly eligible grammar is:

```text
--resume 5ebc5f9b-54b5-4928-9955-dc81c23743dd
```

The value must be a canonical lowercase UUIDv4: lowercase hexadecimal, `8-4-4-4-12` hyphenation, version nibble `4`, and RFC variant nibble `8`, `9`, `a`, or `b`. The launch must remain an interactive foreground invocation under the existing safe-option allowlist.

This certification excludes `-r`, `--resume=<value>`, a missing value, picker or search values, uppercase, braced, unhyphenated, nil, non-v4, or non-RFC-variant UUIDs, duplicate resume flags, and a positional prompt. It does not expand support for `--continue`, `--session-id`, `--fork-session`, print mode, background mode, or any in-process identity transition.

## Provider evidence

- [`launches.jsonl`](launches.jsonl) records the exact native argv for the successful resume. It contains separate `--resume` and UUID tokens and no `--session-id` token. [`02-resumed-before-turn-processes.json`](02-resumed-before-turn-processes.json) independently captures that argv in the native process tree.
- [`signals.jsonl`](signals.jsonl) records a root `SessionStart` with `source=resume`, the requested UUID as `session_id`, the existing transcript path, and no `agent_id` field. The same file records the subsequent root `UserPromptSubmit`, `Stop`, status-line update, and `SessionEnd` for that conversation.
- [`settings.json`](settings.json) configures command hooks with `async: false` and a two-second timeout. [`capture.py`](capture.py) retained the hook and status-line capture path used by the run.
- [`02-resumed-before-turn-usage.json`](02-resumed-before-turn-usage.json) contains the pre-existing root assistant row before the resumed turn. [`03-resumed-after-turn-usage.json`](03-resumed-after-turn-usage.json) contains that row unchanged, with the same file device and inode, plus a new root assistant row with a distinct `(message.id, requestId)` pair. [`03-resumed-output.png`](03-resumed-output.png) and [`03-resumed-output.ax.txt`](03-resumed-output.ax.txt) show `OVRCR_RESUME_HISTORY_amber`, proving that the foreground process received the earlier conversational context.
- [`05-foreground.txt`](05-foreground.txt) records the later resume, during which no new user prompt was submitted, with the Claude child owning the terminal foreground process group: its PID, PGID, and TPGID are all `6486` on `ttys031`, under the OVRCR supervisor.
- The separate canonical UUID `11111111-2222-4333-8444-555555555555` was absent. [`04-missing-uuid.ax.txt`](04-missing-uuid.ax.txt) shows Claude's `No conversation found` result, [`signals.jsonl`](signals.jsonl) contains only its `SessionEnd(reason=other)`, and [`06-inventory.json`](06-inventory.json) records exit code `1`.
- [`06-inventory.json`](06-inventory.json) records exit code `0` for the seed, resumed-turn, and foreground-only invocations. [`cleanup.json`](cleanup.json) records all 21 task-owned PIDs and 19 process groups absent, with the task-owned root and socket removed.

The transcript extractor used for the retained usage snapshots iterated valid JSON rows and retained every assistant row without applying conversation or sidechain filtering. It did not count non-JSON rows. These snapshots therefore support the observed row identity and continuity statements above; they do not establish a malformed-line audit, complete category coverage, or a complete accounting boundary.

## Admission contract

The launcher may derive an expected resume identity before spawning Claude only when the argv matches the certified grammar. It must preserve the caller's `--resume <uuid>` tokens and must not inject `--session-id`.

While the receiver is waiting for its first binding, it may admit exactly one root `SessionStart` only when all of these assertions hold:

1. `source` is exactly `resume`.
2. `session_id` exactly equals the UUID parsed from the launch argv.
3. `transcript_path` is present and becomes the single exact collector path.
4. `agent_id` is absent under the existing Claude root parser contract.
5. The callback belongs to the same eligible invocation and passes the existing synchronous private-channel authentication and deadline checks.

A missing conversation can emit `SessionEnd` without a preceding `SessionStart`; that path must never bind. A wrong root identity or wrong source before binding remains ambiguous and must retain the existing unavailable/closed failure behavior. Child and sidechain events remain excluded by the existing parser and exact-source collector rules.

After binding, `SessionStart(source=resume)`, `source=fork`, or a clear/rebind sequence remains an unsupported identity transition and must retain the existing freeze/unavailable behavior. Same-conversation compaction keeps its separately certified path.

## Accounting consequence

The exact-source collector must start at byte zero on the admitted transcript. A resumed transcript can already contain older assistant usage, as this capture did. Reporting therefore remains `coverage=Partial` and `scope=Conversation`; it cannot be narrowed to the current invocation.

The resumed status-line cost estimate was `0.050545799999999995` before the submitted turn and `0.060461` after it. The later resume with no new user prompt reported `0.0699196`, while [`06-final-usage.json`](06-final-usage.json) still contained the same two extracted assistant rows. This is evidence that estimated status-line cost and the recognized transcript subset have independent scope; it is not evidence of complete coverage or an attributable auxiliary category.

The collector's stable identity remains `(message.id, requestId)`, not the outer transcript record UUID. An equal repeated pair stays idempotent and a differing payload for an existing pair stays unavailable. A row with two new identifiers is a new observed row. For an assistant record, an absent, non-string, or mismatched `sessionId` freezes as foreign; `isSidechain=true` is skipped; and an absent or non-boolean `isSidechain` freezes as unrecognized. This certification adds no Complete accounting or finality claim.

## Minimum implementation and acceptance work

Production behavior should be confined to launch classification and initial admission in `src/report/admission.rs`. The Claude parser, exact-source collector, metrics accumulator, hook transport, and runtime protocol require no broader provider contract for this form.

The minimum affected verification is:

1. argv classification tests for the one accepted form and every excluded grammar family above;
2. receiver tests for matching initial `source=resume`, missing/wrong identity, child exclusion, no-SessionStart failure, and post-bind resume/clear/fork freeze behavior;
3. runner/integration coverage proving the caller preserves `--resume <uuid>` and does not add `--session-id`;
4. one native 2.1.267 integration run from the implemented revision proving bound identity, carried Partial conversation history, synchronous callbacks, foreground ownership, ordinary exit, and task-owned cleanup.

No later provider version, continue form, in-process switch, fork form, Complete accounting, or Confirmed activity transition is part of this source certification.
