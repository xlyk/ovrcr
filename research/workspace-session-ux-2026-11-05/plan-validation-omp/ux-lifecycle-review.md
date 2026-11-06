Verdict: **NEEDS REVISION** (plan quality, not runtime acceptance). The supplied plan/design copies are unnumbered; plan and design citations below are section-level, while source citations are line-based.

## 1. Blocker — Lifecycle acceptance destroys the child dialog before pending/error handling

**Scenario:** User opens Create branch, presses Create. The Server accepts with `Response::Ok`. The reused lifecycle path immediately closes the palette, converts it to a provisional row, and loses the child fields. If creation later fails, there is no child form or `failed_launch` to reattach, so “keep errors and inputs” and “retry session launch in place” cannot work.

**Evidence:** `crates/ovrcr-tui/src/dashboard/palette.rs:2837-2867` stores `failed_launch` and calls `begin_lifecycle_pending`; `crates/ovrcr-tui/src/dashboard/palette.rs:3036-3044` accepts the job on `Response::Ok`; `crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs:245-262` sets the provisional state and `self.palette = None` at line 261. Pending/provisional state is also Dashboard-local at `crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs:330-334`. This conflicts with plan §3 (“Show pending state in the child”; “Keep errors and inputs”; “Retry session launch in place”) and design §“Create branch dialog” (“Keep the child dialog visible while creation is pending”).

**Smallest correction:** Add an explicit state-model step: the child `Page` and parent draft remain open while the accepted lifecycle job runs; `Response::Ok` must not discard them. On `LifecycleCompleted::Failed`, restore fields and error; on success, return to the parent draft and select the returned workspace ID. Add pending/failure/retry tests through this path.

## 2. Blocker — Prompt entry and Enter-vs-submit behavior are not planned

**Scenario:** User focuses Prompt, types multiple lines, and presses Enter. The current form Enter path moves focus or submits; the observed field kinds are text/toggle/pick/path, with no multiline prompt editor or submit-chord dispatch. The plan requires a prompt control and tests its behavior, but names no implementation step for the editor, Enter-newline routing, picker selection, or the portable submit chord.

**Evidence:** `crates/ovrcr-tui/src/dashboard/palette.rs:2435-2438` shows Enter ending in `submit_when_ready`; the plan §3 dialog bullets do not add a prompt field or key path. Design §“Input and layout” requires Enter in Prompt to insert a newline, pickers to accept without submitting, and a visible submit chord.

**Smallest correction:** Add a dedicated multiline Prompt field/editor state and key-dispatch rules: Enter inserts newline; Alt+Enter (or verified equivalent) submits; picker Enter accepts only the picker; Start/Create clicks submit. Cover multiline paste and no input leakage.

## 3. High — Create-branch field defaults and project-change behavior are missing from the implementation steps

**Scenario:** The child dialog opens with Branch pre-filled from `branch_prefix` and Base hardcoded to `main`, even when the repository default is not `main` or Git lookup fails. The design requires Branch initially empty with a placeholder, Base from detected local default state, and Workspace options initially collapsed.

**Evidence:** `crates/ovrcr-tui/src/dashboard/palette.rs:2927-2945` initializes the existing CreateWorkspace form with `self.settings.branch_prefix` and literal `"main"`. Plan §3 has no bullet for changing these initializers, removing the invalid prefill, or implementing collapsed Workspace options.

**Smallest correction:** Add an explicit step for the child form initialization: empty Branch with placeholder, project preselect/change, detected Base with a loading/unknown state, no `main` fallback, collapsed Workspace options, and duplicate/unavailable branch handling. Add tests for non-`main` defaults and Git lookup failure.

## 4. High — `n` availability still depends on an existing workspace

**Scenario:** A registered project has no workspaces. Browse `n` is disabled by `no_workspace`, so the new agent dialog cannot open to offer its Branch picker and Create new branch entry. The design requires a deliberate destination choice when the target is absent, not silently disabling the flow.

**Evidence:** `crates/ovrcr-tui/src/dashboard/keymap.rs:1004` gates `n` with `.unless(launch_blocked.or(no_workspace))`. Plan §3 changes `n` to New agent session but does not change its availability rule. Design §“Session dialog” includes Create new branch and says an empty project can still create a branch.

**Smallest correction:** State new availability rules explicitly: with at least one registered project, `n` opens even when no workspace is selected/exists; the picker requires an explicit choice or Create new branch; the root guard applies to the selected workspace. Update keymap and palette tests.

## 5. High — Project changes overwrite explicit harness/model/prompt choices

**Scenario:** User selects harness/model and writes a prompt, then changes Project. The existing `refresh_workspace_form` sees a different `launch_project` and replaces every launch field wholesale, ignoring per-field edits. Adding model and prompt fields without changing this path will silently discard the draft, contrary to the design’s explicit-choice preservation rule.

**Evidence:** `crates/ovrcr-tui/src/dashboard/palette.rs:845-856` replaces fields from `launch_fields` when the project changes; `crates/ovrcr-tui/src/dashboard/palette.rs:2675` starts the launch-field construction from remembered defaults. Plan §3 says “store explicit edits separately from inherited defaults,” but does not identify this overwrite or the merge rule.

**Smallest correction:** Add a concrete merge policy: inherited defaults may update only fields without explicit edits; harness, model, prompt, and name survive project switches; dependent defaults cannot invalidate an explicit model silently. Test project change in both parent and child dialogs.

## Proof gaps, not findings

- No installed Claude/Codex/Pi/omp model/prompt launch evidence is present; plan §1 correctly treats this as a prerequisite, not proof.
- Lifecycle completion replay/reconciliation after Dashboard reconnect was not established from the supplied excerpts; it needs explicit acceptance evidence or a named reconciliation path.
- Branch-label drift refresh for the new dialogs was not traced through `sync_palette_workspace_identity`; this remains an implementation check.
- No CUA run, terminal Alt+Enter encoding proof, narrow-layout proof, or provider run exists in the supplied material.
