# Human control of Pi assignments

Select an assigned child in the Dashboard to inspect it. Selection, focus,
split panes, resize, and closing a pane keep its current control owner. They do
not interrupt the child, archive it, or transfer authority.

The session status shows **Pi control · view only** while its parent owns input.
Open the action palette with `:` and choose **Take control**, or press `H` in
Browse mode. The Server transfers only the selected assignment. Its process
keeps running. Wait for **Human control**, then press Enter to focus the terminal
and type normally. `Ctrl-g` returns to Browse.

Assigned terminal input is available only through this owning Dashboard after
explicit takeover and an acknowledged focused view. Generic terminal send,
paste and named keystrokes remain disabled for assigned children during human
control. An unrelated ordinary terminal keeps its existing input behavior.

## Return an assignment to Pi

Press `Ctrl-g`, then choose **Return to Pi** in the palette or press `A`.
The Server revokes human input before acknowledging the action. Status becomes
**Pi return pending reconciliation**. Returning does not send an interrupt or
accept a result.

The parent inspects the assignment and explicitly confirms the returned state
through `ovrcr_confirm_return`, supplying `handle`, a distinct `request`, the
`returned_request` from the HumanReturn barrier, `expected_control_epoch` from
`control.epoch`, and exact fresh `control_evidence` as `evidence`. Inspect and Reconcile only observe state. They never
clear this barrier or resend input. If native questions, results, human input,
run or reporting binding change before confirmation, the captured evidence is
stale and must be reconciled again. A changed or uncertain reporting binding
suspends control and cannot be repaired by confirming an ordinary return.

Human edits invalidate only that assignment's current result attribution.
After return confirmation, the parent must issue a scoped follow-up that creates
a fresh result version before automating more work or recording acceptance.
Without human edits or other attributable changes, a passive takeover and return
preserve otherwise valid evidence. Existing Unknown or Partial input receipts
remain unresolved; return confirmation cannot clear them.

Both takeover and return advance a per-child control epoch. Every later input
must match that epoch. An old queued send stays refused after control returns to
Pi, including Pi-to-human-to-Pi transitions. Taking one child never changes a
sibling's authority or the workflow controller's own epoch.

Historical contracts, submissions, dispatch receipts and acceptance references
remain immutable. Late child results remain evidence and never become automatic
parent acceptance. No terminal contents are stored in control metadata.
