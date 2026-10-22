# Codex child delegation

This #329 source candidate integrates the full seven-commit #329 feature boundary
onto accepted #334 `420a462a`, which includes the #322/#327 prerequisites. Its
common Pi runtime retains the accepted invocation, source and action fences.
The eight affected Node suites passed 103 tests during integration; the final
closed replay schema is checked by the 17-test delegation suite. Focused Rust
Codex/publisher fixtures, shared-input regressions, owning unit checks and both
owning Clippy configurations have passed. Current-revision capacity, paired
build, hosted CI and installed-provider task acceptance remain open.

Earlier local verification at source checkpoint `292f59fe` passed 57 tests with
29 fixture lifecycles and 15 production-extension Node tests. Those historical
results cover fixtures and local code. Personal sign-in preparation does not
establish native confinement, model execution, publication or recovery.

## Discovery and exact selection

The installed Codex 0.160.1 supports `debug models --bundled`, `--profile`,
`--no-daemon` and `--strict-config`. Discovery uses a fresh empty profile,
cleared environment, null stdin, a 20-second deadline, a 1 MiB output bound and
an owned subprocess group. It never refreshes, authenticates or starts a turn.
The inspected bundled catalog was 658937 bytes with 11 records. Only exact
listed OpenAI model slugs are admitted. Unsupported controls, malformed or
missing catalogs, custom backends and unavailable selections refuse explicitly;
no model or provider substitution occurs. A bundled listing proves neither
account entitlement nor successful inference.

## Reserve an assignment before personal sign-in

Call the registered `ovrcr_start` tool from its managed Pi parent with the
actual registered project name/workspace ID and an exact advertised model. Set
`prepare_only` to the boolean `true` to save the normal authenticated contract
and call Prepare without Launch or SendInitial. For example:

```json
{
  "project": "PROJECT_NAME",
  "workspace": "WORKSPACE_ID",
  "provider": "codex",
  "model_provider": "openai",
  "model": "gpt-6.1-sol",
  "scope": "Read README and summarize the setup requirements.",
  "permitted_actions": ["read"],
  "acceptance_evidence": ["A versioned result answering the setup question."],
  "mandatory_capabilities": ["read", "result_file"],
  "prepare_only": true
}
```

Replace the placeholders with the registered project name/workspace ID and verify that the selected model
is advertised. Prepare-only reserves the assignment; it does not pause a child
that was already launched. Before using the reservation for login, require an
Applied Prepare receipt, `dispatch: "Prepared"`, null Session/run identity and
no Launch or InitialInput receipt. The callback must have returned, with no
child allocated. An Unknown or Refused reply is not that proof.

Keep the returned handle. If Prepare's reply is lost or uncertain, the tool
returns `recovery_arguments`. Repeat those exact arguments, including
`prepare_only: true`:

```json
{"handle": "ORIGINAL_HANDLE", "prepare_only": true}
```

This reloads the original private intent and repeats authenticated Prepare.
Do not create a replacement request or omit the flag to recover reservation.
A non-boolean `prepare_only` value refuses before control or intent effects.

For personal Sign in with ChatGPT, derive the owning canonical INSTANCE from
its assignment artifact path `INSTANCE/workflows/HANDLE`. The future child's
exact native home is `INSTANCE/codex-children/HANDLE`. Prepare does not create
that home. Create missing `codex-children` and HANDLE directories privately with
mode 0700, checking canonical parents, ownership and absence of symlinks;
reject incompatible existing entries. Leave managed config and auth creation
to their owners. In a human-owned shell, use that exact home and the same
installed Codex executable selected for the child:

```sh
umask 077; CODEX_HOME='/canonical/INSTANCE/codex-children/HANDLE' /absolute/path/to/codex -c 'cli_auth_credentials_store="file"' login
```

The person starts and completes this login-only command; it queues no native
task prompt. Keep codes, URLs and credentials out of captures and evidence.
The login process must exit before later child launch. Preserve the same
instance, handle and home so the future child reuses native auth in place.
Keep credential contents private; setup must not read, copy, export or symlink
the credential files. The constructor creates or verifies immutable managed
config without replacing auth. A
conflicting config file causes refusal rather than overwrite.

When the assignment is ready to run, an explicit `ovrcr_start` call with only
`{"handle":"ORIGINAL_HANDLE"}` resumes normal dispatch. Omitting
`prepare_only`, or setting it to `false`, can call Launch and then SendInitial
for known-unsent input. **Handle-only is an advancing action.** Retain the
original parent authority, or recover/transfer it through the existing workflow
mechanism, before advancing. Personal sign-in alone does not run or qualify
the saved assignment.

## Private launch policy

The existing Server creates and binds the child Session. The existing managed
`ovrcr agent run codex` wrapper composes an immutable private named profile and
Reporter hooks, then starts native Codex with the exact requested model,
`--profile ovrcr-research --ask-for-approval never --no-daemon --strict-config`.
No additional service, process owner or scheduler is introduced. The no-daemon
route skips retirement of an existing native daemon. Ordinary managed launches
retain their current behavior.

Initial task input waits while the native Reporter has not supplied model
evidence. If the connected original child reports a different model, dispatch
returns Refused with a clear reason and a rejected InitialInput receipt before
writing task bytes. It retains that child's identity and the spent launch;
inspect the original child rather than assuming launch was rolled back.
Replaying the same handle or initial request preserves the refusal without
sending input or creating a replacement. A changed run or conversation remains
ineligible for replay.

The candidate profile denies filesystem access at the root, allows minimal
platform reads and the actual workspace, denies the OVRCR instance, then allows
only the same child's artifact directory for immutable briefs and future answer
versions. Native configuration/history live in a separate private
`codex-children/HANDLE` directory and remain explicitly denied to model commands.
The parent workflow registry and other children receive no read grant. No model
filesystem write grant is present. Command network is disabled, login shells
are disabled, and command environment starts from `inherit = "none"` with only
`PATH=/usr/bin:/bin` and the workspace as HOME. Apps, plugins, browser, computer,
search and unrelated ambient integrations are disabled. The only configured MCP
server is `ovrcr_child`, exposing the two publisher operations below.

Native read commands may spawn processes as their execution mechanism. This
policy makes no CPU, process-count, signal or universal IPC isolation claim.
The trusted native host may load its own private profile and Reporter hooks
outside the command sandbox. No user configuration or credentials are copied.
Native login/trust and private-profile recovery remain explicit acceptance gaps;
recovery refuses a mismatched CODEX_HOME instead of selecting another account or
conversation. Safe no-daemon/strict flags survive supported recovery parsing.

## Required installed acceptance

Before advertising usable capability, prove direct Node/code-mode filesystem,
network and environment confinement, patch denial, helper inheritance, exact
MCP namespace discovery and environment forwarding on the installed native
path. Failure or unsupported controls must remain an explicit capability gap;
never widen permissions to publish. Verify Reporter model identity and initial
input fences, result publication, same-conversation followup, questions,
approval/login interruption, missing output, recovery and cleanup. No fixture,
public documentation or prior ordinary reporting acceptance closes these gates.

Historical local verification at `292f59fe` passed 16 Codex integration tests,
8 publisher integration tests and 33 unit tests after fresh compiled lists.
All 29 fixture lifecycles passed cleanup. The 15 registered-extension Node
tests include prepare-only reservation, lost-request/reply recovery and invalid
flag refusal. These results preserve accepted #335 protocol 57, registry schema
18 and canonical publication behavior; #329 adds no serialized contract or
storage shape. Broader delivery, coding, fallback, dependency/review,
wire/storage and workspace gates on the final candidate remain separate.
Earlier #331/#333-base runs are historical evidence. Their original failures
and corrected reruns remain retained with those checkpoints.

Codex currently supports read-only research. Coding, independent review
assignments and unsupported mandatory permissions receive an explicit refusal before assignment, Git
workspace or child creation. Codex capabilities advertise only `read` and
`result_file`; accepted Pi coding retains its existing isolated workspace,
immutable candidate and named-check flow. The native MCP publisher remains
research-only and never treats a server-augmented candidate digest as a
client-verifiable research digest. It preserves exact research publication
bytes and refuses coding or review fields, including explicit nulls, before transport.

## Common publisher source checkpoint

The common native-child publisher is registered as
`ABSOLUTE_OVRCR_BINARY delegation-publisher`. It runs directly in the normal
CLI, without setup offers or Server startup, and emits newline-delimited
JSON-RPC on stdout. It exposes only `ovrcr_publish` and
`ovrcr_publish_question`; no generic control, shell, path, acceptance or
caller-supplied authorization is exposed. The existing connect-only typed
client forwards to the Server's publication owner and final Session fences.

Its managed environment requires `OVRCR_DELEGATION_SESSION`,
`OVRCR_DELEGATION_TOKEN`, `OVRCR_DELEGATION_SOCKET` and
`OVRCR_DELEGATION_HOME`. Native launch integration must forward them directly
from the managed child environment, without persisting their values in provider
configuration or argv. The adapter never falls back to parent or hook authority.

Input lines are bounded to 524288 bytes, output frames to 65536 bytes, and
request IDs to 128 bytes. The supported MCP version is 2025-11-25, using
initialize, notifications/initialized, ping, tools/list and tools/call. Other
requests cannot become control operations. Tool arguments use closed schemas
and typed validation. Native clients that cannot negotiate this version remain
an explicit compatibility gap.

Confirmed success requires the exact assignment, kind, version and SHA256 of
the typed publication bytes, matching the Server's serialization. A local
refusal before sending reports `sent=false`. Explicit Server rejections retain
their code and require reconciliation rather than claiming rollback.
Internal/PartialFailure, a lost response or a mismatched publication reference
report `unknown`. Replays must retain the identical identity, version and payload.
Publication never implies parent acceptance or native settlement.

The historical eight publisher unit tests and eight integration tests passed at
`292f59fe` as part of the 57-test local run above. They exercise the real
adapter/client/Server path: wrong authority, bounded schemas, uncertain replies,
corrupt bytes and safe identical republication of missing bytes. They remain
separate from installed Codex acceptance.

Successful result Publish verifies committed bytes through the existing Server
publication verifier while the retained/Session fence is held. It attaches only
the verified immutable reference to the existing snapshot. Generic reconciliation
still omits submissions. No response includes private submission contents or
adds acceptance, settlement or parent Collect authority.
