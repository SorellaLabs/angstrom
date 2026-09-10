# 23 — One snapshot per round

**Blocks on:** 16

## Files
- `crates/consensus/src/manager.rs:129` — `on_blockchain_state`
- `crates/consensus/src/rounds/mod.rs:190` — `SharedRoundState`
- `crates/consensus/src/rounds/mod.rs:277` — `matching_engine_output`

## Goal
Hold the current rates in memory and fix them for the whole round.

## Do
- Hold the latest `DonationSplitSnapshot` on `SharedRoundState`, seeded at init (16) and updated
  from `EthEvent::ProtocolFeeConfigUpdated` in `on_blockchain_state`, beside the existing
  `NewBlock` handling.
- Capture it once per round in `matching_engine_output`, next to the existing
  `let pool_snapshots = self.fetch_pool_snapshot();` at `:336`. That call site is above both
  consumers, so one capture covers gas estimation and final construction.
- No provider call on this path. Block sync already guarantees the cleanser has applied the
  block's logs before the round runs.

## Done when
- Gas estimation and final construction use the same value.
- An update arriving mid-round does not change the bundle being built.
