# Operator management skills

## Problem Statement

OVRCR users need the Operator to help manage their Projects, Workspaces, and Retained sessions, not just coordinate delegated coding. Routine questions include what needs attention, which Agents are blocked, whether an interrupted conversation can be resumed, and which old sessions or unused Workspaces can be cleaned up.

The current Operator has shared CLI guidance and one explicitly delivered coordinator skill. Adding more instruction documents alone does not make them available. Users also lack a supported way to discover the available Operator skills and select one explicitly.

Housekeeping needs a clear safety boundary. Last activity is not proof that work has finished. A running Agent may be waiting at its composer, a stopped process may have unresolved background ownership, and a clean Workspace may still hold provider history. Users need reviewable Cleanup proposals that distinguish these cases and cannot act on replacement sessions or newly active work after approval.

## Solution

Deliver a maintained collection of five Operator skills with OVRCR:

| Skill | User-facing purpose |
| --- | --- |
| `ovrcr-status` | Summarize inventory, capacity, reporting failures, blocked work, and results needing attention. |
| `ovrcr-workspaces` | Inspect and register Projects, create or reuse Workspaces, and launch requested Terminals or managed Agents. |
| `ovrcr-coordinator` | Coordinate bounded work through visible Workspace Agent sessions and verify asynchronous results. |
| `ovrcr-session-care` | Diagnose session conditions and carry out explicitly requested control or conversation recovery. |
| `ovrcr-cleanup` | Find Stale-session candidates and unused Workspaces, propose exact changes, execute approved actions, and verify the results. |

Users can describe their task normally, browse a searchable `/skills` picker, or submit `/skill:<name> [request]`. Choosing a skill in the picker prepares the shared draft and waits for Send. Explicit skill invocation guarantees that the selected instructions are supplied before model work; an unavailable selection fails visibly instead of silently becoming a normal request.

Status and general Cleanup proposals cover the whole current instance by default and honor narrower requests. The default stale-review filter is 24 hours since Last activity, with per-request overrides. General cleanup first returns a proposal and waits for approval. It may propose stopping and archiving reviewed idle Agents and separately removing clean, unused non-Root Workspaces, while preserving active work, branches, Retained records, provider history, and uncertain ownership.

The collection is shipped with the matching OVRCR binary, independent of the source checkout and personal Pi configuration. New routines run on request. Existing agent-completion watches retain their behavior; periodic reports are a separate follow-up.

## User Stories

1. As an OVRCR user, I want to ask what needs attention across the instance, so that I can find blocked work and review results without visiting every Workspace.
2. As an OVRCR user, I want a summary of Projects, Workspaces, and session states, so that I can understand the current inventory before changing it.
3. As an OVRCR user, I want Workspace session capacity reported separately from the Operator's dedicated slot, so that I know whether another Agent can be launched.
4. As an OVRCR user, I want status reports to distinguish observed activity from unavailable reporting, so that missing telemetry is not mistaken for inactivity.
5. As an OVRCR user, I want status and general Cleanup proposals to cover the whole instance by default, so that old work in another Project is not overlooked.
6. As an OVRCR user, I want to narrow a request to a Project, Workspace, or session, so that unrelated resources stay outside the requested work.
7. As an OVRCR user, I want each report to state its scope, so that I can tell which resources were inspected.
8. As an OVRCR user, I want inventory inspection to preserve Unread and Input requests, so that asking for status does not review results or answer questions for me.
9. As an OVRCR user, I want routine inspection to start with inventory metadata, so that it does not unnecessarily read every Agent conversation.
10. As an OVRCR user, I want to inspect an existing Project before provisioning, so that the Operator reuses the intended repository rather than creating a duplicate registration.
11. As an OVRCR user, I want a repository registered only when I request it, so that inspection alone does not add Projects or start sessions.
12. As an OVRCR user, I want new feature Workspaces created from an explicit base, so that the Operator does not guess which branch my work should start from.
13. As an OVRCR user, I want suitable existing Workspaces reused, so that routine setup does not produce duplicate checkouts.
14. As an OVRCR user, I want ambiguous or detached Workspace targets resolved explicitly, so that an action reaches the intended checkout.
15. As an OVRCR user, I want Root workspace protections preserved, so that delegated work and cleanup cannot remove or silently rearrange my repository checkout.
16. As an OVRCR user, I want requested Terminal or managed Agent launches verified, so that a shell placeholder or trust dialog is not reported as a ready Agent.
17. As an OVRCR user, I want unexpected trust or approval prompts surfaced instead of guessed at, so that the Operator does not make an unrelated permission decision.
18. As an OVRCR user, I want every action to use the current instance and verified resource identities, so that the Operator's working directory or a later Dashboard selection cannot retarget my request.
19. As an OVRCR user, I want to ask why an Agent appears stuck, so that the Operator distinguishes work in progress, an Input request, a reporting failure, and an exited or interrupted session.
20. As an OVRCR user, I want session care to use the appropriate screen or conversation read path for the Agent, so that its diagnosis reflects the actual session rather than command echo or an unrelated transcript.
21. As an OVRCR user, I want a supported conversation resumed only through an explicit request and its verified retained reference, so that recovery does not create a different conversation or replay old work.
22. As an OVRCR user, I want uncertain process ownership reported as a recovery blocker, so that the Operator never acknowledges stopped processes merely to clear an error.
23. As an OVRCR user, I want explicit pause, resume, stop, and reopen requests to respect existing lifecycle and input ownership guards, so that session care does not bypass the person using the Dashboard.
24. As an OVRCR user, I want a failed recovery attempt to preserve the Retained session and its evidence, so that another attempt remains possible without losing context.
25. As an OVRCR user, I want independent delegated coding tasks to use separate Workspaces and branches, so that concurrent Agents do not overwrite each other's work.
26. As an OVRCR user, I want suitable visible Agent sessions reused for requested delegation, so that the Operator does not create unnecessary processes.
27. As an OVRCR user, I want delegation to return a task, Workspace, and session mapping immediately, so that I can continue using the Operator while the Agents run.
28. As an OVRCR user, I want completion watches tied to the dispatched session run and pre-send observations, so that an old Ready observation is not reported as the new task's result.
29. As an OVRCR user, I want delegated coding results checked against Workspace state and reported check outcomes, so that Ready or an Agent's completion claim is not mistaken for success.
30. As an OVRCR user, I want factual blockers handled within my existing request and human decisions brought back to me, so that Agents can progress without expanding their authority.
31. As an OVRCR user, I want removing a completion watch to leave its session running, so that stopping monitoring does not stop my work.
32. As an OVRCR user, I want finished delegated sessions left available for review, so that completion alone does not hide their results.
33. As an OVRCR user, I want a general cleanup request to produce a Cleanup proposal before any resource changes, so that I can review exactly what would happen.
34. As an OVRCR user, I want the proposal to identify each Project, Workspace, session run, observation, proposed action, and blocker, so that my approval is informed and specific.
35. As an OVRCR user, I want stale review to use 24 hours without recorded activity by default, so that routine requests have a predictable starting point.
36. As an OVRCR user, I want to override the review threshold in my request, so that I can inspect a shorter or longer period without changing persistent settings.
37. As an OVRCR user, I want Last activity interpreted as Agent-state change or user input rather than terminal output or focus, so that the review uses OVRCR's actual domain meaning.
38. As an OVRCR user, I want missing activity times to remain unknown, so that the Operator does not invent an age and classify a session as safe.
39. As an OVRCR user, I want age to identify a review candidate rather than prove completion, so that long-running work is not declared abandoned because it is quiet.
40. As an OVRCR user, I want Busy and intentionally paused sessions protected from a general sweep, so that housekeeping does not interrupt active or deliberately suspended work.
41. As an OVRCR user, I want sessions with Unread results preserved for review, so that cleanup does not hide a response I have not handled.
42. As an OVRCR user, I want sessions waiting for input surfaced as attention items, so that a question is not treated as evidence that an Agent is disposable.
43. As an OVRCR user, I want actively watched and task-owned work protected, so that general housekeeping does not interfere with existing coordination or task lifecycles.
44. As an OVRCR user, I want unavailable reporting and uncertain ownership treated as blockers to inferred safety, so that absence of evidence cannot authorize a stop.
45. As an OVRCR user, I want a reviewed idle live Agent eligible for an explicit stop-and-archive proposal, so that completed interactive Agents can be reclaimed even when their processes remain open.
46. As an OVRCR user, I want stopped sessions with resolved ownership eligible for archiving, so that inactive records can leave the active list without being permanently deleted.
47. As an OVRCR user, I want the proposal to distinguish stopping a process, archiving its record, and removing Workspace files, so that I know the effect of each approval.
48. As an OVRCR user, I want approval bound to the listed resource identities and actions, so that it cannot authorize a replacement session or a different Workspace using the same name.
49. As an OVRCR user, I want changed runs or newly active work rejected at action admission, so that an old approval cannot stop work that began after the proposal.
50. As an OVRCR user, I want to approve only selected actions, so that unrelated candidates remain unchanged.
51. As an OVRCR user, I want changed or refused candidates reported separately from completed actions, so that partial progress does not become a misleading all-clear.
52. As an OVRCR user, I want unknown or partial command effects reconciled without blind replay, so that a lost response does not trigger a second destructive action.
53. As an OVRCR user, I want a direct instruction to close a named session treated as explicit authorization without redundant confirmation, so that precise session-control requests remain practical.
54. As an OVRCR user, I want unused non-Root Workspace removal proposed separately from session cleanup, so that approval to archive an Agent does not also approve deleting checkout files.
55. As an OVRCR user, I want dirty Workspaces excluded from general cleanup, so that uncommitted files are not discarded or silently pushed to make removal possible.
56. As an OVRCR user, I want unmerged or unpublished work called out in a Workspace proposal, so that a clean checkout is not confused with work that is no longer valuable.
57. As an OVRCR user, I want active tasks, unresolved ownership, and provider history inside a checkout to block Workspace removal, so that cleanup preserves ongoing work and recovery data.
58. As an OVRCR user, I want Workspace cleanup to keep branches and Retained records, so that I can still find prior work and conversation references afterward.
59. As an OVRCR user, I want permanent record deletion, branch deletion, and Project unregistration excluded from a general sweep, so that those different actions require separate explicit requests.
60. As an OVRCR user, I want closing a cloud attachment distinguished from cancelling remote work, so that cleanup does not falsely claim remote work or charges have stopped.
61. As an OVRCR user, I want Operator conversations excluded from generic session cleanup, so that a Workspace sweep cannot close or archive the Operator itself.
62. As an OVRCR user, I want a searchable catalog showing every shipped skill's name and purpose, so that I can discover the procedures available to me.
63. As an OVRCR user, I want picking a skill to preserve my shared draft and captured target, so that browsing procedures does not discard text or redirect the request.
64. As an OVRCR user, I want picker cancellation to leave my draft intact and start no model work, so that I can explore the catalog without committing to a request.
65. As an OVRCR user, I want a named shortcut for a known skill, so that repeat use does not require opening the picker.
66. As an OVRCR user, I want a bare named invocation to follow the skill's normal workflow and ask for missing required information, so that explicit selection does not invent a target or permission.
67. As an OVRCR user, I want Explicit skill invocation to supply the full selected instructions before model work, so that selecting a procedure is stronger than mentioning its name.
68. As an OVRCR user, I want unknown, unavailable, or stale explicit selections to fail visibly before model work, so that they cannot silently execute as unrelated ordinary requests.
69. As an OVRCR user, I want arbitrary slash-like request text to remain ordinary data unless it is a supported Operator command, so that adding skill selection does not expose native Pi command execution.
70. As an OVRCR user, I want the picker and named shortcuts available from full Operator and Quick message, so that both entry points can use the same procedures and draft.
71. As an OVRCR user, I want the complete skill collection to work in an installed OVRCR without its source checkout, so that runtime behavior does not depend on a development directory.
72. As an OVRCR user, I want supporting skill references and scripts delivered with their documents, so that the Operator does not follow broken references during a task.
73. As an OVRCR user, I want only OVRCR-shipped skills available in built-in Operator, so that personal or project instructions are not silently imported into the instance-wide assistant.
74. As an OVRCR user, I want a worker's skills and descriptions to remain consistent through warm turns, resume, and compaction, so that an ongoing task does not silently change procedure.
75. As an OVRCR user, I want updated skill instructions adopted on a later cold worker start without interrupting live work, so that an update does not stop my sessions merely to refresh guidance.
76. As an OVRCR user, I want selecting a skill to grant no new tools or permissions, so that a procedure cannot bypass the existing Server and user-authorization boundaries.
77. As an OVRCR user, I want new housekeeping checks to run only when requested, so that this feature does not introduce periodic model use or unattended cleanup.
78. As an OVRCR maintainer, I want one shipped catalog to drive delivery, discovery, and explicit selection, so that the picker cannot advertise a skill the worker cannot supply.
79. As an OVRCR maintainer, I want incomplete or invalid required bundled assets to prevent worker startup before prompting, so that installed failures cannot produce partially advertised procedures.
80. As an OVRCR maintainer, I want leftover unregistered skill documents excluded from discovery, so that removing a skill from the shipped collection actually removes it from availability.

## Implementation Decisions

1. **Deliver the full catalog.** Status, Workspace management, the existing coordinator, session care, and cleanup are all required for this feature. Implementing bundle delivery and one demonstration skill alone does not complete it. Each skill defines its triggers, required observations, allowed actions, failure handling, and completion criteria.

2. **Extend existing Operator delivery.** Use the existing compiled-asset and private atomic-materialization mechanism. Deliver each complete owned skill directory, including required references and scripts, independently of the source checkout. Preserve private ownership, permissions, and symlink refusal. Complete and validate required materialization before launching Pi; failures prevent prompting.

3. **Keep a single shipped catalog.** The Operator runtime owns the authoritative set of skill identities, descriptions, documents, and supporting assets. Materialization, Pi launch arguments, Dashboard discovery, and explicit submission must agree with it. Keep automatic skill discovery disabled and supply exact registered skill-document paths explicitly. Do not scan leftover directories, personal profiles, or Project skill collections. Keep built-in Operator separate from the managed-Pi orchestrator package and its different tool contract.

4. **Preserve the existing tool and ownership model.** Operator retains its native read, bash, edit, and write tools plus the existing completion-watch tool. A skill is procedural guidance, not a capability grant. Keep one synchronous Server owner, one active Dashboard, fifty live Workspace session slots, and the Operator's separate live slot. Add no skill server, workflow controller, background watcher, or generic tool framework.

5. **Keep shared policy separate from procedures.** The fixed Operator policy owns instance identity, targeting, privacy, and authorization boundaries. The shared CLI guide remains system guidance. Task-specific procedures live in skills rather than being duplicated across the system policy and every document. Align targeting language with the approved instance-wide inspection defaults without broadening mutation authority.

6. **Use existing Dashboard interaction patterns.** Extend the Operator command catalog and existing searchable-picker mechanisms. `/skills` lists shipped names and purposes. Selection stages the named invocation in the shared draft, preserves existing request text and captured context, and waits for Send. Cancellation preserves the draft. Listing and selecting perform no model work. Quick message may transition to full Operator for the picker while preserving its shared draft and target.

7. **Represent explicit selection independently of arbitrary text.** Carry a validated shipped skill identity through the normal accepted-request path. The worker supplies the corresponding bundled instructions before model work, retaining captured context first, then the selected instructions and their reference location, then the distinct user request. Reject unknown, unavailable, or incompatible selections rather than silently substituting a procedure. Preserve accepted-turn identity and existing journal and checkpoint behavior.

8. **Do not forward native skill commands.** The named Operator shortcut is an Operator interface, not general native Pi command passthrough. Preserve the ordinary request wrapper's protection against user text becoming a native command. Native Pi skill expansion requires a different prefix and may fall through on errors, so explicit invocation is expanded by OVRCR inside its normal wrapper. Natural-language discovery still advertises metadata and lets the model read relevant instructions on demand.

9. **Freeze delivery at worker boundaries.** A worker uses its fixed shipped catalog and materialized documents through warm turns and checkpoint operations. Refresh instructions only through the matching installed binary on a later cold worker start. Do not introduce hot reload or stop live sessions to adopt a new bundle. UI catalog information and submissions must not silently disagree with the active worker.

10. **Resolve scope and targets explicitly.** Status and general Cleanup proposals default to all registered Projects in the current instance and report their scope. Honor narrower requests. Use the inherited matching executable, instance configuration, and socket. Resolve resource identities from current inventory; never infer them from the Operator's fixed working directory, a display title, or a later Dashboard selection. Begin with metadata and read session content only when relevant to the requested workflow.

11. **Preserve existing provisioning contracts.** Inspect before registering or creating. Reuse suitable resources, select explicit branch bases, resolve ambiguous checkouts, and preserve Root protections. Launch requested Terminals and managed Agents through supported routes, verify actual readiness, and surface unexpected trust or approval interactions. Do not infer a usable Agent from its label or from successful process creation alone.

12. **Preserve session recovery contracts.** Use current phase, activity, Reporter health, screen or conversation evidence, and verified Conversation references. Keep inspection separate from review, input, and ownership acknowledgement. Explicit lifecycle actions retain their current stop-proof, input ownership, and conversation-resume guards. Uncertain ownership and unsupported recovery remain visible blockers; no generic acknowledgement, prompt replay, or new-conversation fallback is introduced.

13. **Reuse the existing coordinator.** Keep bounded visible Workspace Agent delegation, separate coding Workspaces, pre-send run/activity/unread baselines, asynchronous watches, provider-appropriate reply inspection, and verification of Workspace state and reported checks. Preserve factual-blocker handling, escalation of human decisions, immediate return after dispatch, and the distinction between removing a watch and stopping its session. Ready and worker trailers are observations, not acceptance of task success.

14. **Classify stale candidates conservatively.** Use the existing Last activity meaning and 24-hour default with per-request overrides; add no persistent threshold setting. Missing times remain unknown. Age only selects a candidate for inspection. Busy, paused, Unread, WaitingInput, ownership-uncertain, actively watched, and task-owned work stays protected from a general sweep. Missing or unavailable reporting is not proof of an idle or safe session. Generic cleanup excludes Operator conversations.

15. **Separate proposal, approval, and action.** A general cleanup request returns exact candidates, observations, proposed actions, and blockers before any change. Approval may cover selected listed actions and does not authorize future resources that reuse a name or identity. A direct instruction to close a named session remains an explicit instruction without an additional generic confirmation. Stop-and-archive of a reviewed idle live Agent must be labeled as such; archiving a stopped record and removing a Workspace are distinct effects.

16. **Enforce approved state at Server admission.** Refresh evidence before execution and enforce the approved identity and relevant state in the same admission path that performs the action. An earlier parent-side refresh alone is insufficient. Extend existing typed CLI and Server preconditions only where needed to reject replacement runs, newly active work, or changed approved targets. Preserve current controlled-stop proof, refusals, and recovery behavior. Do not use saved PIDs or display names as ownership proof.

17. **Keep Workspace removal separately guarded.** Require a clean, unused non-Root Workspace, resolved session ownership, no active task run, and no provider-history dependency inside the checkout. Explain unmerged or unpublished work in the proposal even though branches remain. General cleanup never forces removal, discards changes, or silently pushes work to make removal possible. Preserve branches and Retained records. Permanent record deletion, branch deletion, and Project unregistration need separate explicit requests.

18. **Report and reconcile partial outcomes.** Verify resulting inventory and Workspace state. Distinguish completed, skipped, refused, and uncertain actions. A changed candidate does not broaden approval. Unknown or partial effects require reconciliation of the original action, not replay under a new intent. Preserve uncertain resources and recovery evidence. Closing a cloud attachment must not be reported as cancelling its remote work or charges.

19. **Reuse existing state owners.** Skill delivery is an extension of Operator runtime composition, not a new durable store. Reuse existing accepted-turn, journal, checkpoint, protocol, and lifecycle ownership mechanisms. Add typed fields or conditional requests only as necessary and honor normal wire compatibility and migration rules. Do not invent a second settings source, a per-skill permission system, or a new persistent skill database.

20. **Keep documentation aligned with the feature.** Update Operator usage, CLI help/reference for any changed public preconditions, development guidance, and affected configuration examples in the same implementation. Preserve the domain distinctions between Operator skill, Explicit skill invocation, Cleanup proposal, Stale-session candidate, Retained session, Archived session, Last activity, Ready, Unread, and Input request.

## Testing Decisions

1. **Use one primary existing end-to-end seam.** Extend the production Dashboard-to-Server-to-Operator-worker path driven through the existing Dashboard driver and isolated live-instance fixture. Use installed Pi with the existing local controlled provider to inspect real model requests and execute actual native tools and CLI actions against task-owned Projects, PTYs, sessions, and Git Workspaces. This is the main feature acceptance seam; do not introduce a separate test-only Operator constructor or a parallel management engine.

2. **Test observable behavior.** Assert visible picker contents, draft and target preservation, whether a model request occurs, which instructions reach that request, actual CLI effects, returned refusals, and retained resources. Metadata formatting, mocked tool calls, private helper outputs, and a successful input write cannot alone prove that a skill was delivered, a request was submitted safely, or a resource was cleaned up.

3. **Reuse concrete prior art.** Existing Operator acceptance covers bundled guidance in the system prompt, a relocated first-turn CLI request without a checkout, full and Quick message submission, model-picker interaction, warm leased chat, resumed conversations, compaction, and rollback. The existing tests named `installed_pi_operator_bundled_skill_system_only`, `installed_pi_operator_bundled_skill_first_turn_cli`, and `dashboard_slash_model_picker_and_switching_work_end_to_end` establish the relevant patterns. Extend their owning fixtures rather than creating new global test infrastructure.

4. **Verify complete isolated delivery.** Exercise all five advertised skills in an installed-style fixture without a source checkout, personal skills, or Project discovery. Assert that supporting references and scripts resolve, exact registered files are selected, unregistered leftovers stay invisible, private ownership protections remain effective, and required missing or invalid assets prevent prompting. Confirm actual installed Pi behavior in the explicit installed-host gate; fake-host composition tests are additional evidence, not a substitute.

5. **Verify natural and explicit invocation separately.** Natural selection must advertise the shipped metadata and permit normal instruction reads. Explicit selection must supply the correct full document before the first model request, preserve context-before-user ordering, and reject unknown, unavailable, and stale identities without model work or fallback. Exercise ordinary slash-like text and unsupported commands so the existing native-command boundary remains intact.

6. **Drive both entry points through real input and rendering.** Cover searching the picker, choosing a skill with an existing draft, cancellation, direct named invocation, bare invocation, and transition from Quick message into full Operator. Assert selection alone sends nothing, Send uses the preserved context, and busy or stop-unconfirmed states keep the draft. Reuse existing shared-draft, searchable-model-picker, slash-menu, and Quick-message regression patterns. Cover narrow and tiny layouts without weakening existing assertions.

7. **Exercise the five real workflows.** Through the primary seam, demonstrate a read-only instance overview, explicit narrower inspection, requested Project/Workspace provisioning and Agent readiness, coordinator dispatch and wakeup, session diagnosis and supported recovery, a no-change Cleanup proposal, and exact approved cleanup effects. Observe real inventory, process, and Git results rather than treating a worker's own success claim as proof.

8. **Use deterministic lifecycle regressions for approval races.** Extend existing public Server/CLI lifecycle seams only for precise race cases that cannot be reliably forced through model timing. Reuse the stale-close regression pattern established by `stale_close_of_previous_run_cannot_revoke_or_stop_reopened_run` and existing input/state tests. Force replacement-run and newly-active-work orderings; verify that the approved target is rejected and the replacement stays alive. Do not rely on sleeps or a thread merely having started.

9. **Cover cleanup classifications and failures with real resources.** Include the 24-hour default and request overrides, null times, Busy, paused, Unread, WaitingInput, unavailable reporting, uncertain ownership, active watches, task-owned work, and Operator scope. Prove that age, titles, Ready, and a completion trailer cannot independently authorize cleanup. Cover partial approval, changed candidates, refused actions, lost/unknown effects, and verified final inventory.

10. **Reuse existing Git and retention protections.** Extend the real Workspace-removal and Retained-session fixtures. Prior art includes Last activity persistence, live/uncertain removal blockers, ignored provider-history holders, archive context after Project removal, and recovery across restart. Verify dirty files, Root, active tasks, uncertain archived rows, branches, provider history, and records remain protected, including races between proposal and removal. An attachment closing is not proof of remote cancellation.

11. **Verify lifetime and compatibility behavior.** Check cold starts, warm turns, restored checkpoints, compaction, busy states, and failed or interrupted turns through their existing production paths. Assert consistent catalog identities and instruction delivery, no silent skill substitution, no unrequested replay, no new periodic execution, and no regression to the one-conversation, one-owner, fifty-Workspace-session contracts. Typed request changes must include compatibility and refusal coverage.

12. **Keep native UI proof separate.** Run the actual reviewed application in an isolated instance through a capable computer-use harness. Retain screenshots and relevant accessibility evidence for the full and Quick entry points, searchable catalog, staged invocation, cancellation, Send, errors, and visible results. Headless render tests and a programmatic Dashboard driver do not prove native keyboard or mouse behavior.

13. **Preserve evidence and ownership boundaries.** Give every live test private configuration, sockets, profiles, repositories, and process groups. Keep attempts and failures, confirm filters execute tests, and stop only task-owned resources. Run focused owning suites during development and required acceptance, regression, formatting, and lint gates on the actual delivery revision. Unavailable, skipped, failed, and unrun gates remain unverified.

## Out of Scope

- User-installed or Project-installed Operator skills, ambient profile discovery, or custom skill trust configuration.
- Generic native Pi slash-command passthrough or additional Operator tool capabilities.
- A skill server, plugin framework, new workflow controller, persistent skill database, or second settings source.
- Hot reload of a warm worker's catalog or interrupting live work to refresh instructions.
- New periodic Operator checks, recurring model work, or scheduled cleanup.
- Automatic permission to stop, archive, delete, publish, push, or deploy merely because a skill was selected.
- Forceful general cleanup of dirty Workspaces, uncertain ownership, active work, provider-history dependencies, or Root Workspaces.
- Permanent record deletion, branch deletion, or Project unregistration as implied effects of a general cleanup request.
- Changes to provider authentication, subscription entitlement, account configuration, or the managed-Pi orchestrator's separate capabilities.

## Further Notes

The user confirmed the full catalog, delivery and invocation contracts, instance-wide defaults, cleanup boundaries, and worker-boundary updates through five design rounds. This spec synthesizes those decisions; it does not reopen them or authorize implementation in the spec-publication task.

Use the project's domain vocabulary consistently. An Operator skill provides guidance rather than authority. Explicit skill invocation differs from picker selection. A Stale-session candidate is not an abandoned or safely stopped session. A Cleanup proposal is not standing permission. Ready does not establish task success, and viewing inventory does not review Unread results.

[GitHub #496](https://github.com/xlyk/ovrcr/issues/496) tracks opt-in scheduled read-only Operator housekeeping reports separately. Its report destination, schedule controls, recurring model-use consent, downtime, and overlap behavior remain outside this spec. It must not be treated as permission for scheduled cleanup.

Relevant prior work includes [the bundled coordinator and Workspace delegation workflow](https://github.com/xlyk/ovrcr/issues/470), [Operator asynchronous delegation](https://github.com/xlyk/ovrcr/issues/465), and [Operator slash commands and conversation-tree navigation](https://github.com/xlyk/ovrcr/issues/481). Their retained-session, journal, watch, and input-ownership behavior remains the baseline to preserve.

Feature acceptance is not complete at specification time. Required implementation checks and native evidence must pass on the delivered revision before the feature is reported as implemented.
