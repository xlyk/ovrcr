from pathlib import Path
import os,json
p=Path('/private/tmp/ovrcr-codex-auth-hbxxyxjr')
pid=os.getpid()
(p/'expected-root.json').write_text(json.dumps({'pid':pid})+'\n')
(p/'gui-launch.json').write_text(json.dumps({'pid':pid,'pgid':os.getpgrp(),'argv':['codex','--no-alt-screen','-C',str(p/'workspace'),'Reply exactly OVRCR_CODEX_AUTH_ROOT_OK.'],'credential_reuse_authorized':True})+'\n')
os.chdir(p/'workspace')
os.execve('/opt/homebrew/Caskroom/codex/0.153.0/bin/codex',['codex','--no-alt-screen','-C',str(p/'workspace'),'Reply exactly OVRCR_CODEX_AUTH_ROOT_OK.'],{'HOME':str(p/'home'),'CODEX_HOME':str(p/'provider'),'PATH':'/usr/bin:/bin:/usr/sbin:/sbin','TERM':'xterm-256color','LANG':'en_US.UTF-8'})
