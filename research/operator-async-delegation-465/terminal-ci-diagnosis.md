# #465 terminal CI gate diagnosis

## Result

At hosted revision `4244a8c4d87946476991551e55d72e8f357c25df`, shard 4 reported three failed attempts of `nested_four_pane_terminal_acceptance_preserves_sessions_and_pty_sizes` (20.334s, 20.600s, 20.230s). The shard finished 738/739 tests; this was the only failed test.

Attempts 1 and 3 ended with the four-pane screen visible and the footer in `COPY` mode. The screen already contained the expected waiting, mouse, fourth, and third pane output and PTY-size responses, including the 36×80 zoom probe. Attempt 2 failed at `wait_focused_ready("fourth")`; its screen showed the `Agents` picker with `Search: j` and `No matching agents`.

One isolated local execution passed: 1 test passed, 34 filtered out, 8.78s. Full output is in `/private/tmp/ovrcr-465-terminal-ci-test-elevated.log`. The command was:

```sh
rtk env -i PATH="$PATH" HOME=/private/tmp/ovrcr-465-terminal-ci-home CARGO_HOME=/Users/xlyk/.cargo TMPDIR=/private/tmp CARGO_TARGET_DIR=/private/tmp/ovrcr-467-ci-target RUST_BACKTRACE=1 cargo test --offline -p ovrcr --all-features --test terminal_acceptance nested_four_pane_terminal_acceptance_preserves_sessions_and_pty_sizes -- --exact --nocapture
```

Cargo was offline; the process environment was cleared except for build/runtime paths and a private `HOME`. The test fixture itself uses a private temporary `HOME`, clears provider profile variables, and installs a fake `claude` command. No provider inference or personal credentials were used. A first sandboxed attempt did not reach the test: the fixture server's private Unix socket bind was denied (`Operation not permitted`); the Cargo transcript is `/private/tmp/ovrcr-465-terminal-ci-test.log` and server stderr is preserved at `/private/tmp/ovrcr-465-terminal-sandbox-server.log`; this is not application evidence.

## Source evidence

In `tests/terminal_acceptance.rs`, `OuterDashboard::wait_until` checks the parser's current screen before consuming more PTY output (around line 609). The test's `browse_screen` accepts either `BROWSE` or `No visible Agents` (around line 2006). In this fixture, Ctrl-G leaves Terminal mode and opens the sidebar-jump picker; with no Agent rows, its footer is exactly `No visible Agents  s Agents search` (`crates/ovrcr-tui/src/dashboard/state.rs`, around lines 1129–1137 and 2219–2223). That is an open modal, not a clean Browse frame.

The flaky tail sends Ctrl-G and Escape together, waits on that broad predicate, checks restored panes only by names/header count, then sends `Space s j` (around lines 2272–2279). The `Search: j` screen is consistent with the menu prefix not being active when `j` was processed. This makes a stale readiness predicate and queued modal/input transition the strongest test-side explanation; the captured run does not prove which event was late.

The `COPY` footer is a real `InputMode::Copy` state (`dashboard/hints.rs`, around lines 22–26). That mode is entered by `begin_copy`, reached through the `[` Copy Screen binding (`dashboard/state.rs`, around lines 2244–2268; `dashboard/keymap.rs`, around line 1269). The CI capture does not identify which input triggered it, so the cause of those two attempts remains uncertain. Dynamic header coordinates are computed from the rendered pane header. The fixed divider/probe coordinates are used earlier, before the final focus sequence; preceding pane size responses were correct.

## Relation to #466/#467 and suggested correction

The #466 and #467 changes do not touch split-pane state, the shared key/mouse dispatcher, or this acceptance test. #466 changes Operator tests; #467 changes Operator server/UI behavior, with its event-loop diff limited to a test summary fixture. The failure is therefore not attributable to a direct pane-logic edit in those PRs. It is not labeled historically pre-existing: I did not run a counterfactual base, and the local attempt passed.

Smallest suggested correction: make the tail wait for an acknowledged, clean `BROWSE` frame after closing the Ctrl-G picker (exclude `No visible Agents`, `Agents`, and `COPY`); drive `Space`, `s`, and `j` with an acknowledgement between the menu/group/action; and require the mouse unzoom transition plus clean Browse before sending the next key batch. Keep the pane-focus assertion. Do not treat a four-header screen alone as proof that overlays and input transitions have settled.

No source or assertion changes were made. Hosted failure log: `/private/tmp/ovrcr-465-ci-residency-rerun-shard4.log`.

At `13cbab82`, hosted shard4 passed all738 selected tests; the four-pane test passed on its first attempt (5.935s). Shard2, offline Cursor and selection audit also passed. Required aggregate was red only for the two watch-test issues corrected in `b63bab60`. Final current-head CI remains required.
