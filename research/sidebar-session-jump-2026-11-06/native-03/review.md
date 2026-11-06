# Final combined native GUI check

Source `5916f78cf3ff1755695d45ff6dc648933641848f`, packaged from the integration checkout by `just gui`. The packaged binary hashes, private fixture paths, session inventories, and owned process groups are recorded beside this review. The GUI uses local provider-name shell stand-ins; no model turn was submitted. Optional status setup was declined, and the launch used private account/profile directories.

## Observed outcomes

- Native Cmd+1 selected `review auth handoff`, session #10/run 1, PID 32007. After release, explicitly typed printf produced a separate `FINAL_ONE_DIGIT_OK` line in that Agent: [screenshot](01-one-digit-ack.jpg), [full accessibility capture](01-one-digit-ack.ax.txt.gz).
- Four additional Agent-kind local shells brought the visible Agent count to ten (six ordinary terminal sessions plus ten Agents). The control tool rejected a two-digit native chord; [the exact limitation](tool-limits.md) is preserved.
- An unobscured Operator conversation retained the unsent ASCII draft `FINAL_OPERATOR_DRAFT ` and cursor after native Cmd+1/release cancelled a partial two-digit code. No navigation or submission occurred: [screenshot](02-operator-prefix-release-cancel.jpg), [before](operator-before.ax.txt.gz), [after](02-operator-prefix-release-cancel.ax.txt.gz).
- With the sidebar hidden, native Cmd+1 left it hidden and preserved the previous Agent selection in Browse: [screenshot](03-hidden-sidebar-refusal.jpg), [accessibility capture](03-hidden-sidebar-refusal.ax.txt.gz). The ordinary sidebar toggle restored it, and re-entering Operator restored the same draft.

## User-directed interaction revision

A physical-key handoff was requested with ten Agents visible: hold Cmd, press `1`, retain the hold for a label/prefix capture, then press `0` and release. Kyle replied: “i tried it, but it isn't working as i want. we should only use single digits”. This supersedes the adaptive interaction; it is not recorded as successful two-digit acceptance. Held-label/prefix appearance and native two-digit completion remain unverified historical evidence. The replacement labels at most the first nine eligible visible Agents with 1–9 and selects on one digit.

The superseded GUI closed through CUA. Launcher exited0, the private root/socket disappeared, and all16 recorded session groups returned ProcessLookupError (`cleanup.json`). No task-owned fixture remains. Earlier retained navigation and Cmd-K/modal native evidence remains in [native-02](../native-02/review.md), on its explicitly recorded older source. The single-digit replacement needs its own native check.

This proves only the requested test-GUI route. Standard CLI terminals, iTerm, tmux, SSH, and Linux Cmd-hold support remain unverified and unadvertised.
