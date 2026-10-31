# Linux outer-PTY startup abort: bounded audit

Scope: existing logs, fixture/dependency source and installed Rust 1.98.0 source only. No test/build, process inspection, signal, UI, network operation or repository edit. The full Cargo 57 run was still running when this investigation was assigned; this report does not declare its final outcome.

## Observed boundary

`57-linux-workspace-private-final.log:1223-1236` records `copy_mode_acceptance_emits_selected_text_and_reattaches` failing with:

> outer dashboard closed its PTY before readiness; child_status=Some(Ok(Some(ExitStatus { code: 1, signal: Some("Aborted") }))); size=(30, 100), alternate_screen=false; screen: fatal runtime error: assertion failed: output.write(&bytes).is_ok(), aborting

The fixture had already started server 56928 under `/tmp/.tmpuejkbG` and recorded managed groups `[56993, 57029, 57048, 57073]`. Its first `OuterDashboard::start` failed: the subsequent outer PID receipt at `tests/terminal_acceptance.rs:2320` never printed, nor did the first-detach receipt. Therefore Copy input, selected-text output and reattach assertions were not reached. The terminal-acceptance binary reported 18 passed / 1 failed / 1 ignored; those are that binary’s results, not the still-running full gate.

`50-linux-workspace-final.log:1242-1254` contains the same fatal expression in a different case, `copy_history_acceptance_emits_across_page_and_tile_boundaries`, also before alternate-screen readiness, under `/tmp/.tmpTauhPU` and size `(30,180)`. The Copy case passed in that earlier run; the History case passed in 57. This shared startup symptom across different case names does not establish a common original errno or its cause.

## Exact Rust source

Offline primary source is installed at:

`/opt/homebrew/Cellar/rust/1.98.0/lib/rustlib/src/rust/library/std/src/sys/process/unix/unix.rs`

Homebrew’s installation receipt identifies stable 1.98.0. Existing Linux `/private/tmp/ovrcr-384-evidence/linux-target/.rustc_info.json` identifies `rustc 1.98.0 (88d9e12ae 2026-08-18)`, full commit `88d9e12ae178fab0fb5cc050a94da85685d449ea`, host `aarch64-unknown-linux-gnu`. No additional compiler invocation was performed. The upstream location is `library/std/src/sys/process/unix/unix.rs`, in `Command::spawn`, line 120 of the installed release source.

The relevant sequence is:

- Lines 76-80: Linux creates a private AF_UNIX/SOCK_SEQPACKET startup channel; other Unix targets use a pipe.
- Lines 94-104: after `fork`, only the child branch calls `do_exec`; successful exec does not return.
- Lines 104-120: if `do_exec` returns `Err`, Rust forms an eight-byte packet: four big-endian errno bytes plus `NOEX`. It then asserts `output.write(&bytes).is_ok()`.
- `std/src/rt.rs:53-66` expands a failed `rtassert!` to the exact `fatal runtime error: assertion failed: output.write(&bytes).is_ok(), aborting` message and `process::abort()`.
- Parent-side lines 139-150 normally distinguish EOF (successful spawn) from the eight-byte error packet (return the original OS error).

Thus **`output` here is the child’s private exec-error reporting channel, not Dashboard rendering output**. The text establishes a failed `do_exec` followed by failure to report that error. It does not expose either the original setup/exec errno or the reporting-write errno. `do_exec` includes stdio duplication, cwd/identity/process setup, pre-exec callbacks and finally `execvp` (`unix.rs:283-415`); the expression alone does not prove `execvp` was the operation that initially failed.

Local source SHA256 receipts: `unix.rs` = `19eb534c196a88289e6d65f56af0b1c584f294cc67e80bf984be9f2b2cb9e926`; `std/src/rt.rs` = `f435576c645fb30eaca560e819c2a113b556452d79c0303f49ffd4487018c01f`. These identify the offline source inspected, not an independent byte comparison with the container’s installed source.

## Fixture and dependency path

`OuterDashboard::start_with_server_executable` (`tests/terminal_acceptance.rs:382-445`) opens the real PTY, reuses/copies the compiled CLI into a private executable image, builds a portable-pty command with isolated fixture environment (`tests/support/live.rs:163-190`), sets the reviewed server executable and fixture Dashboard config, and calls `pair.slave.spawn_command(command)?`. It starts a reader and calls `wait_for_startup` only after that spawn returns a child.

Readiness specifically requires alternate-screen mode plus the Dashboard header (`terminal_acceptance.rs:450-459`). PTY disconnection produces the observed error and wait status (`lines 479-526`). The recorded false alternate-screen flag and missing post-start receipt locate this failure before the test drives Copy or retarget behavior. They do not independently prove the intended executable’s main function ran.

Cargo.lock pins portable-pty 0.9.0. Its Unix spawn path (`portable-pty-0.9.0/src/unix.rs:229-287`) attaches stdio to the slave and installs a `pre_exec` callback that resets signals, calls `setsid`, assigns the controlling TTY, then calls `close_random_fds`. That helper (`lines 152-177`) enumerates `/dev/fd` and closes every descriptor above 2 when enumeration succeeds; it does not preserve Rust’s private startup channel explicitly. Rust declines posix_spawn when a pre-exec callback exists (`std unix.rs:467-474`), so this path uses the fork branch above.

This supplies a concrete source-level mechanism by which an already-closed error-reporting descriptor could turn a later setup/exec error into this fatal assertion, while the parent sees EOF and returns a child for readiness monitoring. It is a source-supported explanation of the **reporting boundary**, not proof of the observed descriptor state or the original error. The log has no FD/errno trace or stack identifying the emitting spawn instance. The direct outer-child Aborted status is consistent with the fork child aborting before the copied Dashboard executable successfully execs; no Dashboard render/input stack is established.

## Conclusion and limits

Yes: the exact message is Rust child setup/exec-error reporting, and should not be interpreted as a Dashboard output-write assertion. The fixture’s first Dashboard start failed before behavioral acceptance began. Neither this text, the different failing History case in 50, nor unchanged Rust/product source since the reviewed test revision establishes the initial cause. Missing executable/path/cwd, permissions, resource pressure, an exec race and a product regression are not diagnoses supported by these logs. No product fix, fixture change or deadline/assertion change is justified by this audit alone. Recovering the original `do_exec` errno and failed reporting write’s errno at this spawn boundary would be the discriminating evidence; no additional experiment was run here.
