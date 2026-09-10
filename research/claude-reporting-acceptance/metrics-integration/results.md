# Receiver, collector and status-line integration

Integration builds on the accepted reporting/activity foundation, collector API06bed526 and committed helper correction55857023. The coordinator confirmed independent helper-correction acceptance before the integration commit; this integration still requires its own exact review. Commands ran against the integration working tree while independently owned setup/inspection work was also present. Only runtime agent_runner, root reporting/admission methods, ReportCommand/CLI report handling, owning CLI/lifecycle tests and this evidence belong to the integration commit. Collector, setup, inspection, dashboard and coordinator evidence remain separately owned.

## Contract and behavior

- Runtime transports opaque Request/Poll/NativeCompleted lifecycle events. The root receiver alone owns binding, lease, collector and independent activity/metrics/health revisions. Polling is capped at10Hz; helper advance is nonblocking. Native completion is signaled with one absolute2s deadline measured at native exit observation, prioritized over new requests. A stalled callback cannot cause an unbounded join or change the native exit status.
- Start the collector only from the exact initial admitted hook transcript path; later status lines/startup duplicates cannot select a path. Missing initial file is retried at that pinned path: context and observed activity still work while source health is unavailable, then recover on successful read. A hook with no transcript path starts no collector; activity/context may work and cumulative usage stays unknown/Partial. Connected denotes transport, not complete accounting.
- Context/cost and recognized-root-transcript usage remain Uncertain, with no fabricated source identity or sequence. Updating one component retains the others verbatim. Replay does not increment metrics revision, source-health changes do not freshen components, and finalization preserves component ages. Unknown cost does not erase the retained decrease watermark. Coverage is always Partial; excluded child/auxiliary usage and complete final accounting are not certified.
- Source errors affect metrics diagnostics/health without closing working activity or context. Ambiguous identity and control-delivery failure close admission permanently. Clear cancels the reader, preserves the healthy lease until native completion, and rejects subsequent status lines or reader reopening.
- Native completion drains within a bounded portion of the same deadline, cancels the owned helper, and reserves700ms for partial Finalize/control recovery. Finalize atomically releases the lease. Lost acknowledgement queries the original operation receipt on a separate bounded connection without reattaching a released watch. Failed control delivery falls back to watched disconnection; EOF/Stop is never treated as complete source accounting.
- `report claude-statusline --stdin-json [--render-command <shell command>]` renders default context independently of report success. A custom command receives original bytes once through a streaming tee, including malformed and oversized input; only64KiB is retained for reporting. Networking uses the original1s deadline and is skipped if expired. The user renderer keeps its own normal stdin/runtime semantics and stdout; reporting failure cannot suppress it. RawValue framing preserves original decimal cost lexemes. The documented legacy renderer remains unchanged outside this new route.

The version probe's visibility changed to public bool solely for setup/doctor reuse; capture semantics were not expanded. No setup, UI, source-order certification, confirmed settling or complete-accounting claim is part of this unit. Native model_not_found observation is separately retained by the coordinator in attempt11; other API failure categories remain source/synthetic-specific.

## Verification attempts

All commands below use `rtk proxy`; real socket/PTY/process gates use required execution permission. Logs retain failures separately from passing attempts.

| Log | Command after prefix | Exit/result |
| --- | --- | --- |
|01-statusline-red | cargo test -p ovrcr --test cli claude_statusline -- --nocapture |101, concurrent dashboard import compilation failure; not product RED |
|02-statusline-red | same |101, product RED:2 new CLI behavior assertions failed before route existed |
|03-statusline-green | same |0,2 passed |
|04-route-red | cargo test -p ovrcr --test server_lifecycle claude_metrics_route_collects_partial_usage_and_finalizes_native_exit -- --nocapture |101, product RED: actual statusline/collector metrics absent;0 passed/1 failed |
|05-compile | cargo check -p ovrcr --all-targets |0 |
|06-route-green | cargo test -p ovrcr --test server_lifecycle claude_metrics_ -- --nocapture |0,2 passed |
|07-statusline | cargo test -p ovrcr --test cli claude_statusline -- --nocapture |0,3 passed |
|08-route-recovery | cargo test -p ovrcr --test server_lifecycle claude_metrics_ -- --nocapture |0,3 passed |
|09-stalled-callback | cargo test -p ovrcr --test server_lifecycle native_exit_is_bounded_when_reporting_callback_stalls -- --nocapture |101, concurrent setup module declaration preceded file; not product failure |
|10-metrics | cargo test -p ovrcr --test server_lifecycle claude_metrics_ -- --nocapture |0,4 passed |
|11-stalled-callback | cargo test -p ovrcr --test server_lifecycle native_exit_is_bounded_when_reporting_callback_stalls -- --nocapture |0,1 passed; actual native exit17 preserved within2s plus test allowance |
|12-statusline | cargo test -p ovrcr --test cli claude_statusline -- --nocapture |0,3 passed; includes original bytes/decimal and default/custom output after network timeout |
|13-existing-lifecycle | cargo test -p ovrcr --test server_lifecycle agent_ -- --nocapture |0,13 passed/2 ignored helper entry points |
|14-report-library | cargo test -p ovrcr --lib report:: |0,20 passed |
|15-clippy | cargo clippy -p ovrcr-runtime -p ovrcr --all-targets --all-features -- -D warnings |0; affected compilation/lint |
|16-fmt | cargo fmt --all -- --check |1, coordinator SessionCommand formatting only |
|17-diff | git diff --check |0 |
|18-activity | cargo test -p ovrcr --test server_lifecycle private_claude_activity -- --nocapture |0,2 passed |
|19-cli-hooks | cargo test -p ovrcr --test cli agent_hook |0,4 passed |
|20-fmt | cargo fmt --all -- --check |0 after owning worker formatted SessionCommand |

Metrics lifecycle assertions cover actual CLI/hook/helper/runtime publication, missing-file recovery while Busy/context remain available, replay/independent ages, wrong conversation rejection, unknown-cost watermark protection, clear preventing later component changes, native partial finalization, and explicit observation of the matching retained Finalize receipt after a dropped acknowledgement. The separate collector unit owns stopped-helper, file mutation/rebuild, accounting cap and expired-cancel cleanup evidence. This integration additionally forces a blocked reporting callback through actual native supervision and verifies exit17 and bounded return. macOS automated evidence does not certify Linux or live provider final accounting.

### Helper correction integration follow-up

- Helper correction55857023 was committed without API changes. Log21 reran metrics4: exit101,1 passed/3 failed at the fixture's2s ADMISSION_READY wait with empty terminal output, before any root callback or collector start. Cause is not asserted as a helper/product failure. Log22 diff check passed.
- The metrics fixture startup observation allowance is now5s; the actual native-completion assertion remains below2.5s including test observation allowance for the2s reporting deadline. Log23 reran the same metrics filter: exit0,4 passed,2.36s total suite time.
- Log24 `cargo fmt --all -- --check` and log25 `git diff --check`: both exit0. No previously green unrelated suites were repeated.

Log26 staged diff check exited2 on trailing whitespace/blank EOF copied from tool output. Text logs now normalize only trailing whitespace and final blank lines; commands, results and failure content are retained. Log27 staged diff check passed after this evidence-only normalization.
