# macOS instrumented capacity attempt

Source: `772ab63`, branch `codex/claude-reporting`, feature worktree `.worktrees/claude-reporting`. Product code is `950710a`; the child commit removes only artificial dashboard-reader throttling from this acceptance test. Both units independently reviewed.

Host: macOS26.5.2 build25F84, aarch64, Rust1.98.0 (88d9e12ae, Homebrew), 14 logical CPUs, 51,539,607,552 bytes physical memory. `kern.ipc.somaxconn=128`. Cargo dev/test defaults, `acceptance-diagnostics` feature. GUI was built before this attempt; no native provider or GUI fixture was launched during load.

The test uses isolated temporary config, socket, repository, workspace, 50 PTYs and 50 helper processes. Existing unrelated sessions were left running and untouched. The recorded baseline is the same 50 empty sessions and active dashboard before helper fill. RSS samples include the test/server process and descendants and cannot establish sub-second peaks or full allocator/kernel worst-case memory. Numerical retained payload counters exclude allocation capacity and kernel buffers as documented in the inventory.

The raw log and exit artifact own the result; this provenance does not itself assert success. The earlier failed `950710a` attempt remains beside this directory.
