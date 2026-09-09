# 14 — Read config on canonical commit

**Blocks on:** 13, 07

## Files
- `crates/eth/src/manager.rs` — `handle_commit`, `get_protocol_config_update`
- `crates/eth/src/protocol_fee_config.rs` — created by ticket 13

## Goal
Replace the event-derived config with a storage read.

## Do
- In `handle_commit` (`crates/eth/src/manager.rs`), drop `get_protocol_config_update`'s log scan as
  the config source and read storage at the new head instead.
- Publish `ProtocolFeeConfigUpdated` with that block's identity.
- The read must complete before the block update is released — cleanser callbacks are synchronous,
  so it participates in block synchronization rather than running detached.

## Done when
- A commit that contains no setter transaction still publishes the current rates.
