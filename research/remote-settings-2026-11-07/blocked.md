# Native CUA attempt: blocked

Tested: worktree `work/mobile-remote-web-mvp`, revision `4dc64676`, macOS.
Date: 2026-11-07. Tree clean at launch.

## Launcher

`rtk proxy just gui` built the GUI and launched it:

- `ovrcr-gui` PID 11168, window id 7797
- fixture `/var/folders/.../T/ovrcr-gui-AX752U`
- demo Server PID 11320 under the same bundle

## Blocked checks

1. System Events automation:

   `osascript -e 'tell application "System Events" to set frontmost of (first process whose unix id is 11168) to true'`

   failed, and the follow-up `osascript` probe hung for more than 60 s
   (a permission prompt was not answerable by the harness). `drive.py` stopped
   at its first `front()` call with `CalledProcessError`.

2. Window capture:

   `screencapture -x -o -l 7797 /tmp/ovrcr-cua/window.png`

   printed `could not create image from window` and wrote no file.

No screenshot or accessibility text could be produced. The terminal harness
process lacks Automation and Screen Recording permission; the guide requires a
blocked check in this case.

## Cleanup

The GUI process was terminated, its demo Server was stopped, and the fixture
root was removed. No `ovrcr-gui` or `dashboard-client` processes remain.

## How to complete the native pass

Grant the harness terminal (or the DeepSeek Harness app) Accessibility and
Screen Recording in System Settings → Privacy & Security, then rerun
`rtk proxy just gui` and the System Events harness. The steps are the same:
open Settings (`Space v ,`), filter `/remote`, toggle `remote.enabled`.
