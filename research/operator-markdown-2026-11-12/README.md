# Operator Markdown replies: native GUI check, 2026-11-12

Source: worktree `codex/operator-markdown`, parent `dfefa83e7`, with the
Markdown presentation change in the commit containing this directory. The
worktree stayed clean except for the intended change and this evidence.

The GUI was built from this checkout with `gui`, launched with `just gui`'s
script (`scripts/gui.sh`) against a task-owned HOME and a disposable
`ovrcr-gui-*` root. `OVRCR_PI_EXECUTABLE` selected installed Pi 0.87.1, quota
probes were off (`logs/quota-off.toml`), and the Operator model came from the
`operator_pi_gui_provider` loopback provider. No personal instance, credentials,
provider account, or installed executable was used. `REQUEST_MARKDOWN` in that
provider returns headings, emphasis, inline code, a table, a list, a fenced code
block, and a link.

CUA used Codex CLI (`codex-cli 0.164.0`) through its `cua_repl` tool. No
AppleScript or System Events was used. The prompts and tool logs are in `logs/`.

## Attempt 01 — raw sidebar preview

`native/attempt-01-raw-sidebar/` captures the reply after the main feed began
rendering Markdown. The body had no `##`, `**`, fenced backticks, or `|---|`,
but the one-line sidebar preview still contained `##` and `**`. The Codex report
records that gap, and the screenshots are retained as the failed first attempt.

## Attempt 02 — fixed preview

After the sidebar preview was changed to render the same Markdown to plain text,
the GUI was relaunched against a fresh disposable root and the same request was
sent again. `native/attempt-02-fixed/` contains:

| File | Contents |
| --- | --- |
| `01-operator-open.jpg` / `.ax.txt` | Actual GUI window and full accessibility text after opening Operator |
| `02-markdown-reply.jpg` / `.ax.txt` | Actual GUI window and full accessibility text after the reply completed |
| `02-markdown-reply.json` | Capture call, timestamps, screenshot byte count and SHA-256, AX SHA-256, polling, and validation |
| `cua-report.txt` | Codex's final observation and cleanup report |

The final capture is a real 2200×1864 JPEG (292,390 bytes, SHA-256
`be1685fba88903bae5772ca5c629412ab28842ddfdac82fa7cf5ee6c6f2ac853`). It shows the
heading `Operator report`, the bold “Ready” prose, a boxed Area/Result table,
the first/second-item list, and the teal `cargo test -p ovrcr-tui` code line.
The sidebar row reads `↳ “Operator report Ready with cargo te…` with no source
markers. The JSON's `validation.sidebarContainsRawMarkdown` is `false`, and all
four body markers are `false`.

## Cleanup

Both attempts closed through the native GUI. The GUI launcher and provider
helper exited 0 (`logs/attempt-02-gui-launcher.log`,
`logs/attempt-02-provider.log`; the provider printed `root_exists=false`). The
recorded fixture roots are absent, and CUA/process inventory found no remaining
`OVRCR GUI.app` process or window. No personal or shared instance was touched.

## Not covered here

This pass covers one wide native layout and the markdown constructs listed
above. It does not re-check narrow/floor layouts, scrolling, other themes, links
with long destinations, or a live provider. Those remain covered by the
Dashboard frame snapshots/unit tests and the existing provider acceptance
suites, not by this screenshot.
