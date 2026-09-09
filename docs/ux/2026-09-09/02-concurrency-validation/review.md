# Pass 2: show concurrency validation while editing

Iteration 2 of ten. Baseline `390ae2199f6ff1734b819c5d49ea88945a7cb174`, branch `codex/ux-pass-02`, checkout `/Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes`. After captures show the working diff committed with this report. [Previous pass](../01-picker-recovery/review.md).

## Finding and correction

Entering `0` and pressing Enter in the task concurrency editor appeared to do nothing. The error existed in task state but the input painted over the same status row. Escape closed the editor and finally exposed the explanation. This P2 validation-feedback defect made invalid submission look unresponsive.

The concurrency input now has its own row above status. Its footer shows cancel/save/delete controls rather than unrelated task actions. The error remains visible while the user edits. No validation, queue, acknowledgement, or persistence behavior changed.

| State | Screenshot | Accessibility |
|---|---|---|
| Before: input | [Image](01-before-concurrency.jpg) | [AX](01-before-concurrency.txt) |
| Before: rejected zero with hidden error | [Image](02-before-invalid-zero.jpg) | [AX](02-before-invalid-zero.txt) |
| Before: error appears after Escape | [Image](03-before-error-after-cancel.jpg) | [AX](03-before-error-after-cancel.txt) |
| After: error and input visible together | [Image](04-after-invalid-zero.jpg) | [AX](04-after-invalid-zero.txt) |
| After: narrow window | [Image](05-after-narrow-invalid.jpg) | [AX](05-after-narrow-invalid.txt) |
| After: corrected value saved | [Image](06-after-valid-save.jpg) | [AX](06-after-valid-save.txt) |

Correcting `0` to `5` showed Saved and updated the header to concurrency 5 in the disposable server. Reopening and cancelling preserved that value: [AX](07-after-cancel.txt). No jobs were executed.

## Verification

The regression failed behaviorally on the hidden error, then passed. It exercises blank and zero submissions at 40/80 columns, error/input/control visibility, no request on invalid input, successful request creation, acknowledgement before state change, and cancellation without mutation.

Workspace all-target/all-feature tests: 503 passed, 0 failed, 6 existing ignored cases across 21 binaries; all 20 task UI tests passed. Clippy with warnings denied, formatting and diff checks passed. Raw RED/GREEN and final logs are retained at `/private/tmp/ovrcr-ux-ten-passes/02/`.

Both native launchers exited 0. All recorded fixture PIDs, process groups, roots, and sockets were absent after closing through CUA. This is native macOS validation/recovery evidence, not Linux, clipboard, or real-provider acceptance.

The empty task list still advertises selection-only actions. The next pass will assess that footer against the empty list and empty history states.
