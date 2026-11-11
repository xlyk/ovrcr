# Dashboard #448 native evidence

These screenshots and accessibility snapshots come from disposable native GUI instances. [The acceptance report](native-acceptance.md) identifies the source revisions, observed behavior, limits and cleanup. [Verification](verification.md) records the correction revision and local results; [review](review.md) records both review axes. Hosted status is available on [PR #464](https://github.com/xlyk/ovrcr/pull/464/checks).

| Path checked | Captures |
| --- | --- |
| Missing Pi stays Not set up after opening | [Screenshot](05-fixed-missing-pi-opened.png), [AX](05-fixed-missing-pi-opened.ax.txt) |
| Peach italic thinking, capped to six lines | [Screenshot](10-thinking-long-stream.png), [AX](10-thinking-long-stream.ax.txt) |
| Default applies next turn and survives Stop | [Pending](11-default-pending.png), [after Stop](12-stopped-pending-default.png), [next turn Low](13-next-turn-default-low.png) |
| Four mixed panes and fifth-split refusal | [Screenshot](14-four-panes-fifth-refused.png), [Panes group](47-final-panes-previous.png) |
| Real input in the focused pane | [Screenshot](17-focused-pane-input.png), [AX](17-focused-pane-input.ax.txt) |
| Split too small; compact Recent view | [Refusal](22-too-small-split-refused.png), [80-column Recent](23-recent-80x24.png) |
| Activity reorder, highlight and folded input count | [Recent](24-recent-input-highlight-and-sort.png), [folded project](25-folded-project-input-count.png) |
| Context creation menus and branch options | [Project menu](26-project-hover-menu.png), [Advanced Base](29-advanced-base-preview.png), [workspace menu](30-workspace-hover-menu.png) |
| Reply ready and independent thinking blocks | [Unread](32-reply-ready-unread.png), [independent expansion](36-thinking-block-expansion-independent.png) |
| Actual Off with stored Medium on a non-reasoning model | [Screenshot](37-nonreasoning-actual-vs-stored.png), [AX](37-nonreasoning-actual-vs-stored.ax.txt) |
| Temporary focused-only fallback preserves the layout | [Narrow](40-resize-focused-fallback.png), [restored](41-nested-layout-restored.png) |
| Reattachment keeps Recent and starts with one pane | [Screenshot](44-reattached-one-pane-recent-preserved.png), [AX](44-reattached-one-pane-recent-preserved.ax.txt) |

| Final Off → Idle and missing-Pi guard | [Off](45-final-operator-off.png), [Idle](46-final-operator-idle.png), [missing Pi](50-final-missing-pi-opened.png) |
| Stationary-pointer hover invalidation preserves selection | [Before](48-final-hover-before.png), [after row insertion](49-final-hover-after.png) |

Every PNG has a same-named `.ax.txt` snapshot. The missing-Pi capture uses `77ff9686`; captures 10–44 use `d10803a8`; captures 45–50 use final code `0ce4ca2c`. Native Terminal.app access was unavailable, and native GUI double-click/repeat delivery is not counted as passed.

Published AX text has trailing padding removed; the PNGs retain the exact rendered geometry.
