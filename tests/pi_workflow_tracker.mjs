// This qualifies only the command-boundary stub, not model stage compliance.
import {test} from "node:test";
import assert from "node:assert/strict";
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync} from "node:fs";
import {join} from "node:path";
import {tmpdir} from "node:os";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";

const command=fileURLToPath(new URL("./fixtures/pi/workflow-gh.mjs",import.meta.url));
function fixture(run) {
  const root=mkdtempSync(join(tmpdir(),"ovrcr-workflow-tracker-"));
  const tracker=join(root,"tracker");mkdirSync(tracker,{mode:0o700});
  const parent={number:317,title:"Approved parent",body:"original parent body",state:"OPEN",url:"https://example.invalid/ovrcr/issues/317",comments:[]};
  const ledger=join(tracker,"issues.json");writeFileSync(ledger,JSON.stringify({issues:[parent]}),{mode:0o600});
  const body=join(root,"approved-ticket.md");writeFileSync(body,"## Parent\nhttps://example.invalid/ovrcr/issues/317\n\n## What to build\nOne approved slice.\n");
  const call=(args,extra={})=>spawnSync(process.execPath,[command,...args],{encoding:"utf8",env:{...process.env,OVRCR_TEST_WORKFLOW_TRACKER_DIR:tracker,...extra}});
  try {run({call,body,ledger,parent});} finally {rmSync(root,{recursive:true,force:true});}
}

test("lost create reply is attributable through ordinary issue list without a duplicate publication",()=>fixture(({call,body,ledger,parent})=>{
  const result=call(["issue","create","--title","Approved slice","--body-file",body,"--label","ready-for-agent","--blocked-by","317"],{OVRCR_TEST_WORKFLOW_CREATE_REPLY:"lose-once"});
  assert.equal(result.status,124);
  const reconciled=call(["issue","list","--state","all","--json","number,title,body,url"]);
  assert.equal(reconciled.status,0);
  const matches=JSON.parse(reconciled.stdout).filter(issue=>issue.title==="Approved slice" && issue.body===readFileSync(body,"utf8"));
  assert.equal(matches.length,1);assert.equal(matches[0].url,"https://example.invalid/ovrcr/issues/318");
  assert.deepEqual(matches[0].blockedBy,["317"]);
  const state=JSON.parse(readFileSync(ledger,"utf8"));
  assert.equal(state.create_attempts,1);assert.deepEqual(state.issues[0],parent);
}));

test("the bounded tracker stub exposes duplicate attempts and refuses parent modification",()=>fixture(({call,body,ledger,parent})=>{
  const args=["issue","create","--title","Approved slice","--body-file",body];
  assert.equal(call(args).status,0);
  assert.equal(call(args).status,1);
  assert.notEqual(call(["issue","close","317"]).status,0);
  assert.notEqual(call(["api","graphql"]).status,0);
  const state=JSON.parse(readFileSync(ledger,"utf8"));
  assert.equal(state.issues.length,2);assert.equal(state.create_attempts,2);assert.deepEqual(state.issues[0],parent);
}));
