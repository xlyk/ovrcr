# OVRCR

Synchronous terminal multiplexer. One server owner, one active dashboard, fifty sessions.

## Language

**Dashboard**:
The one active TUI for live sessions: the hierarchy, the panes, and browse / terminal / history / copy.
_Avoid_: TUI (the crate), GUI helper, task UI as a separate product

**Ready**:
The accepted root observation of a completed response cycle from a supported readiness provider (Codex, Pi, Oh My Pi), identified by binding, cycle identity (the `turn` field), and activity revision.
_Avoid_: Confirmed activity as a success claim, Claude observations, a desktop alert

**Unread**:
The current Ready observation that has not been marked reviewed.
_Avoid_: treating view, selection, or notification as review

**Presented**:
The Unread the Dashboard last showed.
_Avoid_: a newer Unread as the review target

**Input request**:
An open request for a human answer, identified by binding, namespace and request identity. A binding carries a bounded set of them, published whole; while any is open the session's effective activity is WaitingInput, and closing the last one restores the underlying activity.
_Avoid_: Unread, a Ready observation, a tool call, or an alert as the request

**Active dashboard**:
The server-side seat the one Dashboard connection holds: its identity, outbound queue, acknowledged view, and focused geometry.
_Avoid_: the dashboard slot, the sink, the view subscription as separate things

**View handshake**:
The Dashboard's exchange with the server that acknowledges a view: one `SetView` in flight, snapshots matched by request, revision, and session, readiness granted by a complete final `Ok`.
_Avoid_: pane ready as a flag anyone sets

**Retarget**:
Any change to what a pane shows: split, focus, close, select, container, cleared, resize, or server removal. Each names which releases apply.
_Avoid_: calling the release steps individually

**Outbox**:
The Dashboard's queue of requests to send, drained once per event-loop pass in push order.
_Avoid_: private slots that hold a request
