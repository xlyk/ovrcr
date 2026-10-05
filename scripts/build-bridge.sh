#!/bin/sh
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mode=build
if [ "${1:-}" = --check ]; then mode=check; shift; fi
if [ "$#" -gt 1 ]; then echo 'Usage: scripts/build-bridge.sh [--check] [OUTPUT_ROOT]' >&2; exit 64; fi
output_root=${1:-"$repo_dir/target/bridge"}
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
    xcrun --sdk macosx swiftc -swift-version 5 -module-cache-path "$stage_dir/cache" \
        "$stage_dir/Version.swift" "$repo_dir/native/bridge/Contract.swift" \
        "$repo_dir/native/bridge/Transport.swift" "$repo_dir/native/bridge/headless/main.swift" \
        -o "$stage_dir/headless-checks"
    "$stage_dir/headless-checks"
    exit
fi
bundle="$stage_dir/OVRCR Bridge.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
xcrun --sdk macosx swiftc -swift-version 5 -O -target "$(uname -m)-apple-macosx11.0" \
    -module-cache-path "$stage_dir/cache" "$stage_dir/Version.swift" \
    "$repo_dir/native/bridge/Contract.swift" "$repo_dir/native/bridge/Transport.swift" \
    "$repo_dir/native/bridge/App.swift" "$repo_dir/native/bridge/main.swift" \
    -o "$bundle/Contents/MacOS/OVRCRBridge"
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
</dict></plist>
EOF
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
