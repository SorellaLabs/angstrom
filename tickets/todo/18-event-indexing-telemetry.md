# 18 — Record config changes for operators

**Blocks on:** 14, 15

## Files
- `crates/eth/src/telemetry.rs`
- `crates/telemetry-recorder/src/lib.rs`

## Goal
Operator-visible change history.

## Do
- On each applied or inverted change, record the old pair, the new pair, and the block identity.
- Mark reorg inversions as such so history does not read as a governance action that never
  happened.

## Done when
- Every rate change the node acted on is visible with its block, including inversions.
