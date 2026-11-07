# OVRCR remote web (MVP)

A loopback browser app for OVRCR. It shells out to the installed `ovrcr` CLI and
serves a phone-sized UI: session inbox, colored screen with scrollback, key bar,
reply box, and an agent transcript.

This is slice 0 of [`plans/2026-11-06-mobile-remote-web.md`](../../plans/2026-11-06-mobile-remote-web.md),
extended with the read-only screen-window and transcript requests. It still has
**no Tailscale and no authentication**: it binds `127.0.0.1`.

## Requirements

The new screen and transcript commands need **protocol 72** on both sides: the
CLI this app spawns and the running Server. Building this branch gives you both,
but the Server must restart before the app can use them, and a restart stops live
sessions.

```sh
cargo build -p ovrcr --bin ovrcr
# stop the old Server, then start it from the rebuilt CLI
```

Until then, the app still lists sessions and can send text and keys. If the CLI
is the older one without `terminal screen`, the gateway falls back to
`terminal read` and shows plain text with no scrollback.

## Run

```sh
cd experiments/remote-web
OVRCR_BIN=/path/to/rebuilt/ovrcr node server.mjs
```

Open <http://127.0.0.1:8787>. It talks to whatever instance the CLI resolves
(`OVRCR_HOME`, or the default config directory), so it **reads and writes your
live sessions**. Stop it with `Ctrl-C` when you are done.

Environment:

| Variable | Default | Meaning |
| --- | --- | --- |
| `PORT` | `8787` | Listen port |
| `HOST` | `127.0.0.1` | Refused unless loopback, or `ALLOW_REMOTE=1` |
| `OVRCR_BIN` | `ovrcr` | CLI to spawn |
| `LIST_TTL_MS` | `5000` | Inventory cache; `terminal list` runs Git per workspace |

## What works

- Inbox grouped Needs you / Working / Idle / Ended, from
  `ovrcr terminal list --json`: activity, unread, run, agent label, elapsed.
- **Colored screen** from `ovrcr terminal screen --json`: per-cell text, width,
  `Default | Indexed(u8) | Rgb` foreground and background, and the
  bold/dim/italic/underline/inverse attribute bits. This includes the 256-color
  palette and truecolor.
- **Scrollback**: the Server retains 512 rows; the view loads the tail first and
  fetches a window when you scroll into unloaded rows. The cursor is drawn when
  the window includes the live screen.
- **Transcript** from `ovrcr terminal transcript --json`: user, assistant and
  tool entries with a "Load older" page button. Claude and Codex only, because
  that is what the Server can read.
- Key bar: Esc, Tab, Shift-Tab, arrows, Enter, Ctrl-C, 1, 2, y, n.
- Reply box: `Send` pastes and presses Enter; `Type only` pastes without Enter.
- Mark reviewed, pause, resume, kill; deep link `#/s/12`.
- The desktop Dashboard can stay attached. This app is a Control client: it
  never takes the Dashboard seat, resizes a PTY, or changes focus.

## Fonts

The terminal uses **JetBrains Mono Nerd Font Mono**, bundled as WOFF2
(`public/fonts/`, regular, bold, italic and bold italic) so a phone does not
need the font installed. It covers Nerd Font private-use glyphs, Powerline
separators, box drawing and block elements; without it those cells render as
tofu.

Rows are 16px for a 12px face. That is the font's own cell height: JetBrains
Mono's ascent plus descent is 1.32em, so 12 × 1.32 = 15.84. Matching it keeps
block and Powerline glyphs tiled vertically. Ligatures and font synthesis are
disabled on the terminal grid because the layout assumes one advance per cell.

The files are WOFF2 conversions of `JetBrainsMonoNerdFontMono` from
[Nerd Fonts](https://github.com/ryanoasis/nerd-fonts), a patched build of
[JetBrains Mono](https://github.com/JetBrains/JetBrainsMono). Copyright 2020 The
JetBrains Mono Project Authors, SIL Open Font License 1.1. The full license is
in `public/fonts/OFL.txt`.

The page preloads the upright and italic faces, `font-display: block` avoids a
tofu flash, and the server marks the fonts immutable for a day. Glyphs the font
does not contain (emoji, CJK) fall back to the system font, where column widths
can differ.

## Limits

- **Alternate-screen sessions have no scrollback.** The Server's history path
  refuses them, so the window reports `history_rows: 0` and only the live screen
  is shown. Most full-screen agent TUIs are in this class; their history is the
  transcript view, not terminal rows.
- Scrollback row numbers are positions in a 512-row ring. When the ring evicts
  old rows, the same number names different text; the page clears its cache when
  `history_rows` shrinks, but a stale row can survive a long scroll.
- A window is at most 128 rows by 256 columns per request. Tall desktop windows
  that show more than that leave a gap at the top until you scroll.
- Transcript text is the provider's, verbatim; the UI renders it with
  `textContent`, never as HTML.
- `send` has no run fence; the UI never retries a failed send.
- No Tailscale, no auth, no notifications, no settings or workspace creation.

## Security boundary

- Binds `127.0.0.1` only; refuses any other host unless `ALLOW_REMOTE=1`.
- Spawns with an argv array, never a shell. Session ids must be positive u64;
  keys must look like `:escape:`; text is capped at 32k characters; bodies at
  64 KiB.
- Logs method, path, status, and duration. Never request bodies, terminal text,
  keys, or transcript content.
- CSP `default-src 'self'`, `no-store`, `no-referrer`, `nosniff`; server text is
  written with `textContent`.
- Do not expose this port. Tailscale Serve, pairing, and a read-only default are
  the next slice.

## Tests

```sh
npm test           # 13 tests: HTTP, paging args, caching, validation, error mapping
npm run test:live  # real ovrcr server against a disposable empty instance
```

`npm test` uses `test/stub-ovrcr.mjs`, which records argv and returns canned
JSON; it never touches a real instance. `npm run test:live` creates a temporary
`OVRCR_HOME`, starts a real `ovrcr server`, asserts an empty inventory, and shuts
it down.

Rust coverage for the new path: `page_rows` unit tests in
`crates/ovrcr-terminal/src/history.rs` and the wire snapshot in
`crates/ovrcr-protocol/src/wire.rs`. A full session round trip is still manual:
this build sandbox has no PTY devices.

## Browser check

Screenshots use Chrome DevTools device emulation at 390×844 with a 2× scale.
`--window-size` cannot reach phone widths headlessly on macOS; use
`Emulation.setDeviceMetricsOverride` or a 390px iframe.

| Inbox | Session |
| --- | --- |
| ![Inbox](docs/inbox.png) | ![Session](docs/session.png) |

| Transcript | Scrollback |
| --- | --- |
| ![Transcript](docs/chat.png) | ![Scrollback](docs/history.png) |

## Files

- `server.mjs` — loopback HTTP server, CLI runner, routes, validation.
- `public/` — `index.html`, `styles.css`, `app.js`, `fonts/`; no external assets.
- `test/stub-ovrcr.mjs` — argv-recording CLI stub.
- `test/server.test.mjs` — the 13 tests.
- `test/live-instance.mjs` — real server, empty instance.
