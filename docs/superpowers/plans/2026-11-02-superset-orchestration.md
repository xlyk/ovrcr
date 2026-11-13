# Superset orchestration adaptation implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Superset’s dispatch-ready-before-wait sequence explicit in OVRCR’s existing managed-Pi delegation procedure, only if the current wording does not already say it.

**Architecture:** Reuse the existing OVRCR workflow record, assignment handles, Server receipts, dependency gates, and acceptance path. The proposed change is a small skill instruction; the Server, tool surface, package routing, and authority model remain as they are.

**Tech Stack:** Markdown skill procedure, existing OVRCR Pi package and Rust acceptance tests.

**Spec:** [Research report](</Users/xlyk/Code/ovrcr/reports/Superset orchestration for OVRCR.md>)

**Status: UNAPPROVED RESEARCH PROPOSAL.** This is a research recommendation only. The user requested research and a plan, not implementation, tests, commits, or expanded parent authority.

## Global Constraints

- OVRCR remains synchronous Rust on macOS and Linux, with one Server owner, one active Dashboard, and a shared limit of 50 live sessions.
- The managed Pi parent and existing authenticated `ovrcr_*` tools remain the only parent-control route in this slice; plain Pi has no delegation authority.
- Use the existing workflow record and exact task/workspace IDs, assignment handles, receipts, and authority epochs. Do not add a parallel task map or scheduler.
- `ovrcr_wait` observes; collection and typed acceptance remain separate. Do not infer success from Ready, process state, or worker-authored markers.
- Do not claim cost, output-token, or native-child bounds that the current protocol rejects.
- Do not expand to non-Pi parents, generic MCP, native provider subagents, remote host provisioning, pages, or pane APIs.
- Do not imply scheduled tasks load the packaged skill; their execution disables discovered extensions and skills.

## Review Focus

| Risk | Existing tests inspected | Coverage limit |
|---|---|---|
| **Stale/retired authority** | `public_dispatch_retired_parent_a_to_b_to_a_cannot_replay_control`; `assignment_control::public_assignment_human_takeover_return_fences_generics_and_preserves_history` | These cover retired controller generations and human takeover/return. They do not establish a separate foreign-parent identity case; no authority change is proposed here. |
| **Observation mistaken for success** | `public_pi_research_assignment_launches_collects_and_fences_real_child`; `dependencies::public_dependencies_gate_inputs_preserve_complete_untracked_lineage_and_invalidate_only_descendants` | Covers collection/acceptance separation and a downstream predecessor gate; no status label becomes success. |
| **Lost or uncertain reply** | `public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects`; `coding::public_coding_lost_launch_reply_reconciles_one_workspace_and_one_child` | Covers request/launch reconciliation. `public_dispatch_partial_and_uncertain_input_is_never_replayed` is acceptance-diagnostics gated. |
| **Capacity/provider substitution** | `public_pi_research_assignment_launches_collects_and_fences_real_child` | Covers explicit choices, refusal, and the shared cap in the inspected fixture; actual provider quotas are outside this proposal. |
| **Premature coding/dependency acceptance** | `coding::public_coding_uses_distinct_workspaces_and_reuses_safe_child_for_followup`; `dependencies::public_dependencies_gate_inputs_preserve_complete_untracked_lineage_and_invalidate_only_descendants`; `dependencies::public_dependencies_authenticate_exact_independent_review_and_refuse_inherited_write_scope_expansion` | Covers candidate/check states, predecessor evidence, and independent review; the proposed instruction must keep those existing gates. |

---

## File map

| File | Use |
|---|---|
| `pi/skills/ovrcr-orchestrator/references/delegation.md` | The only proposed edit: add the explicit ready-set dispatch order if it is absent. |
| `pi/skills/ovrcr-orchestrator/SKILL.md` | Read-only check that direct work remains separate from the explicitly requested Pocock workflow and requires a managed Pi parent. |
| `pi/skills/ovrcr-orchestrator/references/workflow.md`, `implementation.md`, `records.md` | Read-only contract checks for full-workflow separation, coding evidence, recovery, and the existing record. |
| `docs/pi-research-assignments.md`, `docs/pi-package-setup.md`, `docs/agent-reporting.md`, `docs/scheduled-tasks.md` | Read-only contract references; change only if a real contradiction is found and separately reviewed. |

No Rust, TypeScript, package manifest, `.agents/skills`, or test file changes are proposed. The existing package already discovers the skill beside the managed extension and uses one runtime initializer.

## Task 1: Check and clarify direct fan-out order

**Files:**
- Modify only if absent: `pi/skills/ovrcr-orchestrator/references/delegation.md`
- Read: the files listed in the file map
- Test: reuse existing package/delegation acceptance; no new automated test is proposed for this documentation-only scope

**Interfaces:**
- Consumes: the current workflow record and existing capability, start, inspect, wait, collect, dependency, review, acceptance, and delivery tools. Use their current schemas; this plan proposes no API.
- Produces: a concise procedure that records each child’s scope, project/workspace, acceptance evidence, and real prerequisites in the existing workflow record; starts all currently ready independent children before waiting; keeps each returned handle on its existing record; then observes and collects exact results. Use `ovrcr_dependencies` only for actual accepted predecessors.

- [ ] **Step 1: Compare current wording with the proposed sequence.** If `references/delegation.md` already explicitly records each assignment and starts the full ready set before monitoring, make no change and close this task as a no-change review.
- [ ] **Step 2: Add only the missing sequence.** If absent, add the ready-set ordering to `references/delegation.md`. Keep existing routing, authority, evidence, recovery, question, review, acceptance, delivery, and archive instructions intact; do not rewrite or duplicate them.
- [ ] **Step 3: Review the exact diff.** Confirm it introduces no task store, second coordinator table, new dispatch/tool API, new host, provider claim, automatic integration, or implied acceptance.
- [ ] **Step 4: Run focused existing acceptance if implementation is later authorized.** Use the package loader and relevant delegation tests below. Exercise the bounded multi-assignment sequence through the managed Pi package from the reviewed checkout when a capable test host/provider is available. Record unavailable checks as unverified, not passed.

### Existing tests to reuse (inspected, not run)

Root tests in `tests/pi_delegation.rs` require no module prefix:

- `public_pi_research_assignment_launches_collects_and_fences_real_child`
- `public_dispatch_retired_parent_a_to_b_to_a_cannot_replay_control`
- `public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects`

Nested modules must be qualified when using `--exact`:

- `coding::public_coding_uses_distinct_workspaces_and_reuses_safe_child_for_followup`
- `coding::public_coding_lost_launch_reply_reconciles_one_workspace_and_one_child`
- `assignment_control::public_assignment_human_takeover_return_fences_generics_and_preserves_history`
- `dependencies::public_dependencies_gate_inputs_preserve_complete_untracked_lineage_and_invalidate_only_descendants`
- `dependencies::public_dependencies_authenticate_exact_independent_review_and_refuse_inherited_write_scope_expansion`

Focused commands for the ordinary integration tests (run only if implementation is later authorized):

```sh
cargo test -p ovrcr --test pi_delegation public_pi_research_assignment_launches_collects_and_fences_real_child -- --exact
cargo test -p ovrcr --test pi_delegation public_dispatch_retired_parent_a_to_b_to_a_cannot_replay_control -- --exact
cargo test -p ovrcr --test pi_delegation public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects -- --exact
cargo test -p ovrcr --test pi_delegation coding::public_coding_uses_distinct_workspaces_and_reuses_safe_child_for_followup -- --exact
cargo test -p ovrcr --test pi_delegation coding::public_coding_lost_launch_reply_reconciles_one_workspace_and_one_child -- --exact
cargo test -p ovrcr --test pi_delegation assignment_control::public_assignment_human_takeover_return_fences_generics_and_preserves_history -- --exact
cargo test -p ovrcr --test pi_delegation dependencies::public_dependencies_gate_inputs_preserve_complete_untracked_lineage_and_invalidate_only_descendants -- --exact
cargo test -p ovrcr --test pi_delegation dependencies::public_dependencies_authenticate_exact_independent_review_and_refuse_inherited_write_scope_expansion -- --exact
```

Package-loader command after provisioning its documented test host:

```sh
OVRCR_TEST_PI_PACKAGE_ROOT=/absolute/path/to/node_modules/@earendil-works/pi-coding-agent \
  cargo test -p ovrcr --test pi_package --features acceptance-diagnostics \
  production_materializer_loads_local_package_with_one_injected_owner \
  -- --ignored --exact --nocapture --test-threads=1
```

`public_dispatch_partial_and_uncertain_input_is_never_replayed` is a separate acceptance-diagnostics-gated case, not ordinary focused coverage.

**Proposed automated test names:** none. The first slice only clarifies procedure text and reuses tests that exercise the underlying workflow; no brittle test of prose is proposed. If later work changes product behavior or adds a parent host, that design must define its own new tests and acceptance before implementation.

## Conditional roadmap for full cross-client parity (unapproved)

The first slice covers **core Pi-only equivalence**: the managed Pi parent already has durable assignment, observation, collection, review, acceptance, delivery, and recovery. Later stages describe named non-Pi reach and optional status skills. They are conditional, unnecessary for core parity, and unapproved.

### A. Add a named non-Pi parent adapter

**Dependency:** select the client and approve its parent-identity and authentication contract before choosing an entry point. Bind durable parent/controller identity and route through existing Server authority and receipts. It must not create a second state owner, use generic PTY input as parent control, or expose the managed Pi lease credential. The current Pi tool layer is a compatibility reference (`src/ovrcr-pi-runtime.ts`); inspect the existing Server delegation request/receipt implementation (`crates/ovrcr-runtime/src/server/delegation_*.rs`) and protocol validation (`crates/ovrcr-protocol/src/delegation.rs`). The child publisher in `src/cli/delegation_publisher.rs` is not a parent API. Exact new adapter files and tool schemas remain a design output, not a decision made here.

**Acceptance:** a real named client starts, observes, and collects an assignment through the Server; a stale/retired parent is refused; a lost start or follow-up reply reconciles against its original request without duplicate launch or input replay; and the existing one-owner package/managed-Pi path still passes. Current tests such as `public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects` and `public_dispatch_retired_parent_a_to_b_to_a_cannot_replay_control` cover related managed-Pi behavior, but they do not prove a distinct foreign-parent identity contract. Add adapter-specific authentication/identity tests after that contract is approved, then exercise the actual selected client.

### B. Package discovery and setup for that client

**Dependency:** Stage A must define a supported parent surface and how the selected client discovers it. Inspect `package.json`, `pi/extension.ts`, `.agents/skills/ovrcr/SKILL.md`, `docs/pi-package-setup.md`, and the relevant `src/cli/args.rs` entry points. Gate on discovery and actual availability of the approved parent tools, using a capability read exposed by the approved adapter where available; package metadata alone is not proof that the tools loaded. Do not copy `superset-mcp` configuration guidance or claim the child-publisher MCP command grants parent operations. The setup skill must fail closed when the selected client cannot see or invoke the approved parent tools; do not silently fall back to shell-launched agents or PTY control.

**Acceptance:** in a disposable profile, verify discovery, one runtime owner on the Pi route, and a bounded parent operation through the selected host. Missing or incompatible tools must produce a clear refusal with no child effect. The existing `production_materializer_loads_local_package_with_one_injected_owner` gate applies to Pi packaging; a different host needs its own loader and real-client acceptance.

### C. Add optional read-only standup/doctor support

This can remain a skill-only step if composed reads are sufficient: standup may use existing JSON inventory/status plus `ovrcr events`; doctor may use provider-specific `ovrcr agent doctor PROVIDER --json` and the Pi package recovery guide. Inspect `src/cli/args.rs`, `docs/development/runtime.md`, and `docs/pi-package-setup.md`. Clearly label unavailable reads and inter-read drift. Define a bounded read-only snapshot across existing state owners only if a named use case demonstrates that inconsistent separate reads or missing pane state changes a decision; do not create another owner or include worker success claims in a snapshot.

**Acceptance:** both skills stay read-only, report gaps instead of filling them with guesses, and make no dispatch/cleanup changes. If a snapshot is approved, test consistency against concurrent session changes, unavailable state, and Dashboard ownership through the existing Server path; exact tests require that snapshot contract.

**Separate future scopes:** scheduled-task redesign, native provider subagent tracking/cost, remote environment provisioning, page publishing, browser/computer control, panes, and third-party integrations. None follows automatically from adding a parent adapter.

## Decision gate before expanding parent authority

No later phase in this roadmap is authorized by this plan. A later proposal must name the intended parent host and decide whether standup needs only best-effort composed reads or a consistent snapshot, whether provider-native child work needs tracking/bounds, and which optional support skills matter. A non-Pi parent requires a separate identity/authentication and idempotency design against the existing Server receipt path. A provider-native roster requires provider coverage, persistence, size/restart rules, and honest cost semantics.

**2026-11-11 update:** [PR #471](https://github.com/xlyk/ovrcr/pull/471) separately
shipped built-in Operator coordination through ordinary Workspace Agent sessions,
native tools and `watch_sessions`, with leased residency and asynchronous wakeups.
This uses CLI/session observations rather than this roadmap's Server-owned
workflow-controller authority. See [Operator](../../operator.md) and its
[acceptance evidence](../../../research/operator-async-delegation-465/README.md).

## Deferred family mapping

- **Reuse existing material:** setup and provider doctor guides; the Pi task scheduler for automation; current capability discovery for `10x`. Standup could be a read-only JSON/`ovrcr events` skill if inter-read drift is acceptable.
- **No orchestration implementation:** feedback, contribute, browser, computer, third-party plugins/integrations, page publishing, and pane management.
- **Conditional design only:** external MCP parent access, cloud environment provisioning, a host snapshot, or native provider-child accounting. Superset’s MCP surface alone does not justify a new OVRCR service.

The OVRCR audit is pinned to the local checkout `bdd30e77d0c2fab589ff2c605bcf5f0e31cd1944`; local `origin/main` matched local `main` at inspection, but no fetch or remote verification was performed. No tests, commits, provider launches, or product changes were made for this proposal.
