# Workspace and agent session creation: observed flow and mockups

Published tracker specification: [OVRCR #389](https://github.com/xlyk/ovrcr/issues/389), `ready-for-agent`, 2026-11-05. It contains the seven to-spec sections, 60 user stories, implementation/test decisions and scope boundaries. Duplicate search found no matching issue; GitHub body and label were read back and verified. Local publication source: [tracker-spec.md](tracker-spec.md); receipt: [tracker-publication.json](tracker-publication.json). No application implementation or runtime acceptance was performed as part of publication.

2026-11-05 ticket publication: Kyle approved the eight tracer-bullet slices; they are published as [#391](https://github.com/xlyk/ovrcr/issues/391) through [#398](https://github.com/xlyk/ovrcr/issues/398), all `ready-for-agent`, with verified native blocking edges. [Ticket overview and receipts](ticket-drafts/README.md) map the approved draft numbers to actual issues. Parent #389 title/body/state/labels/comments were read back unchanged. No implementation started.

The shared design was explicitly confirmed in Q18 on 2026-11-05. Read the [confirmed specification](../../docs/superpowers/specs/2026-11-05-agent-session-creation-design.md) and [implementation plan](../../docs/superpowers/plans/2026-11-05-agent-session-creation.md) for the current contract. Earlier sections below retain the evolution of the design and may describe superseded alternatives. All eighteen actual submitted answers are preserved in [grilling-answers.txt](grilling-answers.txt).

2026-11-05 follow-up: a user-requested team of three Oh My Pi reviewers on `ollama-cloud/deepseek-v4.1-flash` returned **NEEDS REVISION** for the implementation plan. The primary agent verified and consolidated six plan corrections, distinguishing existing reusable mechanisms from unnecessary protocol expansion. See [team validation and source evidence](plan-validation-omp/review.md). The plan/design/application remain unchanged by this review; runtime acceptance remains unrun.

2026-11-05 subsequent follow-up: Kyle requested closing those gaps. The plan now contains concrete corrections and acceptance mapping for F1–F6; the [closure table](plan-validation-omp/review.md) distinguishes documentation closure from unimplemented runtime proof. This was a local documentation revision only, with no new external model calls or application changes. The initial team reports remain unchanged; no re-review/pass verdict is claimed.

## Grilling decision record

**Q18:** Yes — this is the shared design. Confirmation covers documentation and implementation planning, not application changes, provider runs or merging. The design interview is complete; implementation has not started.

Consolidation: CONTEXT.md gained Initial prompt and Harness default definitions. No ADR was added: the accepted UI choices preserve the existing ownership and identity architecture, and did not establish a new hard-to-reverse integration tradeoff. The plan explicitly requires proving the four distinct native launch contracts before promising model/prompt support.

The final interactive mockup was synchronized with submitted decisions: all existing picker rows are registered workspaces, root/detached labels are explicit, omp is selectable, shared checkout notice is inline, creation shows a pending state and blocks duplicate submission/cancellation, and untouched project defaults do not override an explicit harness/model pair. Names in the model menus remain examples. Code inspection, JavaScript syntax, branch-validation examples and local documentation-link/whitespace checks passed. Browser interaction/pixel verification of this product mockup remains unrun; the successfully tested dark answer sheet is separate evidence. No Rust suites or providers were run for prose/mockup-only changes.

Round 4 submitted 2026-11-05. Source: `/private/tmp/ovrcr-creation-grill-20261105/round-4-answers.txt`.

- **Q14:** full first-release model-selection and initial-prompt support is required for Claude Code, Codex, Pi **and Oh My Pi (`omp`)**. The addition of omp is Kyle's explicit note, not an inferred equivalence with Pi. Other current launchers remain capability-gated. Existing Claude/Codex/Pi discovery helpers are integration candidates, not proof of dashboard or omp support.
- **Q15:** preserve named configured launchers and cloud-account choices in the harness picker. Show configured/pinned models read-only unless a compatible override is proven; retain cloud published-source/account restrictions.
- **Q16:** allow explicitly labeled Harness default; remember that policy instead of pinning the last observed model, and show the resolved model when known.
- **Q17:** n opens New agent session; keep separately discoverable New terminal behavior; w opens standalone branch/workspace creation. Existing shell behavior remains outside this redesign.

Round 3 submitted 2026-11-05. Source: `/private/tmp/ovrcr-creation-grill-20261105/round-3-answers.txt`.

- **Q10:** include registered root and detached checkouts with explicit labels, preserving launch guards. An unavailable current target requires another explicit choice, not silent retargeting.
- **Q11, option b:** follow the selected checkout automatically if its branch changes. Update the label and launch in that same stable workspace on first Start; no added confirmation for branch-label drift. This supersedes the earlier recommendation to block/review. Existing root-default and ownership checks still apply; do not switch checkout identities to chase the old name.
- **Q12, option b:** keep Create branch open until creation completes. Show pending status and disable duplicate submission. Return on success; retain errors/inputs on failure. After submission, cancellation must not claim to abort the lifecycle job. This supersedes the proposed Back-while-running flow.
- **Q13:** keep pending initial-prompt text in memory for this launch, preserving it across Dashboard reconnects while Server lives. Do not persist the prompt for restart recovery or automatically replay after restart; report loss/uncertainty truthfully.

Round 2 submitted 2026-11-05. Source: `/private/tmp/ovrcr-creation-grill-20261105/round-2-answers.txt`.

- **Q5, option b:** list registered workspaces only, displayed by current branch and grouped by project. Do not list unregistered local/remote branches or auto-prepare them from the main picker. The original final mockup's prepare-on-start entry is superseded. Create new branch remains available in the child dialog.
- **Q6:** completing the child dialog commits branch and workspace creation; cancelling the parent afterward leaves those resources available.
- **Q7:** allow multiple agents in the checkout, with a clear inline notice naming/counting other running agents rather than another confirmation.
- **Q8:** keep current harnesses available; explain/gate unsupported model or prompt controls. Never discard a typed prompt or silently substitute a model.
- **Q9:** preserve explicit harness/model edits and prompt across project changes. Only untouched controls inherit destination-project defaults; incompatible choices remain visible errors.

Round 1 submitted 2026-11-05 through the redesigned local answer sheet. All four recommended choices were explicitly submitted; no notes. Source: `/private/tmp/ovrcr-creation-grill-20261105/redesigned/round-1-answers.txt`.

- **Q1:** preselect the currently selected feature branch when opening New agent session; show project/branch clearly. Root/detached/unavailable contexts remain to resolve.
- **Q2:** default new branch Base to the repository default branch's local state. Fetch/update behavior remains separate; do not assume `main`.
- **Q3:** Start authorizes running the optional prompt once through a supported harness mechanism, preserving native trust boundaries. This establishes product intent, not proof that a generic delivery mechanism exists.
- **Q4:** remember the last successfully launched harness and model per project. Unavailable choices require explicit correction; prompts are not reused. Interaction with explicit edits and project changes remains to resolve.

Added Initial prompt to CONTEXT.md as resolved terminology. No ADR created: these choices do not yet establish a hard-to-reverse architecture tradeoff. Implementation remains gated on the completed interview and confirmation of shared understanding.

Code findings informing the next round: Workspace has stable checkout identity and a mutable Git branch label (CONTEXT.md; architecture.md:68–84). Existing-branch worktree creation accepts local refs, not remote-only refs (runtime git.rs:386–397,722–735). Generic CreateSessionRequest/SessionLaunch contain argv/label/kind but no model or prompt (protocol wire.rs:167–183). Dashboard view-input readiness does not establish native provider startup readiness. Lifecycle jobs cannot be cancelled in v1; Esc dismisses status only (ADR 0010). These are constraints to design around, not reasons to reinterpret submitted answers.

## Final branch-based two-dialog mockup — 2026-11-05

Final preview: `/Users/xlyk/.codex/visualizations/2026/11/05/01a1ac84-fa65-7c01-85a4-7cd80aadf68b/branch-session-final.html`. This supersedes the earlier workspace-centric main picker. No application implementation is included.

1. Session dialog shows **Branch**, **Agent harness**, **Model**, **Prompt (optional)**. Branches are grouped by project, with exactly one **Create new branch…** entry. Existing branch selection determines project; no permanent Project field appears here.
2. Choosing Create new branch hides the session dialog and opens **Create branch**. Project defaults to the selected branch's project; Branch name is the only required typed value. Base is main by default and editable in collapsed Workspace options. Name has no invalid prefilled `feature/` value.
3. Create validates the name/base and rejects duplicates within that project. Errors keep the child dialog and inputs. Successful creation simulates a branch plus workspace, refreshes the parent's picker, selects the new entry and returns focus to Harness.
4. The session form is retained rather than rebuilt, preserving harness, model, prompt and session name. Cancel/Esc in Create returns to the prior branch without creating anything. Only one dialog is visible/focusable at a time.
5. Start reuses an existing workspace or prepares one for a selected existing branch without a workspace. The local preview displays chosen destination, harness/model and optional prompt. Native readiness/trust must precede eventual prompt delivery; no prompt is transmitted by the mockup.
6. Branch/workspace creation is committed separately from starting an agent. Cancelling the parent after Create leaves the newly created branch/workspace available. The mockup explicitly shows this outcome. A real implementation must reconcile ambiguous creation outcomes before retrying rather than duplicating resources.

```text
Wide dashboard                     60-column single active dialog
tree | + New agent session -----+  + Create branch -------------------------+
     | | Branch [feature/auth v]|  | Project     [consigint                v]|
     | | Harness [Claude Code v]|  | Branch name [feature/auth-refresh       ]|
     | | Model   [default     v]|  | > Workspace options                    |
     | | Prompt [              ]|  |                                         |
     | | > Advanced options     |  | [Create branch] [Cancel]                |
     | | [Start agent] [Cancel] |  +-----------------------------------------+
     | +------------------------+  Returns to session; draft stays intact.
```

At 80×24 prioritize the active form; at 60 columns hide the sidebar and show one full-width dialog. Expanded options need a scrolling body with fixed actions in the eventual TUI; below the proposed 60×18 floor use the existing review's truthful size message. HTML reflow is illustrative, not proof of terminal-cell layout.

Clutter: one active modal border, four main controls, no Project/Workspace duplication in session setup. Creation has two main controls and one collapsed options section. Text status distinguishes workspace-ready versus prepare-on-start without extra badges. Prompt Enter adds newline; Alt+Enter submits; Tab is trapped in the active dialog; Esc returns/cancels. All controls have keyboard paths.

Checks: JavaScript syntax passed; static checks confirm exactly two dialogs, one initially visible, one Create entry, four main session fields and unique IDs. Validation examples pass for valid names, project-scoped duplicate detection, missing base and six invalid names. No browser rendering or native TUI acceptance was claimed; earlier browser policy restriction was not bypassed. Example model menus are illustrative. The one-workspace-per-project/branch mapping is a design assumption to verify before implementation.

## Accepted direction and revised Quick start — 2026-11-05

Kyle preferred Quick start and requested exactly four main controls: select/create Workspace, Agent harness, Model, and optional Prompt. All remaining fields/options belong inside initially collapsed **Workspace options**. This supersedes the original Quick start's exposed project control and combined agent/model choice below; original exploration and alternatives remain as historical evidence.

Revised preview: `/Users/xlyk/.codex/visualizations/2026/11/05/01a1ac84-fa65-7c01-85a4-7cd80aadf68b/quick-start-revised.html`.

- Workspace picker contains existing workspaces and **Create new workspace** in the same control. Creating does not open a second mandatory form. A read-only destination line shows the generated branch/base; changing it is optional.
- Agent harness and Model are separate controls. Changing harness updates available model choices and preserves each harness's prior selection in the mock. Names are illustrative; implementation must obtain valid choices from the harness instead of hard-coding this example menu. Unavailable models must remain explicit errors, never silent substitutions.
- Optional multiline Prompt stays visible. Blank opens an interactive agent; nonblank is an initial prompt. Proposed delivery waits for native readiness/trust and occurs once, never replays on reattachment or automatic retry. The preview only displays it locally and sends nothing.
- Workspace options contains Project, Branch mode, Branch name, Base branch and optional Session name. Branch/base controls are hidden for existing workspaces. Project changes refresh workspace choices; no workspace from the old project remains selected.
- One Start action creates the workspace when needed and starts the chosen harness/model. Its label says **Create workspace & start** or **Start agent**. The preview shows a local launch receipt rather than launching anything.
- Keyboard order: Workspace → Harness → Model → Prompt → Workspace options → Start → Cancel. Expanding options inserts its fields after the disclosure. Prompt Enter inserts a newline; Alt+Enter starts; Esc cancels; Tab remains inside the form. Real terminal acceptance remains required.
- Wide: retain the sidebar and compact form. At 80×24 use a single-column active form and hide background content as needed; at 60 columns hide the sidebar and use full width. Keep four main controls plus actions visible, put expanded options in a scrollable body in the eventual TUI. This is a design requirement, not a measured native result.

```text
Wide form                          60-column single-view form
+ New agent session ---------+     + New agent session ---------------------+
| Workspace [Create new    v] |     | Workspace [Create new · consigint    v] |
| work/amber-fox from main    |     | New branch work/amber-fox from main     |
| Harness [Claude Code    v] |     | Agent harness [Claude Code           v] |
| Model   [Harness default v]|     | Model         [Harness default       v] |
| Prompt (optional)          |     | Prompt (optional)                       |
| [                        ] |     | [                                     ] |
| > Workspace options        |     | > Workspace options                     |
| [Create & start] [Cancel]  |     | [Create workspace & start] [Cancel]      |
+----------------------------+     +-----------------------------------------+
```

Revision checks passed: JavaScript syntax; main form has exactly Workspace/Harness/Model/Prompt; all five secondary fields are inside the sole initially closed options disclosure. Browser pixels and interactions were not rechecked because the earlier preview was blocked. No application code changed.

## Original exploration and alternatives

2026-11-05. Recommendation: use **Quick start**, a single contextual launcher with a valid generated branch, an explicit destination, and the project's last valid launch choice. No mandatory typing; one visible Start action. Keep inline creation as an alternative to evaluate, rather than introducing another required route.

## Evidence and scope

- Native CUA against `target/OVRCR GUI.app`, built from main at `08d25641d3ef96a2b8f9f74a5710efb2c532b75a`.
- Existing untracked orchestration plans, reports and `research_notes/` were left alone. No application source changed.
- Ran the actual disposable demo, not the user's live server. Screenshots are JPEG; matching full accessibility text uses the same basename.
- CUA explored project registration without submitting, menus, both creation forms, invalid branch submission, workspace plus Claude creation, and an additional shell in that workspace.
- Claude reached its native folder-trust prompt. No trust was granted and no work prompt was sent. Provider readiness, inference and billing were not verified.
- A separate `CUA_SESSION_UX_OK` output line proves input reached the newly created shell. This is not proof of agent readiness.

## What it looks like

| State | Screenshot | Accessibility |
|---|---|---|
| Dashboard: project/workspace/session tree left, terminal right | [Dashboard](01-dashboard.jpg) | [AX](01-dashboard.txt) |
| Menu: bottom-right which-key groups | [Menu](02-menu.jpg) | [AX](02-menu.txt) |
| Actions: centered searchable palette | [Actions](03-actions.jpg) | [AX](03-actions.txt) |
| Invalid default branch: form gone, failed row and footer error | [Validation failure](04-invalid-default.jpg) | [AX](04-invalid-default.txt) |
| Workspace: six visible fields for Terminal | [Workspace form](05-workspace-terminal.jpg) | [AX](05-workspace-terminal.txt) |
| Created workspace and agent selected in Terminal mode | [Creation result](06-workspace-created-agent.jpg) | [AX](06-workspace-created-agent.txt) |
| Existing workspace: agent launch under “Create terminal” | [Session form](07-existing-workspace-agent.jpg) | [AX](07-existing-workspace-agent.txt) |
| New shell accepts input | [Input proof](08-session-input.jpg) | [AX](08-session-input.txt) |
| Workspace: Agent replaces Command with agent picker | [Agent form](09-workspace-agent.jpg) | [AX](09-workspace-agent.txt) |
| Project registration prerequisite | [Registration](10-register-project.jpg) | [AX](10-register-project.txt) |
| Native provider trust handoff | [Handoff](11-provider-handoff.jpg) | [AX](11-provider-handoff.txt) |

The forms occupy a small fixed-looking central region despite the large surrounding window. Pick lists expand into the form's field area; earlier labels can scroll out. They look like text fields and initially filter to their prefilled value. Buttons say Submit rather than naming the outcome.

## Current journey, step by step

### Prerequisite: register a repository if absent

1. In Browse mode, press `a`, or find Register project in Actions/Menu.
2. Browse/type Repository. The observed form also displays Name and Workspace root; repository-derived defaults are documented by the existing CUA guide but were not exercised in this pass.
3. Review the derived name and storage path, submit, then select that project. Registration itself was not submitted in this exploration.
4. Startup may separately offer Claude/Codex reporting-hook installation. Both were skipped. These are setup choices, not necessary steps on every creation.

### New workspace plus first agent (combined path already exists)

1. If inside a terminal, press `Ctrl-g` to return to Browse, or use visible dashboard controls.
2. Select the target project or one of its workspaces/sessions; the form inherits this context.
3. Press `w`. Discoverable mouse route: Actions → Create workspace. Which-key route: Space → Project → Create workspace (`Space p n` in current source). Space → Workspace → Create terminal adds a session, not a workspace.
4. Complete Branch: default is `feature/`, requiring a suffix. Project is already selected; Branch mode defaults to `new`; Base resolves to `main` for the fixture.
5. Choose Start: Agent. In this pass Terminal was initially selected. Focusing the picker displays a filtered match; `Ctrl-u`, typing `Agent`, then Tab changed it and advanced to Agent.
6. Accept/change Agent (Claude was available). The Agent form has Project, Branch, Branch mode, Base, Start and Agent. No separate workspace Name or session Name is required.
7. Submit, or press Enter on Branch. In the observed agent picker, Enter also submitted the whole form; do not add an extra assumed confirmation step.
8. OVRCR creates the workspace and one agent session, selects it and enters Terminal mode.
9. Complete the provider's native setup/trust/sign-in if required. The observed Claude trust screen is a separate handoff; a created session is not yet a ready agent.

### Add an agent to an existing workspace

1. Return to Browse if necessary and select the intended workspace/session.
2. Press `n`, or Actions → Create terminal, or Menu → Workspace → Create terminal.
3. Start uses the remembered launch choice; in this pass it was Agent after the earlier launch.
4. Review the preselected Workspace. Leave optional Name blank for a generated name. Review/change Agent. If Terminal is selected instead, Command is optional and blank starts the shell.
5. Submit. The new session opens directly in Terminal mode; provider setup may follow.

### Observed failure and recovery cost

Clicking Create workspace opened its form immediately. A subsequent Enter submitted the untouched `feature/`. Git rejected it: `fatal: 'feature/' is not a valid branch name`. The form disappeared, a failed workspace row appeared and a long internal error occupied the footer. Recovery required reopening the form and entering a valid branch. This is evidence for local validation and preserved form state, not a request to change backend lifecycle behavior.

## Design alternatives

All are terminal designs with uniform monospace text, square controls, one selection treatment, keyboard equivalents, explicit target review and no additional confirmation for routine creation. Interactive previews are illustrative local simulations, not application implementation.

| Alternative | New workspace + agent | Existing workspace + agent | Benefit | Tradeoff |
|---|---|---|---|---|
| **Quick start — recommended** | `n` → review New workspace, agent and generated branch → Enter | `n` → choose Current workspace → Enter | Zero required typed fields; combined creation; options stay secondary | Generated branch is less meaningful; show it before launch and permit immediate editing |
| **Inline workspace** | Project-local `w` or visible New workspace → optional branch → Enter | Select workspace → `n` starts the contextual session launcher | Keeps terminal visible; insertion point explains ownership | Sidebar is narrow; new UI inside the navigation tree complicates focus and resizing |
| **Explicit form** | `n` → one form showing project, agent, workspace, branch and base → Enter | Change Workspace to an existing entry → Enter | All consequences visible; closest to current form model | More visible controls and tab stops even when defaults are correct |

Counts are proposed action sequences, not usability timing measurements. If the intended project is already selected and defaults are accepted, Quick start needs two keyboard actions (`n`, Enter), or two mouse activations (New session, Start). Provider trust/setup is additional when required. Optional edits add actions. The current shortcut `w` is already fast; the improvement is discoverability, valid defaults and fewer required decisions, not an invented multi-screen baseline.

### Rules shared by the alternatives

- Project inherits selection. A visible target summary allows changing it; never silently use a different project.
- New workspace is the default in the unified `n` proposal to keep work isolated. Existing workspace remains an explicit single selection; `w` opens New workspace directly. This changes current `n` semantics and must be agreed before implementation. A compatible alternative is to retain `n` for current-workspace sessions and give the unified launcher a different binding.
- Generated branch must be valid, unique and shown before submission. The preview's `work/amber-fox` is a placeholder; implementation must allocate collision-free names locally, without a model call. Do not require both task title and branch name.
- Use last valid agent per project. Never substitute another provider or a shell silently when it is unavailable. First use asks for an installed agent once; zero-agent state offers setup or an explicit Terminal choice.
- Keep base/default branch visible as a compact summary; expose editing on demand. Existing-branch selection is a secondary workspace option. Keep current Name generation; offer Rename after creation. Terminal command remains an optional terminal-only advanced field.
- Remember provider choice without storing ephemeral shell commands. Preserve native provider trust/approval boundaries.
- Start is the sole commit action. No automatic submission on changing a picker. No duplicate launch on repeated Enter. After successful creation, move directly to the new session's input.
- Invalid branch/base stays in the form with field-level error and preserved inputs. No mutation is sent until local checks pass; server validation remains authoritative.
- If the workspace succeeds but agent launch fails, retain it and offer Retry agent / Choose another agent / Open terminal there. Do not create another workspace on retry.

## TUI layout and interaction contract

Product: full-screen session with persistent tree + terminal. Creation is a bounded overlay within that session; inline variant is an alternative shape, not a new app or standalone CLI. Preserve the application's existing output and server ownership contracts.

Common actions: start a new isolated agent; add a session to current workspace; change agent; edit branch; change base; switch project; cancel; retry launch.

Wide / standard sketch (left is context, right is the one modal):

```text
OVRCR · consigint                              [New session]
CONSIGINT         | + New agent session ------------------+
  main · root     | | Project  consigint          [Change] |
> auth-handoff    | | Where    New workspace from main    |
  claude          | | Agent    Claude                     |
                  | | Branch   work/amber-fox             |
                  | | > Workspace options                |
                  | | [Start agent] [Cancel]              |
                  | +------------------------------------+
```

60-column fallback sketch (single active creation view):

```text
OVRCR · consigint / New agent session
+--------------------------------------------------------+
| Project  consigint                            [Change] |
| Where    New workspace from main                       |
| Agent    Claude                                        |
| Branch   work/amber-fox                                 |
| > Workspace options                                    |
|                                                        |
| [Start agent] [Cancel]                                 |
+--------------------------------------------------------+
Tab next · Enter start · Esc cancel
```

- At 120+ columns: tree stays in place; one compact overlay, no second settings panel.
- At 80×24: creation takes priority; one column of controls; remove terminal preview and quota detail before compressing fields. Keep target, validation and action visible. The form must fit 24 rows with options closed.
- At 60 columns: hide navigation and terminal temporarily, retain project/workspace breadcrumb and use the full-width creation view. Inline creation promotes into this same single view. Restore selection and focus on cancel.
- Proposed floor: 60×18 for compact creation; below it show “Creation needs 60 columns × 18 rows; resize or Esc to return.” Expanded options use a scrolling form body with a fixed action/footer; do not claim the current app has this behavior.
- Tab/Shift-Tab follow visual order and remain inside the active form. Arrows select picker rows; typing filters without invoking Browse bindings. Enter accepts an open picker; only Enter outside that picker or the Start button submits. Esc first closes a picker, then cancels the form. Ctrl-C cancels this bounded flow. Preserve the current `Ctrl-g` terminal-to-Browse boundary; do not steal printable `n`/`w` from agent input.
- Focus uses a visible cursor or reverse-video row; color is reinforcement. No blinking or required animation, no Nerd Font dependency. All clickable actions have keyboard paths and visible labels.

## State map

```text
Browse → launcher → validating → creating workspace → starting agent
  ↑        │              │                │                  │
  └ cancel ┘              └ field error     │                  ├ native setup → ready
                                            ├ creation error   └ launch error → retry in same workspace
```

| State | Presentation / next action |
|---|---|
| Initial | Show inherited project, explicit destination and last valid agent; Start has a concrete verb |
| Loading project/default branch | “Loading project defaults…”; keep fields and cancel responsive; disable Start until resolved |
| No projects | “Add a repository” with one repository picker; derive name/storage defaults; return to this launcher |
| No agents | Explain none is available; Setup agent or explicitly choose Terminal; no hidden fallback |
| Partial | One option failed to load: preserve known target and show retry beside the unavailable choice |
| Creating / starting | Show which operation is pending; reject duplicate submission; cancellation after a committed mutation must explain retained resources |
| Workspace created, agent failed | Keep workspace visible; retry session only, or select another provider/Terminal |
| Native setup | Focus the real provider UI; do not label Ready until it is actually usable |
| Disconnected | Keep typed values; say connection lost; reconnect before submission; resolve pending request outcome before retry |
| Success | Close creation UI and focus new session; workspace-only creation selects the empty workspace |
| Too small | Truthful size message and escape route; never clip the Start/Cancel controls |

## Clutter audit

- Current workspace form: six field labels, including Start + Agent as two separate launch choices; proposed Quick start normally shows two selectors and one consequence summary. Project becomes context and Branch mode/Base become secondary options. Do not remove their capabilities.
- Current depth: one modal border over the dashboard plus its persistent sidebar separator. Keep one modal border; add no bordered fields inside bordered groups. Inline variant uses no extra box in the tree.
- Current palette descriptions repeat keyboard letters and run into clipping. Use a single action label plus shortcut and a short outcome description.
- Remove the generic Command guidance from Agent and Register project forms; it describes an irrelevant field there.
- Remove repeated per-row decoration from the mockup; only the selected row has `>`/reverse treatment. Success is a text outcome, not a badge + icon + colored border + color-filled row.
- In capture 05 the modal is roughly 40% of the window's text width and 18 rows tall, with unused rows below six fields. Its labels and field values consume about twelve rows. Quick start should fit about 12–14 rows with options closed. These are layout observations/targets, not measured usability gains.

## Verification and remaining gaps

- Build succeeded; native CUA proved combined creation, session selection, native trust handoff, invalid-default failure and actual shell input.
- Closed with native close button; launcher exited 0. `super+w` did not close this helper, so do not use that shortcut as cleanup proof.
- All fourteen recorded session process groups are gone; disposable root and socket are absent. See [cleanup](cleanup.txt) and [ownership evidence](processes-before-cleanup.txt).
- Mockups simulate creation locally, including branch/base validation, destination changes, cancellation and result. Branch checks are illustrative, not a replacement for Git/server validation.
- Browser preview of the local mockup was blocked by URL policy (`file:` unsupported). No alternate browser route was used. Native app screenshots were visually inspected; browser mockup pixels and interaction were not verified in CUA. Syntax/static self-check results are recorded separately.
- Mockup checks passed: JavaScript syntax; three named variants and forms; unique element IDs; no network calls; illustrative branch checks accepted three valid examples and rejected ten invalid examples. `git diff --check` passed for tracked changes (the new research files remain untracked). These checks do not establish rendered layout or browser interaction correctness.
- No product patch, PR, merge, Linux run, agent inference or production deployment. Narrow terminal states are design specifications, not proven native acceptance results. Before implementation: agree on `n` semantics, generated branch naming and default destination; run real keyboard/mouse and 80×24/60-column acceptance against the chosen design.

## Mockup source

`/Users/xlyk/.codex/visualizations/2026/11/05/01a1ac84-fa65-7c01-85a4-7cd80aadf68b/session-launch-designs.html`

Three carousel variants: Quick start, Inline workspace, Explicit form. The browser simulation uses native HTML controls; terminal key handling and terminal-cell geometry are specified above and require application acceptance if implemented.
