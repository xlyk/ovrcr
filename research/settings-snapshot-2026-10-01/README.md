# Settings snapshot: native GUI evidence (PR 2 of the shared-settings work)

Tested revision: `1314a5741ad107e9837546a045c15b2d54bf9dd5` on `feature/settings-snapshot`
(code identical to the pushed head; later commits rebase onto PR 1's docs-only
commits and add this directory). Date: 2026-10-02, macOS, `just gui`.

## Harness

Codex CUA was not available: every `codex exec` call failed with "model is not
supported when using Codex with a ChatGPT account" for the configured model and
for the models tried with `-m`. This pass drives the real helper window instead:

- input: real key events through System Events (`keystroke`, `key code`) to the
  frontmost `ovrcr-gui` process (`drive.py`);
- screen: `screencapture -l <window id>` of that process's window (`winid.swift`),
  taken after raising it;
- accessibility: the static-text rows the helper publishes under
  `group 1 of window 1`, saved verbatim as `pN-*.txt`.

The fixture was launched with `OVRCR_GUI_QUOTA_CONFIG=cua-quota-fragment.toml`, which
the helper appends to the fixture's `dashboard.toml`, giving two findings
(`ready_sound = "yes"`, `mystery = 1`). `ovrcr settings` against the fixture reported
the same two findings before the pass.

## Steps and outcomes

| Step | Input | Observed (verbatim from the `.txt`) | Result |
| --- | --- | --- | --- |
| p1 | attach | footer `2 settings findings; see Settings` | PASS |
| p2 | Space, `v` | View group lists `,  Settings` | PASS |
| p3 | `,` | popup `Settings · read-only · Esc close · ↑↓ scroll`; `Document:` path of the fixture; `Findings: 2`; `line 3, ready_sound: invalid type: string "yes", expected a boolean`, `line 4, mystery: unknown setting; ignored` above the 13 setting rows; off-state lines for `desktop_notifications`, `title_model`, `quota.enabled` | PASS |
| — | arrows to scroll | not exercised: every row fit in the window | NOT RUN |
| p5 | Escape | popup gone | PASS |
| p6 | `,` in Browse | no popup, no menu | PASS |
| p7a, p7 | `:`, `settings`, Return | palette row `› Settings`; same popup with `Findings: 2` | PASS |
| p8 | Escape, `S` | footer `Ready sound: on`; the fixture document now reads `ready_sound = true` with `mystery = 1` and `automatic_local_terminals = "on"` kept (`p8-dashboard.toml-after-toggle.txt`) | PASS |
| p9 | `q` | `Dashboard exited: Success`, **Restart dashboard** button | PASS |
| p10 | AX press of Restart dashboard | footer `1 settings finding; see Settings` | PASS |
| p11 | Space, `v`, `,` | `Findings: 1`, `line 4, mystery: unknown setting; ignored`, `ready_sound = true  (Dashboard, document)` | PASS |
| p12 | Escape | Browse footer | PASS |

p10 and p11 show the Server re-reading the document at the new hello: the toggle
written to the published path is confirmed by the next snapshot, and the finding
count change produced a new footer line.

## Cleanup

The window was closed through its AX close button; the `just gui` launcher exited 0.
The fixture root no longer exists and all twelve recorded session process groups
return `ProcessLookupError` (`gui-pgids.txt`, `gui-cleanup.txt`).
