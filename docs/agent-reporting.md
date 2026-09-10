# Agent reporting

OVRCR does not guess what an agent is doing. A provider hook reports activity and
context occupancy explicitly, and the dashboard shows the last accepted report.
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
labels remain visible.

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

Accepted states are `unknown`, `idle`, `busy`, `waiting-input`, and `error`. The
command has a one-second total deadline, and successful reports are silent,
including with `--json`. OVRCR retains the last accepted report in memory and
exposes that observation across dashboard detach and reconnect.

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

Before any report is accepted, the session's context is unknown: the dashboard
shows `ctx —` and inspection returns `context_usage: null` with `stale: null`. An
accepted report whose usage or capacity is unknown is still a stored sample, but
the dashboard keeps showing `ctx —`; a later complete report replaces it.

### Freshness

Samples are kept in memory with their receipt time. Freshness is advisory and uses
the wall clock: a clock earlier than the receipt marks the sample stale, and
wall-clock changes may shorten or extend its apparent freshness. A sample is fresh
while its receipt age is under five minutes. At five minutes or more, or as soon as
its session exits, the dashboard appends `~` to the value (for example, `25%~`).
The same state appears as `stale: true` in inspection and `context_stale: true` in
terminal inventory. A server restart loses the live sample.

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
