# Operator activity native check — partial, 2026-10-30

Product revision **`ace3077b0c6743648b96b6986c3cac9ce50e7a07`**. The fresh
matching CLI/GUI and synthetic provider holder were frozen before launch with
`gui,acceptance-diagnostics`; [state and source/binary hashes](state.json),
[GUI/CLI build](../checks/native-gui-cli-build.json), [holder build](holder-build-result.json),
[prepare](prepare.py) and [launcher](launch.py) retain provenance. No Cargo command
or target replacement ran while the native owners were alive. The bundle remains
frozen after cleanup.

## Proven visible outcome

The actual native GUI shows a narrow, hidden-sidebar journal with a readable
target, Responding label, composer rules and retained follow-up draft. The image
is 537×355 logical pixels; its ASCII context row is 81 cells wide. This satisfies
the requested narrow-context endpoint. The separate unit/public buffer tests
cover 140×40, 80×24, 60×24, 60×16 and both themes; they are not native captures.

| Capture | CUA start/end Unix ms | Actual AX glyph |
| --- | --- | --- |
| [01 silent A](01-silent-hidden-a.jpg) / [full AX](01-silent-hidden-a.ax.txt.gz) | 1793380889131 / 1793380889193 | ⠋ Responding |
| [02 silent B](02-silent-hidden-b.jpg) / [full AX](02-silent-hidden-b.ax.txt.gz) | 1793380890930 / 1793380891000 | ⠏ Responding |

The exact calls were consecutive `await capture('01-silent-hidden-a')` and
`await capture('02-silent-hidden-b')`; each calls only
`app.getAXStateAndScreenshot({disableDiffing:true,emit:false})` for observation.
[The executed capture wrapper](capture-calls.js) records full AX, JPEG hashes and
call timestamps. No key, paste, resize or provider release occurred between the
pair. Both actual pixel frames were inspected and show different spinner
patterns. [Silent authority](silent-boundary.json) has one Running turn at
sequence 0 with exactly one durable User message and no gate release marker.
Thus the pair proves actual idle movement before any assistant delta, with the
sidebar hidden; it does not rely on provider messages to trigger redraw.

The new-conversation form used private synthetic Grok OAuth and model
`fixture-activity`. One native submission was made: `ACTIVITY hold one actual
Bash result`. A separate `Hold this follow-up draft 界🙂; do not send.` was pasted
after acceptance. It remains visible in both frames and is absent from all five
final durable messages. The fixture has exactly two activity-model inference
requests and zero A/B/Quick requests ([provider counts](provider-finished.json)).
No personal credentials, paid inference, installed executable replacement or
shared live instance was used.

## Blocked native outcomes

After this pair, `await app.drag([534,352],[930,562]); await
app.getAXStateAndScreenshot()` returned a macOS lock error before any new capture
was returned. [The failed attempt](locked-attempt.json) is preserved. The
controller requested manual unlock; no reply arrived within the original
holder budget's cleanup margin. No CUA retry, alternate UI channel or timeout
extension was used.

**The pending-tool animated frame pair, collapsed-sidebar native endpoint and
completed indicator-absent native endpoint remain unverified.** The compiled
worker test separately proves those state transitions and final stop proof,
but it cannot close these native visual gates. The existing native Working
journal evidence remains historical and is not presented as this revision's
activity proof.

Optional recognized Codex hook installation was declined with n/Enter. A
transient startup Bridge-assets diagnostic was observed before the actual
alternate-screen Dashboard. No hooks, Bridge or startup correction was attempted.

## Original waits and owned cleanup

Owner launch sessions: holder **90634**, GUI **40988**. Child PIDs were 32199 and
32953. The original provider budget was 600 seconds, unchanged.

With about 167 seconds of that budget remaining, cleanup released the private
start gate once, recorded the [actual pending Bash boundary](pending-locked-boundary.json)
and its [owned processes](pending-locked-processes.json), then released the Bash
file and final-provider gate once. [Durable completion](completed-locked-boundary.json)
records Complete sequence 5 and five messages, including the successful actual
Bash output and ACTIVITY final. These post-lock database receipts are cleanup
evidence, **not visible native UI proof**.

The holder finish marker produced original wait **0** ([receipt](holder-exit.json)).
The frozen CLI then shut down only the recorded private server/socket with
`shutdown --kill`, exit **0** ([command/log receipt](cleanup-shutdown.json)).
Normal CUA window close remained unavailable, so only the exact verified GUI
PID received SIGTERM ([forced-stop receipt](forced-gui-stop.json)). Its original
child wait is **-15**, and original launcher tool exit is **241**
([receipt](gui-exit.json)); this is not a successful normal GUI close.

[Cleanup](cleanup.json) verifies all **19 recorded PIDs and 16 groups absent**
using process/group signal-0 checks, private socket absent, and unchanged frozen
GUI/CLI hashes. The demo remained after forced close and was removed only after
those absence checks. The entire recorded private root was then removed.
No closed-app CUA query was made. Complete original holder/GUI logs, byte counts
and hashes are retained, including the GUI's empty stdout/stderr. There are no
remaining native owners; the checkout's frozen bundle remains available for a
later unlocked acceptance run.
