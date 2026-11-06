# GUI native-input sidecar report

- Branch: `codex/sidebar-session-jump-gui`
- Tip: `37bb9c82` (includes the GUI fix at `2a270ac5`, then integration merge `cac070cc`)
- Earlier integration base: `2ae54eee`

## Changes

- Observe macOS `ModifiersChanged(mac_cmd)` and synthesize one left-Super hold/release only when no physical Super edge already began the hold. Deduplicate physical edges when aggregate Cmd arrived first; preserve physical two-Cmd state when physical edges arrived first; defer the final physical release across either aggregate/physical release ordering.
- Encode Cmd+Backspace and Cmd+Escape as Kitty key edges only after observed `>11u`; preserve repeats, releases, focus cleanup, ordinary input, and unrelated Cmd shortcut behavior.
- Observe exact DEC 1004 `?1004h`/`?1004l` output because `vt100 0.16.2` exposes no focus-mode accessor. Send `CSI O` before focus-loss key releases and `CSI I` on focus gain only while observed focus reporting is enabled.
- The focused test now waits for target pane header plus `Session selected  release Cmd` before focus-loss assertions; it checks Browse, no selected hint, no label, and no Terminal mode.

## Verification

- GUI input + mode-observer unit suite: 26 passed. Command: `CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-gui rtk proxy cargo test --features gui --lib gui::`. Log: `gui-native-fix-unit.log`. Run on code based on `2ae54eee`, before commit `2a270ac5`.
- All-target GUI clippy: passed with `-D warnings`. Command: `CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-gui rtk proxy cargo clippy --features gui --all-targets -- -D warnings`. Log: `gui-native-fix-clippy-final.log`. Passed at `2a270ac5`, before merging #404.
- Real Dashboard PTY aggregate/control path: 1 passed. Test: `gui_aggregate_modifier_and_command_controls_reach_the_real_dashboard`. Command: `TMPDIR=/private/tmp/ovrcr-401-gui-temp CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-gui rtk proxy cargo test --features gui --test gui gui_aggregate_modifier_and_command_controls_reach_the_real_dashboard -- --nocapture`. Log: `gui-native-aggregate-dashboard.log`. This exercises aggregate-only Cmd label display, Cmd+Backspace/Esc cancellation, and Cmd+digit selection through the production GUI encoder and real Dashboard PTY.
- Real focus-loss PTY test: not yet verified. Test: `gui_focus_loss_cancels_a_selected_jump_before_terminal_mode`. Two elevated runs timed out during Demo server startup before the test body (10.34s and 10.28s); no assertions were reached. Attempt records: `gui-native-focus-loss-dashboard-startup-timeout-01.txt` and `gui-native-focus-loss-dashboard.log`. Root owns the serial rerun.
- A post-merge clippy run failed only in imported #404 code at `crates/ovrcr-tui/src/dashboard/state.rs:609` (`match_like_matches_macro`). #405 owns that correction. The GUI all-target clippy pass above predates that imported code.
- Native GUI CUA remains root-owned and pending; original native Cmd+1 red evidence remains in the integration research directory.

## Cleanup

- Removed task TMPDIRs `/private/tmp/ovrcr-401-gui-temp` and `/private/tmp/ovrcr-401-gui-focus-temp` after checking them. The sandbox-denied run's `ovrcr-gui-zXDKxC` log recorded socket-bind `Operation not permitted`; startup-timeout roots `ovrcr-gui-0Vi9p1` and `ovrcr-gui-7bRPAH` were absent when checked after their runs. No task-owned Demo fixture remains.
