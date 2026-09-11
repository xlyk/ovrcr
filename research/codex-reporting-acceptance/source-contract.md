# Codex 0.153.0 fresh-root source contract

**Selected source: receiver-authenticated root hook admission followed by a versioned reader of the exact native-selected rollout.** The bounded macOS native matrix completed at OVRCR `033f4ef485e4158170a909a14c1d3fccd2cd49ee`. This establishes a source contract for fresh invocations, not an implemented adapter, Linux acceptance or current-checkout OVRCR product acceptance. Independent review and Tasks 2–4 remain required.

The coordinator used actual CUA in the original OVRCR GUI as a generic terminal host, including isolated workspace/hook trust and one-time permission approval. Existing credential reuse was explicitly authorized; only a private temporary copy was used and removed. No live configuration, trust or session was changed. [Native fixtures](../../tests/fixtures/agent-reporting/codex/0.153.0/native-matrix/README.md) contain allowlisted source records and receiver evidence, without conversation bodies or authentication. Earlier login, ENOSPC and CUA failures remain in [historical checkpoints](historical-source-checkpoints.md); their no-route decisions are superseded here.

## Version and route

Installed executable `/opt/homebrew/Caskroom/codex/0.153.0/bin/codex`, SHA-256 `a29d9e86eef88cbbd69f97ce8c590b1d0a287c8f77424f5eef226b883d7eaa22`. Public source [41e22fee981a63b3698df7ed36bad393cda24715](https://github.com/openai/codex/tree/41e22fee981a63b3698df7ed36bad393cda24715) corresponds to the release label; binary reproducibility was not checked. Installed schema/help and source hashes remain in the versioned fixtures. No second general source comparison occurred.

Passive app-server observation was not selected: the installed method set has no passive exact-thread subscribe operation; `thread/read` does not subscribe. No `thread/resume` telemetry was used. The fallback accepts only the live selected root's authenticated path, never a directory scan, newest file or first arbitrary callback.

## Admission and lifetime

The supervisor must retain exact native child PID and live process identity independently of hook payloads. The macOS prototype obtained `LOCAL_PEERPID` from the accepted socket and queried that peer's OS parent while the helper stayed connected. Parent had to equal the selected native PID, whose start identity also matched. Serialized PPID is not trusted. Production must add equivalent lifecycle-bound metadata and receiver validation to the existing runtime boundary; the undeployed seam is an implementation requirement, not source impossibility. Linux requires its own credential/parent implementation and tests.

Main native PID 53509 was admitted through kernel peer 56975 / OS parent 53509; header matched root `01a08e5c-7480-7052-9964-9224aadebef0`. Fresh invalid-model invocation PID 40738 used peer 40826 / parent 40738 with its own empty binding and different matching root. The initial foreign-first probe raced after binding and its assertion failed. A deterministic separate empty-binding prototype then rejected a foreign kernel peer claiming the native PID. This is a synthetic sender test, not a second foreign native conversation.

Reject `agent_id` and `Subagent*` before considering a path. Actual child start/stop were ignored; notably child Stop reported the ROOT file path, so path equality is insufficient. Root startup admission requires `SessionStart`, `source=startup`, non-null absolute path, matching `session_meta.id`, `cli_version=0.153.0`, `source=cli` and supported `history_mode=paginated`. Bound invocations cannot switch identity/path.

The hook path comes from `live_thread.local_rollout_path` and requested materialization in [session/mod.rs](https://github.com/openai/codex/blob/41e22fee981a63b3698df7ed36bad393cda24715/codex-rs/core/src/session/mod.rs#L4586). Matrix reads observed matching header, stable device/inode and ordered records through two turns, permission, child and cancellation. This certifies that fresh current file, not every paginated transition. [thread_rollout_resolver.rs](https://github.com/openai/codex/blob/41e22fee981a63b3698df7ed36bad393cda24715/codex-rs/thread-store/src/local/thread_rollout_resolver.rs#L67) explains SQLite-selected lineage and older immutable files after revert.

Fail closed on missing/null path, unknown version/history mode, header mismatch, changed path/device/inode, truncation, malformed complete records or lost native/observer lifetime. Configured PreCompact/PostCompact hooks provide a compaction invalidation signal in the inspected source/schema; neither was exercised natively here. Reject unrecognized structural records rather than assuming ordinary append semantics. No native revert was exercised, and this matrix does not prove a universal same-file revert/rebinding detector. Such transitions remain uncertified; independent review must assess this limit before downstream support is enabled. Do not claim that naming an unsupported transition alone detects it. Never discover a replacement file. Incomplete trailing records wait; reads must be bounded. Hooks supply admission and refresh/invalidation, not activity ordering. Materialization was observed at saved boundaries, not proven synchronously complete on every callback. Replay the exact file in ordinal order, deduplicate processed records and correlate source turn IDs. Delayed prior-turn hooks cannot change current turn, activity or freshness.

## Literal event mapping

| Source | Identity/order | Interpretation |
| --- | --- | --- |
| `event_msg.task_started` | Root file ordinal and `turn_id` | Busy / Observed for latest ordered turn. |
| `event_msg.task_complete`, absent/null `error` | Current source turn | Idle / Observed; never Confirmed completion or Complete accounting. |
| `event_msg.task_complete`, non-null `error` | Current source turn | Error, taking precedence over Stop/idle for that turn. Invalid model persisted structured status 400 `invalid_request_error`, `codex_error_info=other`. |
| `event_msg.turn_aborted`, `reason=interrupted` | Current source turn | Idle/interrupted observation; does not certify tool termination. |
| `PermissionRequest` hook | Root/tool/turn correlated | Refresh only; generic WaitingInput unavailable because other hooks may approve. No persisted actual unanswered-selector boundary was found. |
| Stop, Interrupt, SessionEnd hooks | Authenticated root | Refresh/lifetime hints; cannot override source error or newer turn. Exit/EOF is not success. |
| Child/foreign callback | Reject before root processing | No root revision, freshness, idle or accounting change. |

Actual CUA screenshot/AX evidence captured assistant ONE then TWO markers, unanswered native permission selector, one-time approval, child completion, interruption and invalid-model error. Permission marker was absent before approval and present afterward; observer never answered approvals. Escape interrupted the conversation while a background terminal initially remained; it later disappeared. Cancelled completion marker was absent. Native error root had no usage records: Unknown, not zero.

## Literal metric mapping

| Source | Semantics |
| --- | --- |
| `token_usage_record.thread_token_usage` | Cumulative admitted root-thread usage: replace, never sum repeated totals. Partial / Conversation scoped to this root thread, excluding child/auxiliary work. |
| `token_usage_record.usage`, distinct `response_id` | Individual request reference; equal values with distinct IDs remain distinct. Do not mix request sums and cumulative totals. |
| `event_msg.token_count.info.last_token_usage.total_tokens` | Occupied-token numerator, separate from cumulative conversation usage. |
| `event_msg.token_count.info.model_context_window` | Capacity if present; missing/null Unknown. |
| Input/cache and output/reasoning | Cached input and reasoning output are subsets. Total=input+output, without adding subsets. Missing components Unknown. Explicit observed cache-write zero is retained. |
| Cost, native remaining percentage | Unknown; no pricing/final-accounting source, and native percent differs from raw occupied/capacity ratio. |

Distinct first/second turns and responses had totals 15,564 and 15,588; cumulative 31,152 and latest occupied 15,588. After permission occupied 15,877 / capacity 258,400 matched native `/status` rounded `15.9K used / 258K`. Its `98% left` is not that raw ratio; do not label a derived percentage as the native percentage. `/context` was unrecognized (failed native probe).

Child usage 16,957 was excluded while root cumulative rose from 78,584 to 126,622 through three own requests, delta 48,038. Final root total 159,049=input 158,733+output 316 includes cached input 145,152. Native exit displayed total 13,897=input 13,581+output 316, plus cached 145,152 separately. Thus 13,897+145,152=159,049. This verified display convention explains the difference. Native reasoning subset observed zero only; nonzero reasoning semantics still require parser reference assertions.

## Supported grammar and remaining gates

Source-certified grammar: exact version 0.153.0 fresh `codex --no-alt-screen -C <workspace> [PROMPT]`, including observed fresh `-m <model>` variant with configured root hooks/receiver. Product parsing must conservatively preserve argv/native behavior; unsupported forms disable reporting. Resume, picker/last, fork, in-process switch, compaction recovery and remote endpoints are not enabled. No resume was invoked.

Passed source observations: OS attribution, matching fresh paginated header, two turns, permission boundary/allow, child isolation, cancellation, error, absolute context and exit-token references, cleanup. Synthetic foreign-first and cumulative replay replacement passed; earlier raced foreign-first assertion failed and remains disclosed. Runtime parser/order/freshness tests remain required. No Linux native run, current-checkout GUI build, native disconnect/recovery, resumed history, compaction, Codex 50-session scenario, hosted capacity or Rust suite ran. Unsupported/deferred/downstream gates are not passes.

Final receiver restart first failed sandbox socket bind with PermissionError; scoped escalation succeeded. All 18 recorded process IDs were absent afterward, auth and observer socket absent. The original unauthenticated attempt's unknown PID cleanup is unchanged. No further native case or general research followed the error case.
