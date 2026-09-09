# UX pass 8: command search recovery

Reviewed clean branch codex/ux-pass-08 at 0c6a29aca34fae71545f72a24e2ebcfaf8717eb5 in /Users/xlyk/Code/ovrcr/.worktrees/codex-ux-ten-passes. This commit contains review evidence only.

No correction was indicated by the observed search states. An unmatched query displayed an explicit empty state; Ctrl-u cleared it. Searching scope quality returned both the workspace local terminal (#4) and review terminal (#10). The selected result retained its project and workspace context after resizing. Return selected #10 and displayed its fixture transcript. Return on a later empty result kept the palette open; Escape closed it.

| State | Screenshot | Accessibility |
| --- | --- | --- |
| 01-empty-search | [JPEG](01-empty-search.jpg) | [AX](01-empty-search.txt) |
| 02-matching-session | [JPEG](02-matching-session.jpg) | [AX](02-matching-session.txt) |
| 03-narrow-selected-result | [JPEG](03-narrow-selected-result.jpg) | [AX](03-narrow-selected-result.txt) |
| 04-selected-session | [JPEG](04-selected-session.jpg) | [AX](04-selected-session.txt) |
| 05-empty-return-retained | [JPEG](05-empty-return-retained.jpg) | [AX](05-empty-return-retained.txt) |

Source inspection followed palette_entries, the Search renderer and the Command::Switch event branch: matching uses terminal context and selection dispatches the chosen session ID. All 56 existing palette tests passed (107 other TUI tests filtered), including input capture, exact session selection, empty/shrinking results, and narrow identity/context checks. Raw command/revision/exit evidence is /private/tmp/ovrcr-ux-ten-passes/08/focused-1.log. No source change or broad Rust rerun was needed. Report links and Markdown whitespace pass; raw AX whitespace is retained.

Native launcher exited 0. Cleanup verified 13 owned PIDs, 12 groups, root and socket absent; evidence is in /private/tmp/ovrcr-ux-ten-passes/08/before-owned.json and before-cleanup.txt. The transcript is a local fixture, not a real provider; no terminal command was entered. Linux rendering and input forwarding are outside this slice. Incidental shell completion warnings were not altered. Next slice: compact help and nested action discovery.
