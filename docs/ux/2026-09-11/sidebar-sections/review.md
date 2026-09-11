# Sections sidebar: acceptance evidence

Branch `sidebar-sections`, one commit on top of main `3574e92`, implementing
[issue #66](https://github.com/xlyk/ovrcr/issues/66). The design reference is
[`../sidebar-mockups/03-sections.html`](../sidebar-mockups/03-sections.html).
One departure from the mockup, requested during review of the native build:
the title band keeps its original mauve fill and icon rather than the quiet
underlined header.

## Automated checks (passed)

Run from this checkout on 2026-09-11:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features --no-fail-fast`: 25
  binaries, including the `tui` binary (180 rendered-buffer tests, 22 in the
  sidebar module), the `gui` binary against a real demo fixture, and the PTY
  acceptance binary that clicks sidebar rows by their visible text.

The sidebar module asserts the section headers and rules, the gap line, one-line
session rows, glyph colours per state, the muted glyph for an unavailable
reporter, soft selection on sessions and containers, folded counts, name
clipping and model dropping, gap-line hit-testing, and fifty-session scrolling.

## Native capture

The disposable GUI helper was built and launched from this checkout with
`just gui` (window, demo server, and ten shell sessions). Computer-use access to
`dev.ovrcr.gui` was granted in background mode. Captures are window
screenshots at native 2x resolution:

- [`01-initial-browse.png`](01-initial-browse.png): Browse mode on attach. The
  mauve title band, two upper-case project sections with rules and a blank line
  between them, workspaces with the branch glyph, `$ local` rows, one line per
  agent session with the muted `-` glyph (no hook report yet) and the model
  right-aligned in the provider colour. Long names clip with `…` before the
  model. The selected local shell shows the mauve bar and surface background.
- [`02-selected-agent-session.png`](02-selected-agent-session.png): after `j`,
  the selection is on `review auth handoff`. The bar and surface background
  move with it, and the row keeps its own glyph and grok colours. The pane and
  metadata line follow the selection.

Accessibility: the window exposes every screen row as a static-text element
(50 rows plus the terminal area), but the background computer-use tool
surfaces no text values for them, so row text is not recorded here; the
rendered-buffer tests carry the literal row text instead. Background clicks
resolve to accessibility selection on a row rather than a mouse press at a
column, so the fold hotspot was not exercised natively; the `tui` and PTY
acceptance tests cover it. Further keys after the first `j` landed in Terminal
mode, and the background tool cannot send `Ctrl-g`, so no more states were
captured.

## Fixture cleanup

The window was closed through its close button. The launcher exited with
status 0, the server's process group was gone, and the fixture directory
(`ovrcr-gui-q32KiI` under the user temp directory, socket `server.sock`) was
removed by the helper. An earlier run in this session that was stopped with
SIGTERM instead left its server running and its fixture behind; both were
cleaned by hand. Only the window-close path performs cleanup.

## Sidebar resizing

A second commit on the branch makes the sidebar's right border draggable
(20 columns to half the window; panes resize with it). It is covered by a
rendered-buffer test in the `tui` binary that presses the border, drags,
checks the drawn border column, the row layout at the new width, the pane's
desired size, the clamps, the release, and the resulting view request. No
native capture: the background computer-use tool resolves clicks to
accessibility actions and cannot deliver a mouse drag.

## Gaps

- Fold and clipped-selection states have automated coverage only.
- The signal-termination path of the GUI helper leaves its fixture directory
  behind. Not changed here.
