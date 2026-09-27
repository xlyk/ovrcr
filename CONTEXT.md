# OVRCR

Synchronous terminal multiplexer. One server owner, one active dashboard, fifty sessions.

## Language

**Retained session**:
A session whose identity and workspace placement remain available after its process ends or the server restarts. Agent sessions retain a reference to their provider-owned conversation; both Agent and Terminal launches have retained sessions.
_Avoid_: live process, running terminal as interchangeable terms

**Archived session**:
A retained session the user has closed and removed from the active list without deleting its record or the agent's own conversation history. Process exit and an agent's suggestion to close do not themselves archive a session.
_Avoid_: deleted session, exited process, closed pane

**Conversation resume**:
An explicit user action to continue an agent's previous conversation where the provider supports it.
_Avoid_: restoring history, reattaching to a live process, automatic execution

**Conversation reference**:
The provider identity and unique conversation identifier needed to reopen an agent's saved conversation.
_Avoid_: transcript, terminal output, most recent conversation

**Session title**:
The name shown for a session. Applications cannot change it, and changing it does not change which session it identifies. A Terminal shows a manual title or the original session name. An Agent launch may show a conversation subject until the user renames the session.
_Avoid_: session identity, agent label, workspace name, application title

**Original session name**:
The name chosen at creation, supplied by the user or generated from the workspace. Clearing a manual title restores this name, even if the workspace's branch has since changed. On an Agent launch, a name typed at creation is this name and is not a manual title.
_Avoid_: current branch name, application title, conversation subject

**Manual title**:
A session title the user sets with Rename, or a name typed when creating a Terminal. It stays until the user edits it or clears it, and it survives reconnect, server restart, and reopen. Clearing it restores the original session name and dismisses the subject of the conversation attached at that moment.
_Avoid_: Automatic mode, conversation subject, pinned session (which could imply a placement or lifecycle change)

**Conversation subject**:
A short topic for one agent conversation. It may be shown as the session title until the user renames the session or dismisses that subject. It is not the original session name.
_Avoid_: automatic title, application title, manual title, session identity

**Launch choice**:
The user's choice to start an Agent or a Terminal in a workspace. Creating a workspace can also leave it empty with Nothing yet.
_Avoid_: custom command as a third session category

**Agent**:
A detected or configured agent selected for launch. This launch choice alone does not establish reporting support or determine everything that may later run in the session.
_Avoid_: any arbitrary command as an agent

**Terminal**:
The launch choice for opening a shell, optionally with a custom command. It creates a terminal session with a stable session title.
_Avoid_: custom command as a separate session type

**Remembered launch choice**:
A project's most recently successfully launched Agent and preset, or Terminal choice. It excludes one-off command text and is unchanged by creating an empty workspace.
_Avoid_: running agent, silently substituted fallback

**Workspace**:
A project's registered Git checkout. Its user-facing name is the current branch of that checkout; changing the branch does not replace the workspace.
_Avoid_: workspace title, an arbitrary name besides the current branch, treating a branch change as a different workspace

**Root workspace**:
The workspace that is the repository checkout itself, required to stay on the repository's default branch. It cannot be removed, and OVRCR never changes Git to restore that branch.
_Avoid_: main, root as a stored name, a removable default worktree

**Dashboard**:
The one active TUI for live sessions: the hierarchy, the panes, and browse / terminal / history / copy.
_Avoid_: TUI (the crate), GUI helper, task UI as a separate product

**Server**:
The one process that owns the socket, the sessions, and the live terminals.
_Avoid_: backend API server, a second server owned by the Bridge

**Bridge**:
The one macOS app that holds the bundle identity and performs system calls the terminal cannot make under its own name. It is not a Dashboard, not a Reporter, and not an owner of sessions or panes. It may ask iTerm to select an existing session. It does not create iTerm sessions.
_Avoid_: helper app, companion app, GUI helper, Notifier as a separate product, an owner of iTerm tabs, a second Server

**Key binding**:
The dashboard's one entry for a key in an input mode: its label, its action, and the reason it is unavailable. Dispatch, the footer, the key popup and the palette read that one entry, so a hint cannot disagree with what the key does.
_Avoid_: the reporting binding, a hint list per consumer

**Keystroke**:
A key delivered to the program inside a named session, as that program would receive it from a keyboard.
_Avoid_: Paste, Input request, Key binding

**Paste**:
Text delivered into a session as a paste, which may be followed by Enter.
_Avoid_: Keystroke

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
