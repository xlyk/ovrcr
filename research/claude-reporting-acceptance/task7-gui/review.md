# Native macOS reporting check — 2026-09-10

Initial bundled source: `2feda3b` (the setup source was committed during the build; no product edit followed the build start). Launcher: `rtk proxy just gui`, retained execution session78083. Fixture: `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-v3edHG`. This is a native GUI check with synthetic reporting data; actual Claude reporting acceptance is blocked at its trust prompt.

- `01-input`: actual CUA paste/Return produced the separate `CUA_CLAUDE_INPUT_OK` output line. Config and socket were confirmed inside the disposable fixture.
- `02-nontty-wrapper`: `rtk proxy python3 launch.py` pipes stdout, so admission remained unavailable and native Claude returned its noninteractive/print-input error. Failed attempt preserved. Retrying with shell redirection from/to `/dev/tty` used the same controlling PTY and reached certified-startup wait.
- `03-trust-pending`: automatic approval review rejected accepting Claude's workspace trust prompt as an access change lacking specific user authorization. Approval was requested for only the displayed disposable folder. No alternate route was used to grant Claude trust.
- `04-synthetic-metrics`: a separately created local synthetic provider fixture (not Claude) exercises real launcher, hooks, statusline, collector, socket publication and native rendering. Input30 + cachewrite10 + cacheread40 + output8 displays canonical88, partial Conversation usage; source-estimated Conversation cost$0.25; uncertain source freshness. The sidebar shows idle observed. The selected header omitted quality and used legacy activity, so this finding was returned for correction.
- `05-synthetic-split`: metrics clip inside their own pane; no overlap with the adjacent real shell. This does not prove native provider accounting.
- `06-synthetic-reattach`: CUA detach followed by Restart dashboard preserved synthetic PID87646, output marker, partial88 and estimated$0.25. Raw inspection snapshot saved separately.
- `07-synthetic-small`: resized native window preserves readable reporting rows and marker. Header quality remains the original finding in this capture.

Images are actual JPEG captures. Accessibility `.ax.txt` files contain lossless JSON-encoded full text so terminal spaces survive version control. Corrected header recapture, native provider approval/accounting, and cleanup are pending at this checkpoint. Linux, native token/cost agreement, completion settling and full accounting are not claimed.

## Corrected header and final synthetic check

The reviewed36213f4 dashboard binary was atomically installed in the disposable
bundle and loaded through CUA Restart dashboard. The existing server and synthetic
supervisor retained their original2feda3b executable; this is a client correction
check, not a fresh final-assembly provider run. `08-corrected-header` shows
`agent idle observed` in the selected header, matching the sidebar. After eleven
minutes the unchanged metrics display stale; reattachment did not refresh them.

CUA sent exit to the synthetic fixture. `09-synthetic-exit` shows the closed PID
and retained partial totals; synthetic-final-usage.json retains explicit final
partial-accounting health. All three recorded synthetic PIDs and their owned
PGIDs were verified absent in synthetic-cleanup.json. Real Claude remains paused
at the still-unapproved trust prompt. The disposable GUI/server and original
shells are retained for that pending approval; their whole-fixture cleanup is
not yet complete. Launcher session78083 remains open.


2026-09-10 authorized native follow-up: trust approval completed; two native Claude turns, observed activity, partial metrics, cost comparison, detach/reattach and exit0 verified. All recorded fixture processes, socket and disposable directory cleaned. See `research/claude-reporting-acceptance/task7-gui-native/review.md`. Earlier pending trust/cleanup statements are historical; provider completeness and Linux gates remain open.
