# OVRCR

Synchronous Rust terminal multiplexer. macOS + Linux.
Preserve one server owner, one active dashboard, 50-session support.
Change constraints only through agreed design.

## Work

- Read affected source, callers, docs. Verify plans against working code.
- Stay scoped. Reuse existing mechanisms. Resolve routine choices;
  raise contract conflicts before dependent edits.
- Check branch and working tree. Features use isolated worktrees.
  Preserve unrelated work.
- Protect live sessions and user data. Tests use isolated config,
  sockets, workspaces. Stop/remove only task-owned resources.
- No credentials in code, logs, captures, public messages.

## Prove

- Preserve behavior and coverage. Bug fixes: regression test failing
  on original defect where practical.
- Every feature: meaningful unit + integration tests covering real
  application path and relevant failures. Helpers/mocks alone insufficient.
- TUI features: CUA through Codex, Hermes, or capable harness.
  Run actual app from reviewed checkout. Verify input + visible outcomes.
  Keep screenshots and relevant accessibility evidence.
- Never weaken assertions or disable checks for green results.
  Changed expectations require intentional behavior change.
- Focused checks while developing; required acceptance + regression
  checks before delivery. Confirm test filters execute tests.
- Report revision, results, gaps. Failed/skipped/unavailable/unrun
  required checks never count as passed.

## Maintain

- Update affected docs, CLI help, config examples in same PR.
  Unverified roadmap items stay open.
- Fix debt needed for task. Report significant unrelated debt:
  location, impact, next step.
- Remove finished task's disposable resources. Keep required evidence,
  recovery state, resources with uncertain ownership.
- Instructions belong in narrowest guide. Root rules apply broadly
  or prevent serious mistakes before guide loads.

## Deliver

- Features merge through PRs. Required checks pass on current PR revision.
  No bypassing checks or required reviews.
- Review final diff against requirements. Resolve blocking findings.
  PR states problem, behavior change, verification, gaps.
- Stage intended files only. Merge requires user authorization.

## Read when relevant

Before edits: load guides covering affected behavior and shared callers.
Load more when crossing boundaries. No bulk loading at startup.

| Area | Guide |
| --- | --- |
| Crates, shared types, dependencies | [Architecture](docs/development/architecture.md) |
| Runtime, PTYs, sockets, persistence, lifecycle | [Runtime](docs/development/runtime.md) |
| Input, panes, rendering, History, Copy | [TUI](docs/development/tui.md) |
| Tests, fixtures, acceptance | [Testing](docs/development/testing.md) |
| TUI computer use | [CUA](docs/testing-computer-use.md) |
| PRs, reviews, merges | [Delivery](docs/development/delivery.md) |
| Issues, specs, tracker operations | [Issue tracker](docs/agents/issue-tracker.md) |
| Triage roles and labels | [Triage labels](docs/agents/triage-labels.md) |
| Domain vocabulary, architectural decisions | [Domain docs](docs/agents/domain.md) |

Tools: `rg` search; `git` + `gh` GitHub; `op` 1Password.

## Cursor Cloud specific instructions

Linux agents are headless. The GUI feature has no Linux windowing backend;
match the `linux` job in `.github/workflows/ci.yml` and omit `--all-features`.

- Rust stable, at least the workspace `rust-version` (1.95), plus rustfmt
  and clippy. The default image may be older.
- `zsh` is required. Shell integration tests invoke `/bin/zsh`.
- Node 22 runs `node --test tests/pi_reporting_extension.mjs
  tests/omp_reporting_extension.mjs tests/pi_delegation_extension.mjs`.
- `cargo nextest` uses the `ci` profile in `.config/nextest.toml`.
- Nothing starts on boot. Drive the CLI with an isolated `OVRCR_HOME`
  and `OVRCR_SOCKET`.
