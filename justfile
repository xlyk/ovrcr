set positional-arguments

default:
    @rtk proxy just --list

# Build, refresh the PATH-installed CLI, then launch it.
run *args:
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
    fi
    mkdir -p "$HOME/.local/bin"
    pending="$(mktemp "$HOME/.local/bin/.ovrcr.XXXXXX")"
    trap 'rm -f "$pending"' EXIT
    install -m 755 "${CARGO_TARGET_DIR:-target}/debug/ovrcr" "$pending"
    mv -f "$pending" "$HOME/.local/bin/ovrcr"
    ovrcr_bin="$HOME/.local/bin/ovrcr"

    # Startup checks belong to the client; callbacks and inspection skip them.
    "$ovrcr_bin" "$@"

run-release *args:
    rtk proxy cargo run -p ovrcr --release -- "$@"

# macOS: package Bridge startup assets for an installed CLI (default ~/.local/bin/ovrcr)
# into ~/.local/lib/ovrcr so client startup can install or repair the Bridge.
install-startup-assets *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "$(uname -s)" != Darwin ]]; then
      echo "Bridge startup assets are macOS-only" >&2
      exit 64
    fi
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
    if [[ "$#" -gt 1 ]]; then
      echo "Usage: just install-startup-assets [CLI_EXECUTABLE]" >&2
      exit 64
    fi
    cli="${1:-$HOME/.local/bin/ovrcr}"
    payload="${CARGO_TARGET_DIR:-target}/startup-assets/$payload_name"
    rtk proxy sh scripts/package-startup.sh ${bridge_package_flag:+"$bridge_package_flag"} "$payload" "$cli"
    sh scripts/publish-startup-assets.sh ${bridge_package_flag:+"$bridge_package_flag"} "$payload"
    echo "Startup assets for $cli published; restart OVRCR to review the Bridge install or repair offer."

# Build a release CLI, install it at ~/.local/bin/ovrcr and, on macOS, its Bridge startup assets.
install-release:
    #!/usr/bin/env bash
    set -euo pipefail
    rtk proxy cargo build -p ovrcr --bin ovrcr --release --target-dir "${CARGO_TARGET_DIR:-target}"
    mkdir -p "$HOME/.local/bin"
    pending="$(mktemp "$HOME/.local/bin/.ovrcr.XXXXXX")"
    trap 'rm -f "$pending"' EXIT
    install -m 755 "${CARGO_TARGET_DIR:-target}/release/ovrcr" "$pending"
    mv -f "$pending" "$HOME/.local/bin/ovrcr"
    if [[ "$(uname -s)" == Darwin ]]; then
      just install-startup-assets "$HOME/.local/bin/ovrcr"
    fi

# Controlled stop of the selected instance, then rebuild and launch; failures stay explicit.
restart *args:
    #!/usr/bin/env bash
    set -euo pipefail
    rtk proxy cargo run -p ovrcr -- shutdown --kill
    rtk proxy just run "$@"

build:
    rtk proxy cargo build --workspace

build-release:
    rtk proxy cargo build --workspace --release

# Stage the macOS app; no installation-identity signing, installation or launch.
bridge-build *args:
    rtk proxy bash scripts/build-bridge.sh "$@"

# Pure headless native protocol/transport checks; no notification permission or app launch.
bridge-check *args:
    rtk proxy bash scripts/build-bridge.sh --check "$@"

check:
    rtk proxy cargo check --workspace --all-targets

fmt:
    rtk proxy cargo fmt --all

fmt-check:
    rtk proxy cargo fmt --all -- --check

lint:
    rtk proxy cargo clippy --workspace --all-targets --all-features -- -D warnings

test *args:
    rtk proxy cargo test --workspace --all-targets --all-features "$@"

verify: fmt-check check lint test

# Launch the optional macOS terminal test helper with a disposable demo.
gui:
    rtk proxy sh scripts/gui.sh
