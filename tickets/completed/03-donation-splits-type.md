# 03 — DonationSplits type and split arithmetic

**Blocks on:** —

## Files
- `crates/types/primitives/src/primitive/protocol_fees.rs` (new)
- `crates/types/primitives/src/primitive/mod.rs` — module wiring

## Goal
Replace `f64` scaling with exact integer splits.

## Do
- `crates/types/primitives/src/primitive/protocol_fees.rs`.
- `DonationSplits { user_lp_share_e6, tob_lp_share_e6 }`, private fields, `DENOM = 1_000_000`.
- `new()` as the only constructor, rejecting either share above `DENOM`.
- `split(gross, share_e6)` in `U256`, returning `(lp, gross - lp)` so LP rounds down and protocol
  takes the exact remainder.
- `split_user` / `split_tob`.

## Done when
- `lp + protocol == gross` holds at `u128::MAX`.
- 75% of 7 is `(5, 2)`.
- 0% and 100% behave at both extremes.
