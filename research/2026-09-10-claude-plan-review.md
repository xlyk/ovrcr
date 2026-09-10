# Claude integration plan review

Reviewed 2026-09-10 against application baseline `188a6456e42607634aedc72d3a93697faebcb0e1` and the untracked, 190-line [Claude plan](../plans/2026-09-09-claude-code-reporting.md). Three independent sub-agents reviewed architecture, Claude contracts, and acceptance/compatibility. The coordinator checked the findings against the shared design and current caller paths.

**Verdict: revise before implementation.** Two P1 and four P2 findings below are missing plan contracts, not demonstrated bugs in an implemented feature. The source-discovery work in Task 1 is appropriate to begin; dependent protocol/adapter implementation should wait for the relevant contracts to be resolved. No code, plan, provider settings or live sessions were changed. No Rust suites or paid provider turns were run.

## 1. P1 — Stale bind requests can reclaim ownership

**Location:** Claude plan line 68; runtime `session/mod.rs:586`.

`BindAgent` authenticates with the PTY-wide capability and carries provider/invocation/conversation, but has no current-owner precondition. Delayed invocation A can bind after B and receive a fresh generation. Rejecting A's old reports and unbinds does not stop that request from taking ownership back.

**Required correction:** define launcher ownership registration and conditional conversation transitions against the active invocation/generation. Add deterministic tests for delayed initial bind, delayed replacement bind, duplicate bind, and A→B→A. Binding creation itself needs stale-owner protection; report validation alone is insufficient.

## 2. P1 — Root SessionStart does not establish foreground ownership

**Location:** Claude plan lines 90–95; shared design lines 56–60.

The plan permits a root SessionStart to establish the conversation without specifying how it distinguishes a foreground transition from a background fork. Claude documents fork and compact sources; root/child metadata is not documented as a complete foreground-ownership signal. A background copy could replace the displayed foreground binding, while same-conversation compaction could unnecessarily reset it. See [SessionStart reference](https://code.claude.com/docs/en/hooks#sessionstart) and [Claude release notes](https://platform.claude.com/docs/en/release-notes/claude-code).

**Required correction:** certify foreground admission in Task 1, then encode it in Task 3. Separately test foreground branch, background fork, clear, same-conversation compaction, and delayed starts. Background work must not take ownership; compaction must preserve the binding. If the provider cannot distinguish these transitions, record an explicit support restriction or blocking dependency instead of accepting every root start.

## 3. P2 — Shared schema lacks component freshness and reporter health

**Location:** Claude plan lines 142 and 149; shared design lines 125–146.

The plan requires context, token and cost measurements of different ages, but the schema supplies one metrics revision/receipt time. A new transcript total could make old context/cost appear fresh. The promised uncertain-freshness state also has no representation. Collector failure/overflow requires unavailable status, yet `AgentObservation` has only activity/metrics; short-lived report socket closure cannot establish collector death.

**Required correction:** define per-component measurement provenance/freshness and a binding-scoped supervisor health update in Task 2. Specify the supervisor-loss path without inferring activity from silence. Test transcript-only progress preserving context/cost age, uncertain source freshness, explicit collector failure, and late health updates from an old binding.

## 4. P2 — Final usage can be lost during binding release

**Location:** Claude plan line 107; shared design line 56; runtime `session/mod.rs:575–594`.

SessionEnd/native exit releases reporting ownership, and the existing PTY exit path revokes the capability. A collector that has not consumed the last transcript record can have its final snapshot rejected, leaving understated totals labeled complete.

**Required correction:** specify a bounded final extraction/publication acknowledgement before normal release, plus failure behavior that retains explicitly partial results. Define abrupt process/collector death separately. Add a deterministic case in which native completion occurs before collector catch-up; verify final accounting is applied or marked incomplete, never silently complete.

## 5. P2 — Bounded exact accounting lacks an algorithm and measurable gate

**Location:** Claude plan lines 139 and 172.

A bounded recent-ID cache cannot identify replay of an evicted ID. A full single-pass rebuild still needs state proportional to unique identities for last-record replacement. Queue bounds and bounded iteration work do not bound that index or overall rebuild cost. The two-request fixture does not test this boundary.

**Required correction:** select a concrete accounting strategy with explicit resource limits, or resolve feasibility before committing the collector design. Test more identities than the cache window, replay/correction of evicted records, file replacement and cancellation during rebuild. Set memory and control-latency acceptance thresholds for the 50-session test. Do not describe full replay alone as the bounded-memory solution.

## 6. P2 — New binding rules break the documented legacy status line

**Location:** Claude plan lines 82 and 152; existing `docs/agent-reporting.md:160`; `src/cli/report.rs:54`.

The existing configured renderer is `ovrcr report claude-context --stdin-json`. It sends a legacy report and exits before printing when rejected. The new rules reject legacy updates during an active binding. Composing that existing renderer unchanged therefore removes its context output even with a healthy server.

**Required correction:** explicitly migrate this known renderer to the new reporting/rendering route while preserving its behavior outside managed bindings. Add a real CLI regression using the documented legacy configuration under an active binding and assert that `ctx N%` remains visible. Also retain the reporting-unavailable/user-renderer-success test.

## What the review did not flag

- Stop settling, cancellation and complete resumed/child usage are already named research dependencies; they are not silently assumed solved.
- Current context is correctly separated from cumulative consumption, and the Claude input/cache occupancy formula matches the documented fields.
- Exact decimal cost parsing remains feasible because the metrics parser accepts raw bytes; the existing serde_json dependency alone is not evidence of unavoidable precision loss.
- Reusing the existing synchronous transport/runtime and preserving provider trust/configuration remain appropriate.

## Recommended next step

Revise the plan and shared schema together for findings 1, 3, 4 and 6. Complete bounded source/algorithm discovery for findings 2 and 5, then review those resulting contracts before implementing the dependent stages. Preserve the original review and record resolutions against each numbered finding.


## 2026-09-10 correction review

The coordinator revised the Claude plan and shared design, and added an authority note to the broader plan. All three original reviewers checked their assigned corrections without expanding the audit:

- Architecture reviewer: findings 1, 3 and 4 addressed by conditional reservation/binding, component freshness and supervisor health, and bounded finalization.
- Claude contract reviewer: finding 2 addressed by foreground admission prerequisites, explicit background/compaction rules, and unavailable/relaunch fallback for uncertified transitions.
- Acceptance reviewer: findings 5 and 6 addressed by a capped non-evicting index with partial-prefix behavior and measurable load gates, plus explicit legacy-renderer migration and real CLI regression requirements.

**Revised verdict:** the six document-level findings are resolved. Task 1 source certification remains required before dependent implementation; no native provider behavior or application acceptance is certified. The original review above is preserved as the historical record. Link, fence and whitespace validation passed for all three plan/design documents; git diff --check passed. No application code changed.
