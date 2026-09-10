# Multi-harness agent reporting design

Status: proposed implementation contract; no implementation or live acceptance is claimed.

## Goal and scope

Complete status, context, and usage reporting for **Claude Code, Codex CLI, Grok CLI, Pi, and Hermes** in OVRCR-managed interactive terminals. All five are required for feature completion. Provider adapters, installation instructions, diagnostics, CLI inspection, dashboard presentation, and real-harness acceptance are part of the feature.

Preserve each harness's native interactive interface. Headless/RPC-only demonstrations do not satisfy interactive support. The existing scheduled Pi runner must remain correct; it is not a substitute for the interactive Pi adapter.

“Usage” means token totals and harness-reported cost, with explicit scope and provenance. Account allowances, subscription quotas, billing reconciliation, a historical analytics database, remote gateways, and agent control are outside this feature. An unavailable cost must display as unknown, not zero. A metric a provider does not expose may be declared unsupported with evidence; an adapter that has not been implemented or verified may not use that designation.

## Inspected baseline

- Repository: `/Users/xlyk/Code/ovrcr`, clean `main`, `188a6456e42607634aedc72d3a93697faebcb0e1` on 2026-09-09.
- Wire protocol is version 4. `AgentUpdate` carries activity or context. `SessionSummary` has `activity` and `context_usage`; no cumulative token/cost report exists.
- `src/report.rs` handles inherited socket/session/capability identity, bounded stdin, deadline-limited transport, and Claude activity mapping.
- `src/cli/report.rs`, `src/cli/args.rs`, `src/cli/output.rs`, and `src/cli/resources.rs` own the actual CLI caller paths.
- `crates/ovrcr-runtime/src/session/mod.rs::apply_agent_report` owns validation/order/state mutation. Connections enqueue reports through the synchronous dispatcher.
- Activity and context currently choose receipt or sequenced ordering independently for the PTY lifetime. A sequence allocated when a delayed callback arrives cannot reconstruct provider causality.
- Dashboard detection in `crates/ovrcr-tui/src/dashboard/agents.rs` includes Claude, Codex, and Pi, but not Grok or Hermes.
- `src/pi-task-extension.mjs` and `crates/ovrcr-runtime/src/task_runner.rs` implement scheduled-Pi process ownership. Preserve them.
- Archived hook/context plans are historical requirements, not proof of live-provider acceptance.

## Provider evidence and required routes

Installed versions are research anchors, **not yet certified minimum versions**. Task 1 of the implementation plan turns these into tested support floors. Do not upgrade or reconfigure a user's installation to make a test pass.

| Harness / inspected version | Activity route | Context route | Cumulative usage route | Required unresolved proof |
| --- | --- | --- | --- | --- |
| Claude Code 2.1.267 | Command hooks; settle only from verified final observations | Status-line `current_usage` and capacity | Cost from status line; tokens from a versioned, explicit-transcript reader if no supported cumulative export exists | Cancellation, continued Stop gates, final-record identity and deduplication |
| Codex CLI 0.153.0 | Native hooks; prefer app-server runtime status for authoritative settling | Same-session app-server token usage `last` plus `modelContextWindow` | `thread/tokenUsage/updated.total`; cost unknown unless separately reported | Joining the exact live CLI thread without control/config changes or altered approval routing |
| Grok CLI 1.0.24 (`68e414c661e3`) | Hooks with `sessionId`, `promptId`, child filtering; status-line reconciliation | `context_window.context_tokens` and `context_window_size` | Status-line `session_*`/`session_usage`, optional cost; `grok usage` for persisted inspection | Shared-leader environment, delayed cancellation, Stop continuation, resume cost scope |
| Pi 0.85.1 | Extension `agent_start`, `agent_settled`, UI prompt lifecycle | `ctx.getContextUsage()` | Entries from `ctx.sessionManager.getEntries()`, matching Pi's session accounting | Retry/compaction continuations, extension reload, summary/tool usage, nested prompt order |
| Hermes 0.20.5, upstream `5ef1409f` | Plugin lifecycle and approval hooks | Supported snapshot export; existing CLI snapshot supplies implementation reference | API usage hooks plus a supported full snapshot for restored/auxiliary totals and cost | Export availability, resumed baseline, non-success endings, native Codex backend, auxiliary work |

### Evidence references

Fetched 2026-09-09; implementation must record the version/source revision it actually tests.

- [Claude hooks](https://code.claude.com/docs/en/hooks) and [status line](https://code.claude.com/docs/en/statusline). Current documentation distinguishes context-window counters from cumulative spend. It does not establish a complete cumulative token export. Do not sum repeated status-line samples.
- [Codex hooks](https://learn.chatgpt.com/docs/hooks) and [app-server](https://learn.chatgpt.com/docs/app-server). Hooks expose session/turn identifiers. `thread/read` is not an event subscription. A generated schema demonstrates message shape, not that a second client can safely observe the native CLI.
- Installed Codex schema generated with `rtk proxy codex app-server generate-json-schema --experimental --out /private/tmp/ovrcr-codex-reporting-schema-20260909`: `ThreadTokenUsageUpdatedNotification`, `ThreadStatusChangedNotification`, `ThreadReadParams`, `ThreadResumeParams`. Regenerate during implementation; the temporary directory is not a delivery dependency.
- [Grok upstream hook guide](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks.md). Installed guides: `/Users/xlyk/.grok/docs/user-guide/{10-hooks,17-sessions,25-status-line}.md`. The installed guide documents late turn-end delivery and the distinction between Stop gates and settled state.
- [Pi extension guide](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md), [session format](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/session-format.md). Inspected installed equivalents and `dist/core/{agent-session.js,extensions/types.d.ts,session-manager.d.ts}` beneath `/Users/xlyk/.local/lib/node_modules/@earendil-works/pi-coding-agent/`.
- [Hermes hook guide](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/hooks.md). Inspected `/Users/xlyk/.hermes/hermes-agent/{hermes_cli/plugins.py,agent/shell_hooks.py,agent/turn_finalizer.py,agent/conversation_loop.py,run_agent.py,cli.py,agent/usage_pricing.py}`. `--usage-file` is one-shot-only and cannot supply interactive reporting.

## Architecture and ownership

The [Superset source review](../research/superset-agent-reporting-2026-09-09.md) informs this design: preserve native adapters, root/child isolation, owned configuration, and separate accounting. Do not copy heuristic token deduplication, broad permission-event mapping, trust bypasses, or its historical analytics service. A delayed attachment must preserve a later activity observation for the same binding.

Reuse the current private Unix socket and synchronous dispatcher. Add typed provider observations, not a second runtime store. The OVRCR session remains the authority for the displayed report, binding generation, accepted revision, and receipt times.

Provider integration consists of a small native hook/extension/plugin plus, only where necessary, a bounded collector for a structured stream or exact transcript. A collector is an ordinary synchronous helper, not a new service or SDK. Its lifecycle belongs to the managed terminal's reporting integration. One collector per managed root is the maximum; no process per token. Fast hook-only paths may send directly through the existing helper.

Installation explicitly chooses an OVRCR-managed root invocation. Introduce `ovrcr agent run --provider <provider> -- <native argv...>` as a PTY-internal launcher when lifecycle binding or a collector is needed. It inherits the existing PTY rather than allocating another one. Before spawning the native CLI, it reserves an inactive reporting slot with an expected runtime epoch and receives a supervisor lease. It cannot replace an active owner. Normal child completion triggers bounded final accounting before lease release; abrupt PTY exit marks unfinalized accounting partial. Preserve native exit status, signals, stdin/stdout/stderr, terminal modes, and process ownership. Raw `ovrcr new ... -- codex` and ordinary shell sessions remain usable; missing reporting setup must be visible in diagnostics.

The launcher provides a fresh invocation identifier to all reporting components. A root provider session start is a candidate observation, not permission to bind. Only a supervisor holding the current lease may bind the conversation after verifying foreground ownership. A child event cannot create or replace the binding. In-process new/resume/fork actions require a certified foreground transition and a compare-and-swap against the current binding. Background forks cannot bind; same-conversation compaction preserves the binding. If the native source cannot distinguish a transition, report identity unavailable and require a new managed invocation rather than guessing. A late event from the prior generation cannot reclaim it, including A→B→A conversation reuse. Native conversation identity is not OVRCR's numeric session ID.

Do not infer roots from cwd, process names, newest log files, or first arbitrary callback. For a shared provider daemon, bind using its explicit thread/session identifiers and verify that identity propagation works. Never broadcast OVRCR capability tokens into another provider session.

## Reporting semantics

### Activity

Keep `unknown`, `idle`, `busy`, `waiting-input`, and `error`. Preserve process phase separately. Pause/exit is not an agent activity observation.

Attach evidence quality: `confirmed` for a current runtime snapshot or verified settled event; `observed` for a hook that reports a lifecycle observation but cannot prove settling. The UI must not imply confirmed completion from an observed Stop callback. Reporting health (`connected`, `stale`, `unavailable`) is separate from agent activity: a silent or disconnected agent is never automatically idle.

Use source ordering when available. A per-process extension/plugin captures its revision synchronously before asynchronous delivery. For shell hooks with opaque turn IDs, compare against the current turn and reject terminal events for another turn. Do not sort opaque IDs or synthesize causal order from timestamps. A terminal callback without a matching turn must be treated as an observation until a current authoritative snapshot reconciles it. Generic legacy reports retain their documented receipt-order limitations.

Continued Stop gates, automatic retries, compaction, queued follow-ups, and background children are explicit acceptance cases. If a supported release has no authoritative signal for a required transition, deliver a provider-side export or a verified versioned reader before marking that transition complete; do not quietly substitute a timer.

### Context

Current occupancy is independent of cumulative consumption. Store source, model, used tokens, capacity, whether the value is estimated, and freshness. Preserve the existing five-minute stale rule, but a timer replay of an unchanged provider sample must not advance the measurement's freshness. Compaction can decrease context. Unknown post-compaction occupancy clears the previous current value rather than retaining a pre-compaction number.

### Token and cost totals

- Store input tokens **including cache reads/writes**, output tokens, optional cache-read/write subsets, and optional reasoning-output subset. Never add subsets twice.
- Store USD as checked integer ticks at `10^10` ticks per dollar. Accept provider decimal lexemes through checked decimal scaling, rounding to the nearest tick with ties to even; validate overflow after rounding. This handles observed provider floating-point residue without unchecked binary-float multiplication. Reject negatives, fractional token counts, overflow, non-finite numbers, and impossible known subset relationships.
- Cost provenance is `reported` or `estimated`; preserve a harness's pricing estimate label. Do not add an OVRCR model-price table or interpret subscription usage as invoice cost.
- Totals have scope: `conversation` or `invocation`. Preserve what the harness actually supplies. Do not combine Grok conversation-token totals with invocation-only cost under one unlabeled total; cost carries its own scope.
- Coverage is `complete` or `partial`. Auxiliary/subagent usage is included only when the provider's declared aggregate includes it. Absence of child activity is not evidence of absence of child cost.
- Prefer full replacement totals with a source revision. Where source records are deltas, deduplicate by stable request/message/entry ID in the collector and emit replacement totals. Duplicate final records and replayed status lines cannot increase totals.
- A missing field clears that field in a new full snapshot; it does not mean zero. Decreasing totals require a new binding/scope or an explicitly verified provider correction path. Do not silently take a maximum and conceal incorrect semantics.
- Keep live reporting in memory. Provider persistence can repopulate totals on explicit resume; OVRCR does not restore dead PTYs or add a reporting database.

## Shared API sketch

Implement concrete definitions in `crates/ovrcr-protocol/src/agent.rs` and re-export through `lib.rs`. Existing legacy activity/context variants remain accepted when there is no active v2 binding. While a v2 binding is active, reject legacy updates for that session to prevent two authorities.

```rust
pub enum AgentProvider { Claude, Codex, Grok, Pi, Hermes }
pub enum SampleQuality { Confirmed, Observed, Estimated }
pub enum UsageScope { Conversation, Invocation }
pub enum UsageCoverage { Complete, Partial }
pub enum CostKind { Reported, Estimated }
pub struct UsageTotals {
    pub scope: UsageScope,
    pub coverage: UsageCoverage,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
}
pub struct UsageCost {
    pub usd_ticks: u64,
    pub kind: CostKind,
    pub scope: UsageScope,
}
pub struct AgentBinding {
    pub provider: AgentProvider,
    pub invocation: String,
    pub conversation: String,
    pub generation: u64,
}
pub struct ActivitySample {
    pub state: crate::AgentActivity,
    pub quality: SampleQuality,
    pub turn: Option<String>,
}
pub struct ContextSample {
    pub used_tokens: Option<u64>,
    pub capacity_tokens: Option<u64>,
    pub quality: SampleQuality,
}
pub enum MeasurementFreshness { SourceIdentified, Uncertain }
pub struct Measurement<T> {
    pub value: T,
    pub source: String,
    pub source_revision: Option<String>,
    pub freshness: MeasurementFreshness,
}
pub struct MetricsSample {
    pub model: Option<String>,
    pub context: Measurement<ContextSample>,
    pub usage: Measurement<UsageTotals>,
    pub cost: Measurement<Option<UsageCost>>,
}
pub enum ReporterHealth { Connected, Unavailable }
pub struct HealthSample {
    pub state: ReporterHealth,
    pub reason: Option<String>,
}
pub enum AgentObservation {
    Activity(ActivitySample),
    Metrics(MetricsSample),
    Health(HealthSample),
}
pub struct ProviderReport {
    pub binding: AgentBinding,
    pub revision: u64,
    pub observation: AgentObservation,
}
```

Use separate revision watermarks for activity, metrics and health, since independent streams must not suppress each other. A `MetricsSample` is atomic: model/context/usage replace together, never half-apply. Store transport receipt time separately from per-component first receipt of a source measurement identity. Reusing the same component source/revision preserves its measurement age even in a new atomic sample. Unidentified source samples have uncertain freshness; arrival does not prove a new measurement. Reject conflicting values for the same identified revision; corrections require a new source revision. Unknown values explicitly clear the component. Stale is derived per measurement, not a health heartbeat. The outer `AgentReport` still authenticates OVRCR session/capability. Add `AgentUpdate::Provider(ProviderReport)` and the following supervisor protocol. Task 2 defines serialization, validation and redacted secrets alongside the stored snapshots:

- `ReserveAgent(expected_epoch, invocation, provider)` authenticates with the PTY capability and succeeds only at the expected epoch with no active lease. It advances a monotonically retained epoch and issues a distinct secret supervisor lease before native spawn. Concurrent/late reserve requests fail rather than taking over. A lost response is resolved by request-ID status lookup, not a new unconditional reservation.
- `BindAgent(lease, expected_binding, conversation)` requires the current lease and exact current binding (initially None). Duplicate request IDs are idempotent; a stale expected binding fails. Only the supervisor can issue it; hook children receive report-channel access, not the supervisor lease. Every change advances generation; same-conversation compaction does not bind again.
- `FinalizeAgent(lease, binding, final_metrics)` validates and publishes final metrics and release atomically, then acknowledges. Pending activity stops at closing; final accounting may run for at most two seconds outside the dispatcher. Known incomplete final sources or deadline failure yield partial coverage and an unavailable reason, not complete totals.
- `ReleaseAgent(lease, expected_binding)` only releases that owner, retains the advanced epoch, and marks unfinalized accounting partial. Old release/reserve/bind requests cannot affect a later owner.

The launcher supervises collector exit and sends health updates scoped to its lease/binding. A dedicated supervisor connection is watched by the runtime; its loss marks that binding unavailable and unfinalized accounting partial without changing agent activity. Ordinary short-lived report connections do not affect health. Late health/disconnect cleanup checks the current lease before mutation. Do not copy the supervisor secret into native child environment.

The native child must be reaped without ending the supervising PTY process until normal finalization is acknowledged or its deadline expires. SessionEnd is reason-qualified: clear or a conversation switch does not end the supervisor invocation. It closes the prior conversation and applies certified transition rules. Native completion, not SessionEnd alone, requests supervisor finalization; wait for the certified source completion boundary. Abrupt PTY death retains the last snapshot with partial coverage before capability revocation and accepts no later reports. Preserve final terminal output before exit notification.

Changing wire types requires a protocol-version bump and updating every constructor, exhaustive match, CLI projection, test fixture, and GUI caller. Preserve legacy JSON inspection fields as projections from the active authority, not parallel mutable state.

## Bounds and failure rules

- Existing hook input maximum: 65,536 bytes; whole helper deadline: one second. Provider callbacks remain fail-open and stdout-silent.
- At most 256 pending normalized collector records and 1 MiB queued normalized input per collector. Metrics can coalesce to the latest full sample. If lifecycle records cannot be preserved, disconnect reporting and expose unavailable state rather than silently reorder/drop them. Transcript records have a separate 32 MiB maximum, decoded one at a time; exceeding it makes coverage partial with a diagnostic, never silently complete. Load acceptance must measure the aggregate memory bound at 50 sessions.
- Keep provider subprocess and stream/file I/O off the server dispatcher. Reuse its bounded queue and sole socket writer.
- Exact-file readers use bounded incremental reads, retain incomplete trailing records, detect truncation/replacement, and never retain prompt/tool/response text after extraction. Unknown schema or ambiguous identity is an explicit diagnostic.
- Exact transcript accounting uses a non-evicting identity index with explicit capacity: 65,536 identities and 16 MiB charged retained keys/values per collector, whichever is reached first. Replace records for existing identities; never evict and then guess whether a record is new. At capacity, freeze the last exact prefix, mark coverage partial with an accounting-limit reason, and stop adding records. A rebuilt index has the same cap; only a certified authoritative aggregate can restore complete coverage beyond it. This deliberately limits exact transcript coverage, not terminal operation or supported conversation length. Stream file reads in at most 256 KiB steps, checking cancellation between steps; no whole-file raw buffer.
- Unbind, helper timeout, or cleanup from an old invocation cannot clear a replacement binding. Closing a pane/detaching the dashboard does not stop reporters or agents.
- Preserve existing provider configuration, approvals, hooks, status-line stdout, and extension lists. No global silent install, trust bypass, or sandbox relaxation.

## Completion contract

Every harness has a checked acceptance record covering root binding, two turns, permission/input wait where the harness has it, cancellation, terminal error, continuation, context refresh, token/cost comparison, resume/new/fork behavior, subagent isolation, detach/reattach, and owned cleanup. A provider legitimately missing a metric gets an evidence-backed unsupported entry; an unverified integration keeps the feature open.

The final release must pass the workspace tests/lint, protocol compatibility tests, mixed 50-session reporting load, and native macOS presentation checks. Linux acceptance is separate. Leave the feature open if required Linux or provider gates cannot run; do not replace them with synthetic fixtures.
