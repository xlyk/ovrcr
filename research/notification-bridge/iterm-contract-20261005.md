# Existing iTerm session focus: source contract, 2026-10-05

**Issue:** [#225](https://github.com/xlyk/ovrcr/issues/225), after #223. **Baseline:** `ecc854265e093ce79f2c881a8a92ee1d557ce470`. **Decision:** primary API/source facts support an isolated local adapter and dependent source implementation. Actual native setup, permission behavior and terminal focus remain **unperformed**. This is not native acceptance or permission to execute the adapter.

The current #222 approval covers its silent banners and notification-permission recovery. It does not cover Apple Events, iTerm control, Accessibility or their permissions. No native APIs, Apple Events, terminal control, application launch, signing, CUA or provider execution were used for this research. Read-only inspection found `/Applications/iTerm.app` version **3.7.2**, bundle ID `com.googlecode.iterm2`. Its scripting dictionary exactly matches official tag `v3.7.2`, commit `8f1f5170a423cb73a32be1a2487434f5984d5f24`.

## Verified mechanism and identity

iTerm's [scripting reference](https://iterm2.com/documentation-scripting.html) exposes existing windows, tabs and sessions. Session `id` and `unique ID` are identifiers; session selection changes the active pane, tab selection changes the window's selected tab, and window selection raises the window. These are different operations. None requires creating a session or sending terminal input.

At the pinned version, [`PTYSession.m`](https://github.com/gnachman/iTerm2/blob/8f1f5170a423cb73a32be1a2487434f5984d5f24/sources/PTYSession/PTYSession.m#L3012) constructs `sessionId` as `w<window>t<tab>p<pane>:<guid>` and puts it in `ITERM_SESSION_ID` at child launch (lines 3185–3192). The source notes that movement can make the environment's position prefix stale. **Ignore positions when targeting.** Parse a bounded value and use its GUID suffix to find an existing session in the current hierarchy; never reinterpret the old indices as current coordinates.

The pinned [dictionary](https://github.com/gnachman/iTerm2/blob/8f1f5170a423cb73a32be1a2487434f5984d5f24/iTerm2.sdef#L558) maps both session ID properties to `guid`, and exposes selection as Apple Event class/ID `Itrm/slct`. [`PTYSession+Scripting.m`](https://github.com/gnachman/iTerm2/blob/8f1f5170a423cb73a32be1a2487434f5984d5f24/sources/Applescript/PTYSession%2BScripting.m#L16) uses a GUID object specifier and its selection handler only sets the active session. [`PTYTab+Scripting.m`](https://github.com/gnachman/iTerm2/blob/8f1f5170a423cb73a32be1a2487434f5984d5f24/sources/Applescript/PTYTab%2BScripting.m#L50) resolves an existing session by GUID and selects the containing tab without creation. [`iTermWindowScriptingImpl.m`](https://github.com/gnachman/iTerm2/blob/8f1f5170a423cb73a32be1a2487434f5984d5f24/sources/Applescript/iTermWindowScriptingImpl.m#L3) addresses windows by their live window number and selects their current terminal.

The candidate adapter uses direct Apple Event descriptors, not executable AppleScript strings, Python connection setup, `osascript`, URL opening or shell commands. Its only target-app operations are identity/hierarchy metadata queries and selection. It never requests `contents`/`text`, creates/splits/closes a pane, changes profile/color/badge/layout, or sends input. Enumeration and each reply are bounded; missing, ambiguous, reordered or unsupported metadata fail to parent activation. Acknowledged selection and activation requests do not prove observed keyboard focus.

The [Python activation API](https://iterm2.com/python-api/session.html#iterm2.Session.async_activate) also distinguishes pane/tab/window focus, but [external Python connections](https://iterm2.com/python-api/tutorial/running.html#command-line) may prompt for their own authorization. They are not a verified no-prompt click preflight. Newer official documentation describes `iterm2:reveal` URLs; that route was not selected because this slice requires explicit setup, existing-process targeting and observable failures. No URL was opened.

## Current Dashboard contract, not remembered banner identity

The #223 coordinator agreed the applied acknowledgement carries `BridgeActivationTarget { dashboard_pid, dashboard_start_seconds, dashboard_start_microseconds, iterm_session_id }`. The Server captures actual socket peer PID and process birth on Dashboard admission. A bounded identity message supplies that **current Dashboard's** environment value to its current owner slot. The original delivered ticket contains Server lifetime/session/run fences, not a remembered terminal destination. Routing, confirmation and applied acknowledgement validate the same current owner before activation.

The adapter consumes those fields only after successful Dashboard Browse navigation. It rechecks same-UID process PID/birth and resolves a currently running ancestor iTerm application. It never uses the Bridge's or provider's environment, a saved notification destination, a bundle-ID lookup that could launch an application, or the foreground terminal as a guess. A missing identity, changed process birth/ancestry, stopped iTerm or unavailable/denied control leaves Dashboard navigation intact and uses #223 parent activation.

An environment variable is inherited context, not an OS-certified assertion of which native pane displays a process. The adapter additionally compares the current Dashboard's controlling-TTY device with the matched iTerm session's read-only `tty` metadata and local character-device identity. A stale GUID naming another native pane fails this check. GUI hosts must prefer their own application and must not focus an inherited shell's iTerm ID. Forwarded/stale identities, remote sessions, tmux with different controlling TTYs, and unusual launch ancestry require fail-closed behavior or separately verified support. The documented baseline is a local Dashboard launched in the existing iTerm session. Owner replacement and moving an existing pane require application-path tests; these source facts do not prove those tests.

The local SDK exposes `proc_pidinfo(..., PROC_PIDTBSDINFO, ...)`, same-UID/parent fields and `pbi_start_tvsec`/`pbi_start_tvusec`. Those fields support process-birth checks. `NSRunningApplication.launchDate` is optional for processes not launched by LaunchServices, so it is not the sole process fence.

## Consent, setup and ordinary clicks

Apple's [permission API](https://developer.apple.com/documentation/coreservices/3025784-aedeterminepermissiontoautomatet) and the installed SDK's `AppleEvents.h` (lines 558–615) establish the following:

- The target address must identify an already running application. Preflight checks permission without sending an Apple Event.
- `askUserIfNeeded=false` does not prompt. Permission that needs consent returns `errAEEventWouldRequireUserConsent` (`-1744`); denied control returns `errAEEventNotPermitted` (`-1743`), and absent target returns `procNotFound`.
- Only an explicit terminal-control setup action may call with `askUserIfNeeded=true`. That call can wait for the user and must not run on the main thread or be retried automatically.
- Preflight alone leaves a revoke/race gap. **Every ordinary metadata and selection send** also includes [`kAEDoNotPromptForUserConsent`](https://developer.apple.com/documentation/coreservices/3025781-anonymous), plus `kAENeverInteract`, and an explicit finite reply timeout. The no-consent flag refuses a send needing consent rather than prompting. Neither notification permission nor a previously successful preflight substitutes for this flag.

Future setup must be an explicit Dashboard action separate from `N` and notification Settings. The integration needs an explicit user opt-in; an incidental existing OS grant is not automatic enablement. Ordinary clicks call only the no-prompt path. Denial, missing setup or later revocation produces fallback/guidance, with no authorization call from the click, loop or automatic notification enabling. A setup result must describe actual current permission, not claim focus or synthesize user consent.

Apple requires [`NSAppleEventsUsageDescription`](https://developer.apple.com/documentation/bundleresources/information-property-list/nsappleeventsusagedescription) for an application sending Apple Events. A hardened sender controlling another team's app also needs the [Apple Events entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.automation.apple-events). This is **the sender Bridge's** purpose string/entitlement and authorization, separate from notification permission and iTerm's own plist key. The currently reviewed #222 bundle is not changed by this research. Future plist, build/signing, setup UI/settings and transport integration belong to their assigned owners; no signing/key/trust change is inferred. Sandboxed distribution would need its own target-specific sandbox contract; no sandbox entitlement is silently added here.

Preflight may involve OS work and explicit setup can wait arbitrarily long. The adapter has a monotonic two-second selection budget and finite per-event waits, but callers must retain their external worker/deadline/cancellation boundary. The UI must remain nonblocking. No API result alone guarantees a visible raise or exact focus.

## Parent activation and limits

Fallback resolves an existing application from the current Dashboard's live PID ancestry (or its own GUI host) and uses documented [`NSRunningApplication.activate(options:)`](https://developer.apple.com/documentation/appkit/nsrunningapplication/activate(options:)). It does not launch an application or send an Automation request. The SDK says default activation raises the application's main/key windows; it does not identify the Dashboard's exact terminal window when the application has several windows. `activateAllWindows` would raise every window, not solve targeting, and `ignoringOtherApps` has no effect starting macOS 14. A true return value acknowledges the request, not observed focus. Preserve this multiple-window limitation in UI/help and evidence.

## Pending integration and proof

Source-only ownership is this document, new `native/bridge/ITermFocus.swift` and its isolated `native/bridge/headless-iterm/main.swift` runner. Existing #223 protocol/runtime/TUI/native entry points and metadata remain coordinator-owned. The adapter provides explicit setup and no-prompt selection entry points without hooking either into accepted #222 execution.

The pure runner passed all 55 assertions on 2026-10-05: seven accepted identities, 37 rejected identities, exact 128/129-byte boundaries, GUID matching across changed positions, and three targets through each early selection guard. Rejections cover malformed/multiple separators, missing or non-ASCII position digits, oversized values, controls, and noncanonical GUID shapes. It first asserts the main thread, then calls enabled selection only there and expects `unavailable` before process/context resolution. Disabled selection expects `notEnabled` before that guard even for invalid targets. The runner never calls setup, `NSRunningApplication`, Apple Event APIs or a background enabled-selection path. Deliberately invalid GUIDs or PID 0/1 prevent live lookup if an early guard regresses. This tests parser and early-guard behavior; it does not exercise permission, metadata enumeration, selection or focus.

The coordinator granted one exclusive compiler/headless slot. With umask 077, a whitelist environment, task-private HOME/TMP/module cache and retained output, Apple Swift 6.2.4 typechecked and compiled only `native/bridge/ITermFocus.swift` and `native/bridge/headless-iterm/main.swift`, both with `-warnings-as-errors`. The compiler arguments were `swiftc -warnings-as-errors -typecheck -module-cache-path "$PRIVATE_CACHE" native/bridge/ITermFocus.swift native/bridge/headless-iterm/main.swift` and `swiftc -warnings-as-errors -module-cache-path "$PRIVATE_CACHE" native/bridge/ITermFocus.swift native/bridge/headless-iterm/main.swift -framework AppKit -framework CoreServices -o "$PRIVATE_TARGET/iterm-headless"`. Only that pure binary ran, exiting 0 with `iTerm pure headless checks: 55`. No existing native entry point, accepted app bundle, build manifest, signing step or runtime iTerm session participated.

The initial typecheck failed on nine imported integer-type mismatches; its log is preserved. Carbon object-form constants needed `OSType(...)`, record keys needed `AEKeyword(...)`, and `AESendMessage`'s bounded timeout needed `Int` instead of `Int32`. Those conversions preserve the selected codes and 120-tick cap. The corrected typecheck/compile passed without warnings, establishing SDK symbol/descriptor-call spelling and type compatibility, including process-birth fields. Successful compilation does not establish permission, event delivery, descriptor semantics in iTerm or observed focus. Local logs, exact commands, source/executable hashes, failure and success receipts, and final cleanup binding are under `bridge-validation/iterm-contract-20261005/` in the coordinator workspace; the retained binary SHA256 is `d931b5953606e107cdf2ac0a7fb3db621a11237d34db9a3690a1e2b0c34f3128`.

The completed compiler/pure-runner slice covers bounded GUID parsing and early opt-in/thread guards. Metadata-only event descriptors, existing PID/birth targeting, preflight false on selection, no-consent flags on every send, finite deadlines, missing/denied/unavailable/changed-owner fallback and no creation/input/screen-read commands still require broader implementation verification. Meaningful production application tests must bind setup and selection to current-owner applied acknowledgements, preserve Linux and the #223 Browse/transient/input/run fences, and exercise owner replacement rather than only a helper mock. Those application tests and all native-control acceptance remain unperformed by this adapter slice.

Actual native acceptance needs separate approval for the exact reviewed signed sender, purpose/entitlement, explicit first-use terminal-control setup and choice, and scoped existing disposable iTerm sessions. It must observe prior-authorized exact selection, denied/missing/revoked no-prompt fallback, ordinary-click silence from consent UI, parent activation's multiple-window limitation, no new panes/input and scoped cleanup. Existing user sessions must not be controlled for acceptance. API guarantees and fake/headless success are not those observations.

## Source pins and unavailable attempts

All successful public reads used TLS verification. The tag ref resolved to commit `8f1f5170a423cb73a32be1a2487434f5984d5f24` on 2026-10-05. SHA256:

| Primary artifact | SHA256 |
| --- | --- |
| Installed iTerm `Contents/Info.plist` | `1297bda5a3e7c305dd002c9ccf1f70e41f82182e4529a121bcc21ef5db219173` |
| Installed and pinned `iTerm2.sdef` | `7eb058981b4f10aaa2bc196362b99c7506ff7ad0f6f327dfb23674764636f464` |
| Pinned `sources/PTYSession/PTYSession.m` | `7400781a0d8325ee5883abc89ea8196ef670a54108c4ad7523fe378e64b3034f` |
| Pinned `sources/Applescript/PTYSession+Scripting.m` | `193bf643bcba8d7acf77173a6858e04c5d8f5d1da4bbab28be654cf378a675ef` |
| Pinned `sources/Applescript/PTYTab+Scripting.m` | `ff0de46049b1148d7eb049882c90cc6e1f260375437436c9f141f59b3d11b10e` |
| Pinned `sources/Applescript/iTermWindowScriptingImpl.m` | `270be2d1cdfa03a49e92f0e9ab094e52da78d111cda21dfa41c6e93d8f3a4d21` |
| Local SDK `AppleEvents.h` | `0d559d2b13ab77dfbc702bb0d92e05033251c8caa90c86db96c703218cf31ce6` |
| Local SDK `AEDataModel.h` | `709ed050dad55db88a2cd4c6fdb7a9a7d5f06ac3c8c72331f7617c141eba64c4` |
| Local SDK `sys/proc_info.h` | `e427fa96b348537b21552b9de71e01039410bcad2cedee5584c5e5fddafd70fc` |
| Local SDK `libproc.h` | `246d87709fc6b9157ce5cf3c475656ac48e0e1ae8bbdc46cf45acd34294448cd` |
| Local SDK `NSRunningApplication.h` | `b7fa5d5b933aef2165268305c631c2e8c617ba55234f83250b37cd9db1899f5e` |
| Apple purpose-key DocC JSON | `3333aca80d2cadaaa1e8ef57c9f3ff60c7468051fd1a223cbe126fc3eea770cb` |
| Apple entitlement DocC JSON | `b94fb4570442d79bb9433edeb1dfd13bb97790b5d50e9957e1bf514590dc3582` |

SDK paths are under `/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk`; AE headers are `System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/AE.framework/Versions/A/Headers/`. Apple DocC JSON was read from the official `developer.apple.com/tutorials/data/documentation/` endpoints corresponding to the linked pages.

The GitHub connector returned 404 for #225; the coordinator's freshly captured Oct5 issue snapshot supplied the requirements. Web raw-source guesses at the former `sources/PTYSession.m` path failed with 404; the public pinned tree resolved the moved paths. Sandboxed network reads lacked DNS, and Python's configured TLS trust store failed verification. The system `/usr/bin/curl` then read the public GitHub/Apple sources successfully with normal TLS verification; no certificate check was disabled. The web tool could not render Apple's Markdown responses; official DocC JSON supplied the text. These unavailable attempts are not evidence of native failure.

### Dependent setup-to-click source continuation

The isolated #225 implementation consumes the three compiled adapter/source
commits through `3fb4e59d75393a56ebd6314d0f9c862729a334d9`, plus the #223 routing
checkpoint `da7800c`. Its adapter adds a mandatory current-Server-owner guard at
context creation, every metadata/selection validation and after the explicit
permission result. Those changed bytes are **not** covered by the earlier 55-check
compiled-source receipt. Fresh warnings-as-errors compilation and the four
pure runners passed 84, 58, 51 and 47 checks respectively. The complete native
entry point was typechecked without execution. Production application tests
use a real Dashboard PTY with injected host and auth executables; their separate
receipts retain every failure and the final corrected-source runs. Actual permission requests,
Apple Events, native focus, signing and GUI acceptance remain unrun.

The product preference `iterm_focus` defaults off. The explicit Dashboard palette
setup entry obtains current admitted peer PID/birth, terminal context and a random
connection-owner token from the Server. A fixed sealed `bridge owner --stdin`
helper validates that token through the existing transport before setup and
ordinary control. Applied click targets contain the current Server reading,
not a preference remembered on the banner. Setup's separate pending state starts
at most one permission worker and never authorizes from a poll or click. An expired
request cannot start a late prompt. UI polling is bounded to thirty seconds;
an explicit later check reads that same attempt. The OS authorization call itself
may wait for the user, so it remains off the main thread and cannot be repeated
while outstanding. The worker retires results after its 120-second observation
budget; a result from a replaced owner cannot enable focus.

Future bundle source supplies the sender purpose string and single hardened
Apple Events entitlement, without changing the helper's sealed digest. This does
not change the frozen #222 acceptance bundle or authorize signing/control. The
independent #223+#225 protocol is schema3/wire36; the coordinator combines the
sound slice and latest main with another version bump before final acceptance.

The production callback's existing 1500-ms compatibility deadline is retained.
Fresh real-CLI diagnostics exposed ARM software SHA-256 exceeding that budget
on a 49,658,152-byte debug CLI, even with unchanged regular-file identity. The
runtime enables the existing locked RustCrypto `sha2` crate's `asm` feature only
for macOS/aarch64, using RustCrypto's CPU dispatch. The locked `cpufeatures`
0.2.17 treats SHA-2 as available under Apple's ARM64 platform baseline, rather
than probing it dynamically. The dispatcher retains a software fallback for
other platform policies.
Other targets retain their existing feature selection. The fix changes no
metadata, byte identity, maximum file size, timeout or callback authority guard;
a fixed independent SHA-256 digest assertion protects byte compatibility.

The retained before/after diagnostic used identical 49,658,152-byte executable
contents. The software implementation reached the 1500-ms deadline before
finishing either independent hash. The corrected production function finished
each read in approximately 0.24–0.29 seconds and both reads in approximately
0.49–0.53 seconds under one unchanged 1500-ms deadline, with the expected digest
asserted. The rebuilt CLI's regular-file/no-symlink/inode identity and cached
Server digest were also checked. Diagnostic-only test changes were retained as
a separate diff and removed before clean-source acceptance.

Early fixture runs accidentally exposed the installed Codex quota worker through
a Homebrew PATH. Their actual commands, owned process groups and absence checks
remain in the validation receipts. Corrected runs use system-only PATH plus the
absolute Rust toolchain, a private HOME/TMP, fixture auth, quota disabled before
Server startup and in every saved Dashboard document. No native iTerm or consent
operation ran. The later process audit distinguishes corrected isolation from
those earlier attempts.

The clean application run then observed `EINVAL` while resetting a valid socket
read timeout between the frame header and body. Separate diagnostics retained
the successful preamble/header reads and failing timeout operation. They do not
establish an upstream Rust or kernel defect. The macOS callback stream now uses
nonblocking readiness polling under the same absolute 1500-ms deadline for
reads and writes; other platforms keep their prior timeout path. Fragmented
frame, write, silent-peer, expired-deadline and EOF cases exercise owned socket
pairs. Transport readiness waits do not repeat a protocol request, setup action
or authorization call.
Clean-source validation of the independent #225 branch after this correction
passed strict offline, locked
workspace/all-target/all-feature Rust Clippy, all five callback transport cases,
all nine Server hash/current-owner/current-preference cases, and both real
Dashboard PTY cases with the fake host. These checks validate application routing,
status and isolation; native setup/selection and visible focus still require the
separate approval and observations described above.

Parent fallback no longer queues an accepted owner snapshot onto the main
thread. The existing bounded worker checks the current Server owner at the
activation boundary, then synchronously rechecks PID/birth/ancestry and attempts
activation. Only notification completion returns to the main queue. The local
official SDK declares `NSRunningApplication` thread safe and Swift Sendable;
`activateWithOptions:` carries no main-actor annotation. Delayed-boundary doubles
replace the connection nonce while keeping PID/birth/session identity unchanged,
and verify that replacement or expiry admits no activation. No native activation
API participates in that regression.
