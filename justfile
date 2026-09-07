set positional-arguments

default:
    @rtk proxy just --list

run *args:
    rtk proxy cargo run -p ovrcr -- "$@"

run-release *args:
    rtk proxy cargo run -p ovrcr --release -- "$@"

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
    rtk proxy cargo test --workspace "$@"

verify: fmt-check check lint test

# Launch the optional macOS terminal test helper with a disposable demo.
gui:
    rtk proxy sh scripts/gui.sh
