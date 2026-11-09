# Singleton Operator GUI computer-use acceptance — 2026-11-08

Run from the feat/operator-review worktree with
`OVRCR_PI_EXECUTABLE=<home>/.local/bin/pi rtk proxy just gui`.
The bundled GUI's `Demo` created its own disposable root; the root was identified
from the running dashboard client path and `server.sock.build.json` (never guessed).
Every screenshot is a fresh capture of the real GUI window; `.txt` beside each PNG is
Vision OCR of that exact capture (this model cannot view images), and
`artifacts/ax-window.txt` is the GUI's own accessibility text for the terminal at the
reconnect state. Input was real keyboard/mouse through the computer helpers; no
test-only render was used as proof. Text captures replace the absolute home-path
prefix with `<home>`. Three screenshots whose OCR showed those paths
(`00-launch.png`, `01-after-n.png`, `30-dashboard-detached.png`) were dropped from
this record for privacy; the remaining PNG screenshots are unmodified.

## Fixture 1 — singleton, tools, Quick, held stream, stop, reconnect

Root: ovrcr-gui-dB0LsN. Provider: `operator_pi_gui_provider` (synthetic loopback
provider seeded into that root's `operator-pi`), two instances because its scripted
turns are one-shot. Provider evidence: `requests-provider1.jsonl` (10 requests) and
`requests-provider2.jsonl` (2 requests).

Order exercised:
1. Launch and decline the startup agent-status offer (`n`, Enter); both screenshots
   were dropped for privacy and their OCR remains in `00-launch.txt` and `01-after-n.txt`.
2. Dashboard with the single sidebar row (`03-reopened.png`).
3. `o` opens the singleton directly: one `Operator` row, header `Operator · pi · ready`,
   composer, `Managing consigint / main / #1 run 1`; no New form/list/archive control (`04-operator-open.png`).
4. `REQUEST_ONE_REAL_TOOLS`: native write/read/edit/bash receipts and `TURN_ONE_DONE`;
   header becomes `Operator · pi · fixture/operator-fixture · finishing` then `complete`
   (`05-turn-one.png`, `06-turn-one-confirmed.png`). Disk marker
   `operator-target/gui-fixture-marker.txt` = `OFFLINE_MARKER_v2`.
5. Receipts expand from real keyboard input: `09-write-receipt-expanded.png` shows
   `write [call-write]` and the marker path/content; `07-bash-receipt-expanded.png`
   shows `bash [call-bash]` with the captured workspace and `"$OVRCR_EXECUTABLE" --json project list`.
6. `REQUEST_TWO_RESUME` → `TURN_TWO_DONE` (`10-turn-two-resume.png`). Provider request 5
   carries turn-one history (REQUEST_ONE_REAL_TOOLS, TURN_ONE_DONE, the write result).
7. `REQUEST_THREE_HELD` streams `HELD_BEFORE_RELEASE` while the provider holds the
   completion (`11-held-stream-before-release.png`); creating
   `operator-pi/fixture-release` completes the turn (`12-held-released-complete.png`).
8. `REQUEST_FOUR_BACKGROUND` starts the Bash `sleep 30` tool (`13-background-tool-running.png`).
   With that turn busy, `Ctrl-g` to Browse and `Ctrl-o` opens Quick with an unsent draft,
   header still running (`14-quick-draft-busy.png`). That Enter landed after the turn had
   completed and its stop was confirmed, so it was accepted as a normal turn (the journal
   entry appears only afterwards, `15-...png`); it does not represent queueing.
9. A second provider instance repeats `REQUEST_THREE_HELD` (fresh hold). Quick's Send while
   running is refused with `The Operator is busy or its worker stop is unconfirmed; draft
   retained.` and the draft stays (`17-quick-busy-send-refused.png`); Esc + `o` shows the
   same draft in the full composer (`18-draft-retained-after-dismiss.png`).
10. `Ctrl-x` stops that held turn. Both captures (taken just after the key and again a few
    seconds later) already show the confirmed header `interrupted` and
    `Interrupted · accepted operations may finish; no rollback.` with the draft kept
    (`19-interrupted-after-ctrl-x.png`, `20-interrupted-later.png`); the transient
    `stopping`/`interrupted · stop unconfirmed` text was not captured.
11. `Ctrl-c`, then `REQUEST_FIVE_AFTER_INTERRUPT` → `TURN_FIVE_DONE`
    (`21-turn-five-after-interrupt.png`). Provider request 2 for that turn contains the
    completed checkpoint history only: exactly one REQUEST_THREE_HELD/HELD_BEFORE_RELEASE
    pair (the interrupted turn is absent), proving the model-context rollback.
12. `Ctrl-g`, Enter, `printf 'CUA_GUI_OK\n'` in the Local shell shows independent output
    (`24-terminal-independent-output.png`), then `o` shows the same history and draft
    (`25-reopen-history-and-draft.png`).
13. Resize: the window was narrowed 1100 -> 575 points by dragging its right edge
    (`27-narrow-resized-retry.png`; the sidebar gives way to the full-width conversation),
    then restored to 1100 (`29-wide-after-restore.png`).
14. Detach with `q` (screenshot dropped for privacy; OCR in `30-dashboard-detached.txt`),
    click `Restart dashboard`, decline the
    startup offer again, observe `1 unread` in the sidebar/title (`31-dashboard-restarted.png`),
    then `o`: the same conversation and full history, unread cleared only after the tail was
    drawn (`32-reconnect-same-conversation.png`). Registry still had exactly one singleton
    row (`db-final.txt`).

## Fixture 2 — stopping a running Bash tool

Root: ovrcr-gui-USVhMB, provider instance 3 (`requests-provider3.jsonl`, 1 request).
Opened Operator (`r2-02-operator-open.png`), sent `REQUEST_FOUR_BACKGROUND`, observed
`Waiting for bash result` (`r2-03-bash-tool-running.png`), then pressed `Ctrl-x`. Both
subsequent captures (`r2-04-tool-stop-after-ctrl-x.png`, `r2-05-tool-stop-later.png`)
show the confirmed header `interrupted` (no `stop unconfirmed`) and
`Interrupted · accepted operations may finish; no rollback.`; the unconfirmed
`interrupted · stop unconfirmed` state was not captured. The turn row is Interrupted with
no checkpoint, the Bash adapter's background child (pid 5678 in
`operator-target/gui-background.pid`) was gone, and attempt 1 remained as evidence
(`db-r2-final.txt`).

## Cleanup receipts

Both windows were closed through the GUI's close control and both launchers exited 0.
Both fixture roots and their `server.sock` were removed; process groups 43699 and 86425
raise ProcessLookupError; all provider test processes exited 0; no GUI, server,
dashboard-client or pi worker process remained. See `process-ownership.txt`.

## Gaps and caveats

- The transient `interrupted · stop unconfirmed` / `Stopping` UI state was not captured for
  either stop; all four stop screenshots already show the confirmed `interrupted` header.
  The underlying `retained_sessions.stopped` row could not be queried after the fixture
  roots were removed; the certified UI state is the captured evidence.
- `Ctrl-c` did not clear a non-empty full-composer draft in this GUI session (tried twice;
  `22-draft-clearme.png`, `23-after-ctrl-c.png`). Ctrl-g/Ctrl-o/Ctrl-x were delivered and
  worked. The GUI deliberately drops repeated Ctrl+C to protect terminal interrupt
  identity, and synthetic presses can be classified as repeats; this is consistent with a
  harness artifact, but it was not proven against a physical keyboard and no code was changed.
- "Stop the held tool" was exercised as (a) stopping a held assistant stream and (b)
  stopping a running Bash tool with an ordinary background child (fixture 2).
- The model driving this run cannot view images; all screen-state claims above are from
  Vision OCR of fresh captures plus the GUI's AX terminal text, with the remaining PNGs
  retained for human review. AX text was dumped in full only for the reconnect state
  (`artifacts/ax-window.txt`); earlier states rely on OCR.
- No production code, tests, settings or provider overrides were changed for this run;
  the fixture's synthetic provider/auth came only from `operator_pi_gui_provider`.
