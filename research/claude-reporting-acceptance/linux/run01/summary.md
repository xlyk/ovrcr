# Linux attempt 01 — environment failure

Tracked product snapshot: cad39abfbe955a304c2231a294debdc888b47c30 (later Task 1 commits change documentation only). Command: `cargo test --workspace --all-targets`. Exit 101; first binary executed 21 tests: 20 passed, one failed. Later binaries did not execute.

Failure: `report::admission::tests::initial_admission_probe_cleans_descendants_after_leader_exit`, admission.rs:1095, `version probe left its descendant after leader exit 0`.

Container was launched without `--init`; PID 1 was `sleep infinity`. After failure, ps showed orphan `sleep` PIDs 5732 and 5745 with PPID 1 and status Z. The process-group absence assertion therefore remained false despite the process being killed. This points to missing init reaping in the test environment; no application assertion was weakened. Retry uses a reaping container init and first reruns this exact failing test. The original failed log remains preserved.
