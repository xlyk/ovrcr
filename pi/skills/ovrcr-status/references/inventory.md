# Inventory fields

`project list` returns registered Projects. `workspace list --project PROJECT`
returns their Workspace ids, current branch labels and absolute paths.
`terminal list` returns session metadata, not terminal content.

- Identify a session by numeric `id` and `run`. Its `workspace` is a display
  branch label; reconcile `project` and `cwd` with the registered Workspace path
  and id. Titles and branch names alone are not durable identities.
- `scope == "Workspace"` identifies Workspace sessions. Operator scope is an
  object carrying a conversation identity, not a Workspace or capacity slot.
- Live capacity counts Workspace rows whose `phase` is `running` or `paused`,
  across the whole instance. Both Agents and Terminals occupy slots. Report
  used/50 and remaining slots, separately from the Operator's dedicated slot.
  Retained stopped/exited/interrupted rows do not consume live slots.
- `activity` is observed Agent state only when reporting is available. Inspect
  `agent.health`, `reporting_unavailable` and phase together. Null Agent data,
  unavailable reporting or `unknown` activity means unknown, not idle or done.
  A Terminal without Agent reporting is not a failed Agent.
- `activity == "waiting_input"` and `agent.input_requests` identify attention
  needed from a human. List the affected session; inspection does not answer it.
- Non-null `unread` identifies a result awaiting review. Preserve it. Ready and
  `response_ready` describe a completed response cycle, not verified task success.
- Report recovery blockers and interrupted/exited phases as observed. Stopped
  process ownership and permission to restart are separate from activity; never
  acknowledge ownership merely to make a report look healthy.
- `last_activity_unix_ms` means Agent-state change or user input, not output,
  selection or focus. A missing time stays unknown. Age is not completion proof.

Finish a metadata overview without reading conversation bodies unless the user
requested an inspection that needs them. Missing or partial inventory must stay
visible as a gap, not an all-clear.
