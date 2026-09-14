# 41 — Make DonationResidual::total() checked

**Blocks on:** —
**Closes:** ISSUES.md 5
**Follows:** 29, 30

## Files
- `crates/types/src/uni_structure/donation.rs:88-92` — `DonationResidual::total`
- `crates/types/src/uni_structure/donation.rs:327` — `check_conservation`, the caller

## Goal
No addition in the conservation path can wrap into agreement with the check.

## Do
- `total()` is `self.rounding + self.unplaced` — unchecked. Return `Option<u128>` via `checked_add`,
  or have `check_conservation` fold the two buckets into its own `checked_add` chain rather than
  calling `total()`.
- Keep the error message naming the residual total, so a failure still reads the way ticket 30's
  does.

## Done when
- Every addition reachable from `check_conservation` is checked.
- A residual whose two buckets sum past `u128::MAX` fails the check instead of wrapping.

## Notes
Ticket 30 step 3 says "Use checked arithmetic in the sums — a `u128` overflow must fail the check,
not wrap into agreement." `check_conservation` honours that for `placed`, `protocol_fee` and the
residual total it is handed, and `sum_donations` folds with `checked_add` — but the one addition
inside `total()` slipped through.

**Not reachable as constructed.** Every exit in `t0_donation_vec` sets exactly one bucket nonzero
(`{rounding: 0, unplaced: total_donation}` at the two early exits, `{rounding: remaining, unplaced:
0}` at the normal one), and ticket 31's book arm sets `{rounding: 0, unplaced: book_budget}`. So one
of the two is always zero and the sum cannot overflow today.

Worth closing anyway: the entire point of the surrounding code is that an overflow must not wrap
into agreement with the equality that is supposed to catch it. A future exit that populates both
buckets — which is exactly what ticket 40 might introduce, or what a third residual category would —
would make this reachable, and it would fail silently by producing a total that balances.

`sum_donations` already has the shape to copy.
