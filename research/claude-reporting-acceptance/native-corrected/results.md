# Corrected native split-pane smoke

The coordinator recorded `7ec5634` as the source immediately before building all
native binaries and exercising the actual GUI with an isolated Claude fixture.
The retrospective [provenance record](provenance.md) supports that sequence, but
no contemporaneous pre-build `git status` file was retained. The first and second
prompts produced the distinct visible responses `OVRCR_CORRECTED_FIRST_OK` and
`OVRCR_CORRECTED_SECOND_OK`. `02-two-turns.json` reports Idle/Observed after the
second response and retains one Claude conversation, invocation, and generation-1
binding across both turns.

`03-split.png` and `03-split.ax.txt` show the live agent in a 62-by-57 pane. The
complete metrics row is visible as:

```text
tok conv partial 82600 uncertain cost conv est $0.06 uncertain
```

This closes the native reproduction of the freshness-label clipping seen in the
earlier `ba7d560` smoke. The compact row retains token and cost scopes, Partial
coverage, both values, estimated cost kind, and both uncertainty labels.

The pane was closed, the dashboard detached, and it reattached to the same agent
process, PID 9744. `04-detached.json` and `05-reattached.ax.txt` retain the same
binding and both actual response markers through that sequence.

The agent exited with status 0. `06-exited.json` and `06-inventory.json` retain
Partial usage of 82,556 input and 44 output tokens, estimated conversation cost
of 596,662,000 USD ticks, and reporting Unavailable with reason
`incomplete_final_accounting`. The run does not establish complete or settled
provider accounting.

`build-7ec5634.log` and `launcher.exit` record the native build and zero launcher
exit. The retained launcher command was tool session 90233. `cleanup.json`
confirms that all 23 recorded PIDs and all 16 recorded process groups were absent;
the fixture root directory and socket were also gone.
