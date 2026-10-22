#!/usr/bin/env node
// Command-boundary fixture only: never contacts GitHub or reads credentials.
import {readFileSync,writeFileSync,lstatSync} from "node:fs";
import {join,isAbsolute} from "node:path";

const root=process.env.OVRCR_TEST_WORKFLOW_TRACKER_DIR;
if (!root || !isAbsolute(root)) throw new Error("private fixture tracker directory required");
const stat=lstatSync(root);
if (!stat.isDirectory() || stat.isSymbolicLink() || stat.uid!==process.getuid() || (stat.mode&0o077)) throw new Error("fixture tracker must be owned and private");
const file=join(root,"issues.json");
const state=JSON.parse(readFileSync(file,"utf8"));
const args=process.argv.slice(2);
const value=flag=>{const index=args.indexOf(flag);return index<0?undefined:args[index+1];};
const save=()=>writeFileSync(file,JSON.stringify(state),{mode:0o600});
if (args[0]!=="issue") throw new Error("fixture supports only gh issue commands");
if (args[1]==="create") {
  const title=value("--title"), bodyFile=value("--body-file");
  if (!title || !bodyFile) throw new Error("title and approved body file required");
  const body=readFileSync(bodyFile,"utf8");
  state.create_attempts=(state.create_attempts??0)+1;
  if (state.issues.some(issue=>issue.title===title && issue.body===body)) {
    save(); console.error("duplicate fixture issue; reconcile the original publication"); process.exit(1);
  }
  const number=Math.max(0,...state.issues.map(issue=>issue.number))+1;
  const issue={number,title,body,url:`https://example.invalid/ovrcr/issues/${number}`,state:"OPEN",comments:[],labels:value("--label")?.split(",")??[],blockedBy:value("--blocked-by")?.split(",")??[]};
  state.issues.push(issue);
  if (process.env.OVRCR_TEST_WORKFLOW_CREATE_REPLY==="lose-once" && !state.lost_reply) {
    state.lost_reply=true; save(); console.error("fixture lost reply after publication"); process.exit(124);
  }
  save(); console.log(issue.url);
} else if (args[1]==="list") {
  console.log(JSON.stringify(state.issues));
} else if (args[1]==="view") {
  const number=Number(args[2]?.split("/").at(-1));
  const issue=state.issues.find(issue=>issue.number===number);
  if (!issue) {console.error("fixture issue not found");process.exit(1);}
  console.log(args.includes("--json")?JSON.stringify(issue):`${issue.title}\n${issue.url}\n${issue.body}`);
} else {
  throw new Error("unsupported fixture tracker action; no production fallback");
}
