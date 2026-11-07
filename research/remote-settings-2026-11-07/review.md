# Remote settings: UI evidence

Tested revision: `4dc64676` on `work/mobile-remote-web-mvp`, 2026-11-07.
Capture harness: a disposable `ovrcr` TUI in a PTY (120×40), isolated
`OVRCR_HOME`, no user sessions.

## Finding

The Settings view renders the new **Remote** group last:

```
Remote
  Remote web server      off  default   Off · Enter to turn on
  Port                   8787 default
  Tailscale login        unset default  … key: remote.login
```

The consent row shows the editor's "Off · Enter to turn on" and keeps the
explanation that the gateway binds loopback and the phone reaches it through
Serve. No clipping or wrapping problem is visible in the text capture.

Evidence: `settings-remote.txt` (screen text) and `settings-remote.raw` (ANSI).
Input sequence: dismiss the startup consent (`n`, Enter), `Space v ,`, End.

## Native CUA

Blocked by macOS TCC: System Events automation failed/hung and
`screencapture -l` could not image the window. Details and cleanup are in
`blocked.md`. No pixel or accessibility-tree evidence exists for this revision.

## Automated coverage

- `tests/remote_web.rs`: a real disposable Server starts the child from
  `[remote] enabled`, serves the embedded page/script/font, stops it through
  `SetSetting remote.enabled false`, and shuts down.
- `crates/ovrcr-runtime/tests/settings_matrix.rs` and the catalog tests cover the
  three new paths, their types, defaults and validation.
- `cargo test -p ovrcr-tui --lib` (409) covers the settings editor's group order
  and consent-row wording.

## TUI toggle

A second disposable instance started with `[remote] port = 8795, enabled = false`.
From Browse: `:`, `remote`, Return opened Settings on `remote.enabled`; Return
toggled it. The screen text shows `off → on → off`; the document changed
`enabled = false → true → false`; a loopback `GET /api/state` on 8795 answered
200 while on and refused once off. Evidence: `toggle-on.txt`/`.raw` and
`toggle-off.txt`/`.raw`; the selecting frame is `settings-selected.txt`.

## Gap

Pixel, contrast, clipping and accessibility-tree evidence remains unavailable
because of the TCC block in `blocked.md`; the terminal captures prove the rows
and the state changes but cannot judge rendering. Native CUA should be rerun
after Accessibility and Screen Recording are granted.
