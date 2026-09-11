# Codex response-ready hooks implementation plan

> Planning only. No adapter or readiness feature is implemented by this document. Use one implementation worker and a separate reviewer for each unit; the coordinator alone stages files. Implement units in order and review their committed revisions.

**Goal:** Let the user see when a managed native Codex terminal has finished a root response and is ready for review.

**Architecture:** Native hooks feed the existing supervised private socket, invocation lease, receipt handling and runtime activity snapshot. Add one activity value, `ResponseReady`, and render it in the existing dashboard. No transcript collector is needed.

**Tech stack:** Existing synchronous Rust workspace, serde, Unix sockets, PTYs and dashboard. No new service, database, async runtime, provider SDK or replacement terminal.

## Authority and scope

Kyle approved hooks after clarifying that the priority is knowing when a new response is ready for review. This is the explicitly smaller milestone requested by the [original reporting plan](2026-09-10-codex-cli-reporting.md). For this milestone, token accounting and continuous foreground-conversation identity are no longer prerequisites. The original investigation remains a valid record of why broader reporting is blocked; its unfinished accounting and source-certification tasks remain unfinished.

Planning baseline: `01562874fa03901634bcc5133a4686ca54e324a1`, branch `codex/codex-reporting`, worktree `/Users/xlyk/Code/ovrcr/.worktrees/codex-reporting`, draft [PR #56](https://github.com/xlyk/ovrcr/pull/56). Shared foundation originally merged at `983dd42a05690b15db073e6c1d8454d5a889db63`. Refresh main before implementation and reconcile any merged Pi helpers; never modify the Pi worktree.

Read `AGENTS.md`, `docs/development/{architecture,runtime,testing,tui,delivery}.md`, `docs/testing-computer-use.md`, the [shared design](2026-09-09-multi-harness-reporting-design.md), [support contract](../docs/agent-reporting-support.md), [native hook evidence](../research/codex-reporting-acceptance/source-contract.md) and [backtrack evidence](../research/codex-reporting-acceptance/history-invalidation.md). The latter two describe the old exact-source contract; this document explicitly replaces that contract only for hook-observed response readiness.

### User-visible contract

- `busy · observed`: an authenticated root prompt was submitted.
- `response ready · observed`: a matching root `Stop` arrived for that prompt. This says a native response ended; it does not certify the answer, task success, or complete accounting.
- `idle · observed`: an authenticated interruption ended the current turn. It never produces Ready.
- Unknown or unavailable stays explicit when reporting cannot establish a state. Silence, process exit and terminal text never produce Ready.
- The ready label remains until the next accepted root prompt, interruption or reporting invalidation. It survives dashboard reconnects through the server's existing snapshot. Selecting a session does not clear it.
- This is the **last observed root turn in the managed terminal**, not a claim about which historical conversation is currently displayed. Browsing/backtracking without submitting a prompt may leave the label unchanged. The next authenticated prompt can establish a new conversation binding.
- First release uses a visible dashboard label and the existing CLI inspection path. It has no sound, OS notification, unread count, “mark reviewed” action or notification preferences. Those can follow separately if needed; the native GUI currently has no proven audible-bell path.

Excluded: metrics/rollout reading, token or cost totals, complete accounting, Confirmed completion, arbitrary provider failure inference, permission-state tracking, continuous history selection, historical unread recovery and alerts after server death. Native CLI approval behavior remains native.

## What to reuse from Superset

Source comparison is pinned to Superset `c7289c606c2dbded6c05244d4e6a755e15f67225`; keep the repository's older Superset research pin intact.

| Superset behavior | OVRCR decision |
| --- | --- |
| Native `UserPromptSubmit` becomes Start; root `Stop` drives completion status/chime | Reuse event meaning: Busy then ResponseReady, both Observed. |
| Child events are routed separately and do not chime the parent | Reject explicit child events before processing any conversation identity. No child roster in this milestone. |
| Terminal identity associates a hook with its workspace | Reuse OVRCR's authenticated invocation/socket ownership plus OS-verified native-parent relationship. Never trust a payload terminal ID or first callback alone. |
| Wrapper's process-specific TUI log is a compatibility source for start/permission events | Do not add it: the pinned native hooks already cover this milestone. No visible terminal pattern matching. |
| Interrupt is mapped into Stop | Deliberately differ: interruption is Idle, never ResponseReady. |
| Wrapper enables hooks and bypasses hook trust; setup manages hooks.json | Do not copy the bypass or assume that file format works for installed Codex. Generate the exact supported version's configuration for user installation, preserving trust and unrelated hooks. |
| Host notification endpoint accepts low-impact lifecycle events | Do not copy its unauthenticated endpoint. Reuse OVRCR's private bounded transport and lease. |

Primary source links: [managed events/setup](https://github.com/superset-sh/superset/blob/c7289c606c2dbded6c05244d4e6a755e15f67225/packages/agent-setup/src/agent-wrappers-claude-codex-opencode.ts), [wrapper](https://github.com/superset-sh/superset/blob/c7289c606c2dbded6c05244d4e6a755e15f67225/packages/agent-setup/templates/codex-wrapper-exec.template.sh), [hook routing](https://github.com/superset-sh/superset/blob/c7289c606c2dbded6c05244d4e6a755e15f67225/packages/agent-setup/templates/notify-hook.template.sh), [event mapping](https://github.com/superset-sh/superset/blob/c7289c606c2dbded6c05244d4e6a755e15f67225/packages/host-service/src/events/map-event-type.ts), [root/child notification handling](https://github.com/superset-sh/superset/blob/c7289c606c2dbded6c05244d4e6a755e15f67225/packages/host-service/src/trpc/router/notifications/notifications.ts).

## Hook contract and failure behavior

Start with the already observed Codex CLI `0.153.0` hook schema and fresh interactive launch grammar. Unknown versions/forms run natively with reporting unavailable. No new general source investigation or full certification matrix is scheduled.

| Input | Required evidence and effect |
| --- | --- |
| SessionStart | Authenticated root; retain bounded startup context only. Does not emit Ready or establish ownership by being first. |
| UserPromptSubmit | OS-verified native root sender, no child marker, nonempty session_id and turn_id. Establish the active `(conversation, turn)` and publish Busy. |
| Stop | Same native invocation and active `(conversation, turn)`, root only. Publish ResponseReady once, then close that turn. |
| Interrupt | Same active root turn. Publish Idle and close it; a later Stop for that turn cannot produce Ready. |
| Subagent events or any nonempty agent_id | Ignore before identity/rebinding/state logic, even when transcript_path points at the root file. |
| SessionEnd/native exit | End reporting through existing lifecycle handling. Never synthesize Ready. |
| Unknown/malformed/missing-ID event | Drop without state or freshness changes. Never default unknown input to Stop. |

Use a synchronous, direct-exec hook command so the accepted helper's OS parent remains the supervised native child. The private receiver must establish native process identity independently of hook JSON, validate kernel socket peer credentials and that peer's OS parent while connected, and fence against native lifetime loss/PID reuse. If that relationship cannot be established, reporting stays unavailable while Codex remains usable. Do not weaken ownership to serialized PPID, first arrival, newest file or terminal text.

The receiver serializes accepted events. Require the supported native synchronous hook path to preserve root prompt/ending order; prove that narrow integration contract in Task 2. A Stop before its prompt is dropped, not buffered. A mismatched or closed-turn Stop is ignored. Duplicate prompts must not reopen closed turns. Keep a bounded set of accepted turn identities (at most 65,536, at most 16 MiB charged identity storage); on exhaustion, disable reporting for the invocation rather than evicting identities and admitting old replays. Reject overlapping new root prompts while an earlier accepted turn is still open; mark reporting unavailable rather than guessing which delayed event is current. Missing hooks may therefore prevent further reporting, but cannot manufacture Ready. Do not use timeouts as completion detection.

On a root prompt in a different conversation after the prior turn closes, use existing compare-and-exchange Bind with the expected current binding, then Busy in the returned generation. SessionStart alone never replaces a binding. The backtrack evidence already establishes that the next prompt carries the new identity; no file access or immediate switch detection is required. Receipt loss/recovery must complete before publishing against the new generation; reuse existing deadlines and fail-closed behavior.

This scope permits event-time rebinding after in-process backtracking. Initial picker/resume/remote forms stay outside the first supported launch grammar. Genuine API failures without a Stop hook may leave no readiness event; document this limitation instead of introducing a transcript reader or treating disappearance as success.

## Implementation units

### Task 1 — Add the minimal readiness state and preserve snapshot behavior

**Files:** `crates/ovrcr-protocol/src/session.rs`, relevant serialization tests, `crates/ovrcr-runtime/src/server/tests.rs`, `crates/ovrcr-tui/src/dashboard/{render,tests}.rs`, `src/cli/{args,resources}.rs`, `tests/cli.rs`.

- [ ] Add `AgentActivity::ResponseReady`; retain existing wire spellings and update exhaustive matches. Use the normal enum serialization convention for the new value.
- [ ] Use existing `ActivitySample { state, quality: Observed, turn }` and `AgentSnapshot.activity_revision`. Do not add a separate readiness queue or acknowledgement watermark.
- [ ] Render `response ready · observed` distinctly using the existing row layout, including narrow/split layouts. Keep reporter health and process exit visible independently. Never relabel generic Idle as ready.
- [ ] Include the state in existing CLI inspection/manual activity parsing consistently. Metrics remain unknown rather than zero; no Codex metrics adapter.
- [ ] Add meaningful serialization and real runtime-socket tests: Ready snapshot round-trip, stale revisions rejected, reconnect retains Ready, next Busy replaces it. Dashboard assertions cover label and clipping.
- [ ] Run focused protocol/runtime/TUI/CLI tests and formatting. Reviewer checks the committed state-only unit before provider wiring.

### Task 2 — Connect authenticated Codex hooks through the supervised launcher

**Files:** `crates/ovrcr-runtime/src/agent_runner.rs`, `src/report.rs`, new `src/report/codex.rs`, `src/cli/{agent,report,args}.rs`, `tests/server_lifecycle.rs`, new `tests/codex_reporting.rs`.

- [ ] Add provider-neutral trusted request metadata to `HookEvent::Request`: a runtime-computed native-root relationship, anchored to the supervised child's lifetime. Keep Codex parsing out of runtime; update Claude callers without changing their behavior. Implement macOS/Linux credential checks with existing platform dependencies where possible.
- [ ] Add `reserve_invocation_for(AgentProvider)` while retaining the Claude compatibility wrapper; generalize private payload dispatch by provider. Reuse any equivalent helper already merged by Pi.
- [ ] Implement `codex::receiver` returning the existing HookHandler and the hook table/state machine above. Do not create a collector. Deduplicate before generating report revisions so repeats never refresh the sample.
- [ ] Add `ovrcr agent run codex -- ...` and `ovrcr report codex --stdin` dispatch. Outside a managed invocation the hook command safely no-ops. Do not fall back to an unauthenticated legacy route.
- [ ] Preserve native argv, PTY, signals, exit code and approvals. Preserve the 65,536-byte envelope limit, 800 ms request deadline, one-second reporter budget and absolute two-second completion budget including receipts. Reporting startup failures must not prevent native launch.
- [ ] Write RED tests through the actual managed CLI/PTY/private socket using deterministic hook-producing child processes: prompt→Stop gives Ready; prompt→Interrupt→Stop never does; duplicate prompt/Stop cannot reopen or refresh; old-turn Stop cannot finish a new turn; child Stop with root path and foreign-first callbacks cannot claim ownership.
- [ ] Test cross-conversation next-prompt rebinding, late old-generation reports, lost Bind receipts, malformed/oversize payloads, missing hooks, identity-capacity exhaustion and native death. Verify output/exit and cleanup, not just returned errors.
- [ ] Verify the pinned native synchronous hook ordering/direct-exec relationship with a focused two-turn and interruption exercise. If the necessary ordering or parent proof fails, retain that failure and stop this unit; do not silently relax it or restart broad research. Automated synthetic tests alone do not prove the native contract.
- [ ] Reviewer checks exact committed changes and meaningful test execution, including unchanged Claude behavior. Fix findings before the next unit.

### Task 3 — Make setup and diagnostics usable without altering live configuration

**Files:** new `src/cli/codex_setup.rs`, `src/cli/{mod,agent,args}.rs`, `tests/{agent_setup,cli}.rs`, new `docs/codex-reporting-setup.md`, `docs/agent-reporting-support.md`, affected CLI reference/help.

- [ ] Dispatch setup/doctor explicitly by provider; resolve doctor executable defaults from the selected provider instead of always using Claude.
- [ ] Print the supported 0.153.0 configuration using the TOML hook form retained in the native evidence, for SessionStart, UserPromptSubmit, Stop, Interrupt and SessionEnd. Show the absolute hook helper/direct-exec command and version requirement. Do not blindly port Superset's hooks.json format.
- [ ] Preserve unrelated hook handlers and ordering in documented composition; installing the example is an explicit user step. Do not write live trust/config, inject trust bypasses, replace approval handlers or inspect credentials. Hooks must return the native no-op response, never an approval decision.
- [ ] Doctor reports executable/version, hook setup requirements and managed-launch instructions without requiring a running server or invoking a provider conversation. State that presence of configuration does not prove hook trust or delivery.
- [ ] Add CLI/setup regressions for paths with spaces, mixed provider dispatch, unrelated handlers, unsupported versions and no-op outside managed launch. Verify all tests actually execute.
- [ ] Document exact semantics: terminal's last observed root turn, next-prompt clearing/rebinding, interruption, missing-hook/API-error limitations, reconnect behavior and first-release exclusions. Mark support planned until Task 4 passes.
- [ ] Reviewer checks setup and docs against actual command output and the supported native schema.

### Task 4 — Accept and deliver the narrow feature

**Files:** retained evidence under `research/codex-response-ready-acceptance/`, support/setup docs, this plan's checkboxes, existing work diary entry.

- [ ] Refresh main and reconcile shared changes without touching Pi's checkout. Have an independent reviewer inspect the complete committed diff against this plan before native GUI acceptance.
- [ ] Run relevant full workspace checks once the reviewed implementation is assembled; preserve failures and any justified retries. Use `CARGO_INCREMENTAL=0` while disk is constrained. Follow the development guides for exact commands and hosted capacity/memory gates; no local 50-session stress on the busy host.
- [ ] From the reviewed checkout, use actual OVRCR native GUI with isolated config/socket/workspace and version-pinned native Codex. Reuse credentials only within the user's authorization; never retain them in fixtures. Obtain only applicable isolated trust approval; do not reuse Claude approval or alter live configuration.
- [ ] Observe a distinct assistant output marker, then background-session Ready; submit another prompt and observe Busy then a second Ready. Disconnect/reconnect the dashboard and verify the retained label without acknowledgement or replay effects. Capture screenshots and relevant accessibility evidence.
- [ ] Exercise interruption and a real child event without parent Ready; backtrack and submit a new root prompt to verify readiness for the new conversation. Do not claim that the label follows callback-free history browsing. Retain all failed attempts separately from successful acceptance.
- [ ] Record macOS native results separately from Linux automated/platform results. Confirm real PTY/socket ownership, version, source revision and task-owned PID/group cleanup. Preserve resources with uncertain ownership and historical failed evidence.
- [ ] Update support/setup docs to only capabilities actually accepted. Final independent review must cover any acceptance-driven changes. Push the final revision to the separate Codex PR; require all current-head CI checks to pass. Do not merge or release.
- [ ] Extend the existing work diary entry, then report PR, exact revision, implemented readiness behavior, test/native/platform results and remaining gaps. No original metrics or continuous-source checkbox is completed by this feature.

## Plan validation and execution handoff

The first useful delivery is the dashboard's response-ready label. A sound or durable unread workflow would need separate UX and host support; neither is hidden in these tasks. Code exploration confirmed that existing runtime snapshots already retain activity across reconnects, so no acknowledgement protocol is required for the selected semantics.

Before staging implementation, verify branch/base/working-tree state and preserve the existing investigation. Each worker gets the current unit and relevant contracts; the reviewer checks the exact committed unit independently. The coordinator owns all staging. This planning artifact is not native acceptance, an implemented hook adapter or a resolution of the broader reporting blocker.
