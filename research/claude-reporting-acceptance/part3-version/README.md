# Claude Code 2.1.268 source discovery

Captured on macOS on 2026-09-10 from the actual disposable OVRCR GUI built at `fa4e12dc36c418cfc7af2079d62c4e72d38edd1c`. `source-prebuild.txt` and `status-prebuild.txt` record the build inputs. That implementation supports only 2.1.267, so these sessions correctly remained untracked (`agent: null`). This directory certifies provider source shapes; it does not substitute for integrated acceptance after enabling 2.1.268.

The default executable and fixture symlink both resolved to exact 2.1.268. Every launch checked the exact version before exec, preserving native argv. The installed default was not changed. Authentication used the existing macOS Keychain login only after explicit user approval, passed in process memory to the task-owned provider. No token or credential store is retained here. The initial credential-access action was rejected before execution; the approved retry succeeded.

## Native results

| Case | Native outcome | Evidence |
| --- | --- | --- |
| Fresh turn | `OVRCR_268_SEED_OK`, remembered word copper, root startup identity | `01-seed*` |
| Stop continuation | Blocking hook output caused another response under the same prompt identity; `OVRCR_268_CONTINUED_OK` | `02-continuation*`, `hook-decisions.jsonl` |
| Delayed idle | Matching idle notification 60.063212 seconds after the final Stop; no guarantee inferred from elapsed time | `03-idle-after-continuation*`, `signals.jsonl` |
| Permission wait/allow | Native Bash permission picker, one-time allow, `OVRCR_268_TOOL_DONE` | `04-permission-wait*`, `05-tool-done*` |
| Child tool failure | Provider backgrounded the requested child; child-tagged PreToolUse/PostToolUseFailure, then parent response | `06-child-and-compaction-start*` |
| Same-ID compaction | Same root ID and source=compact; current usage explicitly zero, not null | `07-compacted*` |
| Long UUID resume | Exact seed ID resumed, preserved history, `OVRCR_268_RESUME_OK copper` | `11-resumed*`, `launches.jsonl` |
| Missing long UUID | Native no-conversation error, no SessionStart, exit 1 | `09-missing*` |
| In-process branch/resume/clear | A to branch B to A to clear C observed; selection authority remains uncertified | `12-transitions*`, `signals.jsonl` |
| Native null context | After clear, current_usage, used_percentage and remaining_percentage are null; GUI shows `ctx —` | `12-clear*`, `signals.jsonl` |
| Short UUID resume | Exact `-r UUID`, matching resume hook, `OVRCR_268_SHORT_RESUME_OK copper` | `14-short-resume*` |
| Missing short UUID | Native no-conversation error, no SessionStart, exit 1 | `15-short-missing*` |
| Child API failure | Nonexistent child model returned model_not_found HTTP 404; child-tagged StopFailure, then `OVRCR_268_CHILD_API_PARENT_OK` | `16-child-api*`, `signals.jsonl` |

The child API case used the process-local `CLAUDE_CODE_SUBAGENT_MODEL=claude-ovrcr-child-invalid-2099` and inline agent definition recorded in `launches.jsonl`. The parent retained the valid sonnet model. This was an actual API error, distinct from the earlier Bash failure. The native provider backgrounded the child. Root reporting exclusion still needs integrated verification because discovery sessions were untracked.

`settings.json` captures the original hook configuration; `settings-next.json` adds SubagentStart/Stop and compaction callbacks for later cases. The earlier child-tool case therefore does not claim a captured SubagentStop. `capture.py` forwards original native bytes and retains an allowlist of identity/numeric fields. The one blocking Stop decision is separately recorded; payloads were not rewritten.

## Telemetry result

The receiver bound only loopback and accepted OTLP HTTP/JSON metrics. It retained numeric values, allowed category metadata, hashed session identity, temporality and monotonicity, dropping account and resource identities. `otel-capture.py` contains its bounds and filtering. The before-prompt receiver had accepted one export containing only session.count with start_type=resume; rejection counts were zero.

| Scope | Recognized input including cache subsets | Output |
| --- | ---: | ---: |
| Eight retained root records before resume | 335379 | 372 |
| One new resumed request | 50478 | 19 |
| Nine recognized records after the response | 385857 | 391 |
| Exported new request | 2 input + 36287 cacheRead + 14189 cacheCreation = 50478 | 19 |

All nine retained (message ID, request ID) pairs are distinct. These are the recognized root subset, not complete provider accounting. The OTEL export did not restore historical token totals. It carries no request/correction identity or final-source boundary. `10-resume-before-prompt-*` and `13-otel-final-*` preserve the actual exports and receiver counters; no filtering rejection is being mistaken for an absent category.

## Certification and limits

The [independent source review](source-review.md) approves exact 2.1.268 for the existing Partial/Observed contract and separate short UUID resume on 2.1.268 only. It does not extend short resume to 2.1.267. Integrated fresh/long/short admission, source recovery, native callback exclusion and final cleanup remain the next acceptance gate.

In-process transitions still lack proven independent foreground intent. Complete accounting lacks category reconciliation, correction ordering and finality. Confirmed activity lacks a guaranteed post-decision source. The captured idle and paired transition events do not establish those guarantees; the remaining cancellation/denial/race matrices are not claimed passed. See [source investigation](source-investigation.md).

## Ownership and cleanup

`*-processes.json` and `owned-processes.json` retain pre-exit PID/PPID/PGID/foreground TPGID/TTY evidence. The missing-conversation cases exited before a full native descendant snapshot; only their creation PIDs were retained and checked absent. No unobserved descendant inventory is claimed for those short-lived errors.

`native-exit-inventory.json` records normal exit 0 for sessions 11, 13, 14 and 16, and native failure exit 1 for sessions 12 and 15. `cleanup.json` verifies all 28 recorded PIDs, 21 groups and retained creation PIDs absent, GUI and receiver launcher exit 0, and fixture root removed. The separate telemetry fixture was removed after retaining its final files. No closed GUI was queried afterward. Unrelated live sessions were untouched.
