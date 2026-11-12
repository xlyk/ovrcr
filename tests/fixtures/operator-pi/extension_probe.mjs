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
  chmodSync,
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
let originalExecutable;
let originalSessionsFile;
let originalGateFile;
let originalCapturedFile;
let originalPollStartedFile;
let originalReleaseFile;
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
  const sessionsPath = join(dir, "sessions.json");
  const cliPath = join(dir, "ovrcr");
  const gatePath = join(dir, "poll-gate");
  const capturedPath = join(dir, "poll-captured.json");
  const pollStartedPath = join(dir, "poll-started");
  const releasePath = join(dir, "poll-release");
  const sessions = [{
    scope: "Workspace",
    id: 41,
    run: 7,
    project: "sample",
    phase: "running",
    activity: "busy",
    kind: { agent: "codex" },
    agent: { activity_revision: 3 },
    unread: { turn: "turn-5" },
  }];
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  writeFileSync(
    cliPath,
    "#!/bin/sh\n[ \"$1\" = terminal ] && [ \"$2\" = list ] && [ \"$3\" = --json ] || exit 64\nif [ -f \"$WATCH_GATE_FILE\" ] && [ ! -f \"$WATCH_POLL_STARTED_FILE\" ]; then\n  cat \"$WATCH_SESSIONS_FILE\" > \"$WATCH_CAPTURED_FILE\"\n  touch \"$WATCH_POLL_STARTED_FILE\"\n  while [ ! -f \"$WATCH_RELEASE_FILE\" ]; do sleep 0.02; done\n  cat \"$WATCH_CAPTURED_FILE\"\nelse\n  cat \"$WATCH_SESSIONS_FILE\"\nfi\n",
  );
  chmodSync(cliPath, 0o700);
  originalExecutable = process.env.OVRCR_EXECUTABLE;
  originalSessionsFile = process.env.WATCH_SESSIONS_FILE;
  originalGateFile = process.env.WATCH_GATE_FILE;
  originalCapturedFile = process.env.WATCH_CAPTURED_FILE;
  originalPollStartedFile = process.env.WATCH_POLL_STARTED_FILE;
  originalReleaseFile = process.env.WATCH_RELEASE_FILE;
  process.env.OVRCR_EXECUTABLE = cliPath;
  process.env.WATCH_SESSIONS_FILE = sessionsPath;
  process.env.WATCH_GATE_FILE = gatePath;
  process.env.WATCH_CAPTURED_FILE = capturedPath;
  process.env.WATCH_POLL_STARTED_FILE = pollStartedPath;
  process.env.WATCH_RELEASE_FILE = releasePath;
  const statuses = [];
  const sentMessages = [];
  const confirmations = [];
  let idle = false;
  let admitWakeup = false;
  const extensionCtx = {
    ui: {
      setStatus(key, value) {
        statuses.push([key, JSON.parse(value)]);
      },
      async confirm(title, value) {
        confirmations.push([title, JSON.parse(value)]);
        return admitWakeup;
      },
    },
    sessionManager: { getSessionId: () => "operator-1" },
    isIdle: () => idle,
  };
  const pi = {
    on(event, handler) {
      handlers.set(event, handler);
    },
    registerTool(tool) {
      tools.set(tool.name, tool);
    },
    getActiveTools() {
      return ["read", "bash", "edit", "write", "watch_sessions"];
    },
    sendUserMessage(message) {
      sentMessages.push(message);
    },
  };
  install(pi);

  const replacementHost = () => {
    const replacementHandlers = new Map();
    const replacementTools = new Map();
    const messages = [];
    const context = {
      ui: {
        setStatus(key, value) {
          statuses.push([key, JSON.parse(value)]);
        },
        async confirm(title, value) {
          confirmations.push([title, JSON.parse(value)]);
          return admitWakeup;
        },
      },
      sessionManager: { getSessionId: () => "operator-1" },
      isIdle: () => idle,
    };
    install({
      on(event, handler) {
        replacementHandlers.set(event, handler);
      },
      registerTool(tool) {
        replacementTools.set(tool.name, tool);
      },
      getActiveTools() {
        return ["read", "bash", "edit", "write", "watch_sessions"];
      },
      sendUserMessage(message) {
        messages.push(message);
      },
    });
    return { handlers: replacementHandlers, tools: replacementTools, context, messages };
  };

  // 1. Sequential registration for every native tool.
  assert.deepEqual(
    [...tools.keys()].sort(),
    ["bash", "edit", "read", "watch_sessions", "write"],
    "the four native tools and watcher are registered",
  );
  for (const name of ["read", "bash", "edit", "write", "watch_sessions"]) {
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

  // watch_sessions validates a managed Workspace target, reports the active
  // lease, and exposes the same state through list.
  await handlers.get("session_start")({ type: "session_start" }, extensionCtx);
  const watcher = tools.get("watch_sessions");
  const add = await watcher.execute(
    "watch-add",
    {
      action: "add",
      sessions: [{
        session_id: 41,
        baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  assert.equal(JSON.parse(add.content[0].text).active, 1);
  const listed = await watcher.execute(
    "watch-list",
    { action: "list" },
    undefined,
    undefined,
    extensionCtx,
  );
  assert.equal(JSON.parse(listed.content[0].text).watches.length, 1);
  assert.equal(statuses.at(-1)[0], "ovrcr_operator_watches");
  assert.equal(statuses.at(-1)[1].active, 1);
  const removed = await watcher.execute(
    "watch-remove",
    { action: "remove", all: true },
    undefined,
    undefined,
    extensionCtx,
  );
  assert.equal(JSON.parse(removed.content[0].text).active, 0);
  assert.equal(sentMessages.length, 0);

  for (const invalid of [
    { action: "list", sessions: [] },
    { action: "add", sessions: [{ session_id: 41 }], all: true },
    { action: "remove", all: false },
    { action: "remove", all: true, session_ids: [41] },
    { action: "remove", sessions: [{ session_id: 41 }] },
  ]) {
    await assert.rejects(
      () => watcher.execute("watch-invalid-shape", invalid, undefined, undefined, extensionCtx),
      /accepts only|requires 1 to|requires session_ids/,
      JSON.stringify(invalid),
    );
  }

  // Omitted baselines capture the current Reporter identity, including no
  // unread turn; the current Ready observation alone does not settle it.
  sessions[0] = {
    ...sessions[0],
    phase: "running",
    activity: "response_ready",
    agent: { activity_revision: 3 },
    unread: null,
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await watcher.execute(
    "watch-capture-baseline",
    { action: "add", sessions: [{ session_id: 41 }] },
    undefined,
    undefined,
    extensionCtx,
  );
  const capturedBaseline = JSON.parse((await watcher.execute(
    "watch-captured-baseline-list",
    { action: "list" },
    undefined,
    undefined,
    extensionCtx,
  )).content[0].text);
  assert.deepEqual(capturedBaseline.watches[0].baseline, {
    run: 7,
    activity_revision: 3,
    unread_turn: null,
  });
  await watcher.execute(
    "watch-captured-baseline-clear",
    { action: "remove", all: true },
    undefined,
    undefined,
    extensionCtx,
  );
  sessions[0] = {
    ...sessions[0],
    activity: "busy",
    unread: { turn: "turn-5" },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));

  // Inventory size is independent of the cap on active watches: terminal list
  // may include fifty Workspace sessions, the dedicated Operator, and retained rows.
  sessions.push({
    scope: "OperatorConversation",
    id: 90,
    run: 1,
    phase: "running",
    activity: "busy",
    kind: { agent: "pi" },
    agent: { activity_revision: 1 },
  });
  for (let index = 0; index < 50; index += 1) {
    sessions.push({
      scope: "Workspace",
      id: 100 + index,
      run: 1,
      phase: "stopped",
      activity: "idle",
      kind: "terminal",
      agent: null,
    });
  }
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  const inventoryAdd = await watcher.execute(
    "watch-inventory",
    {
      action: "add",
      sessions: [{
        session_id: 41,
        baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  assert.equal(JSON.parse(inventoryAdd.content[0].text).active, 1);
  await watcher.execute(
    "watch-inventory-clear",
    { action: "remove", all: true },
    undefined,
    undefined,
    extensionCtx,
  );
  sessions.length = 1;
  writeFileSync(sessionsPath, JSON.stringify(sessions));

  // A managed Agent that stopped before this poll may no longer have a
  // Reporter snapshot; the supplied run baseline still makes terminal catch-up exact.
  sessions[0] = {
    ...sessions[0],
    phase: "running",
    activity: "waiting_input",
    agent: null,
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await assert.rejects(
    () => watcher.execute(
      "watch-unreported-live-agent",
      {
        action: "add",
        sessions: [{
          session_id: 41,
          baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
        }],
      },
      undefined,
      undefined,
      extensionCtx,
    ),
    /no Reporter lifecycle observation/,
  );
  await assert.rejects(
    () => watcher.execute(
      "watch-unbaselined-terminal-agent",
      { action: "add", sessions: [{ session_id: 41 }] },
      undefined,
      undefined,
      extensionCtx,
    ),
    /no Reporter lifecycle observation/,
  );

  sessions[0] = {
    ...sessions[0],
    phase: "stopped",
    activity: "idle",
    agent: null,
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await watcher.execute(
    "watch-stopped-catchup",
    {
      action: "add",
      sessions: [{
        session_id: 41,
        baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  const stoppedCatchup = JSON.parse((await watcher.execute(
    "watch-stopped-catchup-list",
    { action: "list" },
    undefined,
    undefined,
    extensionCtx,
  )).content[0].text);
  assert.equal(stoppedCatchup.watches.length, 0);
  assert.equal(stoppedCatchup.pending_reports[0].reason, "stopped");
  await watcher.execute(
    "watch-stopped-catchup-clear",
    { action: "remove", all: true },
    undefined,
    undefined,
    extensionCtx,
  );
  sessions[0] = {
    ...sessions[0],
    phase: "running",
    activity: "busy",
    agent: { activity_revision: 3 },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));

  // A caller-supplied pre-send Reporter baseline catches up a response that
  // settled before the watch was registered.
  sessions[0] = {
    ...sessions[0],
    activity: "response_ready",
    agent: { activity_revision: 4 },
    unread: { turn: "turn-5" },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await watcher.execute(
    "watch-catchup",
    {
      action: "add",
      sessions: [{
        session_id: 41,
        baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  const caught = JSON.parse((await watcher.execute(
    "watch-catchup-list",
    { action: "list" },
    undefined,
    undefined,
    extensionCtx,
  )).content[0].text);
  assert.equal(caught.active, 1);
  assert.equal(caught.watches.length, 0);
  assert.equal(caught.pending_reports.length, 1);
  assert.equal(caught.pending_reports[0].session_id, 41);
  await assert.rejects(
    () => watcher.execute(
      "watch-duplicate-pending-report",
      {
        action: "add",
        sessions: [{
          session_id: 41,
          baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
        }],
      },
      undefined,
      undefined,
      extensionCtx,
    ),
    /already watched/,
  );
  await watcher.execute(
    "watch-clear",
    { action: "remove", all: true },
    undefined,
    undefined,
    extensionCtx,
  );

  // Reporter unread turn ids are opaque strings, often UUIDs; a changed
  // non-null identity settles even when activity_revision is unchanged.
  sessions[0] = {
    ...sessions[0],
    activity: "response_ready",
    agent: { activity_revision: 3 },
    unread: { turn: "6dfd4aa9-55e1-41ac-a13d-1da5da819703" },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await watcher.execute(
    "watch-opaque-unread-catchup",
    {
      action: "add",
      sessions: [{
        session_id: 41,
        baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  const opaqueCatchup = JSON.parse((await watcher.execute(
    "watch-opaque-unread-list",
    { action: "list" },
    undefined,
    undefined,
    extensionCtx,
  )).content[0].text);
  assert.equal(opaqueCatchup.watches.length, 0);
  assert.equal(opaqueCatchup.pending_reports[0].reason, "response_ready");
  await watcher.execute(
    "watch-opaque-unread-clear",
    { action: "remove", all: true },
    undefined,
    undefined,
    extensionCtx,
  );

  const waitUntil = async (predicate, label) => {
    const until = Date.now() + 4000;
    while (!predicate()) {
      if (Date.now() >= until) throw new Error(label + " timed out");
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  };
  // A delayed old CLI sample cannot settle a replacement watch for the same
  // session after remove/re-add captures a newer baseline.
  sessions[0] = {
    ...sessions[0],
    activity: "busy",
    agent: { activity_revision: 4 },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  const staleWatch = await watcher.execute(
    "watch-stale",
    {
      action: "add",
      sessions: [{
        session_id: 41,
        baseline: { run: 7, activity_revision: 3, unread_turn: "turn-5" },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  assert.equal(JSON.parse(staleWatch.content[0].text).active, 1);
  writeFileSync(gatePath, "");
  sessions[0] = {
    ...sessions[0],
    activity: "response_ready",
    agent: { activity_revision: 5 },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await waitUntil(() => existsSync(pollStartedPath), "watch poll");
  sessions[0] = {
    ...sessions[0],
    activity: "busy",
    agent: { activity_revision: 4 },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await watcher.execute(
    "watch-replace-remove",
    { action: "remove", session_ids: [41] },
    undefined,
    undefined,
    extensionCtx,
  );
  await watcher.execute(
    "watch-replace-add",
    {
      action: "add",
      sessions: [{
        session_id: 41,
        baseline: { run: 7, activity_revision: 4, unread_turn: "turn-5" },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  writeFileSync(releasePath, "");
  await new Promise((resolve) => setTimeout(resolve, 100));
  const afterStalePoll = JSON.parse((await watcher.execute(
    "watch-after-stale-poll",
    { action: "list" },
    undefined,
    undefined,
    extensionCtx,
  )).content[0].text);
  assert.equal(afterStalePoll.watches.length, 1);
  assert.equal(afterStalePoll.pending_reports.length, 0);
  await watcher.execute(
    "watch-final-clear",
    { action: "remove", all: true },
    undefined,
    undefined,
    extensionCtx,
  );

  // Settlements during a user turn stay queued; one idle wakeup admission
  // batches them, and a denied admission retains every report for retry.
  sessions[0] = {
    ...sessions[0],
    phase: "running",
    activity: "busy",
    agent: { activity_revision: 4 },
    unread: { turn: "turn-5" },
  };
  sessions.push({
    scope: "Workspace",
    id: 42,
    run: 2,
    project: "sample",
    label: "second worker",
    phase: "running",
    activity: "busy",
    kind: { agent: "codex" },
    agent: { activity_revision: 2 },
    unread: { turn: "turn-9" },
  });
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await watcher.execute(
    "watch-batch-add",
    {
      action: "add",
      sessions: [
        { session_id: 41, baseline: { run: 7, activity_revision: 4, unread_turn: "turn-5" } },
        { session_id: 42, baseline: { run: 2, activity_revision: 2, unread_turn: "turn-9" } },
      ],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  sessions[0] = { ...sessions[0], activity: "response_ready", agent: { activity_revision: 5 } };
  sessions[1] = { ...sessions[1], activity: "response_ready", agent: { activity_revision: 3 } };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  const beforeSettlements = statuses.length;
  await waitUntil(() => statuses.length > beforeSettlements, "batched settlements");
  assert.equal(sentMessages.length, 0);
  assert.equal(confirmations.length, 0);
  idle = true;
  await handlers.get("agent_settled")({ type: "agent_settled" }, extensionCtx);
  assert.equal(confirmations.length, 1);
  assert.equal(confirmations[0][0], "OVRCR Operator async wakeup admission");
  assert.equal(sentMessages.length, 0);
  const retainedReports = JSON.parse((await watcher.execute(
    "watch-denied-list",
    { action: "list" },
    undefined,
    undefined,
    extensionCtx,
  )).content[0].text);
  assert.equal(retainedReports.active, 2);
  assert.equal(retainedReports.pending_reports.length, 2);

  // A native switch reloads the extension factory inside the same Node
  // process. Keep one live watch and an in-flight CLI poll while the two
  // queued reports cross the resume shutdown/start boundary.
  sessions.push({
    scope: "Workspace",
    id: 43,
    run: 4,
    project: "sample",
    label: "third worker",
    phase: "running",
    activity: "busy",
    kind: { agent: "codex" },
    agent: { activity_revision: 8 },
    unread: null,
  });
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  const thirdWatch = await watcher.execute(
    "watch-resume-add",
    {
      action: "add",
      sessions: [{
        session_id: 43,
        baseline: { run: 4, activity_revision: 8, unread_turn: null },
      }],
    },
    undefined,
    undefined,
    extensionCtx,
  );
  assert.equal(JSON.parse(thirdWatch.content[0].text).active, 3);
  rmSync(pollStartedPath, { force: true });
  rmSync(releasePath, { force: true });
  writeFileSync(gatePath, "");
  await waitUntil(() => existsSync(pollStartedPath), "resume in-flight poll");

  idle = false;
  await handlers.get("session_shutdown")(
    { type: "session_shutdown", reason: "resume" },
    extensionCtx,
  );
  const resumed = replacementHost();
  await resumed.handlers.get("session_start")(
    { type: "session_start", reason: "resume" },
    resumed.context,
  );
  const resumedWatcher = resumed.tools.get("watch_sessions");
  const preserved = JSON.parse((await resumedWatcher.execute(
    "watch-resume-list",
    { action: "list" },
    undefined,
    undefined,
    resumed.context,
  )).content[0].text);
  assert.equal(preserved.active, 3);
  assert.equal(preserved.pending_reports.length, 2);
  assert.deepEqual(preserved.watches.map((watch) => watch.session_id), [43]);
  assert.equal(statuses.at(-1)[1].active, 3, "session_start re-emits the preserved lease");

  admitWakeup = true;
  await resumed.handlers.get("agent_settled")({ type: "agent_settled" }, resumed.context);
  assert.equal(resumed.messages.length, 0, "pending reports wait until the resumed Pi host is idle");
  writeFileSync(releasePath, "");
  sessions[2] = {
    ...sessions[2],
    activity: "response_ready",
    agent: { activity_revision: 9 },
  };
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  idle = true;
  await resumed.handlers.get("agent_settled")({ type: "agent_settled" }, resumed.context);
  assert.equal(resumed.messages.length, 1);
  assert.match(resumed.messages[0], /session 41/);
  assert.match(resumed.messages[0], /second worker/);
  await resumed.handlers.get("agent_start")({ type: "agent_start" }, resumed.context);
  const afterResumeDispatch = JSON.parse((await resumedWatcher.execute(
    "watch-resume-consumed-list",
    { action: "list" },
    undefined,
    undefined,
    resumed.context,
  )).content[0].text);
  assert.equal(afterResumeDispatch.active, 1);
  assert.equal(afterResumeDispatch.pending_reports.length, 0);
  const beforeResumedPoll = statuses.length;
  await waitUntil(() => statuses.length > beforeResumedPoll, "preserved watch timer poll");
  const settledAfterResume = JSON.parse((await resumedWatcher.execute(
    "watch-resumed-session-settled",
    { action: "list" },
    undefined,
    undefined,
    resumed.context,
  )).content[0].text);
  assert.equal(settledAfterResume.active, 1);
  assert.equal(settledAfterResume.pending_reports[0].session_id, 43);
  await resumed.handlers.get("agent_settled")({ type: "agent_settled" }, resumed.context);
  assert.equal(resumed.messages.length, 2);
  assert.match(resumed.messages[1], /session 43/);
  await resumed.handlers.get("agent_start")({ type: "agent_start" }, resumed.context);

  // A genuine shutdown drops owned watch state. A later extension factory in
  // this process starts clean and reports no stale lease or queued report.
  sessions.push({
    scope: "Workspace",
    id: 44,
    run: 5,
    project: "sample",
    phase: "running",
    activity: "busy",
    kind: { agent: "codex" },
    agent: { activity_revision: 1 },
    unread: null,
  });
  writeFileSync(sessionsPath, JSON.stringify(sessions));
  await resumedWatcher.execute(
    "watch-genuine-shutdown-add",
    {
      action: "add",
      sessions: [{
        session_id: 44,
        baseline: { run: 5, activity_revision: 1, unread_turn: null },
      }],
    },
    undefined,
    undefined,
    resumed.context,
  );
  await resumed.handlers.get("session_shutdown")(
    { type: "session_shutdown", reason: "quit" },
    resumed.context,
  );
  const fresh = replacementHost();
  await fresh.handlers.get("session_start")(
    { type: "session_start", reason: "startup" },
    fresh.context,
  );
  const clean = JSON.parse((await fresh.tools.get("watch_sessions").execute(
    "watch-genuine-shutdown-list",
    { action: "list" },
    undefined,
    undefined,
    fresh.context,
  )).content[0].text);
  assert.equal(clean.active, 0);
  assert.deepEqual(clean.watches, []);
  assert.deepEqual(clean.pending_reports, []);
  idle = false;
  admitWakeup = false;

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

  console.log("EXTENSION_PROBE PASS sequential=5 watches=add,list,remove,catchup timeout_ms=" + elapsed);
} finally {
  if (typeof originalExecutable === "undefined") delete process.env.OVRCR_EXECUTABLE;
  else process.env.OVRCR_EXECUTABLE = originalExecutable;
  if (typeof originalSessionsFile === "undefined") delete process.env.WATCH_SESSIONS_FILE;
  else process.env.WATCH_SESSIONS_FILE = originalSessionsFile;
  if (typeof originalGateFile === "undefined") delete process.env.WATCH_GATE_FILE;
  else process.env.WATCH_GATE_FILE = originalGateFile;
  if (typeof originalCapturedFile === "undefined") delete process.env.WATCH_CAPTURED_FILE;
  else process.env.WATCH_CAPTURED_FILE = originalCapturedFile;
  if (typeof originalPollStartedFile === "undefined") delete process.env.WATCH_POLL_STARTED_FILE;
  else process.env.WATCH_POLL_STARTED_FILE = originalPollStartedFile;
  if (typeof originalReleaseFile === "undefined") delete process.env.WATCH_RELEASE_FILE;
  else process.env.WATCH_RELEASE_FILE = originalReleaseFile;
  process.removeListener("exit", cleanup);
  cleanup();
}
