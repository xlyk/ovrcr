# Workspace launch and automatic title acceptance

## Scope and source

Base: `919f2ec4708fe7676ae46a74149ff0ef334260da`; feature branch: `codex/automatic-session-titles`. Captures came from the feature worktree's `target/OVRCR GUI.app`, launched with `just gui` and controlled through Codex CUA on macOS. The reviewed feature commit contains this report. The final build includes the empty-workspace selection and configured-shell fixes; the later Clippy let-chain change is behavior-preserving.

Both demos used task-owned Git repositories, sockets, configuration and shell fixtures. No real provider or user server was launched, installed, or transitioned. A shell script emitted OSC 2 titles and distinct `CUA_READY`, `CUA_TITLE_APPLIED`, and `CUA_EXITED` output markers.

## Native evidence

Each screenshot has adjacent `.txt` accessibility evidence (trailing whitespace trimmed for Git; raw captures remain in the local evidence directory).

| Case | Observed outcome | Evidence |
| --- | --- | --- |
| Workspace + first agent | One selected fixture agent; automatic title updates with semicolon and CJK text | [Automatic title](01-auto-title.png) |
| Manual title | Pinned title survives later application title; clearing Rename restores latest app title | [Pinned](02-pinned.png) |
| Duplicate title | Identical titles remain distinct through session ID suffixes | [Duplicates](03-duplicates.png) |
| Spawn failure | Workspace remains; recovery offers Retry, Choose another agent, Open shell | [Failure](06-launch-failure.png) |
| Recovery | Open shell launches in the retained workspace; distinct output marker confirms input | [Recovered](07-recovered.png) |
| Terminal custom command | Optional command launches in Terminal mode and emits its title | [Custom command](08-custom-terminal.png) |
| Remembered choice | After dashboard restart, workspace form still selects Terminal | [Restart](09-remembered-terminal.png) |
| Empty workspace, final build | New workspace selected with no previous terminal displayed; `n` targets it | [Empty workspace](10-empty-final.png) |
| Configured shell, final build | Blank Terminal command uses configured `shell` argv; automatic title follows emitted title | [Configured shell](11-shell-override-final.png) |
| Relaunch, final build | New process and unique fallback selected; old exited session retained | [Relaunch](12-relaunch-final.png) |
| Missing preset, final build | Submit reports unavailable remembered agent and requires explicit replacement | [Missing agent](13-missing-agent-final.png) |

The initial empty-workspace check exposed stale selection. A regression test reproduced both response/event orders, the fix passed, and the final native capture above verifies the outcome. No native screenshot is claimed as provider telemetry evidence.

## Automated validation

- Runtime title integration: 3 real Git/socket/PTY tests passed (workspace launch, app title/pin/reset, relaunch and retained output).
- CLI title integration: 1 passed; CLI parsing: 2 passed.
- Protocol: 35 passed, including wire-version fixtures; title callback: 1 passed.
- Node extension regression: 41 passed, none skipped.
- Clippy, all targets/features with warnings denied: passed.
- Doctests: passed; no doctests defined (5 suites).
- Full workspace suite (`CARGO_INCREMENTAL=0 cargo test --workspace --all-targets --all-features`): 858 passed, 16 ignored, 27 suites; exit 0.
- Formatting and `git diff --check`: passed.

Focused RED/GREEN evidence includes optional naming, OSC title callback, CLI parsing, and empty-workspace selection. Existing real GUI and terminal acceptance tests were migrated to the approved form and workspace-based names, preserving attachment, output, geometry, and cleanup assertions.

## Failures and limits

Initial socket tests were blocked by sandbox EPERM, then rerun with approved local execution. A shell-fixture escape bug embedded NUL bytes in argv; correcting the Rust escaping made the real integration tests pass. Initial full runs exposed old GUI input/name expectations; those were migrated. One later run aborted when the disk filled; only this worktree's disposable incremental cache was removed, and the rerun disabled incremental caching. A later full run hit `backpressured_input_and_send_do_not_block_inspect_or_kill` with `Broken pipe` in its readiness loop; this test had passed in the previous full run. The pre-existing readiness loop sent repeated Select requests while consuming only one frame per iteration, allowing an undrained reply backlog to fill the bounded queue. The exact disconnect branch was not instrumented; this is the source-supported diagnosis. It now selects once and waits for READY; all subsequent backpressure assertions remain unchanged. The focused test passed before and after the correction; the corrected complete suite passed (858 tests). These failed attempts are not counted as passes.

Both disposable GUI windows were closed. The first fixture, its socket, and all recorded processes were absent afterward. Final fixture `ovrcr-gui-jPQCr0`, its socket, and all 12 recorded session groups (including exited group 2610) were absent afterward. Real terminal acceptance separately checks PTY geometry and process-group cleanup; screenshots establish visible behavior, not geometry correctness by themselves.

Raw local logs and inventories: `/private/tmp/ovrcr-title-evidence`. Linux native GUI behavior and actual provider-specific title emission remain unverified. Ignored capacity/memory tests require their separate CI gates; ignored tests are not counted as passed.

## Agent label correction

The user clarified that the right-hand sidebar column should identify the agent, such as `claude`, `codex`, `grok`, `pi`, `omp`, or `cursor`. The renderer previously selected the model suffix from `agent / model` labels; it now selects the trimmed agent prefix. Labels without a suffix continue to display directly. Alignment, provider color and narrow-width clipping are preserved.

The regression failed with `shared-model` instead of `claude`, then passed for all six agents with and without model suffixes. The 26 sidebar tests passed. The real GUI integration asserts agent names and absence of model suffixes. [Native screenshot](14-agent-labels.png) and adjacent accessibility evidence show the corrected column and separate `AGENT_LABEL_VERIFIED` terminal output. This follow-up does not claim that fixture sessions are actual provider runs.

Follow-up validation: full workspace all-targets/all-features suite passed (859 tests, 16 ignored, 27 suites); Clippy and formatting passed. Native demo `ovrcr-gui-Ltdaih` exited successfully; all ten recorded session groups and fixture path were absent afterward.
