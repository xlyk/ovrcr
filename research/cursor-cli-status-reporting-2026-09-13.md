# Cursor CLI status reporting: research and pre-grill draft

**Status:** Research-backed discussion draft. Not an approved Cursor specification, implementation plan, or ticket. Use this file with the grilling/domain-modeling workflow before implementation or publication.

**Goal:** Prepare native Cursor CLI integration with OVRCR's status, Ready/Unread/Presented review and optional Input-needed alerts, preserving the user's native interface, configuration and permissions.

**Summary:** Cursor CLI has useful outcome-bearing hooks: the installed interactive UI sends `stop` with `completed`, `aborted` or `error`. That is stronger than treating every Stop as success. However, hook follow-ups bypass the normal prompt-submit hook, some lifecycle callsites check only user/project hook configuration, and native input-request identities are internal rather than public hook payloads. Superset is useful packaging prior art, but its Cursor adapter collapses outcomes and treats pre-execution gates as input waits. Do not copy those semantics into OVRCR.

## How to use this document

- Treat the existing OVRCR domain vocabulary as authoritative. Ready means a response is available for review, not task success or guaranteed finality. An Input request is distinct from Unread; selecting a terminal or answering a question is not review.
- The proposed carryovers from Pi/OMP are **not yet approved specifically for Cursor**. Begin with the independent decisions in **Decision tree for grilling** and defer implementation-specific decisions until their prerequisites are settled.
- Facts from the installed CLI, current rolling docs, Superset source, and synthetic probes are labelled separately. Do not promote a documented IDE/cloud hook into a CLI support claim without an installed caller.
- Prepare the final product spec and tickets after the interview. Do not patch the vendor bundle, launch a replacement ACP UI, or publish vendor requests as a side effect of this research.

## Evidence baseline

| Surface | Observed baseline | Meaning |
| --- | --- | --- |
| Installed `cursor-agent` | `2026.08.31-4057e58` | Actual `--version` output |
| Installed `agent` | Resolves to the same launcher as `cursor-agent` | Alias identity established by path resolution, not another harness |
| Native launcher | Bash wrapper execs bundled Node and `index.js` | The wrapper itself does not add a lasting parent process |
| `cursor` command | Separate local shim: searches for an IDE command, otherwise supports `cursor agent` via the agent alias | Do not treat bare `cursor` as the standalone agent or launch the IDE for reporting |
| Source material | Installed webpack module factories from the exact versioned bundle | Installed compiled implementation, not a public upstream TypeScript checkout |
| Official docs | Current Cursor hooks, CLI configuration/parameters/usage and third-party hooks | Rolling reference covering multiple product surfaces |
| Superset | `c3c6717bb7de1ce91dbf82505e9dce12c2c0324e` | Pinned source comparison, not a live Superset test |
| OVRCR | Baseline `b028d3592c38b8f672cdd7f8147daf905b6953eb`, `feature/pi-integration` | Existing Claude/Codex reporting; later provider-neutral work is not presumed implemented |

Evidence labels: **[I]** installed bundle implementation; **[D]** current official docs; **[S]** pinned Superset source; **[R]** executed isolated probe; **[INFERENCE]** a source-derived consequence awaiting native acceptance.

The guessed CLI-specific hook URL returned 404. The official hooks page resolves to the umbrella `/docs/hooks` guide; its IDE/cloud descriptions are not a precise support matrix for this installed CLI. No Cursor upgrade, login/status query, model request or native conversation was performed.

### Readability-only source recovery

The installed package contains minified webpack chunks. Factory definitions were collected in an isolated JavaScript VM, without invoking the agent entrypoint or those factories, then syntax-formatted into temporary readable files. Original webpack module names and container filenames were retained. Derived line numbers are readability anchors, **not original TypeScript source line numbers**.

Two small modules were subsequently invoked in a separate controlled guard experiment described below. The other extracted factories remained inspection-only. Temporary source copies are not proposed runtime dependencies and must never become an integration based on private bundle imports.

## Executed experiments

### Installed lifecycle configuration guard

Called the actual installed `after-agent-hooks` response function with synthetic metadata, the actual shared hook constants, an inert debug logger and a recording executor. The experiment did not load a native conversation, execute a hook script or contact a server.

| Supplied hook configuration | Calls reaching the executor |
| --- | --- |
| User hook for `afterAgentResponse` | 1, with the synthetic conversation/generation IDs |
| Plugin-only hook for the same event | 0 |
| Team-only hook for the same event | 0 |

**[R]** This confirms that specific caller's local user/project guard. It does not prove every `--plugin-dir` scenario fails, nor certify plugin discovery and configuration mutation end to end. It does establish that “the executor accepts plugin hooks” is insufficient proof that CLI lifecycle callers invoke them. [I2], [I3]

### Superset Cursor helper

Executed the actual pinned Cursor-specific shell template with synthetic JSON, normal template substitutions, and a local `curl` recorder that made no network requests. No Superset host service, user configuration or real Cursor session was used.

| Synthetic input | Actual outbound behavior |
| --- | --- |
| Stop, `status=completed` | Stop |
| Stop, `status=error` | The same Stop |
| Stop, `status=aborted` | The same Stop |
| PermissionRequest from a pre-execution hook | PermissionRequest plus stdout `{"continue":true}` |
| Child-shaped Start with subagent/parent metadata | Root-shaped Start using the child session ID |
| Cursor event nested under a Claude root identity | Dropped |
| PermissionRequest without Superset terminal context | No outbound event, but still prints `{"continue":true}` |

All seven processes exited 0 with empty stderr. Outbound events omitted generation ID and stop status. The child case demonstrates this parser's behavior on the supplied metadata, not proof that every installed child hook emits those exact fields. The permission response is not, by itself, proof that Cursor automatically approves the underlying command. [S1], [S2]

These probes establish code-path behavior only. Actual interactive lifecycle delivery, hook-process parentage, native input visibility, Dashboard output, sound/desktop delivery, and platform acceptance remain unrun.

## Installed Cursor CLI findings

### 1. Ordinary foreground CLI is a distinct scope from IDE, workers, persistence and ACP

**[I/D]** `cursor-agent` and `agent` are aliases for the same installed CLI. Its normal launcher execs Node. The CLI supports interactive operation, print mode, local plugin directories, resume, worktrees, persistent sessions and worker commands. `--mode ask` is a read-only agent mode, **not** an ask-the-user interaction. [I1], [D2], [D3]

Recommend beginning with ordinary foreground interactive CLI and explicit resume behavior. Native `persist`, cloud/self-hosted workers, IDE Composer and ACP should be separate scope decisions because they can change process ownership, lifecycle and who answers approvals. OVRCR already preserves live PTYs across Dashboard detach; do not assume a second persistence layer is needed.

OVRCR's picker already detects `cursor-agent` at the inspected baseline. That is not reporting support: its actual launch action still needs a managed route and a distinct protocol provider identity. Do not broadly classify an arbitrary executable named `agent` as Cursor without verifying its resolved implementation.

### 2. CLI generation identity is useful, but automatic work does not always pass the submit hook

**[I]** The native UI uses a fresh UUID for each agent generation. Human prompt handling assigns an ID before `beforeSubmitPrompt`, then passes the generation UUID into the agent run. `conversation_id` identifies the chat; `generation_id` identifies this native run, not OVRCR's binding generation. [I4]

`beforeSubmitPrompt` is a gate. A `continue=false` result returns before the agent run starts, without a stop for that rejected submission. A reporter that marks Busy solely on the pre-submit hook therefore needs admission/rejection reconciliation; a generic pre-event is not accepted model work.

**[I]** A nonempty successful Stop-hook follow-up is queued as another user-message action. The queued scheduler gives it a new generation UUID, but that route bypasses the normal `beforeSubmitPrompt` handler. There is no new sessionStart for it.

**[INFERENCE]** An adapter requiring every accepted ending to match a generation first seen in `beforeSubmitPrompt` will reject legitimate automatic follow-ups. Conversely, simply accepting every new generation from an ending weakens stale/root-event rejection. The integration needs a reliable generation-start/current-state route or an explicitly narrower product contract.

### 3. Outcomes are present, but Stop is not a post-follow-up-settlement event

| Hook in the installed CLI | Actual source behavior | Reporting consequence |
| --- | --- | --- |
| `beforeSubmitPrompt` | Human admission gate; can reject before run | Busy/admission candidate, not proof of accepted work |
| `afterAgentThought` | Fire-and-forget callback | Not response completion |
| `afterAgentResponse` | Success path after the agent run/state processing, fire-and-forget, before Stop processing | Response-availability evidence, not finality after hook follow-ups |
| `stop` / completed | Awaited on the normal success path; returned followup_message can enqueue another generation | Useful outcome, but reporter executes before all follow-up decisions are applied |
| `stop` / aborted | Dispatched without awaiting from abort handling | Preserve cancellation; delivery may race teardown or another outcome |
| `stop` / error | Dispatched without awaiting from normal error handling | Preserve failure; do not map to clean Ready |
| `sessionStart` | Initial new/new-with-ID startup only, nonblocking | Not a universal resume or in-process conversation-change signal |
| `sessionEnd` | Process cleanup, with first cleanup reason memoized | Not a per-response completion event |

**[I]** Completed Stop carries conversation/generation/model, `status`, `loop_count` and token-related fields. Aborted/error paths do not consume returned follow-ups. The installed behavior therefore does not establish the umbrella docs' example of retrying an error through a Stop-hook follow-up. [I4], [D1]

`afterAgentResponse` and aborted/error Stop calls are non-awaited. There are also exceptional paths that rethrow before the ordinary error hook. Do not claim that all failures produce exactly one stop. A possible aborted-then-error race needs native reproduction before asserting a fixed precedence rule.

**[I]** The hook executor applies per-script Stop/subagentStop loop limits; unspecified defaults to five, while null is unlimited. A reporting-only observer may need an explicit unlimited observation limit so it does not disappear after several hook continuations. It must still return no follow-up, retain its own short deadline, and leave other scripts' continuation limits unchanged. [I3], [I5]

Ready can retain the agreed response-available meaning with evidence quality separate from success/finality. Whether each completed automatic generation is reviewable or all known follow-ups form one response cycle must be decided explicitly; the file-hook callback cannot foresee other hooks' merged follow-up result.

### 4. Per-launch plugin loading is advertised, but lifecycle caller coverage must be proven

**[I/D]** `--plugin-dir` exists in the installed CLI and current parameter docs. The hook executor can aggregate enterprise, team, user, project, runtime, Claude-compatible and plugin hooks. [I3], [D2]

However, the inspected interactive callsites for beforeSubmitPrompt, after-agent response/thought, and Stop check only local user/project arrays before calling that executor. The isolated response-hook experiment confirmed the guard for plugin-only and team-only supplied configurations. [I2], [I4]

**[INFERENCE]** A temporary plugin alone is not yet a proven no-global-setup solution for all status events. Prefer to test that exact managed launch before selecting it. A native user/project hook entry can reach the observed callsite guard, but setup must preserve the existing shared hooks object rather than replace it.

Current docs describe configuration reload, but this pass did not certify complete runtime reload behavior for every CLI hook source. Initial loading, plugin refresh, team-hook merge and an already-running conversation may not use the same caller checks.

### 5. SessionStart/sessionEnd are narrower than the umbrella documentation suggests

**[I]** Initial sessionStart is fire-and-forget for new/new-with-ID CLI startup. Resume skips it. In-process new/resume/fork switches reset the active chat and generation tracking without invoking the same sessionStart path. [I6], [I4]

SessionStart initially uses the conversation ID for both conversation_id and generation_id; the first prompt later receives another generation UUID. Hook execution decorates requests with `session_id` and common metadata, so the raw CLI callsite and the final script input are not identical schemas. [I3], [I6]

SessionEnd is emitted through memoized process cleanup with a reason, duration, `is_background_agent=false` and final_status matching that reason. It is not proof that a response was delivered. Setup failures before cleanup is registered and abrupt termination need separate failure treatment.

First-prompt admission may be a workable bounded starting point; immediate launch-time binding, in-process switching and reattachment require additional source identity/current-state evidence. Never use a delayed startup callback or an ending from old history to rebind a newer conversation.

### 6. Root and subagent claims require an installed-caller check

**[D]** The current general docs show subagent IDs, parent-conversation IDs and subagentStop status/summary fields. **[I]** The installed shared hook schema/executor supports subagent hooks and transcript enrichment, and CLI resources have parent/child conversation separation. This investigation did not finish the actual subagent hook producer path. [D1], [I3], [I6]

Do not claim child hook coverage from enum presence or a resource-provider field. Verify actual in-process children, shell-launched nested CLIs and automatic background work. A callback should mutate the root only after session/generation admission and root ownership are established; a filename, compatible hook spelling or inherited environment variable is not enough.

### 7. Native human-input objects exist, but public hook gates do not expose them

**[I/D]** `preToolUse`, beforeShellExecution and beforeMCPExecution run before operations and participate in allow/deny/ask decisions. They do not represent an already visible unresolved input request. The installed generic preToolUse path enforces deny, not a generic ask branch; the docs explicitly acknowledge that limitation. After-tool completion and postToolUseFailure likewise do not provide a stable input-open/input-close contract. [I3], [D1]

**[I]** The native approval store maintains process-local IDs of the form `decision-<time>-<counter>`. It adds/removes decisions and resolves promises on approval, rejection or clearing. Parallel approvals can exist and the UI can switch between them. The store has no public hook forwarding, no presented flag and no complete reconnect-generation contract. [I7]

**[I]** Native AskQuestion handling stores query ID, tool-call ID, arguments and a resolver. Answers resolve the query; skip/abort/retry produce distinct rejected outcomes. An asynchronous question mode can return to the agent before the human answers. A tool result labelled async is therefore not a resolved question. Those IDs are internal UI/query data, not generic hook payload fields. [I4], [I8]

**[I]** Native plan generation can return a successful plan artifact to the agent before a local “Ready to build?” UI decision. Plan decision state is not the same object as an ordinary tool approval. Workspace trust, MCP-server startup approval and execution-mode introduction are additional startup dialogs, not automatically in-session WaitingInput. [I9], [I10]

Recommend a narrow supported observer/forwarding seam at the native interaction owner, using existing request/query identities where possible. It should distinguish requested from presented, emit closure for the same logical request, and provide a safe baseline after reporting recovery. Do not monkeypatch UI components, wrap every tool, auto-approve, or add an MCP question replacement merely to gain status data.

Because this is installed bundled vendor code rather than a public contribution checkout, a missing supported observer may require a vendor feature request or another documented extension mechanism. Do not promise an upstream implementation PR from a minified bundle.

### 8. Native terminal notifications and ACP are not drop-in observability APIs

**[I/D]** Cursor has a notifications preference and internal needs-input sending. The inspected sender accepts a message, deduplicates by message over a short window, and uses terminal-specific escape sequences/bell with optional focus gating. It carries no logical request identity or resolution event. Messages may contain command text. This is an output side effect, not OVRCR's authenticated state source. Finish-alert caller wiring was not fully traced, so this document does not assert it is absent. [I11], [D3]

Do not scrape OSC/bell sequences or disable Cursor's own notification preference silently. Decide overlap with OVRCR's host alerts during the interview.

**[D/I]** Cursor's ACP mode has client-owned permission/question/plan request-response boundaries. The installed ACP handlers map question and plan responses using query IDs. That is useful source vocabulary, but it is a separate custom-client mode, not proof that OVRCR can passively observe the existing native CLI without becoming its UI or approval responder. [D4], [I12]

### 9. Helper ancestry, configuration isolation and recovery remain acceptance gates

The ordinary launcher execs Node, but hooks run through Cursor's terminal executor, not a direct `child_process.spawn` in the CLI callsite. The hook executor constructs a shell command with direct-stdin or heredoc transport and invokes that executor with its native policy. **[INFERENCE] A direct OVRCR parent relationship cannot be concluded from launcher exec alone.** Capture the actual helper PID/PPID in an isolated native hook before choosing a trampoline. Persist/worker modes require their own ownership contract. [I1], [I3], [O1]

Do not copy Cursor's private terminal executor, reach into its live agent state, or weaken OVRCR to trust arbitrary descendants. If native execution adds another owner, use an explicitly authorized forwarding design or keep that mode unavailable until solved.

CLI configuration and data have separate override mechanisms. The installed paths module recognizes CURSOR_CONFIG_DIR/XDG_CONFIG_HOME and CURSOR_DATA_DIR. Do not assume changing only HOME isolates every native resource, or that moving CLI configuration necessarily proves where all hook sources load. Real tests must use explicit task-owned roots and avoid the live credential store. [I13], [D3]

A replacement reporter must fence earlier source generations and preserve server-owned Unread without reconstructing new Ready from history. Recovered visible requests need a baseline, not fresh alerts. The inspected public hook configuration does not supply a complete current interaction/activity snapshot; Unknown until a trustworthy new event may be a safer first contract than pretending full recovery.

## Superset's Cursor integration

Pinned reference: `superset-sh/superset@c3c6717bb7de1ce91dbf82505e9dce12c2c0324e`. This is the Cursor-specific adapter, not the generic helper used by Grok.

### What it does

- Installs a Cursor-specific hook script and merges owned entries into the user's `~/.cursor/hooks.json`, preserving other entries and root configuration. Removal targets its own command paths. The path helper uses OS home rather than a Cursor configuration override. [S1]
- Wraps `cursor-agent`, exports root identity through the shared wrapper and execs the real executable. The interactive preset is plain `cursor-agent`; unlike Superset's Grok preset, it does not add always-approve. Its noninteractive preset separately adds trust/read-only flags; those are not native interactive reporting requirements. [S1], [S3], [S4]
- Maps sessionStart/End to attachment/detachment, beforeSubmitPrompt to Start, Stop to Stop, beforeShellExecution/beforeMCPExecution to PermissionRequest, and Shell/MCP post-tool success/failure back to Start. The post-tool matcher intentionally excludes file tools. [S1]
- The shell helper takes the event name from argv and reads session_id from stdin. It rejects an already-established foreign root harness and otherwise distinguishes CLI from IDE using Cursor environment hints. [S2]
- Sends terminal/session identity to the existing unauthenticated host notification endpoint, with endpoint-manifest lookup and legacy fallback. The common host mapper/store use receipt-time events, not OVRCR capability/generation authority. [S2], [S5]

### Reuse versus adaptation

| Mechanism | Useful part | Difference required for OVRCR |
| --- | --- | --- |
| Owned merge/removal in hooks.json | Preserve unrelated entries; idempotent setup | Honor native path/consent rules and verify actual event delivery |
| Separate CLI and IDE identity | Do not confuse Composer with the native CLI | Admit a distinct OVRCR provider and verify aliases/ownership |
| Root wrapper identity guard | Drop nested foreign-harness reports | Also validate exact conversation/native generation and actual child metadata |
| Pre-shell/MCP attention mapping | Simple indication that a gate was reached | Not evidence that the user is being prompted; cannot implement exact WaitingInput |
| Post-tool Start | Simple working-state restoration | Does not close an exact Input request or handle all questions/plans |
| Stop mapping | A terminal-event transport exists | Retain completed/aborted/error; never turn all three into Ready |
| Host endpoint fallback | Attempts to survive host restart | Reuse OVRCR's authenticated Unix owner/lease; do not replace it with unauthenticated HTTP |

The isolated helper probe confirmed outcome collapse, dropped generation IDs, root-shaped child forwarding and foreign-root rejection. These are reasons to adapt the design, not a claim that Superset's entire product is broken. Its simpler working/attention semantics are different from OVRCR's agreed review and exact Input-request contract.

## OVRCR reuse boundary

At the inspected baseline, OVRCR already owns managed native invocations, report authentication, binding generations, activity/health snapshots, Codex Unread/Presented review and optional Ready alerts. `cursor-agent` is detectable, but there is no Cursor provider adapter or Cursor protocol provider variant in that baseline. [O1], [O2]

The provider-neutral Ready, Input-request snapshots and recovery work in [spec #88](https://github.com/xlyk/ovrcr/issues/88) and its tickets is planned, not assumed available. The [deep-modules refactor #77](https://github.com/xlyk/ovrcr/issues/77) changes the owners of readiness, request exchange and Dashboard/test behavior. Rebase and refresh that map before implementation; do not add another parallel reporting state store.

A future Cursor slice must cover the **actual picker launch**, source configuration/consent and native callback delivery. A detected executable or working hand-typed command alone does not satisfy managed launching. Preserve ordinary native aliases, workspaces, args, approvals, existing hooks and user configuration. Metrics are not prerequisites for this status milestone.

## Candidate product specification

This section is a draft for discussion, not approved implementation scope.

### Problem Statement

A user running Cursor CLI in OVRCR must watch the native terminal to discover new responses or input requests. Generic hook mapping can present errors as completion, miss automatic generations, lose child identity and mistake tool gates for visible questions.

### Solution

Integrate ordinary native Cursor CLI with OVRCR status, explicit Unread review and optional response/input attention alerts. Preserve the native UI, configuration and policy. Use outcome-bearing events and exact conversation/generation/request identity, with honest capability diagnostics wherever native public hooks are insufficient.

Start with installed CLI contracts rather than assuming parity with IDE/cloud hooks. Prove plugin/user/project delivery before selecting installation. Use a documented native observer or vendor-supported addition for real input visibility; do not infer it from pre-execution gates or terminal escapes.

### User Stories

1. As a Cursor CLI user, I want OVRCR's launch action to start a tracked session, so that reporting works without manually assembling commands.
2. As a user, I want Cursor CLI distinguished from IDE Composer and cloud workers, so that the correct session owns my status.
3. As a user, I want Ready to mean a response is available for review, so that it does not promise task success or finality.
4. As a user, I want errors and cancellation distinguished from completed responses, so that unsuccessful work does not create a success alert.
5. As a user, I want automatic follow-up generations tracked correctly, so that their missing submit hook does not hide work or corrupt ordering.
6. As a user, I want a blocked prompt reconciled without a false completion, so that admission hooks cannot leave misleading activity.
7. As a user, I want exact conversation and native generation identity preserved, so that an old ending cannot overwrite newer work.
8. As a user, I want one Unread identity per agreed response cycle and explicit review of Presented, so that delayed acknowledgement cannot clear a newer response.
9. As a user, I want child and nested foreign-harness callbacks excluded, so that a child cannot rebind or complete its parent.
10. As a user, I want actual visible unresolved approvals/questions to produce WaitingInput, so that ordinary tool execution is not mistaken for a human wait.
11. As a user, I want each input request to retain its identity through answer, rejection or cancellation, so that old requests cannot clear newer waits.
12. As a user, I want optional Input needed alerts distinct from Ready/Unread, so that answering a question is not treated as reviewing a response.
13. As a user, I want parallel approvals and asynchronous questions represented honestly, so that a pending request is not declared resolved when a tool returns early.
14. As a user, I want native plan decisions and startup trust prompts scoped explicitly, so that unsupported surfaces are not silently claimed as covered.
15. As a user, I want existing hooks, plugin sources, approvals and notification preferences preserved, so that reporting does not change my workflow or policy.
16. As a user, I want ordinary compatible upgrades attempted with actual-delivery diagnostics, so that a version string alone does not disable reporting.
17. As a user, I want safe session switching and reporter recovery without historical alert replay, so that reconnect does not create duplicate attention events.
18. As a user, I want private prompts, responses, commands, questions and user metadata omitted from reports, so that status integration does not leak content.
19. As a user, I want existing provider reporting and scheduled tasks preserved under fifty-session load, so that Cursor support does not regress other work.
20. As a user, I want missing accounting to remain unknown, so that status support does not falsely imply token/cost completeness.

### Implementation Decisions — proposed defaults

- Target ordinary foreground interactive Cursor CLI first; treat `agent` as a verified alias and avoid confusing the desktop `cursor` shim with the agent. Persist, worker, IDE/cloud and ACP scope must be explicit.
- Use distinct provider identity and the existing supervised report path. Keep native generation UUID separate from OVRCR binding generation and do not infer ordering from timestamps or lexical UUID order.
- Map stop outcomes deliberately: completed is a Ready candidate, aborted is cancellation, error is failure. Define automatic-follow-up cycle semantics before accepting unobserved-start generations.
- Prefer observed rather than confirmed settling claims. A completed hook fires before all follow-up decisions are applied; rejected input and exceptional error paths need reconciliation beyond a simple start/end pair.
- Verify `--plugin-dir` end to end before claiming invocation-only loading works. If local user/project entries are required, compose only the owned entries with consent and preserve existing configuration.
- Require real input-request presentation/resolution and safe current-state access for complete Input-needed behavior. Native internal request IDs are useful design references, not permission to import or monkeypatch private modules.
- Reuse existing OVRCR notification preferences and host delivery. Keep native Cursor notification settings unchanged unless the user explicitly chooses a duplicate-alert policy.
- Keep compatible upgrades enabled with capability/payload checks, known incompatibility diagnostics and runtime evidence. Loaded configuration does not prove the relevant lifecycle callsite runs.
- Recover only current state supported by the chosen source, fence retired senders and baseline history without new Unread/alerts. Unknown is safer than a fabricated idle/completion.
- Leave metrics outside the first milestone; preserve other providers and scheduled tasks; refresh shared owners after the refactor lands.

### Testing Decisions — proposed seams

- **Primary application seam:** the actual OVRCR Dashboard/managed CLI launching native Cursor CLI under isolated PTY/config/data/workspace/socket roots. Assert accepted runtime snapshots, visible Dashboard state, Presented review, native output/exit and owned cleanup.
- **Native lifecycle seam:** initial new/resume, human submission/block, successful response, stop-hook follow-up, abort/error race, exceptional exits and in-process new/resume/fork. Record actual script payloads and ordering; enum/schema presence is not execution evidence.
- **Hook-source seam:** user-only, project-only, plugin-only and mixed sources. Verify actual native delivery for start, response, stop and session events rather than testing only the executor or a config parser.
- **Native input seam:** visible tool approvals, AskQuestion sync/async/skip, plan decisions and any admitted startup dialog. Assert actual presentation and request closure with IDs; do not substitute preShell/preMCP or a terminal bell.
- Verify helper PID/PPID and lifecycle cleanup from the real hook terminal executor, including any selected persistent mode. Do not infer direct ancestry from the outer launcher.
- Reuse existing deterministic host-notification recording, followed by actual enabled native desktop/sound checks. Cover repeat events, close-before-send, visibility suppression, recovery baselines and unchanged Unread on input resolution.
- Exercise children, foreign Claude compatibility hooks, stale generation/session changes, ordinary unlisted versions, malformed/oversized payloads and source disconnection without logging private content.
- Preserve actual native policy: no force/yolo, blanket MCP approval, trust bypass, bundle patching or replacement ACP UI to obtain green results. Separate native macOS, Linux, module probes, source inspection and full interactive evidence.
- Run the current shared protocol/provider regressions and bounded fifty-session socket replay when implementation reaches acceptance. This research has not run those suites.

### Out of Scope — proposed

- IDE Composer, cloud/private workers, ACP replacement UI, native persistent sessions and cross-device ownership unless separately admitted.
- Token/cost/context accounting, quotas, billing, historical analytics or conversation recovery after server death.
- Inferring human waits from tool gates or terminal notifications; inferring finality from completed Stop alone; conflating Ask mode with AskQuestion.
- Modifying the installed bundle, importing private factories in production, monkeypatching tools/UI, forcing approvals or changing trust/notification settings silently.
- A new SDK, database, service, generic observability framework or competing runtime state owner.
- Final spec/ticket publication, vendor issue creation, implementation, installation, upgrade or merge before design confirmation.

### Further Notes

The current source suggests a narrower and more useful basic outcome path than the generic Superset adapter uses. Full status parity still needs admission/follow-up correlation, root/subagent certification and exact native input lifecycle.

Do not declare a vendor API universally absent from a rolling documentation page or one extracted caller. Conversely, do not promise support simply because an event is in the shared hook enum or a local plugin flag exists. Where a supported native observer is missing, the later design must name the dependency or reduce scope explicitly.

## Decision tree for grilling

These are questions for the later interview, not requests to answer now.

| ID | Independent decision | Recommended starting answer | What it unlocks |
| --- | --- | --- | --- |
| D1 | Is the first scope ordinary foreground interactive CLI only, or must native persist/worker modes be included? | Foreground CLI first; preserve aliases, defer modes with another lifecycle owner explicitly | Admission and process-ownership matrix |
| D2 | Does each completed automatic generation become reviewable, or must all known hook follow-ups remain one Busy response cycle? | Carry over suppression of known continuation noise unless the user prefers per-generation review | Start/end correlation and a possible post-hook/current-state export requirement |
| D3 | Is full true input visibility required initially, or may outcome reporting ship before it? | Keep exact native input behavior as an explicit separate capability; never substitute tool gates silently | Vendor observer requirement and independent delivery slices |
| D4 | Is a consented user/project hook merge acceptable if plugin-only lifecycle delivery is incomplete? | Yes; do not promise zero-setup plugin loading until verified | Installation/removal and source compatibility contract |
| D5 | Which input surfaces are required: tool approvals, normal questions, plan decisions, startup trust/MCP prompts? | At least in-session approvals and questions; scope plan/startup surfaces explicitly | Native request/presentation identity and acceptance cases |
| D6 | Should Cursor's native notification settings remain untouched alongside OVRCR alerts? | Preserve them; let the user explicitly decide how to avoid duplicate alerts | Notification ownership and preference documentation |
| D7 | Is this status/review/attention only, with optimistic upgrade support and accounting deferred? | Yes, as proposed for the other harnesses | Bounded completion criteria and diagnostics |

Dependent decisions:

- After D1/D2: Is first trustworthy prompt admission sufficient, or must launch/resume/new-chat attachment be immediate? What happens when a generation starts without beforeSubmitPrompt?
- After D2: Which native event proves no further known hook follow-up is queued, and what cancellation/error precedence is actually demonstrated? Do not settle that race from receipt order.
- After D3/D5: Where can a supported observer distinguish requested from presented and preserve query/approval identity through closure? ACP availability does not answer passive native-UI access.
- After D4: Which native hook/config directory is authoritative under custom environment roots, and how are mixed user/project/plugin/team hooks handled without altering unrelated entries?
- Before final approval: What can be reconstructed after plugin/config reload or native session switch, and when should Unknown persist until another authoritative event? Confirm the Hermes/Grok/Pi/OMP carryovers rather than assuming parity.

## Sources and reproducible anchors

### Official rolling documentation

[D1]: https://cursor.com/docs/hooks
[D2]: https://cursor.com/docs/cli/reference/parameters
[D3]: https://cursor.com/docs/cli/reference/configuration
[D4]: https://cursor.com/docs/cli/acp
[D5]: https://cursor.com/docs/reference/third-party-hooks
[D6]: https://cursor.com/docs/cli/using

The compatibility guide [D5] explicitly lists Claude Notification and PermissionRequest as unsupported. CLI usage [D6] distinguishes Ask mode, normal interactive approvals, ACP and print-mode output. None of these rolling pages pins the installed build's exact caller behavior.

### Installed build `2026.08.31-4057e58`

These links identify actual installed containers. The following module names are preserved webpack IDs; they are the stable anchors for this analysis, not public TypeScript source URLs.

[I1]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/cursor-agent
[I2]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/6041.index.js
[I3]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/190.index.js
[I4]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/6041.index.js
[I5]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/index.js
[I6]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/6041.index.js
[I7]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/6041.index.js
[I8]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/2336.index.js
[I9]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/6041.index.js
[I10]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/index.js
[I11]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/6041.index.js
[I12]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/6863.index.js
[I13]: /Users/xlyk/.local/share/cursor-agent/versions/2026.08.31-4057e58/index.js

| Reference | Original module / methods inspected |
| --- | --- |
| I2 | `./src/after-agent-hooks.ts`: user/project guard and after-response/thought emission; directly exercised in the guard probe |
| I3 | `../hooks-exec/dist/index.js`: executeHookForStep, executeAllAndMerge, executeCommandScript, loop-limit guard, generic/shell/MCP hook wrappers, request enrichment |
| I4 | `./src/ui.tsx`: generation assignment, beforeSubmitPrompt, success/abort/error Stop, hook follow-up queue, native question handlers and chat switches |
| I5 | `../hooks/dist/index.js`: hook enum, matching, response validation/normalization, default limits and compatibility |
| I6 | `./src/run-agent.tsx`: initial session identity, executor configuration, initial-new sessionStart and memoized cleanup/sessionEnd |
| I7 | `./src/pending-decision-store.ts`: request/approve/reject/clearAll and process-local decision IDs |
| I8 | `./src/utils/interaction-utils.ts`, `./src/utils/interaction-responses.ts`, `./src/shared/unified-approval-policy.ts` |
| I9 | `./src/components/ask-question-form.tsx`, `ask-question-tool-ui.tsx`, `create-plan-tool-ui.tsx` and native root UI |
| I10 | `./src/components/approval-dialog.tsx`, `./src/workspace/approval.tsx`, `./src/mcp/approval.tsx` startup dialogs |
| I11 | `./src/notifications/use-notify-on-needs-input.ts`, `notifications-context.tsx`, `config-gated-sender.ts`, `deduping-sender.ts`, `factory.ts`; finish caller not fully traced |
| I12 | ACP `ask-question-handler.ts` and `create-plan-handler.ts`; separate-mode response mapping only |
| I13 | `../cursor-config/dist/paths.js`: CLI config and data roots |

Do not quote the temporary readability-file line numbers as original vendor source lines. The readable files and synthetic fixtures are disposable; no private module import is part of the proposed production integration.

### Pinned Superset and OVRCR context

[S1]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/agent-setup/src/agent-wrappers-cursor.ts
[S2]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/agent-setup/templates/cursor-hook.template.sh
[S3]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/agent-setup/src/agent-wrappers-common.ts
[S4]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/shared/src/builtin-terminal-agents.ts#L192-L200
[S5]: https://github.com/superset-sh/superset/blob/c3c6717bb7de1ce91dbf82505e9dce12c2c0324e/packages/host-service/src/events/map-event-type.ts
[O1]: ../crates/ovrcr-runtime/src/agent_runner.rs
[O2]: ../crates/ovrcr-protocol/src/agent.rs

Related briefs: [Grok and Superset](grok-status-reporting-2026-09-13.md), [Hermes](hermes-status-reporting-2026-09-12.md). These are parallel investigations, not proof that their native mechanisms apply to Cursor.
