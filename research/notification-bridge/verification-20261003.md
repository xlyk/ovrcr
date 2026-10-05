# Bridge verification continuation, 2026-10-03 UTC

**Baseline:** remote `main` at `7b37a99312831bb3b3aa9a446f05736bf93abdf5`.
**Decision:** first-use Allow evidence passed on this host. Kyle subsequently
approved original bundled custom tones plus system default, retaining the sound
selector and removing dependence on Apple Glass. This direction is prepared
locally; Kyle also approved documented System Settings opening plus manual
Notifications → Bridge instructions. Exact asset/contract review passed at
`e6d2602362a237a054c30676becdb8a4964d8b4e`: **revised local #221 GO supports
authorized #222 source implementation**. This claims no new-tone audibility or
downstream production acceptance. Live issues are still OPEN; no tracker edit,
publication, merge, deployment or new sound/banner test occurred in this
verification continuation.

## Current tracker and scope

Live GitHub bodies, discussions and PR status were read on 2026-10-03 UTC.
All listed issues remain OPEN. Labels do not waive dependency or approval gates.

| Issue | Scope | Dependency |
| --- | --- | --- |
| [#220](https://github.com/xlyk/ovrcr/issues/220) | Bridge umbrella: private banners, safe navigation, native sound and terminal activation | Child acceptance |
| [#221](https://github.com/xlyk/ovrcr/issues/221) | Native identity, permission/Settings, compatible peers and applicable sound resource use | None; verification only |
| [#222](https://github.com/xlyk/ovrcr/issues/222) | Signed private banners, bounded delivery, permission/install/update recovery | #221 |
| [#223](https://github.com/xlyk/ovrcr/issues/223) | Callback → Server → active Dashboard, run/lifetime/owner fences and immediate Browse navigation | #222 |
| [#224](https://github.com/xlyk/ovrcr/issues/224) | Banner sound, saved opt-in preservation and persisted choice; current issue still names Glass/default | #222 and sound facts |
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
CUA invoked `Allow` once on that freshly observed container. The combined Allow/AX
call returned `noWindowsAvailable` and its exported tool status is **failed**;
there is no separate per-statement success marker. Independently,
the same owned PID recorded `authorization-completion` with `granted=true`, then
final authorization/alerts/sound `2/2/2`, and exited zero after **48.864 seconds**.
Four preceding polls reported `1/2/2` while completion was pending. Those reads
were not a human denial or a completed request; this observation preserves the
earlier failure's uncertainty rather than assigning it a cause.

Local, scoped artifacts:

- [Before-Allow screenshot](evidence/20261003-permission-only/permission-before-allow.jpg)
- [Before-Allow AX](evidence/20261003-permission-only/permission-before-allow-ax.txt)
- [Actual CUA invocation and error receipt](evidence/20261003-permission-only/cua-tool-receipt.json)
- [Fresh baseline events](evidence/20261003-permission-only/status-events.jsonl)
- [Request and completion events](evidence/20261003-permission-only/request-events.jsonl)
- [Hashes and cleanup receipt](evidence/20261003-permission-only/result.json)

Two pre-request CUA attempts to bind Notification Center timed out while it had no
prompt. The request-time binding and screenshot succeeded. The combined action/AX
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

## Approved replacement sound direction

Kyle asked whether custom notification sounds could be used and agreed to a few
original or properly licensed bundled short tones alongside system default, with
the selector retained. This supersedes the earlier pending default-only proposal.
The new local preparation uses original mathematical synthesis, without samples,
Apple sound bytes, installed-system links or a third-party generation dependency.
The historical [Glass investigation](glass-resource-contract-20261003.md) remains
unchanged evidence for the earlier direction; no Glass license clearance is claimed.

The candidate assets, generator and measured file manifest are in
[sounds/](sounds/README.md). The [proposed sound contract](proposed-sound-contract-20261003.md)
defines stable choices, provenance, missing-resource handling, exact tracker/ADR
reconciliation scope and the bounded native verification still needed. File
inspection and reproducible synthesis do not prove that a notification played a
tone, that the owner heard it or that the candidates sound pleasing.

| Candidate | Duration | Encoding | Read-only validation |
| --- | --- | --- | --- |
| Tap | 0.22 s | Mono 48 kHz PCM16 WAV | Hash/header/numeric checks, two byte-identical generations, `afinfo` and `ffprobe` passed |
| Chime | 0.84 s | Mono 48 kHz PCM16 WAV | Same checks passed |
| Rise | 0.64 s | Mono 48 kHz PCM16 WAV | Same checks passed |

The manifest measures quantized peak `0.179992676` full scale, zero clipping and
zero endpoint samples/steps for each. It records complete SHA-256 hashes and
provenance. Reproducibility was measured on CPython 3.13.7/macOS arm64, without
claiming cross-platform transcendental-function byte identity.

The existing app stores only `desktop_notifications` and `ready_sound`; no persisted
sound selector has shipped. The planned selector must use the existing Server-owned
settings authority, preserving both saved flags and explicit future choices.
macOS still requires both flags for the intended banner sound; Linux retains its
independent behavior. System default as the missing-setting default is a proposed
implementation choice, not an existing stored preference. No production setting,
loader, Dashboard or native bundle has been changed here.

## Revised enabling gate and downstream acceptance

Both product choices are approved. The Settings action now opens System Settings
through a documented application-launch API with instructions for Notifications
→ Bridge. A successful app launch must not be reported as direct pane navigation.
The production action and its visible outcomes belong to #222 acceptance.

The exact live #221 body was re-read after these decisions. Its sound criterion is
to establish lawful availability and native playback **requirements**, not to
hear each new asset. Independent criterion review found no new feasibility fact
unique to plain original bundled PCM WAV files: Apple explicitly supports this
format under 30 seconds, and retained signed-fixture owner listening already
demonstrated the same named/default delivery API. Original bytes remove the
Glass-specific installed-OS use/lookup uncertainty. A fresh listening probe solely
because these bytes changed would add an enabling gate absent from the criterion.

| Revised #221 enabling fact | Evidence / precise boundary |
| --- | --- |
| Stable signed Dock-less identity; install/update and ordinary CLI rebuild continuity | Retained signed native report; local signing, not timestamp/notarization or installed-release acceptance |
| First-use permission request/visible Allow and completion; denied-state recovery | Fresh scoped prompt/native grant receipt above plus retained denial/recovery; no invented human denial |
| Supported explicit Settings mechanism | Documented app opening and Apple manual route, now accepted by Kyle; automatic pane selection removed from the contract |
| Native banner and warm/cold callback | Retained unchanged signed-agent pixels/AX and exact callbacks; production Server routing remains #223 |
| CLI/Server–Bridge compatible contract | Exact-version admission experiment and current wire-32 Rust baselines; future feature wire changes remain normal protocol work |
| Original resource provenance and native sound requirements | Original generator/manifest/MIT notice, PCM WAV/duration validation, owning Apple sources and reused same-API named/default native evidence; new assets unheard |

Exact committed artifact/contract review passed at
`e6d2602362a237a054c30676becdb8a4964d8b4e` against `4b6a5a5`, with no blocking
findings. The independent reviewer regenerated twice in fresh directories, both
exit zero, and verified byte-identical WAVs **and manifests**, all PCM/header/
numeric/hash facts, MIT notice, decoder/source receipts, four narrow issue-body
drafts, 27 relative links and unchanged historical/probe/production inputs. The
[independent receipt](evidence/20261003-original-tones/independent-review.json)
preserves that exact revision and measured assets. The reviewer ran no playback,
notification, signing, installation or permission operation.

This completes revised local #221 enabling verification and permits the authorized
#222 source implementation. It is not production or release GO. Native #222
acceptance, custom sound/selector acceptance and later child dependencies remain
open with their owning tickets; the live #221 checklist has not been edited or
closed by this local finding.

The sound-design approval does not authorize an audible test, a new permission
request, an unrelated prompt acceptance or any notification-policy change. A later
native test needs a reviewed exact fixture, resource hashes, recipient identity,
request/count/deadline bounds and explicit approval while the owner can listen.
Submission success alone cannot pass audibility; suppressed or inconclusive
delivery requires reporting the result and stopping without a retry. The detailed
matrix and cleanup requirements are in the proposed sound contract, owned by
#224/#226. New signing/install/permission operations keep their specific approval
boundary; ordinary source preparation and headless implementation checks proceed
within the authorized local scope.

The [local issue-body drafts](drafts/README.md) reconcile #220, #221, #224 and #226;
the local ADR 0006 diff records both approved decisions and current evidence.
These changes are prepared for review only. Live issues remain unchanged and
unchecked; publishing or closing them requires its separate authorization. Full
production selector, save/preservation, sound/policy and packaged-release checks
stay with #224/#226, rather than becoming new prerequisites for #221.

## Ready implementation ownership after the gate

No downstream code was started during the verification preparation. After the
passing exact gate review, four implementation workers were assigned fresh
worktrees pinned to `e6d2602`, with native bundle/tooling, TUI delivery/status,
shared manual-title provenance/protocol and application tests under separate
ownership. The coordinator integrates on `work/bridge-notifications-20261003`.
Existing config/default planners and discarded Pi sessions stay canceled.

| Stage | Independent worker ownership | Integration dependencies |
| --- | --- | --- |
| #222 native host | New native Bridge bundle, version/admission entry, native permission/settings adapter and separate installer tooling | Stable reviewed contract; no root Rust/TUI edits |
| #222 Dashboard delivery | `crates/ovrcr-tui/src/dashboard/desktop.rs`, its host-facing event loop/status/actions and notification help | Own persistent host teardown/bounded work; connect to native adapter only after interfaces are reviewed |
| #222 privacy and shared payload | Owning protocol session summary/payload fields and runtime summary builders | Explicit manual-title provenance; all shared constructor/caller/wire updates owned here, then integrated before host use |
| #222 application tests | Existing `DesktopAlertDashboard` real-Server/real-PTY fixture in `tests/server_lifecycle.rs` and supporting owned fixture boundary | One fixture/test owner; privacy sentinels, baseline, cancellation, save and slow/missing/denied paths |
| #223 navigation, after #222 | Single runtime/protocol worker for Server routing/owner/run/lifetime admission; single TUI worker for pane/mode/retarget cleanup | Serialize overlapping protocol and test edits; suppress `view_request` auto-recovery on clicks; clean same-target transient modes |
| #224 sound, after #222 | Existing Server-owned settings authority and selector, desktop host payload, bundled original assets and sound help | Serialize with #222 desktop owner; preserve opt-ins/save failures; custom/default chooser retained under the approved direction |
| #225 then #226 | Separate verified iTerm adapter and pinned native acceptance owner | #223 fences first; no surprise terminal-control prompt; final platform/ownership/capacity gates |

The coordinator owns integration order, interface review, docs reconciliation and
exact-revision validation. Assign independent review seats for committed slices;
return blocking corrections to their original owner. A running persistent Bridge
must not inherit the current short-lived host's process-group kill/join teardown.
Clicks must not use ordinary navigation's auto-recovery or same-target early return.
Startup and accepted Server settings/editor/external changes must request permission
without waiting for a later alert. These are implementation requirements, not
claims that the unbuilt feature already passes them.
