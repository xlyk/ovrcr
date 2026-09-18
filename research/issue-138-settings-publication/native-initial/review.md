# OVRCR #138 bounded native acceptance

Result: PARTIAL; the required visible launch-save warning was not observed. No source edits, commits, delegation, cargo tests, or Docker queries were performed by this worker.

Checkout: `/Users/xlyk/.superset/worktrees/ce516841-6a34-4a08-8d1f-cefa87725355/chartreuse-acai`
Branch: `implement-task-138`
HEAD: `2dd2bd1b0065855d9634f98c1eefb33cf620ba72`, with existing uncommitted changes in `crates/ovrcr-tui/src/dashboard/palette.rs`, `crates/ovrcr-tui/src/dashboard/settings.rs`, and `docs/dashboard.md`. The native bundle was built from this checkout by `rtk proxy just gui`.

## Results

1. PASS — Native Shift-N changed desktop alerts to on. Screenshot and full AX show `Desktop notifications: on` (`01-toggle.jpg`, `01-toggle.ax.txt`). The fixture dashboard.toml symlink remained a symlink; its target retained both the full-line and inline comments while changing `desktop_notifications` from false to true. See `settings-before.toml` and `settings-after-toggle.toml`.
2. PASS — Native q detached, and clicking Restart dashboard reattached. The Space menu showed `N  Disable desktop notifications`, proving the enabled value loaded after restart (`02-restarted.jpg`, `02-restarted.ax.txt`).
3. PARTIAL / required warning NOT OBSERVED — Only the fixture settings target was chmod 0444 (`readonly.txt`). Space-w-n opened Create terminal. Start was Terminal, workspace consigint/auth-handoff, Name and Command blank. Return accepted Start, workspace, blank Name and blank Command. The new terminal was selected and active with PID/PGID 48636. However, the immediate post-submit screenshot/AX showed `Terminal mode  Ctrl-g browse`, not `Session started; could not remember launch choice`. Ctrl-g also showed the ordinary browse footer, without the warning. See `03-launch-warning.*` and `04-warning-browse.*`; those filenames identify the attempted check, not a successful warning observation. The target remained 0444 and unchanged (`settings-after-launch.toml`). This worker did not diagnose or fix the missing warning.
4. PASS — Through native input, executed `printf 'CUA_138_%s\n' USABLE`. A separate `CUA_138_USABLE` output line appears below the echoed command in both screenshot and full AX, with the new terminal selected (`05-usable.jpg`, `05-usable.ax.txt`).
5. PASS — Restored target mode to 0644, clicked the native close button, and waited for the retained launcher: exit 0. Every recorded task-owned process group returned ProcessLookupError from `os.kill(-pgid, 0)`; fixture root and socket were absent (`cleanup.json`). No closed-app queries were made.

## Ownership

Fixture root: `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-7ohYCE`
Config: root + `/config.toml`
Socket: root + `/server.sock`
Settings: root + `/dashboard.toml` -> `settings-target.toml`
Launcher PID/PGID: 21044
GUI PID: 22065; server PID: 22668; both PGID 21044.
Initial dashboard PID/PGID: 23153. Restarted dashboard PID/PGID: 38943.
Initial session PID/PGIDs: 22843, 22912, 22985, 23074, 23082, 23096, 23106, 23120, 23129, 23144.
New terminal PID/PGID: 48636.
Full ownership records: `ownership.json`, `initial-processes.txt`.

## Launch and evidence notes

The first sandboxed launch built successfully but the GUI aborted with signal 6 / exit 134. The authorized retry outside the sandbox succeeded after a brief Cargo lock wait. Both logs and final launcher results are retained (`launcher.log`, `launcher-native.log`, `launcher-results.json`).
Screenshots are original JPEG bytes with .jpg extensions, accompanied by full AX text. Existing fixture shell startup output contained zsh compinit errors; no unrelated shell configuration was changed. Evidence and this report remain in the requested native directory; work-diary writing is left to the coordinator under the worker's explicit evidence-only scope.
