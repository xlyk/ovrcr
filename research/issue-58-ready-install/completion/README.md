# Issue #58 installed-native verification

2026-09-11: installation, configuration composition, native reporter trust and
installed-native acceptance passed. The normal protocol-4 server still owns five
pre-existing terminals; its transition requires their deliberate closure. Local
final checks and PR delivery are recorded below as they complete.

## Revision, installation and rollback

PR #56 [merged](pr56-merged.json) at
`cadb7a3174701e1666fa0562ebb714593f9d84f3` on 2026-09-11 at 19:52:37 UTC.
Its complete source tree equals reviewed head
`86ef46b8eff3128566f76022c2cfef49714031ac`; all four checks passed on that head
in [run 34572184009](https://github.com/xlyk/ovrcr/actions/runs/34572184009).
The coordinator merged main into the isolated issue branch at
`b2fc7d8aa3198881340457b40f773df85dcb8d3f`. No product code was changed for this
installation. The later fixture correction is described separately below.

Kyle authorized the remaining installation/configuration work with “56 is merged.
complete the entire list of required work.” [Installation metadata](installation.json)
records `/Users/xlyk/.local/bin/ovrcr`, version `0.1.0 (protocol 6)`, SHA-256
`663ca41654b694acc7cfb232c4fd8a0bb18d3169c07e4eeff4b649739d5585ae`.
It matches the reviewed release build. The destination was previously absent and
is on the current shell's PATH. [GUI bundle metadata](gui-bundle.json) records the
unchanged native GUI, whose companion executable resolves to this installed path.

Private rollback files are in
`/Users/xlyk/.local/share/ovrcr/rollback/issue-58-20260911` (directory mode 0700):
the original Codex config, unchanged JSON hooks, and the compatible old protocol-4
binary `ovrcr-protocol4`. Configuration backups are mode 0600; no credentials or
native trust hashes are committed. Follow the [rollback procedure](../../../docs/codex-ready-installation.md#roll-back-the-installation):
reverse only this installation's reporters if later settings changed, preserve
native trust decisions and drain sessions before switching server protocols.
Removing the newly installed binary restores its prior absent state; the saved
old client remains available. Rollback cannot recover a terminated PTY.

## Configuration and native trust

[Composition](configuration-application.json) added exactly five reporters to the
intended `/Users/xlyk/.codex/config.toml`, with command
`exec '/Users/xlyk/.local/bin/ovrcr' report codex --stdin`.
The parsed reverse-additions comparison recovered every original TOML value,
handler order and trust value. Composition was idempotent and mode 0600 retained.
The existing `/Users/xlyk/.codex/hooks.json` stayed byte-identical, SHA-256
`d1ec33f2de8acf7b69a941e6877057c47fbf343a9f49ec26990a21c8a6664a77`.

The [pinned Codex source review](native-hook-source.md) establishes JSON then TOML
declaration order within a layer. Native synchronous handlers run concurrently;
ordered display/results do not imply serial execution. `hooks.state` controls
trust and enablement, not file inclusion. The mixed-source warning was visible
and retained; no handler migration or wrapper was introduced.

Native Codex **0.153.0** listed 15 hooks: the existing seven JSON handlers required
review, three plugin hooks were new/untrusted, and five OVRCR reporters were new.
Through the native hook browser, the operator trusted only each OVRCR reporter
individually. [Trust preservation](native/trust-preservation.json) confirms all
previous trust entries stayed identical and only five reporter trust keys were
added. Ten unrelated hooks remain in their prior needs-review state. Their
declarations/status were preserved; they were not expected to dispatch and this
record does not claim their downstream notifications executed.

The actual user Codex home was used. No alternate home, authentication copy or
approval bypass was introduced. Native workspace trust was approved only for the
task-owned fixture. Native update offers were skipped to retain exact 0.153.0.
[Installed doctor](installed-doctor.json) accepts that version and the supplied
TOML, while explicitly leaving effective configuration, trust and delivery
unverified. The evidence below supplies the separate native delivery check.

## Installed managed launch on macOS

The native GUI was built from the reviewed checkout and ran installed OVRCR for
both server PID 20874 and dashboard PID 21168. Its disposable root was
`/private/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-JfWqxG`;
config, socket, synthetic repositories and workspaces were all underneath it.
The normal server and its data were not used by this acceptance run.

The selected fixture shell was session 2, PID 20973. Real keyboard input launched:

```sh
/Users/xlyk/.local/bin/ovrcr agent run codex -- /opt/homebrew/bin/codex --no-alt-screen
```

The wrapper PID was 32968, native Codex PID/PGID 32970, and conversation
`01a09234-9a01-77d1-9916-c2ed5bfb035d`. The other nine GUI terminals were synthetic
fixtures; their agent labels are not provider acceptance evidence. Metrics stayed
null. Raw `codex` launches remain untracked; the managed wrapper inside an OVRCR
terminal is required.

| Case | Actual outcome and evidence |
| --- | --- |
| Individual trust | [Native hook list](native/02-five-reporters-trusted.jpg), [AX](native/02-five-reporters-trusted.ax.txt); five reporters active, unrelated ten still require review. |
| First assistant response | Actual assistant `ISSUE58_READY_ONE`, Ready/Observed, [runtime snapshot](native/first-ready-terminals.json). The initial AX/screenshot pair straddled the transition; later captures show the distinct assistant output and Ready together. |
| Next prompt | [Busy capture](native/05-next-busy.jpg), [AX](native/05-next-busy.ax.txt): Ready cleared on submission. `/bin/sleep 15` completed, then actual assistant `ISSUE58_READY_TWO` with [Ready](native/06-second-ready.jpg), [AX](native/06-second-ready.ax.txt). |
| Interruption | During bounded `/bin/sleep 45`, [Busy snapshot](native/interrupt-busy-terminals.json); Ctrl-C produced native “Conversation interrupted” and [Idle/Observed](native/08-interrupted-idle.jpg), [AX](native/08-interrupted-idle.ax.txt), [snapshot](native/interrupted-idle-terminals.json). No assistant completion marker appeared for that turn. Native showed its background tool briefly; process cleanup was verified separately. |
| Final response | Actual assistant `ISSUE58_READY_FINAL`, [Ready/Observed](native/09-final-ready.jpg), [AX](native/09-final-ready.ax.txt). |
| Reconnect | Browse/detach exited dashboard successfully. Restart dashboard retained Ready, the same binding/activity revision 8 and native PID 32970: [capture](native/11-reconnected-ready.jpg), [AX](native/11-reconnected-ready.ax.txt), [snapshot](native/reconnected-ready-terminals.json), [ownership](owned-after-reconnect.json). New dashboard PID was 92047. |
| Native exit | Native `/exit` returned `ISSUE58_NATIVE_EXIT=0`; [capture](native/12-native-exit.jpg), [AX](native/12-native-exit.ax.txt). [Snapshot](native/native-exit-terminals.json) retained the last Ready observation with Unavailable health (`supervisor_disconnected`). This historical observation does not claim current readiness. |
| PTY geometry | Actual shell output `ISSUE58_PTY=57 126`: [capture](native/13-pty-geometry.jpg), [AX](native/13-pty-geometry.ax.txt). |
| Cleanup | Native GUI closed normally; launcher exit 0. [Cleanup](native/cleanup.json) verifies all 23 recorded PIDs and 21 groups absent (ESRCH), including native helpers and bounded sleep; disposable root/socket removed. Ownership manifests before native, during native and after reconnect are retained. |

CUA's first app-selection call stalled for about 2,387 seconds before returning.
The fixture remained running and no native acceptance was claimed during that
stall. Subsequent bounded calls completed. One restart click used an invalid
argument shape and failed without acting; the corrected visible-button click
succeeded. These are tool failures, not provider behavior failures.

The visible status stayed legible in the selected header; long sidebar labels
clip in the narrow pane. No UI redesign is included in this installation ticket.
No Linux native GUI/provider acceptance, continuous identity, metrics, accounting,
Confirmed completion, notifications or unread behavior is certified here.

## Local automated correction and verification

The first new full run [failed](full-attempt-01.json), with three shared
initial-admission fixture failures in [the retained output](full-attempt-01.log).
Two instrumented lifecycle attempts remain in `lifecycle-diagnostic-01/02` logs,
metadata and patches. They localized failure before the fresh version script's
entry marker, with valid TTYs and unchanged native argv after admission fallback.
The exact low-level cause (spawn failure, timeout or another early failure) was
not proven. Prior preparation failures remain in the parent record.

The worker reused the existing `env_lock()` in `assert_initial_admission`,
serializing five competing fixture cases. No production code, deadline, assertion
or test-thread setting changed. Internal concurrency and fault checks remain.
With instrumentation removed, [default-threaded lifecycle verification](lifecycle-fix-01.json)
passed **75 tests, 0 failures, 9 ignored**. Independent review found no blocking
finding and confirmed teardown happens before the guard drops. Coordinator
committed the two-line correction as `a42cb12`.

The [full default-threaded macOS workspace run](full-attempt-02.json) at
`a42cb126ce1675726f64f06be775cad9b8ad6ddf` passed **663 tests, 0 failures,
14 ignored** across 25 binaries. No task builds or native acceptance overlapped.
All-target/all-feature Clippy and formatting passed; doctests passed with zero
cases. Results are retained in their named metadata/logs. Hosted current-head checks remain pending. Ignored
manual/helper cases never count as passes.

## Remaining delivery gate

The original server PID 43232 and dashboard PID 43226 use protocol 4 and own five
existing terminals. They cannot transfer PTYs to protocol 6. The installed-native
fixture is complete; normal-server transition requires closing those exact live
terminals deliberately, then matching-client `shutdown` without `--kill`, followed
by the installed server's normal launch and protocol handshake. An explicit
session-owner decision is pending. No old process has been signaled and no live
socket or registry has been removed.

Current-head CI, independent final review, PR delivery and work-diary closeout
must be recorded before claiming the entire issue complete. The original checkout
and unrelated work remain preserved.

Accessibility text has only trailing line whitespace removed for readable diffs.
Logs have only redundant EOF newlines removed. Exact raw bytes remain in the
matching gzip files, with hashes in [the normalization manifest](evidence-normalization.json).
Snapshots, screenshots and failed attempts are retained.
