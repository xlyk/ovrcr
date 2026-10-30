# Concurrent OVRCR operator implementation plan

**Goal:** Implement the approved native Operator hub with concurrent saved chats, async Rig streaming, Bash/CLI tools and ChatGPT/Grok subscription login.
**Spec:** docs/superpowers/specs/2026-10-29-rig-operator-design.md
**Execution:** GPT-6.1 Sol subagents, high reasoning for integration tasks; root owns coordination, final verification and diary. Implementers do not spawn reviewers. Every task gets independent spec/quality review.
**Baseline:** origin/main 54cc146f39344d8608dd81dbd9a7372f8298b875; feature branch codex/rig-operator. Preserve original dirty checkout.

## Global constraints
- One synchronous Server, one active Dashboard, fifty total running/paused Session slots; operator workers included.
- Multiple conversations run and stream concurrently; one active turn per conversation, independent cancellation.
- Same SessionStore owns durable identities and separate chat journal. Existing content-free event log stays content-free.
- No replay of interrupted commands, no API-key fallback, no live credentials or paid inference during implementation.
- Native chat input is typed protocol, never PTY input. Model/command waits and process termination stay outside dispatcher/retained locks.
- Every feature needs production-path integration tests, focused checks, required acceptance/regression checks and native CUA evidence.
- All shell commands use rtk (rtk proxy for commands not supported directly); GitHub through gh/git.
- Protect live sessions; fixtures own isolated HOME, socket, config, repo and provider data.

## Task 1: Protocol, conversation journal and settings foundation
Read architecture/runtime/testing guides and actual callers. Own crates/ovrcr-protocol, runtime retained/registry/settings (no worker implementation or Dashboard UI).
- Add focused protocol operator module defining provider, conversation/turn IDs, captured context, conversation summary/history, turn state and bounded worker-event batch types. Keep SDK types out.
- Add typed requests for create/list/read/submit/stop conversations and authenticated OperatorExchange supervisor commands/results. Append wire enums, bump protocol and regenerate snapshots.
- Add explicit SessionScope Workspace/OperatorConversation to retained/runtime/wire session representations, preserving old workspace records during migration. Propagate constructors/matches/inventory and global-scope handling; no fake project/workspace entries.
- Add dedicated conversation/message/turn/model-history tables under the existing SessionStore connection and migration version; one retained Session per conversation. Durable ordered batch application validates identities/sequence, deduplicates exact repeated batches, rejects conflicting/stale batches.
- Journal supports create/list/read paged history; submitted user message/captured context; partial assistant/tool output; final complete/interrupted/error state; versioned serialized Rig history; startup interruption of unfinished turns.
- Add typed operator defaults/provider/model settings through sole loader/editor snapshots. Never persist credentials in settings.
- Tests first: migration from prior schema retains all ordinary sessions; idle history opens no process; duplicate/stale/conflicting batches; independent interleaved histories; interrupted startup; bounded validation and malformed protocol; settings matrix.
- Produce a concise interface map for dependent tasks in task report. Run protocol/runtime focused suites, compile workspace all targets. Commit intended files.

## Task 2: Rig dependencies and subscription authentication
Own root Cargo.toml/Cargo.lock, src/operator/auth module and auth-focused tests/third-party notices. Coordinate module exports with root; do not implement Server or Dashboard.
- Verify released pinned dependency source. Add Rig0.45 narrow features/current-thread Tokio/futures, raise MSRV1.97.1 and affected docs.
- Implement protected per-instance credential store, atomic private files and per-account serialized refresh without holding lock for inference.
- Implement ChatGPT SIWC browser sign-in/current public Responses requirements. Port Pi Grok device OAuth/refresh state machine with pinned attribution and license.
- Expose provider-neutral asynchronous authenticate/status/logout/credential resolution and model listing/curated Grok models usable by worker. No automatic personal-profile import or billing fallback.
- Auth HTTP tests use task-owned local servers, including pending/slow_down/denied/expiry/cancel, refresh rotation without replacement token, concurrent refresh, persistence failure and secret redaction. No real login/inference.
- Preserve first-party docs/source references in docs/operator.md. Compile/test focused root auth path and dependency integration. Commit.

## Task 3: Managed conversation runtime and typed control
Consumes Task1 interfaces. Own runtime server/operator, supervisor/connection dispatch, retained runtime lifecycle integration; tests/ operator live fixtures. Add internal worker launch composition hook consumed by Task4.
- Register global retained conversation sessions and spawn one managed worker per active turn through normal publication/admission.
- Enforce one active turn per conversation, fifty total live slots, no global execution queue; preserve draft/request on refusal.
- Implement current-owner Dashboard submit/read/Stop and authenticated worker exchanges with conversation/run/turn fences and durable acknowledgment.
- Reuse Reporter leases/status/revisions and ordinary owned process termination. Keep calls off dispatcher and locks. Expose exact-run Stop/retire through scoped control request handling so concurrent completions are not dropped.
- Completed worker waits alive for controlled retirement. Stop marks interrupted before revocation; uncertain stop prevents restart. On restart no automatic launch/replay.
- Coalesce per-conversation Dashboard notifications and support durable catch-up; slow UI cannot block unrelated terminal input or other chats.
- Production-path tests: two real workers, capacity, duplicate submit, concurrent retire, Stop one while other runs, stale run/turn messages, journal failure, owner replacement, restart/no replay and slow Dashboard. Commit.

## Task 4: Async Rig stream and real Bash tool
Consumes Task1 journal/exchange and Task2 auth, Task3 launch/control. Own src/operator worker/tool modules, internal CLI composition and worker integration tests.
- Internal same-executable worker reserves proper scoped lease, builds configured Rig Agent using OpenAI Responses or xAI plus refreshed OAuth credentials.
- Drive true asynchronous stream; expose text/tool boundaries and final serialized model context. Batch deltas by64KiB maximum/50ms cadence; flush lifecycle/tool boundaries promptly. Tool concurrency1.
- Implement Bash tool with command/cwd,64KiB output cap,120s default/600s max timeout, no profile startup, correct exit/status/truncation. Use matching CLI/socket; keep token credentials out of subprocess environment.
- Bind captured context into prompt; prefer CLI --json for OVRCR; honor control refusals. No implicit screen upload or workflow-controller authority.
- On final response commit completion and await Server retirement; on timeout request same per-turn stop rather than leaving shell descendants. No auto replay after process death.
- Tests: deterministic fake streaming provider drives real agent loop/tool/CLI, tool stdout/stderr/nonzero/timeout/output cap, async partial text before final, multiple workers and exact interruption/retire. Commit.

## Task 5: Native concurrent conversation UI
Consumes Task1 public types and Task3 events. Own TUI dashboard/operator and narrow shared rendering/keymap/input integration; tests/tui and terminal acceptance.
- Pinned global Operator hub, conversation list, selected transcript and multiline composer; narrow list/transcript toggle.
- Per-conversation draft/context/scroll/follow-tail/unread. One busy conversation permits drafting but no second submission. Different chats stream concurrently and remain active when viewing terminals.
- Route strictly by conversation/turn/run/sequence; never focused-pointer routing. New conversation picks provider/model; title first message; context chip captures last project/workspace/session.
- Enter sends, Shift-Enter newline, paste never submits, Ctrl-g Browse retaining draft, o opens hub; overlays retain priority. Native chat never emits PtyBytes.
- Show command/cwd/output/truncation/result, Stop, running/finishing/interrupted/error; history readable without launch; New Conversation for context exhaustion.
- Tests first: A/terminal/B/A while streaming, interleaved tokens/tools, per-chat draft/scroll/context, late stale events, unread completion races, Unicode/paste/narrow rendering and real event-loop integration. Commit.

## Task 6: Acceptance, documentation, final review and delivery
- Add/run real compiled-worker + fake HTTP provider + socket + Dashboard end-to-end acceptance with actual Bash/CLI fixtures, not helper-only proofs.
- Run required workspace all-target/all-feature tests and clippy on macOS, format/diff checks, relevant Node regression, Linux/headless CI checks (native GUI feature omitted on Linux).
- Run reviewed-checkout native CUA; preserve screenshots/AX/input outcome evidence. No live account login/model calls without separate explicit approval; label those unverified.
- Update operator usage/auth/settings/CLI/architecture/runtime and testing docs; replace historical Pi operator proposal status with a link, preserving history.
- Whole-branch spec/quality review by GPT-6.1 Sol, fix blocking findings, fresh current-head checks. Commit intended files, push feature branch/create PR and attach it; do not merge.
- Update single work diary with revision, tests, real gaps, PR/evidence and remaining live-provider acceptance.
