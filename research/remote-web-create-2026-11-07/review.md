# Remote web: starting a session, browser evidence

Tested: `work/mobile-remote-web-mvp`, feature tree on 2026-11-07 (commit follows
this capture). Disposable Server, one registered project (`gui`, `main` root),
Chrome headless at 390×844 @2x with device metrics emulation.

## What was exercised

- Tapped **+**, which opened the **New session** sheet.
- The sheet listed the project, its root workspace (`main (root)`), and every
  detected agent: Claude, Codex, Grok, OpenCode, Pi, Oh My Pi, Cursor Agent,
  Hermes, and Shell.
- Selected Shell, entered a name, and created. The app selected the new session,
  whose screen showed the shell prompt and cursor.
- The form defaults to the last used project/workspace/agent via `localStorage`.

Screenshots: `01-list.png`, `02-sheet.png`, `03-created.png`.

## Findings fixed during the pass

1. **Workspace identity leaked into labels.** Rows showed `gui / 1f512f…`.
   `placeOf` now maps the session's workspace id through `/api/state` and shows
   the branch (`gui / main`).
2. **A new shell opened below its prompt.** `refreshScreen` still set
   `scrollTop = scrollHeight` every poll, overriding cursor-follow. It now scrolls
   to the cursor, renders the whole screen when it is at most 128 rows, and does
   one settle pass 600ms after opening. The prompt and cursor are visible.
3. **Early + tap raced the first inventory load.** `refreshList` now shares the
   in-flight promise, so `openNew` waits for real projects/agents instead of
   immediately reporting none.
4. **Unhandled poll rejections.** Screen/list polling failures now surface as the
   existing banner instead of unhandled promises.

## Gaps seen but not changed

- The keybar overflows horizontally at 390px; it scrolls, but a more compact
  first row would be better.
- The sheet does not create workspaces; it can only use registered ones.
- The screen remains a 200-column window; no fit/resize yet.
