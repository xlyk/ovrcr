# Pass 3: advertise applicable actions in empty task views

Iteration 3 of ten, based on `49f5ea16a2f7f7242bbe5928290c0e9874253e3f`, branch `codex/ux-pass-03`, checkout `/Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes`. After captures show the working diff committed with this report. [Previous pass](../02-concurrency-validation/review.md).

## Finding and correction

The empty task list offered Edit, Pause, Run, and Delete, while empty run history offered navigation, cancellation, and cleanup. The handlers have no target for these actions, so pressing them did nothing. In narrow windows, these unavailable actions also consumed room needed for applicable controls. This P3 discoverability issue was observed in three native states.

Empty tasks now show Back, New, All history, and Concurrency. Empty history shows Back to tasks. The renderer uses the existing selected task/run lookup; populated states retain their controls. No event routing or request semantics changed.

| State | Before | After |
|---|---|---|
| Empty tasks | [Image](01-before-empty-task-actions.jpg), [AX](01-before-empty-task-actions.txt) | [Image](04-after-empty-task-actions.jpg), [AX](04-after-empty-task-actions.txt) |
| Empty run history | [Image](02-before-empty-history-actions.jpg), [AX](02-before-empty-history-actions.txt) | [Image](05-after-empty-history-actions.jpg), [AX](05-after-empty-history-actions.txt) |
| Narrow empty list | [Image](03-before-narrow-empty-tasks.jpg), [AX](03-before-narrow-empty-tasks.txt) | [Image](06-after-narrow-empty-tasks.jpg), [AX](06-after-narrow-empty-tasks.txt) |

History opened and Escape returned to tasks. Empty-state messages were stable completed reads, not loading states. No tasks or runs were created in this pass.

## Verification

Behavioral RED/GREEN drives task events and rendering at 40/120 columns. It asserts applicable hints, absence of selection-only hints, no request from targetless actions, all-history request semantics, and restoration of full controls when task/run data is populated.

Workspace all-target/all-feature tests: 504 passed, 0 failed, 6 existing ignored across 21 binaries; all 21 task UI tests passed. Clippy with warnings denied, formatting, and diff checks passed. Logs and ownership/cleanup records: `/private/tmp/ovrcr-ux-ten-passes/03/`.

Both native launchers exited 0. Recorded fixture PIDs/process groups, roots, and sockets were absent after CUA close. Native evidence covers macOS empty-state navigation and layout; populated-state preservation is automated evidence. Linux rendering, clipboard delivery, and real-provider restoration were not exercised. The next pass will inspect task deletion confirmation with a selected task.
