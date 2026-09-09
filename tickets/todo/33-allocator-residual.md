# 33 — Allocator returns its unallocated remainder

**Blocks on:** 29

## Files
- `crates/types/src/uni_structure/pool_swap.rs` — `t0_donation_vec`

## Goal
Make rounding visible instead of implicit.

## Do
- Have the donation allocator return its residual alongside the vector
  (`crates/types/src/uni_structure/pool_swap.rs`).

## Done when
- Every allocation reports what it did not place.
