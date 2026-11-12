# #468 Submit/wakeup race CI follow-up

## Revision and cause

- Ticket branch: `codex/468-session-watches`.
- Follow-up commit: `882def7aa907d47f28c40de4d8d5718cf81ecfb1` (`test(operator): make wakeup race hook one-shot`).
- Integration base: fast-forwarded to `bb3a890c` before this follow-up; the base is an ancestor of the final ticket commit.
- Reproduced the reported failure against `bb3a890c`: `user_submit_wins_async_wakeup_admission_without_losing_the_report_or_turn` timed out waiting for the async RPC at line 689 (1 test failed after 5.31s). Full RTK output: `468-logs/ovrcr-468-ci-race-red-full.log`; compact result: `468-logs/ovrcr-468-ci-race-red.log`.
- Source diagnosis matched the CI report. Submit and BeginAsyncTurn both call `operator_warm_dispatchable`, which invokes the shared `before_retained` test hook. The test's barrier was blocking both calls for up to five seconds.

## Test-only fix

In `crates/ovrcr-runtime/src/server/operator_tests.rs`, the race test now guards the barrier with `AtomicBool::swap`. The first hook invocation signals and waits; subsequent invocations return immediately. No production code or acceptance assertions changed. The test still asserts that human Submit wins, the user turn is Running on the leased run, the human message is retained, and no AsyncWakeup turn was created.

## Checks

All Cargo commands cleared `OVRCR_AGENT_SOCKET` and `OVRCR_AGENT_TOKEN` and used `CARGO_TARGET_DIR=/private/tmp/ovrcr-467-ci-target`. Socket-backed tests ran with the task-owned test escalation.

| Check | Result | Log |
| --- | --- | --- |
| Pre-fix race reproduction | 0 passed, 1 failed at async response timeout | `468-logs/ovrcr-468-ci-race-red.log` |
| `cargo test -p ovrcr-runtime user_submit_wins_async_wakeup_admission_without_losing_the_report_or_turn -- --nocapture` | 1 passed, 563 filtered | `468-logs/ovrcr-468-ci-race-green.log` |
| `cargo test -p ovrcr-runtime operator_request_lock_order_never_holds_dashboard_while_waiting_for_retained -- --nocapture` | 1 passed, 563 filtered | `468-logs/ovrcr-468-ci-lock-order-final.log` |
| `cargo test -p ovrcr-runtime server::operator_tests -- --nocapture` | 20 passed, 1 ignored, 543 filtered | `468-logs/ovrcr-468-ci-server-suite-final.log` |
| `cargo fmt --all -- --check` and `git diff --check` | Passed | `468-logs/ovrcr-468-ci-race-fmt-final.log` |
