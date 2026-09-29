# Agent reporting

OVRCR does not guess what an agent is doing. A provider hook reports activity and
context occupancy explicitly. The dashboard shows the last accepted activity;
context occupancy is available through session inspection.
Terminal output, elapsed silence, keyboard input, and process liveness never imply
that an agent is busy or idle.

Every managed provider reports through one reporter lifecycle: the reservation it binds,
the Reporting generation that binding carries, and the revisions its observations are
numbered from work the same way for Claude Code, Codex, Pi and Oh My Pi, so those
guarantees hold identically wherever they are repeated below. Which of the rest a
provider uses is not uniform, and the differences are real: Pi and Oh My Pi pause and
recover, Codex and Claude Code do not; Claude Code settles its usage accounting when the
native command exits, the others simply release the reservation. Each provider's section
states what that provider does.

## Managed Claude Code

`ovrcr agent run claude -- claude` (or picking `claude` in the Dashboard) supervises
the launch through the same managed reporting route as Codex, Pi and Oh My Pi.
A plain `claude` launch stays untracked. Explicit custom argv overrides are left
unchanged.

For the supported version, setup instructions, and current acceptance limits, see
[Claude Code setup](claude-code-setup.md) and the
[support matrix](agent-reporting-support.md).

The supervised integration binds one initial Claude Code conversation to one
invocation. Activity, current context, cumulative usage, cost, and reporting health
are separate observations. A matching root `Stop` with the current `prompt_id`
marks Ready (`response ready · observed`), not confirmed settling or task success.
Claude may continue after Stop; a later prompt or tool event under that turn returns
Busy without clearing Unread. Duplicate Stop under the same Root-turn identity does
not create another Unread or alert. Child events and Stops without a matching
`prompt_id` cannot create root Ready. A matching root `permission_prompt` notification opens an Approval Input request
(`approval:{prompt_id}`), so effective activity is WaitingInput and an input-needed
alert can fire once per request identity; the next attributable root activity closes
it. `PermissionRequest`, idle notifications, and bare tool events alone do not open a
request. AskUserQuestion remains unavailable until a distinct question open/close
surface is verified.

Managed Codex reports Approval Input requests from a verified root
`PermissionRequest` (`approval:{turn_id}`) the same way: WaitingInput effective
activity, one input-needed alert per request identity, and close on the next
attributable root activity. `PreToolUse`/`PostToolUse` alone never open a request.
Questions remain unavailable (no distinct question surface). Supported in-process
`SessionStart(source=clear|resume)` replacements rebind Ready/Input to the new
foreground conversation; `source=compact` invents no replacement; conflicting
`source=startup` while bound is `identity_transition_unavailable` (fork/backtrack
gap). See [Codex setup](codex-reporting-setup.md).

Usage covers recognized records from the root transcript and remains **partial**,
even after EOF, Stop, or process exit. It does not certify all auxiliary work or
billing. Input tokens already include cache-read and cache-write subsets; do not
add those subsets again. Cost is independently scoped, source-reported estimated
conversation cost. Missing values are unknown, while explicit zero remains zero.
Component ages are independent, and each one counts from the last sample whose
value actually changed: a timer replay of an unchanged provider sample keeps the
age it already had, so a session whose totals keep moving never ages out. No
provider certifies sample order, so receiving a callback does not prove it is the
newest provider sample; the receipt age is the only provenance a reader gets.

The dashboard keeps the full `estimate` label when the metrics row fits. In a
narrow or split pane it uses `est` so scoped cost and both component stale labels
remain visible. If that row is still too wide, it also shortens `tokens` to `tok`
and removes one separator space while preserving the scopes, values, coverage,
and stale labels. A component is labelled ` stale` once its age reaches five
minutes or its session exits; a fresh component carries no label.

Inspect the complete observation with `ovrcr session usage SESSION_ID` or terminal
inventory. Both retain the raw binding, activity, metrics, and source health.
Reporting transport can be connected while usage or cost is unknown. Supported in-process conversation replacements (`/clear`, in-process resume, and
foreground `/branch` when native leave + `SessionStart(source=fork)` evidence is
present) rebind the same managed invocation to the new foreground identity and
retire obsolete request/turn state without acknowledging Unread. Ambiguous forms
such as a bare fork SessionStart make reporting unavailable with an actionable
warning while Claude keeps running; start a fresh supervised invocation to obtain
a new lease after that warning.

## Legacy unbound reporting

The commands below retain the earlier PTY-scoped reporting contract for sessions
without a supervised agent binding. Their event mappings and freshness rules are
not the managed Claude contract above.

Every managed session receives `OVRCR_HOOK_SOCKET`, `OVRCR_SESSION_ID`, and
`OVRCR_HOOK_TOKEN` in its child environment. The `report` commands require those
inherited variables, use only the inherited hook socket, and never start a server.

Start each root provider process as its own managed session, so its inherited
capability identifies the correct PTY:

```sh
ovrcr new --project demo --workspace hooks --name agent -- claude
```

Do not share one OVRCR PTY between independent agent roots. OVRCR cannot infer
provider causality when callbacks race or arrive late.

## Activity

```sh
ovrcr report activity --state busy --sequence 1
ovrcr report activity --state waiting-input --sequence 2
ovrcr report activity --state idle --sequence 3
```

Accepted states are `unknown`, `idle`, `busy`, `waiting-input`, `response-ready`, and `error`. The
command has a one-second total deadline, and successful reports are silent,
including with `--json`. OVRCR retains the last accepted report in memory and
exposes that observation across dashboard detach and reconnect.

`response-ready` means the last observed root turn has finished responding. Managed
reporters display `response ready · observed` or `· confirmed`, by sample quality,
on the selected session's metadata line, with a `✓` glyph in the sidebar, and retain it until the next activity
report, including across reconnects. Reporter health and process exit remain
separate; Ready does not acknowledge unread output or imply known usage or cost.

Managed sessions of a supported readiness provider (Codex, Pi, Oh My Pi, Claude)
retain one unread identity for its latest root Ready observation. [Mark-reviewed](dashboard.md#unread-responses)
is explicit and checks that identity before clearing it. Selecting or viewing a
terminal, new Busy activity, and reporter loss do not clear unread state. The server
retains it through dashboard reconnect but does not persist it across server death.
Manual activity reports and other providers do not create unread observations.

Reports without `--sequence` are accepted in arrival order; the first such report
selects receipt mode for the whole PTY lifetime. Supplying `--sequence` selects
sequenced mode, which must advance the session's sequence. The two modes cannot
mix, and sequence counters cannot restart between turns. This provides ordering
checks, without causal ordering, exactly-once delivery, health claims, retries, or
an implicit timeout meaning for provider work.

### Legacy Claude Code activity hooks

Claude Code command hooks can translate supported hook events into activity
reports. Add these entries manually to the existing `~/.claude/settings.json`,
keeping unrelated settings and handlers. If `ovrcr` is not on the provider's
`PATH`, replace it with the installed absolute path.

```json
{
  "hooks": {
    "SessionStart": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "UserPromptSubmit": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PreToolUse": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PermissionRequest": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PostToolUse": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PostToolUseFailure": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "Stop": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "StopFailure": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "SessionEnd": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}]
  }
}
```

The adapter reads only the provider JSON on standard input. On this legacy
unbound path it maps `SessionStart` and `Stop` to `idle`, prompt and tool events
to `busy`, `PermissionRequest` to `waiting-input`, `StopFailure` to `error`, and
`SessionEnd` to `unknown`. That idle mapping is not the managed Claude contract
above, where a correlated root Stop is Ready · observed. Events containing
`agent_id`, unknown events, and notifications are ignored. Malformed input, an unavailable server, and report timeouts are fail-open
and produce no stdout; add `--verbose` for a bounded diagnostic on stderr.

A successful adapter invocation means only that its local report attempt was
handled. It does not approve a provider operation or claim that a callback was
delivered exactly once. These are OVRCR's default observations rather than an
upstream provider state-machine guarantee: `Stop` is a last observation, so a
later callback can report that execution continued.

To uninstall, remove only these handlers from the existing settings file and
restart the agent session. OVRCR never installs or modifies provider settings.

## Pi

`ovrcr agent run pi -- pi [ARGS...]` (or picking `pi` in the Dashboard) materializes the
owned reporting extension into a private per-invocation directory and adds `-e <path>`
beside your own extensions (Pi's documented merge rule for an explicit `-e` path; not
exercised by an automated test); nothing under `~/.pi` changes. The extension is inert without
the private channel and in print, JSON and RPC modes. It reports `session_start` (Idle),
`agent_start` (Busy), and at `agent_settled` one of Ready (`response ready · confirmed`,
because Pi settles only after retries, automatic compaction and queued continuations),
Error (the last assistant message stopped with an error) or Idle (aborted, or no assistant
response). Ready creates one unread identity per response cycle; review and alerts work as
for Codex. An extension dialog's outer prompt span is reported as an
[input request](dashboard.md#input-requests): the session waits, whatever activity was
underneath is kept and restored on close, and a background request can raise one
**OVRCR · input needed** alert. Pi has no approval or question surface of its own
beyond these dialogs. Payloads carry identifiers and discriminants only: never
prompts, responses, tool data or prompt titles.

### Transitions and recovery

Pi replaces the whole extension factory when a session is replaced (new, resume, fork) and
when extensions reload. The producer that shut down is retired, and its successor's
`session_start` binds the conversation now in the foreground. A replacement is a fresh
reporting generation: blank activity, no requests, and the server-owned unread response
untouched. An extension reload keeps the same conversation and therefore the same
generation; its open cycle and requests were already closed by the retiring producer.
The announcement names the conversation it left by id, so a transition OVRCR never saw is
detected rather than assumed. Returning to an earlier conversation is a new generation too,
never the reuse of the old one, and every later frame from a retired producer is ignored
however high its source sequence. Compaction inside the same conversation changes nothing.
Tree navigation abandons the response cycle it was in (`cycle_invalidated`): activity falls
back to what Pi's own API answers — Idle when it says the session is idle, otherwise
Unknown — and a historical response is never replayed as a new Ready.

When live reporting stops being certain, the reporter pauses instead of ending: a hole in
the source sequence (`source_gap`), a producer replaced without its shutdown being observed
(`producer_replaced`), the extension's own bounded queue overflowing (`source_overflow`),
or a transition naming a conversation OVRCR is not bound to (`transition_mismatch`). A
paused reporter publishes health `Unavailable` with that reason, keeps its lease and its
binding, applies nothing, and leaves the native session running; the open request set is
cleared, because a reporter that is unsure cannot vouch for a dialog. Nothing is inferred
from silence, idleness, empty queues or elapsed time, and no timer retries.

Recovery is one forced rebind at a trustworthy boundary: the next `session_start` or
`agent_start`, or the `/ovrcr-reattach` command the extension registers, which re-announces
the producer from inside the already-loaded extension and says whether it worked. Each
recovery is a fresh generation that starts blank: activity comes only from Pi's public API,
the cycle identity waits for a genuine `agent_settled`, and restored requests and responses
raise no alerts — later genuine ones alert normally. An earlier unread response survives.
See [Pi reporting setup](pi-reporting-setup.md).

## Oh My Pi

Native macOS evidence for Oh My Pi reporting is recorded in
[research/issue-97-omp-native](../research/issue-97-omp-native/README.md); see the support
record's "Native verification" section for what it proves and what stays open.

`ovrcr agent run omp -- omp [ARGS...]` (or picking `omp` in the Dashboard) materializes the
owned reporting extension into a private per-invocation directory and adds `-e <path>`
beside your own extensions; nothing under `~/.omp` changes. The extension is inert without
the private channel and outside the terminal UI, so in-process task and advisor children
(mode `print`) and the ACP route (mode `rpc`) report nothing.

Oh My Pi has no settled event. The extension reports `session_start` (Idle), `agent_start`
(Busy), `agent_end`, `session_switch`, `session_tree` and `session_shutdown`: an end that
declares a continuation keeps the cycle open and the session Busy, while an end without one
closes it and publishes Ready (`response ready · observed`), Error or Idle from the same
outcome rule as Pi. The quality is Observed, never Confirmed, because the stop hook may
still continue after a clean end; a later `agent_start` then opens another cycle without
acknowledging or replacing the unread response the earlier one created. Payloads carry
identifiers and discriminants only — the one field added for Oh My Pi is the boolean
continuation flag — never prompts, responses or tool data.

### Transitions and recovery

Oh My Pi switches conversations in place on one extension instance. A `session_switch`
(`new`, `resume`, `fork`) re-announces the conversation with `session_start` carrying the
switch reason and the conversation it left, named by id, so a transition OVRCR never saw
pauses with `transition_mismatch` rather than being assumed; a cancelled transition emits
nothing and rebinds nothing. A switch to another conversation is a fresh reporting
generation, and returning to an earlier one is a new generation too, never the reuse of the
old. A same-file reload keeps the generation: Oh My Pi's `reload()` switches to the file it
is already on, so the conversation did not change and there is no other binding to check —
but it still abandons the cycle it interrupted and closes any request left open.
Compaction inside the same conversation changes nothing. A plugin-resource refresh changes
nothing either: it reloads plugin roots, agents, skills, slash commands and MCP servers, but
never re-instantiates an extension factory, so no shutdown or start is reported and no
rebind happens. Tree navigation abandons the response cycle it was in
(`cycle_invalidated`): activity falls back to what Oh My Pi's own API answers — Idle when it
says the session is idle, otherwise Unknown — and a historical response is never replayed as
a new Ready. An existing unread response survives every one of these. Updating the
extension's own code still needs a native restart, and reporter reattachment is not a
conversation reload.

When live reporting stops being certain, the reporter pauses instead of ending: a hole in
the source sequence (`source_gap`), a producer replaced without its shutdown being observed
(`producer_replaced`), the extension's own bounded queue overflowing (`source_overflow`), or
a transition naming a conversation OVRCR is not bound to (`transition_mismatch`). A paused
reporter publishes health `Unavailable` with that reason, keeps its lease and its binding,
applies nothing, and leaves the native session running; the open request set is cleared,
because a reporter that is unsure cannot vouch for a dialog. Nothing is inferred from
silence, idleness, empty queues or elapsed time, and no timer retries.

Recovery is one forced rebind at a trustworthy boundary: the next `session_start` or
`agent_start`, or `/ovrcr-reattach`, the command the extension registers and you type in Oh
My Pi. Each recovery is a fresh generation that starts blank: activity comes only from Oh My
Pi's public API — Unknown while it says the session is not idle — the cycle identity waits
for a genuine later cycle, and restored requests and responses raise no alerts while later
genuine ones alert normally. A reattach during a response does not restore that response's
cycle, so the in-flight response will not become Ready; the next prompt reports normally. An
earlier unread response survives. Because Oh My Pi has that command, a bounded-queue
overflow is a recoverable pause for it too, ended by `/ovrcr-reattach`. Any
`session_shutdown` still ends reporting for that process, and only a fresh managed launch
brings it back.

### Approvals and questions

Oh My Pi reports two kinds of [input request](dashboard.md#input-requests), both
only for the interactive root conversation. A native tool-approval prompt opens
`approval:<tool call id>` on `tool_approval_requested` and closes it on
`tool_approval_resolved`, whether the answer was allow, deny or a cancellation —
a denial closes that one approval and is not by itself a failed run. Approval
events carry the session id they belong to, and OVRCR reports only those whose id
is the interactive root's, so an in-process task or advisor child cannot open or
close a root request.

A question is the ask tool's own execution: `question:<tool call id>` opens at
`tool_execution_start` with `toolName` `ask` and closes at `tool_execution_end`,
whatever the outcome — answered, redirected to chat, cancelled, timed out,
aborted or errored. One execution is one request however many surfaces it walks,
so a multi-question form, a selector and an editor fallback are all the same
request. **This is tool lifetime, not dialog visibility**: the request opens a
moment before the dialog is visible, and it covers a question queued behind
another dialog. OVRCR claims no visibility beyond that, and there is no version
gate. Tool events carry no session id at all, so what keeps in-process children
silent is the same terminal-UI check that silences the rest of their reporting.

Frames carry the namespace, the request identity and the kind. The approval
reason, the question text, the answer and every tool argument or result stay
inside Oh My Pi.

**Side effect:** registering the two approval handlers disables Oh My Pi's
speculative read execution. Its speculation gate refuses while any tool lifecycle
handler is registered, so a managed launch trades that optimization for approval
reporting.

## Current model (Pi and Oh My Pi)

A managed Pi or Oh My Pi session publishes the model the session itself reports —
on `session_start` when `getModel()` already answers, and on every `model_select`
after a mid-session switch. The Dashboard sidebar shows `pi:<model>` or
`omp:<model>`. Until the first report the row stays the agent name alone. The
model never comes from the launch flag or from terminal text. Claude's existing
model report is unchanged.

## Context usage

A context report completely replaces the stored context sample. Every accepted
report replaces the model, conversation, usage, capacity, and receipt timestamp of
the previous sample.

```json
{
  "source": "generic",
  "model": "agent-model",
  "conversation": "conversation-id",
  "used_tokens": 25000,
  "capacity_tokens": 200000
}
```

```sh
ovrcr report context --stdin-json < context.json
```

Fields other than `source` are optional; send `null` or omit a field to clear that
value in the replacement. Unknown keys, negative or fractional counts, zero
capacity, empty or overlong identifiers, and control characters are rejected.
Accepted `source` values are `generic` and `claude_code_statusline`.

Each context stream has its own ordering state, independent of activity. The first
report without `--sequence` selects receipt order for the entire PTY lifetime, and
later unsequenced reports are accepted in arrival order. The first sequenced report
must use a positive value and later values must increase strictly. A stream cannot
switch between receipt and sequenced modes or restart its counter between turns. An
unsequenced adapter, including the Claude status-line adapter, can prove only
receipt order.

Before any report is accepted, the session's context is unknown: inspection
returns `context_usage: null` with `stale: null`. An accepted report whose usage or
capacity is unknown is still a stored sample; a later complete report replaces it.

### Freshness

Samples are kept in memory with their receipt time, which this legacy path sets
to the arrival time of every accepted report: a replay of an unchanged sample
does advance it here, unlike the managed component ages above. Freshness is
advisory and uses the wall clock: a clock earlier than the receipt marks the sample stale, and
wall-clock changes may shorten or extend its apparent freshness. A sample is fresh
while its receipt age is under five minutes. At five minutes or more, or as soon as
its session exits, inspection returns `stale: true` and terminal inventory returns
`context_stale: true`. A server restart loses the live sample.

### Inspect one session

```sh
ovrcr session context SESSION_ID
```

This always emits bare JSON and does not start a server:

```json
{
  "session": 7,
  "context_usage": {
    "report": {
      "source": "generic",
      "model": "agent-model",
      "conversation": "conversation-id",
      "used_tokens": 25000,
      "capacity_tokens": 200000
    },
    "received_unix_ms": 1770000000000
  },
  "stale": false
}
```

### Legacy Claude Code status line

Claude Code can supply status-line context. Apply this manually in a disposable
Claude configuration and preserve any existing status-line setting.

```json
{"statusLine":{"type":"command","command":"ovrcr report claude-context --stdin-json"}}
```

See the [Claude Code status-line documentation](https://code.claude.com/docs/en/statusline#manually-configure-a-status-line)
for the provider setting. The adapter prints `ctx N%` only after the report is
accepted. Invalid input, unavailable transport, and a rejected report return a
nonzero error, unlike the fail-open activity adapter above.

Context is source-reported current occupancy. For Claude status-line input, OVRCR
uses `context_window.current_usage.input_tokens`,
`cache_creation_input_tokens`, and `cache_read_input_tokens` when all three are
present, and sums those counters as `used_tokens`. It does not use output tokens, a
provider percentage, a model-name capacity fallback, or `total_input_tokens` as a
cumulative billing counter.
