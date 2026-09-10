# 29 — Apply the ToB split before the donation merge

**Blocks on:** 28

## Files
- `crates/types/src/traits/bundles.rs` — around `tob_donation_vec`
- `crates/types/src/traits/tob.rs` — `calc_vec_and_reward`, unchanged
- `crates/types/src/uni_structure/pool_swap.rs` — `t0_donation_vec`

## Goal
Give the protocol a share of ToB surplus.

## Do
- After `calc_vec_and_reward` returns the gross ToB payment and before building the donation
  vector, `splits.split_tob(gross)`.
- `tob_vec.t0_donation_vec(tob_lp_budget)` instead of the full gross.
- Apply once per pool to that pool's gross total — never per tick, per fragment, or after assets
  are aggregated across pools.
- Leave `calc_vec_and_reward`, `calc_reward`, bid ranking, swap quantities and the post-ToB price
  alone. Ranking stays on gross.

## Done when
- No ToB order means both values are zero.
- A selected ToB order that fails to evaluate is an error, not zero revenue.
- The share touches only ToB surplus — not user fees, book surplus, gas, or unlocked-swap fees.
