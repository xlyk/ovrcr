# Operator activity native fix round 1 — 2026-10-30

**The previously missing pending-tool animation, collapsed-sidebar and completed
indicator-absent native endpoints are now captured.** This is a fresh owned run
after the human explicitly confirmed “Mac is unlocked”. The earlier
[native-01](../native-01/README.md) silent/narrow proof, lock interruption and forced
GUI wait -15 remain unchanged; this run does not rewrite that history.

## Reviewed source and unchanged artifacts

Product source: **`ace3077b0c6743648b96b6986c3cac9ce50e7a07`**.
Evidence head at launch: `39c1e6e79f0a87e0793e41f513fd082ae332294b`.
[Preparation](prepare.py) verified all six saved source hashes and an empty
`git diff --exit-code ace3077b -- crates tests`, then verified and reused the
exact frozen GUI, CLI and holder from native-01. [State](state.json) retains the
full source/binary hashes, feature set and new private paths. No Cargo command,
suite rerun, source change or target overwrite occurred for this round.

The fixture used the existing synthetic Grok OAuth and loopback provider only.
The new conversation model was `fixture-activity`. A single native submission
was made: `ACTIVITY hold the pending result for native proof`. A separate
`Retained draft 界🙂; do not send.` was pasted afterward. `Ctrl-l`, Left,
`Ctrl-l` collapsed Operator and returned to the composer before the held tool
was released. The actual alternate-screen Dashboard was verified before input;
recognized optional hooks were declined with n/Enter. No hooks/Bridge/startup
change or personal credentials were used.

## Actual pending-result animation

One initial model-gate release let the real compiled worker record its Bash call
and start the actual Bash child. Its result-file gate and final model gate
remained closed. [Before](pending-before-boundary.json) and
[after](pending-after-boundary.json) receipts have exactly equal turn/history and
control marker lists: Running sequence 2, three durable messages (User,
AssistantDelta, ToolCall), no ToolOutput. The pending Bash PID and owned process
group are retained in the paired process inventories.

| Capture | Exact CUA call | Start/end Unix ms | Actual AX glyph |
| --- | --- | --- | --- |
| [01 pending A](01-pending-collapsed-a.jpg) / [full AX](01-pending-collapsed-a.ax.txt.gz) | `await capture2('01-pending-collapsed-a')` | 1793381804924 / 1793381823567 | ⠧ Waiting for bash result |
| [02 pending B](02-pending-collapsed-b.jpg) / [full AX](02-pending-collapsed-b.ax.txt.gz) | `await capture2('02-pending-collapsed-b')` | 1793381826080 / 1793381834756 | ⠇ Waiting for bash result |

These were consecutive capture-only tool invocations. Each invokes
`app2.getAXStateAndScreenshot({disableDiffing:true,emit:false})`; the
[wrapper](capture-calls.js) records full AX, original JPEG hashes and call bounds.
No key, paste, resize, gate release or provider progress occurred between the
pair. Both saved pixel images were inspected: the spinner pattern changes beside
the same readable label, left target and composer/draft. Operator shows `▸ 1`
with its selected chat row absent while the project sidebar remains visible.
This closes the required collapsed-sidebar endpoint as well as actual idle
pending-tool movement.

All three JPEGs are 2200×1864 physical pixels at this run's native scale. No
resize was needed in this bounded correction: native-01 already supplies the
approved narrow hidden-sidebar endpoint. The unchanged automated buffer matrix
covers the other requested geometry and both themes; it is not newly executed or
claimed as native evidence here.

## Completion removes the indicator

After the pair and after-boundary receipt, the Bash result file and final model
marker were each released once ([release receipt](release-receipt.json)).
The [completed receipt](completed-before-boundary.json) records Complete sequence
5 and five durable messages, including actual succeeded Bash output and
ACTIVITY final. The follow-up draft is absent from those durable messages.

[03 completed](03-completed-collapsed.jpg) / [full AX](03-completed-collapsed.ax.txt.gz)
was captured with `await capture2('03-completed-collapsed')` at
**1793381852821–1793381852932**. The actual pixels and AX show the same collapsed
Operator and retained draft, header complete, succeeded tool output, ACTIVITY
final, empty right-hand context activity area, no Stop control, and Enter Send.
There is no Responding, Waiting label or activity spinner. No UI Enter was sent
after the original submission.

## Original waits and full owned cleanup

[Launcher](launch.py) ran in fresh private root
`/private/tmp/ovrcr-activity-native2-7w7dtpj5`; demo was
`/private/tmp/ovrcr-activity-native2-7w7dtpj5/temp/ovrcr-gui-VIyf3Z`.
Original owner tool sessions were holder **2957** (PID75603) and GUI **51116**
(PID76580). The original 600-second holder budget was unchanged and the run
finished within it.

After the completion receipt, the holder finish marker returned original
wait **0** ([holder receipt](holder-exit.json)). A fresh full AX identified
**73 close button**. `await app2.click(73)` returned successfully
([normal-close call](normal-close-call.json)), then the **original GUI owner wait
returned 0** ([GUI receipt](gui-exit.json)). No signal or CLI forced close was
used and no closed-app CUA query followed.

[Cleanup](cleanup.json) verifies all **20 recorded PIDs and 16 groups absent**,
the demo and socket already removed by normal GUI cleanup, and frozen GUI/CLI
hashes unchanged. Only after those checks was the remaining private holder root
removed. [Provider counts](provider-finished.json) are Activity2, A/B/Quick0.
Original owner log bytes and hashes are retained, including empty GUI output.
The reviewed bundle is still frozen in the checkout.

This correction does not add a local full-suite result, hosted final-head result,
Linux/native light-theme claim or live model-quality proof. Those remain separate
from this closed native acceptance gap.
