#!/bin/sh
# Build the optional macOS runtime payload. No signing or user installation.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
[ "$#" -eq 1 ] || { echo 'Usage: scripts/package-startup.sh OUTPUT_DIRECTORY' >&2; exit 64; }
destination=$1
[ ! -L "$destination" ] || { echo 'Startup payload destination must not be a symlink' >&2; exit 65; }
fingerprint=$(python3 -I - "$repo_dir" <<'PY_HASH'
import hashlib, platform, sys
from pathlib import Path
root = Path(sys.argv[1])
files = sorted((root / "native/bridge").glob("**/*"))
files += [root / "scripts" / name for name in ("build-bridge.sh", "install-bridge.sh", "package-startup.sh")]
files += [root / "crates/ovrcr-protocol/src" / name for name in ("bridge.rs", "codec.rs")]
files += sorted((root / "research/notification-bridge/sounds").glob("*"))
digest = hashlib.sha256(platform.machine().encode())
for path in files:
    if path.is_file():
        digest.update(str(path.relative_to(root)).encode())
        digest.update(path.read_bytes())
print(digest.hexdigest())
PY_HASH
)
if [ -f "$destination/source.sha256" ] && [ "$(cat "$destination/source.sha256")" = "$fingerprint" ] &&
   python3 -I "$destination/native/bridge/validate-bundle.py" "$destination/OVRCR Bridge.app" >/dev/null 2>&1; then
    exit
fi
parent=$(dirname -- "$destination")
mkdir -p "$parent"
stage=$(mktemp -d "$parent/.ovrcr-startup-build.XXXXXX")
trap 'rm -rf "$stage"' EXIT
trap 'exit 130' HUP INT TERM
payload="$stage/payload"
mkdir -p "$payload/native/bridge" "$payload/scripts"
sh "$repo_dir/scripts/build-bridge.sh" "$payload"
cp "$repo_dir/scripts/install-bridge.sh" "$payload/scripts/"
cp "$repo_dir/native/bridge/validate-bundle.py" "$payload/native/bridge/"
python3 -I - "$payload" <<'PY_CONTRACT'
import hashlib, json, plistlib, sys
from pathlib import Path
payload = Path(sys.argv[1])
bundle = payload / "OVRCR Bridge.app/Contents"
info = plistlib.loads((bundle / "Info.plist").read_bytes())
resources = bundle / "Resources"
manifest = (resources / "NotificationSounds-manifest.json").read_bytes()
contract = dict(wire=info["OVRCRServerWire"], schema=info["OVRCRBridgeSchema"], build=info["OVRCRBridgeBuild"],
                manifest=json.loads(manifest), manifest_sha256=hashlib.sha256(manifest).hexdigest(),
                license_sha256=hashlib.sha256((resources / "NotificationSounds-LICENSE").read_bytes()).hexdigest())
(payload / "native/bridge/expected-contract.json").write_text(json.dumps(contract))
PY_CONTRACT
printf '%s\n' "$fingerprint" > "$payload/source.sha256"
python3 -I "$payload/native/bridge/validate-bundle.py" "$payload/OVRCR Bridge.app"
if [ -e "$destination" ]; then mv "$destination" "$stage/previous"; fi
if ! mv "$payload" "$destination"; then
    if [ -e "$stage/previous" ] && [ ! -e "$destination" ]; then mv "$stage/previous" "$destination"; fi
    echo 'Startup payload publication failed' >&2
    exit 74
fi
