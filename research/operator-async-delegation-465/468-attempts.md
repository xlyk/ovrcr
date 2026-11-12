# #468 session watches evidence

## Revision and scope

- Ticket branch: `codex/468-session-watches`.
- Ticket commit: `3eb28f49` (`feat(operator): add bounded session watches`).
- Integration base: fast-forwarded from `ce9193d6` to `4244a8c4` before final checks. This only updated the isolated ticket branch; no main branch or PR was changed.
- No new package or dependency was added. `watch_sessions` uses plain JSON Schema and Node built-ins.
- The branch adds the bounded lifecycle watcher, queued/batched idle wakeups, durable `AsyncWakeup` turn admission, and Dashboard handling for wakeup turns.

## Admission and lifecycle evidence

- Each watcher poll runs `OVRCR_EXECUTABLE terminal list --json` with a 5-second timeout and 1 MiB output bound. Polls do not overlap. A stale poll only settles the exact watch entry object it started with, so remove/re-add cannot inherit the old snapshot.
- A supplied baseline preserves `unread_turn` as the CLI's actual opaque string or `null`; only `activity_revision` is numeric and ordered. A changed non-null turn identity is detected by equality, without parsing or ordering. This matches `ReadyObservation.turn` and the serialized `terminal list --json` shape.
- Workspace Agent lifecycle checks use Reporter data and top-level phase/activity. The extension never polls terminal screen text. Missing Reporter observations are refused for live shells, while valid supplied baselines can catch up stopped/exited or changed-run targets.
- A settled report stays queued when the idle Pi confirmation cannot acquire the durable turn slot. Server admission and human Submit share the mutation lock and the same one-running-turn constraint. The worker journals the accepted async turn on the current leased run before confirming it, then Pi receives one `sendUserMessage` call.
- The fake Pi trace at `468-logs/ovrcr-468-async-wakeup-fake-pi-retry.log` records the initial `confirmed:false` at line 13 and the bounded 2.5-second retry accepted at line 14. The live e2e test verifies one durable AsyncWakeup on the same run, with no extra RPC prompt for that extension-triggered turn.
- Dashboard tests cover the clean Operator heading, one unread result for complete/failed/interrupted wakeups, preservation of composer and Quick drafts, and busy refusal while the async turn runs. No Bridge alert path is invoked.

## Final focused checks

All Cargo commands used `CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target` and cleared `OVRCR_AGENT_SOCKET` and `OVRCR_AGENT_TOKEN`. Socket/PTy suites ran with the approved test escalation.

| Check | Result | Log |
| --- | --- | --- |
| `cargo fmt --all -- --check` and `git diff --check` | Passed | `468-logs/ovrcr-468-fmt-integration-final.log` |
| `cargo test -p ovrcr-protocol wire_snapshot -- --nocapture` | 6 passed, 106 filtered | `468-logs/ovrcr-468-wire-final.log` |
| `cargo test -p ovrcr-runtime retained::operator::tests -- --nocapture` | 21 passed, 543 filtered | `468-logs/ovrcr-468-retained-suite-final.log` |
| `cargo test -p ovrcr-runtime server::operator_tests -- --nocapture` | 20 passed, 1 ignored, 543 filtered | `468-logs/ovrcr-468-server-suite-integration-final.log` |
| `cargo test -p ovrcr-tui dashboard::operator::tests -- --nocapture` | 32 passed, 446 filtered | `468-logs/ovrcr-468-tui-integration-final.log` |
| `cargo test -p ovrcr --test operator_e2e -- --nocapture` | 18 passed | `468-logs/ovrcr-468-e2e-integration-final.log` |
| `cargo test -p ovrcr --test operator_worker -- --nocapture` | 50 passed | `468-logs/ovrcr-468-worker-integration-final.log` |

The earlier Server module run against `ce9193d6` had one failure in the pre-existing lock-order test because the test hook ran after the warm-dispatch lookup. After the authorized fast-forward to `4244a8c4`, the module passed 20 tests with one ignored. Its initial log is `468-logs/ovrcr-468-server-suite-final.log`; the green rerun is recorded above.

Development red/green attempts remain in separately named files under `468-logs/ovrcr-468-*`. In particular, the first extension red is `468-logs/ovrcr-468-extension-red-01.log`, and the last standalone probe green is `468-logs/ovrcr-468-extension-green-11.log` (1 passed, 49 filtered). The probe covers add/list/remove, supplied and captured baselines, pre-settled catch-up, string/null turn IDs, invalid/live unmanaged targets, 51+ inventory rows, stale poll after remove/re-add, queue/batch behavior, and declined-confirm retry.

## Remaining acceptance boundary

- This ticket's offline Node, compiled Server/fake-Pi, and Dashboard TUI checks passed. Native GUI acceptance is coordinator-owned, along with broad checks, the work diary, PR creation, and any merge to the PR target.
