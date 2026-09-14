# OVRCR

Synchronous terminal multiplexer. One server owner, one active dashboard, fifty sessions.

## Language

**Dashboard**:
The one active TUI for live sessions: the hierarchy, the panes, and browse / terminal / history / copy.
_Avoid_: TUI (the crate), GUI helper, task UI as a separate product

**Key binding**:
The dashboard's one entry for a key in an input mode: its label, its action, and the reason it is unavailable. Dispatch, the footer, the key popup and the palette read that one entry, so a hint cannot disagree with what the key does.
_Avoid_: the reporting binding, a hint list per consumer

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

**Producer**:
The admitted extension instance whose source sequence fences its events; retired by an observed shutdown or replaced by an admitted successor. In Oh My Pi one producer spans every in-place session switch, so a switch is a new binding, not a new producer.
_Avoid_: the conversation, the binding, the native process

**Reporting generation**:
The `generation` of a binding, bumped by every accepted Bind; a fresh one is admitted by the supervisor while the lease and session identity hold, and it starts with Unknown activity and no requests.
_Avoid_: the reservation epoch, a restart

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
