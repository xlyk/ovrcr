# Operator Activity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show truthful visible activity during Operator responses and pending tool results.

**Architecture:** Derive activity from existing selected Chat authority and caught-up
typed message history. Split the existing context row and reuse shared spinner/time
and redraw cadence; keep transcript geometry and durable anchors unchanged.

**Tech Stack:** Rust, Ratatui, existing status/theme and controlled provider/native fixtures. No dependencies.

**Spec:** `docs/superpowers/specs/2026-10-30-operator-activity-design.md`

## Global Constraints

- Existing theme tokens/shared100ms spinner only; no global palette/dependency change.
- Split existing context row; no added transcript rows, synthetic ordinals, floor shrink or anchor changes.
- Current-turn/call-ID matching plus confirmed tail before a specific tool-wait claim; paging/dirty history stays generic.
- Pending receipt means waiting for a recorded result, not proof of operation execution; no percentage/ETA.
- Preserve sidebar, eligibility/Send, visible-row title identity, Ctrl+O, draft/caret, Stop, archive, Unread and overlay contracts.
- No runtime/protocol/auth/lifecycle changes, live accounts, installed executable replacement, shared/main push or merge.
- Keep failed local gates and historical native revisions explicit; no unrelated stale-error/startup/Bridge fixes or blind retries.

## Review Focus

- Silent Running before any messages and quiet gaps between deltas still animate through actual idle redraw (Task1).
- Reused call IDs across turns, multiple/nonadjacent calls, empty/failed/timed-out outputs and paged histories never imply the wrong pending tool (Task1).
- Hidden/collapsed/offscreen sidebar retains visible selected-chat animation; invisible/new/tiny journal does not request its cadence (Task1).
- Active indicator beside long target/error/CJK names at60×16 preserves readable labels, caret, anchors and Unread guard (Task1).
- Terminal/stopping/unconfirmed-stop transitions remove or qualify feedback without enabling Send/replaying input or leaking another chat's state (Task1).

---

## Task 1: Visible activity, redraw and actual-worker/native proof

**Files:** Modify `crates/ovrcr-tui/src/dashboard/{operator.rs,operator/render.rs,state.rs,render.rs}`,
affected exact `operator/snapshots/original-*.txt` only for intentional running frames,
`tests/tui/operator.rs`, and owning `tests/operator_e2e.rs`/`tests/support/operator_provider.rs`
only as needed for controlled real-worker gates. Update `docs/operator.md`,
`docs/development/{tui,testing}.md` and add source/hash-bound evidence under
`research/operator-activity-2026-10-30/`. Preserve prior research bytes.

**Interfaces:** Consume existing `Chat::{submitting,stopping,closing,messages,reading,dirty,
confirmed_tail,ordinal,summary}`, `OperatorMessage::{turn,ordinal,content}`,
`OperatorWorkerEvent::{ToolCall,ToolOutput,AssistantDelta}`,
`status::sidebar_activity_glyph(AgentActivity::Busy,false,now)->(char,Color)` and
`status::SPINNER_INTERVAL`, `draw_dashboard_at(frame,&Dashboard,now_unix_ms)`,
`operator_layout(area)->OperatorLayout` and `redraw_interval()->Duration`.
Thread `now_unix_ms:u64` through private `draw_operator`/`draw_operator_conversation`;
leave public draw API, transcript rows, geometry and event-loop ownership unchanged.
Derived activity query stays private and is shared by rendering/cadence.

- [ ] Add unit `operator_activity_state_and_cadence` and public
  `operator_activity_draw_tracks_real_responses_and_clock` behavioral RED cases.
  Assert Sending→Responding before first delta, streaming, current-turn unmatched
  call→matched result→Responding→Complete absence; exact labels from spec, multiple
  IDs, wrong-turn/id, empty/fail/timeout, dirty/paged fallback, stop/unconfirmed state
  and independent chats. Render0/100ms through public draw and assert changed
  indicator cell plus unchanged requests/draft/cursor/anchor/unread.
- [ ] Record meaningful failures before product edits. Cover hidden/collapsed/
  offscreen sidebar cadence and no-visible/new/tiny/static cases; both themes,
  long target/error/escaped CJK names and existing140×40/80×24/60×24/60×16 floor.
- [ ] Implement the spec through a private derived status and existing context
  Rect split, cell clipping and theme styles. Reuse shared timestamp/spinner;
  add visible journal predicate to redraw_interval, leaving event_loop scheduling.
- [ ] Add/extend one meaningful compiled-worker case
  `compiled_worker::operator_activity_waits_for_real_model_and_tool` using the
  existing local OAuth/provider/real socket DashboardClient. Hold real Submitted
  receipt (Sending), a silent acknowledged Running turn (Responding), and a real
  Bash tool without output (Waiting for bash result); release each once, assert
  final content/Complete/stop proof and indicator absence. Reuse owned release
  patterns; no new provider backend or personal credentials. Record GREEN/counts.
- [ ] Run affected focused cases, owning serial TUI library, publicTUI and new
  compiled-worker case once, then workspace Clippy/check, fmt and full-phase diff
  checks. Use isolated config/HOME/TMP/socket. Preserve failures and filtered counts;
  no retries without a new relevant outcome or source change. Controller obtains
  final-head hosted macOS/Linux/capacity/memory; no local startup full-suite retry.
- [ ] Commit reviewed/self-checked product/tests/docs. Freeze fresh matching GUI/CLI
  with `gui,acceptance-diagnostics`; reuse current documented native holder/bundle
  route, synthetic OAuth/provider and new actual pending gates. Through CUA capture
  a bounded silent-response frame pair and pending-tool pair at distinct frames
  with no input/provider progress between each pair, sidebar hidden/collapsed;
  release to completed indicator-absent state. Keep one80×24 or narrow context/error
  frame; existing unit/floor buffers cover remaining geometry. Do not run Cargo
  or overwrite targets while native owners alive. Retain full AX/log hashes,
  original GUI/holder waits and owned PID/group/socket/root cleanup. Do not query
  closed app. Preserve failed attempts separately.
- [ ] Commit evidence/docs only after cleanup; verify product equivalence to frozen
  revision and full intended diff check. Write task report with RED/GREEN, exact
  commands/counts/failures, native scope, source/hashes and original wait/cleanup.
  No nested agents, push/merge or diary. Root performs task/final reviews, hosted
  final-head checks, existing DraftPR382 update and required diary extension.
