# Dashboard behavior

Notification offers require a matching pending Server confirmation and current
lifetime/run hierarchy. Application uses `retarget` even for the same session,
lands Browse, cancels unsent overlays/captures and removes queued Input from the
outbox while leaving submitted operation targets intact. The actual event loop
observes the navigation input epoch, stops later Server dispatch and discards
old terminal events through bounded continuation batches until the input queue
is empty. It then emits the ordinary `ViewHandshake` request; new input still
requires all current snapshots and the final matching acknowledgment. A clicked
retained run is excluded from automatic recovery.

Pending shutdown WIP-save questions retain their palette and answer requests.
Notification offers and their matching confirmations are ignored while those
questions are queued; a click must never infer a save choice or hide a question
that blocks shutdown.

- Revoke input permission immediately when focus, assignment, or visible geometry changes. Input requires a Running session in the current acknowledged visible view, using that pane's terminal modes.
- Match snapshots by request, revision, session and run. Require every expected snapshot plus the matching final acknowledgement before enabling input. An empty view expects zero snapshots and must still complete.
- Keep one view request in flight and coalesce desired changes. A true no-op must preserve readiness. Stale responses must not populate reassigned panes, clear current errors, or enable input.
- Use the same geometry for subscriptions, rendering, hit-testing, and cursor placement. Hidden panes retain their assignments and installed screens but cannot receive input. Never send zero PTY dimensions.
- Keep History/Copy tied to the captured session and run. Changing either cancels pending capture requests, selection jobs, and queued clipboard completions. Removing an unrelated pane must preserve a valid focused capture. A new run clears the parser, input readiness and Unread but preserves pane assignment and layout.
- Current-screen Copy cancels on resize through the actual shared client path. Frozen History retains its cells and wrapping while its viewport changes; live output must not rewrite the frozen capture.
- Preserve overlay input priority, bounded event batches, and the input-drain-before-clipboard-emission boundary.
- Treat literal spacing, terminal modes, Unicode width, colors, cursor placement, and accessibility text as behavior. Keep natural wide-glyph widths and clip at pane boundaries. A TestBackend pass alone may miss errors in emitted terminal coordinates.

Sidebar session-jump labels are derived from the drawn tree order, viewport,
row heights, and exact Agent session/run identities. Draw them inside the existing
indent so status, title, row height, hit-testing, and PTY geometry stay fixed.
Use only single-digit labels 1–9 on the first nine eligible drawn Agent title
rows, regardless of how many rows are visible. Later rows remain visible and
unlabelled; scrolling and a fresh Ctrl-g assigns labels to the new first nine.
Capture the mapping when Ctrl-g opens the picker. A bare digit 1–9 selects
immediately (Ctrl+digit remains accepted); 0 and other unmapped digits show a
retry hint, with no prefix state or Backspace editor. Esc or Ctrl-g closes the
picker without selecting. Every other key closes it and continues through its
ordinary Dashboard, Operator, or program handling. Command/Super never opens
the picker. Hide labels on selection. A selected running destination remains in
Browse until the existing view and input boundary is drained; queued loading
input must never become child bytes. Opening an overlay, losing focus,
scrolling, or resizing closes the picker.

Enhanced keyboard reporting belongs to the existing `TerminalGuard` lifecycle.
All-key reporting must also request alternate key values so shifted punctuation
such as `:` and `?` arrives as the character the user typed. The bundled GUI's
mode observer must recognize the same request as the real terminal path.
Request its modes at Dashboard entry, restore the owned mode on normal exit,
interrupt, error, and panic, and preserve the user's prior mode by popping only
the push this Dashboard owns. The session-jump picker needs only the ordinary
Ctrl-g key event every terminal delivers, so it has no per-frontend
qualification, environment probe, or `DashboardInputProfile`. Enhanced key
reporting remains for the modes and shortcuts that need it.

Readiness is derived by `ViewHandshake::is_ready`; pane changes go through `retarget(PaneChange)`; outbound requests go through the outbox and are drained by `drain_outbox`. Session status — the glyph, the plain-words state (with its narrow form, a bare `Unread` when there is one), the reporting words (`starting…`, `status off`) and elapsed clauses the pane headers read, and the activity, Unread and health clauses the Agents picker reads — is decided once by `dashboard::status::SessionStatus`, which the sidebar row, both pane headers, the picker and the spinner redraw cadence read; each surface still arranges and clips the clauses for its own width (`render::pane_header` gives up the pid, model, elapsed time and label before the state), and readiness itself is still decided in `dashboard::ready`. Quality words and reporter reasons never reach the headers. The Ready checkmark is drawn only while that response is Unread; after review the glyph is blank and the header still says Ready. The sidebar agent label appends the current model after a colon when one is known; on a shared line the title keeps its room first, so the model goes, then the agent, before the title clips. A session row is one or two sidebar lines: `render::tree_row_heights` decides, for the whole visible tree at the current sidebar width, whether every agent label fits inline or every one takes a second line, and rendering, scrolling, selection visibility and mouse hit-testing all read those heights through `tree_line_at`, `tree_line_count` and `tree_row_start`, so the extra line always resolves to the same session.
Key bindings live in one table, `dashboard::keymap`: it holds each key's label, action and the reason it is unavailable, and `key_action`, the footer, the key popup and the palette all read it rather than repeating a guard. The palette applies the popup's `shown_in_group` rule: it drops bindings the target cannot use and keeps waiting or protected rows with their reason, so the list never shifts under the cursor when a screen is acknowledged. The title bar's `[Commands]` control opens the palette. Split (`v`) reuses the palette's Search page as the new pane's session chooser (`Palette::split`), and palette setting rows open the editor through `Editor::select`.

Ordinary palette forms submit Enter from any field after `form_problem` checks the visible fields top to bottom (required, pick value in its list, Branch after the prefix and valid under `git check-ref-format --branch` via `branch_name_problem`); the first problem takes the focus and is drawn under its field from `Palette.invalid`. A pick field's value is its highlighted option (`accept_pick` after every arrow, cycle or filter edit). New-agent Branch, Agent harness and Model, plus child Create-branch Project, are compact pickers: Enter opens or accepts the choice and leaves the creation form active; Alt-Enter starts or creates. Other form pickers keep ordinary Enter submission into their existing confirmation flow. These creation menus are bounded overlays anchored to the focused value surface: opening one does not resize or reflow the form, and a menu can temporarily cover nearby fields. The same `creation_picker_popup` rectangle and option-row map drive rendering and mouse hits. Lists use `PickList::lines_bounded`, keep the highlighted item visible while scrolling, flip above the anchor when needed, and stay within the form body. A click on an option accepts it; an outside click closes the menu and is consumed by the palette. Clicking another visible picker field switches the anchored menu. Escape closes the menu before the form, while Tab/Shift-Tab closes it and advances focus. The Initial prompt field is multiline: Enter inserts a newline and the visible Alt-Enter chord submits or retries. `form_keys` builds the footer from the form and its focused field, and Tab remains in the active dialog.
New-agent and child branch forms use one bordered card with theme-backed field surfaces. Harness and Model share a row only when both columns fit. The optional prompt keeps its `Initial prompt` field identity and a bounded, cursor-aware viewport (three rows when wide); mouse targeting uses the same page-aware dialog and field rows as rendering.
Git failure text omits preparation progress before the fatal reason, so narrow creation forms show the cause of the refusal.
A Lifecycle job submitted from the palette carries that palette on its Provisional row while the job runs (`take_palette_for_job`). On failure, `fail_lifecycle_job` returns it through `return_to_form`, which replays the failure through `palette_response` exactly as a synchronous refusal, so recovery pages (RecoverLaunch, WIP save, discard) are shared. Only when the form cannot come back (another overlay, or a non-Browse mode) does a failed row stay. Workspace removal with running sessions sends CloseTerminal for each and then the removal, chaining each request from the previous `LifecycleCompleted` through `Provisional.then`. Standalone `w` opens the branch-only form; the New agent Branch picker can suspend its parent form and open that same child. Branch creation completes through a matching lifecycle receipt and fresh Inspect/List readback, and never starts an Agent. The parent Agent request is submitted separately after the new checkout becomes its selected destination.

The separate Initial prompt field travels through one-use intent allocation. A
blank prompt leaves the exact detected Claude Code, Codex, Pi, or Oh My Pi
launcher interactive. A nonblank prompt is passed to that native launcher as a
transient positional argv item after `--`; it is not a session name or branch,
and is not retained in the session's base argv or metadata or added to the
remembered launch choice. Configured launchers do not use this prompt path.
Model lookup and explicit model selection
use current catalogs for those same four detected launchers and verify the
launch binding again before spawn; configured and cloud launchers keep their
existing model policy. Pi's native resolver may fuzzy-match after OVRCR's final
catalog check. Known pre-spawn refusal restores the frozen harness/model/name
and prompt for an explicit fresh-intent retry. Accepted launches reconcile by
stable Server-lifetime/sequence ID and exact session/run; repeated history
cannot repeat the launch or steal focus after the user selects another session.
Pending and Unknown snapshots may carry the allocated session/run, but the
Dashboard only attaches or focuses a matching run after `LaunchAccepted`. The
UI calls this “Agent started” while delivery remains unknown; uncertain
outcomes say to check for the agent before retrying.
`dashboard::attention` decides which sessions need the user: a running session whose effective activity is WaitingInput, else one with an Unread that is not Busy again. Paused sessions and Operator conversations never count. The sidebar attention strip counts them across the whole hierarchy, and the `i` binding and a strip click visit them in one order (Input before Ready, each in fully expanded tree order, cyclic from a queued selection, otherwise the leading kind after the selection). A keystroke or paste sent to the focused session reviews its Presented Unread with the same `MarkReviewed` request `R` sends, queued in the Outbox ahead of the input, once per Presented observation (`Unread::reply_target`); the response is ignored, so a stale refusal stays silent. Mouse reports forwarded to the application never review.
Footer banners go through `set_error` (a failure, behind a red `ERROR:`), `set_limit` (an expected limit, muted) or `set_server_error`, which reads the typed blocker codes as limits and everything else, `Conflict` included (the Server's catch-all for failed lifecycle work), as failures, passing the message through `plain_error` (no code in front; `project/<id>` becomes `project / branch`, a bare workspace id its branch and `#N` the session title, never inside a path; unknown ids stay). A caller that knows its refusal is a limit calls `set_limit` itself: a refused view, and terminal input the Server did not admit (`Conflict` or `InvalidRequest` to a request id in `input_requests`). A form that shows a Server refusal inline calls `plain_error` too. A banner clears when the request that owns it succeeds, or `BANNER_LIFETIME` after its clock starts (when a limit goes up, or at the first key press after a failure), which `Dashboard::draw` checks every frame.
A pane with no session draws a card from `dashboard::cards`: the welcome steps, the selected project or workspace, or every project while nothing is selected. Cards are display only. They read the hierarchy snapshot, `SessionStatus` (the same plain-words state the headers use), `dashboard::attention` and the keymap (by action, never by key letter), send no request, and leave pane assignment, the view handshake and input permission alone. Next-step hints name a key only while its binding is enabled; the welcome's numbered steps always name theirs. A card fits its pane: columns shrink before the state and elapsed time, and when rows run out the next steps stay and the list ends with `+N more`.

The Outbox owns every fully formed steady-state request. Producers enqueue once;
Dashboard actions carry only local effects or fresh PTY bytes. `drain_outbox`
schedules due History work after requests already queued. Producers that formerly
carried a request in an action schedule due History before enqueueing that request,
preserving cleanup → History page → request order. Deferred palette submission
keeps its existing request-before-History order through the ordinary queued path.
History key/cursor/copy paths
also keep their active scheduling points and pending-page suppression. Each Server
message drains its follow-ups before the next message, except ignored responses;
navigation keeps its bounded input-discard break. Fresh PTY bytes are authorized
only after queued requests flush; captured cleanup keeps its original target and
authority. Clipboard effects retain the input boundary, and startup keeps reserved
request IDs 1/2 and its interleaved handshake outside this steady-state ownership.

`Ctrl-o` (`keymap::is_operator_quick_key`) is reserved everywhere except where
keys reach the program: Terminal mode with no Tasks, details, palette, Which-key,
Operator editor, agent-loading intent or Quick form over the pane
(`Dashboard::opens_operator_quick`). There it is PTY input, since Oh My Pi and
Claude Code bind it; Ctrl-g then Ctrl-o opens the form. Elsewhere key dispatch,
event dispatch before Tasks, and the production mouse path give the Hub-owned
quick form priority. The form layers above existing views without taking their
state; Repeat/Release and all modal mouse events are consumed. Quick addresses the
same singleton conversation as the full view. Startup sends `OperatorRequest::Current`,
which never creates a record or process; the first explicit Send sends `Open` only
when `Current` returned none, then `Submit` in one frozen transaction using the
acknowledged `summary.run`. An unsent cancelled draft sends nothing. Esc hides
submitted work, and only a still-visible matching generation may navigate on
acceptance. A busy or stop-unconfirmed conversation keeps the draft and shows the
reason; nothing queues, steers, creates another conversation or submits later.
The quick dialog starts at seven rows (two editor rows), grows with wrapped input
and an optional error row to sixteen, and keeps its eighty-column cap. Errors and
pending phase hints remain visible; shrinking the form cannot forward modal clicks.
Failures retain the message; an explicit retry reuses the same conversation.
Notification retarget cancels an unsent form or hides its pending continuation.
Quick visibility also blocks durable-tail review acknowledgement.

Full Operator message input and editable Quick Message call
`TextCursor::edit_key(KeyEvent)` for line/grapheme/word editing; other fields keep
`TextCursor::key(KeyCode)` unchanged. Ctrl+A/E and Ctrl+U/K use logical line edges,
Ctrl+W and Alt+B/F use whitespace-delimited words without crossing newlines.
Home/End remain whole-buffer. Modifiers match exactly; unsupported combinations
are consumed without inserting letters. Full input repeats ordinary edits, while
Quick retains Press-only editing. Each owner handles Ctrl+C itself: a Press clears
literal nonempty text and resets the cursor; a separate empty full Press calls the
existing exact Stop. Repeat/Release do neither and empty Quick is a no-op. Quick
edits mirror the singleton draft once that conversation exists, so both entry
points share one draft; an unseeded Quick draft is its own unsent text. A frozen
Open+Submit payload never changes after Send. The native GUI's `encode_event` suppresses known repeated
Ctrl+C before raw byte 3 loses repeat identity; first Press still sends byte 3.
Control-only bytes and ordinary repeat stay unchanged. Alphabetic Control keys
with Alt or Shift retain those bits using the existing CSI-u spelling (lowercase
ASCII codepoint, modifier parameter 1 + Shift 1 + Alt 2 + Control 4), so unsupported
composer chords cannot become destructive Control-only input. The shared terminal
encoder preserves Control-byte aliases, including legacy Ctrl+2-to-NUL behavior.
Legacy external terminals without repeat metadata can report every held byte 3 as a
fresh Press, so they cannot distinguish a hold from separate presses. No timing
heuristic or keyboard-protocol redesign is applied.

Retained rows stay selectable without allocating a parser on the server or
inventing live timing. Reopen targets the existing row and expected run; Retry
never implies ownership acknowledgement. Same-boot or unverifiable recovery
requires a separate explicit confirmation that old agents and background
processes have stopped. Agent rows without a conversation reference, or whose
recorded reference cannot be certified, open the provider's native resume picker
for Claude, Codex, Pi and Oh My Pi. Unknown providers and references without a
native picker keep the unavailable notice.


The Operator occupies a three-row section below the attention line and above the
project tree; quota stays at the bottom. `SidebarRects` supplies attention,
Operator, project-tree and quota rectangles to rendering, mouse input and
viewport calculations. A section can have a trailing blank row, and the shared
height ladder removes gaps first, then compacts or hides Operator and quota to
preserve tree space. Operator's full section shows state and one activity/reply
detail row; the one-row form clips to its name and status. `operator.sidebar`
controls visibility and defaults on. The section preserves the selected-row
background when the conversation is open. Working shows the durable turn-start
age; Idle and unread states show the durable last-reply age when known. Legacy
turns without timestamps remain untimed. Setup detection checks for the Pi
executable without starting a process or validating provider credentials.
Saved summaries load asynchronously after the production handshake and initial
terminal attachment, through `OperatorRequest::Current`, without opening Operator.

Browse `o` and any cell of the section open the single conversation with the composer
focused. The title bar has neither full-chat nor Quick message targets; Quick
message remains in Commands and on its existing key routes. Terminal pane assignments survive, but the
desired terminal view is empty while Operator is visible and native composer input
never produces PTY bytes. `Open` is idempotent: reopening returns the same identity
and resets no history, draft or turn. Leaving Operator (Ctrl-g or Esc) hides it
through `retarget(PaneChange::Focus)`, so the next view request re-subscribes the
unchanged panes. Existing overlays retain input priority; spaces and punctuation in
an active composer remain ordinary text.

`dashboard::operator` owns the singleton's draft, context, grapheme cursor,
scroll/follow-tail and completion Unread state. Selecting a terminal does not
change a message that is already being composed or sent; the captured target
travels with each message, and reopening either composer captures the current
selection for the next one. Scrolled transcripts keep a durable message ordinal,
logical part and byte anchor across streaming, receipt expansion and resize.
Styled graphemes survive wrapping; the Working journal reserves a two-cell gutter
before wrapping and decorates only after source-byte anchor calculation. User
prompts have a mauve cue; assistant/tool activity shares a muted guide. Prose and
command/output details use the available transcript width after the gutter.
Displayable thinking deltas form one disclosure per adjacent same-turn run ahead
of the answer. The body is peach and italic with a two-cell indent after the
muted guide; its header stays muted and non-italic. Count wrapped body rows at
the current width. Untouched current Running blocks show all rows up to six
(`▾ Thinking · N lines`), then only the latest six
(`▸ Thinking · showing last 6 of N lines`). Other untouched blocks collapse to
`▸ Thinking · N lines`, independent of worker-stop proof. Explicit expansion
shows all rows; explicit collapse hides them during streaming too. Choices
survive completion and reopening. Empty or newline-only thinking adds no row
or selection stop; interior blank lines count and trailing newline terminators
do not. The final thinking prefix is deduplicated per content block.
Adjacent tool calls and outputs stay separate disclosures under their original
ordinals, without blank rows or collapsed raw IDs; expanded details retain IDs.
Expanded context prints the captured target and the fixed agent workdir separately.
Successful completion Unread comes from the exact durable turn summary;
`ModelSelected` updates the header without becoming a journal row.
`Tab` cycles transcript and composer; `Ctrl-l` toggles the ordinary Projects
sidebar and never reveals a conversation list. Transcript Up/Down selects a
user-context, tool or thinking entry; each thinking run is one stop. Enter or a
click on a disclosure's header/body toggles only that entry. Visible thinking
headers keep their screen row through toggles using the top durable source
anchor, without tail clamping or a nested scroll. If collapse removes a top body
anchor whose header was offscreen, the header lands at the viewport top.
PageDown and wheel scrolling rejoin the tail normally.
The borderless composer uses `clamp(wrapped_draft_rows, 2, min(6, body_height / 3).max(2))`
text rows and scrolls around the real cursor. One-cell edge padding, a mauve top
rule and muted bottom rule define the editor. `OperatorLayout` owns header,
transcript, context, writable composer and both rule rectangles for rendering,
hit-testing and review. The captured target uses the context row above the top rule.
A single transient journal row follows the latest durable message or receipt,
aligned after the same two-cell gutter: Sending before submission acceptance,
Responding throughout Running (including silence), Waiting for a recorded
current-turn tool result, or Compacting context… while the current turn's last
durable compaction phase is started. Compaction phase events are durable phase
state but never render as transcript rows; the matching end event, a terminal
turn state, or a later turn returns the ordinary activity. The compaction phase
and tool waits require
caught-up history: paging/dirty history falls back to Responding.
Stopping remains explicit and terminal work without stop proof says static
Stop unconfirmed. Confirmed terminal work removes the indicator. The private
derived activity decision also requests the shared 100 ms spinner cadence while
the journal is visible, independent of sidebar visibility; the fixed sidebar row
also animates while the conversation is busy. Rendering uses the
shared draw timestamp. Canonical visual rows carry optional durable anchors; the
activity row has none and replaces only the final durable blank separator. It
occupies one clipped row before viewport slicing, so older history scrolls it away
and returning to the tail reveals it. Focus, disclosure and durable scroll anchors
ignore unanchored rows. The context/error row keeps its full width, and review
requires three durable viewport rows in addition to any activity row.
The header reads `Operator · pi · provider/id` once a model has been selected and
`Operator · pi` before that; it never guesses an identifier from settings or a
catalog. Only visible Operator quiets the global title fill. The keymap-derived
footer prioritizes eligible Send, Newline, Browse and applicable Stop. Send is
disabled in the shared keymap/menu while the draft is empty or the conversation
cannot submit (including running, pending and unconfirmed-worker-stop states).
Secondary actions remain in navigation and menus.
At widths below 100, the ordinary sidebar opens on demand through `Ctrl-l` without
changing the saved preference. Below 60×16, Operator shows resize guidance and
preserves work. Unread review requires an unobscured, actually drawn, adequately
sized transcript at its confirmed durable tail; empty, too-new, too-small and
sidebar-only views cannot acknowledge it.
Enter sends, Shift-Enter inserts a newline, paste only edits, Ctrl-g or Esc returns
to the terminal panes in Browse retaining the draft, and Ctrl-x (or Ctrl-c in an
empty full composer) stops only the exact current turn. There is no `Ctrl-n`,
conversation switching, close confirmation or archived-Operator action. A pending
submission clears only its captured draft after acceptance; refusal or later typing
preserves the draft.

OperatorChanged is a coalescible dirty signal, never transcript content, and only
the singleton's id is tracked: legacy rows stay on disk but never enter this view.
Tracked requests load durable summaries and ordinal-paged history independently of
focus, with one read in flight and a further catch-up when dirtied during a read.
Only retained `worker_stopped` proof enables another submission; Complete without
that proof displays finishing/stop-unconfirmed. History commits completion Unread
only after a successful unobscured tail draw, with sufficient transcript geometry.
Model/tool strings escape terminal and directional controls before rendering,
including tool output with explicit empty-success results.

The read-only `dashboard::transcript` view replaces an alternate-screen History refusal
with provider-owned Claude/Codex conversation pages. Its loading and loaded states use
`Details::Transcript`, ahead of terminal and Operator shortcuts. Captured session/run,
request id and exact `ConversationReference` fence page application; cancel, a changed
run and superseding End requests retire their pending responses. Text uses Operator's
control escaping. The byte/entry-bounded window slides toward older pages, while End
reloads the tail; rendering skips whole wrapped logical lines before its u16 scroll
remainder so long views reach their actual tail. No page causes input, review or launch.

The Operator's `Ctrl-t` and clickable Reasoning header open one supported-level
picker, including `default`. Enter sends the shared `operator.thinking_level`
setting write, Esc closes, and the header distinguishes Pi's substituted actual
level from a saved choice awaiting the next turn. Controls use a second header
row when the title and controls cannot fit and at least 50 content columns remain;
a tighter on-demand sidebar view prioritizes controls.
