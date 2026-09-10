# Authorized native Claude follow-up — 2026-09-10

User explicitly approved the disposable workspace trust prompt. The retained RTK-wrapped session did not respond to terminal navigation; its owned session was closed. A fresh `terminal create` launched Python directly in the runtime PTY, avoiding the intermediary RTK stdin wrapper. CUA Down then Return visibly selected and accepted trust in the same authorized folder. No product code changed.

Source: dashboard, new supervisor and collector binary36213f4; existing server/GUI helper2feda3b. The branch checkpoint was d818899. Session12 supervisor PID49243, native PID49249; all ownership recorded. Claude Code2.1.267, model claude-sonnet-5, existing authorized subscription credential provided privately by the fixture launcher.

- Two actual assistant outputs: OVRCR_NATIVE_TURN_ONE and OVRCR_NATIVE_TURN_TWO. Native busy/idle observations appeared in the dashboard; idle remains Observed.
- Initial transcript absence showed unavailable/unknown usage, then recovered Connected after the first turn without changing the binding.
- After two turns: canonical input87145, output42, total87187; cache subsets already included. Context43621/1000000. All usage Partial and freshness Uncertain.
- Native /cost showed$0.0703, including haiku auxiliary usage and additional model counters. OVRCR's earlier statusline sample was$0.0602644, then reached$0.0702902 by finalization (rounds to native$0.0703). Token totals do not match all native categories; discrepancy is not fully attributed. This is evidence for retaining partial coverage, not complete billing certification.
- CUA detach/Restart dashboard preserved supervisor PID49243, both assistant outputs, binding and metrics.
- Native /exit returned0; final health incomplete_final_accounting and coverage Partial. No confirmed settling inferred.
- CUA closed the app. Retained launcher78083 returned0. All27 recorded PIDs and23 PGIDs absent by ESRCH; socket and disposable GUI directory absent. See08-cleanup.json. No live fixture remains.

Screenshots plus full accessibility JSON and actual CLI snapshots are preserved. Earlier trust blockage is resolved. Linux, final provider accounting boundary, resumed/forked/auxiliary completeness, and other unexecuted release cases remain open. Prior workspace586-test/clippy/fmt results remain applicable; this follow-up changed only evidence/docs.
