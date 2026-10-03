# Bridge verification continuation, 2026-10-03 UTC

**Baseline:** remote `main` at `7b37a99312831bb3b3aa9a446f05736bf93abdf5`.
**Decision:** first-use Allow evidence passed on this host. #221 remains NO-GO
under its current contract; Settings support and applicable Glass use remain open.
No production Bridge implementation, issue/ADR edit, branch publication, merge,
deployment, or new sound/banner test occurred.

## Current tracker and scope

Live GitHub bodies, discussions and PR status were read on 2026-10-03 UTC.
All listed issues remain OPEN. Labels do not waive dependency or approval gates.

| Issue | Scope | Dependency |
| --- | --- | --- |
| [#220](https://github.com/xlyk/ovrcr/issues/220) | Bridge umbrella: private banners, safe navigation, native sound and terminal activation | Child acceptance |
| [#221](https://github.com/xlyk/ovrcr/issues/221) | Native identity, permission/Settings, compatible peers and applicable sound resource use | None; verification only |
| [#222](https://github.com/xlyk/ovrcr/issues/222) | Signed private banners, bounded delivery, permission/install/update recovery | #221 |
| [#223](https://github.com/xlyk/ovrcr/issues/223) | Callback → Server → active Dashboard, run/lifetime/owner fences and immediate Browse navigation | #222 |
| [#224](https://github.com/xlyk/ovrcr/issues/224) | Banner sound, saved opt-in preservation and persisted Glass/default choice | #222 and sound facts |
| [#225](https://github.com/xlyk/ovrcr/issues/225) | Explicitly authorized existing-iTerm focus and parent-app fallback | #223 |
| [#226](https://github.com/xlyk/ovrcr/issues/226) | Integrated signed release and cross-platform acceptance | #224 + #225 |

[PR #238](https://github.com/xlyk/ovrcr/pull/238) and
[PR #239](https://github.com/xlyk/ovrcr/pull/239) are merged verification/design
records. [PR #173](https://github.com/xlyk/ovrcr/pull/173) is the earlier design.
They implement no production Bridge. This baseline still uses `osascript` and
independent macOS `afplay`; original-name banner bodies remain private.
`SessionSummary.title` is an effective display title without manual/generated
provenance and must not be reused as the future subtitle.

## Authorized first-use result

Probe source commit: `63c93a77827bc1b37b7144c91a717b6b48e7bf5c`.
Independent exact-commit review found no blocking source defects. Typecheck attempt
1 failed on a Swift scope binding; attempt 2 passed after mode became a delegate
property. Both attempts and the exit/source-hash receipt remain in local validation
evidence. No native run was performed before review.

Kyle approved one disposable signed permission-only request and then explicitly
authorized CUA to accept only that fixture's notification prompt. This replaced
waiting for an owner observation at the computer. Existing Developer-ID signing
completed without interaction; hardened runtime and strict signature verification
passed. No credential/Keychain/certificate change, secure timestamp or notarization
is claimed.

Fresh fixture: **OVRCR Permission Probe 40a8531a**, bundle
`com.ovrcr.permissiononly.40a8531aed84413596f8789d9b6519a5`.
`LSUIElement=true` and actual accessory activation policy `1` were recorded.
The registered status-only process read `0/0/0` and exited without requesting
permission. The request process recorded the same baseline, made exactly one
alert/sound authorization request, and submitted no notification.

Native CUA exposed the new fixture's exact name and an `Allow` secondary action.
The saved screenshot shows only that permission notice, on a blank backdrop.
CUA invoked `Allow` once on that freshly observed container. The following AX
read returned `noWindowsAvailable`; the prompt had disappeared. Independently,
the same owned PID recorded `authorization-completion` with `granted=true`, then
final authorization/alerts/sound `2/2/2`, and exited zero after **48.864 seconds**.
Four preceding polls reported `1/2/2` while completion was pending. Those reads
were not a human denial or a completed request; this observation preserves the
earlier failure's uncertainty rather than assigning it a cause.

Local, scoped artifacts:

- [Before-Allow screenshot](evidence/20261003-permission-only/permission-before-allow.jpg)
- [Before-Allow AX](evidence/20261003-permission-only/permission-before-allow-ax.txt)
- [Fresh baseline events](evidence/20261003-permission-only/status-events.jsonl)
- [Request and completion events](evidence/20261003-permission-only/request-events.jsonl)
- [Hashes and cleanup receipt](evidence/20261003-permission-only/result.json)

Two pre-request CUA attempts to bind Notification Center timed out while it had no
prompt. The request-time binding and screenshot succeeded. The post-action AX
error is retained as an observation limitation, not a failed callback assertion.
No new banner, audio, click navigation, Server/session operation, Settings change,
permission reset, automatic retry or unrelated prompt acceptance occurred.
The 120-second source timer and external guard were not exercised to expiry.
This is one host's Allow path, not all macOS versions or a new observed Don't Allow
choice. Historical denied-state refusal/recovery and warm/cold CUA callback evidence
remain in the [original report](native-contract-sources.md), with their own limits.

Cleanup verified both owned PIDs/process groups gone and zero matching executable
instances before unregistering/removing only the unique task installation. The
signed recovery bundle still passes strict verification. The OS permission record
remains. No pending/delivered-notification inventory API was called: the reviewed
source has no notification submission API, so there was no submitted banner to
remove. Do not relabel that source fact as a measured inventory readback.

Full private recovery/evidence root:
`/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-permission-only-20261003-hgc89dmw/`.
Original signed recovery and all previous evidence are preserved.

## Current compatibility and source verification

The Server wire is **32**, not the historical probe's hardcoded 25. The probe is a
historical admission experiment, not a compatible production host. Existing
production paths retain 1 MiB frames, a five-second client handshake, and
connect-if-running. Later click transport must preserve no-start behavior and
translate the generic mismatch guidance rather than forwarding `shutdown --kill`.

Executed on the pinned Rust baseline:

| Check | Result |
| --- | --- |
| `cargo test --offline -p ovrcr-protocol preamble` | 2 passed |
| `cargo test --offline -p ovrcr --test server_lifecycle client_with_wrong_protocol_version_is_refused -- --exact` | 1 passed with required socket permissions |
| `cargo test --offline -p ovrcr --test server_lifecycle startup_read_only_commands_do_not_start_a_missing_server -- --exact` | 1 passed |
| Swift permission-only typecheck, attempt 2 | Exit 0; no native execution from typecheck |

The first real-Server test attempt failed to bind its disposable socket under the
sandbox. That failure and the passing permitted rerun have distinct logs/receipts.
Local command/revision/count/exit evidence is at the task's `bridge-validation/`.
These four Rust baselines are not full workspace, feature, native release, Linux,
capacity or memory acceptance. Those later required gates remain unrun.

The concurrent [permission/Settings source report](permission-settings-20261003.md)
and [Glass source report](glass-resource-contract-20261003.md) are integrated
without rewriting historical evidence. Their pre-run statements remain dated
checkpoints; this section records the later actual run.

## Pending owner decisions

The parent asked Kyle about two scope changes; neither is treated as approved here:

1. Replace the unsupported direct Notifications-pane URI contract with a documented
   action opening System Settings and instructions for Notifications → Bridge.
   A successful System Settings launch must not be reported as direct pane navigation.
2. Use standard system notification sound and defer Glass. Apple's current
   [`UNNotificationSound.default`](https://developer.apple.com/documentation/usernotifications/unnotificationsound/default)
   is available on macOS from 10.14, and the retained human A/B observation already
   heard it. This removes the Glass-specific resource/use question if approved;
   native policy and future release audio acceptance remain applicable.

If approved, update #220's sound-choice story/decision, #221's Settings and sound
verification requirements, #224's persisted-choice/native matrix requirements,
#226's final audio matrix, and ADR 0006 together. Preserve both saved flags, the
macOS both-flags-required rule and Linux's independent behavior. Standard sound
needs no new selector or persisted enum. Publication/tracker writes need their
separate established authorization. No GO follows before these decisions and
independent review of the reconciled evidence.

## Ready implementation ownership after the gate

No downstream code is started. Use new worktrees pinned to the approved integration
base and give each file one owner. The existing config/default planners and discarded
Pi sessions stay canceled.

| Stage | Independent worker ownership | Integration dependencies |
| --- | --- | --- |
| #222 native host | New native Bridge bundle, version/admission entry, native permission/settings adapter and separate installer tooling | Stable reviewed contract; no root Rust/TUI edits |
| #222 Dashboard delivery | `crates/ovrcr-tui/src/dashboard/desktop.rs`, its host-facing event loop/status/actions and notification help | Own persistent host teardown/bounded work; connect to native adapter only after interfaces are reviewed |
| #222 privacy and shared payload | Owning protocol session summary/payload fields and runtime summary builders | Explicit manual-title provenance; all shared constructor/caller/wire updates owned here, then integrated before host use |
| #222 application tests | Existing `DesktopAlertDashboard` real-Server/real-PTY fixture in `tests/server_lifecycle.rs` and supporting owned fixture boundary | One fixture/test owner; privacy sentinels, baseline, cancellation, save and slow/missing/denied paths |
| #223 navigation, after #222 | Single runtime/protocol worker for Server routing/owner/run/lifetime admission; single TUI worker for pane/mode/retarget cleanup | Serialize overlapping protocol and test edits; suppress `view_request` auto-recovery on clicks; clean same-target transient modes |
| #224 sound, after #222 | Existing settings authority if still needed, desktop host payload and sound help | Serialize with #222 desktop owner; preserve opt-ins/save failures; default-only removes chooser if approved |
| #225 then #226 | Separate verified iTerm adapter and pinned native acceptance owner | #223 fences first; no surprise terminal-control prompt; final platform/ownership/capacity gates |

The coordinator owns integration order, interface review, docs reconciliation and
exact-revision validation. Assign independent review seats for committed slices;
return blocking corrections to their original owner. A running persistent Bridge
must not inherit the current short-lived host's process-group kill/join teardown.
Clicks must not use ordinary navigation's auto-recovery or same-target early return.
Startup and accepted Server settings/editor/external changes must request permission
without waiting for a later alert. These are implementation requirements, not
claims that the unbuilt feature already passes them.
