# 09 — Route the event conversion through new()

**Blocks on:** 03, 02

## Files
- `crates/types/primitives/src/primitive/protocol_fees.rs` — `From<LpDonationSplitsSet>`
- `crates/eth/src/manager.rs` — the call site

## Goal
Keep `new()` the only construction path, so bounds cannot be skipped.

## Do
- `impl From<LpDonationSplitsSet> for DonationSplits` in
  `crates/types/primitives/src/primitive/protocol_fees.rs` currently builds the struct directly and
  skips validation.
- Keep it `From`, not `TryFrom`. Delegate to `new()` and unwrap the result with
  `.expect("this is not possible - verification is done onchain")`.
- The values come off chain from `AngstromProtocolFeeConfig`, which already rejects anything above
  `1_000_000`, so the expect asserts an invariant the contract enforces rather than handling a case
  that can occur. A panic here means a misconfigured address or a stale ABI, not bad input.
- The eth manager filters logs by the config address, so a same-signature event from another
  contract cannot reach this conversion.

## Done when
- No path constructs `DonationSplits` without going through `new()`.
- The conversion stays infallible at its call site in `crates/eth/src/manager.rs`.
