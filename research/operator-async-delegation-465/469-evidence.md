# OVRCR #469 leased foreground Stop evidence

## Revision and scope

- Feature branch: `codex/469-soft-stop`.
- Implemented commit: `1405e5b245bab08b2bfad797ae88de9958cb1bc1` (`feat(operator): roll back leased foreground stops`).
- Parent: integrated #468/#467 tip `a47c66783ace1fc3e0a1e5a731f17379d7de6b15` (hosted CI reported by coordinator: 7 passed, 3 Linux skipped).
- Focused local test runs used checkout HEAD `3eb28f49` plus the then-current #469 worktree; they predate the fast-forward to `a47c6678`. The merge changed the #468 TUI test and its evidence, with no overlap in #469 source or acceptance tests; no #469 code changed after those passes.
- Protocol version remains 85 and SQLite schema remains 33, as agreed for parent #465.
- `git diff --cached --check`, targeted Rust formatting check, and both fixture `node --check` checks passed. Working tree is clean after the commit.

## Focused acceptance checks

All Rust commands below used `CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target`.

| Case | Command filter | Result |
| --- | --- | --- |
| Leased Stop rolls back in the same worker; holds `Running` until settlement; reconnect sees `Running` and Submit is refused until rollback; no checkpoint promotion; queued watch wakeup runs after rollback | `cargo test -p ovrcr --test operator_e2e leased_stop_rolls_back_on_the_live_worker_without_advancing_checkpoint -- --nocapture` | 1 passed, 21 filtered, 5.00s (final expanded test) |
| Hard fallback after unsettled Pi | `cargo test -p ovrcr --test operator_e2e leased_stop_uses_the_five_second_owned_group_fallback_when_pi_does_not_settle -- --nocapture` | 1 passed, 22 filtered, 9.87s; waited about five seconds, interrupted, confirmed exact owned group gone, checkpoint unchanged |
| First-turn Stop restores empty session | `cargo test -p ovrcr --test operator_e2e leased_first_turn_stop_restores_an_empty_native_session -- --nocapture` | 1 passed, 19 filtered, 4.94s |
| Zero-record failure skips `switch_session`, still clears/aborts, keeps worker leased and reusable | `cargo test -p ovrcr --test operator_e2e leased_zero_record_failure_keeps_the_worker_without_switching_sessions -- --nocapture` | 1 passed, 20 filtered, 5.29s |
| Failure after native writes rolls back, keeps partial journal, does not promote checkpoint, reuses worker | `cargo test -p ovrcr --test operator_e2e leased_failure_with_native_changes_rolls_back_and_keeps_the_partial_journal -- --nocapture` | 1 passed, 23 filtered, 4.98s |
| Tool timeout rollback precedes queued AsyncWakeup; failed turn is not a checkpoint | `cargo test -p ovrcr --test operator_e2e leased_tool_timeout_rolls_back_before_accepting_an_async_wakeup -- --nocapture` | 1 passed, 21 filtered, 4.98s |
| Same-process extension reload preserves active watches, queued reports, and in-flight poll, rebinds on resume, clears state on genuine shutdown | `rtk env CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target cargo test -p ovrcr --test operator_worker the_extension_forces_sequential_tools_and_bounds_its_bash_timeout -- --nocapture` | 1 passed, 49 filtered, 16.66s; nested extension probe printed `EXTENSION_PROBE PASS` |
| Stored failed-worker JSON compatibility | `cargo test -p ovrcr-protocol stored_failed_worker_rows_keep_their_historical_json_shape -- --nocapture` | 1 passed, 104 filtered; old `{"Worker":{"Failed":"..."}}` shape round-trips unchanged |

The extension probe also passed `node --check` for `tests/fixtures/operator-pi/extension_probe.mjs` and `tests/fixtures/operator-pi/operator_fake_pi.mjs`. Touched Rust files passed `rustfmt --edition 2024 --check`. The final probe's raw test output was not saved to a file; there is no existing raw-log path to hand off, and the passing test was not rerun.

## TDD Stop logs

Commands for this sequence used `rtk env CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target cargo test -p ovrcr --test operator_e2e leased_stop_rolls_back_on_the_live_worker_without_advancing_checkpoint -- --nocapture`, on base HEAD `3eb28f49` with the corresponding test/source worktree at each attempt.

| Log | Classification | Result |
| --- | --- | --- |
| `/private/tmp/ovrcr-469-stop-red-01.log` | Environmental fixture failure: sandbox denied fixture server socket creation; preserved separately from behavioral evidence. | 0 passed, 1 failed; 3.09s |
| `/private/tmp/ovrcr-469-stop-red-02.log` | Behavioral red: leased Pi worker did not remain live. | 0 passed, 1 failed; 2.91s |
| `/private/tmp/ovrcr-469-stop-green-01.log` | Intermediate compile failure: missing `AbortRequested` match arm; no test executed. | Build failed |
| `/private/tmp/ovrcr-469-stop-green-02.log` | Intermediate behavioral failure: Stop returned `Error` instead of `Interrupted`. | 0 passed, 1 failed; 9.11s |
| `/private/tmp/ovrcr-469-stop-green-03.log` | First actual green Stop run; preserved raw output at the original path. | 1 passed, 19 filtered; 4.98s |

The final expanded Stop test was rerun after adding the reconnect/held-settlement assertions and passed with 1 passed, 21 filtered, in 5.00s. It did not replace the preserved first-green raw log.

## #470 native acceptance pointers

- `crates/ovrcr-runtime/src/session/mod.rs:1060` — `Session::request_operator_abort`: validates the exact run/session/conversation and owned group before setting the per-turn abort marker and signaling the worker.
- `crates/ovrcr-runtime/src/server/operator.rs:578` — `stop_operator_dashboard`; `:8` defines the five-second soft-stop grace; it waits for `AbortTurn` confirmation before the existing exact-run owned process-group fallback.
- `src/operator/worker.rs:1974` — `rollback_turn`; the failure/Stop/timeout branches preserve the partial attempt journal, restore the previous completed checkpoint (or empty first-turn session), and report leased state without promoting the attempt.
- `src/report/reporter.rs:255` — `operator_abort_turn` report path.
- `src/pi-operator-extension.mjs:41` — process-global `Symbol.for("ovrcr.operator.watchState")`; `:618` and `:652` implement session start/resume rebinding and preserve state during `session_shutdown(reason="resume")`.
- `tests/fixtures/operator-pi/extension_probe.mjs:693` — automated same-process resume proof with active watches, queued reports, and an in-flight poll. `tests/operator_e2e.rs:517`, `:692`, `:815`, `:892`, and `:971` cover Stop, five-second fallback, zero-record failure, recorded failure, and timeout/wakeup respectively.

The automated reload proof uses the real extension handlers in a single Node process, and the Rust end-to-end cases use the fake Pi. The actual installed Pi 0.87.1/native reload with active watches remains #470's acceptance run.

## Cleanup

The sandbox-denied red-01 fixture at `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/.tmpd3zPO7` and the task-owned timeout fake-Pi trace `/private/tmp/ovrcr-469-tool-timeout-fake-pi.jsonl` were inspected and removed after recording the outcomes above. Other attempt logs remain available at the paths listed above.
