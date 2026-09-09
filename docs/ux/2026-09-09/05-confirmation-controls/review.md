# UX pass 5: confirmation controls

Reviewed `codex/ux-pass-05` in `/Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes`, starting clean at `8644f72059541fc231eb9595705187f377acd831`. After captures show the working diff committed with this report.

The deletion question identifies the selected task, but the footer advertised task commands while confirmation consumed those keys. The smallest correction replaces those hints with `n no`, `y yes`, and `Enter/Esc no` during both task deletion and run cleanup. Existing request and confirmation semantics remain intact. Whole-hint fitting keeps Yes/No visible at 24 columns.

| State | Screenshot | Accessibility |
| --- | --- | --- |
| 01-before-task | [JPEG](01-before-task.jpg) | [AX](01-before-task.txt) |
| 02-before-confirmation | [JPEG](02-before-confirmation.jpg) | [AX](02-before-confirmation.txt) |
| 03-before-narrow-confirmation | [JPEG](03-before-narrow-confirmation.jpg) | [AX](03-before-narrow-confirmation.txt) |
| 04-after-confirmation | [JPEG](04-after-confirmation.jpg) | [AX](04-after-confirmation.txt) |
| 05-after-narrow-confirmation | [JPEG](05-after-narrow-confirmation.jpg) | [AX](05-after-narrow-confirmation.txt) |
| 05b-after-narrow-confirmation | [JPEG](05b-after-narrow-confirmation.jpg) | [AX](05b-after-narrow-confirmation.txt) |
| 06-after-cancel | [JPEG](06-after-cancel.jpg) | [AX](06-after-cancel.txt) |

Native evidence: created one disposable future task, opened deletion, resized, answered n, and observed the task retained with normal controls restored. A CUA `Enter` key spelling was rejected by the tool, so this pass used n; Enter behavior is covered by the existing event regression. No provider or scheduled run executed.

Verification: focused regression failed before the fix because it saw `Esc back  n new  e edit`; all 23 task UI tests then passed. Workspace all-targets/all-features passed 506 tests, with six existing ignored cases. Clippy with warnings denied, formatting and diff checks passed. Raw command/exit/revision logs are retained in `/private/tmp/ovrcr-ux-ten-passes/05` (`red-1.log`, `green-1.log`, `workspace-1.log`, `clippy-1.log`).

Both native launchers exited 0. All 13 recorded PIDs and 12 groups per fixture, roots, and sockets were absent after CUA closure (`before-cleanup.txt`, `after-cleanup.txt`). Screenshots cover macOS, not Linux rendering or clipboard delivery. Very narrow question clipping is unchanged; the answer controls remain independently available. Next slice: task editor field navigation and validation recovery.
