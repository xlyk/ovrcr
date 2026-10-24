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
bridge_index = 1 + sum("token_file" in row for row in [json.loads(line) for line in Path(CONFIG["audit"]).read_text().splitlines()]) if Path(CONFIG["audit"]).exists() else 1
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
        github_pat_excluded="GITHUB-SECRET-SENTINEL-364" not in os.environ.values(),
        argv_excludes_secrets=KEY not in str(sys.argv) and TOKEN not in str(sys.argv) and "GITHUB-SECRET-SENTINEL-364" not in str(sys.argv))

def journal_rows():
    return [json.loads(line) for line in (Path(CONFIG["journal"]) / "events.jsonl").read_text().splitlines()]


def wait_record(predicate):
    end = time.monotonic() + 1
    while time.monotonic() < end:
        if any(predicate(row) for row in journal_rows()):
            return
        time.sleep(0.002)
    if scenario == "discard_after_send":
        receipt(journal_gate_timeout=True)
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
        body = json.loads(raw[5:] if method in {"Send", "ObserveRun"} else raw)
        sends = sum(row.get("method") == "Send" for row in [json.loads(line) for line in Path(CONFIG["audit"]).read_text().splitlines()])
        follow_up = method == "Send" and sends == 1
        third = method == "Send" and sends == 2
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
        known_run = "run-cancel" if sends == 3 else "run-follow-up" if sends == 2 else "run-fixture"
        if method == "GetRun":
            shape = body == {"runId": "run-fixture" if bridge_index == 2 else known_run, "options": {"runtime": "RUNTIME_CLOUD", "agentId": "bc-fixture", "apiKey": KEY}}
        if method == "ObserveRun":
            shape = body in ({"runId": "run-fixture"}, {"runId": "run-fixture", "afterOffset": "observe-3"}, {"runId": "run-cancel"})
            if "afterOffset" in body:
                saved_offsets = [row for row in journal_rows() if row["event"] == "observe_offset"]
                shape = shape and bool(saved_offsets) and saved_offsets[-1] == {"event": "observe_offset", "origin": "ObserveRun", "agentId": "bc-fixture", "runId": "run-fixture", "offset": body["afterOffset"]}
        if follow_up:
            earlier = next(row["token"] for row in journal_rows() if row["event"] == "continuity_token")
            shape = shape and earlier not in body["message"]["text"]
            ahead = ahead and any(row["event"] == "verified" for row in journal_rows())
        cancel_gate = None
        if method == "CancelRun" and sends == 3:
            rows = journal_rows()
            cancelled = [(i,row) for i,row in enumerate(rows) if row["event"] == "cancel_intent"]
            cancel_gate = bool(cancelled) and all(any(row["event"] == event and row.get("runId") == "run-cancel" for row in rows[:cancelled[-1][0]]) for event in ("cancellation_activity",)) and any(row["event"] == "closure" and row["kind"] == "cancel" for row in rows[:cancelled[-1][0]])
        if method == "GetUsage":
            shape = body == {"agentId": "bc-fixture"}
        if third:
            shape = shape and 'sleep 90' in body["message"]["text"]
            ahead = ahead and any(row["event"] == "follow_up_verified" for row in journal_rows())
        receipt(method=method, bridge_index=bridge_index, cancel_gate=cancel_gate, request_shape=shape,
                run_id=body.get("runId") if method in {"GetRun", "CancelRun"} else None, key_matches=body.get("options", {}).get("apiKey") == KEY, write_ahead=ahead,
                exact_cancel=body == {"agentId": "bc-fixture", "runId": known_run} if method == "CancelRun" else None,
                observation_request=body if method == "ObserveRun" else None)
        good_auth = self.headers.get("Authorization") == "Bearer " + TOKEN
        if method == "Shutdown":
            if (scenario == "first_token_leftover" and bridge_index == 1) or (scenario == "second_token_leftover" and bridge_index == 2):
                token_file.unlink()
                token_file.symlink_to(CONFIG["unrelated"])
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
            if scenario == "github_pat_response_key":
                response["GITHUB-SECRET-SENTINEL-364"] = "unexpected property"
            if scenario == "github_pat_nested_key":
                response["extra"] = [{"GITHUB-SECRET-SENTINEL-364": "unexpected nested property"}]
            if scenario == "creation_no_id":
                response.pop("agentId")
            if scenario == "creation_wrong_model":
                response["model"] = {"id": "wrong"}
        elif method == "GetAgent":
            # SDK 1.0.40 allocates locally; the first Send creates the cloud agent.
            if sends == 0 or scenario == "get_agent_unavailable":
                receipt(cloud_metadata_uncreated=sends == 0)
                self.send_response(404)
                self.end_headers()
                return
            response = {"agent": {"agentId": "bc-fixture", "cloud": {"repos": ["https://github.com/fixture/disposable"]}}}
            if scenario == "get_agent_wrong_id":
                response["agent"]["agentId"] = "wrong-agent"
            if scenario == "get_agent_local":
                response["agent"]["local"] = {"cwd": "/wrong"}
            if scenario == "get_agent_wrong_repo":
                response["agent"]["cloud"]["repos"] = ["https://github.com/other/repo"]
        elif method == "CancelRun":
            CONFIG["cancelled"] = True
            if scenario == "cancel_stall" or (sends == 3 and scenario == "third_cancel_stall"):
                receipt(stall="third_cancel" if sends == 3 else scenario)
                time.sleep(30)
            response = {}
        elif method == "GetRun":
            status = "RUN_LIFECYCLE_STATUS_CANCELLED" if CONFIG.get("cancelled") else "RUN_LIFECYCLE_STATUS_RUNNING"
            if scenario == "cancel_unconfirmed" or (sends == 3 and scenario in {"third_ack_only", "third_observer_stall", "third_delayed_cancel", "third_observer_wrong_id", "third_observer_bad_status", "third_observer_error"}):
                status = "RUN_LIFECYCLE_STATUS_RUNNING"
            if sends == 3 and scenario == "third_early_finished_stale":
                status = "RUN_LIFECYCLE_STATUS_RUNNING"
            if sends == 3 and bridge_index == 1 and scenario == "third_early_finished_unavailable":
                self.send_response(404)
                self.end_headers()
                return
            if sends == 3 and scenario == "third_completion_race":
                status = "RUN_LIFECYCLE_STATUS_FINISHED"
            if bridge_index == 2:
                status = "RUN_LIFECYCLE_STATUS_FINISHED"
            CONFIG["snapshots"] = CONFIG.get("snapshots", 0) + 1
            if scenario == "complete_before_drop" or (scenario == "complete_during_attach" and CONFIG["snapshots"] > 1):
                status = "RUN_LIFECYCLE_STATUS_FINISHED"
            if scenario.startswith("snapshot_") and (scenario.endswith("before_drop") or CONFIG["snapshots"] > 1):
                status = "RUN_LIFECYCLE_STATUS_" + scenario.split("_")[1].upper()
            response = {"run": {"agentId": "bc-fixture", "runId": body["runId"], "status": status}}
            if sends == 3 and scenario == "third_snapshot_wrong_id":
                response["run"]["runId"] = "wrong-run"
            if bridge_index == 2 and scenario == "fresh_wrong_id":
                response["run"]["runId"] = "wrong-run"
            if bridge_index == 2 and scenario in {"fresh_unavailable", "fresh_requires_resume"}:
                self.send_response(404)
                self.end_headers()
                return
            if bridge_index == 2 and scenario == "fresh_stall":
                receipt(stall="fresh_get_run")
                time.sleep(30)
            if scenario == "snapshot_wrong_id" and not CONFIG.get("cancelled"):
                response["run"]["agentId"] = "wrong-agent"
        elif method in {"Send", "ObserveRun"}:
            identity = {"agent_id": "bc-fixture", "run_id": "run-fixture"}
            def sdk(kind, **fields):
                return {"sdkMessage": {"type": kind, "message": {"type": kind, **identity, **fields}}}
            probe = {"call_id": "source-probe", "name": "shell", "args": {"command": "git rev-parse HEAD && cat SOURCE_MARKER"}}
            result = {"agentId": "bc-fixture", "runId": "run-fixture", "model": {"id": "fixture-model"},
                      "status": "RUN_LIFECYCLE_STATUS_FINISHED", "git": {"branches": [{"repoUrl": "https://github.com/fixture/disposable", "branch": "cursor/result"}]}}
            messages = [{}, {"sdkMessage": {"type": "future", "message": {"opaque": True}}},
                        sdk("thinking", text=""),
                        sdk("tool_call", **probe, status="running"),
                        sdk("tool_call", **probe, status="completed", result={"stdout": CONFIG["source_sha"] + "\nOVRCR_SELECTED_BASELINE_364\n", "stderr": "", "exitCode": 0}),
                        {"result": {"agentId": "bc-fixture", "runId": "run-fixture", "status": "RUN_LIFECYCLE_STATUS_FINISHED", "result": result}},
                        {"done": {"agentId": "bc-fixture", "runId": "run-fixture"}}]
            # GetRun reconstructs a cloud handle without the original requested model.
            if method == "ObserveRun":
                result.pop("model")
            if scenario == "cloud_status_first":
                messages[2] = sdk("status", status="RUNNING")
            if scenario in {"source_prefix", "source_prefix_wrong_sha", "source_prefix_wrong_marker"}:
                probe["args"]["command"] = "cd /workspace && " + probe["args"]["command"]
            if scenario == "source_prefix_wrong_sha":
                messages[4]["sdkMessage"]["message"]["result"]["stdout"] = "b" * 40 + "\nOVRCR_SELECTED_BASELINE_364\n"
            if scenario == "source_prefix_wrong_marker":
                messages[4]["sdkMessage"]["message"]["result"]["stdout"] = CONFIG["source_sha"] + "\nOVRCR_DEFAULT_TIP_364\n"
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
            if follow_up:
                Path(CONFIG["journal"]).parent.joinpath("pr-effects.json").write_text("second turn admitted")
                identity["run_id"] = "run-follow-up"
                result["runId"] = "run-follow-up"
                earlier = next(row["token"] for row in journal_rows() if row["event"] == "continuity_token")
                result["result"] = "TOKEN=" + earlier + "\nFIX=addition.py adds its two inputs."
                messages = [sdk("thinking", text=""),
                            {"result": {"agentId": "bc-fixture", "runId": "run-follow-up", "status": "RUN_LIFECYCLE_STATUS_FINISHED", "result": result}},
                            {"done": {"agentId": "bc-fixture", "runId": "run-follow-up"}}]
                if scenario == "follow_up_lost":
                    messages = [{}, {"futureEnvelope": {"opaque": True}}]
                if scenario == "follow_up_wrong_token":
                    result["result"] = "TOKEN=wrong\nFIX=addition.py adds its two inputs."
                if scenario == "follow_up_failed":
                    result["status"] = messages[1]["result"]["status"] = "RUN_LIFECYCLE_STATUS_ERROR"
                if scenario == "follow_up_same_run":
                    messages[0]["sdkMessage"]["message"]["run_id"] = "run-fixture"
                if scenario == "follow_up_wrong_agent":
                    messages[0]["sdkMessage"]["message"]["agent_id"] = "wrong-agent"
                if scenario == "follow_up_branch":
                    result["git"]["branches"][0]["branch"] = "cursor/extra"
                if scenario == "follow_up_pr":
                    result["git"]["branches"][0]["prUrl"] = "https://github.com/fixture/disposable/pull/1"
                if scenario in {"follow_up_extra_file", "follow_up_extra_ref", "follow_up_extra_tag"}:
                    import subprocess
                    def git(*args):
                        subprocess.run(["/usr/bin/git", "-c", "user.name=Offline Fixture", "-c", "user.email=fixture@example.invalid", *args], cwd=CONFIG["repo"], check=True, capture_output=True)
                    if scenario == "follow_up_extra_tag":
                        git("tag", "cursor-extra", "cursor/result")
                    elif scenario == "follow_up_extra_ref":
                        git("branch", "cursor/extra", "cursor/result")
                    else:
                        git("switch", "cursor/result")
                        Path(CONFIG["repo"], "extra.txt").write_text("unintended change")
                        git("add", "extra.txt")
                        git("commit", "-m", "unintended follow-up file")
                        git("switch", "main")
            if third or (method == "ObserveRun" and body.get("runId") == "run-cancel"):
                identity["run_id"] = "run-cancel"
                result["runId"] = "run-cancel"
                result["status"] = "RUN_LIFECYCLE_STATUS_CANCELLED"
                messages = [sdk("thinking", text=""),
                            sdk("tool_call", call_id="wait", name="shell", args={"command": "sleep 90"}, status="running")]
                if third and scenario in {"third_early_finished_stale", "third_early_finished_unavailable"}:
                    result["status"] = "RUN_LIFECYCLE_STATUS_FINISHED"
                    messages = messages[:1] + [{"result": {"agentId": "bc-fixture", "runId": "run-cancel", "status": result["status"], "result": result}},
                                               {"done": {"agentId": "bc-fixture", "runId": "run-cancel"}}]
                if third and scenario == "third_no_id":
                    messages = [{}]
                if third and scenario == "third_wrong_agent":
                    messages[0]["sdkMessage"]["message"]["agent_id"] = "wrong-agent"
                if third and scenario == "third_reused_id":
                    messages[0]["sdkMessage"]["message"]["run_id"] = "run-follow-up"
                if third and scenario == "third_no_activity":
                    messages = messages[:1]
                if method == "ObserveRun":
                    messages = [{"result": {"agentId": "bc-fixture", "runId": "run-cancel", "status": result["status"], "result": result}},
                                {"done": {"agentId": "bc-fixture", "runId": "run-cancel"}}]
                    if scenario == "third_observer_wrong_id":
                        messages[0]["result"]["runId"] = "wrong-run"
                    if scenario == "third_observer_bad_status":
                        messages[0]["result"]["status"] = "RUN_LIFECYCLE_STATUS_FINISHED"
                    if scenario == "third_ack_only":
                        messages = [{}]
                    if scenario == "third_observer_stall":
                        receipt(stall="third_observer")
                        time.sleep(30)
                if third and scenario == "third_late_discard":
                    close_admission("discard")
            if follow_up and scenario == "follow_up_partial":
                messages = messages[:1] + [{"sdkMessage": []}]
            for i, message in enumerate(messages):
                if message:
                    message["offset"] = ("observe-" if method == "ObserveRun" else "send-") + str(i)
            if method == "ObserveRun":
                if scenario == "invalid_offset":
                    messages[3]["offset"] = False
                if "afterOffset" in body:
                    messages = messages[4:]
                    if scenario == "wrong_offset":
                        messages[0]["offset"] = body["afterOffset"]
                if scenario == "observer_wrong_agent":
                    messages[0 if "afterOffset" in body else 3]["sdkMessage"]["message"]["agent_id"] = "wrong-agent"
                if scenario == "observer_wrong_run":
                    messages[0 if "afterOffset" in body else 3]["sdkMessage"]["message"]["run_id"] = "wrong-run"
                if scenario == "no_observe_offset":
                    for message in messages:
                        message.pop("offset", None)
            data = b"".join(struct.pack(">BI", 0, len(payload)) + payload for payload in [json.dumps(x).encode() for x in messages]) + struct.pack(">BI", 2, 2) + b"{}"
            if scenario in {"cancel_known", "cancel_unconfirmed", "cancel_stall"}:
                first = json.dumps(messages[2]).encode()
                data = struct.pack(">BI", 0, len(first)) + first + struct.pack(">BI", 0, 1) + b"["
            if scenario == "end_error" or (method == "ObserveRun" and (scenario == "observer_error" or (body.get("runId") == "run-cancel" and scenario == "third_observer_error"))):
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
            if method == "Send" and scenario == "discard_after_send":
                # Close after dispatch, before init can move this late-receipt case into observation.
                before = ahead and not any(row["event"] == "closure" for row in journal_rows())
                close_admission("discard")
                receipt(discard_gate={"send_received_before_closure": before,
                                      "closure_before_init": any(row["event"] == "closure" and row["kind"] == "discard" for row in journal_rows())})
            if method == "ObserveRun" and body.get("runId") != "run-cancel" and "afterOffset" not in body and not scenario.startswith("snapshot_") and scenario not in {"complete_before_drop", "complete_during_attach", "no_observe_offset", "partial_frame_trickle", "cancel_known", "cancel_unconfirmed", "cancel_stall", "bad_flags", "bad_json"}:
                prefix = b"".join(struct.pack(">BI", 0, len(payload)) + payload for payload in [json.dumps(x).encode() for x in messages[:4]])
                self.wfile.write(prefix)
                self.wfile.flush()
                if scenario in {"observer_wrong_agent", "observer_wrong_run", "sdk_bad_payload", "late_source_probe"}:
                    return
                wait_record(lambda row: row["event"] == "observe_request" and "afterOffset" in row["request"])
                self.connection.settimeout(1)
                try:
                    dropped = self.connection.recv(1) == b""
                except OSError:
                    dropped = False
                receipt(original_observer_closed=dropped)
                return
            if method == "Send" and not follow_up and not third and scenario not in {"send_lost", "bad_flags", "bad_json", "early_no_id", "early_wrong_agent", "early_wrong_model", "cancel_known", "cancel_unconfirmed", "cancel_stall", "partial_frame_no_id", "discard_after_send"}:
                prefix = b"".join(struct.pack(">BI", 0, len(payload)) + payload for payload in [json.dumps(x).encode() for x in messages[:3]])
                self.wfile.write(prefix)
                self.wfile.flush()
                wait_record(lambda row: row["event"] == "send_stream_dropped")
                self.connection.settimeout(1)
                try:
                    dropped = self.connection.recv(1) == b""
                except OSError:
                    dropped = False
                receipt(original_send_closed=dropped, completed_before_drop=scenario == "complete_before_drop")
                return
            if (method == "ObserveRun" and scenario == "partial_frame_trickle") or (method == "Send" and scenario == "partial_frame_no_id"):
                if scenario != "partial_frame_no_id":
                    prefix = b"".join(struct.pack(">BI", 0, len(payload)) + payload for payload in [json.dumps(x).encode() for x in messages[:3]])
                    self.wfile.write(prefix)
                    self.wfile.flush()
                    wait_record(lambda row: row["event"] == "receipt" and row.get("method") == "Send")
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
        elif method == "GetUsage":
            response = {"usage": {"usage": {"totalTokens": "24"}, "runs": []}}
            if scenario == "usage_cost":
                response["usage"]["cost"] = {"rawCostCents": 2.0, "chargedCents": 1.5}
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
