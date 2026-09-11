import socket,struct,json,os,subprocess,time,signal
from pathlib import Path
p=Path(__file__).parent;path=p/'observer.sock'
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.bind(str(path));os.chmod(path,0o600);s.listen(8);s.settimeout(.5)
(p/'evidence/observer-process.json').write_text(json.dumps({'pid':os.getpid(),'pgid':os.getpgrp()})+'\n')
seq=0;bound=None
try:
 while not (p/'stop-observer').exists():
  try:c,_=s.accept()
  except socket.timeout:continue
  with c:
   c.settimeout(.8);seq+=1;row={'sequence':seq,'received_at':time.time()}
   try:
    peer=struct.unpack('i',c.getsockopt(0,2,4))[0]
    info=subprocess.check_output(['/bin/ps','-o','pid=,ppid=,lstart=','-p',str(peer)],text=True).strip().split(None,2)
    parent=int(info[1]);expected=json.loads((p/'expected.json').read_text())
    # Keep peer connected until ancestry is checked and acknowledgment sent.
    row.update(peer_pid=peer,os_parent_pid=parent,expected_native_pid=expected['pid'])
    native=subprocess.check_output(['/bin/ps','-o','pid=,lstart=','-p',str(expected['pid'])],text=True).strip()
    row['native_lifetime_matches']=native==expected['native_identity']
    raw=b''
    while not raw.endswith(b'\n') and len(raw)<=65536:
     chunk=c.recv(4096)
     if not chunk:break
     raw+=chunk
    event=json.loads(raw);row['event']=event
    same_parent=parent==expected['pid'] and row['native_lifetime_matches']
    if not same_parent:row['decision']='reject_foreign_parent'
    elif event.get('agent_id') or event.get('hook_event_name','').startswith('Subagent'):row['decision']='ignore_child'
    elif event.get('hook_event_name')=='SessionStart' and event.get('source')=='startup':
     sid=event.get('session_id');fp=Path(event.get('transcript_path') or '')
     if bound is not None and bound!=sid:row['decision']='reject_conflicting_root'
     elif not fp.is_relative_to(p/'provider'):row['decision']='reject_path'
     else:
      with fp.open() as f:header=json.loads(f.readline())
      if header.get('type')!='session_meta' or header.get('payload',{}).get('id')!=sid:row['decision']='reject_header'
      else:
       bound=sid;row['decision']='accept_root';row['header']={k:header['payload'][k] for k in ['id','cli_version','source','history_mode'] if k in header['payload']}
       (p/'binding.json').write_text(json.dumps({'thread_id':sid,'path':str(fp),'native_pid':expected['pid']})+'\n')
    elif bound and event.get('session_id')==bound:row['decision']='accept_root_event'
    else:row['decision']='ignore_unbound'
    c.sendall(b'ok\n')
   except Exception as err:
    row['error']=type(err).__name__+': '+str(err)
   with (p/'evidence/callbacks.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
finally:s.close();path.unlink(missing_ok=True)
