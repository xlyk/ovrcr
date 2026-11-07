# OVRCR remote web (MVP)

A loopback browser app for OVRCR. It shells out to the installed `ovrcr` CLI and
serves a phone-sized UI: session inbox, current screen, key bar, and reply box.

This is slice 0 of [`plans/2026-11-06-mobile-remote-web.md`](../../plans/2026-11-06-mobile-remote-web.md).
It deliberately has **no Tailscale and no authentication**: it binds
`127.0.0.1` and proves the browser surface first.

## Run

```sh
cd experiments/remote-web
node server.mjs
```

Open <http://127.0.0.1:8787>. It talks to whatever instance the CLI resolves
(`OVRCR_HOME`, or the default config directory), so it **reads and writes your
live sessions**. Stop it with `Ctrl-C` when you are done.

If `ovrcr` is not on `PATH`:

```sh
OVRCR_BIN="$HOME/.local/bin/ovrcr" node server.mjs
```

Environment:

| Variable | Default | Meaning |
| --- | --- | --- |
| `PORT` | `8787` | Listen port |
| `HOST` | `127.0.0.1` | Refused unless loopback, or `ALLOW_REMOTE=1` |
| `OVRCR_BIN` | `ovrcr` | CLI to spawn |
| `LIST_TTL_MS` | `5000` | Inventory cache; `terminal list` runs Git per workspace |

## What works

- Inbox grouped Needs you / Working / Idle / Ended, from
  `ovrcr terminal list --json`: activity (`waiting_input`), unread, run, agent
  label, project/workspace, elapsed.
- Session screen from `ovrcr terminal read --json --max-lines=40`, polled every
  1.5 s only while that session is open and the tab is visible.
- Key bar: Esc, Tab, Shift-Tab, arrows, Enter, Ctrl-C, 1, 2, y, n. These map to
  `ovrcr terminal keystroke` (`:escape:`, `:ctrl-c:`, …).
- Reply box: `Send` pastes text and presses Enter (`terminal send`); `Type only`
  pastes without Enter (`--no-submit`).
- Mark reviewed (exact `unread` object), pause, resume, kill.
- Deep link: `#/s/12`.
- The desktop dashboard can stay attached. `send` works regardless; `keystroke`
  is refused while the desktop focuses that session, and the banner says so.

## What it does not do

- No live ANSI screen, colors, cursor, or scrollback. `terminal read` returns the
  visible grid as plain text.
- No agent transcript, no History/Copy.
- No auth, no devices, no notifications. Loopback only.
- `send` has no run fence; a reconnect can write into a replacement run, and a
  retried send can duplicate a prompt. The UI never retries a send.
- No split panes, settings, or workspace creation.

## Security boundary

- Binds `127.0.0.1` only; refuses any other host unless `ALLOW_REMOTE=1`.
- Spawns with an argv array, never a shell. Session ids must be positive u64;
  keys must look like `:escape:`; text is capped at 32k characters; bodies at
  64 KiB.
- Logs method, path, status, and duration. Never request bodies, terminal text,
  or keys.
- Sends `Content-Security-Policy: default-src 'self'`, `no-store`,
  `no-referrer`, and `nosniff`. The UI writes server text with `textContent`.
- Do not expose this port. Tailscale Serve, pairing, and a read-only default are
  slice 1 in the plan.

## Tests

```sh
npm test           # 10 tests: HTTP, caching, argv construction, validation, error mapping
npm run test:live  # real ovrcr server against a disposable empty instance
```

`npm test` uses `test/stub-ovrcr.mjs`, which records argv and returns canned
JSON; it never touches a real instance. `npm run test:live` creates a temporary
`OVRCR_HOME`, starts a real `ovrcr server`, asserts an empty inventory, and shuts
it down.

A full session round trip (spawn a shell, send text, read the echo) is not in
these tests: this sandbox has no PTY devices, so `ovrcr new` cannot launch. Run
it manually on a machine with PTYs, or after slice 1 adds protocol-level tests.

## Browser check

The screenshots were rendered by headless Chrome against the stub CLI at 390
CSS pixels wide.

| Inbox | Session |
| --- | --- |
| ![Inbox](docs/inbox.png) | ![Session](docs/session.png) |

Chrome's headless window has a minimum width on macOS, so the phone-sized
capture uses `--force-device-scale-factor=2 --window-size=780,1688`.

```sh
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --no-sandbox --disable-gpu --disable-breakpad \
  --crash-dumps-dir=/tmp/ovrcr-crash --hide-scrollbars \
  --force-device-scale-factor=2 --user-data-dir=/tmp/ovrcr-chrome \
  --window-size=780,1688 --virtual-time-budget=4000 \
  --screenshot=/tmp/ovrcr.png 'http://127.0.0.1:8787/#/s/12'
```

## Files

- `server.mjs` — loopback HTTP server, CLI runner, routes, validation.
- `public/` — `index.html`, `styles.css`, `app.js`; no external assets.
- `test/stub-ovrcr.mjs` — argv-recording CLI stub.
- `test/server.test.mjs` — the 10 tests.
- `test/live-instance.mjs` — real server, empty instance.
