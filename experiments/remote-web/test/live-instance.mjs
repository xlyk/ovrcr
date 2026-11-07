// Starts a real OVRCR server against a disposable instance directory and
// checks that the bridge reads its empty inventory through the real CLI.
//
// It cannot create a session here: this sandbox has no available PTY devices.
// The write paths are covered by test/server.test.mjs with a stub CLI.

import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createApp } from "../server.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "..", "..", "..");

function resolveBin() {
  if (process.env.OVRCR_BIN) return process.env.OVRCR_BIN;
  // A fresh worktree has no target/ of its own; the parent checkout's build is
  // the same revision when the branch was cut from it.
  const sibling = join(REPO, "..", "..", "target", "debug", "ovrcr");
  if (existsSync(sibling)) return sibling;
  try {
    return execFileSync("which", ["ovrcr"], { encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

const BIN = resolveBin();
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

if (!BIN) {
  console.log("SKIP: no ovrcr binary; set OVRCR_BIN or build target/debug/ovrcr");
  process.exit(0);
}

const home = mkdtempSync(join(tmpdir(), "ovrcr-web-live-"));
const env = { ...process.env, OVRCR_HOME: home };
delete env.OVRCR_SOCKET;
delete env.OVRCR_CONFIG;

const server = spawn(BIN, ["server"], { env, stdio: "ignore" });
let app = null;

function stop() {
  spawnSync(BIN, ["--json", "shutdown"], { env, timeout: 5000 });
  if (!server.killed) server.kill("SIGTERM");
  rmSync(home, { recursive: true, force: true });
}

try {
  const socket = join(home, "server.sock");
  const deadline = Date.now() + 10_000;
  while (!existsSync(socket) && Date.now() < deadline) {
    await sleep(50);
  }
  assert.ok(existsSync(socket), "server socket was not created");

  app = createApp({ bin: BIN, env, log: () => {} });
  const address = await app.listen(0);
  const base = `http://127.0.0.1:${address.port}`;

  const response = await fetch(`${base}/api/state`);
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.deepEqual(body.sessions, []);

  const shell = await fetch(`${base}/`);
  assert.equal(shell.status, 200);

  console.log("PASS: real OVRCR server, empty inventory through the real CLI");
} finally {
  if (app) await app.close();
  stop();
}
