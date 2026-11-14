#!/bin/sh
set -eu
package=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -z "${DEVELOPER_DIR:-}" ] && [ -d /Library/Developer/CommandLineTools ]; then
    export DEVELOPER_DIR=/Library/Developer/CommandLineTools
fi
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
for check in HostConfiguration TerminalFontSettings; do
    xcrun swiftc "$package/Sources/OVRCRApp/$check.swift" "$package/checks/${check}Tests.swift" -o "$stage/$check"
    "$stage/$check"
done
codesign --verify --deep --strict "$1"
python3 -I "$package/checks/launch.py" "$1"
