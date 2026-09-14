# Pi reporting setup

Pi needs no settings changes. `ovrcr agent run pi -- pi [ARGS...]` inside an OVRCR
terminal, or picking `pi` in the Dashboard, supervises the launch, materializes the
owned reporting extension into a private per-invocation directory, and adds
`-e <path>` beside your own extensions. A plain `pi` launch stays untracked. Nothing
under `~/.pi` is read or written by OVRCR.

## What is reported

| Pi event | OVRCR |
| --- | --- |
| `session_start` (startup, resume, fork, new, reload) | Idle; the conversation is bound to Pi's session id. Historical responses never become Ready. |
| `agent_start` | Busy. Retries, automatic compaction and queued follow-ups stay in the same response cycle. |
| `agent_end` | The cycle's outcome is recorded from the last assistant message (a tool error alone is not a failed run). |
| `agent_settled` | Ready `· confirmed` for a successful cycle, Error for an assistant error, Idle for an aborted or empty cycle. One Unread per Ready. |
| `ui_prompt_start` | An input request opens: the session waits. The kind (`select`, `confirm`, `input`, `editor`, `custom`) is reported; the title is not. Pi coalesces nested prompts into one outer span, so one dialog is one request. |
| `ui_prompt_end` | The request closes and the activity underneath it — Busy, Idle, Error or Ready — is restored. Closing is never Unread and never a review. |
| `session_tree` | The response cycle in progress is abandoned: activity becomes Idle when Pi's own API says the session is idle and Unknown otherwise. No historical response is replayed as Ready. |
| `session_shutdown` | `quit` ends reporting; replacement or reload publishes an open input request closed against the retiring binding — restoring the activity underneath it — then retires the producer so the next `session_start` re-admits. Either way a request never outlives its producer, and the retired producer's later frames are ignored for good. |
| `/ovrcr-reattach` | The command the extension registers: it re-announces this producer from inside the already-loaded extension, as a fresh reporting generation, and reports in Pi whether reporting came back. Activity comes from Pi's own idleness: a reattach during a response publishes Unknown, not Idle, and does not restore that response's cycle — the response in flight will not become Ready, and the next prompt reports normally. |

Payloads carry identifiers and discriminants only: never prompts, responses, tool
arguments or results, titles or credentials.

## Doctor

`ovrcr agent doctor pi --json [--session ID] [--executable PATH]` reports:

- `version` and `version_status`: `tested` when the release is one this repository
  has exercised (`tested_versions`), `unverified_compatible_until_proven_otherwise`
  for any other release, `unknown` when the bounded `--version` probe fails.
  Ordinary upgrades stay enabled; there is no allowlist.
- `session_status`: `not_requested` without `--session` (or an inherited
  `OVRCR_SESSION_ID`); `inspection_unavailable` when the server cannot be reached;
  `session_not_found` for a stale id; `unbound` until the extension delivered
  `session_start`; `bound` afterwards with `binding` and `lifecycle`.
- `lifecycle.delivery`: `bound_without_activity` until anything is observed,
  `observed` while reporting is live, `paused_recoverable` while the reporter is
  paused and a boundary or `/ovrcr-reattach` can recover it, and `lost` when the
  reporter is gone and only a fresh managed launch restores it. Also `activity`
  (the effective activity `terminal list` reports, so `WaitingInput` while a dialog
  is open), `input_requests` (the open requests' kinds, oldest first), `work_seen`,
  `health` (`Unavailable` while paused, with the pause reason, and after reporter
  loss), `unread`.
- `capabilities`: `reporting` and `approvals` are available; `questions` is
  `unavailable_no_provider_surface` for Pi, which has no question surface beyond
  its dialogs, and `available_tool_lifetime` for Oh My Pi, whose question request
  spans the ask tool's execution rather than the dialog's visibility; `recovery`
  is available for both (a source boundary or `/ovrcr-reattach`) and `metrics` is
  absent by design.

## Removal

There is nothing to remove: the extension lives only in the invocation's private
directory and is deleted when the invocation ends. Stop using the managed launch to
stop reporting.

## Boundaries

Native macOS evidence for this setup is recorded in
[research/issue-92-pi-native](../research/issue-92-pi-native/README.md); the support record's
"Native verification" section says what it proves, what stays open, and how it was isolated.

`WaitingInput` is extension-dialog-only: Pi exposes no other visible question surface,
and a tool call is not an input request. OVRCR does not open dialogs of its own to
probe for one. A long-running response is never declared failed because no end event
has arrived.

A session replacement (new, resume, fork) rebinds the conversation now in the foreground
as a fresh reporting generation; an extension reload re-admits the same conversation and
keeps its generation; compaction inside one conversation changes nothing; tree navigation drops the cycle it was in. When what this reporter delivered
stops being certain — a hole in its source sequence, a producer replaced without its
shutdown being observed, its own queue overflowing, or a transition naming a
conversation OVRCR is not bound to — reporting pauses rather than ends: health goes
`Unavailable` with that reason, the request set is cleared, the native session keeps
running, and nothing is applied until the next `session_start` or `agent_start`, or
`/ovrcr-reattach`, recovers it. Recovery restores nothing: activity comes from Pi's own
API, Ready waits for a genuine `agent_settled`, and no alert is replayed. An unread
response the server already owned survives. Nothing is ever inferred from idleness,
silence, empty queues or elapsed time, and a lost lease still needs a fresh managed
launch. Native acceptance evidence is recorded in
[agent-reporting-support.md](agent-reporting-support.md).
