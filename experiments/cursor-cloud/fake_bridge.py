"""Synthetic external process, packaged by test_client.py; never used by the client."""
import http.server
import json
import os
from pathlib import Path
import signal
import stat
import socket
import socketserver
import struct
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

def journal_rows():
    return [json.loads(line) for line in (Path(CONFIG["journal"]) / "events.jsonl").read_text().splitlines()]


def wait_record(predicate):
    end = time.monotonic() + 1
    while time.monotonic() < end:
        if any(predicate(row) for row in journal_rows()):
            return
        time.sleep(0.002)
    raise RuntimeError("synthetic deterministic journal gate was not reached")


def close_admission(kind):
    os.kill(os.getppid(), signal.SIGUSR1 if kind == "cancel" else signal.SIGUSR2)
    wait_record(lambda row: row["event"] == "closure" and row["kind"] == kind)


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        method = self.path.rsplit("/", 1)[-1]
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        body = json.loads(raw[5:] if method == "Send" else raw)
        ahead = None
        if method in {"CreateAgent", "Send"}:
            journal = Path(CONFIG["journal"])
            records = [json.loads(line) for line in (journal / "events.jsonl").read_text().splitlines()]
            saved = json.loads((journal / (method + ".request.json")).read_text())
            semantic = json.loads(json.dumps(body))
            semantic.get("options", {}).pop("apiKey", None)
            ahead = saved["request"] == semantic and any(x.get("event") == "claim" and x.get("method") == method and x.get("operation_key") == body["idempotencyKey"] for x in records)
            ahead = ahead and stat.S_IMODE(journal.stat().st_mode) == 0o700 and all(stat.S_IMODE(p.stat().st_mode) == 0o600 for p in journal.iterdir())
            if method == "Send":
                ahead = ahead and any(x.get("method") == "CreateAgent" and x.get("event") == "receipt" for x in records)
        shape = True
        if method == "CreateAgent":
            expected = {"apiKey": KEY, "model": {"id": "fixture-model"}, "cloud": {
                "repos": [{"url": "https://github.com/fixture/disposable", "startingRef": CONFIG["source_sha"]}],
                "workOnCurrentBranch": False, "autoCreatePr": False}}
            shape = set(body) == {"options", "idempotencyKey"} and body.get("options") == expected
            shape = shape and body["options"]["cloud"]["workOnCurrentBranch"] is False and body["options"]["cloud"]["autoCreatePr"] is False
        if method == "GetAgent":
            shape = body == {"agentId": "bc-fixture", "options": {"apiKey": KEY}}
        if method == "Send":
            shape = set(body) == {"agentId", "message", "options", "idempotencyKey"} and body["agentId"] == "bc-fixture" and body["options"] == {"model": {"id": "fixture-model"}}
        receipt(method=method, request_shape=shape, key_matches=body.get("options", {}).get("apiKey") == KEY, write_ahead=ahead,
                exact_cancel=body == {"agentId": "bc-fixture", "runId": "run-fixture"} if method == "CancelRun" else None)
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
        elif method == "CreateAgent":
            if scenario == "creation_lost":
                self.connection.shutdown(socket.SHUT_RDWR)
                return
            response = {"agentId": "bc-fixture", "model": {"id": "fixture-model"}}
            if scenario in {"cancel_after_create", "discard_after_create"}:
                close_admission("cancel" if scenario == "cancel_after_create" else "discard")
            if scenario == "receipt_capacity":
                (Path(CONFIG["journal"]) / "capacity").write_bytes(b"x" * (256 * 1024))
            if scenario == "secret_response_key":
                response[KEY] = "unexpected property"
            if scenario == "secret_nested_key":
                response["extra"] = [{KEY: "unexpected nested property"}]
            if scenario == "bridge_token_key":
                response[TOKEN] = "unexpected property"
            if scenario == "creation_no_id":
                response.pop("agentId")
            if scenario == "creation_wrong_model":
                response["model"] = {"id": "wrong"}
        elif method == "GetAgent":
            response = {"agent": {"agentId": "bc-fixture", "cloud": {"repos": ["https://github.com/fixture/disposable"]}}}
            if scenario == "close_before_send":
                close_admission("discard")
            if scenario == "get_agent_wrong_id":
                response["agent"]["agentId"] = "wrong-agent"
            if scenario == "get_agent_local":
                response["agent"]["local"] = {"cwd": "/wrong"}
            if scenario == "get_agent_wrong_repo":
                response["agent"]["cloud"]["repos"] = ["https://github.com/other/repo"]
        elif method == "CancelRun":
            if scenario == "cancel_stall":
                time.sleep(30)
            response = {}
        elif method == "GetRun":
            response = {"run": {"agentId": "bc-fixture", "runId": "run-fixture", "status": "RUN_LIFECYCLE_STATUS_CANCELLED"}}
            if scenario == "cancel_unconfirmed":
                response["run"]["status"] = "RUN_LIFECYCLE_STATUS_RUNNING"
        elif method == "Send":
            identity = {"agent_id": "bc-fixture", "run_id": "run-fixture"}
            def sdk(kind, **fields):
                return {"sdkMessage": {"type": kind, "message": {**identity, **fields}}}
            probe = {"call_id": "source-probe", "name": "shell", "args": {"command": "git rev-parse HEAD && cat SOURCE_MARKER"}}
            result = {"agentId": "bc-fixture", "runId": "run-fixture", "model": {"id": "fixture-model"},
                      "status": "RUN_LIFECYCLE_STATUS_FINISHED", "git": {"branches": [{"repoUrl": "https://github.com/fixture/disposable", "branch": "cursor/result"}]}}
            messages = [{}, {"futureEnvelope": {"opaque": True}},
                        sdk("system", subtype="init", model={"id": "fixture-model"}),
                        sdk("tool_call", **probe, status="running"),
                        sdk("tool_call", **probe, status="completed", result={"stdout": CONFIG["source_sha"] + "\nOVRCR_SELECTED_BASELINE_364\n", "stderr": "", "exitCode": 0}),
                        {"result": {"agentId": "bc-fixture", "runId": "run-fixture", "status": "RUN_LIFECYCLE_STATUS_FINISHED", "result": result}},
                        {"done": {"agentId": "bc-fixture", "runId": "run-fixture"}}]
            if scenario == "result_bad_git":
                result["git"] = []
            if scenario == "result_bad_branch":
                result["git"]["branches"] = [7]
            if scenario == "sdk_bad_payload":
                messages.insert(3, {"sdkMessage": []})
            if scenario == "probe_bool_exit":
                messages[4]["sdkMessage"]["message"]["result"]["exitCode"] = False
            if scenario == "wrong_source":
                messages[4]["sdkMessage"]["message"]["result"]["stdout"] = "b" * 40 + "\nOVRCR_DEFAULT_TIP_364\n"
            if scenario == "assistant_source_only":
                messages[3:5] = [sdk("assistant", message={"content": [{"type": "text", "text": CONFIG["source_sha"] + " OVRCR_SELECTED_BASELINE_364"}]})]
            if scenario == "late_source_probe":
                messages.insert(3, sdk("tool_call", call_id="edit-first", name="edit", status="completed", args={}, result={}))
            if scenario in {"wrong_model", "terminal_wrong_model"}:
                result["model"] = {"id": "wrong"}
            if scenario == "failed_run":
                result["status"] = "RUN_LIFECYCLE_STATUS_ERROR"
                messages[5]["result"]["status"] = result["status"]
            if scenario == "early_no_id":
                messages[2]["sdkMessage"]["message"].pop("run_id")
            if scenario == "early_wrong_agent":
                messages[2]["sdkMessage"]["message"]["agent_id"] = "wrong-agent"
            if scenario == "early_wrong_model":
                messages[2]["sdkMessage"]["message"]["model"] = {"id": "wrong"}
            if scenario == "terminal_wrong_run":
                result["runId"] = "wrong-run"
            if scenario == "missing_result":
                messages.pop(5)
            if scenario == "missing_done":
                messages.pop()
            if scenario == "wrong_done":
                messages[-1]["done"]["runId"] = "wrong-run"
            if scenario == "duplicate_result":
                messages.insert(6, messages[5])
            if scenario == "after_done":
                messages.append({})
            if scenario == "send_lost":
                messages = [{}, {"futureEnvelope": {"opaque": True}}]
            data = b"".join(struct.pack(">BI", 0, len(payload)) + payload for payload in [json.dumps(x).encode() for x in messages]) + struct.pack(">BI", 2, 2) + b"{}"
            if scenario in {"cancel_known", "cancel_unconfirmed", "cancel_stall"}:
                first = json.dumps(messages[2]).encode()
                data = struct.pack(">BI", 0, len(first)) + first + struct.pack(">BI", 0, 1) + b"["
            if scenario == "end_error":
                end = json.dumps({"error": {"code": "internal", "message": KEY + TOKEN}}).encode()
                data = data[:-7] + struct.pack(">BI", 2, len(end)) + end
            if scenario == "missing_end":
                data = data[:-7]
            if scenario == "bad_flags":
                data = b"\x01" + data[1:]
            if scenario == "bad_json":
                data = struct.pack(">BI", 0, 1) + b"["
            self.send_response(200)
            self.send_header("Content-Type", "application/connect+json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            if scenario in {"discard_after_send", "partial_frame_trickle", "partial_frame_no_id"}:
                first = json.dumps(messages[2]).encode()
                if scenario != "partial_frame_no_id":
                    prefix = b"".join(struct.pack(">BI", 0, len(payload)) + payload for payload in [json.dumps(x).encode() for x in messages[:3]])
                    self.wfile.write(prefix)
                    self.wfile.flush()
                    wait_record(lambda row: row["event"] == "receipt" and row.get("method") == "Send")
                if scenario == "discard_after_send":
                    close_admission("discard")
                    rest = messages[3:]
                    data = b"".join(struct.pack(">BI", 0, len(payload)) + payload for payload in [json.dumps(x).encode() for x in rest]) + struct.pack(">BI", 2, 2) + b"{}"
                else:
                    receipt(stall=scenario)
                    for byte in struct.pack(">BI", 0, 2000) + b" " * 2000:
                        self.wfile.write(bytes([byte]))
                        self.wfile.flush()
                        time.sleep(0.02)
                    return
            for offset in range(0, len(data), 3):
                self.wfile.write(data[offset:offset + 3])
                self.wfile.flush()
            return
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
        if method == "ListRepositories":
            if scenario == "close_before_create":
                close_admission("discard")
            if scenario == "discard_storage_failure":
                (Path(CONFIG["journal"]) / "capacity").write_bytes(b"x" * (256 * 1024))
                os.kill(os.getppid(), signal.SIGUSR2)
            if scenario == "capacity_before_create":
                (Path(CONFIG["journal"]) / "capacity").write_bytes(b"x" * (256 * 1024))
            if scenario == "request_conflict":
                (Path(CONFIG["journal"]) / "CreateAgent.request.json").mkdir()
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
