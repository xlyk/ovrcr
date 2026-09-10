# Claude Code Remaining Capabilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development or executing-plans to implement this plan task by task. Workers implement; a different reviewer checks each exact committed unit. One coordinator owns staging. Steps use checkboxes for tracking.

**Goal:** Finish the remaining Claude reporting capabilities and reliability proofs without weakening the existing supported contract or claiming provider guarantees that cannot be established.

**Architecture:** Extend the current synchronous launcher, admission receiver, exact-source collector, and runtime-owned reports. First produce a reviewed, version-specific source contract for each new capability; then implement and verify that capability independently. Missing provider primitives are explicit external dependencies, not permission to replace certainty with heuristics.

**Tech Stack:** Existing Rust workspace, serde/serde_json, Unix sockets, PTYs, native Claude hooks/statusline, macOS CUA, existing Linux CI. No SDK migration, Tokio, database, new harness adapter, or pricing service.

**Spec:** [Original design and safety refinements](2026-09-09-claude-code-reporting.md), [previous remaining-work plan](2026-09-10-claude-code-remaining-work.md), [support contract](../docs/agent-reporting-support.md), [requirement matrix](../research/claude-reporting-acceptance/remaining-matrix.md), [capability contracts](../research/claude-reporting-acceptance/provider-capabilities.md).

## Authority and starting point

This is the next execution plan after Part 2. It supersedes the previous plan's scheduling of unfinished work, not its behavior contracts or historical evidence. A user request to prepare this plan does not start its implementation. Merge/release remain unauthorized; PR 55 stays draft while the full milestone is unresolved.

Inspected checkout: `/Users/xlyk/Code/ovrcr/.worktrees/claude-reporting`, branch `codex/claude-reporting`, clean at `f82ce6e1bbddf53efe9f4d6074fdaa2006e4643e` before this plan was written. Product implementation is `9585271`; subsequent commits retain evidence and documentation. Last verified CI run 34533362749 passed macOS 639 tests, Linux 613 tests, and the 50-session capacity job. Each workspace suite had 11 ignored helpers/acceptance entries; doctests executed zero tests. Refresh Git and PR state before execution rather than assuming this checkpoint is current.

Already complete: fresh admission, separate long `--resume <canonical lowercase RFC4122 UUIDv4>` admission, byte-zero recognized history import, observed activity, context, independent estimated cost, partial tokens, bounded exit, setup/doctor, dashboard, same-ID compaction, application queue diagnostics, macOS/Linux baseline and capacity. Do not restart those units without a relevant change or concrete gap.

## Global constraints

- One synchronous server owner, one active dashboard, 50 sessions; live state does not survive server death.
- Preserve one-second callback deadlines and one absolute two-second native-completion budget, including receipt recovery and cancellation.
- Exactly Claude 2.1.267 is certified. Add another exact version only after Task 2; never infer a supported version range.
- One supervisor lease and one collector per invocation. Native children may receive private callback credentials, never the runtime supervisor lease.
- Activity, process phase, health, context, usage, and cost remain independent.
- Stop, silence, EOF, and process exit never imply Confirmed completion or Complete accounting.
- No source-directory scans. Every collected source requires exact attributable identity.
- Keep 65,536 identities, 16 MiB charged index bytes, 256 KiB reads, 32 MiB raw-record ceiling, and one outstanding collector exchange with 65,536-byte payload plus framing.
- Equal duplicate usage deduplicates; uncertified differing records freeze before application. Cache tokens are input subsets; unknown is not zero.
- Do not bypass decreasing-total protection with a fake generation or a null-then-lower sample.
- Use isolated configs, sockets, provider settings/trust and workspaces. Retain no credentials. Run native input directly in the runtime PTY.
- Capture native/collector/supervisor PID, PGID, and foreground TPGID before exit. Verify recorded groups absent and fixture paths removed; never query a closed GUI through CUA.
- Shell commands use `rtk`; GitHub operations use `git`/`gh`. Use `CARGO_INCREMENTAL=0` while local disk remains constrained. Preserve every failed attempt.

## Execution order and deliverables

| Order | Task | User-visible result | Dependency |
| --- | --- | --- | --- |
| 1 | Process ownership proofs | Confidence that crashes cannot retain reporting ownership | Existing code only |
| 2 | Exact-version compatibility | Reporting on a separately verified newer executable | Native evidence |
| 3 | Initial continue and alternate resume | More native launch forms retain reporting | Per-form intent certification |
| 4 | In-process transitions | Reporting follows clear/resume/foreground branch safely | Independent transition intent |
| 5 | Complete accounting | Root, child and auxiliary usage reconciled without duplication | Category and correction contracts |
| 6 | Final accounting | Trustworthy final Complete totals when available within deadline | Task 5 and final-source boundary |
| 7 | Confirmed activity | A trustworthy finished-turn quality indicator | Post-decision root-turn source |
| 8 | Remaining native and memory proof | Evidence for rare cases and honest resource guarantees | Relevant final implementation |
| 9 | Final assembly and delivery | Reviewed, tested support contract | Every enabled capability |

Tasks 1 and the read-only source investigations for 2–7 can be assigned independently. Mutations to admission, collector, or protocol run sequentially. Tasks 5 and 7 do not depend on broader transitions; do not hold useful progress behind unrelated provider gaps. Task 6 depends on 5. Run native cases only after the corresponding committed implementation has independent approval.

For each research task, inspect retained evidence and exact-version help first, refresh official documentation, then run one targeted native matrix against a named candidate. Permit one additional capture only when it resolves a specific ambiguity in that matrix. If the required primitive is still absent, write the missing guarantee and evidence to the capability table and stop that capability's implementation. More runs require a new concrete hypothesis. Never mark the full milestone complete merely because this investigation budget is exhausted.

## Task 1: Prove supervisor crash and exec descriptor boundaries

**Files:** `tests/server_lifecycle.rs`; `crates/ovrcr-runtime/src/agent_runner.rs`; `src/report.rs`; `src/report/admission.rs`; runtime session/reporting code only if a failing assertion demonstrates a defect. Evidence: `research/claude-reporting-acceptance/part3-lifecycle/`.

**Existing entry points:** `agent_run_native_helper`, `agent_run_runtime_kill_reaches_owned_native_group`, `agent_admission_private_claude_route_binds_once_and_clear_retains_lease`, `run_native`, `receiver`. Reuse `ControlFixture`, its real PTYs and existing process-group cleanup helpers.

- [ ] Add proposed regression `agent_run_supervisor_sigkill_releases_reporting_watch`. Start an outer fixture shell that survives its supervised child; wait for actual root binding and helper readiness before recording supervisor/native/collector ownership.
- [ ] Send SIGKILL to the recorded supervisor PID only, not the process group and not a simulated socket close. Keep the native fixture alive long enough to prove it did not inherit ownership of the runtime watch. Inspect native fate separately; do not invent a new crash-survival policy.
- [ ] Assert this real event sequence through the server, then explicitly clean surviving fixture-owned groups:

```text
root bound -> supervisor PID SIGKILL -> watched ownership lost
health reason = supervisor_disconnected; retained coverage != Complete
new invocation can reserve without the killed supervisor releasing explicitly
late old report/release cannot change the replacement binding
all recorded fixture-owned processes/groups absent after cleanup
```

- [ ] Add proposed regression `agent_run_native_exec_does_not_inherit_supervisor_lease`. Have the native helper inspect its own open descriptors after exec using fcntl/fstat/getsockname/getpeername (no dependency on /proc on macOS). Compare against test-owned listener/watch socket identities, not just descriptor numbers; report only safe type/identity metadata.
- [ ] Assert stdin/stdout/stderr still work, the expected private callback works, and neither the runtime lease connection nor private listener descriptor survives native exec. Environment callback credentials are expected and must not be mistaken for the runtime lease.
- [ ] Run each new exact test before editing production code. Existing correct behavior can pass a new proof test; do not manufacture a failure. Fix only demonstrated close-on-exec/ownership faults, rerun the affected tests on macOS and Linux, commit and obtain independent review.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle agent_run_supervisor_sigkill_releases_reporting_watch -- --exact --nocapture
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle agent_run_native_exec_does_not_inherit_supervisor_lease -- --exact --nocapture
```

**Exit:** Real process-death and exec-boundary assertions pass; no claim that a socket-disconnect test alone covers SIGKILL.

## Task 2: Certify and enable the next exact Claude version

**Files:** `src/report/admission.rs::pinned_version`; `src/cli/agent_setup.rs`; `tests/agent_setup.rs`; `tests/cli.rs`; fixture manifest; support/setup docs. Evidence: `research/claude-reporting-acceptance/part3-version/`.

- [ ] Resolve the default executable and its actual version without changing it. 2.1.268 is the previously observed candidate, not an assumed current value. Keep retained 2.1.267 available through a fixture-local symlink named `claude`.
- [ ] Record exact-version help and official source changes. Compare hook identity, root/child distinction, statusline fields, transcript identity, launch grammar and error behavior with the 2.1.267 contracts.
- [ ] On the candidate, capture fresh launch, explicit UUID resume/history, two turns, permission wait/allow, Stop continuation, source recovery, same-ID compaction, child isolation and normal exit. Reuse evidence machinery; do not rerun the old version's paid baseline solely for comparison.
- [ ] Before enabling the candidate, add tests that supported-version decisions and doctor diagnostics agree, while adjacent unknown versions remain untracked. Preserve native argv and executable availability when reporting is unsupported.
- [ ] Change only the verified exact-version decision and actual schema differences. Run admission/setup/CLI tests, native acceptance and independent review. Preserve 2.1.267 regression coverage.
- [ ] Investigate the retained intermittent version-probe failures separately if they recur: retain exact timeout/read/exit observations, establish the failure ordering, and add a deterministic regression before a fix. Do not increase deadlines or add blind retries to obtain green results.

**Exit:** A named exact candidate is supported with its own evidence, or remains unsupported with a precise incompatibility. Version certification is independent of Complete/Confirmed support.

## Task 3: Support additional initial launch forms

**Files:** `src/report/admission.rs::{eligible_launch,receiver}`; `src/cli/agent_setup.rs`; `tests/server_lifecycle.rs`; `tests/cli.rs`; setup/support docs. Source contract: add per-form rows to `provider-capabilities.md`.

- [ ] Investigate `--continue`, short `-r UUID`, equals syntax, and native picker/name/search independently. For each, require an exact intended-conversation discriminator established before admission; no newest-file scan and no first-callback-wins selection.
- [ ] Exercise two known fixture conversations, an absent selection, cancelled picker, and a background conversation. Retain exact argv and source identity. A form that cannot supply independent intent remains disabled.
- [ ] For each certified form, add a separate grammar unit test and actual callback/runtime lifecycle test. Preserve the exact native argv; do not normalize it into an injected `--session-id`.

```text
certified selected A + root startup/resume for A -> one binding
selection cancelled/missing/ambiguous -> no binding; native behavior preserved
foreign/child callback before A -> never bind foreign/child
contradictory root startup -> permanently closed, even after later matching event
lost Bind reply -> original operation receipt, no duplicate allocation
restored usage -> Partial Conversation including pre-invocation recognized history
```

- [ ] Extend doctor/setup eligibility and per-form native tests in the same unit. Keep plain positional prompts supported for fresh launches; never accidentally extend them to uncertified resume grammar.
- [ ] Review and deliver each supported form independently. Do not add an OVRCR conversation picker as an undocumented workaround. If native intent is unavailable, propose that user-facing design separately for approval.

**Exit:** Every added form has source evidence and real-path tests; disabled forms retain explicit reasons.

## Task 4: Follow certified in-process conversation transitions

**Files:** `src/report/admission.rs::Receiver`; `src/report/claude.rs::parse_claude_hook`; `src/report.rs`; `tests/server_lifecycle.rs`; setup/support docs. Inspect `crates/ovrcr-protocol/src/agent.rs::AgentCommand::Bind` and runtime conditional mutation checks before proposing any protocol change.

- [ ] Capture clear, in-process resume, foreground branch and background fork separately. Establish a trustworthy foreground-intent event/token and ordering before the corresponding SessionStart. Terminal text interception, a slash command string, and SessionStart alone do not meet this contract.
- [ ] Define the admitted transition's source, old/new identity, correlation, cancellation and history scope in a reviewed contract. If the provider cannot establish intent, leave the fallback unchanged; an OVRCR-owned transition action is a separate design decision.
- [ ] Add the following real socket/PTY trace before implementation:

```text
A/g1 -> certified foreground B/g2 -> certified foreground A/g3
late A/g1 Stop, usage, health, context, release -> no A/g3 mutation
background fork -> no foreground replacement
same-ID compact -> same generation, unknown context clears old occupancy
lost Bind response -> recover original receipt before publishing new state
ambiguous transition or recovery failure -> unavailable; native remains usable
```

- [ ] Reuse conditional `Bind { expected_binding, conversation }` and runtime-issued generations. Stop old collection and reject queued old-generation results. Replace prompt, source, cost watermark and component state only after acknowledged binding; do not accept callbacks during uncertain replacement.
- [ ] Preserve the one-second callback budget, including old-helper cancellation and receipt recovery. If safe replacement cannot finish, take the conservative fallback rather than blocking native work.
- [ ] Define fork prefix accounting from provider evidence before enabling collection for it. Update doctor/docs and verify actual GUI labels and native conversation identity for each added transition, then obtain independent review.

**Exit:** No child/background activity can steal the foreground binding; old callbacks cannot mutate a later visit to the same conversation.

## Task 5: Account for every certified usage category and correction

**Files:** `src/report/claude_metrics.rs::ClaudeUsageAccumulator`; `src/report/collector.rs::{CollectorSource,CollectorSnapshot,TranscriptReader}`; `src/report/admission.rs`; `tests/claude_reporting.rs`; `tests/server_lifecycle.rs`. Protocol/runtime files are conditional on a necessary, separately reviewed correction contract.

- [ ] Build a source inventory for root model requests, child requests, provider auxiliary requests, retries/failures, compaction, resumed history and fork prefixes. For every category record exact attribution, identity, inclusion in aggregates, ordering, and whether its final value is observable.
- [ ] Prefer a provider-authoritative aggregate if it actually covers the declared categories and conversation scope. Otherwise require explicit exact child-source references and root association. Keep one owned collector; do not discover child files by directory scans or spawn an unbounded collector per child.
- [ ] Capture equal duplicates and genuinely differing records, including a downward correction if the provider produces one. Synthetic differing records can test arithmetic but cannot certify native replacement semantics. A category without a source keeps total coverage Partial.
- [ ] Produce a numeric reference table from the captured authoritative source. Run these assertions against both arithmetic and the real collector-to-runtime path:

```text
same identity 10, replay 10 -> 10
certified ordered correction of that identity to 12 -> 12, not 22
distinct identity 12 -> 24; replay -> 24
root aggregate already includes child -> child is not added twice
uncertified differing duplicate -> freeze before application
missing category, ambiguous ancestry or source loss -> Partial/unavailable
cache-read/cache-write are subsets of canonical input, not extra totals
```

- [ ] The existing `apply_record` permits replacement arithmetic, while production `apply_unique_record` freezes conflicts. Do not switch production callers merely because arithmetic tests pass. Enable replacement only behind the reviewed schema/order contract.
- [ ] If certified downward corrections are necessary, write and review a concrete protocol amendment before coding it: binding identity, source identity, strictly ordered correction revision, replay rejection, value validation, and acknowledgement semantics. Update codec version and all callers together. Otherwise retain the current decrease rejection.
- [ ] Bound source count, pending bytes and index memory under existing limits before supporting multiple sources. Reject excess work with explicit incomplete accounting; never silently evict old identities or omit a category while claiming Complete.
- [ ] Compare native/reference and OVRCR totals for fresh, resumed and each enabled fork scope. Verify cost independently; equal rounded costs cannot prove token coverage or billing accuracy.

**Exit:** Every included category has attributable, nonduplicated measurements. Complete coverage remains gated on Task 6; an unobservable category is a documented provider dependency.

## Task 6: Publish a trustworthy final accounting snapshot

**Files:** `src/report/admission.rs::{poll,native_completed}`; `src/report/collector.rs`; `crates/ovrcr-runtime/src/agent_runner.rs`; existing metrics/finalization lifecycle tests.

- [ ] Find a provider source-completion primitive covering all Task 5 categories, exact conversation/revision, delayed writes, child work and failure paths. Capture its ordering relative to writes and native exit. EOF, a post-exit scan and idle time are explicitly insufficient.
- [ ] Extend collector snapshots with the proven source-finality metadata only after reviewing the concrete schema. Existing `caught_up` means scan progress; it must not be renamed or treated as finality.
- [ ] Add the acceptance truth table through the real helper/socket/native completion path:

```text
all categories + matching final boundary + acknowledged publish within budget -> Complete
all categories + no final boundary -> Partial
final boundary + missing category -> Partial
boundary after absolute completion deadline -> Partial; no extra launcher wait
old source/generation boundary -> ignored
contradictory export/source replacement -> unavailable, never Complete
lost Finalize response -> original receipt recovery within original deadline
```

- [ ] Exercise pending pre-exit response, delayed final write, helper failure, failed publication, abrupt native exit and cancellation. Keep existing post-exit-drain regressions and eventual helper reaping.
- [ ] Run a targeted native comparison and exact-commit review before any production route can emit Complete.

**Exit:** Complete has both category coverage and a final-source boundary. If the provider supplies neither in time, honest Partial is the required outcome and this feature remains blocked.

## Task 7: Report confirmed finished turns

**Files:** `src/report/claude.rs::{ClaudeEventKind,parse_claude_hook}`; `src/report/admission.rs::Receiver::handle`; `tests/server_lifecycle.rs`; Claude fixtures; existing TUI inspection tests if displayed quality needs changes.

- [ ] Evaluate existing candidates against retained captures first: delayed idle notification and transcript turn-duration records are hypotheses, not guarantees. Require a matching root conversation/turn event causally after all continuation decisions.
- [ ] Native matrix: ordinary response, blocking Stop continuation, cancellation, permission wait/deny, child completion/failure, tool continuation, and a new turn racing an older completion. Capture event order and current UI state. Do not infer causality from timestamps alone.
- [ ] After source approval, add exact parsing plus these real callback/runtime assertions:

```text
Stop(A) -> Idle/Observed
Stop(A) -> continued work(A) -> Busy; no intervening Confirmed
certified settled(A) while A is current -> Idle/Confirmed
start B -> delayed settled(A) -> no state/revision/freshness change
child/foreign/malformed settled -> no root mutation
silence, reporter loss, EOF, exit -> no synthesized Confirmed
```

- [ ] Reuse existing `ActivitySample` and `SampleQuality` if sufficient. Preserve root/child validation and current-turn checks. Transport arrival order alone cannot create a provider source revision.
- [ ] Verify quality in JSON inspection, sidebar and selected header using actual GUI/AX, then independently review. This task adds reliable reporting quality; new notifications or dependent-task automation are separate product work.

**Exit:** A certified finished-turn observation exists, or Confirmed remains disabled with the missing post-decision guarantee recorded.

## Task 8: Close remaining native and resource evidence gaps

**Files:** `tests/claude_reporting.rs`; `tests/claude_reporting_load.rs`; relevant runtime tests; `research/claude-reporting-acceptance/part3-native/` and `part3-memory/`; requirement matrix. No new production queue/diagnostic surface unless a measured defect requires it.

- [ ] Target native null-current-context timing with the exact-version startup/compaction cases where documentation or captures suggest null. Never rewrite native payloads to manufacture evidence. Preserve the existing real callback/runtime null regression when native still emits zero.
- [ ] Exercise a task-owned child API failure using an isolated native configuration that produces an actual API error. Verify child StopFailure attribution and unchanged root binding/activity/metrics; do not generalize a tool error into an API failure or deliberately trigger account-wide rate limits.
- [ ] Inventory memory retained by index keys/values and BTree nodes, Vec capacities, 32 MiB incomplete records, JSON parsing allocations, in-flight IPC, supervisor/runtime/dashboard queues, and kernel sockets. Name owner, multiplicity and bound for every term; retain allocator/runtime baseline separately.
- [ ] Create deterministic high-water fixtures that hold all 50 collectors at the raw-record boundary and fill indexes/queues with barriers. Include deeply nested/many-small-field JSON within the byte limit to measure parsing expansion, not merely input bytes. Preflight isolated-runner memory/disk; do not stress a busy user machine.
- [ ] Retain peak allocated memory where platform instrumentation supports it, OS process high-water measurements, and actual socket buffer limits alongside sampled RSS. Sampling faster is not proof of an instantaneous maximum. Linux and macOS need separate evidence.
- [ ] Compare the measurements to the existing 3 GiB sampled-growth acceptance separately. A universal total-memory maximum cannot be inferred from those results: full closure needs a derived bound for every allocation/buffer or an explicit user-approved platform envelope/contract revision. If a term remains unbounded/unmeasurable, keep the corresponding requirement open.
- [ ] Re-run capacity only for affected collector/runtime changes or the new targeted high-water cases; preserve prior load evidence and limits.
- [ ] Retry a bounded read-only Docker check. If responsive, verify ownership and remove only container `ovrcr-claude-linux-reap-01a08c3f` and image `ovrcr-claude-linux-cache:01a08c3f`. If still hung, retain cleanup notes. No Docker Desktop restart without explicit authorization.

**Exit:** Each rare native case is observed or explicitly unverified; measured resource guarantees and exclusions are distinct. Cleanup is closed only when actually verified.

## Task 9: Final acceptance, review and delivery

**Files:** support/setup docs, provider-capabilities and remaining-matrix, this plan, retained acceptance evidence, existing work diary. Update the existing PR rather than opening duplicate integration work.

- [ ] For each enabled capability, map source contract -> production caller -> actual assertion -> exact native revision -> platform result. An unchecked provider dependency prevents full completion even if all implemented tests pass.
- [ ] Run focused tests during each unit. At final product revision run the workspace gates below once, plus Linux headless and the existing capacity job when relevant runtime/collector code changed. Confirm every targeted filter executed its test.

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test --workspace --all-targets --all-features
rtk proxy env CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk proxy cargo fmt --all -- --check
rtk proxy env CARGO_INCREMENTAL=0 cargo test --workspace --doc
rtk proxy git diff --check
```

- [ ] Build the actual GUI from an independently reviewed committed revision after recording source and tracked state. Verify each newly enabled capability through native input, numeric inspection, screenshot/AX and pre-exit ownership. Repeat unchanged baseline cases only where shared behavior changed.
- [ ] Resolve blocking findings with the original worker and independent reviewer; retain RED/GREEN evidence and unestablished failures. Stage intended files only.
- [ ] Update setup/doctor version/forms and labels in the same unit as behavior changes. Preserve dated historical findings; do not interpret stale unchecked boxes as missing code.
- [ ] Push under existing authorization and require all current-head CI jobs to pass. Keep PR 55 draft until every original requirement is satisfied or the user explicitly approves a narrower scope. Do not merge or release.
- [ ] Extend `~/Documents/Vaults/Knowledge/Work diary/Personal/2026-09-09 OVRCR Grok agent reporting.md`; retain blocked status while any full-milestone condition is unresolved. Remove only disposable resources with established ownership.

## Completion and external dependency decision

The plan is fully implemented only when all intended transitions, accounting coverage/finality, confirmed activity, and required reliability/resource evidence pass their stated gates. Completing research with a negative finding is useful progress, but it is not the missing feature.

If a provider primitive is absent, the next decision is explicit: obtain a supported upstream source, investigate a separately certified newer version, or ask the user to approve a narrower reporting contract. An OVRCR-owned conversation control or different provider execution model requires its own agreed design. No heuristic fallback, execution-model replacement, or release-scope reduction is implicit in this plan.
