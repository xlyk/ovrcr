# GUI focus freshness sidecar

Branch `codex/sidebar-jump-focus-freshness`, commit `ad23734b7f06cae8d16872eee4979e6afa07817e` (parent `f0ce72553bdfdfd9aeb263a20f543ef95c9f864c`). The branch is committed and clean; no merge, rebase, or cherry-pick was performed.

## Finding and change

On macOS, winit 0.30.13 queues `ModifiersChanged(NONE)` from `reset_modifiers()` before `Focused(false)` (`window_delegate.rs:215–227`, `view.rs:905–910`). egui-winit clears its modifier snapshot on focus loss but does not reread native modifiers on gain (`egui-winit/src/lib.rs:427–445`). A later mouse event calls winit's modifier updater (`view.rs:1049–1058`, `1078–1084`), so an aggregate Cmd state can reappear without a fresh Super press.

`KeyboardState` now quarantines aggregate Cmd after focus loss. The frame encoder reorders the exact synthetic clear/reset pair so focus cancellation is sent before releases, preserves quarantine across focus regain and mode changes, and rearms only on a focused clear modifier event, a fresh physical Super press, or a clear end-of-frame snapshot supported by native input after the latest focus gain. Stale Cmd digits and Cmd+Backspace/Escape are also suppressed during quarantine. `TerminalFrameInput` groups the native frame snapshot while keeping `encode_terminal_frame` to seven arguments.

Source: `src/gui/input.rs:88–107`, `420–445`, `715–825`; production caller `src/bin/ovrcr-gui.rs:588–612`; Dashboard PTY regression and sidebar-row lookup `tests/gui.rs:197–225`, `656–781`.

## Verification

- Red before the fix: `stale_aggregate_command_after_focus_regain_does_not_restart_a_hold` failed because stale aggregate Cmd emitted `CSI 57444;1:1u`. Log: `/private/tmp/ovrcr-401-implementation/focus-freshness-red-unit.log` (base `f0ce7255`, 1 failed).
- Green input suite: `CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-freshness rtk proxy cargo test --features gui --lib gui::input::tests:: -- --nocapture` — 27 passed. Log: `/private/tmp/ovrcr-401-implementation/focus-freshness-unit-input-final.log`.
- Green Dashboard PTY path: `CI=1 CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-freshness rtk proxy cargo test --features gui --test gui gui_focus_loss_cancels_a_selected_jump_before_terminal_mode -- --nocapture` — 1 passed. It exercised production `encode_terminal_frame`, asserted focus-loss `CSI O` precedes releases, suppressed stale Cmd, rearmed after a focused clear pointer frame, and verified the visible sidebar label. Log: `/private/tmp/ovrcr-401-implementation/focus-freshness-dashboard-final-02.log`. The test needed scoped permission for its task-owned Unix socket/process group.
- GUI Clippy: `CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-freshness rtk proxy cargo clippy --features gui --bin ovrcr-gui --test gui --no-deps -- -D warnings` passed; log `/private/tmp/ovrcr-401-implementation/focus-freshness-clippy-final.log`. The same command without `--no-deps` stops at the inherited #404 `match_like_matches_macro` lint in `crates/ovrcr-tui/src/dashboard/state.rs:609`; log `/private/tmp/ovrcr-401-implementation/focus-freshness-clippy.log`.
- `rtk proxy cargo fmt --all -- --check` and `rtk proxy git diff --check` passed after the final change.

Earlier failed attempts and their logs remain in the same notes directory (`focus-freshness-dashboard-01.log` through `-07.log`): the first hit sandbox socket denial; later attempts exposed an overly strict byte assertion and a test helper that searched the pane header instead of the sidebar side of `│`. The retained socket-startup fixture `ovrcr-gui-TC5GWZ` was removed after the harness reported the server process had exited; the final PTY test cleaned its fixture and verified child process groups exited.

## Remaining acceptance

Native GUI input was not exercised in this sidecar; root owns CUA and must rerun it on the integrated branch. If the host reports only aggregate Cmd, with no physical Super edge and no focused clear modifier/native-input observation after regain, the adapter cannot distinguish a new press from a stale held modifier and keeps it quarantined.

Coordinator clarification: the inherited lint belongs to #404 and its equivalent `matches!` fix is committed on the unmerged #405 branch at `91994038`. Full integrated Clippy remains unrun.
