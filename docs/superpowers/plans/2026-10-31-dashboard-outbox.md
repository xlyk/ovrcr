# Dashboard Outbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Implement ticket [#384](https://github.com/xlyk/ovrcr/issues/384): one existing Outbox responsibility for fully formed steady-state Dashboard requests.

**Architecture:** Retain the existing queue and public Dashboard message/drain seam. Former action-carried requests schedule due History work before enqueueing; existing queued producers retain their queue-before-due order. The event loop transports Outbox-drained batches and retains late fresh-input authorization, clipboard effects and startup handling.

**Tech Stack:** Existing Rust workspace, Crossterm/Ratatui, Unix sockets and outer-PTY acceptance fixtures; no new dependencies.

**Spec:** [#383](https://github.com/xlyk/ovrcr/issues/383), with the local design in `docs/superpowers/specs/2026-10-30-dashboard-outbox-design.md` as background.

## Global Constraints

- Preserve synchronous Server, one active Dashboard and fifty sessions.
- Preserve fully formed request order, exactly-once delivery, request/error identities and existing wire/persistence/settings/key-binding/layout behavior.
- Keep fresh-input authorization, clipboard effects and startup handling at existing seams.
- Include only scoped ordering defects demonstrated by an actual failing behavioral regression; never weaken assertions.
- Implementation is authorized by Kyle's `/implement`; proceed with bounded delegation and root-owned serialized builds/tests.

## Review Focus

- A due History page formerly emitted before an action request must not move behind it: the production dispatch characterization pins this order.
- Ignored responses must not become accidental drains: the no-drain socket characterization pins the empty write.
- Cleanup must retain captured control/view identity while fresh input uses current authority: socket-frame assertions and the real-child retarget acceptance case cover both.
- History cursor resolution, pending-page suppression and clipboard cancellation must retain current side effects: migrate and retain the existing Dashboard regression suites.
- Startup/Quick Operator, view-before-save and socket failure must retain existing timing: preserve existing startup/action-writer tests and run affected integration suites.

## Task 1: Complete the existing Outbox responsibility

**Files:**
- Core: `crates/ovrcr-tui/src/dashboard/outbox.rs`, `state.rs`, `mod.rs`, `event_loop.rs`.
- Producers: Dashboard `palette.rs`, `lifecycle_ui.rs`, `assignment_control.rs`, `settings_editor.rs`, `desktop.rs`, `events.rs`, `iterm.rs`, `quota.rs`, `operator.rs`; preserve already-queued navigation and Operator producers.
- Tests: Dashboard internal tests, `crates/ovrcr-tui/tests/settings_theme_preview.rs`, `tests/tui/**`, `tests/operator_e2e.rs`, `tests/server_lifecycle.rs`, `tests/terminal_acceptance.rs`; two observed fixture races corrected separately in `tests/pi_delegation.rs` and `tests/provider_quota.rs`.
- Documentation: `docs/development/tui.md`; clarify existing Outbox vocabulary only where needed.

**Interfaces:**
- Retain public `drain_outbox(&mut self) -> Vec<ClientMessage>` and `handle_server_message(&mut self, ServerMessage) -> Vec<ClientMessage>` with existing drain/no-drain cutpoints.
- Keep existing `push_request(ClientMessage)` semantics for already-queued requests.
- Add private `queue_history_request(&mut self)` to append any request from the existing scheduler.
- Add private `enqueue_request(&mut self, ClientMessage)` to schedule due History first, then enqueue a former action-carried request.
- Remove request-carrying action variants and `drained_action`; producers enqueue and return the remaining redraw/effect action. Preserve raw PTY handling.

- [x] Refresh `origin/main`, reuse isolated checkout, and create `codex/384-dashboard-outbox` at `07b792fb9854e38db671ec51fc81ecee25d70907`; preserve the earlier design artifact.
- [x] Run baseline `cargo test --locked --offline -p ovrcr-tui --lib` with isolated HOME: 341 passed, 0 failed.
- [x] Run the new `outbox_dispatch_tests` on original production code: public palette key then outgoing drain failed behaviorally (Inspect count 0 instead of 1); both socket-order characterizations passed. Log: `02-red-outbox-dispatch.log` (2 passed, 1 failed).
- [x] Run `outbox_retarget_releases_original_mouse_child_before_fresh_input` through the compiled binary, real Server and outer Dashboard on original production code: 1 passed, with replacement INPUT_ACK and the original child's exact cleanup checkpoint. Log: `03-baseline-real-dashboard.log`.
- [x] Implement the private queue coordination, remove request-carrying action variants and migrate all producers/consumers/assertions together. Focused ownership/order, owning TUI and root TUI suites pass; the post-refactor real application case passes within the 19-case terminal acceptance suite (log `18-macos-workspace.log`).
- [x] Update owning docs; complete local macOS/Linux Rust CI-profile, typecheck, lint, formatting, doc steps and scoped native GUI acceptance. Preserve failed Cargo attempts, two macOS flaky cases, ignored/skipped counts and cleanup limits in the verification report. Native original-launcher exit 0 and all twelve owned groups/root/socket absence are recorded; the Linux runner and custom image are removed.
- [x] Commit source (`4cb02011`) and scoped fixture corrections (`8a588000`); independent Standards and Spec reviews of both have no blocking findings. Extend the existing work diary with exact revisions, results and gaps. Keep delivery local; no push, PR or merge.

## Evidence

Root-owned logs and receipts: `/private/tmp/ovrcr-384-evidence/`.
Each attempt uses a distinct log. Application, native and platform evidence remain separate.

Final record: [`research/dashboard-outbox-2026-10-31/verification.md`](../../../research/dashboard-outbox-2026-10-31/verification.md). Both local Rust CI-profile waits exited 0: macOS 2431 passed (two flaky cases), Linux 2182 passed (no retries). Earlier Cargo failures remain failed; hosted CI and the explicitly listed native/provider checks remain unrun.
