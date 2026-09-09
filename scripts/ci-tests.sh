#!/usr/bin/env bash
# Coarse CI groups: new integration test files fall into core unless classified
# below. On macOS, the core suite runs the full --all-features workspace gate
# in one shot instead of the per-target loop; that also covers the GUI
# feature and its tests/gui.rs integration test, since the GUI feature has no
# Linux backend and does not run there.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

suite=${1:?expected core, lifecycle, or tasks}

if [[ "$suite" == "core" && "$(uname -s)" == "Darwin" ]]; then
  cargo test --workspace --all-targets --all-features
  exit 0
fi

case "$suite" in
  core)
    cargo test --workspace --lib --bins
    cargo test --workspace --doc
    ;;
  lifecycle|tasks) ;;
  *) printf 'Unknown CI suite: %s\n' "$suite" >&2; exit 2 ;;
esac

targets=()
for path in tests/*.rs; do
  name=${path##*/}
  name=${name%.rs}
  case "$name" in
    gui) continue ;;
    server_lifecycle|terminal_acceptance) group=lifecycle ;;
    task_*) group=tasks ;;
    *) group=core ;;
  esac
  if [[ "$group" == "$suite" ]]; then
    targets+=(--test "$name")
  fi
done

# Fail instead of accidentally running a broader/default suite on an empty list.
if [[ ${#targets[@]} == 0 ]]; then
  printf 'No integration targets for CI suite: %s\n' "$suite" >&2
  exit 1
fi
cargo test -p ovrcr "${targets[@]}"
