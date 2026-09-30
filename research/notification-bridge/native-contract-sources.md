# macOS Bridge native contract: blocked verification

**Ticket:** [#221](https://github.com/xlyk/ovrcr/issues/221)

**Decision:** NO-GO for dependent implementation; this is a partial verification report, not acceptance.

**Observed:** 2026-09-29–30, macOS 26.5.2 (25F84), Xcode 26.3 / Swift 6.2.4, arm64.

**Checkout baseline:** `14c7fe5ffff7e0dd90a920f2006f35c4b3a97c25`.

No production Bridge, CLI, Server, Dashboard or configuration code was changed. Tickets #222–#226 have not begun. Existing design documentation remains separate from implementation acceptance.

## Evidence boundaries

The owner authorized disposable native probes and later authorized parent-led validation after the research toolchain failed. Official Apple/iTerm sources were fetched with ordinary TLS verification; successful responses, effective URLs, timestamps and hashes are recorded in the private source manifest. Failed fetches are not evidence.

The researcher produced a source-only recovery artifact, but both governed workflows failed. The retry reported unavailable strict-allowlist tools `web_search`, `fetch_content`, `get_search_content`, and `source_check`; downstream Standards/Spec reviews did not launch. Its claimed full-report path was overwritten by the workflow error receipt. This report therefore distinguishes retained source findings from separately observed parent-run native results. No successful automated source check or independent Standards/Spec review is claimed.

Private evidence roots, retained for recovery:

- `/tmp/ovrcr-bridge-grill-nw1ylL/`: source originals/derivatives, manifest, owner answers, receipts and captured repository diffs.
- `/tmp/ovrcr-native-contract-ZwWUQp/`: probe/helper source, build/signing results, JSON events, scoped accessibility observations, own-bundle logs and cleanup evidence.

These local paths are not durable public attachments. Preserve the evidence before clearing temporary storage. No signing key was exported, no Keychain ACL was changed, and signing identity names/fingerprints were not recorded in the report.

## Source contracts and limits

### Identity, installation and updates

Apple documents `LSUIElement` agents that do not appear in the Dock [A1]. This alone does not establish eligibility for UserNotifications or callback delivery. The SDK states that the notification-center delegate can only be set from an application [A2].

Apple's signing guidance says updates should retain the identifier and designated requirement (DR). An ad-hoc signature has no cryptographic signing identity; individual macOS subsystems decide how they track trust [A3], [A4]. Leaving a separately installed Bridge unchanged during an ordinary CLI rebuild is the agreed isolation boundary. Later local checks measured unchanged installed executable/Info/DR hashes and retained authorization, as recorded below; this is not complete release-installation acceptance.

Developer ID distribution/notarization additionally requires valid signatures, hardened runtime and secure timestamps [A5]. The local signed fixture used hardened runtime **without a secure timestamp or notarization**. It is not release-installation or Gatekeeper acceptance.

### Authorization, Settings and clicks

`requestAuthorization` and `getNotificationSettings` expose authorization and feature settings; authorization/submission success does not prove a banner or sound [A6], [A7]. Requests and error handling must honor current OS policy.

The Tahoe guide directs users to System Settings → Notifications. Its Help action is `x-help-action://openPrefPane?bundleId=com.apple.Notifications-Settings.extension` [A8]. A Help link is not proof of a supported third-party invocation. `openSettingsForNotification` is a reverse callback for in-app settings, not an API for opening system notification preferences [A9].

The delegate must be assigned before launch finishes; default responses, custom actions and foreground presentation have separate delegate paths, whose completion handlers must be called [A9], [A10]. OS activation of the Bridge does not authorize launching a Server, Dashboard or terminal session. Actual Dashboard navigation and stale Server/run fencing have not been implemented or tested.

### Banner sound and Glass rights

Apple documents the default notification sound and named lookup in app/group containers and the main bundle [A11], [A12]. Those sources do not establish lookup of `/System/Library/Sounds/Glass.aiff` or a built-in `Glass` name. Installed-file existence and successful sound-object construction are insufficient proof of playback.

Custom sounds have format and duration constraints; longer sounds fall back to default [A11]. The owning sources do not specify every macOS missing/unreadable-file fallback. A fallback must not be mistaken for Glass.

The signing/layout question is narrower than previously recorded: Apple's TN2206 and Code Signing Guide explicitly except symlinks into `/System` and `/Library` from the prohibition on links outside an app bundle [A3], [A4]. TN2206 also states that version-2 resource envelopes record symbolic links and that `--strict` checks them. Thus a sealed resource link to the already-installed `/System/Library/Sounds/Glass.aiff` has owning signing-layout support, consistent with the observed strict-verification pass. This does not guarantee Glass's presence on every OS release, prove notarized distribution, or grant asset rights.

The Tahoe license defines accompanying content/data as Apple Software (§1A), grants conditional install/use/run rights (§2A/B), reserves ungranted rights, and restricts copying/redistribution (§2J/K/N), with jurisdictional qualifications [A13], [A14]. These are materially different operations: the tested resource link references the user's installed OS file; it does not ship, extract or copy Glass bytes. A missing Glass-specific redistribution grant is therefore not, by itself, proof that this noncopying use is unlawful. The sources still do not establish a Glass-specific rights determination for this release use. Confirm the applicable license/use interpretation separately; do not describe signing acceptance, an SDK conversion example or heard audio as legal clearance.

No Glass audio bytes were copied or distributed. An exploratory bundle-resource symlink to the installed system file passed Developer-ID signing and strict verification. After an initially silent A/B retry, the owner reported Do Not Disturb was on and explicitly prepared a new context. The second retry's own-bundle logs showed no Focus suppression; the owner then reported hearing Glass and system default clearly and seeing both popup banners. This is attributed human native evidence on this host, not a waveform, popup screenshot, asset-rights determination or release acceptance. Independent `afplay`, silent sound substitution, critical-alert escalation and agent-driven notification-policy bypass are not approved solutions.

### Exact iTerm focus

Official iTerm references distinguish selecting a session, selecting its tab, bringing its window forward and keyboard focus [I1], [I2]. The Python connection path is not established as a nonprompting authorization preflight [I3], [I4]. Apple's SDK provides AppleEvent permission checks without prompting and a no-prompt send flag, but required purpose strings/entitlements and the actual Bridge-to-iTerm mapping remain unverified. No terminal-control permission was requested and no iTerm session was manipulated.

## Actual native observations

The fixture contained synthetic project/workspace/terminal/session names only, no live Server connection and no user-session content. Its first notification was deliberately silent.

| Run | Observed result | Limit |
| --- | --- | --- |
| Ad-hoc, `LSUIElement=true`, `/tmp` | Strict signature verification succeeded. Settings were authorization/alert/sound `0/0/0`; authorization returned false with “Notifications are not allowed for this application.” Bundle lookup returned no application URL. | Does not prove a Developer-ID-signed release is ineligible. |
| Same signed bytes, uniquely owned `~/Applications` directory | Executable hash was unchanged; strict verification succeeded. The same authorization error and missing application URL remained. | Moving the bundle alone did not resolve this fixture's failure. |
| Ad-hoc diagnostic control, `LSUIElement=false`, early accessory activation | Bundle lookup resolved to the owned installation. The OS displayed a fixture-attributed permission notice; retained cropped screenshot and AX bounds identify it. | Metadata/signature changed. The short fixture expired before permission collection; no grant or callback was captured. Do not infer an owner choice. |
| Longer ad-hoc control | Initial settings were `1/2/2`; authorization returned false. | Denial was observed; its cause/provenance was not established. No policy reset was performed. |
| Developer-ID-signed control, hardened runtime, same fixture ID/path | Signing completed without interaction and strict verification succeeded. Actual activation policy was `1` (accessory). Initial settings were `2/2/2`; authorization returned true and submission returned no error. | Permission was already authorized at this run's start. This does not prove a fresh opt-in flow or explain the transition from the earlier denied state. |
| Signed request under current Focus policy | Own-bundle native logs reported `interruptionSuppression: delay delivery`. No popup banner or foreground delegate event was captured. | Submission is not visible-banner acceptance. Focus was not changed or bypassed. |
| Notification Center history inspection/click | AX inspection exposed the synthetic ready card and its fixture body. A fresh, owned-card click succeeded. After the original probe deadline, an owned fixture process was found running. | Original-process output contained no response callback; the cold-launched process's output was not captured. This is not callback-payload or navigation acceptance. |
| Explicit test Settings action, initial attempt | The Help-action URI had no registered LaunchServices handler. The `x-apple.systempreferences:` Notifications URI resolved to System Settings; `open` returned zero. | Initial scoped destination check did not verify the pane. |
| Resumed warm/cold clicks | A warm click reached the signed fixture's delegate. After stopping both owned test instances, a fresh history-card click cold-launched the fixture and reached its delegate with the same fixture/session/default-action payload, without resubmitting an alert. A fresh, window-owner-checked screenshot shows only the synthetic card. | Native probe evidence, not production Server/Dashboard navigation. |
| Ordinary CLI rebuild | `cargo build -p ovrcr` succeeded while the signed fixture remained running. Installed executable/Info/DR hashes stayed identical, and the same fixture reported authorization/alerts/sound `2/2/2` afterward. | One local host and fixture identity; not release distribution acceptance. |
| Separately signed local update | Changing the fixture to bundle version 2 and re-signing retained its DR. A native status-only launch reported `2/2/2`. | Local update without secure timestamp/notarization. |
| Resumed explicit Settings action | Fresh scoped AX traversal after the candidate URI opened found an `AXWindow` titled exactly `Notifications`. | Observed on 26.5.2. The URI's third-party stability and other macOS versions remain unverified; no automatic Settings action was added. |
| Signed agent metadata control | A Developer-ID-signed, strictly verified `LSUIElement=true` status-only launch returned authorization/alerts/sound `2/2/2`, after which the previous control metadata was restored and re-signed/verified. | The earlier ad-hoc agent failure must not be generalized to signed agents. This run did not measure banner/click behavior or startup Dock visibility in that metadata mode. |
| Glass reference candidate | `Contents/Resources/Glass.aiff` referenced the installed OS file via symlink; no audio copy. Developer-ID signing and `--verify --deep --strict` both exited zero. Native test A requested this named bundle resource; test B requested system default. Own-bundle logs reported Focus delay for A; the A driver stopped rather than count absent foreground presentation as playback. B was submitted once separately. Both owned requests were then cleared to avoid late test audio. | Signing/requests do not prove audibility, identity of heard audio or legal clearance. Round 11 timed out after 3600 seconds without a submission. No human listening result or permitting-context choice was inferred; copied owner answers remain acceptable. |
| Missing Glass negative case | Removing only the owned resource link, re-signing and strictly verifying the fixture, then invoking its Glass test entry emitted `sound-resource-unavailable` with no submission/sound request. The link was restored and the owned bundle re-signed/verified. | Actual signed probe guard, not production Bridge coverage. No silent system-default fallback was counted as Glass. |
| Owner-requested A/B retry under Do Not Disturb | Both requests were accepted, authorization/alerts/sound remained `2/2/2`, and both reached foreground delegates. Own logs reported Focus delay. Genuine round-12 answers reported no sound for A or B and only one unidentified popup. The owner subsequently stated Do Not Disturb was on. | No audible acceptance; no A/B identity inferred for the single popup. |
| A/B after owner-prepared policy | Genuine round-13 Q24 authorized one pair after the owner prepared the context. Both requests were accepted and reached foreground delegates. Own logs reported `interruptionSuppression: none` / `resolutionReason: disabled`. Genuine round-14 answers reported Glass clearly, default clearly and both popups. No agent policy/volume/permission changes or audio copy occurred. | Human listening/visibility observations, not captured waveforms or new popup screenshots. Experimental local bundle lookup, not rights/release/portability acceptance. |
| Fresh signed agent permission identity | Round-16 Q27 a authorized a fresh owned denial/recovery probe. `OVRCR Permission Probe 6a24133a` had a new random bundle ID, Developer-ID/hardened signature and `LSUIElement=true`. Strict verification passed. A status-only launch returned `0/0/0` without requesting permission. The initial request remained without an authorization callback during observation; periodic status became `1/2/2`. A restart returned granted=false / “Notifications are not allowed for this application,” with no submission. Scoped OS records show a permission alert queued/presented and later removed. | Genuine round-17 Q28 c says **no permission prompt was seen**, and Q29 b says permission had not yet been changed. Neither SDK denial nor OS presentation logs prove a human denial or visible opt-in. Do not generalize this result to unsupported signed agents or blame Focus without evidence. The initial callback/display-path issue remains unresolved. |
| Same signed agent after explicit Settings recovery | After the owner was instructed to enable only that fixture's Allow Notifications, periodic native read-back changed to `2/2/2`; the owner subsequently said “done.” The agent did not change the toggle. Relaunching the unchanged identity returned granted=true and accepted one silent synthetic notification. Its foreground delegate fired and a fresh window-owner-checked crop captured the visible ready banner. | Measured status recovery, successful authorization and visible banner on this host, not a witnessed first-use OS choice, sound test or complete click path. The previous process expired before the initial recovery-submission attempt; its failed ownership preflight sent nothing. |
| Signed agent history-card input attempts | A scoped synthetic history-card screenshot was retained. An expired transient banner had no selectable control; a later history capture also found no control. A subsequent fresh history capture and pixel-post click attempt completed, but the expected callback assertion failed. A bounded subsequent owner-input observation also recorded no callback. | No input effect or callback acceptance is inferred from a successful event post. The signed-agent callback was unverified at this attempt; the later native CUA results are recorded below. The earlier warm/cold callback successes used the separate diagnostic control, not this fresh signed-agent identity. No notification was resubmitted to retry a click. |

An attempted history-card screenshot used stale geometry after the panel disappeared and captured unrelated browser chrome. It was deleted, excluded from evidence and not published. The retained permission-notice crop is not evidence of a response-ready banner.

## Signed-agent native CUA click continuation (2026-09-30)

**Passed: input delivery and warm/cold callback payloads for the intended signed agent.** The owner genuinely submitted `click-approval` Q1 **both, ready for warm and cold** at `2026-09-30T17:56:02.098Z`. Exactly two new synthetic requests were submitted with `--submit` and no sound flags. Source inspection confirms that this path sets `content.sound = nil`. No additional permission identity, notification mechanism, Server, Dashboard or terminal session was introduced.

The unchanged recovery app was copied with symlinks preserved into `/Users/xlyk/Applications/OVRCR-Permission-Click.vynm29s4/OVRCR Permission Probe 6a24133a.app`. Executable, Info.plist and sealed-resource bytes matched the recovery bundle; strict signature verification passed. The same bundle ID `com.ovrcr.permissionprobe.6a24133af4fb423c800a5d6e81a1c723`, `LSUIElement=true`, early accessory activation and Developer-ID/hardened configuration were retained. No re-signing, secure-timestamp or notarization claim was made. Fresh status-only queries before and after testing returned authorization/alerts/sound `2/2/2`; the initial scoped cleanup also read back zero owned pending/delivered requests.

| Check | Measured result | Independent proof of input delivery |
| --- | --- | --- |
| Warm | Native CUA clicked the fresh accessibility container for the exact synthetic card. The still-running owned PID `87748` reached `callback` at `2026-09-30T18:15:35.000Z`. | The owned card disappeared from the subsequent native CUA AX state; the persistent event log independently contains `fixture=ovrcr-contract-probe`, `session=probe-session`, and `action=com.apple.UNNotificationDefaultActionIdentifier` on that same PID. |
| Cold | The second approved request reached `will-present` on submitter PID `78143`. Its executable path was verified before termination; a fresh process query found zero owned instances before the click. Native CUA clicked the newly identified exact synthetic card. | The card disappeared, then LaunchServices launched new owned PID `92589`, which reached the same expected callback at `2026-09-30T18:20:58.365Z`. Its argv had no `--submit`; its events contained no authorization, submission, foreground presentation or sound request. The ordinary periodic-settings timer continued. |

Event assertions filtered PIDs through `launched` entries for this exact bundle ID, within each recorded test's log range. App activation alone was not counted. The native CUA clicks used freshly identified accessibility element indices, not the previous helpers' unverified pixel-post clicks. The owner assisted only by opening Notification Center and, for the warm test, explicitly reporting the card untouched; the input being proved was delivered by native CUA. The inspected `permission-ui inspect` helper was used read-only to locate exact-card crop bounds. CUA screenshots retained only the synthetic card; no unrelated notification was clicked or retained as evidence.

**Failed attempts, preserved:** several native CUA Notification Center/Control Center observations timed out; a display-name target lookup failed, and CUA rejected the Fn key. These observation/navigation failures sent no card click and are not delegate failures. A first cold-result verifier incorrectly required exactly `launched, callback` and failed when the unchanged source's 30-second `periodic-settings` timer fired. Its failure is retained. The final verifier explicitly allowed that documented lifetime event while retaining exact payload, new-PID/path/argv, zero cold resubmissions, and forbidden-event assertions. No probe change was made to pass the test.

**Unverified:** the cause or delivery of the historical pixel-post clicks remains unknown; these later successful clicks do not prove those earlier inputs landed. First-use permission-alert visibility/completion remains unresolved. This single-host disposable-probe result does not prove production navigation, release/notarization acceptance, other macOS versions, Glass rights/use interpretation, or the entire #221 gate.

Exact evidence directory: `/tmp/ovrcr-native-contract-ZwWUQp/cua-click-20260930/`:

- Approval/identity: `click-approval-questions.json`, `click-approval-answers.json`, `click-approval-answers.txt`, `preflight.json`, `owned-resources.json`, `fresh-settings.json`, `pretest-request-state.json`.
- Warm: `warm-launch.json`, `warm-events-before-click.json`, `warm-ax-current.txt`, `warm-owned-card-before-timeouts.jpg`, `warm-cua-target-before-click.txt`, `warm-cua-state-after-click.txt`, `warm-result.json`.
- Cold: `cold-launch.json`, `cold-pre-click.json`, `cold-card-bounds.txt`, `cold-owned-card.jpg`, `cold-cua-target-before-click.txt`, `cold-cua-target-at-click.txt`, `cold-cua-state-after-click.txt`, `cold-verifier-failed-1.json`, `cold-result.json`.
- Diagnostics/cleanup: `cua-diagnostics.json`, `cleanup-verified.json`, `checkout-before.json`, `native-contract-sources-before.md`. Both real callbacks also remain in `/tmp/ovrcr-native-contract-ZwWUQp/resume-events.jsonl`.

## Compatible-peer contract and measured foundation

The current Server wire protocol is already versioned: `crates/ovrcr-protocol/src/codec.rs` exchanges `OVRC` plus big-endian protocol version **25** before any length-prefixed bincode frame; frames are bounded to 1 MiB. `server/connections.rs::handle_connection` rejects a mismatch before decoding requests or allocating the Dashboard owner. `src/client.rs::connect_if_running` bounds the handshake to five seconds and never starts a missing Server; `connect_or_start` does start one and must not be used for notification clicks. Control request/response callers reuse `ovrcr_protocol::client`.

Existing regression checks were executed against this baseline: two protocol preamble unit tests, one real-Server wrong-version refusal test, and one missing-Server no-start test all passed. These prove the current Server boundary, not a new Bridge integration.

The minimum compatible contract for later slices is two exact compatibility checks, not a general negotiation framework:

1. The native display adapter and producer declare one explicit Bridge envelope/schema version and reject an unsupported version before alert submission or activation. A typed mismatch must reach the Dashboard's unavailable footer with update guidance. A helper acknowledgement is best-effort submission, not visible/audible delivery.
2. Callback routing reuses the existing Server preamble and bounded **connect-if-running** path. A Server wire mismatch, unavailable Server or unsupported navigation request is a failed/no-op route, never an instruction to start/restart/kill a Server, claim a Dashboard or recover a session. The installed Bridge is separately updated; ordinary CLI builds do not overwrite it. The chosen Bridge release must declare the Server wire version it supports.

A disposable native admission experiment now tests this boundary before `NSApplication.shared`: an exact Bridge schema version and supported Server wire version are checked before permission requests, notification submission or activation. The real executable's regression first failed behaviorally because the original probe entered its normal application lifetime instead of rejecting the request. After the guard, the signed/strictly verified executable passed four cross-process cases: schema 2/wire 25 and schema 1/wire 26 returned exit 78 plus a typed `incompatible` reply and caller-visible update guidance; a nonnumeric schema returned exit 64 / `invalid`; schema 1/wire 25 returned exit 0 / `compatible`. None emitted application/notification lifecycle events. No Server was contacted or started.

This is native prototype evidence, not production IPC or a Dashboard-footer integration test. The prototype's argv/JSON receipt does not select the release transport encoding. Later slices must expose the compatibility reply through bounded production delivery and translate it into the unavailable footer. Do not forward the current general Server mismatch text blindly: `exchange_preamble` suggests shutdown/restart, whereas the Bridge path must fail without managing the Server. The baseline wire is 25; adding navigation requests/events requires the normal wire-version bump, and the release Bridge must declare the resulting version rather than freezing this prototype's 25.

`Request::Select` is not the click route: it attaches a view, and ordinary view acquisition can recover a session. Later slices must add fenced navigation through the sole active Dashboard, with original Server lifetime/session run revalidated at routing and application.

## Remaining facts and later acceptance

#221 is a verification-only enabling slice. It must resolve the signed native identity/permission path, applicable Glass use and the compatible contract before dependent implementation. It does not require a production Bridge, Dashboard footer integration or final release distribution to exist already. Those checks belong to their later owning feature/release tickets and must not be counted as passed here. In particular, the successful human sound observation does not require recording unrelated audio or inventing a waveform requirement.

The fresh identity now establishes a not-determined baseline, a denied-to-authorized Settings recovery, a visible signed-agent banner, and later warm/cold response callbacks proved through native CUA in the unchanged signed-agent configuration. It does not establish a witnessed first-use opt-in/denial: the owner saw no permission prompt and the initial authorization callback did not return during observation. Earlier signed-agent history input attempts produced no response callback and remain inconclusive about input delivery; the later successful clicks do not rewrite them. The unresolved first-use permission path and the Glass license/use interpretation still prevent a GO. Single-host Settings/signing/update/callback results remain evidence with stated portability/distribution limits, not blanket claims about every macOS release.

| Fact or later check | Next action |
| --- | --- |
| Release-grade popup/sound evidence | Diagnostic-control callbacks and human-observed A/B audio remain measured. The fresh signed agent now also has scoped card screenshots and warm/cold default-action callbacks proved through native CUA with the expected payload. Keep release-app supported-version acceptance separate; no waveform requirement is added. |
| First-use permission display/completion | Fresh baseline `0/0/0`, denied-state failure and Settings recovery to `2/2/2` are measured without resets. Investigate why the owner did not see the logged permission alert and why the initial authorization callback remained pending. The unchanged Dock-less signed agent now passes separate native CUA warm/cold callback checks; the boundary of the earlier unverified input attempts remains unknown. |
| Later release distribution | #221 measured local CLI-rebuild isolation and signed-update permission continuity. Secure timestamp/notarization/distribution and the actual production release installer remain required later acceptance, not a production implementation prerequisite added to this verification-only ticket. |
| Settings portability/support | The candidate URI now reached an AX window titled `Notifications` on 26.5.2. Establish the owning/stability mechanism and supported-version behavior; do not generalize this single-host result. |
| Applicable Glass rights/use interpretation | Native main-bundle lookup and the system-directed symlink's signing layout now have source/native evidence; the missing-resource guard prevented fallback. Confirm the applicable license interpretation for this noncopying installed-OS use. Do not require a redistribution grant for an operation that does not redistribute audio, but do not claim legal clearance from SDK/signing/playback results. |
| Production compatibility integration | Existing Server admission/no-start regressions and signed native pre-application mismatch admission are measured. Later feature slices must carry the typed failure through the bounded real producer/host boundary into the unavailable footer, after the Server navigation wire-version bump. The prototype is not that production integration. |
| Exact authorized iTerm focus | After preceding gates, measure identity mapping, authorized focus and no-prompt unavailable/denied cases on existing disposable sessions. |

#221 remains incomplete. Release-grade native sound/asset-rights acceptance, production TDD/integration coverage, the full Rust verification suite and independent Standards/Spec review have **not** passed or completed. Publishing this documentation checkpoint does not close those gates. Four focused baseline protocol/lifecycle regressions passed; this is not a full-suite or Bridge feature claim.

Resume evidence includes `recording-red.log` / `recording-green.log` (a real failed/passed regression for events surviving discarded stdout), `resume-events.jsonl` (warm/cold callbacks), `resume-owned-card-3.png` and its AX/capture logs, before/after CLI hashes, signed-update continuity JSON, `settings-destination-3.txt`, and sound-candidate/request records. A failed ownership assertion in an earlier capture script matched the executable against argv too strictly; that attempt was preserved, both owned processes were verified/stopped, and the cold case was rerun with fresh UI state. No failing attempt was counted as a pass.

Sound-retry evidence is separate: `sound-retry-1.json` plus own-bundle logs and round-12 answers record silence under Focus; `round-13-answers.json` records owner readiness; `sound-retry-2.json` and own-bundle logs record requests/delegates with no suppression; `round-14-answers.json` / `.txt` and `sound-retry-2-human-observation.json` retain the attributed successful listening/popup observations. Earlier failures and the round-11 timeout remain evidence, not overwritten successes.

Compatibility evidence: `check_compatibility.py`, `compatibility-red-1.log`, `compatibility-green-1.log`, `compatibility-typecheck-1.log`, `compatibility-build-1.log` and `compatibility-signature-1.json`. This is TDD for a disposable native admission guard only; it does not add production Bridge coverage or constitute a full Rust verification pass. The red process was owned by the test subprocess and terminated on its bounded timeout; after the signed green cases, a fresh application query returned zero owned fixture processes.

Round 15 offered a fresh, separately named task-owned identity to establish opt-in/denial provenance without resetting existing records. Its 600-second readiness sheet expired without submission; no fixture/request/choice resulted from that round. Round 16 genuinely reopened Q27 and received **a, ready for denial then recovery**, at `2026-09-30T15:57:52.640Z`. Round 17's actual observations at `2026-09-30T16:25:02.442Z` were **Q28 c, no prompt seen**, and **Q29 b, not enabled yet**. Readiness was not mistaken for an OS choice. The later native authorized read-back and the owner's “done” acknowledgement are separate from those observations.

Fresh-agent evidence includes `permission-owned-resources.json`, `permission-baseline-1.json`, `permission-denial-first-request.json`, `permission-denied-retry-1.json`, `permission-recovery-status-2.json`, `permission-recovered-submit-1.json`, `permission-recovered-banner-1.png` / AX capture log, and `permission-recovered-history-2.png` / capture and attempted-input logs. `permission-final-callback-observation.json` records zero callbacks; the callback assertion failure is not a pass. Scoped OS permission records are in `permission-own-system-log-1.json`; the initial bundle-ID query also matched global state dumps, which were excluded from the retained evidence and must not be used as fixture evidence. No rights, Focus, input-delivery or human permission choice is inferred from these records.

## Task-owned cleanup

The earlier signed cleanup entry point did not request permission or submit a notification. It removed only fixture request identifier `ovrcr-contract-probe`; SDK queries then reported zero owned pending and delivered notifications. That installed fixture directory was removed and its recovery bundle retained. Continuation reinstalled the same owned fixture in another unique directory for the new checks. The A/B sound requests were subsequently removed and SDK queries again returned zero owned pending/delivered notifications; no authorization/submission occurred in that cleanup entry. After the observation sheet timed out, the resumed cleanup entry again reported zero owned pending/delivered requests without authorization/submission, and the owned installation was unregistered and removed. No fixture process remained before cleanup; the signed recovery bundle and evidence were moved back to the task temporary root. `resume-final-cleanup-verified.json` records this resumed cleanup independently from the earlier one.

The requested sound retries reinstalled the signed recovery app in another unique task directory. After each pair, the cleanup entry reported zero owned pending/delivered requests without authorization/submission. After round 14, a fresh owned-process query returned zero, only the unique installation was unregistered/removed, and the recovered bundle passed strict signature verification. `sound-retry-final-cleanup.json` records this cleanup.

The fresh permission fixture's cleanup entry removed only its synthetic request identifier and reported zero owned pending/delivered notifications, without requesting permission or submitting. Live owned instances were path-checked before stopping. Only its unique installed directory was unregistered/removed; its separately signed recovery bundle was retained under the evidence root and passed strict verification. `permission-cleanup-verified.json` records the result. The earlier recovery fixture was untouched.

The native CUA continuation stopped only path-verified PIDs from its unique installation. Its scoped cleanup entry reported zero owned pending/delivered requests without authorization or submission, a final status-only query retained `2/2/2`, and fresh process inspection found zero owned fixture processes. Only `/Users/xlyk/Applications/OVRCR-Permission-Click.vynm29s4` was unregistered/removed. The separately signed recovery app remained byte-identical and passed strict verification; its Glass symlink and the OS-managed permission record were preserved. `cua-click-20260930/cleanup-verified.json` and `owned-resources.json` record this independent cleanup. The approval form server exited after the genuine submission.

System Settings and the workspace/local shell were not stopped. The agent did not change shared Focus, volume, Keychain settings or production OVRCR configuration. The owner explicitly prepared the later sound policy context and was instructed to change only the fresh fixture's permission for recovery. OS-managed fixture permission records were not reset or manually deleted.

## Primary sources

[A1]: https://developer.apple.com/documentation/bundleresources/information-property-list/lsuielement
[A2]: https://developer.apple.com/documentation/usernotifications/unusernotificationcenter
[A3]: https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/Procedures/Procedures.html
[A4]: https://developer.apple.com/library/archive/technotes/tn2206/_index.html
[A5]: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
[A6]: https://developer.apple.com/documentation/usernotifications/unusernotificationcenter/requestauthorization(options:completionhandler:)
[A7]: https://developer.apple.com/documentation/usernotifications/unusernotificationcenter/getnotificationsettings(completionhandler:)
[A8]: https://support.apple.com/guide/mac-help/mh40583/26/mac/26
[A9]: https://developer.apple.com/documentation/usernotifications/unusernotificationcenterdelegate
[A10]: https://developer.apple.com/documentation/usernotifications/handling-notifications-and-notification-related-actions
[A11]: https://developer.apple.com/documentation/usernotifications/unnotificationsound
[A12]: https://developer.apple.com/documentation/usernotifications/unnotificationsound/init(named:)
[A13]: https://www.apple.com/legal/sla/
[A14]: https://www.apple.com/legal/sla/docs/macOSTahoe.pdf
[I1]: https://iterm2.com/documentation-scripting.html
[I2]: https://iterm2.com/python-api/session.html
[I3]: https://iterm2.com/python-api/tutorial/running.html
[I4]: https://iterm2.com/python-api/tutorial/troubleshooting.html
