# 08 — Fix inverted error messages in DonationSplits::new

**Blocks on:** 03

## Files
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` — `DonationSplits::new`

## Goal
Say what the guard actually enforces.

## Do
- `protocol_fees.rs`: messages read "must be greater than `1_000_000`" but the guard is `> DENOM`.
  Change to "must be at most `1_000_000`" (or similar) for both shares.

## Done when
- Message text matches the condition.
