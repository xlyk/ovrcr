#!/usr/bin/env node
/**
 * Behavioral probe for `src/pi-operator-extension.mjs`.
 *
 * The real extension source is copied next to a minimal stub of
 * `@earendil-works/pi-coding-agent` so the module resolves offline, without the
 * installed Pi package and without a provider. The stub records the definitions
 * the extension registers and hands back the attached Bash `operations` object,
 * which is the code under test. The probe then exercises:
 *
 *   1. every native tool is registered with `executionMode: "sequential"`
 *      (Pi's parallel executor prepares all admissions before executing any,
 *      which is incompatible with OVRCR's serial durable admission),
 *   2. a timed Bash call settles at its deadline even while a background child
 *      keeps writing to the inherited pipe,
 *   3. an abort while the cwd check is pending spawns nothing,
 *   4. an acknowledged timed-out result aborts instead of returning a decision.
 *
 * Usage: node extension_probe.mjs <absolute path to pi-operator-extension.mjs>
 */
import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const sourcePath = process.argv[2];
assert(sourcePath, "extension source path required");
const source = readFileSync(sourcePath, "utf8");

const dir = mkdtempSync(join(tmpdir(), "ovrcr-operator-extension-"));
// The probe owns this directory and nothing else: remove exactly the path
// `mkdtempSync` handed back, whatever way the process ends (success, throw, or
// an explicit `process.exit` elsewhere).
const cleanup = () => {
  try {
    rmSync(dir, { recursive: true, force: true });
  } catch {
    // Teardown is best effort; never mask the probe's own outcome.
  }
};
process.on("exit", cleanup);
try {
  const packageDir = join(
    dir,
    "node_modules",
    "@earendil-works",
    "pi-coding-agent",
  );
  mkdirSync(packageDir, { recursive: true });
  writeFileSync(join(dir, "package.json"), JSON.stringify({ type: "module" }));
  writeFileSync(
    join(packageDir, "package.json"),
    JSON.stringify({
      name: "@earendil-works/pi-coding-agent",
      type: "module",
      exports: { ".": "./index.mjs" },
    }),
  );
  writeFileSync(
    join(packageDir, "index.mjs"),
    `
  export const captured = { bashOptions: null, factories: [] };
  function definition(name) {
    captured.factories.push(name);
    return {
      name,
      label: name,
      description: \`stub \${name}\`,
      parameters: {},
      async execute() { throw new Error("stub execute must not run"); },
    };
  }
  export function createBashToolDefinition(cwd, options) {
    captured.bashOptions = options;
    return definition("bash");
  }
  export function createReadToolDefinition() { return definition("read"); }
  export function createEditToolDefinition() { return definition("edit"); }
  export function createWriteToolDefinition() { return definition("write"); }
  `,
  );
  writeFileSync(join(dir, "extension.mjs"), source);

  const { captured } = await import(
    pathToFileURL(join(packageDir, "index.mjs")).href
  );
  const { default: install } = await import(
    pathToFileURL(join(dir, "extension.mjs")).href
  );
  assert.equal(typeof install, "function", "extension default export");

  const handlers = new Map();
  const tools = new Map();
  const pi = {
    on(event, handler) {
      handlers.set(event, handler);
    },
    registerTool(tool) {
      tools.set(tool.name, tool);
    },
    getActiveTools() {
      return ["read", "bash", "edit", "write"];
    },
  };
  install(pi);

  // 1. Sequential registration for every native tool.
  assert.deepEqual(
    [...tools.keys()].sort(),
    ["bash", "edit", "read", "write"],
    "all four native tools are registered",
  );
  for (const name of ["read", "bash", "edit", "write"]) {
    assert.equal(
      tools.get(name)?.executionMode,
      "sequential",
      `${name} must be sequential so the whole tool batch runs sequentially`,
    );
  }
  assert.deepEqual(
    captured.factories.sort(),
    ["bash", "edit", "read", "write"],
    "the extension uses Pi's own tool factories",
  );
  const operations = captured.bashOptions?.operations;
  assert.equal(typeof operations?.exec, "function", "attached Bash operations");
  assert.equal(captured.bashOptions.exposeSessionEnvironment, false);

  const withDeadline = (promise, ms, label) =>
    Promise.race([
      promise,
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error(`${label} exceeded ${ms}ms`)), ms).unref?.(),
      ),
    ]);

  // 2. The timeout settles at the deadline despite a background pipe writer.
  const writingChild = [
    "( for i in $(seq 1 100); do printf x; sleep 0.05; done ) &",
    "sleep 10",
  ].join("\n");
  const startedAt = Date.now();
  let timeoutError = null;
  try {
    await withDeadline(
      operations.exec(writingChild, process.cwd(), {
        onData() {},
        signal: undefined,
        timeout: 0.3,
        env: process.env,
      }),
      3000,
      "timed bash",
    );
  } catch (error) {
    timeoutError = error;
  }
  assert(timeoutError instanceof Error, "the timed call must reject");
  assert.match(
    String(timeoutError.message),
    /^timeout:0\.3/,
    "Pi's native timeout translation needs the timeout:<seconds> message",
  );
  const elapsed = Date.now() - startedAt;
  assert(elapsed < 1500, `timeout must settle at its deadline, took ${elapsed}ms`);

  // 3. An abort during the async cwd check never spawns the command.
  const marker = join(dir, "spawned-marker");
  const controller = new AbortController();
  const pending = operations.exec(`printf ran > '${marker}'`, process.cwd(), {
    onData() {},
    signal: controller.signal,
    timeout: undefined,
    env: process.env,
  });
  controller.abort();
  let cancelled = null;
  try {
    await withDeadline(pending, 3000, "aborted bash");
  } catch (error) {
    cancelled = error;
  }
  assert(cancelled instanceof Error, "the aborted call must reject");
  assert.match(String(cancelled.message), /aborted/);
  await new Promise((resolve) => setTimeout(resolve, 200));
  assert(!existsSync(marker), "an aborted call must not run its command");

  // 4. A confirmed timed-out result aborts and returns no decision.
  const admission = handlers.get("tool_call");
  const resultHook = handlers.get("tool_result");
  assert.equal(typeof admission, "function");
  assert.equal(typeof resultHook, "function");
  const admissionCtx = {
    ui: { confirm: async () => true, setStatus() {} },
    abort() {},
    signal: undefined,
  };
  assert.equal(
    await admission(
      { toolName: "bash", toolCallId: "call-timeout", input: { command: "true" } },
      admissionCtx,
    ),
    undefined,
  );
  let timedOut = null;
  try {
    await withDeadline(
      operations.exec("sleep 10", process.cwd(), {
        onData() {},
        signal: undefined,
        timeout: 0.05,
        env: process.env,
      }),
      3000,
      "timed result call",
    );
  } catch (error) {
    timedOut = error;
  }
  assert(timedOut instanceof Error, "the timeout must reject");
  let aborted = false;
  const resultCtx = {
    ui: { confirm: async () => true },
    abort() {
      aborted = true;
    },
    signal: undefined,
  };
  const decision = await resultHook(
    {
      toolCallId: "call-timeout",
      content: [{ type: "text", text: "Command timed out after 0.05 seconds" }],
      isError: true,
      details: {},
    },
    resultCtx,
  );
  assert.equal(aborted, true, "a timed-out result must abort the agent loop");
  assert.equal(decision, undefined, "a timed-out result returns no decision");

  console.log("EXTENSION_PROBE PASS sequential=4 timeout_ms=" + elapsed);
} finally {
  process.removeListener("exit", cleanup);
  cleanup();
}
