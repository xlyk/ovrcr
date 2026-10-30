# Operator Main Integration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fulfill the user's explicit request to merge PR382, preserving the reviewed Operator feature and current main's Cursor changes.

**Architecture:** Merge current main into the existing isolated feature branch, preserving both wire layouts with a new protocol fence. Review the conflict resolution, verify real native Operator integration and qualify the new candidate using main's current CI workflow before a normal GitHub merge.

**Tech Stack:** Existing Rust workspace, Git/gh, existing private native/worker fixtures.

**Spec:** Existing `docs/superpowers/specs/2026-10-30-operator-inline-input-design.md`; current-main architecture/runtime/Operator/Cursor contracts; user instruction “Merge this in.” No new feature behavior.

## Global Constraints

- Preserve current Operator sidebar, close/archive, Quick Message, journal, inline activity and editing behavior; preserve current-main Cursor stream/token/revision/storage behavior.
- Use a fresh protocol version70 for the combined layout: main69 and candidate68 are incompatible, so neither may negotiate as70. Preserve schema27 and the existing single SessionStore owner; preserve Operator journals/run/archive/history through migration.
- No force push, admin merge, bypass of required checks/reviews, timeout/assertion weakening, live sessions/configuration or installed application changes.
- Reuse existing isolated branch/worktree. Preserve unrelated untracked `research/operator-layout-2026-10-30/followup-native-01/` and all prior evidence/recovery. Only task-owned private configs/sockets/workspaces/processes.
- All shell commands use rtk; GitHub uses git/gh. One source/Cargo/native owner at a time. No nested agents.

## Review Focus

- Version70 must fence both divergent68/69 layouts without losing either variant or field (Task1).
- Schema27 migration and Operator close/archive/run/history coexist without inventing stopped proof or losing journal data (Task1).
- Auto-merged runtime files and new CI partitions must retain both sides' intent (Task1).
- Actual CLI/GUI must negotiate the combined protocol and expose the reviewed Operator behavior (Task2).
- Prior cd54 CI/native proof remains historical; qualification must cover the integration revision (controller gates).

### Task 1: Integrate main and resolve protocol/storage/docs conflicts

**Inputs:** Feature headcd54f350; fetched main0e63a8c370533001c96132ffca0e4914c95d011b (verify current origin/main, report any further advance before merging). Read affected source/callers and current guides. Expected conflicted files: `crates/ovrcr-protocol/src/{codec.rs,wire.rs}`, `docs/development/{operator.md,runtime.md}`. Auto-merged `retained/operator.rs`, testing/CI and other shared callers require semantic inspection.

- [ ] Perform ordinary merge of exact fetched main into the feature branch; never overwrite unrelated work. Preserve both sides of each source/documentation conflict; do not blindly choose ours/theirs.
- [ ] Adopt protocol70, regenerate current snapshots, reject stale68/69 at real framing/handshake boundary. Update current source/callers/docs only; do not rewrite historical evidence/version records. Preserve the Operator run/archived fields and Close/Closed variants alongside current Cursor stream token/revision changes.
- [ ] Extend existing owning migration/real-entry tests where needed to pin schema27 preservation of Operator journal, retained run/archive/model history and ordinary Cursor state. Existing invariants and assertions stay intact. No parallel storage or provider fixture.
- [ ] Run focused protocol/wire/compatibility/migration/Operator and affected Cursor cases with nonzero counts, then appropriate ready-state workspace check/clippy/fmt/diff and relevant suites once. Read current-main test isolation guide; private process HOME/provider profiles plus required socket/PTY permission. Preserve failed attempts separately; no blind retry or timeout changes.
- [ ] Self-review complete merge diff, commit intended tracked changes only. No push/native/CI/merge on GitHub or diary. Full report in plan workspace with exact base/main/result hashes, conflict decisions, source/tests affected, commands/counts/failures/limits and ownership release.

### Task 2: Native combined-protocol Operator acceptance

**Inputs:** Reviewed Task1 source; existing `research/operator-inline-input-2026-10-30/native-03/` recipes and manual holder. Read CUA/UX guides. Create separate `research/operator-main-integration-2026-10-30/` evidence.

- [ ] Fresh-build matching GUI/CLI/holder for combined protocol70, record source/binary identities; exclusive target/native ownership. No source edits.
- [ ] Run actual app with CUA using private synthetic provider and existing fixture. Prove handshake/dashboard, new/saved Operator chat, actual submitted turn with inline Responding/tool feedback, retained draft and common input, separate selected Stop or completion, and close/archive/restore if already available through the existing bounded recipe. Keep assertions scoped to actual observations; compiled-worker gates separately cover full lifecycle.
- [ ] Save a bounded representative JPEG/AX set and durable receipts. Use original holder deadlines and waits; no closed-app query. Close normally and verify owned PIDs/groups/root/socket cleanup; preserve unsuccessful attempts and limits.
- [ ] Report exact revision/commands/results/gaps, commit intended evidence/docs only, release ownership. No remote operations, diary or nested agents.

## Controller delivery gates

- [ ] Independent Task1 and Task2 reviews, then final integration review; resolve blocking findings within bounded SDD workflow.
- [ ] Publish reviewed candidate to existing PR382; mark ready as needed. Run current main's official required/acceptance CI on that candidate. No admin bypass or counting cancelled/skipped checks as passed.
- [ ] Revalidate current main/PR head/mergeability/rules; normal merge with exact head guard under user's explicit authorization. Verify merged state/commit.
- [ ] Extend existing OVRCR work diary; preserve required evidence and unrelated untracked work. Do not replace installed app or force local main checkout.
