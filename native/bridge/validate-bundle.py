#!/usr/bin/env python3
"""Read-only metadata/resource validation; never launch, sign or install."""
import hashlib
import json
import plistlib
import re
import sys
from pathlib import Path

if not __debug__:
    raise SystemExit("Validation requires assertions; invoke python3 -I without -O")
arguments = sys.argv[1:]
local_development = bool(arguments and arguments[0] == "--local-development")
if local_development:
    arguments = arguments[1:]
if not 1 <= len(arguments) <= (1 if local_development else 3):
    raise SystemExit("Usage: validate-bundle.py [--local-development] APP_BUNDLE [BUNDLE_ID [DISPLAY_NAME]]")
repo = Path(__file__).resolve().parents[2]
bundle = Path(arguments[0])
assert bundle.is_dir() and not bundle.is_symlink()
if local_development:
    expected_id, expected_display = "com.ovrcr.bridge.local", "OVRCR Local"
else:
    expected_id = arguments[1] if len(arguments) >= 2 else "com.ovrcr.bridge"
    expected_display = arguments[2] if len(arguments) == 3 else "OVRCR"
contract_path = Path(__file__).with_name("expected-contract.json")
if contract_path.is_file():
    contract = json.loads(contract_path.read_text())
    assert type(contract["local_development"]) is bool and contract["local_development"] == local_development
    assert contract["bundle_id"] == expected_id and contract["display_name"] == expected_display
    wire, schema = contract["wire"], contract["schema"]
    manifest_hash, license_hash = contract["manifest_sha256"], contract["license_sha256"]
    manifest = contract["manifest"]
    entitlements_hash = contract["entitlements_sha256"]
    callback_hash = contract["callback_sha256"]
    assert re.fullmatch(r"[0-9a-f]{64}", callback_hash)
else:
    wire = int(re.search(r"pub const PROTOCOL_VERSION: u32 = (\d+);",
                         (repo / "crates/ovrcr-protocol/src/codec.rs").read_text())[1])
    schema = int(re.search(r"pub const BRIDGE_SCHEMA_VERSION: u32 = (\d+);",
                           (repo / "crates/ovrcr-protocol/src/bridge.rs").read_text())[1])
    source = repo / "research/notification-bridge/sounds"
    manifest_bytes = (source / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    manifest_hash = hashlib.sha256(manifest_bytes).hexdigest()
    license_hash = hashlib.sha256((source / "LICENSE").read_bytes()).hexdigest()
    entitlements_hash = hashlib.sha256((repo / "native/bridge/entitlements.plist").read_bytes()).hexdigest()
info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
if local_development:
    assert info.get("OVRCRBridgeLocalDevelopment") is True
else:
    assert "OVRCRBridgeLocalDevelopment" not in info
    assert expected_id != "com.ovrcr.bridge.local" and expected_display != "OVRCR Local"
assert info["CFBundleIdentifier"] == expected_id
assert info["CFBundleExecutable"] == "OVRCRBridge"
assert info["CFBundleDisplayName"] == expected_display and info["LSUIElement"] is True
assert info["CFBundlePackageType"] == "APPL"
assert info["NSAppleEventsUsageDescription"] == "OVRCR can select the existing iTerm session hosting your current Dashboard after you explicitly set up iTerm focus."
legacy_entitlements = bundle / "Contents/OVRCRBridge.entitlements"
assert not legacy_entitlements.exists() and not legacy_entitlements.is_symlink()
entitlements = bundle / "Contents/Resources/OVRCRBridge.entitlements"
assert entitlements.is_file() and not entitlements.is_symlink()
assert hashlib.sha256(entitlements.read_bytes()).hexdigest() == entitlements_hash
assert plistlib.loads(entitlements.read_bytes()) == {"com.apple.security.automation.apple-events": True}
assert type(info["OVRCRBridgeSchema"]) is int and info["OVRCRBridgeSchema"] == schema
assert type(info["OVRCRServerWire"]) is int and info["OVRCRServerWire"] == wire
assert info["CFBundleVersion"] == f"{schema}.{wire}"
if contract_path.is_file():
    assert info["OVRCRBridgeBuild"] == contract["build"]
    assert info["OVRCRCallbackSHA256"] == callback_hash
assert (bundle / "Contents/MacOS/OVRCRBridge").is_file()
assert (bundle / "Contents/MacOS/OVRCRBridge").stat().st_mode & 0o111
helper = bundle / "Contents/MacOS/ovrcr"
assert helper.is_file() and not helper.is_symlink() and helper.stat().st_mode & 0o111
assert 0 < helper.stat().st_size <= 512 * 1024 * 1024
assert re.fullmatch(r"[0-9a-f]{64}", info["OVRCRCallbackSHA256"])
assert hashlib.sha256(helper.read_bytes()).hexdigest() == info["OVRCRCallbackSHA256"]
assert {path.name for path in (bundle / "Contents/MacOS").iterdir()} == {"OVRCRBridge", "ovrcr"}
assert not any(path.is_symlink() for path in bundle.rglob("*"))
resources = bundle / "Contents/Resources"
expected_files = {tone["file"] for tone in manifest["tones"]}
expected_files.update(("NotificationSounds-LICENSE", "NotificationSounds-manifest.json", "OVRCRBridge.entitlements"))
assert {path.name for path in resources.iterdir()} == expected_files
assert hashlib.sha256((resources / "NotificationSounds-LICENSE").read_bytes()).hexdigest() == license_hash
assert hashlib.sha256((resources / "NotificationSounds-manifest.json").read_bytes()).hexdigest() == manifest_hash
for tone in manifest["tones"]:
    path = resources / tone["file"]
    assert path.is_file() and not path.is_symlink()
    assert hashlib.sha256(path.read_bytes()).hexdigest() == tone["sha256"]
print(f"Bundle metadata/resources valid: schema {schema}, wire {wire}, pinned callback CLI and three original regular WAV assets.")
