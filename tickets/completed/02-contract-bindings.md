# 02 — Generate Rust bindings for the config contract

**Blocks on:** 01

## Files
- `crates/types/primitives/build.rs` — `WANTED_CONTRACTS`
- `crates/types/primitives/src/contract_bindings/mod.rs` — generated
- `abis-types/AngstromProtocolFeeConfig.sol/AngstromProtocolFeeConfig.json` — generated

## Goal
Make the contract's ABI available to the node.

## Do
- Add `AngstromProtocolFeeConfig.sol` to `WANTED_CONTRACTS` in `crates/types/primitives/build.rs`.
- Regenerate `crates/types/primitives/src/contract_bindings/mod.rs` and `abis-types/`.

## Done when
- `angstrom_protocol_fee_config::AngstromProtocolFeeConfig::LpDonationSplitsSet` resolves.
- Workspace builds.
