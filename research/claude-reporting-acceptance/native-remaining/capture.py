import sys,json,time,fcntl,subprocess
from pathlib import Path
root=Path(__file__).resolve().parent
raw=sys.stdin.buffer.read(65537)
try:
 obj=json.loads(raw)
 keys=['hook_event_name','session_id','prompt_id','source','reason','agent_id','agent_type','notification_type','stop_hook_active','transcript_path','agent_transcript_path','version']
 record={k:obj[k] for k in keys if k in obj}
 record['kind']=sys.argv[1]; record['observed_ns']=time.time_ns()
 if sys.argv[1]=='claude-statusline':
  for k in ['model','context_window','cost']:
   if k in obj: record[k]=obj[k]
 with (root/'capture.jsonl').open('a') as out:
  fcntl.flock(out,fcntl.LOCK_EX); out.write(json.dumps(record)+'\n')
except Exception:
 obj={}
result=subprocess.run(['/Users/xlyk/Code/ovrcr/.worktrees/claude-reporting/target/OVRCR GUI.app/Contents/MacOS/ovrcr','report',sys.argv[1],'--stdin-json'],input=raw,capture_output=True)
sys.stdout.buffer.write(result.stdout)
sys.stderr.buffer.write(result.stderr)
if obj.get('hook_event_name')=='Stop' and not obj.get('stop_hook_active') and (root/'block-next-stop').exists():
 (root/'block-next-stop').unlink()
 print(json.dumps({'decision':'block','reason':'Continue this acceptance turn: read approval-fixture.txt, then reply exactly OVRCR_CONTINUED_OK. Do not write files.'}))
sys.exit(result.returncode)
