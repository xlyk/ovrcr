# Native UX pass: mouse navigation and History

Reviewed 2026-09-10 in `/Users/xlyk/Code/ovrcr`, branch `main`, revision `c4ee88e9906ca6aa7ee95dc6aad06ed7053b6dc1`. Tracked files were clean. Existing untracked `.DS_Store`, three reporting plans, and two research documents were preserved. This pass adds evidence only; no implementation, commit, push or PR.

The prior [split-pane review](../../2026-09-09/10-split-panes/review.md) tested `e6f194f1648e1568a4b0680e05e4726294b4cf20`. Current source includes mouse support merged afterward. This bounded follow-up reviewed four states in the actual checkout's disposable native GUI.

## Findings

No blocking defect demonstrated in the tested History flow. Clicking Actions opened search from terminal mode; typing history and clicking Open showed loaded History. Mouse dragging highlighted the complete ASCII/CJK/emoji line. Clicking Close returned to Browse; clicking the terminal restored Terminal mode and live input in the same PID 73345.

**P3: Search-result descriptions end mid-word.** Capture 02 shows the History description cut at `sessi`, with no ellipsis. The selected explanation is complete and wrapped below, so the action remains understandable. `crates/ovrcr-tui/src/dashboard/palette.rs:1649` renders search rows without wrapping. Smallest useful correction: ellipsize the one-line description to its available display-cell width. Acceptance: constrained-width descriptions end with an ellipsis while selected detail remains complete and mouse row targeting stays aligned. No fix made in this review.

## Evidence

| State | Screenshot | Full accessibility text |
| --- | --- | --- |
| Terminal output and Unicode | [JPEG](01-terminal-output.jpg) | [AX](01-terminal-output.txt) |
| Mouse-opened History search | [JPEG](02-history-search.jpg) | [AX](02-history-search.txt) |
| Mouse selection in loaded History | [JPEG](03-history-selection.jpg) | [AX](03-history-selection.txt) |
| Return to live shell | [JPEG](04-live-return.jpg) | [AX](04-live-return.txt) |

Both `UX_PASS_INPUT_OK` and `UX_PASS_RETURN_OK` appeared as separate output lines produced by printf format substitution. Pixels and AX were inspected. ASCII, CJK, and emoji remained visible in the selected line. The full selected explanation, Open, Cancel, Copy and Close controls were visible at the observed window size.

## Verification and limits

- Native: `rtk proxy just gui` built successfully; launcher session 75319 exited 0 after CUA closed the window.
- Cleanup: [ownership](ownership.json) records GUI PID 72495, server PID 72752, dashboard and ten shells. [Cleanup](cleanup.json) proves all 13 PIDs and 12 process groups absent with ProcessLookupError; disposable root and server socket absent. Unrelated live server was untouched.
- Automated Rust tests: not run for this review-only artifact. Build success is separate from the native observations.
- Unverified: clipboard delivery, frozen view under concurrent output, scrollback beyond one screen, narrow windows, pane resizing, IME, Linux, and real-provider restoration. Selection highlighting does not prove copying.
