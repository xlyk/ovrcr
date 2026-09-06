#!/usr/bin/env python3
"""Run the installed Pi through OVRCR against a controlled streaming provider.
Usage: python3 scripts/validate-pi-rpc.py /absolute/path/to/ovrcr /absolute/path/to/pi
Add --background to verify a bash background child survives between tool calls
and is removed by run cleanup.
Retains the fixture directory for inspecting run metadata and session output.
"""
import http.server
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time

binary, pi = map(lambda arg: str(Path(arg).resolve()), sys.argv[1:3])
root = Path(tempfile.mkdtemp(prefix="ovrcr-real-pi-"))
background = "--background" in sys.argv[3:]
background_pid = None
release = threading.Event()
requests = []

class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        def emit(delta, finish=None):
            event = {"id":"fixture", "object":"chat.completion.chunk", "created":int(time.time()), "model":"model", "choices":[{"index":0,"delta":delta,"finish_reason":finish}]}
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
            self.wfile.flush()
        if background and len(requests) == 1:
            command = "sleep 120 >/dev/null 2>&1 & echo $! > background.pid"
            emit({"role":"assistant", "tool_calls":[{"index":0,"id":"background-tool","type":"function","function":{"name":"bash","arguments":json.dumps({"command":command})}}]})
            emit({}, "tool_calls")
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
            return
        emit({"role":"assistant", "content":"REAL_PI_FIRST\n"})
        if not release.wait(20):
            return
        emit({"content":"REAL_PI_DONE"})
        emit({}, "stop")
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

provider = http.server.ThreadingHTTPServer(("127.0.0.1",0), Provider)
threading.Thread(target=provider.serve_forever, daemon=True).start()
agent = root / "agent"
agent.mkdir()
(agent / "models.json").write_text(json.dumps({"providers":{"fixture":{"baseUrl":f"http://127.0.0.1:{provider.server_port}/v1", "api":"openai-completions", "apiKey":"fixture", "models":[{"id":"model","name":"fixture", "reasoning":False,"input":["text"],"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"contextWindow":8192,"maxTokens":1024}]}}}))
env = dict(os.environ, OVRCR_CONFIG=str(root / "config.toml"), OVRCR_SOCKET=str(root / "server.sock"), OVRCR_PI_EXECUTABLE=pi, PI_CODING_AGENT_DIR=str(agent))

def call(*args, raw=False):
    result = subprocess.run([binary, "--json", *args], env=env, capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise RuntimeError(result.stderr)
    return result.stdout if raw else json.loads(result.stdout)

try:
    task = call("task", "create", "real-pi", "--scratch", "--every", "1d", "--model", "fixture/model", "--timeout", "30s", "--prompt", "Reply with the fixture greeting.")
    run = call("task", "run", str(task["id"]))
    run_id = str(run["id"])
    deadline = time.monotonic() + 20
    while "REAL_PI_FIRST" not in call("run", "logs", run_id, raw=True):
        assert time.monotonic() < deadline, call("run", "get", run_id)
        time.sleep(.05)
    assert call("run", "get", run_id)["status"] == "Running"
    assert not release.is_set(), "provider was released before streaming evidence"
    if background:
        run = call("run", "get", run_id)
        background_pid = int((Path(run["directory"]) / "background.pid").read_text().strip())
        os.kill(background_pid, 0)
        assert len(requests) == 2, requests
        assert any(message.get("role") == "tool" for message in requests[-1]["messages"]), requests[-1]

    release.set()
    while True:
        run = call("run", "get", run_id)
        if run["status"] == "Succeeded":
            break
        assert time.monotonic() < deadline, run
        time.sleep(.05)
    assert "REAL_PI_DONE" in call("run", "logs", run_id, raw=True)
    assert Path(run["session_file"]).is_file(), run
    if background:
        try:
            os.kill(background_pid, 0)
        except ProcessLookupError:
            background_pid = None
        else:
            raise AssertionError(f"background child {background_pid} survived Succeeded")
    print(json.dumps({"background_cleanup":background, "result":"PASS", "pi_version":run["pi_version"], "stream_before_completion":True, "status":run["status"], "session_file":run["session_file"], "provider_requests":len(requests), "fixture":str(root)}, indent=2))
finally:
    release.set()
    subprocess.run([binary,"shutdown","--kill"],env=env,capture_output=True,timeout=30)
    provider.shutdown()
    if background_pid is not None:
        try:
            os.kill(background_pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
