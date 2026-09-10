# 36 — Builder bundles against unchanged Angstrom

**Blocks on:** 30

## Files
- `testing-tools/src/contracts/`
- `testing-tools/src/types/initial_state.rs`
- `crates/types/src/traits/bundles.rs` — code under test

## Goal
Prove settlement against real contracts, not fixtures.

## Do
- Execute builder-produced bundles in the Anvil harness against unchanged Angstrom.
- Assert exact `save`, zero unresolved deltas, expected reward growth.

## Done when
- Hand-written fixtures are not what proves this.
