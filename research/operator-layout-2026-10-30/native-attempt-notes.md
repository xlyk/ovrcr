# Native orchestration attempts

- The first native run's initial CLI `terminal list --json` inventory ran without
  escalation, returned exit1 with empty stdout, and the inventory script then
  failed JSON parsing. Its child stderr was not retained, so no permission or
  application cause is claimed. The scoped escalated inventory later returned
  exit0; its output is `native-terminals-alive.json`.
- The first nested script printed `NESTED_KERNEL_16 60`, then RTK reported that
  `/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr/target/OVRCR` did not
  exist. The attempted executable path contained `OVRCR GUI.app`. The corrected
  script changes into the published bundle directory and invokes `rtk proxy ./ovrcr`.
  The original output is retained in the computer-use transcript; `native-15`
  additionally retains the distinct outer-intercepted Ctrl+O attempt. Correct
  nested evidence is `native-16` and `native2-04`.
- Saving the first `native2-03` JPEG used a duplicated directory segment and
  returned ENOENT after the paired AX text had been written. The next call saved
  the same in-memory screenshot bytes at the correct `native2-03-60x24-quick.jpg`
  path and emitted them for inspection. The JPEG/AX pair is from one capture.
- Native run3's bounded SQLite gate returned an explicit database-lock Create
  rejection. The subsequent inventory found no quick worker, as preserved in the
  transcript. The error popup is `native3-02`; only an explicit manual retry after
  that known rejection created the gated worker recorded in
  `native3-groups-quick-alive.json`. No delayed successful native Created claim
  is made, and no production SQLite behavior was changed.
