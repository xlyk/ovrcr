# Quota display: native GUI evidence (item 3 PR 2)

Tested revision: `c0be446` on `feature/quota-display` (the code commit; later
commits add only this directory and docs). Date: 2026-10-02, macOS, `just gui`.

## Harness

Codex CUA failed before it could act. `codex exec` with the configured model
`gpt-6.1-sol` returned "model is not supported when using Codex with a ChatGPT
account", with or without the `OVRCR_*` variables (`codex-cua-attempt.txt`
has the commands, `codex login status` and environment). As instructed, this
pass used the scripted System Events harness from
`research/settings-write-2026-10-02`, extended:

- `drive.py`: real key events to the `ovrcr-gui` process by PID. Each key is sent
  in the same AppleScript call that checks the process is frontmost.
  `size:` resizes the window, `ctrl:` sends a control key, and `shot:` saves the
  window screenshot (`screencapture -l`, `winid.swift`) and the accessibility
  static text (`pN-*.txt`).
- Native data: `cua-quota-fragment.toml` (via `OVRCR_GUI_QUOTA_CONFIG`) starts with
  `quota.enabled = false`. It points Codex at `codex`, which wraps `fake-native.py`.
  That script is a JSON-RPC stand-in after the fixture in
  `tests/provider_quota.rs`, reporting 63% used (37% left) on 5h and 31% on 7d.
  It reads a `mode` file to answer slowly (`slow:N`), at once (`ok`), or with an
  HTTP 503 whose body must never appear in a reason (`http503`). `methods.log`
  records every native call with its mode. Grok points at a missing path.

A second agent's GUI fixture (`feature/settings-editor`) was running at the same
time, and both processes are named `ovrcr-gui`. One of my early palette attempts
(`:enable codex`, Return) may have reached that window during a focus race: no
request reached my Server. After that, `drive.py` checked focus in the same call
as each key. The fixture's own tree is unchanged in every capture.

## Steps and outcomes

| Step | Input | Observed (verbatim from the `.txt`) | Result |
| --- | --- | --- | --- |
| p1 | attach, `quota.enabled = false` | `Quota left`, `Claude — checking`, `Codex usage off`, `Grok usage off` | PASS (off, Claude Checking) |
| p2 | `u` | `Subscription allowance only.` once; Claude `reason: waiting for a managed Claude session's first response`; Codex and Grok `Codex/Grok usage off: set `quota.enabled = true` in dashboard.toml`; `next check: not scheduled` | PASS (details, off sentence) |
| p3 | Escape, `:`, `enable codex` | `› Enable Codex and Grok usage` | PASS |
| p4 | Return (mode `slow:20`) | the document now has `enabled = true` (`p4-dashboard.toml-after-enable.txt`); the 20 s fake answer hit the Server's 20 s deadline: `Codex — unavailable  retry 1m`, `Grok — unavailable  retry 10m` | PASS (Enable, failed with retry) |
| p4b | `u` | Codex `reason: timed out after 20 s`, `next check: 2026-10-02 13:40:08 (in 1m)`; Grok `reason: grok not found at the configured path`, `next check: 2026-10-02 13:48:48 (in 10m)` | PASS (reason, local next check) |
| p5 | CLI `ovrcr settings set quota.enabled false` | `Codex usage off`, `Grok usage off` | PASS |
| p6a, p6 | `:`, `enable codex`, Return (mode `slow:10`) | 1.5 s later: `Codex — checking`, `Grok — unavailable  retry 10m` | PASS (Checking) |
| p7 | 12 s later | `Codex  5h [███░░░░░░░] 37%`, `       7d [██████░░░░] 69%` | PASS (Current) |
| p8a, p8 | mode `http503`; `:`, `refresh quota`, Return | `methods.log` shows a read at once; `Codex  5h [██░░░░░] 37% left  stale <1m`, `7d … 69% left  stale <1m`; no 503 body anywhere | PASS (Refresh, stale with age) |
| p9 | `:`, `refresh quota`, Return within 30 s | footer `ERROR: Quota refresh cooling down: 3s left`; the block shows `stale 1m` | PASS (cooldown) |
| p10 | resize to 100 px tall | the helper keeps 22 rows (min inner size 320 px), so the window cannot reach the ladder | see p11 |
| p11 | in the fixture's root shell, `sh l.sh 9 8 7 6 5` (`nested-ladder.sh`): an isolated Server with `nested-dashboard.toml` and the real Dashboard after `stty rows N`, `q` between sizes | rows 9 and 8: three lines (`Claude — checking`, `Codex 5h 37%`, `Grok — unavailable  retry 10m`); rows 7 and 6: `Quota: u`; rows 5: no quota line | PASS (three, one, zero) |
| p12 | `q` | nested Server log ends `ovrcr server stopped` | PASS |

The nested Dashboard's footer shows its own boot warning about Claude reporting
hooks; that is unrelated environment state.

## Cleanup

The window was closed through its AX close button, and the `just gui` launcher
exited 0 (`gui-launcher.log`). The fixture root
(`$TMPDIR/ovrcr-gui-ZSOVrX`, which also held the nested root) no longer exists.
All twelve recorded session process groups return `ProcessLookupError`
(`gui-pgids.txt`, `gui-cleanup.txt`).
