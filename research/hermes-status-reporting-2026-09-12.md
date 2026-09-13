# Hermes status reporting: research and pre-grill draft

**Status:** Research-backed discussion draft. Not an approved Hermes specification, implementation plan, or ticket. No application or upstream implementation is authorized. Use this file as input to the grilling/domain-modeling workflow before publishing a Hermes spec.

**Question:** Can native interactive Hermes report Busy, Ready/Unread, real human-input waits, and optional attention alerts through OVRCR, with the same product semantics discussed for Pi/OMP?

**Short answer:** A Python plugin is a viable integration vehicle, and the classic REPL is the simplest process boundary. Full support is not a drop-in hook mapping: terminal hook coverage, actual question/approval visibility, modern-TUI process ownership, and startup/recovery metadata need explicit contracts. The old Hermes accounting-export requirement should not automatically block a status-only feature.

## How to use this brief

- Treat the existing OVRCR glossary as authoritative for Ready, Response cycle, Input request, Unread and Presented.
- The Pi/OMP choices below are **proposed carryovers for Hermes**, not decisions silently made on the user's behalf.
- Start the interview with the independent decisions in **Decision tree for grilling**. Source facts are supplied here so the interview need not ask the user to discover APIs or file locations.
- After agreement, produce a product specification and then vertical tickets. Do not execute the old multi-harness Hermes task as though all its assumptions were certified.

## Evidence baseline

| Item | Observed result | Evidence class |
| --- | --- | --- |
| OVRCR checkout | `feature/pi-integration`, base revision `b028d3592c38b8f672cdd7f8147daf905b6953eb`; existing Pi/OMP planning changes preserved | Local checkout inspection |
| Installed Hermes | `Hermes Agent v0.20.5 (2026.8.19)`, Python 3.11.15 | Actual `hermes --version` |
| Installed Hermes source | Clean checkout at `5ef1409f50484dddc38c9665b32a837ff1b191af` | Actual Git revision/status |
| Native launch choices | Classic `--cli`, modern `--tui`, profile/resume controls; one-shot usage report is explicitly one-shot-only | Actual `hermes --help` |
| Latest upstream spot-check | `b6b53c69a6ed49cb099cf1bfe76b5e6edd718e5a`; only the plugin hook registry was inspected at this newer revision | Pinned remote source, not a full current-upstream audit |
| Plugin mechanics | Opt-in loading, synthetic hook dispatch, direct helper parent, and unload removal passed in an isolated profile | Executed smoke experiment, **not native agent acceptance** |

The installed command reported that an update was available. Nothing was updated. Most source conclusions below describe the installed clean revision, not all Hermes versions. The current public documentation is useful context but can differ from the installed code.

### What was actually exercised

A disposable plugin registered `pre_llm_call` and `on_unload` through Hermes' actual public plugin manager. Each run used a fresh Python process, a task-owned Hermes profile and home, an empty bundled-plugin directory, no inherited credentials, and no model request. The hook received synthetic session/turn IDs and spawned a short Python child to inspect its parent process.

| Run | Exit | Observations |
| --- | --- | --- |
| Plugin discovered but absent from enabled list | 0 | No callback or unload action from that plugin |
| Plugin explicitly enabled | 0 | Exactly one callback with `synthetic-root` / `synthetic-turn`; helper parent matched the Python host; unload callback ran once |
| Dispatch after unload in the enabled run | Included above | No second callback |

Both runs had empty stderr. The temporary profile, plugin and helper resources were removed. This proves the chosen plugin loading/dispatch/cleanup mechanism and Python child ancestry in that fixture. It does **not** prove native conversation events, root admission, Dashboard reporting, human-visible waits, or modern-TUI ownership.

No authenticated Hermes conversation, native UI acceptance, live configuration edit, plugin installation, model upgrade, GitHub issue creation, or application-code change occurred in this research pass.

## Source findings

All installed-source links below pin the clean revision. `[INFERENCE]` marks a conclusion derived from source structure rather than a native end-to-end run.

### 1. A Python plugin is a better default than raw shell-hook forwarding

Hermes plugins have a manifest and a `register(ctx)` entry point. Public hooks are registered through `ctx.register_hook`; cleanup can be registered through `ctx.on_unload`. Forced rediscovery unloads registrations before rebuilding them. The isolated smoke confirmed these mechanics. [H1]

Lifecycle `invoke_hook` callbacks run synchronously in the calling thread in the inspected release. The inter-plugin `emit/subscribe` bus is a separate queued mechanism; accepting an arbitrary `hermes:*` subscription does not establish a native lifecycle event source. Keep callback work bounded and capture sequence/identity before asynchronous delivery. [H1]

Shell hooks use the same native hook sites, require first-use consent, and spawn commands without a shell by default. However, their generic payload includes full hook kwargs under `extra`, including user messages/history or command content on relevant hooks. Forwarding that envelope unchanged would violate the content-free reporting design and can exceed OVRCR's 65,536-byte limit. A Python plugin can extract only the permitted fields **before** invoking the OVRCR helper. [H2]

Recommendation to test during grilling: a small, inert-outside-OVRCR Python observer plugin, not a generic event serializer, monkeypatch, presentation replacement, or agent SDK wrapper.

### 2. Installation is not Pi-style per-launch extension injection

General Hermes plugins are disabled until explicitly enabled in the selected profile. Discovery supports bundled, user, opt-in project, and installed entry-point sources. The user-level plugin directory follows Hermes home/profile selection. A deny-list overrides enablement. The inspected help and discovery path do not provide a Pi-style per-launch extra-extension flag. [H1], [H3]

Project-plugin discovery itself requires an explicit opt-in. Replacing Hermes home or the bundled-plugin root to sneak in OVRCR would hide or change the user's normal setup; the isolated smoke used those controls only to avoid loading user resources during research.

The least disruptive current-source option is an explicit one-time install/enable in the chosen profile, with setup instructions showing the owned change and the plugin inert outside managed invocations. Requiring zero persistent setup would instead need a supported invocation-scoped loading facility. This is a user decision, not permission to modify configuration now.

### 3. Classic REPL and modern TUI have different process owners

The installed `hermes` shell launcher uses `exec` to enter Python. In classic mode, the interactive CLI invokes the agent on a worker thread in that Python process. A plugin-spawned helper is therefore a direct child of the Python host; the smoke verified that relationship for the plugin mechanism. [H4]

Modern `--tui` is different. Despite a launcher docstring describing replacement, the actual implementation uses `subprocess.call` to launch Node. The Node client then spawns a Python `tui_gateway.entry` backend, or attaches to an existing gateway. Optional compute-host isolation adds another process for turn execution. [H4], [H5], [H17]

The ordinary spawned path is:

```text
OVRCR supervisor
  Hermes Python launcher (native anchor)
    Node native TUI
      Python TUI backend
        optional isolated compute host
          plugin-spawned reporting helper
```

Without compute isolation, the helper is a child of the Python backend instead. OVRCR's current check requires the helper peer's parent to equal the native anchor. **[INFERENCE] The modern backend helper does not satisfy that check; reusing classic admission unchanged would reject it.** Environment inheritance or membership in the process tree does not establish which backend owns the foreground conversation. [O1]

Do not solve this by accepting arbitrary descendants or silently forcing `--cli`. Modern-TUI support needs an explicit authenticated ownership/delegation or native forwarding contract, including the attached-gateway case. Whether modern TUI is required initially belongs at the first interview frontier.

### 4. Hook names do not correspond to process start and stop

| Native event | Verified installed-source meaning | Candidate use and limit |
| --- | --- | --- |
| `on_session_start` | First system-prompt build on the first entered turn; skipped on some restore/continuation paths | Initialization evidence, not guaranteed launch-time root attachment |
| `pre_llm_call` | Once per entered Hermes turn after restoration and preflight work | Candidate Busy/turn admission; not the instant the user submitted input |
| `pre_api_request` | Per provider attempt | Continue Busy; not a response-cycle start |
| `post_api_request` | After normalized provider response, before finish/tool/retry decisions | Never Ready on its own |
| `api_request_error` | A failed provider attempt with classification/retry metadata | Not necessarily terminal Error; retry/fallback may follow |
| `post_llm_call` | Normal finalizer with truthy response and no interruption, after text transforms | Incomplete Ready source: no `completed`/`failed` fields and not emitted on every path |
| `on_session_end` | Normally the end of one `run_conversation` call, with terminal flags | Turn-ending evidence, **not process shutdown**; delivery is not universal |
| `on_session_finalize` | True session teardown observer | Invalidate the old binding; never manufacture Ready |
| `on_session_reset` | A boundary event whose timing differs by frontend; also used during modern backend initialization | Interpret reason/source and old/new identity; do not equate every event with user `/new` |

Primary locations: turn setup [H6], first-build start emission [H9], normal finalizer [H7], and native frontend lifecycle [H5], [H8].

`pre_llm_call` contains session/task/turn IDs, platform, parent-session ID, and first-turn information, alongside private content that must be discarded. The normal `on_session_end` includes `completed`, `failed`, `interrupted`, and `turn_exit_reason`, plus session/task/turn/model/platform. Interpret all terminal flags together: `completed` is computed independently of the interruption flag. [H6], [H7]

### 5. Normal finalization is not a universal terminal-event funnel

Representative direct returns **after turn entry** bypass the normal finalizer: content-policy/billing/non-retryable failures, max-retry exhaustion, and interruption during retry backoff. The native Codex app-server backend also follows a separate result-return path rather than the normal finalizer. [H9], [H18]

**[INFERENCE] A plugin that sets Busy at `pre_llm_call` and waits exclusively for `on_session_end` can remain stuck Busy.** Mapping `api_request_error` directly to Error would instead incorrectly terminate recoverable retries. No timer fixes this source-contract gap honestly.

The outer agent wrapper already closes its built-in Relay/task accounting for returned results and exceptions. That is a useful owner at which to investigate a supported public terminal-result export; it is not automatically a public plugin event and must not be reached through private coordinator state. [H10]

For a full status contract, investigate a public outer-turn result/failure/cancellation observer that covers all exits and supported native backends. Keep returned-result availability distinct from actual native presentation and from guaranteed final settling. Recommend Observed Ready unless the chosen source proves something stronger. A hooks-only experiment can demonstrate partial capability, but should not be advertised as complete status support.

### 6. Root identity needs more than session ID or `platform="cli"`

Delegated agents have explicit subagent/parent identity, but background-review forks are subtler: they inherit the parent's platform and deliberately share its session ID while using a parent-session marker. They can run in the same process on another thread. [H11]

A useful candidate for classic admission is a parentless `pre_llm_call` from the owned native invocation, followed by accepting only its admitted opaque turn ID. Turn IDs are generated from session/task identity plus a UUID fallback. This is stronger than accepting every event marked CLI or every event with the parent session ID. It still needs native verification for resumed/forked roots, compression-induced session rotation, background review, and concurrent callbacks. [H6], [H11]

Not every later callback repeats lineage fields. Carry established root/turn admission forward; do not reclassify each approval or end callback independently. Admission on the first trustworthy prompt may be achievable without a new root-marker API. Immediate startup binding and reconstructing foreground identity after a plugin reload are separate requirements and currently lack a simple public plugin snapshot.

### 7. Approval hooks indicate a decision request, not confirmed visibility

Built-in approval hooks expose command/pattern metadata, session routing key, surface and response choice. Context variables add turn/tool-call IDs and, when present, the Hermes session ID. The plugin must omit all command/description content from reporting. [H12]

Important differences:

- Classic `surface="cli"` pre-hook fires **before** the approval callback takes its serialization lock, creates modal state and schedules paint. Multiple pre-hooks can precede the one currently visible panel. [H12], [H13]
- Modern native TUI approvals use the **gateway** queue and therefore `surface="gateway"`. Filtering only `cli` would miss native TUI waits; accepting all gateway requests would admit unrelated messaging sessions. [H5], [H12]
- Smart auxiliary-LLM approval emits `surface="smart"` events despite having no human wait. An escalation need not have a matching smart post-event. Other automatic policy/allowlist paths can skip hooks entirely. [H12]
- Built-in CLI/gateway approval hooks omit the native queue request ID, even though the gateway queue has one. Session/tool IDs alone do not settle coalescing, repeated presentations, or reconnect identity. [H12]
- A selected plugin approval transport has stronger request identity, but it **takes over presentation**. It is not the passive observer needed here. Do not change approval transport merely to gain telemetry. [H14]

Recommendation: expose identity-qualified queued/presented/resolved facts from the native owner, with OVRCR alerting only at the agreed human-attention boundary. Backend event emission is not automatically proof the modern frontend displayed the request; define and verify that boundary rather than relabel it.

### 8. Clarification is a real native tool but has no corresponding plugin visibility hook

Hermes' `clarify` tool supports single and multi-question forms. Classic callbacks create modal state and wait for answers; the modern backend maintains request IDs and emits native `clarify.request` / expiry events. Native clients have pending-request data for reconnect. These are not currently a general plugin-facing question visibility observer/snapshot. [H5], [H13], [H15]

The inspected plugin hook registry has no clarification-open/close events. A generic `pre_tool_call` would describe tool execution rather than visible presentation, and forwarding tool content is unnecessary. `human_wait_window` / `human_wait_seconds` are approval deadline-accounting helpers, not a question observer or request-identity API; they do not cover clarify. [H1], [H12], [H15]

The newer upstream registry spot-check still has no general clarification/presented-input hook. It adds, among other things, gateway-specific `agent_loop_stopped` and hosted-room member activity; neither establishes a universal interactive root lifecycle or clarification observer. This was a registry check, not a complete audit of all newer plugin APIs. [H16]

For exact Input-needed behavior comparable to the proposed OMP export, expect a supported observer at clarification/approval presentation and resolution. Keep it content-free and minimal: request identity, owning session/turn/root context, kind, phase and outcome. Do not add a command digest, content hash, persistent analytics store, or full private agent snapshot just for status reporting.

### 9. Reload cleanup exists; restoring current activity is a separate gap

Public plugin unload callbacks and tracked registration disposal are present, and the smoke verified callback removal. Forced rediscovery unloads/reloads plugins and restores configured shell hooks. That helps cleanly retire an observer, but does not give the replacement a public current root/session/activity/Input-request snapshot. [H1]

A helper channel can reconnect while an already-loaded plugin retains trusted state. A factory reload, native session switch, or backend replacement can lose that state and requires new admission/current-state evidence. Do not replay historical responses as Ready or treat old visible requests as fresh alerts.

For later design, distinguish three recoveries: transport-only reconnect; plugin reload with lost local state; modern backend replacement. A bounded public metadata snapshot and a supervisor-authorized generation handoff may be needed for the latter two. Showing Unknown until the next trustworthy event is preferable to inventing Ready/Idle, but whether that satisfies the Hermes product requirement is a decision for grilling.

## OVRCR reuse and implementation boundaries

**Already implemented at the inspected baseline:** authenticated invocation supervision, direct-child callback attribution, binding generations/receipts, runtime activity/health, Codex Ready/Unread/Presented review, optional Ready sound/desktop alerts, and the Dashboard launch picker. `AgentProvider::Hermes` already exists, but that does not make an adapter or its Ready eligibility implemented. [O1], [O2]

**Specified/ticketed, not assumed implemented:** provider-neutral readiness extensions, Pi/OMP plugin delivery, separate Input-request snapshots/alerts, optimistic version diagnostics, and producer reattachment from [spec #88](https://github.com/xlyk/ovrcr/issues/88) and its [tickets #89–#97](https://github.com/xlyk/ovrcr/issues/89). Reuse whichever shared behavior has actually landed when Hermes execution begins. Hermes must not acquire an artificial dependency on OMP's upstream question export.

The [deep-modules refactor #77](https://github.com/xlyk/ovrcr/issues/77) also changes readiness ownership, request transport, Dashboard internals and test seams. Refresh the implementation map after that work merges. This brief deliberately does not prescribe file-level edits against moving interfaces.

The future Hermes launch slice must own **both** picker discovery and actual managed launch routing. A listed executable or hand-typed `agent run` example alone does not prove the Dashboard produces a tracked session. Profile consent and frontend selection are additional Hermes prerequisites, not reasons to silently alter the user's configuration.

The older [multi-harness Hermes task](../plans/2026-09-09-multi-harness-reporting.md#task-8-complete-hermes-integration-and-required-export) includes context, token/cost, auxiliary accounting and a full snapshot export. Those requirements remain historical broader scope. A status-only Hermes spec should explicitly decide whether to leave them out rather than accidentally make accounting a prerequisite for Ready.

## Candidate product specification

This section is a starting draft for discussion, not a confirmed contract. It intentionally avoids implementation file lists and steps.

### Problem Statement

A user running Hermes inside OVRCR cannot reliably tell when the native agent has produced a new response or needs a human answer without inspecting the terminal. Existing generic hook names do not cover every terminal path, and Hermes' classic and modern interfaces do not share one simple reporting-process boundary.

### Solution

Integrate Hermes' native interactive workflow with OVRCR status, explicit Unread review and optional attention alerts. Preserve the chosen native interface, configuration, profiles and approval behavior. Use public observer contracts and authenticated root/session identity; expose unsupported capabilities honestly rather than infer completion from silence or question visibility from tool execution.

Prefer a status-first integration and an explicitly enabled content-free plugin. Separate classic and modern interface acceptance. Add supported native exports only for the lifecycle/input/current-state gaps required by the agreed product contract. Reuse the accepted OVRCR reporting and review mechanisms once their current implementation is established.

### User Stories

1. As a Hermes user, I want a managed native session to show Busy and Ready, so that I do not continually watch its terminal.
2. As a user, I want Ready to mean a response is available to review, so that I do not mistake it for task success or guaranteed finality.
3. As a user, I want known retries and internal tool turns not to create repeated Unread responses, so that only meaningful response cycles need review.
4. As a user, I want failure and interruption distinguished from Ready, so that unsuccessful work does not produce a completion alert.
5. As a user, I want explicit review to target the response I saw, so that a delayed action cannot clear a newer response.
6. As a user, I want background child/review work excluded from root status, so that an auxiliary session cannot falsely finish the parent.
7. As a user, I want actual visible approvals and clarification questions to show WaitingInput, so that I know when my answer is needed.
8. As a user, I want automatic smart approval excluded from human waiting, so that machine decisions do not demand my attention.
9. As a user, I want optional Input needed and Response ready alerts with separate identities, so that questions do not become Unread responses or replay after reconnect.
10. As a user, I want unanswered, resolved, timed-out and cancelled requests correlated correctly, so that old requests cannot clear newer waits.
11. As a user, I want the Dashboard's Hermes action to launch a tracked session, so that managed launching works without manually typing a wrapper.
12. As a user, I want profile/plugin consent explicit and existing setup preserved, so that reporting does not silently change my Hermes installation.
13. As a user, I want my selected classic or modern native interface preserved, so that enabling reporting does not switch interfaces behind my back.
14. As a user, I want session changes and reporter recovery to preserve safe current state without replaying history, so that a connection problem does not force unnecessary agent restart.
15. As a user, I want ordinary compatible upgrades to remain usable, so that an untested version string alone does not disable reporting.
16. As a user, I want doctor to distinguish loaded plugins, observed lifecycle, known missing exports, frontend incompatibility and transport failure, so that I can identify the actual limitation.
17. As a user, I want private prompts, responses, commands, questions and credentials kept out of reporting, so that status integration does not expose conversation data.
18. As a user, I want existing Claude/Codex/Pi/OMP and scheduled-task behavior preserved, so that adding Hermes does not regress other work.
19. As a user managing fifty sessions, I want bounded reporting with responsive control actions, so that concurrent activity does not overwhelm the synchronous server.
20. As a user, I want absent accounting to remain unknown, so that this status feature does not fabricate token or cost totals.

### Implementation Decisions — proposed defaults, awaiting grilling

- Reuse existing domain semantics and optional notification preferences; keep process phase, reporting health, activity, Unread and Input requests separate.
- Use a Python plugin that strips content before spawning a bounded OVRCR reporting helper. Keep only root/session/turn/request identity, source ordering, outcome and capability metadata.
- Recommend one-time explicit plugin enablement in the intended existing profile, followed by managed launches. Do not fake Pi-style temporary loading by replacing the user's Hermes home or bypassing consent.
- Recommend a classic-REPL first acceptance slice **only if the user agrees that modern TUI can follow independently**. Do not advertise modern support until its backend ownership and actual presentation contracts are solved. Do not override the user's selected interface implicitly.
- Prefer Observed Ready on a supported outer terminal/result boundary. Fill missing terminal paths with a native observer export, not timer-based reconciliation or private Relay access. Native Codex and other separately executed backends need their own coverage check.
- Prefer true presentation-qualified Input-request events for approval and clarify, with stable logical request IDs and terminal outcomes. Do not take over approval transport for observability or use tool lifetime as a hidden approximation.
- Admit root turns through explicit owned-invocation and session/turn evidence. Parentless pre-turn admission is a candidate for classic mode; verify resumed roots and shared-session background reviews before relying on it.
- Support ordinary upgrades through capability/payload checks and runtime delivery evidence, not exact-version gating. Successful registration of a name is not evidence that its event fires.
- Distinguish channel reconnect, plugin factory reload and modern backend replacement. Retain trusted state only for the producer that owns it; recover through explicit generation handoff/current-state evidence and baseline existing requests without alert replay.
- Keep token/cost/context accounting outside the first status milestone unless the interview explicitly expands scope. Required status exports should not become a generic full-agent telemetry API.

### Testing Decisions — proposed seams

1. **Primary OVRCR seam:** the real managed CLI/Dashboard launch under an isolated OVRCR server, PTY, native Hermes process, reporting helper and authenticated socket. Assert visible snapshots, exact review targets, input request closure, output/exit preservation, old-event rejection and owned cleanup.
2. **Native provider seam:** actual terminal/result and approval/clarify presentation owners. An injected callback fixture cannot prove that all errors emit terminal events or that a queued question was displayed. Exercise both the classic and modern frontend paths that are claimed supported.
3. Reuse existing host-notification recording for deterministic identity/queue tests, then actual native sound/desktop delivery where claimed. Baseline reconnect without replay and cancel alerts for resolved requests.
4. Cover preflight failure, normal response, retry exhaustion, interruption during backoff, no-output/partial results, verification continuation, native Codex backend, background review sharing a session ID, delegated children, new/resume/branch, plugin reload and current-state recovery. This is a bounded matrix of identified source risks, not an open-ended provider benchmark.
5. Preserve actual native configuration/approval behavior and leave unsupported modes unchanged. Verify the selected frontend is the one launched; do not obtain green results by forcing classic mode or enabling automatic approvals.
6. Use source-derived sanitized fixtures for deterministic parsing/order tests, followed by native interactive acceptance with unique config/socket/workspace, fresh screenshots/accessibility, exact versions/revisions and PID/PGID ownership. Separate macOS, Linux, classic, modern, helper smoke and native conversation evidence.
7. Reuse the existing fifty-session socket/load and current-revision regression gates. Re-evaluate file/fixture ownership after the ongoing OVRCR refactor; no permanent test or runtime implementation is part of this research document.

### Out of Scope — proposed

- Gateway messaging, cron, kanban workers, desktop/web sessions, ACP and attached remote backends unless explicitly admitted as distinct product surfaces.
- Token/context/cost accounting, quotas, billing and historical analytics.
- Guaranteed task success from Ready, arbitrary text/silence inference, private-agent introspection, presentation takeover, monkeypatching or broad descendant authentication.
- Silent plugin installation, consent bypass, forced frontend changes, global configuration replacement, model/harness upgrade, or a new service/database/SDK.
- Publishing a final spec/tickets, creating upstream issues, coding, merging or installing this integration before the interview resolves the scope.

### Further Notes

The likely upstream work is **not merely the old accounting snapshot export**. For the requested status feature, the relevant candidates are complete terminal-result coverage, identity-qualified input presentation/resolution, safe current-state reconstruction, and modern backend ownership/forwarding. Some may combine at an existing native owner; do not build a general observability framework just because several fields are missing.

The installed plugin mechanism is proven by the bounded smoke. Full Hermes status support is not yet proven. Research suggests the classic path is the lower-complexity starting point, while the modern path requires an explicit architecture decision. Recheck upstream source when execution begins; the newer registry spot-check does not certify all current implementations.

## Decision tree for grilling

The following are decisions, not questions for the user to answer in this research handoff. Ask the independent frontier together when the grilling skill begins; defer dependent decisions until their prerequisites are settled.

| ID | Decision ready to ask | Recommended starting answer | What it unlocks |
| --- | --- | --- | --- |
| D1 | Which native interface must the first delivery cover: classic REPL, modern TUI, or both? | Classic can ship first, with modern separately required only if the user wants it; never silently change the selected frontend | Modern backend ownership/forwarding and per-interface acceptance |
| D2 | Is explicit one-time plugin install/enable in the chosen profile acceptable? | Yes; preserve profile state and keep the observer inert outside OVRCR | Installation/removal/distribution design, or an upstream invocation-loading requirement |
| D3 | Does the first milestone need complete failure/interruption/native-backend status, or is a clearly limited successful-response experiment acceptable? | Complete status for admitted surfaces; require a supported outer result observer to close known holes | Native terminal export boundary and backend acceptance matrix |
| D4 | Must Input needed mean an actually visible human question/approval rather than a decision request being prepared? | Yes, consistent with the Pi/OMP discussion; include native clarify as well as approvals | Presentation export, request identity and modern frontend acknowledgement boundary |
| D5 | Is this status/review/attention only, leaving metrics out? | Yes; do not inherit the broader accounting project by accident | A bounded completion definition |
| D6 | May shared Pi/OMP reporting changes land first while Hermes source/export work proceeds independently? | Yes; reuse the landed shared owner and refresh after the deep-modules refactor | Scheduling without an artificial dependency on OMP's question export |

Dependent decisions for later rounds:

- **After D1:** For modern TUI, is only a locally spawned backend required, or also attached/isolated backends? What explicit ownership/forwarding contract is acceptable? A broad PID-descendant allowlist is not a safe default.
- **After D2:** Which existing profile is the scope of consent, how is the owned plugin distributed, and does the user need per-launch installation-free loading?
- **After D3:** Is first-prompt admission enough, or must reporting attach before any input? How should no-output and partial terminal results map without confusing cancellation/failure with Ready? Do all native inference backends need first-release support?
- **After D4:** Which native owner proves presentation, how is one logical multi-question/approval request identified, and how are cancelled-before-display requests excluded? Modern backend emission alone is not a visible-frame acknowledgement.
- **After D1/D3/D4:** Must recovery work immediately after a plugin/backend replacement, or can Unknown remain until a fresh trustworthy event? Decide what a supported current-state snapshot must contain before designing recovery.
- **Before final confirmation:** Check the proposed inherited Ready/Unread/Presented, failure, upgrade and notification semantics against the Hermes choices. Record resolved terminology only then; do not invent parallel states such as Working or Closed in this status design.

## Primary source references

[H1]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/plugins.py
[H2]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/agent/shell_hooks.py
[H3]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/website/docs/user-guide/features/plugins.md
[H4]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/main.py
[H5]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/tui_gateway/server.py
[H6]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/agent/turn_context.py
[H7]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/agent/turn_finalizer.py
[H8]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/lifecycle.py
[H9]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/agent/conversation_loop.py
[H10]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/run_agent.py
[H11]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/agent/background_review.py
[H12]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/tools/approval.py
[H13]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/cli.py
[H14]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/approval_transport.py
[H15]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/tools/clarify_tool.py
[H16]: https://github.com/NousResearch/hermes-agent/blob/b6b53c69a6ed49cb099cf1bfe76b5e6edd718e5a/hermes_cli/plugins.py#L108-L200
[H17]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/ui-tui/src/gatewayClient.ts#L476-L487
[H18]: https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/agent/codex_runtime.py#L677-L958
[O1]: ../crates/ovrcr-runtime/src/agent_runner.rs
[O2]: ../crates/ovrcr-protocol/src/agent.rs

Useful installed-source anchors for the next research pass:

- Plugin opt-in/discovery: `hermes_cli/plugins.py:591–618, 4192–4241`; register/unload/force reload: `3130–3153, 1643–1678, 3877–3921`; synchronous dispatch: `hermes_cli/lifecycle.py:11–22`; inter-plugin bus is separate: `plugins.py:3232–3306`.
- Classic/modern selection and native Node launch: `hermes_cli/main.py:2685–2862, 2906–2918, 3207–3227`; modern Python backend spawn/attach: `ui-tui/src/gatewayClient.ts:476–487, 657–686`.
- Turn ID and root-parent candidate: `agent/turn_context.py:579–588, 1274–1290`; first-build start: `agent/conversation_loop.py:1043–1056`.
- Terminal flags/end: `agent/turn_finalizer.py:226–235, 616–635, 814–837`; representative bypass: `agent/conversation_loop.py:6310–6344, 6548–6570`; native Codex path: `agent/codex_runtime.py:677–958`; outer returned-result/exception owner: `run_agent.py:8935–8979`.
- Shared-session background review: `agent/background_review.py:1228–1305`; normal delegated children: `tools/delegate_tool.py:1965–2091`.
- Approval context/smart filtering: `tools/approval.py:108–185`; classic pre/callback/post: `5111–5137`; gateway queue/notify and omitted hook request ID: `4284–4544`; private timing accounting: `2548–2679`.
- Actual classic dialog owners: `cli.py:15552–15960`; modern pending/dialog bridge: `tui_gateway/server.py:2308–2355, 4031–4170`; exact native response handling: `tui_gateway/methods_prompt.py:1490–1523, 1658–1714`.

Current public guides: [Plugins](https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins/), [Hooks](https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks/). Prefer pinned source for exact behavior; these pages evolve independently of the installed release.
