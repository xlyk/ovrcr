set positional-arguments

default:
    @rtk proxy just --list

# Build, refresh the PATH-installed CLI, then launch it.
run *args:
    #!/usr/bin/env bash
    set -euo pipefail
    rtk proxy cargo build -p ovrcr --bin ovrcr --target-dir "${CARGO_TARGET_DIR:-target}"
    mkdir -p "$HOME/.local/bin"
    pending="$(mktemp "$HOME/.local/bin/.ovrcr.XXXXXX")"
    trap 'rm -f "$pending"' EXIT
    install -m 755 "${CARGO_TARGET_DIR:-target}/debug/ovrcr" "$pending"
    mv -f "$pending" "$HOME/.local/bin/ovrcr"
    "$HOME/.local/bin/ovrcr" "$@"

run-release *args:
    rtk proxy cargo run -p ovrcr --release -- "$@"

# Stop a leftover local server (including an older build that cannot handshake) and start the dashboard.
restart *args:
    #!/usr/bin/env bash
    set -eu
    rtk proxy cargo run -p ovrcr -- shutdown --kill || true
    if [[ -n "${OVRCR_SOCKET:-}" ]]; then
      socket="$OVRCR_SOCKET"
    else
      socket="${TMPDIR:-/tmp}"
      socket="${socket%/}/ovrcr-$(id -u)/ovrcr/server.sock"
    fi
    if [[ -e "$socket" ]]; then
      pids="$(lsof -nP -U 2>/dev/null | awk -v s="$socket" '$NF == s { print $2 }' | sort -u || true)"
      if [[ -n "${pids}" ]]; then
        kill ${pids} 2>/dev/null || true
        sleep 0.2
        kill -KILL ${pids} 2>/dev/null || true
      fi
    fi
    rtk proxy cargo run -p ovrcr -- "$@"

build:
    rtk proxy cargo build --workspace

build-release:
    rtk proxy cargo build --workspace --release

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
