# Issue 61: unread Ready responses

Implements [issue 61](https://github.com/xlyk/ovrcr/issues/61), using the
explicit acknowledgement contract in [issue 57](https://github.com/xlyk/ovrcr/issues/57).

## Scope and revisions

- Base: `b7f139f62b66eb10f73f22ffff51b8b074163fc3`, including prerequisite
  PR #56 through merge `cadb7a3` and the notification/sound follow-ups.
- Worktree: `/Users/xlyk/Code/ovrcr/.worktrees/issue-61-unread`.
- Branch: `codex/issue-61-unread`.
- Implementation: `8ebbf0b7984f8f277ba8958ddda9689fd2754d5f`.
- Reviewed correction and final tested code:
  `125c557a7e19b619a7e6e1077b61cc83f67ead0e`.
- One server-owned latest Ready identity per terminal. Explicit acknowledgement
  checks the displayed identity. Activity, quality, health and native behavior
  remain independent. No persistence after server death or terminal removal.
- No live installation, trust/configuration change, merge or release.

## Acceptance map

| Requirement | Passing evidence |
| --- | --- |
| Accepted root Ready establishes one unread observation | Runtime socket/PTY and managed CLI lifecycle assertions |
| Expected identity prevents stale clearing; duplicate ack is safe | Newer-Ready socket/CLI race and retry assertions; last-drawn identity tested through real event-loop message/input drain and socket writer |
| Replayed Ready cannot reopen reviewed state | Same binding/turn with higher transport revision |
| Busy, selection, viewing and reporter loss retain unread | Runtime lifecycle, dashboard PTY input, native screenshots |
| Mark-reviewed changes only unread | Full agent snapshot, PID and phase comparisons; native input/output |
| Reconnect retains state; terminal/server lifetime bounds it | `codex_unread_is_discarded_on_terminal_removal_and_server_restart` closes a terminal, stops the real server and starts another on the same saved config; reconnect covered separately |
| Dashboard input and narrow rendering | Browse shortcut, action menu, terminal-mode input, screenshot plus AX |
| Unavailable remains visible with retained unread | Reporter loss and shell-return rendering |
| Wire compatibility | Version fence, nonempty unread wire fixture and protocol tests |
| Synchronous ownership and 50-session support | Workspace regressions; hosted capacity/memory jobs remain required on the PR head |
| Isolated resources and cleanup | Fixture inventories, launcher exit and PID/PGID/path absence |

## Automated evidence

The clean baseline passed 23 protocol tests and the existing Ready socket/PTY
reconnect test. The new
`codex_unread_socket_acknowledges_only_expected_ready_and_survives_reconnect`
compiled and failed on the original implementation: expected unread turn `one`,
received JSON null. The fixture process group was cleaned after failure.

Development RED/GREEN attempts, commands, counts and exit statuses are listed in
the [implementation index](automated/implementation-evidence-index.md) and
[review correction index](automated/review-correction-evidence-index.md).
Failed attempts remain failed and are retained separately from later passes.

| Final local gate at `125c557` | Result |
| --- | --- |
| `cargo test --workspace --all-targets --all-features` | 708 passed, 0 failed, 14 existing intentional ignores, 25 binaries; [log](automated/final-workspace-02.log), [exit 0](automated/final-workspace-02.exit) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed; [log](automated/final-clippy-01.log), [exit 0](automated/final-clippy-01.exit) |
| `cargo fmt --all -- --check`, `git diff --check` | Both passed |
| `cargo test --workspace --doc` | Command passed; zero doctests executed; [log](automated/final-doc-tests-01.log) |

The 14 existing ignores are not claimed as passed. In particular, resource gates
run separately on hosted isolated workers. Current-head macOS, Linux, capacity
and memory results belong to the associated PR checks, not the local count.

## Independent review and retained failures

Independent exact-commit review found a P1 at `8ebbf0b`: the loop can receive B
and then drain queued `R` before drawing B, so reading current hierarchy state
acknowledged an unseen response. The retained behavioral
[RED](automated/review-red-presented-01.log) emitted B where A was expected.
Commit `125c557` records the selected observation only after a successful draw;
both production draw paths use it. Two passing regressions exercise the actual
message/input/writer boundary, later redraw, failed draw, overlay and selection.
Independent re-review and review of final native images/AX/JSON found no
remaining actionable findings.

The first [full workspace run](automated/final-workspace-01.log) failed.
The notification cancellation fixture still expected `✓ codex-queued`; the
intentional new row is `✓ ● codex-queued`. Its timeout poisoned the shared lock,
so the split-stream test failed before setup. The assertion was updated to retain
Ready, unread and terminal identity. Its focused rerun and the complete second
workspace run passed. No production notification behavior or test threshold was
changed. See [diagnosis](automated/regression-diagnosis-01.log).

## Native and platform evidence

The [initial native pass](native/attempt-01/review.md) covered real input,
Busy retention, exact/stale/duplicate CLI acknowledgement, terminal-mode `R`,
reconnect, reporter loss and narrow sidebar/split rendering. The
[final native pass](native/final/review.md) rebuilt exact `125c557`, verified
the corrected action and stale open-menu protection, and repeated narrow
Unavailable rendering and acknowledgement after exit. Both fixture launchers
exited 0; all recorded PIDs, process groups, roots and sockets were removed.

The deterministic child uses the real managed launcher, reporter, PTY and socket.
This validates product behavior in the native macOS GUI; it does not recertify
Codex or expand supported versions. Existing provider evidence remains in the
[Codex setup guide](../../docs/codex-reporting-setup.md).

Linux native GUI and new credentialed provider/capacity runs are outside this
bounded feature. Hosted automated Linux and capacity results will be reported
separately from macOS GUI evidence.

Known existing limitation: after unread is cleared, ordinary Busy/Unavailable
metadata can clip the last character of `unavailable` at width 80 behind PID and
elapsed text (`render.rs`, ordinary `provider_activity` metadata callers). This
does not affect the retained unread branch, which prioritizes both full labels.
A separate layout correction could prioritize ordinary health text too; no
unrelated rendering change was included here.
