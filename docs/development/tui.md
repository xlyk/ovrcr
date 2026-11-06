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
Use one-digit codes for one through nine visible targets and zero-padded
two-digit codes for ten through fifty. Capture the code width and mapping on an
observed standalone Super press; a two-digit prefix may emphasize matching
badges and appear in the footer without changing row geometry. Hide labels on
selection, and keep the normal Agents search available when the host does not
deliver modifier events. A selected running destination remains in Browse until
both the held modifier is released and the existing view/input boundary is
drained. Shortcut events, release events, and queued loading input must never
become child bytes. Opening an overlay, losing focus, scrolling, or resizing
cancels the hold.

Enhanced keyboard reporting belongs to the existing `TerminalGuard` lifecycle.
Request its modes at Dashboard entry, restore the owned mode on normal exit,
interrupt, error, and panic, and preserve the user's prior mode by popping only
the push this Dashboard owns. A reply to a capability query is not proof that a
host delivers the required Command/Super press, modified digit, and release.
Command-hold session jump is enabled only for an explicitly qualified frontend:
the bundled OVRCR GUI supplies `DashboardInputProfile::VerifiedOvrcrGui` after
its native shortcut path has been verified. The CLI and other frontends default
to `Standard`; terminal keyboard-mode negotiation alone does not qualify them.

Readiness is derived by `ViewHandshake::is_ready`; pane changes go through `retarget(PaneChange)`; outbound requests go through the outbox and are drained by `drain_outbox`. Session status — the glyph, the plain-words state (with its narrow form, a bare `Unread` when there is one), the reporting words (`starting…`, `status off`) and elapsed clauses the pane headers read, and the activity, Unread and health clauses the Agents picker reads — is decided once by `dashboard::status::SessionStatus`, which the sidebar row, both pane headers, the picker and the spinner redraw cadence read; each surface still arranges and clips the clauses for its own width (`render::pane_header` gives up the pid, model, elapsed time and label before the state), and readiness itself is still decided in `dashboard::ready`. Quality words and reporter reasons never reach the headers. The Ready checkmark is drawn only while that response is Unread; after review the glyph is blank and the header still says Ready. The sidebar agent label appends the current model after a colon when one is known; on a shared line the title keeps its room first, so the model goes, then the agent, before the title clips. A session row is one or two sidebar lines: `render::tree_row_heights` decides, for the whole visible tree at the current sidebar width, whether every agent label fits inline or every one takes a second line, and rendering, scrolling, selection visibility and mouse hit-testing all read those heights through `tree_line_at`, `tree_line_count` and `tree_row_start`, so the extra line always resolves to the same session.
Key bindings live in one table, `dashboard::keymap`: it holds each key's label, action and the reason it is unavailable, and `key_action`, the footer, the key popup and the palette all read it rather than repeating a guard. The palette applies the popup's `shown_in_group` rule: it drops bindings the target cannot use and keeps waiting or protected rows with their reason, so the list never shifts under the cursor when a screen is acknowledged. The title bar's `[Commands]` control opens the palette. Split (`v`) reuses the palette's Search page as the new pane's session chooser (`Palette::split`), and palette setting rows open the editor through `Editor::select`.

Palette forms have one Enter rule: Enter submits from any field after `form_problem` checks the visible fields top to bottom (required, pick value in its list, Branch after the prefix and valid under `git check-ref-format --branch` via `branch_name_problem`); the first problem takes the focus and is drawn under its field from `Palette.invalid`. A pick field's value is its highlighted option (`accept_pick` after every arrow, cycle or filter edit). An open list gets the body rows its field leaves free, through `PickList::window`, which rendering and mouse hit-testing share. `form_keys` builds the footer from the form and its focused field. New fields (a Task field, say) get these rules without per-field Enter handling.
A Lifecycle job submitted from the palette carries that palette on its Provisional row while the job runs (`take_palette_for_job`). On failure, `fail_lifecycle_job` returns it through `return_to_form`, which replays the failure through `palette_response` exactly as a synchronous refusal, so recovery pages (RecoverLaunch, WIP save, discard) are shared. Only when the form cannot come back (another overlay, or a non-Browse mode) does a failed row stay. Workspace removal with running sessions sends CloseTerminal for each and then the removal, chaining each request from the previous `LifecycleCompleted` through `Provisional.then`. Create workspace with a Task uses the same chain: `CreateWorkspaceWithLaunch` carries no launch (its `SessionLaunch` has no session name) and the named `CreateSession` follows (`chain_session`, which also marks the Provisional `launch` so an early empty workspace is not attached as finished). A failure of that second step returns with `after_workspace` and is handled as `PartialFailure`, so it reaches RecoverLaunch rather than a form that would create the workspace again. The Task reaches an agent only through `agents::with_task` (`-- <task>` for `TASK_AGENTS`); it is never in the remembered launch choice or retained for RecoverLaunch. Retry and resume use only the provider's recovery options, so the first message cannot be replayed.
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
state; Repeat/Release and all modal mouse events are consumed. Its frozen
generation/context/defaults follow a request-owned Create → Submit continuation,
using the acknowledged conversation's retained `summary.run`. Esc hides submitted
work, and only a still-visible matching generation may navigate on acceptance.
The quick dialog starts at seven rows (two editor rows), grows with wrapped input
and an optional error row to sixteen, and keeps its eighty-column cap. Errors and
pending phase hints remain visible; shrinking the form cannot forward modal clicks.
Failures retain the message; retries after Create target that chat. Notification
retarget cancels an unsent form or hides its pending continuation. Quick visibility
also blocks durable-tail review acknowledgement.

Full Operator message input and editable Quick Message call
`TextCursor::edit_key(KeyEvent)` for line/grapheme/word editing; other fields keep
`TextCursor::key(KeyCode)` unchanged. Ctrl+A/E and Ctrl+U/K use logical line edges,
Ctrl+W and Alt+B/F use whitespace-delimited words without crossing newlines.
Home/End remain whole-buffer. Modifiers match exactly; unsupported combinations
are consumed without inserting letters. Full input repeats ordinary edits, while
Quick retains Press-only editing. Each owner handles Ctrl+C itself: a Press clears
literal nonempty text and resets the cursor; a separate empty full Press calls the
existing exact selected Stop. Repeat/Release do neither, empty Quick is a no-op,
and pending Quick/closing chats stay frozen. Quick edits must reach the existing
seeded-retry synchronization without changing unseeded chat drafts or captured
Submit/Create payloads. The native GUI's `encode_event` suppresses known repeated
Ctrl+C before raw byte 3 loses repeat identity; first Press still sends byte 3.
Control-only bytes and ordinary repeat stay unchanged. Alphabetic Control keys
with Alt or Shift retain those bits using the existing CSI-u spelling (lowercase
ASCII codepoint, modifier parameter 1 + Shift 1 + Alt 2 + Control 4), so unsupported
composer chords cannot become destructive Control-only input. The shared terminal
encoder remains unchanged. Legacy external terminals without repeat metadata can
report every held byte 3 as a fresh Press, so they cannot distinguish a hold from separate
presses. No timing heuristic or keyboard-protocol redesign is applied.

Retained rows stay selectable without allocating a parser on the server or
inventing live timing. Reopen targets the existing row and expected run; Retry
never implies ownership acknowledgement. Same-boot or unverifiable recovery
requires a separate explicit confirmation that old agents and background
processes have stopped. Agent rows without a conversation reference open the supported provider's native
resume picker. Unknown providers and invalidated references remain unavailable.


Operator conversations occupy one-line rows above Projects in the existing sidebar.
`SidebarRects` supplies the Operator, project-tree, attention strip and quota rectangles
to rendering, mouse input and viewport calculations. Operator and the tree start at the
top; the strip sits directly above the quota section at the bottom, so its coming and going
never moves a row under the pointer. The Operator heading and three project-tree lines
are reserved first; the strip takes one line, and the quota section uses its full provider bars,
compact provider rows, a one-line details pointer, or hides as space shrinks, and neither depends on the clock. Up to five conversation rows use the remaining space and shrink first.
A server update that brings the strip in or out keeps the selection in view. Collapsing keeps the heading and active-conversation count.
Rows share the session activity glyphs, selected background/bar and hover close mark,
and omit provider/model metadata. Archived chats remain cached as authority tombstones
but never appear in the active list. Saved summaries load asynchronously after the
production handshake and initial terminal attachment, without opening Operator.

Browse `o` and the clickable title-bar control open the native hub. Terminal pane
assignments survive, but the desired terminal view is empty while the hub is visible
and native composer input never produces PTY bytes. Leaving the hub (Ctrl-g or Esc)
hides it through `retarget(PaneChange::Focus)`, so the next view request
re-subscribes the unchanged panes. Existing overlays retain input
priority; spaces and punctuation in an active composer remain ordinary text.

`dashboard::operator` owns per-conversation drafts, context, grapheme cursor,
scroll/follow-tail and completion Unread state. Selecting a saved conversation never
recaptures the last terminal's context. Scrolled transcripts keep a durable message
ordinal, logical part and byte anchor
across streaming, receipt expansion, conversation switches and resize. Styled
graphemes survive wrapping; the Working journal reserves a two-cell gutter before
wrapping and decorates only after source-byte anchor calculation. User prompts
have a mauve cue; assistant/tool activity shares a muted guide. Prose and
command/output details use the available transcript width after the gutter.
Adjacent tool calls and outputs stay separate disclosures under their original
ordinals, without blank rows or collapsed raw IDs; expanded details retain IDs. Successful completion
Unread comes from the exact durable turn summary; SDK model history stays private.
`Tab` cycles sidebar, transcript and composer; `Ctrl-l` toggles sidebar/composer.
Transcript Up/Down selects a user-context/tool entry; Enter expands its full details.
The borderless composer uses `clamp(wrapped_draft_rows, 2, min(6, body_height / 3).max(2))`
text rows and scrolls around the real cursor. One-cell edge padding, a mauve top
rule and muted bottom rule define the editor. `OperatorLayout` owns header,
transcript, context, writable composer and both rule rectangles for rendering,
hit-testing and review. The target uses the context row above the top rule.
A single transient journal row follows the latest durable message or receipt,
aligned after the same two-cell gutter: Sending before submission acceptance,
Responding throughout Running (including silence), or Waiting for a recorded
current-turn tool result. Tool waits require caught-up history and match
call IDs within the current turn; paging/dirty history falls back to Responding.
Stopping/Closing remain explicit and terminal work without stop proof says static
Stop unconfirmed. Confirmed terminal work removes the indicator. The private
derived activity decision also requests the shared 100 ms spinner cadence while
the journal is visible, independent of sidebar visibility. Rendering uses the
shared draw timestamp. Canonical visual rows carry optional durable anchors; the
activity row has none and replaces only the final durable blank separator. It
occupies one clipped row before viewport slicing, so older history scrolls it away
and returning to the tail reveals it. Focus, disclosure and durable scroll anchors
ignore unanchored rows. The context/error row keeps its full width, and review
requires three durable viewport rows in addition to any activity row.
The local header shows Operator/provider/model/state only when the selected chat's
row is rendered in the shared sidebar. Collapsed, offscreen or absent rows retain
the clipped chat title, shortening model metadata first. Only visible Operator
quiets the global title fill. The keymap-derived footer prioritizes eligible Send,
Newline, Browse and applicable Stop. Send is disabled in the shared keymap/menu
while the draft is empty or the selected chat cannot submit (including running,
pending and unconfirmed-worker-stop states). Secondary actions remain in navigation
and menus.
At widths below 100, the same combined sidebar opens on demand without changing the
saved preference. Below 60×16, Operator shows resize guidance and preserves work.
Unread review requires an unobscured, actually drawn, adequately sized transcript
at its confirmed durable tail; empty, new, too-small and sidebar-only views cannot
acknowledge it.
`Ctrl-n` opens creation, Enter sends, Shift-Enter inserts a newline, paste only edits,
Ctrl-g or Esc returns to the terminal panes in Browse retaining drafts, and Ctrl-x
stops only the selected exact current turn. Provider names display through
`OperatorProvider::label` (`ChatGPT`, `Grok`); `as_str` stays the saved identifier. A pending submission clears only its captured draft after acceptance;
refusal or later typing preserves the draft.

Sidebar `X` and `[x]` open the native confirmation for that conversation's retained
run. Confirmation explains uncertain/live work and draft discard, then sends the
Operator Close request rather than workspace WIP-save. Only that chat is frozen
while close is pending. Cancel or error retains its draft and row; acknowledged
archive clears the confirmed draft and removes its active row without stealing a
newer selection. Ordinary `x` in either composer remains text. Sidebar focus,
collapse and viewport movement never acknowledge completion Unread.

OperatorChanged is a coalescible dirty signal, never transcript content. Tracked
requests load durable summaries and ordinal-paged history independently of focus,
with one read per conversation in flight and a further catch-up when dirtied
during a read. Only retained `worker_stopped` proof enables another submission;
Complete without that proof displays finishing/stop-unconfirmed. History commits
completion Unread only after a successful unobscured tail draw, with sufficient
transcript geometry. Model/tool strings escape terminal and directional controls
before rendering, including tool output with explicit empty-success results.

The read-only `dashboard::transcript` view replaces an alternate-screen History refusal
with provider-owned Claude/Codex conversation pages. Its loading and loaded states use
`Details::Transcript`, ahead of terminal and Operator shortcuts. Captured session/run,
request id and exact `ConversationReference` fence page application; cancel, a changed
run and superseding End requests retire their pending responses. Text uses Operator's
control escaping. The byte/entry-bounded window slides toward older pages, while End
reloads the tail; rendering skips whole wrapped logical lines before its u16 scroll
remainder so long views reach their actual tail. No page causes input, review or launch.
