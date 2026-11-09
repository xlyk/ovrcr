# Replace Operator with a single pi RPC conversation

## Context

Replace OVRCR's custom Rig Operator with installed pi driven through its RPC protocol. Expose one persistent conversation per OVRCR instance, with no conversation creation form, list, switching, or archive controls. The user selected a dedicated pi profile, pi's standard tools, and a fresh conversation with all legacy Operator data preserved on disk.

## Reviewed behavior and source basis

Reviewed clean branch `feat/operator-review`, revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`. This was a static review; no app, tests, provider calls, or migrations ran.

- `src/operator/worker.rs:44-83,194-242,637-773` runs one managed same-executable worker per user message. It builds a Rig agent with custom OAuth, one Bash tool, a 20-model-call limit, and completed SDK history. Authenticated Reporter exchanges commit visible text/tool receipts before acknowledgement.
- `crates/ovrcr-runtime/src/server/operator.rs:174-289,388-514` owns admission, spawn, Stop, and retirement. `retained/operator.rs:17-36,97-249,362-508` owns multiple conversations, durable messages, turn/run identities, replay receipts, and Rig history in the existing SessionStore. Model completion and controlled process-stop proof are separate.
- `crates/ovrcr-tui/src/dashboard/operator.rs:26-171,363-467,597-823` keeps a chat map, selection, New form, and a Quick-message create/submit transaction. Quick currently creates another conversation. `docs/operator.md:26-67,133-184` documents multi-chat, settings, archive, and recovery.
- Existing pi integrations are blocking subprocess loops in `crates/ovrcr-runtime/src/task_runner.rs:64-320` and `server/title.rs:388-580`. Copy their strict LF-delimited JSON, bounded nonblocking pipe handling, and settled-event semantics. Do not reuse their task persistence or separate-group ownership.
- Installed pi is **0.87.1**, executable `/Users/xlyk/.local/bin/pi`, package `/Users/xlyk/.local/lib/node_modules/@earendil-works/pi-coding-agent`. Its `docs/{cli,rpc-commands,rpc-extension-ui,json,providers,environment-variables}.md` and `dist/{main.js,modes/rpc/rpc-mode.js,core/agent-session-runtime.js,core/session-manager.js,core/tools/bash.js,core/extensions/types.d.ts}` were read. Use installed 0.87.1 for acceptance; do not install or upgrade pi implicitly.
- Pi has no RPC login. Native `/login`, `/logout`, and `/model` run in its interactive TUI. Default tools are `read,bash,edit,write`. `set_model` uses exact `provider` and `modelId`; CLI `--model` can fuzzy-match. `agent_end` is not final; use `agent_settled`. Pi persists aborted turns, so the completed-context checkpoint must remain separate from the current attempt.
- Pi's native Bash backend uses `detached:true`. `createBashToolDefinition(...,{operations})` permits an attached execution adapter. An awaited extension `tool_call` hook can preserve durable admission through RPC's `extension_ui_request`/`extension_ui_response`; ordinary `tool_execution_start` cannot.

Primary upstream references: [RPC commands](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc-commands.md), [JSON events](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/json.md), [extension UI](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc-extension-ui.md), [CLI](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/cli.md), [authentication](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/providers.md). The installed-version sources, rather than changing upstream main, define this implementation.

## Approach

### 1. Add the pi transport and dedicated-profile integration

Add private root modules `src/operator/pi.rs` and `src/operator/rpc.rs`, plus `src/pi-operator-extension.mjs`. No reusable RPC client currently exists: the task/title loops embed different lifecycle owners. Keep these helpers under Operator; do not refactor scheduled tasks or native interactive Pi reporting as part of this change.

`pi.rs` owns executable resolution and command construction. Resolve `OVRCR_PI_EXECUTABLE`, otherwise `pi` on PATH, exactly as the existing title/task integration does. Resolve all instance paths through `RegistryPath::resolve`. Create `<instance>/operator-pi` and `<instance>/operator-workdir` with mode `0700`; reject symlinked/non-directory/foreign-owned paths rather than replacing them. Use a private `0700` `operator-pi/turns/<turn-id>` directory per attempt. Set child umask `077` so native auth/session/temp files are private.

Set `PI_CODING_AGENT_DIR=<instance>/operator-pi`, `PI_TELEMETRY=0`, `PI_SKIP_VERSION_CHECK=1`, and `OVRCR_EXECUTABLE` to the matching absolute OVRCR binary. Override inherited Pi session-directory variables. Preserve public instance routing and ordinary tool environment, including HOME/PATH; remove the private reporting/delegation variables already stripped by `src/operator/bash.rs:147-175` and `agent_runner.rs:94-104`, plus `OVRCR_PI_ENTRY`, `OVRCR_CLAUDE_ARTIFACTS`, `BASH_ENV`, `ENV`, and inherited `PI_SESSION_*`. Strip inherited Pi provider credentials using the installed `providers.md` environment-variable table, its Anthropic aliases, and cloud-provider credential/config variables. Never copy `operator-auth/` or the personal pi profile, read bearer material into Rust, or pass credentials in argv. The dedicated profile is configuration isolation, not an OS sandbox; explicit native profile configuration remains pi's responsibility.

Use this runtime launch, with pipes for stdin/stdout and continuously drained bounded stderr:

```text
pi --mode rpc --no-extensions --no-skills --no-prompt-templates
   --no-themes --no-context-files --no-approve --offline
   --tools read,bash,edit,write
   --session-dir <attempt-dir> --session-id operator-<conversation-id>
   --append-system-prompt <static-operator-policy-text>
   --extension <private-materialized-pi-operator-extension.mjs>
```

Pi's process cwd stays `<instance>/operator-workdir` for the lifetime of this global conversation. Captured workspace/session context remains explicit per-message target information. The policy must instruct pi to use absolute captured paths for file tools and `cd <captured cwd>` inside Bash when operating the selected workspace. Do not mutate the pi session header or silently retarget the ongoing turn when the Dashboard selection changes.

Keep the existing policy's matching CLI, exact identities, requested-task-only, no implicit terminal-content reads, no credential inspection, no watchers/workflow-controller authority, and no rollback/replay claims. Remove “start New Conversation.” Explain that pi compacts the same conversation and that current tools have pi-native schemas.

`rpc.rs` supplies a blocking, bounded child driver used by the worker and `operator models`. Split only on LF, tolerate CRLF, cap each complete or incomplete JSON record at 16 MiB, and bound stderr retention to 64 KiB without persisting its raw contents. Use request IDs for every command/response; process unsolicited events while awaiting replies. Copy task runner's bounded read batches (16 × 16 KiB) and 10 ms polling so output cannot starve cancellation. Startup/control commands get a 20-second deadline; prompt execution has no added wall-clock deadline. Unknown ordinary events may be ignored; malformed JSON, failed required commands, premature EOF, or required-extension errors fail the turn.

Use these fixed user-facing failure messages; never append native stderr/provider bodies: `"Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE."`, `"No Pi model selected; run ovrcr operator setup."`, `"Configured Operator model is unavailable in the dedicated Pi profile."`, `"Operator Pi context is unavailable; saved data was preserved."`, `"Pi Operator integration is unavailable; no prompt was sent."`, `"Pi request failed; inspect the partial journal before retrying."`, and `"Unsupported Pi interaction; the turn was stopped."`

Keep process ownership at the callers: the managed worker launches pi **without** `process_group(0)` or setsid, so the existing Session owns pi and its tool children. `operator models` launches a task-owned standalone group and performs bounded group termination/reaping on all exits; it never registers a Server session.

### 2. Cut over the singleton wire, storage, worker, and callers together

This is one atomic contract change. Apply the protocol, retained/server, root worker, settings/CLI, and Dashboard changes below together before running their suites; do not commit a mixed old/new protocol. The transport/extension from step 1 is its prerequisite.

#### Singleton requests and identities

In `crates/ovrcr-protocol/src/operator.rs`, replace `OperatorRequest::{Create,List,Close}` with unit variants `Current` and `Open`. Replace `OperatorResponse::{Created,Conversations,Closed}` with `Current(Option<OperatorConversationSummary>)` and `Opened(OperatorConversationSummary)`. Keep `Read`, `Submit`, `Stop`, their exact-run checks, internal `OperatorConversationId`, `OperatorTurnIdentity`, and `ServerEvent::OperatorChanged`.

`Current` never creates a record or process. `Open` atomically gets or creates the sole record and never starts pi. Full Operator opening sends Open; Dashboard startup sends Current. Quick message sends Open only on its first explicit Send when Current returned None. A cancelled unsent Quick draft creates nothing. Multiple/repeated Open requests return the same IDs without resetting state.

Remove `OperatorProvider`. Add:

```rust
pub struct OperatorModel {
    pub provider: String,
    pub id: String,
}
```

Change the summary's `provider`/`model` fields to `model: Option<OperatorModel>` and remove `archived`; retain run, stopped proof, id, session, title and current_turn. Keep title fixed as `"Operator"` for the new singleton rather than deriving a chat-list title.

Replace `OperatorWorkerStart`'s provider/model/history fields with `model: Option<String>` (the captured `operator.model` override) and `checkpoint_turn: Option<OperatorTurnId>`. Keep identity/text/context. Add `OperatorWorkerEvent::ModelSelected(OperatorModel)`. Change `Completed(OperatorModelHistory)` to unit `Completed`; remove `OperatorModelHistory`, its validation, and `MAX_OPERATOR_HISTORY_BYTES`. `ModelSelected` and Completed update retained metadata/state without copying them into the visible transcript. Tool/text/error events retain current bounds and sequencing.

Use LSP references before removing exported symbols. The caller inventory includes protocol wire snapshots/re-exports, `src/{operator,cli/operator.rs,cli/args.rs,report/reporter*}`, runtime `{retained/operator.rs,server/operator.rs,server/connections.rs,server/operator_tests.rs,server/tests.rs}`, TUI `dashboard/{operator.rs,operator/render.rs,event_loop.rs,state.rs,palette.rs,settings_editor.rs}`, and the operator/TUI/GUI acceptance tests. Rerun references for `OperatorRequest`, `OperatorResponse`, `OperatorProvider`, `OperatorModelHistory`, `OperatorSettings`, and `OperatorWorkerStart`; migrate every returned caller, with no compatibility variants. Bump protocol **75 → 76** in `crates/ovrcr-protocol/src/codec.rs` and update its wire contract snapshots.

#### One durable record, preserved legacy data

In `retained/operator.rs`, add an idempotent `migrate_singleton_conversation(connection: &Connection) -> Result<()>`, called by the existing migration transaction in `config.rs`. Advance schema **28 → 29**; keep 28 accepted as an upgrade source. Add:

```sql
ALTER TABLE operator_conversations
  ADD COLUMN singleton INTEGER NOT NULL DEFAULT 0 CHECK(singleton IN (0,1));
ALTER TABLE operator_conversations ADD COLUMN pi_checkpoint_turn INTEGER;
CREATE UNIQUE INDEX operator_single_conversation
  ON operator_conversations(singleton) WHERE singleton=1;
```

Check column existence before ALTER and use `IF NOT EXISTS` for the index. Fresh/in-memory schema constructors must produce the same structure. Validate foreign keys before commit. Migration inserts no conversation, changes no archive/stopped proof, and does not touch legacy message/model-history bytes.

Replace `create_operator_conversation`/`list_operator_conversations` with `current_operator_conversation(&self) -> Result<Option<OperatorConversationSummary>>` and `open_operator_conversation(&mut self, cwd: &Path) -> Result<OperatorConversationSummary>`. Reuse the existing transaction that creates the retained Session and journal row, setting `singleton=1`, title `"Operator"`, and blank provider/model storage until the first ModelSelected. For singleton rows, store pi provider/id as plain strings in the existing provider/model columns; both blank means `model=None`. Never parse legacy provider enum JSON into the new type.

Every ordinary Read/Submit/Stop/worker bootstrap/exchange must require the singleton marker. Keep current owner, retained run, Running-turn exclusion, fifty-live-slot admission, commit-before-spawn, and batch digest/gap checks. `apply_operator_batch` stores ModelSelected in provider/model; successful Completed sets `pi_checkpoint_turn` to that batch's turn in the **same transaction** as Complete and its receipt. No Rig-history reads/writes remain.

Snapshot the effective optional `operator.model` when Submit is durably accepted, not later when the worker asks for Start. Add nullable `model_override TEXT` to `operator_turns` in this migration and populate it during `submit_operator_turn`; Start reads that turn field. A settings edit cannot change an accepted turn.

Retain all legacy rows, `operator_model_history`, credentials, and existing recovery evidence on disk. Hide legacy Operator rows from the Dashboard and Archived palette. Keep them in terminal diagnostics, and keep exact-run `acknowledge-stopped` working without requiring singleton membership. That recovery path must interrupt an old unfinished turn without decoding its legacy model history. Remove Operator archive/unarchive/delete routes, including generic terminal lifecycle entry points; explicitly refuse them for Operator scope instead of relying on a foreign-key error. No new submit/reopen route may revive a legacy conversation. Preserve generic terminal Stop refusals and existing independent recovery guidance.

#### One pi conversation with completed-context checkpoints

Keep `src/operator/worker.rs::run(identity: OperatorTurnIdentity) -> Result<()>`, managed reservation, Reporter bootstrap, sequenced commits and Server-owned retirement. Replace its Rig implementation with the blocking RPC driver. No Tokio runtime or provider HTTP code remains.

For every submitted turn:

1. Obtain authenticated Start before creating pi resources. Create its attempt directory exclusively; an existing directory is an uncertain prior attempt, not permission to replay.
2. Launch a fresh native session in that empty directory with deterministic session ID `operator-<conversation-id>`. This first empty session resolves the dedicated profile's current model/thinking defaults without a second pi process. Read `get_state`, retaining those defaults and the expected session ID. Require the trusted extension's ready notification described below.
3. If `checkpoint_turn` exists, copy `<instance>/operator-pi/turns/<checkpoint-turn>/session.jsonl` into this attempt's `session.jsonl` with exclusive private creation. This one necessary copy isolates pi's append/rewrite behavior from the last completed context. Send RPC `switch_session` with this absolute copy; require `success:true`, `cancelled:false`, a new ready notification and matching `get_state.sessionId`. Never use `--resume`, `--continue`, partial-ID lookup, `new_session`, or `fork`. If the checkpoint is missing, malformed, nonprivate, symlinked, or has the wrong session ID/cwd, fail without starting over or overwriting it.
4. Resolve the model: split an explicit override at the **first** `/`; otherwise use the fresh profile model from step 2, not the resumed session's old model. `get_available_models` must contain the exact provider/id. Send `set_model` with `provider` and `modelId`; verify the returned model and a final get_state. Restore the fresh profile's thinking level using `set_thinking_level` after set_model. Send `set_auto_retry` with `enabled:false` and `set_auto_compaction` with `enabled:true`. Model/auth/configuration failures never trigger model fallback. Missing profile/model gets fixed guidance to run `ovrcr operator setup`.
5. Commit ModelSelected, then send exactly one RPC `prompt` whose message is the existing captured-context JSON plus user text. Prefixing user text with context prevents input such as `/new` from becoming a native command. Do not expose steering/follow-up queues; another Send while busy preserves the draft.
6. Stream text and durable tool receipts as below. `prompt.success` is acceptance only. Require `agent_settled`, final assistant `message_end.stopReason == "stop"`, no unresolved admitted tools, and no transport/extension error for success. Auto-compaction stays inside this one conversation; use its completion before declaring the turn settled. Pi owns the agent loop; remove the old Rig-only 20-call limit rather than implement a second model scheduler.
7. After success, send clear_queue then abort with bounded replies, close stdin, drain/reap the exact pi child within two seconds. Keep the managed worker alive. Require a private regular native session file at the path reported by get_state, confined to this attempt directory. Once pi has exited, rename that file to the attempt's `session.jsonl` if needed; fsync the file and directory. Only now commit unit Completed, atomically promoting the checkpoint turn. Then request authenticated Retire. Server termination still removes ordinary background children and establishes `worker_stopped`; pi exit alone never does.
8. On any failed/interrupted/uncertain turn, do not promote its file. Keep the partial visible journal and attempt files as evidence. The next explicit turn, after controlled-stop proof or independent acknowledgement, copies the last completed checkpoint. This rolls back **model context only**, never external operations, files, or terminal input. No automatic prompt replay occurs. With no prior completed turn, the next attempt starts empty with the same deterministic pi conversation ID.

Bound successful checkpoint storage: after an acknowledged Completed transaction, remove only the previous `checkpoint_turn`'s `session.jsonl` and its now-empty directory. That file is a superseded copy; the new native session contains its history and the SQLite journal remains unchanged. Do this before Retire, outside Server locks. Never remove the new checkpoint, legacy data, failed/interrupted attempt files, or anything after a lost/refused completion acknowledgement. A failed deletion leaves preserved extra bytes and does not invalidate the completed turn; it is not permission to delete a larger directory tree.

Keep hard Stop in the existing Server path: durably interrupt, revoke authority, release locks, terminate the owned group, record proof. Install worker SIGTERM/SIGHUP flag handlers; while shutting down, best-effort send clear_queue/abort and drain, without attempting further journal commits after authority is revoked. Neither a reply nor EOF substitutes for Server proof. A tool timeout requests authenticated Stop. Other worker-detected transport/provider/bootstrap failures commit Failed, then request Retire. If a reporting commit fails, block further tool admissions, best-effort abort pi, request authenticated Stop, and remain alive if that request cannot establish termination. Never launch a replacement child or automatically resend a prompt. Signal only current owned processes; add no process scanner or recovery-by-remembered-PID.

#### Standard pi tools with durable admission and attached Bash

Materialize the bundled `src/pi-operator-extension.mjs` into this private profile using the existing task-extension embedding pattern. All discovered extensions, skills and context files stay disabled. The only custom code is the OVRCR lifecycle adapter; pi supplies the four tool definitions, validation, file operations, rendering/truncation, inference and context management.

The extension overrides only `bash`, using exported `createBashToolDefinition(process.cwd(), { operations, exposeSessionEnvironment:false })`. Implement `operations.exec(command, cwd, {onData, signal, timeout, env})` with `/bin/bash --noprofile --norc -c`, `detached:false`, piped stdout/stderr, and no new process group. Stream both outputs through onData. Reuse the native task-extension pattern for waiting on child `exit` and draining trailing output without hanging on a completed background child's pipe. Keep pi's `command`/optional `timeout` schema, native 50 KiB/2,000-line output behavior, and no default timeout. On timeout, retain structured timed-out state for that call, stop the entire Operator turn, and never return control to another model request. No detached anchor or second cleanup ledger.

On `session_start`, synchronously emit `ctx.ui.setStatus("ovrcr_operator_contract", JSON.stringify({version:1, sessionId:ctx.sessionManager.getSessionId(), tools:pi.getActiveTools()}))` after registering hooks/Bash. Require exactly the four enabled tools and matching session ID before prompting. A missing/failed extension refuses execution; never fall through to stock detached Bash. Duplicate binding notifications are harmless.

For `tool_call`, await `ctx.ui.confirm("OVRCR Operator tool admission", JSON.stringify({toolCallId,toolName,input}), {signal:ctx.signal})`. The worker validates the four-tool allowlist, ID and arguments bounds, flushes preceding assistant text, commits ToolCall through Reporter, and only then replies `{type:"extension_ui_response",id,confirmed:true}`. Serialize admissions: do not grant the next tool until the previous result is durable. Refusal returns `{block:true,terminate:true,reason:"OVRCR tool admission refused"}` and the worker aborts/stops. This is an internal durability handshake, not a replacement for any provider/user safety approval.

Before granting a tool, require the matching completed assistant message to have `stopReason` `"toolUse"` or `"stop"`, with that exact call ID/name/arguments in its ToolCall blocks. Refuse `"length"`, `"pending"`, `"error"`, `"aborted"`, `"deferred"` and unknown outcomes before effects. The native message-types contract defines these literals; do not treat nonempty partial output as completion.

For `tool_result`, use the same awaited RPC dialog with exact title `"OVRCR Operator tool result"` and payload `{toolCallId, output, truncated, status}`. Reduce native content to text, with literal `[image result omitted]` for image blocks; never put base64 images in the journal. Replace NUL with U+FFFD, bound UTF-8 output to 64 KiB, and combine that truncation with native `details.truncation`. Status maps native `isError:false` to Succeeded, errors to Failed with no exit code unless the attached Bash adapter recorded one, and its timeout to TimedOut. Keep per-call execution facts so status is not parsed from prose. Commit ToolOutput before acknowledgement and before allowing the next tool/model step. On refused result acknowledgement or timeout, abort and hold the hook until owned termination; do not feed uncertain outcomes back into the agent loop.

Only answer these exact two internal dialog titles, with valid outstanding IDs/payloads. Cancel every other dialog and report a fixed unsupported-interaction error; never blanket-approve native dialogs. Ignore unrelated display notifications. Do not journal duplicate `tool_execution_start/end` notifications; they can cross-check the admitted lifecycle. Partial tool updates remain transient; the existing durable call/result UI remains the display contract.

For assistant text, consume only `assistantMessageEvent.type == "text_delta"` from message_update, coalesced on the existing 50 ms/64 KiB cadence. Track the current assistant message's streamed text to avoid appending message_end twice; append any unstreamed suffix from authoritative message_end, and fail if final text contradicts an already-committed prefix. Do not expose thinking blocks. Preserve control-character escaping in `display_text`. Map provider/extension/raw stderr failures to fixed local messages rather than storing remote error bodies or tokens.

#### Native setup, model settings, and deletion of the old engine

Replace public `operator login|status|logout` and provider arguments with `ovrcr operator setup`; retain `ovrcr operator models` without a provider argument and the hidden worker. Setup must require a TTY and reject `--json` before launching. It prints instructions for `/login`, `/model`, `/logout`, then execs native pi with the dedicated environment, stable workdir, `--no-session --no-tools --no-extensions --no-skills --no-prompt-templates --no-themes --no-context-files --no-approve`. Do not use `--offline` for interactive setup, where explicit native auth/catalog work must function. No Server is started. There is no embedded OAuth browser flow.

Models uses the shared RPC driver with `--no-session --no-tools` and the same isolation flags/profile; request get_available_models, then shut down. Emit only validated/deduplicated provider/id/name, never baseUrl/headers/auth/raw model objects. Text output is `provider/id<TAB>name`; JSON is `{"models":[{"provider":"...","id":"...","name":"..."}]}`. An empty native list prints an empty list plus setup guidance on stderr, not invented defaults. Bound model count to 4096 and reuse the 16 MiB frame/20-second deadline. Listing proves discoverability, not inference entitlement.

Replace `OperatorSettings` with `{ pub model: Option<String> }`, default None. In catalog/loader/editor/matrix, replace the three provider-specific rows with `operator.model`, `Shape::Text("a provider/model identifier")`, group `"Operator"`, label `"Model"`. Reuse `DelegationSettings::valid_model` and first-slash splitting, which permits slashes within real model IDs. Empty/unset means the dedicated profile's current default before each turn; an explicit value overrides it. Legacy keys are not translated or honored; the existing unknown-setting findings can identify them. Do not rewrite the user's settings document automatically.

Delete `src/operator/{auth.rs,login.rs,bash.rs,worker/chatgpt_http.rs}` and all Rig-specific worker code. Update `src/operator/mod.rs` and CLI dispatch. Remove operator-only `rig`, `tokio`, `futures`, `reqwest`, `jsonwebtoken` dependencies from root Cargo.toml/lock; retain shared dependencies and the existing Rust MSRV. Move `signal-hook` to root regular dependencies for the worker's termination flags. Remove obsolete custom-auth tests and synthetic OAuth-only fixtures once their replacement tests exist; retain and repurpose `tests/support/operator_provider.rs` as the loopback streaming provider. Do not retain fake empty Rig history, deprecated aliases, custom provider mapping, or the old test HTTP endpoint.

#### Single-conversation Dashboard

Replace `Hub.chats`, selected/list/collapse/hover/offset/NewChat state with one optional Chat and one load state. Keep its journal paging, draft, context, scroll/expanded receipts, unread, activity and exact-run stop state. Pending requests still correlate by request ID. Remove List/New/Close pending variants, per-chat sorting/catch-up loops and provider/model creation form.

Keep existing open shortcuts (`o`, title control) and Quick message (`Ctrl-o`). Full view opens the singleton directly with composer focus. Replace the sidebar conversation section with one fixed Operator navigation row showing activity/unread; no count, collapse arrow, plus button, child rows or close hit targets. The ordinary Projects sidebar remains. Remove Ctrl-n, conversation switching, close confirmation and archived-Operator actions from input, palette, menu/help and mouse geometry. Ctrl-l can still toggle/focus the ordinary sidebar; it must not reveal a chat list.

Quick message keeps its overlay, context capture, Unicode editing, paste-only behavior, dismiss/late-receipt protection and draft retention. It targets the same singleton. Keep its generation fence for stale overlay replies; remove only per-created-conversation bookkeeping. If the singleton is busy or stop-unconfirmed, Send preserves the draft and gives that reason; do not queue, steer, create another chat, or submit automatically when it becomes idle. Open+Submit is an explicit transaction with one frozen draft/context and a retry that reuses the singleton. Dismissal never steals the view on a late receipt.

Keep Ctrl-x and empty-full-composer Ctrl-c Stop. Quick's empty Ctrl-c remains a no-op. Leaving/reopening Operator keeps history and drafts; only rendering the loaded tail clears unread. Preserve the 60×16 minimum, narrow/wide layout rules and escaped expandable tool receipts. Show header `Operator · pi · <provider>/<id>` once ModelSelected is known, otherwise `Operator · pi`; show captured target separately from the fixed agent workdir. Setup errors direct the user to `ovrcr operator setup`; no model identifier guessing.

## Critical files and anchors

- `crates/ovrcr-runtime/src/session/process.rs:109-149` and `session/mod.rs::terminate_guarded` — only the owned group and PTY-attached groups receive controlled-stop proof; stock detached pi Bash cannot use this proof.
- `crates/ovrcr-runtime/src/config.rs:265-440` — run the singleton/checkpoint migration inside the existing immediate transaction, preserving foreign-key dependents and old-writer fencing.
- `crates/ovrcr-tui/src/dashboard/operator.rs::operator_dismiss_quick` and `operator_submit_to` — retain late-receipt and frozen-draft protections while deleting conversation creation.
- `crates/ovrcr-protocol/src/settings.rs::DelegationSettings::valid_model` — shared provider/model syntax splits once; do not reject IDs containing additional slashes.
- `tests/support/live.rs` — use existing isolated compiled-server ownership/cleanup, never a personal instance or profile.

## Assumptions and contingencies

- One conversation means one global conversation per OVRCR instance, not one per project/workspace. An idle conversation uses no live slot; at most one Operator turn runs at once.
- The dedicated profile, standard four native tools and fresh-start legacy preservation are explicit user decisions. There is no Reset/New conversation action in this change; pi auto-compaction maintains the conversation.
- The user-visible target remains the captured workspace/session. Pi's physical cwd is fixed for safe global-session resume, so the policy uses explicit target paths rather than changing session identity.
- The supported implementation contract is pi 0.87.1. If that binary is unavailable, finish offline fixtures and report installed-pi/native acceptance as blocked; never substitute Oh My Pi, an SDK agent, or the old Rig implementation. If a different pi version lacks a required flag/event/hook, refuse the turn with upgrade guidance instead of a compatibility fallback.
- An unsupported/corrupt checkpoint fails closed and preserves all bytes. No automatic history repair, session rotation, import or reset is authorized.
- Never delete legacy journals, credentials or uncertain attempt evidence during the cutover. Normal completed-attempt files referenced by checkpoints are durable state, not disposable fixtures.

## Verification

Run from `/Users/xlyk/Code/ovrcr-workspaces/feat-operator-review` or the execution worktree created from its reviewed revision. Use the repo's `tests/support/live.rs` fixtures for temporary HOME, OVRCR_HOME, socket, Git workspace, profile and owned processes. Never use the user's pi auth for automated or GUI acceptance. Tests must assert their expected count is nonzero.

### Permanent behavioral coverage

Rewrite the owning tests rather than retaining old two-conversation/auth assumptions:

1. **Storage/protocol:** seed schema 28 with multiple legacy chats, archived/idle/uncertain runs and Rig history. Upgrade; assert IDs and raw legacy payload bytes are preserved, foreign keys are valid, no singleton exists until Open, repeated/concurrent Open yields one fresh singleton, and legacy submit/archive/unarchive/reopen is refused. Verify Current/reconnect creates no worker. Test legacy acknowledge-stopped separately from singleton guards.
2. **Turn and checkpoint authority:** accept a first completed turn, then interrupted/failing turns, then a follow-up. Assert only Completed changes `pi_checkpoint_turn`, and its promotion is atomic with the batch receipt. Exercise identical replay, conflicting replay, stale run/owner, lost completion acknowledgement, crash before/after file sync and before/after SQLite commit, missing/corrupt checkpoint, and rejected existing attempt directories. Never infer process-stop proof from a terminal turn state.
3. **Compiled worker with fake RPC peer:** rewrite `tests/operator_worker.rs`/`tests/operator_e2e.rs` using a fixture pi executable. Verify command IDs/interleaved notifications, LF framing with Unicode separators, oversize complete and incomplete frames, startup/extension failure before prompt, exact model resolution, profile-default changes across resumed turns, config snapshot at submit, prefix-deduplicated assistant text, result errors, sequential durable admission and no command execution after refused/late acknowledgement. Raw provider/stderr credential sentinels must not enter journal, settings or Events.
4. **Actual pi tools:** add an installed-pi test named `installed_pi_operator_single_conversation` in `tests/operator_e2e.rs`, opt-in through `OVRCR_TEST_PI_EXECUTABLE` and ignored only when not explicitly selected. Use a local streaming OpenAI-compatible provider and synthetic model/auth in the fixture's `operator-pi`, following `scripts/validate-pi-rpc.py`. Drive the actual Dashboard model, Server, managed worker, pi RPC and extension. Provider prompts must cause real read/write/edit/Bash operations in a temporary workspace, and the test must observe the resulting file bytes and CLI output.
5. **Real lifecycle:** that installed test also holds streamed output before releasing completion; observes the same pi session ID and prior successful messages on turn two; interrupts a Bash command with an ordinary background child; proves owned child cleanup and stop proof; submits a third turn and verifies its provider context includes the last successful checkpoint but not the interrupted request/tool results. Keep the visible interrupted journal. Verify profile credential isolation, models/setup using no Server, automatic compaction retaining one conversation, and malformed pi/protocol failure without fallback.
6. **Dashboard:** migrate `tests/tui/operator.rs`, owning TUI unit tests, `tests/operator_scope.rs`, relevant `tests/gui.rs`, `tests/terminal_acceptance.rs` and `tests/terminal_acceptance/session_jump_operator.rs`. Assert one navigation row, direct open, no create/list/archive controls, shared Quick/full conversation, preserved drafts on busy/stop-unconfirmed, cancelled unsent Quick creating no row, late replies after dismissal, Unicode input, ordinary terminal input remaining responsive during Stop, narrow layout, unread and paged history.

Use deterministic fixture gates for races and capture actual filesystem/process outcomes. Delete obsolete custom-OAuth/provider-dialect tests rather than simulating a Rig interface around pi. Keep applicable lifecycle/privacy/context/geometry assertions under the new entry points.

### Commands

After the atomic cutover, run these focused suites and then the workspace regression gate once:

```sh
cargo test -p ovrcr-protocol --lib
cargo test -p ovrcr-runtime --lib operator
cargo test -p ovrcr-runtime --test settings_matrix
cargo test -p ovrcr-tui --lib
cargo test -p ovrcr --test tui operator
cargo test -p ovrcr --features acceptance-diagnostics --test operator_scope --test operator_worker --test operator_e2e
OVRCR_TEST_PI_EXECUTABLE=/Users/xlyk/.local/bin/pi cargo test -p ovrcr --features acceptance-diagnostics --test operator_e2e installed_pi_operator_single_conversation -- --ignored --nocapture
cargo test --workspace --all-targets --all-features
```

The installed test's local provider and private profile must be created by the fixture, not supplied from a real account. On Linux omit GUI/all-features as required by `docs/development/testing.md`; report platform-specific gates separately.

### Actual UI smoke

Follow `docs/testing-computer-use.md` with the enabled `computer` helpers. Launch `OVRCR_PI_EXECUTABLE=/Users/xlyk/.local/bin/pi rtk proxy just gui` from the reviewed checkout. `src/gui.rs::Demo` creates its own disposable root; obtain that root from the GUI's Events view or by printing `$OVRCR_HOME` in one fixture terminal, never by guessing a `/tmp` directory.

Add ignored `operator_pi_gui_provider` in `tests/operator_e2e.rs` using the repurposed `tests/support/operator_provider.rs`. It requires `OVRCR_OPERATOR_GUI_HOME` to name an existing fixture root containing `registry.sqlite3` and `server.sock`; it must refuse a root outside a task-owned GUI fixture. It seeds only that root's `operator-pi` with synthetic auth/models/settings for the loopback provider, prints `OPERATOR_PI_GUI_READY`, and serves until the fixture root disappears or a private `operator-pi/fixture-stop` marker appears. A private `operator-pi/fixture-release` marker releases a held stream. Reuse the same scripted tool/request scenarios as the installed-pi test, including a unique file marker, a held assistant stream, and a held Bash/background child; no live provider is contacted.

Run the provider alongside the still-open GUI launcher:

```sh
OVRCR_OPERATOR_GUI_HOME=/absolute/task-owned/ovrcr-gui-root cargo test -p ovrcr --features acceptance-diagnostics --test operator_e2e operator_pi_gui_provider -- --ignored --nocapture
```

Wait for `OPERATOR_PI_GUI_READY` before Send. Retain launcher/provider exit status, process ownership and cleanup evidence. Do not add production GUI settings or provider overrides for this acceptance helper.

Through real keyboard/mouse input: open Operator, observe the sole row/no New form, send a request that writes a unique fixture marker, observe text before provider release and an expandable file/Bash receipt, then send a follow-up proving prior context. Open Quick from another pane while busy and verify its draft stays unsent. Stop the held tool, observe Interrupted followed by confirmed stop, return to terminal input and observe independent command output, reopen Operator, resize to narrow/wide layouts, and reconnect the Dashboard without creating another conversation. Capture fresh screenshots and AX text for these states. A test-only render or screenshot without exercised input is not this smoke proof.

Report the tested revision, executed counts, installed pi version, singleton/session/checkpoint evidence, native captures, cleanup receipts and any unavailable checks. Authentication with a real account is a separate user-operated setup step; offline provider fixtures do not prove subscription entitlement. No diary write is needed for this planning-only artifact.
