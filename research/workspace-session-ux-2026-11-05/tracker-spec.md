## Problem Statement

Starting an agent in OVRCR requires users to understand a form called Create terminal, distinguish launch types, and manage workspace details even though they normally identify their work by Git branch. Adding a workspace exposes routine Git fields, can submit an incomplete branch prefix, and can lose useful form state when creation fails. Selecting an agent does not currently provide a consistent way to choose its model and supply the first instruction.

Users need to choose where to work, which agent harness and model to use, and optionally what to do, without navigating unrelated fields or re-entering a draft after creating a branch.

## Solution

Provide a branch-based New agent session dialog with four main controls: **Branch**, **Agent harness**, **Model**, and **Prompt (optional)**. Keep remaining session controls in collapsed **Advanced options**.

The Branch picker searches registered workspaces, grouped by project and labeled by their current checkout branch. It contains exactly one **Create new branch…** entry. Choosing that entry opens a separate **Create branch** dialog with Project, Branch name, and collapsed Workspace options containing Base. Only one dialog is visible and receives input at a time.

Creating the branch commits the branch and registered workspace before returning to session setup. The new destination is selected automatically and the harness, model, prompt and optional session name remain intact. Start agent launches in that workspace, running the optional Initial prompt through a supported harness mechanism. Empty prompt opens interactive use.

Full first-release model-selection and Initial-prompt support is required separately for **Claude Code, Codex, Pi, and Oh My Pi (`omp`)**. Existing other launchers remain available with unsupported capabilities clearly explained rather than silently ignored.

## User Stories

1. As an OVRCR user, I want an action named New agent session, so that I can find agent creation without guessing that Create terminal also launches agents.
2. As an OVRCR user, I want to choose branch, harness, model and optional prompt in one form, so that I can start work without navigating routine configuration.
3. As an OVRCR user, I want secondary session controls collapsed by default, so that uncommon options do not compete with the normal path.
4. As an OVRCR user, I want the current eligible feature checkout preselected, so that adding an agent to my current work takes fewer actions.
5. As an OVRCR user, I want the destination's project and current branch clearly visible, so that I know where the agent will operate.
6. As an OVRCR user, I want to search registered workspaces by their displayed branch and project, so that I can find existing work without knowing an internal workspace identity.
7. As an OVRCR user, I want the picker limited to registered workspaces, so that selecting a result does not silently adopt or provision an unrelated checkout.
8. As an OVRCR user, I want retained-session rows without a registered workspace excluded from creation destinations, so that an old session record is not mistaken for a usable checkout.
9. As an OVRCR user, I want unavailable registered checkouts distinguished from unregistered history, so that I understand why a destination cannot be launched.
10. As an OVRCR user, I want root and detached checkouts explicitly labeled, so that the branch-oriented interface remains truthful about exceptional states.
11. As an OVRCR user, I want root-workspace launch restrictions preserved, so that changing this flow does not weaken checkout protection.
12. As an OVRCR user, I want to open session setup even when the current destination is unavailable, so that I can choose another valid workspace or create a branch.
13. As an OVRCR user, I want an explicit destination choice when no eligible current workspace exists, so that OVRCR does not silently launch elsewhere.
14. As an OVRCR user, I want an Add/register project route when no project is registered, so that I can reach branch creation from an empty instance.
15. As an OVRCR user, I want to create a branch for a registered project without an existing selectable workspace, so that missing current selection does not block setup.
16. As an OVRCR user, I want one Create new branch entry, so that the picker is not cluttered with a separate creation action for every project.
17. As an OVRCR user, I want branch creation to open a focused second dialog, so that the workspace task is clear and my session draft remains separate.
18. As an OVRCR user, I want Project preselected from my current destination and still changeable, so that creating work in the usual repository is fast without preventing another choice.
19. As an OVRCR user, I want an initially empty branch-name field with an example, so that I do not accidentally submit an invalid partial name.
20. As an OVRCR user, I want Base to use the repository's default branch in its local state, so that unrelated feature work is not inherited accidentally.
21. As an OVRCR user, I want to change Base inside collapsed Workspace options, so that stacked work is possible without adding a routine decision for everyone.
22. As an OVRCR user, I want unresolved or failed default-branch lookup to be explicit, so that OVRCR does not guess `main` or create from an unintended base.
23. As an OVRCR user, I want invalid names, duplicate branches and unavailable bases reported beside preserved inputs, so that correcting a mistake does not mean starting over.
24. As an OVRCR user, I want Cancel before branch submission to restore my previous destination and draft, so that exploring creation has no side effects.
25. As an OVRCR user, I want the child dialog to remain visible while creation runs, so that I can see the operation's actual status.
26. As an OVRCR user, I want repeated Submit and pending Esc handled without another mutation or a false cancellation claim, so that slow operations do not create duplicates or mislead me.
27. As an OVRCR user, I want successful creation to refresh and select the exact new workspace, so that I can continue without finding it manually.
28. As an OVRCR user, I want my harness, model, prompt and optional name preserved after the child dialog, so that branch creation does not undo session preparation.
29. As an OVRCR user, I want partial creation and unknown outcomes described accurately, so that retrying does not duplicate resources or destroy useful work.
30. As an OVRCR user, I want a created branch/workspace to remain if I cancel session setup afterward, so that completed workspace creation is not silently undone.
31. As an OVRCR user, I want a failed agent launch retried in the same workspace, so that retry does not create another branch or checkout.
32. As an OVRCR user, I want selection to follow the same checkout when another terminal switches its branch, so that workspace and session identity stay consistent.
33. As an OVRCR user, I want current root, existence and ownership checks applied at Start, so that a stale selection cannot bypass launch restrictions.
34. As an OVRCR user, I want an inline notice identifying other running agents sharing the checkout, so that I understand concurrent file access without another confirmation step.
35. As a Claude Code user, I want verified model selection and Initial-prompt delivery, so that the new form fully supports my harness.
36. As a Codex user, I want verified model selection and Initial-prompt delivery, so that the new form fully supports my harness.
37. As a Pi user, I want verified model selection and Initial-prompt delivery, so that the new form fully supports my harness.
38. As an Oh My Pi user, I want independently verified model selection and Initial-prompt delivery, so that OVRCR does not assume Pi compatibility proves omp support.
39. As a user of another installed agent, I want my launcher to remain available with clear capability limits, so that this redesign does not remove working launches or promise ignored options.
40. As a user of a configured launcher or cloud account, I want its recognizable name and pinned settings preserved, so that the new controls do not rewrite my launch contract.
41. As an OVRCR user, I want an explicitly labeled Harness default model policy, so that I can use native defaults without making an unnecessary concrete-model choice.
42. As an OVRCR user, I want an invalid or unavailable explicit model to stop launch with a useful explanation, so that another model is never substituted silently.
43. As an OVRCR user, I want successful harness/model choices remembered per project, so that each repository starts with its usual defaults.
44. As an OVRCR user, I want explicit draft choices preserved across project changes, so that destination defaults cannot silently undo my current intent.
45. As an OVRCR user, I want a blank Prompt to open the agent interactively, so that I do not need to invent an instruction merely to create a session.
46. As an OVRCR user, I want Start to authorize execution of my nonblank Initial prompt, so that I do not need to submit the same instruction a second time.
47. As an OVRCR user, I want native trust, sign-in and approval behavior preserved during launch, so that my prompt is not mistaken for an answer to a setup dialog.
48. As an OVRCR user, I want pending prompt state retained through a Dashboard reconnect while the Server lives, so that reconnecting does not lose or duplicate a launch.
49. As an OVRCR user, I want lost or uncertain delivery reported without automatic resend after a Server restart, so that a task is not executed twice based on a guess.
50. As an OVRCR user, I want Initial-prompt text excluded from ordinary settings, retained-session metadata and OVRCR logs, so that a convenience field does not create unintended durable copies.
51. As a keyboard user, I want Enter to add a newline in Prompt and a visible verified submit chord to start, so that multiline instructions are easy to enter predictably.
52. As a keyboard user, I want picker Enter to accept only its selection and Tab to stay inside the active dialog, so that navigation cannot accidentally submit the form or reach another view.
53. As a mouse user, I want visible controls to operate both dialogs without forwarding clicks to a terminal, so that form interaction cannot affect a running program behind it.
54. As a terminal user, I want modal typing and paste isolated from underlying sessions, so that preparing a new agent cannot send unintended input to existing work.
55. As a narrow-terminal user, I want the active form to take priority, preserve drafts and keep actions reachable, so that setup remains usable at supported sizes.
56. As a user of an undersized terminal, I want truthful resize guidance, so that clipped controls are not mistaken for a usable form.
57. As a shell user, I want New terminal and custom-command behavior to remain separately discoverable, so that optimizing agent creation does not remove shell workflows.
58. As an OVRCR user, I want `n` to open New agent session and `w` to create a branch/workspace directly in Browse, so that both common tasks have clear entry points without stealing keys inside an agent.
59. As an OVRCR user, I want standalone creation to preserve project setup and automatic-local-terminal policy, so that the new dialog does not silently change existing provisioning behavior.
60. As an OVRCR user, I want focus restored to the right field or newly launched session, so that completing setup flows naturally into the next action.

## Implementation Decisions

- Preserve the existing single Server, active Dashboard, lifecycle worker, live-session limit, workspace identity and retained-session ownership. This is a creation-flow change, not a new orchestration or persistence architecture.
- The Branch picker selects stable registered workspace identities and displays observed branch labels. Join membership from the existing inventory/registry response with current hierarchy labels by project and workspace identity. Hierarchy rows alone are insufficient because retained sessions can synthesize rows without registration. A new registration flag is not required merely to implement this picker.
- Follow the selected checkout automatically through branch-label changes, updating its label without another confirmation. Never switch checkout identity to chase the old branch name. Revalidate launch eligibility and existing root/ownership restrictions at Start.
- Separate permission to open New agent session from permission to launch its selected target. Do not carry the existing current-workspace launch guard onto opening the new form. Preserve separate New terminal restrictions.
- Keep parent and child draft state under the existing Dashboard form owner, including explicit-versus-inherited choice state and previous destination/focus. Only one dialog receives input.
- Branch creation is a real committed operation in the child, separate from starting the requested agent. Preserve project setup and optional automatic local shells. Cancel before submission has no effect; cancelling the parent after successful creation retains the workspace.
- Change the new child flow's lifecycle-acceptance transition so accepted work does not discard the form. Gate existing hierarchy-driven auto-attachment for this path as well: seeing a workspace is not sufficient to close the child before correlated successful completion. Preserve unrelated lifecycle behavior.
- Retain the project/workspace identity already allocated in the submitted creation request and correlate the lifecycle completion. Current success carries no created-workspace identity. Wait for matching successful completion and matching registry/hierarchy presence before selecting it; support either event order. A new result variant is not mandatory if retained submitted identity suffices.
- Submitted creation is not cancellable. Show pending state, reject duplicate mutations, keep the child visible and preserve error inputs. Partial success and unknown outcomes require reconciliation before retry; do not automatically delete resources or recreate the workspace.
- Initialize Branch name empty and keep Base inside collapsed Workspace options. Reuse asynchronous default-branch discovery, with explicit loading/failure handling rather than an invalid prefix or silent `main` fallback. Use the local default-branch state without automatic fetch/update.
- Add a genuine multiline Prompt editor using existing editing mechanisms where suitable. Route newline, picker acceptance, form submission, paste, cursor/grapheme edits and modal clicks deliberately; adding a single-line field is insufficient. The mockup uses Alt+Enter for submission; verify its native encoding and provide a visible working chord/action.
- Full model and Initial-prompt capability is required for Claude Code, Codex, Pi and omp separately. First identify each installed harness's supported discovery, model/default application, prompt, native setup and delivery-evidence contract. Existing delegation discovery primitives are candidates for reuse, not Dashboard acceptance or proof for omp.
- Preserve configured override provenance and bind capabilities to the exact resolved launcher configuration, including executable/arguments and detected, configured or cloud origin. A familiar display name does not establish safe override support. Respect native versus wrapper argument boundaries and existing trust flags. Unknown/replaced wrappers receive no guessed injection; configured or cloud-pinned models remain read-only unless compatible override is proven.
- Add a current-Dashboard capability query whose Server response is bound to the verified launch configuration. Changed or unavailable configurations invalidate earlier capabilities before Start. Reuse provider primitives below existing authority-specific entry points; do not impersonate a delegated session to call its protected endpoint. Keep discovery off input-reader and lifecycle-lock paths.
- Extend the new Dashboard launch path with a connection-independent launch-intent identifier using an existing ID mechanism. The Server owns frozen target/options/Initial-prompt state in memory before acceptance and binds it to the exact session/run after allocation. Repeated accepted intents reconcile status rather than repeat effects; the existing lifecycle/mutation owner still performs launch.
- Provide a bounded current-Dashboard reconciliation/read path discoverable after reconnect without the old connection request token. Distinguish known pending, accepted, refused and unknown delivery; expose recoverable draft text only when delivery is known not to have occurred. This does not authorize reading another provider's conversations.
- Keep pending prompt text only for the current launch/run and Server lifetime. Clear accepted/retired text at the actual delivery boundary; redact prompt-bearing debug/error output. Do not write it into ordinary settings, retained metadata, Events or diagnostic logs. Absence after restart/retirement is not proof of non-delivery and never authorizes automatic resend. Provider-owned conversation storage is a separate existing contract.
- Use a provider-supported launch/input mechanism. Dashboard view-input readiness, elapsed time, visible terminal text and response-cycle Ready are not universal native-startup acceptance. Do not broaden existing trust behavior or infer setup completion from screen scraping.
- Remember successful harness/model policy per project through the existing settings owner. Harness default remains a policy, not a pin to the last observed model. Preserve explicit edits and dependent choices across project changes; untouched defaults may update without invalidating an explicit model. Do not remember prompts or one-off commands as launch settings.
- Update wire constructors/callers, protocol version and exact encoding snapshots together when formats change; preserve explicit incompatible-version refusal. Extend the launch-choice parser, settings catalog/editor/write paths and matrix together, retaining old Agent/Terminal settings and interpreting absent model policy as Harness default.
- Use `n` for New agent session, retain New terminal as a separate named action, and use `w` for standalone branch/workspace creation. Keep labels, dispatch, availability and menus consistent through the existing key-binding owner. Inside a running program, preserve ordinary input and the established return-to-Browse boundary.
- Preserve compact terminal presentation: one active modal border, semantic color plus non-color focus/state cues, no required special font. At 80×24 prioritize the form; at 60 columns hide the sidebar and use full width. Expanded options scroll while actions/status stay reachable. Proposed minimum is 60×18, subject to native acceptance; smaller sizes show resize guidance and preserve draft.

## Testing Decisions

- **Main seam:** extend the existing compiled Dashboard-to-Server flow using the established disposable live-server, real Git checkout, socket and PTY fixtures. Assert externally visible state, requests/effects and actual output rather than helper implementation or private field layout. This is the primary cross-module seam already agreed during design; do not introduce a parallel fixture framework.
- **Supporting seams:** use owning state/input tests for deterministic ordering and editing, existing protocol/settings compatibility tests for serialization and persistence, native CUA for rendering/real keyboard/mouse behavior, and task-owned installed-harness fixtures for provider acceptance. These supplement the main seam where it cannot prove the contract.
- **Prior art:** existing compiled TUI acceptance, workspace-identity lifecycle tests, settings-lifecycle/matrix tests, retained-session recovery tests, provider fixtures and disposable native GUI checks. Setup/doctor stand-ins and shell markers remain useful for their own boundaries but cannot substitute for provider-level model/prompt proof.
- **Parent/child lifecycle:** accepted submission leaves the child and parent draft intact; duplicate Submit and pending Esc produce no additional mutation. Force completion-before-hierarchy and hierarchy-before-completion, including early updates before acceptance. Only correlated success plus matching registered identity returns/selects; failure retains inputs and distinguishes partial registration. Verify pre-submit cancellation and parent cancellation after committed child creation separately.
- **Membership and guards:** distinguish a retained-only synthetic row from an unavailable registered checkout; exclude the former from creation. Open the new form without a launchable current target, select another valid target, and prove Start still rejects stale/root-drifted/replaced targets. Verify same-workspace branch changes follow that identity. Include non-main default branches and failed/loading Base discovery.
- **Draft and defaults:** project changes modify inherited choices only, preserve explicit harness/model/prompt/name and block unsupported combinations without silent substitution. Successful launches alone update per-project memory. Prove Harness-default versus explicit-model round trips and backward-readable Agent/Terminal settings through the real loader, writer and editor.
- **Editor and input:** through production Dashboard dispatch, prove Prompt Enter adds a newline; the verified submit chord produces one request; picker Enter only accepts a choice; multiline paste and cursor/grapheme edits preserve text; no modal bytes or clicks reach the underlying PTY. Verify the actual submit encoding natively rather than relying solely on synthetic key events.
- **Capability and reconnect authority:** exercise the real Dashboard request/dispatcher and managed-launch path for exact launcher binding, changed arguments, stale capabilities, current-owner checks and no delegation-authority impersonation. Use a stable accepted launch intent and exact run; force a lost response, reconnect, stale-run request and duplicate intent to prove no duplicate spawn/send.
- **Prompt lifetime/privacy:** use a unique synthetic prompt marker. Prove pending state survives Dashboard reconnect while the Server lives, does not replay after Server restart, and distinguishes acceptance, refusal and uncertainty. Verify the marker is absent from ordinary settings, retained-session metadata, OVRCR Events and diagnostic logs. Do not misapply this assertion to the provider's own conversation history.
- **Compatibility:** exact wire bytes must match the updated protocol version; incompatible clients fail explicitly. Exercise updated settings catalog/parser/editor/write fixtures and the existing settings matrix so new model policies are neither dropped nor rejected as unknown fields.
- **Four-harness gate:** record an independent evidence tuple for each of Claude Code, Codex, Pi and omp: installed executable/version; exact nonsecret explicit-model or Harness-default options; provider-visible resolved model or explicit refusal; provider receipt/executed response establishing the unique Initial prompt reached that provider; native setup/trust interruption and delivery state; fixture/process ownership and cleanup. A launcher banner, shell printf or mock-only assertion is insufficient.
- **Native TUI acceptance:** use CUA against the actual reviewed checkout, preserving screenshots with accessibility evidence for both dialogs, selection, focus, pending/errors and restored draft. Cover mouse and keyboard, 80×24, 60-column and minimum/undersized states. Keep Linux, headless, native GUI and provider evidence distinct; unavailable or unrun checks are gaps, not passes.
- **Regression and delivery:** preserve separate Terminal/custom-command flows, standalone provisioning, existing cloud restrictions, live input during unrelated lifecycle work, root ownership and retained-session behavior. Run the repository's required current-revision regression/build/lint and hosted checks before delivery, without weakening assertions. Clean up only task-owned fixtures and verify process, socket and directory cleanup.

## Out of Scope

- Listing or adopting unregistered local/remote Git branches in the main picker.
- Automatically fetching, pulling or updating the base branch.
- Renaming workspace identity when its checkout branch changes, or silently switching to a different checkout with the old name.
- Cancelling an already submitted lifecycle job, adding a jobs tray, or introducing a second scheduler/server/settings store.
- Persisting Initial prompts for restart recovery, storing prompt templates in remembered launch settings, or automatically replaying uncertain delivery.
- Broadening native trust/approval behavior or changing cloud credential, account, pinned-model or published-source policy.
- Requiring every other installed/configured launcher to gain full model/prompt capability before release; their existing launches remain available with honest capability limits.
- Redesigning terminal panes, the Operator, conversation resume, task orchestration or unrelated workspace/session lifecycle features.
- Treating specification publication, mockup success or source review as application implementation, native provider acceptance, deployment or merge authorization.

## Further Notes

The design was confirmed after five decision rounds covering 18 submitted answers. The required full-support set explicitly includes omp in addition to Claude Code, Codex and Pi.

A three-role OMP/DeepSeek V4.1 Flash source review identified six implementation-plan gaps. The subsequent local plan revision addresses them in this specification: preserved lifecycle form state and identity correlation; registry-based membership; form-opening versus Start eligibility; multiline input dispatch; capability/pending-launch reconciliation contracts; and concrete compatibility/four-harness evidence requirements. The original reviewers were not rerun after that revision, and no runtime pass is claimed.

First establish the four supported native launch contracts before promising the controls. If a required contract cannot be proved, report that acceptance blocker rather than silently dropping a required harness or weakening the test. Native or paid validation needs explicit scope and task-owned configuration; preserve live sessions and user data.

Related prior decisions: [branch names and protected root workspaces](https://github.com/xlyk/ovrcr/issues/141), [stable workspace identity](https://github.com/xlyk/ovrcr/issues/142), and [non-blocking lifecycle work](https://github.com/xlyk/ovrcr/issues/297). Preserve their contracts while changing this creation flow.
