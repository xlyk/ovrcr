# Claude Code Integration Implementation Plan

> **For agentic workers:** Use the executing-plans skill to implement this plan task by task. This is a planning artifact, not an instruction to start implementation. Keep acceptance checkboxes open until their recorded gates pass.

**Goal:** Make Claude Code the first complete native interactive integration for accurate activity, current context, cumulative token usage and scoped cost reporting in OVRCR.

**Architecture:** Extend the existing report commands, authenticated Unix socket and runtime-owned state. Use native command hooks for observations and the status line for context/cost. A single supervised, exact-conversation collector supplies cumulative tokens when the certified Claude release has no supported cumulative export.

**Tech stack:** Existing Rust crates, serde/serde_json, synchronous Unix I/O and native Claude command hooks. No SDK, new crate, database, Tokio or telemetry service.

**Spec:** [Multi-harness design](2026-09-09-multi-harness-reporting-design.md). This plan implements its shared foundation and Claude scope only. [Superset findings](../research/superset-agent-reporting-2026-09-09.md) inform identity guards and replacement accounting. Other provider adapters remain in the [broader plan](2026-09-09-multi-harness-reporting.md).

Revised 2026-09-10 against the [six-finding review](../research/2026-09-10-claude-plan-review.md). The contracts below supersede conflicting shorthand in the broader implementation plan. Provider source certification remains a prerequisite, not a claimed result.

## Scope and baseline

Inspected 2026-09-09 at `188a6456e42607634aedc72d3a93697faebcb0e1`. Existing planning documents are untracked and must be preserved. Read the current checkout instructions, branch, base and status again before execution; use an isolated `codex/` worktree for code changes.

Already implemented:

- `src/report.rs`: inherited capability identity, 65,536-byte stdin limit, one-second deadline and socket transport. `claude_activity` filters child events but discards conversation identity and maps SessionStart/Stop directly to idle.
- `src/cli/report.rs`: fail-open Claude activity command; separate context command that prints `ctx N%` after a successful report and otherwise fails.
- `crates/ovrcr-protocol/src/context.rs`: checked context parsing, complete replacement and five-minute staleness.
- Runtime `session/mod.rs::apply_agent_report`: authentication, separate activity/context ordering and state mutation. It has no native invocation binding or cumulative accounting.
- `tests/cli.rs`, `tests/server_lifecycle.rs`: real reporter/socket/process fixtures. Extend their fixtures rather than copying their server harness into another binary.
- `docs/agent-reporting.md`: existing manual setup and legacy behavior. Modify this existing document; do not recreate it.

Required outcome: reliable identity; honest status quality; current context; complete or explicitly partial token totals; optional cost with scope/provenance; safe setup; actionable diagnostics; CLI/dashboard display; native Claude acceptance. Preserve all generic reporting commands and scheduled Pi behavior.

The dated [remaining-requirement matrix](../research/claude-reporting-acceptance/remaining-matrix.md) reconciles this checklist with the delivered implementation and evidence. Unchecked mixed clauses remain open even when the matrix marks their narrower implementation `Verified` or `Partial`.

Not included: implementing other harnesses, account quotas, pricing tables, historical analytics, automatic global configuration edits or restoration of dead PTYs.

## Contract decisions

1. **Root identity:** retain OVRCR session/capability and add native provider, invocation, conversation and runtime generation. Only the supervisor holding the runtime lease can bind a root; child hooks never receive that lease. Reserve requires an inactive slot and expected epoch; bind requires the expected current binding. A late report/unbind cannot replace a newer generation, including A→B→A reuse.
2. **One authority:** use the shared types in the design's API sketch. Activity and metrics revision watermarks are independent. Existing inspection fields become projections, not another mutable store.
3. **Causality:** shell callback arrival order is not native turn order. Use verified source IDs/revisions where present; an uncorrelated terminal callback cannot declare confirmed idle. Never invent a turn ID from callback completion time.
4. **Activity:** distinguish confirmed state from observed events. SessionStart attaches without resetting later progress. Stop is observed until settling is proved. Process exit, reporter health and activity are separate.
5. **Metrics:** current context can decrease after compaction; cumulative tokens are independent. Normalize Claude input as uncached input + cache creation + cache read. Cache counters remain subsets of canonical input. Optional values stay unknown when missing.
6. **Accounting:** use full replacement totals. For transcript records, identity is message ID plus request ID, and the latest valid usage record replaces the prior one for that identity. Equal usage from distinct requests counts twice.
7. **Scope:** report which conversation/invocation and usage categories totals cover. Root-only usage must not claim complete totals if native accounting includes children or auxiliary work. Cost has its own scope and reported/estimated label; use checked USD ticks at 10^10 ticks per dollar.
8. **Failure:** hooks fail open and stay stdout-silent. Preserve user status-line output even when reporting is unavailable. Keep the legacy `claude-context` command's documented behavior unless explicitly migrated with compatibility tests.
9. **Bounds:** retain existing hook limits. Collector queues hold at most 256 normalized records/1 MiB; decode at most one raw transcript record capped at 32 MiB. Exact accounting retains at most 65,536 identities/16 MiB of charged keys and values without eviction; exceeding either cap freezes an explicitly partial prefix. Oversized/unknown accounting records produce partial coverage and a diagnostic. No transcript text enters runtime state or logs.
10. **Truthful completion:** source research does not prove live behavior. Missing settled or full-accounting sources are explicit delivery dependencies, not permission to infer completion from silence.

## Task 1: Pin Claude's actual reporting contract

**Files:** Create `tests/fixtures/agent-reporting/claude/manifest.json`, sanitized JSON fixtures in that directory, and `docs/agent-reporting-support.md`.

**Output:** A versioned capability record with executable version, source date, fixture names, available identity fields, context semantics, token/cost scope, and source-versus-live evidence.

- [ ] Verify the installed executable version; the prior research anchor was Claude Code 2.1.267, not a certified minimum.
- [ ] Capture isolated interactive payloads for start/resume, prompt, permission wait, tool success/failure, Stop, a blocking Stop continuation, cancellation, API failure, compaction and session end. Capture child hooks and in-process new/resume/fork. Use disposable configuration and owned sessions; do not edit live settings.
- [ ] Refresh [official hooks](https://code.claude.com/docs/en/hooks) and [status-line documentation](https://code.claude.com/docs/en/statusline), then compare to actual payloads. Hook opportunities and status-line fields alone do not establish ordering or full-session accounting.
- [ ] Prove whether a final settled signal exists after all Stop decisions. A displayed assistant message is not sufficient proof. If absent, retain observed Stop and document the exact missing transition; confirmed settling requires a supported export or verified source before final acceptance.
- [ ] Certify foreground admission separately for initial start, foreground branch, background fork, clear, resume and compaction. Root SessionStart alone is insufficient. Background forks leave the foreground binding unchanged; same-conversation compaction preserves it. If evidence cannot prove an in-process foreground change, mark identity unavailable, stop accepting new metrics for that candidate, and require a new managed invocation. This restriction must appear in doctor/setup/support documentation; do not claim in-process switching support until certified.
- [ ] Verify exact transcript identity/path, stable usage-record keys, partial/final replacement, resumed/forked history, child/auxiliary attribution and cost scope. Prefer a supported cumulative export if present; otherwise certify the versioned reader route in Task 5.
- [ ] Sanitize content and credentials while preserving synthetic identity relationships. Record whether each fixture is source-derived or captured live.

Fixture manifest example:

```json
{"provider":"claude","fixture":"replaced-usage.jsonl","evidence":"source","identity":["message.id","requestId"],"rule":"last valid record for identity replaces earlier usage"}
```

**Gate:** Every required metric/state has a concrete source or a named blocking dependency. Do not guess undocumented fields to unblock implementation. Source capture does not authorize broad provider usage or changes outside disposable fixtures.

## Task 2: Add shared binding and report snapshots

**Files:** Create `crates/ovrcr-protocol/src/agent.rs`; modify protocol `wire.rs`, `session.rs`, `codec.rs`, `lib.rs`; runtime `session/mod.rs`, `session/tests.rs`, `server/dispatch.rs`, `server/connections.rs`, `server/tests.rs`; affected constructors/re-exports.

**Interfaces:** Implement the design's `AgentBinding`, `ProviderReport`, `ActivitySample`, `MetricsSample`, `UsageTotals` and associated enums. Implement the shared design’s `ReserveAgent`, conditional `BindAgent`, atomic `FinalizeAgent` and conditional `ReleaseAgent` protocol, including request-ID retry/status semantics and a distinct supervisor secret. Runtime epochs survive release for the PTY lifetime; hooks cannot reserve/bind/release through their report channel. `AgentUpdate::Provider` carries the report. Only Claude needs an adapter now.

- [x] Add failing dispatcher assertions for wrong capability/session, old generation, old unbind, A→B→A, delayed attachment and reports after exit. Assert rejection changes neither displayed values nor freshness.
- [x] Add races for delayed initial reservation, stale bind after replacement, duplicate request-ID retry and reservation-response loss. Require the stale request to fail without allocating a fresh generation; reserve may never replace an active lease.
- [x] Add codec validation/round trips for all new variants, redacted capability Debug, IDs over 256 bytes/control characters, invalid subsets, negative/fractional/overflowing values and unknown enum values.
- [x] Add separate-stream ordering assertions and atomic metrics replacement:

```text
Bind A/generation 1; activity revision 8 busy; metrics revision 20 accepted.
Activity revision 9 waiting => accepted despite metrics revision 20.
Metrics revision 19 => rejected without freshness change.
Bind B/generation 2, then A/generation 3.
Stop or unbind for A/generation 1 => rejected; generation 3 survives.
```

- [x] Define `Measurement<T>`, identified versus uncertain freshness, separately stored component age, and binding-scoped health as in the revised shared schema. Test transcript-only progress preserving context/cost age, conflicting reuse of a source revision, explicit null clearing, collector loss and late supervisor-disconnect cleanup. Report-connection closure alone is not a health event.
- [x] Implement under the existing state lock and publication path. Reject legacy updates while the new binding is active; preserve legacy behavior otherwise. Do not let attachment reset later busy/error observations.
- [x] Bump wire version from 4 and update all constructors/exhaustive matches, root facades and optional-feature callers. Incompatible peers must fail explicitly.
- [x] Run `rtk proxy cargo test -p ovrcr-protocol --lib` and focused runtime tests with an `agent_report` filter that executes nonzero tests. Save behavioral RED and passing evidence separately; missing-type compiler failures are not behavioral proof. Review and commit the shared unit.

## Task 3: Establish invocation ownership and supervised collection

**Files:** `src/report.rs`; new `src/report/claude.rs`, `src/report/collector.rs`, `src/cli/agent.rs`; CLI `args.rs`, `mod.rs`; existing `tests/cli.rs` and `tests/server_lifecycle.rs` fixtures.

**Interfaces:** Add `ovrcr agent run --provider claude -- <native argv...>` and an internal authenticated bind/unbind client using Task 2's wire types. Native hook commands retain their existing names. The managed launcher establishes an invocation; only a supervisor-authorized, certified foreground start establishes its conversation. SessionStart by itself never authorizes binding. A supervised collector receives exact-bound source metadata and emits `ProviderReport` replacements through the existing transport.

- [ ] Write real entry-point tests for argv after `--`, paths with spaces, native stdin/stdout/stderr, help/version, exit code, signal delivery and unavailable server. Help/version must not create a conversation binding.
- [ ] Implement an invocation-local channel/handle for the reporter and collector. Give the native process tree only access to the private report channel; keep the supervisor lease in the launcher, excluded from child environment/descriptors. The supervisor reserves ownership before native spawn and remains the PTY process until bounded finalization completes. Root/origin checks still reject child events and foreign-harness compatibility replay; inherited environment alone cannot prove root identity.
- [ ] Use one supervised collector at most. Keep transcript/provider I/O off the server dispatcher, and preserve sole socket-writer/queue rules. A callback must return within its existing one-second deadline even if collection is rebuilding.
- [ ] Validate exact root-reported transcript identity before reading. Do not discover files by cwd, newest mtime or a whole-home scan. Session switches explicitly replace the binding and cancel prior reads.
- [ ] Implement foreground admission from Task 1 with a table of certified transitions. Background fork => ignore for root binding; same-session compact => preserve; certified foreground switch => conditional bind; ambiguous new conversation => unavailable identity and no guessed switch. Test each through actual hook-to-supervisor routing.
- [ ] Test old callback/cleanup after replacement, collector crash, missing/rotated/truncated file, queue overflow and descriptor closure. Control/lifecycle delivery must be preserved or reporting explicitly disconnected.
- [ ] Add closing/finalization: SessionEnd(reason=clear) closes only the old conversation and invokes certified foreground-transition rules; it does not close the supervisor lease. Other SessionEnd reasons remain observations until native completion. Native completion plus certified accounting-source completion permits final extraction. Keep the supervisor alive for at most two seconds, publish final metrics and release atomically through `FinalizeAgent`, then preserve native exit status. On timeout, missing source or abrupt PTY death retain partial accounting with a reason. Test completion before collector catch-up, finalization-ack loss, hung collector and supervisor death; no post-exit capability bypass.
- [ ] Verify actual fixture-owned PID/PGID cleanup and native PTY behavior. Dashboard detach and pane close must leave the agent and reporting running.
- [ ] Run focused `agent_hook` tests in `tests/cli.rs` and `tests/server_lifecycle.rs`, verifying executed counts; save evidence and commit.

## Task 4: Make Claude activity observations accurate

**Files:** `src/report/claude.rs`, existing `src/report.rs::claude_activity`, `src/cli/report.rs`, Claude fixtures, runtime and CLI tests.

**Interface:** `parse_claude_hook(input: &[u8]) -> anyhow::Result<Option<ClaudeEvent>>`. `ClaudeEvent` retains session identity, optional source turn/event identity, optional exact transcript path and typed event. Conversion to `ActivitySample` occurs only after binding validation. Unrecognized events return `None`; malformed identity returns an error handled fail-open by the CLI.

- [ ] Add fixture assertions for root/child identity, malformed agent metadata, unknown events and delayed SessionStart. Preserve existing event tests while updating expectations for explicit quality.
- [ ] Map prompt/tool observations to busy, actual human approval/input signals to waiting, API turn failure to error, and SessionEnd to a reason-qualified conversation-end observation rather than immediate supervisor release or dead PTY; clear preserves the invocation and requires a new conversation binding. Finalization belongs to Task 3. Verify automatic approval and elicitation behavior against Task 1; generic notifications do not imply human wait.
- [ ] Keep Stop observed until its post-decision settling evidence is verified. Test a Stop hook that blocks completion and causes more work; no confirmed idle may appear between them.

```text
Root busy; child Stop => root remains busy.
Root busy; delayed same-binding SessionStart => root remains busy.
Turn B busy; terminal observation attributable to turn A => rejected.
Stop without reliable turn correlation => never confirmed idle for B.
Reporter disconnect or terminal silence => never inferred idle.
```

- [ ] Preserve activity command fail-open semantics, empty stdout and bounded redacted verbose errors for malformed input, missing identity, timeout and transport rejection.
- [ ] Run `rtk proxy cargo test -p ovrcr --lib claude` and `rtk proxy cargo test -p ovrcr --test cli agent_hook`; run runtime ordering regressions. Review and commit.

## Task 5: Add context, cumulative tokens and scoped cost

**Files:** `src/report/claude.rs`, `src/report/collector.rs`, `crates/ovrcr-protocol/src/context.rs` only where shared validation changes; new `tests/claude_reporting.rs` for pure accounting fixtures; existing socket/CLI tests for publication.

**Interfaces:** Keep `parse_claude_context` as the legacy projection. Add `parse_claude_metrics(input: &[u8]) -> anyhow::Result<ClaudeMetrics>` retaining context and optional cost separately from transcript totals. Add `ClaudeUsageAccumulator::apply_record(&mut self, record: &serde_json::Value) -> anyhow::Result<bool>` and `snapshot(&self) -> UsageTotals`. The accumulator runs in the collector, never the runtime dispatcher.

- [ ] Add context regression fixtures: input 30 + cache creation 10 + cache read 40 => occupancy 80; output excluded; missing component => unknown; post-compaction unknown clears the old value. No model-name capacity guesses.
- [ ] Add replacement-accounting tests before implementation:

```text
(message M, request R): input 10 then input 12 => total 12.
Distinct request S: input 12 => total 24.
Replayed R/S records => total remains 24.
Input 10 + cache read 4 + cache write 2, output 8 => canonical total 24.
No cost field => unknown; explicit 0 => zero.
```

- [ ] Implement the certified cumulative export or exact-transcript reader. Retain incomplete trailing records, replace prior values for a stable identity, and detect schema changes. Never sum status-line context counters as conversation usage.
- [ ] Use a non-evicting map keyed by `(message_id, request_id)` with last-record replacement. Charge retained key/value bytes before insertion and enforce both 65,536 identities and 16 MiB. On the next insertion exceeding a cap, freeze the exact prefix and mark `partial/accounting_limit`; do not skip a new identity and continue updating a misleading total. Clear/rebuild only on verified file replacement or conversation transition, with the same bounds. An authoritative aggregate may supersede the index if Task 1 certifies it.
- [ ] Read at most 256 KiB per iteration and check cancellation between reads; one raw record is capped at 32 MiB. Test correction/replay of the oldest retained identity, cap+1 distinct records, post-cap replays (partial prefix remains frozen), truncation/replacement and cancellation during rebuild. Tests use a configurable small capacity for boundary cases plus one production-capacity case. No eviction means no false claim of recognizing forgotten IDs.
- [ ] Handle new/resume/fork history and verified child/auxiliary sources. Missing identity/records or excluded native categories must result in partial coverage. Do not count both root aggregates and their child components.
- [ ] Parse the original USD decimal lexeme, scale by 10^10, then round to nearest integer tick with ties to even using checked decimal arithmetic. Test provider residue such as 0.23859819999999995 => 2385982000 ticks, exact half-tick ties, and overflow after rounding. Do not multiply/truncate binary floating-point values; overflow/negative/nonfinite cost is invalid. Preserve source estimate label and scope. Costs and tokens with different scope remain separately labeled.
- [ ] Merge context/status-line cost and transcript totals in one collector-owned sample before atomic publication. Retain per-component source freshness so a transcript update cannot freshen old context or cost. Equal values alone do not distinguish a new measurement from replay; use certified source identity, or expose freshness as uncertain.
- [ ] Run `rtk proxy cargo test -p ovrcr --test claude_reporting`, Claude parser tests, and actual CLI-to-runtime metrics publication assertions. Review and commit.

## Task 6: Deliver setup, diagnostics and display

**Files:** CLI `agent.rs`, `args.rs`, `report.rs`, `resources.rs`, `output.rs`; dashboard `state.rs`, `render.rs`, `input.rs`, `palette.rs`; existing `docs/agent-reporting.md`, README; new support document; CLI/resource/TUI tests.

**Commands:** `ovrcr agent setup claude --print`, `ovrcr agent doctor claude --json`, and `ovrcr session usage SESSION_ID`. Setup emits exact hooks/launcher/status-line composition instructions, without writing global settings. Doctor reports executable/version, configuration support, binding and source health, capabilities and concrete remediation; it never prints tokens or transcript content.

- [ ] Test setup output with existing hooks/status line and paths containing quotes/spaces. Mark OVRCR-owned entries so removal instructions cannot remove unrelated handlers. Do not alter trust/approval policy.
- [ ] Add `ovrcr report claude-statusline --stdin-json [--render-command <command>]`. Parse/report through the new route; default rendering prints `ctx N%` from the parsed payload independently of reporting success. A configured user command receives the original bytes once, with its normal stdout preserved. Bound OVRCR reporting independently so failure cannot suppress renderer output.
- [ ] Explicitly migrate the documented exact `ovrcr report claude-context --stdin-json` renderer (including a resolved absolute OVRCR path) to the new command’s default renderer. Never compose that known legacy reporter under an active binding. Leave arbitrary user shell commands untouched rather than attempting unsafe textual rewriting; print a concrete manual migration instruction for wrappers around the legacy command.
- [ ] Retain legacy `claude-context` behavior outside managed bindings. Test actual CLI paths for the documented legacy setup migrated under an active binding (`ctx N%` remains visible and one provider metrics update occurs), unknown external renderer called once, reporting timeout with renderer success, and legacy behavior outside the binding.
- [ ] Add JSON inspection for provider/conversation, activity quality, context freshness, token subsets, coverage and independently scoped cost. Unknown is null, not zero. Existing context inspection remains compatible.
- [ ] Render compact indicators using existing dashboard rows/actions. Include observed completion, partial usage and stale/unavailable reports. No new dashboard layout is required.

```text
Display checks: absent cost => —; explicit zero => $0.00;
estimated cost => estimate marker; partial tokens => partial marker;
unknown context after compaction => —, not the old percentage.
```

- [ ] Run affected `cli`, `resource_cli`, `tui` and `ovrcr-tui` tests with actual counts. Document setup/removal, migration from legacy adapters, privacy, version floor and known limitations. Review and commit.

## Task 7: Native acceptance and release gate

**Files:** Evidence under `research/claude-reporting-acceptance/` during execution; support documentation and this checklist.

- [ ] Use an isolated native interactive Claude session with unique OVRCR socket/config, provider settings and workspace. Record exact versions, revision, commands, PID/PGID ownership and sanitized observations.
- [ ] Prove root start, two turns, real approval wait/allow/deny, user-input elicitation where supported, cancellation, API failure, Stop continuation and final settling. Compare dashboard observations to actual CLI state, not command echo.
- [ ] Prove context before/after compaction, request replacement, native token/cost comparison, missing values, new/resume/fork and child isolation. Record the native reference total and OVRCR total with scope/coverage for each comparison.
- [ ] Prove detach/reattach, collector loss, stale prior-generation delivery, native exit status and owned cleanup. Server death is loss of live state, not conversation recovery.
- [ ] Run 50-session synthetic load through real sockets: 10 normalized updates/second/session for 60 seconds, with stale IDs and 10% collector failures injected. Require p99 control acknowledgement below 250 ms and no control acknowledgement above one second on the recorded host; zero cross-session updates; explicit unavailable state on overflow; and accounting index/queue counters never above their specified caps. Require aggregate process-tree RSS growth below 3 GiB above the same empty-session baseline under production-capacity fixtures. Record CPU/RSS and latency raw results; revise thresholds only by an explicit documented design decision, never after a failing run to label it passing. This is capacity evidence, not 50 paid Claude runs.
- [ ] Perform native macOS dashboard acceptance per `docs/testing-computer-use.md`, including small layouts and fresh screenshot/accessibility evidence. Execute Linux PTY/socket/process acceptance separately; unavailable required gates remain open.
- [ ] Run final workspace checks once:

```sh
rtk proxy cargo test --workspace --all-targets --all-features
rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk proxy cargo fmt --all -- --check
rtk proxy git diff --check
```

- [ ] Preserve every attempt under a distinct filename with command, revision, exit status and executed count. Keep compiler failures, behavioral failures and unavailable environment evidence distinct.
- [ ] Review the exact diff against this plan; update tested capabilities and the existing work diary. Claim full Claude integration only when required settling/accounting/platform gates pass. A provider export dependency may block release; it must not be replaced by a timeout heuristic.

## Handoff and relation to the broader plan

Tasks 2–3 implement the Claude-required portion of the broader plan's shared foundation. Reuse those interfaces when Pi, Grok, Codex and Hermes follow; do not repeat their implementation or require those adapters to ship before Claude. The additional component-freshness requirement in Task 5 refines atomic metrics: one update may carry several measurements of different ages.

## Review corrections and remaining prerequisites

| Finding | Correction | Required execution evidence |
| --- | --- | --- |
| 1: stale bind | Expected-epoch reservation, supervisor-only conditional bind, idempotent request IDs | Delayed reserve/bind and lost-ack races |
| 2: foreground ownership | Certified admission table; background/compact rules; explicit unsupported-switch fallback | Native branch/fork/clear/compact fixtures before dependent binding logic |
| 3: freshness/health | Per-component measurement identity/age, uncertainty and supervisor health path | Independent-age and old-supervisor cleanup assertions |
| 4: final accounting | Closing phase, two-second finalization, atomic final publication/release; abrupt-loss partial state | Exit-before-catch-up and failure assertions |
| 5: bounded accounting | Non-evicting capped index, frozen partial prefix and explicit load thresholds | Boundary/correction/rebuild and 50-session gates |
| 6: legacy status line | Known-renderer migration to explicit new command; independent rendering | Real CLI migration and unavailable-reporting checks |

These corrections define implementation contracts. Foreground/settled source certification and native accounting coverage are still open Task 1 gates; neither this revision nor a document review certifies provider behavior.

Deliver seven reviewed implementation units in dependency order. This plan itself is documentation only; no feature code, provider configuration or live acceptance is claimed by its creation.

## 2026-09-10 implementation refinements from native evidence

- The supported activity route uses synchronous command hooks on pinned Claude2.1.267. UserPromptSubmit is an observed turn boundary; opaque prompt IDs do not certify arbitrary asynchronous callback order. Setup/doctor must make that supported configuration explicit. A correlated native Notification(permission_prompt) was captured while the actual approval selector remained unanswered (attempt10); this supports Waiting/Observed. PermissionRequest by itself remains insufficient.
- Attempt10 ties the exact SessionStart transcript_path to the opened file and matching sessionId on assistant usage records, preserving device/inode across append. It supports the subset label **recognized root-transcript records**, not complete or root-only usage. Non-assistant metadata may omit identity; never treat it as usage.
- Differing-value replacement remains uncertified. The pure last-record replacement accumulator can be implemented and tested, but a live reader must reject a differing duplicate before applying it and freeze publication with partial coverage and `conflicting_usage_record`. Its retained number is neither current nor a proven lower bound after a contradiction. Unique identities and equal duplicates may be accumulated only after exact identity, lifecycle/bounds and actual publication tests pass. A later certified provider correction route can enable differing-value replacement; this refinement does not claim that certification.
- Complete accounting and final-write completion remain unverified. Native exit permits only bounded draining and partial final metrics, never promotion to complete based on EOF, Stop, SessionEnd or transcript silence.

## 2026-09-10 assembled implementation checkpoint

Implemented and reviewed: shared binding/observations, supervised initial
admission, synchronous observed activity, partial exact-source collector,
independent context/cost publication, bounded native finalization, setup/doctor,
usage inspection and compact dashboard presentation. Review corrections include
expired-deadline child reaping, macOS zombie-group cleanup, a required post-native
completion read, conservative supplied-file diagnostics and selected-header quality.
See the dated support document and saved acceptance evidence for exact commits.

The remaining unchecked source/acceptance requirements are intentional. Settled
completion, full auxiliary/resumed/forked accounting and final-source completion
remain uncertified; the implementation exposes their limits rather than inferring
success. That trust-pending statement records the first GUI attempt. The later
authorized native follow-up accepted trust, completed two turns, detach/reattach,
exit and owned cleanup; see the remaining-requirement matrix for the open cases.
Linux acceptance and internal queue-counter
capacity instrumentation remain unverified. No full-integration release claim is
made by this checkpoint.


2026-09-10 authorized native follow-up: trust approval completed; two native Claude turns, observed activity, partial metrics, cost comparison, detach/reattach and exit0 verified. All recorded fixture processes, socket and disposable directory cleaned. See `research/claude-reporting-acceptance/task7-gui-native/review.md`. Earlier pending trust/cleanup statements are historical; provider completeness and Linux gates remain open.
