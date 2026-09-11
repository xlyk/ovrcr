import os,pty,select,time,signal,subprocess,json,tempfile,fcntl,termios,struct
from pathlib import Path
base=Path(tempfile.mkdtemp(prefix="ovrcr-codex-auth-",dir="/private/tmp"))
os.chmod(base,0o700)
for n in ["home","provider","workspace"]:(base/n).mkdir(mode=0o700)
auth_path=base/"provider/auth.json"
source_auth=Path("/Users/xlyk/.codex/auth.json")
with source_auth.open("rb") as source, os.fdopen(os.open(auth_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),"wb") as target:
 import shutil
 shutil.copyfileobj(source,target)
(base/"provider/config.toml").write_text("""[[hooks.SessionStart]]
matcher = "startup|resume"
[[hooks.SessionStart.hooks]]
type = "command"
command = "/usr/local/bin/python3 NEVER"
""")
# Use an absolute verified interpreter and task-owned callback path.
import sys
hook=base/"hook.py"
hook.write_text("""import json,os,sys,subprocess
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
(root/'hook-event.json').write_text(json.dumps(allowed,indent=2)+'\\n')
""")
config='[[hooks.SessionStart]]\nmatcher = "startup|resume"\n[[hooks.SessionStart.hooks]]\ntype = "command"\ncommand = '+json.dumps(sys.executable+' '+str(hook))+'\n'
(base/"provider/config.toml").write_text(config)
exe="/opt/homebrew/Caskroom/codex/0.153.0/bin/codex"
argv=[exe,"--no-alt-screen","-C",str(base/"workspace"),"Reply exactly OVRCR_CODEX_AUTH_ROOT_OK."]
env={"HOME":str(base/"home"),"CODEX_HOME":str(base/"provider"),"PATH":"/usr/bin:/bin:/usr/sbin:/sbin","TERM":"xterm-256color","LANG":"en_US.UTF-8"}
record={"directory":str(base),"argv":argv,"environment_keys":sorted(env),"started_at":time.time(),"credentials_supplied":True,"credential_reuse_authorized":True,"credential_values_recorded":False,"input_sent":False}
gate_read,gate_write=os.pipe()
pid,fd=pty.fork()
if pid==0:
 os.close(gate_write)
 os.read(gate_read,1)
 os.close(gate_read)
 os.chdir(base/"workspace")
 os.execve(exe,argv,env)
os.close(gate_read)
record["pid"]=pid
(base/"expected-root.json").write_text(json.dumps({"pid":pid})+"\n")
os.write(gate_write,b"x")
os.close(gate_write)
(base/"record-start.json").write_text(json.dumps(record,indent=2)+"\n")
fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack("HHHH",35,100,0,0))
raw=bytearray();deadline=time.monotonic()+12
try:
 while time.monotonic()<deadline:
  ready,_,_=select.select([fd],[],[],0.25)
  if ready:
   try:chunk=os.read(fd,65536)
   except OSError:break
   if not chunk:break
   raw.extend(chunk)
   (base/"terminal.txt").write_bytes(raw)
   if b"\x1b[6n" in chunk:
    os.write(fd,b"\x1b[1;1R")
    record["terminal_cursor_response"]=True
   if len(raw)>262144:record["capture_limit"]=True;break
  # No user keystrokes or automatic approval responses.
 record["before_cleanup"]=subprocess.run(["/bin/ps","-o","pid=,ppid=,pgid=,tpgid=,tty=,stat=","-p",str(pid)],capture_output=True,text=True).stdout.strip()
 try:record["pgid"]=os.getpgid(pid)
 except ProcessLookupError:record["pgid"]=None
 if record["pgid"]==pid:
  os.killpg(pid,signal.SIGTERM)
 else:
  try:os.kill(pid,signal.SIGTERM)
  except ProcessLookupError:pass
 limit=time.monotonic()+3
 while True:
  done,status=os.waitpid(pid,os.WNOHANG)
  if done:record["wait_status"]=status;break
  if time.monotonic()>limit:
   if record["pgid"]==pid:os.killpg(pid,signal.SIGKILL)
   else:os.kill(pid,signal.SIGKILL)
   _,record["wait_status"]=os.waitpid(pid,0);break
  time.sleep(.05)
 try:os.killpg(pid,0);record["group_absent"]=False
 except ProcessLookupError:record["group_absent"]=True
 except PermissionError:record["group_absent"]="permission denied"
finally:
 auth_path.unlink(missing_ok=True)
 record["temporary_auth_removed"]=not auth_path.exists()
 (base/"terminal.txt").write_bytes(raw)
 (base/"record.json").write_text(json.dumps(record,indent=2)+"\n")
 os.close(fd)
(base/"terminal.txt").write_bytes(raw)
(base/"record.json").write_text(json.dumps(record,indent=2)+"\n")
print(json.dumps(record,indent=2));print(raw.decode("utf-8","replace"))
