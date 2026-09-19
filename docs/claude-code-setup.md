# Set up Claude Code reporting

Use stable Claude Code **>=2.1.267** and an interactive foreground
invocation. OVRCR supports a fresh invocation or an initial resume using the
separate-token form `--resume UUID`, where `UUID` is the known canonical lowercase
RFC 4122 UUIDv4. 2.1.268 and later compatible patches also support the separate-token form `-r UUID`.
The short form remains unavailable on 2.1.267. Equals syntax, picker/name/search
values, and positional prompts on resume are excluded. Fresh invocations retain
their existing safe options and single positional prompt. Continue, print mode,
background/fork mode, and in-process conversation switching are not supported
reporting paths. See the
[support matrix](agent-reporting-support.md) for evidence and remaining gaps.

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

When `claude` is on `PATH` and the settings file under `CLAUDE_CONFIG_DIR` (default
`~/.claude`) lacks these synchronous hooks, the dashboard shows a one-line warning
in its error banner at startup naming the file and the setup command to run.

Check the supplied file and executable:

```sh
ovrcr agent doctor claude --json --settings ~/.claude/settings.json
```

For a nonstandard Claude executable, add `--executable /path/to/claude`. Doctor
reports `compatible_versions` separately from `tested_versions`, plus
`version_compatible` and `version_tested`, the detected version when
the probe returns a recognized Claude version line, and separate `unsupported`
or `unavailable` probe status. Its `resume_forms` field describes the forms for
the detected supported version. Doctor never starts a server and does not certify
all effective settings sources. The probe waits at most one second for the
executable's version line. When it misses that budget, doctor reports
`unavailable` and a managed launch prints `agent admission unavailable;
running native command`, then runs the native command. A heavily loaded host
can cause this with a supported version, so rerun on a quieter host before
treating it as a version problem.

## Launch and inspect

Launch the supervisor inside an OVRCR session:

```sh
ovrcr new --project demo --workspace hooks --name agent -- ovrcr agent run --provider claude -- claude
```

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

On 2.1.268 and later compatible patches, the short spelling preserves the same native argv:

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

Patch compatibility is policy, not native acceptance evidence. See [provider version policy](agent-versions.md).
