# 04 — Decode both shares from slot 0

**Blocks on:** 03

## Files
- `crates/types/primitives/src/primitive/protocol_fees.rs`

## Goal
One storage read yields a consistent pair.

## Do
- `DonationSplits::from_slot0(word: U256)`.
- `user = word & 0xffff_ffff`, `tob = (word >> 32) & 0xffff_ffff`.
- Reject nonzero padding above bit 63, then route through `new()` for bounds.

## Done when
- Decodes the exact word the deployed contract holds at `(750_000, 1_000_000)`.
- Rejects padding at bit 64 and `U256::MAX`.
- Rejects out-of-range shares in either half.

## Notes
A zero word decodes to a valid 0/0 pair, so this cannot distinguish a deliberate 0% config from an
empty account. Code validation is ticket 13's job.
