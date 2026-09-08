# Schedule agent tasks

Build and install the updated OVRCR binary. Install Pi separately and configure
a model with `pi` before scheduling it. Pi 0.84.4 is the validated RPC baseline.
Task commands require the updated server; stop the old server intentionally
before upgrading. Service installation asks a managed server to stop gracefully
and refuses while sessions or task runs remain; pass `--kill-sessions` to
terminate them. It never touches an unmanaged server.

## Create a task

For a repository task, register the project first. Each run fetches the named
remote branch and creates a new worktree and branch:

```sh
ovrcr task create daily-review --project my-project --remote origin --branch main \
  --cron '0 9 * * MON-FRI' --timezone America/Los_Angeles \
  --model PROVIDER/MODEL --prompt-file ./review-prompt.txt
```

For a task that needs no repository, use a fresh scratch directory:

```sh
ovrcr task create research --scratch --every 6h \
  --model PROVIDER/MODEL --prompt 'Research the topic and save your findings here.'
```

Replace `PROVIDER/MODEL` with a model configured in Pi. Prompt files are copied
into the task when created or updated; subsequent edits to the original file
do not change the task. A task definition, including its prompt, is limited to
256 KiB; lists page internally so retained history can exceed the socket frame limit.

Use `--at 2026-10-01T09:00:00-07:00` instead of `--cron` or `--every` for a
one-time task. Durations accept positive integers followed by `s`, `m`, `h`, or
`d`. Cron requires five fields and an explicit IANA timezone. Each task runs
with thinking `off` and a `1h` timeout unless `--thinking` or `--timeout` overrides
it. Discovered extensions, skills, prompt templates, and project settings are
disabled; Pi still reads project instruction files and uses its global model
and authentication configuration. OVRCR explicitly loads its bundled bash lifecycle
extension so background commands remain owned until run cleanup.

## Inspect and change tasks

```sh
ovrcr task list
ovrcr task get 1 --json
ovrcr task update 1 --prompt-file ./revised-prompt.txt --timeout 2h
ovrcr task pause 1
ovrcr task resume 1
ovrcr task concurrency 3
ovrcr task run 1
```

`task run` returns a run record with its durable ID. Manual runs leave the
recurring schedule unchanged and work for paused tasks too. Lowering the
concurrency limit does not stop active runs. The same task never overlaps
itself; repeated requests coalesce into at most one pending run.

Schedules continue while the dashboard is closed. After sleep or downtime,
missed occurrences coalesce into one run. Intervals keep their original cadence.
For fixed daily cron times, a skipped spring-forward time runs at the end of
the gap, and a repeated fall-back time runs once. Task details include upcoming
occurrence timestamps.

Pause cancels scheduled pending work and leaves active runs alone. Resume uses
the next future occurrence. Updating a task affects future runs; already queued
or active runs retain their captured instructions. Delete refuses active work,
cancels pending runs, and keeps history:

```sh
ovrcr task delete 1
```

## Follow a run and review its files

```sh
ovrcr run list --task 1
ovrcr run get 1
ovrcr run logs 1 --follow
ovrcr run logs 1 --json
ovrcr run cancel 1
```

Human output includes incremental response text and tool activity. JSON logs
are newline-delimited event records. Closing a viewer leaves the run alive.
Task lists and retained logs can be read without starting the server.

Completion means Pi settled successfully and process cleanup finished. Provider
errors, truncated generation, cancellation, timeout, and interruption are
reported separately. A cancelled run shows `Cancelling` until its supervisor
confirms cleanup. If the supervisor does not confirm within 20 seconds, the
server terminates it along with every process it recorded and reports
`CleanupFailed`; inspect `processes.json` and the logs in the run directory. Background processes started by the bash tool can remain alive
between tool calls and are terminated when the run ends. A server crash interrupts active work; a manual rerun
starts a fresh directory and conversation. Later scheduled occurrences still
run normally.

Working files remain available after success or failure. Clean a completed Git
worktree only after reviewing its changes; Git cleanup refuses dirty or occupied
worktrees and preserves the branch. Scratch cleanup removes the working files
and requires explicit confirmation. Both retain run records and transcripts:

```sh
ovrcr run clean 1
ovrcr run clean 2 --yes
```

Run history is bounded. The scheduler keeps the newest 200 runs of each task
and every run that finished within the last 30 days. Older finished runs are
removed together with their run directory, which holds the transcript and, for
scratch tasks, the working files; copy anything you need within that window.
Git worktrees live outside the run directory and are not deleted, but once the
run record is gone the worktree must be removed with `ovrcr workspace remove`.
A run whose `run.json` cannot be read is reported as `Interrupted` with the
file path in its error; the server still starts and other runs continue.

## Run the scheduler at login

```sh
ovrcr service install
ovrcr service status
ovrcr service stop
ovrcr service start
ovrcr service uninstall
```

The per-user service uses launchd on macOS and systemd on Linux. It starts at
login and restarts after failure. Intentional clean shutdown stays stopped
until the next explicit start or login. It cannot execute while the machine is
asleep or before user login.

Installation records absolute executable and configuration paths, plus a
non-secret execution environment. For credentials supplied through environment
variables, provide `--environment-file /absolute/path/to/env-file`. Keep that
file private. OVRCR reads it as environment values rather than executing it as
a shell script. Credentials are not copied into task records or service files.
Pi credentials must be available to the service account without a terminal.

Uninstalling the service preserves tasks, history, and working directories.
An already running unmanaged server must be stopped intentionally before service
installation. `OVRCR_CONFIG` selects a separate registry and task store;
`OVRCR_SOCKET` selects an isolated server. Custom registries also receive distinct
OS service names. `OVRCR_PI_EXECUTABLE` can select a Pi
binary for testing or a nonstandard installation.

## Manage tasks in the TUI

In browse mode, press Ctrl-t to open Tasks; Esc returns to the terminal dashboard. The Tasks
view provides creation and editing, pause/resume, run-now, deletion, concurrency,
run history, streaming transcripts, cancellation, and confirmed working-file
cleanup. Use n/e to create/edit, Tab or Shift-Tab to move between fields, Enter
for a prompt newline, and Ctrl-s to save. Use h for history and PgUp/PgDn to
scroll the transcript. Follow the on-screen key hints. Task output uses its own control
connection and does not change terminal selection.

## Validate an installation

The optional probes retain their isolated fixtures for inspection:

```sh
python3 scripts/validate-pi-rpc.py /absolute/path/to/ovrcr /absolute/path/to/pi --background
python3 scripts/validate-service.py /absolute/path/to/ovrcr
```

The Pi probe uses a local streaming provider and checks that output arrives before
completion. With `--background`, it also verifies that a real Pi bash background
command stays alive between tool calls and is gone after successful run cleanup. The macOS service probe installs a uniquely named temporary LaunchAgent,
executes a scheduled fixture run, checks crash restart, and uninstalls it while
preserving task history. Linux service definitions have automated contract tests;
the native systemd lifecycle needs validation on a Linux host.
