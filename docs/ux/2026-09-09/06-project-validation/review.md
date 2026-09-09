# UX pass 6: project validation recovery

Reviewed branch codex/ux-pass-06 in /Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes, starting clean at b96f955d581ab2a2c8e4563fa04e2b9177254f71. Corrected captures show the working diff committed with this report.

The task editor blocked saving an unmatched project correctly, but kept “Select an available project” visible after a valid project was accepted. This made successful recovery look unsuccessful. The event path now clears that specific validation message when leaving the project picker through acceptance. Other messages remain visible. Filtering, acceptance, selected project and server request behavior are unchanged.

| State | Screenshot | Accessibility |
| --- | --- | --- |
| 01-project-options | [JPEG](01-project-options.jpg) | [AX](01-project-options.txt) |
| 02-project-no-match | [JPEG](02-project-no-match.jpg) | [AX](02-project-no-match.txt) |
| 03-project-recovered | [JPEG](03-project-recovered.jpg) | [AX](03-project-recovered.txt) |
| 04-narrow-project-selection | [JPEG](04-narrow-project-selection.jpg) | [AX](04-narrow-project-selection.txt) |
| 05-after-invalid | [JPEG](05-after-invalid.jpg) | [AX](05-after-invalid.txt) |
| 06-after-accepted | [JPEG](06-after-accepted.jpg) | [AX](06-after-accepted.txt) |
| 07-after-narrow-accepted | [JPEG](07-after-narrow-accepted.jpg) | [AX](07-after-narrow-accepted.txt) |

Native review entered a git target, searched for a missing project, attempted save, cleared the filter, selected spacelift-agent, accepted with Tab, and resized. The corrected view shows the accepted project with no stale error in both normal and narrow windows. No task was saved or run.

Verification: the strengthened existing regression failed at the stale-error assertion before the fix, then all 23 task UI tests passed. It also proves a server error survives valid acceptance, invalid save emits no request, and a corrected save submits the original task identity and selected project. Full workspace passed 506 tests with six existing ignored cases; Clippy with warnings denied, formatting and the source/test diff whitespace check passed. Raw native AX files retain literal trailing spaces; an all-files committed diff whitespace check flags those captures. Full workspace and Clippy logs are retained in /private/tmp/ovrcr-ux-ten-passes/06, alongside RED/GREEN command logs and fixture evidence.

Both native launchers exited 0. All 13 recorded PIDs and 12 groups per fixture, root directories and sockets were absent after CUA closure. Native evidence is macOS only; provider execution and clipboard delivery were outside this slice. Incidental shell completion warnings were not changed. Next slice: long Unicode task input and multiline prompt navigation.
