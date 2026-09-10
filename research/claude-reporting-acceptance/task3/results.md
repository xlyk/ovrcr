# Task 3 partial mechanics acceptance

Base: `8108941` (coordinator evidence). All saved commands test that base plus the Task 3 working tree; the resulting source commit is the review unit. This is the approved mechanics-only slice, not completed provider admission.

Implemented `ovrcr agent run --provider claude -- <native argv...>`. The CLI reserves on a watched socket before spawning, holds the lease outside the native environment, and releases the unbound reservation on normal exit or spawn error. Missing identity/server runs native with one reporting-unavailable diagnostic and never starts a server. An explicit reservation conflict refuses native spawn. Successful reservation prints admission-unavailable. No command injects provider arguments, binds a conversation, reads transcripts, or publishes provider observations.

The runtime owns a direct Child and its new process group. It inherits stdio and the existing terminal, transfers foreground ownership before exec, forwards launcher signals to that group, handles stop/continue, and restores terminal ownership/modes. It retains the unreaped child as the process-group ownership anchor until group cleanup. It does not use the Pi process scanner or create another PTY.

Native children receive `OVRCR_AGENT_SOCKET` and `OVRCR_AGENT_TOKEN`; `OVRCR_HOOK_SOCKET`, `OVRCR_SESSION_ID`, and `OVRCR_HOOK_TOKEN` are removed. The private socket lives in a 0700 directory with 0600 mode, accepts only a bounded token handshake, and returns `admission-unavailable` for authenticated requests. Wrong tokens are rejected. The report CLI selects this route before legacy runtime reporting. The receiver uses a 200 ms absolute authentication deadline and bounded response deadline; there is no collector queue or provider parser in this slice. Normal launcher cleanup removes the endpoint.

## Behavioral evidence

- 01: real CLI RED, missing `agent` subcommand. 02: one passing exact argv/stdin/stdout/stderr/exit-7 test.
- 03: real PTY RED, native did not own its process group. 04: one passing process-group/foreground/Ctrl-C/forwarded-TERM/restoration test.
- 05: one passing expanded real Ctrl-Z/continue and runtime pause/resume test.
- 06: real PTY RED, private invocation endpoint absent. 07: one passing reservation-before-ready, stripped capability and authenticated unavailable endpoint test.
- 08: real PTY RED, native terminal modes lost after stop/continue. 09: attempt failed because the fixture initially used the wrong report CLI syntax (`busy` instead of `--state busy`); the retained output is not a product failure claim. 10: two passing corrected lifecycle tests, one ignored child fixture.
- 11: final lifecycle assertions passed: 2 tests, 1 ignored helper (the helper executes inside the native PTYs). Assertions cover child-owned PGID, foreground ownership, kernel 40x120 terminal size, Ctrl-C counted exactly once, native input after interrupt, Ctrl-Z/continue, actual runtime pause/resume/kill, launcher-targeted TERM and exit 23, native and shell termios, authenticated/incorrect-token private requests, actual report CLI refusal, active reservation conflict with no second native output, normal release, failed-spawn release, next invocation at epoch 3, no binding, native PGID absence, and removed private socket. The log records owned native PID/PGID and launcher/session PID.

## Affected gates

- 12: CLI suite, 24 passed. Includes missing environment/server without autostart, native help/version arguments passed through, and native signal exit preserved.
- 13: report library suite, 3 passed.
- 14: runtime library suite, 90 passed.
- 15: affected packages check, all targets/all features, passed.
- 16: affected packages clippy, all targets/all features with warnings denied, passed.
- 17: formatting and diff checks recorded separately.

Tests needing real sockets/PTYS used authorized execution permission with isolated fixture paths. Linux and native GUI acceptance were not run. No full workspace regression was repeated for this slice. Native provider admission/routing eligibility, transcript accounting, setup/doctor, and provider publication remain outside this unit. SIGKILL cannot execute user-space terminal or filesystem cleanup; the existing runtime kill path is tested to remove both owned process groups.
