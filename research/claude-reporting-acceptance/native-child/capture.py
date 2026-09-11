import sys,json,time,fcntl,subprocess
from pathlib import Path
p=Path(__file__).resolve().parent
b='/Users/xlyk/Code/ovrcr/.worktrees/claude-reporting/target/OVRCR GUI.app/Contents/MacOS/ovrcr'
raw=sys.stdin.buffer.read(65537)
try: obj=json.loads(raw)
except Exception: obj={}
keys='hook_event_name session_id prompt_id source reason agent_id agent_type notification_type stop_hook_active transcript_path agent_transcript_path version'.split()
record={k:obj[k] for k in keys if k in obj};record['kind']=sys.argv[1];record['observed_ns']=time.time_ns()
child=bool(obj.get('agent_id')) or obj.get('hook_event_name')=='SubagentStop'
def snapshot():
 try:return json.loads(subprocess.check_output([b,'session','usage','11'],text=True,timeout=.2))
 except Exception:return {'snapshot_unavailable':True}
if child:record['before']=snapshot()
result=subprocess.run([b,'report',sys.argv[1],'--stdin-json'],input=raw,capture_output=True)
if child:record['after']=snapshot()
record['forward_exit']=result.returncode
with (p/'capture.jsonl').open('a') as out:
 fcntl.flock(out,fcntl.LOCK_EX);out.write(json.dumps(record)+'\n')
sys.stdout.buffer.write(result.stdout);sys.stderr.buffer.write(result.stderr);sys.exit(result.returncode)
