# Antigravity CLI acceptance

The native macOS run used Antigravity CLI 1.5.9 in a disposable HOME and Git
workspace. The planning reference was 1.5.5. A Gemini-compatible loopback
endpoint supplied synthetic responses. Onboarding used the native UI with
optional interaction-data collection turned off.

| Check | Evidence |
| --- | --- |
| Setup | [Connect preview](01-connect.jpg) showed both Antigravity files. Accepting it preserved the existing observer and composed the foreground renderer. |
| Model and prompt | The [picker](02-model-picker.jpg) showed the ID and display label returned by `agy models`. Starting [the form](03-prompt.jpg) sent the first prompt to Gemini 3.8 Flash (Low). The [native terminal](04-native-turn.jpg) showed the answer. |
| Completed turn | [Working](05-working.jpg) preceded [Ready · unread reply](06-ready-unread.jpg). The foreground callback, successful Stop and subsequent idle named the same UUID. |
| Interruption | [The interrupted turn](07-interrupted.jpg) returned to Idle. Its previous unread response kept revision 7. |
| Continued Stop | A second native Stop handler returned `continue` once. Execution 0 emitted a successful-looking Stop, followed by `working`. Execution 1 emitted its own successful Stop followed by `idle`. |
| Recovery | [Reopen](08-exact-reopen.jpg) shows the exact UUID in the launch command, native title, prior messages and the new reply. Row 13 advanced from run 1 to run 2. The endpoint returned `AGY_NATIVE_CONTINUITY` only when the request contained the previous marker. |
| Startup before workspace trust | The CLI completed the supplied prompt behind its trust screen. Foreground status stayed `initializing` after the Stop. The row acquired no unread response from that sequence. |

Each JPEG has an adjacent accessibility snapshot with the same basename.
[Sanitized observations](native-probe.json) record callback order and Server state.
Antigravity supplied an incorrect transcript path in its foreground payload.
The adapter verified the reported UUID against the `cascade_id` in the profile’s
per-conversation SQLite file.

A second installed Antigravity process attached to row 14 in the same checkout.
Reopening row 13 again advanced it to run 3. Row 14 stayed attached to its own
UUID and original run.

The Rust integration suite also runs two managed PTYs in one checkout with scripted
native callbacks, including a pinned title on the surviving row. Installed
Antigravity and the native Dashboard were exercised on macOS. Linux CI uses
the fixture provider for platform regression coverage.
