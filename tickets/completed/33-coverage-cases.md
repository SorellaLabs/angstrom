# 33 — Coverage that must not be dropped

**Blocks on:** 27, 31

## Overview
PLAN.md names a set of awkward cases that must not lose coverage, and this ticket turns each one
into a named test. It is also the first real coverage of `process_solution`: `bundles.rs` has no
`mod tests` today, and the crate's only integration test is an `#[ignore]`d fixture whose base64
blob went stale. Every test asserts ticket 30's conservation identity *and* which bucket the
budget landed in — a test that only checks the total still passes when retention is misfiled as
rounding. Build the solutions programmatically; a fixture is exactly what went stale last time.
`two_pools_sharing_token0` is the case that catches a split applied after cross-pool aggregation
rather than per pool, which is the mistake the arithmetic is most likely to make.

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

**As built.** All seven cases are named tests, split across the two files the ticket lists:
`process_solution` in `bundles.rs` for the three that are about the split, `t0_donation_vec` in
`pool_swap.rs` for the four that are about which residual bucket a budget lands in.

Two cases already had tests from ticket 31 and were left where they were.
`true_noop_without_liquidity_retains_its_budget` already carried the ticket's own name;
`empty_steps_retains_its_whole_budget_as_unplaced` is renamed to
`empty_donation_vec_retains_its_budget` to match the table, which makes ticket 31's coverage note
name-stale but nothing else.

`zero_budget_with_swap_metadata` asserts every donation is zero rather than "empty vec" as the
table says: `reduce_ranges` yields one range per batching step, so a swap that moved always comes
back with entries — they are all `0`, and both residual buckets are `0`. That is the same claim
the table is making; the vector is just not the thing that is empty.

**`process_solution` does not return residuals, so retention is asserted through conservation.**
`check_conservation` already runs inside `process_solution`, so a call that returns `Ok` with a
nonzero book budget and nothing placed *is* the assertion that the budget was reported rather than
dropped. `book_noop_after_tob_move` and `some_empty_still_allocates` are built on that, and the
mutation that reverts ticket 31 step 2 to `DonationResidual::default()` fails
`book_noop_after_tob_move` exactly as intended. `some_empty_still_allocates` adds a second,
independent signal: an entered allocator returns an empty `DonationCalculation` whose
`expected_liquidity` is `0`, where the skipped path would emit the pool's live liquidity and the
whole budget as the reward.

`book_only_exact_match_with_user_fees` is sensitive to the user split through `take`/`settle`, not
`save`. With no allocator range to place into, the whole fee is retained either way, so `save`
alone cannot tell 75% from 100%; the reserving `allocate` borrows exactly `user_protocol_fee` from
Uniswap, which does move with the rate.

`two_pools_sharing_token0` uses grosses of `1_001` and `2_002` at `tobLpShareE6 = 750_000`, chosen
so the per-pool fees (`251 + 501`) and a single split of the aggregate (`751`) differ by a unit. The
test asserts that gap is real before relying on it, so the case cannot silently stop discriminating.

Each new test was checked against a mutation: dropping the book no-op residual, short-circuiting the
empty-step allocator, handing the ToB allocator the gross, and removing the user split each fail
exactly the tests that own them.

**Unrelated finding: ticket 26 step 4 is not in the tree.** The `Err` arm of
`calc_vec_and_reward` at `bundles.rs:372` still logs and returns `None` rather than propagating,
though 26's notes record it as landed. `git log -S "return Err(error);"` finds no commit that ever
added it. Not fixed here — it is 26's behavior change, not 33's coverage — but it is exactly the
regression this ticket's `book_noop_after_tob_move` note anticipated.
