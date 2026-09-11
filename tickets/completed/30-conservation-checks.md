# 30 — Conservation per source

**Blocks on:** 29, 27

## Overview
Tickets 26, 27 and 29 split each pool's gross into three destinations — LP donations, the
configured protocol fee, and whatever the allocator could not place — but nothing yet proves the
pieces add back up. This ticket adds equality checks at two levels: inside `t0_donation_vec`,
which owns `placed + residual == budget`, and inside `process_solution`, where ToB gross must
account three ways and the book budget two. Making the allocator fallible is the structural
change; over-allocation then fails the same check without needing a branch of its own. The
subtlety worth holding onto is that residuals reach `save` through `collect_extra` and must
never be added to `save_amount`, or the same amount is both reserved and swept. Every check is
an equality — PLAN.md rules out a "material" threshold anywhere.

## Files
- `crates/types/src/uni_structure/pool_swap.rs:237` — `t0_donation_vec`, per-source check
- `crates/types/src/traits/bundles.rs:427-447` — per-pool check across both sources

## Goal
Prove nothing is created or lost.

## Do

1. **Per source, inside `t0_donation_vec`.** Before returning, assert the identity the function
   is responsible for:

```rust
let placed: u128 = donations.iter().map(|d| d.donation()).sum();
let accounted = placed
    .checked_add(residual.total())
    .ok_or_else(|| eyre::eyre!("donation accounting overflowed"))?;
if accounted != total_donation {
    eyre::bail!("allocator placed {placed} + residual {} != budget {total_donation}",
                residual.total());
}
```

   This makes the return type `eyre::Result<(Vec<DonationType>, DonationResidual)>`. A `placed`
   above `total_donation` fails the same check — over-allocation needs no separate branch.

2. **Per pool, in `process_solution`,** after the merge at `:435` and with both residuals from
   ticket 29 in scope:

```rust
// ToB: gross splits three ways and nothing else.
let tob_placed = tob_donation_vec.as_ref().map(sum_donations).unwrap_or(0);
let gross_tob = tob_swap_info.as_ref().map(|(_, g)| *g).unwrap_or(0);
if tob_placed + tob_protocol_fee + tob_residual.total() != gross_tob {
    eyre::bail!(...);
}

// Book: the budget it was handed is reward_t0 + the LP share of user fees.
let book_budget = solution.reward_t0 + total_lp_user_donate;
if book_placed + book_residual.total() != book_budget {
    eyre::bail!(...);
}
```

3. Use checked arithmetic in the sums — a `u128` overflow must fail the check, not wrap into
   agreement.

## Done when
- All quantities nonnegative, each bucket attributed to a documented allocation step.
- The retained remainder is counted exactly once — it reaches `save` via `collect_extra`, not
  `save_amount`.
- A deliberately inflated donation vector fails the per-source check.

## Notes
`u128` is unsigned, so "all quantities nonnegative" is a type property, not a runtime one. What
needs asserting is the *sum*, which is what step 1 and 2 do.

Four buckets, kept separate and each attributable:

| bucket | where it goes | who owns it |
| --- | --- | --- |
| placed donations | `RewardsUpdate` | LPs |
| configured fee | `save_amount` → `save` (27) | protocol |
| rounding residual | `contract_liquid` → `collect_extra` → `save` (29) | protocol |
| unplaced residual | same path as rounding (29, 31) | protocol |

The last two reach `save` without passing through `save_amount`. That is the "counted exactly
once" requirement: adding a residual to `save_amount` would both reserve it *and* let
`collect_extra` sweep it, double counting. Ticket 27's Notes has the mechanism.

No "material" threshold anywhere — the checks are equality, and any discrepancy fails the bundle.

**As built.** All three steps landed. The one structural deviation: step 1's per-source check and
step 2's two per-pool checks are the same equality with a different number of buckets, so they are
one shared `check_conservation(source, placed, protocol_fee, residual, gross)` in `donation.rs`
rather than three hand-rolled blocks. A two-way source passes a fee of `0`. `sum_donations` sits
beside it — the name ticket 30's own snippet used — and folds with `checked_add` rather than
`.sum()`, per step 3; `check_conservation` chains `checked_add` the same way, so an overflow fails
the check instead of wrapping into agreement with it.

`t0_donation_vec` is now `eyre::Result<(Vec<DonationType>, DonationResidual)>`. Its two call sites
in `bundles.rs` moved from `.map(..).unwrap_or(..)` to `match`, because `?` cannot propagate out of
a closure.

The per-pool checks run just before the donation merge rather than just after. The merge consumes
both vectors by value, and it does not change what was placed, so the assertion is identical and
this is only where the values are still in scope.

`book_budget` is now a named binding built with `checked_add`, replacing the unchecked
`solution.reward_t0 + total_lp_user_donate` in both the allocator call and `total_donation`'s
`unwrap_or` fallback.

Coverage: `an_inflated_donation_vector_fails_the_check` is the "Done when" bullet — one more unit
placed than was available fails the same equality, with no branch of its own.
`the_configured_fee_is_its_own_bucket` asserts the three-way ToB shape and that misfiling the fee
does not balance. `sums_overflow_rather_than_wrapping_into_agreement` covers step 3.
`allocation_conserves_its_budget` drives a real swap through `t0_donation_vec` across five budgets.
The per-pool checks in `process_solution` are wired but not yet driven by a test — `crates/types`
has no way to build a `PoolSolution` programmatically today, which is ticket 35's job.
