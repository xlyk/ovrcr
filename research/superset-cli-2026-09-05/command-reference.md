# Superset CLI command reference

Captured 2026-09-05. Installed desktop-bundled CLI **1.25.1**. All **76 executable commands** below were discovered both from the installed recursive help tree and the matching release source. Aliases are alternate spellings and are not counted twice. Runtime behavior and limits are explained in [the research report](report.md).

## Global behavior

`superset <command> [subcommand] [arguments] [options]`

| Option | Meaning |
| --- | --- |
| `--json` | Formatted data payload, without a result envelope. Errors still use stderr text and exit 1. |
| `--quiet` | IDs for objects with an `id`; other values fall back to JSON. Do not assume every create operation prints just an ID. |
| `--api-key <key>` | Credential override; also `SUPERSET_API_KEY`. With `auth login`, stores the key. |
| `--help`, `-h` | Static contextual help. |
| Root `--version`, `-v` | Binary version. `update --version <version>` instead selects an update target. |

Agent/CI detection defaults to JSON unless quiet is requested. Bare invocation in an ordinary TTY opens a guided command browser. The browser shows the generated command before execution.

Group aliases: `workspaces` = `ws`; `terminals` = `term`; `scripts` = `presets`; `tasks` = `t`; `automations` = `auto`; `pages` = `page`; `organization` = `org`.

All help blocks below omit the repeated global-options footer. Argument requiredness and cross-option rules are sometimes enforced in the handler rather than printed in help; consult the linked source and report.

## Installed command index

| Command | Purpose |
| --- | --- |
| [`agents create`](#agents-create) | Create an agent session in an existing workspace |
| [`agents list`](#agents-list) | List agents configured on a host |
| [`auth login`](#auth-login) | Authenticate with Superset. Re-run to switch organizations. |
| [`auth logout`](#auth-logout) | Clear stored credentials |
| [`auth whoami`](#auth-whoami) | Show current user, organization, and auth source |
| [`automations create`](#automations-create) | Create a scheduled automation |
| [`automations delete`](#automations-delete) | Delete an automation |
| [`automations get`](#automations-get) | Show a single automation's configuration |
| [`automations list`](#automations-list) | List automations in the organization |
| [`automations logs`](#automations-logs) | List recent runs of an automation |
| [`automations pause`](#automations-pause) | Pause an automation (stops scheduled firing) |
| [`automations prompt get`](#automations-prompt-get) | Print an automation's prompt to stdout |
| [`automations prompt set`](#automations-prompt-set) | Replace an automation's prompt from a file or stdin |
| [`automations resume`](#automations-resume) | Resume a paused automation |
| [`automations run`](#automations-run) | Trigger an automation to run immediately |
| [`automations update`](#automations-update) | Update an automation's metadata (name, schedule, agent, host) |
| [`browser cdp`](#browser-cdp) | Print a raw CDP WebSocket endpoint for a pane (for browser-use / Playwright-class tools) |
| [`browser console`](#browser-console) | Read a browser pane's captured console output |
| [`browser eval`](#browser-eval) | Evaluate JavaScript in a browser pane and return the result |
| [`browser import-login`](#browser-import-login) | Import logins (cookies) from a system browser into a browser pane's session |
| [`browser list`](#browser-list) | List the browser panes open in a workspace |
| [`browser navigate`](#browser-navigate) | Navigate an existing browser pane to a URL |
| [`browser open`](#browser-open) | Open a URL in a workspace browser pane and return its pane id |
| [`browser screenshot`](#browser-screenshot) | Capture a PNG screenshot of a browser pane |
| [`feedback submit`](#feedback-submit) | Submit feedback privately to the Superset team (sent from your account so we can reply) |
| [`hosts list`](#hosts-list) | List hosts accessible to you in an organization |
| [`hosts set-wake`](#hosts-set-wake) | Set (or clear) the command used to wake a host |
| [`hosts wake`](#hosts-wake) | Wake a host by running its configured wake command locally |
| [`organization list`](#organization-list) | List organizations you belong to |
| [`organization members list`](#organization-members-list) | List members of the active organization |
| [`organization switch`](#organization-switch) | Switch the active organization for this CLI |
| [`pages comments list`](#pages-comments-list) | List comment threads on a page, oldest first |
| [`pages comments reply`](#pages-comments-reply) | Reply to a comment thread — answer only threads handed off to this session |
| [`pages comments resolve`](#pages-comments-resolve) | Mark a comment thread resolved — reply first, so the reader sees what changed |
| [`pages get`](#pages-get) | Show a page |
| [`pages list`](#pages-list) | List pages in the organization |
| [`pages publish`](#pages-publish) | Publish an HTML file as a page |
| [`pages pull`](#pages-pull) | Write a published version's HTML to stdout |
| [`pages versions`](#pages-versions) | List a page's versions, newest first |
| [`pages watch list`](#pages-watch-list) | List the pages this host is watching for comments |
| [`pages watch stop`](#pages-watch-stop) | Stop watching a page for comments |
| [`projects create`](#projects-create) | Create a project on a host |
| [`projects list`](#projects-list) | List projects on a host (default: this machine) |
| [`projects setup`](#projects-setup) | Adopt an existing project on a host (clone its repo or import a folder) |
| [`scripts add`](#scripts-add) | Add a reusable terminal script |
| [`settings get`](#settings-get) | Print the current value of a desktop app setting |
| [`settings list`](#settings-list) | List desktop app settings and their current values |
| [`settings reset`](#settings-reset) | Reset a desktop app setting to its default |
| [`settings set`](#settings-set) | Set a desktop app setting |
| [`settings theme export`](#settings-theme-export) | Export a theme (built-in or custom) as JSON — a starting point for custom themes |
| [`settings theme get`](#settings-theme-get) | Print the active desktop theme |
| [`settings theme import`](#settings-theme-import) | Import custom themes from a JSON file (single theme, array, or { themes: [...] }) |
| [`settings theme list`](#settings-theme-list) | List available themes (built-in, custom, and system) |
| [`settings theme remove`](#settings-theme-remove) | Remove a custom theme |
| [`settings theme set`](#settings-theme-set) | Set the active desktop theme |
| [`start`](#start) | Start the host service |
| [`status`](#status) | Check host service status |
| [`stop`](#stop) | Stop the host service daemon |
| [`tasks create`](#tasks-create) | Create a task |
| [`tasks delete`](#tasks-delete) | Delete tasks |
| [`tasks get`](#tasks-get) | Get a task by ID or slug |
| [`tasks list`](#tasks-list) | List tasks in the organization |
| [`tasks statuses list`](#tasks-statuses-list) | List task statuses in the active organization |
| [`tasks update`](#tasks-update) | Update a task |
| [`terminals close`](#terminals-close) | Close (dispose) a terminal running in a workspace |
| [`terminals create`](#terminals-create) | Create a terminal session in an existing workspace |
| [`terminals list`](#terminals-list) | List the live terminal sessions in a workspace |
| [`terminals read`](#terminals-read) | Read a terminal's current screen back as text |
| [`terminals send`](#terminals-send) | Send a follow-up message to a terminal already running in a workspace |
| [`update`](#update) | Update the Superset CLI and host service to the latest release |
| [`workspaces create`](#workspaces-create) | Create a workspace on a host |
| [`workspaces delete`](#workspaces-delete) | Delete workspaces by ID on a host (default: this machine) |
| [`workspaces get`](#workspaces-get) | Show details for a single workspace by id |
| [`workspaces list`](#workspaces-list) | List workspaces on a host (default: this machine) |
| [`workspaces open`](#workspaces-open) | Open a workspace in the Superset desktop app |
| [`workspaces update`](#workspaces-update) | Update a workspace on a host (default: this machine) |

## Installed command details

### agents create

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/agents/create/command.ts).

```text
Usage: superset agents create [options]

Create an agent session in an existing workspace

Options:
  --workspace <string>       Workspace ID
  --host <string>            Host the workspace lives on (default: this machine)
  --agent <string>           Agent preset id (e.g. `claude`), HostAgentConfig instance UUID, or `superset` for a Superset session
  --prompt <string>          Prompt sent to the agent (required unless resuming, forking, or handing off a session)
  --resume-session <string>  Session id of a previous run of this agent to restore instead of starting fresh
  --fork-session <string>    Session id of a previous run to clone into a new provider session
  --from-terminal <string>   Terminal ID whose recent context seeds the new session, so another agent can pick the work up
  --context-chars <number>   Cap the handed-over context, 1-36000 characters (default 36000, roughly 9-12k tokens)
  --effort <string>          Reasoning effort for this launch (agent-specific; omit to use the agent default)
  --attachment-id <string>   Pre-uploaded attachment UUID; pass --attachment-id repeatedly
  --attachment <string>      Local file path to upload as an attachment to the host. Repeatable
```

### agents list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/agents/list/command.ts).

```text
Usage: superset agents list [options]

List agents configured on a host

Options:
  --host <string>  Target host machineId
  --local          Target this machine
```

### auth login

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/auth/login/command.ts).

```text
Usage: superset auth login [options]

Authenticate with Superset. Re-run to switch organizations.

Options:
  --organization <string>  Organization id or slug — required for non-TTY logins when you belong to multiple orgs
  --api-key <string>       Store a Superset API key (sk_live_…) at ~/.superset/config.json instead of running the OAuth flow
```

### auth logout

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/auth/logout/command.ts).

```text
Usage: superset auth logout

Clear stored credentials
```

### auth whoami

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/auth/whoami/command.ts).

```text
Usage: superset auth whoami

Show current user, organization, and auth source
```

### automations create

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/create/command.ts).

```text
Usage: superset automations create [options]

Create a scheduled automation

Options:
  --name <string>         Human-readable automation name
  --prompt <string>       Prompt to send to the agent
  --prompt-file <string>  Path to a file containing the prompt
  --rrule <string>        RFC 5545 RRULE body, e.g. FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9;BYMINUTE=0
  --timezone <string>     IANA timezone (default: host TZ, else UTC)
  --dtstart <string>      ISO 8601 start anchor (default: now)
  --project <string>      v2 project id for new-workspace-per-run mode. Omit (with no --workspace) to create a project-less session per run
  --workspace <string>    existing v2 workspace id — reuses it every run
  --host <string>         Host the target project/workspace lives on (default: this machine)
  --agent <string>        Host agent instance id or presetId (claude, codex, ...). (default: claude)
  --tag <string>          Workspace tag applied to each run's created workspace. Repeatable. Each tag files the workspace into a sidebar folder of the same name
```

### automations delete

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/delete/command.ts).

```text
Usage: superset automations delete <id>

Delete an automation

Arguments:
  id  Automation id (required)
```

### automations get

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/get/command.ts).

```text
Usage: superset automations get <id>

Show a single automation's configuration

Arguments:
  id  Automation id (required)
```

### automations list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/list/command.ts).

```text
Usage: superset automations list [options]

List automations in the organization

Options:
  -n, --name <string>  Filter by name (case-insensitive substring match)
```

### automations logs

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/logs/command.ts).

```text
Usage: superset automations logs <id> [options]

List recent runs of an automation

Arguments:
  id  Automation id (required)

Options:
  --limit <number>  Max runs to return (1-100) (default: 20)
```

### automations pause

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/pause/command.ts).

```text
Usage: superset automations pause <id>

Pause an automation (stops scheduled firing)

Arguments:
  id  Automation id (required)
```

### automations prompt get

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/prompt/get/command.ts).

```text
Usage: superset automations prompt get <id>

Print an automation's prompt to stdout

Arguments:
  id  Automation id (required)
```

### automations prompt set

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/prompt/set/command.ts).

```text
Usage: superset automations prompt set <id> [options]

Replace an automation's prompt from a file or stdin

Arguments:
  id  Automation id (required)

Options:
  --from-file <string>  Path to a markdown file with the new prompt. Use '-' to read from stdin.
```

### automations resume

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/resume/command.ts).

```text
Usage: superset automations resume <id>

Resume a paused automation

Arguments:
  id  Automation id (required)
```

### automations run

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/run/command.ts).

```text
Usage: superset automations run <id>

Trigger an automation to run immediately

Arguments:
  id  Automation id (required)
```

### automations update

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/automations/update/command.ts).

```text
Usage: superset automations update <id> [options]

Update an automation's metadata (name, schedule, agent, host)

Arguments:
  id  Automation id (required)

Options:
  --name <string>       New name
  --rrule <string>      New RRule body (RFC 5545)
  --timezone <string>   New IANA timezone
  --dtstart <string>    New ISO 8601 start anchor
  --agent <string>      New host agent instance id or presetId (e.g. claude, codex, superset).
  --host <string>       New target host id
  --project <string>    New v2 project id
  --workspace <string>  New v2 workspace id
  --session             Switch to session mode: no project, each run creates a project-less session workspace
  --enabled             Enable or pause the automation
  --tag <string>        Replace the tag set applied to each run's created workspace. Repeatable
  --clear-tags          Remove every tag from the automation
```

### browser cdp

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/cdp/command.ts).

```text
Usage: superset browser cdp [options]

Print a raw CDP WebSocket endpoint for a pane (for browser-use / Playwright-class tools)

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --pane <string>       Pane ID (from `superset browser list`)
```

### browser console

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/console/command.ts).

```text
Usage: superset browser console [options]

Read a browser pane's captured console output

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --pane <string>       Pane ID (from `superset browser list`)
  --max-lines <number>  Cap returned entries from the bottom
```

### browser eval

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/eval/command.ts).

```text
Usage: superset browser eval [options]

Evaluate JavaScript in a browser pane and return the result

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --pane <string>       Pane ID (from `superset browser list`)
  --code <string>       JavaScript expression to evaluate
```

### browser import-login

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/import-login/command.ts).

```text
Usage: superset browser import-login [options]

Import logins (cookies) from a system browser into a browser pane's session

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --pane <string>       Pane ID (from `superset browser list`)
  --from <string>       Source browser to import from, e.g. 'Comet', 'Chrome' (matches the browser name)
  --profile <string>    Profile name to disambiguate when the browser has several
```

### browser list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/list/command.ts).

```text
Usage: superset browser list [options]

List the browser panes open in a workspace

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
```

### browser navigate

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/navigate/command.ts).

```text
Usage: superset browser navigate [options]

Navigate an existing browser pane to a URL

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --pane <string>       Pane ID (from `superset browser list`)
  --url <string>        URL to navigate to
```

### browser open

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/open/command.ts).

```text
Usage: superset browser open [options]

Open a URL in a workspace browser pane and return its pane id

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --url <string>        URL to open
  --target <string>     `current-tab` (default) or `new-tab`
```

### browser screenshot

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/browser/screenshot/command.ts).

```text
Usage: superset browser screenshot [options]

Capture a PNG screenshot of a browser pane

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --pane <string>       Pane ID (from `superset browser list`)
  --out <string>        Write PNG to this path instead of printing base64
```

### feedback submit

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/feedback/submit/command.ts).

```text
Usage: superset feedback submit [options]

Submit feedback privately to the Superset team (sent from your account so we can reply)

Options:
  --type <bug|feature|general>  Kind of feedback
  --title <string>              One-line summary
  --body <string>               Full report
  --body-file <string>          Path to a file containing the report, - for stdin
  --attach <string>             Comma-separated file paths to attach (screenshots, logs; 10MB total)
  --diagnostics                 Attach a diagnostics bundle (CLI version, OS, last 200 app log lines)
```

### hosts list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/hosts/list/command.ts).

```text
Usage: superset hosts list [options]

List hosts accessible to you in an organization

Options:
  --org <string>  Organization (id, slug, or name); defaults to active
```

### hosts set-wake

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/hosts/set-wake/command.ts).

```text
Usage: superset hosts set-wake <host> [command...] [options]

Set (or clear) the command used to wake a host

Arguments:
  host     Host name or id (required)
  command  Command to run to wake the host, e.g. "vercel sandbox resume my-box" (variadic)

Options:
  --clear         Remove the wake command
  --org <string>  Organization (id, slug, or name); defaults to active
```

### hosts wake

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/hosts/wake/command.ts).

```text
Usage: superset hosts wake <host> [options]

Wake a host by running its configured wake command locally

Arguments:
  host  Host name or id (required)

Options:
  --yes           Skip the confirmation prompt
  --org <string>  Organization (id, slug, or name); defaults to active
```

### organization list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/organization/list/command.ts).

```text
Usage: superset organization list

List organizations you belong to
```

### organization members list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/organization/members/list/command.ts).

```text
Usage: superset organization members list [options]

List members of the active organization

Options:
  -s, --search <string>  Search by name or email
  --limit <number>       Max results (default: 50)
```

### organization switch

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/organization/switch/command.ts).

```text
Usage: superset organization switch <idOrSlug>

Switch the active organization for this CLI

Arguments:
  idOrSlug  Organization id or slug (required)
```

### pages comments list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/comments/list/command.ts).

```text
Usage: superset pages comments list [options]

List comment threads on a page, oldest first

Options:
  --pageId, --page <string>       Page id or slug (omit to sweep every page you can see)
  --thread, --thread-id <string>  Show only this thread
  --open, --unresolved            Show only threads that are still open
  --workspace <string>            With no --page, only sweep this workspace's pages
```

### pages comments reply

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/comments/reply/command.ts).

```text
Usage: superset pages comments reply <body> [options]

Reply to a comment thread — answer only threads handed off to this session

Arguments:
  body  Reply text (required)

Options:
  --thread, --thread-id <string>  Thread id to reply to
```

### pages comments resolve

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/comments/resolve/command.ts).

```text
Usage: superset pages comments resolve [options]

Mark a comment thread resolved — reply first, so the reader sees what changed

Options:
  --thread, --thread-id <string>  Thread id to resolve
  --reopen                        Reopen the thread instead of resolving it
```

### pages get

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/get/command.ts).

```text
Usage: superset pages get <page>

Show a page

Arguments:
  page  Page id or slug (required)
```

### pages list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/list/command.ts).

```text
Usage: superset pages list [options]

List pages in the organization

Options:
  --workspace <string>  Only pages published from this workspace, by name or id (defaults to $SUPERSET_WORKSPACE_ID)
```

### pages publish

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/publish/command.ts).

```text
Usage: superset pages publish <path> [options]

Publish an HTML file as a page

Arguments:
  path  Path to the .html file (required)

Options:
  --title <string>        Page title (defaults to the filename)
  --description <string>  Short description
  -l, --label <string>    What changed in this version, shown in the version history
  --visibility <string>   One of: just_me, org
  --page <string>         Publish a new version of this page id, instead of resolving by workspace
  --workspace <string>    Workspace to publish into, by name or id (defaults to $SUPERSET_WORKSPACE_ID)
  --no-watch              Do not watch this page for new comments from this session
```

### pages pull

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/pull/command.ts).

```text
Usage: superset pages pull <page> [options]

Write a published version's HTML to stdout

Arguments:
  page  Page id or slug (required)

Options:
  -v, --version <number>  Version to fetch (defaults to the one currently served)
```

### pages versions

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/versions/command.ts).

```text
Usage: superset pages versions <page>

List a page's versions, newest first

Arguments:
  page  Page id or slug (required)
```

### pages watch list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/watch/list/command.ts).

```text
Usage: superset pages watch list [options]

List the pages this host is watching for comments

Options:
  --workspace <string>  Only show pages watched from this workspace id
```

### pages watch stop

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/pages/watch/stop/command.ts).

```text
Usage: superset pages watch stop [options]

Stop watching a page for comments

Options:
  --page <string>  Page id or slug
```

### projects create

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/projects/create/command.ts).

```text
Usage: superset projects create [options]

Create a project on a host

Options:
  --host <string>        Target host machineId
  --local                Target this machine
  --name <string>        Project name
  --clone <string>       Git remote URL to clone (requires --parent-dir). Mutually exclusive with --import
  --parent-dir <string>  Parent directory the cloned repo lands in (required with --clone)
  --import <string>      Existing local repo path on the target host. Mutually exclusive with --clone
```

### projects list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/projects/list/command.ts).

```text
Usage: superset projects list [options]

List projects on a host (default: this machine)

Options:
  --host <string>  List projects on a specific host machineId
  --local          List projects on this machine (the default)
```

### projects setup

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/projects/setup/command.ts).

```text
Usage: superset projects setup [id] [options]

Adopt an existing project on a host (clone its repo or import a folder)

Arguments:
  id  Project UUID to adopt

Options:
  --host <string>        Target host machineId
  --local                Target this machine
  --project <string>     Project UUID to adopt
  --path <string>        Existing local repo path on the target host (alias for --import)
  --parent-dir <string>  Parent directory to clone the project's repo into (clone mode)
  --import <string>      Existing local repo path on the target host (import mode)
  --allow-relocate       Permit re-importing at a different path if the project is already set up here
  --repo-url <string>    Repo clone URL, when the target host doesn't know the project yet
  --name <string>        Project name, when the target host doesn't know it yet
```

### scripts add

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/scripts/add/command.ts).

```text
Usage: superset scripts add [options]

Add a reusable terminal script

Options:
  --name <string>                                                      Display name
  --command <string>                                                   Shell command; repeat to launch multiple commands
  --description <string>                                               Optional description
  --cwd <string>                                                       Working directory relative to the workspace
  --project <string>                                                   Limit to a project UUID; repeat for multiple projects
  --execution-mode <split-pane|new-tab|new-tab-split-pane|sequential>  How multiple commands open
  --hidden                                                             Create without showing it in the Scripts bar
  --workspace-run                                                      Use as the project's Run action
```

### settings get

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/get/command.ts).

```text
Usage: superset settings get <key>

Print the current value of a desktop app setting

Arguments:
  key  Setting key (see: settings list) (required)
```

### settings list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/list/command.ts).

```text
Usage: superset settings list

List desktop app settings and their current values
```

### settings reset

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/reset/command.ts).

```text
Usage: superset settings reset <key>

Reset a desktop app setting to its default

Arguments:
  key  Setting key (see: settings list) (required)
```

### settings set

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/set/command.ts).

```text
Usage: superset settings set <key> <value>

Set a desktop app setting

Arguments:
  key    Setting key (see: settings list) (required)
  value  New value (required)
```

### settings theme export

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/theme/export/command.ts).

```text
Usage: superset settings theme export <id> [options]

Export a theme (built-in or custom) as JSON — a starting point for custom themes

Arguments:
  id  Theme id (see: settings theme list) (required)

Options:
  --out <string>  Write to a file instead of stdout
```

### settings theme get

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/theme/get/command.ts).

```text
Usage: superset settings theme get

Print the active desktop theme
```

### settings theme import

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/theme/import/command.ts).

```text
Usage: superset settings theme import <file>

Import custom themes from a JSON file (single theme, array, or { themes: [...] })

Arguments:
  file  Path to a theme JSON file (required)
```

### settings theme list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/theme/list/command.ts).

```text
Usage: superset settings theme list

List available themes (built-in, custom, and system)
```

### settings theme remove

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/theme/remove/command.ts).

```text
Usage: superset settings theme remove <id>

Remove a custom theme

Arguments:
  id  Custom theme id (built-ins can't be removed) (required)
```

### settings theme set

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/settings/theme/set/command.ts).

```text
Usage: superset settings theme set [theme] [options]

Set the active desktop theme

Arguments:
  theme  Theme id, or "system" to follow the OS appearance (see: settings theme list)

Options:
  --system-light <string>  Theme used for OS light mode when the active theme is "system"
  --system-dark <string>   Theme used for OS dark mode when the active theme is "system"
```

### start

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/start/command.ts).

```text
Usage: superset start [options]

Start the host service

Options:
  --daemon         Run in background
  --port <number>  Port to listen on
  --org <string>   Organization to register under (id, slug, or name)
```

### status

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/status/command.ts).

```text
Usage: superset status [options]

Check host service status

Options:
  --org <string>  Organization (id, slug, or name); defaults to active
```

### stop

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/stop/command.ts).

```text
Usage: superset stop

Stop the host service daemon
```

### tasks create

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/tasks/create/command.ts).

```text
Usage: superset tasks create [options]

Create a task

Options:
  --title <string>                          Task title
  --description <string>                    Task description
  --priority <urgent|high|medium|low|none>  Priority
  --assignee <string>                       Assignee user ID
  --status-id <string>                      Status ID
  --estimate <number>                       Story-point estimate
  --due-date <string>                       Due date (ISO 8601)
  --labels <string>                         Comma-separated labels
```

### tasks delete

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/tasks/delete/command.ts).

```text
Usage: superset tasks delete <ids...>

Delete tasks

Arguments:
  ids  Task IDs or slugs (required) (variadic)
```

### tasks get

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/tasks/get/command.ts).

```text
Usage: superset tasks get <idOrSlug>

Get a task by ID or slug

Arguments:
  idOrSlug  Task ID or slug (required)
```

### tasks list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/tasks/list/command.ts).

```text
Usage: superset tasks list [options]

List tasks in the organization

Options:
  --status <string>                                 Filter by status id
  --priority <urgent|high|medium|low|none>          Filter by priority
  --assignee <string>                               Filter by assignee user id
  -m, --assignee-me                                 Filter to my tasks
  --creator-me                                      Filter to tasks I created
  -s, --search <string>                             Search by title or description
  --project <string>                                Filter by Linear project id
  --project-name <string>                           Filter by Linear project name (prefix, case-insensitive)
  --cycle <string>                                  Filter by Linear cycle id
  --due-from <string>                               Tasks due on or after this date (YYYY-MM-DD)
  --due-to <string>                                 Tasks due on or before this date (YYYY-MM-DD)
  --sort-by <createdAt|updatedAt|dueDate|priority>  Sort field (default: createdAt)
  --sort-order <asc|desc>                           Sort direction
  --limit <number>                                  Max results (default: 50)
  --offset <number>                                 Skip results (default: 0)
```

### tasks statuses list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/tasks/statuses/list/command.ts).

```text
Usage: superset tasks statuses list

List task statuses in the active organization
```

### tasks update

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/tasks/update/command.ts).

```text
Usage: superset tasks update <idOrSlug> [options]

Update a task

Arguments:
  idOrSlug  Task ID or slug (required)

Options:
  --title <string>                          Task title
  --description <string>                    Task description
  --priority <urgent|high|medium|low|none>  Priority
  --assignee <string>                       Assignee user ID
  --status-id <string>                      Status ID
  --pr-url <string>                         Linked PR URL
  --estimate <number>                       Story-point estimate
  --due-date <string>                       Due date (ISO 8601)
  --labels <string>                         Comma-separated labels
```

### terminals close

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/terminals/close/command.ts).

```text
Usage: superset terminals close [options]

Close (dispose) a terminal running in a workspace

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --terminal <string>   Terminal ID to close
```

### terminals create

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/terminals/create/command.ts).

```text
Usage: superset terminals create [options]

Create a terminal session in an existing workspace

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --command <string>    Shell command to run in the terminal. Omit to open an interactive shell
  --cwd <string>        Working directory for the terminal (defaults to the worktree)
```

### terminals list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/terminals/list/command.ts).

```text
Usage: superset terminals list [options]

List the live terminal sessions in a workspace

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
```

### terminals read

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/terminals/read/command.ts).

```text
Usage: superset terminals read [options]

Read a terminal's current screen back as text

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --terminal <string>   Terminal ID to read
  --max-lines <number>  Cap returned rows from the bottom
```

### terminals send

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/terminals/send/command.ts).

```text
Usage: superset terminals send [options]

Send a follow-up message to a terminal already running in a workspace

Options:
  --workspace <string>  Workspace ID
  --host <string>       Host the workspace lives on (default: this machine)
  --terminal <string>   Terminal ID (the sessionId `agents create` returned)
  --text <string>       Text to write into the terminal
  --no-submit           Stage the text without pressing Enter
```

### update

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/update/command.ts).

```text
Usage: superset update [options]

Update the Superset CLI and host service to the latest release

Options:
  --check             Only check for updates; don't install
  --force             Re-install even if already on that version
  --version <string>  Install a specific CLI version (e.g. 0.1.2) instead of the rolling latest
```

### workspaces create

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/workspaces/create/command.ts).

```text
Usage: superset workspaces create [options]

Create a workspace on a host

Options:
  --host <string>         Target host machineId
  --local                 Target this machine
  --project <string>      Project ID. Omit to create a project-less session (a managed scratch folder)
  --name <string>         Workspace name
  --branch <string>       Git branch (required unless --pr or --task is set)
  --pr <number>           PR number — checks out the verified PR head
  --task <string>         Task ID to link. When --branch is omitted, the task's provider branch name (e.g. Linear's) is used verbatim
  --base-branch <string>  Branch to fork from when `branch` does not exist (defaults to project default)
  --skip-branch-prefix    Use --branch exactly as given instead of namespacing it under the project branch prefix
  --agent <string>        Agent to spawn after creation. Preset id (`claude`, `codex`, …), HostAgentConfig instance UUID, or `superset`
  --prompt <string>       Initial prompt the agent starts with. Required when --agent is set
  --effort <string>       Reasoning effort for the spawned agent (agent-specific; omit to use the agent default)
  --command <string>      Shell command to run in the new workspace after creation
  --attachment <string>   Local file path to upload as an attachment to the host. Repeatable. Only used when --agent is set
  --tag <string>          Workspace tag. Repeatable. Each tag files the workspace into a sidebar folder of the same name
```

### workspaces delete

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/workspaces/delete/command.ts).

```text
Usage: superset workspaces delete <ids...> [options]

Delete workspaces by ID on a host (default: this machine)

Arguments:
  ids  Workspace IDs (required) (variadic)

Options:
  --host <string>  Host the workspaces live on
  --local          Target this machine (the default)
```

### workspaces get

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/workspaces/get/command.ts).

```text
Usage: superset workspaces get [id] [options]

Show details for a single workspace by id

Arguments:
  id  Workspace ID (defaults to $SUPERSET_WORKSPACE_ID)

Options:
  --host <string>       Host the workspace lives on (default: this machine)
  -f, --field <string>  Print a single field's raw value (e.g. name, branch, worktreePath)
```

### workspaces list

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/workspaces/list/command.ts).

```text
Usage: superset workspaces list [options]

List workspaces on a host (default: this machine)

Options:
  --host <string>        List workspaces on a specific host (machineId)
  --local                List workspaces on this machine (the default)
  --project <string>     Filter by project name (case-insensitive) or id
  -s, --search <string>  Search by workspace name or branch substring
  --tag <string>         Filter to workspaces carrying this tag
```

### workspaces open

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/workspaces/open/command.ts).

```text
Usage: superset workspaces open <id> [options]

Open a workspace in the Superset desktop app

Arguments:
  id  Workspace ID (required)

Options:
  --host <string>  Host the workspace lives on (default: this machine)
  --print          Print the deep link URL instead of opening the desktop app
```

### workspaces update

Source: [installed release handler](https://github.com/superset-sh/superset/blob/108b9bf0901c61ff5192874cab285054eb1e88fe/packages/cli/src/commands/workspaces/update/command.ts).

```text
Usage: superset workspaces update <id> [options]

Update a workspace on a host (default: this machine)

Arguments:
  id  Workspace UUID (required)

Options:
  --host <string>     Host the workspace lives on (default: this machine)
  --name <string>     Workspace name
  --task-id <string>  Link the workspace to a task by id
  --clear-task        Unlink the workspace from its current task
  --tag <string>      Replace the workspace's tag set. Repeatable. Each tag files the workspace into a sidebar folder of the same name
  --clear-tags        Remove every tag from the workspace
```

## Commands added on main after the 1.26.0 release

Source snapshot: `42bd65b92c7c7e30187160af8c3fca49b0f7556a`. These **18 commands are absent from the installed 1.25.1 help tree and absent from the published 1.26.0 tag**. The declarations below come from source; no newer binary was installed or executed. `plugins` also has alias `plugin`, and `plugins uninstall` has alias `remove`.

### mcp call-tool

`superset mcp call-tool [plugin] <tool> [arguments-json|-] [--connection ID] [--plugin-id ID]`

Call a tool on a connected plugin

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/mcp/call-tool/command.ts).

Arguments (exact source declaration):

```typescript
positional("plugin").desc("Plugin name, when it has one connection"),
		positional("tool").required().desc("Tool name"),
		positional("arguments").desc(
			'Tool arguments as JSON (default: {}; "-" reads them from stdin)',
		),
```

Options (exact source declaration):

```typescript
connection: string().desc("Connection id from `superset plugins list`"),
		pluginId: string().desc("Deprecated alias for --connection"),
```


### mcp tools

`superset mcp tools [plugin] [--connection ID] [--plugin-id ID]`

List the tools a connected plugin exposes

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/mcp/tools/command.ts).

Options (exact source declaration):

```typescript
connection: string().desc("Connection id from `superset plugins list`"),
		pluginId: string().desc("Deprecated alias for --connection"),
```


### plugins build

`superset plugins build [names...] [--force]`

Build plugin servers from TypeScript source

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/build/command.ts).

Arguments (exact source declaration):

```typescript
positional("names")
			.variadic()
			.desc("Plugin names to build (default: every plugin in the marketplace)"),
```

Options (exact source declaration):

```typescript
force: boolean().desc("Rebuild even when the existing build is current"),
```


### plugins connect

`superset plugins connect <plugin> [--marketplace NAME] [--method oauth2|api_key] [--inputs JSON|-]`

Connect an account to an installed plugin

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/connect/command.ts).

Arguments (exact source declaration):

```typescript
positional("plugin")
			.required()
			.desc("Plugin name, or name@marketplace to disambiguate"),
```

Options (exact source declaration):

```typescript
marketplace: string().desc(
			"Which marketplace's install to connect, when several offer this name",
		),
		method: string()
			.enum("oauth2", "api_key")
			.desc("Which declared auth method to use, when a plugin offers several"),
		inputs: string().desc(
			'Credential inputs as JSON, or "-" to read them from stdin',
		),
```


### plugins connections

`superset plugins connections [--plugin NAME]`

List the plugin accounts connected to your Superset account

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/connections/command.ts).

Options (exact source declaration):

```typescript
plugin: string().desc("Only this plugin"),
```


### plugins create

`superset plugins create <name> --kind url|server|none [--url URL] [--skills] [--auth] [--display-name TEXT] [--description TEXT] [--category TEXT]`

Scaffold a new plugin and add it to the marketplace

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/create/command.ts).

Arguments (exact source declaration):

```typescript
positional("name")
			.required()
			.desc("Plugin name: lowercase letters, digits, dots and hyphens"),
```

Options (exact source declaration):

```typescript
kind: string()
			.required()
			.enum("url", "server", "none")
			.desc(
				"Where tools come from: url (remote MCP server), server (custom MCP you write), none (skills only)",
			),
		url: string().desc("MCP server URL, required when --kind url"),
		skills: boolean().desc("Scaffold a skills/ folder with a starter skill"),
		auth: boolean().desc("Include an OAuth2 block to fill in"),
		"display-name": string().desc(
			"Name shown in the UI (default: derived from name)",
		),
		description: string().desc("One-line description"),
		category: string().desc("Category shown in the UI"),
```


### plugins disable

`superset plugins disable <plugin> [--marketplace NAME]`

Disable an installed plugin without dropping it

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/disable/command.ts).

Arguments (exact source declaration):

```typescript
positional("plugin")
			.required()
			.desc("Plugin name, or name@marketplace to disambiguate"),
```

Options (exact source declaration):

```typescript
marketplace: string().desc(
			"Which marketplace's install to change, when several offer this name",
		),
```


### plugins enable

`superset plugins enable <plugin> [--marketplace NAME]`

Enable an installed plugin

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/enable/command.ts).

Arguments (exact source declaration):

```typescript
positional("plugin")
			.required()
			.desc("Plugin name, or name@marketplace to disambiguate"),
```

Options (exact source declaration):

```typescript
marketplace: string().desc(
			"Which marketplace's install to change, when several offer this name",
		),
```


### plugins install

`superset plugins install <plugin> [--marketplace NAME] [--inputs JSON|-] [--update]`

Install a plugin: materialize its skills locally and record it on your account

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/install/command.ts).

Arguments (exact source declaration):

```typescript
positional("plugin")
			.required()
			.desc("Plugin name, or name@marketplace to disambiguate"),
```

Options (exact source declaration):

```typescript
marketplace: string().desc(
			"Which marketplace to install from, when several offer this name",
		),
		inputs: string().desc(
			'Credential inputs as JSON, or "-" to read them from stdin',
		),
		update: boolean().desc(
			"Replace an existing install with the marketplace's current version, and re-sync its skills",
		),
```


### plugins list

`superset plugins list [--available]`

List installed plugins, or everything available with --available

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/list/command.ts).

Options (exact source declaration):

```typescript
available: boolean().desc("Include plugins you have not installed"),
```


### plugins marketplace add

`superset plugins marketplace add <source> [--name NAME]`

Add a marketplace from a GitHub repo or local path

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/marketplace/add/command.ts).

Arguments (exact source declaration):

```typescript
positional("source")
			.required()
			.desc("owner/repo, owner/repo@ref, a GitHub URL, or a local path"),
```

Options (exact source declaration):

```typescript
name: string().desc("Register under this name instead of the manifest's"),
```


### plugins marketplace list

`superset plugins marketplace list`

List the marketplaces on your account

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/marketplace/list/command.ts).


### plugins marketplace remove

`superset plugins marketplace remove <name>`

Remove a marketplace from your account

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/marketplace/remove/command.ts).


### plugins publish

`superset plugins publish [names...] [--bump major|minor|patch] [--force]`

Cut a version of a plugin: build, record it in the marketplace and the generated bundle, and name the tag to publish it at

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/publish/command.ts).

Arguments (exact source declaration):

```typescript
positional("names")
			.variadic()
			.desc(
				"Plugin names to publish (default: every plugin in the marketplace)",
			),
```

Options (exact source declaration):

```typescript
bump: string().desc(
			"Bump the version before publishing: major, minor, or patch",
		),
		force: boolean().desc("Overwrite an already-published version"),
```


### plugins sync

`superset plugins sync`

Reconcile installed plugins with the skill directories agents read, adding, refreshing, and reaping skill folders

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/sync/command.ts).


### plugins uninstall

`superset plugins uninstall <plugin> [--marketplace NAME]`

Uninstall a plugin and drop its skills

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/uninstall/command.ts).

Arguments (exact source declaration):

```typescript
positional("plugin")
			.required()
			.desc("Plugin name, or name@marketplace to disambiguate"),
```

Options (exact source declaration):

```typescript
marketplace: string().desc(
			"Which marketplace's install to remove, when several offer this name",
		),
```


### plugins validate

`superset plugins validate [path] [--strict]`

Validate a plugin or a whole marketplace: manifest, release tag, and whether the built server matches its source

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/plugins/validate/command.ts).

Arguments (exact source declaration):

```typescript
positional("path").desc(
			"A plugin directory or a marketplace checkout (default: search upward from here)",
		),
```

Options (exact source declaration):

```typescript
strict: boolean().desc(
			"Also require every plugin's current version to be released",
		),
```


### skills list

`superset skills list [--paths]`

List skills installed from plugins, with the path of each

[Source definition](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/commands/skills/list/command.ts).

Options (exact source declaration):

```typescript
paths: boolean().desc("Print only the paths, one per line"),
```


## Changes without new command names

| Version boundary | Change |
| --- | --- |
| Installed 1.25.1 to published 1.26.0 | `pages publish` gains directory input with `index.html`, asset upload and reuse, and detailed partial-upload output. Command count stays 76. |
| Published 1.26.0 to inspected main | `agents create` and `workspaces create` gain `--model`; page document uploads change to presigned file upload. The 18 commands above are also added. |

Flags such as `--thread-id`, `--unresolved`, `--page`, `--field`, `--search`, and `--label` already exist in 1.25.1; help may display their aliases first. They are not new upstream flags.

## All 35 installed settings

Read with `settings get KEY`; change with `settings set KEY VALUE`; restore the built-in value with `settings reset KEY`. Values below are the defaults and allowed values reported by the installed CLI, not a disclosure of personal overrides. Empty defaults mean unset/app-selected.

| Key | Section | Default | Allowed values | Purpose |
| --- | --- | --- | --- | --- |
| `confirmOnQuit` | behavior | `true` | true \| false | Ask for confirmation before quitting the app |
| `fileOpenMode` | behavior | `split-pane` | split-pane \| new-tab | How files open from the sidebar and terminal links |
| `showResourceMonitor` | behavior | `true` | true \| false | Show the CPU/memory resource monitor |
| `openLinksInApp` | behavior | `false` | true \| false | Open http(s) links in an in-app browser pane |
| `browserHomepageUrl` | behavior | (unset) | string | URL new in-app browser tabs open to (unset = about:blank) |
| `defaultEditor` | behavior | (unset) | vscode \| vscode-insiders \| cursor \| antigravity \| devin \| zed \| sublime \| xcode \| intellij \| webstorm \| pycharm \| phpstorm \| rubymine \| goland \| clion \| rider \| datagrip \| appcode \| fleet \| rustrover \| android-studio | External editor used to open files and workspaces |
| `branchPrefixMode` | git | `none` | none \| github \| author \| custom | Prefix applied to generated workspace branch names |
| `branchPrefixCustom` | git | (unset) | string | Custom branch prefix (used when branchPrefixMode is "custom") |
| `worktreeBaseDir` | git | (unset) | string | Directory where workspace worktrees are created (default: ~/.superset/worktrees) |
| `selectedRingtoneId` | notifications | `arcade` | shamisen \| arcade \| ping \| quick \| doowap \| woman \| african \| afrobeat \| edm \| comeback \| shabala | Sound played when an agent finishes |
| `notificationSoundsMuted` | notifications | `false` | true \| false | Mute notification sounds |
| `notificationVolume` | notifications | `100` | integer 0..100 | Notification volume (0-100) |
| `language` | appearance | `auto` | auto \| en \| ja \| zh-CN \| fr \| ko \| zh-TW \| es \| de \| pt-BR \| it \| ru \| tr \| pl \| nl \| id \| cs \| vi | App display language (auto follows the system language) |
| `terminalLinkBehavior` | terminal | `file-viewer` | external-editor \| file-viewer | Where file paths clicked in a terminal open |
| `terminalParkedRuntimeCap` | terminal | `12` | integer 2..64 | Max number of background terminals kept running |
| `terminalCopyOnSelect` | terminal | `false` | true \| false | Copy selected terminal text to the clipboard right away |
| `showPresetsBar` | terminal | `true` | true \| false | Show the terminal scripts bar |
| `useCompactTerminalAddButton` | terminal | `true` | true \| false | Use the compact new-terminal button |
| `autoApplyDefaultPreset` | terminal | `true` | true \| false | Apply the default terminal script to new workspaces |
| `waitForSetupBeforeAgent` | terminal | `false` | true \| false | Wait for workspace setup to finish before starting agents |
| `terminalFontFamily` | terminal appearance | (unset) | string (max 500 chars) | Terminal font family (unset = app default) |
| `terminalFontSize` | terminal appearance | `14` | number 10..24 (step 0.5) | Terminal font size in px (10-24, 0.5 steps) |
| `terminalLineHeight` | terminal appearance | `1` | number 1..2.5 (step 0.1) | Terminal line height (1-2.5, 0.1 steps) |
| `terminalLetterSpacing` | terminal appearance | `0` | number -2..4 (step 0.1) | Terminal letter spacing in px (-2-4, 0.1 steps) |
| `terminalFontWeight` | terminal appearance | (unset) | integer 100..900 (step 100) | Terminal font weight (100-900, hundreds) |
| `terminalLigatures` | terminal appearance | `true` | true \| false | Enable terminal font ligatures |
| `terminalMinimumContrast` | terminal appearance | `1` | 1 \| 3 \| 4.5 \| 7 | Minimum terminal color contrast ratio (1, 3, 4.5, or 7) |
| `terminalCursorStyle` | terminal appearance | `block` | block \| bar \| underline | Terminal cursor style |
| `terminalCursorBlink` | terminal appearance | `true` | true \| false | Blink the terminal cursor |
| `editorFontFamily` | editor appearance | (unset) | string (max 500 chars) | File editor font family (unset = app default) |
| `editorFontSize` | editor appearance | `13` | number 10..24 (step 0.5) | File editor font size in px (10-24, 0.5 steps) |
| `editorLineHeight` | editor appearance | `1.5` | number 1..2.5 (step 0.1) | File editor line height (1-2.5, 0.1 steps) |
| `editorLetterSpacing` | editor appearance | `0` | number -2..4 (step 0.1) | File editor letter spacing in px (-2-4, 0.1 steps) |
| `editorFontWeight` | editor appearance | (unset) | integer 100..900 (step 100) | File editor font weight (100-900, hundreds) |
| `editorLigatures` | editor appearance | `true` | true \| false | Enable file editor font ligatures |

Settings source: [typed registry](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/packages/cli/src/lib/settings/registry.ts). `terminalParkedRuntimeCap` governs desktop background terminal rendering runtimes; it should not be read as a global PTY session limit. [Renderer eviction policy](https://github.com/superset-sh/superset/blob/42bd65b92c7c7e30187160af8c3fca49b0f7556a/apps/desktop/src/renderer/lib/terminal/terminal-runtime-eviction.ts).
