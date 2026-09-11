import os,pty,select,time,signal,subprocess,json,tempfile,fcntl,termios,struct
from pathlib import Path
base=Path(tempfile.mkdtemp(prefix="ovrcr-codex-native-",dir="/private/tmp"))
for n in ["home","provider","workspace"]:(base/n).mkdir()
exe="/opt/homebrew/Caskroom/codex/0.153.0/bin/codex"
argv=[exe,"--no-alt-screen","-C",str(base/"workspace")]
env={"HOME":str(base/"home"),"CODEX_HOME":str(base/"provider"),"PATH":"/usr/bin:/bin:/usr/sbin:/sbin","TERM":"xterm-256color","LANG":"en_US.UTF-8"}
record={"directory":str(base),"argv":argv,"environment_keys":sorted(env),"started_at":time.time(),"credentials_supplied":False,"input_sent":False}
pid,fd=pty.fork()
if pid==0:
 os.chdir(base/"workspace")
 os.execve(exe,argv,env)
record["pid"]=pid
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
 (base/"terminal.txt").write_bytes(raw)
 (base/"record.json").write_text(json.dumps(record,indent=2)+"\n")
 os.close(fd)
(base/"terminal.txt").write_bytes(raw)
(base/"record.json").write_text(json.dumps(record,indent=2)+"\n")
print(json.dumps(record,indent=2));print(raw.decode("utf-8","replace"))
