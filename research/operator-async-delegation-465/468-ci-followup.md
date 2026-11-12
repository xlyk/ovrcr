# #468 CI follow-up evidence

## Revision and change

- Ticket branch: `codex/468-session-watches`.
- Follow-up commit: `b63bab60ab9a66c33f8a35fdf287139bba464874` (`test(tui): make wakeup heading check portable`).
- The branch was clean and fast-forwarded from `3eb28f49` to integration `13cbab82` before the follow-up; `13cbab82` is an ancestor of the follow-up commit.
- The follow-up changes only `crates/ovrcr-tui/src/dashboard/operator.rs`:
  - Replace the manual `trim_start_matches` character predicate with the Clippy-suggested `['│', ' ']` pattern.
  - Wrap the existing async wakeup heading test with the shared `pi_available_test::run_with_pi_available` fixture. Its assertions are unchanged.

## Reproductions and final checks

All Cargo commands used `CARGO_TARGET_DIR=/private/tmp/ovrcr-467-ci-target` and cleared `OVRCR_AGENT_SOCKET` and `OVRCR_AGENT_TOKEN`.

| Check | Result | Evidence |
| --- | --- | --- |
| Clippy finding on integration `13cbab82` | `manual_pattern_char_comparison`; root reproduction: `/private/tmp/ovrcr-465-watch-clippy.log` | Root CI follow-up |
| Hosted no-Pi failure before the fixture wrapper | 0 passed, 1 failed when `OVRCR_PI_EXECUTABLE=/nonexistent` | `468-logs/ovrcr-468-ci-heading-no-pi-red.log`; hosted run: `/private/tmp/ovrcr-465-ci-watches-shard3.log` |
| Heading test with the fixture wrapper and `OVRCR_PI_EXECUTABLE=/nonexistent` | 1 passed, 477 filtered | `468-logs/ovrcr-468-ci-heading-postfmt.log` |
| `cargo clippy -p ovrcr-tui --all-targets -- -D warnings` | Passed, no issues | `468-logs/ovrcr-468-ci-clippy-postfmt.log` |
| `cargo fmt --all -- --check` and `git diff --check` | Passed | `468-logs/ovrcr-468-ci-fmt-final.log` |

The focused test was run under the unavailable-Pi setting both before and after adding the fixture wrapper. The test now installs `/bin/sh` only in its isolated child process, following the established Dashboard test pattern, so it does not depend on a developer's installed Pi binary.
