---
name: ovrcr-cleanup
description: Preview stale-session candidates and unused Workspaces with exact proposed effects and blockers. Use for cleanup, housekeeping, old sessions, or reclaiming unused checkouts; this version proposes changes only.
---

# Preview OVRCR cleanup

## Installation and boundary

Bundled with the matching OVRCR binary for Operator on macOS and Linux. Use the
inherited `"$OVRCR_EXECUTABLE" --version`, instance configuration and socket;
no source checkout or personal skill installation is required.

This procedure is proposal-only. General cleanup and approval of a previous
proposal change nothing in this version: report **Cleanup execution unavailable**.
Never substitute generic close, kill, remove, ownership acknowledgement, Git
cleanup, or task cleanup for guarded approved execution. A separate direct user
instruction to close a named session belongs to `ovrcr-session-care`, not an
inferred approval of this sweep. Skill selection grants no authority.

## 1. Establish scope and observations

Default to all registered Projects in the current instance, even if captured
Dashboard context names one Workspace. Honor narrower Project, Workspace or
session requests; resolve identities from inventory and ask about ambiguous
names. State scope, observation time and threshold in the proposal.

Start with metadata, not every conversation:

```sh
"$OVRCR_EXECUTABLE" --json project list
"$OVRCR_EXECUTABLE" --json workspace list
"$OVRCR_EXECUTABLE" --json terminal list
"$OVRCR_EXECUTABLE" --json terminal list --archived
"$OVRCR_EXECUTABLE" --json run list
```

Use `watch_sessions` with `action: "list"` to inspect existing watches; retain
all watches unchanged. Match task run `workspace` ids and `directory` paths to
registered Workspaces, and inspect `assignment_control` on sessions. Any assigned
session stays protected whether control is Controller, Human or Uncertain.
Missing control, watch or task evidence is not proof of unowned work. Active
runs (Queued, Preparing, Running, Cancelling), interrupted ownership and unresolved
cleanup remain blockers; completed task status alone does not certify ownership.

Identify sessions by numeric `id` and `run`, Workspaces by Project, `id` and exact
registered `path`; the session's `workspace` is only a display branch label.
Exclude every non-Workspace scope, including Operator conversations. Keep
archived rows as ownership/history evidence, not new archive candidates.

## 2. Classify sessions

Use **24 hours** since `last_activity_unix_ms` by default. A request may override
that review threshold without changing settings. Record a single observation
clock and compare age >= threshold, including the exact boundary. Last activity
means Agent-state change or user input, not terminal output, focus or selection.
Null, unavailable or future timestamps have unknown age; never replace them with
start time, file mtime or an invented age. Below-threshold rows stay outside the
stale review; report unknown ages separately. Age selects review candidates,
never proves completion.

For each candidate, list all applicable protections before considering an effect:

- Busy or paused: preserve active or deliberately suspended work.
- Unread or WaitingInput / open `agent.input_requests`: preserve the result or
  surface the human question; inspection neither reviews nor answers it.
- Uncertain ownership (`recovery.requires_ack`, interrupted state, missing stop
  proof), active watches or task-owned work: preserve and explain the blocker.
- Missing/unavailable Reporter data or unknown activity: no inferred idle live
  Agent. Ready, a title such as "finished", and a completion trailer independently
  prove neither accepted completion nor safety.

Only an otherwise unblocked stopped record with resolved ownership can have the
proposed effect **archive stopped record** (hide from the active list, preserve
its Retained record and provider history). For an otherwise unblocked live Agent,
inspect the relevant reply using the provider-supported path in
`ovrcr-session-care`, verify review and idle evidence, and label the distinct
proposed effect **stop live Agent, then archive record**. A running Terminal is
not an idle Agent; leave it for an explicit session-control request. Unknown or
missing review/ownership evidence stays a blocker, not implicit approval.

## 3. Inspect Workspace candidates separately

Inspect registered paths read-only, including ignored files when checking history:

```sh
git --no-optional-locks -C PATH status --porcelain=v1 --untracked-files=all
git -C PATH rev-parse HEAD
git -C PATH branch --show-current
git -C PATH for-each-ref --format='%(refname:short) %(upstream:short)' refs/heads
git -C PATH log --oneline BASE..HEAD
git -C PATH log --oneline HEAD --not --remotes
```

Resolve BASE from observed repository policy; if unavailable, state that merged
status is unknown rather than guessing. No fetch, push, WIP commit, checkout,
clean, force, or mutation-as-dry-run. Local remote refs are observations, not a
fresh proof that work is published.

Root (the registered repository checkout), dirty or occupied Workspaces, active
task runs, unresolved ownership including archived rows, and provider-history
dependencies block removal. Check retained conversation evidence even when Git
is clean: ignored provider history is still valuable. Inventory may identify a
conversation without exposing its history location; report **provider-history
location unverified** and preserve the checkout until that dependency is resolved.
A reference can point outside its session's original cwd: inspect references
across the instance, including archived rows outside a narrowed action scope.
If a reference's location cannot be verified, do not infer that another unused
checkout is independent of it. Do not read private provider profiles or modify
the registry to fill the gap.
Git/identity/read failures also block a safe-removal inference.

For a clean, unused non-Root Workspace with resolved ownership/history evidence,
propose **remove Workspace checkout**, separately from any session action. Explain
unmerged and unpublished commits (or the evidence gap) even though branches stay.
An archive proposal never approves checkout removal.

## 4. Return the proposal, then stop

Each row names Project, Workspace id/path, session id/run where relevant,
observed phase/activity/Reporter and attention state, Last activity and age,
watch/task/ownership/history evidence, exact intended effect or preservation,
and every blocker. Distinguish candidates, protected resources and unknowns;
never present incomplete inventory as an all-clear.

State that no resources changed. Preserve processes, registry, Git files,
branches, Retained records, provider history, Unread, Input requests, watches and
ownership acknowledgements. Project unregistration, branch deletion and permanent
record deletion are outside a general sweep. Closing a cloud attachment is not
remote cancellation or proof that charges stopped.

Approval can name only selected listed identities and effects, never replacement
runs or future resources with reused names. In this proposal-only version even
that approval reports Cleanup execution unavailable; it performs no action and
starts no periodic check. On failed or partial inspection, retain uncertain
resources, identify the missing observations, and stop without replaying changes.
