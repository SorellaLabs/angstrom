default:
    @just --list

ci: check-format check-clippy test test-integration

check: check-format check-clippy test

fix: fix-format fix-clippy

test:
    cargo nextest run --workspace --lib

test-integration:
    cargo nextest run --workspace --tests

check-format:
    cargo +nightly fmt --all -- --check

fix-format:
    cargo +nightly fmt --all

check-clippy:
    cargo clippy --all-targets -- -D warnings

fix-clippy:
    cargo clippy --all-targets --fix --allow-dirty --allow-staged

build:
    cargo build --release

clean:
    cargo clean

