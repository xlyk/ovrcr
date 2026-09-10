# Integrated native Claude smoke

`source.txt` identifies the reviewed merge revision as
`d12abda8100bd15cde4f9740c004eafd2f691116`, and the contemporaneous
`tracked-status.txt` captured before the build is empty. This native macOS run
therefore used one clean tracked source revision after the mainline integration.

The first and second prompts produced the distinct visible responses
`OVRCR_INTEGRATED_FIRST_OK` and `OVRCR_INTEGRATED_SECOND_OK`.
`02-two-turns.json` reports Idle/Observed after the second response and retains
one Claude conversation, invocation, and generation-1 binding across both turns.

`03-split.png` and `03-split.ax.txt` show the live agent in a 62-by-57 pane with
the complete compact metrics row:

```text
tok conv partial 82618 uncertain cost conv est $0.06 uncertain
```

The row preserves token and cost scopes, Partial coverage, both values, estimated
cost kind, and both uncertainty labels. The pane was closed, the dashboard
detached, and it reattached to the same agent process, PID 15954.
`04-detached.json` and `05-reattached.ax.txt` preserve the binding and both actual
response markers through that sequence.

The agent exited with status 0. `06-exited.json` and `06-inventory.json` retain
Partial usage of 82,572 input and 46 output tokens, estimated conversation cost
of 597,726,000 USD ticks, and reporting Unavailable with reason
`incomplete_final_accounting`.

The launcher command was tool session 90828 and exited with status 0.
`cleanup.json` confirms that all 22 recorded PIDs and all 16 recorded process
groups were absent, and both the fixture root directory and socket were gone.

This smoke verifies the integrated supported route and corrected split rendering.
It does not certify complete accounting, Confirmed settling, native child
Stop/failure isolation, or native null-current-context timing.
