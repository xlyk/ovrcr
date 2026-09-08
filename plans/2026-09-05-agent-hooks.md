# Agent Hooks Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Goal:** Let managed agent processes report explicit activity through a working child-to-server-to-dashboard hook path.

**Architecture:** Extend the existing synchronous Unix-socket protocol and ordered dispatcher. The server owns activity alongside session lifecycle, while a small CLI helper translates generic reports and one Claude Code adapter into the same typed request. Dashboard reconnect uses the existing hierarchy snapshot.

**Tech Stack:** Rust 2024, blocking Unix I/O and threads, existing serde/bincode/clap/Ratatui; reuse the existing `serde_json = "1"` dependency for provider JSON, without an agent SDK.

**Spec:** README checkbox “Agent hooks to report agent-specific activity and status”; original constraints in [MVP design](2026-09-04-ovrcr-mvp-design.md), especially Architecture, Session lifecycle, Local protocol, and Failure handling. This proposal replaces only that design's deferral of explicit agent activity.

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → session restore**. Multiple dashboards is deferred as of 2026-09-07 and excluded from this sequence until explicitly resumed. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Global constraints and source grounding

- macOS and Linux, one user, one dashboard, and the tested 50-session scale.
- “OVRCR does not use Tokio, a database, a shell-command builder, or an agent SDK.”
- “The server stores live sessions in memory.” No hook state survives server death.
- “OVRCR owns every session it manages.” No adoption of externally launched agents.
- Terminal output, elapsed silence, keyboard input, PID liveness, and CPU usage never imply busy or idle.
- Keep PTY drainage, process-group termination, final-output ordering, and removal authority unchanged.
- Planning baseline inspected: `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb`; no implementation or tests were run for this document.
- `src/session.rs`: `SessionSpec`, `SessionSummary`, `SessionState`, `spawn_internal`, `summary`, and `apply_event` own the relevant process and state boundaries.
- `src/server.rs`: `create_session_locked` holds registration through spawn; `run_dispatcher` orders events; `dispatch_session_event` publishes summaries; `snapshot_from_state` supplies reconnect state.
- `src/protocol.rs`: typed `Request`, `Response`, `DispatchMessage`, `SessionChanged`, and 1 MiB frame cap already exist.
- `src/main.rs`: `run`, `run_terminal`, `request_started`, `request_without_start`, `print_mutation`, and `print_legacy_response` provide CLI dispatch. Hook reports use their explicit inherited socket and never auto-start a server.
- `src/tui.rs`: activity currently exists only as `Dashboard::busy_sessions`; `session_is_busy` and animation scheduling must move to authoritative summaries.
- `tests/tui.rs::sidebar_animates_only_explicitly_busy_sessions` already guards explicit activity and exited-session behavior. `tests/server_lifecycle.rs` and `tests/cli.rs` provide real socket/PTY fixtures.

## Proposed behavior and boundaries

| Activity | Meaning | Fixed sidebar slot | Selected metadata |
| --- | --- | --- | --- |
| Unknown | No report, explicitly cleared, or reporting ended | `-` | `agent unknown` |
| Idle | Agent explicitly reports no current turn | blank | `agent idle` |
| Busy | Agent explicitly reports an active turn | existing braille spinner | `agent busy` |
| WaitingInput | Agent reports an input/permission wait | `?` | `agent waiting input` |
| Error | Agent reports a failed turn | `!` | `agent error` |

These are reported observations, not assertions that an agent has succeeded or finished its process. A silent agent remains at its last reported state; there is no invented timeout-to-idle rule. Exited sessions retain the existing dimmed lifecycle treatment, suppress the activity slot, and show `pid closed` rather than a live activity claim.

Default every newly managed session, including `local`, to Unknown. A report never changes `SessionPhase`, makes a session removable, terminates it, or grants permission to an agent. An explicit next report can clear Error or WaitingInput; ordinary PTY input cannot.

Scope includes the generic helper, one concrete provider adapter, installation instructions, authenticated routing, ordering rules, dashboard updates/reconnect, and cleanup tests. Non-goals: context counters, conversation restore, subagent aggregation, automatic provider detection, editing user settings, prompt/transcript storage, historical activity logs, webhooks, plugin frameworks, and a new notification service.

## Shared interfaces and ownership

Add `AgentActivity` to `src/session.rs`; add envelope types to `src/protocol.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActivity { Unknown, Idle, Busy, WaitingInput, Error }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentUpdate { Activity(AgentActivity) }

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReport {
    pub session: SessionId,
    pub capability: [u8; 32],
    pub sequence: Option<u64>,
    pub update: AgentUpdate,
}
// Request gains AgentReport(AgentReport); accepted reports return Response::Ok.
// Implement Debug for AgentReport with capability rendered as "[redacted]".
```

`SessionSummary` gains `pub activity: AgentActivity`. `SessionState` holds activity, `Option<[u8; 32]>` capability, and activity ordering state. Never include capability in summaries, registry TOML, errors, CLI output, or derived debug output.

The context-usage sibling may extend `AgentUpdate` with `Context(ContextUsageReport)` later. It must reuse this transport and helper, with a separate ordering state for context; an activity update cannot invalidate a context update. This plan does not create the context module or payload.

```rust
// src/session.rs: ordering is per update kind, in memory only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReportOrder { #[default] Unset, Receipt, Sequenced(u64) }
impl ReportOrder {
    pub fn accept(&mut self, incoming: Option<u64>) -> anyhow::Result<()>;
}
// src/report.rs: shared by generic and provider-specific CLI branches.
pub fn send_report(update: AgentUpdate, sequence: Option<u64>,
    deadline: std::time::Instant) -> anyhow::Result<()>;
pub fn claude_activity(input: &[u8]) -> anyhow::Result<Option<AgentActivity>>;
pub fn read_hook_stdin(deadline: std::time::Instant) -> anyhow::Result<Vec<u8>>;
// src/session.rs: called only by the dispatcher, locks SessionState once.
pub fn apply_agent_report(&self, report: &AgentReport) -> anyhow::Result<bool>;
```

The session method returns whether visible activity changed. It first validates capability, phase, payload, and ordering; failed validation changes nothing, including the ordering watermark. Auth/phase/order validation and applying activity occur under the same state lock.

Add `DispatchMessage::AgentReport { report: AgentReport, completion: SyncSender<Response> }`. The connection handler uses `try_send`; a full queue returns Conflict without acceptance. The dispatcher applies the report, publishes `SessionChanged` only for visible changes, then completes the response. Never hold registry/sessions/mutation locks while waiting for dispatcher completion. Accepted reports may apply after their requesting client disconnects.

### Identity, ordering, and cleanup

Generate 32 bytes from `/dev/urandom` per managed session before spawn; failure prevents that spawn. Add `SessionSpec::hook_env: Option<HookEnvironment>` with `HookEnvironment { socket: PathBuf, session: SessionId, capability: [u8; 32] }` and redacted Debug. Direct session tests use None.

`spawn_internal` removes inherited hook identity variables first. For Some, inject only into `CommandBuilder`: `OVRCR_HOOK_SOCKET` (absolute socket path), `OVRCR_SESSION_ID` (decimal), and `OVRCR_HOOK_TOKEN` (64 lowercase hex characters). Never change the daemon's process environment. Nested OVRCR sessions receive fresh identity. Registration continues to hold the sessions guard until insertion, so a fast startup hook waits for the correct record. Construct the hook socket from the server's bound socket path: resolve a relative path against the server working directory, canonicalize its existing parent, and join its file name. Parent symlinks resolve to that canonical directory; reject a symlink socket leaf and fail spawn if the bound socket cannot be resolved. Do not interpret the path relative to the child workspace or change ordinary CLI socket resolution. Test a relative socket path and a symlinked parent with child cwd different from server cwd.

The private socket directory plus session capability prevent accidental cross-session updates and old-lifetime ID reuse. This is not isolation against hostile same-user processes: existing control clients already have that user's authority, and descendants inherit the capability. Do not place the token in argv or installation JSON. A copied wrong token must be rejected for another session.

First accepted report selects Receipt (`sequence: None`) or Sequenced (`Some(n)`, n > 0). Receipt reports apply in dispatcher receive order. Sequenced reports must strictly increase; equal or lower values are rejected, and mixing modes is rejected. Sequence numbers are producer-owned for the whole PTY lifetime, without wall clocks or reset on an agent turn.

Receipt order cannot reconstruct causal order across concurrently delayed provider callbacks. The Claude adapter uses synchronous hooks and None because the documented input does not supply a sequence; it makes no exactly-once or causal-order claim. No helper retries, delivery queue, reservation RPC, or guessed timestamp sequence. Generic producers needing stale-report rejection supply their own counter.

Revoke the capability when kill, close, or shutdown-with-kill accepts a termination attempt, on final Exited application, and on session removal. Clear activity to Unknown on final Exited. Kill failure retains the process record and reports the existing failure; reporting remains revoked. Dashboard detach neither clears nor revokes activity. Reconnect acceptance is report → detach → reattach → inspect the fresh HierarchySnapshot for the authoritative activity, without requiring another report or SessionChanged event. Server restart starts with empty state and new random capabilities, even if numeric session IDs repeat.

## Files to change during authorized implementation

| File | Responsibility |
| --- | --- |
| `src/session.rs` | Activity, ordering, child-only environment, capability validation/revocation |
| `src/protocol.rs` | Report envelope/request and ordered dispatch message |
| `src/server.rs` | Spawn identity, ingestion, update delivery, kill revocation |
| `src/report.rs` (new), `src/lib.rs` | Bounded synchronous report client and Claude input adapter/export |
| `src/main.rs` | `report activity` and `report claude` command parsing/dispatch |
| `src/tui.rs` | Summary-based activity, symbols, selected text, animation scheduling |
| `Cargo.toml`, `Cargo.lock` (read only) | serde_json is already present; no dependency change is needed |
| `tests/server_lifecycle.rs`, `tests/cli.rs`, `tests/tui.rs` | Existing fixtures and meaningful end-to-end assertions |
| `README.md` | Generic use, adapter setup/removal, supported observations and limits |

Any existing `SessionSpec`/`SessionSummary` literal must gain the new fields where compilation identifies it; do not restructure surrounding tests or GUI code. All work above is future implementation scope, not work performed by this plan.

## Current integration paths and task checkpoints

Pause/resume precedes this plan in the recommended order. Accept authenticated reports while Running **or Paused**; reject Exited and any saved-only phase if restoration already exists. Paused presentation keeps `P` in the status slot and suppresses busy animation, while preserving reported activity for resume. Replace `busy_sessions` authority completely, including its test setters, rather than keeping two writable activity sources.

In Task 2, revoke reporting immediately before each accepted termination attempt through `kill_session`, `close_terminal`, and the per-session loop in production `request_shutdown`. Perform ordinary existence/stopping validation first. Revocation is an idempotent session-state operation; it is never undone after failed termination. Revoke again on final Exited/removal. Do not revoke every session when shutdown without `--kill` is merely refused. Extend the lifecycle test to hold a copied capability, call `CloseTerminal`, and prove the late report is refused and its PGID is absent. Also test a refused non-killing shutdown leaves a valid live report usable.

`new` and `terminal create` both reach `create_terminal`/`CreateSessionRequest`; automatic workspace shells reach the same server spawn path. All server-owned spawns receive fresh child-only hook identity. Direct `SessionSpec` test fixtures use `hook_env: None`. In the immediate-startup-report test, read the token only through the managed child or a test-local accessor; never add a production RPC exposing capabilities.

In Task 3, extend the existing `run` command dispatch and reuse current error rendering. Generic helper success remains silent, including when the global `--json` flag is supplied; the report subcommand is a documented stream-oriented exception to ordinary mutation output. Refusals use the existing structured error convention under `--json`. The Claude adapter must not print an ordinary mutation success envelope that a provider could interpret as hook content.

In Task 4, extend `src/main.rs::terminal_value` with `activity` using stable snake_case strings (`unknown`, `idle`, `busy`, `waiting_input`, `error`). Add `agent_hook_resource_inventory_reports_activity` in `tests/resource_cli.rs`: a managed child invokes the helper, then `terminal list --json` (including its existing project/workspace filters) exposes the accepted activity without capability fields. Add that file to implementation scope. Run its exact test (one executed), plus the existing resource CLI regression. Task 1 may introduce the summary field and Unknown constructor defaults before this observable wiring; do not leave the JSON projection silently stale at completion.

Provider documentation links describe the adapter baseline, not a permanent schema guarantee. Before executing the provider acceptance task, read the linked official pages and compare actual installed-provider input. Keep generic transport tests independent of provider installation/authentication and report an unavailable real-provider gate explicitly.

## Task 1: Define activity and report ordering

**Files:** `src/session.rs`, `src/protocol.rs`, `tests/tui.rs` fixture literals.
**Consumes:** Existing SessionId, SessionPhase, serde framing.
**Produces:** The types above, `SessionSummary.activity`, and `ReportOrder::accept`.

- [ ] Add two unit tests in `session.rs`: `agent_report_order_rejects_replay_and_mode_switch` and `agent_report_order_receipt_accepts_repeated_values`. Add protocol test `agent_report_round_trip_and_redaction`.

```rust
let mut order = ReportOrder::default();
assert!(order.accept(Some(2)).is_ok());
assert!(order.accept(Some(1)).is_err());
assert!(order.accept(Some(2)).is_err());
assert!(order.accept(None).is_err());
assert_eq!(order, ReportOrder::Sequenced(2));
assert!(order.accept(Some(3)).is_ok());
```

- [ ] Run `rtk proxy cargo test --lib agent_report_ -- --nocapture`; RED must fail on absent types/behavior, not zero matching tests.
- [ ] Implement match cases `(Unset, None) -> Receipt`, `(Unset, Some(n > 0)) -> Sequenced(n)`, `(Receipt, None) -> Receipt`, and `(Sequenced(old), Some(n > old)) -> Sequenced(n)`; every other case returns an error before mutation. Add round-trip and Debug redaction assertions with a real nonzero token.
- [ ] Initialize summary/state activity to Unknown; update required literals. GREEN: same command executes at least 3 tests, all passing. These counts are planned minimums, not recorded results.

## Task 2: Wire child identity and ordered server ingestion

**Files:** `src/session.rs`, `src/protocol.rs`, `src/server.rs`, `tests/server_lifecycle.rs`.
**Consumes:** Task 1 types and existing dispatcher/snapshot delivery.
**Produces:** `apply_agent_report`, report request handling, child-only HookEnvironment.

- [ ] Add `agent_hook_capability_and_exit_are_enforced`, `agent_hook_sequence_does_not_regress_state`, and `agent_hook_startup_registration_is_visible` to real server fixtures. Reuse the existing spawn-ready seam for an immediate child report; do not add a timing sleep.
- [ ] In the authorization test, create two real PTYs with separate tokens; send a report for B with A's token, then a correct B report. Assert B changes only for the correct report, A is untouched, and the resulting phase remains Running.

```rust
// Core logic inside Session::apply_agent_report, after locking SessionState:
if state.hook_capability.as_ref() != Some(&report.capability) {
    anyhow::bail!("agent report rejected");
}
if matches!(state.phase, SessionPhase::Exited { .. }) {
    anyhow::bail!("session is not accepting agent reports");
}
let AgentUpdate::Activity(activity) = &report.update;
state.activity_order.accept(report.sequence)?;
let changed = state.activity != *activity;
state.activity = *activity;
Ok(changed)
```

- [ ] Run `rtk proxy cargo test --test server_lifecycle agent_hook_ -- --nocapture`; expect RED for the absent request/path.
- [ ] Generate/inject identity, extend dispatcher, and publish `SessionChanged` after acceptance. Keep capability validation in the dispatcher rather than authenticating then queuing a pre-approved mutation. Clone the session Arc then release the sessions lock before taking its state lock.
- [ ] For invalid auth, mode mixing, replay, revoked reporting, and unknown session, return structured refusal without including identity secrets; full dispatch queue returns Conflict. Implement revocation before termination begins, even if termination later fails.
- [ ] GREEN: same filter executes at least 3 passing tests. Observe final PTY output, group disappearance, and Exited before testing rejected late reports/removal; use existing bounded cleanup and retain fixture on cleanup failure.

## Task 3: Deliver the generic helper and verified adapter

**Files:** `src/report.rs`, `src/lib.rs`, `src/main.rs`, `tests/cli.rs`, `README.md`; Cargo files remain unchanged.
**Consumes:** `Request::AgentReport`, `Response::Ok`, identity environment.
**Produces:** `send_report`, `claude_activity`, and these commands:

```sh
rtk proxy ovrcr report activity --state busy --sequence 1
rtk proxy ovrcr report activity --state waiting-input --sequence 2
rtk proxy ovrcr report activity --state idle --sequence 3
rtk proxy ovrcr report claude --stdin-json
```

Generic commands require inherited identity and exit nonzero on invalid arguments, rejection, unavailable server, or timeout. Accept exactly the five state spellings `unknown`, `idle`, `busy`, `waiting-input`, `error`. Read only the hook socket variable: do not resolve an unrelated default socket or auto-start a server. Use the existing frame encoder/decoder and verify response request_id 1.

Create one `Instant::now() + Duration::from_secs(1)` deadline at helper entry and pass it to both `read_hook_stdin` and `send_report`. Use that same deadline across stdin reading, connection, writes, and response reads; apply remaining-time socket timeouts and nonblocking connect/poll for bounded connection setup. `read_hook_stdin` uses `libc::poll` plus bounded reads until EOF, rejects more than 65,536 bytes, and checks remaining time after interruption. No worker left blocked on stdin after timeout. Do not echo provider JSON, prompts, or environment values in diagnostics.

- [ ] Add CLI tests `agent_hook_cli_requires_identity_without_starting_server`, `agent_hook_cli_reaches_managed_session`, and `agent_hook_cli_timeout_is_bounded`. Use a Unix listener that accepts but withholds acknowledgement for the last case, plus a child pipe deliberately held open for stdin timeout.
- [ ] Add adapter unit tests `agent_hook_claude_maps_supported_events`, `agent_hook_claude_ignores_nested_and_unknown_events`, and `agent_hook_claude_rejects_malformed_or_oversized_input`.

```rust
let input = br#"{"hook_event_name":"UserPromptSubmit","session_id":"root-1"}"#;
assert_eq!(claude_activity(input).unwrap(), Some(AgentActivity::Busy));
let nested = br#"{"hook_event_name":"Stop","session_id":"root-1","agent_id":"child-1"}"#;
assert_eq!(claude_activity(nested).unwrap(), None);
assert!(claude_activity(b"not json").is_err());
```

- [ ] RED: `rtk proxy cargo test --test cli agent_hook_ -- --nocapture` and `rtk proxy cargo test --lib agent_hook_claude_ -- --nocapture` fail on missing behavior. Implement clap ReportCommand variants and shared client before provider translation.

Verified on 2026-09-05: Claude command hooks receive stdin JSON; relevant event names and `agent_id` scope are documented in the [official hooks reference](https://code.claude.com/docs/en/hooks). Proposed mapping: SessionStart → Idle; UserPromptSubmit, PreToolUse, PostToolUse and PostToolUseFailure → Busy; PermissionRequest → WaitingInput; Stop → Idle; StopFailure → Error; SessionEnd → Unknown. Ignore payloads containing `agent_id`, unknown events, and Notification events. Tool failure stays Busy because a tool failure does not prove the whole turn failed. Stop is a last observation; another hook can continue execution.

The adapter accepts required string `hook_event_name` and nonempty string `session_id`; optional `agent_id` must be a string. Ignore extra fields. Use serde_json's typed deserialization; never open `transcript_path` or interpret notification messages. These mappings are OVRCR defaults, not an upstream state-machine guarantee.

Adapter CLI exits zero with empty stdout for unconfigured environment, ignored events, malformed provider input, or transport failure; optional `--verbose` emits a bounded error category to stderr for diagnosis, never exit 2. Generic CLI remains strict. This prevents reporting from blocking an agent or injecting model instructions.

- [ ] Document installation as a manual merge of these entries into a user's existing `~/.claude/settings.json`; retain all unrelated settings/handlers. Set the hook command to the installed absolute OVRCR path if it is not on the agent's PATH. Synchronous command settings and removal are described in the [official hook setup guide](https://code.claude.com/docs/en/hooks-guide).

```json
{
  "hooks": {
    "SessionStart": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "UserPromptSubmit": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "PreToolUse": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "PermissionRequest": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "PostToolUse": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "PostToolUseFailure": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "Stop": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "StopFailure": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}],
    "SessionEnd": [{"hooks":[{"type":"command","command":"rtk proxy ovrcr report claude --stdin-json","timeout":2}]}]
  }
}
```

Launch one root Claude process directly with `rtk proxy ovrcr new --project demo --workspace hooks --name agent -- claude`. Do not share one OVRCR PTY between multiple independent agent roots: inherited capability intentionally represents the PTY, not a provider conversation. Remove only these handlers to uninstall, then restart that agent session. No installer writes configuration.

- [ ] GREEN: both test filters above execute at least 3 tests each. Verify missing identity produces no socket/config files. Record the real adapter version during manual acceptance; successful synthetic JSON tests alone do not prove the installed provider fires hooks.

## Task 4: Render authoritative activity and prove reconnect

**Files:** `src/tui.rs`, `tests/tui.rs`, `tests/server_lifecycle.rs`, `README.md`.
**Consumes:** `SessionSummary.activity` through existing SessionChanged and hierarchy messages.
**Produces:** Visible state across detach/reconnect without a second activity cache.

- [ ] Replace `busy_sessions` and its tests with summary activity. `session_is_busy` returns true only for Running + Busy. Existing `redraw_interval` scans summary state; only Busy schedules animation frames. Keep existing dense three-line rows, spacing, selection, and context field.

```rust
// Rendering decision, using the existing spinner frame when Busy:
match (&session.phase, session.activity) {
    (SessionPhase::Exited { .. }, _) => ' ',
    (_, AgentActivity::Unknown) => '-',
    (_, AgentActivity::Idle) => ' ',
    (_, AgentActivity::WaitingInput) => '?',
    (_, AgentActivity::Error) => '!',
    (_, AgentActivity::Busy) => spinner_frame,
}
```

- [ ] Add `agent_hook_sidebar_states_are_literal` and `agent_hook_summary_updates_drive_animation` using Ratatui TestBackend. Assert exact slots/text and that PTY Output/Input cannot turn Unknown/Idle into Busy. Existing busy test must keep its exited-session assertion.
- [ ] Add `agent_hook_round_trip_survives_dashboard_reconnect` to the server fixture: run the real helper as a managed PTY child, receive Busy, detach, send WaitingInput from that child, reconnect, and assert the initial hierarchy contains WaitingInput. Select its terminal and verify hook stdout injected no protocol/control content.
- [ ] RED then GREEN: `rtk proxy cargo test --test tui agent_hook_ -- --nocapture` executes at least 2 tests; `rtk proxy cargo test --test server_lifecycle agent_hook_round_trip_survives_dashboard_reconnect -- --exact --nocapture` executes exactly 1. Synchronize steps with request acknowledgements and pipe/PTY markers, not sleeps.
- [ ] Update README's “defaults to idle” claim to Unknown until reported, explain last-observation semantics and receipt-order limitations, and document the glyphs plus selected text.

## Acceptance and execution closeout

- [ ] Inspect the diff against the files above; run `rtk proxy cargo fmt --check`, `rtk proxy cargo test --lib`, `rtk proxy cargo test --test cli --test server_lifecycle --test tui`, and `rtk proxy cargo check --all-features`. Require nonzero counts in each test target; do not claim a filtered zero-test success.
- [ ] Run `rtk proxy cargo test --test terminal_acceptance -- --nocapture` once to retain the actual dashboard/PTY lifecycle gate, including cleanup. Broaden checks only for failures or changed behavior.
- [ ] In a disposable registered workspace, launch the installed Claude adapter and observe start, submitted turn, permission wait where available, continuation, and stop. Detach/reconnect during an active turn. Record supported events actually observed and the CLI version; do not provoke billing/API failures just to manufacture Error.
- [ ] Verify an uninstrumented shell stays Unknown while producing output. Wrong-session, revoked, stale sequenced, malformed, and oversized reports must leave activity and lifecycle unchanged.
- [ ] Kill/remove the fixture sessions and verify process-group disappearance and server shutdown using existing bounded fixtures. Hook clients must not create a replacement daemon after shutdown.

Risks: providers can omit callbacks, external settings can suppress them, and unsequenced overlapping callbacks can arrive late. Therefore the UI exposes last reported activity, without health claims or implicit retries. Root-only Claude mapping does not aggregate parallel subagents or independently launched children. Receiving a report never approves the provider's requested operation.

Dependencies: no other roadmap feature is required. Context usage must extend the shared report enum/client and add its own ordering state; it must not copy the transport or reset the activity watermark. If pause/resume lands, a Paused session may still accept an already-running hook helper's report while its capability is valid. Pause does not revoke reporting or erase activity; its `P` status takes precedence over the activity slot, and only Running + Busy animates. Keep the Exited-only report phase refusal above when adding Paused. Revisit adapter fields against official docs at implementation time if their schema changes.

Self-review: all five explicit activity states reach the summary/UI; the helper-to-managed-child path is covered; startup, detach, exit, kill, invalid identity, stale sequences, payload bounds, and failed reporting have defined outcomes. The original process ownership/lifecycle rules remain authoritative. No implementation, test execution, commits, README edits, provider configuration changes, or diary work were performed while writing this plan.
