# Workspace launch and automatic session titles

Approved design: create a workspace and its first selected agent in one flow; terminal naming is optional; application OSC titles update automatic display names; manual rename pins a name and clearing it restores Automatic. Session identity stays stable.

## Interview status

The user confirmed shared understanding after grill-with-docs: “ok, it is settled”. Implementation resumed with the decisions below.

### Settled decisions

- Creating a workspace offers its first agent in the same flow. Launch is the default; “Nothing yet” creates an empty workspace.
- Automatic titles follow each application-supplied title immediately, including status-like titles. Activity indicators remain separate.
- Manual titles stay pinned until edited or returned to Automatic.
- Start offers Agent, Terminal, or Nothing yet. Agent selects a detected/configured preset; Terminal opens the default shell or an optional custom command. A new project defaults to Terminal. Selection remains visible before launch.
- Session identity stays stable as titles change.
- If workspace creation succeeds but agent launch fails, preserve the workspace and offer Retry, Choose another agent, and Open shell. Retry launches into the existing workspace; it never repeats workspace creation.
- Remember successfully launched Agent + preset or Terminal per project across app restarts. Never remember one-off command text. Choosing Nothing yet leaves the preference intact.
- If the remembered agent is unavailable, show that condition and require an explicit replacement instead of silently launching a shell.
- Preserve identical application titles and display a short session identifier alongside duplicates. Manual titles remain available.
- When a successfully launched first agent finishes or crashes, retain the exited session and its output. Relaunch or another session requires explicit user action; the workspace remains available.

## Implementation

- [x] Runtime/protocol: append CreateWorkspaceWithLaunch { project, name, branch, launch: Option<SessionLaunch> }; SessionLaunch contains argv and label and SetSessionTitle { session, title: Option<String> } requests. Add SessionSummary.title: Option<String> and display_name() -> &str. Empty CreateSessionRequest.name requests automatic naming; allocate unique stable names from workspace name, use emitted OSC 0/2 titles for display. Explicit names remain pinned. Bound/sanitize titles. Use vt100 callbacks. Publish SessionChanged on effective title changes, including hidden sessions. Bump wire version and fixtures. Reuse create_workspace_inner and its mutation lock/partial-failure handling; selected first session replaces default shell, return CreatedSession when launched and Ok for an empty workspace. Add RelaunchSession for an exited session: explicit action starts a new session with the same command in the existing workspace and retains old output.
- [x] TUI: workspace form adds Start (Agent/Terminal/Nothing yet), conditional Agent preset and optional Terminal command; submit one CreateWorkspaceWithLaunch request. Standalone creation shares these launch choices without Nothing yet. Persist successful choices per project, never custom command text. Partial launch failure switches to recovery actions targeting the existing workspace. Missing remembered agents require explicit replacement. Standalone terminal Name defaults empty and optional. Add Rename terminal palette action with empty value restoring Automatic; use display_name() for session labels while commands continue to target SessionId; disambiguate duplicate titles using session IDs. Keep desktop notification identity unchanged. Cover successful launch, custom command, default fallback, title events and failure retention through existing entry-point tests.
- [ ] Verification: focused RED/GREEN tests with real PTY/Git/socket paths, workspace tests and clippy, wire fixtures, native disposable GUI create/select/type/title/pin/reset checks. Update user docs and computer-use flow. Review exact final diff independently, resolve findings, open PR without merging. Record results and limitations in diary.

## Boundaries

Synchronous server, one owner/dashboard, 50 sessions. No live server transition or credentialed agent probe. Native title acceptance uses an owned shell emitting OSC sequences. Provider-specific automatic title behavior remains unverified unless observed.

## Acceptance evidence

See [native captures and validation](../docs/ux/2026-09-15/automatic-session-titles/README.md). Local verification passed: 858 tests, 16 ignored; Clippy, formatting, doctests, Node tests, native CUA and independent diff review. PR creation, exact-commit review and hosted CI are delivery gates tracked in the PR; merge remains unauthorized.
