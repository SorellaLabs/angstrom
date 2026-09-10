# 39 — Coverage that must not be dropped

**Blocks on:** 30, 35

## Files
- `crates/types/src/traits/bundles.rs`
- `crates/types/src/uni_structure/pool_swap.rs`

## Goal
Keep the awkward cases tested.

## Do
- Empty donation vectors and true no-ops, with and without active liquidity — assert the budget is
  retained and accounted, not silently dropped.
- Book-only exact-match batches with positive user fees.
- Book no-ops after a ToB move.
- Zero budgets that still carry swap metadata.
- `Some(empty)`.
- Two pools sharing token0 — assert per-pool application and checked accumulation.

## Done when
- Each case is a named test, and conservation holds including the retained amount with no double
  counting.
