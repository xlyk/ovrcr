#!/usr/bin/env python3
"""Exercise the compiled app's early guards only; never send an admitted request."""
import json
import plistlib
import subprocess
import sys
import time
from pathlib import Path

if not __debug__:
    raise SystemExit("Guard checks require assertions; invoke python3 -I without -O")
arguments = sys.argv[1:]
local_development = arguments[:1] == ["--local-development"]
if local_development:
    arguments = arguments[1:]
assert len(arguments) == 1, "Usage: check-guards.py [--local-development] APP_BUNDLE"
bundle = Path(arguments[0])
info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
schema, wire = info["OVRCRBridgeSchema"], info["OVRCRServerWire"]
binary = bundle / "Contents/MacOS/OVRCRBridge"
checks = 0
client_arguments = ["--client", "--local-development"] if local_development else ["--client"]


def check(arguments, data, status, code=0):
    global checks
    result = subprocess.run([str(binary), *arguments], input=data,
                            capture_output=True, timeout=2.5)
    assert result.returncode == code, (arguments, result.returncode)
    assert result.stderr == b"", result.stderr
    assert result.stdout.endswith(b"\n") and result.stdout.count(b"\n") == 1
    assert json.loads(result.stdout) == {"schema": schema, "server_wire": wire,
                                         "status": status}
    checks += 1


check(["--check-contract", str(schema), str(wire)], b"", "available")
check(["--check-contract", str(schema + 1), str(wire)], b"", "incompatible", 78)
check(["--check-contract", str(schema), str(wire + 1)], b"", "incompatible", 78)
for arguments in [["--check-contract"], ["--check-contract", "-1", str(wire)],
                  ["--check-contract", str(schema), "4294967296"], ["--unexpected"]]:
    check(arguments, b"", "failed", 64)
check(["--client", "unexpected"], b"", "failed")
for data in [b"", b"{}", b"not JSON", b"[]", b"null", b"x" * (1_048_576 + 1)]:
    check(client_arguments, data, "failed")
for wrong in [{"schema": schema + 1, "server_wire": wire},
              {"schema": schema, "server_wire": wire + 1},
              {"schema": 1, "server_wire": 33},
              {"schema": 2, "server_wire": 34}]:
    check(client_arguments, json.dumps({**wrong, "op": {"type": "invalid"}}).encode(),
          "incompatible")
for op in [{"type": "invalid"}, {"type": "deliver"},
           {"type": "deliver", "title": "private prompt", "subtitle": "x", "body": "x"},
           {"type": "deliver", "title": "OVRCR · response ready", "subtitle": "x\n", "body": "x"},
           {"type": "deliver", "title": "OVRCR · input needed", "subtitle": "x", "body": "x" * 1025}]:
    check(client_arguments, json.dumps({"schema": schema, "server_wire": wire, "op": op}).encode(),
          "failed")
for invalid in ["Glass", "/tmp/tap.wav", "ovrcr-tap-v1.wav", True, 7, ["tap"]]:
    op = {"type": "deliver", "title": "OVRCR · response ready", "subtitle": "label",
          "body": "p / w / n (#1)", "sound": invalid}
    check(client_arguments, json.dumps({"schema": schema, "server_wire": wire, "op": op}).encode(),
          "failed")

# Keep stdin open without a full request: the executable must time out before IPC.
start = time.monotonic()
child = subprocess.Popen([str(binary), *client_arguments], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
try:
    child.wait(timeout=2.5)
    assert 1.5 <= time.monotonic() - start < 2.5
    assert child.returncode == 0 and child.stderr.read() == b""
    assert json.loads(child.stdout.read()) == {"schema": schema, "server_wire": wire,
                                               "status": "failed"}
    checks += 1
finally:
    if child.poll() is None:
        child.kill()
        child.wait()
    child.stdin.close()
    child.stdout.close()
    child.stderr.close()

opposite_arguments = ["--client"] if local_development else ["--client", "--local-development"]
check(opposite_arguments, b"", "incompatible")
check(["--client", "--local-development", "unexpected"], b"", "failed")
# Wrong-profile clients must exit while stdin remains open, before its deadline.
start = time.monotonic()
child = subprocess.Popen([str(binary), *opposite_arguments], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
try:
    child.wait(timeout=1)
    assert time.monotonic() - start < 1
    assert child.returncode == 0 and child.stderr.read() == b""
    assert json.loads(child.stdout.read()) == {"schema": schema, "server_wire": wire,
                                               "status": "incompatible"}
    checks += 1
finally:
    if child.poll() is None:
        child.kill()
        child.wait()
    child.stdin.close()
    child.stdout.close()
    child.stderr.close()

# Mutate only this uninstalled fixture's metadata; inputs remain inadmissible.
# Restore even on failure. No test asks the OS to authorize or display anything.
info_path = bundle / "Contents/Info.plist"
original_info = info_path.read_bytes()
try:
    if local_development:
        for key, value in [("CFBundleIdentifier", "com.ovrcr.bridge"),
                           ("CFBundleDisplayName", "OVRCR"),
                           ("OVRCRBridgeLocalDevelopment", False),
                           ("OVRCRBridgeLocalDevelopment", 1),
                           ("OVRCRBridgeLocalDevelopment", "true"),
                           ("OVRCRBridgeLocalDevelopment", None)]:
            changed = dict(info)
            if value is None:
                changed.pop(key)
            else:
                changed[key] = value
            info_path.write_bytes(plistlib.dumps(changed))
            check(client_arguments, b"", "incompatible")
    else:
        for value in [False, True, 1, "true"]:
            changed = {**info, "OVRCRBridgeLocalDevelopment": value}
            info_path.write_bytes(plistlib.dumps(changed))
            check(client_arguments, b"", "incompatible")
        changed = {**info, "CFBundleIdentifier": "com.ovrcr.bridge.local"}
        info_path.write_bytes(plistlib.dumps(changed))
        check(client_arguments, b"", "incompatible")
        changed = {**info, "CFBundleIdentifier": "com.ovrcr.private-unmarked-fixture"}
        info_path.write_bytes(plistlib.dumps(changed))
        check(client_arguments, b"", "failed")
finally:
    info_path.write_bytes(original_info)
print(f"{checks} compiled-entry guard checks passed; no admitted request, IPC or native app lifecycle.")
