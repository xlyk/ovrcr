# Native macOS acceptance

Tested code revision: `422bc2d8a02042dfa5462920341944113d3eb7ee` in
`/Users/xlyk/Code/ovrcr/.worktrees/issue-59-desktop-alerts` on 2026-09-11.
The [GUI launcher](01-gui-launch.log) built and ran the exact checkout and exited 0.
Native UI interactions and captures used Codex CUA exclusively.

## Observed behavior

- Uppercase `N` in browse mode enabled notifications, then later disabled and
  reenabled them. See [enabled screenshot](02-enabled.jpg),
  [enabled accessibility text](02-enabled.ax.txt), and
  [disabled screenshot](15-disabled.jpg).
- The actual terminal produced separate `CUA_INPUT_OK` and `CUA_TERMINAL_N`
  output lines. The latter used an uppercase `N` keypress through CUA after
  pasting the rest of the command. It remained terminal input. See
  [input screenshot](03-terminal-input.jpg) and [AX](03-terminal-input.ax.txt).
- The terminal printed matching disposable `OVRCR_CONFIG` and `OVRCR_SOCKET`
  paths. `OVRCR_DASHBOARD_CONFIG` was unset in both the launcher environment
  and the GUI terminal. Settings changes stayed inside the disposable fixture.
- A new terminal, `consigint / auth-handoff / issue59-ready (#11)`, was created
  through the public CLI. It ran the reviewed `ovrcr agent run codex` with the
  deterministic [native child](codex-fixture.py), then invoked the real reporter
  using its inherited parent authentication. No credentials were read or copied.
  See [creation](04-managed-create.json) and [accepted Ready](06-background-ready.json).
- While `local` remained in the visible pane, `native-a` produced a real macOS
  notification: `OVRCR · response ready` and
  `consigint / auth-handoff / issue59-ready (#11)`.
  Notification Center ID: `C4963CAA-0AAB-4A31-B9D8-6A0F1D3957FD`.
  See [native AX excerpt](08-feature-notification.excerpt.txt) and the
  [background GUI screenshot](07-background-ready.jpg).
- After closing only that generated test notification, a duplicate `native-a`,
  visible-pane `native-b`, and disabled `native-c` produced no observed new
  notification. Reenabling produced no replay. The visible pane still showed
  ResponseReady and callback 5, so suppression did not acknowledge the response.
  See [visible screenshot](13-visible-ready.jpg), [AX](13-visible-ready.ax.txt),
  and [suppression observation](16-suppression-observation.json).
- The dashboard detached successfully while the managed child remained alive.
  A disconnected `native-d` completed. The fixture's previously absent
  `dashboard.toml` was created with `desktop_notifications = true`, then
  **Restart dashboard** reattached. No historical notification appeared.
  See [disconnected callbacks and enabled config](17-disconnected-ready.json)
  and [reattachment observation](18-reconnect-observation.json).
- A fresh background `native-e` after reattachment produced a new real macOS
  notification with ID `4C79B852-D606-489B-8A7C-D6FE7C882BBC` and the same
  identity-only title/body. This confirms delivery was enabled after reattach.
  See [callbacks](19-post-reconnect-ready.json) and
  [native AX excerpt](20-post-reconnect-notification.excerpt.txt).

The private prompt/response canaries did not appear in the observed notification
title or body. These native observations complement the deterministic integration
tests; Notification Center observations are not a substitute for exact subprocess
dispatch counts. The deterministic child proves the desktop route through real
PTYs, managed launcher, reporter, socket and dashboard. It does not recertify the
installed Codex CLI; PR #56 covers the unchanged actual Codex 0.153.0 semantics.
No Linux desktop delivery claim is made.

## Evidence and cleanup

All feature GUI captures are JPEG files with corresponding full AX text. The
managed terminal reported 126 columns by 57 rows. Full Notification Center
screenshots and AX also include unrelated user notifications, so they remain
only in `/private/tmp/ovrcr-issue59-native-evidence` with directory mode 0700 and
file mode 0600. Only task-specific excerpts are checked into this evidence set.
The [cleanup record](22-cleanup.json) lists hashes of the private captures.

The owned fixture root was
`/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-gAh1Q1`.
The [process inventory](05-owned-processes.json) records GUI PID 84707, server
PID 84806, both dashboard PIDs 85100 and 20455, eleven terminal sessions,
and deterministic child PID 1908. After CUA closed the native window, the launcher
exited 0, the root and socket were absent, all 17 recorded PIDs were absent,
and all 15 recorded process groups returned `ProcessLookupError`.
No closed-app CUA query was made. The generated feature notifications were closed
individually; no unrelated notification was cleared and no OS permission or
notification preference changed.

The host preflight was independently visible before the feature run; its
[observation](preflight-observation.json) records a 178-second Notification Center
selection despite a requested 20-second timeout. Subsequent native observations
were responsive. The feature run itself had no failed acceptance steps.
