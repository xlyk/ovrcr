# Pi / Oh My Pi Reporting, Phase 4 Implementation Plan (tickets #95, #91)

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Workers implement; a different reviewer checks each committed unit. Push, PR and merge require Kyle's authorization.

**Goal:** Oh My Pi approvals and questions become Input requests with Input needed alerts (#95), and Pi reporting survives session changes, factory replacement, source gaps and helper loss with a bounded recovery and an explicit reattach action (#91).

**Architecture:** #95 turns the single Input-request slot into a bounded ordered set published whole: `AgentObservation::Input(Vec<InputRequest>)`, `AgentSnapshot::input_requests`, `InputKind::Approval`, protocol 10. Ids carry their namespace (`approval:<toolCallId>`, `question:<toolCallId>`). The OMP extension reports approvals from `tool_approval_requested`/`tool_approval_resolved` and questions from the ask tool's own execution lifecycle (`tool_execution_start`/`tool_execution_end` with `toolName === "ask"`), both gated on the root session id; no upstream change (decision 2026-09-13). The Dashboard's second lane alerts once per new request id and matches by membership. #91 adds no wire change: the receiver gains a `paused` state (lease kept), a sequence-gap detector, a retired-producer fence, a forced same-conversation `Bind` as the fresh reporting generation, a `cycle_invalidated` event for tree navigation, a `previous` conversation check on `session_start`, and the extension gains an `ovrcr-reattach` slash command and one absolute shutdown drain deadline.

**Tech Stack:** as Phases 1–3.

**Spec:** #88 (updated 2026-09-13), #95, #91. Base: merged main d9afe8f (Phases 1–3). Decision notes: `.superpowers/sdd/2026-09-13-pi-omp-phase4/next-95-decisions.md`, `next-91-decisions.md`.

## Global Constraints

All Phase 1–3 constraints apply. In addition:
- #95 bumps `PROTOCOL_VERSION` 9 → 10 exactly once (Task 1); #91 makes no wire change. `InputKind::Approval` and `AgentObservation::Input(Vec<..>)` keep every existing variant index; only the wire rows that embed a `SessionSummary` may change, plus the two `AgentRequest` Input fixtures.
- Pi's behavior under the set model is byte-for-byte equivalent: one request at a time, published as `[req]` and `[]` at the same revisions. Every Phase 3 Pi test keeps its count with `input_request` read as `input_requests.first()`.
- Questions are tool lifetime, never dialog visibility: the documentation, doctor and support record say "opens with the ask tool, a moment before the dialog; includes a queued question". No capability probe, no version gating, no ask-tool-lifetime disclaimer hidden.
- Bounds: at most 32 open requests per binding (`MAX_INPUT_REQUESTS`), ids unique, validated at the trust boundary (`ProviderReport::validate`); overflow ignores the new open (never disables).
- Registering the approval handlers disables OMP's speculative read execution; the support record states it.
- #91: permanent disablement stays reserved for identity/ordering ambiguity; a paused receiver keeps its lease and binding; recovery is one forced `Bind` per trustworthy boundary (`session_start`, `agent_start`, or the reattach command); `HookEvent::Poll` stays inert (no timer, no retry loop). Unread survives; nothing is inferred from history, files, process names or elapsed time.
- Tickets run sequentially in this phase (both rework `src/report/extension.rs`): #95 in `.worktrees/ticket-95-omp-inputs` (`ticket/95-omp-inputs`) from `feature/pi-omp-phase4`; #91 in `.worktrees/ticket-91-pi-recovery` (`ticket/91-pi-recovery`) cut after #95 merges.
- Build prefix, scoped commands, `env_lock`, Node suites: as before (`node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs`).

## Execution order

| Order | Task | Ticket | Depends on |
| --- | --- | --- | --- |
| 1 | Protocol: set model, `Approval`, bound + uniqueness validation, protocol 10, wire snapshot, literals | #95 | — |
| 2 | Runtime: set assignment, loss clears the set; doctor field rename | #95 | 1 |
| 3 | Receiver: ordered set, namespace check, whole-set publication, retire paths | #95 | 1 |
| 4 | OMP extension: approvals + ask-tool questions, root-only; host grammar; Node tests | #95 | 3 |
| 5 | Dashboard: set lane (`input_live` slice, per-id dedup, membership matching), rendering; unit tests | #95 | 2 |
| 6 | OMP lifecycle + alert tests, doctor split (approvals/questions), docs, glossary, gates | #95 | 4, 5 |
| 7 | Lease `force` bind; receiver `paused`/`admit`/retire/`recover`/`cycle_invalidated`/`previous` | #91 | 6 |
| 8 | Transport `reattach` + one absolute shutdown drain; Pi extension (`previous`, `session_tree`, reattach command); host (`session_replace`, `session_tree`, `run_command`, `isIdle`, `ui.notify`, `registerCommand`, drop hook); Node tests | #91 | 7 |
| 9 | Lifecycle tests: replacement + A→B→A, tree, compaction no-op, gap → paused → recover, reattach while paused, lost close, drain deadline | #91 | 8 |
| 10 | Docs, glossary (Producer, Reporting generation), doctor recovery, gates | #91 | 9 |

---

## Ticket #95 — Oh My Pi approvals and questions as Input requests

### Task 1: protocol set model

**Files:** `crates/ovrcr-protocol/src/agent.rs` (`InputKind` ~:109, `InputRequest` ~:118, `AgentObservation` ~:133, `AgentSnapshot` ~:184, `effective_activity` ~:203, `validate` ~:328, `request_fixtures` ~:557), `codec.rs:18`, `wire.rs` (`summary()` ~:407, `EXPECTED`), and every literal: `crates/ovrcr-runtime/src/session/reporting.rs` (~:409, tests ~:576, ~:623), `crates/ovrcr-tui/src/dashboard/{ready.rs ~:99, desktop.rs ~:543, unread.rs ~:110, tests.rs ~:1400}`, `crates/ovrcr-protocol/src/agent.rs` tests ~:467, `tests/tui/{sidebar.rs ~:191, unread.rs ~:220}`, `tests/agent_setup.rs ~:295`, `tests/resource_cli.rs ~:725` (JSON: `"input_request":null` → `"input_requests":[]`).

**Interfaces (produces):**

```rust
pub enum InputKind { Select, Confirm, Input, Editor, Custom, Approval } // Approval appended LAST
pub const MAX_INPUT_REQUESTS: usize = 32;
pub enum AgentObservation { Activity(..), Metrics(..), Health(..),
    /// The complete set of open Input requests for the binding, oldest first; empty closes all.
    Input(Vec<InputRequest>) }                                            // index 3 kept
pub struct AgentSnapshot { .., pub input_requests: Vec<InputRequest>, pub input_revision: u64 }
impl AgentSnapshot { pub fn effective_activity(&self) -> AgentActivity }  // WaitingInput while non-empty
pub fn validate_input_requests(requests: &[InputRequest]) -> Result<()>   // ≤32, each id valid, ids unique
```

`PROTOCOL_VERSION = 10`. In `wire.rs` `summary()` the embedded snapshot carries `input_requests: vec![InputRequest { id: "approval:req".into(), kind: InputKind::Approval }]`; `request_fixtures` pushes `Input(vec![InputRequest{ id: "question:req", kind: Select }])` and `Input(Vec::new())`.

- [ ] **Step 1: Tests** — replace Phase 3's `effective_activity_is_waiting_input_only_while_a_request_is_open` with a set version (two requests: still waiting after removing one; empty restores the sample), and add `input_observation_rejects_overflow_and_duplicate_ids` (33 requests → Err; two equal ids → Err; 32 distinct → Ok; a bad id → Err).
- [ ] **Step 2: Implement**, bump the version, regenerate `EXPECTED`, and confirm only the three `SessionSummary` rows and `AgentRequest::10/11` changed. `cargo check --workspace --all-targets --all-features` finds the literals.
- [ ] **Step 3: Commit** `feat(protocol): Input requests as a bounded set; Approval kind; protocol 10`.

### Task 2: runtime and doctor field

**Files:** `crates/ovrcr-runtime/src/session/reporting.rs` (`unavailable` ~:74, Health arm ~:145, Input arm ~:152, tests), `src/cli/managed.rs` (`lifecycle.input_request` → `input_requests` array of kinds).

- [ ] Input arm: `snapshot.input_requests = requests.clone()`; loss paths: `.clear()`. Tests: the Phase 3 runtime tests adapted to sets plus one that a two-request set followed by a one-request set keeps WaitingInput and the third publication `[]` restores the sample. Doctor: `"input_requests": agent.input_requests.iter().map(|r| r.kind).collect::<Vec<_>>()`; keep `pi_doctor…` assertions valid.
- [ ] **Commit** `feat(runtime): apply the Input request set atomically; loss clears it`.

### Task 3: receiver set

**Files:** `src/report/extension.rs` (`open_request` ~:140, arms `input_open` ~:326, `input_close` ~:349, retire paths ~:227 and ~:314, `input_kind` ~:420; tests).

**Interfaces:**

```rust
struct Receiver { .., open_requests: Vec<String> }  // ordered, ≤ MAX_INPUT_REQUESTS
fn open_request(&mut self, id: &str, kind: InputKind, deadline: Instant) -> Vec<u8>   // insert-if-absent, publish set
fn close_request(&mut self, id: &str, deadline: Instant) -> Vec<u8>                   // remove-if-present, publish set
fn publish_requests(&mut self, deadline: Instant) -> Vec<u8>                          // AgentObservation::Input(current set)
fn input_kind(value: Option<&str>) -> Option<InputKind>                               // + "approval"
```

Rules: `input_open` payload must carry `request_id`, `kind`, and `namespace` ∈ {`prompt`,`approval`,`question`} equal to the id's prefix (`prompt:` for Pi — see Task 4 note; Pi's existing `<instance>:p<n>` ids keep working by treating a missing `namespace` as `prompt` and not enforcing a prefix for it); a repeated open of a known id → IGNORED before any mutation; the 33rd open → IGNORED; `input_close` of an unknown id → IGNORED; retire paths (`session_start`, non-quit `session_shutdown`) publish `[]` when the set was non-empty, before binding/retiring, exactly where they publish `Input(None)` today. Kinds map: `question:` ids → `Select`; `approval:` ids → `Approval`.

- [ ] Unit tests: order preserved (a, b, close a → `[b]`); duplicate open ignored; overflow ignored without disabling; namespace/prefix mismatch ignored; retire publishes empty set once (with a lease-less receiver, assert the state mutations and that a lease-less publish path returns UNAVAILABLE exactly as today). Run the Phase 3 Pi lifecycle tests unchanged in expectation (`input_request` → `input_requests.first()` in the assertions, cited).
- [ ] **Commit** `feat(report): bounded ordered Input request set with namespaced ids`.

### Task 4: OMP extension, host grammar, Node tests

**Files:** `src/omp-reporting-extension.mjs`, `tests/fixtures/pi/pi_host.mjs`, `tests/omp_reporting_extension.mjs`, `src/pi-reporting-extension.mjs` (add `namespace: "prompt"` to its two frames for symmetry).

Extension additions (inside the factory, after the lifecycle handlers):

```js
  // Root-only: task and advisor sessions run in this same process and their events carry
  // their own session id (or ""), never the interactive root's.
  const root = (event, ctx) =>
    Boolean(event?.sessionId) && event.sessionId === ctx.sessionManager.getSessionId();
  const request = (ns, id) => `${ns}:${id}`;

  omp.on("tool_approval_requested", (event, ctx) =>
    root(event, ctx)
      ? report("input_open", ctx, { namespace: "approval", request_id: request("approval", event.toolCallId), kind: "approval" })
      : Promise.resolve(false));
  omp.on("tool_approval_resolved", (event, ctx) =>
    root(event, ctx)
      ? report("input_close", ctx, { namespace: "approval", request_id: request("approval", event.toolCallId) })
      : Promise.resolve(false));
  // Questions are the ask tool's lifetime (decision 2026-09-13): the request opens with the
  // tool, a moment before the dialog, and includes a question queued behind another dialog.
  // Tool events carry no session id; the mode guard in report() keeps children silent.
  omp.on("tool_execution_start", (event, ctx) =>
    event?.toolName === "ask"
      ? report("input_open", ctx, { namespace: "question", request_id: request("question", event.toolCallId), kind: "select" })
      : Promise.resolve(false));
  omp.on("tool_execution_end", (event, ctx) =>
    event?.toolName === "ask"
      ? report("input_close", ctx, { namespace: "question", request_id: request("question", event.toolCallId) })
      : Promise.resolve(false));
```

(Confirm in the installed binary that `tool_execution_start`/`end` carry `toolCallId` and `toolName` — Phase 2's investigation recorded `{ type: "tool_execution_start", toolCallId, toolName, args, intent }` and `{ type: "tool_execution_end", toolCallId, toolName, result, isError }`. `args`/`result` are never read.)

Host grammar: `tool_approval_requested:<id>` (emit with `sessionId: state.session`, `toolName: "bash"`, `reason: "APPROVAL_REASON_SECRET"`, `approvalMode: "always-ask"`), `tool_approval_resolved:<id>:<true|false>` (with `reason: "denied by user"` when false), `foreign_approval:<id>` (sessionId `"other-session"`), `tool_execution_start:<toolName>:<id>` (with `args: { question: "QUESTION_TEXT_SECRET" }`), `tool_execution_end:<toolName>:<id>` (with `result: { text: "ANSWER_SECRET" }`).

- [ ] Node tests (`tests/omp_reporting_extension.mjs`): registered list gains the four events; approval open/close pair with `namespace`/`request_id` and no `APPROVAL_REASON_SECRET`; denial closes; foreign session id emits nothing; two approvals overlap (frames carry both ids); an ask start/end pair keyed by tool call id with no `QUESTION_TEXT_SECRET`/`ANSWER_SECRET`; a non-ask tool emits nothing; an approval opened during a question (four frames, order preserved); `mode: "print"` emits nothing.
- [ ] **Commit** `feat(omp): approvals and ask-tool questions as Input requests, root-only; host grammar; Node tests`.

### Task 5: Dashboard set lane

**Files:** `crates/ovrcr-tui/src/dashboard/ready.rs` (`input_live` → `&[InputRequest]`), `unread.rs` (`requests: HashMap<SessionId, (AgentBinding, HashSet<String>)>`, `observe_request -> Vec<InputRequest>` returning only newly seen ids for the current binding; binding change resets the set; `retain`), `desktop.rs` (loop over the returned requests; `Alert::Input` matching by membership `input_live(session).iter().any(|l| l.id == request.id)`), `render.rs` (`agent.input_requests.first()` labels the oldest; `Approval` → "approval").

- [ ] Unit tests: `observe_request` returns exactly the new ids across publications `[a]`, `[a,b]`, `[b]`, `[]`, `[b]` (b again after a close → new); Dashboard: two open requests → two deliveries with distinct ids; closing one drops only its pending alert; reconnect baseline with two open → nothing; `tests/tui/unread.rs` set variant.
- [ ] **Commit** `feat(tui): Input needed alerts once per request in a set; membership matching`.

### Task 6: OMP lifecycle + alert tests, doctor split, docs, gates

**Files:** `tests/server_lifecycle.rs` (`wait_alert_titles` leak assertions gain the three new secrets; new tests), `src/cli/managed.rs` (`Managed.input_requests` → `approvals: &'static str, questions: &'static str`; Pi: `"available"` / `"unavailable_no_provider_surface"`; OMP: `"available"` / `"available_tool_lifetime"`; doctor `capabilities` emits both), `docs/agent-reporting.md` (OMP section: approvals, ask-tool questions, the approximation, speculative-read side effect), `docs/agent-reporting-support.md` (correct the 18.1.19 statement: approvals ARE exported; 0 matches for a question export; tool lifetime chosen), `docs/dashboard.md` (Input requests: namespaces, several at once, `agent.input_requests` array, oldest labeled), `docs/pi-reporting-setup.md` (`input_requests`), `docs/cli-reference.md`, `CONTEXT.md` (Input request entry: "identified by binding, namespace and request identity").

- [ ] Lifecycle test `omp_input_requests_wait_until_the_last_closes_and_alert_once_each` modeled on `pi_input_request_shows…`: session_start; agent_start; `tool_approval_requested:c1` → WaitingInput, `input_requests == [approval:c1]`, one input alert; `tool_execution_start:ask:q1` → two requests, second alert; `tool_approval_resolved:c1:false` → still WaitingInput (`[question:q1]`), no new alert, activity sample still Busy; `tool_execution_end:ask:q1` → Busy restored; `tool_approval_requested:c1` again (same id after close) → new request, alert; resolve; `foreign_approval:x` → nothing; `agent_end:ok` → Ready + ready alert; `tool_approval_requested:c2` while Ready → WaitingInput over Ready, unread unchanged; resolve → Ready restored; `session_switch:sess-b:resume` with c3 open → `[]` published against sess-a, then sess-b bound at generation 2, Unread retained; `session_shutdown:quit` with a request open → health Unavailable and set cleared, Unread retained. CLI `terminal list --json` row asserts `agent.input_requests[0].kind == "Approval"`. Doctor with `--session` shows `lifecycle.input_requests == ["Approval","Select"]` and `capabilities.questions == "available_tool_lifetime"`.
- [ ] Gates (prefix): Node (both suites), protocol, runtime lib, tui lib, `--test tui`, `--lib`, `--bin ovrcr`, `pi_reporting`, `codex_reporting`, `claude_reporting`, `server_lifecycle pi_` / `omp_` / `codex_` / `desktop_notifications` / `ready_sound`, `agent_setup`, `cli`, `resource_cli`, fmt, clippy workspace, `git diff --check`; record counts.
- [ ] **Commit** `test(lifecycle): Oh My Pi approvals and questions wait, alert once each, restore, never review; docs`.

---

## Ticket #91 — Pi session changes and reporter recovery

### Task 7: lease `force` bind; receiver pause, admission, retire, recover, invalidate

**Files:** `src/report.rs` (`InvocationLease::bind` ~:513), `src/report/extension.rs`, `src/report/codex.rs` (its `bind` call passes `false`).

**Interfaces:**

```rust
// report.rs
pub(crate) fn bind(&mut self, provider: AgentProvider, conversation: &str, deadline: Instant, force: bool) -> Option<bool>
// extension.rs
struct Receiver { .., paused: Option<&'static str> }
enum Admission { Accepted, Ignored, Gap, LostClose }
fn admit(&mut self, event: &str, instance: &str, sequence: u64) -> Admission
fn retire(&mut self, instance: &str)                       // seen.insert(format!("p:{instance}")), capacity-accounted
fn pause(&mut self, reason: &'static str, deadline: Instant) -> Vec<u8>   // keeps the lease; one Health(Unavailable{reason}) via lease.command; clears current + open_requests (local only)
fn recover(&mut self, conversation: &str, deadline: Instant) -> bool      // one forced Bind (+ receipt recovery); Some(true) → paused = None, revision = 0
```

Rules:
- `admit`: retired instance → `Ignored`; same instance: `sequence <= last` → `Ignored`, `sequence > last + 1` → `Gap` (advance `last` first), else `Accepted`; new instance + `session_start` + no live producer → admit; new instance + `session_start` + live producer → `LostClose` (unobserved replacement: retire the live one, admit the new one, then pause); anything else → `Ignored`.
- `Gap`, `LostClose`, the overflow `unavailable` arm, and a `session_start` whose `previous` is `Some(p)` with `lease.binding.conversation != p` → `pause(reason)` with reasons `source_gap`, `producer_replaced`, `source_overflow`, `transition_mismatch`.
- While `paused`, every event is IGNORED except `session_start` and `agent_start`, which first call `recover(session)`; failure → `disable()`.
- `cycle_invalidated { idle: bool }` arm: `current = None`; publish `Idle` if `idle` else `Unknown`, quality Observed, turn None.
- `session_shutdown` non-quit path calls `retire(instance)` after publishing the empty request set.
- Health publication in `pause`: `lease.command(private_identifier, AgentCommand::Health(ProviderReport { binding, revision: self.revision + 1, observation: Health(HealthSample { state: Unavailable, reason: Some(reason) }) }))` expecting `HealthUpdated`; bump `self.revision` on success.

- [ ] Unit tests (lease-less where possible): gap detection pauses and stops applying (an `agent_start` after a gap does not mutate `current` until recovered); retired instance ignored even with higher sequence (A→B→A); `LostClose` retires the old producer and admits the new; `cycle_invalidated` clears the cycle; `previous` mismatch pauses; while paused a non-boundary event is IGNORED and leaves state untouched. Codex: `cargo test -p ovrcr --test codex_reporting` and `server_lifecycle codex_` unchanged (`force: false`).
- [ ] **Commit** `feat(report): paused receiver with bounded recovery; retired producers fenced; forced rebind as a fresh generation`.

### Task 8: transport reattach + absolute drain; Pi extension; host; Node tests

**Files:** `src/ovrcr-reporting-transport.mjs`, `src/pi-reporting-extension.mjs`, `tests/fixtures/pi/pi_host.mjs`, `tests/pi_reporting_extension.mjs`.

Transport additions:

```js
  // Test hook: the helper for exactly this sequence number is never spawned, simulating a
  // helper lost at its deadline. Inert unless the variable is set.
  const dropSequence = Number(process.env.OVRCR_TEST_DROP_SEQUENCE ?? 0);
  // in report(): after frame(): if (dropSequence && producer.sequence === dropSequence) return Promise.resolve(false);

  // One absolute deadline for a shutdown or reload drain: queued frames are discarded and
  // only the final frame is delivered, within whatever remains of `deadlineMs`.
  function flush(event, ctx, extra = {}) {
    queue.length = 0; queuedBytes = 0;
    const body = frame(event, ctx, extra);
    draining = draining.then(() => deliver(body));
    return draining;
  }
  // Re-announce this producer from inside the already-loaded extension: clears the disabled
  // flag and the queue, then delivers one session_start-shaped frame directly.
  function reattach(ctx, extra = {}) {
    producer.disabled = false; queue.length = 0; queuedBytes = 0;
    producer.run = 0; producer.open = false; producer.outcome = "none";
    const body = frame("session_start", ctx, { reason: "reattach", ...extra });
    draining = draining.then(() => deliver(body));
    return draining;
  }
  return { producer, report, flush, reattach };
```

Pi extension: `session_start` gains `previous: idOf(event.previousSessionFile)` with `const idOf = (file) => file?.match(/([0-9a-f-]{36})\.jsonl$/)?.[1] ?? null;` (an id, never a path); `pi.on("session_tree", (_e, ctx) => { producer.open = false; producer.outcome = "none"; return report("cycle_invalidated", ctx, { idle: Boolean(ctx.isIdle?.()) }); })`; `session_shutdown` uses `flush`; and

```js
  pi.registerCommand("ovrcr-reattach", {
    description: "Reattach OVRCR reporting for this session",
    handler: async (_args, ctx) => {
      const ok = await reattach(ctx, { idle: Boolean(ctx.isIdle?.()) });
      ctx.ui?.notify?.(ok ? "OVRCR reporting reattached" : "OVRCR reporting is unavailable", ok ? "info" : "warning");
    },
  });
```

OMP extension: `session_shutdown` uses `flush`; `announce` for `session_switch` passes `previous: null` (no expectation; #96 owns OMP transitions).

Host: `api.registerCommand(name, options)` recorded in a map; `ctx.isIdle()` from `state.idle` (default true); `ctx.ui.notify` recorded to `state.notices`; grammar `session_replace[:<id>[:<reason>]]` (emit `session_shutdown{reason}` on the current host, then `createHost` again from the same extension path, set the new session, emit `session_start{reason, previousSessionFile: "/private/x/<oldId>.jsonl"}`), `session_tree`, `session_compact` (emitted; the extension does not subscribe), `run_command:<name>`, `idle:<true|false>`. The fake executable path passes `OVRCR_TEST_DROP_SEQUENCE` through from the Rust test's environment.

- [ ] Node tests: `previous` carries the id only (no path); `session_tree` frames `cycle_invalidated` with `idle`; the reattach command re-enables a disabled producer and sends one `session_start` with reason `reattach`; a hung-helper shutdown with five queued frames completes within ~1.2 s (absolute drain) and delivers only the shutdown frame; `session_replace` yields a new instance id with `previous` = the old session id.
- [ ] **Commit** `feat(pi): previous-conversation identity, tree invalidation, reattach command; one absolute shutdown drain`.

### Task 9: lifecycle tests

**Files:** `tests/server_lifecycle.rs`.

- [ ] `pi_replacement_binds_the_foreground_conversation_and_rejects_the_retired_producer`: session_start:sess-a; agent_start; `session_replace:sess-b:resume` → generation 2 on `sess-b`, activity Idle, the old cycle gone, Unread retained if one existed; `session_replace:sess-a:resume` → generation 3; then drive a late frame from the first producer: not possible through the host (it is gone), so assert via the receiver unit test in Task 7 and here assert that a `session_replace` back to `sess-a` yields generation 3 (A→B→A produces a fresh generation, never a reuse). `session_compact` → binding unchanged, generation unchanged. `session_tree` after an agent_start → activity Unknown (host idle false) then Idle when `idle:true`; a subsequent cycle Readies normally.
- [ ] `pi_source_gap_pauses_then_recovers_at_the_next_boundary`: with `OVRCR_TEST_DROP_SEQUENCE` set to the sequence of an `input_close`: open a prompt, close it (dropped) → the next frame's sequence has a gap → receiver pauses: health Unavailable with reason `source_gap`, input set cleared, Unread retained, native still running; an `agent_end`/`agent_settled` while paused change nothing; `agent_start` → recovered: generation +1, health Connected, activity Busy, `input_requests` empty; a later Ready creates a new Unread normally.
- [ ] `pi_reattach_command_recovers_a_paused_reporter`: pause via a gap, then `run_command:ovrcr-reattach` → generation +1, Connected, activity Idle (host idle true), a notice recorded; then `exit`.
- [ ] Gates: `server_lifecycle pi_` and the Phase 3 Pi tests keep passing; `omp_` unchanged.
- [ ] **Commit** `test(lifecycle): Pi replacement, tree, compaction, gap pause and recovery, reattach`.

### Task 10: docs, glossary, doctor, gates

- [ ] `CONTEXT.md`: **Producer** ("The admitted extension instance whose source sequence fences its events; retired by an observed shutdown or replaced by an admitted successor. _Avoid_: the conversation, the binding, the native process") and **Reporting generation** ("The `generation` of a binding, bumped by every accepted Bind; a fresh one is admitted by the supervisor while the lease and session identity hold, and it starts with Unknown activity and no requests. _Avoid_: the reservation epoch, a restart"). `docs/agent-reporting.md` Pi section: transitions, pause reasons, recovery, the reattach command, what stays Unknown; `docs/pi-reporting-setup.md` Boundaries rewritten; `docs/agent-reporting-support.md`; `src/cli/managed.rs` `PI.recovery = "available"`, remediation text for `Unavailable` health distinguishes a paused reporter (reason `source_gap`/`producer_replaced`/`source_overflow`/`transition_mismatch`: "run /ovrcr-reattach in Pi or send the next prompt") from a lost one.
- [ ] Full gate list as Task 6; record counts. **Commit** `docs(pi): transitions and recovery; glossary Producer and Reporting generation; doctor recovery available`.

---

## Whole-branch gates (controller): `.superpowers/sdd/2026-09-13-pi-omp-phase4/branch-gates.sh` after each ticket merges; whole-branch review after #91; PR to main with Kyle's word.
