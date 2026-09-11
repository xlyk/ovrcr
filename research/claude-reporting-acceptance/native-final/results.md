# Final native Claude smoke results

This run used one native macOS build of product revision `ba7d560`. The retained
screenshots, accessibility captures, JSON snapshots, and allowlisted provider
signals contain the acceptance evidence. Provider raw transcripts and secrets
were not retained in this evidence directory.

## Session 11: two turns and dashboard continuity

The first and second prompts produced distinct visible responses,
`OVRCR_FINAL_FIRST_OK` and `OVRCR_FINAL_SECOND_OK`. `01-first-turn.json` and
`02-second-turn.json` retain the same Claude conversation, invocation, and
generation-1 binding while activity returns to `Idle` with `Observed` quality.
The usage observation increases from 41,248 to 82,590 total input and output
tokens and remains explicitly `Partial`.

`03-split.ax.txt` shows the live session in a 62-column pane with both actual
response markers. The pane was then closed, the dashboard detached, and it
reattached to the same running agent process, PID 54942. `04-detached.json` and
`05-reattached.ax.txt` preserve the same reporting binding and the visible
responses after that lifecycle sequence.

After exit, `06-exited.json` retains the partial metrics and marks reporting
`Unavailable` with reason `incomplete_final_accounting`. This is evidence of
preserved incomplete observations, not complete or settled provider accounting.

## Session 12: API failure and recovery

The second native session started with the deliberately invalid model
`ovrcr-acceptance-invalid-model`. The allowlisted signal capture records the
corresponding `UserPromptSubmit` and real `StopFailure`. `07-api-error.json`
reports `Error` with `Observed` quality, and `07-api-error.ax.txt` plus the PNG
show the provider's invalid-model error in the actual terminal.

The model was then changed through the native terminal with `/model sonnet`.
`08-api-recovered.ax.txt` shows the distinct actual response
`OVRCR_API_RECOVERED_OK`; `08-api-recovered.json` reports `Idle` with `Observed`
quality. Failure and recovery use the same conversation, invocation, and
generation-1 binding. The recovery therefore demonstrates continued reporting
within the existing supervised invocation rather than a replacement session.

Computer use timed out while a confirmation dialog was active during the model
change. Reading fresh UI state and confirming the existing dialog completed the
interaction. The subsequent provider response and idle snapshot passed, so the
timeout is retained as an automation-control interruption rather than a product
failure.

After exit, `09-api-exited.json` again reports `Unavailable` with
`incomplete_final_accounting` and retains `Partial` usage. No complete-accounting
claim is made for either session.

## Split metrics finding

The 62-column row in `03-split.ax.txt` and `03-split.png` reads
`tokens conv partial 82590 uncertain  cost conv est $0.06 uncer`; the final
freshness label is clipped. This native run therefore found a real remaining
layout defect in `ba7d560`.

The bounded correction at `94566b5`, with evidence follow-up `7ec5634`, was
independently reviewed. It preserves the wide format and uses the smaller
`tok ... cost ... est` form only when the intermediate compact row still does
not fit. A corrected native capture has not yet been run, so the reviewed source
change does not count as final native layout acceptance.

## Build and cleanup

`build-ba7d560.log` and `launcher.exit` record the single native build and a zero
launcher exit status. The retained launcher command was tool session 68442.
`cleanup.json` confirms that all 27 recorded PIDs and all 19 recorded process
groups were absent after cleanup. The fixture root directory and socket were
also gone.

The remaining native gate is a smoke capture of the corrected 62-column metrics
row from the reviewed post-`ba7d560` source.
