/**
 * OVRCR lifecycle adapter for the dedicated Operator Pi profile.
 *
 * This is the only custom code loaded into that profile: Pi supplies the
 * read/bash/edit/write tool definitions, validation, truncation, inference and
 * context management. This file registers those same definitions with
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
import { spawn } from "node:child_process";
import { constants } from "node:fs";
import { access } from "node:fs/promises";
import { constants as osConstants } from "node:os";
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
  ];
  for (const definition of definitions) {
    pi.registerTool({ ...definition, executionMode: "sequential" });
  }
}
