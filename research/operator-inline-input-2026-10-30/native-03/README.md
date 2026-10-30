# Native03 — mixed Control modifiers

The corrected native GUI preserves Alt/Shift on alphabetic Control chords before
raw control bytes can erase those bits. Actual Ctrl+Alt+C, Ctrl+Shift+C and mixed
U/K/W keys left full and Quick Unicode drafts and cursor markers unchanged. Empty
mixed Ctrl+C also left both real responses Running. Ordinary Ctrl+C still cleared
first, then a separate empty full press stopped only selected A; B survived and
completed with its draft. A normal terminal Ctrl+C interrupted a real sleep child
and the same shell printed a fresh marker.

Source: **`8a141c71c0e3ccd6928b463ac24d6b9eb66ffff2`**. This is a fresh correction
run. The older native02 ordinary-path evidence keeps its original source/hashes;
it is not relabeled as this candidate. [State and hashes](state.json) records
matching fresh CLI/GUI/holder builds and eight source hashes. Builds exited 0
([CLI/GUI](gui-cli-build.json), [holder](holder-build-result.json)). No target or
source writer ran during native ownership.

## Actual input and visible outcomes

Each JPEG below has same-name full `.ax.txt.gz` and call/hash `.json` receipts.
All eleven actual 1100×932 JPEG/AX pairs were inspected at capture time; no image
editing or application mock was used. [Exact input calls](input-calls.jsonl) and
[recipe](capture-calls.js) retain the real CUA sequence.

| Check | Native evidence |
| --- | --- |
| Current-source inline Responding, full context, retained draft | [01](01-full-before-mixed.jpg): Responding follows A's actual Bash output/partial reply; full Managing row and Unicode draft remain visible |
| Full mixed modifiers preserve draft/cursor | [01 before](01-full-before-mixed.jpg) / [02 after](02-full-after-mixed.jpg): `Full ▏preserved 界🙂 tail` and `second café` unchanged after C/U/K/W with Ctrl+Alt, Ctrl+Shift and Ctrl+Alt+Shift |
| Quick mixed modifiers preserve both drafts/cursors | [03 before](03-quick-before-mixed.jpg) / [04 after](04-quick-after-mixed.jpg): `Quick ▏preserved 界🙂 tail` unchanged; underlying full draft stays unchanged |
| Ordinary Quick clear and empty no-op | [05](05-quick-cleared-empty-no-stop.jpg): empty form remains open; A/B Running, no Quick conversation/inference |
| Full clear before Stop | [06](06-full-cleared-still-running.jpg): first plain Ctrl+C clears; both responses stay Running. Three subsequent mixed empty C chords also leave both Running in `full-empty-mixed-boundary.json` |
| Separate plain empty selected Stop | [07](07-separate-empty-plain-stop-a.jpg): A Interrupted; [08](08-independent-b-survives.jpg): B Running with retained draft |
| Ordinary terminal interrupt | [09](09-terminal-sleep-running.jpg) / [10](10-terminal-ctrlc-survived.jpg): actual `/bin/sleep 30` PID14490/group14489 disappears, shell PID83249 survives and prints separate `NATIVE3_TERMINAL_SURVIVED` within 30 seconds |
| Independent completion | [11](11-independent-b-completed-draft.jpg): B final, no activity, Enter Send, same retained draft |

Full-before-owned, full-after, quick-after, quick-empty, full-clear and
full-empty-mixed receipts have exactly equal durable conversations, turns,
messages, control-marker lists and binary hashes: A/B both Running sequence4,
ten messages. Selected Stop removes worker A PID91463 while B PID94226 remains
present through the terminal check. Final state is A Interrupted/B Complete,
two conversations/two turns/twelve messages. Provider counts are A2/B2/Quick0/
Activity0, all private synthetic loopback requests.

## Evidence partition and limits

The automated real byte-path regression begins with egui events, uses the actual
GUI encoder, and sends those bytes through the compiled Dashboard PTY/crossterm
path. Per-chord text barriers check full/Quick draft and cursor preservation. It
does not inspect an outbox. The existing production socket fixtures separately
assert no outbound request for decoded mixed chords, and native durable Running
receipts catch unintended real Stop. No new request recorder, proxy or provider
fixture was introduced. See the retained `automated-*.log.gz` files and the
controller's final-fix report for exact commands/counts and RED history.

Known native repeated Ctrl+C suppression remains first and is covered in the
11-test GUI input suite; physical held-key Repeat/Release metadata was not
attested by CUA. Legacy external terminals without repeat metadata still cannot
distinguish a hold from separate presses. Native floor/theme matrices, Linux,
IME/clipboard, paid-provider quality and hosted CI were not repeated here. This
narrow pass does not replace historical native02 ordinary editing/history proof.

One receipt attempt hit the default sandbox's `ps` EPERM after writing its
read-only boundary. [Failure](process-receipt-failure.json) and partial boundary
remain; the separately named permitted `full-before-owned` receipt succeeded.
No failed observation was overwritten or counted as ownership proof.

## Ownership and cleanup

The unchanged native holder retained its original 600-second deadline and ran
**1 passed, 15 filtered, 203.51s**. Original holder/GUI waits were **0/0**.
Normal close used the current AX close index73; there was no closed-app query.
[Cleanup](cleanup.json) proves all **20 recorded PIDs / 18 groups** absent,
Demo/socket gone, frozen binary hashes unchanged and private outer root removed.
No forced cleanup, live configuration, installed binary, paid call or remote
operation was used. Scripts reused native02's private fixture and owner waits.

`rtk proxy python3 research/operator-inline-input-2026-10-30/native-03/verify.py`
checks saved hashes, draft markers, durable states, worker/terminal identity,
provider counts, original waits and cleanup: [passed verification](verification.json).

Representative image: [Quick after all mixed chords](04-quick-after-mixed.jpg).
