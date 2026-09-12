# How to test OVRCR with computer use

Use this macOS smoke check after changes to the dashboard, terminal input, or CLI session controls. Run it in the checkout being reviewed. It complements the automated tests.

## 1. Launch and connect

```sh
rtk proxy just gui
```

Keep the command session open to collect its exit status and cleanup errors. The helper builds `target/OVRCR GUI.app` and creates a disposable server with two projects, four workspaces, and ten real shell sessions.

With the Codex computer-use tool, initialize in a separate call using the actual checkout path:

```js
var app = await cua.getApp("/absolute/checkout/target/OVRCR GUI.app");
```

Read the returned tool documentation before continuing. Then inspect the window:

```js
await app.getAXStateAndScreenshot();
```

Use fresh accessibility indices or coordinates from the current screenshot. Sidebar row accessibility bounds can span the terminal: click within the visible sidebar or use `j`/`k`. Request the needed execution permission if local socket, PTY, or GUI access is sandbox-blocked; record a blocked check if access remains unavailable.

## 2. Prove real input and rendering

Select `consigint / auth-handoff / local`, then press Return to enter terminal mode. Paste this command through the GUI and press Return:

```sh
rtk proxy printf 'CUA_%s\n' INPUT_OK
```

Confirm a separate output line containing `CUA_INPUT_OK`; the echoed command alone is insufficient. Inspect both the screenshot and accessibility text for readable rows, correct selection, and overlapping or clipped content. Resize the window and check the layout again. `Ctrl-g` returns to browse mode; `j`/`k` select sessions and Return resumes terminal input.

Check Unicode in the same shell:

```sh
rtk proxy printf '\033[31mRED 界🙂 END\033[0m\n'
```

Confirm readable red ASCII/CJK and a visible emoji, not replacement squares. The macOS helper uses native fallback artwork for missing glyphs, including color emoji. Compare the screenshot with accessibility text; correct text alone does not prove rendering. Repeat after opening a split with `Ctrl-g`, `v`, then Enter, after resizing, and after reattachment. Check wide-glyph spacing, cursor position, and clipping at the pane boundary.

Check modified, composed, and dead keys in the same shell. Put the PTY in raw
mode so the line editor cannot consume the escapes:

```sh
rtk proxy stty -icanon -echo min 1
rtk proxy cat -v
```

Press Ctrl-Left, Shift-Enter, Alt-Backspace, and Option-b. Confirm `^[[1;5D`,
`^[[13;2u`, `^[^?`, and `^[b`; Option-b must print the meta form, never the
composed glyph.

Then press Option-e, a US-layout dead key, and record exactly what arrives. This
is an open question, not a known expectation: the helper publishes an IME area, so
winit's macOS `keyDown:` runs `interpretKeyEvents` first, and a dead key
(Option-e, `i`, `u`, `n`, or backtick) calls `setMarkedText:` and suppresses the
`KeyboardInput` event, meaning no `Key::E` reaches the helper. So Option-e may
start a composition (no bytes, an accent pending) where Option-b sends `^[b`.
Report which happens, and whether the following key commits an accented character
into the shell. If dead keys compose, Meta for those five keys needs a decision:
either the composition is correct for this helper or `option_as_alt` must be set.
Press Ctrl-C, then restore the shell:

```sh
rtk proxy stty sane
```

At the restored prompt, select a CJK input method and type a composition (with
Pinyin, `ce` then Space). Confirm only the committed characters reach the command
line and that no pre-edit letters leak into it. Check the candidate window's
position against the screenshot: it must sit at the cursor cell, not merely
somewhere over the terminal, because macOS places that panel from the single rect
the helper publishes. Record the input method used.

**IME composition: unverified as of 2026-09-08.** This machine enables only the
U.S. keyboard layout and the Character Palette (`defaults read
com.apple.HIToolbox AppleEnabledInputSources`), and no desktop computer-use tool
was available to drive the native window, so the composition, dead-key, and
candidate-window checks above were not run against the helper. The modifier and
meta encodings they exercise are covered by the unit tests
`special_keys_carry_modifiers`, `alt_letter_encodes_meta`, and
`unmodified_letter_keeps_its_text` in `src/gui/input.rs`; an automated egui test
cannot prove composition or panel placement, so the IME path still needs this
native check.

Return to browse with `Ctrl-g`. Press Space. Confirm a compact bottom-right
list above the footer, with the surrounding dashboard still visible. Press
`t` and confirm the terminal submenu names the selected session and shows
`Space t`. Press Backspace and confirm the group list returns. Press Escape,
then `?`, then Enter to open the Terminal group. Confirm Pause is listed for a
running session and Resume is absent. Disabled actions should show a reason.
Press Backspace, then `w`, then `x`; confirm the workspace picker highlights
the selected `project / workspace` and excludes `root`. Type to filter to a
different workspace, then press Enter; confirmation must name the chosen target.
Escape cancels. Repeat with `Space p x` for projects, then `Space t x` for direct
terminal-close confirmation, cancelling both. Open tasks with `Ctrl-t`, press
`n`, and Tab to Project; check the same filtering, Up/Down, and Tab/Enter controls.
Escape cancels the editor, then Escape returns to the dashboard. Click project and workspace sidebar rows and
confirm `Space` shows only their applicable groups. Resize the window and scroll to
the last entry; confirm it remains visible and clickable. Press Escape.
Press Space then `w` then `n`; confirm the key popup closes and the terminal form opens.
On Agent, move to `shell` and press Tab. Confirm Workspace shows the selected `project / workspace` and Name is filled. Enter through the remaining fields. Confirm terminal mode on the new session, then paste this command and press Return:

```sh
rtk proxy printf 'CUA_%s\n' N_SHELL_OK
```

Confirm a separate output line containing `CUA_N_SHELL_OK`; the echoed command alone is insufficient.

Return to browse with `Ctrl-g`, press `w`, and type `cua-workspace`. Confirm
Project matches the selected session, Branch reads `feature/cua-workspace`,
and Base shows the repository's default branch once suggestions finish loading.
Press Enter from Name. Confirm the new workspace's `local` shell is selected
in terminal mode. Paste and execute:

```sh
rtk proxy printf 'CUA_%s\n' W_SHELL_OK
```

Confirm a separate `CUA_W_SHELL_OK` output line. Record this shell's PID and
process group with the other fixture processes before cleanup. Return to
`consigint / auth-handoff / local` for the checks below.

Return to browse with `Ctrl-g`. Press `a`. Tab through a configured `picker_roots` directory to the demo repository (a `git` marker sorts it first). Confirm Name is the repository basename and Workspace root is `<config dir>/workspaces/<name>`. Escape without submitting if this run should not register a project.

## 3. Mouse forwarding and wheel history

### Dashboard controls

Run these checks with the mouse for navigation; use the keyboard only to enter
text. Save a screenshot and accessibility text after each changed view. Repeat
the form and menu checks in a short window to exercise scrolling and clipping.

1. Open the action menu with its visible control and open a second terminal pane.
   Click each pane and run `rtk proxy printf 'MOUSE_%s\n' LEFT_OK` or
   `rtk proxy printf 'MOUSE_%s\n' RIGHT_OK`. Confirm distinct output in the
   intended pane. Switch directly from an active terminal without `Ctrl-g`.
2. Drag the sidebar border and confirm the pane narrows and widens with it.
   Drag the divider left and right. In each pane run `rtk proxy stty size` and
   compare the reported columns with the drawn terminal width. Make the window
   too narrow for both panes, widen it again, and confirm assignments and the
   chosen split survive. Release the drag outside the terminal and confirm the
   next click works normally.
3. Click a project or workspace name to select it without collapsing it. Click
   the first cell of a project header, or the branch glyph of a workspace, to
   collapse and expand it. Scroll the sidebar and select a row that was
   initially offscreen. Confirm the selected target by its name.
4. Open a creation form from the visible action menu. Click a nonactive field,
   edit in the middle of its value, choose a pick-list option, and operate any
   toggle. Browse a directory through the path picker. Use Cancel, reopen, and
   submit a disposable terminal; prove input with a separate `MOUSE_FORM_OK`
   output line. Cancel a destructive confirmation and confirm its target remains.
5. Open Tasks using mouse controls. Select and scroll its rows, open an editor,
   click fields and a project choice, and cancel. Exercise task controls only on
   a task owned by this fixture; inspect confirmation targets before proceeding.
6. Print a line containing `ASCII 界🙂 tail`, open Copy from the action menu, drag
   across the text, and use Copy. Paste into an owned fixture input and compare
   the selected text. Open History, scroll, select text, and close it through its
   visible control. Confirm new live output does not rewrite the frozen view.
7. While a form or menu is open, click outside it over a terminal. Confirm the
   click does not reach the terminal. Repeat a focus-changing click with Vim
   mouse tracking enabled: the click selects the other pane without moving that
   application's cursor. A subsequent click inside the focused Vim pane should
   reach Vim as described below.

Keep these observations separate from automated test results. If the computer-use
tool cannot perform a drag or inspect the clipboard, mark that check unverified
and retain any available automated evidence. Selection highlighting and emitted
OSC52 bytes do not prove clipboard delivery; test paste in a terminal that handles
OSC52 when the GUI helper cannot deliver it.

### Application forwarding

Stay on `consigint / auth-handoff / local` in terminal mode. Move the pointer over the terminal pane, not the sidebar or footer, and scroll the wheel up. History should open at the tail (`HISTORY` in the footer). Wheel down at the newest row returns to the live prompt.

In the same or another live pane, run `vim -n -u NONE`, then `:set mouse=a`. Click in the buffer: the Vim cursor moves to that cell. Wheel should scroll Vim; History must stay closed. `:set mouse=` then wheel up while Vim is still running must not open History (alternate screen). Quit Vim before repeating the plain-shell History check.

Computer-use tools often cannot hold a mouse button across Ctrl-g. Skip that gesture here; the TUI test `ctrl_g_releases_held_buttons_before_switching` covers it.

## 4. Exercise CLI controls through the GUI

First inspect each inherited path separately; macOS `printenv` accepts one name:

```sh
rtk proxy printenv OVRCR_CONFIG
rtk proxy printenv OVRCR_SOCKET
```

Confirm both paths belong to this launch's disposable directory, ending in `config.toml` and `server.sock`. Stop if either is missing or points elsewhere. Do not try a config-only CLI command: config selection does not select the server socket.

In that same shell, replace the binary path below with this checkout's absolute path. Paste and execute each line, inspecting its result before continuing. At each terminal list, save the PIDs and use host process inspection to record their process-group IDs while they are running:

```sh
c=/absolute/checkout/target/debug/ovrcr
rtk proxy "$c" project get consigint --json
rtk proxy "$c" workspace get --project consigint --name auth-handoff --json
rtk proxy "$c" terminal list --json
t=$(rtk proxy "$c" terminal create --project consigint --workspace auth-handoff --name cua-check -- /bin/sh)
rtk proxy "$c" terminal list --json
rtk proxy "$c" terminal send "$t" --text "printf 'CUA_%s\n' BACKGROUND_OK"
rtk proxy "$c" terminal read "$t" --json
```

Confirm the new sidebar row and `CUA_BACKGROUND_OK` in the read result. The original terminal must remain selected. If output has not arrived, read again after observing progress. Reads contain the current screen only.

Use `Ctrl-g`, select `cua-check`, and press Return. Confirm the marker in its visible screen and record its PID. Detach with `Ctrl-g`, then `q`; click **Restart dashboard**, reselect `cua-check`, and confirm the same PID and output.

Return to the original `local` shell, where `$c` and `$t` are still set:

```sh
rtk proxy "$c" terminal close "$t" --json
```

Wait for `{"ok":true}` and confirm the sidebar row disappears. Closing a shell can take several seconds.

## 5. Verify cleanup and report evidence

Use the demo paths and process-group IDs recorded above. Close the native window through computer use and wait for the launch command to exit successfully; cleanup can take about a minute.

After closing, use process/filesystem inventory. Do not query the closed app through app-specific CUA state methods: they can relaunch it and create another fixture. If that happens, record the new owned processes and paths, close that fixture too, and report its cleanup separately.

Verify the demo directory and socket are gone. For each recorded process group, `os.kill(-pgid, 0)` must raise `ProcessLookupError`; a permission error does not prove cleanup. Keep the fixture and report its path if cleanup fails.

Report the tested commit, GUI checks and observed markers, cleanup result, and any blocked checks. Keep automated test results separate from computer-use evidence. A successful build or screenshot alone does not prove terminal input or lifecycle behavior.
