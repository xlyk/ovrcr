# Verification for dashboard #448

Behavior-reviewed source: `0ce4ca2c8f5b091eecb55eb4280925a26e8df6b7` (protocol 84, schema 32). Evidence commit `fb7cd309` adds only this directory. Follow-ups after that review fixed Linux cfg/visibility, a live-palette settings race, and view-only Dashboard navigation settling. The latest production correction clears the loading state for a ready assigned child while keeping the Dashboard in Browse until human ownership is explicitly granted; native captures remain evidence for their recorded revisions and do not cover this keyboard path. All thirteen slices #449–#461 are integrated. The accepted decisions are Off before first Open / Idle afterward, an eighth Unknown usage counter, and the larger known conversation/invocation token total without duplicate cache/reasoning subsets.

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
| Assigned view-only switch regression, before behavior fix | 0 passed, 1 failed at the missing completion redraw assertion | Latest follow-up, red |
| Assigned view-only switch regression, after behavior fix | 1 passed, 460 filtered | Latest follow-up |
| TUI unit suite | 461 passed | Latest follow-up |
| Dashboard integration suite with isolated missing-Pi parent | 532 passed | Latest follow-up |
| Real PTY assigned-child takeover/return acceptance | 1 passed, 34 filtered | Latest follow-up |
| Latest fmt, diff check, workspace all-target/all-feature check and clippy | Passed | Latest follow-up |

The live Usage case accepts the same-conversation sequence 100 → rebind 90 → 110 and retains 100 then advances to 110. The terminal case covers actual application PTYs, four mixed panes, focus/input routing, header double-click zoom/unzoom, divider reset, repeated resize and nonce-tagged kernel dimensions.

The first parallel resource test run passed 10 tests and missed a macOS CPU tick in the 30 ms live sampling test; a focused retry passed, and the complete resource tests passed serially (11 passed). The Linux resource parser tests are cfg-excluded on this host.

An earlier full all-target workspace run failed twenty tests; it is not reported as green. The affected fixtures and regressions were corrected and the suites above rerun. The latest un-escalated TUI unit run passed 458 and failed three Bridge process-group tests: one witness was killed and two Bridge workers did not complete. The same full suite with process permissions passed all 461; the three failures were fixture process restrictions, and no assertions changed. Prior hosted runs are historical evidence only.

## Hosted verification

[PR checks](https://github.com/xlyk/ovrcr/pull/464/checks) and manual jobs below are results from the previously published head `fb7cd309`; they do not validate the follow-up corrections in this worktree. After the corrected head is published, the existing workflow runs Linux headless tests, actual Linux resource sampling, isolated 50-session capacity and memory high-water acceptance. These manual jobs are policy-skipped on ordinary PR events. No workflow exclusions or required checks were weakened by this change. The existing partial-resize exclusion and paid/ignored tests remain separate limits.

Hosted CI at `fb7cd309` exposed Linux-only parser visibility errors in resource tests and an `unused_mut` warning in macOS-specific settings row customization. The follow-up correction described above addresses those compile issues. The raw Linux job log is `/tmp/ovrcr-448-context/ci-linux-fb7.log`.

The typed-branch live Server test failed all three retries in each of two hosted shard runs (6/6 failures; raw logs `/tmp/ovrcr-448-context/ci-macos-shard2-fb7.log` and `/tmp/ovrcr-448-context/ci-pr-shard2-fb7.log`). The Server queues its greeting, starts its writer, then publishes settings; the test previously installed `fixture-agent` only in Dashboard memory. A late initial `SettingsChanged` could therefore remove the launcher and remembered project choice before form submission. The regression now waits for and checks the Server's initial settings, and writes the matching launcher and choice to the isolated `dashboard.toml` before Server startup. It failed against the default Server settings with `agents: []` and passes after the fixture correction. The Git fixture initializes explicitly with `git init -b main`; no reliance on user `init.defaultBranch` remains. The test's runner-controlled `PATH` is isolated by `Live`, and the exact test passed with both missing-Pi and `/bin/sh` Pi environment settings.

The local machine does not have `nextest`, so the exact test was run with `cargo test --all-features`, `CI=true`, `GITHUB_ACTIONS=true`, and `NEXTEST=1`; this is not claimed as a nextest run. The corrected source has not yet been rerun on hosted Linux or the hosted macOS shards. Those hosted results remain pending.

## Linux TUI fixture follow-up

The next manual Linux job (`879`; raw log `/tmp/ovrcr-448-context/ci-linux-879.log`) passed the earlier Linux fixes, then failed compiling the TUI integration target because `tests/tui/main.rs` exposed `tests/support/live.rs` only on macOS. Sidebar tests at lines 922 and 1084 already use the shared Unix Server fixture on Linux. The module gate is `cfg(unix)`. The following Linux job (`533`; raw log `/tmp/ovrcr-448-context/ci-linux-533.log`) then found `deadline.rs` compiled twice: once at the TUI root and once inside the now-shared live fixture. The root module was removed and palette tests import the fixture's existing `live::wait_deadline` export. This keeps one deadline implementation and adds no lint suppression or test exclusions.

On the available macOS host, both affected real-Server sidebar tests passed (1 each), and workspace all-target/all-feature check and clippy passed after the gate change. The latest macOS checks also pass after removing the duplicate module. These checks do not establish Linux compilation; the corrected hosted Linux job remains pending.

The intentional red run's task-owned Server PID `35051` was absent afterward, and its private temporary root `.tmpfJ8j0a` had been removed. Green live runs use the same `Live` fixture shutdown and temporary-directory cleanup.

## Assigned view-only navigation follow-up

Hosted macOS job `879` failed this real takeover acceptance in all three attempts. The same case failed its first two retries and passed the third on the later `533` PR shard. The failure screen retained `Loading agent` after the assigned child was selected. The readiness path cleared `agent_typing` only when `input_is_allowed()` was true; controller-owned view-only sessions never satisfy that input check. The pending marker therefore survived a ready view and kept navigation in the loading state.

The new Dashboard regression failed before the fix because a ready view did not report completion for redraw. It now passes while asserting Browse mode, denied input, unchanged Controller ownership, and no PTY bytes or input requests before or after readiness. The fix clears the pending navigation marker once the exact view is ready and the input boundary has completed. It enters Terminal mode only when the existing input permission check succeeds. Assignment ownership, input guards, view-revision checks, and pending-input drain order remain in place.

The focused real PTY acceptance passed after the fix (1 passed, 34 filtered). That test verifies the child receives no bytes before explicit takeover, accepts nonce-marked human input after takeover, and rejects paste after the return fence. Its task-owned temporary root was absent after exit; an escalated process check found none of the recorded fixture PIDs or process groups. Hosted macOS and Linux checks on the corrected source remain pending.

Follow-up commands (all Rust commands used `CARGO_BUILD_JOBS=2`, the private target directory, and cleared inherited agent socket/token authority). The first exact-test command also ran before adding the Server fixture settings; the regression assertion then failed with `agents: []` (0 passed, 531 filtered), and passed after the fixture was seeded as shown here.

```sh
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CI=true GITHUB_ACTIONS=true NEXTEST=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/private/tmp/ovrcr-ci-missing-pi rtk cargo test --all-features -p ovrcr --test tui palette::live_dashboard_creates_typed_branch_before_starting_agent -- --exact --nocapture
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CI=true GITHUB_ACTIONS=true NEXTEST=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/bin/sh rtk cargo test --all-features -p ovrcr --test tui palette::live_dashboard_creates_typed_branch_before_starting_agent -- --exact --nocapture
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CI=true GITHUB_ACTIONS=true NEXTEST=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/private/tmp/ovrcr-ci-missing-pi rtk cargo test --all-features -p ovrcr --test tui palette:: -- --test-threads=1
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target rtk cargo check --workspace --all-targets --all-features
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo fmt --all -- --check
rtk git diff --check
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target rtk cargo test -p ovrcr-tui assigned_view_only_agent_switch_settles_without_flushing_input -- --nocapture
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/private/tmp/ovrcr-ci-missing-pi rtk proxy cargo test -p ovrcr-tui --lib
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/private/tmp/ovrcr-ci-missing-pi rtk cargo test -p ovrcr --test tui -- --test-threads=2
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CI=true CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target OVRCR_PI_EXECUTABLE=/private/tmp/ovrcr-ci-missing-pi rtk proxy cargo test --all-features -p ovrcr --test terminal_acceptance assignment_control::assigned_child_is_view_only_until_explicit_dashboard_takeover_and_return -- --nocapture --test-threads=1
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target rtk cargo check --workspace --all-targets --all-features
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/ovrcr-448-461/target rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Native verification and remaining limits

See [native acceptance](native-acceptance.md) for actual GUI input, screenshots, AX evidence, source revisions and fixture cleanup. No paid inference ran. Terminal.app computer use was denied by the tool safety policy; no alternate control route was used. GUI double-click/repeat delivery was not confirmed, although the real PTY acceptance passed those behaviors. The GUI minimum size prevents the shortest terminal-height layouts; those use frame coverage.

The PR remains unmerged pending user authorization. No original checkout, live user session or unrelated worktree was altered.
