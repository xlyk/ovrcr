# Operator thinking block (#449)

## Context

Implement GitHub issue #449: thinking text in the Operator transcript is peach and italic, streams with its latest six wrapped lines, collapses after a turn ends, and expands or collapses per block with Enter or a click. Preserve a single transcript scroll and keep a toggled block's header at the same screen row. Cover the real application paths with tests and terminal/native-GUI computer-use evidence, then review and commit on the current branch without pushing.

## Execution baseline

- Issue: https://github.com/xlyk/ovrcr/issues/449. Parent testing decisions: https://github.com/xlyk/ovrcr/issues/448. No blocking ticket.
- Work in `/Users/xlyk/Code/ovrcr-workspaces/feature-issue-449-operator-thinking-block`, branch `feature/issue-449-operator-thinking-block`. It was clean at `6d0eb9c97b3a8b378d8e17094225ba324330e02e`, also the local `main` baseline. This already satisfies the isolated-worktree requirement; do not create another worktree.
- Read the root `AGENTS.md`, `CONTEXT.md`, and the guides for TUI, Operator, testing, computer use, and delivery before editing. Preserve intervening user changes.
- Parent #448 already approves the testing seams: public Dashboard input/rendering and the stand-in-Pi Operator worker/end-to-end path. Use these seams for red–green slices.
- All checks and native launches happen after plan approval. No builds, tests, source edits, or app launches ran while planning.

## Grounded design

`Chat` already owns per-entry expansion, focus, and the single transcript viewport. `logical_lines` merges thinking deltas; `transcript_rows` wraps them and maps each body row back to a durable source delta. Keep that durable anchor separate from the block's disclosure identity.

`transcript_offset` already supports an anchored viewport without forcing a full tail page. Reuse this behavior to keep a toggled header stationary. `theme::PEACH()` and the native GUI's italic font path already exist; change neither the palette nor the GUI renderer. No dependency, setting, protocol, registry, or production worker change is needed.

## Approach

Steps 1–3 form one production change and should have one owner. Step 4 changes only the end-to-end fixture/test file and can proceed independently against the behavior below. Keep builds and live tests sequential when they share the worktree's executable.

### 1. Represent the user's per-block choice

In `crates/ovrcr-tui/src/dashboard/operator.rs`, change `Chat.expanded` from `BTreeSet<u64>` to `BTreeMap<u64, bool>`, keeping the field name and all state local to the existing `Chat`. Initialize it with `BTreeMap::new()`. Missing means automatic; `true` means explicitly expanded; `false` means explicitly collapsed. This reuses the existing entry store without a second set that could conflict with it. Existing user/tool entries still default to collapsed.

Key a thinking block by the first ordinal of a consecutive, same-turn run of `OperatorWorkerEvent::ThinkingDelta`. More deltas and later history pages retain that key. Preserve `operator_attach` and `operator_history`: they already install authoritative turn summaries and retain Chat state across catch-up and reopen.

Use these exact display rules, with `N` computed from wrapped body rows:

| Entry choice | Turn state | Display | `entry.expanded` |
| --- | --- | --- | --- |
| `Some(true)` | Any | `▾ Thinking · N lines`, all body rows | `true` |
| `Some(false)` | Any | `▸ Thinking · N lines`, no body | `false` |
| Missing | This block's turn is current and `Running`, `N > 6` | `▸ Thinking · showing last 6 of N lines`, latest six body rows | `false` |
| Missing | This block's turn is current and `Running`, `N <= 6` | `▾ Thinking · N lines`, all body rows | `true` |
| Missing | Otherwise | `▸ Thinking · N lines`, no body | `false` |

“Ended” includes `OperatorTurnState::Complete`, `Error`, `Interrupted`, and older turns while another turn runs. `Failed` is a worker event, not a turn-state variant. `worker_stopped == false` does not keep an already-ended turn's thinking open. A zero-text block emits nothing before evaluating this table.

This table defines the thinking behavior to describe in `docs/operator.md:142–160` and `docs/development/operator.md:177–181`. Replace the obsolete “muted Thinking paragraph” and user/tool-only selection description in `docs/development/tui.md:189–199` with the same per-block contract. The native scenario below replaces the short-stream-only walkthrough in `docs/testing-computer-use.md:467–474`.

Migrate every old store use: the renderer's `contains` lookup, both input toggles, `Chat::new`, and direct unit-test insertions near `operator.rs:1747,2213` become boolean-map uses. Locate all remaining uses with `grep` pattern `expanded` limited to `crates/ovrcr-tui/src/dashboard/operator.rs` and its render submodule; leave no set-based compatibility path.

### 2. Render once, count after wrapping, and keep source anchors

In `operator/render.rs`, add `const THINKING_PREVIEW_LINES: usize = 6;` and private `ThinkingDisplay { Capped, Collapsed, Expanded }`. Add `fn thinking_display(chat: &Chat, ordinal: u64, turn: OperatorTurnId, total: usize) -> ThinkingDisplay` implementing the table above.

Replace the canonical row tuple with these internal types; no equivalent currently separates source identity from effective disclosure state:

```rust
pub(super) type Anchor = (u64, usize, usize);

#[derive(Clone, Copy)]
pub(super) struct TranscriptEntry {
    pub(super) ordinal: u64,
    pub(super) expanded: bool,
}

pub(super) struct TranscriptRow {
    pub(super) line: Line<'static>,
    pub(super) anchor: Option<Anchor>,
    pub(super) entry: Option<TranscriptEntry>,
}
```

Change `transcript_rows(chat: &Chat, width: u16, now_unix_ms: u64) -> Vec<TranscriptRow>` and `transcript_offset(chat: &Chat, rows: &[TranscriptRow], height: usize) -> usize`. `entry.expanded` describes the effective display, not just a stored override. It is true for an untouched short block, so its first click collapses immediately rather than appearing to do nothing.

Keep `logical_lines` as the existing source-text builder. Extend `TranscriptLine` with `entry: Option<TranscriptEntry>` and `thinking: Option<ThinkingLine>`, where private `ThinkingLine` has `Header(OperatorTurnId)` and `Body` variants. `push_lines` initializes these fields to `None`; annotate newly produced rows at the existing `first_line` boundary:

- User/tool rows receive their message's ordinal and current boolean expansion, preserving existing disclosure behavior.
- The thinking arm joins same-turn adjacent deltas once, escapes with `display_text`, trims only trailing `'\n'` terminators, and suppresses an empty result. Preserve interior blank lines and spaces.
- Push one tagged header and tagged body logical lines with the first ordinal, existing body `part = 3`, and original byte offsets. Do not cap logical lines before wrapping.
- Add `ThinkingDelta(_)` to the existing `expandable` predicate and use that predicate at logical-row construction. It must no longer enumerate raw messages for keyboard/mouse selection.
- Assistant, status, blank separator, and transient activity rows have `entry: None`. Empty thinking must bypass the common separator insertion as well: append a separator only if that message/group actually added rows.

In `transcript_rows`, replace the `flat_map` with ordered row construction. For a tagged thinking header, consume its immediately following body lines, move their `Line`s into the existing `wrapped_rows` call, and retain their base byte offsets. Wrap once at `width.saturating_sub(4).max(1)`: two cells for the existing `│ ` gutter and two additional body-indent cells, matching the ticket and prototype. `N` is that wrapped result's length.

Derive the display, emit its one-line header, and move only the selected wrapped body rows into the final transcript (all / last six / none). Do not clone the full wrapped body or perform a second wrap for counting. Drop the unused prefix with consuming iteration. Keep the existing grapheme-aware `wrapped_rows` implementation; do not introduce a second wrapping algorithm.

For body rows, calculate the full display-byte offset before indentation. Use the existing `sources` map to recover `(actual_delta_ordinal, 3, byte_within_delta)`; constrain thinking-source grouping to the same turn just as logical grouping is. Set every visible header/body row's disclosure owner to the first ordinal. Header anchor is `(first_ordinal, 0, 0)`. Body selection must never replace its durable source ordinal with the owner ordinal.

Use `Style::default().fg(PEACH()).add_modifier(Modifier::ITALIC)` on body text and its two-space indent. Keep the guide in `SURFACE2()` without italic. Header label/disclosure uses the existing muted `SUBTEXT()` with bold label; its count uses muted regular text. Neither header span is peach or italic. Clip the header to the transcript width after its gutter; never wrap a header into multiple rows. Keep the literal `N lines` wording, including `1 lines`.

Migrate all canonical-row consumers identified by LSP:

- `operator_key`: change both Tab's visible-focus test/first-visible selection near 816 and the Up/Down/Home/End row lookup near 875 from `anchor.0` to `row.entry.ordinal`. After finding a navigation target, still assign `chat.anchor = rows[target].anchor`; selection identity must not replace the durable scroll anchor.
- `operator_scroll`, near 939.
- `operator_mouse`, near 984.
- `draw_operator_conversation`, near render line 851.
- `transcript_offset` itself.

Scroll uses `row.anchor`; hit-testing, focus, and whole-block highlighting use `row.entry.ordinal`. The transient activity row keeps both fields `None`. Preserve the existing activity-row placement and review rules.

Remove the test-only `transcript_lines` helper, which bypasses width-dependent final display. LSP found its import and three callsites near `operator.rs:2477,2508,2536`. Move those checks to actual `transcript_rows`/Dashboard output at a specified width. The escaping regression must explicitly expand an ended block and keep its original escape assertions.

### 3. Share toggling and preserve the visible header

Add `Chat::toggle_entry(&mut self, entry: render::TranscriptEntry, top: Option<render::Anchor>)`. No shared toggle exists today; Enter and mouse duplicate the set mutation.

Both input paths build the current canonical rows once, resolve the current offset, and pass the selected/hit `TranscriptEntry` plus the current top durable row anchor to this method. Identify the original message with `binary_search_by_key` on its ordinal in the already sorted `chat.messages`; an absent entry is a no-op. Expanding stores `true`. Collapsing thinking stores `false`, while collapsing a user/tool entry removes its redundant override. For thinking entries with `Some(top)`, set `follow = false`, `anchor = Some(top)`, and `scroll = 0`. If the top row is unanchored, leave all three viewport fields untouched rather than resetting to the tail. For user/tool entries, leave current viewport behavior unchanged.

Reuse the canonical rows for keyboard entry order: collect each consecutive distinct `entry.ordinal` once, skipping `None`, rather than filtering `chat.messages`. A multi-delta block is one stop; an empty block is none. Tab chooses the first visible entry only when the current one is not visible. A selected entry absent from the freshly built rows is a no-op on Enter.

Mouse targeting reads the visible row's entry, including its header, indent, or body. A later delta's body row therefore toggles the same block. Blank rows and activity rows do not toggle anything. Apply the same helper as Enter.

Rows before a visible thinking header do not change when it toggles, so retaining the top source anchor retains the header's screen row. Do not clamp anchored viewports to `rows.len() - height`, and do not re-enable follow-tail inside the toggle. Blank space below a collapsed tail block is intentional. Existing PageDown/wheel scrolling rejoins follow-tail normally.

If the header was above the viewport and collapse removes the top body anchor, the existing ordered-anchor lookup lands on that block's header at the viewport top. Expanding from a visible body retains its source anchor. No nested scroll state or inner scroll widget is added.

### 4. Exercise the real Server/worker path and expose native acceptance data

In `tests/operator_e2e.rs`, add a non-ignored `thinking_block_disclosure_through_live_server` test inside the existing `#[cfg(feature = "acceptance-diagnostics")] mod installed_pi`, beside its reusable `DashboardDriver`. Despite the module name, this new test uses `super::fixture`, `Scenario`, and `Step` with the fake Pi; it must not require an installed model/account.

Reuse the worker test's held-stream recipe: `Start`, `ThinkingStart { index: 0 }`, forty numbered thinking lines across multiple `ThinkingDelta` steps, `WaitFile(release)`, a final `ThinkingMessageEnd` containing the identical thinking plus `THINKING_ANSWER_AFTER_RELEASE` and `stop: "stop"`, then `Settle`. Create the release path in a test-owned temp directory. Wait on durable history predicates, not a fixed delay.

Open and submit through `DashboardDriver`, verify the running preview, release the worker and wait for Complete plus stop proof, and verify collapse. Enter uses `driver.dashboard.key(KeyCode::Tab)`, then `End`, then `Enter`, following the existing receipt test. Mouse uses `driver.dashboard.mouse_action(mouse, Rect::new(0, 0, 160, 50))`, with absolute column/row coordinates taken from the newly rendered buffer's thinking header/body cells; do not add another transcript offset to absolute buffer coordinates. Assert rendered expansion/collapse and unchanged header screen row. Also verify that an explicit expansion before completion survives completion in a second held turn. Keep provider transcript deduplication assertions in the existing worker suite; production worker code stays unchanged.

Add `DashboardDriver::screen_buffer(&mut self) -> ratatui::buffer::Buffer` using the same pump and 160×50 TestBackend draw as `screen`. Make `screen` derive its existing flattened string from this buffer, preserving all existing callers' output. The new test uses buffer rows for hit coordinates and same-screen-row assertions instead of guessed coordinates. Use PageDown to inspect content below that fixed viewport rather than pretending all forty expanded lines must fit.

For native acceptance, add fixture-only marker `REQUEST_THINKING_LONG_HELD` to `scenario_handler_mode`'s marker list and match. Return `Plan::new` with forty `Chunk::Thinking` fragments forming `THINKING_LINE_01` through `THINKING_LINE_40`, separated by single newlines with no final newline, then `.holding().then_text_after_release("THINKING_LONG_ANSWER")`. The same existing handler feeds the GUI provider. Leave `REQUEST_THINKING_HELD` and its existing expectations unchanged. This new marker is test data, not a product command or setting.

### 5. Review and commit the completed slice

Run `/code-review` along both required axes in parallel: repository standards and issue #449 plus this plan's confirmed UI choices. Use baseline `6d0eb9c97b3a8b378d8e17094225ba324330e02e`; before the requested commit, review `git diff <baseline> --` and all intended new files. Do not ask the user for a baseline already recorded here. Resolve blocking findings, then run the final verification below on the resulting source and retain native evidence from that reviewed checkout.

Stage intended changes only and commit on `feature/issue-449-operator-thinking-block` with subject `feat(operator): tame thinking blocks` and body `Closes #449`. Do not push, open a PR, or merge. Report the commit, exact check results and counts, intentional old-expectation changes, and remaining verification gaps. A failed or unavailable required check is never reported as passed.

## Verification

### Behavioral tests and red–green order

Drive keys/mouse through the public Dashboard and assert rendered cells/rows. Add one failing test, implement that behavior, then run its focused check before the next vertical slice.

1. **Automatic stream and completion:** in `tests/tui/operator.rs`, use `opened`, `respond`, `history_of`, `turn_message`, and `changed`. Forty numbered short lines from multiple deltas show exactly lines 35–40 plus `▸ Thinking · showing last 6 of 40 lines`. Complete removes all body rows and changes the header to `▸ Thinking · 40 lines`. `Error`/`Interrupted` summaries and an older turn while a later turn runs also use the ended default.
2. **Keyboard overrides:** Tab/End selects the thinking entry, Enter expands, Enter collapses completely, a new delta does not reopen an explicit collapse, and explicit expansion survives completion/reopening. A two-line untouched stream shows `▾ Thinking · 2 lines`; its first Enter collapses it. Up/Down traverses one entry for a many-delta block.
3. **Mouse and independence:** click a capped body row whose anchor belongs to a later delta, then a header/body row to collapse. Assert exact header-row equality before/after. Keep a second block in the same transcript and prove its display does not change. Include both two blocks in one turn and an older-turn block.
4. **Viewport stability:** exercise a header visible near the viewport bottom and expansion taller than the viewport; collapse must not snap to the tail. PageDown/wheel reaches the answer in the same transcript. Also scroll so a body's header is above the viewport, collapse by a visible body row, and verify its header lands at the top.
5. **Wrapping/style boundaries:** use public 60-column rendering with the sidebar hidden. A body row of 55 ASCII `a` characters followed by `界` wraps into a 55-character row and a second wide-glyph row at the 56-cell body width; include combining characters to keep grapheme integrity. Resizing changes `N` without creating another disclosure identity. Interior blank lines count; final newline terminators do not. Inspect body cells against active-theme `PEACH()` and `Modifier::ITALIC`, and check non-italic/non-peach header cells plus non-italic guide and answer.
6. **Empty and escaped text:** empty or newline-only thinking, including an empty prefix delta followed later by real text, adds no phantom rows or entry stops. Compare final rendered rows with the same journal without the empty block. Preserve the existing escaped OSC/control-character assertions while the body is explicitly expanded.

Put the distinct boundary/style/escaping unit cases in the existing `operator.rs` unit module, driven through its `Dashboard::new`, response, key, and draw helpers. Put interaction/state transitions in `tests/tui/operator.rs`; do not duplicate identical tables at both seams. The new live-server test from step 4 supplies the Server/worker integration proof.

Intentional existing expectation changes:

- `thinking_renders_as_a_distinct_block_before_the_answer`: replace old `│ Thinking` with `│ ▾ Thinking · 1 lines` for its short running block. Keep the existing `contains` match for the now-indented body and all thinking-before-answer ordering assertions. Do not explicitly expand this fixture: it proves that starting the answer does not end the still-Running turn's automatic short-block display.
- `thinking_text_escapes_terminal_controls`: explicitly expand the ended block before inspecting escaped content; preserve the security assertions.
- `transcript_lines` consumers migrate to canonical rows after that test-only helper is removed. No assertion may be weakened just to accommodate the helper change.

### Commands and isolation

Run from the worktree above. Use its own `target/`; clear inherited `CARGO_TARGET_DIR` rather than borrowing another checkout. For each test run, create a fresh private HOME and provider-profile directories, preserving the original `CARGO_HOME` (and `RUSTUP_HOME` only if present) for the toolchain/cache. Clear inherited `OVRCR_AGENT_SOCKET`, `OVRCR_AGENT_TOKEN`, `OVRCR_HOME`, `OVRCR_CONFIG`, `OVRCR_SOCKET`, `OVRCR_DASHBOARD_CONFIG`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, and `PI_CODING_AGENT_DIR`; point XDG config/data/cache paths into that private HOME. Never use the installed `ovrcr` or a live instance. Keep each command's log, status, count, and tested revision under a task-owned evidence directory with distinct attempt names.

During implementation:

```sh
cargo check --workspace --all-targets
cargo test -p ovrcr-tui --lib dashboard::operator::tests
cargo test --test tui operator
cargo test -p ovrcr --test operator_worker thinking
cargo test -p ovrcr --features acceptance-diagnostics --test operator_e2e thinking
```

Use the individual new test's name for each red–green iteration, then run the owning filters above. Confirm each filter executes at least one test. The end-to-end `thinking` filter must include the new non-ignored live-server test, not only provider helpers.

After review corrections, run the required final gates, with the full suite once:

```sh
just fmt-check
just lint
cargo test --workspace --all-targets --all-features
```

macOS is the current host and supports the all-features GUI build. No protocol/registry edits are planned; if an actual prerequisite requires them, stop and report the scope conflict rather than adding wire/schema changes.

### Terminal and native GUI acceptance

Use the runtime's `computer` helpers for app/window inspection, input, fresh screenshots, and accessibility evidence. Use semantic controls where exposed and pixels for the terminal canvas. Do not substitute AppleScript or a headless rendered buffer for computer use.

1. Before launching native sessions, build the helper without running it:

   ```sh
   cargo test -p ovrcr --features acceptance-diagnostics --test operator_e2e \
     --no-run --message-format=json
   ```

   Retain the `operator_e2e` test executable path from Cargo's `compiler-artifact` record with `profile.test == true`. Run that exact executable directly in later steps so no Cargo rebuild can overwrite a running terminal's binary. Complete GUI builds before starting the terminal fixture.
2. Create separate task-owned GUI and terminal roots. Set each launcher's `HOME`, XDG paths, and `TMPDIR` inside a new private `/tmp` task directory; preserve Cargo's original cache path. Keep paths short enough for a Unix socket. Clear inherited OVRCR agent/instance variables and provider profiles/credentials. Set `OVRCR_PI_EXECUTABLE=/Users/xlyk/.local/bin/pi` only for native loopback acceptance; the provider helper seeds a synthetic profile and contacts no paid provider. If that executable is unavailable or incompatible with the documented Pi 0.87.1 contract, record the prerequisite instead of installing/upgrading or using a personal profile.
3. For GUI fonts, link only the existing JetBrains Mono Nerd Font Regular/Bold/Italic/BoldItalic files into the private HOME's `Library/Fonts`. If an exact face is absent, leave it absent and report whether the existing GUI fallback renders italic; do not change system fonts.
4. Write a task-owned quota fragment containing `[quota]` and `enabled = false`. Launch `OVRCR_GUI_QUOTA_CONFIG=<fragment> rtk proxy just gui` as a named, retained command session. This builds `target/OVRCR GUI.app` from this checkout. Keep stdout/stderr and eventual exit status. Because this launcher has a unique owned `TMPDIR`, locate its single `ovrcr-gui-*` child there; never pick the newest fixture from shared system temp. Canonicalize that root and verify its registry/socket before proceeding.
5. Write that root's private mode-0600 `operator-pi-fixture-owner` with exactly `ovrcr-operator-gui-fixture v1\nroot=<canonical-root>\ntoken=issue-449-thinking\n`. Start the prebuilt helper as a second retained command:

   ```sh
   OVRCR_OPERATOR_GUI_HOME=<canonical-root> \
   OVRCR_OPERATOR_GUI_TOKEN=issue-449-thinking \
     <operator_e2e-test-executable> operator_pi_gui_provider --ignored --nocapture
   ```

   Wait for `OPERATOR_PI_GUI_READY`. Its `operator-pi/fixture-release` marker releases the held turn; `fixture-stop` is helper shutdown only.
6. In GUI fixture A, open Operator with `o` and send `REQUEST_THINKING_LONG_HELD`. Capture the automatic six-line peach/italic preview and muted header without toggling it. Release with the owned marker and capture the automatic one-line collapse plus `THINKING_LONG_ANSWER`. Now exercise Enter and clicks on header/body, verify stationary header positions, and use the one transcript scroll to reach older/latest body lines. Resize and capture fresh screenshots plus AX.
7. In a fresh GUI fixture B, repeat startup and the long request, then expand, manually collapse while running, and re-expand. Capture each state and header position. Release while explicitly expanded and verify expansion survives completion. Fresh fixtures are required because both a user's override and the helper's release latch are retained; do not attempt to clear them through product internals.
8. After both GUI launchers have ended, open Terminal.app through `computer.apps.open("com.apple.Terminal")`, create a task-owned window through its menu, and type the command `<checkout>/target/debug/ovrcr` with the same private HOME/TMPDIR/environment isolation plus `OVRCR_HOME=<owned-root> OVRCR_SOCKET=<owned-root>/server.sock`. Use a separate root named `ovrcr-gui-terminal-*`, and write quota-off `dashboard.toml` there before launch. Keep the program in the foreground and retain `dashboard exit=<status>` in the terminal or an owned receipt. Once registry/socket exist, bind the owner marker and run the prebuilt helper. Repeat scenarios A and B in separate terminal fixtures. A terminal profile lacking italic must still display peach; record that limitation rather than changing the user's profile.
9. Decline recognized optional native-hook/Bridge offers with `n`; do not grant account or OS permissions. Unknown prompts remain unanswered. Keep final screenshots/AX/logs outside fixture roots that cleanup removes. Stop each helper via its owned `fixture-stop` marker and await its result. Close only the task-owned GUI window and retain launcher cleanup stderr/status. For each terminal fixture, detach its Dashboard, run only `OVRCR_HOME=<owned-root> OVRCR_SOCKET=<owned-socket> <checkout>/target/debug/ovrcr shutdown --kill`, and verify that owned socket/processes are gone. Preserve failed-cleanup resources for recovery.

## Critical files & anchors

- `crates/ovrcr-tui/src/dashboard/operator/render.rs:627–745`, source map and `transcript_offset`: preserve byte-source identity and the intentionally unclamped anchored viewport.
- `crates/ovrcr-tui/src/dashboard/operator.rs:1234–1342`, `operator_attach`/`operator_history`: authoritative lifecycle updates already exist; do not add a second turn-completion mechanism.
- `tests/operator_worker.rs:737–810`, held thinking/deduplication test: copy its real-worker handshake and release ordering without changing runtime behavior.
- `tests/operator_e2e.rs:760–998,2668–2865`, `DashboardDriver` and GUI helper: reuse public Dashboard/socket flow and private ownership validation.
- `src/gui.rs:70–117,353–408`, `Demo` creation/cleanup: the native launcher owns its fixture and removes it only after successful shutdown. GUI rendering already honors italics at `src/bin/ovrcr-gui.rs:1024–1051`.

## Assumptions & contingencies

- User-confirmed choices: a manual collapse during streaming hides the body; short streams use `▾ Thinking · N lines`; headers are muted and non-italic. These take precedence over differing prototype controls.
- Keep the current branch/worktree. If execution starts on a different branch or with unrelated edits, preserve them and restore this task's worktree context before changing files; do not reset the user's work.
- The CLI has no new command, flag, setting, or key binding in this slice. Its help remains intentionally unchanged; the exact behavior-documentation targets are named beside the display contract.
- If native tools, the Pi executable, or desktop permissions become unavailable, finish reachable code and automated verification, retain the exact attempted command/error, and report the affected mandatory acceptance as not run. Never substitute a personal account, live instance, fixture mock screenshot, or another checkout.
- Review and commit are authorized; pushing, PR creation, merge, provider purchases, account changes, and OS permission grants are not.

