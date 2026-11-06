Finalize this independent read-only plan review now, using only the supplied plan, design, and observed source excerpts. Your earlier source-inspection phase hit its time limit without a final report. No tools are available in this final phase. Do not ask questions or propose more browsing. Separate substantiated findings from proof gaps; no evidence means no finding. Return at most 5 actionable issues and 1200 words total, with exact path:line citations where present. Each issue: priority, concrete scenario, evidence, and smallest plan correction. Finish PASS / PASS WITH CLARIFICATIONS / NEEDS REVISION for plan quality, never runtime acceptance. Do not reopen submitted user decisions.

You are an independent Oh My Pi plan reviewer using the user-requested DeepSeek V4.1 Flash model. Review only; do not implement or edit any files. Tools are limited to read/grep/glob. Stay inside /Users/xlyk/Code/ovrcr. Never read credentials, .secrets, personal homes, unrelated reports or live app state. No provider/tool execution, network access, nested agents or user questions.

Task: validate docs/superpowers/plans/2026-11-05-agent-session-creation.md against docs/superpowers/specs/2026-11-05-agent-session-creation-design.md and actual owning source. Read AGENTS.md, CONTEXT.md and the relevant narrow development guides. This is an approved design but an unimplemented plan. Do not report desired new behavior as a current-code bug; identify missing/wrong implementation steps or invalid assumptions. Do not reopen explicit user decisions: registered workspaces only; child creation commits; branch drift follows same checkout; pending Create stays open; prompt memory-only per server lifetime; full model/prompt support required for Claude/Codex/Pi/omp; other launchers gated; n agent / separate terminal / w workspace.

Read the connected code needed for each finding. Return only substantiated, actionable plan issues. Cite exact repository-relative path:line for both plan/spec and source evidence. Distinguish blockers from nonblocking clarifications. For each finding give priority, concrete failing scenario, why current plan misses it, and smallest plan correction. Avoid speculative hardening or demands to add new frameworks. If no issue, say no actionable findings and list substantive checks and remaining proof gaps. Finish with PASS / PASS WITH CLARIFICATIONS / NEEDS REVISION, understood as plan quality, never runtime acceptance.

Review axis: verification and delivery sufficiency. Read existing relevant test entry points and fixtures, docs/testing-computer-use.md, docs/development/testing.md, and delivery guidance. Map requirements to actual production path assertions; check provider/model/prompt evidence vs mocks, four-harness coverage, restart/reconnect delivery, fixed-size and input behavior, compatibility callers and required check scope. Identify acceptance holes that could let this plan claim complete incorrectly. Do not run tests or providers.


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

SOURCE LOOKUP {"path": "docs/development/delivery.md"}
# Delegation, review, and delivery

- Honor the requested model, effort, and sequential/parallel workflow in the actual agent configuration. Give workers bounded briefs with current interfaces and explicit acceptance cases.
- Workers implement and self-review; the coordinator assigns independent review. Workers and reviewers must not recursively create duplicate review seats.
- Review the exact committed diff against the requirements and saved evidence. Check the real caller paths and test assertions, not just test names or passing counts.
- Return blocking findings to the same worker with a precise correction brief. Re-review the findings and correction diff; keep unrelated observations for the final feature review.
- Keep checkpoints current with completed work, actual commands/results, remaining cases, and blockers. Use the agent messaging tool when another agent needs a live update.
- Use `git` and `gh` for GitHub operations. Stage only intended files. Commit coherent reviewed units and honor existing authorization to push/open PRs; merging requires authorization too.
- PRs and handoffs state the problem, behavior change, exact verification, and remaining gaps. Keep roadmap checkboxes open until their required acceptance gates pass.

SOURCE LOOKUP {"path": "docs/development/testing.md"}
# Testing and acceptance

- Test the real entry point that changed. A passing helper test is insufficient if the event loop, dispatcher, or writer bypasses that helper. Build through the production constructor as well: a test-only constructor with different wakeup, ordering, or ownership rules proves nothing about the one production runs.
- For behavior changes, add a regression that demonstrates the defect before fixing it where practical. Distinguish compiler failures, printed reproductions, and failing behavioral assertions in the report.
- Map each acceptance requirement to an actual assertion. Include partial acknowledgements, wrong/stale IDs, A→B→A reassignment, coalesced changes, empty/tiny layouts, failure paths, and late cleanup where relevant.
- Preserve existing assertions when migrating fixtures. Send current-revision output when testing output handling; an ignored stale event cannot prove that output was processed safely.
- Prefer deterministic synchronization over sleeps for races. Force the disputed ordering or boundary; starting a thread does not prove it reached a lock.
- Use real PTYs, sockets, Git repositories, and process groups for claims about those boundaries. Query kernel PTY size with `MasterPty::get_size()` or `stty`; parser dimensions are different evidence. Verify actual child output, not command echo.
- Reuse existing test fixtures in their owning test binary. Confirm filters execute tests; zero tests is not a passing gate.
- Run focused checks while iterating, then the affected suites once when ready. Repeat only after relevant changes, failures, or a concrete unresolved concern.
- Save attempts under distinct filenames with the command, exit status, executed counts, and tested revision. Preserve failures; never overwrite them with a passing run.
- Separate headless, native macOS, Linux, clipboard, and provider evidence. A build or emitted OSC52 bytes does not prove native input, paste, or conversation restoration. Report unavailable checks as unverified.

## The live-server fixture

`tests/support/live.rs` builds the live servers the integration suites drive.
A `Live` owns a private temporary root and, inside it, the Git repository,
workspace root, registry file and socket its server is given; the wait for the
socket, control requests through `ovrcr_protocol::client`, "project and
workspace ready", the process groups the fixture owns and the cleanup that runs
from `Drop` all live there, so a suite writes none of it again.
Each binary fixture uses its own root as HOME and clears inherited provider/XDG
profile paths; explicit child overrides can point to task-owned provider fixtures.
The thread adapter shares the test process environment, so local regression runs
also require a private process HOME and provider profiles.

Concurrent `OuterDashboard` clients share one private executable copy while
their children are alive, avoiding startup delays from concurrent fresh images.
Each client keeps its fixture HOME, settings, socket and selected Server image;
custom executable fixtures retain their own copies. The last child owner removes
the shared image after reaping, and startup assets remain outside the Cargo target.

`Live::request` blocks until the server answers; `Live::bounded` — which `Live::binary` applies
to itself — gives every request a deadline instead, so a test fails rather than
hangs when its server stops answering. `Live::ready` creates the fixture
workspace on one branch and asserting a second branch is a test's mistake, not a
silent no-op. Two adapters
sit at its one seam, how the server is hosted: `Live::thread` runs `run_server`
on a thread of the test process, for tests that reach into server state or
change the environment the server reads; `Live::binary` spawns the compiled
`ovrcr`, for tests that need a real process — signals, exit status, a CLI
talking to a server it did not start. `Live::idle` hands back the paths without
a server, for tests whose subject is start-up itself. `Live::isolated_home` is
`idle` for a fresh install: the binary server and every `Live::command` run with
an empty private HOME and no `OVRCR_HOME`, so the default instance directory is the
one under test. Session, CLI and terminal helpers stay in the suite that needs
them, wrapped around a `Live`; `tests/support/claude_auth.rs` installs a
stand-in `claude` whose `auth status --json` answer a test can change while a
Server runs.

The settings document matrix (`crates/ovrcr-runtime/tests/settings_matrix.rs`)
is generated from the loader's own rows, so a new setting gets its missing,
default, non-default, wrong-type and misspelling cases without new test code.
A setting with an unset default, an enum, or a collection default the matrix
cannot extend names itself in the matrix's failure and needs one table entry.

## Hosted CI schedule

Pull requests and pushes to `main` run `macOS checks` and
`macOS offline Cursor preflight` on separate macOS runners. Each acceptance job
keeps its 45-minute limit; the standalone offline Cursor suite no longer shares
the Rust, packaging and installed-Pi job's budget.

The required `checks` job waits for both acceptance jobs. Its shell guard requires
both results to be `success`; a final step fails if the workflow was cancelled.
Failed, cancelled or skipped dependencies cannot satisfy it. This one-minute
macOS aggregate has no checkout or tools. Its job and guards use `always()` to
evaluate failures and cancellation; the acceptance jobs remain cancellable, so a
superseded pull-request run releases its expensive work normally.

The `linux`, `linux capacity` and `linux memory high-water` jobs run only on
manual dispatch: GitHub **Actions → CI → Run workflow**, then select the branch.
Manual dispatch also runs the macOS checks. Linux jobs and their assertions remain
intact; a skipped job is not evidence of passing Linux acceptance.

The `macOS checks` and `linux` test steps run under `cargo nextest` with the `ci`
profile from `.config/nextest.toml`: a failing test is retried twice and
reported as `FLAKY` instead of failing the job, and a test that hangs is
terminated after five minutes. A test that fails every attempt still fails the
job. `FLAKY` lines in the job log are the list of tests to harden; retries hide
the red, not the defect. Locally, `cargo nextest run --profile ci` reproduces
the CI behavior once nextest is installed.

### Linux and headless agents

Linux agents (Cursor Cloud, CI runners) are headless. The GUI feature has no
Linux windowing backend, so match the `linux` job in `.github/workflows/ci.yml`
and omit `--all-features`.

- Rust stable, at least the workspace `rust-version` (1.97.1), plus rustfmt and
  clippy. A default cloud image may be older.
- `zsh` is required. Shell integration tests invoke `/bin/zsh`.
- Node 22 runs `node --test tests/pi_reporting_extension.mjs
  tests/omp_reporting_extension.mjs tests/pi_delegation_extension.mjs`.
- `cargo nextest` uses the `ci` profile in `.config/nextest.toml`.
- Nothing starts on boot. Drive the CLI with an isolated `OVRCR_HOME` and
  `OVRCR_SOCKET`.

Container runners also need an unprivileged test user, a reaping init (Docker
`--init`), `SHELL` pointing to an installed shell, and a writable disposable
checkout. Root bypasses permission-denial cases; a non-reaping PID 1 retains
orphan zombies; relative-socket tests bind in the package working directory.
A private source copy keeps these writes separate from the host checkout.

## Commands and isolated acceptance

Operator activity regressions exercise derived state/cadence in the owning TUI
unit suite, clock-controlled `draw_dashboard_at` through the public Dashboard,
and `compiled_worker::operator_activity_waits_for_real_model_and_tool` in
`operator_e2e` with `acceptance-diagnostics`. The latter holds a real Submitted
receipt, a silent acknowledged model request and an actual Bash result, then
releases each gate once through private synthetic OAuth and loopback inference.
Native no-message frame pairs separately prove idle redraw; fixed TestBackend
timestamps alone cannot prove event-loop animation.
The [initial activity record](../../research/operator-activity-2026-10-30/native-01/README.md)
proves silent native animation; its later lock remains recorded. The
[unlocked correction](../../research/operator-activity-2026-10-30/native-02/README.md)
adds actual pending-tool movement with Operator collapsed, completed indicator
absence and normal CUA close on the same frozen product revision.
The [inline activity and message-input acceptance](../../research/operator-inline-input-2026-10-30/README.md)
captures activity after message/tool entries, history scroll-away and retained
viewport through completion, full/Quick Unicode edits, selected Ctrl+C Stop,
independent response survival and ordinary terminal Ctrl+C on matching native
artifacts. Known-repeat/frozen-receipt/overlay permutations remain explicitly
separate automated evidence.

For Rust changes, choose the owning package and add a focused test filter as appropriate:

```sh
cargo test -p ovrcr-tui --lib
cargo test -p ovrcr --test tui
cargo check --workspace --all-targets --all-features
cargo fmt --all -- --check
git diff --check
node --test tests/pi_reporting_extension.mjs
```

Node 22; the Pi extension and its host run outside cargo.

On macOS, the Cursor dependency packaging check exercises the real installer and
a relocated CLI without executing the bridge or reading credentials. Supply the
pinned host archive and the built CLI explicitly (CI downloads that exact official
release into an isolated home first):

```sh
OVRCR_CURSOR_BRIDGE_ARCHIVE=/absolute/official/archive.tar.gz OVRCR_TEST_EXECUTABLE=/absolute/ovrcr python3 -I tests/cursor_bridge_install.py
```

This verifies passive setup/doctor and installation/refusal behavior only. It
cannot close the upstream local-agent smoke, native cloud acceptance or capability
gates in [Cursor cloud](../cursor-cloud.md).

At the feature acceptance checkpoint, run the assigned gates and the workspace regression/lint checks; do not repeat them after every edit:

```sh
cargo test --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- Keep documentation-only verification proportional; do not run Rust suites for prose edits.
- Give every live fixture its own `OVRCR_HOME`, `OVRCR_SOCKET`, and temporary workspace. Use the required execution permission for PTY/socket/process tests; sandbox denial is not an application failure.
- Follow [Computer-use testing](../testing-computer-use.md) for `just gui`. Use the exact checkout under review and fresh screenshots/accessibility state.
- Record owned fixture PIDs/PGIDs and paths, retain the launcher result, and verify cleanup. Permission denial does not prove a process group is gone. Stop a hung owned test before starting another.
- Never bypass a tool's access denial through another control channel or modify the user's live agent configuration to make an acceptance check pass.
- Interactive Dashboard fixtures can encounter optional native-hook or Bridge setup offers. Decline recognized optional offers with `n`, leave unknown decisions unanswered, and wait for the actual alternate-screen Dashboard before sending test input. Preserve fresh-output assertions separately from waits for already-rendered state. The development GUI retains a single Cmd-K intent across dashboard restart; tests must not resend the shortcut to compensate for startup.
- The shared initial-admission lifecycle cases can fail with `supervisor must select a fresh UUID` when a saturated host pushes the fixture's `/bin/sh` reply past the product's one-second probe budget; read a recurrence as contention unless its evidence says otherwise, per the [issue #62 diagnosis](../../research/issue-62-lifecycle-diagnosis/README.md).

## Standalone Cursor cloud client

The offline-only client suite uses synthetic credentials, a controlled external bridge
process and real private files/sockets/Git repositories. It covers preflight, a
coding run with exact-run observation recovery, same-conversation follow-up,
third-run cancellation and optional read-only owned-bridge restart, including
uncertain receipts, source/result verification, original deadlines and closure. It creates no cloud work and requires no
OVRCR Server or live account. Run `python3 -I -m unittest discover -s experiments/cursor-cloud -p test_client.py -v`.
See [invocation and acceptance mapping](../../experiments/cursor-cloud/README.md)
for deadlines, ownership receipts and the separately gated live proof.

## Git owner acceptance receipts

With `acceptance-diagnostics`, a private fixture can set
`OVRCR_GIT_DIAGNOSTICS_DIR` to a fresh, owned, absolute directory with mode
0700 and `OVRCR_GIT_DIAGNOSTICS_RUN` to a nonsecret identifier of at most 64
ASCII letters, digits, underscores, or hyphens. The fixed `git-owner.jsonl`
file is created exclusively with mode 0600. Without the feature or opt-in,
Git keeps its original `Command.output` and `Command.status` paths.

The journal records construction sequence, original owner and child identity
(PID, parent, actual group, birth), original wait status or signal, pipe drain,
and health. It excludes arguments, environment, working directory, output,
and error bodies. Bounds are 8192 calls, 8 MiB, 1024 bytes per row, and a
512-byte terminal-seal reserve. Identity, overflow, and write failures remain
sticky; recording failures never skip the original wait or change the Git
result. Diagnostics add no signals, timeouts, or retries. Final diagnostic
health is returned only after the terminal write and sync. Opted-in Server
shutdown returns an error if that health is unqualified, preserving any
existing task-shutdown error first. Feature-off/no-opt-in Server behavior
is unchanged. A healthy-looking seal alone cannot attest its own final sync:
qualification requires both the healthy journal and the original opted-in
Server wait with status 0, never status inferred from kernel absence.

Normal Server shutdown writes a terminal seal only after its existing owner
joins. Pending calls, early shutdown errors, missing rows, and unhealthy seals
remain unqualified. The private CLI fixture must drain and reap its clients
and native owners before Server shutdown and retain its original Server wait.
These receipts do not prove arbitrary descendant or process-group closure;
nested Git roles and unmatched hooks need their separate lifecycle evidence.

The terminal call is sole and last after the existing owner joins. The fixture
must have drained/reaped all clients and native owners first; it must not admit
new Git work after that boundary. Late begin or duplicate finalization is
unhealthy, even when failed persistence leaves prior healthy seal bytes.
This fixture prerequisite does not add joins or general client containment.

## Built-in Operator acceptance

The [Operator reference](operator.md) documents its feature-gated loopback model
endpoint. The fixture signs in through a local OAuth server and normal credential
storage; it neither imports personal accounts nor bypasses authentication.

```sh
rtk cargo test -p ovrcr --test operator_auth
rtk cargo test -p ovrcr --test operator_worker
rtk cargo test -p ovrcr --test operator_e2e --features acceptance-diagnostics
rtk cargo test -p ovrcr --test operator_scope
```

The E2E fixture executes the compiled worker, real Bash/CLI, authenticated framing,
Server journal and native Dashboard actions. It asserts concurrent streams,
independent Stop, context resume without command replay, real timeout-child
termination, missing-auth retirement and incomplete-response rejection. Explicit
ignored fixture entrypoints are invoked and checked by their owning parent or
native acceptance; they are not passing product cases merely because ignored.
Keep actual native screenshots/accessibility and live subscription acceptance
separate from loopback evidence.

Layout acceptance also holds real Create/Submit receipts outside the Dashboard to
prove a dismissed quick message keeps its captured text and context, submits once,
and preserves terminal focus while another chat completes. The close/archive case
records two real Bash child groups, closes A, completes B and receives terminal
output, then restarts and restores A without replay before an explicit new turn.
The outer-PTY `operator_quick` case sends Ctrl+O, CSI-u Shift+Enter and bracketed
paste through the compiled Dashboard, checks separate editor lines, dismisses to
the original terminal, and verifies no conversation was created.
See the [2026-10-30 layout acceptance record](../../research/operator-layout-2026-10-30/README.md)
for exact revisions, native JPEG/AX evidence, commands, failures and platform gaps.
The [Working journal native acceptance record](../../research/operator-working-journal-2026-10-30/README.md)
adds matching-source journal/receipt/quick captures, the native 80×24 frame and
kernel-confirmed nested 60×24/60×16 frames, with original owner waits and gate limits.

The [2026-10-29–30 Operator acceptance record](../../research/rig-operator-2026-10-29/README.md)
contains real ChatGPT GUI turns and a separate controlled concurrent-chat run,
with executable hashes, cleanup receipts and remaining gate limitations.

SOURCE LOOKUP {"path": "docs/testing-computer-use.md"}
# How to test OVRCR with computer use

Use this macOS smoke check after changes to the dashboard, terminal input, or CLI session controls. Run it in the checkout being reviewed. It complements the automated tests.

## 1. Launch and connect

```sh
rtk proxy just gui
```

Keep the command session open to collect its exit status and cleanup errors. The helper builds `target/OVRCR GUI.app` and creates a disposable server with two project roots and four feature workspaces: two initial root shells, four feature shells, and six fixture agents (twelve sessions). Each root is on `main` with a root marker. Feature workspaces are labeled by branches such as `gui-consigint-auth-handoff`.

With the Codex computer-use tool, initialize in a separate call using the actual checkout path:

```js
var app = await cua.getApp("/absolute/checkout/target/OVRCR GUI.app");
```

Read the returned tool documentation before continuing. Then inspect the window:

```js
await app.getAXStateAndScreenshot();
```

Use fresh accessibility indices or coordinates from the current screenshot. Sidebar row accessibility bounds can span the terminal: click within the visible sidebar or use `j`/`k`. Request the needed execution permission if local socket, PTY, or GUI access is sandbox-blocked; record a blocked check if access remains unavailable.

### Driving the window

Use Codex single-shot with the model configured in `~/.codex/config.toml`;
do not pass `-m`:

```sh
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN codex exec -s workspace-write --skip-git-repo-check - < prompt.md
```

On 2026-10-02 (codex-cli 0.155.1, `codex login status`: "Logged in using
ChatGPT", `CODEX_HOME` unset, model `gpt-6.1-sol`) every call failed with
"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT
account", with or without the `OVRCR_*` variables
(`research/quota-display-2026-10-02/codex-cua-attempt.txt`). No working Codex
invocation is recorded yet. Until one is, record that failure and fall back to
the scripted System Events harness in `research/quota-display-2026-10-02`:
`drive.py` sends real key events to the helper process by PID and checks focus in
the same AppleScript call as each key, so a key never lands in another
fixture's window; `winid.swift` finds the window for `screencapture -l`, and
the helper's accessibility static text is saved beside each screenshot. When
another worktree drives its own fixture at the same time, run each step only
after its driver has gone idle (`research/settings-editor-2026-10-02/run-step.sh`),
so neither run steals focus mid-step. The
helper's window cannot be shorter than 22 rows. To check layouts shorter than
that, run a second isolated `ovrcr` Dashboard in a fixture shell after
`stty rows N` (`nested-ladder.sh`).

## 2. Prove real input and rendering

Select `consigint / gui-consigint-auth-handoff / local`, then press Return to enter terminal mode. Paste this command through the GUI and press Return:

```sh
rtk proxy printf 'CUA_%s\n' INPUT_OK
```

Confirm a separate output line containing `CUA_INPUT_OK`; the echoed command alone is insufficient. Inspect both the screenshot and accessibility text for readable rows, correct selection, and overlapping or clipped content. Resize the window and check the layout again. `Ctrl-g` returns to browse mode; `j`/`k` select sessions and Return resumes terminal input.

Check Unicode in the same shell:

```sh
rtk proxy printf '\033[31mRED 界🙂 END\033[0m\n'
```

Confirm readable red ASCII/CJK and a visible emoji, not replacement squares. The macOS helper uses native fallback artwork for missing glyphs, including color emoji. Compare the screenshot with accessibility text; correct text alone does not prove rendering. Repeat after opening a split with `Ctrl-g`, `v`, then Enter, after resizing, and after reattachment. Check wide-glyph spacing, cursor position, and clipping at the pane boundary.

Check modified, composed, and dead keys in the same shell. Put the PTY in raw
mode so the line editor cannot consume the escapes:

```sh
rtk proxy stty -icanon -echo min 1
rtk proxy cat -v
```

Press Ctrl-Left, Shift-Enter, Alt-Backspace, and Option-b. Confirm `^[[1;5D`,
`^[[13;2u`, `^[^?`, and `^[b`; Option-b must print the meta form, never the
composed glyph.

Then press Option-e, a US-layout dead key, and record exactly what arrives. This
is an open question, not a known expectation: the helper publishes an IME area, so
winit's macOS `keyDown:` runs `interpretKeyEvents` first, and a dead key
(Option-e, `i`, `u`, `n`, or backtick) calls `setMarkedText:` and suppresses the
`KeyboardInput` event, meaning no `Key::E` reaches the helper. So Option-e may
start a composition (no bytes, an accent pending) where Option-b sends `^[b`.
Report which happens, and whether the following key commits an accented character
into the shell. If dead keys compose, Meta for those five keys needs a decision:
either the composition is correct for this helper or `option_as_alt` must be set.
Press Ctrl-C, then restore the shell:

```sh
rtk proxy stty sane
```

At the restored prompt, select a CJK input method and type a composition (with
Pinyin, `ce` then Space). Confirm only the committed characters reach the command
line and that no pre-edit letters leak into it. Check the candidate window's
position against the screenshot: it must sit at the cursor cell, not merely
somewhere over the terminal, because macOS places that panel from the single rect
the helper publishes. Record the input method used.

**IME composition: unverified as of 2026-09-08.** This machine enables only the
U.S. keyboard layout and the Character Palette (`defaults read
com.apple.HIToolbox AppleEnabledInputSources`), and no desktop computer-use tool
was available to drive the native window, so the composition, dead-key, and
candidate-window checks above were not run against the helper. The modifier and
meta encodings they exercise are covered by the unit tests
`special_keys_carry_modifiers`, `alt_letter_encodes_meta`, and
`unmodified_letter_keeps_its_text` in `src/gui/input.rs`; an automated egui test
cannot prove composition or panel placement, so the IME path still needs this
native check.

Return to browse with `Ctrl-g`. Press Space. Confirm a compact bottom-right
list above the footer, with the surrounding dashboard still visible. Press
`t` and confirm the terminal submenu names the selected session and shows
`Space t`. Press Backspace and confirm the group list returns. Press Escape,
then `?`, then Enter to open the Terminal group. Confirm Pause is listed for a
running session and Resume is absent. Disabled actions should show a reason.
Press Backspace, then `w`, then `x`; confirm the workspace picker highlights
the selected `consigint / gui-consigint-auth-handoff` and omits the
repository-root workspace. Type to filter to a different workspace, then press Enter; confirmation must name the chosen target.
Escape cancels. Repeat with `Space p x` for projects, then `Space t x` for direct
terminal-close confirmation, cancelling both. Open tasks with `Ctrl-t`, press
`n`, and Tab to Project; check the same filtering, Up/Down, and Tab/Enter controls.
Escape cancels the editor, then Escape returns to the dashboard. Click project and workspace sidebar rows and
confirm `Space` shows only their applicable groups. Resize the window and scroll to
the last entry; confirm it remains visible and clickable. Press Escape.
Press Space then `w` then `n`; confirm the key popup closes and the terminal form opens.
On Start, choose Terminal. Confirm Workspace shows the selected `consigint / gui-consigint-auth-handoff`, Name is optional and blank, and Command is optional. Submit with both blank. Confirm terminal mode on the new session, then paste this command and press Return:

```sh
rtk proxy printf 'CUA_%s\n' N_SHELL_OK
```

Confirm a separate output line containing `CUA_N_SHELL_OK`; the echoed command alone is insufficient.

Return to browse with `Ctrl-g`, press `w`. Confirm there is no Name field.
Branch starts as `feature/` and is focused; typing appends. Type
`cua-workspace` so Branch reads `feature/cua-workspace`. Base shows the
repository's default branch once suggestions finish loading. Choose Start:
Terminal. Enter on Branch submits. Confirm a single new terminal is selected
in terminal mode under `feature/cua-workspace`. Paste and execute:

```sh
rtk proxy printf 'CUA_%s\n' W_SHELL_OK
```

Confirm a separate `CUA_W_SHELL_OK` output line. Record this shell's PID and
process group with the other fixture processes before cleanup. Return to
`consigint / gui-consigint-auth-handoff / local` for the checks below.

Return to browse with `Ctrl-g`. Press `a`. Tab through a configured `picker_roots` directory to the demo repository (a `git` marker sorts it first). Confirm Name is the repository basename and Workspace root is `<config dir>/workspaces/<name>`. Escape without submitting if this run should not register a project.

### Workspace launch and titles

In the disposable fixture, also verify:

1. Create a workspace with Nothing yet from Branch only (no Name field); confirm it is selected and has no sessions.
2. Create a workspace with Terminal and a custom command that prints a unique
   marker and waits for input. Confirm exactly one session and its actual output.
3. In that session emit `printf '\033]2;CUA title 界\007'` followed by a unique
   output marker; confirm the output appears but sidebar and pane titles keep
   the original session name. Repeat with OSC 0 and a different title.
4. Set a title using Rename terminal; emit another app title and confirm the
   manual title holds. Clear Rename and confirm the original name returns and
   stays unchanged after further application title updates.
5. Give two sessions the same manual title and confirm their visible identifiers
   distinguish them. Confirm both still receive input in the intended pane.
6. Exercise an owned failing launch after workspace creation. Confirm the
   workspace remains and recovery starts a program there without recreating it.
7. Exercise Agent using a disposable configured preset, recording that this proves
   the launch UI only, not provider reporting. Verify its project preference after
   dashboard restart. Verify Terminal custom command text is absent from settings.
8. Exit an owned session. Verify Reopen explicitly asks you to confirm that old
   agent/background processes stopped; cancel once and verify no new process.
   After independently verifying the fixture processes are gone, confirm.
   Check the same row/title, absence of old output, and a separate fresh-shell
   execution marker. An older capture without this acknowledgement is not
   evidence for the current recovery policy.
9. Select the `feature/cua-workspace` session created above, then run
   `rtk proxy git checkout -b cua-switched`. Confirm the workspace row becomes
   `cua-switched` without replacing that session. Paste a unique printf in the
   same shell and confirm its output. The old branch label must not remain as
   that workspace's name.
10. Select consigint's root workspace (`main` with the root marker). From there,
    `w` then `x` must be unavailable (`repository-root workspace cannot be
    removed`). In its `local` shell, run `rtk proxy git checkout -b cua-root-drift`.
    Confirm the root warning, that `n` is unavailable (`root workspace is not on
    the default branch`), and that the existing shell still accepts a unique
    printf. Repeat the removal check while drifted. Restore with
    `rtk proxy git checkout main`, confirm `n` is available again, and leave
    the root on `main`.

For a deliberate server-loss check, record the fixture paths and owned processes
first. Verify restored rows without launching processes, then explicitly confirm
that old agents and background processes have stopped before reopening a shell.
A host reboot is not required: `retained_ownership` tests deterministic boot-ID
transitions and independently checks the native reader on macOS and Linux.

The GUI helper deliberately preserves its fixture after an unexpected server
exit. Record that cleanup-guard result separately. Shut down any replacement
server through the fixture socket, verify every owned process group is gone,
stop only the task-owned GUI launcher, and remove only its recorded disposable
root. Do not weaken the normal cleanup guard to make a crash check pass.

### Other-running-agent search

Use two explicitly Agent-launch fixtures in different disposable projects or
workspaces. The helper's labeled shell fixtures still have Terminal launch kind
and must not count as search destinations. Use a disposable configured Agent
preset or the production `CreateSession` request with an input-acknowledging
shell command; never start a paid provider just to populate this check.

From Browse, press lowercase `s`; also check `Space v s`. Confirm Agents lists
the other Running agents and shows the selected project/workspace. Filter using
words from a different project and session title. Press Enter once and confirm
Terminal mode after the matching screen is ready, without a second Enter. Send
a unique marker and confirm a separately
printed acknowledgement only in the chosen Agent, not the previous PTY.
From Terminal, press `Ctrl-g` then `s`; confirm the current Agent is omitted.
Escape must preserve the selection in Browse. Pause an owned candidate while
the picker is open, then confirm it: the unavailable reason must remain visible
and no neighboring row may be selected. Resume it; it stays unavailable until
reopening. While a view is loading, input and paste must be rejected rather than
replayed; cancel pending typing with `Ctrl-g` and verify a late completion stays
in Browse. Repeat selection into an Agent already in the other pane and confirm
it is focused without duplication. Keep screenshots and matching accessibility
text for these outcomes. The outer-PTY acceptance fixture deterministically
holds a real view's final acknowledgement to prove loading-input rejection,
including a pending keyboard/paste burst released across acknowledgement cutover.
The event-loop regression forces acknowledgement-before-input ordering and a
multi-batch input drain; Terminal mode begins only at the quiet input boundary.

For attention-first presentation, publish accepted fixture observations through
the existing reporting protocol: Waiting Input, Busy plus Unread, reviewed Ready,
error and unavailable reporting. Verify that only Waiting Input and otherwise
Unread are promoted, and that a current attention-bearing Agent is excluded.
While the picker remains open, change an observation and confirm visible status
updates without moving its highlight or rows; reopening must use the new order.
Check duplicate titles, compact selected location and offscreen filtering at the
fifty-session limit. Preserve screenshots, accessibility text and destination
acknowledgements; opening or selecting must not clear Unread or resolve Input.

## 3. Mouse forwarding and wheel history

### Dashboard controls

Run these checks with the mouse for navigation; use the keyboard only to enter
text. Save a screenshot and accessibility text after each changed view. Repeat
the form and menu checks in a short window to exercise scrolling and clipping.

1. Open the action menu with its visible control and open a second terminal pane.
   Click each pane and run `rtk proxy printf 'MOUSE_%s\n' LEFT_OK` or
   `rtk proxy printf 'MOUSE_%s\n' RIGHT_OK`. Confirm distinct output in the
   intended pane. Switch directly from an active terminal without `Ctrl-g`.
2. Drag the sidebar border and confirm the pane narrows and widens with it.
   Drag the divider left and right. In each pane run `rtk proxy stty size` and
   compare the reported columns with the drawn terminal width. Make the window
   too narrow for both panes, widen it again, and confirm assignments and the
   chosen split survive. Release the drag outside the terminal and confirm the
   next click works normally.
3. Click a project or workspace name to select it without collapsing it. Click
   the first cell of a project header, or the branch glyph of a workspace, to
   collapse and expand it. Scroll the sidebar and select a row that was
   initially offscreen. Confirm the selected target by its name.
4. Open a creation form from the visible action menu. Click a nonactive field,
   edit in the middle of its value, choose a pick-list option, and operate any
   toggle. Browse a directory through the path picker. Use Cancel, reopen, and
   submit a disposable terminal; prove input with a separate `MOUSE_FORM_OK`
   output line. Cancel a destructive confirmation and confirm its target remains.
5. Open Tasks using mouse controls. Select and scroll its rows, open an editor,
   click fields and a project choice, and cancel. Exercise task controls only on
   a task owned by this fixture; inspect confirmation targets before proceeding.
6. Print a line containing `ASCII 界🙂 tail`, open Copy from the action menu, drag
   across the text, and use Copy. Paste into an owned fixture input and compare
   the selected text. Open History, scroll, select text, and close it through its
   visible control. Confirm new live output does not rewrite the frozen view.
7. While a form or menu is open, click outside it over a terminal. Confirm the
   click does not reach the terminal. Repeat a focus-changing click with Vim
   mouse tracking enabled: the click selects the other pane without moving that
   application's cursor. A subsequent click inside the focused Vim pane should
   reach Vim as described below.

Keep these observations separate from automated test results. If the computer-use
tool cannot perform a drag or inspect the clipboard, mark that check unverified
and retain any available automated evidence. Selection highlighting and emitted
OSC52 bytes do not prove clipboard delivery; test paste in a terminal that handles
OSC52 when the GUI helper cannot deliver it.

### Application forwarding

Stay on `consigint / gui-consigint-auth-handoff / local` in terminal mode. Move the pointer over the terminal pane, not the sidebar or footer, and scroll the wheel up. History should open at the tail (`HISTORY` in the footer). Wheel down at the newest row returns to the live prompt.

In the same or another live pane, run `vim -n -u NONE`, then `:set mouse=a`. Click in the buffer: the Vim cursor moves to that cell. Wheel should scroll Vim; History must stay closed. `:set mouse=` then wheel up while Vim is still running must not open History (alternate screen). Quit Vim before repeating the plain-shell History check.

Computer-use tools often cannot hold a mouse button across Ctrl-g. Skip that gesture here; the TUI test `ctrl_g_releases_held_buttons_before_switching` covers it.

…
Report the tested commit, GUI checks and observed markers, cleanup result, and any blocked checks. Keep automated test results separate from computer-use evidence. A successful build or screenshot alone does not prove terminal input or lifecycle behavior.

[Showing lines 1-300 of 346. Use :301 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr/tests/*.rs;tests/*.rs;tests/support/*.rs"}
# tests/
terminal_acceptance.rs
task_tui.rs
service.rs
server_lifecycle.rs
provider_quota.rs
pi_delegation.rs
operator_worker.rs
operator_scope.rs
operator_e2e.rs
operator_auth.rs
gui.rs
cursor_cloud.rs
client_startup_server.rs
cli.rs
agent_setup.rs
claude_reporting_load.rs
workspace_identity.rs
settings_lifecycle.rs
retained_sessions.rs
retained_capacity.rs
resource_cli.rs
pi_reporting.rs
pi_package.rs
automatic_titles.rs
quota_schema.rs
codex_reporting.rs
git_lifecycle.rs
client_startup.rs
claude_quota_probe.rs
wip_save.rs
task_schedule.rs
task_paging.rs
task_execution.rs
task_cli.rs
retained_ownership.rs
registry_migration.rs
quota_ui_option_a_smoke.rs
claude_reporting.rs
task_runner.rs
## support/
operator_provider.rs
live.rs
dashboard_proxy.rs
claude_auth.rs
deadline.rs
mouse_app.rs

Skipped missing paths: crates/ovrcr/tests/*.rs

SOURCE LOOKUP {"path": "crates/*/src/dashboard/*.rs"}
# crates/ovrcr-tui/src/dashboard/
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

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/agents.rs"}
use super::settings::AgentOverride;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const KNOWN_AGENTS: [&str; 12] = [
    "claude",
    "codex",
    "grok",
    "gemini",
    "aider",
    "opencode",
    "pi",
    "omp",
    "goose",
    "amp",
    "cursor-agent",
    "hermes",
];
/// Detected agents OVRCR launches through `agent run <name> --` so managed reporting
/// (Grok retains history for titles; Hermes has process supervision only). Claude, Codex,
/// Pi, and Oh My Pi defaults also carry that provider's auto-trust flag. Explicit custom
/// overrides keep their stored argv unchanged; a raw shell command is never adopted silently.
pub const MANAGED_AGENTS: [&str; 7] = [ … ];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSource { … }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEntry { … }

pub fn detect_agents(
    path: &OsStr,
    shell: Option<&OsStr>,
    launcher: Option<&Path>,
) -> Vec<AgentEntry> { … }

pub fn apply_overrides(
    mut entries: Vec<AgentEntry>,
    overrides: &[AgentOverride],
) -> Vec<AgentEntry> { … }

fn executable_in(dir: &Path, name: &str) -> Option<PathBuf> { … }

#[cfg(test)]
mod tests { … }

[…376ln elided; re-read needed ranges, e.g. crates/ovrcr-tui/src/dashboard/agents.rs:24-32,35-39]

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

SOURCE LOOKUP {"pattern": "enum Page|Launch|AgentEntry|SessionLaunch|CreateSessionRequest", "path": "crates"}
# crates/

## ovrcr-protocol/src/
### catalog.rs
 6|use crate::{
*7|    DESKTOP_NOTIFICATIONS_OFF, ITERM_FOCUS_OFF, LaunchChoice, QUOTA_OFF, ReadySoundChoice,
 8|    SAVE_UNCOMMITTED_WORK_OFF, SettingOwner, SettingRow, SettingSource, Settings, TITLES_OFF,
 9|    ThemeId,
 10|};
...
 54|    /// A map of `{ kind, preset }` tables.
*55|    Launches,
 56|}
 57|
 58|const AGENT_FIELDS: &[&str] = &["name", "argv"];
...
 124|        Shape::Agents => Some(AGENT_FIELDS),
*125|        Shape::Launches => Some(LAUNCH_FIELDS),
 126|        _ => None,
 127|    }
 128|}
...
 167|            }
*168|            (Shape::Launches, []) => SettingWrite::Typed("a table of { kind, preset } tables"),
*169|            (Shape::Launches, [SettingName::Key(_)]) => {
 170|                SettingWrite::Entry(LAUNCH_FIELDS, "an inline table { kind, preset }")
 171|            }
*172|            (Shape::Launches, [SettingName::Key(_), SettingName::Key("kind")]) => {
 173|                SettingWrite::Text("\"Terminal\" or \"Agent\"")
 174|            }
*175|            (Shape::Launches, [SettingName::Key(_), SettingName::Key("preset")]) => {
 176|                SettingWrite::Text("a string")
 177|            }
 178|            _ => return None,
...
 217|
*218|fn display_choices(choices: &BTreeMap<String, LaunchChoice>) -> Option<String> {
 219|    #[derive(Serialize)]
 220|    #[serde(tag = "kind", content = "preset")]
 221|    enum Spell<'a> {
...
 228|            let spell = match choice {
*229|                LaunchChoice::Terminal => Spell::Terminal,
*230|                LaunchChoice::Agent(preset) => Spell::Agent(preset),
 231|            };
 232|            (project.as_str(), spell)
 233|        })
...
 486|        path: "launch_choices",
*487|        shape: Shape::Launches,
 488|        owner: SettingOwner::Dashboard,
 489|        group: "Remembered launches",
 490|        label: "Remembered launches",
...
 938|        let mut launch_choices = BTreeMap::new();
*939|        launch_choices.insert("my.project".into(), LaunchChoice::Agent("claude".into()));
 940|        let settings = Settings {
 941|            desktop_notifications: true,
 942|            ready_sound_choice: Some(ReadySoundChoice::Chime),
### delegation.rs
 289|    Prepared,
*290|    LaunchUnknown,
*291|    Launched,
 292|    InputUnknown,
 293|    Applied,
 294|    Refused,
...
 308|    Prepare,
*309|    Launch,
 310|    InitialInput,
 311|    Followup,
 312|    Takeover,
...
 761|    },
*762|    Launch {
 763|        auth: DelegationAuth,
 764|        handle: String,
 765|        identity: AssignmentIdentity,
### session.rs
 6|
*7|/// Generation of a retained session row. Launch, archive and unarchive invalidate
 8|/// prior process ownership and requests. Distinct from scheduled-task [`crate::RunId`].
 9|#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
 10|pub struct SessionRunId(pub u64);
### settings.rs
 1520|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*1521|pub enum LaunchChoice {
 1522|    Terminal,
 1523|    Agent(String),
 1524|}
...
 1674|    pub cursor_cloud_accounts: Vec<CursorCloudAccount>,
*1675|    pub launch_choices: BTreeMap<String, LaunchChoice>,
 1676|    pub quota: QuotaSettings,
 1677|    #[serde(default)]
 1678|    pub delegation: DelegationSettings,
### wire.rs
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
 211|    },
*212|    CreateSession(CreateSessionRequest),
 213|    RemoveSession {
 214|        session: SessionId,
 215|    },
...
 285|    },
*286|    CreateWorkspaceWithLaunch {
 287|        project: String,
 288|        id: String,
 289|        branch: BranchRequest,
*290|        launch: Option<SessionLaunch>,
 291|    },
 292|    SetSessionTitle {
 293|        session: SessionId,
...
 498|    CloseTerminal,
*499|    CreateWorkspaceWithLaunch,
 500|}
 501|
 502|/// Result of a finished Lifecycle job, correlated by `client_token`.
...
 605|            .launch_choices
*606|            .insert("p".into(), crate::LaunchChoice::Agent("a".into()));
 607|        settings
 608|            .launch_choices
*609|            .insert("q".into(), crate::LaunchChoice::Terminal);
 610|        // Keep the wire fixture's settings values explicit; quota collection
 611|        // is enabled by default for real settings documents.
 612|        settings.quota.enabled = false;
...
 636|    /// `deserialize_any` (an internally tagged enum, for one) would encode
*637|    /// and then fail to decode. The adjacently tagged `LaunchChoice` must not.
 638|    #[test]
 639|    fn settings_report_round_trips() {
 640|        let report = settings_report();
...
 778|                "CreateSession",
*779|                Request::CreateSession(CreateSessionRequest {
 780|                    project: "a".into(),
 781|                    workspace: "b".into(),
 782|                    name: "c".into(),
...
 906|            (
*907|                "CreateWorkspaceWithLaunch",
*908|                Request::CreateWorkspaceWithLaunch {
 909|                    project: "a".into(),
 910|                    id: "b".into(),
 911|                    branch: BranchRequest::Existing { branch: "c".into() },
*912|                    launch: Some(SessionLaunch {
 913|                        argv: vec!["sh".into()],
 914|                        label: None,
 915|                        kind: SessionKind::Terminal,
...
 919|            (
*920|                "CreateWorkspaceWithoutLaunch",
*921|                Request::CreateWorkspaceWithLaunch {
 922|                    project: "a".into(),
 923|                    id: "b".into(),
 924|                    branch: BranchRequest::Existing { branch: "c".into() },
...
 1601|            (
*1602|                "Launch",
*1603|                crate::DelegationRequest::Launch {
 1604|                    auth: auth.clone(),
 1605|                    handle: "r".into(),
 1606|                    identity: identity.clone(),
...
 3356|        (
*3357|            "Request::CreateWorkspaceWithLaunch",
 3358|            "1f016101620101630101000273680000",
 3359|        ),
 3360|        (
*3361|            "Request::CreateWorkspaceWithoutLaunch",
 3362|            "1f0161016201016300",
 3363|        ),
 3364|        ("Request::SetSessionTitle", "200101057469746c65"),
...
 3646|        (
*3647|            "Delegation::Launch",
 3648|            "3201010766697874757265000001720169017701630101740101017201020101",
 3649|        ),
 3650|        (

## ovrcr-runtime/

### src/
#### cursor_cloud.rs
 20|#[derive(Debug, PartialEq, Eq)]
*21|pub struct CloudLaunchPreview {
 22|    pub account: String,
 23|    pub repository: String,
 24|    pub source_sha: String,
...
 26|}
*27|impl std::fmt::Display for CloudLaunchPreview {
 28|    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
 29|        write!(
 30|            f,
...
 97|    fixture: Option<&Value>,
*98|) -> Result<CloudLaunchPreview> {
 99|    profile.validate()?;
 100|    let origin = git(workspace, &["remote", "get-url", "origin"], deadline)?;
 101|    let repository = canonical_cloud_repository(&origin).context("unsupported workspace origin")?;
...
 130|    );
*131|    Ok(CloudLaunchPreview {
 132|        account: profile.name.clone(),
 133|        repository,
 134|        source_sha: sha,
#### extension_recovery.rs
 1|//! Native Pi and Oh My Pi recovery. Identity comes from SessionManager, never
*2|//! filenames or most-recent lookup. Launching remains the shared server's job.
 3|use anyhow::{Context, Result, bail};
 4|use ovrcr_protocol::{AgentProvider, ExtensionConversation, validate_agent_id};
 5|use std::{

#### server/
##### connections.rs
 768|                request.as_ref(),
*769|                ovrcr_protocol::DelegationRequest::Launch { .. }
 770|            );
 771|            state.delegation(*request).map_or_else(
 772|                |error| {
...
 986|        ),
*987|        Request::CreateWorkspaceWithLaunch {
 988|            project,
 989|            id: name,
 990|            branch,
...
 994|            request_id,
*995|            lifecycle::JobKind::CreateWorkspaceWithLaunch {
 996|                project,
 997|                id: name,
 998|                branch,
...
 1001|        ),
*1002|        Request::CreateWorkspaceWithLaunch {
 1003|            project,
 1004|            id: name,
 1005|            branch,
##### delegation.rs
 149|            && self
*150|                .receipt(DispatchOperation::Launch)
 151|                .is_some_and(|receipt| receipt.outcome == DispatchOutcome::Applied)
 152|            && self
 153|                .receipt(DispatchOperation::InitialInput)
...
 193|            DispatchStatus::Prepared => !allocated && self.binding.is_none(),
*194|            DispatchStatus::LaunchUnknown => self.binding.is_none(),
*195|            DispatchStatus::Launched => {
 196|                allocated && (self.binding.is_none() || self.pending_initial_binding())
 197|            }
 198|            DispatchStatus::InputUnknown | DispatchStatus::Applied => {
...
 395|                DispatchOperation::Prepare
*396|                    | DispatchOperation::Launch
 397|                    | DispatchOperation::InitialInput
 398|            ) {
 399|                return None;
...
 411|        }
*412|        let launch = self.receipt(DispatchOperation::Launch).map(|r| r.outcome);
 413|        let initial = self.receipt(DispatchOperation::InitialInput);
 414|        let crossed = matches!(
 415|            launch,
...
 428|                self.dispatch,
*429|                DispatchStatus::LaunchUnknown
*430|                    | DispatchStatus::Launched
 431|                    | DispatchStatus::InputUnknown
 432|                    | DispatchStatus::Applied
 433|            );
...
 464|            let prepare = self.receipt(DispatchOperation::Prepare).map(|r| r.outcome);
*465|            // Raw legacy Prepared was durably replaced with LaunchUnknown before
 466|            // the native boundary. A validated intact record with no operation
 467|            // evidence contradicting it retains that old pre-effect invariant.
 468|            return if self.receipts.is_empty() && self.request_digests.is_empty()
...
 580|            }
*581|            DispatchStatus::LaunchUnknown => {
 582|                self.identity.session.is_some() == self.identity.run.is_some()
 583|                    && self.binding.is_none()
 584|            }
*585|            DispatchStatus::Launched => allocated && self.binding.is_none(),
 586|            DispatchStatus::InputUnknown | DispatchStatus::Applied => {
 587|                allocated && self.binding.is_some()
 588|            }
...
 598|            DispatchOperation::Prepare,
*599|            DispatchOperation::Launch,
 600|            DispatchOperation::InitialInput,
 601|        ] {
 602|            let outcome = if valid.is_none() || !consistent {
...
 606|                    (DispatchOperation::Prepare, _) => DispatchOutcome::Applied,
*607|                    (DispatchOperation::Launch, DispatchStatus::Prepared) => {
 608|                        DispatchOutcome::Accepted
 609|                    }
*610|                    (DispatchOperation::Launch, DispatchStatus::LaunchUnknown) => {
 611|                        DispatchOutcome::Unknown
 612|                    }
*613|                    (DispatchOperation::Launch, DispatchStatus::Refused)
 614|                        if self.binding.is_none() =>
 615|                    {
 616|                        DispatchOutcome::Rejected
 617|                    }
*618|                    (DispatchOperation::Launch, _) => DispatchOutcome::Applied,
 619|                    (
 620|                        DispatchOperation::InitialInput,
*621|                        DispatchStatus::Prepared | DispatchStatus::LaunchUnknown,
 622|                    ) => continue,
*623|                    (DispatchOperation::InitialInput, DispatchStatus::Launched)
 624|                        if self.binding.is_none() =>
 625|                    {
 626|                        DispatchOutcome::Accepted
...
 889|        let mut native_proof = None;
*890|        let availability = if let DelegationRequest::Launch { auth, handle, .. } = &request {
 891|            let record = self.owned_assignment(auth, handle)?;
 892|            if record.dispatch == DispatchStatus::Prepared {
 893|                Some(match available_model(&read_contract(&record)?) {
...
 1150|                            || !previous
*1151|                                .receipt(DispatchOperation::Launch)
 1152|                                .is_some_and(|receipt| receipt.outcome == DispatchOutcome::Rejected)
 1153|                        {
 1154|                            return Err(lifecycle_error(
...
 1364|            }
*1365|            DelegationRequest::Launch {
 1366|                auth,
 1367|                handle,
 1368|                identity,
##### delegation_control.rs
 721|        let mut record =
*722|            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Launched);
 723|        record.upgrade_legacy();
 724|        let mut raw = serde_json::to_value(&record).unwrap();
 725|        for field in [
##### delegation_dependencies.rs
 384|                || !previous
*385|                    .receipt(DispatchOperation::Launch)
 386|                    .is_some_and(|receipt| receipt.outcome == DispatchOutcome::Rejected)
 387|                || previous.spent_execution_attempts().is_none()
 388|            {
##### delegation_review.rs
 578|
*579|    /// Hooked into ordinary Prepare/Launch. No independent scheduler or child owner.
 580|    pub(super) fn validate_review_assignment(
 581|        &self,
 582|        auth: &DelegationAuth,
##### lifecycle.rs
 3|use super::*;
*4|use ovrcr_protocol::{LifecycleOp, LifecycleOutcome, SessionLaunch};
 5|
 6|pub(super) enum JobKind {
 7|    CreateWorkspace {
...
 11|    },
*12|    CreateWorkspaceWithLaunch {
 13|        project: String,
 14|        id: String,
 15|        branch: BranchRequest,
*16|        launch: Option<SessionLaunch>,
 17|    },
 18|    RemoveWorkspace {
 19|        project: String,
...
 22|    },
*23|    CreateSession(ovrcr_protocol::CreateSessionRequest),
 24|    CloseTerminal {
 25|        session: SessionId,
 26|        expected_run: SessionRunId,
...
 33|            Self::CreateWorkspace { .. } => LifecycleOp::CreateWorkspace,
*34|            Self::CreateWorkspaceWithLaunch { .. } => LifecycleOp::CreateWorkspaceWithLaunch,
 35|            Self::RemoveWorkspace { .. } => LifecycleOp::RemoveWorkspace,
 36|            Self::CreateSession(_) => LifecycleOp::CreateSession,
 37|            Self::CloseTerminal { .. } => LifecycleOp::CloseTerminal,
...
 166|            },
*167|            JobKind::CreateWorkspaceWithLaunch {
 168|                project,
 169|                id,
 170|                branch,
...
 204|    use super::*;
*205|    use ovrcr_protocol::{CreateSessionRequest, SessionKind};
 206|    use std::ffi::OsString;
 207|    use std::sync::mpsc;
 208|    use std::time::Duration;
...
 227|            &mut ClientRole::Dashboard,
*228|            Request::CreateSession(CreateSessionRequest {
 229|                project: "p".into(),
 230|                workspace: "w".into(),
 231|                name: "s".into(),
...
 276|            &mut ClientRole::Control,
*277|            Request::CreateSession(CreateSessionRequest {
 278|                project: "missing".into(),
 279|                workspace: "w".into(),
 280|                name: "s".into(),
##### mod.rs
 201|
*202|enum WorkspaceLaunch {
 203|    None,
 204|    Shell,
*205|    Session(ovrcr_protocol::SessionLaunch),
 206|}
 207|
 208|const MAX_LIVE_SESSIONS: usize = 50;
...
 840|        &self,
*841|        request: ovrcr_protocol::CreateSessionRequest,
 842|    ) -> Result<SessionSummary> {
 843|        let _mutation = self.mutation_lock.lock().unwrap();
 844|        self.reject_if_stopping()?;
...
 850|        &self,
*851|        request: ovrcr_protocol::CreateSessionRequest,
 852|        ready: Arc<dyn Fn() + Send + Sync>,
 853|    ) -> Result<SessionSummary> {
 854|        let _mutation = self.mutation_lock.lock().unwrap();
...
 861|        &self,
*862|        mut request: ovrcr_protocol::CreateSessionRequest,
 863|        ready: Option<Arc<dyn Fn() + Send + Sync>>,
 864|        assignment: Option<&str>,
 865|    ) -> Result<SessionSummary> {
...
 1122|            let message = if no_process {
*1123|                "Launch failed before a process started; check the executable and retry"
 1124|            } else {
*1125|                "Launch did not complete; confirm previous processes stopped before retrying"
 1126|            };
 1127|            let persisted = {
 1128|                let mut retained = self.retained.lock();
...
 2145|        {
*2146|            WorkspaceLaunch::Shell
 2147|        } else {
*2148|            WorkspaceLaunch::None
 2149|        };
 2150|        self.create_workspace_inner(project, name, branch, launch)
 2151|            .map(|_| ())
...
 2164|            BranchRequest::New { branch, base },
*2165|            WorkspaceLaunch::None,
 2166|        )
 2167|        .map(|_| ())
 2168|    }
...
 2174|        branch: BranchRequest,
*2175|        launch: Option<ovrcr_protocol::SessionLaunch>,
 2176|    ) -> Result<Option<SessionSummary>> {
 2177|        self.create_workspace_inner(
 2178|            project,
...
 2180|            branch,
*2181|            launch.map_or(WorkspaceLaunch::None, WorkspaceLaunch::Session),
 2182|        )
 2183|    }
 2184|
...
 2189|        branch: BranchRequest,
*2190|        launch: WorkspaceLaunch,
 2191|    ) -> Result<Option<SessionSummary>> {
 2192|        let _mutation = self.mutation_lock.lock().unwrap();
 2193|        self.reject_if_stopping()?;
...
 2202|        branch: BranchRequest,
*2203|        launch: WorkspaceLaunch,
 2204|        assignment: Option<&str>,
 2205|    ) -> Result<Option<SessionSummary>> {
 2206|        self.reject_if_stopping()?;
...
 2284|        let (launch, session_name) = match launch {
*2285|            WorkspaceLaunch::None => return Ok(None),
*2286|            WorkspaceLaunch::Session(launch) => (launch, String::new()),
*2287|            WorkspaceLaunch::Shell => {
 2288|                let shell = std::env::var_os("SHELL").ok_or_else(|| {
 2289|                    lifecycle_error_with_hierarchy(
 2290|                        ErrorCode::PartialFailure,
...
 2299|                (
*2300|                    ovrcr_protocol::SessionLaunch {
 2301|                        argv: vec![shell],
 2302|                        label: None,
 2303|                        kind: SessionKind::Terminal,
...
 2313|        self.create_session_locked(
*2314|            ovrcr_protocol::CreateSessionRequest {
 2315|                project,
 2316|                workspace: name.clone(),
 2317|                name: session_name,
...
 2764|        self.create_session_locked(
*2765|            ovrcr_protocol::CreateSessionRequest {
 2766|                project: project.to_owned(),
 2767|                workspace: workspace_id.to_owned(),
 2768|                name: "local".into(),
##### tests.rs
 2504|    let summary = state
*2505|        .create_session(ovrcr_protocol::CreateSessionRequest {
 2506|            kind: ovrcr_protocol::SessionKind::Terminal,
 2507|            project: "project".into(),
 2508|            workspace: "workspace".into(),
...
 2563|    let error = state
*2564|        .create_session(ovrcr_protocol::CreateSessionRequest {
 2565|            kind: ovrcr_protocol::SessionKind::Terminal,
 2566|            project: "project".into(),
 2567|            workspace: "workspace".into(),
...
 5410|        creator_state.create_session_with_ready(
*5411|                ovrcr_protocol::CreateSessionRequest {
 5412|                    kind: ovrcr_protocol::SessionKind::Terminal,
 5413|                    project: "project".into(),
 5414|                    workspace: "workspace".into(),
...
 5631|        creator_state.create_session_with_ready(
*5632|            ovrcr_protocol::CreateSessionRequest {
 5633|                kind: ovrcr_protocol::SessionKind::Terminal,
 5634|                project: "project".into(),
 5635|                workspace: "workspace".into(),
...
 8689|                launched.run,
*8690|                "Launch did not complete; confirm previous processes stopped before retrying"
 8691|                    .into(),
 8692|            )
 8693|            .unwrap()
...
 8757|        let summary = state
*8758|            .create_session(ovrcr_protocol::CreateSessionRequest {
 8759|                kind,
 8760|                project: "project".into(),
 8761|                workspace: "workspace".into(),
...
 8880|    let pi = state
*8881|        .create_session(ovrcr_protocol::CreateSessionRequest {
 8882|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 8883|            project: "project".into(),
 8884|            workspace: "workspace".into(),
...
 8900|    let claude = state
*8901|        .create_session(ovrcr_protocol::CreateSessionRequest {
 8902|            kind: ovrcr_protocol::SessionKind::Agent {
 8903|                name: "claude".into(),
 8904|            },
...
 8915|    let shell = state
*8916|        .create_session(ovrcr_protocol::CreateSessionRequest {
 8917|            kind: ovrcr_protocol::SessionKind::Terminal,
 8918|            project: "project".into(),
 8919|            workspace: "workspace".into(),
...
 9123|    let created = state
*9124|        .create_session(ovrcr_protocol::CreateSessionRequest {
 9125|            project: "fixture".into(),
 9126|            workspace: "feature-topic".into(),
 9127|            name: "manual".into(),

#### settings/
##### mod.rs
 10|use ovrcr_protocol::{
*11|    AgentOverride, AutomaticLocalTerminals, DelegationHarness, DelegationSettings, LaunchChoice,
 12|    NativeCommand, ReadySoundChoice, SettingRow, Settings, SettingsFinding, SettingsReport,
 13|    ThemeId,
 14|};
...
 270|        self.drop_unknown_fields(table, "launch_choices", element_fields("launch_choices"));
*271|        if let Some(choices) = self.take_table::<DocumentLaunchChoice>(
 272|            table,
 273|            "launch_choices",
 274|            "a table { kind, preset }",
...
 726|#[serde(tag = "kind", content = "preset", deny_unknown_fields)]
*727|enum DocumentLaunchChoice {
 728|    Terminal,
 729|    Agent(String),
 730|}
 731|
*732|impl From<DocumentLaunchChoice> for LaunchChoice {
*733|    fn from(choice: DocumentLaunchChoice) -> Self {
 734|        match choice {
*735|            DocumentLaunchChoice::Terminal => Self::Terminal,
*736|            DocumentLaunchChoice::Agent(preset) => Self::Agent(preset),
 737|        }
 738|    }
 739|}
...
 762|
*763|/// Serde names the Rust type (`struct AgentOverride`, `enum DocumentLaunchChoice`).
 764|/// The finding should name the document shape instead.
 765|fn element_message(message: String, expected: &str) -> String {
 766|    if (message.contains("struct ") || message.contains("enum "))
...
 1010|            settings.launch_choices["ovrcr"],
*1011|            LaunchChoice::Agent("claude".into())
 1012|        );
*1013|        assert_eq!(settings.launch_choices["notes"], LaunchChoice::Terminal);
 1014|        assert!(settings.quota.enabled);
 1015|        assert!(settings.quota.claude_probe);
 1016|        assert_eq!(settings.quota.codex.command, PathBuf::from("/opt/codex"));
...
 1209|            report.settings.launch_choices["p"],
*1210|            LaunchChoice::Agent("x".into())
 1211|        );
*1212|        assert_eq!(report.settings.launch_choices["q"], LaunchChoice::Terminal);
 1213|        assert_eq!(
 1214|            row(&report, "launch_choices").source,
 1215|            SettingSource::Document
...
 1265|            report.settings.launch_choices,
*1266|            [("kept".to_owned(), LaunchChoice::Terminal)]
 1267|                .into_iter()
 1268|                .collect()
 1269|        );
...
 1322|            report.settings.launch_choices.get("plain"),
*1323|            Some(&LaunchChoice::Terminal)
 1324|        );
 1325|        finding("launch_choices.\"my.project\"", 4, "invalid type", &report);
 1326|    }
...
 1341|            report.settings.launch_choices.get("my.project"),
*1342|            Some(&LaunchChoice::Terminal)
 1343|        );
 1344|        finding(
 1345|            "launch_choices.my.project.extra",
##### write.rs
 472|    use ovrcr_protocol::{
*473|        AgentOverride, AutomaticLocalTerminals, LaunchChoice, ReadySoundChoice, Settings,
 474|    };
 475|    use std::os::unix::fs::{PermissionsExt, symlink};
 476|    use std::path::PathBuf;
...
 605|                "launch_choices",
*606|                |s| s.launch_choices["p"] == LaunchChoice::Terminal,
 607|            ),
 608|            ("launch_choices.\"p.q\"", AGENT, "launch_choices", |s| {
*609|                s.launch_choices["p.q"] == LaunchChoice::Agent("new".into())
 610|            }),
 611|            ("quota.enabled", "true", "quota.enabled", |s| {
 612|                s.quota.enabled
...
 736|            reading(&document).settings.launch_choices["p"],
*737|            LaunchChoice::Agent("claude".into())
 738|        );
 739|        set(&document, "launch_choices.p", None).unwrap();
 740|        assert!(reading(&document).settings.launch_choices.is_empty());
...
 1112|        let settings = reading(&target).settings;
*1113|        assert_eq!(settings.launch_choices["demo"], LaunchChoice::Terminal);
 1114|        assert_eq!(
 1115|            settings.launch_choices["other.project"],
*1116|            LaunchChoice::Agent("other".into())
 1117|        );
 1118|    }
 1119|
...
 1172|            for (value, choice) in [
*1173|                (AGENT, LaunchChoice::Agent("new".into())),
*1174|                (TERMINAL, LaunchChoice::Terminal),
 1175|            ] {
 1176|                set(
 1177|                    &document,
...
 1185|                assert!(saved.contains("extra = 42"));
*1186|                if matches!(choice, LaunchChoice::Agent(_)) {
 1187|                    assert_eq!(
 1188|                        saved,
 1189|                        original
...
 1221|            settings.launch_choices[project],
*1222|            LaunchChoice::Agent("new".into())
 1223|        );
 1224|        assert_eq!(
 1225|            settings.launch_choices["other.project"],
*1226|            LaunchChoice::Agent("other".into())
 1227|        );
 1228|        assert_eq!(settings.launch_choices.len(), 2);
 1229|        assert_eq!(settings.branch_prefix, "fix/");

### tests/
#### delegation_cancellation_state.rs
 387|        DispatchOperation::Prepare,
*388|        DispatchOperation::Launch,
 389|        DispatchOperation::InitialInput,
 390|        DispatchOperation::Followup,
 391|    ] {

## ovrcr-tui/src/dashboard/
### agents.rs
 41|#[derive(Clone, Debug, PartialEq, Eq)]
*42|pub struct AgentEntry {
 43|    pub name: String,
 44|    pub argv: Vec<OsString>,
 45|    pub source: AgentSource,
...
 51|    launcher: Option<&Path>,
*52|) -> Vec<AgentEntry> {
 53|    let mut entries = Vec::new();
 54|    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
 55|    for name in KNOWN_AGENTS {
...
 73|            };
*74|            entries.push(AgentEntry {
 75|                name: name.into(),
 76|                argv,
 77|                source: AgentSource::Detected,
...
 83|        .unwrap_or_else(|| OsString::from("/bin/sh"));
*84|    entries.push(AgentEntry {
 85|        name: "shell".into(),
 86|        argv: vec![shell],
 87|        source: AgentSource::Shell,
 88|    });
*89|    entries.push(AgentEntry {
 90|        name: "Custom".into(),
 91|        argv: Vec::new(),
 92|        source: AgentSource::Custom,
...
 97|pub fn apply_overrides(
*98|    mut entries: Vec<AgentEntry>,
 99|    overrides: &[AgentOverride],
*100|) -> Vec<AgentEntry> {
 101|    let mut custom = entries.pop();
 102|    let mut shell = entries.pop();
 103|    for override_agent in overrides {
...
 128|        } else {
*129|            entries.push(AgentEntry {
 130|                name: override_agent.name.clone(),
 131|                argv,
 132|                source: AgentSource::Detected,
### desktop.rs
 1190|
*1191|/// Only the short-lived client is a child. Its LaunchServices app lives outside
 1192|/// this owned process group and survives cancellation or Dashboard detach.
 1193|#[cfg(target_os = "macos")]
 1194|fn run_bridge_with_profile(

Showing files 1-20 of 26+. Use skip=20 for the next page, or narrow paths/pattern.

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/mod.rs:830-960"}
    pub fn set_session_paused(&self, id: SessionId, paused: bool) -> Result<()> {
…
        let session = self.session_for_control(id)?;
        let control = session.set_paused(paused).map(|_| ());
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, control, refresh)
    }

    pub fn handle_request(&self, role: &mut ClientRole, request: Request) -> Response {
        handle_request_with_id(self, role, request, 0, None)
    }

    pub fn create_session(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, None, None)
    }

    #[cfg(test)]
    pub(crate) fn create_session_with_ready(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
        ready: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, Some(ready), None)
    }

    /// Admission stays locked from the live-slot check through durable intent and publication.
    fn create_session_locked(
        &self,
        mut request: ovrcr_protocol::CreateSessionRequest,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
        assignment: Option<&str>,
    ) -> Result<SessionSummary> {
        // Every error in this preflight precedes the sole native spawn call.
        let record = (|| -> Result<_> {
            if request.argv.is_empty() {
                return Err(lifecycle_error(
                    ErrorCode::InvalidRequest,
                    "session command cannot be empty",
                ));
            }
            let workspace_record = self
                .registry
                .lock()
                .unwrap()
                .workspace(&request.project, &request.workspace)
                .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
                .clone();
            let cwd = workspace_record.path.clone();
            if !cwd.is_dir() {
                return Err(lifecycle_error(
                    ErrorCode::NotFound,
                    "working directory is unavailable",
                ));
            }
            let display =
                git::checkout_name(&cwd).unwrap_or_else(|_| git::UNAVAILABLE_CHECKOUT.to_owned());
            let automatic = request.name.is_empty();
            {
                let retained = self.retained.lock();
                let used: std::collections::HashSet<_> = retained
                    .records()
                    .filter(|record| {
                        record.metadata.project == request.project
                            && record.metadata.workspace == request.workspace
                    })
                    .map(|record| record.metadata.name.as_str())
                    .collect();
                if automatic {
                    request.name = display.clone();
                    let mut suffix = 2u64;
                    while used.contains(request.name.as_str()) {
                        request.name = format!("{display}-{suffix}");
                        suffix += 1;
                    }
                } else if used.contains(request.name.as_str()) {
                    return Err(lifecycle_error(
                        ErrorCode::AlreadyExists,
                        "duplicate session name",
                    ));
                }
            }
            let pinned_title = if automatic || matches!(request.kind, SessionKind::Agent { .. }) {
                None
            } else {
                Some(
                    crate::session::sanitize_title(&request.name).ok_or_else(|| {
                        lifecycle_error(
                            ErrorCode::InvalidRequest,
                            "title must contain visible text",
                        )
                    })?,
                )
            };
            self.check_live_capacity()?;
            let label = default_session_label(&request.kind, request.label, &request.argv);
            let record = self.retained.lock().create(SessionMetadata {
                scope: ovrcr_protocol::SessionScope::Workspace,
                project: request.project,
                workspace: request.workspace,
                name: request.name,
                label,
                cwd,
                kind: request.kind,
                pinned_title,
                application_title: None,
            })?;
            if let Some(handle) = assignment {
                self.bind_assignment(handle, &record)?;
            }
            Ok(record)
        })()
        .map_err(crate::session::NoProcessStarted::context)?;
        let error = match self.spawn_record_locked(record.id, record.run, request.argv, ready, None)
        {
            Ok(summary) => return Ok(summary),
            Err(error) => error,
        };
        let cleanup = {
            let mut retained = self.retained.lock();
            let current = retained.get(record.id).map(|record| record.run);
            match current {
                Some(run) if run.0 == 0 || error.is::<crate::session::NoProcessStarted>() => {
                    retained.remove(record.id, run)
                }
                _ => Ok(false),
            }
        };
        match cleanup {
            Ok(removed) => {
                if removed {
…
                }
…
            }
…
        }
    }

[Showing lines 829-963 of 3142. Use :964 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs:150-230"}
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
…
}

[Showing lines 149-233 of 4049. Use :234 to continue]

SOURCE LOOKUP {"pattern": "enum Page", "path": "crates/ovrcr-tui/src/dashboard/palette.rs"}
 89|
*90|enum Page {
 91|    Search {
 92|        query: String,
 93|        selected: usize,

SOURCE LOOKUP {"pattern": "launch_choices|LaunchChoice|remembered", "path": "crates/ovrcr-tui/src"}
# crates/ovrcr-tui/src/
## lib.rs
 14|pub use dashboard::{
*15|    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, KeyEncoding, LaunchChoice,
 16|    PaneRects, Settings, actual_drawn_inner_rect, dashboard_message_channel, detect_agents,
 17|    draw_dashboard, draw_dashboard_at, encode_key, encode_mouse, pane_rects, render_terminal,
 18|    run_dashboard, write_clipboard,

## dashboard/
### lifecycle_ui.rs
 375|            let request = super::settings::launch_choice_request(&project, &choice);
*376|            self.settings.launch_choices.insert(project, choice);
 377|            let message = crate::protocol::ClientMessage {
 378|                request_id: self.error_owning_request_id(),
 379|                request,
### mod.rs
 39|pub use render::{actual_drawn_inner_rect, draw_dashboard, draw_dashboard_at, render_terminal};
*40|pub use settings::{LaunchChoice, Settings};
 41|pub(crate) use state::{HistoryView, PendingHistoryBegin};
 42|
 43|use crate::task_tui::TasksView;
...
 336|    /// Remembered launch choice held across Provisional wait until CreatedSession completes.
*337|    lifecycle_preference: Option<(String, settings::LaunchChoice)>,
 338|}
 339|
 340|thread_local! {
### palette.rs
 149|    workspace_id: Option<String>,
*150|    pub(super) launch_preference: Option<(String, super::settings::LaunchChoice)>,
 151|    failed_launch: Option<CreateSessionRequest>,
 152|    launch_project: Option<String>,
 153|    suggestions: Suggestions,
...
 2675|    fn launch_fields(&self, project: &str, empty: bool) -> Vec<Field> {
*2676|        let choice = self.settings.launch_choices.get(project);
 2677|        let start = match choice {
*2678|            Some(super::settings::LaunchChoice::Agent(_)) => "Agent",
 2679|            _ => "Terminal",
 2680|        };
 2681|        let mut choices = vec!["Agent", "Terminal"];
...
 2695|        let agent = match choice {
*2696|            Some(super::settings::LaunchChoice::Agent(name)) => name.clone(),
 2697|            _ => agents.first().map(|a| a.name.clone()).unwrap_or_default(),
 2698|        };
 2699|        let mut fields = vec![
...
 2857|                match &launch.label {
*2858|                    Some(label) => super::settings::LaunchChoice::Agent(label.clone()),
*2859|                    None => super::settings::LaunchChoice::Terminal,
 2860|                },
 2861|            ));
 2862|            palette.failed_launch = Some(launch);
...
 3301|                    if let Some((project, choice)) = preference {
*3302|                        // The Server saves it; remembered here at once so the
 3303|                        // next launch offers it before the reading arrives.
 3304|                        let request = super::settings::launch_choice_request(&project, &choice);
*3305|                        self.settings.launch_choices.insert(project, choice);
 3306|                        outgoing.push(ClientMessage {
 3307|                            request_id: self.error_owning_request_id(),
 3308|                            request,
...
 4480|mod launch_tests {
*4481|    use super::super::settings::{AgentOverride, LaunchChoice};
 4482|    use super::*;
 4483|    use crate::protocol::{
 4484|        AgentActivity, ErrorCode, ProjectSummary, ServerEvent, ServerMessage, SessionId,
...
 4515|            None,
*4516|            Some(LaunchChoice::Terminal),
*4517|            Some(LaunchChoice::Agent("fixture-agent".into())),
 4518|        ] {
 4519|            let mut d = dashboard();
 4520|            d.settings.agents.push(AgentOverride {
...
 4524|            if let Some(preference) = preference {
*4525|                d.settings.launch_choices.insert("demo".into(), preference);
 4526|            }
 4527|            let mut retained = summary(1);
 4528|            retained.kind = SessionKind::Agent {
...
 4737|            assert_eq!(
*4738|                d.settings.launch_choices.get("demo"),
*4739|                Some(&LaunchChoice::Terminal),
 4740|                "{entrypoint}"
 4741|            );
 4742|        }
...
 4866|
*4867|    /// The Dashboard writes no file: the Server merges the remembered choice
 4868|    /// into the project's own entry (see `ovrcr-runtime`'s writer tests).
 4869|    #[test]
 4870|    fn successful_launch_asks_the_server_to_remember_the_choice() {
...
 4875|            d.hierarchy.projects[0].workspaces[0].project = project.into();
*4876|            let other = LaunchChoice::Agent("other".into());
 4877|            d.settings
*4878|                .launch_choices
 4879|                .insert("other.project".into(), other.clone());
 4880|            let sent = launch(&mut d, start);
 4881|            assert!(
...
 4889|            let expected = if start == "Agent" {
*4890|                LaunchChoice::Agent("fixture-agent".into())
 4891|            } else {
*4892|                LaunchChoice::Terminal
 4893|            };
 4894|            assert_eq!(saves.len(), 1, "{start}");
 4895|            assert_eq!(
...
 4898|            );
*4899|            assert_eq!(d.settings.launch_choices.get(project), Some(&expected));
### settings.rs
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
### settings_editor.rs
 7|use super::render::{BASE, MAUVE, MUTED, PEACH, RED, SUBTEXT, SURFACE0, TEXT, YELLOW};
*8|use super::settings::LaunchChoice;
 9|use super::text_cursor::TextCursor;
 10|use super::{Dashboard, DashboardAction};
 11|use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
...
 136|
*137|fn choice_text(choice: &LaunchChoice) -> String {
 138|    match choice {
*139|        LaunchChoice::Terminal => "Terminal".into(),
*140|        LaunchChoice::Agent(preset) => format!("Agent: {preset}"),
 141|    }
 142|}
 143|
*144|/// The value written at `launch_choices.<project>` for `choice`.
*145|fn choice_value(project: &str, choice: &LaunchChoice) -> String {
 146|    match super::settings::launch_choice_request(project, choice) {
 147|        Request::SetSetting { value, .. } => value.unwrap_or_default(),
 148|        _ => unreachable!("launch_choice_request builds SetSetting"),
...
 329|                );
*330|                parent.value = count(settings.launch_choices.len());
 331|                items.push(parent);
*332|                for (project, choice) in &settings.launch_choices {
*333|                    let path = format!("launch_choices.{}", toml_edit::Key::new(project.as_str()));
*334|                    let mut choices = vec![LaunchChoice::Terminal];
*335|                    choices.extend(presets.iter().cloned().map(LaunchChoice::Agent));
 336|                    if !choices.contains(choice) {
 337|                        choices.push(choice.clone());
 338|                    }
...
 359|                    entry.group,
*360|                    "launch_choices+".into(),
 361|                    "+ Add project".into(),
 362|                    String::new(),
 363|                    Kind::Text,
...
 740|        self.settings_editor.selected = Some(row.id.clone());
*741|        let kind = if row.id.starts_with("launch_choices.") {
 742|            // Presets come from PATH, so only list them when asked to.
 743|            let presets = self.launch_presets();
 744|            let report = self.settings_report.as_deref().expect("checked above");
...
 965|            ),
*966|            "launch_choices+" if text.is_empty() => return DashboardAction::Redraw,
*967|            "launch_choices+" => {
*968|                let request = super::settings::launch_choice_request(text, &LaunchChoice::Terminal);
 969|                let Request::SetSetting { path, value } = request else {
 970|                    unreachable!()
 971|                };

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:85-200"}
struct Field {
…
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
…
    );
…
}

[Showing lines 84-203 of 6184. Use :204 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs", "pattern": "fn launch_fields|fn submit|Page::Launch|Page::Workspace|fn open_|CreateWorkspaceWithLaunch|failed_launch"}
 150|    pub(super) launch_preference: Option<(String, super::settings::LaunchChoice)>,
*151|    failed_launch: Option<CreateSessionRequest>,
 152|    launch_project: Option<String>,
 153|    suggestions: Suggestions,
 154|    // Some(true) retains explicit whole-form Submit; Some(false) replays Enter.
...
 529|            launch_preference: None,
*530|            failed_launch: None,
 531|            launch_project: None,
 532|            suggestions: Suggestions::default(),
 533|            submit_when_ready: None,
...
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
 808|            launch_preference: None,
*809|            failed_launch: None,
 810|            launch_project: None,
 811|            suggestions: Suggestions::default(),
 812|            submit_when_ready: None,
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
 2366|                    {
*2367|                        let mut previous = palette.failed_launch.as_ref().unwrap().clone();
 2368|                        if !self
 2369|                            .hierarchy
 2370|                            .projects
...
 2511|                                };
*2512|                                Request::CreateWorkspaceWithLaunch {
 2513|                                    project: field("Project"),
 2514|                                    id,
 2515|                                    launch: selected_launch.clone(),
...
 2528|                            Command::RecoverLaunch => {
*2529|                                let previous = palette.failed_launch.as_ref().unwrap();
 2530|                                let registered =
 2531|                                    find_workspace(self, &previous.project, &previous.workspace)
 2532|                                        .is_some();
...
 2674|
*2675|    fn launch_fields(&self, project: &str, empty: bool) -> Vec<Field> {
 2676|        let choice = self.settings.launch_choices.get(project);
 2677|        let start = match choice {
 2678|            Some(super::settings::LaunchChoice::Agent(_)) => "Agent",
...
 2837|        let launch = match &request {
*2838|            Request::CreateWorkspaceWithLaunch {
 2839|                project,
 2840|                id: name,
 2841|                launch: Some(launch),
...
 2861|            ));
*2862|            palette.failed_launch = Some(launch);
 2863|        }
 2864|        let request_id = self.next_request_id();
 2865|        palette.pending = Some(request_id);
...
 3197|                } else if code == crate::protocol::ErrorCode::OwnershipUncertain {
*3198|                    palette.failed_launch = None;
 3199|                    palette.page = Page::Confirm {
 3200|                        request: None,
 3201|                        target: "Launch ownership is uncertain. No process-stop acknowledgement was sent. Close this notice, select the intended retained row, then use its Reopen or Acknowledge action after stopping old processes. Do not create another session.".into(),
...
 3210|                    )
*3211|                    && palette.failed_launch.is_some()
 3212|                {
 3213|                    let fields = vec![pick_field(
 3214|                        "Recovery",
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
 4565|    }
*4566|    fn submit(d: &mut Dashboard) -> DashboardAction {
 4567|        // This is the production handler used by the visible Submit control.
 4568|        d.palette_key_with_submit(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), true)
 4569|    }
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
...
 5736|        assert!(
*5737|            palette.failed_launch.is_none(),
 5738|            "uncertain create must not keep a recoverable launch"
 5739|        );
 5740|        assert!(

SOURCE LOOKUP {"path": "*/tests/*.rs"}
# crates/
## ovrcr-tui/tests/
settings_theme_preview.rs
## ovrcr-runtime/tests/
settings_matrix.rs
delegation_fallback_files.rs
delegation_cancellation_state.rs
theme_catalog.rs
## ovrcr-protocol/tests/
questions.rs

SOURCE LOOKUP {"path": "**/tests/tui.rs;crates/ovrcr-tui/tests/*"}
# crates/ovrcr-tui/tests/
settings_theme_preview.rs

SOURCE LOOKUP {"path": "tests"}
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
pi_workflow_tracker.mjs
pi_package_installed.mjs
claude_reporting_load.rs
workspace_identity.rs
settings_lifecycle.rs
retained_sessions.rs
retained_capacity.rs
resource_cli.rs
pi_reporting.rs
pi_publish_contract_installed.mjs
pi_package_bootstrap.mjs
pi_package.rs
pi_observation_extension.mjs
pi_interrupt_extension.mjs
pi_continuation_records.mjs
pi_child_tools_installed.mjs
omp_reporting_extension.mjs
automatic_titles.rs
quota_schema.rs
codex_reporting.rs
git_lifecycle.rs
client_startup.rs
claude_quota_probe.rs
wip_save.rs
task_schedule.rs
task_paging.rs
task_execution.rs
task_cli.rs
retained_ownership.rs
registry_migration.rs
quota_ui_option_a_smoke.rs
claude_reporting.rs
task_runner.rs
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
live.rs
dashboard_proxy.rs
claude_probe.sh
claude_auth.rs
deadline.rs
mouse_app.rs
## pi_delegation/
review.rs
control.rs
cancellation.rs
claude.rs
dependencies.rs
native_publisher.rs
codex.rs
recovery.rs
questions.rs
observation.rs
followups.rs
fallback.rs
delivery.rs
coding.rs
## fixtures/
### pi/
pi_host.mjs
claude_delegation_host.mjs
workflow-gh.mjs
pi_review.mjs
pi_dependencies.mjs
codex_delegation_host.mjs
research_pi.sh
pi_publish_contract.mjs
pi_keybindings.mjs
### operator-auth/
rsa-test-only.pem
jwks.json
### cursor-cloud/
bridge.py
### agent-reporting/
#### codex/0.153.0/
thread-identity-fields.json
resume-contract.json
request-methods.json
provenance.json
help.txt
app-server-help.txt
ThreadTokenUsageUpdatedNotification.json
ThreadStatusChangedNotification.json
ThreadReadParams.json
README.md
InitializeParams.json
##### native-matrix/
snapshot.py
snapshot-two-turns.json
snapshot-invalid-model.json
snapshot-cancelled.json
snapshot-after-permission.json
snapshot-after-child.json
sha256.json
reference-checks.py
permission-unanswered.json
permission-allowed.json
observer.py
numeric-reference-checks.json
main-exit-reference.json
launch.py
launch-error.py
invalid-model.json
foreign-first-guard.json
cleanup.json
child-isolation.json
cancellation.json
callbacks.jsonl
callbacks-main.jsonl
callback.py
README.md
##### native-invalidation/
snapshot-before-backtrack.json
sha256.json
postfork-source.json
postfork-callbacks.json
cleanup.json
callbacks.jsonl
before-exit-processes.json
before-backtrack.json
after-backtrack.json
README.md
#### claude/
tool-success.json
tool-failure.json
stop.json
stop-continuation.json
stop-block-output.json
statusline-context-cost.json
statusline-before-response.json
session-start-startup.json
session-start-resume.json
session-start-fork.json
session-start-compact.json
session-start-clear.json
session-end.json
prompt.json
pre-tool.json
pre-compact.json
post-compact.json
permission-request.json
permission-notification.json
named-root-start.json
manifest.json
child-stop.json
child-pre-tool.json
api-failure.json

SOURCE LOOKUP {"path": "tests/tui/main.rs"}
#[cfg(not(target_os = "macos"))]
#[path = "../support/deadline.rs"]
mod deadline;

#[cfg(target_os = "macos")]
#[path = "../support/live.rs"]
mod live;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[cfg(not(target_os = "macos"))]
use deadline::wait_deadline;
#[cfg(target_os = "macos")]
use live::wait_deadline;
…
use std::sync::mpsc::TrySendError;

mod agent_attention;
mod agent_search;
mod bridge_navigation;
mod copy_history;
mod events;
mod mouse;
mod operator;
mod palette;
mod quota;
mod settings;
mod sidebar;
mod split;
mod unread;

fn session_summary(
    id: u64,
    project: &str,
    workspace: &str,
    name: &str,
    label: &str,
    pid: Option<u32>,
    started_unix_ms: u64,
) -> SessionSummary { … }

fn fixture_hierarchy() -> HierarchySnapshot { … }

fn dashboard_fixture() -> Dashboard { … }

fn only_request(mut requests: Vec<ClientMessage>) -> ClientMessage { … }

fn optional_request(mut requests: Vec<ClientMessage>) -> Option<ClientMessage> { … }

fn pump_view(dashboard: &mut Dashboard, _action: DashboardAction) { … }

fn pump_view_requests(dashboard: &mut Dashboard, requests: Vec<ClientMessage>) { … }

fn acknowledge_view_request(dashboard: &mut Dashboard, request: ovrcr::protocol::ClientMessage) {
    acknowledge_all_view_targets(dashboard, request);
}

fn acknowledge_all_view_targets(
    dashboard: &mut Dashboard,
    request: ovrcr::protocol::ClientMessage,
) { … }

fn assert_view_input_blocked(dashboard: &mut Dashboard) { … }

fn assert_view_input_allowed(dashboard: &mut Dashboard) { … }

fn deliver_all_view_screens(
    dashboard: &mut Dashboard,
    request_id: u64,
    view: &ovrcr::protocol::DashboardView,
) { … }

fn palette_text(dashboard: &Dashboard) -> String { … }

fn palette_search(dashboard: &mut Dashboard, query: &str) { … }

fn answer_palette_inspect(dashboard: &mut Dashboard, _action: DashboardAction) { … }

fn hint_repository(extra_branches: &[&str]) -> tempfile::TempDir { … }

/// Answers the create-workspace Inspect so Git hints start for a real repository.
fn answer_workspace_inspect(dashboard: &mut Dashboard, request_id: u64, repo: &std::path::Path) { … }

/// Drives the create-workspace form to a deferred submit on a typed branch.
fn type_existing_branch_and_submit(dashboard: &mut Dashboard, branch: &str) -> u64 { … }

fn assert_workspace_id(id: &str) { … }

/// Polls the deferred submit until the palette stops waiting on Git, refusing
/// any request along the way, and returns the rendered palette.
fn poll_until_deferred_submit_refused(dashboard: &mut Dashboard, why: &str) -> String { … }

/// The fixture after the selected session's screen has arrived, so live
/// output is applied to the pane.
fn screen_ready_dashboard(bytes: &[u8]) -> Dashboard { … }

fn copy_ready_dashboard() -> Dashboard {
    screen_ready_dashboard(b"abc")
}

fn history_opened(total_rows: u32) -> HistoryOpened { … }

fn history_cell(text: &str, width: u8) -> HistoryCell { … }

#[test]
fn ordinary_response_errors_remain_visible_to_dashboard() { … }

#[test]
fn ctrl_g_is_reserved_and_other_control_keys_reach_the_pty() { … }

#[test]
fn paste_honors_bracketed_paste_mode() { … }

#[test]
fn runtime_paste_event_becomes_selected_session_input() { … }

#[test]
fn dashboard_reader_channel_applies_bounded_backpressure() { … }

#[test]
fn output_for_newly_selected_session_waits_for_screen() { … }

#[test]
fn control_punctuation_and_modified_keys_encode() { … }

#[test]
fn inactive_to_live_requests_a_current_run_view_without_old_output() { … }

/// Slightly longer than `VIEW_RETRY_BACKOFF` in `crates/ovrcr-tui/src/dashboard/state.rs`.
const PAST_VIEW_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(260);

fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent { … }

fn focused_terminal_rect(dashboard: &Dashboard, area: Rect) -> Rect { … }

fn enable_terminal_mouse(dashboard: &mut Dashboard, modes: &[u8]) { … }

fn click_in(inner: Rect, kind: MouseEventKind, dx: u16, dy: u16) -> MouseEvent {
    mouse_event(kind, inner.x + dx, inner.y + dy, KeyModifiers::NONE)
}

fn rendered_footer(dashboard: &Dashboard, width: u16) -> String { … }

fn rendered_rows(dashboard: &Dashboard, width: u16, height: u16) -> Vec<String> { … }

[…551ln elided; re-read needed ranges, e.g. tests/tui/main.rs:16-33,58-83]

SOURCE LOOKUP {"path": "tests/tui/palette.rs"}
//! Command palette, which-key popup, and the creation forms.

use crate::*;

fn extra_workspace_hierarchy(id: impl Into<String>) -> HierarchySnapshot { … }

fn install_picker_roots(dashboard: &mut Dashboard, roots: Vec<PathBuf>) { … }

fn footer_text(dashboard: &Dashboard) -> String {
    rendered_footer(dashboard, 120)
}

#[test]
fn leader_workspace_uses_inspection_and_name_first_picker_defaults() { … }

#[test]
fn palette_hint_action_ignores_late_inspection_error() { … }

#[test]
fn space_w_n_opens_the_terminal_form() { … }

#[test]
fn question_mark_popup_is_browsable() { … }

#[test]
fn question_mark_popup_lists_ready_sound_beside_desktop_notifications() { … }

#[test]
fn whichkey_compact_corner_preserves_the_surrounding_dashboard() { … }

fn whichkey_test_bounds(buffer: &ratatui::buffer::Buffer) -> Rect { … }

#[test]
fn whichkey_scrolled_last_row_remains_clickable_after_resize() { … }

#[test]
fn narrow_popup_moves_description_to_detail_line() { … }

#[test]
fn empty_hierarchy_shows_start_screen() { … }

#[test]
fn palette_entries_show_keys_and_descriptions() { … }

#[test]
fn whichkey_mouse_uses_rendered_rows_and_blocks_background_clicks() { … }

#[test]
fn keyboard_overlays_cancel_divider_gesture_before_consuming_release() { … }

#[test]
fn tasks_hotkey_cancels_pending_history_like_control_t() { … }

#[test]
fn whichkey_preserves_readiness_and_never_forwards_overlay_input() { … }

#[test]
fn whichkey_copy_leader_preserves_capture_and_v_anchors() { … }

#[test]
fn whichkey_copy_cursor_moves_into_popup_instead_of_underlying_capture() { … }

#[test]
fn whichkey_empty_workspace_click_sets_terminal_form_target() { … }

#[test]
fn whichkey_tiny_layouts_and_last_disabled_detail_remain_browsable() { … }

#[test]
fn uppercase_x_confirms_session_close_without_closing_pane() { … }

#[test]
fn palette_filters_and_captures_input_without_sending_it_to_terminal() { … }

#[test]
fn palette_switches_by_search_and_confirms_exact_close_target() { … }

#[test]
fn palette_forms_build_workspace_and_project_requests_and_draw_at_small_sizes() { … }

#[test]
fn create_workspace_form_submits_branch_without_name() { … }

#[test]
fn create_workspace_existing_branch_keeps_generated_id() { … }

#[test]
fn w_opens_workspace_form_with_project_and_derived_branch() { … }

#[test]
fn workspace_branch_mode_uses_local_branches_and_preserves_edits() { … }

#[test]
fn typed_existing_branch_survives_hint_arrival() { … }

#[test]
fn fuzzy_matched_branch_is_not_submitted_without_confirmation() { … }

#[test]
fn case_only_branch_mismatch_is_refused_not_submitted() { … }

#[test]
fn typed_branch_missing_from_hints_is_refused_not_replaced() { … }

#[test]
fn workspace_created_with_exited_local_shell_closes_palette() { … }

#[test]
fn palette_switch_drops_late_inventory_without_clearing_current_error() { … }

#[test]
fn workspace_creation_cancel_and_failure_do_not_attach_late() { … }

#[test]
fn workspace_creation_attaches_to_its_local_shell() { … }

#[test]
fn n_opens_terminal_form_prefilled_for_selected_workspace() { … }

#[test]
fn created_session_enters_terminal_mode() { … }

#[test]
fn browse_footer_lists_only_menu_search_and_help() { … }

#[test]
fn n_is_listed_in_the_hint_table() { … }

#[test]
fn a_opens_project_form_with_roots_and_derives_name_and_root() { … }

#[test]
fn tab_on_a_leaf_path_advances_to_the_next_field() { … }

#[test]
fn paste_into_repository_field_refreshes_the_listing() { … }

#[test]
fn double_enter_refreshes_the_derived_workspace_root_listing() { … }

#[test]
fn search_selection_clamps_when_entries_shrink() { … }

#[test]
fn palette_requires_fields_and_keeps_terminal_keys_outside_palette() { … }

#[test]
fn palette_close_updates_selection_and_clears_removed_screen() { … }

#[test]
fn palette_active_field_remains_visible_in_a_small_window() { … }

#[test]
fn palette_escape_cancels_pending_request() { … }

#[test]
fn nested_whichkey_terminal_group_waits_for_action_and_backspace_returns() { … }

#[test]
fn nested_whichkey_removal_confirms_the_selected_target_and_can_cancel() { … }

fn whichkey_menu_text(dashboard: &Dashboard) -> String { … }

fn click_whichkey_text(
    dashboard: &mut Dashboard,
    text: &str,
    sidebar: bool,
) -> ovrcr::tui::DashboardAction { … }

#[test]
fn nested_whichkey_container_selection_limits_groups_and_removal_target() { … }

#[test]
fn nested_whichkey_mouse_and_enter_open_groups_before_running_actions() { … }

#[test]
fn nested_whichkey_live_phase_updates_pause_resume_and_ignores_invalid_keys() { … }

#[test]
fn nested_whichkey_empty_hierarchy_can_register_and_open_global_view() { … }

#[test]
fn ux_browse_footer_is_stable_across_widths_and_state() { … }

#[test]
fn ux_search_keeps_session_identity_and_filters_complete_context() { … }

#[test]
fn ux_forms_put_task_name_in_border_and_keep_active_field_visible() { … }

#[test]
fn removal_pickers_filter_and_confirm_a_different_target() { … }

#[test]
fn removal_picker_rejects_no_match_and_excludes_root() { … }

#[test]
fn removal_pickers_with_empty_hierarchy_never_confirm() { … }

#[test]
fn picker_validation_keeps_recovery_hints_and_cancels_without_removal() { … }

#[test]
fn manual_title_events_update_sidebar_pane_and_palette_without_identity_change() { … }

#[test]
fn empty_workspace_creation_selects_its_empty_state_in_either_response_order() { … }

#[test]
fn reopen_confirm_names_stopped_processes_and_does_not_treat_retry_as_ack() { … }

#[test]
fn failed_reopen_retries_the_current_run_from_the_confirm() { … }

#[test]
fn ownership_uncertain_create_does_not_create_another_row_on_enter() { … }

#[test]
fn ownership_uncertain_create_does_not_guess_among_two_rows() { … }

#[test]
fn reopen_confirm_keeps_captured_run_when_hierarchy_advances() { … }

#[test]
fn refused_workspace_removal_offers_force_and_sends_it_only_on_explicit_confirm() { … }

#[test]
fn dirty_workspace_removal_asks_to_save_then_removes_only_after_yes() { … }

#[test]
fn declining_the_save_does_not_remove_the_workspace() { … }

#[test]
fn shutdown_asks_once_per_dirty_worktree() { … }

fn shutdown_navigation_fixture() -> (
    Dashboard,
    ovrcr::protocol::BridgeNavigationOffer,
    tempfile::TempDir,
) { … }

fn shutdown_navigation_confirmation(
    dashboard: &mut Dashboard,
    offer: &ovrcr::protocol::BridgeNavigationOffer,
) -> ClientMessage { … }

fn shutdown_question(dashboard: &mut Dashboard, name: &str) { … }

fn assert_shutdown_question(dashboard: &Dashboard, name: &str, hidden: &str) { … }

fn answer_shutdown_question(dashboard: &mut Dashboard, name: &str, save: bool) -> ClientMessage { … }

#[test]
fn shutdown_question_rejects_notification_offer_without_inferring_an_answer() { … }

#[test]
fn shutdown_question_rejects_already_pending_navigation_confirmation() { … }

#[test]
fn operator_quick_palette_fallback_restores_same_search_after_dismissal() { … }

#[test]
fn operator_quick_pending_shutdown_ack_advances_under_visible_modal() { … }

#[test]
fn operator_quick_notification_retarget_cancels_unsent_and_retains_submitted_continuation() { … }

#[test]
fn operator_quick_preserves_workspace_save_chain_and_exact_pending_response() { … }

#[test]
fn operator_quick_escape_never_answers_or_replaces_pending_shutdown_question() { … }

#[test]
fn shutdown_pending_answer_survives_navigation_and_advances_only_on_its_exact_ok() { … }

#[test]
fn force_removal_prompt_survives_unchanged_hierarchy_then_needs_consent_after_relabel() { … }

#[test]
fn workspace_removal_errors_keep_confirmation_and_session_context_visible() { … }

#[test]
fn archive_search_unarchives_without_launching() { … }

#[test]
fn close_confirms_live_work_but_archives_exited_work_immediately() { … }

#[test]
fn exited_agent_keeps_actionable_recovery_reason_in_the_header() { … }

#[test]
fn exited_agent_with_empty_recovery_keeps_stale_activity_out_of_header() { … }

#[test]
fn workspace_selection_and_fold_survive_branch_relabel() { … }

#[test]
fn duplicate_branch_labels_include_distinguishing_paths() { … }

#[test]
fn warning_disables_new_terminal_and_shows_reason() { … }

#[test]
fn slash_branch_picker_does_not_split_on_slashes() { … }

#[test]
fn shared_internal_ids_are_resolved_with_project() { … }

#[test]
fn terminal_workspace_picker_shows_current_branch_and_keeps_stable_target() { … }

#[test]
fn workspace_removal_confirmation_cannot_submit_after_branch_relabel() { … }

#[test]
fn relabeled_workspace_filter_requires_reselection_before_retargeting() { … }

#[test]
fn cloud_close_warns_before_archiving_live_and_exited_rows_from_key_and_palette() { … }

[…3733ln elided; re-read needed ranges, e.g. tests/tui/palette.rs:5-21,23-28]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/keymap.rs", "pattern": "KeyCode::Char\\('n'\\)|KeyCode::Char\\('w'\\)|New terminal|New agent|CreateWorkspace|open_create_terminal|open_create_workspace"}
 22|    CreateTerminal,
*23|    CreateWorkspace,
 24|    RegisterProject,
 25|    CloseTerminal,
 26|    RemoveWorkspace,
...
 290|            }
*291|            Action::CreateTerminal => self.open_create_terminal(),
*292|            Action::CreateWorkspace => self.open_create_workspace(),
 293|            Action::RegisterProject => self.open_register_project(),
 294|            Action::CloseTerminal => {
 295|                if let Some(action) = self.soft_fail_if_provisional() {
...
 1004|            key_binding("n", "Create terminal", format!("Choose an agent or shell to start in {workspace_target}; opens a form"), Char('n'), Action::CreateTerminal).unless(launch_blocked.or(no_workspace)).group('w', "n"),
*1005|            key_binding("w", "Create workspace", format!("Create a worktree and branch under {project_target} and choose its first Agent or Terminal; opens a form"), Char('w'), Action::CreateWorkspace).unless(dashboard.hierarchy.projects.is_empty().then_some("no project registered")).group('p', "n"),
 1006|            key_binding("a", "Register project", "Register a repository and its root workspace; opens a form; keeps the repository".into(), Char('a'), Action::RegisterProject).group('p', "a"),
 1007|        ] },
 1008|        KeyGroup { title: "Session".into(), keys: vec![

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs", "pattern": "compat|version|unknown|golden|fixture"}
 609|            .insert("q".into(), crate::LaunchChoice::Terminal);
*610|        // Keep the wire fixture's settings values explicit; quota collection
 611|        // is enabled by default for real settings documents.
 612|        settings.quota.enabled = false;
 613|        settings.quota.codex.home = Some("/h".into());
...
 1400|                next_check_unix_ms: Some(60_000),
*1401|                ..crate::ProviderQuota::unknown(
 1402|                    crate::QuotaProvider::Codex,
 1403|                    crate::QuotaState::Unavailable,
 1404|                )
...
 1421|        all.extend(
*1422|            crate::agent::tests::request_fixtures()
 1423|                .iter()
 1424|                .enumerate()
 1425|                .map(|(i, v)| (format!("AgentRequest::{i}"), encode(v))),
...
 1427|        all.extend(
*1428|            crate::agent::tests::response_fixtures()
 1429|                .iter()
 1430|                .enumerate()
 1431|                .map(|(i, v)| (format!("AgentResponse::{i}"), encode(v))),
...
 1542|            session: SessionId(1),
*1543|            capability: "fixture".into(),
 1544|        };
 1545|        let identity = crate::AssignmentIdentity {
 1546|            instance: "i".into(),
...
 1586|            identity: identity.clone(),
*1587|            version: 1,
 1588|            outcome: "success".into(),
 1589|            summary: "done".into(),
 1590|            evidence: vec!["citation".into()],
...
 1645|                    expected_identity: identity.clone(),
*1646|                    expected_result_version: 1,
 1647|                    expected_control_epoch: 1,
 1648|                    text: "clarify".into(),
 1649|                    revised_contract: Some(Box::new(contract)),
...
 1656|                    handle: "r".into(),
*1657|                    version: 1,
 1658|                },
 1659|            ),
 1660|            (
...
 1665|                    identity: identity.clone(),
*1666|                    result_version: 1,
 1667|                    target: crate::AcceptanceTarget::ChildSubmission {
 1668|                        digest: "digest".into(),
 1669|                    },
...
 1700|                        kind: crate::PublicationKind::Submission,
*1701|                        version: 1,
 1702|                        digest: "digest".into(),
 1703|                    },
 1704|                },
...
 1720|            },
*1721|            result_version: 1,
 1722|            activity_revision: 2,
 1723|            input_revision: 3,
 1724|            publication: None,
...
 1770|                        session: SessionId(1),
*1771|                        capability: "fixture".into(),
 1772|                    },
 1773|                    handle: "r".into(),
 1774|                    request: "confirm".into(),
...
 1850|                acceptance: "pending".into(),
*1851|                result_version: 1,
 1852|                publication: None,
 1853|                acceptance_ref: None,
 1854|                history: vec![],
*1855|                selected_version: None,
 1856|                historical_submission: None,
 1857|                receipts: vec![],
 1858|                resources: crate::AssignmentResources {
...
 1898|                                kind: crate::PublicationKind::Submission,
*1899|                                version: 1,
 1900|                                digest: "digest".into(),
 1901|                            },
 1902|                            handled: false,
...
 1914|            session: SessionId(1),
*1915|            capability: "fixture".into(),
 1916|            controller: None,
 1917|            source: None,
 1918|        };
...
 1958|                    workflow: "w".into(),
*1959|                    expected_version: 0,
 1960|                    controller: "c".into(),
 1961|                    graph: crate::DependencyGraph {
 1962|                        tasks: vec![crate::DependencyTask {
...
 1990|                    workflow: "w".into(),
*1991|                    expected_version: 0,
 1992|                    index: index.clone(),
 1993|                },
 1994|            ),
...
 2033|                workflow: "w".into(),
*2034|                version: 1,
 2035|                digest: "digest".into(),
 2036|            }),
 2037|            continuation: Some(index),
...
 2129|        let history = OperatorModelHistory {
*2130|            version: 1,
 2131|            json: "[]".into(),
 2132|        };
 2133|        let auth = crate::SupervisorAuth {
...
 2352|            session: crate::SessionId(1),
*2353|            capability: "fixture".into(),
 2354|            controller: None,
 2355|            source: None,
 2356|        };
...
 2409|                identity: identity.clone(),
*2410|                result_version: 3,
 2411|                contract_digest: "c".repeat(64),
 2412|                brief_digest: "f".repeat(64),
 2413|                freshness_nonce: "nonce".into(),
...
 2420|                binding,
*2421|                result_version: 3,
 2422|                activity_revision: 7,
 2423|                input_revision: 0,
 2424|                publication: None,
...
 2437|            schema: 1,
*2438|            version: 2,
 2439|            author: authority,
 2440|            capture: capture.clone(),
 2441|            artifact: crate::FrozenFallbackFile {
...
 2466|            identity: identity.clone(),
*2467|            result_version: 3,
 2468|            target: crate::AcceptanceTarget::ParentRecovery {
*2469|                recovery_version: 2,
 2470|                digest: "e".repeat(64),
 2471|            },
 2472|            path: "/acceptance-v3.json".into(),
...
 2475|        let reference = crate::RecoveryReference {
*2476|            version: 2,
 2477|            digest: "e".repeat(64),
 2478|        };
 2479|        for (name, request) in [
...
 2485|                    expected_identity: identity.clone(),
*2486|                    expected_result_version: 3,
 2487|                    expected_control_epoch: 1,
 2488|                },
 2489|            ),
...
 2496|                        capture_id: "capture".into(),
*2497|                        version: 2,
 2498|                        artifact,
 2499|                        original,
 2500|                        evidence: vec![evidence],
...
 2509|                    identity: identity.clone(),
*2510|                    result_version: 3,
 2511|                    target: crate::AcceptanceTarget::ParentRecovery {
*2512|                        recovery_version: 2,
 2513|                        digest: "e".repeat(64),
 2514|                    },
 2515|                    expected_control_epoch: 1,
...
 2538|                acceptance: "accepted".into(),
*2539|                result_version: 3,
 2540|                publication: None,
 2541|                acceptance_ref: Some(accepted.clone()),
 2542|                history: vec![],
*2543|                selected_version: Some(3),
 2544|                historical_submission: None,
 2545|                receipts: vec![],
 2546|                resources: crate::AssignmentResources {
...
 2610|            session: SessionId(1),
*2611|            capability: "fixture".into(),
 2612|            controller: None,
 2613|            source: None,
 2614|        };
...
 2622|                identity: identity.clone(),
*2623|                result_version: 3,
 2624|                obligation_revision: 4,
 2625|                publication: None,
 2626|                acceptance: None,
...
 2662|            expected_identity: identity,
*2663|            expected_result_version: 3,
 2664|            expected_obligation_revision: 4,
 2665|            expected_control_epoch: 2,
 2666|            delivery: reference,
...
 2757|            assignment: identity.clone(),
*2758|            result_version: 3,
 2759|            question_id: "section".into(),
*2760|            question_version: 1,
 2761|            digest: "a".repeat(64),
 2762|        };
 2763|        let reference = crate::QuestionReference {
*2764|            question_version: 2,
 2765|            digest: "b".repeat(64),
 2766|            ..previous.clone()
 2767|        };
...
 2770|            identity: identity.clone(),
*2771|            result_version: 3,
 2772|            question_id: "section".into(),
*2773|            question_version: 2,
 2774|            supersedes: Some(previous.clone()),
 2775|            kind: crate::QuestionKind::Clarification,
 2776|            question: "Which section?".into(),
...
 2792|            session: SessionId(1),
*2793|            capability: "fixture".into(),
 2794|            source: None,
 2795|            controller: Some(crate::ControllerFence {
 2796|                workflow: "w".into(),
...
 2840|            identity: identity.clone(),
*2841|            result_version: 3,
 2842|            outcome: crate::DispatchOutcome::Partial,
 2843|            input: Some(crate::InputDelivery {
 2844|                written_bytes: 3,
...
 2874|            acceptance: "pending".into(),
*2875|            result_version: 3,
 2876|            publication: None,
 2877|            acceptance_ref: None,
 2878|            history: vec![],
*2879|            selected_version: None,
 2880|            historical_submission: None,
 2881|            receipts: vec![receipt],
 2882|            resources: crate::AssignmentResources {
...
 2971|            session: SessionId(1),
*2972|            capability: "fixture".into(),
 2973|            controller: None,
 2974|            source: None,
 2975|        };
...
 2997|            binding: binding.clone(),
*2998|            result_version: 1,
 2999|            activity_revision: 3,
 3000|            input_revision: 0,
 3001|            publication: None,
...
 3012|                    expected_identity: identity.clone(),
*3013|                    expected_result_version: 1,
 3014|                    expected_control_epoch: 1,
 3015|                })),
 3016|            ),
...
 3086|                identity,
*3087|                result_version: 1,
 3088|                control_epoch: 1,
 3089|                interrupt_request: "interrupt".into(),
 3090|                outcome: crate::DispatchOutcome::Unknown,
...
 3113|            session: SessionId(1),
*3114|            capability: "fixture".into(),
 3115|            controller: None,
 3116|            source: None,
 3117|        };
...
 3132|            kind: crate::PublicationKind::Submission,
*3133|            version: 1,
 3134|            digest: "a".repeat(64),
 3135|        };
 3136|        let coding = crate::CodingTargetReference {
...
 3196|        let snapshot:crate::AssignmentSnapshot=serde_json::from_value(serde_json::json!({
*3197|            "coding_target":null,"dependency_gap":null,"handle":"implementation","identity":identity,"dispatch":"Applied","reason":null,"activity":"ResponseReady","conversation":"conversation","brief":"/brief","result_path":"/result","submission":{"schema":1,"identity":publication.assignment,"version":1,"outcome":"success","summary":"Independent review completed.","evidence":[],"unresolved":[],"review":report},"acceptance":"pending","result_version":1,"publication":null,"acceptance_ref":null,"history":[]... [col 295]
 3198|        })).unwrap();
 3199|        let response = Response::Delegation(Box::new(snapshot));
 3200|        let bytes = bincode::serde::encode_to_vec(&response, bincode::config::standard()).unwrap();
...
 3285|                    session: SessionId(1),
*3286|                    capability: "fixture".into(),
 3287|                    source: None,
 3288|                    controller: None,
 3289|                },
...
 3292|                    identity: identity.clone(),
*3293|                    result_version: 3,
 3294|                    control_epoch: 1,
 3295|                    target,
 3296|                    offset: 0,
...
 3308|            identity,
*3309|            result_version: 3,
 3310|            control_epoch: 1,
 3311|            offset: 0,
 3312|            next_offset: 4,
...
 3979|    #[test]
*3980|    fn wire_encoding_matches_snapshot_for_protocol_version() {
 3981|        let actual = actual();
 3982|        let expected = EXPECTED
 3983|            .iter()

[Some lines truncated to 512 bytes]

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs:3920-4049"}
mod wire_snapshot {
…
    const EXPECTED: &[(&str, &str)] = &[
…
        (
…
            "3a030707070707070707070707070707070707070707070707070707070707070707010203040101020463616c6c066f75747075740100",
        ),
        (
            "Request::OperatorExchange::Completed",
            "3a0307070707070707070707070707070707070707070707070707070707070707070102030401010301025b5d",
        ),
        (
            "Request::OperatorExchange::Failed",
            "3a03070707070707070707070707070707070707070707070707070707070707070701020304010104056572726f72",
        ),
        (
            "Request::OperatorWorker::Start",
            "3b0307070707070707070707070707070707070707070707070707070707070707070001020304",
        ),
        (
            "Request::OperatorWorker::Retire",
            "3b0307070707070707070707070707070707070707070707070707070707070707070101020304",
        ),
        (
            "Request::OperatorWorker::Stop",
            "3b0307070707070707070707070707070707070707070707070707070707070707070201020304",
        ),
        (
            "Response::Operator::Created",
            "1900040000010300056d6f64656c0568656c6c6f01010203040000",
        ),
        (
            "Response::Operator::Conversations",
            "190101040000010300056d6f64656c0568656c6c6f01010203040000",
        ),
        (
            "Response::Operator::History",
            "1902040000010300056d6f64656c0568656c6c6f01010203040000010102000568656c6c6f01017001056c6162656c0106737461626c6501022f77010501060101",
        ),
        (
            "Response::Operator::Closed",
            "1905040000010300056d6f64656c0568656c6c6f01010203040000",
        ),
        ("Response::Operator::Submitted", "1903010203040000"),
        ("Response::Operator::Stopped", "1904010203040200"),
        ("Response::OperatorExchange", "1a010203040100"),
        (
            "Response::OperatorWorker::Start",
            "1b000102030400056d6f64656c0568656c6c6f01017001056c6162656c0106737461626c6501022f77010501060101025b5d",
        ),
        ("Response::OperatorWorker::Retired", "1b0101020304"),
        ("ServerEvent::OperatorChanged", "0c01"),
        ("SessionScope::Workspace", "00"),
        ("SessionScope::OperatorConversation", "0101"),
        ("OperatorToolStatus::Succeeded", "00"),
        ("OperatorToolStatus::Failed", "01010e"),
        ("OperatorToolStatus::TimedOut", "02"),
        ("OperatorTurnState::Running", "00"),
        ("OperatorTurnState::Complete", "01"),
        ("OperatorTurnState::Interrupted", "02"),
        ("OperatorTurnState::Error", "03"),
        ("OperatorMessageContent::Worker", "01000161"),
        ("OperatorMessageContent::Interrupted", "02"),
    ];

    #[test]
    fn wire_encoding_matches_snapshot_for_protocol_version() {
        let actual = actual();
        let expected = EXPECTED
            .iter()
            .map(|(name, hex)| (name.to_string(), hex.to_string()))
            .collect::<Vec<_>>();
        if actual != expected {
            eprintln!(
                "wire snapshot for PROTOCOL_VERSION {}:",
                crate::PROTOCOL_VERSION
            );
            for (name, hex) in &actual {
                eprintln!("        (\"{name}\", \"{hex}\"),");
            }
            panic!(
                "wire encoding changed; bump PROTOCOL_VERSION in codec.rs and replace EXPECTED with the block above"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_with(size: TerminalSize) -> DashboardView {
        DashboardView {
            revision: 1,
            panes: vec![PaneTarget {
                session: SessionId(1),
                run: SessionRunId(4),
                size,
            }],
            focused: Some(SessionId(1)),
        }
    }

    #[test]
    fn validate_rejects_oversized_panes() {
        let oversized_rows = view_with(TerminalSize {
            rows: 1001,
            cols: 80,
        });
        let error = oversized_rows
            .validate()
            .expect_err("1001 rows must be rejected");
        assert!(
            error.contains("1000"),
            "error should name the limit: {error}"
        );

        let oversized_cols = view_with(TerminalSize {
            rows: 24,
            cols: 1001,
        });
        let error = oversized_cols
            .validate()
            .expect_err("1001 cols must be rejected");
        assert!(
            error.contains("1000"),
            "error should name the limit: {error}"
        );

        let at_limit = view_with(TerminalSize {
            rows: 1000,
            cols: 1000,
        });
        assert!(at_limit.validate().is_ok(), "1000 by 1000 must pass");
    }
}

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/**/*delegation*"}
# crates/ovrcr-runtime/src/
claude_delegation.rs
codex_delegation.rs
## server/
delegation_files.rs
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
### delegation_review/
### delegation_coding/

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src", "pattern": "MANAGED_AGENTS|agent run|auto-trust|--dangerously|bypass|trust"}
# crates/ovrcr-runtime/src/
## agent_runner.rs
 400|                                    input: &request,
*401|                                    native_root: trusted_native_root(&stream, &thread_native),
 402|                                    deadline: started + Duration::from_millis(800),
 403|                                })
 404|                            } else {
...
 466|// process tree; reject callers outside it even with a stolen token.
*467|fn trusted_native_root(
 468|    stream: &std::os::unix::net::UnixStream,
 469|    native: &std::sync::Mutex<Option<libc::pid_t>>,
 470|) -> bool {
## claude_recovery.rs
 103|            | "--verbose"
*104|            | "--dangerously-skip-permissions"
 105|            | "--allow-dangerously-skip-permissions"
 106|                if inline.is_none() =>
 107|            {
## codex_delegation.rs
 200|        "features":{"apps":false,"plugins":false,"browser_use":false,"computer_use":false,"in_app_browser":false,"image_generation":false,"goals":false,"memories":false,"shell_snapshot":false,"workspace_dependencies":false,"tool_suggest":false,"request_permissions_tool":false},
*201|        "projects":{workspace.to_string_lossy().as_ref():{"trust_level":"untrusted"}},
 202|        "shell_environment_policy":{"inherit":"none","ignore_default_excludes":false,"set":{"PATH":"/usr/bin:/bin","HOME":workspace.to_string_lossy()}},
 203|        "permissions":{"ovrcr-research":{"filesystem":fs,"network":{"enabled":false}}},
 204|        "mcp_servers":{MCP_SERVER:{"command":binary.to_string_lossy(),"args":["delegation-publisher"],"enabled":true,"required":true,"enabled_tools":["ovrcr_publish","ovrcr_publish_question"],"env_vars":["OVRCR_DELEGATION_SESSION","OVRCR_DELEGATION_TOKEN","OVRCR_DELEGATION_SOCKET","OVRCR_DELEGATION_HOME"],"startup_timeout_sec":10,"tool_timeout_sec":15,"default_tools_approval_mode":"approve"}}
## codex_recovery.rs
 257|fn process_cmdline(pid: libc::pid_t) -> Option<Vec<String>> {
*258|    // macOS has no /proc; trust the CODEX_HOME pid file and confirm the pid is live.
 259|    process_exists(pid).then(|| vec!["codex".into()])
 260|}
 261|
...
 331|            "--last",
*332|            "--dangerously-bypass-hook-trust",
 333|        ] {
 334|            assert!(
 335|                launch_options(&args(&["codex", option])).is_err(),
## config.rs
 378|    if schema_identity(&transaction)?.1 < 16 {
*379|        // Older writers discard trusted fallback evidence and typed acceptance.
 380|        transaction.pragma_update(None, "user_version", 16)?;
 381|    }
 382|    if schema_identity(&transaction)?.1 < 17 {

## cursor_cloud/
### run.rs
 851|                }
*852|                // Never trust the registered handle's locally cached cancelled status.
 853|                match lookup(&catalog) {
 854|                    Ok(Some(status)) => {
 855|                        admit(&C::Reconcile {

## server/
### cursor_quota.rs
 273|    }
*274|    // Sidecars belong to the real database. An alias must not bypass its WAL
 275|    // and return a checkpointed login for a previous account.
 276|    let resolved = path.canonicalize().map_err(|_| unreadable_auth())?;
 277|    let path = resolved.as_path();
### delegation.rs
 1803|                            Ok(policy) => policy,
*1804|                            Err(_) => return self.reject_initial(record, "generated native policy is unavailable or differs from the trusted recipe; no initial bytes written"),
 1805|                        };
 1806|                        if record
 1807|                            .native_policy
...
 2730|            ErrorCode::Conflict,
*2731|            "generated native policy differs from the trusted composer recipe",
 2732|        ));
 2733|    }
 2734|    Ok(serde_json::from_slice(&files.manifest)?)
### delegation_delivery.rs
 200|        });
*201|        // Parent PTY exit can bypass retained; keep its state pinned through commit.
 202|        parent.with_workflow_context(&auth, &workflow.authority, || {
 203|            let transaction = store.connection.unchecked_transaction()?;
 204|            store.save_workflow(&workflow)?;
### delegation_fallback.rs
 223|
*224|/// The Server supplies a trusted capture and current immutable contract. A caller
 225|/// cannot turn an arbitrary screen or its own success boolean into evidence.
 226|pub(super) fn prepare_recovery(
 227|    files: &FallbackFiles<'_>,
### delegation_fallback_routes.rs
 270|                if digest(&bytes) != reference.digest {
*271|                    bail!("trusted fallback capture changed");
 272|                }
 273|                let capture: FallbackCapture = serde_json::from_slice(&bytes)?;
 274|                let (contract, brief, original) = names(record);
### delegation_question_tests.rs
 380|#[test]
*381|fn fresh_answer_evidence_cannot_bypass_prior_applied_partial_unknown_or_pending_answer() {
 382|    for outcome in [
 383|        DispatchOutcome::Applied,
 384|        DispatchOutcome::Partial,
### delegation_recovery.rs
 528|            };
*529|            let trusted_keys = identifier(&key) && identifier(&handle);
 530|            gaps.push(WorkflowRecoveryGap {
*531|                workflow: trusted_keys.then_some(key),
*532|                handle: trusted_keys.then_some(handle),
 533|                identity: row.as_ref().map(|row| row.identity.clone()),
 534|                brief: row.as_ref().map(|row| row.root.join("brief-v1.json")),
 535|                result_path: row.as_ref().map(|row| {
...
 550|                }
*551|                gaps.push(WorkflowRecoveryGap { workflow: identifier(key).then(||key.clone()), handle: None, identity: None, brief: None, result_path: None, reason: "Retained authority record has unverifiable metadata and no trustworthy assignment attribution. Original evidence remains read-only; automatic recovery and transfer are unavailable.".into() });
 552|            }
 553|        }
 554|        if extra > 0 {
### quota.rs
 2153|            matches!(rpc.message(&state, Instant::now()), Ok(None)),
*2154|            "a complete buffered message bypassed the expired receive deadline"
 2155|        );
 2156|        let result = rpc.call("account/read", json!({}), &state, Instant::now());
 2157|        assert!(
...
 2164|            ),
*2165|            "a complete buffered reply bypassed the expired request deadline"
 2166|        );
 2167|    }
 2168|
### quota_probe.rs
 335|        }
*336|        if !answered && trust_dialog_for(&seen, &probe_path) {
 337|            let _ = writer.write_all(b"\x1b[B\r");
 338|            let _ = writer.flush();
 339|            answered = true;
...
 379|
*380|fn trust_dialog_for(seen: &str, probe_path: &str) -> bool {
*381|    seen.contains("Yes, I trust this folder") && seen.contains(probe_path)
 382|}
 383|
 384|fn end_child(child: &mut Box<dyn portable_pty::Child + Send + Sync>) {

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src", "pattern": "fn available_model|model_discovery|discover_models|--model|model_choices|model:"}
# crates/ovrcr-runtime/src/
## claude_delegation.rs
 121|/// differences are explicit and never claim identical complete MCP topology.
*122|pub fn launch_argv(executable: &Path, model: &str, artifacts: &Path) -> Vec<std::ffi::OsString> {
 123|    vec![
 124|        executable.as_os_str().into(),
*125|        "--model".into(),
 126|        model.into(),
 127|        "--permission-mode".into(),
 128|        "default".into(),
...
 151|}
*152|fn policy_digest(model: &str, profile: &Path) -> Result<String> {
 153|    Ok(format!(
 154|        "{:x}",
 155|        Sha256::digest(serde_json::to_vec(&serde_json::json!({
...
 263|            "stream-json",
*264|            "--model",
 265|            requested,
 266|            "--permission-mode",
 267|            "default",
## claude_recovery.rs
 75|        match name {
*76|            "--model" | "--permission-mode" | "--agent" | "--settings" | "--setting-sources"
 77|            | "--tools" | "--mcp-config" => {
 78|                let value = if let Some(value) = inline {
 79|                    value
...
 193|                "claude",
*194|                "--model=sonnet",
 195|                "--permission-mode",
 196|                "default",
 197|                "private task prompt"
...
 199|            .unwrap(),
*200|            vec!["--model", "sonnet", "--permission-mode", "default"]
 201|        );
 202|        assert!(
 203|            launch_options(&args(&["claude", "--", "private task prompt"]))
## codex_delegation.rs
 137|            model_provider: "openai".into(),
*138|            model: slug.into(),
 139|        });
 140|    }
 141|    choices.sort_by(|a, b| a.model.cmp(&b.model));
## codex_recovery.rs
 33|            | "--full-auto" => options.push(arg.into()),
*34|            "--model" | "-m" | "--profile" | "-p" | "--sandbox" | "-s" | "--ask-for-approval"
 35|            | "-a" | "--cd" | "-C" => {
 36|                let value = args
 37|                    .next()
## cursor_cloud.rs
 24|    pub source_sha: String,
*25|    pub model: String,
 26|}
 27|impl std::fmt::Display for CloudLaunchPreview {
 28|    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
...
 134|        source_sha: sha,
*135|        model: profile.model.clone(),
 136|    })
 137|}
 138|
...
 644|                .into(),
*645|            model: response["model"]["id"]
 646|                .as_str()
 647|                .context("creation model receipt missing")?
 648|                .into(),
## extension_recovery.rs
 71|            name,
*72|            "--model" | "--provider" | "--thinking" | "--tools" | "--models" | "--session-dir"
 73|        ) || (provider == AgentProvider::Omp && name == "--approval-mode");
 74|        if identity || value_option {
 75|            let value = if let Some(value) = inline {
...
 307|                "/exact/history.jsonl",
*308|                "--model=some-model",
 309|                control,
 310|                "PRIVATE_PROMPT",
 311|            ]
...
 318|                if provider == AgentProvider::Pi {
*319|                    vec!["--model", "some-model", "--no-approve"]
 320|                } else {
*321|                    vec!["--model", "some-model", "--approval-mode", "always-ask"]
 322|                }
 323|            );
 324|            for argument in [

## cursor_cloud/
### run.rs
 276|        credential_generation: format!("{:x}", Sha256::digest(key.as_bytes())),
*277|        model: creation.account.model.clone(),
 278|        prompt: if source_required {
 279|            format!(
 280|                "Before any other tool, run exactly `git rev-parse HEAD` and wait for its result to verify the selected source. Then follow this request:\n{prompt}"
...
 589|}
*590|fn terminal_status_model(value: &Value, model: &str) -> Result<()> {
 591|    let status = terminal(&value["status"])?;
 592|    if status == CloudRunStatus::Finished {
 593|        ensure!(

## retained/
### cloud_creation.rs
 564|                            agent_id: run_id.clone(),
*565|                            model: record.intent.account.model.clone(),
 566|                        }
 567|                        .validate()?;
 568|                        if agent_id != agent
...
 806|                    keychain_account: "work".into(),
*807|                    model: "composer-2.5".into(),
 808|                    repositories: vec!["https://github.com/acme/app".into()],
 809|                    owners: vec![],
 810|                    bridge_archive: "/sdk.tar.gz".into(),
...
 825|                agent_id: "agent-1".into(),
*826|                model: "composer-2.5".into(),
 827|            }),
 828|            sends: vec![SendRecord {
 829|                key: "d".repeat(64),
### operator.rs
 99|        provider: OperatorProvider,
*100|        model: &str,
 101|        cwd: &Path,
 102|    ) -> Result<OperatorConversationSummary> {
 103|        validate_operator_model(model).map_err(anyhow::Error::msg)?;
...
 483|            provider: conversation.provider,
*484|            model: conversation.model,
 485|            text,
 486|            context,
 487|            history: self.operator_model_history(identity.conversation)?,

## server/
### delegation.rs
 58|    #[serde(default)]
*59|    pub native_model: Option<crate::claude_delegation::ModelProof>,
 60|    #[serde(default)]
 61|    pub native_policy: Option<crate::claude_delegation::PolicyManifest>,
 62|    #[serde(default)]
...
 1250|                        review: None,
*1251|                        native_model: native_proof.clone(),
 1252|                        native_policy: None,
 1253|                        dependencies: dependency_witness,
 1254|                        obligation_revision: 1,
...
 1543|                                    .into_os_string(),
*1544|                                "--model".into(),
 1545|                                contract.model.clone().into(),
 1546|                                "--profile".into(),
 1547|                                crate::codex_delegation::PROFILE.into(),
...
 1567|                                contract.model_provider.clone().into(),
*1568|                                "--model".into(),
 1569|                                contract.model.clone().into(),
 1570|                            ]
 1571|                        };
...
 1858|                        child_binding: record.binding.as_ref().unwrap(),
*1859|                        model: &native_model,
 1860|                        text: &text,
 1861|                    }) {
 1862|                        Ok(crate::session::DelegatedDelivery::Applied(input)) => {
...
 2783|
*2784|fn available_models() -> Result<Vec<DelegationChoice>> {
 2785|    // Provider inventory is local to the same inherited profile as the eventual child.
 2786|    // File-backed bounded output avoids a full pipe blocking the probe at exit.
 2787|    let mut output = tempfile::tempfile()?;
...
 2829|            model_provider: backend.into(),
*2830|            model: model.into(),
 2831|        });
 2832|    }
 2833|    if serde_json::to_vec(&choices)?.len() > 512 * 1024 {
...
 2838|
*2839|fn available_model(
 2840|    contract: &ResearchContract,
 2841|) -> Result<Option<crate::claude_delegation::ModelProof>> {
 2842|    if contract.provider == "claude" {
...
 2882|            model_provider: "test".into(),
*2883|            model: "fixture-model".into(),
 2884|            acceptance_evidence: vec!["citation".into()],
 2885|            mandatory_capabilities: vec!["read".into()],
 2886|            limits: DelegationLimits::default(),
...
 2896|            review: None,
*2897|            native_model: None,
 2898|            native_policy: None,
 2899|            dependencies: None,
 2900|            obligation_revision: 1,
### delegation_archive.rs
 468|                    child_binding: &binding,
*469|                    model: &model,
 470|                    text: "",
 471|                    control: &control,
 472|                    control_epoch: request.expected_control_epoch,
### delegation_control.rs
 514|                    child_binding: &binding,
*515|                    model: &model,
 516|                    text: "",
 517|                    control: &captured,
 518|                    control_epoch: epoch,
### delegation_fallback_routes.rs
 102|                    child_binding: &child_binding,
*103|                    model: &model,
 104|                    text: "",
 105|                },
 106|                |agent| {
### delegation_followups.rs
 557|                .context("child assignment not bound")?,
*558|            model: &expected_native_model(&record)?,
 559|            text: &prompt,
 560|        }) {
 561|            Ok(crate::session::DelegatedDelivery::Applied(input)) => {
...
 722|                    child_binding: &child_binding,
*723|                    model: &model,
 724|                    text: "",
 725|                },
 726|                |agent| {
### delegation_interrupt_effects.rs
 91|                        child_binding: binding,
*92|                        model: &model,
 93|                        text: "",
 94|                        control: &record.control,
 95|                        control_epoch: epoch,
...
 185|                child_binding: &baseline.binding,
*186|                model: &model,
 187|                text: "",
 188|                control: &captured,
 189|                control_epoch: intent.control_epoch,
...
 319|                    child_binding: &baseline.binding,
*320|                    model: &model,
 321|                    text: "",
 322|                    control: &captured,
 323|                    control_epoch: epoch,
### delegation_policy.rs
 184|            model_provider: contract.model_provider.clone(),
*185|            model: contract.model.clone(),
 186|        };
 187|        let (live, children) = self.delegation_live_counts()?;
 188|        let selected = DelegationSelection {
...
 190|            model_provider: Some(choice.model_provider.clone()),
*191|            model: Some(choice.model.clone()),
 192|            required_limits: contract.limits.clone(),
 193|        };
 194|        Ok(resolve(
...
 209|
*210|    fn choice(backend: &str, model: &str) -> DelegationChoice {
 211|        DelegationChoice {
 212|            provider: DelegationHarness::Pi,
 213|            model_provider: backend.into(),
*214|            model: model.into(),
 215|        }
 216|    }
 217|
...
 225|        let selected = DelegationSelection {
*226|            model: Some("b".into()),
 227|            ..Default::default()
 228|        };
 229|        assert_eq!(
...
 233|        let policy = DelegationSettings {
*234|            default_model: Some("test/a".into()),
 235|            ..policy
 236|        };
 237|        let missing = DelegationSelection {
*238|            model: Some("missing".into()),
 239|            ..Default::default()
 240|        };
 241|        assert!(
...
 345|                model_provider: Some("openai".into()),
*346|                model: Some("exact-model".into()),
 347|                ..Default::default()
 348|            };
 349|            let native = DelegationChoice {
...
 351|                model_provider: "openai".into(),
*352|                model: "exact-model".into(),
 353|            };
 354|            for unavailable in [false, true] {
 355|                let report = resolve(
### delegation_questions.rs
 760|            child_binding: &child_binding,
*761|            model: &model,
 762|            text: "",
 763|        };
 764|        child
...
 887|            child_binding: &child_binding,
*888|            model: &model,
 889|            text: &prompt,
 890|        }) {
 891|            Ok(crate::session::DelegatedDelivery::Applied(input)) => {
### operator_tests.rs
 13|        provider: OperatorProvider::ChatGpt,
*14|        model: "fixture".into(),
 15|    });
 16|    assert!(matches!(
 17|        state.handle_request(&mut ClientRole::Control, create.clone()),
...
 206|                provider: OperatorProvider::ChatGpt,
*207|                model: "offline-fixture".into(),
 208|            }))
 209|        else {
 210|            panic!("create")
...
 1056|                provider: OperatorProvider::ChatGpt,
*1057|                model: "fixture".into(),
 1058|            },
 1059|            "list" => OperatorRequest::List {
 1060|                after: 0,
### quota_probe.rs
 244|    let mut child_cmd = CommandBuilder::new(command);
*245|    child_cmd.arg("--model");
 246|    child_cmd.arg("haiku");
 247|    child_cmd.arg("--tools");
 248|    child_cmd.arg("");
### tests.rs
 6604|                keychain_account: "work".into(),
*6605|                model: "composer-2.5".into(),
 6606|                repositories: vec!["https://github.com/acme/app".into()],
 6607|                owners: vec![],
 6608|                bridge_archive: "/fixture-sdk.tar.gz".into(),
...
 6649|                agent_id: "agent-373".into(),
*6650|                model: "composer-2.5".into(),
 6651|            },
 6652|        };
 6653|        rejected(f.command(
...
 6702|                agent_id: "agent-373".into(),
*6703|                model: "composer-2.5".into(),
 6704|            },
 6705|        };
 6706|        assert_eq!(
...
 6748|            credential_generation: creation.credential_generation.clone(),
*6749|            model: creation.account.model.clone(),
 6750|            prompt: "Only inspect the selected source".into(),
 6751|        };
 6752|        let submit = |operation: &str, command| {
...
 6776|                        agent_id: send.agent_id.clone(),
*6777|                        model: send.model.clone(),
 6778|                    },
 6779|                },
 6780|            ),
...
 6839|            credential_generation: creation.credential_generation.clone(),
*6840|            model: creation.account.model.clone(),
 6841|            prompt: "Inspect source".into(),
 6842|        };
 6843|        let submit = |op: &str, c| f.command(&auth, op, AgentCommand::CloudCreation(Box::new(c)));
...
 6857|                        agent_id: send.agent_id.clone(),
*6858|                        model: send.model.clone(),
 6859|                    },
 6860|                },
 6861|            ),
...
 7134|            credential_generation: creation.credential_generation.clone(),
*7135|            model: creation.account.model.clone(),
 7136|            prompt: "inspect".into(),
 7137|        };
 7138|        let submit = |op: &str, c| f.command(&auth, op, AgentCommand::CloudCreation(Box::new(c)));
...
 7152|                        agent_id: send.agent_id.clone(),
*7153|                        model: send.model.clone(),
 7154|                    },
 7155|                },
 7156|            ),
...
 7265|        MetricsSample {
*7266|            model: Some("test".into()),
 7267|            context: measurement(ContextSample {
 7268|                used_tokens: Some(10),
 7269|                capacity_tokens: Some(100),
...
 8423|                    source: ovrcr_protocol::context::ContextSource::Generic,
*8424|                    model: None,
 8425|                    conversation: None,
 8426|                    used_tokens: Some(10),
 8427|                    capacity_tokens: Some(100)

Showing files 1-20 of 28. Use skip=20 for the next page, or narrow paths/pattern.

SOURCE LOOKUP {"path": "Cargo.toml"}
[workspace]
members = [".", "crates/ovrcr-protocol", "crates/ovrcr-terminal", "crates/ovrcr-runtime", "crates/ovrcr-tui"]
default-members = [".", "crates/ovrcr-protocol", "crates/ovrcr-terminal", "crates/ovrcr-runtime", "crates/ovrcr-tui"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
# Rig 0.45 requires Rust 1.97.1.
rust-version = "1.97.1"
license = "MIT"

[workspace.dependencies]
anyhow = "1"
base64 = "0.22"
bincode = { version = "2", features = ["serde"] }
chrono = "0.4"
chrono-tz = "0.10"
clap = { version = "4", features = ["derive"] }
# Exact pin: the DST gap and overlap behaviour tests/task_schedule.rs relies on
# is verified against this release; bump deliberately and rerun that suite.
croner = "=3.0.1"
crossterm = { version = "0.29", features = ["use-dev-tty", "osc52"] }
directories = "6"
# The GUI helper builds a macOS app bundle only, so no Linux windowing backend.
eframe = { version = "0.36.1", default-features = false, features = ["accesskit", "default_fonts", "glow"] }
libc = "0.2"
parking_lot = "0.12"
portable-pty = "0.9"
ratatui = "0.30"
rusqlite = { version = "0.40.2", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["raw_value"] }
sha2 = "0.10.9"
signal-hook = "0.3"
toml = "0.9"
toml_edit = "0.25"
unicode-width = "0.2"
ureq = { version = "2.12", default-features = false, features = ["tls"] }
tempfile = "3"
vt100 = "0.16"
ovrcr-protocol = { path = "crates/ovrcr-protocol" }
ovrcr-terminal = { path = "crates/ovrcr-terminal" }
ovrcr-runtime = { path = "crates/ovrcr-runtime" }
ovrcr-tui = { path = "crates/ovrcr-tui" }

[package]
name = "ovrcr"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish = false
default-run = "ovrcr"

[features]
gui = ["dep:eframe"]
acceptance-diagnostics = ["ovrcr-runtime/acceptance-diagnostics"]

[[bin]]
name = "ovrcr-gui"
path = "src/bin/ovrcr-gui.rs"
required-features = ["gui"]

[dependencies]
rig = { version = "=0.45.0", default-features = false, features = ["agent", "reqwest", "rustls"] }
tokio = { version = "1", features = ["rt", "time", "net", "io-util", "sync", "macros", "process"] }
futures = "0.3"
reqwest = { version = "0.13", default-features = false, features = ["rustls", "json", "form"] }
jsonwebtoken = { version = "10", default-features = false, features = ["rust_crypto"] }
base64 = { workspace = true }
toml = { workspace = true }
toml_edit = { workspace = true }
anyhow = { workspace = true }
chrono = { workspace = true }
clap = { workspace = true }
crossterm = { workspace = true }
directories = { workspace = true }
eframe = { workspace = true, optional = true }
libc = { workspace = true }
ovrcr-protocol = { workspace = true }
ovrcr-terminal = { workspace = true }
ovrcr-runtime = { workspace = true }
ovrcr-tui = { workspace = true }
portable-pty = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
tempfile = { workspace = true }
vt100 = { workspace = true }

[dev-dependencies]
bincode = { workspace = true }
sha2 = { workspace = true }
ratatui = { workspace = true }
signal-hook = { workspace = true }
rusqlite = { workspace = true }
tempfile = { workspace = true }

SOURCE LOOKUP {"path": "tests/agent_setup.rs"}
use serde_json::{Value, json};
…
use std::process::{Command, Stdio};

// These setup/doctor fixtures launch temporary executables under bounded native
// probe deadlines. Coordinate their cold starts; this suite checks reporting and
// cleanup contracts, not concurrent provider-launch throughput. Use the same
// poison-tolerant env_lock pattern as server_lifecycle so every assertion still
// runs after an unrelated fixture failure.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> { … }

fn command(root: &tempfile::TempDir) -> Command { … }
fn setup(root: &tempfile::TempDir, value: &Value) -> (Value, String) { … }
#[test]
fn setup_preserves_settings_and_external_renderer_bytes_once() { … }
#[test]
fn setup_migrates_only_exact_legacy_statusline_and_marks_hooks() { … }
#[test]
fn setup_adds_status_line_refresh_interval_and_keeps_the_users() { … }
#[test]
fn setup_quotes_actual_executable_path() { … }
#[test]
fn doctor_reports_partial_config_proof_and_any_version_without_secrets() { … }

#[test]
fn doctor_inspects_unbound_and_unavailable_sessions_without_private_values() { … }

#[test]
fn doctor_interrupt_cleans_its_owned_version_probe_before_exit() { … }

#[test]
fn doctor_does_not_certify_filtered_hooks_wrong_types_or_statusline_suffixes() { … }

#[test]
fn codex_setup_preserves_handlers_trust_and_quotes_noop_helper() { … }

#[test]
fn codex_doctor_defaults_dispatch_and_reports_versions_without_gate_server_or_secrets() { … }

#[test]
fn codex_doctor_missing_session_and_failed_inspection_do_not_start_a_server() { … }

#[test]
fn codex_doctor_checks_supplied_hooks_without_certifying_trust() { … }

#[test]
fn doctor_reports_tested_releases_as_evidence_and_gates_no_version() { … }

#[test]
fn retarget_hooks_rewrites_managed_reporter_paths_and_stops() { … }

#[test]
fn setup_repairs_stale_owned_claude_statusline_without_wrapping_it() { … }

#[test]
fn codex_setup_repairs_stale_owned_handler_and_preserves_comments() { … }

#[test]
fn codex_setup_repairs_inline_filtered_hooks_once_without_changing_native_approvals() { … }

#[test]
fn codex_setup_keeps_already_configured_native_value_formatting() { … }

#[test]
fn cursor_cloud_requires_managed_admission_even_for_direct_adapter() { … }

[…977ln elided; re-read needed ranges, e.g. tests/agent_setup.rs:2-3,13-17]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/lifecycle.rs:1-205"}
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
…
}

[Showing lines 1-208 of 295. Use :209 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src", "pattern": "enum LifecycleOutcome|struct LifecycleOutcome|LifecycleResult|client_token|CreatedWorkspace"}
# crates/ovrcr-protocol/src/
## wire.rs
 501|
*502|/// Result of a finished Lifecycle job, correlated by `client_token`.
 503|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*504|pub enum LifecycleOutcome {
 505|    Succeeded,
 506|    CreatedSession(Box<SessionSummary>),
 507|    Failed { code: ErrorCode, message: String },
...
 542|    },
*543|    /// A Lifecycle job finished. `client_token` is the request_id that accepted
 544|    /// the job on the Dashboard connection.
 545|    LifecycleCompleted {
*546|        client_token: u64,
 547|        op: LifecycleOp,
 548|        outcome: LifecycleOutcome,
 549|    },
...
 1256|                ServerEvent::LifecycleCompleted {
*1257|                    client_token: 7,
 1258|                    op: LifecycleOp::CreateSession,
 1259|                    outcome: LifecycleOutcome::Succeeded,
 1260|                },

SOURCE LOOKUP {"path": "tests", "pattern": "Live::binary|Live::thread|run_dashboard|OuterDashboard|outer PTY|outer_pty|CreateSessionRequest"}
# tests/
## automatic_titles.rs
 6|    AgentBinding, AgentCommand, AgentOperationResult, AgentProvider, AgentSecret, BranchRequest,
*7|    ClientMessage, ConversationReference, CreateSessionRequest, DashboardView, ErrorCode,
 8|    ExtensionConversation, Request, ReserveAgent, Response, ServerEvent, ServerMessage, SessionId,
 9|    SessionLaunch, SessionPhase, SessionSummary, SupervisorAuth, SupervisorRequest, client,
 10|    connect_server, read_frame, write_frame,
...
 17|fn fixture() -> Live {
*18|    let live = Live::binary();
 19|    assert_eq!(
 20|        live.request(Request::AddProject {
 21|            name: PROJECT.into(),
...
 617|            &live,
*618|            Request::CreateSession(CreateSessionRequest {
 619|                kind: ovrcr_protocol::SessionKind::Agent { name: name.into() },
 620|                project: PROJECT.into(),
 621|                workspace: WORKSPACE.into(),
...
 738|        &live,
*739|        Request::CreateSession(CreateSessionRequest {
 740|            kind: ovrcr_protocol::SessionKind::Terminal,
 741|            project: PROJECT.into(),
 742|            workspace: WORKSPACE.into(),
...
 750|        &live,
*751|        Request::CreateSession(CreateSessionRequest {
 752|            kind: ovrcr_protocol::SessionKind::Agent {
 753|                name: "grok".into(),
 754|            },
...
 793|        &live,
*794|        Request::CreateSession(CreateSessionRequest {
 795|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 796|            project: PROJECT.into(),
 797|            workspace: WORKSPACE.into(),
...
 840|        &live,
*841|        Request::CreateSession(CreateSessionRequest {
 842|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 843|            project: PROJECT.into(),
 844|            workspace: WORKSPACE.into(),
...
 902|        &missing,
*903|        Request::CreateSession(CreateSessionRequest {
 904|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 905|            project: PROJECT.into(),
 906|            workspace: WORKSPACE.into(),
...
 966|        &live,
*967|        Request::CreateSession(CreateSessionRequest {
 968|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 969|            project: PROJECT.into(),
 970|            workspace: WORKSPACE.into(),
...
 1031|        &live,
*1032|        Request::CreateSession(CreateSessionRequest {
 1033|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 1034|            project: PROJECT.into(),
 1035|            workspace: WORKSPACE.into(),
...
 1094|        &live,
*1095|        Request::CreateSession(CreateSessionRequest {
 1096|            kind: ovrcr_protocol::SessionKind::Agent {
 1097|                name: "hermes".into(),
 1098|            },
...
 1210|        &live,
*1211|        Request::CreateSession(CreateSessionRequest {
 1212|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 1213|            project: PROJECT.into(),
 1214|            workspace: WORKSPACE.into(),
...
 1264|        &live,
*1265|        Request::CreateSession(CreateSessionRequest {
 1266|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 1267|            project: PROJECT.into(),
 1268|            workspace: WORKSPACE.into(),
...
 1313|        &live,
*1314|        Request::CreateSession(CreateSessionRequest {
 1315|            kind: ovrcr_protocol::SessionKind::Terminal,
 1316|            project: PROJECT.into(),
 1317|            workspace: WORKSPACE.into(),
...
 1377|        &live,
*1378|        Request::CreateSession(CreateSessionRequest {
 1379|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 1380|            project: PROJECT.into(),
 1381|            workspace: WORKSPACE.into(),
...
 1461|        &live,
*1462|        Request::CreateSession(CreateSessionRequest {
 1463|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 1464|            project: PROJECT.into(),
 1465|            workspace: WORKSPACE.into(),
...
 1504|        &live,
*1505|        Request::CreateSession(CreateSessionRequest {
 1506|            kind: ovrcr_protocol::SessionKind::Agent { name: "pi".into() },
 1507|            project: PROJECT.into(),
 1508|            workspace: WORKSPACE.into(),
...
 1665|        let launch = title_program();
*1666|        Request::CreateSession(CreateSessionRequest {
 1667|            kind: ovrcr_protocol::SessionKind::Terminal,
 1668|            project: PROJECT.into(),
 1669|            workspace: WORKSPACE.into(),
...
 1766|                &live,
*1767|                Request::CreateSession(CreateSessionRequest {
 1768|                    kind: kind.clone(),
 1769|                    project: PROJECT.into(),
 1770|                    workspace: WORKSPACE.into(),
...
 1832|            &live,
*1833|            Request::CreateSession(CreateSessionRequest {
 1834|                kind: launch.kind,
 1835|                project: PROJECT.into(),
 1836|                workspace: WORKSPACE.into(),
## claude_quota_probe.rs
 736|        run.fixture
*737|            .request(Request::CreateSession(CreateSessionRequest {
 738|                kind: SessionKind::Agent {
 739|                    name: "claude".into(),
 740|                },
## claude_reporting_load.rs
 82|    fn new() -> Self {
*83|        let fixture = Self(Live::thread());
 84|        let mut control = connect(&fixture.socket);
 85|        assert_eq!(
 86|            send(
...
 215|        let path = fixture.root.path().join(format!("identity-{index}"));
*216|        let Response::CreatedSession(session) = send(&mut control, Request::CreateSession(CreateSessionRequest { kind: ovrcr_protocol::SessionKind::Terminal, project: "load".into(), workspace: "work".into(), name: format!("load-{index}"), label: None,
 217|        argv: vec!["sh".into(), "-c".into(), "printf '%s' \"$OVRCR_HOOK_TOKEN\" > \"$1\"; while IFS= read -r line; do :; done".into(), "load".into(), path.clone().into_os_string()], })) else { panic!("create session"); };
 218|        let deadline = Instant::now() + Duration::from_secs(5);
 219|        let token = loop {
## cli.rs
 2615|    use ovrcr::protocol::{SessionKind, SessionPhase};
*2616|    let fixture = live::Live::binary().bounded();
 2617|    fixture.ready("feature/hermes-harness");
 2618|    let native = fixture.root.path().join("hermes");
 2619|    std::fs::write(&native, "#!/bin/sh\nprintf 'HERMES_FIXTURE_READY\\n'\nwhile IFS= read -r line; do [ \"$line\" = quit ] && exit 0; printf 'HERMES_REPLY:%s\\nBusy Ready approval\\n' \"$line\"; done\n").unwrap();
...
 2964|fn cursor_rejected_managed_launches_preserve_argv_without_plugin_or_binding() {
*2965|    let fixture = live::Live::binary().bounded();
 2966|    fixture.ready("feature/cursor-harness");
 2967|    let native = fixture.root.path().join("cursor-agent");
 2968|    let oversized_help = format!("  --plugin-dir <path>\\n{}", "x".repeat(16 * 1024));
...
 3054|    use ovrcr::protocol::{AgentActivity, AgentProvider, SessionKind, SessionPhase};
*3055|    let fixture = live::Live::binary().bounded();
 3056|    fixture.ready("feature/cursor-harness");
 3057|    let native = fixture.root.path().join("cursor-agent");
 3058|    let probe = fixture.root.path().join("plugin-path");
...
 3241|fn stable_titles_rename_reset_and_reopen_through_cli() {
*3242|    let fixture = live::Live::binary();
 3243|    fixture.ready("feature/cli-titles");
 3244|    let run = |args: &[&str]| {
 3245|        let mut command = fixture.command();
## client_startup.rs
 71|fn dashboard_startup_offers_missing_hooks_and_decline_preserves_profiles() -> Result<()> {
*72|    let fixture = live::Live::binary();
 73|    profiles(&fixture)?;
 74|    let claude = fixture.root.path().join(".claude/settings.json");
 75|    let codex = fixture.root.path().join(".codex/config.toml");
...
 115|fn dashboard_accepts_hook_offers_without_changing_native_approvals() -> Result<()> {
*116|    let fixture = live::Live::binary();
 117|    profiles(&fixture)?;
 118|    let mut terminal = launch(&fixture)?;
 119|    let result = (|| -> Result<()> {
## client_startup_server.rs
 80|fn serving_fixture() -> Live {
*81|    let fixture = Live::binary();
 82|    // A successful request establishes that startup reached the accept loop;
 83|    // the shared fixture's socket-existence wait alone does not establish it.
 84|    assert!(matches!(
## cursor_cloud.rs
 4|use live::{Live, PROJECT, WORKSPACE};
*5|use ovrcr::protocol::{CreateSessionRequest, Request, Response, SessionKind, SessionSummary};
 6|use serde_json::{Value, json};
 7|use std::{
 8|    fs,
...
 99|            self.live
*100|                .request(Request::CreateSession(CreateSessionRequest {
 101|                    project: PROJECT.into(),
 102|                    workspace: WORKSPACE.into(),
 103|                    name: "cloud".into(),
...
 3289|            .live
*3290|            .request(Request::CreateSession(CreateSessionRequest {
 3291|                project: PROJECT.into(),
 3292|                workspace: WORKSPACE.into(),
 3293|                name: "second-cloud".into(),
## git_lifecycle.rs
 55|fn add_project_creates_a_missing_workspace_root() {
*56|    let fixture = Live::binary();
 57|
 58|    let workspace_root = fixture.root.path().join("workspaces").join("demo");
 59|    assert!(!workspace_root.exists());
...
 98|fn add_project_refuses_relative_paths_without_creating_anything() {
*99|    let fixture = Live::thread();
 100|
 101|    // A relative workspace root is created under the server's working directory, nowhere near
 102|    // the path the caller named, so it is refused before anything is created.
...
 152|fn add_project_accepts_the_current_checkout_branch() {
*153|    let fixture = Live::binary();
 154|    live::git(
 155|        &fixture.repo,
 156|        &["checkout", "-b", "fix/audio-skip-bluetooth-mic"],
...
 200|fn add_project_leaves_no_workspace_root_when_the_repository_is_invalid() {
*201|    let fixture = Live::thread();
 202|    let workspace_root = fixture.root.path().join("workspaces").join("unvalidated");
 203|    let missing_repo = fixture.root.path().join("not-a-repository");
 204|    assert!(!workspace_root.exists());
...
 1285|fn automatic_local_terminals_policy_controls_provisioning_paths() {
*1286|    use ovrcr::protocol::{CreateSessionRequest, SessionKind, SessionLaunch};
 1287|    use std::time::{Duration, Instant};
 1288|
 1289|    for (policy, expect_root, expect_feature) in [
...
 1386|            for response in [
*1387|                fixture.request(Request::CreateSession(CreateSessionRequest {
 1388|                    project: "demo".into(),
 1389|                    workspace: "feature".into(),
 1390|                    name: format!("explicit-{label}"),
## gui.rs
 731|#[test]
*732|fn outer_pty_parses_attributes_and_resize_reaches_the_tty() -> Result<()> {
 733|    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
 734|    command.args([
 735|        "-c",
...
 878|    use ovrcr::protocol::{
*879|        BranchRequest, CreateSessionRequest, Request, Response, SessionKind, client,
 880|    };
 881|    use std::os::unix::fs::PermissionsExt;
 882|
...
 942|
*943|    let Response::CreatedSession(manual) = request(Request::CreateSession(CreateSessionRequest {
 944|        project: "consigint".into(),
 945|        workspace: "policy-off".into(),
 946|        name: "policy-manual".into(),
## operator_e2e.rs
 1132|        let Response::CreatedSession(other) =
*1133|            live.request(Request::CreateSession(protocol::CreateSessionRequest {
 1134|                project: live::PROJECT.into(),
 1135|                workspace: live::WORKSPACE.into(),
 1136|                name: "other".into(),
## operator_scope.rs
 11|fn compiled_terminal_list_exposes_idle_operator_scope_and_identity() {
*12|    let live = live::Live::binary();
 13|    let mut dashboard = connect_server(&live.socket).unwrap();
 14|    dashboard
 15|        .set_read_timeout(Some(Duration::from_secs(5)))
## pi_delegation.rs
 160|    let Response::CreatedSession(parent) =
*161|        live.request(Request::CreateSession(CreateSessionRequest {
 162|            project: PROJECT.into(),
 163|            workspace: workspace_id.clone(),
 164|            name: "parent".into(),
...
 652|        let Response::CreatedSession(session) =
*653|            live.request(Request::CreateSession(CreateSessionRequest {
 654|                project: PROJECT.into(),
 655|                workspace: workspace_id.clone(),
 656|                name: format!("capacity-{index}"),
...
 731|    let Response::CreatedSession(parent) =
*732|        live.request(Request::CreateSession(CreateSessionRequest {
 733|            project: PROJECT.into(),
 734|            workspace: workspace_id.into(),
 735|            name: "parent".into(),
...
 918|    };
*919|    let live = Live::binary();
 920|    live.ready("input-progress");
 921|    let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
 922|        panic!("inventory")
...
 1192|fn control_proxy_post_handshake_disconnect_preserves_the_next_request() {
*1193|    let live = Live::binary();
 1194|    let mut proxy = ControlProxy::new(
 1195|        live.root.path().join("disconnect.sock"),
 1196|        live.socket.clone(),
...
 1589|        let Response::CreatedSession(parent) =
*1590|            live.request(Request::CreateSession(CreateSessionRequest {
 1591|                project: PROJECT.into(),
 1592|                workspace: workspace.clone(),
 1593|                name: "parent".into(),
## pi_package.rs
 5|use live::{Live, PROJECT};
*6|use ovrcr::protocol::{CreateSessionRequest, Request, Response, SessionKind};
 7|use serde_json::{Value, json};
 8|use sha2::{Digest, Sha256};
 9|use std::{
...
 150|        let Response::CreatedSession(parent) =
*151|            live.request(Request::CreateSession(CreateSessionRequest {
 152|                project: PROJECT.into(),
 153|                workspace: live::WORKSPACE.into(),
 154|                name: "package-parent".into(),

## pi_delegation/
### cancellation.rs
 550|    let Response::CreatedSession(parent) =
*551|        f.live.request(Request::CreateSession(CreateSessionRequest {
 552|            project: PROJECT.into(),
 553|            workspace: f.workspace.clone(),
 554|            name: "successor".into(),
### codex.rs
 101|        let Response::CreatedSession(parent) =
*102|            live.request(Request::CreateSession(CreateSessionRequest {
 103|                project: PROJECT.into(),
 104|                workspace: workspace.clone(),
 105|                name: "parent".into(),
### delivery.rs
 773|    let Response::CreatedSession(successor) =
*774|        f.live.request(Request::CreateSession(CreateSessionRequest {
 775|            project: PROJECT.into(),
 776|            workspace: f.workspace.clone(),
 777|            name: "delivery-successor".into(),
...
 1317|    let Response::CreatedSession(successor) =
*1318|        f.live.request(Request::CreateSession(CreateSessionRequest {
 1319|            project: PROJECT.into(),
 1320|            workspace: f.workspace.clone(),
 1321|            name: "archive-transfer-target".into(),
...
 1965|fn nonexistent_lifecycle_targets_keep_not_found_across_sqlite_integer_boundary() {
*1966|    let live = Live::binary();
 1967|    for id in [i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
 1968|        let session = SessionId(id);
 1969|        let expected_run = SessionRunId(1);
...
 2054|    let Response::CreatedSession(ordinary) =
*2055|        f.live.request(Request::CreateSession(CreateSessionRequest {
 2056|            project: PROJECT.into(),
 2057|            workspace: f.workspace.clone(),
 2058|            name: "archive-unrelated-input".into(),
### fallback.rs
 615|    let Response::CreatedSession(second) =
*616|        f.live.request(Request::CreateSession(CreateSessionRequest {
 617|            project: PROJECT.into(),
 618|            workspace: f.workspace.clone(),
 619|            name: "fallback-successor".into(),
...
 1151|    let Response::CreatedSession(parent) =
*1152|        f.live.request(Request::CreateSession(CreateSessionRequest {
 1153|            project: PROJECT.into(),
 1154|            workspace: f.workspace.clone(),
 1155|            name: name.into(),
### observation.rs
 51|        let Response::CreatedSession(parent) =
*52|            live.request(Request::CreateSession(CreateSessionRequest {
 53|                project: PROJECT.into(),
 54|                workspace: workspace.clone(),
 55|                name: "parent".into(),
### recovery.rs
 464|            .live
*465|            .request(Request::CreateSession(CreateSessionRequest {
 466|                project: PROJECT.into(),
 467|                workspace: fixture.workspace.clone(),
 468|                name: format!("new-parent-{unique}"),
### review.rs
 806|            .live
*807|            .request(Request::CreateSession(CreateSessionRequest {
 808|                project: PROJECT.into(),
 809|                workspace: fixture.workspace.clone(),
 810|                name: "owned-review-capacity-occupant".into(),

Showing files 1-20 of 32+. Use skip=20 for the next page, or narrow paths/pattern.

SOURCE LOOKUP {"path": "crates;tests", "pattern": "SessionLaunch \\{"}
# crates/

## ovrcr-protocol/src/
### wire.rs
 178|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*179|pub struct SessionLaunch {
 180|    pub argv: Vec<OsString>,
 181|    pub label: Option<String>,
 182|    pub kind: SessionKind,
...
 911|                    branch: BranchRequest::Existing { branch: "c".into() },
*912|                    launch: Some(SessionLaunch {
 913|                        argv: vec!["sh".into()],
 914|                        label: None,
 915|                        kind: SessionKind::Terminal,

## ovrcr-runtime/src/server/
### delegation.rs
 1591|                                },
*1592|                                super::WorkspaceLaunch::Session(SessionLaunch {
 1593|                                    argv,
 1594|                                    label: Some("Pi coding".into()),
 1595|                                    kind: SessionKind::Agent { name: "pi".into() },
### mod.rs
 2299|                (
*2300|                    ovrcr_protocol::SessionLaunch {
 2301|                        argv: vec![shell],
 2302|                        label: None,
 2303|                        kind: SessionKind::Terminal,

## ovrcr-tui/src/dashboard/
### palette.rs
 2756|                .map(|a| {
*2757|                    Some(SessionLaunch {
 2758|                        argv: a.argv,
 2759|                        kind: SessionKind::Agent {
 2760|                            name: a.name.clone(),
...
 2770|                }),
*2771|            "Terminal" => Ok(Some(SessionLaunch {
 2772|                argv: if value("Command").is_empty() {
 2773|                    self.shell_argv()
 2774|                } else {
...
 5152|            Request::CreateWorkspaceWithLaunch {
*5153|                launch: Some(SessionLaunch { label: None, .. }),
 5154|                ..
 5155|            }
 5156|        ));

# tests/
## automatic_titles.rs
 42|
*43|fn title_program() -> SessionLaunch {
*44|    SessionLaunch { kind: ovrcr_protocol::SessionKind::Terminal, argv: vec!["/bin/sh".into(), "-c".into(),
 45|        "stty -echo; printf 'TITLE-READY\\n'; while IFS= read -r title; do [ \"$title\" = EXIT ] && exit 0; printf '\\033]2;%s\\007applied:%s\\n' \"$title\" \"$title\"; done".into()],
 46|    label: Some("owned title fixture".into()), }
 47|}
...
 3121|    let live = fixture();
*3122|    let response = live.request(workspace(Some(SessionLaunch {
 3123|        kind: ovrcr_protocol::SessionKind::Terminal,
 3124|        argv: vec!["/does-not-exist/ovrcr-owned-test".into()],
 3125|        label: None,
## git_lifecycle.rs
 1401|                    },
*1402|                    launch: Some(SessionLaunch {
 1403|                        argv: argv.clone(),
 1404|                        label: None,
 1405|                        kind: kind.clone(),

## pi_delegation/
### coding.rs
 764|        },
*765|        launch: Some(SessionLaunch {
 766|            argv: vec![
 767|                fixture
 768|                    .live

SOURCE LOOKUP {"path": "docs/development/runtime.md"}
# Runtime ownership and lifecycle

Notification navigation lives in `server::navigation` and the existing
`ActiveDashboard` slot. Startup caches a bounded SHA256 identity of the actual
CLI executable and creates a fresh Server-lifetime token. A connect-only callback
must match that context and an unarchived retained run. One pending offer is
stored per current owner with a 1.2-second budget. Lifecycle admission uses
`try_lock`; a busy or stopping owner yields a no-op instead of deferred work.
Confirmation is queued on the sole Dashboard writer while the mutation lock
still excludes lifecycle changes. The Dashboard compares its current hierarchy
before applying; the Server revalidates owner, deadline, callback connection and
run again on the applied receipt. Only that receipt may expose current socket
peer PID/birth/terminal context for native parent activation. This path never
uses terminal attach/select or launches a managed process.

- Give each piece of state one authority. Avoid parallel stores for selection, geometry, readiness, request completion, or process ownership. Compatibility helpers must delegate to the same implementation.
- Respect the existing lock order and spawn/registration boundary. Keep owner verification and state publication atomic. `Session::spawn_registered` is the only production spawn; it publishes the session before the PTY reader and child waiter start. The two `cfg(test)` hook constructors publish at that same point with a no-op register, so no spawn skips publication. Delayed cleanup from an old connection must leave a replacement owner's state intact.
- Preserve the synchronous dispatcher and sole socket writer. Socket writes must not block the parser dispatcher. Bound queues and retained state; preserve control responses and lifecycle events or disconnect explicitly when they cannot be delivered. `reporting_channel` is the only queue constructor; its monitor argument turns the accounting on for the `acceptance-diagnostics` build and for tests that assert on it, and nothing else varies. The `after_send` / `before_wait` ordering hooks belong to that accounting path and fire only on a monitored channel; an unmonitored one is a plain `mpsc::sync_channel`.
- Apply final PTY output before exit notification. Detach, hide, and pane close change subscriptions; they must not terminate sessions.
- Signal only process groups whose ownership the current runtime or fixture established. A discovered or remembered PID is not ownership proof. Never use broad process-name cleanup.
- Live PTYs and screens belong to the running server. Detach/reattach preserves the current run. Reopen starts a fresh process in the same durable session row; never restore a PTY, output, or activity from metadata.
- Preserve atomic persistence and failure boundaries. A failed write must not be reported as committed, and a partial external side effect must not be described as rolled back.

The active dashboard (identity, queue, view, geometry) lives in `server/dashboard.rs`. Prove ownership with `ActiveDashboard::owns`; never compare identities inline.

`SessionStore` is the durable authority for session identity, run intent and
recovery metadata. Persist run intent before spawning; hold `mutation_lock`
through live-slot admission and publication. Only Running and Paused runs count
toward fifty slots. Reject stale run-tagged events, input and completion paths.
A known pre-process launch failure is retryable; a failure after process creation
must not claim the process or its descendants are gone.

Operator conversations use those same retained rows and live slots with explicit
global scope. Submission commits its user message and next run together before the
shared prepared-run spawn body; it never advances that run twice. Current-owner
Dashboard submit/Stop work runs on bounded, tracked control threads so the reader
continues accepting terminal input. Completed handles are pruned during admission,
and shutdown closes admission and joins controls before stopping the dispatcher.
Authenticated Start and event batches run on their scoped connection thread; they
hold retained then Session authority only through validation and durable commit.
Only IDs cross worker argv; immutable prompt/context/history use authenticated Start.

Model completion commits history before acknowledgment and leaves the managed
worker alive. Separate exact-run Retire, Dashboard Stop and authenticated timeout
Stop release all retained/Session/mutation guards before the ordinary owned-group
termination chain. Stop first durably interrupts the turn, then revokes its lease.
Only controlled stop or the existing explicit acknowledgment sets retained stopped
proof; natural exit remains uncertain. `worker_stopped` derives directly from that
proof, so Complete with false means finishing or stop-unconfirmed. Protocol 70
coalesces per-conversation `OperatorChanged` notifications on the sole writer;
paged durable history supplies catch-up. Retirement, Stop and acknowledgment
publish the proof change as well as batch updates. Restart never launches or
replays interrupted turns.

Persist the native OS boot identity with every run. Only validated, different
boot identities establish prior-process exit without explicit acknowledgement.
Missing, malformed or same-boot evidence fails closed. Never signal remembered
PIDs or infer descendant exit from the leader or server disappearing.

Natural exit retains ownership uncertainty: process-group or TTY disappearance
is not proof about background jobs. Keep `stopped` as the durable authority;
do not add process-table polling or a parallel cached proof. An already-exited
termination result is not a controlled stop. Only a verified stop of the current
owned run may certify that control outcome; failed discovery, races and partial
termination retain uncertainty. Explicit acknowledgement is fenced to its
captured session and run.

The Server records its own decisions as Events (ADR 0009). Each event is a
component (`titles`, `settings`, or `quota`), a time, the session or provider
concerned, and one line that follows the Quota reason rule: no prompt, transcript,
credential, account identifier, or native body. The in-memory ring holds 2000
events and drops the oldest. Every new event is also appended to `events.jsonl`
in the instance directory, mode 0600, one JSON object per line. At 5 MB the
file rotates once, to `events.jsonl.1`; the rotation renames the whole file, so
a line is never split, and older lines are not rewritten. stderr is unchanged.
`ovrcr events` reads the ring through a Server request. The Dashboard popup that
shows the same ring is a separate change and is not implemented here.

Dashboard create/remove/close (CreateWorkspace, CreateWorkspaceWithLaunch,
CreateSession, CloseTerminal) are accepted immediately on the Dashboard
connection and run as one Server Lifecycle job on a dedicated worker under
`mutation_lock` (ADR 0010). Input and SetView keep flowing on the reader while
the job runs. Assigned runs receive an immutable input-ownership marker before
Session publication; generic input checks that marker without taking the
lifecycle mutation lock. Explicit assigned-human input has a separate gate:
retained authority, child state, writer, active Dashboard slot, acknowledged view.
It validates the captured child epoch/run/view and current Human owner, then
persists a content-free human-input revision before bytes. It never takes the
lifecycle mutation lock. Takeover/return use the same durable per-child control
record; neither changes immutable assigned membership. Scoped delegation input
checks its captured control epoch and current parent/child bindings under their
Session state locks through the PTY write. Parent effects additionally hold the
retained workflow owner through the final publication/spawn/write fence, validate
the captured controller epoch, and authenticate the current in-memory source key.
Source registration and Reporter disconnect retirement follow retained → Session
lock order. Explicit parent transfers use the same retained row and never reset
child control or immutable assignment identity. A second Lifecycle job is refused. Completion is a typed
`LifecycleCompleted` event correlated by the accepting request id, plus
`HierarchyChanged` as today. CLI-only RemoveSession and DeleteArchived stay
synchronous on the caller.

Workflow cancellation lives in that same durable WorkflowRecord. Commit the
bounded complete capture, fixed member identities/gaps and child interrupt
intents atomically before effects. A tombstone gates new and resumed Accepted
dispatch after discovery and survives parent transfer. Native interrupts use
the exact current child writer with an Unknown receipt and advanced child
barrier already durable. Confirm only that barrier with fresh same-turn Pi
agent_settled evidence under current parent/child locks; preserve historical
delivery uncertainty and unrelated input or attribution fences. Wait aborts
terminate only their observer helper. No operation archives or removes resources.


Delegated archive commits an exact-run Archive barrier and unknown stop receipt
before capability revocation, then releases mutation/retained/Session state for
the existing owned termination chain. Its termination mutex is acquired with
try-lock before ordered Session state and released before completion relocks.
Stop proof plus pending receipt commit together; archive/run advancement plus
final receipt and exact barrier retirement commit together. Retained caches
publish only after those commits. Final filing pins the current parent source
through commit, including continuations. An explicit continuation consumes its
protected slot/bytes only on successful filing and never performs another stop.
Same-resource lifecycle changes respect the barrier; unrelated input/view remain
available during the stop. Archive preserves private evidence and workspaces.

Coding delegation uses the same already-locked combined workspace/managed-launch
body. Record proposed repository/path/pinned base before Git; save created Git
generation before registration and registered resource before native launch.
Revalidate the registered repository root's existing Git generation at coding
intent, effect and current-capture gates, with Git outside retained/Session guards.
A NoProcessStarted proof is independent of worktree existence. Final rejected
launch receipts stay immutable; only one explicit linked successor may reuse a
verified no-process/no-input workspace. Unknown ownership never permits adoption.
Candidate capture runs without retained/Session locks. Its in-memory bytes gain
a discoverable filename only after the existing child-authenticated publication
intent binds the exact enriched result digest. Recovery uses original stored
bytes and selected version/contract; it never captures a newer checkout.


Dependency edges stay in the existing WorkflowRecord. The parent declares the
bounded graph; ordinary Prepare/Launch/SendInitial retain scheduling and effects.
Prepare authenticates selected original candidate/acceptance/reviewer files and
pins a private complete input baseline. Lifecycle workspace registration precedes
compatible snapshot materialization, and the existing creation gate rechecks
parent/source authority before spawn. Initial input rechecks actual baseline
content and ancestor candidate/review versions. Followup and acceptance reject
stale dependency witnesses; collection preserves the original evidence and
reports the gap. Graph revision changes do not interrupt existing children.

Cursor cloud previews require a positive managed reservation and invocation
channel. A descendant admission probe is authenticated against that same
supervised native tree; direct adapter invocation fails closed. The shared
Reporter owns lease teardown. Each Session owns a unique private cloud scratch
root, removed by its waiter after PTY drain and process-group exit, so killing
the adapter cannot leave its bearer file behind or erase a replacement run's
root. The SDK Bridge receives an environment allowlist without OVRCR credentials.
Git, Keychain, archive extraction, discovery and catalog I/O run in the adapter
under a thirty-second preflight deadline, outside Server input/lifecycle locks.

Preflight Git, Keychain and extraction helpers inherit the adapter's supervised
process group. A helper timeout reaps the direct child and fails preflight,
exiting the adapter; the surviving invocation supervisor then kills the whole
group, including helper descendants. Helpers must not create separate groups
whose only cleanup owner would disappear with the adapter.

Cursor cloud creation and Send share one Server/SessionStore operation record. Authenticated Reporter
commands require the current CursorCloud reservation; intent/private publication,
dispatch claim and exact handle receipt are separate durable boundaries. Claim
and local closure serialize under the retained mutex; the adapter releases those
locks before bridge I/O. Local Discard is a closure, never a remote-cancellation
receipt. Archive preserves unresolved bytes; local row deletion requires explicit
Discard, including for acknowledged handles. The original Session owns the operation across restart; no automatic creation or uncertain Retry is permitted. Exact-run read-only
reconciliation uses the shared reopen path; active observation stays unavailable. Send requires the
original durable creation receipt, immutable body/options and a distinct key/claim;
exact run retention precedes payload retirement. Busy input is rejected for manual
resubmission. Exact run observations use the existing Reporter/revisions; progress
carries bounded opaque per-stream tokens and locally ordered revisions, never a
provider-token ordering. Legacy numeric progress remains historical-only, and a
Send token never resumes ObserveRun. Stream loss remains unknown and no newest-run
lookup supplies identity. The
accepted pending-submission exception permits immutable request bytes only in
confined owner-only plaintext files, readable by same-user programs and backups;
credentials stay outside them. Creation payload retirement follows durable exact
identity/receipt; Send payload retirement requires its exact durable run ID.

Cloud Cancel uses the same serialized operation row. Intent and delivery closure
commit together; a separate exact-run live cancellation claim commits before RPC.
Original delayed run receipts remain admissible after cancellation, then may
complete that live intent. Cancellation acknowledgement never proves terminal
state; only independent credentialed exact GetRun supplies the outcome. Discard
retires canonical private files and owned aliases after durable closure, with
startup cleanup retry. Restart never issues CancelRun automatically.

The original live adapter stdin reader checks typed exact-target cancellation
intent at most once per second, outside Server locks. CheckCancel is read-only;
one successful check admits one ClaimCancel attempt even if persistence/RPC fails.
Observation failure retains that exact target until authoritative terminal receipt
or attachment exit. Recovery never starts this original-attachment reader. Private
temporary alias sweeps run only during startup/explicit Discard, not each check.

Cloud recovery adds no process/store owner: the shared retained reopen path starts
its managed invocation and unique bridge scratch tree. Original operation identity
is immutable; schema 27 protects subordinate recovery-owner, cancellation-claim,
opaque observation-cursor and local-revision metadata from older writers. Read-only historical reconciliation bypasses response
cycles. Exact fresh GetRun registers a selected-key handle only; active observation
remains unproved and no ResumeAgent/options replay is allowed.

Executable equality uses every byte in fixed 64 KiB blocks with aHash 0.8.12,
explicit zero seeds and an algorithm/version descriptor. The descriptor includes
architecture, endianness and AES/SSSE3 compilation flags; flags conservatively
distinguish builds even when the chosen backend ignores them. It is not an
authenticity proof. Untagged private legacy records remain recognized for stale
replacement but cannot equal the new algorithm; unknown algorithm names or fields
remain refused. Full verification and metadata rechecks precede socket binding
and identity publication, including `/proc/self/exe` on Linux.

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

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:3140-3360"}
    pub(super) fn palette_response(
…
    ) -> Option<Vec<ClientMessage>> {
…
        match response {
…
            Response::Error { code, message } => {
…
                let forced = match &palette.page {
                    Page::Confirm {
                        request:
                            Some(Request::RemoveWorkspace {
…
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
…
    }

[Showing lines 3139-3363 of 6184. Use :3364 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:2450-2560"}
                        let selected_launch = if matches!(
…
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

[Showing lines 2449-2563 of 6184. Use :2564 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard", "pattern": "LifecycleCompleted|lifecycle_preference|detached_palette_requests|client_token"}
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
 256|            .and_then(|palette| palette.launch_preference.take());
*257|        self.lifecycle_preference = preference;
 258|        self.lifecycle_strip = Some(row.kind.strip_message());
 259|        self.provisional = Some(row);
 260|        self.palette = None;
...
 290|        self.lifecycle_strip = None;
*291|        self.lifecycle_preference = None;
 292|    }
 293|
 294|    pub(super) fn on_lifecycle_completed(
 295|        &mut self,
*296|        client_token: u64,
 297|        op: LifecycleOp,
 298|        outcome: LifecycleOutcome,
 299|    ) -> DashboardAction {
...
 305|                if let Some(row) = &self.provisional
*306|                    && row.token != client_token
 307|                {
 308|                    return DashboardAction::None;
 309|                }
...
 317|                };
*318|                if row.token != client_token {
 319|                    self.provisional = Some(row);
 320|                    return DashboardAction::None;
 321|                }
...
 343|                };
*344|                if row.token != client_token {
 345|                    self.provisional = Some(row);
 346|                    return DashboardAction::None;
 347|                }
...
 357|                }
*358|                self.lifecycle_preference = None;
 359|                row.failed = Some((code.clone(), message.clone()));
 360|                self.lifecycle_strip = Some(format!("{code:?}: {message}"));
 361|                self.provisional = Some(row);
...
 373|        }
*374|        if let Some((project, choice)) = self.lifecycle_preference.take() {
 375|            let request = super::settings::launch_choice_request(&project, &choice);
 376|            self.settings.launch_choices.insert(project, choice);
 377|            let message = crate::protocol::ClientMessage {
...
 447|        match event {
*448|            ServerEvent::LifecycleCompleted {
*449|                client_token,
 450|                op,
 451|                outcome,
*452|            } => Some(self.on_lifecycle_completed(*client_token, op.clone(), outcome.clone())),
 453|            _ => None,
 454|        }
 455|    }
## mod.rs
 336|    /// Remembered launch choice held across Provisional wait until CreatedSession completes.
*337|    lifecycle_preference: Option<(String, settings::LaunchChoice)>,
 338|}
 339|
 340|thread_local! {
## navigation.rs
 19|    pub(super) input_epoch: u64,
*20|    pub(super) detached_palette_requests: HashSet<u64>,
 21|}
 22|
 23|impl Dashboard {
## palette.rs
 163|            if let Some(request_id) = palette.pending {
*164|                self.navigation.detached_palette_requests.insert(request_id);
 165|            }
 166|            if let Some(request_id) = palette.suggestions.inspect {
 167|                self.ignored_responses.insert(request_id);
...
 3009|            .navigation
*3010|            .detached_palette_requests
 3011|            .remove(&request_id)
 3012|        {
 3013|            if let Response::Error { message, .. } = response {
## state.rs
 614|            lifecycle_strip: None,
*615|            lifecycle_preference: None,
 616|            settings: Settings::default(),
 617|            theme: crate::theme::ThemeTokens::dark(),
 618|            config_dir: std::path::PathBuf::new(),
...
 3570|                }
*3571|                ServerEvent::LifecycleCompleted {
*3572|                    client_token,
 3573|                    op,
 3574|                    outcome,
 3575|                } => {
*3576|                    self.on_lifecycle_completed(client_token, op, outcome);
 3577|                }
 3578|                ServerEvent::Output {
 3579|                    session,

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src", "pattern": "reconnect|Reconnect|disconnect|Disconnected"}
# crates/ovrcr-tui/src/dashboard/
## desktop.rs
 2|//! identity, and an Input request keyed by request identity. Both reuse the same
*3|//! preferences, host and reconnect baseline. Selection and pane visibility
 4|//! do not suppress or cancel otherwise valid alerts.
 5|use super::ready::{delivery_live, input_live};
 6|use super::settings::{AutomaticLocalTerminals, Settings};
...
 1720|        );
*1721|        assert!(d.desktop.pending.is_empty(), "reconnect does not replay");
 1722|        deliver(
 1723|            &mut d,
 1724|            snapshot_with_requests(
...
 1816|    #[test]
*1817|    fn desktop_a_reconnect_baseline_with_several_open_requests_never_replays() {
 1818|        let mut d = Dashboard::new(TerminalSize {
 1819|            rows: 24,
 1820|            cols: 120,
...
 1843|        );
*1844|        assert!(d.desktop.pending.is_empty(), "reconnect does not replay");
 1845|        deliver(
 1846|            &mut d,
 1847|            snapshot_with_requests(
## event_loop.rs
 362|        Err(mpsc::TryRecvError::Empty) => Ok(None),
*363|        Err(mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!("dashboard connection lost")),
 364|    }
 365|}
 366|
## mod.rs
 282|    agent_typing: Option<state::AgentTyping>,
*283|    /// One display request per retained run; the server persists launch failures across reconnects.
 284|    recovery_requests:
 285|        std::collections::HashMap<(SessionId, crate::protocol::SessionRunId), Option<u64>>,
 286|    outer_area: Rect,
## state.rs
 3359|                Response::Hierarchy(hierarchy) => {
*3360|                    // Reconnect drops client Provisional fiction; Server hierarchy is truth.
 3361|                    self.drop_provisional_fiction();
 3362|                    for request in self.update_hierarchy(hierarchy) {
 3363|                        self.push_request(request);
## status.rs
 787|        );
*788|        snapshot.health.reason = Some("supervisor_disconnected".into());
 789|        let clause = activity_clause(&session(SessionPhase::Running, Some(snapshot)));
 790|        assert!(
*791|            clause.contains("unavailable · supervisor_disconnected"),
 792|            "{clause}"
 793|        );
 794|        assert!(!clause.contains("Ready"), "{clause}");
## tests.rs
 180|#[test]
*181|fn dashboard_surfaces_hello_refusal_and_reader_disconnect() {
 182|    let refusal = ServerMessage::Response {
 183|        request_id: 1,
 184|        response: Response::Error {

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs:3336-3470"}
    pub(super) fn attach_created_workspace(&mut self) -> Vec<ClientMessage> {
…
        if !matches!(
…
            Page::Form {
…
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
…
                    } else if let Command::Unarchive(id, _)
…
                    }
                }
            }
…
        }
…
    }

[Showing lines 3335-3473 of 6184. Use :3474 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs:160-390"}
    pub(super) fn attach_lifecycle_workspace(
…
    ) -> DashboardAction {
…
        if workspace.sessions.is_empty() {
…
            return match self.view_request(self.outer_area, request_id) {
…
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
                    self.lifecycle_strip = Some(format!("{code:?}: {message}"));
                    self.open_forced_remove_workspace(project, id, code, message);
                    return DashboardAction::Redraw;
                }
                self.lifecycle_preference = None;
                row.failed = Some((code.clone(), message.clone()));
                self.lifecycle_strip = Some(format!("{code:?}: {message}"));
                self.provisional = Some(row);
                DashboardAction::Redraw
            }
        }
    }

    fn select_created_session(&mut self, session: SessionSummary) -> DashboardAction {
        self.select_session(session.id);
        self.mode = super::InputMode::Terminal;
        let request_id = self.next_request_id();
        if let Ok(Some(message)) = self.view_request(self.outer_area, request_id) {
            self.enqueue_request(message);
        }
        if let Some((project, choice)) = self.lifecycle_preference.take() {
            let request = super::settings::launch_choice_request(&project, &choice);
            self.settings.launch_choices.insert(project, choice);
            let message = crate::protocol::ClientMessage {
                request_id: self.error_owning_request_id(),
                request,
            };
            self.enqueue_request(message);
        }
        DashboardAction::Redraw
    }

    pub(super) fn provisional_tree_rows(&self) -> Vec<(TreeRow, String)> {
        let Some(row) = &self.provisional else {
            return Vec::new();
        };
        let label = row.row_label();
        match &row.kind {
            ProvisionalKind::CreatingWorkspace { project, id, .. } => {
                // If the Server already published the workspace (partial), overlay label only.
…
            }
…
        }
    }

[Showing lines 159-393 of 604. Use :394 to continue]