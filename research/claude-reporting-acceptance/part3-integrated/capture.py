import json,sys,subprocess,time,os,fcntl
from pathlib import Path
p=Path(__file__).parent
raw=sys.stdin.buffer.read(65537)
if len(raw)>65536: sys.exit(0)
try: value=json.loads(raw)
except ValueError: sys.exit(0)
keys=['hook_event_name','session_id','prompt_id','agent_id','agent_type','source','reason','transcript_path','notification_type','stop_hook_active','tool_name','agent_transcript_path']
record={k:value[k] for k in keys if k in value}
record['field_names']=sorted(value)
record['observed_ns']=time.time_ns()
if sys.argv[1]=='status':
 record.update({k:value[k] for k in ['context_window','cost'] if k in value})
 record['hook_event_name']='statusLine'
child=bool(value.get('agent_id')) or value.get('hook_event_name','').startswith('Subagent')
b='/Users/xlyk/Code/ovrcr/.worktrees/claude-reporting/target/OVRCR GUI.app/Contents/MacOS/ovrcr'
def snapshot():
 try:return json.loads(subprocess.check_output([b,'session','usage',(p/'inspection-session').read_text().strip()],text=True,timeout=.2,env={**os.environ,'OVRCR_CONFIG':str(p.parent/'config.toml'),'OVRCR_SOCKET':str(p.parent/'server.sock')}))
 except Exception:return {'snapshot_unavailable':True}
if child:record['before']=snapshot()
cmd=[b,'report','claude-statusline' if sys.argv[1]=='status' else 'claude','--stdin-json']
try:
 result=subprocess.run(cmd,input=raw,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=1.1)
 record['forward_exit']=result.returncode
 if sys.argv[1]=='status': sys.stdout.buffer.write(result.stdout)
except subprocess.TimeoutExpired:record['forward_timeout']=True
if child:record['after']=snapshot()
with (p/'signals.jsonl').open('a') as f:
 fcntl.flock(f,fcntl.LOCK_EX);f.write(json.dumps(record)+'\n')

if value.get('hook_event_name')=='Stop' and (p/'block-next-stop').exists():
 (p/'block-next-stop').unlink()
 print(json.dumps({'decision':'block','reason':'For this acceptance test, continue once and reply exactly OVRCR_268_CONTINUED_OK without tools.'}))
 with (p/'hook-decisions.jsonl').open('a') as f:f.write(json.dumps({'observed_ns':time.time_ns(),'session_id':value.get('session_id'),'prompt_id':value.get('prompt_id'),'decision':'block'})+'\n')
