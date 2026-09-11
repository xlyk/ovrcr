# Native child-failure isolation

`source.txt` records product revision
`d12abda8100bd15cde4f9740c004eafd2f691116`. The contemporaneous
`tracked-status.txt` contains only a documentation change to
`docs/agent-reporting-support.md`; no product source was changed from `d12abda`
for this targeted native run.

The allowlisted `provider-signals.jsonl` records one real child agent,
`a78d93c3af567835b`, emitting `PreToolUse`, `PostToolUseFailure`, and
`SubagentStop`. Each callback forwarded with exit status 0. The bounded capture
wrapper retained identities and runtime snapshots only; it waited 0.2 seconds
between the before and after reads for child callbacks. It stored no provider
transcript content or credentials.

`isolation-checks.json` compares the runtime state around each of the three child
callbacks. Binding, activity, activity revision, health, and root usage are equal
before and after every callback. `01-child-completed.png` and its accessibility
capture show the parent's distinct actual response
`OVRCR_CHILD_FAILURE_ISOLATED_OK` after the child completed.

This proves isolation for an observed real child tool failure and
`SubagentStop`. It does not certify the behavior of an arbitrary child API
`StopFailure`, which was not observed in this run, or prove complete child
accounting coverage.

The supervisor process, PID 73171, and the native session exited with status 0.
`02-exited.json` retains Partial accounting and
`incomplete_final_accounting`; `02-inventory.json` records the exited session.
The launcher command was tool session 9244 and exited with status 0.

`cleanup.json` reports all 19 recorded PIDs and all 12 recorded process groups
absent, with the fixture root and socket gone. That cleanup list was captured
after native exit and does not contain an independent pre-exit inventory of the
native and collector PID/PGID set. It therefore proves cleanup only for the
recorded resources in this targeted case. The earlier integrated native run's
complete 22-PID and 16-process-group cleanup evidence is unchanged.
