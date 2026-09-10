# 45 — Set the deployed config constants

**Blocks on:** 12, 44

## Files
- `crates/types/constants/src/lib.rs`

## Goal
Point nodes at the live contract.

## Do
- Replace the `Address::ZERO` / `0` defaults with the deployed address and deployment block from
  ticket 44, per network.

## Done when
- Nodes read config from the deployed contract, and require it to exist in canonical state at
  the block before activation.

## Notes
This is rollout step 3-4. Ship it separately from any rate change: step 5 enables a nonzero ToB
share via the setter, and combining them would make a discrepancy ambiguous between the code change
and the economic change.
