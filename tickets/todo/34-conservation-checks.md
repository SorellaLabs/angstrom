# 34 — Conservation per source

**Blocks on:** 33, 30

## Files
- `crates/types/src/traits/bundles.rs`
- `crates/types/src/uni_structure/pool_swap.rs`

## Goal
Prove nothing is created or lost.

## Do
- Per source: `sum(donations) + remainder == budget`.
- Then `encoded_tob_donation + tob_protocol_fee + tob_remainder == gross_tob_reward`.
- Account rounding, retained remainder, and the configured fee separately from each other.
- Reject over-allocation, malformed ranges, and unexplained discrepancies of any size. No
  "material" threshold.

## Done when
- All quantities nonnegative, each bucket attributed to a documented allocation step.
- The retained remainder is counted exactly once — it reaches `save` via `collect_extra`, not
  `save_amount`.
