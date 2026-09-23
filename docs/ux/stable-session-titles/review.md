# Issue #160 — native stable-title acceptance

For automated checks, failed attempts, and independent review outcomes, see [verification](verification.md).

2026-09-23, macOS. **PASS for the bounded native flow; no acceptance blocker.**

Tested the current checkout at `/Users/xlyk/Code/ovrcr-workspaces/328516bf02bf0fd013df29db37f91774`, branch `feature/ignore-title-updates`, base `68c771e97536e5e2a04a58e0552adefe8fef5e69` **plus existing uncommitted changes**. [Checkout inventory](checkout-before.txt) and [build/binary evidence](build-evidence.json) identify this run. No source edits, commits, worktrees, delegation, agent-config changes, or Rust test suites were performed by this worker.

Launched the actual checkout app with `rtk proxy just gui` (build succeeded in 11.99s; launcher session `54252`). All GUI input, screenshots, and closing used the available CUA MCP after reading its documentation.

## Observations

AX captures are gzip-compressed without changing their terminal-cell spacing.

Created an unnamed Terminal in the disposable `consigint / main` workspace. Its generated session name was `main` (#13), shell PID/PGID `627`. All four markers below appeared as separate real output lines; the entered printf commands contained `CUA_%s`, not those complete output strings.

| Evidence | Actual observation |
| --- | --- |
| [01 screenshot](01-unnamed-terminal-form.jpg) · [AX](01-unnamed-terminal-form.ax.txt.gz) | Terminal form had blank Name and Command; wording: `Name blank = generated · Command blank = shell`. |
| [02 screenshot](02-generated-name-after-osc0-osc2.jpg) · [AX](02-generated-name-after-osc0-osc2.ax.txt.gz) | OSC 0 `APP_OSC0_REJECTED` produced `CUA_OSC0_OK`; OSC 2 `APP_OSC2_REJECTED` produced `CUA_OSC2_OK`. Sidebar, pane title, and status retained `main`. [Intermediate OSC 0 AX](osc0-observation.ax.txt.gz) independently records the first result. |
| [03 screenshot](03-rename-form.jpg) · [AX](03-rename-form.ax.txt.gz) | GUI Rename showed `Rename terminal · blank = original name` and `Blank = original name · Enter save · Ctrl-u clear · Esc cancel`. [Palette AX](rename-menu.ax.txt.gz) likewise said `Rename terminal (blank = original name)`. |
| [04 screenshot](04-manual-title-after-osc2.jpg) · [AX](04-manual-title-after-osc2.ax.txt.gz) | GUI Rename set `Manual Stable 160`. OSC 2 `APP_MANUAL_REJECTED` produced `CUA_MANUAL_OK`; sidebar, pane title, and status retained the manual title while the emitting command waited on `read`. |
| [05 screenshot](05-cleared-title-after-osc0.jpg) · [AX](05-cleared-title-after-osc0.ax.txt.gz) | Ctrl-u in Rename cleared the field; save restored `main`. A further OSC 0 `APP_CLEAR_REJECTED` produced `CUA_CLEAR_OK`; the restored title held while the emitting command waited on `read`. [Empty field AX](rename-cleared-input.ax.txt.gz) and [immediate restoration AX](original-name-restored.ax.txt.gz) record the intermediate states. |

The titles, commands, output, Rename wording, PID/status, and mode footer were readable without overlap in the captured layout. All five images are unmodified CUA JPEG captures with verified JPEG signatures.

## Cleanup and limits

Owned fixture: `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-U2VQP6`; config `config.toml`, socket `server.sock` beneath it. Both environment paths were printed through GUI input and belong to this fixture. GUI PID `63381`, server PID `63486`, dashboard PID/PGID `67682`; GUI/server launcher PGID `59966`. Owned shells and temporary command groups are recorded in [initial inventory](processes-before.json), [manual-title inventory](processes-manual-title.json), and [pre-close inventory](processes-before-close.json).

Closed the native close button through CUA. Launcher **exited 0**, with no cleanup-error output. [Cleanup verification](cleanup.json) confirms the fixture directory/socket are absent and `os.kill(..., 0)` raised `ProcessLookupError` for every recorded PID and all **17** recorded process groups. No unrelated processes or live sessions were stopped; no app-specific CUA query was made after closing.

One CUA paste attempt while navigating from the key popup timed out with `Computer Use server error -10005: Timed out waiting for the application to read the clipboard`. Fresh CUA state showed the popup remained open; clicking its Close control and Actions, then typing `Rename`, completed the same flow. No alternate input channel was used.

This is bounded macOS evidence for one generated-name Terminal, manual Rename, and clear; it does not claim exhaustive session/provider coverage, Linux acceptance, or automated-suite results. The universal ignore-title contract remains the coordinator's automated coverage responsibility. No required native step remains blocked.
