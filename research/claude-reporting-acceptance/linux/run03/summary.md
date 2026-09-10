# Linux baseline acceptance — attempt 03

Product source: `cad39abfbe955a304c2231a294debdc888b47c30`, exported through `git archive` into an isolated container. Task 1 descendants through `e812d39` change documentation only. No host mounts, provider credentials, or paid native provider runs were used.

Environment: Linux aarch64, Rust 1.98.0, Debian bookworm image, Docker `--init`, unprivileged `ovrcr-test` user, SHELL=/bin/bash, installed zsh, fixture Git identity. This is real Linux execution, not cross-compilation or an Ubuntu-hosted CI result.

- `cargo test --workspace --all-targets`: 562 passed, 0 failed, 11 ignored across 23 binaries. Real launcher PTY/signals, stopped-group lifecycle, admission replacement, callback deadline, collector truncation/crash/cancellation/reaping, and post-native drain tests executed. The raw log identifies every test.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- `cargo test --workspace --doc`: exit 0, zero doc tests defined/executed. This is not additional behavioral coverage.
- The CLI binary first passed 28/28 after correcting the environment. Process inventory after the gates contains only fixture init/sleep and inspection; the container remains for capacity and updated-source acceptance.

Attempts 01 and 02 remain preserved. Missing reaping init, missing SHELL, and root bypassing EACCES were corrected in the runner without changing or weakening tests. The exact initially failed descendant-cleanup test passed with init, and the CLI suite passed as an unprivileged user.

The explicit 50-session capacity run is pending Task 2 counter instrumentation. These baseline results do not cover subsequent queue changes. Final acceptance must name its actual revised source; hosted Ubuntu CI remains unrun.
