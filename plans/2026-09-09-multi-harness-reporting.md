# Multi-harness Agent Reporting Implementation Plan

> **For agentic workers:** Use the executing-plans skill to implement this plan task by task. This document authorizes planning only; implementation begins under a separate execution request. Checkboxes remain open until their evidence exists.

**Goal:** Complete interactive status, context, token, and cost reporting for Claude Code, Codex CLI, Grok CLI, Pi, and Hermes.

**Architecture:** Native hooks/extensions report through the existing authenticated Unix socket. The synchronous runtime owns the current binding and observations. Exact-session collectors are used only when native callbacks cannot provide the required metrics.

**Tech stack:** Existing Rust workspace and dependencies, provider-native JavaScript/Python extensions where required, bounded synchronous helper I/O. No new service, database, agent SDK, or Tokio.

**Spec:** [Reporting design](2026-09-09-multi-harness-reporting-design.md). Read with [Superset source review](../research/superset-agent-reporting-2026-09-09.md), pinned at `1112b2feb1a0cfc59e8eb7133e073e8741c87f20`.

For the shared foundation and Claude adapter, the revised [Claude plan](2026-09-09-claude-code-reporting.md) and shared design govern lease ownership, per-component freshness/health, finalization, bounded accounting and legacy status-line migration. Their explicit contracts supersede the earlier shorthand below.

## Global constraints

- All five harnesses are required. Interactive native-CLI acceptance is mandatory; one-shot examples do not substitute.
- Preserve one server owner, one active dashboard, synchronous dispatch, and 50 sessions.
- Keep live reporting in memory. Historical analytics, account quotas, billing reconciliation, gateway integration, and conversation recovery are out of scope.
- Unknown metrics remain unknown. Cost carries reported/estimated provenance and its own conversation/invocation scope.
- Preserve provider configuration, approvals, hooks, status-line stdout, and extension lists. No global silent install, trust bypass, or sandbox relaxation.
- Hook stdin maximum is 65,536 bytes; helper deadline is one second. Callbacks fail open and emit no reporting text on stdout.
- Activity and metrics have separate revision watermarks. Metrics replace atomically. Context occupancy and cumulative consumption are different quantities.
- Installed versions in the design are research anchors, not certified support floors. Live verification establishes the support matrix.
- Read current AGENTS.md and source before implementation; verify branch/base/status and create an isolated `codex/` worktree. Do not change the user's live provider configuration for testing.

## Delivery order and file ownership

Execute Tasks 1–3 first, then provider Tasks 4–8, then presentation and release Tasks 9–10. Provider tasks consume the same interface but may expose missing provider exports; resolve those gates before claiming the task complete. No provider may be omitted because its route is harder.

| Files | Responsibility |
| --- | --- |
| `crates/ovrcr-protocol/src/agent.rs` (new), `session.rs`, `wire.rs`, `codec.rs`, `lib.rs` | Validated provider observations, binding requests, snapshots and versioning |
| `crates/ovrcr-runtime/src/session/{mod.rs,tests.rs}`, `server/{dispatch.rs,connections.rs,tests.rs}` | Sole state authority, authentication, binding/revision checks, publication |
| `src/report.rs` | Keep existing transport and declare provider modules |
| `src/report/{provider.rs,claude.rs,codex.rs,grok.rs,pi.rs,hermes.rs,collector.rs}` (new) | Normalization, source accounting and bounded collectors; modules declared from existing `src/report.rs` |
| `src/cli/{args.rs,mod.rs,report.rs,resources.rs,output.rs,agent.rs}` (`agent.rs` new) | Native-report command, managed launch, setup/diagnostics, inspection output |
| `integrations/pi/ovrcr-reporting.ts`, `integrations/hermes/ovrcr_reporting.py` (new) | Native extension/plugin glue only |
| `tests/agent_reporting.rs`, `tests/fixtures/agent-reporting/` (new) | Sanitized source fixtures and real entry-point regression tests |
| `crates/ovrcr-tui/src/dashboard/{agents.rs,state.rs,render.rs,input.rs,palette.rs}` | Harness names, status/usage presentation and actions |
| `tests/{cli.rs,resource_cli.rs,server_lifecycle.rs}`, `tests/tui/{sidebar.rs,palette.rs}` | Existing caller-path acceptance |
| `docs/agent-reporting.md`, `docs/agent-reporting-support.md` (new), `README.md` | Setup, semantics, diagnosis and tested capabilities |

## Task 1: Certify source contracts before building dependent adapters

**Files:** Create `tests/fixtures/agent-reporting/manifest.json` and `docs/agent-reporting-support.md`; update design routes only when evidence warrants.

**Interface:** Manifest entries have `provider`, `version`, `source_revision`, `fixture`, `event`, `identity_fields`, `counter_scope`, `cost_kind`, `evidence_kind`. Evidence is `source` or `live`; a source fixture is never labeled live.

- [ ] Capture sanitized root/child lifecycle and metrics payloads for each installed version. Remove prompt, tool, response and credential content; retain IDs needed to demonstrate ordering using synthetic replacements.
- [ ] For Codex, prove a second observer can receive the exact native CLI thread's status/token notifications without modifying its configuration, resuming work, or becoming an approval responder. If not, select an explicit-thread, versioned rollout reader and document its limits; do not attach through mutating `thread/resume` merely to obtain telemetry.
- [ ] For Hermes, verify whether a public plugin export provides the CLI's full context/session accounting snapshot. If absent, specify and deliver a supported provider-side export before completing Task 8. Required payload: session/turn identity, source revision, model, current context and capacity, complete session token breakdown, optional cost with scope. Cover restored and auxiliary work. Do not access a live private agent object from OVRCR.
- [ ] For Claude and Grok, establish which event proves final settling after a Stop hook continues. If the installed release cannot prove it, record Stop as observed and make a supported settled export/versioned reader a delivery dependency.
- [ ] Record live new/resume/fork behavior and whether IDs change in-process. Explicitly list unsupported metrics versus missing implementation.
- [ ] Validate every manifest fixture parses and every named field exists. Save command, revision and result. This task is complete when each provider has a concrete source route; unavailable upstream exports remain named blockers for that provider.

## Task 2: Add typed reports and runtime binding authority

**Files:** Protocol and runtime files in the ownership table; `src/report.rs`; `tests/agent_reporting.rs`.

**Interfaces:** Implement the design's `AgentProvider`, `AgentBinding`, `ProviderReport`, `AgentObservation`, `ActivitySample`, `MetricsSample`, `UsageTotals`, and related enums. Add capability-authenticated bind/unbind wire requests. A bind request supplies provider/invocation/conversation; runtime returns an `AgentBinding` with a new generation. An unbind supplies that exact binding. Add `AgentUpdate::Provider(ProviderReport)`; retain the existing outer authentication envelope.

- [ ] Add failing runtime assertions through the report dispatcher: a current-generation busy report is applied; a higher-revision report from an old generation is rejected; old unbind cannot clear a replacement. Include A→B→A conversation reuse.
- [ ] Add independent-stream assertions: metrics revision 20 does not reject activity revision 3; metrics revision 19 cannot replace metrics 20. A rejected report changes neither state nor freshness.
- [ ] Add codec round trips and validation for optional counters, subset relationships, overflow, IDs over 256 bytes/control characters, unknown enum variants and invalid capacity. Preserve redacted capability Debug.

```rust
// Accounting validation example; construct the remaining optional fields as None.
let totals = UsageTotals {
    scope: UsageScope::Conversation, coverage: UsageCoverage::Complete,
    input_tokens: Some(100), output_tokens: Some(20),
    cache_read_tokens: Some(40), cache_write_tokens: Some(10),
    reasoning_output_tokens: Some(5),
};
assert_eq!(totals.input_tokens.unwrap() + totals.output_tokens.unwrap(), 120);
// A cache subset of 101 with input 100 must fail wire validation.
```

- [ ] Implement allocation/validation/mutation under the existing runtime state authority and enqueue publication through the existing dispatcher. Do not create a second report map in root CLI or TUI.
- [ ] Preserve later activity on delayed attachment. Treat process exit independently; apply final PTY output first. Reject legacy updates only while an active provider binding owns the report.
- [ ] Bump wire version from 4, update every constructor/match/re-export and optional GUI caller, and test incompatible-client failure explicitly.
- [ ] Run `rtk proxy cargo test -p ovrcr-protocol --lib`, `rtk proxy cargo test -p ovrcr-runtime --lib agent_report`, and `rtk proxy cargo test -p ovrcr --test agent_reporting`. Verify nonzero executed counts. Record initial behavioral failures separately from missing-type compiler errors, then passing evidence and a coherent commit.

## Task 3: Implement shared adapter transport, lifecycle and safe setup

**Files:** `src/report.rs`, new `src/report/provider.rs`, `src/report/collector.rs`, new `src/cli/agent.rs`, existing CLI modules; new integration tests.

**Interfaces:** Native glue invokes `ovrcr report native --provider <name>` with bounded JSON stdin. Rust normalizers expose `normalize(payload: &serde_json::Value) -> Result<Option<AgentObservation>, String>` inside each provider module; `None` means intentionally ignored event. The caller supplies authenticated binding/revision separately. Stateful sources live in the collector, not this pure function.

Expose `ovrcr agent run --provider <name> -- <native argv...>`, `ovrcr agent setup <name> --print`, and `ovrcr agent doctor <name> --json`. Setup prints exact owned configuration/extension instructions; it does not silently edit global files. Doctor returns provider/version, integration availability, binding/source health, metric capabilities and specific remediation, with credentials redacted.

- [ ] Write real CLI/socket tests for missing/invalid capability, oversized stdin, deadline expiry, malformed JSON, unrecognized event, and foreign-harness replay. Assert callback stdout is empty and native command behavior is unaffected.
- [ ] Implement root invocation ownership and origin-harness guards. Child callbacks cannot create bindings. Native in-process session switches must replace the binding explicitly.
- [ ] Implement launcher only where binding/collector supervision requires it. Preserve argv after `--`, PTY, native exit status and signals. Help/version invocations must not register an active conversation. No binary shadowing or trust flags.
- [ ] Add deterministic tests for late callback/unbind after replacement, collector timeout, and process-group cleanup using only fixture-owned processes. Detach/pane close must leave agent/reporting alive.
- [ ] Implement bounded incremental collectors: 256 normalized pending records, 1 MiB normalized queue, one raw record at a time capped at 32 MiB. Preserve partial JSONL tails and detect file replacement/truncation. Oversized/unknown records make coverage partial with an error reason.
- [ ] Coalesce replacement metrics; preserve lifecycle ordering or explicitly disconnect reporting on overflow. Allocate revisions before asynchronous callback delivery; never use receipt order to infer a stale turn's completion.
- [ ] Test setup output with spaces/quotes in paths and preservation of an existing status-line command. Doctor must distinguish unsupported version, disabled hook trust, missing source and temporarily disconnected reporter.
- [ ] Run `rtk proxy cargo test -p ovrcr --test agent_reporting`, `rtk proxy cargo test -p ovrcr --test cli`, and affected runtime lifecycle filters with actual test counts. Commit after focused review.

## Task 4: Complete Claude Code integration

**Files:** New `src/report/claude.rs`; existing Claude path in `src/report.rs`; Claude fixtures; setup/doctor branch in `src/cli/agent.rs`; `docs/agent-reporting.md`.

**Consumes:** Shared binding, native-report command and normalization interface from Tasks 2–3. **Produces:** Claude activity observations, atomic context/usage snapshots and owned setup instructions.

- [ ] Add fixtures for SessionStart, prompt submit, actual permission request, tool outcomes, Stop, continued Stop, StopFailure, cancellation and SessionEnd. Assert child `agent_id` events do not change root activity/identity.
- [ ] Map tool execution to activity without claiming every tool requires permission. Preserve observed versus confirmed settling from Task 1. Test delayed SessionStart after busy/failed state.
- [ ] Normalize status-line current occupancy and capacity. Do not call context-window cumulative-looking fields conversation totals. Preserve the user's status-line output by teeing the same payload through the existing command exactly once.
- [ ] Implement a versioned exact-transcript reader if the certified release lacks a cumulative export. Deduplicate by message plus request identity; replace earlier usage for that identity with the final record. Aggregate usage fields without adding cache subsets twice.

```text
Fixture assertion: same (message,request), input 10 then input 12 => 12.
Different request with identical input 12 => combined 24.
Status-line replay => unchanged tokens and measurement freshness.
Context input 30 + cache read 40 + cache write 10 => occupancy 80.
```

- [ ] Carry provider cost with documented scope/provenance; absent cost stays unknown. Test resume/fork replay and incomplete transcript tail without false complete coverage.
- [ ] Supply setup/removal instructions for only OVRCR-owned hooks and status-line composition; test fixtures containing unrelated hooks/settings.
- [ ] Run `rtk proxy cargo test -p ovrcr --test agent_reporting claude`; then execute the full live checklist in Task 10 for Claude. Record tested version, source contracts and any unsupported metric. Commit only the completed adapter unit; keep live acceptance unchecked if unavailable.

## Task 5: Complete Codex CLI integration

**Files:** New `src/report/codex.rs`; shared collector; Codex fixtures; setup/doctor; reporting docs.

**Consumes:** Task 1's proven exact-thread route and Tasks 2–3. **Produces:** Codex activity/context/totals with no approval/config changes.

- [ ] Add native hook fixtures with thread/session and turn IDs, approval requests, interruption, child callbacks and failed turns. Test child completion never idles the root.
- [ ] If passive app-server observation passed Task 1, consume `thread/tokenUsage/updated.total` and `last` plus `modelContextWindow`, and thread status active flags for approval/user input. Reconnection replaces current totals instead of adding replayed notifications.
- [ ] Otherwise implement the certified rollout schema against the explicitly bound thread file. Prefer authoritative cumulative usage; where only per-request data exists, require stable identity and correction semantics. Never skip legitimate equal consecutive requests by JSON equality.

```text
Fixture assertion: cumulative total 100, 100, 140 => 140, not 340.
Two distinct requests each using 20 => 40, not 20.
inputTokens 100, cachedInputTokens 40, outputTokens 20,
reasoningOutputTokens 5 => total 120, not 165.
```

- [ ] Prove context mapping against the actual CLI display after compaction. Unknown context clears old occupancy. Cost remains unknown unless a verified source supplies it; add no local price table.
- [ ] Include hook trust checks and actionable setup instructions. Do not copy Superset's `--dangerously-bypass-hook-trust`, legacy notify assumptions or undocumented TUI text heuristics.
- [ ] Test resume/new/fork, observer disconnection, source truncation/replacement, terminal exit, and old-thread events after rebinding.
- [ ] Run `rtk proxy cargo test -p ovrcr --test agent_reporting codex`; execute Task 10's live checklist, including an approval whose responder remains the native CLI. Save evidence and commit the adapter unit.

## Task 6: Complete Grok CLI integration

**Files:** New `src/report/grok.rs`; Grok fixtures; setup/doctor; reporting docs.

**Consumes:** Shared interfaces; installed hooks/status-line/usage contracts. **Produces:** Grok reports using native structured sources rather than global inference-log scanning.

- [ ] Normalize both documented hook naming conventions and validate `sessionId`, `promptId`, `subagentType`. Add SessionStart/End, prompt, tool success/failure, permission notifications, Stop, StopFailure and StopCancelled fixtures.
- [ ] Enforce current prompt identity. A cancelled older prompt cannot idle a newer prompt even if delivered later. A Stop gate is not confirmed settled until the certified source proves it.

```text
Fixture sequence: prompt A busy; prompt B busy; StopCancelled(A).
Expected final activity: busy for B; no freshness advance from rejected A.
Foreign Claude compatibility hook inside Grok: cannot bind Claude.
```

- [ ] Normalize status-line context, `session_usage`/session token totals and optional cost. Repeated refresh payloads do not increase totals or measurement freshness.
- [ ] Use persisted `grok usage SESSION_ID` only when necessary to obtain a complete resumed baseline or inspect discrepancies. Convert `costUsdTicks` at 10^10 per USD. Label conversation tokens and invocation cost separately when their scopes differ.
- [ ] Test minimal and fullscreen interfaces, shared-leader environment propagation, in-process session changes, child isolation and new/resume/fork accounting.
- [ ] Produce owned hook/status-line configuration instructions without disabling the user's unrelated Claude/Cursor compatibility settings.
- [ ] Run `rtk proxy cargo test -p ovrcr --test agent_reporting grok`; execute Task 10's live checklist with direct provider-total comparison. Save evidence and commit.

## Task 7: Complete interactive Pi integration

**Files:** New `integrations/pi/ovrcr-reporting.ts`, `src/report/pi.rs`, Pi fixtures; setup/doctor/docs. Inspect but preserve `src/pi-task-extension.mjs` and runtime task-runner ownership.

**Consumes:** Native-report command and binding protocol. **Produces:** Extension snapshots with revisions captured synchronously before sending.

- [ ] Add extension fixtures for session_start, agent_start, agent_end, agent_settled, UI prompt start/end, shutdown, reload and in-process new/resume/fork. `agent_end` during retry/compaction must not mean final idle.
- [ ] Implement `agent_settled` reporting and human UI-prompt wait using installed extension APIs. Coalesce nested prompt spans; dismissing an inner prompt must not clear an outer wait.
- [ ] Obtain context from `ctx.getContextUsage()`, preserving null/estimated state after compaction. Obtain session identity/file/entries from the public session manager.
- [ ] Match Pi session accounting across all entries, including assistant usage, tool-result usage, and compaction/branch-summary usage. Do not count only the visible branch or only assistant messages when native totals include more.

```text
Fixture usage: assistant 10; tool-result 3; compaction 4; branch-summary 2.
Expected native-equivalent aggregate: 19; replaying the snapshot remains 19.
agent_end followed by continuation => no confirmed idle before agent_settled.
```

- [ ] Mark provider-computed pricing estimates accordingly; preserve cache convention in canonical input totals. Never read a private agent instance to obtain stats.
- [ ] Load explicitly or install an owned extension without setting an alternate agent directory that hides user extensions. Ensure callbacks outside OVRCR are inert and reload cleans owned resources.
- [ ] Run `rtk proxy cargo test -p ovrcr --test agent_reporting pi` and affected existing `task_runner`/`task_execution` tests to prove scheduled Pi remains correct. Execute Task 10's interactive checklist separately; save evidence and commit.

## Task 8: Complete Hermes integration and required export

**Files:** New `integrations/hermes/ovrcr_reporting.py`, `src/report/hermes.rs`, Hermes fixtures; setup/doctor/docs. Provider-side export changes belong in the Hermes repository, separately reviewed and versioned, if Task 1 proves they are required.

**Consumes:** Plugin hooks and the supported full snapshot contract established by Task 1. **Produces:** Interactive lifecycle, context and complete/partial accounting with explicit capability diagnostics.

- [ ] Add lifecycle fixtures keyed by session and turn. Map `on_session_end` as a turn-finalization hook for the inspected version, not process termination. Inspect completed/failed/interrupted/turn_exit_reason, since post_llm_call omits some non-success paths.
- [ ] Use actual human approval surfaces for waiting state. Smart automatic approval is not user-input wait. Cover once/session/always/deny/timeout and restoration of busy after response.
- [ ] Normalize per-request usage by stable API request identity. Hermes canonical input excludes cache: canonical OVRCR input is input + cache read + cache write. Reasoning remains an output subset.

```text
Fixture: Hermes input 10, cache read 4, cache write 2, output 8,
reasoning 3 => OVRCR input 16, output 8, total 24.
Duplicate api_request_id => no increment.
interrupted on_session_end => cancelled turn observation, not ended PTY.
```

- [ ] Consume full context/session snapshots for restored baselines, auxiliary work and cost; API deltas alone cannot claim complete coverage. Test native Codex backend and auxiliary accounting paths explicitly.
- [ ] If export is missing, add a supported Hermes plugin snapshot event/accessor at the owner of CLI accounting, including final/error/interrupt updates and session identity. Ship/test that upstream dependency and set the minimum version. Until available, doctor reports the concrete missing capability and Task 8 remains incomplete.
- [ ] Preserve plugin and first-use shell-hook consent. Do not monkeypatch `cli.py`, serialize raw request content, or use one-shot `--usage-file` as interactive acceptance.
- [ ] Run `rtk proxy cargo test -p ovrcr --test agent_reporting hermes`, provider-native plugin tests, and Task 10's full live checklist. Save separate upstream export and OVRCR adapter evidence; commit completed units.

## Task 9: Expose consistent status and usage in CLI/dashboard

**Files:** CLI output/resources, dashboard files and caller tests from ownership table; protocol snapshot constructors as required.

**Consumes:** Runtime-authoritative snapshots. **Produces:** All five names, status quality/health, context freshness, token breakdown and scoped cost in inspection and dashboard.

- [ ] Add JSON inspection tests for unknown vs zero, partial coverage, estimated cost, different token/cost scopes, stale context and disconnected reporting. Keep legacy fields as projections from the same snapshot.
- [ ] Add Grok/Hermes detection and launch/setup actions using the existing searchable action UI. Executable-name detection is presentation only, never reporting identity authority.
- [ ] Render compact status/context in existing rows and usage in existing inspection surfaces. Make observed completion and unsupported/unknown values unambiguous without introducing a new dashboard layout.

```text
Rendering fixtures: unknown cost => "—", exact zero => "$0.00";
estimated cost => approximation marker; partial tokens => partial marker;
context stale => stale marker; disconnected reporter => never inferred idle.
```

- [ ] Test tiny widths, Unicode names, long models and all five providers. Verify no panics/overflow and current view/input acknowledgement invariants remain unchanged.
- [ ] Run `rtk proxy cargo test -p ovrcr --test resource_cli`, `rtk proxy cargo test -p ovrcr --test tui`, and affected `ovrcr-tui` library tests. Save counts and commit.

## Task 10: Verify all five end to end and publish support documentation

**Files:** `docs/agent-reporting.md`, `docs/agent-reporting-support.md`, README, evidence under `research/agent-reporting-acceptance/` (created during execution). Update plan checkboxes only from recorded results.

- [ ] For **each** of Claude, Codex, Grok, Pi and Hermes, run an isolated native interactive fixture with unique `OVRCR_CONFIG`, `OVRCR_SOCKET`, workspace and provider configuration. Record executable version, source revision, fixture PIDs/PGIDs, commands and terminal evidence.
- [ ] Prove root attachment; two successive turns; actual permission/input wait where supported; denial; interruption; provider error; automatic continuation; final settling; compaction/context refresh; token and cost comparison to native source; new/resume/fork; child isolation; detach/reattach; old-event rejection; and owned cleanup. Mark truly unavailable provider features unsupported with source evidence, never mark unrun cases passed.
- [ ] Run deterministic 50-session mixed-provider replay/load through actual sockets/dispatcher, including repeated metrics, stale identities and collector failure. Assert bounded queues/memory, responsive control acknowledgements, no cross-session updates and no lost lifecycle events without explicit disconnection. Record observed latency/memory and compare with the pre-change baseline; investigate regressions before acceptance.
- [ ] Perform native macOS dashboard acceptance using `docs/testing-computer-use.md`, actual child-output markers, fresh screenshots/accessibility, and tiny/normal layouts. Run Linux PTY/socket/process acceptance separately. Unavailable platform gates stay open.
- [ ] Run final checks once at the acceptance checkpoint:

```sh
rtk proxy cargo test --workspace --all-targets --all-features
rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk proxy cargo fmt --all -- --check
rtk proxy git diff --check
```

- [ ] Save each attempt under a distinct filename with revision, command, exit status and executed count. Retain failing evidence. Verify cleanup through owned process/PTY/socket boundaries; permission denial is not proof of cleanup.
- [ ] Document exact setup/removal steps for every harness, tested minimum version, metric provenance/scope, unsupported fields, trust/consent requirements, stale/error diagnosis and the distinction between detached sessions and dead-server relaunch.
- [ ] Review the exact completed diff against all five provider checklists and shared invariants. Resolve blocking findings, update the same work diary entry, and deliver verification plus remaining limitations. Do not claim full implementation while an upstream export, provider run or required platform gate remains open.

## Completion ledger

| Requirement | Owning tasks | Evidence required |
| --- | --- | --- |
| Identity, ordering, legacy compatibility | 2–3 | Dispatcher/socket tests including A→B→A and delayed attachment |
| Context, token/cost normalization and bounded collectors | 2–8 | Sanitized source fixtures plus native comparisons |
| Claude Code | 4, 10 | Installed native CLI checklist |
| Codex CLI | 1, 5, 10 | Safe exact-thread observation and native checklist |
| Grok CLI | 6, 10 | Delayed cancel/shared-leader/status-line checklist |
| Pi | 7, 10 | Interactive extension and scheduled-runner regression |
| Hermes | 1, 8, 10 | Supported full snapshot, auxiliary/backend and native checklist |
| Setup, diagnostics, CLI and dashboard | 3, 9–10 | Config fixtures, CLI assertions and native GUI evidence |
| Capacity and platforms | 10 | 50-session load, macOS, Linux and workspace gates |

Planning and source review are complete when these documents are validated. Application implementation and all acceptance checkboxes above remain open.
