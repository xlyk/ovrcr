# OVRCR two-line sidebar acceptance — 2026-09-25

Partial acceptance: steps 01, 02, 03 and 07 passed; steps 04, 05 and 06 remain unverified. No feature failure was established.

## Window and evidence

Drove `OVRCR GUI` from `/Users/xlyk/Code/ovrcr-workspaces/0f4218e6ec3543471736db3cdf39d3af/target/OVRCR GUI.app`. Verified process **39390**, window **34457**, title **OVRCR GUI**, using the fixture containing **CONSIGINT → gui-consigint-auth-handoff → cua-two-line**. The selected session reports PID **88435**. No input was sent to the unrelated checkout.

Observed window bounds: **1100 × 932**, position **(478, 110)**, unchanged across the resize attempt. Executable: the app bundle's `Contents/MacOS/ovrcr-gui`. Process/path evidence: `08-window-identity.json`.

Evidence directory: `research/sidebar-two-line-2026-09-25/native/`. Every capture stem listed below has both **.jpg** and **.ax.txt** files. All 21 PNGs were validated (907 × 768). CUA screenshot bytes were JPEG and were re-encoded as PNG without cropping or annotation; full accessibility text was saved alongside them.

## 01 — Default width: PASS

`cua-two-line` occupies two lines. The full `claude:opus-4.5-long-context` label sits directly below the name, aligned with its start. `implement lif…` and `claude:sonnet-4` share one line, with the label at the right. Every visible `$ local` row uses one line. No overlap or incorrect clipping was visible.

Files: `01-default`, `01-initial`.

## 02 — Selection covers both lines: PASS, at the later sidebar width

The mauve bar and lighter background cover both the name and label lines of `cua-two-line`. The label retains its colour.

Between steps 01 and 02, the sidebar changed from approximately 40 to 30 columns without a successful CUA resize. The tool reported that the app had changed; the cause was not established. Consequently, the selected long label ends in an ellipsis. Early key attempts showed no selection change; subsequent keys worked after an accessibility Raise action.

Files: `02-selected`. Diagnostic captures: `02-before-keys`, `02-key-unresponsive`, `02-key-probe`, `02-after-click-error`, `02-after-raise`.

## 03 — Whole-session keyboard navigation: PASS

One `j` selects the next tree row, workspace `gui-consigint-worktree-lifecycle`, rather than the label line. One `k` selects `cua-two-line` again, highlighting both lines.

Files: `03-next`, `03-back`.

## 04 — Hover: UNVERIFIED

The documented CUA API exposes no pointer-only move operation. No hover was simulated with a click. The saved capture shows the current selection and label, but does not prove first-line-only `[x]` placement.

Files: `04-hover` (unverified-state capture).

## 05 — Click the label line: UNVERIFIED

Pressed `k` to select `review auth handoff`, then attempted a click in the middle of `cua-two-line`'s label. CUA returned `Computer Use server error -10005: noWindowsAvailable`. A plain session-name control click failed with the same error. Selection remained on `review auth handoff`; no confirmation dialog appeared. This does not establish a dashboard click defect.

Files: `05-before-click`, `05-click-label-line`, `05-name-click-control`.

## 06 — Narrow and restore: UNVERIFIED

The CUA resize drag failed with `noWindowsAvailable`. Native window bounds remained 1100 × 932; there was no successful resize to restore.

At the later, narrower sidebar width, both `implement lifecycle c… / claude:sonnet-4` and `build pipeline progre… / claude:opus-4` occupy two lines, while all visible `$ local` rows remain one line. That visual state is confirmed, but its transition was not caused by the requested controlled resize. Return to the initial inline layout was not demonstrated; `cua-two-line` remains two lines.

Files: `06-narrow`, `06-restored`. These names identify the requested checkpoints, not successful resize/restoration results.

## 07 — Terminal input and browse return: PASS

Selected `cua-two-line`, entered terminal mode with Return, and typed exactly:

~~~sh
printf 'CUA_%s\n' TWO_LINE_OK
~~~

After Return, a separate **CUA_TWO_LINE_OK** output line appears beneath the echoed command in both the screenshot and accessibility text. `Ctrl-g` successfully returns to **BROWSE** mode.

Files: `07-before-input`, `07-terminal-mode`, `07-input`, `07-browse`.

## 08 — Finish

Left the app open in browse mode with `cua-two-line` selected and the output marker visible. Did not close the window, quit the app, modify source, run cargo, or run git. Saved `codex-report.md` and the required work-diary entry.

Files: `08-final` (same final observation as `07-browse`), `08-window-identity.json`, `01-app-inventory.json`, `01-window-identity.txt` (initial sandbox inspection error).

## Outstanding checks

- **04:** Hover and first-line-only close marker — pointer-only movement unavailable.
- **05:** Label-line selection — coordinate clicks fail in the CUA tool, including a plain-name control.
- **06:** Controlled window narrowing and restoration — CUA drag fails; initial inline layout was not restored.

The native acceptance check is incomplete until these three checks are verified.
