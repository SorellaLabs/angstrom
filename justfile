default:
    @just --list

ci: check-format check-clippy test test-integration

check: check-format check-clippy test

fix: fix-format fix-clippy

test:
    cargo nextest run --workspace --lib

test-integration:
    cargo nextest run --workspace --tests

test-anvil:
    cargo nextest run -p angstrom-types --features anvil --test anvil_settlement

check-format:
    cargo +nightly-2026-04-23 fmt --all -- --check

fix-format:
    cargo +nightly-2026-04-23 fmt --all

check-clippy:
    cargo clippy --all-targets -- -D warnings

fix-clippy:
    cargo clippy --all-targets --fix --allow-dirty --allow-staged

build:
    cargo build --release

clean:
    cargo clean

