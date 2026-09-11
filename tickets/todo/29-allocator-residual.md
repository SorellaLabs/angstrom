# 29 — Allocator returns its unallocated remainder

**Blocks on:** 26

## Files
- `crates/types/src/uni_structure/pool_swap.rs:237` — `t0_donation_vec`
- `crates/types/src/uni_structure/donation.rs` — home for the residual type
- `crates/types/src/traits/bundles.rs:427,429` — the two call sites

## Goal
Make unplaced budget visible instead of implicit.

## Do

1. Add the residual type beside `DonationCalculation` in `donation.rs`:

```rust
/// What `t0_donation_vec` was handed but did not place, by reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DonationResidual {
    /// Left over because each range's target is computed by integer division.
    pub rounding: u128,
    /// Budget the allocator had no range to place into at all.
    pub unplaced: u128
}

impl DonationResidual {
    pub fn total(&self) -> u128 { self.rounding + self.unplaced }
}
```

2. Change the signature at `:237` to
   `pub fn t0_donation_vec(&self, total_donation: u128) -> (Vec<DonationType>, DonationResidual)`.

3. Three exits, three residuals:

- `:239`, `self.steps.is_empty()` — return
  `(vec![], DonationResidual { rounding: 0, unplaced: total_donation })`. Today this drops the
  whole budget on the floor with no record; that is ticket 31's case and this is where it becomes
  visible.
- `filled_price == None` (`:310`, empty blob) — every range's donation is `0`, so the loop places
  nothing. Report the whole budget as `unplaced`.
- Otherwise — the `remaining_donation` still standing after the `:335` loop is `rounding`.

4. Update both call sites in `bundles.rs` to destructure the pair. Ticket 30 consumes the
   residuals; for this ticket, binding them `_book_residual` / `_tob_residual` is enough to keep
   `-D warnings` quiet.

## Done when
- Every allocation reports what it did not place, in which bucket.
- `sum(donations) + residual.total() == total_donation` at every exit.

## Notes
Reporting only. Allocation policy is unchanged — no logic is added to exhaust the LP budget.

The distinction is by *exit*, not by inspecting the numbers: an allocator that never ran reports
`unplaced`, and one that ran but could not place the last few units reports `rounding`. Keeping
them apart is what lets ticket 38 treat a rounding remainder as expected and an unplaced budget as
worth reading, without a threshold.

`remaining_donation` is reused as a loop variable twice — set at `:250` for the blob pass and
reset at `:313` for the distribution pass. Only the second value is the residual; read it after
the `:315` map completes, not the first.
