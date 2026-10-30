# Task 2 — native combined-protocol Operator acceptance

The bounded native smoke **passed** on source **`ffebfe859b7aae83872354ef55b12edd63db484e`** (`codex/operator-layout-design`). This was a new launch of matching GUI, CLI, server and unchanged manual provider holder from the combined protocol 70/schema 27 source. Seven actual JPEG/AX pairs were inspected. No source, provider fixture, proxy, live configuration, installed app, remote service or historical capture was changed.

## Identity and integration

[State](state.json) records the reviewed source revision, three binary hashes, and affected Operator/input/codec/wire/storage source hashes. Both fresh invocations exited 0:

- `rtk proxy cargo build --features gui,acceptance-diagnostics --bin ovrcr --bin ovrcr-gui`
- `rtk proxy cargo test -p ovrcr --test operator_e2e --features gui,acceptance-diagnostics --no-run --message-format=json`

The app bundle was copied from those matching debug binaries. [Protocol/schema receipt](protocol-schema.json) ties the owned server PID 15289 and bundle executable/build identity to the recorded Demo socket. A direct eight-byte preamble exchange returned **`OVRC` + 70** (`4f56524300000046`); the same instance's read-only SQLite `PRAGMA user_version` returned **27**. Its normal matching CLI `--json list` exited 0 ([output](cli-list.json)). This is positive native combined-version evidence; the controller's separate rejection tests cover 68/69/future peers.

Fresh CUA inventory and availability-only Finder AX succeeded before bounded owners started. Unrelated availability AX was not saved here. Only actual controlled fixture screenshots are committed.

## Visible application outcomes

Every linked JPEG has full same-name `.ax.txt.gz` and timestamp/source/hash `.json` receipts. Actual inputs are in [input-calls.jsonl](input-calls.jsonl), with [capture recipe](capture-calls.js).

| Observation | Evidence |
| --- | --- |
| Matching Dashboard/server handshake; disposable 12-session sidebar and current terminal rendered | [01](01-protocol70-dashboard.jpg) |
| New Grok/fixture-a conversation, actual submitted prompt, Bash succeeded, inline Responding and retained full Unicode draft/cursor | [02](02-responding-tools-retained-draft.jpg) |
| Quick common editing preserved `Quick ▏preserved 界🙂 tail`/`second café`, mixed Control C preserved it and the underlying full draft | [03](03-quick-common-input.jpg) |
| Quick clear then empty no-op, full clear; selected turn still Running with inline feedback | [04](04-cleared-still-running.jpg), [durable Running receipt](clear-running-boundary.json) |
| Separate empty full Ctrl+C stopped selected turn, showing Interrupted | [05](05-separate-selected-stop.jpg), [durable Interrupted receipt](selected-stop-boundary.json) |
| Sidebar close confirmation named conversation 1/session 13/run 1; confirmed archive removed row; archive picker offered Unarchive for owned Operator | [06](06-archived-history-picker.jpg), [archive boundary](archived-boundary.json) |
| Unarchive restored row; reopening saved chat preserved prompt, tool history and Interrupted journal | [07](07-restored-saved-journal.jpg), [final boundary](restored-final-boundary.json) |

Pixels show readable tool/activity feedback and visible CJK/emoji in the drafts, with no observed overlap in these 1100×932 captures. Context/Managing rows remain visible. Durable conversation/turn/message rows are equal across stop/archive/restore boundaries: one conversation, one Interrupted turn, six messages. Session archive flags were not separately snapshotted; the archive/restore claim rests on the actual visible controls and restored history. Draft retention was checked during the active turn, before deliberate clearing; this does not claim draft persistence through archive.

Provider requests were fixture-a 2, fixture-b 0, Quick 0, activity 0, all through existing private synthetic loopback OAuth/fixture. The real Bash tool used the production CLI list path. No paid provider call occurred.

## Attempts and limits

One CUA literal-colon call returned `Computer Use server error -10005: keyNotFound(":")`; [failure receipt](cua-key-error.json) is preserved. The documented supported `shift+semicolon` chord then opened search. In the New Conversation model field, Ctrl+U did not clear; the observed appended value was corrected with Backspace before creating/submitting. Neither attempt is counted as successful input behavior.

This smoke intentionally uses one selected turn and Stop. Independent concurrent completion, all 38 prior captures' cases, native floor/theme matrix, Linux, real-provider quality, clipboard, IME and resize were not repeated. The controller's compiled-worker E2E 14 and broader gates are separate automated evidence, not native observations from this run. The controller's earlier copied-server startup failure and original cleanup uncertainty remain separate preserved failures; this native pass does not establish a clean local full-suite run.

## Ownership and cleanup

Exclusive source/Cargo/target/native ownership was held during build/launch and released only after the original holder and GUI owners were reaped. Task-owned root was `/private/tmp/ovrcr-main-native-hmmtjegy`, Demo `/private/tmp/ovrcr-main-native-hmmtjegy/temp/ovrcr-gui-jH3i9M`, with socket beneath that Demo. Recorded originals: holder 14801, GUI 15270, server 15289. Read-only process receipts captured their descendant PIDs/groups while alive.

The unchanged holder retained its original 600-second deadline. It ended normally: **1 passed, 15 filtered, 106.61s**, original wait 0. GUI close used fresh AX close index 73; original wait 0. No closed-app query followed. [Cleanup](cleanup.json) proves all **17 recorded PIDs / 16 groups absent**, Demo/socket gone, binary hashes unchanged and outer private root removed. No forced cleanup was needed.

The offline saved-evidence verifier passed:

`rtk proxy python3 research/operator-main-integration-2026-10-30/verify.py`

[Verification receipt](verification.json) checks capture hashes/text markers, actual protocol/schema, states/history/provider counts, original waits, source identities and cleanup. Scripts reuse the native03 owner/provider recipe and contain no new source behavior. The existing plan edit and unrelated `research/operator-layout-2026-10-30/followup-native-01/` were left untouched.
