# SwiftTerm as OVRCR's embedded terminal

**Snapshot date: 2026-11-13.** The latest non-prerelease release found was SwiftTerm `v1.99.3`, published 2026-11-06. I inspected upstream `main` at `d4f52faaae413b2e69c728ad79cd225cdbebc042` (2026-11-12 21:45 UTC), 26 commits beyond that release. Claims about newer fixes below are main-only unless stated otherwise; the proposed app was not built or exercised. ([v1.99.3 release](https://github.com/migueldeicaza/SwiftTerm/releases/tag/v1.99.3), [release-to-main comparison](https://github.com/migueldeicaza/SwiftTerm/compare/v1.99.3...main), [inspected main commit](https://github.com/migueldeicaza/SwiftTerm/commit/d4f52faaae413b2e69c728ad79cd225cdbebc042))

## Can SwiftTerm host OVRCR's existing Rust TUI without changing its owner model?

### Takeaway

Yes, the integration shape fits: SwiftTerm can be the outer terminal emulator while the existing Crossterm/Ratatui dashboard continues to write and read ordinary terminal bytes through a PTY. This preserves the Rust dashboard and server as owners, but the OVRCR byte-stream, resize, process, and app-lifecycle bridge still needs implementation and a real-app acceptance pass.

### Cited Findings

- SwiftTerm describes `TerminalView` as an AppKit `NSView` for an arbitrary source implemented through `TerminalViewDelegate`; it separately offers `LocalProcessTerminalView` to connect that view to a local Unix process and PTY. ([SwiftTerm README at inspected main](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/README.md), [AppKit TerminalView source](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacTerminalView.swift), [delegate API](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Apple/TerminalViewDelegate.swift))
- The view exposes the expected host boundary: feed terminal output into its emulator, receive user bytes through `send`, and receive column/row resize notifications through its delegate. SwiftUI can host the AppKit view with `NSViewRepresentable`. ([MacTerminalView.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacTerminalView.swift), [TerminalViewDelegate.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Apple/TerminalViewDelegate.swift), [Apple NSViewRepresentable](https://developer.apple.com/documentation/swiftui/nsviewrepresentable))
- `LocalProcessTerminalView.startProcess` accepts an executable, arguments, environment, argv[0], and working directory; nil environment uses SwiftTerm's terminal defaults, including `TERM=xterm-256color`. Its API also owns the child process/PTY connection and forwards lifecycle/size events to a process delegate. ([MacLocalTerminalView.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacLocalTerminalView.swift), [LocalProcess.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/LocalProcess.swift))
- Both `v1.99.3` and inspected `main` declare Swift tools 6.2 and a macOS 11 package minimum. The repository is MIT licensed. The manifest also lists swift-png, swift-argument-parser, and swift-docc-plugin; PNG/LZ77 are target dependencies only under portable-graphics non-Apple platforms, and ArgumentParser is used by the separate Termcast executable. ([v1.99.3 Package.swift](https://github.com/migueldeicaza/SwiftTerm/blob/v1.99.3/Package.swift), [main Package.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Package.swift), [MIT license](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/LICENSE))
- OVRCR's current terminal guard pushes Kitty keyboard flags with `CSI >15u` and restores them with `CSI <1u`; the runtime contract still assigns PTYs and live screens to the Rust server. ([OVRCR terminal guard](../../crates/ovrcr-tui/src/dashboard/terminal_guard.rs), [OVRCR runtime guide](../../docs/development/runtime.md))

### Inferences

- The least disruptive architecture is an AppKit `TerminalView` fed from the existing dashboard PTY, with user bytes and size changes sent back through a bounded adapter. This avoids giving SwiftTerm or a second child server authority over sessions. `LocalProcessTerminalView` is also viable if it launches exactly one bundled OVRCR dashboard client that attaches to the single Rust server; it is not itself an OVRCR server/session integration.
- If the Swift app is sandboxed, test actual child execution and filesystem/workspace access: SwiftTerm's local-process documentation warns that a sandboxed app restricts what the child shell can access. ([MacLocalTerminalView.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacLocalTerminalView.swift))

### Gaps

- No OVRCR process was run inside SwiftTerm, and no `.app` bundle was launched. Still unproved: embedded executable lookup, cwd/environment parity, terminal resize timing, startup ordering, one-dashboard admission, close-to-hide behavior, Cmd-Q notification shutdown, and persistence of Rust sessions across UI exit.
- The custom `TerminalViewDelegate` adapter's threading/backpressure and socket-write behavior must be checked in the real application; SwiftTerm's own local-process adapter does not establish that OVRCR's socket path is safe.

## Does it preserve the terminal features and input contract OVRCR needs?

### Takeaway

The current OVRCR Kitty mask is supported by both the latest release and inspected main, including its push/pop stack. SwiftTerm also preserves committed IME text without flag 16 by sending it as raw UTF-8, which OVRCR's Crossterm 0.29 input path can consume; however, the release predates a macOS held-key-repeat fix, and the specific OVRCR key matrix still needs an end-to-end PTY run.

### Cited Findings

- SwiftTerm assigns Kitty flag 4 to alternate-key reporting, flag 8 to all-key reporting, and flag 16 to associated-text reporting. Flag 16 is distinct from alternate key values, and its encoder includes associated text only when both all-key flag 8 and text flag 16 are set. The protocol tests exercise push/pop restoration and state changes; the AppKit view tests exercise changes reaching the view. ([KittyKeyboardProtocol.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/KittyKeyboardProtocol.swift), [KittyKeyboardEncoder.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/KittyKeyboardEncoder.swift), [Kitty protocol tests](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Tests/SwiftTermTests/KittyKeyboardProtocolTests.swift), [view tests](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Tests/SwiftTermTests/TerminalViewKeyboardEnhancementTests.swift))
- OVRCR's `15` mask consists of disambiguation, press/repeat/release reporting, alternate key values, and all-key reporting; it deliberately omits associated text flag 16. SwiftTerm's `v1.99.3` encoder test already expects raw UTF-8 for key-less committed text with all-key mode but without flag 16. Current main additionally tests AppKit `insertText` for a committed Korean IME string under flags 10 (event types + all keys, no flag 16), expecting the committed text once and no physical-key release. ([OVRCR terminal guard](../../crates/ovrcr-tui/src/dashboard/terminal_guard.rs), [release encoder test](https://github.com/migueldeicaza/SwiftTerm/blob/v1.99.3/Tests/SwiftTermTests/KittyKeyboardEncoderTests.swift#L113), [main AppKit IME test](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Tests/SwiftTermTests/KittyKeyboardAppKitReproductionTests.swift#L631), [crossterm 0.29 Unix parser](https://docs.rs/crate/crossterm/0.29.0/source/src/event/sys/unix/parse.rs))
- Crossterm's Unix parser turns raw UTF-8 input into `KeyCode::Char`; OVRCR's Operator inserts `KeyCode::Char` into its draft. Crossterm has no associated-text field for Kitty flag 16, so adding flag 16 to the OVRCR push is not a safe substitute for the raw-UTF-8 path. ([crossterm 0.29 parser](https://docs.rs/crate/crossterm/0.29.0/source/src/event/sys/unix/parse.rs), [OVRCR Operator input](../../crates/ovrcr-tui/src/dashboard/operator.rs#L1041), [crossterm 0.29 event types](https://docs.rs/crate/crossterm/0.29.0/source/src/event.rs))
- SwiftTerm's AppKit router feeds Return/Enter through `doCommand`, carrying the pending NSEvent modifiers into the Kitty encoder. An encoder test expects Shift+Enter as `CSI 13;2u`; the same modifier encoding implies Ctrl+Enter's Control modifier, but I found no OVRCR runtime test for either binding through SwiftTerm. ([MacKeyboardRouter.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacKeyboardRouter.swift), [Shift+Enter encoder test](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Tests/SwiftTermTests/KittyKeyboardEncoderTests.swift#L216))
- SwiftTerm's `performKeyEquivalent` only sends Command keys to the terminal when the host callback explicitly approves them; otherwise the host menu gets the shortcut. This gives the macOS app a path to keep Cmd shortcuts native while leaving Ctrl/terminal input to the TUI. ([MacKeyboardRouter.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacKeyboardRouter.swift))
- The `v1.99.3` release predates main commit `faf7ce8d` (2026-11-09), which fixes macOS held-text-key repeats being intercepted by the press-and-hold accent menu. The released version has Kitty flags and encoder support, but this repeat fix is main-only as of the inspected source date. ([v1.99.3 release](https://github.com/migueldeicaza/SwiftTerm/releases/tag/v1.99.3), [repeat fix commit](https://github.com/migueldeicaza/SwiftTerm/commit/faf7ce8d757343fbacc7e165f40c6751b8229a78))
- SwiftTerm documents truecolor, 256 colors, Unicode/emoji/combining characters, terminal resize, selection/search, mouse events, and hyperlinks; its terminal source handles alternate screen, bracketed-paste mode 2004, and synchronized output 2026. ([README](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/README.md), [Terminal.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Terminal.swift))

### Inferences

- Under OVRCR's existing flags 15, committed IME text should survive as raw UTF-8 through SwiftTerm and arrive at OVRCR as ordinary `KeyCode::Char`; this is supported by upstream unit tests and local source inspection, but has not been observed through an OVRCR PTY in a running `.app`.
- SwiftTerm is not a behavior-preserving Ratatui adapter by itself: it is a separate VT emulator. Common Crossterm sequences are covered, but an agent or nested TUI using uncommon private sequences remains a compatibility risk.

### Gaps

- Verify through the actual Swift app and OVRCR PTY: mask 15 push/pop, shifted punctuation and alternate keys, physical press/repeat/release, Ctrl/Shift+Enter, Command menu ownership, focus changes, mouse reports, bracketed paste, OSC52, resize, alternate screen, truecolor, and synchronized output.
- The keyboard API tests separate parser flags, encoder output, and some AppKit paths; they do not prove OVRCR's complete `crossterm::event::read()` + dashboard input behavior. Do not add flag 16 without changing and verifying the receiver contract.

## Does the AppKit view provide the required native input/rendering quality?

### Takeaway

SwiftTerm provides a genuinely native macOS view and text-input-client implementation, with an optional Metal renderer and a Core Graphics/CoreText path. That is a credible route to native feel, but current upstream issues expose two macOS IME overlay display defects; VoiceOver completeness and ligatures are not established by the sources checked.

### Cited Findings

- The macOS `TerminalView` subclasses `NSView`; SwiftTerm implements `NSTextInputClient` marked-text methods for AppKit composition. Its Option-as-Meta setting defaults to true, with an option to let macOS compose Option-based characters. ([MacTerminalView.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacTerminalView.swift), [Apple NSTextInputClient](https://developer.apple.com/documentation/appkit/nstextinputclient))
- Metal rendering is optional and disabled by default; `setUseMetal(true)` creates a Metal/MTKView renderer and can throw if device/renderer setup fails. The README calls the Mac renderer CoreText-based and lists Metal as optional GPU acceleration. ([MacTerminalView.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Mac/MacTerminalView.swift), [SwiftTerm README](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/README.md))
- Standard OSC 52 reads/writes are routed through terminal delegate callbacks. `TerminalView`'s default clipboard callbacks are no-ops/deny-by-absence, so the host must wire macOS pasteboard access. Selection-based copy is a separate built-in view interaction. OSC 8 links are supported and can be handled by the host's link callback. ([Terminal.swift OSC 52 handling](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Terminal.swift), [TerminalViewDelegate.swift](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/Sources/SwiftTerm/Apple/TerminalViewDelegate.swift), [README](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/README.md))
- Two upstream macOS IME issues remain open. #825 reports that during composition with a mid-line caret, cells to the right go blank until commit; it describes an overlay background fill, not lost input. #826 reports that after asynchronous output advances the caret, the marked-text overlay remains at the old position and can cover committed text until another keystroke; it also reports a display/placement defect, not lost committed text. Both issues say observed on `1.20.0` and main at `ac430fa`; I did not reproduce them on `v1.99.3`. ([issue #825](https://github.com/migueldeicaza/SwiftTerm/issues/825), [issue #826](https://github.com/migueldeicaza/SwiftTerm/issues/826))
- Main's performance guide includes headless and actual-view harnesses, but explicitly says its deterministic Swift adaptations are not byte-for-byte Ghostty workloads and absolute times must not be compared. It reports one workload-specific 26% `ghostty_osc` feed-time improvement after a parser change, and cautions that this does not predict a general gain or UI/PTy latency. ([PERFORMANCE.md](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/PERFORMANCE.md))

### Inferences

- Native AppKit event routing, pasteboard, font, cursor, and resize hooks should let OVRCR keep system Cmd shortcuts while using the Rust dashboard for terminal commands. The visual cell renderer is custom terminal UI, not an `NSTextView`; native view hosting alone does not guarantee native text layout, full VoiceOver support, or ligatures.
- The IME issue reports target the exact hard case OVRCR cares about: a fullscreen terminal UI or tmux-style delayed echo moving the caret while composition continues. Treat visual composition as a release gate even though upstream does not report input loss.
- The benchmark evidence supports that SwiftTerm has active performance engineering, not that SwiftTerm is as fast as Ghostty in a real OVRCR workload.

### Gaps

- No macOS CUA/VoiceOver run, pasteboard run, IME run, or Metal/Core Graphics comparison was performed. The reports are upstream issue evidence, not a reproduction on the proposed OVRCR build.
- No source-backed guarantee was found for macOS ligature behavior or complete macOS VoiceOver traversal. Verify if either is a ship requirement; test emoji, fallback fonts, combining marks, CJK width, and bidirectional text with OVRCR's actual fonts/theme.

## Is SwiftTerm preferable to Ghostty for this embedding boundary?

### Takeaway

SwiftTerm is the lower-risk fit for embedding an unchanged Rust dashboard because it exposes a public AppKit view connected to arbitrary byte sources. Ghostty may offer a more mature full terminal stack and native Metal renderer, but upstream labels its full embedder header internal and directs external embedders toward the lower-level `libghostty-vt` API, whose signatures are still changing.

### Cited Findings

- Ghostty's own `ghostty.h` calls the full embedder API internal, says its only consumer is the macOS app, and says it is not designed for external use; the README directs external embedders to `libghostty-vt`. Ghostty 1.4 release notes say the `libghostty-vt` API remains work-in-progress and has broken compatibility several times. ([Ghostty embedder header](https://github.com/ghostty-org/ghostty/blob/main/include/ghostty.h), [Ghostty README](https://github.com/ghostty-org/ghostty/blob/main/README.md), [Ghostty 1.4 release notes](https://ghostty.org/docs/install/release-notes/1-4-0))
- Ghostty documents a shared Zig core, macOS Metal/CoreText rendering, and separate read/write/render threads. This is evidence of its architecture and platform investment, not an apples-to-apples speed result for OVRCR. ([Ghostty README](https://github.com/ghostty-org/ghostty/blob/main/README.md))
- SwiftTerm publishes its Swift source package and public `TerminalViewDelegate`/`LocalProcessTerminalView` APIs under MIT; the external C API status for Ghostty is different from SwiftTerm's directly embeddable NSView. ([SwiftTerm README](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/README.md), [SwiftTerm LICENSE](https://github.com/migueldeicaza/SwiftTerm/blob/d4f52faaae413b2e69c728ad79cd225cdbebc042/LICENSE), [Ghostty header](https://github.com/ghostty-org/ghostty/blob/main/include/ghostty.h))

### Inferences

- **Conditional go for SwiftTerm:** it appears to satisfy the architectural objective—leave Rust/Crossterm/Ratatui intact and place a native AppKit terminal emulator around its PTY. Gate adoption on a small product-real proof using the chosen dependency version, especially the released-version key-repeat behavior and live IME overlay defects.
- If exact Ghostty emulation is a stronger requirement than a direct public AppKit wrapper, investigate the cost of Ghostty's internal API/fork or `libghostty-vt` plus a custom renderer. Neither route is a drop-in SwiftUI terminal view according to current upstream API guidance.

### Gaps

- No equal-hardware, equal-stream SwiftTerm-versus-Ghostty benchmark was found or run. SwiftTerm's benchmark guide forbids comparing its adapted workload's absolute numbers with Ghostty's.
- Before go/no-go, run the actual bundled OVRCR `.app` with a real production server and dashboard PTY; exercise close/hide, Cmd-Q, one-dashboard ownership and 50 sessions; verify the key/protocol matrix above; use native IME composition while dashboard output moves the caret; test OSC52 plus selection copy/paste, hyperlinks, resize, focus, accessibility, Unicode/fallback rendering; and record responsiveness with Core Graphics and optional Metal. This research pass installed, built, or changed no product code.
