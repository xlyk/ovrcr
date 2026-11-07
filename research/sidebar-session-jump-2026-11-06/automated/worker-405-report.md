# Issue #405 Operator entry implementation report

## Revision and scope

- Branch: `codex/sidebar-session-jump-405`
- Commit: `91994038ead6ab1678d1e7a3105f9e5ac725f1df` (`feat(tui): allow Cmd jumps from Operator`)
- Worktree: `/private/tmp/ovrcr-401-t405`, clean at report time.
- The branch includes Operator Cmd+digit session jump entry, public TUI coverage, compiled terminal acceptance, and the Dashboard help text. It also contains the narrow equivalent `matches!` correction needed for the retained-target eligibility code.
- No integration merge was attempted; root is coordinating the pending direct user approval. The GUI/CUA and integrated broad checks remain root-owned.

## Behavior and acceptance coverage

- Visible Operator can begin Cmd+digit jump when the sidebar is drawn. Existing visible forms keep priority. A stale hidden Operator form does not block normal Browse behavior.
- Operator composer draft and cursor survive preselection cancel, selecting an Agent, and a subsequent cancel/reopen. The selected conversation and transcript scroll anchor are retained. Tests use synthetic local conversation/transcript data; no model turn is made.
- Cmd jump does not reveal an on-demand hidden sidebar. Explicitly opening it enables the visible Agent labels.
- Labels decorate only Agent rows, preserve title/status columns, row geometry, Unicode titles, and ordinary mouse hit testing. Coverage exercises 120x40, 100x24, 80x24, 60x24, and 60x16.
- At 59x16, Operator keeps the existing resize guidance and suppresses labels and Cmd selection, as required by the design contract below 60x16; ordinary click selection remains available.
- Compiled acceptance drives the actual Dashboard and live local server through the outer PTY with Kitty command press/digit/repeat/release sequences. It selects a synthetic local Agent, verifies the `OPERATOR_TO_AGENT_OK` marker route, returns to Operator, and confirms the unsent draft was not forwarded.
- Native Codex GUI/CUA coverage (including visible gesture and hidden-sidebar refusal) is still outstanding for root; the outer-PTY test does not establish production terminal-host key reporting.

## Focused checks

All Rust checks used the prepared copy-on-write target directory `/private/tmp/ovrcr-401-build-405` and `CI=1` for the existing 15-second terminal fixture budget.

| Command | Result | Preserved log |
| --- | --- | --- |
| `CI=1 CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-405 rtk proxy cargo test -p ovrcr --test tui operator_sidebar_jump_labels_visible_agents_and_cancel_keeps_the_composer_cursor -- --nocapture` | 1 passed, 393 filtered | `/private/tmp/ovrcr-401-implementation/worker-405-tui-transcript-green-04.log` |
| `CI=1 CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-405 rtk proxy cargo test -p ovrcr --test tui operator_jump_keeps_agent_columns_and_rows_at_sidebar_boundaries -- --nocapture` | 1 passed, 393 filtered | `/private/tmp/ovrcr-401-implementation/worker-405-tui-geometry-green-04.log` |
| `CI=1 CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-405 rtk proxy cargo test -p ovrcr --test tui operator_cmd_jump_does_not_reveal_its_on_demand_sidebar -- --nocapture` | 1 passed, 393 filtered | `/private/tmp/ovrcr-401-implementation/worker-405-tui-hidden-green-02.log` |
| `CI=1 CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-405 rtk proxy cargo test -p ovrcr --test terminal_acceptance operator_command_jump_uses_the_live_agent_and_preserves_its_unsent_draft -- --nocapture` (scoped socket access escalation) | 1 passed, 26 filtered, 6.77s | `/private/tmp/ovrcr-401-implementation/worker-405-outerpty-green-04.log` |
| `rtk cargo fmt --all -- --check` | passed | no log required |
| `rtk git diff --check` | passed before commit | no log required |

TDD red evidence: `/private/tmp/ovrcr-401-implementation/worker-405-tui-red-02.log` records 0 passed / 1 expected failure against the baseline, where the visible Operator Agent row lacked its `[1]` jump label.

Intermediate attempts are preserved rather than overwritten. The first sandboxed live fixture could not bind its private Unix socket; an early escalated attempt missed the one-time status-off notice. Subsequent fixture/test corrections passed without weakening assertions. Relevant logs include `worker-405-outerpty-sandbox-root-inventory.log` and other `worker-405-*.log` files in `/private/tmp/ovrcr-401-implementation/`.

## Cleanup and remaining work

- Final successful fixture root `.tmpymAOS0` was removed; server PID 37216 and session process groups 37480, 37611, 37670, 37757, and 37855 were confirmed stopped.
- Earlier successful root `.tmpI3FpnV` was removed; groups 55224, 55349, 55407, 55498, and 55594 were confirmed stopped.
- Failed sandbox root `.tmpKffSKC` was inventoried, its server PID 35166 confirmed stopped, then the task-owned root removed. Intermediate root `.tmpyMbQgS` and server PID 45016 were also cleaned up.
- Final worktree status: clean. No build was run in a second target directory.
- Root still owns native GUI/CUA, integrated regression/lint checks, review, diary, and any authorized integration/PR delivery steps.

## Approved integration follow-up

- Merged current integration `cd983972862139f8f9a49ff067ce5e4289b09ed4` into the feature branch and committed the merge as `c4fbcb054742892faf454d9ef110c2701647c57e` (parents: `91994038ead6ab1678d1e7a3105f9e5ac725f1df` and `cd983972862139f8f9a49ff067ce5e4289b09ed4`). The branch worktree is clean.
- Resolved the Dashboard key-table conflict by combining shared running/retained Agent code semantics with the Operator open-sidebar and 60×16 constraints. Kept both `session_jump_adaptive` and `session_jump_operator` compiled acceptance modules registered. The state logic auto-merged with retained-target eligibility, adaptive cancellation, Operator modal/floor rules, and the narrow `matches!` lint correction intact.
- Post-merge `CI=1 CARGO_HOME=/Users/xlyk/.cargo RUSTUP_HOME=/Users/xlyk/.rustup CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-405 rtk proxy cargo test --offline -p ovrcr --test tui operator -- --nocapture`: 20 passed, 382 filtered; log `/private/tmp/ovrcr-401-implementation/worker-405-merged-operator-01.log`.
- Post-merge `CI=1 CARGO_HOME=/Users/xlyk/.cargo RUSTUP_HOME=/Users/xlyk/.rustup CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-405 rtk proxy cargo test --offline -p ovrcr --test terminal_acceptance operator_command_jump_uses_the_live_agent_and_preserves_its_unsent_draft -- --nocapture` (approved scoped private-socket/PTY access): 1 passed, 27 filtered, 6.60s; log `/private/tmp/ovrcr-401-implementation/worker-405-merged-outerpty-01.log`.
- Post-merge `rtk cargo fmt --all -- --check` and staged source `rtk git diff --cached --check -- crates docs/dashboard.md docs/development/tui.md tests src` passed.
- The final fixture root was absent, server PID 77084 was gone, and groups 77343, 77470, 77528, 77615, 77711 were all stopped. No additional target directory or paid model turn was used.
- Integration includes native GUI evidence from the integration branch. Root owns final review and broader integrated checks; no main merge or push was made.

Original raw capture files named in this report are preserved byte for byte in the [artifact index](../raw-artifacts.md).
