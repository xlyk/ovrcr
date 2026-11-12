# Ticket #466 evidence

## Revision

- Base: `ceb8f06d` (`codex/465-integration` and `origin/main`).
- Final commit: `5ce4fdc3` (`fix: give Operator a dedicated live session slot`).
- The final green checks ran on the working tree later committed as `5ce4fdc3`;
  no repository changes followed those checks. Merging the latest integration
  branch reported `Already up to date`.
- CI follow-up commit: `caae6b32b56011fcd2317eabac603f5bd8cc7bfb`
  (`test: accept Operator at Workspace capacity`), based on the latest merged
  integration commit `6eb6da0eb06ab309d12f85ea0ea229361f06c876`. The focused
  runtime test and formatting checks below ran on the exact tree committed as
  `caae6b32`; the final merge check reported `Already up to date`.

## Attempts

Raw console output was not saved to attempt files. The observed output below is
recorded from the tool results; absent details are marked unknown. No matching
`operator_scope` or ticket #466 `.log`/`.out` file was found in the task
worktree. `/private/tmp/rem_pi_delegation.log` is not this task's run: it shows a
different test binary hash and `running 260 tests`; the recorded focused run
used a different binary and ran one test.

### A0 — Initial build attempt, status not captured

- Command: `rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr --test operator_scope generic_terminal_controls_refuse_a_running_operator_conversation -- --nocapture`
- Revision: `ceb8f06d` plus uncommitted test changes.
- Exit status: unknown; the wrapper did not return it.
- Tests executed: unknown; captured output ended during compilation. This
  attempt is not counted as a pass.

### A1 — Scope fence red

- Command: `rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr --test operator_scope generic_terminal_controls_refuse_a_running_operator_conversation -- --nocapture`
- Revision: `ceb8f06d` plus uncommitted test-only changes.
- Exit status: `101`.
- Tests: 1 executed, 0 passed, 1 failed, 9 filtered out.
- Observed failure: the Server accepted `SendTerminal` for the live Operator
  session (`Server route must refuse SendTerminal ...`).
- Raw console output: not saved.

### A2 — Capacity red

- Command: `rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr --test operator_scope fifty_workspace_sessions_coexist_with_the_operator_dedicated_slot -- --nocapture`
- Revision: `ceb8f06d` plus uncommitted test-only changes.
- Exit status: `101`.
- Tests: 1 executed, 0 passed, 1 failed, 9 filtered out.
- Observed failure: the 50th Workspace create while Operator was live returned
  `Conflict: all 50 live process slots are occupied`.
- Raw console output: not saved.

### A3 — Idle-scope refusal red

- Command: `rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr --test operator_scope generic_terminal_controls_refuse_a_running_operator_conversation -- --nocapture`
- Revision: intermediate, uncommitted worktree based on `ceb8f06d`; exact
  intermediate tree hash is unavailable.
- Exit status: `101`.
- Tests: 1 executed, 0 passed, 1 failed, 9 filtered out.
- Observed failure: an idle Operator row returned a conflict that did not
  identify Operator scope (`idle Operator scope must be explicitly refused by
  SendTerminal ...`).
- Raw console output: not saved.

### A4 — Test compile error

- Command: `rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr --test operator_scope generic_terminal_controls_refuse_a_running_operator_conversation -- --nocapture`
- Revision: `ceb8f06d` plus uncommitted test changes.
- Exit status: `101`.
- Tests: 0 executed; compilation failed.
- Observed failure: the test used nonexistent `OperatorMessageContent::Assistant`
  variants (two compiler errors). The test was corrected before implementation
  acceptance runs.
- Raw console output: not saved.

### A5 — Final scope and capacity suite green

- Command: `rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr --test operator_scope`
- Revision: working tree later committed as `5ce4fdc3`.
- Exit status: `0`.
- Tests: 10 executed, 10 passed, 0 failed, 0 ignored.
- Covers the 50 Workspace + 1 live Operator case, refusal of the 51st Workspace
  session, Operator submission at full Workspace capacity, and CLI/Server Paste,
  Keystroke, Pause and Resume refusals for idle and live Operator scope.
- Raw console output: not saved.

### A6 — Existing delegation capacity regression green

- Command: `rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr --test pi_delegation public_pi_research_assignment_launches_collects_and_fences_real_child`
- Revision: working tree later committed as `5ce4fdc3`.
- Exit status: `0`.
- Tests: 1 executed, 1 passed, 0 failed, 190 filtered out.
- Raw console output: not saved.

### A7 — Formatting and diff checks

- `rtk cargo fmt --all --check`: exit `0`; 0 tests executed; working tree later
  committed as `5ce4fdc3`.
- `rtk git diff --check`: exit `0`; 0 tests executed; working tree later
  committed as `5ce4fdc3`.

CLI help was inspected with `target/debug/ovrcr` for `terminal send`,
`terminal keystroke`, `pause`, and `resume` (exit `0`, 0 tests). That inspection
preceded the final wording tweak to Pause/Resume help and was not repeated, so it
is not claimed as final help-output verification.

## CI regression follow-up

### CI failure preserved

- Failing revision: draft PR head `3083aac1`.
- CI command: `cargo nextest run --workspace --all-targets --all-features --profile ci -E 'not test(partial_resize_connection_delivers_error_before_owner_close)' --partition hash:3/4`.
- CI run/job: [run 43102686631, job 130797927456](https://github.com/xlyk/ovrcr/actions/runs/43102686631/job/130797927456).
- Exit status: `100`.
- Shard summary: 615/788 tests run, 614 passed, 1 failed, 2265 skipped.
- The old `operator_capacity_refuses_without_consuming_prompt_or_run` assertion
  expected Submit to return `Conflict` with 50 live Workspace sessions. The
  dedicated Operator slot now accepts the turn, so that stale assertion failed
  at `operator_tests.rs:740`. Nextest retried it three times; each test attempt
  reported 0 passed, 1 failed, and 511 filtered out.
- Preserved raw CI output: `/private/tmp/ovrcr-465-ci-capacity-failure.log`.

### A8 — Full-Workspace Operator runtime regression green

- Command: `rtk proxy zsh -o pipefail -c 'rtk proxy env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr-runtime --lib operator_uses_dedicated_capacity_at_fifty_workspace_sessions -- --nocapture 2>&1 | rtk proxy tee /private/tmp/ovrcr-466-runtime-test-A8.log'`.
- Tested revision: `caae6b32b56011fcd2317eabac603f5bd8cc7bfb` (the test ran
  before commit on the same tree).
- Exit status: `0`.
- Tests: 1 executed, 1 passed, 0 failed, 503 filtered out.
- The updated regression confirms that at 50 live Workspace sessions, Operator
  submission journals the prompt, advances the run to 1, remains Running, then
  completes through the existing fixture worker.
- Preserved raw test output: `/private/tmp/ovrcr-466-runtime-test-A8.log`.

### A9 — Follow-up formatting and diff checks

- `rtk cargo fmt --all --check`: exit `0`; 0 tests executed; same committed tree
  as `caae6b32`.
- `rtk git diff --check`: exit `0`; 0 tests executed; same committed tree as
  `caae6b32`.

## Environment and remaining checks

The integration fixtures used temporary Server instances, sockets, Workspace
PTYs and a deterministic fake Pi worker. Test processes ran with
`OVRCR_AGENT_SOCKET` and `OVRCR_AGENT_TOKEN` unset. Full workspace/lint checks
remain coordinator-owned; no GUI check was required for this CLI/Server change.
