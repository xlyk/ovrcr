---
status: accepted
date: 2026-09-29
---

# Bridge alerts preserve private names and run boundaries

This refines [ADR 0004](0004-bridge-displays-alerts.md). The user confirmed the decisions below in the 2026-09-29 Bridge design interview, Rounds 1–3, and confirmed the shared understanding in Round 4. They describe the intended Bridge release, not shipped behavior or authorization to build it. Technical verification remains open.

## First slice

The Bridge is a signed, Dock-less macOS app at a stable installation path. The Dashboard starts it as needed and leaves it running for notification delivery; `just run` does not replace it. The Dashboard decides alerts, the Bridge displays them, and clicks go through the Server to the active Dashboard. The Server remains the only session/process owner. This slice creates no second Dashboard or pane owner, settings window, server-management path, iTerm tab/color/badge integration, screen reader or input sender. Linux is unchanged.

## Names and navigation

The banner title distinguishes response-ready from input-needed. Its subtitle uses the manual title, otherwise the original session name; its body carries project/workspace/terminal identity and session ID. It must not include a generated conversation subject, raw prompts, responses or answers. [ADR 0005](0005-conversation-subjects-may-replace-display.md) changed Dashboard display titles; it does not grant permission to publish conversation topics on the desktop or lock screen. The current banner body already uses the original session name. The proposed subtitle must preserve that privacy boundary rather than blindly use the displayed title.

A click may select an unarchived session only within the original Server lifetime and process run. Reopen and Server restart invalidate old clicks even when the retained session ID is unchanged. Process exit alone need not invalidate navigation. A missing target or absent active Dashboard remains a no-op, and clicking never marks reviewed. This chooses run-bound navigation over following a retained row into different work. It does not authorize process launch or automatic recovery from a click.

## macOS sound

In the intended Bridge release, macOS sound requires desktop notifications and is requested on the banner. This intentionally replaces macOS sound-only operation; Linux keeps independent channels and its existing sound path. The previously selected Dashboard short list of Glass and system default remains the design baseline. Preserve both saved flags, including an existing sound-only configuration. On macOS sound is effective only when both flags are on. The Dashboard explains that sound requires desktop notifications. `S` may preconfigure sound but never enables banners; turning `N` off keeps the saved sound preference but produces no sound. This avoids silently expanding a previous opt-in or discarding it. Current documentation and code still implement independent macOS `afplay` playback; they must not be represented as already changed.

## Delivered history and click handling

The OS and user manage delivered banner history. The first slice adds no delivered-banner retraction or replacement lifecycle; it keeps existing cancellation of pending/in-flight work and best-effort delivery. A delivered banner can therefore outlast its original Ready or Input request. Its click remains navigation subject to the Server-lifetime/run boundary above, not an assertion that the event is still current.

The user wants a valid click to switch away from ongoing work immediately, not wait for Browse mode or an additional confirmation. The user explained: “i wont be typing when i click on a notification.” The Dashboard lands in Browse, matching ordinary navigation. It dismisses transient dialogs and the palette without submitting, discards their unsent drafts, and releases History/Copy through existing retarget cleanup. Already-submitted operations remain tied to their original targets. Existing input fences still apply; no old queued input may be redirected to the target, and the Bridge never sends input.

Exact iTerm-session focusing is used only when the integration is available and authorized through explicit setup. An ordinary banner click does not deliberately initiate terminal-control authorization. Otherwise activate the parent terminal application. Notification permission is not terminal-control permission. The exact native mechanism and permission behavior require technical verification.

## Permission recovery and release boundary

Denied permission shows unavailable status and offers an explicit Dashboard action to open notification settings, without an interrupting modal or automatically opening Settings. A missing Bridge instead gets installation guidance. There is no `osascript` fallback. Requesting notification permission when notifications are enabled remains the earlier interview baseline.

The first Bridge release may consume currently eligible Dashboard alerts without waiting for Grok parity. Shared reporting remains the sole eligibility authority; the Bridge adds no provider filter and does not reinterpret Ready as task success. At reviewed revision `14c7fe5`, readiness supports Claude, Codex, Pi and Oh My Pi. Grok and Hermes are excluded. Provider evidence and limitations remain separate from the Bridge release decision.

## Remaining technical verification

Before implementation is authorized, verify stable signed installation and update behavior, compatibility with CLI/Server protocol changes, native notification authorization and settings navigation, lawful availability of the chosen Glass sound resource, and exact iTerm focusing without surprise permission prompts. These are unresolved technical facts, not user choices supplied by this interview. Banner-based sound follows macOS notification policy; submission success does not prove that a banner appeared or a sound was audible. The first slice adds no policy bypass, retries or delivery guarantee.
