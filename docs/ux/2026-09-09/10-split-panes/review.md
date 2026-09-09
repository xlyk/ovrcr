# UX pass 10: split panes and session continuity

Reviewed clean branch codex/ux-pass-10 at e6f194f1648e1568a4b0680e05e4726294b4cf20 in /Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes. This commit adds review evidence only.

The two panes kept distinct terminal output. The left shell emitted UX10_LEFT_OUTPUT and the right shell emitted UX10_RIGHT_OUTPUT as separate output lines, not merely echoed command text. The right shell reported stty size 57 63, then 35 44 after native window resize; both matched its displayed pane dimensions (columns x rows). Closing the right pane left its session in the tree. Reopening restored the same PID 39524 and prior output; a new UX10_SURVIVED output line demonstrated continued input/execution. No code correction was indicated by these states.

| State | Screenshot | Accessibility |
| --- | --- | --- |
| 01-left-output | [JPEG](01-left-output.jpg) | [AX](01-left-output.txt) |
| 02-split-created | [JPEG](02-split-created.jpg) | [AX](02-split-created.txt) |
| 03-both-outputs-size | [JPEG](03-both-outputs-size.jpg) | [AX](03-both-outputs-size.txt) |
| 04-resized-output-size | [JPEG](04-resized-output-size.jpg) | [AX](04-resized-output-size.txt) |
| 05-pane-closed | [JPEG](05-pane-closed.jpg) | [AX](05-pane-closed.txt) |
| 06-pane-reopened-output | [JPEG](06-pane-reopened-output.jpg) | [AX](06-pane-reopened-output.txt) |

Commands were entered through CUA in Terminal mode: rtk proxy printf with a format placeholder generated each marker, and rtk proxy stty size queried kernel PTY geometry. Source inspection followed split_pane, focus_pane, close_focused_pane and desired_view; pane removal changes the subscription/view state without issuing session termination. All 31 split-related TUI tests passed (132 others filtered), including geometry, per-pane input, matching acknowledgements, close/reopen and stale completion cases. Raw command/revision/exit evidence is /private/tmp/ovrcr-ux-ten-passes/10/focused-1.log. No broad rerun was needed for this report-only commit.

The CUA tool required one fresh state query after an external app-state change before accepting further actions; no input was sent until that query completed. The native launcher exited 0, and all 13 recorded PIDs, 12 groups, root and socket were absent after closure. Ownership and cleanup logs are retained in /private/tmp/ovrcr-ux-ten-passes/10. Report links and Markdown whitespace pass; literal AX whitespace is retained.

This establishes the observed macOS split flow, output and right-pane PTY resize. It does not establish Linux appearance, hidden-pane output under load, server-crash recovery, or clipboard delivery. No real provider ran. The ten requested iterations are catalogued in the adjacent index.
