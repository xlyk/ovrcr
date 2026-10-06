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
arguments = sys.argv[1:]
local_development = bool(arguments and arguments[0] == "--local-development")
if local_development:
    arguments = arguments[1:]
if len(arguments) != 1:
    raise SystemExit("Usage: check-packaging.py [--local-development] APP_BUNDLE")
source = Path(arguments[0])
validator = Path(__file__).with_name("validate-bundle.py")
environment = dict(os.environ, PYTHONOPTIMIZE="1")
checks = 0


def validate(bundle, expected=True, *identity, local=local_development):
    global checks
    profile = ["--local-development"] if local else []
    result = subprocess.run([sys.executable, "-I", str(validator), *profile, str(bundle), *identity],
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
    # The legacy explicit fixture identity is a production-profile API.
    # A local artifact cannot use it to bypass the fixed local identity/marker.
    info.pop("OVRCRBridgeLocalDevelopment", None)
    info_path.write_bytes(plistlib.dumps(info))
    validate(fixture, True, "com.ovrcr.bridge.packaging-fixture", "OVRCR Packaging Fixture", local=False)
    if local_development:
        validate(source, False, local=False)
        validate(source, False, "com.ovrcr.bridge.local", "OVRCR Local", local=False)
        validate(source, False, "com.ovrcr.bridge.local", "OVRCR Local")
        for case, value in (("absent", None), ("false", False), ("integer", 1), ("string", "true")):
            bundle = root / f"local-marker-{case}.app"
            shutil.copytree(source, bundle)
            info_path = bundle / "Contents/Info.plist"
            info = plistlib.loads(info_path.read_bytes())
            if value is None:
                info.pop("OVRCRBridgeLocalDevelopment")
            else:
                info["OVRCRBridgeLocalDevelopment"] = value
            info_path.write_bytes(plistlib.dumps(info))
            validate(bundle, False)
        production = root / "production-profile.app"
        shutil.copytree(source, production)
        info_path = production / "Contents/Info.plist"
        info = plistlib.loads(info_path.read_bytes())
        info.pop("OVRCRBridgeLocalDevelopment")
        info.update(CFBundleIdentifier="com.ovrcr.bridge", CFBundleDisplayName="OVRCR")
        info_path.write_bytes(plistlib.dumps(info))
        validate(production, False)
        validate(production, True, local=False)
print(f"{checks} packaging checks passed under PYTHONOPTIMIZE=1; no signing/install/launch.")
