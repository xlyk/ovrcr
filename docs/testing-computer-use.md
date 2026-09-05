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

## 3. Exercise CLI controls through the GUI

In that same shell, replace the binary path below with this checkout's absolute path. The shell already has the disposable `OVRCR_CONFIG` and `OVRCR_SOCKET` values. Paste and execute each line, inspecting its result before continuing. At each terminal list, save the PIDs and use host process inspection to record their process-group IDs while they are running:

```sh
c=/absolute/checkout/target/debug/ovrcr
rtk proxy printenv OVRCR_CONFIG OVRCR_SOCKET
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

## 4. Verify cleanup and report evidence

Use the demo paths and process-group IDs recorded above. Close the native window through computer use and wait for the launch command to exit successfully; cleanup can take about a minute.

Verify the demo directory and socket are gone. For each recorded process group, `os.kill(-pgid, 0)` must raise `ProcessLookupError`; a permission error does not prove cleanup. Keep the fixture and report its path if cleanup fails.

Report the tested commit, GUI checks and observed markers, cleanup result, and any blocked checks. Keep automated test results separate from computer-use evidence. A successful build or screenshot alone does not prove terminal input or lifecycle behavior.
