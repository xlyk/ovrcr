// Deterministic model work for dependency tests; runtime/Git/publication stay real.
import assert from "node:assert/strict";
import {readFileSync,writeFileSync,mkdirSync,unlinkSync,chmodSync,symlinkSync,lstatSync,readlinkSync,existsSync} from "node:fs";
import {join,dirname} from "node:path";
import {spawnSync} from "node:child_process";
import {createHash} from "node:crypto";

export function dependencyResult(brief,path,submission) {
  const config=JSON.parse(brief.contract.scope.slice("DEPENDENCY_FIXTURE\n".length));
  const root=process.cwd();
  const artifact=name=>{assert.match(name,/^[^/]+(?:\/[^/]+)*$/);assert(!name.split("/").some(part=>part==="."||part===".."));return join(root,name);};
  for (const expected of config.expect??[]) {
    const file=artifact(expected.path);
    if (expected.missing) {assert(!existsSync(file));continue;}
    if (expected.symlink!==undefined) assert.equal(readlinkSync(file),expected.symlink);
    else assert.equal(readFileSync(file,"utf8"),expected.content);
    if (expected.executable!==undefined) assert.equal(Boolean(lstatSync(file).mode&0o111),expected.executable);
  }
  if (brief.contract.review) {
    const review=brief.contract.review;
    for (const source of review.source_documents) assert.equal(createHash("sha256").update(source.content).digest("hex"),source.source.digest);
    assert.equal(createHash("sha256").update(readFileSync(review.target.coding.candidate.path)).digest("hex"),review.target.coding.candidate.digest);
    submission.review={target:review.target,axis:review.axis,outcome:"Passed",findings:[]};
    submission.evidence=["exact immutable candidate and axis source bytes verified"];
    return;
  }
  assert.equal(root,brief.coding_workspace.path);
  for (const output of config.outputs??[]) {
    const file=artifact(output.path);
    mkdirSync(dirname(file),{recursive:true});
    if (output.missing) {unlinkSync(file);continue;}
    if (output.symlink!==undefined) symlinkSync(output.symlink,file);
    else {writeFileSync(file,output.content);chmodSync(file,output.executable?0o755:0o644);}
  }
  const checked=spawnSync("/bin/sh",["-c",'test -f "$1"',"dependency-check",artifact(config.check_path)],{encoding:"utf8"});
  if (checked.error) throw checked.error;
  writeFileSync(`${path}.dependency-check.log`,`test -f ${config.check_path}: exit ${checked.status}\n`);
  submission.outcome=checked.status===0?"success":"failed";
  submission.summary="Verified actual inherited inputs and performed scoped output/check.";
  submission.evidence=[`${path}.dependency-check.log`];
  submission.coding={candidate:null,checks:[{name:"dependency-check",command:`test -f ${config.check_path}`,outcome:checked.status===0?"Passed":"Failed",exit_code:checked.status,evidence:[`${path}.dependency-check.log`]}]};
}
