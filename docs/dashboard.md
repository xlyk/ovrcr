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
| `j` / `k` / Down / Up | Select the next or previous visible sidebar row |
| `Enter` | Collapse or expand the selected project or workspace; send terminal input when a session is selected |
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
| `N` | Toggle desktop notifications for this dashboard |
| `S` | Toggle the ready sound for this dashboard |
| `q` | Detach; the server and every session keep running |

`N`, `S`, `R` and `[` act on a key press; a key repeat or release does nothing.
`Alt` and `Super` do not change which action a key runs: `Alt-x` closes the pane
that `x` closes. `Ctrl` reaches Browse only as `Ctrl-t`, whatever else is held
with it; every other `Ctrl`-modified key is ignored. `Esc` and `Ctrl-g` cancel a
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

## Input requests

A managed terminal reports the human answers it is waiting for. Each request is
identified by its binding, its namespace and its own identity, and several can be
open at once:

| Namespace | Source | Kinds |
| --- | --- | --- |
| `prompt` | A Pi extension dialog's outer prompt span | select, confirm, input, editor, custom |
| `approval` | An Oh My Pi native tool-approval prompt, keyed by tool call id | approval |
| `question` | An Oh My Pi ask-tool execution, keyed by tool call id | select |

Pi coalesces nested prompts into one outer span, so one dialog is one request.
An Oh My Pi question is the ask tool's lifetime, not the dialog's visibility: it
opens a moment before the dialog appears and covers a question queued behind
another one. At most 32 requests are open per binding; a producer that would
exceed that drops the new request rather than stopping reporting.

While any request is open the terminal's effective activity is `waiting_input`,
whatever was underneath it — Busy, Idle, Error or Ready — stays recorded in
`agent.activity`, and closing the last one restores it. The requests travel in
the same publication as the activity sample, so the two are never seen apart,
and the set is published whole rather than as a stream of deltas.

An input request is not a response. It never creates, clears or retargets an
[unread](#unread-responses) response, and answering one is not a review. Reporter
loss — and a reporter paused because what it delivered is no longer certain —
forgets every open request and keeps the unread response; a recovered reporter
starts with none and replays no alert for what was open before. This holds for Oh
My Pi's approvals and questions exactly as it does for Pi's prompts. Only the request
identity, its namespace and its kind leave the extension; the prompt title, the
approval reason, the question and the answer never do.

Each newly opened request may raise one **OVRCR · input needed** alert, once.
Closing a request cancels its queued alert and leaves the others; a set already
open when the Dashboard attaches or reconnects is a baseline and never replays.
Cancellation only reaches an alert that has not been sent yet, so a request that
opens and closes before its dialog is ever drawn — an Oh My Pi approval resolved
in the same tick as its request, or an ask execution aborted with no interactive
UI — can still deliver one alert for a wait you will not find on screen.

Read `agent.input_requests` from a terminal's JSON row for the open set, oldest
first. The metadata line labels the oldest one.

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
glyph, the session name, and the agent name from its label right-aligned in the
provider colour. A long name clips with `…` before the agent name, and the agent name is
dropped when fewer than twelve cells would remain for the name. Local shells
show `$ local` with no agent label unless a hook reports activity inside them. The
selected row shows a mauve bar in its first column and a lighter background; its
own colours stay visible. A folded project shows `▸ N ws` and a folded workspace
`▸ N` at the right edge.

| Glyph | Meaning |
| --- | --- |
| `-` | No hook report accepted yet |
| blank | Idle |
| braille spinner (green) | Busy |
| `?` (yellow) | Waiting for input, including an open [input request](#input-requests) |
| `!` (red) | Reported error |
| `✓` (teal) | Response ready |
| `P` | Paused |
| `·` | Exited (the row is dimmed) |

An unavailable reporter draws the glyph in the muted colour. The literal state,
quality, and reporter health (`agent busy confirmed`, `response ready ·
observed`, `unavailable`) appear on the selected session's metadata line, not
in the sidebar. While an input request is open the metadata line reads
`input needed · select` (or `confirm`, `input`, `editor`, `custom`) in place of
the activity underneath it; the question itself never appears.

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
The Terminal group is hidden while a project or workspace is selected; bare
Enter still collapses or expands that row. Bare hotkeys remain unchanged.

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

Popup actions come from the same key-binding table as the footer and the
palette: the group's key, the target description, and the reason a dimmed row
is unavailable are the table's, and choosing a row runs the same handler the
bare key does. The palette and direct-key footer retain their original
shortcuts.


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

Choose **Agent** or **Terminal** in Start. Agent offers installed agents and named
`[[agents]]` presets. Terminal opens your default shell; its optional Command
runs through `/bin/sh -lc`. Custom commands belong to Terminal, even when the
command happens to invoke an agent. A launch choice does not guarantee agent
reporting support.

Workspace defaults to the selected workspace. Name is optional: leave it blank
for Automatic, or enter a pinned name. After creation, typing goes to the new
session immediately. Detected `pi` and `omp` entries retain their managed launch
wrappers; an `agents` override replaces the command entirely.

### Create workspace (`w`)

Name the workspace and choose what to start: **Agent**, **Terminal**, or
**Nothing yet**. Agent and Terminal have the same options as terminal creation.
Only the selected program starts; an Agent launch does not also open a shell.
Nothing yet creates an empty workspace.

Each project remembers its last successfully launched Agent and preset, or
Terminal choice, across dashboard restarts. One-off command text is never saved.
Nothing yet leaves the preference unchanged. A project without a preference
starts with Terminal selected. If a remembered agent is unavailable, choose a
replacement explicitly before launching.

Project defaults to the selected terminal's project. Name has initial focus;
`Tab` and `Shift-Tab` reach the other fields. On Branch mode, `Space` or
Left/Right switches between `new` and `existing`. New branches start as
`feature/<name>` and follow Name until you edit Branch. Existing mode lists local
branches and hides Base.

Base defaults to the repository's `origin/HEAD` target, then `main`, `master`, or
the current HEAD. A remote-only default uses its full Git ref as Base. Git
suggestions load in the background, are cached per project until the palette
closes, and time out after two seconds. A footer note explains failures, after
which Branch and Base accept free text. Submission waits for pending suggestions.

If the workspace is created but its first program cannot launch, the workspace
remains. Retry, choose another agent, or open a shell in that existing workspace.
If a program starts and later exits, its session and output remain visible;
relaunching is an explicit action and creates a new session while retaining the
old output. Workspace removal remains a separate action.

### Automatic and pinned titles

Automatic titles follow the running application's terminal-title updates (OSC 0
and OSC 2), including titles such as “Thinking…”. Before an application supplies
a title, the session uses its workspace-based fallback name. Apps that do not
supply titles keep that fallback. Duplicate displayed titles include a short
session identifier so they can be distinguished.

Use **Rename terminal** in the command palette to pin a title. Clear it to return
to Automatic. Title changes never change the session ID or retarget an action.
Activity indicators remain separate, and desktop notifications continue to use
stable identity rather than application titles. Titles and exited output survive
dashboard detach while the server remains alive; this feature does not restore
sessions after server restart.

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
path with `OVRCR_DASHBOARD_CONFIG`. Remembered launch choices stay in this file.
Do not put these keys in `config.toml`. That path remains the instance identity
for the project/workspace database (`config.toml.sqlite3`) and scheduled-task
storage (`config.tasks`); it is not dashboard settings. A missing file uses defaults,
and a parse error shows in the footer and also uses defaults.

```toml
desktop_notifications = false        # opt in to background Ready and input-needed alerts
ready_sound = false                  # opt in to a sound for the same two alert kinds
branch_prefix = "feature/"            # prefix for new workspace branches
picker_roots = ["~/Code", "~/src", "~"]

[[agents]]
name = "claude"
argv = ["claude", "--verbose"]
```

`picker_roots` defaults to `~/Code`, `~/src`, and `~`, keeping only the paths
that exist. An `[[agents]]` row whose name matches a detected agent replaces its
argv; a new name becomes an Agent preset. The argv is used as configured; a preset
name alone does not prove what program it launches or whether reporting is supported.

Successful launches update a per-project `launch_choices` table in this file.
For example, `[launch_choices.consigint]` with `kind = "Terminal"` remembers a
terminal; `kind = "Agent"` and `preset = "claude"` remember an agent preset.
These entries contain no one-off command text. Saving a preference preserves
other settings values but may reformat the TOML file.

## Desktop notifications

Desktop notifications are off by default. Set `desktop_notifications = true` in
`dashboard.toml` to enable them when attaching. In browse mode, press uppercase
`N`, or search the command palette for **desktop notifications**, to enable or
disable them. Each toggle saves to `dashboard.toml` (or the path selected by
`OVRCR_DASHBOARD_CONFIG`) and takes effect immediately. The next dashboard
loads those saved settings. A missing file is created; a save error is shown
without changing the active preference. Other configuration values and comments are retained. Manual file edits are
loaded on the next attach. In terminal mode `N` remains ordinary terminal input; use Ctrl-g first.

Two kinds of alert share these preferences, this host and this suppression.
**OVRCR · response ready** means a managed root response from a supported
readiness provider is ready to review, not that its task succeeded; delivery keys
on a new [unread](#unread-responses) identity (binding and turn), not on activity
quality, and Confirmed activity without unread does not notify.
**OVRCR · input needed** means a managed terminal has opened a new
[input request](#input-requests); delivery keys on the request identity, at most
one alert per logical request, and answering it is not a review. The two lanes are
independent: a Ready and an open request on the same cycle each get their alert and
neither suppresses the other. Only project, workspace and terminal identity appear
in either alert. Prompt and response text, prompt titles and answers are never
included. The accepted Codex reporting setup is still
required; see [Codex setup](codex-reporting-setup.md). Pi and Oh My Pi need no setup
beyond the managed launch.

The active dashboard delivers alerts only when the terminal is absent from every
visible pane. A terminal assigned to a split hidden by a small window is eligible;
Tasks shows no terminal panes. Visibility does not acknowledge or change Ready.
Attachment and reconnect establish a baseline: old unread observations and input
requests already open, including ones that arrived while disconnected, are not
replayed. Enabling alerts or hiding a terminal does not replay a previously
suppressed response or request. A later Busy or reporter loss cancels a pending or
in-flight Ready alert; closing or replacing an input request cancels its own.
Mark-reviewed does not.
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
command palette for **ready sound**, to toggle and save it using the same settings file and failure behavior. In terminal mode `S` remains ordinary
terminal input; use Ctrl-g first.

A sound follows exactly the same selection as a desktop alert: a new unread
identity for one accepted managed root response, or a new input request, outside
every visible pane, with the same deduplication, visibility suppression and
attach/reconnect baseline. The sound is the same for both kinds.
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
