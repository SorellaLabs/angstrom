# 49 — Attribute capacity-limited remainder separately from rounding

**Blocks on:** —
**Closes:** ISSUES.md 8 (PR #680 A.7)
**Follows:** 29, 30, 40, 41

## Overview
`t0_donation_vec` reports every nonempty allocation's leftover as `rounding`. In the upward
direction that is wrong: the blob pass saturates `c_t0` to `1` (`pool_swap.rs:304-309`), so the
allocator can refund at most `d_t0 − 1` per range whatever the budget, and the distribution pass
places exactly that. With token0 out 100, token1 in 102 and a 5,000 budget, 99 is placed and
4,901 is left — and labelled "what integer division left behind". It is the allocator's capacity
limit. Ticket 29's two buckets name "never ran" (`unplaced`) and "ran but could not place the last
units" (`rounding`); a capacity cap is a third thing the design has no word for. The reviewer's
counterexample reproduces by hand and is reachable on every upward swap whose budget exceeds
`d_t0 − 1`. No change to the accepted allocation policy — only the attribution.

## Files
- `crates/types/src/uni_structure/donation.rs:81-93` — `DonationResidual`, now with a checked `total()`
- `crates/types/src/uni_structure/pool_swap.rs:296-370` — `t0_donation_vec`, both passes and the residual
- `crates/types/src/uni_structure/donation.rs:327` — `check_conservation`, the consumer
- `crates/types/src/traits/bundles.rs:427-460` — the per-pool checks, unchanged in shape

## Goal
The residual says *why* budget was not placed, for all three reasons.

## Do
1. Add a third bucket to `DonationResidual` — `capacity: u128` — for budget the allocator could not
   place because of its per-range limit, distinct from `rounding` (integer division) and `unplaced`
   (no allocator ran). `total()` sums all three, checked (ticket 41 already made it `Option`).
2. In `t0_donation_vec`, attribute by *exit and direction*, not by inspecting numbers:
   - Upward (`!direction`): the amount by which the blob pass saturated
     (`remaining_donation − (c_t0_before − 1)` when `saturating_sub` bottomed out) is `capacity`;
     what the distribution pass then leaves is `rounding`.
   - Downward (`direction`): the blob absorbs the whole remainder, so the distribution pass's
     leftover is genuinely `rounding`, as today.
   - Empty steps / empty blob: `unplaced`, as today.
3. Keep every conservation check an equality over the new three-way sum.
4. Test with the reviewer's exact counterexample and assert `capacity == 4901`, `rounding == 0`,
   `unplaced == 0`, placed `== 99`. Add the downward mirror and assert `capacity == 0`.

## Done when
- The upward counterexample reports its leftover as `capacity`, not `rounding`.
- A downward swap with the same budget reports `capacity == 0`.
- Every existing conservation test still passes with the identity extended to three buckets.
- Allocation policy, `save_amount`, and the money's path are unchanged — only the ledger.

## Notes
PLAN.md, Conservation: remainders "split into two buckets accounted separately from each other and
from the configured protocol fee: integer-allocation rounding, and budget the allocator did not
place." A capacity-limited remainder is budget the allocator did not place; PLAN.md's two names
just did not anticipate a third reason. This adds the name, not a policy.

Why it matters beyond tidiness: the residual's intended consumer is the step-5 reconciliation
component, which needs to tell "expected, small, arithmetic" from "large, structural, worth
reading". Today a 98% capacity remainder and a 1-unit division remainder land in the same bucket.

Tickets 40 and 41 are complete (`61af55f3`): the `(None, None)` mis-attribution is fixed at the
call site and `total()` is checked. This is the last mis-bucketing in the family and it lives inside
`t0_donation_vec`, which is why it gets its own bucket rather than a call-site fix.
