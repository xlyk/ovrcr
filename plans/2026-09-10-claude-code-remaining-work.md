# Remaining Claude Code Integration Implementation Plan

> **For agentic workers:** Use the executing-plans skill to implement this plan task by task. Use independent review for each committed unit. This document does not authorize starting implementation, changing provider trust/settings outside disposable fixtures, or publishing a release.

**Goal:** Close the remaining acceptance gaps and implement only evidence-supported completion, accounting and conversation-transition capabilities.

**Architecture:** Extend the existing synchronous supervisor, exact-source collector and runtime-owned observations. Keep the fresh-session observed/partial route working throughout. Provider certification produces explicit capability contracts before any behavior is enabled; missing sources leave a named blocker and the existing conservative fallback.

**Tech Stack:** Existing Rust workspace, serde/serde_json, synchronous Unix sockets, PTYs, Claude command hooks/statusline and GitHub Actions. No new crate, SDK, database, Tokio, pricing service or persistent analytics.

**Spec:** [Original Claude plan and dated refinements](2026-09-09-claude-code-reporting.md), [multi-harness design](2026-09-09-multi-harness-reporting-design.md), [current support contract](../docs/agent-reporting-support.md).

## Baseline and authority

Written against `cad39ab` on `codex/claude-reporting`, checkout `.worktrees/claude-reporting`. Verify current branch, commit and clean/unrelated changes before execution. Continue this isolated feature worktree or create a successor from its accepted head; do not start from main and lose the existing integration.

This plan owns the remaining work; the original design and its dated safety refinements own behavior contracts. Historical unchecked boxes are not proof of missing code. Conflicting new provider evidence requires a reviewed contract correction before dependent implementation.

Already delivered: protocol/binding/lease ownership; initial Claude 2.1.267 admission; synchronous observed activity; exact-source partial collector; context/cost publication; bounded finalization; setup/doctor; usage inspection; compact dashboard. Verified 586 workspace tests, clippy/fmt, native two-turn reporting/reattachment/exit, and full owned-fixture cleanup. The 50-session test passed its measured subset (30k reports, p99 control 1.205ms, sampled RSS growth 1.354GiB).

Not yet proved: complete accounting, post-Stop settling, broader transition admission, arbitrary child API StopFailure isolation, complete child accounting, native null-current-context timing, a full heap maximum, or kernel socket-buffer memory. Application queue occupancy and macOS/Linux capacity now have measured evidence. Native /cost included more categories than collected tokens; matching rounded final cost does not establish token completeness.

## Global constraints

- One server owner, one active dashboard, supported 50-session workload; live state ends with the server.
- Certified executable is exactly Claude Code 2.1.267, not a minimum version floor. Recheck executable/version before every provider run; unsupported versions remain fail-open/untracked.
- One supervisor lease and collector per invocation. Provider children never receive the lease. Runtime generations and conditional expected-binding mutations remain authoritative.
- Preserve one-second reporting deadlines and the single absolute two-second native-completion budget, including callbacks, collector cancellation and final RPC/receipt recovery.
- Activity, process phase, reporter health, context, token totals and cost are independent. Stop/EOF/silence/exit cannot imply confirmed settling or complete accounting.
- Exact source identity only: no cwd/home/newest-file scan. Child and auxiliary sources require explicit attributable identities and independently certified scope.
- Retain 65,536 identities/16MiB charged index bytes without eviction; 256KiB reads; 32MiB maximum raw record. Collector IPC is currently one outstanding request/response with 65,536-byte payloads (plus framing), not a 256-record queue. Preserve this tighter design; do not add a queue merely to match old prose.
- Cache counters are subsets of canonical input; cost retains independent scope/kind and checked 10^10 USD ticks. Unknown is not zero.
- Synchronous hooks only unless a source supplies certified causal ordering. Uncertain source updates cannot fabricate source revisions or freshen unrelated components.
- Scope: Claude only. No other harness adapters, user-global auto-installation, account quotas or historical reporting.

## Delivery milestones and dependency order

| Milestone | Tasks | Acceptance |
| --- | --- | --- |
| Auditable baseline | 1 | Every old requirement mapped to code/assertions/evidence or a precise gap |
| Existing supported route validated | 2, 3, 4, 9 | Fresh-session observed/partial contract passes remaining native, queue and Linux gates |
| Broader provider capabilities | 5 then 6/7/8, then 9 | Each newly enabled capability independently certified and tested |

Tasks 2–4 can proceed independently after 1. Task 5 research can also proceed then; tasks 6–8 depend on its specific certified capability, not on all research succeeding. Shared admission/collector edits are sequential. Workers implement/self-review; a different reviewer checks the exact committed diff and actual assertions. One coordinator owns Git staging; no concurrent index writers.

A release of the narrower contract is a separate user decision. This plan does not silently waive the original full-integration gates. A missing provider source can block the broader milestone without stopping acceptance of existing behavior.

## Task 1: Reconcile requirements with the delivered implementation

**Files:** original Claude plan; support document; create `research/claude-reporting-acceptance/remaining-matrix.md`. Read existing evidence, especially `final-gates/results.md`, `task7-load/scope.md`, `task7-gui-native/review.md`, and real tests.

- [x] Give each remaining requirement a stable ID and these columns: intended behavior, actual caller, assertion/test name, tested revision/platform, native evidence, gap, owning task, status.
- [x] Split mixed requirements: e.g. two-turn native acceptance is verified; permission denial, elicitation and confirmed settling are separate rows. Map clear fallback separately from supported clear/rebinding.
- [x] Inspect assertions rather than names: collector failure traffic generated by the load harness does not establish launcher-owned failure propagation; sampled RSS is not an instantaneous maximum.
- [x] Mark old checkboxes complete only where every clause has matching evidence. Link incomplete clauses here and correct historical “trust pending” shorthand without deleting prior findings.
- [x] Record the exact list of ignored tests and which run independently; zero executed tests cannot satisfy a gate.
- [x] Validate document links/whitespace and commit the reconciliation. No Rust changes or repeat broad suites.

**Gate:** No already-passing implementation is rescheduled without a concrete uncovered requirement.

## Task 2: Prove real queue bounds and overflow behavior

**Files:** `src/report/collector.rs`; `crates/ovrcr-runtime/src/agent_runner.rs`; runtime `server/mod.rs`, `server/connections.rs`, `server/dispatch.rs`, existing `server/tests.rs`; `tests/claude_reporting_load.rs`; owning Cargo manifests only if acceptance-only diagnostics need feature plumbing.

**Interfaces:** Keep the wire protocol unchanged. Prefer colocated tests for private counters. Where the existing in-process load server needs access, add an opt-in `acceptance-diagnostics` feature with a read-only aggregate `ReportingQueueSnapshot { pending_items: usize, pending_bytes: usize, peak_items: usize, peak_bytes: usize, rejected: u64 }` per real queue. No payloads, secrets, report IDs or public CLI diagnostics. Counters observe actual enqueue/dequeue/drop boundaries; they do not become another admission authority.

- [x] Inventory each actual bounded channel/buffer: dispatcher 64 entries; runtime raw events 64; dashboard 64; collector one request/response, payload 65,536 plus framing; private callback input 65,536 and actual listener/backlog semantics. State separately what is kernel-buffered, connection-limited, or byte-bounded. Do not apply the old collector 256/1MiB ceiling to unrelated terminal-output queues.
- [x] Add deterministic saturation regressions using a barrier to stop the consumer before filling. Capture actual occupancy while blocked, then admit one extra producer. Include dequeue, failed send, disconnect and teardown so diagnostic counts return to zero without underflow.

```rust
// Assertions against the actual queue under its paused consumer.
assert_eq!(snapshot.pending_items, configured_capacity);
assert!(snapshot.peak_bytes <= configured_byte_limit);
assert!(extra_send_was_rejected_or_connection_closed);
assert_eq!(after_drain.pending_items, 0);
```

- [x] Force report overflow through real AgentReport/supervisor paths. Assert explicit error or watched disconnect/unavailable state, no silent accepted loss, no new binding allocation, and no late cleanup clearing a replacement owner. Saturate dashboard output separately and prove control/lifecycle delivery or explicit disconnect.
- [x] Add only missing bounds discovered by those failures. Never replace the dispatcher, add an asynchronous runtime, or add speculative provider queues.
- [x] Extend the existing 50-session load fixture to sample real counters and exercise automatic supervisor failure propagation in a separate focused lifecycle case. Run the load once after corrections: 50 × 10Hz×60s, 3k stale reports, 10% helper failures; p99 < 250ms, max < 1s, zero cross-session updates, all queue/index limits respected, sampled process-tree RSS growth < 3GiB against the same empty-session baseline. Record sub-second RSS limitations explicitly.
- [x] Run owning runtime/collector regressions and affected clippy; review exact commit and raw counts. Keep failed saturation/load attempts.

**Gate:** Counter assertions measure actual retained state. Kernel backlog and unexposed buffers must have explicit limits/proof or remain named gaps; a single-outstanding-request test alone is insufficient.

## Task 3: Finish native acceptance of the existing supported route

**Files:** reuse `tests/server_lifecycle.rs`, `tests/claude_reporting.rs`, `tests/cli.rs`; add fixtures to the existing Claude fixture directory; evidence under `research/claude-reporting-acceptance/native-remaining/`; update support matrix.

- [ ] Launch from one final built source revision with isolated OVRCR_CONFIG/OVRCR_SOCKET/provider config/workspace. Use direct runtime PTY launch; an intermediary `rtk proxy` must not consume native stdin. Record wrapper/server/collector/dashboard revisions separately if a mixed build is unavoidable, and do not label it final-assembly acceptance.
- [x] Use existing synchronous hooks and a bounded capture wrapper that retains only event/turn/conversation/source identities and numeric measurements. Store raw sensitive provider files only in the isolated fixture; commit sanitized records without transcript content or credentials.
- [ ] Execute the cases below through actual native controls and compare CLI snapshots plus fresh GUI/AX. For every defect, first add a deterministic regression through its owning entry point, fix minimally, and repeat only the affected native case.

| Case | Required assertion |
| --- | --- |
| Approval wait then allow | Matching permission notification => WaitingInput/Observed; allow/tool activity => Busy; later matching Stop => Idle/Observed |
| Approval denial / automatic approval | Record subsequent native activity after denial; denial alone cannot imply settled completion. Automatic approval does not fabricate human wait |
| Input elicitation | If supported and attributable, capture actual unanswered input UI and matching signal; otherwise keep capability unsupported |
| Cancellation / API failure | Interrupted turn never becomes Confirmed from silence; matching StopFailure => Error/Observed; next valid turn can progress |
| Blocking Stop hook | Continued work remains observable; no Confirmed idle between Stop and continuation |
| Child isolation | Child Stop/tool/failure cannot change root state, binding or root accounting scope |
| Compaction / unknown context | Same known conversation retains binding; missing context clears display instead of reusing an old percentage; usage never sums context snapshots |
| Clear / unsupported resume or fork | Existing documented fallback becomes unavailable, freezes old metrics and rejects late callbacks; no guessed binding |
| Collector death / source loss | Owned helper failure reaches runtime unavailable automatically; native remains usable; same-path transient source recovery behaves as documented |
| Exit / dashboard lifecycle | Detach and pane close preserve native process; bounded finalization retains Partial when no final-source proof; all owned groups eventually disappear |

- [x] Compare numeric native totals with OVRCR at named observation points, including final exit. Record precision, timing and scope discrepancies; never assert complete coverage from equal rounded cost.
- [ ] Follow `docs/testing-computer-use.md`: bounded feature screenshots, literal labels, small/split layouts, actual output rather than echo, PID/PGID ownership and launcher exit. Verify cleanup by ESRCH and socket/directory absence; do not re-query a closed app.
- [ ] Update the matrix per case, commit evidence and any reviewed regression fixes.

**Gate:** All current-route cases are verified or explicitly unavailable due to an identified provider capability. No settling/accounting upgrade is made in this task.

## Task 4: Add and pass Linux acceptance

**Files:** `.github/workflows/ci.yml`; platform-specific failures only in owning runtime/report code and existing tests. Preserve macOS GUI coverage.

- [x] Add an Ubuntu headless job alongside the existing macOS all-features job. Install zsh, configure fixture Git identity, use the existing stable Rust/cache steps and a 30-minute timeout. Do not install provider credentials or run paid native Claude turns in CI.

```yaml
linux:
  runs-on: ubuntu-latest
  timeout-minutes: 30
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: rustfmt, clippy
    - uses: Swatinem/rust-cache@v2
      with:
        shared-key: linux-headless
        cache-on-failure: true
    - name: Install shell test dependency
      run: sudo apt-get update && sudo apt-get install -y zsh
    - name: Configure fixture Git identity
      run: |
        git config --global user.name "OVRCR CI"
        git config --global user.email "ci@ovrcr.invalid"
    - run: cargo fmt --all -- --check
    - run: cargo clippy --workspace --all-targets -- -D warnings
    - run: cargo test --workspace --all-targets
    - run: cargo test --workspace --doc
```

- [x] Run `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, and workspace doc tests on Linux; macOS retains all-features GUI checks. Local shell invocations use `rtk proxy`; hosted CI need not add RTK just to run cargo.
- [x] Confirm nonzero Linux executions for native argv/stdio/signals/foreground group, stopped-child cleanup, private channel descriptors/auth, reservation replacement, callback deadline, collector crash/truncation, expired-deadline reap and post-exit drain. Ignore only actual helper entry points, never whole failing behavioral suites.
- [x] Fix demonstrated platform differences with cfg-specific code only where required; preserve macOS assertions. Treat missing ps/zsh or socket permissions as environment failures, not passing skips.
- [x] Run the explicit 50-session test on an isolated Linux runner as a separate capacity attempt with the original thresholds; upload raw counters/latency/RSS and cleanup evidence. Do not make a shared busy runner's failed threshold pass by changing the limit.
- [x] Review Linux logs and macOS affected regressions. Workflow edits alone do not close Linux acceptance; require an actual completed Linux run. If no runner is accessible before a PR, record that exact external dependency.

**Gate:** Actual Linux PTY/socket/process results, not cross-compilation or a green macOS job.

## Task 5: Certify additional Claude sources before expanding behavior

**Files:** fixture manifest and sanitized captures; support document; create `research/claude-reporting-acceptance/provider-capabilities.md`. No production behavior changes.

- [x] Re-read installed CLI help and current official hook/statusline documentation, with version/date/source links. Compare to 2.1.267; a newer release is a separate certification target, never a silent widened version range.
- [x] Produce a capability table with: capability, exact event/export field, origin/root discriminator, source ordering, supported invocation forms, scope, end boundary, fixture IDs, and verdict (`certified` or `blocked` plus reason).
- [ ] Prove settled completion across blocking Stop continuation, interruption, tool/permission pauses and children. A candidate signal must occur after all relevant decisions and identify the current turn.
- [ ] Prove accounting identity and correction semantics using both equal and differing records, replay, truncation/replacement, auxiliary/child work, fresh/resumed/forked history, compaction and delayed final writes. Establish whether a supported aggregate/export replaces the transcript route.
- [ ] Prove a final-source boundary that includes all certified categories; native process exit plus an EOF scan is insufficient. Record what can still arrive afterward.
- [ ] Certify initial resume/continue, in-process foreground branch/clear/resume, background fork and compaction separately. Record how foreground intent is established before a callback; SessionStart by itself cannot authorize it.
- [x] Review each capability independently. An absent reliable signal yields a named provider dependency and an unchanged fallback; it must not trigger a timeout heuristic or an invented payload field.

**Gate:** Tasks 6–8 receive exact versioned schemas and fixtures, or remain blocked individually. An implementation plan cannot promise undocumented provider behavior.

## Task 6: Implement certified conversation transitions

**Depends on:** Task 5 foreground/identity certification; Task 1 baseline. **Files:** `src/report/admission.rs`, `src/report/claude.rs`, `src/cli/agent_setup.rs`, existing lifecycle/CLI tests, support/setup docs. Change protocol/runtime only if the existing conditional Bind contract is insufficient and separately reviewed.

**Interfaces:** Reuse `AgentCommand::Bind`, expected binding and runtime generation; keep the existing receiver as sole lease/collector owner. Enable only the exact invocation options and transition forms certified in Task 5.

- [ ] Add real hook-to-supervisor regressions before enabling any new argv form or event transition. For resume at launch, establish the intended existing conversation without injecting a conflicting new UUID. For in-process changes, require independently established foreground intent and correlation.

```text
A/g1 -> certified foreground B/g2 -> certified foreground A/g3.
Late A/g1 Stop/metrics/release must not mutate A/g3.
Background fork callback must not replace foreground A.
Same-ID compact preserves generation but clears unknown context.
Ambiguous/malformed transition freezes old identity; native continues.
Lost Bind acknowledgement resolves the original operation receipt.
```

- [ ] Stop old collection before conditional replacement; discard pending old results by binding/generation. Reset prompt, source metadata, component state and accounting only after acknowledged replacement. On uncertainty, preserve the existing unavailable fallback within the callback deadline.
- [ ] Keep history scope explicit: resumed conversation totals may include pre-invocation history; do not label them invocation-only. Define fork prefix ownership from the certified contract before counting it.
- [ ] Update doctor/setup eligibility and the capability table together; leave uncertified flags/versions disabled.
- [ ] Run focused admission/lifecycle/CLI tests and native cases for each newly enabled transition. Independent exact-commit review required.

## Task 7: Implement certified accounting and final-source completion

**Depends on:** Task 5 accounting/final-source certification; Task 6 only for newly supported transition histories. **Files:** `src/report/claude_metrics.rs`, `src/report/collector.rs`, `src/report/admission.rs`, `tests/claude_reporting.rs`, existing lifecycle tests; shared protocol/runtime only for a reviewed correction requirement.

- [ ] Prefer a certified authoritative aggregate over a more complicated reader. Otherwise enable differing-record replacement only for the certified schema/order; retain conflict freeze for unverified input.

```text
Certified same identity: 10 -> 12 => 12, not 22.
Distinct identity with 12 => 24; replay => 24.
Uncertified differing duplicate => freeze/unavailable before applying it.
Root aggregate plus included child record => counted once, not twice.
Missing category/final boundary => Partial, including after native exit.
```

- [ ] Keep checked decimal cost conversion and all index/read/IPC bounds. Define category deduplication and history scope from Task 5; do not scan for child files or infer attribution from directory layout.
- [ ] Handle certified downward corrections explicitly. The current runtime rejects cumulative decreases: do not bypass it with a new generation or null-then-lower sample. Either retain Partial/unavailable or introduce a separately reviewed supervisor-authorized correction carrying the current binding, source identity and strictly ordered correction revision. Update wire version, codec validation and all callers together only if this extension is necessary.
- [ ] Promote coverage to Complete only when all required categories and the certified final-source boundary agree. A source boundary arriving after the two-second budget leaves final metrics Partial with a reason; do not extend launcher lifetime.
- [ ] Test delayed final writes, pending pre-exit response, source replacement, contradictory export, missing auxiliary source, failed publish, lost Finalize acknowledgement, abrupt native exit and old-generation results through actual helper/socket/native paths.
- [ ] Compare exact native/reference and normalized totals with declared precision/scope for every enabled route. Review before enabling Complete in production.

## Task 8: Implement certified settled activity

**Depends on:** Task 5 settling certification; Task 6 only when applying to new transitions. **Files:** `src/report/claude.rs`, `src/report/admission.rs`, Claude fixtures, existing lifecycle tests; dashboard tests if new labels are needed.

- [ ] Add the certified event parser with exact root/conversation/turn discrimination; unknown fields/events fail open under existing limits.
- [ ] Keep ordinary Stop as Idle/Observed. Emit Idle/Confirmed only for the certified post-decision event matching the currently accepted root turn and binding.

```text
Stop(A) -> continuation(A) -> Busy: never Confirmed between them.
Turn B starts; delayed settled(A): reject without timestamp/state mutation.
Child settled(B), foreign binding, malformed/missing identity: ignore.
Reporter loss, silence and process exit: never synthesize settled(B).
```

- [ ] Run parser and actual callback-to-runtime regressions, then native blocking-Stop/cancellation/child cases. Confirm quality appears consistently in sidebar, selected header and inspection.
- [ ] Update capability docs/doctor only for the certified version. If no valid source exists, this task remains blocked and observed behavior stays supported.

## Task 9: Final contract review and delivery

**Files:** remaining matrix, both plans, support/setup/CLI docs, final evidence, existing task diary. Product changes only for findings returned to their original worker.

- [x] Review every enabled capability against exact source, actual caller, behavioral assertions and native/platform evidence. Each unresolved source gap stays visible. Distinguish internal-counter proof, sampled RSS, real provider coverage and GUI checks.
- [x] After relevant code corrections, run the final macOS all-features workspace tests/clippy/fmt once, Linux headless gates, doc tests and `rtk proxy git diff --check`. Repeat only affected checks after later relevant changes; don't rerun paid provider cases for prose edits.
- [x] Run a final native smoke from one built revision covering initial binding, two turns, metrics, reattachment and exit/cleanup; repeat expanded capabilities only if enabled. Preserve failed attempts and revision provenance.
- [x] Update evidence-backed checkboxes and extend the existing work diary. Declare the limited supported contract or full contract accurately; releasing a smaller contract requires the user's explicit scope decision.
- [x] Prepare a concise PR description with supported version/forms, changed behavior, verification and remaining limits. Push/open a PR only when authorized; merge and release require their own authorization.

**Full completion criterion:** All required provider sources are certified, enabled routes pass native and Linux acceptance, queue/load limits are proved, final review has no blockers, and documentation matches the shipped capabilities. If a provider dependency remains absent, report the exact blocked capability rather than claiming this full milestone complete.

## Evidence checkpoint, 2026-09-10

This checkpoint records completed clauses without changing the plan's full completion
criterion. The [remaining-requirement matrix](../research/claude-reporting-acceptance/remaining-matrix.md)
is the row-level authority for callers, assertions, revisions, platforms, native evidence,
and open gaps.

- Task 1 is complete: every inherited requirement is reconciled in the matrix and the
  independent review corrections are incorporated.
- Task 2 has passing application-queue saturation and overflow regressions and passing
  macOS and Linux 50-session loads. Hosted Linux recorded `somaxconn=4096`.
  Kernel socket-buffer memory and a full heap/RSS maximum remain exclusions, so the
  task's overall proof is not complete.
- The runtime ownership-guard regression covers supervisor socket-owner disconnect,
  including the exact `supervisor_disconnected` reason, retained Partial metrics, a
  replacement reservation, and rejection of late cleanup. It does not simulate a
  literal supervisor-process kill. Provider exec descriptor inheritance also remains
  without a direct assertion.
- Task 3 has native evidence for actual output, two turns, approval and tool paths,
  continuation, compaction, source/collector loss, detach/reattach, exit, and cleanup in
  [native-remaining](../research/claude-reporting-acceptance/native-remaining/results.md)
  and [native-final](../research/claude-reporting-acceptance/native-final/results.md). The
  invalid-model `StopFailure` and same-binding recovery pass in native-final cases 07 and
  08. The final [integrated native evidence](../research/claude-reporting-acceptance/native-integrated/results.md)
  records a contemporaneously clean `d12abda` source, two turns, full uncertainty
  labels, reattachment, exit, and cleanup.
- Task 4 is complete at exact `d12abda`: private PR 55 run 34518312559 passed the
  macOS, Linux, and isolated Linux-capacity jobs. Linux passed 607 tests with 11
  ignores; the capacity case passed 30,000 reports, 3,000 stale rejections, five
  collector failures, latency/RSS thresholds, queue bounds, and owned cleanup. The
  earlier local Docker interruption is historical; its container/image cleanup remains.
- Task 5's source review and capability table passed independent review. Required provider
  sources for settled completion, complete accounting, final-source completion, and wider
  conversation transitions were not found, so those capabilities remain blocked.
- Task 6 has a real same-ID compaction regression. Native compaction reported explicit zero
  current usage; native null-current-context timing remains open. Broader transition forms
  remain disabled. Tasks 7 and 8 remain blocked on the missing Task 5 provider sources.
- Integrated macOS all-features testing at `d12abda` passed 633 tests with 11
  intentional ignores across 24 binaries; Linux passed 607 with 11 ignores across
  23. Clippy and formatting passed on both, while doctest commands executed zero
  tests. The contemporaneously clean native run passed two turns, 62-column metrics,
  reattachment, exit, and cleanup. A targeted native child run at unchanged
  `d12abda` proved real child tool-failure and `SubagentStop` isolation.

Independent final evidence review approved `2d18bb0` on 2026-09-10, and the
existing work diary was extended. Draft PR 55 run 34520248256 passed all three
jobs at documentation head `c2fb7ea`; the PR remains open, draft, and mergeable.
The current default `claude` executable is 2.1.268, while retained discovery and
native evidence verify 2.1.267; the supported-version contract therefore remains
exactly 2.1.267. A read-only Docker information check timed out after five seconds
on 2026-09-10, so the historical task-owned container/image cleanup remains
unverified; no shared-service restart was attempted. These completed review and
documentation actions do not close the provider-source, arbitrary child API
`StopFailure`, complete child-accounting, native null-context, full-heap, kernel
socket-buffer accounting, or local Docker cleanup gaps. Merge and release are not
authorized.
