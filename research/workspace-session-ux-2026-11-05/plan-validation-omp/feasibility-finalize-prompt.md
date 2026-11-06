Finalize this independent read-only plan review now, using only the supplied plan, design, and observed source excerpts. Your earlier source-inspection phase hit its time limit without a final report. No tools are available in this final phase. Do not ask questions or propose more browsing. Separate substantiated findings from proof gaps; no evidence means no finding. Return at most 5 actionable issues and 1200 words total, with exact path:line citations where present. Each issue: priority, concrete scenario, evidence, and smallest plan correction. Finish PASS / PASS WITH CLARIFICATIONS / NEEDS REVISION for plan quality, never runtime acceptance. Do not reopen submitted user decisions.

You are an independent Oh My Pi plan reviewer using the user-requested DeepSeek V4.1 Flash model. Review only; do not implement or edit any files. Tools are limited to read/grep/glob. Stay inside /Users/xlyk/Code/ovrcr. Never read credentials, .secrets, personal homes, unrelated reports or live app state. No provider/tool execution, network access, nested agents or user questions.

Task: validate docs/superpowers/plans/2026-11-05-agent-session-creation.md against docs/superpowers/specs/2026-11-05-agent-session-creation-design.md and actual owning source. Read AGENTS.md, CONTEXT.md and the relevant narrow development guides. This is an approved design but an unimplemented plan. Do not report desired new behavior as a current-code bug; identify missing/wrong implementation steps or invalid assumptions. Do not reopen explicit user decisions: registered workspaces only; child creation commits; branch drift follows same checkout; pending Create stays open; prompt memory-only per server lifetime; full model/prompt support required for Claude/Codex/Pi/omp; other launchers gated; n agent / separate terminal / w workspace.

Read the connected code needed for each finding. Return only substantiated, actionable plan issues. Cite exact repository-relative path:line for both plan/spec and source evidence. Distinguish blockers from nonblocking clarifications. For each finding give priority, concrete failing scenario, why current plan misses it, and smallest plan correction. Avoid speculative hardening or demands to add new frameworks. If no issue, say no actionable findings and list substantive checks and remaining proof gaps. Finish with PASS / PASS WITH CLARIFICATIONS / NEEDS REVISION, understood as plan quality, never runtime acceptance.

Review axis: source feasibility and dependency order. Trace protocol launch types/callers, managed harness argv and authentication/reporting boundaries, existing model discovery and exact OMP distinctness, settings ownership/launch memory, root/workspace identity. Is the plan concrete enough to implement without inventing unsafe model/prompt semantics? Prioritize invalid reuse assumptions, source omissions and contradictory native launch requirements. Use source inspection, not actual provider invocations.


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

SOURCE LOOKUP {"path": "crates/*/src/protocol/wire.rs;crates/*/src/dashboard/*.rs;crates/*/src/server/*.rs"}
# crates/
## ovrcr-tui/src/dashboard/
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
## ovrcr-runtime/src/server/
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

SOURCE LOOKUP {"path": "**/delegation.rs;**/*_delegation.rs;**/settings*.rs;CONTEXT.md"}
CONTEXT.md
# tests/
pi_delegation.rs
settings_lifecycle.rs
## tui/
settings.rs
# crates/
## ovrcr-tui/
### tests/
settings_theme_preview.rs
### src/dashboard/
settings_editor.rs
settings.rs
## ovrcr-runtime/
### tests/
settings_matrix.rs
### src/
claude_delegation.rs
codex_delegation.rs
#### server/
delegation.rs
## ovrcr-protocol/src/
settings.rs
delegation.rs
# src/cli/
delegation.rs
claude_delegation.rs
codex_delegation.rs

SOURCE LOOKUP {"pattern": "CreateSessionRequest|SessionLaunch|struct AgentEntry|enum.*Launch|launch_kind|model|prompt", "path": "crates/ovrcr-protocol/src/wire.rs"}
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
 289|        branch: BranchRequest,
*290|        launch: Option<SessionLaunch>,
 291|    },
 292|    SetSessionTitle {
 293|        session: SessionId,
...
 370|    },
*371|    /// Answer one shutdown prompt. `workspace` is the workspace id.
 372|    /// `save` false continues shutdown without pushing that worktree.
 373|    AnswerWipSave {
 374|        project: String,
...
 586|        let mut settings = crate::Settings {
*587|            title_model: Some("pi/m".into()),
 588|            picker_roots: vec!["/c".into()],
 589|            cursor_cloud_accounts: vec![crate::CursorCloudAccount {
 590|                name: "work".into(),
...
 592|                keychain_account: "work".into(),
*593|                model: "composer-2.5".into(),
 594|                repositories: vec!["https://github.com/acme/app".into()],
 595|                owners: vec![],
 596|                bridge_archive: "/sdk.tar.gz".into(),
...
 698|                    source: ContextSource::Generic,
*699|                    model: None,
 700|                    conversation: None,
 701|                    used_tokens: Some(4),
 702|                    capacity_tokens: Some(5),
...
 778|                "CreateSession",
*779|                Request::CreateSession(CreateSessionRequest {
 780|                    project: "a".into(),
 781|                    workspace: "b".into(),
 782|                    name: "c".into(),
...
 911|                    branch: BranchRequest::Existing { branch: "c".into() },
*912|                    launch: Some(SessionLaunch {
 913|                        argv: vec!["sh".into()],
 914|                        label: None,
 915|                        kind: SessionKind::Terminal,
...
 1569|            provider: "pi".into(),
*1570|            model_provider: "test".into(),
*1571|            model: "m".into(),
 1572|            acceptance_evidence: vec!["cite".into()],
 1573|            mandatory_capabilities: vec!["read".into()],
 1574|            limits: crate::DelegationLimits {
...
 1787|                        provider: Some(crate::DelegationHarness::Pi),
*1788|                        model_provider: Some("test".into()),
*1789|                        model: Some("m".into()),
 1790|                        required_limits: crate::DelegationLimits::default(),
 1791|                    },
 1792|                },
...
 1800|                        provider: crate::DelegationHarness::Pi,
*1801|                        model_provider: "test".into(),
*1802|                        model: "m".into(),
 1803|                    }],
 1804|                    resolved_choice: None,
 1805|                    effective_limits: crate::DelegationLimits {
...
 2124|            provider: OperatorProvider::ChatGpt,
*2125|            model: "model".into(),
 2126|            title: "hello".into(),
 2127|            current_turn: Some(turn.clone()),
 2128|        };
...
 2146|                    provider: OperatorProvider::Grok,
*2147|                    model: "model".into(),
 2148|                },
 2149|            ),
 2150|            (
...
 2283|                    provider: OperatorProvider::ChatGpt,
*2284|                    model: "model".into(),
 2285|                    text: "hello".into(),
 2286|                    context,
 2287|                    history: Some(history),

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

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/agents.rs:20-140"}
const KNOWN_AGENTS: [&str; 12] = [
…
];
/// Detected agents OVRCR launches through `agent run <name> --` so managed reporting
/// (Grok retains history for titles; Hermes has process supervision only). Claude, Codex,
/// Pi, and Oh My Pi defaults also carry that provider's auto-trust flag. Explicit custom
/// overrides keep their stored argv unchanged; a raw shell command is never adopted silently.
pub const MANAGED_AGENTS: [&str; 7] = [
    "claude",
    "codex",
    "pi",
    "omp",
    "grok",
    "hermes",
    "cursor-agent",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSource {
    Detected,
    Shell,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEntry {
    pub name: String,
    pub argv: Vec<OsString>,
    pub source: AgentSource,
}

pub fn detect_agents(
    path: &OsStr,
    shell: Option<&OsStr>,
    launcher: Option<&Path>,
) -> Vec<AgentEntry> {
    let mut entries = Vec::new();
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    for name in KNOWN_AGENTS {
        if let Some(found) = dirs.iter().find_map(|dir| executable_in(dir, name)) {
            let argv = match launcher {
                Some(launcher) if MANAGED_AGENTS.contains(&name) => {
                    let mut native = vec![OsString::from(found)];
                    // Native argv only: the wrapper's `--` is not a prompt separator.
                    ovrcr_protocol::apply_default_auto_trust(name, &mut native);
                    let mut argv = vec![
                        launcher.as_os_str().to_owned(),
                        "agent".into(),
                        "run".into(),
                        name.into(),
                        "--".into(),
                    ];
                    argv.extend(native);
                    argv
                }
                _ => vec![found.into()],
            };
            entries.push(AgentEntry {
                name: name.into(),
                argv,
                source: AgentSource::Detected,
            });
        }
    }
    let shell = shell
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("/bin/sh"));
    entries.push(AgentEntry {
        name: "shell".into(),
        argv: vec![shell],
        source: AgentSource::Shell,
    });
    entries.push(AgentEntry {
        name: "Custom".into(),
        argv: Vec::new(),
        source: AgentSource::Custom,
    });
    entries
}

pub fn apply_overrides(
    mut entries: Vec<AgentEntry>,
    overrides: &[AgentOverride],
) -> Vec<AgentEntry> {
    let mut custom = entries.pop();
    let mut shell = entries.pop();
    for override_agent in overrides {
        let argv = override_agent
            .argv
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        if let Some(existing) = entries
            .iter_mut()
            .find(|entry| entry.name == override_agent.name)
        {
            existing.argv = argv;
        } else if shell
            .as_ref()
            .is_some_and(|entry| entry.name == override_agent.name)
        {
            if let Some(entry) = shell.as_mut() {
                entry.argv = argv;
            }
        } else if custom
            .as_ref()
            .is_some_and(|entry| entry.name == override_agent.name)
        {
            if let Some(entry) = custom.as_mut() {
                entry.argv = argv;
            }
        } else {
            entries.push(AgentEntry {
                name: override_agent.name.clone(),
                argv,
                source: AgentSource::Detected,
            });
        }
    }
    if let Some(shell) = shell {
        entries.push(shell);
    }
    if let Some(custom) = custom {
        entries.push(custom);
    }
    entries
}

[Showing lines 19-143 of 429. Use :144 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs:160-200"}

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
…
}

[Showing lines 159-203 of 4049. Use :204 to continue]

SOURCE LOOKUP {"pattern": "apply_default_auto_trust|CreateSession|spawn_session|agent run|--", "path": "crates/ovrcr-runtime/src/server/mod.rs;crates/ovrcr-protocol/src/lib.rs;crates/ovrcr-protocol/src/agents.rs"}
# crates/

## ovrcr-runtime/src/server/
### mod.rs
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
 1406|                    ErrorCode::OwnershipUncertain,
*1407|                    "confirm that the previous agent and background processes stopped; use --ack-stopped",
 1408|                ));
 1409|            }
 1410|            self.acknowledge_stopped_locked(id, expected_run)?;
...
 1447|                        "cursor-cloud".into(),
*1448|                        "--".into(),
 1449|                        target.intent.account.name.clone().into(),
 1450|                        serde_json::to_string(&target)?.into(),
 1451|                    ])
...
 2313|        self.create_session_locked(
*2314|            ovrcr_protocol::CreateSessionRequest {
 2315|                project,
 2316|                workspace: name.clone(),
 2317|                name: session_name,
...
 2488|                hints.push(format!(
*2489|                    "unacknowledged stopped: {}; run `ovrcr terminal acknowledge-stopped <id>` or `ovrcr terminal remove <id>`, or remove with --force",
 2490|                    uncertain.iter().map(describe).collect::<Vec<_>>().join(", ")
 2491|                ));
 2492|            }
...
 2764|        self.create_session_locked(
*2765|            ovrcr_protocol::CreateSessionRequest {
 2766|                project: project.to_owned(),
 2767|                workspace: workspace_id.to_owned(),
 2768|                name: "local".into(),
...
 2829|                .unwrap_or_else(|| {
*2830|                    "confirm that the previous agent and background processes stopped; use --ack-stopped"
 2831|                        .to_owned()
 2832|                });
 2833|            return Err(lifecycle_error_with_hierarchy(
...
 2969|                None,
*2970|                &argv(&["/usr/local/bin/ovrcr", "agent", "run", "pi", "--", "pi"]),
 2971|            ),
 2972|            "pi"
 2973|        );
...
 2987|                    "run",
*2988|                    "--provider",
 2989|                    "claude",
*2990|                    "--",
 2991|                    "claude"
 2992|                ]),
 2993|            ),
...
 3011|                Some("custom-harness".into()),
*3012|                &argv(&["ovrcr", "agent", "run", "pi", "--", "pi"]),
 3013|            ),
 3014|            "custom-harness"
 3015|        );

## ovrcr-protocol/src/
### lib.rs
 24|pub use agent::*;
*25|pub use auto_trust::{apply_default_auto_trust, default_auto_trust_flag};
 26|pub use bridge::{
 27|    BRIDGE_SCHEMA_VERSION, BridgeActivationTarget, BridgeContext, BridgeNavigationOffer,
 28|    BridgeNavigationResult, BridgeNavigationTicket, BridgeOperation, BridgeReply, BridgeRequest,

Skipped missing paths: crates/ovrcr-protocol/src/agents.rs

SOURCE LOOKUP {"pattern": "omp|discover_models|models|auto_trust|initial_prompt|prompt", "path": "crates/ovrcr-runtime/src/server/delegation.rs;crates/ovrcr-runtime/src/claude_delegation.rs;crates/ovrcr-runtime/src/codex_delegation.rs;crates/ovrcr-protocol/src/delegation.rs"}
# crates/

## ovrcr-runtime/src/
### claude_delegation.rs
 60|    policy: String,
*61|    // This identifies only model/backend-affecting policy, not complete launch
 62|    // topology. Discovery excludes the assignment publisher and Reporter transport.
 63|    policy_digest: String,
 64|}
...
 120|/// Only model/backend-affecting flags are shared with discovery; transport
*121|/// differences are explicit and never claim identical complete MCP topology.
 122|pub fn launch_argv(executable: &Path, model: &str, artifacts: &Path) -> Vec<std::ffi::OsString> {
 123|    vec![
 124|        executable.as_os_str().into(),
...
 200|
*201|fn models(value: &serde_json::Value) -> Result<Vec<(String, String)>> {
 202|    let list = value
 203|        .as_array()
 204|        .filter(|list| list.len() <= 512)
...
 402|                }
*403|                return Ok((models(&native["models"])?, native_version.to_owned()));
 404|            }
 405|            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
 406|            let status = unsafe {
...
 530|        assert_eq!(
*531|            models(&serde_json::json!([{"value":"alias","resolvedModel":"native-resolved"}]))
 532|                .unwrap(),
 533|            [("alias".into(), "native-resolved".into())]
 534|        );
 535|        assert!(
*536|            models(&serde_json::json!([{"value":"alias"}]))
 537|                .unwrap()
 538|                .is_empty()
 539|        );
*540|        assert!(models(&serde_json::json!([{"value":"alias","resolvedModel":"one"},{"value":"alias","resolvedModel":"two"}])).is_err());
 541|        assert!(
*542|            models(&serde_json::json!([{"value":"alias","resolvedModel":"bad model"}])).is_err()
 543|        );
 544|        assert!(!version("2.1.299"));
 545|        assert!(version("2.1.300"));
### codex_delegation.rs
 6|    os::unix::{fs::PermissionsExt, process::CommandExt},
*7|    path::{Component, Path, PathBuf},
 8|    process::{Command, Stdio},
 9|    time::{Duration, Instant},
 10|};
...
 31|
*32|pub fn models() -> Result<Vec<DelegationChoice>> {
 33|    // Bundled discovery never refreshes, authenticates or starts a conversation.
 34|    let executable = executable()?;
 35|    let private = tempfile::Builder::new().prefix("ocdx-").tempdir()?;
...
 47|        &executable,
*48|        &["debug", "models", "--bundled"],
 49|        &home,
 50|        deadline,
 51|    )?;
*52|    parse_models(&bytes)
 53|}
 54|
 55|fn inspect(executable: &Path, args: &[&str], home: &Path, deadline: Instant) -> Result<Vec<u8>> {
...
 112|
*113|fn parse_models(bytes: &[u8]) -> Result<Vec<DelegationChoice>> {
 114|    ensure!(
 115|        bytes.len() as u64 <= MAX_CATALOG,
 116|        "Codex catalog exceeds byte bound"
...
 118|    let value: serde_json::Value = serde_json::from_slice(bytes)?;
*119|    let models = value["models"]
 120|        .as_array()
*121|        .context("Codex catalog missing models")?;
*122|    ensure!(models.len() <= 128, "Codex catalog exceeds model bound");
 123|    let mut choices = Vec::new();
*124|    for model in models {
 125|        let slug = model["slug"]
 126|            .as_str()
 127|            .context("Codex model lacks exact slug")?;
...
 162|                && path
*163|                    .components()
*164|                    .all(|c| !matches!(c, Component::ParentDir | Component::CurDir)),
 165|            "Codex policy requires canonical absolute paths"
 166|        );
 167|    }
...
 199|        "analytics":{"enabled":false},
*200|        "features":{"apps":false,"plugins":false,"browser_use":false,"computer_use":false,"in_app_browser":false,"image_generation":false,"goals":false,"memories":false,"shell_snapshot":false,"workspace_dependencies":false,"tool_suggest":false,"request_permissions_tool":false},
 201|        "projects":{workspace.to_string_lossy().as_ref():{"trust_level":"untrusted"}},
 202|        "shell_environment_policy":{"inherit":"none","ignore_default_excludes":false,"set":{"PATH":"/usr/bin:/bin","HOME":workspace.to_string_lossy()}},
 203|        "permissions":{"ovrcr-research":{"filesystem":fs,"network":{"enabled":false}}},
...
 213|    fn catalog_keeps_exact_listed_choices_and_refuses_malformed_or_oversized_data() {
*214|        let choices = parse_models(br#"{"models":[{"slug":"exact-model","visibility":"list"},{"slug":"hidden","visibility":"hide"}]}"#).unwrap();
 215|        assert_eq!(choices.len(), 1);
 216|        assert_eq!(choices[0].model, "exact-model");
 217|        assert_eq!(choices[0].model_provider, "openai");
...
 219|            b"{}".as_slice(),
*220|            br#"{"models":[{"visibility":"list"}]}"#,
*221|            br#"{"models":[{"slug":"","visibility":"list"}]}"#,
 222|        ] {
*223|            assert!(parse_models(bytes).is_err());
 224|        }
*225|        assert!(parse_models(&vec![b' '; MAX_CATALOG as usize + 1]).is_err());
 226|    }
 227|    #[test]
 228|    fn profile_is_read_only_private_and_exposes_only_scoped_publication() {

### server/
#### delegation.rs
 809|        let discovered = match harness {
*810|            DelegationHarness::Pi => available_models(),
 811|            DelegationHarness::Codex
 812|                if selection
 813|                    .model_provider
...
 816|            {
*817|                crate::codex_delegation::models()
 818|            }
 819|            DelegationHarness::Codex => bail!(
 820|                "Codex delegation supports only the native OpenAI backend; no substitute selected"
...
 834|                    )
*835|                    .map(|(models, _)| {
*836|                        models
 837|                            .into_iter()
 838|                            .map(|(model, _)| DelegationChoice {
 839|                                provider: DelegationHarness::Claude,
...
 927|        // Same serialization as session admission: validation, receipts and effects cannot
*928|        // race another lifecycle mutation or a competing assignment input.
 929|        #[cfg(feature = "acceptance-diagnostics")]
 930|        if let DelegationRequest::PublishQuestion { handle, .. } = &request
 931|            && take_fault("question-mutation-blocked", handle).is_some()
...
 1159|                        let old_contract = read_contract(&previous)?;
*1160|                        let mut comparable = contract.clone();
*1161|                        comparable.request = old_contract.request.clone();
*1162|                        comparable.previous_request = old_contract.previous_request.clone();
*1163|                        if comparable != old_contract
 1164|                            || previous.spent_execution_attempts().is_none()
 1165|                        {
 1166|                            return Err(lifecycle_error(
...
 1650|                                let reason = format!(
*1651|                                    "launch did not complete; retained resources require reconciliation: {error:#}"
 1652|                                );
 1653|                                if no_process || record.identity.session.is_none() {
 1654|                                    record.dispatch = DispatchStatus::Refused;
...
 1841|                    store.save_assignment(&record)?;
*1842|                    let text = assignment_prompt(
 1843|                        &contract,
 1844|                        &record.root.join("brief-v1.json"),
 1845|                        &record,
...
 1874|                        Ok(crate::session::DelegatedDelivery::Partial(input)) => record.outcome(DispatchOperation::InitialInput, DispatchOutcome::Partial, Some(input), Some("only a prefix reached the PTY; do not replay")),
*1875|                        Ok(crate::session::DelegatedDelivery::Unknown(input)) => record.outcome(DispatchOperation::InitialInput, DispatchOutcome::Unknown, Some(input), Some("input bytes reached the PTY but completion is uncertain; do not replay")),
 1876|                        Err(_) => {
 1877|                            record.reason =
 1878|                                Some("initial input effect is unknown; do not replay".into())
...
 2379|        let publication = if contract.coding.is_some() {
*2380|            "Call ovrcr_publish with actual named check outcomes (Passed/Failed/Missing/Unrun). The Server captures a complete immutable authorized delta against coding_workspace.base. Do not commit, push, merge, deploy, access credentials or modify task/parent records. Tool selection is not an OS sandbox. Only the parent can accept the result."
 2381|        } else {
 2382|            "Call ovrcr_publish. Only this child can publish; it cannot accept the result. Research tools are read-only."
 2383|        };
...
 2615|/// absent defaults. A different contract never acquires the old digest.
*2616|fn assignment_prompt(
 2617|    contract: &ResearchContract,
 2618|    brief: &std::path::Path,
 2619|    record: &Record,
...
 2730|            ErrorCode::Conflict,
*2731|            "generated native policy differs from the trusted composer recipe",
 2732|        ));
 2733|    }
 2734|    Ok(serde_json::from_slice(&files.manifest)?)
...
 2783|
*2784|fn available_models() -> Result<Vec<DelegationChoice>> {
 2785|    // Provider inventory is local to the same inherited profile as the eventual child.
 2786|    // File-backed bounded output avoids a full pipe blocking the probe at exit.
 2787|    let mut output = tempfile::tempfile()?;
 2788|    let mut child = Command::new("pi")
*2789|        .args(["--no-extensions", "--no-skills", "--list-models"])
 2790|        .stdin(Stdio::null())
 2791|        .stdout(output.try_clone()?)
 2792|        .stderr(Stdio::null())
...
 2846|    let inventory = match contract.provider.as_str() {
*2847|        "pi" => available_models()?,
*2848|        "codex" if contract.model_provider == "openai" => crate::codex_delegation::models()?,
 2849|        _ => bail!("requested harness/backend is unsupported; no substitute selected"),
 2850|    };
 2851|    if !inventory.iter().any(|choice| {
...
 3604|    #[test]
*3605|    fn old_contract_bytes_preserve_immutable_digest_without_weakening_comparison() {
 3606|        let directory = tempfile::tempdir().unwrap();
 3607|        let value = record(
 3608|            directory.path().join("assignment"),

## ovrcr-protocol/src/
### delegation.rs
 304|
*305|/// Delivery of one operation, never provider completion or parent acceptance.
 306|#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
 307|pub enum DispatchOperation {
 308|    Prepare,
...
 359|    pub workspace_present: bool,
*360|    /// May identify a replacement run; compare it with the allocated identity.
 361|    pub current_session: Option<CurrentAssignmentSession>,
 362|    #[serde(default)]
 363|    pub coding: Option<crate::CodingWorkspace>,
...
 695|
*696|/// A hint cursor is valid only for one Server lifetime. It is not task completion.
 697|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
 698|pub struct ObservationCursor {
 699|    pub server_epoch: String,

SOURCE LOOKUP {"pattern": "struct Workspace|branch|detached|WorktreeState|workspace_id", "path": "crates/ovrcr-protocol/src/wire.rs"}
 161|pub enum BranchRequest {
*162|    New { branch: String, base: String },
*163|    Existing { branch: String },
 164|}
 165|
 166|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
...
 202|        id: String,
*203|        branch: BranchRequest,
 204|    },
 205|    RemoveWorkspace {
 206|        project: String,
...
 288|        id: String,
*289|        branch: BranchRequest,
 290|        launch: Option<SessionLaunch>,
 291|    },
 292|    SetSessionTitle {
...
 363|    BridgeOwner(crate::BridgeOwnerCall),
*364|    /// Commit unignored changes onto `refs/heads/wip/<branch>` and push that
*365|    /// ref to `origin`. Does not move the checkout's branch or remove the
 366|    /// workspace.
 367|    SaveWorkspaceWip {
 368|        project: String,
...
 429|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*430|pub struct WorkspaceSummary {
 431|    pub project: String,
 432|    pub id: String,
 433|    pub name: String,
...
 540|        workspace: String,
*541|        branch: String,
 542|    },
 543|    /// A Lifecycle job finished. `client_token` is the request_id that accepted
 544|    /// the job on the Dashboard connection.
...
 765|                    id: "b".into(),
*766|                    branch: BranchRequest::Existing { branch: "c".into() },
 767|                },
 768|            ),
 769|            (
...
 910|                    id: "b".into(),
*911|                    branch: BranchRequest::Existing { branch: "c".into() },
 912|                    launch: Some(SessionLaunch {
 913|                        argv: vec!["sh".into()],
 914|                        label: None,
...
 923|                    id: "b".into(),
*924|                    branch: BranchRequest::Existing { branch: "c".into() },
 925|                    launch: None,
 926|                },
 927|            ),
...
 1250|                    workspace: "b".into(),
*1251|                    branch: "feature/topic".into(),
 1252|                },
 1253|            ),
 1254|            (
...
 1521|                path: "/work".into(),
*1522|                branch: "feature/topic".into(),
 1523|                git_identity: Some("1:2".into()),
 1524|                setup_pending: false,
 1525|            }),
...
 2107|            workspace: Some("label".into()),
*2108|            workspace_id: Some("stable".into()),
 2109|            cwd: Some("/w".into()),
 2110|            session: Some(SessionId(5)),
 2111|            run: Some(SessionRunId(6)),

SOURCE LOOKUP {"pattern": "agent run|fn run|apply_default_auto_trust|default_auto_trust_flag|prompt", "path": "crates/ovrcr-protocol/src/auto_trust.rs;crates/ovrcr-cli/src;crates/ovrcr-runtime/src/cli"}
 10|/// The flag a managed launch adds when the caller omitted the permission family.
*11|pub fn default_auto_trust_flag(provider: &str) -> Option<&'static str> {
 12|    match provider {
 13|        "claude" => Some("--dangerously-skip-permissions"),
 14|        "codex" => Some("--approve-for-me"),
...
 49|
*50|/// Append [`default_auto_trust_flag`] unless that family is already present.
 51|///
*52|/// The flag is inserted before a `--` prompt separator so admission still sees
*53|/// one prompt. Existing flags are never replaced or duplicated.
*54|pub fn apply_default_auto_trust(provider: &str, argv: &mut Vec<OsString>) {
*55|    let Some(flag) = default_auto_trust_flag(provider) else {
 56|        return;
 57|    };
 58|    if argv.is_empty()
...
 89|            let mut argv = args(&["/opt/bin/agent"]);
*90|            apply_default_auto_trust(provider, &mut argv);
 91|            assert_eq!(argv, args(&["/opt/bin/agent", flag]), "{provider}");
 92|        }
 93|    }
...
 98|            let mut argv = args(&["/opt/bin/agent"]);
*99|            apply_default_auto_trust(provider, &mut argv);
 100|            assert_eq!(argv, args(&["/opt/bin/agent"]), "{provider}");
 101|        }
 102|    }
...
 126|            let mut argv = args(original);
*127|            apply_default_auto_trust(provider, &mut argv);
 128|            assert_eq!(argv, args(original), "{provider} {original:?}");
 129|        }
 130|    }
...
 132|    #[test]
*133|    fn the_trust_flag_stays_before_a_prompt_separator() {
 134|        let mut argv = args(&["claude", "--model", "sonnet", "--", "ship it"]);
*135|        apply_default_auto_trust("claude", &mut argv);
 136|        assert_eq!(
 137|            argv,
 138|            args(&[
...
 151|        let mut argv = args(&["codex", "--model", "gpt-5", "--sandbox", "workspace-write"]);
*152|        apply_default_auto_trust("codex", &mut argv);
 153|        assert_eq!(
 154|            argv,
 155|            args(&[
...
 164|        let mut claude = args(&["claude", "-a"]);
*165|        apply_default_auto_trust("claude", &mut claude);
 166|        assert_eq!(
 167|            claude,
 168|            args(&["claude", "-a", "--dangerously-skip-permissions"])
...
 176|        let original = argv.clone();
*177|        apply_default_auto_trust("claude", &mut argv);
 178|        assert_eq!(argv, original);
 179|    }
 180|}

Skipped missing paths: crates/ovrcr-cli/src, crates/ovrcr-runtime/src/cli

SOURCE LOOKUP {"pattern": "last_launch|launch_memory|LaunchMemory|last_harness|remember|AgentOverride", "path": "crates"}
# crates/

## ovrcr-protocol/src/
### catalog.rs
 947|            picker_roots: vec![PathBuf::from("/tmp/a")],
*948|            agents: vec![crate::AgentOverride {
 949|                name: "claude".into(),
 950|                argv: vec!["claude".into()],
 951|            }],
### iterm.rs
 104|                "wire" => changed.server_wire += 1,
*105|                "owner" => changed.dashboard_owner = "remembered dashboard".into(),
 106|                "lifetime" => changed.context.server_lifetime = "not uuid".into(),
 107|                "socket" => changed.context.server_socket = "/tmp/../foreign.sock".into(),
 108|                _ => changed.context.callback_executable_sha256 = "g".repeat(64),
### settings.rs
 1428|#[serde(deny_unknown_fields)]
*1429|pub struct AgentOverride {
 1430|    pub name: String,
 1431|    pub argv: Vec<String>,
 1432|}
...
 1672|    pub picker_roots: Vec<PathBuf>,
*1673|    pub agents: Vec<AgentOverride>,
 1674|    pub cursor_cloud_accounts: Vec<CursorCloudAccount>,
 1675|    pub launch_choices: BTreeMap<String, LaunchChoice>,
 1676|    pub quota: QuotaSettings,
### wire.rs
 597|            }],
*598|            agents: vec![crate::AgentOverride {
 599|                name: "a".into(),
 600|                argv: vec!["b".into()],
 601|            }],

## ovrcr-runtime/src/

### server/
#### mod.rs
 609|        // Exited rows can be filed immediately. This does not acknowledge
*610|        // ownership uncertainty or signal a remembered process.
 611|        {
 612|            let _mutation = self.mutation_lock.lock().unwrap();
 613|            self.reject_if_stopping()?;
#### title.rs
 222|
*223|        // Phase 1: remember every eligible session whose history file changed.
 224|        // Marking seen here must not drop a sibling that becomes due in the
 225|        // same poll or while a call is in flight — those stay in `due`.
 226|        for record in &records {

### settings/
#### mod.rs
 10|use ovrcr_protocol::{
*11|    AgentOverride, AutomaticLocalTerminals, DelegationHarness, DelegationSettings, LaunchChoice,
 12|    NativeCommand, ReadySoundChoice, SettingRow, Settings, SettingsFinding, SettingsReport,
 13|    ThemeId,
 14|};
...
 265|        if let Some(agents) =
*266|            self.take_array::<AgentOverride>(table, "agents", "a table { name, argv }")
 267|        {
 268|            self.accept("agents", &mut settings.agents, agents);
 269|        }
...
 762|
*763|/// Serde names the Rust type (`struct AgentOverride`, `enum DocumentLaunchChoice`).
 764|/// The finding should name the document shape instead.
 765|fn element_message(message: String, expected: &str) -> String {
 766|    if (message.contains("struct ") || message.contains("enum "))
...
 1003|            settings.agents,
*1004|            vec![AgentOverride {
 1005|                name: "claude".into(),
 1006|                argv: vec!["claude".into(), "--x".into()]
 1007|            }]
#### write.rs
 472|    use ovrcr_protocol::{
*473|        AgentOverride, AutomaticLocalTerminals, LaunchChoice, ReadySoundChoice, Settings,
 474|    };
 475|    use std::os::unix::fs::{PermissionsExt, symlink};
 476|    use std::path::PathBuf;
...
 489|
*490|    /// The requests the Dashboard sends for `N`, `S`, `L` and a remembered
 491|    /// launch; `crates/ovrcr-tui` asserts it emits exactly these.
 492|    const N_ON: (&str, &str) = ("desktop_notifications", "true");
 493|    const S_ON: (&str, &str) = ("ready_sound", "true");
...
 583|                    s.agents
*584|                        == [AgentOverride {
 585|                            name: "a".into(),
 586|                            argv: vec!["a".into(), "-v".into()],
 587|                        }]
...
 595|                    s.agents
*596|                        == [AgentOverride {
 597|                            name: "b".into(),
 598|                            argv: vec!["b".into()],
 599|                        }]

## ovrcr-tui/src/dashboard/
### agents.rs
*1|use super::settings::AgentOverride;
 2|use std::ffi::{OsStr, OsString};
 3|use std::os::unix::fs::PermissionsExt;
 4|use std::path::{Path, PathBuf};
...
 98|    mut entries: Vec<AgentEntry>,
*99|    overrides: &[AgentOverride],
 100|) -> Vec<AgentEntry> {
 101|    let mut custom = entries.pop();
 102|    let mut shell = entries.pop();
...
 202|            agents,
*203|            &[AgentOverride {
 204|                name: "claude".into(),
 205|                argv: vec!["claude".into(), "--verbose".into()],
 206|            }],
...
 223|            agents,
*224|            &[AgentOverride {
 225|                name: "Custom".into(),
 226|                argv: vec!["my-tool".into(), "--flag".into()],
 227|            }],
...
 269|            entries,
*270|            &[AgentOverride {
 271|                name: "cursor-agent".into(),
 272|                argv: vec!["/custom/cursor-agent".into(), "--resume".into()],
 273|            }],
...
 348|            &[
*349|                AgentOverride {
 350|                    name: "claude".into(),
 351|                    argv: vec!["/custom/claude".into(), "--verbose".into()],
 352|                },
*353|                AgentOverride {
 354|                    name: "codex".into(),
 355|                    argv: vec!["/custom/codex".into()],
 356|                },
*357|                AgentOverride {
 358|                    name: "pi".into(),
 359|                    argv: vec!["/custom/pi".into()],
 360|                },
...
 400|            entries,
*401|            &[AgentOverride {
 402|                name: "hermes".into(),
 403|                argv: vec!["/custom/hermes".into(), "--profile".into(), "work".into()],
 404|            }],
### lifecycle_ui.rs
 251|        // Keep the launch preference across the Provisional wait so success can
*252|        // still ask the Server to remember it (palette closes on accept).
 253|        let preference = self
 254|            .palette
 255|            .as_mut()
### palette.rs
 3301|                    if let Some((project, choice)) = preference {
*3302|                        // The Server saves it; remembered here at once so the
 3303|                        // next launch offers it before the reading arrives.
 3304|                        let request = super::settings::launch_choice_request(&project, &choice);
 3305|                        self.settings.launch_choices.insert(project, choice);
...
 4480|mod launch_tests {
*4481|    use super::super::settings::{AgentOverride, LaunchChoice};
 4482|    use super::*;
 4483|    use crate::protocol::{
 4484|        AgentActivity, ErrorCode, ProjectSummary, ServerEvent, ServerMessage, SessionId,
...
 4504|        });
*4505|        d.settings.agents.push(AgentOverride {
 4506|            name: "fixture-agent".into(),
 4507|            argv: vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
 4508|        });
...
 4519|            let mut d = dashboard();
*4520|            d.settings.agents.push(AgentOverride {
 4521|                name: "claude".into(),
 4522|                argv: vec!["claude".into()],
 4523|            });
...
 4677|            let mut d = dashboard();
*4678|            d.settings.agents.push(AgentOverride {
 4679|                name: "shell".into(),
 4680|                argv: argv
 4681|                    .iter()
...
 4757|        }];
*4758|        d.settings.agents = vec![ovrcr_protocol::AgentOverride {
 4759|            name: "cursor-cloud:work".into(),
 4760|            argv: vec!["unrelated-custom-command".into()],
 4761|        }];
...
 4866|
*4867|    /// The Dashboard writes no file: the Server merges the remembered choice
 4868|    /// into the project's own entry (see `ovrcr-runtime`'s writer tests).
 4869|    #[test]
*4870|    fn successful_launch_asks_the_server_to_remember_the_choice() {
 4871|        for start in ["Terminal", "Agent"] {
 4872|            let mut d = dashboard();
 4873|            let project = "project.with-dots_and-punctuation";
...
 5131|    #[test]
*5132|    fn unavailable_remembered_agent_requires_explicit_replacement() {
 5133|        let mut d = dashboard();
 5134|        d.settings.launch_choices.insert(
 5135|            "demo".into(),
### settings.rs
 5|use ovrcr_protocol::Request;
*6|pub use ovrcr_protocol::{AgentOverride, AutomaticLocalTerminals, LaunchChoice, Settings};
 7|
 8|pub(super) const INVALID_SOUND_CHOICE_NOTICE: &str =
 9|    "Ready sound choice invalid; banners are silent. Choose a sound in Settings";
...
 31|
*32|/// The `SetSetting` request that remembers `choice` for `project`. The
 33|/// Server merges it into the project's entry and keeps other projects.
 34|pub(super) fn launch_choice_request(project: &str, choice: &LaunchChoice) -> Request {
 35|    let mut entry = toml_edit::InlineTable::new();
### settings_editor.rs
 1423|    fn editor(document: bool) -> super::super::Dashboard {
*1424|        use ovrcr_protocol::{AgentOverride, ServerEvent, ServerMessage, TerminalSize};
 1425|        let settings = Settings {
 1426|            picker_roots: vec!["/a".into(), "/b".into(), "/c".into()],
 1427|            agents: vec![
*1428|                AgentOverride {
 1429|                    name: "one".into(),
 1430|                    argv: vec!["one".into()],
 1431|                },
*1432|                AgentOverride {
 1433|                    name: "two".into(),
 1434|                    argv: vec!["two".into()],
 1435|                },
### unread.rs
 37|            // Nothing open: forget the delivered ids so a later reopening alerts again,
*38|            // and leave the entry only while a binding is worth remembering.
 39|            self.requests.remove(&session.id);
 40|            return Vec::new();
 41|        }

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/settings.rs:1-80"}
//! The Dashboard's settings: what the Server published, and the requests that
//! change it. Reading and writing belong to the Server, which publishes its
//! reading as a `SettingsReport`; the Dashboard has no parser and touches no
//! file.
use ovrcr_protocol::Request;
pub use ovrcr_protocol::{AgentOverride, AutomaticLocalTerminals, LaunchChoice, Settings};

pub(super) const INVALID_SOUND_CHOICE_NOTICE: &str =
    "Ready sound choice invalid; banners are silent. Choose a sound in Settings";

pub(super) fn invalid_sound_choice_notice(settings: &Settings) -> Option<&'static str> {
    (cfg!(target_os = "macos")
        && settings.desktop_notifications
        && settings.ready_sound
        && settings.ready_sound_choice.is_none())
    .then_some(INVALID_SOUND_CHOICE_NOTICE)
}

/// Keep the actionable sound finding visible when both macOS alert flags are on.
/// The full reading and every finding remain available in Settings.
pub(super) fn findings_notice(settings: &Settings, count: usize) -> String {
    if let Some(notice) = invalid_sound_choice_notice(settings) {
        return notice.into();
    }
    match count {
        0 => "No settings findings".into(),
        1 => "1 settings finding; see Settings".into(),
        count => format!("{count} settings findings; see Settings"),
    }
}

/// The `SetSetting` request that remembers `choice` for `project`. The
/// Server merges it into the project's entry and keeps other projects.
pub(super) fn launch_choice_request(project: &str, choice: &LaunchChoice) -> Request {
    let mut entry = toml_edit::InlineTable::new();
    match choice {
        LaunchChoice::Terminal => {
            entry.insert("kind", "Terminal".into());
        }
        LaunchChoice::Agent(preset) => {
            entry.insert("kind", "Agent".into());
            entry.insert("preset", preset.as_str().into());
        }
    }
    Request::SetSetting {
        path: format!("launch_choices.{}", toml_edit::Key::new(project)),
        value: Some(entry.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ovrcr-runtime`'s writer tests apply exactly these values.
    #[test]
    fn launch_choice_requests_quote_the_project_and_name_every_field() {
        assert_eq!(
            launch_choice_request("project.with.dots", &LaunchChoice::Agent("new".into())),
            Request::SetSetting {
                path: "launch_choices.\"project.with.dots\"".into(),
                value: Some("{ kind = \"Agent\", preset = \"new\" }".into()),
            }
        );
        assert_eq!(
            launch_choice_request("demo", &LaunchChoice::Terminal),
            Request::SetSetting {
                path: "launch_choices.demo".into(),
                value: Some("{ kind = \"Terminal\" }".into()),
            }
        );
    }
}

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/settings.rs:1370-1460"}
pub enum AutomaticLocalTerminals {
…
}

impl AutomaticLocalTerminals {
    pub const KEY: &'static str = "automatic_local_terminals";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::DefaultBranchOnly => "default_branch_only",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::DefaultBranchOnly => "default branch only",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            "default_branch_only" | "default branch only" => Some(Self::DefaultBranchOnly),
            _ => None,
        }
    }

    /// Cycle for the Dashboard preference control.
    pub fn next(self) -> Self {
        match self {
            Self::DefaultBranchOnly => Self::On,
            Self::On => Self::Off,
            Self::Off => Self::DefaultBranchOnly,
        }
    }

    /// Whether automatic provisioning should create a `local` terminal.
    ///
    /// `is_default_branch_workspace` is true only for the protected repository-root
    /// workspace (the detected default-branch checkout), never a feature worktree.
    pub fn should_create(self, is_default_branch_workspace: bool) -> bool {
        match self {
            Self::On => true,
            Self::Off => false,
            Self::DefaultBranchOnly => is_default_branch_workspace,
        }
    }
}

impl fmt::Display for AutomaticLocalTerminals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOverride {
    pub name: String,
    pub argv: Vec<String>,
}

/// Nonsecret selection policy; this name does not attest a billing identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorCloudAccount {
    pub name: String,
    pub keychain_service: String,
    pub keychain_account: String,
    pub model: String,
    pub repositories: Vec<String>,
    pub owners: Vec<String>,
    pub bridge_archive: PathBuf,
}

impl CursorCloudAccount {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.name.is_empty()
                && self.name.len() <= 64
                && self
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
            "invalid cloud account name"
        );
        for value in [&self.keychain_service, &self.keychain_account, &self.model] {
            anyhow::ensure!(
                !value.trim().is_empty()
                    && value.len() <= 256
                    && !value.chars().any(char::is_control),
                "explicit Keychain reference and model required"
            );
        }
…
    }
…
}

[Showing lines 1369-1463 of 2030. Use :1464 to continue]

SOURCE LOOKUP {"pattern": "fn agent_run|AgentRun|agent.*Run|Command::Agent|\\\"run\\\"", "path": "crates"}
# crates/

## ovrcr-protocol/src/
### delegation.rs
 1136|    fn publication_bytes_preserve_legacy_research_and_exact_coding_payload() {
*1137|        let legacy = br#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original research.","evidence":["README:1"],"unresolved":[]}"#;
 1138|        let mut submission: ResearchSubmission = serde_json::from_slice(legacy).unwrap();
 1139|        assert!(submission.coding.is_none());
 1140|        assert_eq!(submission.publication_bytes().unwrap(), legacy);
### delegation_coding.rs
 244|            assert_eq!(used, bytes.len());
*245|            let mut submission = serde_json::json!({"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"failed","summary":"actual failed check","evidence":["run.log"],"unresolved":[]});
 246|            if coding {
 247|                submission["coding"] = serde_json::json!({"checks":[{"name":"test","command":"false","outcome":"Failed","exit_code":1,"evidence":["run.log"]}],"candidate":null});
 248|            }
### delegation_fallback.rs
 201|        // Legacy declaration golden, including the nested flat child reference.
*202|        let original = r#"{"request":"legacy-delivery","workflow":"workflow","scope":"Deliver the original reviewed result","provenance":"Original parent decision","resources":[{"handle":"request","identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":3},"result_version":1,"obligation_revision":1,"publication":{"assignment":{"instance":"instance","workflow":"workflow","controller":"par... [col 368]
 203|        let declaration: crate::DeliveryDeclaration = serde_json::from_str(original).unwrap();
 204|        declaration.validate().unwrap();
 205|        assert_eq!(declaration.resources.len(), 1);
### delegation_review_tests.rs
 4|    serde_json::from_value(serde_json::json!({
*5|        "assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},
*6|        "publication":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},"kind":"Submission","version":1,"digest":"a".repeat(64)},
 7|        "candidate":{"path":"/private/task/candidate-v1.json","digest":"b".repeat(64),"workspace":"coding","base":"c".repeat(40),"git_identity":"generation","changed_artifacts":["untracked.cjs"]},
 8|        "source_contract_digest":"d".repeat(64)
 9|    })).unwrap()
...
 165|fn canonical_submission_bytes_preserve_literal_old_research_and_coding_and_bind_review() {
*166|    const RESEARCH: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[]}"#;
*167|    const CODING: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[],"coding":{"checks":[],"candidate":null}}"#;
 168|    // Intentional accepted332 union compatibility: review Some retains coding:null.
 169|    // These are literal authenticated bytes, independent of the serializer under test.
*170|    const RESEARCH_REVIEW: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[],"coding":null,"review":{"target":{"coding":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract... [col 220]
*171|    const CODING_REVIEW: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[],"coding":{"checks":[],"candidate":null},"review":{"target":{"coding":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementatio... [col 218]
 172|    for (original, expected_review) in [(RESEARCH, RESEARCH_REVIEW), (CODING, CODING_REVIEW)] {
 173|        let mut submission: crate::ResearchSubmission = serde_json::from_str(original).unwrap();
 174|        assert_eq!(submission.publication_bytes().unwrap(), original.as_bytes());

## ovrcr-runtime/

### src/
#### claude_recovery.rs
 172|        "agent".into(),
*173|        "run".into(),
 174|        "claude".into(),
 175|        "--".into(),
 176|        reference.executable.clone().into_os_string(),
#### codex_recovery.rs
 145|        "agent".into(),
*146|        "run".into(),
 147|        "codex".into(),
 148|        "--".into(),
 149|        reference.executable.clone().into_os_string(),
#### config.rs
 897|                    },
*898|                    "run": 7,
 899|                    "lifetime": "lifetime-1",
 900|                    "invocation": "invocation-1",
 901|                    "claimed": true,
...
 1001|        // An upgrade must preserve raw legacy metadata, including unknown outcomes.
*1002|        let metadata = r#"{"handle":"original","identity":{"instance":"i","workflow":"wf","controller":"parent","controller_epoch":1,"task":"t","attempt":1,"contract":1,"request":"original","session":42,"run":7},"receipts":[{"request":"original","operation":"InitialInput","outcome":"Unknown","input":{"written_bytes":7,"total_bytes":100,"submit_written":false}}],"contract_digest":"original-contract","result_digest":"original-result"}"#;
 1003|        connection.execute("INSERT INTO delegations(handle,workflow,session,metadata) VALUES('original','wf',42,?1)", [metadata]).unwrap();
 1004|        let evidence = config.join("workflows/original");
 1005|        std::fs::create_dir_all(&evidence).unwrap();
#### extension_recovery.rs
 178|        "agent".into(),
*179|        "run".into(),
 180|        provider.name().into(),
 181|        "--".into(),
 182|        reference.executable.clone().into_os_string(),

#### cursor_cloud/
##### run.rs
 537|        )?;
*538|        identity(&snapshot["run"], agent, &id)?;
*539|        terminal_status_model(&snapshot["run"], &creation.account.model)?;
 540|        ensure!(
 541|            control.attempted.load(std::sync::atomic::Ordering::Acquire)
*542|                || snapshot["run"]["status"] == result["status"],
 543|            "independent exact-run status mismatch"
 544|        );
*545|        let status = terminal(&snapshot["run"]["status"])?;
 546|        if status == CloudRunStatus::Finished {
 547|            ensure!(source, "source tool proof missing; result unverified");
 548|        }
*549|        Ok((id, status, snapshot["run"].clone()))
 550|    }();
 551|    match qualify {
 552|        Ok((id, status, snapshot)) => {
...
 781|        let snapshot = catalog.rpc("SdkAgentService", "GetRun", &json!({"runId":target.run_id,"options":{"runtime":"RUNTIME_CLOUD","agentId":agent,"apiKey":key}}))?;
*782|        identity(&snapshot["run"], agent, &target.run_id)?;
*783|        if let Ok(status) = terminal(&snapshot["run"]["status"]) {
 784|            return Ok(Some(status));
 785|        }
 786|        ensure!(
 787|            matches!(
*788|                snapshot["run"]["status"].as_str(),
 789|                Some("RUN_LIFECYCLE_STATUS_CREATING" | "RUN_LIFECYCLE_STATUS_RUNNING")
 790|            ),
 791|            "unknown exact-run lifecycle; observation remains uncertain"

#### git/
##### candidate.rs
 732|        let root = tempfile::tempdir().unwrap();
*733|        fs::write(root.path().join("run"), b"old bytes").unwrap();
*734|        fs::set_permissions(root.path().join("run"), fs::Permissions::from_mode(0o755)).unwrap();
 735|        assert_eq!(
*736|            read_entry(root.path(), Path::new("run")).unwrap(),
 737|            Some((0o100755, b"old bytes".to_vec()))
 738|        );
*739|        symlink("run", root.path().join("link")).unwrap();
 740|        assert_eq!(
 741|            read_entry(root.path(), Path::new("link")).unwrap(),
*742|            Some((0o120000, b"run".to_vec()))
 743|        );
 744|        symlink("../escape", root.path().join("bad-link")).unwrap();
 745|        assert!(read_entry(root.path(), Path::new("bad-link")).is_err());
##### diagnostics.rs
 95|        }
*96|        journal.record(json!({"kind":"ready", "run":run, "owner":owner}), false);
 97|        Ok(journal)
 98|    }
 99|

##### candidate/
###### baseline.rs
 319|    fn target() -> CodingTargetReference {
*320|        serde_json::from_value(serde_json::json!({"assignment":{"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"a","attempt":1,"contract":1,"request":"a","session":1,"run":1},"publication":{"assignment":{"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"a","attempt":1,"contract":1,"request":"a","session":1,"run":1},"kind":"Submission","version":1,"digest":"1".repeat(64)},"candidate":{"path":"/private/candidate.json","digest":"2".repeat(64),"workspace"... [col 195]
 321|    }
 322|    #[test]
 323|    fn complete_inherited_untracked_bytes_do_not_expand_successor_write_authority() {

#### server/
##### delegation.rs
 1512|                                "agent".into(),
*1513|                                "run".into(),
 1514|                                "claude".into(),
 1515|                                "--".into(),
 1516|                            ];
...
 1537|                                "agent".into(),
*1538|                                "run".into(),
 1539|                                "codex".into(),
 1540|                                "--".into(),
 1541|                                codex_executable
...
 1556|                                "agent".into(),
*1557|                                "run".into(),
 1558|                                "pi".into(),
 1559|                                "--".into(),
 1560|                                "pi".into(),
##### delegation_observation.rs
 536|    fn epoch_changes_gaps_and_repeated_snapshots_do_not_reset_deadline() {
*537|        let identity: AssignmentIdentity = serde_json::from_value(serde_json::json!({"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"t","attempt":1,"contract":1,"request":"r","session":1,"run":1})).unwrap();
 538|        let mut checkpoint = Checkpoint {
 539|            identity,
 540|            result_version: 1,
...
 585|    fn future_checkpoint_expires_instead_of_extending_its_deadline() {
*586|        let identity: AssignmentIdentity = serde_json::from_value(serde_json::json!({"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"t","attempt":1,"contract":1,"request":"r","session":1,"run":1})).unwrap();
 587|        let mut checkpoint = Checkpoint {
 588|            identity,
 589|            result_version: 1,
##### mod.rs
 1445|                        "agent".into(),
*1446|                        "run".into(),
 1447|                        "cursor-cloud".into(),
 1448|                        "--".into(),
 1449|                        target.intent.account.name.clone().into(),
...
 2969|                None,
*2970|                &argv(&["/usr/local/bin/ovrcr", "agent", "run", "pi", "--", "pi"]),
 2971|            ),
 2972|            "pi"
 2973|        );
...
 2986|                    "agent",
*2987|                    "run",
 2988|                    "--provider",
 2989|                    "claude",
 2990|                    "--",
...
 3011|                Some("custom-harness".into()),
*3012|                &argv(&["ovrcr", "agent", "run", "pi", "--", "pi"]),
 3013|            ),
 3014|            "custom-harness"
 3015|        );
##### tests.rs
 6914|            submit(
*6915|                "run",
 6916|                CloudCreationCommand::AcknowledgeSend {
 6917|                    key: send.key.clone(),
 6918|                    agent_id: send.agent_id.clone(),

##### delegation_coding/
###### publication.rs
 148|    fn report() -> ResearchSubmission {
*149|        serde_json::from_value(serde_json::json!({"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"failed","summary":"Actual failed check.","evidence":["check.log"],"unresolved":[],"coding":{"checks":[{"name":"failure","command":"false","outcome":"Failed","exit_code":1,"evidence":["check.log"]},{"name":"unrun","command":null,"outcome":"Unrun",... [col 232]
 150|    }
 151|    fn prepare_and_commit(
 152|        directory: &Directory,

### tests/
#### delegation_cancellation_state.rs
 505|        "result_version",
*506|        "run",
 507|        "attempt",
 508|        "contract",
 509|    ] {
...
 514|            "result_version" => changed.result_version += 1,
*515|            "run" => changed.identity.run = Some(SessionRunId(100)),
 516|            "attempt" => changed.identity.attempt += 1,
 517|            _ => changed.identity.contract += 1,
 518|        }

## ovrcr-tui/src/dashboard/
### agents.rs
 64|                        "agent".into(),
*65|                        "run".into(),
 66|                        name.into(),
 67|                        "--".into(),
 68|                    ];
...
 261|                "agent".into(),
*262|                "run".into(),
 263|                "cursor-agent".into(),
 264|                "--".into(),
 265|                dir.path().join("cursor-agent").into_os_string(),
...
 320|                "agent".into(),
*321|                "run".into(),
 322|                name.into(),
 323|                "--".into(),
 324|                dir.path().join(name).into_os_string(),
...
 392|                "agent".into(),
*393|                "run".into(),
 394|                "hermes".into(),
 395|                "--".into(),
 396|                dir.path().join("hermes").into_os_string(),
### palette.rs
 1097|        let command = if unavailable {
*1098|            Command::AgentResumeUnavailable(session.id)
 1099|        } else {
 1100|            Command::ReopenTerminal(session.id)
 1101|        };
...
 1415|            },
*1416|            Command::AgentResumeUnavailable(id) => Page::Confirm {
 1417|                request: None,
 1418|                target: find_session(self, id)
 1419|                    .and_then(|session| session.recovery.as_ref())
...
 1647|                                label: "Agent resume is unavailable".into(),
*1648|                                command: Command::AgentResumeUnavailable(id),
 1649|                            }
 1650|                        });
 1651|                        entries.push(Entry {
...
 2640|                                "agent".into(),
*2641|                                "run".into(),
 2642|                                "cursor-cloud".into(),
 2643|                                "--".into(),
 2644|                                account.name.clone().into(),
...
 4778|            &launch.argv[1..],
*4779|            ["agent", "run", "cursor-cloud", "--", "work"].map(OsString::from)
 4780|        );
 4781|    }
 4782|
...
 5318|        d.palette = Some(Palette {
*5319|            page: d.command_page(Command::AgentResumeUnavailable(SessionId(1))),
 5320|            ..Palette::new()
 5321|        });
 5322|        assert!(matches!(
...
 5348|        d.palette = Some(Palette {
*5349|            page: d.command_page(Command::AgentResumeUnavailable(SessionId(1))),
 5350|            ..Palette::new()
 5351|        });
 5352|        let Page::Confirm { request, target } = &d.palette.as_ref().unwrap().page else {

Showing files 1-20 of 21. Use skip=20 for the next page, or narrow paths/pattern.

[Some lines truncated to 512 bytes]

SOURCE LOOKUP {"path": "crates/*/Cargo.toml;Cargo.toml"}
Cargo.toml
# crates/
## ovrcr-runtime/
Cargo.toml
## ovrcr-protocol/
Cargo.toml
## ovrcr-tui/
Cargo.toml
## ovrcr-terminal/
Cargo.toml

SOURCE LOOKUP {"pattern": "struct AgentArgs|AgentCommand|Run \\{|enum Agent|fn run_agent|launch_agent", "path": "crates"}
# crates/

## ovrcr-protocol/src/
### agent.rs
 6|#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
*7|pub enum AgentProvider {
 8|    Claude,
 9|    Codex,
 10|    Grok,
...
 200|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*201|pub enum AgentObservation {
 202|    Activity(ActivitySample),
 203|    Metrics(Box<MetricsSample>),
 204|    Health(HealthSample),
...
 329|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*330|pub enum AgentCommand {
 331|    /// Sent by the native-root-authenticated Reporter, never a task tool.
 332|    WorkflowSource {
 333|        binding: Box<AgentBinding>,
...
 367|    pub operation: String,
*368|    pub command: AgentCommand,
 369|}
 370|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*371|pub enum AgentOperationResult {
 372|    WorkflowUpdated,
 373|    Reserved(AgentReservation),
 374|    Bound(AgentBinding),
...
 931|        for command in [
*932|            AgentCommand::Bind {
 933|                expected_binding: None,
 934|                conversation: "conv".into(),
 935|            },
*936|            AgentCommand::Finalize {
 937|                binding: binding.clone(),
 938|                revision: 3,
 939|                final_metrics: metrics().into(),
 940|            },
*941|            AgentCommand::Release {
 942|                expected_binding: Some(binding.clone()),
 943|            },
*944|            AgentCommand::Health(health.clone()),
*945|            AgentCommand::RetainConversation {
 946|                binding: binding.clone(),
 947|                reference: Box::new(ConversationReference::Claude(ClaudeConversation {
 948|                    conversation: "conv".into(),
...
 954|            },
*955|            AgentCommand::InvalidateConversation,
 956|        ] {
 957|            requests.push(Request::Supervisor(SupervisorRequest {
 958|                auth: auth.clone(),
...
 1039|                operation: "creation".into(),
*1040|                command: AgentCommand::CloudCreation(Box::new(command)),
 1041|            }));
 1042|        }
 1043|        requests
### session.rs
 53|#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
*54|pub enum AgentActivity {
 55|    Unknown,
 56|    Idle,
 57|    Busy,
### task.rs
 78|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*79|pub struct Run {
 80|    pub id: RunId,
 81|    pub task_id: TaskId,
 82|    pub spec: TaskSpec,
### wire.rs
 123|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*124|pub enum AgentUpdate {
 125|    Activity(AgentActivity),
 126|    Context(ContextUsageReport),
 127|    Provider(crate::ProviderReport),

## ovrcr-runtime/src/
### tasks.rs
 262|        let id = RunId(self.take_run_id()?);
*263|        let run = Run {
 264|            id,
 265|            task_id: task.id,
 266|            spec: task.spec.clone(),

### server/
#### connections.rs
 577|            let (completion, result) = mpsc::sync_channel(1);
*578|            match state.dispatch.try_send(DispatchMessage::AgentCommand {
 579|                request,
 580|                owner: owner.cloned(),
 581|                completion,
#### dispatch.rs
 40|    },
*41|    AgentCommand {
 42|        request: Request,
 43|        owner: Option<Arc<()>>,
 44|        completion: SyncSender<Response>,
...
 130|            }
*131|            DispatchMessage::AgentCommand {
 132|                request,
 133|                owner,
 134|                completion,
...
 172|                                crate::session::AgentPersistence::Command(
*173|                                    ovrcr_protocol::AgentCommand::WorkflowSource {
 174|                                        binding,
 175|                                        producer,
 176|                                        reason,
...
 187|                                crate::session::AgentPersistence::Command(
*188|                                    ovrcr_protocol::AgentCommand::WorkflowDeparture {
 189|                                        binding,
 190|                                        producer,
 191|                                        reaped,
...
 201|                                crate::session::AgentPersistence::Command(
*202|                                    ovrcr_protocol::AgentCommand::RetainConversation {
 203|                                        reference,
 204|                                        ..
 205|                                    },
...
 209|                                crate::session::AgentPersistence::Command(
*210|                                    ovrcr_protocol::AgentCommand::InvalidateConversation,
 211|                                ) => retained.retain_conversation(id, session.run(), None),
 212|                                _ => Ok(()),
 213|                            })
#### mod.rs
 139|        }
*140|        DispatchMessage::AgentCommand { request, .. } => {
 141|            bincode::serde::encode_to_vec(request, bincode::config::standard())
 142|                .map_or(0, |bytes| bytes.len())
 143|        }
#### quota_probe.rs
 139|
*140|enum Run {
 141|    Finished(ProbeUpdate),
 142|    Aborted,
 143|}
 144|
*145|fn run_one(state: &ServerState) -> Run {
 146|    let command = quota::claude_command(&state.settings.lock().unwrap().report.settings);
 147|    let instance = state
 148|        .socket
...
 232|    listener: &std::os::unix::net::UnixListener,
*233|) -> Run {
 234|    let pty = native_pty_system();
 235|    let pair = match pty.openpty(PtySize {
 236|        rows: 24,
#### tests.rs
 6361|                    &format!("retain-{index}"),
*6362|                    AgentCommand::RetainConversation {
 6363|                        binding: binding.clone(),
 6364|                        reference: reference(id)
 6365|                    }
...
 6378|                "late-a",
*6379|                AgentCommand::RetainConversation {
 6380|                    binding: original,
 6381|                    reference: reference(a)
 6382|                }
...
 6389|                "wrong-reference",
*6390|                AgentCommand::RetainConversation {
 6391|                    binding: binding.clone(),
 6392|                    reference: reference(b)
 6393|                }
...
 6400|                "release",
*6401|                AgentCommand::Release {
 6402|                    expected_binding: Some(binding.clone())
 6403|                }
 6404|            ),
...
 6410|                "retired-invalidate",
*6411|                AgentCommand::InvalidateConversation
 6412|            ),
 6413|            Response::Error { .. }
 6414|        ));
...
 6418|                "retired-retain",
*6419|                AgentCommand::RetainConversation {
 6420|                    binding,
 6421|                    reference: reference(b)
 6422|                }
...
 6517|            operation: &str,
*6518|            command: AgentCommand,
 6519|        ) -> Response {
 6520|            self.request(Request::Supervisor(SupervisorRequest {
 6521|                auth: auth.clone(),
...
 6535|                operation,
*6536|                AgentCommand::Bind {
 6537|                    expected_binding: expected,
 6538|                    conversation: conversation.into(),
 6539|                },
...
 6629|                    operation,
*6630|                    AgentCommand::CloudCreation(Box::new(command))
 6631|                ),
 6632|                Response::AgentOperation(AgentOperationResult::CloudCreationCommitted)
 6633|            );
...
 6638|                "release",
*6639|                AgentCommand::Release {
 6640|                    expected_binding: None
 6641|                }
 6642|            ),
...
 6655|            "stale-ack",
*6656|            AgentCommand::CloudCreation(Box::new(receipt.clone())),
 6657|        ));
 6658|        rejected(f.command(
 6659|            &replacement,
 6660|            "replacement-ack",
*6661|            AgentCommand::CloudCreation(Box::new(receipt)),
 6662|        ));
 6663|        assert!(
 6664|            f.state
...
 6691|                    operation,
*6692|                    AgentCommand::CloudCreation(Box::new(command))
 6693|                ),
 6694|                Response::AgentOperation(AgentOperationResult::CloudCreationCommitted)
 6695|            );
...
 6709|                "late-ack",
*6710|                AgentCommand::CloudCreation(Box::new(receipt.clone()))
 6711|            ),
 6712|            Response::AgentOperation(AgentOperationResult::CloudCreationCommitted)
 6713|        );
...
 6733|            "late-after-delete",
*6734|            AgentCommand::CloudCreation(Box::new(receipt)),
 6735|        ));
 6736|        assert!(f.state.session_summary(SessionId(900)).is_none());
 6737|    }
...
 6755|                operation,
*6756|                AgentCommand::CloudCreation(Box::new(command)),
 6757|            )
 6758|        };
 6759|        rejected(submit(
...
 6842|        };
*6843|        let submit = |op: &str, c| f.command(&auth, op, AgentCommand::CloudCreation(Box::new(c)));
 6844|        for (op, c) in [
 6845|            ("prepare", CloudCreationCommand::Prepare(creation.clone())),
 6846|            (
...
 7137|        };
*7138|        let submit = |op: &str, c| f.command(&auth, op, AgentCommand::CloudCreation(Box::new(c)));
 7139|        for (op, c) in [
 7140|            ("prepare", CloudCreationCommand::Prepare(creation.clone())),
 7141|            (
...
 7325|            "bind-a",
*7326|            AgentCommand::Bind {
 7327|                expected_binding: None,
 7328|                conversation: "A".into(),
 7329|            },
...
 7340|            "late-bind",
*7341|            AgentCommand::Bind {
 7342|                expected_binding: Some(a.clone()),
 7343|                conversation: "C".into(),
 7344|            },

### session/
#### reporting.rs
 263|
*264|pub(crate) enum AgentPersistence<'a> {
 265|    CloudCreation(&'a ovrcr_protocol::CloudCreationCommand, &'a str),
 266|    Reservation(&'a ReserveAgent),
*267|    Command(&'a AgentCommand),
 268|}
 269|
 270|impl Session {
...
 507|        let result = match &r.command {
*508|            AgentCommand::CloudCreation(command) => {
 509|                let reserve = &reporting
 510|                    .reservation
 511|                    .as_ref()
...
 531|
*532|            AgentCommand::WorkflowSource {
 533|                binding,
 534|                producer,
 535|                source,
...
 555|            }
*556|            AgentCommand::WorkflowDeparture {
 557|                binding, producer, ..
 558|            } => {
 559|                reporting.expected(Some(binding))?;
...
 568|
*569|            AgentCommand::InvalidateConversation => {
 570|                // Invalidation concerns the entire authenticated invocation, even
 571|                // when a successful Bind receipt was lost before this callback.
 572|                if reporting
...
 581|            }
*582|            AgentCommand::RetainConversation { binding, reference } => {
 583|                reporting.expected(Some(binding))?;
 584|                if !reference.matches_binding(binding) {
 585|                    bail!("Recovery must match the certified provider binding");
...
 590|            }
*591|            AgentCommand::Bind {
 592|                expected_binding,
 593|                conversation,
 594|            } => {
...
 633|            }
*634|            AgentCommand::Release { expected_binding } => {
 635|                reporting.expected(expected_binding.as_ref())?;
 636|                reporting.lost("unfinalized_release");
 637|                AgentOperationResult::Released
 638|            }
*639|            AgentCommand::Finalize {
 640|                binding,
 641|                revision,
 642|                final_metrics,
...
 658|            }
*659|            AgentCommand::Health(report) => {
 660|                if !matches!(report.observation, AgentObservation::Health(_)) {
 661|                    bail!("expected supervisor health observation");
 662|                }
#### tests.rs
 1558|                    operation: operation.into(),
*1559|                    command: AgentCommand::Bind {
 1560|                        expected_binding: previous,
 1561|                        conversation: conversation.into(),
 1562|                    },

## ovrcr-tui/src/
### task_tui.rs
 1359|                format!(
*1360|                    " Run {} · {:?} · {} ",
 1361|                    r.id.0,
 1362|                    r.status,
 1363|                    r.directory

### dashboard/
#### agents.rs
 34|#[derive(Clone, Copy, Debug, PartialEq, Eq)]
*35|pub enum AgentSource {
 36|    Detected,
 37|    Shell,
 38|    Custom,

SOURCE LOOKUP {"path": "crates/ovrcr-terminal/src", "pattern": "agent|run|prompt"}
No matches found

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

SOURCE LOOKUP {"path": "."}
.
  - CONTEXT.md 15.3KB 21m ago
  - .worktrees/ 40m ago
    - ux-pass-integration/ 1h ago
    - ux-transcript/ 23h ago
    - ux-new-task/ 23h ago
    - ux-omp-scroll/ 1d ago
    - ux-header/ 1d ago
    - ux-workspace-card/ 1d ago
    - ux-onboarding/ 1d ago
    - ux-worktree-names/ 1d ago
    - ux-commands/ 1d ago
    - ux-sidebar/ 1d ago
    - ux-forms/ 1d ago
    - … 18 more
    - codex-ux-ten-passes/ 1mo ago
  - research/ 1h ago
    - workspace-session-ux-2026-11-05/ 7m ago
    - operator-ux-2026-11-05/ 1h ago
    - operator-full-width-2026-11-03/ 1d ago
    - rig-operator-2026-10-29/ 3d ago
    - operator-working-journal-2026-10-30/ 3d ago
    - operator-main-integration-2026-10-30/ 3d ago
    - operator-layout-2026-10-30/ 3d ago
    - operator-inline-input-2026-10-30/ 3d ago
    - operator-activity-2026-10-30/ 3d ago
    - dashboard-outbox-2026-10-31/ 3d ago
    - notification-bridge/ 4w ago
    - … 38 more
    - superset-cli-2026-09-05/ 2mo ago
  - tests/ 1d ago
    - pi_question_extension.mjs 7.6KB 1d ago
    - pi_delegation_extension.mjs 50.4KB 1d ago
    - tui/ 3d ago
    - terminal_acceptance/ 3d ago
    - terminal_acceptance.rs 96.3KB 3d ago
    - task_tui.rs 38.6KB 3d ago
    - support/ 3d ago
    - service.rs 23.0KB 3d ago
    - server_lifecycle.rs 710.3KB 3d ago
    - provider_quota.rs 92.4KB 3d ago
    - pi_reporting_extension.mjs 22.8KB 3d ago
    - … 45 more
    - task_runner.rs 17.1KB 1mo ago
  - src/ 1d ago
    - ovrcr-pi-runtime.ts 60.8KB 1d ago
    - report/ 3d ago
    - report.rs 27.8KB 3d ago
    - operator/ 3d ago
    - lib.rs 1.5KB 3d ago
    - gui/ 3d ago
    - cli/ 3d ago
    - pi-reporting-extension.mjs 368B 2w ago
    - ovrcr-pi-interrupt.ts 2.3KB 2w ago
    - ovrcr-delegation-observer.mjs 6.0KB 2w ago
    - ovrcr-delegation-continuation.mjs 5.8KB 2w ago
    - … 8 more
    - main.rs 41B 1mo ago
  - docs/ 1d ago
    - pi-research-assignments.md 47.9KB 1d ago
    - development/ 1d ago
    - ux/ 3d ago
    - operator.md 9.6KB 3d ago
    - dashboard.md 69.4KB 3d ago
    - cursor-cloud.md 24.5KB 3d ago
    - cli-reference.md 31.7KB 3d ago
    - adr/ 3d ago
    - pi-claude-delegation.md 5.6KB 2w ago
    - pi-package-setup.md 6.3KB 2w ago
    - pi-independent-review.md 10.3KB 2w ago
    - … 20 more
    - scheduled-tasks.md 8.2KB 1mo ago
  - reports/ 2d ago
    - Superset orchestration for OVRCR.md 42.2KB 2d ago
    - cursor-cloud-363/ 3d ago
  - research_notes/ 2d ago
    - Superset orchestration for OVRCR/ 2d ago
  - plans/ 3d ago
    - 2026-09-08-operator-agent-design.md 14.7KB 3d ago
    - 2026-10-02-event-log.md 3.4KB 4w ago
    - 2026-10-01-usage-defaults-plan.md 4.4KB 1mo ago
    - 2026-10-01-shared-settings.md 9.8KB 1mo ago
    - 2026-10-01-settings-lifecycle-tests.md 4.0KB 1mo ago
    - 2026-10-01-settings-editor.md 5.7KB 1mo ago
    - 2026-10-01-quota-startup.md 5.3KB 1mo ago
    - 2026-10-01-claude-quota-retrieval.md 7.0KB 1mo ago
    - 2026-09-08-session-restore.md 10.4KB 1mo ago
    - 2026-09-15-durable-agent-sessions-spec.md 22.0KB 1mo ago
    - 2026-09-15-automatic-session-titles.md 4.7KB 1mo ago
    - … 15 more
    - 2026-09-08-operator-agent.md 73.4KB 1mo ago
  - vendor/ 3d ago
    - cursor-sdk-bridge/ 3d ago
  - scripts/ 3d ago
    - install-cursor-bridge.py 3.3KB 3d ago
    - generate_theme_catalog.py 16.1KB 3w ago
    - publish-startup-assets.sh 1.3KB 4w ago
    - package-startup.sh 4.9KB 4w ago
    - install-bridge.sh 7.6KB 4w ago
    - build-bridge.sh 7.3KB 4w ago
    - gui.sh 1.2KB 1mo ago
    - ci-tests.sh 1.6KB 1mo ago
    - validate-service.py 3.4KB 1mo ago
    - validate-pi-rpc.py 5.0KB 1mo ago
  - justfile 4.8KB 3d ago
  - experiments/ 3d ago
    - cursor-cloud/ 3d ago
  - THIRD_PARTY_NOTICES.md 1.3KB 3d ago
  - README.md 6.0KB 3d ago
  - Cargo.toml 3.2KB 3d ago
  - Cargo.lock 256.7KB 3d ago
  - .config/ 2w ago
    - nextest.toml 1.4KB 2w ago
  - pi/ 2w ago
    - skills/ 2w ago
    - extension.ts 231B 2w ago
  - package.json 256B 2w ago
  - native/ 1mo ago
    - bridge/ 4w ago
  - provider-usage-spec.md 21.7KB 1mo ago
  - provider-usage-research.md 53.4KB 1mo ago
  - .gitignore 50B 1mo ago
  - .claude/ 1mo ago
    - settings.local.json 160B 1mo ago
    - worktrees/ 1mo ago
    - launch.json 250B 1mo ago
  - .scratch/ 1mo ago
    - durable-sessions/ 1mo ago
  - AGENTS.md 3.1KB 1mo ago
  - .agents/ 1mo ago
    - skills/ 1mo ago
  - crates/ 1mo ago
    - ovrcr-runtime/ 3d ago
    - ovrcr-protocol/ 2w ago
    - ovrcr-tui/ 3w ago
    - ovrcr-terminal/ 1mo ago
  - LICENSE 1.0KB 1mo ago
  - .github/ 1mo ago
    - workflows/ 3d ago
  - .superpowers/ 2mo ago
    - sdd/ 1mo ago

SOURCE LOOKUP {"path": "src/cli/*.rs;src/report/*.rs"}
# src/
## report/
reporter.rs
cursor_cloud.rs
admission.rs
codex.rs
pi.rs
extension.rs
versions.rs
grok.rs
hermes.rs
cursor.rs
claude_metrics.rs
claude.rs
omp.rs
collector.rs
## cli/
resources.rs
output.rs
operator.rs
mod.rs
cursor_cloud.rs
args.rs
agent.rs
delegation_publisher.rs
delegation.rs
claude_delegation.rs
agent_setup.rs
codex_setup.rs
codex_delegation.rs
managed.rs
bridge.rs
startup_bridge.rs
startup.rs
report.rs
hermes.rs
startup_hooks.rs
cursor.rs

SOURCE LOOKUP {"path": "src", "pattern": "Agent|Run|prompt|model"}
# src/

## bin/
### ovrcr-gui.rs
 528|                    // Keep one Cmd-K intent across startup; never send it to
*529|                    // a setup prompt or make a setup choice on the user's behalf.
 530|                    self.palette_pending = false;
 531|                    if let Err(error) = terminal.send(b"\x07:") {
 532|                        self.error = Some(format!("{error:#}"));
...
 764|            );
*765|            decline_test_startup_prompt(app.terminal.as_mut().unwrap(), &mut declined)?;
 766|            // Real GUI frames continue while the restarted PTY reaches the
 767|            // Dashboard. Do not resend Cmd-K: its original intent must survive.
 768|            let mut output = context.run_ui(
...
 787|#[cfg(test)]
*788|fn decline_test_startup_prompt(
 789|    terminal: &mut Terminal,
 790|    declined: &mut std::collections::HashSet<String>,
 791|) -> anyhow::Result<()> {
...
 808|        .rsplit_once("OVRCR · startup")
*809|        .and_then(|(_, prompt)| prompt.lines().map(str::trim).find(|line| !line.is_empty()))
 810|        .unwrap_or("");
 811|    anyhow::ensure!(
 812|        matches!(
...
 841|        );
*842|        decline_test_startup_prompt(terminal, &mut declined)?;
 843|        std::thread::yield_now();
 844|    }
 845|}

## cli/
### agent.rs
*1|use super::{AppResult, RuntimeError, args::AgentCommand};
*2|use ovrcr::protocol::AgentProvider;
 3|use std::os::unix::process::ExitStatusExt;
 4|
 5|/// A managed Grok launch needs no hooks or configuration: nothing is reported and the
 6|/// retained history is a title source, so there is nothing to set up or diagnose.
*7|fn grok_needs_no_setup() -> RuntimeError {
*8|    RuntimeError::new(
 9|        ovrcr::protocol::ErrorCode::InvalidRequest,
 10|        "grok has no setup or doctor: `ovrcr agent run grok -- grok` retains the session history without hooks; resume is unavailable",
 11|    )
...
 13|
*14|fn provider(name: &str) -> AgentProvider {
*15|    AgentProvider::from_name(name).expect("clap value_parser accepts known provider names")
 16|}
 17|
*18|pub(super) fn run(command: AgentCommand) -> AppResult<()> {
 19|    let (name, argv) = match command {
*20|        AgentCommand::CloudPreview { account, recovery } => {
 21|            return super::cursor_cloud::preview(&account, recovery.as_deref());
 22|        }
*23|        AgentCommand::CloudAdmission => return super::cursor_cloud::admission(),
*24|        AgentCommand::CloudCreation => return super::cursor_cloud::creation(),
*25|        AgentCommand::Setup {
 26|            provider,
 27|            print: _,
 28|            settings,
...
 41|        }
*42|        AgentCommand::Doctor {
 43|            provider,
 44|            settings,
 45|            session,
...
 62|        }
*63|        AgentCommand::Run {
 64|            provider,
 65|            legacy_provider,
 66|            argv,
...
 99|                if let Some(report) = error.downcast_ref::<ovrcr::report::ReportError>() {
*100|                    RuntimeError::new(report.code(), report.to_string())
 101|                } else {
*102|                    RuntimeError::internal(error)
 103|                }
 104|            })?
 105|    };
...
 126|                ovrcr::config::RegistryPath::resolve()
*127|                    .map_err(RuntimeError::internal)?
 128|                    .0
 129|                    .into_os_string(),
 130|            ),
...
 159|    })
*160|    .map_err(RuntimeError::internal)?;
 161|    if let Some(signal) = status.signal() {
 162|        unsafe {
 163|            libc::signal(signal, libc::SIG_DFL);
### agent_setup.rs
*1|use super::{AppResult, RuntimeError};
 2|use ovrcr::protocol::ReporterHealth;
 3|use serde_json::{Value, json};
 4|use std::ffi::OsStr;
...
 225|            Err(error) => {
*226|                return Err(RuntimeError::internal(anyhow::anyhow!(
 227|                    "{}: {:#}",
 228|                    path.display(),
 229|                    error
...
 451|    })();
*452|    let value = result.map_err(|_| RuntimeError::internal(anyhow::anyhow!("Cannot compose Claude JSON: settings unreadable, invalid, oversized, or incompatible hook structure")))?;
 453|    println!(
 454|        "{}",
*455|        serde_json::to_string_pretty(&value).map_err(RuntimeError::internal)?
 456|    );
 457|    Ok(())
 458|}
...
 595|        (Some(_), Ok(value)) => {
*596|            let executable = binary().map_err(RuntimeError::internal)?;
 597|            issues.extend(configuration_issues(&value, &executable));
 598|            if issues.is_empty() {
 599|                "supplied_file_supported"
...
 605|    if path.is_none() || !issues.is_empty() {
*606|        remediation.push("Run agent setup claude --print with the intended --settings file, review the composition, and verify synchronous hooks in all effective settings sources.".into());
 607|    }
 608|    let requested = session.or_else(|| std::env::var("OVRCR_SESSION_ID").ok()?.parse().ok());
 609|    let mut binding = Value::Null;
...
 661|        "remediation":remediation
*662|    })).map_err(RuntimeError::internal)?);
 663|    Ok(())
 664|}
 665|
...
 679|    if blocked != 0 {
*680|        return Err(RuntimeError::internal(std::io::Error::from_raw_os_error(
 681|            blocked,
 682|        )));
 683|    }
...
 687|    if restored != 0 {
*688|        return Err(RuntimeError::internal(std::io::Error::from_raw_os_error(
 689|            restored,
 690|        )));
 691|    }
### args.rs
 1|use clap::{ArgGroup, Args, Parser, Subcommand};
*2|use ovrcr::session::AgentActivity;
 3|use std::ffi::OsString;
 4|use std::path::PathBuf;
 5|
...
 77|    },
*78|    /// Run a native agent with invocation supervision.
*79|    Agent {
 80|        #[command(subcommand)]
*81|        command: AgentCommand,
 82|    },
*83|    /// Run the server in the foreground (normally started on demand).
 84|    Server,
 85|    /// Manage scheduled Pi tasks.
 86|    #[command(visible_alias = "tasks")]
...
 89|    #[command(visible_alias = "runs")]
*90|    Run(ovrcr::task_cli::RunArgs),
 91|    /// Install, start, stop, or remove the background service.
 92|    Service(ovrcr::service::ServiceArgs),
 93|    #[command(name = "__task-runner", hide = true)]
*94|    TaskRunner {
 95|        run_dir: PathBuf,
 96|        pi_executable: PathBuf,
 97|    },
 98|    #[command(name = "__agent-collector", hide = true)]
*99|    AgentCollector,
 100|    /// Point existing managed reporter commands at this binary. `just run` calls this
 101|    /// after installing ~/.local/bin/ovrcr. It does not add hooks or change trust.
 102|    #[command(name = "__retarget-hooks", hide = true)]
...
 183|    Status { provider: OperatorProviderArg },
*184|    /// List available ChatGPT models or the supported Grok catalog.
 185|    Models { provider: OperatorProviderArg },
 186|    /// Remove this instance's provider credential; report remote revocation separately.
 187|    Logout { provider: OperatorProviderArg },
...
 216|    /// Passive native Cursor startup identity hook.
*217|    CursorAgent {
 218|        #[arg(long, required = true)]
 219|        stdin: bool,
 220|    },
...
 229|        #[arg(long, value_parser = parse_activity_state)]
*230|        state: AgentActivity,
 231|        #[arg(long)]
 232|        sequence: Option<u64>,
 233|    },
...
 484|
*485|pub(super) fn parse_activity_state(value: &str) -> std::result::Result<AgentActivity, String> {
 486|    match value {
*487|        "unknown" => Ok(AgentActivity::Unknown),
*488|        "idle" => Ok(AgentActivity::Idle),
*489|        "busy" => Ok(AgentActivity::Busy),
*490|        "waiting-input" => Ok(AgentActivity::WaitingInput),
*491|        "error" => Ok(AgentActivity::Error),
*492|        "response-ready" => Ok(AgentActivity::ResponseReady),
 493|        _ => {
 494|            Err("must be one of: unknown, idle, busy, waiting-input, response-ready, error".into())
 495|        }
...
 499|#[derive(Subcommand)]
*500|pub(super) enum AgentCommand {
 501|    #[command(hide = true)]
 502|    CloudPreview {
 503|        account: String,
...
 531|    #[command(
*532|        long_about = "Run a native command with invocation supervision. cursor-cloud requires one named account after --, positive managed admission and macOS Keychain; it validates a pinned SDK Bridge preview of the published workspace origin/HEAD and explicit model and excludes local edits. Explicit prepare retains a local handle, then prompts Send to that same conversation with durable claims and exact run receipts. Busy input requires manual resubmission. An interrupted known-run transport may make a... [col 23]
 533|    )]
### bridge.rs
 1|//! Fixed-argument callback transport. It deliberately bypasses connect_or_start.
*2|use super::RuntimeError;
 3|use ovrcr::protocol::{self, BridgeNavigationResult, BridgeNavigationTicket, Request, Response};
 4|use std::io::{self, Read, Write};
 5|use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
...
 14|    let deadline = Instant::now() + CALLBACK_BUDGET;
*15|    let bytes = read_ticket(deadline).map_err(RuntimeError::internal)?;
 16|    let ticket: BridgeNavigationTicket =
*17|        serde_json::from_slice(&bytes).map_err(RuntimeError::internal)?;
 18|    if !ticket.validate()
 19|        || !std::env::current_exe().ok().is_some_and(|path| {
 20|            compatible_executable(
...
 28|    {
*29|        return Err(RuntimeError::new(
 30|            protocol::ErrorCode::InvalidRequest,
 31|            "invalid notification navigation ticket",
 32|        ));
...
 35|        .unwrap_or_else(|_| BridgeNavigationResult::ignored());
*36|    super::output::print_json(&serde_json::to_value(result).map_err(RuntimeError::internal)?)
 37|}
 38|
 39|pub(super) fn owner() -> super::AppResult<()> {
 40|    let deadline = Instant::now() + CALLBACK_BUDGET;
*41|    let bytes = read_ticket(deadline).map_err(RuntimeError::internal)?;
 42|    let call: protocol::BridgeOwnerCall =
*43|        serde_json::from_slice(&bytes).map_err(RuntimeError::internal)?;
 44|    let context = &call.owner.context;
 45|    let valid = call.owner.validate();
 46|    owner_diagnostic("ticket-validation", deadline, None, Ok(valid));
...
 64|    if !compatible {
*65|        return Err(RuntimeError::new(
 66|            protocol::ErrorCode::InvalidRequest,
 67|            "invalid Bridge owner context",
 68|        ));
...
 71|        .unwrap_or_else(|_| protocol::BridgeOwnerResult::unavailable());
*72|    super::output::print_json(&serde_json::to_value(result).map_err(RuntimeError::internal)?)
 73|}
 74|
 75|fn connect_and_owner(
...
 618|            session: protocol::SessionId(1),
*619|            run: protocol::SessionRunId(1),
 620|        };
 621|        let start = Instant::now();
 622|        assert!(connect_and_navigate(ticket, start + Duration::from_millis(25)).is_err());
### claude_delegation.rs
 1|//! Compose the positively assigned Claude policy with the existing Reporter.
*2|use super::{AppResult, RuntimeError};
 3|use anyhow::{Context, Result, ensure};
 4|use ovrcr_runtime::claude_delegation::{self, ARTIFACT_ENV};
 5|use std::{
...
 52|        .map(|()| true)
*53|        .map_err(RuntimeError::internal)
 54|}
 55|
 56|fn prepare_assigned(argv: &[OsString]) -> Result<()> {
...
 86|        contract.provider == "claude"
*87|            && contract.model_provider == "native"
 88|            && artifacts.file_name().and_then(|name| name.to_str())
 89|                == Some(contract.request.as_str()),
 90|        "unsupported or foreign Claude research policy"
...
 93|    ensure!(
*94|        argv == claude_delegation::launch_argv(&executable, &contract.model, &artifacts),
 95|        "native Claude argv differs from assigned read-only policy"
 96|    );
 97|    ensure!(
### codex_delegation.rs
 1|//! Only the Server-selected private Codex child profile is composed here.
*2|use super::{AppResult, RuntimeError};
 3|use anyhow::{Context, Result, ensure};
 4|use ovrcr_runtime::codex_delegation::{self, ARTIFACT_ENV, PROFILE};
 5|use std::{
...
 70|    }
*71|    prepare_private(argv).map_err(RuntimeError::internal)
 72|}
 73|fn prepare_private(argv: &[OsString]) -> Result<()> {
 74|    let artifacts = PathBuf::from(
...
 103|    ensure!(
*104|        contract.provider == "codex" && contract.model_provider == "openai",
 105|        "unsupported Codex policy contract"
 106|    );
 107|    contract.validate().map_err(anyhow::Error::msg)?;
...
 114|        native.into_os_string(),
*115|        "--model".into(),
*116|        contract.model.into(),
 117|        "--profile".into(),
 118|        PROFILE.into(),
 119|        "--ask-for-approval".into(),
### codex_setup.rs
 1|use super::{
*2|    AppResult, RuntimeError,
 3|    agent_setup::{binary, owned_reporter, public_health_reason, quote, with_version_probe},
 4|};
*5|use ovrcr::protocol::{AgentProvider, ReporterHealth};
 6|use serde_json::json;
 7|use std::{ffi::OsStr, io::Read, path::Path};
 8|use toml::Value;
...
 19|];
*20|const REQUIREMENTS: &str = "Requires synchronous direct-exec command hooks; no Codex CLI version is required or refused. Review and trust these hooks in native Codex before the first tracked prompt; an initial prompt supplied during hook review may run untracked. Configuration presence does not prove hook trust or delivery. Hooks exit successfully with empty stdout (native no-op), never an approval decision. Launch through the Dashboard agent picker or `ovrcr agent run codex -- codex` inside an OVRCR ter... [col 192]
*21|const FORMS: &str = "Fresh interactive codex (executable basename codex): optional --no-daemon, --strict-config, --no-alt-screen, --approve-for-me, --full-auto, --dangerously-bypass-hook-trust; separate-token --model/-m, --profile/-p, --sandbox/-s, --ask-for-approval/-a, --cd/-C followed by a nonempty value not starting with '-'; at most one prompt (use -- before a prompt matching a subcommand). Exact resume: `codex resume UUID` with the same optional flags and no prompt (UUID must be the exact canonical... [col 212]
 22|
 23|fn settings(path: Option<&Path>) -> anyhow::Result<Value> {
 24|    let Some(path) = path else {
...
 84|        format!(
*85|            "Codex reporting hooks missing in {shown}: run `ovrcr agent setup codex --print --settings {shown}` and merge the result; trust hooks in native Codex before the first tracked prompt"
 86|        )
 87|    })
 88|}
...
 264|    // Do not echo parser diagnostics: malformed supplied configuration may contain secrets.
*265|    let value = result.map_err(|_| RuntimeError::internal(anyhow::anyhow!("Cannot compose Codex TOML: settings unreadable, invalid, oversized, or incompatible hook structure")))?;
 266|    print!("{value}");
 267|    eprintln!(
 268|        "{REQUIREMENTS}\n{FORMS}\nNo file was written. Review the printed TOML and explicitly merge it into the intended Codex config.toml. Unrelated native values, handler order, comments and formatting are preserved. OVRCR-owned reporters are repaired to one unfiltered synchronous reporter per event. Do not redirect onto the input file. Preserve permission, approval and trust settings; never copy trust hashes from an example. Removal: remove only the exact added direct-exec reporter handlers; preserve ...
...
 296|        (Some(_), Ok(value)) => {
*297|            let command = command().map_err(RuntimeError::internal)?;
 298|            for event in HOOKS {
 299|                if !value
 300|                    .get("hooks")
...
 337|                        binding = json!({"provider":agent.binding.provider,"generation":agent.binding.generation});
*338|                        if agent.binding.provider != AgentProvider::Codex {
 339|                            guidance.push_str("This session is bound to another reporting provider; select a Codex session before diagnosing Codex reporting. ");
 340|                            "provider_mismatch"
 341|                        } else {
...
 368|        "remediation":guidance
*369|    })).map_err(RuntimeError::internal)?);
 370|    Ok(())
 371|}
 372|
### cursor.rs
 1|//! Cursor diagnostics do not run native code or modify provider configuration.
*2|use super::{AppResult, RuntimeError};
*3|use ovrcr::protocol::{AgentProvider, ErrorCode, SessionKind};
 4|use serde_json::json;
 5|use std::{ffi::OsStr, os::unix::fs::PermissionsExt, path::Path};
 6|
...
 8|    if settings.is_some() {
*9|        return Err(RuntimeError::new(
 10|            ErrorCode::InvalidRequest,
 11|            "Cursor startup reporting uses a temporary local plugin; --settings is unsupported",
 12|        ));
...
 19|    println!(
*20|        "Cursor startup identity uses a temporary local plugin, with no persistent settings changes. Configure Cursor separately; pick cursor-agent or run `ovrcr agent run cursor-agent -- cursor-agent` inside an OVRCR terminal. Fresh interactive launches (optionally --model VALUE) inspect the installed CLI's --help for local plugin support and validate its native startup identity at runtime. Activity, Ready/Unread, Input, metrics, generated titles and recovery are unavailable. See docs/cursor-harness.md ... [col 270]
 21|    );
 22|    Ok(())
 23|}
...
 51|                    if session.kind
*52|                        != (SessionKind::Agent {
 53|                            name: "cursor-agent".into(),
 54|                        }) =>
 55|                {
...
 58|                Some(session) => match &session.agent {
*59|                    Some(agent) if agent.binding.provider == AgentProvider::Cursor => {
 60|                        binding = json!({"provider":agent.binding.provider,"generation":agent.binding.generation,"health":agent.health.state});
 61|                        "bound"
 62|                    }
...
 77|        "guidance":"Fresh interactive cursor-agent or explicit agent alias only; launch probes --help for local plugin support with a bounded deadline, then validates the native startup hook. No release whitelist. Resume/continue/headless/management/other options remain native with reporting unavailable. Doctor never runs the provider; see docs/cursor-harness.md."
*78|    })).map_err(RuntimeError::internal)?);
 79|    Ok(())
 80|}
### cursor_cloud.rs
*1|//! Managed Cursor cloud admission and terminal prompt entry.
*2|use super::{AppResult, RuntimeError};
*3|use ovrcr::protocol::{AgentProvider, CursorCloudAccount, ErrorCode};
 4|use std::{
 5|    ffi::OsString,
 6|    path::Path,
...
 10|
*11|fn invalid(message: impl Into<String>) -> RuntimeError {
*12|    RuntimeError::new(ErrorCode::InvalidRequest, message)
 13|}
 14|fn accounts(document: Option<&Path>) -> AppResult<Vec<CursorCloudAccount>> {
 15|    let home = ovrcr::config::RegistryPath::resolve()
*16|        .map_err(RuntimeError::internal)?
 17|        .0;
 18|    let report = match document {
 19|        Some(path) => ovrcr_runtime::settings::load_document(&home, path),
...
 55|        "runtime_checked":false, "release_ready":false,
*56|        "guidance":"Use just install-release, or python3 -I scripts/install-cursor-bridge.py [OFFICIAL_ARCHIVE] from the checkout; set each profile's absolute bridge_archive to the printed path. Passive SHA256 only; bridge/protocol/capabilities are checked on managed launch. Correlated cloud terminal questions/approvals, uncertain CreateAgent/Send Retry and fresh-process active restoration and fresh-handle old-offset reuse are unavailable and not planned; same-owned-bridge ObserveRun and completed exact-... [col 340]
 57|    });
 58|    println!(
 59|        "{}",
 60|        serde_json::json!({"provider":"cursor-cloud", "platform_available":cfg!(target_os="macos"), "accounts":accounts, "bridge_dependency":dependency,
*61|        "guidance":"macOS only. Configure named Keychain references, explicit model and repository/owner policy in dashboard.toml. Choose cursor-cloud:ACCOUNT in ordinary Agent creation. Preview validates origin and published HEAD; local edits are excluded. Profiles select credentials, not attested billing identities. Type prepare in the validated preview to retain a local SDK handle. Send prompts on the retained conversation; /cancel persists cancellation intent before exact live CancelRun, cloud-discar... [col 79]
 62|    );
 63|    Ok(())
 64|}
...
 82|        .ok_or_else(|| invalid("invalid account name"))?;
*83|    let lease = ovrcr::report::reserve_invocation_for(AgentProvider::CursorCloud)
*84|        .map_err(RuntimeError::internal)?
 85|        .ok_or_else(|| invalid("cursor-cloud requires positive managed admission"))?;
*86|    let executable = std::env::current_exe().map_err(RuntimeError::internal)?;
 87|    let mut native = vec![
 88|        executable.into_os_string(),
 89|        "agent".into(),
...
 102|    })
*103|    .map_err(RuntimeError::internal)?;
 104|    use std::os::unix::process::ExitStatusExt;
 105|    if let Some(signal) = status.signal() {
 106|        std::process::exit(128 + signal);
...
 115|    ovrcr::report::send_cloud_admission(Instant::now() + Duration::from_secs(1))
*116|        .map_err(RuntimeError::internal)
 117|}
 118|
 119|pub(super) fn creation() -> AppResult<()> {
...
 124|        .read_to_end(&mut bytes)
*125|        .map_err(RuntimeError::internal)?;
 126|    if bytes.len() > 65536 {
 127|        return Err(invalid("creation frame exceeds bound"));
 128|    }
 129|    ovrcr::report::send_cloud_creation(&bytes, Instant::now() + Duration::from_secs(5))
*130|        .map_err(RuntimeError::internal)
 131|}
 132|
 133|pub(super) fn preview(account: &str, recovery: Option<&str>) -> AppResult<()> {
 134|    // A strict descendant proves membership in the currently supervised native tree.
*135|    let status = Command::new(std::env::current_exe().map_err(RuntimeError::internal)?)
 136|        .args(["agent", "cloud-admission"])
 137|        .stdin(Stdio::null())
 138|        .stdout(Stdio::null())
...
 140|        .status()
*141|        .map_err(RuntimeError::internal)?;
 142|    if !status.success() {
 143|        return Err(invalid("cursor-cloud managed admission unavailable"));
 144|    }
...
 153|        .ok_or_else(|| invalid("named cursor-cloud account is not configured"))?;
*154|    let directory = std::env::current_dir().map_err(RuntimeError::internal)?;
 155|    let recovery = recovery
 156|        .map(serde_json::from_str::<ovrcr::protocol::CloudRecovery>)
 157|        .transpose()
*158|        .map_err(RuntimeError::internal)?;
 159|    ovrcr_runtime::cursor_cloud::preview(&profile, &directory, recovery.as_ref(), |command| {
 160|        use std::io::Write;
 161|        let mut child = Command::new(std::env::current_exe()?).args(["agent", "cloud-creation"])
### delegation.rs
*1|use super::{AppResult, RuntimeError};
 2|use ovrcr::protocol::{DelegationRequest, ErrorCode, Request, Response, WorkflowRequest};
 3|use std::io::Read;
 4|
...
 9|        .read_to_end(&mut bytes)
*10|        .map_err(RuntimeError::internal)?;
 11|    if bytes.len() > 524_288 {
*12|        return Err(RuntimeError::new(
 13|            ErrorCode::InvalidRequest,
 14|            "delegation request is too large",
 15|        ));
...
 20|            serde_json::from_slice::<DelegationRequest>(&bytes).map_err(|_| {
*21|                RuntimeError::new(ErrorCode::InvalidRequest, "invalid delegation request")
 22|            })?,
 23|        )),
 24|    };
...
 27|        Response::WorkflowRecovery(recovery) => {
*28|            super::print_json(&serde_json::to_value(recovery).map_err(RuntimeError::internal)?)
 29|        }
 30|        Response::WorkflowParents(parents) => {
*31|            super::print_json(&serde_json::to_value(parents).map_err(RuntimeError::internal)?)
 32|        }
 33|        Response::Workflows(workflows) => {
*34|            super::print_json(&serde_json::to_value(workflows).map_err(RuntimeError::internal)?)
 35|        }
 36|        Response::Delegation(snapshot) => {
*37|            super::print_json(&serde_json::to_value(snapshot).map_err(RuntimeError::internal)?)
 38|        }
 39|        Response::DelegationObservation(batch) => {
*40|            super::print_json(&serde_json::to_value(batch).map_err(RuntimeError::internal)?)
 41|        }
 42|        Response::DelegationCapabilities(capabilities) => {
*43|            super::print_json(&serde_json::to_value(capabilities).map_err(RuntimeError::internal)?)
 44|        }
 45|        Response::AssignedRead(read) => {
*46|            super::print_json(&serde_json::to_value(read).map_err(RuntimeError::internal)?)
 47|        }
*48|        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
*49|        _ => Err(RuntimeError::new(
 50|            ErrorCode::Internal,
 51|            "unexpected delegation response",
 52|        )),
### delegation_publisher.rs
 2|//! Native clients use `ovrcr delegation-publisher` over bounded stdio JSON-RPC.
*3|use super::{AppResult, RuntimeError};
 4|use ovrcr::protocol::{
 5|    AssignedRead, AssignedReadResult, AssignmentIdentity, AssignmentQuestion, DelegationAuth,
 6|    DelegationRequest, PublicationIdentity, PublicationKind, Request, ResearchSubmission, Response,
...
 406|         "inputSchema":object(json!({"handle":id,"submission":submission})),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
*407|        {"name":QUESTION,"description":"Publish this child's exact model-originated question before waiting. Native login/approval prompts are separate; never include credentials. Publication grants no approval.",
 408|         "inputSchema":object(json!({"handle":id,"question":question})),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
 409|    ]})
 410|}
...
 591|    )
*592|    .map_err(RuntimeError::internal)
 593|}
 594|
 595|#[cfg(test)]
...
 643|                session: Some(SessionId(u64::MAX)),
*644|                run: Some(ovrcr::protocol::SessionRunId(u64::MAX)),
 645|            },
 646|            result_version: u32::MAX,
 647|            control_epoch: u64::MAX,
### hermes.rs
 1|//! Hermes reporting through native shell hooks, plus process supervision.
*2|use super::{AppResult, RuntimeError};
 3|use ovrcr::protocol::{ErrorCode, SessionKind};
 4|use serde_json::json;
 5|use std::ffi::OsStr;
...
 10|    if settings.is_some() {
*11|        return Err(RuntimeError::new(
 12|            ErrorCode::InvalidRequest,
 13|            "Hermes supervision uses no OVRCR provider settings; --settings is unsupported",
 14|        ));
...
 36|        .ok_or_else(|| {
*37|            RuntimeError::new(
 38|                ErrorCode::InvalidRequest,
 39|                "Hermes home is unavailable; set HOME or HERMES_HOME",
 40|            )
...
 42|    if !home.is_absolute() {
*43|        return Err(RuntimeError::new(
 44|            ErrorCode::InvalidRequest,
 45|            "Hermes home must be absolute",
 46|        ));
...
 51|fn hook_command() -> AppResult<String> {
*52|    let executable = std::env::current_exe().map_err(RuntimeError::internal)?;
 53|    let executable = executable.to_str().ok_or_else(|| {
*54|        RuntimeError::new(
 55|            ErrorCode::InvalidRequest,
 56|            "ovrcr path is not UTF-8; Hermes hooks need a UTF-8 command",
 57|        )
...
 59|    if executable.contains('\'') || executable.contains('\n') {
*60|        return Err(RuntimeError::new(
 61|            ErrorCode::InvalidRequest,
 62|            "ovrcr path cannot be quoted as a Hermes hook command",
 63|        ));
...
 74|    }) {
*75|        return Err(RuntimeError::new(
 76|            ErrorCode::Conflict,
 77|            "Hermes config.yaml already has a hooks: key; review it and add the `ovrcr report hermes --stdin` command for on_session_start, pre_api_request, post_api_request, post_llm_call, on_session_end, pre_approval_request and post_approval_response",
 78|        ));
...
 83|    }
*84|    std::fs::create_dir_all(home).map_err(RuntimeError::internal)?;
 85|    let mut file = std::fs::OpenOptions::new()
 86|        .create(true)
 87|        .append(true)
 88|        .open(&config)
*89|        .map_err(RuntimeError::internal)?;
 90|    use std::io::Write;
 91|    file.write_all(block.as_bytes())
*92|        .map_err(RuntimeError::internal)?;
 93|    let allowlist = home.join("shell-hooks-allowlist.json");
 94|    let mut document = std::fs::read_to_string(&allowlist)
 95|        .ok()
...
 103|    let Some(approvals) = approvals else {
*104|        return Err(RuntimeError::new(
 105|            ErrorCode::Internal,
 106|            "Hermes hook allowlist is not an object",
 107|        ));
...
 122|        &allowlist,
*123|        serde_json::to_vec_pretty(&document).map_err(RuntimeError::internal)?,
 124|    )
*125|    .map_err(RuntimeError::internal)?;
 126|    Ok(())
 127|}
 128|
...
 152|    println!(
*153|        "Installed Hermes reporting hooks in {}. They forward session id, turn id, model, approval id and token counts to OVRCR. Prompts, transcripts and tool arguments are dropped. Ready/Unread follows post_llm_call. Approval prompts follow pre_approval_request. Token totals follow post_api_request. Cost is not in that summary, so cost stays unavailable. Titles are read from the session row in state.db. Resume is `hermes --resume <id>`. Questions that are not approval hooks stay unavailable. Re-run setu... [col 84]
 154|        home.display()
 155|    );
 156|    Ok(())
...
 185|                    if session.kind
*186|                        == (SessionKind::Agent {
 187|                            name: "hermes".into(),
 188|                        }) =>
 189|                {
...
 220|        "session_status": session_status, "lifecycle": lifecycle,
*221|        "guidance": "Run `ovrcr agent setup hermes` once per Hermes profile (set HERMES_HOME). Hooks report session id, turn activity, approval ids and token counts. Cost and non-approval questions stay unavailable. Titles come from the session row. Resume is `hermes --resume <session id>`. Doctor does not inspect credentials or run Hermes. See docs/hermes-harness.md.",
 222|    });
 223|    println!(
 224|        "{}",
*225|        serde_json::to_string_pretty(&report).map_err(RuntimeError::internal)?
 226|    );
 227|    Ok(())
 228|}
### managed.rs
 4|//! allowlist.
*5|use super::{AppResult, RuntimeError};
*6|use ovrcr::protocol::{AgentActivity, ReporterHealth};
 7|use serde_json::{Value, json};
 8|use std::ffi::OsStr;
 9|
...
 15|    pub reporting: &'static str,
*16|    /// Native tool-approval prompts as Input requests.
 17|    pub approvals: &'static str,
*18|    /// Agent questions as Input requests. `available_tool_lifetime` means the request
 19|    /// spans the asking tool's execution, not the dialog's visibility: it opens a moment
 20|    /// before the dialog and covers a question queued behind another one.
 21|    pub questions: &'static str,
...
 50|/// is a reporter that is gone, and only a fresh managed launch brings it back.
*51|fn paused(provider: &Managed, agent: &ovrcr::protocol::AgentSnapshot) -> bool {
 52|    agent.health.state == ReporterHealth::Unavailable
 53|        && agent
 54|            .health
...
 164|                            "input_requests": agent.input_requests.iter().map(|request| request.kind).collect::<Vec<_>>(),
*165|                            "work_seen": matches!(activity, Some(AgentActivity::Busy | AgentActivity::ResponseReady | AgentActivity::Error | AgentActivity::WaitingInput)),
 166|                            "health": agent.health.state,
 167|                            "unread": session.unread.is_some(),
 168|                        });
...
 175|                                format!(
*176|                                    "Run /ovrcr-reattach in {}; the next prompt alone will not recover it",
 177|                                    provider.display
 178|                                )
 179|                            } else if provider.recovery == "available" {
 180|                                format!(
*181|                                    "Run /ovrcr-reattach in {}, or send the next prompt — the next session boundary recovers reporting as a fresh generation",
 182|                                    provider.display
 183|                                )
 184|                            } else {
*185|                                "Send the next prompt — the next session boundary recovers reporting as a fresh generation".to_owned()
 186|                            };
 187|                            remediation.push(format!(
 188|                                "Reporting is paused for this invocation ({reason}): the source data this reporter delivered is no longer certain, and nothing is being applied. The native session is untouched. {control}.",
...
 223|        "{}",
*224|        serde_json::to_string_pretty(&report).map_err(RuntimeError::internal)?
 225|    );
 226|    Ok(())
 227|}
...
 235|        use ovrcr::protocol::{
*236|            AgentBinding, AgentProvider, AgentSnapshot, HealthSample, ReporterHealth,
 237|        };
*238|        let snapshot = |reason: Option<&str>| AgentSnapshot {
*239|            binding: AgentBinding {
*240|                provider: AgentProvider::Pi,
 241|                invocation: "inv".into(),
 242|                conversation: "conv".into(),
 243|                generation: 1,
### mod.rs
 39|#[derive(Debug)]
*40|struct RuntimeError {
 41|    code: ErrorCode,
 42|    message: String,
 43|}
 44|
*45|type AppResult<T> = std::result::Result<T, RuntimeError>;
 46|
*47|impl RuntimeError {
 48|    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
 49|        Self {
 50|            code,
...
 73|    let Some(command) = cli.command else {
*74|        let paths = ServerPaths::resolve().map_err(RuntimeError::internal)?;
 75|        return run_dashboard(
*76|            startup::run(&paths).map_err(RuntimeError::internal)?,
 77|            agent_setup::boot_hook_warning(),
 78|        )
*79|        .map_err(RuntimeError::internal);
 80|    };
 81|    match command {
 82|        Command::Operator { command } => operator::run(command, json_output),
...
 88|        } => bridge::owner(),
*89|        Command::AgentCollector => {
*90|            ovrcr::report::collector::run_helper().map_err(RuntimeError::internal)
 91|        }
 92|        Command::RetargetHooks { settings } => {
*93|            let executable = agent_setup::binary().map_err(RuntimeError::internal)?;
 94|            let files = if settings.is_empty() {
 95|                agent_setup::default_hook_files()
 96|            } else {
...
 100|        }
*101|        Command::Agent { command } => agent::run(command),
 102|        Command::Delegation => delegation::run(),
 103|        Command::DelegationPublisher { read_handle } => delegation_publisher::run(read_handle),
 104|        Command::Task(args) => {
*105|            ovrcr::task_cli::run_task(args.command, json_output).map_err(RuntimeError::internal)
 106|        }
*107|        Command::Run(args) => {
*108|            ovrcr::task_cli::run_run(args.command, json_output).map_err(RuntimeError::internal)
 109|        }
 110|        Command::Service(args) => {
*111|            ovrcr::service::run(args.command, json_output).map_err(RuntimeError::internal)
 112|        }
*113|        Command::TaskRunner {
 114|            run_dir,
 115|            pi_executable,
 116|        } => {
*117|            ovrcr::task_runner::supervise(&run_dir, &pi_executable).map_err(RuntimeError::internal)
 118|        }
 119|        Command::Server => {
*120|            ovrcr::service::load_environment_file().map_err(RuntimeError::internal)?;
 121|            run_server(
*122|                ServerPaths::resolve().map_err(RuntimeError::internal)?,
*123|                RegistryPath::resolve().map_err(RuntimeError::internal)?.0,
 124|            )
*125|            .map_err(RuntimeError::internal)
 126|        }
 127|        Command::List => {
### operator.rs
*1|use super::{AppResult, RuntimeError, args::OperatorCommand, output::print_json};
 2|use ovrcr::operator::{
 3|    auth::{AuthCancel, AuthManager},
 4|    login::BrowserRedirect,
...
 7|    OperatorConversationId, OperatorProvider, OperatorTurnId, OperatorTurnIdentity, SessionId,
*8|    SessionRunId,
 9|};
 10|use serde_json::json;
 11|
...
 23|            session: SessionId(session),
*24|            run: SessionRunId(run),
 25|        })
*26|        .map_err(RuntimeError::internal);
 27|    }
 28|    let instance = ovrcr::config::RegistryPath::resolve()
*29|        .map_err(RuntimeError::internal)?
 30|        .0;
*31|    std::fs::create_dir_all(&instance).map_err(RuntimeError::internal)?;
*32|    let auth = AuthManager::new(&instance).map_err(RuntimeError::internal)?;
 33|    let runtime = tokio::runtime::Builder::new_current_thread()
 34|        .enable_all()
 35|        .build()
*36|        .map_err(RuntimeError::internal)?;
 37|    runtime.block_on(async {
 38|        let cancel = AuthCancel::default();
 39|        match command {
...
 41|                let provider: OperatorProvider = provider.into();
*42|                let status = auth.status(provider).await.map_err(RuntimeError::internal)?;
 43|                if json_output {
 44|                    print_json(&json!({"provider": provider.as_str(), "signed_in":status.signed_in,
 45|                        "needs_refresh":status.needs_refresh, "plan_usage":status.plan_usage,
...
 54|                let provider: OperatorProvider = provider.into();
*55|                let models = auth.models(provider, &cancel).await.map_err(RuntimeError::internal)?;
*56|                if json_output { print_json(&json!({"provider":provider.as_str(), "models": models})) }
*57|                else { for model in models { println!("{}\t{}", model.id, model.display_name); } Ok(()) }
 58|            }
 59|            OperatorCommand::Logout { provider } => {
*60|                let result = auth.logout(provider.into(), &cancel).await.map_err(RuntimeError::internal)?;
 61|                if json_output { print_json(&json!({"signed_in":false, "remote_revocation_confirmed": result.remote_revocation_confirmed})) }
 62|                else { println!("Signed out locally. Remote revocation {}.", if result.remote_revocation_confirmed { "confirmed" } else { "unconfirmed" }); Ok(()) }
 63|            }
...
 66|                    OperatorProvider::ChatGpt => {
*67|                        let login = auth.begin_chatgpt().await.map_err(RuntimeError::internal)?;
*68|                        let redirect = BrowserRedirect::new(login.authorization_url()).await.map_err(RuntimeError::internal)?;
 69|                        eprintln!("Complete ChatGPT subscription sign-in in your browser.");
*70|                        redirect.open().await.map_err(RuntimeError::internal)?;
*71|                        auth.finish_chatgpt(login, &cancel).await.map_err(RuntimeError::internal)?;
 72|                    }
 73|                    OperatorProvider::Grok => {
*74|                        let login = auth.begin_grok(&cancel).await.map_err(RuntimeError::internal)?;
 75|                        eprintln!("Enter code {} at {}", login.user_code, login.verification_uri);
 76|                        // Use the same secret-free launcher path; no provider URL enters argv.
*77|                        let redirect = BrowserRedirect::new(&login.verification_uri).await.map_err(RuntimeError::internal)?;
*78|                        redirect.open().await.map_err(RuntimeError::internal)?;
### output.rs
 3|use ovrcr::protocol::Response;
*4|use ovrcr::session::{AgentActivity, SessionPhase, SessionSummary};
 5|use serde_json::{Value, json};
 6|use std::path::Path;
 7|
*8|use super::{AppResult, RuntimeError, request_started, request_without_start, unexpected_response};
 9|
 10|pub(super) fn mutate_started(
 11|    request: ovrcr::protocol::Request,
...
 32|        }
*33|        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
 34|        response => Err(unexpected_response(response)),
 35|    }
 36|}
...
 75|    let (phase, exit_code, exit_signal) = match &session.phase {
*76|        SessionPhase::Running => ("running", Value::Null, Value::Null),
 77|        SessionPhase::Paused => ("paused", Value::Null, Value::Null),
 78|        SessionPhase::Exited { code, signal } => (
 79|            "exited",
...
 87|        ovrcr::protocol::SessionKind::Terminal => json!("terminal"),
*88|        ovrcr::protocol::SessionKind::Agent { name } => json!({"agent": name}),
 89|    };
 90|    json!({
 91|        "scope": session.scope,
...
 106|        "activity": match session.activity {
*107|            AgentActivity::Unknown => "unknown",
*108|            AgentActivity::Idle => "idle",
*109|            AgentActivity::Busy => "busy",
*110|            AgentActivity::WaitingInput => "waiting_input",
*111|            AgentActivity::Error => "error",
*112|            AgentActivity::ResponseReady => "response_ready",
 113|        },
 114|        "exit_code": exit_code,
 115|        "exit_signal": exit_signal,
...
 267|        "{}",
*268|        serde_json::to_string(value).map_err(RuntimeError::internal)?
 269|    );
 270|    Ok(())
 271|}
 272|
*273|pub(super) fn print_runtime_error(error: &RuntimeError, json_output: bool) {
 274|    if json_output {
 275|        let value = json!({
 276|            "error": {
...
 293|    match response {
*294|        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
 295|        Response::CreatedSession(summary) => {
 296|            println!("{}", summary.id.0);
 297|            Ok(())
...
 338|                        let phase = match session.phase {
*339|                            SessionPhase::Running => "running",
 340|                            SessionPhase::Paused => "paused",
 341|                            SessionPhase::Exited { .. } => "exited",
 342|                            SessionPhase::Stopped => "stopped",
...
 405|    use ovrcr::protocol::{
*406|        OperatorConversationId, SessionId, SessionKind, SessionRunId, SessionScope,
 407|    };
 408|
 409|    fn summary(scope: SessionScope) -> SessionSummary {
...
 414|            cwd: "/fixture".into(),
*415|            run: SessionRunId(2),
 416|            kind: SessionKind::Terminal,
 417|            recovery: None,
 418|            project: "fixture".into(),
...
 424|            phase: SessionPhase::Stopped,
*425|            activity: AgentActivity::Idle,
 426|            agent: None,
 427|            agent_epoch: 0,
 428|            unread: None,
### report.rs
 3|};
*4|use ovrcr::protocol::{AgentUpdate, ErrorCode};
 5|use ovrcr::report as app_report;
 6|use std::time::{Duration, Instant};
 7|
 8|use super::args::ReportCommand;
*9|use super::{AppResult, RuntimeError};
 10|
 11|pub(super) fn run_report(command: ReportCommand) -> AppResult<()> {
 12|    match command {
...
 21|        }
*22|        ReportCommand::CursorAgent { stdin: _ } => {
 23|            if std::env::var_os("OVRCR_AGENT_SOCKET").is_some() {
 24|                let deadline = Instant::now() + Duration::from_secs(1);
 25|                if let Ok(input) = app_report::read_hook_stdin(deadline) {
...
 59|            let deadline = Instant::now() + Duration::from_secs(1);
*60|            app_report::send_report(AgentUpdate::Activity(state), sequence, deadline)
 61|                .map_err(report_runtime_error)
 62|        }
 63|        ReportCommand::Context {
...
 80|    if !stdin_json {
*81|        return Err(RuntimeError::new(
 82|            ErrorCode::InvalidRequest,
 83|            "--stdin-json is required",
 84|        ));
...
 88|    let context = parse_context_json(&input).map_err(|_| invalid_context_input())?;
*89|    app_report::send_report(AgentUpdate::Context(context), sequence, deadline)
 90|        .map_err(report_runtime_error)
 91|}
 92|
...
 94|    if !stdin_json {
*95|        return Err(RuntimeError::new(
 96|            ErrorCode::InvalidRequest,
 97|            "--stdin-json is required",
 98|        ));
...
 102|    let report = parse_claude_context(&input).map_err(|_| invalid_context_input())?;
*103|    app_report::send_report(AgentUpdate::Context(report.clone()), None, deadline)
 104|        .map_err(report_runtime_error)?;
 105|    let sample = ContextUsageSnapshot {
 106|        report,
...
 114|    if !stdin_json {
*115|        return Err(RuntimeError::new(
 116|            ErrorCode::InvalidRequest,
 117|            "--stdin-json is required",
 118|        ));
...
 147|    };
*148|    if app_report::send_report(AgentUpdate::Activity(activity), None, deadline).is_err() && verbose
 149|    {
 150|        eprintln!("hook adapter: report transport failed");
 151|    }
...
 154|
*155|fn report_runtime_error(error: anyhow::Error) -> RuntimeError {
 156|    if let Some(report_error) = error
 157|        .chain()
 158|        .find_map(|cause| cause.downcast_ref::<app_report::ReportError>())
 159|    {
*160|        RuntimeError::new(report_error.code(), report_error.to_string())
 161|    } else {
*162|        RuntimeError::new(ErrorCode::Internal, "hook report failed")
 163|    }
 164|}
 165|
*166|fn invalid_context_input() -> RuntimeError {
*167|    RuntimeError::new(ErrorCode::InvalidRequest, "hook input invalid")
 168|}
 169|
 170|fn run_claude_statusline(stdin_json: bool, render_command: Option<String>) -> AppResult<()> {
...
 180|            .spawn()
*181|            .map_err(|_| RuntimeError::new(ErrorCode::Internal, "renderer could not start"))?;
 182|        let mut pipe = renderer.stdin.take();
 183|        let mut retained = Vec::new();
 184|        let mut oversized = false;
...
 214|            .wait()
*215|            .map_err(|_| RuntimeError::new(ErrorCode::Internal, "renderer wait failed"))?;
 216|        if !status.success() {
 217|            std::process::exit(status.code().unwrap_or(1));
 218|        }
### resources.rs
 5|use ovrcr::protocol::{
*6|    AgentProvider, BranchRequest, CreateSessionRequest, ErrorCode, Request, Response, SessionKind,
 7|};
 8|use ovrcr::session::{SessionId, SessionSummary};
 9|use serde_json::json;
...
 14|use super::{
*15|    AppResult, RuntimeError, inspect, request_started, request_without_start, unexpected_response,
 16|};
 17|
 18|pub(super) fn run_project(command: ProjectCommand, json_output: bool) -> AppResult<()> {
...
 26|                name,
*27|                repo: resolve_cli_path(repo).map_err(RuntimeError::internal)?,
*28|                workspace_root: resolve_cli_path(workspace_root).map_err(RuntimeError::internal)?,
 29|            },
 30|            json_output,
 31|        ),
...
 60|                    project: args.project,
*61|                    id: ovrcr::protocol::new_workspace_id().map_err(RuntimeError::internal)?,
 62|                    branch,
 63|                },
 64|                json_output,
...
 206|                        print!("{text}");
*207|                        std::io::stdout().flush().map_err(RuntimeError::internal)
 208|                    }
 209|                }
 210|                response => Err(unexpected_response(response)),
...
 305|    let Response::Inventory { sessions, .. } = request_without_start(Request::Inspect)? else {
*306|        return Err(RuntimeError::new(
 307|            ErrorCode::Internal,
 308|            "expected session inventory",
 309|        ));
...
 313|        .find(|session| session.id == SessionId(id))
*314|        .ok_or_else(|| RuntimeError::new(ErrorCode::NotFound, format!("session not found: {id}")))
 315|}
 316|
 317|pub(super) fn create_terminal(args: NewArgs, json_output: bool) -> AppResult<()> {
...
 321|                .context("SHELL is unset and no command was provided")
*322|                .map_err(RuntimeError::internal)?,
 323|        ]
 324|    } else {
 325|        args.argv
...
 356|    };
*357|    AgentProvider::from_name(name)
*358|        .map(|provider| SessionKind::Agent {
 359|            name: provider.name().to_string(),
 360|        })
 361|        .unwrap_or(SessionKind::Terminal)
...
 366|    match cli.command {
*367|        Some(Command::Agent {
 368|            command:
*369|                AgentCommand::Run {
 370|                    provider,
 371|                    legacy_provider,
 372|                    argv,
...
 374|        }) => {
*375|            let name = match AgentProvider::from_name(&provider.or(legacy_provider)?)? {
*376|                AgentProvider::CursorCloud => {
 377|                    format!("cursor-cloud:{}", argv.first()?.to_str()?)
 378|                }
 379|                provider => provider.name().to_string(),
 380|            };
*381|            Some(SessionKind::Agent { name })
 382|        }
 383|        _ => None,
 384|    }
...
 392|        .ok_or_else(|| {
*393|            RuntimeError::new(ErrorCode::NotFound, format!("session not found: {id}"))
 394|        })?;
 395|    print_created_terminal(
 396|        request_started(Request::ReopenSession {
...
 421|        .find(|session| session.id == SessionId(id))
*422|        .ok_or_else(|| RuntimeError::new(ErrorCode::NotFound, format!("session not found: {id}")))
 423|}
 424|
 425|fn print_created_terminal(
...
 447|        .find(|project| project.name == name)
*448|        .ok_or_else(|| RuntimeError::new(ErrorCode::NotFound, format!("project not found: {name}")))
 449|}
 450|
 451|fn select_workspace<'a>(
...
 470|    if let Some(path) = path {
*471|        let path = resolve_cli_path(path).map_err(RuntimeError::internal)?;
 472|        return project_record
 473|            .workspaces
 474|            .iter()
### startup.rs
 8|    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
*9|    let mut can_prompt = interactive;
 10|    let mut confirm = |message: &str| -> Result<bool> {
*11|        if !can_prompt {
 12|            return Ok(false);
 13|        }
*14|        write_prompt(&mut std::io::stderr(), message, use_color())?;
 15|        std::io::stderr().flush()?;
 16|        let mut answer = String::new();
 17|        let count = std::io::stdin().lock().take(128).read_line(&mut answer)?;
 18|        if count == 128 && !answer.ends_with('\n') {
*19|            can_prompt = false;
 20|            eprintln!("Answer too long; leaving remaining repairs unchanged.");
 21|            return Ok(false);
 22|        }
...
 57|
*58|fn write_prompt(output: &mut impl Write, message: &str, color: bool) -> std::io::Result<()> {
 59|    use crossterm::style::{Stylize, style};
 60|    use ovrcr_tui::theme;
 61|    let clean: String = message
...
 215|    #[test]
*216|    fn plain_prompt_keeps_the_decision_and_cannot_execute_path_controls() {
 217|        let mut output = Vec::new();
*218|        write_prompt(
 219|            &mut output,
 220|            "Restart OVRCR server\nSocket: /tmp/\x1b[2Jprivate\nAll running sessions will stop.",
 221|            false,

Showing files 1-20 of 46+. Use skip=20 for the next page, or narrow paths/pattern.

[Some lines truncated to 512 bytes]

SOURCE LOOKUP {"path": "src/report/omp.rs"}
//! Managed interactive Oh My Pi: argv eligibility and the harness the shared extension
//! receiver is parameterized by. Oh My Pi has no settled event of its own; its extension
//! synthesizes one from an `agent_end` that does not declare a continuation, so the Ready
//! published there is Observed — a stop hook may still continue after a clean end.
use super::{InvocationLease, extension::Harness};
use ovrcr_protocol::{AgentProvider, SampleQuality};
use ovrcr_runtime::agent_runner::HookHandler;
use std::ffi::{OsStr, OsString};
use std::path::Path;

pub const EXTENSION_SOURCE: &str = include_str!("../omp-reporting-extension.mjs");

pub static HARNESS: Harness = Harness {
    provider: AgentProvider::Omp,
    name: "omp",
    display: "Oh My Pi",
    origin: "omp-extension",
    extension_file: "ovrcr-omp-reporting.mjs",
    extension_source: EXTENSION_SOURCE,
    eligible_argv,
    settled_quality: SampleQuality::Observed,
};

pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("omp")) {
        return false;
    }
    let Some(rest) = argv.get(1..) else {
        return false;
    };
    rest.iter().all(|arg| {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        !matches!(
            arg,
            "--mode" | "-p" | "--print" | "-h" | "--help" | "-v" | "-V" | "--version"
        )
    })
}

/// The extension text for one invocation: the helper path is embedded as a JSON string.
pub fn extension_source(binary: &Path) -> Option<String> {
    super::extension::extension_source(&HARNESS, binary)
}

pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler {
    super::extension::receiver(&HARNESS, lease, argv)
}

SOURCE LOOKUP {"path": "src/cli/agent.rs:1-200"}
use super::{AppResult, RuntimeError, args::AgentCommand};
use ovrcr::protocol::AgentProvider;
use std::os::unix::process::ExitStatusExt;

/// A managed Grok launch needs no hooks or configuration: nothing is reported and the
/// retained history is a title source, so there is nothing to set up or diagnose.
fn grok_needs_no_setup() -> RuntimeError {
    RuntimeError::new(
        ovrcr::protocol::ErrorCode::InvalidRequest,
        "grok has no setup or doctor: `ovrcr agent run grok -- grok` retains the session history without hooks; resume is unavailable",
    )
}

fn provider(name: &str) -> AgentProvider {
    AgentProvider::from_name(name).expect("clap value_parser accepts known provider names")
}

pub(super) fn run(command: AgentCommand) -> AppResult<()> {
    let (name, argv) = match command {
        AgentCommand::CloudPreview { account, recovery } => {
            return super::cursor_cloud::preview(&account, recovery.as_deref());
        }
        AgentCommand::CloudAdmission => return super::cursor_cloud::admission(),
        AgentCommand::CloudCreation => return super::cursor_cloud::creation(),
        AgentCommand::Setup {
            provider,
            print: _,
            settings,
        } => {
            return match provider.as_str() {
                "cursor-cloud" => super::cursor_cloud::setup(settings.as_deref()),
                "cursor-agent" => super::cursor::setup(settings.as_deref()),
                "hermes" => super::hermes::setup(settings.as_deref()),
                "grok" => Err(grok_needs_no_setup()),
                "codex" => super::codex_setup::setup(settings.as_deref()),
                "pi" | "omp" => super::managed::setup(
                    super::managed::managed(&provider).expect("managed provider"),
                ),
                _ => super::agent_setup::setup(settings.as_deref()),
            };
        }
        AgentCommand::Doctor {
            provider,
            settings,
            session,
            executable,
        } => {
            let executable = executable.unwrap_or_else(|| provider.clone().into());
            return match provider.as_str() {
                "cursor-cloud" => super::cursor_cloud::doctor(settings.as_deref()),
                "cursor-agent" => super::cursor::doctor(settings.as_deref(), session, &executable),
                "hermes" => super::hermes::doctor(settings.as_deref(), session, &executable),
                "grok" => Err(grok_needs_no_setup()),
                "codex" => super::codex_setup::doctor(settings.as_deref(), session, &executable),
                "pi" | "omp" => super::managed::doctor(
                    super::managed::managed(&provider).expect("managed provider"),
                    session,
                    &executable,
                ),
                _ => super::agent_setup::doctor(settings.as_deref(), session, &executable),
            };
        }
        AgentCommand::Run {
            provider,
            legacy_provider,
            argv,
        } => (
            provider.or(legacy_provider).expect("required provider"),
            argv,
        ),
    };
    if name == "cursor-cloud" {
        return super::cursor_cloud::launch(&argv);
    }
    let mut argv = argv;
    let assigned_claude = name == "claude" && super::claude_delegation::prepare(&argv)?;
    if name == "codex" {
        super::codex_delegation::prepare(&argv)?;
    }
    ovrcr::protocol::apply_default_auto_trust(&name, &mut argv);
    let pi_resume = if name == "pi" && ovrcr::report::pi::eligible_argv(&argv) {
        ovrcr_runtime::extension_recovery::explicit_pi_resume(&argv)
            .ok()
            .flatten()
    } else {
        None
    };
    let swallow_conflict = name != "claude";
    let mut lease = {
        ovrcr::report::reserve_invocation_with_resume(provider(&name), pi_resume)
            .or_else(|error| {
                if swallow_conflict {
                    Ok(None)
                } else {
                    Err(error)
                }
            })
            .map_err(|error| {
                if let Some(report) = error.downcast_ref::<ovrcr::report::ReportError>() {
                    RuntimeError::new(report.code(), report.to_string())
                } else {
                    RuntimeError::internal(error)
                }
            })?
    };
    let mut delegation_environment = Vec::new();
    if (name == "pi"
        || assigned_claude
        || (name == "codex"
            && std::env::var_os(ovrcr_runtime::codex_delegation::ARTIFACT_ENV).is_some()))
        && let (Ok(session), Ok(token), Ok(socket)) = (
            std::env::var("OVRCR_SESSION_ID"),
            std::env::var("OVRCR_HOOK_TOKEN"),
            std::env::var("OVRCR_HOOK_SOCKET"),
        )
    {
        delegation_environment = vec![
            ("OVRCR_DELEGATION_SESSION".into(), session.into()),
            (
                "OVRCR_DELEGATION_TOKEN".into(),
                ovrcr::agent_runner::delegation_capability(&token).into(),
            ),
            ("OVRCR_DELEGATION_SOCKET".into(), socket.into()),
            (
                "OVRCR_DELEGATION_HOME".into(),
                ovrcr::config::RegistryPath::resolve()
                    .map_err(RuntimeError::internal)?
                    .0
                    .into_os_string(),
            ),
        ];
    }
    let status = ovrcr::agent_runner::run_native_with_env(&argv, &delegation_environment, move |available, native_argv, native_environment| {
        if name == "hermes" {
            if !available {
                drop(lease.take());
                eprintln!(
                    "Hermes reporting unavailable; supervising native process only. Repair: `ovrcr agent doctor hermes --json`."
                );
                return None;
            }
            return Some(ovrcr::report::hermes::receiver(lease, native_argv));
        }
        if !available {
            drop(lease.take());
            eprintln!(
                "agent reporting unavailable; running native command. Repair: `ovrcr agent doctor {name} --json` (setup/doctor never rewrite settings or approve trust)."
            );
            return None;
        }
        Some(match name.as_str() {
            "cursor-agent" => ovrcr::report::cursor::receiver(lease, native_argv),
            "codex" => ovrcr::report::codex::receiver(lease, native_argv),
            "pi" => ovrcr::report::pi::receiver_with_environment(lease, native_argv, native_environment),
            "omp" => ovrcr::report::omp::receiver(lease, native_argv),
            "grok" => ovrcr::report::grok::receiver(lease, native_argv),
            _ => ovrcr::report::admission::receiver(lease, native_argv),
        })
    })
    .map_err(RuntimeError::internal)?;
    if let Some(signal) = status.signal() {
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
        std::process::exit(128 + signal);
    }
    std::process::exit(status.code().unwrap_or(1));
}

SOURCE LOOKUP {"path": "src/report/pi.rs:1-140"}
//! Managed interactive Pi: argv eligibility and the harness the shared extension
//! receiver is parameterized by. Pi's `agent_settled` is a real event that fires once the
//! whole prompt is finished, so its Ready is Confirmed.
use super::{InvocationLease, extension::Harness};
use ovrcr_protocol::{AgentProvider, SampleQuality};
use ovrcr_runtime::agent_runner::HookHandler;
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

pub const EXTENSION_SOURCE: &str = include_str!("../pi-reporting-extension.mjs");

pub static HARNESS: Harness = Harness {
    provider: AgentProvider::Pi,
    name: "pi",
    display: "Pi",
    origin: "pi-extension",
    extension_file: "ovrcr-pi-reporting.ts",
    extension_source: EXTENSION_SOURCE,
    eligible_argv,
    settled_quality: SampleQuality::Confirmed,
};

/// Interactive terminal launches only. Provider arguments are never rewritten here.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("pi")) {
        return false;
    }
    let Some(rest) = argv.get(1..) else {
        return false;
    };
    if let Some(first) = rest.first().and_then(|s| s.to_str())
        && !first.starts_with('-')
        && matches!(
            first,
            "auth"
                | "install"
                | "uninstall"
                | "remove"
                | "list"
                | "update"
                | "upgrade"
                | "config"
                | "help"
        )
    {
        return false;
    }
    rest.iter().all(|arg| {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        !matches!(
            arg,
            "--mode"
                | "-p"
                | "--print"
                | "-h"
                | "--help"
                | "-v"
                | "--version"
                | "--export"
                | "--list-models"
        )
    })
}

/// The extension text for one invocation: the helper path is embedded as a JSON string.
pub fn extension_source(binary: &Path) -> Option<String> {
    super::extension::extension_source(&HARNESS, binary)
}

pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler {
    super::extension::receiver(&HARNESS, lease, argv)
}

/// Finalize invocation-local package selection only after materialization succeeds.
pub fn receiver_with_environment(
    lease: Option<InvocationLease>,
    argv: &mut Vec<OsString>,
    environment: &mut Vec<(OsString, OsString)>,
) -> HookHandler {
    super::extension::receiver_with_environment(&HARNESS, lease, argv, environment)
}

SOURCE LOOKUP {"path": "src/report/extension.rs:1-220"}
//! One authenticated receiver for every provider whose managed launch loads an OVRCR
//! reporting extension. The vocabulary is Pi's — `session_start`, `agent_start`,
//! `agent_end`, `agent_settled`, `model_select`, `session_shutdown` — and each harness
//! says only who it is and how good the Ready at its settled boundary is. Oh My Pi has no
//! settled event of its own: its extension synthesizes one and this receiver publishes it
//! as Observed, because a stop hook may still continue after a clean end. The current
//! model arrives only from the session's own report (`session_start` or `model_select`),
//! never from a launch flag or terminal text, and is published as a Metrics observation
//! for the sidebar. Everything that is not this vocabulary — the lease, the binding, the
//! revisions, the fences, the pause — belongs to the [`Reporter`](super::reporter::Reporter)
//! each call is handed.
use super::InvocationLease;
use super::reporter::{self, Fence, Reporter};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, ContextSample, InputKind,
    InputRequest, MAX_INPUT_REQUESTS, Measurement, MetricsSample, SampleQuality, UsageCoverage,
    UsageScope, UsageTotals, validate_agent_id,
};
use ovrcr_runtime::agent_runner::{HookHandler, private_identifier};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

/// Bounded delivery shared by every provider extension; materialized as its sibling.
pub const TRANSPORT_SOURCE: &str = include_str!("../ovrcr-reporting-transport.mjs");
const TRANSPORT_FILE: &str = "ovrcr-reporting-transport.mjs";
const DELEGATION_CONTINUATION_SOURCE: &str = include_str!("../ovrcr-delegation-continuation.mjs");
const DELEGATION_OBSERVER_SOURCE: &str = include_str!("../ovrcr-delegation-observer.mjs");
const BINARY_TOKEN: &str = "__OVRCR_BINARY__";

/// Everything that differs between one extension-reporting provider and the next.
pub struct Harness {
    pub provider: AgentProvider,
    pub name: &'static str,
    pub display: &'static str,
    pub origin: &'static str,
    pub extension_file: &'static str,
    pub extension_source: &'static str,
    pub eligible_argv: fn(&[OsString]) -> bool,
    /// Quality of the Ready published at the extension's settled event.
    pub settled_quality: SampleQuality,
}

/// The extension text for one invocation: the helper path is embedded as a JSON string.
pub fn extension_source(harness: &Harness, binary: &Path) -> Option<String> {
    let encoded = serde_json::to_string(binary.to_str()?).ok()?;
    Some(harness.extension_source.replacen(BINARY_TOKEN, &encoded, 1))
}

/// A task-owned 0700 directory holding the materialized extension and the transport
/// module it imports; both 0600, both removed with the directory on disable.
fn materialize_extension(harness: &Harness) -> std::io::Result<(PathBuf, PathBuf)> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let binary = std::env::current_exe()?;
    let source = extension_source(harness, &binary)
        .ok_or_else(|| std::io::Error::other("ovrcr path is not valid UTF-8"))?;
    let dir =
        std::env::temp_dir().join(format!("ovrcr-{}-{}", harness.name, private_identifier()?));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let write = |name: &str, text: &str| -> std::io::Result<PathBuf> {
        let path = dir.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(text.as_bytes())?;
        Ok(path)
    };
    write(TRANSPORT_FILE, TRANSPORT_SOURCE)?;
    if harness.provider == AgentProvider::Pi {
        write(
            "ovrcr-pi-runtime.ts",
            include_str!("../ovrcr-pi-runtime.ts"),
        )?;
        write(
            "ovrcr-pi-interrupt.ts",
            include_str!("../ovrcr-pi-interrupt.ts"),
        )?;
        write("ovrcr-delegation-observer.mjs", DELEGATION_OBSERVER_SOURCE)?;
        write(
            "ovrcr-delegation-continuation.mjs",
            DELEGATION_CONTINUATION_SOURCE,
        )?;
    }
    let path = write(harness.extension_file, &source)?;
    Ok((dir, path))
}

/// Admission is a reservation, eligible argv and an interactive terminal — nothing about
/// the executable itself. The launch does not probe `--version`: a non-runnable executable
/// already fails visibly in the native launch, and a bounded probe on a loaded machine
/// times out and disables reporting for an executable that was perfectly fine. Version
/// stays a doctor diagnostic.
pub fn receiver(
    harness: &'static Harness,
    lease: Option<InvocationLease>,
    argv: &mut Vec<OsString>,
) -> HookHandler {
    receiver_with_environment(harness, lease, argv, &mut Vec::new())
}

pub fn receiver_with_environment(
    harness: &'static Harness,
    lease: Option<InvocationLease>,
    argv: &mut Vec<OsString>,
    environment: &mut Vec<(OsString, OsString)>,
) -> HookHandler {
    // Capture only non-secret launch references, before adding our temporary extension.
    let recovery = (|| {
        let executable = argv.first().map(PathBuf::from)?;
        let executable = if executable.is_absolute() {
            executable
        } else {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|directory| directory.join(&executable))
                .find(|path| path.is_absolute() && path.is_file())?
        };
        Some(ovrcr_protocol::ExtensionConversation {
            conversation: String::new(),
            executable,
            history: None,
            config_dir: ovrcr_runtime::extension_recovery::config_dir(harness.provider).ok()?,
            options: ovrcr_runtime::extension_recovery::launch_options(harness.provider, argv)
                .ok()?,
        })
    })();
    let mut scratch = None;
    let unavailable = reporter::preflight(
        lease.is_some(),
        argv,
        harness.eligible_argv,
        reporter::interactive(),
    )
    .map(str::to_owned)
    .or_else(|| match materialize_extension(harness) {
        Ok((dir, path)) => {
            scratch = Some(dir);
            argv.insert(1, "-e".into());
            argv.insert(2, path.clone().into_os_string());
            if harness.provider == AgentProvider::Pi {
                environment.push((
                    "OVRCR_PI_ENTRY".into(),
                    serde_json::json!({ "schema": 1, "path": path })
                        .to_string()
                        .into(),
                ));
            }
            None
        }
        Err(error) => Some(format!("extension not materialized: {error}")),
    });
    let mut reporter = Reporter::new(harness.provider, lease, scratch);
    if let Some(reason) = unavailable {
        reporter.unavailable(harness.display, &reason);
    }
    reporter.handler(Events {
        harness,
        recovery,
        current: None,
        open_requests: Vec::new(),
        model: None,
    })
}

/// One extension's frames. Only the response cycle they open, the Input requests they
/// leave on screen, and the model they last published are this receiver's to remember.
struct Events {
    harness: &'static Harness,
    recovery: Option<ovrcr_protocol::ExtensionConversation>,
    /// The open response cycle: the harness run counter and the identity published as `turn`.
    current: Option<(u64, String)>,
    /// The open Input requests, oldest first, bounded by `MAX_INPUT_REQUESTS`. Every
    /// mutation republishes the whole set, so the runtime never merges deltas. The kind
    /// travels with the id because only the `prompt` namespace's kind comes from the
    /// payload; `approval` and `question` are fixed by their namespace.
    open_requests: Vec<(String, InputKind)>,
    /// The model last accepted for this binding. Cleared when a bind starts a fresh
    /// generation or reporting pauses, so a later identical report is published again.
    model: Option<String>,
}

impl reporter::Frames for Events {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        if let Some(answer) = reporter.own_frame(input, native_root) {
            return answer;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(input) else {
            return reporter::IGNORED.to_vec();
        };
        if value["provider"] != self.harness.name || value["origin"] != self.harness.origin {
            return reporter::IGNORED.to_vec();
        }
        let payload = &value["payload"];
        if payload["schema"] != 1 || payload["mode"] != "tui" {
            return reporter::IGNORED.to_vec();
        }
        let (Some(event), Some(instance), Some(sequence), Some(session)) = (
            payload["event"].as_str(),
            payload["instance"]
                .as_str()
                .filter(|s| validate_agent_id(s).is_ok()),
            payload["sequence"].as_u64().filter(|s| *s > 0),
            payload["session_id"]
                .as_str()
                .filter(|s| validate_agent_id(s).is_ok()),
        ) else {
            return reporter::IGNORED.to_vec();
        };
        // Producer fencing: a new instance is admitted only by its session_start; a stale,
        // replayed, retired or foreign sequence never mutates state, freshness, or Unread.
        let fence = reporter.admit_producer(instance, sequence, event == "session_start");
        if fence == Fence::Ignored {
            return reporter::IGNORED.to_vec();
        }
…
    }
}

[Showing lines 1-223 of 1883. Use :224 to continue]

SOURCE LOOKUP {"path": "src", "pattern": "initial|prompt|stdin|write_all"}
# src/
## client.rs
 93|        .arg("server")
*94|        .stdin(Stdio::null())
 95|        .stdout(log.try_clone().context("share server log")?)
 96|        .stderr(log);
 97|    #[cfg(unix)]
...
 243|    };
*244|    if !confirm(&restart_prompt(&paths.socket, reason, &executable))? {
 245|        bail!(
 246|            "server restart declined for {}; running sessions were left untouched. Retry and approve the separate restart offer, or use the matching CLI to stop this server",
 247|            paths.socket.display()
...
 282|/// wording follows `request_shutdown(kill = true)` and retained-row recovery.
*283|fn restart_prompt(socket: &Path, running: &str, replacement: &Path) -> String {
 284|    format!(
 285|        "Restart OVRCR server\nSocket: {}\nRunning build: {running}\nReplacement: {}\n\nAll running sessions will stop.\nTheir processes are terminated and terminal output is not kept.\nSession rows stay: Reopen starts a fresh shell, and certified\nagent conversations offer Resume conversation.",
 286|        socket.display(),
...
 317|    );
*318|    if !confirm(&restart_prompt(&paths.socket, &running, &server.executable))? {
 319|        bail!(
 320|            "server restart declined for {}; running sessions were left untouched. Retry and approve the separate restart offer, or use the matching old CLI to stop this server",
 321|            paths.socket.display()
## gui.rs
 184|                    .arg("server")
*185|                    .stdin(Stdio::null())
 186|                    .stdout(log.try_clone()?)
 187|                    .stderr(log)
 188|                    .spawn()?,
...
 358|    let mut child = command
*359|        .stdin(Stdio::null())
 360|        .stdout(Stdio::piped())
 361|        .stderr(Stdio::piped())
 362|        .spawn()?;
...
 482|        self.writer
*483|            .write_all(bytes)
 484|            .context("write dashboard input")?;
 485|        self.writer.flush()?;
 486|        Ok(())
## omp-reporting-extension.mjs
 23|    binary: OVRCR_BINARY,
*24|    helperArgs: ["report", "omp", "--stdin"],
 25|  });
 26|
 27|  // Cycles are numbered for the life of this instance, never per conversation: a switch

## bin/
### ovrcr-gui.rs
 528|                    // Keep one Cmd-K intent across startup; never send it to
*529|                    // a setup prompt or make a setup choice on the user's behalf.
 530|                    self.palette_pending = false;
 531|                    if let Err(error) = terminal.send(b"\x07:") {
 532|                        self.error = Some(format!("{error:#}"));
...
 764|            );
*765|            decline_test_startup_prompt(app.terminal.as_mut().unwrap(), &mut declined)?;
 766|            // Real GUI frames continue while the restarted PTY reaches the
 767|            // Dashboard. Do not resend Cmd-K: its original intent must survive.
 768|            let mut output = context.run_ui(
...
 787|#[cfg(test)]
*788|fn decline_test_startup_prompt(
 789|    terminal: &mut Terminal,
 790|    declined: &mut std::collections::HashSet<String>,
 791|) -> anyhow::Result<()> {
...
 808|        .rsplit_once("OVRCR · startup")
*809|        .and_then(|(_, prompt)| prompt.lines().map(str::trim).find(|line| !line.is_empty()))
 810|        .unwrap_or("");
 811|    anyhow::ensure!(
 812|        matches!(
...
 841|        );
*842|        decline_test_startup_prompt(terminal, &mut declined)?;
 843|        std::thread::yield_now();
 844|    }
 845|}

## cli/
### agent_setup.rs
 250|    .iter()
*251|    .any(|prefix| command == format!("{prefix} report {report} --stdin-json"))
 252|}
 253|
 254|pub(super) fn owned_reporter(command: &str, executable: &str, report: &str, input: &str) -> bool {
...
 267|    let default = format!(
*268|        "{MARKER}{} report claude-statusline --stdin-json",
 269|        quote(executable)
 270|    );
 271|    if command == default || exact(command, executable, "claude-statusline") {
...
 316|    );
*317|    let command = format!("{MARKER}{} report claude --stdin-json", quote(executable));
 318|    let hooks = value
 319|        .as_object_mut()
 320|        .unwrap()
...
 361|                        text.starts_with(MARKER)
*362|                            || owned_reporter(text, executable, "claude", "--stdin-json")
 363|                    })
 364|            });
 365|            originally_empty || !handlers.is_empty()
...
 376|    let default = format!(
*377|        "{MARKER}{} report claude-statusline --stdin-json",
 378|        quote(executable)
 379|    );
 380|    match renderer {
...
 384|                .filter(|(end, _)| {
*385|                    marked[*end..].starts_with(" report claude-statusline --stdin-json")
 386|                })
 387|                .map(|(end, _)| {
 388|                    format!(
...
 400|            if renderer.contains("claude-context")
*401|                && !owned_reporter(renderer, executable, "claude-context", "--stdin-json") =>
 402|        {
 403|            eprintln!(
 404|                "Manual migration required: statusLine contains a wrapper around the legacy claude-context reporter and was left untouched. Edit that wrapper to remove its reporting call, then rerun setup with a renderer-only command; or replace statusLine.command with: {default}"
...
 409|                Some(renderer)
*410|                    if !owned_reporter(&renderer, executable, "claude-context", "--stdin-json")
 411|                        && !owned_reporter(
 412|                            &renderer,
 413|                            executable,
 414|                            "claude-statusline",
*415|                            "--stdin-json",
 416|                        ) =>
 417|                {
 418|                    format!("{default} --render-command {}", quote(&renderer))
...
 482|                                == format!(
*483|                                    "{MARKER}{} report claude --stdin-json",
 484|                                    quote(executable)
 485|                                )
 486|                                || exact(command, executable, "claude"))
...
 659|        "session_status":session_status, "binding":binding, "source_health":health,
*660|        "capabilities":{"initial_invocation":{"fresh":true,"resume":"explicit_canonical_lowercase_uuid_v4","resume_forms":["--resume", "-r"],"continue":false,"fork":false}, "activity":"observed", "ready":"available", "completion_quality":"observed", "input_requests":"approvals_available","questions":"unavailable_no_distinct_surface", "settled_completion":"unverified", "usage":"recognized_root_transcript_records_partial", "complete_accounting":false, "context":"statusline_source_reported"},
 661|        "remediation":remediation
 662|    })).map_err(RuntimeError::internal)?);
 663|    Ok(())
...
 723|        assert!(empty.contains(&"statusline_reporting_unverified".to_owned()));
*724|        let command = format!("{MARKER}{} report claude --stdin-json", quote(executable));
*725|        let mut composed = json!({"statusLine":{"type":"command","command":format!("{MARKER}{} report claude-statusline --stdin-json", quote(executable))}});
 726|        for event in HOOKS {
 727|            composed["hooks"][*event] =
 728|                json!([{"hooks":[{"type":"command","command":command,"timeout":2,"async":false}]}]);
...
 744|        let source = "\
*745|# keep\ncommand = \"exec '/tmp/old/ovrcr' report codex --stdin\"\nother = \"exec '/tmp/notify.sh'\"\nstatus = \": ovrcr-managed-claude-v1; '/tmp/old/ovrcr' report claude-statusline --stdin-json --render-command 'ccstatusline'\"\nsame = \"exec '/Users/xlyk/.local/bin/ovrcr' report codex --stdin\"\nplain = \"echo keep /tmp/old/ovrcr\"\n";
 746|        let next = retarget_reporter_text(source, installed);
 747|        assert!(next.contains("# keep"));
 748|        assert!(next.contains("exec '/tmp/notify.sh'"));
 749|        assert!(next.contains("echo keep /tmp/old/ovrcr"));
*750|        assert!(next.contains(": ovrcr-managed-claude-v1; '/Users/xlyk/.local/bin/ovrcr' report claude-statusline --stdin-json --render-command 'ccstatusline'"));
*751|        assert!(next.contains("exec '/Users/xlyk/.local/bin/ovrcr' report codex --stdin"));
 752|        assert!(next.contains("echo keep /tmp/old/ovrcr"));
 753|        assert!(!next.contains("exec '/tmp/old/ovrcr'"));
 754|        assert_eq!(retarget_reporter_text(&next, installed), next);
### args.rs
 67|    },
*68|    /// Submit research/coding assignment and workflow control JSON on stdin (including interrupt/cancel/delivery/archive); connect only.
 69|    /// Question publication, private collection and scoped answers are separate
 70|    /// requests. An answer receipt confirms delivery only, never native approval.
 71|    Delegation,
...
 205|        #[arg(long, required = true)]
*206|        stdin: bool,
 207|    },
 208|    Navigate {
 209|        #[arg(long, required = true)]
*210|        stdin: bool,
 211|    },
 212|}
 213|
...
 218|        #[arg(long, required = true)]
*219|        stdin: bool,
 220|    },
 221|    /// Hermes shell hook. Reads the native payload and forwards identity,
 222|    /// activity, approval and token fields only.
...
 224|        #[arg(long, required = true)]
*225|        stdin: bool,
 226|    },
 227|    Activity {
 228|        /// unknown, idle, busy, waiting-input, response-ready, or error.
...
 235|        #[arg(long)]
*236|        stdin_json: bool,
 237|        #[arg(long)]
 238|        sequence: Option<u64>,
 239|    },
...
 241|        #[arg(long)]
*242|        stdin_json: bool,
 243|        #[arg(long)]
 244|        verbose: bool,
 245|    },
...
 247|        #[arg(long, required = true)]
*248|        stdin: bool,
 249|    },
 250|    Pi {
 251|        #[arg(long, required = true)]
*252|        stdin: bool,
 253|    },
 254|    Omp {
 255|        #[arg(long, required = true)]
*256|        stdin: bool,
 257|    },
 258|    ClaudeContext {
 259|        #[arg(long)]
*260|        stdin_json: bool,
 261|    },
 262|    ClaudeStatusline {
 263|        #[arg(long)]
*264|        stdin_json: bool,
 265|        #[arg(long)]
 266|        render_command: Option<String>,
 267|    },
...
 531|    #[command(
*532|        long_about = "Run a native command with invocation supervision. cursor-cloud requires one named account after --, positive managed admission and macOS Keychain; it validates a pinned SDK Bridge preview of the published workspace origin/HEAD and explicit model and excludes local edits. Explicit prepare retains a local handle, then prompts Send to that same conversation with durable claims and exact run receipts. Busy input requires manual resubmission. An interrupted known-run transport may make a... [col 341]
 533|    )]
 534|    Run {
 535|        #[arg(value_parser = PROVIDERS, required_unless_present = "legacy_provider")]
### bridge.rs
 178|fn read_ticket(deadline: Instant) -> io::Result<Vec<u8>> {
*179|    let mut input = io::stdin().lock();
 180|    let mut bytes = Vec::new();
 181|    loop {
 182|        let remaining = deadline.saturating_duration_since(Instant::now());
...
 487|            for fragment in frame.chunks(3) {
*488|                server.write_all(fragment).unwrap();
 489|                std::thread::park_timeout(Duration::from_millis(1));
 490|            }
 491|        });
...
 515|        let mut client = CallbackStream::new(client, deadline).unwrap();
*516|        client.write_all(&payload).unwrap();
 517|        client.flush().unwrap();
 518|        assert_eq!(received.join().unwrap(), payload);
 519|    }
...
 533|        let (client, mut peer) = UnixStream::pair().unwrap();
*534|        peer.write_all(&[2]).unwrap();
 535|        let mut client = CallbackStream::new(client, Instant::now()).unwrap();
 536|        assert_eq!(
 537|            client.read(&mut [0]).unwrap_err().kind(),
### claude_delegation.rs
 15|    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
*16|    temporary.write_all(bytes)?;
 17|    temporary.as_file().sync_all()?;
 18|    match temporary.persist_noclobber(path) {
 19|        Ok(_) => {
### codex_delegation.rs
 38|        Ok(mut file) => {
*39|            file.write_all(bytes)?;
 40|            file.sync_all()?;
 41|        }
 42|        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
### codex_setup.rs
 19|];
*20|const REQUIREMENTS: &str = "Requires synchronous direct-exec command hooks; no Codex CLI version is required or refused. Review and trust these hooks in native Codex before the first tracked prompt; an initial prompt supplied during hook review may run untracked. Configuration presence does not prove hook trust or delivery. Hooks exit successfully with empty stdout (native no-op), never an approval decision. Launch through the Dashboard agent picker or `ovrcr agent run codex -- codex` inside an OVRCR ter... [col 192]
*21|const FORMS: &str = "Fresh interactive codex (executable basename codex): optional --no-daemon, --strict-config, --no-alt-screen, --approve-for-me, --full-auto, --dangerously-bypass-hook-trust; separate-token --model/-m, --profile/-p, --sandbox/-s, --ask-for-approval/-a, --cd/-C followed by a nonempty value not starting with '-'; at most one prompt (use -- before a prompt matching a subcommand). Exact resume: `codex resume UUID` with the same optional flags and no prompt (UUID must be the exact canonical... [col 345]
 22|
 23|fn settings(path: Option<&Path>) -> anyhow::Result<Value> {
 24|    let Some(path) = path else {
...
 34|fn command() -> anyhow::Result<String> {
*35|    Ok(format!("exec {} report codex --stdin", quote(&binary()?)))
 36|}
 37|fn reporter(group: &Value, command: &str) -> bool {
 38|    let matcher = group.get("matcher").and_then(Value::as_str);
...
 84|        format!(
*85|            "Codex reporting hooks missing in {shown}: run `ovrcr agent setup codex --print --settings {shown}` and merge the result; trust hooks in native Codex before the first tracked prompt"
 86|        )
 87|    })
 88|}
...
 93|    let mut document: toml_edit::DocumentMut = text.parse()?;
*94|    let command = format!("exec {} report codex --stdin", quote(executable));
 95|    let hooks = document
 96|        .entry("hooks")
 97|        .or_insert_with(|| Item::Table(Table::new()))
...
 131|                    .and_then(|text| text.strip_prefix("exec "))
*132|                    .is_some_and(|text| owned_reporter(text, executable, "codex", "--stdin"));
 133|                if !owned {
 134|                    return Ok(true);
 135|                }
...
 365|        "session_status":session_status,"binding":binding,"source_health":health,
*366|        "capabilities":{"initial_invocation":{"fresh":true,"resume":"exact_uuid","fork":false,"picker":false},"conversation_switches":{"clear":"supported","resume":"supported","compact":"same_conversation_no_replacement","startup_while_bound":"identity_transition_unavailable_named_blocker","fork_launch":false},"activity":"last_observed_root_turn","ready":"available_fresh_and_exact_resume","input_requests":"approvals_available","questions":"unavailable_no_distinct_surface","completion_quality":"observed",... [col 26]
 367|        "requirements":REQUIREMENTS, "launch_forms":FORMS,
 368|        "remediation":guidance
 369|    })).map_err(RuntimeError::internal)?);
...
 379|        let base: Value = toml::from_str(
*380|            "[[hooks]]\ntype = 'command'\ncommand = 'exec exact report codex --stdin'\n",
 381|        )
 382|        .unwrap();
*383|        let command = "exec exact report codex --stdin";
 384|        assert!(reporter(&base, command));
*385|        assert!(!reporter(&base, "exec other report codex --stdin"));
 386|        for matcher in [Value::String("never".into()), Value::Boolean(false)] {
 387|            let mut group = base.clone();
 388|            group
### cursor_cloud.rs
*1|//! Managed Cursor cloud admission and terminal prompt entry.
 2|use super::{AppResult, RuntimeError};
 3|use ovrcr::protocol::{AgentProvider, CursorCloudAccount, ErrorCode};
 4|use std::{
...
 60|        serde_json::json!({"provider":"cursor-cloud", "platform_available":cfg!(target_os="macos"), "accounts":accounts, "bridge_dependency":dependency,
*61|        "guidance":"macOS only. Configure named Keychain references, explicit model and repository/owner policy in dashboard.toml. Choose cursor-cloud:ACCOUNT in ordinary Agent creation. Preview validates origin and published HEAD; local edits are excluded. Profiles select credentials, not attested billing identities. Type prepare in the validated preview to retain a local SDK handle. Send prompts on the retained conversation; /cancel persists cancellation intent before exact live CancelRun, cloud-discar... [col 394]
 62|    );
 63|    Ok(())
 64|}
...
 121|    let mut bytes = Vec::new();
*122|    std::io::stdin()
 123|        .take(65537)
 124|        .read_to_end(&mut bytes)
 125|        .map_err(RuntimeError::internal)?;
...
 136|        .args(["agent", "cloud-admission"])
*137|        .stdin(Stdio::null())
 138|        .stdout(Stdio::null())
 139|        .stderr(Stdio::null())
 140|        .status()
...
 161|        let mut child = Command::new(std::env::current_exe()?).args(["agent", "cloud-creation"])
*162|            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
*163|        child.stdin.take().ok_or_else(|| anyhow::anyhow!("creation admission input missing"))?.write_all(&serde_json::to_vec(command)?)?;
 164|        anyhow::ensure!(child.wait()?.success(), "durable creation admission unavailable; original request retained, do not retry");
 165|        Ok(())
 166|    }).map_err(|error| {
### delegation.rs
 6|    let mut bytes = Vec::new();
*7|    std::io::stdin()
 8|        .take(524_289)
 9|        .read_to_end(&mut bytes)
 10|        .map_err(RuntimeError::internal)?;
### delegation_publisher.rs
 406|         "inputSchema":object(json!({"handle":id,"submission":submission})),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
*407|        {"name":QUESTION,"description":"Publish this child's exact model-originated question before waiting. Native login/approval prompts are separate; never include credentials. Publication grants no approval.",
 408|         "inputSchema":object(json!({"handle":id,"question":question})),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
 409|    ]})
 410|}
...
 452|        // Notifications never receive a reply and can never execute a tool.
*453|        if method == "notifications/initialized"
 454|            && matches!(phase, Phase::Initializing)
 455|            && params.is_none_or(|value| value.as_object().is_some_and(|value| value.is_empty()))
 456|        {
...
 463|    let result = match method {
*464|        "initialize" if matches!(phase, Phase::New) => {
 465|            let Some(_) = params.filter(|p| {
 466|                p["protocolVersion"].is_string()
 467|                    && p["capabilities"].is_object()
...
 470|            }) else {
*471|                return error(-32602, "invalid initialize parameters");
 472|            };
 473|            *phase = Phase::Initializing;
 474|            json!({"protocolVersion":VERSION,"capabilities":{"tools":{}},
...
 476|        }
*477|        "initialize" => return error(-32600, "already initialized"),
 478|        "ping" => json!({}),
 479|        _ if !matches!(phase, Phase::Ready) => {
*480|            return error(-32600, "initialize and notifications/initialized required");
 481|        }
 482|        "tools/list" => {
 483|            if params.is_some_and(|params| {
...
 564|            }
*565|            output.write_all(&bytes)?;
*566|            output.write_all(b"\n")?;
 567|            output.flush()?;
 568|        }
 569|        if fatal {
...
 586|    serve(
*587|        &mut io::stdin().lock(),
 588|        &mut io::stdout().lock(),
 589|        authority,
 590|        forward,
...
 610|    }
*611|    fn initialize() -> Value {
*612|        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}})
 613|    }
 614|    fn identity() -> Value {
 615|        json!({"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"handle","session":7,"run":1})
...
 653|        let frames = [
*654|            initialize(),
*655|            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
 656|            json!({"jsonrpc":"2.0","id":request_id,"method":"tools/call",
 657|                "params":{"name":READ,"arguments":read}}),
 658|        ];
...
 754|        assert_eq!(early["error"]["code"], -32600);
*755|        let mut request = initialize();
 756|        request["params"]["protocolVersion"] = "future-version".into();
 757|        assert_eq!(
 758|            message(&mut phase, &auth, request, &mut no_effect).unwrap()["result"]["protocolVersion"],
...
 765|                &auth,
*766|                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
 767|                &mut no_effect
 768|            )
 769|            .is_none()
...
 881|        let mut frames = vec![
*882|            initialize(),
*883|            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
 884|            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
 885|        ];
 886|        let mut bytes = Vec::new();
...
 907|            vec![b'x'; MAX_LINE + 1],
*908|            serde_json::to_vec(&initialize()).unwrap(),
 909|            b"{broken\n".to_vec(),
 910|        ] {
 911|            let mut output = Vec::new();
### hermes.rs
 64|    }
*65|    Ok(format!("'{executable}' report hermes --stdin"))
 66|}
 67|
 68|fn install_hooks(home: &Path, command: &str) -> AppResult<()> {
...
 76|            ErrorCode::Conflict,
*77|            "Hermes config.yaml already has a hooks: key; review it and add the `ovrcr report hermes --stdin` command for on_session_start, pre_api_request, post_api_request, post_llm_call, on_session_end, pre_approval_request and post_approval_response",
 78|        ));
 79|    }
 80|    let mut block = String::from("\n# ovrcr hermes reporting hooks\nhooks:\n");
...
 90|    use std::io::Write;
*91|    file.write_all(block.as_bytes())
 92|        .map_err(RuntimeError::internal)?;
 93|    let allowlist = home.join("shell-hooks-allowlist.json");
 94|    let mut document = std::fs::read_to_string(&allowlist)
...
 143|        .all(|event| text.contains(&format!("{event}:")))
*144|        && text.contains("report hermes --stdin")
 145|}
 146|
 147|pub(super) fn setup(settings: Option<&Path>) -> AppResult<()> {
...
 152|    println!(
*153|        "Installed Hermes reporting hooks in {}. They forward session id, turn id, model, approval id and token counts to OVRCR. Prompts, transcripts and tool arguments are dropped. Ready/Unread follows post_llm_call. Approval prompts follow pre_approval_request. Token totals follow post_api_request. Cost is not in that summary, so cost stays unavailable. Titles are read from the session row in state.db. Resume is `hermes --resume <id>`. Questions that are not approval hooks stay unavailable. Re-run setu... [col 228]
 154|        home.display()
 155|    );
 156|    Ok(())
...
 197|    // Do not invoke --version: Hermes' native version/startup may check updates or
*198|    // initialize profile state. Filesystem discovery is not native acceptance.
 199|    let home = hermes_home().ok();
 200|    let hooks = home.as_deref().is_some_and(hooks_installed);
 201|    let report = json!({
### managed.rs
 15|    pub reporting: &'static str,
*16|    /// Native tool-approval prompts as Input requests.
 17|    pub approvals: &'static str,
 18|    /// Agent questions as Input requests. `available_tool_lifetime` means the request
 19|    /// spans the asking tool's execution, not the dialog's visibility: it opens a moment
...
 175|                                format!(
*176|                                    "Run /ovrcr-reattach in {}; the next prompt alone will not recover it",
 177|                                    provider.display
 178|                                )
 179|                            } else if provider.recovery == "available" {
 180|                                format!(
*181|                                    "Run /ovrcr-reattach in {}, or send the next prompt — the next session boundary recovers reporting as a fresh generation",
 182|                                    provider.display
 183|                                )
 184|                            } else {
*185|                                "Send the next prompt — the next session boundary recovers reporting as a fresh generation".to_owned()
 186|                            };
 187|                            remediation.push(format!(
 188|                                "Reporting is paused for this invocation ({reason}): the source data this reporter delivered is no longer certain, and nothing is being applied. The native session is untouched. {control}.",
### report.rs
 12|    match command {
*13|        ReportCommand::Hermes { stdin: _ } => {
 14|            if std::env::var_os("OVRCR_AGENT_SOCKET").is_some() {
 15|                let deadline = Instant::now() + Duration::from_secs(2);
*16|                if let Ok(input) = app_report::read_hook_stdin_limited(deadline, 8 * 1024 * 1024) {
 17|                    let _ = app_report::send_hermes_hook(&input, deadline);
 18|                }
 19|            }
...
 21|        }
*22|        ReportCommand::CursorAgent { stdin: _ } => {
 23|            if std::env::var_os("OVRCR_AGENT_SOCKET").is_some() {
 24|                let deadline = Instant::now() + Duration::from_secs(1);
*25|                if let Ok(input) = app_report::read_hook_stdin(deadline) {
 26|                    let _ = app_report::send_cursor_hook(&input, deadline);
 27|                }
 28|            }
...
 30|        }
*31|        ReportCommand::Codex { stdin: _ } => {
 32|            if std::env::var_os("OVRCR_AGENT_SOCKET").is_none() {
 33|                return Ok(());
 34|            }
 35|            let deadline = Instant::now() + Duration::from_secs(1);
*36|            if let Ok(input) = app_report::read_hook_stdin(deadline) {
 37|                let _ = app_report::send_codex_hook(&input, deadline);
 38|            }
 39|            Ok(())
...
 52|            let deadline = Instant::now() + Duration::from_secs(1);
*53|            if let Ok(input) = app_report::read_hook_stdin(deadline) {
 54|                let _ = send(&input, deadline);
 55|            }
 56|            Ok(())
...
 63|        ReportCommand::Context {
*64|            stdin_json,
 65|            sequence,
*66|        } => run_report_context(stdin_json, sequence),
 67|        ReportCommand::Claude {
*68|            stdin_json,
 69|            verbose,
*70|        } => run_report_claude(stdin_json, verbose),
*71|        ReportCommand::ClaudeContext { stdin_json } => run_report_claude_context(stdin_json),
 72|        ReportCommand::ClaudeStatusline {
*73|            stdin_json,
 74|            render_command,
*75|        } => run_claude_statusline(stdin_json, render_command),
 76|    }
 77|}
 78|
*79|fn run_report_context(stdin_json: bool, sequence: Option<u64>) -> AppResult<()> {
*80|    if !stdin_json {
 81|        return Err(RuntimeError::new(
 82|            ErrorCode::InvalidRequest,
*83|            "--stdin-json is required",
 84|        ));
 85|    }
 86|    let deadline = Instant::now() + Duration::from_secs(1);
*87|    let input = app_report::read_hook_stdin(deadline).map_err(report_runtime_error)?;
 88|    let context = parse_context_json(&input).map_err(|_| invalid_context_input())?;
 89|    app_report::send_report(AgentUpdate::Context(context), sequence, deadline)
 90|        .map_err(report_runtime_error)
...
 92|
*93|fn run_report_claude_context(stdin_json: bool) -> AppResult<()> {
*94|    if !stdin_json {
 95|        return Err(RuntimeError::new(
 96|            ErrorCode::InvalidRequest,
### startup.rs
 7|pub(super) fn run(paths: &ServerPaths) -> Result<UnixStream> {
*8|    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
*9|    let mut can_prompt = interactive;
 10|    let mut confirm = |message: &str| -> Result<bool> {
*11|        if !can_prompt {
 12|            return Ok(false);
 13|        }
*14|        write_prompt(&mut std::io::stderr(), message, use_color())?;
 15|        std::io::stderr().flush()?;
 16|        let mut answer = String::new();
*17|        let count = std::io::stdin().lock().take(128).read_line(&mut answer)?;
 18|        if count == 128 && !answer.ends_with('\n') {
*19|            can_prompt = false;
 20|            eprintln!("Answer too long; leaving remaining repairs unchanged.");
 21|            return Ok(false);
 22|        }
...
 57|
*58|fn write_prompt(output: &mut impl Write, message: &str, color: bool) -> std::io::Result<()> {
 59|    use crossterm::style::{Stylize, style};
 60|    use ovrcr_tui::theme;
 61|    let clean: String = message
...
 164|    command
*165|        .stdin(Stdio::null())
 166|        .stdout(stdout.try_clone()?)
 167|        .stderr(stderr.try_clone()?);
 168|    unsafe {
...
 215|    #[test]
*216|    fn plain_prompt_keeps_the_decision_and_cannot_execute_path_controls() {
 217|        let mut output = Vec::new();
*218|        write_prompt(
 219|            &mut output,
 220|            "Restart OVRCR server\nSocket: /tmp/\x1b[2Jprivate\nAll running sessions will stop.",
 221|            false,
### startup_bridge.rs
 127|        }
*128|        let prompt = format!(
 129|            "Restart {label}\nRunning: {}\nInstalled: {}\n\nThe running Bridge is older than the installed app, so the\nDashboard reports it as needing an update. Quits that process\nand starts the installed app.",
 130|            describe(&stale),
 131|            destination.display(),
...
 133|        );
*134|        if confirm(&prompt)? {
 135|            restart_processes(profile, &destination, &stale, processes, execute, notice);
 136|        } else {
 137|            notice(
...
 173|    };
*174|    let prompt = if profile == BridgeProfile::LocalDevelopment {
 175|        format!(
 176|            "Install OVRCR Local\nLocation: {}\n\nInstalls a separate local-development app with ad-hoc signing.\nNo Developer ID certificate or Keychain signing identity is used.\nNotification permission is requested separately.{restart}",
 177|            destination.display()
...
 184|    };
*185|    if !confirm(&prompt)? {
 186|        return Ok(());
 187|    }
 188|    let mut installer = Command::new("/bin/sh");
...
 784|        let mut calls = Vec::new();
*785|        let mut prompts = Vec::new();
 786|        run_at(
 787|            fixture.inputs(Some(OsStr::new("1"))),
*788|            &mut |prompt| {
*789|                prompts.push(prompt.to_owned());
 790|                Ok(true)
 791|            },
 792|            &mut |command, _| {
...
 800|        .unwrap();
*801|        assert_eq!(prompts.len(), 1);
*802|        assert!(prompts[0].contains("Install OVRCR Local"));
*803|        assert!(prompts[0].contains("ad-hoc signing"));
*804|        assert!(prompts[0].contains("No Developer ID certificate or Keychain signing identity"));
*805|        assert!(prompts[0].contains("Notification permission is requested separately"));
 806|        assert_eq!(calls.len(), 2);
 807|        assert_eq!(calls[0].program, "python3");
 808|        assert_eq!(
...
 846|            fixture.inputs(Some(OsStr::new("1"))),
*847|            &mut |_| panic!("missing local assets must not prompt"),
 848|            &mut |_, _| panic!("missing local assets must not run a tool"),
 849|            &mut no_identity,
 850|            &mut |notice, success| notices.push((notice.to_owned(), success)),
...
 909|            fixture.inputs(Some(OsStr::new("1"))),
*910|            &mut |_| panic!("valid local installation must not prompt"),
 911|            &mut |command, _| {
 912|                calls.push(Invocation::capture(command));
 913|                Ok(output(true, b""))
...
 985|            fixture.inputs(Some(OsStr::new("1"))),
*986|            &mut |_| panic!("symlink must not prompt"),
 987|            &mut |_, _| panic!("symlink must not run a tool"),
 988|            &mut no_identity,
 989|            &mut |notice, _| notices.push(notice.to_owned()),
...
 1017|                },
*1018|                &mut |_| panic!("invalid profile must not prompt"),
 1019|                &mut |_, _| panic!("invalid profile must not execute"),
 1020|                &mut no_identity,
 1021|                &mut |_, _| {},
...
 1038|                fixture.inputs(opt_in),
*1039|                &mut |prompt| {
*1040|                    assert!(prompt.contains("Install OVRCR Bridge"));
 1041|                    Ok(true)
 1042|                },
 1043|                &mut |command, _| {
...
 1098|                &mut |_| {
*1099|                    assert!(valid, "invalid payload must not prompt");
 1100|                    Ok(false)
 1101|                },
 1102|                &mut |command, _| {
...
 1265|                fixture.inputs(None),
*1266|                &mut |_| panic!("missing assets must not prompt"),
 1267|                &mut |_, _| panic!("missing assets must not run a tool"),
 1268|                &mut no_identity,
 1269|                &mut |notice, _| notices.push(notice.to_owned()),
### startup_hooks.rs
 85|    };
*86|    let prompt = format!(
 87|        "{provider} reporting hooks\nInstall or repair hooks in:\n{}\n\n{effect}\nNative approvals and trust are preserved.\nReview native hook trust and delivery before relying on tracking.",
 88|        path.display()
 89|    );
*90|    if !confirm(&prompt)? {
 91|        return Ok(());
 92|    }
 93|    snapshot.write(&next)?;
...
 232|        temporary
*233|            .write_all(text.as_bytes())
 234|            .context("cannot write native settings file")?;
 235|        if let Some(permissions) = &self.permissions {
 236|            temporary
...
 367|        }
*368|        run(&mut |_| panic!("managed-only and nonexecutable providers must not prompt")).unwrap();
 369|        for name in ["claude", "codex"] {
 370|            let path = bin.join(name);
 371|            std::fs::write(&path, "#!/bin/sh\nexit 83\n").unwrap();
...
 373|        }
*374|        let mut prompts = Vec::new();
*375|        run(&mut |prompt| {
*376|            prompts.push(prompt.to_owned());
 377|            Ok(true)
 378|        })
 379|        .unwrap();
*380|        assert_eq!(prompts.len(), 2);
 381|        assert!(claude.join("settings.json").is_file());
 382|        assert!(codex.join("config.toml").is_file());
 383|        assert!(!home.join(".claude/settings.json").exists());
...
 385|        assert_eq!(std::fs::read_to_string(&historical).unwrap(), "{}");
*386|        run(&mut |_| panic!("unchanged installed hooks must not prompt")).unwrap();
 387|        unsafe {
 388|            std::env::remove_var("CLAUDE_CONFIG_DIR");
 389|            std::env::remove_var("CODEX_HOME");
...
 396|    #[test]
*397|    fn offers_install_preserve_native_settings_and_stop_prompting_after_repair() {
 398|        for (provider, filename, original) in [
 399|            (
 400|                "Claude",
...
 413|            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
*414|            let mut prompts = Vec::new();
*415|            offer(provider, &path, "/installed/ovrcr", &mut |prompt| {
*416|                prompts.push(prompt.to_owned());
 417|                Ok(false)
 418|            })
 419|            .unwrap();
*420|            assert_eq!(prompts.len(), 1);
*421|            assert!(prompts[0].contains(provider));
*422|            assert!(prompts[0].contains(path.to_str().unwrap()));
*423|            assert!(prompts[0].contains("reporting"));
*424|            assert!(!prompts[0].contains("private-hash"));
*425|            assert!(!prompts[0].contains("Bash(secret)"));
 426|            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
 427|            offer(provider, &path, "/installed/ovrcr", &mut |_| Ok(true)).unwrap();
 428|            let installed = std::fs::read_to_string(&path).unwrap();
...
 453|            offer(provider, &path, "/installed/ovrcr", &mut |_| {
*454|                panic!("configured hooks must not prompt")
 455|            })
 456|            .unwrap();
 457|            assert_eq!(std::fs::read_to_string(&path).unwrap(), installed);

## operator/
### auth.rs
 619|        socket
*620|            .write_all(response.as_bytes())
 621|            .await
 622|            .map_err(|_| AuthError::Transport)?;
 623|        Ok(())

Showing files 1-20 of 40+. Use skip=20 for the next page, or narrow paths/pattern.

[Some lines truncated to 512 bytes]

SOURCE LOOKUP {"path": "docs", "pattern": "initial prompt|--prompt|prompt.*argv|model flag|--model|omp"}
# docs/
## agent-reporting-support.md
 2|
*3|Claude contract review date: 2026-09-10. Retained discovery and native evidence verify exact **2.1.267** and **2.1.268**. These are tested releases, separate from the current [patch compatibility policy](agent-versions.md). This record describes reporting research for the [Claude implementation plan](../plans/2026-09-09-claude-code-reporting.md); it does not declare the planned integration shipped.
 4|
 5|The [remaining-requirement matrix](../research/claude-reporting-acceptance/remaining-matrix.md) reconciles this rolling record with the delivered callers, assertions, tested revisions and native evidence. It splits passing parts from open provider and platform clauses.
 6|
...
 12|| --- | --- |
*13|| Hook identity | `session_id`, optional `prompt_id`, and `transcript_path`; `agent_id` identifies a child. `agent_type` can also name a root session. |
*14|| Session transitions | `SessionStart.source` includes startup, resume, clear, compact and fork. Fork covers both foreground branch and background fork operations. |
*15|| Completion observations | Stop can cause continuation. Interrupted turns do not emit Stop. StopFailure covers API failure. Transcript writes can lag callbacks. |
 16|
*17|These fields come from the [official hooks reference](https://code.claude.com/docs/en/hooks), refreshed on the review date. Prompt identity offers correlation; this review does not certify callback order or a final settled notification.
 18|
 19|| Measurement | Source and scope |
 20|| --- | --- |
...
 24|
*25|The [official status-line reference](https://code.claude.com/docs/en/statusline) supplies these semantics. The fixtures use small synthetic values: 30 fresh input + 10 cache-write input + 40 cache-read input = 80 occupied tokens. The pre-response null fixture does not establish post-compaction timing.
 26|
 27|## Foreground admission support
 28|
...
 32|| Clear | Supported: `SessionEnd(reason=clear)` retires obsolete turn/request state without claiming permanent invalidation; `SessionStart(source=clear, new id)` rebinds a fresh Reporting generation. |
*33|| In-process resume | Supported: after an observed leave (`prompt_input_exit` / `resume`) or when `SessionStart(source=resume)` names a different conversation, rebind to that id. Same-id resume without a leave is treated as a duplicate announcement. |
 34|| Foreground branch | Supported only with native leave of the current conversation then `SessionStart(source=fork, new id)` (observed `/branch` form). |
*35|| Compaction | Same-conversation: `SessionStart(source=compact)` does not invent a replacement or replay Ready. |
 36|| Background fork | **Named gap / blocker:** a bare `SessionStart(source=fork)` without a preceding leave of the current conversation cannot be distinguished from background fork; reporting becomes `identity_transition_unavailable` without guessing identity or restarting Claude. |
 37|
 38|Ambiguous or unsupported transitions keep Claude running, clear open Input requests, and surface persistent recovery guidance (`ovrcr agent doctor claude --json`). Switching does not acknowledge server-owned Unread and does not replay history as new alerts.
...
 43|| --- | --- |
*44|| `live-payloads` | Isolated interactive start/resume, prompts, actual approval wait/allow/deny, tool success/failure, Stop continuation, cancellation, API failure, compaction, session end, children and new/resume/fork captures. |
*45|| `settled-transition` | A supported or verified event after every Stop decision, correlated with the current work. Empty background arrays, assistant text, silence and callback completion do not discharge this gate. |
 46|| `cumulative-accounting` | A supported aggregate or a versioned transcript reader with certified record identity, last-record replacement, replay, resumed/forked history and child/auxiliary coverage. |
*47|| `final-source-completion` | A reliable final-accounting boundary, including delayed writes and abrupt native exit. |
*48|| `cost-scope` | Comparison with native totals establishing child, auxiliary, resumed and forked attribution independently of token coverage. |
 49|
 50|Live captures establish equal-usage duplicate rows with the same `(message.id, requestId)` and different record UUIDs. They do not establish differing-value replacement. The partial reader must stop before applying a conflicting counted value and label its retained result unavailable; it cannot claim the retained number remains current or is a lower bound. Recognized root-transcript records do not cover every native usage category. Missing cost remains unknown; explicit zero is a value.
 51|
...
 67|
*68|Authentication was authorized and succeeded after the earlier blocked attempt. [Attempt 05](../research/claude-reporting-acceptance/attempt-05-summary.md) records real startup, consecutive turns, continued Stop, clear, resume, permission wait/allow, tool completion and sanitized usage samples. The source-derived fixture directory remains source-only; live evidence is stored separately. Final settling, complete accounting and transition admission remain uncertified.
 69|
*70|## Compaction and implementation gate follow-up
 71|
*72|[Attempt 07](../research/claude-reporting-acceptance/attempt-07-summary.md) observed same-conversation compaction and a foreground branch with a new ID. The restricted fixture refused background fork, so that case remains unverified. The approved fallback remains unavailable identity for ambiguous transitions.
 73|
*74|Task2 shared runtime work is permitted by the plan's source-or-named-blocker gate. It does not certify a Claude adapter: initial foreground admission, complete final accounting and confirmed settling remain independent provider gates.
 75|
 76|## Initial startup evidence follow-up
 77|
...
 81|
*82|The implementation branch now has independently reviewed shared ownership/protocol, managed launcher, narrow initial admission, observed activity, pure metrics arithmetic and compact dashboard presentation. [Attempt09](../research/claude-reporting-acceptance/attempt-09-managed/commands.md) exercised real managed startup, native output, clear/exit and owned-process cleanup. [Attempt10](../research/claude-reporting-acceptance/attempt-10-sources/summary.md) linked an unanswered approval selector to its corr... [col 177]
 83|
*84|Activity support requires synchronous hooks. Matching permission_prompt means observed waiting; PermissionRequest alone does not. Stop means observed response end (Ready · observed) when correlated by root `prompt_id`, never confirmed settling; Unread and response-ready alerts follow that same turn identity at most once. Native model-not-found StopFailure now has correlated evidence in [attempt11](../research/claude-reporting-acceptance/attempt-11-api-failure/summary.md); broader API failures remain unv... [col 68]
 85|
 86|## Assembled implementation follow-up, 2026-09-10
 87|
...
 91|diagnostics `d139563`. Finalization requires a scan requested after native
*92|completion, keeps the original two-second deadline, and always retains Partial
*93|coverage. Neither EOF nor this scan establishes provider accounting completion.
 94|
 95|Collector cleanup now retains eventual child-reaping ownership at an expired
 96|deadline (`5585702`) and handles macOS zombie-only process groups by reaping the
## agent-reporting.md
 30|invocation. Activity, current context, cumulative usage, cost, and reporting health
*31|are separate observations. A matching root `Stop` with the current `prompt_id`
 32|marks Ready (`response ready · observed`), not confirmed settling or task success.
*33|Claude may continue after Stop; a later prompt or tool event under that turn returns
 34|Busy without clearing Unread. Duplicate Stop under the same Root-turn identity does
 35|not create another Unread or alert. Child events and Stops without a matching
*36|`prompt_id` cannot create root Ready. A matching root `permission_prompt` notification opens an Approval Input request
*37|(`approval:{prompt_id}`), so effective activity is WaitingInput and an input-needed
 38|alert can fire once per request identity; the next attributable root activity closes
 39|it. `PermissionRequest`, idle notifications, and bare tool events alone do not open a
 40|request. AskUserQuestion remains unavailable until a distinct question open/close
...
 48|`SessionStart(source=clear|resume)` replacements rebind Ready/Input to the new
*49|foreground conversation; `source=compact` invents no replacement; conflicting
 50|`source=startup` while bound is `identity_transition_unavailable` (fork/backtrack
 51|gap). See [Codex setup](codex-reporting-setup.md).
 52|
...
 57|conversation cost. Missing values are unknown, while explicit zero remains zero.
*58|Component ages are independent, and each one counts from the last sample whose
 59|value actually changed: a timer replay of an unchanged provider sample keeps the
 60|age it already had, so a session whose totals keep moving never ages out. No
 61|provider certifies sample order, so receiving a callback does not prove it is the
...
 64|The dashboard keeps the full `estimate` label when the metrics row fits. In a
*65|narrow or split pane it uses `est` so scoped cost and both component stale labels
 66|remain visible. If that row is still too wide, it also shortens `tokens` to `tok`
 67|and removes one separator space while preserving the scopes, values, coverage,
*68|and stale labels. A component is labelled ` stale` once its age reaches five
*69|minutes or its session exits; a fresh component carries no label.
 70|
*71|Inspect the complete observation with `ovrcr session usage SESSION_ID` or terminal
 72|inventory. Both retain the raw binding, activity, metrics, and source health.
 73|Reporting transport can be connected while usage or cost is unknown. Supported in-process conversation replacements (`/clear`, in-process resume, and
 74|foreground `/branch` when native leave + `SessionStart(source=fork)` evidence is
...
 144|    "SessionStart": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
*145|    "UserPromptSubmit": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
 146|    "PreToolUse": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
 147|    "PermissionRequest": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
 148|    "PostToolUse": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
...
 157|The adapter reads only the provider JSON on standard input. On this legacy
*158|unbound path it maps `SessionStart` and `Stop` to `idle`, prompt and tool events
 159|to `busy`, `PermissionRequest` to `waiting-input`, `StopFailure` to `error`, and
 160|`SessionEnd` to `unknown`. That idle mapping is not the managed Claude contract
 161|above, where a correlated root Stop is Ready · observed. Events containing
...
 181|`agent_start` (Busy), and at `agent_settled` one of Ready (`response ready · confirmed`,
*182|because Pi settles only after retries, automatic compaction and queued continuations),
 183|Error (the last assistant message stopped with an error) or Idle (aborted, or no assistant
 184|response). Ready creates one unread identity per response cycle; review and alerts work as
*185|for Codex. An extension dialog's outer prompt span is reported as an
 186|[input request](dashboard.md#input-requests): the session waits, whatever activity was
 187|underneath is kept and restored on close, and a background request can raise one
 188|**OVRCR · input needed** alert. Pi has no approval or question surface of its own
 189|beyond these dialogs. Payloads carry identifiers and discriminants only: never
*190|prompts, responses, tool data or prompt titles.
 191|
 192|### Transitions and recovery
 193|
...
 202|never the reuse of the old one, and every later frame from a retired producer is ignored
*203|however high its source sequence. Compaction inside the same conversation changes nothing.
 204|Tree navigation abandons the response cycle it was in (`cycle_invalidated`): activity falls
 205|back to what Pi's own API answers — Idle when it says the session is idle, otherwise
 206|Unknown — and a historical response is never replayed as a new Ready.
...
 227|Native macOS evidence for Oh My Pi reporting is recorded in
*228|[research/issue-97-omp-native](../research/issue-97-omp-native/README.md); see the support
 229|record's "Native verification" section for what it proves and what stays open.
 230|
*231|`ovrcr agent run omp -- omp [ARGS...]` (or picking `omp` in the Dashboard) materializes the
 232|owned reporting extension into a private per-invocation directory and adds `-e <path>`
*233|beside your own extensions; nothing under `~/.omp` changes. The extension is inert without
 234|the private channel and outside the terminal UI, so in-process task and advisor children
 235|(mode `print`) and the ACP route (mode `rpc`) report nothing.
 236|
...
 244|identifiers and discriminants only — the one field added for Oh My Pi is the boolean
*245|continuation flag — never prompts, responses or tool data.
 246|
 247|### Transitions and recovery
 248|
## agent-versions.md
 14|
*15|`agent doctor PROVIDER --json` reports `version`, `tested_versions`, `version_tested` and `version_gate: "none"`. An untested release is untested evidence, not unsupported. The former `compatible_versions` / `version_compatible` fields and the minimum-version floors (Claude >=2.1.267, Codex >=0.153.0, Pi >=0.85.1, Oh My Pi >=18.2.2, Grok >=1.0.40) were removed with the gates. No successful probe proves hook trust, delivery, conversation attachment or native continuity.
 16|
 17|Claude accepts both `--resume UUID` and the separate-token `-r UUID` on every release. Native UUID, hook authentication, ownership, history and configuration checks remain unchanged.
 18|
## claude-code-setup.md
 6|RFC 4122 UUIDv4. Equals syntax, picker/name/search
*7|values, and positional prompts on resume are excluded. Fresh invocations retain
*8|their existing safe options and single positional prompt. Continue, print mode,
 9|and background/fork launch flags remain unsupported reporting paths.
 10|
 11|In-process conversation replacements are followed only when native hooks name a
...
 14|an observed leave), and foreground `/branch` (`SessionEnd` of the current conversation
*15|then `SessionStart(source=fork)`). Same-conversation `/compact` does not invent a
 16|replacement. A bare `SessionStart(source=fork)` without that leave is treated as the
 17|unobservable background-fork gap and makes reporting unavailable while Claude keeps
 18|running. See the [support matrix](agent-reporting-support.md).
 19|
*20|## Compose the settings
 21|
 22|Print a proposed configuration from your existing settings file:
 23|
...
 65|`agent doctor` keeps those fields `unverified` until evidence arrives. Ready is
*66|available as observed root-turn response end (`completion_quality: observed`);
 67|Confirmed settling remains unverified. Approval Input requests are available from a
*68|verified root `Notification(permission_prompt)` (identity `approval:{prompt_id}`);
 69|closing follows the next attributable root activity. AskUserQuestion / elicitation
 70|Input requests remain unavailable: native hooks do not yet provide a distinct
 71|question open/close identity separate from approvals (see support matrix).
...
 131|token usage, and estimated cost when available. Unknown values remain unknown. A
*132|connected reporter does not imply complete accounting or Confirmed settling. Read
 133|the [reporting contract](agent-reporting.md) for scope and freshness semantics.
 134|Claude's explicit `<synthetic>` assistant records (including interruption/error
 135|messages) are excluded from partial usage counting; they are not metered API
...
 160|
*161|Patch compatibility is policy, not native acceptance evidence. See [provider version policy](agent-versions.md).
## cli-reference.md
 39|if the database is absent or still empty and uninitialized after an interrupted
*40|import. A foreign, unreadable, or incompatible database is an error, not a TOML
 41|fallback. Terminal lists include retained rows. Reading or controlling a missing
 42|terminal returns an error.
 43|
...
 74|
*75|`project list` shows projects only; `ovrcr list` prints the complete hierarchy as
 76|`project`, `  workspace`, and `    session <id> <display-name> <phase>` lines.
 77|Workspace lines use the current branch name. Terminal
 78|lists include running sessions and retained exited records. A `--workspace` filter
...
 162|`events` prints the running Server's event ring, oldest first and newest last.
*163|Each line is the time in UTC, the component (`titles`, `settings`, or `quota`),
 164|the session id or provider name when the line is about one, and the one-line
 165|message. `--json` prints that same list as one JSON array (`time_unix_ms`,
*166|`component`, `subject`, `message`) which parses back to the ring. `--follow`
 167|prints the ring and then each event recorded afterwards until the Server
 168|closes the connection; with `--json`, events after the array are one JSON
 169|object per line.
...
 172|read `events.jsonl`. That file, in the instance directory, is for people and other
*173|tools. An event message never includes a prompt, a transcript, a credential,
 174|an account identifier, or a native body.
 175|
 176|The Dashboard Events popup is not part of this command.
...
 265|rows enter their managed resume command into that shell to resume the exact retained
*266|conversation without a new prompt (Claude, Codex, Pi and Oh My Pi). Without a saved
 267|conversation, these providers open their native resume picker instead. The shell
 268|remains usable when the agent exits. Reopen starts the server if it is not running. With `--json`,
 269|reopen returns the terminal object and rename returns `{"ok":true}`.
...
 286|without bracketed-paste support may process them as input. A successful send means
*287|the input was written: it does not establish program readiness or completion. A
 288|read immediately after a send can therefore show the earlier screen; read again to
 289|observe subsequent output.
 290|
...
 350|| `agent run pi -- pi [ARGS...]` | Supervise an interactive Pi launch inside an OVRCR PTY; the owned reporting extension is loaded beside the user's own. Reports Busy, Ready (confirmed at Pi's settled boundary), Error and Idle; creates Unread and Ready alerts. Help, version, print, RPC, JSON, export and package commands run native with reporting unavailable. |
*351|| `agent run omp -- omp [ARGS...]` | Supervise an interactive Oh My Pi launch inside an OVRCR PTY; the owned reporting extension is loaded beside the user's own. Reports Busy, Ready (observed at an end without continuation), Error and Idle; creates Unread and Ready alerts. Headless modes run native with reporting unavailable. |
*352|| `agent setup pi\|omp --print` | Print the managed-launch contract; nothing to write. |
*353|| `agent doctor pi\|omp --json [--session ID] [--executable PATH]` | Probe `--version` (tested / compatible untested / outside range / unknown), inspect an optional `--session` (binding, delivery, activity, health, unread) and print remediation; no server is started. |
*354|| `agent setup codex --print [--settings PATH]` | Print composed Codex TOML with synchronous direct-exec reporters (including unfiltered SessionStart and PermissionRequest/PreToolUse/PostToolUse for Approval Input); preserve existing values and handler order. Explicit installation and native trust review are required. See [Codex setup](codex-reporting-setup.md). |
 355|| `agent doctor codex --json [--settings PATH] [--session ID] [--executable PATH]` | Defaults to `codex`; invokes the provider only for `--version`, checks supplied TOML, and inspects optional session binding and health. Without `--session`, uses inherited `OVRCR_SESSION_ID` when present. Unavailable reporting uses `reporting_unavailable` with recognized diagnostic codes; private reason text stays redacted. Identity freezes explain fresh-invocation recovery; `remediation` remains a string. Does not start...
*356|| `agent setup claude --print [--settings PATH]` | Print composed JSON; migration and removal notes go to stderr. Does not write provider settings. |
 357|| `agent doctor claude --json [--settings PATH] [--session ID] [--executable PATH]` | Probe the executable, report the detected version beside the recorded tested versions (diagnostic only; no version gate), and inspect supplied configuration and optional session health. Unavailable reporting uses `session_status=reporting_unavailable` and exposes recognized diagnostic codes in `source_health.reason`; arbitrary/private reason text remains redacted. Identity freezes explain the unsupported foreground tran...
 358|| `session usage ID` | Emit JSON containing `agent_epoch`, raw `agent`, `reporting_unavailable`, and separate `measurement_age_ms` fields. Does not start a server. |
 359|| `session context ID` | Emit the legacy context sample and stale flag as JSON. Does not start a server. |
...
 363|Terminal inventory includes the same agent snapshot and independent measurement
*364|ages as `session usage`. Each component age counts from the last sample whose
 365|value changed, not from the last delivery, and `context_stale` reports the same
*366|five-minute rule over the context component's age. Unknown observations and
 367|unavailable ages are JSON `null`; zero usage or cost is a known value. The `agent` snapshot contains distinct
 368|binding, activity, metrics, and reporter health. Token and cost scopes must be
 369|interpreted independently. No provider capability or private launcher lease is
...
 400|group is resumed with SIGCONT during cleanup so its handlers and waiters can run;
*401|OVRCR then waits for the final PTY output, reader and child-waiter completion, and
 402|group disappearance before reporting successful cleanup. A session stays in the
 403|hierarchy after exit until `session remove` is requested, so its final screen
 404|remains available.
## codex-ready-installation.md
 31|
*32|Inspect the intended Codex home, config file, `hooks.json` and any profile or project configuration that the launch will actually load. Codex discovers `hooks.json` from each configuration layer's hook directory; `hooks.state` stores per-handler trust and enablement rather than file-inclusion directives. Keep private backups of these existing hook surfaces and preserve their handlers and declaration order. A `--settings` argument tells OVRCR which TOML file to inspect or compose; it does not select Codex... [col 478]
 33|
*34|Inventory any existing OVRCR client, server executable, socket, registry, service definition and live sessions before proposing a change. Inspect the actual selected `OVRCR_CONFIG`, `OVRCR_SOCKET` and any `OVRCR_SERVER_EXECUTABLE` override; a PATH lookup alone does not establish which server owns the sessions. Record absent installations as absent rather than assuming a default path. Use the existing compatible client for read-only session inventory. Do not treat a new client's protocol mismatch as an em... [col 406]
 35|
*36|## Prepare and review the hook composition
 37|
*38|For an existing settings file, keep a private backup with its original permissions, then compose to a different file. If replacing old reporter paths, remove only those exact handlers from a private working copy first and set `CODEX_COMPOSE_INPUT` to that copy:
 39|
 40|```sh
 41|cp -p "$CODEX_SETTINGS" "$OVRCR_PREP/config.before.toml"
...
 50|
*51|Review the synchronous `SessionStart`, `UserPromptSubmit`, `PermissionRequest`, `PreToolUse`, `PostToolUse`, `Stop`, `Interrupt` and `SessionEnd` reporters. They must directly `exec` the reviewed OVRCR executable with `report codex --stdin`. Preserve unrelated hooks and their relative declaration order, including approval hooks, and preserve native configuration/trust values. Setup adds missing reporters and repairs OVRCR-owned stale, filtered, asynchronous or duplicate reporters to one unfiltered synchr... [col 47]
 52|
*53|For exact Codex 0.153.0, keep existing `hooks.json` bytes unchanged and add only the generated reporters to TOML. The [pinned source](../research/issue-58-ready-install/completion/native-hook-source.md) loads configuration layers from low to high and appends JSON then TOML declarations within each layer. Both sources load even though Codex warns that a single representation is preferred. Their handlers run concurrently when synchronous; results are collected back into configured order. Preserve declarati... [col 171]
 54|
*55|The candidate composition embeds `OVRCR_CANDIDATE`, because setup uses its own absolute executable path. It is a preparation draft. For the proposed permanent configuration, change only those exact newly generated reporter commands to the reviewed `OVRCR_INSTALL` path, using the shell quoting shown by setup. Retain both drafts and review the full semantic change privately. After installation, regenerate from the permanent binary and confirm its reporters match the proposed change before writing live conf... [col 16]
 56|
*57|...n, trust and delivery remain `unverified`. Doctor does not inspect the effective hook list or establish that any handler executed. Verify native source paths, declaration order and trust status separately from dispatch. Doctor neither contacts the server nor tests a native prompt, and its accepted `release_status` is an implementation claim. A path-adjusted draft checked by the candidate binary will correctly fail its exact-path configuration check; verify it with the permanent binary after installation. [col 573]
 58|
 59|At this point the binary and configuration change are reviewable. Writing live configuration, replacing an installed binary, changing native trust or transitioning an existing server requires the operator's installation authorization. Preparation alone does not grant it.
 60|
...
 62|
*63|After authorization, retain the prior executable or symlink and its target for rollback. If the selected path is new, record that it was absent. Install the reviewed candidate at the proposed permanent path using the host's chosen installation method, then compare its SHA-256 to the candidate and run its `--version`. The [standard local install](../README.md#install) copies the release binary with executable permissions. Keep a known compatible old client until the server transition and rollback checks a... [col 259]
 64|
*65|Regenerate the composition with the installed binary into another private file. If replacing old reporter paths, supply the reviewed private working copy from which only those old reporters were removed:
 66|
 67|```sh
 68|"$OVRCR_INSTALL" agent setup codex --print \
...
 73|
*74|Omit `--settings` if the file was absent. Compare this output with the approved draft and verify that the source configuration has not changed since preparation. Merge the reviewed change into the intended file, retaining its permissions and any intervening unrelated edits. Do not blindly replace a newer live config with an old composed file. Run doctor again against the resulting live file from the installed binary; retain a redacted result.
 75|
 76|Installation does not upgrade a running server. Perform the following native check with a fresh isolated server before scheduling any normal-server transition. Existing user sessions can continue on their original server and protocol throughout this check.
 77|
...
 101|
*102|Confirm the fixture server runs the installed executable and uses only the recorded temporary config, socket and workspace. Select `ready-check / codex/ready-install-check / codex-ready` in the dashboard and press Enter. In that OVRCR terminal, verify its inherited `OVRCR_CONFIG` and `OVRCR_SOCKET` match the fixture, then run the installed wrapper and exact native executable without an initial prompt:
 103|
 104|```sh
 105|"$OVRCR_INSTALL" agent run codex -- "$CODEX_BIN"
...
 109|
*110|Use native `/hooks` to review the effective list, including existing JSON and plugin hooks. Within each layer, verify JSON declarations appear before TOML declarations for the same event. Preserve original handler keys, contents and relative declaration order, and record each handler's trust and enablement. The list's display order and the collected-result order do not establish execution or completion order; synchronous hooks can run concurrently.
 111|
*112|Individually trust only the reviewed OVRCR reporters through the native UI, and trust the temporary workspace as authorized. Preserve the existing approval policy and unrelated hook state. Existing JSON or plugin hooks may still need review; leave those decisions unchanged and report their status rather than claiming they dispatched. Do not use a blanket trust action or copy a trust hash from an example. Submit the first tracked prompt only after review of the OVRCR reporters finishes; an initial prompt ... [col 436]
 113|
*114|Use a bounded, non-sensitive prompt with a distinct response marker. Verify the marker is actual assistant output, then retain screenshots and relevant accessibility text showing these outcomes:
 115|
*116|1. The root prompt produces Busy / Observed, and its matching Stop produces Ready / Observed while the native process is still running. Ready describes that observed response; it does not certify task success.
*117|2. A second prompt clears Ready to Busy and produces a new Ready observation. Inspect `"$OVRCR_INSTALL" session usage "$OVRCR_NATIVE_SESSION"` from a shell with the same fixture config and socket for its binding, activity and reporting health. Codex metrics remain null.
 118|3. Detach and reattach the one dashboard while preserving the server and provider PID. The last Ready observation remains visible with its reporting health.
*119|4. During a bounded active native turn, press Ctrl-C and verify Idle / Observed without Ready for that interrupted turn. Submit another marker prompt and verify a fresh Busy then Ready before testing exit.
 120|5. On normal native exit, reporting becomes Unavailable. Any retained Ready is historical; it does not establish current readiness.
 121|
*122|Record the installed executable hash, source revision, exact Codex version, server identity, session ID, observed transitions and any failed attempts. Keep OVRCR reporter trust/delivery separate from preservation and dispatch of unrelated hooks. Native `/hooks` proves the listed configuration and review status; claim dispatch only with observed hook run/completion evidence. Preserve any unmet review or dispatch gate explicitly. Never record hook tokens, credentials, full configuration or sensitive conver... [col 358]
 123|
 124|After native exit and evidence capture, exit the fixture's shell and detach its dashboard. From the dedicated fixture shell, inspect `terminal list`, close only any remaining fixture terminals with `terminal close ID`, then run `"$OVRCR_INSTALL" shutdown` without `--kill`. Verify the recorded fixture processes and socket are absent before removing its temporary root. Preserve evidence outside that root first. If ownership or cleanup is uncertain, retain the directory and report the gap. This proves the i...
 125|
...
 129|
*130|If live sessions remain, keep the current server and compatible client available. Finish and close sessions only with their owners' authorization, or postpone the live transition. Native acceptance in the isolated fixture does not require stopping these sessions. Do not connect a protocol 6 candidate to a protocol 4 server as an installation test, remove its live socket, signal a discovered PID, or follow the protocol-mismatch message's `shutdown --kill` suggestion as an upgrade shortcut.
 131|
 132|Once all live sessions have been deliberately closed, use the matching old client and the discovered registry/socket to request `shutdown` **without** `--kill`. This request refuses while sessions remain. If it refuses, preserve the server and resolve the remaining sessions with their owners. Confirm the identified old server has exited and its socket is gone before starting its replacement. A mismatched new client cannot safely issue this shutdown to the old protocol.
 133|
## codex-reporting-setup.md
 16|
*17|The legacy `agent run --provider codex -- codex` form also works. A managed launch appends `--approve-for-me` when argv has no approve, approve-for-me, full-auto, or dangerously-* flag; an existing flag in that family is left untouched. The executable basename must be `codex`. Supported optional arguments are `--no-alt-screen`, `--no-daemon`, `--strict-config`, `--approve-for-me`, `--full-auto` (obsolete; still admitted for saved sessions), `--dangerously-bypass-hook-trust`, and separate-token `--model/-... [col 501]
 18|
 19|## Print and review configuration
 20|
...
 25|
*26|Omit `--settings` to print a standalone example. Setup prints TOML to stdout and instructions to stderr; it never writes configuration or trust. Review the output, then explicitly merge it into the intended Codex `config.toml`. Do not redirect output onto the input file. Composition preserves unrelated native values, handlers and their order, approval/trust entries, comments and formatting. It adds missing reporters and repairs OVRCR-owned stale, filtered, asynchronous or duplicate reporters to one unfil... [col 274]
 27|
 28|`just run` atomically installs `~/.local/bin/ovrcr` and launches the client. Interactive Dashboard startup offers missing-hook installation or repairs in the active Codex profile; it writes only after y/n confirmation. Read-only commands and reporting callbacks skip these offers. Changed reporter commands need native trust review again; configuration does not prove delivery. Never copy trust hashes from retained evidence.
 29|
*30|Run setup and doctor from the exact OVRCR binary whose absolute path the hooks will execute. Setup embeds its own executable path; output from a temporary build points to that temporary build. Doctor checks for reporters using its own path too. Moving the binary requires reviewing a new composition and any native trust prompt. Keep full compositions and backups private because they contain the supplied configuration.
 31|
*32|Inventory and preserve any existing `hooks.json` alongside the intended TOML configuration. Codex discovers that file from each configuration layer's hook directory. `hooks.state` stores per-handler trust and enablement; it does not include or load files. Setup composes only the supplied TOML, and doctor does not inspect other hook sources.
 33|
*34|Exact Codex 0.153.0 loads both JSON and TOML handlers, appending JSON declarations before TOML declarations within each layer. It warns when both contain handlers but disables neither source for that reason. Keep existing JSON bytes, handler indices and trust state unchanged when adding the generated TOML reporters. Synchronous handlers execute concurrently; displayed and collected-result order follow configured declaration order, not guaranteed execution or completion order. Preserve this native behavio... [col 465]
 35|
 36|The [installed-binary check](codex-ready-installation.md#verify-the-installed-path-natively) uses a temporary OVRCR registry, socket and workspace while retaining the intended authorized Codex configuration; it can run without stopping the normal server or its sessions. Verify the effective native hook list and each handler's trust status separately from delivery. A listed handler still needing review is not evidence of dispatch.
 37|
...
 45|
*46|[[hooks.UserPromptSubmit]]
*47|[[hooks.UserPromptSubmit.hooks]]
 48|type = "command"
 49|command = "exec '/absolute/path/to/ovrcr' report codex --stdin"
 50|
...
 83|string). Codex 0.155.1 supplies `fork` as a Root source; the former
*84|`startup|resume|clear|compact` matcher suppresses that callback and prevents
 85|OVRCR's existing identity guard from seeing it. Setup replaces the owned filtered
 86|reporter with one unfiltered reporter, preserving unrelated handlers and native
 87|trust state; doctor flags a supplied file containing only the filtered reporter
...
 106|
*107|An `identity_transition_unavailable` freeze requires a fresh supervised invocation when you are ready. Repairing configuration cannot recover that reporting lifetime; doctor leaves the native Codex process usable. Use `ovrcr session usage ID` for the broader server snapshot and complete the [native installation check](codex-ready-installation.md#verify-the-installed-path-natively). The [diagnostic regression and pinned source review](../research/issue-161-codex-doctor/README.md) do not close the question... [col 281]
 108|
 109|## What the indicator means
 110|
...
 114|`/model` switch replaces the previous model on the next reporting hook. The launch
*115|`--model` flag and terminal text are never read. Context, usage and cost remain
 116|unavailable in this milestone.
 117|
*118|An authenticated root `UserPromptSubmit` binds the observed conversation and marks Busy. A matching root `PermissionRequest` opens an Approval Input request (`approval:{turn_id}`), so effective activity is WaitingInput and an input-needed alert can fire once per request identity; the next attributable root activity closes it. `PreToolUse`/`PostToolUse` alone never open a request. Its matching `Stop` marks ResponseReady with Observed quality once; this means the matching root Stop was observed and the res... [col 30]
 119|
*120|The last Ready observation survives dashboard reconnect while the server remains alive. New observed activity replaces it; reporting becoming Unavailable preserves the last observation with Unavailable health. The next authenticated prompt clears Ready to Busy and may rebind after the prior turn closed. In-process backtracking without a new prompt does not change the displayed last-observed-turn identity. When the next prompt surfaces a conflicting `SessionStart(source=startup)`, reporting becomes `ident... [col 236]
 121|
*122|Missing or unknown callbacks cannot synthesize Ready. An API error without a matching ending hook can leave Busy; a subsequent prompt while the previous turn remains open disables reporting. No timeout, transcript, screen text or metrics infer completion. Restart a fresh managed invocation after correcting hook configuration if reporting becomes unavailable.
 123|
 124|Optional [desktop notifications](dashboard.md#desktop-notifications) can alert
 125|from an active dashboard when a new root Ready response arrives, including for
...
 150|
*151|Exact `codex resume UUID` (managed Reopen / initial resume) is supported for reporting after matching SessionStart(source=resume) Root identity. In-process replacements are followed only when native SessionStart evidence names a trustworthy foreground identity: `source=clear` or `source=resume` with a different conversation id rebinds a fresh Reporting generation; `source=compact` is same-conversation and invents no replacement. A conflicting `SessionStart(source=startup)` or a `source=fork` while alread... [col 377]
 152|
 153|## Retained conversation recovery
 154|
 155|Managed fresh Codex CLI launches capture the exact conversation UUID
*156|and hook-supplied history path at an authenticated root startup or prompt. The
 157|bounded native `session_meta` header must match the UUID and CLI source. Missing
 158|or mismatched history leaves that identity unavailable; it never selects an older
*159|saved conversation. OVRCR stores metadata only, not prompts or transcripts.
 160|
*161|`ovrcr terminal reopen ID` runs managed `codex resume UUID` with no new prompt,
 162|preserving the row, title and reference even across another immediate restart.
 163|The shared ownership acknowledgement, capacity and stale-run rules still apply.
 164|Missing history, executable, working directory, CODEX_HOME or an explicit profile
...
 167|
*168|Saved options are separate-token `--model/-m`, `--profile/-p`, `--sandbox/-s`,
 169|`--ask-for-approval/-a`, absolute `--cd/-C`, `--no-alt-screen`, `--approve-for-me`,
 170|`--no-daemon`, `--strict-config`, and obsolete `--full-auto`.
*171|Task prompts are discarded. Other options, including `--dangerously-bypass-hook-trust`,
 172|inline configuration and relative `--cd`, leave recovery unavailable without changing
 173|existing reporting. Reopen keeps a saved `--approve-for-me` or `--full-auto` instead of rejecting it.
 174|CODEX_HOME (default `$HOME/.codex`) must be absolute and match on reopen; explicit
...
 195|history. Native backtracking can switch conversations without a hook until the
*196|next prompt; an unobserved switch cannot update recovery. Initial raw resume,
 197|picker and fork launches do not capture a new recoverable reference. Independent
 198|native continuity, reporting and platform acceptance remains open in
 199|[#128](https://github.com/xlyk/ovrcr/issues/128); controlled PTY tests do not certify
## cursor-cloud.md
*1|# Cursor cloud prompts, cancellation and results
 2|
 3|On macOS, ordinary **Agent** creation lists `cursor-cloud:ACCOUNT` for each valid
 4|named account in `dashboard.toml`. Local `cursor-agent` remains separate.
...
 29|keychain_account = "work"
*30|model = "composer-2.5"
 31|repositories = ["https://github.com/acme/app"]
 32|owners = []
 33|bridge_archive = "/Users/YOU/.local/lib/ovrcr-cursor-sdk/1.0.40/bridge.tar.gz"
...
 69|handle through the pinned SDK. The acknowledgement is a local handle, not proof that a backend agent/run exists.
*70|After preparation, enter a prompt to Send on that retained conversation. Its first
 71|Send creates backend work. The terminal names its exact accepted run, streams
 72|recognized assistant content, and reports Ready only after source-tool proof,
 73|matching terminal result/done/end receipts and an independent exact GetRun.
 74|GetAgent must confirm the same conversation, cloud runtime and repository.
*75|A later idle prompt uses the same account/conversation with a new Send key and
 76|run. A failed run reports Error; a disconnected or unrecognized stream reports
 77|observation unknown, since remote work and charges may continue. After a transport
 78|interruption with an exact durable run receipt, the original owned bridge may make
...
 85|already-applied occurrences, preserving repeated equal-content output.
*86|Replayed completion cannot create another Ready/Unread. Invalid identity, malformed evidence,
 87|missing/changed offsets or failed progress persistence stop observation. No Send is
 88|replayed, and no fresh bridge is started for this bounded recovery. Prose never
 89|creates a cloud terminal request. Correlated terminal questions/approvals, uncertain
...
 105|
*106|Busy prompts are rejected visibly and the latest rejected prompt stays in local
*107|adapter memory for manual `/resubmit` when idle. It is never a future-prompt queue
 108|and is lost when that adapter exits. An empty idle line or `/detach` closes local
 109|observation; an active synchronous stream processes `/detach` after it returns.
*110|An uncertain Send refuses further prompts and offers `/detach` and the existing
 111|local Discard control. There is no automatic retry, new conversation or newest-run
 112|lookup. Changed profile defaults cannot rewrite retained requests.
 113|
...
 131|requests or agent operations. Missing/malformed capability lists refuse launch;
*132|unknown additional strings are forward compatible. Advertisements are protocol
 133|negotiation, not behavioral proof of terminal responses, retry or active recovery.
 134|The client uses authenticated Connect unary requests over loopback HTTP/1.1,
 135|with redirects/proxy discovery disabled and bounded responses. Only GetVersion,
...
 161|owner. Its credential generation, explicit model and acknowledged agent must
*162|match the original creation receipt. Full prompt/options bytes live only in the
 163|same private directory; SQLite keeps their digest and non-payload operation
 164|receipts. A successful durable Send claim retires the prior Ready/Unread immediately;
 165|activity stays unknown until its exact run is observed. A failed claim commit
...
 182|is also unavailable and not planned. CreateAgent/Send Retry is unavailable and not
*183|planned; exact completed-run GetRun reconciliation remains supported.
 184|
 185|## Cancel and Discard
 186|
...
 195|on that original live bridge. If the original in-flight Send supplies the missing
*196|run ID later, that identity commits before the live attachment completes the
 197|already requested cancellation. An RPC acknowledgement means **requested**.
 198|Only an independent credentialed exact GetRun confirms Cancelled, Finished or
 199|Error; the bridge's cached cancelled status is not remote termination proof.
*200|A completion that wins the race is shown accurately. Delivery remains closed.
 201|A failed intent/claim write permits no cancellation RPC and reports no durable
 202|success for the failed boundary. There is no automatic cancellation retry.
 203|
...
 296|`observe_scenario` exercises wrong-run, changed/missing offsets, repeated transport
*297|loss and replayed completion. These are synthetic fault controls, not cloud proof.
 298|
 299|## Exact-run recovery
 300|
...
 321|cancellation intent, delivery closure and a recovery cancellation claim. CancelRun
*322|then targets that handle; a separate fresh GetRun supplies the outcome. Completion
 323|may win the race and remains Finished. An acknowledgement alone never means stopped.
 324|Each recovered attachment permits one admitted cancellation attempt. A failed or
 325|still-active outcome keeps the intent; detach and explicitly reopen before another
...
 338|recovery never grants CreateAgent/Send dispatch. CreateAgent/Send Retry is
*339|unavailable and not planned under the current scope. Completed lookup supports
 340|historical reconciliation only; fixture success does not establish live behavior
 341|or replace the remaining native, live, packaging and CI acceptance.
 342|
...
 356|planned, not release gates or feature passes. Retain the implemented same-owned-
*357|bridge ObserveRun path and completed exact-run GetRun reconciliation; fresh-process
 358|active restoration, fresh-handle old-offset reuse and CreateAgent/Send Retry
 359|remain excluded.
 360|The current upstream [v1.2.0 local-agent smoke procedure](https://github.com/cursor/sdk-bridge/blob/v1.2.0/docs/smoke-test.md)
*361|uses a Cursor API key for account/catalog access and sends a model prompt through
 362|a local agent; it is not credential-free or offline and may incur charges. OVRCR
 363|remains pinned to SDK 1.0.40, so this upstream documentation is a separate smoke
 364|reference, not runtime evidence for the packaged OVRCR provider. It needs separate
 365|concrete approval. Passive checks or the zero-model local control subset cannot
*366|count as that full smoke. Its `composer-2` sample is stale/redirected and is not
 367|evidence that the model is currently available. Existing fixture results and the
 368|standalone cloud POC retain their original scope.
 369|
...
 374|informational events, future outer frames, exact metadata identity checks, and
*375|receipt/cursor/Ready/Unread ordering. Capability-list forward compatibility does
 376|not by itself establish event-envelope behavior. These fixtures do not prove real
 377|Bridge/provider behavior or packaged native Dashboard acceptance.
## cursor-harness.md
 19|release whitelist. Only fresh interactive launches with no arguments, or
*20|`--model VALUE` / `--model=VALUE`, are eligible. The managed launch reads native
 21|`--help` using the existing owned process-group probe: a one-second deadline,
 22|16 KiB output limit, and OVRCR reporting credentials removed. Help must advertise
 23|`--plugin-dir` as an option. Missing support, a failed/oversized probe,
...
 35|native permissions, or replace other hook configuration. The hook emits no
*36|response JSON, additional context, environment changes or follow-up prompt.
 37|The existing Reporter removes the directory when reporting ends or native exit
 38|releases its ownership; failed materialization removes partial files too.
 39|
...
 47|Only whitelisted identity fields cross the private reporting channel: no email,
*48|workspace data, transcript path, prompt/response text or model is forwarded.
 49|
 50|The Server still observes native process lifecycle and the supervisor preserves
 51|terminal input/output, signals and exit status. Agent activity stays **Unknown**.
...
 64|identifiers, with `is_background_agent=false`. The receiver validates that
*65|contract for each invocation; it does not infer compatibility from a release
*66|string. An absent or incompatible hook cannot manufacture a binding. The hook's
 67|version, email, workspace, transcript and model metadata are not forwarded.
 68|
 69|Primary documentation describes [CLI parameters](https://cursor.com/docs/cli/reference/parameters),
 70|[plugin format](https://cursor.com/docs/plugins), and [hooks](https://cursor.com/docs/hooks).
*71|A submitted prompt can be denied by another hook; a `stop` hook can schedule
 72|follow-up work. Neither establishes a terminal Ready boundary.
 73|
 74|Automated fixtures use disposable executables through actual managed CLI,
## dashboard.md
 2|
*3|Complete key, mouse, and rendering behaviour for the OVRCR dashboard. The
 4|[README](../README.md#the-dashboard) covers the keys you need first; this page is
 5|the full contract.
 6|
...
 29|Browse `o` or the pinned Operator title-bar control opens native chat. It has its
*30|own multiline composer and conversation history, independent of terminal panes.
 31|Conversations stream concurrently and continue while another view is selected.
 32|See [Operator setup and controls](operator.md).
 33|
...
 111|and show Agent identity and session ID; the selected location remains visible
*112|in compact layouts. Unsupported or unavailable reporting never excludes a
 113|Running agent or implies Ready.
 114|
 115|Search uses case-insensitive, every-word substring matching against the full
...
 123|pending typing and stays in Browse. Pane/window focus, selection or geometry
*124|changes, lifecycle/run changes and failed, incomplete or superseded views also cancel it.
 125|A late acknowledgement cannot enable an obsolete choice. Escape while the picker
 126|is still open closes it in Browse without changing selection.
 127|
...
 148|acknowledgement and preserves the newer unread response. Refresh your view and
*149|review that response before acknowledging it. Repeating a completed
 150|acknowledgement is safe.
 151|
 152|For scripts, read `unread` from the intended terminal's JSON row and pass that
*153|complete JSON object to the CLI:
 154|
 155|```sh
 156|ovrcr terminal list --json
...
 179|| --- | --- | --- |

[Showing lines 1-579 of 772 (50.0KB limit). Use :580 to continue. Some lines truncated to 512 bytes]

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs:425-475"}
pub struct ProjectSummary {
    pub name: String,
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub project: String,
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub root: bool,
    pub warning: Option<String>,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Hierarchy(HierarchySnapshot),
    CreatedSession(Box<SessionSummary>),
    Screen {
        session: SessionId,
        run: SessionRunId,
        revision: u64,
        size: TerminalSize,
        bytes: Vec<u8>,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
    Inventory {
        registry: Registry,
        sessions: Vec<SessionSummary>,
    },
    TerminalText {
        session: SessionId,
        size: TerminalSize,
        text: String,
    },
    Task(Box<TaskResponse>),
    HistoryOpened(HistoryOpened),
    HistoryRows(HistoryRows),
    AgentOperation(crate::AgentOperationResult),
    /// A manual quota refresh refused by the Server's cooldown.
    QuotaCooldown {
        remaining_ms: u64,
    },
    /// The Server's event ring, oldest first, newest last.
    Events(Vec<crate::Event>),
    NotificationNavigation(crate::BridgeNavigationResult),
    NotificationNavigationConfirmed(Option<Box<crate::BridgeNavigationOffer>>),
    ITermFocusPrepared(Option<crate::BridgeActivationTarget>),
    BridgeOwner(crate::BridgeOwnerResult),
…
}

[Showing lines 424-478 of 4049. Use :479 to continue]

SOURCE LOOKUP {"path": "tests/workspace_identity.rs;tests/settings_lifecycle.rs;tests/retained_sessions.rs;tests/agent_setup.rs;tests/tui/*.rs"}
# tests/
agent_setup.rs
workspace_identity.rs
settings_lifecycle.rs
retained_sessions.rs
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

SOURCE LOOKUP {"pattern": "fn .*branch|current_branch|detached|WorkspaceSummary \\{|WorkspaceRecord|worktree|is_default_branch", "path": "crates/ovrcr-runtime/src"}
# crates/ovrcr-runtime/src/
## agent_runner.rs
 462|//
*463|// Codex CLI >=0.158 runs synchronous hooks from a detached
 464|// `app-server --managed-daemon` that is a descendant of the native TUI (often
 465|// under a node wrapper), not a direct child. Trust any peer in the native
 466|// process tree; reject callers outside it even with a stolen token.
## codex_recovery.rs
 158|///
*159|/// Codex CLI 0.158 runs hooks from a detached `app-server --managed-daemon` that
 160|/// captures environment at daemon start and can outlive a prior TUI. Stop any
 161|/// CODEX_HOME-owned managed daemon so the new TUI starts a fresh one under this
 162|/// process tree with the current `OVRCR_AGENT_*` channel. Does not rewrite
## config.rs
 10|
*11|pub use ovrcr_protocol::{ProjectRecord, Registry, WorkspaceRecord};
 12|
 13|/// Directory that names one OVRCR instance (`OVRCR_HOME`).
 14|///
...
 532|            Ok(_) => {
*533|                let identity = crate::git::capture_worktree_identity(&path).with_context(|| {
 534|                    format!(
 535|                        "capture Git identity for existing workspace {project}/{id} at {}",
 536|                        path.display()
...
 544|            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
*545|                if let Some(identity) = crate::git::recover_worktree_identity(&repo, &path)? {
 546|                    transaction.execute(
 547|                        "UPDATE workspaces SET git_identity = ?1 WHERE project = ?2 AND id = ?3 AND git_identity IS NULL",
 548|                        params![identity, project, id],
...
 610|                };
*611|                Ok(WorkspaceRecord {
 612|                    id: row.get(0)?,
 613|                    path: PathBuf::from(OsString::from_vec(row.get(1)?)),
 614|                    branch: row.get(2)?,
...
 747|                repo: PathBuf::from("/repo/consigint"),
*748|                workspace_root: PathBuf::from("/worktrees/consigint"),
*749|                workspaces: vec![WorkspaceRecord {
 750|                    id: "cleanup".into(),
*751|                    path: PathBuf::from("/worktrees/consigint/cleanup"),
 752|                    branch: "feature/cleanup".into(),
 753|                    git_identity: Some("1:2".into()),
 754|                    setup_pending: true,
...
 836|                    PathBuf::from("/repo/demo").as_os_str().as_bytes(),
*837|                    PathBuf::from("/worktrees/demo").as_os_str().as_bytes(),
 838|                ],
 839|            )
 840|            .unwrap();
...
 846|                    "legacy",
*847|                    PathBuf::from("/worktrees/demo/legacy").as_os_str().as_bytes(),
 848|                    "feature/legacy",
 849|                ],
 850|            )
...
 1526|            root.git_identity.as_deref(),
*1527|            Some(crate::git::worktree_identity(&repo).unwrap()).as_deref()
 1528|        );
 1529|        let archived = registry.projects[0]
 1530|            .workspaces
...
 1663|            registry.projects[0].workspaces[0].git_identity.as_deref(),
*1664|            Some(crate::git::worktree_identity(&repo).unwrap()).as_deref()
 1665|        );
 1666|        assert_eq!(schema_version(&database), SCHEMA_VERSION);
 1667|    }
...
 1706|    #[test]
*1707|    fn writable_v5_recovers_missing_worktree_identity_from_admin_metadata() {
 1708|        use std::process::Command;
 1709|
 1710|        let dir = tempfile::tempdir().unwrap();
...
 1715|            .args([
*1716|                "worktree",
 1717|                "add",
 1718|                "-b",
 1719|                "feature/kept",
...
 1752|            .unwrap();
*1753|        let recovered = crate::git::recover_worktree_identity(&repo, &canonical).unwrap();
 1754|        assert_eq!(feature.git_identity, recovered);
 1755|        assert!(
 1756|            feature.git_identity.is_some(),
...
 1769|        init_git_repo(&repo);
*1770|        let worktree = volume.join("work");
 1771|        let output = std::process::Command::new("git")
 1772|            .arg("-C")
 1773|            .arg(&repo)
*1774|            .args(["worktree", "add", "-b", "feature/work"])
*1775|            .arg(&worktree)
 1776|            .output()
 1777|            .unwrap();
 1778|        assert!(
...
 1788|            &volume,
*1789|            &[("work", &worktree, "feature/work")],
 1790|        );
 1791|        let unavailable = dir.path().join("unavailable-volume");
 1792|        std::fs::rename(&volume, &unavailable).unwrap();
...
 1802|        let workspace = &project.workspaces[0];
*1803|        let inspection = crate::git::inspect_worktree(project, workspace).unwrap();
 1804|        assert_eq!(inspection.branch, "feature/work");
*1805|        crate::git::remove_worktree(project, workspace, false).unwrap();
 1806|        assert!(
## git.rs
 11|
*12|use crate::config::{ProjectRecord, Registry, WorkspaceRecord};
 13|
 14|pub const UNAVAILABLE_CHECKOUT: &str = "checkout unavailable";
 15|
...
 28|
*29|/// Current checkout label: the full local branch, or `detached @ <short>`.
 30|pub fn checkout_name(path: &Path) -> Result<String> {
 31|    let abbrev = git_stdout(
 32|        path,
...
 47|        )?;
*48|        Ok(format!("detached @ {short}"))
 49|    } else if abbrev.is_empty() {
 50|        bail!("Git checkout name is unavailable");
 51|    } else {
...
 57|/// `origin`, then local `main`/`master`, then the current branch. Never invents
*58|/// a branch for a detached checkout.
*59|pub fn default_branch(repo: &Path) -> Result<String> {
 60|    match detected_default(repo)? {
 61|        DefaultSource::Established(name) | DefaultSource::HeadFallback(name) => Ok(name),
 62|    }
...
 124|        let branch = workspace.branch.trim();
*125|        if branch.is_empty() || branch == UNAVAILABLE_CHECKOUT || branch.starts_with("detached @ ")
 126|        {
 127|            None
 128|        } else {
...
 182|/// Read the captured generation marker. Never writes; a missing marker is not ours.
*183|pub fn worktree_identity(path: &Path) -> Result<String> {
 184|    read_generation(&absolute_git_dir(path)?)
 185|}
 186|
...
 189|/// Capture on create/migrate/register only. Verification must use
*190|/// [`worktree_identity`].
*191|pub fn capture_worktree_identity(path: &Path) -> Result<String> {
 192|    capture_generation(&absolute_git_dir(path)?)
 193|}
 194|
*195|/// Capture identity for a missing worktree from surviving, uniquely matching
 196|/// admin metadata. Writable migration only: stamps a generation marker if the
 197|/// admin has none. Verification must never call this.
*198|pub(crate) fn recover_worktree_identity(repo: &Path, path: &Path) -> Result<Option<String>> {
 199|    fs::symlink_metadata(repo).with_context(|| {
 200|        format!(
 201|            "repository {} is unavailable; restore access and retry workspace migration",
...
 204|    })?;
*205|    let Some(admin) = find_worktree_admin(repo, path)? else {
 206|        return Ok(None);
 207|    };
 208|    Ok(Some(capture_generation(&admin)?))
...
 228|        Some(token) => Ok(token),
*229|        None => bail!("Git identity is missing; refuse replacement worktree"),
 230|    }
 231|}
 232|
...
 288|/// The repository half of [`validate_project`]: the canonical path of `repo`, confirmed to be the
*289|/// root of its own Git worktree. Separate so a caller that has to create the workspace root can
 290|/// check the repository first and leave no directory behind when this half fails.
 291|pub fn validate_repo(repo: &Path) -> Result<PathBuf> {
 292|    let repo = fs::canonicalize(repo)
...
 312|        bail!(
*313|            "repository path is not its Git worktree root: {}",
 314|            repo.display()
 315|        );
 316|    }
...
 340|
*341|pub fn create_worktree(
 342|    project: &ProjectRecord,
 343|    id: &str,
 344|    branch: BranchSpec,
*345|) -> Result<WorkspaceRecord> {
 346|    let (repo, workspace_root) = validate_project(&project.repo, &project.workspace_root)?;
 347|    ovrcr_protocol::validate_name(id, "workspace")?;
 348|    let destination = workspace_root.join(id);
...
 350|        bail!(
*351|            "worktree destination already exists: {}",
 352|            destination.display()
 353|        );
 354|    }
...
 356|        .parent()
*357|        .context("worktree destination has no parent")?;
 358|    if fs::canonicalize(parent)
*359|        .with_context(|| format!("canonicalize worktree parent {}", parent.display()))?
 360|        != workspace_root
 361|    {
## task_manager.rs
 155|        project: &str,
*156|        workspace: &crate::config::WorkspaceRecord,
 157|    ) -> Result<()> {
 158|        self.change(|store| {
 159|            merge_runs(&self.directory, store)?;
...
 326|                let name = format!("task-{}-run-{}", run.task_id.0, run.id.0);
*327|                // Save intent before worktree creation so a partial preparation can be inspected.
 328|                run.directory = Some(record.workspace_root.join(&name));
 329|                run.base_commit = Some(base.clone());
 330|                write_run(&dir, &run)?;
...
 985|                workspace_root: workspaces.clone(),
*986|                workspaces: vec![crate::config::WorkspaceRecord {
 987|                    id: "root".into(),
 988|                    path: repo.clone(),
 989|                    branch: "main".into(),
*990|                    git_identity: Some(crate::git::capture_worktree_identity(&repo).unwrap()),
 991|                    setup_pending: false,
 992|                }],
 993|            })
...
 1024|                    .is_err(),
*1025|                "removed an admitted task's intended worktree"
 1026|            );
 1027|            assert!(workspaces.join("task-1-run-1").is_dir());
 1028|            assert!(
## task_runner.rs
 283|    })();
*284|    // Snapshot detached descendants before Pi can exit and orphan them.
 285|    let discovery = scanner.scan(&mut descendants, &mut groups);
 286|    // Never append commands in the middle of an incomplete prompt JSON line.
 287|    if sent == request.len() {

## git/
### candidate.rs
 3|pub mod review;
*4|use super::{git_stdout, inspect_worktree, run_git, worktree_identity};
*5|use crate::config::{ProjectRecord, WorkspaceRecord};
 6|use anyhow::{Context, Result, bail};
 7|use base64::{Engine, engine::general_purpose::STANDARD};
 8|use serde::{Deserialize, Serialize};
...
 214|fn preimage(
*215|    worktree: &Path,
 216|    base: &str,
 217|    path: &str,
 218|) -> Result<(u32, Option<String>, Option<String>, usize)> {
 219|    let output = run_git(
*220|        worktree,
 221|        &args(&["--literal-pathspecs", "ls-tree", "-z", base, "--", path]),
 222|    )?;
 223|    if output.stdout.is_empty() {
...
 233|    let oid = fields.next().context("missing base object")?;
*234|    let size = git_stdout(worktree, &args(&["cat-file", "-s", oid]))?.parse::<usize>()?;
 235|    if size > MAX_BYTES {
 236|        bail!("candidate base preimage exceeds 256 KiB");
 237|    }
*238|    let bytes = run_git(worktree, &args(&["cat-file", "blob", oid]))?.stdout;
 239|    Ok((
 240|        mode,
 241|        Some(STANDARD.encode(&bytes)),
...
 246|
*247|fn changed_paths(worktree: &Path, base: &str) -> Result<BTreeSet<String>> {
 248|    // Index flags can hide tracked edits even when content and modes changed.
*249|    let flags = run_git(worktree, &args(&["ls-files", "-v", "-z"]))?.stdout;
 250|    if flags
 251|        .split(|byte| *byte == 0)
 252|        .filter_map(|entry| entry.first())
...
 254|    {
*255|        bail!("candidate capture refuses assume-unchanged or skip-worktree index entries");
 256|    }
*257|    if !run_git(worktree, &args(&["ls-files", "--unmerged", "-z"]))?
 258|        .stdout
 259|        .is_empty()
 260|    {
...
 264|        &run_git(
*265|            worktree,
 266|            &args(&[
 267|                "-c",
 268|                "core.filemode=true",
...
 283|        &run_git(
*284|            worktree,
 285|            &args(&["ls-files", "--others", "--exclude-standard", "-z"]),
 286|        )?
 287|        .stdout,
...
 329|
*330|fn read_entry(worktree: &Path, relative: &Path) -> Result<Option<(u32, Vec<u8>)>> {
 331|    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
 332|    let mut parent = OpenOptions::new()
 333|        .read(true)
 334|        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
*335|        .open(worktree)?;
 336|    for part in relative.parent().into_iter().flat_map(Path::components) {
 337|        let Some(next) = open_child(
 338|            &parent,
...
 407|
*408|fn scan(worktree: &Path, base: &str, scope: &[String]) -> Result<Vec<CandidateFile>> {
*409|    let changed = changed_paths(worktree, base)?;
 410|    let mut files = Vec::new();
 411|    let mut total = 0usize;
 412|    for path in changed {
...
 418|        }
*419|        let (base_mode, base_content, base_digest, base_size) = preimage(worktree, base, &path)?;
 420|        total = total
 421|            .checked_add(base_size)
 422|            .context("candidate size overflow")?;
...
 425|        }
*426|        let (mode, bytes) = match read_entry(worktree, Path::new(&path))? {
 427|            Some(entry) => entry,
 428|            None => {
 429|                files.push(CandidateFile {
...
 463|    project: &ProjectRecord,
*464|    workspace: &WorkspaceRecord,
 465|    base: &str,
 466|    scope: &[String],
 467|) -> Result<Vec<u8>> {
...
 474|    }
*475|    let inspected = inspect_worktree(project, workspace)?;
 476|    let path = &inspected.canonical_path;

### candidate/
#### baseline.rs
 143|    project: &ProjectRecord,
*144|    workspace: &WorkspaceRecord,
 145|    baseline: &InputBaseline,
 146|) -> Result<()> {
 147|    baseline.validate()?;
*148|    inspect_worktree(project, workspace)?;
*149|    let generation = worktree_identity(&workspace.path)?;
 150|    let head = git_stdout(
 151|        &workspace.path,
 152|        &args(&["rev-parse", "--verify", "HEAD^{commit}"]),
...
 174|    verify_workspace(project, workspace, baseline)?;
*175|    if worktree_identity(&workspace.path)? != generation {
 176|        bail!("successor replaced during input integration");
 177|    }
 178|    Ok(())
...
 182|    project: &ProjectRecord,
*183|    workspace: &WorkspaceRecord,
 184|    baseline: &InputBaseline,
 185|) -> Result<()> {
 186|    baseline.validate()?;

## server/
### delegation_coding.rs
 79|        })?;
*80|        let actual = crate::git::worktree_identity(&project.repo)
 81|            .context("read registered repository root Git identity")?;
 82|        if actual != expected {
 83|            return Err(lifecycle_error(
...
 143|        record: &Record,
*144|    ) -> Result<(ProjectRecord, WorkspaceRecord)> {
 145|        let coding = record
 146|            .coding
 147|            .as_ref()
...
 164|        }
*165|        crate::git::inspect_worktree(&project, &workspace)?;
 166|        Ok((project, workspace))
 167|    }
 168|
...
 172|        handle: &str,
*173|    ) -> Result<Option<WorkspaceRecord>> {
 174|        let record = self
 175|            .retained
 176|            .lock()
...
 200|        }
*201|        let workspace = WorkspaceRecord {
 202|            id: record.workspace.clone(),
 203|            path: coding.path.clone(),
 204|            branch: crate::git::checkout_name(&coding.path)?,
...
 210|        }
*211|        crate::git::inspect_worktree(&project, &workspace)?;
 212|        if crate::git::resolve_commit(&workspace.path, "HEAD")? != coding.base {
 213|            bail!("prelaunch coding workspace no longer has its exact original base");
 214|        }
...
 232|        handle: &str,
*233|        workspace: &WorkspaceRecord,
 234|        creation: WorkspaceCreation,
 235|    ) -> Result<()> {
 236|        let mut record = self
...
 338|            .clone();
*339|        let inspected = crate::git::inspect_worktree(&project, &workspace)?;
 340|        if let Some(baseline) = Self::read_input_baseline(previous)? {
 341|            if crate::git::resolve_commit(&workspace.path, "HEAD")? != resource.base {
 342|                bail!("coding retry commit anchor changed before launch");
### delegation_reads.rs
 62|            }
*63|            crate::git::inspect_worktree(&project, &workspace)?;
 64|            Some((project, workspace))
 65|        } else {
 66|            None
...
 122|        if let Some((project, workspace)) = &workspace {
*123|            crate::git::inspect_worktree(project, workspace)?;
 124|            let registry = self.registry.lock().unwrap();
 125|            let current = registry.workspace(&record.project, &record.workspace)?;
 126|            if current.path != workspace.path
### mod.rs
 7|use crate::config::{
*8|    ProjectRecord, Registry, WorkspaceRecord, initialize_registry, save_registry_atomic,
 9|};
 10|use crate::git::{self, BranchSpec};
 11|use crate::retained::{RetainedSession, SessionMetadata, SessionStore};
...
 1649|
*1650|    /// Dirty feature worktrees that can be saved as `wip/<branch>`. The root
*1651|    /// checkout and a detached checkout are left alone.
 1652|    fn wip_targets(&self) -> Vec<WipTarget> {
 1653|        let registry = self.registry.lock().unwrap().clone();
 1654|        let mut targets = Vec::new();
...
 1659|                }
*1660|                let Ok(inspection) = git::inspect_worktree(project, workspace) else {
 1661|                    continue;
 1662|                };
 1663|                if !inspection.dirty || git::wip_refname(&inspection.branch).is_err() {
...
 1701|        }
*1702|        let inspection = git::inspect_worktree(&project_record, &workspace)
 1703|            .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?;
 1704|        if !inspection.dirty {
 1705|            return Ok(());
...
 1732|
*1733|    /// Save dirty feature worktrees before shutdown. With the consent setting
 1734|    /// on, every one is pushed. Otherwise an attached Dashboard is asked once
*1735|    /// per worktree; a declined worktree is left unsaved. No Dashboard and the
 1736|    /// setting off continues without saving.
 1737|    fn resolve_shutdown_wip(&self) -> Result<(), String> {
 1738|        let targets = self.wip_targets();
...
 1974|        // The repository is validated before the workspace root is created, so a repository that
*1975|        // is missing or not its own worktree root leaves no empty directory behind.
 1976|        let repo = git::validate_repo(&repo)?;
 1977|        let existing = {
 1978|            let registry = self.registry.lock().unwrap();
...
 2139|    ) -> Result<()> {
*2140|        // Feature worktrees are never the default-branch root workspace.
 2141|        let launch = if crate::settings::load(&self.registry_path)
 2142|            .settings
 2143|            .automatic_local_terminals
...
 2240|            Some(workspace) => workspace,
*2241|            None => git::create_worktree(&project_record, &name, branch)
 2242|                .with_context(|| format!("create workspace {project}/{name}"))?,
 2243|        };
 2244|        if !already_registered {
...
 2266|                    format!(
*2267|                        "registry write failed: worktree remains at {}: {}",
 2268|                        workspace.path.display(),
 2269|                        error_chain_string(&error)
 2270|                    ),
...
 2291|                        format!(
*2292|                            "worktree for workspace {} exists at {} but SHELL is unset",
 2293|                            name,
 2294|                            workspace.path.display()
 2295|                        ),
...
 2336|                format!(
*2337|                    "worktree for workspace {} remains at {}: {}",
 2338|                    name,
 2339|                    workspace.path.display(),
 2340|                    error_chain_string(&error)
...
 2390|            .as_deref()
*2391|            .context("retained worktree is not owned by this run; inspect it before cleanup")?;
 2392|        self.registry
 2393|            .lock()
 2394|            .unwrap()
 2395|            .workspace(project, name)
*2396|            .context("retained worktree is not registered; inspect it before cleanup")?;
 2397|        self.remove_workspace_locked(project, name, Some((&run, run_dir)), false)
 2398|    }
 2399|
...
 2500|        }
*2501|        // Git also deletes ignored files. A clean worktree is not permission to
 2502|        // delete a provider's recorded history, even for an archived session.
 2503|        let history_holders = self
 2504|            .retained
...
 2549|                format!(
*2550|                    "sessions {} record provider history inside this worktree; preserve it outside the worktree and update the provider reference, or `ovrcr terminal remove <id>` to drop the record before removal",
 2551|                    history_holders.join(", ")
 2552|                ),
 2553|            ));
...
 2558|        if directory_present {
*2559|            let inspection = git::inspect_worktree(&project_record, &workspace)
 2560|                .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?;
 2561|            if inspection.dirty && self.save_uncommitted_work() {
 2562|                git::publish_wip(&inspection.canonical_path, &inspection.branch)
...
 2567|                    ErrorCode::DirtyWorktree,
*2568|                    "worktree has changes",
 2569|                ));
 2570|            }
 2571|        }
...
 2575|        {
*2576|            bail!("registered worktree does not match this run's ownership");
 2577|        }
 2578|        if let Some((run, run_dir)) = cleanup {
 2579|            // Release ownership durably before deletion, so a crash or a reused name cannot
### quota.rs
 672|    pub(super) deterministic: bool,
*673|    /// The server stopped or the Dashboard detached mid-wait; nothing was read.
 674|    pub(super) interrupted: bool,
 675|}
 676|impl Failure {
### quota_probe.rs
 114|        }
*115|        // A manual refresh asked for while detached stays due until one attaches.
 116|        let can = attached && enabled && signed_in && !fresh;
 117|        let manual =
 118|            can && std::mem::take(&mut state.quota_refresh.lock().unwrap().claude_probe_due);
### startup.rs
 459|/// about to start, applying the same rules the server applies itself, and
*460|/// return the log file path the detached server should write to.
 461|pub fn prepare_socket_directory(socket: &Path) -> Result<PathBuf> {
 462|    let parent = socket
 463|        .parent()
### tests.rs
 2492|            workspace_root: root.path().to_path_buf(),
*2493|            workspaces: vec![crate::config::WorkspaceRecord {
 2494|                id: "workspace".into(),
 2495|                path: child_cwd.clone(),
 2496|                branch: "main".into(),
...
 4502|            ErrorCode::PartialFailure,
*4503|            "registry write failed: worktree remains",
 4504|        ),
 4505|    );
 4506|    assert!(matches!(
...
 5341|            workspace_root: root.path().to_path_buf(),
*5342|            workspaces: vec![crate::config::WorkspaceRecord {
 5343|                id: "workspace".into(),
 5344|                path: workspace.clone(),
 5345|                branch: "main".into(),
...
 5533|            workspace_root: root.path().to_path_buf(),
*5534|            workspaces: vec![crate::config::WorkspaceRecord {
 5535|                id: "workspace".into(),
 5536|                path: workspace.clone(),
 5537|                branch: "main".into(),
...
 8610|    let repo = std::fs::canonicalize(&repo).unwrap();
*8611|    let git_identity = Some(crate::git::capture_worktree_identity(&repo).unwrap());
 8612|    let config = dir.path().to_path_buf();
 8613|    let root_id = "root-workspace";
 8614|    let registry = Registry {
...
 8618|            workspace_root: dir.path().to_path_buf(),
*8619|            workspaces: vec![crate::config::WorkspaceRecord {
 8620|                id: root_id.into(),
 8621|                path: repo.clone(),
 8622|                branch: "main".into(),
...
 8739|            workspace_root: root.path().to_path_buf(),
*8740|            workspaces: vec![crate::config::WorkspaceRecord {
 8741|                id: "workspace".into(),
 8742|                path: cwd,
 8743|                branch: "main".into(),
...
 8867|            workspace_root: root.path().to_path_buf(),
*8868|            workspaces: vec![crate::config::WorkspaceRecord {
 8869|                id: "workspace".into(),
 8870|                path: cwd,
 8871|                branch: "main".into(),
...
 8934|
*8935|fn init_repo_with_default(repo: &Path, default_branch: &str) {
 8936|    use std::process::Command;
 8937|    std::fs::create_dir_all(repo).unwrap();
 8938|    for args in [
...
 9041|#[test]
*9042|fn automatic_local_terminals_default_branch_only_skips_feature_workspaces() {
 9043|    let dir = tempfile::tempdir().unwrap();
 9044|    let repo = dir.path().join("repo");
 9045|    init_repo_with_default(&repo, "trunk");

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs", "pattern": "struct HierarchySnapshot|project:|workspace|workspaces|branch"}
 161|pub enum BranchRequest {
*162|    New { branch: String, base: String },
*163|    Existing { branch: String },
 164|}
 165|
 166|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
 167|pub struct CreateSessionRequest {
*168|    pub project: String,
*169|    pub workspace: String,
*170|    /// Empty requests a unique workspace-based session name.
 171|    /// The display title stays at this name unless the user renames it.
 172|    pub name: String,
 173|    pub label: Option<String>,
...
 194|        repo: PathBuf,
*195|        workspace_root: PathBuf,
 196|    },
 197|    RemoveProject {
 198|        name: String,
...
 200|    CreateWorkspace {
*201|        project: String,
 202|        id: String,
*203|        branch: BranchRequest,
 204|    },
 205|    RemoveWorkspace {
*206|        project: String,
 207|        name: String,
 208|        /// Acknowledge ownership-uncertain stopped sessions and discard uncommitted
 209|        /// changes. Live sessions and active task runs still block removal.
...
 286|    CreateWorkspaceWithLaunch {
*287|        project: String,
 288|        id: String,
*289|        branch: BranchRequest,
 290|        launch: Option<SessionLaunch>,
 291|    },
 292|    SetSessionTitle {
...
 363|    BridgeOwner(crate::BridgeOwnerCall),
*364|    /// Commit unignored changes onto `refs/heads/wip/<branch>` and push that
*365|    /// ref to `origin`. Does not move the checkout's branch or remove the
*366|    /// workspace.
 367|    SaveWorkspaceWip {
*368|        project: String,
 369|        name: String,
 370|    },
*371|    /// Answer one shutdown prompt. `workspace` is the workspace id.
 372|    /// `save` false continues shutdown without pushing that worktree.
 373|    AnswerWipSave {
*374|        project: String,
*375|        workspace: String,
 376|        save: bool,
 377|    },
 378|    Delegation(Box<crate::DelegationRequest>),
...
 418|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*419|pub struct HierarchySnapshot {
 420|    pub projects: Vec<ProjectSummary>,
 421|}
 422|
...
 425|    pub name: String,
*426|    pub workspaces: Vec<WorkspaceSummary>,
 427|}
 428|
 429|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
 430|pub struct WorkspaceSummary {
*431|    pub project: String,
 432|    pub id: String,
 433|    pub name: String,
 434|    pub path: PathBuf,
...
 538|    WipSavePrompt {
*539|        project: String,
*540|        workspace: String,
*541|        branch: String,
 542|    },
 543|    /// A Lifecycle job finished. `client_token` is the request_id that accepted
 544|    /// the job on the Dashboard connection.
...
 662|            recovery: None,
*663|            project: "p".into(),
*664|            workspace: "w".into(),
 665|            name: "n".into(),
 666|            label: "l".into(),
 667|            pid: Some(2),
...
 756|                    repo: "/r".into(),
*757|                    workspace_root: "/w".into(),
 758|                },
 759|            ),
 760|            ("RemoveProject", Request::RemoveProject { name: "a".into() }),
...
 763|                Request::CreateWorkspace {
*764|                    project: "a".into(),
 765|                    id: "b".into(),
*766|                    branch: BranchRequest::Existing { branch: "c".into() },
 767|                },
 768|            ),
 769|            (
...
 771|                Request::RemoveWorkspace {
*772|                    project: "a".into(),
 773|                    name: "b".into(),
 774|                    force: false,
 775|                },
...
 779|                Request::CreateSession(CreateSessionRequest {
*780|                    project: "a".into(),
*781|                    workspace: "b".into(),
 782|                    name: "c".into(),
 783|                    label: None,
 784|                    argv: vec!["sh".into()],
...
 908|                Request::CreateWorkspaceWithLaunch {
*909|                    project: "a".into(),
 910|                    id: "b".into(),
*911|                    branch: BranchRequest::Existing { branch: "c".into() },
 912|                    launch: Some(SessionLaunch {
 913|                        argv: vec!["sh".into()],
 914|                        label: None,
...
 921|                Request::CreateWorkspaceWithLaunch {
*922|                    project: "a".into(),
 923|                    id: "b".into(),
*924|                    branch: BranchRequest::Existing { branch: "c".into() },
 925|                    launch: None,
 926|                },
 927|            ),
...
 1054|                Request::SaveWorkspaceWip {
*1055|                    project: "a".into(),
 1056|                    name: "b".into(),
 1057|                },
 1058|            ),
...
 1061|                Request::AnswerWipSave {
*1062|                    project: "a".into(),
*1063|                    workspace: "b".into(),
 1064|                    save: true,
 1065|                },
 1066|            ),
...
 1248|                ServerEvent::WipSavePrompt {
*1249|                    project: "a".into(),
*1250|                    workspace: "b".into(),
*1251|                    branch: "feature/topic".into(),
 1252|                },
 1253|            ),
 1254|            (
...
 1521|                path: "/work".into(),
*1522|                branch: "feature/topic".into(),
 1523|                git_identity: Some("1:2".into()),
 1524|                setup_pending: false,
 1525|            }),
...
 1529|            encode(&WorkspaceSummary {
*1530|                project: "p".into(),
 1531|                id: "stable".into(),
 1532|                name: "feature/topic".into(),
 1533|                path: "/work".into(),
...
 1564|            request: "r".into(),
*1565|            project: "p".into(),
*1566|            workspace: "ws".into(),
 1567|            scope: "read".into(),
 1568|            permitted_actions: vec!["read".into()],
 1569|            provider: "pi".into(),
...
 1859|                    coding: None,
*1860|                    project: Some("p".into()),
*1861|                    workspace: Some("ws".into()),
*1862|                    workspace_present: true,
 1863|                    current_session: None,
 1864|                },
 1865|                retryable_capacity_refusal: false,
...
 1965|                            request: "r".into(),
*1966|                            project: "p".into(),
*1967|                            workspace: "ws".into(),
 1968|                            revision: 1,
 1969|                            integrate_snapshots: false,
 1970|                        }],
...
 2105|        let context = OperatorContext {
*2106|            project: Some("p".into()),
*2107|            workspace: Some("label".into()),
*2108|            workspace_id: Some("stable".into()),
 2109|            cwd: Some("/w".into()),
 2110|            session: Some(SessionId(5)),
 2111|            run: Some(SessionRunId(6)),
...
 2546|                resources: crate::AssignmentResources {
*2547|                    project: Some("p".into()),
*2548|                    workspace: Some("ws".into()),
*2549|                    workspace_present: true,
 2550|                    current_session: None,
 2551|                    coding: None,
 2552|                },
...
 2883|                coding: None,
*2884|                project: Some("p".into()),
*2885|                workspace: Some("ws".into()),
*2886|                workspace_present: true,
 2887|                current_session: None,
 2888|            },
 2889|            retryable_capacity_refusal: false,
...
 3142|                digest: "b".repeat(64),
*3143|                workspace: "coding".into(),
 3144|                base: "c".repeat(40),
 3145|                git_identity: "generation".into(),
 3146|                changed_artifacts: vec!["untracked.cjs".into()],
...
 3196|        let snapshot:crate::AssignmentSnapshot=serde_json::from_value(serde_json::json!({
*3197|...mpleted.","evidence":[],"unresolved":[],"review":report},"acceptance":"pending","result_version":1,"publication":null,"acceptance_ref":null,"history":[],"selected_version":null,"historical_submission":null,"receipts":[],"resources":{"project":"p","workspace":"ws","workspace_present":true,"current_session":null,"coding":null},"control":crate::AssignmentControl::default(),"control_evidence":null,"retryable_capacity_refusal":false,"successor_request":null,"execution_attempts":1,"questions":[],"collected_... [col 606]
 3198|        })).unwrap();
 3199|        let response = Response::Delegation(Box::new(snapshot));
 3200|        let bytes = bincode::serde::encode_to_vec(&response, bincode::config::standard()).unwrap();

[Some lines truncated to 512 bytes]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src", "pattern": "HierarchySnapshot|WorkspaceSummary \\{"}
# crates/ovrcr-runtime/src/server/
## mod.rs
 18|use ovrcr_protocol::{
*19|    BranchRequest, ClientMessage, ClientRole, DashboardView, ErrorCode, HierarchySnapshot,
 20|    PROTOCOL_VERSION, ProjectSummary, Request, Response, ServerEvent, ServerMessage, SessionKind,
 21|    SessionRunId, WorkspaceCreation, WorkspaceSummary, read_frame, read_preamble, write_frame,
 22|    write_preamble,
...
 408|
*409|    pub fn hierarchy(&self) -> HierarchySnapshot {
 410|        self.observe_checkouts();
 411|        snapshot_from_state(self)
 412|    }
...
 3032|
*3033|fn snapshot(state: &Arc<ServerState>) -> HierarchySnapshot {
 3034|    snapshot_from_state(state)
 3035|}
 3036|
*3037|fn snapshot_from_state(state: &ServerState) -> HierarchySnapshot {
 3038|    let registry = state.registry.lock().unwrap().clone();
 3039|    let observations = state.observations.lock().clone();
 3040|    let mut sessions = state.session_summaries();
...
 3059|                        .unwrap_or_else(|| workspace.path == project.repo);
*3060|                    WorkspaceSummary {
 3061|                        project: project.name.clone(),
 3062|                        id: workspace.id.clone(),
 3063|                        name: observed
...
 3101|        } else {
*3102|            project.workspaces.push(WorkspaceSummary {
 3103|                project: session.project.clone(),
 3104|                id: session.workspace.clone(),
 3105|                name: git::UNAVAILABLE_CHECKOUT.to_owned(),
...
 3123|    projects.sort_by(|left, right| left.name.cmp(&right.name));
*3124|    HierarchySnapshot { projects }
 3125|}
 3126|
 3127|fn is_root_workspace(project: &ProjectRecord, workspace: &WorkspaceRecord) -> bool {
## tests.rs
 3500|            if sink.enqueue(DashboardOutbound {
*3501|                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
 3502|                    projects: Vec::new(),
 3503|                })),
 3504|                completion: None,
...
 3671|            sink.enqueue(DashboardOutbound {
*3672|                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
 3673|                    projects: Vec::new()
 3674|                },)),
 3675|                completion: None,
...
 3715|        sink.enqueue(DashboardOutbound {
*3716|            message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
 3717|                projects: Vec::new()
 3718|            })),
 3719|            completion: None,
...
 3738|            sink.enqueue(DashboardOutbound {
*3739|                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
 3740|                    projects: Vec::new()
 3741|                })),
 3742|                completion: None,
...
 3748|        sink.enqueue(DashboardOutbound {
*3749|            message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
 3750|                projects: Vec::new()
 3751|            })),
 3752|            completion: None,

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/mod.rs:3030-3135"}
mod default_session_label_tests {
…
    fn direct_agent_executable_matches_kind_when_label_omitted() {
        assert_eq!(
…
        );
    }
}

fn snapshot(state: &Arc<ServerState>) -> HierarchySnapshot {
    snapshot_from_state(state)
}

fn snapshot_from_state(state: &ServerState) -> HierarchySnapshot {
    let registry = state.registry.lock().unwrap().clone();
    let observations = state.observations.lock().clone();
    let mut sessions = state.session_summaries();
    sessions.retain(|session| {
        !session.archived && session.scope == ovrcr_protocol::SessionScope::Workspace
    });
    let mut projects = registry
        .projects
        .iter()
        .map(|project| {
            let workspaces = project
                .workspaces
                .iter()
                .map(|workspace| {
                    let observed = observations
                        .get(&(project.name.clone(), workspace.id.clone()))
                        .filter(|obs| {
                            obs.path == workspace.path && obs.git_identity == workspace.git_identity
                        });
                    let root = observed
                        .map(|obs| obs.root)
                        .unwrap_or_else(|| workspace.path == project.repo);
                    WorkspaceSummary {
                        project: project.name.clone(),
                        id: workspace.id.clone(),
                        name: observed
                            .map(|obs| obs.name.clone())
                            .unwrap_or_else(|| git::UNAVAILABLE_CHECKOUT.to_owned()),
                        path: workspace.path.clone(),
                        root,
                        warning: observed
                            .filter(|_| root)
                            .and_then(|obs| obs.warning.clone()),
                        sessions: Vec::new(),
                    }
                })
                .collect::<Vec<_>>();
            ProjectSummary {
                name: project.name.clone(),
                workspaces,
            }
        })
        .collect::<Vec<_>>();
    // Returned records outlive registry entries. Keep them selectable without
    // registering a new workspace or substituting a different working directory.
    for session in sessions {
        let project_index = projects
            .iter()
            .position(|p| p.name == session.project)
            .unwrap_or_else(|| {
                projects.push(ProjectSummary {
                    name: session.project.clone(),
                    workspaces: Vec::new(),
                });
                projects.len() - 1
            });
        let project = &mut projects[project_index];
        if let Some(workspace) = project
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == session.workspace)
        {
            workspace.sessions.push(session);
        } else {
            project.workspaces.push(WorkspaceSummary {
                project: session.project.clone(),
                id: session.workspace.clone(),
                name: git::UNAVAILABLE_CHECKOUT.to_owned(),
                path: session.cwd.clone(),
                root: false,
                warning: None,
                sessions: vec![session],
            });
        }
    }
    for project in &mut projects {
        project
            .workspaces
            .sort_by(|a, b| (!a.root, &a.name).cmp(&(!b.root, &b.name)));
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .sort_by_key(|row| (row.name != "local", row.id.0));
        }
    }
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    HierarchySnapshot { projects }
}

fn is_root_workspace(project: &ProjectRecord, workspace: &WorkspaceRecord) -> bool {
    same_path(&workspace.path, &project.repo)
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

[Showing lines 3029-3138 of 3142. Use :3139 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server", "pattern": "fn observe_checkouts|observations|ObservedWorkspace|struct Observation"}
# crates/ovrcr-runtime/src/server/
## delegation.rs
 2057|                if let Some(session) = record.identity.session {
*2058|                    self.delegation_observations.invalidate(session);
 2059|                }
 2060|                return self.assignment_snapshot(record);
 2061|            }
...
 2205|                drop(store);
*2206|                self.delegation_observations.invalidate(auth.session);
 2207|                let publication = publication.map(|identity| PublicationState {
 2208|                    handled: record.handled_publication.as_ref() == Some(&identity),
 2209|                    identity,
## delegation_fallback_routes.rs
 329|        if let Some(session) = record.identity.session {
*330|            self.delegation_observations.invalidate(session);
 331|        }
 332|        let mut snapshot = self.reconciliation_snapshot(record)?;
 333|        snapshot.recovered = Some(publication);
## delegation_followups.rs
 563|                    drop(store);
*564|                    self.delegation_observations.invalidate(session.id());
 565|                    return self.reconciliation_snapshot(record);
 566|                }
 567|                record.dispatch = DispatchStatus::Applied;
...
 595|        if let Some(session) = record.identity.session {
*596|            self.delegation_observations.invalidate(session);
 597|        }
 598|        self.reconciliation_snapshot(record)
 599|    }
...
 845|        if let Some(session) = record.identity.session {
*846|            self.delegation_observations.invalidate(session);
 847|        }
 848|        self.assignment_snapshot(record)
 849|    }
## delegation_observation.rs
 61|#[derive(Default)]
*62|pub(super) struct Observations {
 63|    ring: Mutex<Ring>,
 64|    changed: Condvar,
 65|    checkpoints: Mutex<()>,
...
 292|            let deadline = self.delegation_progress_deadline_ms();
*293|            let mut observations = Vec::with_capacity(handles.len());
 294|            let mut sessions = Vec::with_capacity(handles.len());
 295|            for handle in handles {
 296|                let record = self.owned_assignment(auth, handle)?;
...
 298|                let session = record.identity.session.unwrap_or(auth.session);
*299|                self.delegation_observations.publication_changed(
 300|                    handle,
 301|                    session,
 302|                    Fingerprint {
...
 316|                sessions.push(session);
*317|                observations.push(observed);
 318|            }
*319|            let delta = self.delegation_observations.delta(sequence, &sessions);
 320|            let remaining = until.saturating_duration_since(std::time::Instant::now());
 321|            if delta.changed || remaining.is_zero() {
 322|                let status = if delta.reconcile
*323|                    || observations.iter().any(|observed| {
 324|                        observed.control.barrier.is_some()
 325|                            || observed.control.owner == ovrcr_protocol::ControlOwner::Uncertain
 326|                    }) {
 327|                    ObservationStatus::ReconcileRequired
*328|                } else if observations.iter().any(|o| o.stalled) {
 329|                    ObservationStatus::Stalled
 330|                } else if delta.changed {
 331|                    ObservationStatus::Changed
...
 341|                    progress_deadline_ms: deadline,
*342|                    observations,
 343|                });
 344|            }
*345|            self.delegation_observations
 346|                .wait(delta.sequence, remaining.min(RECONCILE_INTERVAL));
 347|        }
 348|    }
...
 375|        let parent = self.delegation_auth(auth)?;
*376|        let _checkpoint = self.delegation_observations.checkpoints.lock().unwrap();
 377|        let store = self.retained.lock();
 378|        let authority = self.require_workflow(&store, auth, record)?;
 379|        let progress =
...
 420|                    let accepted = {
*421|                        let ring = self.delegation_observations.ring.lock().unwrap();
 422|                        ring.sources
 423|                            .get(&summary.id)
 424|                            .filter(|(cached, _)| *cached == source)
...
 482|    fn overflow_and_foreign_cursors_require_reconciliation() {
*483|        let observations = Observations::default();
 484|        for _ in 0..HINT_CAPACITY + 2 {
*485|            observations.invalidate(SessionId(1));
 486|        }
*487|        assert!(observations.delta(Some(0), &[SessionId(1)]).reconcile);
 488|        assert!(
*489|            observations
 490|                .delta(Some(u64::MAX), &[SessionId(1)])
 491|                .reconcile
 492|        );
*493|        let last = observations.delta(None, &[]).sequence;
*494|        assert!(!observations.delta(Some(last), &[SessionId(1)]).changed);
*495|        assert!(!observations.delta(Some(last - 1), &[SessionId(2)]).changed);
*496|        assert!(observations.delta(Some(last - 1), &[SessionId(1)]).changed);
 497|    }
 498|    #[test]
 499|    fn seeding_and_health_changes_do_not_certify_fresh_progress() {
*500|        let observations = Observations::default();
 501|        let mut source = Source {
 502|            run: SessionRunId(1),
 503|            binding: None,
## delegation_questions.rs
 598|        if take_fault("question-after-link", &handle).is_none() {
*599|            self.delegation_observations.invalidate(auth.session);
 600|        }
 601|        self.reconciliation_snapshot(record)
 602|    }
...
 650|        if let Some(id) = record.identity.session {
*651|            self.delegation_observations.invalidate(id);
 652|        }
 653|        self.reconciliation_snapshot(record)
 654|    }
...
 893|                    drop(store);
*894|                    self.delegation_observations.invalidate(child.id());
 895|                    return self.reconciliation_snapshot(record);
 896|                }
 897|                record.answer_outcome(&answer.request, DispatchOutcome::Applied, Some(input), None);
...
 920|        drop(store);
*921|        self.delegation_observations.invalidate(child.id());
 922|        self.reconciliation_snapshot(record)
 923|    }
 924|}
## dispatch.rs
 471|    if let Some(summary) = state.session_summary(id) {
*472|        state.delegation_observations.session_changed(&summary);
 473|        state
 474|            .dashboard
 475|            .try_send(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
## mod.rs
 249|    server_lifetime: String,
*250|    delegation_observations: delegation_observation::Observations,
 251|    callback_executable_sha256: Option<String>,
 252|    pub tasks: Option<Arc<TaskManager>>,
 253|    socket: PathBuf,
...
 261|    pub(crate) retained: parking_lot::Mutex<SessionStore>,
*262|    observations: parking_lot::Mutex<HashMap<(String, String), CheckoutObservation>>,
 263|    pub mutation_lock: Mutex<()>,
 264|    pub(crate) lifecycle: lifecycle::Lifecycle,
 265|    operator: operator::OperatorRuntime,
...
 1611|            server_lifetime: navigation::new_identity().unwrap(),
*1612|            delegation_observations: delegation_observation::Observations::default(),
 1613|            callback_executable_sha256: Some("0".repeat(64)),
 1614|            tasks: Some(tasks),
 1615|            quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
...
 1623|            retained: parking_lot::Mutex::new(retained),
*1624|            observations: parking_lot::Mutex::new(HashMap::new()),
 1625|            mutation_lock: Mutex::new(()),
 1626|            lifecycle: lifecycle::Lifecycle::default(),
 1627|            operator: operator::OperatorRuntime::default(),
...
 2634|
*2635|    pub(crate) fn observe_checkouts(&self) {
 2636|        if self.stopping.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
 2637|            return;
 2638|        }
...
 2661|        let current = self.registry.lock().unwrap().clone();
*2662|        let mut cache = self.observations.lock();
 2663|        cache.retain(|(project, id), obs| {
 2664|            current.projects.iter().any(|candidate| {
 2665|                candidate.name == *project
...
 3038|    let registry = state.registry.lock().unwrap().clone();
*3039|    let observations = state.observations.lock().clone();
 3040|    let mut sessions = state.session_summaries();
 3041|    sessions.retain(|session| {
 3042|        !session.archived && session.scope == ovrcr_protocol::SessionScope::Workspace
...
 3051|                .map(|workspace| {
*3052|                    let observed = observations
 3053|                        .get(&(project.name.clone(), workspace.id.clone()))
 3054|                        .filter(|obs| {
 3055|                            obs.path == workspace.path && obs.git_identity == workspace.git_identity
## startup.rs
 241|        server_lifetime: super::navigation::new_identity()?,
*242|        delegation_observations: super::delegation_observation::Observations::default(),
 243|        callback_executable_sha256: std::env::current_exe().ok().and_then(|path| {
 244|            super::navigation::bridge_executable_sha256(
 245|                &path,
...
 259|        retained: parking_lot::Mutex::new(retained),
*260|        observations: parking_lot::Mutex::new(HashMap::new()),
 261|        mutation_lock: Mutex::new(()),
 262|        lifecycle: lifecycle::Lifecycle::default(),
 263|        operator: operator::OperatorRuntime::default(),
## tests.rs
 2644|            server_lifetime: super::navigation::new_identity().unwrap(),
*2645|            delegation_observations: delegation_observation::Observations::default(),
 2646|            callback_executable_sha256: Some("0".repeat(64)),
 2647|            tasks: None,
 2648|            socket: PathBuf::from("/tmp/ovrcr-test.sock"),
...
 2656|            retained: parking_lot::Mutex::new(SessionStore::in_memory()),
*2657|            observations: parking_lot::Mutex::new(HashMap::new()),
 2658|            mutation_lock: Mutex::new(()),
 2659|            lifecycle: lifecycle::Lifecycle::default(),
 2660|            operator: operator::OperatorRuntime::default(),
...
 2696|            server_lifetime: super::navigation::new_identity().unwrap(),
*2697|            delegation_observations: delegation_observation::Observations::default(),
 2698|            callback_executable_sha256: Some("0".repeat(64)),
 2699|            tasks: None,
 2700|            socket,
...
 2708|            retained: parking_lot::Mutex::new(SessionStore::in_memory()),
*2709|            observations: parking_lot::Mutex::new(HashMap::new()),
 2710|            mutation_lock: Mutex::new(()),
 2711|            lifecycle: lifecycle::Lifecycle::default(),
 2712|            operator: operator::OperatorRuntime::default(),
...
 5354|        server_lifetime: super::navigation::new_identity().unwrap(),
*5355|        delegation_observations: delegation_observation::Observations::default(),
 5356|        callback_executable_sha256: Some("0".repeat(64)),
 5357|        tasks: None,
 5358|        quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
...
 5366|        retained: parking_lot::Mutex::new(SessionStore::in_memory()),
*5367|        observations: parking_lot::Mutex::new(HashMap::new()),
 5368|        mutation_lock: Mutex::new(()),
 5369|        lifecycle: lifecycle::Lifecycle::default(),
 5370|        operator: operator::OperatorRuntime::default(),
...
 5577|        server_lifetime: super::navigation::new_identity().unwrap(),
*5578|        delegation_observations: delegation_observation::Observations::default(),
 5579|        callback_executable_sha256: Some("0".repeat(64)),
 5580|        tasks: None,
 5581|        quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
...
 5589|        retained: parking_lot::Mutex::new(SessionStore::in_memory()),
*5590|        observations: parking_lot::Mutex::new(HashMap::new()),
 5591|        mutation_lock: Mutex::new(()),
 5592|        lifecycle: lifecycle::Lifecycle::default(),
 5593|        operator: operator::OperatorRuntime::default(),
...
 8633|        server_lifetime: super::navigation::new_identity().unwrap(),
*8634|        delegation_observations: delegation_observation::Observations::default(),
 8635|        callback_executable_sha256: Some("0".repeat(64)),
 8636|        tasks: None,
 8637|        socket: dir.path().join("socket"),
...
 8645|        retained: parking_lot::Mutex::new(retained),
*8646|        observations: parking_lot::Mutex::new(HashMap::new()),
 8647|        mutation_lock: Mutex::new(()),
 8648|        lifecycle: lifecycle::Lifecycle::default(),
 8649|        operator: operator::OperatorRuntime::default(),
...
 9057|        server_lifetime: super::navigation::new_identity().unwrap(),
*9058|        delegation_observations: delegation_observation::Observations::default(),
 9059|        callback_executable_sha256: Some("0".repeat(64)),
 9060|        tasks: None,
 9061|        quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
...
 9069|        retained: parking_lot::Mutex::new(retained),
*9070|        observations: parking_lot::Mutex::new(HashMap::new()),
 9071|        mutation_lock: Mutex::new(()),
 9072|        lifecycle: lifecycle::Lifecycle::default(),
 9073|        operator: operator::OperatorRuntime::default(),
...
 9157|            server_lifetime: super::navigation::new_identity().unwrap(),
*9158|            delegation_observations: delegation_observation::Observations::default(),
 9159|            callback_executable_sha256: Some("0".repeat(64)),
 9160|            tasks: None,
 9161|            quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
...
 9169|            retained: parking_lot::Mutex::new(retained),
*9170|            observations: parking_lot::Mutex::new(HashMap::new()),
 9171|            mutation_lock: Mutex::new(()),
 9172|            lifecycle: lifecycle::Lifecycle::default(),
 9173|            operator: operator::OperatorRuntime::default(),
...
 9262|        server_lifetime: super::navigation::new_identity().unwrap(),
*9263|        delegation_observations: delegation_observation::Observations::default(),
 9264|        callback_executable_sha256: Some("0".repeat(64)),
 9265|        tasks: None,
 9266|        quotas: Mutex::new(ovrcr_protocol::QuotaSnapshot::default()),
...
 9274|        retained: parking_lot::Mutex::new(retained),
*9275|        observations: parking_lot::Mutex::new(HashMap::new()),
 9276|        mutation_lock: Mutex::new(()),
 9277|        lifecycle: lifecycle::Lifecycle::default(),
 9278|        operator: operator::OperatorRuntime::default(),

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/mod.rs:2600-2700"}
    fn remove_workspace_locked(
…
    ) -> Result<()> {
…
        let archived = self.retained.lock().archive_workspace(&next, project, name, || {
            git::remove_worktree(&project_record, &workspace, force).with_context(|| {
…
            })?;
            removed = true;
            Ok(())
        }).map_err(|error| {
            if removed || !workspace.path.exists() {
                lifecycle_error_with_hierarchy(
                    ErrorCode::PartialFailure,
                    format!("worktree is unavailable at {} but removal metadata was not committed; workspace and session records are retained for recovery: {}", workspace.path.display(), error_chain_string(&error)),
                    true,
                )
            } else {
                error
            }
        })?;
        *registry = next;
        drop(registry);
        // The worktree is gone; only now persist the forced acknowledgement, by
        // each row's current run (archiving just bumped the unarchived ones), so
        // a Git refusal above leaves no acknowledgement behind.
        {
            let mut retained = self.retained.lock();
            for session in &uncertain {
                if let Some(run) = retained.get(session.id).map(|record| record.run) {
                    retained.mark_stopped(session.id, run)?;
                }
            }
        }
        for id in archived {
            if let Some(session) = self.sessions.lock().unwrap().remove(&id) {
                session.revoke_hook_capability();
            }
            self.dashboard.forget_session(id);
        }
        Ok(())
    }

    pub(crate) fn observe_checkouts(&self) {
        if self.stopping.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return;
        }
        let captured = self.registry.lock().unwrap().clone();
        let mut computed = HashMap::new();
        for project in &captured.projects {
            for workspace in &project.workspaces {
                let root = is_root_workspace(project, workspace);
                let name = git::checkout_name(&workspace.path)
                    .unwrap_or_else(|_| git::UNAVAILABLE_CHECKOUT.to_owned());
                computed.insert(
                    (project.name.clone(), workspace.id.clone()),
                    CheckoutObservation {
                        path: workspace.path.clone(),
                        git_identity: workspace.git_identity.clone(),
                        name,
                        root,
                        warning: root.then(|| git::root_warning(project)).flatten(),
                    },
                );
            }
        }
        if self.stopping.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return;
        }
        let current = self.registry.lock().unwrap().clone();
        let mut cache = self.observations.lock();
        cache.retain(|(project, id), obs| {
            current.projects.iter().any(|candidate| {
                candidate.name == *project
                    && candidate.workspaces.iter().any(|workspace| {
                        workspace.id == *id
                            && workspace.path == obs.path
                            && workspace.git_identity == obs.git_identity
                    })
            })
        });
        for (key, obs) in computed {
            let Some(project) = current
                .projects
                .iter()
                .find(|project| project.name == key.0)
            else {
                continue;
            };
            let Some(workspace) = project
                .workspaces
                .iter()
                .find(|workspace| workspace.id == key.1)
            else {
                continue;
            };
            if workspace.path != obs.path || workspace.git_identity != obs.git_identity {
                continue;
            }
            cache.insert(key, obs);
        }
    }

    fn refuse_root_launch(&self, cwd: &Path) -> Result<()> {
        let projects = self.registry.lock().unwrap().projects.clone();
        for project in &projects {
            if !same_path(cwd, &project.repo) {
                continue;
            }
            if let Some(warning) = git::root_warning(project) {
                return Err(lifecycle_error(ErrorCode::Conflict, warning));
            }
        }
…
    }

[Showing lines 2599-2703 of 3142. Use :2704 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/settings.rs", "pattern": "LaunchChoice|launch_choices|preset"}
 1518|/// A plain enum on the wire: bincode cannot decode serde-tagged enums. The
*1519|/// settings loader reads the document's `kind`/`preset` form itself.
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
...
 1716|            cursor_cloud_accounts: Vec::new(),
*1717|            launch_choices: BTreeMap::new(),
 1718|            quota: QuotaSettings::default(),
 1719|            delegation: DelegationSettings::default(),
 1720|            save_uncommitted_work: false,

SOURCE LOOKUP {"path": "crates/ovrcr-tui/src/dashboard/palette.rs", "pattern": "LaunchChoice|launch_choices|CreateSession|SessionLaunch|AgentEntry|detected_agents|CreateWorkspaceWithLaunch"}
 12|use crate::protocol::{
*13|    BranchRequest, ClientMessage, CreateSessionRequest, Request, Response, SessionKind,
*14|    SessionLaunch, SessionPhase, SessionRunId, SessionSummary, new_workspace_id,
 15|};
 16|use crate::session::SessionId;
 17|use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
...
 149|    workspace_id: Option<String>,
*150|    pub(super) launch_preference: Option<(String, super::settings::LaunchChoice)>,
*151|    failed_launch: Option<CreateSessionRequest>,
 152|    launch_project: Option<String>,
 153|    suggestions: Suggestions,
 154|    // Some(true) retains explicit whole-form Submit; Some(false) replays Enter.
...
 2408|                            &mut palette,
*2409|                            Request::CreateSession(previous),
 2410|                            history_before_request,
 2411|                        );
 2412|                        self.palette = Some(palette);
...
 2477|                                }
*2478|                                Request::CreateSession(CreateSessionRequest {
 2479|                                    project: workspace.project.clone(),
 2480|                                    workspace: workspace.id.clone(),
 2481|                                    name: field("Name"),
...
 2511|                                };
*2512|                                Request::CreateWorkspaceWithLaunch {
 2513|                                    project: field("Project"),
 2514|                                    id,
 2515|                                    launch: selected_launch.clone(),
...
 2537|                                }
*2538|                                Request::CreateSession(CreateSessionRequest {
 2539|                                    project: previous.project.clone(),
 2540|                                    workspace: previous.workspace.clone(),
 2541|                                    name: String::new(),
...
 2617|
*2618|    pub(super) fn detected_agents(&self) -> Vec<super::agents::AgentEntry> {
 2619|        let path = std::env::var_os("PATH").unwrap_or_default();
 2620|        let shell = std::env::var_os("SHELL");
 2621|        let launcher = std::env::current_exe().ok();
...
 2635|                        entries.len().saturating_sub(2),
*2636|                        super::agents::AgentEntry {
 2637|                            name: format!("cursor-cloud:{}", account.name),
 2638|                            argv: vec![
 2639|                                launcher.as_os_str().to_owned(),
...
 2675|    fn launch_fields(&self, project: &str, empty: bool) -> Vec<Field> {
*2676|        let choice = self.settings.launch_choices.get(project);
 2677|        let start = match choice {
*2678|            Some(super::settings::LaunchChoice::Agent(_)) => "Agent",
 2679|            _ => "Terminal",
 2680|        };
 2681|        let mut choices = vec!["Agent", "Terminal"];
...
 2685|        let agents = self
*2686|            .detected_agents()
 2687|            .into_iter()
 2688|            .filter(|a| {
 2689|                a.source != AgentSource::Shell
...
 2695|        let agent = match choice {
*2696|            Some(super::settings::LaunchChoice::Agent(name)) => name.clone(),
 2697|            _ => agents.first().map(|a| a.name.clone()).unwrap_or_default(),
 2698|        };
 2699|        let mut fields = vec![
...
 2728|    fn shell_argv(&self) -> Vec<OsString> {
*2729|        self.detected_agents()
 2730|            .into_iter()
 2731|            .find(|entry| entry.source == AgentSource::Shell)
 2732|            .expect("agent detection always includes the shell entry")
...
 2735|
*2736|    fn selected_launch(&self, fields: &[Field]) -> Result<Option<SessionLaunch>, String> {
 2737|        let value = |label| {
 2738|            fields
 2739|                .iter()
...
 2746|            "Agent" => self
*2747|                .detected_agents()
 2748|                .into_iter()
 2749|                .find(|a| {
 2750|                    a.name == value("Agent")
...
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
 2837|        let launch = match &request {
*2838|            Request::CreateWorkspaceWithLaunch {
 2839|                project,
 2840|                id: name,
 2841|                launch: Some(launch),
 2842|                ..
*2843|            } => Some(CreateSessionRequest {
 2844|                project: project.clone(),
 2845|                workspace: name.clone(),
 2846|                name: String::new(),
...
 2850|            }),
*2851|            Request::CreateSession(session) => Some(session.clone()),
 2852|            _ => None,
 2853|        };
 2854|        if let Some(launch) = launch {
...
 2857|                match &launch.label {
*2858|                    Some(label) => super::settings::LaunchChoice::Agent(label.clone()),
*2859|                    None => super::settings::LaunchChoice::Terminal,
 2860|                },
 2861|            ));
 2862|            palette.failed_launch = Some(launch);
...
 3304|                        let request = super::settings::launch_choice_request(&project, &choice);
*3305|                        self.settings.launch_choices.insert(project, choice);
 3306|                        outgoing.push(ClientMessage {
 3307|                            request_id: self.error_owning_request_id(),
 3308|                            request,
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
 4539|            let message = request(&mut d);
*4540|            let Request::CreateSession(created) = message.request else {
 4541|                panic!("new conversation must create a separate session");
 4542|            };
 4543|            assert_eq!(created.project, "demo");
...
 4719|            match &m.request {
*4720|                Request::CreateSession(session) => {
 4721|                    assert_eq!(session.argv, argv, "{entrypoint}");
 4722|                    assert_eq!(session.label, None);
 4723|                }
*4724|                Request::CreateWorkspaceWithLaunch {
 4725|                    launch: Some(launch),
 4726|                    ..
 4727|                } => {
...
 4737|            assert_eq!(
*4738|                d.settings.launch_choices.get("demo"),
*4739|                Some(&LaunchChoice::Terminal),
 4740|                "{entrypoint}"
 4741|            );
 4742|        }
...
 4791|            let m = request(&mut d);
*4792|            let Request::CreateWorkspaceWithLaunch {
 4793|                project,
 4794|                id: name,
 4795|                launch,
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
*4900|            assert_eq!(d.settings.launch_choices["other.project"], other);
 4901|            assert_eq!(d.focused_session(), Some(SessionId(1)));
 4902|            assert!(d.palette.is_none());
 4903|            assert_eq!(d.mode, InputMode::Terminal);
...
 4913|        let m = request(&mut d);
*4914|        let Request::CreateSession(s) = &m.request else {
 4915|            panic!()
 4916|        };
 4917|        assert!(s.name.is_empty());
...
 4932|        }));
*4933|        assert!(d.settings.launch_choices.is_empty());
 4934|        assert!(saves_after_failure.is_empty());
 4935|        let m = request(&mut d);
 4936|        let (saves, _) = saves(d.handle_server_message(ServerMessage::Response {
...
 4943|            Request::SetSetting {
*4944|                path: "launch_choices.demo".into(),
 4945|                value: Some("{ kind = \"Terminal\" }".into()),
 4946|            }
 4947|        );
...
 4952|        let (saves, _) = saves(launch(&mut d, "Agent"));
*4953|        let saved = LaunchChoice::Agent("fixture-agent".into());
 4954|        assert_eq!(
 4955|            saves[0].request,
 4956|            super::super::settings::launch_choice_request("demo", &saved)
...
 4961|            .settings
*4962|            .launch_choices
 4963|            .insert("demo".into(), saved);
 4964|        restarted.open_create_terminal();
 4965|        let Page::Form { fields, .. } = &restarted.palette.as_ref().unwrap().page else {
...
 4982|        assert_eq!(
*4983|            restarted.settings.launch_choices.get("demo"),
*4984|            Some(&LaunchChoice::Agent("fixture-agent".into()))
 4985|        );
 4986|    }
 4987|
...
 5016|
*5017|    const REFUSED: &str = "could not save launch_choices.demo: settings document /x is read-only";
 5018|
 5019|    #[test]
 5020|    fn launch_save_warning_survives_selection_ack_and_session_accepts_input() {
...
 5126|                .iter()
*5127|                .all(|message| !matches!(message.request, Request::CreateSession(_)))
 5128|        );
 5129|    }
 5130|
...
 5133|        let mut d = dashboard();
*5134|        d.settings.launch_choices.insert(
 5135|            "demo".into(),
*5136|            LaunchChoice::Agent("missing-fixture-agent".into()),
 5137|        );
 5138|        start_workspace(&mut d, "Agent");
 5139|        assert!(matches!(submit(&mut d), DashboardAction::Redraw));
...
 5151|            request(&mut d).request,
*5152|            Request::CreateWorkspaceWithLaunch {
*5153|                launch: Some(SessionLaunch { label: None, .. }),
 5154|                ..
 5155|            }
 5156|        ));
...
 5196|            let m = request(&mut d);
*5197|            let Request::CreateSession(s) = m.request else {
 5198|                panic!("recovery must never recreate the workspace")
 5199|            };
 5200|            assert_eq!(s.workspace, "new-work");
...
 5563|        let launched = request(&mut d);
*5564|        assert!(matches!(launched.request, Request::CreateSession(_)));
 5565|        let mut retained = summary(7);
 5566|        retained.phase = SessionPhase::Stopped;
 5567|        retained.recovery = Some(SessionRecovery {
...
 5585|            match message.request {
*5586|                Request::CreateSession(_) | Request::AcknowledgeSessionStopped { .. } => {
 5587|                    panic!("uncertain create must not replay creation or acknowledge a guessed row")
 5588|                }
 5589|                other => panic!("unexpected request {other:?}"),
...
 5620|        });
*5621|        assert!(matches!(request(&mut d).request, Request::CreateSession(_)));
 5622|    }
 5623|
 5624|    #[test]
...
 5647|        assert!(
*5648|            matches!(request(&mut d).request, Request::CreateSession(_)),
 5649|            "Retry recovers by creating in the existing workspace, without stopped-process acknowledgement"
 5650|        );
 5651|    }
...
 5677|        let launched = request(&mut d);
*5678|        assert!(matches!(launched.request, Request::CreateSession(_)));
 5679|        d.handle_server_message(ServerMessage::Response {
 5680|            request_id: launched.request_id,
 5681|            response: Response::Error {
...
 5689|            match message.request {
*5690|                Request::CreateSession(_) | Request::AcknowledgeSessionStopped { .. } => {
 5691|                    panic!("uncertain create must not replay creation or acknowledge a guessed row")
 5692|                }
 5693|                other => panic!("unexpected request {other:?}"),
...
 5721|        let launched = request(&mut d);
*5722|        assert!(matches!(launched.request, Request::CreateSession(_)));
 5723|        d.handle_server_message(ServerMessage::Response {
 5724|            request_id: launched.request_id,
 5725|            response: Response::Error {
...
 5746|            match message.request {
*5747|                Request::CreateSession(_)
 5748|                | Request::AcknowledgeSessionStopped { .. }
 5749|                | Request::ReopenSession { .. } => {
 5750|                    panic!(

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/auto_trust.rs:1-100"}
//! Default auto-trust flags for managed Claude, Codex, Pi, and Oh My Pi launches.
//!
//! Callers append one flag only when argv has none of the permission, approve,
//! auto-trust, or dangerously-* family. An existing flag in that family wins.
//!
//! Codex defaults to `--approve-for-me` (Codex CLI >=0.155). Obsolete `--full-auto`
//! remains in the family presence check so a saved argv never gets a second flag.
use std::ffi::OsString;

/// The flag a managed launch adds when the caller omitted the permission family.
pub fn default_auto_trust_flag(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("--dangerously-skip-permissions"),
        "codex" => Some("--approve-for-me"),
        "pi" => Some("--approve"),
        "omp" => Some("--auto-approve"),
        _ => None,
    }
}

/// True when `argv` already names a permission, approve, auto-trust, or dangerously-* flag.
///
/// Codex's short `-a` is `--ask-for-approval`. The same token on another provider is not
/// that family. `--yolo` is Oh My Pi's alias of `--auto-approve`. Codex keeps obsolete
/// `--full-auto` in the family so old saved argv does not also receive `--approve-for-me`.
fn auto_trust_family_present(provider: &str, argv: &[OsString]) -> bool {
    argv.iter().any(|arg| {
        let Some(text) = arg.to_str() else {
            return false;
        };
        let name = text.split_once('=').map(|(name, _)| name).unwrap_or(text);
        name.starts_with("--dangerously-")
            || matches!(
                name,
                "--permission-mode"
                    | "--allow-dangerously-skip-permissions"
                    | "--approve"
                    | "--no-approve"
                    | "--auto-approve"
                    | "--yolo"
                    | "--approval-mode"
                    | "--approve-for-me"
                    | "--full-auto"
                    | "--ask-for-approval"
            )
            || (provider == "codex" && name == "-a")
    })
}

/// Append [`default_auto_trust_flag`] unless that family is already present.
///
/// The flag is inserted before a `--` prompt separator so admission still sees
/// one prompt. Existing flags are never replaced or duplicated.
pub fn apply_default_auto_trust(provider: &str, argv: &mut Vec<OsString>) {
    let Some(flag) = default_auto_trust_flag(provider) else {
        return;
    };
    if argv.is_empty()
        || argv.iter().any(|arg| arg.to_str().is_none())
        || auto_trust_family_present(provider, argv)
    {
        return;
    }
    let flag = OsString::from(flag);
    if let Some(index) = argv.iter().position(|arg| arg == "--") {
        argv.insert(index, flag);
    } else {
        argv.push(flag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn bare_managed_launches_gain_one_provider_flag() {
        let cases = [
            ("claude", "--dangerously-skip-permissions"),
            ("codex", "--approve-for-me"),
            ("pi", "--approve"),
            ("omp", "--auto-approve"),
        ];
        for (provider, flag) in cases {
            let mut argv = args(&["/opt/bin/agent"]);
            apply_default_auto_trust(provider, &mut argv);
            assert_eq!(argv, args(&["/opt/bin/agent", flag]), "{provider}");
        }
    }

    #[test]
    fn providers_without_an_equivalent_are_left_alone() {
        for provider in ["grok", "hermes", "cursor-agent", "gemini", "shell"] {
            let mut argv = args(&["/opt/bin/agent"]);
            apply_default_auto_trust(provider, &mut argv);
            assert_eq!(argv, args(&["/opt/bin/agent"]), "{provider}");
        }
    }

…
}

[Showing lines 1-103 of 180. Use :104 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/extension_recovery.rs", "pattern": "launch_options|--model|-m|prompt|unsupported|eligible|fn config_dir"}
 11|
*12|pub fn config_dir(provider: AgentProvider) -> Result<PathBuf> {
 13|    let default = match provider {
 14|        AgentProvider::Pi => ".pi/agent",
 15|        AgentProvider::Omp => {
...
 45|/// makes recovery unavailable rather than silently changing native behavior.
*46|pub fn launch_options(provider: AgentProvider, argv: &[OsString]) -> Result<Vec<String>> {
 47|    let mut options = Vec::new();
 48|    let mut index = 1;
 49|    while index < argv.len() {
...
 56|            continue;
*57|        } // task prompts are never retained
 58|        let (name, inline) = arg
 59|            .split_once('=')
 60|            .map_or((arg, None), |(a, b)| (a, Some(b)));
...
 71|            name,
*72|            "--model" | "--provider" | "--thinking" | "--tools" | "--models" | "--session-dir"
*73|        ) || (provider == AgentProvider::Omp && name == "--approval-mode");
 74|        if identity || value_option {
 75|            let value = if let Some(value) = inline {
 76|                value
...
 96|                            | "--no-context-files"
*97|                            | "--no-prompt-templates"
 98|                            | "--no-themes"
 99|                    ))
 100|                || (provider == AgentProvider::Omp
...
 132|    argv.extend(reference.options.iter().map(OsString::from));
*133|    if launch_options(provider, &argv)? != reference.options {
 134|        bail!("invalid retained provider configuration");
 135|    }
 136|    Ok(())
...
 205|/// An explicit native launch intent, obtained from managed-wrapper argv, not history custom entries.
*206|/// Only one absolute original session reference is eligible for automatic continuation.
 207|pub fn explicit_pi_resume(argv: &[OsString]) -> Result<Option<ExtensionConversation>> {
 208|    use std::os::unix::fs::OpenOptionsExt;
 209|    let mut selected = None;
...
 244|    let history = std::fs::canonicalize(history)?;
*245|    let options = launch_options(AgentProvider::Pi, argv)?;
 246|    let executable = argv
 247|        .first()
 248|        .map(PathBuf::from)
...
 291|    #[test]
*292|    fn recovery_preserves_explicit_controls_but_never_prompts_or_selectors() {
 293|        for provider in [AgentProvider::Pi, AgentProvider::Omp] {
 294|            let selector = if provider == AgentProvider::Pi {
 295|                "--session"
...
 301|            } else {
*302|                "--approval-mode=always-ask"
 303|            };
 304|            let argv: Vec<OsString> = [
 305|                provider.name(),
...
 307|                "/exact/history.jsonl",
*308|                "--model=some-model",
 309|                control,
 310|                "PRIVATE_PROMPT",
 311|            ]
...
 314|            .collect();
*315|            let options = launch_options(provider, &argv).unwrap();
 316|            assert_eq!(
 317|                options,
 318|                if provider == AgentProvider::Pi {
*319|                    vec!["--model", "some-model", "--no-approve"]
 320|                } else {
*321|                    vec!["--model", "some-model", "--approval-mode", "always-ask"]
 322|                }
 323|            );
 324|            for argument in [
 325|                "--api-key=PRIVATE",
*326|                "--system-prompt=PRIVATE",
 327|                "--profile=work",
 328|                "--config=relative.yml",
 329|                "--extension=custom.js",
...
 331|                assert!(
*332|                    launch_options(provider, &[provider.name().into(), argument.into()]).is_err(),
 333|                    "{argument}"
 334|                );
 335|            }

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/claude_delegation.rs:1-150"}
//! Native Claude policy identities and bounded pre-input model discovery.
//! No SDK, Session, Reporter lease or assignment publisher is created by discovery.
use anyhow::{Context, Result, bail, ensure};
use ovrcr_protocol::ResearchContract;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const ARTIFACT_ENV: &str = "OVRCR_CLAUDE_ARTIFACTS";
pub const POLICY: &str = "ovrcr-claude-research-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified: i64,
    modified_ns: i64,
}
fn file_identity(path: &Path) -> Result<FileIdentity> {
    let meta = fs::metadata(path)?;
    ensure!(
        meta.is_file() && meta.mode() & 0o111 != 0,
        "native Claude executable unavailable"
    );
    Ok(FileIdentity {
        device: meta.dev(),
        inode: meta.ino(),
        size: meta.len(),
        modified: meta.mtime(),
        modified_ns: meta.mtime_nsec(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelProof {
    pub requested: String,
    pub resolved: String,
    pub executable: PathBuf,
    target: PathBuf,
    binary_digest: String,
    file: FileIdentity,
    pub profile: PathBuf,
    profile_entry: PathBuf,
    profile_device: u64,
    profile_inode: u64,
    pub native_version: String,
    policy: String,
    // This identifies only model/backend-affecting policy, not complete launch
    // topology. Discovery excludes the assignment publisher and Reporter transport.
    policy_digest: String,
}

pub fn executable() -> Result<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").context("PATH unavailable")?) {
        let candidate = directory.join("claude");
        if candidate.is_file() {
            let entry = candidate
                .parent()
                .context("native executable has no directory")?
                .canonicalize()?
                .join("claude");
            file_identity(&entry)?;
            return Ok(entry);
        }
    }
    bail!("native Claude executable unavailable")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicyManifest {
    recipe: String,
    helper: PathBuf,
    settings: String,
    mcp: String,
}
pub struct PolicyFiles {
    pub settings: Vec<u8>,
    pub mcp: Vec<u8>,
    pub manifest: Vec<u8>,
}
pub fn policy_files(helper: &Path, handle: &str) -> Result<PolicyFiles> {
    let helper = helper.canonicalize()?;
    let settings = format!(
        "{}\n",
        serde_json::to_string_pretty(&crate::claude_recovery::reporter_settings(
            helper.to_str().context("non-UTF8 Reporter executable")?
        ))?
    )
    .into_bytes();
    let mcp = serde_json::to_vec(&serde_json::json!({"mcpServers":{"ovrcr_child":{
        "type":"stdio","command":helper,"args":["delegation-publisher","--read-handle",handle]
    }}}))?;
    let manifest = PolicyManifest {
        recipe: POLICY.into(),
        helper,
        settings: format!("{:x}", Sha256::digest(&settings)),
        mcp: format!("{:x}", Sha256::digest(&mcp)),
    };
    Ok(PolicyFiles {
        settings,
        mcp,
        manifest: serde_json::to_vec(&manifest)?,
    })
}

/// Only model/backend-affecting flags are shared with discovery; transport
/// differences are explicit and never claim identical complete MCP topology.
pub fn launch_argv(executable: &Path, model: &str, artifacts: &Path) -> Vec<std::ffi::OsString> {
    vec![
        executable.as_os_str().into(),
        "--model".into(),
        model.into(),
        "--permission-mode".into(),
        "default".into(),
        "--tools".into(),
        "".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        artifacts.join("claude-mcp.json").into_os_string(),
        "--settings".into(),
        artifacts.join("claude-settings.json").into_os_string(),
        "--setting-sources".into(),
        "".into(),
    ]
}

fn native_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:/[]-".contains(&byte))
}
fn version(value: &str) -> bool {
    let values: Option<Vec<u64>> = value.split('.').map(|part| part.parse().ok()).collect();
    values.is_some_and(|parts| parts.len() == 3 && parts.as_slice() >= [2, 1, 300].as_slice())
}
fn policy_digest(model: &str, profile: &Path) -> Result<String> {
    Ok(format!(
…
    ))
}

[Showing lines 1-153 of 547. Use :154 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/claude_delegation.rs:180-240"}
impl ModelProof {
    pub(crate) fn validate(&self, contract: &ResearchContract) -> Result<()> {
…
        ensure!(
            executable()? == self.executable
                && self.executable.canonicalize()? == self.target
                && file_identity(&self.executable)? == self.file,
            "native executable changed since model discovery"
        );
        let profile_entry = crate::claude_recovery::config_dir()?;
        let profile = profile_entry.canonicalize()?;
        let meta = fs::metadata(&profile)?;
        ensure!(
            profile_entry == self.profile_entry
                && meta.is_dir()
                && profile == self.profile
                && meta.dev() == self.profile_device
                && meta.ino() == self.profile_inode
                && policy_digest(&self.requested, &profile)? == self.policy_digest,
            "native profile or model policy changed since discovery"
        );
        Ok(())
    }
}

fn models(value: &serde_json::Value) -> Result<Vec<(String, String)>> {
    let list = value
        .as_array()
        .filter(|list| list.len() <= 512)
        .context("native model catalog unavailable")?;
    let mut found = std::collections::BTreeMap::new();
    for model in list {
        let requested = model["value"]
            .as_str()
            .filter(|value| native_id(value))
            .context("invalid native model alias")?;
        let Some(resolved) = model
            .get("resolvedModel")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        ensure!(native_id(resolved), "invalid native resolved model");
        ensure!(
            found
                .insert(requested.to_owned(), resolved.to_owned())
                .is_none(),
            "duplicate native model alias"
        );
    }
    Ok(found.into_iter().collect())
}

/// This remains preflight evidence, never a proof of managed-TUI tool inventory,
/// native authentication, helper access or actual model inference.
pub(crate) fn discover(requested: &str) -> Result<(Vec<(String, String)>, ModelProof)> {
    ensure!(native_id(requested), "invalid native model request");
    let executable = executable()?;
    let file = file_identity(&executable)?;
    let target = executable.canonicalize()?;
    let mut binary = Vec::new();
    File::open(&target)?
        .take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut binary)?;
    ensure!(
        binary.len() <= 512 * 1024 * 1024 && file_identity(&executable)? == file,
        "native executable exceeds bounds or changed"
    );
…
}

[Showing lines 179-243 of 547. Use :244 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/delegation.rs:790-870"}

    pub(crate) fn reject_assigned_input(&self, id: SessionId) -> Result<()> {
        if self.session_for_control(id)?.has_assigned_input() {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "Pi owns this assignment; generic terminal input is disabled",
            ));
        }
        Ok(())
    }

    pub(crate) fn delegation_capabilities(
        &self,
        auth: &DelegationAuth,
        selection: &DelegationSelection,
    ) -> Result<DelegationCapabilities> {
        self.delegation_auth(auth)?;
        self.reject_assigned_input(auth.session)?;
        let (settings, invalid) = self.policy_snapshot();
        let harness = selection.provider.unwrap_or(settings.default_harness);
        let discovered = match harness {
            DelegationHarness::Pi => available_models(),
            DelegationHarness::Codex
                if selection
                    .model_provider
                    .as_deref()
                    .is_none_or(|p| p == "openai") =>
            {
                crate::codex_delegation::models()
            }
            DelegationHarness::Codex => bail!(
                "Codex delegation supports only the native OpenAI backend; no substitute selected"
            ),
            DelegationHarness::Claude => {
                if selection
                    .model_provider
                    .as_deref()
                    .is_some_and(|backend| backend != "native")
                {
                    Err(anyhow::anyhow!(
                        "Claude requires the explicit native backend; no substitute selected"
                    ))
                } else {
                    crate::claude_delegation::discover(
                        selection.model.as_deref().unwrap_or("default"),
                    )
                    .map(|(models, _)| {
                        models
                            .into_iter()
                            .map(|(model, _)| DelegationChoice {
                                provider: DelegationHarness::Claude,
                                model_provider: "native".into(),
                                model,
                            })
                            .collect()
                    })
                }
            }
            _ => bail!("requested delegation harness is unavailable; no substitute selected"),
        };
        let (inventory, unavailable) = match discovered {
            Ok(inventory) => (inventory, invalid),
            Err(_) => (
                Vec::new(),
                Some(if harness == DelegationHarness::Claude {
                    if selection
                        .model_provider
                        .as_deref()
                        .is_some_and(|backend| backend != "native")
                    {
                        "Claude requires the explicit native backend; no substitute selected".into()
                    } else {
                        "native Claude model/policy preflight is unavailable; no substitute selected"
                            .into()
                    }
                } else {
                    format!(
                        "{} model inventory or required native policy is unavailable; no substitute selected",
                        harness.name()
                    )
                }),
            ),
        };
        let (live, children) = self.delegation_live_counts()?;
        Ok(policy::resolve(
…
        ))
    }

[Showing lines 789-873 of 3634. Use :874 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/mod.rs:830-1000"}
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
                    self.dashboard.forget_session(record.id);
                }
                Err(error)
            }
            Err(cleanup) => Err(error.context(format!("session cleanup failed: {cleanup:#}"))),
        }
    }

    fn check_live_capacity(&self) -> Result<()> {
        if self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.is_live())
            .count()
            >= MAX_LIVE_SESSIONS
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "all 50 live process slots are occupied",
            ));
        }
        Ok(())
    }

    /// The mutation lock is the slot reservation: no other admission can pass while spawning.
    fn spawn_record_locked(
        &self,
        id: SessionId,
        expected_run: SessionRunId,
        argv: Vec<std::ffi::OsString>,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
        restore_command: Option<String>,
    ) -> Result<SessionSummary> {
        self.spawn_prepared_record_locked(id, expected_run, argv, ready, restore_command, false)
    }

    fn spawn_prepared_record_locked(
        &self,
…
    ) -> Result<SessionSummary> {
…
    }

[Showing lines 829-1003 of 3142. Use :1004 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src", "pattern": "struct SessionMetadata|argv|SessionLaunch|launch"}
# crates/ovrcr-runtime/src/
## agent_runner.rs
 29|pub fn run_native(
*30|    argv: &[OsString],
 31|    channel_ready: impl FnOnce(bool, &mut Vec<OsString>) -> Option<HookHandler>,
 32|) -> Result<ExitStatus> {
*33|    run_native_with_env(argv, &[], move |available, native_argv, _environment| {
*34|        channel_ready(available, native_argv)
 35|    })
 36|}
 37|
...
 48|pub fn run_native_with_env(
*49|    argv: &[OsString],
 50|    environment: &[(OsString, OsString)],
 51|    channel_ready: impl FnOnce(
 52|        bool,
...
 56|) -> Result<ExitStatus> {
*57|    run_native_admitted(argv, environment, move |available, argv, environment| {
*58|        Ok(channel_ready(available, argv, environment))
 59|    })
 60|}
 61|
...
 63|pub fn run_native_admitted(
*64|    argv: &[OsString],
 65|    environment: &[(OsString, OsString)],
 66|    channel_ready: impl FnOnce(
 67|        bool,
...
 71|) -> Result<ExitStatus> {
*72|    argv.first().context("native command is required")?;
 73|    let mut signals = Signals::new([
 74|        libc::SIGINT,
 75|        libc::SIGTERM,
...
 82|    let mut channel = InvocationChannel::new().ok();
*83|    let mut argv = argv.to_vec();
 84|    // The caller releases reporting ownership before an untracked native spawn.
 85|    let mut native_environment = environment.to_vec();
*86|    let handler = channel_ready(channel.is_some(), &mut argv, &mut native_environment)?;
*87|    let executable = argv.first().context("native command is required")?;
 88|    let mut command = Command::new(executable);
 89|    command
*90|        .args(&argv[1..])
 91|        .stdin(Stdio::inherit())
 92|        .stdout(Stdio::inherit())
 93|        .stderr(Stdio::inherit())
...
 340|        let (initialize, ready) = std::sync::mpsc::sync_channel::<Option<HookHandler>>(1);
*341|        // Start the receiver before reporting setup can authorize argv changes.
 342|        let thread = std::thread::Builder::new().spawn(move || {
 343|            use std::{io::Write, sync::atomic::Ordering, time::Instant};
 344|            let Ok(mut handler) = ready.recv() else {
## claude_delegation.rs
 60|    policy: String,
*61|    // This identifies only model/backend-affecting policy, not complete launch
 62|    // topology. Discovery excludes the assignment publisher and Reporter transport.
 63|    policy_digest: String,
 64|}
...
 121|/// differences are explicit and never claim identical complete MCP topology.
*122|pub fn launch_argv(executable: &Path, model: &str, artifacts: &Path) -> Vec<std::ffi::OsString> {
 123|    vec![
 124|        executable.as_os_str().into(),
 125|        "--model".into(),
## claude_recovery.rs
 47|/// as non-secret references. Inline settings and system prompts are not persisted.
*48|pub fn launch_options(argv: &[OsString]) -> Result<Vec<String>> {
 49|    let mut options = Vec::new();
 50|    let mut index = 1;
 51|    let mut prompt = false;
*52|    while index < argv.len() {
*53|        let arg = argv[index].to_str().context("non-UTF8 Claude option")?;
 54|        if arg == "--" {
*55|            if prompt || index + 2 != argv.len() {
 56|                bail!("unsupported Claude prompt arguments");
 57|            }
 58|            break;
...
 81|                    index += 1;
*82|                    argv.get(index)
 83|                        .and_then(|value| value.to_str())
 84|                        .context("missing Claude option value")?
 85|                };
...
 109|            }
*110|            _ => bail!("Claude launch configuration cannot be retained as non-secret references"),
 111|        }
 112|        index += 1;
 113|    }
...
 145|    }
*146|    let mut argv = vec![reference.executable.clone().into_os_string()];
*147|    argv.extend(reference.options.iter().map(OsString::from));
*148|    if launch_options(&argv)? != reference.options {
 149|        bail!("invalid retained Claude configuration");
 150|    }
 151|    Ok(())
...
 153|
*154|pub fn resume_argv(reference: &ClaudeConversation) -> Result<Vec<OsString>> {
 155|    validate(reference)?;
 156|    if config_dir()? != reference.config_dir {
 157|        bail!("Claude configuration changed; restore the recorded CLAUDE_CONFIG_DIR before Retry");
...
 169|    }
*170|    let mut argv = vec![
 171|        std::env::current_exe()?.into_os_string(),
 172|        "agent".into(),
 173|        "run".into(),
...
 179|    ];
*180|    argv.extend(reference.options.iter().map(OsString::from));
*181|    Ok(argv)
 182|}
 183|
 184|#[cfg(test)]
...
 188|    #[test]
*189|    fn saved_launch_options_drop_task_prompts_and_refuse_inline_configuration() {
 190|        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
 191|        assert_eq!(
*192|            launch_options(&args(&[
 193|                "claude",
 194|                "--model=sonnet",
 195|                "--permission-mode",
...
 202|        assert!(
*203|            launch_options(&args(&["claude", "--", "private task prompt"]))
 204|                .unwrap()
 205|                .is_empty()
 206|        );
...
 214|            assert!(
*215|                launch_options(&args(&["claude", option, "{private inline configuration}"]))
 216|                    .is_err(),
 217|                "{option}"
 218|            );
...
 226|        std::fs::write(&settings, "{\"secret\":\"do-not-copy\"}").unwrap();
*227|        let options = launch_options(&[
 228|            "claude".into(),
 229|            "--settings".into(),
 230|            settings.clone().into_os_string(),
...
 246|        assert!(
*247|            launch_options(&[
 248|                "claude".into(),
 249|                "--settings".into(),
 250|                settings.into_os_string()
...
 259|        std::fs::write(&config, "{\"mcpServers\":{}}").unwrap();
*260|        let options = launch_options(&[
 261|            "claude".into(),
 262|            "--tools".into(),
 263|            "".into(),
## codex_recovery.rs
*1|//! Exact Codex recovery through the shared managed launch path. No history discovery.
 2|use anyhow::{Context, Result, bail};
 3|use ovrcr_protocol::CodexConversation;
 4|use std::{
...
 22|
*23|/// Keep only options supported by both fresh and exact resume launches. Prompts
 24|/// and inline configuration never enter the durable metadata store.
*25|pub fn launch_options(argv: &[OsString]) -> Result<Vec<String>> {
 26|    let mut options = Vec::new();
*27|    let mut args = argv.iter().skip(1);
 28|    while let Some(arg) = args.next() {
 29|        let arg = arg.to_str().context("non-UTF8 Codex option")?;
 30|        match arg {
...
 46|            value if !value.starts_with('-') => {} // task prompt
*47|            _ => bail!("Codex launch configuration cannot be retained as non-secret references"),
 48|        }
 49|    }
 50|    Ok(options)
...
 80|    }
*81|    let mut argv = vec![reference.executable.clone().into_os_string()];
*82|    argv.extend(reference.options.iter().map(OsString::from));
*83|    if launch_options(&argv)? != reference.options {
 84|        bail!("invalid retained Codex configuration");
 85|    }
 86|    Ok(())
...
 113|
*114|pub fn resume_argv(reference: &CodexConversation) -> Result<Vec<OsString>> {
 115|    validate(reference)?;
 116|    if !is_exact_identity(&reference.conversation) {
 117|        bail!("Codex recovery requires an exact canonical UUID");
...
 142|    validate_history(reference)?;
*143|    let mut argv = vec![
 144|        std::env::current_exe()?.into_os_string(),
 145|        "agent".into(),
 146|        "run".into(),
...
 152|    ];
*153|    argv.extend(reference.options.iter().map(OsString::from));
*154|    Ok(argv)
 155|}
 156|
*157|/// Prepare this managed Codex launch so reporters can reach the private channel.
 158|///
 159|/// Codex CLI 0.158 runs hooks from a detached `app-server --managed-daemon` that
 160|/// captures environment at daemon start and can outlive a prior TUI. Stop any
...
 163|/// `CODEX_HOME` (recovery compares the recorded directory exactly).
*164|pub fn prepare_managed_launch() {
 165|    stop_stale_app_server_daemon();
 166|}
 167|
...
 269|        assert_eq!(
*270|            launch_options(&args(&[
 271|                "codex",
 272|                "-m",
 273|                "gpt-5",
...
 298|        assert_eq!(
*299|            launch_options(&args(&[
 300|                "codex",
 301|                "--profile",
 302|                "ovrcr-research",
...
 319|        assert_eq!(
*320|            launch_options(&args(&["codex", "--full-auto"])).unwrap(),
 321|            ["--full-auto"]
 322|        );
 323|        assert!(
*324|            launch_options(&args(&["codex", "--", "resume"]))
 325|                .unwrap()
 326|                .is_empty()
 327|        );
...
 334|            assert!(
*335|                launch_options(&args(&["codex", option])).is_err(),
 336|                "{option}"
 337|            );
 338|        }
*339|        assert!(launch_options(&args(&["codex", "-C", "relative"])).is_err());
 340|    }
 341|
 342|    #[test]
## extension_recovery.rs
 45|/// makes recovery unavailable rather than silently changing native behavior.
*46|pub fn launch_options(provider: AgentProvider, argv: &[OsString]) -> Result<Vec<String>> {
 47|    let mut options = Vec::new();
 48|    let mut index = 1;
*49|    while index < argv.len() {
*50|        let arg = argv[index].to_str().context("non-UTF8 provider option")?;
 51|        if arg == "--" {
 52|            break;
 53|        }
...
 78|                index += 1;
*79|                argv.get(index)
 80|                    .and_then(|v| v.to_str())
 81|                    .context("missing provider option value")?
 82|            };
...
 107|        } else {
*108|            bail!("provider launch configuration cannot be retained as non-secret references");
 109|        }
 110|        index += 1;
 111|    }
...
 130|    }
*131|    let mut argv = vec![reference.executable.clone().into_os_string()];
*132|    argv.extend(reference.options.iter().map(OsString::from));
*133|    if launch_options(provider, &argv)? != reference.options {
 134|        bail!("invalid retained provider configuration");
 135|    }
 136|    Ok(())
...
 138|
*139|pub fn resume_argv(
 140|    provider: AgentProvider,
 141|    reference: &ExtensionConversation,
 142|) -> Result<Vec<OsString>> {
...
 175|    };
*176|    let mut argv = vec![
 177|        std::env::current_exe()?.into_os_string(),
 178|        "agent".into(),
 179|        "run".into(),
...
 185|    ];
*186|    argv.extend(reference.options.iter().map(OsString::from));
*187|    Ok(argv)
 188|}
 189|
 190|/// Canonical paths identify the original native reference; copying its UUID does not.
...
 204|
*205|/// An explicit native launch intent, obtained from managed-wrapper argv, not history custom entries.
 206|/// Only one absolute original session reference is eligible for automatic continuation.
*207|pub fn explicit_pi_resume(argv: &[OsString]) -> Result<Option<ExtensionConversation>> {
 208|    use std::os::unix::fs::OpenOptionsExt;
 209|    let mut selected = None;
 210|    let mut index = 1;
*211|    while index < argv.len() {
*212|        let arg = argv[index].to_str().context("non-UTF8 Pi option")?;
 213|        if arg == "--" {
 214|            break;
 215|        }
...
 227|                index += 1;
*228|                argv.get(index)
 229|                    .and_then(|v| v.to_str())
 230|                    .context("missing explicit Pi session")?
 231|            };
...
 244|    let history = std::fs::canonicalize(history)?;
*245|    let options = launch_options(AgentProvider::Pi, argv)?;
*246|    let executable = argv
 247|        .first()
 248|        .map(PathBuf::from)
 249|        .context("missing Pi executable")?;
...
 303|            };
*304|            let argv: Vec<OsString> = [
 305|                provider.name(),
 306|                selector,
 307|                "/exact/history.jsonl",
## git.rs
 161|        Some(format!(
*162|            "repository root is on {actual}; expected default branch {expected}. Restore the default branch before launching new sessions here."
 163|        ))
 164|    }
 165|}
## grok_recovery.rs
 1|//! Grok history retention for the title path. Grok has no resume adapter: this module
*2|//! names the one file Grok's documented session store keeps for a managed launch and
 3|//! checks that the file belongs to the recorded conversation before anyone reads it.
 4|use anyhow::{Context, Result, bail};
 5|use ovrcr_protocol::GrokConversation;
## hermes_recovery.rs
 78|
*79|pub fn resume_argv(reference: &HermesConversation) -> Result<Vec<OsString>> {
 80|    validate(reference)?;
*81|    let mut argv = vec![reference.executable.clone().into_os_string()];
 82|    if let Some(profile) = profile_name(&reference.state_db) {
*83|        argv.push("-p".into());
*84|        argv.push(profile.into());
 85|    }
*86|    argv.push("--resume".into());
*87|    argv.push(reference.conversation.clone().into());
*88|    Ok(argv)
 89|}
 90|
 91|#[cfg(test)]
## recovery.rs
*1|//! Shared provider capability and launch boundary. Adapters only validate saved
*2|//! references and construct argv; ServerState remains the sole process owner.
 3|use anyhow::{Result, bail};
 4|use ovrcr_protocol::{AgentProvider, ConversationReference};
 5|use std::ffi::OsString;
...
 48|        None if provider == AgentProvider::Hermes => Some(
*49|            "Native resume is not available for hermes until a managed launch reports a session id"
 50|                .into(),
 51|        ),
 52|        None => None,
...
 70|
*71|pub fn resume_argv(name: &str, reference: &ConversationReference) -> Result<Vec<OsString>> {
 72|    if let Some(reason) = unavailable(name, Some(reference), false) {
 73|        bail!("{reason}");
 74|    }
 75|    match reference {
*76|        ConversationReference::Claude(reference) => crate::claude_recovery::resume_argv(reference),
*77|        ConversationReference::Codex(reference) => crate::codex_recovery::resume_argv(reference),
 78|        ConversationReference::Pi(reference) => {
*79|            crate::extension_recovery::resume_argv(AgentProvider::Pi, reference)
 80|        }
 81|        ConversationReference::Omp(reference) => {
*82|            crate::extension_recovery::resume_argv(AgentProvider::Omp, reference)
 83|        }
 84|        // `unavailable` refused it above; a Grok reference names a title source, not a resume.
 85|        ConversationReference::Grok(_) => bail!("Native resume is not available for grok"),
*86|        ConversationReference::Hermes(reference) => crate::hermes_recovery::resume_argv(reference),
 87|    }
 88|}
 89|
 90|/// Without a retained identity, let the native agent offer its own history picker.
*91|pub fn picker_argv(name: &str) -> Result<Vec<OsString>> {
 92|    let provider = AgentProvider::from_name(name)
 93|        .filter(|provider| supported(*provider))
 94|        .ok_or_else(|| anyhow::anyhow!("Native resume is not available for {name}"))?;
...
 105|/// Submit one literal command line to the interactive terminal, then press Enter.
*106|pub fn terminal_command(argv: &[OsString]) -> Result<String> {
*107|    let words = argv
 108|        .iter()
 109|        .map(|arg| {
 110|            let arg = arg
...
 136|            assert_eq!(
*137|                terminal_command(&picker_argv(provider).unwrap()).unwrap(),
 138|                command
 139|            );
 140|        }
*141|        assert!(picker_argv("unrecognized; exit").is_err());
 142|        let command = terminal_command(&[
 143|            "/usr/bin/printf".into(),
 144|            "%s".into(),
...
 211|        assert!(
*212|            resume_argv("grok", &grok)
 213|                .unwrap_err()
 214|                .to_string()
 215|                .contains("not available")
...
 224|            assert!(
*225|                resume_argv(provider.name(), &reference)
 226|                    .unwrap_err()
 227|                    .to_string()
 228|                    .contains("not available")
## retained.rs
 173|#[derive(Clone, Debug)]
*174|pub(crate) struct SessionMetadata {
 175|    pub scope: ovrcr_protocol::SessionScope,
 176|    pub project: String,
 177|    pub workspace: String,
...
 605|
*606|    /// Commit launch intent before any process exists. The caller holds admission ownership.
 607|    pub fn begin_run(&mut self, id: SessionId, expected: SessionRunId) -> Result<RetainedSession> {
 608|        let record = self.records.get(&id).context("session not found")?;
 609|        if record.run != expected || record.disposition == Disposition::Archived {
...
 627|            )
*628|            .context("retain session launch intent")?;
 629|        if changed != 1 {
 630|            bail!("session run changed");
 631|        }

## retained/
### operator.rs
 250|    /// Caller holds mutation_lock and has admitted one of the fifty live slots.
*251|    /// Persists launch intent and the user message atomically, without spawning.
 252|    pub(crate) fn submit_operator_turn(
 253|        &mut self,
 254|        id: OperatorConversationId,

## server/
### connections.rs
 766|            );
*767|            let launching = matches!(
 768|                request.as_ref(),
 769|                ovrcr_protocol::DelegationRequest::Launch { .. }
 770|            );
...
 780|                |snapshot| {
*781|                    if (launching && snapshot.identity.session.is_some())
 782|                        || (archiving
 783|                            && snapshot.archive_receipts.iter().any(|r| {
 784|                                matches!(r.outcome, ovrcr_protocol::ArchiveOutcome::Archived { .. })
...
 990|            branch,
*991|            launch,
 992|        } if dashboard && state.lifecycle.async_ready() => lifecycle::try_accept(
 993|            state,
 994|            request_id,
...
 998|                branch,
*999|                launch,
 1000|            },
 1001|        ),
 1002|        Request::CreateWorkspaceWithLaunch {
...
 1005|            branch,
*1006|            launch,
 1007|        } => {
 1008|            state
*1009|                .create_workspace_with_launch(project, name, branch, launch)
 1010|                .map_or_else(
 1011|                    |error| lifecycle_response_with_partial_hierarchy(state, error),
 1012|                    |summary| {
### dashboard.rs
 39|        if let Some(current) = slot.as_ref() {
*40|            // A relaunched GUI connects before the previous handler observes
 41|            // EOF. The peer is already gone; keeping the slot refuses the
 42|            // only dashboard that still exists.
 43|            if !peer_has_closed(&current.stream) {
### delegation.rs
 137|        // SendInitial freezes this binding before native model readiness. It
*138|        // has spent one launch, but has not crossed the input boundary yet.
 139|        // Recognize that journaled state without repairing unknown history.
 140|        self.execution_attempts.is_some_and(|count| count > 0)
 141|            && self.binding.as_ref().is_some_and(|binding| {
...
 411|        }
*412|        let launch = self.receipt(DispatchOperation::Launch).map(|r| r.outcome);
 413|        let initial = self.receipt(DispatchOperation::InitialInput);
 414|        let crossed = matches!(
*415|            launch,
 416|            Some(DispatchOutcome::Applied | DispatchOutcome::Unknown | DispatchOutcome::Partial)
 417|        ) || initial.is_some()
 418|            || !self.followups.is_empty()
...
 438|                && (crossed
*439|                    || self.identity.session.is_some() && launch != Some(DispatchOutcome::Rejected))
 440|            {
 441|                return None;
 442|            }
...
 469|                || prepare == Some(DispatchOutcome::Applied)
*470|                    && matches!(launch, None | Some(DispatchOutcome::Accepted))
 471|            {
 472|                Some(0)
 473|            } else {
...
 657|            let _ = self.intent(operation, &identity);
*658|            // Legacy allocations can differ from their original pre-launch request.
 659|            if self.receipt(operation).is_none() {
 660|                self.receipts.push(DispatchReceipt {
 661|                    request: self.identity.request.clone(),
...
 886|        }
*887|        // Capability discovery is outside lifecycle serialization. A concurrent launch
 888|        // still rechecks its durable status under the lock before creating anything.
 889|        let mut native_proof = None;
 890|        let availability = if let DelegationRequest::Launch { auth, handle, .. } = &request {
...
 1380|                        ErrorCode::Conflict,
*1381|                        "launch belongs to a retired parent epoch; reconcile without replay",
 1382|                    ));
 1383|                }
 1384|                self.retained.lock().save_assignment(&record)?;
*1385|                if take_fault("launch-after-intent", &handle).is_some() {
 1386|                    return self.reconciliation_snapshot(record);
 1387|                }
 1388|                if !record
...
 1408|                        record.outcome(DispatchOperation::Launch, DispatchOutcome::Rejected, None,
*1409|                            Some("native model or launch policy changed since Prepare; no substitute selected"));
 1410|                        self.retained.lock().save_assignment(&record)?;
 1411|                        return self.reconciliation_snapshot(record);
 1412|                    }
...
 1431|                            } else {
*1432|                                "delegation live-child limit or minimum free slots after launch prevents admission".into()
 1433|                            });
 1434|                            record.retryable_capacity_refusal =
 1435|                                record.spent_execution_attempts().is_some();
...
 1484|                            None,
*1485|                            Some("launch effect is unresolved; do not replay"),
 1486|                        );
 1487|                        self.retained.lock().save_assignment(&record)?;
*1488|                        if take_fault("launch-before-effect", &handle).is_some() {
 1489|                            return self.reconciliation_snapshot(record);
 1490|                        }
 1491|                        let coding = contract.coding.is_some();
...
 1496|                        };
*1497|                        let argv = if contract.provider == "claude" {
 1498|                            let proof = record
 1499|                                .native_model
 1500|                                .as_ref()
...
 1502|                            proof.validate(&contract)?;
*1503|                            let mut argv = vec![
 1504|                                "/usr/bin/env".into(),
 1505|                                format!(
 1506|                                    "{}={}",
...
 1516|                            ];
*1517|                            argv.extend(crate::claude_delegation::launch_argv(
 1518|                                &proof.executable,
 1519|                                &contract.model,
 1520|                                &record.root,
 1521|                            ));
*1522|                            argv
 1523|                        } else if contract.provider == "codex" {
 1524|                            let home = std::fs::canonicalize(&self.registry_path)?
 1525|                                .join("codex-children")
...
 1591|                                },
*1592|                                super::WorkspaceLaunch::Session(SessionLaunch {
*1593|                                    argv,
 1594|                                    label: Some("Pi coding".into()),
 1595|                                    kind: SessionKind::Agent { name: "pi".into() },
 1596|                                }),
...
 1606|                                    label: Some(format!("{} research", contract.provider)),
*1607|                                    argv,
 1608|                                    kind: SessionKind::Agent {
 1609|                                        name: contract.provider.clone(),
 1610|                                    },
### delegation_coding.rs
 212|        if crate::git::resolve_commit(&workspace.path, "HEAD")? != coding.base {
*213|            bail!("prelaunch coding workspace no longer has its exact original base");
 214|        }
 215|        if coding.creation == WorkspaceCreation::Registered {
 216|            let actual = self
...
 341|            if crate::git::resolve_commit(&workspace.path, "HEAD")? != resource.base {
*342|                bail!("coding retry commit anchor changed before launch");
 343|            }
 344|            crate::git::candidate::baseline::verify_workspace(&project, &workspace, &baseline)?;
 345|        } else if inspected.dirty {
*346|            bail!("coding retry workspace changed before launch");
 347|        }
 348|        Ok(Some(resource.clone()))
 349|    }
### delegation_policy.rs
 75|            "max_execution_attempts".into(),
*76|            "min_free_slots_after_launch".into(),
 77|        ],
 78|        native_helpers: false,
 79|        native_descendant_usage: false,
...
 124|        .saturating_sub(live_sessions)
*125|        .saturating_sub(response.effective_limits.min_free_slots_after_launch)
 126|        .min(
 127|            response
 128|                .effective_limits
...
 263|                max_execution_attempts: Some(10),
*264|                min_free_slots_after_launch: 2,
 265|                ..Default::default()
 266|            },
 267|            ..Default::default()
...
 293|    #[test]
*294|    fn malformed_disabled_or_unenforceable_policy_never_resolves_a_launch() {
 295|        let policy = DelegationSettings::default();
 296|        let selection = DelegationSelection {
 297|            required_limits: DelegationLimits {
### delegation_recovery.rs
 606|
*607|    /// This only records an existing caller's exact retained resume intent; it never launches.
 608|    pub(crate) fn record_workflow_reopen(
 609|        &self,
 610|        id: SessionId,
...
 935|                if workflow.is_some() && selected.is_empty() && gaps.is_empty() {
*936|                    bail!("workflow identity is missing; no work was launched or replayed");
 937|                }
 938|                drop(store);
 939|                let output = selected
### delegation_review.rs
 1|//! Parent review policy over existing assignments and scoped same-child input.
*2|//! This module never launches, waits for, archives, commits or publishes code.
 3|use super::*;
 4|
 5|const MAX_REVIEW_CANDIDATES: usize = 33;
...
 325|/// Fixed, axis-specific context. Sources are in contract.review.source_documents.
*326|/// The Server authenticates complete readable content before any reviewer launch.
 327|pub(super) fn review_scope(assignment: &ReviewAssignment) -> String {
 328|    let axis = match assignment.axis {
 329|        ReviewAxis::Standards => {
...
 336|    format!(
*337|...t base {}, sources, axis and round; no reviewer shell, hashing or decoding is required. Missing/unreadable content or necessary context not supplied means Blocked; never omit coverage or substitute HEAD/current checkout. {} Publish a typed review report copying the exact contract.review target (including view) and axis. Keep blocking findings identifiable across correction rounds. Do not implement fixes, reuse the implementation conversation, consult the other axis, or launch another outer review team.", [col 857]
 338|        assignment.axis,
 339|        assignment
 340|            .target
...
 698|
*699|    /// A refused zero-effect launch has no native context to contaminate a retry.
 700|    fn verify_review_retry(
 701|        &self,
 702|        auth: &DelegationAuth,
### lifecycle.rs
 3|use super::*;
*4|use ovrcr_protocol::{LifecycleOp, LifecycleOutcome, SessionLaunch};
 5|
 6|pub(super) enum JobKind {
 7|    CreateWorkspace {
...
 15|        branch: BranchRequest,
*16|        launch: Option<SessionLaunch>,
 17|    },
 18|    RemoveWorkspace {
 19|        project: String,
...
 170|                branch,
*171|                launch,
*172|            } => match state.create_workspace_with_launch(project, id, branch, launch) {
 173|                Ok(None) => (LifecycleOutcome::Succeeded, true),
 174|                Ok(Some(summary)) => (LifecycleOutcome::CreatedSession(Box::new(summary)), true),
 175|                Err(error) => outcome_from_error(error),
...
 232|                label: None,
*233|                argv: vec![OsString::from("/bin/true")],
 234|                kind: SessionKind::Terminal,
 235|            }),
 236|            42,
...
 281|                label: None,
*282|                argv: vec![OsString::from("/bin/true")],
 283|                kind: SessionKind::Terminal,
 284|            }),
 285|            1,

### delegation_review/
#### view.rs
 235|        .as_ref()
*236|        .context("complete readable review view missing; Blocked before launch")?;
 237|    reference.validate().map_err(anyhow::Error::msg)?;
 238|    let prefix = format!("review-view-{}-{}", reference.round, axis_name(axis));
 239|    if reference.path != root.join(format!("{prefix}.json")) {

Showing files 1-20 of 30+. Use skip=20 for the next page, or narrow paths/pattern.

[Some lines truncated to 512 bytes]

SOURCE LOOKUP {"path": "crates/ovrcr-runtime/src/server/lifecycle.rs:1-200"}
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
…
}

[Showing lines 1-203 of 295. Use :204 to continue]

SOURCE LOOKUP {"path": "crates/ovrcr-protocol/src/wire.rs", "pattern": "LifecycleOutcome|LifecycleFinished|LifecycleOp|CreatedSession|client_token"}
 443|    Hierarchy(HierarchySnapshot),
*444|    CreatedSession(Box<SessionSummary>),
 445|    Screen {
 446|        session: SessionId,
 447|        run: SessionRunId,
...
 493|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*494|pub enum LifecycleOp {
 495|    CreateWorkspace,
 496|    RemoveWorkspace,
 497|    CreateSession,
...
 501|
*502|/// Result of a finished Lifecycle job, correlated by `client_token`.
 503|#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
*504|pub enum LifecycleOutcome {
 505|    Succeeded,
*506|    CreatedSession(Box<SessionSummary>),
 507|    Failed { code: ErrorCode, message: String },
 508|}
 509|
...
 542|    },
*543|    /// A Lifecycle job finished. `client_token` is the request_id that accepted
 544|    /// the job on the Dashboard connection.
 545|    LifecycleCompleted {
*546|        client_token: u64,
*547|        op: LifecycleOp,
*548|        outcome: LifecycleOutcome,
 549|    },
 550|    OperatorChanged(crate::OperatorConversationId),
 551|}
...
 1079|            (
*1080|                "CreatedSession",
*1081|                Response::CreatedSession(Box::new(summary())),
 1082|            ),
 1083|            (
 1084|                "Screen",
...
 1256|                ServerEvent::LifecycleCompleted {
*1257|                    client_token: 7,
*1258|                    op: LifecycleOp::CreateSession,
*1259|                    outcome: LifecycleOutcome::Succeeded,
 1260|                },
 1261|            ),
 1262|        ]
...
 3398|        (
*3399|            "Response::CreatedSession",
 3400|            "02000100052f776f726b04000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a7265710503000000010103696e7604636f6e760101047475726e02010000000104010506000000",
 3401|        ),
 3402|        ("Response::Screen", "0301040301020107"),