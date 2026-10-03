# Permission-only diagnostic for #221

This is a prepared disposable diagnostic, not a production Bridge. It separates
permission from banner submission in the earlier retained `NotificationProbe.swift`.
It has not been installed, signed, launched, or used to request permission.

`--status-only NEW_ABSOLUTE_EVENT_FILE` reads settings and exits with a ten-second
deadline. `--request-permission-only NEW_ABSOLUTE_EVENT_FILE` requests alert/sound
authorization once, only after application launch and a recorded `notDetermined`
baseline. It persists the independent completion receipt and final settings, or
records a 120-second deadline. Polling does not stand in for the callback or the
owner's visible permission choice. Neither mode submits a banner, plays audio,
opens Settings, contacts a Server, or resets native policy. Existing event files
are refused; output persists independently of launcher stdout.

Main-queue invocation after launch is a diagnostic simplification, not an
established fix for the prior failure. The intended bundle retains `LSUIElement=true`
and accessory activation. Do not add a diagnostic window or activation behavior
without identifying that experiment separately.

## Exact approval scope for the next run

Issue #221 requires explicit approval for executable native probes. The earlier
session's silent-notification approvals do not authorize this run. After fresh
owner approval, use one new random bundle ID/name and one unique task installation
under the owner's Applications directory; compile and sign with the existing
Developer-ID identity without any credential, Keychain, certificate or permission
configuration change. Stop if signing needs interaction. No secure timestamp,
notarization or production distribution claim follows from local signing.

Kyle approved this one-shot diagnostic on 2026-10-03 and subsequently explicitly
authorized native CUA to accept its notification permission. CUA may click Allow
only after the new fixture's exact name is visible in that permission prompt;
unrelated permission or signing dialogs are outside the approval. This supersedes
waiting for the owner at the computer. It does not change the historical evidence
that the earlier owner saw no prompt or establish a result for this new test.

Register that owned bundle and first run status-only. Proceed only if its recorded
authorization is `notDetermined`. Run permission-only once while the owner is ready
to observe, or under the explicit CUA approval above. Save native CUA evidence scoped
to this fixture's permission UI; attribute a CUA choice separately from an owner's
observation. A missing prompt, absent callback,
unexpected state or deadline remains unresolved; do not reset permissions or retry
automatically. Capture the resulting settings without changing them. Then verify
the owned process exited, unregister and remove only the unique installation, and
retain source, signed recovery bundle and evidence. The OS-managed permission record
remains. This diagnostic needs no new banner, sound, click test, live session or
terminal-control permission.

## Local preparation check

Type checking does not launch the app or exercise permission behavior:

```sh
swiftc -swift-version 5 -typecheck -module-cache-path /tmp/ovrcr-permission-typecheck-cache PermissionOnlyProbe.swift
```

Only the real fresh-identity run and attributed owner/CUA evidence can resolve the
first-use gate. A compiler pass and synthetically generated logs cannot do so.
