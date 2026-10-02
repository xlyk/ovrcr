# Settings editor: native GUI evidence (item 2 PR B)

Branch `feature/settings-editor`, macOS, `just gui`, 2026-10-02. Run 1 used
commit `e83cc6d`; run 2 used `1b6e72a`, which adds only line wrapping for
refusals, findings and descriptions, and repeats the rejected-value step on the
final rendering.

## Harness

The coordinator asked for Codex single-shot (`codex exec`) with the configured
model `gpt-6.1-sol`. It is refused for this account (`codex-probe.txt`):

```sh
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN \
  codex exec -m gpt-6.1-sol --skip-git-repo-check "Reply with the single word READY."
# ERROR: ... "The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account."
```

`codex login status`: `Logged in using ChatGPT`; `codex-cli 0.155.1`;
`CODEX_HOME` unset; `~/.codex/config.toml` sets `model = "gpt-6.1-sol"`. The
only `OVRCR_*` variables in this shell are `OVRCR_AGENT_SOCKET` and
`OVRCR_AGENT_TOKEN`, which were unset for every command. That is the same
failure wave 2 reported, so the evidence uses the scripted System Events
harness from `research/settings-write-2026-10-02`: real key events through
System Events to the `ovrcr-gui` process (`drive.py`), window screenshots with
`screencapture -l <window id>` (`winid.swift`), and the helper's accessibility
static text saved as `*.txt`. `drive.py` refuses to type unless the GUI process
is frontmost. Changes from the wave 2 copy: `"` is typed through AppleScript's
`quote`, and the focus check waits up to 24 s. `run-step.sh` runs a step only
after any other worktree's `drive.py` has been idle for 15 s, then saves a
screenshot, the AX text and the settings document as `<step>-dashboard.toml.txt`.

Fixture documents: run 1 appended `fixture-fragment.toml` (`branch_prefix`, one
`[[agents]]`) to the demo's `automatic_local_terminals = "on"`
(`p0-dashboard.toml.txt`); run 2 appended `fixture-fragment-run2.toml`
(`title_model`). `gui-launcher-attempt1-fragment-path.log` is a failed first
launch: `OVRCR_GUI_QUOTA_CONFIG` takes a file path, not TOML text.

## Run 1 steps

| Step | Input | Observed | Result |
| --- | --- | --- | --- |
| p0 | attach | Browse footer | PASS |
| p1 | Space, `v`, `,` | editor opens: Alerts, Workspaces, Titles, Usage, Agents, Remembered launches; `Branch prefix  kh/  set  (default: feature/)`; consent sentences under Desktop notifications, Title model and Codex and Grok usage; `claude  ["claude"]` child row with `+ Add agent`, `+ Add root`, `+ Add project` | PASS |
| p2 toggle | `/ready`, Enter, Enter | `Ready sound  on  set  (default: off)`, no confirmation; document gains `ready_sound = true` | PASS |
| p3a/b enum | `/automatic`, Enter, Enter, Down, Enter | pick list `› on / off / default_branch_only` in place (p3a); then `off  set  (default: default_branch_only)` and `automatic_local_terminals = "off"` | PASS |
| p4a/b string | `/branch prefix`, Enter, Enter, 3×Backspace, `fx/`, Enter | in-place field `[fx/▏]` (p4a); then `fx/  set  (default: feature/)`, `branch_prefix = "fx/"` | PASS |
| p5a/b collection element | `/agents claude`, Enter, Enter, Backspace×N, `["claude", "--verbose"]`, Enter | TOML array field with hint (p5a); then `claude  ["claude", "--verbose"]`, document `argv = ["claude", "--verbose"]` | PASS |
| p6 reset | `/branch prefix`, Enter, `r` | `Branch prefix  feature/  default`; the key is gone from the document | PASS |
| p7a/b rejected value | `/title model`, Enter, Enter, `no-slash`, Enter | `refused: could not save title_model: "no-slash" is not provider/model; …` under the row, row still `unset  default`; document byte-identical to p6 (`cmp`) | PASS (clipped at the popup edge; fixed in run 2) |
| p8 | Escape ×2 | filter cleared, then the editor closed | PASS |

## Run 2 steps (final build `1b6e72a`)

| Step | Input | Observed | Result |
| --- | --- | --- | --- |
| r2-p2 rejected value | menu Space `v` `,`, `/title model`, Enter, Enter, Backspace×25, `no-slash`, Enter | row keeps `anthropic/claude-haiku  set  (default: unset)`; the refusal wraps onto two lines; document byte-identical to `r2-p0` | PASS |
| r2-p3 | Escape ×2 | closed | PASS |
| r2-p4 | `,` in Browse | no Settings view (no Browse key) | PASS |

## Focus contention with a parallel worktree

The quota-display worktree drove its own `ovrcr-gui` through System Events at
the same time. Several steps were refused by the focus check before typing and
were retried; no refused step left partial input. Once, before `run-step.sh`
existed, a `:` from the other driver arrived in this window's filter field (the
other driver checked focus, then this one took focus before its keystroke
landed). It changed only the filter text, which was cleared before the next
step. This harness can cause the same race in the other direction, so its
`run-step.sh` waits for the other driver to go idle. Whether any keystroke from
this run reached the other window cannot be verified from here; a full-screen
capture taken during the contention showed only that window's own shell input.

## Cleanup

Both windows were closed with their close buttons. Each `just gui` launcher
exited 0 (`gui-launcher-run1.log`, `gui-launcher-run2.log`), each fixture root
no longer exists, and no process from this checkout's bundle remains.
