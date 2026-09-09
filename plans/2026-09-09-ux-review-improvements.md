# UX Review Improvements Implementation Plan

> **Execution:** Use the executing-plans workflow sequentially. Finish the first visible checkpoint before expanding the work. This document authorizes planning only; implementation begins after the user requests it.

**Goal:** Address the issues in the twelve-screenshot UX review while preserving OVRCR’s dense terminal interface.

**Architecture:** Changes belong in the existing TUI hint table, palette renderer and dashboard renderer. Preserve the current input handlers, session authority, shared geometry and synchronous runtime. No new dependency, crate, framework or configuration surface is needed.

**Tech stack:** Rust, Ratatui, existing Unicode-width utilities, existing TUI fixtures, native macOS CUA.

**Spec and evidence:** [UX review](/private/tmp/ovrcr-ux-first-pass-20260909/review.md). Screenshots describe `3751a9a417f9ca3f46a19cd73e8ca81c8b2da828`. This plan also inspects the locally recorded `origin/main` at `37e0df2`; no remote fetch or newer-build GUI acceptance was performed while planning.

## Constraints and baseline

- Work in an isolated checkout on a `codex/` branch, starting from refreshed `origin/main`. Record its SHA and inspect the diff from the planning reference before editing. Preserve the current checkout and unrelated `.worktrees/` content.
- One server owner, one active dashboard and the supported 50-session workload remain unchanged.
- Keep one hint table authoritative for command names, availability and descriptions. Footer presentation must not invent independent action semantics.
- Do not change terminal input routing, view acknowledgement, Copy/History identity, clipboard sequencing or process ownership for visual improvements.
- Preserve terminal output colors and Unicode cell widths. Changes to UI metadata must not recolor PTY content.
- Keep errors and pending-operation messages ahead of ordinary hints. Destructive actions retain their confirmations.
- Prefix shell commands and pipeline stages with `rtk`. Preserve distinct evidence files for failed and successful attempts.

## Issue coverage and order

| Review issue | Disposition | Work item |
|---|---|---|
| Oversized help overlay | Existing compact implementation found in locally recorded main; verify instead of rebuilding | 1 |
| Bare Browse shortcuts | Render a short, prioritized set of named actions | 2 |
| Bare History/Copy shortcuts | Reuse the same footer rendering with state-accurate labels | 2 |
| Long search results clip identity | Session-first rows with visible ID and selected-result details | 3 |
| Empty terminal search says actions only | Correct the message and recovery hint | 3 |
| Generic form titles | Use the active task as the border title | 4 |
| Faint sidebar metadata and form hints | Adjust only relevant UI text styles | 4 |
| Narrow sidebar takes substantial space | Bounded verification; implementation is conditional on an actual failure | 5 |

## 1. Establish the updated baseline and verify compact help

**Files to inspect:** `crates/ovrcr-tui/src/dashboard/whichkey.rs`, `tests/tui/palette.rs`, `docs/testing-computer-use.md` and `plans/2026-09-08-which-key-popup.md`.

The local remote-tracking history contains `0e05cd8` and merge `37e0df2`. Its popup uses short rows, a selected description, a bottom-right anchor, a maximum width of 48 cells and a maximum height of 30 rows. Existing tests assert that cells outside the popup remain unchanged. This addresses the screenshot’s design complaint in source, but has not been visually verified in this task.

- [ ] Refresh and record the implementation baseline. Reconcile each issue against that source so completed changes are not duplicated.
- [ ] Run the existing compact-popup cases: `rtk proxy cargo test -p ovrcr --test tui whichkey`. Confirm a nonzero executed count.
- [ ] Launch the isolated native GUI using the repository computer-use guide. Capture the popup at a normal window size and inspect arrow navigation, selected descriptions, disabled reasons and the final reachable action.
- [ ] Mark this issue verified if the popup leaves session context visible and navigation works. If it fails, record the specific failure before proposing any correction; do not replace the existing popup implementation wholesale.

## 2. Make Browse and History actions understandable from the footer

**Implementation:** `crates/ovrcr-tui/src/dashboard/hints.rs`, `crates/ovrcr-tui/src/dashboard/render.rs`.

**Tests:** `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui/palette.rs`, `tests/tui/copy_history.rs`.

**Existing interfaces:** `key_hints(&Dashboard) -> Vec<HintGroup>` determines action semantics; `footer(&Dashboard, u16) -> String` formats hints; `draw_dashboard_at` composes mode, selection and error information.

- [ ] Add a regression through the real dashboard renderer showing that a usable Browse footer contains named actions rather than a sequence of bare letters. Retain existing key availability assertions.
- [ ] Change footer formatting to use complete `key + name` tokens drawn from enabled hints. Prioritize mode and `? Help`, then Enter/Focus, n/Create terminal and colon/Search in Browse. In Copy/History prioritize Escape’s current action, selection, copy and movement. Fit whole tokens by Unicode display width; omit lower-priority tokens rather than cutting labels.
- [ ] Keep the existing Terminal footer. At widths too small for ordinary hints, preserve a clipped mode label; at widths that fit mode plus help, preserve both. Keep all omitted actions available through the popup.
- [ ] Derive History escape wording from the same state that controls the existing handler: cancel a pending copy, clear an active selection, or leave History as applicable. Do not implement new Escape behavior. Have the shared hint name reflect that state so help and footer agree.
- [ ] Account for selection/status prefixes before asking `footer` to fill the remaining width. Avoid the existing pattern where the renderer prepends more text after the footer has consumed the width budget. Errors and clipboard notices still take precedence.
- [ ] Assert Browse, empty dashboard, Copy, History without selection, History with selection, and pending copy at representative widths of 20, 40, 80 and 120 cells. Include a nonready pane so a disabled action is not advertised. Verify that rendered text stays within the available cells and existing input behavior is unchanged.

**First visible checkpoint:** Capture three screenshots on the updated implementation: compact help, Browse footer and History selection footer. Present them with a short comparison before continuing to search and forms. This keeps useful feedback early.

## 3. Preserve search identity and clarify no-match feedback

**Implementation:** `crates/ovrcr-tui/src/dashboard/palette.rs`.

**Tests:** `tests/tui/palette.rs`; private formatting cases can live in the existing owning-module test structure if needed.

**Existing interfaces:** `Entry { label, command }`, `Command::Switch(SessionId)`, `palette_entries`, `draw_palette`, and the existing selected-index clamping. The selected command remains the authoritative target.

- [ ] Add two sessions with the same name in different workspaces to the existing palette fixture. Search by session, project, workspace and ID. Verify the selected command still routes to the expected session.
- [ ] Keep complete searchable identity in `Entry.label`; shorten only the displayed `Command::Switch` row. Format the visible row as `session name (#ID)`, reserving width for the ID and adding an ellipsis to an overlong name using existing cell-width helpers. Remove the repeated “Switch terminal” prefix from displayed rows.
- [ ] Resolve the selected session by its ID and use the existing detail area for project/workspace context. Preserve action descriptions for non-session entries. Wrap detail text within the available area and show an explicit ellipsis if it cannot fit; do not promise unlimited path visibility in a tiny fixed popup. Ensure ID remains visible whenever the row is wide enough to hold it.
- [ ] Replace the empty notice with `No matching actions or terminals`. Show `Edit search · Backspace delete · Esc close`; do not advertise Ctrl-u unless the search handler already supports it. Avoid adding a new editing shortcut to solve a wording issue.
- [ ] Test CJK/emoji and long names at 40, 80 and 120 cells; inspect buffer cells at the right border. Assert duplicate-name IDs are distinguishable, filtering still uses full identity, shrinking result sets keeps selection valid, and Enter chooses the intended session.
- [ ] Capture populated and empty search at the same GUI size as the review. The pass criterion is readable distinguishing identity, explicit truncation where unavoidable, and accurate no-match wording.

## 4. Clarify form titles and increase secondary-text contrast

**Implementation:** `crates/ovrcr-tui/src/dashboard/palette.rs`, `crates/ovrcr-tui/src/dashboard/render.rs`.

**Tests:** `tests/tui/palette.rs`, `tests/tui/sidebar.rs`.

- [ ] Move the existing form-title match into the border-title decision: Search remains `Command palette`; forms use their existing task names; confirmation uses `Confirm action`. Remove the duplicate body title and adjust the focus-line accounting accordingly. Do not change form field order, defaults, submission or cancellation.
- [ ] Add one secondary-text color near the existing renderer colors, initially `Color::Rgb(166, 173, 200)`. Use it for form labels, ordinary form instructions and unselected runtime/context metadata. Leave borders and disabled actions on their existing styles.
- [ ] Remove the extra `faded(..., 65)` treatment from live unselected model labels while retaining their existing label colors. Do not globally brighten `MUTED`, which also styles separators and disabled controls. Preserve selected rows and exited-session dimming.
- [ ] Verify through rendered buffers that the three creation forms have task titles, the active field/cursor remains visible at 40×12, and validation/error text keeps its priority. Preserve the existing request/default assertions rather than rewriting them as title-only tests.
- [ ] Inspect screenshots of an unselected sidebar and one form. Compare normal, selected and exited rows. Accept the color only if metadata is readable while session names remain more prominent; adjust this single color if needed. Do not claim accessibility conformance from style assertions.

## 5. Resolve the narrow-layout hypothesis with a bounded check

**Inspect:** the shared layout in `crates/ovrcr-tui/src/dashboard/render.rs`; existing cases in `tests/tui/sidebar.rs` and `tests/tui/split.rs`.

The current sidebar width is already capped by both `DEFAULT_SIDEBAR_WIDTH` and half the available width. The screenshot alone does not establish a geometry defect. A narrower sidebar would trade terminal space against session-name visibility.

- [ ] Inspect 80×24, 60×20 and 40×12 layouts using the same selected session, first with one pane and then with a split. Verify sidebar navigation, focused-pane visibility, essential footer text and narrow split fallback.
- [ ] If these work, record the concern as evaluated with no change required. Do not add collapse controls or responsive settings just because the sidebar occupies more of a small window.
- [ ] If a required action or focused pane becomes inaccessible, retain the failing screenshot and add the smallest assertion through the actual shared geometry. Define the specific width-allocation correction from that failure before editing. Rendering, subscriptions, cursor placement and hit testing must continue to consume the same layout; never send zero PTY dimensions.

## Verification and delivery

Run focused cases during each change. At the completed feature checkpoint, run each affected suite and the repository gates once:

```sh
rtk proxy cargo test -p ovrcr-tui --lib
rtk proxy cargo test -p ovrcr --test tui
rtk proxy cargo check --workspace --all-targets --all-features
rtk proxy cargo fmt --all -- --check
rtk proxy git diff --check
rtk proxy cargo test --workspace --all-targets --all-features
rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Use the required permission for PTY/socket/process checks. Preserve failure output and executed counts with the tested SHA. After relevant failures or edits, rerun the affected gates; avoid repeating unrelated checks.

Native acceptance is bounded to eight final screenshots: help, Browse, History selection, populated search, empty search, terminal form, workspace form and a narrow dashboard. Inspect project registration as well; take a ninth screenshot only if it reveals a distinct issue. Pair captures with accessibility state, verify actual shell output, and record owned-fixture cleanup. Clipboard delivery, real providers and Linux GUI behavior remain separate evidence, not implied passes.

Review the final diff against the issue table, including callers and retained assertions. Commit coherent units only when requested implementation reaches reviewable checkpoints. Pushing, opening a PR or merging is not part of this planning request.

Update the existing UX diary entry and add a dated implementation follow-up to the review. Preserve the original screenshots and conclusions; new evidence must identify the new SHA. An issue closes only when its stated behavior is observed or its bounded investigation explains why no change is warranted.


## Implementation checkpoint — 2026-09-09

- Baseline refreshed to `a66a0e74e1a2d9acc7869c56f44376f07ddfaab1` in `/private/tmp/ovrcr-ux-implementation`, branch `codex/ux-review-improvements`. Incoming contextual which-key submenus are preserved.
- Compact help verified in the native GUI; 14 existing which-key tests passed. First checkpoint screenshots cover help, Browse labels and History selection. A separate `UX_FOOTER_OK` line verified input. Baseline and checkpoint fixtures exited 0 and cleanup checks passed.
- Implemented named, width-budgeted footer hints; session-first search rows with IDs and selected context; accurate empty-search text; task form titles; brighter live metadata and form labels using the existing `SUBTEXT` color.
- The actual History handler leaves History on Escape even with an anchor. Only pending copy changes it to cancellation; labels preserve that behavior.
- Narrow checks at 80×24, 60×20 and 40×12 found the verbose split-hidden notice displaced Help. Shortened that notice; no sidebar geometry change. Focused views remain nonzero and navigation works.
- Regression evidence includes behavioral RED for footer, search/form/contrast and narrow split footer. The first narrow test attempt was a compile error from a private method; the corrected attempt failed behaviorally before the fix. Older literal-footer/color assertions were migrated without dropping lifecycle/request checks.
- Affected TUI integration suite: 159 passed. Final workspace gates, independent review and final native captures pending at this checkpoint.
- Evidence: `/private/tmp/ovrcr-ux-implementation-evidence/`. Original screenshots and review remain unchanged except for dated follow-ups.
