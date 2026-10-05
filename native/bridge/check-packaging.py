#!/usr/bin/env python3
"""Validate disposable bundle copies; never sign, install or launch them."""
import os
import plistlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

if not __debug__:
    raise SystemExit("Packaging checks require assertions; invoke python3 -I without -O")
source = Path(sys.argv[1])
validator = Path(__file__).with_name("validate-bundle.py")
environment = dict(os.environ, PYTHONOPTIMIZE="1")
checks = 0


def validate(bundle, expected=True, *identity):
    global checks
    result = subprocess.run([sys.executable, "-I", str(validator), str(bundle), *identity],
                            env=environment, capture_output=True, timeout=5)
    assert (result.returncode == 0) == expected, (bundle, result.returncode, result.stderr)
    checks += 1


validate(source)
with tempfile.TemporaryDirectory(prefix="ovrcr-bridge-packaging-") as temporary:
    root = Path(temporary)
    for case in ["schema", "wire", "schema_bool", "wire_float", "display", "executable", "license", "hash", "missing", "symlink", "extra", "callback_hash", "callback_missing", "callback_symlink", "callback_extra", "purpose", "entitlements", "entitlements_missing"]:
        bundle = root / f"{case}.app"
        shutil.copytree(source, bundle)
        info_path = bundle / "Contents/Info.plist"
        info = plistlib.loads(info_path.read_bytes())
        resources = bundle / "Contents/Resources"
        if case in ("schema", "wire", "display", "executable"):
            field = {"schema": "OVRCRBridgeSchema", "wire": "OVRCRServerWire",
                     "display": "CFBundleDisplayName", "executable": "CFBundleExecutable"}[case]
            info[field] = info[field] + 1 if isinstance(info[field], int) else "unexpected"
            info_path.write_bytes(plistlib.dumps(info))
        elif case in ("schema_bool", "wire_float"):
            info["OVRCRBridgeSchema" if case == "schema_bool" else "OVRCRServerWire"] = True if case == "schema_bool" else float(info["OVRCRServerWire"])
            info_path.write_bytes(plistlib.dumps(info))
        elif case == "license":
            (resources / "NotificationSounds-LICENSE").write_bytes(b"changed")
        elif case == "hash":
            (resources / "ovrcr-tap-v1.wav").write_bytes(b"changed")
        elif case == "missing":
            (resources / "ovrcr-rise-v1.wav").unlink()
        elif case == "symlink":
            tone = resources / "ovrcr-tap-v1.wav"
            tone.unlink()
            tone.symlink_to(source / "Contents/Resources/ovrcr-tap-v1.wav")
        elif case == "callback_hash":
            (bundle / "Contents/MacOS/ovrcr").write_bytes(b"changed")
        elif case == "callback_missing":
            (bundle / "Contents/MacOS/ovrcr").unlink()
        elif case == "callback_symlink":
            helper = bundle / "Contents/MacOS/ovrcr"
            helper.unlink()
            helper.symlink_to(source / "Contents/MacOS/ovrcr")
        elif case == "purpose":
            info["NSAppleEventsUsageDescription"] = ""
            info_path.write_bytes(plistlib.dumps(info))
        elif case == "entitlements":
            (bundle / "Contents/OVRCRBridge.entitlements").write_bytes(plistlib.dumps({"com.apple.security.automation.apple-events": False}))
        elif case == "entitlements_missing":
            (bundle / "Contents/OVRCRBridge.entitlements").unlink()
        elif case == "callback_extra":
            (bundle / "Contents/MacOS/unreviewed-helper").write_bytes(b"extra")
        else:
            (resources / "unexpected-resource").write_bytes(b"extra")
        validate(bundle, False)
    alias = root / "alias.app"
    alias.symlink_to(source)
    validate(alias, False)
    fixture = root / "fixture.app"
    shutil.copytree(source, fixture)
    info_path = fixture / "Contents/Info.plist"
    info = plistlib.loads(info_path.read_bytes())
    info.update(CFBundleIdentifier="com.ovrcr.bridge.packaging-fixture", CFBundleDisplayName="OVRCR Packaging Fixture")
    info_path.write_bytes(plistlib.dumps(info))
    validate(fixture, False)
    validate(fixture, True, "com.ovrcr.bridge.packaging-fixture", "OVRCR Packaging Fixture")
print(f"{checks} packaging checks passed under PYTHONOPTIMIZE=1; no signing/install/launch.")
