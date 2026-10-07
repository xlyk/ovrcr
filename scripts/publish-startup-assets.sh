#!/bin/sh
# Atomically publish a packaged Bridge startup payload beside the PATH CLI:
# ~/.local/lib/ovrcr (or ovrcr-local-development). No build, signing or app install.
set -eu
assets_name=ovrcr
if [ "${1-}" = --local-development ]; then
    assets_name=ovrcr-local-development
    shift
fi
[ "$#" -eq 1 ] || { echo 'Usage: scripts/publish-startup-assets.sh [--local-development] PAYLOAD_DIRECTORY' >&2; exit 64; }
payload=$1
[ -f "$payload/native/bridge/expected-contract.json" ] || { echo "Not a packaged startup payload: $payload" >&2; exit 65; }
mkdir -p "$HOME/.local/lib"
assets="$HOME/.local/lib/$assets_name"
if [ -L "$assets" ] || { [ -e "$assets" ] && [ ! -f "$assets/native/bridge/expected-contract.json" ]; }; then
    echo "Refusing to replace unmanaged startup assets at $assets" >&2
    exit 1
fi
asset_stage=$(mktemp -d "$HOME/.local/lib/.ovrcr-startup.XXXXXX")
cp -R "$payload" "$asset_stage/current"
if [ -e "$assets" ]; then mv "$assets" "$asset_stage/previous"; fi
if ! mv "$asset_stage/current" "$assets"; then
    if [ -e "$asset_stage/previous" ] && [ ! -e "$assets" ]; then
        mv "$asset_stage/previous" "$assets" || { echo "Previous assets retained at $asset_stage/previous" >&2; exit 1; }
    fi
    rm -rf "$asset_stage"
    exit 1
fi
rm -rf "$asset_stage"
