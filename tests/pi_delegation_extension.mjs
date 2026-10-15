import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, rmSync, chmodSync, symlinkSync, appendFileSync } from "node:fs";
import { open } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHost } from "./fixtures/pi/pi_host.mjs";

test("managed Pi exposes research assignment and explicit collection tools", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-delegation-"));
  const before = { ...process.env };
  try {
    process.env.OVRCR_AGENT_SOCKET = "/fixture/socket";
    process.env.OVRCR_AGENT_TOKEN = "fixture-token";
    const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify("/fixture/ovrcr"));
    writeFileSync(join(dir, "reporting.mjs"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    writeFileSync(join(dir, "ovrcr-delegation-observer.mjs"), readFileSync(new URL("../src/ovrcr-delegation-observer.mjs", import.meta.url)));
    const host = await createHost(join(dir, "reporting.mjs"));
    assert.deepEqual(host.tools(), ["ovrcr_accept", "ovrcr_collect", "ovrcr_followup", "ovrcr_inspect", "ovrcr_mark_handled", "ovrcr_publish", "ovrcr_start", "ovrcr_wait"]);
    assert.deepEqual(host.toolDefinition("ovrcr_publish").parameters.required, ["handle", "submission"]);
    assert.deepEqual(host.toolDefinition("ovrcr_collect").parameters.required, ["handle"]);
    assert.deepEqual(host.toolDefinition("ovrcr_start").parameters.anyOf, [{ required: ["handle"] }, { required: ["project", "workspace", "scope", "model_provider", "model", "acceptance_evidence"] }]);
    assert.deepEqual(host.toolDefinition("ovrcr_followup").parameters.anyOf, [{required:["handle","request"]},{required:["handle","expected_identity","expected_result_version","text"]}]);
    assert.deepEqual(host.toolDefinition("ovrcr_accept").parameters.required, ["handle","identity","result_version","result_digest"]);
    const malformed = await host.tool("ovrcr_followup", {handle:"assignment",expected_identity:{},expected_result_version:1,text:""});
    assert.equal(malformed.details.dispatch, "Refused");
    assert.equal(malformed.details.transport, "NotSent");
    await assert.rejects(() => host.tool("ovrcr_accept", {}), /missing required argument: handle/);
    const missing = await host.tool("ovrcr_start", {});
    assert.equal(missing.details.dispatch, "Refused");
    assert.equal(missing.details.handle, null);
    await assert.rejects(() => host.tool("ovrcr_inspect", {}), /missing required argument: handle/);
  } finally {
    process.env = before;
    rmSync(dir, { recursive: true, force: true });
  }
});

for (const loss of ["before", "after"]) test(`original intent recovers a lost Prepare ${loss} persistence`, async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-original-intent-"));
  const before = { ...process.env };
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
if (operation === 'Prepare') {
  const contract = request.Prepare.contract;
  value ??= { handle: contract.request, identity: {instance:'i',workflow:contract.workflow,controller:contract.controller,controller_epoch:1,task:contract.task,attempt:1,contract:1,request:contract.request,session:null,run:null}, dispatch:'Prepared', receipts:[] };
  if (!existsSync(${JSON.stringify(join(dir, "lost"))})) {
    writeFileSync(${JSON.stringify(join(dir, "lost"))}, 'lost');
    if (${JSON.stringify(loss)} === 'after') writeFileSync(${JSON.stringify(state)}, JSON.stringify(value));
    process.exit(1);
  }
  value.receipts = [{operation:'Prepare',outcome:'Applied'}];
} else if (!value) { console.error(JSON.stringify({error:{code:'NotFound',message:'assignment not found'}})); process.exit(1); }
if (operation === 'Launch') { value.identity.session = 41; value.identity.run = 1; value.dispatch = 'Launched'; value.receipts.push({operation:'Launch',outcome:'Applied'}); }
if (operation === 'SendInitial') { value.dispatch = 'Applied'; value.receipts.push({operation:'InitialInput',outcome:'Applied'}); }
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
    writeFileSync(join(dir, "reporting.mjs"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    const host = await createHost(join(dir, "reporting.mjs"));
    const first = (await host.tool("ovrcr_start", {project:"p",workspace:"w",scope:"Read",model_provider:"test",model:"fixture-model",acceptance_evidence:["citation"]})).details;
    assert.equal(first.dispatch, "Unknown");
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
  } finally { process.env = before; rmSync(dir, {recursive:true,force:true}); }
});

test("followup recovery retains exact private request after a lost reply", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-followup-intent-"));
  const before = { ...process.env };
  try {
    const binary = join(dir, "control.mjs"), calls = join(dir, "calls"), lost = join(dir, "lost");
    writeFileSync(binary, `#!/usr/bin/env node
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
let input='';for await(const bytes of process.stdin)input+=bytes;
const fields=JSON.parse(input).Followup;
appendFileSync(${JSON.stringify(calls)},JSON.stringify(fields)+'\\n');
if(!existsSync(${JSON.stringify(lost)})){writeFileSync(${JSON.stringify(lost)},'lost');process.exit(1);}
console.log(JSON.stringify({handle:fields.handle,identity:fields.expected_identity,dispatch:'Applied',receipts:[{request:fields.request,operation:'Followup',outcome:'Applied'}]}));
`);
    chmodSync(binary, 0o700);
    Object.assign(process.env, {OVRCR_AGENT_SOCKET:"/fixture/reporting",OVRCR_AGENT_TOKEN:"fixture-reporting",OVRCR_DELEGATION_SESSION:"1",OVRCR_DELEGATION_TOKEN:"fixture-delegation",OVRCR_DELEGATION_SOCKET:"/fixture/control",OVRCR_DELEGATION_HOME:dir});
    const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify(binary));
    writeFileSync(join(dir, "reporting.mjs"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    const host = await createHost(join(dir, "reporting.mjs"));
    const first = (await host.tool("ovrcr_followup", {handle:"assignment",expected_identity:{request:"assignment",contract:1},expected_result_version:1,text:"Clarify the scoped result"})).details;
    assert.equal(first.dispatch,"Unknown");
    assert.equal(first.receipts,undefined);
    const recovered = (await host.tool("ovrcr_followup", {handle:"assignment",request:first.request})).details;
    assert.equal(recovered.dispatch,"Applied");
    const records=readFileSync(calls,"utf8").trim().split("\n").map(JSON.parse);
    assert.deepEqual(records[1],records[0],"recovery sends the identical operation identity and payload");
    const saved=join(dir,"parent-intents",`followup-${first.request}.json`);
    const bytes=readFileSync(saved,"utf8");
    assert.ok(!bytes.includes("fixture-delegation"),"persist no capability in task payloads");
    rmSync(saved);symlinkSync(calls,saved);
    const unsafe=(await host.tool("ovrcr_followup",{handle:"assignment",request:first.request})).details;
    assert.equal(unsafe.dispatch,"Refused");
    assert.equal(readFileSync(calls,"utf8").trim().split("\n").length,2,"unsafe original intent cannot contact control");
  } finally {process.env=before;rmSync(dir,{recursive:true,force:true});}
});
