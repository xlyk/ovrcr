# Dashboard behavior

- Revoke input permission immediately when focus, assignment, or visible geometry changes. Input requires a Running session in the current acknowledged visible view, using that pane's terminal modes.
- Match snapshots by request, revision, session and run. Require every expected snapshot plus the matching final acknowledgement before enabling input. An empty view expects zero snapshots and must still complete.
- Keep one view request in flight and coalesce desired changes. A true no-op must preserve readiness. Stale responses must not populate reassigned panes, clear current errors, or enable input.
- Use the same geometry for subscriptions, rendering, hit-testing, and cursor placement. Hidden panes retain their assignments and installed screens but cannot receive input. Never send zero PTY dimensions.
- Keep History/Copy tied to the captured session and run. Changing either cancels pending capture requests, selection jobs, and queued clipboard completions. Removing an unrelated pane must preserve a valid focused capture. A new run clears the parser, input readiness and Unread but preserves pane assignment and layout.
- Current-screen Copy cancels on resize through the actual shared client path. Frozen History retains its cells and wrapping while its viewport changes; live output must not rewrite the frozen capture.
- Preserve overlay input priority, bounded event batches, and the input-drain-before-clipboard-emission boundary.
- Treat literal spacing, terminal modes, Unicode width, colors, cursor placement, and accessibility text as behavior. Keep natural wide-glyph widths and clip at pane boundaries. A TestBackend pass alone may miss errors in emitted terminal coordinates.

Readiness is derived by `ViewHandshake::is_ready`; pane changes go through `retarget(PaneChange)`; outbound requests go through the outbox and are drained by `drain_outbox`. Session status — the glyph and the process, paused, activity, Unread and elapsed clauses, with an Unread replacing a header line outright — is decided once by `dashboard::status::SessionStatus`, which the sidebar row, both pane headers and the spinner redraw cadence read; each surface still arranges and clips the clauses for its own width, and readiness itself is still decided in `dashboard::ready`.
Key bindings live in one table, `dashboard::keymap`: it holds each key's label, action and the reason it is unavailable, and `key_action`, the footer, the key popup and the palette all read it rather than repeating a guard.

Retained rows stay selectable without allocating a parser on the server or
inventing live timing. Reopen targets the existing row and expected run; Retry
never implies ownership acknowledgement. Same-boot or unverifiable recovery
requires a separate explicit confirmation that old agents and background
processes have stopped. Agent rows without a conversation reference open the supported provider's native
resume picker. Unknown providers and invalidated references remain unavailable.
