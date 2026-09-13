# Pi / Oh My Pi Reporting, Phase 3 Implementation Plan (ticket #90)

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Workers implement; a different reviewer checks each committed unit. Push, PR and merge require Kyle's authorization.

**Goal:** A visible Pi extension dialog publishes `WaitingInput` together with the active Input-request identity in the same session snapshot, the Dashboard shows it and can deliver one "Input needed" alert per logical request through the existing preferences, and closing the last request restores whatever activity was underneath (Busy, Ready, Idle or Error) without touching Unread or the review target.

**Architecture:** The Input request lives on `AgentSnapshot` (protocol), so it travels in the same `SessionChanged`/`Hierarchy` publication as the activity sample and is atomic with it by construction; the underlying activity sample is never overwritten by a wait, so "restore" is the absence of code, via one `AgentSnapshot::effective_activity()` used by the server summary and the Dashboard alike. The runtime gains a fourth observation arm with its own revision watermark. The Pi extension mints one request identity per outer prompt span (Pi 0.85.1 coalesces nested prompts into one span and awaits nothing, so a counter is a complete identity) and never forwards the prompt title. The receiver keeps one open-request slot (bound 1 is exact for Pi; Oh My Pi's approvals in #95 will need a set). The Dashboard adds a second alert lane keyed by request identity: `ready::input_live`, an `Unread` request map, and an `Alert` enum in the notification queue; the queue, host, toggles, cancellation, visibility suppression and reconnect baseline are unchanged. One wire bump to `PROTOCOL_VERSION` 9, and the wire snapshot fixture finally carries an `AgentSnapshot` so future snapshot changes are pinned.

**Tech Stack:** as Phases 1–2.

**Spec:** GitHub issue #88 ("Input requests and the OMP upstream dependency", "Ready, Unread, and attention delivery"); ticket #90. Base: `feature/pi-omp-phase2` at 31e10f1 (Phases 1–2 merged; PR #102 open for Phase 1).

## Global Constraints

All Phase 1–2 constraints apply. In addition:
- `PROTOCOL_VERSION` 8 → 9 exactly once, in Task 1. `AgentObservation::Input` is appended LAST. The `wire_snapshot` `summary()` fixture gains `agent: Some(..)` with an open Input request so the pinned rows cover `AgentSnapshot` from now on; every pre-existing row that does not embed a `SessionSummary` must stay byte-identical.
- No new settings key, keybinding, or preference system. "Input needed" reuses `desktop_notifications` and `ready_sound`, the host, the queue, visible-pane suppression and the reconnect baseline.
- An Input request never creates Unread, never clears Unread, never changes the Presented review target, and is never represented as a `ReadyObservation`. Answering a request is not a review.
- Payload contract gains two events, `input_open` and `input_close`, with two fields, `request_id` and `kind` (`select|confirm|input|editor|custom`). The prompt `title` never leaves the extension.
- Codex and Claude behavior unchanged; Claude's own `WaitingInput` activity samples still work (they carry no Input request and `effective_activity` returns the sample's state).
- Worktrees: integration `.worktrees/pi-omp-phase3` (branch `feature/pi-omp-phase3`, from 31e10f1); ticket `.worktrees/ticket-90-input-requests` (`ticket/90-input-requests`). Build prefix as before; scoped commands only; Node tests `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs`.

## Execution order

| Order | Task | Depends on |
| --- | --- | --- |
| 1 | Protocol: `InputKind`, `InputRequest`, `AgentObservation::Input`, snapshot fields, `effective_activity`, protocol 9, wire snapshot | — |
| 2 | Runtime: Input arm, loss clears the request, summary uses `effective_activity` | 1 |
| 3 | Pi extension + transport counter + host grammar + Node tests | — |
| 4 | Receiver: open-request slot, `input_open`/`input_close` arms, `publish_observation` | 1, 3 |
| 5 | Dashboard: `input_live`, request lane, `Alert` enum, rendering, hints; unit tests | 2 |
| 6 | Lifecycle test through PTY + shipped Dashboard, CLI JSON, docs, glossary, doctor capability, gates | 4, 5 |

One ticket, one implementer (Opus), tasks sequential.

---

### Task 1: protocol

**Files:** `crates/ovrcr-protocol/src/agent.rs` (types near `ReadyObservation` ~:129, `AgentObservation` ~:113, `AgentSnapshot` ~:162, `ProviderReport::validate` ~:294, `request_fixtures` ~:560), `crates/ovrcr-protocol/src/codec.rs:18`, `crates/ovrcr-protocol/src/wire.rs` (`summary()` ~:381, `EXPECTED` ~:767), plus every `AgentSnapshot` literal: `crates/ovrcr-runtime/src/session/reporting.rs` (Bind ~:384 and the test near :549), `crates/ovrcr-runtime/src/session/mod.rs:~407`, `crates/ovrcr-tui/src/dashboard/ready.rs:~63`, `crates/ovrcr-tui/src/dashboard/desktop.rs` `snapshot()` ~:479, `crates/ovrcr-tui/src/dashboard/tests.rs:~1357`, `tests/tui/sidebar.rs:~171`, `tests/agent_setup.rs:~279`.

**Interfaces (produces):**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKind { Select, Confirm, Input, Editor, Custom }

/// An open request for a human answer from a visible provider dialog (CONTEXT.md: Input
/// request). Identified within its binding by `id`; carries no content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputRequest { pub id: String, pub kind: InputKind }

impl InputRequest {
    pub fn validate(&self) -> Result<()> { validate_agent_id(&self.id) }
}

pub enum AgentObservation {
    Activity(ActivitySample),
    Metrics(Box<MetricsSample>),
    Health(HealthSample),
    /// `Some` opens or replaces the active Input request; `None` closes it. Appended last:
    /// bincode numbers variants by declaration order.
    Input(Option<InputRequest>),
}

pub struct AgentSnapshot {
    // existing fields …
    pub input_request: Option<InputRequest>,
    pub input_revision: u64,
}

impl AgentSnapshot {
    /// WaitingInput while an Input request is open; otherwise the underlying activity sample.
    /// The one rule the server summary and the Dashboard both use.
    pub fn effective_activity(&self) -> AgentActivity {
        if self.input_request.is_some() {
            AgentActivity::WaitingInput
        } else {
            self.activity.as_ref().map_or(AgentActivity::Unknown, |sample| sample.state)
        }
    }
}
```

`ProviderReport::validate`: `AgentObservation::Input(request) => request.as_ref().map_or(Ok(()), InputRequest::validate)`. `PROTOCOL_VERSION = 9`.

- [ ] **Step 1: Write the failing tests** (in `agent.rs` tests):

```rust
    #[test]
    fn effective_activity_is_waiting_input_only_while_a_request_is_open() {
        let mut snapshot = AgentSnapshot {
            binding: AgentBinding { provider: AgentProvider::Pi, invocation: "inv".into(), conversation: "conv".into(), generation: 1 },
            activity: Some(ActivitySample { state: AgentActivity::ResponseReady, quality: SampleQuality::Confirmed, turn: Some("t".into()) }),
            metrics: None,
            health: HealthSample { state: ReporterHealth::Connected, reason: None },
            activity_revision: 3,
            metrics_revision: 0,
            health_revision: 0,
            input_request: Some(InputRequest { id: "i:p1".into(), kind: InputKind::Select }),
            input_revision: 4,
        };
        assert_eq!(snapshot.effective_activity(), AgentActivity::WaitingInput);
        snapshot.input_request = None;
        assert_eq!(snapshot.effective_activity(), AgentActivity::ResponseReady, "closing restores the underlying sample");
        snapshot.activity = None;
        assert_eq!(snapshot.effective_activity(), AgentActivity::Unknown);
    }

    #[test]
    fn input_observation_validates_its_identity() {
        let binding = AgentBinding { provider: AgentProvider::Pi, invocation: "inv".into(), conversation: "conv".into(), generation: 1 };
        let ok = ProviderReport { binding: binding.clone(), revision: 1, observation: AgentObservation::Input(Some(InputRequest { id: "i:p1".into(), kind: InputKind::Editor })) };
        ok.validate().unwrap();
        let close = ProviderReport { binding: binding.clone(), revision: 2, observation: AgentObservation::Input(None) };
        close.validate().unwrap();
        let bad = ProviderReport { binding, revision: 3, observation: AgentObservation::Input(Some(InputRequest { id: "bad\nid".into(), kind: InputKind::Custom })) };
        assert!(bad.validate().is_err());
    }
```

- [ ] **Step 2: Run to verify they fail**: `cargo test -p ovrcr-protocol effective_activity` (prefix) — compile error.

- [ ] **Step 3: Implement** the types above; add the two snapshot fields to every literal listed in Files (defaults `None` / `0`); in `wire.rs` `summary()` set `agent: Some(AgentSnapshot { binding: unread().binding.clone(), activity: Some(ActivitySample { state: AgentActivity::Busy, quality: SampleQuality::Observed, turn: Some("turn".into()) }), metrics: None, health: HealthSample { state: ReporterHealth::Connected, reason: None }, activity_revision: 2, metrics_revision: 0, health_revision: 0, input_request: Some(InputRequest { id: "req".into(), kind: InputKind::Select }), input_revision: 3 })`; in `request_fixtures` add `AgentObservation::Input(Some(InputRequest { id: "req".into(), kind: InputKind::Confirm }))` and `AgentObservation::Input(None)` to the observation array (they become `AgentRequest::10` and `::11`); bump the version; regenerate `EXPECTED` (`cargo test -p ovrcr-protocol wire_snapshot -- --nocapture`) and confirm in the diff that only the three `SessionSummary`-bearing rows (`Response::CreatedSession`, `Response::Inventory`, `ServerEvent::SessionChanged`) and the two new `AgentRequest` rows changed.

- [ ] **Step 4: Run** `cargo test -p ovrcr-protocol`, then `cargo check --workspace --all-targets --all-features` (prefix) to find every literal the compiler still rejects.

- [ ] **Step 5: Commit**: `feat(protocol): Input request on the agent snapshot; effective activity; protocol 9`.

### Task 2: runtime

**Files:** `crates/ovrcr-runtime/src/session/reporting.rs` (`unavailable` ~:63, `apply` ~:88, Bind ~:384, tests), `crates/ovrcr-runtime/src/session/mod.rs` (`summary()` ~:500).

- [ ] **Step 1: Write the failing tests** in `reporting.rs` tests, using the existing fixtures that build a bound `ReportingState` (see `unread_requires_supported_provider_ready_with_identified_turn` for the reservation + Bind dance):

```rust
    #[test]
    fn input_request_opens_and_closes_atomically_without_touching_activity_or_unread() {
        // bind a Pi state, publish Busy (rev 1) then Ready/Confirmed (rev 2) → unread Some
        // apply Input(Some{id:"a:p1", Select}) rev 3 → snapshot.input_request Some, activity sample still ResponseReady, unread unchanged, effective_activity WaitingInput
        // apply Input(Some{id:"a:p1"}) rev 3 again → Err("stale input revision"), nothing changed
        // apply Input(None) rev 4 → input_request None, effective_activity ResponseReady, unread unchanged
        // apply Activity Busy rev 3 while a request is open (rev 5 opened first) → activity sample Busy, effective WaitingInput
    }
    #[test]
    fn reporter_loss_clears_the_open_input_request() {
        // open a request, then state.lost("supervisor_disconnected") → snapshot.health Unavailable AND snapshot.input_request None; unread retained
    }
```

Write them out fully with the crate's existing helpers; the comments above are the assertions to encode, not the code.

- [ ] **Step 2: Implement** — fourth arm in `apply`:

```rust
            AgentObservation::Input(request) => {
                if report.revision <= snapshot.input_revision {
                    bail!("stale input revision");
                }
                snapshot.input_request = request.clone();
                snapshot.input_revision = report.revision;
            }
```

In `unavailable()`, where the snapshot's health becomes `Unavailable`, also `snapshot.input_request = None;` with the comment "a reporter that is gone cannot vouch for a visible dialog; the request is unknown until a fresh authoritative event". In `Session::summary()` replace the `map_or(AgentActivity::Unknown, |a| a.state)` with `agent.effective_activity()`. Bind literal: `input_request: None, input_revision: 0`.

- [ ] **Step 3: Run** `cargo test -p ovrcr-runtime --lib` (prefix). **Commit**: `feat(runtime): apply Input requests atomically with activity; loss clears them`.

### Task 3: Pi extension, transport counter, host grammar, Node tests

**Files:** `src/ovrcr-reporting-transport.mjs` (producer gains `prompt: 0`), `src/pi-reporting-extension.mjs`, `tests/fixtures/pi/pi_host.mjs`, `tests/pi_reporting_extension.mjs`.

- [ ] **Step 1: Node tests** (append to the Pi suite; the `registered()` deep-equal grows by `ui_prompt_end`, `ui_prompt_start` — cite in the commit):

```js
test("an outer prompt span opens and closes one Input request; the title never leaves", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "ui_prompt_start", reason: "ui_prompt", kind: "select", title: "PROMPT_TITLE_SECRET" });
  await host.emit({ type: "ui_prompt_end", reason: "ui_prompt", kind: "select", title: "PROMPT_TITLE_SECRET" });
  await host.emit({ type: "ui_prompt_start", reason: "ui_prompt", kind: "editor" });
  await host.emit({ type: "ui_prompt_end", reason: "ui_prompt", kind: "editor" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.run, f.kind ?? null]), [
    ["agent_start", 1, null], ["input_open", 1, "select"], ["input_close", 1, null], ["input_open", 1, "editor"], ["input_close", 1, null],
  ]);
  assert.equal(sent[1].request_id, sent[2].request_id);
  assert.notEqual(sent[1].request_id, sent[3].request_id);
  assert.match(sent[1].request_id, /^[0-9a-f]{32}:p1$/);
  assert.ok(!readFileSync(record, "utf8").includes("PROMPT_TITLE_SECRET"));
  assert.ok(!("title" in sent[1]));
});

test("a prompt can open while no cycle is running and never touches the cycle", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "ui_prompt_start", reason: "ui_prompt", kind: "confirm" });
  await host.emit({ type: "ui_prompt_end", reason: "ui_prompt", kind: "confirm" });
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.run]), [["input_open", null], ["input_close", null], ["agent_start", 1]]);
});
```

- [ ] **Step 2: Implement** — transport `producer.prompt = 0`; extension:

```js
  // Pi coalesces nested or overlapping prompts into one outer span and does not await these
  // handlers; one counter per instance is therefore a complete identity. The prompt title is
  // never read.
  pi.on("ui_prompt_start", (event, ctx) => {
    producer.prompt += 1;
    return report("input_open", ctx, { request_id: `${producer.instance}:p${producer.prompt}`, kind: event.kind });
  });
  pi.on("ui_prompt_end", (_event, ctx) =>
    report("input_close", ctx, { request_id: `${producer.instance}:p${producer.prompt}` }));
```

Host grammar (`main()`): `ui_prompt_start:<kind>` → `await host.emit({ type: "ui_prompt_start", reason: "ui_prompt", kind: a, title: "PROMPT_TITLE_SECRET" })`; `ui_prompt_end:<kind>` likewise; update the grammar comment.

- [ ] **Step 3: Run** `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs` — Expected: 19 pass. **Commit**: `feat(pi): report outer prompt spans as Input requests; host grammar; Node tests`.

### Task 4: receiver

**Files:** `src/report/extension.rs` (`Receiver` fields ~:127, `handle` arms ~:209, `publish` ~:325; unit tests).

- [ ] **Step 1: Unit tests** (pattern of the existing ones, no lease → only non-publishing paths, plus assertions on `open_request`):

```rust
    #[test]
    fn input_close_for_an_unknown_or_stale_request_is_ignored() { /* producer admitted; open_request None → input_close p1 IGNORED; open_request Some(p1) → input_close p2 IGNORED, open_request unchanged */ }
    #[test]
    fn input_open_requires_a_known_kind_and_valid_identity() { /* kind "dialog" → IGNORED; request_id with a control char → IGNORED; open_request stays None */ }
    #[test]
    fn a_repeated_open_for_the_same_request_is_ignored_before_any_mutation() { /* open_request Some(p1); input_open p1 → IGNORED; revision 0 */ }
    #[test]
    fn session_start_and_shutdown_forget_the_open_request() { /* set open_request; session_shutdown reload → None; set again; session_start (no lease → disables) → None */ }
```

- [ ] **Step 2: Implement** — field `open_request: Option<String>` (cleared in `new`, `disable`, the `session_start` arm and the `session_shutdown` arm); helper

```rust
fn input_kind(value: Option<&str>) -> Option<InputKind> {
    Some(match value? {
        "select" => InputKind::Select, "confirm" => InputKind::Confirm, "input" => InputKind::Input,
        "editor" => InputKind::Editor, "custom" => InputKind::Custom, _ => return None,
    })
}
```

arms (before the `_ => return IGNORED` fallthrough):

```rust
            "input_open" => {
                let (Some(id), Some(kind)) = (
                    payload["request_id"].as_str().filter(|s| validate_agent_id(s).is_ok()),
                    input_kind(payload["kind"].as_str()),
                ) else { return IGNORED.to_vec(); };
                if self.open_request.as_deref() == Some(id) { return IGNORED.to_vec(); }
                // Pi coalesces nested prompts into one span; a different id while one is open
                // is a replacement, published as such. Bound: one slot (#95's approvals need a set).
                self.open_request = Some(id.to_owned());
                return self.publish_observation(
                    AgentObservation::Input(Some(InputRequest { id: id.to_owned(), kind })), deadline);
            }
            "input_close" => {
                let Some(id) = payload["request_id"].as_str() else { return IGNORED.to_vec(); };
                if self.open_request.as_deref() != Some(id) { return IGNORED.to_vec(); }
                self.open_request = None;
                return self.publish_observation(AgentObservation::Input(None), deadline);
            }
```

and `fn publish_observation(&mut self, observation: AgentObservation, deadline: Instant) -> Vec<u8>` holding the tail of today's `publish` (lease/binding checks, `revision += 1`, `ProviderReport`, `publish_observation`, `ACCEPTED`), with `publish()` reduced to building the `ActivitySample` and delegating.

- [ ] **Step 3: Run** `cargo test -p ovrcr --lib report::`, `cargo test -p ovrcr --test pi_reporting`, `cargo test -p ovrcr --test server_lifecycle pi_managed_extension`, `cargo test -p ovrcr --test server_lifecycle omp_managed_extension`, `cargo test -p ovrcr --test codex_reporting` (prefix) — Phase 2 counts unchanged. **Commit**: `feat(report): Input requests through the shared receiver: one open slot, exact closes`.

### Task 5: Dashboard

**Files:** `crates/ovrcr-tui/src/dashboard/ready.rs`, `unread.rs`, `desktop.rs` (types ~:27, `observe_desktop_session` ~:130, `notification_matches_session` ~:245, tests `snapshot()` ~:459 and the delivery tests), `render.rs` (`provider_activity` ~:1177), `hints.rs:423-424`, `settings.rs` (doc comments on the two keys), `tests/tui/unread.rs`.

**Interfaces (produces):**

```rust
// ready.rs
pub(super) fn activity(session: &SessionSummary) -> AgentActivity {
    session.agent.as_ref().map_or(session.activity, |agent| agent.effective_activity())
}
/// The open Input request of a running, connected, supported session: the one identity the
/// Dashboard may deliver as an Input needed alert.
pub(super) fn input_live(session: &SessionSummary) -> Option<&InputRequest> {
    if session.phase != SessionPhase::Running { return None; }
    let agent = session.agent.as_ref()?;
    if !agent.binding.provider.supports_readiness() || agent.health.state != ReporterHealth::Connected { return None; }
    agent.input_request.as_ref()
}

// unread.rs — second lane; `retain` prunes both maps
requests: HashMap<SessionId, (AgentBinding, InputRequest)>,
/// Records this session's open Input request. True when a request opened or was replaced.
pub(super) fn observe_request(&mut self, session: &SessionSummary) -> bool { … }

// desktop.rs
#[derive(Clone)]
enum Alert { Ready(ReadyObservation), Input { binding: AgentBinding, request: InputRequest } }
struct Notification { session: SessionId, alert: Alert, title: &'static str, body: String }
fn identity_body(session: &SessionSummary) -> String   // the existing format!, factored
```

`notification_matches_session` matches on the variant: `Ready` keeps today's rule; `Input { binding, request }` requires `input_live(session)` with the same `id` and `agent.binding == *binding`. `observe_desktop_session` gains, after the Ready block, the request block: `let new_request = self.unread.observe_request(session);` then enqueue `Alert::Input` with title `"OVRCR · input needed"` under the same guards (`!initial`, channels, `input_live`, not visible, capacity). Closing or replacing a request makes the pending `retain` drop its alert and cancels an in-flight one, exactly as Ready does today, because both go through `notification_matches_session`.

`render.rs` `provider_activity`: right after `let Some(agent)`, `if let Some(request) = &agent.input_request { return format!(" input needed · {}", match request.kind { InputKind::Select => "select", InputKind::Confirm => "confirm", InputKind::Input => "input", InputKind::Editor => "editor", InputKind::Custom => "custom" }); }` (the sidebar `?` glyph already follows `ready::activity`). `hints.rs`: "…new background agent responses or input requests…" for both toggles. `settings.rs` comments likewise.

- [ ] **Step 1: Tests** — in `desktop.rs` tests, extend `snapshot()` with an `input: Option<InputRequest>` parameter (or a sibling `snapshot_with_request`) and add, using `stub_host`: (a) an open request on a hidden session enqueues one delivery titled `OVRCR · input needed` with the identity body; (b) a second publication with the same id enqueues nothing; a publication with a new id enqueues one more; (c) closing the request (`input_request: None`) before the host runs removes the pending delivery, and cancels an in-flight one; (d) a request first seen on the initial hierarchy (reconnect baseline) enqueues nothing, and a later new id does; (e) a visible session enqueues nothing; (f) a Ready and an Input request on the same session produce two deliveries with distinct titles and neither suppresses the other. In `unread.rs` tests (or the same file): `observe_request` true on open, false on same id, true on replacement, false after close; `retain` prunes. In `tests/tui/unread.rs`: publish a summary with an open request and an unread → `R` still targets the same Presented unread; an open/close never changes `session.unread`.

- [ ] **Step 2: Implement**, run `cargo test -p ovrcr-tui --lib` and `cargo test -p ovrcr --test tui` (prefix); any test that referenced `notification.unread` moves to `notification.alert` (cite). **Commit**: `feat(tui): Input needed alerts as a second lane; effective activity; input needed status`.

### Task 6: lifecycle through PTY and the shipped Dashboard; CLI; docs; gates

**Files:** `tests/server_lifecycle.rs` (`wait_named_calls` gains a sibling that returns the titles), `src/cli/managed.rs` (`PI.input_requests: "available"`), `docs/dashboard.md`, `docs/agent-reporting.md`, `docs/pi-reporting-setup.md`, `CONTEXT.md`, `docs/agent-reporting-support.md`.

- [ ] **Step 1: Fixture** — add `fn wait_alert_titles(&mut self, expected: usize, session: SessionId, name: &str) -> Vec<String>`: same identity and leak assertions as `wait_named_calls` (plus `assert!(!call.contains("PROMPT_TITLE_SECRET"))`), accepting either title, and returning the titles in record order by re-reading the record after `wait_record`.

- [ ] **Step 2: Test** — model on `pi_ready_alerts_once_creates_unread_and_explicit_review_clears_only_presented` (same hidden-session setup):

```rust
#[test]
fn pi_input_request_shows_waiting_alerts_once_and_restores_the_underlying_activity() {
    // session_start:sess-a → Idle
    // agent_start → Busy
    // ui_prompt_start:select → summary.activity WaitingInput; agent.input_request.kind Select; agent.activity.state Busy (underlying); titles == ["OVRCR · input needed"]
    //   `terminal list --json` row: activity "WaitingInput", agent.input_request.kind "Select", unread null
    // ui_prompt_end:select → activity Busy, input_request None; titles unchanged (no close alert)
    // ui_prompt_start:select → a new logical request: titles == [input, input]
    // ui_prompt_end:select
    // agent_end:ok; agent_settled → ResponseReady, unread Some(u1); titles == [input, input, "OVRCR · response ready"]
    // ui_prompt_start:confirm → WaitingInput with ResponseReady underneath; unread still u1; titles += input
    // ui_prompt_end:confirm → ResponseReady restored; unread still u1
    // dashboard.detach(); DesktopAlertDashboard::start_for(...) again with the request re-opened before reattach → no replay: titles length unchanged after reattach; then a genuinely new request after reattach → one more
    // session_shutdown:quit → health Unavailable AND agent.input_request None; unread u1 retained
}
```

Write it out with `pi_callback` and explicit assertions per step; index arithmetic is yours to keep straight.

- [ ] **Step 3: Docs** — `CONTEXT.md` after **Presented**: `**Input request**: An open request for a human answer from a visible provider dialog, identified by binding and request identity. While one is open the session's effective activity is WaitingInput; closing the last one restores the underlying activity. _Avoid_: Unread, a Ready observation, a tool call, or an alert as the request.` `docs/dashboard.md`: glyph row, metadata line, the two notification sections (two alert kinds, same preferences, same suppression and baseline; answering is not a review). `docs/agent-reporting.md` Pi section: replace the "later tickets" clause. `docs/pi-reporting-setup.md`: two event rows, the doctor capability line, Boundaries rewritten (session switches and recovery remain #91). `docs/agent-reporting-support.md`: note the wire bump and that native dialog evidence is #92. `src/cli/managed.rs`: `PI.input_requests: "available"`.

- [ ] **Step 4: Gates** — (prefix) `node --test …` (19), `cargo test -p ovrcr-protocol`, `cargo test -p ovrcr-runtime --lib`, `cargo test -p ovrcr-tui --lib`, `cargo test -p ovrcr --test tui`, `cargo test -p ovrcr --lib`, `cargo test -p ovrcr --bin ovrcr`, `cargo test -p ovrcr --test pi_reporting`, `cargo test -p ovrcr --test codex_reporting`, `cargo test -p ovrcr --test claude_reporting`, `cargo test -p ovrcr --test server_lifecycle pi_`, `… omp_`, `… codex_`, `… desktop_notifications`, `… ready_sound`, `cargo test -p ovrcr --test agent_setup`, `cargo test -p ovrcr --test cli`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `git diff --check`. Record executed counts. **Commit**: `test(lifecycle): Pi input requests wait, alert once, restore, never review; docs and glossary`.

---

## Whole-branch gates (controller): `.superpowers/sdd/2026-09-13-pi-omp-phase3/branch-gates.sh`, then the whole-branch review. Push, PR (stacked), CI and merge wait for Kyle.
