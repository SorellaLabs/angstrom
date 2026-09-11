# 31 — Empty-vector and unallocated-budget retention

**Blocks on:** 29

## Overview
Several paths through the donation allocator retain budget for the protocol today without
recording that they did: an empty step vector, and the `ucp.is_zero()` book branch that never
hands its budget to an allocator at all. Ticket 29 already built the residual type that makes
retention reportable; this ticket is the one that says which cases must route through it, and
that the resulting retention is intended rather than an unremarked side effect. Almost no code
is unique to it — the book-side residual in step 2 and the two unwraps in step 3 are the whole
diff. Those unwraps are the one behavior change: a range that moved but carries no tick bound is
malformed input, and turning a panic into an error is not the same as accepting it. Allocation
policy is explicitly unchanged, and an earlier requirement that a true no-op allocate its whole
budget to the active range is withdrawn rather than deferred.

## Files
- `crates/types/src/uni_structure/pool_swap.rs:239` — the `steps.is_empty()` exit
- `crates/types/src/uni_structure/pool_swap.rs:341,344` — `lower_tick` / `upper_tick` unwraps
- `crates/types/src/traits/bundles.rs:392` — the `solution.ucp.is_zero()` branch

## Goal
Keep today's behavior, but make it deliberate and accounted.

## Do

1. **`Some(empty)` must not skip allocation silently.** Ticket 29 already routes the
   `steps.is_empty()` exit at `:239` through `DonationResidual { unplaced: total_donation, .. }`,
   so the budget is reported rather than dropped. Confirm the call sites keep calling
   `t0_donation_vec` for an empty-step vec instead of short-circuiting around it — the reporting
   only happens if the allocator is actually entered.

2. **The `ucp.is_zero()` branch at `:392`** sets `book_swap_vec = None`, so
   `solution.reward_t0 + total_lp_user_donate` is never handed to an allocator at all. That budget
   stays in `contract_liquid` and `collect_extra` sweeps it into `save`. Keep that behavior, but
   record it: emit the amount as an `unplaced` residual for the book source so ticket 30's per-pool
   check still balances and ticket 36 can report it.

3. **A moving swap with missing range metadata is an error.** `:341` and `:344` call
   `r.lower_tick.unwrap()` / `r.upper_tick.unwrap()` on a non-final range. A range that moved but
   carries no tick bound is malformed, not a no-op — replace both with a `bail!` naming the range
   index. This is the one behavior change in this ticket, and it converts a panic into an error
   rather than accepting bad input.

4. Leave every path that already allocates to LPs alone. No redistribution logic, no attempt to
   exhaust the budget.

## Done when
- Retention is reported as its own bucket, not folded into rounding or the configured fee.
- Malformed metadata still fails.
- A true no-op with no active liquidity retains and reports its budget rather than panicking.

## Notes
Supersedes the earlier requirement that a true no-op allocate its whole budget to the active range
and fail without liquidity. That is withdrawn: no redistribution logic is added.

This ticket adds almost no code of its own — ticket 29's residual is the mechanism, and this is the
ticket that says which cases must route through it and that the resulting retention is intended.
The only edits unique to it are step 2's book-side residual and step 3's two unwraps.

A book no-op after a ToB swap uses the post-ToB state: `post_tob_price` at `:384` is already the
ToB swap's end price, and the `ucp.is_zero()` branch is below it, so this holds by construction.
Ticket 35 asserts it rather than anything needing to change here.

**As built.** Steps 1, 2 and 4 landed as written. Step 3 landed as written but is unreachable —
see below.

Step 1 needed no change, as the ticket anticipated: neither call site inspects `steps` before
calling, so a `Some(empty)` swap vec enters `t0_donation_vec` and comes back out as
`unplaced == budget` rather than being short-circuited around.

Step 2 is the ticket's one real edit. The `ucp.is_zero()` arm now returns
`DonationResidual { rounding: 0, unplaced: book_budget }` instead of `DonationResidual::default()`,
so ticket 30's per-pool book check balances on a branch that never reaches an allocator. Behavior
is unchanged: the budget still stays in `contract_liquid` and `collect_extra` still sweeps it into
`save`. `total_donation`'s `unwrap_or` fallback is untouched apart from reading the same
`book_budget` binding.

**Step 3's `bail!`s are defensive, not reachable.** `reduce_ranges` only ends a batch early when
`init` is true (`while !acc.4`), so `final_tick` is `Some` for every range except the last one —
and the last range takes the `DonationType::current` arm, which reads no bound. A non-final range
with a missing bound therefore cannot be constructed through this function. The unwraps are
converted to `bail!`s naming the range index as specified, but there is no test, because
`TickInterval` and `reduce_ranges` are both private and the input cannot be built. Recorded rather
than faked, the same way ticket 29 recorded the unreachable `filled_price == None` arm. If a seam
ever makes it reachable, "malformed metadata still fails" becomes testable then.

Coverage: `empty_steps_retains_its_whole_budget_as_unplaced` asserts retention lands in its own
bucket rather than folded into rounding, and `true_noop_without_liquidity_retains_its_budget`
covers the no-liquidity case without a panic. The `ucp.is_zero()` book branch needs a
`PoolSolution` to drive, so its assertion belongs to ticket 35's `book_noop_after_tob_move`.
