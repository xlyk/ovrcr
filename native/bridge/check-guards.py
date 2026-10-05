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
bundle = Path(sys.argv[1])
info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
schema, wire = info["OVRCRBridgeSchema"], info["OVRCRServerWire"]
binary = bundle / "Contents/MacOS/OVRCRBridge"
checks = 0


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
    check(["--client"], data, "failed")
for wrong in [{"schema": schema + 1, "server_wire": wire},
              {"schema": schema, "server_wire": wire + 1}]:
    check(["--client"], json.dumps({**wrong, "op": {"type": "invalid"}}).encode(),
          "incompatible")
for op in [{"type": "invalid"}, {"type": "deliver"},
           {"type": "deliver", "title": "private prompt", "subtitle": "x", "body": "x"},
           {"type": "deliver", "title": "OVRCR · response ready", "subtitle": "x\n", "body": "x"},
           {"type": "deliver", "title": "OVRCR · input needed", "subtitle": "x", "body": "x" * 1025}]:
    check(["--client"], json.dumps({"schema": schema, "server_wire": wire, "op": op}).encode(),
          "failed")

# Keep stdin open without a full request: the executable must time out before IPC.
start = time.monotonic()
child = subprocess.Popen([str(binary), "--client"], stdin=subprocess.PIPE,
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
print(f"{checks} compiled-entry guard checks passed; no admitted request, IPC or native app lifecycle.")
