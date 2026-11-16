---
name: ovrcr-cleanup
description: Propose cleanup of stale sessions and unused Workspaces, then execute separately approved stopped-record archives and clean checkout removals. Use for housekeeping, old sessions, or reclaiming unused checkouts.
---

# Review and approve OVRCR cleanup

## Installation and boundary

Bundled with the matching OVRCR binary for Operator on macOS and Linux. Use the
inherited `"$OVRCR_EXECUTABLE" --version`, instance configuration and socket;
no source checkout or personal skill installation is required.

General cleanup is proposal-first: inspect, return exact proposed effects and
wait for approval. **Archive stopped record** and separately approved **remove
Workspace checkout** can execute. Live-Agent stop-and-archive remains **Cleanup
execution unavailable**. Never substitute generic close, kill, remove, ownership
acknowledgement, Git cleanup or task cleanup for guarded execution. A separate
direct instruction to close a named session belongs to `ovrcr-session-care`.
Skill selection grants no authority.

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
A Git run with retained `workspace` still owns that Workspace even when terminal.
A valid terminal Git run with `workspace: null` never acquired or has released
ownership: its historical `directory` alone must not protect a replacement there.
Nonterminal Git runs and retained Scratch directories remain protected. Malformed
or unavailable task evidence is uncertain, not clearance; other session guards
still apply after task ownership is released.

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
location unverified** until the Server's read-only check below verifies retained
references across the instance, including archived rows outside the action scope.
A reference can point outside its session's original cwd. Do not read private
provider profiles or modify the registry to fill gaps. Unregistered or otherwise
uncertain history evidence still blocks removal even after a successful check.
Git/identity/read failures also block a safe-removal inference.

All associated records must already be archived. Stopped active-list records need
separate archive proposals and approvals first; removal cannot archive them.
For each otherwise eligible exact path, obtain the original approval state:

```sh
"$OVRCR_EXECUTABLE" --json workspace removal-preview --project PROJECT --path PATH
```

This read-only command returns JSON binding Project repository, Workspace id,
path, Git generation, branch, HEAD and archived session runs. It checks Root,
cleanliness, occupancy, task/watch/assignment ownership and retained provider
history without changing anything. Record the entire JSON with the proposal.
A refusal leaves the candidate protected; do not substitute ordinary remove.

For a clean, unused non-Root Workspace with resolved ownership/history evidence,
propose **remove Workspace checkout**, separately from any session action. Explain
unmerged and unpublished commits (or the evidence gap) even though branches stay.
An archive proposal never approves checkout removal.

## 4. Return the proposal and wait

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
runs or future resources with reused names. Start no periodic check. On failed or
partial inspection, retain uncertain resources and identify missing observations.

## 5. Execute selected stopped-record approvals

Match approval to the original proposal's numeric session id, run and **archive
stopped record** effect. If approval is ambiguous, ask which listed actions; do
not refresh the run number into broader permission. Leave unapproved candidates
unchanged. Refresh inventory, watches (including queued reports), assignments and
task evidence. Changed runs, live processes, Unread, Input requests, Busy,
uncertain ownership and protected work stay unchanged; report their blockers.

For each still-eligible approved record, use only:

```sh
"$OVRCR_EXECUTABLE" --json terminal archive-stopped SESSION_ID --expected-run APPROVED_RUN
```

The Server checks the exact run, stopped ownership, attention, assignments, task
Workspace ownership and current watch protection in action admission. It never
stops a live process or acknowledges uncertainty. A refusal is not permission to
fall back to close, change the run or remove a watch. Missing watch protection
is an explicit uncertainty blocker, not proof that the session is unwatched.

Read active and archived inventory after each result. Archive advances the run
by one and preserves session id, original cwd, Project/Workspace context and
Conversation reference. Verify the original id appears archived with that next
run and is absent from the active list; leave provider history and checkout files
untouched. Do not confuse closing a cloud attachment with remote cancellation.

Report **verified completed**, **changed/skipped**, **refused**, and **uncertain**
actions separately. A timeout, disconnect or lost response may follow a committed
archive. Reconcile the original id/run against both inventories before deciding
what happened; never blindly replay, refresh approval to a newer run, or claim a
lost response proves no change. If disposition cannot be established, retain the
original approval and evidence and report uncertainty. Permanent record deletion,
branch deletion and Workspace removal are not effects of archive approval.

## 6. Execute separately approved Workspace removals

Match approval to the listed **remove Workspace checkout** action and its original
JSON. An archive-only approval or broad approval of session cleanup never includes
checkout removal. Honor partial approval; leave other candidates unchanged.

```sh
"$OVRCR_EXECUTABLE" --json workspace remove-approved --expected "$APPROVED_OBSERVATION_JSON"
```

Pass the original JSON unchanged, not a fresh preview substituted after approval.
The Server revalidates under task and lifecycle admission, rejects changed state,
and uses non-forced Git removal. Dirty files refuse even when automatic WIP saving
is on: no WIP commit, push, force, acknowledgement or session archive is authorized.
No fallback to `workspace remove`, task cleanup or direct Git deletion is allowed.

After each result, read Workspace and archived inventories and inspect the original
path, Git worktree list and branch ref. Done means the approved id is absent, its
checkout is gone, and branches and archived original context remain. Report
refused and uncertain actions separately. A Git refusal leaves files and records;
PartialFailure can mean Git removed files but registry commit failed. Preserve
records and recovery evidence, report actual state, and reconcile the original
id/path/generation without replay or a replacement approval. A lost response does
not prove rollback. Missing or replacement checkouts require reconciliation, not
blind retry. Project unregistration and permanent branch/record deletion remain
separate explicit requests.
