# 35 — Coverage that must not be dropped

**Blocks on:** 27, 31

## Files
- `crates/types/src/traits/bundles.rs` — `process_solution` under test
- `crates/types/src/uni_structure/pool_swap.rs` — `t0_donation_vec` under test

## Goal
Keep the awkward cases tested.

## Do

One named test per case. Each asserts ticket 30's conservation identity *and* which bucket the
budget landed in — a test that only checks the total passes when retention is misfiled as rounding.

| test | case | asserts |
| --- | --- | --- |
| `empty_donation_vec_retains_its_budget` | `steps.is_empty()`, active liquidity | `unplaced == budget`, vec empty |
| `true_noop_without_liquidity_retains_its_budget` | no active liquidity | same, and no panic |
| `book_only_exact_match_with_user_fees` | `searcher == None`, `total_user_fees > 0` | ToB buckets all zero, user split applied |
| `book_noop_after_tob_move` | ToB swap then `ucp.is_zero()` | book budget retained; post-ToB price used |
| `zero_budget_with_swap_metadata` | budget `0`, steps present | empty vec, all buckets zero |
| `some_empty_still_allocates` | `Some(vec![])` | allocator entered, budget reported not dropped |
| `two_pools_sharing_token0` | two `PoolSolution`s, same t0 | per-pool split, `save` accumulates checked |

`two_pools_sharing_token0` is the one that catches a split applied after cross-pool aggregation:
run two pools with different gross ToB and assert each got its own share, and that the t0 `save`
is the checked sum of both rather than a single split of the total.

## Done when
- Each case is a named test, and conservation holds including the retained amount with no double
  counting.

## Notes
`crates/types` has no `mod tests` in `bundles.rs` today and its only integration test
(`tests/angstrom.rs`) is an `#[ignore]`d stale fixture, so this ticket lands the first real
coverage of `process_solution`. Build the solutions programmatically — a base64 blob is what went
stale last time.

`book_noop_after_tob_move` is the case that would catch ticket 26's error arm regressing: if a
failed ToB ever returns to yielding `None`, `post_tob_price` silently reverts to the pre-ToB price
and this test's book state is wrong.
