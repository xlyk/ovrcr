# 50-session reporting capacity fixture

The ignored integration test `fifty_session_reporting_capacity` uses an isolated real server, Unix socket protocol, 50 live PTY shell sessions, 50 watched supervisor reservations and bindings, and 50 actual collector helper processes. No paid provider runs or authentication are involved. The server runs as a thread in the test process; aggregate memory includes that process plus every descendant.

Each helper reads the same exact synthetic root transcript into its own independent production index. The file has 65,537 unique recognized assistant usage records, so each helper reaches the 65,536-identity cap and freezes with `accounting_limit`. Every returned fill snapshot is checked against both production accounting caps (65,536 identities / 16 MiB charged keys and values). Sharing an input file does not share the helpers' memory indices. This fixture exercises identity-cap occupancy; it does not simultaneously fill every helper's 32 MiB pending raw-record buffer.

The load publishes 600 normalized Metrics reports per session at scheduled 100 ms intervals during a fixed 60-second window: 30,000 accepted updates total. It sends 60 additional stale-invocation reports per session and requires rejection. All metrics retain uncertain freshness and partial usage. At tick 300 the harness SIGKILLs five owned helpers, observes controller failure, and publishes explicit unavailable health. This establishes failure traffic capacity, not the launcher's automatic failure propagation (covered separately by integration tests). Overflow health is likewise published by the harness after observing the actual collector diagnostic.

A separate control connection repeatedly requests Inspect while traffic runs. Every observed metrics bundle is checked against its session's unique model and conversation. Final snapshots must contain revision 600 and all 50 expected models, conversations, context values and unavailable reasons. The harness records every control acknowledgement latency, per-session maximum scheduling lateness, every collector accounting counter, and all owned PTY/helper PIDs. It verifies helper reaping while the parent remains alive and PTY absence after server shutdown.

RSS and CPU are sampled with `ps` once per second during filling/loading. RSS is summed over the test process and every descendant; the same 50-empty-PTY baseline is taken before helper creation. CPU is the native ps lifetime-average %CPU sum, not an instantaneous interval CPU measurement. The recorded process set includes the short-lived ps probe. Peak sampled growth must remain below 3 GiB; sampling cannot prove no sub-second transient peak. Control thresholds remain p99 < 250 ms and maximum < 1 s without revision.

Internal server/controller queue counters are not exposed by the current interfaces, so this fixture cannot certify their requested numerical caps. It uses one outstanding control/report per worker and the production controller's one-in-flight API, but those usage patterns are not internal counter measurements. This gate remains explicitly open. This is synthetic macOS capacity evidence, not 50 native Claude sessions, full provider accounting, or Linux acceptance.

## Run history

- run01: harness failed because it reused a one-request control connection.
- run02: harness failed because it dropped a watched reservation connection before transferring ownership; retained default workspace shell also made a raw inventory count 51.
- run03: ended bootstrap shell remained in inventory. Final harness filters the 50 explicit load names and records 50 live PTY PIDs separately.
- run04: 60 seconds of traffic completed, but teardown failed for all 5 deliberately killed helpers. It is not a passing acceptance run. Raw 2380 control samples have p99 1.244ms/max5.539ms; baseline 127168 KiB, peak 1546528 KiB. Later isolated diagnosis found macOS EPERM for zombie-only groups; Drop reaped the child. The focused permanent regression and collector correction are recorded in task5-collector/evidence 15–17. Failed evidence is retained.

## Final measured acceptance: run05

PASS for the measured subset after collector correction `e1bbe4baebb513eec6090f179f93cdcba14cd4be` was independently accepted. The binary was built at HEAD `2c432e001ce540d3616e64bf95e5df204afdac99` with the concurrent uncommitted final-drain change in `src/report/admission.rs`; build metadata records that state explicitly. This harness directly exercises the server protocol and collector, not admission/finalization. Darwin 25.5.0 arm64, Rust 1.98.0; exact uname/compiler output is retained.

All 30,000 updates completed inside the 60-second window; 3,000 stale reports were rejected. Maximum scheduling lateness was 5584 microseconds. All 50 final snapshots matched revision 600 and their own binding/model/context/health. All 50 independent helper indices reached 65,536 identities, with at most 7973172 charged bytes each; overflow state was explicit. Five deliberate helper failures were observed and reported unavailable.

The 2,388 control samples had p99 1.205 ms and maximum 2.822 ms, below the unchanged 250 ms / 1 s thresholds. Baseline RSS was 127088 KiB; peak sampled RSS 1546480 KiB; growth 1419392 KiB (1.354GiB), below 3 GiB. Peak recorded ps CPU was 1165.9% during filling and 6.0% during loading; these values use the ps lifetime-average semantics described above.

All 50 helpers were successfully cancelled and reaped while the test parent remained alive. All 50 recorded PTY PIDs were absent after the server acknowledged shutdown. One integration test passed in 67.32 seconds including fixture setup/fill/cleanup. Owned clippy with warnings denied and rustfmt checks also passed.

The internal queue-counter gate remains OPEN. No complete Task7/platform/provider certification is claimed. Raw latency, resources, accounting counters, PIDs, per-worker scheduling and cleanup results remain in run05 files; result.json provides a machine-readable summary.
