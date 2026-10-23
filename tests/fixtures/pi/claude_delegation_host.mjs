// #328 deterministic native-model fixture. Root hooks,
// statusline reports, publication helper, Server and PTY remain production code.
// A matching fixture statusline is not installed-Claude availability evidence.
import { createInterface } from "node:readline";
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { readFileSync, writeFileSync, appendFileSync, existsSync, mkdirSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { isDeepStrictEqual } from "node:util";

const args = process.argv.slice(2);
const profile = process.env.CLAUDE_CONFIG_DIR ?? join(process.env.HOME, ".claude");
const config = name => join(profile, name);
const mode = () => existsSync(config("delegation-mode")) ? readFileSync(config("delegation-mode"), "utf8").trim() : "result";
const atomic = (path, value) => {
  const temporary = `${path}.${process.pid}.tmp`;
  writeFileSync(temporary, JSON.stringify(value), { mode: 0o600 });
  renameSync(temporary, path);
};
const option = name => args[args.indexOf(name) + 1];

// Native discovery and the existing version/auth probe owners construct
// process_group(0). These are source-derived PID/group claims, not live PS or
// birth observations. Never retain the returned account/version bodies here.
function recordProbeOwner(kind) {
  if (!["initialize", "version", "auth"].includes(kind)) throw new Error("unknown fixture probe kind");
  appendFileSync(config("delegation-discovery-owners.jsonl"), JSON.stringify({ pid: process.pid, pgid: process.pid, kind }) + "\n", { mode: 0o600 });
}

if (args.join(" ") === "--version") {
  recordProbeOwner("version");
  process.stdout.write("2.1.300 (Claude Code)\n");
  process.exit(0);
}
if (args.join(" ") === "auth status --json") {
  recordProbeOwner("auth");
  process.stdout.write(JSON.stringify({ loggedIn: true, authMethod: "claude.ai", subscriptionType: "max" }));
  process.exit(0);
}
if (args.includes("--input-format")) {
  recordProbeOwner("initialize");
  const countFile = config("delegation-discovery-count");
  const count = existsSync(countFile) ? Number(readFileSync(countFile, "utf8")) + 1 : 1;
  writeFileSync(countFile, String(count), { mode: 0o600 });
  for await (const line of createInterface({ input: process.stdin })) {
    const request = JSON.parse(line);
    if (request.type !== "control_request" || request.request?.subtype !== "initialize") throw new Error("discovery sent an unsupported or user frame");
    const resolved = alias => mode() === "resolution-mismatch" && count >= 3 ? "different-resolution" : `${alias}-resolved`;
    process.stdout.write(JSON.stringify({ type: "control_response", response: { subtype: "success", request_id: request.request_id,
      response: { pid: process.pid, claude_code_version: "2.1.300", models: ["default", "fixture-model"].map(value => ({ value, resolvedModel: resolved(value) })), pending_permission_requests: [], pending_control_requests: [] } } }) + "\n");
  }
  if (mode() === "discovery-running-after-eof" && count === 1) {
    atomic(config("delegation-discovery-eof.json"), { pid: process.pid });
    // Retain only this owned process, exercising production's bounded shutdown.
    await new Promise(resolve => setTimeout(resolve, 15000));
    atomic(config("delegation-discovery-natural-expiry.json"), { pid: process.pid });
  }
  process.exit(0);
}
if (!args.includes("--session-id")) throw new Error("managed admission did not supply a conversation UUID");
if (!args.includes("--permission-mode")) throw new Error("delegated launch omitted explicit native permission policy");
if (args.some(arg => ["--dangerously-skip-permissions", "--allow-dangerously-skip-permissions", "--bare"].includes(arg))) {
  throw new Error("delegation silently bypassed native permissions or hooks");
}
mkdirSync(profile, { recursive: true });
// Deliberately retain no environment/token values in evidence.
atomic(config("delegation-launch.json"), { argv: args, pid: process.pid, cwd: process.cwd() });
const settings = JSON.parse(readFileSync(args.includes("--settings") ? option("--settings") : config("settings.json"), "utf8"));
if (mode() === "policy-before-bind") writeFileSync(option("--mcp-config"), JSON.stringify({ mcpServers: {} }));
let conversation = option("--session-id");
const transcript = join(profile, "projects", "fixture", `${conversation}.jsonl`);
mkdirSync(dirname(transcript), { recursive: true });
if (mode() !== "source-not-created" && !mode().startsWith("startup-bootstrap")) writeFileSync(transcript, "", { mode: 0o600 });
let turn = 0;
let active;
let questionReference;

function hook(event, extra = {}) {
  const payload = { hook_event_name: event, session_id: conversation, transcript_path: transcript, cwd: process.cwd(), ...extra };
  const commands = (settings.hooks?.[event] ?? []).flatMap(group => group.hooks ?? []);
  const command = commands.find(item => item.type === "command" && item.command.includes("report claude --stdin-json"));
  if (!command) throw new Error(`missing production ${event} hook`);
  execFileSync("/bin/sh", ["-c", command.command], { input: JSON.stringify(payload), stdio: ["pipe", "pipe", "pipe"] });
}

function statusline() {
  if (["missing-model", "source-not-created"].includes(mode()) || mode().startsWith("startup-bootstrap")) return;
  const model = mode() === "wrong-model" ? "different-model" : `${option("--model")}-resolved`;
  const command = settings.statusLine?.command;
  if (!command?.includes("report claude-statusline --stdin-json")) throw new Error("missing production statusline command");
  execFileSync("/bin/sh", ["-c", command], {
    input: JSON.stringify({ session_id: conversation, model: { id: model } }),
    stdio: ["pipe", "pipe", "pipe"],
  });
}

// Speak the actual bounded MCP CLI protocol; no arbitrary parent request surface.
function mcpCall(name, arguments_, id = 2) {
  const path = option("--mcp-config");
  if (!args.includes("--mcp-config") || !path) throw new Error("missing production MCP configuration");
  const mcpConfig = JSON.parse(readFileSync(path, "utf8"));
  const server = mcpConfig.mcpServers?.ovrcr_child;
  if (!server || !server.command?.startsWith("/") || server.args?.length !== 3 || server.args?.[0] !== "delegation-publisher" || server.args?.[1] !== "--read-handle") {
    throw new Error("native fixture requires the exact production publisher command");
  }
  const frames = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "claude-fixture", version: "1" } } },
    { jsonrpc: "2.0", method: "notifications/initialized" },
    { jsonrpc: "2.0", id, method: "tools/call", params: { name, arguments: arguments_ } },
  ];
  try {
    const stdout = execFileSync(server.command, server.args, {
      input: frames.map(frame => JSON.stringify(frame)).join("\n") + "\n",
      encoding: "utf8", stdio: ["pipe", "pipe", "pipe"], timeout: 15000, maxBuffer: 131072,
    });
    const lines = stdout.trim().split("\n");
    const index = lines.findIndex(line => JSON.parse(line).id === id);
    const reply = index < 0 ? undefined : JSON.parse(lines[index]);
    if (mode() === "read-pages" && name === "ovrcr_read" && arguments_.target?.Workspace?.path === "escape-heavy") {
      atomic(config(`delegation-read-page-${arguments_.offset}.json`), {
        reply, encoded_bytes: index < 0 ? null : Buffer.byteLength(lines[index]),
      });
    }
    return reply?.result?.structuredContent ?? { status: "unknown" };
  } catch {
    return { status: "unknown" };
  }
}

function publish(kind, payload, handle = active.brief.handle) {
  const name = kind === "submission" ? "ovrcr_publish" : "ovrcr_publish_question";
  const field = kind === "submission" ? "submission" : "question";
  const outcome = mcpCall(name, { handle, [field]: payload });
  return outcome.status === "published" ? { details: outcome } : { error: outcome.status, details: outcome };
}
function readCurrent(request, id = 2) {
  let text = "";
  // A 512 KiB file needs at most 129 pages when a 4096-byte boundary backs up by 3 UTF-8 bytes.
  for (let pages = 0; pages < 129; pages++) {
    const outcome = mcpCall("ovrcr_read", request, id);
    if (outcome.status !== "read") throw new Error(`current scoped read refused: ${outcome.status}`);
    text += outcome.read.text;
    if (outcome.read.next_offset === outcome.read.total_bytes) return text;
    request = { ...request, offset: outcome.read.next_offset };
  }
  throw new Error("scoped read page bound exceeded");
}

function submission(brief) {
  return { schema: 1, identity: brief.identity, version: brief.result_version, outcome: "success", summary: "CLAUDE_RESULT_PRIVATE: Read the assigned README.", evidence: ["README:1 fixture"], unresolved: [] };
}

async function finish(path, brief) {
  const gate = process.env.OVRCR_TEST_DELEGATION_RESULT_GATE;
  const deadline = performance.now() + 60000;
  while (!existsSync(gate)) {
    if (performance.now() > deadline) throw new Error("owned result gate timed out");
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  const payload = submission(brief);
  if (mode() === "incomplete") { payload.outcome = "blocked"; payload.unresolved = ["Requested evidence is missing."]; }
  if (mode() === "stale-result") payload.identity = { ...payload.identity, contract: payload.identity.contract + 1 };
  if (mode() === "policy-before-publish") {
    writeFileSync(option("--settings"), "{}", { mode: 0o600 });
    atomic(`${path}.replaced-read`, mcpCall("ovrcr_read", { ...active.read, target: "Contract", offset: 0 }));
  }
  const reply = publish("submission", payload);
  if (mode() === "stale-result") reply.attempted_identity = payload.identity;
  atomic(`${path}.published`, reply);
  if (mode() !== "no-stop") hook("Stop", { prompt_id: `turn-${turn}`, stop_hook_active: false });
  process.stdout.write("CLAUDE_CHILD_RESULT_PUBLISHED\n");
}

if (mode() === "source-not-created") {
  // Pin the diagnostic test's uncaptured-input ordering, not a wall-clock race.
  const deadline = performance.now() + 60000;
  while (!existsSync(config("startup-source-release"))) {
    if (performance.now() > deadline) throw new Error("owned startup source gate timed out");
    await new Promise(resolve => setTimeout(resolve, 10));
  }
}
hook("SessionStart", { source: "startup", ...(mode().startsWith("startup-bootstrap") ? { model: mode() === "startup-bootstrap-wrong-model" ? "different-model" : `${option("--model")}-resolved` } : {}) });
if (mode() === "pre-input-prompt") {
  hook("UserPromptSubmit", { prompt_id: "native-startup" });
  hook("Notification", { prompt_id: "native-startup", notification_type: "permission_prompt" });
}
statusline();
process.stdout.write("CLAUDE_NATIVE_READY\n");
if (process.env.OVRCR_TEST_DELEGATION_BYTES) {
  const session = process.env.OVRCR_DELEGATION_SESSION;
  if (!/^[1-9][0-9]*$/.test(session ?? "")) throw new Error("native byte counter requires assigned session");
  process.stdin.setRawMode?.(true);
  process.stdin.on("data", bytes => appendFileSync(join(process.env.OVRCR_TEST_DELEGATION_BYTES, session), `${bytes.length}\n`));
}
for await (const line of createInterface({ input: process.stdin })) {
  const clean = line.replaceAll("\x1b[200~", "").replaceAll("\x1b[201~", "");
  const briefPath = clean.match(/in (.+\/brief-v[1-9][0-9]*\.json)\. Read/)?.[1];
  if (briefPath) {
    const read = JSON.parse(clean.match(/OVRCR_READ_ARGUMENTS:(\{.*\}) END_OVRCR_READ_ARGUMENTS/)?.[1] ?? "null");
    if (!read) throw new Error("managed input omitted exact read authority");
    const brief = JSON.parse(readCurrent(read));
    const workspaceRead = { ...read, target: { Workspace: { path: "README.md" } }, offset: 0 };
    atomic(`${briefPath}.read`, mcpCall("ovrcr_read", workspaceRead));
    if (mode() === "read-pages") {
      const text = readCurrent({ ...workspaceRead, target: { Workspace: { path: "escape-heavy" } } }, "\0".repeat(128));
      atomic(`${briefPath}.paged-read`, { text });
    }
    if (existsSync(config("delegation-read-cases.json"))) {
      const cases = JSON.parse(readFileSync(config("delegation-read-cases.json"), "utf8"));
      atomic(`${briefPath}.read-cases`, cases.map(delta => mcpCall("ovrcr_read", { ...workspaceRead, ...delta, identity: { ...workspaceRead.identity, ...(delta.identity ?? {}) } })));
    }
    active = { path: briefPath, brief, read };
    turn++;
    appendFileSync(`${briefPath}.input-count`, "1\n");
    atomic(`${briefPath}.started`, { conversation, turn, pid: process.pid });
    const createSource = () => {
      appendFileSync(transcript, JSON.stringify({ type: "assistant", sessionId: conversation, isSidechain: false, requestId: "bootstrap-fixture", message: { id: "bootstrap-fixture", usage: { input_tokens: 7, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: 1 } } }) + "\n", { mode: 0o600 });
      atomic(`${briefPath}.source-created`, { conversation });
    };
    if (mode() === "startup-bootstrap") createSource();
    hook("UserPromptSubmit", { prompt_id: `turn-${turn}` });
    if (mode() === "startup-bootstrap-after-prompt") {
      atomic(`${briefPath}.prompt-submitted`, { conversation, turn });
      const deadline = performance.now() + 10000;
      while (!existsSync(config("bootstrap-create-source"))) {
        if (performance.now() > deadline) throw new Error("source creation gate timed out");
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      createSource();
    }
    process.stdout.write("CLAUDE_CHILD_INPUT_CONSUMED\n");
    if (mode() === "native-prompt") {
      hook("Notification", { prompt_id: `turn-${turn}`, notification_type: "permission_prompt" });
      process.stdout.write("CLAUDE_FIXTURE_NATIVE_PERMISSION_PROMPT\n");
    } else if (mode() === "question") {
      const question = { schema: 1, identity: brief.identity, result_version: brief.result_version, question_id: "readme-choice", question_version: 1, supersedes: null, kind: "Clarification", question: "CLAUDE_QUESTION_PRIVATE: Which README section?", context: "Existing authority permits read-only research.", choices: ["Introduction", "Installation"] };
      const reply = publish("question", question);
      const publication = reply.details?.publication;
      const kind = publication?.kind?.Question;
      questionReference = publication && kind ? { assignment: publication.assignment, result_version: kind.result_version, question_id: kind.question_id, question_version: publication.version, digest: publication.digest } : undefined;
      atomic(`${briefPath}.question-published`, reply);
    } else if (mode() === "missing-result") {
      hook("Stop", { prompt_id: `turn-${turn}`, stop_hook_active: false });
      atomic(`${briefPath}.stopped`, { observed: true });
    } else if (mode() === "rebind") {
      hook("SessionEnd", { reason: "clear" });
      conversation = randomUUID();
      hook("SessionStart", { source: "clear" });
      atomic(`${briefPath}.rebound`, { conversation });
    } else {
      void finish(briefPath, brief).catch(error => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });
    }
  } else if (clean.startsWith("Read the scoped answer in ")) {
    const match = clean.match(/in (.+\/answer-[a-zA-Z0-9_-]+\.json) for question ([a-zA-Z0-9_-]+) version ([1-9][0-9]*)\./);
    if (!match || !active) throw new Error("invalid scoped answer instruction");
    const read = JSON.parse(clean.match(/OVRCR_READ_ARGUMENTS:(\{.*\}) END_OVRCR_READ_ARGUMENTS/)?.[1] ?? "null");
    if (!read) throw new Error("answer input omitted exact read authority");
    const answer = JSON.parse(readCurrent(read));
    if (!isDeepStrictEqual(answer.question, questionReference)) throw new Error("answer belongs to another question");
    appendFileSync(`${active.path}.answer-count`, "1\n");
    atomic(`${active.path}.answer-received`, answer);
    process.stdout.write("CLAUDE_SCOPED_ANSWER_CONSUMED\n");
  } else {
    throw new Error("unexpected native input; refusing fixture success");
  }
}
