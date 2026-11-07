// Loopback web bridge for OVRCR. Serves a phone-sized browser UI and shells
// out to the installed `ovrcr` CLI. No dependencies, no auth, 127.0.0.1 only.
//
// This is the slice-0 prototype from plans/2026-11-06-mobile-remote-web.md:
// prove the browser surface before any protocol or Tailscale work.

import http from "node:http";
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PUBLIC_DIR = path.join(HERE, "public");

const MAX_BODY_BYTES = 64 * 1024;
const MAX_TEXT_CHARS = 32_000;
const DEFAULT_SCREEN_ROWS = 40;
const MAX_SCREEN_ROWS = 128;
const DEFAULT_SCREEN_COLS = 200;
const MAX_SCREEN_COLS = 256;
const CLI_TIMEOUT_MS = 20_000;
const KILL_TIMEOUT_MS = 25_000;

const LOOPBACK_HOSTS = new Set(["127.0.0.1", "::1", "localhost"]);
const LOOPBACK_HOST = /^(127\.0\.0\.1|localhost|\[::1\])(?::\d+)?$/i;
const SESSION_ID = /^[1-9][0-9]{0,19}$/;
const KEY = /^:[a-z0-9]+(?:-[a-z0-9]+)*:$/;
const U64_MAX = 2n ** 64n - 1n;

const STATIC_FILES = new Map([
  ["/", "index.html"],
  ["/index.html", "index.html"],
  ["/app.js", "app.js"],
  ["/styles.css", "styles.css"],
  ["/fonts/jetbrains-mono-nerd-regular.woff2", "fonts/jetbrains-mono-nerd-regular.woff2"],
  ["/fonts/jetbrains-mono-nerd-bold.woff2", "fonts/jetbrains-mono-nerd-bold.woff2"],
  ["/fonts/jetbrains-mono-nerd-italic.woff2", "fonts/jetbrains-mono-nerd-italic.woff2"],
  ["/fonts/jetbrains-mono-nerd-bold-italic.woff2", "fonts/jetbrains-mono-nerd-bold-italic.woff2"],
  ["/fonts/OFL.txt", "fonts/OFL.txt"],
]);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
};

const BASE_HEADERS = {
  "Cache-Control": "no-store",
  "X-Content-Type-Options": "nosniff",
  "Referrer-Policy": "no-referrer",
  "Content-Security-Policy":
    "default-src 'self'; img-src 'self' data:; style-src 'self'; " +
    "script-src 'self'; connect-src 'self'; font-src 'self'; " +
    "base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
};

class HttpError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

class CliError extends Error {
  constructor({ code, message, status, stdout, stderr }) {
    super(message);
    this.code = code;
    this.status = status;
    this.stdout = stdout;
    this.stderr = stderr;
  }
}

function parseSessionId(raw) {
  if (!SESSION_ID.test(raw ?? "")) {
    throw new HttpError(400, "session id must be a positive integer");
  }
  if (BigInt(raw) > U64_MAX) {
    throw new HttpError(400, "session id is out of range");
  }
  return raw;
}

function parseBounded(raw, fallback, min, max, name) {
  if (raw === null || raw === "") return fallback;
  if (!/^[0-9]{1,6}$/.test(raw)) {
    throw new HttpError(400, `${name} must be an integer`);
  }
  const value = Number(raw);
  if (value < min || value > max) {
    throw new HttpError(400, `${name} must be between ${min} and ${max}`);
  }
  return value;
}

function parseKey(raw) {
  if (typeof raw !== "string" || raw.length > 48 || !KEY.test(raw)) {
    throw new HttpError(400, "key must look like :escape:, :ctrl-c: or :down:");
  }
  return raw;
}

function parseText(raw) {
  if (typeof raw !== "string") {
    throw new HttpError(400, "text must be a string");
  }
  if (raw.length > MAX_TEXT_CHARS) {
    throw new HttpError(413, `text is longer than ${MAX_TEXT_CHARS} characters`);
  }
  return raw;
}

function cliStatus(code) {
  switch (code) {
    case "NotFound":
      return 404;
    case "Conflict":
      return 409;
    case "InvalidRequest":
      return 400;
    case "Internal":
      return 502;
    default:
      return 502;
  }
}

// The CLI writes {"error":{code,message}} to stderr in --json mode.
function parseCliFailure(stderr) {
  const text = String(stderr ?? "").trim();
  if (!text) return null;
  try {
    const parsed = JSON.parse(text);
    if (parsed && typeof parsed === "object" && parsed.error) {
      return {
        code: String(parsed.error.code ?? "Internal"),
        message: String(parsed.error.message ?? "command failed"),
      };
    }
  } catch {
    // A non-JSON failure (missing binary, crash) falls through to the text.
  }
  return { code: "Internal", message: text.split("\n")[0].slice(0, 400) };
}

function runCli(bin, args, timeoutMs, env) {
  return new Promise((resolve, reject) => {
    execFile(
      bin,
      args,
      { timeout: timeoutMs, maxBuffer: 8 * 1024 * 1024, encoding: "utf8", env },
      (error, stdout, stderr) => {
        if (!error) {
          resolve(stdout);
          return;
        }
        if (error.code === "ENOENT") {
          reject(
            new CliError({
              code: "Unavailable",
              status: 503,
              message: `ovrcr CLI not found (${bin}); set OVRCR_BIN to its path`,
              stdout,
              stderr,
            }),
          );
          return;
        }
        if (error.killed) {
          reject(
            new CliError({
              code: "Timeout",
              status: 504,
              message: `ovrcr did not answer within ${timeoutMs}ms`,
              stdout,
              stderr,
            }),
          );
          return;
        }
        const failure = parseCliFailure(stderr) ?? {
          code: "Internal",
          message: String(error.message),
        };
        const status =
          failure.message.includes("server is not running") ? 503 : cliStatus(failure.code);
        reject(
          new CliError({
            ...failure,
            status,
            stdout,
            stderr,
          }),
        );
      },
    );
  });
}

async function readJsonBody(request) {
  let size = 0;
  const chunks = [];
  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_BODY_BYTES) {
      throw new HttpError(413, `request body is larger than ${MAX_BODY_BYTES} bytes`);
    }
    chunks.push(chunk);
  }
  if (size === 0) return {};
  try {
    const parsed = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new HttpError(400, "request body must be a JSON object");
    }
    return parsed;
  } catch (error) {
    if (error instanceof HttpError) throw error;
    throw new HttpError(400, "request body is not valid JSON");
  }
}

function legacyWindow(id, screen) {
  const lines = String(screen.text ?? "").split("\n");
  while (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  const rows = lines.map((line) => ({
    width: line.length,
    cells: Array.from(line, (character) => ({
      text: character,
      width: 1,
      fg: "Default",
      bg: "Default",
      attributes: 0,
    })),
    wrapped: false,
  }));
  return {
    session: Number(screen.id ?? id),
    run: null,
    size: { rows: screen.rows, cols: screen.cols },
    history_rows: 0,
    total_rows: rows.length,
    start_row: 0,
    alternate_screen: false,
    cursor: null,
    rows,
  };
}

function originAllowed(request, host) {
  const origin = request.headers.origin;
  if (!origin) return true;
  return origin === `https://${host}` || origin === `http://${host}`;
}

// A Tailscale Serve request carries the caller's login from WhoIs and a ts.net
// Host. Loopback requests are the person at the machine and bypass the login
// check; everything else must name the configured login.
function authorized(request, remoteLogin) {
  const host = String(request.headers.host || "");
  const local = LOOPBACK_HOST.test(host);
  if (!local) {
    if (!remoteLogin) return false;
    const login = String(request.headers["tailscale-user-login"] || "").toLowerCase();
    if (login !== remoteLogin) return false;
  }
  if (request.method !== "GET" && request.method !== "HEAD" && !originAllowed(request, host)) {
    return false;
  }
  return true;
}

function sendJson(response, status, value) {
  const body = JSON.stringify(value);
  response.writeHead(status, {
    ...BASE_HEADERS,
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(body),
  });
  response.end(body);
}

function sendError(response, status, message) {
  sendJson(response, status, { error: { message } });
}

async function sendStatic(response, file) {
  const absolute = path.join(PUBLIC_DIR, file);
  if (!absolute.startsWith(PUBLIC_DIR + path.sep)) {
    sendError(response, 404, "not found");
    return 404;
  }
  let body;
  try {
    body = await readFile(absolute);
  } catch {
    sendError(response, 404, "not found");
    return 404;
  }
  const cache = file.endsWith(".woff2") ? "public, max-age=86400, immutable" : "no-store";
  response.writeHead(200, {
    ...BASE_HEADERS,
    "Cache-Control": cache,
    "Content-Type": MIME[path.extname(file)] ?? "application/octet-stream",
    "Content-Length": body.length,
  });
  response.end(body);
  return 200;
}

export function createApp({
  bin = process.env.OVRCR_BIN || "ovrcr",
  ttlMs = Number(process.env.LIST_TTL_MS || 5000),
  log = console.log,
  cli = runCli,
  env = process.env,
  remoteLogin = process.env.OVRCR_REMOTE_LOGIN || "",
} = {}) {
  const login = remoteLogin.trim().toLowerCase();
  let cache = { at: 0, sessions: null };
  let inflight = null;

  async function listSessions() {
    const now = Date.now();
    if (cache.sessions && now - cache.at < ttlMs) {
      return cache.sessions;
    }
    if (inflight) return inflight;
    inflight = (async () => {
      try {
        const stdout = await cli(bin, ["--json", "terminal", "list"], CLI_TIMEOUT_MS, env);
        const parsed = JSON.parse(stdout || "[]");
        if (!Array.isArray(parsed)) {
          throw new CliError({
            code: "Internal",
            status: 502,
            message: "terminal list did not return an array",
          });
        }
        cache = { at: Date.now(), sessions: parsed };
        return parsed;
      } finally {
        inflight = null;
      }
    })();
    return inflight;
  }

  async function handleApi(request, response, url, parts) {
    if (parts[1] === "state" && parts.length === 2) {
      if (request.method !== "GET") {
        response.setHeader("Allow", "GET");
        throw new HttpError(405, "use GET");
      }
      const sessions = await listSessions();
      sendJson(response, 200, { sessions, fetched_at: new Date().toISOString() });
      return;
    }

    if (parts[1] !== "session" || parts.length < 3) {
      throw new HttpError(404, "not found");
    }

    const id = parseSessionId(parts[2]);
    const action = parts[3];

    if (action === "screen" && parts.length === 4) {
      if (request.method !== "GET") {
        response.setHeader("Allow", "GET");
        throw new HttpError(405, "use GET");
      }
      const startRow = parseBounded(url.searchParams.get("start_row"), 0, 0, 1_000_000, "start_row");
      const rows = parseBounded(
        url.searchParams.get("rows"),
        DEFAULT_SCREEN_ROWS,
        1,
        MAX_SCREEN_ROWS,
        "rows",
      );
      const cols = parseBounded(
        url.searchParams.get("cols"),
        DEFAULT_SCREEN_COLS,
        1,
        MAX_SCREEN_COLS,
        "cols",
      );
      try {
        const stdout = await cli(
          bin,
          [
            "--json",
            "terminal",
            "screen",
            id,
            `--start-row=${startRow}`,
            `--rows=${rows}`,
            `--cols=${cols}`,
          ],
          CLI_TIMEOUT_MS,
          env,
        );
        sendJson(response, 200, JSON.parse(stdout));
      } catch (error) {
        // An older CLI has no `terminal screen`; keep the plain-text view working
        // until the CLI and Server are rebuilt together.
        if (!/unrecognized subcommand|unexpected argument/i.test(error.message)) throw error;
        const legacy = await cli(
          bin,
          ["--json", "terminal", "read", id, `--max-lines=${Math.min(cols, 200)}`],
          CLI_TIMEOUT_MS,
          env,
        );
        sendJson(response, 200, legacyWindow(id, JSON.parse(legacy)));
      }
      return;
    }

    if (request.method !== "POST") {
      response.setHeader("Allow", "POST");
      throw new HttpError(405, "use POST");
    }

    const body = await readJsonBody(request);

    switch (action) {
      case "send": {
        const text = parseText(body.text);
        const submit = body.submit !== false;
        const args = ["--json", "terminal", "send", id, `--text=${text}`];
        if (!submit) args.push("--no-submit");
        await cli(bin, args, CLI_TIMEOUT_MS, env);
        sendJson(response, 200, { ok: true });
        return;
      }
      case "key": {
        const key = parseKey(body.key);
        await cli(bin, ["--json", "terminal", "keystroke", id, key], CLI_TIMEOUT_MS, env);
        sendJson(response, 200, { ok: true });
        return;
      }
      case "transcript": {
        const args = ["--json", "terminal", "transcript", id];
        if (body.before !== undefined && body.before !== null) {
          if (!Number.isInteger(body.before) || body.before < 0) {
            throw new HttpError(400, "before must be a non-negative integer");
          }
          args.push(`--before=${body.before}`);
        }
        if (body.identity !== undefined && body.identity !== null) {
          if (typeof body.identity !== "object" || Array.isArray(body.identity)) {
            throw new HttpError(400, "identity must be the object from a previous page");
          }
          args.push(`--identity=${JSON.stringify(body.identity)}`);
        }
        const stdout = await cli(bin, args, CLI_TIMEOUT_MS, env);
        sendJson(response, 200, JSON.parse(stdout));
        return;
      }
      case "review": {
        const expected = body.expected;
        if (expected === null || typeof expected !== "object" || Array.isArray(expected)) {
          throw new HttpError(400, "expected must be the unread object from the session list");
        }
        await cli(
          bin,
          ["--json", "terminal", "mark-reviewed", id, `--expected=${JSON.stringify(expected)}`],
          CLI_TIMEOUT_MS,
          env,
        );
        sendJson(response, 200, { ok: true });
        return;
      }
      case "pause":
        await cli(bin, ["--json", "pause", id], CLI_TIMEOUT_MS, env);
        sendJson(response, 200, { ok: true });
        return;
      case "resume":
        await cli(bin, ["--json", "resume", id], CLI_TIMEOUT_MS, env);
        sendJson(response, 200, { ok: true });
        return;
      case "kill":
        await cli(bin, ["--json", "kill", id], KILL_TIMEOUT_MS, env);
        sendJson(response, 200, { ok: true });
        return;
      default:
        throw new HttpError(404, "not found");
    }
  }

  const server = http.createServer((request, response) => {
    const started = Date.now();
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    const parts = url.pathname.split("/").filter(Boolean);

    const finish = (status) => {
      log(`${request.method} ${url.pathname} ${status} ${Date.now() - started}ms`);
    };

    if (!authorized(request, login)) {
      sendError(
        response,
        403,
        login
          ? "this tailnet request is not from the configured OVRCR login"
          : "set OVRCR_REMOTE_LOGIN to accept tailnet requests",
      );
      finish(403);
      return;
    }

    Promise.resolve()
      .then(async () => {
        if (parts[0] === "api") {
          await handleApi(request, response, url, parts);
          finish(response.statusCode);
          return;
        }
        if (parts.length === 0) {
          finish(await sendStatic(response, "index.html"));
          return;
        }
        const file = STATIC_FILES.get(`/${parts.join("/")}`);
        if (!file) {
          sendError(response, 404, "not found");
          finish(404);
          return;
        }
        finish(await sendStatic(response, file));
      })
      .catch((error) => {
        const status = error instanceof HttpError ? error.status : error.status ?? 502;
        const message =
          error instanceof HttpError || error instanceof CliError
            ? error.message
            : "internal error";
        if (!response.headersSent) {
          sendError(response, status, message);
        } else {
          response.end();
        }
        finish(status);
        if (!(error instanceof HttpError) && !(error instanceof CliError)) {
          log(`error: ${error?.stack ?? error}`);
        }
      });
  });

  return {
    server,
    listSessions,
    listen(port, host = "127.0.0.1") {
      return new Promise((resolve, reject) => {
        server.once("error", reject);
        server.listen(port, host, () => {
          server.off("error", reject);
          resolve(server.address());
        });
      });
    },
    close() {
      return new Promise((resolve) => server.close(resolve));
    },
  };
}

async function main() {
  const host = process.env.HOST || "127.0.0.1";
  if (!LOOPBACK_HOSTS.has(host) && process.env.ALLOW_REMOTE !== "1") {
    console.error(
      `refusing to listen on ${host}: this MVP has no authentication and is loopback-only.\n` +
        "Set ALLOW_REMOTE=1 only on a network you trust.",
    );
    process.exit(1);
  }

  const port = Number(process.env.PORT || 8787);
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    console.error(`invalid PORT: ${process.env.PORT}`);
    process.exit(1);
  }

  const app = createApp();
  const address = await app.listen(port, host);
  const shown = host.includes(":") ? `[${host}]` : host;
  console.log(`ovrcr remote web (mvp) listening on http://${shown}:${address.port}`);
  console.log(`  cli: ${process.env.OVRCR_BIN || "ovrcr"}`);
  const login = (process.env.OVRCR_REMOTE_LOGIN || "").trim();
  console.log(`  tailnet login: ${login || "unset (loopback only)"}`);
  if (!login) console.log("  set OVRCR_REMOTE_LOGIN to accept Tailscale Serve requests");

  const stop = () => {
    app.close().then(() => process.exit(0));
  };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
