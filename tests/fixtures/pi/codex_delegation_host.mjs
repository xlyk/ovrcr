#!/usr/bin/env node
// Private #329 draft. This is deterministic Codex-shaped work, not installed-provider proof.
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { appendFileSync, existsSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { isDeepStrictEqual } from "node:util";

const args = process.argv.slice(2);
if (args.includes("--version")) {
  console.log("codex-cli 0.160.1");
  process.exit(0);
}
if (args.includes("--help")) {
  console.log("Fixture only: codex --model MODEL --profile NAME --ask-for-approval POLICY --no-daemon --strict-config");
  process.exit(0);
}
// Mirror the installed public refresh-free catalog route; never infer entitlement.
if (args.includes("debug") && args.includes("models") && args.includes("--bundled")) {
  console.log(JSON.stringify({ models: [{ slug: "fixture-codex-model", visibility: "list" }] }));
  process.exit(0);
}
if (args.some(value => ["app-server", "daemon", "--list-models"].includes(value))) {
  process.stderr.write("CODEX_FIXTURE_UNSUPPORTED_DISCOVERY\n");
  process.exit(2);
}
const binary = process.env.OVRCR_TEST_CONTROL_BINARY;
const session = Number(process.env.OVRCR_DELEGATION_SESSION);
const capability = process.env.OVRCR_DELEGATION_TOKEN;
const socket = process.env.OVRCR_TEST_DELEGATION_CONTROL_SOCKET ?? process.env.OVRCR_DELEGATION_SOCKET;
const instance = process.env.OVRCR_DELEGATION_HOME;
if (!binary || !Number.isSafeInteger(session) || session <= 0 || !capability || !socket || !instance) {
  throw new Error("native child requires environment-derived scoped publication authority");
}
const root = join(process.env.OVRCR_TEST_CODEX_ROOT, String(session));
mkdirSync(root, { mode: 0o700, recursive: true });
let serial = 0;
function record(name, value) {
  const path = join(root, name);
  const temporary = `${path}.${process.pid}.${serial++}.tmp`;
  writeFileSync(temporary, JSON.stringify(value), { mode: 0o600 });
  renameSync(temporary, path);
}
let conversation = randomUUID();
const originalConversation = conversation;
const nativeModel = process.env.OVRCR_TEST_CODEX_REPORTED_MODEL;
let turn;
let brief;
let question;
let lastSubmission;
function hook(event, extra = {}) {
  // This direct child is the same helper shape as a synchronous command hook.
  const payload = { hook_event_name: event, session_id: conversation, ...(turn ? { turn_id: turn } : {}), ...(nativeModel && nativeModel !== "omit" ? { model: nativeModel } : {}), ...extra };
  const reply = spawnSync(binary, ["report", "codex", "--stdin"], { input: JSON.stringify(payload), encoding: "utf8", timeout: 5000, maxBuffer: 65536 });
  if (reply.error || reply.status !== 0 || reply.stdout !== "") throw new Error("native reporting helper failed or returned an approval decision");
}
function control(operation, body, override = {}) {
  if (["Publish", "PublishQuestion"].includes(operation)) {
    const name = operation === "Publish" ? "ovrcr_publish" : "ovrcr_publish_question";
    const frames = [
      { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "codex-fixture", version: "1" } } },
      { jsonrpc: "2.0", method: "notifications/initialized" },
      { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name, arguments: body } },
    ];
    const reply = spawnSync(binary, ["delegation-publisher"], {
      input: frames.map(frame => JSON.stringify(frame)).join("\n") + "\n",
      env: { ...process.env, OVRCR_DELEGATION_SOCKET: socket, OVRCR_DELEGATION_TOKEN: override.capability ?? capability },
      encoding: "utf8", timeout: 15000, maxBuffer: 131072,
    });
    if (reply.error || reply.status !== 0) return { error: "publication outcome unknown" };
    const values = reply.stdout.trim().split("\n").map(line => JSON.parse(line));
    const outcome = values.at(-1).result?.structuredContent;
    return outcome?.status === "published" ? { details: outcome } : { error: outcome?.status ?? "publication protocol error" };
  }
  const reply = spawnSync(binary, ["--json", "delegation"], {
    input: JSON.stringify({ [operation]: { auth: { session, capability, ...override }, ...body } }),
    env: { ...process.env, OVRCR_SOCKET: socket, OVRCR_HOME: instance },
    encoding: "utf8", timeout: 10000, maxBuffer: 1048576,
  });
  if (reply.error) return { error: "control outcome unknown" };
  if (reply.status !== 0) return { error: "control refused or outcome unknown", status: reply.status };
  return { details: JSON.parse(reply.stdout) };
}
function submission() {
  return { schema: 1, identity: brief.identity, version: brief.result_version ?? 1, outcome: "success", summary: "Codex fixture read README.", evidence: ["README:1 controlled fixture"], unresolved: [] };
}
function publishQuestion() {
  const body = { schema: 1, identity: brief.identity, result_version: brief.result_version ?? 1, question_id: "readme-section", question_version: 1, supersedes: null, kind: "Clarification", question: "Which README section is authorized?", context: "Fixture has Introduction and Installation.", choices: ["Introduction", "Installation"] };
  const reply = control("PublishQuestion", { handle: brief.handle, question: body });
  const publication = reply.details?.publication;
  const kind = publication?.kind?.Question;
  question = publication && kind ? { assignment: publication.assignment, result_version: kind.result_version, question_id: kind.question_id, question_version: publication.version, digest: publication.digest } : undefined;
  record(`question-v${body.result_version}.json`, reply);
  return reply;
}
function startBrief(path) {
  brief = JSON.parse(readFileSync(path, "utf8"));
  turn = `turn-${brief.result_version ?? 1}`;
  hook("UserPromptSubmit");
  appendFileSync(join(root, "brief-inputs.jsonl"), `${JSON.stringify({ path, identity: brief.identity, version: brief.result_version ?? 1, conversation })}\n`, { mode: 0o600 });
  record(`brief-v${brief.result_version ?? 1}.json`, { path, conversation });
  if (brief.contract.scope.includes("QUESTION")) publishQuestion();
}
function action(command) {
  switch (command.operation) {
    case "publish": {
      let value = submission();
      if (command.variant === "stale") value = lastSubmission;
      if (command.variant === "foreign-run") value.identity = { ...value.identity, run: 999999 };
      if (command.variant === "incomplete") value = { ...value, summary: "", evidence: [] };
      if (command.variant === "malformed") value = { schema: 1, identity: brief.identity };
      const reply = control("Publish", { handle: brief.handle, submission: value }, command.unauthorized ? { capability: "0".repeat(64) } : {});
      if (!reply.error && command.variant !== "stale") lastSubmission = structuredClone(value);
      return reply;
    }
    case "question": return publishQuestion();
    case "stop": hook("Stop"); return { observed: true };
    case "approval": hook("PermissionRequest"); return { observed: true };
    case "login": process.stdout.write("CODEX_FIXTURE_LOGIN_REQUIRED\n"); return { unresolved_native_input: true };
    case "replace":
      conversation = randomUUID(); hook("SessionStart", { source: "clear" });
      if (command.return_original) { conversation = originalConversation; hook("SessionStart", { source: "clear" }); }
      return { conversation };
    case "foreign-root": hook("Stop", { agent_id: "native-helper", session_id: randomUUID() }); return { observed: true };
    case "parent-control": return control("Collect", { handle: brief.handle });
    default: throw new Error("unknown controlled fixture action");
  }
}
hook("SessionStart", { source: "startup" });
record("startup.json", { argv: args, conversation, reported_model: nativeModel ?? null, pid: process.pid, codex_home: process.env.CODEX_HOME,
  profile: readFileSync(join(process.env.CODEX_HOME, "ovrcr-research.config.toml"), "utf8"),
  reporter_config: readFileSync(join(process.env.CODEX_HOME, "config.toml"), "utf8"),
  artifact_root: process.env.OVRCR_DELEGATION_ARTIFACT_ROOT,
  delegation_session: session, delegation_token_present: Boolean(capability), raw_hook_token_present: Boolean(process.env.OVRCR_HOOK_TOKEN),
});
process.stdout.write("CODEX_NATIVE_FIXTURE_READY\n");
// File commands drive model work only. They never supply Server observations or authority.
const timer = setInterval(() => {
  const path = join(root, "command.json");
  if (!existsSync(path)) return;
  const command = JSON.parse(readFileSync(path, "utf8"));
  unlinkSync(path);
  record(`reply-${command.sequence}.json`, action(command));
}, 10);
try {
  for await (const line of createInterface({ input: process.stdin })) {
    const text = line.replaceAll("\x1b[200~", "").replaceAll("\x1b[201~", "");
    const match = text.match(/(?:in |brief )(\/[^\n]+\/brief-v[1-9][0-9]*\.json)(?:\.|;|$)/);
    if (match) { startBrief(match[1]); continue; }
    if (text.startsWith("Read the scoped answer in ")) {
      const match = text.match(/in (.+\/answer-[A-Za-z0-9_-]+\.json) for question ([A-Za-z0-9_-]+) version ([1-9][0-9]*)\./);
      if (!match) throw new Error("missing exact answer file reference");
      const answer = JSON.parse(readFileSync(match[1], "utf8"));
      if (!isDeepStrictEqual(answer.question, question) || answer.question.question_id !== match[2] || answer.question.question_version !== Number(match[3])) throw new Error("answer reference mismatch");
      appendFileSync(join(root, "answers.jsonl"), `${JSON.stringify(answer)}\n`, { mode: 0o600 });
      record("answer-received.json", answer);
    }
  }
} finally {
  clearInterval(timer);
}
