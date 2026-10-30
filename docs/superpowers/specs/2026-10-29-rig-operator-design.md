# Concurrent built-in OVRCR operator design

Approved by Kyle in this conversation on 2026-10-29; implementation authorized using GPT-6.1 Sol subagents.

## Intent
A pinned global Operator hub operates ordinary OVRCR projects, workspaces, terminals and agent sessions through natural language. It has native Dashboard chat, multiple concurrent conversations, saved conversation history and asynchronous token streaming. It acts only on user requests, without per-command approval prompts.

## Product contract
- Operator sits above projects. The content area contains conversation history plus the selected transcript/composer; narrow layouts toggle history/transcript.
- Each conversation has its own provider/model, title, target context, draft, scroll position, running/error status and unread completion marker.
- Multiple conversations execute and stream concurrently; one active turn per conversation. Navigation never pauses work or retargets it.
- ChatGPT and Grok subscription sign-in are required. No implicit API-key billing fallback. Pick provider/model for each new conversation and keep it fixed.
- Enter sends; Shift-Enter inserts newline; paste never submits; Ctrl-g returns to Browse while retaining drafts; lowercase o opens Operator. A busy conversation can be drafted into but not submitted again.
- The previous nonoperator project/workspace/session is visible as context. Each submission captures stable identities (including session run); later navigation affects future submissions only. Do not automatically upload terminal content.
- Stop cancels the selected current operator turn only. Previously accepted OVRCR operations may finish; never claim rollback or infer their outcome. Interrupted commands never automatically replay.
- Persist completed model context and visible partial/interrupted history. New Conversation starts fresh context without deleting old history. Context exhaustion offers New Conversation; no automatic summarization in v1.
- Initial titles derive from the first message; no extra title-generation model request.
- No background watching, autonomous triggers, scheduler replacement, MCP/plugin framework, or Pi workflow-controller replacement.

## Architecture
- Preserve synchronous Server dispatcher, sole socket writer, one active Dashboard and total fifty live-session slots.
- One retained Session row per operator conversation; explicit Workspace vs OperatorConversation scope, no fake project or registered workspace.
- Each active TURN launches a same-executable internal worker through existing Session::spawn_registered/admission, with an existing managed PTY as diagnostic/lifecycle anchor. Worker chat uses typed protocol, not terminal bytes. Active workers count toward fifty; history-only conversations allocate no PTY.
- Worker owns a current-thread Tokio runtime and Rig agent/stream. It emits bounded batched events through an authenticated supervisor exchange; the Server owns chat persistence in dedicated tables on the existing SessionStore connection. Keep Rig SDK types out of shared crates; serialize versioned model history for the journal.
- IDs/epochs: every event and control carries conversation + turn + session/run and sequence; reject stale identities and deduplicate replayed batches. Persist before acknowledgment/UI publication. Coalesce UI notifications; load durable history to recover display gaps.
- FinalResponse commits completed turn, then worker remains alive until Server-owned exact-run controlled termination certifies stop and frees slot. Never use natural exit as controlled-stop proof. Simultaneous retirements must not be dropped by the single-job Dashboard Lifecycle worker; perform scoped control handling outside dispatcher/retained locks through existing termination internals.
- Stop durably interrupts current turn before retiring its authority and terminating its owned worker group. Other conversations continue. Uncertain stop blocks another run in that conversation until existing ownership acknowledgment contract is satisfied.
- Bash remains in supervised ownership chain; no detached helper process owner or second lifecycle store.
- Defaults: one tool at a time within a conversation; 64 KiB combined retained output per Bash command with explicit truncation; 120-second default timeout, 600-second maximum. Timeout aborts that turn via owned worker termination. Run bash --noprofile --norc -c. Default cwd is captured workspace, else dedicated operator workdir; explicit cwd permitted. Use absolute matching OVRCR executable, inherited instance socket/config and --json. Exclude model credentials from shell environment, arguments, logs and chat.

## Dependencies and authentication
- Pin rig =0.45.0, narrow agent/reqwest/rustls features; raise workspace MSRV to1.97.1. Add Tokio/futures only where worker/auth requires; no new workspace crate.
- ChatGPT: official SIWC dynamic registration + browser OAuth/PKCE, identity/grant validation and refresh, public Responses route. Do not use Rig's legacy chatgpt preset/Codex client identity.
- Grok: port MIT Pi OAuth at earendil-works/pi f09d01b85e0384647fcab6779d35ab9dabe04be3 packages/ai/src/auth/oauth/xai.ts and device-code.ts. Device code auth.x.ai/oauth2/device/code, token/refresh auth.x.ai/oauth2/token; feed OAuth bearer token to Rig's xAI Responses provider at api.x.ai/v1. Preserve license notice.
- Credential files are instance-owned mode0600 under private directory0700, atomically replaced, refresh read/modify/write serialized per account. Never hold refresh lock for inference lifetime. No automatic reading/modification of personal Pi/Codex profiles.
- Models: use ChatGPT's documented discovery; Grok supported curated tool-capable catalog (no false account-entitlement claim). Actual authorized inference is acceptance evidence.

## Acceptance
Real worker -> framed protocol -> Server -> Dashboard tests, concurrent/interleaved streams, independent Stop, restart/no replay, durable transcripts, backpressure and responsive terminal input, capacity and simultaneous retirement, schema/settings migration, auth/refresh wire fixtures, real Bash/CLI isolated fixtures, native reviewed-checkout CUA with screenshots/accessibility. Distinguish offline, live provider, macOS native and Linux evidence. No live provider calls until separately explicitly authorized. Feature PR, required checks/reviews, no merge without authorization. Work diary required.
