import json,os,sys,subprocess
from pathlib import Path
root=Path(__file__).parent
payload=json.load(sys.stdin)
allowed={k:payload.get(k) for k in ['session_id','transcript_path','hook_event_name','source','turn_id','agent_id','agent_type'] if k in payload}
chain=[];pid=os.getppid()
for _ in range(12):
 p=subprocess.run(['/bin/ps','-o','pid=,ppid=,comm=','-p',str(pid)],capture_output=True,text=True)
 row=p.stdout.strip();chain.append(row)
 parts=row.split(None,2)
 if len(parts)<2 or parts[0]=='1':break
 pid=int(parts[1])
expected=json.loads((root/'expected-root.json').read_text())['pid']
allowed['ancestry']=chain
allowed['expected_root_pid']=expected
allowed['root_ancestor_present']=any(x.split(None,1)[0]==str(expected) for x in chain if x)
allowed['binding_decision']='not admitted: process route awaiting review'
(root/'hook-event.json').write_text(json.dumps(allowed,indent=2)+'\n')
