# Verification for dashboard #448

Behavior-reviewed source: `0ce4ca2c8f5b091eecb55eb4280925a26e8df6b7` (protocol 84, schema 32). Evidence commit `fb7cd309` adds only this directory. The current follow-up also includes a Linux cfg/visibility correction and a deterministic live-palette fixture correction after that evidence commit. Linux resource parsers are visible to their sibling tests, the mutable settings row binding is compiled only on macOS, and the typed-branch integration test now seeds its launcher in the private Server settings before startup and waits for the initial Server settings report. These changes do not alter production behavior or the native and review evidence below. All thirteen slices #449–#461 are integrated. The accepted decisions are Off before first Open / Idle afterward, an eighth Unknown usage counter, and the larger known conversation/invocation token total without duplicate cache/reasoning subsets.

## Local checks

Rust runs used two build jobs and cleared inherited agent socket/token authority. Socket/PTY and Bridge tests used isolated fixtures with the required process permissions. Pi-available tests run in exact child processes; the parent suites explicitly exercised missing Pi.

| Check | Result | Source revision |
| --- | --- | --- |
| TUI unit suite | 460 passed | 1869f446, identical final code tree |
| Dashboard integration suite | 532 passed | 1869f446 |
| Application library | 164 passed | 168b9cba |
| Runtime + protocol, nine suites | 670 passed, 1 ignored | 168b9cba |
| Terminal acceptance, two threads | 34 passed, 1 ignored | 168b9cba |
| Live Server Usage archive/restart/rebind | 1 passed, 169 filtered | 168b9cba |
| Pi availability / first-open status | 1 passed | 168b9cba |
| New hover regressions | 3 failed before fix; focused suite 10 passed after | 1869f446 |
| Format, diff whitespace, all-target/all-feature check and clippy | Passed | 1869f446 |
| Integration all-target/all-feature check | Passed | 0ce4ca2c |
| Follow-up local resource/settings tests on macOS | Resources 11 passed serially; settings editor 31 passed; all-target check and clippy passed | Follow-up correction on `fb7cd309` |
| Typed-branch regression assertion against an unconfigured live Server | Failed as expected: 0 passed, 531 filtered; Server reported `agents: []` | Follow-up regression, before fixture fix |
| Exact live typed-branch test, missing-Pi CI-like environment | 1 passed, 531 filtered | Follow-up correction |
| Exact live typed-branch test, `/bin/sh` Pi-available-style environment | 1 passed, 531 filtered | Follow-up correction |
| Palette integration filter, serial | 122 passed, 410 filtered | Follow-up correction |
| Follow-up fmt/diff check, workspace all-target/all-feature check and clippy | Passed | Follow-up correction |

The live Usage case accepts the same-conversation sequence 100 → rebind 90 → 110 and retains 100 then advances to 110. The terminal case covers actual application PTYs, four mixed panes, focus/input routing, header double-click zoom/unzoom, divider reset, repeated resize and nonce-tagged kernel dimensions.

The first parallel resource test run passed 10 tests and missed a macOS CPU tick in the 30 ms live sampling test; a focused retry passed, and the complete resource tests passed serially (11 passed). The Linux resource parser tests are cfg-excluded on this host.

An earlier full all-target workspace run failed twenty tests; it is not reported as green. The affected fixtures and regressions were corrected and the suites above rerun. An un-escalated TUI run failed three Bridge process-group tests; the unchanged escalated suite passed all 460. Prior hosted runs are historical evidence only.

## Hosted verification

[PR checks](https://github.com/xlyk/ovrcr/pull/464/checks) and manual jobs below are results from the previously published head `fb7cd309`; they do not validate the follow-up corrections in this worktree. After the corrected head is published, the existing workflow runs Linux headless tests, actual Linux resource sampling, isolated 50-session capacity and memory high-water acceptance. These manual jobs are policy-skipped on ordinary PR events. No workflow exclusions or required checks were weakened by this change. The existing partial-resize exclusion and paid/ignored tests remain separate limits.

Hosted CI at `fb7cd309` exposed Linux-only parser visibility errors in resource tests and an `unused_mut` warning in macOS-specific settings row customization. The follow-up correction described above addresses those compile issues. The raw Linux job log is `/tmp/ovrcr-448-context/ci-linux-fb7.log`.

The typed-branch live Server test failed all three retries in each of two hosted shard runs (6/6 failures; raw logs `/tmp/ovrcr-448-context/ci-macos-shard2-fb7.log` and `/tmp/ovrcr-448-context/ci-pr-shard2-fb7.log`). The Server queues its greeting, starts its writer, then publishes settings; the test previously installed `fixture-agent` only in Dashboard memory. A late initial `SettingsChanged` could therefore remove the launcher and remembered project choice before form submission. The regression now waits for and checks the Server's initial settings, and writes the matching launcher and choice to the isolated `dashboard.toml` before Server startup. It failed against the default Server settings with `agents: []` and passes after the fixture correction. The Git fixture initializes explicitly with `git init -b main`; no reliance on user `init.defaultBranch` remains. The test's runner-controlled `PATH` is isolated by `Live`, and the exact test passed with both missing-Pi and `/bin/sh` Pi environment settings.

The local machine does not have `nextest`, so the exact test was run with `cargo test --all-features`, `CI=true`, `GITHUB_ACTIONS=true`, and `NEXTEST=1`; this is not claimed as a nextest run. The corrected source has not yet been rerun on hosted Linux or the hosted macOS shards. Those hosted results remain pending.

## Linux TUI fixture follow-up

The next manual Linux job (`879`; raw log `/tmp/ovrcr-448-context/ci-linux-879.log`) passed the earlier Linux fixes, then failed compiling the TUI integration target because `tests/tui/main.rs` exposed `tests/support/live.rs` only on macOS. Sidebar tests at lines 922 and 1084 already use the shared Unix Server fixture on Linux. The module gate is now `cfg(unix)`; the root `deadline` module remains enabled on non-macOS and the palette's Linux import still resolves there. No tests were disabled and no production code changed.

On the available macOS host, both affected real-Server sidebar tests passed (1 each), and workspace all-target/all-feature check and clippy passed after the gate change. These checks do not establish Linux compilation; the corrected hosted Linux job remains pending.

The intentional red run's task-owned Server PID `35051` was absent afterward, and its private temporary root `.tmpfJ8j0a` had been removed. Green live runs use the same `Live` fixture shutdown and temporary-directory cleanup.

Follow-up commands (all Rust commands used `CARGO_BUILD_JOBS=2`, the private target directory, and cleared inherited agent socket/token authority). The first exact-test command also ran before adding the Server fixture settings; the regression assertion then failed with `agents: []` (0 passed, 531 filtered), and passed after the fixture was seeded as shown here.

```sh
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CI=true GITHUB_ACTIONS=true NEXTEST=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/private/tmp/ovrcr-ci-missing-pi rtk cargo test --all-features -p ovrcr --test tui palette::live_dashboard_creates_typed_branch_before_starting_agent -- --exact --nocapture
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CI=true GITHUB_ACTIONS=true NEXTEST=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/bin/sh rtk cargo test --all-features -p ovrcr --test tui palette::live_dashboard_creates_typed_branch_before_starting_agent -- --exact --nocapture
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CI=true GITHUB_ACTIONS=true NEXTEST=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/private/tmp/ovrcr-ci-missing-pi rtk cargo test --all-features -p ovrcr --test tui palette:: -- --test-threads=1
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target rtk cargo check --workspace --all-targets --all-features
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo fmt --all -- --check
rtk git diff --check
```

## Native verification and remaining limits

See [native acceptance](native-acceptance.md) for actual GUI input, screenshots, AX evidence, source revisions and fixture cleanup. No paid inference ran. Terminal.app computer use was denied by the tool safety policy; no alternate control route was used. GUI double-click/repeat delivery was not confirmed, although the real PTY acceptance passed those behaviors. The GUI minimum size prevents the shortest terminal-height layouts; those use frame coverage.

The PR remains unmerged pending user authorization. No original checkout, live user session or unrelated worktree was altered.
