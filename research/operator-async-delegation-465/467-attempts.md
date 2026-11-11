# OVRCR #467 evidence

## Revisions

- Feature commit: `8df8b930d40e7c6b556b04c38b5faa0168edaae9` (`feat(operator): keep leased workers warm across turns`).
- Final branch commit after merging `codex/465-integration` at `130e2897e8b108f7ff1e4b20db7097c105cabe6b`: `010e38125033b75255c65b1ef5d780c902111480`.
- Base: `ceb8f06d25096dfb490efca5861532b009b6949f`.
- The red/green runs below used the isolated worktree based on the base revision; each green implementation state was captured by the feature commit. The post-merge 50+1 capacity proof ran on the final branch commit.

## Red/green attempts

| Boundary | Attempt | Command/result | Source revision | Log |
| --- | --- | --- | --- | --- |
| Protocol snapshot | Red: stale golden | `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr-protocol wire_encoding_matches_snapshot_for_protocol_version -- --nocapture`; exit 101, 0/1 passed. Protocol-version and new Start identity bytes did not match the golden. | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-wire-red-2.log` |
| Protocol snapshot | Green: protocol 85 golden updated | Same command; exit 0, 1/1 passed. | Feature `8df8b930` source | `467-logs/ovrcr-467-wire-run-start-green.log` |
| Dashboard | Red: old idle worker was not submit-ready | `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr-tui leased_idle_worker_accepts_a_followup_on_the_same_run_without_stop_control -- --nocapture`; exit 101, 0/1 passed at `operator_submit_available()`. | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-tui-red.log` |
| Dashboard | Intermediate failure found Submit still pinned to the old turn identity | Same command; exit 101, 0/1 passed. The returned newer same-run turn was not adopted. | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-tui-green.log` |
| Dashboard | Green: composer accepts next turn on same run | Same command; exit 0, 1/1 passed. | Feature `8df8b930` source | `467-logs/ovrcr-467-tui-final-green.log` |
| Warm settings | Red: cold resumed context path was rejected | `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test --test operator_e2e leased_worker_reuses_one_live_run_for_warm_user_turns_then_retires -- --nocapture`; exit 101, 0/1 passed with `CONTEXT_UNAVAILABLE` on the warm path. | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-settings-red.log` |
| Warm settings | Red: explicit `high` remained after returning to Default | Same command; exit 101, 0/1 passed. Observed levels were `[high, high]`; expected captured profile default `medium` on turn three. | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-settings-red-2.log` |
| Warm settings | Green: model and thinking defaults restore across turns | Same command; exit 0, 1/1 passed. | Feature `8df8b930` source | `467-logs/ovrcr-467-settings-green.log` |
| Checkpoint lifecycle | Red: assertion still looked for native active session under the turn directory | `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test --test operator_e2e only_completed_turns_advance_the_checkpoint_across_failure_and_interruption -- --nocapture`; exit 101, 0/1 passed (`NotFound`). | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-checkpoint-e2e-green.log` |
| Checkpoint lifecycle | Green: canonical per-turn `session.jsonl` checkpoints and run-owned active session survive failure/interruption | Same command; exit 0, 1/1 passed. | Feature `8df8b930` source | `467-logs/ovrcr-467-checkpoint-e2e-green-2.log` |
| Worker integration suite | Red: one resumed-run assertion still expected `switch_session` under `turns/<turn>` | `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test --test operator_worker -- --nocapture`; exit 101, 49/50 passed. Actual target was `runs/<run>`. | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-operator-worker-suite.log` |
| Worker integration suite | Green after updating the assertion to the run directory | Same command; exit 0, 50/50 passed. | Feature `8df8b930` source | `467-logs/ovrcr-467-operator-worker-suite-green.log` |
| Registry migration | Sandbox attempt could not bind isolated Unix sockets | `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test --test registry_migration -- --nocapture`; 9 passed, 5 failed to start their fixture sockets, 1 ignored. This was a sandbox socket restriction, not a migration assertion failure. | Base `ceb8f06d` plus WIP | `467-logs/ovrcr-467-registry-green.log` |
| Registry migration | Green with the required socket access | Same command under the test runner's approved elevated socket context; exit 0, 14 passed, 1 ignored. | Feature `8df8b930` source | `467-logs/ovrcr-467-registry-final-green.log` |

## Final focused verification

Commands used `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN`; live suites use private temporary profiles, registries, sockets, workspaces and fake Pi processes.

- `cargo test --test operator_e2e -- --nocapture` — 17 passed, 0 failed. Log: `467-logs/ovrcr-467-operator-e2e-suite.log`.
- `cargo test --test operator_worker -- --nocapture` — 50 passed, 0 failed. Log: `467-logs/ovrcr-467-operator-worker-suite-green.log`.
- `cargo test -p ovrcr-runtime retained::operator::tests -- --nocapture` — 20 passed, 0 failed. Log: `467-logs/ovrcr-467-retained-final-green.log`.
- `cargo test --test registry_migration -- --nocapture` — 14 passed, 1 ignored, 0 failed. Log: `467-logs/ovrcr-467-registry-final-green.log`.
- `cargo test -p ovrcr-tui leased_idle_worker_accepts_a_followup_on_the_same_run_without_stop_control -- --nocapture` — 1 passed. Log: `467-logs/ovrcr-467-tui-final-green.log`.
- `cargo test -p ovrcr-protocol wire_encoding_matches_snapshot_for_protocol_version -- --nocapture` — 1 passed. Log: `467-logs/ovrcr-467-wire-run-start-green.log`.
- `cargo test -p ovrcr --bin ovrcr hidden_operator_worker_uses_run_identity_without_a_turn_argument -- --nocapture` — 1 passed on final merge commit `010e3812`. Log: `467-logs/ovrcr-467-cli-worker-green.log`.
- `cargo test -p ovrcr-runtime worker_argv_carries_run_identity_without_turn_identity -- --nocapture` — 1 passed on final merge commit `010e3812`. Log: `467-logs/ovrcr-467-run-argv-final-green.log`.
- `cargo test -p ovrcr --lib operator_uses_watched_lease_and_validates_exact_journal_receipts -- --nocapture` — 1 passed on final merge commit `010e3812`. Log: `467-logs/ovrcr-467-reporter-start-final-green.log`.
- Post-merge `cargo test --test operator_scope fifty_workspace_sessions_coexist_with_the_operator_dedicated_slot -- --nocapture` — 1 passed, 9 filtered. This confirms 50 live Workspace sessions coexist with Operator on the final branch. Log: `467-logs/ovrcr-467-capacity-combined-green.log`.
- `cargo fmt --all -- --check`, `git diff --check`, and staged diff checks passed.

## Architecture pointers

- Protocol identity, leased completion, and prepared control messages: [operator.rs](../../crates/ovrcr-protocol/src/operator.rs).
- Schema 33 rebuild, lease column, same-run turn acceptance, and turn receipt state: [retained/operator.rs](../../crates/ovrcr-runtime/src/retained/operator.rs), [config.rs](../../crates/ovrcr-runtime/src/config.rs).
- Exact-run liveness publication, warm Submit, and authenticated cold/warm Start dispatch: [server/operator.rs](../../crates/ovrcr-runtime/src/server/operator.rs).
- Run-scoped hidden CLI and authenticated Reporter Start/StartTurn/AbortTurn interfaces: [args.rs](../../src/cli/args.rs), [reporter.rs](../../src/report/reporter.rs).
- One active Pi session directory per `SessionRunId`, canonical `turns/<turn>/session.jsonl` checkpoint copies, settings restoration, settled shutdown ordering, and dual-source idle polling: [pi.rs](../../src/operator/pi.rs), [worker.rs](../../src/operator/worker.rs).
- Leased idle composer and stop semantics: [operator.rs](../../crates/ovrcr-tui/src/dashboard/operator.rs).
- Lifecycle and storage contract: [operator.md](../../docs/development/operator.md).

For #468, the worker already consumes the bounded `ovrcr_operator_watches` status in `Connection::poll_idle`; the actual `watch_sessions` tool and extension producer are not part of this change. For #469, authenticated `StartTurn` and `AbortTurn` command/result interfaces and Reporter methods are present; Server-side abort behavior and async wakeup turn creation remain downstream work.

## CI follow-up on integration head

- Integration head `ce9193d64d43a905d725d152f4c7214942d1b364` was merged into `codex/467-work-lease` before the fixes. The follow-up commit is `6506e4e5f2caa9d6709c73cfbc4a93b24be908d9` (`fix(operator): address residency CI findings`). The integration worktree remained at `ce9193d6` after the commit.
- Runtime lock-order regression: on `ce9193d6`, `CARGO_TARGET_DIR=467-logs/ovrcr-467-ci-target rtk env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test -p ovrcr-runtime operator_request_lock_order_never_holds_dashboard_while_waiting_for_retained -- --nocapture` — red, 0/1 passed, exit 101; Submit did not reach the hook because its warm lookup had already waited on retained. Log: `467-logs/ovrcr-467-ci-lock-order-red.log`.
- Runtime lock-order regression: same command after the fix — green, 1 passed, 0 failed (506 filtered), exit 0. The test hook now observes Submit before the warm lookup's first retained lock; the later duplicate hook was removed. Log: `467-logs/ovrcr-467-ci-lock-order-green.log`.
- TUI lint: `CARGO_TARGET_DIR=467-logs/ovrcr-467-ci-target rtk env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo clippy -p ovrcr-tui --all-targets -- -D warnings` — green, exit 0. The residency guard uses a let-chain to satisfy `clippy::collapsible_if`. Log: `467-logs/ovrcr-467-ci-tui-clippy-green.log`.
- `rtk cargo fmt --all -- --check`, `rtk git diff --check`, and staged diff checks passed. Only the two CI-fix source files were committed.

## Gaps

- Live application behavior was exercised through OVRCR's Server, Dashboard driver, RPC codec, fake Pi boundary and real owned process groups. The installed Pi acceptance and native GUI walkthrough were not run in this ticket worktree; the coordinator owns the broader acceptance pass.
- No full workspace CI/lint run was performed here. The primary focused suites ran on the feature tree later captured by `8df8b930`; final merge `010e3812` adds the coordinator integration changes. The Submit hunk resolved to the already-tested warm check with no capacity gate, and the final 50+1 test passed. The coordinator owns broader final checks.
