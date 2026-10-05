#!/usr/bin/env python3
"""Exercise a relocated runtime payload; never install or launch the Bridge."""
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
    (bundle / "Contents/Resources/ovrcr-tap-v1.wav").write_bytes(b"changed")
    result = subprocess.run([sys.executable, "-I", str(validator), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted a changed sound"
    result = subprocess.run(["sh", str(moved / "scripts/install-bridge.sh"), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode == 64 and b"non-ad-hoc" in result.stderr
print("3 relocated startup payload checks passed; no install or app launch")
