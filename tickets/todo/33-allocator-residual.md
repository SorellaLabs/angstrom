# 33 — Allocator returns its unallocated remainder

**Blocks on:** 29

## Files
- `crates/types/src/uni_structure/pool_swap.rs` — `t0_donation_vec`

## Goal
Make unplaced budget visible instead of implicit.

## Do
- Return the remainder alongside the vector.
- Split it into two buckets: integer-allocation rounding, and budget the allocator did not place.

## Done when
- Every allocation reports what it did not place, in which bucket.

## Notes
Reporting only. Allocation policy is unchanged — no logic is added to exhaust the LP budget.
