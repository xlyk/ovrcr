#!/bin/sh
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
identity=
destination="$HOME/Applications/OVRCR Bridge.app"
destination_given=false
expected_id=com.ovrcr.bridge
expected_display=OVRCR
source_bundle="$repo_dir/target/bridge/OVRCR Bridge.app"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --identity|--destination|--bundle-id|--display-name)
            [ "$#" -ge 2 ] || { echo 'Missing installer argument' >&2; exit 64; }
            case "$1" in
                --identity) identity=$2;;
                --destination) destination=$2; destination_given=true;;
                --bundle-id) expected_id=$2;;
                --display-name) expected_display=$2;;
            esac
            shift 2;;
        --*) echo 'Unknown installer option' >&2; exit 64;;
        *) source_bundle=$1; shift; [ "$#" -eq 0 ] || { echo 'Unexpected installer argument' >&2; exit 64; };;
    esac
done
case "$identity" in ''|-) echo 'An explicit non-ad-hoc --identity is required' >&2; exit 64;; esac
[ -n "$destination" ] && [ -n "$expected_id" ] && [ -n "$expected_display" ] || { echo 'Installer destination/identity/display name must be nonempty' >&2; exit 64; }
[ ! -L "$destination" ] || { echo 'Destination must not be a symbolic link' >&2; exit 65; }
if [ "$expected_id" != com.ovrcr.bridge ] || [ "$expected_display" != OVRCR ]; then
    [ "$destination_given" = true ] || { echo 'Fixture identity/name requires an explicit separate destination' >&2; exit 64; }
    python3 -I - "$destination" "$HOME/Applications/OVRCR Bridge.app" <<'PY'
import sys
from pathlib import Path
if Path(sys.argv[1]).resolve() == Path(sys.argv[2]).resolve():
    raise SystemExit('Fixture destination must differ from the production installation')
PY
fi
python3 -I "$repo_dir/native/bridge/validate-bundle.py" "$source_bundle" "$expected_id" "$expected_display"
parent_dir=$(dirname -- "$destination")
mkdir -p "$parent_dir"
stage_dir=$(mktemp -d "$parent_dir/.ovrcr-bridge-install.XXXXXX")
backup="$stage_dir/previous.app"
installed=false
cleanup() {
    if [ -e "$backup" ] && [ "$installed" != true ]; then
        printf 'Previous application retained for recovery: %s\n' "$backup" >&2
    else
        rm -rf "$stage_dir"
    fi
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM
rename_bundle() {
    python3 -I - "$1" "$2" <<'PY'
import os, sys
os.rename(sys.argv[1], sys.argv[2])
PY
}
staged="$stage_dir/OVRCR Bridge.app"
ditto "$source_bundle" "$staged"
codesign --force --options runtime --timestamp --entitlements "$staged/Contents/OVRCRBridge.entitlements" --sign "$identity" "$staged"
codesign --verify --deep --strict "$staged"
codesign -d --entitlements - "$staged" > "$stage_dir/signed-entitlements.plist"
python3 -I - "$stage_dir/signed-entitlements.plist" <<'PY'
import plistlib, sys
with open(sys.argv[1], 'rb') as file:
    entitlements = plistlib.load(file)
assert set(entitlements) == {'com.apple.security.automation.apple-events'}
assert entitlements['com.apple.security.automation.apple-events'] is True
PY
python3 -I "$repo_dir/native/bridge/validate-bundle.py" "$staged" "$expected_id" "$expected_display"
schema=$(/usr/libexec/PlistBuddy -c 'Print :OVRCRBridgeSchema' "$staged/Contents/Info.plist")
wire=$(/usr/libexec/PlistBuddy -c 'Print :OVRCRServerWire' "$staged/Contents/Info.plist")
"$staged/Contents/MacOS/OVRCRBridge" --check-contract "$schema" "$wire"
if [ -e "$destination" ]; then
    installed_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$destination/Contents/Info.plist")
    [ "$installed_id" = "$expected_id" ] || { echo 'Existing destination belongs to another application' >&2; exit 65; }
    codesign --verify --deep --strict "$destination"
    codesign -d -r- "$destination" > "$stage_dir/installed-requirement.txt" 2>&1
    codesign -d -r- "$staged" > "$stage_dir/staged-requirement.txt" 2>&1
    sed -n '/^designated =>/p' "$stage_dir/installed-requirement.txt" > "$stage_dir/installed-dr.txt"
    sed -n '/^designated =>/p' "$stage_dir/staged-requirement.txt" > "$stage_dir/staged-dr.txt"
    [ -s "$stage_dir/installed-dr.txt" ] && [ -s "$stage_dir/staged-dr.txt" ] || { echo 'Cannot compare signing requirements' >&2; exit 65; }
    cmp -s "$stage_dir/installed-dr.txt" "$stage_dir/staged-dr.txt" || { echo 'Signing requirement changed; refusing update' >&2; exit 65; }
    rename_bundle "$destination" "$backup"
fi
if ! rename_bundle "$staged" "$destination"; then
    if [ -e "$backup" ] && [ ! -e "$destination" ]; then rename_bundle "$backup" "$destination" || true; fi
    echo 'Installation rename failed' >&2; exit 74
fi
installed=true
printf 'Installed %s. Existing Bridge processes are not restarted; no permission or notification operation was run.\n' "$destination"
