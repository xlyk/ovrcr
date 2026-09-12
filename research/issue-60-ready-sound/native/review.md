# Native macOS acceptance

Tested code revision: `7861a97844f6760514522b191550bae836bd5179` in
`/Users/xlyk/Code/ovrcr` on 2026-09-11 (UTC 2026-09-12). The
[GUI launcher](01-gui-launch.log) built and ran that checkout and exited 0.
The which-key popup listing was re-checked at
`6169e3b` with a second launch ([log](23-gui-launch-popup.log)). GUI
input and captures used the Claude Code computer-use tool in background mode
(raw key input into the helper window, `screencapture -l` for files); the
public CLI drove the fixture terminal.

## Host

[Audio host record](00-audio-host.txt): the fixed sound is
`/System/Library/Sounds/Glass.aiff` (AIFF, 1.65 s); the default output device
was `MacBook Pro Speakers`, not muted, output volume 6 %. OVRCR changed no
audio setting.

## Observed behavior

Every turn was sent to the background fixture terminal
`consigint / auth-handoff / issue60-ready (#11)` (pid 43163,
[creation](03-managed-create.json)) through `ovrcr terminal send` while
`consigint / auth-handoff / local` stayed in the only visible pane. The
[observer](afplay-observer.py) read the process table every 20 ms; it never
spawned or replaced a host tool.

| Step | Channels | Turn | Host processes seen after `Stop` | Evidence |
| --- | --- | --- | --- | --- |
| `S` in browse mode | sound | – | footer `Ready sound: on` | [screenshot](04-ready-sound-on.jpg) |
| Background completion | sound | `native-a` | `afplay /System/Library/Sounds/Glass.aiff` pid 45871, own process group, parent dashboard 37784, started 0.08 s after Stop, ran 2.33 s | [record](05-sound-only-ready.json), [sidebar ✓](06-background-ready.jpg) |
| `N` in browse mode | both | – | footer `Desktop notifications: on` | [screenshot](07-both-on.jpg) |
| Background completion | both | `native-b` | `osascript … display notification … consigint / auth-handoff / issue60-ready (#11) OVRCR · response ready` pid 53391, then `afplay` pid 53405 (0.21 s after Stop, 2.31 s) | [record](08-both-ready.json) |
| Duplicate `Stop` | both | `native-b` again | none; activity revision stayed 4 | [record](09-duplicate-stop.json) |
| Fixture pane visible (`j` `j`) | both | `native-c` | none; Ready advanced to revision 6 | [screenshot](10-visible-pane.jpg), [record](11-visible-ready.json) |
| `k` `k` then `S` | desktop | – | footer `Ready sound: off` | [screenshot](12-sound-off.jpg) |
| Background completion | desktop | `native-d` | `osascript` pid 59543 only | [record](13-desktop-only-ready.json) |
| `N` | none | – | footer `Desktop notifications: off` | [screenshot](14-neither.jpg) |
| Background completion | none | `native-e` | none; revision 10 | [record](15-neither-ready.json) |
| `q` detach, completion while disconnected | (no dashboard) | `native-f` | none; dashboard 37784 absent; revision 12 | [record](16-disconnected-ready.json) |
| Restart dashboard with `ready_sound = true` in the disposable `dashboard.toml` | sound (from configuration) | `native-g` | exactly one `afplay` pid 66371, parent new dashboard 64466, 0.08 s after Stop, 2.34 s; no replay of `native-f`; which-key showed the toggle enabled | [record](17-post-reconnect-ready.json), [screenshot](18-post-reconnect.jpg) |
| `?` popup at `6169e3b` | – | – | popup lists `S Enable ready sound` beside `N`; pressing `S` inside the popup toggled the footer to `Ready sound: on` | [screenshot](24-popup-lists-ready-sound.jpg) |

The `osascript` arguments contained only project, workspace, terminal name and
id. The fixture's private prompt and response markers never appeared in any
host argument. The player received only the fixed file path.

## Cleanup

The fixture terminal received `exit`, printed `ISSUE60_NATIVE_FIXTURE_EXIT`,
and was closed through the CLI (`{"ok":true}`). Closing the window ended the
launcher with exit 0. [Cleanup record](22-cleanup.json): demo root and socket
absent; all 19 recorded PIDs and 16 process groups absent ([inventory](20-before-cleanup.json));
no `afplay` process remained. The popup re-check fixture was cleaned the same
way ([record](26-popup-cleanup.json)). The user's real OVRCR configuration,
socket and sessions were never used; `OVRCR_DASHBOARD_CONFIG` was unset, so the
settings file lived inside the disposable root.

## Limits

- **Audibility is not machine-verified.** The evidence is the real `afplay`
  process spawned by the dashboard, running for the file's duration through
  the default output device at the user's 6 % volume. No loopback capture
  exists on this host; a listener must confirm the chime.
- The computer-use tool withholds accessibility row text for this window, so
  screenshots are the visual evidence; the sidebar and footer text was also
  cross-checked through the dashboard's own screen state in the recorded JSON.
- Terminal-mode `S` as ordinary input, the failing-player notice and the
  five-second player deadline were not exercised natively; unit and real
  managed-path integration tests cover them (see the [acceptance record](../README.md)).
- Linux native playback was not attempted.
