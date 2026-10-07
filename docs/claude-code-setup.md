# Set up Claude Code reporting

Use an interactive foreground Claude Code invocation; no Claude Code version is
required or refused. OVRCR supports a fresh invocation or an initial resume using the
separate-token form `--resume UUID` or `-r UUID`, where `UUID` is the known canonical lowercase
RFC 4122 UUIDv4. Equals syntax, picker/name/search
values, and positional prompts on resume are excluded. Fresh invocations retain
their existing safe options and single positional prompt. Continue, print mode,
and background/fork launch flags remain unsupported reporting paths.

In-process conversation replacements are followed only when native hooks name a
trustworthy foreground identity: `/clear` (`SessionEnd(reason=clear)` then
`SessionStart(source=clear)`), in-process resume (`SessionStart(source=resume)` after
an observed leave), and foreground `/branch` (`SessionEnd` of the current conversation
then `SessionStart(source=fork)`). Same-conversation `/compact` does not invent a
replacement. A bare `SessionStart(source=fork)` without that leave is treated as the
unobservable background-fork gap and makes reporting unavailable while Claude keeps
running. See the [support matrix](agent-reporting-support.md).

## Compose the settings

Print a proposed configuration from your existing settings file:

```sh
ovrcr agent setup claude --print --settings ~/.claude/settings.json > /tmp/ovrcr-claude-settings.json
```

If you have no existing file, omit `--settings`. Review the printed JSON and merge
it into the intended Claude settings file. Setup itself never writes that file.
`just run` does one narrower write after it installs `~/.local/bin/ovrcr`: it
replaces the `ovrcr` path inside existing managed reporter commands, including
the status line, so those hooks call the binary it just installed. It does not
add hooks or change permissions. A command that already names that binary is
left unchanged.
Keep your permissions, trust policy, unrelated hooks, and renderer settings.
Avoid loading the same merged configuration twice through different settings
sources, which can duplicate existing hooks.

The generated status line carries `"refreshInterval": 60`. Claude Code re-runs
the status line on that timer while the session is idle, so an idle managed
session keeps its `rate_limits` (the Quota left block) current. Setup keeps a
`refreshInterval` you already set, and running setup on its own output changes
nothing:

```json
"statusLine": {
  "type": "command",
  "command": ": ovrcr-managed-claude-v1; '/path/to/ovrcr' report claude-statusline --stdin-json",
  "refreshInterval": 60
}
```

Generated OVRCR command hooks are synchronous. Check any project, plugin, or
managed settings that can add or override hooks: inspecting one file does not
prove Claude's effective configuration. Setup preserves an external status-line
renderer using `--render-command`. It migrates an exact legacy
`ovrcr report claude-context --stdin-json` command. A wrapper containing that
legacy reporter is left untouched with manual migration instructions on stderr.

When `claude` is on `PATH` and the settings file under `CLAUDE_CONFIG_DIR` (default
`~/.claude`) lacks these synchronous hooks, the Dashboard shows a persistent
one-line warning in its error banner at startup naming the file and the setup
command to run. The same banner reports a missing Codex reporter configuration
when `codex` is on `PATH`. Configuration presence is not hook trust or delivery;
`agent doctor` keeps those fields `unverified` until evidence arrives. Ready is
available as observed root-turn response end (`completion_quality: observed`);
Confirmed settling remains unverified. Approval Input requests are available from a
verified root `Notification(permission_prompt)` (identity `approval:{prompt_id}`);
closing follows the next attributable root activity. AskUserQuestion / elicitation
Input requests remain unavailable: native hooks do not yet provide a distinct
question open/close identity separate from approvals (see support matrix).

Check the supplied file and executable:

```sh
ovrcr agent doctor claude --json --settings ~/.claude/settings.json
```

For a nonstandard Claude executable, add `--executable /path/to/claude`. Doctor
reports the detected version when the probe returns a recognized Claude version
line (`probe_status` `probed`, otherwise `unavailable`), `tested_versions` and
`version_tested` as evidence, and `version_gate: "none"`. Its `resume_forms` field
is `--resume` and `-r` on every release. Doctor never starts a server and does not
certify all effective settings sources. The probe waits at most one second for the
executable's version line; a managed launch never runs it.

## Launch and inspect

Pick **claude** in the Dashboard agent picker, or launch the supervisor inside an
OVRCR session. Both use the same managed reporting contract:

```sh
ovrcr new --project demo --workspace hooks --name agent -- ovrcr agent run claude -- claude
```

The legacy `agent run --provider claude -- claude` form also works. A plain
`claude` launch stays untracked even when hooks are configured.

To resume a known conversation, preserve the native Claude arguments exactly:

```sh
ovrcr new --project demo --workspace hooks --name resumed -- ovrcr agent run --provider claude -- claude --resume 5ebc5f9b-54b5-4928-9955-dc81c23743dd
```

OVRCR does not add `--session-id` to this form. Reporting binds only after a
matching root `SessionStart` callback with `source=resume` supplies its transcript
path. Existing
recognized assistant rows in that transcript remain partial conversation usage;
new distinct rows are added once, while estimated status-line cost remains an
independent measurement.

The short spelling preserves the same native argv:

```sh
ovrcr new --project demo --workspace hooks --name resumed -- ovrcr agent run --provider claude -- claude -r 5ebc5f9b-54b5-4928-9955-dc81c23743dd
```

Use one root agent per PTY. The supervisor preserves Claude's terminal and native
exit behavior. Missing reporting transport lets Claude run without tracking;
an explicit reservation conflict refuses a second reporting owner.

Inspect the session ID shown in `ovrcr list`:

```sh
ovrcr session usage SESSION_ID
ovrcr agent doctor claude --json --settings ~/.claude/settings.json --session SESSION_ID
```

The dashboard shows observed activity (Busy, Ready · observed, WaitingInput,
Error), Unread for the latest correlated root Ready turn, current context, partial
token usage, and estimated cost when available. Unknown values remain unknown. A
connected reporter does not imply complete accounting or Confirmed settling. Read
the [reporting contract](agent-reporting.md) for scope and freshness semantics.
Claude's explicit `<synthetic>` assistant records (including interruption/error
messages) are excluded from partial usage counting; they are not metered API
responses. Other malformed usage and foreign-conversation records still make the
source unavailable.

Supported in-process clear / resume / foreground-branch replacements rebind
reporting to the new foreground conversation under a fresh Reporting generation.
An unsupported or ambiguous transition shows `identity_transition_unavailable`
with `ovrcr agent doctor claude --json` recovery guidance and leaves Claude running;
start a fresh supervised invocation only when you need a new lease after that
warning. Detaching the dashboard leaves the running invocation intact; server death
does not preserve live sessions or reporting state.
Doctor retains the binding details but reports `session_status=reporting_unavailable`
and a recognized diagnostic code in `source_health.reason` when reporting is
unavailable; unknown/private provider reason text remains redacted. An identity
freeze names the ambiguous foreground conversation and explains that configuration
repair cannot recover that reporting lifetime. It does not restart or interrupt
the native agent.

## Remove the integration

Remove only hook handlers whose command begins with
`: ovrcr-managed-claude-v1; `. Preserve other handlers in each group. Restore the
previous status-line command from the quoted `--render-command` argument, or
remove the marked default status line. Start subsequent sessions with `claude`
directly when supervision is no longer wanted.

Patch compatibility is policy, not native acceptance evidence. See [provider version policy](agent-versions.md).
