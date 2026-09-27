# Sidebar two-line session rows — native evidence, 2026-09-25

Native macOS checks of the stacked sidebar label (PR #189, branch
`feature/sidebar-harness-model-by-width`) against the `just gui` fixture from
this checkout. Automated results live in the PR; this directory holds only what
was seen in the real app. Everything under `native/` was captured from
disposable fixtures whose roots, sockets and process groups were verified gone
afterwards (`09-cleanup.txt`, `p2-07-cleanup.txt`, `p4-03-cleanup.txt`).

The four passes track the design as it changed during review, so only the last
one shows the shipped layout:

| Pass | Revision | Layout under test | Driver |
| --- | --- | --- | --- |
| 1 (`0*`–`09-*`) | 9f98d42 | per-row stacking, no connector | Codex computer use |
| 2 (`p2-*`) | 47dba57 | per-row stacking, `└` under the glyph | Codex computer use |
| 3 (`p3-*`) | 1222eb5 | all-or-nothing stacking, `└` under the glyph | screencapture by window id |
| 4 (`p4-*`) | 7a85bd9 | **shipped**: all-or-nothing, `└` under the name | screencapture by window id |

## What the shipped layout looks like

- `p4-01-inline.jpg`: the fixture's own sessions. Every agent label fits beside
  its name at the default sidebar width, so every session is one line.
- `p4-02-stacked.jpg`: after creating `cua-stacked` with the label
  `claude / opus-4.5-long-context`. That label cannot fit, so every session
  with an agent label is two lines: the full name, then `└ agent:model` two
  cells in from the status glyph. `$ local` rows stay one line. Names that were
  clipped inline (`implement lif…`) are whole again.

`p3-01-inline.jpg` / `p3-02-stacked.jpg` show the same all-or-nothing switch one
revision earlier, with the connector under the glyph.

## What Codex computer use verified (passes 1 and 2)

From `native/codex-report.md` and `native/codex-report-pass2.md`, with the
per-row layout of the time:

- The stacked row draws the label on its own line, aligned and unclipped, and
  one-line rows and shells are unaffected (`01-default`, `p2-01-connector`).
- The selection bar and background cover both lines of a selected two-line
  session, and the label keeps its colours (`02-selected`, `p2-02-selected`).
- `j` from a two-line session lands on the next tree row, never on the label
  line, and `k` returns (`03-next`, `03-back`).
- Terminal input reaches a two-line session: `printf 'CUA_%s\n' TWO_LINE_OK`
  produced a separate `CUA_TWO_LINE_OK` line, and `Ctrl-g` returned to Browse
  (`07-input`, `07-browse`).
- The red close button closed the window and the launcher exited 0 both times.

## Not verified natively

- Hover `[x]` placement on the first line and selecting a session by clicking
  its label line. The Codex computer-use tool has no pointer-move operation and
  its coordinate clicks did not reach the dashboard (`noWindowsAvailable` in
  pass 1, silently ignored in pass 2). Both are covered by
  `narrow_session_row_keeps_its_model_on_a_second_line` in `tests/tui/sidebar.rs`.
- Reflow on a controlled window resize. Pass 2 resized the window to 700 px
  through System Events, but the helper scaled its content instead of
  re-gridding, so no rows changed layout; the sidebar-width switch is shown by
  the automated tests and by the sidebar drag that happened during pass 1
  (`02-selected` at ~30 columns).
- Pass 3 and 4 had no key or mouse driver: Codex's backend dropped every
  websocket and its HTTPS fallback returned 401, so those passes are
  screenshots only. Selection and navigation did not change between passes 2
  and 4; only row height assignment and the label line's indent did.

## Fixture notes

- `native/00-*`, `p2-00-*`, `p3-00-*`, `p4-00-*`: session lists, roots and
  process groups recorded before each pass, for the cleanup checks.
- A fixture launched between passes 2 and 3 died on launch: `scripts/gui.sh`
  copied fresh binaries over the bundle while the previous fixture's processes
  were still finishing cleanup, which invalidated the running binary's
  signature, and macOS then killed every exec of the bundle's `ovrcr` with
  SIGKILL (exit 137). Its orphaned GUI, server and shells were stopped and its
  root removed by hand. The script now removes the old binaries before copying.
- A screenshot of an occluded helper window returns its last frame, not the
  current one; bring the window to the front first.
