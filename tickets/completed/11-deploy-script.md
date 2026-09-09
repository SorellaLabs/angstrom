# 11 — Deploy script

**Blocks on:** 01

## Files
- `contracts/script/AngstromProtocolFeeConfig.s.sol` (new)
- `contracts/script/BaseScript.sol` — existing base

## Goal
Deploy standalone against the existing Angstrom address.

## Do
- Forge script deploying `AngstromProtocolFeeConfig(existingAngstrom, 750_000, 1_000_000)`.
- Post-deploy assertions: runtime code, `angstrom()`, `controller()`, resolved owner and fast
  owner, initial values, getter and slot-0 agreement.

## Done when
- Script runs against a fork and prints the verified values.
