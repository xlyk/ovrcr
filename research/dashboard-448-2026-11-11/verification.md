# Verification for dashboard #448

Final reviewed code: `0ce4ca2c8f5b091eecb55eb4280925a26e8df6b7` (protocol 84, schema 32). The evidence commit adds only this directory. All thirteen slices #449–#461 are integrated. The accepted decisions are Off before first Open / Idle afterward, an eighth Unknown usage counter, and the larger known conversation/invocation token total without duplicate cache/reasoning subsets.

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

The live Usage case accepts the same-conversation sequence 100 → rebind 90 → 110 and retains 100 then advances to 110. The terminal case covers actual application PTYs, four mixed panes, focus/input routing, header double-click zoom/unzoom, divider reset, repeated resize and nonce-tagged kernel dimensions.

An earlier full all-target workspace run failed twenty tests; it is not reported as green. The affected fixtures and regressions were corrected and the suites above rerun. An un-escalated TUI run failed three Bridge process-group tests; the unchanged escalated suite passed all 460. Prior hosted runs are historical evidence only.

## Hosted verification

[Current PR checks](https://github.com/xlyk/ovrcr/pull/464/checks) establish results on the exact published head. The existing workflow is also dispatched on that head to run Linux headless tests, actual Linux resource sampling, isolated 50-session capacity and memory high-water acceptance. These manual jobs are policy-skipped on ordinary PR events. No workflow exclusions or required checks were weakened by this change. The existing partial-resize exclusion and paid/ignored tests remain separate limits.

## Native verification and remaining limits

See [native acceptance](native-acceptance.md) for actual GUI input, screenshots, AX evidence, source revisions and fixture cleanup. No paid inference ran. Terminal.app computer use was denied by the tool safety policy; no alternate control route was used. GUI double-click/repeat delivery was not confirmed, although the real PTY acceptance passed those behaviors. The GUI minimum size prevents the shortest terminal-height layouts; those use frame coverage.

The PR remains unmerged pending user authorization. No original checkout, live user session or unrelated worktree was altered.
