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
them apart is what lets a reader treat a rounding remainder as expected and an unplaced budget as
worth reading, without a threshold. Reconciliation was the intended consumer; it is deferred to
the step-5 follow-up, and the distinction is still what makes the residual legible.

`remaining_donation` is reused as a loop variable twice — set at `:250` for the blob pass and
reset at `:313` for the distribution pass. Only the second value is the residual; read it after
the `:315` map completes, not the first.

**As built.** All four steps landed. Both call sites needed slightly more than a destructure: the
residual has to escape the closure, so the `Option` moved inside it. The book site gained the
`map` / `unwrap_or` shape the ToB site already had from ticket 26, and the ToB tuple widened from
two elements to three. Both residuals bind as `_book_residual` / `_tob_residual` for ticket 30.

The `filled_price == None` arm is unreachable today and is kept exactly as the ticket specifies.
`reduce_ranges` yields one range per `batching` step and the function has already returned on
`steps.is_empty()`, so `ranges` is non-empty wherever that arm could be reached, `current_blob` is
`Some`, and `filled_price` is `Some`. Were it ever `None`, `let last_range = ranges.len() - 1` at
`:318` would underflow first. Recorded rather than fixed — ticket 31 owns the malformed-metadata
conversions in this function.

The conservation identity holds at all three exits by construction, which is what gives ticket 30
something to assert rather than something to repair: the empty-steps exit places nothing and
reports the whole budget; the empty-blob arm reports the whole budget while every `donation` takes
the `else { 0 }` branch; and the normal exit's per-range `std::cmp::min(remaining_donation, ..)`
means `remaining_donation` falls by exactly what each range placed and cannot underflow.
