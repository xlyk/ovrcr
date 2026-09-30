# Issue #216 — supplemental native Agent search acceptance

2026-09-30, macOS 26.5.2 (25F84), arm64. **The available macOS cases passed. Native Linux and exact 24×9 remain unverified; #216 stays open.** This evidence-only change makes no application, test, configuration-contract, or acceptance-policy changes.

Tested clean application source `248e2e5f48681ea90c7516e09b149772dd92ae4b` from an isolated worktree on `work/issue-216-acceptance-20260930`. Main already contained merged [#232](https://github.com/xlyk/ovrcr/pull/232); [#216](https://github.com/xlyk/ovrcr/issues/216) remained OPEN and followed #215 in creation order. The original checkout, index, CONTEXT and ADR edits were preserved. [Build evidence](build-evidence.json) identifies the actual bundled executables. The later evidence commit has identical application source.

## Native scope and fixture

Built and launched this checkout with `rtk proxy just gui` (build exit 0, 21.90 seconds). The parent allocated the native desktop slot; all GUI input, resize, screenshots and closing used Codex CUA on the task-owned bundle. No live user session, provider configuration, OS notification, permission, or security setting was controlled.

The helper owned `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-k0WFVh`, with `config.toml` and `server.sock` beneath it. These paths were first printed through native GUI terminal input. The initial twelve sessions were Terminal launches. Through the production CLI, a task-owned shell script named `codex` created 38 **synthetic Agent launches**, distributed across two existing fixture projects/workspaces, bringing the real Server to exactly **50 Running sessions**. The script disabled echo, printed `READY_synthetic-NN`, and acknowledged each submitted line as `RECEIVED_synthetic-NN_<line>`. No real Codex/provider reporting or paid run is claimed.

Two Agents had the identical Unicode title `審査 界🙂 é duplicate` (CJK, emoji, and combining acute); their IDs were #13 and #14. The others were `navigation-01` through `navigation-36`, ending at #50. See the [complete session/process inventory](fifty-inventory.json). Membership was based on the actual returned launch kind `{"agent":"codex"}`, rather than a provider-looking label on a Terminal launch.

## Observed outcomes

Every retained image is an unmodified CUA JPEG paired with full accessibility text. The pixels were inspected for glyph spacing, clipping, identity disambiguation, highlight and controls; the text was inspected for selected identity, mode and child-generated output.

| Check | Evidence and actual result |
| --- | --- |
| Unicode identity and filtering | [Screenshot](02-unicode-duplicates.jpg) · [AX](02-unicode-duplicates.ax.txt): pasted `審査` filters to both duplicate titles, with readable CJK/emoji/accent, distinct #13/#14 IDs and distinct project/workspace locations. |
| One Enter and correct duplicate destination | Down then Enter chooses #14; native typing produces a separate `RECEIVED_synthetic-01_UNICODE_DESTINATION_OK` line in Terminal mode. [Screenshot](03-unicode-destination.jpg) · [AX](03-unicode-destination.ax.txt). |
| Fifty-session navigation beyond visible results | From #14, Ctrl-g then lowercase s opens 37 eligible candidates (current #14 excluded). **36 native Down presses** reach `navigation-36 (#50)` beyond the first viewport. [Screenshot](04-fifty-last-selection.jpg) · [AX](04-fifty-last-selection.ax.txt). |
| Fifty-session destination typing | One Enter selects #50; native typing produces `RECEIVED_synthetic-37_FIFTY_NATIVE_OK`, with Terminal mode visible. [Screenshot](05-fifty-destination.jpg) · [AX](05-fifty-destination.ax.txt). |
| Compact Unicode and case-insensitive query | Native resize reaches **73×22**, confirmed with kernel `stty`, not inferred from a screenshot. Uppercase `DUPLICATE` matches both Unicode titles; selected ID, status/provider, location, navigation instructions and Open/Cancel remain readable. The long second workspace label clips at the border without overwriting it. [Screenshot](07-compact-unicode-duplicates.jpg) · [AX](07-compact-unicode-duplicates.ax.txt) · [kernel PTY evidence](compact-pty-size.txt). |
| Cancellation | Escape closes search, preserves #50 and its output, and leaves Browse. [Screenshot](08-compact-cancel.jpg) · [AX](08-compact-cancel.ax.txt). |
| Current Agent exclusion after filtering | From #50, query `navigation-36` shows `No matching agents`; the current Agent is absent. [Screenshot](09-current-excluded.jpg) · [AX](09-current-excluded.ax.txt). |
| Native shortcut delivery after Dashboard replacement | Detach exits the first owned Dashboard successfully. Restart through the helper, Browse s/filter/Enter select #50, and lowercase s plus Return in Terminal mode produces `RECEIVED_synthetic-37_s`; it does not open Agents. [Screenshot](10-compact-native-shortcut.jpg) · [AX](10-compact-native-shortcut.ax.txt). |

Read-only CLI verification of **all fifty children** then checked that IDs, runs, PIDs, phases and Unread values exactly matched the pre-input inventory. The Unicode marker occurred only in #14, and the fifty-session marker only in #50. None of the 38 synthetic children received an overlay query or an empty line from confirmation Enter. [Assertions and captured screens](native-routing-verification.json) · [result](native-routing-verification-final.log).

## Automated regression and cleanup

On the same application revision, `cargo test -p ovrcr --test terminal_acceptance agent_ -- --test-threads=1` executed **4 tests: 4 passed, 0 failed, 0 ignored, 10 filtered out**, in 39.77 seconds. [Full log](search-regressions.log). These existing regressions drive the compiled Dashboard, live Server, real outer PTY and session PTYs: attention ranking, fifty-session navigation, loading-input discard/no replay, and selection/typing. This is automated macOS evidence, separate from native screenshots. No full workspace, lint, Linux, capacity-job, or memory-job rerun is claimed by this supplemental evidence task; application source was unchanged.

Closed the owned GUI through CUA; launcher session exited **0** with no cleanup-error output. [Cleanup](cleanup.json) confirms the fixture root/socket are absent; `os.kill(..., 0)` raised `ProcessLookupError` for all fifty session process groups, both Dashboard groups, and GUI/Server/Dashboard PIDs. The GUI and Server shared the launcher process group, so cleanup checks their exact PIDs and does not signal that shared group. No app-specific CUA query was made after closing. The parent acknowledged release of the desktop slot.

The [attempt record](attempts.md) preserves two corrected evidence-driver assumptions and the environment denials. They did not require an application change or weakened test assertion. [Manifest](evidence-manifest.json) records retained evidence hashes.

## Remaining gates

- **Native Linux terminal/shortcut delivery: unverified.** The available CUA target is macOS. A Docker context or hosted/headless tests would not certify native Linux input/rendering. No Linux desktop/terminal harness was available to this task.
- **Native exact 24×9: unverified.** The actual GUI helper enforces a 480×320 minimum (`src/bin/ovrcr-gui.rs`); the smallest tested native terminal was 73×22. CUA denied access to both iTerm and Terminal for safety. Neither terminal was controlled, no alternate control channel was used, and the helper was not modified to evade the limit.
- This closes the missing **macOS Unicode and fifty-session native evidence**, and adds measured compact macOS evidence. It does not certify the excluded 24×9 case, Linux delivery, the entire parent acceptance matrix on this revision, clipboard delivery, genuine provider behavior, or conversation restoration. Prior #232 CI/review/native evidence remains separately scoped. No issue closure or merge is authorized by this report.
