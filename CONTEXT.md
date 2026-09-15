# OVRCR

Synchronous terminal multiplexer. One server owner, one active dashboard, fifty sessions.

## Language

**Session title**:
The name shown for a terminal session. It can follow the running application's title or remain pinned to a name chosen by the user; changing it does not change which session it identifies.
_Avoid_: session identity, agent label, workspace name as interchangeable terms

**Automatic title**:
A session title that follows titles supplied by the running application, using a fallback when no application title is available.
_Avoid_: generated task summary, inferred activity

**Pinned title**:
A session title chosen by the user that stays unchanged until the user edits it or returns to Automatic.
_Avoid_: pinned session, which could imply a placement or lifecycle change

**Launch choice**:
The user's choice to start an Agent or a Terminal in a workspace. Creating a workspace can also leave it empty with Nothing yet.
_Avoid_: custom command as a third session category

**Agent**:
A detected or configured agent selected for launch. This launch choice alone does not establish reporting support or determine everything that may later run in the session.
_Avoid_: any arbitrary command as an agent

**Terminal**:
The launch choice for opening a shell, optionally with a custom command. Like an Agent launch, it creates a terminal session with automatic or pinned titles.
_Avoid_: custom command as a separate session type

**Remembered launch choice**:
A project's most recently successfully launched Agent and preset, or Terminal choice. It excludes one-off command text and is unchanged by creating an empty workspace.
_Avoid_: running agent, silently substituted fallback

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

**Reporter**:
The in-process owner of one managed invocation's reporting: the lease, the binding and Reporting generation that lease carries, the one revision set every observation is numbered from, the retained identities it fences, and the teardown. One per invocation, shared by every provider; the provider's receiver translates that provider's frames into observations and chooses which of these the provider needs.
_Avoid_: the provider's receiver, the extension Producer, the helper process

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

**Session status**:
What one session says about itself on screen, decided once per draw: the row's glyph and the process, paused, effective activity, Unread and elapsed clauses. An Unread clause replaces a header line outright; each surface arranges and clips the rest for its own width. The sidebar row, both pane headers, and the spinner redraw cadence read the same value.
_Avoid_: the raw rollup activity, the glyph on its own, a per-renderer reading

**Retarget**:
Any change to what a pane shows: split, focus, close, select, container, cleared, resize, or server removal. Each names which releases apply.
_Avoid_: calling the release steps individually

**Outbox**:
The Dashboard's queue of requests to send, drained once per event-loop pass in push order.
_Avoid_: private slots that hold a request
