# Superset agent reporting source review

Reviewed 2026-09-09 at commit `1112b2feb1a0cfc59e8eb7133e073e8741c87f20` of `superset-sh/superset`. Source checkout: `/private/tmp/ovrcr-superset-hooks-20260909`. This is source inspection, not a live Superset test. No dependencies were installed and no user agent configuration was changed.

## How it works

1. Managed launch wrappers export root harness identity, preserving the first wrapper's identity for nested launches. A delayed launch report establishes attachment without implying work. [Wrapper implementation](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/agent-setup/src/agent-wrappers-common.ts)
2. Native hooks/extensions invoke a shared helper. It requires Superset terminal context, compares the originating hook harness with the root harness, recognizes child metadata, extracts conversation identity, and drops unknown events. It posts to the host service with bounded network deadlines. [Notify helper](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/agent-setup/templates/notify-hook.template.sh)
3. The host normalizes events and updates terminal bindings. Subagent events update a separate roster instead of parent lifecycle. Attachment preserves a prior working/stopped/failed state for the same conversation; ended bindings have a straggler window. This does not provide a general causal turn/generation protocol. [Router](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/trpc/router/notifications/notifications.ts), [store](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/terminal-agents/store.ts)
4. Historical usage is collected independently by scanning provider files and profiles. It is not populated by lifecycle hooks. Quota polling is another separate feature. [History collection](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/trpc/router/usage/history/entries.ts), [usage types](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/trpc/router/usage/types.ts)

## Coverage and consequences for OVRCR

| Harness | Superset implementation at this revision | OVRCR decision |
| --- | --- | --- |
| Claude Code | Managed native hooks. History parser deduplicates assistant usage by message ID plus request ID, retaining the last record so later content blocks can replace earlier totals. | Adopt identity-aware replacement accounting and config preservation. Separate current context from cumulative tokens. Verify cancellation and continued Stop behavior. |
| Codex CLI | Native hooks plus a best-effort TUI-log watcher. History reader sums `last_token_usage` while skipping consecutive identical usage objects. | Prefer authoritative same-thread cumulative totals when safely observable. Do not copy a heuristic that can discard two legitimate equal-cost requests. A versioned exact-thread reader is an alternative if passive app-server observation fails. |
| Grok CLI | Managed global hook file, including prompt/tool/Stop/failure events and permission notifications. Usage comes from inference events in a unified log joined to session summaries. | Installed Grok exposes richer status-line and persisted usage interfaces. Use those first. Add delayed `StopCancelled` and continued Stop cases absent from this wrapper. |
| Pi | Managed extension emits status hooks using `agent_end`. History parser reads assistant usage/cost. | Use the installed `agent_settled` event and UI-prompt hooks. Match current session accounting, including usage attached to tool results and compaction/branch summaries. |
| Hermes | Can be registered/launched, but no Hermes adapter was found in agent setup; usage types explicitly exclude it from history collection. | Implement independently. Prove an interactive context/full-accounting export; a launch preset is not reporting support. |

Provider source: [Claude/Codex setup](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/agent-setup/src/agent-wrappers-claude-codex-opencode.ts), [Codex wrapper](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/agent-setup/templates/codex-wrapper-exec.template.sh), [Grok setup](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/agent-setup/src/agent-wrappers-grok.ts), [Pi extension](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/agent-setup/templates/pi-extension.template.ts), [Claude/Codex parser](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/trpc/router/usage/history/parse.ts), [Grok parser](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/trpc/router/usage/history/grok.ts), [Pi parser](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/trpc/router/usage/history/pi.ts).

## Patterns to retain

- Root identity and hook-origin identity are different. Compatibility replay of Claude hooks by another harness must not impersonate Claude; nested agents must not change the parent binding.
- Installation owns named files/entries, merges around user settings, and can remove only its own changes. Hooks outside a managed terminal do nothing.
- Attachment is liveness, not busy/idle. Delayed attachment must not overwrite later progress.
- Usage is an independent data stream. Record identity and replacement semantics matter more than counting callbacks.
- Isolate failures per provider. Missing usage should not disable the terminal or another provider's reporting.

## Patterns not to carry over

- The Codex wrapper bypasses hook trust and enables hooks through launch flags. OVRCR must preserve user trust/approval choices and diagnose incomplete setup.
- Generic `Notification` and `PreToolUse` can normalize to permission-wait in Superset's [event mapper](https://github.com/superset-sh/superset/blob/1112b2feb1a0cfc59e8eb7133e073e8741c87f20/packages/host-service/src/events/map-event-type.ts). OVRCR requires an actual human-input signal.
- Receipt timestamps, delayed launch sleeps, and an ended-session grace window cannot reject every stale turn. Use explicit binding generations and provider causality where available.
- Regex JSON extraction, detached per-event delivery without ordering, and consecutive-equal-usage heuristics are not suitable for exact OVRCR accounting.
- Superset's global history scan, host-manifest discovery, cloud task updates, quota polling, persistence, and pricing catalog are outside this feature. Reuse OVRCR's authenticated Unix transport and memory-only runtime.
- A parser accepting a schema is not proof of current live coverage. Superset's Pi events and Grok usage route differ from the installed harness interfaces examined for this plan.

## Effect on the implementation plan

Keep small native adapters and separate status/accounting paths. Add explicit tests for delayed attachment, cross-harness hook replay, parent/child isolation, last-record replacement, and equal consecutive requests. Do not require a helper process for a harness whose native extension can report directly. Use a launcher only to establish invocation ownership or supervise a necessary collector.

The [design](../plans/2026-09-09-multi-harness-reporting-design.md) and [implementation plan](../plans/2026-09-09-multi-harness-reporting.md) incorporate these findings. All provider acceptance remains open.
