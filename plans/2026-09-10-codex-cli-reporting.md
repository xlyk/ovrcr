# Codex CLI Reporting Implementation Plan

> **2026-09-10 scope follow-up:** Kyle approved a smaller [hooks-first response-ready milestone](2026-09-10-codex-response-ready-hooks.md). That plan replaces the metrics/continuous-source prerequisites only for response readiness. This document and its blocked source/accounting tasks remain historical and unfinished; no adapter is implemented by the follow-up plan.

> **For agentic workers:** Use `subagent-driven-development` or `executing-plans` task by task. One worker implements each unit; an independent reviewer checks its committed revision. One coordinator stages files. Checkboxes record completed acceptance, not effort spent.

**Goal:** Add useful live Codex status, context and conversation-token reporting to OVRCR while preserving the native CLI and its approval handling.

**Architecture:** Extend the existing supervised invocation and runtime-owned reporting model. Choose one proven exact-thread observation route before writing the Codex adapter: passive native app-server observation if safe, otherwise an exact-path, versioned rollout reader. Keep Codex interpretation in the root application crate, not the runtime or dashboard.

**Tech Stack:** Existing synchronous Rust workspace, serde/serde_json, Unix sockets, PTYs, bounded collector helper; native Codex hooks where certified. No new service, database, Tokio, agent SDK, or replacement TUI.

**Spec:** [Shared reporting design](2026-09-09-multi-harness-reporting-design.md), [umbrella Task 5](2026-09-09-multi-harness-reporting.md#task-5-complete-codex-cli-integration), and [merged support contract](../docs/agent-reporting-support.md). This standalone plan replaces the umbrella's obsolete shared-code scaffolding steps for Codex. Its first-release boundary below is a proposed delivery scope; it does not declare every umbrella requirement complete.

## Baseline and authority

- Worktree: `/Users/xlyk/Code/ovrcr/.worktrees/codex-reporting`; branch `codex/codex-reporting`.
- Base: merged `main`, `983dd42a05690b15db073e6c1d8454d5a889db63` (Claude PR #55). Root checkout has unrelated untracked files; leave it alone.
- Planning inspection on 2026-09-10: installed `codex --version` reports `codex-cli 0.153.0`. Help lists native hooks/trust-related options, app-server, resume/fork and paginated-history migration tooling. This establishes candidate version and CLI grammar only, not observer safety or a stable rollout schema. PATH-alias creation produced a sandbox warning while version/help exited zero.
- Planning created this document only. No Codex adapter, live authentication, native test, or new test result is claimed. The prior Claude suite is baseline evidence, not Codex acceptance.
- Before implementation read `AGENTS.md`, `docs/development/{architecture,runtime,testing,tui,delivery}.md` and `docs/testing-computer-use.md`. Refresh current `main` and worktree status.

## First-release boundary and finite stopping rule

| Required deliverable | Quality/limit |
| --- | --- |
| Exact root admission for a certified native interactive launch | Native remains usable when unsupported; no first-callback-wins or newest-file inference |
| Busy, waiting-for-input, idle and error where native events establish them | Observed by default; unknown is explicit when a state lacks evidence |
| Context occupancy and token totals for the admitted conversation | Preserve unknowns, source/scope and Partial coverage; history import only from the exact source |
| Setup/doctor, CLI inspection and existing dashboard labels | Print configuration; preserve native trust and approval settings |
| Native/macOS and Linux lifecycle acceptance | Real input/output, numeric reference and recorded process cleanup |

Initial explicit resume is enabled only if the same bounded source investigation certifies its selected identity. Picker/last-session forms, forks, in-process rebinding, Complete final accounting, Confirmed completion, notifications, pricing tables and universal memory guarantees are outside this first release. Native commands retain their normal behavior when reporting for a form is disabled. Record the deferred features explicitly; do not silently claim full umbrella completion.

Task 1 has one source-comparison pass and one isolated native matrix, with one repeat allowed to resolve a specific observation failure. If neither route provides safe root identity, stop adapter implementation and report that concrete dependency. If identity/status work but a metric source is unavailable, report that capability gap and obtain an explicit smaller acceptance scope before calling this first release complete. Do not investigate successive versions or repeat the same matrix indefinitely. Subsequent tasks implement the selected contract; no additional general research round is scheduled.

## Global constraints

- One synchronous server owner, one active dashboard, 50 sessions. Live state does not survive server death.
- Reuse one supervisor lease and at most one collector per invocation. Native children never receive the supervisor lease or outer reporting capability.
- Preserve a one-second reporter budget and one absolute two-second native-completion budget, including receipt recovery and cancellation. The private receiver currently supplies an 800 ms request deadline; honor its remaining time.
- Preserve exact native argv/stdio/exit/signals/foreground ownership. Do not use `thread/resume` to attach a telemetry observer, answer approvals, inject approval/sandbox bypasses, or change the user's live Codex configuration.
- Source identity must precede collection. No directory scans, latest-file selection, TUI scraping, or independent app-server process assumed to own an existing CLI thread.
- Preserve 65,536-byte private-envelope limit, one outstanding helper exchange, 256 KiB reads, 32 MiB raw records, 65,536 identities and 16 MiB charged index storage. These are component bounds, not a universal process-memory guarantee.
- Keep activity, health, process state, context, usage and cost independent. Stop/EOF/silence/exit do not synthesize Confirmed or Complete. Unknown is not zero; replay never refreshes measurements.
- No downward-total workaround using fake generations or null-then-lower samples. Uncertified conflicting records freeze before application.
- Tests use isolated config/socket/workspace/provider home. Use `rtk` commands, `git`/`gh` for GitHub and `CARGO_INCREMENTAL=0` while disk is constrained. No local 50-way stress on the busy host.
- The separate Pi plan starts at the same base. Either provider may land first. Implement shared-file changes sequentially; refresh/rebase the second worktree and reuse already-landed provider helpers. Do not copy a second transport or overwrite the other provider's dispatch branch.

## Current code map and proposed interfaces

| File | Current reality / planned responsibility |
| --- | --- |
| `crates/ovrcr-protocol/src/agent.rs` | Already defines Codex, bindings, revisions, health, scoped metrics and qualities. No new provider enum or wire protocol is needed. |
| `crates/ovrcr-runtime/src/agent_runner.rs` | Existing native process/PTY and opaque HookEvent lifecycle. Keep provider parsing out. |
| `src/report.rs` | InvocationLease, receipts/deadlines, private transport. Reservation and raw payload helper currently hard-code Claude. |
| `src/cli/{args,agent,report,agent_setup}.rs` | Run/setup/doctor currently accept only Claude and dispatch directly to it. Add explicit provider dispatch; preserve legacy commands. |
| `src/report/admission.rs` | Claude-specific grammar/receiver. Keep its supported 2.1.267/268 behavior intact. |
| New `src/report/codex.rs` | Codex event normalization and receiver returned as existing HookHandler. |
| New `src/report/codex_metrics.rs` | Codex context/cumulative or identified-record normalization. |
| `src/report/collector.rs` | Currently a Claude reader, not a generic parser. Add one explicit Codex source/parser variant only if rollout route is selected; preserve old source construction/default behavior. |
| New `src/cli/codex_setup.rs` | Codex setup composition and diagnostics; wire module in `src/cli/mod.rs`. |
| `tests/server_lifecycle.rs` | Reuse private ControlFixture and actual managed launcher path; do not pretend a nonexistent `tests/agent_reporting.rs` fixture already exists. |
| New `tests/codex_reporting.rs`; `tests/{cli,agent_setup}.rs` | Numeric/source tests and public command/setup acceptance. |
| `crates/ovrcr-tui/src/dashboard/{agents,render,tests}.rs`; `src/cli/resources.rs` | Detection and generic rendering already exist. Change only demonstrated missing Codex paths/labels. |
| New `docs/codex-reporting-setup.md`; existing reporting support/CLI docs | Supported versions/forms, installation/removal, visible quality and unsupported cases. |

Use this additive shared seam (reuse it if Pi lands first):

```rust
// src/report.rs: existing compatibility entry point retains Claude behavior.
pub fn reserve_invocation() -> anyhow::Result<Option<InvocationLease>> {
    reserve_invocation_for(ovrcr_protocol::AgentProvider::Claude)
}
pub fn reserve_invocation_for(
    provider: ovrcr_protocol::AgentProvider,
) -> anyhow::Result<Option<InvocationLease>>;

pub fn send_native_payload(
    provider: ovrcr_protocol::AgentProvider,
    origin: &str,
    input: &[u8],
    deadline: std::time::Instant,
) -> anyhow::Result<()>;

// New src/report/codex.rs; same lifecycle seam as Claude.
pub fn receiver(
    lease: Option<super::InvocationLease>,
    argv: &mut Vec<std::ffi::OsString>,
) -> ovrcr_runtime::agent_runner::HookHandler;
```

These are proposed signatures, not existing APIs. `send_native_payload` reuses the existing bounded RawValue envelope/framing/token path with an exhaustive provider-to-wire-name match. Claude wrappers retain exact origins. No plugin registry/trait framework or public exposure of lease secrets. `agent doctor` must default its executable from the selected provider, not retain the current hard-coded `claude` default. Existing direct callers of reserve_invocation stay compatible.

## Task 1: Select and certify one exact-thread source contract

**Files:** create `research/codex-reporting-acceptance/source-contract.md` and versioned allowlisted fixtures under `tests/fixtures/agent-reporting/codex/`; inspect installed CLI/schema and the source references in the shared design. Do not edit runtime behavior yet.

**Interface produced:** A table mapping each native event to root identity, turn identity, source path/endpoint, ordering, usage scope/category and version. The selected route and supported launch grammar are explicit; unsupported rows have a named missing primitive.

- [x] Record version/help and generate candidate schema in a task-owned temporary directory:

```sh
rtk proxy codex --version
rtk proxy codex --help
rtk proxy codex app-server --help
rtk proxy codex app-server generate-json-schema --experimental --out "$codex_schema_dir"
```

Define `codex_schema_dir` with `mktemp -d`; never repurpose CODEX_HOME or HOME as a scratch variable. Schema generation is read-only with respect to user sessions. Refresh official hook/app-server documentation using the installed version as the candidate, and retain provenance. Schema shape alone is not a source guarantee.

- [ ] Test passive observation of a task-owned native CLI conversation. Required proof: observer receives only that exact thread's status/token stream, can disconnect without ending it, does not resume/start a second thread, change configuration, or become an approval responder. An unanswered native permission selector must remain unanswered until real CUA input. Do not use a mutating attachment as a workaround.
- [ ] If passive observation fails, inspect a versioned exact-thread rollout source. Establish the exact path through a trusted native root/start metadata route, verify header identity, and account for the candidate's paginated-history behavior. If no exact supported file exists, this fallback is unavailable; do not scan legacy directories.
- [ ] Capture fresh root startup, two distinct turns, approval wait/allow, cancellation, root failure, child activity, and one explicitly selected resume if its grammar can be certified. Record event order, exact source, version, root/child distinction and actual native output. Bound the matrix as above; no credentials or private prompt bodies in retained source fixtures.
- [ ] Check source invariants using literal reference traces:

```text
Selected thread A; foreign/child B startup first => never bind B.
Bind A; child completion B => no root idle/revision/freshness mutation.
Cumulative tokens 100, 100, 140 => 140; replay stays 140.
Two distinct request identities each using20 =>40.
Input100 including cached40; output20 including reasoning5 => total120.
A current; delayed prior-turn completion => no new-turn idle.
```

- [ ] Write the supported event/field definitions before implementing parsing; mark inaccessible cost or unavailable components unknown. Record a review decision for the source contract. Gate outcome is either one selected safe route or a concrete blocker, never an assumed fallback.

**Exit:** Safe exact root binding plus named status/context/token sources are established, or dependent work stops. A source-negative result does not count as an implemented adapter.

### Task 1 blocked checkpoint, 2026-09-11 UTC

Version/help/schema evidence is recorded in [the source contract](../research/codex-reporting-acceptance/source-contract.md). Independent review rejected the initial zero-native-attempt stopping point. The corrected [isolated native attempt](../research/codex-reporting-acceptance/native-attempt/results.md) reached the authentication selector without selecting login. Its one repeat addressed sandboxed process-inspection failure. No native conversation, hook binding, transcript header, metric reference or GUI/AX acceptance is claimed. Repeat process-group cleanup passed; the first attempt's unpersisted PID/group cleanup remains unverified and its task directory is retained.

No safe observation route is certified; root-only callback routing and current paginated lineage remain open. The native matrix now has a concrete authentication prerequisite, so dependent implementation stops. All other Task 1 acceptance checkboxes remain open. Source-only comparison and PTY onboarding are not adapter delivery.

Authenticated continuation at `084698e`: user authorized existing credentials. Isolated CUA hook trust and a root response succeeded; matching native parent/header and first-turn token observations are retained in [the authenticated checkpoint](../research/codex-reporting-acceptance/authenticated-attempt/checkpoint.md). ENOSPC stopped the incomplete matrix. No further Task 1 checkbox is closed; receiver-side OS peer/native-parent identity and remaining source invariants still need proof. No adapter work follows this partial evidence.

## Task 2: Wire Codex launch, transport and root admission

**Files:** `src/report.rs`, new `src/report/codex.rs`, `src/cli/{args,agent,report,mod}.rs`, `tests/{server_lifecycle,cli}.rs`. Runtime changes only if the source contract demonstrates missing provider-independent process metadata.

**Consumes:** Task 1 source contract and existing InvocationLease/HookHandler. **Produces:** `ovrcr agent run --provider codex -- codex ...` and `ovrcr report codex --stdin-json`, one runtime-issued binding on the intended root.

- [ ] Add a real managed-PTY test `codex_admission_preserves_native_argv_and_root_ownership` in server_lifecycle, using its ControlFixture and a task-owned native executable fixture. Add CLI assertions that the new command parses while existing Claude grammar remains unchanged. Run the new exact test and retain its behavioral RED.
- [ ] Implement the shared signatures above; pass the selected provider through reservation and CLI dispatch. Add Codex-only eligibility/version checks from Task 1. Keep original native argv on unsupported forms. Do not inject Claude's UUID/--session-id strategy into Codex without native evidence.
- [ ] Implement Waiting -> Bound -> Closed admission with conditional runtime Bind/receipt recovery. Children and foreign callbacks cannot bind or replace the root. A contradictory root startup closes admission permanently; a later matching event cannot reopen it. Source path/observer target must match the admitted identity.
- [ ] Exercise these actual socket/PTY sequences before review:

```text
Reserve Codex -> matching root Bind -> one Codex generation.
Foreign startup -> ignored -> intended root startup -> bind only intended root.
Contradictory certified root startup -> closed -> later intended root -> remains closed.
Bind response lost -> query original receipt -> one generation, not a second Bind.
Old invocation report/release -> replacement invocation remains unchanged.
Supervisor PID SIGKILL -> supervisor_disconnected; replacement can reserve.
Native exec -> private callbacks work; supervisor lease/listener not inherited.
Unsupported version/argv/source -> original native output and exit preserved.
```

- [ ] On in-process identity changes, retain native usability and mark reporting unavailable for the previous binding; do not rebind in this first release. Same-ID compaction is not a new generation. Hook loss/source loss affects health independently of native process state.
- [ ] Run exact new lifecycle tests plus the affected existing Claude admission/lease tests; inspect executed counts. Commit only this unit and obtain independent review before native acceptance.

## Task 3: Publish live activity and bounded metrics

**Files:** `src/report/codex.rs`, new `src/report/codex_metrics.rs`, conditional `src/report/collector.rs`, new `tests/codex_reporting.rs`, `tests/server_lifecycle.rs`.

**Consumes:** Admitted binding and selected source. **Produces:** existing ActivitySample, MetricsSample and HealthSample with separate monotonically increasing component revisions; no protocol change.

- [ ] Add numeric/parser unit REDs and collector-to-runtime integration REDs from the literal traces below. Synthetic fixtures test normalization; Task 1 native records establish semantics.

```text
last-request/context100 -> cumulative total1000 => context100, not1000.
Cumulative100 -> replay100 ->140 =>140, without replay freshness refresh.
Unknown context after compaction => clear prior occupancy; keep retained usage.
Missing cost => null cost; no local price inference.
Missing cache/reasoning breakdown => unknown subset; do not invent zero.
Equal identified duplicate => deduplicate; uncertified differing duplicate => freeze.
Source disappears -> retained totals plus unavailable; exact source returns -> recover.
EOF/helper exit/native exit without final primitive => Partial, never Complete.
```

- [ ] Map only the certified root turn events: new input/tool work Busy; correlated approval wait WaitingInput; ordinary root stop Idle/Observed; root failure Error/Observed. Ignore child and stale-turn events without revising freshness. A status emitted before continuation decisions cannot be Confirmed.
- [ ] For cumulative sources replace authoritative snapshots rather than summing notifications. Scope restored baselines to Conversation only when the source proves it. Cache/reasoning remain subsets. Conflicting/decreasing totals use the existing unavailable/freeze behavior, not a new generation or correction protocol.
- [ ] For a rollout route add one explicit source/parser variant to the existing helper with the same request/response bounds and owned cancellation/reaping. Do not feed Codex rows through ClaudeUsageAccumulator or create an unbounded parallel file watcher. Open the exact source from byte zero for recognized history, validate regular file/root identity and retain watermarks across temporary loss; reject replacement/truncation according to the certified contract.
- [ ] For a passive stream route preserve one bounded collector exchange; cap decoded source records and pending observations, coalesce only replaceable metric snapshots, and report an explicit gap on state-event overflow. Source callbacks cannot directly mutate runtime state or answer native requests. No new permanently running daemon.
- [ ] Add `codex_metrics_reach_runtime_without_replay_or_child_mutation` through ControlFixture. Force lost replies, stale generation, source loss and pending completion work with barriers, not sleeps. Confirm final publication and cancellation share the original two-second deadline.
- [ ] Run the new numeric suite and affected collector/admission lifecycle suites, then independently review the exact committed unit. If collector machinery changed, run the existing hosted capacity and memory jobs; those jobs are regressions, not Codex-specific capacity proof.

## Task 4: Setup, diagnostics and native acceptance

**Files:** new `src/cli/codex_setup.rs`, `src/cli/{mod,agent,args}.rs`, `tests/{agent_setup,cli,server_lifecycle}.rs`, new `docs/codex-reporting-setup.md`, existing reporting support/CLI docs; conditional dashboard/resources tests. Evidence: `research/codex-reporting-acceptance/`.

**Produces:** Printed setup and removal instructions, `agent doctor codex`, native end-to-end proof and a documented capability matrix.

- [ ] Implement setup printing for the exact reviewed hooks/source route, with an owned marker and no automatic edits. Preserve unrelated config and hook trust. Doctor reports provider executable/version, supported forms, transport/root/source capability, independent unknown metrics and remediation; never print credentials or start a server.
- [ ] Add CLI/setup tests for supported/unsupported versions, missing executable, managed/unmanaged launch, unrelated hooks, malformed configuration, and root bound/source unavailable states. Test selected provider determines the doctor executable default.
- [ ] Verify generic `session usage` and dashboard rendering use the new binding. Add only missing Codex expectations to the existing tests; preserve narrow-window/unknown/Partial/Observed labels. No notification/toast implementation is included.
- [ ] Build `just gui` from the independently reviewed committed product source with clean tracked status recorded before build. Use an isolated Codex configuration and provider-owned authentication only with applicable user authorization; do not reuse the prior Claude Keychain approval.
- [ ] Through actual CUA: fresh response marker, second turn, visible Busy/Waiting/Idle or documented unknown states, permission allow, cancellation/failure/recovery, metrics/reference comparison, same-ID compaction, exact resume only if enabled, detach/reattach, then normal exit. Run a child event case and source disconnect/recovery case. Preserve screenshots/AX and JSON snapshots; distinguish submitted text from actual assistant output.
- [ ] Capture supervisor/native/collector PID, PGID, foreground TPGID and TTY before native exit. Verify each recorded owned group absent and isolated directory/socket removed after launcher exit. Do not kill remembered/unrelated processes or query a closed GUI.
- [ ] Mark each enabled capability pass/fail/unverified against source -> caller -> assertion -> native source revision -> platform. Unsupported rows remain explicit; no repeated broad certification cycle.

## Task 5: Final validation and delivery

- [ ] Run the final reviewed product gates once, retaining exact source, command, exit, test counts and earlier failures:

```sh
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test codex_reporting
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle codex_
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test cli
rtk proxy env CARGO_INCREMENTAL=0 cargo test --workspace --all-targets --all-features
rtk proxy env CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk proxy cargo fmt --all -- --check
rtk proxy env CARGO_INCREMENTAL=0 cargo test --workspace --doc
rtk proxy git diff --check
```

These named new tests are created in Tasks 2–3; verify filters execute them. Zero doctests is reported as zero, not fabricated coverage. Do not run this suite merely to review the present plan.

- [ ] Obtain final independent review. Refresh/merge current main, resolve only actual conflicts, rerun affected checks if the tree changed, and preserve Pi/Claude paths if they landed meanwhile.
- [ ] With execution-time delivery authorization, push and open a separate Codex PR; require current-head macOS/Linux checks plus existing hosted capacity/memory jobs. If no provider-aware 50-session scenario was run, say so; generic capacity does not certify the Codex source at scale.
- [ ] Extend the existing work diary with supported scope, precise gaps and evidence. Keep only required evidence/recovery files; remove proven task-owned disposable resources. Merge/release need explicit user authorization for this PR.

**Completion:** The first-release capabilities above are implemented, independently reviewed and verified, or a named source dependency blocks completion. Deferred capabilities have separate entries and cannot silently expand this plan into repeated open-ended iterations.

### Authorized targeted invalidation follow-up

After the blocked checkpoint, the user authorized one narrow pinned-source investigation and one native reproduction. The [result](../research/codex-reporting-acceptance/history-invalidation.md) distinguishes paginated revert from native prompt backtracking: the latter forks and unsubscribes, with startup notification deferred until the next prompt. The reproduced interval lacks foreground invalidation. Task 1 remains blocked under the current exact-current-conversation contract; no broader source pass, scope reduction or dependent implementation is implied.

### Task 1 resumed source checkpoint, 2026-09-11 UTC

The interrupted bounded matrix now establishes the fresh-root 0.153.0 exact-file route on macOS: receiver OS peer/parent attribution, matching paginated header, two turns, approval observation/allow, child isolation, cancellation/error, and numeric native references. See the current source contract and versioned native fixtures. This supersedes earlier no-route checkpoints, while preserving their failures. Task 1 independent review remains pending; source-only evidence does not check off product implementation/acceptance. Generic WaitingInput and native remaining percentage are unavailable; absolute occupied tokens/capacity and Partial root-thread usage are supported source components. Resume, compaction recovery, Linux native, current-checkout GUI and capacity remain unrun/deferred. No runtime code changed.

### Task 1 independent-review correction, 2026-09-11 UTC

BLOCKED: the preceding resumed checkpoint establishes ordinary-turn observations, not a safe ongoing route. Review of 4a0d0eb found no proven detectable invalidation/current-lineage guarantee when the same native process performs an unsupported history operation. Fresh launch grammar does not prevent that operation, and unchanged path/header/inode cannot prove continued authority. Tasks 2–3 must not begin. Stop under the finite Task 1 rule; no further source pass or native matrix is implied. A later explicitly scoped continuation would need to prove a concrete fail-closed boundary. Hard-coded replay/turn examples are illustrative arithmetic only; actual replay/freshness behavior remains unverified. Cleanup evidence lists 17 PIDs and 16 groups.
