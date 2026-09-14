# Agent reporting

OVRCR does not guess what an agent is doing. A provider hook reports activity and
context occupancy explicitly. The dashboard shows the last accepted activity;
context occupancy is available through session inspection.
Terminal output, elapsed silence, keyboard input, and process liveness never imply
that an agent is busy or idle.

## Managed Claude Code

For the supported version, setup instructions, and current acceptance limits, see
[Claude Code setup](claude-code-setup.md) and the
[support matrix](agent-reporting-support.md).

The supervised integration binds one initial Claude Code conversation to one
invocation. Activity, current context, cumulative usage, cost, and reporting health
are separate observations. `Stop` means observed idle, not confirmed completion.
A matching `permission_prompt` notification means observed waiting for input;
`PermissionRequest` alone does not establish that state.

Usage covers recognized records from the root transcript and remains **partial**,
even after EOF, Stop, or process exit. It does not certify all auxiliary work or
billing. Input tokens already include cache-read and cache-write subsets; do not
add those subsets again. Cost is independently scoped, source-reported estimated
conversation cost. Missing values are unknown, while explicit zero remains zero.
Component ages are independent. Native source freshness is uncertain; receiving a
callback does not prove it is the newest provider sample.

The dashboard keeps the full `estimate` label when the metrics row fits. In a
narrow or split pane it uses `est` so scoped cost and both component freshness
labels remain visible. If that row is still too wide, it also shortens `tokens`
to `tok` and removes one separator space while preserving the scopes, values,
coverage, and freshness labels.

Inspect the complete observation with `ovrcr session usage SESSION_ID` or terminal
inventory. Both retain the raw binding, activity, metrics, and source health.
Reporting transport can be connected while usage or cost is unknown. A conversation
switch such as `/clear` makes the old binding unavailable; it does not create a
new binding automatically. Start a fresh supervised invocation to track another
conversation.

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

Managed sessions of a supported readiness provider (Codex, Pi, Oh My Pi) retain one
unread identity for its latest root Ready observation. [Mark-reviewed](dashboard.md#unread-responses)
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

The adapter reads only the provider JSON on standard input. It maps
`SessionStart` and `Stop` to `idle`, prompt and tool events to `busy`,
`PermissionRequest` to `waiting-input`, `StopFailure` to `error`, and `SessionEnd`
to `unknown`. Events containing `agent_id`, unknown events, and notifications are
ignored. Malformed input, an unavailable server, and report timeouts are fail-open
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
`session_start` binds the conversation now in the foreground as a fresh reporting
generation: blank activity, no requests, and the server-owned unread response untouched.
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

`ovrcr agent run omp -- omp [ARGS...]` (or picking `omp` in the Dashboard) materializes the
owned reporting extension into a private per-invocation directory and adds `-e <path>`
beside your own extensions; nothing under `~/.omp` changes. The extension is inert without
the private channel and outside the terminal UI, so in-process task and advisor children
(mode `print`) and the ACP route (mode `rpc`) report nothing.

Oh My Pi has no settled event. The extension reports `session_start` (Idle), `agent_start`
(Busy), and `agent_end`: an end that declares a continuation keeps the cycle open and the
session Busy, while an end without one closes it and publishes Ready
(`response ready · observed`), Error or Idle from the same outcome rule as Pi. The quality
is Observed, never Confirmed, because the stop hook may still continue after a clean end; a
later `agent_start` then opens another cycle without acknowledging or replacing the unread
response the earlier one created.

Oh My Pi switches conversations in place on one extension instance. A `session_switch`
re-announces the conversation with `session_start` carrying the switch reason, which rebinds
and invalidates any cycle left open by the conversation being left. An existing unread
response survives the switch until it is reviewed. Payloads carry identifiers and
discriminants only — the one field added for Oh My Pi is the boolean continuation flag —
never prompts, responses or tool data. Any `session_shutdown` ends reporting for that
process: Oh My Pi does not re-run extension factories, so a reload or replacement leaves
the invocation unreported until the next managed launch (reporter recovery is #96).

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

Samples are kept in memory with their receipt time. Freshness is advisory and uses
the wall clock: a clock earlier than the receipt marks the sample stale, and
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
