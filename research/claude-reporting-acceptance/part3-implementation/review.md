# Tasks 2-3 independent implementation review

Reviewed exact implementation commit `c1699390e12579f826a100488720a40daa06422a` against `3b391581e87eb25ad8d6de9a245e78579fee28a9` on 2026-09-10. The comparison contains nine intended files: admission, setup/doctor, CLI help, three focused test files, and three user documents. Later commits and current dirty matrix, load-test, workflow, and capability-record changes were excluded.

## Blocking findings

None.

## Spec review

The implementation matches the independently approved provider source contract.

- `src/report/admission.rs:23-30` performs the cheap interactive/superset grammar check, invokes the bounded version probe once, then classifies the launch with the detected exact version. Neither eligibility function starts another probe.
- `src/report/admission.rs:132` accepts separate-token `-r` only for `ClaudeVersion::V2_1_268`. Separate-token `--resume` retains support for both enum values. The existing canonical lowercase UUIDv4 parser, no-positional-prompt rule, and unsupported-option rejection are unchanged.
- `src/report/admission.rs:204-259` defines exactly two supported versions, recognizes only the exact `2.1.267 (Claude Code)\n` and `2.1.268 (Claude Code)\n` outputs, and reports other well-formed Claude version lines as unsupported rather than supported by range.
- `src/report/admission.rs:264-345` retains the one-second deadline, 128-byte bound, private-environment removal, owned process group, and timeout/error cleanup. The diff adds no retry or wider timing allowance.
- Fresh startup still receives an injected supervisor UUID. Both resume spellings retain their original argv and bind only through the existing matching initial root `SessionStart(source=resume)` path. The receiver, Claude parser, collector, activity quality, accounting coverage, and transition handling are otherwise unchanged.
- `src/cli/agent_setup.rs:190-207,320-335` uses the same probe and reports the detected version, exact allowlist, distinct unsupported/unavailable states, and version-specific resume forms. Adjacent-version and unavailable-probe tests exercise these outputs.
- `tests/server_lifecycle.rs:8536-8771` preserves the 2.1.267 long-resume lifecycle and adds the 2.1.268 short-resume lifecycle through the same real PTY/socket helper. The shared assertion covers native argv, initial foreign/child and contradictory-root rejection, matching binding, byte-zero Partial conversation history, a new distinct row, Observed activity, same-ID compaction, post-bind resume/clear/fork freeze, exit, and fixture cleanup.
- `src/report/admission.rs:1032-1087` directly proves long resume on both exact versions, short resume on 2.1.268, short rejection on 2.1.267, and rejection of missing values, equals syntax, alternate UUID spellings, prompts, duplicate/mixed resume flags, continue, session ID, fork, background, and print forms.
- Setup text, generated CLI help, `docs/claude-code-setup.md`, `docs/cli-reference.md`, and the Part 3 support-record addition describe the same two-version and version-specific grammar. They retain Partial / Conversation usage, Observed activity, and unsupported transition/finality language.

No in-process rebinding, OTEL collection, Complete accounting, final-source completion, or Confirmed activity behavior was added.

## Standards review

Pass. The change stays inside the provider-specific admission boundary, reuses the existing UUID, receiver, collector, and lifecycle mechanisms, and does not alter protocol or runtime ownership. The public version enum and probe result give setup/doctor and admission one shared decision rather than duplicating executable parsing. No blocking documented-standard violation or material code smell was found.

The preliminary `eligible(argv, ClaudeVersion::V2_1_268)` is intentionally the union of the two supported grammars. This avoids probing clearly unrelated native commands; the later `eligible_launch(argv, detected_version)` prevents short-form admission on 2.1.267. It does not silently broaden the older version.

## Verification

Independent checks against the same target code passed:

```text
cargo test -p ovrcr --lib report::admission::tests::initial_admission_argv_accepts_only_certified_explicit_uuid_resume -- --exact
1 passed; 25 filtered out

cargo test -p ovrcr --test agent_setup doctor_reports_partial_config_proof_missing_and_unsupported_versions_without_secrets -- --exact
1 passed; 6 filtered out

cargo test -p ovrcr --test cli claude_doctor_reports_exact_version_and_version_specific_resume_forms -- --exact
1 passed; 28 filtered out

cargo test -p ovrcr --test server_lifecycle agent_admission_explicit_resume_preserves_argv_and_collects_conversation_usage -- --exact --nocapture
1 passed; 78 filtered out

git diff --check 3b39158..c169939
exit 0; empty output
```

The coordinator separately reported the new `agent_admission_short_resume_on_2_1_268_preserves_argv_and_lifecycle` test passing 1/1 with 78 filtered out. The implementation report records all nine admission unit tests, all seven setup tests, the focused lifecycle and unavailable-argv cases, and the serialized CLI suite passing 29/29.

Two retained default-parallel CLI runs each failed 1/29 in a different pre-existing status-line subprocess timing assertion with empty stdout. The exact rerun of the first failed test passed, the second test passed in the other parallel run, and the serialized suite passed 29/29. The reviewed diff changes no status-line execution, timeout, sleep, or retry behavior. These are visible nonblocking regression-suite gaps; they are not passing parallel-suite evidence.

## Remaining acceptance gates

Implementation review passes, so the coordinator can proceed to the actual 2.1.268 GUI/native gate. Support is not complete until that reviewed-build run proves fresh launch, long and short UUID resume, bound identity, source recovery, synchronous callbacks, foreground ownership, retained Partial history, child exclusion, normal exit, and attributable cleanup.

The native run must retain the exact executable version and argv. It cannot widen short-form support to 2.1.267 or infer support for adjacent versions, continue, picker/name/search, equals syntax, in-process transitions, Complete accounting, finality, or Confirmed activity.

Review result: **approved for native acceptance, with no blocking implementation finding**. No source, test, or documentation edit, staging, commit, or native provider execution was performed by this reviewer; this report is the only review artifact added.
