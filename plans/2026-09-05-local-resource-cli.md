# OVRCR project, workspace, and terminal CLI

## Approved behavior

Expand the local CLI using Superset command patterns while retaining the Rust server, Unix socket, TOML registry, one dashboard, and memory-only PTY sessions. Use singular groups with plural aliases. Preserve workspace creation's local shell and guarded deletion.

| Command | Contract |
| --- | --- |
| project add NAME REPO --workspace-root PATH | Existing registration; create alias |
| project list / get NAME | Project-specific records including repository and workspace-root paths |
| project remove NAME | Existing unregister guard; delete alias |
| workspace create | Existing branch flags and local-shell launch |
| workspace list [--project NAME] | All workspaces or one project's workspaces |
| workspace get --project NAME --name NAME | Path, branch, terminal count |
| workspace remove --project NAME --name NAME | Existing ownership, clean-Git, and empty-session guards; delete alias |
| terminal create --project NAME --workspace NAME --name NAME [--label TEXT] -- COMMAND ARGS | Existing session creation; omitted command uses SHELL |
| terminal list [--project NAME] [--workspace NAME] | Running and retained exited sessions; workspace filter requires project |
| terminal read ID [--max-lines N] | Plain current-screen text, including exited final screen; positive N selects final text lines |
| terminal send ID --text TEXT [--no-submit] | Bracketed-paste-aware text, then separately written Enter unless suppressed |
| terminal close ID | Terminate process group, drain/join, remove record only after successful cleanup |
| terminal kill ID / remove ID | Existing separate lifecycle operations |

Preserve new, list, kill, session remove, and bare dashboard invocation. Legacy list retains hierarchy output; project list becomes project-specific. Explicit --json controls resource command output: arrays for lists, objects for get/terminal creation, {"ok":true} for other mutations. Runtime errors emit {"error":{"code":"…","message":"…"}} to stderr and exit 1. Clap retains normal help/argument errors and exit 2.

## Task 1: Resource inspection and CLI

Add an inventory control request using registry and session summary types; do not change dashboard messages. CLI filters/formats resources; projects/workspaces sort by name, terminals by ID. Read-only commands do not start a server: absent server uses persisted registry with zero live sessions. Missing explicit targets produce NotFound; connection/registry errors remain failures. Add serde_json only. Keep executable argv after -- literal and cwd at workspace root.

## Task 2: Terminal read/send

Add dedicated control requests without relaxing dashboard selection/raw-input/resize restrictions. Snapshot text and dimensions under the parser lock. Return plain text or JSON with terminal ID, dimensions, text. Read must not select, resize, or mutate parser. Send uses parser bracketed-paste mode and existing encoding. Hold the writer lock for the entire paste/submission operation, write Enter separately, and release parser/session-map locks before writing. Backpressure must not block inspection/termination. Reject missing/exited targets. Acknowledgement means written input, not program completion.

## Task 3: Close and compatibility

Close is one server lifecycle operation using existing termination escalation. Drain PTY, join threads, verify group disappearance, then remove. On failure retain the record. Clear selected session and publish hierarchy. Reuse lifecycle internals without nested mutation locks. Append protocol variants, preserving existing message layouts. New commands require the updated server; document coordinated upgrade and never automatically stop existing sessions. No registry migration.

## Test plan

- CLI aliases, existing commands, literal argv, filters, missing targets, JSON and stdout/stderr separation.
- Offline reads return persisted metadata without starting a server.
- Real PTY Unicode/alternate-screen/final-screen reads; send/no-submit/multiline/concurrent paste through child acknowledgements.
- Background read/send leave dashboard selection/size unchanged; backpressured send does not block list/kill.
- Close running/exited sessions with process-group disappearance; retain records on cleanup failure.
- Existing workspace dirty/ownership/session guards and branch preservation.
- Run CLI, server lifecycle, Git lifecycle, TUI, terminal acceptance suites with nonzero counts; formatting and Clippy.

## Handoff

README includes register/create/send/read/close/remove transcript. Write one Work diary/Personal entry. No cloning, scratch workspaces, tags/rename, scripts, integrations, provider adapters, remote hosts, scrollback, streaming, terminal attach, context inference, or new persistent IDs.
