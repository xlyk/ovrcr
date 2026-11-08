# Remote web app: feature review

Reviewed 2026-11-07 on `work/mobile-remote-web-mvp`. Scope: the phone PWA served
by `ovrcr remote serve` and its gateway routes.

## What works now

- Session list grouped **Needs you / Working / Idle / Ended**, with unread and
  activity badges, auto-refreshed every 5s.
- Terminal screen with 24-bit color, Nerd Font glyphs, 512-row history paging,
  and a keybar for common keys.
- Agent transcript (Chat) for supported Claude/Codex sessions.
- Send text or keys, pause, resume, kill, mark reviewed, reload.
- Deep links (`#/s/<id>`), connection banner, toasts.
- **Start a new session**: project, workspace, detected agent, optional name and
  initial prompt. The gateway builds the argv; the page never sends a command.
- Settings-driven autostart and a Tailscale login gate.

## Missing, ranked by value

### 1. Live updates and push (highest)

The page polls every 5s (list) and 1.5s (screen). That is fine for watching, but
it cannot wake a phone or give sub-second feedback. A tailnet-only PWA with Web
Push, or SSE while open plus a service-worker notification when an agent needs
input, would replace the "refresh and hope" loop. This is the single largest
quality-of-life gap for remote work.

### 2. Diff review

The phone can watch and reply but cannot see what the agent changed. A Changes
view (`git status`, `git diff --stat`, then a patch) with a file list would make
the app useful for the final review step, not just supervision.

### 3. Workspace lifecycle

The new-session sheet uses existing workspaces. Add "new workspace" (branch from
base) and workspace switching in the same sheet. Without it, starting a fresh
task from the phone still requires the desktop.

### 4. Session lifecycle and recovery

Only pause/resume/kill exist. Missing: rename, close/reopen (including
interrupted/retained sessions), archived list, and delete. A phone is the natural
place to clean up and resume after a disconnect.

### 5. Find the session you mean

The list has no search or filter, and archived sessions are hidden. With dozens
of sessions, filter by project/agent/needs-you and show an archived view.

### 6. Quota and usage

The desktop shows Claude/Codex/Grok quota and context. The phone has no
usage display; a compact quota line and context percentage would prevent
surprises when starting work remotely.

### 7. Screen ergonomics

- No terminal fit/resize; the 200-column window is hard to read on a phone.
- No scrollback search or copy button.
- The keybar overflows at 390px.
These are smaller but constant friction.

### 8. Installability and auth

There is no manifest or service worker, so "Add to Home Screen" is a bookmark.
A manifest plus offline shell would pair with push. Auth is only the Tailscale
Serve header + optional login match; a device pairing token would make exposure
less dependent on Serve being configured exactly right.

## Recommended next slice

1. **Push + live events**: SSE while the page is open, Web Push for "needs you".
2. **Workspace creation in the new-session sheet**, because it is the same flow
   and removes the last desktop dependency for starting a task.
3. **Diff review**, the missing second half of remote supervision.

The smallest useful version of (1) is an SSE stream for the list and the current
screen plus `Notification` on a new unread observation; Web Push can follow.
