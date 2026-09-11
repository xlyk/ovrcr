# Native Claude acceptance results

This run exercised the native macOS GUI and Claude Code through the supervised
OVRCR path. OVRCR product source stayed at `950710a` throughout the run. The
launcher began while repository `HEAD` was `ea7f235`; later changes before GUI
closure affected tests and documentation only. The split-label correction at
`ba7d560` belongs to the next GUI build and has no native result in this run.

The capture wrapper retained 69 allowlisted records in
`provider-signals.jsonl`. They contain event, turn, conversation, source, and
numeric status-line fields selected by `capture.py`. The saved set includes 11
`UserPromptSubmit`, 10 `Stop`, 7 notifications, 6 `PreToolUse`, 5
`PostToolUse`, one same-conversation `SessionStart(source=compact)`, two child
tool events, and the clear and exit boundaries. These counts describe captured
signals; they do not prove settled completion or complete accounting.

## Native cases

| Case | Saved evidence | Result and limit |
| --- | --- | --- |
| Initial input and first turn | `01-input.ax.txt`, `01-input.png`, `02-first-turn.ax.txt` | The native terminal accepted real input. The first completed turn reached `Idle / Observed`; the dashboard showed partial conversation tokens and estimated conversation cost. |
| Approval wait and allow | `03-approval-wait.json`, `03-approval-wait.ax.txt`, `03-approval-wait.png`, `04-approved-turn.json`, `04-two-turns.ax.txt`, `04-two-turns.png` | A matching permission notification produced `WaitingInput / Observed` for one turn. After approval and subsequent tool activity, the same binding remained connected and the turn later reached `Idle / Observed`. The snapshots do not call that state confirmed or settled. |
| Denial and later activity | `05-denied-turn.json`, `05-denial.ax.txt`, `06-continued-stop.json`, `06-continued-stop.ax.txt` | The denial observation remained `WaitingInput / Observed`. Later native activity advanced the observed turn and metrics on the same generation. Denial alone did not create a settled state. Automatic-approval behavior was observed only through the later provider events; no separate human-approval claim follows from it. |
| Blocking Stop continuation | `06-continued-stop.ax.txt`, `06-continued-stop.json`, allowlisted Stop/tool records | The terminal displayed the Stop-hook continuation and subsequent work. OVRCR retained observed activity and never upgraded it to `Confirmed`. The capture does not establish a general final-completion signal. |
| Same-conversation compaction | `07-compaction.png`, `07-compaction.ax.txt`, `07-compacted.json`, `provider-signals.jsonl` | `SessionStart(source=compact)` kept the same conversation, invocation, binding generation `1`, and partial usage totals. The native status line reported `current_usage` as explicit zero, so this capture proves a zero context value, not a null context value. The separate deterministic regression at `da6e4fc` covers null current usage clearing the old context while preserving cumulative usage and cost; native null timing remains open. |
| Child isolation | `08-child.ax.txt`, `08-child-finished.json`, two allowlisted child tool records | The wrapper captured child `PreToolUse` and `PostToolUse` identities. The root binding stayed on generation `1` and root activity remained independently attributable. No child `Stop` or child failure was captured, so those isolation cases remain open. |
| Input elicitation | `09-elicitation.ax.txt`, `09-elicitation-wait.json` | The actual unanswered input UI coincided with `WaitingInput / Observed` on its matching root turn. This establishes the captured case only; it does not turn silence into a wait signal. |
| Cancellation | `10-cancelled.ax.txt`, `10-cancelled.json` | After cancellation, a later attributable event left the root `Busy / Observed`. Cancellation did not fabricate confirmed idle or completion. The planned API-failure case was not run. |
| Transient source loss | `11-transient-source.json` | Removing the source first preserved the binding and metrics, then published `Unavailable(source_unavailable)`. Restoring the same path returned health to `Connected` without changing generation or decreasing the retained partial totals. |
| Cost comparison | `12-cost.ax.txt`, `12-cost-comparison.json` | OVRCR reported partial root-transcript totals of 710,359 input and 1,295 output tokens and status-line estimated cost of 4,493,476,000 USD ticks at the saved JSON observation. Claude `/cost` displayed $0.4607 at a different observation time and included broader model/category rows: Haiku and Sonnet input, output, cache read, cache write, and a stated subagent share. A later status-line observation reached 4,712,302,000 ticks. The sources, category coverage, and capture times differ, so the numbers are not an equality or completeness check. |
| Collector loss | `13-collector-loss.json` | Killing the owned collector published `Unavailable(collector_unavailable)` while `supervisor_alive` remained true. The binding and retained metrics stayed intact. |
| Pane close, detach, and reattach | `14-pane-closed.json`, `14-split.ax.txt`, `14-split.png`, `15-detached.json`, `15-reattached.ax.txt` | Closing the pane and detaching the dashboard preserved the native session, binding, generation, activity, and metrics. Reattachment restored the live terminal view. |
| Clear fallback | `16-clear-native.ax.txt`, `16-clear-native.png`, `16-cleared.json` | Native clear left the old generation frozen and published `Unavailable(identity_transition_unavailable)`. The saved old partial metrics remained visible; no new binding was inferred. |
| Exit and finalization | `17-exited.ax.txt`, `17-exited.png`, `17-exited.json` | Exit retained `Partial` conversation usage with 710,359 input and 1,295 output tokens, estimated conversation cost of 4,712,302,000 USD ticks, and `Unavailable(unfinalized_release)`. This is an explicit incomplete-finalization result, not settled completion or complete billing. |

## Split-layout finding

`14-split.png` and `14-split.ax.txt` exposed a rendering defect in the
`950710a` GUI: at the observed 62-column split, the final cost freshness word
was clipped from `stale` to `sta`. The reviewed `ba7d560` correction uses the
compact `est` label only when the full metrics row does not fit, preserving both
freshness words. This run predates that build. A final native GUI capture of the
corrected row is still required.

## Provenance and cleanup

`build-950710a.exit` records a successful build. The retained launcher command (tool session 84021) exited with status 0. `owned-processes.json` records the fixture's
process tree. `cleanup.json` confirms all 27 recorded PIDs and all 18 recorded
process groups were absent, and both the fixture directory and socket were
gone.

Screenshots and accessibility captures establish what the macOS GUI displayed.
The JSON snapshots establish the runtime state at named observations. The
allowlisted signal file establishes only the retained provider fields and their
capture order. This evidence does not cover Linux, the unrun API-failure case,
child Stop/failure, native null context after compaction, a settled-completion
signal, complete accounting, or the post-`ba7d560` split layout.
