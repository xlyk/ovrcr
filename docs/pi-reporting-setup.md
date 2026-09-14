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
| `session_shutdown` | `quit` ends reporting; replacement or reload retires the producer and the next `session_start` re-admits. |

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
- `lifecycle.delivery` (`observed` once any activity arrived), `activity`,
  `work_seen`, `health` (`Unavailable` after transport loss or a retired producer),
  `unread`.
- `capabilities`: `reporting` is available; `input_requests` (#90), `recovery`
  (#91) and `metrics` (absent by design) are stated explicitly.

## Removal

There is nothing to remove: the extension lives only in the invocation's private
directory and is deleted when the invocation ends. Stop using the managed launch to
stop reporting.

## Boundaries

Extension dialogs (`WaitingInput`), session switches and reporter recovery are later
tickets. A long-running response is never declared failed because no end event has
arrived. Native acceptance evidence is recorded in
[agent-reporting-support.md](agent-reporting-support.md).
