# 05 — Config address in the address config

**Blocks on:** —

## Files
- `crates/types/constants/src/lib.rs`

## Goal
Let each network name its deployed config contract.

## Do
- `ANGSTROM_PROTOCOL_FEE_CONFIG_ADDRESS` in `crates/types/constants/src/lib.rs`.
- `protocol_fee_config_address` on `AngstromAddressBuilder` / `AngstromAddressConfig` with a
  `with_protocol_fee_config` setter.

## Done when
- Address resolves per network alongside the existing addresses.

## Notes
Rates are never constants. Activation block **A** is ticket 12.
