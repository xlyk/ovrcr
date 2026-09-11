import json,sys,subprocess,time,os
from pathlib import Path
p=Path(__file__).parent
raw=sys.stdin.buffer.read(65537)
if len(raw)>65536: sys.exit(0)
try: value=json.loads(raw)
except ValueError: sys.exit(0)
keys=['hook_event_name','session_id','prompt_id','agent_id','agent_type','source','reason','transcript_path','notification_type','stop_hook_active','tool_name']
record={k:value[k] for k in keys if k in value}
record['field_names']=sorted(value)
record['observed_ns']=time.time_ns()
if sys.argv[1]=='status':
 record.update({k:value[k] for k in ['context_window','cost'] if k in value})
 record['hook_event_name']='statusLine'
with (p/'signals.jsonl').open('a') as f: f.write(json.dumps(record)+'\n')
cmd=['/Users/xlyk/Code/ovrcr/.worktrees/claude-reporting/target/OVRCR GUI.app/Contents/MacOS/ovrcr','report','claude-statusline' if sys.argv[1]=='status' else 'claude','--stdin-json']
try:
 result=subprocess.run(cmd,input=raw,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=1.1)
 if sys.argv[1]=='status': sys.stdout.buffer.write(result.stdout)
except subprocess.TimeoutExpired: pass
