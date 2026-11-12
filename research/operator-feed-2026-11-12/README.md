# Operator operations feed: native GUI check, 2026-11-12

Source revision: `efa4d782` on `claude/operator-chat-tui-lockup-4cf9a1`.
Launched with `rtk proxy just gui` from that checkout, with a task-owned `HOME`,
the parent session's `CLAUDE_*` and `OVRCR_*` variables removed,
`OVRCR_PI_EXECUTABLE` set to installed Pi 0.87.1 and quota probes off
(`quota-off.toml`). The Operator's model was the loopback helper
`operator_pi_gui_provider` (`provider.log`). The fixture root was identified
from its `server.sock.build.json`, never guessed.

Input was real: keys through System Events with a focus check in the same call
(`drive.py`), and clicks and wheel events as CGEvents posted only while the
fixture was frontmost and owned the window under the pointer (`mouse.swift`).
Each `.jpg` is a capture of the real window; the `.txt` beside it is the
window's accessibility text at that moment. `00-launch` keeps its text only,
because the startup offer shows a home path. Text captures replace home and
scratch paths with `<home>` and `<scratch>`.

## What was exercised

| Step | Input | Seen | Capture |
| --- | --- | --- | --- |
| Open | `o` | Empty feed, composer focused | `02-operator-open` |
| First request | type `REQUEST_ONE_REAL_TOOLS`, Enter | One `✓` entry: target row, `› write`, dimmed `read`, `› edit`, the shell command, then the reply; the feed sits against the composer | `03-first-request` |
| Second request | type `REQUEST_TWO_RESUME`, Enter | The first request collapses to one row with its outcome; the new one is open | `04-second-request` |
| Reopen by keyboard | Tab, Home, Enter | The first request opens again | `05-first-reopened-by-keyboard` |
| Expand a command by keyboard | Down ×5, Enter | The whole shell command after `$`, then its output, highlighted as the selection | `06-command-expanded-by-keyboard` |
| Expand a command by mouse | click the `read` row | That command opens; the others are unchanged | `07-read-expanded-by-click` |
| Wheel | 40 wheel-down, 4 wheel-up, 60 wheel-up over a feed longer than the pane | The view moves to the tail, back to the top, and stays responsive | `09-wheel-down-to-tail`, `10-wheel-up-four`, `11-wheel-up-to-top` |
| Running | type `REQUEST_THREE_HELD`, Enter | `●` entry with the streamed text and the `Responding` row; the second request collapses | `12-running-held` |
| Stop | Ctrl-x | `■` entry with the interruption notice | `13-interrupted` |

The fixture's journal ended with three turns: Complete (10 rows), Complete (2
rows), Interrupted (3 rows) (`journal-summary.txt`).

## Found in this run and fixed afterwards

- A non-shell tool's expansion showed a lone `$` above its JSON arguments
  (`07-read-expanded-by-click`). It now shows the arguments as sent, without `$`.
- The footer said `Enter Expand details` while the selection was already open
  (`06-command-expanded-by-keyboard`). It now says `Enter Open or close`.

Both fixes have unit coverage or are a label change; neither was re-checked in
the GUI.

## Not covered here

- The scroll lock-up needs a long journal. This fixture's journal is 15 rows, so
  the wheel check shows that real wheel events scroll the feed, not that a
  5,000-row journal stays fast. That is measured in the commit message of
  `91da1ee7` and guarded by `scrolling_an_unchanged_transcript_reuses_wrapped_rows`.
- The helper produces no thinking, wake-ups, failed turns or folded lookups.
  Those are covered by unit tests and by `request_feed_through_live_server`.
- Narrow layouts (80×24, 60×16) were not resized to; the frame snapshots cover them.

## Cleanup

The provider helper and the GUI launcher both exited 0 (`provider.log`,
`gui-launcher.log`). Process groups 98462 (GUI and server) and 10416 (helper)
were absent afterwards (`gui-pgids.txt` lists them as recorded before shutdown),
and the fixture root was removed by the launcher. Two older `ovrcr-gui-*` roots
from other sessions were left untouched.
