# OVRCR Bridge

The macOS Bridge owns notification display and authorization state only. The
Dashboard decides eligible alerts; the Server retains all session/process
ownership. Source/build checks do not establish installed native acceptance.

Build from the checkout with Xcode command-line tools and Python 3:

```sh
scripts/build-bridge.sh --check
scripts/build-bridge.sh
python3 -I native/bridge/check-guards.py 'target/bridge/OVRCR Bridge.app'
python3 -I native/bridge/check-packaging.py 'target/bridge/OVRCR Bridge.app'
```

`--check` compiles Foundation/CoreFoundation-only checks using fresh task-owned
message-port names. It never initializes AppKit/UserNotifications or launches
an app. The default build compiles and validates an uninstalled bundle, then
executes its early `--check-contract` guard only. An optional final argument
selects the output root. Existing output bundles are refused. The compiler may
emit an ad-hoc Mach-O code directory; the build does not sign/seal the app with
an installation identity. Neither build mode installs, registers or requests
native permission. The ordinary `just run` path does not build or replace it.

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

Schema 1 requests carry `schema`, `server_wire` and an `op` tagged by `type`:
`status`, `authorize`, `settings`, or `deliver` with `title`, `subtitle`, `body`.
Replies carry only the two versions and `status`: `available`,
`not_determined`, `permission_pending`, `denied`, `submitted`, `settings_opened`,
`incompatible`, or `failed`. No raw caller input or OS error text is reflected.
Unknown fields are ignored. Delivery admits only the fixed Ready/Input title
literals, at most 320 UTF-8 subtitle bytes and 1024 body bytes, rejecting control,
line/paragraph and bidirectional control scalars. The producer sanitizes identities
before this defensive check. The native adapter copies only these three
admitted fields and ignores other metadata, including sound/click fields.

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

Every #222 notification has `sound=nil`, no `userInfo`, and no click routing or
terminal activation. `submitted` means the native add request succeeded; it
does not establish visible delivery. Original Tap/Chime/Rise WAVs, their manifest
and MIT notice are regular bundled resources for later #224. Their presence
does not enable sound; no Apple sound bytes or independent player are used.

Unverified native cases include signed installation/update, LaunchServices
process separation, first-use pending/Allow/denial, explicit Settings opening,
native banner visibility and foreground callbacks. Those require an exact
reviewed revision and separately authorized task-owned native acceptance.
