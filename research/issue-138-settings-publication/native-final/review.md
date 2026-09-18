# OVRCR #138 native acceptance repeat

Result: PASS for all three requested native acceptance criteria.

Tested `implement-task-138`, HEAD `2dd2bd1b0065855d9634f98c1eefb33cf620ba72`, with the uncommitted settings publication fix. `tested.diff.gz` preserves the exact tested source changes; it matched `final.diff.gz` at closeout. No source edits, commits, delegation, cargo tests, Docker queries, or unrelated UI interactions were performed.

## Results

1. **Native settings save and persistence: PASS.** Prepared only the disposable fixture's `dashboard.toml` with `# CUA_138 retained comment`, notifications false, and sound false. Native Shift-N produced visible `Desktop notifications: on` (01-toggle JPEG + full AX). Disk contents retained the comment and saved `desktop_notifications = true` (`settings-after-toggle.toml`). Detached using q, clicked Restart dashboard, and opened Space menu: `N Disable desktop notifications` confirmed the restarted dashboard loaded the saved value (02-restarted-menu JPEG + full AX).
2. **Read-only save failure and usable selected session: PASS.** Changed only fixture `dashboard.toml` to 0444 (`readonly-mode.txt`). Native Space w n opened Create terminal with Terminal selected; Name and Command remained blank (03-create-form full AX). Submitted using Return. Before any terminal input, the selected new session showed PID 94788, and the footer visibly displayed `ERROR: Session started; could not remember launch choice: dashboard settings are read-only` (04-warning-before-input JPEG + full AX). This capture includes the ready shell prompt and selected sidebar row. Native paste/Return of `printf 'CUA_138_%s\n' USABLE` produced a separate `CUA_138_USABLE` output line below the command echo, with the same selected PID (05-usable JPEG + full AX). The footer changed to Terminal mode after input. The failed save left settings unchanged (`settings-after-failed-save.toml`).
3. **Cleanup: PASS.** Restored fixture settings to 0644, clicked the native close button, and retained launcher exit 0. All 14 recorded process groups returned ProcessLookupError; root and socket were absent (`cleanup.json`). The closed app was not queried afterward.

## Ownership

Fixture root: `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-6uTi6i`

Config: fixture root + `/config.toml`; socket: fixture root + `/server.sock`; settings: fixture root + `/dashboard.toml`.

Launcher RTK PID 61643, just PID 61644; GUI PID 61680; server PID 61812. Launcher/GUI/server PGID 61643. Initial dashboard PID/PGID 62440; restarted dashboard PID/PGID 81119. Initial session PID/PGIDs: 62048, 62126, 62267, 62360, 62368, 62385, 62395, 62408, 62424, 62437. New session PID/PGID: 94788. Full parentage and command ownership are retained in `ownership-initial.json` and `ownership-after-create.json`.

## Launch and limitations

The first sandboxed launcher rebuilt successfully but GUI aborted with signal 6 / launcher exit 134 (`launcher.log`). The requested `rtk proxy just gui` was then run with approved native GUI access; it built successfully and completed native acceptance with cleanup exit 0 (`launcher-native.log`, `launcher-results.json`). No automatic approval rejection occurred. No fixture path was observed for the aborted launch; all ownership and lifecycle claims above refer to the successful native launcher.

Screenshots are actual JPEG files, verified by file signature. Every acceptance screenshot has corresponding full AX text. The ordinary commented settings-file path was tested; symlink behavior was optional and not exercised. Automated testing remains the coordinator's responsibility. Prior-attempt evidence in `native/` was not reused or changed. Work diary closeout is left to the coordinator because this worker's explicit write/evidence scope is this directory and the owned disposable fixture.
