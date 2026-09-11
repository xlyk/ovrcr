import os,json
from pathlib import Path
p=Path(__file__).parent
native='/opt/homebrew/Caskroom/codex/0.153.0/bin/codex'
helper='/Users/xlyk/Code/ovrcr/.worktrees/codex-reporting/target/debug/ovrcr'
values={'HOME':str(p/'home'),'CODEX_HOME':str(p/'provider'),'PATH':'/usr/bin:/bin:/usr/sbin:/sbin','TERM':'xterm-256color','LANG':'en_US.UTF-8'}
for key in ['OVRCR_CONFIG','OVRCR_SOCKET','OVRCR_HOOK_SOCKET','OVRCR_SESSION_ID','OVRCR_HOOK_TOKEN']:
 if key in os.environ:values[key]=os.environ[key]
(p/'evidence/launch.json').write_text(json.dumps({'supervisor_pid':os.getpid(),'supervisor_pgid':os.getpgrp(),'native':native,'helper':helper,'environment_keys':sorted(values),'ovrcr_config':values.get('OVRCR_CONFIG'),'ovrcr_socket':values.get('OVRCR_SOCKET'),'session_id':values.get('OVRCR_SESSION_ID')},indent=2)+'\n')
os.execve(helper,[helper,'agent','run','codex','--',native,'--no-alt-screen','-C',str(p/'workspace')],values)
