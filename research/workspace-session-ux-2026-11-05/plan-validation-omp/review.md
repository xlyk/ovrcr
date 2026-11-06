# OMP team validation of the agent-session creation plan

2026-11-05. **Initial review result: needs revision before implementation.** This is a source-based plan review, not runtime acceptance. The approved product design remains intact. The plan/spec and application source were not changed by the original review.

## Follow-up: local plan corrections, 2026-11-05

At Kyle's request to close the gaps, the primary agent revised only the [implementation plan](../../../docs/superpowers/plans/2026-11-05-agent-session-creation.md) and review/diary records. No additional external review, provider execution or application implementation was performed. The original team verdicts below remain historical evidence; this follow-up does not claim the team re-reviewed or approved the revision.

| Finding | Correction now explicit in plan | Closure level |
|---|---|---|
| F1 | §3 preserves parent/child state on accepted Ok, correlates the submitted project/ID/token, waits for confirmed completion and matching inventory/hierarchy, and tests both event orders and partial failure | Planning omission addressed; implementation/tests pending |
| F2 | §3 joins existing Inspect registry membership with live labels by identity, excludes retained-only rows, preserves unavailable registered rows and revalidates at Start | Planning omission addressed; implementation/tests pending |
| F3 | §3 separates new-form opening guards from selected-target Start guards and preserves the separate Terminal path; acceptance includes no-current/unavailable/root-drifted cases | Planning omission addressed; implementation/tests pending |
| F4 | §3 adds a multiline editor/input-dispatch task, with Enter-newline, verified submit chord, picker-only acceptance, paste/cursor behavior and no terminal leakage | Planning omission addressed; native encoding/tests pending |
| F5 | §1 defines exact-configuration capability evidence; §2 specifies a Dashboard-owned query, stable in-memory launch intent bound to session/run, bounded reconnect lookup and no uncertain replay | Planning omission addressed; native mechanism proof and implementation pending |
| F6 | §2 names protocol version/snapshot and settings parser/catalog/editor/write/matrix updates; §4 maps assertions and defines a distinct evidence tuple for all four harnesses plus prompt-marker privacy checks | Planning omission addressed; compatibility/runtime/provider acceptance pending |

Local verification: re-read the relevant inventory, lifecycle, keymap, editor/default and protocol/settings source paths; checked document links/whitespace and that each finding has a concrete task plus an acceptance row. The confirmed product design and its four-harness requirement are unchanged. Actual native APIs remain the first implementation gate, not facts supplied by a prose edit.

Reviewed [plan](../../../docs/superpowers/plans/2026-11-05-agent-session-creation.md) and [confirmed specification](../../../docs/superpowers/specs/2026-11-05-agent-session-creation-design.md), against source at `08d25641d3ef96a2b8f9f74a5710efb2c532b75a` plus the current uncommitted documentation.

## Team and evidence

Three independent Oh My Pi roles used **`ollama-cloud/deepseek-v4.1-flash`**, with high thinking. The installed harness reported v18.10.3. The user explicitly authorized sending the plan, specification and relevant repository excerpts to this destination before the completed review attempts.

| Role | Final result | Report |
|---|---|---|
| Source feasibility | NEEDS REVISION | [Full findings](feasibility-review.md) |
| UX and lifecycle consistency | NEEDS REVISION | [Full findings](ux-lifecycle-review.md) |
| Acceptance and delivery | NEEDS REVISION | [Full findings](acceptance-review.md) |

Initial source scans used only read/grep/glob with extensions, skills, rules, LSP, PTY and nested-agent tools disabled. They did not return substantive final reports within the intended eight-minute scan window; OMP termination/yield processing overran that window. One scan returned exit 1 with Deadline exceeded, one yielded without a report and later exited 0, and the lingering UX scan was explicitly interrupted. None is counted as a successful verdict.

Each role then received its own collected tool-result excerpts plus the plan/spec in a bounded finalization pass with **all tools disabled**. No private reasoning was copied into those bundles. All three finalization processes exited 0 and returned substantive reports. Event logs confirm the requested provider/model for every final assistant message and zero finalization tool calls. See [team-results.json](team-results.json), timestamped `*-receipt.json` files and matching JSONL streams. Initial cancelled/failed attempts are retained separately.

The primary agent independently checked the cited owning source and narrowed/deduplicated the findings below. Reviewer priorities are advisory; the consolidated priorities account for requirements already present in the plan and existing reusable mechanisms.

## Confirmed plan gaps

### F1 — P1: Preserve the dialogs across lifecycle acceptance and track the submitted identity

**Plan:** lines 34–36 assume the child can remain visible while creation runs and then use a returned workspace ID.

**Concrete mismatch:** `palette_response` sends `Response::Ok` into `accept_lifecycle_job` (`crates/ovrcr-tui/src/dashboard/palette.rs:3036–3044`); that path moves state into a provisional row and clears `self.palette` (`dashboard/lifecycle_ui.rs:245–262`). The requested child/parent drafts therefore need an explicit preserved state owner, rather than reusing this transition unchanged. `LifecycleOutcome::Succeeded` carries no created-workspace ID (`crates/ovrcr-protocol/src/wire.rs:502–507`; runtime `server/lifecycle.rs:167–176`).

**Smallest correction:** name the palette/lifecycle transition that keeps the parent and child draft through acceptance, failure and completion. Retain the project/workspace ID already allocated and submitted by the Dashboard (`palette.rs:2493–2515`), correlate the completion token, and select that exact registered identity only after confirmed success plus the matching hierarchy update. A new CreatedWorkspace outcome is an alternative, not inherently necessary. Test either event ordering and partial failure without duplicate creation or lost fields.

### F2 — P1: Use registry membership, not hierarchy presence, for the registered-only picker

**Plan:** line 33 says build from registered workspace identities but does not name the data source.

**Concrete mismatch:** the hierarchy includes synthetic workspace rows for retained sessions whose registry entry no longer exists (`crates/ovrcr-runtime/src/server/mod.rs:3081–3110`). Both those synthetic rows and genuinely registered but unavailable checkouts may display the same unavailable label. `WorkspaceSummary` has no registration flag (`crates/ovrcr-protocol/src/wire.rs:429–438`), so filtering hierarchy rows by name/path cannot satisfy Q5.

**Smallest correction:** explicitly join registry membership with live hierarchy labels by project + workspace ID. Existing `Request::Inspect` already returns `Response::Inventory { registry, ... }`, consumed by the palette for repository hints (`wire.rs:456`; `palette.rs:3021–3027`). Prefer reusing that source before adding another endpoint or wire discriminator. Test a retained-only synthetic row versus a registered-but-unavailable checkout; revalidate the selected registry identity at Start.

### F3 — P1: Change action availability, not just the `n` label

**Plan:** line 38 changes the entry point but does not specify its opening guards.

**Concrete mismatch:** current `n` is disabled for `launch_blocked.or(no_workspace)` (`crates/ovrcr-tui/src/dashboard/keymap.rs:796–804,1004`). `open_create_terminal` separately refuses a current workspace warning (`palette.rs:785–793`). Reusing either guard on New agent session prevents a user on an unavailable/root-drifted target from opening the picker to choose a valid destination; it also blocks the design's empty-project path.

**Smallest correction:** explicitly distinguish permission to **open the new form** from permission to **launch its selected target**. Make the form/Create branch route reachable for registered projects without requiring an already-launchable workspace; retain root, existence and ownership checks at Start. Preserve the old New terminal guard on its separate flow. Add keymap and production-entry tests for no workspace and an unavailable current selection with another valid destination.

### F4 — P2: Add a multiline editor and submit-dispatch task

**Plan:** the prompt control/input contract is in the specification, but step 3 names no owning editor work. Step 4 names multiline paste and picker Enter, without explicitly naming Enter-newline and the submit chord.

**Source:** existing form field kinds and Enter handling are single-line/picker oriented (`palette.rs:74`; `palette.rs:2340–2440`). Merely adding a text field risks advancing/submitting on Enter. This is a missing implementation task, not evidence of a bug in a feature that does not exist yet.

**Smallest correction:** reuse the existing multiline cursor/editor mechanism where suitable; explicitly cover Prompt Enter-newline, portable submit chord, picker accept without form submit, multiline paste, cursor editing and no input leakage. Add production Dashboard input tests plus native CUA proof of the chosen chord/terminal encoding.

### F5 — P2: Make capability discovery and pending-launch reconciliation explicit deliverables

**Plan:** step 1 correctly makes native contracts a research gate, and step 2 correctly requires Server ownership. It still leaves the Dashboard capability-query binding and reconnect lookup implicit.

**Source:** current model discovery is protected by delegation/session authority (`crates/ovrcr-runtime/src/server/delegation.rs:799–850`). Agent overrides can replace argv behind a familiar name (`crates/ovrcr-tui/src/dashboard/agents.rs:98–110`). Current lifecycle completion uses a Dashboard-connection request token and has no pending-launch reconciliation query (`wire.rs:543–550`; `server/lifecycle.rs:75–101`). A generic visible PTY is not native-startup evidence.

**Smallest correction:** require step 1 to identify how the Dashboard obtains capabilities for the exact launcher configuration, without impersonating a delegated session. Require step 2 to define the stable Server-owned launch/session-run correlation and a reconnect status/read path before implementing prompt retention. Separate known non-delivery from uncertainty; never silently inject options into unknown overrides, prompt setup menus or auto-replay uncertain requests. Keep the approved memory-only/no-restart-replay policy. These are engineering decisions within the approved design, not requests to reopen it.

### F6 — P2: Make compatibility and four-harness evidence executable acceptance items

**Plan:** step 2 already requires callers/wire compatibility and a settings owner; step 4 already requires actual provider proof. The correction is precision, not expanding scope.

**Source:** the exact wire snapshot requires a protocol-version bump when encoding changes (`crates/ovrcr-protocol/src/wire.rs:3980–3998`). The launch-choice settings parser/catalog/editor currently describe `{kind, preset}` only (`crates/ovrcr-runtime/src/settings/mod.rs:270–280,726`; protocol `catalog.rs:168–175,486`; TUI `settings_editor.rs:322–338`). Stand-in setup tests or a shell output marker do not prove the native model or provider prompt path.

**Smallest correction:** name the wire-version/snapshot gate, settings parser/catalog/editor/write-fixture/matrix updates and round-trip compatibility assertions. For each of Claude Code, Codex, Pi and **omp**, define evidence of installed version, applied explicit model or explicitly resolved Harness default, provider-level receipt/executed initial prompt, and native trust interruption. Add real reconnect/restart/uncertain-delivery assertions and a unique prompt marker absence check for settings, retained metadata and OVRCR-owned Events/log output. Generic reconnect/restart coverage is already requested in step 2; the plan should map it to concrete assertions and evidence, not claim it was absent altogether.

## Findings narrowed or not promoted

- **New registration flag required:** not necessarily. Existing Inspect inventory can provide membership, while the hierarchy provides observed labels (F2).
- **New workspace-result wire variant required:** not necessarily. The request already carries the allocated ID; retaining/correlating it is the smaller candidate (F1).
- **Base permanently hard-coded to main:** overstated. The initializer and lookup-error/loading fallback use main, but successful asynchronous Git hints replace the unedited Base (`palette.rs:935–977`). Include non-main/default-detection-failure cases in the child-form task; do not report that discovery mechanism as missing.
- **Project edits preservation absent:** the plan already says to keep explicit edits separately from inherited defaults. Record `refresh_workspace_form`'s wholesale replacement (`palette.rs:845–856`) as the path to change/test, not an independent unspecified requirement.
- **No omp proof today:** true, but explicitly expected by step 1. It is an outstanding implementation gate, not a newly discovered plan contradiction. Do not drop omp or equate it with Pi.

## Outcome and scope

The plan is directionally consistent with the confirmed design but needs the six corrections above to become implementation-ready. No additional product interview is needed for these corrections. No application code, approved design or implementation-plan text was changed by this review. No application tests, native launch smoke, CUA product checks or four-harness capability acceptance ran; the only model execution was the user-authorized reviewer team.

The user-requested narrow execution rule is installed at `/Users/xlyk/.codex/rules/ovrcr-omp-plan-review.rules`. It permits this task's fixed role-constrained launcher, not arbitrary OMP commands. It does not override automatic approval review. All spawned task-owned reviewers were allowed to exit or explicitly stopped; cleanup inventory is recorded alongside this report.
