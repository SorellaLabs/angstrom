# 34 — Conservation per source

**Blocks on:** 33, 30

## Files
- `crates/types/src/traits/bundles.rs`
- `crates/types/src/uni_structure/pool_swap.rs`

## Goal
Prove nothing is created or lost.

## Do
- Per source: `sum(donations) + residual == budget`.
- Then `encoded_tob_donation + tob_protocol_fee + tob_residual == gross_tob_reward`.
- Reject over-allocation, malformed ranges, and unexplained remainders of any size. No "material"
  threshold.

## Done when
- All quantities nonnegative and residuals attributable to documented allocation steps.
