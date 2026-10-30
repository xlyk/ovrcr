# Operator Working Journal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply Kyle's selected Working journal treatment to the native Operator UI using the current theme.

**Architecture:** Refine the existing Ratatui renderer and its shared geometry;
retain durable message identity and native lifecycle ownership. Keep tool call
and output as compact independent disclosures, rather than creating a pairing
layer. Verify through current Dashboard input paths and a fresh disposable GUI.

**Tech Stack:** Rust, Ratatui, crossterm, current theme tokens and existing native/provider fixtures. No dependencies.

**Spec:** `docs/superpowers/specs/2026-10-30-operator-working-journal-design.md`

## Global Constraints

- Use existing theme accessors only; do not edit global palette values.
- Keep the existing one-line, collapsible Operator sidebar above Projects, its status/[x] conventions, and no model labels.
- No protocol/runtime/auth changes or new keyboard bindings; preserve request, context, exactly-once, no-replay, lifecycle and overlay/WIP-save contracts.
- Keep call/output independently expandable under original durable ordinals; no pairing abstraction, reordering or source-byte decoration.
- All draw, input, cursor, Stop/disclosure hit-testing, scroll/anchor and unread geometry derive from the same OperatorLayout/transcript rows.
- Preserve useful140×40,100×24,80×24,60×24,60×16 frames, at least3 transcript rows at the floor, and truthful too-small draft retention.
- Quick dialog:7rows empty/one-line, grows to16rows, max80columns; preserve modal consumption and real cursor.
- No live credentials/providers, installed executable replacement, main/shared-branch push or merge. Use only task-owned disposable resources.
- Preserve existing fresh native evidence and unresolved local-gate records; no blanket exoneration, timeout/assertion weakening or unrelated stale-error fix.

## Review Focus

- Decoration must not shift durable source-byte anchors across wrapping, streaming, resize, paging or disclosure changes (Task1 tests).
- Hidden/on-demand sidebar and floor/editor growth must not acknowledge unread or move cursor outside the visible editor (Task1 tests).
- Paired, unmatched, failed, timed-out, truncated and empty tool messages retain distinct meaning/identity and full details (Task1 typed-event tests).
- Quick errors/long Unicode input, modal outside clicks and overlays preserve pending requests/drafts and underlying state (Task1 public/input tests; Task2 native check).
- Shared-header/footers retain ordinary terminal appearance and unavailable-action guards, including Stop's exact hit target (Task1 comparative buffers; Task2 native terminal marker).

---

## Task 1: Native Working journal renderer and geometry

**Files:** Modify `crates/ovrcr-tui/src/dashboard/operator/{render.rs,snapshots/original-*.txt}`,
`crates/ovrcr-tui/src/dashboard/{operator.rs,render.rs,hints.rs}`, `tests/tui/operator.rs`,
and affected `docs/development/tui.md`, `docs/operator.md`, `docs/ux/2026-10-30/operator/README.md`.
Do not touch protocol/runtime or the retained untracked screenshot evidence.

**Interfaces:** Consume existing `operator_layout(&self, Rect)->OperatorLayout`,
`transcript_rows(&Chat,u16)->Vec<(Line<'static>,Anchor)>`,
`transcript_offset(&Chat,&[(Line<'_>,Anchor)],usize)->usize`, `operator_quick_mouse`,
`operator_mouse`, `draw_operator_header` and keymap-derived `hints::footer`.
Produce the same behavioral interfaces; private layout fields/helpers may be
extended only to share geometry. Existing anchor=(ordinal,part,byte) remains.

- [ ] Add behavioral/style RED cases `operator_journal_theme_geometry_and_receipts`
  and `operator_journal_quick_content_height_and_modal_consumption`, using current
  setup/history/draw helpers and the real Dashboard buffer/input entry points.
  Assert existing token colors, journal guide/user cue, hidden collapsed IDs/full
  expanded IDs, unchanged ordinal navigation, explicit failure/no-output/truncation,
 7-row short quick dialog,16-row cap, cursor within editor and no outside-click
  forwarding. Check current plus one alternate theme via existing theme selection.
- [ ] Add public integration `operator_working_journal_input_geometry_preserves_context`:
  create/select through real Dashboard responses; draw, focus/expand by keyboard
  and mouse after resize; type multiline Unicode; verify exact Submit text/context,
  no PTY bytes, retained independent A/B draft/anchor/selection and guarded unread.
- [ ] Run the new named cases and confirm relevant failures on the current renderer;
  retain command/count/output. Extend existing typed-event, anchor/floor/cursor,
  quick-overlay tests rather than duplicating their fixtures.
- [ ] Implement the spec in the existing renderer: reserve gutter width before
  wrapping, decorate afterward; compact separate receipts, rules/editor/target,
  quiet Operator-only title fill and contextual hints. Size quick editor from
  wrapped text/error without changing its request/input state machine.
- [ ] Update the five pinned frame files for this intentional design change.
  Preserve substantive content, Unicode, no-frame, selection and behavior checks;
  do not weaken assertions. Update affected docs to distinguish the selected
  Working journal refinement from the previous Original appearance.
- [ ] Run new cases and existing affected tests to GREEN. Then run once:
  `rtk proxy cargo test -p ovrcr-tui --lib`,
  `rtk proxy cargo test -p ovrcr --test tui`,
  `rtk proxy cargo clippy -p ovrcr-tui --all-targets -- -D warnings`,
  `rtk proxy cargo fmt --all -- --check`, `rtk proxy git diff --check`.
  Use private HOME/TMPDIR/config for process/socket tests and scoped required
  permissions. Failed gates remain explicit; report concrete concerns.
- [ ] Self-review, stage intended files only, commit `feat(operator): polish working journal UI`,
  and write the task report with RED/GREEN and exact executed counts.

## Task 2: Native acceptance and delivery evidence

**Files:** Add fresh evidence under `research/operator-working-journal-2026-10-30/`
and update affected acceptance/user docs. Reuse `tests/operator_e2e.rs`'s native
holder and provider fixture unchanged unless a concrete acceptance blocker needs
controller adjudication. Product writes remain owned by Task1 implementer.

**Interfaces:** Consume the reviewed Task1 commit and matching built GUI/CLI,
current holder/gates, original launcher waits and normal Dashboard input controls.
Produce revision/hash-bound screenshots, paired AX, command/results and cleanup
receipts. Controller owns GitHub/CI, final review and diary.

- [ ] Freeze matching checkout GUI/CLI with `gui,acceptance-diagnostics`, using the
  documented bundle launch route and private synthetic local auth/provider. Record
  source/head, hashes, root/socket/PIDs/groups and original owner command sessions.
- [ ] Through CUA, capture a bounded set: wide journal with concurrent A/B and
  unread; keyboard/mouse receipt expansion/full details; real terminal marker then
  Ctrl+O multiline quick dialog and completion; native80×24/60-column and kernel-
  confirmed60×16 fallback. Check cursor/guide/rules/contrast and no clipping in
  pixels, not AX alone. Preserve any failed attempts beside corrected evidence.
- [ ] Close through CUA; retain original GUI/holder exit codes. Verify every
  recorded owned group and root/socket absent before deleting task-owned resources.
  Do not query the closed app or infer waits from process absence.
- [ ] Run required current-source gates once after native cleanup: existing
  compiled-worker/quick outer-PTY acceptance, workspace all-target/all-feature
  regression, workspace Clippy/check, fmt and diff check. Inspect filters/counts.
  On concrete failures, retain command/env/results and investigate only the named
  boundary; no blind retries or unrelated startup/Bridge fixes. Controller obtains
  actual final-head hosted macOS/Linux/capacity/memory evidence before acceptance.
- [ ] Store raw AX/logs losslessly as deterministic gzip with original/stored
  hashes; keep metadata JSON readable. Review pixels, artifact links and final
  diff; commit evidence/docs only, report exact source equivalence after artifacts.
- [ ] Controller reviews this task and whole branch, updates existing Draft PR382
  and diary with final revision/results/gaps. Preserve required PR recovery/worktree;
  no merge or installed app replacement.
