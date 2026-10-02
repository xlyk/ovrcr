#!/usr/bin/env python3
"""A deterministic stand-in for `codex app-server`, after the fixture in
tests/provider_quota.rs. The file `mode` beside this script picks the answer to
each rate-limit read: `slow:N` waits N seconds then answers, `ok` answers at
once, `http503` returns a native error whose body no Quota reason may echo."""
import json, os, sys, time
HERE = os.path.dirname(os.path.abspath(__file__))
if sys.argv[1:] == ["--version"]:
    print("codex-cli 0.155.1"); sys.exit(0)
def mode():
    try: return open(os.path.join(HERE, "mode")).read().strip()
    except OSError: return "ok"
def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n"); sys.stdout.flush()
for line in sys.stdin:
    request = json.loads(line)
    with open(os.path.join(HERE, "methods.log"), "a") as log:
        log.write(f"{time.strftime('%H:%M:%S')} {request['method']} {mode()}\n")
    if "id" not in request:
        continue
    method, rid = request["method"], request["id"]
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": rid, "result": {"capabilities": {}}})
    elif method == "account/read":
        send({"jsonrpc": "2.0", "id": rid, "result": {"account": {"type": "chatgpt", "email": "fixture@example.invalid", "planType": "plus"}}})
    elif method == "account/rateLimits/read":
        m = mode()
        if m.startswith("slow:"):
            time.sleep(int(m[5:]))
        if m == "http503":
            body = "FIXTURE-BODY-must-not-appear"
            send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32000, "message": body, "data": {"status": 503, "body": body}}})
            continue
        reset = int(time.time()) + 3600
        send({"jsonrpc": "2.0", "id": rid, "result": {"rateLimits": {"limitId": "codex",
            "primary": {"usedPercent": 63, "windowDurationMins": 300, "resetsAt": reset},
            "secondary": {"usedPercent": 31, "windowDurationMins": 10080, "resetsAt": reset + 86400}},
            "rateLimitsByLimitId": {}}})
    else:
        send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "unexpected"}})
