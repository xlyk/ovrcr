# Which-Key Popup and Start Screen Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Every dashboard hotkey is discoverable: Space or `?` opens a popup that lists the keys available right now, grouped, each with a description that names its target and states its consequence; the same table feeds the palette and footer, and an empty dashboard shows a start screen instead of a blank pane.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- One hint table is the single source of truth for the popup, the palette's secondary text, and the footer.
- Descriptions name the selected target and state the consequence; disabled keys stay listed with the reason.
- Space is a leader (next key runs the entry); `?` opens a browsable popup usable by mouse; bare hotkeys keep working.

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

- [ ] **Step 1: RED tests**
  1. `hints` unit: with a running session selected in `consigint / auth-handoff`, the Create group's `w` description contains `consigint`, the Session group's `x` description contains the session name and id and the word "confirmation", `r` is disabled with reason "not paused"; with nothing selected the Session group's entries are disabled with "no session selected".
  2. `tests/tui.rs`: `space_then_n_opens_the_terminal_form`: press Space, assert the popup is drawn with the Create group, press `n`, assert the popup is gone and the terminal form is open.
  3. `tests/tui.rs`: `question_mark_popup_is_browsable`: press `?`, Down twice, Enter, assert the third enabled entry's action ran.
  4. `tests/tui.rs`: `narrow_popup_moves_description_to_detail_line` on an 80-column `TestBackend`: descriptions are absent from the rows and the highlighted one appears on the last popup line.
  5. `tests/tui.rs`: `empty_hierarchy_shows_start_screen` and `palette_entries_show_keys_and_descriptions`.

- [ ] **Step 2: Implement** `hints.rs`, `whichkey.rs`, the `x` key, the palette's secondary text, the compressed footer line, and the start screen.

- [ ] **Step 3: Verify** `rtk proxy cargo test -p ovrcr-tui` and `rtk proxy cargo test --test tui --test terminal_acceptance`.

**Gate:** new tests pass; existing browse-mode key tests pass unchanged, since bare hotkeys keep working without the leader.

---

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Document Space and `?` in the README "Command palette" section, including the description rules so future keys follow them, and add a smoke-check step to `docs/testing-computer-use.md`: press `?`, confirm the popup names the selected session in its Session group, press Escape.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. Press `?` with a session selected and confirm every Session entry names that session; press Space then `n` and confirm the terminal form opens.
2. Run the computer-use smoke check in `docs/testing-computer-use.md`.
