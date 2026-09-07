# Task 1 report: protocol and terminal foundations

Base revision: `cde0ae9a72a36e8a8908344fe7e73e7f16ce8abe`

## Changes

- Added the Cargo workspace members `ovrcr-protocol` and `ovrcr-terminal`, resolver 3, inherited package metadata, shared third-party dependency declarations, and the two local package dependencies. `Cargo.lock` contains only the expected root dependency edges and the two new local package entries.
- Added `crates/ovrcr-protocol` with `context`, `registry`, `session`, `wire`, and `codec` ownership. Moved the shared session types, registry data and pure methods, context API, wire types, frame codec, codec tests, context tests, and pure registry tests without changing field order, enum order, serde defaults, frame limits, or redacted `AgentReport` debug output.
- Added `crates/ovrcr-terminal` with the existing `encode_paste` implementation and a pinned `vt100` re-export.
- Replaced the old root protocol/context implementations with thin root facades. Root session re-exports the shared session types. Runtime `Session::send_text` uses the terminal crate directly; TUI preserves its paste re-export for existing callers.
- Converted registry file operations to `load_registry(&Path)` and `save_registry_atomic(&Registry, &Path)` with the existing diagnostics, atomic temporary-file/rename/fsync behavior, and validation unchanged. Updated runtime, GUI, server lifecycle, and terminal acceptance call sites.
- Moved `DispatchMessage` to `src/server/dispatch.rs` and re-exported it from the server module. It is no longer a protocol type.
- No artificial RED tests, new behavior, unrelated files, diary entries, pushes, merges, or PRs were added.

## Baseline inventory

Command (exit 0):

```text
rtk proxy cargo test --all-targets --all-features -- --list
```

Output: `/private/tmp/ovrcr-task-1-scratch/baseline-tests.txt`

The baseline listed 169 test entries: root library 57, root main 0, GUI binary 2, CLI 18, Git lifecycle 13, GUI integration 3, resource CLI 4, server lifecycle 39, terminal acceptance 2, and TUI 31. Cargo's list output did not identify ignored tests; the final run confirmed the two intentional server lifecycle ignores: `hook_child_report_helper` and `pause_resume_child_fixture`.

## Verification

All commands below use the required `rtk` wrapper.

| Command | Exit | Result | Output |
| --- | ---: | --- | --- |
| `rtk proxy cargo check --all-targets --all-features` | 0 | completed | `/private/tmp/ovrcr-task-1-scratch/check-all-targets-all-features.txt` |
| `rtk proxy cargo fmt --all -- --check` | 0 | clean | `/private/tmp/ovrcr-task-1-scratch/fmt-check.txt` |
| `rtk git diff --check` | 0 | clean | `/private/tmp/ovrcr-task-1-scratch/diff-check.txt` |
| `rtk proxy cargo test -p ovrcr-protocol -p ovrcr-terminal` | 0 | protocol 12 passed; terminal 0 tests; doc tests 0 | `/private/tmp/ovrcr-task-1-scratch/test-protocol-terminal.txt` |
| `rtk proxy cargo test -p ovrcr --lib config::` | 0 | 3 passed; 42 filtered | `/private/tmp/ovrcr-task-1-scratch/test-config.txt` |
| `rtk proxy cargo test -p ovrcr --test tui` | 0 | 31 passed | `/private/tmp/ovrcr-task-1-scratch/test-tui.txt` |
| `rtk proxy cargo test --all-targets --all-features` (elevated, final) | 0 | 167 passed; 0 failed; 2 ignored | `/private/tmp/ovrcr-task-1-scratch/test-all-targets-all-features-elevated.txt` |

The final all-targets run passed root library 45, root main 0, GUI binary 2, CLI 18, Git lifecycle 13, GUI integration 3, resource CLI 4, server lifecycle 37 with 2 intentional ignores, terminal acceptance 2, TUI 31, protocol 12, and terminal 0 tests.

## Self-review

- Compared the moved session, wire, context, registry, codec, and dispatch declarations with the base sources. Public field/variant order, serde attributes/defaults, constants, diagnostics, atomic persistence sequence, process ownership, and redaction behavior are preserved.
- Searched the resulting tree: no old `Registry::load`/`save_atomic` call sites, duplicate root shared-type implementations, or protocol `DispatchMessage` remain. The only `DispatchMessage` owner is `src/server/dispatch.rs` with the server facade re-export.
- Cargo.lock dependency churn is limited to the two local crates and root edges. No protocol dependency on directories, TOML, runtime, or TUI code was introduced.

## Concerns

The first durable sandboxed rerun of the full suite exited 101 after 40 passed and 5 failures: three server socket tests failed with `Operation not permitted`, and two session process-group tests failed to observe expected zombie state. Its output is `/private/tmp/ovrcr-task-1-scratch/test-all-targets-all-features.txt`. These are sandbox IPC/PTY permission failures; the required elevated rerun passed all targets, and a read-only elevated process check found no leftover fixture processes.

