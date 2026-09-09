set positional-arguments

default:
    @rtk proxy just --list

run *args:
    rtk proxy cargo run -p ovrcr -- "$@"

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
