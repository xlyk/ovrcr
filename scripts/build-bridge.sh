#!/bin/sh
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mode=build
callback_executable=
output_root="$repo_dir/target/bridge"
output_given=false
while [ "$#" -gt 0 ]; do
    case "$1" in
        --check) mode=check; shift;;
        --callback-executable)
            [ "$#" -ge 2 ] || { echo 'Missing callback executable' >&2; exit 64; }
            callback_executable=$2; shift 2;;
        --*) echo 'Unknown build option' >&2; exit 64;;
        *) [ "$output_given" = false ] || { echo 'Unexpected output root' >&2; exit 64; }
            output_root=$1; output_given=true; shift;;
    esac
done
if [ "$mode" = build ] && [ -z "$callback_executable" ]; then
    echo 'Build requires --callback-executable with the reviewed CLI matching the running Server' >&2
    exit 64
fi
mkdir -p "$output_root"
stage_dir=$(mktemp -d "$output_root/.bridge-build.XXXXXX")
trap 'rm -rf "$stage_dir"' EXIT
trap 'exit 130' HUP INT TERM
wire=$(sed -n 's/^pub const PROTOCOL_VERSION: u32 = \([0-9][0-9]*\);$/\1/p' "$repo_dir/crates/ovrcr-protocol/src/codec.rs")
schema=$(sed -n 's/^pub const BRIDGE_SCHEMA_VERSION: u32 = \([0-9][0-9]*\);$/\1/p' "$repo_dir/crates/ovrcr-protocol/src/bridge.rs")
case "$wire:$schema" in *[!0-9:]*|:*|*:) echo 'Cannot derive Bridge versions' >&2; exit 65;; esac
cat > "$stage_dir/Version.swift" <<EOF
let bridgeSchema: UInt32 = $schema
let bridgeServerWire: UInt32 = $wire
EOF
if [ "$mode" = check ]; then
    # These runners use parser/early guards and injected owner/permission/
    # selector doubles. They cannot establish native consent or visible focus.
    for runner in headless headless-iterm headless-iterm-setup headless-iterm-workflow; do
        sound_checks_source=
        if [ "$runner" = headless ]; then
            sound_checks_source="$repo_dir/native/bridge/headless/SoundChecks.swift"
        fi
        xcrun --sdk macosx swiftc -swift-version 5 -module-cache-path "$stage_dir/cache" \
            "$stage_dir/Version.swift" "$repo_dir/native/bridge/Contract.swift" \
            "$repo_dir/native/bridge/Transport.swift" "$repo_dir/native/bridge/Sounds.swift" \
            "$repo_dir/native/bridge/Navigation.swift" \
            "$repo_dir/native/bridge/ITermFocus.swift" "$repo_dir/native/bridge/ITermWorkflow.swift" \
            "$repo_dir/native/bridge/ITermSetup.swift" ${sound_checks_source:+"$sound_checks_source"} \
            "$repo_dir/native/bridge/$runner/main.swift" \
            -o "$stage_dir/$runner-checks"
        "$stage_dir/$runner-checks" "$repo_dir"
    done
    exit
fi
bundle="$stage_dir/OVRCR Bridge.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
callback_hash=$(python3 -I - "$callback_executable" "$bundle/Contents/MacOS/ovrcr" <<'PY'
import hashlib, os, stat, sys
from pathlib import Path
source, destination = map(Path, sys.argv[1:])
assert source.is_absolute() and source.name == 'ovrcr' and not source.is_symlink()
descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
with os.fdopen(descriptor, 'rb') as original, destination.open('xb') as output:
    before = os.fstat(original.fileno())
    assert stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
    assert before.st_mode & 0o111 and 0 < before.st_size <= 512 * 1024 * 1024
    digest = hashlib.sha256()
    count = 0
    for chunk in iter(lambda: original.read(65536), b''):
        count += len(chunk)
        assert count <= before.st_size
        digest.update(chunk)
        output.write(chunk)
    after = os.fstat(original.fileno())
    assert count == before.st_size
    assert (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)
    named = source.lstat()
    assert stat.S_ISREG(named.st_mode) and (named.st_dev, named.st_ino) == (before.st_dev, before.st_ino)
destination.chmod(0o755)
print(digest.hexdigest())
PY
)
# Preserve reviewed bytes. Re-signing a helper would change its compatibility hash.
codesign --verify --strict "$bundle/Contents/MacOS/ovrcr"
xcrun --sdk macosx swiftc -swift-version 5 -O -target "$(uname -m)-apple-macosx11.0" \
    -module-cache-path "$stage_dir/cache" "$stage_dir/Version.swift" \
    "$repo_dir/native/bridge/Contract.swift" "$repo_dir/native/bridge/Transport.swift" \
    "$repo_dir/native/bridge/Sounds.swift" \
    "$repo_dir/native/bridge/Navigation.swift" "$repo_dir/native/bridge/ITermFocus.swift" \
    "$repo_dir/native/bridge/ITermWorkflow.swift" "$repo_dir/native/bridge/ITermSetup.swift" \
    "$repo_dir/native/bridge/App.swift" \
    "$repo_dir/native/bridge/main.swift" \
    -o "$bundle/Contents/MacOS/OVRCRBridge"
build=$(python3 -I - "$bundle/Contents/MacOS/OVRCRBridge" <<'PY_HASH'
import hashlib, sys
from pathlib import Path
print(hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest())
PY_HASH
)
cat > "$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>OVRCRBridge</string>
<key>CFBundleIdentifier</key><string>com.ovrcr.bridge</string>
<key>CFBundleName</key><string>OVRCR Bridge</string>
<key>CFBundleDisplayName</key><string>OVRCR</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>$schema.$wire</string>
<key>CFBundleShortVersionString</key><string>1.0</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>LSUIElement</key><true/>
<key>OVRCRBridgeSchema</key><integer>$schema</integer>
<key>OVRCRServerWire</key><integer>$wire</integer>
<key>OVRCRCallbackSHA256</key><string>$callback_hash</string>
<key>NSAppleEventsUsageDescription</key><string>OVRCR can select the existing iTerm session hosting your current Dashboard after you explicitly set up iTerm focus.</string>
<key>OVRCRBridgeBuild</key><string>$build</string>
</dict></plist>
EOF
cp "$repo_dir/native/bridge/entitlements.plist" "$bundle/Contents/OVRCRBridge.entitlements"
for tone in tap chime rise; do
    cp "$repo_dir/research/notification-bridge/sounds/ovrcr-$tone-v1.wav" "$bundle/Contents/Resources/"
done
cp "$repo_dir/research/notification-bridge/sounds/LICENSE" "$bundle/Contents/Resources/NotificationSounds-LICENSE"
cp "$repo_dir/research/notification-bridge/sounds/manifest.json" "$bundle/Contents/Resources/NotificationSounds-manifest.json"
python3 -I "$repo_dir/native/bridge/validate-bundle.py" "$bundle"
"$bundle/Contents/MacOS/OVRCRBridge" --check-contract "$schema" "$wire"
destination="$output_root/OVRCR Bridge.app"
if [ -e "$destination" ]; then echo 'Output bundle exists; choose a fresh output root' >&2; exit 73; fi
mv "$bundle" "$destination"
printf '%s\n' "$destination"
