# UX pass 9: compact help and live actions

Reviewed clean branch codex/ux-pass-09 at 072a1ecef397bed6e6e65f337d3941b09569aef4 in /Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes. This commit adds evidence only.

The compact lower-right help popup preserved surrounding dashboard context at normal and narrow window sizes. The Terminal submenu named local (#2) and its project/workspace. Pausing the disposable terminal changed its visible phase and replaced Focus/Pause with Resume; resuming restored Focus/Pause. Escape dismissed the popup. These observed states did not justify a code change.

| State | Screenshot | Accessibility |
| --- | --- | --- |
| 01-help-groups | [JPEG](01-help-groups.jpg) | [AX](01-help-groups.txt) |
| 02-terminal-actions | [JPEG](02-terminal-actions.jpg) | [AX](02-terminal-actions.txt) |
| 03-narrow-terminal-actions | [JPEG](03-narrow-terminal-actions.jpg) | [AX](03-narrow-terminal-actions.txt) |
| 04-paused-actions | [JPEG](04-paused-actions.jpg) | [AX](04-paused-actions.txt) |
| 05-resumed-actions | [JPEG](05-resumed-actions.jpg) | [AX](05-resumed-actions.txt) |

Source inspection followed whichkey_hints and whichkey_key/run_hint: relevant entries derive from current session state and reuse normal dashboard handlers. All 14 existing whichkey tests passed (149 other TUI tests filtered), including compact placement, tiny layouts, nested actions, phase updates, readiness and input capture. The CUA key name Backspace was rejected by the tool; native back-navigation is unverified in this pass, while the existing regression covers it. No new tests or broad Rust rerun were needed for an evidence-only change. Report links/Markdown whitespace pass; literal AX whitespace is retained.

The native launcher exited 0. All 13 recorded PIDs, 12 groups, root and socket were absent after closure. Raw command and ownership/cleanup logs are retained in /private/tmp/ovrcr-ux-ten-passes/09. Pausing/resuming was limited to the owned fixture; no provider or terminal command ran. Screenshots establish UI state, not an independent kernel signal audit. Linux rendering and mouse hit-testing were outside this slice. Next slice: split panes, real terminal output, and pane closure without session loss.
