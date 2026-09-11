# Supervisor disconnect proof follow-up

At `b8fce8c`, the existing real ownership-guard regression additionally asserts the exact `supervisor_disconnected` diagnostic. Independent review approved the committed unit. The retained [terminal capture](supervisor-disconnect.txt) is RTK-compressed, not raw Cargo output: one passed, 98 filtered out, exit 0.

Command:

```sh
rtk proxy script -q /tmp/ovrcr-supervisor-disconnect-20260910.txt rtk cargo test -p ovrcr-runtime server::tests::agent_reporting::agent_report_supervisor_connection_loss_and_late_cleanup -- --exact --nocapture
```

Format and scoped whitespace checks passed. The socket-owner disconnect path is covered; a literal supervisor process-kill and native exec descriptor-inheritance check remain separate unverified assertions.

The repository copy removes only the leading terminal EOF echo (`^D` and two backspace bytes) and normalizes CRLF to LF. The RTK test summary is unchanged; the original terminal capture remains at the command path above.
