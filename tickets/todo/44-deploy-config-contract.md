# 44 — Deploy AngstromProtocolFeeConfig

**Blocks on:** 10, 11

## Files
- `contracts/script/AngstromProtocolFeeConfig.s.sol` — from ticket 11

## Goal
Get the contract on chain at the existing economics.

## Do
- Deploy `AngstromProtocolFeeConfig(existingAngstrom, 750_000, 1_000_000)` against the existing
  Angstrom address.
- Verify on the explorer.
- Record the deployed address and its deployment block.

## Done when
- Runtime code, `angstrom()`, `controller()`, resolved owner and fast owner, initial values, and
  getter / slot-0 agreement all check out against the live deployment.

## Notes
Deploying at `(750_000, 1_000_000)` preserves current economics exactly. Nothing reads the contract
until ticket 45 sets the constants.
