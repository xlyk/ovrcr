# OVRCR Bridge

The macOS Bridge owns notification display and authorization, and forwards
default-action clicks through the existing Server connection. The Dashboard
decides eligible alerts and applies selection; the Server retains session/process
ownership. Source/build checks do not establish installed native acceptance.

Build from the checkout with Xcode command-line tools and Python 3:

```sh
scripts/build-bridge.sh --check
scripts/build-bridge.sh --callback-executable /absolute/reviewed/path/ovrcr
scripts/package-startup.sh target/ovrcr-startup /absolute/reviewed/path/ovrcr
python3 -I native/bridge/check-guards.py 'target/bridge/OVRCR Bridge.app'
python3 -I native/bridge/check-packaging.py 'target/bridge/OVRCR Bridge.app'
python3 -I native/bridge/check-installer.py
```

`--check` compiles contract, forwarding and early-guard checks using fresh task-owned
message-port names. AppKit/CoreServices types are linked for the adapter; the checks never initialize an application/notification lifecycle or launch
an app. The default build compiles and validates an uninstalled bundle, then
executes its early `--check-contract` guard only. An optional final argument
selects the output root. Existing output bundles are refused. The compiler may
emit an ad-hoc Mach-O code directory; the build does not sign/seal the app with
an installation identity. Neither build mode installs, registers or requests
native permission. `just run` builds a cached, validated runtime payload and installs that payload
beside the CLI under `~/.local/lib/ovrcr`. Interactive Dashboard startup offers
installation or repair of the app from this payload; runtime does not compile
Swift or require a checkout. The relocated validator carries exact contract,
build, callback CLI, entitlement and sound/license hashes. The cache fingerprint
includes the supplied CLI bytes, so an unchanged native source cannot retain an
older callback helper after a CLI rebuild. The package command requires the
reviewed CLI as its second argument; `just run` supplies the CLI it just built.
The relocated checks preserve installer refusal without an identity and reject
changed helper metadata, missing entitlements, altered purpose and sound bytes.

Startup defaults to no. An accepted install uses the single valid non-ad-hoc
signing identity when available, or `OVRCR_BRIDGE_SIGNING_IDENTITY` when set.
Missing or ambiguous identities leave the app unchanged and produce a warning.
Optional setup failures do not prevent Dashboard attachment. No notification
permission operation runs as part of installation.

The build requires an explicitly reviewed CLI matching the running Server. It
copies its exact bytes into the fixed `Contents/MacOS/ovrcr` helper and records
SHA256 in `OVRCRCallbackSHA256`. The enclosing app signature seals that metadata
and helper. The CLI must already have a valid Mach-O integrity signature;
compiler-produced ad-hoc signatures are accepted. Packaging does not re-sign or
rewrite helper bytes. Installation strict/deep verification also validates the
nested helper. No builder-specific source path is stored in the app. Updating
CLI or Bridge bytes without a matching Server restart/package makes clicks fail
closed; there is no alternate executable or retry.

Install explicitly, after the relevant native operation is authorized:

```sh
scripts/install-bridge.sh --identity 'YOUR SIGNING IDENTITY'
```

The default source is `target/bridge/OVRCR Bridge.app`, with identity
`com.ovrcr.bridge`, display name `OVRCR` and default destination
`~/Applications/OVRCR Bridge.app`. A final positional argument selects a built
source bundle; `--destination PATH` selects an installation destination.
The installer copies into a private staging directory, signs there with the
explicit non-ad-hoc identity, checks the strict signature and bundled versions,
and verifies regular resource bytes/license. It verifies an existing app and
refuses a changed designated signing requirement before replacement. A failed
rename attempts restoration; unresolved recovery state is preserved.
Signing/notarization and supported-release acceptance remain separate checks.

`check-installer.py` runs the shipping installer against seven private fixtures
with fake signing, copy and CLI executables. It checks fresh installation,
matching designated requirements from either output stream, changed requirements
and empty requirements, preserving exact old-bundle bytes on refusal. It requires
macOS's read-only PlistBuddy; it performs no actual signing, native app launch,
permission, notification or audio operation.

For a separately approved native fixture, `--bundle-id ID --display-name NAME`
sets the explicitly expected metadata of an already prepared source bundle.
Changing either requires an explicit destination distinct from the production
installation. Use a unique bundle ID, distinguishable display name and owned
destination together. These options validate metadata; they do not modify the
source bundle. The builder retains the fixed production defaults.

Updating the disk bundle leaves an already running Bridge process in place.
A new client can therefore receive `incompatible` from an older endpoint.
After an intentional update, use Activity Monitor to inspect the selected
`OVRCRBridge` process's executable path, confirm it belongs to this installation,
and quit that exact process. The next opted-in Dashboard request starts the
installed app. Never terminate unrelated processes by a broad name match.

The client executable is
`~/Applications/OVRCR Bridge.app/Contents/MacOS/OVRCRBridge --client`. Feed one
UTF-8 JSON request on stdin and close stdin. It emits exactly one JSON reply
plus a newline, with exit 0 for all typed statuses. The total input/reply limit
is 1 MiB; its 1.8-second deadline includes input, startup and IPC. Client argument
misuse also returns typed `failed` with exit 0; non-client CLI misuse exits 64.
`--check-contract SCHEMA WIRE` exits 0 for a match, 78 for a
mismatch and 64 for invalid arguments, before any app/notification/IPC lifecycle.
Both versions are derived from the shared Rust constants at build time.

Schema 4 / wire 39 requests carry `schema`, `server_wire` and an `op` tagged by `type`:
`status`, `authorize`, `settings`, `iterm_setup`, `iterm_status`, or `deliver`
with `title`, `subtitle`, `body`
and nullable `sound`, plus `navigation`. Null sound is silent; the only choices
are `default`, `tap`, `chime`, `rise`. The ticket contains versions, original
Server socket and CLI path, CLI SHA256 cached at Server startup, a fresh
Server-lifetime UUID, session ID and original run. It contains no generated
subject, prompt, answer or Input request payload.
Replies carry the two versions and `status`: `available`,
`not_determined`, `permission_pending`, `denied`, `submitted`, `settings_opened`,
`incompatible`, or `failed`, plus optional `sound_unavailable`. Explicit iTerm
operations return `iterm_pending`, `iterm_authorized`, `iterm_authorization_required`,
`iterm_denied`, or `iterm_unavailable`. No raw caller input or OS error text is reflected.
Unknown fields are ignored. Delivery admits only the fixed Ready/Input title
literals, at most 320 UTF-8 subtitle bytes and 1024 body bytes, rejecting control,
line/paragraph and bidirectional control scalars. The producer sanitizes identities
before this defensive check. The native adapter copies these three admitted
display fields, the allowlisted sound choice and the opaque navigation ticket.

The transient client checks versions/admission before contacting the native app.
It launches its own enclosing bundle once through
[NSWorkspace](https://developer.apple.com/documentation/appkit/nsworkspace/openapplication(at:configuration:completionhandler:)),
with [a separate application instance](https://developer.apple.com/documentation/appkit/nsworkspace/openconfiguration/createsnewapplicationinstance).
The persistent app claims a per-user message port derived from its bundle ID
and UID before AppKit/notification initialization; a duplicate owner exits.
Requests wait for the launch callback within the same bounded operation deadline,
so endpoint creation does not expose an uninitialized notification delegate.
The client does not spawn the persistent process into its own process group.
This local, same-user transport is not a caller authentication/security boundary.
There is no delivery retry or app-owned unbounded work queue.
[CFMessagePort request/reply](https://developer.apple.com/documentation/corefoundation/cfmessageportsendrequest(_:_:_:_:_:_:_:))
uses explicit send/receive timeouts and a
[background serial dispatch queue](https://developer.apple.com/documentation/corefoundation/cfmessageportsetdispatchqueue(_:_:)).
One native operation remains outstanding until its OS callback returns; a stalled
callback makes subsequent ordinary operations fail rather than accumulating work.

`status` and `deliver` never request permission. Explicit `authorize` requests
authorization once when undetermined and immediately reports pending. Owned
pending state overrides interim denied settings until the callback completes;
subsequent status reads fresh overall and alert settings. Denial is never retried
automatically. `settings` explicitly opens the documented System Settings app;
the Dashboard directs the user to **Notifications → OVRCR** manually. No pane
URI, automatic Settings opening or permission toggle is used.

Only a Dashboard with both notification and sound consent enabled sends a
sound ID; the Bridge does not interpret settings or eligibility. `submitted`
means the native add request succeeded; it establishes neither visibility nor
audibility.

Absent/null sound maps to `sound=nil`; `default` maps to the ordinary
[system notification sound](https://developer.apple.com/documentation/usernotifications/unnotificationsound/default).
Custom choices map only to `ovrcr-tap-v1.wav`, `ovrcr-chime-v1.wav` and
`ovrcr-rise-v1.wav`. The Foundation/CryptoKit preflight checks regular readable
in-bundle files, bounded reads, exact pinned SHA-256, mono 48 kHz PCM16 WAV headers
and exact frame counts/durations (0.22/0.84/0.64 seconds). The manifest and MIT
notice remain packaged as regular resources; no Apple audio or player is used.

[Named-sound lookup](https://developer.apple.com/documentation/usernotifications/unnotificationsound/init(named:))
checks app-container `Library/Sounds`, configured group containers, then bundle.
The helper uses Foundation's
[current sandbox/user home](https://developer.apple.com/documentation/foundation/nshomedirectory())
for the known `Library/Sounds` candidate. This build has no app-group entitlements
or additional sound containers. A matching higher-priority filename is a
`lookup_conflict`; unreadable lookup metadata also fails closed. This source
mapping does not claim actual OS lookup or native hearing has been observed.

Missing/unreadable/invalid custom resources or a lookup conflict select silence,
preserving the requested choice outside the Bridge. After a successful otherwise
eligible silent add, `status=submitted` carries one of `missing_resource`,
`unreadable_resource`, `invalid_resource`, `lookup_conflict`. Failed/timed-out add
returns `failed` without this field. There is no automatic system-default or
other-tone substitution, player, retry or permission/policy bypass. Foreground
presentation requests `.sound` only when content has a sound object; OS policy
still governs playback. Reinstalling resolves damaged bundle resources; a known
lookup conflict requires an explicit alternate selection or owner investigation,
and the app never removes competing user data.

Each notification carries its admitted ticket in `userInfo` alongside its
selected sound or silence. Every Dashboard delivery carries all six operation
fields, including `sound=null` when silent. Only the default notification action
forwards a callback. The persistent app admits one callback at a time with a
3.8-second overall budget, including at most 1.8 seconds for navigation and the
remaining time for current-owner focus/fallback checks. It verifies its enclosing
signature, the fixed helper's sealed SHA256, the original CLI's current regular
same-user bytes and the cached ticket digest. It launches only that fixed helper
with `bridge navigate --stdin`; ticket paths never authorize an executable and
no shell is used. The helper owns a 1.5-second connect/read/write budget and never
starts a Server or Dashboard. Input/reply frames are capped at 1 MiB. An owned
client process group may be killed before reap on failure/timeout; after reap
numeric PGIDs are never signaled. The persistent Bridge remains independent.

The Server routes through the current active Dashboard owner and revalidates
lifetime, unarchived run, callback connection and deadline at confirmation and
applied receipt. Missing/reopened/archived targets and absent/draining/replaced
owners are no-ops. Original-run process exit is still navigable. The Dashboard
lands Browse through its normal selection and view acknowledgment paths,
cancels unsent overlays/history/copy/old input and preserves submitted targets,
Unread and Input requests. Only an applied current-owner receipt returns the
socket peer's PID/birth/terminal identity. The Bridge checks the live same-user
process birth and current ancestor chain before requesting AppKit parent-app
activation. Parent-app fallback does not create a terminal or select an exact
tab/window. Exact existing-iTerm selection follows the separately enabled and
previously authorized flow below. Ordinary clicks never request terminal-control
permission, reopen/recover a run or send input. OS activation is best effort and
does not establish foreground success.
While shutdown WIP-save questions are queued, the Dashboard ignores offers and
matching confirmations without hiding the questions or inferring an answer.
The local same-user transport is not a security boundary against concurrent
modification by that user; signature/hash checks fail closed when observed
changes or mismatches occur.

Unverified native cases include signed installation/update, LaunchServices
process separation, first-use pending/Allow/denial, explicit Settings opening,
native banner visibility, listening to each selected tone/default for both
alert kinds, default-action navigation and parent-app activation. Those require an exact
reviewed revision and separately authorized task-owned native acceptance.

## Explicit iTerm setup and ordinary clicks (#225)

The Server's default-off `iterm_focus` preference is independent of notifications.
Only the explicit Dashboard palette action prepares a setup target for its current
admitted owner. A random per-connection owner token plus Server lifetime and
sealed CLI digest fence setup and ordinary activation. The fixed bundled helper's
`bridge owner --stdin` command validates through the existing Server transport;
it cannot discover or start a Server, recover a session or request OS consent.

Setup starts one background `AEDeterminePermissionToAutomateTarget(..., true)`
call, reports typed pending status and never retries automatically. Status polling
cannot start it. Every ordinary selection preflight passes `false`; every metadata
and selection Apple Event includes `kAEDoNotPromptForUserConsent`. The adapter
revalidates the current Server owner before each operation, verifies the actual
Dashboard process birth/ancestry and compares the live iTerm GUID's TTY with the
Dashboard's controlling TTY. Acknowledged selection is not observed keyboard focus.

The future bundle carries `NSAppleEventsUsageDescription` and the single hardened
Apple Events entitlement in `Contents/OVRCRBridge.entitlements`. The installer
uses that file when signing the enclosing app, without re-signing the sealed
callback helper or changing its bytes. This is source packaging, not permission
or signing authority: actual setup, native control, signing and acceptance each
retain their explicit approval gates. The accepted #222 bundles are unchanged.

This combined #223/#224/#225 source includes main's workspace WIP-save messages
and Cursor quota settings and uses schema 4 / wire 39. It is incompatible with
the earlier combined schema 4 / wire 37 and wire 38 builds and independent
schema 2 / wire 35 and schema 3 / wire 36 slices. Pure
adapter, setup-state and click-policy runners are under `headless-iterm*`; their
checks use early guards or injected permission/current-owner/selection doubles.
`scripts/build-bridge.sh --check` compiles and runs all four pure runners.
No such pass proves native consent, session selection or visible focus. Production
application tests exercise the actual Server, CLI, PTY Dashboard/palette and a
fake native host. Native screenshots/accessibility acceptance remains unrun.
