# Settings sidebar acceptance — #413

## First native walkthrough

Tested the uncommitted implementation based on `9e027779bfc94cb217f4921a04e30ed78747ed92`, using `rtk proxy just gui` from this checkout. The exact application image hashes are in `native-first-images.json`. This was a pre-correction run, not final-revision acceptance.

The fixture used private HOME/XDG directories under `/tmp/ovrcr-413-checks.DjTg8r` and the disposable Server `/tmp/ovrcr-gui-hPZsDy`. Optional agent-hook setup was declined. No personal settings or live instance was edited.

| Evidence | Observed outcome |
| --- | --- |
| `01-appearance-wide` | Menu opened Settings; Appearance showed one Theme row with its own header/footer and compact sidebar. |
| `02-theme-preview-wide` | Filtering to Solarized Light repainted the entire Settings view and named both preview and saved palettes; the document still had no theme override. |
| `03-theme-cancel-wide` | Esc restored Catppuccin Mocha and returned to Theme. |
| `04-theme-saved-wide` | Reopen/filter/Enter saved canonical `solarized-light` through the Server and returned to Theme. Reopening highlighted the saved palette. |
| `05-theme-preview-narrow` | Resizing below 80 columns preserved the Nord filter/preview and saved Solarized Light name in a single-pane editor. |
| `06-root-added-narrow-before-fix` | Adding a folder preserved the original root, but exposed stale manager source/reset details. |
| `07-workspace-restored-narrow`, `09-browse-restored` | Esc unwound manager/list/categories to the original `consigint / main / local (#1)` context. |
| `08-terminal-input-restored` | A real shell printed the separate output line `SETTINGS_413_INPUT_OK`; this is not command-echo-only evidence. |

Each numbered capture has a PNG and accessibility-tree text. Wide frames are 1074×704 pixels (81×24 terminal cells); narrow frames are 964×704 pixels (72×24 cells). Exact 80×24, 60×24 and too-small/recovery layouts are exercised through Dashboard rendering tests.

The native root edit exposed a second bug: opening an existing folder produced an empty draft rather than its current path. The attempted suffix edit consequently saved `/native-root` in this disposable document only. The entry was reordered and then removed through the manager, leaving the original root intact. No `/native-root` filesystem directory was created or removed. `native-settings-after.toml` records the resulting settings.

The two collection defects were returned to the implementation owner for correction and regression coverage. This first run does not qualify them as passing.

## Cleanup

The launcher exited 0. Its disposable Server root was removed. Every recorded owned PID and process group was absent (`ESRCH`); identities and probes are retained in `native-owned-processes.json` and `native-first-cleanup.json`.

## Implementation-candidate native walkthrough

The implementation-candidate Dashboard source is `f3d21df13ac9200a021a065033c0b6973d530726`.
`native-final-revision.json` records its executable hash and the unchanged
Server/GUI harness images. After each correction, the owned Dashboard was
detached successfully, replaced from this checkout's build, and restarted
through the GUI's **Restart dashboard** control; no personal instance was used.

- `19-final-theme-preview-wide`: Menu → Appearance → Theme, filtering to Nord,
  with the complete view previewing Nord while Saved remained Solarized Light.
- `20-final-theme-saved-wide`: cancel, reopen, filter and Enter saved `nord`
  through the Server and returned to Theme with confirmed colors.
- `21-final-root-prefilled-narrow`: an existing folder opened with its complete
  path prefilled. Appending `/native-root` preserved the existing path prefix.
- `22-final-collection-reset-focus`: add/edit/reorder/remove/reset preserved the
  original default root and left a visible manager focus marker. Add offers
  **Enter add**, without unavailable remove/reorder controls.
- `23-final-workspace-input-restored`: the original `local (#1)` was selected
  again, a real shell printed `SETTINGS_413_FINAL_INPUT_OK`, and Browse returned.

The final persisted document contains the original
`automatic_local_terminals = "on"` plus `theme = "nord"` and no picker-roots
override; see `native-final-settings-after.toml`. The final launcher exited 0,
its disposable root was removed, and all 20 recorded owned PIDs/groups were
absent. `native-final-cleanup.json` retains the probes.

Captures 10–18 preserve intermediate correction evidence. In particular, 15
exposed a missing focus marker after shrinking/resetting a collection; 17
verified the focus correction, and 18 verified contextual Add controls.

## Review-corrected native walkthrough

Revision `f6d15cf5f4a6bfaf1c12ea3485658828ed50ad4d` addresses the independent
review findings. Captures 24–32 exercise the corrected surface:

- A sidebar click aligned with Tokyo Night leaves the saved Nord selection
  unchanged (`24`); saving Nord unchanged completes without a new publication
  or a stuck pending indicator (`25`).
- With both alert prerequisites Off, the saved Tap choice is highlighted in
  its picker and submitting it unchanged keeps Tap (`26`).
- Full notification and Automation permission consequences are visible before
  activation, including the narrow view (`27`–`29`). No permission was requested
  or enabled.
- Boundary and empty-draft feedback are visible, with no edit submitted
  (`30`–`31`).
- Closing Settings restores `local (#1)`, and the shell separately prints
  `SETTINGS_413_REVIEW_INPUT_OK` (`32`).

`native-reviewed-revision.json` pins the image; the fixture exited 0, removed its
private root, and all 20 recorded owned PIDs/groups were absent
(`native-reviewed-cleanup.json`).

## Current-revision child-save acceptance

At `a98ffde42bc110238c9333bfdc2298e4860d3d66`, captures 33–35 exercise an
unchanged, explicitly configured picker-root child through the native editor.
The full path is prefilled, Enter completes without a stuck `saving…`
indicator, the private settings document remains byte-identical, and Escape
returns to the original `local (#1)` in Browse. The corresponding live Server
regression also covers unchanged agent argv and remembered launches.

`native-current-revision.json` records the executable hash and source revision.
The launcher exited 0, removed its disposable root, and all 19 recorded owned
PIDs/groups were absent (`native-current-cleanup.json`).

## Broader regression gaps

The clean-environment, serialized extension command passed 65/67 tests. Two
unchanged OMP tests remain failing:

- `tests/omp_reporting_extension.mjs:43`: the session-switch case misses the
  first native conversation/file frame.
- `tests/omp_reporting_extension.mjs:230`: the overlapping-approval case misses
  the first `input_open` frame.

These tests materialize a shell mock helper, not the Rust application
(`tests/omp_reporting_extension.mjs:20–29`). Their source and transport files
were not changed for Settings. The cause is not established; investigate the
transport/harness completion boundary without weakening ordering assertions.
The initial 59/67 run and final 65/67 run are both retained under `checks/`.

Static follow-up: `src/ovrcr-reporting-transport.mjs:44,91–99` bounds each helper
with a 900 ms SIGKILL timer and resolves failed delivery as `false`.
[INFERENCE] The missing first frames and approximately 1.0–1.3 s failing cases
are consistent with the first helper hitting that deadline; the reason for
any slow spawn is not proven. The next diagnostic is per-sequence spawn/close
timing and exit signal, not weakening the callback budget or ordering checks.

## Verification summary

Local macOS verification covers all 55 Rust test targets. The complete
serialized workspace command reached its 3600-second wrapper limit during
`server_lifecycle`; the interrupted backpressure case passed alone, and all
unexecuted targets/cases were then completed explicitly. This is recorded
continuation coverage, not a claim that the single full-workspace command
exited successfully.

| Cases after corrections | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Rust workspace, including recorded continuations | 2864 | 0 | 31 |
| Node extensions, clean environment and serialized files | 65 | 2 | 0 |

The Rust total includes the full 440-case Dashboard target, 1129 shared-library
cases, and all 168 registered server-lifecycle cases (157 passed, 11 ignored).
The three old alert/sound PTY drivers were migrated to category/list/search
controls and passed without changing their persistence, consent, Bridge,
no-preview or no-replay assertions. The palette round-trip now unwinds both
Settings levels; the 80-column frame treats source/default as reachable
metadata rather than requiring them ahead of permission guidance.

Workspace all-target/all-feature typechecking, Clippy with `-D warnings`, and
formatting passed. Both independent review axes are clear after corrections;
see `review.md`. Linux execution and hosted CI were not run.

`checks/rust-acceptance-summary.json` and `checks/current-run-context.json`
record the revision, environment isolation, target coverage and commands.
Earlier failed and timed-out attempts remain under distinct filenames.
The unrelated Node failures above remain open.
