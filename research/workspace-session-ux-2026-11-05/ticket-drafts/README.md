# Proposed tracer-bullet breakdown of OVRCR #389

Status: user-approved and published on 2026-11-05 as GitHub issues #391–#398, all ready-for-agent. Bodies, labels and native blocking edges were read back and verified. Parent #389 title/body/state/labels/comments are unchanged. Original drafts are retained alongside the exact published bodies.

## Published tickets

| Draft | Issue | Blocked by |
|---|---|---|
| T1 | [#391: Choose and launch a verified Claude Code model](https://github.com/xlyk/ovrcr/issues/391) | None |
| T2 | [#392: Run a Claude Code Initial prompt safely through reconnects](https://github.com/xlyk/ovrcr/issues/392) | #391 |
| T3 | [#393: Choose Codex models and run Initial prompts from session setup](https://github.com/xlyk/ovrcr/issues/393) | #392 |
| T4 | [#394: Choose Pi models and run Initial prompts from session setup](https://github.com/xlyk/ovrcr/issues/394) | #392 |
| T5 | [#395: Choose Oh My Pi models and run Initial prompts from session setup](https://github.com/xlyk/ovrcr/issues/395) | #392 |
| T6 | [#396: Start agents from a branch-based registered-workspace dialog](https://github.com/xlyk/ovrcr/issues/396) | #392 |
| T7 | [#397: Create a branch in a child dialog and return to the intact session draft](https://github.com/xlyk/ovrcr/issues/397) | #396 |
| T8 | [#398: Verify the complete four-harness branch-to-agent creation flow](https://github.com/xlyk/ovrcr/issues/398) | #393, #394, #395, #397 |

[Publication receipt](published-tickets.json) · [Read-back verification](verification.json)

Each ticket delivers a complete production UI→Server→native/Git path for its narrow behavior, with its own tests/native evidence. Shared capability/launch work is introduced with the first consuming behavior rather than as a horizontal infrastructure ticket. Any necessary behavior-preserving refactor comes first within that slice. Existing inventory, lifecycle, editor and fixture mechanisms are reused; no standalone speculative prefactor is proposed.

T1 records the four-harness contract matrix before shared native-option composition. T1 and T2 split model selection from prompt/reconnect work to keep each implementation bounded. T3–T5 adapt the shared path independently. T6–T7 build the final branch-oriented experience on the working launch controls. T8 checks combinations and delivery; it does not postpone prior slices’ acceptance. Every integration step must remain green; no wide shared-branch refactor is assumed.

Edges below are immediate prerequisites, omitting transitive duplicates. T3/T4/T5/T6 can proceed in parallel after T2; T7 needs T6 but not the other provider adapters. Final feature delivery waits for all four harnesses and both dialogs. GitHub publication uses real issue identifiers and native blocking links between the new issues; #389 was neither edited nor closed.

## T1: Choose and launch a verified Claude Code model

Blocked by: None

From the existing Agent creation flow, choose a verified Claude Code model or explicitly use Harness default, launch in the selected registered workspace, and remember the successful choice for that project. Preserve existing launchers and Terminal behavior while adding the first complete model-capable path.

[Full draft](01-claude-model-launch.md)

## T2: Run a Claude Code Initial prompt safely through reconnects

Blocked by: T1

In the existing Agent form, optionally compose a multiline Initial prompt and run it when starting Claude Code. Keep pending launch state recoverable through Dashboard reconnect, with honest refusal/uncertainty and no automatic replay after a Server restart.

[Full draft](02-claude-initial-prompt.md)

## T3: Choose Codex models and run Initial prompts from session setup

Blocked by: T2

Use the same session-creation controls to choose a Codex model or Harness default and optionally run an Initial prompt, with the shared reconnect and no-replay behavior.

[Full draft](03-codex-model-and-prompt.md)

## T4: Choose Pi models and run Initial prompts from session setup

Blocked by: T2

Choose a Pi model or Harness default and optionally run an Initial prompt through the real creation flow, retaining per-project choices and the shared pending-delivery guarantees.

[Full draft](04-pi-model-and-prompt.md)

## T5: Choose Oh My Pi models and run Initial prompts from session setup

Blocked by: T2

Use Oh My Pi (omp) as a fully supported model-and-prompt harness in session creation, independently of Pi, with the same per-project defaults and reconnect behavior.

[Full draft](05-omp-model-and-prompt.md)

## T6: Start agents from a branch-based registered-workspace dialog

Blocked by: T2

Open New agent session with Branch, Agent harness, Model and optional Prompt as its four main controls, and start an agent in a registered checkout selected by its current branch label. Keep secondary options collapsed and preserve separate shell creation.

[Full draft](06-registered-branch-session-dialog.md)

## T7: Create a branch in a child dialog and return to the intact session draft

Blocked by: T6

Select the single Create new branch entry, create a real branch and registered workspace in a focused child dialog, then return with that exact destination selected and the parent session draft intact. Reuse the same creation flow for standalone w.

[Full draft](07-create-branch-child-dialog.md)

## T8: Verify the complete four-harness branch-to-agent creation flow

Blocked by: T3, T4, T5, T7

Demonstrate the finished branch-to-agent journey across all four required harnesses and close the combined feature acceptance against the parent specification. This integrates already-proven slices rather than deferring their correctness or native tests.

[Full draft](08-integrated-creation-acceptance.md)

