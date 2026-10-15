# Pi research assignments

A managed Pi parent can start one read-only assignment in an existing OVRCR
workspace, keep working, and explicitly collect the child's result. Launch Pi
through `ovrcr agent run pi -- pi` or the Dashboard picker. The managed extension
registers `ovrcr_start`, `ovrcr_inspect`, `ovrcr_collect` and `ovrcr_publish`.
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

Call `ovrcr_start` with those IDs, a bounded scope, an exact model provider and
model ID, and the evidence required for acceptance:

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
- `acceptance` remains `pending`; collection does not accept the child's claim.

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
Unclassifiable records remain unknown. Storage is bounded to 4096 assignments
and three initial-dispatch receipt slots each (at most 12,288 receipts). New
admission fails before effects when full; existing and unresolved records remain
recoverable. No receipt is silently evicted to make room. This technical bound
does not change live-session capacity or introduce a scheduling policy.

## Child result and control

The child reads the assigned brief and calls `ovrcr_publish` with its `handle`
and a `submission` containing `schema: 1`, the exact `identity` object from the
brief, `version: 1`, `outcome` (`success`, `failed`, or `blocked`), `summary`,
`evidence`, and `unresolved` arrays. Success requires evidence. The Server checks
the child's current session, run and provider conversation before publishing a
complete, immutable `submission-v1.json`. A parent or sibling cannot publish for
it. A modified, partial, foreign, stale or symlinked publication cannot satisfy
collection.

The child's built-in tools are restricted to read, grep, find and ls; the managed
result tool supplies the one deliberate write. Discovered extensions and skills
are disabled for the child, and automatic approval is not enabled. This restricts
the available tool surface; it is not a separate OS-user sandbox. All local
workflow files follow the same trusted-user boundary as the OVRCR instance.

Viewing or focusing a child does not grant control. While assigned, generic
terminal send, named keystrokes and Dashboard terminal input are refused. The
parent can still receive ordinary input. This slice has no takeover/return UI;
those controls, follow-ups, automatic wake, rich restart recovery, other child
providers and coding workspace creation are later work.

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
tool schema and missing-input behavior). `tests/fixtures/pi/research_pi.sh`
replaces model work for acceptance; it is not installed-provider evidence.
Native and paid-provider acceptance must be recorded separately on the reviewed
revision.
