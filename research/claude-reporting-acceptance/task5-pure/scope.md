# Task 5 pure metrics unit

Owned source: src/report/claude_metrics.rs. This file was tested as the library target of the saved disposable Cargo manifest before module wiring. It uses the existing protocol crate and existing dependencies, with the coordinator-approved serde_json raw_value feature. No live reader, transcript path validation, runtime publication or provider replacement certification is included.

Evidence baseline: bbd76dc0db534989c76765fa2b227ea40b8c7403 plus the uncommitted owned file. Parallel workers may advance HEAD; these are pure-unit results, not exact-commit integration acceptance.

01: compiling initial stubs, 5 tests ran, 4 failed behaviorally and 1 passed. 02: parser/replacement implementation without cap enforcement and invalid-record diagnostics, 9 tests ran, 4 failed behaviorally. 03: 9 tests passed, including a 65,536-identity case. 04: standalone all-target clippy passed with warnings denied. No workspace tests were claimed.

Status-line measurements retain uncertain freshness and provider-estimated conversation cost; token snapshots are always partial. The accumulator charges boxed key lengths plus key/value storage, without counting BTree node overhead as charged bytes. It freezes all later updates after the next valid insertion would exceed either 65,536 identities or 16 MiB. Rejected malformed/overflowing records leave numeric state unchanged and retain a diagnostic. Non-assistant record classification belongs to the future reader.

No commit was made; coordinator owns wiring, independent review and delivery.

Coordinator integration: added existing serde_json raw_value feature (no new dependency) and cfg(test) module registration after Task4 commit731d5a8. This arithmetic remains unavailable to production callers until reader/statusline integration. Owning crate `cargo test -p ovrcr --lib report::claude_metrics` passed9; root library/tests all-feature Clippy and workspace fmt check passed. Commands and results saved05–07. No runtime publication or live-reader claim.
