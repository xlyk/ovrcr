import json, os, subprocess, sys
from pathlib import Path
root=Path(sys.argv[1])
p=root/'provider-resume'
e=Path('/Users/xlyk/Code/ovrcr/.worktrees/claude-reporting/research/claude-reporting-acceptance/native-resume-integrated')
b='/Users/xlyk/Code/ovrcr/.worktrees/claude-reporting/target/OVRCR GUI.app/Contents/MacOS/ovrcr'
env=os.environ.copy(); env.update(OVRCR_CONFIG=str(root/'config.toml'),OVRCR_SOCKET=str(root/'server.sock'))
label=sys.argv[2]; sid=sys.argv[3]
state=subprocess.check_output([b,'session','usage',sid],env=env,text=True)
(e/(label+'.json')).write_text(state)
rows=[]
for line in subprocess.check_output(['ps','-axo','pid=,ppid=,pgid=,tpgid=,tty=,args='],text=True).splitlines():
 f=line.split(None,5)
 if len(f)==6: rows.append(dict(pid=int(f[0]),ppid=int(f[1]),pgid=int(f[2]),tpgid=int(f[3]),tty=f[4],command=f[5]))
owned={int(sys.argv[4])}
while True:
 new=owned|{r['pid'] for r in rows if r['ppid'] in owned}
 if new==owned: break
 owned=new
current=[r for r in rows if r['pid'] in owned]
(e/(label+'-processes.json')).write_text(json.dumps(current,indent=2)+'\n')
f=e/'owned-processes.json'; previous={r['pid']:r for r in json.loads(f.read_text())} if f.exists() else {}
previous.update({r['pid']:r for r in current});f.write_text(json.dumps(list(previous.values()),indent=2)+'\n')
signals=[json.loads(l) for l in (p/'signals.jsonl').read_text().splitlines()]
for name in ['signals.jsonl','launches.jsonl']:(e/name).write_bytes((p/name).read_bytes())
sources={r['transcript_path'] for r in signals if r.get('hook_event_name')=='SessionStart' and 'transcript_path' in r}
usage=[]
for source in sources:
 path=Path(source)
 if not path.exists(): continue
 st=path.stat()
 for line in path.read_text().splitlines():
  try:r=json.loads(line)
  except ValueError:continue
  if r.get('type')!='assistant':continue
  m=r.get('message',{})
  usage.append({'path':source,'device':st.st_dev,'inode':st.st_ino,'sessionId':r.get('sessionId'),'uuid':r.get('uuid'),'requestId':r.get('requestId'),'message_id':m.get('id'),'isSidechain':r.get('isSidechain'),'usage':m.get('usage')})
(e/(label+'-usage.json')).write_text(json.dumps(usage,indent=2)+'\n')
print(state)
print('owned processes',len(current),'usage records',len(usage))
