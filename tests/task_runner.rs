use ovrcr::{
    task_runner,
    tasks::{self, Run, RunStatus},
};
use serde_json::json;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
fn prepare(mode: &str, timeout: u64) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("work")).unwrap();
    let run: Run = serde_json::from_value(json!({"id":1,"task_id":1,
        "spec":{"name":"test","prompt":mode,"target":"Scratch","schedule":{"Once":{"at":0}},"model":"fixture/model","thinking":"off","timeout_seconds":timeout},
        "trigger":"Manual","scheduled_at":0,"created_at":0,"started_at":null,"finished_at":null,
        "status":"Preparing","directory":root.join("work"),"workspace":null,"base_commit":null,"pi_version":null,"session_file":null,"error":null})).unwrap();
    tasks::write_run(root, &run).unwrap();
    fs::write(root.join("pi"), r#"#!/usr/bin/env python3
import json, sys, os, time, subprocess, signal
if '--version' in sys.argv:
 print('0.84.4'); sys.exit(0)
def emit(data): print(json.dumps(data), flush=True)
open('../args.json','w').write(json.dumps(sys.argv[1:]))
with open('../pid.tmp','w') as f: f.write(str(os.getpid()))
os.replace('../pid.tmp','../pid')
if os.path.exists('../unresponsive'): time.sleep(300)
for line in sys.stdin:
 req=json.loads(line)
 with open('../requests.jsonl','a') as f: f.write(line)
 if req['type']=='get_state': emit({'type':'response','command':'get_state','id':req['id'],'success':True,'data':{'sessionFile':os.path.abspath('../session.jsonl'),'sessionId':'test-session'}})
 if req['type']=='prompt':
  mode=req['message']
  emit({'type':'response','command':'prompt','id':req['id'],'success':mode!='reject','error':'bad provider' if mode=='reject' else None})
  if mode=='reject': continue
  if mode=='hang':
   child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(300)'],start_new_session=True)
   with open('../descendant.tmp','w') as f: f.write(str(child.pid))
   os.replace('../descendant.tmp','../descendant')
   continue
  if mode=='ack': continue
  print('separate diagnostic', file=sys.stderr, flush=True)
  emit({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'streamed text'}})
  emit({'type':'message_end','message':{'role':'assistant','stopReason':mode,'errorMessage':'provider detail' if mode=='error' else None}})
  emit({'type':'agent_end','messages':[]})
  if mode=='delay':
   while not os.path.exists('../settle'): time.sleep(.01)
  emit({'type':'agent_settled'})
"#).unwrap();
    fs::set_permissions(root.join("pi"), fs::Permissions::from_mode(0o755)).unwrap();
    temp
}
fn launch(root: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .arg("__task-runner")
        .arg(root)
        .arg(root.join("pi"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}
fn wait(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "supervisor failed: {:?}",
                child.stderr.take().map(|mut s| {
                    let mut b = String::new();
                    std::io::Read::read_to_string(&mut s, &mut b).unwrap();
                    b
                })
            );
            return;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("supervisor stuck");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "condition not reached");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn completion_retains_stream_logs_and_explicit_configuration() {
    let temp = prepare("stop", 5);
    let root = temp.path();
    let mut child = launch(root);
    wait(&mut child);
    let run = tasks::read_run(root).unwrap();
    assert_eq!(run.status, RunStatus::Succeeded, "{:?}", run.error);
    assert!(run.finished_at.is_some());
    assert_eq!(run.pi_version.as_deref(), Some("0.84.4"));
    assert!(run.session_file.is_some());
    let args: Vec<String> =
        serde_json::from_slice(&fs::read(root.join("args.json")).unwrap()).unwrap();
    for flag in [
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-approve",
    ] {
        assert!(args.iter().any(|a| a == flag));
    }
    assert!(!args.iter().any(|a| a == "--no-context-files"));
    for pair in [
        ["--provider", "fixture"],
        ["--model", "model"],
        ["--thinking", "off"],
    ] {
        assert!(args.windows(2).any(|w| w == pair));
    }
    let (bytes, end) = task_runner::read_log(root, 0, usize::MAX).unwrap();
    assert_eq!(end, bytes.len() as u64);
    assert!(String::from_utf8(bytes).unwrap().contains("streamed text"));
    assert!(
        fs::read_to_string(root.join("stderr.log"))
            .unwrap()
            .contains("separate diagnostic")
    );
}
#[test]
fn acknowledgement_and_agent_end_are_not_success() {
    let temp = prepare("delay", 5);
    let root = temp.path();
    let mut child = launch(root);
    until(|| {
        fs::read_to_string(root.join("events.jsonl"))
            .unwrap_or_default()
            .contains("agent_end")
    });
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(tasks::read_run(root).unwrap().status, RunStatus::Running);
    fs::write(root.join("settle"), "").unwrap();
    wait(&mut child);
    assert_eq!(tasks::read_run(root).unwrap().status, RunStatus::Failed);
}
#[test]
fn error_length_abort_and_preflight_rejection_fail() {
    for mode in ["error", "length", "aborted", "reject"] {
        let temp = prepare(mode, 5);
        let mut child = launch(temp.path());
        wait(&mut child);
        let run = tasks::read_run(temp.path()).unwrap();
        assert_eq!(run.status, RunStatus::Failed, "{mode}");
        assert!(run.error.is_some());
    }
}
#[test]
fn timeout_cancel_and_parent_loss_remove_detached_descendants() {
    for mode in ["timeout", "cancel", "parent_loss"] {
        let temp = prepare("hang", if mode == "timeout" { 1 } else { 20 });
        let root = temp.path();
        let mut child = launch(root);
        until(|| root.join("descendant").exists());
        let pid: i32 = fs::read_to_string(root.join("descendant"))
            .unwrap()
            .parse()
            .unwrap();
        if mode == "cancel" {
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"cancel\n")
                .unwrap();
        }
        if mode == "parent_loss" {
            drop(child.stdin.take());
        }
        wait(&mut child);
        assert_eq!(
            tasks::read_run(root).unwrap().status,
            match mode {
                "timeout" => RunStatus::TimedOut,
                "cancel" => RunStatus::Cancelled,
                _ => RunStatus::Interrupted,
            }
        );
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "detached process {pid} survived {mode}"
        );
    }
}
#[test]
fn logs_are_bounded_and_cursor_is_in_bytes() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("events.jsonl"), vec![b'x'; 200_000]).unwrap();
    let (bytes, end) = task_runner::read_log(temp.path(), 3, usize::MAX).unwrap();
    assert!(!bytes.is_empty());
    assert!(bytes.len() <= 64 * 1024);
    assert_eq!(end, 3 + bytes.len() as u64);
    let (bytes, end) = task_runner::read_log(temp.path(), 199_998, 20).unwrap();
    assert_eq!(bytes, b"xx");
    assert_eq!(end, 200_000);
}

#[test]
fn blocked_prompt_write_still_obeys_timeout() {
    let temp = prepare("stop", 1);
    let root = temp.path();
    let mut run = tasks::read_run(root).unwrap();
    run.spec.prompt = "x".repeat(2_000_000);
    tasks::write_run(root, &run).unwrap();
    fs::write(root.join("unresponsive"), "").unwrap();
    let mut child = launch(root);
    until(|| root.join("pid").exists());
    let deadline = Instant::now() + Duration::from_secs(4);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let finished = child.try_wait().unwrap().is_some();
    if !finished {
        let pid: i32 = fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert!(finished, "blocked Pi stdin prevented timeout");
    assert_eq!(tasks::read_run(root).unwrap().status, RunStatus::TimedOut);
}

// Uses the installed Pi loader/backend without a model or provider request.
#[test]
#[ignore = "requires OVRCR_TEST_PI_PACKAGE pointing at the installed Pi package and node on PATH"]
fn installed_pi_background_work_survives_between_tools_but_not_the_run() {
    let package = std::env::var("OVRCR_TEST_PI_PACKAGE").expect("set OVRCR_TEST_PI_PACKAGE");
    let temp = prepare("stop", 10);
    let root = temp.path();
    let adapter = root.join("pi.mjs");
    fs::write(&adapter, r#"#!/usr/bin/env node
import readline from 'node:readline';
import {readFileSync} from 'node:fs';
if(process.argv.includes('--version')) {console.log('0.84.4');process.exit(0);}
const packagePath=process.env.OVRCR_TEST_PI_PACKAGE;
const {createBashToolDefinition}=await import(packagePath+'/dist/core/tools/bash.js');
const {loadExtensions}=await import(packagePath+'/dist/core/extensions/loader.js');
const i=process.argv.indexOf('--extension');
const loaded=await loadExtensions(i<0?[]:[process.argv[i+1]],process.cwd());
if(loaded.errors.length) throw new Error(JSON.stringify(loaded.errors));
const registered=loaded.extensions.flatMap(e=>[...e.tools.values()]).find(t=>t.definition.name==='bash');
const tool=registered?.definition ?? createBashToolDefinition(process.cwd(),{exposeSessionEnvironment:false});
const ctx={cwd:process.cwd(),sessionManager:{getSessionId:()=> 'test',getSessionFile:()=> undefined}};
for await(const line of readline.createInterface({input:process.stdin})) {
 const cmd=JSON.parse(line);
 if(cmd.type==='prompt') {
  await tool.execute('start',{command:'sleep 30 >/dev/null 2>&1 & echo $! > ../descendant'},undefined,undefined,ctx);
  const pid=readFileSync('../descendant','utf8').trim();
  const result=await tool.execute('check',{command:`kill -0 ${pid} && echo BACKGROUND_ALIVE_BETWEEN_TOOLS`},undefined,undefined,ctx);
  if(!JSON.stringify(result).includes('BACKGROUND_ALIVE_BETWEEN_TOOLS')) throw new Error('background work stopped before next tool');
  console.log(JSON.stringify({type:'message_end',message:{role:'assistant',stopReason:'stop'}}));
  console.log(JSON.stringify({type:'agent_settled'}));
 }
}
for(const extension of loaded.extensions) for(const handler of extension.handlers.get('session_shutdown')??[]) await handler({type:'session_shutdown'},ctx);
"#).unwrap();
    fs::set_permissions(&adapter, fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .arg("__task-runner")
        .arg(root)
        .arg(&adapter)
        .env("OVRCR_TEST_PI_PACKAGE", package)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait(&mut child);
    let run = tasks::read_run(root).unwrap();
    let pid: i32 = fs::read_to_string(root.join("descendant"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    // Ensure a failed regression never leaves its disposable background child alive.
    if alive {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert_eq!(run.status, RunStatus::Succeeded, "{:?}", run.error);
    assert!(
        !alive,
        "Pi background child {pid} survived successful run cleanup"
    );
}

#[test]
#[ignore = "requires OVRCR_TEST_PI_PACKAGE pointing at the installed Pi package and node on PATH"]
fn installed_pi_task_shell_preserves_output_and_cleans_abort_timeout_and_parent_loss() {
    let package = std::env::var("OVRCR_TEST_PI_PACKAGE").expect("set OVRCR_TEST_PI_PACKAGE");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let probe = root.join("probe.mjs");
    fs::write(&probe, r#"
import {readFileSync,writeFileSync} from 'node:fs';
import assert from 'node:assert/strict';
const [pkg,extension,mode]=process.argv.slice(2);
const {loadExtensions}=await import(pkg+'/dist/core/extensions/loader.js');
const loaded=await loadExtensions([extension],process.cwd());
assert.equal(loaded.errors.length,0,JSON.stringify(loaded.errors));
const tool=loaded.extensions[0].tools.get('bash').definition;
const ctx={cwd:process.cwd(),sessionManager:{getSessionId:()=> 'test',getSessionFile:()=> undefined}};
try {
 if(mode==='output') {
  const result=await tool.execute('output',{command:`python3 -c 'import sys; sys.stdout.write("x"*2000000+"OUTPUT_TAIL_MARKER"); sys.stderr.write("STDERR_TAIL_MARKER")'`},undefined,undefined,ctx);
  const full=readFileSync(result.details.fullOutputPath,'utf8');
  assert.equal(full.split('x').length-1,2000000);
  assert.ok(full.includes('OUTPUT_TAIL_MARKER'));
  assert.ok(full.includes('STDERR_TAIL_MARKER'));
 } else {
  const control=new AbortController();
  const command='echo $$ > activepid; echo READY_TO_CANCEL; sleep 30';
  let ready=false;
  const update=value=>{
   if(!ready && JSON.stringify(value).includes('READY_TO_CANCEL')) {
    ready=true;
    if(mode==='abort') control.abort();
    if(mode==='parent_loss') process.kill(process.pid,'SIGKILL');
   }
  };
  await assert.rejects(tool.execute('active',{command,...(mode==='timeout'?{timeout:0.2}:{})},control.signal,update,ctx),mode==='timeout'?/timed out|timeout/:/aborted/);
 }
} finally {
 for(const extension of loaded.extensions) for(const handler of extension.handlers.get('session_shutdown')??[]) await handler({type:'session_shutdown'},ctx);
}
"#).unwrap();
    for mode in ["output", "timeout", "abort", "parent_loss"] {
        let _ = fs::remove_file(root.join("activepid"));
        let result = Command::new("node")
            .arg(&probe)
            .arg(&package)
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pi-task-extension.mjs"))
            .arg(mode)
            .current_dir(root)
            .output()
            .unwrap();
        let mut alive = false;
        if let Ok(pid) = fs::read_to_string(root.join("activepid")) {
            let pid: i32 = pid.trim().parse().unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            alive = unsafe { libc::kill(pid, 0) } == 0;
            if alive {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
        assert!(!alive, "tool child survived {mode}");
        if mode == "parent_loss" {
            assert!(
                root.join("activepid").exists(),
                "parent-loss command did not start"
            );
            assert!(!result.status.success());
        } else {
            assert!(
                result.status.success(),
                "{mode}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
