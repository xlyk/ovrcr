Finalize this independent read-only plan review now, using only the supplied plan, design, and observed source excerpts. Your earlier source-inspection phase hit its time limit without a final report. No tools are available in this final phase. Do not ask questions or propose more browsing. Separate substantiated findings from proof gaps; no evidence means no finding. Return at most 5 actionable issues and 1200 words total, with exact path:line citations where present. Each issue: priority, concrete scenario, evidence, and smallest plan correction. Finish PASS / PASS WITH CLARIFICATIONS / NEEDS REVISION for plan quality, never runtime acceptance. Do not reopen submitted user decisions.

You are an independent Oh My Pi plan reviewer using the user-requested DeepSeek V4.1 Flash model. Review only; do not implement or edit any files. Tools are limited to read/grep/glob. Stay inside /Users/xlyk/Code/ovrcr. Never read credentials, .secrets, personal homes, unrelated reports or live app state. No provider/tool execution, network access, nested agents or user questions.

Task: validate docs/superpowers/plans/2026-11-05-agent-session-creation.md against docs/superpowers/specs/2026-11-05-agent-session-creation-design.md and actual owning source. Read AGENTS.md, CONTEXT.md and the relevant narrow development guides. This is an approved design but an unimplemented plan. Do not report desired new behavior as a current-code bug; identify missing/wrong implementation steps or invalid assumptions. Do not reopen explicit user decisions: registered workspaces only; child creation commits; branch drift follows same checkout; pending Create stays open; prompt memory-only per server lifetime; full model/prompt support required for Claude/Codex/Pi/omp; other launchers gated; n agent / separate terminal / w workspace.

Read the connected code needed for each finding. Return only substantiated, actionable plan issues. Cite exact repository-relative path:line for both plan/spec and source evidence. Distinguish blockers from nonblocking clarifications. For each finding give priority, concrete failing scenario, why current plan misses it, and smallest plan correction. Avoid speculative hardening or demands to add new frameworks. If no issue, say no actionable findings and list substantive checks and remaining proof gaps. Finish with PASS / PASS WITH CLARIFICATIONS / NEEDS REVISION, understood as plan quality, never runtime acceptance.

Review axis: approved design compliance and real state transitions. Trace actual palette, keymap, lifecycle completion, hierarchy and view-handshake paths. Stress parent/child draft preservation, project/default edits, branch-label drift, root/detached availability, cancelled parent after child commit, pending/noncancellable creation, disconnect and partial failure, and new terminal entry point. Find concrete missing plan steps or conflicts, not subjective redesign suggestions.


PLAN
# Implement branch-based agent session creation

Status: implementation plan, not executed. Design confirmed by Kyle in Q18 on 2026-11-05. Source baseline inspected: `08d25641d3ef96a2b8f9f74a5710efb2c532b75a`. Recheck current branch/source and create an isolated feature worktree before implementation.

The [confirmed design](../specs/2026-11-05-agent-session-creation-design.md) owns behavior and acceptance. The [research review](../../../research/workspace-session-ux-2026-11-05/review.md) preserves native CUA evidence and all submitted decisions. This plan changes neither server ownership nor workspace identity.

## 1. Prove the four native launch contracts before promising controls

Read `dashboard/agents.rs`, `dashboard/palette.rs::detected_agents`, `src/report/` and the owning provider integration paths. Inspect installed versions and authoritative provider documentation before relying on exact launch/model/prompt APIs. Do not equate Pi and omp.

- Produce a bounded capability matrix for Claude Code, Codex, Pi and omp: model discovery, Harness default semantics, explicit-model application, initial-prompt delivery, native setup/trust and delivery evidence.
- Reuse existing Claude/Codex/Pi model-discovery work in runtime `server/delegation.rs`, `claude_delegation.rs` and `codex_delegation.rs` where its contract fits. It is currently session-authenticated delegation functionality; do not expose that authority or forge a delegation session to serve a Dashboard.
- Preserve arbitrary configured argv and named aliases. A familiar display name does not prove safe model override. Preserve existing cloud account pins and source constraints.
- Begin with installed-artifact/offline inspection and task-owned fixtures. Native or paid validation scope must be explicit before running it; no model calls are part of this planning task.

Exit: a supported mechanism is identified for each required harness, with gaps clearly stated. An unsupported mechanism is a blocker for that harness's required acceptance, not a reason to weaken the requirement or silently hide omp.

## 2. Extend the existing launch path and settings, with tests through real entry points

Affected owners include protocol `wire.rs` (`CreateSessionRequest`, `SessionLaunch`), TUI `AgentEntry`/selection, runtime spawn/managed invocation, and typed settings catalog/loader. Current launch values are argv/label/kind; model and prompt are not structured fields today.

- Add only the structured options needed by the proven provider contracts and validate at the Server trust boundary. Update every constructor, exhaustive match, CLI/caller/fixture and wire compatibility requirement together.
- Route bounded discovery outside the input reader and lifecycle mutation locks; use existing request correlation/ownership patterns. Do not add a second model/provider authority, framework, SDK or settings parser to solve the form.
- Put ephemeral initial-prompt ownership with the exact Server session/run. Preserve pending text across Dashboard reconnects; never serialize text into ordinary retained metadata, settings or Events. Define known refusal versus unknown delivery, with no automatic uncertain replay.
- Extend successful per-project launch memory with model-policy selection through the existing settings owner. Keep Terminal memory and one-off command exclusion intact.

Exit: real request→validation→provider launch paths demonstrate explicit/default model behavior and prompt blank/delivery/refusal/reconnect/restart semantics. Cover invalid/stale targets, capability changes, configuration overrides and missing models without silent fallback.

## 3. Implement the two dialogs using current Dashboard and lifecycle mechanisms

Primary owners: `dashboard/palette.rs`, `dashboard/keymap.rs`, the hierarchy/workspace pickers and existing lifecycle completion handling in `server/mod.rs`.

- Build the Branch picker from registered workspace identities with observed branch labels. Include explicit root/detached states and reasons for unavailable rows. Search only this list; do not introduce a Git-branch catalog or implicit worktree adoption.
- Keep the complete parent draft while showing Create branch; store explicit edits separately from inherited defaults. On child success, use the returned workspace ID and restore the correct draft/focus.
- Reuse the existing standalone workspace creation job. Show pending state in the child; disable duplicate Create and avoid claiming cancellation after acceptance. Use actual lifecycle completion, not provisional branch text, to select the resource.
- Keep errors and inputs; distinguish failed creation from registered workspace plus failed launch. Retry session launch in place and reconcile ambiguous creation before retry.
- Follow stable checkout identity through branch-label drift, preserve root/existence/ownership guards, and show the shared-agent notice without blocking.
- Change `n` and palette/menu labels consistently through the single key table. Retain separately discoverable New terminal and standalone `w`; preserve input priority and view-handshake behavior.

Exit: state tests and compiled Dashboard/Server integration cover success, cancel, pending, drift, partial failure, invalid branch, branch duplicates, unavailable targets and all accepted default/edit rules. Check filters actually execute these cases.

## 4. Native acceptance and documentation

Reuse owning regression fixtures: `tests/tui`, `tests/workspace_identity.rs`, `tests/settings_lifecycle.rs`, `tests/retained_sessions.rs`, `tests/agent_setup.rs` and existing provider fixtures as appropriate. Read their actual constructors and ownership before extending them; create no parallel fixture framework.

- Drive the actual reviewed build in the disposable GUI via CUA. Preserve screenshots + AX for both dialogs, picker selection, focus, pending/error and post-create restoration.
- Exercise mouse and keyboard, multiline paste, picker Enter versus form submission, no input leakage, narrow 80×24/60-column layouts and the minimum-size state. Prove a separately printed output line in the new target session.
- Record actual installed-provider proof for each of the four required harnesses, separately from stand-ins and protocol tests. Decline optional integration setup outside the agreed fixture scope. Report unrun/blocked provider proof explicitly.
- Update affected TUI/runtime/testing guides, help/action names and settings examples in the same PR. Link the design, state which old mockup behaviors were superseded, and retain the work diary.
- Close only task-owned resources and verify launcher exit, fixture path/socket removal and owned process-group cleanup.

## 5. Verify once on the delivery revision and prepare a PR

Run focused owning tests during development. At acceptance, run the current repository's required regression, build, formatting and lint checks per `docs/development/testing.md`; repeat only for new changes or unresolved failures. Hosted check status must be current to the PR revision; Linux and native/provider checks remain separate evidence categories.

Review final diff against each acceptance row. Stage intended files only. Deliver a PR describing the concrete form behavior, four-harness capability evidence and material gaps. Merge requires separate user authorization. Do not call a capability-gated UI complete if any required Claude/Codex/Pi/omp model-and-prompt path remains unproved.

## Planning verification

No implementation, native model discovery, provider inference or Rust tests were run for this plan. Current code confirms stable workspace identity, registered-workspace launch selection, one noncancellable lifecycle job and per-project launch memory; it does not prove the proposed model/prompt feature. Documentation-only checks and mockup checks are recorded in the research review.


APPROVED DESIGN
# Branch-based agent session creation

Status: design confirmed by Kyle on 2026-11-05 (grilling Q18). Implementation and provider runs are not authorized by that confirmation.

## Problem and outcome

Creating an agent session currently hides behind Create terminal and a launch-type selector. Workspace creation already supports combined launch, but its form exposes routine Git details, accepts an incomplete branch prefix, and can lose the form on a Git error. Users usually identify their work by branch, not by a separate workspace name.

The new flow has one agent-session dialog and a separate Create branch dialog. Users choose the checkout by its current branch, then harness, model and an optional prompt. Creation details stay in the child dialog; the session draft survives the round trip.

This document is authoritative over earlier exploratory mockups. Evidence and individual answers are indexed in [the review](../../../research/workspace-session-ux-2026-11-05/review.md). Domain terms are defined in [CONTEXT.md](../../../CONTEXT.md).

## Session dialog

Four main controls, in order:

1. **Branch:** searchable registered workspaces, grouped by project, labeled with the current checkout branch. Exactly one Create new branch… entry opens the child dialog.
2. **Agent harness:** installed/configured launch choices, including recognizable named configurations and existing cloud-account choices.
3. **Model:** verified choices for that launch configuration, including explicitly labeled Harness default where supported. A configured/pinned model stays read-only unless a compatible override is proven.
4. **Prompt (optional):** multiline initial instruction. Blank opens an interactive agent. Start authorizes its execution, not merely prefilling a draft.

Other session controls, such as optional session name, are inside collapsed Advanced options. No separate Project field or workspace-name field appears here. A compact read-only destination line shows project, branch and exceptional state; an inline notice identifies other running agents sharing that checkout without adding confirmation.

Preselect the currently selected eligible feature workspace. Include registered root and detached checkouts with explicit labels. Retain the existing root-default-branch launch guard. If the current target is unavailable or absent, require a deliberate destination choice rather than selecting a different checkout silently. Empty registry offers the existing Add/register project route before a workspace can be created; an empty project can still create a branch. No raw unregistered local/remote branch catalog or implicit checkout adoption is included.

The selected target is a stable workspace ID and checkout, not a branch-name identity. If another terminal switches that checkout's branch during setup, update the label and follow the same checkout on first Start. Do not add a drift confirmation or switch to another checkout bearing the old name. Revalidate current ownership/existence and the root guard at launch; label drift does not waive those guards.

## Create branch dialog

Selecting Create new branch… hides the session dialog while preserving its complete draft and previous destination. Only one dialog is visible and receives input.

- **Project:** preselected from the current target; explicitly changeable.
- **Branch name:** required, initially empty with an example placeholder. Do not prefill an invalid value such as `feature/`.
- **Workspace options:** initially collapsed; Base defaults to the repository's detected default branch using its local state. Do not assume `main` or introduce an automatic fetch/update. Preserve existing project setup and optional automatic-local-terminal policy.
- **Create branch / Cancel:** concrete action labels. Cancel before submission restores the old destination and draft without creation.

Create commits the branch and registered workspace before returning to session setup. It does not launch the requested agent. Keep the child dialog visible while creation is pending; show status and reject duplicate submission. A submitted lifecycle job is not cancellable: pending controls must not claim otherwise, and Esc does not imply an abort. Preserve normal process/connection failure handling rather than trapping the application indefinitely.

On success, refresh the parent's registered-workspace picker, select the actual returned identity, restore the harness/model/prompt/name draft and focus the next unfinished field. Invalid branch/base and Git/setup failures keep useful inputs and errors in the child. Server validation remains authoritative; local validation prevents obvious mistakes before mutation. If an operation partly succeeds or its outcome is unknown, describe actual retained resources and reconcile that request before retrying. Do not automatically delete a checkout or create a duplicate.

Cancelling the parent after successful child creation leaves the branch/workspace available. Starting an agent is a separate committed action; its failure retries session launch in the same registered workspace, not branch creation.

## Harness, model and initial prompt

Full first-release model-selection and initial-prompt support is required for **Claude Code, Codex, Pi and Oh My Pi (`omp`)**. These are four distinct acceptance targets. Existing Claude/Codex/Pi delegation discovery helpers are reusable candidates, not proof of Dashboard or omp support.

Preserve the remaining detected/configured launchers. Capabilities must come from a verified launch configuration, not only a display name: a configured override can replace argv behind a known name. Unsupported controls explain the limitation. A typed prompt or explicit model is never silently dropped, translated to an unrelated option, or replaced; preserve it and block launch until corrected if the destination cannot support it. Existing cloud account/model/published-source restrictions remain in force; this is not a cloud-source or credential redesign.

Harness default means use the native launcher's default policy; it does not pin the last observed model. Show the actual resolved model when known, without inventing one from an old observation. A missing/invalid explicit model is an error, not permission to substitute another model.

Remember harness and model after successful launch per project. A project change applies destination defaults only where the draft has not expressed a conflicting explicit choice. Preserve explicit harness/model selections and prompt across the child dialog and project changes; dependent defaults cannot invalidate an explicitly chosen model silently. Prompts and one-off command text do not become remembered launch settings.

Initial prompt delivery uses a provider-supported launch/input mechanism that preserves native trust/approval boundaries. A Dashboard view handshake, visible PTY, elapsed delay or response-cycle Ready event is not generic proof of native startup readiness. Do not use sleeps or screen text matching as a substitute for a supported contract. Preserve existing configured launch/trust behavior; this feature does not expand it.

Keep pending initial-prompt text in memory for the current launch, with server-owned target/run identity so a Dashboard reconnect can retain it. Do not persist it into ordinary retained-session metadata, settings, Events or logs. A server restart does not automatically resend it. Report lost or uncertain delivery truthfully; do not claim externally guaranteed exactly-once execution. Never automatically retry an uncertain delivery. Clearing accepted pending text, retaining a failed draft and retrying a known pre-process failure must each follow their actual acceptance boundary.

## Input and layout

- Browse `n`: New agent session. Actions/Menu expose it by name. Preserve a separately discoverable New terminal action and existing shell/custom-command behavior.
- Browse `w`: standalone branch/workspace creation. Success selects that workspace and returns to Browse; invoked from session setup, success returns to that draft.
- Preserve the Terminal/Browse boundary: printable `n`/`w` never become global shortcuts inside a running agent. Existing `Ctrl-g` returns to Browse.
- Tab/Shift-Tab stay in the active dialog. Pickers accept choices without submitting the whole form. Enter in Prompt inserts a newline; provide the visible portable submit chord and clickable Start/Create actions. The final mockup uses Alt+Enter; actual terminal encoding must be verified.
- Esc closes an open picker first, then cancels an unsubmitted form. Pending creation cannot be cancelled; its status explains this. No underlying terminal input/clicks leak through either dialog.
- One modal border, uniform monospace hierarchy, cursor/reverse-video focus and semantic color. No color-only status, redundant badges or required Nerd Font glyphs.
- At 80×24 prioritize the active form and hide background preview details first. At 60 columns hide the sidebar and use full width. Expanded options get a scrollable form body while actions/status remain reachable. Proposed minimum is 60×18; below it give truthful resize guidance and preserve draft. These dimensions require real TUI acceptance.

```text
Wide: session                     60 columns: child only
tree | + New agent session ----+  + Create branch -------------------------+
     | | Branch [feature/auth v]|  | Project     [consigint                v]|
     | | Harness [Claude Code v]|  | Branch name [feature/auth-refresh       ]|
     | | Model [Harness default]|  | > Workspace options                    |
     | | Prompt [              ]|  |                                         |
     | | > Advanced options     |  | [Create branch] [Cancel]                |
     | | [Start agent] [Cancel] |  +-----------------------------------------+
     | +------------------------+  Return selects created workspace.
```

## Acceptance

| Area | Required evidence |
|---|---|
| Core flow | Real parent→child→success/cancel paths preserve draft, restore focus and select the returned workspace identity; one visible modal |
| Workspace safety | Registered-only filtering; duplicate branch validation; root guard; detached label; same-checkout branch change; missing/replaced checkout refusal |
| Lifecycle | Pending creation consumes duplicate Submit/Esc safely; errors retain inputs; partial success/unknown outcome avoids duplicate creation; agent retry reuses workspace |
| Model/defaults | Valid per-configuration choices; removed model refusal; named/pinned launcher protection; default-policy semantics; successful-only project memory; explicit edits survive project switches |
| Initial prompt | Blank and multiline prompt paths; native setup/trust interruption; delivery refusal/uncertainty; duplicate Start and reconnect; server restart does not replay; content absent from logs/settings/retained metadata |
| Required harnesses | Actual installed-launcher model and initial-prompt proof separately for Claude Code, Codex, Pi and omp; helpers/mocks alone cannot close these gates |
| Existing behavior | New terminal/custom command, standalone workspace setup, root ownership, retained sessions, live input during lifecycle work and current cloud restrictions preserved |
| Native UI | CUA from reviewed checkout, keyboard + mouse, 80×24/60-column/minimum layouts, screenshots with AX, actual session output and cleanup ownership evidence |

No provider run, paid inference, application patch, PR or merge was performed for this design. Implementation must report any unavailable acceptance as a gap, not as passed. Additional native/provider validation must use task-owned configuration and preserve the user's live sessions.


OBSERVED SOURCE EXCERPTS (tool results only; no private reasoning)
SOURCE LOOKUP {"path": "docs/superpowers/specs/2026-11-05-agent-session-creation-design.md"}
# Branch-based agent session creation

Status: design confirmed by Kyle on 2026-11-05 (grilling Q18). Implementation and provider runs are not authorized by that confirmation.

## Problem and outcome

Creating an agent session currently hides behind Create terminal and a launch-type selector. Workspace creation already supports combined launch, but its form exposes routine Git details, accepts an incomplete branch prefix, and can lose the form on a Git error. Users usually identify their work by branch, not by a separate workspace name.

The new flow has one agent-session dialog and a separate Create branch dialog. Users choose the checkout by its current branch, then harness, model and an optional prompt. Creation details stay in the child dialog; the session draft survives the round trip.

This document is authoritative over earlier exploratory mockups. Evidence and individual answers are indexed in [the review](../../../research/workspace-session-ux-2026-11-05/review.md). Domain terms are defined in [CONTEXT.md](../../../CONTEXT.md).

## Session dialog

Four main controls, in order:

1. **Branch:** searchable registered workspaces, grouped by project, labeled with the current checkout branch. Exactly one Create new branch… entry opens the child dialog.
2. **Agent harness:** installed/configured launch choices, including recognizable named configurations and existing cloud-account choices.
3. **Model:** verified choices for that launch configuration, including explicitly labeled Harness default where supported. A configured/pinned model stays read-only unless a compatible override is proven.
4. **Prompt (optional):** multiline initial instruction. Blank opens an interactive agent. Start authorizes its execution, not merely prefilling a draft.

Other session controls, such as optional session name, are inside collapsed Advanced options. No separate Project field or workspace-name field appears here. A compact read-only destination line shows project, branch and exceptional state; an inline notice identifies other running agents sharing that checkout without adding confirmation.

Preselect the currently selected eligible feature workspace. Include registered root and detached checkouts with explicit labels. Retain the existing root-default-branch launch guard. If the current target is unavailable or absent, require a deliberate destination choice rather than selecting a different checkout silently. Empty registry offers the existing Add/register project route before a workspace can be created; an empty project can still create a branch. No raw unregistered local/remote branch catalog or implicit checkout adoption is included.

The selected target is a stable workspace ID and checkout, not a branch-name identity. If another terminal switches that checkout's branch during setup, update the label and follow the same checkout on first Start. Do not add a drift confirmation or switch to another checkout bearing the old name. Revalidate current ownership/existence and the root guard at launch; label drift does not waive those guards.

## Create branch dialog

Selecting Create new branch… hides the session dialog while preserving its complete draft and previous destination. Only one dialog is visible and receives input.

- **Project:** preselected from the current target; explicitly changeable.
- **Branch name:** required, initially empty with an example placeholder. Do not prefill an invalid value such as `feature/`.
- **Workspace options:** initially collapsed; Base defaults to the repository's detected default branch using its local state. Do not assume `main` or introduce an automatic fetch/update. Preserve existing project setup and optional automatic-local-terminal policy.
- **Create branch / Cancel:** concrete action labels. Cancel before submission restores the old destination and draft without creation.

Create commits the branch and registered workspace before returning to session setup. It does not launch the requested agent. Keep the child dialog visible while creation is pending; show status and reject duplicate submission. A submitted lifecycle job is not cancellable: pending controls must not claim otherwise, and Esc does not imply an abort. Preserve normal process/connection failure handling rather than trapping the application indefinitely.

On success, refresh the parent's registered-workspace picker, select the actual returned identity, restore the harness/model/prompt/name draft and focus the next unfinished field. Invalid branch/base and Git/setup failures keep useful inputs and errors in the child. Server validation remains authoritative; local validation prevents obvious mistakes before mutation. If an operation partly succeeds or its outcome is unknown, describe actual retained resources and reconcile that request before retrying. Do not automatically delete a checkout or create a duplicate.

Cancelling the parent after successful child creation leaves the branch/workspace available. Starting an agent is a separate committed action; its failure retries session launch in the same registered workspace, not branch creation.

## Harness, model and initial prompt

Full first-release model-selection and initial-prompt support is required for **Claude Code, Codex, Pi and Oh My Pi (`omp`)**. These are four distinct acceptance targets. Existing Claude/Codex/Pi delegation discovery helpers are reusable candidates, not proof of Dashboard or omp support.

Preserve the remaining detected/configured launchers. Capabilities must come from a verified launch configuration, not only a display name: a configured override can replace argv behind a known name. Unsupported controls explain the limitation. A typed prompt or explicit model is never silently dropped, translated to an unrelated option, or replaced; preserve it and block launch until corrected if the destination cannot support it. Existing cloud account/model/published-source restrictions remain in force; this is not a cloud-source or credential redesign.

Harness default means use the native launcher's default policy; it does not pin the last observed model. Show the actual resolved model when known, without inventing one from an old observation. A missing/invalid explicit model is an error, not permission to substitute another model.

Remember harness and model after successful launch per project. A project change applies destination defaults only where the draft has not expressed a conflicting explicit choice. Preserve explicit harness/model selections and prompt across the child dialog and project changes; dependent defaults cannot invalidate an explicitly chosen model silently. Prompts and one-off command text do not become remembered launch settings.

Initial prompt delivery uses a provider-supported launch/input mechanism that preserves native trust/approval boundaries. A Dashboard view handshake, visible PTY, elapsed delay or response-cycle Ready event is not generic proof of native startup readiness. Do not use sleeps or screen text matching as a substitute for a supported contract. Preserve existing configured launch/trust behavior; this feature does not expand it.

Keep pending initial-prompt text in memory for the current launch, with server-owned target/run identity so a Dashboard reconnect can retain it. Do not persist it into ordinary retained-session metadata, settings, Events or logs. A server restart does not automatically resend it. Report lost or uncertain delivery truthfully; do not claim externally guaranteed exactly-once execution. Never automatically retry an uncertain delivery. Clearing accepted pending text, retaining a failed draft and retrying a known pre-process failure must each follow their actual acceptance boundary.

## Input and layout

- Browse `n`: New agent session. Actions/Menu expose it by name. Preserve a separately discoverable New terminal action and existing shell/custom-command behavior.
- Browse `w`: standalone branch/workspace creation. Success selects that workspace and returns to Browse; invoked from session setup, success returns to that draft.
- Preserve the Terminal/Browse boundary: printable `n`/`w` never become global shortcuts inside a running agent. Existing `Ctrl-g` returns to Browse.
- Tab/Shift-Tab stay in the active dialog. Pickers accept choices without submitting the whole form. Enter in Prompt inserts a newline; provide the visible portable submit chord and clickable Start/Create actions. The final mockup uses Alt+Enter; actual terminal encoding must be verified.
- Esc closes an open picker first, then cancels an unsubmitted form. Pending creation cannot be cancelled; its status explains this. No underlying terminal input/clicks leak through either dialog.
- One modal border, uniform monospace hierarchy, cursor/reverse-video focus and semantic color. No color-only status, redundant badges or required Nerd Font glyphs.
- At 80×24 prioritize the active form and hide background preview details first. At 60 columns hide the sidebar and use full width. Expanded options get a scrollable form body while actions/status remain reachable. Proposed minimum is 60×18; below it give truthful resize guidance and preserve draft. These dimensions require real TUI acceptance.

```text
Wide: session                     60 columns: child only
tree | + New agent session ----+  + Create branch -------------------------+
     | | Branch [feature/auth v]|  | Project     [consigint                v]|
     | | Harness [Claude Code v]|  | Branch name [feature/auth-refresh       ]|
     | | Model [Harness default]|  | > Workspace options                    |
     | | Prompt [              ]|  |                                         |
     | | > Advanced options     |  | [Create branch] [Cancel]                |
     | | [Start agent] [Cancel] |  +-----------------------------------------+
     | +------------------------+  Return selects created workspace.
```

## Acceptance

| Area | Required evidence |
|---|---|
| Core flow | Real parent→child→success/cancel paths preserve draft, restore focus and select the returned workspace identity; one visible modal |
| Workspace safety | Registered-only filtering; duplicate branch validation; root guard; detached label; same-checkout branch change; missing/replaced checkout refusal |
| Lifecycle | Pending creation consumes duplicate Submit/Esc safely; errors retain inputs; partial success/unknown outcome avoids duplicate creation; agent retry reuses workspace |
| Model/defaults | Valid per-configuration choices; removed model refusal; named/pinned launcher protection; default-policy semantics; successful-only project memory; explicit edits survive project switches |
| Initial prompt | Blank and multiline prompt paths; native setup/trust interruption; delivery refusal/uncertainty; duplicate Start and reconnect; server restart does not replay; content absent from logs/settings/retained metadata |
| Required harnesses | Actual installed-launcher model and initial-prompt proof separately for Claude Code, Codex, Pi and omp; helpers/mocks alone cannot close these gates |
| Existing behavior | New terminal/custom command, standalone workspace setup, root ownership, retained sessions, live input during lifecycle work and current cloud restrictions preserved |
| Native UI | CUA from reviewed checkout, keyboard + mouse, 80×24/60-column/minimum layouts, screenshots with AX, actual session output and cleanup ownership evidence |

No provider run, paid inference, application patch, PR or merge was performed for this design. Implementation must report any unavailable acceptance as a gap, not as passed. Additional native/provider validation must use task-owned configuration and preserve the user's live sessions.

SOURCE LOOKUP {"path": "docs/superpowers/plans/2026-11-05-agent-session-creation.md"}
# Implement branch-based agent session creation

Status: implementation plan, not executed. Design confirmed by Kyle in Q18 on 2026-11-05. Source baseline inspected: `08d25641d3ef96a2b8f9f74a5710efb2c532b75a`. Recheck current branch/source and create an isolated feature worktree before implementation.

The [confirmed design](../specs/2026-11-05-agent-session-creation-design.md) owns behavior and acceptance. The [research review](../../../research/workspace-session-ux-2026-11-05/review.md) preserves native CUA evidence and all submitted decisions. This plan changes neither server ownership nor workspace identity.

## 1. Prove the four native launch contracts before promising controls

Read `dashboard/agents.rs`, `dashboard/palette.rs::detected_agents`, `src/report/` and the owning provider integration paths. Inspect installed versions and authoritative provider documentation before relying on exact launch/model/prompt APIs. Do not equate Pi and omp.

- Produce a bounded capability matrix for Claude Code, Codex, Pi and omp: model discovery, Harness default semantics, explicit-model application, initial-prompt delivery, native setup/trust and delivery evidence.
- Reuse existing Claude/Codex/Pi model-discovery work in runtime `server/delegation.rs`, `claude_delegation.rs` and `codex_delegation.rs` where its contract fits. It is currently session-authenticated delegation functionality; do not expose that authority or forge a delegation session to serve a Dashboard.
- Preserve arbitrary configured argv and named aliases. A familiar display name does not prove safe model override. Preserve existing cloud account pins and source constraints.
- Begin with installed-artifact/offline inspection and task-owned fixtures. Native or paid validation scope must be explicit before running it; no model calls are part of this planning task.

Exit: a supported mechanism is identified for each required harness, with gaps clearly stated. An unsupported mechanism is a blocker for that harness's required acceptance, not a reason to weaken the requirement or silently hide omp.

## 2. Extend the existing launch path and settings, with tests through real entry points

Affected owners include protocol `wire.rs` (`CreateSessionRequest`, `SessionLaunch`), TUI `AgentEntry`/selection, runtime spawn/managed invocation, and typed settings catalog/loader. Current launch values are argv/label/kind; model and prompt are not structured fields today.

- Add only the structured options needed by the proven provider contracts and validate at the Server trust boundary. Update every constructor, exhaustive match, CLI/caller/fixture and wire compatibility requirement together.
- Route bounded discovery outside the input reader and lifecycle mutation locks; use existing request correlation/ownership patterns. Do not add a second model/provider authority, framework, SDK or settings parser to solve the form.
- Put ephemeral initial-prompt ownership with the exact Server session/run. Preserve pending text across Dashboard reconnects; never serialize text into ordinary retained metadata, settings or Events. Define known refusal versus unknown delivery, with no automatic uncertain replay.
- Extend successful per-project launch memory with model-policy selection through the existing settings owner. Keep Terminal memory and one-off command exclusion intact.

Exit: real request→validation→provider launch paths demonstrate explicit/default model behavior and prompt blank/delivery/refusal/reconnect/restart semantics. Cover invalid/stale targets, capability changes, configuration overrides and missing models without silent fallback.

## 3. Implement the two dialogs using current Dashboard and lifecycle mechanisms

Primary owners: `dashboard/palette.rs`, `dashboard/keymap.rs`, the hierarchy/workspace pickers and existing lifecycle completion handling in `server/mod.rs`.

- Build the Branch picker from registered workspace identities with observed branch labels. Include explicit root/detached states and reasons for unavailable rows. Search only this list; do not introduce a Git-branch catalog or implicit worktree adoption.
- Keep the complete parent draft while showing Create branch; store explicit edits separately from inherited defaults. On child success, use the returned workspace ID and restore the correct draft/focus.
- Reuse the existing standalone workspace creation job. Show pending state in the child; disable duplicate Create and avoid claiming cancellation after acceptance. Use actual lifecycle completion, not provisional branch text, to select the resource.
- Keep errors and inputs; distinguish failed creation from registered workspace plus failed launch. Retry session launch in place and reconcile ambiguous creation before retry.
- Follow stable checkout identity through branch-label drift, preserve root/existence/ownership guards, and show the shared-agent notice without blocking.
- Change `n` and palette/menu labels consistently through the single key table. Retain separately discoverable New terminal and standalone `w`; preserve input priority and view-handshake behavior.

Exit: state tests and compiled Dashboard/Server integration cover success, cancel, pending, drift, partial failure, invalid branch, branch duplicates, unavailable targets and all accepted default/edit rules. Check filters actually execute these cases.

## 4. Native acceptance and documentation

Reuse owning regression fixtures: `tests/tui`, `tests/workspace_identity.rs`, `tests/settings_lifecycle.rs`, `tests/retained_sessions.rs`, `tests/agent_setup.rs` and existing provider fixtures as appropriate. Read their actual constructors and ownership before extending them; create no parallel fixture framework.

- Drive the actual reviewed build in the disposable GUI via CUA. Preserve screenshots + AX for both dialogs, picker selection, focus, pending/error and post-create restoration.
- Exercise mouse and keyboard, multiline paste, picker Enter versus form submission, no input leakage, narrow 80×24/60-column layouts and the minimum-size state. Prove a separately printed output line in the new target session.
- Record actual installed-provider proof for each of the four required harnesses, separately from stand-ins and protocol tests. Decline optional integration setup outside the agreed fixture scope. Report unrun/blocked provider proof explicitly.
- Update affected TUI/runtime/testing guides, help/action names and settings examples in the same PR. Link the design, state which old mockup behaviors were superseded, and retain the work diary.
- Close only task-owned resources and verify launcher exit, fixture path/socket removal and owned process-group cleanup.

## 5. Verify once on the delivery revision and prepare a PR

Run focused owning tests during development. At acceptance, run the current repository's required regression, build, formatting and lint checks per `docs/development/testing.md`; repeat only for new changes or unresolved failures. Hosted check status must be current to the PR revision; Linux and native/provider checks remain separate evidence categories.

Review final diff against each acceptance row. Stage intended files only. Deliver a PR describing the concrete form behavior, four-harness capability evidence and material gaps. Merge requires separate user authorization. Do not call a capability-gated UI complete if any required Claude/Codex/Pi/omp model-and-prompt path remains unproved.

## Planning verification

No implementation, native model discovery, provider inference or Rust tests were run for this plan. Current code confirms stable workspace identity, registered-workspace launch selection, one noncancellable lifecycle job and per-project launch memory; it does not prove the proposed model/prompt feature. Documentation-only checks and mockup checks are recorded in the research review.

SOURCE LOOKUP {"path": "CONTEXT.md"}
# OVRCR

Synchronous terminal multiplexer. One server owner, one active dashboard, fifty sessions.

## Language

**Retained session**:
A session whose identity and workspace placement remain available after its process ends or the server restarts. Agent sessions retain a reference to their provider-owned conversation; both Agent and Terminal launches have retained sessions.
_Avoid_: live process, running terminal as interchangeable terms

**Archived session**:
A retained session the user has closed and removed from the active list without deleting its record or the agent's own conversation history. Process exit and an agent's suggestion to close do not themselves archive a session.
_Avoid_: deleted session, exited process, closed pane

**Conversation resume**:
An explicit user action to continue an agent's previous conversation where the provider supports it.
_Avoid_: restoring history, reattaching to a live process, automatic execution

**Conversation reference**:
The provider identity and unique conversation identifier needed to reopen an agent's saved conversation.
_Avoid_: transcript, terminal output, most recent conversation

**Session title**:
The name shown for a session. Applications cannot change it, and changing it does not change which session it identifies. A Terminal shows a manual title or the original session name. An Agent launch may show a conversation subject until the user renames the session.
_Avoid_: session identity, agent label, workspace name, application title

**Original session name**:
The name chosen at creation, supplied by the user or generated from the workspace. Clearing a manual title restores this name, even if the workspace's branch has since changed. On an Agent launch, a name typed at creation is this name and is not a manual title.
_Avoid_: current branch name, application title, conversation subject

**Manual title**:
A session title the user sets with Rename, or a name typed when creating a Terminal. It stays until the user edits it or clears it, and it survives reconnect, server restart, and reopen. Clearing it restores the original session name and dismisses the subject of the conversation attached at that moment.
_Avoid_: Automatic mode, conversation subject, pinned session (which could imply a placement or lifecycle change)

**Conversation subject**:
A short topic for one agent conversation. It may be shown as the session title until the user renames the session or dismisses that subject. It is not the original session name.
_Avoid_: automatic title, application title, manual title, session identity

**Launch choice**:
The user's choice to start an Agent or a Terminal in a workspace. Creating a workspace can also leave it empty with Nothing yet.
_Avoid_: custom command as a third session category

**Agent**:
A detected or configured agent selected for launch. This launch choice alone does not establish reporting support or determine everything that may later run in the session.
_Avoid_: any arbitrary command as an agent

**Initial prompt**:
The optional instruction the user supplies when creating an Agent session and authorizes it to run by starting that session. An empty initial prompt leaves the agent open for interactive use.
_Avoid_: session name, reusable prompt template, conversation resume, setup or trust answer

**Harness default**:
A model choice that uses the selected agent launcher's own default instead of a model explicitly selected in OVRCR. It names a policy, not a promise that the resolved model stays the same.
_Avoid_: a pinned model, the last observed model, a silently substituted model

**Cursor cloud account**:
A nonsecret named selection policy containing one explicit Keychain credential reference, model and allowed repositories or owners. Selecting it does not attest a remote billing identity. Local `cursor-agent` is a separate provider.

**Cursor SDK Bridge**:
The pinned official Cursor process owned by one cloud attachment inside the Server-managed process tree. It is separate from the macOS OVRCR Bridge app.

**Cloud reconciliation**:
A read of the original retained cloud conversation/run through a freshly validated
credential-bound bridge. A historical terminal outcome creates no new Ready or
Unread. The original live attachment may recover a transport interruption on its
still-owned bridge using separate durable Observe progress; this does not authorize
fresh-handle restoration. Active reattachment and uncertain CreateAgent/Send replay remain gated;
explicit Retry Cancel is a separate durable exact-run action.

**Published source**:
The selected workspace's canonical origin and exact HEAD commit, verified as published at that origin. Local edits are excluded; launching does not upload, commit or push them.

**Terminal**:
The launch choice for opening a shell, optionally with a custom command. It creates a terminal session with a stable session title.
_Avoid_: custom command as a separate session type

**Remembered launch choice**:
A project's most recently successfully launched Agent and preset, or Terminal choice. It excludes one-off command text and is unchanged by creating an empty workspace.
_Avoid_: running agent, silently substituted fallback

**Workspace**:
A project's registered Git checkout. Its user-facing name is the current branch of that checkout; changing the branch does not replace the workspace.
_Avoid_: workspace title, an arbitrary name besides the current branch, treating a branch change as a different workspace

**Root workspace**:
The workspace that is the repository checkout itself. Registration accepts the current checkout branch; new launches into the root are blocked while it is off the repository's default branch. It cannot be removed, and OVRCR never changes Git to restore that branch.
_Avoid_: main, root as a stored name, a removable default worktree, requiring checkout of the default branch before register

**Dashboard**:
The one active TUI for live sessions: the hierarchy, the panes, and browse / terminal / history / copy.
_Avoid_: TUI (the crate), GUI helper, task UI as a separate product

**Server**:
The one process that owns the socket, the sessions, and the live terminals.
_Avoid_: backend API server, a second server owned by the Bridge

**Bridge**:
The one macOS app that holds the bundle identity and performs system calls the terminal cannot make under its own name. It is not a Dashboard, not a Reporter, and not an owner of sessions or panes. It may ask iTerm to select an existing session. It does not create iTerm sessions.
_Avoid_: helper app, companion app, GUI helper, Notifier as a separate product, an owner of iTerm tabs, a second Server

**Key binding**:
The dashboard's one entry for a key in an input mode: its label, its action, and the reason it is unavailable. Dispatch, the footer, the key popup and the palette read that one entry, so a hint cannot disagree with what the key does.
_Avoid_: the reporting binding, a hint list per consumer

**Keystroke**:
A key delivered to the program inside a named session, as that program would receive it from a keyboard.
_Avoid_: Paste, Input request, Key binding

**Paste**:
Text delivered into a session as a paste, which may be followed by Enter.
_Avoid_: Keystroke

**Ready**:
The accepted root observation of a completed response cycle from a supported readiness provider, identified by binding, cycle identity (the `turn` field), and activity revision. It does not establish task success.
_Avoid_: Confirmed activity as a success claim, a desktop alert

**Unread**:
The current Ready observation that has not been marked reviewed.
_Avoid_: treating view, selection, or notification as review

**Presented**:
The Unread the Dashboard last showed.
_Avoid_: a newer Unread as the review target

**Input request**:
An open request for a human answer, identified by binding, namespace and request identity. A binding carries a bounded set of them, published whole; while any is open the session's effective activity is WaitingInput, and closing the last one restores the underlying activity.
_Avoid_: Unread, a Ready observation, a tool call, or an alert as the request

**Alert**:
An OVRCR notice that a response is ready to review or that an agent needs a human answer. Receiving or clicking it does not review the response or answer the request.
_Avoid_: Ready, Unread or Input request as interchangeable terms

**Reporter**:
The in-process owner of one managed invocation's reporting: the lease, the binding and Reporting generation that lease carries, the one revision set every observation is numbered from, the retained identities it fences, and the teardown. One per invocation, shared by every provider; the provider's receiver translates that provider's frames into observations and chooses which of these the provider needs.
_Avoid_: the provider's receiver, the extension Producer, the helper process

**Producer**:
The admitted extension instance whose source sequence fences its events; retired by an observed shutdown or replaced by an admitted successor. In Oh My Pi one producer spans every in-place session switch, so a switch is a new binding, not a new producer.
_Avoid_: the conversation, the binding, the native process

**Reporting generation**:
The `generation` of a binding, bumped by every accepted Bind; a fresh one is admitted by the supervisor while the lease and session identity hold, and it starts with Unknown activity and no requests.
_Avoid_: the reservation epoch, a restart

**Active dashboard**:
The server-side seat the one Dashboard connection holds: its identity, outbound queue, acknowledged view, and focused geometry.
_Avoid_: the dashboard slot, the sink, the view subscription as separate things

**View handshake**:
The Dashboard's exchange with the server that acknowledges a view: one `SetView` in flight, snapshots matched by request, revision, and session, readiness granted by a complete final `Ok`.
_Avoid_: pane ready as a flag anyone sets

**Session status**:
What one session says about itself on screen, decided once per draw: the row's glyph and the process, paused, effective activity, Unread and elapsed clauses. An Unread clause replaces a header line outright; each surface arranges and clips the rest for its own width. The sidebar row, both pane headers, and the spinner redraw cadence read the same value.
_Avoid_: the raw rollup activity, the glyph on its own, a per-renderer reading

**Retarget**:
Any change to what a pane shows: split, focus, close, select, container, cleared, resize, or server removal. Each names which releases apply.
_Avoid_: calling the release steps individually

**Outbox**:
The Dashboard's one queue of fully formed steady-state requests to send, preserving
their enqueue order. Fresh PTY input and the startup handshake retain their own authority.
_Avoid_: private slots that hold a request

**Setting**:
One user-chosen key with a typed value, a default, and one owner (Server or Dashboard) that acts on it.
_Avoid_: preference, configuration, option as synonyms

**Settings document**:
The one TOML file holding every setting, `dashboard.toml` in the instance directory.
_Avoid_: config, dashboard config, a per-process settings file

**Instance identity**:
The instance directory (`OVRCR_HOME`, or the platform config directory by default) that holds `registry.sqlite3`, `dashboard.toml`, `tasks/`, `events.jsonl` and `server.sock`. It is not a settings file. `OVRCR_CONFIG` remains a deprecated file-path alias for one release.
_Avoid_: registry file, config file as "where settings go"

**Effective value**:
The value a process acts on after the default and the settings document are applied.
_Avoid_: the raw TOML value, the default on its own

**Source**:
Where an effective value came from: the default or the settings document.
_Avoid_: an environment layer, a per-process override

**Finding**:
One load problem on one setting or on the settings document (unknown key, wrong type, unknown spelling, unparseable file), reported wherever settings are shown and never silently absorbed.
_Avoid_: a warning that is logged and dropped, a parse error that resets unrelated settings

**Consent setting**:
A setting that stays off until the user turns it on because turning it on spends money, touches credentials, exposes data, or needs an OS permission. Its off state says what to set.
_Avoid_: feature flag, default-off as a synonym, "Unavailable" for a setting that is simply off

**Settings snapshot**:
The Server's one published reading of the settings document: every effective value with its source, the findings, the document path and the read time. Sent to the Dashboard at hello and again whenever the reading changes.
_Avoid_: the Dashboard's own parse of the document, a per-setting notification

**Setting path**:
The dotted address of one setting, one element of a list setting, or one entry of a table setting, such as `quota.codex.command`, `picker_roots[2]` or `launch_choices.myproj.kind`. The editor, the CLI and the Server's write request all name a value this way.
_Avoid_: a TOML key on its own, a row index in the popup

**Settings catalog**:
The one declaration of every setting path: its shape, its owner, and what the off state says. The shape is what a write accepts and what the Dashboard editor presents, including the pattern of a list or table of entries. It does not hold the effective value.
_Avoid_: a second settings document, the settings snapshot, a separate path list for the editor, the writer, and the rows, a value type and an editor kind that can disagree

**Allowance**:
What a provider's subscription still permits in a window, shown in the Quota left block as the remainder of a percentage. It is never a session's token count, context occupancy or API spend.
_Avoid_: usage, tokens, cost, quota as a synonym for spend

**Quota reason**:
OVRCR's own one-line classification of why a provider's allowance is not current, such as the executable not being found or a read timing out. It never carries a response body, an account value or a credential.
_Avoid_: the raw error, a provider message copied through, a log line

**Next check**:
The time the Server will next ask a native provider for its allowance, after the regular cadence, a backoff or a provider's Retry-After. Shown so a failed row says when it will try again.
_Avoid_: the stale boundary, the last check, a countdown that resets on redraw

**Quota probe**:
A short, hidden run of the unmodified Claude Code binary that the Server starts, with the user's consent, only to receive one status-line report of the account's allowance when no managed Claude session has reported recently. It sends one minimal prompt, spends a little allowance, is never a Session, and never touches a credential.
_Avoid_: a managed session, a background agent, an OAuth read, polling

**Event**:
One dated, OVRCR-written line about a decision the Server made or a state it changed: which component (titles, settings, quota, provisioning), the session or provider concerned, and a one-line message under the same rule as a Quota reason. Never a prompt, a transcript, a credential, an account identifier or a native body.
_Avoid_: a log line copied from stderr, a Ready or Unread, an alert, a wire message

**Lifecycle job**:
One in-flight Server-owned create/remove/close of a workspace or session. At most one runs at a time.
_Avoid_: background task, async request, pending palette request as synonyms

**Provisional row**:
A Dashboard hierarchy row shown before the Server has accepted the real identity; fiction until success, failure, or dismiss of a failed state.
_Avoid_: optimistic session, phantom session, client-side SessionId

SOURCE LOOKUP {"path": "*"}
CONTEXT.md
# research/
## workspace-session-ux-2026-11-05/
review.md
grilling-answers.txt
cleanup.txt
11-provider-handoff.jpg
11-provider-handoff.txt
10-register-project.jpg
10-register-project.txt
session-process-groups.json
processes-before-cleanup.txt
09-workspace-agent.jpg
09-workspace-agent.txt
08-session-input.jpg
08-session-input.txt
07-existing-workspace-agent.jpg
07-existing-workspace-agent.txt
06-workspace-created-agent.jpg
06-workspace-created-agent.txt
05-workspace-terminal.jpg
05-workspace-terminal.txt
04-invalid-default.jpg
04-invalid-default.txt
03-actions.jpg
03-actions.txt
02-menu.jpg
02-menu.txt
01-dashboard.jpg
01-dashboard.txt
### plan-validation-omp/
ux-lifecycle-1793903280658756000-events.jsonl
acceptance-1793903280576049000-events.jsonl
feasibility-1793903281505240000-events.jsonl
feasibility-1793903281505240000-stderr.log
ux-lifecycle-1793903280658756000-stderr.log
acceptance-1793903280576049000-stderr.log
acceptance-1793903182650603000-events.jsonl
feasibility-1793903182005336000-events.jsonl
acceptance-1793903182650603000-stderr.log
feasibility-1793903182005336000-stderr.log
run-review.py
reviewer-system.md
acceptance-prompt.md
ux-lifecycle-prompt.md
feasibility-prompt.md
#### __pycache__/
run-review.cpython-313.pyc
## operator-ux-2026-11-05/
team-focused-conversation.png
team-attention-inbox.png
team-review.md
team-refined-journal.png
team-preview-frames.txt
preview-library.png
preview-notes.md
preview-frames.txt
review.md
cleanup.json
06-narrow-chat.jpg
06-narrow-chat.ax.txt
processes.json
05-larger-text.jpg
05-larger-text.ax.txt
04-close-confirmation.jpg
04-close-confirmation.ax.txt
03-quick-message.jpg
03-quick-message.ax.txt
## operator-full-width-2026-11-03/
wide.txt
wide.png
README.md
## rig-operator-2026-10-29/
offline-provider-finished.json
offline-independent-stop.json
offline-concurrent-running.json
offline-concurrency-2-proof.json
offline-concurrency-2-cleanup.json
live-attempt-2-proof.json
live-attempt-2-cleanup.json
attempt-2-bundle.json
README.md
09-background-complete.jpg
# docs/
pi-research-assignments.md
## superpowers/
### plans/
2026-11-05-agent-session-creation.md
2026-11-02-superset-orchestration.md
### specs/
2026-11-05-agent-session-creation-design.md
## development/
tui.md
# tests/
pi_question_extension.mjs
pi_delegation_extension.mjs
terminal_acceptance.rs
task_tui.rs
service.rs
server_lifecycle.rs
provider_quota.rs
pi_reporting_extension.mjs
pi_delegation.rs
operator_worker.rs
operator_scope.rs
operator_e2e.rs
operator_auth.rs
gui.rs
cursor_cloud.rs
cursor_bridge_install.py
client_startup_server.rs
cli.rs
agent_setup.rs
## tui/
unread.rs
split.rs
sidebar.rs
settings.rs
quota.rs
palette.rs
operator.rs
mouse.rs
main.rs
events.rs
copy_history.rs
bridge_navigation.rs
agent_search.rs
agent_attention.rs
## terminal_acceptance/
assignment_control.rs
## support/
operator_provider.rs
## pi_delegation/
review.rs
control.rs
cancellation.rs
## fixtures/
### pi/
pi_host.mjs
### operator-auth/
rsa-test-only.pem
jwks.json
### cursor-cloud/
bridge.py
# src/
ovrcr-pi-runtime.ts
report.rs
lib.rs
## report/
reporter.rs
cursor_cloud.rs
admission.rs
### reporter/
tests.rs
scripted.rs
## operator/
worker.rs
mod.rs
login.rs
bash.rs
auth.rs
### worker/
chatgpt_http.rs
## gui/
input.rs
## cli/
resources.rs
output.rs
operator.rs
mod.rs
cursor_cloud.rs
args.rs
agent.rs
# crates/ovrcr-tui/src/dashboard/
operator.rs
## operator/
render.rs
# research_notes/Superset orchestration for OVRCR/
runtime.md
skills.md
ovrcr.md
# reports/
Superset orchestration for OVRCR.md
# plans/
# vendor/cursor-sdk-bridge/1.0.40/
README.md
## proto/sdk/v1/
sdk_store_callback_service.proto
sdk_messages.proto
sdk_errors.proto
sdk_custom_tool_callback_service.proto
sdk_cursor_service.proto
sdk_bridge_control_service.proto
sdk_agent_service.proto
# scripts/
install-cursor-bridge.py

[200 results limit reached]

SOURCE LOOKUP {"path": "docs/development/tui.md"}
# Dashboard behavior

Notification offers require a matching pending Server confirmation and current
lifetime/run hierarchy. Application uses `retarget` even for the same session,
lands Browse, cancels unsent overlays/captures and removes queued Input from the
outbox while leaving submitted operation targets intact. The actual event loop
observes the navigation input epoch, stops later Server dispatch and discards
old terminal events through bounded continuation batches until the input queue
is empty. It then emits the ordinary `ViewHandshake` request; new input still
requires all current snapshots and the final matching acknowledgment. A clicked
retained run is excluded from automatic recovery.

Pending shutdown WIP-save questions retain their palette and answer requests.
Notification offers and their matching confirmations are ignored while those
questions are queued; a click must never infer a save choice or hide a question
that blocks shutdown.

- Revoke input permission immediately when focus, assignment, or visible geometry changes. Input requires a Running session in the current acknowledged visible view, using that pane's terminal modes.
- Match snapshots by request, revision, session and run. Require every expected snapshot plus the matching final acknowledgement before enabling input. An empty view expects zero snapshots and must still complete.
- Keep one view request in flight and coalesce desired changes. A true no-op must preserve readiness. Stale responses must not populate reassigned panes, clear current errors, or enable input.
- Use the same geometry for subscriptions, rendering, hit-testing, and cursor placement. Hidden panes retain their assignments and installed screens but cannot receive input. Never send zero PTY dimensions.
- Keep History/Copy tied to the captured session and run. Changing either cancels pending capture requests, selection jobs, and queued clipboard completions. Removing an unrelated pane must preserve a valid focused capture. A new run clears the parser, input readiness and Unread but preserves pane assignment and layout.
- Current-screen Copy cancels on resize through the actual shared client path. Frozen History retains its cells and wrapping while its viewport changes; live output must not rewrite the frozen capture.
- Preserve overlay input priority, bounded event batches, and the input-drain-before-clipboard-emission boundary.
- Treat literal spacing, terminal modes, Unicode width, colors, cursor placement, and accessibility text as behavior. Keep natural wide-glyph widths and clip at pane boundaries. A TestBackend pass alone may miss errors in emitted terminal coordinates.

Readiness is derived by `ViewHandshake::is_ready`; pane changes go through `retarget(PaneChange)`; outbound requests go through the outbox and are drained by `drain_outbox`. Session status — the glyph and the process, paused, activity, Unread and elapsed clauses, with an Unread replacing a header line outright — is decided once by `dashboard::status::SessionStatus`, which the sidebar row, both pane headers and the spinner redraw cadence read; each surface still arranges and clips the clauses for its own width, and readiness itself is still decided in `dashboard::ready`. The Ready checkmark is drawn only while that response is Unread; after review the glyph is blank and the activity clause may still say ready. The sidebar agent label appends the current model…
Key bindings live in one table, `dashboard::keymap`: it holds each key's label, action and the reason it is unavailable, and `key_action`, the footer, the key popup and the palette all read it rather than repeating a guard.

The Outbox owns every fully formed steady-state request. Producers enqueue once;
Dashboard actions carry only local effects or fresh PTY bytes. `drain_outbox`
schedules due History work after requests already queued. Producers that formerly
carried a request in an action schedule due History before enqueueing that request,
preserving cleanup → History page → request order. Deferred palette submission
keeps its existing request-before-History order through the ordinary queued path.
History key/cursor/copy paths
also keep their active scheduling points and pending-page suppression. Each Server
message drains its follow-ups before the next message, except ignored responses;
navigation keeps its bounded input-discard break. Fresh PTY bytes are authorized
only after queued requests flush; captured cleanup keeps its original target and
authority. Clipboard effects retain the input boundary, and startup keeps reserved
request IDs 1/2 and its interleaved handshake outside this steady-state ownership.

`Ctrl-o` is reserved globally by `keymap::is_operator_quick_key`. Key dispatch,
event dispatch before Tasks, and the production mouse path give the Hub-owned
quick form priority. The form layers above existing views without taking their
state; Repeat/Release and all modal mouse events are consumed. Its frozen
generation/context/defaults follow a request-owned Create → Submit continuation,
using the acknowledged conversation's retained `summary.run`. Esc hides submitted
work, and only a still-visible matching generation may navigate on acceptance.
The quick dialog starts at seven rows (two editor rows), grows with wrapped input
and an optional error row to sixteen, and keeps its eighty-column cap. Errors and
pending phase hints remain visible; shrinking the form cannot forward modal clicks.
Failures retain the message; retries after Create target that chat. Notification
retarget cancels an unsent form or hides its pending continuation. Quick visibility
also blocks durable-tail review acknowledgement.

Full Operator message input and editable Quick Message call
`TextCursor::edit_key(KeyEvent)` for line/grapheme/word editing; other fields keep
`TextCursor::key(KeyCode)` unchanged. Ctrl+A/E and Ctrl+U/K use logical line edges,
Ctrl+W and Alt+B/F use whitespace-delimited words without crossing newlines.
Home/End remain whole-buffer. Modifiers match exactly; unsupported combinations
are consumed without inserting letters. Full input repeats ordinary edits, while
Quick retains Press-only editing. Each owner handles Ctrl+C itself: a Press clears
literal nonempty text and resets the cursor; a separate empty full Press calls the
existing exact selected Stop. Repeat/Release do neither, empty Quick is a no-op,
and pending Quick/closing chats stay frozen. Quick edits must reach the existing
seeded-retry synchronization without changing unseeded chat drafts or captured
Submit/Create payloads. The native GUI's `encode_event` suppresses known repeated
Ctrl+C before raw byte 3 loses repeat identity; first Press still sends byte 3.
Control-only bytes and ordinary repeat stay unchanged. Alphabetic Control keys
with Alt or Shift retain those bits using the existing CSI-u spelling (lowercase
ASCII codepoint, modifier parameter 1 + Shift 1 + Alt 2 + Control 4), so unsupported
composer chords cannot become destructive Control-only input. The shared terminal
encoder remains unchanged. Legacy external terminals without repeat metadata can
report every held byte 3 as a fresh Press, so they cannot distinguish a hold from separate
presses. No timing heuristic or keyboard-protocol redesign is applied.

Retained rows stay selectable without allocating a parser on the server or
inventing live timing. Reopen targets the existing row and expected run; Retry
never implies ownership acknowledgement. Same-boot or unverifiable recovery
requires a separate explicit confirmation that old agents and background
processes have stopped. Agent rows without a conversation reference open the supported provider's native
resume picker. Unknown providers and invalidated references remain unavailable.


Operator conversations occupy one-line rows above Projects in the existing sidebar.
`SidebarRects` supplies Operator, project-tree and quota rectangles to rendering,
mouse input and viewport calculations. The quota ladder reserves the Operator
heading and three project-tree lines; up to five conversation rows use remaining
space and shrink first. Collapsing keeps the heading and active-conversation count.
Rows share the session activity glyphs, selected background/bar and hover close mark,
and omit provider/model metadata. Archived chats remain cached as authority tombstones
but never appear in the active list. Saved summaries load asynchronously after the
production handshake and initial terminal attachment, without opening Operator.

Browse `o` and the clickable title-bar control open the native hub. Terminal pane
assignments survive, but the desired terminal view is empty while the hub is visible
and native composer input never produces PTY bytes. Existing overlays retain input
priority; spaces and punctuation in an active composer remain ordinary text.

`dashboard::operator` owns per-conversation drafts, context, grapheme cursor,
scroll/follow-tail and completion Unread state. Selecting a saved conversation never
recaptures the last terminal's context. Scrolled transcripts keep a durable message
ordinal, logical part and byte anchor
across streaming, receipt expansion, conversation switches and resize. Styled
graphemes survive wrapping; the Working journal reserves a two-cell gutter before
wrapping and decorates only after source-byte anchor calculation. User prompts
have a mauve cue; assistant/tool activity shares a muted guide. Prose and
command/output details use the available transcript width after the gutter.
Adjacent tool calls and outputs stay separate disclosures under their original
ordinals, without blank rows or collapsed raw IDs; expanded details retain IDs. Successful completion
Unread comes from the exact durable turn summary; SDK model history stays private.
`Tab` cycles sidebar, transcript and composer; `Ctrl-l` toggles sidebar/composer.
Transcript Up/Down selects a user-context/tool entry; Enter expands its full details.
The borderless composer uses `clamp(wrapped_draft_rows, 2, min(6, body_height / 3).max(2))`
text rows and scrolls around the real cursor. One-cell edge padding, a mauve top
rule and muted bottom rule define the editor. `OperatorLayout` owns header,
transcript, context, writable composer and both rule rectangles for rendering,
hit-testing and review. The target uses the context row above the top rule.
A single transient journal row follows the latest durable message or receipt,
aligned after the same two-cell gutter: Sending before submission acceptance,
Responding throughout Running (including silence), or Waiting for a recorded
current-turn tool result. Tool waits require caught-up history and match
call IDs within the current turn; paging/dirty history falls back to Responding.
Stopping/Closing remain explicit and terminal work without stop proof says static
Stop unconfirmed. Confirmed terminal work removes the indicator. The private
derived activity decision also requests the shared 100 ms spinner cadence while
the journal is visible, independent of sidebar visibility. Rendering uses the
shared draw timestamp. Canonical visual rows carry optional durable anchors; the
activity row has none and replaces only the final durable blank separator. It
occupies one clipped row before viewport slicing, so older history scrolls it away
and returning to the tail reveals it. Focus, disclosure and durable scroll anchors
ignore unanchored rows. The context/error row keeps its full width, and review
requires three durable viewport rows in addition to any activity row.
The local header shows Operator/provider/model/state only when the selected chat's
row is rendered in the shared sidebar. Collapsed, offscreen or absent rows retain
the clipped chat title, shortening model metadata first. Only visible Operator
quiets the global title fill. The keymap-derived footer prioritizes eligible Send,
Newline, Browse and applicable Stop. Send is disabled in the shared keymap/menu
while the draft is empty or the selected chat cannot submit (including running,
pending and unconfirmed-worker-stop states). Secondary actions remain in navigation
and menus.
At widths below 100, the same combined sidebar opens on demand without changing the
saved preference. Below 60×16, Operator shows resize guidance and preserves work.
Unread review requires an unobscured, actually drawn, adequately sized transcript
at its confirmed durable tail; empty, new, too-small and sidebar-only views cannot
acknowledge it.
`Ctrl-n` opens creation, Enter sends, Shift-Enter inserts a newline, paste only edits,
Ctrl-g returns to Browse retaining drafts, and Ctrl-x stops only the selected exact
current turn. A pending submission clears only its captured draft after acceptance;
refusal or later typing preserves the draft.

Sidebar `x` and `[x]` open the native confirmation for that conversation's retained
run. Confirmation explains uncertain/live work and draft discard, then sends the
Operator Close request rather than workspace WIP-save. Only that chat is frozen
while close is pending. Cancel or error retains its draft and row; acknowledged
archive clears the confirmed draft and removes its active row without stealing a
newer selection. Ordinary `x` in either composer remains text. Sidebar focus,
collapse and viewport movement never acknowledge completion Unread.

OperatorChanged is a coalescible dirty signal, never transcript content. Tracked
requests load durable summaries and ordinal-paged history independently of focus,
with one read per conversation in flight and a further catch-up when dirtied
during a read. Only retained `worker_stopped` proof enables another submission;
Complete without that proof displays finishing/stop-unconfirmed. History commits
completion Unread only after a successful unobscured tail draw, with sufficient
transcript geometry. Model/tool strings escape terminal and directional controls
before rendering, including tool output with explicit empty-success results.

[Some lines truncated to 768 chars. Use :raw:27-27 to read line 27 whole]

SOURCE LOOKUP {"path": "crates/", "limit": 300}
# crates/
## ovrcr-tui/
Cargo.toml
### src/
theme.rs
task_tui.rs
lib.rs
#### dashboard/
operator.rs
unread.rs
text_cursor.rs
tests.rs
status.rs
state.rs
settings_editor.rs
render.rs
ready.rs
quota.rs
palette.rs
outbox.rs
navigation.rs
mod.rs
lifecycle_ui.rs
keymap.rs
iterm.rs
hints.rs
events.rs
event_loop.rs
desktop.rs
assignment_control.rs
picker.rs
whichkey.rs
settings.rs
agents.rs
input.rs
view_handshake.rs
terminal_guard.rs
copy.rs
git_hints.rs
##### operator/
render.rs
###### snapshots/
original-80x24.txt
original-60x24.txt
original-60x16.txt
original-140x40.txt
original-100x24.txt
#### theme/
catalog.rs
### tests/
settings_theme_preview.rs
## ovrcr-runtime/
Cargo.toml
### tests/
settings_matrix.rs
delegation_fallback_files.rs
delegation_cancellation_state.rs
theme_catalog.rs
### src/
retained.rs
lib.rs
cursor_cloud.rs
config.rs
agent_runner.rs
claude_recovery.rs
claude_delegation.rs
codex_recovery.rs
codex_delegation.rs
task_runner.rs
git.rs
extension_recovery.rs
recovery.rs
hermes_recovery.rs
tasks.rs
grok_recovery.rs
task_manager.rs
task_paging.rs
#### settings/
write.rs
mod.rs
#### session/
tests.rs
reporting.rs
mod.rs
io.rs
process.rs
shell_prompt.zshrc
shell_prompt.zshenv
shell_prompt.rs
#### server/
title.rs
tests.rs
startup.rs
outbound.rs
operator_tests.rs
operator.rs
navigation.rs
mod.rs
dispatch.rs
delegation_files.rs
connections.rs
build_identity.rs
delegation_reads.rs
delegation_questions.rs
delegation_policy.rs
delegation_observation.rs
delegation_interrupt_effects.rs
delegation_followups.rs
delegation_fallback_routes.rs
delegation_control.rs
delegation_archive.rs
delegation.rs
delegation_review.rs
delegation_dependencies.rs
delegation_coding.rs
delegation_recovery.rs
delegation_question_tests.rs
delegation_interrupt.rs
delegation_fallback.rs
delegation_delivery.rs
delegation_cancellation_effects.rs
delegation_cancellation.rs
dashboard.rs
quota.rs
quota_probe.rs
claude_allowance.rs
lifecycle.rs
watch.rs
event_log.rs
cursor_quota.rs
reporting_queue.rs
##### delegation_review/
view.rs
##### delegation_coding/
publication.rs
#### retained/
operator.rs
cloud_creation.rs
#### cursor_cloud/
run.rs
#### git/
candidate.rs
diagnostics.rs
##### candidate/
review.rs
baseline.rs
##### diagnostics/
tests.rs
## ovrcr-protocol/
Cargo.toml
### src/
wire.rs
settings.rs
session.rs
operator.rs
lib.rs
codec.rs
cloud_creation.rs
catalog.rs
agent.rs
delegation.rs
delegation_review_tests.rs
delegation_review.rs
workflow.rs
questions.rs
delegation_fallback.rs
delegation_dependencies.rs
delegation_delivery.rs
delegation_coding.rs
assignment_control.rs
quota.rs
iterm.rs
bridge_installation.rs
bridge.rs
auto_trust.rs
event.rs
registry.rs
client.rs
freshness.rs
context.rs
task.rs
### tests/
questions.rs
## ovrcr-terminal/
Cargo.toml
### src/
lib.rs
key.rs
paste.rs
history.rs

Requested limit 300 clamped to the max of 200

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs"}
use super::agents::{AgentSource, apply_overrides, detect_agents};
…
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
struct Suggestions { … }

#[derive(Clone)]
enum Command { … }

struct Entry { … }

enum FieldKind { … }

struct Field { … }

enum Page { … }

struct AgentCandidate { … }

fn running_agent(session: &SessionSummary) -> bool { … }

impl AgentCandidate { … }

pub(super) struct Palette { … }

impl Dashboard { … }

#[derive(Clone, Copy)]
enum PaletteTarget { … }

fn palette_geometry(outer: Rect) -> (Rect, Rect, Rect, Rect) { … }
fn button_text(page: &Page, width: u16) -> &'static str { … }

fn close_terminal_id(page: &Page) -> Option<SessionId> { … }

/// Direction D accent-rail card, terminal-close confirms only.
/// `d-confirmation-72.svg` uses a 9px cell: the peach rail is the rect at
/// x=9, width=18 (`#fab387`, two cells) and body glyphs start at x=36
/// (column 4). directions.md: the rail scales with measured card height.
const CLOSE_CONFIRM_RAIL_WIDTH: u16 = 2;
const CLOSE_CONFIRM_TEXT_COLUMN: u16 = 4;
const CLOSE_CONFIRM_ACTION_GAP: u16 = 2;
const CLOSE_CANCEL_LABEL: &str = "[Esc Cancel]";
const CLOSE_CONFIRM_LABEL: &str = "[Enter Close]";
const CLOSE_CONSEQUENCE: &str = "Stops its currently owned processes and archives its record.";
const CLOSE_TITLE: &str = "Close terminal?";

fn close_terminal_identity(name: &str, id: u64) -> String { … }

fn close_confirm_action_line() -> String { … }

struct CloseConfirmText { … }

#[derive(Clone, Copy)]
enum CloseConfirmRole { … }

struct CloseConfirmLayout { … }

fn close_confirm_dialog_width(screen_width: u16) -> u16 { … }

fn close_confirm_content_width(dialog_width: u16) -> u16 { … }

fn close_confirm_layout(screen: Rect, body_rows: u16) -> CloseConfirmLayout { … }

fn measure_close_confirm(screen: Rect, text: &CloseConfirmText) -> CloseConfirmLayout { … }

fn close_confirm_segments(text: &CloseConfirmText) -> Vec<(String, CloseConfirmRole)> { … }

fn close_confirm_lines(text: &CloseConfirmText, width: u16) -> Vec<Line<'static>> { … }

fn close_confirm_line_count(text: &CloseConfirmText, width: u16) -> u16 {
    u16::try_from(close_confirm_lines(text, width).len()).unwrap_or(u16::MAX)
}

fn close_confirm_style(role: CloseConfirmRole) -> Style { … }

fn wrap_plain(text: &str, width: usize) -> Vec<String> { … }

fn split_at_width(text: &str, width: usize) -> (&str, &str) { … }

fn paint_rect(frame: &mut Frame<'_>, area: Rect, style: Style) { … }

fn paint_close_confirm_title(frame: &mut Frame<'_>, dialog: Rect) { … }

impl Palette { … }

fn text_field(label: &'static str, value: String, required: bool) -> Field { … }

fn ensure_workspace_id(palette: &mut Palette) -> Result<String, String> { … }
fn pick_field(label: &'static str, value: String, items: Vec<PickItem>) -> Field { … }

fn visible_indices(fields: &[Field]) -> Vec<usize> { … }

/// `draw_palette` only holds `&self` and cannot recompute a Path field's
/// directory listing, so every key handler that edits a Path field's value
/// or moves its selection must refresh the picker's cache here.
fn refresh_path_listing(field: &mut Field, roots: &[PathBuf]) { … }

fn move_form_field(fields: &[Field], active: &mut usize, backwards: bool) { … }

/// A deferred submit replays Enter on a pick list the user never saw, so only
/// the branch they actually typed may stand; a fuzzy match must be confirmed.
fn unconfirmed_pick(page: &Page) -> Option<&'static str> { … }

fn accept_pick(field: &mut Field) -> bool { … }

impl Dashboard { … }

fn form_list_active(page: &Page) -> bool { … }

fn mark_form_edit(
    fields: &mut [Field],
    active: usize,
    name_edited: &mut bool,
    root_edited: &mut bool,
) { … }
fn git_ref_branch(branch: &str) -> bool { … }

impl Dashboard { … }

fn removal_confirmation(request: Request, display: &str) -> Page { … }

#[cfg(test)]
mod mouse_tests { … }

#[cfg(test)]
mod launch_tests { … }

#[cfg(test)]
mod close_confirm_tests { … }

[…6005ln elided; re-read needed ranges, e.g. crates/ovrcr-tui/src/dashboard/palette.rs:2-25,29-37]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:1-360"}
use super::agents::{AgentSource, apply_overrides, detect_agents};
use super::input::is_browse_key;
use super::keymap::{Action, KeyBinding, keymap};
use super::picker::{
    PathPicker, PickItem, PickList, complete_path, expand_path, split_workspace_pick,
};
use super::render::{CRUST, MAUVE, MUTED, PEACH, SUBTEXT, TEXT, clip_text};
use super::state::{
    find_session, find_workspace, session_workspace_heading, workspace_heading, workspace_label,
};
use super::{Dashboard, DashboardAction, InputMode};
use crate::protocol::{
    BranchRequest, ClientMessage, CreateSessionRequest, Request, Response, SessionKind,
    SessionLaunch, SessionPhase, SessionRunId, SessionSummary, new_workspace_id,
};
use crate::session::SessionId;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
struct Suggestions {
    inspect: Option<u64>,
    repos: HashMap<String, PathBuf>,
    cache: HashMap<String, Result<super::git_hints::Hints, String>>,
    worker: Option<super::git_hints::Worker>,
    in_flight: Option<String>,
    project: Option<String>,
    note: Option<String>,
}

#[derive(Clone)]
enum Command {
    WorkflowRecovery(SessionId, SessionRunId),
    TransferWorkflow(Box<crate::protocol::WorkflowTransfer>),
    CreateTerminal,
    CreateWorkspace,
    RenameTerminal(SessionId),
    ReopenTerminal(SessionId),
    StartNewConversation(SessionId),
    AcknowledgeStopped(SessionId, crate::protocol::SessionRunId),
    AgentResumeUnavailable(SessionId),
    RecoverLaunch,
    RegisterProject,
    CloseTerminal(SessionId),
    Archives,
    Unarchive(SessionId, crate::protocol::SessionRunId),
    DeleteArchived(SessionId, crate::protocol::SessionRunId, String),
    RemoveWorkspace,
    RemoveProject,
    Switch(SessionId),
    SwitchAgent(SessionId, SessionRunId),
    Hint(Action),
    Settings,
    Events,
    RefreshQuota,
    EnableQuota,
    #[cfg(target_os = "macos")]
    ITermSetup,
}

struct Entry {
    label: String,
    command: Command,
}

enum FieldKind {
    Text,
    Toggle,
    Pick(PickList),
    Path(PathPicker),
}

struct Field {
    label: &'static str,
    value: String,
    required: bool,
    hidden: bool,
    edited: bool,
    kind: FieldKind,
}

enum Page {
    Search {
        query: String,
        selected: usize,
    },
    Form {
        command: Command,
        fields: Vec<Field>,
        active: usize,
        name_edited: bool,
        root_edited: bool,
    },
    Confirm {
        request: Option<Request>,
        target: String,
    },
}

struct AgentCandidate {
    session: SessionSummary,
    search_identity: String,
    unavailable: Option<&'static str>,
}

fn running_agent(session: &SessionSummary) -> bool {
    !session.archived
        && session.phase == SessionPhase::Running
        && matches!(session.kind, SessionKind::Agent { .. })
}

impl AgentCandidate {
    fn reason(&self, dashboard: &Dashboard) -> Option<&'static str> {
        if self.unavailable.is_some() {
            return self.unavailable;
        }
        match find_session(dashboard, self.session.id) {
            None => Some("Agent unavailable: session removed; reopen Agents to refresh"),
            Some(current) if current.run != self.session.run => {
                Some("Agent unavailable: process run changed; reopen Agents to refresh")
            }
            Some(current) if !running_agent(current) => {
                Some("Agent unavailable: paused or stopped; reopen Agents to refresh")
            }
            Some(current) if dashboard.focused_session() == Some(current.id) => {
                Some("Agent unavailable: already focused; reopen Agents to refresh")
            }
            _ => None,
        }
    }
}

pub(super) struct Palette {
    page: Page,
    agents: Option<Vec<AgentCandidate>>,
    pending: Option<u64>,
    archive: Option<Vec<crate::protocol::SessionSummary>>,
    workflows: Option<Vec<crate::protocol::WorkflowTransfer>>,
    error: Option<String>,
    workspace_acknowledged: bool,
    workspace_id: Option<String>,
    pub(super) launch_preference: Option<(String, super::settings::LaunchChoice)>,
    failed_launch: Option<CreateSessionRequest>,
    launch_project: Option<String>,
    suggestions: Suggestions,
    // Some(true) retains explicit whole-form Submit; Some(false) replays Enter.
    submit_when_ready: Option<bool>,
    cursor: super::text_cursor::TextCursor,
    scroll: Option<usize>,
}

impl Dashboard {
    pub(super) fn cancel_notification_palette(&mut self) {
        if let Some(palette) = self.palette.take() {
            if let Some(request_id) = palette.pending {
                self.navigation.detached_palette_requests.insert(request_id);
            }
            if let Some(request_id) = palette.suggestions.inspect {
                self.ignored_responses.insert(request_id);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum PaletteTarget {
    Search,
    Entry(usize),
    Field(usize, bool),
    Option(usize, usize),
}

fn palette_geometry(outer: Rect) -> (Rect, Rect, Rect, Rect) {
    let width = outer.width.min(82);
    let height = outer.height.min(19);
    let area = Rect::new(
        outer.x + (outer.width - width) / 2,
        outer.y + (outer.height - height) / 2,
        width,
        height,
    );
    let inner = Block::bordered().inner(area);
    let button_height = inner.height.min(1);
    let footer = inner
        .height
        .saturating_sub(button_height)
        .min(4)
        .min(inner.height / 3);
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer + button_height),
    );
    let buttons = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(button_height),
        inner.width,
        button_height,
    );
    (area, inner, body, buttons)
}
fn button_text(page: &Page, width: u16) -> &'static str {
    if width < 17 {
        return "[OK] [X]";
    }
    match page {
        Page::Search { .. } => "[Open] [Cancel]",
        Page::Confirm { .. } => "[Confirm] [Cancel]",
        Page::Form { .. } => "[Submit] [Cancel]",
    }
}

fn close_terminal_id(page: &Page) -> Option<SessionId> {
    match page {
        Page::Confirm {
            request: Some(Request::CloseTerminal { session, .. }),
            ..
        } => Some(*session),
        _ => None,
    }
}

/// Direction D accent-rail card, terminal-close confirms only.
/// `d-confirmation-72.svg` uses a 9px cell: the peach rail is the rect at
/// x=9, width=18 (`#fab387`, two cells) and body glyphs start at x=36
/// (column 4). directions.md: the rail scales with measured card height.
const CLOSE_CONFIRM_RAIL_WIDTH: u16 = 2;
const CLOSE_CONFIRM_TEXT_COLUMN: u16 = 4;
const CLOSE_CONFIRM_ACTION_GAP: u16 = 2;
const CLOSE_CANCEL_LABEL: &str = "[Esc Cancel]";
const CLOSE_CONFIRM_LABEL: &str = "[Enter Close]";
const CLOSE_CONSEQUENCE: &str = "Stops its currently owned processes and archives its record.";
const CLOSE_TITLE: &str = "Close terminal?";

fn close_terminal_identity(name: &str, id: u64) -> String {
    let marked = format!("(#{id})");
    if name.contains(&marked) {
        name.to_string()
    } else {
        format!("{name} {marked}")
    }
}

fn close_confirm_action_line() -> String {
    let gap = " ".repeat(usize::from(CLOSE_CONFIRM_ACTION_GAP));
    format!("{CLOSE_CANCEL_LABEL}{gap}{CLOSE_CONFIRM_LABEL}")
}

struct CloseConfirmText {
    warning: Option<&'static str>,
    identity: String,
    place: String,
    project: String,
    workspace: String,
    error: Option<String>,
    pending: bool,
}

#[derive(Clone, Copy)]
enum CloseConfirmRole {
    Blank,
    Identity,
    Place,
    Meta,
    Consequence,
}

struct CloseConfirmLayout {
    dialog: Rect,
    rail: Rect,
    text: Rect,
    cancel: Rect,
    confirm: Rect,
    body_rows: u16,
}

fn close_confirm_dialog_width(screen_width: u16) -> u16 {
    let labels = CLOSE_CANCEL_LABEL.len() as u16
        + CLOSE_CONFIRM_ACTION_GAP
        + CLOSE_CONFIRM_LABEL.len() as u16;
    let min_width = CLOSE_CONFIRM_TEXT_COLUMN.saturating_mul(2) + labels;
    if screen_width <= min_width {
        screen_width
    } else {
        // Same horizontal cap as the other palette cards.
        screen_width.min(82)
    }
}

fn close_confirm_content_width(dialog_width: u16) -> u16 {
    let edge = CLOSE_CONFIRM_TEXT_COLUMN.min(dialog_width / 2);
    dialog_width.saturating_sub(edge.saturating_mul(2))
}

fn close_confirm_layout(screen: Rect, body_rows: u16) -> CloseConfirmLayout {
    let width = close_confirm_dialog_width(screen.width).min(screen.width);
    let mut height = body_rows.saturating_add(3); // action row + two border rows
    if screen.height > 0 {
        height = height.min(screen.height);
    }
    if screen.height >= 3 {
        height = height.max(3);
    }
    let dialog = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let rail = Rect::new(
        dialog.x.saturating_add(1),
        dialog.y.saturating_add(1),
        CLOSE_CONFIRM_RAIL_WIDTH.min(dialog.width.saturating_sub(2)),
        dialog.height.saturating_sub(2),
    );
    let edge = CLOSE_CONFIRM_TEXT_COLUMN.min(dialog.width / 2);
    let text = Rect::new(
        dialog.x.saturating_add(edge),
        dialog.y.saturating_add(1),
        close_confirm_content_width(dialog.width),
        dialog.height.saturating_sub(3),
    );
    let action_y = dialog.y + dialog.height.saturating_sub(2);
    let action_h = u16::from(dialog.height >= 2);
    let confirm_w = CLOSE_CONFIRM_LABEL.len() as u16;
    let cancel_w = CLOSE_CANCEL_LABEL.len() as u16;
    let content_right = text.x.saturating_add(text.width);
    let confirm_x = content_right.saturating_sub(confirm_w);
    let cancel_x = confirm_x.saturating_sub(CLOSE_CONFIRM_ACTION_GAP + cancel_w);
    CloseConfirmLayout {
        dialog,
        rail,
        text,
        cancel: Rect::new(cancel_x, action_y, cancel_w, action_h),
        confirm: Rect::new(confirm_x, action_y, confirm_w, action_h),
        body_rows,
    }
}

fn measure_close_confirm(screen: Rect, text: &CloseConfirmText) -> CloseConfirmLayout {
    let width = close_confirm_dialog_width(screen.width).min(screen.width);
    let rows = close_confirm_line_count(text, close_confirm_content_width(width));
    close_confirm_layout(screen, rows)
}

fn close_confirm_segments(text: &CloseConfirmText) -> Vec<(String, CloseConfirmRole)> {
    let mut lines = vec![
        (String::new(), CloseConfirmRole::Blank),
        (text.identity.clone(), CloseConfirmRole::Identity),
        (text.place.clone(), CloseConfirmRole::Place),
        (format!("Project: {}", text.project), CloseConfirmRole::Meta),
        (
            format!("Workspace: {}", text.workspace),
            CloseConfirmRole::Meta,
        ),
…
    ];
…
}

[Showing lines 1-363 of 6184. Use :364 to continue]

SOURCE LOOKUP {"pattern": "fn (open_|draw_palette|handle_|form_|submit_|apply_|create_|launch_|workspace_|agent_|branch_)", "path": "crates/ovrcr-tui/src/dashboard/palette.rs"}
 680|impl Dashboard {
*681|    pub(super) fn open_agent_search(&mut self) -> DashboardAction {
 682|        self.agent_typing = None;
 683|        self.cancel_mouse_gesture();
 684|        self.whichkey = None;
...
 763|
*764|    pub(super) fn open_palette(&mut self) -> DashboardAction {
 765|        self.agent_typing = None;
 766|        self.cancel_mouse_gesture();
 767|        self.whichkey = None;
...
 785|
*786|    pub(super) fn open_create_terminal(&mut self) -> DashboardAction {
 787|        if let Some(warning) = self
 788|            .current_workspace()
 789|            .and_then(|workspace| workspace.warning.clone())
...
 819|
*820|    pub(super) fn open_create_workspace(&mut self) -> DashboardAction {
 821|        let action = self.open_palette();
 822|        let page = self.palette_form(Command::CreateWorkspace);
 823|        let palette = self.palette.as_mut().unwrap();
...
 1015|
*1016|    pub(super) fn open_register_project(&mut self) -> DashboardAction {
 1017|        self.cancel_mouse_gesture();
 1018|        if let Some(begin) = self.history_begin_request.as_mut() {
 1019|            begin.cancelled = true;
...
 1040|
*1041|    pub(super) fn open_close_terminal(&mut self) -> DashboardAction {
 1042|        let Some(id) = self.action_session() else {
 1043|            return DashboardAction::None;
 1044|        };
...
 1048|    /// Archive `id` without making it the focused pane's session.
*1049|    pub(super) fn open_close_terminal_for(&mut self, id: SessionId) -> DashboardAction {
 1050|        if find_session(self, id).is_none() {
 1051|            return DashboardAction::None;
 1052|        }
...
 1083|    /// palette's Resume / Reopen entry does, or the unavailable notice.
*1084|    pub(super) fn open_reopen_terminal(&mut self) -> DashboardAction {
 1085|        let Some(session) = self
 1086|            .action_session()
 1087|            .and_then(|id| find_session(self, id))
...
 1114|
*1115|    pub(super) fn open_close_operator_for(
 1116|        &mut self,
 1117|        id: ovrcr_protocol::OperatorConversationId,
 1118|    ) -> DashboardAction {
...
 1259|
*1260|    pub(super) fn open_forced_remove_workspace(
 1261|        &mut self,
 1262|        project: String,
 1263|        name: String,
...
 1284|
*1285|    pub(super) fn open_remove_workspace(&mut self, project: String, id: String) -> DashboardAction {
 1286|        let Some(workspace) = find_workspace(self, &project, &id) else {
 1287|            return DashboardAction::None;
 1288|        };
...
 1299|
*1300|    pub(super) fn open_remove_context(&mut self, workspace: bool) -> DashboardAction {
 1301|        let (project, name) = self.creation_context();
 1302|        if project.is_empty() || (workspace && name.is_empty()) {
 1303|            return DashboardAction::None;
...
 2674|
*2675|    fn launch_fields(&self, project: &str, empty: bool) -> Vec<Field> {
 2676|        let choice = self.settings.launch_choices.get(project);
 2677|        let start = match choice {
 2678|            Some(super::settings::LaunchChoice::Agent(_)) => "Agent",
...
 3750|
*3751|    pub(super) fn draw_palette(&self, frame: &mut Frame<'_>) {
 3752|        let Some(palette) = &self.palette else {
 3753|            return;
 3754|        };
...
 3933|
*3934|fn form_list_active(page: &Page) -> bool {
 3935|    matches!(
 3936|        page,
 3937|        Page::Form {
...
 4783|    #[test]
*4784|    fn workspace_submits_one_selected_launch_or_empty() {
 4785|        for start in ["Terminal", "Agent", "Nothing yet"] {
 4786|            let mut d = dashboard();
 4787|            start_workspace(&mut d, start);
...
 5019|    #[test]
*5020|    fn launch_save_warning_survives_selection_ack_and_session_accepts_input() {
 5021|        for coalesced in [false, true] {
 5022|            let mut d = dashboard();
 5023|            let mut hierarchy = d.hierarchy.clone();

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:2600-3000"}
    fn palette_key_inner(
…
    ) -> DashboardAction {
…
        match key.code {
…
            KeyCode::Enter => match &mut palette.page {
…
                Page::Confirm { request, .. } => {
                    if let Some(request) = request.clone() {
                        if let Request::Operator(ovrcr_protocol::OperatorRequest::Close {
…
                        {
                            self.operator_close(conversation, expected_run);
                            self.palette = None;
                            return DashboardAction::Redraw;
                        }
                        action = self.palette_submit(&mut palette, request, history_before_request);
                    } else {
                        self.palette = None;
                        return DashboardAction::Redraw;
                    }
                }
            },
            _ => {}
        }
        self.refresh_workspace_form(&mut palette);
        self.palette = Some(palette);
        action
    }

    pub(super) fn detected_agents(&self) -> Vec<super::agents::AgentEntry> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let shell = std::env::var_os("SHELL");
        let launcher = std::env::current_exe().ok();
        let mut entries = apply_overrides(
            detect_agents(&path, shell.as_deref(), launcher.as_deref()),
            &self.settings.agents,
        );
        entries.retain(|entry| {
            entry.name != "cursor-cloud" && !entry.name.starts_with("cursor-cloud:")
        });
        if cfg!(target_os = "macos")
            && let Some(launcher) = launcher
        {
            for account in &self.settings.cursor_cloud_accounts {
                if account.validate().is_ok() {
                    entries.insert(
                        entries.len().saturating_sub(2),
                        super::agents::AgentEntry {
                            name: format!("cursor-cloud:{}", account.name),
                            argv: vec![
                                launcher.as_os_str().to_owned(),
                                "agent".into(),
                                "run".into(),
                                "cursor-cloud".into(),
                                "--".into(),
                                account.name.clone().into(),
                            ],
                            source: AgentSource::Detected,
                        },
                    );
                }
            }
        }
        entries
    }

    fn refresh_terminal_form(&self, fields: &mut [Field], _name_edited: bool) {
        let Some(start) = fields
            .iter()
            .find(|f| f.label == "Start")
            .map(|f| f.value.clone())
        else {
            return;
        };
        for field in fields {
            if field.label == "Agent" {
                field.hidden = start != "Agent";
                field.required = start == "Agent";
            }
            if field.label == "Command" {
                field.hidden = start != "Terminal";
                field.required = false;
            }
        }
    }

    fn launch_fields(&self, project: &str, empty: bool) -> Vec<Field> {
        let choice = self.settings.launch_choices.get(project);
        let start = match choice {
            Some(super::settings::LaunchChoice::Agent(_)) => "Agent",
            _ => "Terminal",
        };
        let mut choices = vec!["Agent", "Terminal"];
        if empty {
            choices.push("Nothing yet");
        }
        let agents = self
            .detected_agents()
            .into_iter()
            .filter(|a| {
                a.source != AgentSource::Shell
                    && a.source != AgentSource::Custom
                    && a.name != "shell"
                    && a.name != "Custom"
            })
            .collect::<Vec<_>>();
        let agent = match choice {
            Some(super::settings::LaunchChoice::Agent(name)) => name.clone(),
            _ => agents.first().map(|a| a.name.clone()).unwrap_or_default(),
        };
        let mut fields = vec![
            pick_field(
                "Start",
                start.into(),
                choices
                    .into_iter()
                    .map(|s| PickItem {
                        label: s.into(),
                        value: s.into(),
                    })
                    .collect(),
            ),
            pick_field(
                "Agent",
                agent,
                agents
                    .into_iter()
                    .map(|a| PickItem {
                        label: a.name.clone(),
                        value: a.name,
                    })
                    .collect(),
            ),
            text_field("Command", String::new(), false),
        ];
        self.refresh_terminal_form(&mut fields, false);
        fields
    }

    fn shell_argv(&self) -> Vec<OsString> {
        self.detected_agents()
            .into_iter()
            .find(|entry| entry.source == AgentSource::Shell)
            .expect("agent detection always includes the shell entry")
            .argv
    }

    fn selected_launch(&self, fields: &[Field]) -> Result<Option<SessionLaunch>, String> {
        let value = |label| {
            fields
                .iter()
                .find(|f| f.label == label)
                .map(|f| f.value.trim())
                .unwrap_or("")
        };
        match value("Start") {
            "Nothing yet" => Ok(None),
            "Agent" => self
                .detected_agents()
                .into_iter()
                .find(|a| {
                    a.name == value("Agent")
                        && a.source != AgentSource::Shell
                        && a.source != AgentSource::Custom
                        && a.name != "shell"
                        && a.name != "Custom"
                })
                .map(|a| {
                    Some(SessionLaunch {
                        argv: a.argv,
                        kind: SessionKind::Agent {
                            name: a.name.clone(),
                        },
                        label: Some(a.name),
                    })
                })
                .ok_or_else(|| {
                    format!(
                        "Agent '{}' is unavailable. Choose another agent or explicitly select Terminal.",
                        value("Agent")
                    )
                }),
            "Terminal" => Ok(Some(SessionLaunch {
                argv: if value("Command").is_empty() {
                    self.shell_argv()
                } else {
                    vec![
                        "/bin/sh".into(),
                        "-lc".into(),
                        OsString::from(value("Command")),
                    ]
                },
                label: None,
                kind: SessionKind::Terminal,
            })),
            _ => Err("Choose Agent, Terminal, or Nothing yet".into()),
        }
    }

    fn refresh_project_form(&self, fields: &mut [Field], name_edited: bool, root_edited: bool) {
        if fields.first().map(|field| field.label) != Some("Repository") {
            return;
        }
        let repo = fields[0].value.trim_end_matches('/').to_string();
        if !name_edited {
            let name = Path::new(&repo)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(field) = fields.iter_mut().find(|field| field.label == "Name") {
                field.value = name;
            }
        }
        if root_edited {
            return;
        }
        let name = fields
            .iter()
            .find(|field| field.label == "Name")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        if let Some(field) = fields
            .iter_mut()
            .find(|field| field.label == "Workspace root")
        {
            field.value = self
                .config_dir
                .join("workspaces")
                .join(name)
                .display()
                .to_string();
        }
    }

    fn palette_submit(
        &mut self,
        palette: &mut Palette,
        request: Request,
        history_before_request: bool,
    ) -> DashboardAction {
        if self.lifecycle_busy()
            && super::lifecycle_ui::Provisional::from_request(0, &request).is_some()
        {
            let message = "a lifecycle job is already running".to_string();
            self.refuse_lifecycle_job(message.clone());
            palette.error = Some(message);
            return DashboardAction::Redraw;
        }
        let launch = match &request {
            Request::CreateWorkspaceWithLaunch {
                project,
                id: name,
                launch: Some(launch),
                ..
            } => Some(CreateSessionRequest {
                project: project.clone(),
                workspace: name.clone(),
                name: String::new(),
                argv: launch.argv.clone(),
                label: launch.label.clone(),
                kind: launch.kind.clone(),
            }),
            Request::CreateSession(session) => Some(session.clone()),
            _ => None,
        };
        if let Some(launch) = launch {
            palette.launch_preference = Some((
                launch.project.clone(),
                match &launch.label {
                    Some(label) => super::settings::LaunchChoice::Agent(label.clone()),
                    None => super::settings::LaunchChoice::Terminal,
                },
            ));
            palette.failed_launch = Some(launch);
        }
        let request_id = self.next_request_id();
        palette.pending = Some(request_id);
        palette.error = None;
        self.begin_lifecycle_pending(request_id, &request);
        if history_before_request {
            self.queue_history_request();
        }
        self.push_request(ClientMessage {
            request_id,
            request,
        });
        DashboardAction::Redraw
    }

    fn palette_form(&self, command: Command) -> Page {
        let (project, workspace) = self.creation_context();
        let text = |label, value: String, required| Field {
            label,
            value,
            required,
            hidden: false,
            edited: false,
            kind: FieldKind::Text,
        };
        let fields = match command {
            Command::CreateTerminal => {
                let workspace_list =
                    PickList::workspaces(&self.hierarchy, &project, &workspace, true);
                let workspace_value = workspace_list
                    .accepted()
                    .map(|item| item.value.clone())
                    .unwrap_or_default();
                let launch_project = split_workspace_pick(&workspace_value).0;
                let launch_project = if launch_project.is_empty() {
                    project.clone()
                } else {
                    launch_project
                };
                let mut launch = self.launch_fields(&launch_project, false);
                let command = launch.pop().unwrap();
                let agent = launch.pop().unwrap();
                vec![
                    launch.pop().unwrap(),
                    Field {
                        label: "Workspace",
                        value: workspace_value,
                        required: true,
                        hidden: false,
                        edited: false,
                        kind: FieldKind::Pick(workspace_list),
                    },
                    text("Name", String::new(), false),
                    command,
                    agent,
                ]
            }
            Command::RenameTerminal(id) => vec![text(
                "Name",
                find_session(self, id)
                    .map(|s| s.display_name().to_string())
                    .unwrap_or_default(),
                false,
            )],
            Command::CreateWorkspace => {
                let list = PickList::projects(&self.hierarchy, &project);
                let mut project = text(
                    "Project",
                    list.accepted()
                        .map(|item| item.value.clone())
                        .unwrap_or_default(),
                    true,
                );
                project.kind = FieldKind::Pick(list);
                let mut mode = text("Branch mode", "new".into(), true);
                mode.kind = FieldKind::Toggle;
                let project_name = project.value.clone();
                let mut fields = vec![
                    project,
                    text("Branch", self.settings.branch_prefix.clone(), true),
                    mode,
                    text("Base", "main".into(), true),
                ];
                fields.extend(self.launch_fields(&project_name, true));
                fields
            }
            Command::RegisterProject => vec![
                Field {
                    label: "Repository",
                    value: String::new(),
                    required: true,
                    hidden: false,
                    edited: false,
                    kind: FieldKind::Path(PathPicker::new()),
                },
                text("Name", String::new(), true),
                Field {
                    label: "Workspace root",
                    value: String::new(),
                    required: true,
                    hidden: false,
                    edited: false,
                    kind: FieldKind::Path(PathPicker::new()),
                },
            ],
            Command::RemoveWorkspace | Command::RemoveProject => {
                let (label, list) = if matches!(command, Command::RemoveWorkspace) {
                    (
                        "Workspace",
                        PickList::workspaces(&self.hierarchy, &project, &workspace, false),
                    )
                } else {
                    ("Project", PickList::projects(&self.hierarchy, &project))
                };
                let mut field = text(
                    label,
                    list.accepted()
                        .map(|item| item.value.clone())
                        .unwrap_or_default(),
                    true,
                );
                field.kind = FieldKind::Pick(list);
                vec![field]
            }
            _ => unreachable!(),
        };
        let active = if matches!(command, Command::CreateWorkspace) {
            1
        } else {
            0
        };
        Page::Form {
            command,
            fields,
            active,
            name_edited: false,
            root_edited: false,
        }
    }

    pub(super) fn palette_response(
…
    ) -> Option<Vec<ClientMessage>> {
…
    }

[Showing lines 2599-3003 of 6184. Use :3004 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:760-1100"}
    fn entry_session<'a>(
…
    ) -> Option<&'a SessionSummary> {
        match *command {
…
            Command::SwitchAgent(id, run) => {
…
            }
            _ => None,
        }
    }

    pub(super) fn open_palette(&mut self) -> DashboardAction {
        self.agent_typing = None;
        self.cancel_mouse_gesture();
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.mode = InputMode::Browse;
        if self.palette.is_some() {
            return DashboardAction::Redraw;
        }
        let request_id = self.next_request_id();
        let mut palette = Palette::new();
        palette.suggestions.inspect = Some(request_id);
        self.palette = Some(palette);
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::Inspect,
        });
        DashboardAction::Redraw
    }

    pub(super) fn open_create_terminal(&mut self) -> DashboardAction {
        if let Some(warning) = self
            .current_workspace()
            .and_then(|workspace| workspace.warning.clone())
        {
            self.set_error(warning);
            return DashboardAction::Redraw;
        }
        self.cancel_mouse_gesture();
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.palette = Some(Palette {
            page: self.palette_form(Command::CreateTerminal),
            agents: None,
            pending: None,
            archive: None,
            workflows: None,
            error: None,
            workspace_acknowledged: false,
            workspace_id: None,
            launch_preference: None,
            failed_launch: None,
            launch_project: None,
            suggestions: Suggestions::default(),
            submit_when_ready: None,
            cursor: Default::default(),
            scroll: None,
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_create_workspace(&mut self) -> DashboardAction {
        let action = self.open_palette();
        let page = self.palette_form(Command::CreateWorkspace);
        let palette = self.palette.as_mut().unwrap();
        palette.page = page;
        let _ = ensure_workspace_id(palette);
        action
    }

    fn refresh_workspace_form(&self, palette: &mut Palette) {
        if let Page::Form {
            fields, command, ..
        } = &mut palette.page
        {
            let project = match command {
                Command::CreateTerminal => fields
                    .iter()
                    .find(|field| field.label == "Workspace")
                    .map(|field| split_workspace_pick(&field.value).0),
                Command::CreateWorkspace => fields
                    .iter()
                    .find(|field| field.label == "Project")
                    .map(|field| field.value.clone()),
                _ => None,
            };
            if let Some(project) = project {
                if palette
                    .launch_project
                    .as_ref()
                    .is_some_and(|old| old != &project)
                {
                    for replacement in
                        self.launch_fields(&project, matches!(command, Command::CreateWorkspace))
                    {
                        if let Some(field) =
                            fields.iter_mut().find(|f| f.label == replacement.label)
                        {
                            *field = replacement;
                        }
                    }
                }
                palette.launch_project = Some(project);
            }
            self.refresh_terminal_form(fields, false);
        }
        let hints = &mut palette.suggestions;
        if let Some((project, result)) = hints.worker.as_ref().and_then(|worker| worker.poll()) {
            hints.cache.insert(project, result);
            hints.in_flight = None;
        }
        let Page::Form {
            command: Command::CreateWorkspace,
            fields,
            ..
        } = &mut palette.page
        else {
            return;
        };
        let project = fields
            .iter()
            .find(|field| field.label == "Project")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        let existing = fields
            .iter()
            .find(|field| field.label == "Branch mode")
            .is_some_and(|field| field.value == "existing");
        let Some(branch_i) = fields.iter().position(|field| field.label == "Branch") else {
            return;
        };
        let Some(base_i) = fields.iter().position(|field| field.label == "Base") else {
            return;
        };
        fields[base_i].hidden = existing;
        fields[base_i].required = !existing;
        if hints.project.as_ref() != Some(&project) {
            hints.project = Some(project.clone());
            fields[branch_i].kind = FieldKind::Text;
            if existing {
                fields[branch_i].value.clear();
            }
        }
        if !existing && !fields[branch_i].edited {
            fields[branch_i].value = self.settings.branch_prefix.clone();
        }
        if hints.inspect.is_none()
            && !hints.cache.contains_key(&project)
            && hints.in_flight.is_none()
        {
            if let Some(repo) = hints.repos.get(&project) {
                if hints.worker.is_none() {
                    hints.worker = super::git_hints::Worker::start().ok();
                }
                if hints
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.request(project.clone(), repo.clone()))
                {
                    hints.in_flight = Some(project.clone());
                } else {
                    hints
                        .cache
                        .insert(project.clone(), Err("Could not start Git lookup".into()));
                }
            } else {
                hints
                    .cache
                    .insert(project.clone(), Err("Repository path unavailable".into()));
            }
        }
        hints.note = None;
        match hints.cache.get(&project) {
            Some(Ok(result)) => {
                if !fields[base_i].edited {
                    fields[base_i].value.clone_from(&result.base);
                }
                if existing && !matches!(fields[branch_i].kind, FieldKind::Pick(_)) {
                    let mut list = PickList::new(
                        result
                            .branches
                            .iter()
                            .map(|branch| PickItem {
                                label: branch.clone(),
                                value: branch.clone(),
                            })
                            .collect(),
                    );
                    if fields[branch_i].value.is_empty() {
                        fields[branch_i].value =
                            result.branches.first().cloned().unwrap_or_default();
                    }
                    if result.branches.contains(&fields[branch_i].value) {
                        list.select_value(&fields[branch_i].value);
                    } else {
                        list.query.clone_from(&fields[branch_i].value);
                    }
                    if !result.branches.is_empty() {
                        fields[branch_i].kind = FieldKind::Pick(list);
                    }
                }
            }
            Some(Err(error)) => {
                hints.note = Some(format!("{error}; enter branch/base manually"));
                if !fields[base_i].edited {
                    fields[base_i].value = "main".into();
                }
            }
            None => {
                hints.note = Some("Loading Git suggestions…".into());
                if !fields[base_i].edited {
                    fields[base_i].value = "main".into();
                }
            }
        }
    }

    // Called by the real client loop, even when there is no keyboard/server input.
    pub fn poll_palette(&mut self) -> bool {
        let Some(mut palette) = self.palette.take() else {
            return false;
        };
        let was_loading = palette.suggestions.in_flight.is_some();
        self.refresh_workspace_form(&mut palette);
        let ready = match &palette.page {
            Page::Form {
                command: Command::CreateWorkspace,
                fields,
                ..
            } => palette.suggestions.cache.contains_key(&fields[0].value),
            _ => false,
        };
        let mut submit = palette.submit_when_ready.filter(|_| ready);
        let mut refused = false;
        if submit.is_some() {
            palette.submit_when_ready = None;
            // The hints arrived after the user pressed Enter, so they never saw
            // this list. Make them confirm anything but what they typed.
            if let Some(label) = unconfirmed_pick(&palette.page) {
                palette.error = Some(format!("{label} not found in repository"));
                submit = None;
                refused = true;
            }
        }
        self.palette = Some(palette);
        if let Some(submit_all) = submit {
            // Deferred submission was already queued before the writer scheduled History.
            self.palette_key_inner(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                submit_all,
                false,
            );
            self.refresh_active_path_listing();
        }
        was_loading || submit.is_some() || refused
    }

    pub(super) fn open_register_project(&mut self) -> DashboardAction {
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let mut page = self.palette_form(Command::RegisterProject);
        if let Page::Form { fields, .. } = &mut page {
            for field in fields.iter_mut() {
                refresh_path_listing(field, &self.settings.picker_roots);
            }
        }
        self.palette = Some(Palette {
            page,
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn close_confirm_session(&self) -> Option<SessionId> {
        self.palette
            .as_ref()
            .and_then(|palette| close_terminal_id(&palette.page))
    }

    pub(super) fn open_close_terminal(&mut self) -> DashboardAction {
        let Some(id) = self.action_session() else {
            return DashboardAction::None;
        };
        self.open_close_terminal_for(id)
    }

    /// Archive `id` without making it the focused pane's session.
    pub(super) fn open_close_terminal_for(&mut self, id: SessionId) -> DashboardAction {
        if find_session(self, id).is_none() {
            return DashboardAction::None;
        }
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        let session = find_session(self, id).unwrap();
        let immediate = !session.phase.is_live() && session.close_warning().is_none();
        let run = session.run;
        let mut palette = Palette {
            page: self.command_page(Command::CloseTerminal(id)),
            ..Palette::new()
        };
        let action = if immediate {
            self.palette_submit(
                &mut palette,
                Request::CloseTerminal {
                    session: id,
                    expected_run: run,
                },
                true,
            )
        } else {
            DashboardAction::Redraw
        };
        self.palette = Some(palette);
        self.mode = InputMode::Browse;
        action
    }

    /// Enter on a session that is no longer live. Opens the same confirmation the
    /// palette's Resume / Reopen entry does, or the unavailable notice.
    pub(super) fn open_reopen_terminal(&mut self) -> DashboardAction {
        let Some(session) = self
            .action_session()
            .and_then(|id| find_session(self, id))
            .filter(|session| !session.phase.is_live())
        else {
            return DashboardAction::None;
        };
        let unavailable = matches!(session.kind, SessionKind::Agent { .. })
            && session
                .recovery
                .as_ref()
                .is_none_or(|recovery| recovery.unavailable.is_some());
        let command = if unavailable {
            Command::AgentResumeUnavailable(session.id)
        } else {
            Command::ReopenTerminal(session.id)
        };
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
…
        }
…
    }

[Showing lines 759-1103 of 6184. Use :1104 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:3004-3760"}
    pub(super) fn palette_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> Option<Vec<ClientMessage>> {
        if self
            .navigation
            .detached_palette_requests
            .remove(&request_id)
        {
            if let Response::Error { message, .. } = response {
                self.set_error(message);
            }
            return Some(Vec::new());
        }
        if self.palette.as_ref()?.suggestions.inspect == Some(request_id) {
            let mut palette = self.palette.take().unwrap();
            palette.suggestions.inspect = None;
            if let Response::Inventory { registry, .. } = response {
                palette.suggestions.repos = registry
                    .projects
                    .iter()
                    .map(|project| (project.name.clone(), project.repo.clone()))
                    .collect();
            }
            self.refresh_workspace_form(&mut palette);
            self.palette = Some(palette);
            return Some(Vec::new());
        }
        if self.palette.as_ref()?.pending != Some(request_id) {
            return None;
        }
        // Lifecycle jobs accept with Ok immediately; Provisional row takes over.
        if self
            .lifecycle_pending
            .as_ref()
            .is_some_and(|row| row.token == request_id)
        {
            match response {
                Response::Ok => {
                    self.accept_lifecycle_job(request_id);
                    return Some(Vec::new());
                }
                Response::Error { code, message }
                    if *code == crate::protocol::ErrorCode::Conflict
                        && message.contains("lifecycle job") =>
                {
                    let message = format!("{code:?}: {message}");
                    self.refuse_lifecycle_job(message.clone());
                    if let Some(palette) = self.palette.as_mut() {
                        palette.pending = None;
                        palette.error = Some(message);
                    }
                    return Some(Vec::new());
                }
                Response::Error { .. } => {
                    self.lifecycle_pending = None;
                }
                _ => {}
            }
        }
        let workspace_form = matches!(
            self.palette.as_ref()?.page,
            Page::Form {
                command: Command::CreateWorkspace,
                ..
            }
        );
        match response {
            Response::WorkflowCandidates(choices) => {
                let palette = self.palette.as_mut().unwrap();
                palette.pending = None;
                palette.workflows = Some(choices.candidates.clone());
                palette.page = Page::Search {
                    query: String::new(),
                    selected: 0,
                };
                palette.error = if choices.gaps.is_empty() {
                    choices.candidates.is_empty().then(|| "No workflow needs transfer to this Pi. Reconcile its existing workflows in Pi.".into())
                } else {
                    Some(format!(
                        "{} retained record(s) have missing or unverifiable recovery authority; their artifacts/receipts remain read-only and cannot be transferred. Available workflows are listed below.",
                        choices.gaps.len()
                    ))
                };
                return Some(Vec::new());
            }
            Response::Inventory { sessions, .. } if self.palette.as_ref()?.archive.is_some() => {
                let palette = self.palette.as_mut().unwrap();
                palette.pending = None;
                let mut rows: Vec<_> = sessions
                    .iter()
                    .filter(|row| row.archived)
                    .cloned()
                    .collect();
                rows.sort_by_key(|row| std::cmp::Reverse(row.id.0));
                palette.archive = Some(rows);
                return Some(Vec::new());
            }
            Response::Error { code, message } => {
                let code = code.clone();
                let message = message.clone();
                let mut palette = self.palette.take().unwrap();
                palette.pending = None;
                palette.error = Some(format!("{code:?}: {message}"));
                // A refused removal reopens as a forced one; only another
                // explicit Confirm sends it.
                let save_wip = match &palette.page {
                    Page::Confirm {
                        request:
                            Some(Request::RemoveWorkspace {
                                project,
                                name,
                                force: false,
                            }),
                        ..
                    } if code == crate::protocol::ErrorCode::DirtyWorktree => {
                        let branch = find_workspace(self, project, name)
                            .map(|workspace| workspace.name.clone())
                            .unwrap_or_default();
                        (git_ref_branch(&branch)).then(|| {
                            (
                                project.clone(),
                                name.clone(),
                                format!("Save uncommitted work to origin/wip/{branch}?"),
                            )
                        })
                    }
                    _ => None,
                };
                let forced = match &palette.page {
                    Page::Confirm {
                        request:
                            Some(Request::RemoveWorkspace {
                                project,
                                name,
                                force: false,
                            }),
                        ..
                    } if save_wip.is_none()
                        && matches!(
                            code,
                            crate::protocol::ErrorCode::SessionsRemain
                                | crate::protocol::ErrorCode::DirtyWorktree
                        ) =>
                    {
                        Some(Request::RemoveWorkspace {
                            project: project.clone(),
                            name: name.clone(),
                            force: true,
                        })
                    }
                    _ => None,
                };
                if let Some((project, name, target)) = save_wip {
                    palette.page = Page::Confirm {
                        request: Some(Request::SaveWorkspaceWip { project, name }),
                        target,
                    };
                }
                if let Some(request) = forced {
                    let display = match &request {
                        Request::RemoveWorkspace { project, name, .. } => {
                            find_workspace(self, project, name)
                                .map(|workspace| workspace_heading(self, workspace))
                                .unwrap_or_default()
                        }
                        _ => String::new(),
                    };
                    palette.page = removal_confirmation(request, &display);
                }
                let retry = match &palette.page {
                    Page::Confirm {
                        request: Some(Request::ReopenSession { session, .. }),
                        ..
                    } => Some((*session, Command::ReopenTerminal(*session))),
                    Page::Confirm {
                        request:
                            Some(Request::AcknowledgeSessionStopped {
                                session,
                                expected_run,
                            }),
                        ..
                    } => Some((
                        *session,
                        Command::AcknowledgeStopped(*session, *expected_run),
                    )),
                    _ => None,
                };
                if let Some((id, retry)) = retry {
                    if find_session(self, id).is_some() {
                        palette.page = self.command_page(retry);
                        palette.error = Some(format!("{code:?}: {message}"));
                    }
                } else if code == crate::protocol::ErrorCode::OwnershipUncertain {
                    palette.failed_launch = None;
                    palette.page = Page::Confirm {
                        request: None,
                        target: "Launch ownership is uncertain. No process-stop acknowledgement was sent. Close this notice, select the intended retained row, then use its Reopen or Acknowledge action after stopping old processes. Do not create another session.".into(),
                    };
                } else if code == crate::protocol::ErrorCode::PartialFailure
                    && matches!(
                        palette.page,
                        Page::Form {
                            command: Command::CreateWorkspace,
                            ..
                        }
                    )
                    && palette.failed_launch.is_some()
                {
                    let fields = vec![pick_field(
                        "Recovery",
                        "Retry".into(),
                        ["Retry", "Choose another agent", "Open shell"]
                            .into_iter()
                            .map(|s| PickItem {
                                label: s.into(),
                                value: s.into(),
                            })
                            .collect(),
                    )];
                    palette.page = Page::Form {
                        command: Command::RecoverLaunch,
                        fields,
                        active: 0,
                        name_edited: false,
                        root_edited: false,
                    };
                }
                self.palette = Some(palette);
                return Some(Vec::new());
            }
            Response::Ok if workspace_form => {
                self.palette.as_mut().unwrap().workspace_acknowledged = true;
                return Some(self.attach_created_workspace());
            }
            _ => {
                let mut palette = self.palette.take().unwrap();
                if let Response::Ok = response
                    && let Page::Confirm {
                        request: Some(Request::SaveWorkspaceWip { project, name }),
                        ..
                    } = &palette.page
                {
                    let project = project.clone();
                    let name = name.clone();
                    let display = find_workspace(self, &project, &name)
                        .map(|workspace| workspace_heading(self, workspace))
                        .unwrap_or_default();
                    let request = Request::RemoveWorkspace {
                        project,
                        name,
                        force: true,
                    };
                    let request_id = self.next_request_id();
                    palette.pending = Some(request_id);
                    palette.error = None;
                    palette.page = Page::Confirm {
                        request: Some(request.clone()),
                        target: format!("Saved to origin/wip and removing workspace {display}."),
                    };
                    self.palette = Some(palette);
                    return Some(vec![ClientMessage {
                        request_id,
                        request,
                    }]);
                }
                let answered_wip = matches!(
                    (&palette.page, response),
                    (
                        Page::Confirm {
                            request: Some(Request::AnswerWipSave { .. }),
                            ..
                        },
                        Response::Ok,
                    )
                );
                if let Some(request_id) = palette.suggestions.inspect {
                    self.ignored_responses.insert(request_id);
                }
                if let Response::CreatedSession(session) = response
                    && self.stale_created_session(session)
                {
                    return Some(Vec::new());
                }
                let preference = palette.launch_preference.take();
                self.dismiss_error_banner();
                if let Response::CreatedSession(session) = response {
                    self.select_session(session.id);
                    // The new pane's view goes first: the save waits on a
                    // synced write, and input waits on the view.
                    let request_id = self.next_request_id();
                    let mut outgoing: Vec<ClientMessage> = self
                        .view_request(self.outer_area, request_id)
                        .ok()
                        .flatten()
                        .into_iter()
                        .collect();
                    if let Some((project, choice)) = preference {
                        // The Server saves it; remembered here at once so the
                        // next launch offers it before the reading arrives.
                        let request = super::settings::launch_choice_request(&project, &choice);
                        self.settings.launch_choices.insert(project, choice);
                        outgoing.push(ClientMessage {
                            request_id: self.error_owning_request_id(),
                            request,
                        });
                    }
                    self.mode = InputMode::Terminal;
                    return Some(outgoing);
                }
                self.palette = None;
                if answered_wip {
                    self.wip_prompts.pop_front();
                }
                self.present_wip_prompt();
            }
        }
        Some(Vec::new())
    }

    pub(super) fn attach_created_workspace(&mut self) -> Vec<ClientMessage> {
        let Some(palette) = &self.palette else {
            return Vec::new();
        };
        if !palette.workspace_acknowledged {
            return Vec::new();
        }
        if !matches!(
            palette.page,
            Page::Form {
                command: Command::CreateWorkspace,
                ..
            }
        ) {
            return Vec::new();
        }
        let Some(id) = palette.workspace_id.clone() else {
            return Vec::new();
        };
        let project = match &palette.page {
            Page::Form { fields, .. } => fields
                .iter()
                .find(|field| field.label == "Project")
                .map(|field| field.value.trim().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let Some(workspace) = find_workspace(self, &project, &id).cloned() else {
            return Vec::new();
        };
        if workspace.sessions.is_empty() {
            let row = super::TreeRow::Workspace {
                project: workspace.project.clone(),
                id: workspace.id.clone(),
            };
            self.palette = None;
            self.dismiss_error_banner();
            self.select_container(row);
            let request_id = self.next_request_id();
            return self
                .view_request(self.outer_area, request_id)
                .ok()
                .flatten()
                .into_iter()
                .collect();
        }
        let session = workspace
            .sessions
            .iter()
            .find(|session| {
                session.name == "local" && session.phase == crate::session::SessionPhase::Running
            })
            .map(|session| session.id);
        // The workspace is in the hierarchy, so creation finished; the palette
        // must not stay on "Working…" just because its shell already exited.
        self.palette = None;
        self.dismiss_error_banner();
        let Some(session) = session else {
            return Vec::new();
        };
        self.select_session(session);
        let request_id = self.next_request_id();
        let outgoing = self
            .view_request(self.outer_area, request_id)
            .ok()
            .flatten()
            .into_iter()
            .collect();
        self.mode = InputMode::Terminal;
        outgoing
    }

    fn palette_body(&self, body: Rect) -> (Vec<Line<'_>>, Vec<(usize, PaletteTarget)>, usize) {
        let palette = self.palette.as_ref().unwrap();
        let mut lines = Vec::new();
        let mut focus_line = 0;
        let mut targets = Vec::new();
        match &palette.page {
            Page::Search { query, selected } => {
                targets.push((lines.len(), PaletteTarget::Search));
                let (visible, _) = palette
                    .cursor
                    .display(query, body.width.saturating_sub(8) as usize);
                lines.push(Line::from(format!("Search: {visible}")));
                let entries = self.palette_entries(
                    query,
                    palette.archive.as_deref(),
                    palette.agents.as_deref(),
                    palette.workflows.as_deref(),
                );
                if entries.is_empty() {
                    lines.push(Line::from(if palette.archive.is_some() {
                        "No matching archived sessions"
                    } else if palette.agents.is_some() {
                        if query.is_empty() {
                            "No other running agents"
                        } else {
                            "No matching agents"
                        }
                    } else {
                        "No matching actions or terminals"
                    }));
                }
                let selected = (*selected).min(entries.len().saturating_sub(1));
                let count = usize::from(body.height.saturating_sub(1) / 2).max(1);
                let start = selected.saturating_sub(count - 1);
                for (index, entry) in entries.iter().enumerate().skip(start).take(count) {
                    let available = usize::from(body.width.saturating_sub(2));
                    let label = if let Some(session) = self.entry_session(&entry.command, palette) {
                        let id = session.id;
                        let mut suffix = format!(" (#{})", id.0);
                        if palette.agents.is_some()
                            && available < 48
                            && let SessionKind::Agent { name } = &session.kind
                        {
                            let name_width = available
                                .saturating_sub(suffix.width() + 1)
                                .min(available / 2);
                            suffix.push_str(&format!(" {}", clip_text(name, name_width)));
                        }
                        let suffix_width = suffix.width();
                        if available >= suffix_width {
                            format!(
                                "{}{}",
                                clip_text(session.display_name(), available - suffix_width),
                                suffix
                            )
                        } else {
                            clip_text(&format!("#{}", id.0), available)
                        }
                    } else {
                        clip_text(&entry.label, available)
                    };
                    targets.push((lines.len(), PaletteTarget::Entry(index)));
                    lines.push(Line::styled(
                        format!("{} {}", if index == selected { "›" } else { " " }, label),
                        if index == selected {
                            Style::default().bg(MAUVE()).fg(CRUST())
                        } else {
                            Style::default().fg(TEXT())
                        },
                    ));
                    if let Command::SwitchAgent(id, run) = entry.command {
                        let reason = palette
                            .agents
                            .as_ref()
                            .and_then(|candidates| {
                                candidates
                                    .iter()
                                    .find(|c| c.session.id == id && c.session.run == run)
                            })
                            .and_then(|candidate| candidate.reason(self));
                        let detail = if let Some(reason) = reason {
                            reason.to_owned()
                        } else if let Some(session) = self.entry_session(&entry.command, palette) {
                            // ponytail: static picker spinner; pass draw time if animation is needed.
                            let status = super::status::SessionStatus::of(session, 0);
                            let agent = match &session.kind {
                                SessionKind::Agent { name } => name.as_str(),
                                SessionKind::Terminal => session.label.as_str(),
                            };
                            let mut activity = status
                                .unread
                                .unwrap_or_else(|| status.activity.trim().to_owned());
                            if let Some(health) = status
                                .reporting_health
                                .filter(|health| !activity.contains(health))
                            {
                                activity.push_str(&format!(" · {health}"));
                            }
                            if available < 48 {
                                status.compact
                            } else {
                                format!(
                                    "{} {} · {} · {}",
                                    status.glyph,
                                    activity,
                                    agent,
                                    session_workspace_heading(self, session)
                                )
                            }
                        } else {
                            String::new()
                        };
                        lines.push(Line::styled(
                            format!("  {detail}"),
                            Style::default().fg(SUBTEXT()),
                        ));
                    } else if let Command::Unarchive(id, _)
                    | Command::DeleteArchived(id, _, _)
                    | Command::AcknowledgeStopped(id, _) = &entry.command
                        && let Some(row) = palette
                            .archive
                            .as_ref()
                            .and_then(|rows| rows.iter().find(|row| row.id == *id))
                    {
                        lines.push(Line::styled(
                            format!("  {}", row.cwd.display()),
                            Style::default().fg(MUTED()),
                        ));
                    } else if let Some(hint) = self.command_hint(&entry.command) {
                        lines.push(Line::styled(
                            format!("  {} — {}", hint.key, hint.description),
                            Style::default().fg(MUTED()),
                        ));
                    } else {
                        lines.push(Line::from(""));
                    }
                }
            }
            Page::Form { fields, active, .. } => {
                for (index, field) in fields.iter().enumerate() {
                    if field.hidden {
                        continue;
                    }
                    if index == *active {
                        focus_line = lines.len() + 1;
                    }
                    targets.push((lines.len(), PaletteTarget::Field(index, false)));
                    lines.push(Line::styled(field.label, Style::default().fg(SUBTEXT())));
                    let pick_label;
                    let editing = match &field.kind {
                        FieldKind::Pick(list) if index == *active && !list.query.is_empty() => {
                            list.query.as_str()
                        }
                        FieldKind::Pick(list) => {
                            pick_label = list
                                .items
                                .iter()
                                .find(|item| item.value == field.value)
                                .map(|item| item.label.as_str())
                                .unwrap_or(field.value.as_str());
                            pick_label
                        }
                        _ => field.value.as_str(),
                    };
                    let cursor = if index == *active {
                        palette.cursor
                    } else {
                        Default::default()
                    };
                    let (mut text, _) =
                        cursor.display(editing, body.width.saturating_sub(2) as usize);
                    if index != *active {
                        text.pop();
                    }
                    targets.push((lines.len(), PaletteTarget::Field(index, true)));
                    lines.push(Line::styled(
                        format!("{} {text}", if index == *active { "›" } else { " " }),
                        Style::default().fg(if index == *active { MAUVE() } else { TEXT() }),
                    ));
                    if index == *active
                        && let FieldKind::Pick(list) = &field.kind
                    {
                        let (options, selected) = list.lines(8);
                        focus_line = lines.len() + selected;
                        let start = list.selected.saturating_sub(selected);
                        for offset in 0..options.len().min(list.filtered().len()) {
                            targets.push((
                                lines.len() + offset,
                                PaletteTarget::Option(index, start + offset),
                            ));
                        }
                        lines.extend(options);
                    }
                    if index == *active
                        && let FieldKind::Path(picker) = &field.kind
                        && let Some(listing) = picker.cached()
                    {
                        let count = 8.min(listing.entries.len());
                        let start = picker.selected.saturating_sub(count.saturating_sub(1));
                        for (offset, item) in
                            listing.entries.iter().enumerate().skip(start).take(count)
                        {
                            let chosen = offset == picker.selected;
                            targets.push((lines.len(), PaletteTarget::Option(index, offset)));
                            if chosen {
                                focus_line = lines.len();
                            }
                            lines.push(Line::styled(
                                format!("  {} {}", if chosen { "›" } else { " " }, item.label),
                                if chosen {
                                    Style::default().bg(MAUVE()).fg(CRUST())
                                } else {
                                    Style::default().fg(TEXT())
                                },
                            ));
                        }
                        if listing.omitted > 0 {
                            lines.push(Line::styled(
                                format!("  … {} more", listing.omitted),
                                Style::default().fg(MUTED()),
                            ));
                        }
                    }
                }
            }
            Page::Confirm { target, .. } => {
                lines.push(Line::from(target.clone()));
            }
        }
        let scroll = palette
            .scroll
            .unwrap_or_else(|| {
                focus_line.saturating_sub(usize::from(body.height.saturating_sub(1)))
            })
            .min(lines.len().saturating_sub(body.height as usize));
        (lines, targets, scroll)
    }

    fn close_confirm_text(&self, palette: &Palette) -> CloseConfirmText {
        let Page::Confirm { request, target } = &palette.page else {
            return CloseConfirmText {
                warning: None,
                identity: CLOSE_TITLE.to_string(),
                place: String::new(),
                project: String::new(),
                workspace: String::new(),
                error: palette.error.clone(),
                pending: palette.pending.is_some(),
            };
        };
        let Some(Request::CloseTerminal { session, .. }) = request else {
            return CloseConfirmText {
                warning: None,
                identity: CLOSE_TITLE.to_string(),
                place: target.clone(),
                project: String::new(),
                workspace: String::new(),
                error: palette.error.clone(),
                pending: palette.pending.is_some(),
            };
        };
        let Some(found) = find_session(self, *session) else {
            return CloseConfirmText {
                warning: None,
                identity: close_terminal_identity("Terminal", session.0),
                place: target.clone(),
                project: "unavailable".into(),
                workspace: "unavailable".into(),
                error: palette.error.clone(),
                pending: palette.pending.is_some(),
            };
        };
        let workspace = find_workspace(self, &found.project, &found.workspace)
            .map(|workspace| workspace_label(self, workspace))
            .unwrap_or_else(|| found.workspace.clone());
        let name = found.display_name();
        CloseConfirmText {
            warning: found.close_warning(),
            identity: close_terminal_identity(name, found.id.0),
            place: format!("{} / {name}", session_workspace_heading(self, found)),
            project: found.project.clone(),
            workspace,
            error: palette.error.clone(),
            pending: palette.pending.is_some(),
        }
    }

    fn close_confirm_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        let text = self.close_confirm_text(self.palette.as_ref().expect("close confirm is open"));
        let layout = measure_close_confirm(area, &text);
        let point = Position::new(mouse.column, mouse.row);
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) && layout.dialog.contains(point)
        {
            let max = usize::from(layout.body_rows).saturating_sub(usize::from(layout.text.height));
            let delta = if mouse.kind == MouseEventKind::ScrollUp {
                -1
            } else {
                1
            };
            let palette = self.palette.as_mut().expect("close confirm is open");
            let current = palette.scroll.unwrap_or(0);
            palette.scroll = Some(current.saturating_add_signed(delta).min(max));
            return DashboardAction::Redraw;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && layout.cancel.contains(point) {
            return self.palette_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && layout.confirm.contains(point) {
            return self
                .palette_key_with_submit(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), true);
        }
        // Outside clicks and chrome clicks stay on the overlay. They do not
        // reach the dashboard underneath, and they do not confirm.
        DashboardAction::None
    }

    fn draw_close_confirm(&self, frame: &mut Frame<'_>, palette: &Palette) {
        let text = self.close_confirm_text(palette);
        let layout = measure_close_confirm(frame.area(), &text);
        frame.render_widget(Clear, layout.dialog);
        frame.render_widget(
            Block::bordered()
                .border_style(Style::default().fg(MAUVE()))
                .style(Style::default().bg(CRUST()).fg(TEXT())),
            layout.dialog,
        );
        paint_rect(frame, layout.rail, Style::default().bg(PEACH()).fg(PEACH()));
        paint_close_confirm_title(frame, layout.dialog);
        let lines = close_confirm_lines(&text, layout.text.width);
        let max_scroll = lines.len().saturating_sub(usize::from(layout.text.height));
        let scroll = palette.scroll.unwrap_or(0).min(max_scroll);
        if layout.text.width > 0 && layout.text.height > 0 {
            frame.render_widget(
                Paragraph::new(lines).scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0)),
                layout.text,
            );
        }
        if layout.text.width > 0 && layout.cancel.height > 0 {
            frame.render_widget(
                Paragraph::new(close_confirm_action_line())
                    .alignment(Alignment::Right)
                    .style(Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD)),
                Rect::new(
                    layout.text.x,
                    layout.cancel.y,
                    layout.text.width,
                    layout.cancel.height,
                ),
            );
        }
    }

    pub(super) fn draw_palette(&self, frame: &mut Frame<'_>) {
        let Some(palette) = &self.palette else {
            return;
        };
        if close_terminal_id(&palette.page).is_some() {
            self.draw_close_confirm(frame, palette);
            return;
        }
        let (area, inner, body, buttons) = palette_geometry(frame.area());
        frame.render_widget(Clear, area);
        let title = match &palette.page {
            Page::Search { .. } if palette.agents.is_some() => "Agents",
            Page::Search { .. } if palette.archive.is_some() => "Archived sessions",
…
        };
…
    }

[Showing lines 3003-3763 of 6184. Use :3764 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:1104-1700"}
    pub(super) fn open_reopen_terminal(&mut self) -> DashboardAction {
…
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        self.palette = Some(Palette {
            page: self.command_page(command),
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_close_operator_for(
        &mut self,
        id: ovrcr_protocol::OperatorConversationId,
    ) -> DashboardAction {
        let Some((session, run, title, busy, draft)) = self.operator_close_target(id) else {
            return DashboardAction::None;
        };
        self.whichkey = None;
        self.palette = Some(Palette {
            page: Page::Confirm {
                request: Some(Request::Operator(ovrcr_protocol::OperatorRequest::Close {
                    conversation: id,
                    expected_run: run,
                })),
                target: format!(
                    "Close Operator {} (#{} / session {} / run {}). {}Archive its history.{}",
                    title,
                    id.0,
                    session.0,
                    run.0,
                    if busy {
                        "Stop its live or unconfirmed work, then "
                    } else {
                        ""
                    },
                    if draft {
                        " The draft will be discarded."
                    } else {
                        ""
                    }
                ),
            },
            ..Palette::new()
        });
        DashboardAction::Redraw
    }

    pub(super) fn dismiss_close_confirm_for(&mut self, session: SessionId) {
        let matches_close = matches!(
            self.palette.as_ref().map(|palette| &palette.page),
            Some(Page::Confirm {
                request: Some(Request::CloseTerminal { session: id, .. }),
                ..
            }) if *id == session
        );
        if matches_close {
            self.palette = None;
        }
    }

    pub(super) fn sync_palette_workspace_identity(&mut self) {
        let removal = match &self.palette {
            Some(palette) if palette.pending.is_some() => None,
            Some(Palette {
                page:
                    Page::Confirm {
                        request:
                            Some(Request::RemoveWorkspace {
                                project,
                                name,
                                force,
                            }),
                        target,
                    },
                ..
            }) => Some((project.clone(), name.clone(), *force, target.clone())),
            _ => None,
        };
        if let Some((project, name, force, target)) = removal {
            let stale = match find_workspace(self, &project, &name) {
                None => true,
                Some(workspace) => {
                    let heading = workspace_heading(self, workspace);
                    let expected = removal_confirmation(
                        Request::RemoveWorkspace {
                            project: project.clone(),
                            name: name.clone(),
                            force,
                        },
                        &heading,
                    );
                    workspace.root
                        || !matches!(expected, Page::Confirm { target: current, .. } if current == target)
                }
            };
            if stale {
                let page = self.palette_form(Command::RemoveWorkspace);
                if let Some(palette) = self.palette.as_mut() {
                    palette.page = page;
                    palette.error = None;
                }
                return;
            }
        }
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let Page::Form {
            command, fields, ..
        } = &palette.page
        else {
            return;
        };
        if !matches!(command, Command::CreateTerminal | Command::RemoveWorkspace) {
            return;
        }
        let include_root = !matches!(command, Command::RemoveWorkspace);
        let Some(field) = fields.iter().find(|field| field.label == "Workspace") else {
            return;
        };
        let FieldKind::Pick(list) = &field.kind else {
            return;
        };
        let selected = list
            .accepted()
            .map(|item| item.value.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| field.value.clone());
        let query = list.query.clone();
        let (project, id) = split_workspace_pick(&selected);
        let mut list = PickList::workspaces(&self.hierarchy, &project, &id, include_root);
        // Keep the typed filter; never copy the selected branch label into query.
        list.query = query;
        let filtered = list.filtered();
        // An out-of-range selection cannot be accepted. A branch handoff must
        // require a new user selection, not silently choose the first match.
        list.selected = filtered
            .iter()
            .position(|item| item.value == selected)
            .unwrap_or(filtered.len());
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        let Page::Form { fields, .. } = &mut palette.page else {
            return;
        };
        let Some(field) = fields.iter_mut().find(|field| field.label == "Workspace") else {
            return;
        };
        if !selected.is_empty() {
            field.value = selected;
        }
        field.kind = FieldKind::Pick(list);
    }

    pub(super) fn open_forced_remove_workspace(
        &mut self,
        project: String,
        name: String,
        code: crate::protocol::ErrorCode,
        message: String,
    ) {
        let display = find_workspace(self, &project, &name)
            .map(|workspace| workspace_heading(self, workspace))
            .unwrap_or_else(|| format!("{project}/{name}"));
        let request = Request::RemoveWorkspace {
            project,
            name,
            force: true,
        };
        self.cancel_mouse_gesture();
        self.whichkey = None;
        self.palette = Some(Palette {
            page: removal_confirmation(request, &display),
            error: Some(format!("{code:?}: {message}")),
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
    }

    pub(super) fn open_remove_workspace(&mut self, project: String, id: String) -> DashboardAction {
        let Some(workspace) = find_workspace(self, &project, &id) else {
            return DashboardAction::None;
        };
        if workspace.root {
            self.set_error(super::keymap::ROOT_PROTECTED);
            return DashboardAction::Redraw;
        }
        let previous = self.selected_container.clone();
        self.selected_container = Some(super::TreeRow::Workspace { project, id });
        let action = self.open_remove_context(true);
        self.selected_container = previous;
        action
    }

    pub(super) fn open_remove_context(&mut self, workspace: bool) -> DashboardAction {
        let (project, name) = self.creation_context();
        if project.is_empty() || (workspace && name.is_empty()) {
            return DashboardAction::None;
        }
        if workspace
            && self
                .current_workspace()
                .is_some_and(|workspace| workspace.root)
        {
            self.set_error(super::keymap::ROOT_PROTECTED);
            return DashboardAction::Redraw;
        }
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        self.palette = Some(Palette {
            page: self.palette_form(if workspace {
                Command::RemoveWorkspace
            } else {
                Command::RemoveProject
            }),
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }
    fn command_page(&self, command: Command) -> Page {
        match command {
            Command::StartNewConversation(id) => {
                let session = find_session(self, id).expect("new conversation target exists");
                if let Some(warning) = find_workspace(self, &session.project, &session.workspace)
                    .and_then(|workspace| workspace.warning.clone())
                {
                    return Page::Confirm {
                        request: None,
                        target: warning,
                    };
                }
                let mut page = self.palette_form(Command::CreateTerminal);
                if let Page::Form { fields, .. } = &mut page {
                    for field in fields.iter_mut() {
                        match field.label {
                            "Start" => field.value = "Agent".into(),
                            "Workspace" => {
                                field.value = super::picker::workspace_pick_value(
                                    &session.project,
                                    &session.workspace,
                                )
                            }
                            "Agent" => {
                                if let SessionKind::Agent { name } = &session.kind {
                                    field.value = name.clone();
                                }
                            }
                            _ => {}
                        }
                        if let FieldKind::Pick(list) = &mut field.kind {
                            list.query.clear();
                            list.select_value(&field.value);
                        }
                    }
                    self.refresh_terminal_form(fields, false);
                }
                page
            }
            Command::ReopenTerminal(id) => {
                let session = find_session(self, id).expect("reopen target exists");
                if let Some(warning) = find_workspace(self, &session.project, &session.workspace)
                    .and_then(|workspace| workspace.warning.clone())
                {
                    return Page::Confirm {
                        request: None,
                        target: warning,
                    };
                }
                let acknowledge_stopped = session
                    .recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.requires_ack);
                let target = if matches!(session.kind, SessionKind::Agent { .. }) {
                    if session.recovery.as_ref().is_none_or(|recovery| recovery.conversation.is_none()) {
                        if acknowledge_stopped {
                            "Confirm the previous agent and background processes have stopped, then open the agent's native resume picker?"
                        } else {
                            "Open the agent's native resume picker?"
                        }
                    } else if acknowledge_stopped {
                        "Confirm the previous agent and background processes have stopped, then resume this exact conversation without a new prompt?"
                    } else {
                        "Resume this exact conversation without a new prompt?"
                    }
                } else if acknowledge_stopped {
                    "Previous agent or background processes may still be running. Confirm they have stopped, then reopen this session in a fresh shell?"
                } else {
                    "Reopen this session in a fresh shell?"
                };
                Page::Confirm {
                    request: Some(Request::ReopenSession {
                        session: id,
                        expected_run: session.run,
                        acknowledge_stopped,
                    }),
                    target: target.into(),
                }
            }
            // Carries the run so archived rows, absent from the hierarchy, can be acknowledged too.
            Command::AcknowledgeStopped(id, run) => Page::Confirm {
                request: Some(Request::AcknowledgeSessionStopped {
                    session: id,
                    expected_run: run,
                }),
                target: "Acknowledge that previous agent or background processes have stopped, without launching a replacement?".into(),
            },
            Command::AgentResumeUnavailable(id) => Page::Confirm {
                request: None,
                target: find_session(self, id)
                    .and_then(|session| session.recovery.as_ref())
                    .and_then(|recovery| recovery.unavailable.clone())
                    .unwrap_or_else(|| {
                        "Agent resume is unavailable. No certified recovery reference is available."
                            .into()
                    }),
            },
            Command::CloseTerminal(id) => {
                let session = find_session(self, id).expect("close target exists");
                Page::Confirm {
                    request: Some(Request::CloseTerminal {
                        session: id,
                        expected_run: session.run,
                    }),
                    target: format!(
                        "Close {} / {} (#{}). Stop its currently owned processes and archive its record.",
                        session_workspace_heading(self, session),
                        session.display_name(),
                        id.0
                    ),
                }
            }
            command => self.palette_form(command),
        }
    }

    /// The key binding a palette entry duplicates, so the list can print its key
    /// and its reason instead of restating them.
    fn command_hint(&self, command: &Command) -> Option<KeyBinding> {
        let action = match command {
            Command::CreateTerminal => Action::CreateTerminal,
            Command::CreateWorkspace => Action::CreateWorkspace,
            Command::RegisterProject => Action::RegisterProject,
            Command::CloseTerminal(_) => Action::CloseTerminal,
            Command::Hint(action) => *action,
            _ => return None,
        };
        keymap(self)
            .into_iter()
            .flat_map(|g| g.keys)
            .find(|binding| binding.action == action)
    }

    pub(super) fn palette_paste(&mut self, text: &str) -> DashboardAction {
        let mut palette = self.palette.take().unwrap();
        palette.insert(text);
        self.refresh_workspace_form(&mut palette);
        self.palette = Some(palette);
        self.refresh_active_path_listing();
        DashboardAction::Redraw
    }

    fn palette_entries(
        &self,
        query: &str,
        archive: Option<&[crate::protocol::SessionSummary]>,
        agents: Option<&[AgentCandidate]>,
        workflows: Option<&[crate::protocol::WorkflowTransfer]>,
    ) -> Vec<Entry> {
        if let Some(transfers) = workflows {
            let query = query.to_lowercase();
            return transfers
                .iter()
                .filter_map(|transfer| {
                    let label = format!(
                        "Recover workflow {} from parent #{} epoch {}",
                        transfer.expected.workflow,
                        transfer.expected.parent.0,
                        transfer.expected.epoch
                    );
                    query
                        .split_whitespace()
                        .all(|word| label.to_lowercase().contains(word))
                        .then_some(Entry {
                            label,
                            command: Command::TransferWorkflow(Box::new(transfer.clone())),
                        })
                })
                .collect();
        }
        if let Some(candidates) = agents {
            let query = query.to_lowercase();
            return candidates
                .iter()
                .filter_map(|candidate| {
                    let label = candidate.search_identity.clone();
                    query
                        .split_whitespace()
                        .all(|word| label.to_lowercase().contains(word))
                        .then_some(Entry {
                            label,
                            command: Command::SwitchAgent(
                                candidate.session.id,
                                candidate.session.run,
                            ),
                        })
                })
                .collect();
        }
        if let Some(rows) = archive {
            let query = query.to_lowercase();
            return rows
                .iter()
                .filter(|row| {
                    let place = session_workspace_heading(self, row);
                    let label = format!(
                        "{} {} {} {} {} {}",
                        row.display_name(),
                        place,
                        row.project,
                        row.workspace,
                        row.cwd.display(),
                        if matches!(
                            row.scope,
                            ovrcr_protocol::SessionScope::OperatorConversation { .. }
                        ) {
                            "Operator"
                        } else {
                            ""
                        }
                    )
                    .to_lowercase();
                    query.split_whitespace().all(|word| label.contains(word))
                })
                .flat_map(|row| {
                    let operator = matches!(
                        row.scope,
                        ovrcr_protocol::SessionScope::OperatorConversation { .. }
                    );
                    let label = if operator {
                        format!("Operator / {} (#{})", row.display_name(), row.id.0)
                    } else {
                        format!(
                            "{} / {} (#{})",
                            session_workspace_heading(self, row),
                            row.display_name(),
                            row.id.0
                        )
                    };
                    let mut entries = vec![Entry {
                        label: format!("Unarchive: {label}"),
                        command: Command::Unarchive(row.id, row.run),
                    }];
                    if !operator {
                        entries.push(Entry {
                            label: format!("Delete record: {label}"),
                            command: Command::DeleteArchived(
                                row.id,
                                row.run,
                                row.display_name().to_owned(),
                            ),
                        });
                    }
                    // Filing a row keeps its ownership uncertainty, which blocks
                    // workspace removal; the archive must offer the acknowledgement.
                    if row
                        .recovery
                        .as_ref()
                        .is_some_and(|recovery| recovery.requires_ack)
                    {
                        entries.push(Entry {
                            label: format!("Acknowledge stopped: {label}"),
                            command: Command::AcknowledgeStopped(row.id, row.run),
                        });
                    }
                    entries
                })
                .collect();
        }
        let mut entries = vec![
            Entry {
                label: "Archived sessions".into(),
                command: Command::Archives,
            },
            Entry {
                label: "Create terminal (n)".into(),
                command: Command::CreateTerminal,
            },
            Entry {
                label: "Create workspace (w)".into(),
                command: Command::CreateWorkspace,
            },
            Entry {
                label: "Register project (a)".into(),
                command: Command::RegisterProject,
            },
        ];
        if let Some(id) = self
            .action_session()
            .filter(|id| find_session(self, *id).is_some())
        {
            if let Some(session) = find_session(self, id).filter(|session| {
                session.phase.is_live()
                    && matches!(&session.kind, SessionKind::Agent { name } if name == "pi")
            }) {
                entries.push(Entry {
                    label: "Recover workflow control in this Pi".into(),
                    command: Command::WorkflowRecovery(id, session.run),
                });
            }
            entries.push(Entry {
                label: "Rename terminal (blank = original name)".into(),
                command: Command::RenameTerminal(id),
            });
            if let Some(session) = find_session(self, id).filter(|session| !session.phase.is_live())
            {
                match &session.kind {
                    SessionKind::Agent { .. } => {
                        let available = session
                            .recovery
                            .as_ref()
                            .is_some_and(|recovery| recovery.unavailable.is_none());
                        entries.push(if available {
                            Entry {
                                label: if session
                                    .recovery
                                    .as_ref()
                                    .is_some_and(|recovery| recovery.failure.is_some())
                                {
                                    "Retry resume conversation"
                                } else {
                                    "Resume conversation"
                                }
                                .into(),
                                command: Command::ReopenTerminal(id),
                            }
                        } else {
                            Entry {
                                label: "Agent resume is unavailable".into(),
                                command: Command::AgentResumeUnavailable(id),
                            }
                        });
                        entries.push(Entry {
                            label: "Start new conversation in a separate session".into(),
                            command: Command::StartNewConversation(id),
                        });
                        if session
                            .recovery
                            .as_ref()
                            .is_some_and(|recovery| recovery.requires_ack)
                        {
                            entries.push(Entry {
                                label: "Acknowledge stopped processes".into(),
                                command: Command::AcknowledgeStopped(id, session.run),
                            });
                        }
                    }
                    SessionKind::Terminal => {
                        entries.push(Entry {
                            label: "Reopen in a fresh shell".into(),
                            command: Command::ReopenTerminal(id),
                        });
                        if session
                            .recovery
                            .as_ref()
                            .is_some_and(|recovery| recovery.requires_ack)
                        {
                            entries.push(Entry {
                                label: "Acknowledge stopped without reopening".into(),
                                command: Command::AcknowledgeStopped(id, session.run),
                            });
                        }
                    }
                }
            }
            entries.push(Entry {
                label: "Close terminal".into(),
                command: Command::CloseTerminal(id),
            });
        }
        entries.push(Entry {
            label: "Remove workspace".into(),
            command: Command::RemoveWorkspace,
        });
        entries.push(Entry {
            label: "Remove project".into(),
            command: Command::RemoveProject,
        });
        #[cfg(target_os = "macos")]
        entries.push(Entry {
            label: "Set up iTerm focus".into(),
            command: Command::ITermSetup,
        });
        entries.push(Entry {
            label: "Settings".into(),
…
        });
…
    }

[Showing lines 1103-1703 of 6184. Use :1704 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs", "pattern": "fn palette_key|Command::CreateWorkspace =>|Command::CreateTerminal =>|CreateWorkspaceWithLaunch|CreateSession \\{|submit_when_ready|fn submit_form"}
 154|    // Some(true) retains explicit whole-form Submit; Some(false) replays Enter.
*155|    submit_when_ready: Option<bool>,
 156|    cursor: super::text_cursor::TextCursor,
 157|    scroll: Option<usize>,
 158|}
...
 532|            suggestions: Suggestions::default(),
*533|            submit_when_ready: None,
 534|            cursor: Default::default(),
 535|            scroll: None,
 536|        }
...
 539|    fn insert(&mut self, text: &str) {
*540|        if self.pending.is_some() || self.submit_when_ready.is_some() {
 541|            return;
 542|        }
 543|        self.scroll = None;
...
 811|            suggestions: Suggestions::default(),
*812|            submit_when_ready: None,
 813|            cursor: Default::default(),
 814|            scroll: None,
 815|        });
...
 834|            let project = match command {
*835|                Command::CreateTerminal => fields
 836|                    .iter()
 837|                    .find(|field| field.label == "Workspace")
 838|                    .map(|field| split_workspace_pick(&field.value).0),
*839|                Command::CreateWorkspace => fields
 840|                    .iter()
 841|                    .find(|field| field.label == "Project")
 842|                    .map(|field| field.value.clone()),
...
 990|        };
*991|        let mut submit = palette.submit_when_ready.filter(|_| ready);
 992|        let mut refused = false;
 993|        if submit.is_some() {
*994|            palette.submit_when_ready = None;
 995|            // The hints arrived after the user pressed Enter, so they never saw
 996|            // this list. Make them confirm anything but what they typed.
 997|            if let Some(label) = unconfirmed_pick(&palette.page) {
...
 1448|        let action = match command {
*1449|            Command::CreateTerminal => Action::CreateTerminal,
*1450|            Command::CreateWorkspace => Action::CreateWorkspace,
 1451|            Command::RegisterProject => Action::RegisterProject,
 1452|            Command::CloseTerminal(_) => Action::CloseTerminal,
 1453|            Command::Hint(action) => *action,
...
 1790|                && palette.pending.is_none()
*1791|                && palette.submit_when_ready.is_none()
 1792|            {
 1793|                return self.palette_key_with_submit(
 1794|                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
...
 1799|        }
*1800|        if palette.pending.is_some() || palette.submit_when_ready.is_some() || !body.contains(point)
 1801|        {
 1802|            return DashboardAction::None;
 1803|        }
...
 1912|
*1913|    pub(super) fn palette_key(&mut self, key: KeyEvent) -> DashboardAction {
 1914|        self.palette_key_with_submit(key, false)
 1915|    }
 1916|
*1917|    fn palette_key_with_submit(&mut self, key: KeyEvent, submit: bool) -> DashboardAction {
 1918|        let action = self.palette_key_inner(key, submit, true);
 1919|        self.refresh_active_path_listing();
 1920|        action
...
 1922|
*1923|    fn palette_key_inner(
 1924|        &mut self,
 1925|        key: KeyEvent,
 1926|        submit: bool,
...
 1961|        // Other keys wait for the running request so a submit cannot repeat.
*1962|        if palette.pending.is_some() || palette.submit_when_ready.is_some() {
 1963|            self.palette = Some(palette);
 1964|            return DashboardAction::Redraw;
 1965|        }
...
 2359|                        }
*2360|                        palette.submit_when_ready = None;
 2361|                        self.palette = Some(palette);
 2362|                        return action;
 2363|                    }
...
 2435|                        {
*2436|                            palette.submit_when_ready = Some(submit);
 2437|                            self.palette = Some(palette);
 2438|                            return action;
 2439|                        }
...
 2464|                        let request = match command {
*2465|                            Command::CreateTerminal => {
 2466|                                let (project, id) = split_workspace_pick(&field("Workspace"));
 2467|                                let Some(workspace) = find_workspace(self, &project, &id) else {
 2468|                                    palette.error =
...
 2493|                            },
*2494|                            Command::CreateWorkspace => {
 2495|                                let id = if let Some(id) = palette.workspace_id.clone() {
 2496|                                    id
 2497|                                } else {
...
 2511|                                };
*2512|                                Request::CreateWorkspaceWithLaunch {
 2513|                                    project: field("Project"),
 2514|                                    id,
 2515|                                    launch: selected_launch.clone(),
...
 2837|        let launch = match &request {
*2838|            Request::CreateWorkspaceWithLaunch {
 2839|                project,
 2840|                id: name,
 2841|                launch: Some(launch),
...
 2888|        let fields = match command {
*2889|            Command::CreateTerminal => {
 2890|                let workspace_list =
 2891|                    PickList::workspaces(&self.hierarchy, &project, &workspace, true);
 2892|                let workspace_value = workspace_list
...
 2926|            )],
*2927|            Command::CreateWorkspace => {
 2928|                let list = PickList::projects(&self.hierarchy, &project);
 2929|                let mut project = text(
 2930|                    "Project",
...
 3765|            Page::Form { command, .. } => match command {
*3766|                Command::CreateTerminal => "Create terminal",
*3767|                Command::CreateWorkspace => "Create workspace",
 3768|                Command::RenameTerminal(_) => "Rename terminal · blank = original name",
 3769|                Command::RecoverLaunch => "Workspace retained · recover launch",
 3770|                Command::RegisterProject => "Register project",
...
 3871|            error
*3872|        } else if palette.submit_when_ready.is_some() {
 3873|            "Waiting for Git suggestions before creating… · Esc cancel"
 3874|        } else if matches!(&palette.page, Page::Form { fields, .. } if fields.iter().any(|f| f.label == "Start" && f.value == "Agent") && self.selected_launch(fields).is_err())
 3875|        {
...
 4407|            requests[0].request,
*4408|            Request::CreateWorkspaceWithLaunch { .. }
 4409|        ));
 4410|        assert!(matches!(
 4411|            requests[1].request,
...
 4463|        assert!(
*4464|            matches!(message.request, Request::CreateWorkspaceWithLaunch { project, id, branch: BranchRequest::New { branch, base, .. }, .. } if project == "demo" && id.len() == 32 && branch.ends_with("deferred") && base == "develop")
 4465|        );
 4466|        d.poll_palette();
 4467|        assert!(
...
 4723|                }
*4724|                Request::CreateWorkspaceWithLaunch {
 4725|                    launch: Some(launch),
 4726|                    ..
 4727|                } => {
...
 4791|            let m = request(&mut d);
*4792|            let Request::CreateWorkspaceWithLaunch {
 4793|                project,
 4794|                id: name,
 4795|                launch,
...
 5151|            request(&mut d).request,
*5152|            Request::CreateWorkspaceWithLaunch {
 5153|                launch: Some(SessionLaunch { label: None, .. }),
 5154|                ..
 5155|            }

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:2230-2560"}
                Page::Search { query, selected } => {
…
                    if let Some(entry) = entries.get(*selected) {
                        match entry.command.clone() {
…
                            Command::SwitchAgent(id, run) => {
…
                                if let Some(reason) = candidate.and_then(|c| c.reason(self)) {
…
                                } else if candidate.is_some() {
                                    return self.confirm_agent_switch(id, run);
                                }
                            }
                            Command::Switch(id) => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                self.select_session(id);
                                return self.request_selected();
                            }
                            #[cfg(target_os = "macos")]
                            Command::ITermSetup => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.begin_iterm_setup();
                            }
                            Command::Settings => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.open_details(super::quota::Details::Settings);
                            }
                            Command::Events => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.open_events();
                            }
                            command @ (Command::RefreshQuota | Command::EnableQuota) => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.quota_request(match command {
                                    Command::RefreshQuota => {
                                        Request::RefreshQuota { provider: None }
                                    }
                                    _ => Request::SetSetting {
                                        path: "quota.enabled".into(),
                                        value: Some("true".into()),
                                    },
                                });
                            }
                            Command::Hint(action) => {
                                if self
                                    .command_hint(&entry.command)
                                    .is_some_and(|h| h.enabled())
                                {
                                    if action == Action::OperatorQuick {
                                        self.palette = Some(palette);
                                        return self.run(action);
                                    }
                                    if let Some(request_id) = palette.suggestions.inspect {
                                        self.ignored_responses.insert(request_id);
                                    }
                                    return self.run(action);
                                }
                            }
                            Command::Archives => {
                                palette.archive = Some(Vec::new());
                                palette.page = Page::Search {
                                    query: String::new(),
                                    selected: 0,
                                };
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::Inspect,
                                    history_before_request,
                                );
                            }
                            Command::Unarchive(session, expected_run) => {
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::UnarchiveSession {
                                        session,
                                        expected_run,
                                    },
                                    history_before_request,
                                );
                            }
                            Command::DeleteArchived(session, expected_run, title) => {
                                palette.page = Page::Confirm {
                                    request: Some(Request::DeleteArchivedSession {
                                        session,
                                        expected_run,
                                    }),
                                    target: format!(
                                        "Delete record {}? Provider history files will be kept.",
                                        title
                                    ),
                                };
                            }
                            Command::CloseTerminal(id)
                                if find_session(self, id).is_some_and(|s| {
                                    !s.phase.is_live() && s.close_warning().is_none()
                                }) =>
                            {
                                let run = find_session(self, id).unwrap().run;
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::CloseTerminal {
                                        session: id,
                                        expected_run: run,
                                    },
                                    history_before_request,
                                );
                            }
                            command => {
                                palette.page = self.command_page(command.clone());
                                if matches!(command, Command::CreateWorkspace) {
                                    let _ = ensure_workspace_id(&mut palette);
                                }
                            }
                        }
                    }
                }
                Page::Form {
                    command,
                    fields,
                    active,
                    name_edited,
                    root_edited,
                } => {
                    if !accept_pick(&mut fields[*active]) {
                        let field = &fields[*active];
                        if field.required {
                            // A deferred submit must not wait on a pick that can
                            // never resolve; say which field went unmatched.
                            palette.error =
                                Some(format!("{} not found in available choices", field.label));
                        }
                        palette.submit_when_ready = None;
                        self.palette = Some(palette);
                        return action;
                    }
                    if matches!(command, Command::RecoverLaunch)
                        && fields.first().is_some_and(|f| f.label == "Recovery")
                    {
                        let mut previous = palette.failed_launch.as_ref().unwrap().clone();
                        if !self
                            .hierarchy
                            .projects
                            .iter()
                            .find(|p| p.name == previous.project)
                            .is_some_and(|p| {
                                p.workspaces.iter().any(|w| w.id == previous.workspace)
                            })
                        {
                            palette.error = Some("Workspace not yet registered; wait for the hierarchy update before retrying.".into());
                            self.palette = Some(palette);
                            return action;
                        }
                        match fields[0].value.as_str() {
                            "Choose another agent" => {
                                *fields = self.launch_fields(&previous.project, false);
                                fields[0] = pick_field(
                                    "Start",
                                    "Agent".into(),
                                    ["Agent", "Terminal"]
                                        .into_iter()
                                        .map(|s| PickItem {
                                            label: s.into(),
                                            value: s.into(),
                                        })
                                        .collect(),
                                );
                                self.refresh_terminal_form(fields, false);
                                *active = 1;
                                palette.error = None;
                                self.palette = Some(palette);
                                return action;
                            }
                            "Open shell" => {
                                previous.argv = self.shell_argv();
                                previous.label = None;
                            }
                            _ => {}
                        }
                        action = self.palette_submit(
                            &mut palette,
                            Request::CreateSession(previous),
                            history_before_request,
                        );
                        self.palette = Some(palette);
                        return action;
                    }
                    self.refresh_terminal_form(fields, *name_edited);
                    self.refresh_project_form(fields, *name_edited, *root_edited);
                    let visible = visible_indices(fields);
                    let last = submit
                        || visible.last().copied() == Some(*active)
                        || (matches!(command, Command::CreateWorkspace)
                            && fields
                                .get(*active)
                                .is_some_and(|field| field.label == "Branch"));
                    if !last {
                        move_form_field(fields, active, false);
                    } else if let Some(index) = fields
                        .iter()
                        .position(|f| !f.hidden && f.required && f.value.trim().is_empty())
                    {
                        *active = index;
                        palette.error = Some(format!("{} is required", fields[index].label));
                    } else {
                        if matches!(command, Command::CreateWorkspace)
                            && !palette.suggestions.cache.contains_key(&fields[0].value)
                        {
                            palette.submit_when_ready = Some(submit);
                            self.palette = Some(palette);
                            return action;
                        }
                        let field = |label: &str| {
                            fields
                                .iter()
                                .find(|f| f.label == label)
                                .map(|f| f.value.trim().to_string())
                                .unwrap_or_default()
                        };
                        let selected_launch = if matches!(
                            command,
                            Command::CreateTerminal
                                | Command::CreateWorkspace
                                | Command::RecoverLaunch
                        ) {
                            match self.selected_launch(fields) {
                                Ok(launch) => launch,
                                Err(error) => {
                                    palette.error = Some(error);
                                    self.palette = Some(palette);
                                    return action;
                                }
                            }
                        } else {
                            None
                        };
                        let request = match command {
                            Command::CreateTerminal => {
                                let (project, id) = split_workspace_pick(&field("Workspace"));
                                let Some(workspace) = find_workspace(self, &project, &id) else {
                                    palette.error =
                                        Some("Workspace not found in available choices".into());
                                    self.palette = Some(palette);
                                    return action;
                                };
                                if let Some(warning) = &workspace.warning {
                                    palette.error = Some(warning.clone());
                                    self.palette = Some(palette);
                                    return action;
                                }
                                Request::CreateSession(CreateSessionRequest {
                                    project: workspace.project.clone(),
                                    workspace: workspace.id.clone(),
                                    name: field("Name"),
                                    argv: selected_launch.as_ref().unwrap().argv.clone(),
                                    label: selected_launch.as_ref().unwrap().label.clone(),
                                    kind: selected_launch.as_ref().unwrap().kind.clone(),
                                })
                            }
                            Command::RenameTerminal(session) => Request::SetSessionTitle {
                                session: *session,
                                title: {
                                    let name = field("Name");
                                    (!name.is_empty()).then_some(name)
                                },
                            },
                            Command::CreateWorkspace => {
                                let id = if let Some(id) = palette.workspace_id.clone() {
                                    id
                                } else {
                                    match new_workspace_id() {
                                        Ok(id) => {
                                            palette.workspace_id = Some(id.clone());
                                            id
                                        }
                                        Err(error) => {
                                            palette.error = Some(format!(
                                                "Could not allocate workspace identity: {error}"
                                            ));
                                            self.palette = Some(palette);
                                            return action;
                                        }
                                    }
                                };
                                Request::CreateWorkspaceWithLaunch {
                                    project: field("Project"),
                                    id,
                                    launch: selected_launch.clone(),
                                    branch: if field("Branch mode") == "existing" {
                                        BranchRequest::Existing {
                                            branch: field("Branch"),
                                        }
                                    } else {
                                        BranchRequest::New {
                                            branch: field("Branch"),
                                            base: field("Base"),
                                        }
                                    },
                                }
                            }
                            Command::RecoverLaunch => {
                                let previous = palette.failed_launch.as_ref().unwrap();
                                let registered =
                                    find_workspace(self, &previous.project, &previous.workspace)
                                        .is_some();
                                if !registered {
                                    palette.error = Some("Workspace is not yet registered in the dashboard. Wait for the hierarchy update before retrying.".into());
                                    self.palette = Some(palette);
                                    return action;
                                }
                                Request::CreateSession(CreateSessionRequest {
                                    project: previous.project.clone(),
                                    workspace: previous.workspace.clone(),
                                    name: String::new(),
                                    argv: selected_launch.as_ref().unwrap().argv.clone(),
                                    label: selected_launch.as_ref().unwrap().label.clone(),
                                    kind: selected_launch.as_ref().unwrap().kind.clone(),
                                })
                            }
                            Command::RegisterProject => Request::AddProject {
                                name: field("Name"),
                                repo: expand_path(&field("Repository")),
                                workspace_root: expand_path(&field("Workspace root")),
                            },
                            Command::RemoveWorkspace => {
                                let (project, id) = split_workspace_pick(&field("Workspace"));
                                let Some(workspace) = find_workspace(self, &project, &id) else {
                                    palette.error =
                                        Some("Workspace not found in available choices".into());
                                    self.palette = Some(palette);
                                    return action;
                                };
                                if workspace.root {
                                    palette.error = Some(super::keymap::ROOT_PROTECTED.to_string());
                                    self.palette = Some(palette);
                                    return action;
                                }
…
                            }
…
                        };
…
                    }
                }

[Showing lines 2229-2563 of 6184. Use :2564 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/keymap.rs"}
//! The dashboard's key bindings. One entry per key in an input mode carries the
//! label, the description, the reason the key is unavailable, and the action to
//! run. `key_action`, the footer, the key popup and the palette all read this
//! table, so a hint can never disagree with what the key does.

use super::state::{find_session, workspace_heading};
…
use ovrcr_protocol::{SessionKind, SessionPhase};

/// What a key does. Dispatch, the key popup and the palette run these directly,
/// without synthesising a key event to reach one another. `Capture` is the one
/// exception, and says why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action { … }

/// Where the key popup's group shows a binding, and under which key there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GroupSlot { … }

/// The one reason a key-popup group keeps a row, dimmed, instead of dropping it:
/// the pane has not acknowledged a screen yet, so the key becomes available on
/// its own. Every other reason means the action does not apply to this target.
const WAITING: &str = "waiting for acknowledged screen";
pub(super) const LAUNCH_BLOCKED: &str = "root workspace is not on the default branch";
pub(super) const ROOT_PROTECTED: &str = "repository-root workspace cannot be removed";
pub(super) struct KeyBinding { … }

pub(super) struct KeyGroup { … }

pub(super) fn key_binding(
    key: &'static str,
    name: &'static str,
    description: String,
    code: KeyCode,
    action: Action,
) -> KeyBinding { … }

/// A Copy or History key: the capture dispatcher interprets it.
fn capture(
    key: &'static str,
    name: &'static str,
    description: String,
    code: KeyCode,
) -> KeyBinding {
    key_binding(key, name, description, code, Action::Capture(code))
}

/// Browse's own modifier rule, kept from the dispatcher this table replaced:
/// Alt, Super and Shift never select a different action, so `Alt-x` closes the
/// pane that `x` closes, and Ctrl reaches Browse only as Ctrl-t. The key popup
/// is stricter and matches bindings without this fold.
fn browse_modifiers(mut key: KeyEvent) -> KeyEvent { … }

/// Arrows and back-tab are spellings of the letter keys a `j/Down` style label
/// names. A single-character label answers to that character alone, so an arrow
/// never stands in for it.
fn folded(code: KeyCode) -> KeyCode { … }

impl KeyBinding { … }

impl Dashboard { … }

pub(super) fn agents_binding() -> KeyBinding { … }

pub(super) fn is_operator_quick_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('o') && key.modifiers == KeyModifiers::CONTROL
}
pub(super) fn keymap(dashboard: &Dashboard) -> Vec<KeyGroup> { … }
fn mode_keymap(dashboard: &Dashboard) -> Vec<KeyGroup> { … }

#[cfg(test)]
mod agent_search_tests { … }

#[cfg(test)]
mod local_terminals_binding_tests { … }

[…1018ln elided; re-read needed ranges, e.g. crates/ovrcr-tui/src/dashboard/keymap.rs:7-9,16-70]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/keymap.rs", "pattern": "CreateTerminal|CreateWorkspace|Char\\('n'\\)|Char\\('w'\\)|LAUNCH_BLOCKED|browse_modifiers"}
 21|    OperatorKey(KeyCode, KeyModifiers),
*22|    CreateTerminal,
*23|    CreateWorkspace,
 24|    RegisterProject,
 25|    CloseTerminal,
 26|    RemoveWorkspace,
...
 82|const WAITING: &str = "waiting for acknowledged screen";
*83|pub(super) const LAUNCH_BLOCKED: &str = "root workspace is not on the default branch";
 84|pub(super) const ROOT_PROTECTED: &str = "repository-root workspace cannot be removed";
 85|pub(super) struct KeyBinding {
 86|    /// The label the footer and the popup print.
...
 142|/// is stricter and matches bindings without this fold.
*143|fn browse_modifiers(mut key: KeyEvent) -> KeyEvent {
 144|    key.modifiers &= KeyModifiers::CONTROL;
 145|    key
 146|}
...
 193|    pub(super) fn shown_in_group(&self) -> bool {
*194|        self.enabled() || matches!(self.reason, Some(WAITING | LAUNCH_BLOCKED | ROOT_PROTECTED))
 195|    }
 196|
 197|    /// This binding as the popup group lists it: the group's label, and the
...
 255|                    {
*256|                        browse_modifiers(key)
 257|                    } else {
 258|                        key
 259|                    },
...
 290|            }
*291|            Action::CreateTerminal => self.open_create_terminal(),
*292|            Action::CreateWorkspace => self.open_create_workspace(),
 293|            Action::RegisterProject => self.open_register_project(),
 294|            Action::CloseTerminal => {
 295|                if let Some(action) = self.soft_fail_if_provisional() {
...
 492|                        "New Conversation",
*493|                        Char('n'),
 494|                        KeyModifiers::CONTROL,
 495|                    ),
 496|                    native("Ctrl-g", "Browse", Char('g'), KeyModifiers::CONTROL),
...
 508|                "New Conversation",
*509|                Char('n'),
 510|                KeyModifiers::CONTROL,
 511|            ),
 512|            native("Tab", "Chats", Tab, KeyModifiers::NONE),
...
 803|        .and_then(|workspace| workspace.warning.as_ref())
*804|        .map(|_| LAUNCH_BLOCKED);
 805|    let workspace_target = current
 806|        .map(|workspace| workspace_heading(dashboard, workspace))
 807|        .unwrap_or_else(|| {
...
 1003|        KeyGroup { title: "Create".into(), keys: vec![
*1004|            key_binding("n", "Create terminal", format!("Choose an agent or shell to start in {workspace_target}; opens a form"), Char('n'), Action::CreateTerminal).unless(launch_blocked.or(no_workspace)).group('w', "n"),
*1005|            key_binding("w", "Create workspace", format!("Create a worktree and branch under {project_target} and choose its first Agent or Terminal; opens a form"), Char('w'), Action::CreateWorkspace).unless(dashboard.hierarchy.projects.is_empty().then_some("no project registered")).group('p', "n"),
 1006|            key_binding("a", "Register project", "Register a repository and its root workspace; opens a form; keeps the repository".into(), Char('a'), Action::RegisterProject).group('p', "a"),
 1007|        ] },
 1008|        KeyGroup { title: "Session".into(), keys: vec![

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/keymap.rs:780-1060"}
    if matches!(dashboard.mode, InputMode::Copy | InputMode::History) {
…
        return vec![
…
            KeyGroup {
…
            },
        ];
    }
    let selected = dashboard
        .action_session()
        .and_then(|id| find_session(dashboard, id));
    let (project, workspace) = dashboard.creation_context();
    let current = dashboard.current_workspace();
    let target = selected
        .map(|s| {
            let place = current
                .map(|workspace| workspace_heading(dashboard, workspace))
                .unwrap_or_else(|| s.project.clone());
            format!("{} (#{}) in {place}", s.display_name(), s.id.0)
        })
        .unwrap_or_default();
    let missing = selected.is_none().then_some("no session selected");
    let no_workspace = (!dashboard
        .hierarchy
        .projects
        .iter()
        .any(|p| !p.workspaces.is_empty()))
    .then_some("no workspace available; register a project first");
    let launch_blocked = current
        .and_then(|workspace| workspace.warning.as_ref())
        .map(|_| LAUNCH_BLOCKED);
    let workspace_target = current
        .map(|workspace| workspace_heading(dashboard, workspace))
        .unwrap_or_else(|| {
            if workspace.is_empty() {
                "the workspace you choose".into()
            } else {
                format!("{project} / {workspace}")
            }
        });
    let project_target = if project.is_empty() {
        "the project you choose"
    } else {
        &project
    };
    let running = selected.is_some_and(|s| s.phase == SessionPhase::Running);
    let paused = selected.is_some_and(|s| s.phase == SessionPhase::Paused);
    let mut view = vec![
        agents_binding(),
        key_binding(
            "o",
            "Operator",
            "Open concurrent Operator conversations".into(),
            Char('o'),
            Action::Operator,
        )
        .bare()
        .group('v', "o"),
        key_binding(
            "u",
            "Quota details",
            "Inspect native provider allowance, source, reset and freshness".into(),
            Char('u'),
            Action::QuotaDetails,
        )
        .bare()
        .group('v', "u"),
        key_binding(
            "t/Ctrl-t",
            "Tasks",
            "Open scheduled tasks; sessions keep running".into(),
            Char('t'),
            Action::Tasks,
        )
        .group('v', "t/Ctrl-t"),
        key_binding(
            ":",
            "Search",
            "Search dashboard actions and terminals".into(),
            Char(':'),
            Action::Palette,
        )
        .group('v', ":"),
        key_binding(
            "v",
            "Split",
            format!("Open a second pane beside {target}; sessions keep running"),
            Char('v'),
            Action::Split,
        )
        .unless(
            (dashboard.panes.len() >= 2)
                .then_some("already split")
                .or(missing)
                .or_else(|| {
                    (dashboard
                        .visible_rows()
                        .iter()
                        .filter(|r| matches!(r, TreeRow::Session { .. }))
                        .count()
                        < 2)
                    .then_some("no other visible session")
                }),
        )
        .group('v', "v"),
        key_binding(
            "Tab/Shift-Tab",
            "Next pane",
            "Focus the other pane; sessions keep running".into(),
            Tab,
            Action::OtherPane,
        )
        .unless(
            (dashboard.panes.len() < 2)
                .then_some("only one pane")
                .or(missing),
        )
        .group('v', "Tab/Shift-Tab"),
        key_binding(
            "x",
            "Close pane",
            format!("Hide the focused pane for {target}; its session keeps running"),
            Char('x'),
            Action::ClosePane,
        )
        .unless(
            (dashboard.panes.len() < 2)
                .then_some("only one pane")
                .or(missing),
        )
        .group('v', "x"),
        key_binding(
            "j/Down",
            "Next row",
            "Select the next visible sidebar row".into(),
            Char('j'),
            Action::NextSession,
        )
        .group('v', "j/Down"),
        key_binding(
            "k/Up",
            "Previous row",
            "Select the previous visible sidebar row".into(),
            Char('k'),
            Action::PreviousSession,
        )
        .group('v', "k/Up"),
        key_binding(
            "b",
            if dashboard.sidebar_hidden {
                "Show sidebar"
            } else {
                "Hide sidebar"
            },
            "Hide or show the sidebar; the panes take its width; j and k still move the selection"
                .into(),
            Char('b'),
            Action::ToggleSidebar,
        )
        .once()
        .group('v', "b"),
    ];
    view.extend(help());
    let (enter_name, enter_description, enter_disabled, enter_action) = if let Some(container) =
        dashboard.selected_container.as_ref()
    {
        let collapsed = match container {
            TreeRow::Project { name } => dashboard.collapsed_projects.contains(name),
            TreeRow::Workspace { project, id } => dashboard
                .collapsed_workspaces
                .contains(&(project.clone(), id.clone())),
            TreeRow::Session { .. }
            | TreeRow::ProvisionalSession { .. }
            | TreeRow::ProvisionalWorkspace { .. } => false,
        };
        if collapsed {
            (
                "Expand",
                "Expand the selected project or workspace".into(),
                None,
                Action::Focus,
            )
        } else {
            (
                "Collapse",
                "Collapse the selected project or workspace".into(),
                None,
                Action::Focus,
            )
        }
    } else if let Some(session) = selected.filter(|s| !s.phase.is_live()) {
        // The row outlived its process. Enter is the one key everyone tries, so
        // it leads straight to the recovery the palette already offers.
        let recovery = session.recovery.as_ref();
        let (name, description) = match &session.kind {
            SessionKind::Agent { .. } if recovery.is_some_and(|r| r.unavailable.is_some()) => (
                "Resume unavailable",
                format!("Explain why {target} cannot resume its conversation"),
            ),
            SessionKind::Agent { .. } if recovery.is_some_and(|r| r.failure.is_some()) => (
                "Retry resume conversation",
                format!(
                    "Resume the exact conversation of {target} in the same row, with confirmation"
                ),
            ),
            SessionKind::Agent { .. } => (
                "Resume conversation",
                format!(
                    "Resume the exact conversation of {target} in the same row, with confirmation"
                ),
            ),
            SessionKind::Terminal => (
                "Reopen shell",
                format!("Reopen {target} in a fresh shell in the same row, with confirmation"),
            ),
        };
        (name, description, None, Action::Reopen)
    } else {
        (
            "Focus",
            format!("Send terminal input to {target}"),
            missing
                .or_else(|| (!running).then_some("session is not running"))
                .or_else(|| selected.and_then(super::assignment_control::session_input_denial))
                .or_else(|| (!dashboard.input_is_allowed()).then_some(WAITING)),
            Action::Focus,
        )
    };
    let groups = vec![
        KeyGroup { title: "Create".into(), keys: vec![
            key_binding("n", "Create terminal", format!("Choose an agent or shell to start in {workspace_target}; opens a form"), Char('n'), Action::CreateTerminal).unless(launch_blocked.or(no_workspace)).group('w', "n"),
            key_binding("w", "Create workspace", format!("Create a worktree and branch under {project_target} and choose its first Agent or Terminal; opens a form"), Char('w'), Action::CreateWorkspace).unless(dashboard.hierarchy.projects.is_empty().then_some("no project registered")).group('p', "n"),
            key_binding("a", "Register project", "Register a repository and its root workspace; opens a form; keeps the repository".into(), Char('a'), Action::RegisterProject).group('p', "a"),
        ] },
        KeyGroup { title: "Session".into(), keys: vec![
            key_binding("Enter", enter_name, enter_description, Enter, enter_action).unless(enter_disabled).group('t', "Enter"),
            key_binding("H", "Take control", format!("Take human control of {target}; the child keeps running; does not interrupt or archive"), Char('H'), Action::TakeControl).unless(super::assignment_control::transition_denial(selected, true).or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).once().group('t', "H"),
            key_binding("A", "Return to Pi", format!("Return {target} to Pi pending fresh parent reconciliation; human input stops immediately"), Char('A'), Action::ReturnControl).unless(super::assignment_control::transition_denial(selected, false).or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).once().group('t', "A"),
            key_binding("p", "Pause", format!("Pause the processes of {target}; no confirmation"), Char('p'), Action::Pause).unless(missing.or_else(|| (!running).then_some("not running"))).group('t', "p"),
            key_binding("r", "Resume", format!("Resume the processes of {target}; no confirmation"), Char('r'), Action::Resume).unless(missing.or_else(|| (!paused).then_some("not paused"))).group('t', "r"),
            key_binding("R", "Mark reviewed", format!("Mark the displayed unread agent response from {target} reviewed; activity and reporting health stay unchanged"), Char('R'), Action::MarkReviewed).unless(missing.or_else(|| selected.and_then(|s| s.unread.as_ref()).is_none().then_some("no unread response"))).once().group('t', "R"),
            key_binding("X", "Close terminal", format!("Archive {target}. Asks for confirmation before stopping live work."), Char('X'), Action::CloseTerminal).unless(missing).group('t', "x"),
            key_binding("[", "Copy screen", format!("Freeze the current screen of {target} for copying; sessions keep running"), Char('['), Action::CopyScreen).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).once().group('t', "c"),
            key_binding("PageUp", "History", format!("Read frozen output from {target}; sessions keep running"), PageUp, Action::History).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).group('t', "h"),
        ] },
        KeyGroup { title: "View".into(), keys: view },
        KeyGroup { title: "Dashboard".into(), keys: vec![
            key_binding("N", if dashboard.settings.desktop_notifications { "Disable desktop notifications" } else { "Enable desktop notifications" }, "Save notifications through the Server for new agent responses or input requests; no replay".into(), Char('N'), Action::ToggleNotifications).once(),
            key_binding("S", if dashboard.settings.ready_sound { "Disable ready sound" } else { "Enable ready sound" }, if cfg!(target_os = "macos") {
                "Save ready sound through the Server for new agent responses or input requests; requires desktop notifications; choose a sound in Settings; no replay"
            } else {
                "Save ready sound through the Server for new agent responses or input requests, independent of desktop notifications; no replay"
            }.into(), Char('S'), Action::ToggleSound).once(),
            key_binding("L", "Automatic local terminals", format!("Cycle automatic local terminal creation (currently {}); applies to newly provisioned workspaces only", dashboard.settings.automatic_local_terminals.label()), Char('L'), Action::CycleLocalTerminals).once(),
            key_binding("q", "Detach", "Detach this dashboard; the server and every session keep running".into(), Char('q'), Action::Detach),
        ] },
    ];
    #[cfg(target_os = "macos")]
    let groups = {
        let mut groups = groups;
        let checking = dashboard.notification_permission_unconfirmed();
        groups.last_mut().unwrap().keys.insert(
            1,
            key_binding(
                "O",
                if checking {
                    "Check notification permission"
                } else {
                    "Open notification settings"
                },
                if checking {
                    "Check OVRCR's current macOS notification permission once"
                } else {
                    "Open System Settings, then choose Notifications → OVRCR to allow banners"
                }
                .into(),
                Char('O'),
                Action::NotificationSettings,
            )
            .once()
            .bare()
            .unless(if !dashboard.settings.desktop_notifications {
                Some("desktop notifications are disabled")
            } else {
                (!dashboard.notification_recovery_available())
                    .then_some("notification permission does not need recovery")
            }),
        );
        groups
    };

[Showing lines 779-1063 of 1106. Use :1064 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/settings.rs", "pattern": "launch_choices|LaunchChoice|launch_choice_request|branch_prefix|agents"}
 5|use ovrcr_protocol::Request;
*6|pub use ovrcr_protocol::{AgentOverride, AutomaticLocalTerminals, LaunchChoice, Settings};
 7|
 8|pub(super) const INVALID_SOUND_CHOICE_NOTICE: &str =
 9|    "Ready sound choice invalid; banners are silent. Choose a sound in Settings";
...
 33|/// Server merges it into the project's entry and keeps other projects.
*34|pub(super) fn launch_choice_request(project: &str, choice: &LaunchChoice) -> Request {
 35|    let mut entry = toml_edit::InlineTable::new();
 36|    match choice {
*37|        LaunchChoice::Terminal => {
 38|            entry.insert("kind", "Terminal".into());
 39|        }
*40|        LaunchChoice::Agent(preset) => {
 41|            entry.insert("kind", "Agent".into());
 42|            entry.insert("preset", preset.as_str().into());
 43|        }
...
 45|    Request::SetSetting {
*46|        path: format!("launch_choices.{}", toml_edit::Key::new(project)),
 47|        value: Some(entry.to_string()),
 48|    }
 49|}
...
 56|    #[test]
*57|    fn launch_choice_requests_quote_the_project_and_name_every_field() {
 58|        assert_eq!(
*59|            launch_choice_request("project.with.dots", &LaunchChoice::Agent("new".into())),
 60|            Request::SetSetting {
*61|                path: "launch_choices.\"project.with.dots\"".into(),
 62|                value: Some("{ kind = \"Agent\", preset = \"new\" }".into()),
 63|            }
 64|        );
 65|        assert_eq!(
*66|            launch_choice_request("demo", &LaunchChoice::Terminal),
 67|            Request::SetSetting {
*68|                path: "launch_choices.demo".into(),
 69|                value: Some("{ kind = \"Terminal\" }".into()),
 70|            }
 71|        );

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs", "pattern": "struct CreateSessionRequest|struct SessionLaunch|CreateWorkspaceWithLaunch|struct SessionSummary|enum SessionKind"}
 166|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*167|pub struct CreateSessionRequest {
 168|    pub project: String,
 169|    pub workspace: String,
 170|    /// Empty requests a unique workspace-based session name.
...
 178|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*179|pub struct SessionLaunch {
 180|    pub argv: Vec<OsString>,
 181|    pub label: Option<String>,
 182|    pub kind: SessionKind,
...
 285|    },
*286|    CreateWorkspaceWithLaunch {
 287|        project: String,
 288|        id: String,
 289|        branch: BranchRequest,
...
 498|    CloseTerminal,
*499|    CreateWorkspaceWithLaunch,
 500|}
 501|
 502|/// Result of a finished Lifecycle job, correlated by `client_token`.
...
 906|            (
*907|                "CreateWorkspaceWithLaunch",
*908|                Request::CreateWorkspaceWithLaunch {
 909|                    project: "a".into(),
 910|                    id: "b".into(),
 911|                    branch: BranchRequest::Existing { branch: "c".into() },
...
 920|                "CreateWorkspaceWithoutLaunch",
*921|                Request::CreateWorkspaceWithLaunch {
 922|                    project: "a".into(),
 923|                    id: "b".into(),
 924|                    branch: BranchRequest::Existing { branch: "c".into() },
...
 3356|        (
*3357|            "Request::CreateWorkspaceWithLaunch",
 3358|            "1f016101620101630101000273680000",
 3359|        ),
 3360|        (

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs:130-330"}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReport {
    pub session: SessionId,
    pub capability: [u8; 32],
    pub sequence: Option<u64>,
    pub update: AgentUpdate,
}

impl std::fmt::Debug for AgentReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "AgentReport {{ session: {:?}, capability: [redacted], sequence: {:?}, update: {:?} }}",
            self.session, self.sequence, self.update
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    Response { request_id: u64, response: Response },
    Event(ServerEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientRole {
    Control,
    Dashboard,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchRequest {
    New { branch: String, base: String },
    Existing { branch: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub project: String,
    pub workspace: String,
    /// Empty requests a unique workspace-based session name.
    /// The display title stays at this name unless the user renames it.
    pub name: String,
    pub label: Option<String>,
    pub argv: Vec<OsString>,
    pub kind: SessionKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLaunch {
    pub argv: Vec<OsString>,
    pub label: Option<String>,
    pub kind: SessionKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    DashboardHello,
    DashboardGeometry {
        size: TerminalSize,
    },
    List,
    AddProject {
        name: String,
        repo: PathBuf,
        workspace_root: PathBuf,
    },
    RemoveProject {
        name: String,
    },
    CreateWorkspace {
        project: String,
        id: String,
        branch: BranchRequest,
    },
    RemoveWorkspace {
        project: String,
        name: String,
        /// Acknowledge ownership-uncertain stopped sessions and discard uncommitted
        /// changes. Live sessions and active task runs still block removal.
        force: bool,
    },
    CreateSession(CreateSessionRequest),
    RemoveSession {
        session: SessionId,
    },
    KillSession {
        session: SessionId,
    },
    PauseSession {
        session: SessionId,
    },
    ResumeSession {
        session: SessionId,
    },
    Select {
        session: SessionId,
        size: TerminalSize,
    },
    Input {
        session: SessionId,
        run: SessionRunId,
        bytes: Vec<u8>,
    },
    Resize {
        session: SessionId,
        size: TerminalSize,
    },
    Shutdown {
        kill: bool,
    },
    Inspect,
    ReadTerminal {
        session: SessionId,
        max_lines: Option<usize>,
    },
    SendTerminal {
        session: SessionId,
        text: String,
        submit: bool,
    },
    CloseTerminal {
        session: SessionId,
        expected_run: SessionRunId,
    },
    Task(Box<TaskRequest>),
    AgentReport(AgentReport),
    HistoryBegin {
        session: SessionId,
    },
    HistoryPage {
        session: SessionId,
        snapshot: HistorySnapshotId,
        start_row: u32,
        rows: u16,
        start_col: u16,
        cols: u16,
    },
    HistoryEnd {
        session: SessionId,
        snapshot: HistorySnapshotId,
    },
    SetView {
        view: DashboardView,
    },
    ReserveAgent(crate::ReserveAgent),
    Supervisor(crate::SupervisorRequest),
    AgentStatus {
        auth: crate::SupervisorAuth,
        operation: String,
    },
    SupervisorHello(crate::SupervisorAuth),
    MarkReviewed {
        session: SessionId,
        expected: crate::ReadyObservation,
    },
    CreateWorkspaceWithLaunch {
        project: String,
        id: String,
        branch: BranchRequest,
        launch: Option<SessionLaunch>,
    },
    SetSessionTitle {
        session: SessionId,
        title: Option<String>,
    },
    ReopenSession {
        session: SessionId,
        expected_run: SessionRunId,
        acknowledge_stopped: bool,
    },
    DeleteArchivedSession {
        session: SessionId,
        expected_run: SessionRunId,
    },
    UnarchiveSession {
        session: SessionId,
        expected_run: SessionRunId,
    },
    AcknowledgeSessionStopped {
        session: SessionId,
        expected_run: SessionRunId,
    },
    /// First display of an interrupted agent; never substitutes for explicit Retry or acknowledgement.
    RecoverSession {
        session: SessionId,
        expected_run: SessionRunId,
    },
    /// One named key, such as `:enter:`, written to the current run as the
    /// keyboard would send it. Refused while the Dashboard is focused on it.
    Keystroke {
        session: SessionId,
        expected_run: SessionRunId,
        key: String,
    },
    /// Edit one setting in the settings document. `path` is a setting path
    /// such as `quota.codex.command`, `picker_roots[2]` or
    /// `launch_choices.myproj.kind`; `value` is TOML value text, and `None`
    /// removes the key or element. The Server validates, writes, re-reads and
    /// republishes `SettingsChanged`.
    SetSetting {
        path: String,
        value: Option<String>,
    },
…
}

[Showing lines 129-333 of 4049. Use :334 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs"}
//! Provisional rows and the Lifecycle status strip (ADR 0010 / issue #297).

use super::{Dashboard, DashboardAction, TreeRow};
use crate::protocol::{
    ErrorCode, LifecycleOp, LifecycleOutcome, Request, ServerEvent, SessionId, SessionKind,
};
use crate::session::SessionSummary;

/// Fiction until the Server completes the Lifecycle job or a failed row is dismissed.
#[derive(Clone, Debug)]
pub(super) struct Provisional {
    pub token: u64,
    pub kind: ProvisionalKind,
    pub failed: Option<(ErrorCode, String)>,
}

#[derive(Clone, Debug)]
pub(super) enum ProvisionalKind {
    CreatingWorkspace { … },
    RemovingWorkspace { … },
    CreatingSession { … },
    ClosingSession {
        session: SessionId,
    },
}

impl ProvisionalKind {
    pub(super) fn strip_message(&self) -> String { … }

    #[allow(dead_code)]
    pub(super) fn op(&self) -> LifecycleOp { … }
}

impl Provisional { … }

fn branch_label(branch: &crate::protocol::BranchRequest) -> String { … }

impl Dashboard { … }

#[cfg(test)]
mod tests { … }

[…554ln elided; re-read needed ranges, e.g. crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs:19-23,24-27]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/lifecycle.rs"}
//! One-at-a-time Lifecycle jobs off the Dashboard connection reader (ADR 0010).

use super::*;
use ovrcr_protocol::{LifecycleOp, LifecycleOutcome, SessionLaunch};

pub(super) enum JobKind {
    CreateWorkspace { … },
    CreateWorkspaceWithLaunch { … },
    RemoveWorkspace { … },
    CreateSession(ovrcr_protocol::CreateSessionRequest),
    CloseTerminal { … },
}

impl JobKind { … }

pub(super) struct Job { … }

/// Accepts at most one Lifecycle job; the worker runs accepted work.
pub(crate) struct Lifecycle { … }

impl Default for Lifecycle { … }

impl Lifecycle { … }

pub(super) fn try_accept(state: &ServerState, client_token: u64, kind: JobKind) -> Response { … }

fn clear_inflight(state: &ServerState, client_token: u64) { … }

fn publish_completed(
    state: &ServerState,
    client_token: u64,
    op: LifecycleOp,
    outcome: LifecycleOutcome,
    publish_hierarchy: bool,
) { … }

fn outcome_from_error(error: anyhow::Error) -> (LifecycleOutcome, bool) { … }

/// Runs accepted Lifecycle jobs until the channel closes or the server shuts down.
pub(super) fn run(state: Arc<ServerState>, rx: std::sync::mpsc::Receiver<Job>) { … }

#[cfg(test)]
mod tests { … }

[…237ln elided; re-read needed ranges, e.g. crates/ovrcr-runtime/src/server/lifecycle.rs:7-11,12-17]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/lifecycle.rs:1-260"}
//! One-at-a-time Lifecycle jobs off the Dashboard connection reader (ADR 0010).

use super::*;
use ovrcr_protocol::{LifecycleOp, LifecycleOutcome, SessionLaunch};

pub(super) enum JobKind {
    CreateWorkspace {
        project: String,
        id: String,
        branch: BranchRequest,
    },
    CreateWorkspaceWithLaunch {
        project: String,
        id: String,
        branch: BranchRequest,
        launch: Option<SessionLaunch>,
    },
    RemoveWorkspace {
        project: String,
        name: String,
        force: bool,
    },
    CreateSession(ovrcr_protocol::CreateSessionRequest),
    CloseTerminal {
        session: SessionId,
        expected_run: SessionRunId,
    },
}

impl JobKind {
    pub(super) fn op(&self) -> LifecycleOp {
        match self {
            Self::CreateWorkspace { .. } => LifecycleOp::CreateWorkspace,
            Self::CreateWorkspaceWithLaunch { .. } => LifecycleOp::CreateWorkspaceWithLaunch,
            Self::RemoveWorkspace { .. } => LifecycleOp::RemoveWorkspace,
            Self::CreateSession(_) => LifecycleOp::CreateSession,
            Self::CloseTerminal { .. } => LifecycleOp::CloseTerminal,
        }
    }
}

pub(super) struct Job {
    pub client_token: u64,
    pub kind: JobKind,
}

/// Accepts at most one Lifecycle job; the worker runs accepted work.
pub(crate) struct Lifecycle {
    inflight: Mutex<Option<u64>>,
    tx: Mutex<Option<SyncSender<Job>>>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            inflight: Mutex::new(None),
            tx: Mutex::new(None),
        }
    }
}

impl Lifecycle {
    pub(super) fn attach(&self, tx: SyncSender<Job>) {
        *self.tx.lock().unwrap() = Some(tx);
    }

    pub(super) fn detach(&self) {
        *self.tx.lock().unwrap() = None;
    }

    #[cfg(test)]
    pub(super) fn inflight_token(&self) -> Option<u64> {
        *self.inflight.lock().unwrap()
    }

    /// True when a Dashboard Lifecycle job can be queued (worker attached).
    pub(super) fn async_ready(&self) -> bool {
        self.tx.lock().unwrap().is_some()
    }
}

pub(super) fn try_accept(state: &ServerState, client_token: u64, kind: JobKind) -> Response {
    let tx = {
        let guard = state.lifecycle.tx.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx.clone(),
            None => {
                return error_response(ErrorCode::Internal, "lifecycle worker is not attached");
            }
        }
    };
    {
        let mut slot = state.lifecycle.inflight.lock().unwrap();
        if slot.is_some() {
            return error_response(ErrorCode::Conflict, "a lifecycle job is already running");
        }
        *slot = Some(client_token);
    }
    match tx.try_send(Job { client_token, kind }) {
        Ok(()) => Response::Ok,
        Err(_) => {
            *state.lifecycle.inflight.lock().unwrap() = None;
            error_response(ErrorCode::Internal, "lifecycle worker is unavailable")
        }
    }
}

fn clear_inflight(state: &ServerState, client_token: u64) {
    let mut slot = state.lifecycle.inflight.lock().unwrap();
    if *slot == Some(client_token) {
        *slot = None;
    }
}

fn publish_completed(
    state: &ServerState,
    client_token: u64,
    op: LifecycleOp,
    outcome: LifecycleOutcome,
    publish_hierarchy: bool,
) {
    if publish_hierarchy {
        state
            .dashboard
            .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                state.hierarchy(),
            )));
    }
    state
        .dashboard
        .try_send(ServerMessage::Event(ServerEvent::LifecycleCompleted {
            client_token,
            op,
            outcome,
        }));
}

fn outcome_from_error(error: anyhow::Error) -> (LifecycleOutcome, bool) {
    let publish_hierarchy = error
        .downcast_ref::<LifecycleFailure>()
        .is_some_and(|failure| failure.publish_hierarchy);
    let code = super::connections::lifecycle_code(&error);
    let message = error_chain_string(&error);
    (
        LifecycleOutcome::Failed { code, message },
        publish_hierarchy,
    )
}

/// Runs accepted Lifecycle jobs until the channel closes or the server shuts down.
pub(super) fn run(state: Arc<ServerState>, rx: std::sync::mpsc::Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        if state.shutdown.load(Ordering::Acquire) {
            clear_inflight(&state, job.client_token);
            break;
        }
        let op = job.kind.op();
        let (outcome, publish_hierarchy) = match job.kind {
            JobKind::CreateWorkspace {
                project,
                id,
                branch,
            } => match state.create_workspace(project, id, branch) {
                Ok(()) => (LifecycleOutcome::Succeeded, true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CreateWorkspaceWithLaunch {
                project,
                id,
                branch,
                launch,
            } => match state.create_workspace_with_launch(project, id, branch, launch) {
                Ok(None) => (LifecycleOutcome::Succeeded, true),
                Ok(Some(summary)) => (LifecycleOutcome::CreatedSession(Box::new(summary)), true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::RemoveWorkspace {
                project,
                name,
                force,
            } => match state.remove_workspace(&project, &name, force) {
                Ok(()) => (LifecycleOutcome::Succeeded, true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CreateSession(request) => match state.create_session(request) {
                Ok(summary) => (LifecycleOutcome::CreatedSession(Box::new(summary)), true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CloseTerminal {
                session,
                expected_run,
            } => match state.close_terminal(session, expected_run, requested_kill_grace()) {
                Ok(()) => (LifecycleOutcome::Succeeded, true),
                Err(error) => outcome_from_error(error),
            },
        };
        publish_completed(&state, job.client_token, op, outcome, publish_hierarchy);
        clear_inflight(&state, job.client_token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{CreateSessionRequest, SessionKind};
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;

    fn attach_worker(state: &Arc<ServerState>) -> std::thread::JoinHandle<()> {
        let (tx, rx) = mpsc::sync_channel(4);
        state.lifecycle.attach(tx);
        let worker_state = Arc::clone(state);
        std::thread::spawn(move || run(worker_state, rx))
    }

    #[test]
    fn dashboard_create_session_accepts_immediately_and_refuses_a_second_job() {
        let state = crate::server::tests::test_state(None, None);
        let _worker = attach_worker(&state);

        // Hold mutation_lock so the accepted job cannot finish before we refuse a second.
        let held = state.mutation_lock.lock().unwrap();

        let first = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::CreateSession(CreateSessionRequest {
                project: "p".into(),
                workspace: "w".into(),
                name: "s".into(),
                label: None,
                argv: vec![OsString::from("/bin/true")],
                kind: SessionKind::Terminal,
            }),
            42,
            None,
        );
        assert_eq!(first, Response::Ok);
        assert_eq!(state.lifecycle.inflight_token(), Some(42));

        let second = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::CloseTerminal {
                session: SessionId(1),
                expected_run: SessionRunId(1),
            },
            43,
            None,
        );
        assert_eq!(
            second,
            Response::Error {
                code: ErrorCode::Conflict,
                message: "a lifecycle job is already running".into(),
            }
        );

        drop(held);
        // Allow the worker to fail the create (no project) and clear the slot.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while state.lifecycle.inflight_token().is_some() {
…
        }
…
    }
…
}

[Showing lines 1-263 of 295. Use :264 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs:1-230"}
//! Provisional rows and the Lifecycle status strip (ADR 0010 / issue #297).

use super::{Dashboard, DashboardAction, TreeRow};
use crate::protocol::{
    ErrorCode, LifecycleOp, LifecycleOutcome, Request, ServerEvent, SessionId, SessionKind,
};
use crate::session::SessionSummary;

/// Fiction until the Server completes the Lifecycle job or a failed row is dismissed.
#[derive(Clone, Debug)]
pub(super) struct Provisional {
    pub token: u64,
    pub kind: ProvisionalKind,
    pub failed: Option<(ErrorCode, String)>,
}

#[derive(Clone, Debug)]
pub(super) enum ProvisionalKind {
    CreatingWorkspace {
        project: String,
        id: String,
        label: String,
    },
    RemovingWorkspace {
        project: String,
        id: String,
    },
    CreatingSession {
        project: String,
        workspace: String,
        name: String,
        label: Option<String>,
        #[allow(dead_code)]
        kind: SessionKind,
    },
    ClosingSession {
        session: SessionId,
    },
}

impl ProvisionalKind {
    pub(super) fn strip_message(&self) -> String {
        match self {
            Self::CreatingWorkspace { label, .. } => format!("Creating workspace {label}…"),
            Self::RemovingWorkspace { .. } => "Removing workspace…".into(),
            Self::CreatingSession { name, label, .. } => {
                let shown = label.as_deref().filter(|s| !s.is_empty()).unwrap_or(name);
                let shown = if shown.is_empty() { "session" } else { shown };
                format!("Creating {shown}…")
            }
            Self::ClosingSession { .. } => "Closing terminal…".into(),
        }
    }

    #[allow(dead_code)]
    pub(super) fn op(&self) -> LifecycleOp {
        match self {
            Self::CreatingWorkspace { .. } => LifecycleOp::CreateWorkspace,
            Self::RemovingWorkspace { .. } => LifecycleOp::RemoveWorkspace,
            Self::CreatingSession { .. } => LifecycleOp::CreateSession,
            Self::ClosingSession { .. } => LifecycleOp::CloseTerminal,
        }
    }
}

impl Provisional {
    pub(super) fn from_request(token: u64, request: &Request) -> Option<Self> {
        let kind = match request {
            Request::CreateWorkspace {
                project,
                id,
                branch,
            } => ProvisionalKind::CreatingWorkspace {
                project: project.clone(),
                id: id.clone(),
                label: branch_label(branch),
            },
            Request::CreateWorkspaceWithLaunch {
                project,
                id,
                branch,
                ..
            } => ProvisionalKind::CreatingWorkspace {
                project: project.clone(),
                id: id.clone(),
                label: branch_label(branch),
            },
            Request::RemoveWorkspace { project, name, .. } => ProvisionalKind::RemovingWorkspace {
                project: project.clone(),
                id: name.clone(),
            },
            Request::CreateSession(session) => ProvisionalKind::CreatingSession {
                project: session.project.clone(),
                workspace: session.workspace.clone(),
                name: session.name.clone(),
                label: session.label.clone(),
                kind: session.kind.clone(),
            },
            Request::CloseTerminal { session, .. } => {
                ProvisionalKind::ClosingSession { session: *session }
            }
            _ => return None,
        };
        Some(Self {
            token,
            kind,
            failed: None,
        })
    }

    pub(super) fn row_label(&self) -> String {
        if let Some((_, message)) = &self.failed {
            return format!("Failed: {message}");
        }
        match &self.kind {
            ProvisionalKind::CreatingWorkspace { label, .. } => format!("Creating… {label}"),
            ProvisionalKind::RemovingWorkspace { .. } => "Removing…".into(),
            ProvisionalKind::CreatingSession { name, label, .. } => {
                let shown = label.as_deref().filter(|s| !s.is_empty()).unwrap_or(name);
                let shown = if shown.is_empty() { "session" } else { shown };
                format!("Creating… {shown}")
            }
            ProvisionalKind::ClosingSession { .. } => "Closing…".into(),
        }
    }
}

fn branch_label(branch: &crate::protocol::BranchRequest) -> String {
    match branch {
        crate::protocol::BranchRequest::New { branch, .. }
        | crate::protocol::BranchRequest::Existing { branch } => branch.clone(),
    }
}

impl Dashboard {
    /// Select a just-created workspace once it appears in hierarchy (empty or with local shell).
    pub(super) fn attach_lifecycle_workspace(
        &mut self,
        project: String,
        id: String,
    ) -> DashboardAction {
        let Some(workspace) = super::state::find_workspace(self, &project, &id).cloned() else {
            return DashboardAction::Redraw;
        };
        if workspace.sessions.is_empty() {
            self.provisional = None;
            self.lifecycle_strip = None;
            self.dismiss_error_banner();
            self.select_container(TreeRow::Workspace {
                project: workspace.project.clone(),
                id: workspace.id.clone(),
            });
            let request_id = self.next_request_id();
            return match self.view_request(self.outer_area, request_id) {
                Ok(Some(message)) => {
                    self.enqueue_request(message);
                    DashboardAction::Redraw
                }
                _ => DashboardAction::Redraw,
            };
        }
        if let Some(session) = workspace
            .sessions
            .iter()
            .find(|session| {
                session.name == "local" && session.phase == crate::session::SessionPhase::Running
            })
            .map(|session| session.id)
        {
            let summary = super::state::find_session(self, session)
                .cloned()
                .unwrap_or_else(|| workspace.sessions[0].clone());
            self.provisional = None;
            self.lifecycle_strip = None;
            self.dismiss_error_banner();
            return self.select_created_session(summary);
        }
        // Launched first terminal is named after the workspace, not `local`.
        // Wait for LifecycleCompleted::CreatedSession rather than dropping fiction here.
        DashboardAction::Redraw
    }

    /// Hierarchy may land before LifecycleCompleted. Only auto-attach empty workspaces;
    /// WithLaunch waits for CreatedSession so we do not drop the Provisional early.
    pub(super) fn try_attach_lifecycle_workspace_from_hierarchy(
        &mut self,
    ) -> Option<DashboardAction> {
        let Some(row) = &self.provisional else {
            return None;
        };
        let ProvisionalKind::CreatingWorkspace { project, id, .. } = &row.kind else {
            return None;
        };
        let workspace = super::state::find_workspace(self, project, id)?;
        let ready = workspace.sessions.is_empty()
            || workspace.sessions.iter().any(|session| {
                session.name == "local" && session.phase == crate::session::SessionPhase::Running
            });
        if !ready {
            // Named launch session: wait for LifecycleCompleted::CreatedSession.
            return None;
        }
        let project = project.clone();
        let id = id.clone();
        Some(self.attach_lifecycle_workspace(project, id))
    }

    /// Id-requiring actions soft-fail while a Provisional row stands in for a real identity.
    pub(super) fn soft_fail_if_provisional(&mut self) -> Option<DashboardAction> {
        let Some(row) = &self.provisional else {
            return None;
        };
        if row.failed.is_some() {
            self.lifecycle_strip = Some("dismiss the failed row first (Esc)".into());
            return Some(DashboardAction::Redraw);
        }
        match &row.kind {
            ProvisionalKind::CreatingSession { .. } | ProvisionalKind::CreatingWorkspace { .. } => {
                self.lifecycle_strip =
                    Some("wait for the Lifecycle job to finish before using this action".into());
                Some(DashboardAction::Redraw)
            }
            ProvisionalKind::RemovingWorkspace { .. } | ProvisionalKind::ClosingSession { .. } => {
                self.lifecycle_strip = Some("a lifecycle job is already running".into());
                Some(DashboardAction::Redraw)
            }
        }
    }

    pub(super) fn lifecycle_busy(&self) -> bool {
        self.provisional
            .as_ref()
            .is_some_and(|row| row.failed.is_none())
            || self.lifecycle_pending.is_some()
    }
…
}

[Showing lines 1-233 of 604. Use :234 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard", "pattern": "LifecycleCompleted|accept_lifecycle_job|lifecycle_pending|refuse_lifecycle_job|lifecycle_busy"}
# crates/ovrcr-tui/src/dashboard/
## lifecycle_ui.rs
 178|        // Launched first terminal is named after the workspace, not `local`.
*179|        // Wait for LifecycleCompleted::CreatedSession rather than dropping fiction here.
 180|        DashboardAction::Redraw
 181|    }
 182|
*183|    /// Hierarchy may land before LifecycleCompleted. Only auto-attach empty workspaces;
 184|    /// WithLaunch waits for CreatedSession so we do not drop the Provisional early.
 185|    pub(super) fn try_attach_lifecycle_workspace_from_hierarchy(
 186|        &mut self,
...
 199|        if !ready {
*200|            // Named launch session: wait for LifecycleCompleted::CreatedSession.
 201|            return None;
 202|        }
 203|        let project = project.clone();
...
 229|
*230|    pub(super) fn lifecycle_busy(&self) -> bool {
 231|        self.provisional
 232|            .as_ref()
 233|            .is_some_and(|row| row.failed.is_none())
*234|            || self.lifecycle_pending.is_some()
 235|    }
 236|
*237|    pub(super) fn begin_lifecycle_pending(&mut self, token: u64, request: &Request) {
 238|        if let Some(row) = Provisional::from_request(token, request) {
*239|            self.lifecycle_pending = Some(row);
 240|        }
 241|    }
 242|
...
 244|    /// If HierarchyChanged already landed, attach immediately (either-order).
*245|    pub(super) fn accept_lifecycle_job(&mut self, token: u64) -> Option<DashboardAction> {
*246|        let row = self.lifecycle_pending.take()?;
 247|        if row.token != token {
*248|            self.lifecycle_pending = Some(row);
 249|            return None;
 250|        }
 251|        // Keep the launch preference across the Provisional wait so success can
...
 263|
*264|    pub(super) fn refuse_lifecycle_job(&mut self, message: String) {
*265|        self.lifecycle_pending = None;
 266|        self.lifecycle_strip = Some(message);
 267|    }
 268|
...
 288|        self.provisional = None;
*289|        self.lifecycle_pending = None;
 290|        self.lifecycle_strip = None;
 291|        self.lifecycle_preference = None;
 292|    }
...
 447|        match event {
*448|            ServerEvent::LifecycleCompleted {
 449|                client_token,
 450|                op,
 451|                outcome,
...
 501|            crate::dashboard::Dashboard::new(crate::session::TerminalSize { rows: 24, cols: 80 });
*502|        d.begin_lifecycle_pending(
 503|            3,
 504|            &Request::CreateSession(CreateSessionRequest {
 505|                project: "p".into(),
...
 512|        );
*513|        d.accept_lifecycle_job(3);
 514|        assert!(d.lifecycle_strip.is_some());
 515|        assert!(d.provisional.is_some());
 516|        assert!(d.dismiss_lifecycle_strip());
...
 583|            crate::dashboard::Dashboard::new(crate::session::TerminalSize { rows: 24, cols: 80 });
*584|        d.begin_lifecycle_pending(
 585|            1,
 586|            &Request::CreateWorkspace {
 587|                project: "p".into(),
...
 591|        );
*592|        d.accept_lifecycle_job(1);
*593|        assert!(d.lifecycle_busy());
*594|        d.refuse_lifecycle_job("a lifecycle job is already running".into());
 595|        assert_eq!(
 596|            d.lifecycle_strip.as_deref(),
 597|            Some("a lifecycle job is already running")
## mod.rs
 330|    /// Lifecycle job accepted but not yet shown as a Provisional row (awaiting Response::Ok).
*331|    lifecycle_pending: Option<lifecycle_ui::Provisional>,
 332|    /// In-flight or failed Provisional row.
 333|    provisional: Option<lifecycle_ui::Provisional>,
 334|    /// Bottom status strip for Lifecycle progress / soft refuse; Esc dismisses this only.
## palette.rs
 2828|    ) -> DashboardAction {
*2829|        if self.lifecycle_busy()
 2830|            && super::lifecycle_ui::Provisional::from_request(0, &request).is_some()
 2831|        {
 2832|            let message = "a lifecycle job is already running".to_string();
*2833|            self.refuse_lifecycle_job(message.clone());
 2834|            palette.error = Some(message);
 2835|            return DashboardAction::Redraw;
 2836|        }
...
 2866|        palette.error = None;
*2867|        self.begin_lifecycle_pending(request_id, &request);
 2868|        if history_before_request {
 2869|            self.queue_history_request();
 2870|        }
...
 3036|        if self
*3037|            .lifecycle_pending
 3038|            .as_ref()
 3039|            .is_some_and(|row| row.token == request_id)
 3040|        {
...
 3042|                Response::Ok => {
*3043|                    self.accept_lifecycle_job(request_id);
 3044|                    return Some(Vec::new());
 3045|                }
 3046|                Response::Error { code, message }
...
 3050|                    let message = format!("{code:?}: {message}");
*3051|                    self.refuse_lifecycle_job(message.clone());
 3052|                    if let Some(palette) = self.palette.as_mut() {
 3053|                        palette.pending = None;
 3054|                        palette.error = Some(message);
...
 3058|                Response::Error { .. } => {
*3059|                    self.lifecycle_pending = None;
 3060|                }
 3061|                _ => {}
 3062|            }
## state.rs
 611|            ignored_responses: HashSet::new(),
*612|            lifecycle_pending: None,
 613|            provisional: None,
 614|            lifecycle_strip: None,
 615|            lifecycle_preference: None,
...
 3570|                }
*3571|                ServerEvent::LifecycleCompleted {
 3572|                    client_token,
 3573|                    op,
 3574|                    outcome,

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs:236-350"}
    pub(super) fn lifecycle_busy(&self) -> bool {
…
    }

    pub(super) fn begin_lifecycle_pending(&mut self, token: u64, request: &Request) {
        if let Some(row) = Provisional::from_request(token, request) {
            self.lifecycle_pending = Some(row);
        }
    }

    /// Accept response for a Lifecycle job: show the Provisional row and strip.
    /// If HierarchyChanged already landed, attach immediately (either-order).
    pub(super) fn accept_lifecycle_job(&mut self, token: u64) -> Option<DashboardAction> {
        let row = self.lifecycle_pending.take()?;
        if row.token != token {
            self.lifecycle_pending = Some(row);
            return None;
        }
        // Keep the launch preference across the Provisional wait so success can
        // still ask the Server to remember it (palette closes on accept).
        let preference = self
            .palette
            .as_mut()
            .and_then(|palette| palette.launch_preference.take());
        self.lifecycle_preference = preference;
        self.lifecycle_strip = Some(row.kind.strip_message());
        self.provisional = Some(row);
        self.palette = None;
        self.try_attach_lifecycle_workspace_from_hierarchy()
    }

    pub(super) fn refuse_lifecycle_job(&mut self, message: String) {
        self.lifecycle_pending = None;
        self.lifecycle_strip = Some(message);
    }

    pub(super) fn dismiss_lifecycle_strip(&mut self) -> bool {
        self.lifecycle_strip.take().is_some()
    }

    pub(super) fn dismiss_failed_provisional(&mut self) -> bool {
        if self
            .provisional
            .as_ref()
            .is_some_and(|row| row.failed.is_some())
        {
            self.provisional = None;
            self.lifecycle_strip = None;
            true
        } else {
            false
        }
    }

    pub(super) fn drop_provisional_fiction(&mut self) {
        self.provisional = None;
        self.lifecycle_pending = None;
        self.lifecycle_strip = None;
        self.lifecycle_preference = None;
    }

    pub(super) fn on_lifecycle_completed(
        &mut self,
        client_token: u64,
        op: LifecycleOp,
        outcome: LifecycleOutcome,
    ) -> DashboardAction {
        // CreateWorkspaceWithLaunch shares the CreatingWorkspace provisional.
        let _ = op;
        match outcome {
            LifecycleOutcome::CreatedSession(summary) => {
                // Hierarchy may have arrived first; still select from the typed outcome.
                if let Some(row) = &self.provisional
                    && row.token != client_token
                {
                    return DashboardAction::None;
                }
                self.provisional = None;
                self.lifecycle_strip = None;
                self.select_created_session(*summary)
            }
            LifecycleOutcome::Succeeded => {
                let Some(row) = self.provisional.take() else {
                    return DashboardAction::Redraw;
                };
                if row.token != client_token {
                    self.provisional = Some(row);
                    return DashboardAction::None;
                }
                self.lifecycle_strip = None;
                match &row.kind {
                    ProvisionalKind::CreatingWorkspace { project, id, .. } => {
                        let project = project.clone();
                        let id = id.clone();
                        // Keep provisional until hierarchy has the row, then attach.
                        self.provisional = Some(row);
                        let action = self.attach_lifecycle_workspace(project, id);
                        if self.provisional.is_some() {
                            // Job finished but no empty/local attach target (e.g. exited local).
                            self.provisional = None;
                            self.lifecycle_strip = None;
                        }
                        action
                    }
                    _ => DashboardAction::Redraw,
                }
            }
            LifecycleOutcome::Failed { code, message } => {
                let Some(mut row) = self.provisional.take() else {
                    return DashboardAction::Redraw;
                };
                if row.token != client_token {
                    self.provisional = Some(row);
                    return DashboardAction::None;
                }
                // Preserve the sync remove UX: DirtyWorktree / SessionsRemain reopen as force.
                if let ProvisionalKind::RemovingWorkspace { project, id } = &row.kind
                    && matches!(code, ErrorCode::DirtyWorktree | ErrorCode::SessionsRemain)
                {
                    let project = project.clone();
                    let id = id.clone();
…
                }
…
            }
        }
    }

[Showing lines 235-353 of 604. Use :354 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/state.rs:3540-3660"}
            ServerMessage::Response {
…
            } => match response {
…
                Response::Error { code, message } => {
…
                    if matched_page.is_some() {
…
                        if matches!(code, ErrorCode::Conflict | ErrorCode::NotFound) {
                            if let Some(view) = self.history.take() {
…
                            }
                        } else if let Some(view) = self.history.as_mut() {
                            view.pending = None;
                            view.copy_job = None;
                            view.copy_completion = None;
                        }
                    }
                    if matched_view || !was_view_request {
                        self.set_error(format!("{code:?}: {message}"));
                        // Only a refused view hands the banner to the next view completion.
                        self.error_owned_by_view = matched_view;
                    }
                    if matched_history && self.copy.is_none() {
                        self.mode = if self.history.is_some() {
                            InputMode::History
                        } else {
                            InputMode::Browse
                        };
                    }
                }
            },
            ServerMessage::Event(event) => match event {
                ServerEvent::BridgeContext(_)
                | ServerEvent::NotificationNavigation(_)
                | ServerEvent::ITermFocus(_) => {}
                ServerEvent::OperatorChanged(id) => self.operator_changed(id),
                ServerEvent::HierarchyChanged(hierarchy) => {
                    for request in self.update_hierarchy(hierarchy) {
                        self.push_request(request);
                    }
                    self.try_attach_lifecycle_workspace_from_hierarchy();
                }
                ServerEvent::LifecycleCompleted {
                    client_token,
                    op,
                    outcome,
                } => {
                    self.on_lifecycle_completed(client_token, op, outcome);
                }
                ServerEvent::Output {
                    session,
                    run,
                    revision,
                    bytes,
                } => {
                    if revision == self.handshake.revision()
                        && self.handshake.is_ready(session)
                        && self.handshake.current_run(session) == Some(run)
                    {
                        if let Some(pane) = self
                            .panes
                            .iter_mut()
                            .find(|pane| pane.session == Some(session) && pane.run == Some(run))
                        {
                            pane.parser.process(&bytes);
                        }
                        if self.focused_session() == Some(session) {
                            self.reconcile_mouse_protocol();
                            if let Some(view) = self.history.as_mut()
                                && view.opened.session == session
                            {
                                view.new_output = true;
                            }
                        }
                    }
                }
                ServerEvent::ScreenDirty {
                    session,
                    run,
                    revision,
                } => {
                    if revision == self.handshake.revision()
                        && self.handshake.current_run(session) == Some(run)
                        && self
                            .panes
                            .iter()
                            .any(|pane| pane.session == Some(session) && pane.run == Some(run))
                    {
                        self.handshake.mark_stale(session);
                        if let Some(view) = self.history.as_mut()
                            && view.opened.session == session
                        {
                            view.new_output = true;
                        }
                        if !self.handshake.in_flight() {
                            let _ = self.request_view_at(self.outer_area);
                        }
                    }
                }
                ServerEvent::QuotaChanged(snapshot) => {
                    self.quotas = Some(*snapshot);
                    self.ensure_selection_visible(&self.visible_rows());
                }
                ServerEvent::SettingsChanged(report) => self.install_settings_report(*report),
                ServerEvent::Recorded(event) => self.events_recorded(event),
                ServerEvent::WipSavePrompt {
                    project,
                    workspace,
                    branch,
                } => self.queue_wip_prompt(project, workspace, branch),
                ServerEvent::SessionChanged(summary) => {
                    if find_session(self, summary.id).is_some_and(|current| {
                        current.run.0 > summary.run.0
                            || (current.run == summary.run
                                && super::assignment_control::older_control(&summary, current))
                    }) {
                        return self.drain_outbox();
                    }
                    self.assignment_control_changed(&summary);
                    self.observe_desktop_update(&summary);
                    let summary = *summary;
                    let previous_live =
                        find_session(self, summary.id).map(|session| session.phase.is_live());
                    for session in self
                        .hierarchy
                        .projects
                        .iter_mut()
                        .flat_map(|project| project.workspaces.iter_mut())
                        .flat_map(|workspace| workspace.sessions.iter_mut())
                    {
                        if session.id == summary.id {
                            *session = summary.clone();
                            break;
                        }
                    }
…
                }
            },

[Showing lines 3539-3663 of 4369. Use :3664 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/picker.rs"}
use super::render::{CRUST, MAUVE, TEXT};
…
use std::path::{Path, PathBuf};

const PATH_LIST_LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickItem { … }

pub(crate) fn workspace_pick_value(project: &str, id: &str) -> String {
    format!("{project}/{id}")
}

pub(crate) fn split_workspace_pick(value: &str) -> (String, String) { … }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickList { … }

impl PickList { … }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathEntry { … }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathListing { … }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathPicker { … }

impl PathPicker { … }

pub fn list_path_entries(input: &str, roots: &[PathBuf]) -> PathListing { … }

pub fn complete_path(input: &str, roots: &[PathBuf], selected: usize) -> Option<String> { … }

fn path_entry(name: String, git: bool) -> PathEntry { … }

fn read_dirs(dir: &Path, segment: &str, show_hidden: bool) -> Vec<PathEntry> { … }

fn split_input(input: &str) -> (&str, &str) { … }

fn expand_dir(prefix: &str) -> PathBuf {
    expand_path(prefix)
}

pub(crate) fn expand_path(input: &str) -> PathBuf { … }

fn display_path(path: &Path) -> String { … }

fn file_name(path: &Path) -> String { … }

fn home_dir() -> PathBuf { … }

fn subsequence(query: &str, label: &str) -> bool { … }

#[cfg(test)]
mod tests { … }

[…487ln elided; re-read needed ranges, e.g. crates/ovrcr-tui/src/dashboard/picker.rs:2-3,9-12]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/picker.rs", "pattern": "fn workspaces|fn projects|workspace_label|root|warning"}
 50|
*51|    pub fn projects(hierarchy: &HierarchySnapshot, preferred: &str) -> Self {
 52|        let mut list = Self::new(
 53|            hierarchy
 54|                .projects
...
 65|
*66|    pub fn workspaces(
 67|        hierarchy: &HierarchySnapshot,
 68|        project: &str,
 69|        workspace_id: &str,
*70|        include_root: bool,
 71|    ) -> Self {
 72|        let mut list = Self::new(
 73|            hierarchy
...
 77|                    project.workspaces.iter().filter_map(move |workspace| {
*78|                        if !include_root && workspace.root {
 79|                            return None;
 80|                        }
 81|                        let mut label = format!("{} / {}", project.name, workspace.name);
...
 107|                        .iter()
*108|                        .find(|workspace| include_root || !workspace.root)
 109|                })
 110|                .map(|workspace| workspace.id.clone())
 111|            {
...
 231|    /// it only holds `&self`, so callers must refresh this after every edit.
*232|    pub fn listing(&mut self, input: &str, roots: &[PathBuf]) -> &PathListing {
 233|        if self.cached.as_ref().is_none_or(|(key, _)| key != input) {
*234|            self.cached = Some((input.to_owned(), list_path_entries(input, roots)));
 235|        }
 236|        &self.cached.as_ref().unwrap().1
 237|    }
...
 245|
*246|pub fn list_path_entries(input: &str, roots: &[PathBuf]) -> PathListing {
 247|    let (prefix, segment) = split_input(input);
 248|    let show_hidden = segment.starts_with('.');
 249|    let mut entries = if prefix.is_empty() {
...
 251|        // sorting is for the children of a directory, below.
*252|        roots
 253|            .iter()
*254|            .filter_map(|root| {
*255|                if !root.is_dir() {
 256|                    return None;
 257|                }
*258|                let name = display_path(root);
*259|                if !subsequence(segment, &name) && !subsequence(segment, &file_name(root)) {
 260|                    return None;
 261|                }
*262|                Some(path_entry(name, root.join(".git").exists()))
 263|            })
 264|            .collect::<Vec<_>>()
 265|    } else {
...
 275|
*276|pub fn complete_path(input: &str, roots: &[PathBuf], selected: usize) -> Option<String> {
*277|    let listing = list_path_entries(input, roots);
 278|    let entry = listing.entries.get(selected)?;
 279|    let (prefix, _) = split_input(input);
 280|    if prefix.is_empty() {
...
 456|        with_home(dir.path(), || {
*457|            let roots = [dir.path().join("Code")];
*458|            let listing = list_path_entries("~/Co", &roots);
 459|            assert_eq!(
 460|                listing
 461|                    .entries
...
 466|            );
*467|            assert_eq!(complete_path("~/Co", &roots, 0).as_deref(), Some("~/Code/"));
 468|        });
 469|    }
 470|
 471|    #[test]
*472|    fn slash_prefix_lists_filesystem_root() {
 473|        let dir = tempfile::tempdir().unwrap();
 474|        std::fs::create_dir(dir.path().join("Code")).unwrap();
 475|        with_home(dir.path(), || {
*476|            let roots = [dir.path().join("Code")];
*477|            let listing = list_path_entries("/", &roots);
 478|            assert!(!listing.entries.is_empty());
 479|            assert!(
 480|                listing.entries.iter().all(|entry| entry.name != "Code"),
...
 494|        with_home(dir.path(), || {
*495|            let roots = [code.clone()];
 496|            let mut picker = PathPicker::new();
*497|            let first = picker.listing("~/Code/", &roots).clone();
 498|            assert_eq!(
 499|                first
 500|                    .entries
...
 506|            std::fs::remove_dir(code.join("repo")).unwrap();
*507|            let second = picker.listing("~/Code/", &roots).clone();
 508|            assert_eq!(first, second);
*509|            let third = picker.listing("~/Code/r", &roots).clone();
 510|            assert!(third.entries.is_empty(), "{:?}", third.entries);
 511|        });
 512|    }
 513|
*514|    /// An empty field is the picker roots themselves. Git-first and name
 515|    /// sorting apply to a directory's children, not to this list.
 516|    #[test]
*517|    fn empty_field_lists_roots_in_the_given_order() {
 518|        let dir = tempfile::tempdir().unwrap();
 519|        let plain = dir.path().join("z-plain");
 520|        let repo = dir.path().join("a-git");
...
 546|        with_home(dir.path(), || {
*547|            let roots = [code.clone()];
*548|            let listing = list_path_entries("~/Code/", &roots);
 549|            assert_eq!(
 550|                listing
 551|                    .entries
...
 556|            );
*557|            let hidden = list_path_entries("~/Code/.", &roots);
 558|            assert!(hidden.entries.iter().any(|entry| entry.name == ".hidden"));
 559|        });
 560|    }