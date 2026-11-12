# Native Operator orchestration proof

Source revision: `1dc1e248f032d04090133ef7177fb620e3551254`.
Installed Pi 0.87.1, unmodified; one local scripted streaming provider. The GUI
and matching CLI were built from the integration checkout after compiling the
ignored helper. HOME, sockets, workspaces and provider profiles were disposable.

- Two real managed Pi sessions (13/14), separate Git worktrees, connected Reporter.
- Operator read the bundled coordinator skill, captured pre-send baselines,
  Pasted tasks, registered watches, returned a mapping, and freed the composer.
- Ctrl-x interrupted a held reply. The following warm chat passed the provider’s
  assertion that the interrupted prompt/partial reply was absent from context.
  Operator PID 21898/run 1 stayed unchanged across dispatch, Stop and warm chat.
- After both delegates were released, Reporter revisions advanced 1→6. One
  AsyncWakeup contained both reports; the hidden dock showed 1 unread.
- Reopening showed a clean Operator heading and preserved
  `NATIVE_465_UNSENT_DRAFT` entered through Quick message. Both sessions remained
  running/Ready with two README insertions each; both diff checks exited 0.
- The durable journal has four turns on run 1: Complete, Interrupted, Complete,
  Complete; exactly one AsyncWakeup. Final completion released the lease and
  Operator exited. Both delegates stayed open until fixture cleanup.
- Provider/helper and GUI launcher exited 0. Fixture root/socket were removed;
  all 15 recorded owned process groups were absent.

[Dispatch](01-dispatch.png) · [Stopped turn](02-stopped.png) ·
[Warm chat](03-warm-chat.png) · [Hidden unread](04-hidden-unread.png) ·
[Wakeup and draft](05-wakeup-draft.png). Matching AX text is beside each image.
JSON snapshots, Git checks, journal summary and redacted provider requests retain
the underlying observations.

The final run also verified the corrected interruption row: “operations may
finish; file changes remain.” [Before the copy fix](00-interrupted-before-fix.png)
is retained as the finding reproduction. The final source includes the identity
fence extraction and legacy-history reader fix; the latter is covered by the
schema-32 production migration/read regression.
