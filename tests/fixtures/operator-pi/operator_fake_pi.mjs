#!/usr/bin/env node
/**
 * Deterministic fake `pi` for OVRCR's Operator RPC tests.
 *
 * It speaks the framing and command/event vocabulary of pi 0.87.1 RPC mode but
 * never loads an extension, contacts a provider, or writes outside the attempt
 * directory it is given. A scenario file (JSON) scripts every observable step:
 *
 *   startup          "ok" | "exit"        exit before answering anything
 *   stderr_startup   text written to stderr at startup (credential sentinels)
 *   startup_records  raw strings written verbatim before the first command
 *   contract         "ok" | "wrong_session" | "wrong_tools" | "invalid"
 *   contract_delay_ms
 *   models           [{provider,id,name}] for get_available_models
 *   default_model    {provider,id} | null (the profile's startup default)
 *   thinking_level   string | null
 *   switch           "ok" | "cancel" | "wrong_file" | "no_ready"
 *   prompt_response  "ok" | "error" | "hang"
 *   clear_queue_result  "ok" | "error"
 *   abort_result     "ok" | "error"
 *   abort_records    raw strings written verbatim after the abort reply
 *   close_exit_code  exit status after stdin closes (default 0)
 *   hang_command     command type that is never answered
 *   hang_flood       keep stdout full of harmless notifications while it hangs
 *   steps            events/dialogs executed after `prompt` is accepted
 *   exit_after_steps exit once the step list completes
 *   log              append-only JSONL record path
 *                     (default $PI_CODING_AGENT_DIR/fake-pi.log)
 *
 * The log is the fixture's receipt: commands, prompt text, admission/result
 * acknowledgements, executed side effects and switches. Tests assert against it.
 */
import { closeSync, existsSync, fsyncSync, mkdirSync, openSync, readFileSync, writeSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";

const argv = process.argv.slice(2);
const arg = (name) => {
  const index = argv.indexOf(name);
  return index >= 0 ? argv[index + 1] : undefined;
};
const sessionDir = arg("--session-dir");
const requestedSession = arg("--session-id") ?? "fake-session";

function scenarioPath() {
  if (process.env.FAKE_PI_SCENARIO) return process.env.FAKE_PI_SCENARIO;
  const agent = process.env.PI_CODING_AGENT_DIR;
  return agent ? join(agent, "fake-pi-scenario.json") : undefined;
}

function loadScenario() {
  const path = scenarioPath();
  if (!path || !existsSync(path)) return {};
  return JSON.parse(readFileSync(path, "utf8"));
}

const scenario = loadScenario();
const logPath =
  scenario.log ?? (process.env.PI_CODING_AGENT_DIR ? join(process.env.PI_CODING_AGENT_DIR, "fake-pi.log") : undefined);

function record(event) {
  if (!logPath) return;
  appendLine(logPath, JSON.stringify({ at: Date.now(), ...event }));
}

function appendLine(path, line) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const fd = openSync(path, "a", 0o600);
  try {
    writeSync(fd, line + "\n");
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

function writeRecord(value) {
  process.stdout.write(JSON.stringify(value) + "\n");
}

function writeRaw(text) {
  process.stdout.write(text);
}

const tools = ["read", "bash", "edit", "write"];
const expectedSession = requestedSession;
let sessionFile = sessionDir ? join(sessionDir, `${requestedSession}.jsonl`) : undefined;
let currentModel = scenario.default_model ?? null;
let thinkingLevel = scenario.thinking_level ?? null;
const thinkingLevels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
function availableThinkingLevels() {
  return scenario.model_thinking_levels?.[`${currentModel?.provider}/${currentModel?.id}`]
    ?? scenario.available_thinking_levels ?? thinkingLevels.slice(0, 5);
}
function clampThinkingLevel(level) {
  const available = availableThinkingLevels();
  const index = thinkingLevels.indexOf(level);
  return thinkingLevels.slice(Math.max(0, index)).find((item) => available.includes(item))
    ?? thinkingLevels.slice(0, index).reverse().find((item) => available.includes(item));
}
let promptRunning = false;
let aborted = false;
let exited = false;
let stdinClosed = false;

// ---------------------------------------------------------------- session file

function isoNow() {
  return new Date().toISOString();
}

function writeHeader(path) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const header = JSON.stringify({
    type: "session",
    version: 3,
    id: expectedSession,
    timestamp: isoNow(),
    cwd: process.cwd(),
  });
  const fd = openSync(path, "w", 0o600);
  try {
    writeSync(fd, header + "\n");
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

function appendEntry(path, entry) {
  const fd = openSync(path, "a", 0o600);
  try {
    writeSync(fd, JSON.stringify(entry) + "\n");
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

if (sessionFile) writeHeader(sessionFile);

function appendSessionEntry(entry) {
  if (!sessionFile) return;
  appendEntry(sessionFile, { id: `e${Date.now()}${Math.random().toString(16).slice(2)}`, parentId: null, timestamp: isoNow(), ...entry });
}

// Pi records the initial model/thinking so a resumed session can restore them.
if (sessionFile) {
  if (currentModel) {
    appendSessionEntry({ type: "model_change", provider: currentModel.provider, modelId: currentModel.id });
  }
  if (thinkingLevel) appendSessionEntry({ type: "thinking_level_change", thinkingLevel });
}

record({
  event: "start",
  argv,
  sessionId: expectedSession,
  sessionDir: sessionDir ?? null,
  cwd: process.cwd(),
  piCodingAgentDir: process.env.PI_CODING_AGENT_DIR ?? null,
});

if (scenario.stderr_startup) process.stderr.write(scenario.stderr_startup + "\n");

function finish(code) {
  if (exited) return;
  exited = true;
  record({ event: "exit", code });
  process.exit(code);
}

if (scenario.startup === "exit") {
  // A failed native start before any command is answered.
  writeRaw("");
  finish(3);
}

for (const raw of scenario.startup_records ?? []) writeRaw(raw);

// ---------------------------------------------------------------- contract

function contractPayload() {
  const sessionId = scenario.contract === "wrong_session" ? "operator-999" : expectedSession;
  const listed = scenario.contract === "wrong_tools" ? ["read", "bash", "edit"] : tools;
  if (scenario.contract === "invalid") return "not-json";
  return JSON.stringify({ version: 1, sessionId, tools: listed });
}

function emitContract() {
  if (scenario.contract === "none") return;
  if (scenario.contract_delay_ms) {
    setTimeout(emitContractNow, scenario.contract_delay_ms);
    return;
  }
  emitContractNow();
}

function emitContractNow() {
  record({ event: "contract", mode: scenario.contract ?? "ok" });
  writeRecord({
    type: "extension_ui_request",
    id: `status-${Math.random().toString(16).slice(2)}`,
    method: "setStatus",
    statusKey: "ovrcr_operator_contract",
    statusText: contractPayload(),
  });
}

// ---------------------------------------------------------------- dialogs

const pendingDialogs = new Map();
let dialogSequence = 0;

function askConfirm(title, message) {
  const id = `dialog-${++dialogSequence}`;
  writeRecord({ type: "extension_ui_request", id, method: "confirm", title, message: JSON.stringify(message) });
  return new Promise((resolvePromise) => {
    pendingDialogs.set(id, resolvePromise);
    // A dialog can be cancelled by the worker; do not leak forever.
    setTimeout(() => {
      if (pendingDialogs.delete(id)) resolvePromise({ cancelled: true });
    }, 30_000).unref?.();
  });
}

function resolveDialog(id, response) {
  const pending = pendingDialogs.get(id);
  if (!pending) return;
  pendingDialogs.delete(id);
  pending(response);
}

// ---------------------------------------------------------------- commands

function respond(command, id, data, success = true) {
  const response = { type: "response", id, command, success };
  if (!success) response.error = "fixture refusal";
  else if (data !== undefined) response.data = data;
  writeRecord(response);
}

function stateData() {
  return {
    model: currentModel ? { provider: currentModel.provider, id: currentModel.id, name: currentModel.name ?? currentModel.id } : null,
    thinkingLevel,
    isStreaming: promptRunning,
    isCompacting: false,
    steeringMode: "all",
    followUpMode: "one-at-a-time",
    sessionFile: sessionFile ?? null,
    sessionId: expectedSession,
    autoCompactionEnabled: true,
    messageCount: 1,
    pendingMessageCount: 0,
  };
}

function sleep(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

async function waitForFile(path, timeoutMs = 20_000) {
  const deadline = Date.now() + timeoutMs;
  while (!existsSync(path)) {
    if (aborted || stdinClosed || Date.now() >= deadline) return false;
    await sleep(25);
  }
  return true;
}

function assistantMessage(text, stop, toolCalls) {
  const content = [];
  if (text !== undefined && text !== null) content.push({ type: "text", text });
  for (const call of toolCalls ?? []) {
    content.push({ type: "toolCall", id: call.id, name: call.name, arguments: call.input });
  }
  return { role: "assistant", content, stopReason: stop, timestamp: Date.now() };
}

function emitStart() {
  writeRecord({ type: "message_start", message: { role: "assistant", content: [], stopReason: "pending", timestamp: Date.now() } });
}

function emitDelta(delta) {
  emitTextDelta(0, delta);
}

function emitTextDelta(index, delta) {
  writeRecord({ type: "message_update", assistantMessageEvent: { type: "text_delta", contentIndex: index, delta } });
}

function emitThinkingStart(index) {
  writeRecord({ type: "message_update", assistantMessageEvent: { type: "thinking_start", contentIndex: index } });
}

function emitThinkingDelta(index, delta) {
  writeRecord({ type: "message_update", assistantMessageEvent: { type: "thinking_delta", contentIndex: index, delta } });
}

function emitEnd(text, stop, toolCalls) {
  writeRecord({ type: "message_end", message: assistantMessage(text, stop, toolCalls) });
}

function emitEndThinking(thinking, text, stop) {
  const content = [{ type: "thinking", thinking }];
  if (text !== undefined && text !== null && text !== "") content.push({ type: "text", text });
  writeRecord({
    type: "message_end",
    message: { role: "assistant", content, stopReason: stop, timestamp: Date.now() },
  });
}

/** One admission dialog plus (when granted) one durable result dialog. */
async function runTool(step, execute) {
  emitStart();
  const call = { id: step.id, name: step.name, input: step.input ?? {} };
  emitEnd(step.text ?? undefined, "toolUse", [call]);
  writeRecord({
    type: "tool_execution_start",
    toolCallId: call.id,
    toolName: call.name,
    args: call.input,
  });
  const admission = await askConfirm("OVRCR Operator tool admission", {
    toolCallId: call.id,
    toolName: call.name,
    input: call.input,
  });
  const granted = admission?.confirmed === true;
  record({ event: "admission", id: call.id, confirmed: granted, cancelled: admission?.cancelled === true });
  if (!granted) {
    writeRecord({
      type: "tool_execution_end",
      toolCallId: call.id,
      toolName: call.name,
      result: { content: [{ type: "text", text: "blocked" }] },
      isError: true,
    });
    return false;
  }
  if (execute) {
    record({ event: "executed", id: call.id, marker: step.marker ?? null });
    if (step.marker) appendLine(step.marker, `executed ${call.id}`);
  }
  const result = await askConfirm("OVRCR Operator tool result", {
    toolCallId: call.id,
    output: step.output ?? "",
    truncated: step.truncated === true,
    status: step.status ?? "succeeded",
  });
  record({ event: "result_ack", id: call.id, confirmed: result?.confirmed === true, cancelled: result?.cancelled === true });
  writeRecord({
    type: "tool_execution_end",
    toolCallId: call.id,
    toolName: call.name,
    result: { content: [{ type: "text", text: step.output ?? "" }] },
    isError: (step.status ?? "succeeded") !== "succeeded",
  });
  return result?.confirmed === true;
}

async function runSteps() {
  let finalText;
  for (const step of scenario.steps ?? []) {
    if (aborted || exited) return;
    if (step.kind === "assistant" || step.kind === "message_end" || step.kind === "thinking_message_end") {
      finalText = step.text;
    }
    switch (step.kind) {
      case "delta":
        emitDelta(step.text);
        break;
      case "text_delta":
        emitTextDelta(step.index ?? 0, step.text ?? "");
        break;
      case "thinking_start":
        emitThinkingStart(step.index ?? 0);
        break;
      case "thinking_delta":
        emitThinkingDelta(step.index ?? 0, step.text ?? "");
        break;
      case "thinking_end":
        writeRecord({
          type: "message_update",
          assistantMessageEvent: {
            type: "thinking_end",
            contentIndex: step.index ?? 0,
            content: step.content ?? "",
          },
        });
        break;
      case "thinking_message_end":
        emitEndThinking(step.thinking ?? "", step.text, step.stop ?? "stop");
        break;
      case "start":
        emitStart();
        break;
      case "message_end":
        emitEnd(step.text, step.stop ?? "stop");
        break;
      case "assistant":
        emitStart();
        emitDelta(step.text);
        emitEnd(step.text, step.stop ?? "stop");
        break;
      case "assistant_partial": {
        emitStart();
        if (step.streamed) emitDelta(step.streamed);
        emitEnd(step.text, step.stop ?? "stop");
        break;
      }
      case "assistant_contradiction":
        emitStart();
        emitDelta(step.streamed ?? "");
        emitEnd(step.text, step.stop ?? "stop");
        break;
      case "tool":
        if (!(await runTool(step, true))) return;
        break;
      case "unadmitted_execution": {
        // Pi refused the call before beforeToolCall (unknown tool or invalid
        // arguments), so no admission dialog opens and the error result is
        // immediate. A durable-admission peer must fail the turn here.
        emitStart();
        const call = { id: step.id, name: step.name, input: step.input ?? {} };
        emitEnd(undefined, "toolUse", [call]);
        writeRecord({
          type: "tool_execution_start",
          toolCallId: call.id,
          toolName: call.name,
          args: call.input,
        });
        writeRecord({
          type: "tool_execution_end",
          toolCallId: call.id,
          toolName: call.name,
          result: { content: [{ type: "text", text: step.output ?? "refused before admission" }] },
          isError: true,
        });
        break;
      }
      case "model_request":
        // Receipt for "Pi asked the provider again": a timed-out result must
        // never let the scripted peer reach this step.
        record({ event: "model_request", text: step.text ?? null });
        break;
      case "two_call_batch": {
        // One assistant message with two tool calls, executed one at a time with
        // a durable admission and result for each before the next begins.
        const first = { id: step.id, name: step.name, input: step.input ?? {} };
        const second = step.second;
        emitStart();
        emitEnd(undefined, "toolUse", [first, { id: second.id, name: second.name, input: second.input ?? {} }]);
        for (const [call, output] of [
          [first, step.output ?? ""],
          [{ id: second.id, name: second.name, input: second.input ?? {} }, second.output ?? ""],
        ]) {
          writeRecord({ type: "tool_execution_start", toolCallId: call.id, toolName: call.name, args: call.input });
          const admission = await askConfirm("OVRCR Operator tool admission", {
            toolCallId: call.id,
            toolName: call.name,
            input: call.input,
          });
          record({ event: "admission", id: call.id, confirmed: admission?.confirmed === true });
          if (admission?.confirmed !== true) return;
          record({ event: "executed", id: call.id });
          const result = await askConfirm("OVRCR Operator tool result", {
            toolCallId: call.id,
            output,
            truncated: false,
            status: "succeeded",
          });
          record({ event: "result_ack", id: call.id, confirmed: result?.confirmed === true });
          writeRecord({
            type: "tool_execution_end",
            toolCallId: call.id,
            toolName: call.name,
            result: { content: [{ type: "text", text: output }] },
            isError: false,
          });
          if (result?.confirmed !== true) return;
        }
        break;
      }
      case "tool_then_second_admission": {
        // The worker must not grant a second tool before the first result is
        // durable, so this admission is refused and its marker never executes.
        emitStart();
        const first = { id: step.id, name: step.name, input: step.input ?? {} };
        const second = step.second;
        emitEnd(undefined, "toolUse", [first, { id: second.id, name: second.name, input: second.input ?? {} }]);
        const admission = await askConfirm("OVRCR Operator tool admission", {
          toolCallId: first.id,
          toolName: first.name,
          input: first.input,
        });
        record({ event: "admission", id: first.id, confirmed: admission?.confirmed === true });
        if (admission?.confirmed !== true) return;
        record({ event: "executed", id: first.id, marker: step.marker ?? null });
        if (step.marker) appendLine(step.marker, `executed ${first.id}`);
        // Second admission arrives while the first result is unsettled.
        writeRecord({
          type: "tool_execution_start",
          toolCallId: second.id,
          toolName: second.name,
          args: second.input ?? {},
        });
        const early = await askConfirm("OVRCR Operator tool admission", {
          toolCallId: second.id,
          toolName: second.name,
          input: second.input ?? {},
        });
        record({ event: "admission", id: second.id, confirmed: early?.confirmed === true });
        if (early?.confirmed === true && second.marker) {
          appendLine(second.marker, "SECOND-TOOL-RAN-WITHOUT-DURABLE-ADMISSION\n");
        }
        const result = await askConfirm("OVRCR Operator tool result", {
          toolCallId: first.id,
          output: step.output ?? "",
          truncated: false,
          status: step.status ?? "succeeded",
        });
        record({ event: "result_ack", id: first.id, confirmed: result?.confirmed === true });
        break;
      }
      case "result_for_unknown_call": {
        // A result whose call was never admitted must be refused, and its marker
        // (the command it would have run) must never execute.
        const result = await askConfirm("OVRCR Operator tool result", {
          toolCallId: step.id,
          output: step.output ?? "",
          truncated: false,
          status: "succeeded",
        });
        record({ event: "result_ack", id: step.id, confirmed: result?.confirmed === true });
        if (result?.confirmed === true && step.marker) appendLine(step.marker, "UNKNOWN-CALL-RAN\n");
        break;
      }
      case "event":
        writeRecord(step.event);
        break;
      case "extension_error":
        writeRecord({ type: "extension_error", message: step.message ?? "fixture" });
        break;
      case "stderr":
        process.stderr.write(step.text + "\n");
        break;
      case "sleep":
        await sleep(step.ms ?? 10);
        break;
      case "wait_file":
        if (!(await waitForFile(step.path))) return;
        break;
      case "settle":
        writeRecord({ type: "agent_settled" });
        break;
      case "exit":
        finish(step.code ?? 0);
        return;
      case "hold":
        promptRunning = true;
        while (!aborted && !stdinClosed) await sleep(25);
        return;
      default:
        throw new Error(`unknown fixture step ${step.kind}`);
    }
  }
  promptRunning = false;
  if (scenario.exit_after_steps) finish(0);
  return finalText;
}

function appendPromptHistory(message) {
  if (!sessionFile) return;
  appendEntry(sessionFile, {
    type: "message",
    id: `u${Date.now()}`,
    parentId: null,
    timestamp: isoNow(),
    message: { role: "user", content: message, timestamp: Date.now() },
  });
}

function appendAssistantHistory(text, stop) {
  if (!sessionFile || text === undefined) return;
  appendEntry(sessionFile, {
    type: "message",
    id: `a${Date.now()}`,
    parentId: null,
    timestamp: isoNow(),
    message: { role: "assistant", content: [{ type: "text", text }], stopReason: stop ?? "stop", timestamp: Date.now() },
  });
}

function restoreSessionState(path) {
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch {
    return;
  }
  let thinking = null;
  let model = null;
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    let entry;
    try {
      entry = JSON.parse(line);
    } catch {
      // Pi's session manager skips malformed lines; so does this restore.
      continue;
    }
    if (!entry || typeof entry !== "object") continue;
    if (entry.type === "thinking_level_change" && typeof entry.thinkingLevel === "string") {
      thinking = entry.thinkingLevel;
    } else if (entry.type === "model_change" && typeof entry.provider === "string" && typeof entry.modelId === "string") {
      model = { provider: entry.provider, id: entry.modelId, name: entry.modelId };
    } else if (
      entry.type === "message" &&
      entry.message?.role === "assistant" &&
      typeof entry.message.provider === "string" &&
      typeof entry.message.model === "string"
    ) {
      model = { provider: entry.message.provider, id: entry.message.model, name: entry.message.model };
    }
  }
  if (model) currentModel = model;
  if (thinking) thinkingLevel = thinking;
}

function handleCommand(command) {
  const id = command.id;
  if (scenario.hang_command === command.type) {
    record({ event: "command", command: command.type, id, hung: true });
    if (scenario.hang_flood) {
      record({ event: "hang_flood" });
      // Keep stdout continuously full with complete frames. A consumer that
      // only checks its deadline on an idle poll therefore never observes one;
      // the write blocks when the pipe is full and resumes as it is read.
      const frame =
        JSON.stringify({ type: "notify", message: "harmless fixture notification" }) + "\n";
      for (;;) {
        try {
          writeSync(1, frame);
        } catch (error) {
          if (error?.code !== "EAGAIN") break;
        }
      }
    }
    return;
  }
  switch (command.type) {
    case "get_state":
      record({ event: "command", command: "get_state", id });
      writeRecord({ type: "response", id, command: "get_state", success: true, data: stateData() });
      break;
    case "get_available_models":
      record({ event: "command", command: "get_available_models", id });
      respond("get_available_models", id, { models: scenario.models ?? [] });
      break;
    case "set_model": {
      const models = scenario.models ?? [];
      const match = models.find((model) => model.provider === command.provider && model.id === command.modelId);
      record({ event: "command", command: "set_model", id, provider: command.provider, modelId: command.modelId, found: Boolean(match) });
      if (!match) {
        respond("set_model", id, undefined, false);
        break;
      }
      currentModel = match;
      thinkingLevel = clampThinkingLevel(scenario.model_default_thinking?.[`${match.provider}/${match.id}`]
        ?? scenario.thinking_level ?? thinkingLevel);
      appendSessionEntry({ type: "model_change", provider: match.provider, modelId: match.id });
      respond("set_model", id, { provider: match.provider, id: match.id, name: match.name ?? match.id });
      break;
    }
    case "get_available_thinking_levels":
      record({ event: "command", command: command.type, id });
      respond(command.type, id, { levels: availableThinkingLevels() });
      break;
    case "set_thinking_level":
      record({ event: "command", command: "set_thinking_level", id, level: command.level });
      thinkingLevel = clampThinkingLevel(command.level);
      appendSessionEntry({ type: "thinking_level_change", thinkingLevel });
      respond("set_thinking_level", id);
      break;
    case "set_auto_retry":
    case "set_auto_compaction":
      record({ event: "command", command: command.type, id, enabled: command.enabled });
      respond(command.type, id);
      break;
    case "switch_session": {
      const path = command.sessionPath;
      const exists = typeof path === "string" && existsSync(path);
      record({ event: "switch", path, exists, mode: scenario.switch ?? "ok" });
      if ((scenario.switch ?? "ok") === "cancel") {
        respond("switch_session", id, { cancelled: true });
        break;
      }
      if (!exists || (scenario.switch ?? "ok") === "wrong_file") {
        respond("switch_session", id, { cancelled: false });
        sessionFile = resolve(path ?? ".");
        break;
      }
      sessionFile = resolve(path);
      // A resumed native session restores its own model/thinking; the worker
      // must still resolve the fresh profile default captured before the switch.
      restoreSessionState(sessionFile);
      respond("switch_session", id, { cancelled: false });
      if ((scenario.switch ?? "ok") !== "no_ready") emitContract();
      break;
    }
    case "prompt":
      record({ event: "prompt", id, message: command.message });
      if (scenario.prompt_response === "error") {
        respond("prompt", id, undefined, false);
        break;
      }
      if (scenario.prompt_response === "hang") break;
      respond("prompt", id);
      promptRunning = true;
      appendPromptHistory(command.message);
      setTimeout(() => {
        runSteps()
          .then((text) => {
            appendAssistantHistory(text, "stop");
            promptRunning = false;
          })
          .catch((error) => {
            process.stderr.write(`fixture step failure: ${error?.message ?? error}\n`);
            finish(4);
          });
      }, 0);
      break;
    case "clear_queue":
      record({ event: "command", command: "clear_queue", id });
      respond("clear_queue", id, { steering: [], followUp: [] }, scenario.clear_queue_result !== "error");
      break;
    case "abort":
      record({ event: "command", command: "abort", id });
      if (scenario.abort_result === "error") {
        respond("abort", id, undefined, false);
        break;
      }
      aborted = true;
      respond("abort", id);
      for (const raw of scenario.abort_records ?? []) writeRaw(raw);
      break;
    case "extension_ui_response":
      resolveDialog(command.id, command);
      break;
    default:
      record({ event: "unknown_command", type: command.type, id });
      respond(command.type ?? "unknown", id, undefined, false);
      break;
  }
}

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
lines.on("line", (line) => {
  if (!line.trim()) return;
  let command;
  try {
    command = JSON.parse(line);
  } catch {
    record({ event: "malformed_stdin", line });
    return;
  }
  if (command.type === "extension_ui_response") {
    resolveDialog(command.id, command);
    return;
  }
  handleCommand(command);
});
lines.on("close", () => {
  stdinClosed = true;
  record({ event: "stdin_closed", aborted });
  // Pi exits after its client closes stdin; flush queued records first.
  process.stdout.write("", () => finish(scenario.close_exit_code ?? 0));
});

// Startup records are emitted before any command; the contract comes after the
// first get_state so the fresh state is authoritative before binding.
emitContract();
