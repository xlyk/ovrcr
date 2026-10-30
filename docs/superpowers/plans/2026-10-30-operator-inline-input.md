# Operator Inline Activity and Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Put Operator progress in the conversation flow and support familiar editing commands in both message inputs.

**Architecture:** Reuse the existing derived activity state, canonical transcript rows and TextCursor. Keep all persistence and runtime ownership unchanged. Sequential source tasks share operator.rs, followed by one combined native acceptance pass.

**Tech Stack:** Rust, Ratatui, crossterm; existing real socket/worker and native GUI fixtures.

**Spec:** `docs/superpowers/specs/2026-10-30-operator-inline-input-design.md`

## Global Constraints

- Keep the approved Working journal, existing theme tokens, sidebar and quick-message behavior.
- Restore the entire context/error row above the composer. The indicator is one clipped row, using the existing animated glyph and cadence; it disappears under the existing stopped-proof rules.
- Scrolling to older messages naturally scrolls it away, and returning to the tail reveals it. Never pin it to the bottom of the terminal or force the reader to follow.
- Three durable viewport rows remain required for review acknowledgement. Keep the 60×16 floor and existing responsive layout.
- Apply editing only to editable full Operator message input and editable Quick Message; preserve existing shared TextCursor::key behavior elsewhere.
- Ctrl+C clears a literally non-empty draft first; a separate Press with empty full composer uses existing selected exact Stop. Empty Quick is a consumed no-op. Repeat/Release Ctrl+C performs no action.
- Never mutate frozen Submit/Quick payloads, queued requests, another chat's draft or run. Editing produces no PTY bytes, Shutdown or Detach.
- No dependencies, protocol/runtime/provider changes, selection model, kill ring, undo/history/completion, merge or installed-binary replacement.
- Preserve unrelated untracked research/operator-layout-2026-10-30/followup-native-01/. All fixtures use private config/socket/workspaces and stop only owned resources.
- All shell commands use rtk; GitHub operations use git/gh. Each source task owns Cargo exclusively; no target writes during native ownership. No nested agents. Keep full task report in the plan workspace, return only status/commits/test summary/concerns.

## Review Focus

- Transient-only history and None selection must not highlight, expand, acknowledge or fabricate an ordinal (Task 1).
- Older history must keep durable byte anchors when the activity appears, streams, disappears or the window resizes (Task 1).
- Multiline CJK/combining/emoji word and deletion commands must retain grapheme and newline boundaries (Task 2).
- Holding Ctrl+C after clearing must not interrupt a response; Quick and frozen retries must not target another draft/run (Task 2).
- Real native coordinates, input dispatch and cleanup can disagree with TestBackend output (Task 3).

---

### Task 1: Move activity into canonical transcript rows

**Files:** Modify `crates/ovrcr-tui/src/dashboard/operator/render.rs`, `crates/ovrcr-tui/src/dashboard/operator.rs`, `tests/tui/operator.rs`, `docs/operator.md`, `docs/development/tui.md`. Read TUI/testing guides, spec and `/private/tmp/operator-inline-preflight.md`.

**Interfaces:** Consume existing `conversation_activity`, `operator_conversation_busy`, timestamp and `Anchor`. Produce `transcript_rows(chat: &Chat, width: u16, now_unix_ms: u64) -> Vec<(Line<'static>, Option<Anchor>)>` and matching optional-row `transcript_offset`. Durable stored anchors remain unchanged; geometry callers pass 0.

- [x] Write/extend owning unit/public tests showing baseline label is outside transcript. Assert exactly one activity row follows durable content/tool, full context/error width and one clock cell changes within transcript. Cover existing themes and 60×16, 60×24, 80×24 and wide fixtures.
- [x] Run focused tests and retain behavioral RED output before source implementation (compiler errors are not defect proof).
- [x] Append one unanchored, gutter-aligned clipped row in canonical rows before offset/slice. Remove only the final part-4 durable separator while activity exists. Restore full context row; reuse state/cadence without new state or fake messages. Adapt Tab, selection/reveal, scroll, mouse, offset and direct tests to ignore None anchors for durable actions. Guard review height for the extra transient row before borrowing chat mutably.
- [x] Test long history PageUp/wheel hides activity without forcing follow, return tail reveals, anchor persists across stream/resize/removal and transient-only/click/Tab/Enter creates no message action. Preserve successful completion review and tiny/overlay no-review cases. Keep logical durable transcript_lines unchanged.
- [x] Update affected docs. Run focused regressions, then full serial TUI lib and public TUI once, fmt/check/clippy as needed for this code; confirm nonzero test counts. Save commands/counts/revision/failures to task report. Self-review and commit intended files only. Do not launch native or remote CI.

### Task 2: Add scoped message editing commands

**Files:** `crates/ovrcr-tui/src/dashboard/{text_cursor.rs,operator.rs,keymap.rs,event_loop.rs}`, `crates/ovrcr-tui/src/dashboard/operator/render.rs`, `tests/tui/operator.rs`, `tests/operator_e2e.rs`, `docs/operator.md`, `docs/development/tui.md`, and `src/gui/input.rs` for native repeated Ctrl+C only. Read TUI/testing guides, spec and `/private/tmp/operator-input-preflight.md`.

**Interfaces:** Consume Task 1 optional-anchor rows without changing their behavior. Add `TextCursor::edit_key(&mut self, text: &mut String, key: KeyEvent) -> bool` (true means consumed) for only the full composer and Quick editors. Keep `TextCursor::key` unchanged. Ctrl+C is application-specific and remains in the input owners, not TextCursor. Reuse existing `operator_stop` and retry synchronization.

- [x] Add failing owning tests for Ctrl+A/E line edges, Ctrl+B/F graphemes, Ctrl+H/D deletion, Ctrl+U/K line-prefix/suffix deletion retaining newline, Ctrl+W whitespace-delimited previous word within line, Alt+B/F within-line word movement; empty/boundary/midline/CJK/combining/emoji cases. Preserve Home/End whole-buffer and other editor semantics. Unsupported modifier combinations never insert letters; Ctrl+Alt is not Ctrl-only.
- [x] Add failing production-dispatch regression for full and Quick Ctrl+C: clear literal nonempty including whitespace; Repeat/Release consumed; empty full Press exact selected Stop once; other chat remains usable; empty idle no-op; Quick never stops underlying; Creating/Submitting frozen; seeded/unseeded retry and captured Submit payloads remain intact. Existing overlays, model/sidebar/transcript focus and ordinary Terminal Ctrl+C byte 3 retain behavior. Record behavioral RED output.
- [x] Implement smallest grapheme-safe TextCursor helpers and scoped owner dispatch, using the spec's exact key semantics. Full composer may repeat ordinary editing; destructive Ctrl+C is Press-only. Keep Quick's existing Press-only behavior. No global interception. Suppress the native GUI's known repeated Ctrl+C events before raw byte encoding; preserve first Ctrl+C byte3, other control keys and ordinary repeat. Add a GUI encoder regression and document the legacy external-terminal repeat limitation; no timing heuristic or protocol redesign.
- [x] Make common commands discoverable through existing help/action mechanisms without crowding the footer. If Alt bindings are in the action table, adapt exact Operator modifier matching narrowly. Update user/developer docs and Quick hint as needed.
- [x] Extend existing compiled-worker independent Stop case to drive empty-composer Ctrl+C and prove other Bash group/terminal survives. Extend existing production socket/Quick cases and public Dashboard assertions; do not create a parallel provider fixture.
- [x] Run focused new tests, full serial TUI lib/public TUI, affected production/E2E cases, workspace all-target/all-feature check/clippy, fmt and diff checks once at ready state. Preserve nonzero counts and failures in report; self-review and commit only intended files. No native launch or remote CI.

### Task 3: Combined native acceptance and evidence

**Files:** Create `research/operator-inline-input-2026-10-30/` evidence/recipes/README; update `docs/development/testing.md` only if linking evidence. Read `.agents/skills/ovrcr-ux-pass/SKILL.md`, docs/testing-computer-use.md, spec and the previous activity native-02 recipes as existing fixture reference.

**Interfaces:** Consume the frozen reviewed source after Tasks 1–2. Reuse existing feature-gated fixture-activity real worker gates from tests/operator_e2e.rs and tests/support/operator_provider.rs. No source edits or shared target writes by others during ownership.

- [ ] Build matching GUI/CLI/holder and record exact source revision/hashes. Prepare private config/socket/workspaces, launch through owned recipe retaining original process wait handles. Use actual CUA native app; never query a closed app. Do not use live user configuration or paid providers.
- [ ] Capture silent Responding and pending-tool pairs at distinct shared clock frames, with gates and durable turn/messages unchanged between each pair. Show activity immediately after message/tool inside transcript, full context, retained unsent draft, hidden/collapsed sidebar independence. Scroll long history away/return via real input, preserving older viewport. Release existing gates once and show completion removes activity and restores Send with same draft.
- [ ] Use real native input for full and Quick multiline/Unicode line/word/deletion shortcuts; show before/after. Prove Ctrl+C draft clear followed by Repeat safety where native API supports it (otherwise automated proof explicitly separated), and separate empty full Press stops a real selected response while another chat/terminal survives. Prove empty Quick does not stop underlying and normal terminal Ctrl+C retains control behavior. Deliberately held Create/Submit receipts, frozen retry payloads, Repeat/Release and overlay permutations are owned by Task 2 production-dispatch tests; do not add a native proxy or timing race to duplicate them. Check the narrow/floor layout using the established nested fixture if required by observed clipping. Save screenshots and AX for visible outcomes; state automated-only cases honestly.
- [ ] Close via CUA; obtain original GUI and holder waits, verify recorded task-owned PIDs/groups/root/socket closure. Record failed/blocked/forced paths separately, never infer exit0 from absence. Stop only owned resources and preserve evidence/recovery on failure.
- [ ] Write exact acceptance mapping/commands/counts/gaps/revision and cleanup report. Commit intended research/docs only; no source changes, GitHub writes or diary edits. Return report and concise status. Root performs final review, CI/PR and diary closeout.
