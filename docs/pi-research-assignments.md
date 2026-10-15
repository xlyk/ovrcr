# Pi research assignments

A managed Pi parent can start one read-only assignment in an existing OVRCR
workspace, keep working, and explicitly collect the child's result. Launch Pi
through `ovrcr agent run pi -- pi` or the Dashboard picker. The managed extension
registers `ovrcr_start`, `ovrcr_inspect`, `ovrcr_collect` and `ovrcr_publish`.
A plain Pi process has no delegation authority.

## Start and collect

Call `ovrcr_start` with the registered project and workspace IDs, a bounded scope,
an exact model provider and model ID, and the evidence required for acceptance:

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
