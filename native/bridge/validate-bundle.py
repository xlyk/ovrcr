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
if not 2 <= len(sys.argv) <= 4:
    raise SystemExit("Usage: validate-bundle.py APP_BUNDLE [BUNDLE_ID [DISPLAY_NAME]]")
repo = Path(__file__).resolve().parents[2]
bundle = Path(sys.argv[1])
assert bundle.is_dir() and not bundle.is_symlink()
expected_id = sys.argv[2] if len(sys.argv) >= 3 else "com.ovrcr.bridge"
expected_display = sys.argv[3] if len(sys.argv) == 4 else "OVRCR"
wire = int(re.search(r"pub const PROTOCOL_VERSION: u32 = (\d+);",
                     (repo / "crates/ovrcr-protocol/src/codec.rs").read_text())[1])
schema = int(re.search(r"pub const BRIDGE_SCHEMA_VERSION: u32 = (\d+);",
                       (repo / "crates/ovrcr-protocol/src/bridge.rs").read_text())[1])
info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
assert info["CFBundleIdentifier"] == expected_id
assert info["CFBundleExecutable"] == "OVRCRBridge"
assert info["CFBundleDisplayName"] == expected_display and info["LSUIElement"] is True
assert info["CFBundlePackageType"] == "APPL"
assert info["OVRCRBridgeSchema"] == schema and info["OVRCRServerWire"] == wire
assert info["CFBundleVersion"] == f"{schema}.{wire}"
assert (bundle / "Contents/MacOS/OVRCRBridge").is_file()
assert (bundle / "Contents/MacOS/OVRCRBridge").stat().st_mode & 0o111
assert not any(path.is_symlink() for path in bundle.rglob("*"))
resources = bundle / "Contents/Resources"
source = repo / "research/notification-bridge/sounds"
manifest = json.loads((source / "manifest.json").read_text())
expected_files = {tone["file"] for tone in manifest["tones"]}
expected_files.update(("NotificationSounds-LICENSE", "NotificationSounds-manifest.json"))
assert {path.name for path in resources.iterdir()} == expected_files
assert (resources / "NotificationSounds-LICENSE").read_bytes() == (source / "LICENSE").read_bytes()
assert (resources / "NotificationSounds-manifest.json").read_bytes() == (source / "manifest.json").read_bytes()
for tone in manifest["tones"]:
    path = resources / tone["file"]
    assert path.is_file() and not path.is_symlink()
    assert hashlib.sha256(path.read_bytes()).hexdigest() == tone["sha256"]
print(f"Bundle metadata/resources valid: schema {schema}, wire {wire}, three original regular WAV assets.")
