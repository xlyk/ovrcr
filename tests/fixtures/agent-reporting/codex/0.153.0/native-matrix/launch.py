from pathlib import Path
import json,os,subprocess,sys
p=Path(__file__).parent;pid=os.getpid()
identity=subprocess.check_output(['/bin/ps','-o','pid=,lstart=','-p',str(pid)],text=True).strip()
(p/'expected.json').write_text(json.dumps({'pid':pid,'native_identity':identity})+'\n')
with (p/'evidence/native-launches.jsonl').open('a') as f:f.write(json.dumps({'pid':pid,'pgid':os.getpgrp(),'native_identity':identity,'mode':'fresh'})+'\n')
os.chdir(p/'workspace')
os.execve('/opt/homebrew/Caskroom/codex/0.153.0/bin/codex',['codex','--no-alt-screen','-C',str(p/'workspace'),'Reply exactly OVRCR_MATRIX_ONE.'],{'HOME':str(p/'home'),'CODEX_HOME':str(p/'provider'),'PATH':'/usr/bin:/bin:/usr/sbin:/sbin','TERM':'xterm-256color','LANG':'en_US.UTF-8'})
