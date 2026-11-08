# Ctrl-g sidebar labels: native GUI check

- Tested revision: `2d6b79bd` on `codex/ctrl-g-session-labels`.
- Checkout: `/private/tmp/ovrcr-ctrl-g-labels`.
- Date: 2026-11-07, macOS, disposable `just gui` fixture.
- Harness: the documented scripted System Events fallback in
  `docs/testing-computer-use.md` (`drive.py` + `winid.swift`, copied here). It
  sends real native keystrokes to the GUI process by PID, captures the window,
  and records the accessibility text beside it. Codex `cua` was not available in
  this shell.

Automated test results are separate and are recorded in the branch commit.

## Fixture

- GUI pid `38948`, window `8142`.
- Server pid `39335`, Dashboard pid `41427`.
- Fixture root `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-40V40R`.
- Twelve session process groups were recorded in `session-groups.txt` before
  close.

## Steps and observations

| Step | Input | Evidence | Result |
| --- | --- | --- | --- |
| Warmup | dismissed the one-time status-off dialog with `n`, Return | `01-initial.*`, `02-startup-typed-n.*` | The startup form only; `02` records the typed `n` before Return, not Browse. |
| Baseline | none | `03-browse.*` | Browse shows the six fixture Agent rows; footer `BROWSE  Space Menu  : Commands  ? Help`. |
| Open labels | Ctrl-g | `04-ctrl-g.*` | Labels `[1]`–`[6]` appear on Agent title rows after the Control key is released. Footer `1–9 jump  Ctrl-g/Esc cancel`. No row moved; status/title columns are unchanged. |
| Select | bare `2` | `05-selected-2.*` | Focus moved to `implement lifecycle cleanup`; pane header shows `claude:sonnet-4`; footer `Terminal mode …`. Labels are gone. No Control was held. |
| From Terminal | Ctrl-g | `06-ctrl-g-from-terminal.*` | Returns to Browse and re-labels the selected row as `▌[2]`; footer is the picker hint. |
| Toggle closed | Ctrl-g again | `07-ctrl-g-closed.*` | Labels disappear, selection stays on `implement lifecycle cleanup`, footer returns to `BROWSE  Space Menu  : Commands  ? Help`. |

The accessibility text and pixels agree: the labels are drawn in the existing
indent, the status glyph and title columns do not move, and the footer changes
from Browse to the picker hint and back.

## Screenshots / accessibility

- `03-browse.png` / `.txt`
- `04-ctrl-g.png` / `.txt`
- `05-selected-2.png` / `.txt`
- `06-ctrl-g-from-terminal.png` / `.txt`
- `07-ctrl-g-closed.png` / `.txt`
- Warmup: `01-initial.png`, `02-startup-typed-n.png` / `.txt`

## Cleanup

The window was closed with its AX close button. The launcher exited 0, the GUI,
server, Dashboard, and all twelve recorded session pids are gone, and the
fixture root was removed. See `gui-cleanup.txt`.

## Scope and gaps

This check covers Browse → labels → bare digit → Terminal, Terminal → Browse +
labels, and a second Ctrl-g closing the picker. Retained rows, scrolling,
unavailable targets, multi-pane selection, and legacy BEL delivery remain covered
by the automated unit/integration and compiled acceptance suites; they were not
repeated through this GUI fixture.
