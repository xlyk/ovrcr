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
const DEFAULT_SCREEN_LINES = 40;
const MAX_SCREEN_LINES = 200;
const CLI_TIMEOUT_MS = 20_000;
const KILL_TIMEOUT_MS = 25_000;

const LOOPBACK_HOSTS = new Set(["127.0.0.1", "::1", "localhost"]);
const SESSION_ID = /^[1-9][0-9]{0,19}$/;
const KEY = /^:[a-z0-9]+(?:-[a-z0-9]+)*:$/;
const U64_MAX = 2n ** 64n - 1n;

const STATIC_FILES = new Map([
  ["/", "index.html"],
  ["/index.html", "index.html"],
  ["/app.js", "app.js"],
  ["/styles.css", "styles.css"],
]);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
};

const BASE_HEADERS = {
  "Cache-Control": "no-store",
  "X-Content-Type-Options": "nosniff",
  "Referrer-Policy": "no-referrer",
  "Content-Security-Policy":
    "default-src 'self'; img-src 'self' data:; style-src 'self'; " +
    "script-src 'self'; connect-src 'self'; base-uri 'none'; " +
    "form-action 'none'; frame-ancestors 'none'",
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

function parseLines(raw) {
  if (raw === null || raw === "") return DEFAULT_SCREEN_LINES;
  if (!/^[0-9]{1,3}$/.test(raw)) {
    throw new HttpError(400, "lines must be a positive integer");
  }
  const lines = Number(raw);
  if (lines < 1 || lines > MAX_SCREEN_LINES) {
    throw new HttpError(400, `lines must be between 1 and ${MAX_SCREEN_LINES}`);
  }
  return lines;
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
  response.writeHead(200, {
    ...BASE_HEADERS,
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
} = {}) {
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
      const lines = parseLines(url.searchParams.get("lines"));
      const stdout = await cli(
        bin,
        ["--json", "terminal", "read", id, `--max-lines=${lines}`],
        CLI_TIMEOUT_MS,
        env,
      );
      const screen = JSON.parse(stdout);
      const sessions = await listSessions().catch(() => []);
      const summary = sessions.find((session) => Number(session.id) === Number(id));
      sendJson(response, 200, {
        id: Number(screen.id ?? id),
        rows: screen.rows,
        cols: screen.cols,
        text: String(screen.text ?? ""),
        run: summary?.run ?? null,
        fetched_at: new Date().toISOString(),
      });
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
  console.log("  loopback only, no auth; do not expose this port");

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
