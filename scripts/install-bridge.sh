#!/bin/sh
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
identity=
identity_given=false
local_development=false
destination="$HOME/Applications/OVRCR Bridge.app"
destination_given=false
expected_id=com.ovrcr.bridge
expected_display=OVRCR
metadata_given=false
source_bundle="$repo_dir/target/bridge/OVRCR Bridge.app"
source_given=false
while [ "$#" -gt 0 ]; do
    case "$1" in
        --local-development) local_development=true; shift;;
        --identity|--destination|--bundle-id|--display-name)
            [ "$#" -ge 2 ] || { echo 'Missing installer argument' >&2; exit 64; }
            case "$1" in
                --identity) identity=$2; identity_given=true;;
                --destination) destination=$2; destination_given=true;;
                --bundle-id) expected_id=$2; metadata_given=true;;
                --display-name) expected_display=$2; metadata_given=true;;
            esac
            shift 2;;
        --*) echo 'Unknown installer option' >&2; exit 64;;
        *) source_bundle=$1; source_given=true; shift; [ "$#" -eq 0 ] || { echo 'Unexpected installer argument' >&2; exit 64; };;
    esac
done
app_name='OVRCR Bridge.app'
if [ "$local_development" = true ]; then
    [ "$identity_given" = false ] && [ "$metadata_given" = false ] || { echo 'Local development mode refuses identity and metadata overrides' >&2; exit 64; }
    app_name='OVRCR Bridge Local.app'
    expected_id=com.ovrcr.bridge.local
    expected_display='OVRCR Local'
    if [ "$destination_given" = false ]; then destination="$HOME/Applications/$app_name"; fi
    if [ "$source_given" = false ]; then source_bundle="$repo_dir/target/bridge/$app_name"; fi
    [ "${destination##*/}" = "$app_name" ] || { echo 'Local development destination must be OVRCR Bridge Local.app' >&2; exit 64; }
    if [ -e "$destination" ] || [ -L "$destination" ]; then
        echo 'Local development installation requires a fresh destination; existing path retained' >&2
        exit 65
    fi
    python3 -I - "$destination" "$HOME/Applications/OVRCR Bridge.app" <<'PY'
import sys
from pathlib import Path
if Path(sys.argv[1]).resolve() == Path(sys.argv[2]).resolve():
    raise SystemExit('Local development destination must differ from the production installation')
PY
else
    case "$identity" in ''|-) echo 'An explicit non-ad-hoc --identity is required' >&2; exit 64;; esac
fi
[ -n "$destination" ] && [ -n "$expected_id" ] && [ -n "$expected_display" ] || { echo 'Installer destination/identity/display name must be nonempty' >&2; exit 64; }
[ ! -L "$destination" ] || { echo 'Destination must not be a symbolic link' >&2; exit 65; }
if [ "$local_development" = false ] && { [ "$expected_id" != com.ovrcr.bridge ] || [ "$expected_display" != OVRCR ]; }; then
    [ "$destination_given" = true ] || { echo 'Fixture identity/name requires an explicit separate destination' >&2; exit 64; }
    python3 -I - "$destination" "$HOME/Applications/OVRCR Bridge.app" <<'PY'
import sys
from pathlib import Path
if Path(sys.argv[1]).resolve() == Path(sys.argv[2]).resolve():
    raise SystemExit('Fixture destination must differ from the production installation')
PY
fi
validate_bundle() {
    if [ "$local_development" = true ]; then
        python3 -I "$repo_dir/native/bridge/validate-bundle.py" --local-development "$1"
    else
        python3 -I "$repo_dir/native/bridge/validate-bundle.py" "$1" "$expected_id" "$expected_display"
    fi
}
validate_bundle "$source_bundle"
parent_dir=$(dirname -- "$destination")
mkdir -p "$parent_dir"
stage_dir=$(mktemp -d "$parent_dir/.ovrcr-bridge-install.XXXXXX")
staged="$stage_dir/$app_name"
backup="$stage_dir/previous.app"
installed=false
# Retain local staging after copy, signing or verification failures.
# Production keeps its existing backup restoration and cleanup behavior.
retain_stage=$local_development
cleanup() {
    if [ "$retain_stage" = true ]; then
        if [ -e "$staged" ] || [ -L "$staged" ]; then
            printf 'Local staged application retained for inspection: %s\n' "$staged" >&2
        else
            printf 'Local installation staging retained for inspection: %s\n' "$stage_dir" >&2
        fi
    elif [ -e "$backup" ] && [ "$installed" != true ]; then
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
publish_local_bundle() {
    python3 -I - "$1" "$2" <<'PY'
import ctypes, os, sys
# macOS SDK sys/stdio.h: RENAME_EXCL=0x00000004, available since macOS 10.12.
# Refuse a destination created after preflight; never replace even an empty app.
rename = ctypes.CDLL(None, use_errno=True).renamex_np
rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
rename.restype = ctypes.c_int
if rename(os.fsencode(sys.argv[1]), os.fsencode(sys.argv[2]), 0x00000004) != 0:
    error = ctypes.get_errno()
    raise OSError(error, os.strerror(error), sys.argv[2])
PY
}
ditto "$source_bundle" "$staged"
if [ "$local_development" = true ]; then
    codesign --force --options runtime --timestamp=none --entitlements "$staged/Contents/Resources/OVRCRBridge.entitlements" --sign - "$staged"
else
    codesign --force --options runtime --timestamp --entitlements "$staged/Contents/Resources/OVRCRBridge.entitlements" --sign "$identity" "$staged"
fi
codesign --verify --deep --strict "$staged"
codesign -d --entitlements - --xml "$staged" > "$stage_dir/signed-entitlements.plist"
python3 -I - "$stage_dir/signed-entitlements.plist" <<'PY'
import plistlib, sys
with open(sys.argv[1], 'rb') as file:
    entitlements = plistlib.load(file)
assert set(entitlements) == {'com.apple.security.automation.apple-events'}
assert entitlements['com.apple.security.automation.apple-events'] is True
PY
validate_bundle "$staged"
schema=$(/usr/libexec/PlistBuddy -c 'Print :OVRCRBridgeSchema' "$staged/Contents/Info.plist")
wire=$(/usr/libexec/PlistBuddy -c 'Print :OVRCRServerWire' "$staged/Contents/Info.plist")
"$staged/Contents/MacOS/OVRCRBridge" --check-contract "$schema" "$wire"
if [ "$local_development" = false ] && [ -e "$destination" ]; then
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
if [ "$local_development" = true ]; then
    if ! publish_local_bundle "$staged" "$destination"; then
        retain_stage=true
        echo 'Local development publication refused; destination retained' >&2
        exit 74
    fi
elif ! rename_bundle "$staged" "$destination"; then
    if [ -e "$backup" ] && [ ! -e "$destination" ]; then rename_bundle "$backup" "$destination" || true; fi
    echo 'Installation rename failed' >&2; exit 74
fi
installed=true
retain_stage=false
printf 'Installed %s. Existing Bridge processes are not restarted; no permission or notification operation was run.\n' "$destination"
