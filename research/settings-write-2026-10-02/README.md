# Settings write path: native GUI evidence (item 2 PR A)

Branch `feature/settings-write`, macOS, `just gui`, 2026-10-02. The tested
binaries were built before one later change: a remembered launch choice's
`SetSetting` now goes after the new pane's `SetView` instead of before it, so
the synced write cannot delay the view that input waits on. This pass did not
launch from the palette; that path is covered by
`tests/gui.rs::palette_creates_switches_and_closes_a_real_terminal` against the
real Dashboard and Server, which failed with the old order ("Pane is loading;
retry input") and passes with the new one.

## Harness

Same as `research/settings-snapshot-2026-10-01`: real key events through System
Events to the `ovrcr-gui` process (`drive.py`), window screenshots with
`screencapture -l <window id>` (`winid.swift`), and the helper's accessibility
static text saved as `pN-*.txt`. `drive.py` now refuses to type unless the GUI
process is frontmost (see "Misdirected keystrokes"). CLI steps ran the bundle's
`ovrcr` with the fixture's `OVRCR_CONFIG` and `OVRCR_SOCKET`.

The fixture document started as `automatic_local_terminals = "on"`.

## Steps and outcomes

| Step | Input | Observed | Result |
| --- | --- | --- | --- |
| p1 | attach | Browse footer | PASS |
| p2 | `N` | footer `Desktop notifications: on`; document gains `desktop_notifications = true` (`p2-dashboard.toml.txt`) | PASS |
| p3 | `S` | footer `Ready sound: on`; `ready_sound = true` | PASS |
| p4 | `L` | footer `Automatic local terminals: off`; `automatic_local_terminals = "off"`, other lines kept | PASS |
| p5d | document `chmod 444`, `S` | footer `ERROR: InvalidRequest: could not save ready_sound: settings document … is read-only`; document byte-identical (`p5d-before/after.toml.txt`) | PASS |
| p6b | `ovrcr settings set ready_sound true` with the Dashboard attached | CLI prints `Set ready_sound through the Server`, exit 0 (`p6b-cli.txt`); document updated | PASS |
| p7 | `ovrcr settings set ready_sound loud` | `InvalidRequest: could not save ready_sound: "loud" is not a TOML value; expected a boolean`, exit 1 | PASS |
| p8 | Space, `v`, `,` | popup `Findings: 0`, `desktop_notifications = true (Dashboard, document)`, `ready_sound = true (Dashboard, document)` (the CLI's edit, republished), `automatic_local_terminals = off (Server, document)` | PASS |
| p9b | Escape | popup closed; the p5d error banner is still shown | NOTE |
| p10b | `S` | footer `Ready sound: off`, error gone; `ready_sound = false` | PASS |

p9b: the footer error belongs to the Dashboard's own refused request and
clears on its next successful one (p10b), not when another client saves. That
is the existing error-banner rule, unchanged here.

## Misdirected keystrokes

Two earlier attempts at p5 are not evidence and were deleted. In the first,
the Dashboard was already in Terminal mode when `S` was sent, so the letter
reached the fixture's own shell (cleared with Ctrl-U, nothing ran; why the mode
changed after p4 was not determined). In the second, the window was not focused
and the keystroke did not reach the helper. `drive.py` now checks focus before
every keystroke; every step above ran with that check.

## Cleanup

The window was closed through its close button; the `just gui` launcher exited
0 (`gui-launcher.log`), the fixture root no longer exists, and all twelve
recorded session process groups return `ProcessLookupError` (`gui-pgids.txt`,
`gui-cleanup.txt`).
