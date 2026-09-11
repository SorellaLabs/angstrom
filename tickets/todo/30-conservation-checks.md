# 30 — Conservation per source

**Blocks on:** 29, 27

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
