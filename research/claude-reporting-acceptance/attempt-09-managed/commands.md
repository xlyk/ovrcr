# Managed native attempt 09

Tested source: `665831a2a43a82592591c41c883bc9de168237dc`, clean before fixture execution. macOS; no GUI acceptance claim.

All commands were prefixed with `rtk proxy`. `cargo build -p ovrcr --bin ovrcr` exited0. Dedicated config/socket/repository/worktree under `/private/tmp/ovrcr-claude-managed-20260910`; no user server or agent settings changed. Existing subscription login was supplied privately in memory, as previously authorized.

1. `python3 launch-server.py`: foreground server tool session63949, PID7086 / launcher PGID7084.
2. `ovrcr project add fixture <fixture>/repo --workspace-root <fixture>/workspaces`: exit0.
3. `ovrcr workspace create --project fixture --name native --new-branch fixture-native --base main`: exit0; owned idle shell PID/PGID9760.
4. `ovrcr terminal create --project fixture --workspace native --name claude -- python3 <fixture>/launch-claude.py`: exit0, session2, supervisor PID/PGID11124. Native argv: `ovrcr agent run --provider claude -- claude --setting-sources '' --settings <fixture>/settings.json --strict-mcp-config`.
5. First attempt reached the workspace trust prompt. `terminal send` pasted an arrow sequence then submitted, exiting native with code1 before binding. This is failed fixture interaction, not a successful admission run. Inventory confirmed agent=None and exit1.
6. Repeated the same create command as claude-retry: exit0, session3, supervisor PID/PGID25660 and its directly observed child Claude PID/PGID25663.
7. Fixture dashboard client used DashboardHello, Select(session3,40x120), then native Input down and enter in separate acknowledged requests. Native trusted only the fresh disposable worktree. The client disconnected afterward, leaving native/reporting running.
8. Read-only Inspect matched native SessionStart(startup) conversation-A to runtime binding generation1 and Connected health. Activity/metrics were absent as expected for this slice.
9. `terminal send 3 --text 'Reply with exactly OVRCR_MANAGED_OK. Do not use tools.'`: exit0. Native response marker observed separately from command echo; saved excerpt.
10. `terminal send 3 --text /clear`: exit0. Actual SessionEnd(clear) and SessionStart(clear) switched native to conversation-B. Runtime retained conversation-A generation1, Unavailable(identity_transition_unavailable), native Running. This does not claim support for new conversation binding or prove private lease retention independently; the real socket regression covers the latter.
11. `terminal send 3 --text /exit`: exit0. Runtime session3 Exited(code0); old binding retained with Unavailable(unfinalized_release). No usage-finalization claim.
12. Authorized kill0/killpg0 checks found the recorded native/supervisor PIDs/PGIDs absent.
13. `ovrcr shutdown --kill` against only dedicated config/socket: exit0. Server session63949 completed exit0 with server stopped. Server/launcher/idle-shell cleanup checks found recorded IDs absent and socket removed.

Captures retain only hook identity fields and full field-name inventory, never prompt/transcript bodies or credentials. Conversation/invocation/prompt identities are consistently pseudonymized. No provider API settling, metrics, Linux, native GUI or complete Task3 acceptance is established by this attempt.
