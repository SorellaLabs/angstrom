# 06 — DonationSplitSnapshot type

**Blocks on:** 03

## Files
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs`

## Goal
Bind a rate pair to the exact block it was read from.

## Do
- `DonationSplitSnapshot { block_number: u64, block_hash: B256, splits: DonationSplits }` in
  `protocol_fees.rs`, `Copy`.

## Done when
- Every consumer can carry the pair and its parent identity together.
