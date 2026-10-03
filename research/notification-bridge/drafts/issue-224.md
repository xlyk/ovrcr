## Parent

https://github.com/xlyk/ovrcr/issues/220

## What to build

Make macOS sound part of the Bridge banner, with a Dashboard choice of original bundled short tones or system default, while preserving the user's saved notification and sound flags. This is an end-to-end settings-to-native-audio slice. Sound becomes effective only when both flags are on; the prior independent macOS player path is intentionally replaced. Linux remains independent. Feature implementation remains subject to explicit authorization.

## Acceptance criteria

- [ ] For both response-ready and input-needed alerts, request at most one banner sound only when both saved notification and sound flags are enabled. All other flag combinations produce no macOS sound. No additional afplay/independent player, retry or notification-policy bypass occurs.
- [ ] The Dashboard exposes a persisted short choice of original bundled tones and system default (the missing-setting default), with off controlled by the existing sound toggle. Use the existing settings authority; do not add a Bridge window, arbitrary file paths, conversation/output persistence or a Linux sound selector.
- [ ] Preserve existing flags, including desktop notifications off with sound on. Explain that sound requires notifications. The sound toggle may preconfigure sound but never enables banners; disabling banners retains the saved sound flag silently. Reconnect loads the saved choice and failed saves leave active settings unchanged.
- [ ] Denied/suppressed notifications or native sound policy do not trigger a separate sound fallback or alter reporting/process state. Successful submission is not reported as proof of audibility. Linux keeps notify-send/paplay behavior and independent sound-only operation.
- [ ] Meaningful unit and production Dashboard/live-Server/real-PTY tests cover all four flag combinations, migration-by-preservation, both alert kinds, choice persistence and save failure without duplicate playback. Native evidence separately verifies each bundled-tone/system-default choice and playback and policy limits on the actual signed Bridge; update macOS controls/help/config documentation to describe the intentional contract change without weakening Linux assertions.

## Blocked by

- https://github.com/xlyk/ovrcr/issues/222
