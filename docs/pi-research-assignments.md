# Pi research assignments

A managed Pi parent can start independent read-only assignments in existing OVRCR
workspaces, keep working, and explicitly collect each child's result. Launch Pi
through `ovrcr agent run pi -- pi` or the Dashboard picker. The managed extension
registers `ovrcr_capabilities`, `ovrcr_start`, `ovrcr_retry`, `ovrcr_inspect`,
`ovrcr_collect`, `ovrcr_followup`, `ovrcr_accept`, `ovrcr_confirm_return`, `ovrcr_publish`, `ovrcr_wait`
and `ovrcr_mark_handled`.
A plain Pi process has no delegation authority.

## Start and collect

Discover the registered project and workspace through the public CLI, using the
same OVRCR instance as the managed parent:

```sh
ovrcr --json project list
ovrcr --json workspace list --project my-project
ovrcr --json workspace get --project my-project --branch feature/research
```

Use the project's `name` as `project` and the workspace row's `id` as `workspace`
in `ovrcr_start`. The workspace `name` and `branch` are display labels, not IDs;
the `id` stays stable when the checkout branch changes. When a branch is
ambiguous or detached, use `workspace get --project my-project --path /absolute/worktree/path`.

Call `ovrcr_capabilities` with `{}` to discover allowed installed selections,
defaults and available slots. If several choices remain without a configured
default, Pi selects one from the returned choices. Explicit selections never
fall back. Call `ovrcr_start` with the workspace IDs, a bounded scope, the
selection (or omit it to use the default), and required acceptance evidence:

```json
{
  "project": "my-project",
  "workspace": "existing-workspace-id",
  "scope": "Read the authentication code and explain how tokens expire. Do not modify files.",
  "provider": "pi",
  "model_provider": "anthropic",
  "model": "claude-sonnet-4-5",
  "permitted_actions": ["read"],
  "acceptance_evidence": ["Cite the expiration checks by file and line."],
  "mandatory_capabilities": ["read", "result_file"]
}
```

The model above is an example, not a promised available selection. The Server
checks Pi's available-model inventory using the same profile as the child. It
refuses an unavailable exact selection or an unsupported mandatory capability.
Only `read` and `result_file` are supported in this first flow.

The response includes a stable `handle`, the Server-created session and run IDs,
the immutable brief location, and a dispatch status. The child appears in the
existing Dashboard as an ordinary managed Pi session. Starting waits only for
launch and initial delivery, never for task completion. If startup has not yet
produced a connected Pi binding with the requested model, the status stays
`Launched`. Call `ovrcr_start` with **only** the same `handle` to finish that
known-undelivered initial input. Repeating an applied or uncertain operation
returns its receipt without writing another prompt.

Call `ovrcr_inspect` or `ovrcr_collect` with `{"handle":"..."}`. Collection
returns the complete validated submission when available. These are separate
facts:

- `dispatch` describes launch and initial input delivery.
- `activity` is the provider's current observation. Ready is not task success.
- `submission` is the child's versioned result, or null when none is published.
- `acceptance` stays `pending` until the parent explicitly calls `ovrcr_accept`
  for the exact current result. Collection never accepts the child's claim.

`Refused` means this operation is known not to have taken effect. `LaunchUnknown`,
`InputUnknown`, or a tool response with `Unknown` requires inspection of the
original handle. Do not start a replacement or resend an uncertain prompt.
Cancelling a tool wait does not cancel the child.

### Recover a lost control reply

Call `ovrcr_start` with only the original handle. The extension reads the private
original intent and reconciles that request with the Server. If Prepare never
arrived, it submits that exact contract again. If an immutable contract publication
was interrupted, only bytes matching the authenticated saved intent can complete
it. Recovery advances launch or initial input only when Server metadata proves
that operation has not crossed its effect boundary. A partial or unknown effect
is returned for inspection and never replayed. A missing or unsafe local intent
file requires manual recovery; creating another assignment is not recovery.

Snapshots include three initial-dispatch `receipts`, keyed by original request
and operation (`Prepare`, `Launch`, `InitialInput`). Their outcomes mean:

| Outcome | Meaning |
| --- | --- |
| `Accepted` | Intent is durable; the effect has not started. There is no dispatch queue. |
| `Applied` | This operation completed, not the provider task. |
| `Rejected` | This operation is known to have produced no effect. |
| `Partial` | A known input prefix reached the PTY; replay is unsafe. |
| `Unknown` | The effect boundary was crossed and completion is unconfirmed. |

Input receipts contain byte counts and whether submit was written, never input
contents. A transport error has no fabricated Server receipt. Repeating an
operation returns its known outcome; changing its original contract or identity
is refused. These receipts do not promise exactly-once execution across crashes.

The connect-only `Reconcile` request returns receipts and `resources` even when
submission bytes are damaged. Original allocated session/run IDs remain in
`identity`; resources identify the registered project/workspace, whether that
workspace is still registered, and the currently present session/run/phase.
Compare the current run with the allocated run before treating it as the original
child. Collection still strictly validates result bytes and current attribution.

Pre-receipt records are interpreted conservatively using their saved status and
validated contract. Missing receipts alone never mean an operation was unsent.
Unclassifiable records remain unknown. Storage is bounded to 4096 assignments, three initial-dispatch receipt slots and
64 follow-up slots per assignment. This permits the initial result plus 64 new
result versions, with at most one immutable acceptance per result version. These
are storage ceilings, not a scheduling or repair policy. New admission fails
before changing current references or causing effects when full; existing and
unresolved records remain recoverable. No receipt is silently evicted to make room. This technical bound
does not change live-session capacity or introduce a scheduling policy.

## Scoped follow-ups and changed requirements

Keep the task-owned child and workspace through review and repair. Call
`ovrcr_followup` with the stable `handle`, the latest `expected_identity`,
`expected_result_version`, `expected_control_epoch` from `control.epoch`, and bounded `text` describing the scoped clarification
or repair. Use the identities and version from a fresh snapshot. A valid follow-up
uses the same child session, run and conversation; it creates a distinct request
receipt and expects a new result version. It never creates a replacement attempt.
A changed run, conversation or reporting generation suspends automatic input;
returning to an old conversation name does not restore its earlier authority.

For changed requirements, additionally supply `revised_contract`: the complete
original contract with the revised scope and acceptance evidence. The same-child
operation preserves the task, workflow, provider, model, workspace and allowed
actions. It publishes a new immutable contract and explicitly sends the revised
brief. Editing a local file does not dispatch it. Both a clarification and a
revision clear only this assignment's current result and acceptance references;
previous contracts, results, receipts and acceptance references remain available.
The `contract` identity changes only for an explicit revision. A new task or
execution choice needs its own assignment; follow-ups do not reuse a specialist
for unrelated work.

The response includes the follow-up `request`. If a reply is lost, call
`ovrcr_followup` with **only** the original `handle` and `request`. The extension
reads its private saved intent and submits the same operation. A repeated request
returns its existing receipt; changing the payload or expected version is refused.
An Accepted receipt can finish interrupted immutable publication before input.
Partial or Unknown input blocks further sends, including a fresh request ID.
It does not authorize retrying the bytes or starting a replacement assignment.

`ovrcr_collect` returns only the current validated result. To inspect previous
evidence, supply `version` with the handle: the result appears separately as
`historical_submission` with `selected_version`. It cannot populate the current
submission or satisfy current acceptance. Snapshot `history` records exact
identities, versions and immutable paths. Stale child publications are refused;
previously published evidence is preserved.

After judging the deliverable, the parent may explicitly call `ovrcr_accept` with
`handle`, exact current `identity`, `result_version`, `expected_control_epoch`, and `result_digest` from
the current publication reference. The operation records the parent's judgment
in an immutable private acceptance file. Only the current parent binding can
record it, and the child binding must still match. Repeating the same acceptance
is idempotent. A child report, Ready signal, delivery receipt, local edit or
collection never accepts work. No dependency or review engine is added here.

## Wake, wait and handling

The managed parent automatically observes assignments it starts or inspects.
A result file can wake idle Pi or queue a native follow-up while Pi is busy,
even when the child emits no Ready. Wake messages contain handles and status
only. Pi retrieves submission contents separately with `ovrcr_collect`.
These observations use a control connection and leave the existing Dashboard
and its Unread acknowledgements alone.

Call `ovrcr_wait` with `{"handles":["..."],"wait_ms":1000}` to reconcile a batch.
The response contains a `cursor`, content-free observations, publication
identities and handling state. Pass the returned cursor as `after` on the next
call to wait for changes. A batch holds at most 64 distinct handles and waits
at most 20 seconds. These are observation bounds, not a child-concurrency cap.
The 256-entry hint ring returns `ReconcileRequired` after overflow or a foreign
Server cursor; files and durable identities remain authoritative. Disconnects
are explicit and the observer reconnects through the same public control path.
`/ovrcr-observe-reconnect` restarts observation without changing child work.

After processing a collected result, call `ovrcr_mark_handled` with `handle`
and the exact `publication.identity` returned by observation. Its assignment,
kind, version and digest must match. Repeating that acknowledgement is safe;
a different digest cannot inherit handling. Collection, wake delivery and
handling do not accept the child's task-success claim. Explicit acceptance stays
unchanged.
Unacknowledged results remain discoverable across observer reconnects. Prior
handling and acceptance remain in version history. A follow-up expects a new
publication identity; handling an old result cannot acknowledge its successor.

Set the observation progress deadline through the normal settings owner:

```toml
[delegation]
progress_deadline_ms = 300000
```

The default is five minutes; valid values are 1 through 86400000 milliseconds.
A silent child becomes stalled/incomplete without being interrupted. Private
parent observation checkpoints preserve the deadline across reconnects.
Repeated polls, title/metrics updates, an epoch change or unproven gapped samples
do not manufacture progress. Future persisted times are unresolved/expired.
Cancelling `ovrcr_wait` aborts only that observation helper. The same child run
continues and its later result remains collectable.

Before publication becomes visible, the Server records an authenticated child
intent containing the exact assignment, original binding, version and digest.
File-only reconciliation can recover matching complete bytes after publication
failure or a lost wake. A parseable file without that intent cannot acquire
child authorship. Partial, changed, foreign and escaping files stay invalid;
unresolved intent and identifiable late evidence are retained. A stale run or
conversation cannot satisfy current collection or handling.
A capacity refusal has `retryable_capacity_refusal: true`. Keep that assignment
pending, inspect it, then call `ovrcr_retry` with its handle after capacity returns.
The retry creates a new request and monotonically increasing attempt identity,
preserving the logical task, scope and exact effective choices/limits. It persists
that successor before sending. Repeating retry with the predecessor handle
reconciles the same successor; it cannot launch another child. If the successor
is itself capacity-refused, retry its returned handle on the next reconciliation.
The predecessor stays refused and reports `successor_request`. Unknown or partial
effects never qualify, regardless of current free capacity.

`execution_attempts` is the Server's consumed execution count; `null` preserves
legacy or unknown accounting. Only proven zero execution is reported as zero.
A stopped or missing child does not refund a possibly consumed attempt.

## Child result and control

The child reads the assigned brief and calls `ovrcr_publish` with its `handle`
and a `submission` containing `schema: 1`, the exact `identity` object from the
brief, `version` equal to that brief's `result_version`, `outcome` (`success`, `failed`, or `blocked`), `summary`,
`evidence`, and `unresolved` arrays. Success requires evidence. The Server checks
the child's current session, run and provider conversation before publishing a
complete, immutable `submission-v<version>.json`. A parent or sibling cannot publish for
it. A modified, partial, foreign, stale or symlinked publication cannot satisfy
collection.

The child's built-in tools are restricted to read, grep, find and ls; the managed
result tool supplies the one deliberate write. Discovered extensions and skills
are disabled for the child, and automatic approval is not enabled. This restricts
the available tool surface; it is not a separate OS-user sandbox. All local
workflow files follow the same trusted-user boundary as the OVRCR instance.

Viewing, selecting, focusing, resizing or hiding a child's pane does not grant
control or interrupt it. The Dashboard shows its authoritative owner. Choose
**Take control** (`H` in Browse) to enable that focused child's dedicated human
input. Choose **Return to Pi** (`A`) to revoke human input and install a fresh
reconciliation barrier. Generic terminal send, paste and named keystrokes stay
refused for assigned children even during Human control. Ordinary sessions keep
their normal input behavior.

The parent reads the current `control` and `control_evidence`, then explicitly
calls `ovrcr_confirm_return` with the original return request, a distinct
confirmation `request`, the captured `expected_control_epoch`, and exact fresh
`evidence`. Inspect/Collect/Reconcile never transfer authority or clear barriers.
Both transitions advance a per-child epoch; old initial/followup/acceptance
requests remain fenced after Pi-to-human-to-Pi control changes. Sibling assignments
and the workflow controller epoch are unchanged.

Human changes suspend current result attribution. Confirmation clears only its
own return barrier; a scoped follow-up must establish a fresh result version
before further automated effect or acceptance. Unknown/Partial dispatch receipts
remain unresolved. Previously reserved versions from a rejected stale intent are
never reused; a later follow-up may skip their version numbers while keeping the
immutable files and receipts. Unexpected reporting binding changes make control
Uncertain and require supported recovery rather than ordinary return confirmation.
See [Human control](pi-human-control.md) for Dashboard steps and lost-reply rules.

## Delegation policy

Delegation policy uses the Server's existing `dashboard.toml` settings document.
The Settings view and `ovrcr settings set` use the same typed catalog, writer and
published snapshot. For example:

```toml
[delegation]
enabled = true
allowed_harnesses = ["pi"]
allowed_models = ["anthropic/claude-sonnet-4-5"]
default_harness = "pi"
default_model = "anthropic/claude-sonnet-4-5"
max_live_children = 50
# Optional per logical task; unset by default:
# max_execution_attempts = 2
```

The model is illustrative and still requires an available exact inventory entry.
`allowed_models = []` adds no model restriction. A configured harness does not
install or enable an adapter. Invalid policy produces ordinary settings findings;
dispatch refuses malformed policy rather than using a permissive fallback.
Explicit assignment choices override defaults, subject to the allowlists and
implemented capabilities. Unsupported mandatory token, cost or native-helper
limits are refused before dispatch.

`max_live_children` accepts 1 through 50 and adds a delegation limit. Ordinary
sessions and delegated children share the existing fifty Running/Paused slots;
stopped and archived sessions consume none. There is no four-worker limit.
An assignment may require `min_free_slots_after_launch` (0 through 49) in
`required_limits`. Admission checks that threshold atomically with the existing
lifecycle lock; it does not reserve those slots against later ordinary creators.
Requested limits cannot loosen Server policy. Native descendants are not separate
inventoried OVRCR sessions. This Pi research flow advertises no native-helper capability because its managed
launch restricts the built-in tools and disables discovered extensions and skills.
This is an adapter capability gap, not a global policy ban. Later provider
adapters can permit verified helpers within their child contracts while applying
the same settings policy. Native helper use and descendant accounting require
separate provider acceptance.

`max_execution_attempts` is an optional positive integer for one logical task.
Known zero-effect refusals, including a full instance, consume no execution
budget. A launch that started or may have started consumes one execution attempt.
The retry identity still advances after a capacity refusal. Uncertain effects do
not become retryable when capacity returns.

## Storage and diagnostics

The existing `SessionStore` and `registry.sqlite3` own non-payload control
identities and receipts. Immutable contracts, briefs and results live below
`<OVRCR instance>/workflows/<handle>/`, outside the workspace. Parent intent files
live below `parent-intents/`. Directories are private (0700), files are private
(0600), and publication uses complete files with no replacement. File access is
relative to held directory descriptors and rejects symlink traversal. Editing a
file never changes the Server's control ownership or accepts work.

The extension uses the connect-only `ovrcr --json delegation` command, which
reads one typed `DelegationRequest` JSON document on stdin. It sends requests via
the existing OVRCR control client. The managed wrapper gives native Pi a scoped
delegation capability; raw reporting hook credentials remain unavailable to the
native process. Task content stays out of reporting Events, notifications and
retained session metadata. The tool's returned data and private workflow files
are intentional task artifacts.

Deterministic coverage is in `tests/pi_delegation.rs` (real managed PTYs, binary
Server, public tools and files) and `tests/pi_delegation_extension.mjs` (public
tool schema and missing-input behavior). Observation integration lives in
`tests/pi_delegation/observation.rs` within the same binary;
`tests/pi_observation_extension.mjs` covers continuation coalescing and cancellation. `tests/fixtures/pi/research_pi.sh`
replaces model work for acceptance; it is not installed-provider evidence.
Native and paid-provider acceptance must be recorded separately on the reviewed
revision.
