# SwiftTerm prototype

This disposable macOS app tests whether SwiftTerm can host OVRCR’s existing Dashboard with comfortable native typing, selection, and zoom. It runs real local shells in a private fixture. One AppKit view hosts the Rust Dashboard in its own PTY. The separate Rust Server owns the sessions.

From this checkout on a Mac with Rust, Swift, and the project’s development tools, run:

```sh
just swiftterm-prototype
```

Start with a shell selected. Press Return to type into it and Ctrl-G to return to Browse. Use Ctrl-O in Browse to open an Operator Quick draft. Shift-Return adds a line. Use local fixture text for clipboard checks.

Shift-double-click a word to select it in SwiftTerm, then press Cmd-C. Option-drag selects a block of cells. An ordinary click goes to the Dashboard. For Rust’s own Copy mode, press `[` in Browse, move to the first cell, press `v`, extend the selection, and press `y`. Cmd-V pastes into the focused Dashboard control.

After Cmd-Q, the launcher accepts `reopen` to attach the app to the same fixture or `teardown` to stop the private Server and remove its workspace. Closing runner stdin also tears down the fixture. The `SWIFTERM_FIXTURE_READY=` JSON record includes the private socket, bundle, CLI, and process identities for a terminal reattachment.

The runner honors `DEVELOPER_DIR`; when unset, it selects `/Library/Developer/CommandLineTools` if present, then falls back to the active `xcrun` toolchain. The upstream dependency compiled here with the standalone Command Line Tools’ Swift 6.3.3. Xcode’s Swift 6.2.4 failed compiling the upstream `PowerlineRenderer.swift`. Both attempts used revision `d4f52faaae413b2e69c728ad79cd225cdbebc042`.

| Try it | Look for |
|---|---|
| Run `stty size`, change the font size, and run it again. | The grid reflows and the shell reports its new rows and columns. Cmd-0 restores the original font size. |
| Press `v` in Browse to split a second pane. Run `for n in 1 2 3 4 5; do echo "$n"; sleep 1; done` in one shell while editing in the other. | The input stays in the selected pane while the other pane redraws. |
| Open Quick and compose mid-line using a Japanese or Korean input source. Cancel once, then commit once. | The marked text and candidate window follow the caret. The adjacent text stays visible and the committed phrase appears exactly once. |
| Use Cmd-W during shell output, reopen the window from the Dock, then quit with Cmd-Q. | Reopening the hidden window preserves its Dashboard. After quit, another Dashboard can attach to the runner’s fixture and use the same shells. |

## Observed on macOS 26.5.2, arm64

**Hold engine adoption.** One native window ran the production Rust Dashboard from `56ef76943`, including two visible shell panes. The live evidence supports pursuing this host. Comfortable long-form input, selection and assistive reading still need a hands-on pass.

| Path | Result |
|---|---|
| Shell, font and layout | Cmd-plus changed the selected shell’s `stty size` from 39×85 to 37×71. Ctrl-C interrupted `sleep`, and the next command printed `INT_OK`. The fixture rendered CJK, an accented character, emoji, and box borders. Cmd-0 restored the original font and the Dashboard split into two panes. [Zoomed shell](evidence/zoom-shell.png) |
| Input while another pane redraws | A multiline paste stayed in Quick. Shift-Return inserted a third line, and U.S. Option-E followed by E composed `é`. Return submitted one visible entry. The isolated Operator then reported that Pi was unavailable. [Draft during shell output](evidence/quick-during-output.png) · [Submitted entry](evidence/quick-submit.png) |
| Dashboard Copy and History | Copied `CLIP_OK` from Dashboard Copy with `y`, then pasted that exact token into Quick using Cmd-V. Page Up opened the second shell’s frozen History. [Clipboard round trip](evidence/copy-roundtrip.png) · [History](evidence/history-key.png) |
| Quit and reconnect | The normal launch command passed a real PTY lifecycle test. A second Dashboard was refused, quitting reaped the app’s Dashboard, both shell PIDs and run IDs survived, a CLI Dashboard attached, and the app reopened on the same Server. Explicit teardown removed the fixture. Repeat with `python3 -I experiments/swiftterm/tests/lifecycle.py`. [Transcript](evidence/lifecycle.txt) |

The first window’s [accessibility tree](evidence/zoom-shell-ax.txt) exposed only the window controls. The final build exposes the visible terminal rows on demand as a focused, labelled AppKit text area. A fresh shell printed `NATIVE_AX_OK` in red and the same text appeared in AX. Shift-double-click, Cmd-C and Cmd-V copied it into Quick. Cmd-W hid the window. Reopening preserved a shell variable, and dragging the window corner changed the inner PTY from 39×85 to 29×60. [Live AX text](evidence/accessible-shell-ax.txt) · [Shell rendering](evidence/accessible-shell.png) · [Native selection round trip](evidence/native-selection.png) · [Restored and resized shell](evidence/resize-shell.png)

The Mac had only the U.S. keyboard layout enabled. Japanese/Korean marked text and candidate placement during redraw, physical key repeat and release, and spoken VoiceOver traversal remain open. A held-key event from computer use produced one character, so it could not establish physical keyboard repeat.
