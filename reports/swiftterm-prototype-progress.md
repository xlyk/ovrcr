# SwiftTerm prototype progress

**November 13, 2026. Keep SwiftTerm as the leading candidate.** One native Mac window ran the existing Rust Dashboard, rendered two live panes, and let a terminal Dashboard reconnect to the same shells after the app quit. Three input and accessibility checks remain before choosing the engine for the shipping app. [Original acceptance criteria](../docs/superpowers/plans/2026-11-13-swiftterm-prototype.md)

## What worked in the native window

| Question | Observed result |
|---|---|
| Does the layout follow the window and font? | Cmd-plus changed the shell’s measured size from 39×85 to 37×71 rows and columns. Dragging the window corner changed it from 39×85 to 29×60. The shell rendered colored text, box borders, CJK characters, accents, and emoji. [Zoom](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/zoom-shell.png) · [Window resize](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/resize-shell.png) |
| Can I edit while another pane is busy? | With a second shell printing output, multiline paste stayed in the Quick draft. Shift-Return inserted a newline, Option-E then E composed `é`, and Return submitted the draft once. Ctrl-C interrupted a shell command and the next command ran. [Quick draft](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/quick-during-output.png) · [Submitted entry](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/quick-submit.png) |
| Do both copy workflows work? | Dashboard Copy sent `CLIP_OK` to the pasteboard. In a separate check, Shift-double-click selected a word in SwiftTerm and Cmd-C copied it. Cmd-V pasted the expected text into Quick in both cases. Page Up opened frozen History. [Dashboard Copy](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/copy-roundtrip.png) · [Native selection](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/native-selection.png) |
| Can assistive software read the shell? | The first build exposed only window controls. The revised host publishes visible rows on demand as a labelled AppKit text area. The accessibility value contained `NATIVE_AX_OK` after the shell printed it. [Original accessibility tree](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/zoom-shell-ax.txt) · [Revised tree](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/accessible-shell-ax.txt) |
| Who owns the sessions when the window goes away? | Cmd-W hid the window and reopening preserved a shell variable. In the lifecycle test, a second Dashboard was refused while the app was attached. Quitting reaped the Dashboard child, both shell PIDs and run IDs survived, a real CLI Dashboard attached and detached, and the app reopened on the same Server. Teardown removed the fixture. [Lifecycle transcript](https://github.com/xlyk/ovrcr/blob/1c26b31700b2809d9f7e237834f25369dcfa370c/experiments/swiftterm/evidence/lifecycle.txt) |

## Font settings

Open **SwiftTermPrototype → Settings…** (⌘,) to choose an installed monospaced font and a size from 8 to 32 pt. **Apply** reflows the current Dashboard and saves both preferences for future launches. The default on the test Mac was SF Mono at 13 pt. Cmd-plus and Cmd-minus zoom temporarily, and Cmd-0 restores the configured size. [Prototype instructions](https://github.com/xlyk/ovrcr/blob/46c2cd79c497cc80c6f99b1d404ee5febb6de8dd/experiments/swiftterm/README.md)

On the test Mac, applying JetBrainsMono Nerd Font Mono at 18 pt reflowed the same running shell. Zoom Out followed by Cmd-0 restored its grid. After quitting and reopening the app, Settings showed the same family and size. The focused Swift check also covered fallback to System Monospaced when the saved family was unavailable. [Settings after relaunch](https://github.com/xlyk/ovrcr/blob/46c2cd79c497cc80c6f99b1d404ee5febb6de8dd/experiments/swiftterm/evidence/font-settings.jpg) · [Accessibility values](https://github.com/xlyk/ovrcr/blob/46c2cd79c497cc80c6f99b1d404ee5febb6de8dd/experiments/swiftterm/evidence/font-settings-ax.txt) · [Font settings check](https://github.com/xlyk/ovrcr/blob/46c2cd79c497cc80c6f99b1d404ee5febb6de8dd/experiments/swiftterm/host/Tests/SwiftTermPrototypeTests/TerminalFontSettingsTests.swift)

## Reproduce the build

From a clone of this repository, open the tested revision in a separate worktree:

```sh
git fetch origin codex/swiftterm-prototype
git worktree add --detach ../ovrcr-swiftterm-try 46c2cd79c497cc80c6f99b1d404ee5febb6de8dd
cd ../ovrcr-swiftterm-try
just swiftterm-prototype
```

The launcher builds the Mac app and starts a private Server and disposable shell sessions. After Cmd-Q, type `reopen` in the launcher to return to the fixture or `teardown` to remove it. The recorded run used arm64 macOS 26.5.2 and SwiftTerm `d4f52faaae413b2e69c728ad79cd225cdbebc042`. Standalone Command Line Tools Swift 6.3.3 built that pin. Xcode’s Swift 6.2.4 stopped in SwiftTerm’s Powerline renderer. [Prototype runbook](https://github.com/xlyk/ovrcr/blob/46c2cd79c497cc80c6f99b1d404ee5febb6de8dd/experiments/swiftterm/README.md)

## What decides adoption

1. **Japanese or Korean composition.** Compose mid-line in Quick and a shell while the second pane redraws. Confirm the candidate window follows the caret, adjacent text stays visible, cancel restores the original line, and commit inserts the phrase exactly once. The walkthrough used the U.S. layout.
2. **Physical key repeat.** Hold and release a key on a real keyboard under the Dashboard’s existing keyboard protocol. Check that repeats arrive and stop on release. The automated hold produced one character.
3. **Spoken VoiceOver traversal.** Focus the terminal with VoiceOver, navigate its contents, and read back a known line after output or a resize. The recorded accessibility value establishes that the text is available to assistive software.

Notification delivery and activation, release packaging, and responsiveness with 50 live sessions belong to the subsequent product milestone. [Full terminal comparison and acceptance matrix](<OVRCR embedded terminal options.md>)
