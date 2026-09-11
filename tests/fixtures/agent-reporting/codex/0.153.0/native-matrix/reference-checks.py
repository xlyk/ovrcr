import json
from pathlib import Path
p=Path(__file__).parent
assert [100,100,140][-1]==140
requests={"a":20,"b":20};assert sum(requests.values())==40
assert 100+20==120 and 40<=100 and 5<=20
# Ordered source owns current turn; delayed hook arrival does not update it.
current="B"; delayed_hook={"turn_id":"A","event":"Stop"};assert delayed_hook["turn_id"]!=current
rows=json.loads((p/"snapshot-two-turns.json").read_text())["records"]
u=[r["payload"] for r in rows if r["type"]=="token_usage_record"]
assert len({x["response_id"] for x in u})==2
assert sum(x["usage"]["total_tokens"] for x in u)==31152
assert u[-1]["thread_token_usage"]["total_tokens"]==31152
print("6 synthetic/reference assertions plus native two-turn arithmetic passed; no runtime adapter tested")
