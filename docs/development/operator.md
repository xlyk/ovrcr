# Operator runtime and authentication reference

User setup and controls are in [Use the built-in operator](../operator.md).

The Operator is one durable conversation per OVRCR instance, driven by the
installed [pi](https://github.com/earendil-works/pi) coding agent 0.87.1 through
its RPC protocol. There is no embedded model client, no Rig agent and no custom
OAuth: pi owns authentication, model catalog, the agent loop, its four native
tool definitions, inference and context management. OVRCR adds `watch_sessions`.
The Server stays synchronous; a managed same-executable worker owns pi for a
cold run. When a completed turn carries a
work lease, that process stays live across warm user turns on the same run.

## Package layout

- `src/operator/pi.rs` — executable resolution, the dedicated profile, launch
  command construction, the system-only CLI guide, coordinator-skill
  materialization, extension materialization and bounded model listing.
- `src/operator/rpc.rs` — the blocking, bounded JSONL child driver shared by the
  worker and `ovrcr operator models`.
- `src/operator/worker.rs` — cold Start and run-scoped warm StartTurn, one live
  pi conversation, per-turn checkpoint copies, exact model resolution for each
  prompt, ordered journal batches, lease-aware Completed and authenticated Retire.
- `src/pi-operator-extension.mjs` — the only custom code loaded into pi. It
  publishes the readiness contract, overrides `bash` with an attached execution
  adapter, and turns pi's tool callbacks into the durable admission handshake.
  The worker consumes the bounded `ovrcr_operator_watches` status as its work
  lease count; the separate watch tool owns producing that status.

`OVRCR_PI_EXECUTABLE`, when set and nonempty, names the pi executable; otherwise
`pi` is resolved on `PATH`. A missing executable is reported as
`Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.` and no turn
proceeds.

## Dedicated profile

All Operator state lives inside the instance directory (`RegistryPath::resolve`):

| Path | Mode | Purpose |
| --- | --- | --- |
| `<instance>/operator-pi` | 0700 | Pi's `PI_CODING_AGENT_DIR`: native auth, models and settings |
| `<instance>/operator-pi/ovrcr-coordinator` | 0700 | Explicitly loaded coordinator skill |
| `<instance>/operator-pi/ovrcr-coordinator/SKILL.md` | 0600 | Bundled coordinator instructions |
| `<instance>/operator-pi/turns` | 0700 | One attempt directory per turn id |
| `<instance>/operator-pi/runs/<run-id>` | 0700 | Live Pi session files for one worker run |
| `<instance>/operator-workdir` | 0700 | Fixed pi process cwd for the whole conversation |

Existing paths that are symlinks, non-directories, owned by another user, or
group/world-writable are refused rather than replaced. The personal pi profile
and OVRCR credentials are never copied. Every child runs with umask `077`.

Each turn attempt is `operator-pi/turns/<turn-id>`, created exclusively with
0700; an existing directory is an uncertain prior attempt and refuses the turn
instead of replaying it. The worker's native Pi session lives in its
`operator-pi/runs/<run-id>` directory. The deterministic native session id is
`operator-<conversation-id>`, so cold worker runs can resume the same Pi
conversation from the last completed per-turn `session.jsonl` checkpoint.

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
   --tools read,bash,edit,write,watch_sessions
   --skill <operator-pi>/ovrcr-coordinator/SKILL.md
   --session-dir <operator-pi>/runs/<run-id>
   --session-id operator-<conversation-id>
   --append-system-prompt <policy + bundled CLI guide>
   --extension <operator-pi>/pi-operator-extension.mjs
```

One `--append-system-prompt` argument carries the fixed Operator policy followed
by `.agents/skills/ovrcr/SKILL.md`, compiled into the binary with `include_str!`.
The CLI guide is available before the first model request in full chat and Quick
message without a repository checkout. The separate coordinator skill is
atomically materialized as a private profile file and passed through `--skill`.
`--no-skills` remains, so Pi advertises that explicit file without discovering
skills from the profile, project or personal home. Pi reads its body on demand
when the task matches. The CLI guide remains only in system prompt text and never
enters the user message or journal. Its interactive-shell
guidance resolves `ovrcr` once, while Operator keeps the inherited
`OVRCR_EXECUTABLE`, `OVRCR_HOME`, `OVRCR_CONFIG`, `OVRCR_SOCKET` and exactly the
captured project/workspace/session run identities.

The coordinator skill source is `pi/skills/ovrcr-coordinator/SKILL.md` and is
embedded with `include_str!`; each cold worker launch atomically writes the
profile copy with mode 0600. Its name, description and exact private path are
advertised to Pi.

pi stays in the worker's process group (no `process_group(0)`), so the Server's
lifetime Session owns pi and its tool children. `ovrcr operator models` runs in
its own task-owned group with `--mode rpc --no-session --no-tools` and the same
isolation flags; it always terminates and reaps that group.

Interactive setup (`ovrcr operator setup`) execs native pi with `--no-session
--no-tools --no-extensions --no-skills --no-prompt-templates --no-themes
--no-context-files --no-approve`, without `--offline` so `/login`, `/model` and
`/logout` can use the network. It requires a TTY, refuses `--json`, and starts no
Server.

## Turn lifecycle

1. A cold worker starts with `OperatorRunIdentity { conversation, session, run }`.
   Authenticated `Start` resolves the current Running turn from retained state;
   no turn identity comes from argv, and a refused Start creates no pi resources.
   Warm dispatch uses the same run identity; authenticated
   `StartTurn` returns the next accepted user turn without changing that run.
   The returned `OperatorWorkerStart` carries the turn's captured `model`
   override, text, context and `checkpoint_turn`.
2. Create the exclusive turn attempt and run directory, materialize the
   extension and coordinator skill, then launch pi. `get_state` resolves the
   fresh profile's model and thinking level; the
   session id must equal `operator-<conversation-id>`; the trusted extension must
   bind before anything else happens.
3. On a cold start, if `checkpoint_turn` is set, copy its private
   `session.jsonl` checkpoint into the new run directory,
   validate the regular file and session header (type `session`, id, version
   1–3, absolute cwd equal to the agent workdir), then `switch_session` to that
   file. A warm turn keeps the already-running Pi session attached; its separate
   checkpoint copy is stored as this turn's `session.jsonl`, so normal chat does
   not reload the extension or overwrite the active run session.
4. Resolve the model: split the captured override at the first `/`, otherwise use
   the fresh profile default (never the resumed session's old model). The exact
   provider/id must appear in `get_available_models`; `set_model` must return the
   same identity. Set `set_auto_retry` off and `set_auto_compaction` on, then
   commit `ModelSelected`, clearing the previous model's reasoning readback.
   Apply the turn's captured `operator.thinking_level`. `default` restores the
   model-specific level captured when that model was first selected in the run,
   so a previous explicit override cannot leak into a later Default turn.
   Read `get_state.thinkingLevel`
   and `get_available_thinking_levels` (`{levels: [...]}`) back and commit
   `ReasoningSelected` with the effective level, supported list and captured
   request.
   A missing or unavailable model fails the turn; there is no fallback.
5. Send exactly one `prompt` whose message is the
   captured context JSON followed by the user text, so input such as `/new` is
   never a native command.
6. Stream until `agent_settled`. `prompt.success` is acceptance only. Success
   also requires the final assistant message's `stopReason == "stop"`, no
   unresolved admitted tool, and no transport or extension error. Auto-compaction
   runs inside the one conversation. Every ordinary model request, including the
   one after compaction, carries the policy and bundled CLI guide only in its
   system prompt; the compaction conversation payload and durable conversation
   journal never contain that guidance. The coordinator skill remains a separate
   explicit `--skill` file.
7. Read the bounded `ovrcr_operator_watches` status. With an active lease, copy
   the live session into this turn's private `session.jsonl`, validate the
   session header, and fsync the copy and directory. Commit
   `Completed { leased: true }`; the Server promotes `pi_checkpoint_turn` and
   the worker enters a short idle loop alternating authenticated `StartTurn`
   checks with Pi RPC output polling. A warm Submit keeps the `SessionRunId`,
   advances the `OperatorTurnId`, resolves the current model and reasoning
   settings, and prompts the same Pi process.
8. With no active lease, successfully settle Pi (`clear_queue`, `abort`, clean
   exit and reap) before copying the final session snapshot. After validating
   and fsyncing it, commit `Completed { leased: false }`. This clears Server
   warm-dispatch eligibility before authenticated `Retire` returns ownership.
   The prior checkpoint copy is removed only after the completion receipt.

A failed, interrupted or uncertain turn keeps its partial journal and its attempt
files as evidence and never becomes the checkpoint. The next explicit turn
resumes from the last completed checkpoint; the rollback is model context only.
No prompt is ever replayed automatically. With no prior completed turn the next
attempt starts empty with the same deterministic session id.

`OperatorWorkerStart.model` is the `operator.model` override captured when Submit
was durably accepted. A settings edit cannot change an accepted turn; the next
warm turn captures the new value and applies it to the still-live Pi conversation.

The Dashboard's `worker_live` means the exact Operator process is live on the
current run and has a running turn or a completed leased turn. A leased idle
conversation can accept Submit on the same run and hides Stop. `worker_stopped`
continues to mean the Server has controlled-stop proof; neither Pi exit nor an
unconfirmed teardown sets it.

Registry schema 33 allows several completed Operator turns on one run while
retaining the one-Running-turn-per-conversation index. Its migration rebuilds
the turn table with foreign keys disabled and verifies dependent journal rows
and IDs afterward. Wire protocol 85 carries `OperatorRunIdentity`,
`worker_live`, warm `StartTurn`, prepared `AbortTurn`, and leased completion.
`Open` and `Current` remain read-only and never create a worker.
The migration is schema 32 → 33 and the wire change is 84 → 85. Older binaries
refuse the newer schema; clients and Server require matching wire versions.
The history reader accepts legacy `{"Worker":"Completed"}` rows as unleased
completion without rewriting their stored payloads or batch receipts.

## Workspace coordination watches

The explicit coordinator skill and fixed policy teach Operator to use visible
Retained Workspace Agent sessions for delegated work. Coding tasks use separate
worktree Workspaces on dedicated branches, protecting Root; shared Workspaces
are limited to read-only research or an explicit same-branch request. The
`watch_sessions` tool adds, lists or removes bounded Reporter watches. Removing
a watch does not stop its session.

Before `terminal send`, Operator captures the session `run`, numeric
`agent.activity_revision` and opaque `unread.turn` from `terminal list --json`.
The tool maps `unread.turn` to `baseline.unread_turn` without parsing it. Its
deterministic poller wakes on a post-baseline Ready revision/identity change,
`waiting_input`, a run change, or an exited/stopped/interrupted session. It
batches reports received during another turn and starts one `AsyncWakeup` only
when Pi is idle; queued reports keep the worker leased until that wakeup settles.
Each non-overlapping poll runs every 2.5 seconds with a five-second CLI timeout.
Before `sendUserMessage` or model work, the extension asks the worker to reserve
`BeginAsyncTurn` on the Server. Only a confirmed reservation permits delivery;
if user Submit wins, all reports remain queued for the next idle attempt.

On wakeup, Operator reads only the watched session replies: Claude Code and
Codex use `terminal transcript --json`, falling back to the terminal tail when a
transcript entry reaches the 16 KiB cap; Pi and Oh My Pi use `terminal read` or
`terminal screen`. It inspects a reply once if the requested completion trailer
is missing, verifies `git status`, `git diff --stat` and reported check exit codes
in the matching Workspace, and leaves completed Retained sessions open. Human
decisions and `waiting_input` prompts return to the user.

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
session id and `getActiveTools()`. The worker requires exactly the five tools and
the expected session id before prompting; a missing or wrong contract refuses the
turn with `Pi Operator integration is unavailable; no prompt was sent.`

Tool calls use an awaited `confirm` dialog titled
`OVRCR Operator tool admission` carrying `{toolCallId, toolName, input}`. The
worker validates the five-tool allowlist, the id and argument bounds, requires
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

Dashboard Stop first requests an authenticated per-turn abort from the exact live
worker. With an active lease, it clears Pi's queue, aborts the foreground turn,
and saves the partial native session as `session.partial.jsonl`. If records were
appended, it copies and validates the last completed checkpoint into a fresh run
file and calls `switch_session`; without a checkpoint it creates an empty native
session. A zero-record attempt skips the switch. This rolls back model context,
preserving external effects, journal entries and attempt files.

The extension keeps watches, queued reports and poll state in process-global
state across `session_shutdown(reason="resume")`, rebinds on session start, and
cleans up on genuine shutdown. The worker acknowledges `AbortTurn` only after
rollback. Until then the Server keeps the turn Running, so a reconnect cannot
submit into a rolling-back worker. After acknowledgement a leased worker remains
ready on the same run; an unleased worker retires. If it cannot settle within
five seconds, the Server falls back to scoped process-group termination.

Hard Stop remains a Server path: durably interrupt, revoke authority, release
locks, terminate the owned group, record controlled-stop proof. The worker
installs SIGTERM/SIGHUP flags and a 2-second watchdog; while shutting down it
best-effort sends `clear_queue`/`abort` and drains, and commits nothing after
authority is revoked. pi exit alone is never stop proof: only the Server's scoped
termination establishes `worker_stopped`. Stop frees the conversation for another
turn; an OVRCR operation already accepted may finish.

A recoverable provider failure or tool timeout while leased follows the same
checkpoint rollback, preserves the failed attempt without promoting it, and
keeps watches available for a later wakeup. An unleased failure commits the fixed
`Failed` message and requests Retire; an unleased timeout requests Stop.
A failed reporting commit blocks further admissions and requests Stop. The worker
never launches a replacement child or resends a prompt, and an uncertain tool
result holds the turn until the Server terminates it or the user stops it.

## Model selection and settings

`ovrcr operator models` prints `provider/id<TAB>name`, or JSON
`{"models":[{"provider":"...","id":"...","name":"..."}]}`, bounded to 4096
validated, deduplicated entries. An empty native list prints no invented
defaults and points at `ovrcr operator setup`.

`operator.model` is one optional `provider/model` string in the shared settings
document. Empty/unset uses the dedicated profile default captured at worker
startup; an explicit value overrides it and is captured per accepted Submit,
including warm turns. The legacy `operator.provider`, `operator.chat_gpt_model`
and `operator.grok_model`
keys are not translated; the unknown-setting findings identify them, and the
document is never rewritten automatically.

## Thinking level

`OperatorSettings.thinking_level` is a typed catalog enum (`default`, `off`,
`minimal`, `low`, `medium`, `high`, `xhigh`, `max`), defaulting to `default`.
Submit captures it beside the model in the same durable transaction. A settings
edit never reconfigures an accepted turn; the next turn applies it after model
selection, including checkpoint resume and warm turns. There is no
per-conversation override. Returning to `default` restores the captured
model-specific default instead of retaining an earlier explicit level.

Pi can silently clamp `set_thinking_level` while reporting success. The worker
reads `get_state.thinkingLevel` and `get_available_thinking_levels` after applying
the request and validates the bounded list before prompting. `ReasoningSelected`
updates retained conversation metadata without adding a transcript row. The
summary publishes the actual level, supported levels and captured request so the
Dashboard can distinguish substitution from a saved change for the next turn.
Before the first selection the level and supported list remain unknown; opening
the conversation never launches Pi just to discover capabilities.

The header control and `Ctrl-t` share one picker. It writes `SetSetting` through
the Outbox and waits for the Server's settings publication; a refused write
leaves the saved value unchanged and displays the error. The controls move to a
second header row when the title and controls cannot fit and at least 50 content
columns remain. A tighter on-demand sidebar view prioritizes controls over the
title. The ordinary 80×24 and 60-column views retain the model title.

## Removed engine

The previous Rig-based engine, its custom OAuth login, embedded Bash tool,
provider-specific request transformation and SDK model-history persistence are
gone. Root dependencies on `rig`, `tokio`, `futures`, `reqwest` and
`jsonwebtoken` were removed for the Operator. Legacy Operator rows, journals,
model-history bytes and credentials stay on disk untouched and hidden from the
Dashboard; see [Use the built-in operator](../operator.md#existing-data).
