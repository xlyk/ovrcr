# Dashboard reference

Complete key, mouse, and rendering behaviour for the OVRCR dashboard. The
[README](../README.md#the-dashboard) covers the keys you need first; this page is
the full contract.

## Modes

The terminal workspace view has four input modes. The footer names the current one. In
Browse it shows only the three keys that open everything else, `Space Menu`,
`: Commands` and `? Help`; every other action is listed in the menu and the
palette. Copy and History show their own prioritized shortcuts, and hints that
do not fit remain available through `?` or `Space`. History shows `Esc Cancel
copy` while a copy is pending and `Esc Back` otherwise.
While a session needs you, the Browse footer also shows the attention key: `i
Next input needed` or `i Next ready`, naming where it goes.

| Mode | Entered by | Leaves with |
| --- | --- | --- |
| Browse | the default; `Ctrl-g` from any other mode | `q` detaches the dashboard |
| Terminal | `Enter` on a running session | `Ctrl-g` |
| Copy | `[` in Browse | `Esc`, `q`, or `Ctrl-g` |
| History | `PageUp` in Browse, or wheel-up over a pane | `Esc`, `q`, or `Ctrl-g` |
| Transcript | `PageUp` or wheel-up on full-screen Claude Code or Codex | `Esc`, `q`, or `Ctrl-g` |

Terminal mode is the only mode that sends input to the PTY. `Ctrl-g` leaves it and
opens the session-jump labels in Browse. Other Control shortcuts, including
Ctrl-c and Ctrl-o, keep their normal program or Dashboard handling. `Ctrl-o`
reaches the program there (Oh My Pi expands tool
output with it, Claude Code opens its transcript), so Operator **Quick message** is
`Ctrl-g` then `Ctrl-o`; the Terminal footer shows this when space permits. Click the
title bar's Commands control to open the palette or Menu to open the action menu.
Press `Ctrl-g` first to use dashboard shortcuts. The label picker it opens closes
on any non-digit key, which then runs its normal action.

### Footer messages

When something cannot be done, the footer says why in place of the hints. An
expected limit (a pane still loading, a paused session, a refresh cooldown, a
view the Server refused for now, keys the Server did not let through to a
session, or a refusal that names its blocker, such as sessions that still block
a removal) reads muted and in plain words. A failure, including any lifecycle
work the Server could not do, reads `ERROR:` in red. Neither shows the Server's
error code (`Conflict:`); `project/<workspace id>` reads `project / branch`, a
session `#N` reads as its title, an id nothing names stays as written, and a
path keeps its folder names as they are on disk. A message clears when a later
request of yours succeeds. Otherwise a limit clears after ten seconds, and a failure ten
seconds after the first key you press while it shows, so one that arrives while
you are away waits for you. A notice that waited behind it shows next.

## Operator chat

OVRCR has one Operator conversation, run by the installed pi agent. The sidebar
shows attention first when needed, then a three-row **Operator** section before
the project tree, followed by Resources and quota. Click anywhere in the section
or press `o` in Browse to open the conversation with the composer focused. There
are no Operator controls in the title bar.

Before the conversation is opened, the section says **Off** when the Pi
executable is available and **Not set up** when it is missing. A known Pi setup
error after opening also shows **Not set up**. After opening, it shows **Working** with
elapsed time and current activity, **Reply ready · 1 unread** with a reply
preview, or **Idle** with the latest reply. The section omits the model and
harness. Drawing it never reviews a reply or starts a worker. The
`operator.sidebar` setting controls the section and is on by default.

**Quick message** remains in Commands. `Ctrl-o` opens it for the same conversation
from every view except Terminal mode; its composer edits the same draft and
captures the same workspace/session target for the next message as the full view.
`Ctrl-g` or `Esc` leaves Operator for the terminal panes in Browse,
keeping the draft; `o` returns to the same history and draft. Reopening never
resets the conversation: it keeps the draft and re-captures the current selection
as the target for the next message, and an earlier message's recorded target never
overwrites that capture. The header reads `Operator · pi` before a model is
selected and `Operator · pi · provider/id` afterwards.
The **Reasoning** header control and `Ctrl-t` open the model's supported levels
plus `default`. Up/Down selects, Enter saves `operator.thinking_level`, and Esc
closes. The header shows Pi's actual level, `(set: X)` for a substituted request,
and `→ X next turn` when the saved choice changes during a running turn.

The section hides with the sidebar and shrinks or disappears as a short window
needs room for the tree. The `o` route still works. There is no New,
switch or close action: pi compacts the one conversation when it needs room.
`Ctrl-l` toggles the ordinary Projects sidebar and never reveals a conversation
list. At widths below 100, the conversation opens full-width by default; reveal
the sidebar on demand. Below 60×16, resize guidance preserves the draft.
`Ctrl-x`, or `Ctrl-c` in an empty full composer, stops the exact
current turn; a busy or stop-unconfirmed conversation keeps a Send draft instead of
queuing it. Legacy Operator conversations stay on disk but do not appear in the
Dashboard or the Archived palette.

See [Operator setup and controls](operator.md).

## Browse keys

The sidebar's quota section at the bottom, after the project tree, summarizes remaining Claude, Codex, Grok and Cursor account
allowance, as in `Claude 98% · Codex 38% · Grok 95%`: each provider shows the
least allowance left in any of its general windows. The number is red at 5% or
less and yellow at 20% or less. A stale value is muted. A provider with no value
yet (still checking, failed, or every window past its reset) shows a muted
`Claude ?`. Providers switched off are left out, and providers that do not fit
give way from the end to `…`. Click the line, or press `u`, for the windows,
states, ages and next checks in the [details](provider-quota.md). That page also
covers native sources, collection settings, freshness and reset behavior. The
line compacts or hides when the sidebar is short so the tree keeps room; sidebar
and pane dividers remain
draggable over their full height.

The palette offers **Refresh quota**, which asks the Server to read Codex and
Grok now, Cursor while `quota.cursor.dashboard` is on, and Claude too while `quota.claude.probe` is on (a refusal inside the
Server's 30-second cooldown shows the remaining seconds in the footer), and,
while collection is off, **Enable native account usage**, which asks the Server
to set `quota.enabled = true`. The Dashboard changes nothing itself; the rows
change when the Server republishes. The Claude probe is a separate consent,
`quota.claude.probe`, off until set; see [provider allowance](provider-quota.md).

| Key | Action |
| --- | --- |
| `j` / `k` / Down / Up | Select the next or previous visible sidebar row |
| `i` | Select the next session that [needs you](#sidebar-and-status), in the focused pane |
| `b` | Hide or show the sidebar; the panes take its width, and `j` / `k` still move the selection |
| `m` | Switch between the project tree and recent Agent activity; the Dashboard saves the choice through the Server |
| `Enter` | Collapse or expand the selected project or workspace; send terminal input when a session is selected |
| `v` / `V` | Split the focused pane to the right / down, then choose its session; `Esc` keeps the suggestion |
| `<` / `>` | Make the focused pane two columns narrower / wider; the neighboring pane changes by the same amount |
| `-` / `+` / `=` | Make the focused pane one row shorter / taller; `+` and `=` are aliases |
| `z` | Zoom the focused pane to fill the pane area, or restore the layout |
| `Tab` / `Shift-Tab` | Focus the next / previous pane |
| `x` | Close the focused pane; its session keeps running |
| `n` | Open New agent session |
| `w` | Create a branch (form) |
| `a` | Register a project (form) |
| `X` | Archive the selected terminal; confirms before stopping live work |
| `p` / `r` | Pause or resume the selected session; no confirmation |
| `R` | Mark the selected terminal's displayed unread Ready observation reviewed |
| `[` | Freeze the current screen for copying |
| `PageUp` | Open the session's retained history |
| `t` or `Ctrl-t` | Open scheduled tasks |
| `s` | Search other running agents across all projects and workspaces |
| `Ctrl-g` | Label at most the first nine visible running or retained Agent rows. Then press a shown digit `1`–`9` to select immediately. Press `Ctrl-g` again or `Esc` to close without choosing; any other key closes the labels and keeps its ordinary handling. Further visible rows stay unlabelled; scroll and press `Ctrl-g` again, or use `s` Agents search. `Ctrl-g` never reveals a hidden sidebar. |
| `u` | Open [provider allowance details](provider-quota.md), including when the sidebar is hidden |
| `:` | Open the command palette (footer `: Commands`) |
| `Space` | Show contextual groups, then choose an action; `Space s` opens pane actions |
| `?` | Browse the same popup with arrows and `Enter` |
| `N` | Toggle the Server-saved desktop notification setting; also in Settings |
| `O` | Check unresolved macOS notification permission or open Settings when denied |
| `S` | Toggle the Server-saved ready sound setting; also in Settings |
| `q` | Detach; the server and every session keep running |

`N`, `O`, `S`, `R`, `b`, `i` and `[` act on a key press; a key repeat or release does nothing.
`Alt` and `Shift` do not change which action a key runs: `Alt-x` closes the pane
that `x` closes. Command/Super remains distinct from Control and never opens the
session-jump picker. A bare digit `1`–`9` chooses a labeled Agent while the picker
is open; every other key closes the picker and keeps its ordinary Dashboard,
Operator, or program handling. `Ctrl-t` reaches Browse with optional Alt or Shift.
`Esc` and `Ctrl-g` cancel a history request that has not opened yet.

The session-jump picker works in every terminal: pressing `Ctrl-g` captures the
visible Agent rows, draws their labels, and leaves the picker open until a digit
selects or `Esc`/`Ctrl-g` closes it. Labels appear only while the sidebar is
already visible; `Ctrl-g` never reveals a hidden sidebar. In Operator, `Ctrl-g`
keeps its ordinary meaning of leaving Operator, then labels the Browse sidebar
if it is visible. The fixed Operator navigation row and its conversation never
receive Agent labels.

## Search other running agents

Press lowercase `s` in Browse, `Space v s` from the View menu, or sequentially
`Ctrl-g` then `s` while typing into a terminal. When space permits, the Terminal
footer shows this sequence and `Ctrl-o` for Quick message after it. Uppercase
`S` still toggles ready sound; `:` still opens the full command palette.

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
sidebar shows the Ready checkmark, the attention strip counts it as ready, and the pane
header adds `unread reply` after the state (`Ready · unread reply`). Mark reviewed clears
all of those marks. The Agent's reported activity stays Ready, so the header still says
`Ready`. A new Busy report does not clear an earlier Unread response; the header then
reads `Working · unread reply`.
Selecting a terminal, viewing its output or receiving a notification does not acknowledge it.

Press `R` in Browse mode, or choose **Mark reviewed** in the terminal action
menu or command palette. The action names the unread observation displayed by
the dashboard. If a newer Ready arrives first, the server rejects the stale
acknowledgement and preserves the newer unread response. Refresh your view and
review that response before acknowledging it. Repeating a completed
acknowledgement is safe.

Replying reviews it too. The first keystroke or paste you send to a session
whose unread response the dashboard displays sends the same request `R` sends,
for that displayed observation, ahead of the input; later keys of the reply
send nothing more. A stale refusal stays out of the footer: the newer response
it protects stays unread until you reply after seeing it, or press `R`.
Selecting, viewing, focusing a pane for typing and mouse input forwarded to the
application do not review.

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
first. The pane header says `Waiting for input`, or `Waiting for approval` when the
oldest one is a tool approval.

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
- Full-screen applications on the alternate screen cannot supply terminal History.
  Claude Code and Codex can open their provider-owned [Transcript](#transcript-keys)
  instead. Without a readable conversation, PageUp gives a muted explanation and
  wheel-up stays quiet.
- An application that erases the saved lines (`ESC[3J`) empties History, as a
  terminal's scrollback would be. `clear` sends it, and Pi and Oh My Pi send it
  before every full repaint (a resize, a new or resumed session), then write
  their transcript again, so History holds that transcript once.
- Retained history lives in memory with the session and is lost when the server
  exits. It is not persisted to disk.

### Clipboard

Clipboard requests use OSC52 and need permission from the host terminal,
including any intervening multiplexer. The limit is 65,536 UTF-8 bytes. The
notice `Clipboard request sent; paste to verify` confirms only that the request
was written, so verify it by pasting in the destination application. Terminals
that disable or lack OSC52 can ignore the request silently. Selections remain
available after clipboard or size errors. Soft-wrapped rows join without added
newlines; hard line breaks, spaces inside a line, and stored Unicode text are
preserved. Spaces at the end of each copied line are dropped, as terminals do:
Oh My Pi pads every row to the pane width.

## Transcript keys

On full-screen Claude Code or Codex, PageUp or wheel-up opens a read-only view of
the session's recorded conversation: user prompts, assistant text, and a short
line for each tool call. Inline agents such as Pi and Oh My Pi keep terminal
History. Reading Transcript does not write the provider file or replay commands.

Up/Down or `k`/`j` scroll one row; PageUp/PageDown scroll a viewport; the wheel
moves three rows. Scrolling to the top loads earlier messages. Home moves toward
the loaded beginning; End reloads the newest page. Esc, `q`, or Ctrl-g closes it.
Keyboard, paste, and mouse input stay in the view while loading and reading.
Changing the session run closes it; pages from another conversation cannot be
joined to the current view.

The view keeps at most 1 MiB or 16,384 entries, dropping newest entries as you
page further back; End returns to the latest messages. It does not update live.
Individual JSONL records over 512 KiB are skipped, prose is shortened after
16,384 characters, and tool input summaries after 160 characters. Tool results,
private reasoning, and image data are omitted. A readable file whose entries are
all filtered or oversized opens an empty Transcript view. Unsupported providers,
missing conversation references, and unreadable files keep PageUp's muted History
explanation and leave wheel-up quiet.

## Panes

`v` splits the focused pane to the right; `V` splits it down. Each opens a
session chooser with the next different visible session highlighted. `Enter`
shows the highlighted session in the new pane, typing filters by project,
branch and title, and `Esc` keeps the suggestion. The chooser omits sessions
already assigned to another pane. A layout can contain up to four panes, each
at least 20 columns wide and 8 rows tall including its header. The footer gives
the reason when another split cannot fit or the four-pane limit is reached.
Drag any divider to resize its neighboring panes; click a pane to focus it.
In Browse, `<` and `>` move the nearest left-right divider around the focused
pane by two columns, and `-`, `+`, or `=` move its nearest top-bottom divider
by one row. Repeated key events repeat that step. A pane stays at least 20
columns wide and 8 rows tall including its header; the key does nothing at the
minimum, and the Panes group explains when no matching divider exists. These
keys are also available in `Space s`. Double-click a pane header to zoom or
restore the layout, double-click a divider to make that split even, or
right-click a pane to focus it and open the Panes group.
`Tab` and `Shift-Tab` cycle through panes. Sidebar navigation replaces the
focused pane's session; selecting a session already shown elsewhere moves focus
to that pane instead. `x` closes the focused pane and gives its space to its
sibling, leaving the session running. If the window becomes too small for the
layout, only the focused pane is drawn and its header says `split hidden`;
widening the window restores the full layout. A new attachment starts with one
pane. Each pane is capped at 1000 rows and 1000 columns; a `SetView` requesting
a larger pane is rejected before any PTY resize.

`Space s` opens the **Panes** group. `h`, `j`, `k`, and `l` focus the pane to
the left, below, above, or right; when several panes are in that direction,
the one with the greatest shared edge is chosen, then the nearer pane breaks a
tie. `H`, `J`, `K`, and `L` swap with that neighbour while focus follows the
same session. These directional keys work in the group; in Browse, arrows and
`j` / `k` continue to navigate the sidebar. `z` in Browse or in the Panes group
shows only the focused pane and marks its header `zoomed`; the tree and other
assignments stay in place. Press `z` again to restore the tree. `Tab` and
`Shift-Tab` change the focused pane without ending zoom, while splitting or
closing a pane restores the tree.

## Sidebar and status

The attention line leads when any session needs you. The three-row **OPERATOR**
section follows it, then the project tree and quota. Each project is a section:
its name in upper case with a rule to the sidebar edge, and a blank line before
every project after the first. Workspaces follow
with a branch glyph and the current branch name in bold. The repository-root
workspace also shows a `root` marker. A warning marker appears when that
checkout is not on the default branch. Duplicate visible names include the
worktree path. A branch change updates the label in place; selection and panes
stay on that workspace. Folded project and workspace rows show counts before
their fold marker: `?N` sessions needing input and `✓N` ready Unread responses.
A project row shows `[+]` at its right edge while hovered. A hovered workspace row
shows `[+]` just before `[x]`. The project menu offers **New workspace** and
**New agent session**; the workspace menu offers **New agent session** and
**New terminal**. Each opens its existing form with that row's context. Use Up
and Down to move, Enter to choose, or click an entry; Esc closes the menu. A
launch action that cannot run is dimmed with its reason. The root workspace can
still open the Agent branch picker, where its unavailable branch remains
guarded.
A session row starts with a status
glyph and the session name, with the agent harness (from session kind for agents, else the label) right-aligned in the
provider colour. When a model is known, it follows a colon in the quiet gray,
as in `pi:grok-4.7`. A model that repeats the agent name drops that repeat, and a
missing model leaves the agent name alone. On a shared line the name keeps its
room first: when the whole name and label do not fit, the model goes
(`claude:opus-5-5` becomes `claude`), then the agent, and only then does the name
clip with `…`. The label shares the name's line only while every visible `agent:model`
fits beside at least twelve cells of its name. As soon as one does not, the
sidebar stacks as a whole: every session with an agent label keeps its name on
one line and moves the label to one extra line under it, hung by a `└`
connector two cells in from the status glyph, under the start of the name, and
clipped with `…` if even that line is too narrow, while local shells stay on one line. A stacked
label keeps its model, and no two rows use different layouts at the same width. The extra
line belongs to the same session: `j`, `k` and the mouse treat both lines as one
row, the selection bar and background cover both, and the hover `[x]` sits on
the first line. Local shells show `$ local` on one line with no agent label
unless a hook reports activity inside them. The
selected row shows a mauve bar in its first column and a lighter background; its
own colours stay visible. A folded project shows `▸ N ws` and a folded workspace
`▸ N` at the right edge.

Press `m` to switch to **Recent activity**; press it again to return to the tree.
The saved `sidebar_view` setting defaults to `tree`. Recent activity lists only
projects with Agents and only their Agent sessions, sorted newest first within
each alphabetically ordered project. The age uses Last activity and falls back
to the session start time. Each row always takes two lines: status, title and age,
then the branch and harness name. Both lines keep the same selection, hover,
close button and click behavior as one row. Project folds hide the Agent rows
and retain their Input and Unread counts; hovering a project still shows `[+]`.

`b` hides the sidebar and the panes take the full width; `b` again shows it at
the width it had before. The choice lasts for the attachment and is not saved.
While the sidebar is hidden, `j` and `k` still move the selection, and
`Space v`, `?` and the palette offer **Show sidebar**.

The **OPERATOR** section shows whether the conversation is working, unread,
idle, not yet opened, or not set up, plus its current activity or latest reply.
It omits the model and harness. Click anywhere in the section or press `o` in
Browse to open the conversation. The `operator.sidebar` setting controls this
section and is on by default. A short sidebar drops section gaps, compacts
Resources, quota and Operator, and hides them as needed to preserve room for the
tree; `o` still opens Operator when its section is hidden.

**Resources** follows the project tree and shows whole-machine CPU use and
memory in use over installed memory. It samples on the Dashboard's existing
redraw tick, at most every two seconds. CPU stays `…` until two samples show
advancing counters; a failed reading says `unavailable` instead of keeping an
older value. The one-line form keeps both values. Set `resources.sidebar = false`
to hide the section and stop machine reads; it is on by default and changes
apply while the Dashboard is running.

The **QUOTA LEFT** section shows colored bars for each provider’s allowance windows.
Short windows use compact provider summaries, then a `Quota: u` pointer. Press
`u` or click the section for details.

While any session needs you, a line at the top of the sidebar, above Operator,
says how many: `? 1 needs input · ✓ 2 ready` (`? 2 need input`
for more than one), or `? 1 · ✓ 2` when the words do not fit. The attention
line stays at the top and is never hidden while counts are nonzero. It counts every project,
including folded and scrolled-away rows and the focused session, and lists needs
input first, the order the jump visits. Needs input means a running session
waiting for an answer (including an open [input request](#input-requests)). A
paused session does not count as needing input: it cannot take the answer until
`r` resumes it. Ready means an [unread response](#unread-responses) from a session
that is not busy again; a busy session's older response waits for the turn in
progress, as its spinner says, and still shows Unread in its pane header. A
session that is both counts once, as needs input. The Operator conversation stays
out; its section shows its unread reply. The line disappears when
nothing is left, and stays visible while the sidebar is short.

`i` in Browse (or `Ctrl-g` then `i` while typing), `Space v i`, the palette, or a
click on that line selects the next session that needs you and shows it in the
focused pane, unfolding its project and workspace. Sessions waiting for input
come first, then unread responses, each in sidebar order after the current
selection, wrapping; pressing again from one of them moves to the next, so
repeated presses visit every one. With nothing to visit, `i` says so in the
footer.

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

An unavailable reporter draws the glyph in the muted colour. The state in words
appears in the [pane header](#pane-header), not in the sidebar.

### Pane header

Each pane has a two-line header. The first line reads, left to right: the
session title, the agent and model (`claude:opus-5-5`; `shell` for a terminal
without an agent label), the state in words and the elapsed time, for example
`verify spinner  claude:opus-5-5  Working · 2m` or `local  shell · 16m`. The
process id follows, muted, at the far right while the line has room. With a project
or workspace row selected, the header names it (`consigint / feature/x`) above its card.

| State | Meaning |
| --- | --- |
| `Working` | Busy |
| `Waiting for input` | Waiting for you, including an open [input request](#input-requests); `Waiting for approval` for a tool approval. The question itself never appears. |
| `Ready` | The response is done; `Ready · unread reply` until you [mark it reviewed](#unread-responses) |
| `Idle` | Idle |
| `Error` | Reported error |
| `Paused`, `Exited`, `Stopped`, `Interrupted` | The process phase, which outranks the activity |

A terminal that reports nothing shows no state. Reporting health appears only
when it matters, muted after the state: `starting…` while an agent has not
reported yet (for up to a minute after launch), and `status off` when an agent's
reporting is lost (even before its first report), was never set up, or is not
supported, or when an Unread reply has no reporter left behind it. Report quality (observed, confirmed) and
the reporter's own reasons stay out of the header; `ovrcr terminal list --json`
keeps them. Recovery and control explanations follow the state after ` · `.

A narrow pane gives up the process id first, then the model, the elapsed time,
the agent label, and then clips the title, so the state stays readable. Narrower
still, a bare `Unread` takes the state's place (`Unread · status off`), since the
sidebar glyph still shows the activity.

The second line shows the agent's metrics when it reports them:
`ctx 6% · 417k tokens · ~$0.37` — context used, tokens in and out, and cost, `~`
marking an estimate. Unknown values are left out. A value older than five
minutes, or any value once the process has exited, is muted and followed by its
own age, such as `417k tokens (12m ago)`. A narrow pane drops the ages first,
then values from the right. Without metrics the second line is a plain rule; in
History it shows the History position instead. Split panes carry the same words
on one colour band, `> ` marking the focused pane and `loading` until its screen
arrives.

Terminal output, elapsed silence, keyboard input, and process liveness do not
imply that an agent is busy or idle. See [agent reporting](agent-reporting.md)
for provider reporting behavior.

## Mouse

In Browse and Terminal modes:

- Hovering a project or workspace row shows `[+]`; the workspace keeps `[x]`
  immediately to its right. Click `[+]` for that row's creation menu.
- Clicking a session row selects it. `[x]` shows only while the pointer is over a session or workspace row. On a session it replaces the agent label and archives that session, the same as `X`, without switching the pane. A live or paused session asks first. An exited session archives immediately. On a workspace it opens Remove workspace for that row and does not clear the pane. A root workspace refuses, as the key does.
- Clicking a project or workspace name selects it. Clicking the first cell of a
  project header, or the branch glyph of a workspace, folds or unfolds it. The
  blank line before a project does nothing.
- Scrolling over the sidebar scrolls its visible rows.
- Clicking the attention line above the quota section does what `i` does;
  clicking the quota section at the bottom opens the quota details, as `u` does.
- Clicking inside a terminal focuses that pane for typing. A click that changes
  focus is consumed by the dashboard.
- Double-clicking a pane header zooms or restores that pane; double-clicking a
  divider sets its split to an even ratio. Right-clicking a pane focuses it and
  opens the Panes group in Browse. Terminal mouse input remains forwarded to
  programs that request it.
- Dragging the divider between two panes changes their widths. Each visible pane
  keeps at least 20 columns. The split proportion survives window resizing and
  the temporary single-pane layout used in narrow windows.
- Dragging the sidebar's right border changes the sidebar width, between 20
  columns and half the window; the panes take the remaining width and are
  resized. The chosen width lasts for the attachment and is not saved. A
  hidden sidebar has no border to drag, and clicks in its former area reach
  the pane.
- Wheel-up over a pane opens History at the captured tail. Further wheel ticks
  move three rows (keys move one); wheel-down at the newest row returns to the
  live pane.
  On full-screen Claude Code or Codex, wheel-up opens Transcript when its recorded
  conversation is available; other full-screen applications stay unchanged.

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

The `Ctrl-g` session-jump picker labels at most the first nine drawn running or
retained Agent rows, including the current session, in sidebar order and freezes
each target's session and run while the picker is open. The labels are always
`1`–`9`; a bare `1`–`9` selects immediately, and `Ctrl+1`–`Ctrl+9` remains an
accepted spelling. `0` and other unmapped digits show a retry hint without
selecting a session or sending program input. Further rows stay visible and
unlabelled; scroll and press `Ctrl-g` again to label the new first nine, or use
`s` Agents search. Pressing `Ctrl-g` again without choosing closes the picker;
the key keeps its ordinary Browse meaning elsewhere. After choosing a running
Agent, it stays in Browse until the exact view is acknowledged and the input
boundary is drained; queued loading input is discarded. Any other key closes the
picker and keeps its ordinary Dashboard, Operator, or program handling. A
retained Agent stays in Browse for inspection and is never resumed or reopened
by navigation; use its explicit Resume/Reopen action when needed. Command/Super
shortcuts remain separate.

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
`v` View, and `s` Panes. Only groups relevant to the selected row appear. The
Panes group lists split, directional focus, swap, resize, next pane, zoom and
close. A resize action explains when the focused pane has no divider in that
direction. Terminal rows
also expose their parent workspace and project. `a` registers a project and
`q` detaches from the top level, including when the hierarchy is empty. Nothing
else sits beside the groups: desktop notifications and the ready sound are
settings, toggled in Settings or with their Browse keys `N` and `S`.

Choose a group, then an action: `Space t X` closes the selected terminal,
`Space w X` removes its workspace, and `Space p X` unregisters its project.
Every destructive group action is uppercase. Lowercase `x` only ever closes a
pane: in Browse and in `Space v` it hides the focused pane and the session keeps
running; in the other groups it does nothing.
Workspace and project removal open a searchable picker with the current target
highlighted; Enter accepts it and opens confirmation. Closing a live terminal opens
confirmation directly. Existing server removal safeguards
still apply; repositories and workspace branches are retained. Confirmed workspace
removal archives stopped records with their original paths. When the workspace
still has running sessions, the confirmation names them by title
(`feature/status-check still has 1 running session: verify spinner. Stop it and
remove the workspace?`); confirming closes each one, one at a time, and then
removes the workspace. If a session refuses to stop, the confirmation returns
saying which one and nothing is removed. The sessions are stopped before the
Server checks the worktree, so the uncommitted-changes question below comes after
they have stopped. If the Dashboard detaches partway through, the sessions
already stopped stay stopped and the workspace stays; remove it again to finish.
A stopped session whose processes may still be running (ownership uncertain)
blocks removal. When the Dashboard can see one, the confirmation names it first
and stops nothing: choose "Acknowledge stopped" for it, then remove again.
Archived ones and active task runs also block, even though the Dashboard cannot
see them beforehand; the refusal says so in plain words and the Archived sessions
page offers "Acknowledge stopped" for archived rows. No forced removal is offered
for them. A removal refused because the worktree is
dirty asks "Save uncommitted work to origin/wip/<branch>?". Enter commits the
unignored files onto that ref, pushes it to `origin`, and then removes the
workspace; the checkout's own branch is unchanged. Escape leaves the workspace
in place. A checkout that cannot be saved that way, such as a detached HEAD,
asks instead whether to discard the uncommitted changes and remove it; that is
the only forced removal the Dashboard offers. The repository-root workspace is
never offered the save. With `save_uncommitted_work` on, removal and server shutdown push
those refs without asking. The repository-root workspace cannot be removed. Remove every
other workspace first, stop or acknowledge remaining sessions, then unregister
the project; repository files stay on disk. Archived context is kept. `Space w n`
opens New agent session, `Space w t` creates a terminal, `Space p n` creates a
branch, `Space v t` opens tasks,
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

Session rows read `<project> / <branch> / <title>`. Two sessions that would read
the same also show their ID, `(#id)`, which clipping keeps visible. Long text
ends with an ellipsis; filtering still searches the full identity, including a
session's ID (`#7`) that is not drawn.

`:` in Browse, or `Ctrl-g` then `:` from a terminal, opens the palette. If the
dashboard has detached it reconnects first. Type to filter actions or to search a
terminal by project, branch, and title. Arrows select a result, `Enter`
continues, and `Esc` cancels.

Entries are, in order: `New agent session (n)`, `Create terminal`, `Create branch (w)`,
`Register project (a)`, the selected session's actions (`Rename terminal`,
resume or reopen, `Close terminal`), `Remove workspace`, `Remove project`,
Settings, Events, quota actions, `Archived sessions`, one row per session, and
the remaining key-popup actions by name, including `New terminal`. The palette follows the key popup's
rule: it drops actions the current target cannot use, and keeps one that is
waiting for the pane's screen or blocked (such as `Create terminal` while the
root workspace is off its default branch) with the reason below it, so rows do
not shift under the cursor when the action becomes available.

Searching also finds every setting by its label or its `dashboard.toml` key, as
`Settings: <label>` with the group and key below it (`theme`, `notifications`,
`sound`, `branch_prefix`, …). `Enter` opens Settings on that row. Setting rows
appear only for a search, not in the unfiltered list.

Within a form, `Enter` submits from any field, the same as the Submit control.
In **New agent session**, `Enter` opens or accepts a Branch, Agent harness or
Model choice and keeps the form open; use `Alt-Enter` to start. The multiline
prompt uses `Enter` for a newline and `Alt-Enter` to start. In the child
**Create branch** form, `Enter` opens or accepts the Project choice and keeps
the form open; use `Alt-Enter` to create. Other forms keep their existing Enter
submission behavior, including the explicit confirmation shown by removal
forms.
It first checks the whole form: a required field left empty, a choice that is
not in its list, or a branch Git would refuse keeps the form open, moves the
focus to the first such field and says what is wrong just below it
(`Add a name after feature/`, `Branch names can't contain spaces`). `Tab` and
`Shift-Tab` move between fields; on a text field Up and Down do too. On a pick
list, Up and Down change the value at once, Left and Right step through the
choices and wrap at the ends, and typing filters the list; `Ctrl-u` clears the
filter. The open list shows as many choices as fit, all of them when there is
room, and always the current one. On a text field Left/Right and Home/End move
the text cursor; Backspace and Delete remove text beside it, and `Ctrl-u` clears
it. The footer lists only the keys that work for the form and its focused field.
Click a field to focus it and position its cursor, click options or
toggles to choose them, and use the wheel to scroll lists and long forms.
Input stays in the palette while it is open.

The New agent session and Create branch dialogs need at least 60 columns by
18 rows. Below that size, the draft stays open with resize guidance; edits and
submission wait until the window is large enough, and Escape still cancels.

Creating a workspace or terminal and removing a workspace run as Lifecycle jobs:
the form steps aside and the sidebar shows `Creating…` or `Removing…` while the
Server works. If the job fails before anything was created, the form comes back
with your values and the reason in plain words (`Couldn't create the workspace:
a branch named 'feature/x' already exists`); no failed row is left behind. When
the form cannot come back without taking keys meant for something else, such as
a terminal you are typing in, a short failed row stays (`Not created · Esc
dismiss`) and the footer gives the reason; one Esc clears both.

Click a search result to open it, or select it and click Open. Confirmations
have separate Confirm and Cancel controls; clicking outside an open palette
does not activate the terminal behind it.

All existing-project and workspace fields use the same searchable picker,
including removal forms and the scheduled-task editor. In palette forms, type to
filter and use Up/Down to choose; the scheduled-task editor keeps Tab or Enter
to accept. Workspace choices show the
project and current branch, with the worktree path when those names collide.
Removal excludes the protected repository-root workspace. Session names and
project names remain text fields, and project registration retains its path pickers.

Closing live work and deleting records show their current branch target and require confirmation.
Closing an exited row archives it immediately. Removing a workspace with running
sessions names them and stops them first, as described under the key popup.
Ownership-uncertain stopped sessions and active task runs still block removal,
and no forced removal is offered for them. A dirty worktree asks whether to
save that work to `origin/wip/<branch>` first; declining leaves the workspace
where it is. A checkout that cannot be saved there asks whether to discard the
changes instead. Errors stay in the palette with the form values retained, in
plain words: no error codes, branch names instead of internal workspace ids, and
sessions by title.

### New agent session (`n`)

Choose a registered checkout by its current branch in **Branch**, then choose an
**Agent harness**, **Model**, and optional **Initial prompt**. Workspaces are
grouped by project; the picker searches the displayed project and branch labels
and excludes retained sessions without a registered checkout. It follows the
same registered checkout when its branch label changes. Unavailable checkouts
remain visible with their state; root and detached destinations are labeled.
An eligible current feature checkout is preselected. If the current target is
missing or cannot launch, the form keeps its identity when registered and asks
you to choose deliberately; it never substitutes the first listed checkout.
Start checks the selected checkout again, including its registration, path,
Git identity and root protection. Register a project with `a` when no project
is available.

**Create new branch…** opens the branch-only child form. The parent session
form stays suspended with its prompt, destination, focus, and explicit harness
or model choices. `Esc` from the unsubmitted child returns to that exact form.
After a successful, identity-matched branch readback, the new checkout becomes
the selected destination; the Agent still starts only when you submit the
parent form.

The picker opens even when the current workspace cannot launch. This does not
change the separate **Create terminal** form or its existing root-workspace
guard. The form shows a compact destination line and, when another agent is
already using that checkout, a compact shared-files notice. Harness and model
defaults can follow the selected project; explicit choices, the prompt and
advanced name stay intact when changing destinations. **Advanced options** is
collapsed and contains the optional session name.

Choice lists stay collapsed while you move through the form. Press `Enter` or
click a choice field to open its list; while it is open, `Enter` accepts the
highlighted choice and keeps the form open. `Tab` moves between fields without
accepting or changing untouched defaults. `Esc` closes an open list first, then
cancels the form. Use `Alt-Enter` or the visible Start control to launch. The Initial
prompt is multiline: `Enter` adds a line and `Alt-Enter` submits. The prompt is
sent through a supported harness mechanism and is not used as the session name
or remembered launch setting. Harnesses without verified model or prompt
support stay available with that limitation shown in the form.

### Create terminal

Choose **Agent** or **Terminal** in Start. Agent offers installed agents and named
`[[agents]]` presets. Terminal opens your default shell; its optional Command
runs through `/bin/sh -lc`. Custom commands belong to Terminal, even when the
command happens to invoke an agent. A launch choice does not guarantee agent
reporting support.

Workspace defaults to the selected workspace. Name is optional: leave it blank
for a stable workspace-based name, or enter your own name. After creation, typing goes to the new
session immediately.

The generic **Create terminal** command keeps the existing **Start** choice.
For Agent, its optional **Initial prompt** is sent only through a supported,
current detected harness; it does not supply the session name. For Terminal, the
prompt and Agent fields are hidden and the optional **Command** runs through
`/bin/sh -lc`. Custom commands belong to Terminal, even when they invoke an
agent. The prompt is one-off and is never replayed by retry, reopen, resume, or
server-restart recovery. **New terminal** in `Space w t` opens this same form
with Terminal already selected, so shell creation does not require changing
Start. A selected root workspace with a warning blocks terminal creation;
`n` still opens New agent session so you can choose an eligible checkout.
Detected `claude`, `codex`, `pi`, `omp`, `grok`, `hermes` and `cursor-agent`
entries launch through the managed `agent run` route; an
`agents` override replaces the command entirely. Detected Claude, Codex, Pi, and Oh My Pi
launches auto-trust by default: the argv gains `--dangerously-skip-permissions`,
`--approve-for-me`, `--approve`, or `--auto-approve` when it does not already contain a
permission-mode, approve, auto-approve, approve-for-me, full-auto, or dangerously-* flag. A flag already
in that family is left as written; nothing is appended or replaced, and there is no
global switch. `ovrcr agent run` does the same when its native argv is bare, including
a `[[agents]]` preset that is itself a bare `agent run`. A preset that already names one
of those flags is stored and launched as written; a raw command that does not go through
`agent run` is not rewritten. Grok, Hermes, and Cursor are not given a flag. This
replaces the earlier non-goal that managed launches would not add a trust bypass. Claude, Codex, Pi and Oh My Pi
deliver Ready/Unread on their supported readiness paths (Claude Ready is Observed,
never Confirmed settling). Claude/Codex Input-request support remains unavailable
until their dependent tickets. Missing hook configuration shows a one-time footer
note such as `Claude and Codex status off — :connect to set up`, never an error;
the palette's **Connect agent status**, listed until it succeeds, opens a
confirmation page with the startup screen's text: what connecting does, the files it
adds or updates and the Codex trust line. Enter installs the hooks and shows the
result line (with any manual-migration note); Esc closes it and writes nothing. When
nothing needs writing the result shows at once. A failure stays in the footer and
keeps the entry.
Observed reporting loss shows `status off` in the pane header, and the terminal's JSON
row keeps the published reason.

The optional **Initial prompt** is a separate multiline field. Leave it blank to
open the detected Claude Code, Codex, Pi, or Oh My Pi launcher interactively. A
nonblank prompt is passed to one of those exact detected launchers as a native
positional argument after `--`. It is transient launch input, not a session name
or branch: OVRCR does not retain it in the session's base argv or metadata, or
add it to the remembered launch choice. Configured launcher overrides keep
their stored arguments and cannot use this prompt path.

The **Model** field starts at **Harness default**, which leaves model choice to
the native launcher. Detected Claude Code, Codex, Pi, and Oh My Pi launches can
offer current catalog choices for a registered workspace. OVRCR checks the
launch binding and rereads the catalog before Start, refusing a changed or
missing choice. Configured and cloud launchers keep their existing model policy.
Pi choices use exact `provider/model` IDs; Pi's native `--model` resolver can
still fuzzy-match or synthesize a model if its catalog changes after that final
check, and Pi has no exact-only CLI flag. A catalog listing or accepted process
start does not prove that the provider used or accepted the chosen model.

Hermes uses the same process supervisor with native arguments unchanged. Its row
shows unknown activity; reporting, Ready/Unread, Input requests, metrics, generated
titles and recovery remain unavailable. See [Hermes harness](hermes-harness.md) for
setup/doctor guidance and the distinction between source review and native acceptance.

Cursor CLI validates fresh startup identity through a temporary local plugin when
the installed CLI advertises support at runtime. Activity remains
unknown; Ready/Unread, Input, metrics, generated titles and recovery are unavailable.
Resume, unavailable plugin support and unsupported options remain native with
reporting unavailable.
See [Cursor harness](cursor-harness.md) for the runtime capability check and passive plugin contract.

### Create branch (`w`)

`w` opens a branch-only form. Project starts at the current project and can be
changed. Branch name starts empty; `feature/auth-refresh` is a placeholder, not
a submitted value. **Workspace options** is collapsed by default and contains
Base. The detected local default branch fills Base when one is found. Detection
uses the repository's local Git state, including `origin/HEAD` when available;
it does not fetch or assume `main`. If no default can be resolved, Base stays
empty until a valid local branch or detected default is entered.

Git suggestions load in the background and time out after two seconds. A branch
name must pass `git check-ref-format --branch` and must not already exist in the
repository. Base must resolve to a local branch or the detected default. While
suggestions are pending, Submit waits; if the lookup fails, the form explains
why it cannot validate a Base. No workspace is created until both names are
valid and resolved.

The worktree folder under the project root is derived from the branch name:
`feature/auth-refresh` becomes `feature-auth-refresh`. Workspace creation uses
the Server's automatic local-terminal setting. It never starts an Agent from
this form. From New agent session, the child hides the parent form while keeping
its prompt, branch choice, focus, and explicit harness/model choices; after a
correlated successful receipt and matching fresh registry/hierarchy readback,
the new branch is selected and the parent form returns. `Esc` before Submit
restores that parent form. While creation or readback is pending, the child
stays visible and cannot be dismissed or submitted again.

When `w` is used on its own, a verified branch returns to Browse with its
workspace selected, even if the Server also created a local terminal. A normal
`n` start submits the Agent only after the parent form is confirmed. `Create
terminal` remains a separate command for an already registered workspace.

If creation fails before a workspace exists, the child keeps its branch and
Base values for correction and retry with the same workspace identity. After a
partial failure, the Dashboard performs a fresh Inspect and List before it
allows another attempt; an uncertain or already-registered identity blocks
retry to avoid creating a duplicate checkout.

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
non-secret configuration references. Missing history, executable or matching
configuration opens Claude's native resume picker instead of failing; the UUID
stays retained, so a later Retry still attempts the exact conversation. A missing
recorded working directory still blocks with a diagnostic because the shell cannot
start. An observed unsupported or ambiguous Claude
conversation transition (`identity_transition_unavailable`, including bare fork
without a leave of the current conversation) permanently disables exact recovery
for that row, retaining its old UUID as context; an explicit reopen still offers
Claude's native picker. Supported clear / in-process resume /
foreground-branch replacements update the retained reference instead.
Absent callbacks or temporary reporting failures alone do not erase that UUID.
Inventory restoration never launches a process. First display in a visible pane
requests one native resume for an unarchived interrupted Agent with an available
adapter and no previous failure. A verified different boot permits automatic
recovery; same-boot or unknown ownership still requires explicit confirmation
that old agents and descendants stopped. Live runs are reused. Shells, deliberately
stopped/exited rows and newly unarchived rows do not launch automatically.

Full live capacity leaves a diagnostic and **Retry resume conversation** in the
command palette. A reference that cannot be certified opens the provider's native
resume picker (Claude, Codex, Pi and Oh My Pi); providers without a picker keep the
diagnostic. Fix capacity, then explicitly retry; redraws and reconnects do not retry
failed launches. Hidden panes and sidebar rows
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
can cause a native fresh session and is outside the recovery guarantee. A reference
that cannot be validated at reopen opens the provider's native resume picker; the
reason is logged and the reference stays retained for a later exact retry.
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
appends `/`. With nothing left to complete, or once the path names a Git
checkout, `Tab` moves to the next field instead. `Enter` keeps the typed text
and registers the project from any field. An empty field lists
`picker_roots` from `dashboard.toml` in document order, not sorted. Name becomes
the repository basename once a path is chosen. Workspace root defaults to `<config dir>/workspaces/<name>` until
you edit it, and is created on registration if missing. Registration creates the
protected repository-root workspace on the current checkout branch. Whether it
also starts a `local` shell there follows `automatic_local_terminals` (default
**default branch only**); when the root is off the detected default branch,
registration still succeeds and the initial shell is skipped until that branch is
restored. It does not switch Git.

## Empty states

A pane with no session shows a card instead of a blank screen. Cards only
display the hierarchy the Dashboard already has; they send no request and do
not change which keys work or what input a pane accepts. Every key a card
names comes from the same key table as the footer and the menu.

- **No project registered:** `Welcome to OVRCR` with two numbered steps,
  `1  a  Register a repository` and `2  w  Create a branch`,
  then `Space actions · : commands · ? keys`. The instance directory and
  socket paths are at the top of [Events](#events).
- **Workspace row selected:** the pane header names `project / branch`; the
  card shows its path with `~` for your home directory (and `· root` on the
  repository-root workspace), any root warning, and its sessions in sidebar
  order: status glyph, title, `agent:model`, the header's state words
  (`Working`, `Ready · unread reply`, `Waiting for input`, …; a plain shell has
  none) and elapsed time. Then
  the next steps: `j open <session>` while the row below is that workspace's
  first session (`j` selects the next row), `Enter expand…` when the workspace
  is collapsed, `n new agent session`, `w create branch` and `Space w more`.
  A key that is unavailable there is left out. `n` opens the branch-based form
  even when a root warning blocks starting in this checkout; the warning
  remains visible and Start is blocked until a valid destination is chosen.
  Existing sessions stay usable; OVRCR does not switch the checkout.
- **Project row selected:** the pane header names the project; the card shows
  its repository path, its workspace root (read from where its worktrees are,
  so it appears once the project has one), and each workspace with its session
  count and attention in the sidebar strip's words and order:
  `? 1 needs input` for a running agent waiting on you, `✓ 1 ready` for an
  Unread reply. Then
  `n new agent session`, `w create branch` and `a register another project`.
  While the project has no agent yet, the card leads with
  `Next: press n to start your first agent`.
- **Nothing selected,** for example right after registering a project: every
  project's card, each led by its name, then the same two keys.

A card fits its pane. In a narrow pane the model label goes first, then the
agent, and titles and branch names end in `…`, so each row keeps its state
and elapsed time. Project attention counts shorten to `? 1 · ✓ 1` when their
words do not fit. When rows run out, the list ends with `+N more` and the next
steps stay. In a split the pane header says only `no session`, so the card in
the emptied pane names the project or workspace itself; `j` opens the session
in the focused pane, and the emptied pane then shows the projects.

Enter on a project or workspace row still collapses or expands it.

## Dashboard settings

Every setting lives in one settings document, `dashboard.toml` in the instance
directory, including the Server's `title_model`, `automatic_local_terminals`
and `[quota]`. `OVRCR_HOME` chooses that directory, which also holds
`registry.sqlite3`, `tasks/`, `events.jsonl` and `server.sock`. Override the
settings document path with `OVRCR_DASHBOARD_CONFIG`; it chooses a file only.
Remembered launch choices stay in this file. Do not put settings in a preserved
legacy `config.toml`: a `[quota]` table there configures nothing and is reported
as a finding. `OVRCR_CONFIG` remains a deprecated file-path alias for one
release; its parent directory selects the instance and is reported as a finding.

A missing file uses defaults. A symlink at that path whose target does not
exist is not a missing file: it is one document-level finding naming the path.
A wrong-typed value keeps its own default. An invalid explicit
`quota.cursor.state_db` disables the Cursor reader so it cannot select the default
account. A wrong-typed element of a list or
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

Open **Settings** from the menu (`Space v ,`) or search Commands for
**settings**. A Commands shortcut for a setting, such as `theme`, opens its
category and focuses that row. Settings owns the header, content and footer;
closing it restores the workspace and session selection.

The sidebar follows the settings catalog: **Alerts**, **Appearance**,
**Workspaces**, **Titles**, **Usage**, **Agents**, **Remembered launches**,
**Operator**, **Delegation** and **Remote**. Only the active category's settings
are listed. Each setting has one aligned label/value row; `›` marks keyboard
focus without depending on color. The explanation changes in a fixed details
area instead of expanding the selected row.

Details show the effective value, its source and default, the configuration key,
and the document path and read time. An explicit document value remains distinct
from a built-in default even when both are Off. Reset is available only for a
document override and names the affected setting or whole collection. Consent
guidance explains permission, access or cost consequences before activation.
The sound choice also explains when Desktop notifications or Ready sound is off.

At 80 columns the category sidebar stays visible. Below 80 columns, a breadcrumb
and one category index, list or editor use the full width. Resizing preserves
focus, drafts and scroll positions. Below 48 columns or eight rows, Settings
shows resize guidance and accepts only cancel/close controls.

| Key | Action |
| --- | --- |
| `↑`/`↓`, Page Up/Down, Home/End, wheel | Navigate the focused category, list or picker |
| `j` / `k` in categories or lists | Move down/up; in text inputs these remain ordinary letters |
| `Enter` / `Right` in categories | Enter that category's settings |
| `Enter` on a setting | Perform the footer's named action: turn on/off, choose, edit or manage |
| `Left` / `Escape` in a list | Return to categories, or restore the starting point of a search |
| `/` | Search labels and configuration keys across every category |
| `Enter` in global search | Open the highlighted result in its owning category |
| `Escape` in search or an editor | Cancel that layer and restore its previous selection |
| `r` | Reset a document override; collection reset affects the whole collection |
| `Ctrl-d` / `Ctrl-u` | Scroll selected details down/up to reach full values and explanations |
| `f` | Visit the next finding, selecting its affected setting when possible |
| `d` | Open scrollable document details and diagnostics |
| `Escape` in categories | Close Settings |
| `Ctrl-g` | Cancel drafts and close Settings |

Global search and picker filters have separate text inputs. Ordinary letters
and paste belong to the active input. Results name their category; no matches
explain how to revise or cancel the search and cannot submit an edit.

**Remote** is the optional loopback web UI. `remote.enabled` is a consent
setting: turning it on starts a supervised `ovrcr remote serve` child, and
turning it off stops the child. `remote.port` and `remote.login` are passed to
the child; changing either restarts it. The child runs outside the Server's
threads, writes `remote.log` in the instance directory, and reports start,
stop and failure lines as `remote` Events. It never takes the Dashboard seat or
resizes a PTY. The page's **+** starts a session from a registered workspace and
a detected agent; that is a Control `CreateSession`, not a Dashboard action.

Enter edits with the palette's field kinds: a boolean saves the other value at
once; `automatic_local_terminals` and a remembered launch open a pick list
(Terminal or a detected agent); strings open a text field; paths open a text
field whose Tab completes directories. Enter saves that one row and Escape cancels
only that edit. An emptied top-level value resets it. Every save is one
`SetSetting` request. The row shows `saving…` until its own response and the
Server's published value confirm it; an unchanged value already confirmed from
the document settles on the successful response without waiting for another
publication. Values always follow the Server's reading. A refusal shows its
reason on the affected row and leaves the document unchanged.

`picker_roots`, `[[agents]]` and `launch_choices` each occupy one summary row,
including when empty. Enter opens a manager; Escape returns to the same summary.
Inside a manager, Enter edits the selected entry, `a` adds one and `x` removes
only that entry. The details identify the affected folder, agent or project.
Resetting a collection is separate from removing an entry.

Picker roots also support `[` and `]` to move the selected folder up or down.
Reordering sends one `SetSetting` of the whole list and preserves neighboring
folders. The first folder cannot move up and the last cannot move down.
Agent overrides and remembered launches have no reorder action. An agent entry
edits its `argv` as a TOML array such as `["claude", "--verbose"]`; Add asks for
its name and starts its argv as that name. A remembered launch chooses Terminal
or a detected Agent/preset; Add asks for a project name and remembers Terminal.
Changing default picker roots writes the default list with just that change.

Active findings remain visible across categories. `f` visits affected settings;
`d` exposes unknown keys, full diagnostics, document path and read time.
Editing is unavailable before the Server's reading arrives or while the
document is unparseable. A valid published reading restores the controls and
removes resolved findings.

At attach the footer shows one line when the reading has findings, for example
**2 settings findings; see Settings**, and nothing when it has none. When a later
reading changes the number of findings the footer says so again, or **No settings
findings** once they are gone. The next key clears the line. An error banner owns
that line first: the findings notice waits behind it, a key while the banner is up
does not drop the notice, and the notice shows when the banner is dismissed or
clears itself (see Footer messages). The startup `… status off — :connect to set
up` note likewise waits behind the findings notice, and a successful **Connect
agent status** removes it.

The block is the production default document. A missing file loads these values
(`picker_roots` keeps only the paths that exist). Commented lines show optional
keys that are unset by default.

```toml
desktop_notifications = false        # opt in to background Ready and input-needed alerts
ready_sound = false                  # opt in to a sound for the same two alert kinds
ready_sound_choice = "default"        # macOS: default | tap | chime | rise
iterm_focus = false                  # separate opt-in; explicit macOS Automation setup
theme = "catppuccin-mocha"            # default; alias dark. All 80 keys: docs/themes/catalog.md
automatic_local_terminals = "default_branch_only"  # on | off | default_branch_only
# save_uncommitted_work = true       # opt in: push dirty feature worktrees to origin/wip/<branch>
# title_model = "provider/model"     # optional; unset leaves automatic titles off
branch_prefix = "feature/"            # prefix for new workspace branches
picker_roots = ["~/Code", "~/src", "~"]

# [[agents]]                         # optional; a row replaces a detected agent's argv
# name = "claude"
# argv = ["claude", "--verbose"]

[resources]
# sidebar = false                    # hide Resources and stop local machine reads; default true

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

[quota.cursor]
dashboard = false                 # opt in: experimental personal Cursor dashboard reader
# state_db = "/absolute/path/to/state.vscdb" # omit for the Cursor desktop default

[operator]
# model = "provider/model"           # optional; unset uses the dedicated pi profile's default
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

For detected Claude Code, Codex, Pi, and Oh My Pi launchers, the Agent form can
offer verified model choices for an already registered workspace. The Server
reads the launcher's current catalog and checks the selected model and launch
binding again before launch. Configured and cloud launchers keep their existing
model policy. Leaving Model at Harness default preserves the native launch
arguments.

Successful launches update a per-project `launch_choices` table in this file.
For example, `[launch_choices.consigint]` with `kind = "Terminal"` remembers a
terminal; `kind = "Agent"` and `preset = "claude"` remember an agent preset.
Agent entries may include `model_policy = "HarnessDefault"` or
`model_policy = { Explicit = "opus" }`. The default leaves model selection to
the native launcher. A concrete model choice is offered only for a detected
Claude Code, Codex, Pi, or Oh My Pi launcher whose current native catalog is
available; the Server checks the selected model and launch binding again before
starting the session. Pi choices are exact provider/model IDs, although Pi's
native resolver may fuzzy-match if the catalog changes after OVRCR's final
check. If a choice
or its launcher binding changes before Start, the launch is refused and the
remembered choice is left unchanged. Older Agent entries without `model_policy`
keep using the Harness default. These entries contain no one-off command text.
Failed launches and Nothing yet leave the remembered choice unchanged.

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

The list starts with the Dashboard's instance directory (`Home:`) and Server
socket (`Socket:`), with `~` for your home directory. They scroll with the
list; the Home key brings them back. Each event line is the local time, the
component (`titles`, `settings`, or `quota`),
the subject when there is one, and the message. While the popup is open, an
event the Server records is appended. Events recorded while it is closed appear
the next time it opens. The list opens on the newest line and stays there until
you scroll up.

A failed automatic title call names its reason: `call timed out after 30s`
when Pi did not answer in time, or `call failed: <reason>` otherwise, for
example `Pi exited with code 2: <last stderr line>`, `model error: <provider
message>`, or `Pi rejected the prompt: <error>`. OVRCR never puts the
conversation excerpt in the reason. Title calls run `pi --mode rpc
--no-session`, so they do not add sessions under `~/.pi/agent/sessions`.

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
neither suppresses the other. On macOS the subtitle is the manual title, otherwise
the original session name; the body carries project, workspace, original terminal
identity and session ID. Generated conversation subjects, prompt and response text,
prompt titles and answers are never included. Display labels are bounded and strip
control characters. The accepted Codex reporting setup is still
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

Clicking a macOS alert navigates to the original session run in the current
Dashboard and lands in Browse. If that session already occupies a pane, its pane
receives focus; otherwise it replaces the focused pane's selection. Unsent
drafts, dialogs, palette text and History/Copy captures are cancelled, including
when the clicked session already has focus. Submitted operations keep their
original targets. Buffered keys and paste are discarded before new terminal
input can cross the view acknowledgment boundary.

Pending shutdown questions about saving uncommitted work take priority over
notification navigation. Clicks leave those questions and submitted answers
intact; they never infer a save choice or hide a question that blocks shutdown.

An exited original run remains selectable. Archived, removed or reopened rows,
an old Server lifetime, and an absent, draining or replaced Dashboard are ignored.
A click never starts a Server, opens or recovers a terminal, acknowledges Unread,
answers an Input request or sends input. Parent-terminal app activation follows
only a validated current Dashboard's applied receipt; exact window/tab selection
is a separate capability. Native click and activation acceptance is pending.

Host submission runs outside the input loop with bounded queues and a two-second
client deadline. Detaching cancels pending work. On macOS disabling notifications
cancels pending banners and the running delivery client. A saved sound toggle
cannot keep macOS delivery enabled without notification consent. Linux retains
its independent channels; disabling the last channel cancels pending work.
Delivery is best effort, with no retries. Host failures do not change reporting,
native approvals, input or process state. A failed host command displays an
unavailable footer; macOS Bridge failures include installation or update guidance.

On macOS install the separately built and signed
[OVRCR Bridge](../native/bridge/README.md) at `~/Applications/OVRCR Bridge.app`.
The Dashboard starts it when notifications need it and leaves it running after
detach. Ordinary CLI rebuilds do not replace that installation. The footer
distinguishes a missing app ("not installed"), a stale installed app ("needs
updating") and a running Bridge process older than the installed app ("older
build"); each points at the offer the next interactive startup makes. There is
no Script Editor fallback. Startup never restarts the Server without approval.

When saved notifications are enabled at startup, or a successful Server reading
enables them later, the Dashboard checks permission before waiting for an alert.
An undetermined state requests permission once; a pending choice remains pending
and is not treated as denial or retried. Status checks stop after thirty seconds.
If the result remains unconfirmed, use **Check notification permission** (`O`) for
a fresh bounded read; this does not repeat the authorization request.
Denied permission shows recovery guidance.
In Browse use **Open notification settings** (`O`) to open System Settings, then
navigate manually to **Notifications → OVRCR**. This opens the app, not a particular
pane or application row. OVRCR does not change a toggle or open Settings
automatically. After returning, use `O` again to check the current permission;
this also works after opening Settings manually if the launch failed.
Both-off and sound-only startup do not start the Bridge.

Linux uses `notify-send` from the desktop session. Banners request no notification
sound; the [ready sound](#ready-sound) is a separate option. A successful submission
or an unchanged footer does not prove visible delivery: permission, Focus/Do Not Disturb
and desktop policy can suppress it. OVRCR does not change those settings or retry.
The [earlier issue 59 evidence](../research/issue-59-desktop-alerts/README.md)
measured the previous macOS host, not this new Bridge. Native production-Bridge
acceptance is still pending; Linux has automated coverage and native Linux desktop
delivery remains unverified.

## Ready sound


### Theme

`theme` chooses the Dashboard palette. The default is `"catppuccin-mocha"`
(Catppuccin Mocha), which is also the public README brand; `"dark"` is accepted
as an alias. There are 80 built-in choices: 54 Dark and 26 Light, classified
using explicit source metadata. The [complete catalog](themes/catalog.md)
lists every native ID, approved gallery ID, family, attribution, and semantic
mapping decision. The original eight keys below retain their values and aliases:

| Key | Label | Notes |
| --- | --- | --- |
| `catppuccin-mocha` | Catppuccin Mocha | default; alias `dark` |
| `catppuccin-latte` | Catppuccin Latte | alias `light` |
| `tokyo-night` | Tokyo Night | |
| `dracula` | Dracula | |
| `gruvbox-dark` | Gruvbox Dark | |
| `gruvbox-light` | Gruvbox Light | |
| `nord` | Nord | |
| `rose-pine` | Rosé Pine | |

Every theme shares the same fifteen token names. Change it in **Settings**
(Appearance) or with `ovrcr settings set theme tokyo-night`; the Dashboard
hot-switches on the next `SettingsChanged` without a restart. Borders use
`SURFACE2`, selected rows use `SURFACE0` with `TEXT` (not inverted), and the
accent rail stays `MAUVE`. Custom CSS, per-session themes, and follow-terminal
are out of scope.

Appearance initially shows one compact **Theme** row. Enter replaces the list
with a searchable picker, keeping the category sidebar on wide terminals.
It opens on the Server's saved choice and groups all palettes under **Dark**
and **Light**. Arrows, mouse wheel and Page Up/Down navigate; Home/End jumps to
the first/last match. Typing or pasting filters the choices and previews the
first match. An empty result restores saved colors and cannot save.

Highlighting repaints the whole Settings view without writing. When the preview
differs, both **Preview** and **Saved** names remain visible. Enter submits the
canonical theme ID once and shows `saving…` until the Server publishes the
effective value. A refusal shows its reason and restores confirmed colors.
Escape returns to the Theme row using the latest saved colors; Ctrl-g closes
Settings. Reopening while an earlier save is pending starts from the latest
saved choice, and a late reply cannot replace a newer preview. Previewing
changes neither `dashboard.toml` nor embedded agent PTY colors.

The ready sound is off by default. Set `ready_sound = true` in `dashboard.toml`,
press uppercase `S` in Browse, or use **ready sound** in the command palette to
save that opt-in through the Server. In terminal mode `S` remains ordinary input;
use Ctrl-g first. Notification and sound preferences are saved separately, and
changing either does not silently enable the other.

On macOS the Bridge attaches sound to an eligible native banner only when both
notification (`N`) and sound (`S`) opt-ins are on:

| N | S | macOS delivery |
| --- | --- | --- |
| Off | Off | None |
| Off | On | Sound saved; no banner or sound |
| On | Off | Silent banner |
| On | On | Banner with the selected sound, subject to native permission and policy |

In Settings choose **Ready sound choice**: **System default**, **Tap**, **Chime**
or **Rise**. The persisted IDs are `default`, `tap`, `chime` and `rise`;
`ready_sound_choice` defaults to `"default"`. Saving/resetting a choice does not
enable N or S, preview audio, request permission or replay an old event. An
explicit unknown or wrongly typed choice remains a finding, suppresses sound and
shows recovery guidance when both flags are enabled. Choose a valid value in
Settings or fix that key in the document; unrelated settings and raw invalid
values are preserved.

A sound follows exactly the same selection as a desktop alert: a new unread
identity for one accepted managed root response, or a new input request, with the
same deduplication and attach/reconnect baseline, including for selected and
visible sessions. The sound is the same for both kinds.
Confirmed activity without unread does not play. When both channels are on, one
response produces one native request carrying its sound on macOS. Requests carry
only the four fixed choice IDs; no caller path, prompt, response or terminal
content is passed as a sound resource.

macOS uses the native notification default sound or the three original bundled
WAV assets through [OVRCR Bridge](../native/bridge/README.md). It does not launch
`afplay`. Missing, unreadable, damaged or conflicting custom resources submit an
otherwise eligible banner silently and show fixed recovery guidance. There is no
fallback to another tone/player and no retry. Queued events use the latest
accepted settings at dispatch. Changing S or the choice best-effort cancels an
already running owned delivery client; a request already admitted by macOS
cannot be retracted, and a change does not replay it. Permission/Settings jobs
and the persistent Bridge are kept separate from that cancellation.

Linux still plays the freedesktop
`/usr/share/sounds/freedesktop/stereo/complete.oga` with `paplay`, independently
of notifications. It keeps the five-second playback deadline and
**Ready sound unavailable** failure notice. The macOS selector has no Linux
runtime effect.

Submission and an unchanged footer do not prove audible playback. Native policy,
output volume or an absent device can suppress sound; OVRCR changes none of them.
Signed installation/update, visible banners and listening to all four choices
remain unverified for this source slice. The [issue 60 evidence](../research/issue-60-ready-sound/README.md)
records the prior macOS player path, and native Linux playback remains unverified.

## Output delivery

Dashboard output is delivered through a bounded queue, so a detached or slow
dashboard can reattach and refresh the current screen. Output backlog is
coalesced into a per-session refresh when necessary. Control responses and
lifecycle events are preserved, and the dashboard disconnects if the queue cannot
accept one of those messages.


### Archived sessions

Closing a live session (`X` or `Space t X`) asks for confirmation, stops its
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

### Exact existing-iTerm-session focus

On macOS, `iterm_focus = false` is a separate default-off preference. Enable
**Exact iTerm focus** in Settings, then choose **Set up iTerm focus** in the
command palette (`:`). That explicit action may show a separate macOS Automation
permission prompt for OVRCR controlling iTerm. Enabling desktop notifications,
receiving a banner, ordinary clicks and setup-status checks do not request this
permission. Setup is nonblocking and reports pending, authorized, denied or
unavailable status. It observes pending status for thirty seconds; afterward the
same palette action checks that attempt once without starting another prompt.

A valid click navigates to Browse through the Server and its current Dashboard
unless shutdown WIP-save questions are pending. When the current Server-accepted
preference is enabled and
Automation permission is already available, the Bridge may select that
Dashboard's existing local iTerm session, tab and window. It matches the live
session GUID and controlling TTY, not remembered window/tab/pane indices or a
banner's former Dashboard. Every ordinary Apple Event forbids consent prompts.
Changing the active Dashboard or turning the preference off invalidates an
in-flight exact-selection attempt.

Denied, missing, revoked or unsupported integration preserves notification
permission and Dashboard navigation. It falls back to activating the current
Dashboard's parent app; with several terminal windows this cannot select the
exact window. Denial guidance points to System Settings → Privacy & Security →
Automation and another explicit setup action. The Bridge creates no iTerm
sessions, tabs or panes, changes no layout/color/badge, reads no screen content
and sends no terminal input. Linux's existing alert and terminal paths continue
unchanged. API acknowledgements and headless tests do not establish visible focus;
reviewed native setup/click/fallback acceptance remains required.
