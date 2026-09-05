set positional-arguments

default:
    @rtk proxy just --list

run *args:
    rtk proxy cargo run -- "$@"

run-release *args:
    rtk proxy cargo run --release -- "$@"

build:
    rtk proxy cargo build

build-release:
    rtk proxy cargo build --release

check:
    rtk proxy cargo check --all-targets

fmt:
    rtk proxy cargo fmt --all

fmt-check:
    rtk proxy cargo fmt --all -- --check

lint:
    rtk proxy cargo clippy --all-targets --all-features -- -D warnings

test *args:
    rtk proxy cargo test "$@"

verify: fmt-check check lint test

# Launch the optional macOS terminal test helper with a disposable demo.
gui:
    rtk proxy sh scripts/gui.sh
