# Set up Claude Code reporting

Use Claude Code **2.1.267** and a fresh interactive invocation. Resume, continue,
print mode, background/fork mode, and in-process conversation switching are not
supported reporting paths. See the [support matrix](agent-reporting-support.md)
for evidence and remaining acceptance gaps.

## Compose the settings

Print a proposed configuration from your existing settings file:

```sh
ovrcr agent setup claude --print --settings ~/.claude/settings.json > /tmp/ovrcr-claude-settings.json
```

If you have no existing file, omit `--settings`. Review the printed JSON and merge
it into the intended Claude settings file. The command never writes that file.
Keep your permissions, trust policy, unrelated hooks, and renderer settings.
Avoid loading the same merged configuration twice through different settings
sources, which can duplicate existing hooks.

Generated OVRCR command hooks are synchronous. Check any project, plugin, or
managed settings that can add or override hooks: inspecting one file does not
prove Claude's effective configuration. Setup preserves an external status-line
renderer using `--render-command`. It migrates an exact legacy
`ovrcr report claude-context --stdin-json` command. A wrapper containing that
legacy reporter is left untouched with manual migration instructions on stderr.

Check the supplied file and executable:

```sh
ovrcr agent doctor claude --json --settings ~/.claude/settings.json
```

For a nonstandard Claude executable, add `--executable /path/to/claude`. An
unsupported or unavailable version probe is reported explicitly. Doctor never
starts a server and does not certify all effective settings sources.

## Launch and inspect

Launch the supervisor inside an OVRCR session:

```sh
ovrcr new --project demo --workspace hooks --name agent -- ovrcr agent run --provider claude -- claude
```

Use one root agent per PTY. The supervisor preserves Claude's terminal and native
exit behavior. Missing reporting transport lets Claude run without tracking;
an explicit reservation conflict refuses a second reporting owner.

Inspect the session ID shown in `ovrcr list`:

```sh
ovrcr session usage SESSION_ID
ovrcr agent doctor claude --json --settings ~/.claude/settings.json --session SESSION_ID
```

The dashboard shows observed activity, current context, partial token usage, and
estimated cost when available. Unknown values remain unknown. A connected reporter
does not imply complete accounting. Read the [reporting contract](agent-reporting.md)
for scope and freshness semantics.

After `/clear` or another identity transition, start a fresh supervised invocation
to restore tracking. Detaching the dashboard leaves the running invocation intact;
server death does not preserve live sessions or reporting state.

## Remove the integration

Remove only hook handlers whose command begins with
`: ovrcr-managed-claude-v1; `. Preserve other handlers in each group. Restore the
previous status-line command from the quoted `--render-command` argument, or
remove the marked default status line. Start subsequent sessions with `claude`
directly when supervision is no longer wanted.
