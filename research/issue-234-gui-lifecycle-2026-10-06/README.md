# GUI lifecycle recheck, 2026-10-06

Recheck of the two failed gates from the 2026-10-05 Hermes native record: the
helper's close panic, and a Dashboard that stayed on the pre-pause header after
CLI pause, resume and exit.

Checkout: `fix/issue-234-gui-lifecycle` in `/Users/xlyk/Code/ovrcr-234`.
Quota collection was off for this demo. Startup hook prompts were declined, so
the user's Claude and Codex settings were not rewritten.

## Automated

- `close_request_leaves_the_dashboard_running_until_outside_the_frame` failed first by blocking inside the egui pass for 3.1s, then passed after shutdown moved to `on_exit`.
- `external_pause_resume_and_exit_redraw_the_live_dashboard` passed: a live Dashboard PTY showed `paused`, then its removal, then `closed`.

## Native helper

The disposable `ovrcr-gui` window showed the focused session header change after each CLI command:

| Step | Header |
| --- | --- |
| Running | `pid: 74420  elapsed: 0m  agent unknown  local (#1)` |
| `ovrcr pause 1` | `pid: 74420  elapsed: 0m  agent unknown  paused  local (#1)` |
| `ovrcr resume 1` | `pid: 74420  elapsed: 0m  agent unknown  local (#1)` |
| `ovrcr kill 1` | `pid: closed  elapsed: 0m  local (#1)` |

The window close button then returned launcher exit 0. The launcher log has no epaint panic. The demo directory was removed. Accessibility text is in `01-running.txt` through `04-killed.txt`.

This run used a demo shell session, not a new Hermes model turn. Interrupt forwarding, pause, refused paused input and native-group cleanup remain covered by `hermes_supervision_preserves_job_control_failure_and_peer_isolation`. Native Linux, other Hermes auth/model/profile setups, native hooks and recovery stay unverified.
