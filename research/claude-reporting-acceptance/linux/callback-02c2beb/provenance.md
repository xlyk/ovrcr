# Callback Linux verification

The runner retained the tracked cad39ab snapshot used by baseline run03. `git show 02c2beb:crates/ovrcr-runtime/src/agent_runner.rs` supplied the single changed Rust file; no uncommitted queue instrumentation was copied. This matches 02c2beb product source (intervening commits affect only documentation/CI/evidence).

Unprivileged Linux aarch64 with --init and SHELL=/bin/bash: `cargo test -p ovrcr-runtime invocation_channel_` executed 2 tests, both passed, 88 filtered. Raw stdout and exit0 are retained. This is focused callback verification, not final workspace or queue acceptance.
