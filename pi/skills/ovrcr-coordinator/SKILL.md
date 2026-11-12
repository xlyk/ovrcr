---
name: ovrcr-coordinator
description: Coordinate parallel coding or research through visible OVRCR Workspace Agent sessions, register watches, and verify asynchronous results.
---

# Coordinate OVRCR sessions

Use this workflow for work that needs multiple Workspace Agent sessions. For a
quick inspection or one small step, use Operator's CLI directly.

## Requirements

Operator supplies the OVRCR CLI and the `watch_sessions` tool. Run instance
commands as `"$OVRCR_EXECUTABLE" --json ...`; the inherited instance and socket
select the user's live OVRCR Server. Verify command output and honor refusals.

## Installation

This skill is bundled with OVRCR and loaded explicitly for Operator. No project
or personal skill installation is needed. Check the available CLI with
`"$OVRCR_EXECUTABLE" --version` and `"$OVRCR_EXECUTABLE" --help`.

## Usage

### Dispatch work

1. Choose the route. Use Operator's Bash for a quick, single-step CLI task.
   For parallel coding, put each independent task in its own Git worktree
   Workspace on a dedicated branch. The Root workspace is protected; do not use
   it for delegated edits. Share a Workspace only for read-only research or
   when the user explicitly requests concurrent work on the same branch.
2. Reuse suitable running Retained sessions or create Agent sessions in those
   Workspaces. A typical worktree command is
   `"$OVRCR_EXECUTABLE" --json workspace create --project PROJECT --new-branch BRANCH --base BASE`.
   Start a managed Agent with `terminal create` and `ovrcr agent run PROVIDER --`
   around its native executable, or use the Dashboard Agent picker. Do not
   assume a shell placeholder is an Agent session.
3. Before sending each task, read `"$OVRCR_EXECUTABLE" terminal list --json`.
   Match the intended session by its id and Workspace. Require `scope ==
   "Workspace"`, a valid `kind.agent`, and `phase == "running"`. If setup,
   trust, or approval input is unexpected, stop and ask the user; do not guess
   keystrokes.
4. Save the session's pre-send baseline: numeric `run`, numeric
   `agent.activity_revision`, and `unread.turn` exactly as returned (an opaque
   string or `null`). Do not parse or increment the unread turn. Pass it to
   `watch_sessions` as `baseline: {run, activity_revision, unread_turn}`; the
   tool's `unread_turn` field maps to CLI `unread.turn`.

   ```json
   {"action":"add","sessions":[{"session_id":42,"project":"example","label":"agent","baseline":{"run":7,"activity_revision":12,"unread_turn":null}}]}
   ```
5. Send each task with `terminal send SESSION_ID --text MESSAGE` (Paste). Ask
   for a final trailer in this form:

   ```text
   OVRCR_WORKER_DONE
   summary: what changed
   files: paths changed
   checks: command and exit code, or none requested
   ```

   If blocked, ask for:

   ```text
   OVRCR_WORKER_BLOCKED
   reason: the concrete blocker
   need: the information or decision required
   ```

6. After sending, call `watch_sessions` with `action: "add"` for every task,
   using the captured session id, project, label, and baseline. Then return a
   compact Task / Workspace / Session table and finish the turn immediately.
   Do not wait or poll in a shell; watched sessions keep Operator leased and
   the composer available for follow-up messages. Use `list` to report watches
   or `remove` when the user asks to stop watching. Removing a watch does not
   stop its Workspace session.

### Handle wakeups

On `AsyncWakeup`, use the reported session ids and current `terminal list
--json` rows to choose a provider read path:

- Claude Code and Codex: `terminal transcript SESSION_ID --json`. If a transcript
  entry is truncated at the 16 KiB cap, read the terminal tail with
  `terminal read SESSION_ID --max-lines N`.
- Pi and Oh My Pi: `terminal read SESSION_ID` or `terminal screen SESSION_ID`.

Read the full relevant reply once. If it omits the trailer, summarize what it
actually says and note that the marker was absent; do not resend the task just
to obtain formatting. A routine factual blocker can be answered within the
user's existing scope: capture a fresh baseline, use `terminal send`, then
register that session with `watch_sessions` again. Escalate human decisions and
`waiting_input` prompts as a concise question that identifies the session.

For coding tasks, work in the matching Workspace and verify `git status
--short --branch`, `git diff --stat`, and each reported check's actual exit
code before calling the result ready for review. A Ready observation or a
`OVRCR_WORKER_DONE` trailer alone does not prove success. Leave finished
Retained sessions open in the Dashboard unless the user asks to close or
archive them.

## Troubleshooting

If `watch_sessions` rejects a session for missing Reporter lifecycle data,
report that the session cannot be watched and use a managed Agent launch when
the user wants to continue. Do not infer completion from terminal silence or
switch that session to read-only polling.
