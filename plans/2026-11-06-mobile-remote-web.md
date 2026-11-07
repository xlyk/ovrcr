# Mobile remote for OVRCR

Date: 2026-11-06
Status: proposal, not approved. Slice 0 is implemented in
[`experiments/remote-web`](../experiments/remote-web/) — loopback only, no
Tailscale, no authentication.

## Problem

OVRCR's only surface is one Dashboard TUI, attached over a Unix socket, on the
machine that owns the PTYs. Leave the desk and you lose every session: which
agent is waiting for an answer, which finished, which is stuck. The sessions keep
running — that is the point of `q` detaching — but nothing can reach them.

A phone does not need the dashboard. In order of value, it needs to know which
session needs a human, see enough text to decide, and send the smallest possible
answer. Full terminal fidelity is a later problem, and possibly the wrong one.

Tailscale is the right connectivity layer: the Mac and the phone are already on
one tailnet, Tailscale issues the HTTPS certificate, and `tailscale serve` can
tell the backend who is calling. What it cannot do is reach a Unix socket or
authenticate an OVRCR client.

## What the code gives us today

### Transport and roles

- One `UnixListener` at `server.sock`, mode `0700`, no TCP path anywhere
  (`crates/ovrcr-runtime/src/server/startup.rs:191-193`).
- Every connection starts as `ClientRole::Control`; `Request::DashboardHello`
  promotes it (`crates/ovrcr-runtime/src/server/connections.rs:114,187-273`).
- A Control connection carries **one request**, then closes
  (`connections.rs:411-413`). The only long-lived Control frame loop is
  `Request::Events { follow: true }` (`connections.rs:274-282`).
- A second Dashboard is refused with `Conflict` while the first is alive
  (`crates/ovrcr-runtime/src/server/dashboard.rs:36-58`).
- Protocol is `PROTOCOL_VERSION = 71`, 8-byte preamble, then u32-BE length +
  bincode frames (`crates/ovrcr-protocol/src/codec.rs:5-150`). A JavaScript
  client would have to reimplement bincode and every enum index; there is no
  JSON or cell protocol.

### Capability matrix (verified against the handlers)

| Phone need | Control | Dashboard | Wire |
| --- | --- | --- | --- |
| Session list, status, unread | yes | yes | `Request::List` → `Response::Hierarchy` |
| Current screen as text | yes | yes | `Request::ReadTerminal` → `Response::TerminalText` |
| Live screen (ANSI) | **no** | seat only | `Request::SetView` → `Response::Screen` + `ServerEvent::Output` |
| Send prose | yes | yes | `Request::SendTerminal` → `Ok` |
| Named keys (`:escape:`, `:ctrl-c:`, `:down:`) | yes, unless desktop focused | yes | `Request::Keystroke` → `Ok` |
| Raw bytes | no | focused pane | `Request::Input` |
| Pause / resume / kill | yes | yes | `PauseSession`, `ResumeSession`, `KillSession` |
| Mark reviewed | yes | yes | `MarkReviewed { expected }` |
| Transcript | **no** | seat only | `Request::Transcript` (protocol 71) |
| History cells | no | seat only | `HistoryBegin/Page/End` |
| Create session | yes | yes | `CreateSession` / `CreateWorkspaceWithLaunch` |
| Event ring, follow | yes | yes | `Events { follow }` |

Three asymmetries matter:

1. `SendTerminal` has no `run` fence and no focus rule
   (`crates/ovrcr-runtime/src/server/mod.rs:553-562`), while `Keystroke` refuses
   when the desktop Dashboard is focused on that session (`mod.rs:566-586`).
   Prose can currently land in a session the desktop owns the keyboard for;
   keys cannot.
2. `Transcript` and live screen are dashboard-only (`connections.rs:1193-1210`,
   `:1167-1191`). A Control client cannot see either.
3. `InputRequest` is deliberately content-free — `{ id, kind }` only
   (`crates/ovrcr-protocol/src/agent.rs:161-171`). The phone can learn that
   Claude wants approval, but not what for. The question text exists only in the
   agent's own TUI.

### Live screen path

PTY bytes → per-session `vt100::Parser` → raw bytes forwarded as
`ServerEvent::Output` **only** for sessions in the published view
(`crates/ovrcr-runtime/src/server/dispatch.rs:504-524`) → Dashboard queue
(cap 64, output coalesced into `ScreenDirty` under pressure,
`crates/ovrcr-runtime/src/server/outbound.rs:15-27,205-232`). The initial paint
is `Response::Screen`, which is `current_screen()` = `vt100 State::state_formatted()`
— a full ANSI repaint, not cells (`crates/ovrcr-runtime/src/session/mod.rs:989-993`).

So a browser that receives the seat's stream must run a real terminal emulator
(xterm.js), not a diff renderer. There is no cell protocol for the live screen.

### Input authority

`Request::Input` requires: Dashboard role, owned seat, `view.focused == session`,
matching pane `run`, not assigned to a child, live session
(`connections.rs:839-883`). No revision fence, no idempotency key; a retried
write is written twice. `Response::Ok` means bytes reached the PTY writer, not
that the program consumed them.

## Options

### Option 0 — no build: Tailscale SSH + a mobile terminal

`tailscale ssh user@mac` from Blink/Termius, run `ovrcr`. Zero code, full
fidelity, bad ergonomics: 50 columns on a phone, no attention inbox, no push,
and the TUI's keyboard model assumes a hardware keyboard. This is the baseline
that proves or kills demand before any Rust is written.

### Option A — control-plane companion (`ovrcr remote`)

A separate loopback HTTP gateway that connects as `ClientRole::Control`. It
never claims the Dashboard seat, so it coexists with the desktop TUI.

- Inbox from `List` (poll 10-15 s, force on `Events { follow }`).
- Screen from `ReadTerminal` (plain text, no color, no scrollback), polled 2 s
  only for the open session.
- Writes via `SendTerminal` and `Keystroke`; `MarkReviewed`; pause/resume/kill;
  new shell session.
- No transcript, no live ANSI, no history.

Strengths: no Server change at all, no seat conflict, no PTY resize, small
attack surface, testable against the real socket. Weaknesses: the screen is
text-only and 120 columns wide; `List` runs one or two `git rev-parse`
subprocesses per workspace on every call
(`crates/ovrcr-runtime/src/server/mod.rs:410-413,2647-2668`), so polling 50
sessions is not free; and "what is it asking" is a guess from screen text.

### Option A′ — CLI-shelling prototype (recommended first)

The same product as A, with the Rust bridge replaced by the installed CLI:

| Action | Command |
| --- | --- |
| Inbox | `ovrcr terminal list --json` |
| Screen | `ovrcr terminal read <id> --json --max-lines 40` |
| Prose | `ovrcr terminal send <id> --text <text>` (`--no-submit` to stage) |
| Key | `ovrcr terminal keystroke <id> :escape:` |
| Review | `ovrcr terminal mark-reviewed <id> --expected '<unread json>'` |
| Pause / resume / kill | `ovrcr pause <id>` / `resume <id>` / `kill <id>` |

Why it is genuinely a better first step than A:

- `terminal list --json` already carries the inbox: `id`, `run`, `project`,
  `workspace`, `title`, `phase`, `activity` (including `waiting_input`),
  `unread`, `agent` (including `input_requests`), `context_usage`
  (`src/cli/output.rs:70-130`).
- `terminal read --json` returns `{id, rows, cols, text}` — the visible grid,
  already trimmed to `--max-lines`.
- The web app can be written in any language and thrown away. No Rust crate, no
  bincode, no protocol version, no Server change.
- It runs beside the desktop: `send` has no seat check, and `keystroke` is
  refused only when the desktop is focused on that session.

Where it costs more than A:

- Every poll spawns a process and opens a socket. `terminal list` also runs
  `git rev-parse` once or twice per workspace
  (`crates/ovrcr-runtime/src/server/mod.rs:410-413`,
  `crates/ovrcr-runtime/src/git.rs:30-48`), so cache it for 5-10 s. `terminal
  read` does no Git or registry walk and can poll at 1-2 s for the open session
  alone.
- `terminal read --json` has no `run` field, so a stale screen is detectable
  only through the list's `run`/`phase`.
- `send` has no run fence. Refresh the list before sending, and never retry a
  failed send automatically — a duplicate prompt is worse than a lost one.
- Errors are exit codes plus stderr, not typed responses.

Security is unchanged: the CLI has no auth, so the Tailscale, pairing, and
read-only-default work still applies. Spawn with an argv array (never a shell
string), parse session ids as `u64`, and pass key names through untouched.

This is not the architecture; it is the fastest way to test whether the inbox
is the product. The HTTP shape it settles on is the shape slice 1 implements,
so the PWA survives the backend swap.

### Option B — live-terminal gateway that takes the seat

The gateway sends `DashboardHello` and holds the one seat, streams
`Response::Screen` + `Output` to xterm.js, and sends keystrokes through
`Request::Input`.

- Fidelity is real: colors, cursor, alt screen, wide glyphs.
- But `SetView` resizes the PTY to the phone's pane
  (`crates/ovrcr-runtime/src/server/dispatch.rs:574-699`), so an attached phone
  reflows the desktop's agents; and the desktop is refused while the phone holds
  the seat. Handoff is manual: `q` on the desktop, then the phone connects.
- Interactive TUI on a 390 px screen needs a modifier bar anyway; 80 columns do
  not fit (about 50 at 13 px mono, estimate).

Strengths: zero Server changes, full fidelity, the right answer for phone-only
use when the desktop is detached. Weaknesses: cannot coexist with the desktop,
fights over PTY geometry, and the phone becomes a full terminal-emulation
client — the most expensive client to get right.

### Option C — observer seat plus Control writes (recommended v2)

Add a read-only observer client role to the Server. An observer subscribes to a
session (or a bounded set), receives `Response::Screen` on subscribe and
`ServerEvent::Output` thereafter, through its own bounded queue. It cannot
resize, focus, or write. Input stays on Control (`SendTerminal`, `Keystroke`),
which already has the "desktop owns the keyboard" rule.

- Preserves one server owner, one dashboard seat, one PTY geometry.
- The per-session parser already produces exactly the bytes needed; this is a
  second consumer of the same output, not a second parser or a second owner.
- Needs a contract change (new role + subscription request + queue policy), so
  an ADR is required before the PR. It does not relax the one-dashboard rule.
- `Transcript` can be allowed for an observer with the same read-only reasoning.

This is the only option that gives a live screen on the phone while the desktop
is still attached, without a takeover.

### Comparison

| | 0 SSH | A′ CLI | A Control | B Seat | C Observer |
| --- | --- | --- | --- | --- | --- |
| Server change | none | none | none | none | role + subscription + ADR |
| Coexists with desktop | n/a | yes | yes | no | yes |
| Live ANSI | yes | no | no | yes | yes |
| Transcript | yes (in TUI) | no | no | yes | yes (with change) |
| Phone fits the layout | no | n/a | n/a | poor | n/a (render, not lay out) |
| PTY resize by phone | no | no | no | yes | no |
| Poll cost | — | process + git per list | process per request | stream | stream |
| Build effort | 0 | ~1-2 days | ~6-8 days | ~8-10 days | ~10-13 days |

## Recommendation

1. **Option A′ is built** (slice 0): `experiments/remote-web`, loopback only.
   Use it before adding transport; it needs no Rust and no Tailscale.
2. **If it survives daily use, replace the CLI calls with Option A** — the
   protocol-direct gateway — keeping the same HTTP/SSE contract and PWA. Poll
   cost, the missing run fence, and exit-code errors are the reasons to move.
3. **Then add Option C's observer seat** for live screen and transcript, because
   the desktop stays attached in real use. Option B is a fallback for a
   phone-only mode, not the main path.
4. **Never** expose the socket or the protocol directly over TCP, and never
   `tailscale funnel` this.

The first slice should prove one thing: **the attention inbox is the product,
and the terminal is a detail view.** If the inbox works, the rest is
incremental.

## Architecture

```
iPhone / Android browser (PWA)
   │  HTTPS, tailnet only
   ▼
tailscaled ── tailscale serve ──► 127.0.0.1:8787   [ovrcr remote]
                                         │ AF_UNIX, preamble v71
                                         ▼
                                   server.sock     [the one Server]
                                         │ PTYs
                                         ▼
                                   50 sessions
```

Ownership stays honest:

- The **Server** still owns the socket, sessions, PTYs, and SQLite. Unchanged.
- The **gateway** owns HTTP clients, device tokens, and at most one or two
  upstream connections. It owns no session, no PTY, and no durable session
  state. It must use `connect_if_running` (`src/client.rs:64`), never
  `connect_or_start`, so a dead Server is a 503, not a second Server.
- The **phone** owns nothing durable. Cookies only.

### HTTP + SSE, not WebSocket

Use `GET /api/events` as `text/event-stream` and POST for commands.

- Works through any reverse proxy, including `tailscale serve`, with no upgrade
  negotiation. WebSocket upgrade through Serve is undocumented.
- `Last-Event-ID` gives reconnect-and-resume for free, which is exactly the
  mobile failure mode (sleep, roam, cellular handoff). Replay is bounded by a
  sequence number; the client re-reads state when it detects a gap.
- One-way fits: screen/output flow down, input is request/response up.
- No async runtime change and no WebSocket crate. A small synchronous HTTP
  server is enough for one user and two devices. Candidate: `tiny_http`
  (sync, MIT, small), behind an optional `remote` feature like the existing
  `gui` feature (`Cargo.toml`). Hand-rolling HTTP is possible but not worth the
  parser risk; the dependency needs the usual "concrete requirement" note in the
  PR.

If a future version needs bidirectional streaming, add WebSocket then, behind
its own decision — do not start there.

### API sketch

```
POST /api/pair                   {code, label}        -> sets HttpOnly cookie
GET  /api/state                                       -> sessions, device, capabilities
GET  /api/session/:id/screen                          -> {text, size, run}
GET  /api/events?last=<seq>                           -> text/event-stream
POST /api/session/:id/send       {text, submit}
POST /api/session/:id/key        {key: ":escape:"}
POST /api/session/:id/review     {expected}
POST /api/session/:id/pause      {}
POST /api/unlock                 {}                   -> 5-minute input lease
```

Every response carries the session `run`; a mutation whose `run` no longer
matches is refused, so a reconnect cannot write into a replacement run. The
JSON schema is the gateway's, not the wire protocol's — the gateway owns the
translation and the protocol version stays private.

### Gateway placement

A separate executable or subcommand (`ovrcr remote serve`) started by the user.
Not a child of the Server: the Server is synchronous and has no supervisor, and
a gateway crash must not touch PTYs. For persistence, reuse the existing
launchd/systemd installer in `src/service.rs` rather than inventing a second
supervision model.

## Security

Slice 0 deliberately has none of the controls below: it binds loopback, has no
auth, and refuses any other host. They are the entry price for the first
non-loopback listener, including a tailnet one.

Remote terminal input is remote code execution as the logged-in user. Managed
Claude launches add `--dangerously-skip-permissions` by default
(`crates/ovrcr-protocol/src/auto_trust.rs:13-15`), so a misdirected keystroke
does not even hit an approval prompt. Every control below is load-bearing.

### Threat model

| Threat | Impact | Control |
| --- | --- | --- |
| Lost or unlocked phone | critical | 5-minute input lease, server-side idle, one-tap revoke |
| Token in URL, history, `Referer`, logs | critical | pairing code in the URL **fragment**; HttpOnly cookie; hash-only storage; never log headers |
| CSRF / cross-origin POST from a page the phone visits | critical | exact `Origin` check, `SameSite=Strict`, custom header on mutations |
| Duplicate input from two tabs or a reconnect | high | one lease per device; monotonic input sequence; no replay queue |
| Shared tailnet / another user's device | high | ACL grant to one login **and** an app-level device token |
| Compromised tailnet device | high | per-device token with generation bump on revoke |
| `tailscale funnel` accidentally on | critical | refuse to start when the backend is reachable off-loopback; document Funnel as unsupported |

### Tailscale shape

- `tailscale serve --bg --https=443 http://127.0.0.1:8787`. Tailscaled owns the
  certificate and renewal; the tailnet is the only reachable network.
- Serve injects `Tailscale-User-Login`, `Tailscale-User-Name`, and
  `Tailscale-User-Profile-Pic`, derived from `WhoIs`. It strips client-supplied
  copies — **but only for traffic that actually arrived through Serve**. Bind the
  gateway to `127.0.0.1` and reject any request whose `Host` is not the MagicDNS
  name. ([serve](https://tailscale.com/docs/features/tailscale-serve),
  [header semantics](https://pypi.org/project/datasette-auth-tailscale/0.0.1a1/))
- Funnel does **not** add identity headers, and publishes to the internet. Not
  supported. ([funnel](https://tailscale.com/docs/features/tailscale-funnel))
- Keep the Mac's node user-owned. Tagged nodes have no user identity, so the
  header may be absent; treat that as a configuration error rather than falling
  back to "allow".
- ACL: grant only the owner's login to the host on `tcp:443`. Device approval is
  a cheap second lock; check its interaction with Tailnet Lock before enabling
  both.

### Auth sequence

1. `ovrcr remote pair` mints a 120-second single-use code, prints
   `https://<magicdns>/#<code>` and a QR. The fragment never reaches an HTTP
   request.
2. The page POSTs the code with a device label; the gateway exchanges it for a
   32-byte token and sets `ovrcr_device` as `HttpOnly; Secure; SameSite=Strict`.
   Only `SHA-256(token)` is stored, in a `0600` file or the existing SQLite
   authority, never in `events.jsonl` or `server.log`.
3. Read-only by default. `POST /api/unlock` requires the cookie, a custom
   `X-OVRCR-Unlock` header, and the owner login; it returns a 5-minute lease
   bound to `(device, session, run)`. Every mutation re-checks it.
4. `ovrcr remote revoke <device>` bumps the generation and closes that device's
   stream. Removing the phone from the tailnet is the independent kill switch.

No credentials in code, logs, events, or captures (AGENTS.md). Device label and
device id may be logged; login identity may not.

## Mobile UX

### Screen 1 — the queue

```
OVRCR                         ● tailnet
NEEDS YOU  2
  ? api-refactor      input  2m   claude
  ✓ docs-site         reply  8m   codex
WORKING  3
  ● billing-fix       busy  14m   codex
IDLE  1
  ○ scratch           idle  1h    shell
```

Ordering copies the dashboard's attention rule: `WaitingInput` first, then
`Unread`, then busy, then idle. One row per session, no tree; grouping by
project only if the flat list gets too long.

### Screen 2 — the session

Header: project / branch, title, agent + model, status words, elapsed.
Body: the terminal text (monospace, horizontally scrollable) or, once the
observer seat exists, the live screen. Footer: a key bar and four actions
(Reply, Keys, Pause, Stop). While the desktop holds the keyboard, show a banner
instead of silently refusing keys.

### Screen 3 — reply

A text field, a send button, and a sticky key row:

```
Esc   Tab   ⇧Tab   ↑ ↓ ← →   Enter   ^C   1 2 3  y n
```

The key row maps one-to-one onto `parse_keystroke` vocabulary
(`:escape:`, `:tab:`, `:backtab:`, `:up:`, `:ctrl-c:`, …, 
`crates/ovrcr-terminal/src/key.rs:75-130`), which is already exposed as
`ovrcr terminal keystroke`. Modifiers latch for one key, not toggle.

Approvals are the honest weak spot: OVRCR knows an `InputRequest` is open but
not what it says. Start by showing the bottom of the screen plus the key bar —
"it is asking, here is the screen, press 1 or Esc". Do not pretend to parse
provider menus into semantic buttons until an answered request can be confirmed.

### Notifications

PWA web push only, and only as a nudge with a deep link
(`/s/<id>`): iOS requires Add to Home Screen, a gesture for permission, and a
visible notification per push. Always re-read state on open. The existing macOS
Bridge is a desktop concern; do not route phone notifications through it.

### Deliberately absent

Split panes, the sidebar tree, settings and theme editing, History/Copy,
workspace creation, and multi-user accounts. Each is a desktop job or a much
larger contract.

## Plan

### Slice 0 — CLI-shelling prototype (1-2 days) — implemented

Implemented in `experiments/remote-web`. It is loopback-only and has no
Tailscale or auth: the MVP proves the browser surface first, and the transport
and pairing gates move to slice 1.

Exit criteria (loopback): on the same machine, with the desktop Dashboard
attached, you can see the inbox, open a session, read its bottom 40 lines, send
a prompt into a shell and see it echoed, send `:escape:` and `:ctrl-c:`, and
mark a review.

Work:

- One small HTTP server in Node (no dependencies, no Rust). Bind `127.0.0.1`.
- `GET /api/state` shells `ovrcr terminal list --json` behind a 5-10 s cache.
- `GET /api/session/:id/screen` shells `ovrcr terminal read <id> --json
  --max-lines 40` at 1-2 s, open session only.
- `POST` routes spawn with argv arrays; ids parsed as `u64`; key names passed
  through unchanged; no retry on send.
- If the server runs under launchd/systemd, start the web app with the same
  `OVRCR_HOME`/`OVRCR_SOCKET` (or none, to use the default instance directory).
- Log method, path, status, duration — never the text.
- Expected outcome: throw the server away when slice 1 lands; keep the PWA.

The prototype's tests cover HTTP, caching, argv construction, and error mapping
with a stub CLI, plus the inventory path against a disposable real server. A
full PTY round trip remains manual on a machine with PTY devices.

### Slice 1 — control-plane companion (6-8 days)

Same HTTP contract and PWA as slice 0; only the backend changes, from CLI
processes to one Rust gateway that speaks the protocol.

Exit criteria: from a phone on the tailnet, with the desktop Dashboard attached,
you can list sessions with correct `WaitingInput`/`Unread`, open one, see the
current screen text, send prose into a shell and see it echoed, send `:ctrl-c:`
and `:escape:`, mark a session reviewed, and pause/resume/kill. Every mutation is
refused without a valid cookie and unlock lease.

Work:

- `src/remote/` module + `ovrcr remote serve|pair|revoke|status`, feature-gated.
- HTTP server (small sync dep), static PWA (`include_str!`), SSE stream.
- One upstream Control connection per request; a cached `Inspect` (flat
  sessions, the shape the CLI already returns) with a 10 s floor.
- Device store, pairing, unlock lease, origin/host checks.
- Tests: real Server fixture with isolated `OVRCR_HOME`, socket, and workspace;
  assert bytes reached the PTY, stale runs refused, desktop-focus refusal
  surfaced, spoofed headers rejected, no token in logs.
- Docs: `docs/remote.md`, CLI reference, README.

### Slice 2 — observer seat and live screen (5-7 days)

Exit criteria: the phone shows the live screen (colors, cursor, alt screen)
while the desktop Dashboard is attached, and never resizes a PTY.

Work: ADR for the new role; subscribe/unsubscribe requests; per-observer bounded
queue with `ScreenDirty` overflow; xterm.js (vendored, MIT) in the PWA; allow
read-only `Transcript` for an observer; integration tests for two simultaneous
consumers and queue overflow.

### Slice 3 — answer quality (3-5 days)

Transcript view on the phone, approval and question cards driven by the
provider's screen region, push notifications, offline cache, better reconnect.
Only after 1 and 2 are used daily.

### Test posture

Per AGENTS.md: real application path, isolated config/socket/workspaces, no
assertion weakening. The web tier adds unit tests for HTTP/auth/SSE and
integration tests through the actual `server.sock`. There is no browser CUA
harness in the repo; browser behavior is a stated gap until one exists.

## Open questions

1. **Simultaneous use or handoff?** If the desktop and phone must both work at
   once, slice 1 only gets you text, and slice 2 (observer seat) is required.
   If a handoff is acceptable, Option B is cheaper and needs no Server change.
2. **Read-only by default?** Recommended yes, with an explicit unlock. The
   alternative is "paired device may always type", which is simpler and worse.
3. **iPhone only or both?** Push and installability differ; the web surface
   should be the same.
4. **Where do remote settings live?** ADR 0007 says the Server owns the one
   settings document. Either the gateway reads `[remote]` through a Server
   request, or the remote config stays a separate `0600` file. Prefer one
   document, but that is a small protocol addition.
5. **Is "Remote gateway" the right domain term?** It is not a Server and not a
   Bridge; CONTEXT.md needs one word for it before code lands.

## Evidence map

- Roles and gating: `crates/ovrcr-runtime/src/server/connections.rs:114,187-273,411-413,839-883`
- Seat arbitration: `crates/ovrcr-runtime/src/server/dashboard.rs:36-94`
- Screen path: `crates/ovrcr-runtime/src/server/dispatch.rs:504-524,574-699`,
  `crates/ovrcr-runtime/src/server/outbound.rs:252-313`,
  `crates/ovrcr-runtime/src/session/mod.rs:989-1001`
- Control writes: `crates/ovrcr-runtime/src/server/mod.rs:530-586`
- Wire types: `crates/ovrcr-protocol/src/wire.rs:92-310,475-560`,
  `crates/ovrcr-protocol/src/codec.rs:5-150`
- Input request content-free: `crates/ovrcr-protocol/src/agent.rs:161-171`
- Auto-trust flags: `crates/ovrcr-protocol/src/auto_trust.rs:13-15`
- Key grammar: `crates/ovrcr-terminal/src/key.rs:75-130`
- CLI JSON the prototype shells: `src/cli/output.rs:70-130` (`terminal_value`),
  `src/cli/args.rs:355-395` (read/send/keystroke), `src/cli/mod.rs:129-141`
  (`list`), `src/cli/mod.rs:377-397` (`inspect`, with offline fallback)
- Service installer to reuse: `src/service.rs:146-190`
- Tailscale Serve identity headers: <https://tailscale.com/docs/features/tailscale-serve>,
  <https://pypi.org/project/datasette-auth-tailscale/0.0.1a1/>
- Funnel has no identity headers: <https://tailscale.com/docs/features/tailscale-funnel>
- iOS web push constraints: <https://webkit.org/blog/13878/web-push-for-web-apps-on-ios-and-ipados/>
