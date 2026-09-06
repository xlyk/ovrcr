#!/usr/bin/env python3
"""Exercise an isolated macOS LaunchAgent; never controls the default service."""
import datetime
import json
import os
from pathlib import Path
import plistlib
import re
import signal
import subprocess
import sys
import tempfile
import time

assert sys.platform == "darwin", "this probe requires macOS"
binary = str(Path(sys.argv[1]).resolve())
root = Path(tempfile.mkdtemp(prefix="ovrcr-service-probe-"))
pi = root / "pi-fixture"
pi.write_text("""#!/usr/bin/env python3
import sys,json
if '--version' in sys.argv:print('0.84.4');sys.exit(0)
for line in sys.stdin:
 c=json.loads(line);kind=c['type']
 print(json.dumps({'type':'response','id':c.get('id'),'command':kind,'success':True,'data':{}}),flush=True)
 if kind=='prompt':
  print(json.dumps({'type':'message_end','message':{'role':'assistant','stopReason':'stop','content':[]}}),flush=True)
  print(json.dumps({'type':'agent_settled'}),flush=True)
""")
pi.chmod(0o755)
envfile = root / "service.env"
envfile.write_text(f"OVRCR_PI_EXECUTABLE={pi}\n")
env = dict(os.environ, OVRCR_CONFIG=str(root/"config.toml"), OVRCR_SOCKET=str(root/"server.sock"))

def call(*args):
 p=subprocess.run([binary,"--json",*args],env=env,text=True,capture_output=True,timeout=30)
 if p.returncode:raise RuntimeError(f"{args}: {p.stderr}")
 return json.loads(p.stdout)

def wait(predicate, seconds=20):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  value=predicate()
  if value:return value
  time.sleep(.1)
 raise TimeoutError("service probe condition timed out")

label=None
try:
 call("service","install","--environment-file",str(envfile))
 definition=Path(call("service","status")["definition"])
 data=plistlib.loads(definition.read_bytes());label=data["Label"]
 assert label != "com.ovrcr.server" and data["EnvironmentVariables"]["OVRCR_CONFIG"] == str(root/"config.toml")
 target=f"gui/{os.getuid()}/{label}"
 def pid():
  p=subprocess.run(["/bin/launchctl","print",target],capture_output=True,text=True,timeout=5)
  match=re.search(r"^\s*pid = (\d+)",p.stdout,re.M)
  return int(match[1]) if match else None
 wait(lambda:call("service","status")["running"])
 at=(datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=3)).isoformat()
 task=call("task","create","scheduled-service-probe","--scratch","--at",at,"--model","fixture/model","--prompt","local fixture")
 run=wait(lambda: next((r for r in call("run","list") if r["status"]=="Succeeded"),None))
 old=pid();assert old
 os.kill(old,signal.SIGKILL)
 new=wait(lambda: (value if (value:=pid()) and value != old else None),30)
 wait(lambda:call("service","status")["running"])
 call("shutdown","--kill")
 wait(lambda:not call("service","status")["running"])
 # launchd reports the clean exit; the definition restarts only unsuccessful exits.
 assert data["KeepAlive"]["SuccessfulExit"] is False
 call("service","start");wait(lambda:call("service","status")["running"])
 call("service","stop");assert not call("service","status")["running"]
 call("service","uninstall");assert not definition.exists()
 assert call("task","get",str(task["id"]))["id"] == task["id"]
 assert call("run","get",str(run["id"]))["status"] == "Succeeded"
 print(json.dumps({"result":"PASS","label":label,"scheduled_run":run["id"],"restart_pids":[old,new],"uninstalled":True,"history_retained":True,"fixture":str(root)},indent=2))
finally:
 try:call("service","uninstall")
 except Exception as error:print(f"cleanup check: {error}; fixture={root}",file=sys.stderr)
