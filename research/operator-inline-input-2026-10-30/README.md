# Operator inline activity and input — native acceptance

The reviewed native GUI shows activity inside the journal, preserves history,
context and unsent drafts, and applies the new editing commands in both message
editors. A first Ctrl+C clears a draft; a separate empty full press stops only the
selected response. Empty Quick Ctrl+C leaves its underlying response running.
Ordinary terminal Ctrl+C still interrupts a real child and leaves its shell usable.

Tested source: **`6e560817477ed9993c5806da958e39dd239b1511`**, with product correction
`8074d55c1105447e35225c432ce5f93eec5cf726`. [Native state](native-02/state.json)
records eight source hashes, matching fresh GUI/CLI/holder hashes, features and
private paths. Both build commands exited 0: [GUI/CLI](native-02/gui-cli-build.json),
[holder](native-02/holder-build-result.json). Target files stayed frozen while
the original native owners were alive. No source, installed binary, runtime,
provider protocol, paid service or live user configuration was changed.

The earlier [native-01](native-01/README.md) is an interrupted attempt against
`31b1684`: it reached the new-conversation form, then paused for the native repeat
encoder correction before any conversation was submitted. Its initial sandbox
loopback failure and successful normal cleanup remain separate from native-02.

## Visible acceptance

Every linked JPEG has a same-name full `.ax.txt.gz` and call/hash `.json` receipt.
All 27 saved JPEG/AX pairs were inspected when captured. Images retain actual JPEG
bytes; no crop, compositing or mock application screen was used.

| Requirement | Actual native evidence |
| --- | --- |
| Silent inline animation after latest user message | [01](native-02/01-silent-collapsed-a.jpg) / [02](native-02/02-silent-collapsed-b.jpg), glyphs ⠴→⠼; Running sequence 0, one User message, no start-gate release |
| Pending tool animation after Bash entry | [03](native-02/03-pending-collapsed-a.jpg) / [04](native-02/04-pending-collapsed-b.jpg), glyphs ⠇→⠦; Running sequence 2, User/AssistantDelta/ToolCall only; real Bash result and final-model gates closed |
| Collapsed and hidden sidebar independence; context/draft | Operator shows `▸1` in 01–04; [07 compact hidden sidebar](native-02/07-pending-hidden-compact.jpg) retains the entire Managing row and `Retained follow-up draft 界🙂; do not send.` |
| History scroll-away and return | Real PageUp [05](native-02/05-history-away.jpg) hides activity; capture 06 has identical JPEG/AX hashes. PageDown returns activity in 07. |
| Older viewport survives provider completion | [08 before](native-02/08-history-before-progress.jpg) / [09 after](native-02/09-history-after-progress.jpg) retain exactly History 07–27 despite transition to Complete and unread indication; no forced tail-follow |
| Completion removes activity and restores Send | [10 completed tail](native-02/10-completed-tail.jpg) shows succeeded Bash output, ACTIVITY final, no activity, Enter Send and the identical unsent draft |
| Full multiline line/word/deletion input | [11 before](native-02/11-full-edit-before.jpg) / [12 after](native-02/12-full-edit-after.jpg): `alpha 界🙂 omega\nsecond café tail`→`alpha 界🙂 omega\nond café`; [13](native-02/13-full-grapheme-delete.jpg) removes CJK+emoji as complete graphemes |
| First Ctrl+C clears without stopping | [14 A draft](native-02/14-running-a-draft.jpg) / [15 empty but Running](native-02/15-a-cleared-still-running.jpg); durable receipt independently records A/B both Running |
| Separate empty press stops exact selected response | [16 A Interrupted](native-02/16-separate-empty-a-stop.jpg), [17 B survives](native-02/17-b-survives-a-stop.jpg); real workers and journal confirm only A interrupted |
| Quick multiline line/word/deletion input | [18 before](native-02/18-quick-edit-before.jpg), [19 after](native-02/19-quick-edit-after.jpg), [20 whole-grapheme deletion](native-02/20-quick-grapheme-delete.jpg), matching the full-editor results |
| Empty Quick Ctrl+C is no-op | [21](native-02/21-quick-empty-no-stop.jpg): first press cleared text, second empty press left Quick open and B Running; no Quick conversation or inference was created |
| Ordinary terminal Ctrl+C | [24 sleep running](native-02/24-terminal-sleep-running.jpg) / [25 interrupted and shell survives](native-02/25-terminal-ctrlc-survived.jpg): actual `/bin/sleep 30` existed before, disappeared after Ctrl+C, same shell PID 43232 produced a separate `CUA_TERMINAL_SURVIVED` output line within 30 seconds |
| Independent response completes with its draft | [26 running journal](native-02/26-independent-running-journal.jpg) / [27 complete](native-02/27-b-complete-draft-retained.jpg) retain `Review the next workspace change 界🙂` |

Representative images: [clean running journal](native-02/26-independent-running-journal.jpg)
and [compact inline pending tool](native-02/07-pending-hidden-compact.jpg).
The actual saved JPEG sizes were 1100×932, 1100×604, 723×406 and 1003×496
pixels. No observed composer/context
clipping required the nested floor fixture. These captures do **not** claim native
80×24/60×16 or both-theme coverage; the existing deterministic buffer matrix owns
those exact geometries and themes.

## Gates, authority and commands

The existing `compiled_worker::native_provider_holder` and loopback synthetic OAuth
provider were reused unchanged. The actual UI created model `fixture-activity`
and submitted `ACTIVITY Inspect this bounded native acceptance journal.` plus 32
numbered Unicode history lines. The follow-up draft was pasted only after submit.
The [CUA recipe](native-02/capture-calls.js) records the capture wrapper, native
input sequences, exact editing sequence and cleanup control.

Before/after silent and pending receipts compare exactly equal conversations,
turns, messages, gate-marker lists and binary hashes. The pair calls were
consecutive capture-only invocations, with no key, resize, release or provider
progress between them. Distinct actual glyphs and timestamps prove idle motion.
The initial model gate, Bash-result file and final-model gate were each released
once. The final activity receipt is Complete sequence 5 with five durable messages.

The short real A/B prompts were `A hold selected response for clear then separate
Stop` and `B independent held response must survive selected A Stop`. A/B reached
Running sequence 4 before the first clear. After separate Stop, A is Interrupted
and B stays Running; final B is Complete sequence 6. Final database has three
conversations, three turns and 17 messages. Provider counts are Activity 2 / A 2 / B 2,
Quick 0. These are controlled fixture requests, not paid inference.

All shell invocations used `rtk proxy`. Build/launch commands:

```sh
rtk proxy cargo build -p ovrcr --features gui,acceptance-diagnostics --bins
rtk proxy python3 research/operator-inline-input-2026-10-30/native-02/prepare.py
rtk proxy python3 research/operator-inline-input-2026-10-30/native-02/launch.py holder
rtk proxy python3 research/operator-inline-input-2026-10-30/native-02/launch.py gui
rtk proxy python3 research/operator-inline-input-2026-10-30/verify.py
rtk proxy git diff --check
```

Preparation builds `cargo test -p ovrcr --test operator_e2e --features
gui,acceptance-diagnostics --no-run --message-format=json`, copies the exact holder
into its mode 0700 root, and copies the matching CLI/GUI to the checkout bundle.
Launches need local socket/PTY/GUI execution permission. `record.py <phase>` saves
read-only database and owned descendant receipts. No Cargo or target writer ran
concurrently with the native owners.

## Failures, limits and separate automated proof

Capture 23 preserves a harness quoting error: a malformed nested trap command in
the disposable root shell produced `trap: usage` and a zsh math error, then that
shell exited. It is not a product failure or a passing Ctrl+C check. The corrected
bounded proof used the fixture's already-existing feature shell, a simple printf
plus `rtk proxy sleep 30`, native Ctrl+C and a fresh separate printf output.

Physical held-key repeat was not exercised. CUA exposes `pressKey` (including
duration) but no independently addressed Repeat/Release event or repeat metadata;
a timed press alone would not establish which event kind crossed the GUI encoder.
The source encoder regression `gui::input::tests::ctrl_c_repeat_cannot_become_another_press`
and existing production dispatcher tests own the known-repeat guard and
Repeat/Release safety. They also own deliberately frozen Create/Submit receipts,
retry payload synchronization and overlay permutations. No native proxy, timing
race or keyboard-protocol fixture was added. Legacy external terminals without
repeat metadata retain the documented physical-hold limitation.

This task did not repeat the unchanged 341 owning TUI tests, 330 public tests or 14
compiled-worker product cases. The separate source-task records report those
passes, plus the later 10 GUI input tests, check/clippy/fmt passes for the encoder
correction. Those results are distinct from this native run and do not imply a
new full-suite run at this evidence commit. The two ignored fixture helpers are
not counted as passing product cases. This native run explicitly invoked the
holder: **1 passed, 15 filtered, 216.26 s**, original exit 0.

Linux, real provider quality, clipboard delivery, physical hold, IME composition,
native floor/theme matrix and final hosted CI remain outside this native proof.
There was no lock/access failure during either GUI run, no merge, GitHub write or
work-diary edit by this worker.

## Original waits and owned cleanup

Native-02 original owner sessions: holder 56349/PID 40926, GUI 7448/PID 41798.
Private root `/private/tmp/ovrcr-inline-native2-ukewk5au`; demo
`.../temp/ovrcr-gui-yhWah0`. The holder's original 600-second deadline was never
extended; it finished in 216.26 s. Finish was set once after final receipts.
Fresh full AX identified 39 close button; `await app.click(39)` returned, then
**both original waits returned 0**. No signal, CLI forced close or closed-app query
was used. Logs, original log hashes, waits and close-call receipts are retained.

[Cleanup](native-02/cleanup.json) verifies all 24 recorded PIDs and 19 groups absent,
demo and socket removed by normal GUI cleanup, and frozen CLI/GUI hashes unchanged.
Only then was the remaining private holder root removed. The bundle remains in
the reviewed checkout. [Saved-evidence verification](verify.py) checks 27 JPEG/AX
hashes, pair boundaries/glyphs, older viewport lines, actual worker states, provider
counts, terminal child/shell outcome and both original waits. It passed. Its first
assertion draft incorrectly included the changing header in the history equality;
restricting it to actual numbered history rows corrected the verifier, with no
source or evidence modification.
