# 15 — Read config on reorg

**Blocks on:** 14

## Files
- `crates/eth/src/manager.rs` — `handle_reorg`, the `todo!()` branch

## Goal
Remove the `todo!()` and handle a reorg that only removes a setter.

## Do
- In `handle_reorg`, read storage at the new head and publish with that identity.
- Delete the `else if let Some(...) = self.get_protocol_config_update(&old) { todo!() }` branch —
  it panics today.

## Done when
- A reorg that removes a setter transaction publishes the reverted rates.
- No panic path remains in reorg handling.
