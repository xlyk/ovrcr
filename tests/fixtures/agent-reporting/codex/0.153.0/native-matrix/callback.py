import json,socket,sys
from pathlib import Path
p=Path(__file__).parent
raw=sys.stdin.buffer.read(65537)
if len(raw)>65536:sys.exit(0)
x=json.loads(raw)
allow=['session_id','transcript_path','hook_event_name','source','turn_id','agent_id','agent_type','tool_name','tool_use_id','notification_type','reason']
y={k:x[k] for k in allow if k in x}
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.settimeout(.8)
try:
 s.connect(str(p/'observer.sock'));s.sendall(json.dumps(y).encode()+b'\n');s.recv(16)
except (OSError,ValueError):pass
finally:s.close()
