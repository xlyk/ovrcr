/**
 * OVRCR lifecycle adapter for the dedicated Operator Pi profile.
 *
 * This is the only custom code loaded into that profile: Pi supplies the four
 * native tool factories (read/bash/edit/write) and their validation and truncation,
 * inference and context management; OVRCR defines watch_sessions. This file registers
 * the four native tools and the custom watch_sessions tool with
 * `executionMode: "sequential"` (the durable admission handshake is serial),
 * overrides just `bash` so commands stay attached to the worker's process
 * group and stream its output through Pi's native Bash definition, and turns
 * Pi's ordinary tool callback into the OVRCR durable admission handshake.
 *
 * Import mechanism (pi 0.87.1): extensions are loaded through jiti with an
 * alias table, so a bare `@earendil-works/pi-coding-agent` import resolves to
 * the running Pi package even though this file lives outside node_modules.
 * See dist/core/extensions/loader.js (getAliases / VIRTUAL_MODULES) and
 * dist/core/extensions/virtual-modules.js. No node_modules or bundling is
 * required next to the materialized copy.
 */
import { execFile, spawn } from "node:child_process";
import { constants } from "node:fs";
import { access } from "node:fs/promises";
import { constants as osConstants } from "node:os";
import { promisify } from "node:util";
import {
  createBashToolDefinition,
  createEditToolDefinition,
  createReadToolDefinition,
  createWriteToolDefinition,
} from "@earendil-works/pi-coding-agent";

/** Keep the journal payload bounded; Pi keeps its own native truncation. */
const OUTPUT_LIMIT = 64 * 1024;
/** Quiet window after child exit before trailing inherited pipes are ignored. */
const DRAIN_GRACE_MS = 100;
const MAX_TIMEOUT_SECONDS = 2_147_483.647;

const ADMISSION_TITLE = "OVRCR Operator tool admission";
const RESULT_TITLE = "OVRCR Operator tool result";
const WAKEUP_ADMISSION_TITLE = "OVRCR Operator async wakeup admission";
const WATCHES_KEY = "ovrcr_operator_watches";
const WATCH_STATE = Symbol.for("ovrcr.operator.watchState");
const MAX_WATCHES = 50;
const MAX_REPORTS = 50;
const MAX_WATCH_OUTPUT = 1024 * 1024;
const WATCH_CLI_TIMEOUT_MS = 5000;
const WATCH_POLL_MS = 2500;
const execFileAsync = promisify(execFile);

function validName(value) {
  return typeof value === "string" &&
    value.length > 0 &&
    Buffer.byteLength(value, "utf8") <= 256 &&
    !/[\u0000-\u001f\u007f]/u.test(value);
}

function validCounter(value, allowZero = false) {
  return Number.isSafeInteger(value) && (allowZero ? value >= 0 : value > 0);
}

function validUnreadTurn(value) {
  return value === null || validName(value);
}

function validTerminalPhase(value) {
  return ["running", "paused", "exited", "stopped", "interrupted"].includes(value);
}

function validTerminalActivity(value) {
  return ["unknown", "idle", "busy", "waiting_input", "error", "response_ready"].includes(value);
}

function unreadTurn(session) {
  return session?.unread?.turn ?? null;
}

function activeCount(state) {
  return state.watched.size + state.pendingReports.length;
}

function publishWatchStatus(state) {
  state.ctx?.ui.setStatus(WATCHES_KEY, JSON.stringify({ active: activeCount(state) }));
}

function ensureWatchTimer(state) {
  if (!state.timer && activeCount(state) > 0) {
    state.timer = setInterval(() => {
      void pollWatches(state);
    }, WATCH_POLL_MS);
    state.timer.unref?.();
  }
}

async function pollWatches(state) {
  if (state.inFlight || state.suspended) return;
  state.inFlight = true;
  try {
    if (state.watched.size > 0) {
      const watched = new Map(state.watched);
      const sessions = await terminalSessions();
      if (!state.suspended) {
        const byId = new Map(sessions.map((session) => [session?.id, session]));
        let changed = false;
        for (const [sessionId, watch] of watched) {
          if (state.watched.get(sessionId) !== watch) continue;
          const session = byId.get(sessionId);
          const reason = settledReason(session, watch.baseline);
          if (!reason || state.pendingReports.length >= MAX_REPORTS) continue;
          state.watched.delete(sessionId);
          state.pendingReports.push(settledReport(watch, session, reason));
          changed = true;
        }
        if (changed) publishWatchStatus(state);
      }
    }
  } catch {
    // A transient CLI failure leaves the watch lease in place for the next tick.
  } finally {
    state.inFlight = false;
  }
  await flushPendingReports(state);
}

function reportText(reports) {
  return [
    "These watched OVRCR Workspace Agent sessions settled:",
    ...reports.map((report) =>
      "- session " + report.session_id + " (" + report.label + ") in " +
      report.project + ": " + report.reason + " (phase " + report.phase +
      ", activity " + report.activity + ", run " + report.run + ")",
    ),
  ].join("\n");
}

function finishDispatch(state, batch) {
  if (state.dispatchBatch !== batch) return;
  state.dispatchBatch = null;
  state.dispatching = false;
}

async function flushPendingReports(state) {
  if (state.suspended || state.dispatching || !state.ctx ||
    state.pendingReports.length === 0 || !state.ctx.isIdle()) {
    return;
  }
  const batch = state.pendingReports.slice();
  const text = reportText(batch);
  if (Buffer.byteLength(text, "utf8") > 64 * 1024) return;
  state.dispatching = true;
  state.dispatchBatch = batch;
  let accepted;
  try {
    accepted = await state.ctx.ui.confirm(
      WAKEUP_ADMISSION_TITLE,
      JSON.stringify({ text }),
    );
  } catch {
    finishDispatch(state, batch);
    return;
  }
  if (!accepted) {
    finishDispatch(state, batch);
    return;
  }
  if (state.suspended || state.dispatchBatch !== batch) {
    finishDispatch(state, batch);
    return;
  }
  try {
    const sending = state.pi.sendUserMessage(text);
    if (sending && typeof sending.catch === "function") {
      sending.catch(() => finishDispatch(state, batch));
    }
  } catch {
    finishDispatch(state, batch);
  }
}

function stopWatchTimerIfEmpty(state) {
  if (activeCount(state) === 0 && state.timer) {
    clearInterval(state.timer);
    state.timer = null;
  }
}

function watchResult(value) {
  return { content: [{ type: "text", text: JSON.stringify(value) }] };
}

function watchState(pi) {
  let state = globalThis[WATCH_STATE];
  if (!state) {
    state = {
      watched: new Map(),
      pendingReports: [],
      timer: null,
      inFlight: false,
      dispatching: false,
      dispatchBatch: null,
      pi,
      ctx: null,
      suspended: false,
    };
    globalThis[WATCH_STATE] = state;
  }
  state.pi = pi;
  return state;
}

async function terminalSessions() {
  const executable = process.env.OVRCR_EXECUTABLE;
  if (!executable) throw new Error("OVRCR_EXECUTABLE is unavailable");
  const { stdout } = await execFileAsync(executable, ["terminal", "list", "--json"], {
    encoding: "utf8",
    timeout: WATCH_CLI_TIMEOUT_MS,
    maxBuffer: MAX_WATCH_OUTPUT,
  });
  if (Buffer.byteLength(stdout, "utf8") > MAX_WATCH_OUTPUT) {
    throw new Error("terminal list output is too large");
  }
  const sessions = JSON.parse(stdout);
  if (!Array.isArray(sessions)) {
    throw new Error("terminal list returned invalid session data");
  }
  return sessions;
}

function validateBaseline(value) {
  if (!value || typeof value !== "object" || Array.isArray(value) ||
    !validCounter(value.run) ||
    !validCounter(value.activity_revision, true) ||
    !validUnreadTurn(value.unread_turn) ||
    Object.keys(value).some((key) => !["run", "activity_revision", "unread_turn"].includes(key))) {
    throw new Error("invalid watch baseline");
  }
  return {
    run: value.run,
    activity_revision: value.activity_revision,
    unread_turn: value.unread_turn,
  };
}

function validateTarget(input, sessions) {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    throw new Error("invalid watch entry");
  }
  const { session_id: sessionId, project, label, baseline: suppliedBaseline } = input;
  if (!validCounter(sessionId) ||
    (project !== undefined && !validName(project)) ||
    (label !== undefined && !validName(label)) ||
    Object.keys(input).some((key) => !["session_id", "project", "label", "baseline"].includes(key))) {
    throw new Error("invalid watch entry");
  }
  const target = sessions.find((session) => session?.id === sessionId);
  if (!target || target.scope !== "Workspace" ||
    !validName(target.kind?.agent) ||
    !validCounter(target.run) ||
    !validName(target.project) ||
    !validTerminalPhase(target.phase) ||
    !validTerminalActivity(target.activity) ||
    !validUnreadTurn(unreadTurn(target)) ||
    (target.unread != null &&
      (typeof target.unread !== "object" || Array.isArray(target.unread) ||
        !Object.hasOwn(target.unread, "turn"))) ||
    (project !== undefined && target.project !== project)) {
    throw new Error("session " + sessionId + " is not a reported Workspace Agent session");
  }
  const hasReporterRevision = validCounter(target.agent?.activity_revision, true);
  const baseline = suppliedBaseline === undefined
    ? {
        run: target.run,
        activity_revision: hasReporterRevision ? target.agent.activity_revision : 0,
        unread_turn: unreadTurn(target),
      }
    : validateBaseline(suppliedBaseline);
  if (!hasReporterRevision) {
    const suppliedTerminalOrRunChange = suppliedBaseline !== undefined &&
      (target.run !== baseline.run ||
        ["exited", "stopped", "interrupted"].includes(target.phase));
    if (!suppliedTerminalOrRunChange) {
      throw new Error("session " + sessionId + " has no Reporter lifecycle observation");
    }
  }
  const targetLabel = [target.label, target.display_name]
    .find((candidate) => validName(candidate));
  return {
    session_id: sessionId,
    project: target.project,
    label: label ?? targetLabel ?? ("session " + sessionId),
    baseline,
    target,
  };
}

function settledReason(session, baseline) {
  if (!session) return "missing";
  if (["exited", "stopped", "interrupted"].includes(session.phase)) return session.phase;
  if (session.run !== baseline.run) return "run_changed";
  if (session.activity === "waiting_input") return "waiting_input";
  const revision = session.agent?.activity_revision;
  const currentUnreadTurn = unreadTurn(session);
  if (session.activity === "response_ready" &&
    ((validCounter(revision, true) && revision > baseline.activity_revision) ||
      (currentUnreadTurn !== null && currentUnreadTurn !== baseline.unread_turn))) {
    return "response_ready";
  }
  return null;
}

function settledReport(watch, session, reason) {
  return {
    session_id: watch.session_id,
    project: watch.project,
    label: watch.label,
    run: session?.run ?? watch.baseline.run,
    phase: session?.phase ?? "missing",
    activity: session?.activity ?? "unknown",
    reason,
  };
}

function validateWatchInput(input) {
  if (!input || typeof input !== "object" || Array.isArray(input) ||
    !["add", "list", "remove"].includes(input.action)) {
    throw new Error("watch_sessions requires action add, list, or remove");
  }
  if (Object.keys(input).some((key) =>
    !["action", "sessions", "session_ids", "all"].includes(key))) {
    throw new Error("invalid watch_sessions arguments");
  }
  if (input.action === "list" && Object.keys(input).length !== 1) {
    throw new Error("list accepts only action");
  }
  if (input.action === "add" &&
    (Object.keys(input).some((key) => key !== "action" && key !== "sessions") ||
      !Array.isArray(input.sessions) || input.sessions.length === 0 || input.sessions.length > MAX_WATCHES)) {
    throw new Error("add requires 1 to " + MAX_WATCHES + " sessions");
  }
  if (input.action === "remove") {
    const keys = Object.keys(input);
    const all = input.all === true && input.session_ids === undefined && keys.length === 2;
    const ids = input.all === undefined && Array.isArray(input.session_ids) &&
      input.session_ids.length > 0 && input.session_ids.every(validCounter) && keys.length === 2;
    if (!all && !ids) throw new Error("remove requires session_ids or all: true");
  }
  return input;
}

async function watchSessions(state, rawInput, ctx) {
  const input = validateWatchInput(rawInput);
  state.ctx = ctx;
  if (input.action === "list") {
    return watchResult({
      active: activeCount(state),
      watches: [...state.watched.values()],
      pending_reports: state.pendingReports,
    });
  }
  if (input.action === "remove") {
    const ids = input.all ? null : new Set(input.session_ids);
    const before = activeCount(state);
    if (ids === null) {
      state.watched.clear();
      state.pendingReports = [];
      state.dispatchBatch = null;
      state.dispatching = false;
    } else {
      for (const id of ids) state.watched.delete(id);
      state.pendingReports = state.pendingReports.filter((report) => !ids.has(report.session_id));
      if (state.dispatchBatch?.some((report) => ids.has(report.session_id))) {
        state.dispatchBatch = null;
        state.dispatching = false;
      }
    }
    stopWatchTimerIfEmpty(state);
    if (before !== activeCount(state)) publishWatchStatus(state);
    return watchResult({ active: activeCount(state), removed: before - activeCount(state) });
  }

  const sessions = await terminalSessions();
  const additions = input.sessions.map((entry) => validateTarget(entry, sessions));
  const incoming = new Set();
  const activeSessionIds = new Set([
    ...state.watched.keys(),
    ...state.pendingReports.map((report) => report.session_id),
  ]);
  for (const entry of additions) {
    if (incoming.has(entry.session_id) || activeSessionIds.has(entry.session_id)) {
      throw new Error("session " + entry.session_id + " is already watched");
    }
    incoming.add(entry.session_id);
  }
  if (activeCount(state) + additions.length > MAX_WATCHES) {
    throw new Error("at most " + MAX_WATCHES + " sessions and queued reports can be watched");
  }
  for (const entry of additions) {
    const { target, ...watch } = entry;
    const reason = settledReason(target, watch.baseline);
    if (reason) {
      if (state.pendingReports.length >= MAX_REPORTS) {
        throw new Error("too many queued watch reports");
      }
      state.pendingReports.push(settledReport(watch, target, reason));
    } else {
      state.watched.set(entry.session_id, watch);
    }
  }
  publishWatchStatus(state);
  ensureWatchTimer(state);
  return watchResult({ active: activeCount(state), added: additions.length });
}

function boundedText(content) {
  const parts = [];
  for (const block of content ?? []) {
    if (block?.type === "text" && typeof block.text === "string") parts.push(block.text);
    else if (block?.type === "image") parts.push("[image result omitted]");
  }
  let text = parts.join("\n").replace(/\0/g, "\uFFFD");
  const encoded = Buffer.from(text, "utf8");
  if (encoded.length <= OUTPUT_LIMIT) return { text, truncated: false };
  let cut = OUTPUT_LIMIT;
  while (cut > 0 && (encoded[cut] & 0xc0) === 0x80) cut -= 1;
  return { text: encoded.subarray(0, cut).toString("utf8"), truncated: true };
}

function exitCodeOf(code, signalName) {
  if (code !== null) return code;
  if (signalName) return 128 + (osConstants.signals[signalName] ?? 0);
  return 1;
}

export default function ovrcrOperatorExtension(pi) {
  const watches = watchState(pi);
  // toolCallId -> timeout seconds, recorded by the attached adapter so a
  // TimedOut status never depends on Pi's prose.
  const timedOutCalls = new Map();
  let activeBashCall;

  const operations = {
    exec(command, cwd, { onData, signal, timeout, env }) {
      if (typeof command !== "string") {
        return Promise.reject(new Error("invalid bash command"));
      }
      if (signal?.aborted) return Promise.reject(new Error("aborted"));
      if (
        timeout !== undefined &&
        (!Number.isFinite(timeout) || timeout <= 0 || timeout > MAX_TIMEOUT_SECONDS)
      ) {
        return Promise.reject(
          new Error(`timeout must be positive and at most ${MAX_TIMEOUT_SECONDS} seconds`),
        );
      }
      const callId = activeBashCall;
      return access(cwd, constants.F_OK)
        .catch(() => {
          throw new Error(
            `Working directory does not exist: ${cwd}\nCannot execute bash commands.`,
          );
        })
        .then(
          () =>
            new Promise((resolve, reject) => {
              // An abort while the cwd check was pending must not spawn anything.
              if (signal?.aborted) {
                reject(new Error("aborted"));
                return;
              }
              const child = spawn("/bin/bash", ["--noprofile", "--norc", "-c", command], {
                cwd,
                env,
                // Attached: the managed worker's Session owns Pi and this child's
                // whole tree. No new process group and no detached anchor.
                detached: false,
                stdio: ["ignore", "pipe", "pipe"],
                windowsHide: true,
              });
              let settled = false;
              let exited = false;
              let exitCode = null;
              let signalName = null;
              let timedOut = false;
              let aborted = false;
              let idleTimer;
              let timeoutTimer;

              const clearDrain = () => clearTimeout(idleTimer);
              const settle = (failure) => {
                if (settled) return;
                settled = true;
                clearDrain();
                clearTimeout(timeoutTimer);
                signal?.removeEventListener("abort", onAbort);
                child.stdout?.removeListener("data", output);
                child.stderr?.removeListener("data", output);
                // As with Pi's native backend, output from a completed
                // background child neither keeps this tool open nor streams.
                child.stdout?.resume();
                child.stderr?.resume();
                // Pi's native Bash contract: rejection messages `timeout:<s>`
                // and `aborted` are translated into the tool's error result.
                if (failure) reject(failure);
                else if (timedOut) reject(new Error(`timeout:${timeout}`));
                else if (aborted) reject(new Error("aborted"));
                else resolve({ exitCode: exitCodeOf(exitCode, signalName) });
              };
              // Bounded trailing drain: every chunk re-arms a short idle window,
              // so a completed background child holding the pipe cannot hang us.
              const armDrain = () => {
                clearDrain();
                idleTimer = setTimeout(() => settle(undefined), DRAIN_GRACE_MS);
              };
              const output = (bytes) => {
                if (settled) return;
                try {
                  onData(bytes);
                } catch (error) {
                  settle(error instanceof Error ? error : new Error(String(error)));
                  return;
                }
                if (exited) armDrain();
              };
              const onAbort = () => {
                aborted = true;
                try {
                  child.kill("SIGKILL");
                } catch (error) {
                  settle(error instanceof Error ? error : new Error(String(error)));
                }
              };
              child.once("error", (error) => {
                settle(error);
              });
              child.once("exit", (code, signal) => {
                exited = true;
                exitCode = code;
                signalName = signal;
                armDrain();
              });
              child.stdout?.on("data", output);
              child.stderr?.on("data", output);
              if (signal) {
                // An AbortSignal does not replay, so check the current state at
                // the moment the listener is attached, as Pi's native Bash does.
                if (signal.aborted) onAbort();
                else signal.addEventListener("abort", onAbort, { once: true });
              }
              if (timeout !== undefined) {
                timeoutTimer = setTimeout(() => {
                  timedOut = true;
                  if (callId) timedOutCalls.set(callId, timeout);
                  try {
                    child.kill("SIGKILL");
                  } catch {
                    // Rejected below regardless of the kill outcome.
                  }
                  // Settle at the deadline itself. An inherited background pipe
                  // that keeps writing must never extend the timeout or hold
                  // this promise open past the requested limit.
                  settle(undefined);
                }, timeout * 1000);
              }
              // A completed child always settles through exit + the idle drain,
              // so a completed background child holding the pipe cannot hang.
            }),
        );
    },
  };

  pi.on("tool_call", async (event, ctx) => {
    activeBashCall = event.toolName === "bash" ? event.toolCallId : undefined;
    const confirmed = await ctx.ui.confirm(
      ADMISSION_TITLE,
      JSON.stringify({
        toolCallId: event.toolCallId,
        toolName: event.toolName,
        input: event.input,
      }),
      { signal: ctx.signal },
    );
    if (!confirmed) {
      activeBashCall = undefined;
      return { block: true, terminate: true, reason: "OVRCR tool admission refused" };
    }
    return undefined;
  });

  pi.on("tool_result", async (event, ctx) => {
    const timedOut = timedOutCalls.has(event.toolCallId);
    timedOutCalls.delete(event.toolCallId);
    const native = boundedText(event.content);
    const truncated = native.truncated || event.details?.truncation?.truncated === true;
    const status = event.isError ? (timedOut ? "timedout" : "failed") : "succeeded";
    const confirmed = await ctx.ui.confirm(
      RESULT_TITLE,
      JSON.stringify({
        toolCallId: event.toolCallId,
        output: native.text,
        truncated,
        status,
      }),
      { signal: ctx.signal },
    );
    if (!confirmed) {
      // The durable journal refused this result; never hand control back to
      // another model request. Returning no decision keeps the hook from
      // feeding an uncertain outcome into the agent loop.
      ctx.abort();
      return undefined;
    }
    if (status === "timedout") {
      // A timeout is never fed back into another model request, even if a peer
      // acknowledged the receipt: abort and return no decision.
      ctx.abort();
    }
    return undefined;
  });

  pi.on("session_start", (_event, ctx) => {
    watches.ctx = ctx;
    watches.suspended = false;
    // Synchronous contract notification: the worker requires this exact status
    // key and payload before it sends a prompt, and falls back to stock bash
    // (detached) otherwise.
    ctx.ui.setStatus(
      "ovrcr_operator_contract",
      JSON.stringify({
        version: 1,
        sessionId: ctx.sessionManager.getSessionId(),
        tools: pi.getActiveTools(),
      }),
    );
    publishWatchStatus(watches);
    ensureWatchTimer(watches);
  });

  pi.on("agent_settled", (_event, ctx) => {
    if (ctx) watches.ctx = ctx;
    void flushPendingReports(watches);
  });

  pi.on("agent_start", () => {
    const batch = watches.dispatchBatch;
    if (batch) {
      const consumed = new Set(batch);
      watches.pendingReports = watches.pendingReports.filter((report) => !consumed.has(report));
      finishDispatch(watches, batch);
      publishWatchStatus(watches);
    }
    stopWatchTimerIfEmpty(watches);
  });

  pi.on("session_shutdown", (event) => {
    watches.suspended = true;
    if (event.reason !== "resume") {
      if (watches.timer) clearInterval(watches.timer);
      watches.timer = null;
      watches.watched.clear();
      watches.pendingReports = [];
      watches.dispatchBatch = null;
      watches.dispatching = false;
      watches.ctx = null;
      delete globalThis[WATCH_STATE];
    }
  });

  // Registered at load time so Pi's tool registry already contains the attached
  // Bash override when the session starts and the contract reports it.
  //
  // Sequential execution is required, not a preference. Pi 0.87.1's agent loop
  // defaults to `toolExecution: "parallel"`, and its parallel executor prepares
  // EVERY tool call (including the awaited `tool_call` admission hook) before
  // executing ANY of them. OVRCR's durable admission is serial, so a message with
  // two calls would fail at the second admission even though Pi is behaving
  // correctly. The agent loop runs the whole batch sequentially as soon as one
  // call in it names a tool whose definition is marked `executionMode:
  // "sequential"` (dist/agent-loop.js: `hasSequentialToolCall`), so every native
  // tool is registered here with that mark. The read/edit/write definitions are
  // Pi's own factories, unchanged except for the execution mode; the dedicated
  // profile is the only source of tool configuration.
  const cwd = process.cwd();
  const definitions = [
    createReadToolDefinition(cwd),
    createBashToolDefinition(cwd, {
      operations,
      exposeSessionEnvironment: false,
    }),
    createEditToolDefinition(cwd),
    createWriteToolDefinition(cwd),
    {
      name: "watch_sessions",
      label: "Watch sessions",
      description:
        "Watch reported OVRCR Workspace Agent sessions, list active watches, or remove watches.",
      parameters: {
        type: "object",
        additionalProperties: false,
        required: ["action"],
        properties: {
          action: { type: "string", enum: ["add", "list", "remove"] },
          sessions: {
            type: "array",
            items: {
              type: "object",
              additionalProperties: false,
              required: ["session_id"],
              properties: {
                session_id: { type: "integer", minimum: 1 },
                project: { type: "string", minLength: 1, maxLength: 256 },
                label: { type: "string", minLength: 1, maxLength: 256 },
                baseline: {
                  type: "object",
                  additionalProperties: false,
                  required: ["run", "activity_revision", "unread_turn"],
                  properties: {
                    run: { type: "integer", minimum: 1 },
                    activity_revision: { type: "integer", minimum: 0 },
                    unread_turn: { type: ["string", "null"], maxLength: 256 },
                  },
                },
              },
            },
          },
          session_ids: {
            type: "array",
            items: { type: "integer", minimum: 1 },
          },
          all: { type: "boolean" },
        },
      },
      execute(_toolCallId, input, _signal, _onUpdate, ctx) {
        return watchSessions(watches, input, ctx);
      },
    },
  ];
  for (const definition of definitions) {
    pi.registerTool({ ...definition, executionMode: "sequential" });
  }
}
