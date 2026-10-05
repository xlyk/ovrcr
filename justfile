set positional-arguments

default:
    @rtk proxy just --list

# Build, refresh the PATH-installed CLI, then launch it.
run *args:
    #!/usr/bin/env bash
    set -euo pipefail
    rtk proxy cargo build -p ovrcr --bin ovrcr --target-dir "${CARGO_TARGET_DIR:-target}"
    if [[ "$(uname -s)" == Darwin ]]; then
      rtk proxy sh scripts/package-startup.sh "${CARGO_TARGET_DIR:-target}/debug/ovrcr-startup" "${CARGO_TARGET_DIR:-target}/debug/ovrcr"
      mkdir -p "$HOME/.local/lib"
      assets="$HOME/.local/lib/ovrcr"
      if [[ -L "$assets" ]] || { [[ -e "$assets" ]] && [[ ! -f "$assets/native/bridge/expected-contract.json" ]]; }; then
        echo "Refusing to replace unmanaged startup assets at $assets" >&2
        exit 1
      fi
      asset_stage="$(mktemp -d "$HOME/.local/lib/.ovrcr-startup.XXXXXX")"
      cp -R "${CARGO_TARGET_DIR:-target}/debug/ovrcr-startup" "$asset_stage/current"
      if [[ -e "$assets" ]]; then mv "$assets" "$asset_stage/previous"; fi
      if ! mv "$asset_stage/current" "$assets"; then
        if [[ -e "$asset_stage/previous" ]] && [[ ! -e "$assets" ]]; then
          mv "$asset_stage/previous" "$assets" || { echo "Previous assets retained at $asset_stage/previous" >&2; exit 1; }
        fi
        rm -rf "$asset_stage"
        exit 1
      fi
      rm -rf "$asset_stage"
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
