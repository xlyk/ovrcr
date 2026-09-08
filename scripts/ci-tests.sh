#!/usr/bin/env bash
# Coarse CI groups: new integration test files fall into core unless classified
# below. GUI targets run separately on macOS with their required feature.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

suite=${1:?expected core, lifecycle, or tasks}
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
