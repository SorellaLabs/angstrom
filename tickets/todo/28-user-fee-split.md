# 28 — Integer user-fee split

**Blocks on:** 27

## Files
- `crates/types/src/traits/bundles.rs:408`

## Goal
Replace the `f64` split.

## Do
- `bundles.rs:408`: `(total_user_fees as f64 * LP_DONATION_SPLIT) as u128` becomes
  `splits.split_user(total_user_fees)`.
- Book donations stay on `solution.reward_t0 + lp_user_fees`.

## Done when
- Bundles at 75% match the old ones except for the documented unit-level rounding change.
