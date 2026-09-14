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
| `session_shutdown` | `quit` ends reporting; replacement or reload publishes an open input request closed against the retiring binding — restoring the activity underneath it — then retires the producer so the next `session_start` re-admits. Either way a request never outlives its producer. |

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
- `lifecycle.delivery` (`observed` once any activity arrived), `activity` (the
  effective activity `terminal list` reports, so `WaitingInput` while a dialog is
  open), `input_requests` (the open requests' kinds, oldest first), `work_seen`,
  `health` (`Unavailable` after transport loss or a retired producer), `unread`.
- `capabilities`: `reporting` and `approvals` are available; `questions` is
  `unavailable_no_provider_surface` for Pi, which has no question surface beyond
  its dialogs, and `available_tool_lifetime` for Oh My Pi, whose question request
  spans the ask tool's execution rather than the dialog's visibility; `recovery`
  (#91) and `metrics` (absent by design) are stated explicitly.

## Removal

There is nothing to remove: the extension lives only in the invocation's private
directory and is deleted when the invocation ends. Stop using the managed launch to
stop reporting.

## Boundaries

`WaitingInput` is extension-dialog-only: Pi exposes no other visible question surface,
and a tool call is not an input request. OVRCR does not open dialogs of its own to
probe for one. Session switches and reporter recovery are later tickets (#91). A
long-running response is never declared failed because no end event has arrived.
Native acceptance evidence is recorded in
[agent-reporting-support.md](agent-reporting-support.md).
