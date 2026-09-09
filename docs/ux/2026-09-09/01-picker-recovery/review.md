# Pass 1: keep recovery controls visible after picker errors

Iteration 1 of the requested ten sequential UX passes. Reviewed baseline `52d6d0111de78da97653d9efd46ce45ed2a81bea`; implemented on `codex/ux-pass-01` in `/Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes`. The after captures show the working diff committed with this report.

## Finding and correction

Submitting an unmatched workspace or project leaves the form open with a useful error, but removes Escape and clear instructions. The shared palette status renderer selected the error instead of the form hints. This is a P3 recovery/discoverability issue, reproduced in three native states.

The palette now reserves a separate recovery row when an error is present and no mutation is pending. Editable forms show `Esc cancel · Ctrl-u clear`; other error states show `Esc cancel`. Errors remain in their own area. No request, validation, or cancellation semantics changed.

| State | Before | After |
|---|---|---|
| Workspace validation | [Image](02-before-workspace-error-correct-fixture.jpg), [AX](02-before-workspace-error-correct-fixture.txt) | [Image](05-after-workspace-error.jpg), [AX](05-after-workspace-error.txt) |
| Narrow window | [Image](03-before-narrow-error.jpg), [AX](03-before-narrow-error.txt) | [Image](06-after-narrow-error.jpg), [AX](06-after-narrow-error.txt) |
| Project validation | [Image](04-before-project-error.jpg), [AX](04-before-project-error.txt) | Shared renderer; both picker kinds covered by regression |
| Clear and select another workspace | — | [Confirmation image](07-after-recovery-confirmation.jpg), [AX](07-after-recovery-confirmation.txt) |

Ctrl-u cleared the invalid query; `scope-quality` selected the correct workspace. Escape cancelled its confirmation, with the workspace still visible. No removal was submitted.

## Verification

The regression drives the real dashboard event and renderer paths for both pickers at 120×40 and 40×12, asserts error and recovery visibility, and checks clearing/cancellation emits no removal request. The first attempt had an unqualified test type and did not compile; the corrected RED failed on the missing Escape hint. GREEN passed the one regression.

Workspace all-target/all-feature tests passed: 502 passed, 0 failed, 6 existing ignored cases across 21 binaries, including all 163 TUI integration tests. Clippy with warnings denied, formatting, and diff whitespace checks passed. Independent review follows this commit. Evidence directory: `/private/tmp/ovrcr-ux-ten-passes/01/` (distinct `red-1.log`, `red-2.log`, `green-1.log`, `workspace-1.log`, `clippy-1.log`, and native fixture records).

Both intended GUI launchers exited 0. All recorded PIDs/process groups and fixture roots/sockets were absent after close. A stale CUA capture closure reopened the old main-checkout app; its screenshot is excluded from this report. That extra owned fixture was recorded and cleaned up separately. Resetting CUA and passing the app explicitly to capture resolved the mismatch. The unrelated shell-completion warning in captures was left unchanged.

Native evidence covers macOS validation, recovery, and cancellation. It does not establish Linux rendering, native clipboard delivery, or provider restoration. The next pass will inspect task-list action availability with no selection.
