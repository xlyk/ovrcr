# Pi / Oh My Pi Reporting, Phase 5 Implementation Plan (ticket #96)

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Workers implement; a different reviewer checks each committed unit. Push, PR and merge require Kyle's authorization.

**Goal:** Oh My Pi reporting survives its in-place session transitions (new, resume, fork, same-file reload, tree navigation), distinguishes a plugin-resource refresh from a factory replacement and from a reporter reattachment, and recovers from source loss at the next trustworthy boundary or through an explicit `/ovrcr-reattach` command — with no native restart, no fabricated lifecycle, and no historical attention replay (#96).

**Architecture:** #96 adds **no wire change and no receiver change**. Oh My Pi's extension already normalizes an in-place `session_switch` onto a `session_start` frame on the same producer instance with a continuing sequence, so `admit()` answers `Accepted` and the existing `session_start` arm invalidates the open cycle, closes the request set against the binding that owned it, and rebinds with `expected_binding: Some(<current>)` — which is the ticket's "fresh reporting generation". What #96 builds is entirely on the extension side plus one string in doctor: the switch handler starts naming the conversation it left (`previous`, an id parsed from `previousSessionFile`, so `transition_mismatch` applies to Oh My Pi as it does to Pi); `session_tree` becomes `cycle_invalidated` exactly as in Pi; `omp.registerCommand("ovrcr-reattach", …)` gives Oh My Pi the explicit reattachment action the spec requires, which also makes a bounded-queue overflow recoverable and closes the #91 review's B4 regression (`OMP.recovery` flips `"pending #96"` → `"available"`); and `idOf` moves into the shared transport so both extensions use one definition. A plugin-resource refresh is distinguished by subscribing to nothing: Oh My Pi emits no event for it and never re-instantiates an extension factory, so no shutdown/start is fabricated by construction, and two tests pin that a later ticket cannot quietly add one.

**Tech Stack:** as Phases 1–4.

**Spec:** #88 (updated 2026-09-13), #96. Base: `feature/pi-omp-phase5` = merged main with Phase 4 (#95 + #91 + the `ce0c7ff` fix wave). Decision notes: `.superpowers/sdd/2026-09-14-pi-omp-phase5/next-96-decisions.md` (which records the 18.1.19 binary evidence, with byte offsets, behind every claim above).

## Global Constraints

All Phase 1–4 constraints apply. Restated because they bind every task here:

- One server owner, one active dashboard, fifty sessions. Synchronous runtime; no async runtime, no new service, no new crate, no new dependency.
- Supported readiness providers are exactly Codex, Pi, Oh My Pi. Claude, Grok and Hermes stay excluded from Ready observations, Unread, review and alerts.
- Helper input and the private envelope stay within `HOOK_INPUT_LIMIT` = 65,536 bytes. One helper in flight, at most 256 queued events, at most 1 MiB queued bytes, the 900 ms per-event deadline and the one absolute drain deadline unchanged.
- Payloads carry identifiers and discriminants only: schema, event, mode, owner_pid, instance, sequence, session_id, run, outcome, reason, request_id, namespace, kind, previous, idle, will_continue. Never prompts, responses, tool arguments or results, dialog or prompt titles, file paths, or credentials.
- Extension loading passes `-e <path>` only; `--no-extensions` is never passed on the interactive route, and `task_runner.rs` is untouched.
- Never modify the user's `~/.pi`, `~/.omp`, or any live provider configuration. Every fixture gets its own `OVRCR_CONFIG`, `OVRCR_SOCKET` and tempdir; the opt-in installed-harness tests get an isolated `HOME` / `PI_CODING_AGENT_DIR`.
- Build prefix on every cargo command: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0`, always scoped (`-p <crate>` / `--test <name>`); never `cargo test --workspace` inside a ticket. Node suites: `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs`.
- Every ticket ends with `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `git diff --check`. One commit per task; each message ends with a blank line then the implementing agent's `Co-Authored-By:` trailer, matching the rest of the branch. Commit only; never push.

#96 specifics:

- **No wire change. `PROTOCOL_VERSION` stays 10.** Every frame this ticket sends already has a receiver arm from #91 and #95. If an implementer believes a wire change is necessary, stop and say so before writing it — the plan asserts it is not.
- **No change to `src/report/extension.rs`, `src/report.rs`, `crates/ovrcr-protocol`, `crates/ovrcr-runtime` or `crates/ovrcr-tui`.** The only Rust production edit is `src/cli/managed.rs` (one `&'static str`). A diff that touches the receiver has either found a genuine gap — report it — or has drifted.
- **Recovery discipline is #91's, unchanged.** Permanent disablement stays reserved for identity/ordering ambiguity; a paused receiver keeps its lease and binding; recovery is one forced `Bind` per trustworthy boundary (`session_start`, `agent_start`, or the reattach command); `HookEvent::Poll` stays inert — no timer, no retry loop, no polling. Unread survives every pause and every transition. Nothing is inferred from silence, idleness, empty queues, process names, files or elapsed time.
- **A same-file reload keeps its generation.** Only a switch to a *different* conversation advances it. This is a decision, not an omission (see `next-96-decisions.md`), and Task 2 pins it.
- **The reattach handler may never call `ctx.reload()`, `ctx.switchSession()`, `ctx.newSession()` or `ctx.navigateTree()`.** Oh My Pi's command context offers all four; #88 forbids conversation reload as a reporting-reconnection substitute.
- **Pi's behaviour is byte-for-byte unchanged.** `idOf` moves file but not meaning; `tests/pi_reporting_extension.mjs` keeps its 17 cases and `server_lifecycle pi_` its 11 (10 + 1 ignored) with no assertion edited.
- **The deferred forget-on-empty gating (#95 minor 2 / #91 minor 7) stays deferred**, with the concrete argument recorded in `next-96-decisions.md`. `crates/ovrcr-tui/src/dashboard/unread.rs` is not touched.
- Worktree: `.worktrees/pi-omp-phase5` (branch `feature/pi-omp-phase5`); ticket worktree `.worktrees/ticket-96-omp-recovery` (branch `ticket/96-omp-recovery`) cut from it. #96 is the only ticket in this phase; #92 and #97 follow separately.

## Execution order

| Order | Task | Ticket | Depends on |
| --- | --- | --- | --- |
| 1 | Shared `idOf`; Oh My Pi extension gains `previous`, tree invalidation and `/ovrcr-reattach`; host grammar (`session_switch:<id>[:<reason>[:<prev>]]`, `plugin_refresh`); Node tests | #96 | — |
| 2 | Lifecycle tests: transitions/reload/refresh/tree, gap → pause → recover, reattach during an input wait | #96 | 1 |
| 3 | Doctor `recovery` available; docs, glossary, support record; gates | #96 | 2 |

---

## Ticket #96 — Oh My Pi session changes and reporter recovery

### Task 1: shared `idOf`; Oh My Pi `previous`, tree invalidation and the reattach command; host grammar; Node tests

**Files:** `src/ovrcr-reporting-transport.mjs` (export `idOf`), `src/pi-reporting-extension.mjs` (import it instead of defining it), `src/omp-reporting-extension.mjs`, `tests/fixtures/pi/pi_host.mjs`, `tests/omp_reporting_extension.mjs`, `tests/pi_reporting_extension.mjs` (re-run unchanged).

**Interfaces (produces):**

```js
// src/ovrcr-reporting-transport.mjs — one definition, two call sites.
/**
 * The conversation a transition left, as its identity alone. Both harnesses name a session
 * file `<timestamp>_<session id>.jsonl` while binding the bare session id, so this takes the
 * trailing id and nothing else — never the timestamp, never the directory, never anything
 * inside the file. Anchored, so a name no bounded id can be read out of yields null (no
 * expectation) instead of a truncation that would contradict a healthy binding.
 */
export function idOf(file) {
  return file?.match(/(?:^|[/_])([A-Za-z0-9][A-Za-z0-9.-]{0,63})\.jsonl$/)?.[1] ?? null;
}

// src/omp-reporting-extension.mjs
function announce(ctx, reason, previous)            // previous: string | null
omp.on("session_tree", handler)                     // → frame "cycle_invalidated" { idle: boolean }
omp.registerCommand("ovrcr-reattach", { description: string, handler: (args, ctx) => Promise<void> })
```

```js
// tests/fixtures/pi/pi_host.mjs — grammar additions
// session_switch[:<id>[:<reason>[:<previous id>]]]   previous defaults to the host's current session
// plugin_refresh                                     emits nothing and keeps the extension instance
```

- [ ] **Step 1: RED — Node tests.** Add to `tests/omp_reporting_extension.mjs` and run `node --test tests/omp_reporting_extension.mjs` (expect 5 failures: the registered-list and command assertions, `previous`, the tree frame, and both reattach cases).

```js
// (a) extend the existing "registers OMP's lifecycle and switch events" — an EXACT list, so a
// later ticket cannot add a fabricated plugin-refresh or before-switch subscription unnoticed.
test("registers OMP's lifecycle and switch events", async () => {
  managed();
  const host = await createHost(materialize().extension);
  assert.deepEqual(host.registered(), [
    "agent_end", "agent_start", "session_shutdown", "session_start", "session_switch",
    "session_tree", "tool_approval_requested", "tool_approval_resolved",
    "tool_execution_end", "tool_execution_start",
  ]);
  // A plugin-resource refresh emits no extension event in Oh My Pi and never re-instantiates
  // a factory; nothing here may pretend otherwise, and a cancellable pre-switch never binds.
  assert.equal(host.registered().includes("session_before_switch"), false);
  assert.equal(host.registered().includes("session_compact"), false);
  assert.deepEqual(host.commands(), ["ovrcr-reattach"]);
});

test("a switch names the conversation it left by id, never by its file path", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "session_start" });
  host.state.session = "sess-b";
  await host.emit({
    type: "session_switch", reason: "resume",
    previousSessionFile: "/private/x/2026-09-13T00-00-00-000Z_sess-a.jsonl",
  });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.session_id, f.reason, f.previous]), [
    // A startup announcement states no expectation about a previous conversation.
    ["session_start", "sess-a", "startup", null],
    ["session_start", "sess-b", "resume", "sess-a"],
  ]);
  const body = JSON.stringify(sent);
  assert.ok(!body.includes("/private/x"), "no directory leaves the extension");
  assert.ok(!body.includes("2026-09-13T00-00-00-000Z"), "no timestamp leaves the extension");
  assert.ok(!body.includes(".jsonl"));
});

test("a same-file reload is a switch that names the conversation it is already on", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({
    type: "session_switch", reason: "resume",
    previousSessionFile: "/private/x/2026-09-13T00-00-00-000Z_sess-a.jsonl",
  });
  assert.deepEqual(frames(record).map((f) => [f.event, f.session_id, f.previous]), [
    ["session_start", "sess-a", "sess-a"],
  ]);
});

test("tree navigation invalidates the response cycle and carries only idleness", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a", idle: false });
  await host.emit({ type: "agent_start" });
  await host.emit({
    type: "session_tree", newLeafId: "LEAF_SECRET", oldLeafId: "OLD_LEAF_SECRET",
    summaryEntry: { text: "SUMMARY_SECRET" }, fromExtension: undefined,
  });
  host.state.idle = true;
  await host.emit({ type: "agent_start" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.run, f.idle ?? null]), [
    ["agent_start", 1, null],
    ["cycle_invalidated", null, false],
    // The abandoned cycle is not resumed: the next start opens a new one.
    ["agent_start", 2, null],
  ]);
  const body = JSON.stringify(sent);
  for (const secret of ["LEAF_SECRET", "OLD_LEAF_SECRET", "SUMMARY_SECRET"]) {
    assert.ok(!body.includes(secret), secret);
  }
});

test("the reattach command re-announces this producer once and says so", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a", idle: false });
  await host.emit({ type: "agent_start" });
  await host.run("ovrcr-reattach");
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.reason ?? null, f.idle ?? null, f.run]), [
    ["agent_start", null, null, 1],
    ["session_start", "reattach", false, null],
    // The reattached producer abandons the cycle it had open; the next start is a new one.
    ["agent_start", null, null, 2],
  ]);
  assert.deepEqual(host.state.notices, [["OVRCR reporting reattached", "info"]]);
});

test("a print-mode reattach delivers nothing, spawns no helper, and says it is unavailable", async () => {
  managed();
  const { extension, record } = materialize();
  const child = await createHost(extension, { mode: "print", session: "child" });
  await child.run("ovrcr-reattach");
  assert.deepEqual(frames(record), []);
  assert.equal(existsSync(record), false, "no helper was spawned for a child session");
  assert.deepEqual(child.state.notices, [["OVRCR reporting is unavailable", "warning"]]);
});
```

Also update the existing `"an in-place session switch re-announces the conversation and closes any open cycle"` to use `previousSessionFile: "/private/x/2026-09-13T00-00-00-000Z_sess-a.jsonl"` and to carry `f.previous` in its tuple (`[..., "resume", "sess-a"]`); its `!includes("a.jsonl")` assertion becomes `!includes(".jsonl")`. Every other assertion in it is unchanged.

- [ ] **Step 2: GREEN — implementation.**

`src/ovrcr-reporting-transport.mjs`: add the exported `idOf` above `createReporter` with the doc comment given under Interfaces. No other change to this file.

`src/pi-reporting-extension.mjs`: `import { createReporter, idOf, outcomeOf } from "./ovrcr-reporting-transport.mjs";` and delete the local `const idOf = …` at `:29` (keeping the comment on the export). Nothing else moves.

`src/omp-reporting-extension.mjs`:

```js
import { createReporter, idOf, outcomeOf } from "./ovrcr-reporting-transport.mjs";
…
  const { producer, report, flush, reattach } = createReporter({
    binary: OVRCR_BINARY,
    helperArgs: ["report", "omp", "--stdin"],
  });
…
  // `previous` is the conversation this announcement says it left, as an id: an Oh My Pi
  // switch carries `previousSessionFile`, and the receiver pauses on a switch that names a
  // conversation OVRCR is not bound to. Startup carries none, which is no expectation.
  function announce(ctx, reason, previous) {
    producer.run = 0; // an announcement belongs to no cycle
    producer.open = false;
    producer.outcome = "none";
    return report("session_start", ctx, { reason, previous });
  }

  omp.on("session_start", (_event, ctx) => announce(ctx, "startup", null));
  omp.on("session_switch", (event, ctx) => announce(ctx, event.reason, idOf(event.previousSessionFile)));
```

and, beside the existing input handlers:

```js
  // Tree navigation moves the conversation to a node this producer never reported: the
  // response cycle it was in the middle of is gone, and the only activity that survives is
  // what Oh My Pi's own API answers. Historical responses are never replayed as new ones.
  // Compaction is deliberately not subscribed: it preserves the conversation binding.
  omp.on("session_tree", (_event, ctx) => {
    producer.open = false;
    producer.outcome = "none";
    return report("cycle_invalidated", ctx, { idle: Boolean(ctx.isIdle?.()) });
  });
```

and, after the `session_shutdown` handler:

```js
  // The explicit reattachment action: it runs inside the already-loaded extension, so it
  // never needs a fresh native launch to restore reporting for this same session — and it
  // never reloads the conversation, which #88 forbids as a reconnection substitute.
  omp.registerCommand("ovrcr-reattach", {
    description: "Reattach OVRCR reporting for this session",
    handler: async (_args, ctx) => {
      const ok = await reattach(ctx, { idle: Boolean(ctx.isIdle?.()) });
      ctx.ui?.notify?.(ok ? "OVRCR reporting reattached" : "OVRCR reporting is unavailable", ok ? "info" : "warning");
    },
  });
```

`tests/fixtures/pi/pi_host.mjs` — replace the `session_switch` branch and add `plugin_refresh`, and extend the grammar comment:

```js
      else if (command === "session_switch") {
        // Oh My Pi switches in place on the same extension instance and names the session
        // file it left; `<id>` equal to the current session is a same-file reload.
        const previous = c ?? host.state.session;
        if (a) host.state.session = a;
        await host.emit({
          type: "session_switch", reason: b ?? "resume",
          previousSessionFile: `/private/x/2026-09-13T00-00-00-000Z_${previous}.jsonl`,
        });
      } else if (command === "plugin_refresh") {
        // Oh My Pi's plugin-resource refresh reloads plugin roots, agents, skills, slash
        // commands and MCP servers and emits no extension event; it never re-instantiates an
        // extension factory. So this host deliberately does nothing, and the tests assert
        // that nothing is what the reporter observes.
      }
```

- [ ] **Step 3: verify.** `node --test tests/omp_reporting_extension.mjs` (expect 25) and `node --test tests/pi_reporting_extension.mjs` (expect 17, unchanged). Then `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib report::extension` (25, unchanged — the receiver is not touched).
- [ ] **Step 4: Commit** `feat(omp): previous-conversation identity, tree invalidation and an explicit reattach command`.

### Task 2: lifecycle tests

**Files:** `tests/server_lifecycle.rs` only.

**Interfaces (produces):**

```rust
fn omp_session_dropping(
    fixture: &ControlFixture,
    socket: &Path,
    name: &str,
    drop_sequence: u64,
) -> (ovrcr::session::SessionSummary, std::path::PathBuf) {
    harness_session_dropping(fixture, socket, name, "omp", "omp/18.1.19", drop_sequence)
}
```

Frame sequences the drop hook targets (the transport numbers every delivered frame, one per reported event; `agent_end` without a continuation flag delivers two: `agent_end` then `agent_settled`):

| # | Host line | Frame |
| --- | --- | --- |
| 1 | `session_start:sess-a` | `session_start` |
| 2 | `agent_start` | `agent_start` |
| 3 | `tool_execution_start:ask:q1` | `input_open question:q1` |
| 4 | `tool_approval_requested:c1` | `input_open approval:c1` — **dropped** when `drop_sequence = 4` |
| 5 | the next reported event | arrives with a hole → `pause("source_gap")` |

- [ ] **Test 1 — `omp_in_place_transitions_rebind_reload_and_refresh_without_a_new_producer`** (`omp_session_named`, modeled on `pi_replacement_binds_the_foreground_conversation_and_rejects_the_retired_producer`). Drive with `pi_callback` and assert against `fixture.session_summary(summary.id)` at each step:

  1. `session_start:sess-a` → generation 1 on `sess-a`, Idle/Observed, health Connected.
  2. `agent_start` → Busy; record the turn.
  3. `agent_end:ok` → ResponseReady/Observed, Unread `u1 = current.unread.clone().expect(..)`.
  4. `tool_approval_requested:c1` → effective activity WaitingInput, `input_requests == [approval:c1]` (assert `agent.input_requests[0].kind == InputKind::Approval`).
  5. `session_switch:sess-b:resume:sess-a` → `input_requests` empty, binding `("sess-b", 2)`, Idle, `sample.turn == None`, health Connected, `current.unread == u1`.
  6. `session_compact` → the whole `SessionSummary` byte-identical to step 5's (`assert_eq!(current, previous)`), including the generation: compaction preserves the binding.
  7. `session_switch:sess-a:resume:sess-b` → binding `("sess-a", 3)` — A→B→A is a fresh generation, never a reuse.
  8. `session_switch:sess-a:resume:sess-a` (**same-file reload**) → binding `("sess-a", 3)` **unchanged**, Idle, `turn == None`, health Connected, `current.unread == u1`. The comment names the decision: a reload does not change the conversation, so there is no other binding to check and no generation to spend.
  9. `plugin_refresh` → the whole `SessionSummary` byte-identical to step 8's. No shutdown, no start, no rebind is fabricated for a plugin-resource refresh.
  10. `idle:false`, then `agent_start` → Busy; then `session_tree` → activity Unknown, `turn == None`, `current.unread == u1` (a historical response is never replayed).
  11. `idle:true`, `session_tree` → activity Idle.
  12. `agent_start`, `agent_end:ok` → ResponseReady on generation 3 and `assert_ne!(current.unread, u1)` — a new Unread, proving the instance survived every in-place transition and the refresh. Capture `u2`.
  13. `session_switch:sess-c:resume:sess-zzz` → health `Unavailable` with reason `transition_mismatch`, binding still `("sess-a", 3)`, `input_requests` empty, `current.unread == u2`: the switch named a conversation this receiver is not bound to.
  14. `agent_start` → recovered: binding `("sess-c", 4)`, health Connected, Busy, `current.unread == u2`.
  15. `session_shutdown:quit` → health `Unavailable`, `unread.is_some()`.
  16. `exit` → `PI_NATIVE_EXIT=17`.

  **RED evidence to record.** These assertions exercise behaviour Task 1 introduces, so they fail at base. Run the test at base first and paste the failure (expect step 5's `previous` assertion or step 10's tree step to fail: at base `session_tree` is unsubscribed, so activity stays Busy). To show step 9 discriminates, mutate the host's `plugin_refresh` branch to `await host.emit({ type: "session_start" })`, confirm the byte-identical assertion fails, and revert.

- [ ] **Test 2 — `omp_source_gap_pauses_then_recovers_at_the_next_boundary`** (`omp_session_dropping(.., 4)`):

  `session_start:sess-a`; `agent_start`; `tool_approval_requested:c1` → WaitingInput with `[approval:c1]`, Unread absent; `tool_approval_resolved:c1:true` (**frame 4, dropped** — the snapshot is unchanged, the request still stands); `tool_approval_requested:c2` → the hole pauses: health `Unavailable` / reason `source_gap`, `input_requests` empty, binding still `("sess-a", 1)`, the native session still answering (`PI_CALLBACK` keeps advancing). `agent_end:ok` while paused → the whole `SessionSummary` byte-identical (a non-boundary event applies nothing). `agent_start` → recovered: generation 2, health Connected, Busy, `input_requests` empty. `agent_end:ok` → ResponseReady on generation 2 and a new Unread. `session_shutdown:quit`; `exit`.

  **RED evidence.** Mutate the fixture's drop sequence to `0` (nothing dropped) and confirm the pause assertion fails with `left: Connected / right: Unavailable`; revert.

- [ ] **Test 3 — `omp_reattach_recovers_a_paused_reporter_during_an_input_wait`** (`omp_session_dropping(.., 4)`):

  `session_start:sess-a`; `agent_start`; `tool_execution_start:ask:q1` → WaitingInput with `[question:q1]`; `tool_approval_requested:c1` (**frame 4, dropped**); `tool_approval_resolved:c1:true` → hole → paused on `source_gap` **with a question open at the server**: `input_requests` empty (the runtime's `Unavailable` arm cleared the standing set), health `Unavailable`.

  Doctor, through the real CLI with `--session`: `lifecycle.delivery == "paused_recoverable"`, `lifecycle.health == "Unavailable"`, `capabilities.recovery == "available"`, and a remediation line containing `/ovrcr-reattach` and `Oh My Pi` (coordinator's decision: Task 2 lands the one-line `managed.rs` edit and its unit test FIRST — Task 3's Steps 1–2 move here as Task 2's Step 0, so this commit is green on its own and Task 3 is documentation only).

  `run_command:ovrcr-reattach` → generation 2, health Connected, Idle (the host is idle), `input_requests` empty, Unread unchanged, and `PI_NOTICE=[info] OVRCR reporting reattached` on the terminal.

  Then the criterion verbatim — **an old request cannot clear a newer one after reattachment**: `tool_execution_start:ask:q2` → `[question:q2]`, WaitingInput, exactly one Input-needed alert (`wait_alert_titles`); then `tool_execution_end:ask:q1` (the *old* question's close, arriving after the reattach) → `input_requests` still `[question:q2]`, still WaitingInput, no new alert; then `tool_execution_end:ask:q2` → the set empties and the underlying activity is restored. `session_shutdown:quit`; `exit`.

  **RED evidence.** Delete the `run_command:ovrcr-reattach` step and confirm the generation-2 assertion fails (`left: 1 / right: 2`); restore it.

- [ ] **Step: verify.** `cargo test -p ovrcr --test server_lifecycle omp_` (expect 7 passed, 1 ignored: 4 existing + 3 new), `--test server_lifecycle pi_` (10 passed, 1 ignored, unchanged), `--test server_lifecycle codex_` (6 passed, 1 ignored, unchanged). Record counts.
- [ ] **Commit** `test(lifecycle): Oh My Pi switches, same-file reload, plugin refresh, tree, gap pause and reattach during an input wait; doctor recovery available`.

### Task 3: docs, glossary, support record, gates (the `managed.rs` recovery flip and its unit test — Steps 1–2 below — are executed at the start of Task 2 per the coordinator's decision; Task 3 commits documentation only)

**Files:** `src/cli/managed.rs`, `docs/agent-reporting.md`, `docs/agent-reporting-support.md`, `docs/dashboard.md`, `CONTEXT.md`, `tests/server_lifecycle.rs` (the Test-3 doctor assertions, if deferred from Task 2).

**Interfaces (produces):**

```rust
pub(super) const OMP: Managed = Managed {
    // …
    recovery: "available",   // was "pending #96"
};
```

- [ ] **Step 1: RED — `managed.rs` unit test.** In `a_pause_is_recoverable_only_where_the_provider_has_a_way_out_of_it`, replace the Oh My Pi overflow assertion and its comment:

```rust
        // An overflow disables the producer inside the extension, so no source boundary can
        // end it on its own: only a provider whose extension registers a reattach command
        // can recover from one. Both providers register `/ovrcr-reattach` (#91, #96).
        assert!(paused(&PI, &snapshot(Some("source_overflow"))));
        assert!(paused(&OMP, &snapshot(Some("source_overflow"))));
        // A reason with no recovery path, and a healthy reporter, are still not a pause.
        assert!(!paused(&PI, &snapshot(Some("collector_unavailable"))));
        assert!(!paused(&OMP, &snapshot(Some("collector_unavailable"))));
        assert!(!paused(&PI, &snapshot(None)));
```

`cargo test -p ovrcr --bin ovrcr managed` fails on the second line (`paused(&OMP, source_overflow)` is `false` while `OMP.recovery == "pending #96"`).

- [ ] **Step 2: GREEN.** `OMP.recovery = "available"`. `paused()` and the remediation branch need no other edit: the reattach sentence is already gated on `provider.recovery == "available"` (`managed.rs:169-177`) and now reads true for Oh My Pi.
- [ ] **Step 3: docs.**

  `docs/agent-reporting.md`, Oh My Pi section — replace the paragraph beginning "Oh My Pi switches conversations in place on one extension instance." and the two sentences after it (the "#96", "no reattach command", "lost reporter" ones, which Task 3 makes false) with a **Transitions and recovery** subsection that says, in this order: a `session_switch` (`new`, `resume`, `fork`) re-announces the conversation with `session_start` on the same extension instance and names the conversation it left by id, so a transition OVRCR never saw pauses with `transition_mismatch`; a switch to another conversation is a fresh reporting generation and A→B→A is a new one too, never a reuse; **a same-file reload keeps the generation** — the conversation did not change — but still abandons the cycle it interrupted and closes any open request; compaction changes nothing; **a plugin-resource refresh changes nothing** — it reloads plugin roots, agents, skills, slash commands and MCP servers but never re-instantiates an extension factory, so no shutdown or start is reported and no rebind happens; tree navigation abandons the response cycle (`cycle_invalidated`) and falls back to Idle-or-Unknown from Oh My Pi's own API; updating the extension's own code still needs a native restart, and **reporter reattachment is not a conversation reload**. Then the recovery paragraph: the four pause reasons, the pause contract (lease and binding kept, nothing applied, request set cleared, native session untouched, no timer), and recovery as one forced rebind at the next `session_start` / `agent_start` **or** `/ovrcr-reattach`, each a fresh generation that starts blank — including that a reattach during a response does not restore that response's cycle and the next prompt reports normally. Finally: a bounded-queue overflow now pauses recoverably for Oh My Pi too, ended by `/ovrcr-reattach`.

  `docs/agent-reporting.md`, Oh My Pi section (prose — there is no Oh My Pi setup table; `docs/pi-reporting-setup.md` is Pi-only and stays untouched): the opening paragraph's event list gains `session_tree`, and the new subsection names `/ovrcr-reattach` as the command a user types in Oh My Pi, with the same wording Pi's `/ovrcr-reattach` row uses (fresh generation, Unknown while not idle, the in-flight response is not restored).

  `docs/agent-reporting-support.md`, "Oh My Pi 18.1.19 evidence": add a **Transitions and recovery** paragraph carrying the binary evidence from `next-96-decisions.md` — the three `session_switch` emit sites and their `reason` values, `previousSessionFile`, the `<timestamp>_<sessionId>.jsonl` naming and Oh My Pi's own reverse parse, the cancellable `session_before_switch` the extension does not subscribe to, `reloadPlugins` emitting nothing and never rebuilding the extension runner, `session_tree`'s payload, and `registerCommand` / `createCommandContext` / `ctx.isIdle` / `ctx.ui.notify`. Then the new automated proof: the three lifecycle tests by name, and the honest gaps — no Oh My Pi old-producer lifecycle test (unreachable: Oh My Pi never replaces its factory in-process; the fence is pinned by `admit_fences_gaps_retired_producers_and_an_unobserved_replacement`), no Oh My Pi `source_overflow` lifecycle test (257 PTY round trips; the doctor consequence is pinned by `a_pause_is_recoverable_only_where_the_provider_has_a_way_out_of_it`), no Oh My Pi lost-bind-receipt test (the `InvocationLease::operation_status` path is shared and covered by the Codex lifecycle test). Native acceptance stays #97.

  `docs/dashboard.md`: one sentence that a paused Oh My Pi reporter forgets its open requests exactly as Pi's does, and a recovered one replays no Input-needed alert.

  `CONTEXT.md`: no new term. Extend **Producer** with "in Oh My Pi one producer spans every in-place session switch, so a switch is a new binding, not a new producer" — the one place the existing definition could be misread for this harness.

  Check for and remove every surviving "reporter recovery is #96", "pending #96" and "Oh My Pi has no reattach command" across `docs/`, `src/` and `CONTEXT.md` (`rg -n 'pending #96|recovery is #96|no reattach command'`).

- [ ] **Step 4: gates** (each prefixed `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0`, counts recorded in the ticket report): `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs`; `cargo test -p ovrcr-protocol`; `-p ovrcr-runtime`; `-p ovrcr-tui`; `-p ovrcr --test tui`; `-p ovrcr --lib`; `-p ovrcr --bin ovrcr`; `-p ovrcr --test pi_reporting`; `--test codex_reporting`; `--test claude_reporting`; `--test server_lifecycle pi_` / `omp_` / `codex_` / `desktop` / `ready_sound`; `--test agent_setup`; `--test resource_cli`; `--test cli`; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `git diff --check`. Shared-surface counts that must hold exactly: `codex_reporting` 4, `server_lifecycle codex_` 6 (+1 ignored), `claude_reporting` 15, `server_lifecycle pi_` 10 (+1 ignored), `pi_reporting` 8, Node pi 17. `ovrcr-tui` lib stays 114 (untouched).
- [ ] **Commit** `docs(omp): transitions, plugin refresh and reporter reattachment; doctor recovery available`.

---

## What #97 (native Oh My Pi gate) needs from this ticket

- The native run must exercise, against an installed 18.1.19: `/new`, `/resume`, `/fork`, tree navigation, an in-place `/reload` (**generation unchanged** — the one transition that does not advance it), a plugin `/marketplace`-driven refresh (**nothing changes**), and `/ovrcr-reattach` during a real approval dialog and a real ask dialog.
- The documented behaviours #97 must confirm rather than assume: a reattach abandons the in-flight response cycle and the next prompt reports normally; a reattach publishes Unknown while Oh My Pi is not idle; a transition that names a conversation OVRCR is not bound to pauses rather than rebinds.
- `source_overflow` on Oh My Pi is exercised end to end **only** in #97's mixed-provider fifty-session replay; #96 proves the doctor consequence and the mechanism, not the overflow itself.
- Old-producer rejection on Oh My Pi has no in-process case and so no native case: #97 should record that as a property of the harness (one extension runner per process), not look for evidence of it.

## Whole-branch gates (controller)

`.superpowers/sdd/2026-09-14-pi-omp-phase5/branch-gates.sh` after #96 merges into `feature/pi-omp-phase5`; whole-branch review; PR to main with Kyle's word.

---

## Self-review against #96's acceptance criteria

| # | Criterion (abridged) | Task / test |
| --- | --- | --- |
| 1 | new/resume/fork/tree and same-file reload preserve identity and expected-binding checks; cancelled transitions do not rebind; A→B→A and delayed prior-session ends cannot mutate the binding | Task 1 (`session_switch` → `previous`; `session_tree` → `cycle_invalidated`; `session_before_switch` deliberately unsubscribed, asserted by the exact `registered()` list). Task 2 Test 1 steps 5, 7, 8, 10, 11, 13. Delayed prior-session ends: Oh My Pi has one producer per process, so a delayed frame is a stale sequence — `admit` at `extension.rs:420-422`, pinned by `stale_and_replayed_sequences_are_ignored_before_any_mutation` and `admit_fences_gaps_retired_producers_and_an_unobserved_replacement`. |
| 2 | in-place transitions distinguished from factory replacement and reporter generation; no fabricated shutdown/start for plugin refresh; recovery never invokes conversation reload | Task 1 (`registered()` exact list; `commands()`; the reattach handler calls only `reattach()` and `ctx.ui.notify`). Task 2 Test 1 steps 6, 8, 9 (byte-identical summaries) and 12 (the same instance survives). Task 3 docs. |
| 3 | transient failure pauses without ending native work; the next trustworthy boundary permits one bounded recovery; an explicit reattachment action stays available while delivery is paused | Task 2 Test 2 (pause, native still answering, `agent_start` recovers once) and Test 3 (`/ovrcr-reattach` runs while paused). `HookEvent::Poll` inert, unchanged (`extension.rs:122`). |
| 4 | only the authorized owner admits a fresh generation; old callbacks, stale closures, delayed cleanup and stale recovery cannot displace a replacement; a missing old close does not block authorized reattachment | Unchanged shared mechanism: every bind carries `expected_binding` and authenticates through the lease (`report.rs:531-550`); `admit`'s `LostClose` arm admits the successor and pauses. Task 2 Test 3's old-close step. Receiver unit tests unchanged (25). |
| 5 | recovery uses public current-state APIs and observer-owned tracking; unsupported activity becomes Unknown/unavailable; idle, queue emptiness, timeouts and old messages never generate Ready | Task 1 (`idle: Boolean(ctx.isIdle?.())` is the only activity source for `cycle_invalidated` and for a reattach; nothing reads files, history or elapsed time). Task 2 Test 1 steps 10–11 (Unknown then Idle) and Test 3 (Idle after reattach, Ready only from a genuine later cycle). |
| 6 | earlier Unread remains reviewable; restored responses and active requests baseline without alert replay; later cycles and genuinely opened requests notify normally | Task 2 Test 1 (`u1` across steps 5–11, `u2` at 12–14), Test 2 (Unread across the pause, a new one after recovery), Test 3 (`wait_alert_titles`: one alert for `question:q2`, none for the restored state). |
| 7 | active approvals/questions retain identity and closure through recoverable failures or are explicitly invalidated; an old request cannot clear a newer one after reattachment | Task 2 Test 3 verbatim (open `q2` after the reattach, then deliver `q1`'s close, assert `[question:q2]` stands). Invalidation-to-empty is the existing runtime rule (`reporting.rs:144-146`), asserted at Test 2's and Test 3's pause. |
| 8 | bounded helper deadlines and queue limits; no continuous polling, service, unbounded retries, credential exposure or lease takeover; server death keeps explicit-new-invocation behaviour | No transport change (Task 1 touches only `idOf`); `Poll` inert; `reattach` is user-initiated and one frame. Re-reservation after `lost()` is deliberately not built (#91 decision, unchanged). |
| 9 | actual managed/native tests prove transitions, loss/recovery during an input request and a response cycle, old-producer rejection, lost receipts, detach/reattach, native output/signals/exit and owned cleanup; Dashboard evidence and docs distinguish reload, refresh and reattachment | Task 2's three managed tests (real CLI, PTY, private socket, receiver) plus the existing `omp_managed_launch_inserts_its_extension_and_removes_it` (cleanup) and `omp_ready_alerts_once_and_creates_unread` (Dashboard). Old-producer rejection, lost receipts and overflow are covered by the shared tests named in Task 3's support paragraph, and the three gaps are stated there rather than hidden. Native Dashboard evidence is #97. |
| 10 | current Pi recovery/input/Ready, Claude/Codex reporting/review/notifications and scheduled Pi regressions remain intact; compatible upgrades are not disabled by version string | Task 3's gate list, with the exact shared-surface counts that must hold. The version probe is untouched: `version_status` stays `unverified_compatible_until_proven_otherwise` for an unlisted release and never disables. |

## No-placeholders check

Every task names its files, its exact signatures, its RED test bodies with their assertions, its GREEN code, its verification commands with expected counts, and its commit message. No step says "add tests", "TBD", "etc.", or names a symbol that does not exist: `idOf`, `announce`, `reattach`, `flush`, `createReporter`, `harness_session_dropping`, `pi_callback`, `omp_session_named`, `wait_alert_titles`, `paused`, `OMP`, `PI`, `Managed.recovery`, `input_live`, `observe_request` and every test name cited are all present in the tree at base or defined by this plan. The two deliberate non-builds (the overflow lifecycle test, the old-producer lifecycle test) are named with their reasons in Task 3's documentation step, not left as gaps in the code.
