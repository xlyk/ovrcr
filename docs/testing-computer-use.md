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

Return to browse with `Ctrl-g`. Press `n`. On Agent, move to `shell` and press Tab. Confirm Workspace shows the selected `project / workspace` and Name is filled. Enter through the remaining fields. Confirm terminal mode on the new session, then paste this command and press Return:

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
