# Dashboard reference

Complete key, mouse, and rendering behaviour for the OVRCR dashboard. The
[README](../README.md#the-dashboard) covers the keys you need first; this page is
the full contract.

## Modes

The dashboard has four input modes. The footer names the current one. In
Browse it shows only the three keys that open everything else, `Space Menu`,
`: Search` and `? Help`; every other action is listed in the menu and the
palette. Copy and History show their own prioritized shortcuts, and hints that
do not fit remain available through `?` or `Space`. History shows `Esc Cancel
copy` while a copy is pending and `Esc Back` otherwise.

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

The sidebar's **Quota left** block shows remaining Claude, Codex and Grok account
allowance. See [provider allowance](provider-quota.md) for native sources,
collection settings, freshness and reset behavior.
Providers remain visible before the first report: **checking** until the
account read returns. **Codex usage off** and **Grok usage off** only while
`quota.enabled` is explicitly false. A stale value shows its age (`37% left  stale 12m`),
and a failed row shows when the Server tries again (`unavailable  retry 3m`).
Narrow sidebars put window values and full state text on a second row
before sacrificing labels or percentages. A short sidebar keeps at least three
tree lines and shrinks the block in steps: one line per provider, then a single
`Quota: u` pointer line, and only below that hides it. The tree scrolls above
the reserved quota rows; sidebar and pane dividers remain draggable over their
full height.

The palette offers **Refresh quota**, which asks the Server to read Codex and
Grok now, and Claude too while `quota.claude.probe` is on (a refusal inside the
Server's 30-second cooldown shows the remaining seconds in the footer), and,
while collection is off, **Enable Codex and Grok usage**, which asks the Server
to set `quota.enabled = true`. The Dashboard changes nothing itself; the rows
change when the Server republishes. The Claude probe is a separate consent,
`quota.claude.probe`, off until set; see [provider allowance](provider-quota.md).

| Key | Action |
| --- | --- |
| `j` / `k` / Down / Up | Select the next or previous visible sidebar row |
| `b` | Hide or show the sidebar; the panes take its width, and `j` / `k` still move the selection |
| `Enter` | Collapse or expand the selected project or workspace; send terminal input when a session is selected |
| `v` | Open a second pane showing the next different visible session |
| `Tab` / `Shift-Tab` | Focus the other pane |
| `x` | Close the focused pane; its session keeps running |
| `n` | Create a terminal (form) |
| `w` | Create a workspace (form) |
| `a` | Register a project (form) |
| `X` | Archive the selected terminal; confirms before stopping live work |
| `p` / `r` | Pause or resume the selected session; no confirmation |
| `R` | Mark the selected terminal's displayed unread Ready observation reviewed |
| `[` | Freeze the current screen for copying |
| `PageUp` | Open the session's retained history |
| `t` or `Ctrl-t` | Open scheduled tasks |
| `s` | Search other running agents across all projects and workspaces |
| `u` | Open [provider allowance details](provider-quota.md), including when the sidebar is hidden |
| `:` | Open the command palette |
| `Space` | Show contextual groups, then choose an action |
| `?` | Browse the same popup with arrows and `Enter` |
| `N` | Toggle desktop notifications for this dashboard |
| `S` | Toggle the ready sound for this dashboard |
| `q` | Detach; the server and every session keep running |

`N`, `S`, `R`, `b` and `[` act on a key press; a key repeat or release does nothing.
`Alt` and `Super` do not change which action a key runs: `Alt-x` closes the pane
that `x` closes. `Ctrl` reaches Browse only as `Ctrl-t`, whatever else is held
with it; every other `Ctrl`-modified key is ignored. `Esc` and `Ctrl-g` cancel a
history request that has not opened yet.

## Search other running agents

Press lowercase `s` in Browse, `Space v s` from the View menu, or sequentially
`Ctrl-g` then `s` while typing into a terminal. When space permits, the Terminal
footer shows this sequence. Uppercase `S` still toggles ready sound; `:` still
opens the full command palette.

The Agents picker includes nonarchived Agent-launch sessions with a running,
unpaused process, regardless of activity or reporting health. It includes every
project and workspace, even when their sidebar sections are collapsed or the
sidebar is hidden. Local shells, inactive agents and the currently focused
session are excluded. With no destinations it says **No other running agents**.
It never starts or reopens a process.

On opening, effective Waiting Input comes first, then otherwise Unread, then
other Running agents. Waiting Input plus Unread appears once in the first group;
Busy plus Unread remains in the second. Reviewed Ready, errors and reporting
loss receive no additional priority. Fully expanded hierarchy order breaks ties.
Rows reuse the dashboard's status wording, including Unread and reporting health,
and show Agent identity and session ID; the selected location remains visible
in compact layouts. Unsupported or unavailable reporting never excludes a
Running agent or implies Ready.

Search uses case-insensitive, every-word substring matching against the full
session title, Agent name, session ID, project and workspace identity. Opening or changing the query
highlights the first match; arrows move the highlight. One Enter selects the
highlighted destination and enables Terminal mode only after every matching
visible screen, the final view acknowledgement and the drain of already-pending
loading input. Enter used to choose never
reaches either PTY. During loading, keyboard input and paste are discarded,
never queued; the footer shows **Loading agent**. `Ctrl-g` or Escape cancels
pending typing and stays in Browse. Pane/window focus, selection or geometry
changes, lifecycle/run changes and failed, incomplete or superseded views also cancel it.
A late acknowledgement cannot enable an obsolete choice. Escape while the picker
is still open closes it in Browse without changing selection.

Membership, process runs, searchable identities and attention-ranked order are
captured when the picker opens. Live status, title, location and reporting health
update without reordering choices; updates cannot replace a highlighted choice. New agents and updated search identities appear on reopening. A removed, paused, stopped, archived or
restarted candidate stays unavailable until reopening, with a visible reason;
confirming it never selects a neighboring row. Switching focuses the other pane
if it already shows that session; otherwise only the focused pane is replaced.
It does not mark Unread reviewed or change process or conversation state.

## Unread responses

A managed Codex, Pi or Oh My Pi terminal keeps one Unread fact for its latest unreviewed
root Ready response. The sidebar does not draw a circle for it. While it is present, the
sidebar shows the Ready checkmark and the pane header says Unread. Mark reviewed clears
both of those marks. The Agent's reported activity stays Ready, so the header may still
say the session is ready. A new Busy report does not clear an earlier Unread response.
Selecting a terminal, viewing its output or receiving a notification does not acknowledge it.

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
- Within a server run, exited terminals keep their history until reopened or removed.
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
with a branch glyph and the current branch name in bold. The repository-root
workspace also shows a `root` marker. A warning marker appears when that
checkout is not on the default branch. Duplicate visible names include the
worktree path. A branch change updates the label in place; selection and panes
stay on that workspace. A session row starts with a status
glyph and the session name, with the agent harness (from session kind for agents, else the label) right-aligned in the
provider colour. When a model is known, it follows a colon in the quiet gray,
as in `pi:grok-4.7`. A model that repeats the agent name drops that repeat, and a
missing model leaves the agent name alone. A long name clips with `…` before that
label. The label shares the name's line only while every visible `agent:model`
fits beside at least twelve cells of its name. As soon as one does not, the
sidebar stacks as a whole: every session with an agent label keeps its name on
one line and moves the label to one extra line under it, hung by a `└`
connector two cells in from the status glyph, under the start of the name, and
clipped with `…` if even that line is too narrow, while local shells stay on one line. The model is never
dropped, and no two rows use different layouts at the same width. The extra
line belongs to the same session: `j`, `k` and the mouse treat both lines as one
row, the selection bar and background cover both, and the hover `[x]` sits on
the first line. Local shells show `$ local` on one line with no agent label
unless a hook reports activity inside them. The
selected row shows a mauve bar in its first column and a lighter background; its
own colours stay visible. A folded project shows `▸ N ws` and a folded workspace
`▸ N` at the right edge.

`b` hides the sidebar and the panes take the full width; `b` again shows it at
the width it had before. The choice lasts for the attachment and is not saved.
While the sidebar is hidden, `j` and `k` still move the selection, and
`Space v`, `?` and the palette offer **Show sidebar**.

| Glyph | Meaning |
| --- | --- |
| `-` | No hook report accepted yet |
| blank | Idle |
| braille spinner (green) | Busy |
| `?` (yellow) | Waiting for input, including an open [input request](#input-requests) |
| `!` (red) | Reported error |
| `✓` (teal) | Response ready, not yet marked reviewed |
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

- Clicking a session row selects it. `[x]` shows only while the pointer is over a session or workspace row. On a session it replaces the agent label and archives that session, the same as `X`, without switching the pane. A live or paused session asks first. An exited session archives immediately. On a workspace it opens Remove workspace for that row and does not clear the pane. A root workspace refuses, as the key does.
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
  resized. The chosen width lasts for the attachment and is not saved. A
  hidden sidebar has no border to drag, and clicks in its former area reach
  the pane.
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
highlighted; Enter accepts it and opens confirmation. Closing a live terminal opens
confirmation directly. Existing server removal safeguards
still apply; repositories and workspace branches are retained. Confirmed workspace
removal archives stopped records with their original paths. Live or ownership-uncertain
sessions block removal, even when archived; the Archived sessions page offers
"Acknowledge stopped" for such rows. A removal refused for uncertain sessions or a
dirty worktree reopens as a forced removal: confirming it acknowledges the stopped
processes and discards uncommitted changes, while live sessions and active task
runs still block. The repository-root workspace cannot be removed. Remove every
other workspace first, stop or acknowledge remaining sessions, then unregister
the project; repository files stay on disk. Archived context is kept. `Space w n`
creates a terminal, `Space p n` creates a workspace, `Space v t` opens tasks,
`Space v b` hides or shows the sidebar, and `Space v ,` opens Settings.
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
Up/Down to choose, and Tab or Enter to accept. Workspace choices show the
project and current branch, with the worktree path when those names collide.
Removal excludes the protected repository-root workspace. Session names and
project names remain text fields, and project registration retains its path pickers.

Closing live work and deleting records show their current branch target and require confirmation.
Closing an exited row archives it immediately. A workspace with live sessions or
active task runs cannot be removed. A refusal for ownership-uncertain stopped
sessions or a dirty worktree reopens as a forced confirmation that discards
uncommitted changes and acknowledges those stopped processes. Errors stay in the
palette with the form values retained.

### Create terminal (`n`)

Choose **Agent** or **Terminal** in Start. Agent offers installed agents and named
`[[agents]]` presets. Terminal opens your default shell; its optional Command
runs through `/bin/sh -lc`. Custom commands belong to Terminal, even when the
command happens to invoke an agent. A launch choice does not guarantee agent
reporting support.

Workspace defaults to the selected workspace. Name is optional: leave it blank
for a stable workspace-based name, or enter your own name. After creation, typing goes to the new
session immediately. `n` and other new launches are unavailable when the selected
root workspace shows a warning. Detected `claude`, `codex`, `pi`, `omp`, `grok`, `hermes` and `cursor-agent`
entries launch through the managed `agent run` route; an
`agents` override replaces the command entirely. Detected Claude, Codex, Pi, and Oh My Pi
launches auto-trust by default: the argv gains `--dangerously-skip-permissions`,
`--full-auto`, `--approve`, or `--auto-approve` when it does not already contain a
permission-mode, approve, auto-approve, full-auto, or dangerously-* flag. A flag already
in that family is left as written; nothing is appended or replaced, and there is no
global switch. `ovrcr agent run` does the same when its native argv is bare, including
a `[[agents]]` preset that is itself a bare `agent run`. A preset that already names one
of those flags is stored and launched as written; a raw command that does not go through
`agent run` is not rewritten. Grok, Hermes, and Cursor are not given a flag. This
replaces the earlier non-goal that managed launches would not add a trust bypass. Claude, Codex, Pi and Oh My Pi
deliver Ready/Unread on their supported readiness paths (Claude Ready is Observed,
never Confirmed settling). Claude/Codex Input-request support remains unavailable
until their dependent tickets. Missing hook configuration shows
a persistent Dashboard banner with the exact setup command; observed reporting loss
keeps an `unavailable` status with its published reason.

Hermes uses the same process supervisor with native arguments unchanged. Its row
shows unknown activity; reporting, Ready/Unread, Input requests, metrics, generated
titles and recovery remain unavailable. See [Hermes harness](hermes-harness.md) for
setup/doctor guidance and the distinction between source review and native acceptance.

Cursor CLI has a source-pinned fresh startup identity adapter. Activity remains
unknown; Ready/Unread, Input, metrics, generated titles and recovery are unavailable.
Resume and unsupported versions/options remain native with reporting unavailable.
See [Cursor harness](cursor-harness.md) for the exact release and passive plugin contract.

### Create workspace (`w`)

Choose a branch and what to start: **Agent**, **Terminal**, or
**Nothing yet**. There is no separate workspace name. Agent and Terminal have the same options as terminal creation.
Only the selected program starts; an Agent launch does not also open a shell.
Nothing yet creates an empty workspace.

Each project remembers its last successfully launched Agent and preset, or
Terminal choice, across dashboard restarts. One-off command text is never saved.
Nothing yet leaves the preference unchanged. A project without a preference
starts with Terminal selected. If a remembered agent is unavailable, choose a
replacement explicitly before launching.

Project defaults to the selected terminal's project. Branch has initial focus
and starts as `feature/`; typing appends to that prefix. `Enter` on Branch
submits. `Tab` and `Shift-Tab` reach the other fields. On Branch mode, `Space` or
Left/Right switches between `new` and `existing`. Existing mode lists local
branches and hides Base.

Base defaults to the repository's `origin/HEAD` target, then `main`, `master`, or
the current HEAD. A remote-only default uses its full Git ref as Base. Git
suggestions load in the background, are cached per project until the palette
closes, and time out after two seconds. A footer note explains failures, after
which Branch and Base accept free text. Submission waits for pending suggestions.

If the workspace is created but its first program cannot launch, the workspace
remains. Retry, choose another agent, or open a shell in that existing workspace.
Retry does not confirm that a previous process has stopped.

If a program starts and later exits, or the server restarts, the session row
stays. Reopen in a fresh shell is an explicit action on that same row; it does
not keep the previous output, history, copy selection, or unread marker.
Natural exit alone does not prove background jobs stopped. Confirm they have
stopped before reopening, or acknowledge stopped without launching. Retry is not
that confirmation. A stale confirmation is rejected; a fresh confirmation names
the current run and requires another explicit submission.
If a first launch cannot prove that no process started, the retained row stays.
The failed create form cannot launch another session. Select the intended row
yourself, then use its Reopen or Acknowledge action; the Dashboard never guesses
which uncertain row to acknowledge. Certified retained Claude, Codex, Pi and Oh My Pi conversations offer
Resume conversation on the same row, without submitting a new prompt. The status
shows that the agent launched while conversation attachment is still pending; a
matching native callback establishes attachment. Start new conversation opens
the create form for a separate session. Codex, Pi and Oh My Pi also have native
resume adapters, with the limitations below. Other providers remain unavailable.
Workspace removal remains a separate action.

Claude recovery retains the exact UUID, provider history path, executable and
non-secret configuration references. Missing history, executable, recorded
working directory or matching configuration blocks recovery with a diagnostic;
Retry never starts fresh. An observed unsupported or ambiguous Claude
conversation transition (`identity_transition_unavailable`, including bare fork
without a leave of the current conversation) permanently disables recovery for
that row, retaining its old UUID as context. Supported clear / in-process resume /
foreground-branch replacements update the retained reference instead.
Absent callbacks or temporary reporting failures alone do not erase that UUID.
Inventory restoration never launches a process. First display in a visible pane
requests one native resume for an unarchived interrupted Agent with an available
adapter and no previous failure. A verified different boot permits automatic
recovery; same-boot or unknown ownership still requires explicit confirmation
that old agents and descendants stopped. Live runs are reused. Shells, deliberately
stopped/exited rows and newly unarchived rows do not launch automatically.

Missing prerequisites or full live capacity leave a diagnostic and **Retry resume
conversation** in the command palette. Fix the prerequisite, then explicitly retry;
redraws and reconnects do not retry failed launches. Hidden panes and sidebar rows
do not trigger recovery. Native GUI acceptance of this automatic path is tracked
separately in #127. See [Claude recovery support](agent-reporting-support.md#retained-claude-conversations).

Codex recovery resumes the exact UUID from its last authenticated root hook.
Until matching `SessionStart(source=resume)` binds that conversation, status says
`Agent launched; awaiting conversation attachment` — process creation is not
attachment, and historical Ready is not restored. After attachment, a new root
turn reports Observed Ready/Unread through the existing path. Supported clear and
in-process resume SessionStart replacements update the saved reference; a
conflicting startup while bound shows `identity_transition_unavailable` and
disables recovery for that row without restarting Codex. Silent native history
switches cannot update the reference without a supported hook. See
[Codex recovery limitations](codex-reporting-setup.md#retained-conversation-recovery).

Pi and Oh My Pi retain the exact native session ID and file from their managed
extension. History must remain unchanged by external processes during reopening,
from prelaunch validation until native attachment. Concurrent deletion or replacement
can cause a native fresh session and is outside the recovery guarantee.
Accepted conversation switches replace the reference, including switches
back to an earlier conversation. An ephemeral conversation replaces the old
reference but cannot be resumed. Unsupported launch configuration leaves resume
unavailable without disabling the provider's existing reporting.
See [provider recovery support](agent-reporting-support.md#retained-pi-and-oh-my-pi-conversations).

### Stable session titles

Terminal sessions keep their user-supplied name or original workspace-based
name. Agent sessions show a manual title first; otherwise a saved conversation
subject for the recorded conversation may be shown before the original session
name. Application title updates (OSC 0 and OSC 2), including “Thinking…”, are
ignored for both Agent and Terminal launches—even when an agent starts inside
a shell. Activity indicators and terminal output continue to update normally.
Duplicate displayed titles include a short session identifier so they can be
distinguished.

Use **Rename terminal** in the command palette to choose a new display title.
Clear it to restore the original session name, not the workspace's current branch
name. Rename never changes the session ID or retargets an action. Desktop
notifications continue to use stable identity.

Manual titles survive dashboard detach, server restart, and reopen. Reopen
replaces the process, not the row or its naming choice. Existing user-set titles
are preserved after upgrading; previously saved application titles are ignored.
A silent Codex history switch does not change the displayed subject; the row
follows only the conversation reference OVRCR has recorded.

### Register project (`a`)

Repository and Workspace root are path pickers: the list is one directory of
children, directories only, filtered by fuzzy subsequence on the last path
segment. Hidden names stay hidden unless the typed segment starts with `.`. Git
checkouts sort first and are marked `git`. `Tab` accepts the highlight and
appends `/`; `Enter` keeps the typed text and moves on. An empty field lists
`picker_roots` from `dashboard.toml` in document order, not sorted. Name becomes
the repository basename once a path is chosen. Workspace root defaults to `<config dir>/workspaces/<name>` until
you edit it, and is created on registration if missing. Registration creates the
protected repository-root workspace on the detected default branch. Whether it
also starts a `local` shell there follows `automatic_local_terminals` (default
**default branch only**). It does not switch Git.

## Empty states

With no project registered, the dashboard shows `Welcome to OVRCR`, the `a`, `:`,
and `?` shortcuts, and the resolved config and socket paths. Clicking an empty
workspace shows how to start a terminal there and prefills that workspace in the
terminal form. A root-workspace warning disables `n` and other new launches
there; existing sessions stay usable. OVRCR does not switch the checkout.

## Dashboard settings

Every setting lives in one settings document, `dashboard.toml` beside
`config.toml`, including the Server's `title_model`, `automatic_local_terminals`
and `[quota]`. Override the path with `OVRCR_DASHBOARD_CONFIG`; it chooses a file
only. Remembered launch choices stay in this file. Do not put settings in
`config.toml`. That path remains the instance identity for the project/workspace
database (`config.toml.sqlite3`) and scheduled-task storage (`config.tasks`); a
`[quota]` table there configures nothing and is reported as a finding.

A missing file uses defaults. A symlink at that path whose target does not
exist is not a missing file: it is one document-level finding naming the path.
A wrong-typed value keeps its own default. A wrong-typed element of a list or
table (`picker_roots[1]`, `agents[1]`, `launch_choices.demo`) is dropped on its
own, with a finding naming that element; sibling elements stay. An
unknown key or an unknown spelling such as `automatic_local_terminals = "always"`
is ignored; each is reported as a finding with its key and line. Only a document
that is not valid TOML falls back to every default, with one document-level
finding. Run `ovrcr settings` to see the document path, each setting's owner,
effective value and source (default or document), and the findings.

The Server is the only reader. The Dashboard parses no settings file: at every
attach the Server reads the document and sends its reading, and the Dashboard
takes every setting from it. Set `OVRCR_DASHBOARD_CONFIG` where the Server runs;
the installed service does not see a shell export, and the Dashboard follows the
Server's document whatever its own environment says.

### Settings view

Open **Settings** from the menu (`Space v ,`) or search the command palette for
**settings**; it has no Browse key of its own. The view is the settings editor.
It shows the document path and when the Server read it, then findings that belong
to no setting (unknown keys and document-level problems, each with its line), then
every setting in one scrolling list under the group headers **Alerts**
(`desktop_notifications`, `ready_sound`), **Workspaces**
(`automatic_local_terminals`, `branch_prefix`, `picker_roots`), **Titles**
(`title_model`), **Usage** (`quota.*`), **Agents** (`[[agents]]`) and
**Remembered launches** (`launch_choices`, one row per project).

Each row shows its label, effective value (`unset` for an empty optional value)
and source badge, `default` or `set`; a value the document sets also shows the
default beside it, for example `kh/  set  (default: feature/)`. A setting's
finding shows under its row. A consent setting carries its one-sentence side
effect and, while off, the line that says how to turn it on: turning it on asks
for no confirmation.

| Key | Action |
| --- | --- |
| `↑`/`↓`, `j`/`k`, Page Up/Down, Home/End, wheel | Move the selection |
| `Enter` | Edit the selected row in place |
| `[` / `]` | Move the selected picker root up or down |
| `r` | Reset: remove the key so the source returns to `default` (only for a `set` row) |
| `x` / Delete | Remove the selected element of a collection |
| `/` | Filter rows across groups with the palette's word filter; Enter keeps it, Escape clears it |
| `Escape` | Cancel the current edit, else clear the filter, else close |

Enter edits with the palette's field kinds: a boolean saves the other value at
once; `automatic_local_terminals` and a remembered launch open a pick list
(Terminal or a detected agent); strings open a text field; paths open a text
field whose Tab completes directories. Enter saves that one row and Escape cancels
only that edit. An emptied top-level value resets it. Every save is one
`SetSetting` request: the row shows `saving…` and changes only when the Server's
new reading arrives. A value the Server refuses shows under its row as
**refused: …** and the document is unchanged.

`picker_roots`, `[[agents]]` and `launch_choices` expand into one child row per
element or entry plus an **Add** row. `[` and `]` on a picker-root row send one
`SetSetting` of the whole `picker_roots` list in the new order; the first row
cannot move up and the last cannot move down. Agent rows have no order, so those
keys do nothing there. An agent row edits
its `argv` as a TOML array literal such as `["claude", "--verbose"]`; Add asks for
the agent's name and starts its argv as that name. Add under Remembered launches
asks for a project name and remembers Terminal. While `picker_roots` is its
default, an edit or Add writes the whole default list with the change.

While the document is not valid TOML, every row shows its default, the top
finding names the line, and **Editing is off until dashboard.toml is fixed by
hand.**; Enter, `[`, `]`, `r` and `x` change nothing.

At attach the footer shows one line when the reading has findings, for example
**2 settings findings; see Settings**, and nothing when it has none. When a later
reading changes the number of findings the footer says so again, or **No settings
findings** once they are gone. The next key clears the line. A startup error
banner owns that line first: the findings notice waits behind it, a key while the
banner is up does not drop the notice, and the notice shows when the banner is
dismissed.

The block is the production default document. A missing file loads these values
(`picker_roots` keeps only the paths that exist). Commented lines show optional
keys that are unset by default.

```toml
desktop_notifications = false        # opt in to background Ready and input-needed alerts
ready_sound = false                  # opt in to a sound for the same two alert kinds
automatic_local_terminals = "default_branch_only"  # on | off | default_branch_only
# title_model = "provider/model"     # optional; unset leaves automatic titles off
branch_prefix = "feature/"            # prefix for new workspace branches
picker_roots = ["~/Code", "~/src", "~"]

# [[agents]]                         # optional; a row replaces a detected agent's argv
# name = "claude"
# argv = ["claude", "--verbose"]

[quota]                              # account quota; see provider-quota.md
# enabled = false                    # explicit off; omit to collect while a Dashboard is attached

[quota.claude]
probe = false                        # opt in: a hidden Claude run may spend allowance

[quota.codex]
command = "codex"
# home = "/absolute/path/to/codex-profile"

[quota.grok]
command = "grok"
# home = "/absolute/path/to/grok-profile"
```

Every setting takes effect without a restart: the Server checks the file every
two seconds and applies a saved change on its next check.

`automatic_local_terminals` controls only **automatic** creation of terminals named
`local` when a workspace is provisioned:

- `on` — create a local terminal for every newly provisioned workspace.
- `off` — never create local terminals automatically, including on the default-branch workspace.
- `default_branch_only` — create a local terminal only in the project's detected default-branch (repository-root) workspace. This is the default when the key is missing.

Changing the setting leaves existing terminals and agents untouched. Explicit
terminal and agent launches remain available under every option. Cycle the
preference from Browse with `L`, run `ovrcr settings set automatic_local_terminals
on`, or edit `dashboard.toml` and keep the running server; the next provisioning
read picks up the saved value.

`picker_roots` defaults to `~/Code`, `~/src`, and `~`, keeping only the paths
that exist. An `[[agents]]` row whose name matches a detected agent replaces its
argv; a new name becomes an Agent preset. The argv is used as configured; a preset
name alone does not prove what program it launches or whether reporting is supported.

Successful launches update a per-project `launch_choices` table in this file.
For example, `[launch_choices.consigint]` with `kind = "Terminal"` remembers a
terminal; `kind = "Agent"` and `preset = "claude"` remember an agent preset.
These entries contain no one-off command text. Failed launches and Nothing yet
leave the remembered choice unchanged.

### Settings writer

The Server is the only writer. The Dashboard touches no file: `N`, `S`, `L` and
a remembered launch choice each send the Server a `SetSetting` request naming a
setting path (`ready_sound`, `launch_choices."my.project"`) and TOML value text.
`ovrcr settings set` and `reset` send the same request, and edit the document
themselves only when no Server is running (see the
[CLI reference](cli-reference.md#settings)). The Server checks the value against
the setting's declared type, rejects a path that names no setting, edits the
document it reads, re-reads it at once and sends the new reading to the
Dashboard. It applies one edit at a time to the latest document; editors outside
OVRCR are not locked.

Each edit preserves comments, unrelated formatting, unknown keys and tables,
other settings, and other projects' choices. A reset removes the key, list
element or launch choice, and any table it leaves empty; comment lines above a
removed key stay in the document. Existing symlinks are followed without
replacing the link, and target permissions are retained. A missing ordinary
file and its parent directories are created; new settings files are private
(mode `0600`). Dangling links, read-only targets, and a document that is not
valid TOML are rejected without changing the document; while the document is
not valid TOML, editing is off until it is fixed by hand. A wrongly typed value
elsewhere is a finding, not a reason to refuse: an edit keeps it byte for byte,
and an edit of that same key replaces it. A value the loader would turn into a
new finding for the edited setting, such as `ready_sound = "yes"`, is rejected
with the expected type and the document is unchanged.

Saves write and synchronize a temporary file beside the resolved target, then
replace the target atomically. A failed save leaves the original document intact
and removes its temporary file.

A toggle applies when the Server's new reading arrives, not when the key is
pressed; the footer then names the change, for example **Desktop notifications:
on**. A refused save shows the Server's message in the footer, such as
**could not save ready_sound: settings document … is read-only**, and leaves
active preferences and alert channels unchanged. If a session starts but
remembering its launch choice fails, the session remains selected and usable,
the choice stays remembered in the current Dashboard, and the footer shows
**could not save launch_choices.…** with the reason. The Dashboard does not
retry creation or reopen the launch form.

## Events

Open **Events** from the menu (`Space v e`) or search the command palette for
**events**. It has no Browse key of its own. The popup is the same bordered
panel as Quota details. It lists the Server's event ring, oldest first and
newest last. Opening it asks the Server for that ring. The Dashboard does not
read `events.jsonl`, and this view does not change the file.

Each line is the local time, the component (`titles`, `settings`, or `quota`),
the subject when there is one, and the message. While the popup is open, an
event the Server records is appended. Events recorded while it is closed appear
the next time it opens. The list opens on the newest line and stays there until
you scroll up.

| Key | Action |
| --- | --- |
| `/` | Word filter, the same rule as Settings: every word is a case-insensitive substring of the component, the subject, and the message. The time is not searched. Enter keeps the filter. |
| `Tab` / `Shift-Tab` | Component filter: all, then titles, settings, and quota, and back. It applies together with the word filter. |
| `↑`/`↓`, `j`/`k`, Page Up/Down, Home/End, wheel | Scroll. Home shows the oldest match. End returns to the newest. |
| `Escape` | Clear the word filter, or close when the filter is empty |
| `Ctrl-g` | Close |

Enter leaves the word filter and does not close the popup. `j` and `k` scroll
rather than moving a selection, because a row has nothing to activate. Tab
changes the component only when the word filter is not being typed.

## Desktop notifications

Desktop notifications are off by default. Set `desktop_notifications = true` in
`dashboard.toml` to enable them when attaching. In browse mode, press uppercase
`N`, or search the command palette for **desktop notifications**, to enable or
disable them. Each toggle asks the Server to save it in the document it reads
(normally `dashboard.toml`) and takes effect when the Server's new reading
arrives, normally at once. A missing file is created; a refused save is shown
without changing the active preference. Other configuration values and comments
are retained. Manual file edits are loaded within two seconds. In terminal mode
`N` remains ordinary terminal input; use Ctrl-g first.

Two kinds of alert share these preferences and this host.
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

The active dashboard delivers otherwise eligible alerts whether or not the
terminal is selected or occupies a visible pane, including focused, unfocused and
split views. Selection and pane visibility do not suppress or cancel pending or
in-flight alerts, and application focus is not tracked. Viewing a session or
delivering an alert does not acknowledge Ready. Attachment and reconnect
establish a baseline: old unread observations and input requests already open,
including ones that arrived while disconnected, are not replayed. Enabling alerts
does not replay a previously consumed response or request. A later Busy or
reporter loss cancels a pending or in-flight Ready alert; closing or replacing an
input request cancels its own. Mark-reviewed does not.
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
command palette for **ready sound**, to toggle and save it through the Server
with the same settings file and failure behavior. In terminal mode `S` remains ordinary
terminal input; use Ctrl-g first.

A sound follows exactly the same selection as a desktop alert: a new unread
identity for one accepted managed root response, or a new input request, with the
same deduplication and attach/reconnect baseline, including for selected and
visible sessions. The sound is the same for both kinds.
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


### Archived sessions

Closing a live session (`X` or `Space t x`) asks for confirmation, stops its
currently owned processes, then archives its record. A failed stop leaves the
active row available. Closing an exited row archives immediately. Hiding a pane
or detaching never archives a session.

Open the command palette (`:`), choose **Archived sessions**, and search by title,
project, workspace or original working-directory path. Each result shows the
retained path, even after workspace removal. **Unarchive** returns the row stopped
without launching anything. Rows whose workspace or project was removed remain
selectable under their original context. Reopen reports a missing directory
rather than choosing another working directory. **Delete record** asks for confirmation and removes only OVRCR metadata;
provider conversation files remain untouched. If old process ownership was
uncertain before archiving, reopening still requires explicit acknowledgement.
