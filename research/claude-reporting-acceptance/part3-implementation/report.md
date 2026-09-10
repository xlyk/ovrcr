# Tasks 2-3 implementation report

Implemented against `3b391581` on 2026-09-10. This unit changes the exact Claude
version decision, initial-launch grammar, doctor/setup diagnostics, CLI help,
current documentation, and focused tests. It does not change reporting state,
activity quality, accounting coverage, collector behavior, completion semantics,
deadlines, retries, polling, sleeps, or other production timing.

## Result

- The existing bounded `--version` subprocess still runs once per eligible
  admission attempt, with its existing one-second deadline, 128-byte read bound,
  environment removal, process-group ownership, and cleanup. Its result now
  distinguishes supported exact version, unsupported detected Claude version,
  and unavailable/malformed probe.
- The supported executable allowlist is exactly `2.1.267` and `2.1.268`.
  Adjacent `2.1.266` and `2.1.269` remain unsupported.
- Fresh launch and separate-token `--resume UUID` remain supported on both exact
  versions. Separate-token `-r UUID` is supported only on exact 2.1.268. Resume
  argv is passed through unchanged. The existing canonical lowercase UUIDv4,
  root identity, source, collector, Partial usage, Observed activity, and
  transition-freeze checks are unchanged.
- Doctor reports `supported_versions`, the detected `version` when available,
  separate `supported`/`unsupported`/`unavailable` probe status, and
  version-specific `resume_forms`. Setup text, CLI help, and current docs describe
  the same contract.

Production and test files in this unit:

- `src/report/admission.rs`
- `src/cli/agent_setup.rs`
- `src/cli/args.rs`
- `tests/agent_setup.rs`
- `tests/cli.rs`
- `tests/server_lifecycle.rs`
- `docs/claude-code-setup.md`
- `docs/agent-reporting-support.md`
- `docs/cli-reference.md`

The coordinator-owned `research/claude-reporting-acceptance/provider-capabilities.md`,
`research/claude-reporting-acceptance/remaining-matrix.md`, and native acceptance
resources were not edited as part of this implementation unit. The concurrent
memory worker owns `tests/claude_reporting_load.rs`.

## RED evidence

Before production changes:

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib report::admission::tests::initial_admission_probe_cleans_descendants_after_leader_exit -- --exact --nocapture
```

Failed because exact 2.1.268 returned `false` while the new test expected
support. The captured output is
[`01-red-version-matrix.txt`](../../../research/claude-reporting-acceptance/part3-implementation/01-red-version-matrix.txt).

## Focused passing gates

All commands used `CARGO_INCREMENTAL=0`.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib report::admission::tests::initial_admission_probe_cleans_descendants_after_leader_exit -- --exact --nocapture
```

Passed 1/1. The matrix accepts successful exact 2.1.267 and 2.1.268, rejects
2.1.266 and 2.1.269, rejects a nonzero 2.1.268 probe, verifies detected-version
diagnostics on successful probes, and retains owned descendant cleanup.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib report::admission::tests::initial_admission_argv -- --nocapture
```

Passed 2/2. A later exact rerun of
`initial_admission_argv_accepts_only_certified_explicit_uuid_resume` also passed
1/1 after adding mixed-duplicate, short-form uppercase, prompt, missing-value,
and equals-form rejection assertions.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib report::admission::tests -- --nocapture
```

After sandbox socket denial, the escalated rerun passed 9/9. The denied output is
[`03-admission-suite-sandbox-denied.txt`](../../../research/claude-reporting-acceptance/part3-implementation/03-admission-suite-sandbox-denied.txt).

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup doctor_reports_partial_config_proof_missing_and_unsupported_versions_without_secrets -- --exact --nocapture
```

Passed 1/1. It covers both supported versions, both adjacent unsupported
versions, actual version output, exact allowlist output, per-version resume forms,
and unavailable probe output.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup -- --nocapture
```

After sandbox socket denial, the escalated rerun passed 7/7, including signal
cleanup for the owned version-probe group. The denied output is
[`04-agent-setup-sandbox-denied.txt`](../../../research/claude-reporting-acceptance/part3-implementation/04-agent-setup-sandbox-denied.txt).

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test cli claude_doctor_reports_exact_version_and_version_specific_resume_forms -- --exact --nocapture
```

Passed 1/1, including current CLI help assertions.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle agent_admission_explicit_resume_preserves_argv_and_collects_conversation_usage -- --exact --nocapture
```

Passed 1/1 with isolated socket permission. This preserves the 2.1.267 long-form
real PTY/socket path.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle agent_admission_short_resume_on_2_1_268_preserves_argv_and_lifecycle -- --exact --nocapture
```

Passed 1/1 with isolated socket permission. The first sandboxed attempt was
denied before the server could bind; its exact output is
[`02-short-lifecycle-sandbox-denied.txt`](../../../research/claude-reporting-acceptance/part3-implementation/02-short-lifecycle-sandbox-denied.txt).
The passing test reuses the full long-resume lifecycle assertion: unchanged argv,
foreign/child and contradictory-root rejection, matching resume bind, retained
pre-invocation Partial conversation history, new distinct usage, activity,
same-ID compaction, later transition freeze, native exit, and fixture cleanup.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle agent_admission_ineligible_argv_and_probe_failures_preserve_native_arguments -- --exact --nocapture
```

Passed 1/1 with isolated socket permission after adding 2.1.266, 2.1.269,
missing `--resume` value, and missing `-r` value cases. Native argv and exit remain
unchanged when admission is unavailable.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test cli -- --test-threads=1 --nocapture
```

Passed 29/29. Exact output is
[`08-cli-serial-pass.txt`](../../../research/claude-reporting-acceptance/part3-implementation/08-cli-serial-pass.txt).

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo fmt --all -- --check
rtk git diff --check
```

Both passed.

## Parallel CLI timing failures retained

Two default-parallel CLI-suite runs each passed 28/29 but failed a different
pre-existing status-line subprocess timing assertion with empty stdout:

1. `claude_statusline_preserves_decimal_and_renders_after_reporting_timeout`
   failed once. Its exact output is
   [`05-cli-parallel-failure-1.txt`](../../../research/claude-reporting-acceptance/part3-implementation/05-cli-parallel-failure-1.txt).
   The immediate exact rerun passed 1/1; retained output is
   [`06-cli-timeout-exact-rerun-pass.txt`](../../../research/claude-reporting-acceptance/part3-implementation/06-cli-timeout-exact-rerun-pass.txt).
2. A second parallel suite run passed that test and failed
   `claude_statusline_renders_default_without_reporting_server` once. Its exact
   output is
   [`07-cli-parallel-failure-2.txt`](../../../research/claude-reporting-acceptance/part3-implementation/07-cli-parallel-failure-2.txt).

The serial 29/29 pass shows the suite passes when its tests are not concurrent.
The cause of the two parallel-only timing failures was not investigated in this
unit. No timeout, sleep, retry, or status-line production behavior was changed.
These failures remain visible rather than being counted as passing parallel-suite
runs.

## Boundaries

No authenticated Claude process or native provider acceptance was run in this
unit. The coordinator owns exact 2.1.268 fresh, both resume spellings,
source-recovery, child API-failure, foreground ownership, exit, and cleanup
acceptance after independent review. No staging or commit was performed here.
With 944 MiB free during the final checks, no broad Clippy/workspace build was
added beyond the focused compiled targets above.
