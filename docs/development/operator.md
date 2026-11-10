# Operator runtime and authentication reference

User setup and controls are in [Use the built-in operator](../operator.md).

The Operator is one durable conversation per OVRCR instance, driven by the
installed [pi](https://github.com/earendil-works/pi) coding agent 0.87.1 through
its RPC protocol. There is no embedded model client, no Rig agent and no custom
OAuth: pi owns authentication, model catalog, the agent loop, tool definitions,
inference and context management. The Server stays synchronous; a managed
same-executable worker owns each turn's pi process.

## Package layout

- `src/operator/pi.rs` — executable resolution, the dedicated profile, launch
  command construction, the bundled CLI skill, extension materialization and
  bounded model listing.
- `src/operator/rpc.rs` — the blocking, bounded JSONL child driver shared by the
  worker and `ovrcr operator models`.
- `src/operator/worker.rs` — one turn: authenticated Start, attempt directory,
  fresh native session, checkpoint restore, exact model resolution, one prompt,
  ordered journal batches, session promotion, Completed and authenticated Retire.
- `src/pi-operator-extension.mjs` — the only custom code loaded into pi. It
  publishes the readiness contract, overrides `bash` with an attached execution
  adapter, and turns pi's tool callbacks into the durable admission handshake.

`OVRCR_PI_EXECUTABLE`, when set and nonempty, names the pi executable; otherwise
`pi` is resolved on `PATH`. A missing executable is reported as
`Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.` and no turn
proceeds.

## Dedicated profile

All Operator state lives inside the instance directory (`RegistryPath::resolve`):

| Path | Mode | Purpose |
| --- | --- | --- |
| `<instance>/operator-pi` | 0700 | Pi's `PI_CODING_AGENT_DIR`: native auth, models and settings |
| `<instance>/operator-pi/turns` | 0700 | One attempt directory per turn id |
| `<instance>/operator-workdir` | 0700 | Fixed pi process cwd for the whole conversation |

Existing paths that are symlinks, non-directories, owned by another user, or
group/world-writable are refused rather than replaced. The personal pi profile
and OVRCR credentials are never copied. Every child runs with umask `077`.

Each attempt is `operator-pi/turns/<turn-id>`, created exclusively with 0700; an
existing directory is an uncertain prior attempt and refuses the turn instead of
replaying it. The deterministic native session id is
`operator-<conversation-id>`, so every attempt resumes the same pi conversation.

Environment for every child: `PI_CODING_AGENT_DIR=<instance>/operator-pi`,
`PI_TELEMETRY=0`, `PI_SKIP_VERSION_CHECK=1`, `OVRCR_EXECUTABLE` pointing at the
matching OVRCR binary, cwd `<instance>/operator-workdir`, and the ordinary tool
environment. Inherited OVRCR reporting, hook and delegation authority,
`OVRCR_PI_ENTRY`, `OVRCR_CLAUDE_ARTIFACTS`, `BASH_ENV`, `ENV`, `PI_SESSION_*`,
`PI_CODING_AGENT_SESSION*` and provider credential variables from pi's
environment-variable table are removed.

## Launch commands

Managed turn (`pi.rs::rpc_command`):

```text
pi --mode rpc --no-extensions --no-skills --no-prompt-templates
   --no-themes --no-context-files --no-approve --offline
   --tools read,bash,edit,write
   --session-dir <attempt> --session-id operator-<conversation-id>
   --append-system-prompt <policy + bundled CLI skill>
   --extension <operator-pi>/pi-operator-extension.mjs
```

One `--append-system-prompt` argument carries the fixed Operator policy followed
by `.agents/skills/ovrcr/SKILL.md`, compiled into the binary with `include_str!`.
The skill is available before the first model request in full chat and Quick
message without a repository checkout. `--no-skills` remains, so neither that
source tree nor a personal profile contributes a second copy; no runtime skill
file is read and the skill is never injected into a conversation message or tool
result. Its interactive-shell guidance resolves `ovrcr` once, while Operator
keeps the inherited `OVRCR_EXECUTABLE`, `OVRCR_HOME`, `OVRCR_CONFIG`,
`OVRCR_SOCKET` and exactly the captured project/workspace/session run identities.

pi stays in the worker's process group (no `process_group(0)`), so the Server's
lifetime Session owns pi and its tool children. `ovrcr operator models` runs in
its own task-owned group with `--mode rpc --no-session --no-tools` and the same
isolation flags; it always terminates and reaps that group.

Interactive setup (`ovrcr operator setup`) execs native pi with `--no-session
--no-tools --no-extensions --no-skills --no-prompt-templates --no-themes
--no-context-files --no-approve`, without `--offline` so `/login`, `/model` and
`/logout` can use the network. It requires a TTY, refuses `--json`, and starts no
Server.

## One turn

1. Authenticated `Reporter::operator_start` from the Server. A refused Start
   creates no pi resources. The returned `OperatorWorkerStart` carries the identity, the
   turn's captured `model` override, text, context and `checkpoint_turn`.
2. Create the exclusive attempt directory, materialize the extension, and launch
   pi. `get_state` resolves the fresh profile's model and thinking level; the
   session id must equal `operator-<conversation-id>`; the trusted extension must
   bind before anything else happens.
3. If `checkpoint_turn` is set, copy
   `<instance>/operator-pi/turns/<checkpoint-turn>/session.jsonl` into the
   attempt as a private 0600 file (regular, non-symlink, owned), validate its
   session header (type `session`, id, version 1–3, absolute cwd equal to the
   agent workdir), then `switch_session` pointing at the copy. `switch_session`
   must report success with `cancelled:false`, bind the extension again, and the
   following `get_state` must report exactly that file.
4. Resolve the model: split the captured override at the first `/`, otherwise use
   the fresh profile default (never the resumed session's old model). The exact
   provider/id must appear in `get_available_models`; `set_model` must return the
   same identity; the profile's thinking level is restored with
   `set_thinking_level`. Then `set_auto_retry` off and `set_auto_compaction` on.
   A missing or unavailable model fails the turn; there is no fallback.
5. Commit `ModelSelected`, then send exactly one `prompt` whose message is the
   captured context JSON followed by the user text, so input such as `/new` is
   never a native command.
6. Stream until `agent_settled`. `prompt.success` is acceptance only. Success
   also requires the final assistant message's `stopReason == "stop"`, no
   unresolved admitted tool, and no transport or extension error. Auto-compaction
   runs inside the one conversation. Every ordinary model request, including the
   one after compaction, carries the policy and bundled CLI skill only in its
   system prompt; the compaction conversation payload and the durable
   conversation journal never contain that guidance.
7. Final `get_state`, bounded `clear_queue` and `abort`, then close stdin and
   drain/reap pi within two seconds. The native session at the reported path,
   confined to the attempt directory, is renamed to `session.jsonl` when needed
   and fsynced with its directory.
8. Commit unit `Completed`. That transaction atomically promotes
   `pi_checkpoint_turn` to this turn. Only after the acknowledgement is the
   superseded checkpoint's `session.jsonl` and its now-empty attempt directory
   removed.
9. Authenticated `Reporter::operator_retire` returns ownership to the Server.

A failed, interrupted or uncertain turn keeps its partial journal and its attempt
files as evidence and never becomes the checkpoint. The next explicit turn
resumes from the last completed checkpoint; the rollback is model context only.
No prompt is ever replayed automatically. With no prior completed turn the next
attempt starts empty with the same deterministic session id.

`OperatorWorkerStart.model` is the `operator.model` override captured when Submit
was durably accepted, not the setting's value when the worker starts; a settings
edit cannot change an accepted turn.

## Standard pi tools and durable admission

pi supplies `read`, `bash`, `edit` and `write` with their native schemas,
validation, truncation and rendering. The extension replaces only `bash`, using
`createBashToolDefinition(process.cwd(), { operations, exposeSessionEnvironment:
false })`. Its `operations.exec` runs `/bin/bash --noprofile --norc -c` with
`detached:false`, piped stdout/stderr, no new process group, pi's native command
and optional timeout schema, and a 64 KiB journal cap. On timeout the call is
recorded as timed-out and the whole turn stops; control never returns to another
model request. Trailing output from a completed background child is drained
within a short idle window so a held pipe cannot hang the tool.

The extension reports a version-1 contract through `ctx.ui.setStatus` with the
session id and `getActiveTools()`. The worker requires exactly the four tools and
the expected session id before prompting; a missing or wrong contract refuses the
turn with `Pi Operator integration is unavailable; no prompt was sent.`

Tool calls use an awaited `confirm` dialog titled
`OVRCR Operator tool admission` carrying `{toolCallId, toolName, input}`. The
worker validates the four-tool allowlist, the id and argument bounds, requires
the matching completed assistant message with `stopReason` `toolUse` or `stop`
and the same call id/name/arguments, flushes preceding assistant text, commits
`ToolCall`, and only then answers `confirmed:true`. Admissions are serialized.

Results use an awaited dialog titled `OVRCR Operator tool result` carrying
`{toolCallId, output, truncated, status}`. The worker maps `succeeded`/`failed`/
`timedout`, replaces NUL with U+FFFD, bounds output to 64 KiB, combines that with
pi's native truncation, commits `ToolOutput`, and only then acknowledges. A
refused acknowledgement aborts and holds the turn as uncertain.

No other dialog is answered: the worker cancels it and fails with
`Unsupported Pi interaction; the turn was stopped.` Display-only notifications,
partial tool updates and duplicate execution notifications are ignored.
Displayable thinking is journaled as its own `ThinkingDelta` stream in content
order; provider thinking signatures are never carried. Adjacent same-turn deltas
form one disclosure keyed by their first ordinal. Its peach, italic body wraps
before counting: untouched Running blocks show all lines up to six, then the
latest six; untouched ended or older-turn blocks collapse. An explicit expansion
or collapse overrides that default across streaming, completion and reopening.
Empty or newline-only thinking adds no block. The muted, upright header reports
the wrapped count and, for a capped preview, `showing last 6 of N lines`.
Disclosure identity stays separate from each body's durable source anchor;
keyboard and mouse toggles share the single transcript viewport.

A compaction summary's stream is never journaled as assistant text:
`compaction_start`/`compaction_end` are durable control events, and the activity
row reports `Compacting context…` until the end event.

## RPC driver

The driver splits records only on LF, tolerates a trailing CR, and caps every
complete or incomplete record at 16 MiB (`rpc::FRAME_LIMIT`). Reads are
nonblocking in 16 × 16 KiB batches with a 10 ms poll, so deadlines and
cancellation are never starved. Responses are matched by request id while
unsolicited events are processed in order. Startup and control commands use a
20-second deadline; prompt execution has no added wall-clock deadline. stderr is
drained by a helper thread into a bounded ring (64 KiB retained, 4 KiB excerpt)
and never persisted. Malformed JSON, failed required commands, premature EOF and
required-extension errors fail the turn.

Fixed user-facing failures, never with native stderr or provider bodies:

- `Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.`
- `No Pi model selected; run ovrcr operator setup.`
- `Configured Operator model is unavailable in the dedicated Pi profile.`
- `Operator Pi context is unavailable; saved data was preserved.`
- `Pi Operator integration is unavailable; no prompt was sent.`
- `Pi request failed; inspect the partial journal before retrying.`
- `Unsupported Pi interaction; the turn was stopped.`

pi's placeholder model object (`provider`/`id` both `unknown`) is normalized to
"no model selected".

## Stop, failure and retirement

Hard Stop remains a Server path: durably interrupt, revoke authority, release
locks, terminate the owned group, record controlled-stop proof. The worker
installs SIGTERM/SIGHUP flags and a 2-second watchdog; while shutting down it
best-effort sends `clear_queue`/`abort` and drains, and commits nothing after
authority is revoked. pi exit alone is never stop proof: only the Server's scoped
termination establishes `worker_stopped`. Stop frees the conversation for another
turn; an OVRCR operation already accepted may finish.

A worker-detected transport, provider or bootstrap failure commits the fixed
`Failed` message and requests Retire. A tool timeout requests authenticated Stop.
A failed reporting commit blocks further admissions and requests Stop. The worker
never launches a replacement child or resends a prompt, and an uncertain tool
result holds the turn until the Server terminates it or the user stops it.

## Model selection and settings

`ovrcr operator models` prints `provider/id<TAB>name`, or JSON
`{"models":[{"provider":"...","id":"...","name":"..."}]}`, bounded to 4096
validated, deduplicated entries. An empty native list prints no invented
defaults and points at `ovrcr operator setup`.

`operator.model` is one optional `provider/model` string in the shared settings
document. Empty/unset uses the dedicated profile's current default before each
turn; an explicit value overrides it and is captured per accepted Submit. The
legacy `operator.provider`, `operator.chat_gpt_model` and `operator.grok_model`
keys are not translated; the unknown-setting findings identify them, and the
document is never rewritten automatically.

## Removed engine

The previous Rig-based engine, its custom OAuth login, embedded Bash tool,
provider-specific request transformation and SDK model-history persistence are
gone. Root dependencies on `rig`, `tokio`, `futures`, `reqwest` and
`jsonwebtoken` were removed for the Operator. Legacy Operator rows, journals,
model-history bytes and credentials stay on disk untouched and hidden from the
Dashboard; see [Use the built-in operator](../operator.md#existing-data).
