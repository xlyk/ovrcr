# Initial native macOS pass

Date: 2026-09-12. Checkout: `codex/issue-61-unread`. The GUI was built from the
working implementation on `b7f139f`, before code commit `8ebbf0b`; subsequent
worker changes before that commit were formatting, wire fixtures and tests.
Binary hashes and launcher output are retained. This first pass is **not**
acceptance of the later correction to the displayed-observation race.

The unchanged issue60 deterministic Codex fixture ran through the real managed
launcher, synchronous report command, PTY and server socket. `ISSUE60` markers
identify that reused child. This is native product UI evidence, not new provider
certification. No live provider configuration, trust or credentials were used.

| Check | Observed result | Evidence |
| --- | --- | --- |
| Native terminal input | Separate `ISSUE61_INPUT_OK` output; both runtime paths belong to disposable root | `00-input.jpg` (initial AX file captured a diff only) |
| Selection/viewing | `✓ ● issue61-unread`, `Unread response ready · observed`; selection leaves unread | `04-selected-unread.*` |
| Next Busy | Spinner plus unread dot; `Unread busy observed` | `05-busy.json`, `06-busy-unread.*` |
| Explicit dashboard action | `Space t R` clears only unread; full snapshot otherwise unchanged, including Busy, agent, PID and phase | `07-mark-reviewed-menu.*`, `08-reviewed-during-busy.*`, `09-after-gui-ack.json` |
| Newer Ready vs old acknowledgement | CLI exit 1; full newer snapshot unchanged | `10-newer-ready.json`, `11-stale-cli-ack.json` |
| Terminal-mode R | Native input produced `ISSUE60_CALLBACK_5_OK UserPromptSubmit native-R`; previous unread retained | `12-terminal-R-is-input.*`, before snapshot in `13-cli-ack.json` |
| Exact/duplicate CLI acknowledgement | Both exit 0, only unread cleared | `13-cli-ack.json`, `14-cli-duplicate-ack.json` |
| Reconnect | Same provider PID/binding and complete unread observation | `15-ready-for-reconnect.json`, `16-reconnected-unread.*`, `17-after-reconnect.json` |
| Reporter exit | Historical Ready and unread retained with Unavailable health | `18-reporter-exit.json`, `19-reporter-unavailable.*` |
| Narrow rendering | Full `Unread Unavailable` in 27-column split metadata; activity/exit glyph plus unread dot in a 22-column sidebar | `20-narrow-unavailable-attempt01.*`, `21-narrow-split-unavailable.*`, `22-narrow-sidebar-unread.*` |
| Review after loss | Browse R clears unread; health, phase and agent observation unchanged | `23-reviewed-after-loss.*`, `24-after-loss-ack.json` |

Full AX and JPEG captures are paired except the explicitly noted initial input
capture. Text and pixels were inspected; narrow metadata ellipsizes later
activity text while retaining both complete unread and unavailable labels.

The initial socket inventory command using a canonical `/private/var/...` path
returned no lsof match. Reading Unix socket names and resolving their `/var/...`
aliases fixed inventory without changing the fixture. A later close attempt was
rejected because the window had changed; fresh AX state identified the current
close control, which closed the fixture normally.

Cleanup: launcher exit 0. `25-owned-inventory.json` and `26-cleanup.json` prove
all 15 recorded PIDs and 14 process groups absent, plus removal of the root and
server socket. Permission-denied results were not counted as absence.

Remaining at this checkpoint: independent review's draw/input race, exact-head
native recapture after correction, and full automated gates. Native Linux GUI,
credentialed Codex behavior and simultaneous provider load were not exercised.
