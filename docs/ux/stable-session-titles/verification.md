# Stable session titles — verification

Issues [#159](https://github.com/xlyk/ovrcr/issues/159) and [#160](https://github.com/xlyk/ovrcr/issues/160). macOS, 2026-09-23. Base `68c771e97536e5e2a04a58e0552adefe8fef5e69`; tested the implementation and final test changes in this commit before committing. Only evidence/documentation was added after the final checks.

## Final checks

Raw logs are gzip-compressed; decompression preserves the original output.

| Command | Exit | Result |
| --- | --- | --- |
| `env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=2` | 0 | **1,055 passed, 0 failed, 20 ignored**, 32 test binaries. [Log](checks/full-suite-final.log.gz). |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 | [Log](checks/clippy.log.gz). |
| `cargo check --workspace --all-targets --all-features` | 0 | [Log](checks/check-final.log.gz). |
| `cargo fmt --all -- --check` | 0 | [Log](checks/fmt-final.log.gz), no output. |
| `git diff --check` | 0 | No whitespace errors. |

Ignored tests are existing helper entrypoints and opt-in acceptance cases, not disabled tests. Helpers are exercised by their parent integration cases. Linux, the explicit fifty-session load/memory runs, installed-provider checks, and opt-in memory benchmarks were not run; ignored cases are not counted as passes.

Focused development checks also passed: all five title integration tests, both title unit tests, the CLI title lifecycle test, the TUI manual-title test, all 33 CLI tests, and all 19 retained-session tests (two helper entrypoints ignored).

## Red/green evidence and unsuccessful attempts

All attempts used the same base plus the evolving feature diff. Logs are separate rather than overwritten.

| Attempt | Command | Exit / observation |
| --- | --- | --- |
| Live-title red | `cargo test -p ovrcr --test automatic_titles stable_workspace_launch -- --nocapture` | 101; 1 failed, 2 filtered. The original runtime displayed `first title 🦀` instead of `title-work`. [Log](checks/red-integration.log.gz). |
| Legacy-storage test compile | `cargo test -p ovrcr --test automatic_titles saved_titles` | 101; test did not run because SQLite parameters require a supported integer type. Corrected the fixture conversion. [Log](checks/red-retained.log.gz). |
| Legacy-storage red | Same command after fixture correction | 101; 1 failed, 3 filtered. Offline inventory displayed `Legacy app` instead of `title-work`. [Log](checks/red-retained-2.log.gz). |
| CLI reset red | `cargo test -p ovrcr --test automatic_titles agent_and_terminal` | 101; 1 failed, 4 filtered. CLI rejected the new `--reset` spelling. [Log](checks/red-reset-cli.log.gz). |
| First full attempt | `cargo test --workspace --all-targets --all-features`, launched in a background shell | 101; stopped at doctor signal cleanup (10 passed, 1 failed in that binary). Background signal disposition was the suspected cause; the unchanged test passed in a foreground run for all four signals. [Log](checks/full-suite.log.gz). |
| Foreground full attempt | `cargo test --workspace --all-targets --all-features --no-fail-fast` | 101; CLI, retained-session, and server-lifecycle targets failed. [Log](checks/full-suite-foreground.log.gz). |

The foreground attempt exposed one stale expectation for an application title after reset; that assertion now checks a null manual title, the original display name, and unchanged identity. Other CLI and Claude recovery failures were caused by inherited `OVRCR_AGENT_SOCKET` / `OVRCR_AGENT_TOKEN` routing test children toward the parent agent channel. Removing only those variables from the test command resolved them. The unchanged backpressure test passed in isolation and in the final two-thread full run; its earlier timing-sensitive failure remains recorded, not erased.

## Independent review

Separate Standards and Spec reviewers examined the diff against `68c771e`.

- **Standards:** two test-ordering blockers corrected. The no-title-event assertion now fences publication with a dispatcher-acknowledged empty `SetView`, rather than a connection-thread `List` response. Dashboard reconnection retries only the bounded, exact ownership conflict. [Re-review: no remaining blockers](review-standards-final.txt).
- **Spec:** the stale retained-session reset expectation was corrected without dropping lifecycle assertions. [Re-review: no remaining blockers](review-spec-final.txt).

## Native evidence

[Five screenshot/AX pairs and cleanup report](review.md) prove the actual dashboard's generated title, OSC 0/2 suppression, manual Rename, and clearing Rename. Production source was unchanged between that native run and final verification; later corrections affected tests only. The fixture launcher exited 0, and all recorded fixture processes/groups and paths were confirmed absent.
