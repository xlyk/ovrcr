import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createApp } from "../server.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const STUB = join(HERE, "stub-ovrcr.mjs");

async function harness(t, { ttlMs = 5000, env = {} } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-web-"));
  const log = join(dir, "calls.log");
  const app = createApp({
    bin: STUB,
    ttlMs,
    env: { ...process.env, STUB_LOG: log, ...env },
    log: () => {},
  });
  const address = await app.listen(0);
  t.after(async () => {
    await app.close();
    rmSync(dir, { recursive: true, force: true });
  });

  const calls = () =>
    existsSync(log)
      ? readFileSync(log, "utf8")
          .trim()
          .split("\n")
          .filter(Boolean)
          .map((line) => JSON.parse(line))
      : [];

  return { base: `http://127.0.0.1:${address.port}`, calls };
}

const get = (base, path) => fetch(`${base}${path}`);
const post = (base, path, body) =>
  fetch(`${base}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

test("serves the app shell with a restrictive CSP", async (t) => {
  const { base } = await harness(t);
  const response = await get(base, "/");
  assert.equal(response.status, 200);
  assert.match(await response.text(), /OVRCR/);
  assert.match(response.headers.get("content-security-policy"), /default-src 'self'/);
  assert.equal(response.headers.get("cache-control"), "no-store");
});

test("state returns the CLI inventory and caches it for the TTL", async (t) => {
  const { base, calls } = await harness(t);

  const first = await get(base, "/api/state");
  assert.equal(first.status, 200);
  const body = await first.json();
  assert.equal(body.sessions.length, 2);
  assert.equal(body.sessions[0].activity, "waiting_input");

  await get(base, "/api/state");
  assert.equal(calls().filter((args) => args.includes("list")).length, 1);
});

test("a zero TTL refetches the inventory", async (t) => {
  const { base, calls } = await harness(t, { ttlMs: 0 });
  await get(base, "/api/state");
  await get(base, "/api/state");
  assert.equal(calls().filter((args) => args.includes("list")).length, 2);
});

test("screen reads the requested line count and reports the run from the list", async (t) => {
  const { base, calls } = await harness(t);

  const response = await get(base, "/api/session/12/screen?lines=7");
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.id, 12);
  assert.equal(body.rows, 24);
  assert.equal(body.text, "screen for 12\n");
  assert.equal(body.run, 3);

  const read = calls().find((args) => args.includes("read"));
  assert.deepEqual(read, ["--json", "terminal", "read", "12", "--max-lines=7"]);
});

test("send submits by default and omits Enter on request", async (t) => {
  const { base, calls } = await harness(t);

  const submitted = await post(base, "/api/session/12/send", { text: "echo hi" });
  assert.equal(submitted.status, 200);

  await post(base, "/api/session/12/send", { text: "echo hi", submit: false });

  const sends = calls().filter((args) => args.includes("send"));
  assert.deepEqual(sends[0], ["--json", "terminal", "send", "12", "--text=echo hi"]);
  assert.deepEqual(sends[1], ["--json", "terminal", "send", "12", "--text=echo hi", "--no-submit"]);
});

test("keys are validated and passed through unchanged", async (t) => {
  const { base, calls } = await harness(t);

  const accepted = await post(base, "/api/session/12/key", { key: ":escape:" });
  assert.equal(accepted.status, 200);
  assert.deepEqual(calls().at(-1), ["--json", "terminal", "keystroke", "12", ":escape:"]);

  const rejected = await post(base, "/api/session/12/key", { key: "escape" });
  assert.equal(rejected.status, 400);
  assert.equal(calls().filter((args) => args.includes("keystroke")).length, 1);
});

test("review forwards the exact unread object", async (t) => {
  const { base, calls } = await harness(t);
  const expected = {
    binding: { provider: "Claude", invocation: "inv", conversation: "conv", generation: 1 },
    turn: "turn-1",
    activity_revision: 7,
  };

  const response = await post(base, "/api/session/12/review", { expected });
  assert.equal(response.status, 200);

  const review = calls().find((args) => args.includes("mark-reviewed"));
  assert.equal(review[3], "12");
  assert.equal(review[4], `--expected=${JSON.stringify(expected)}`);
});

test("bad identifiers, methods, and paths are refused", async (t) => {
  const { base, calls } = await harness(t);

  assert.equal((await get(base, "/api/session/0/screen")).status, 400);
  assert.equal((await get(base, "/api/session/abc/screen")).status, 400);
  assert.equal((await get(base, "/api/session/12/send")).status, 405);
  assert.equal((await get(base, "/nope")).status, 404);
  assert.equal((await post(base, "/api/session/12/nope", {})).status, 404);
  assert.equal(calls().length, 0);
});

test("oversized text and bodies are refused before the CLI runs", async (t) => {
  const { base, calls } = await harness(t);

  assert.equal((await post(base, "/api/session/12/send", { text: "x".repeat(33_000) })).status, 413);
  assert.equal(
    (await post(base, "/api/session/12/send", { text: "x".repeat(70 * 1024) })).status,
    413,
  );
  assert.equal(calls().length, 0);
});

test("CLI failures map to HTTP status and message", async (t) => {
  const { base } = await harness(t, { env: { STUB_FAIL: "NotFound:session 99 not found" } });
  const missing = await get(base, "/api/session/99/screen");
  assert.equal(missing.status, 404);
  assert.equal((await missing.json()).error.message, "session 99 not found");

  const { base: offline } = await harness(t, {
    env: { STUB_FAIL: "Internal:OVRCR server is not running" },
  });
  const down = await get(offline, "/api/state");
  assert.equal(down.status, 503);
  assert.match((await down.json()).error.message, /not running/);
});
