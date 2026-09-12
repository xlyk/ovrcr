# Which-Key Popup and Start Screen Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Every dashboard hotkey is discoverable: Space or `?` opens a popup that lists the keys available right now, grouped, each with a description that names its target and states its consequence; the same table feeds the palette and footer, and an empty dashboard shows a start screen instead of a blank pane.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- One hint table is the single source of truth for the popup, the palette's secondary text, and the footer.
- Descriptions name the selected target and state the consequence; disabled keys stay listed with the reason.
- Space is a leader (next key runs the entry); `?` opens a browsable popup usable by mouse; bare hotkeys keep working.

**2026-09-08 execution review:** Current main already uses `x` to close a split
pane without stopping its session. Kyle approved keeping `x` and using `X`
for session-close confirmation instead of the `x` references below. Space
becomes the leader in Copy/History; `v` remains the anchor key. Current main
also has split-pane controls and detected-agent forms, which the hint table
must preserve. Native computer-use acceptance remains required separately
from headless tests.

**Dependencies:** Depends on `plans/2026-09-08-detected-agents.md` only for the `n` entry's target; the table can ship first with the keys that exist today.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

---

### Task 1: Which-key popup with descriptions, and a start screen



**Files:**
- Create: `crates/ovrcr-tui/src/dashboard/hints.rs`
- Create: `crates/ovrcr-tui/src/dashboard/whichkey.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/render.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/mod.rs`
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui.rs`

**Design:**
- `hints.rs`: one function, `pub fn key_hints(dashboard: &Dashboard) -> Vec<HintGroup>`, builds the whole table from current state. `HintGroup { title, hints: Vec<KeyHint> }` and `KeyHint { key: &'static str, name: &'static str, description: String, enabled: bool, action: HintAction }`. This table is the single source of truth: the popup renders it, the palette shows each entry's key and description as secondary text, and the footer prints a compressed line of the enabled keys.
- **Description rules**, which are the acceptance criteria:
  1. Name the target. The description substitutes the selected session, workspace, or project name, so `w` reads "Create a worktree and branch under consigint and start its local shell" and `x` reads "Stop agent (#12) in auth-handoff and remove its record. Asks for confirmation."
  2. State the consequence. Destructive or side-effecting keys say what happens and whether a confirmation follows. Detach says "the server and every session keep running".
  3. Disabled keys stay listed, dimmed, with the reason in place of the description: "Resume: not paused", "Close: no session selected".
- **Groups from browse mode:** Create (`n`, `w`, `a`), Session (`Enter`, `p`, `r`, `x`, `[`, `PageUp`), View (`t`, `:`, `o` once the operator exists, `?`), Dashboard (`q`). Copy and history modes get their own tables listing `hjkl`, `0`, `$`, `g`, `G`, `v`, `y`, and the exits. Terminal mode has no popup; `Ctrl-g` is its only chord and the footer already names it.
- **Leader and help keys.** Space in browse, copy, or history mode opens the popup; the next key runs that entry and closes it, so `Space n` and bare `n` are the same action. `?` opens the same popup in a browsing state where arrows move and Enter runs the highlighted entry, which also makes it usable from the GUI helper by mouse. Escape closes. `whichkey.rs` holds the popup state (`pending_leader: bool`, `browsing: Option<usize>`) and its key handling; `key_action` in `state.rs` consults it first while it is open.
- **Rendering.** A centered popup, borrowing the palette's `Clear` and block layout, with one column per group when the width allows, otherwise stacked. Each row is key, name, description. On a pane narrower than 100 columns the rows show key and name only, and the highlighted row's description moves to a detail line at the bottom, so nothing is truncated silently. `x` is a new browse key that opens the existing close confirmation for the selected session; it exists so the Session group has a close entry.
- **Start screen.** When the hierarchy has no projects, the terminal pane shows a short list instead of an empty screen: `a` register project, `:` palette, `?` help, and the config and socket paths. When a workspace with no sessions is selected by mouse, the pane shows "press n to start a terminal here". Both are drawn from the same hint table so their wording matches the popup.

- [x] **Step 1: RED tests**
  1. `hints` unit: with a running session selected in `consigint / auth-handoff`, the Create group's `w` description contains `consigint`, the Session group's `x` description contains the session name and id and the word "confirmation", `r` is disabled with reason "not paused"; with nothing selected the Session group's entries are disabled with "no session selected".
  2. `tests/tui.rs`: `space_then_n_opens_the_terminal_form`: press Space, assert the popup is drawn with the Create group, press `n`, assert the popup is gone and the terminal form is open.
  3. `tests/tui.rs`: `question_mark_popup_is_browsable`: press `?`, Down twice, Enter, assert the third enabled entry's action ran.
  4. `tests/tui.rs`: `narrow_popup_moves_description_to_detail_line` on an 80-column `TestBackend`: descriptions are absent from the rows and the highlighted one appears on the last popup line.
  5. `tests/tui.rs`: `empty_hierarchy_shows_start_screen` and `palette_entries_show_keys_and_descriptions`.

- [x] **Step 2: Implement** `hints.rs`, `whichkey.rs`, the `x` key, the palette's secondary text, the compressed footer line, and the start screen.

- [x] **Step 3: Verify** `rtk proxy cargo test -p ovrcr-tui` and `rtk proxy cargo test --test tui --test terminal_acceptance`.

**Gate:** new tests pass; existing browse-mode key tests pass unchanged, since bare hotkeys keep working without the leader.

---

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [x] **Step 1:** Document Space and `?` in the README "Command palette" section, including the description rules so future keys follow them, and add a smoke-check step to `docs/testing-computer-use.md`: press `?`, confirm the popup names the selected session in its Session group, press Escape.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Execution checkpoint — 2026-09-08

- Implemented on `codex/which-key-popup` in `/private/tmp/ovrcr-which-key`, based
  on `6da699f868d729efdc59b2837ba5baf71ee8c72c`. Main and other worktrees remain
  untouched. Implementation and documentation are separate local commits;
  nothing has been pushed or merged.
- Six initial behavioral tests failed before implementation. Supplemental
  assertions cover hint targets/reasons, disabled-row dimming, the last narrow
  detail line, wide-layout mouse hit testing, tiny layouts, overlay input,
  Copy capture identity, empty-workspace form defaults, and real config/socket
  paths. Cursor placement, leader Ctrl-g, and the tasks alias each received
  a failing regression before correction. Copy/History fixtures now use `v`;
  their clipboard, input, geometry, and lifecycle assertions remain.
- `rtk proxy just verify` passed, including all-feature Clippy with warnings
  denied. `rtk proxy cargo test --workspace --all-targets --all-features`
  passed on the final code. The TUI suite has 116 passing tests; the TUI crate
  has 23. Terminal acceptance has eight passing tests and one existing ignored
  test. The six GUI-feature tests passed; these are headless evidence.
- One all-feature attempt timed out in the unchanged server fixture's
  `split_server_two_streams_resize_resync_and_detach` cleanup (`session
  SessionId(1) did not exit`). Its isolated rerun and a full rerun passed
  without changing server code. The failed attempt is retained, not replaced.
- Logs, commands, exit statuses, and tested diffs are in
  `/private/tmp/ovrcr-which-key-evidence/`. Initial red, compilation, fixture
  migration, lint, and server-cleanup failures are preserved separately.
- **Still unverified:** native release-build computer use, native clipboard
  paste, Linux execution, the literal interactive README transcript, and
  verification from a clean merged checkout. The README transcript is
  unchanged. No independent reviewer was available in this session; the code
  received a self-review. These acceptance gaps are not covered by passing
  headless tests.

## Conflict-resolution follow-up — 2026-09-08

- Merged incoming `origin/main` at `c392059` into the PR branch. Kept both the
  which-key table and Git-suggestion module, routed `w` and `a` through the
  incoming creation entry points, and retained contextual palette details plus
  the workspace form's submit/toggle instructions. `X` still opens the existing
  close confirmation with the new palette defaults.
- Added coverage proving leader `w` requests repository inspection and keeps
  Name-first/derived-branch behavior. A failing regression exposed late Inspect
  errors after palette hint actions; those responses are now ignored just as
  they are for palette session switching. Updated old registration-label
  assertions to the new path-picker fields without dropping form checks.
- `rtk proxy env RUST_TEST_THREADS=1 just verify`: passed, 413 tests passed and
  six existing ignored, including all-feature Clippy with warnings denied.
- `rtk proxy env RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features`:
  passed, 424 tests passed and six existing ignored. TUI integration: 124;
  TUI crate: 28; terminal acceptance: nine passed and one ignored. Serialization
  follows the incoming workspace plan's recorded timing-test limitation.
- Evidence and preserved failures: `/private/tmp/ovrcr-which-key-conflict-evidence/`.
  Native/clipboard/Linux and independent-review gaps above remain open. This
  integrates main into the feature branch, not PR #34 into main.

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. Press `?` with a session selected and confirm every Session entry names that session; press Space then `n` and confirm the terminal form opens.
2. Run the computer-use smoke check in `docs/testing-computer-use.md`.
