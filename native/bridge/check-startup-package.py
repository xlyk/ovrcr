#!/usr/bin/env python3
"""Exercise a relocated runtime payload; never install or launch the Bridge."""
import hashlib
import plistlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

assert __debug__, "Run python3 -I without -O"
payload = Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix="ovrcr-startup-package-") as temporary:
    moved = Path(temporary) / "outside-checkout"
    shutil.copytree(payload, moved)
    bundle = moved / "OVRCR Bridge.app"
    validator = moved / "native/bridge/validate-bundle.py"
    result = subprocess.run([sys.executable, "-I", str(validator), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode == 0, result.stderr.decode()
    info_path = bundle / "Contents/Info.plist"
    info_bytes = info_path.read_bytes()
    helper = bundle / "Contents/MacOS/ovrcr"
    helper.write_bytes(b"changed callback helper")
    info = plistlib.loads(info_bytes)
    info["OVRCRCallbackSHA256"] = hashlib.sha256(helper.read_bytes()).hexdigest()
    info_path.write_bytes(plistlib.dumps(info))
    result = subprocess.run([sys.executable, "-I", str(validator), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted a changed helper and matching metadata"
    shutil.copy2(payload / "OVRCR Bridge.app/Contents/MacOS/ovrcr", helper)
    info_path.write_bytes(info_bytes)
    entitlements = bundle / "Contents/OVRCRBridge.entitlements"
    entitlements_bytes = entitlements.read_bytes()
    entitlements.unlink()
    result = subprocess.run([sys.executable, "-I", str(validator), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted missing entitlements"
    entitlements.write_bytes(entitlements_bytes)
    info = plistlib.loads(info_bytes)
    info["NSAppleEventsUsageDescription"] = "changed purpose"
    info_path.write_bytes(plistlib.dumps(info))
    result = subprocess.run([sys.executable, "-I", str(validator), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted a changed purpose"
    info_path.write_bytes(info_bytes)
    (bundle / "Contents/Resources/ovrcr-tap-v1.wav").write_bytes(b"changed")
    result = subprocess.run([sys.executable, "-I", str(validator), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted a changed sound"
    result = subprocess.run(["sh", str(moved / "scripts/install-bridge.sh"), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode == 64 and b"non-ad-hoc" in result.stderr
print("6 relocated startup payload checks passed; no install or app launch")
