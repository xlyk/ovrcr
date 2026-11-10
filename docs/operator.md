# Use the built-in operator

OVRCR's Operator is one persistent conversation per OVRCR instance, run by the
installed [pi](https://github.com/earendil-works/pi) coding agent through its RPC
protocol. There is one conversation, not one per project or workspace: the
workspace or session you capture is sent with each message, so the same
conversation can work on different targets without losing its history.

## Connect your own pi profile

Operator uses a dedicated pi profile inside the instance directory, separate from
your personal pi account. Start it with:

```sh
ovrcr operator setup
```

That command needs an interactive terminal and rejects `--json`. It explains the
native pi commands and starts pi in the dedicated profile, where you can:

- `/login` — sign in with a subscription account, or refresh it
- `/model` — choose this profile's default model
- `/logout` — remove the account from this profile

Leave that session when you are done. OVRCR records the profile's current default
model; it never copies your personal pi profile, its credentials or its settings.

List the models the dedicated profile can currently discover:

```sh
ovrcr operator models
ovrcr --json operator models
```

Text output is `provider/id<TAB>name`; JSON is
`{"models":[{"provider":"...","id":"...","name":"..."}]}`. An empty list prints no
invented defaults and points back to `ovrcr operator setup`. Discovery proves a
model is visible in the profile, not that the account may run it.

The model pi uses for a turn is either the profile's current default or the
`operator.model` setting; there is no per-conversation model form.

## One conversation in the Dashboard

The sidebar starts with the first project. One fixed **Operator** dock sits below
attention and quota, immediately above the Dashboard footer. Click anywhere on
the row or press `o` in Browse to open the conversation with the composer focused.
The title bar has no Operator controls.

The name stays on the left and one state appears on the right. A closed idle
conversation says **Open chat**; an open idle conversation shows its selection
marker without that prompt. **1 unread** takes priority until the loaded tail
has been seen. Next come current activity or an exceptional outcome, then
**Draft**, then the idle action. Sending, Responding, Waiting for a tool result,
Stopping, Stop unconfirmed, failed and interrupted turns remain distinguishable.
A reviewed successful turn returns to idle. Drawing only the dock never reviews
a reply or starts a worker. Opening and unavailable states have their own labels.
The dock hides with the sidebar and yields its row to preserve three tree lines
in a very short window; `o` still works.

**Quick message** is available in Commands and on `Ctrl-o` from Dashboard views.
In Terminal mode `Ctrl-o` goes to the program (Oh My Pi expands tool output, Claude
Code opens its transcript), so press `Ctrl-g` then `Ctrl-o` for Quick message.

Quick message captures the current workspace/session target each time it opens an
editable draft. Type a message and press Enter to send it once. If the conversation
has never been opened, the first Send also creates the durable conversation; a
cancelled unsent draft creates nothing. `Shift-Enter` adds a newline; paste only
edits. Empty messages cannot send. The dialog starts at seven rows, grows with
wrapped input and errors up to sixteen rows, and stays at most eighty columns
wide. Outside clicks remain modal.

Opening Quick message or the full view captures the current selection for the next
message while keeping the draft text; the target caption shows what Send will
carry, so check it before sending a retained draft. A Quick overlay that is
already opening or submitting keeps the context frozen by that explicit Send, and
a message that was sent keeps the target recorded in its journal entry.

Esc discards an unsent Quick draft only while it is unseeded and error-free: that
is, before the conversation exists, where a cancelled draft creates nothing and
restores the underlying view. Once the conversation exists, both entry points edit
the same draft, so Esc only hides the overlay and the draft stays. Esc after
sending dismisses the form while sending continues; its late receipt cannot switch
your view. `Ctrl-o` reopens the same draft or retained error. If the conversation
is busy, or its previous worker's stop is unconfirmed, Send keeps the draft and
explains why. It never queues the message, steers the running turn, starts another
conversation or resubmits later by itself. Below 60×16, resize guidance preserves
the form and the draft.

## Composer and journal

Type your request and press Enter. The operator runs pi's native `read`, `bash`,
`edit` and `write` tools against the target you captured; it does not ask for a
separate approval for each command. Durable tool calls and results appear in the
journal before the model continues, so a crash cannot lose an accepted step.
Existing OVRCR CLI guards still apply.

The Operator's system prompt includes OVRCR's own CLI guide before the first
message in both the full composer and Quick message. That guide is compiled into
OVRCR, keeps the inherited instance, socket and captured target, and is available
without a checkout. Automatic skill discovery stays off: personal and project
skills are not loaded, and the guide never appears in your conversation, tool
results or journal. Every ordinary model request, including one after pi
compacts context automatically, receives the guide only as system prompt text.

`Shift-Enter` inserts a newline; paste edits the draft without submitting. Both
message inputs support these editing commands:

| Keys | Action |
| --- | --- |
| Ctrl+A / Ctrl+E | Start / end of the current logical line |
| Ctrl+B / Ctrl+F | Previous / next grapheme |
| Ctrl+H / Ctrl+D | Delete previous / next grapheme; empty Ctrl+D does nothing |
| Ctrl+U / Ctrl+K | Delete the line prefix / suffix, retaining newlines |
| Ctrl+W | Delete preceding whitespace and the preceding word within the line |
| Alt+B / Alt+F | Previous / next whitespace-delimited word boundary within the line |
| Ctrl+C | Clear the draft, including whitespace; a separate press in an empty full composer stops the current turn |

Word commands treat punctuation as part of a word and never cross a newline.
Home/End move to the start or end of the whole draft; Up/Down move between logical
lines. Empty Quick message Ctrl+C does nothing and never stops the conversation.
Native alphabetic Control keys with Alt or Shift remain modifiers, so mixed chords
such as Ctrl+Alt+C or Ctrl+Shift+U do not clear, stop or edit either message input.
The native GUI ignores held Ctrl+C repeats; legacy external terminals may report
each held byte as a fresh press. The full composer's key menu and palette list the
editing actions; `Ctrl+X` is the explicit Stop shortcut.

Send is offered only for a nonempty draft when the conversation can accept it.
Running, sending and unconfirmed-worker-stop states keep the draft.

A single activity row follows the latest message or tool receipt in the journal.
It shows `Sending` while submission acceptance is pending, then `Responding` while
the turn is running, including quiet periods. `Waiting for <tool> result` (or a
count) means a recorded call has no matching result in the current turn's
caught-up history; it does not certify that an operation is still executing.
While pi replaces old context automatically, it says `Compacting context…` and
clears when the next model request resumes. Paging history falls back to
`Responding`. These labels scroll away with older history and return when you
scroll to the tail. Stopping has its own label, and `Stop unconfirmed` is static
until stop proof arrives.

Displayable provider thinking streams into a distinct **Thinking** block in the
order it arrives. Its body is peach and italic; the header stays muted and upright.
While its turn runs, an untouched block shows all wrapped lines up to six, then
only the latest six with `▸ Thinking · showing last 6 of N lines`. Short blocks
show `▾ Thinking · N lines`. When the turn ends, untouched blocks collapse to
`▸ Thinking · N lines`, including failed, interrupted and older turns. Empty
thinking adds no block. Counts include interior blank lines but exclude trailing
newline terminators, and change with the available width.

Enter on a selected block or a click on its header or body toggles that block.
Manual expansion shows all lines and survives completion and reopening. Manual
collapse hides the body even while more thinking arrives. Other blocks keep their
own choices. Toggling keeps a visible header at the same screen row; blank space
below a collapsed block is intentional. The journal has one scroll, so PageDown
or the wheel reaches the answer below an expanded block.

`PageUp` and `PageDown` scroll the journal. User prompts are marked, and assistant
and tool activity share a fine guide. Tool call and output receipts stay
independently expandable: adjacent rows keep their own details and raw IDs appear
only when expanded. Failures, timeouts, truncation and empty output remain
explicit. Expanded details retain the full IDs, captured context, captured
directory and output. Control characters in model or tool text are escaped before
rendering.

`Ctrl-g` or `Esc` leaves Operator for the terminal panes in Browse, keeping the
draft; `o` returns to the same conversation, history and draft. Completed replies
stay Unread until the loaded tail is actually drawn.

`Tab` cycles the transcript and composer. With the transcript focused, Up/Down
select a context, tool or thinking entry and Enter toggles it. A thinking block
is one entry even when it arrives in many deltas. `Ctrl-l` shows or hides the
ordinary Projects sidebar; it never opens a conversation list. At widths below 100
columns the conversation uses the full width by default. Below 60×16, resize
guidance preserves drafts and active work.

The header reads `Operator · pi · provider/id` once a model has been selected, and
`Operator · pi` before that; it never guesses an identifier from settings or a
catalog. The row above the composer shows the target the next message will carry:
the workspace/session selection captured when the composer opened, not an older
message's target. The agent's working directory is fixed for the life of the
conversation and is shown separately in a user message's expanded context, so it
never silently follows the Dashboard selection.

## Model, compaction and checkpoints

Change the model in Settings, or in the instance's `dashboard.toml`:

```toml
[operator]
model = "provider/model"
```

The value is one exact `provider/model` identifier; the model id may contain
further slashes. Leave it unset to use the dedicated pi profile's current default
before each turn. An explicit value is captured with the accepted message, so a
later settings edit cannot change a turn that is already running. Model
availability is checked against the profile's own catalog; there is no fallback to
another model. A missing model is fixed with `ovrcr operator setup`, not by editing
an identifier you guessed.

pi compacts the conversation automatically when its context fills, so the
conversation keeps working without a Reset or New action. After each successful
turn, OVRCR keeps that turn's completed pi session as the context checkpoint for
the next turn. A failed, interrupted or uncertain turn keeps its partial journal
and attempt files as evidence but never becomes the checkpoint: the next explicit
message resumes from the last completed turn. This rolls back model context only.
It never replays a prompt and never rolls back files, commands or terminal input
that already ran.

## Stop or recover a turn

Use the **Stop** control, `Ctrl-x`, or `Ctrl-c` in an empty full composer to stop
the current turn. Stop affects that exact turn's model work; an OVRCR operation
already accepted may finish. Visible partial text and command outcomes remain in
history. Generic `terminal kill` and `terminal close` refuse Operator scope, and
there is no archive, unarchive or delete route for Operator conversations.

Wait for confirmed worker stop before sending again. **Finishing / stop
unconfirmed** is not confirmation. If an old turn is uncertain after a crash,
inspect it with `ovrcr --json terminal list`; its `scope` identifies the
conversation. Only after you independently confirm its worker and background
processes have stopped, acknowledge the retained session:

```sh
ovrcr terminal acknowledge-stopped SESSION_ID
```

Acknowledgement marks an unfinished turn Interrupted and unlocks the next message.
Interrupted or uncertain commands are never automatically replayed.

Each running Operator turn uses one of the existing fifty live-session slots; an
idle conversation uses none and at most one Operator turn runs at a time. At
capacity, stop or close an ordinary session through the normal controls, then
retry the preserved draft.

## Existing data

Legacy Operator conversations, their journals, credentials and recovery evidence
stay on disk untouched. The Dashboard hides legacy Operator rows, and the Archived
palette does not offer them, so there is no way to reopen or archive them from the
UI. Their records still appear in terminal diagnostics for inspection and
`acknowledge-stopped` still works for them.

See the [runtime and authentication reference](development/operator.md) for the
worker, pi transport and checkpoint contracts.
