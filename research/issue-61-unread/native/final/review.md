# Final native macOS acceptance

Date: 2026-09-12. Built and launched with `rtk proxy just gui` from clean tracked
code `125c557a7e19b619a7e6e1077b61cc83f67ead0e`. Only untracked acceptance
artifacts were present. [Revision](revision.txt), [binary hashes](binaries.sha256)
and [launcher log](native-launch-final.log) identify this run.

CUA drove the actual native window and saved paired full accessibility text and
JPEG images. The unchanged [issue60 synthetic child](../../../issue-60-ready-sound/native/codex-fixture.py)
ran through the managed launcher, synchronous reporter, real PTY and private
server socket. No provider trust, credentials or live configuration changed.

| Check | Result | Evidence |
| --- | --- | --- |
| Actual input | Separate `ISSUE61_FINAL_INPUT_OK` output and disposable config/socket paths | `00-final-input-complete.*` |
| Drawn Ready acknowledgement | Select and draw A, then `Space t R`; unread clears while full agent/activity/PID/phase remain identical | `03-ready.json`, `04-drawn-ready.*`, `05-reviewed-after-draw.*`, `06-after-gui-ack.json` |
| New response while menu open | B is drawn, menu opens, then C arrives. Menu R returns `Conflict: Ready observation changed`; full newer C snapshot stays identical | `08-ready-b.json`, `09-menu-open-on-b.*`, `11-ready-c.json`, `12-stale-menu-preserves-c.*`, `13-after-stale-menu.json` |
| Reporter loss and narrow split | Exit retains unread; a 27-column pane shows full `Unread Unavailable` followed by ellipsized historical activity | `14-reporter-exit.json`, `15-final-narrow-unavailable.*` |
| Review after loss | Direct Browse R clears the dot and unread; Unavailable health remains visible | `16-final-reviewed-after-loss.*`, `17-after-loss-ack.json` |

The early `00-final-input.*` capture preceded the queued terminal input becoming
visible. The separately named completed capture contains the actual output;
the early frame is not treated as input proof. The deterministic message-arrival
before-key-before-draw race itself is covered by the real event-loop boundary
regression; native open-menu ordering provides an additional visible race case.

Independent review inspected the images, full AX and JSON and found no native
acceptance blocker. The [initial pass](../attempt-01/review.md) provides additional
Busy, reconnect, CLI duplicate and terminal-mode R evidence for unchanged paths.

Cleanup: launcher exit 0. [Owned inventory](25-owned-inventory.json) and
[cleanup](26-cleanup.json) verify all 15 recorded PIDs and 14 process groups
absent, plus the temporary root and server socket removed. The worktree and
retained evidence remain for review. Linux native UI and real Codex certification
are not claimed by this synthetic native test.
