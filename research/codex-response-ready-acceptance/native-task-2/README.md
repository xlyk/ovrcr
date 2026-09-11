# Native Task 2 check — 26be5eb

Actual OVRCR GUI from independently reviewed source26be5eb, native Codex0.153.0, macOS. `CARGO_INCREMENTAL=0 cargo build -p ovrcr --features gui --bins` exited0; `just gui` launch exited0 after native window close. Binary hashes are retained. Tracked checkout matched reviewed revision; only untracked task evidence existed.

## Observations

- Disposable provider/workspace config only; explicitly authorized credential reuse used a private0600 copy, removed after native exit. Trusted the empty task-owned workspace and five reviewed reporting hooks through native UI. No live trust/configuration changed, no provider approvals intercepted.
- Initial supplied prompt completed before hooks were trusted: visible OVRCR_CODEX_READY_ONE, reporting remained Unknown. It is an untracked startup observation, not a passing hook turn.
- After trust, prompt OVRCR_CODEX_READY_TWO visibly produced Busy/Observed then an actual assistant OVRCR_CODEX_READY_TWO response and ResponseReady/Observed. Snapshot records activity revision2, matching root binding, metrics null.
- Next prompt OVRCR_CODEX_READY_THREE visibly cleared Ready to Busy, produced actual OVRCR_CODEX_READY_THREE, and returned to Ready. Background screenshot shows its Ready label while another terminal is selected. Subsequent Busy revision5 and final Idle revision6 establish the two prior prompt/Stop pairs without extra Ready synthesis.
- Long harmless integer output: observed Busy. Escape attempt left Busy in both UI and retained after-interrupt snapshot. Ctrl-C then visibly produced native Conversation interrupted and OVRCR Idle/Observed; no Ready. Do not count the earlier Escape attempt as successful cancellation.
- /exit returned to the shell; distinct CODEX_NATIVE_EXIT=0 output retained. Final snapshot retained Idle revision6 with supervisor_disconnected/Unavailable and metrics null. Kernel `stty size` reported57 rows ×126 columns in the same unchanged pane.
- Production receiver's accepted events pass kernel peer/native-parent and live unreaped-child checks; successful native Busy/Ready/Idle demonstrates that route. Process inventory independently records supervisor36652 and its native child37526. No raw peer-PID trace was added to production, no transcript was read, and no universal hook-order guarantee is inferred beyond this supported native exercise.

## Failed UI attempt and cleanup

One paste after immediate selection/Return timed out waiting for clipboard consumption; fresh AX showed dashboard still in Browse. Explicitly entered the acknowledged pane before retrying. No duplicated prompt was submitted. CUA session was reset only to reload API documentation; no application restart resulted.

All15 recorded PIDs and13 owned groups absent; GUI root/socket and private provider fixture removed; GUI launcher exit0. Credentials were never written to retained evidence. Screenshots/AX, snapshots, binary hashes and ownership/cleanup records retained here.

This passes Task2's narrow native two-turn/Interrupt route. Task3 setup/doctor, final reviewed-checkout GUI acceptance (including child/backtrack/reconnect), Linux and final CI remain open. Native API-error inference, metrics, sound and unread tracking are excluded.
