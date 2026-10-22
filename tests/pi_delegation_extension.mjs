import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, rmSync, chmodSync, symlinkSync, appendFileSync, mkdirSync, openSync, closeSync, constants, existsSync } from "node:fs";
import { open } from "node:fs/promises";
import { tmpdir } from "node:os";
import childProcess, { execFileSync } from "node:child_process";
import { syncBuiltinESMExports } from "node:module";
import { join } from "node:path";
import { createHost } from "./fixtures/pi/pi_host.mjs";

test("managed Pi exposes research assignment and explicit collection tools", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-delegation-"));
  const before = { ...process.env };
  let host;
  try {
    process.env.OVRCR_AGENT_SOCKET = "/fixture/socket";
    process.env.OVRCR_AGENT_TOKEN = "fixture-token";
    const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify("/fixture/ovrcr"));
    writeFileSync(join(dir, "reporting.ts"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-observer.mjs"), readFileSync(new URL("../src/ovrcr-delegation-observer.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-continuation.mjs"), readFileSync(new URL("../src/ovrcr-delegation-continuation.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
    host = await createHost(join(dir, "reporting.ts"));
    assert.deepEqual(host.tools(), ["ovrcr_accept", "ovrcr_answer_question", "ovrcr_archive", "ovrcr_cancel_workflow", "ovrcr_capabilities", "ovrcr_capture_fallback", "ovrcr_checkpoint", "ovrcr_collect", "ovrcr_collect_question", "ovrcr_confirm_interrupt", "ovrcr_confirm_return", "ovrcr_deliver", "ovrcr_dependencies", "ovrcr_followup", "ovrcr_inspect", "ovrcr_interrupt", "ovrcr_mark_handled", "ovrcr_parents", "ovrcr_publish", "ovrcr_publish_question", "ovrcr_recover", "ovrcr_recover_fallback", "ovrcr_retry", "ovrcr_review", "ovrcr_start", "ovrcr_transfer", "ovrcr_wait"]);
    assert.deepEqual(host.toolDefinition("ovrcr_publish").parameters.required, ["handle", "submission"]);
    assert.deepEqual(host.toolDefinition("ovrcr_collect").parameters.required, ["handle"]);
    assert.deepEqual(host.toolDefinition("ovrcr_start").parameters.anyOf, [{ required: ["handle"], maxProperties: 2, properties: { handle: { type: "string" }, prepare_only: { type: "boolean" } }, additionalProperties: false }, { required: ["project", "workspace", "scope", "acceptance_evidence"], not: { required: ["handle"] } }]);
    assert.equal(host.toolDefinition("ovrcr_start").parameters.properties.prepare_only.type, "boolean");
    const coding = host.toolDefinition("ovrcr_start").parameters.properties.coding;
    assert.deepEqual(coding.required,["requested_base","branch","authorized_paths","required_checks"]);
    assert.equal(coding.additionalProperties,false);
    const graph=host.toolDefinition("ovrcr_dependencies").parameters.properties.graph;
    assert.equal(graph.properties.tasks.maxItems,64);
    assert.equal(graph.properties.edges.maxItems,256);
    assert.equal(graph.additionalProperties,false);
    assert.equal(host.toolDefinition("ovrcr_start").parameters.properties.review.additionalProperties,false);
    const reviewSchema = host.toolDefinition("ovrcr_start").parameters.properties.review;
    assert.equal(reviewSchema.properties.context_paths.maxItems,32);
    assert.equal(reviewSchema.properties.context_paths.uniqueItems,true);
    assert.ok(!reviewSchema.required.includes("context_paths"),"legacy direct review remains valid");
    assert.ok(!reviewSchema.properties.target.required.includes("view"),"Server Prepare enriches direct review");
    assert.equal(reviewSchema.properties.target.properties.view.anyOf[0].properties.round.pattern,"^[A-Za-z0-9_-]+$");
    assert.equal(coding.properties.authorized_paths.minItems,1);
    assert.equal(coding.properties.required_checks.uniqueItems,true);
    assert.deepEqual(host.toolDefinition("ovrcr_followup").parameters.anyOf, [{required:["handle","request"]},{required:["handle","expected_identity","expected_result_version","expected_control_epoch","text"]}]);
    assert.deepEqual(host.toolDefinition("ovrcr_accept").parameters.required, ["handle","identity","result_version","expected_control_epoch"]);
    const malformed = await host.tool("ovrcr_followup", {handle:"assignment",expected_identity:{},expected_result_version:1,expected_control_epoch:1,text:""});
    assert.equal(malformed.details.dispatch, "Refused");
    assert.equal(malformed.details.transport, "NotSent");
    assert.deepEqual(host.toolDefinition("ovrcr_confirm_return").parameters.required, ["handle","request","returned_request","expected_control_epoch","evidence"]);
    await assert.rejects(() => host.tool("ovrcr_confirm_return", {handle:"assignment"}), /missing required argument: request/);
    const missingEpoch = await host.tool("ovrcr_followup", {handle:"assignment",expected_identity:{},expected_result_version:1,text:"scoped"});
    assert.equal(missingEpoch.details.transport,"NotSent");
    await assert.rejects(() => host.tool("ovrcr_accept", {}), /missing required argument: handle/);
    const missing = await host.tool("ovrcr_start", {});
    assert.equal(missing.details.dispatch, "Refused");
    assert.equal(missing.details.handle, null);
    const start = host.toolDefinition("ovrcr_start").execute;
    for (const mode of ["coding", "review"]) {
      const refused = await start("claude-refusal", {
        project: "p", workspace: "w", scope: "Unsupported Claude mode",
        acceptance_evidence: ["fixture"], provider: "claude", [mode]: {},
      }, undefined, undefined, {});
      assert.equal(refused.details.dispatch, "Refused");
      assert.equal(refused.details.transport, "NotSent");
      assert.equal(refused.details.handle, null);
      assert.deepEqual(refused.details.identity, { session: null, run: null });
      assert.equal(refused.details.reason, mode === "coding"
        ? "Claude coding delegation is not supported" : "Claude independent review delegation is not supported");
    }
    await assert.rejects(() => host.tool("ovrcr_inspect", {}), /missing required argument: handle/);
  } finally {
    process.env = before;
    rmSync(dir, { recursive: true, force: true });
  }
});

test("acceptance exclusivity rejects direct registered callbacks before transport", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-accept-exclusivity-"));
  const before = { ...process.env };
  const launchNames = ["spawn", "spawnSync", "exec", "execSync", "execFile", "execFileSync", "fork"];
  const originals = new Map(launchNames.map(name => [name, childProcess[name]]));
  let launches = 0;
  try {
    for (const name of launchNames) childProcess[name] = () => {
      launches += 1;
      throw new Error("pure acceptance validation test forbids process launch");
    };
    syncBuiltinESMExports();
    Object.assign(process.env, {
      OVRCR_AGENT_SOCKET: "/fixture/reporting", OVRCR_AGENT_TOKEN: "fixture-reporting",
      OVRCR_DELEGATION_SESSION: "1", OVRCR_DELEGATION_TOKEN: "fixture-delegation",
      OVRCR_DELEGATION_SOCKET: "/fixture/control", OVRCR_DELEGATION_HOME: dir,
    });
    writeFileSync(join(dir, "reporting.ts"), readFileSync(
      process.env.OVRCR_TEST_EXTENSION_SOURCE ?? new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8"
    ).replace("__OVRCR_BINARY__", JSON.stringify("/fixture/never-launch")));
    for (const name of ["ovrcr-reporting-transport.mjs", "ovrcr-delegation-observer.mjs", "ovrcr-delegation-continuation.mjs"]) {
      writeFileSync(join(dir, name), readFileSync(new URL(`../src/${name}`, import.meta.url)));
    }
    // This validation needs no optional installed-provider API or lifecycle event.
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), "export async function loadPiKeybindings(){return undefined}\nexport function inspectPiInterruptBinding(){return 'Unavailable'}\n");
    const host = await createHost(join(dir, "reporting.ts"));
    const callback = host.toolDefinition("ovrcr_accept").execute;
    for (const fields of [{}, { target: { ChildSubmission: { digest: "a".repeat(64) } }, result_digest: "a".repeat(64) }]) {
      // Some hosts invoke callbacks directly without enforcing JSON-schema oneOf.
      const response = await callback("direct-host", {
        handle: "assignment", identity: {}, result_version: 1, expected_control_epoch: 1, ...fields,
      }, undefined, undefined, {});
      assert.deepEqual(response.details, {
        handle: "assignment", dispatch: "Refused", transport: "NotSent",
        reason: "provide exactly one target or legacy result_digest",
      });
      assert.equal(response.details.receipts, undefined);
      assert.equal(response.details.acceptance_ref, undefined);
      assert.equal(launches, 0, "invalid local arguments must never attempt control transport");
    }
  } finally {
    for (const [name, original] of originals) childProcess[name] = original;
    syncBuiltinESMExports();
    process.env = before;
    rmSync(dir, { recursive: true, force: true });
  }
});

test("registered start rejects invalid action authority before intent or transport", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-start-authority-"));
  const before = { ...process.env };
  const launchNames = ["spawn", "spawnSync", "exec", "execSync", "execFile", "execFileSync", "fork"];
  const originals = new Map(launchNames.map(name => [name, childProcess[name]]));
  let launches = 0;
  try {
    for (const name of launchNames) childProcess[name] = () => {
      launches += 1;
      throw new Error("invalid start must never launch control transport");
    };
    syncBuiltinESMExports();
    Object.assign(process.env, {
      OVRCR_AGENT_SOCKET: "/fixture/reporting", OVRCR_AGENT_TOKEN: "fixture-reporting",
      OVRCR_DELEGATION_SESSION: "1", OVRCR_DELEGATION_TOKEN: "fixture-delegation",
      OVRCR_DELEGATION_SOCKET: "/fixture/control", OVRCR_DELEGATION_HOME: dir,
    });
    writeFileSync(join(dir, "reporting.ts"), readFileSync(
      new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8"
    ).replace("__OVRCR_BINARY__", JSON.stringify("/fixture/never-launch")));
    for (const name of ["ovrcr-reporting-transport.mjs", "ovrcr-delegation-observer.mjs",
      "ovrcr-delegation-continuation.mjs", "ovrcr-pi-runtime.ts"]) {
      writeFileSync(join(dir, name), readFileSync(name === "ovrcr-pi-runtime.ts"
        ? process.env.OVRCR_TEST_PI_RUNTIME_SOURCE ?? new URL(`../src/${name}`, import.meta.url)
        : new URL(`../src/${name}`, import.meta.url)));
    }
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), "export async function loadPiKeybindings(){return undefined}\nexport function inspectPiInterruptBinding(){return 'Unavailable'}\n");
    const host = await createHost(join(dir, "reporting.ts"));
    const callback = host.toolDefinition("ovrcr_start").execute;
    const research = { project: "p", workspace: "w", scope: "Read", acceptance_evidence: ["citation"] };
    for (const fields of [
      { permitted_actions: ["read", "result_file"] },
      { permitted_actions: ["read", "write"] },
      { permitted_actions: [] },
      { permitted_actions: null },
      { review: {}, permitted_actions: ["read", "result_file"] },
      { coding: {}, permitted_actions: ["read", "write", "edit"] },
      { coding: {}, permitted_actions: ["write", "read", "edit", "check"] },
    ]) {
      const response = await callback("direct-host", { ...research, ...fields }, undefined, undefined, {});
      assert.equal(launches, 0, "invalid actions must not even query capabilities or reach Prepare");
      assert.match(response.details.reason, /exactly.*read/);
      assert.equal(response.details.dispatch, "Refused");
      assert.equal(response.details.transport, "NotSent");
      assert.equal(response.details.handle, null);
      assert.equal(response.details.receipts, undefined);
      assert.equal(existsSync(join(dir, "parent-intents")), false, "invalid actions must not persist an original intent");
    }
    for (const extra of [{ request: "replacement" }, { scope: "changed" }, { unknown: true }]) {
      const refused = await callback("direct-host", { handle: "original", prepare_only: true, ...extra }, undefined, undefined, {});
      assert.equal(refused.details.dispatch, "Refused");
      assert.equal(launches, 0, "prepare-only handle replay must reject every extra key before transport");
      assert.equal(existsSync(join(dir, "parent-intents")), false);
    }
    const mixed = await callback("direct-host", { ...research, handle: "original" }, undefined, undefined, {});
    assert.equal(mixed.details.dispatch, "Refused");
    assert.equal(mixed.details.handle, "original");
    assert.equal(launches, 0);
    assert.equal(existsSync(join(dir, "parent-intents")), false);
  } finally {
    for (const [name, original] of originals) childProcess[name] = original;
    syncBuiltinESMExports();
    process.env = before;
    rmSync(dir, { recursive: true, force: true });
  }
});

for (const loss of ["before", "after"]) test(`original intent recovers a lost Prepare ${loss} persistence`, async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-original-intent-"));
  const before = { ...process.env };
  let host;
  try {
    const binary = join(dir, "control.mjs");
    const state = join(dir, "state.json");
    const calls = join(dir, "calls");
    writeFileSync(binary, `#!/usr/bin/env node
import { readFileSync, writeFileSync, appendFileSync, existsSync } from 'node:fs';
let input = ''; for await (const bytes of process.stdin) input += bytes;
const request = JSON.parse(input); const operation = Object.keys(request)[0];
appendFileSync(${JSON.stringify(calls)}, operation + '\\n');
let value = existsSync(${JSON.stringify(state)}) ? JSON.parse(readFileSync(${JSON.stringify(state)})) : null;
if (operation === 'Capabilities') { console.log(JSON.stringify({resolved_choice:{provider:'pi',model_provider:'test',model:'fixture-model'},effective_limits:{max_live_children:50,max_execution_attempts:null,min_free_slots_after_launch:0,max_cost_microusd:null,max_output_tokens:null,max_native_children:null},capabilities:['read','result_file']})); process.exit(0); }
if (operation === 'Prepare') {
  const contract = request.Prepare.contract;
  value ??= { handle: contract.request, identity: {instance:'i',workflow:contract.workflow,controller:contract.controller,controller_epoch:1,task:contract.task,attempt:1,contract:1,request:contract.request,session:null,run:null}, dispatch:'Prepared', control:{epoch:1}, receipts:[] };
  if (!existsSync(${JSON.stringify(join(dir, "lost"))})) {
    writeFileSync(${JSON.stringify(join(dir, "lost"))}, 'lost');
    if (${JSON.stringify(loss)} === 'after') writeFileSync(${JSON.stringify(state)}, JSON.stringify(value));
    process.exit(1);
  }
  value.receipts = [{operation:'Prepare',outcome:'Applied'}];
} else if (!value) { console.error(JSON.stringify({error:{code:'NotFound',message:'assignment not found'}})); process.exit(1); }
if (operation === 'Launch') { value.identity.session = 41; value.identity.run = 1; value.dispatch = 'Launched'; value.receipts.push({operation:'Launch',outcome:'Applied'}); }
if (operation === 'SendInitial') { if(request.SendInitial.expected_control_epoch !== 1) throw new Error('missing initial epoch'); value.control = {epoch:1}; value.dispatch = 'Applied'; value.receipts.push({operation:'InitialInput',outcome:'Applied'}); }
writeFileSync(${JSON.stringify(state)}, JSON.stringify(value)); console.log(JSON.stringify(value));
`);
    chmodSync(binary, 0o700);
    process.env.OVRCR_AGENT_SOCKET = "/fixture/reporting";
    process.env.OVRCR_AGENT_TOKEN = "fixture-reporting";
    process.env.OVRCR_DELEGATION_SESSION = "1";
    process.env.OVRCR_DELEGATION_TOKEN = "fixture-delegation";
    process.env.OVRCR_DELEGATION_SOCKET = "/fixture/control";
    process.env.OVRCR_DELEGATION_HOME = dir;
    const source = readFileSync(process.env.OVRCR_TEST_EXTENSION_SOURCE ?? new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify(binary));
    writeFileSync(join(dir, "reporting.ts"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-observer.mjs"), readFileSync(new URL("../src/ovrcr-delegation-observer.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-continuation.mjs"), readFileSync(new URL("../src/ovrcr-delegation-continuation.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
    host = await createHost(join(dir, "reporting.ts"));
    const first = (await host.tool("ovrcr_start", {project:"p",workspace:"w",scope:"Read",model_provider:"test",model:"fixture-model",acceptance_evidence:["citation"],...(loss === "before" ? {permitted_actions:["read"]} : {})})).details;
    assert.equal(first.dispatch, "Unknown");
    assert.equal(first.transport, "Unknown", "a genuinely lost Prepare response remains uncertain");
    assert.equal(first.receipts, undefined, "transport failure must not fabricate Server receipts");
    const recovered = (await host.tool("ovrcr_start", {handle:first.handle})).details;
    assert.equal(recovered.dispatch, "Applied");
    assert.equal(recovered.identity.session, 41);
    const operations = readFileSync(calls, "utf8").trim().split("\n");
    assert.equal(operations.filter(v => v === "Launch").length, 1);
    assert.equal(operations.filter(v => v === "SendInitial").length, 1);
    // A symlinked original intent cannot acquire authority from its contents.
    const bad = (await host.tool("ovrcr_start", {handle:"../escape"})).details;
    assert.equal(bad.dispatch, "Refused");
    const intent = join(dir,"parent-intents",`${first.handle}.json`);
    const originalBytes = readFileSync(intent);
    rmSync(intent);
    symlinkSync(state,intent);
    const unsafe = (await host.tool("ovrcr_start", {handle:first.handle})).details;
    assert.equal(unsafe.dispatch,"Refused");
    assert.equal(readFileSync(calls,"utf8").trim().split("\n").length,operations.length,"unsafe intent must not contact the Server");
    rmSync(intent);
    writeFileSync(intent,originalBytes,{mode:0o600});
    const probe = await open(intent,"r");
    const prototype = Object.getPrototypeOf(probe);
    await probe.close();
    const stat = prototype.stat, read = prototype.read, readFile = prototype.readFile;
    let grew = false, readBytes = 0;
    try {
      prototype.stat = async function(...args) {
        const value = await stat.apply(this,args);
        if (!grew && value.isFile() && value.size === originalBytes.length) {
          grew = true;
          appendFileSync(intent,Buffer.alloc(524288,32));
        }
        return value;
      };
      prototype.read = async function(...args) { const result=await read.apply(this,args); readBytes+=result.bytesRead; return result; };
      prototype.readFile = async function(...args) { const result=await readFile.apply(this,args); readBytes+=result.length; return result; };
      const growing = (await host.tool("ovrcr_start",{handle:first.handle})).details;
      assert.equal(growing.dispatch,"Refused");
      assert.ok(grew);
      assert.equal(readBytes,262145,"a file growing after stat must not escape the actual read budget");
    } finally { prototype.stat=stat; prototype.read=read; prototype.readFile=readFile; }
  } finally { await host?.emit({type:"session_shutdown",reason:"quit"}); process.env = before; rmSync(dir, {recursive:true,force:true}); }
});

test("followup recovery retains exact private request after a lost reply", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-followup-intent-"));
  const before = { ...process.env };
  let host;
  try {
    const binary = join(dir, "control.mjs"), calls = join(dir, "calls"), lost = join(dir, "lost");
    writeFileSync(binary, `#!/usr/bin/env node
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
let input='';for await(const bytes of process.stdin)input+=bytes;
const request=JSON.parse(input);
if (request.Reconcile) {
  appendFileSync(${JSON.stringify(calls)},JSON.stringify(request)+'\\n');
  console.log(JSON.stringify({handle:request.Reconcile.request,control:{epoch:9},receipts:[{request:'legacy',operation:'Followup',outcome:'Applied'}]}));
  process.exit(0);
}
const fields=request.Followup;
if (fields.expected_control_epoch !== 1) throw new Error('missing captured epoch');
appendFileSync(${JSON.stringify(calls)},JSON.stringify(fields)+'\\n');
if(!existsSync(${JSON.stringify(lost)})){writeFileSync(${JSON.stringify(lost)},'lost');process.exit(1);}
console.log(JSON.stringify({handle:fields.handle,identity:fields.expected_identity,dispatch:'Applied',receipts:[{request:fields.request,operation:'Followup',outcome:'Applied'}]}));
`);
    chmodSync(binary, 0o700);
    Object.assign(process.env, {OVRCR_AGENT_SOCKET:"/fixture/reporting",OVRCR_AGENT_TOKEN:"fixture-reporting",OVRCR_DELEGATION_SESSION:"1",OVRCR_DELEGATION_TOKEN:"fixture-delegation",OVRCR_DELEGATION_SOCKET:"/fixture/control",OVRCR_DELEGATION_HOME:dir});
    const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify(binary));
    writeFileSync(join(dir, "reporting.ts"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-observer.mjs"), readFileSync(new URL("../src/ovrcr-delegation-observer.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-continuation.mjs"), readFileSync(new URL("../src/ovrcr-delegation-continuation.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
    host = await createHost(join(dir, "reporting.ts"));
    const first = (await host.tool("ovrcr_followup", {handle:"assignment",expected_identity:{request:"assignment",contract:1},expected_result_version:1,expected_control_epoch:1,text:"Clarify the scoped result"})).details;
    assert.equal(first.dispatch,"Unknown");
    assert.equal(first.receipts,undefined);
    const recovered = (await host.tool("ovrcr_followup", {handle:"assignment",request:first.request})).details;
    assert.equal(recovered.dispatch,"Applied");
    const records=readFileSync(calls,"utf8").trim().split("\n").map(JSON.parse);
    assert.deepEqual(records[1],records[0],"recovery sends the identical operation identity and payload");
    const saved=join(dir,"parent-intents",`followup-${first.request}.json`);
    const bytes=readFileSync(saved,"utf8");
    assert.ok(!bytes.includes("fixture-delegation"),"persist no capability in task payloads");
    const legacy=JSON.parse(bytes);delete legacy.expected_control_epoch;
    writeFileSync(saved,JSON.stringify(legacy),{mode:0o600});
    const legacyBytes=readFileSync(saved,"utf8");
    const observed=(await host.tool("ovrcr_followup",{handle:"assignment",request:first.request})).details;
    assert.equal(observed.control.epoch,9);
    assert.equal(observed.receipts[0].outcome,"Applied");
    assert.equal(readFileSync(saved,"utf8"),legacyBytes,"legacy immutable bytes are preserved");
    const latest=JSON.parse(readFileSync(calls,"utf8").trim().split("\n").at(-1));
    assert.deepEqual(Object.keys(latest),["Reconcile"],"legacy recovery only observes even if a fresh epoch exists");
    rmSync(saved);symlinkSync(calls,saved);
    const unsafe=(await host.tool("ovrcr_followup",{handle:"assignment",request:first.request})).details;
    assert.equal(unsafe.dispatch,"Refused");
    assert.equal(readFileSync(calls,"utf8").trim().split("\n").length,3,"unsafe original intent cannot contact control");
  } finally {await host?.emit({type:"session_shutdown",reason:"quit"});process.env=before;rmSync(dir,{recursive:true,force:true});}
});


test("failed registered collection releases pending follow-up without accepting either result", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-failed-collect-"));
  const before = { ...process.env };
  let host;
  const statePath = join(dir, "state.json");
  const callsPath = join(dir, "calls.jsonl");
  const eventually = async predicate => {
    const deadline = Date.now() + 5000;
    while (!predicate()) {
      assert(Date.now() < deadline, "fixture observation did not arrive");
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  };
  try {
    const binary = join(dir, "control.mjs");
    writeFileSync(binary, String.raw`#!/usr/bin/env node
import { readFileSync, appendFileSync } from "node:fs";
if (process.argv[2] !== "--json") process.exit(0);
let input = "";
for await (const bytes of process.stdin) input += bytes;
const request = JSON.parse(input);
const operation = Object.keys(request)[0];
const state = JSON.parse(readFileSync(process.env.OVRCR_TEST_COLLECT_STATE, "utf8"));
appendFileSync(process.env.OVRCR_TEST_COLLECT_CALLS, JSON.stringify({ operation, phase: state.phase }) + "\n");
if (operation === "Collect") {
  process.stderr.write(JSON.stringify({ error: { code: "InvalidRequest", message: "publication A changed or is partial" } }));
  process.exit(1);
}
if (operation !== "Observe") process.exit(2);
const item = handle => ({ handle, current: true, activity: { state: "Busy" }, input_requests: [], stalled: false, publication: { identity: { assignment: { task: handle }, kind: "Submission", version: 1, digest: handle }, handled: false } });
process.stdout.write(JSON.stringify({ cursor: { server_epoch: "fixture", sequence: state.phase === "A" ? 1 : 2 }, status: "Changed", progress_deadline_ms: 1000, observations: (state.phase === "A" ? ["A"] : ["A", "B"]).map(item) }));
`);
    chmodSync(binary, 0o700);
    writeFileSync(statePath, JSON.stringify({ phase: "A" }));
    writeFileSync(callsPath, "");
    Object.assign(process.env, {
      OVRCR_AGENT_SOCKET: "/fixture/reporting", OVRCR_AGENT_TOKEN: "fixture-reporting-token",
      OVRCR_DELEGATION_SESSION: "1", OVRCR_DELEGATION_TOKEN: "fixture-control-token",
      OVRCR_DELEGATION_SOCKET: "/fixture/control", OVRCR_DELEGATION_HOME: dir,
      OVRCR_TEST_COLLECT_STATE: statePath, OVRCR_TEST_COLLECT_CALLS: callsPath,
    });
    const extension = join(dir, "reporting.ts");
    writeFileSync(extension, readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify(binary)));
    for (const name of ["ovrcr-reporting-transport.mjs", "ovrcr-delegation-observer.mjs"]) writeFileSync(join(dir, name), readFileSync(new URL(`../src/${name}`, import.meta.url)));
    for (const name of ["ovrcr-reporting-transport.mjs", "ovrcr-delegation-continuation.mjs"]) writeFileSync(join(dir, name), readFileSync(new URL(`../src/${name}`, import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
    host = await createHost(extension, { idle: false });
    await host.tool("ovrcr_wait", { handles: ["A", "B"], wait_ms: 0 });
    await eventually(() => host.state.messages.length === 1);
    assert.deepEqual(host.state.messages[0].message.details.handles, ["A"]);
    writeFileSync(statePath, JSON.stringify({ phase: "B" }));
    // A subsequent public Observe request proves the previous B snapshot was consumed.
    await eventually(() => readFileSync(callsPath, "utf8").trim().split("\n").map(line => JSON.parse(line)).filter(call => call.operation === "Observe" && call.phase === "B").length >= 2);
    assert.equal(host.state.messages.length, 1, "B is pending behind A's outstanding continuation");
    await assert.rejects(host.tool("ovrcr_collect", { handle: "A" }), /publication A changed or is partial/);
    assert.equal(host.state.messages.length, 2, "failed collection must release the consumed continuation for B");
    assert.deepEqual(host.state.messages[1].message.details.handles, ["B"]);
    assert.deepEqual(host.state.messages[1].options, { triggerTurn: true, deliverAs: "followUp" });
    const calls = readFileSync(callsPath, "utf8").trim().split("\n").map(line => JSON.parse(line));
    assert(calls.every(call => ["Observe", "Collect"].includes(call.operation)), "collection failure must not mark either publication handled or accepted");
    assert.equal(JSON.parse(readFileSync(statePath, "utf8")).phase, "B");
  } finally {
    await host?.emit({ type: "session_shutdown", reason: "quit" });
    process.env = before;
    rmSync(dir, { recursive: true, force: true });
  }
});

test("retry rejects a FIFO intent before a blocking open can wait for a writer", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-retry-fifo-"));
  const before = { ...process.env };
  let release;
  try {
    process.env.OVRCR_AGENT_SOCKET = "/fixture/socket";
    process.env.OVRCR_AGENT_TOKEN = "fixture-token";
    process.env.OVRCR_DELEGATION_HOME = dir;
    mkdirSync(join(dir, "parent-intents"), { mode: 0o700 });
    const fifo = join(dir, "parent-intents", "fifo.json");
    execFileSync("mkfifo", ["-m", "600", fifo]);
    const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify("/fixture/ovrcr"));
    writeFileSync(join(dir, "reporting.ts"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-observer.mjs"), readFileSync(new URL("../src/ovrcr-delegation-observer.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-continuation.mjs"), readFileSync(new URL("../src/ovrcr-delegation-continuation.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
    const host = await createHost(join(dir, "reporting.ts"));
    let waitedForWriter = false;
    // Release the original defect so a failed regression cannot hang Node's
    // filesystem worker. The fixed reader refuses without needing this writer.
    release = setTimeout(() => {
      waitedForWriter = true;
      closeSync(openSync(fifo, constants.O_RDWR | constants.O_NONBLOCK));
    }, 2000);
    const reply = await host.tool("ovrcr_retry", { handle: "fifo" });
    assert.equal(waitedForWriter, false, "intent recovery blocked until a FIFO writer appeared");
    assert.equal(reply.details.dispatch, "Refused");
    assert.match(reply.details.reason, /regular bounded private owned file/);
  } finally {
    clearTimeout(release);
    process.env = before;
    rmSync(dir, { recursive: true, force: true });
  }
});

test("tool queued on old recovery cannot start work after explicit new", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-retired-routing-"));
  const before = { ...process.env };
  let host;
  try {
    const binary = join(dir, "control.mjs"), calls = join(dir, "calls"), gate = join(dir, "release");
    writeFileSync(binary, `#!/usr/bin/env node
import { appendFileSync, existsSync } from 'node:fs';
let input='';for await(const bytes of process.stdin)input+=bytes;
if(process.argv.includes('report'))process.exit(0);
const request=JSON.parse(input), operation=Object.keys(request)[0];
appendFileSync(${JSON.stringify(calls)},operation+'\\n');
if(operation==='Recover'){while(!existsSync(${JSON.stringify(gate)}))await new Promise(r=>setTimeout(r,5));console.log('[]');}
else if(operation==='Capabilities')console.log(JSON.stringify({reason:'fixture refuses allocation'}));
else throw new Error('Unexpected effect '+operation);
`);
    chmodSync(binary, 0o700);
    Object.assign(process.env, { OVRCR_AGENT_SOCKET: "/fixture/reporting", OVRCR_AGENT_TOKEN: "fixture-reporting", OVRCR_DELEGATION_SESSION: "1", OVRCR_DELEGATION_TOKEN: "fixture-control", OVRCR_DELEGATION_SOCKET: "/fixture/control", OVRCR_DELEGATION_HOME: dir });
    const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify(binary));
    writeFileSync(join(dir,"reporting.ts"),source);
    for (const file of ["ovrcr-reporting-transport.mjs","ovrcr-delegation-observer.mjs","ovrcr-delegation-continuation.mjs"]) writeFileSync(join(dir,file),readFileSync(new URL(`../src/${file}`,import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
    host = await createHost(join(dir,"reporting.ts"));
    const starting = host.emit({type:"session_start",reason:"startup"});
    const deadline = Date.now()+5000;
    while (!(() => { try { return readFileSync(calls,"utf8").includes("Recover"); } catch { return false; } })()) {
      assert.ok(Date.now()<deadline,"old recovery was admitted");
      await new Promise(r=>setTimeout(r,5));
    }
    const queued = host.tool("ovrcr_start",{project:"p",workspace:"w",scope:"Old queued work",acceptance_evidence:["citation"]});
    const outcome = queued.then(value=>({value}),error=>({error}));
    await host.emit({type:"session_start",reason:"new"});
    writeFileSync(gate,"1");
    await starting;
    const retired = await outcome;
    assert.match(String(retired.error?.message ?? retired.value?.details?.reason),/previous Pi context|retired/);
    assert.deepEqual(readFileSync(calls,"utf8").trim().split("\n"),["Recover"],"no stale Capabilities/Prepare/Launch/input may run with fresh routing");
  } finally {
    await host?.emit({type:"session_shutdown",reason:"quit"});
    process.env=before;
    rmSync(dir,{recursive:true,force:true});
  }
});

test("an awaited old private intent cannot reacquire auth after fork", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-retired-intent-"));
  const before = { ...process.env };
  let host, prototype, originalRead, release;
  try {
    const binary = join(dir,"control.mjs"), calls = join(dir,"calls");
    writeFileSync(binary, `#!/usr/bin/env node
import {appendFileSync} from 'node:fs';
let input='';for await(const bytes of process.stdin)input+=bytes;
if(process.argv.includes('report'))process.exit(0);
const request=JSON.parse(input);appendFileSync(${JSON.stringify(calls)},Object.keys(request)[0]+'\\n');console.log('[]');
`);
    chmodSync(binary,0o700);
    Object.assign(process.env,{OVRCR_AGENT_SOCKET:"/fixture/reporting",OVRCR_AGENT_TOKEN:"reporting",OVRCR_DELEGATION_SESSION:"1",OVRCR_DELEGATION_TOKEN:"control",OVRCR_DELEGATION_SOCKET:"/fixture/control",OVRCR_DELEGATION_HOME:dir});
    writeFileSync(join(dir,"reporting.ts"),readFileSync(new URL("../src/pi-reporting-extension.mjs",import.meta.url),"utf8").replace("__OVRCR_BINARY__",JSON.stringify(binary)));
    for(const file of ["ovrcr-reporting-transport.mjs","ovrcr-delegation-observer.mjs","ovrcr-delegation-continuation.mjs"])writeFileSync(join(dir,file),readFileSync(new URL(`../src/${file}`,import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-interrupt.ts"), readFileSync(new URL("../src/ovrcr-pi-interrupt.ts", import.meta.url)));
    mkdirSync(join(dir,"parent-intents"),{mode:0o700});
    const intent = join(dir,"parent-intents","original.json"), bytes = JSON.stringify({schema:1,request:"original",workflow:"old-workflow",controller:"old-controller"});
    writeFileSync(intent,bytes,{mode:0o600});
    host=await createHost(join(dir,"reporting.ts"));
    const probe=await open(intent,"r");prototype=Object.getPrototypeOf(probe);await probe.close();
    originalRead=prototype.read;
    let entered;
    const reading=new Promise(resolve=>{entered=resolve;});
    let held=false;
    prototype.read=async function(...args){if(!held){held=true;entered();await new Promise(resolve=>{release=resolve;});}return originalRead.apply(this,args);};
    const pending=host.tool("ovrcr_start",{handle:"original"});
    await reading;
    await host.emit({type:"session_start",reason:"fork"});
    release();
    await assert.rejects(pending,/previous Pi context/);
    assert.equal(readFileSync(intent,"utf8"),bytes,"old intent remains immutable");
    assert.throws(()=>readFileSync(calls),{code:"ENOENT"},"stale advance never spawns even Reconcile with fresh auth");
  } finally {
    release?.();if(prototype)prototype.read=originalRead;
    await host?.emit({type:"session_shutdown",reason:"quit"});
    process.env=before;rmSync(dir,{recursive:true,force:true});
  }
});


for (const operation of ["DeclareDelivery", "Archive"]) test(`${operation} persists an exact private intent before transport and reconciles a lost reply`, async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-delivery-intent-"));
  const before = {...process.env};
  let host;
  try {
    const binary=join(dir,"control.mjs"), calls=join(dir,"calls.jsonl"), lost=join(dir,"lost"), failure=join(dir,"failure");
    const prefix=operation === "Archive" ? "archive-" : "delivery-";
    writeFileSync(binary, `#!/usr/bin/env node
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
let input='';for await(const bytes of process.stdin)input+=bytes;
const wire=JSON.parse(input);const fields=wire.Archive?.archive ?? wire.DeclareDelivery?.declaration;
const saved=JSON.parse(readFileSync(${JSON.stringify(dir)}+'/parent-intents/'+${JSON.stringify(prefix)}+fields.request+'.json','utf8'));
if ((saved.declaration ?? (({schema,...rest})=>rest)(saved)).request !== fields.request) throw Error('intent was not durable before transport');
appendFileSync(${JSON.stringify(calls)},JSON.stringify(fields)+'\\n');
if(existsSync(${JSON.stringify(failure)})){console.error(JSON.stringify({error:{code:readFileSync(${JSON.stringify(failure)},'utf8'),message:'fixture authoritative rejection'}}));process.exit(1);}
if(!existsSync(${JSON.stringify(lost)})){writeFileSync(${JSON.stringify(lost)},'lost');process.exit(1);}
console.log(JSON.stringify(wire.Archive ? {handle:fields.handle,archive_receipts:[{request:fields.request,outcome:'fixture-recorded'}]} : [{deliveries:[{reference:{request:fields.request,digest:'a'.repeat(64)}}]}]));
`);
    chmodSync(binary,0o700);
    Object.assign(process.env,{OVRCR_AGENT_SOCKET:"/fixture/reporting",OVRCR_AGENT_TOKEN:"fixture-reporting",OVRCR_DELEGATION_SESSION:"1",OVRCR_DELEGATION_TOKEN:"fixture-secret",OVRCR_DELEGATION_SOCKET:"/fixture/control",OVRCR_DELEGATION_HOME:dir});
    writeFileSync(join(dir,"reporting.ts"),readFileSync(new URL("../src/pi-reporting-extension.mjs",import.meta.url),"utf8").replace("__OVRCR_BINARY__",JSON.stringify(binary)));
    for (const name of ["ovrcr-reporting-transport.mjs","ovrcr-delegation-observer.mjs","ovrcr-delegation-continuation.mjs"]) writeFileSync(join(dir,name),readFileSync(new URL(`../src/${name}`,import.meta.url)));
    writeFileSync(join(dir, "ovrcr-pi-runtime.ts"), readFileSync(new URL("../src/ovrcr-pi-runtime.ts", import.meta.url)));
    writeFileSync(join(dir,"ovrcr-pi-interrupt.ts"),readFileSync(new URL("../src/ovrcr-pi-interrupt.ts",import.meta.url)));
    host=await createHost(join(dir,"reporting.ts"));
    const name=operation === "Archive" ? "ovrcr_archive" : "ovrcr_deliver";
    const args=operation === "Archive" ? {handle:"assignment",expected_identity:{request:"assignment"},expected_result_version:1,expected_obligation_revision:1,expected_control_epoch:1,delivery:{request:"delivery",digest:"a".repeat(64)}} : {declaration:{workflow:"workflow",scope:"delivered finding",provenance:"parent decision",resources:[]}};
    const unknown=(await host.tool(name,args)).details;
    assert.equal(unknown.transport,"Unknown");
    assert.equal(unknown.dispatch,"Unknown","lost reply cannot claim a known refusal");
    assert.equal(unknown.archive_receipts,undefined,"transport cannot invent a Server outcome");
    assert.equal(unknown.deliveries,undefined);
    assert.ok(unknown.request);
    const saved=join(dir,"parent-intents",`${prefix}${unknown.request}.json`);
    const bytes=readFileSync(saved,"utf8");
    assert.ok(!bytes.includes("fixture-secret"));
    const recovery=operation === "Archive" ? {handle:"assignment",request:unknown.request} : {request:unknown.request};
    const result=(await host.tool(name,recovery)).details;
    assert.ok(operation === "Archive" ? result.archive_receipts : result.deliveries);
    const records=readFileSync(calls,"utf8").trim().split("\n").map(JSON.parse);
    assert.deepEqual(records[1],records[0],"lost reply preserves original public intent and target");
    assert.equal(readFileSync(saved,"utf8"),bytes,"reconciliation never rewrites immutable local intent");
    for (const code of ["Conflict","Internal"]) {
      writeFileSync(failure,code);
      const rejected=(await host.tool(name,recovery)).details;
      assert.equal(rejected.dispatch,code === "Conflict" ? "Refused" : "Unknown");
      assert.equal(rejected.reason,"fixture authoritative rejection");
      assert.equal(rejected.request,unknown.request);
      assert.equal(rejected.archive_receipts,undefined,"error classification invents no Server receipt");
      assert.equal(rejected.deliveries,undefined);
      assert.equal(readFileSync(saved,"utf8"),bytes);
    }
    if(operation === "Archive") {
      const foreign=(await host.tool(name,{handle:"different-assignment",request:unknown.request})).details;
      assert.equal(foreign.transport,"NotSent");
      assert.equal(readFileSync(calls,"utf8").trim().split("\n").length,4);
    }
  } finally {await host?.emit({type:"session_shutdown",reason:"quit"});process.env=before;rmSync(dir,{recursive:true,force:true});}
});

// Registered production tool callbacks with deterministic control transport only.
async function withPrepareOnlyHost(loss, run) {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-prepare-only-"));
  const before = { ...process.env };
  let host;
  try {
    const binary = join(dir, "control.mjs"), state = join(dir, "state.json"), calls = join(dir, "calls"), lost = join(dir, "lost");
    writeFileSync(binary, `#!/usr/bin/env node
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
let input='';for await(const bytes of process.stdin)input+=bytes;
const request=JSON.parse(input),operation=Object.keys(request)[0];
appendFileSync(${JSON.stringify(calls)},JSON.stringify(request)+'\\n');
let value=existsSync(${JSON.stringify(state)})?JSON.parse(readFileSync(${JSON.stringify(state)})):null;
if(operation==='Capabilities'){console.log(JSON.stringify({resolved_choice:{provider:'codex',model_provider:'openai',model:'fixture-model'},effective_limits:{max_live_children:1},capabilities:['read','result_file']}));process.exit(0);}
if(operation==='Prepare'){
 const contract=request.Prepare.contract;
 value??={handle:contract.request,identity:{instance:'i',workflow:contract.workflow,controller:contract.controller,controller_epoch:1,task:contract.task,attempt:1,contract:1,request:contract.request,session:null,run:null},dispatch:'Prepared',result_version:1,control:{epoch:1},receipts:[{operation:'Prepare',outcome:'Applied'}]};
 if(${JSON.stringify(loss)}!=='none'&&!existsSync(${JSON.stringify(lost)})){
  writeFileSync(${JSON.stringify(lost)},'lost');
  if(${JSON.stringify(loss)}==='after')writeFileSync(${JSON.stringify(state)},JSON.stringify(value));
  process.exit(1);
 }
}else if(!value){console.error(JSON.stringify({error:{code:'NotFound',message:'not found'}}));process.exit(1);}
if(operation==='Launch'){value.identity.session=41;value.identity.run=1;value.dispatch='Launched';value.receipts.push({operation:'Launch',outcome:'Applied'});}
if(operation==='SendInitial'){value.dispatch='Applied';value.receipts.push({operation:'InitialInput',outcome:'Applied'});}
writeFileSync(${JSON.stringify(state)},JSON.stringify(value));console.log(JSON.stringify(value));
`);
    chmodSync(binary, 0o700);
    Object.assign(process.env, {OVRCR_AGENT_SOCKET:"/fixture/reporting",OVRCR_AGENT_TOKEN:"fixture-reporting",OVRCR_DELEGATION_SESSION:"1",OVRCR_DELEGATION_TOKEN:"fixture-delegation",OVRCR_DELEGATION_SOCKET:"/fixture/control",OVRCR_DELEGATION_HOME:dir});
    writeFileSync(join(dir,"reporting.ts"),readFileSync(process.env.OVRCR_TEST_EXTENSION_SOURCE??new URL("../src/pi-reporting-extension.mjs",import.meta.url),"utf8").replace("__OVRCR_BINARY__",JSON.stringify(binary)));
    for(const name of ["ovrcr-reporting-transport.mjs","ovrcr-delegation-observer.mjs","ovrcr-delegation-continuation.mjs","ovrcr-pi-runtime.ts"])writeFileSync(join(dir,name),readFileSync(new URL(`../src/${name}`,import.meta.url)));
    writeFileSync(join(dir,"ovrcr-pi-interrupt.ts"),"export async function loadPiKeybindings(){return undefined}\nexport function inspectPiInterruptBinding(){return 'Unavailable'}\n");
    host=await createHost(join(dir,"reporting.ts"));
    const requests=()=>readFileSync(calls,"utf8").trim().split("\n").filter(Boolean).map(line=>JSON.parse(line));
    await run({host,dir,requests,args:{project:"p",workspace:"w",scope:"Reserved native login only; no task input.",provider:"codex",model_provider:"openai",model:"fixture-model",acceptance_evidence:["Reserved exact home; no child launch"],prepare_only:true}});
  } finally {await host?.emit({type:"session_shutdown",reason:"quit"});process.env=before;rmSync(dir,{recursive:true,force:true});}
}

for(const loss of ["none","before","after"])test(`prepare-only registered start reserves without effects and recovers ${loss} Prepare loss`,async()=>{
  await withPrepareOnlyHost(loss,async({host,dir,requests,args})=>{
    const first=(await host.tool("ovrcr_start",args)).details;
    if(loss!=="none"){
      assert.equal(first.dispatch,"Unknown");
      assert.deepEqual(first.recovery_arguments,{handle:first.handle,prepare_only:true},"prepare-only unknown must preserve non-advancing recovery arguments");
      assert.match(first.recovery,/Prepare only/);
    }
    assert.equal(requests().filter(row=>row.Launch||row.SendInitial).length,0,"prepare_only must never call Launch or SendInitial");
    const recovered=(await host.tool("ovrcr_start",{handle:first.handle,prepare_only:true})).details;
    const duplicate=(await host.tool("ovrcr_start",{handle:first.handle,prepare_only:true})).details;
    assert.equal(recovered.dispatch,"Prepared");
    assert.equal(recovered.identity.session,null);
    assert.equal(recovered.identity.run,null);
    assert.deepEqual(duplicate,recovered);
    assert.equal(requests().filter(row=>row.Launch||row.SendInitial).length,0,"duplicate Prepare must retain zero launched children and zero input");
    assert.ok(requests().every(row => row.Capabilities || row.Prepare), "Prepare-only must not even Reconcile or invoke another operation");
    const prepares=requests().filter(row=>row.Prepare).map(row=>row.Prepare);
    assert.equal(prepares.length, 3, "each Prepare-only call makes exactly one Prepare attempt");
    for(const value of prepares){assert.deepEqual(value.contract,prepares[0].contract);assert.deepEqual(value.auth,prepares[0].auth);}
    const intent=JSON.parse(readFileSync(join(dir,"parent-intents",`${first.handle}.json`)));
    assert.equal(intent.prepare_only,undefined,"transport mode must not change the canonical contract");
    const resumed=(await host.tool("ovrcr_start",{handle:first.handle,prepare_only:false})).details;
    assert.equal(resumed.dispatch,"Applied");
    assert.equal(resumed.handle,first.handle);
    assert.equal(resumed.identity.request,intent.request);
    assert.equal(requests().filter(row=>row.Launch).length,1,"explicit later resume launches once");
    assert.equal(requests().filter(row=>row.SendInitial).length,1,"explicit later resume delivers once");
  });
});

test("prepare-only invalid flags refuse before control or private intent effects",async()=>{
  await withPrepareOnlyHost("none",async({host,dir,args})=>{
    for(const value of [null,0,1,"true",{},[]]){
      const reply=(await host.tool("ovrcr_start",{...args,prepare_only:value})).details;
      assert.equal(reply.dispatch,"Refused");
      assert.equal(reply.transport,"NotSent");
      assert.throws(()=>readFileSync(join(dir,"calls")),{code:"ENOENT"},"invalid prepare_only must not call any control operation");
      assert.throws(()=>readFileSync(join(dir,"parent-intents")),{code:"ENOENT"},"invalid flag must not create private intents");
    }
    const invalidHandle=(await host.tool("ovrcr_start",{handle:"reserved",prepare_only:"true"})).details;
    assert.equal(invalidHandle.transport,"NotSent");
    assert.throws(()=>readFileSync(join(dir,"calls")),{code:"ENOENT"});
  });
});

test("Codex coding and resolved-default review refuse before intent or Prepare", async () => {
  await withPrepareOnlyHost("none", async ({host, dir, requests, args}) => {
    const coding = (await host.tool("ovrcr_start", {...args, coding: {requested_base:"main",branch:"work",authorized_paths:["."],required_checks:["check"]}})).details;
    assert.equal(coding.dispatch, "Refused");
    assert.match(coding.reason, /does not support coding permissions/);
    const {provider, ...defaults} = args;
    const review = (await host.tool("ovrcr_start", {...defaults, review: {}})).details;
    assert.equal(review.dispatch, "Refused");
    assert.equal(review.transport, "NotSent");
    assert.match(review.reason, /typed independent-review/);
    assert.equal(requests().length, 2);
    assert.ok(requests().every(row => row.Capabilities), "pre-effect capability refusals cannot Prepare");
    assert.equal(existsSync(join(dir, "parent-intents")), false);
  });
});
