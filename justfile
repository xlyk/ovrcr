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
    ovrcr_bin="$HOME/.local/bin/ovrcr"

    # Reuse the real protocol handshake (via `list`) so an outdated long-running
    # server fails the same way a later launch would, before we continue.
    set +e
    list_out="$("$ovrcr_bin" list 2>&1)"
    list_status=$?
    set -e
    if [[ "$list_status" -ne 0 ]]; then
      if [[ "$list_out" == *"protocol version mismatch"* || "$list_out" == *"older OVRCR build"* ]]; then
        printf '%s\n' "$list_out" >&2
        if [[ ! -t 0 ]]; then
          echo "Existing ovrcr server is out of date with this client; re-run interactively to confirm pkill, or use \`just restart\`." >&2
          exit 1
        fi
        printf 'Existing ovrcr server is out of date with this client. pkill it and continue? [y/n] ' >&2
        read -r answer
        case "$answer" in
          y|Y|yes|YES)
            pkill -f 'ovrcr server' 2>/dev/null || true
            # Match ServerPaths::resolve so the socket we clear is the one `list` used.
            if [[ -n "${OVRCR_SOCKET:-}" ]]; then
              socket="$OVRCR_SOCKET"
            elif [[ "$(uname -s)" == Linux && -n "${XDG_RUNTIME_DIR:-}" ]]; then
              socket="${XDG_RUNTIME_DIR%/}/ovrcr/server.sock"
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
            sleep 0.2
            ;;
          *)
            echo "Aborted: left the existing ovrcr server running. Use \`just restart\` or answer y next time." >&2
            exit 1
            ;;
        esac
      fi
    fi

    "$ovrcr_bin" __retarget-hooks
    "$ovrcr_bin" "$@"

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
