# Branch-based agent session creation

Status: design confirmed by Kyle on 2026-11-05 (grilling Q18). Implementation and provider runs are not authorized by that confirmation.

Tracker specification: [OVRCR #389](https://github.com/xlyk/ovrcr/issues/389), published 2026-11-05 with `ready-for-agent`. It consolidates this design and the subsequent implementation-plan corrections; this publication does not claim runtime acceptance.

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
