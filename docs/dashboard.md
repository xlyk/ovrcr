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
only key it intercepts. Click the title bar's Actions control to open the palette
or Menu to open the action menu. Press `Ctrl-g` first to use dashboard shortcuts.

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
| `R` | Mark the selected terminal's displayed unread Ready observation reviewed |
| `[` | Freeze the current screen for copying |
| `PageUp` | Open the session's retained history |
| `t` or `Ctrl-t` | Open scheduled tasks |
| `:` | Open the command palette |
| `Space` | Show contextual groups, then choose an action |
| `?` | Browse the same popup with arrows and `Enter` |
| `q` | Detach; the server and every session keep running |

Other `Ctrl`-modified keys are ignored in Browse. `Esc` and `Ctrl-g` cancel a
history request that has not opened yet.

## Unread responses

A managed Codex, Pi or Oh My Pi terminal keeps one unread indicator for its latest unreviewed
root Ready response. The indicator is separate from its activity glyph: a new
Busy report does not clear an earlier unread response. Selecting a terminal,
viewing its output or receiving a notification does not acknowledge it.

Press `R` in Browse mode, or choose **Mark reviewed** in the terminal action
menu or command palette. The action names the unread observation displayed by
the dashboard. If a newer Ready arrives first, the server rejects the stale
acknowledgement and preserves the newer unread response. Refresh your view and
review that response before acknowledging it. Repeating a completed
acknowledgement is safe.

For scripts, read `unread` from the intended terminal's JSON row and pass that
complete JSON object to the CLI:

```sh
ovrcr terminal list --json
ovrcr terminal mark-reviewed 11 --expected "$observation_json" --json
```

Set `observation_json` to the exact non-null `unread` object you reviewed; it
contains the binding, turn and activity revision. Do not replace it with a newly
fetched identity merely to retry a stale request. Mark-reviewed changes only
unread state, preserving activity, quality, health and native provider behavior.

Unread state survives dashboard detach and reconnect while the server lives.
Reporter loss retains the historical unread response alongside Unavailable
health. The server stores at most one observation per terminal; terminal removal
or server death discards it. No response queue or durable review history is kept.
Unread tracking is independent of notification preferences and needs no extra
hook configuration.

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

Each project is a section: its name in upper case with a rule to the sidebar
edge, and a blank line before every project after the first. Workspaces follow
with a branch glyph and a bold name. Each session takes one line: a status
glyph, the session name, and the model from its label right-aligned in the
provider colour. A long name clips with `…` before the model, and the model is
dropped when fewer than twelve cells would remain for the name. Local shells
show `$ local` with no model unless a hook reports activity inside them. The
selected row shows a mauve bar in its first column and a lighter background; its
own colours stay visible. A folded project shows `▸ N ws` and a folded workspace
`▸ N` at the right edge.

| Glyph | Meaning |
| --- | --- |
| `-` | No hook report accepted yet |
| blank | Idle |
| braille spinner (green) | Busy |
| `?` (yellow) | Waiting for input |
| `!` (red) | Reported error |
| `✓` (teal) | Response ready |
| `P` | Paused |
| `·` | Exited (the row is dimmed) |

An unavailable reporter draws the glyph in the muted colour. The literal state,
quality, and reporter health (`agent busy confirmed`, `response ready ·
observed`, `unavailable`) appear on the selected session's metadata line, not
in the sidebar.

The selected metadata line shows `pid: closed` after the managed process exits.
Live metadata labels the same observation as `agent unknown`, `agent idle`,
`agent busy`, `agent waiting input`, or `agent error`. Paused sessions keep their
last label and add `paused`; exited sessions omit the live activity label.
Terminal output, elapsed silence, keyboard input, and process liveness do not
imply that an agent is busy or idle. See [agent reporting](agent-reporting.md)
for provider reporting behavior.

## Mouse

In Browse and Terminal modes:

- Clicking a session row selects it.
- Clicking a project or workspace name selects it. Clicking the first cell of a
  project header, or the branch glyph of a workspace, folds or unfolds it. The
  blank line before a project does nothing.
- Scrolling over the sidebar scrolls its visible rows.
- Clicking inside a terminal focuses that pane for typing. A click that changes
  focus is consumed by the dashboard.
- Dragging the divider between two panes changes their widths. Each visible pane
  keeps at least 20 columns. The split proportion survives window resizing and
  the temporary single-pane layout used in narrow windows.
- Dragging the sidebar's right border changes the sidebar width, between 20
  columns and half the window; the panes take the remaining width and are
  resized. The chosen width lasts for the attachment and is not saved.
- Wheel-up over a pane opens History at the captured tail. Further wheel ticks
  move one row; wheel-down at the newest row returns to the live pane.

In Terminal mode, mouse events reach the focused live pane only when that
application has requested a tracking mode (X10, 1000, 1002, or 1003). Enabling
SGR encoding alone forwards nothing. Coordinates are pane cells; dashboard
controls outside the pane handle their own clicks. Held buttons are released on
the previous session before `Ctrl-g`, a selection change, or a resize. When the focused
application has not requested tracking, wheel-up over the pane opens History
instead.

In Copy and History, drag across the painted terminal cells to select text.
History's wheel scrolling moves through the frozen capture. The title bar's
Copy control copies the selection, and Close returns to the dashboard. A new
selection replaces the previous one and cancels an unfinished History copy.
Pane metadata, status rows, and controls are outside the selectable text.

Tasks supports clicking rows, fields, picker options, and footer actions.
Scroll over the task list, run list, or transcript to move that view. Footer
actions wrap in short or narrow layouts; save, cancel, and destructive
confirmation use the same actions as the keyboard. See
[scheduled tasks](scheduled-tasks.md#manage-tasks-in-the-tui).

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
`Space v` to set an anchor. The title bar's Menu control also opens the menu
from Terminal mode. Popup Back and Close controls support mouse navigation.

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
accepts. On a text field, `Tab`, Up, and Down move between fields, and
`Enter` advances and submits at the last field. Left/Right and Home/End move
the text cursor; Backspace and Delete remove text beside it. `Ctrl-u` clears
a field. Click a field to focus it and position its cursor, click options or
toggles to choose them, and use the wheel to scroll lists and long forms.
Visible Submit and Cancel controls work from any field. Input stays in the
palette while it is open.

Click a search result to open it, or select it and click Open. Confirmations
have separate Confirm and Cancel controls; clicking outside an open palette
does not activate the terminal behind it.

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
desktop_notifications = false        # opt in to background Codex Ready alerts
ready_sound = false                  # opt in to a sound for the same responses
branch_prefix = "feature/"            # prefix for new workspace branches
picker_roots = ["~/Code", "~/src", "~"]

[[agents]]
name = "claude"
argv = ["claude", "--verbose"]
```

`picker_roots` defaults to `~/Code`, `~/src`, and `~`, keeping only the paths
that exist. An `[[agents]]` row whose name matches a detected agent replaces its
argv; a new name is inserted before `shell`.

## Desktop notifications

Desktop notifications are off by default. Set `desktop_notifications = true` in
`dashboard.toml` to enable them when attaching. In browse mode, press uppercase
`N`, or search the command palette for **desktop notifications**, to enable or
disable them for the current dashboard. The toggle does not rewrite your settings
file. In terminal mode `N` remains ordinary terminal input; use Ctrl-g first.

An alert means a managed root response from a supported readiness provider is ready to review, not that its task
succeeded. Delivery keys on a new [unread](#unread-responses) identity (binding and
turn), not on activity quality. Confirmed activity without unread does not notify.
Only project, workspace and terminal identity appear in the alert. Prompt and
response text are never included. The accepted Codex reporting setup is still
required; see [Codex setup](codex-reporting-setup.md).

The active dashboard delivers alerts only when the terminal is absent from every
visible pane. A terminal assigned to a split hidden by a small window is eligible;
Tasks shows no terminal panes. Visibility does not acknowledge or change Ready.
Attachment and reconnect establish a baseline: old unread observations, including
responses completed while disconnected, are not replayed. Enabling alerts or
hiding a terminal does not replay a previously suppressed response. A later Busy
or reporter loss cancels a pending or in-flight alert. Mark-reviewed does not.
No active dashboard means no delivery.

Host submission runs outside the input loop with bounded queues and a two-second
subprocess deadline. Disabling the last enabled alert channel (notifications or the
[ready sound](#ready-sound)) or detaching cancels pending work.
Delivery is best effort, with no retries. Host failures do not change reporting,
native approvals, input or process state. A host command error displays
**Desktop notifications unavailable** in the footer.

macOS uses `osascript`; the system chooses its sender identity and notification
preferences. Linux uses `notify-send` from the desktop session. OVRCR requests no
notification sound; the [ready sound](#ready-sound) is a separate option. Neither
a successful host command nor an unchanged footer proves that the
desktop displayed the alert: OS permission, Focus/Do Not Disturb and desktop
policy can suppress it. OVRCR does not change those settings. Native acceptance
and platform limits are recorded in the [issue 59 evidence](../research/issue-59-desktop-alerts/README.md).
Actual desktop delivery is verified on macOS. Linux has automated coverage; native
Linux desktop delivery remains unverified.

## Ready sound

The ready sound is off by default and independent of desktop notifications:
either, both or neither can be on. Set `ready_sound = true` in `dashboard.toml`
to enable it when attaching. In browse mode, press uppercase `S`, or search the
command palette for **ready sound**, to toggle it for the current dashboard
without rewriting your settings file. In terminal mode `S` remains ordinary
terminal input; use Ctrl-g first.

A sound follows exactly the same selection as a desktop alert: a new unread
identity for one accepted managed root response outside every visible pane,
with the same deduplication, visibility suppression and attach/reconnect baseline.
Confirmed activity without unread does not play. When both channels are on, one
response produces one alert and one sound. Nothing about the response or terminal
is passed to the player.

macOS plays `/System/Library/Sounds/Glass.aiff` with `afplay` at the current
output volume. Linux plays the freedesktop sound theme's
`/usr/share/sounds/freedesktop/stereo/complete.oga` with `paplay`. Playback uses
the same bounded host queue as notifications, after the notification when both
are on, with a five-second deadline. A missing or failing player displays
**Ready sound unavailable** in the footer and changes nothing else. Neither a
successful player command nor an unchanged footer proves the sound was audible:
a muted or absent output device silences it, and OVRCR does not change those
settings. Real host playback is recorded in the
[issue 60 evidence](../research/issue-60-ready-sound/README.md) for macOS.
Linux has automated coverage only; native Linux playback remains unverified.

## Output delivery

Dashboard output is delivered through a bounded queue, so a detached or slow
dashboard can reattach and refresh the current screen. Output backlog is
coalesced into a per-session refresh when necessary. Control responses and
lifecycle events are preserved, and the dashboard disconnects if the queue cannot
accept one of those messages.
