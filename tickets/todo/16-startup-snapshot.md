# 16 — Seed the config at init

**Blocks on:** 13, 12

## Files
- `bin/angstrom/src/components.rs:271` — beside the `AngstromPoolConfigStore::load_from_chain` call
- `crates/eth/src/manager.rs:61` — new field beside `pool_store`
- `crates/consensus/src/manager.rs` — initial value for the round state

## Goal
Give the cleanser and consensus a starting value before the first block.

## Do
- In `components.rs`, call `load_from_chain` at the init block and pass the result into
  `EthDataCleanser`, the same way `pool_store` is passed as `Arc<AngstromPoolConfigStore>`.
- Hold it on the cleanser as a field; log application (14, 15) mutates it from there.
- Seed consensus with the same starting value so the first round has rates before any
  `ProtocolFeeConfigUpdated` arrives.
- Read at the init block, and make sure the cleanser's first processed notification is the block
  after it, so no update is missed or applied twice.

## Done when
- A node started mid-chain has rates before its first round, with no gap or double-apply.
