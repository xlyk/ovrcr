# Authorized credential reuse: partial source evidence, disk blocker

The initial boundary below is historical; the GUI continuation and final outcome follow.

Continuation at clean `084698e8d2a71a40010e8560d3afe2757008901a`, 2026-09-11 UTC. User explicitly authorized using existing Codex credentials. Only `auth.json` was copied to a new 0700 task-owned provider home with mode 0600. No credential contents/hashes, whole live config or live sessions were inspected or retained. Original credential file was not written. The temporary copy was removed in harness cleanup, verified absent.

Native 0.153.0 skipped login onboarding and reached its workspace trust selector. This demonstrates local credential reuse past onboarding, not a completed authenticated provider request. No assistant response or root hook was observed. The exact positional prompt requested only the task marker; it did not execute before the boundary.

Readable transcription of the retained PTY screen:

```text
You are in /private/tmp/ovrcr-codex-auth-hbxxyxjr/workspace
Do you trust the contents of this directory? Working with untrusted contents comes with higher
risk of prompt injection. Trusting the directory allows project-local config, hooks, and exec
policies to load.
1. Yes, continue
2. No, quit
Press enter to continue
```

No trust selection, Enter, native approval or other user keystroke was sent. Only the terminal cursor-position query received ESC[1;1R. An isolated SessionStart command was configured, but no hook event file was created. The harness records the intended root PID before releasing a gate into native exec; any callback would record ancestry and remain **not admitted pending process-route review**. No transcript was opened or source binding inferred.

PID=PGID=TPGID `4322`, PPID `4319`, TTY `ttys016`, native state `Ss+`. After the 12-second observation, the harness signaled its still-owned group with SIGTERM, reaped wait status 15, and verified group absence. Harness exit 0 is capture/cleanup success, not native normal exit. A scoped `lsof -t +D` on this exact fixture returned no holders (exit 1). Credential removal succeeded. Public evidence is retained here; generated private provider state will be removed after coordinating the GUI boundary.

CUA could not select Terminal: `Computer Use is not allowed to use the app 'com.apple.Terminal' for safety reasons.` No alternate channel was used to operate Terminal. This refusal is specific to Terminal; it is not evidence that the project OVRCR GUI is unavailable. No screenshot/AX result or GUI input is claimed. Coordinator separately reported no target build and approximately 640 MiB disk free; that is a build-resource limitation, not a source/authentication limitation.

One authenticated matrix attempt has reached a concrete trust input boundary. No repeat has run. Root provenance, exact path/header/pagination, token numbers, turns, child/foreign ordering, approvals, resume and disconnect remain unverified. Next action requires an allowed actual native GUI and the applicable approval for the isolated workspace trust choice; do not substitute PTY input or change live trust.


## GUI continuation and final result

The coordinator authorized a copied existing OVRCR GUI as a generic native terminal host, with distinct bundle ID `dev.ovrcr.codex-source-fixture`, isolated HOME/TMPDIR and disposable OVRCR demo. [Host provenance](host-provenance.json) pins the two reused binaries by SHA-256. This is **not** a current-checkout build or Task 4 product acceptance. Original app and live configuration were not changed.

Actual CUA input accepted the empty task workspace, reviewed the single configured SessionStart command, and trusted that task-owned hook in isolated provider configuration. The first GUI thread produced `OVRCR_CODEX_AUTH_ROOT_OK` while the hook was still awaiting review; it therefore supplies authentication/response evidence, not root callback evidence. Native `/exit` ended PID/group 38891, verified absent. A fresh launch after hook trust was the specific startup-observation correction within this same matrix; no telemetry resume occurred.

On that fresh launch the supervisor-style wrapper wrote expected native PID **48614 before exec**. The configured hook read its immediate parent from the OS: `48614 17622 codex`. Its root `SessionStart(source=startup)` carried thread `01a08e40-4e38-7201-a363-05acbb2d8762` and an exact transcript path. The helper was written and reviewed by this task; it kept its binding decision as **not admitted pending review**. [Callback evidence](hook-event.json) and [header assertions](header-validation.json) establish this particular native route, not a production transport guarantee.

Only that hook-reported file was opened after matching the expected parent. Its first record is `session_meta`, ID matching the callback, `cli_version=0.153.0`, `source=cli`, **history_mode=paginated**. No directory scan, newest-file choice or terminal-output identity inference was used. Five [allowlisted source rows](root-source-allowlist.json) preserve the header, task start/completion and token records; message bodies, base instructions, account limits/credits and authentication are excluded.

| Observation | Native values and limit |
| --- | --- |
| Root turn | `01a08e40-4e67-7d62-bc7f-0755978ae906`; same ID in task start, token row and completion |
| `token_usage_record` | `usage`, `turn_token_usage`, `thread_token_usage`: input 14649, cached input 11904, output 12, reasoning 0, cache write 0, total 14661 |
| `event_msg/token_count` | `total_token_usage` and `last_token_usage` match those values; `model_context_window=258400` |
| Ordering | Persisted ordinals 12 token usage, 13 token count, 14 task completion; this single trace does not certify every ordering/replay rule |
| Native visible result | Separate assistant output `OVRCR_CODEX_AUTH_ROOT_OK`, CUA screenshot and AX in tool trace |
| Coverage | One observed turn on the root with trusted hook. No same-root second turn, permission wait/allow, child/foreign-first case, cancellation, root failure, resume, compaction or source recovery certification |

The repeated totals are two representations of the same first-turn usage; they must not be added. This sample suggests cached input is a subset (14649 + 12 = 14661), but the full reference matrix and context-occupancy interpretation remain unverified. No cost or final-accounting guarantee is claimed.

### Production identity requirement

A reporter-supplied `expected_root_pid`, PPID or ancestry JSON is not authentication. Production requires independently known native process identity from the launcher and receiver-side OS-authenticated peer PID plus actual parent verification while both processes remain live. The existing reporting socket/launcher does not expose that complete guard. The implementation must reject foreign/nested roots and subagents even if they inherit callback configuration, and must fail closed on exit/PID reuse/ancestry races. This fixture demonstrates a direct-parent relationship; it does not implement or race-test that mechanism. Paginated current-path/replacement/lineage behavior also remains uncertified beyond this exact header.

### Failures, disk stop and cleanup

A CUA `paste` inserted stale clipboard task text into the disposable shell instead of the requested launcher. The shell printed command errors; Ctrl-C followed by `typeText` launched the intended script. No clipboard content was sent to Codex and no source was inferred from it. This host-input failure is retained here; the successful direct input does not erase it.

After the fresh root completed, shell heredoc creation failed with **no space left on device**. Read-only `python -c` then validated the exact file/header and printed selected metrics. Native persistence and subsequent history integrity are not certified after ENOSPC. No further model turn was sent. CUA screenshots and AX also failed with `-10005: failedToCreateImageDestination`. Removing only the task-owned generated plugin cache (27 MiB) briefly restored CUA; a root-result screenshot/AX was captured in the tool trace. Disk filled again, including failure to save the pre-exit PID file; the 14-process snapshot was instead retained in tool output. No screenshot file export is claimed.

Native `/exit` ran via CUA (the following capture failed), and PID 48614 was verified absent. Cmd-Q closed the host; GUI launcher exited 0. All **14 recorded GUI/server/native/demo session PIDs** were verified absent and the host demo TMPDIR was empty. The copied app was removed only after cleanup. [Cleanup record](cleanup.json) lists the IDs. The auth copy was removed before shutdown and verified absent. Remaining generated provider state and task-owned host scratch were removed after evidence extraction; no secret values or hashes were retained.

The earlier unauthenticated attempt 01's missing PID/group cleanup proof remains unresolved and its separate scratch `/private/tmp/ovrcr-codex-native-sd67gle4` is untouched. It is not covered by this successful 14-process cleanup.

**Outcome:** authentication and isolated hook trust are resolved; root/header/first-turn token observations are real but partial. Source certification and adapter implementation remain blocked by incomplete matrix evidence after ENOSPC and the still-unimplemented production peer/process identity guard. No more native runs or broader research were performed. Independent review is required before any scope decision or runtime work.
