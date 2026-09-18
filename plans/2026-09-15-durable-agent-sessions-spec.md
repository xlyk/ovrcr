# Retain sessions in SQLite and restore agent conversations through native resume

## Problem Statement

After the OVRCR server restarts, the user sees their projects and workspaces but loses the agent sessions associated with them. They must rediscover conversations and reconstruct their working layout even though the agents already store their own conversation histories. Process lifetime currently determines whether a session remains available.

Users need their sessions to remain recognizable and reopenable until they deliberately close them. A restart or reboot should not make their ongoing work disappear.

## Solution

Retain every interactive Agent and Terminal session in SQLite alongside its project and workspace. Persist session metadata and exact provider conversation references; do not persist terminal output, previews or agent transcripts. Agents remain responsible for their conversation histories.

Restore the session list after server restart or reboot. Following the Superset behavior investigated for this feature, displaying an eligible interrupted Agent session automatically launches the provider's native resume command without submitting a new task prompt. This is lazy recovery when the session is displayed, not an eager launch of every saved session. A live session is reattached without launching another process. Explicitly stopped sessions and shells require an explicit reopen action.

Closing archives a session. Archived sessions remain searchable and can be returned to the active list without starting a process. Only live terminal processes count toward the fifty-session limit.

## User Stories

1. As an OVRCR user, I want sessions retained by default, so that I do not have to opt in before an unexpected restart.
2. As an OVRCR user, I want my projects and workspaces restored, so that my session list retains its organization.
3. As an OVRCR user, I want each session to retain its identity and title, so that I recognize the same work after reopening the app.
4. As an OVRCR user, I want session records to survive a reboot, so that recovery does not depend on an old process remaining alive.
5. As an OVRCR user, I want a displayed interrupted agent to reopen its exact conversation automatically, so that I can return to its context without reconstructing a resume command.
6. As an OVRCR user, I want recovery to send no new task prompt, so that reopening does not manufacture another instruction to the agent.
7. As an OVRCR user, I want reconnecting to a live session to reuse its process, so that I do not create duplicate agents.
8. As an OVRCR user, I want undisplayed retained sessions to remain dormant, so that opening the app does not launch every historical agent.
9. As an OVRCR user, I want a deliberately stopped agent to remain stopped until I reopen it, so that the app respects my decision to stop work.
10. As a Claude user, I want my saved Claude conversation resumed by its exact native identifier, so that I continue the intended conversation.
11. As a Codex user, I want my saved Codex conversation resumed by its exact native identifier, so that another conversation in the same directory is not selected accidentally.
12. As a Pi user, I want native conversation resume, so that Pi remains responsible for recovering its context.
13. As an Oh My Pi user, I want verified native conversation resume, so that OMP support does not depend on Superset having an adapter for it.
14. As a user of another configured agent, I want its session row retained with a clear resume capability state, so that unsupported recovery does not erase the record or pretend to work.
15. As an OVRCR user, I want a missing conversation reference or missing provider history explained, so that I understand why recovery cannot proceed.
16. As an OVRCR user, I want a failed resume to retain my session and offer Retry, so that one failure does not discard my work context.
17. As an OVRCR user, I want starting a new conversation to be a separate action, so that a failed recovery never silently replaces my old conversation.
18. As an OVRCR user, I want the current conversation reference updated when an agent switches conversations, so that subsequent recovery returns to the last authoritative conversation associated with that session.
19. As an OVRCR user, I want a resumed session to remain recoverable before I send another prompt, so that another immediate restart does not lose its known identifier.
20. As a shell user, I want the terminal's title and working directory retained, so that I can explicitly reopen a fresh shell in the same place.
21. As a shell user, I want reopening to make clear that shell variables, output and background jobs were not restored, so that I understand the fresh process state.
22. As an OVRCR user, I want exiting, crashing or completing an agent response to leave the session visible, so that process events do not remove my working context.
23. As an OVRCR user, I want closing a stopped session to archive it immediately, so that I can tidy the active list.
24. As an OVRCR user, I want confirmation before closing running work, so that archiving does not stop a process unexpectedly.
25. As an OVRCR user, I want an agent's suggestion to close a session to require my acceptance, so that the agent cannot archive my work unilaterally.
26. As an OVRCR user, I want pane hiding and dashboard detachment to preserve sessions, so that layout changes do not stop or archive work.
27. As an OVRCR user, I want to search archived sessions by title and workspace, so that I can find earlier work.
28. As an OVRCR user, I want unarchiving to restore a row without starting a process, so that reviewing the archive does not execute work.
29. As an OVRCR user, I want deleting an OVRCR record to leave provider conversation files untouched, so that managing the app's list does not destroy the agent's history.
30. As an OVRCR user, I want stopped and archived records excluded from live capacity, so that retaining history does not consume the fifty available process slots.
31. As an OVRCR user, I want live capacity exhaustion to leave recovery pending and visible, so that repeated refreshes do not trigger failed launch storms.
32. As an OVRCR user, I want running sessions to block workspace removal, so that a workspace cannot disappear beneath live work.
33. As an OVRCR user, I want confirmed workspace removal to archive stopped sessions and preserve their original path, so that their context remains recognizable.
34. As an OVRCR user, I want a missing working directory reported before reopening, so that an agent does not resume in an unintended repository.
35. As an existing OVRCR user, I want projects and workspaces migrated once without losing identifiers or duplicating records, so that upgrading preserves my organization.
36. As an OVRCR user, I want my dashboard preferences and scheduled tasks preserved during migration, so that session persistence does not disrupt unrelated features.
37. As a CLI user, I want inventory and session lifecycle operations to agree with the dashboard, including offline inspection, so that automation sees the same durable records.
38. As an OVRCR user, I want uncertainty about an old process to block automatic relaunch visibly, so that recovery does not create competing agents for the same work.

## Implementation Decisions

- **Durable authority:** SQLite owns projects, workspaces and retained interactive session records. The synchronous server remains the one mutation authority, with one active Dashboard. Do not introduce an asynchronous runtime or a separate PTY daemon for this feature.
- **Storage boundary:** Persist stable session identity, project/workspace association, title mode and title, working directory, selected provider/launch configuration reference, exact conversation reference, archive state and the minimum lifecycle/attempt metadata necessary for safe recovery. Do not copy credentials, environments, prompts, terminal screens, scrollback or provider transcripts into the database. Preserve current provider account/configuration references needed to address the same conversation; changed or unavailable configurations must be surfaced rather than silently substituted.
- **Migration:** Import the existing machine-owned registry transactionally and idempotently. Preserve original data for recovery, reject malformed or unsupported input without replacement, and never leave two writable authorities. Once migration succeeds, SQLite is authoritative. Preserve offline readers, configuration-path isolation and the scheduled-task storage namespace. User preferences and remembered launch choices remain in their current settings storage; scheduled-task persistence stays unchanged.
- **Identity:** Retained session identity is independent of each process run and never reused after restart. Resume/reopen preserves the session row and title while creating fresh process ownership and run identity. Late output, lifecycle events and reporting callbacks from a previous run cannot alter a replacement run or archive decision. Existing Relaunch behavior must be deliberately reconciled with this same-session Reopen contract rather than left as a competing recovery path.
- **Capture:** Extend the existing managed-launch and Reporter boundaries to obtain provider-authoritative conversation identifiers. Persist accepted conversation changes, including in-place switches. Never infer identity from the newest file, a working directory, a most-recent-session picker or an arbitrary callback. Missing identifiers remain explicitly unavailable. Once a valid reference has been recorded, a new process attempt must not erase it while awaiting a fresh provider event.
- **Resume coverage:** Verify exact identifier capture and native interactive resume for Claude, Codex, Pi and Oh My Pi. Superset's built-in configuration is evidence of its behavior, not proof of OMP or installed-provider support. Retain rows for other configured agents but expose resume as unavailable until their integration is verified.
- **Automatic trigger:** Restoring inventory is read-only. First display of an interrupted, unarchived, eligible Agent session may request recovery; the server validates and serializes that request. Repeated draws, reconnects or concurrent CLI requests coalesce into one launch attempt. A deliberately exited, explicitly stopped, archived, newly unarchived or failed session does not automatically relaunch; it requires an explicit action. Shells always require explicit reopening. Lack of capacity or an unmet prerequisite leaves the record visible without repeated execution attempts.
- **Ownership:** Persist a reliable OS boot identity with each run. A verified different boot establishes that the old processes are gone and permits eligible automatic recovery. A same-boot crash or unavailable/unverifiable boot identity requires an explicit Dashboard or CLI acknowledgement that the previous agent and its background processes have stopped. Retry alone is not that acknowledgement. Durable PIDs never authorize signaling; server or process-leader disappearance is not proof that descendants exited. This resolution mechanism is new work owned by the common reopening slice, not an already implemented contract.
- **Command execution:** Invoke the provider's native resume command with the exact conversation reference and no new prompt. OVRCR must execute the launch without requiring the user to press Enter. Reuse the existing managed production spawn path wherever possible. Superset achieves this by writing a command then Enter to a shell PTY; copying its transport and fixed delay is not a requirement. If shell command injection is necessary, quote arguments, gate on readiness and prove actual execution rather than command echo.
- **Success and failure:** Distinguish recovery requested, process launched, provider conversation attached, and failed/unavailable. Do not claim the conversation was restored merely because spawn or a terminal write succeeded. A failed attempt preserves the reference, exposes Retry and never silently starts fresh. Explicit Start new conversation creates a separate session, retaining the failed record until the user closes it.
- **Reporting:** Reset live activity, Ready, Unread, input requests and process timing across runs. Retain only the conversation reference needed for recovery; old reporting observations do not establish new-run state. Preserve existing reporting guarantees through native resume for providers that support them. Where a current integration has a documented gap, including initial Codex resume, expose unavailable reporting honestly and verify the stated capability; do not bypass provider trust or approval controls to obtain it.
- **Archive lifecycle:** Explicit close of a stopped session archives it. Closing a running session confirms, stops only the currently owned process group, and archives only after stop succeeds. Stop failure leaves the row actionable and does not falsely report an archive. Unarchive returns a stopped row. Record deletion does not remove provider files. Agent suggestions have no autonomous close authority.
- **Workspace lifecycle:** Running or ownership-uncertain sessions block workspace removal. Existing worktree protections remain in force. Confirm removal with stopped records, retain their original workspace context, and archive them only consistently with the resulting removal outcome. Reopening cannot silently change the working directory; a missing directory remains an actionable failure. Project removal must preserve the same retained context through its workspace lifecycle.
- **Capacity:** Count live and reserved launch slots, not retained records. Admission and recovery share the existing fifty-session limit and cannot oversubscribe it under concurrent requests.
- **Interfaces and documentation:** Update public CLI/socket inventory and lifecycle interfaces, dashboard actions, archive search, error text, configuration help and user documentation consistently. Serialized protocol changes follow the existing versioning contract. Do not expose saved sensitive launch data in inventory or errors.

## Testing Decisions

- **Primary seam:** Exercise the real compiled server through the existing isolated live-server fixture and public CLI/socket requests. Extend that fixture only as needed to restart a binary against the same retained temporary storage. The unit under test is the production application path, not a substitute database repository or mocked recovery coordinator.
- **Prior art:** Reuse the current server lifecycle, resource CLI, offline inventory, automatic-title, Git lifecycle, provider-reporting and TUI fixture patterns. Existing restart tests assert that live sessions and Unread disappear; deliberately update only the session-record expectation while retaining the prohibition on resurrecting old processes or reporting state.
- **Restart regression:** Create sessions through public entry points, capture known provider references, stop or kill only the owned test server, restart against the same SQLite database and inspect preserved identities, titles, placement and stopped/interrupted states. Inventory alone must not launch processes. Displaying an eligible session must launch exactly once. Include a second restart before the first new prompt.
- **Real execution:** Use controlled provider executables inside real PTYs to record received arguments, working directory and invocation count and emit a distinct output marker. Assert exact resume identifier and absence of a task prompt. This proves orchestration, not native provider conversation recovery. If shell input is used, include delayed shell readiness and confirm command execution rather than echoed text.
- **Lifecycle races:** Use deterministic gates to overlap display/reconnect/CLI recovery, archive and late previous-run callbacks. Assert one owned replacement process, stale-event rejection, slot reservation, no unintended resurrection, and retryable failure without loss of the conversation reference. Include controlled live descendants surviving a server exit; automatic recovery must refuse while ownership is unresolved.
- **SQLite and migration:** Exercise first migration, repeated startup, interrupted import, schema incompatibility, malformed legacy data, unavailable/locked storage and persistence failure at lifecycle transitions. Verify original records remain recoverable, committed identities are not reused, archive state survives restart, and no duplicate registry authority appears. Retained session count above fifty must coexist with enforcement of fifty live processes.
- **No-output contract:** Drive unique terminal output through agent and shell sessions, restart and verify it is neither restored nor stored by the interactive retention mechanism. Provider-native files and existing scheduled-task logs are distinct and remain outside this assertion.
- **Archive and workspace outcomes:** Verify close confirmation, failed stop, search, unarchive-without-execution, delete-without-provider-file-deletion, dirty-worktree refusal, confirmed removal with stopped records and missing-directory recovery failure. Run these through public operations with real temporary Git repositories and provider-history sentinel files.
- **Provider acceptance:** For each first-release provider, create an isolated native conversation with a recognizable fact, record its exact identity, restart OVRCR and verify the same native conversation opens. Report each tested harness/version and native/reporting capability separately. Do not substitute helper tests for these checks or run paid/high-concurrency probes without the required bounded authorization and isolated setup.
- **Native TUI acceptance:** Use the actual reviewed app through the existing disposable GUI workflow. Verify visible restored rows, automatic eligible recovery, stopped shells, Retry, archive search and unarchive using real input and screenshots/accessibility evidence. Check macOS and Linux evidence separately; a process-restart test is not a native reboot test.
- **Completion:** Meaningful owning-module tests cover transition predicates and failures where the public seam cannot force them. Run the required workspace regression, formatting, lint and protocol checks at delivery. Any unavailable provider, platform or native recovery gate remains explicitly unverified and prevents claiming full acceptance.

## Out of Scope

- Persisting or reconstructing provider conversation content, terminal output, screen snapshots or shell environments.
- Keeping live PTYs alive across server death through a new daemon, or adopting processes without current ownership proof.
- Automatically submitting a follow-up task, replaying a prior prompt, or automatically launching all retained sessions at startup.
- Cross-host synchronization, cloud backups, provider-account migration or discovering arbitrary conversations started outside OVRCR.
- Automatic archive/delete decisions by agents, automatic expiration of session records, or deletion of provider history.
- Scheduled-task storage migration, task-log retention changes, settings redesign, and broader database conversion.
- A complete provider conversation browser, support for every configurable agent, or relaxing existing readiness/trust/ownership guarantees.
- Git commits, implementation, deployment or merges as part of publishing this specification.

## Further Notes

This specification synthesizes the interview and the subsequent investigation of installed Superset 1.28.0. The SQLite and metadata-only choices came from the interview. Lazy automatic recovery is the research-informed proposal replacing the earlier provisional explicit-reopen-only recommendation; it opens the conversation without injecting a task. The prior opt-in, TOML-based, Claude-only restore plan is superseded by this contract when this spec is accepted; its process-ownership safeguards still apply. Reconcile the provisional glossary/ADRs and older plan during implementation so they do not present competing instructions.

Superset source evidence is pinned to release `a415227dc25f806dd3ef6bf05b464adba5ac08fb`; ten installed package source-map files matched that source exactly. It automatically resumes eligible restored panes, uses SQLite bindings, creates a successor terminal, and writes the native command followed by Enter. OVRCR retains its own stable session identity and production launch boundary. Superset's limited fresh-start fallback for unprompted sessions is deliberately excluded here.

- [Superset automatic recovery](https://github.com/superset-sh/superset/blob/a415227dc25f806dd3ef6bf05b464adba5ac08fb/apps/desktop/src/renderer/routes/_authenticated/_dashboard/v2-workspace/$workspaceId/hooks/usePaneRegistry/components/TerminalPane/components/TerminalAgentAutoResume/TerminalAgentAutoResume.tsx#L28-L40)
- [Superset command execution](https://github.com/superset-sh/superset/blob/a415227dc25f806dd3ef6bf05b464adba5ac08fb/packages/host-service/src/terminal/terminal.ts#L2386-L2404)
- [Superset SQLite recovery metadata](https://github.com/superset-sh/superset/blob/a415227dc25f806dd3ef6bf05b464adba5ac08fb/packages/host-service/src/db/schema.ts#L46-L73)

The research established installed-package/source behavior, not live Superset crash/reboot acceptance. No native OVRCR recovery capability is claimed by this specification before implementation and verification.

### Approved ticket refinements

After publication of parent issue #112, the user selected four refinements. The implementation tickets carry these refinements; the published parent remains unchanged.

- The retained-session/shell-reopening ticket owns the common reopening operation, serialization, capacity reservations, fresh-run fencing and boot-aware ownership resolution. Claude and subsequent providers reuse it.
- A detected unsupported Claude conversation transition durably invalidates recovery eligibility. Any old reference is context-only and cannot become an automatic resume target. Broader Claude switch tracking and an explicit old-conversation action are outside this scope. Temporary reporting failures must not invalidate an otherwise valid reference.
- Pi and Oh My Pi share one implementation ticket, with independent native acceptance for both providers.
- Recovery uses the verified-boot versus same-boot/unknown distinction described above.
