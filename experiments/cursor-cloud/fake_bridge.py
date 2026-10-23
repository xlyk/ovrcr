"""Synthetic external process, packaged by test_client.py; never used by the client."""
import http.server
import json
import os
from pathlib import Path
import signal
import socket
import socketserver
import sys
import tempfile
import threading
import time

CONFIG = FIXTURE_CONFIG
KEY = "API-SECRET-SENTINEL-363"
TOKEN = "BRIDGE-SECRET-SENTINEL-363"

def receipt(**items):
    with open(CONFIG["audit"], "a") as f:
        f.write(json.dumps(items) + "\n")

args = dict(zip(sys.argv[1::2], sys.argv[2::2]))
workspace = args["--workspace"]
state = args["--state-root"]
scenario = CONFIG.get("scenario", "success")
if scenario == "startup_stall":
    receipt(stall=scenario)
    time.sleep(30)
if scenario == "startup_trickle":
    receipt(stall=scenario)
    while True:
        sys.stderr.write("diagnostic " + KEY + " " + TOKEN + "\n")
        sys.stderr.flush()
        time.sleep(0.02)
token_dir = Path(tempfile.mkdtemp(prefix="cursor-sdk-bridge-"))
token_file = token_dir / "auth-token"
fd = os.open(token_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
os.write(fd, TOKEN.encode())
os.close(fd)
if scenario == "token_mode":
    token_file.chmod(0o644)
if scenario == "token_symlink":
    token_file.unlink()
    token_file.symlink_to(CONFIG["unrelated"])
receipt(pid=os.getpid(), token_file=str(token_file), root=str(Path(workspace).parent),
        env_key_matches=os.environ.get("CURSOR_API_KEY") == KEY,
        argv_excludes_secrets=KEY not in str(sys.argv) and TOKEN not in str(sys.argv))

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        method = self.path.rsplit("/", 1)[-1]
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        receipt(method=method, key_matches=body.get("options", {}).get("apiKey") == KEY)
        good_auth = self.headers.get("Authorization") == "Bearer " + TOKEN
        if method == "Shutdown":
            if scenario == "shutdown_stall":
                time.sleep(30)
            response = {}
            threading.Thread(target=lambda: (time.sleep(0.03), os._exit(0)), daemon=True).start()
        elif not good_auth and scenario != "bad_bearer_accepted":
            self.send_response(401)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"code": "unauthenticated", "message": KEY + TOKEN}).encode())
            return
        elif method == "Ping":
            response = {"message": "pong"}
        elif method == "GetVersion":
            response = {"bridgeVersion": "1.0.0", "protocolVersion": "sdk.v1", "capabilities": []}
            if scenario == "version_mismatch":
                response["protocolVersion"] = "sdk.v2"
        elif method == "Me":
            response = {"user": {"userId": "123", "userEmail": KEY, "apiKeyName": TOKEN}}
            if scenario == "key_rejected":
                self.send_response(401)
                self.end_headers()
                return
        elif method == "ListModels":
            response = {"items": [{"id": "fixture-model" if scenario != "model_mismatch" else "other"}]}
        elif method == "ListRepositories":
            response = {"items": [{"url": "https://github.com/fixture/disposable" if scenario != "repo_mismatch" else "https://github.com/other/repo"}]}
        else:
            response = {"error": "unexpected method"}
        if method == "Me" and scenario == "body_trickle":
            response["user"]["userEmail"] += "a" * 4000
        data = json.dumps(response).encode()
        if method == "ListRepositories" and scenario == "token_replaced":
            token_file.unlink()
            token_file.symlink_to(CONFIG["unrelated"])
        if method == "Me" and scenario in ("error_stall", "error_trickle"):
            self.send_response(401)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", "2000")
            self.end_headers()
            receipt(stall=scenario)
            for byte in b'{"code":"unauthenticated","message":"' + KEY.encode() + b'a' * 2000:
                self.wfile.write(bytes([byte]))
                self.wfile.flush()
                time.sleep(30 if scenario == "error_stall" else 0.02)
            return
        if method == "Me" and scenario == "headers_stall":
            self.wfile.write(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n")
            self.wfile.flush()
            receipt(stall=scenario)
            time.sleep(30)
            return
        if method == "Me" and scenario == "headers_trickle":
            receipt(stall=scenario)
            for byte in b"HTTP/1.1 200 OK\r\nX-Long: " + b"a" * 2000:
                self.wfile.write(bytes([byte]))
                self.wfile.flush()
                time.sleep(0.02)
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if method == "Me" and scenario in ("body_stall", "body_trickle"):
            receipt(stall=scenario)
            for byte in data:
                self.wfile.write(bytes([byte]))
                self.wfile.flush()
                time.sleep(30 if scenario == "body_stall" else 0.03)
        else:
            self.wfile.write(data)

if scenario == "shutdown_stall":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
class LoopbackHTTPServer(http.server.ThreadingHTTPServer):
    def server_bind(self):
        # The synthetic loopback peer needs its bound port, not reverse DNS.
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]

# Controlled resolver faults reproduce work that HTTPServer performs before ready.
if scenario in ("dns_stall", "dns_error"):
    def failed_resolution(host):
        receipt(dns_lookup=host)
        if scenario == "dns_stall":
            time.sleep(30)
        raise OSError("synthetic reverse DNS unavailable")
    socket.getfqdn = failed_resolution
server = LoopbackHTTPServer((args["--host"], int(args["--port"])), Handler)
ready = {"schemaVersion": 1, "serverVersion": "1.0.0", "pid": os.getpid(),
         "transport": "tcp", "protocol": "connect", "host": "127.0.0.1",
         "port": server.server_port, "url": "http://127.0.0.1:" + str(server.server_port),
         "authTokenFile": str(token_file), "workspaceRef": workspace, "stateRoot": state}
if scenario == "ready_array":
    ready = []
if scenario == "ready_null":
    ready = None
if scenario == "ready_scalar":
    ready = 7
if scenario == "foreign_token":
    ready["authTokenFile"] = CONFIG["unrelated"]
sys.stderr.write("diagnostic " + KEY + TOKEN + "\n")
sys.stderr.write("cursor-sdk-bridge ready " + json.dumps(ready) + "\n")
sys.stderr.flush()
server.serve_forever()
