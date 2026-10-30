# Operator conversation activity

Kyle approved the Working journal appearance and explicitly requires visible
loading/progress while the assistant responds or tool results are pending.
This is a required addition to the same feature, preserving that appearance.

Use one compact activity indicator on the right of the existing context row,
immediately above the composer rule. Keep the target or current error on the
left, clipped to its remaining cells. This keeps progress visible while the
transcript is scrolled or the sidebar is hidden, without taking a transcript
row, changing durable anchors or reducing the 60×16 floor.

Wide: `Managing consigint / main / #1 run 1                 ⠋ Responding`

60 columns: `Managing consigint / main / #1…        ⠋ Responding`

## Behavior

- Reuse the app's existing themed braille spinner and100ms cadence, using the
  same draw timestamp. No timer thread, persisted progress state or dependency.
- A pending selected Submit says `Sending`. A current Running turn says
  `Responding`, including silence before the first delta and gaps in streaming.
- With current-turn, fully caught-up history, an unmatched ToolCall says
  `Waiting for <tool> result`; multiple pending calls say `Waiting for N tool
  results`. Match outputs by current turn and call ID, including nonadjacent,
  empty-success, failed and timed-out results. This reports a missing recorded
  result, not proof that a particular operation is executing. Escape and
  cell-clip tool names. During paging/read/dirty catch-up use `Responding`.
- Stopping and closing requests may show `Stopping`/`Closing`. A terminal turn
  without worker-stop proof uses a static `Stop unconfirmed` label, never
  claims the assistant or a historical tool is still responding. Stop-confirmed
  terminal/idle/archived chats have no live indicator. State is derived from
  existing selected-chat authority; another chat's progress cannot leak in.
- Text labels retain meaning without color. Use existing theme tokens; spinner
  color/frame follows the shared app mechanism. Keep errors readable on the
  left; activity must remain visible while active even if a retained error is
  present. Do not clear/expire the separately open stale error.
- The visible journal requests spinner redraws even with its sidebar hidden,
  collapsed or selected row offscreen. No new fast redraw for an invisible
  journal, new-chat form, too-small guidance or static stop-unconfirmed label.
  Existing independently busy sidebar/project surfaces retain their behavior.
- Clock-only redraws do not send requests or input, change history/anchors,
  acknowledge Unread, move the caret, steal focus, or mutate drafts/selection.
  All prior sidebar, Send guard, title fallback, Ctrl+O and lifecycle contracts
  remain unchanged. No protocol/runtime/auth/global palette changes.

## Approaches and verification

Controller choice: split the existing context row. A transient transcript row
would require a new effective viewport height in scrolling, hit-testing and
Unread paths; a sidebar-only spinner remains invisible when that section is
hidden. Neither is needed for this requirement. No percentages or ETA are
invented from indeterminate model/tool latency.

Prove state transitions and clock frames through actual Dashboard response,
history and draw paths; retain wide/narrow/floor geometry, target/error/caret,
source anchors and Unread tests. A controlled compiled-worker fixture must hold
silent Running and a real unmatched tool receipt before release. Native GUI
captures at distinct clock frames without input/server progress must prove
visible movement, and completion must remove it. Use private synthetic auth,
owned process/socket resources and original wait/cleanup receipts.
