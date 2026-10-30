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

Readiness is derived by `ViewHandshake::is_ready`; pane changes go through `retarget(PaneChange)`; outbound requests go through the outbox and are drained by `drain_outbox`. Session status — the glyph and the process, paused, activity, Unread and elapsed clauses, with an Unread replacing a header line outright — is decided once by `dashboard::status::SessionStatus`, which the sidebar row, both pane headers and the spinner redraw cadence read; each surface still arranges and clips the clauses for its own width, and readiness itself is still decided in `dashboard::ready`. The Ready checkmark is drawn only while that response is Unread; after review the glyph is blank and the activity clause may still say ready. The sidebar agent label appends the current model after a colon when one is known. A session row is one or two sidebar lines: `render::tree_row_heights` decides, for the whole visible tree at the current sidebar width, whether every agent label fits inline or every one takes a second line, and rendering, scrolling, selection visibility and mouse hit-testing all read those heights through `tree_line_at`, `tree_line_count` and `tree_row_start`, so the extra line always resolves to the same session.
Key bindings live in one table, `dashboard::keymap`: it holds each key's label, action and the reason it is unavailable, and `key_action`, the footer, the key popup and the palette all read it rather than repeating a guard.

`Ctrl-o` is reserved globally by `keymap::is_operator_quick_key`. Key dispatch,
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

Retained rows stay selectable without allocating a parser on the server or
inventing live timing. Reopen targets the existing row and expected run; Retry
never implies ownership acknowledgement. Same-boot or unverifiable recovery
requires a separate explicit confirmation that old agents and background
processes have stopped. Agent rows without a conversation reference open the supported provider's native
resume picker. Unknown providers and invalidated references remain unavailable.


Operator conversations occupy one-line rows above Projects in the existing sidebar.
`SidebarRects` supplies Operator, project-tree and quota rectangles to rendering,
mouse input and viewport calculations. The quota ladder reserves the Operator
heading and three project-tree lines; up to five conversation rows use remaining
space and shrink first. Collapsing keeps the heading and active-conversation count.
Rows share the session activity glyphs, selected background/bar and hover close mark,
and omit provider/model metadata. Archived chats remain cached as authority tombstones
but never appear in the active list. Saved summaries load asynchronously after the
production handshake and initial terminal attachment, without opening Operator.

Browse `o` and the clickable title-bar control open the native hub. Terminal pane
assignments survive, but the desired terminal view is empty while the hub is visible
and native composer input never produces PTY bytes. Existing overlays retain input
priority; spaces and punctuation in an active composer remain ordinary text.

`dashboard::operator` owns per-conversation drafts, context, grapheme cursor,
scroll/follow-tail and completion Unread state. Selecting a saved conversation never
recaptures the last terminal's context. Scrolled transcripts keep a durable message
ordinal, logical part and byte anchor
across streaming, receipt expansion, conversation switches and resize. Styled
graphemes survive wrapping; the Working journal reserves a two-cell gutter before
wrapping and decorates only after source-byte anchor calculation. User prompts
have a mauve cue; assistant/tool activity shares a muted guide. Prose is bounded
to 88 cells after the gutter while command/output details use the remaining width.
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
Ctrl-g returns to Browse retaining drafts, and Ctrl-x stops only the selected exact
current turn. A pending submission clears only its captured draft after acceptance;
refusal or later typing preserves the draft.

Sidebar `x` and `[x]` open the native confirmation for that conversation's retained
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
