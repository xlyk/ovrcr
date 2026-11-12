# Operator async session delegation acceptance

Spec: [#465](https://github.com/xlyk/ovrcr/issues/465). Implementation:
[PR #471](https://github.com/xlyk/ovrcr/pull/471), branch
`codex/465-integration`, based on `ceb8f06d`.

This report is in progress. Unverified requirements remain open.

## Capacity and control fences (#466)

Implementation commit: `5ce4fdc3`; integrated by `3083aac1`.

The new compiled-binary regressions failed on the original code:

- A running Operator accepted generic `SendTerminal` input.
- With 49 live Workspace sessions and a running Operator, creating the 50th
  Workspace session returned `Conflict: all 50 live process slots are occupied`.
- Before a worker existed, the Operator row lacked an explicit scope refusal.

After the fix:

| Check | Result |
| --- | --- |
| `cargo test -p ovrcr --test operator_scope` | 10 passed |
| `cargo test -p ovrcr --test pi_delegation public_pi_research_assignment_launches_collects_and_fences_real_child` | 1 passed |
| `cargo fmt --all --check` | Passed |
| `git diff --check` | Passed |

The scope suite uses a compiled Server, private sockets, Workspace PTYs and a
deterministic fake Pi. It covers both admission orders for 50 Workspace sessions
plus one Operator, refusal of the 51st Workspace session, and Server/CLI Paste,
Keystroke, Pause and Resume refusals for idle and running Operator rows. The
existing delegation test preserves its capacity refusal at 50 Workspace sessions.
Tests cleared `OVRCR_AGENT_SOCKET` and `OVRCR_AGENT_TOKEN`.

Draft CI at `3083aac1` found an existing expectation that still requires Operator
to be refused at 50 Workspace sessions:
`operator_capacity_refuses_without_consuming_prompt_or_run`.
[Failed macOS shard](https://github.com/xlyk/ovrcr/actions/runs/43102686631/job/130797927456).
The other three macOS shards, offline Cursor preflight and selection audit passed;
Linux gates were skipped. These results apply only to that draft revision.

Correction `caae6b32` replaces the superseded refusal assertion with acceptance,
journal/run advancement, and completion at 50 Workspace sessions. The owning
runtime regression passed (1 test; 503 filtered); formatting and diff checks
passed. [Attempt metadata](466-attempts.md) preserves the observed failures,
commands, exit statuses and known evidence limits.

At `130e2897`, all four macOS shards, offline Cursor preflight, selection audit and
the required aggregate passed ([run](https://github.com/xlyk/ovrcr/actions/runs/43106035491)).
The three Linux jobs were skipped. This covers the capacity revision only.

## Work-leased residency (#467)

Feature commit: `8df8b930`; integrated revision: `010e3812`.
[Attempts and source pointers](467-attempts.md) include preserved red/green logs.

| Check | Result |
| --- | --- |
| Operator e2e suite | 17 passed |
| Operator worker suite | 50 passed |
| Retained Operator tests | 20 passed |
| Registry migration | 14 passed; 1 ignored |
| Wire snapshot, leased-idle Dashboard, hidden CLI, runtime argv, Reporter Start | 1 passed each |
| Combined 50 Workspace plus Operator regression after merging #466 | 1 passed |
| Formatting and diff checks | Passed |

The live fake-Pi proofs cover same-run warm turns, model override/default and
thinking default restoration, private validated checkpoint copies, cold resume
followed by warm turns, shutdown refusal while leased, clean retirement and forced
owned-group cleanup. Cold worker arguments carry only the run identity. SQLite
schema 33 preserves the journal and receipt children while removing the old
one-turn-per-run constraint; wire protocol 85 carries the new state.

Pi writes its active session under a private per-run directory. Completed turns
retain the canonical `turns/<turn>/session.jsonl` checkpoint path; cleanup removes
superseded copies after an acknowledged completion. Native Pi and GUI acceptance
remain separate, open gates.

Hosted residency CI at `ce9193d6` found a Clippy `collapsible_if` warning and a
lock-order test hook placed after the new first retained lookup
([run](https://github.com/xlyk/ovrcr/actions/runs/43110937314)). Correction
`6506e4e5` keeps the lock-order assertions, moves the hook to the first lookup,
and uses a let-chain for the same-run UI guard. The runtime regression passed
(1 test), owning TUI Clippy passed, and formatting/diff checks passed. New hosted
results are pending.

The next run at `4244a8c4` cleared the residency Clippy and lock-order failures.
Its required aggregate still failed because
`nested_four_pane_terminal_acceptance_preserves_sessions_and_pty_sizes` failed
three attempts ([run](https://github.com/xlyk/ovrcr/actions/runs/43115269908)).
The four-pane failure is under separate investigation; it is not a passed gate.

## Session watches and wakeups (#468)

Integrated commit: `3eb28f49`.
[Attempts and log index](468-attempts.md) preserve the development failures and
final focused results.

| Check | Result |
| --- | --- |
| Wire tests | 6 passed |
| Retained Operator tests | 21 passed |
| Server Operator tests | 20 passed; 1 ignored |
| Dashboard Operator tests | 32 passed |
| Operator e2e suite | 18 passed |
| Operator worker suite, including the Node extension probe | 50 passed |
| Formatting and diff checks | Passed |

The Node probe covers add/list/remove, actual string/null baseline types,
pre-settled catch-up, stopped Agents without a current Reporter snapshot,
inventories containing more than 50 rows, and stale in-flight poll protection.
Polling uses only `terminal list --json`, with a five-second timeout and bounded
output, and does not overlap. Reports batch while busy and remain queued after a
declined wakeup admission.

The live fake-Pi trace demonstrates an early preflight denied while the previous
turn is still committing, followed by successful idle admission. Its retry is
response-driven at 2.5-second intervals within a fixture budget. The worker
streams the extension-triggered turn without issuing a second RPC prompt.
Dashboard cases cover the Operator heading, one unread result across all terminal
outcomes, both drafts and busy refusal.

## Remaining acceptance

- [x] Leased and unleased worker lifecycle, same-run user turns, model updates,
  checkpoint permissions and retirement ordering (#467).
- [x] Forward SQLite migration and wire/controller/render coverage (#467).
- [x] Deterministic watch actions, baseline settlement, idle batching and durable
  wakeups with unread/draft behavior (#468).
- [ ] Leased Stop/failure/timeout rollback, extension state survival and hard
  termination fallback (#469).
- [ ] Isolated coordinator skill and provider-aware workflow (#470).
- [ ] Reviewed-checkout native GUI walkthrough with screenshots and accessibility
  evidence (#470).
- [ ] Final workspace regression, lint, formatting and hosted checks.
- [ ] Independent Standards and Spec reviews of the integrated committed diff.

## Compatibility and boundaries

The spec's version literals predate the current base: `ceb8f06d` uses protocol 84
and schema 32. The implementation migrates forward from those versions. Matching
clients and Server are required; no live instance is upgraded by this task.

Offline fixtures establish OVRCR behavior. Native GUI evidence and any live
provider inference are separate claims. No paid model calls are part of this
acceptance plan.

The CLI's `unread.turn` is an opaque string or null (`ReadyObservation.turn`), so
the watch baseline preserves that identity and detects a changed non-null turn
alongside the numeric activity revision. This corrects the spec's numeric-turn
assumption without changing the CLI format.

Watch delivery reserves its asynchronous turn through the existing extension UI
handshake before calling `sendUserMessage`. That lets the Server serialize the
reservation against Submit; a losing wakeup keeps its reports and starts no model
work. Stock Pi extension APIs remain unmodified.

## Watch CI follow-up

At `13cbab82`, run [43121584362](https://github.com/xlyk/ovrcr/actions/runs/43121584362) exposed a Clippy manual-character comparison in the new heading test and the same test’s implicit dependence on an installed Pi. `b63bab60` uses the established isolated Pi-availability fixture and a character-array pattern; its assertions are unchanged. The unavailable-Pi reproduction fails before and passes 1/1 after. Owning TUI Clippy, fmt and diff checks pass. See [follow-up attempts](468-ci-followup.md).

The earlier four-pane terminal failure was not reproduced in one isolated offline run (1/1); [diagnosis](terminal-ci-diagnosis.md) distinguishes the observed picker/COPY screens from the inferred synchronization issue. No terminal-test source or assertion changed. This local pass does not clear required hosted checks.

At `13cbab82`, hosted shard4 passed all738 selected tests; the four-pane test passed on its first attempt (5.935s). Shard2, offline Cursor and selection audit also passed. Required aggregate was red only for the two watch-test issues corrected in `b63bab60`. Final current-head CI remains required.

## Submit/wakeup barrier CI correction

Hosted run [43122941722](https://github.com/xlyk/ovrcr/actions/runs/43122941722) at `bb3a890c` passed shards2–4, offline Cursor and selection audit. Shard1 exposed a repeated blocking test hook in the new Submit/wakeup regression (three timeouts); an existing GUI palette check passed on its second attempt. The hook now blocks only its first invocation in `882def7a`, preserving the race assertions. The original failure reproduces before the fix; race1/1, lock-order1/1 and Server Operator20 passed/1ignored after it. See [attempts](468-ci-race-followup.md). Current-head CI is still required.
