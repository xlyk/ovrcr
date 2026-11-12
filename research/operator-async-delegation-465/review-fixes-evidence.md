# OVRCR review fixes evidence

- Worktree: `/private/tmp/ovrcr-465-review-fixes`
- Base: `a6b508b47684ef1d2134d00ab396af8d3be67bbf`
- Commit: `1dc1e248f032d04090133ef7177fb620e3551254`
- The base is an ancestor of the commit; worktree is clean.

## Changes

- Replaced the interrupted-history “no rollback” claim with external-effects wording.
- Corrected Pi/OVRCR attribution for the four native tool factories and custom `watch_sessions` tool.
- Centralized the run-to-turn conversation/session/run comparison on `OperatorRunIdentity`; kept the async-wakeup check separate and covered all four Reporter response cases.
- Added storage-boundary decoding for the old `{"Worker":"Completed"}` literal as `Completed { leased: false }`. A schema-32 fixture confirms migration and production `SessionStore::open`/history reads preserve the original message JSON and batch receipt digest.

## Checks

- `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target cargo test -p ovrcr-protocol operator` — 5 passed.
- `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target cargo test -p ovrcr --lib operator_fences_bootstraps_and_validates_exact_journal_receipts` — 1 passed.
- `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target cargo test -p ovrcr-tui dashboard::operator` — 32 passed.
- `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target cargo test -p ovrcr-runtime schema_32_migration_preserves_legacy_completion_and_turn_dependents` — 1 passed. Before the fallback, this reproduced `invalid type: unit variant, expected struct variant`.
- `rtk cargo fmt --all -- --check` and `rtk git diff --check` — passed.

Root owns the final native rerun, final-head CI, diary, and PR steps.
