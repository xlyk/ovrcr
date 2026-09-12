# Implementation evidence

Base revision: b7f139f62b66eb10f73f22ffff51b8b074163fc3.
Worktree: /Users/xlyk/Code/ovrcr/.worktrees/issue-61-unread.
All non-baseline runs used the evolving uncommitted issue-61 implementation on that base. These are development gates, not final committed-revision certification.
All shell invocations used rtk proxy. Socket/PTY runs had escalated execution permission.
Original attempt logs are retained without replacement.

- `impl-baseline-protocol-01.log`: `rtk proxy cargo test -p ovrcr-protocol --lib`. Exit 0. 23 passed; clean base.
- `impl-baseline-runtime-01.log`: `rtk proxy cargo test -p ovrcr-runtime --lib response_ready_socket_snapshot_reconnect_and_revision_order -- --nocapture`. Exit 0. 1 passed; clean base; real PTY/socket.
- `impl-red-runtime-01.log`: `rtk proxy cargo test -p ovrcr-runtime --lib codex_unread_socket -- --nocapture`. Exit 101. 1 behavioral failure; accepted Ready lacks unread; no compiler error.
- `impl-green-runtime-01.log`: `rtk proxy cargo test -p ovrcr-runtime --lib codex_unread_socket -- --nocapture`. Exit 0. 1 passed.
- `impl-red-tui-01.log`: `rtk proxy cargo test -p ovrcr --test tui unread -- --nocapture`. Exit 101. 3 behavioral failures; no compiler error.
- `impl-green-tui-01.log`: `rtk proxy cargo test -p ovrcr --test tui unread -- --nocapture`. Exit 101. 2 passed; 1 fixture assertion selected the other pane after split hid unread pane.
- `impl-green-tui-02.log`: `rtk proxy cargo test -p ovrcr --test tui unread -- --nocapture`. Exit 0. 3 passed; split test explicitly focuses unread pane.
- `impl-red-eligibility-01.log`: `rtk proxy cargo test -p ovrcr-runtime --lib unread_requires -- --nocapture`. Exit 101. 1 behavioral failure: Confirmed incorrectly generated unread.
- `impl-eligibility-green-02.log`: `rtk proxy cargo test -p ovrcr-runtime --lib unread_requires -- --nocapture`. Exit 0. 1 passed; Codex, Observed, identified turn, Connected required.
- `impl-integration-01.log`: `rtk proxy cargo test -p ovrcr --test server_lifecycle codex_unread_real_cli -- --nocapture`. Exit 101. 1 failure at final narrow post-review wait: existing metadata clips unavailable to unavailab. Unread plus Unavailable had already passed..
- `impl-integration-02.log`: `rtk proxy cargo test -p ovrcr --test server_lifecycle codex_unread_real_cli -- --nocapture`. Exit 0. 1 passed; post-review snapshot health unchanged and full wide text checked; actual CLI, PTY input, callbacks.
- `impl-wire-golden-01.log`: `rtk proxy cargo test -p ovrcr-protocol wire_snapshot -- --nocapture`. Exit 101. 1 expected golden mismatch after version 7 and new non-empty fixture.
- `impl-protocol-green-02.log`: `rtk proxy cargo test -p ovrcr-protocol --lib`. Exit 0. 24 passed; regenerated golden.
- `impl-runtime-reporting-green-02.log`: `rtk proxy cargo test -p ovrcr-runtime --lib agent_reporting -- --nocapture`. Exit 0. 11 passed; real PTY/socket fixture cleanup assertions passed.

Final implementation checks: cargo fmt --all -- --check and git diff --check both exited 0.
Full workspace test/clippy/check, independent review, native GUI, diary, staging, commit and PR belong to coordinator. Worker did not run full workspace gates or commit.

Known pre-existing limitation: after unread is cleared, Busy + Unavailable at 80 columns uses ordinary pid/elapsed metadata and clips the final unavailable word. The unread branch prioritizes both complete labels at that width.
