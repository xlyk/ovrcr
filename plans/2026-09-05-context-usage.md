# Context Usage Accounting Implementation Plan

> **For the assigned worker:** Implement this plan task by task using the Luna execution contract below. Configure the worker as `gpt-5.6-luna` with `xhigh` reasoning effort. This document is a plan; execution starts only when the orchestrator assigns it.

**Goal:** Show the latest explicitly reported context occupancy for each agent session in the CLI and existing TUI context field.

**Architecture:** Extend the agent-hooks report envelope and existing in-memory session summary. Normalize one complete usage snapshot per report, publish through `SessionChanged`, and use the same formatting rules in the reporting helper and dashboard. Keep all I/O synchronous and use the existing socket and queue machinery.

**Tech Stack:** Rust 2024, serde, bincode, clap, Ratatui, stdlib synchronization; reuse the existing `serde_json = "1"` dependency.

**Spec:** `plans/2026-09-04-ovrcr-mvp-design.md` supplies the architecture constraints; README's unchecked “Context usage accounting for agent sessions” supplies this extension's goal. This document proposes its detailed behavior rather than treating the checkbox as an approved specification.

## Luna execution contract

- The orchestrator assigns **one plan** to a `gpt-5.6-luna` worker at **xhigh** effort, with the checkout path, actual base SHA, this file, and the preceding worker's interface/test handoff. The model setting belongs to the agent launch configuration; mentioning it in a prompt alone does not set it.
- Integrate plans serially in this order: **pause/resume → agent hooks → context usage → historical scrollback → copy mode → split panes → mouse forwarding → session restore**. Multiple dashboards is deferred as of 2026-09-07 and excluded from this sequence until explicitly resumed. This is an integration order, not a product priority. Context requires hooks; historical copying requires scrollback. The other ordering choices avoid simultaneous edits to shared session, protocol, server, and TUI files.
- Read this file, repository instructions, and the named source symbols first. Resolve symbols with `rtk proxy rg -n`; line numbers and code sketches are not a substitute for the landed implementation. If a preceding plan is already implemented, preserve its behavior and use its actual interfaces. Resolve a conflicting contract before coding that dependent task; do not build a second transport or state owner.
- Work through one numbered task at a time. Add its focused failing behavioral check, implement the smallest change, then require that check to execute and pass. When adding enum variants or fields, update all constructors and exhaustive matches in the same compiling step, including CLI JSON and optional GUI fixtures. Run `rtk proxy cargo check --all-targets --all-features` after that step.
- Fixture helpers are private to their integration-test binary. Add cases in the named existing file; a new test file needs its own explicitly defined fixture. Confirm a test filter with `-- --list` when uncertain; zero executed tests never satisfy a gate. Runtime/GUI acceptance commands below are future checks, not evidence already obtained.
- Use the defaults specified here when assigned to implement. Keep the roadmap checkbox unchecked until the required acceptance gates pass. Return the implemented task range, actual base/head, changed interfaces, exact checks with executed counts, cleanup evidence, and any unverified gate. Do not start a sibling plan as an incidental fix.

## Constraints and source grounding

- macOS and Linux; one owning server, one dashboard, up to 50 live sessions.
- Blocking I/O and threads; no Tokio, database, agent SDK, or additional daemon.
- Live metadata remains memory-only and disappears with the owning server.
- Preserve existing PTY ownership, output drainage, process-group exit, and removal semantics.
- Preserve three-line sidebar rows, literal tree indentation, selection behavior, and terminal layout.
- All execution commands below begin with `rtk`; commands are planned checks, not results from this planning session.
- Inspected HEAD: `9ca7a2d8c49c9743c7ba419fce3e3ae9e302cadb`; re-read affected files after the hooks dependency lands.
- README currently promises unknown context and explicitly says labels/output do not establish agent activity.
- `src/session.rs` defines `SessionSummary`; `summary()` copies synchronized lifecycle state; `apply_event()` owns exit publication.
- `src/protocol.rs` already carries summaries in `HierarchySnapshot` and `ServerEvent::SessionChanged`, with a 1 MiB frame bound.
- `src/server.rs` serializes session mutations and routes lifecycle updates through the dispatcher; reuse hooks' ordered report application there.
- `src/tui.rs` replaces changed summaries; `tree_line_text()` currently formats the literal `ctx —`.
- `src/main.rs` prints a stable three-column session list; add a focused inspection command without changing that output.
- `tests/tui.rs` has `dashboard_fixture()`, `TestBackend` rendering, and exact sidebar assertions; `tests/cli.rs` has real Git/PTY fixtures and `CleanupGuard`.
- `Cargo.toml` already includes `serde_json = "1"`; this plan adds no dependency.

## Product contract

“Used tokens” means provider-reported occupancy of the latest context sample, with source-specific semantics documented below. It is neither a running sum of requests nor a billing total. It does not predict occupancy while a response is still being generated. Never derive it from PTY byte counts, elapsed time, labels, executable names, or guessed model windows.

Every report replaces all context fields atomically. Missing or JSON-null optional fields become unknown; they never retain a previous model's capacity. Zero usage is valid. Capacity must be positive when known. A smaller usage value is valid after compaction, reset, or another provider update. Conversation and model changes require no accumulated-total reconciliation.

The displayed percentage is integer floor of `used * 100 / capacity`, computed in `u128`. A reported usage above capacity remains recorded verbatim and displays `>100%`; do not clamp stored values or panic. Unknown usage or unknown capacity displays `—`, even if the other field is known. No token estimate is synthesized from a reported percentage alone.

Record the server's receipt timestamp. Proposed freshness policy: a report is stale at 300,000 ms without another accepted report, whenever the observation clock is earlier than receipt, or after process exit. This is an advisory receipt-age policy, not proof that a quiet conversation changed. Stale known percentages display `25%~`; stale unknown samples display `—~`. Never-reported sessions display `—` with no stale suffix.

The CLI returns raw optional counts, provenance, receipt time, and the calculated `stale` flag. Wall-clock adjustments can conservatively mark data stale early or extend apparent age; document this small implementation's limit. Do not add a polling thread solely for context expiration: the existing dashboard redraw clock evaluates it.

Plain shells begin unknown; an explicitly instrumented agent launched inside a shell may report for that managed PTY. Names such as `local` do not prohibit legitimate reports. One root conversation reports per PTY; do not aggregate nested agents. On removal drop the usage; on exit retain the final sample but mark it stale. Reattach retrieves the sample from the surviving server.

Non-goals: dollar costs, billing totals, tokenizer estimation, model-window catalogs, automatic compaction, historical graphs, usage persistence, cross-session aggregation, provider transcript tailing, and automatic edits to provider configuration.

## Shared dependency: agent-hooks report transport

**Execution gate:** `plans/2026-09-05-agent-hooks.md` must land and its transport/lifecycle acceptance checks must pass first. Coordinate any interface change in both plans before implementation; do not implement a competing transport here.

Agreed interface owned by that plan:

```rust
Request::AgentReport(AgentReport {
    session: SessionId,
    capability: [u8; 32],
    sequence: Option<u64>,
    update: AgentUpdate,
}) // successful application returns Response::Ok

pub fn send_report(update: AgentUpdate, sequence: Option<u64>, deadline: std::time::Instant) -> anyhow::Result<()>;
// src/report.rs; reads OVRCR_HOOK_SOCKET, OVRCR_SESSION_ID, OVRCR_HOOK_TOKEN.
```

This plan adds `AgentUpdate::Context(ContextUsageReport)` in `src/protocol.rs`. Capability is private to the current managed PTY/server incarnation and is never present in summaries, JSON inspection, diagnostics, or terminal output. The helper uses hooks' one-second bounded exchange and never starts a missing server.

Hooks supplies separate ordering state per update variant. Context chooses its own mode on its first accepted report: `Some(sequence)` requires positive, strictly increasing sequence values; `None` uses receipt order thereafter. Mixing modes is rejected without modifying the sample or watermark. Sequences span the entire PTY lifetime, including compaction and conversation changes; reset does not restart them. Malformed/rejected reports do not refresh age or consume an applied sequence.

The real adapter uses `None`: its provider has no documented monotonic context event sequence. Receipt order cannot resolve a canceled older invocation that sends late. Explicitly describe the value as the latest accepted report; never claim causal ordering from helper timestamps. Producers with source order use generic reports with `--sequence`.

## Exact files and interfaces

| File | Responsibility |
| --- | --- |
| Create `src/context.rs` | Context types, validation, percentage/staleness formatting, generic JSON and Claude normalization |
| Modify `src/lib.rs` | Export `context` |
| Modify `src/protocol.rs` | Extend hooks' `AgentUpdate` with typed context payload |
| Modify `src/session.rs` | Store snapshot under existing session state lock; copy into summaries |
| Modify `src/server.rs` | Apply context variant through hooks' dispatcher, order/exit checks, notification |
| Modify `src/report.rs` | Reuse hooks' bounded input/report flow for context helper entry points |
| Modify `src/main.rs` | Parse report commands and `session context ID`; JSON inspection |
| Modify `src/tui.rs` | Replace literal context text with shared formatted value |
| Modify `tests/cli.rs`, `tests/server_lifecycle.rs`, `tests/tui.rs` | Extend existing fixtures and add focused behavior tests |
| Modify `README.md` at implementation time | Explain reporting commands, sample semantics, staleness, manual provider setup |

Use these concrete types; derive `Clone, Debug, PartialEq, Eq, Serialize, Deserialize`:

```rust
#[serde(rename_all = "snake_case")]
pub enum ContextSource { Generic, ClaudeCodeStatusline }
#[serde(deny_unknown_fields)]
pub struct ContextUsageReport {
    pub source: ContextSource,
    pub model: Option<String>,
    pub conversation: Option<String>,
    pub used_tokens: Option<u64>,
    pub capacity_tokens: Option<u64>,
}
pub struct ContextUsageSnapshot {
    pub report: ContextUsageReport,
    pub received_unix_ms: u64,
}
// Add to SessionSummary and private SessionState:
pub context_usage: Option<ContextUsageSnapshot>;

pub const CONTEXT_STALE_AFTER_MS: u64 = 300_000;
pub fn validate_context(report: &ContextUsageReport) -> anyhow::Result<()>;
pub fn parse_context_json(bytes: &[u8]) -> anyhow::Result<ContextUsageReport>;
pub fn parse_claude_context(bytes: &[u8]) -> anyhow::Result<ContextUsageReport>;
pub fn context_is_stale(sample: &ContextUsageSnapshot, now_ms: u64, exited: bool) -> bool;
pub fn format_context(sample: Option<&ContextUsageSnapshot>, now_ms: u64, exited: bool) -> String;
```

The source enum is an adapter identifier, not cryptographic provider verification. Reject empty model/conversation strings, control characters, and strings exceeding 256 UTF-8 bytes. This bounds displayed provenance and avoids control-sequence injection. Accept omitted optional keys as unknown. Reject negative/fractional/out-of-range counts, zero capacity, extra generic keys, and invalid source names. Validate identically after socket decoding, not only in the CLI.

## Current integration paths and task checkpoints

Start only after the hooks handoff names the actual report envelope, dispatcher application function, capability revocation points, and helper deadline functions. In Task 2, replace the hooks-only irrefutable `let AgentUpdate::Activity(...)` with an exhaustive match. The landed contract is `DispatchMessage::AgentReport { report, completion: SyncSender<Response> }` calling `Session::apply_agent_report(&AgentReport) -> anyhow::Result<bool>`; its bool means visible summary changed. Equal counts with a newer receipt timestamp return true and publish SessionChanged. Keep separate `ReportOrder` fields for Activity and Context; clone/validate a candidate ordering state before committing it with the validated sample under the same session mutex. An invalid context payload must neither consume sequence nor refresh receipt time.

The resource CLI already depends on serde_json and manually projects session summaries through `src/main.rs::terminal_value`. In Task 4, add `context_usage` (snapshot or null) and `context_stale` (boolean or null when never reported) there as well as the dedicated inspection command. Capture observation time once per output operation and use the same freshness helper for JSON and TUI. Paused sessions retain their last sample; pause itself does not make it stale. Exited and later saved-only records cannot accept reports.

Implement `session context ID` through `request_without_start(Request::List)` and existing `RuntimeError`/printing conventions. This focused inspection command is explicitly always JSON, including without the global flag. Its successful default output remains the specified JSON object; global `--json` prints the same bare object, matching current resource inspection output. Missing server or ID returns the existing structured NotFound error under `--json`, nonzero status, and no server creation. Do not invent a success wrapper or call the removed `print_response` path.

Add `tests/resource_cli.rs` to the allowed test files. Extend Task 4 with `context_resource_inventory_matches_inspection`: obtain Unknown, accept a generic context report, then compare `terminal list --json` with `session context ID --json` by parsed fields. Verify replacement clears omitted capacity and that exit makes both views stale. Run that exact test and require one executed case; keep existing resource CLI behavior passing.

Task 3's three provider input counters are **all required to be known** for a known sum. If any is missing or null, usage is unknown; never silently substitute zero. Malformed present counters remain errors. For deterministic freshness tests supply `now_ms` to pure helpers, including one millisecond before/at the 300,000 ms threshold and a backward clock; do not wait five minutes. On the real adapter gate, record actual input provenance without saving prompts, transcripts, or capabilities.

## Task 1: Define snapshot semantics and pure checks

**Files:** `src/context.rs`, `src/lib.rs`.
**Consumes:** The current package already supplies `serde_json`; this pure logic uses no hooks runtime interface.
**Produces:** All types and functions above except the Claude parser, implemented in Task 3.

- [ ] Add one table-driven test `context_semantics` covering zero, unknown halves, exact percentage, over-capacity, `u64::MAX`, and stale boundaries. Representative executable assertions inside the test:

```rust
let mut sample = ContextUsageSnapshot {
    report: ContextUsageReport { source: ContextSource::Generic,
        model: None, conversation: None,
        used_tokens: Some(25), capacity_tokens: Some(100) },
    received_unix_ms: 1_000,
};
assert_eq!(format_context(Some(&sample), 1_000, false), "25%");
assert_eq!(format_context(Some(&sample), 301_000, false), "25%~");
assert_eq!(format_context(Some(&sample), 999, false), "25%~");
sample.report.used_tokens = Some(u64::MAX);
assert_eq!(format_context(Some(&sample), 1_000, false), ">100%");
sample.report.capacity_tokens = None;
assert_eq!(format_context(Some(&sample), 1_000, false), "—");
assert_eq!(format_context(None, 1_000, true), "—");
```

- [ ] Add `context_json_validation`: parse a minimal generic object, then reject zero capacity, negative/fractional counts, escaped control characters, overlong identifiers, malformed JSON, and unknown keys. Assert omitted usage remains `None`, while explicit `0` remains `Some(0)`.
- [ ] Run `rtk proxy cargo test --lib context_ -- --nocapture`. Expected RED: missing context implementation or failing assertions; after implementation exactly two new tests must pass, not zero matching tests.
- [ ] Implement validation with serde JSON and explicit bounded string/capacity checks; perform safe percentage arithmetic:

```rust
let percentage = match (sample.report.used_tokens, sample.report.capacity_tokens) {
    (Some(used), Some(capacity)) if capacity > 0 => {
        if used > capacity { ">100%".to_owned() }
        else { format!("{}%", u128::from(used) * 100 / u128::from(capacity)) }
    }
    _ => "—".to_owned(),
};
```

- [ ] Format the stale suffix using `context_is_stale`; use `checked_sub` for clock reversal, not unchecked subtraction. Run the same focused check and inspect its nonzero count.

## Task 2: Carry context through live session state

**Files:** `src/session.rs`, `src/protocol.rs`, `src/server.rs`, `tests/server_lifecycle.rs`, existing summary constructors in `tests/tui.rs`.
**Consumes:** Task 1 types and the landed hooks identity/order/dispatcher interfaces.
**Produces:** `AgentUpdate::Context`, `SessionSummary.context_usage`, atomic application, existing `SessionChanged` notifications.

- [ ] Initialize every new session and test summary with `context_usage: None`. Extend hooks' session-level report application so context validation, mode/watermark checks, state replacement, and resulting summary happen under the same state lock. Do not introduce another mutation path or call back into `summary()` while holding its lock.
- [ ] Add `context_report_replaces_snapshot_and_preserves_activity` to the hooks-enabled server fixture. Use its real managed PTY and report capability; report `(used=80, cap=100)`, then `(used=5, cap=200, model changed)`, then both counts unknown. Assert exact snapshots through `Request::List`, and unchanged activity after each context update.
- [ ] Add `context_report_rejects_old_and_invalid_samples`: apply sequenced 10, reject 9 and repeated 10, accept 11 with lower occupancy, reject 12 with zero capacity, then accept valid 12. Assert state/receipt time/watermark remain unchanged on each refusal. Reject receipt-order mixing and wrong capability; repeat for an exited PTY.
- [ ] Add `context_snapshot_survives_dashboard_reattach`: observe an actual `SessionChanged` event, detach, reattach, then assert the hierarchy retains counts, model, and original receipt time. Kill the managed process group, observe exit, and verify the retained sample is stale without allowing further reports.
- [ ] Run `rtk proxy cargo test --test server_lifecycle context_ -- --nocapture`. Expected RED before wiring; expected GREEN with exactly three new tests once wired.
- [ ] Extend the existing dispatcher report match after validation. The assignment is complete replacement, including nulls:

```rust
state.context_usage = Some(ContextUsageSnapshot {
    report,
    received_unix_ms: now_unix_ms,
});
```

- [ ] Publish the resulting summary through hooks' existing `SessionChanged` path after application; receipt-time changes count as visible changes even when counts are equal, so freshness reaches the dashboard. Use the same queue-full/disconnect policy as other metadata. Preserve exit ordering and prevent a late context report from reviving exited state. Run the same three tests.

## Task 3: Generic reporting and one real provider adapter

**Files:** `src/context.rs`, `src/report.rs`, `src/main.rs`, `tests/cli.rs`.
**Consumes:** `report::send_report(AgentUpdate, Option<u64>, Instant)` and context validation.
**Produces:** `ovrcr report context --stdin-json [--sequence N]` and `ovrcr report claude-context --stdin-json`.

- [ ] Extend hooks' clap report namespace with these commands; generic input is precisely `ContextUsageReport` JSON. Set `deadline = Instant::now() + Duration::from_secs(1)` once before reading stdin and pass it to both `report::read_hook_stdin(deadline: Instant) -> Result<Vec<u8>>` and `send_report`. Reuse the reader's 65,536-byte bound and deadline checks, including slow/incomplete stdin. Invalid input returns nonzero and never sends a report. The Claude command has no sequence flag.
- [ ] Implement the verified mapping: Claude Code's status line receives JSON on stdin. Read `model.id`, `session_id`, and `context_window.context_window_size`. Sum `current_usage.input_tokens`, `cache_creation_input_tokens`, and `cache_read_input_tokens` with checked addition; exclude output tokens. Missing/null `current_usage` clears usage, including after compaction. Ignore cumulative counters and percentage-only fallbacks. These fields and the input-only percentage formula are documented in [Claude Code status-line documentation](https://code.claude.com/docs/en/statusline#context-window-fields), checked 2026-09-05.
- [ ] Parse only a small typed projection, tolerating unrelated provider keys. Missing/null optional counts produce unknown usage; present malformed values fail. Missing model/conversation becomes unknown. Never inspect transcript paths, prompts, or tools. Representative source-shaped test payload:

```json
{"session_id":"fixture-conversation","model":{"id":"fixture-model"},
 "context_window":{"context_window_size":200000,"current_usage":{
 "input_tokens":8500,"output_tokens":1200,
 "cache_creation_input_tokens":5000,"cache_read_input_tokens":2000}}}
```

- [ ] Add `context_claude_projection` asserting `used_tokens == Some(15500)` and capacity 200000, then null/missing components, checked-add overflow, and model/conversation replacement. Synthetic fixtures prove parsing, not provider execution.
- [ ] Implement both helpers by parsing once and calling the shared transport once. Generic success is silent. Claude success prints `ctx ` followed by the shared context formatter to stdout; errors are terse on stderr and expose no payload or capability. Do not swallow transport failures.

```rust
let report = parse_claude_context(&input)?;
send_report(AgentUpdate::Context(report.clone()), None, deadline)?;
let sample = ContextUsageSnapshot { report, received_unix_ms: 0 };
println!("ctx {}", format_context(Some(&sample), 0, false));
```

- [ ] Add `context_helper_reports_from_managed_pty` in `tests/cli.rs`: start a real managed shell with hooks env, feed the generic helper JSON through its PTY input, read socket state until exact sample arrives, then run the Claude helper with the fixture payload and assert the replacement. Use `CleanupGuard` and bounded observable waits; no shell-output marker substitutes for checking server state.
- [ ] Add `context_helper_invalid_input_has_no_effect` and `context_helper_missing_server_does_not_start_one`: assert nonzero status, unchanged sample for malformed input, absent socket/config creation for missing server, and hooks' existing deadline under incomplete stdin.
- [ ] Run `rtk proxy cargo test --lib context_ -- --nocapture` (three new unit tests total) and `rtk proxy cargo test --test cli context_helper_ -- --nocapture` (three new integration tests). Observe RED before implementation and GREEN afterward.

## Task 4: CLI inspection and literal TUI integration

**Files:** `src/main.rs`, `src/tui.rs`, `tests/cli.rs`, `tests/tui.rs`.
**Consumes:** Updated summaries and `format_context`/`context_is_stale`.
**Produces:** `ovrcr session context ID` and real values in the existing sidebar field.

- [ ] Add `SessionCommand::Context { id: u64 }`. Use `request_without_start(Request::List)` and select the exact ID. Missing server/session returns nonzero with a concise error. Print one JSON object with `session`, `context_usage` (snapshot or null), and `stale` (null if never reported, otherwise boolean). No new server request or changes to `ovrcr list` are needed.
- [ ] Update the session metrics line in `tree_line_text()`:

```rust
format!("     └ run {}  ctx {}",
    format_elapsed_at(session.started_unix_ms, now_unix_ms),
    format_context(session.context_usage.as_ref(), now_unix_ms,
        matches!(session.phase, SessionPhase::Exited { .. })))
```

- [ ] Add `context_sidebar_updates_and_expires` using `dashboard_fixture()` and `ServerEvent::SessionChanged`. Render with `draw_dashboard_at` at receipt and exactly 300,000 ms later, checking the full metrics string and unchanged row positions/background selection. Add `context_sidebar_unknown_and_over_capacity`, including a narrow terminal to check clipping stays within the sidebar.
- [ ] Add `context_inspect_reports_unknown_and_sample` in `tests/cli.rs`, exercising unknown JSON first, a real accepted sample second, and stale retained context after exit. Add `context_inspect_missing_session_fails` and assert default `list` output still matches existing tests.
- [ ] Run `rtk proxy cargo test --test tui context_sidebar_ -- --nocapture` (two new tests) and `rtk proxy cargo test --test cli context_inspect_ -- --nocapture` (two new tests). Run RED before changing behavior, then GREEN. Keep existing `ctx —` fixtures unknown instead of weakening their assertions.

## Task 5: End-to-end acceptance and user documentation

**Files:** `README.md`; focused tests above only if acceptance exposes a defect.
**Consumes:** Tasks 1–4 and an installed supported Claude Code for manual acceptance.
**Produces:** Reproducible reporting instructions and evidence for the checkbox.

- [ ] Document a generic complete replacement object, `--sequence` lifetime, unknown semantics, staleness marker, and inspection JSON. Explain source-reported context versus cumulative billing counts, and the receipt-order limit of unsequenced adapters.
- [ ] Provide this optional status-line setting in README, to be applied manually in a disposable Claude configuration for acceptance. Preserve an existing status line rather than overwriting it automatically. The command setting is documented by [Claude Code](https://code.claude.com/docs/en/statusline#manually-configure-a-status-line):

```json
{"statusLine":{"type":"command","command":"rtk proxy ovrcr report claude-context --stdin-json"}}
```

- [ ] Run `rtk proxy cargo test --lib context_ -- --nocapture`, `rtk proxy cargo test --test server_lifecycle context_ -- --nocapture`, `rtk proxy cargo test --test cli context_ -- --nocapture`, and `rtk proxy cargo test --test tui context_sidebar_ -- --nocapture`. Expected new counts: 3, 3, 5, and 2 respectively. Any zero-test filter fails the gate; existing similarly named tests may increase totals.
- [ ] Run `rtk proxy cargo test --test tui --test cli --test terminal_acceptance`, then `rtk proxy cargo fmt --all -- --check` and `rtk git diff --check`. Existing applicable tests must pass; record actual counts without inventing a baseline.
- [ ] In a disposable OVRCR workspace, launch Claude Code with the optional adapter enabled. Record installed version and verify a provider-produced report reaches `rtk proxy ovrcr session context SESSION_ID`, the matching TUI percentage, and reattach. `SESSION_ID` is the actual ID returned by that launch, not a literal command argument.
- [ ] Exercise `/compact` and confirm unknown or reduced occupancy according to the actual next source payload. Change model and confirm capacity comes from the next report, never a name lookup. Stop the session and verify retained data is stale. Record an unavailable CLI/auth/configuration gate as blocked; passing synthetic parser fixtures do not prove real-provider acceptance.
- [ ] Mark the roadmap checkbox complete only after the generic vertical slice, focused tests, and real-provider acceptance succeed. Do not claim other provider support.

## Acceptance and dependency risks

- A session with no report remains `ctx —`, regardless of its label or terminal output.
- Valid generic JSON reaches authenticated in-memory state, inspection JSON, live TUI updates, and reattach; complete replacement clears old capacity/model data.
- Compaction can reduce occupancy; zero and unknown differ; percentages cannot overflow and over-capacity remains explicit.
- Rejected identity, sequence, input, or exited-session reports leave state and freshness unchanged. Activity and context cannot overwrite one another's order or value.
- Staleness is visible and documented as receipt age. Disconnection does not erase a sample; server death does.
- Hooks transport is a hard prerequisite. Its evolving source locations must be reconciled before applying this plan; the shared envelope here is the agreed plan-time contract.
- Provider schema can change. Recheck the primary source and capture only a sanitized versioned sample at implementation time; unsupported/missing data stays unknown.
- Cancellation and unsequenced receipt order do not establish provider causality. Nested agents sharing a root PTY are unsupported producers; do not silently combine their contexts.
- Protocol struct/enum changes require matching server/client versions under the existing compatibility policy. Restart an empty/disposable server for acceptance instead of killing live user sessions.

Planning self-review: the report-to-state-to-CLI/TUI path is covered, shared transport is explicitly gated, proposed defaults are distinguished from the MVP specification, and tests above describe future checks only. No implementation or test execution is part of this planning change.
