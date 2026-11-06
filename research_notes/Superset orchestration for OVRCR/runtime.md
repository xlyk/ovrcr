# Superset orchestration for OVRCR

Research date: 2026-11-02 (America/Los_Angeles). Runtime review is pinned to `ParadevAI/Paradev` commit [`4cb2bf762b462001b2e535c9955624cd9c2bf0ed`](https://github.com/ParadevAI/Paradev/commit/4cb2bf762b462001b2e535c9955624cd9c2bf0ed). The separately published skills repository is pinned to [`ParadevAI/skills` commit `a56e0162c5631d9859e70872323bf986f4a926c6`](https://github.com/ParadevAI/skills/commit/a56e0162c5631d9859e70872323bf986f4a926c6). The skill inventory is documented separately; this note covers runtime mechanisms and their limits.

## Does `superset:orchestrate` implement a scheduler or teach an agent to coordinate?

### Takeaway

It teaches the currently running coordinator agent to fan work out through existing CLI operations. The backend supplies workspace, agent-session, terminal, hook-status, and review surfaces; the cited orchestration path does not create or persist a parent/child workflow graph. Automations are a separate trigger-and-dispatch feature.

### Cited Findings

- The orchestration guide describes the coordinator as Claude Code or Codex, tells it to use `workspaces create`, `agents create`, and `terminals read`/`send`, and expects workers to report completion or blockage for coordinator verification. [Orchestration guide, lines 6–36](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/orchestration.mdx#L6-L36)
- The agent launch input carries workspace, agent, prompt, provider session resume/fork options, and surface, but no coordinator ID, parent task ID, or child-workflow ID. [AgentRunInput, lines 138–168](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/agents/agents.ts#L138-L168)
- Workspace creation accepts an optional task link and a list of agent launches. Launches are dispatched with `Promise.all`; each result is returned as success plus session data or an error. [Workspace schema, lines 125–148](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/workspaces/workspaces.ts#L125-L148), [dispatch implementation, lines 5–52](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/workspace-creation/shared/dispatch-agents.ts#L5-L52)

The documented control flow is:

```mermaid
sequenceDiagram
  participant C as Coordinator agent
  participant CLI as Paradev CLI
  participant H as Host or relay API
  participant W as Worker workspace and PTY
  participant Hooks as Agent lifecycle hooks
  C->>CLI: workspaces create(branch, agent, bounded prompt)
  CLI->>H: workspaces.create
  H->>W: create/reuse workspace; launch worker session
  W-->>Hooks: lifecycle events
  C->>CLI: agents wait / terminals read
  CLI->>H: inspect hook binding and terminal liveness or screen
  H-->>C: state, terminal text, or timeout/exit
  C->>CLI: terminals send(follow-up text or keys)
  CLI->>H: write input to worker PTY
  C->>C: interpret report, verify branch/tests, decide integration
```

The sequence summarizes the documented CLI workflow and the implementations cited in the following sections; it is not a server-owned job state machine.

### Inferences

- The coordinator's task decomposition, “done/blocked” report format, and verification checklist are prompt-level behavior. The runtime launches and observes sessions but does not interpret a worker report as a durable workflow transition.
- A coordinator interruption/restart has no documented worker graph to reload. A replacement coordinator can rediscover workspaces and terminals, but must reconstruct its own plan and task-to-worker mapping.
- “Full terminal context” in the orchestration guide should be read carefully: the CLI handoff implementation seeds from recent terminal context and caps it at 36,000 characters by default, rather than guaranteeing an unbounded full transcript. [CLI handoff options, lines 1327–1335](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L1327-L1335), [handoff implementation, lines 103–130](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/cli/src/commands/agents/create/command.ts#L103-L130)

### Gaps

- This is source/documentation research at the pinned revision; no live Superset account, provider launch, or end-to-end orchestration run was exercised.
- No claim is made that the source search proves no other product-internal workflow exists outside the reviewed host/CLI paths. The documented `superset:orchestrate` route itself is CLI fan-out.

## What launches a worker, and what isolation does it provide?

### Takeaway

The canonical worker is an agent process started in a workspace, normally a separate branch/worktree. A workspace may launch several independent sessions. Worktrees isolate checked-out files and branches; they are not an OS or network security boundary.

### Cited Findings

- CLI dispatch creates a workspace on a named branch and can launch an agent in the same call. JSON reports the workspace ID and, for each requested agent, `ok`, `error`, and terminal `sessionId`. The command can exit successfully even when an individual launch failed, so callers must check each `ok`. [CLI getting started, lines 150–188](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/getting-started.mdx#L150-L188)
- The direct CLI launch is `superset agents create --workspace WORKSPACE_ID --agent claude --prompt ...`; its result uses `sessionId`, which is the Paradev terminal ID. It supports fresh prompt, provider session resume, provider session fork, or recent-terminal-context handoff, with at most one session operation. [CLI agent commands, lines 1320–1363](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L1320-L1363)
- The host API's `agents.run` input accepts `workspaceId`, agent preset/config, prompt, optional model/effort/mode/attachments, resume/fork IDs, and optional `continueTerminalId`; the latter reuses a still-running matching session from the same caller. It has no parent-agent field. [AgentRunInput, lines 138–168](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/agents/agents.ts#L138-L168)
- MCP offers `workspaces_create` with `hostId`, `projectId`, checkout, branch/PR/task links, optional `agents[]`, and command. Each launch has agent, prompt, and optional model, effort, and attachment IDs. `agents_create` launches into an existing workspace and accepts an optional provider `resumeSessionId`; its returned `sessionId` is explicitly the Paradev terminal ID, distinct from the provider's own session ID. [MCP workspace tool schema, lines 15–50 and 113–128](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/mcp/src/tools/workspaces/create.ts#L15-L50), [MCP agent tool schema, lines 9–75](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/mcp/src/tools/agents/create.ts#L9-L75)
- Local project workspaces use Git worktrees; cloud workspaces use clones. A worktree still shares machine processes, network access, and files outside the worktree. [Workspaces concept, lines 6–8](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/workspaces.mdx#L6-L8), [FAQ, lines 50–60](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/faq.mdx#L50-L60)
- The MCP workspace tool says cloud workspaces accept at most one Claude/Codex launch. The host dispatcher itself maps all supplied launches concurrently and has no per-request count check in the cited dispatch path. [MCP workspace tool description, lines 47–50](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/mcp/src/tools/workspaces/create.ts#L47-L50), [host dispatch, lines 25–52](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/workspace-creation/shared/dispatch-agents.ts#L25-L52)

Representative CLI sequence:

```bash
superset workspaces create --project PROJECT_ID --name task-a --branch task/task-a \
  --local --agent claude --prompt "Implement only the bounded task; report changes and checks."
superset agents wait --workspace WORKSPACE_ID --terminal TERMINAL_ID --until settled
superset terminals read --workspace WORKSPACE_ID --terminal TERMINAL_ID
superset terminals send --workspace WORKSPACE_ID --terminal TERMINAL_ID \
  --text "Run the targeted checks and report their exact results." --wait
```

The first command's JSON must be checked for `agents[].ok`; a returned `sessionId` is the `TERMINAL_ID` for the later commands. [CLI getting started, lines 178–205](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/getting-started.mdx#L178-L205)

### Inferences

- “One branch/worktree per worker” is a practical file-conflict boundary, not containment. A safe OVRCR replication should preserve the existing OS process/network boundary policy separately from checkout isolation.
- There is no orchestration-level concurrency limit in the reviewed fan-out path. Do not infer unlimited capacity from that: provider, host, organization, and product quotas were not measured here.
- A “worker” in the skill's CLI workflow is a separate terminal agent session; provider-native child subagents are a different feature described below.

### Gaps

- Provider startup behavior, model/account quotas, cloud machine isolation, and practical maximum fan-out were not tested.
- MCP cloud provisioning is asynchronous and billed until deletion; MCP docs direct callers to poll until workspace status is ready. [MCP workspace tool description, lines 47–50](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/mcp/src/tools/workspaces/create.ts#L47-L50)

## What does status, wait, read, and send actually mean?

### Takeaway

`agents.wait` observes lifecycle-hook bindings and terminal liveness. “Settled” means not currently starting or working; it does not mean the requested work succeeded. `terminals.send` writes into the PTY, not a durable mailbox. The coordinator must inspect the output and independently verify work.

### Cited Findings

- Status is hook-derived. Paradev installs lifecycle hooks and wrappers; status only works for agents launched through Paradev, and support for waiting/input signals varies by provider. [Agent status docs, lines 17–27](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/agent-status.mdx#L17-L27)
- `agentWaitState` derives state from the binding's last hook event and whether the terminal process is alive. Its pure `settled` predicate matches every state except `working` and `starting`, and the `idle` predicate also matches `exited`; after a send, a newer hook timestamp is required. The command layer separately rejects an observed `exited` state, so an exited worker is not reported as a successful wait result. [Wait-state implementation, lines 21–75](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/cli/src/lib/agent-wait.ts#L21-L75), [wait command exit handling, lines 44–71](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/cli/src/commands/agents/wait/command.ts#L44-L71)
- `agents.wait` returns `{terminalId,state,lastEventType}`, defaults to a 600-second timeout, returns immediately if the requested state already holds, and throws on terminal/agent exit or timeout. [CLI wait reference, lines 1388–1418](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L1388-L1418)
- Polling is every 500 ms. After a send, no new hook event for 10 seconds is treated as a stalled prompt (for example, text left in the input box); agent exit and overall timeout are separate errors. [Agent binding poll, lines 13–71](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/cli/src/lib/agent-binding.ts#L13-L71), [send command, lines 65–127](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/cli/src/commands/terminals/send/command.ts#L65-L127)
- A `Stop` event maps to idle; the CLI's own explanation says it means the turn ended, either by finishing or interruption. `PermissionRequest` maps to blocked. `Failed` is recognized as a separate visual status, but `agentRunState`'s default maps it to idle. Tests explicitly prove that a permission request is “settled.” These are state labels, not a pass/fail assertion. [Shared status mapping, lines 22–53](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/shared/src/agent-status.ts#L22-L53), [wait tests, lines 10–42](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/cli/src/lib/agent-wait.test.ts#L10-L42), [agent explanation example, lines 1422–1439](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L1422-L1439)
- `terminals.send` accepts `workspaceId`, `terminalId`, text, and `submit` (default true). With a live agent binding the host validates that binding and writes a bracketed paste and Enter to its PTY; without a live binding it writes framed terminal input. Raw keys use `writeInput`. [Terminal router, lines 162–208](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/terminal/terminal.ts#L162-L208), [PTY send implementation, lines 1187–1309](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal/terminal.ts#L1187-L1309)
- `terminals read` returns current screen plus recent scrollback as text. `agents read` attempts the saved provider transcript. The host `snapshot` composes workspace/terminal/agent-state data and desktop pane layout; it does not include workspace files or screen pixels. [CLI terminal read, lines 1533–1554](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L1533-L1554), [agent transcript read, lines 1365–1385](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L1365-L1385), [snapshot result, lines 1646–1679](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L1646-L1679)
- Public MCP tools include `terminals_send` and `terminals_read`, but no `agents_wait` tool. MCP callers must poll available workspace/terminal reads or drive waits through the CLI. [MCP capability list, lines 14–26](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/mcp-server.mdx#L14-L26), [MCP tool catalog, lines 312–327](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/mcp-server.mdx#L312-L327)

### Inferences

- Do not map `settled`, `idle`, `blocked`, or a “completion” hook directly to successful task status. An exited worker is a command error; a stopped worker can have been interrupted; and a `Failed` event collapses to the script-facing `idle` state. Success needs an explicit worker result plus OVRCR-owned checks/evidence.
- PTY send has no message ID, delivery receipt, or durable mailbox semantics in this interface. A timeout after input can leave uncertainty about whether the model received it; an OVRCR dispatcher should record its own send/attempt state and reconcile from transcript/output before retrying.
- A screen read is lossy/current terminal state; it is not a canonical worker result. Prefer a durable output artifact or structured server result for restart-safe collection.

### Gaps

- Provider hook delivery reliability, event ordering across providers, and whether every provider's `Failed` event is wired identically were not live-tested.
- MCP does not expose the CLI wait primitive in the cited public catalog; the complete polling/error ergonomics over MCP were not exercised.

## How are tasks, external workers, and native child agents related?

### Takeaway

The durable external-worker hierarchy is workspace → terminal session → agent binding. A workspace may link to a task, but the launch input has no parent-worker relationship. Superset's native Claude/Codex subagent roster is a separate UI/status feature, is in-memory, and is not the skill's durable worker graph.

### Cited Findings

- Workspaces have a `taskId` field, and the SDK describes it as a link from the workspace to a task. Workspace rows are worktree/local/session identities; terminal bindings identify the agent process and terminal. [Workspace schema, lines 125–148](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/workspaces/workspaces.ts#L125-L148), [workspace DB fields, lines 255–278](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/db/schema.ts#L255-L278), [SDK create contract, lines 137–167](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/sdk/reference.mdx#L137-L167)
- The runtime's `TerminalSubagent` is keyed by the provider hook's child ID and is explicitly in-memory; it lives and dies with the parent's session. The store records hook events without changing the parent binding's lifecycle state. [Subagent types, lines 26–49](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/types.ts#L26-L49), [subagent event store, lines 318–380](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/store.ts#L318-L380)
- The harness registry has specialized parsers only for `claude` and `codex`, falling back to a generic parser for other agent IDs. Claude child hooks use the parent's session file and a child transcript path; Codex children use their own thread/rollout file. [Harness registry, lines 18–37](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/subagent-harnesses/index.ts#L18-L37), [Claude harness, lines 109–145](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/subagent-harnesses/claude.ts#L109-L145), [Codex harness, lines 132–139](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/subagent-harnesses/codex.ts#L132-L139)
- Tests show children expire after ten minutes without activity and the in-memory roster is capped at 64 entries per terminal. That cap bounds UI roster storage; it is not a concurrent-worker limit. [Roster tests, lines 177–203](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/store.test.ts#L177-L203), [staleness/cap constants, lines 73–90](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/store.ts#L73-L90)

### Inferences

- The child roster should not be used as a substitute for durable OVRCR child-task records: it has no independent worktree/result/retry/approval lifecycle and is discarded with the parent session.
- Provider parity has at least two dimensions: external launch support is configurable by installed agent preset, while hook state and child-transcript parsing vary. A generic fallback is not proof that a provider's child events or success semantics are fully supported.
- Superset's orchestration skill's worker tree is assembled by the coordinator choosing workspaces and terminals; provider-native children are created inside one agent session and observed through hooks.

### Gaps

- The exact Claude/Codex provider hook formats can drift as provider CLIs change. Source tests encode the current parsing assumptions but no provider-version compatibility matrix was run.

## What survives retries, a host restart, or an agent crash?

### Takeaway

Workspace reuse and agent-session resume are distinct. Some host/session records and provider session IDs are persisted, and the host can resume supported agents after a terminal crash. There is no corresponding durable state for the orchestration coordinator's decomposition, worker report, or verification decision.

### Cited Findings

- The terminal-agent persistence layer stores the agent/provider session ID, last event, and end markers. A startup sweep marks exited/disposed terminal bindings as ended so they can be considered for resume. [Persistent binding fields, lines 25–65](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/persistence.ts#L25-L65), [defunct sweep, lines 494–555](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/terminal-agents/persistence.ts#L494-L555)
- Resume uses an atomic candidate claim and an in-flight map to coalesce concurrent calls. It relaunches with the saved provider session ID when the configured agent supports resume; a failed launch unclaims the candidate for retry, and success disposes the old terminal and broadcasts the new terminal ID. Startup considers only recent candidates and a bounded number per pass. [Resume implementation, lines 101–202 and 248–281](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/terminal-agents/terminal-agents.ts#L101-L202), [resume tests, lines 143–181 and 254–359](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/terminal-agents/terminal-agents.test.ts#L143-L181)
- On a Linux host, the documented `KillMode=process` setup keeps the terminal daemon alive when the host service restarts, so the next start can reconnect to the same terminals. [Host server docs, lines 90–120](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/host-server.mdx#L90-L120)
- Worktree create reuses a workspace already associated with the requested branch. Without a repeated client ID, the request still carries its agent launches; only an already-existing `checkout: local` workspace suppresses launches. A low-level create request that repeats the same `input.id` returns the workspace with empty `terminals` and `agents` before dispatch. [Branch reuse, lines 1136–1144](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/workspaces/workspaces.ts#L1136-L1144), [same-ID retry, lines 661–708](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/workspaces/workspaces.ts#L661-L708), [agent replay logic, lines 1301–1304 and 1338–1384](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/host-service/src/trpc/router/workspaces/workspaces.ts#L1301-L1304)
- The documented CLI create options and the MCP `workspaces_create` schema do not expose that `id` field. [CLI create options, lines 794–818](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/cli/cli-reference.mdx#L794-L818), [MCP workspace schema, lines 47–129](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/packages/mcp/src/tools/workspaces/create.ts#L47-L129)

### Inferences

- A retry of the skill's ordinary branch-based CLI call can relaunch the worker in a reused workspace; workspace reuse is not worker-dispatch idempotency. The MCP tool has the same practical limitation in its exposed schema.
- OVRCR should persist a stable orchestration/run ID, each child launch ID, request payload, state transitions, terminal/session mapping, and collected evidence. A provider session resume is useful recovery for one conversation, but does not resume an interrupted coordinator's fan-out plan.
- If a terminal process survives the host API restart, there is no reason to relaunch it solely because the host process restarted. If it died, provider resume support and provider state availability determine recovery; the host preserves the candidate when it cannot resume.

### Gaps

- The host's resume constants and exact persisted database retention policy were not evaluated against long outages. Automatic resume is bounded; this is not a general retry queue.
- The `input.id` idempotency path is a host-router capability, not present in the cited public CLI/MCP create schemas. Other SDK internals may supply it, but that was not assumed here.

## How do MCP and scheduled automations fit?

### Takeaway

MCP is an alternate remote control surface for task/workspace/agent/terminal operations, authenticated for an organization. Automations dispatch one configured agent prompt from events or schedules. Neither is the `superset:orchestrate` child-worker scheduler.

### Cited Findings

- The public MCP catalog includes task, workspace, agent launch, terminal send/read, automation, project, host, and organization tools. It has no exposed worker wait tool. [MCP capability list, lines 14–26](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/mcp-server.mdx#L14-L26), [MCP tool catalog, lines 303–327](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/mcp-server.mdx#L303-L327)
- MCP uses OAuth 2.1 for interactive browser authorization or an API key for headless use. The docs state API keys grant full organization access. [MCP authentication, lines 240–288](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/mcp-server.mdx#L240-L288)
- Automations run an agent session for an event or schedule; the configured prompt receives event context. Each run records a handoff status: `created` is documented as workspace creation, `missed` when no device was online and not retried, and `failed` for a broken handoff. Webhook triggers have no de-duplication, and schedules advance even when a run is missed/failed. [Automation overview/prompt setup, lines 6–45](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/automations.mdx#L6-L45), [webhook behavior, lines 70–90](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/automations.mdx#L70-L90), [run state, lines 108–119](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/automations.mdx#L108-L119)

### Inferences

- MCP supports an OVRCR-style coordinator that calls remote workspace and agent primitives, but the orchestration/wait/retry policy must be implemented by the coordinator/server because the MCP tool list has no agent wait or workflow-completion API.
- Automation `created` should not be interpreted as coding success: it marks workspace handoff. A schedule or webhook that must retry safely needs its own durable event identity, dedupe policy, and retry strategy.

### Gaps

- Subscription enforcement, webhook event loss under provider retries, and schedule service behavior beyond the cited documented status semantics were not exercised.
- API key rotation/least-privilege options were not assessed; the docs characterize the key as full-organization access.

## What should OVRCR replicate, and what reuse does the repository license allow?

### Takeaway

For runtime parity, reuse the useful user-facing workflow (preflight, bounded worker prompts, separate branches, progress inspection, verification, and review) while keeping OVRCR's orchestration state server-owned. Do not treat Superset's prompt-level skill, terminal hooks, or native child roster as a durable scheduler. Both inspected repositories carry Elastic License 2.0 (ELv2).

### Cited Findings

- The orchestration guide calls for mechanical fan-out with clear acceptance criteria and results the coordinator can verify; the coordinator verifies worker reports before integrating. [Orchestration guide, lines 6–8 and 26–36](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/apps/docs/content/docs/orchestration.mdx#L6-L8)
- The application repository license grants use/copy/distribute/derivative-work rights subject to limits, including no third-party hosted/managed service providing substantial product functionality, preservation of notices, and prominent modification notices. [Application license, lines 1–27](https://github.com/ParadevAI/Paradev/blob/4cb2bf762b462001b2e535c9955624cd9c2bf0ed/LICENSE.md#L1-L27)
- The separately published skills repository has the same ELv2 terms at its pinned revision. [Standalone skills license, lines 1–27](https://github.com/ParadevAI/skills/blob/a56e0162c5631d9859e70872323bf986f4a926c6/LICENSE.md#L1-L27)

### Inferences

- Replication plan for OVRCR:
  1. Keep skill text/instructions versioned and separately testable from runtime APIs; skills provide policy and task framing, not durable execution state.
  2. Let OVRCR's server-owned parent workflow create child records with stable IDs, assign one bounded task per child workspace/process, and persist request, branch, child terminal/provider session IDs, status evidence, retry count, and result artifact.
  3. Define success from explicit structured worker output plus required OVRCR checks/review; treat process exit, idle, blocked, hook loss, and timeout as distinct outcomes.
  4. Make dispatch and send retries idempotent with stable operation keys and reconciliation before retry. A reused branch alone is insufficient.
  5. Keep provider status/parser capability explicit. The source's general agent support does not guarantee uniform hook completion semantics or native child-roster parity.
  6. Keep file isolation and process/security isolation as separate policies, and make event/schedule deduplication and retry rules explicit if OVRCR adds recurring dispatch.
- Reimplementing behavioral ideas from the documented flow is different from copying licensed source or skill text. The license permits derivative works subject to its restrictions, but whether a particular OVRCR deployment falls within them is a legal question; use counsel for source-copying or hosted-service decisions.

### Gaps

- This is a technical license summary, not legal advice. No repository-level subdirectory license exceptions were reviewed beyond the two repository license files.
- No OVRCR code was changed and no capability was live-tested; this document is a source-backed replication plan only.

## Research checkouts

- App source: `/tmp/superset-runtime-research`
- Standalone skill source (shared checkout created by the skill inventory researcher): `/tmp/superset-agent-skills-research`

