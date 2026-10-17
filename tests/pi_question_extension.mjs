// Supplemental extension/schema and private-intent checks. The Rust questions
// suite owns actual Server/PTY delivery; this transport stub cannot prove it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, chmodSync, rmSync, symlinkSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHost } from "./fixtures/pi/pi_host.mjs";

const assignment = { instance: "instance", workflow: "workflow", controller: "parent", controller_epoch: 1, task: "task", attempt: 1, contract: 1, request: "assignment", session: 2, run: 3 };
const question = { assignment, result_version: 4, question_id: "section", question_version: 2, digest: "a".repeat(64) };
const fresh = { handle: "assignment", question, expected_control_epoch: 7, basis: "ExistingAuthority", scope: "Read only README", answer: "ANSWER_PRIVATE_SECRET: use the Introduction section" };

async function fixture(t, { loseFirst = false } = {}) {
  const directory = mkdtempSync(join(tmpdir(), "ovrcr-question-extension-"));
  const before = { ...process.env };
  const binary = join(directory, "control.mjs"), calls = join(directory, "calls.jsonl");
  writeFileSync(binary, `#!/usr/bin/env node
import { readFileSync, writeFileSync, appendFileSync, existsSync } from 'node:fs';
let input=''; for await (const chunk of process.stdin) input+=chunk;
const request=JSON.parse(input);
if(request.Observe){console.log(JSON.stringify({cursor:{server_epoch:'fixture',sequence:1},status:'Deadline',progress_deadline_ms:1000,observations:[]}));process.exit(0);}
appendFileSync(${JSON.stringify(calls)},JSON.stringify(request)+'\\n');
const fields=request.AnswerQuestion;
if(!fields) throw new Error('unexpected operation');
if(${JSON.stringify(loseFirst)} && !existsSync(${JSON.stringify(join(directory, "lost"))})){writeFileSync(${JSON.stringify(join(directory, "lost"))},'lost');process.exit(1);}
console.log(JSON.stringify({handle:fields.handle,dispatch:'Applied',result_version:fields.answer.question.result_version,questions:[],receipts:[{request:fields.answer.request,operation:'AnswerQuestion',outcome:'Applied'}]}));
`);
  chmodSync(binary, 0o700);
  Object.assign(process.env, { OVRCR_AGENT_SOCKET: "/fixture/reporting", OVRCR_AGENT_TOKEN: "fixture-reporting-secret", OVRCR_DELEGATION_SESSION: "1", OVRCR_DELEGATION_TOKEN: "fixture-control-secret", OVRCR_DELEGATION_SOCKET: "/fixture/control", OVRCR_DELEGATION_HOME: directory });
  const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify(binary));
  writeFileSync(join(directory, "reporting.ts"), source);
  for (const name of ["ovrcr-reporting-transport.mjs", "ovrcr-delegation-observer.mjs", "ovrcr-delegation-continuation.mjs"]) writeFileSync(join(directory, name), readFileSync(new URL(`../src/${name}`, import.meta.url)));
  writeFileSync(join(directory, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
  writeFileSync(join(directory, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
  const host = await createHost(join(directory, "reporting.ts"));
  t.after(async () => { await host.emit({ type: "session_shutdown", reason: "quit" }); process.env = before; rmSync(directory, { recursive: true, force: true }); });
  return { host, directory, calls: () => { try { return readFileSync(calls, "utf8").trim().split("\n").map(JSON.parse); } catch (error) { if (error.code === "ENOENT") return []; throw error; } } };
}

test("question tools expose exact nested references and explicit fresh/recovery alternatives", async t => {
  const f = await fixture(t);
  const publication = f.host.toolDefinition("ovrcr_publish_question").parameters;
  assert.deepEqual(publication.required, ["handle", "question"]);
  const body = publication.properties.question;
  assert.equal(body.additionalProperties, false);
  assert.deepEqual(body.properties.kind.enum, ["Clarification", "HumanDecision"]);
  assert.equal(body.properties.choices.maxItems, 32);
  assert.equal(body.properties.question.maxLength, 32768);
  assert.deepEqual(body.properties.supersedes.anyOf[0].required, ["assignment", "result_version", "question_id", "question_version", "digest"]);
  assert.equal(body.properties.supersedes.anyOf[0].properties.assignment.additionalProperties, false);
  assert.equal(body.properties.supersedes.anyOf[0].properties.digest.pattern, "^[a-f0-9]{64}$");
  const answer = f.host.toolDefinition("ovrcr_answer_question").parameters;
  assert.deepEqual(answer.anyOf, [{ required: ["handle", "request"] }, { required: ["handle", "question", "expected_control_epoch", "basis", "scope", "answer"] }]);
  assert.deepEqual(answer.properties.basis.enum, ["ExistingAuthority", "ExplicitHumanDecision"]);
  assert.deepEqual(f.host.toolDefinition("ovrcr_collect_question").parameters.required, ["handle", "reference"]);
  assert.equal(f.calls().length, 0, "schema discovery has no task effect");
});

test("lost scoped answer reply retains exact private payload and captured parent epoch", async t => {
  const f = await fixture(t, { loseFirst: true });
  const lost = (await f.host.tool("ovrcr_answer_question", fresh)).details;
  assert.equal(lost.dispatch, "Unknown");
  assert.equal(lost.receipts, undefined, "a transport failure cannot fabricate delivery evidence");
  const intent = join(f.directory, "parent-intents", `answer-${lost.request}.json`);
  const bytes = readFileSync(intent, "utf8"), saved = JSON.parse(bytes);
  assert.equal(statSync(intent).mode & 0o777, 0o600);
  assert.equal(saved.expected_parent_epoch, 1);
  assert.equal(saved.answer.control_epoch, 7);
  assert.deepEqual(saved.answer.question, question);
  assert.equal(saved.answer.answer, fresh.answer);
  assert(!bytes.includes("fixture-control-secret") && !bytes.includes("fixture-reporting-secret"));
  const recovered = (await f.host.tool("ovrcr_answer_question", { handle: fresh.handle, request: lost.request })).details;
  assert.equal(recovered.result_version, 4);
  assert.equal(recovered.receipts[0].operation, "AnswerQuestion");
  const records = f.calls();
  assert.deepEqual(records[1], records[0], "recover the original request, exact question, content and captured epochs");
  assert.equal(readFileSync(intent, "utf8"), bytes, "recovery preserves original immutable evidence");
  const augmented = (await f.host.tool("ovrcr_answer_question", { handle: fresh.handle, request: lost.request, answer: "changed" })).details;
  assert.equal(augmented.dispatch, "Refused");
  assert.equal(augmented.transport, "NotSent");
  assert.equal(f.calls().length, 2);
  rmSync(intent); symlinkSync(join(f.directory, "calls.jsonl"), intent);
  const unsafe = (await f.host.tool("ovrcr_answer_question", { handle: fresh.handle, request: lost.request })).details;
  assert.equal(unsafe.dispatch, "Refused");
  assert.equal(unsafe.transport, "NotSent");
  assert.equal(f.calls().length, 2, "a symlink cannot become answer recovery authority");
});

test("missing basis, blank content, UTF-8 byte overflow and mixed recovery refuse before transport", async t => {
  const f = await fixture(t);
  for (const malformed of [
    { ...fresh, basis: undefined }, { ...fresh, scope: " " }, { ...fresh, answer: "" },
    { ...fresh, answer: "λ".repeat(20000) }, { ...fresh, expected_control_epoch: 0 },
    { ...fresh, request: "new-request" },
  ]) {
    const refused = (await f.host.tool("ovrcr_answer_question", malformed)).details;
    assert.equal(refused.dispatch, "Refused");
    assert.equal(refused.transport, "NotSent");
  }
  assert.equal(f.calls().length, 0);
});
