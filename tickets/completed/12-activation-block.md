# 12 — Config address and deployed-block constants

**Blocks on:** 05

## Files
- `crates/types/constants/src/lib.rs` — statics, `AngstromAddressBuilder`,
  `AngstromAddressConfig`, `init`, `try_init`

## Goal
Name both config constants consistently and give them safe pre-deployment defaults.

## Do
- Rename `ANGSTROM_PROTOCOL_FEE_CONFIG_ADDRESS` to `PROTOCOL_FEE_CONFIG_ADDRESS`.
- Add `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK: OnceLock<u64>` beside it, plus the matching
  `protocol_fee_config_deployed_block` field, `with_` setter, and `init` / `try_init` wiring,
  following `ANGSTROM_DEPLOYED_BLOCK`.
- Default both to `Address::ZERO` and `0` on every network until the contract is deployed —
  `INTERNAL_TESTNET` already does this for the address.
- Fix the rename's call sites.

## Done when
- Both constants resolve per network at their defaults, and the workspace builds.

## Notes
This is the activation block **A**: the contract must exist in canonical state at **A-1**, so
deployment block and activation are the same value. A node holding the zero address or block `0`
has no config and must not build affected bundles — see ticket 17. Real values land in ticket 45.
