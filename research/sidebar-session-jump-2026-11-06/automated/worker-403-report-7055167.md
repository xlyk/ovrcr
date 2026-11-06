# OVRCR #403 implementation report

## Revision and scope

- Branch: `codex/sidebar-session-jump-403`
- Commit: `7055167accc989a1fa5de6e84c0cf78998d1385d` (`feat: add adaptive sidebar session jump labels`)
- Base tested for the initial red: `2ae54eee`.
- The isolated worktree is clean at the commit above. No merge/rebase/cherry-pick was performed; local feature-branch history changes are waiting for direct user approval as requested by the automatic reviewer.
- All Rust builds used only `CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-403`.

## Implementation

The frozen mapping uses one digit for 1–9 visible eligible Agents and zero-padded two-digit labels for 10–50. The displayed badge width is fixed for a hold. In two-digit mode, the first digit is shown in the footer and emphasized on matching badges; Command+Backspace clears the partial code. Invalid codes remain retryable, and selection happens on the second digit. The existing release, exact-run validation, acknowledged-view and input-drain protections remain in place.

The change adds separate public TUI and compiled terminal acceptance modules. The compiled acceptance runs the real Dashboard against the live Server test fixture and gates the tenth Agent view while checking queued typing and paste do not reach a child before the view boundary completes. Docs cover adaptive labels and the distinction between retained inspection and explicit Resume/Reopen actions.

## Executed checks

| Revision/tree | Command | Result |
| --- | --- | --- |
| Base `2ae54eee` | `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-403 cargo test -p ovrcr --test tui session_jump_adaptive::ten_visible_agents_render_adaptive_two_digit_labels` | Red: 1 test failed as expected; baseline still reported the nine-Agent limit and did not render padded labels. |
| Feature tree matching `7055167` | `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-403 cargo test -p ovrcr-tui --lib production_view_geometry_change_cancels_the_current_jump_hold` | Green: 1 passed. |
| Feature tree matching `7055167` | `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-403 cargo test -p ovrcr --test tui agent_search::` | Green: 26 passed. |
| Feature tree matching `7055167` | `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-403 cargo test -p ovrcr --test tui session_jump_adaptive::` | Green: 8 passed. |
| Feature tree matching `7055167` | `rtk proxy env CI=1 CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-403 cargo test -p ovrcr --test terminal_acceptance session_jump_adaptive::two_digit_jump_selects_the_tenth_agent_through_the_compiled_dashboard -- --exact` | Green: 1 passed in 10.41s. `CI=1` uses the fixture's existing 15-second deadline. |
| Feature tree matching `7055167` | `rtk proxy cargo fmt --all`, `rtk proxy cargo fmt --all -- --check`, `rtk git diff --check` | Passed. |

The successful live test is an injected Kitty enhanced-key event acceptance through the compiled Dashboard and Server. It does not replace the native-host Command-key CUA owned by root.

## Failed development attempts and resources

- An unprivileged live fixture could not create its private Unix socket (`.tmpSMz1WF`, PID 81426); that process was later confirmed absent and the task-owned temporary root removed.
- A live attempt without `CI=1` timed out before alternate-screen startup (`.tmpX4POCS`, PID 83260). The missing Bridge-assets notice was nonfatal. No assets were installed or packaged; the process was confirmed exited.
- The first compiled-acceptance screen predicate checked the wrong intermediate view, and the second expected “Loading agent” where the rendered state was “Session selected.” Both task processes exited (`.tmp1J6ssC`, PID 58503; `.tmpwkhIfO`, PID 69864). The final corrected predicate passed as recorded above.
- Early TUI assertions used an incorrect pane/label locator and an incorrect Unicode marker/clip length; they were corrected without weakening assertions. The search regression was also corrected to release the selected-view latch before opening search.
- The final successful acceptance cleaned its Dashboard, proxy and Server fixtures. No task-owned live process or fixture is intentionally retained.

## Integration drift at `8575c5f6296df8be619f3e46e8b4845655855ff6`

Read-only comparison against `codex/sidebar-session-jump-integration` found overlap with the retained-Agent implementation. Preserve the incoming `SidebarJumpTarget` phase capture, exact run/phase revalidation, retained browse-only selection branch and recovery suppression. Reintroduce #403's captured code width/prefix, two-digit dispatch/backspace/footer helpers and adaptive badge rendering alongside that path. Keep both adaptive and retained test modules in the top-level module declarations, and combine the docs so they describe both adaptive numbering and retained browse-only navigation. No conflict resolution or history mutation has been performed in this worktree.

Native Command-key input, native-visible selection, screenshot/accessibility capture and native cancel/incomplete-release cases remain for root's CUA pass.
