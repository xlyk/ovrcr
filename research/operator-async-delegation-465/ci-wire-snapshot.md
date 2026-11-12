# OVRCR protocol wire fix evidence

- Commit: `cdfc5ca70783da7267ec66329f948555b15ab577`
- Parent/integration: `8c4d1ab3e043c2f8dbd589e3b2e89bde78a0ad05`
- Updated only the protocol-85 `AbortTurn` expected bytes: actual `...020102030400`, expected was `...0201020304`.
- `cargo test -p ovrcr-protocol wire_encoding_matches_snapshot_for_protocol_version` — passed (1 test).
- `cargo fmt --all -- --check` and `git diff --check` — passed.
- Worktree is clean; protocol version remains 85.
