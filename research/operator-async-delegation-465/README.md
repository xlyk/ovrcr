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

## Remaining acceptance

- [x] Leased and unleased worker lifecycle, same-run user turns, model updates,
  checkpoint permissions and retirement races (#467).
- [x] Forward SQLite migration and wire/controller/render coverage (#467).
- [ ] Deterministic watch actions, baseline settlement, idle batching and durable
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
