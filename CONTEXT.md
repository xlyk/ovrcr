# OVRCR

Synchronous terminal multiplexer. One server owner, one active dashboard, fifty sessions.

## Language

**Dashboard**:
The one active TUI for live sessions: the hierarchy, the panes, and browse / terminal / history / copy.
_Avoid_: TUI (the crate), GUI helper, task UI as a separate product

**Ready**:
The accepted Codex root observation of a completed turn, identified by binding, turn, and activity revision.
_Avoid_: Confirmed activity, Claude observations, a desktop alert

**Unread**:
The current Ready observation that has not been marked reviewed.
_Avoid_: treating view, selection, or notification as review

**Presented**:
The Unread the Dashboard last showed.
_Avoid_: a newer Unread as the review target
