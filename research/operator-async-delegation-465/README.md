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

## Remaining acceptance

- [ ] Leased and unleased worker lifecycle, same-run user turns, model updates,
  checkpoint permissions and retirement races (#467).
- [ ] Forward SQLite migration and wire/controller/render coverage (#467).
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
