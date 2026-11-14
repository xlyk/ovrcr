#!/usr/bin/env bash
set -euo pipefail
bridge_package_flag=""
payload_name="ovrcr-startup"
case "${OVRCR_BRIDGE_LOCAL_DEVELOPMENT-}" in
  ""|0) ;;
  1)
    bridge_package_flag="--local-development"
    payload_name="ovrcr-startup-local-development"
    ;;
  *)
    echo "OVRCR_BRIDGE_LOCAL_DEVELOPMENT must be unset, 0 or 1" >&2
    exit 64
    ;;
esac
rtk proxy cargo build -p ovrcr --bin ovrcr --target-dir "${CARGO_TARGET_DIR:-target}"
if [[ "$(uname -s)" == Darwin ]]; then
  rtk proxy sh scripts/package-startup.sh ${bridge_package_flag:+"$bridge_package_flag"} "${CARGO_TARGET_DIR:-target}/debug/$payload_name" "${CARGO_TARGET_DIR:-target}/debug/ovrcr"
  sh scripts/publish-startup-assets.sh ${bridge_package_flag:+"$bridge_package_flag"} "${CARGO_TARGET_DIR:-target}/debug/$payload_name"
  python3 -I scripts/install-cursor-bridge.py ${OVRCR_CURSOR_BRIDGE_ARCHIVE:+"$OVRCR_CURSOR_BRIDGE_ARCHIVE"}
fi
mkdir -p "$HOME/.local/bin"
pending="$(mktemp "$HOME/.local/bin/.ovrcr.XXXXXX")"
trap 'rm -f "$pending"' EXIT
install -m 755 "${CARGO_TARGET_DIR:-target}/debug/ovrcr" "$pending"
mv -f "$pending" "$HOME/.local/bin/ovrcr"
