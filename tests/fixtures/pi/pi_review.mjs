// Deterministic model work only; publication still uses the real registered child tool.
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, unlinkSync, existsSync, symlinkSync, chmodSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const digest = bytes => createHash("sha256").update(bytes).digest("hex");
const checked = bytes => spawnSync(process.execPath, ["-e", `${bytes.toString("utf8")}\nconst observed=module.exports.add(2,3);console.log(observed);if(observed!==5)process.exit(7);`], {encoding:"utf8"});

export function applyReviewFixture(brief, path, submission, host) {
  if (brief.contract.review) {
    const assignment = brief.contract.review;
    for (const source of assignment.source_documents) {
      if (digest(source.content) !== source.source.digest) throw new Error("fixture source digest changed");
    }
    const bytes = readFileSync(assignment.target.coding.candidate.path);
    if (digest(bytes) !== assignment.target.coding.candidate.digest) throw new Error("fixture candidate digest changed");
    const {snapshot} = JSON.parse(bytes);
    const base = spawnSync("git", ["-C",snapshot.repository,"cat-file","-e",`${snapshot.base}^{commit}`], {encoding:"utf8"});
    if (base.status !== 0) throw new Error("fixture exact base context missing");
    const original = spawnSync("git", ["-C",snapshot.repository,"show",`${snapshot.base}:obsolete.txt`], {encoding:"utf8"});
    if (original.status !== 0 || original.stdout !== "original base context\n") throw new Error("fixture complete original base source context missing");
    const file = snapshot.files.find(file => file.path === "calc.cjs");
    if (!file?.content) throw new Error("fixture complete untracked candidate omitted");
    const artifact = Buffer.from(file.content,"base64");
    if (digest(artifact) !== file.digest) throw new Error("fixture artifact digest changed");
    const result = checked(artifact);
    if (result.error || ![0,7].includes(result.status)) throw new Error("fixture executable assertion did not run");
    writeFileSync(`${path}.artifact-check`, JSON.stringify({exit:result.status,target:assignment.target,files:snapshot.files.map(file=>({path:file.path,mode:file.mode}))}));
    const source = assignment.target.sources[0];
    const blocked = assignment.axis === "Spec" && assignment.source_documents.some(source => source.content.includes("REVIEW_FIXTURE_HUMAN_DECISION"));
    const bad = assignment.axis === "Spec" && result.status !== 0;
    submission.review = {target:assignment.target,axis:assignment.axis,outcome:blocked ? "Blocked" : bad ? "ChangesRequired" : "Passed",findings:bad ? [{id:"addition",blocking:true,location:"calc.cjs:1",finding:`add(2,3) returns ${result.stdout.trim()}; implement addition.`,source}] : []};
    submission.outcome = blocked ? "blocked" : bad ? "failed" : "success";
    submission.summary = blocked ? "The supplied task leaves a real human decision unresolved." : bad ? "Pinned untracked artifact fails executable addition assertion." : "This independent axis has no blocking findings.";
    submission.evidence = [`${path}.artifact-check`];
    return true;
  }
  if (!brief.contract.scope.startsWith("REVIEW_FIXTURE_")) return false;
  if (!brief.contract.coding || process.cwd() !== brief.coding_workspace.path) throw new Error("review worker is outside assigned workspace");
  const stalled = brief.contract.scope.includes("STALLED");
  const progress = brief.contract.scope.includes("PROGRESS");
  const correct = brief.contract.scope.includes("CLEAN") || (!stalled && !progress && (brief.result_version ?? 1) > 1);
  const expression = correct ? "a + b" : progress && (brief.result_version ?? 1) > 1 ? `a * b + ${(brief.result_version ?? 1) - 2}` : "a - b";
  const code = `module.exports = { add: (a, b) => ${expression} };\n`;
  writeFileSync(join(process.cwd(),"calc.cjs"),code);
  writeFileSync(join(process.cwd(),"README"),`candidate version ${brief.result_version ?? 1}\n`);
  const obsolete = join(process.cwd(),"obsolete.txt");
  if (existsSync(obsolete)) unlinkSync(obsolete);
  const script = join(process.cwd(),"executable.sh");
  writeFileSync(script,"#!/bin/sh\nexit 0\n"); chmodSync(script,0o755);
  const link = join(process.cwd(),"calc-link");
  if (!existsSync(link)) symlinkSync("calc.cjs",link);
  const result = checked(Buffer.from(code));
  if (result.error || result.status !== (correct ? 0 : 7)) throw new Error("worker executable assertion did not run");
  writeFileSync(`${path}.artifact-check`, JSON.stringify({exit:result.status}));
  submission.outcome = correct ? "success" : "failed";
  submission.summary = correct ? "Corrected actual addition behavior." : "Actual addition assertion failed.";
  submission.coding = {checks:[{name:"addition",command:"node -e addition assertion",outcome:correct ? "Passed" : "Failed",exit_code:result.status,evidence:[`${path}.artifact-check`]}],candidate:null};
  if (brief.contract.scope.includes("OPTIONAL")) {
    const optional = spawnSync("/bin/sh", ["-c","false"], {encoding:"utf8"});
    if (optional.error || optional.status !== 1) throw new Error("optional fixture assertion did not execute");
    submission.coding.checks.push(
      {name:"optional-failure",command:"false",outcome:"Failed",exit_code:optional.status,evidence:[]},
      {name:"optional-missing",command:null,outcome:"Missing",exit_code:null,evidence:[]},
      {name:"optional-unrun",command:null,outcome:"Unrun",exit_code:null,evidence:[]},
    );
  }
  if (brief.contract.scope.includes("MISSING")) submission.coding.checks = [];
  if (brief.contract.scope.includes("REBIND_AT_REPAIR") && (brief.result_version ?? 1) === 1) {
    if (!host?.state || typeof host.emit !== "function") throw new Error("review rebind fixture requires the actual Pi host");
    const original = host.state.session;
    void (async () => {
      const deadline = performance.now() + 20000;
      while (!existsSync(`${path}.repair-rebind`)) {
        if (performance.now() >= deadline) throw new Error("owned repair rebind gate timed out");
        await new Promise(resolve => setTimeout(resolve,5));
      }
      for (const session of ["review-replacement-B",original]) {
        host.state.session = session;
        await host.emit({type:"session_start",reason:"new"});
      }
      writeFileSync(`${path}.repair-rebound`,"actual A-to-B-to-A reporter cycles completed");
    })().catch(error => {
      writeFileSync(`${path}.repair-rebind-error`,error.message);
      process.exitCode = 1;
    });
  }
  return true;
}
