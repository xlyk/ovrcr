# Operator full-width check — 2026-11-03

The isolated native GUI was built from this branch with `gui,acceptance-diagnostics`. The only product change removes the 88-cell prose cap; the existing two-cell gutter stays.

- `cargo test -p ovrcr-tui operator_prose_uses_available_width_after_resize` failed before the fix: expected 140 cells, got 90.
- `cargo test -p ovrcr-tui dashboard::operator::tests`: 77 passed. The new test renders user and assistant prose through the Dashboard at 180, 80 and 140 columns, checks the rightmost cell and durable wrapping anchors.
- `cargo test -p ovrcr-tui`: 342 passed, 4 Bridge failures: `bridge_sound_resource_failure_is_a_silent_submitted_banner_without_retry`, `bridge_sound_choices_flags_and_both_lanes_use_server_settings`, `bridge_reaped_client_failures_do_not_signal_its_former_process_group`, `bridge_sound_changes_cancel_only_started_delivery_clients_without_replay`. Their causes were not investigated in this layout fix.
- Formatting, diff whitespace checks and `cargo clippy -p ovrcr-tui --all-targets -- -D warnings` passed.
- Native CUA: opened Operator, created a disposable conversation, pasted and submitted a long message. [Screenshot](wide.png) and [accessibility text](wide.txt) show text filling the available pane. The isolated account is signed out: the normal sign-in error appears after the persisted user message. No live provider response was tested.
- A local provider holder timed out after 600 seconds while the initial computer-use call was stalled. It exited 101. No provider requests were used for the captured check.
- A native resize drag did not change the window; resize coverage is automated only.
- The native close button exited the GUI owner with code 0; its process and disposable demo directory were verified absent. The failed holder had already exited. No installed app or personal instance was changed.

Full workspace/hosted CI and Linux GUI were not run for this local change.
