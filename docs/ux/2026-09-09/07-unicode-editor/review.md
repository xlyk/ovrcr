# UX pass 7: long Unicode editor input

Reviewed clean branch codex/ux-pass-07 at b9003da8d398ceea5ee94ad3efba96264aff59db in /Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes. This commit adds review evidence only.

No application defect was demonstrated in this slice. At a narrow native window, long names and prompt lines kept the active cursor and END/TAIL suffix visible. Native paste preserved wide characters 界 and 猫 in multiline text. Up then End moved to the long middle prompt line; switching back to Name kept the prompt intact. Escape returned to an empty task list without creating a task.

| State | Screenshot | Accessibility |
| --- | --- | --- |
| 04-pasted-unicode | [JPEG](04-pasted-unicode.jpg) | [AX](04-pasted-unicode.txt) |
| 05-unicode-prompt-tail | [JPEG](05-unicode-prompt-tail.jpg) | [AX](05-unicode-prompt-tail.txt) |
| 06-pasted-unicode-name | [JPEG](06-pasted-unicode-name.jpg) | [AX](06-pasted-unicode-name.txt) |
| 07-cancelled-unsaved | [JPEG](07-cancelled-unsaved.jpg) | [AX](07-cancelled-unsaved.txt) |

The initial CUA typeText attempt omitted non-ASCII characters. Those three original attempt captures remain in /private/tmp/ovrcr-ux-ten-passes/07; they are not Unicode acceptance evidence. Native paste was used for the four linked states. The CUA spelling Esc was rejected before input; Escape succeeded. This is tool evidence, not an application keybinding defect.

Source inspection: TaskEditor::edit handles character boundaries and multiline navigation; visible_input measures display columns, and draw_tasks renders the cursor-aware slice. Existing task UI suite passed all 23 tests, including multiline Unicode mutation assertions and long-field cursor visibility. Command/revision/exit evidence: /private/tmp/ovrcr-ux-ten-passes/07/focused-1.log. No new regression or broad Rust rerun was needed for this report-only change. Markdown link checks pass. Raw AX whitespace is preserved.

The retained launcher exited 0. Cleanup verified all 13 owned PIDs, 12 groups, fixture root and socket absent (/private/tmp/ovrcr-ux-ten-passes/07/before-owned.json and before-cleanup.txt). No provider ran. This does not establish IME composition, grapheme-cluster editing, Linux appearance, or clipboard copy delivery. Next slice: dashboard search matching and empty-result recovery.
