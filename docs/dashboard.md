# Dashboard reference

Complete key, mouse, and rendering behaviour for the OVRCR dashboard. The
[README](../README.md#the-dashboard) covers the keys you need first; this page is
the full contract.

## Modes

The dashboard has four input modes. The footer names the current one and shows
a prioritized set of enabled shortcuts with action names. Hints that do not fit
remain available through `?` or `Space`. History shows `Esc Cancel copy` while
a copy is pending and `Esc Back` otherwise.

| Mode | Entered by | Leaves with |
| --- | --- | --- |
| Browse | the default; `Ctrl-g` from any other mode | `q` detaches the dashboard |
| Terminal | `Enter` on a running session | `Ctrl-g` |
| Copy | `[` in Browse | `Esc`, `q`, or `Ctrl-g` |
| History | `PageUp` in Browse, or wheel-up over a pane | `Esc`, `q`, or `Ctrl-g` |

Terminal mode is the only mode that sends input to the PTY, and `Ctrl-g` is the
only key it intercepts. There is no key popup in Terminal mode; press `Ctrl-g`
first.

## Browse keys

| Key | Action |
| --- | --- |
| `j` / `k` / Down / Up | Select the next or previous visible session |
| `Enter` | Send terminal input to the focused pane's session |
| `v` | Open a second pane showing the next different visible session |
| `Tab` / `Shift-Tab` | Focus the other pane |
| `x` | Close the focused pane; its session keeps running |
| `n` | Create a terminal (form) |
| `w` | Create a workspace (form) |
| `a` | Register a project (form) |
| `X` | Close the selected terminal; asks for confirmation |
| `p` / `r` | Pause or resume the selected session; no confirmation |
| `[` | Freeze the current screen for copying |
| `PageUp` | Open the session's retained history |
| `t` or `Ctrl-t` | Open scheduled tasks |
| `:` | Open the command palette |
| `Space` | Show contextual groups, then choose an action |
| `?` | Browse the same popup with arrows and `Enter` |
| `q` | Detach; the server and every session keep running |

Other `Ctrl`-modified keys are ignored in Browse. `Esc` and `Ctrl-g` cancel a
history request that has not opened yet.

## Copy keys

`[` freezes the focused session's current screen. Live output continues behind
the frozen copy; resizing the terminal cancels the selection.

| Key | Action |
| --- | --- |
| Arrows or `h` `j` `k` `l` | Move the cursor one cell |
| `Home` / `0` | Move to the row start |
| `End` / `$` | Move to the row end |
| `g` / `G` | Move to the first or last row of the captured screen |
| `v` | Set the selection anchor |
| `y` or `Enter` | Request a clipboard copy of the range; both endpoints included |
| `Esc`, `q`, `Ctrl-g` | Leave Copy mode |

## History keys

History opens as a frozen snapshot, so live PTY output does not move the rows
under review. The footer marks new live output while the snapshot is open.
`Enter` and paste do not send input while reading history.

Before an anchor is set, motion scrolls the viewport:

| Key | Action |
| --- | --- |
| `k` / `j` / Up / Down | One row |
| `PageUp` / `PageDown` | One viewport |
| `Home` / `End` | The oldest or the newest captured row |
| `h` / `l` / Left / Right | One column, for rows wider than the viewport |
| `v` | Anchor the cursor and start a selection |

After `v` anchors the cursor, the same keys extend the selection, and:

| Key | Action |
| --- | --- |
| `Home` / `0` | The start of the selected row |
| `End` / `$` | The end of the selected row |
| `g` / `G` | The first or last row of the snapshot |
| `y` | Load and copy the selected range across history pages |
| `Esc` | Cancel an in-progress copy and keep the selection; a second `Esc` leaves History |
| `q` or `Ctrl-g` | Leave History immediately |

Motion waits when a target cell still needs loading.

### History bounds

- The server keeps at most 512 physical history rows per session. New output
  evicts the oldest rows after that; it does not create a second retained copy
  for each finite burst.
- Eviction can leave a wrapped continuation after the beginning of its logical
  line is gone. Retained rows preserve their original physical widths.
- Exited session records keep their retained rows until the record is removed.
- A dashboard connection owns at most one frozen snapshot, and its client cache
  keeps at most sixteen bounded pages. History pages hold at most 16 rows by
  128 columns within the 128 KiB response bound.
- History rows keep the terminal geometry and wrapping captured when the
  snapshot was made. Resizing the live terminal changes the history viewport but
  does not reflow or rewrite captured rows; use horizontal navigation to reveal
  columns captured outside the viewport.
- The history view is capped at 64 rows by 256 columns inside a larger pane.
- Alternate-screen applications do not provide a history transcript, and History
  stays closed while an application is on the alternate screen.
- Retained history lives in memory with the session and is lost when the server
  exits. It is not persisted to disk.

### Clipboard

Clipboard requests use OSC52 and need permission from the host terminal,
including any intervening multiplexer. The limit is 65,536 UTF-8 bytes. The
notice `Clipboard request sent; paste to verify` confirms only that the request
was written, so verify it by pasting in the destination application. Terminals
that disable or lack OSC52 can ignore the request silently. Selections remain
available after clipboard or size errors. Soft-wrapped rows join without added
newlines; hard line breaks, explicit spaces, and stored Unicode text are
preserved.

## Panes

`v` opens a second pane with the next different visible session. Sidebar
navigation replaces the focused pane's session; selecting the session already
shown in the other pane moves focus there instead. `x` closes the focused pane
and expands the survivor, leaving its session running. If the window becomes too
narrow for both panes, only the focused pane is drawn; widening it restores both.
A new attachment starts with one pane. Each pane is capped at 1000 rows and
1000 columns; a `SetView` requesting a larger pane is rejected before any PTY
resize.

## Sidebar and status

Each sidebar session uses three lines: the session name, its label, and elapsed
runtime with context usage. The selected session is highlighted across all three
lines, and clicking any of them selects it.

| Slot | Meaning |
| --- | --- |
| `-` | No hook report accepted yet |
| blank | Idle, or exited (exited rows are dimmed) |
| braille spinner | Busy |
| `?` | Waiting for input |
| `!` | Reported error |

The selected metadata line shows `pid: closed` after the managed process exits.
Live metadata labels the same observation as `agent unknown`, `agent idle`,
`agent busy`, `agent waiting input`, or `agent error`. Paused sessions keep their
last label and add `paused`; exited sessions omit the live activity label.
Terminal output, elapsed silence, keyboard input, and process liveness do not
imply that an agent is busy or idle. Context usage shows `-` until a provider
reports it; see [agent reporting](agent-reporting.md).

## Mouse

In Browse mode:

- Clicking a session row selects it.
- Clicking a project or workspace row selects it and toggles its collapse state.
- Clicking inside a terminal focuses that pane.
- Wheel-up over a pane opens History at the captured tail. Further wheel ticks
  move one row; wheel-down at the newest row returns to the live pane.

In Terminal mode, mouse events reach the focused live pane only when that
application has requested a tracking mode (X10, 1000, 1002, or 1003). Enabling
SGR encoding alone forwards nothing. Coordinates are pane cells; clicks outside
the pane are ignored, not clamped. Held buttons are released on the previous
session before `Ctrl-g`, a selection change, or a resize. When the focused
application has not requested tracking, wheel-up over the pane opens History
instead.

## Key encoding

Modified keys are forwarded with xterm modifier parameters. `Shift+Enter` is
forwarded as the CSI-u sequence `ESC [ 13 ; 2 u`, so agents that support it
insert a newline instead of submitting. Terminals without the kitty keyboard
protocol report `Shift+Enter` as plain `Enter`, so the dashboard cannot tell them
apart there.

## Pause and resume

`p` pauses the selected live session and `r` resumes it. The server is
authoritative for the phase, so a paused row appears only after the server's
session refresh is applied. `Enter` on a paused row stays in Browse mode and
reports that `r` will resume it. Keyboard input, bracketed paste, and CLI
terminal sends are rejected while a session is paused; `Ctrl-g` still works in
Terminal mode so you can return to Browse and resume.

Pause records successful signal delivery to the original process group. It does
not synchronously confirm that every group member has stopped. Input admitted
before the pause may finish writing or execute after resume, and output buffered
before the pause may appear after it. Pause and resume are process lifecycle
controls: they do not declare an agent idle, cancel remote agent work, or replace
the hook-owned activity state.

## Key popup

In Browse, `Space` opens a group list: `t` Terminal, `w` Workspace, `p` Project,
and `v` View. Only groups relevant to the selected row appear. Terminal rows
also expose their parent workspace and project. `a` registers a project and
`q` detaches from the top level, including when the hierarchy is empty.

Choose a group, then an action: `Space t x` closes the selected terminal,
`Space w x` removes its workspace, and `Space p x` unregisters its project.
Workspace and project removal open a searchable picker with the current target
highlighted; Enter accepts it and opens confirmation. Terminal close opens
confirmation directly. Existing server removal safeguards
still apply; repositories and workspace branches are retained. `Space w n`
creates a terminal, `Space p n` creates a workspace, and `Space v t` opens tasks.
Terminal actions include Enter to focus, `p` to pause or `r` to resume, `c` for
Copy, and `h` for History. Pause/Resume follow the session's current state.
Bare hotkeys remain unchanged.

The compact popup sits above the footer in the bottom-right corner. Its title
shows the current prefix, and the submenu heading names the target. Backspace
returns to the group list; Escape dismisses it. `?` opens the same groups with
arrow navigation; Enter opens a group or runs the selected action. Enabled rows
are clickable. Scroll the wheel to reveal offscreen rows. Invalid or unavailable
keys leave the popup open. Relevant actions that are waiting for screen readiness
remain dimmed, with the reason in the detail area below the keys.

Copy and History retain their direct movement/selection menus, including
`Space v` to set an anchor. Terminal input mode has no popup.

Popup actions reuse the hint table's target descriptions and the existing action
handlers. The palette and direct-key footer retain their original shortcuts.


## Command palette

Terminal search rows show the session name and ID. The selected result shows
its project and workspace below the list. Long text ends with an ellipsis;
filtering still searches the full identity. Creation forms use their task name
as the window title.

`:` in Browse, or `Ctrl-g` then `:` from a terminal, opens the palette. If the
dashboard has detached it reconnects first. Type to filter actions or to search a
terminal by project, workspace, and name. Arrows select a result, `Enter`
continues, and `Esc` cancels.

Entries are `Create terminal (n)`, `Create workspace (w)`,
`Register project (a)`, `Close terminal`, `Remove workspace`, `Remove project`,
one `Switch terminal: <project> / <workspace> / <name> (#id)` per session, and
the remaining key-popup actions by name.

Within a form: on a pick list, Up/Down move the highlight and `Tab` or `Enter`
accepts. On a text field, `Tab` or the arrow keys move between fields, and
`Enter` advances and submits at the last field. `Ctrl-u` clears a field;
`Backspace` deletes its last character. Input stays in the palette while it is
open.

All existing-project and workspace fields use the same searchable picker,
including removal forms and the scheduled-task editor. Type to filter, use
Up/Down to choose, and Tab or Enter to accept. Workspace choices show
`project / workspace`; removal excludes the protected `root` workspace.
New names remain text fields, and project registration retains its path pickers.

Close and remove actions show their target and require confirmation. Server
safeguards still apply: a workspace must have no terminal records and a clean
worktree before removal. Errors stay in the palette with the form values
retained.

### Create terminal (`n`)

The Agent field lists coding agents found on `PATH` in this order: `claude`,
`codex`, `gemini`, `aider`, `opencode`, `pi`, `goose`, `amp`, `cursor-agent`.
`shell` (`$SHELL`, or `/bin/sh` when unset) is always last before `Custom`.
Workspace lists `project / workspace` pairs and defaults to the selected
session's. Name defaults to `local` when the agent is `shell` and that name is
free in the workspace; otherwise `<agent>-1`, `<agent>-2`, and so on. `Custom`
shows a Command field that runs through `/bin/sh -lc`. After the session starts,
typing goes to it immediately.

### Create workspace (`w`)

Project defaults to the selected terminal's project. Name has initial focus;
`Tab` and `Shift-Tab` reach the other fields. On Branch mode, `Space` or
Left/Right switches between `new` and `existing`. New branches start as
`feature/<name>` and follow Name until you edit Branch. Existing mode lists local
branches and hides Base.

Base defaults to the repository's `origin/HEAD` target, then `main`, `master`, or
the current HEAD. A remote-only default uses its full Git ref as Base. Git
suggestions load in the background, are cached per project until the palette
closes, and time out after two seconds. A footer note explains failures, after
which Branch and Base accept free text. `Enter` waits for pending suggestions
before submitting.

### Register project (`a`)

Repository and Workspace root are path pickers: the list is one directory of
children, directories only, filtered by fuzzy subsequence on the last path
segment. Hidden names stay hidden unless the typed segment starts with `.`. Git
checkouts sort first and are marked `git`. `Tab` accepts the highlight and
appends `/`; `Enter` keeps the typed text and moves on. An empty field lists
`picker_roots` from `dashboard.toml`. Name becomes the repository basename once a
path is chosen. Workspace root defaults to `<config dir>/workspaces/<name>` until
you edit it, and is created on registration if missing.

## Empty states

With no project registered, the dashboard shows `Welcome to OVRCR`, the `a`, `:`,
and `?` shortcuts, and the resolved config and socket paths. Clicking an empty
workspace shows how to start a terminal there and prefills that workspace in the
terminal form.

## Dashboard settings

Dashboard settings live in `dashboard.toml` beside `config.toml`. Override the
path with `OVRCR_DASHBOARD_CONFIG`. Do not put these keys in `config.toml`: the
server rewrites that file and drops unknown tables. A missing file uses defaults,
and a parse error shows in the footer and also uses defaults.

```toml
branch_prefix = "feature/"            # prefix for new workspace branches
picker_roots = ["~/Code", "~/src", "~"]

[[agents]]
name = "claude"
argv = ["claude", "--verbose"]
```

`picker_roots` defaults to `~/Code`, `~/src`, and `~`, keeping only the paths
that exist. An `[[agents]]` row whose name matches a detected agent replaces its
argv; a new name is inserted before `shell`.

## Output delivery

Dashboard output is delivered through a bounded queue, so a detached or slow
dashboard can reattach and refresh the current screen. Output backlog is
coalesced into a per-session refresh when necessary. Control responses and
lifecycle events are preserved, and the dashboard disconnects if the queue cannot
accept one of those messages.
