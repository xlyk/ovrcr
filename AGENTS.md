# Developing OVRCR

OVRCR is a synchronous Rust terminal multiplexer for macOS and Linux. Preserve one server owner, one active dashboard, and the supported 50-session workload unless the assigned design explicitly changes those limits.

## Scope and starting points

- Read the assigned plan in `plans/`, the relevant README section, and the actual source before editing. Plans describe intended work; checkboxes and old reports are not proof that it shipped or passed acceptance.
- Follow the user's current scope and decisions. Resolve routine implementation choices yourself; raise concrete contract conflicts before making dependent changes. Keep future features and unrelated cleanup out of the diff.
- Check the checkout path, branch, base commit, and working tree first. Use an isolated worktree and a `codex/` branch for feature work. Preserve unrelated edits, branches, servers, and terminal sessions.

## Crate boundaries

| Location | Responsibility |
| --- | --- |
| `crates/ovrcr-protocol` | Shared wire types, validation, and framing. |
| `crates/ovrcr-terminal` | Terminal parsing, encoding, and screen/history primitives. |
| `crates/ovrcr-runtime` | Server, PTY/session ownership, registry persistence, Git/workspaces, and task execution. |
| `crates/ovrcr-tui` | Dashboard state, input routing, rendering, and task UI. |
| Root `src/` | Application composition, CLI/client, reporting, service integration, and optional GUI helper. |

- Runtime and TUI depend on protocol/terminal primitives, not on each other. Keep root facades thin; put implementations in the owning crate.
- Keep modules focused and exports small. Reuse existing helpers and dependencies before adding abstractions or libraries. Do not introduce Tokio, a database, an agent SDK, or a new crate without a concrete requirement in the approved design.
- Update constructors, exhaustive matches, public re-exports, CLI output, and optional-feature callers together when shared types change.

## Ownership, ordering, and lifecycle

- Give each piece of state one authority. Avoid parallel stores for selection, geometry, readiness, request completion, or process ownership. Compatibility helpers must delegate to the same implementation.
- Respect the existing lock order and spawn/registration boundary. Keep owner verification and state publication atomic. Delayed cleanup from an old connection must leave a replacement owner's state intact.
- Preserve the synchronous dispatcher and sole socket writer. Socket writes must not block the parser dispatcher. Bound queues and retained state; preserve control responses and lifecycle events or disconnect explicitly when they cannot be delivered.
- Apply final PTY output before exit notification. Detach, hide, and pane close change subscriptions; they must not terminate sessions.
- Signal only process groups whose ownership the current runtime or fixture established. A discovered or remembered PID is not ownership proof. Never use broad process-name cleanup.
- Live PTYs and screens belong to the running server. Detach/reattach and explicit relaunch after server loss are different operations; do not infer crash recovery from successful reattachment.
- Preserve atomic persistence and failure boundaries. A failed write must not be reported as committed, and a partial external side effect must not be described as rolled back.

## Dashboard state and rendering

- Revoke input permission immediately when focus, assignment, or visible geometry changes. Input requires a Running session in the current acknowledged visible view, using that pane's terminal modes.
- Match snapshots by request, revision, and session. Require every expected snapshot plus the matching final acknowledgement before enabling input. An empty view expects zero snapshots and must still complete.
- Keep one view request in flight and coalesce desired changes. A true no-op must preserve readiness. Stale responses must not populate reassigned panes, clear current errors, or enable input.
- Use the same geometry for subscriptions, rendering, hit-testing, and cursor placement. Hidden panes retain their assignments and installed screens but cannot receive input. Never send zero PTY dimensions.
- Keep History/Copy tied to the captured session. Changing that identity cancels pending capture requests, selection jobs, and queued clipboard completions. Removing an unrelated pane must preserve a valid focused capture.
- Current-screen Copy cancels on resize through the actual shared client path. Frozen History retains its cells and wrapping while its viewport changes; live output must not rewrite the frozen capture.
- Preserve overlay input priority, bounded event batches, and the input-drain-before-clipboard-emission boundary.
- Treat literal spacing, terminal modes, Unicode width, colors, cursor placement, and accessibility text as behavior. Keep natural wide-glyph widths and clip at pane boundaries. A TestBackend pass alone may miss errors in emitted terminal coordinates.

## Testing and evidence

- Test the real entry point that changed. A passing helper test is insufficient if the event loop, dispatcher, or writer bypasses that helper.
- For behavior changes, add a regression that demonstrates the defect before fixing it where practical. Distinguish compiler failures, printed reproductions, and failing behavioral assertions in the report.
- Map each acceptance requirement to an actual assertion. Include partial acknowledgements, wrong/stale IDs, A→B→A reassignment, coalesced changes, empty/tiny layouts, failure paths, and late cleanup where relevant.
- Preserve existing assertions when migrating fixtures. Send current-revision output when testing output handling; an ignored stale event cannot prove that output was processed safely.
- Prefer deterministic synchronization over sleeps for races. Force the disputed ordering or boundary; starting a thread does not prove it reached a lock.
- Use real PTYs, sockets, Git repositories, and process groups for claims about those boundaries. Query kernel PTY size with `MasterPty::get_size()` or `stty`; parser dimensions are different evidence. Verify actual child output, not command echo.
- Reuse existing test fixtures in their owning test binary. Confirm filters execute tests; zero tests is not a passing gate.
- Run focused checks while iterating, then the affected suites once when ready. Repeat only after relevant changes, failures, or a concrete unresolved concern.
- Save attempts under distinct filenames with the command, exit status, executed counts, and tested revision. Preserve failures; never overwrite them with a passing run.
- Separate headless, native macOS, Linux, clipboard, and provider evidence. A build or emitted OSC52 bytes does not prove native input, paste, or conversation restoration. Report unavailable checks as unverified.

## Commands and isolated acceptance

Prefix every shell command and pipeline stage with `rtk`; use `rtk proxy` for raw test counts and commands needing native flags. Use `rg` for targeted searches. Follow `~/.codex/RTK.md` when available.

For Rust changes, choose the owning package and add a focused test filter as appropriate:

```sh
rtk proxy cargo test -p ovrcr-tui --lib
rtk proxy cargo test -p ovrcr --test tui
rtk proxy cargo check --workspace --all-targets --all-features
rtk proxy cargo fmt --all -- --check
rtk proxy git diff --check
```

At the feature acceptance checkpoint, run the assigned gates and the workspace regression/lint checks; do not repeat them after every edit:

```sh
rtk proxy cargo test --workspace --all-targets --all-features
rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- Keep documentation-only verification proportional; do not run Rust suites for prose edits.
- Give every live fixture its own `OVRCR_CONFIG`, `OVRCR_SOCKET`, and temporary workspace. Use the required execution permission for PTY/socket/process tests; sandbox denial is not an application failure.
- Follow `docs/testing-computer-use.md` for `rtk proxy just gui`. Use the exact checkout under review and fresh screenshots/accessibility state.
- Record owned fixture PIDs/PGIDs and paths, retain the launcher result, and verify cleanup. Permission denial does not prove a process group is gone. Stop a hung owned test before starting another.
- Never bypass a tool's access denial through another control channel or modify the user's live agent configuration to make an acceptance check pass.

## Delegation, review, and delivery

- Honor the requested model, effort, and sequential/parallel workflow in the actual agent configuration. Give workers bounded briefs with current interfaces and explicit acceptance cases.
- Workers implement and self-review; the coordinator assigns independent review. Workers and reviewers must not recursively create duplicate review seats.
- Review the exact committed diff against the requirements and saved evidence. Check the real caller paths and test assertions, not just test names or passing counts.
- Return blocking findings to the same worker with a precise correction brief. Re-review the findings and correction diff; keep unrelated observations for the final feature review.
- Keep checkpoints current with completed work, actual commands/results, remaining cases, and blockers. Use the agent messaging tool when another agent needs a live update.
- Use `git` and `gh` for GitHub operations. Stage only intended files. Commit coherent reviewed units and honor existing authorization to push/open PRs; merging requires authorization too.
- PRs and handoffs state the problem, behavior change, exact verification, and remaining gaps. Keep roadmap checkboxes open until their required acceptance gates pass.

## Local work diary and credentials

- Every substantive task requires a work diary entry. Read `~/Documents/Vaults/Knowledge/Work diary/AGENTS.md` and the vault rules, then create or extend the appropriate entry in that folder. Keep one entry per unit of work and date later findings; preserve past conclusions.
- Use `op` for 1Password access. Retrieve credentials only when needed; keep secret values out of source, command output, logs, reports, and commits.
