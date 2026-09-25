#!/bin/sh
set -eu
cd "$(rtk proxy dirname "$0")/.."
rtk proxy cargo build -p ovrcr --features gui --bins
bundle="$PWD/target/OVRCR GUI.app"
rtk proxy mkdir -p "$bundle/Contents/MacOS"
# Replace, never overwrite in place: writing over a signed binary that a previous
# fixture still runs invalidates its signature and every later exec is SIGKILLed.
rtk proxy rm -f "$bundle/Contents/MacOS/ovrcr" "$bundle/Contents/MacOS/ovrcr-gui"
rtk proxy cp target/debug/ovrcr target/debug/ovrcr-gui "$bundle/Contents/MacOS/"
rtk proxy cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>ovrcr-gui</string>
<key>CFBundleIdentifier</key><string>dev.ovrcr.gui</string>
<key>CFBundleName</key><string>OVRCR GUI</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
# Launch directly to retain the invoking shell's environment and report errors.
cd "$bundle/Contents/MacOS"
exec rtk proxy ./ovrcr-gui
