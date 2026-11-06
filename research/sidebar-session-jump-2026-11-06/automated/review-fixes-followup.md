# Review fixes follow-up

Checkout: `/private/tmp/ovrcr-401-review-fixes`, branch `codex/sidebar-jump-review-fixes`.
Follow-up commit: `de90eea77eddc160528acaf4758b1d524656b7a6`, based on the merged
integration tip `4efdf48df21707742ff97a8af406ebfab23e71ea`. The worktree is clean.

## Corrections

- The four compiled terminal jump acceptance cases now opt into
  `OVRCR_DASHBOARD_INPUT_PROFILE=ovrcr-gui` only for their Dashboard child.
  `OuterDashboard::start` removes that variable and retains the Standard profile,
  so unrelated acceptance cases continue to exercise unsupported-host behavior.
  The opt-in follows the production CLI path: `src/cli/mod.rs` parses the known
  value and passes `VerifiedOvrcrGui` through `run_dashboard_with_input_profile`
  to the production Dashboard constructor.
- The Dashboard guide now limits Command-hold support to the bundled GUI and
  directs Standard CLI users to Agents search. It also distinguishes Command
  from modifiers that Browse folds into `Ctrl-t`.
- The adaptive badge test uses `symbol().starts_with(expected)` to preserve its
  cell assertion without the `chars_next_cmp` Clippy lint.

## Evidence

All Cargo commands used `--offline`, `CI=1`, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, and `CARGO_INCREMENTAL=0`. Green acceptance runs
used `/private/tmp/ovrcr-401-build` sequentially after the root full-test build
was quiescent.

- **Red profile reproduction, before fixture correction:**
  `cargo test -p ovrcr --test terminal_acceptance session_jump_adaptive::two_digit_jump_selects_the_tenth_agent_through_the_compiled_dashboard`
  — 0 passed, 1 failed, 27 filtered. The compiled Dashboard reached Browse but
  never rendered `[01]`/`[10]` because its child had the Standard profile.
  Log: `/private/tmp/ovrcr-401-review-fixes-followup-terminal-red.log`; full
  RTK output: `/Users/xlyk/Library/Application Support/rtk/tee/1793991564_cargo_test.log`.
- **Green compiled terminal jump cases:**
  `cargo test -p ovrcr --test terminal_acceptance session_jump -- --nocapture --test-threads=1`
  — 3 passed, 25 filtered (adaptive two-digit, retained, Operator).
  `/private/tmp/ovrcr-401-review-fixes-followup-terminal-green.log`.
- **Green core compiled Dashboard case:**
  `cargo test -p ovrcr --test terminal_acceptance command_digit_jumps_to_the_frozen_visible_agent_through_real_dashboard -- --nocapture --test-threads=1`
  — 1 passed, 27 filtered.
  `/private/tmp/ovrcr-401-review-fixes-followup-core-terminal.log`.
- **Green GUI active-hold Cmd-K case:**
  `cargo test -p ovrcr --features gui --bin ovrcr-gui command_palette_opens_when_another_control_has_focus -- --nocapture --test-threads=1`
  — 1 passed, 8 filtered. The test starts a real Dashboard Command hold,
  triggers the GUI Cmd-K path, and verifies the palette remains open after
  Command release. `/private/tmp/ovrcr-401-review-fixes-followup-gui-cmdk.log`.
- **Focused TUI regression:** adaptive jump tests — 8 passed, 397 filtered.
  `/private/tmp/ovrcr-401-review-fixes-followup-tui-adaptive.log`.
- **Focused Clippy:** `cargo clippy -p ovrcr --test tui --offline -- -D warnings`
  — no issues found. `/private/tmp/ovrcr-401-review-fixes-followup-clippy.log`.
- `cargo fmt --all -- --check` and `git diff --check` passed.

The red fixture's owned server PID was `57502`; its temporary root no longer
exists and the PID no longer exists. Acceptance fixtures shut down their owned
servers, sessions, and PTYs on successful completion. The full workspace suite,
full Clippy run, and native macOS CUA remain with the root task.
