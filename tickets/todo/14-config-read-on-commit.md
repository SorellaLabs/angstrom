# 14 — Apply config changes from logs

**Blocks on:** 07

## Files
- `crates/eth/src/manager.rs:209` — `apply_periphery_logs`
- `crates/eth/src/manager.rs:345` — `get_protocol_config_update`, to be folded in and deleted

## Goal
Maintain the config from logs, exactly as `pool_store` is maintained.

## Do
- Handle `LpDonationSplitsSet` inside `apply_periphery_logs`, beside `NodeAdded` and
  `PoolConfigured`, filtered on the config address. Delete the separate
  `get_protocol_config_update` and its call sites in `handle_commit` / `handle_reorg`.
- Process **every block in the notification**, not only the tip. `apply_periphery_logs` and
  `get_protocol_config_update` both scan `receipts_by_block_hash(chain.tip_hash())` today, so a
  change in a non-tip block is silently missed.
- Apply in block order; the last update in the notification wins.
- Publish `EthEvent::ProtocolFeeConfigUpdated` with the notification tip's number and hash.

## Done when
- A config change in any block of a multi-block commit is applied.
- No provider call happens anywhere in the cleanser.
