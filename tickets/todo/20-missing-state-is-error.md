# 20 — Unavailable state is an error, not a default

**Blocks on:** 19

## Files
- `crates/types/src/reth_db_wrapper.rs` — `storage_ref`, `basic_ref`, `code_by_hash_ref`, `block_hash_ref`

## Goal
Stop silent zeros standing in for missing state.

## Do
- `reth_db_wrapper.rs`: `storage_ref` returns `unwrap_or_default()`, so missing storage reads as
  zero. Surface it as an error instead.
- Same for the other accessors that default on absence, and the `.latest().unwrap()`.

## Done when
- A read against unavailable state errors rather than returning zero or falling back to current
  state.
