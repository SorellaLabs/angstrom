# 35 — No-op allocation semantics

**Blocks on:** 33

## Files
- `crates/types/src/uni_structure/pool_swap.rs`
- `crates/types/src/traits/bundles.rs` — the `solution.ucp.is_zero()` branch

## Goal
Stop no-ops from silently skipping allocation.

## Do
- A true no-op (zero deltas, unchanged price and tick) allocates its whole budget to the active
  range at that source's end state, with zero residual, and fails if that range has no liquidity.
- A book no-op after a ToB swap uses the post-ToB state.
- A moving swap with missing range metadata is an error, not a no-op.
- `Some(empty)` must not silently skip allocation.

## Done when
- Each case above is covered and fails the suite when violated.
