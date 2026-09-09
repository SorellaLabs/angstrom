# 21 — Carry parent hash on bundle validation

**Blocks on:** 19

## Files
- `crates/validation/src/bundle/validator.rs` — `ValidationRequest::Bundle`
- `crates/types/src/reth_db_wrapper.rs`

## Goal
Simulate at H, execute at H+1, and say which H.

## Do
- `crates/validation/src/bundle/validator.rs`: add the parent hash to
  `ValidationRequest::Bundle`.
- Simulate at that state, execute in an H+1 environment, and return the identity with the result.
- Every read, including cache misses, resolves against the requested parent hash.

## Done when
- A result carries the parent it was produced against.
- A request for an unavailable parent errors.
