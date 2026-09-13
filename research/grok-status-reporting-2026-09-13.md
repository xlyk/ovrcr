# Grok status reporting: research, Superset comparison, and pre-grill draft

**Status:** Research-backed discussion draft for the grilling/domain-modeling workflow. This is not an approved Grok specification or implementation plan. No tickets, application changes, native configuration changes, or upstream implementation are authorized by this document.

**Goal:** Prepare a Grok integration specification comparable to the Pi/OMP status feature: native interactive launching, Busy/Ready/Unread, actual human-input requests, optional attention alerts, identity-safe ordering and recovery. Keep accounting separate unless the user explicitly expands scope.

**Bottom line:** Superset supplies a useful working implementation reference, especially for native hook packaging and the `permission_prompt` / `elicitation_dialog` attention signals. It does not supply OVRCR's required prompt correlation, Grok-specific child filtering, exact request lifecycle, or authenticated shared-leader ownership. Grok already has richer native pending-interaction and turn-result paths beyond its shell-hook interface; inspect their safe accessibility before inventing another upstream event API.

## How to use this brief

- Begin with the **Decision tree for grilling**. Facts, limitations, candidate mechanisms and test evidence are already separated below.
- Use the OVRCR glossary: Ready, Response cycle, Input request, Unread and Presented. Do not introduce competing labels such as Working or treat a notification as review.
- Pi/OMP decisions are proposed carryovers for Grok, not silent approvals. In particular, decide the attention boundary and shared-leader scope before fixing the implementation route.
- After the interview, synthesize a product spec and vertical tickets. The old multi-harness Grok task is background, not evidence of completed implementation.

## Evidence and revision boundaries

| Evidence | Revision / result | What it establishes |
| --- | --- | --- |
| Installed executable | `grok 1.0.30 (04b7ffed98c6) [stable]` | Actual version and CLI behavior observed in this pass |
| Executable target | `grok-1.0.30-macos-aarch64` | Installed native binary, not a local source checkout |
| Installed guides | Grok-extracted user guides read during this investigation | Local first-party documentation; not an immutable source checkout or proof of runtime behavior |
| Public Grok source | `37949780c144e37df692e3d669051a21fec24f20`, synced 2026-09-09 | Exact implementation source for this public snapshot, **not a verified match to the installed binary** |
| Binary/source mismatch | Public GitHub API could not resolve build hash `04b7ffed98c6` | Do not label the public snapshot as installed 1.0.30 source |
| Current Superset source | `c3c6717bb7de1ce91dbf82505e9dce12c2c0324e`, read 2026-09-13 | Fresh comparison, distinct from the older source review at `1112b2f` |
| OVRCR baseline | `b028d3592c38b8f672cdd7f8147daf905b6953eb`, `feature/pi-integration` | Existing Claude/Codex reporting and planned Pi/OMP interfaces; unrelated planning work preserved |

Notation: **[D]** installed documentation; **[P]** pinned public Grok implementation; **[S]** pinned Superset implementation; **[R]** executed isolated experiment; **[INFERENCE]** a consequence of those facts that has not been demonstrated in a native end-to-end session.

The prior multi-harness plan inspected Grok 1.0.24 at another build hash. This investigation did not downgrade or upgrade Grok. Its evidence must not be silently merged with the older version or treated as an operational exact-version allowlist.

## Executed checks and limits

### Native Grok configuration inspection

Ran the installed binary's version/help, inspection help and plugin-validation help. Then ran `grok inspect --json` against task-owned HOME/GROK_HOME/workspace directories, with no inherited credentials and a fixture configuration disabling auto-update and shared-leader use. No native conversation or model request was submitted.

| Fixture | Exit | Verified result |
| --- | --- | --- |
| One existing user hook | 0 | Native inspection discovered the existing Stop hook |
| Existing user hook + owned reporting hook + Claude-compatible hook | 0 | All three fixture hooks remained discoverable |
| Same sources plus attempted hook injection through `GROK_CONFIG` | 0 | Original three remained; overlay hook was not loaded |

Every inspection had empty stderr. This proves additive discovery for the tested files and the overlay restriction in the installed binary. It does **not** prove hooks executed, native session identity, leader environment propagation, or any Ready/input transition.

### Superset adapter experiment

Executed the actual pinned notify-hook shell template after substituting its normal template markers. Used synthetic payloads and a task-owned `curl` recorder that returned an accepted response without making a network request. Separately executed the actual pure host event mapper with Bun. No Superset installation or host service was used.

| Synthetic input | Actual helper/mapper result |
| --- | --- |
| Root Grok `UserPromptSubmit` | Outbound Start for the root session |
| `notification` / `elicitation_dialog` | Outbound PermissionRequest |
| Grok child `UserPromptSubmit` carrying `subagentType` | Outbound **root-shaped** Start naming the child session; no subagent envelope |
| `StopCancelled` | Helper forwards the name, but the host mapper returns null; managed Grok config does not register it either |
| Stop with `stopHookActive=true` | Ordinary Stop; continuation distinction and prompt ID are not forwarded |
| Claude-origin hook under a Grok root wrapper identity | Dropped before transport |

All six helper runs exited 0 with empty stdout/stderr. The forwarded payloads omitted `promptId`. These are executed adapter semantics with synthetic input, not proof that a particular live Grok/Superset session exhibits each case. Source boundaries and the absence of actual backend/notification UI acceptance remain explicit.

## Native Grok findings

### 1. Grok plugins package hooks; they are not Pi-style runtime extensions

**[D]** Grok plugins bundle hook files, skills, commands, agents and MCP/LSP configuration. A plugin is not the same registration API as Pi's in-process extension or Hermes' Python plugin. Native hooks are command or HTTP handlers merged from Grok files, configuration layers and compatible vendor settings. [D1], [D2]

**[D/R]** An owned hook JSON file can coexist with existing user and Claude-compatible hook files. That is a promising minimal packaging route. Do not overwrite unrelated hook files or disable compatibility globally merely to avoid duplicate OVRCR reports.

**[D/P/R]** `GROK_CONFIG` is a soft-settings overlay. It deliberately drops code-execution, discovery and trust settings; it is not a way to inject hooks. The native inspection experiment confirmed that a hooks table in the overlay was not loaded. [D3], [P1]

**[D]** Session `_meta.pluginDirs` and process `--plugin-dir` exist for ACP/dedicated agent flows. The documented process flag belongs to `grok agent … stdio`, is ignored in leader mode, and is not an exposed interactive pager flag. Do not replace the user's native UI with an SDK/headless client just to gain that option. [D2]

Global/user hooks are trusted differently from project/plugin sources. Folder trust also admits other project surfaces; setup must not silently issue `--trust`, disable folder trust, or move reporting into a broader trust scope without consent. Honor the user's actual `GROK_HOME` rather than assuming one hard-coded home. [D1], [D3]

### 2. The shared leader changes callback ownership and environment

**[P]** Grok's embedded mode runs an agent on a thread in the native pager process. Shared-leader mode instead bridges the pager to an agent hosted in the leader process. On-disk hooks are dispatched by the session actor's local hook registry. [P2], [P3], [P4]

**[P]** The command runner inherits the execution process environment, applies per-hook env settings, injects reserved Grok session/workspace variables, and spawns the command. Commands containing shell syntax/arguments run through `sh -c` on Unix; a plain executable path can run directly. Reserved identity fields cannot be overridden by hook env. [P5]

**[INFERENCE]** An embedded-mode hook can potentially preserve OVRCR's direct-parent check by directly executing the helper or using an `exec` trampoline. An ordinary shell that spawns a second helper process adds an extra parent and will not satisfy that check. In leader mode, even an exec trampoline has the **leader**, not the managed pager, as parent. It therefore does not establish OVRCR's required native-anchor relationship. [O1]

Do not infer that a previously running shared leader inherited this terminal's OVRCR variables, or broadcast one terminal's capability into a multi-session leader. This pass found no verified per-session capability-forwarding contract for the native interactive launch. That is a named native probe/architecture gate, not proof that all Grok integrations are impossible.

`--leader-socket` isolates which leader the pager connects to, but does not by itself make that leader a child with reporting authority over this terminal. A local/dedicated mode or client-side forwarding route may be simpler; changing execution mode must be an explicit product choice rather than an undocumented workaround.

### 3. Preserve Grok's own event names and causal identity

**[D/P]** The hook envelope uses `sessionId` and opaque `promptId` for turn correlation. `hookEventName` carries snake-case values; the compatibility key `hook_event_name` carries PascalCase. Selected aliases such as `session_id` exist, but not every field has both spellings. In particular, do not assume `prompt_id` or `notification_type` aliases in on-disk hook payloads. [D1], [P6]

Relevant events are SessionStart, SessionEnd, UserPromptSubmit, Stop, StopFailure, StopCancelled, Notification and the tool/subagent events. `subagentType` is the important Grok child marker on events that can originate in children. Native request, prompt, tool-call and session identities are different quantities.

**[D/P]** Cancellation can be delivered after the next prompt begins, and timestamps are stamped at hook dispatch rather than at the original transition. Prompt IDs are opaque and session-scoped; clients can supply them, so do not order them lexically or assume global uniqueness. The public turn-end worker rejects stale epochs internally, but a local hook payload does not expose a general monotonic source-generation protocol. [P7]

The simple documentation recipe settles unconditionally on some events without `promptId`. OVRCR must not copy that into an authenticated runtime that may already have a newer binding or active prompt. Missing identity needs scoped reconciliation, not permission to clear whatever is current.

### 4. Prompt submission and Stop are gates, not accepted-start/final-stop observations

**[D/P]** Another UserPromptSubmit hook can reject the prompt after an observer has seen it. Busy can describe admission work, but an observed submit must not be presented as proof that the model accepted or ran the prompt. In the public implementation, a rejected prompt becomes a cancelled outcome with a HookDenied category; verify its actual observer delivery rather than claiming that rejected prompts universally have no ending. [D1], [P8]

**[D]** Grok explicitly warns that a passive Stop observer cannot distinguish a continuation fire from the final fire. `stopHookActive` becomes true after a previous block and can be true for both. A blocking Stop hook continues within the same prompt without another UserPromptSubmit. A separate Stop also fires during session teardown; `reason=end_turn` must be distinguished from shutdown/channel closure. [D1]

**[INFERENCE]** Mapping every Stop to Ready can notify while a user-supplied gate is deliberately continuing work. Calling that Observed does not solve the product question of whether known continuation attempts should create unread responses. Disabling the user's Stop gates would violate configuration preservation.

A later interrupt can follow a completed Stop callback with StopCancelled. End-of-turn hooks can also be lost at bounded teardown or skipped on documented exceptional paths. No clock-based success inference repairs this honestly. [D1], [P7]

### 5. Failure, cancellation and idle have distinct meanings

| Source | Candidate status use | Guardrail |
| --- | --- | --- |
| Correlated UserPromptSubmit | Busy/admission activity | Another hook may reject it; it is not accepted-model-work proof |
| Stop at current prompt boundary | Response-available candidate | Gate may continue; cannot independently prove finality |
| StopFailure | Error observation | Its `lastAssistantMessage` can be rendered error text, not a successful answer |
| StopCancelled | Cancelled/Idle observation | Includes permission rejection, dismissal, max-turn and no-progress causes; preserve reason without success alerts |
| SessionEnd / teardown Stop | Reporting/session teardown | Never Ready merely because the process exits |
| `idle_prompt` Notification | Delayed state observation | Fires after error/interruption too; not a response outcome |
| `task_complete` Notification | Undetermined until its exact producer is certified | Background task completion is not necessarily a root response |
| Subagent events | Exclude from root status | Child completion cannot rebind or finish the parent |

**[D]** `idle_prompt` fires roughly a minute after settling and is cancelled if a new message arrives first. It can follow a cancelled or failed turn. It is not a substitute for prompt-specific Ready, nor proof that an output is newly Unread. Cancel-and-send can replace work without StopCancelled, while completed bash/slash activity has different reporting behavior. [D1]

Keep the desired product rule explicit: failures/cancellation should not create new Ready/Unread or acknowledge an earlier response, if the user carries over the Pi/OMP contract.

### 6. Grok already has attention-opening signals

**[S/P]** Superset listens to Notification subtypes `permission_prompt` and `elicitation_dialog`. The public Grok source confirms `elicitation_dialog` is emitted for `ask_user_question`; it precedes the actual ACP question request. The subtype is therefore real, but its hook alone does not prove that the frontend has already displayed a question. [S1], [P9]

**[P]** Native source tests distinguish a real permission wait from an auto-approved call. An ignored PTY test waits for visible permission-card text before checking that the hook fired. These are useful acceptance examples, not tests run in this investigation. `permission_prompt` is stronger than PreToolUse; a generic tool start is not a human wait. [P10], [P11]

**[P]** Notification's on-disk payload contains a subtype string and optional message/title/level. It has no logical request ID, phase, resolve outcome or visibility flag. Common `promptId` identifies a turn, not one of several questions within it. StopCancelled after denial is a turn-ending report, not a paired request-close event. [P6]

Do not conclude that Grok has no question hook. The actual gap is a complete, identity-qualified request lifecycle and a certified presentation boundary for OVRCR alerts.

### 7. A richer pending-interaction stream already exists

**[P]** Grok's session owner maintains a per-session pending-interaction map keyed by stable tool-call ID. Kinds include permission, question, plan approval and MCP elicitation. A guard broadcasts `pending_interaction` on opening and `interaction_resolved` on drop, including cancellation/error paths. The roster uses the same map for NeedsInput. These are native `x.ai/session_notification` messages, **not** equivalent fields in shell-hook Notification. [P12]

This is a concrete candidate to reuse before designing a brand-new upstream request registry. It already has identity and closure that Superset's hook payload does not preserve. However:

- A pending reverse-request is not necessarily an already visible frontend card.
- Notifications are transient; safe recovery requires a current-state baseline rather than assuming history replays every opening.
- The inspected broadcast has no generic source sequence in its metadata; request identity alone is not a cross-generation ordering scheme.
- OVRCR must prove it can observe the exact native session without taking over approval responses, mutating/resuming work just for telemetry, or binding another subscriber's session.

The built-in question, plan approval and MCP elicitation surfaces should be scoped explicitly in the interview. Do not promise all four from a single generic Notification handler.

### 8. Other reconciliation candidates must be evaluated before requiring new exports

**Client-side status line [D/P].** Its structured payload has session ID, current prompt ID and live-turn start time. The script is executed by the pager; this is a potentially useful client-owned reporting boundary even when the agent lives in a leader. It does not carry terminal success/failure or logical Input-request identities, and a missing prompt ID does not mean Ready. [D4], [P13], [P14]

Status-line refresh timers reuse cached payloads; a fresh execution is not a new measurement. The row can be hidden and scripts have bounded lifetime/cleanup. Any composition must preserve the user's command and stdout exactly once and must not turn on a new layout as an unnoticed side effect. This is an investigation candidate, not a chosen dependency or a reason to add metrics scope.

**Native after-turn path [P].** The public session owner has before/after-turn calls through workspace operations and richer internal completion bookkeeping after its processing loop. That is a concrete place to investigate a minimal observer export. It is not a registered on-disk Stop hook. The generic ObservabilityBridge is a server-leg mechanism and can be constructed without a transport; finding its type is not proof of a usable local observer. [P8], [P15], [P16]

A robust implementation may need a narrow native-client forwarding/observer hook that carries admitted session/prompt identity, outcome and pending-request state. First certify whether the existing ACP or client-side surfaces can supply it safely. Do not add an SDK, a second UI, transcript/log polling, or a general observability framework just because the file-hook interface is incomplete.

### 9. Preserve source bounds, trust and recovery

**[D/P]** Grok exposes private content in ordinary hook inputs: prompt/tool text, last assistant text, errors and background-task descriptions. Extract only needed identity/outcome fields; never forward or log complete native payloads. Native text/payload caps do not establish that every serialized hook input fits OVRCR's existing 65,536-byte limit. Oversized input handling and any pre-normalization boundary need explicit acceptance. [D1], [P5], [P6]

Avoid detached helper delivery that outlives Grok's owned hook process group. The native hook queue can drop work at teardown; OVRCR must preserve reporting health separately from activity and must not fabricate finality after that loss.

Reload re-reads hooks, but does not itself expose all source-generation/current-request state needed for safe reporter recovery. Reconnection should fence retired producers, baseline current state without old alerts, and remain Unknown when the chosen native source cannot establish it. No silent trust bypass, blanket version gate, arbitrary descendant authentication, or persistent metrics database is justified by this status feature.

## How Superset integrates Grok today

This section describes pinned Superset source `c3c6717bb7de1ce91dbf82505e9dce12c2c0324e` and the isolated helper/mapper experiment, not live Superset acceptance.

### End-to-end path

1. **Preset and wrapper.** The built-in Grok preset runs `grok --always-approve`, including resume/fork variants. The wrapper resolves the real binary, exports first-wins root harness identity, and execs the native arguments. A delayed two-second liveness check sends an attachment event for launches that survive help/version filtering. [S1], [S2], [S3]
2. **Owned global hook file.** Setup writes `superset-notify.json` under the home Grok hooks directory. It registers SessionStart, SessionEnd, UserPromptSubmit, PostToolUse, PostToolUseFailure, Stop, StopFailure and Notification. Notification matches exactly permission_prompt or elicitation_dialog. PreToolUse is intentionally absent to avoid adding latency without useful signal. StopCancelled is absent. [S1]
3. **Compatibility settings.** Setup can add a marker-owned TOML block disabling Claude and Cursor hook replay, while skipping vendor tables the user already defined. That avoids duplicate foreign hooks for Superset but can change the behavior of previously implicit compatibility settings. [S1]
4. **Notify helper.** The shell helper requires Superset terminal context, rejects a hook-origin/root-harness mismatch, extracts a session identity, and posts a normalized event to the host. It tries the current endpoint and host manifests, with a legacy fallback. [S4]
5. **Host mapping.** SessionStart/End become Attached/Detached; prompt submission and tool-result events become Start; selected notifications become PermissionRequest; Stop becomes Stop; StopFailure becomes Failed. The endpoint is deliberately unauthenticated and validates terminal/workspace existence rather than OVRCR-style capability ownership. [S5], [S6]
6. **Binding store.** The host records receipt time and session identity. A delayed Attached event preserves later lifecycle state for the same session; a time-based ended-binding straggler window handles some late callbacks. No prompt ID, source sequence or exact Input-request object travels through this path. [S7]

### What to reuse and what to change

| Superset mechanism | Useful lesson for OVRCR | Required difference |
| --- | --- | --- |
| Dedicated owned hook file | Additive setup and targeted removal | Honor actual Grok home, consent and existing native settings |
| Separate root harness and hook origin | Drop Claude/Cursor compatibility replays without mislabelling the terminal | Use OVRCR's capability/binding authority; do not rely on environment identity alone |
| Explicit permission_prompt / elicitation_dialog selection | Concrete Grok attention events to certify | Retain logical request identity/closure and clarify requested versus presented |
| PostToolUse/PostToolUseFailure returning Start | Useful simple resumption approximation | Restore actual underlying activity; tool completion is not a universal request-close contract |
| Attachment separate from working state | Launching an idle agent is not Busy; delayed attachment should not erase progress | Use explicit ownership/admission instead of a sleep as identity proof |
| Stop and StopFailure separated | Failures should not look like clean completion | Add cancellation handling and resolve the Stop-gate ambiguity |
| First-wins generic child/origin logic | Preserve the parent across nested foreign harnesses | Explicitly parse Grok subagentType and scope native prompt/binding identity |
| Host endpoint recovery | A stale endpoint should not permanently break reporting | Reuse authenticated Unix supervision; no manifest scanning or unauthenticated HTTP replacement |

### Limits demonstrated or visible in the pinned adapter

- **[S/R] Prompt causality is discarded.** The helper posts agent/session identity but not `promptId`. The host stamps `Date.now()`; this cannot reject every delayed Grok cancellation or A-to-B-to-A reuse correctly.
- **[S/R] Grok child markers are missed by the generic parser.** It recognizes `agent_id`/`agentId`, not Grok's `subagentType`. A synthetic child prompt therefore produced a root-shaped Start with the child session identity. This demonstrates the helper behavior, not an assertion about every live child path.
- **[S/R] StopCancelled has no mapping.** It is absent from both the generated Grok registrations and the host event mapper. Manually supplying it to the helper forwards an event the mapper ignores.
- **[S/R] Stop continuation is not distinguished.** `stopHookActive` and prompt correlation are not sent downstream. This path does not solve the native Stop-gate problem.
- **[S] Defaults differ from the desired OVRCR contract.** Always-approve launch presets and disabling compatibility hooks must not be copied as reporting requirements. The Grok home helper uses the OS home plus `.grok`, rather than consulting Grok's configurable home.
- **[S] Parser and privacy boundaries differ.** The helper uses regex extraction from the raw JSON and may send message/error previews. OVRCR should parse structurally, bound input and transmit only the agreed status metadata.
- **[S/P] Shared-leader safety is not established by this reference.** The inspected Superset wrapper has no explicit per-terminal authenticated leader-forwarding mechanism. Superset's HTTP endpoint does not enforce OVRCR's direct-parent requirement, so its successful delivery path would not prove OVRCR admission is correct.

Superset is useful implementation prior art, not certification of Grok finality or of OVRCR's stricter identity and attention contracts. This fresh check also improves on the older [Superset source review](superset-agent-reporting-2026-09-09.md): normal Grok questions have a concrete elicitation_dialog opening signal, but complete per-request observation is a separate capability.

## Candidate product specification

This is a discussion draft. The interview must approve the Grok-specific choices before it becomes a tracker specification.

### Problem Statement

A user running Grok in OVRCR must keep watching the native terminal to discover whether a response is ready or a question needs attention. Generic hook integration can confuse Stop-gate attempts with completion, lose delayed cancellation identity, misclassify child activity and fail when callbacks originate in a shared leader.

### Solution

Integrate Grok's native interactive workflow into OVRCR's existing status, explicit Unread review and optional attention alerts. Preserve native approvals, user hooks, compatible vendor configuration and the selected interface. Use exact root/session/prompt/request identity and public native observations rather than terminal text, timers or global inference-log scanning.

Reuse small hook packaging where it fits, and first evaluate the existing native pending-interaction, client status-line and after-turn surfaces for the missing guarantees. Add only the smallest supported forwarding/export contract required by the agreed native launch mode.

### User Stories

1. As a Grok user, I want the Dashboard's launch action to start a tracked native session, so that I do not need to assemble a wrapper manually.
2. As a user, I want Ready to mean a root response is available for review, so that I do not mistake it for task success or guaranteed finality.
3. As a user, I want known Stop-gate continuations and internal tool turns to remain Busy, so that intermediate output does not produce repeated unread responses.
4. As a user, I want blocked prompts, failures and cancellation handled distinctly, so that rejected or interrupted work does not generate a success alert or leave incorrect activity indefinitely.
5. As a user, I want delayed old-prompt callbacks rejected, so that they cannot clear newer work or a replacement conversation.
6. As a user, I want Grok child sessions excluded from root reporting, so that child completion cannot overwrite the parent session identity.
7. As a user, I want one Unread identity per completed response cycle and explicit review of the Presented response, so that acknowledgement cannot accidentally clear newer output.
8. As a user, I want actual human questions and approval requests to show WaitingInput, so that I know when my answer is needed.
9. As a user, I want automatic approvals excluded from human waiting, so that the Dashboard does not ask for attention when Grok already decided automatically.
10. As a user, I want optional Response ready and Input needed alerts with separate identities, so that questions do not become Unread responses or replay after reconnect.
11. As a user, I want answered, cancelled, timed-out and obsolete requests to stop waiting and cancel queued alerts, so that I am not notified about a question that is gone.
12. As a user, I want my existing Stop gates, Claude/Cursor hooks and native permissions preserved, so that reporting does not change my safety or automation policy.
13. As a user, I want minimal/fullscreen presentation and native session resume/fork behavior preserved, so that enabling reporting does not change how I use Grok.
14. As a user, I want shared-leader sessions bound to the correct terminal, so that a callback or capability cannot cross into another session.
15. As a user, I want safe reporter recovery without replaying historical responses or requests, so that temporary disconnection does not create duplicate alerts.
16. As a user, I want ordinary compatible upgrades to remain enabled and doctor to describe actual delivery/capability gaps, so that a version string alone does not decide support.
17. As a user, I want private prompts, responses, commands, questions and credentials kept out of reporting, so that status integration does not disclose conversation data.
18. As a user, I want existing provider reporting and scheduled tasks to remain correct at fifty-session capacity, so that adding Grok does not regress other work.
19. As a user, I want absent token/cost/context accounting to stay unknown, so that status reporting does not imply an unfinished metrics feature is complete.
20. As a maintainer, I want the chosen implementation anchored to the actual target build and verified source contracts, so that public-source drift does not silently change the integration's guarantees.

### Implementation Decisions — proposed defaults

- Keep Ready, Unread, Presented, Input requests, activity, process phase and reporting health separate. Reuse landed OVRCR owners rather than duplicate stores or notification settings.
- Use native Grok identity and explicit origin-harness guards. Preserve compatible vendor hooks instead of disabling them to avoid duplicate OVRCR callbacks.
- Prefer an owned, consented hook file for configuration. Do not assume `GROK_CONFIG` accepts hooks or that an ACP-only plugin flag works in the native pager.
- Define shared-leader support and authenticated callback ownership before implementing transport. An exec trampoline only solves an extra shell parent, not the leader/pager identity split.
- Preserve arbitrary user Stop gates. Do not claim a final Ready from every Stop attempt; investigate post-gate outcomes and current-state sources before requiring a new API.
- Use permission_prompt and elicitation_dialog as known request-opening candidates. Prefer the richer native pending-interaction identities/resolution for complete waiting semantics when safe access is established. Do not confuse pending requests with visible frames.
- Treat cancellation/failure as non-Ready outcomes, and use prompt-scoped causality rather than receipt order. Never let a session-wide idle hint clear a newer binding or create an Unread response.
- Ordinary version upgrades remain attempted unless a specific incompatible capability is known. Record the installed binary, installed docs and public source separately; handler registration/discovery does not certify actual delivery.
- Baseline recovery without historical alerts; leave Unknown/unavailable where current ownership/activity cannot be established safely. Keep queues, helpers, input and cleanup within existing bounds.
- Keep metrics and log scanning outside the first status milestone. Status-line metadata may be a reporting candidate without expanding the feature into accounting.

### Testing Decisions — proposed seams

- **Primary OVRCR seam:** the actual Dashboard/managed CLI launching native Grok under an isolated server/PTY and authenticated reporter. Exercise the picker route, not merely executable detection or a manually typed command.
- **Native source seam:** real prompt admission, blocking Stop gates, failure/cancellation, and permission/question presentation with source IDs captured at emission. Include a second user hook that blocks or continues work; an adapter-only callback fixture cannot prove those interactions.
- **Leader ownership seam:** a native pager using an already-running task-owned leader and another concurrent session, plus the selected dedicated/embedded mode. Verify callback PID/parent and per-session environment/capability routing. Do not touch the user's leader.
- **Input and review seam:** runtime snapshots and Dashboard consumption, exact Presented acknowledgement, native question/approval response, request close-before-alert, duplicate delivery, visibility suppression, reconnect baseline and actual enabled host notifications.
- Cover cancel-and-send, cancellation after the next prompt begins, teardown Stop, Stop interrupted mid-gate, hook-disabled/untrusted cases, blocked prompt, missing/oversized payloads, child markers, existing compatibility hooks, new/resume/fork/rewind, backend reconnect and stale source generations.
- If using native pending-interaction or status-line data, prove the exact-session subscription does not become an approval responder, resume/mutate work or steal foreground ownership. Replayed/cached state must not count as a new request or measurement.
- Preserve the user's status-line output if composition is selected, and verify both minimal and fullscreen presentation without changing the default UI policy.
- Reuse existing fifty-session socket/load, protocol compatibility and provider regression fixtures after the deep-modules refactor lands. Native macOS, Linux, hosted checks, source tests read, offline inspection and actual UI evidence remain separate categories.
- Use isolated HOME/GROK_HOME/workspaces/sockets and explicit fixture ownership. No credentials or user conversation content in capture files; retain failing attempts and verify removal of only task-owned resources.

### Out of Scope — proposed

- Context/token/cost accounting, quotas, billing reconciliation, Superset-style historical log scans and analytics.
- Replacing the native pager with a headless/SDK UI just to obtain events, arbitrary descendant authentication, blanket auto-approval, disabling user gates or compatibility, and global trust bypass.
- Treating idle_prompt/task_complete, process exit, a timestamp or an empty background-task array as proof of a new root response.
- A second service, general observability framework, new database or persistent conversation recovery.
- Automatic upstream issue creation, tracker publication, installation, implementation, native upgrade or merge before the design interview.

### Further Notes

The old [multi-harness Grok task](../plans/2026-09-09-multi-harness-reporting.md#task-6-complete-grok-cli-integration) includes usage/status-line accounting. Keep that broader work open rather than silently importing it into this status feature.

The [Pi/OMP spec #88](https://github.com/xlyk/ovrcr/issues/88) and its implementation tickets are planned shared work, not assumed completed. The [deep-modules refactor #77](https://github.com/xlyk/ovrcr/issues/77) changes readiness, reporting transport, Dashboard internals and test seams. Refresh the integration map after it merges. Grok source/configuration research can proceed independently; Grok does not need an artificial dependency on OMP's question export.

## Decision tree for grilling

Ask the independent frontier together. The answers are not presumed by this research document.

| ID | Decision | Recommended starting answer | Dependent questions |
| --- | --- | --- | --- |
| D1 | Must the first delivery support the user's default shared-leader mode, or may a dedicated/embedded native mode ship first? | Preserve the selected native mode; make a reduced first slice explicit rather than silently forcing no-leader | Exact authenticated leader ownership, already-running leader and multi-client scope |
| D2 | What Ready behavior is acceptable while user Stop gates can continue the same prompt? | Preserve user gates and suppress known intermediate completions; certify a stronger boundary | Existing after-turn/native stream access versus a small upstream client/export change |
| D3 | Does Input needed cover permissions and agent questions only, or also plan approval and MCP elicitation? | At least permissions and ordinary questions; name the other surfaces explicitly | Logical request identity, native pending stream access, requested versus actually presented |
| D4 | Is an explicit one-time owned hook/plugin setup in the selected Grok home acceptable? | Yes; preserve other files, compatibility and trust choices | Distribution/removal, custom GROK_HOME and per-invocation loading needs |
| D5 | May an existing status-line command be composed, or should that surface remain untouched? | Keep it untouched unless a concrete source benefit justifies composition and exact stdout preservation | Client-owned identity/reconciliation and hidden/cached refresh semantics |
| D6 | Which build is the implementation target when installed 1.0.30 and public-source revision differ? | Treat installed behavior as the current target; obtain matching source or native certification for source-dependent claims | Upstream contribution/release strategy; no runtime exact-version allowlist |
| D7 | Is this a status/review/attention milestone with accounting excluded? | Yes | Completion ledger, independent later metrics work and scheduling with shared OVRCR changes |

Later decisions depend on these answers:

- How should a prompt blocked by another hook affect Busy and recovery? Native submit is not accepted model work.
- Can the exact native session be passively observed without an approval responder or mutating load/resume? Existing schema names alone do not prove this.
- What snapshot and source generation are available after hook reload, leader replacement, session rebind or Dashboard reconnect?
- Which cancellation causes map to Idle versus Error while preserving prior Unread? Unknown native values must stay forward-compatible without success inference.
- Confirm that minimal/fullscreen, children, background wakeups, normal/native resume and custom Grok home are covered by the chosen slice rather than assumed from Superset's defaults.
- Reconfirm the proposed Pi/OMP carryovers and authorize a final specification only after the Grok-specific choices are settled.

## Source references

### Installed documentation

The installed docs were read from Grok's extracted guide directory. Their current local text is evidence for this pass, not a source checkout pinned to the build hash.

[D1]: /Users/xlyk/.grok/docs/user-guide/10-hooks.md
[D2]: /Users/xlyk/.grok/docs/user-guide/09-plugins.md
[D3]: /Users/xlyk/.grok/docs/user-guide/05-configuration.md
[D4]: /Users/xlyk/.grok/docs/user-guide/25-status-line.md

Useful guide anchors: hooks sources/trust 60–81; common fields 238–256; Stop/failure/cancel/idle and gaps 331–456; env 480–528; plugin per-session/process paths 415–438; overlay allowlist 23–30; status-line live prompt and cached refresh 52–110. Session and native input context also came from `17-sessions.md`, `22-permissions-and-safety.md`, `03-keyboard-shortcuts.md` and `19-plan-mode.md` in the same installed guide directory.

### Pinned public Grok implementation

[P1]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-config/src/config_layers.rs#L17-L26
[P2]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-pager/src/acp/spawn.rs#L267-L380
[P3]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/leader/in_process.rs
[P4]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/acp_session_impl/hook_dispatch.rs#L274-L295
[P5]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-hooks/src/runner/command.rs#L103-L227
[P6]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-hooks/src/event.rs
[P7]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/acp_session_impl/turn_end_hooks.rs
[P8]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/acp_session_impl/turn.rs#L1258-L1595
[P9]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/acp_session_impl/spawn.rs#L2190-L2248
[P10]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/acp_session_tests/permission_prompt_notification_tests.rs
[P11]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-pager-pty-harness/tests/pty_e2e/permission_prompt_hook_chimes_only_on_real_wait.rs
[P12]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/pending_interaction.rs#L1-L114
[P13]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-pager/src/app/status_line/command.rs#L257-L320
[P14]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/acp_session_impl/status_line.rs#L107-L220
[P15]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/session/acp_session.rs#L1175-L1197
[P16]: https://github.com/xai-org/grok-build/blob/37949780c144e37df692e3d669051a21fec24f20/crates/common/xai-computer-hub-sdk/src/observability.rs
[O1]: ../crates/ovrcr-runtime/src/agent_runner.rs

### Pinned Superset implementation

[S1]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/agent-setup/src/agent-wrappers-grok.ts
[S2]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/agent-setup/src/agent-wrappers-common.ts
[S3]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/shared/src/builtin-terminal-agents.ts#L181-L190
[S4]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/agent-setup/templates/notify-hook.template.sh
[S5]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/host-service/src/events/map-event-type.ts
[S6]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/host-service/src/trpc/router/notifications/notifications.ts
[S7]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/host-service/src/terminal-agents/store.ts

The older [Superset source review](superset-agent-reporting-2026-09-09.md) remains historical evidence at its own revision. The [Hermes pre-grill brief](hermes-status-reporting-2026-09-12.md) provides the parallel research format, not a claim that the native mechanisms are the same.
