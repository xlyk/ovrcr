# Implementation progress — scheduled Pi tasks

Base: origin/main 84f8bd0. Isolated worktree /private/tmp/ovrcr-scheduled-tasks.

## Ownership

- Task model/schedule/store: worker, src/tasks.rs and focused tests.
- Pi supervisor: worker, src/task_runner.rs and focused tests.
- Background service: worker, src/service.rs and focused tests.
- Server/CLI and cross-module integration: root.
- TUI and independent review: later scoped workers.

## Rulings

- Use disjoint-file parallel workers for independent modules; shared type/API contract is in the plan. Root alone edits common module exports/dependencies/protocol/main/server.
- No automatic service installation on the user's real account during development. Use isolated service probes and explicit evidence.
- Preserve existing registry and terminal wire variants; append task protocol variants only.

## Status

- Baseline: 34 focused tests passed.
- Task store: 12 tests passed after two meaningful RED cycles.
- Service: 13 tests passed. Native isolated LaunchAgent probe passed scheduled execution, crash restart, stop/start/uninstall, and retained history (label com.ovrcr.server-e7cb4863919afa20).
- CLI: 3 tests passed. Execution: 10 tests passed, including CLI and signal shutdown despite final-state persistence failure.
- TUI: 14 tests plus 23 existing dashboard tests passed. Real GUI created and edited a multiline-prompt task, changed concurrency, paused it, displayed delete confirmation, and showed an authenticated xai/grok-4.6 run succeed with OVRCR_GUI_RPC_OK. GUI exited 0 and fixture removed.
- Installed Pi 0.84.4 through OVRCR: controlled local SSE provider proved streaming before completion and persisted session.
- Supervisor: 8 tests passed including both installed-Pi gates; native process birth identity guards have 2 unit tests.
- Independent integration review: seven findings corrected. Barrier tests cover cancel/admission and graceful-shutdown/admission. Fault test covers final-state-write failure. Stable-ID paging tests cover concurrent insert/delete between pages.
- Final independent recheck closed all seven integration findings. Runner identity review also completed.
- Final `cargo test --all-features -- --include-ignored` with `OVRCR_TEST_PI_PACKAGE` set: 170 tests passed, zero failures or ignored tests. Formatting, strict all-target/all-feature Clippy, Python probe compilation, and diff checks passed. Linux native service lifecycle remains untested on this macOS host.
- Work diary updated with final results. Implementation is complete in the uncommitted isolated branch. No commits, pushes, or merge performed.

## Constructive review corrections

- Closed all five reproduced findings: run cleanup ownership (including same-branch replacements), active-task workspace removal, native-service peer/job identity, Unicode duration validation, and ordinary Pi bash background-process cleanup.
- Explicit bundled Pi bash extension retains process-group ownership until run end and reacts to parent-control EOF. Discovered extensions remain disabled. Background work survives between tool calls.
- Fresh independent fix recheck: PASS for all five scoped corrections. Meaningful RED regressions preceded fixes.
- Full installed Pi 0.84.4 RPC background probe passed through a local provider; background lived between tools and was gone after Succeeded. Native temporary LaunchAgent probe passed with the PID guard and uninstalled successfully.
- Combined testing exposed an old one-second Running-state timing assumption in timeout/coalescing coverage. Replaced it with an acknowledged HOLD run and explicit release before checking the queued timeout; final combined suite passed all 170 tests.
- Detailed correction evidence: `/private/tmp/ovrcr-fix-verification.md`.
