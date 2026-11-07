# Native GUI correction checkpoint

Reviewed source `8575c5f6296df8be619f3e46e8b4845655855ff6`, integration checkout, packaged via `just gui`. `revision.json` and `binaries.json` identify the launch; `fixture.json` names its owned root/socket/process. No provider turn or live-session changes. Declined optional personal status hooks; disabled quota collection only in this disposable instance.

## Observed native outcomes

- `02-cmd1-selection.jpg` / `.ax.txt`: native Cmd+1 moves from local shell #1 to `review auth handoff` (Agent #10, pid82517), then Terminal mode after release.
- `03-destination-ack.jpg` / `.ax.txt`: explicitly typed printf produces separate `NATIVE_JUMP_OK`, not merely echoed command.
- `06-retained-browse.jpg` / `.ax.txt`: after exiting only that owned fixture shell, Cmd+2 visits a running Agent and Cmd+1 returns to the retained Exited Agent in BROWSE. No PID/new run is created.
- `07-running-after-retained.jpg` / `.ax.txt`: Cmd+2 returns to running Agent #7, and separate `NATIVE_FROM_RETAINED` acknowledges post-release typing.
- `08-modal-priority.jpg` / `.ax.txt`: existing Cmd+K opens Commands; Cmd+1 preserves palette and destination; Escape closes palette.
- `sessions-before.json`, `sessions-after.json` and private screen snapshots corroborate unchanged retained run1 with no PID, same total twelve sessions, and markers in the intended panes.

## Hold-capture limitation

`01-cmd-held` and `04-invalid-zero-held` retain unsuccessful capture attempts. They show post-release idle state, not a successful held-label capture. The CUA tool rejects modifier-only pressKey, and serializes observations behind input actions: `hold-observation-timing.json` records the first observation at8708ms for an8000ms Cmd+0 hold, followed by observations8933…9607ms. A separate unawaited15000ms hold also completed before its tool call returned. Thus these snapshots cannot verify the held visual state. No product failure is inferred from them; native held-label/prefix appearance still needs an observation during a user-held Cmd key or a capable permitted tool. Automated rendered labels and compiled key/selection paths remain separate evidence.

## Automated correction evidence

`gui-focus-integrated-01.json` / `.log` (shared implementation evidence directory): exact integration8575, real GUI encoder→outer PTY→Dashboard focus-loss test passed1/1,12 filtered,11.18s. Focus loss cancels selected pending typing; mode-gated focus reports precede synthesized key releases. Worker unit/aggregate reports are separate.

## Cleanup

Closed window through CUA; inventory showed no GUI application, retained launcher23469 exited0. `cleanup.json`: private directory/socket absent and all twelve recorded process groups returned ProcessLookupError. Captured retained run stayed1, phaseExited, PID null. No fixture resources remain.

Original raw capture files named in this report are preserved byte for byte in the [artifact index](../raw-artifacts.md).
